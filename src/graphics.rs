use crate::enemies::{CrabType, EnemyCrab};
use crate::{CRAB_SIZE, Flashlight, PLAYER_SIZE};
use crevice::std140::AsStd140;
use ggez::Context;
use ggez::glam::Vec2;
use ggez::graphics::{
    BlendMode, Canvas, Color, DrawMode, DrawParam, Image, Mesh, Rect, Shader, ShaderParamsBuilder,
};
use rand::Rng;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

// A single unit-radius circle mesh, built once and reused for every particle by
// scaling it via `DrawParam` instead of baking each particle's radius into fresh
// mesh geometry. Milestone fireworks alone can push 200+ live particles, each
// previously allocating two brand-new GPU mesh buffers every single frame.
static UNIT_CIRCLE: OnceLock<Mesh> = OnceLock::new();

// A single unit-length horizontal segment (a 1x1 rect centered on the x-axis),
// built once and reused for every rope/line segment by scaling it via `DrawParam`
// (scale.x = segment length, scale.y = thickness) and rotating it to match the
// segment's direction, instead of baking each segment's two endpoints into a
// fresh `Mesh::new_line` GPU buffer. The conga rope draws ~2 line segments per
// micro-subdivision per chain link (SEGS=14), so a long conga train — the whole
// point of this game — was allocating hundreds of GPU meshes every frame.
static UNIT_LINE: OnceLock<Mesh> = OnceLock::new();

// A unit square (1x1, top-left corner at the origin), built once and reused for every
// axis-aligned fill rectangle — level backgrounds, full-screen flashes, HUD/UI bars —
// via `DrawParam::dest`+`scale`, instead of a fresh `Mesh::new_rectangle` GPU buffer on
// every draw call. Several of these (the grass background, the stamina bar) get redrawn
// every single frame regardless of whether any effect is active.
static UNIT_SQUARE: OnceLock<Mesh> = OnceLock::new();

thread_local! {
    // Cache of stroke-circle meshes keyed by (radius, thickness) quantized to the nearest
    // pixel/quarter-pixel. Ring-style effects (beat ghost rings, catch shockwaves, attraction
    // glow) can't reuse a single unit-circle scaled via DrawParam like fill circles do, because
    // scaling a stroke ring scales its line thickness along with its radius, distorting the
    // taper these effects rely on. Instead we memoize the actual built mesh per rounded
    // (radius, thickness) pair. This matters most for beat ghost rings: every crab in the conga
    // chain gets a ring on each beat, and since they're all spawned in lockstep they share the
    // same age every frame, so in practice one cache entry is reused by every ring in the chain
    // instead of the whole chain rebuilding a fresh GPU mesh each frame.
    static STROKE_CIRCLE_CACHE: RefCell<HashMap<(i32, i32), Mesh>> = RefCell::new(HashMap::new());

    // Same idea as STROKE_CIRCLE_CACHE but for axis-aligned stroke rectangles (bar borders,
    // panel outlines). Bounded in practice: only a handful of distinct UI element sizes ever
    // get drawn, so this cache stays tiny for the life of the process.
    static STROKE_RECT_CACHE: RefCell<HashMap<(i32, i32, i32), Mesh>> = RefCell::new(HashMap::new());

    // Cache of partial-circle ("arc") stroke meshes, keyed by (radius, thickness, filled
    // segments out of a fixed 48-segment ring). Used by the King Crab health ring, which
    // otherwise rebuilt a fresh ~48-point Vec plus a fresh Mesh::new_line every single frame
    // for its whole (multi-second) time on screen. Bounded to at most a handful of live boss
    // radii times 49 possible fill levels, so this cache stays small.
    static STROKE_ARC_CACHE: RefCell<HashMap<(i32, i32, usize), Mesh>> = RefCell::new(HashMap::new());
}

/// Fetch a cached stroke-arc mesh spanning `filled` of `segs` segments of a circle of the given
/// `radius`/`thickness`, starting at the top and sweeping clockwise — the same shape
/// `draw_boss_health_ring`'s health arc needs, but built once per (radius, thickness, filled)
/// combo instead of allocating a fresh point Vec + GPU mesh every frame. Mesh is centered at the
/// origin in local space; draw with `.dest(pos)` only (no `.scale`, which would distort the
/// stroke thickness the same way it would for `cached_stroke_circle`).
fn cached_stroke_arc(
    ctx: &mut Context,
    radius: f32,
    thickness: f32,
    segs: usize,
    filled: usize,
) -> ggez::GameResult<Mesh> {
    let radius = radius.max(0.5);
    let thickness = thickness.max(0.25);
    let filled = filled.clamp(1, segs);
    let key = ((radius * 2.0).round() as i32, (thickness * 4.0).round() as i32, filled);

    if let Some(mesh) = STROKE_ARC_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return Ok(mesh);
    }

    let start = -std::f32::consts::FRAC_PI_2;
    let pts: Vec<[f32; 2]> = (0..=filled)
        .map(|i| {
            let a = start + (i as f32 / segs as f32) * std::f32::consts::TAU;
            [a.cos() * radius, a.sin() * radius]
        })
        .collect();
    let mesh = Mesh::new_line(ctx, &pts, thickness, Color::WHITE)?;
    STROKE_ARC_CACHE.with(|c| c.borrow_mut().insert(key, mesh.clone()));
    Ok(mesh)
}

/// Fetch a cached stroke-circle mesh for the given radius/thickness (built once per rounded
/// key, reused after that), instead of calling `Mesh::new_circle` fresh every draw. The mesh is
/// baked with `Color::WHITE` — callers should tint it via `DrawParam::color`, exactly like the
/// existing `UNIT_CIRCLE`/`UNIT_LINE` fill meshes.
fn cached_stroke_circle(ctx: &mut Context, radius: f32, thickness: f32) -> ggez::GameResult<Mesh> {
    let radius = radius.max(0.5);
    let thickness = thickness.max(0.25);
    let key = ((radius * 2.0).round() as i32, (thickness * 4.0).round() as i32);

    if let Some(mesh) = STROKE_CIRCLE_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return Ok(mesh);
    }

    let mesh = Mesh::new_circle(
        ctx,
        DrawMode::stroke(thickness),
        [0.0, 0.0],
        radius,
        1.2,
        Color::WHITE,
    )?;
    STROKE_CIRCLE_CACHE.with(|c| c.borrow_mut().insert(key, mesh.clone()));
    Ok(mesh)
}

/// Fetch the cached unit-square mesh (1x1, top-left corner at the origin), building it once
/// on first use. Scale by `(w, h)` and set `.dest((x, y))` to place/size an axis-aligned fill
/// rectangle without allocating a fresh mesh — the same trick `UNIT_CIRCLE`/`UNIT_LINE` use.
/// Baked with `Color::WHITE`; tint via `DrawParam::color`.
pub fn unit_square(ctx: &mut Context) -> ggez::GameResult<&'static Mesh> {
    match UNIT_SQUARE.get() {
        Some(mesh) => Ok(mesh),
        None => {
            let mesh = Mesh::new_rectangle(ctx, DrawMode::fill(), Rect::new(0.0, 0.0, 1.0, 1.0), Color::WHITE)?;
            Ok(UNIT_SQUARE.get_or_init(|| mesh))
        }
    }
}

/// Fetch a cached stroke-rectangle mesh for the given size/thickness (built once per rounded
/// key, reused after that), instead of calling `Mesh::new_rectangle` fresh every draw. Baked at
/// its actual size (not unit-scaled), since scaling would distort the stroke thickness the same
/// way it would for a stroke circle — draw with `.dest((x, y))` only, no `.scale(..)`.
pub fn cached_stroke_rect(ctx: &mut Context, w: f32, h: f32, thickness: f32) -> ggez::GameResult<Mesh> {
    let w = w.max(0.5);
    let h = h.max(0.5);
    let thickness = thickness.max(0.25);
    let key = (
        (w * 2.0).round() as i32,
        (h * 2.0).round() as i32,
        (thickness * 4.0).round() as i32,
    );

    if let Some(mesh) = STROKE_RECT_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return Ok(mesh);
    }

    let mesh = Mesh::new_rectangle(
        ctx,
        DrawMode::stroke(thickness),
        Rect::new(0.0, 0.0, w, h),
        Color::WHITE,
    )?;
    STROKE_RECT_CACHE.with(|c| c.borrow_mut().insert(key, mesh.clone()));
    Ok(mesh)
}

