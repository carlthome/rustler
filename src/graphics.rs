use crate::enemies::{CrabType, EnemyCrab};
use crate::{CRAB_SIZE, Flashlight, PLAYER_SIZE};
use crevice::std140::AsStd140;
use ggez::Context;
use ggez::glam::Vec2;
use ggez::graphics::{
    BlendMode, Canvas, Color, DrawMode, DrawParam, Image, InstanceArray, Mesh, Rect, Shader,
    ShaderParamsBuilder,
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

    // Cache of fill-rectangle meshes keyed by (x, y, w, h) quantized plus the RGBA color,
    // for rects whose exact geometry can't just be a scaled UNIT_SQUARE (the full-screen
    // shader quads in `draw_grass`/`draw_flashlight` bake actual screen pixel offsets into
    // their vertex positions, since the custom vertex shaders consume raw mesh-local
    // position directly as clip space). These two quads plus the flashlight's small torso
    // rect were being rebuilt (fresh Vec + fresh GPU buffer) every single frame regardless
    // of whether anything on screen changed, on every frame of gameplay — the worst kind of
    // per-frame allocation since it's unconditional. Resolution only changes on window
    // resize, so in practice this cache stays at 2-3 entries for the life of the process.
    static FILL_RECT_CACHE: RefCell<HashMap<(i32, i32, i32, i32, u32), Mesh>> = RefCell::new(HashMap::new());

    // Scratch buffer for `draw_conga_rope`'s per-micro-segment geometry (position, rotation,
    // length, rgb), persisted and `clear()`-ed each frame instead of a fresh `Vec` allocation.
    // The rope used to draw its main segment then immediately flip to additive blend for the
    // glow segment and flip back, every single micro-segment (SEGS=14 per link) — on a long
    // conga train that's hundreds of blend-mode switches a frame, each one breaking ggez's
    // draw-call batching. Buffering the geometry lets both passes run back-to-back with only
    // two blend-mode switches total, no matter how long the chain gets.
    static CONGA_SEGMENT_BUF: RefCell<Vec<(Vec2, f32, f32, [f32; 3])>> = RefCell::new(Vec::new());

    // Scratch buffer for `draw_conga_rope`'s player->crab0->crab1->... waypoint list, persisted
    // and cleared each frame instead of a fresh `Vec::with_capacity` allocation. Grows with chain
    // length just like CONGA_SEGMENT_BUF above, so on a long train this was a real per-frame heap
    // allocation on top of the (already-fixed) segment buffer.
    static CONGA_WAYPOINT_BUF: RefCell<Vec<Vec2>> = RefCell::new(Vec::new());

    // Cache of the lasso's spinning open-loop ring mesh, keyed by rounded (radius, thickness).
    // Built once in local space (centered at the origin, sweeping `LASSO_LOOP_ARC_FRACTION` of a
    // circle starting at angle 0) and reused every frame via `DrawParam::rotation` to spin it and
    // `.dest` to place it at the lasso tip. The lasso is one of the most-used actions in the game
    // (thrown on basically every catch attempt), and this ring used to rebuild a fresh 21-point
    // Vec plus two fresh `Mesh::new_line` GPU buffers every single frame it was in flight.
    static LASSO_LOOP_CACHE: RefCell<HashMap<(i32, i32), Mesh>> = RefCell::new(HashMap::new());

    // Reusable instance buffers for the particle system's two draw passes (main dot + soft glow
    // for larger particles). Milestone fireworks/catch bursts/dash trails can push close to
    // MAX_PARTICLES (900) live particles at once, and each one used to cost its own
    // `canvas.draw` call — a separate uniform-buffer allocation, bind group and `draw_indexed`
    // submission per particle per pass (ggez does NOT batch consecutive `canvas.draw(&same_mesh,
    // ...)` calls; only `InstanceArray` is truly instanced). That was up to ~1800 GPU draw calls
    // a frame just for particles. Filling one `InstanceArray` per pass and issuing a single
    // `draw_instanced_mesh` collapses that to two draw calls total, independent of particle
    // count, with identical on-screen output (same mesh, same blend mode, no rotation to lose).
    static PARTICLE_MAIN_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    static PARTICLE_GLOW_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

    // Crab legs (6 unit-line draws per crab) were the single biggest per-crab draw-call
    // contributor — a long conga train plus a fresh wild herd can easily put 40-50+ crabs on
    // screen at once, i.e. 240-300+ individual leg draw calls a frame on top of everything else
    // draw_crab issues. draw_crab() pushes its 6 leg DrawParams here instead of drawing them
    // immediately; flush_crab_legs() (called once per crab-drawing pass) fills one InstanceArray
    // and issues a single draw_instanced_mesh, the same technique already used for particles.
    // Legs still land at the same world position/rotation/color, so this is purely a batching
    // change — no visible difference, just far fewer GPU submissions.
    static CRAB_LEG_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static CRAB_LEG_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
}

