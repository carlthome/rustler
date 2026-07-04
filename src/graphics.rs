use crate::enemies::{CrabType, EnemyCrab};
use crate::{CRAB_SIZE, Flashlight, PLAYER_SIZE};
use crevice::std140::AsStd140;
use ggez::Context;
use ggez::glam::Vec2;
use ggez::graphics::{
    BlendMode, Canvas, Color, DrawMode, DrawParam, Image, Mesh, Rect, Shader, ShaderParamsBuilder,
};
use rand::Rng;

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
        
        let particle_mesh = Mesh::new_circle(
            ctx,
            DrawMode::fill(),
            [0.0, 0.0],
            particle.size,
            0.1,
            color,
        )?;
        
        canvas.draw(&particle_mesh, DrawParam::default().dest(particle.pos));
        
        // Add a subtle glow effect for larger particles
        if particle.size > 4.0 {
            let glow_color = Color::new(
                particle.color[0],
                particle.color[1], 
                particle.color[2],
                alpha * 0.3,
            );
            
            let glow_mesh = Mesh::new_circle(
                ctx,
                DrawMode::fill(),
                [0.0, 0.0],
                particle.size * 1.5,
                0.1,
                glow_color,
            )?;
            
            canvas.draw(&glow_mesh, DrawParam::default().dest(particle.pos));
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
) -> ggez::GameResult {
    let blend_mode = canvas.blend_mode();
    let solid_bg = Mesh::new_rectangle(
        ctx,
        DrawMode::fill(),
        Rect::new(0.0, 0.0, width, height),
        Color::from_rgb(0, 100, 0),
    )?;
    canvas.draw(&solid_bg, DrawParam::default());

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
    canvas.set_blend_mode(blend_mode);
    Ok(())
}

pub fn draw_rustler(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    sprite: &Image,
) -> ggez::GameResult {
    let color = Color::from_rgba(255, 255, 255, 255);

    // Offset the sprite a little bit.
    let offset = Vec2 { x: 15.0, y: 15.0 };
    canvas.draw(
        sprite,
        DrawParam::default()
            .dest(pos + offset)
            .color(color)
            .scale(Vec2 { x: 0.05, y: 0.05 }),
    );

    Ok(())
}

pub fn draw_crab(ctx: &mut Context, canvas: &mut Canvas, crab: &EnemyCrab, draw_pos: Vec2, beat_phase: f32, join_pulse: f32) -> ggez::GameResult {
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

    // Color: more red as crab ages, and different color for type
    let [r, g, b] = crab.crab_color();
    let flash = if join_pulse > 0.0 && join_pulse <= 1.0 {
        join_pulse * (1.0 - join_pulse) * 4.0 * 0.5  // peak 0.5 at pulse=0.5
    } else {
        0.0
    };
    let crab_color = Color::new((r + flash).min(1.0), (g + flash).min(1.0), (b + flash).min(1.0), 1.0);

    // Crab body
    let crab_body = Mesh::new_circle(
        ctx,
        DrawMode::fill(),
        [0.0, 0.0],
        size / 2.0,
        0.5,
        crab_color,
    )?;

    // Crab legs (6 lines)
    let mut leg_meshes = Vec::new();
    let leg_len = size * 0.7;
    let leg_color = Color::from_rgb(200, 50, 50);
    for i in 0..6 {
        let base_angle = std::f32::consts::PI * (0.25 + i as f32 / 6.0);
        let time = ctx.time.time_since_start().as_secs_f32();
        let phase = (crab.pos.x + crab.pos.y) * 0.05;
        let wiggle_speed = 2.0 + crab.speed * 0.08; // scale with crab speed
        let wiggle_amp = 0.18 + beat_phase * 0.12;
        let wiggle = (time * wiggle_speed * (1.0 + beat_phase * 0.5) + phase + i as f32).sin() * wiggle_amp;
        let angle = base_angle + wiggle;
        let x1 = (size / 2.0) * angle.cos();
        let y1 = (size / 2.0) * angle.sin();
        let x2 = (size / 2.0 + leg_len) * angle.cos();
        let y2 = (size / 2.0 + leg_len) * angle.sin();
        let leg = Mesh::new_line(ctx, &[[x1, y1], [x2, y2]], 2.0, leg_color)?;
        leg_meshes.push(leg);
    }

    // Crab claws (small circles)
    let claw_offset = size * 0.7;
    let claw_radius = size * 0.18;
    let left_claw = Mesh::new_circle(
        ctx,
        DrawMode::fill(),
        [-(claw_offset), -(claw_offset * 0.3)],
        claw_radius,
        0.5,
        crab_color,
    )?;
    let right_claw = Mesh::new_circle(
        ctx,
        DrawMode::fill(),
        [claw_offset, -(claw_offset * 0.3)],
        claw_radius,
        0.5,
        crab_color,
    )?;

    // Draw all parts at crab.pos
    canvas.draw(&crab_body, DrawParam::default().dest(draw_pos));
    for leg in &leg_meshes {
        canvas.draw(leg, DrawParam::default().dest(draw_pos));
    }
    canvas.draw(&left_claw, DrawParam::default().dest(draw_pos));
    canvas.draw(&right_claw, DrawParam::default().dest(draw_pos));

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
    let lw = Mesh::new_circle(ctx, DrawMode::fill(), [-eye_x, eye_y], eye_radius, 0.3, Color::WHITE)?;
    let rw = Mesh::new_circle(ctx, DrawMode::fill(), [eye_x, eye_y], eye_radius, 0.3, Color::WHITE)?;
    let lp = Mesh::new_circle(ctx, DrawMode::fill(), [-eye_x + pdx, eye_y + pdy], pupil_r, 0.3, Color::BLACK)?;
    let rp = Mesh::new_circle(ctx, DrawMode::fill(), [eye_x + pdx, eye_y + pdy], pupil_r, 0.3, Color::BLACK)?;
    canvas.draw(&lw, DrawParam::default().dest(draw_pos));
    canvas.draw(&rw, DrawParam::default().dest(draw_pos));
    canvas.draw(&lp, DrawParam::default().dest(draw_pos));
    canvas.draw(&rp, DrawParam::default().dest(draw_pos));

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

                if prev_point.distance(point) > 0.5 {
                    let seg_line = Mesh::new_line(
                        ctx,
                        &[[prev_point.x, prev_point.y], [point.x, point.y]],
                        thickness,
                        color,
                    )?;
                    canvas.draw(&seg_line, DrawParam::default());

                    // Thinner glow pass with additive blend for a neon look
                    let glow_color = Color::new(rr, gg, bb, alpha_base * 0.35);
                    let glow_line = Mesh::new_line(
                        ctx,
                        &[[prev_point.x, prev_point.y], [point.x, point.y]],
                        thickness * 2.2,
                        glow_color,
                    )?;
                    canvas.set_blend_mode(BlendMode::ADD);
                    canvas.draw(&glow_line, DrawParam::default());
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
    let base_r = 20.0;
    let pulse_r = base_r + beat_intensity * 14.0;
    let alpha = ((80.0 + beat_intensity * 175.0) as u8).min(255);
    let outer = Mesh::new_circle(
        ctx, DrawMode::fill(), [0.0, 0.0], pulse_r, 0.5,
        Color::from_rgba(255, 200, 50, alpha),
    )?;
    canvas.draw(&outer, DrawParam::default().dest(center));
    let inner = Mesh::new_circle(
        ctx, DrawMode::fill(), [0.0, 0.0], base_r * 0.55, 0.5,
        Color::from_rgba(255, 140, 50, 220),
    )?;
    canvas.draw(&inner, DrawParam::default().dest(center));
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