#[derive(Copy, Clone, Debug, AsStd140)]
pub struct ResolutionUniform {
    pub width: f32,
    pub height: f32,
    pub time: f32,
}

#[derive(Copy, Clone, Debug, AsStd140)]
pub struct FlashlightUniform {
    pub center_x: f32,
    pub center_y: f32,
    pub angle: f32,
    pub spread: f32,
    pub range: f32,
    pub time: f32,
    pub time_since_catch: f32,
    pub laser_level: f32,
    pub screen_width: f32,
    pub screen_height: f32,
}

#[derive(Copy, Clone, Debug, AsStd140)]
pub struct ParticleUniform {
    pub screen_width: f32,
    pub screen_height: f32,
    pub time: f32,
    pub _padding: f32,
}

#[derive(Clone, Debug)]
pub struct Particle {
    pub pos: Vec2,
    pub vel: Vec2,
    pub life: f32,
    pub max_life: f32,
    pub size: f32,
    pub color: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct ParticleSystem {
    pub particles: Vec<Particle>,
}

impl ParticleSystem {
    pub fn new() -> Self {
        Self {
            particles: Vec::new(),
        }
    }

    pub fn spawn_catch_effect(&mut self, pos: Vec2, crab_color: [f32; 3], crab_type: CrabType, rng: &mut impl Rng) {
        let (particle_count, speed_range, size_range, special_effect) = match crab_type {
            CrabType::Normal => (20, 80.0..180.0, 3.0..6.0, false),
            CrabType::Fast => (35, 120.0..300.0, 2.0..5.0, true), // More particles, faster
            CrabType::Big => (40, 60.0..150.0, 4.0..10.0, false), // Larger particles
            CrabType::Sneaky => (15, 100.0..250.0, 1.5..4.0, true), // Fewer, sneaky particles
            CrabType::Boss => (70, 90.0..320.0, 4.0..13.0, true),   // Huge celebratory burst
        };
        
        // Create main particles
        for _ in 0..particle_count {
            let angle = rng.random_range(0.0..std::f32::consts::TAU);
            let speed = rng.random_range(speed_range.clone());
            let vel = Vec2::new(angle.cos(), angle.sin()) * speed;
            let life = rng.random_range(0.8..1.8);
            let size = rng.random_range(size_range.clone());
            
            // Add some color variation and make particles brighter
            let color_variation = rng.random_range(-0.2..0.2);
            let brightness_boost = rng.random_range(0.3..0.7);
            let particle_color = [
                (crab_color[0] + color_variation + brightness_boost).clamp(0.0, 1.0),
                (crab_color[1] + color_variation + brightness_boost).clamp(0.0, 1.0),
                (crab_color[2] + color_variation + brightness_boost).clamp(0.0, 1.0),
            ];
            
            self.particles.push(Particle {
                pos,
                vel,
                life,
                max_life: life,
                size,
                color: particle_color,
            });
        }
        
        // Add special sparkly particles for Fast and Sneaky crabs
        if special_effect {
            let sparkle_count = match crab_type {
                CrabType::Fast => 15,
                CrabType::Sneaky => 8,
                _ => 0,
            };
            
            for _ in 0..sparkle_count {
                let angle = rng.random_range(0.0..std::f32::consts::TAU);
                let speed = rng.random_range(150.0..400.0);
                let vel = Vec2::new(angle.cos(), angle.sin()) * speed;
                let life = rng.random_range(0.4..1.0);
                let size = rng.random_range(1.0..3.0);
                
                let sparkle_color = match crab_type {
                    CrabType::Fast => [1.0, 0.8, 0.2], // Golden sparkles for fast crabs
                    CrabType::Sneaky => [0.7, 0.9, 1.0], // Blue sparkles for sneaky crabs
                    _ => [1.0, 1.0, 0.9],
                };
                
                self.particles.push(Particle {
                    pos,
                    vel,
                    life,
                    max_life: life,
                    size,
                    color: sparkle_color,
                });
            }
        } else {
            // Regular sparkles for Normal and Big crabs
            for _ in 0..8 {
                let angle = rng.random_range(0.0..std::f32::consts::TAU);
                let speed = rng.random_range(120.0..300.0);
                let vel = Vec2::new(angle.cos(), angle.sin()) * speed;
                let life = rng.random_range(0.4..0.8);
                let size = rng.random_range(1.5..4.0);
                
                self.particles.push(Particle {
                    pos,
                    vel,
                    life,
                    max_life: life,
                    size,
                    color: [1.0, 1.0, 0.9], // Bright white/yellow sparkles
                });
            }
        }
    }

    pub fn spawn_movement_trail(&mut self, pos: Vec2, velocity: Vec2, time: f32, rng: &mut impl Rng) {
        let speed = velocity.length();
        if speed < 15.0 {
            return;
        }
        let count = ((speed / 60.0) as usize).clamp(1, 5);
        for _ in 0..count {
            // Cycle hue over time for a rainbow trail
            let hue = (time * 0.6 + pos.x * 0.003 + pos.y * 0.002) % 1.0;
            let r = ((hue * 6.0 - 3.0).abs() - 1.0).clamp(0.0, 1.0);
            let g = (2.0 - (hue * 6.0 - 2.0).abs()).clamp(0.0, 1.0);
            let b = (2.0 - (hue * 6.0 - 4.0).abs()).clamp(0.0, 1.0);
            let spread_angle = rng.random_range(0.0..std::f32::consts::TAU);
            let spread_dist = rng.random_range(0.0..12.0);
            let vel = Vec2::new(spread_angle.cos(), spread_angle.sin()) * spread_dist
                - velocity * 0.08;
            let life = rng.random_range(0.12..0.30);
            self.particles.push(Particle {
                pos: pos + Vec2::new(
                    rng.random_range(-5.0..5.0),
                    rng.random_range(-5.0..5.0),
                ),
                vel,
                life,
                max_life: life,
                size: rng.random_range(2.0..5.5),
                color: [r, g, b],
            });
        }
    }

    /// Kick up a small warm dust puff from a conga-train crab's feet as it scuttles along.
    /// Called once per caught crab per frame; internally throttled so the emission rate is
    /// framerate-independent and only fires while the crab is actually moving. Because every
    /// crab in the train emits, a longer conga line kicks up a bigger, more spectacular cloud
    /// — the visual payoff scales with how many crabs you've caught. `move_delta` is the crab's
    /// per-frame position change; `dt` the frame time.
    pub fn spawn_conga_dust(&mut self, pos: Vec2, move_delta: Vec2, dt: f32, rng: &mut impl Rng) {
        let dt = dt.max(1e-4);
        let speed = move_delta.length() / dt;
        if speed < 40.0 {
            return;
        }
        // ~10-18 puffs/sec per crab, a touch faster the quicker it's moving. Probability per
        // frame = rate * dt, so total emission is stable regardless of FPS.
        let rate = (10.0 + speed * 0.02).min(18.0);
        if rng.random::<f32>() > rate * dt {
            return;
        }
        let back = -move_delta.normalize_or_zero();
        let perp = Vec2::new(-back.y, back.x);
        // Mostly backward, with a little sideways scatter and a gentle upward kick so the puff
        // rises before the particle system's gravity settles it back down.
        let vel = back * rng.random_range(15.0..45.0)
            + perp * rng.random_range(-18.0..18.0)
            + Vec2::new(0.0, rng.random_range(-40.0..-15.0));
        let life = rng.random_range(0.30..0.6);
        // Warm sandy tone; drawn additively so keep it dim — reads as a soft haze, not a blob.
        let shade = rng.random_range(0.0..0.08);
        self.particles.push(Particle {
            pos: pos + Vec2::new(rng.random_range(-4.0..4.0), rng.random_range(-3.0..3.0)),
            vel,
            life,
            max_life: life,
            size: rng.random_range(2.5..3.9),
            color: [0.34 + shade, 0.28 + shade, 0.20 + shade],
        });
    }