/// Draw (and clear) every leg DrawParam accumulated by draw_crab() calls since the last flush, as
/// a single instanced batch. Call this once after all draw_crab() calls in a drawing pass (e.g.
/// once per frame in draw_crabs_with_shake) so legs still land in the same relative draw order —
/// after bodies, before the claw/eye overlays each draw_crab() call still draws immediately.
pub fn flush_crab_legs(ctx: &mut Context, canvas: &mut Canvas) -> ggez::GameResult {
    CRAB_LEG_PARAMS.with(|params_cell| -> ggez::GameResult {
        let mut params = params_cell.borrow_mut();
        if params.is_empty() {
            return Ok(());
        }
        let unit_line = match UNIT_LINE.get() {
            Some(mesh) => mesh.clone(),
            None => {
                let mesh = Mesh::new_rectangle(
                    ctx,
                    DrawMode::fill(),
                    Rect::new(0.0, -0.5, 1.0, 1.0),
                    Color::WHITE,
                )?;
                UNIT_LINE.get_or_init(|| mesh).clone()
            }
        };
        CRAB_LEG_INSTANCES.with(|inst_cell| -> ggez::GameResult {
            let mut inst_slot = inst_cell.borrow_mut();
            let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
            instances.set(params.iter().copied());
            canvas.draw_instanced_mesh(unit_line, instances, DrawParam::default());
            Ok(())
        })?;
        params.clear();
        Ok(())
    })
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
/// existing `UNIT_CIRCLE`/`UNIT_LINE` fill meshes. Public so one-off ring effects driven from
/// main.rs (e.g. the beat-wave expanding outline) can reuse it instead of building a fresh
/// `Mesh::new_circle` every frame they're active.
pub fn cached_stroke_circle(ctx: &mut Context, radius: f32, thickness: f32) -> ggez::GameResult<Mesh> {
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

// Fraction of a full circle the lasso's spinning loop covers (leaves a gap so it reads as an
// open lasso loop rather than a closed ring). Shared between the mesh builder below and
// `draw_lasso`'s doc comment.
const LASSO_LOOP_ARC_FRACTION: f32 = 0.88;
const LASSO_LOOP_SEGMENTS: usize = 20;

/// Fetch a cached lasso-loop mesh for the given radius/thickness (built once per rounded key).
/// The mesh is built in local space starting at angle 0 and sweeping `LASSO_LOOP_ARC_FRACTION`
/// of a full circle — callers spin it by passing a `.rotation(spin)` `DrawParam` (rotating local
/// points by `spin` around the origin reproduces the old per-frame `angle = spin + t*frac*TAU`
/// computation exactly) and place it via `.dest(tip)`.
fn cached_lasso_loop(ctx: &mut Context, radius: f32, thickness: f32) -> ggez::GameResult<Mesh> {
    let radius = radius.max(0.5);
    let thickness = thickness.max(0.25);
    let key = ((radius * 2.0).round() as i32, (thickness * 4.0).round() as i32);

    if let Some(mesh) = LASSO_LOOP_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return Ok(mesh);
    }

    let pts: Vec<[f32; 2]> = (0..=LASSO_LOOP_SEGMENTS)
        .map(|s| {
            let angle = (s as f32 / LASSO_LOOP_SEGMENTS as f32) * LASSO_LOOP_ARC_FRACTION * std::f32::consts::TAU;
            [angle.cos() * radius, angle.sin() * radius]
        })
        .collect();
    let mesh = Mesh::new_line(ctx, &pts, thickness, Color::WHITE)?;
    LASSO_LOOP_CACHE.with(|c| c.borrow_mut().insert(key, mesh.clone()));
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

/// Fetch the cached unit-line mesh (a 1x1 rect centered on the x-axis, spanning x in [0,1]),
/// building it once on first use. Place a line segment of `length`/`thickness` from `origin` in
/// direction `dir` via `.dest(origin).rotation(dir.y.atan2(dir.x)).scale((length, thickness))`
/// instead of calling `Mesh::new_line` fresh every draw — the same trick `UNIT_CIRCLE`/
/// `UNIT_SQUARE` use. Baked with `Color::WHITE`; tint via `DrawParam::color`.
pub fn unit_line(ctx: &mut Context) -> ggez::GameResult<&'static Mesh> {
    match UNIT_LINE.get() {
        Some(mesh) => Ok(mesh),
        None => {
            let mesh = Mesh::new_rectangle(ctx, DrawMode::fill(), Rect::new(0.0, -0.5, 1.0, 1.0), Color::WHITE)?;
            Ok(UNIT_LINE.get_or_init(|| mesh))
        }
    }
}

/// Fetch the cached unit-circle mesh (radius 1, centered at the origin), building it once on
/// first use. Scale by `(r, r)` and set `.dest((x, y))` to place a filled circle of any size/
/// color without allocating a fresh `Mesh::new_circle` GPU buffer — the same trick
/// `UNIT_SQUARE`/`UNIT_LINE` use. Baked with `Color::WHITE`; tint via `DrawParam::color`. Public
/// so one-off fill-circle effects driven from outside graphics.rs (e.g. the menu screen's stars/
/// moon) can reuse the same mesh internal particle/ring drawing already relies on instead of each
/// keeping its own private copy of the `UNIT_CIRCLE.get_or_init` dance.
pub fn unit_circle(ctx: &mut Context) -> ggez::GameResult<&'static Mesh> {
    match UNIT_CIRCLE.get() {
        Some(mesh) => Ok(mesh),
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            Ok(UNIT_CIRCLE.get_or_init(|| mesh))
        }
    }
}

/// Draw the dash speed-line wake trailing behind the player: a small fan of short streaks in
/// the direction the player just came from, brighter the more recently the dash started. Reuses
/// the cached unit-line mesh (scaled/rotated per streak via `DrawParam`) instead of building a
/// fresh `Mesh::new_line` GPU buffer per streak per frame — this used to be up to 7 fresh line
/// allocations every single frame for the whole dash window.
pub fn draw_speed_lines(
    ctx: &mut Context,
    canvas: &mut Canvas,
    center: Vec2,
    last_dir: Vec2,
    intensity: f32,
) -> ggez::GameResult {
    if last_dir.length() < 0.01 {
        return Ok(());
    }
    let line = unit_line(ctx)?;
    let wake = -last_dir.normalize();
    let angle = wake.y.atan2(wake.x);
    let perp = Vec2::new(-wake.y, wake.x);
    let alpha = (intensity.clamp(0.0, 1.0) * 110.0) as u8;
    for i in 0i32..7 {
        let t = (i as f32 - 3.0) / 3.0;
        let origin = center + perp * (t * 14.0);
        let length = 20.0 + (3.0 - (i as f32 - 3.0).abs()) * 8.0;
        canvas.draw(
            line,
            DrawParam::default()
                .dest(origin)
                .rotation(angle)
                .scale(Vec2::new(length, 1.5))
                .color(Color::from_rgba(190, 215, 255, alpha)),
        );
    }
    Ok(())
}