    pub fn spawn_dash_burst(&mut self, pos: Vec2, move_dir: Vec2, rng: &mut impl Rng) {
        // shoot particles mostly backward from the movement direction, spread into a fan
        let back = if move_dir.length() > 0.1 { -move_dir.normalize() } else { Vec2::new(0.0, 1.0) };
        for _ in 0..30 {
            let spread = rng.random_range(-0.9_f32..0.9_f32);
            let perp = Vec2::new(-back.y, back.x);
            let dir = (back + perp * spread).normalize();
            let speed = rng.random_range(160.0_f32..480.0_f32);
            let life = rng.random_range(0.18_f32..0.40_f32);
            // Cyan-white colour with slight variation
            let g = rng.random_range(0.85_f32..1.0_f32);
            self.particles.push(Particle {
                pos: pos + Vec2::new(rng.random_range(-6.0_f32..6.0_f32), rng.random_range(-6.0_f32..6.0_f32)),
                vel: dir * speed,
                life,
                max_life: life,
                size: rng.random_range(3.0_f32..7.5_f32),
                color: [0.7, g, 1.0],
            });
        }
    }

    pub fn spawn_beat_pulse(&mut self, positions: &[Vec2], beat_intensity: f32, chain_len: usize, rng: &mut impl Rng) {
        if positions.is_empty() { return; }
        // Scale ring size with chain length — bigger train = bigger burst
        let base_count = (4 + chain_len / 3).min(16) as usize;
        let ring_speed = 180.0 + beat_intensity * 120.0;
        for &center in positions {
            for j in 0..base_count {
                let angle = (j as f32 / base_count as f32) * std::f32::consts::TAU;
                // Slight random spread on the angle so rings aren't perfectly geometric
                let spread = rng.random_range(-0.18_f32..0.18_f32);
                let dir = Vec2::new((angle + spread).cos(), (angle + spread).sin());
                let speed = ring_speed * rng.random_range(0.7_f32..1.3_f32);
                let life = rng.random_range(0.25_f32..0.55_f32);
                // Rainbow hue per particle, offset by position for variety
                let hue = (angle / std::f32::consts::TAU + center.x * 0.001 + center.y * 0.0007) % 1.0;
                let r = ((hue * 6.0 - 3.0).abs() - 1.0).clamp(0.0, 1.0);
                let g = (2.0 - (hue * 6.0 - 2.0).abs()).clamp(0.0, 1.0);
                let b = (2.0 - (hue * 6.0 - 4.0).abs()).clamp(0.0, 1.0);
                self.particles.push(Particle {
                    pos: center,
                    vel: dir * speed,
                    life,
                    max_life: life,
                    size: rng.random_range(2.0_f32..5.5_f32),
                    color: [r, g, b],
                });
            }
        }
    }

    pub fn spawn_milestone_fireworks(&mut self, center: Vec2, milestone: usize, rng: &mut impl Rng) {
        // Scale particle count with milestone tier, capped at 200
        let count = (120 + (milestone / 5).min(8) * 10).min(200);

        // --- Color burst pass ---
        for i in 0..count {
            let angle = rng.random_range(0.0_f32..std::f32::consts::TAU);
            let speed = rng.random_range(200.0_f32..600.0_f32);
            // Bias direction upward: subtract from y so particles tend to shoot upward
            let upward_bias = rng.random_range(100.0_f32..300.0_f32);
            let vel = Vec2::new(angle.cos() * speed, angle.sin() * speed - upward_bias);
            let life = rng.random_range(1.2_f32..2.8_f32);
            // Full rainbow: spread hue evenly across particles with random jitter
            let hue = ((i as f32 / count as f32) + rng.random_range(-0.05_f32..0.05_f32)).rem_euclid(1.0);
            let r = ((hue * 6.0 - 3.0).abs() - 1.0).clamp(0.0, 1.0);
            let g = (2.0 - (hue * 6.0 - 2.0).abs()).clamp(0.0, 1.0);
            let b = (2.0 - (hue * 6.0 - 4.0).abs()).clamp(0.0, 1.0);
            self.particles.push(Particle {
                pos: center,
                vel,
                life,
                max_life: life,
                size: rng.random_range(4.0_f32..12.0_f32),
                color: [r, g, b],
            });
        }

        // --- Sparkle pass: 30 bright white/yellow "star" particles ---
        for _ in 0..30 {
            let angle = rng.random_range(0.0_f32..std::f32::consts::TAU);
            let speed = rng.random_range(300.0_f32..700.0_f32);
            let upward_bias = rng.random_range(100.0_f32..250.0_f32);
            let vel = Vec2::new(angle.cos() * speed, angle.sin() * speed - upward_bias);
            let life = rng.random_range(0.6_f32..1.2_f32);
            // Alternate between pure white and bright yellow for sparkle variety
            let color = if rng.random_range(0.0_f32..1.0_f32) < 0.5 {
                [1.0, 1.0, 1.0] // white
            } else {
                [1.0, 0.95, 0.3] // bright yellow
            };
            self.particles.push(Particle {
                pos: center,
                vel,
                life,
                max_life: life,
                size: rng.random_range(2.0_f32..5.0_f32),
                color,
            });
        }
    }

    pub fn update(&mut self, dt: f32) {
        self.particles.retain_mut(|particle| {
            particle.life -= dt;
            particle.pos += particle.vel * dt;

            // Add gravity effect
            particle.vel.y += 200.0 * dt;
            
            // Add air resistance
            particle.vel *= 0.96;
            
            // Shrink particles over time
            let life_ratio = particle.life / particle.max_life;
            particle.size = particle.size * (0.95 + 0.05 * life_ratio);
            
            particle.life > 0.0
        });
    }
}

pub fn draw_particles(
    ctx: &mut Context,
    canvas: &mut Canvas,
    particle_system: &ParticleSystem,
) -> ggez::GameResult {
    let unit_circle = match UNIT_CIRCLE.get() {
        Some(mesh) => mesh,
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh)
        }
    };

    // Set additive blend mode for glowing effect
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    for particle in &particle_system.particles {
        let life_ratio = particle.life / particle.max_life;
        let alpha = (life_ratio * 0.8).clamp(0.0, 1.0);

        // Main particle
        let color = Color::new(
            particle.color[0],
            particle.color[1],
            particle.color[2],
            alpha,
        );

        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(particle.pos)
                .scale(Vec2::splat(particle.size))
                .color(color),
        );

        // Add a subtle glow effect for larger particles
        if particle.size > 4.0 {
            let glow_color = Color::new(
                particle.color[0],
                particle.color[1],
                particle.color[2],
                alpha * 0.3,
            );

            canvas.draw(
                unit_circle,
                DrawParam::default()
                    .dest(particle.pos)
                    .scale(Vec2::splat(particle.size * 1.5))
                    .color(glow_color),
            );
        }
    }

    // Restore original blend mode
    canvas.set_blend_mode(original_blend);
    Ok(())
}