/// Draw the beat-wave's expanding ring outline. Reuses `cached_stroke_circle` instead of
/// building a fresh `Mesh::new_circle` GPU buffer every frame the wave is expanding.
pub fn draw_beat_wave_ring(
    ctx: &mut Context,
    canvas: &mut Canvas,
    center: Vec2,
    radius: f32,
) -> ggez::GameResult {
    let alpha = ((1.0 - radius / 300.0).clamp(0.0, 1.0) * 150.0) as u8;
    let ring = cached_stroke_circle(ctx, radius, 3.0)?;
    canvas.draw(
        &ring,
        DrawParam::default()
            .dest(center)
            .color(Color::from_rgba(255, 200, 100, alpha)),
    );
    Ok(())
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

/// Fetch a cached fill-rectangle mesh built at the exact `(x, y, w, h)` offset/size given, in
/// `color` — for the handful of rects that need real (non-unit) vertex positions baked in,
/// instead of a fresh `Mesh::new_rectangle` GPU buffer every single frame. Unlike
/// `unit_square`, this does NOT get scaled/positioned via `DrawParam`; draw it with
/// `DrawParam::default()` (or whatever transform the caller already used), matching how the
/// mesh used to be built fresh each time.
pub fn cached_fill_rect(ctx: &mut Context, x: f32, y: f32, w: f32, h: f32, color: Color) -> ggez::GameResult<Mesh> {
    let key = (
        (x * 2.0).round() as i32,
        (y * 2.0).round() as i32,
        (w * 2.0).round() as i32,
        (h * 2.0).round() as i32,
        color.to_rgba_u32(),
    );

    if let Some(mesh) = FILL_RECT_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return Ok(mesh);
    }

    let mesh = Mesh::new_rectangle(ctx, DrawMode::fill(), Rect::new(x, y, w, h), color)?;
    FILL_RECT_CACHE.with(|c| c.borrow_mut().insert(key, mesh.clone()));
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

// Hard ceiling on live particles. Several emitters (beat pulses, conga dust) scale their
// spawn rate with conga-chain length, which grows unbounded over a long run — without a cap,
// a big train can pile up thousands of live sparkles and tank frame time even though each
// individual draw call is cheap on its own. Capping here (instead of per-emitter) keeps every
// effect's look identical for normal-sized trains and only kicks in once the screen is already
// visually saturated with particles, so it reads as "same effect" rather than "less effect."
const MAX_PARTICLES: usize = 900;

impl ParticleSystem {
    pub fn new() -> Self {
        Self {
            particles: Vec::new(),
        }
    }

    /// Push a particle unless the live-particle budget is already spent. Newest triggers (the
    /// catch/beat/dash the player just caused) matter most, so once at the cap we simply drop
    /// further spawns for this frame rather than evicting older ones — cheaper than a shift,
    /// and anything dropped would already be buried in the crowd.
    #[inline]
    pub(crate) fn push(&mut self, particle: Particle) {
        if self.particles.len() < MAX_PARTICLES {
            self.particles.push(particle);
        }
    }

    pub fn spawn_catch_effect(&mut self, pos: Vec2, crab_color: [f32; 3], crab_type: CrabType, rng: &mut impl Rng) {
        let (particle_count, speed_range, size_range, special_effect) = match crab_type {
            CrabType::Normal => (20, 80.0..180.0, 3.0..6.0, false),
            CrabType::Fast => (35, 120.0..300.0, 2.0..5.0, true), // More particles, faster
            CrabType::Big => (40, 60.0..150.0, 4.0..10.0, false), // Larger particles
            CrabType::Sneaky => (15, 100.0..250.0, 1.5..4.0, true), // Fewer, sneaky particles
            CrabType::Armored => (40, 60.0..150.0, 4.0..10.0, false), // Chunky, shell-cracking burst
            CrabType::Dancer => (30, 110.0..280.0, 2.0..5.0, true), // Lively disco confetti burst
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
            
            self.push(Particle {
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
                CrabType::Dancer => 14,
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
                    CrabType::Dancer => [1.0, 0.5, 0.95], // Hot-pink disco confetti
                    _ => [1.0, 1.0, 0.9],
                };
                
                self.push(Particle {
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
                
                self.push(Particle {
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
            self.push(Particle {
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
        self.push(Particle {
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
            self.push(Particle {
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
                self.push(Particle {
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

    /// A soft warm puff of gently-rising motes off a crab the whistle just talked down out of a
    /// panic — the calming counterpart to the cold "!" alarm rings the stampede throws.
    pub fn spawn_soothe_puff(&mut self, pos: Vec2, rng: &mut impl Rng) {
        for _ in 0..6 {
            let angle = rng.random_range(0.0_f32..std::f32::consts::TAU);
            let speed = rng.random_range(12.0_f32..40.0_f32);
            // Drift outward but bias upward so the puff wafts off like a settling sigh.
            let vel = Vec2::new(angle.cos() * speed, angle.sin() * speed - rng.random_range(20.0_f32..55.0_f32));
            let life = rng.random_range(0.5_f32..0.95_f32);
            self.push(Particle {
                pos,
                vel,
                life,
                max_life: life,
                size: rng.random_range(2.5_f32..5.0_f32),
                color: [1.0, 0.82 + rng.random_range(-0.08_f32..0.08_f32), 0.42],
            });
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
            self.push(Particle {
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
            self.push(Particle {
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
        Some(mesh) => mesh.clone(),
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh).clone()
        }
    };

    if particle_system.particles.is_empty() {
        return Ok(());
    }

    // Set additive blend mode for glowing effect
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    PARTICLE_MAIN_INSTANCES.with(|main_cell| -> ggez::GameResult {
        PARTICLE_GLOW_INSTANCES.with(|glow_cell| -> ggez::GameResult {
            let mut main_slot = main_cell.borrow_mut();
            let main = main_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
            let mut glow_slot = glow_cell.borrow_mut();
            let glow = glow_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));

            main.set(particle_system.particles.iter().map(|particle| {
                let life_ratio = particle.life / particle.max_life;
                let alpha = (life_ratio * 0.8).clamp(0.0, 1.0);
                let color = Color::new(
                    particle.color[0],
                    particle.color[1],
                    particle.color[2],
                    alpha,
                );
                DrawParam::default()
                    .dest(particle.pos)
                    .scale(Vec2::splat(particle.size))
                    .color(color)
            }));

            glow.set(particle_system.particles.iter().filter(|p| p.size > 4.0).map(|particle| {
                let life_ratio = particle.life / particle.max_life;
                let alpha = (life_ratio * 0.8).clamp(0.0, 1.0);
                let glow_color = Color::new(
                    particle.color[0],
                    particle.color[1],
                    particle.color[2],
                    alpha * 0.3,
                );
                DrawParam::default()
                    .dest(particle.pos)
                    .scale(Vec2::splat(particle.size * 1.5))
                    .color(glow_color)
            }));

            canvas.draw_instanced_mesh(unit_circle.clone(), main, DrawParam::default());
            canvas.draw_instanced_mesh(unit_circle, glow, DrawParam::default());
            Ok(())
        })
    })?;

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
    let quad = cached_fill_rect(ctx, -width / 2.0, -height / 2.0, width, height, Color::RED)?;
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

    // Shell shading: give the flat body circle a rounded, lit look. Light comes from a fixed
    // screen-space direction (up and slightly left) so the whole herd reads as lit from the same
    // sky, independent of each crab's facing rotation — hence these offsets are NOT rotated.
    let light_dir = Vec2::new(-0.4, -0.72);
    // Domed highlight: a smaller, brighter disc pushed toward the light makes the body read as a
    // rounded shell rather than a paper cut-out.
    let hi = |c: f32| (c + (1.0 - c) * 0.34).min(1.0);
    let dome_color = Color::new(hi(crab_color.r), hi(crab_color.g), hi(crab_color.b), 0.85);
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(draw_pos + light_dir * size * 0.15)
            .scale(Vec2::splat(size / 2.0 * 0.62))
            .color(dome_color),
    );
    // Glossy specular glint near the top of the shell — a tiny bright dot that catches the eye and
    // pulses faintly with the beat so the herd shimmers on the downbeat.
    let glint_a = 0.5 + beat_phase * 0.35;
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(draw_pos + light_dir * size * 0.26)
            .scale(Vec2::splat(size / 2.0 * 0.2))
            .color(Color::new(1.0, 1.0, 1.0, glint_a)),
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
        // Deferred: collected here and drawn as one instanced batch by flush_crab_legs() instead
        // of an individual canvas.draw() per leg per crab (see CRAB_LEG_PARAMS above).
        CRAB_LEG_PARAMS.with(|params| {
            params.borrow_mut().push(
                DrawParam::default()
                    .dest(root)
                    .rotation(angle)
                    .scale(Vec2::new(leg_len, 2.0))
                    .color(leg_color),
            );
        });
    }

    // Crab claws (small circles)
    let claw_offset = size * 0.7;
    let claw_radius = size * 0.18;
    let claw_l = draw_pos + rotate_offset(-(claw_offset), -(claw_offset * 0.3));
    let claw_r = draw_pos + rotate_offset(claw_offset, -(claw_offset * 0.3));
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(claw_l)
            .scale(Vec2::splat(claw_radius))
            .color(crab_color),
    );
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(claw_r)
            .scale(Vec2::splat(claw_radius))
            .color(crab_color),
    );
    // Matching lit highlight on each claw so they look like the same rounded shell as the body.
    for claw_pos in [claw_l, claw_r] {
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(claw_pos + light_dir * claw_radius * 0.5)
                .scale(Vec2::splat(claw_radius * 0.55))
                .color(dome_color),
        );
    }

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
    let flashlight_quad = cached_fill_rect(
        ctx,
        -screen_width / 2.0,
        -screen_height / 2.0,
        screen_width,
        screen_height,
        Color::WHITE,
    )?;

    // Set additive blend mode for the flashlight effect
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);
    canvas.draw(&flashlight_quad, DrawParam::default());

    let rotation = dir.y.atan2(dir.x) + std::f32::consts::PI / 2.0;

    // Draw flashlight body.
    let flashlight_body = cached_fill_rect(ctx, -5.0, 0.0, 10.0, 24.0, Color::BLACK)?;
    canvas.draw(
        &flashlight_body,
        DrawParam::default().dest(center).rotation(rotation),
    );

    // --- Volumetric dust motes drifting inside the beam ---
    // Cheap procedural "god-ray dust": a fixed set of specks, each riding a straight ray out
    // from the flashlight, twinkling and recycling at the far end so the cone reads as lit
    // airborne dust rather than a flat gradient. Every mote's position/brightness is a pure
    // function of its index and `time`, so this stays allocation-free and reuses the shared
    // cached unit circle. Switch back to the default shader first (the flashlight shader is
    // screen-space and ignores mesh colour), but keep the ADD blend so the motes glow.
    canvas.set_default_shader();
    let unit_circle = match UNIT_CIRCLE.get() {
        Some(mesh) => mesh,
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh)
        }
    };
    // Fresh-catch flare: the beam briefly sparkles brighter right after grabbing a crab.
    let catch_flare = (0.6 - time_since_catch).max(0.0) / 0.6 * 0.8;
    let half_spread = spread * 0.5 * 0.9; // keep motes just inside the visible cone edge
    const MOTE_COUNT: usize = 20;
    let hash = |n: f32| -> f32 {
        let s = (n * 12.9898).sin() * 43758.5453;
        s - s.floor()
    };
    for i in 0..MOTE_COUNT {
        let fi = i as f32;
        // Stable per-mote randoms.
        let lateral = hash(fi + 1.0) * 2.0 - 1.0; // where across the cone this ray sits
        let speed = 0.35 + hash(fi + 2.0) * 0.65; // how fast it drifts outward
        let seed = hash(fi + 3.0); // phase / twinkle offset
        let size = 1.2 + hash(fi + 4.0) * 1.6; // mote radius in px
        // Drift outward along the beam and recycle at the far end.
        let dfrac_raw = seed + time * speed * 0.14;
        let dfrac = dfrac_raw - dfrac_raw.floor(); // 0..1 distance fraction
        let dist = dfrac * flashlight_len * 1.02;
        // Cone widens with distance: motes near the apex hug the axis, far ones fan out.
        let mote_angle = angle + lateral * half_spread * (0.25 + 0.75 * dfrac);
        let pos = center + Vec2::new(mote_angle.cos(), mote_angle.sin()) * dist;
        // Brightness: fade in from the apex and out at the far edge, dim toward the cone
        // sides, and twinkle over time so the dust shimmers.
        let along_fade = (dfrac * std::f32::consts::PI).sin(); // 0 at both ends, 1 mid-beam
        let edge_fade = 1.0 - lateral * lateral; // dim near the cone's sides
        let twinkle = 0.45 + 0.55 * (time * (2.0 + seed * 3.0) + fi).sin();
        let alpha = (0.22 + catch_flare * 0.35) * along_fade * edge_fade * twinkle;
        if alpha <= 0.01 {
            continue;
        }
        let r = size + catch_flare * 0.8;
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::new(r, r))
                .color(Color::new(1.0, 0.96, 0.82, alpha.clamp(0.0, 1.0))),
        );
    }

    // Restore original blend mode and shader
    canvas.set_blend_mode(original_blend);
    canvas.set_default_shader();
    Ok(())
}

pub fn draw_conga_rope(
    ctx: &mut Context,
    canvas: &mut Canvas,
    player_pos: Vec2,
    // (chain_index, pos) pairs, already sorted by chain_index by the caller. Only the position
    // is used here — the index just rides along because the caller sorts by it before this is
    // called (see CHAIN_SORT_BUF in main.rs), so a plain &[Vec2] would force a second copy.
    chain_links: &[(usize, Vec2)],
    time: f32,
    beat_intensity: f32,
) -> ggez::GameResult {
    if chain_links.is_empty() {
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

    // Total chain length for hue mapping
    let total_links = chain_links.len() as f32;

    CONGA_WAYPOINT_BUF.with(|wbuf| -> ggez::GameResult {
        let mut waypoints = wbuf.borrow_mut();
        waypoints.clear();
        waypoints.push(player_center);
        for &(_, pos) in chain_links {
            waypoints.push(pos);
        }

        CONGA_SEGMENT_BUF.with(|buf| -> ggez::GameResult {
            let mut segs = buf.borrow_mut();
            segs.clear();

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

                        let seg_delta = point - prev_point;
                        let seg_len = seg_delta.length();
                        if seg_len > 0.5 {
                            let seg_angle = seg_delta.y.atan2(seg_delta.x);
                            segs.push((prev_point, seg_angle, seg_len, [rr, gg, bb]));
                        }
                    }
                    prev_point = point;
                }
            }

            // Pass 1: main rope segments, plain alpha blend (whatever the canvas is already using).
            for &(pos, angle, len, rgb) in segs.iter() {
                let color = Color::new(rgb[0], rgb[1], rgb[2], alpha_base);
                canvas.draw(
                    unit_line,
                    DrawParam::default()
                        .dest(pos)
                        .rotation(angle)
                        .scale(Vec2::new(len, thickness))
                        .color(color),
                );
            }

            // Pass 2: neon glow, additive blend switched on once for the whole rope instead of
            // once per micro-segment.
            canvas.set_blend_mode(BlendMode::ADD);
            for &(pos, angle, len, rgb) in segs.iter() {
                let glow_color = Color::new(rgb[0], rgb[1], rgb[2], alpha_base * 0.35);
                canvas.draw(
                    unit_line,
                    DrawParam::default()
                        .dest(pos)
                        .rotation(angle)
                        .scale(Vec2::new(len, thickness * 2.2))
                        .color(glow_color),
                );
            }
            canvas.set_blend_mode(BlendMode::ALPHA);
            Ok(())
        })
    })
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

/// Telegraph that a fresh herd is armed and will drop on the next downbeat (bar-quantized
/// spawns). Draws a ring around the beat indicator that tightens as the wave approaches, plus
/// a soft cyan halo that brightens with anticipation — a clear "here it comes, on the beat" cue
/// so the quantized arrival reads as intentional rhythm rather than a random spawn.
pub fn draw_wave_telegraph(
    ctx: &mut Context,
    canvas: &mut Canvas,
    center: Vec2,
    // 0..1 anticipation: climbs while the wave is armed, driving brightness/pull-in.
    anticipation: f32,
    // beat phase 0..1 within the current beat, so the ring throbs in time.
    beat_phase: f32,
) -> ggez::GameResult {
    let unit_circle = match UNIT_CIRCLE.get() {
        Some(mesh) => mesh,
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh)
        }
    };
    let a = anticipation.clamp(0.0, 1.0);
    // Ring starts wide and tightens toward the indicator as the drop nears.
    let throb = (beat_phase * std::f32::consts::TAU).sin() * 0.5 + 0.5;
    let ring_r = 58.0 - a * 20.0 + throb * 4.0;
    // Soft filled halo behind the indicator — cheap, no stroke mesh needed. Brightens with
    // anticipation so the impending drop is unmistakable.
    let halo_alpha = ((28.0 + a * 70.0) as u8).min(140);
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(center)
            .scale(Vec2::splat(ring_r + 6.0))
            .color(Color::from_rgba(80, 220, 255, halo_alpha)),
    );
    // Thin bright leading ring, built stroked so it reads as an outline closing in. Reuses
    // `cached_stroke_circle` (same cache every other beat-synced ring in this file draws from)
    // instead of building a fresh `Mesh::new_circle` GPU buffer every frame the wave is armed.
    let bright = ((130.0 + a * 125.0) as u8).min(255);
    let ring = cached_stroke_circle(ctx, ring_r, 2.5 + a * 1.5)?;
    canvas.draw(
        &ring,
        DrawParam::default()
            .dest(center)
            .color(Color::from_rgba(120, 235, 255, bright)),
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

/// Draw the Stomp ground-pound shockwave — a fast, dusty ring that slams outward from the player.
/// Earthier and heavier than the whistle's bright horn-blast so the two abilities read differently.
pub fn draw_stomp_ring(
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
    let fade = 1.0 - frac;

    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    // Thick leading dust wall — dirty tan, like kicked-up sand.
    let thickness = (10.0 * fade + 2.0).max(2.0);
    let front = cached_stroke_circle(ctx, radius, thickness)?;
    canvas.draw(
        &front,
        DrawParam::default()
            .dest(center)
            .color(Color::new(0.85, 0.74, 0.5, (fade * 0.85).clamp(0.0, 1.0))),
    );
    // A brighter thin crest riding the front for a bit of snap.
    let crest = cached_stroke_circle(ctx, radius, 1.5)?;
    canvas.draw(
        &crest,
        DrawParam::default()
            .dest(center)
            .color(Color::new(1.0, 0.95, 0.8, (fade * 0.9).clamp(0.0, 1.0))),
    );

    canvas.set_blend_mode(original_blend);
    Ok(())
}

/// Draw the delivery pen — the "bank your train" corral the player drives the conga line into.
/// A warm gold goal-zone disc ringed by slowly-turning buoy posts, with a bobbing chevron beacon
/// marking the drop-off. It's dormant-but-visible with no train, and lights up (brighter fill,
/// faster pulse, a green "GO" halo) once the player has crabs to bank. `flash` (0..1, decaying)
/// blooms a bright celebratory ring right after a delivery lands. All geometry reuses the shared
/// cached circle/line meshes, so this costs a handful of tinted draws — no per-frame allocation.
#[allow(clippy::too_many_arguments)]
pub fn draw_delivery_pen(
    ctx: &mut Context,
    canvas: &mut Canvas,
    center: Vec2,
    radius: f32,
    time: f32,
    beat_intensity: f32,
    ready: bool,
    flash: f32,
) -> ggez::GameResult {
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

    // Breathing pulse — gentle when idle, urgent when there's a train to bank.
    let pulse_speed = if ready { 6.0 } else { 2.2 };
    let pulse = 0.5 + 0.5 * (time * pulse_speed).sin();
    let beat = beat_intensity.clamp(0.0, 1.0);

    // Warm goal-zone fill (normal blend so it reads as a marked patch of ground, not a glow).
    let fill_alpha = if ready { 0.16 + 0.12 * pulse } else { 0.08 + 0.04 * pulse };
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(center)
            .scale(Vec2::splat(radius))
            .color(Color::new(1.0, 0.82, 0.28, fill_alpha)),
    );

    let orig_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    // Outer boundary ring — the "fence line" of the pen. Greenish + brighter when ready to bank.
    let (rr, rg, rb) = if ready { (0.5, 1.0, 0.5) } else { (1.0, 0.82, 0.35) };
    let ring_alpha = if ready { 0.55 + 0.35 * pulse } else { 0.3 + 0.15 * pulse };
    let boundary = cached_stroke_circle(ctx, radius, 3.0)?;
    canvas.draw(
        &boundary,
        DrawParam::default()
            .dest(center)
            .color(Color::new(rr, rg, rb, ring_alpha.clamp(0.0, 1.0))),
    );
    // Inner accent ring, breathing on the beat.
    let inner = cached_stroke_circle(ctx, radius * 0.7, 1.5)?;
    canvas.draw(
        &inner,
        DrawParam::default()
            .dest(center)
            .color(Color::new(rr, rg, rb, (0.2 + beat * 0.5) * 0.6)),
    );

    // Buoy posts around the rim, slowly turning like a rotating corral.
    let post_count = 10;
    let spin = time * if ready { 0.9 } else { 0.35 };
    for i in 0..post_count {
        let ang = spin + (i as f32 / post_count as f32) * std::f32::consts::TAU;
        let p = center + Vec2::new(ang.cos(), ang.sin()) * radius;
        let post_r = 4.0 + 1.5 * pulse;
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(p)
                .scale(Vec2::splat(post_r))
                .color(Color::new(rr, rg, rb, (0.6 + beat * 0.4).clamp(0.0, 1.0))),
        );
    }

    // Bobbing chevron beacon above the pen pointing down into it — "deliver here".
    let bob = (time * (if ready { 4.0 } else { 2.0 })).sin() * 6.0;
    let apex = center + Vec2::new(0.0, -radius - 26.0 + bob);
    let wing = 13.0;
    let drop = 15.0;
    let bright = (0.7 + 0.3 * pulse).clamp(0.0, 1.0);
    let beacon_col = Color::new(rr, rg, rb, bright);
    for side in [-1.0f32, 1.0] {
        let tip = apex + Vec2::new(side * wing, drop);
        let d = tip - apex;
        let len = d.length();
        let angle = d.y.atan2(d.x);
        canvas.draw(
            unit_line,
            DrawParam::default()
                .dest(apex)
                .rotation(angle)
                .scale(Vec2::new(len, 4.0))
                .color(beacon_col),
        );
    }

    // Delivery bloom — a jackpot flare right after a successful bank. Layered so cashing in the
    // train reads as a real payoff, not just a number ticking: an expanding shockwave ring, a
    // spinning starburst of god-rays, a rising column of light, and a hot core pop that all bloom
    // out of the pen and fade together. Everything except the single shockwave ring reuses the
    // already-fetched cached unit line/circle meshes (scaled via DrawParam), so this stays a
    // handful of draws with no per-frame GPU-buffer allocation.
    if flash > 0.0 {
        let f = flash.clamp(0.0, 1.0);
        let grow = 1.0 - f; // 0 at the instant of banking, 1 as the flare finishes

        // Expanding shockwave ring sweeping outward past the pen boundary.
        let burst_r = radius * (1.0 + grow * 1.4);
        let burst = cached_stroke_circle(ctx, burst_r, 4.0 + f * 8.0)?;
        canvas.draw(
            &burst,
            DrawParam::default()
                .dest(center)
                .color(Color::new(0.6, 1.0, 0.6, f)),
        );

        // Starburst of god-rays firing out of the pen, turning slowly as they stretch and fade.
        let ray_count = 12;
        let ray_spin = time * 1.5;
        let ray_len = radius * (0.5 + grow * 1.6);
        let ray_thick = (2.0 + f * 6.0).max(0.5);
        let ray_alpha = (f * 0.8).clamp(0.0, 1.0);
        for i in 0..ray_count {
            let ang = ray_spin + (i as f32 / ray_count as f32) * std::f32::consts::TAU;
            canvas.draw(
                unit_line,
                DrawParam::default()
                    .dest(center + Vec2::new(ang.cos(), ang.sin()) * radius * 0.25)
                    .rotation(ang)
                    .scale(Vec2::new(ray_len, ray_thick))
                    .color(Color::new(0.8, 1.0, 0.7, ray_alpha)),
            );
        }

        // Rising column of light — a bright shaft climbing out of the pen as the flare peaks.
        let col_h = radius * (1.2 + grow * 2.2);
        let col_w = (radius * 0.5 * f).max(1.0);
        canvas.draw(
            unit_line,
            DrawParam::default()
                .dest(center)
                .rotation(-std::f32::consts::FRAC_PI_2)
                .scale(Vec2::new(col_h, col_w))
                .color(Color::new(0.7, 1.0, 0.75, f * 0.5)),
        );

        // Hot core pop — a white-gold flash at the pen center, fiercest right as you bank.
        let core_r = radius * (0.35 + grow * 0.5);
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(center)
                .scale(Vec2::splat(core_r))
                .color(Color::new(1.0, 1.0, 0.85, f * f * 0.7)),
        );

        // Full-zone gold flare fading out.
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(center)
                .scale(Vec2::splat(radius))
                .color(Color::new(1.0, 0.9, 0.4, f * 0.4)),
        );
    }

    canvas.set_blend_mode(orig_blend);
    Ok(())
}

/// Draw the level's tide pools — patches of shallow water that drag on movement and force the
/// player to route the conga train around (or dash across) them. Each pool is a translucent blue
/// disc with a soft rim, a couple of slowly expanding ripple rings, and a glint highlight, all
/// gently breathing on the beat so the water feels alive without stealing focus from the crabs.
/// `wading` brightens the pool the player is currently standing in for feedback. Drawn on the
/// ground layer, under the crabs and rope, so the train visibly wades through it. All geometry
/// reuses the cached unit circle / stroke-circle meshes — no per-frame GPU buffer allocation.
pub fn draw_tide_pools(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pools: &[(Vec2, f32)],
    time: f32,
    beat_intensity: f32,
    player_center: Vec2,
) -> ggez::GameResult {
    if pools.is_empty() {
        return Ok(());
    }
    let unit_circle = match UNIT_CIRCLE.get() {
        Some(mesh) => mesh,
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh)
        }
    };
    let beat = beat_intensity.clamp(0.0, 1.0);

    for (i, (center, radius)) in pools.iter().enumerate() {
        let center = *center;
        let radius = *radius;
        // Per-pool phase so they don't all breathe in lockstep.
        let phase = i as f32 * 1.7;
        let breathe = 0.5 + 0.5 * (time * 1.3 + phase).sin();
        let wading = player_center.distance(center) < radius;

        // Base water disc — normal blend so it reads as a darker, cooler patch of ground.
        let fill_a = 0.30 + 0.06 * breathe + if wading { 0.10 } else { 0.0 };
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(center)
                .scale(Vec2::splat(radius))
                .color(Color::new(0.16, 0.34, 0.52, fill_a)),
        );
        // Lighter shallow center for a bit of depth.
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(center)
                .scale(Vec2::splat(radius * 0.6))
                .color(Color::new(0.30, 0.55, 0.72, 0.16)),
        );

        let orig_blend = canvas.blend_mode();
        canvas.set_blend_mode(BlendMode::ADD);

        // Soft rim so the pool edge — the line you route around — reads clearly.
        let rim = cached_stroke_circle(ctx, radius, 2.5)?;
        canvas.draw(
            &rim,
            DrawParam::default().dest(center).color(Color::new(
                0.45,
                0.8,
                1.0,
                (0.22 + 0.18 * breathe + if wading { 0.25 } else { 0.0 }).clamp(0.0, 1.0),
            )),
        );

        // Two ripple rings expanding outward from the middle and fading at the rim.
        for k in 0..2 {
            let t = ((time * 0.35 + phase + k as f32 * 0.5).fract()).clamp(0.0, 1.0);
            let rr = radius * (0.15 + t * 0.85);
            let a = (1.0 - t) * 0.28;
            if a > 0.01 {
                let ripple = cached_stroke_circle(ctx, rr, 1.5)?;
                canvas.draw(
                    &ripple,
                    DrawParam::default()
                        .dest(center)
                        .color(Color::new(0.55, 0.85, 1.0, a)),
                );
            }
        }

        // A drifting glint highlight, brighter on the beat, to sell the wet surface.
        let g_ang = time * 0.6 + phase;
        let glint = center + Vec2::new(g_ang.cos(), g_ang.sin() * 0.5) * radius * 0.4;
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(glint)
                .scale(Vec2::splat(6.0 + 3.0 * beat))
                .color(Color::new(0.7, 0.95, 1.0, 0.18 + 0.25 * beat)),
        );

        canvas.set_blend_mode(orig_blend);
    }
    Ok(())
}