pub fn draw_grass(
    ctx: &mut Context,
    canvas: &mut Canvas,
    width: f32,
    height: f32,
    texture: &Image,
    shader: &Shader,
    time: f32,
    biome_tint: Color,
) -> ggez::GameResult {
    let blend_mode = canvas.blend_mode();
    let solid_bg = unit_square(ctx)?;
    canvas.draw(
        solid_bg,
        DrawParam::default()
            .scale(Vec2::new(width, height))
            .color(Color::from_rgb(0, 100, 0)),
    );

    // Draw a full-screen quad using the grass shader.
    let params = ShaderParamsBuilder::new(&ResolutionUniform {
        width,
        height,
        time,
    })
    .build(ctx);
    canvas.set_shader_params(&params);
    canvas.set_shader(shader);
    let quad = Mesh::new_rectangle(
        ctx,
        DrawMode::fill(),
        Rect::new(-width / 2.0, -height / 2.0, width, height),
        Color::RED,
    )?;
    canvas.draw(&quad, DrawParam::default());
    canvas.set_default_shader();
    canvas.set_blend_mode(BlendMode::MULTIPLY);

    // Repeat a tiled grass texture across the screen.
    let tile_w = texture.width() as f32;
    let tile_h = texture.height() as f32;
    let tiles_x = (width / tile_w).ceil() as i32;
    let tiles_y = (height / tile_h).ceil() as i32;
    for y in 0..tiles_y {
        for x in 0..tiles_x {
            let dest = [x as f32 * tile_w, y as f32 * tile_h];
            canvas.draw(texture, DrawParam::default().dest(dest));
        }
    }

    // Biome color grade: a full-screen multiply pass that recolors the whole ground so each
    // level reads as a distinct zone (warm meadow, cool tide pools, stony shore, neon kelp).
    // Blend mode is already MULTIPLY here from the tiling pass, so this is a single extra quad.
    canvas.draw(
        unit_square(ctx)?,
        DrawParam::default()
            .scale(Vec2::new(width, height))
            .color(biome_tint),
    );

    canvas.set_blend_mode(blend_mode);
    Ok(())
}

pub fn draw_rustler(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    sprite: &Image,
    velocity: Vec2,
    beat_intensity: f32,
    time: f32,
    dashing: bool,
) -> ggez::GameResult {
    let base = 0.05_f32;
    let dims = Vec2::new(sprite.width() as f32, sprite.height() as f32) * base;
    // Keep the sprite centered on the same point it used to occupy (top-left was
    // pos + (15,15) at 0.05 scale) so transforms can pivot around the center.
    let center = pos + Vec2::new(15.0, 15.0) + dims * 0.5;

    let beat = beat_intensity.clamp(0.0, 1.0);

    // Beat-synced hop: the rustler pops upward on every downbeat like everything else
    // in the conga, plus a gentle idle breathing bob so it's never fully still.
    let hop = beat * 8.0;
    let idle = (time * 2.2).sin() * 1.5;
    let bob = -hop + idle;

    // Squash & stretch: stretch tall on the up-beat, and stretch along the run when
    // moving fast (extra on a dash) for a snappy sense of momentum.
    let hspeed = velocity.x.abs();
    let run_stretch = (hspeed / 200.0).clamp(0.0, 1.0) * if dashing { 0.20 } else { 0.09 };
    let sx = base * (1.0 - beat * 0.08 + run_stretch);
    let sy = base * (1.0 + beat * 0.13 - run_stretch * 0.5);

    // Lean into horizontal movement — tilt forward as if leaning into the run.
    let lean_amt = if dashing { 0.26 } else { 0.16 };
    let lean = (velocity.x / 200.0).clamp(-1.0, 1.0) * lean_amt;

    // Grounding drop shadow that shrinks and fades as the rustler leaves the ground.
    let unit_circle = match UNIT_CIRCLE.get() {
        Some(mesh) => mesh,
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh)
        }
    };
    let ground_y = center.y + dims.y * 0.42;
    let lift = hop.max(0.0);
    let shadow_shrink = (1.0 - lift * 0.02).clamp(0.55, 1.0);
    let shadow_alpha = (0.32 * shadow_shrink).clamp(0.0, 1.0);
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(Vec2::new(center.x, ground_y))
            .scale(Vec2::new(
                dims.x * 0.34 * shadow_shrink,
                dims.y * 0.13 * shadow_shrink,
            ))
            .color(Color::new(0.0, 0.0, 0.0, shadow_alpha)),
    );

    // Draw the sprite pivoting around its center so the hop, squash and lean all
    // anchor sensibly.
    canvas.draw(
        sprite,
        DrawParam::default()
            .dest(Vec2::new(center.x, center.y + bob))
            .offset(Vec2::new(0.5, 0.5))
            .rotation(lean)
            .scale(Vec2::new(sx, sy))
            .color(Color::from_rgba(255, 255, 255, 255)),
    );

    Ok(())
}

pub fn draw_crab(ctx: &mut Context, canvas: &mut Canvas, crab: &EnemyCrab, draw_pos: Vec2, beat_phase: f32, join_pulse: f32, y_lift: f32, rotation: f32) -> ggez::GameResult {
    // Crabs previously rebuilt ~13 fresh GPU meshes every frame (shadow, body, 6 legs,
    // 2 claws, 4 eye parts) via Mesh::new_circle/new_line/new_ellipse. With a long conga
    // train this was easily 100+ mesh allocations per frame. Instead reuse the same cached
    // unit-circle and unit-line meshes the particle system and conga rope already share,
    // positioning/rotating/scaling them per-part via DrawParam instead of baking shape into
    // fresh vertex buffers. A body-space offset that needs to rotate with the crab (claw
    // and eye positions, leg roots) is rotated by hand via `rotate_offset` before being
    // folded into `dest`, since DrawParam only applies one rotation after one translation.
    let unit_circle = match UNIT_CIRCLE.get() {
        Some(mesh) => mesh,
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh)
        }
    };
    let unit_line = match UNIT_LINE.get() {
        Some(mesh) => mesh,
        None => {
            let mesh = Mesh::new_rectangle(
                ctx,
                DrawMode::fill(),
                Rect::new(0.0, -0.5, 1.0, 1.0),
                Color::WHITE,
            )?;
            UNIT_LINE.get_or_init(|| mesh)
        }
    };

    let cos_r = rotation.cos();
    let sin_r = rotation.sin();
    // Rotates a body-local offset (x, y) by the crab's facing rotation, matching what the
    // old per-part mesh + `.rotation(rotation)` draw used to do implicitly.
    let rotate_offset = |x: f32, y: f32| Vec2::new(x * cos_r - y * sin_r, x * sin_r + y * cos_r);

    // Grow size with age
    let grow_t = (crab.spawn_time / 10.0).min(1.0);
    let base_size = CRAB_SIZE * (0.6 + 0.4 * grow_t) * crab.scale;
    // Scale pop when joining the chain (bell-curve: peak at join_pulse=0.5)
    let pulse_scale = if join_pulse <= 1.0 {
        1.0 + 0.45 * join_pulse * (1.0 - join_pulse) * 4.0
    } else {
        1.0
    };
    let size = base_size * pulse_scale;

    // Drop shadow: shrinks and moves away as the crab lifts off the ground
    let shadow_scale_x = (1.0 - y_lift / 60.0).clamp(0.4, 1.0);
    let shadow_scale_y = shadow_scale_x * 0.45;
    let shadow_offset_y = size * 0.35 + y_lift * 0.6;
    let shadow_offset_x = y_lift * 0.25;
    let shadow_alpha = ((1.0 - y_lift / 55.0) * 100.0).clamp(20.0, 100.0) as u8;
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(draw_pos + Vec2::new(shadow_offset_x, shadow_offset_y))
            .scale(Vec2::new(
                size * shadow_scale_x * 0.55,
                size * shadow_scale_y * 0.55,
            ))
            .color(Color::from_rgba(0, 0, 0, shadow_alpha)),
    );

    // Color: more red as crab ages, and different color for type
    let [r, g, b] = crab.crab_color();
    let flash = if join_pulse > 0.0 && join_pulse <= 1.0 {
        join_pulse * (1.0 - join_pulse) * 4.0 * 0.5  // peak 0.5 at pulse=0.5
    } else {
        0.0
    };
    let crab_color = Color::new((r + flash).min(1.0), (g + flash).min(1.0), (b + flash).min(1.0), 1.0);

    // Crab body (rotation-invariant, so no need to rotate the draw)
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(draw_pos)
            .scale(Vec2::splat(size / 2.0))
            .color(crab_color),
    );

    // Crab legs (6 lines): the leg root sits on the body's radius at `angle`, so rotating
    // the whole leg (root + direction) by the crab's facing is the same as just adding
    // `rotation` to `angle` before computing everything in world space directly.
    let leg_len = size * 0.7;
    let leg_color = Color::from_rgb(200, 50, 50);
    for i in 0..6 {
        let base_angle = std::f32::consts::PI * (0.25 + i as f32 / 6.0);
        let time = ctx.time.time_since_start().as_secs_f32();
        let phase = (crab.pos.x + crab.pos.y) * 0.05;
        let wiggle_speed = 2.0 + crab.speed * 0.08; // scale with crab speed
        let wiggle_amp = 0.18 + beat_phase * 0.12;
        let wiggle = (time * wiggle_speed * (1.0 + beat_phase * 0.5) + phase + i as f32).sin() * wiggle_amp;
        let angle = base_angle + wiggle + rotation;
        let root = draw_pos + Vec2::new(angle.cos(), angle.sin()) * (size / 2.0);
        canvas.draw(
            unit_line,
            DrawParam::default()
                .dest(root)
                .rotation(angle)
                .scale(Vec2::new(leg_len, 2.0))
                .color(leg_color),
        );
    }

    // Crab claws (small circles)
    let claw_offset = size * 0.7;
    let claw_radius = size * 0.18;
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(draw_pos + rotate_offset(-(claw_offset), -(claw_offset * 0.3)))
            .scale(Vec2::splat(claw_radius))
            .color(crab_color),
    );
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(draw_pos + rotate_offset(claw_offset, -(claw_offset * 0.3)))
            .scale(Vec2::splat(claw_radius))
            .color(crab_color),
    );

    // Eyes
    let eye_radius = size * 0.13;
    let eye_x = size * 0.22;
    let eye_y = -size * 0.18;
    let pupil_r = eye_radius * (0.50 + beat_phase * 0.15);
    let (pdx, pdy) = if !crab.caught {
        let vl = crab.vel.length();
        if vl > 1.0 {
            (crab.vel.x / vl * eye_radius * 0.4, crab.vel.y / vl * eye_radius * 0.4)
        } else {
            (0.0, 0.0)
        }
    } else {
        (0.0, 0.0)
    };
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(draw_pos + rotate_offset(-eye_x, eye_y))
            .scale(Vec2::splat(eye_radius))
            .color(Color::WHITE),
    );
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(draw_pos + rotate_offset(eye_x, eye_y))
            .scale(Vec2::splat(eye_radius))
            .color(Color::WHITE),
    );
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(draw_pos + rotate_offset(-eye_x + pdx, eye_y + pdy))
            .scale(Vec2::splat(pupil_r))
            .color(Color::BLACK),
    );
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(draw_pos + rotate_offset(eye_x + pdx, eye_y + pdy))
            .scale(Vec2::splat(pupil_r))
            .color(Color::BLACK),
    );

    Ok(())
}

/// Draws the King Crab's menacing aura plus a health ring showing how much wearing-down is left.
/// While `health_frac > 0` a golden arc drains counter-clockwise as the player holds the beam on it;
/// once worn down (`health_frac <= 0`) the ring flips to a bright pulsing "CATCH ME" glow instead.
pub fn draw_boss_health_ring(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    size: f32,
    health_frac: f32,
    time: f32,
) -> ggez::GameResult {
    let radius = size * 0.85;
    let pulse = (time * 6.0).sin() * 0.5 + 0.5; // 0..1

    // Pulsing aura ring behind the boss — deep gold, breathing with the beat of the track.
    // Reuses the same STROKE_CIRCLE_CACHE every other ring effect in this file draws from,
    // instead of rebuilding a fresh ~48-point Vec + GPU mesh every frame this boss is alive.
    let aura_radius = radius * (1.12 + pulse * 0.08);
    let aura = cached_stroke_circle(ctx, aura_radius, 3.0)?;
    canvas.draw(
        &aura,
        DrawParam::default()
            .dest(pos)
            .color(Color::new(1.0, 0.8, 0.25, 0.30 + pulse * 0.25)),
    );

    if health_frac > 0.0 {
        // Faint full track so the empty portion still reads as "health you've drained".
        let track = cached_stroke_circle(ctx, radius, 5.0)?;
        canvas.draw(
            &track,
            DrawParam::default()
                .dest(pos)
                .color(Color::new(0.0, 0.0, 0.0, 0.45)),
        );

        // Filled arc from the top, clockwise, spanning the remaining health fraction. Cached
        // per (radius, filled-segment) combo — bounded to 49 possible fill levels for the
        // lifetime of a single boss, instead of a fresh mesh every single frame.
        let segs = 48usize;
        let filled = ((segs as f32) * health_frac.clamp(0.0, 1.0)).ceil().max(1.0) as usize;
        // Green when fresh, shading to red as it's worn down.
        let col = Color::new(
            (1.0 - health_frac).clamp(0.2, 1.0),
            (0.35 + health_frac * 0.55).clamp(0.0, 1.0),
            0.15,
            1.0,
        );
        let arc = cached_stroke_arc(ctx, radius, 5.0, segs, filled)?;
        canvas.draw(&arc, DrawParam::default().dest(pos).color(col));
    } else {
        // Worn down — flash a bright "catch me now" ring so the player knows to grab it.
        let ring = cached_stroke_circle(ctx, radius, 4.0 + pulse * 3.0)?;
        canvas.draw(
            &ring,
            DrawParam::default()
                .dest(pos)
                .color(Color::new(0.4, 1.0, 0.5, 0.6 + pulse * 0.4)),
        );
    }
    Ok(())
}

pub fn draw_flashlight(
    ctx: &mut Context,
    canvas: &mut Canvas,
    player_pos: Vec2,
    dir: Vec2,
    time_since_catch: f32,
    flashlight: &Flashlight,
    shader: &Shader,
    screen_width: f32,
    screen_height: f32,
) -> ggez::GameResult {
    // To position the flashlight in the player sprite hand.
    let offset = Vec2 { x: -50.0, y: -5.0 };

    // Flicker logic
    let time = ctx.time.time_since_start().as_secs_f32();

    // Flashlight parameters
    let laser_level = flashlight.laser_level;
    let cone_angle = flashlight.cone_upgrade;
    let range = flashlight.range_upgrade;

    // Calculate flashlight properties
    let flashlight_len = range.max(80.0);
    let spread = cone_angle.max(0.15);
    let center = Vec2::new(
        player_pos.x + PLAYER_SIZE / 2.0,
        player_pos.y + PLAYER_SIZE / 2.0,
    );
    let angle = dir.y.atan2(dir.x);

    // Create uniform data for the shader
    let uniform_data = FlashlightUniform {
        center_x: center.x,
        center_y: center.y,
        angle,
        spread,
        range: flashlight_len,
        time,
        time_since_catch,
        laser_level: laser_level as f32,
        screen_width,
        screen_height,
    };

    // Set up shader parameters
    let params = ShaderParamsBuilder::new(&uniform_data).build(ctx);
    canvas.set_shader_params(&params);
    canvas.set_shader(shader);

    // Draw a full-screen quad that the shader will render the flashlight onto
    // Use the same pattern as the grass shader
    let flashlight_quad = Mesh::new_rectangle(
        ctx,
        DrawMode::fill(),
        Rect::new(
            -screen_width / 2.0,
            -screen_height / 2.0,
            screen_width,
            screen_height,
        ),
        Color::WHITE,
    )?;

    // Set additive blend mode for the flashlight effect
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);
    canvas.draw(&flashlight_quad, DrawParam::default());

    let rotation = dir.y.atan2(dir.x) + std::f32::consts::PI / 2.0;

    // Draw flashlight body.
    let flashlight_body = Mesh::new_rectangle(
        ctx,
        DrawMode::fill(),
        Rect::new(-5.0, 0.0, 10.0, 24.0),
        Color::BLACK,
    )?;
    canvas.draw(
        &flashlight_body,
        DrawParam::default().dest(center).rotation(rotation),
    );

    // Restore original blend mode and shader
    canvas.set_blend_mode(original_blend);
    canvas.set_default_shader();
    Ok(())
}