/// Draw the thrown lasso: the rope from the player to its tip, a catch-radius indicator ring
/// that fades in as it extends, the spinning open-loop noose, and a bright knot at the tip.
/// `outward_progress` is 0..1 (how far the throw has extended) and `spin` is the loop's current
/// rotation in radians. All geometry here reuses cached meshes (`UNIT_LINE`/`UNIT_CIRCLE` scaled
/// via `DrawParam`, plus the dedicated stroke-circle and lasso-loop caches) instead of building
/// fresh `Mesh::new_line`/`Mesh::new_circle` GPU buffers every frame — the lasso is thrown on
/// nearly every catch attempt, so this used to be several fresh mesh allocations a frame for as
/// long as it stayed in flight.
pub fn draw_lasso(
    ctx: &mut Context,
    canvas: &mut Canvas,
    player_center: Vec2,
    tip: Vec2,
    outward_progress: f32,
    spin: f32,
) -> ggez::GameResult {
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

    let rope_delta = tip - player_center;
    let rope_len = rope_delta.length();
    if rope_len > 1.0 {
        let rope_angle = rope_delta.y.atan2(rope_delta.x);

        // Glowing rope: thick glow pass + thin bright pass
        let orig_blend = canvas.blend_mode();
        canvas.set_blend_mode(BlendMode::ADD);
        canvas.draw(
            unit_line,
            DrawParam::default()
                .dest(player_center)
                .rotation(rope_angle)
                .scale(Vec2::new(rope_len, 6.0))
                .color(Color::from_rgba(230, 160, 30, 60)),
        );
        canvas.set_blend_mode(orig_blend);

        canvas.draw(
            unit_line,
            DrawParam::default()
                .dest(player_center)
                .rotation(rope_angle)
                .scale(Vec2::new(rope_len, 2.5))
                .color(Color::from_rgba(220, 160, 50, 220)),
        );
    }

    // Catch-radius indicator ring (fades in as lasso extends)
    let catch_r = 60.0_f32;
    let ring_alpha = (outward_progress * 80.0) as u8;
    if ring_alpha > 4 {
        let catch_ring = cached_stroke_circle(ctx, catch_r, 1.5)?;
        canvas.draw(
            &catch_ring,
            DrawParam::default()
                .dest(tip)
                .color(Color::from_rgba(255, 220, 80, ring_alpha)),
        );
    }

    // Spinning lasso loop: an open ring (gap = open lasso) that spins as it flies and grows
    // slightly as the throw extends.
    let loop_r = 18.0 + outward_progress * 6.0;
    let loop_glow = cached_lasso_loop(ctx, loop_r, 8.0)?;
    let orig_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);
    canvas.draw(
        &loop_glow,
        DrawParam::default()
            .dest(tip)
            .rotation(spin)
            .color(Color::from_rgba(255, 200, 60, 80)),
    );
    canvas.set_blend_mode(orig_blend);
    let loop_line = cached_lasso_loop(ctx, loop_r, 3.5)?;
    canvas.draw(
        &loop_line,
        DrawParam::default()
            .dest(tip)
            .rotation(spin)
            .color(Color::from_rgba(255, 210, 70, 230)),
    );

    // Bright center dot at the tip knot
    let unit_circle = match UNIT_CIRCLE.get() {
        Some(mesh) => mesh,
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh)
        }
    };
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(tip)
            .scale(Vec2::splat(5.0))
            .color(Color::from_rgba(255, 240, 160, 240)),
    );

    Ok(())
}

/// Draw a hard-shelled crab's shell indicator — a thin steely arc that depletes as the shell is
/// worn down or cracked, so the player can read at a glance which crabs need a Stomp.
pub fn draw_armor_ring(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    size: f32,
    shell_frac: f32,
    time: f32,
) -> ggez::GameResult {
    let radius = size * 0.8;
    let pulse = (time * 5.0).sin() * 0.5 + 0.5;

    // Faint full track so the drained portion still reads as progress.
    let track = cached_stroke_circle(ctx, radius, 3.0)?;
    canvas.draw(
        &track,
        DrawParam::default()
            .dest(pos)
            .color(Color::new(0.0, 0.0, 0.0, 0.35)),
    );

    let segs = 40usize;
    let filled = ((segs as f32) * shell_frac.clamp(0.0, 1.0)).ceil().max(1.0) as usize;
    let arc = cached_stroke_arc(ctx, radius, 3.0, segs, filled)?;
    canvas.draw(
        &arc,
        DrawParam::default()
            .dest(pos)
            .color(Color::new(0.6, 0.72, 0.88, 0.85 + pulse * 0.15)),
    );
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