pub fn draw_conga_rope(
    ctx: &mut Context,
    canvas: &mut Canvas,
    player_pos: Vec2,
    chain_crabs: &[&EnemyCrab],
    time: f32,
    beat_intensity: f32,
) -> ggez::GameResult {
    if chain_crabs.is_empty() {
        return Ok(());
    }

    let unit_line = match UNIT_LINE.get() {
        Some(mesh) => mesh,
        None => {
            let mesh = Mesh::new_rectangle(
                ctx,
                DrawMode::fill(),
                Rect::new(0.0, -0.5, 1.0, 1.0),
                Color::WHITE,
            )?;
            UNIT_LINE.get_or_init(|| mesh)
        }
    };

    // Number of sub-segments per chain link — more = smoother curve
    const SEGS: usize = 14;
    // Amplitude of the sine-wave wiggle (pixels perpendicular to the link)
    let wiggle_amp = 5.0 + beat_intensity * 8.0;
    // Speed of the wave traveling along the rope (faster on beat)
    let wave_speed = 3.5 + beat_intensity * 2.5;
    let thickness = 3.0 + beat_intensity * 4.5;
    let alpha_base: f32 = 0.55 + beat_intensity * 0.4;

    // Build the full ordered list of waypoints: player → crab0 → crab1 → …
    let player_center = player_pos + Vec2::new(24.0, 24.0);
    let mut waypoints: Vec<Vec2> = Vec::with_capacity(chain_crabs.len() + 1);
    waypoints.push(player_center);
    for crab in chain_crabs {
        waypoints.push(crab.pos);
    }

    // Total chain length for hue mapping
    let total_links = chain_crabs.len() as f32;

    for (link_idx, window) in waypoints.windows(2).enumerate() {
        let start = window[0];
        let end = window[1];
        let dist = start.distance(end);
        if dist < 1.0 {
            continue;
        }

        // Unit vectors along and perpendicular to this link
        let along = (end - start) / dist;
        let perp = Vec2::new(-along.y, along.x);

        // Hue for this link (rainbow along the chain)
        let hue = (link_idx as f32 / total_links.max(1.0) + time * 0.12) % 1.0;

        // Subdivide into SEGS micro-segments
        let mut prev_point = start;
        for seg in 0..=SEGS {
            let t = seg as f32 / SEGS as f32;

            // Travelling sine wave: phase depends on position-along-rope + time
            let phase = t * std::f32::consts::TAU * 1.5
                + link_idx as f32 * 0.9
                - time * wave_speed;
            let offset = perp * wiggle_amp * phase.sin();
            let point = start.lerp(end, t) + offset;

            if seg > 0 {
                // Rainbow color for this micro-segment
                let seg_hue = (hue + t * 0.08) % 1.0;
                let r = ((seg_hue * 6.0 - 3.0).abs() - 1.0).clamp(0.0, 1.0);
                let g = (2.0 - (seg_hue * 6.0 - 2.0).abs()).clamp(0.0, 1.0);
                let b = (2.0 - (seg_hue * 6.0 - 4.0).abs()).clamp(0.0, 1.0);
                // Slightly boost saturation/brightness
                let boost = 0.35;
                let rr = (r + boost).min(1.0);
                let gg = (g + boost).min(1.0);
                let bb = (b + boost).min(1.0);
                let color = Color::new(rr, gg, bb, alpha_base);

                let seg_delta = point - prev_point;
                let seg_len = seg_delta.length();
                if seg_len > 0.5 {
                    let seg_angle = seg_delta.y.atan2(seg_delta.x);
                    let seg_param = DrawParam::default()
                        .dest(prev_point)
                        .rotation(seg_angle)
                        .scale(Vec2::new(seg_len, thickness))
                        .color(color);
                    canvas.draw(unit_line, seg_param);

                    // Thinner glow pass with additive blend for a neon look
                    let glow_color = Color::new(rr, gg, bb, alpha_base * 0.35);
                    let glow_param = DrawParam::default()
                        .dest(prev_point)
                        .rotation(seg_angle)
                        .scale(Vec2::new(seg_len, thickness * 2.2))
                        .color(glow_color);
                    canvas.set_blend_mode(BlendMode::ADD);
                    canvas.draw(unit_line, glow_param);
                    canvas.set_blend_mode(BlendMode::ALPHA);
                }
            }
            prev_point = point;
        }
    }
    Ok(())
}

pub fn draw_beat_indicator(
    ctx: &mut Context,
    canvas: &mut Canvas,
    center: Vec2,
    beat_intensity: f32,
    _time: f32,
) -> ggez::GameResult {
    let unit_circle = match UNIT_CIRCLE.get() {
        Some(mesh) => mesh,
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh)
        }
    };
    let base_r = 20.0;
    let pulse_r = base_r + beat_intensity * 14.0;
    let alpha = ((80.0 + beat_intensity * 175.0) as u8).min(255);
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(center)
            .scale(Vec2::splat(pulse_r))
            .color(Color::from_rgba(255, 200, 50, alpha)),
    );
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(center)
            .scale(Vec2::splat(base_r * 0.55))
            .color(Color::from_rgba(255, 140, 50, 220)),
    );
    Ok(())
}

pub struct FloatingText {
    pub text: String,
    pub pos: Vec2,
    pub vel: Vec2,
    pub life: f32,
    pub max_life: f32,
    pub scale: f32,
    pub color: [f32; 4], // rgba 0..1
}

pub struct FloatingTextSystem {
    pub texts: Vec<FloatingText>,
}

impl FloatingTextSystem {
    pub fn new() -> Self {
        Self { texts: Vec::new() }
    }

    pub fn spawn(&mut self, text: String, pos: Vec2, scale: f32, color: [f32; 4]) {
        self.texts.push(FloatingText {
            text,
            pos,
            vel: Vec2::new(0.0, -90.0),
            life: 1.1,
            max_life: 1.1,
            scale,
            color,
        });
    }

    pub fn update(&mut self, dt: f32) {
        self.texts.retain_mut(|t| {
            t.life -= dt;
            t.pos += t.vel * dt;
            t.vel.y *= 0.97;
            t.life > 0.0
        });
    }
}

pub fn draw_floating_texts(
    ctx: &mut Context,
    canvas: &mut Canvas,
    system: &FloatingTextSystem,
) -> ggez::GameResult {
    use ggez::graphics::Text;
    for ft in &system.texts {
        let ratio = ft.life / ft.max_life;
        let alpha = (ft.color[3] * ratio).clamp(0.0, 1.0);
        let color = Color::new(ft.color[0], ft.color[1], ft.color[2], alpha);
        // Slight upward scale pop at start, shrinks as it fades
        let scale = ft.scale * (0.8 + 0.2 * ratio);
        let mut text = Text::new(&ft.text);
        text.set_scale(scale);
        canvas.draw(&text, DrawParam::default().dest(ft.pos).color(color));
    }
    Ok(())
}

pub fn draw_combo_meter(
    ctx: &mut Context,
    canvas: &mut Canvas,
    player_pos: Vec2,
    player_size: f32,
    combo_count: usize,
    combo_timer: f32,
    beat_intensity: f32,
    time: f32,
) -> ggez::GameResult {
    use ggez::graphics::Text;

    if combo_count < 3 {
        return Ok(());
    }

    // Determine multiplier tier
    let (multiplier_label, tier_color) = if combo_count >= 10 {
        ("x5", Color::new(0.8, 0.3, 1.0, 1.0))
    } else if combo_count >= 6 {
        ("x3", Color::new(1.0, 0.2, 0.2, 1.0))
    } else {
        ("x2", Color::new(1.0, 0.6, 0.1, 1.0))
    };

    let center = player_pos + Vec2::new(player_size / 2.0, player_size / 2.0);
    let radius = 36.0 + beat_intensity * 8.0;
    let fill_fraction = (combo_timer / 1.8).clamp(0.0, 1.0);
    let rotation_offset = time * 0.5;

    const SEGMENTS: usize = 32;
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    // Draw the main arc
    for i in 0..SEGMENTS {
        let t0 = i as f32 / SEGMENTS as f32;
        let t1 = (i + 1) as f32 / SEGMENTS as f32;
        if t0 >= fill_fraction {
            break;
        }
        let angle0 = rotation_offset + t0 * fill_fraction * std::f32::consts::TAU;
        let angle1 = rotation_offset + t1.min(fill_fraction) * fill_fraction * std::f32::consts::TAU;
        let p0 = center + Vec2::new(angle0.cos(), angle0.sin()) * radius;
        let p1 = center + Vec2::new(angle1.cos(), angle1.sin()) * radius;
        if p0.distance(p1) > 0.5 {
            let seg = Mesh::new_line(
                ctx,
                &[[p0.x, p0.y], [p1.x, p1.y]],
                3.0,
                tier_color,
            )?;
            canvas.draw(&seg, DrawParam::default());
        }
    }

    // Draw glow duplicate with larger radius and lower alpha
    let glow_radius = radius + 5.0;
    let glow_color = Color::new(tier_color.r, tier_color.g, tier_color.b, tier_color.a * 0.35);
    for i in 0..SEGMENTS {
        let t0 = i as f32 / SEGMENTS as f32;
        let t1 = (i + 1) as f32 / SEGMENTS as f32;
        if t0 >= fill_fraction {
            break;
        }
        let angle0 = rotation_offset + t0 * fill_fraction * std::f32::consts::TAU;
        let angle1 = rotation_offset + t1.min(fill_fraction) * fill_fraction * std::f32::consts::TAU;
        let p0 = center + Vec2::new(angle0.cos(), angle0.sin()) * glow_radius;
        let p1 = center + Vec2::new(angle1.cos(), angle1.sin()) * glow_radius;
        if p0.distance(p1) > 0.5 {
            let seg = Mesh::new_line(
                ctx,
                &[[p0.x, p0.y], [p1.x, p1.y]],
                6.0,
                glow_color,
            )?;
            canvas.draw(&seg, DrawParam::default());
        }
    }

    canvas.set_blend_mode(original_blend);

    // Draw multiplier text just above the player center
    let text_alpha = (0.7 + 0.3 * beat_intensity).clamp(0.0, 1.0);
    let text_color = Color::new(tier_color.r, tier_color.g, tier_color.b, text_alpha);
    let mut label = Text::new(multiplier_label);
    label.set_scale(22.0);
    let text_pos = center - Vec2::new(14.0, radius + 20.0);
    canvas.draw(&label, DrawParam::default().dest(text_pos).color(text_color));

    Ok(())
}

/// Draw screen-edge radar arrows pointing to free (uncaught) crabs.
/// Each arrow is a filled triangle sitting just inside the screen border,
/// rotated to point toward the crab. Color matches the crab type.
/// Arrows pulse in scale with `beat_intensity`.
pub fn draw_crab_radar(
    ctx: &mut Context,
    canvas: &mut Canvas,
    crabs: &[EnemyCrab],
    width: f32,
    height: f32,
    beat_intensity: f32,
    time: f32,
) -> ggez::GameResult {
    let margin = 22.0_f32;
    let base_size = 12.0_f32;
    let pulse = 1.0 + beat_intensity * 0.35 + (time * 6.0).sin() * 0.08;
    let arrow_size = base_size * pulse;

    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    for crab in crabs {
        if crab.caught {
            continue;
        }
        // Only show arrow if crab is near an edge (within margin*5) or fully off-screen
        let cx = crab.pos.x;
        let cy = crab.pos.y;
        let near_edge = cx < margin * 5.0
            || cx > width - margin * 5.0
            || cy < margin * 5.0
            || cy > height - margin * 5.0;
        if !near_edge {
            continue;
        }

        // Clamp the indicator to the screen edge
        let edge_x = cx.clamp(margin, width - margin);
        let edge_y = cy.clamp(margin, height - margin);

        // Direction from indicator position to actual crab position (points inward)
        let dir = Vec2::new(cx - edge_x, cy - edge_y);
        let angle = if dir.length() > 0.1 {
            dir.y.atan2(dir.x)
        } else {
            // crab is right at edge, just point inward from nearest edge
            let dx = cx - width / 2.0;
            let dy = cy - height / 2.0;
            dy.atan2(dx)
        };

        // Build a small equilateral triangle pointing in `angle` direction
        // tip is at (arrow_size, 0) in local space, base at (-arrow_size/2, ±arrow_size*0.75)
        let tip   = Vec2::new(angle.cos(), angle.sin()) * arrow_size;
        let left  = Vec2::new((angle + 2.2).cos(), (angle + 2.2).sin()) * arrow_size * 0.75;
        let right = Vec2::new((angle - 2.2).cos(), (angle - 2.2).sin()) * arrow_size * 0.75;
        let origin = Vec2::new(edge_x, edge_y);

        let [r, g, b] = crab.crab_color();
        // Add brightness boost so arrow reads even when washed out
        let brightness = 0.4 + beat_intensity * 0.3;
        let color = Color::new(
            (r + brightness).min(1.0),
            (g + brightness).min(1.0),
            (b + brightness).min(1.0),
            0.75 + beat_intensity * 0.2,
        );

        let pts = [
            [origin.x + tip.x,   origin.y + tip.y],
            [origin.x + left.x,  origin.y + left.y],
            [origin.x + right.x, origin.y + right.y],
        ];
        let triangle = Mesh::new_polygon(ctx, DrawMode::fill(), &pts, color)?;
        canvas.draw(&triangle, DrawParam::default());

        // Glow outline
        let glow_color = Color::new(r.min(1.0), g.min(1.0), b.min(1.0), 0.35 + beat_intensity * 0.15);
        let glow_pts = [
            [origin.x + tip.x   * 1.5, origin.y + tip.y   * 1.5],
            [origin.x + left.x  * 1.5, origin.y + left.y  * 1.5],
            [origin.x + right.x * 1.5, origin.y + right.y * 1.5],
        ];
        let glow = Mesh::new_polygon(ctx, DrawMode::fill(), &glow_pts, glow_color)?;
        canvas.draw(&glow, DrawParam::default());
    }

    canvas.set_blend_mode(original_blend);
    Ok(())
}

/// Draw expanding ghost rings for each crab in the conga chain.
/// Each ring is (center_pos, age 0..1, rgb color).
/// age=0 means just spawned (small, bright), age=1 means about to disappear (large, transparent).
pub fn draw_chain_rings(
    ctx: &mut Context,
    canvas: &mut Canvas,
    rings: &[(Vec2, f32, [f32; 3])],
) -> ggez::GameResult {
    if rings.is_empty() {
        return Ok(());
    }
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    for &(pos, age, color) in rings {
        // age 0..1: radius grows from 8 to 70, alpha fades from bright to zero
        let radius = 8.0 + age * 62.0;
        let alpha = ((1.0 - age) * 0.65).clamp(0.0, 1.0);
        // Stroke thickness tapers as ring expands
        let thickness = 3.5 * (1.0 - age * 0.7);

        // Main ring — rings spawned on the same beat share the same age every frame, so this
        // cache lookup is shared across the whole conga chain instead of building one fresh
        // mesh per crab per frame.
        let ring = cached_stroke_circle(ctx, radius, thickness)?;
        canvas.draw(
            &ring,
            DrawParam::default()
                .dest(pos)
                .color(Color::new(color[0], color[1], color[2], alpha)),
        );

        // Soft outer glow ring (larger radius, lower alpha)
        if age < 0.7 {
            let glow_alpha = alpha * 0.3;
            let glow = cached_stroke_circle(ctx, radius + 4.0, thickness * 2.0)?;
            canvas.draw(
                &glow,
                DrawParam::default()
                    .dest(pos)
                    .color(Color::new(color[0], color[1], color[2], glow_alpha)),
            );
        }
    }

    canvas.set_blend_mode(original_blend);
    Ok(())
}

/// Draw a snappy impact shockwave at each spot a crab was just caught. Unlike the
/// beat-synced ghost rings, these fire once per catch, expand fast and wide, and lead
/// with a white-hot edge that resolves into the crab's own color — a crisp "pop" of
/// feedback at the exact catch position.
pub fn draw_catch_shockwaves(
    ctx: &mut Context,
    canvas: &mut Canvas,
    shockwaves: &[(Vec2, f32, [f32; 3])],
) -> ggez::GameResult {
    if shockwaves.is_empty() {
        return Ok(());
    }
    let unit_circle = match UNIT_CIRCLE.get() {
        Some(mesh) => mesh,
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh)
        }
    };
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    for &(pos, age, color) in shockwaves {
        // Ease-out expansion: fast at first, then decelerating — reads as an impact snap.
        let ease = 1.0 - (1.0 - age).powi(2);
        let radius = 6.0 + ease * 120.0;
        let fade = (1.0 - age).clamp(0.0, 1.0);

        // Initial white-hot filled flash that grows and fades in the first fraction.
        if age < 0.22 {
            let flash_t = age / 0.22;
            let flash_alpha = (1.0 - flash_t) * 0.9;
            let flash_r = 10.0 + flash_t * 26.0;
            canvas.draw(
                unit_circle,
                DrawParam::default()
                    .dest(pos)
                    .scale(Vec2::splat(flash_r))
                    .color(Color::new(1.0, 1.0, 1.0, flash_alpha)),
            );
        }

        // Leading edge: white-hot early, blending toward the crab color as it expands.
        let edge_r = (color[0] * age + (1.0 - age)).min(1.0);
        let edge_g = (color[1] * age + (1.0 - age)).min(1.0);
        let edge_b = (color[2] * age + (1.0 - age)).min(1.0);
        let thickness = (5.0 * fade).max(1.0);
        let ring = cached_stroke_circle(ctx, radius, thickness)?;
        canvas.draw(
            &ring,
            DrawParam::default()
                .dest(pos)
                .color(Color::new(edge_r, edge_g, edge_b, fade * 0.95)),
        );

        // Soft trailing glow just inside the leading edge for extra body.
        if age < 0.8 {
            let glow = cached_stroke_circle(ctx, (radius - 6.0).max(1.0), thickness * 2.2)?;
            canvas.draw(
                &glow,
                DrawParam::default()
                    .dest(pos)
                    .color(Color::new(color[0], color[1], color[2], fade * 0.28)),
            );
        }
    }

    canvas.set_blend_mode(original_blend);
    Ok(())
}

/// Draw the cold "alarm" rings kicked off when a catch startles the surrounding herd
/// (the stampede ripple). Cyan/white and a little wider than the warm catch pop so the two
/// read as different events: warm = a crab joined, cold = the rest just bolted.
pub fn draw_fear_rings(
    ctx: &mut Context,
    canvas: &mut Canvas,
    rings: &[(Vec2, f32)],
) -> ggez::GameResult {
    if rings.is_empty() {
        return Ok(());
    }
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    for &(pos, age) in rings {
        // Ease-out expansion, wider reach than the catch shockwave (matches the startle radius).
        let ease = 1.0 - (1.0 - age).powi(2);
        let radius = 8.0 + ease * 135.0;
        let fade = (1.0 - age).clamp(0.0, 1.0);

        // Bright leading edge, cyan-white.
        let thickness = (4.0 * fade).max(1.0);
        let ring = cached_stroke_circle(ctx, radius, thickness)?;
        canvas.draw(
            &ring,
            DrawParam::default()
                .dest(pos)
                .color(Color::new(0.55, 0.9, 1.0, fade * 0.85)),
        );

        // Faint inner echo for a double-pulse "alarm" feel.
        if age < 0.75 {
            let echo = cached_stroke_circle(ctx, (radius - 14.0).max(1.0), thickness * 1.6)?;
            canvas.draw(
                &echo,
                DrawParam::default()
                    .dest(pos)
                    .color(Color::new(0.35, 0.7, 1.0, fade * 0.3)),
            );
        }
    }

    canvas.set_blend_mode(original_blend);
    Ok(())
}

/// Draw the expanding sonic ring of the Whistle ability — a warm double-pulse that sweeps out from
/// the player and yanks nearby crabs in. `radius` is the current front, `max_radius` its reach;
/// alpha fades as the front nears its limit so the ring dissolves rather than snapping off.
pub fn draw_whistle_ring(
    ctx: &mut Context,
    canvas: &mut Canvas,
    center: Vec2,
    radius: f32,
    max_radius: f32,
) -> ggez::GameResult {
    if radius <= 0.0 {
        return Ok(());
    }
    let frac = (radius / max_radius).clamp(0.0, 1.0);
    let fade = 1.0 - frac; // bright at the cast, gone by full reach

    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    // Leading edge — bright amber, like a horn blast made visible.
    let thickness = (6.0 * fade + 1.5).max(1.5);
    let front = cached_stroke_circle(ctx, radius, thickness)?;
    canvas.draw(
        &front,
        DrawParam::default()
            .dest(center)
            .color(Color::new(1.0, 0.82, 0.35, (fade * 0.9).clamp(0.0, 1.0))),
    );

    // A couple of trailing echo rings for a "wub" of concentric pulses chasing the front.
    for (offset, alpha_scale) in [(26.0_f32, 0.45_f32), (54.0_f32, 0.22_f32)] {
        let er = radius - offset;
        if er > 2.0 {
            let echo = cached_stroke_circle(ctx, er, thickness * 0.7)?;
            canvas.draw(
                &echo,
                DrawParam::default().dest(center).color(Color::new(
                    1.0,
                    0.7,
                    0.3,
                    (fade * alpha_scale).clamp(0.0, 1.0),
                )),
            );
        }
    }

    canvas.set_blend_mode(original_blend);
    Ok(())
}

/// Draw a pulsing attraction halo around a crab that is inside the flashlight beam.
/// `crab_color` is [r, g, b] 0..1. `time` is total elapsed seconds. `beat_intensity` 0..1.
pub fn draw_attracted_crab_glow(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    size: f32,
    crab_color: [f32; 3],
    time: f32,
    beat_intensity: f32,
) -> ggez::GameResult {
    // Pulse: fast sine wave (3 Hz) scaled up on beat
    let pulse = (time * 3.0 * std::f32::consts::TAU).sin() * 0.5 + 0.5; // 0..1
    let pulse = pulse * (0.7 + beat_intensity * 0.3);

    let base_radius = size * 0.9;
    let outer_radius = base_radius + 6.0 + pulse * 9.0;

    let [r, g, b] = crab_color;

    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    // Outer soft glow ring — attracted crabs tend to share similar size/pulse phase (same
    // global beat clock), so this cache lookup is often shared across every glowing crab
    // instead of building a fresh GPU mesh per crab per frame.
    let glow_alpha = (0.18 + pulse * 0.22).clamp(0.0, 1.0);
    let glow = cached_stroke_circle(ctx, outer_radius + outer_radius * 0.18, outer_radius * 0.35)?;
    canvas.draw(
        &glow,
        DrawParam::default()
            .dest(pos)
            .color(Color::new(r, g, b, glow_alpha)),
    );

    // Bright inner ring
    let ring_alpha = (0.45 + pulse * 0.45).clamp(0.0, 1.0);
    let ring = cached_stroke_circle(ctx, outer_radius, 2.5)?;
    canvas.draw(
        &ring,
        DrawParam::default().dest(pos).color(Color::new(
            (r * 0.5 + 0.5).min(1.0),
            (g * 0.5 + 0.5).min(1.0),
            (b * 0.5 + 0.5).min(1.0),
            ring_alpha,
        )),
    );

    canvas.set_blend_mode(original_blend);
    Ok(())
}
