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

// A unit equilateral-ish triangle pointing along +x (tip at (1,0), base corners at roughly
// (-0.5, +-0.75)), built once and reused for every screen-edge radar arrow via `DrawParam`
// rotation + scale instead of baking each arrow's rotated tip/base points into two fresh
// `Mesh::new_polygon` GPU buffers (arrow + glow) every frame. Every uncaught crab near a
// screen edge was allocating two brand-new GPU meshes per frame it stayed there.
static UNIT_TRIANGLE: OnceLock<Mesh> = OnceLock::new();

thread_local! {
    // Cache of stroke-circle meshes keyed by (radius, thickness) quantized to the nearest
    // 2px/1px (see cached_stroke_circle). Ring-style effects (beat ghost rings, catch
    // shockwaves, attraction glow, magnet/thief/golden auras, the delivery pen) can't reuse a
    // single unit-circle scaled via DrawParam like fill circles do, because scaling a stroke
    // ring scales its line thickness along with its radius, distorting the taper these effects
    // rely on. Instead we memoize the actual built mesh per rounded (radius, thickness) pair.
    // This matters most for beat ghost rings: every crab in the conga chain gets a ring on each
    // beat, and since they're all spawned in lockstep they share the same age every frame, so in
    // practice one cache entry is reused by every ring in the chain instead of the whole chain
    // rebuilding a fresh GPU mesh each frame. Size-capped in cached_stroke_circle so a long play
    // session sweeping many distinct radii can't grow this without bound.
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

    // Reusable instance buffers for draw_conga_rope's two passes (main rope + additive glow).
    // Each pass used to issue one canvas.draw() per micro-segment (SEGS=14 per chain link) — on a
    // 50-crab train that's 2 * 14 * 50 = 1400 individual GPU submissions a frame for the rope
    // alone, the same per-call overhead the particle/leg/body/trail/marcher batching above already
    // eliminated everywhere else. Collapsed into one InstanceArray fill + draw_instanced_mesh per
    // pass, so the rope costs 2 draw calls total no matter how long the train gets. Same unit_line
    // mesh, same per-segment position/rotation/scale/color, identical on-screen output.
    static CONGA_MAIN_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    static CONGA_GLOW_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

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

    // Reusable instance array for the flashlight's volumetric dust motes (see draw_flashlight)
    // so the beam's ~20 drifting specks are one batched GPU submission per frame instead of up
    // to 20 individual canvas.draw() calls — this ran every frame the flashlight was held on,
    // i.e. most of active play.
    static FLASHLIGHT_DUST_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

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

    // Every other round part of a crab (shadow, body, shell dome, specular glint, 2 claws, 2 claw
    // highlights, 2 eye-whites, 2 pupils — 12 unit-circle draws) was still issued as an individual
    // canvas.draw() call, same problem the legs had: a long conga train plus a fresh wild herd
    // (40-50+ crabs) meant 500+ of these a frame, each its own GPU submission even though every
    // one uses the exact same UNIT_CIRCLE mesh. draw_crab() now pushes these into this buffer
    // instead, and flush_crab_bodies() (called right alongside flush_crab_legs()) drains it as one
    // instanced batch. Same positions/scales/colors, same draw order relative to each other within
    // a crab, just reordered relative to other crabs' legs/rings — invisible in motion, same as
    // the legs batching already shipped.
    static CRAB_BODY_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static CRAB_BODY_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

    // Reusable instance buffers for draw_catch_trails' two line passes (soft glow underlay +
    // bright core) and its spark pass. Up to 48 live trails, each issuing 3 individual
    // canvas.draw() calls, was up to 144 GPU submissions a frame during any catch-heavy stretch —
    // the same per-call overhead the particle/leg/body batching above already eliminated
    // elsewhere. Same two meshes (unit_line, unit_circle) reused via InstanceArray instead.
    static TRAIL_GLOW_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    static TRAIL_CORE_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    static TRAIL_SPARK_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

    // Reusable instance buffers for draw_penned_marchers' three passes (shadow, body, rim
    // highlight) — same batching technique as the particle/leg/body/trail instances above. A big
    // bank can queue up to 40 marchers at once, each previously issuing 3 individual canvas.draw()
    // calls (shadow + body + rim), i.e. up to 120 separate GPU submissions for a purely cosmetic
    // parade. Filling one InstanceArray per pass collapses that to 3 draw calls total regardless
    // of marcher count, with identical on-screen output.
    static MARCHER_SHADOW_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    static MARCHER_BODY_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    static MARCHER_RIM_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    // The DrawParam lists fed into the three InstanceArrays above were freshly allocated
    // (Vec::new() + push) every single call to draw_penned_marchers, even though the instance
    // arrays themselves were already reused — three heap allocations a frame growing with
    // marcher count, the one place in this file that didn't follow the reused-scratch-buffer
    // pattern every other batched draw function here uses. Reused and cleared in place instead.
    static MARCHER_SHADOW_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static MARCHER_BODY_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static MARCHER_RIM_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());

    // draw_grass tiled its texture across the whole window with one canvas.draw() per tile — at
    // the default 800x600 window and a 4x4 grass tile, that's 200x150 = 30,000 individual GPU
    // submissions every single frame just for the ground, dwarfing every other draw-call cost in
    // the game combined. Same batching technique as the instances above: fill one InstanceArray
    // with a DrawParam per tile position and issue a single draw_instanced_mesh. Same texture,
    // same positions, identical on-screen output.
    static GRASS_TILE_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

    // Scratch grouping map + reusable InstanceArrays for draw_chain_rings, keyed by the same
    // rounded (radius*2, thickness*4) key cached_stroke_circle() already uses to memoize the
    // mesh itself. A stroke ring can't be instanced via one shared unit mesh scaled by DrawParam
    // like a fill circle (scaling would stretch the stroke thickness along with the radius), but
    // rings spawned on the same beat share the same age every frame — and therefore the exact
    // same cached mesh — so grouping same-mesh rings into one InstanceArray each still collapses
    // most of the draw calls. A long conga train pushes up to MAX_CHAIN_RINGS (64) rings, each
    // previously costing 2 individual canvas.draw() calls (ring + inner glow) every frame for its
    // whole lifetime — up to 128 GPU submissions a frame, the same per-call overhead already
    // eliminated for particles/legs/bodies/trails/marchers/grass. Same meshes, same positions,
    // same draw order within a beat's rings, identical on-screen output.
    static CHAIN_RING_GROUPS: RefCell<HashMap<(i32, i32), Vec<DrawParam>>> = RefCell::new(HashMap::new());
    static CHAIN_RING_INSTANCES: RefCell<HashMap<(i32, i32), InstanceArray>> = RefCell::new(HashMap::new());

    // Reusable instance buffers for draw_crab_radar's two passes (arrow + glow outline). A big
    // wild herd can put a couple dozen uncaught crabs near the screen edges at once, each
    // previously costing 2 individual canvas.draw() calls (arrow + glow) every frame it lingered
    // there — the same per-call overhead already eliminated for particles/legs/bodies/trails/
    // marchers/grass/chain rings. Same UNIT_TRIANGLE mesh, same positions/rotations/scales/colors,
    // identical on-screen output, just batched into one InstanceArray fill + draw per pass.
    static RADAR_ARROW_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static RADAR_GLOW_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static RADAR_ARROW_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    static RADAR_GLOW_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
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

/// Draw (and clear) every body-part DrawParam (shadow/body/dome/glint/claws/eyes/pupils)
/// accumulated by draw_crab() calls since the last flush, as a single instanced batch — the same
/// technique flush_crab_legs() uses. Call once per drawing pass, alongside flush_crab_legs().
pub fn flush_crab_bodies(ctx: &mut Context, canvas: &mut Canvas) -> ggez::GameResult {
    CRAB_BODY_PARAMS.with(|params_cell| -> ggez::GameResult {
        let mut params = params_cell.borrow_mut();
        if params.is_empty() {
            return Ok(());
        }
        let unit_circle = match UNIT_CIRCLE.get() {
            Some(mesh) => mesh.clone(),
            None => {
                let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
                UNIT_CIRCLE.get_or_init(|| mesh).clone()
            }
        };
        CRAB_BODY_INSTANCES.with(|inst_cell| -> ggez::GameResult {
            let mut inst_slot = inst_cell.borrow_mut();
            let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
            instances.set(params.iter().copied());
            canvas.draw_instanced_mesh(unit_circle, instances, DrawParam::default());
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

/// Quantization used to key `STROKE_CIRCLE_CACHE`, shared by `cached_stroke_circle` and any
/// caller (like `draw_chain_rings`'s instancing groups) that needs to compute the *same* key
/// independently to look up a mesh `cached_stroke_circle` already inserted. Keeping this in one
/// place avoids the two sides drifting out of sync — they used to duplicate the rounding formula
/// inline, and a change to one without the other silently turned every cache lookup into a miss
/// (the mesh existed under a different key, so the ring just never got drawn).
///
/// Quantized to the nearest 2px of radius / 1px of thickness. Most callers drive radius/
/// thickness off continuous per-frame values (time, beat pulse, per-crab jitter), so a
/// fine-grained key meant almost every call rounded to a *new* bucket every frame — the cache
/// almost never hit, silently defeating the whole point of memoizing. A stroke ring's outline
/// doesn't need sub-pixel precision, so this coarseness is visually indistinguishable but turns
/// "rebuild a GPU mesh nearly every call" into "reuse the same handful of meshes across a run of
/// nearby frames".
pub fn stroke_circle_key(radius: f32, thickness: f32) -> (i32, i32) {
    let radius = radius.max(0.5);
    let thickness = thickness.max(0.25);
    ((radius * 0.5).round() as i32, thickness.round() as i32)
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
    let key = stroke_circle_key(radius, thickness);

    if let Some(mesh) = STROKE_CIRCLE_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return Ok(mesh);
    }

    // Even with coarser buckets, a long play session sweeping many distinct crab sizes/radii
    // over time would otherwise let this HashMap grow without bound (entries are never
    // evicted). Cap it: if it's gotten large, clear it and let it repopulate from the
    // (now coarser, so cheap to rebuild) working set instead of accreting stale meshes
    // forever. In practice the live working set is tiny (a few dozen distinct rings on
    // screen at once), so this almost never triggers during normal play.
    const MAX_STROKE_CIRCLE_CACHE: usize = 512;
    STROKE_CIRCLE_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= MAX_STROKE_CIRCLE_CACHE {
            c.clear();
        }
    });

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

/// A full-screen edge glow that turns being "in the groove" into peripheral feedback: the
/// four screen edges bloom inward with a soft colored gradient that intensifies with the Groove
/// meter and breathes on the beat. Below a floor `groove` it draws nothing (no cost when the
/// player is cold). The color walks from cool cyan while the meter builds to hot magenta/gold as
/// it tops out, so a maxed groove frames the whole screen in a pulsing glow — the same read as
/// the corner meter, but felt at the edge of vision instead of needing a glance.
///
/// Cheap: a soft falloff is faked with a few stacked translucent bands per edge (each a single
/// `unit_square` draw), not a shader — a couple dozen batched fills a frame, and only while hot.
pub fn draw_groove_vignette(
    ctx: &mut Context,
    canvas: &mut Canvas,
    width: f32,
    height: f32,
    groove: f32,
    beat_intensity: f32,
) -> ggez::GameResult {
    // Nothing until the player is meaningfully in the groove — keeps it a reward, not clutter,
    // and means zero draws during ordinary cold play.
    if groove < 0.25 {
        return Ok(());
    }
    // Remap 0.25..1.0 onto 0..1 so the glow eases in from the threshold rather than popping on.
    let t = ((groove - 0.25) / 0.75).clamp(0.0, 1.0);

    // Color walks cyan -> magenta/gold as the meter fills, matching the corner groove bar.
    let r = 0.30 + t * 0.70;
    let g = 0.95 - t * 0.45;
    let b = 0.90 - t * 0.55;

    // Breathe on the beat: a maxed groove pulses harder so the frame throbs in time with the music.
    let pulse = 1.0 + beat_intensity * (0.25 + t * 0.55);
    // How far the glow reaches in from each edge, and its peak opacity — both grow with the meter.
    let reach = (26.0 + t * 90.0) * pulse;
    let peak = (0.10 + t * 0.32) * pulse;

    // Stack a few bands per edge, fading toward the interior, to fake a smooth gradient falloff.
    const BANDS: usize = 5;
    let sq = unit_square(ctx)?;
    for i in 0..BANDS {
        // Band 0 sits at the very edge (widest/brightest); inner bands are thinner slivers that
        // taper the glow off toward the play area.
        let f = i as f32 / BANDS as f32;
        let band = reach * (1.0 - f);
        if band < 0.5 {
            continue;
        }
        // Alpha falls off quadratically inward so the edge reads as a soft bloom, not a hard bar.
        let a = (peak * (1.0 - f) * (1.0 - f)).clamp(0.0, 0.85);
        let col = Color::new(r, g, b, a);
        // Top edge
        canvas.draw(
            sq,
            DrawParam::default()
                .dest(Vec2::new(0.0, 0.0))
                .scale(Vec2::new(width, band))
                .color(col),
        );
        // Bottom edge
        canvas.draw(
            sq,
            DrawParam::default()
                .dest(Vec2::new(0.0, height - band))
                .scale(Vec2::new(width, band))
                .color(col),
        );
        // Left edge
        canvas.draw(
            sq,
            DrawParam::default()
                .dest(Vec2::new(0.0, 0.0))
                .scale(Vec2::new(band, height))
                .color(col),
        );
        // Right edge
        canvas.draw(
            sq,
            DrawParam::default()
                .dest(Vec2::new(width - band, 0.0))
                .scale(Vec2::new(band, height))
                .color(col),
        );
    }
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
    // Beat phase in [0,1): 0.0 the instant a beat lands, climbing to ~1.0 just before the next.
    // The grass shader uses it to fire a concentric ripple of light out from screen center on
    // each downbeat, so the whole ground breathes in time with the music.
    pub beat: f32,
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
            CrabType::Magnet => (45, 90.0..260.0, 3.0..7.0, true),  // Chunky lodestone burst — the cluster pops with it
            CrabType::Thief => (28, 120.0..300.0, 2.0..5.0, true),  // Wiry poison-green burst — catching it feels like relief
            CrabType::Golden => (55, 100.0..320.0, 2.5..7.0, true), // Lavish gold coin-burst — the treasure catch pops
            CrabType::Boss => (70, 90.0..320.0, 4.0..13.0, true),   // Huge celebratory burst
            CrabType::TideBoss => (70, 90.0..320.0, 4.0..13.0, true), // Huge tidal splash burst
            CrabType::RhythmBoss => (70, 90.0..320.0, 4.0..13.0, true), // Huge violet disco burst
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
                CrabType::Magnet => 12,
                CrabType::Thief => 10,
                CrabType::Golden => 20, // a lavish shower of gold sparkles for the treasure catch
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
                    CrabType::Magnet => [1.0, 0.55, 0.2], // Molten lodestone sparks
                    CrabType::Thief => [0.5, 1.0, 0.6],   // Poison-green thief sparks
                    CrabType::Golden => [1.0, 0.85, 0.25], // Bright treasure-gold coin sparks
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

    /// A King Crab fissure GEYSERS on the beat: fling a short burst of molten debris up out of the
    /// pit. Launched near-vertically with a little spread so gravity (applied in `update`) arcs it
    /// back down like sparks off lava — a rhythmic spout that reads as the hazard surging, not just
    /// glowing. Particle count scales gently with pit radius; the shared MAX_PARTICLES cap guards it.
    pub fn spawn_fissure_geyser(&mut self, center: Vec2, radius: f32, rng: &mut impl Rng) {
        let count = (6.0 + radius * 0.08) as usize;
        for _ in 0..count {
            // Spawn from somewhere inside the pit mouth so the column has width, not a single jet.
            let off_a = rng.random_range(0.0_f32..std::f32::consts::TAU);
            let off_r = rng.random_range(0.0_f32..radius * 0.5);
            let pos = center + Vec2::new(off_a.cos() * off_r, off_a.sin() * off_r);
            // Mostly upward with a slight sideways fan.
            let up = rng.random_range(180.0_f32..340.0_f32);
            let side = rng.random_range(-70.0_f32..70.0_f32);
            let life = rng.random_range(0.35_f32..0.75_f32);
            // Hot molten palette: orange core flecked toward yellow-white.
            let heat = rng.random_range(0.0_f32..1.0);
            let color = [1.0, 0.4 + 0.45 * heat, 0.08 + 0.25 * heat];
            self.push(Particle {
                pos,
                vel: Vec2::new(side, -up),
                life,
                max_life: life,
                size: rng.random_range(2.0_f32..4.5_f32),
                color,
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
            // The glow pass only takes larger particles (size > 4.0); if none qualify this frame
            // it's an empty array, and ggez's instanced flush asserts capacity > 0 on draw. Skip
            // it when empty. (main always has ≥1 here — we returned early on no particles above.)
            if !glow.instances().is_empty() {
                canvas.draw_instanced_mesh(unit_circle, glow, DrawParam::default());
            }
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
    beat: f32,
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
        beat,
    })
    .build(ctx);
    canvas.set_shader_params(&params);
    canvas.set_shader(shader);
    let quad = cached_fill_rect(ctx, -width / 2.0, -height / 2.0, width, height, Color::RED)?;
    canvas.draw(&quad, DrawParam::default());
    canvas.set_default_shader();
    canvas.set_blend_mode(BlendMode::MULTIPLY);

    // Repeat a tiled grass texture across the screen. Batched into a single InstanceArray draw
    // (see GRASS_TILE_INSTANCES) instead of one canvas.draw() per tile — at the default window
    // size and a 4x4 grass tile that was up to 30,000 individual GPU submissions a frame.
    let tile_w = texture.width() as f32;
    let tile_h = texture.height() as f32;
    let tiles_x = (width / tile_w).ceil() as i32;
    let tiles_y = (height / tile_h).ceil() as i32;
    GRASS_TILE_INSTANCES.with(|inst_cell| -> ggez::GameResult {
        let mut inst_slot = inst_cell.borrow_mut();
        let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, texture.clone()));
        instances.set((0..tiles_y).flat_map(|y| (0..tiles_x).map(move |x| (x, y))).map(
            |(x, y)| DrawParam::default().dest([x as f32 * tile_w, y as f32 * tile_h]),
        ));
        canvas.draw(instances, DrawParam::default());
        Ok(())
    })?;

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

// One reusable instance array for the ambient atmosphere motes, so the whole drifting
// field is a single batched GPU submission per frame instead of one draw per speck.
thread_local! {
    static AMBIENT_MOTE_INSTANCES: RefCell<Option<InstanceArray>> = const { RefCell::new(None) };
}

// How many ambient motes drift across the field. Fixed and modest so the atmosphere layer
// is essentially free — one batched draw of this many scaled unit-circles.
const AMBIENT_MOTE_COUNT: usize = 46;

/// Draw a field of slow-drifting ambient motes over the ground — sea spray / drifting spores
/// that give the empty space between the action a sense of depth and living atmosphere, tinted
/// to the current biome's accent color so each zone still reads distinctly. Purely cosmetic and
/// stateless: every mote's motion is a deterministic function of `time` and its own index, so
/// there's nothing to update in the game loop. Motes bob a touch on the beat so the whole
/// atmosphere breathes with the music like everything else. One batched instanced draw.
pub fn draw_ambient_motes(
    ctx: &mut Context,
    canvas: &mut Canvas,
    width: f32,
    height: f32,
    time: f32,
    beat: f32,
    accent: Color,
) -> ggez::GameResult {
    let unit_circle = match UNIT_CIRCLE.get() {
        Some(mesh) => mesh.clone(),
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh).clone()
        }
    };

    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    // A gentle upward-and-sideways bob on the beat so the whole field lifts with the pulse.
    let beat_lift = beat.clamp(0.0, 1.0) * 3.0;

    AMBIENT_MOTE_INSTANCES.with(|cell| -> ggez::GameResult {
        let mut slot = cell.borrow_mut();
        let instances = slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
        instances.set((0..AMBIENT_MOTE_COUNT).map(|i| {
            // Deterministic, spread-out per-mote constants from the index — no RNG, no state.
            let fi = i as f32;
            let seed_a = (fi * 12.9898).sin() * 43758.547;
            let seed_b = (fi * 78.233).sin() * 12543.219;
            let rx = seed_a - seed_a.floor(); // 0..1
            let ry = seed_b - seed_b.floor(); // 0..1
            // Each mote drifts diagonally and wraps around the screen so the field never empties.
            let drift = 9.0 + rx * 14.0; // px/s, per-mote speed
            let base_x = rx * width;
            let base_y = ry * height;
            let x = (base_x + time * drift) % width;
            // Slow vertical sway layered on a slow downward drift, both wrapped to the field.
            let sway = (time * (0.4 + ry * 0.5) + fi).sin() * 10.0;
            let y = (base_y + time * (drift * 0.35) + sway) % height - beat_lift;
            // Twinkle: a slow per-mote brightness pulse so the field shimmers subtly.
            let twinkle = 0.45 + 0.4 * (time * (0.8 + rx) + fi * 1.7).sin();
            let size = 1.4 + ry * 2.2;
            let alpha = (0.10 + 0.14 * twinkle) + beat.clamp(0.0, 1.0) * 0.06;
            DrawParam::default()
                .dest(Vec2::new(x - width / 2.0, y - height / 2.0))
                .scale(Vec2::splat(size))
                .color(Color::new(accent.r, accent.g, accent.b, alpha))
        }));
        canvas.draw_instanced_mesh(unit_circle, instances, DrawParam::default());
        Ok(())
    })?;

    canvas.set_blend_mode(original_blend);
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

// `canvas` is threaded through but no longer drawn to directly: every part draw_crab() used to
// issue immediately is now deferred into CRAB_LEG_PARAMS/CRAB_BODY_PARAMS and flushed as instanced
// batches by flush_crab_legs()/flush_crab_bodies() (called once per drawing pass by the caller).
// Kept in the signature so call sites don't need to change and so a future direct-draw effect
// (e.g. a one-off overlay) has it on hand without threading it through again.
pub fn draw_crab(ctx: &mut Context, _canvas: &mut Canvas, crab: &EnemyCrab, draw_pos: Vec2, beat_phase: f32, join_pulse: f32, y_lift: f32, rotation: f32) -> ggez::GameResult {
    // Crabs previously rebuilt ~13 fresh GPU meshes every frame (shadow, body, 6 legs,
    // 2 claws, 4 eye parts) via Mesh::new_circle/new_line/new_ellipse. With a long conga
    // train this was easily 100+ mesh allocations per frame. Instead reuse the same cached
    // unit-circle and unit-line meshes the particle system and conga rope already share,
    // positioning/rotating/scaling them per-part via DrawParam instead of baking shape into
    // fresh vertex buffers. A body-space offset that needs to rotate with the crab (claw
    // and eye positions, leg roots) is rotated by hand via `rotate_offset` before being
    // folded into `dest`, since DrawParam only applies one rotation after one translation.
    // All circle parts (shadow/body/dome/glint/claws/eyes/pupils) below are deferred into
    // CRAB_BODY_PARAMS and flushed as one instanced batch by flush_crab_bodies() — draw_crab()
    // itself no longer needs a mesh handle, just the per-part transforms.
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

    // Color: more red as crab ages, and different color for type
    let [r, g, b] = crab.crab_color();
    let flash = if join_pulse > 0.0 && join_pulse <= 1.0 {
        join_pulse * (1.0 - join_pulse) * 4.0 * 0.5  // peak 0.5 at pulse=0.5
    } else {
        0.0
    };
    let crab_color = Color::new((r + flash).min(1.0), (g + flash).min(1.0), (b + flash).min(1.0), 1.0);

    // Shell shading: give the flat body circle a rounded, lit look. Light comes from a fixed
    // screen-space direction (up and slightly left) so the whole herd reads as lit from the same
    // sky, independent of each crab's facing rotation — hence these offsets are NOT rotated.
    let light_dir = Vec2::new(-0.4, -0.72);
    // Domed highlight: a smaller, brighter disc pushed toward the light makes the body read as a
    // rounded shell rather than a paper cut-out.
    let hi = |c: f32| (c + (1.0 - c) * 0.34).min(1.0);
    let dome_color = Color::new(hi(crab_color.r), hi(crab_color.g), hi(crab_color.b), 0.85);
    // Glossy specular glint near the top of the shell — a tiny bright dot that catches the eye and
    // pulses faintly with the beat so the herd shimmers on the downbeat.
    let glint_a = 0.5 + beat_phase * 0.35;

    // Crab claws (small circles)
    let claw_offset = size * 0.7;
    let claw_radius = size * 0.18;
    let claw_l = draw_pos + rotate_offset(-(claw_offset), -(claw_offset * 0.3));
    let claw_r = draw_pos + rotate_offset(claw_offset, -(claw_offset * 0.3));

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

    // All ~14 body-part pushes (shadow, body, dome, glint, 2 claws, 2 claw highlights, 2 eyes,
    // 2 pupils) collected under a single thread-local borrow instead of one `.with()` call per
    // part. `.with()` itself is cheap, but with a 100+ crab train this was 700+ separate
    // thread-local accesses a frame for a batch that's flushed once anyway — one borrow per crab
    // does the same job for a fraction of the access count.
    CRAB_BODY_PARAMS.with(|params| {
        let mut params = params.borrow_mut();
        // Deferred into CRAB_BODY_PARAMS and flushed as one instanced batch by flush_crab_bodies().
        params.push(
            DrawParam::default()
                .dest(draw_pos + Vec2::new(shadow_offset_x, shadow_offset_y))
                .scale(Vec2::new(
                    size * shadow_scale_x * 0.55,
                    size * shadow_scale_y * 0.55,
                ))
                .color(Color::from_rgba(0, 0, 0, shadow_alpha)),
        );
        // Crab body (rotation-invariant, so no need to rotate the draw)
        params.push(
            DrawParam::default()
                .dest(draw_pos)
                .scale(Vec2::splat(size / 2.0))
                .color(crab_color),
        );
        params.push(
            DrawParam::default()
                .dest(draw_pos + light_dir * size * 0.15)
                .scale(Vec2::splat(size / 2.0 * 0.62))
                .color(dome_color),
        );
        params.push(
            DrawParam::default()
                .dest(draw_pos + light_dir * size * 0.26)
                .scale(Vec2::splat(size / 2.0 * 0.2))
                .color(Color::new(1.0, 1.0, 1.0, glint_a)),
        );
        params.push(
            DrawParam::default()
                .dest(claw_l)
                .scale(Vec2::splat(claw_radius))
                .color(crab_color),
        );
        params.push(
            DrawParam::default()
                .dest(claw_r)
                .scale(Vec2::splat(claw_radius))
                .color(crab_color),
        );
        // Matching lit highlight on each claw so they look like the same rounded shell as the body.
        params.push(
            DrawParam::default()
                .dest(claw_l + light_dir * claw_radius * 0.5)
                .scale(Vec2::splat(claw_radius * 0.55))
                .color(dome_color),
        );
        params.push(
            DrawParam::default()
                .dest(claw_r + light_dir * claw_radius * 0.5)
                .scale(Vec2::splat(claw_radius * 0.55))
                .color(dome_color),
        );
        params.push(
            DrawParam::default()
                .dest(draw_pos + rotate_offset(-eye_x, eye_y))
                .scale(Vec2::splat(eye_radius))
                .color(Color::WHITE),
        );
        params.push(
            DrawParam::default()
                .dest(draw_pos + rotate_offset(eye_x, eye_y))
                .scale(Vec2::splat(eye_radius))
                .color(Color::WHITE),
        );
        params.push(
            DrawParam::default()
                .dest(draw_pos + rotate_offset(-eye_x + pdx, eye_y + pdy))
                .scale(Vec2::splat(pupil_r))
                .color(Color::BLACK),
        );
        params.push(
            DrawParam::default()
                .dest(draw_pos + rotate_offset(eye_x + pdx, eye_y + pdy))
                .scale(Vec2::splat(pupil_r))
                .color(Color::BLACK),
        );
    });

    // Crab legs (6 lines): the leg root sits on the body's radius at `angle`, so rotating
    // the whole leg (root + direction) by the crab's facing is the same as just adding
    // `rotation` to `angle` before computing everything in world space directly.
    let leg_len = size * 0.7;
    let leg_color = Color::from_rgb(200, 50, 50);
    // Hoisted out of the loop: `time_since_start()` reads the system clock (Instant::now())
    // every call, and the value is identical across all 6 legs — with a long conga train this
    // was 6 redundant clock reads per crab per frame (1000s/frame at high crab counts) for a
    // value that never changes within the loop.
    let time = ctx.time.time_since_start().as_secs_f32();
    // Single thread-local borrow for all 6 legs, same reasoning as CRAB_BODY_PARAMS above.
    CRAB_LEG_PARAMS.with(|params| {
        let mut params = params.borrow_mut();
        for i in 0..6 {
            let base_angle = std::f32::consts::PI * (0.25 + i as f32 / 6.0);
            let phase = (crab.pos.x + crab.pos.y) * 0.05;
            let wiggle_speed = 2.0 + crab.speed * 0.08; // scale with crab speed
            let wiggle_amp = 0.18 + beat_phase * 0.12;
            let wiggle = (time * wiggle_speed * (1.0 + beat_phase * 0.5) + phase + i as f32).sin() * wiggle_amp;
            let angle = base_angle + wiggle + rotation;
            let root = draw_pos + Vec2::new(angle.cos(), angle.sin()) * (size / 2.0);
            // Deferred: collected here and drawn as one instanced batch by flush_crab_legs() instead
            // of an individual canvas.draw() per leg per crab (see CRAB_LEG_PARAMS above).
            params.push(
                DrawParam::default()
                    .dest(root)
                    .rotation(angle)
                    .scale(Vec2::new(leg_len, 2.0))
                    .color(leg_color),
            );
        }
    });

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
    aura_color: [f32; 3],
) -> ggez::GameResult {
    let radius = size * 0.85;
    let pulse = (time * 6.0).sin() * 0.5 + 0.5; // 0..1

    // Pulsing aura ring behind the boss — tinted to the archetype (gold King Crab, cyan Tide Boss),
    // breathing with the beat of the track. Reuses the same STROKE_CIRCLE_CACHE every other ring
    // effect in this file draws from, instead of rebuilding a fresh mesh every frame this boss is alive.
    let aura_radius = radius * (1.12 + pulse * 0.08);
    let aura = cached_stroke_circle(ctx, aura_radius, 3.0)?;
    canvas.draw(
        &aura,
        DrawParam::default()
            .dest(pos)
            .color(Color::new(aura_color[0], aura_color[1], aura_color[2], 0.30 + pulse * 0.25)),
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
        Some(mesh) => mesh.clone(),
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh).clone()
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
    // Batched into one instanced draw instead of up to 20 individual canvas.draw() calls per
    // frame — the flashlight is on for most of active play, so this ran every frame the beam
    // was lit. Same reusable-thread-local-InstanceArray pattern as draw_ambient_motes/particles.
    FLASHLIGHT_DUST_INSTANCES.with(|cell| -> ggez::GameResult {
        let mut slot = cell.borrow_mut();
        let instances = slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
        instances.set((0..MOTE_COUNT).filter_map(|i| {
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
                return None;
            }
            let r = size + catch_flare * 0.8;
            Some(
                DrawParam::default()
                    .dest(pos)
                    .scale(Vec2::new(r, r))
                    .color(Color::new(1.0, 0.96, 0.82, alpha.clamp(0.0, 1.0))),
            )
        }));
        if !instances.instances().is_empty() {
            canvas.draw_instanced_mesh(unit_circle, instances, DrawParam::default());
        }
        Ok(())
    })?;

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
    // 0..1 "on fire" factor driven by the live Groove Gamble multiplier: at 0 the rope is its
    // usual rainbow neon; as the risked streak climbs it visibly overheats — wider hotter glow,
    // more energetic wiggle, and the segment colors bleed toward white-hot amber so the reward at
    // stake reads directly on the conga train the player is staring at.
    gamble_heat: f32,
) -> ggez::GameResult {
    if chain_links.is_empty() {
        return Ok(());
    }
    let heat = gamble_heat.clamp(0.0, 1.0);

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

    // Total chain length, used both for hue mapping and to scale sub-segment resolution below.
    let total_links = chain_links.len() as f32;

    // Number of sub-segments per chain link — more = smoother curve. This is rebuilt from
    // scratch every frame (sine + HSV-ish color math per micro-segment) before the batched
    // instanced draw below, and chain_count grows unbounded over a run (a long train can hit
    // 100+ links). At a flat 14 segs/link that's 1500+ trig calls a frame just to build the
    // rope geometry, invisible in the two draw calls but very visible in frame time. Scale the
    // per-link resolution down as the train gets long so total micro-segment work stays roughly
    // bounded (~700 segs) instead of growing linearly forever — a long rope is mostly straight
    // runs between links anyway, so fewer wiggle segments per link is indistinguishable in
    // motion, while short/medium trains (the common case) keep the full smooth 14.
    const MAX_TOTAL_SEGS: usize = 700;
    let segs: usize = if total_links > 0.0 {
        (MAX_TOTAL_SEGS as f32 / total_links).floor().clamp(4.0, 14.0) as usize
    } else {
        14
    };
    // A hot streak whips the rope harder and thicker so it looks like it's straining with energy.
    // Amplitude of the sine-wave wiggle (pixels perpendicular to the link)
    let wiggle_amp = 5.0 + beat_intensity * 8.0 + heat * 5.0;
    // Speed of the wave traveling along the rope (faster on beat, faster still when overheating)
    let wave_speed = 3.5 + beat_intensity * 2.5 + heat * 3.0;
    let thickness = 3.0 + beat_intensity * 4.5 + heat * 2.5;
    let alpha_base: f32 = (0.55 + beat_intensity * 0.4 + heat * 0.25).min(1.0);

    // Build the full ordered list of waypoints: player → crab0 → crab1 → …
    let player_center = player_pos + Vec2::new(24.0, 24.0);

    CONGA_WAYPOINT_BUF.with(|wbuf| -> ggez::GameResult {
        let mut waypoints = wbuf.borrow_mut();
        waypoints.clear();
        waypoints.push(player_center);
        for &(_, pos) in chain_links {
            waypoints.push(pos);
        }

        CONGA_SEGMENT_BUF.with(|buf| -> ggez::GameResult {
            let mut seg_buf = buf.borrow_mut();
            seg_buf.clear();

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

                // Subdivide into `segs` micro-segments (scaled down for long trains, see above)
                let mut prev_point = start;
                for seg in 0..=segs {
                    let t = seg as f32 / segs as f32;

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
                        let mut rr = (r + boost).min(1.0);
                        let mut gg = (g + boost).min(1.0);
                        let mut bb = (b + boost).min(1.0);
                        // Overheat: pull each micro-segment toward a white-hot amber. A faint per-
                        // segment flicker keeps the fire alive rather than a flat tint. The rainbow
                        // still shows through underneath so a hot rope reads as the same rope, lit.
                        if heat > 0.0 {
                            let flicker = 0.85
                                + 0.15 * (time * 11.0 + link_idx as f32 * 2.3 + t * 6.0).sin();
                            let hot = heat * flicker;
                            rr = rr + (1.0 - rr) * hot;
                            gg = gg + (0.72 - gg) * hot;
                            bb = bb + (0.28 - bb) * hot * 0.6;
                        }

                        let seg_delta = point - prev_point;
                        let seg_len = seg_delta.length();
                        if seg_len > 0.5 {
                            let seg_angle = seg_delta.y.atan2(seg_delta.x);
                            seg_buf.push((prev_point, seg_angle, seg_len, [rr, gg, bb]));
                        }
                    }
                    prev_point = point;
                }
            }

            // Pass 1: main rope segments, plain alpha blend (whatever the canvas is already using).
            // Batched into one InstanceArray + draw_instanced_mesh instead of one canvas.draw()
            // per micro-segment (see CONGA_MAIN_INSTANCES doc comment).
            CONGA_MAIN_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                let mut inst_slot = inst_cell.borrow_mut();
                let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                instances.set(seg_buf.iter().map(|&(pos, angle, len, rgb)| {
                    let color = Color::new(rgb[0], rgb[1], rgb[2], alpha_base);
                    DrawParam::default()
                        .dest(pos)
                        .rotation(angle)
                        .scale(Vec2::new(len, thickness))
                        .color(color)
                }));
                canvas.draw_instanced_mesh(unit_line.clone(), instances, DrawParam::default());
                Ok(())
            })?;

            // Pass 2: neon glow, additive blend switched on once for the whole rope instead of
            // once per micro-segment. Same batching as pass 1.
            canvas.set_blend_mode(BlendMode::ADD);
            // Overheating widens and brightens the additive halo so a hot rope actually casts light.
            let glow_alpha = alpha_base * (0.35 + heat * 0.35);
            let glow_width = thickness * (2.2 + heat * 1.6);
            CONGA_GLOW_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                let mut inst_slot = inst_cell.borrow_mut();
                let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                instances.set(seg_buf.iter().map(|&(pos, angle, len, rgb)| {
                    let glow_color = Color::new(rgb[0], rgb[1], rgb[2], glow_alpha);
                    DrawParam::default()
                        .dest(pos)
                        .rotation(angle)
                        .scale(Vec2::new(len, glow_width))
                        .color(glow_color)
                }));
                canvas.draw_instanced_mesh(unit_line.clone(), instances, DrawParam::default());
                Ok(())
            })?;
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
    // 0..1 progress toward the next beat, where ~0 means the beat just landed and ~1 means it's
    // about to land again. Drives an approach ring that shrinks toward the marker so the player
    // can *anticipate* the downbeat and time on-beat tool hits, instead of only reacting after.
    beat_progress: f32,
    // True while the current instant counts as "on beat" (within BEAT_WINDOW). Flashes the marker
    // green so the exact hit window is unmistakable.
    on_beat: bool,
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

    // Approach ring: starts wide right after a beat and closes in on the marker as the next beat
    // nears, snapping tight exactly on the downbeat. This is the timing cue a rhythm player reads
    // to land PERFECT hits. Reuses the shared cached stroke circle so no per-frame mesh is built.
    let p = beat_progress.clamp(0.0, 1.0);
    let approach_r = base_r + (1.0 - p) * 46.0;
    // Fades in as it converges so a freshly-reset ring doesn't pop; brightens near the hit window.
    let ring_alpha = ((40.0 + p * p * 200.0) as u8).min(255);
    let ring_col = if on_beat {
        Color::from_rgba(120, 255, 140, 255)
    } else {
        Color::from_rgba(255, 220, 120, ring_alpha)
    };
    // The ring sweeps continuously from base_r to base_r+46 every single beat, so looking it up
    // in the shared stroke-circle cache at full precision (rounded to the nearest half-pixel)
    // missed on almost every frame — quietly building a brand-new GPU mesh buffer per frame for
    // the whole time the game runs. Quantize to the nearest 4px for the cache lookup only (the
    // draw call still positions/colors it per-frame via DrawParam, so the sweep still reads as
    // smooth); this bounds the ring to ~12 reusable mesh variants instead of one alloc per frame.
    let cache_r = (approach_r / 4.0).round() * 4.0;
    let approach = cached_stroke_circle(ctx, cache_r, 2.5)?;
    canvas.draw(&approach, DrawParam::default().dest(center).color(ring_col));

    let pulse_r = base_r + beat_intensity * 14.0;
    let alpha = ((80.0 + beat_intensity * 175.0) as u8).min(255);
    // The marker itself flashes green in the on-beat window, otherwise its usual warm amber.
    let marker_col = if on_beat {
        Color::from_rgba(150, 255, 160, alpha.max(200))
    } else {
        Color::from_rgba(255, 200, 50, alpha)
    };
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(center)
            .scale(Vec2::splat(pulse_r))
            .color(marker_col),
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

/// Reef DJ call-and-response HUD. Draws the four-beat phrase the rhythm boss called for the
/// current bar as a row of pips: a *hot* (called) beat is a big violet ring the player must echo
/// with the light, a silent beat is a small dim dot. The beat currently playing is ringed white so
/// you can read where you are in the bar. `phrase[i]` = beat i is hot; `current_beat` = beat_count%4;
/// `on_beat` flashes the active pip; `hit_flash` (0..1) blooms the whole row when a hot beat landed.
pub fn draw_reef_phrase(
    ctx: &mut Context,
    canvas: &mut Canvas,
    center: Vec2,
    phrase: [bool; 4],
    current_beat: usize,
    on_beat: bool,
    hit_flash: f32,
) -> ggez::GameResult {
    let unit_circle = match UNIT_CIRCLE.get() {
        Some(mesh) => mesh,
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh)
        }
    };
    let spacing = 34.0;
    let start_x = center.x - spacing * 1.5;
    let bloom = (hit_flash * 0.6).min(0.6);
    for i in 0..4 {
        let pos = Vec2::new(start_x + spacing * i as f32, center.y);
        let is_current = i == current_beat;
        if phrase[i] {
            // Hot beat — a filled violet pip, the "hit here" call. Brightens on the active beat and
            // blooms with hit_flash when the player just echoed a hot beat cleanly.
            let r = 9.0 + if is_current && on_beat { 5.0 } else { 0.0 } + bloom * 6.0;
            let a = if is_current { 255 } else { 170 };
            canvas.draw(
                unit_circle,
                DrawParam::default()
                    .dest(pos)
                    .scale(Vec2::splat(r))
                    .color(Color::from_rgba(
                        (185.0 + bloom * 70.0).min(255.0) as u8,
                        (90.0 + bloom * 120.0).min(255.0) as u8,
                        245,
                        a,
                    )),
            );
        } else {
            // Silent beat — a small dim dot, nothing to do here.
            canvas.draw(
                unit_circle,
                DrawParam::default()
                    .dest(pos)
                    .scale(Vec2::splat(4.0))
                    .color(Color::from_rgba(120, 100, 150, 120)),
            );
        }
        // The playhead: a white ring around whichever beat is sounding now, so the phrase reads as
        // a moving cursor over the four slots rather than a static pattern.
        if is_current {
            let ring = cached_stroke_circle(ctx, 15.0, 2.0)?;
            let ring_a = if on_beat { 255 } else { 130 };
            canvas.draw(
                &ring,
                DrawParam::default()
                    .dest(pos)
                    .color(Color::from_rgba(255, 255, 255, ring_a)),
            );
        }
    }
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
    // A frenzy wave recolors the telegraph gold and pumps it harder, so the special spike
    // reads as different long before it lands.
    frenzy: bool,
) -> ggez::GameResult {
    let unit_circle = match UNIT_CIRCLE.get() {
        Some(mesh) => mesh,
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh)
        }
    };
    let a = anticipation.clamp(0.0, 1.0);
    // Frenzy telegraphs are gold and swing wider on each throb; normal ones are the calm cyan.
    let (halo_rgb, ring_rgb, throb_gain) = if frenzy {
        ((255, 200, 60), (255, 225, 120), 8.0)
    } else {
        ((80, 220, 255), (120, 235, 255), 4.0)
    };
    // Ring starts wide and tightens toward the indicator as the drop nears.
    let throb = (beat_phase * std::f32::consts::TAU).sin() * 0.5 + 0.5;
    let ring_r = 58.0 - a * 20.0 + throb * throb_gain;
    // Soft filled halo behind the indicator — cheap, no stroke mesh needed. Brightens with
    // anticipation so the impending drop is unmistakable.
    let halo_alpha = ((28.0 + a * 70.0) as u8).min(140);
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(center)
            .scale(Vec2::splat(ring_r + 6.0))
            .color(Color::from_rgba(halo_rgb.0, halo_rgb.1, halo_rgb.2, halo_alpha)),
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
            .color(Color::from_rgba(ring_rgb.0, ring_rgb.1, ring_rgb.2, bright)),
    );
    // Second, outer contra-rotating gold ring for frenzy waves only — cheap extra flourish that
    // makes the special wave unmistakable without another mechanic.
    if frenzy {
        let outer = cached_stroke_circle(ctx, ring_r + 14.0 + throb * 6.0, 2.0)?;
        canvas.draw(
            &outer,
            DrawParam::default()
                .dest(center)
                .color(Color::from_rgba(255, 170, 40, ((70.0 + a * 120.0) as u8).min(210))),
        );
    }
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

/// One caught crab marching into the delivery pen after a bank. Instead of the banked train
/// vanishing instantly, each delivered crab spawns a marcher that skips from where it was into
/// the pen center, staggered by its old chain index so the whole conga line files in one-by-one
/// like a little parade, then pops in a sparkle on arrival. Purely cosmetic — the score is already
/// awarded the instant the bank happens.
pub struct PennedMarcher {
    pub start: Vec2,     // where the crab was when the train banked
    pub target: Vec2,    // pen center it's marching toward
    pub color: [f32; 3], // the crab's own body color
    pub size: f32,       // body radius (from its scale)
    pub delay: f32,      // seconds before it starts marching (stagger by chain index)
    pub t: f32,          // 0..1 march progress once delay elapses
    pub speed: f32,      // 1/seconds to cover the march
    pub done: bool,      // true once it has arrived (its arrival pop was emitted)
}

pub struct PennedMarcherSystem {
    pub marchers: Vec<PennedMarcher>,
}

impl PennedMarcherSystem {
    pub fn new() -> Self {
        Self { marchers: Vec::new() }
    }

    /// Queue the just-banked train to march into the pen. `crabs` is (pos, color, size) per
    /// delivered crab in chain order (head first) so they file in staggered down the line.
    pub fn spawn_train(&mut self, pen: Vec2, crabs: &[(Vec2, [f32; 3], f32)]) {
        // Cap the queue so an enormous bank can't spawn hundreds of marchers at once.
        const MAX_MARCHERS: usize = 40;
        let count = crabs.len().min(MAX_MARCHERS);
        for (i, &(pos, color, size)) in crabs.iter().take(count).enumerate() {
            // Later links in the line start marching a beat behind the head — a rolling parade.
            let delay = i as f32 * 0.045;
            // Shorter marches finish a touch quicker so nothing drags; keeps the parade snappy.
            let dist = pos.distance(pen).max(1.0);
            let speed = (260.0 / dist).clamp(0.9, 2.4);
            self.marchers.push(PennedMarcher {
                start: pos,
                target: pen,
                color,
                size,
                delay,
                t: 0.0,
                speed,
                done: false,
            });
        }
    }

    /// Advance every marcher. Returns the arrival (pos, color) of any that reached the pen this
    /// frame so the caller can pop a sparkle burst there via the particle system.
    pub fn update(&mut self, dt: f32) -> Vec<(Vec2, [f32; 3])> {
        let mut arrivals = Vec::new();
        for m in self.marchers.iter_mut() {
            if m.delay > 0.0 {
                m.delay -= dt;
                continue;
            }
            m.t = (m.t + dt * m.speed).min(1.0);
            if m.t >= 1.0 && !m.done {
                m.done = true;
                arrivals.push((m.target, m.color));
            }
        }
        // Drop marchers that have arrived (they popped their sparkle already).
        self.marchers.retain(|m| !m.done);
        arrivals
    }

    /// Current drawn position of a marcher: eased march from start toward the pen with a small
    /// arc lift so it hops in rather than sliding flat.
    fn marcher_draw(m: &PennedMarcher) -> (Vec2, f32, f32) {
        // Ease-in-out so it accelerates off the mark and settles into the pen.
        let t = m.t;
        let eased = t * t * (3.0 - 2.0 * t);
        let pos = m.start.lerp(m.target, eased);
        // A parabolic hop: peaks mid-march, zero at both ends.
        let lift = (t * (1.0 - t)) * 4.0 * 26.0;
        // Shrink as it nears the pen so it reads as "filing away" into the corral.
        let shrink = 1.0 - 0.35 * eased;
        (pos - Vec2::new(0.0, lift), lift, shrink)
    }
}

/// Draw the crabs currently marching into the pen. Cheap: a shadow + colored body disc + a
/// bright rim per marcher, reusing the shared unit circle — it evokes the crab silhouette
/// without rigging full legs, since these are only on screen for a fraction of a second.
/// All three passes are batched into one InstanceArray draw call each (see
/// MARCHER_SHADOW_INSTANCES et al.) instead of issuing a canvas.draw() per marcher per pass, the
/// same technique already used for crab legs/bodies and catch trails.
pub fn draw_penned_marchers(
    ctx: &mut Context,
    canvas: &mut Canvas,
    system: &PennedMarcherSystem,
    time: f32,
) -> ggez::GameResult {
    if system.marchers.is_empty() {
        return Ok(());
    }
    let unit_circle = match UNIT_CIRCLE.get() {
        Some(mesh) => mesh.clone(),
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh).clone()
        }
    };

    MARCHER_SHADOW_PARAMS.with(|shadow_cell| -> ggez::GameResult {
        MARCHER_BODY_PARAMS.with(|body_cell| -> ggez::GameResult {
            MARCHER_RIM_PARAMS.with(|rim_cell| -> ggez::GameResult {
                let mut shadow_params = shadow_cell.borrow_mut();
                let mut body_params = body_cell.borrow_mut();
                let mut rim_params = rim_cell.borrow_mut();
                shadow_params.clear();
                body_params.clear();
                rim_params.clear();

                for m in &system.marchers {
                    if m.delay > 0.0 {
                        continue;
                    }
                    let (pos, lift, shrink) = PennedMarcherSystem::marcher_draw(m);
                    let body_r = CRAB_SIZE * 0.5 * m.size * shrink;

                    let sh_scale = (1.0 - lift / 60.0).clamp(0.4, 1.0);
                    let sh_alpha = ((1.0 - lift / 55.0) * 90.0).clamp(15.0, 90.0) as u8;
                    shadow_params.push(
                        DrawParam::default()
                            .dest(pos + Vec2::new(0.0, body_r * 0.7 + lift * 0.6))
                            .scale(Vec2::new(body_r * sh_scale, body_r * 0.45 * sh_scale))
                            .color(Color::from_rgba(0, 0, 0, sh_alpha)),
                    );

                    // A little beat-independent bob so the parade feels alive.
                    let bob = 1.0 + 0.08 * (time * 9.0 + m.start.x).sin();
                    let [r, g, b] = m.color;
                    body_params.push(
                        DrawParam::default()
                            .dest(pos)
                            .scale(Vec2::splat(body_r * bob))
                            .color(Color::new(r, g, b, 1.0)),
                    );

                    rim_params.push(
                        DrawParam::default()
                            .dest(pos - Vec2::new(0.0, body_r * 0.3))
                            .scale(Vec2::splat(body_r * 0.45))
                            .color(Color::new((r + 0.4).min(1.0), (g + 0.4).min(1.0), (b + 0.4).min(1.0), 0.7)),
                    );
                }

                // Shadows first (normal blend so they read as ground contact).
                MARCHER_SHADOW_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                    let mut inst_slot = inst_cell.borrow_mut();
                    let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                    instances.set(shadow_params.iter().copied());
                    canvas.draw_instanced_mesh(unit_circle.clone(), instances, DrawParam::default());
                    Ok(())
                })?;

                // Bodies + rims in additive so they glow warm as they file in.
                let orig_blend = canvas.blend_mode();
                MARCHER_BODY_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                    let mut inst_slot = inst_cell.borrow_mut();
                    let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                    instances.set(body_params.iter().copied());
                    canvas.draw_instanced_mesh(unit_circle.clone(), instances, DrawParam::default());
                    Ok(())
                })?;

                // Bright rim highlight, additive, so the marchers pop against the pen glow.
                canvas.set_blend_mode(BlendMode::ADD);
                MARCHER_RIM_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                    let mut inst_slot = inst_cell.borrow_mut();
                    let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                    instances.set(rim_params.iter().copied());
                    canvas.draw_instanced_mesh(unit_circle, instances, DrawParam::default());
                    Ok(())
                })?;
                canvas.set_blend_mode(orig_blend);
                Ok(())
            })
        })
    })
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

    // Reuse the cached unit-line mesh (placed per segment via DrawParam) instead of calling
    // Mesh::new_line fresh for every arc segment — this built up to 64 brand-new GPU line
    // buffers (32 segments x main+glow passes) every single frame the combo meter was on
    // screen, which is most of active play once a run gets going.
    let line = unit_line(ctx)?;

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
        let d = p0.distance(p1);
        if d > 0.5 {
            let dir = (p1 - p0) / d;
            canvas.draw(
                line,
                DrawParam::default()
                    .dest(p0)
                    .rotation(dir.y.atan2(dir.x))
                    .scale(Vec2::new(d, 3.0))
                    .color(tier_color),
            );
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
        let d = p0.distance(p1);
        if d > 0.5 {
            let dir = (p1 - p0) / d;
            canvas.draw(
                line,
                DrawParam::default()
                    .dest(p0)
                    .rotation(dir.y.atan2(dir.x))
                    .scale(Vec2::new(d, 6.0))
                    .color(glow_color),
            );
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

    let triangle = match UNIT_TRIANGLE.get() {
        Some(mesh) => mesh,
        None => {
            let pts = [[1.0_f32, 0.0], [-0.5, 0.75], [-0.5, -0.75]];
            let mesh = Mesh::new_polygon(ctx, DrawMode::fill(), &pts, Color::WHITE)?;
            UNIT_TRIANGLE.get_or_init(|| mesh)
        }
    };

    let triangle = triangle.clone();
    RADAR_ARROW_PARAMS.with(|arrow_cell| -> ggez::GameResult {
        RADAR_GLOW_PARAMS.with(|glow_cell| -> ggez::GameResult {
            let mut arrow_params = arrow_cell.borrow_mut();
            let mut glow_params = glow_cell.borrow_mut();
            arrow_params.clear();
            glow_params.clear();

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

                // Arrow points toward `angle` from the edge position — the cached unit triangle
                // already points along +x with its tip at local (1,0), so a rotation to `angle`
                // plus a scale by `arrow_size` reproduces the old per-crab tip/left/right
                // geometry exactly, without rebuilding it.
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
                arrow_params.push(
                    DrawParam::default()
                        .dest(origin)
                        .rotation(angle)
                        .scale(Vec2::splat(arrow_size))
                        .color(color),
                );

                // Glow outline — same shape at 1.5x scale, matching the old glow_pts geometry.
                let glow_color =
                    Color::new(r.min(1.0), g.min(1.0), b.min(1.0), 0.35 + beat_intensity * 0.15);
                glow_params.push(
                    DrawParam::default()
                        .dest(origin)
                        .rotation(angle)
                        .scale(Vec2::splat(arrow_size * 1.5))
                        .color(glow_color),
                );
            }

            if !arrow_params.is_empty() {
                RADAR_ARROW_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                    let mut inst_slot = inst_cell.borrow_mut();
                    let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                    instances.set(arrow_params.iter().copied());
                    canvas.draw_instanced_mesh(triangle.clone(), instances, DrawParam::default());
                    Ok(())
                })?;
                RADAR_GLOW_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                    let mut inst_slot = inst_cell.borrow_mut();
                    let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                    instances.set(glow_params.iter().copied());
                    canvas.draw_instanced_mesh(triangle.clone(), instances, DrawParam::default());
                    Ok(())
                })?;
            }
            Ok(())
        })
    })?;

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

    // Group both passes' DrawParams by the same (radius, thickness) key cached_stroke_circle()
    // rounds to internally, so rings that land in the same mesh bucket (typically the whole
    // conga chain on a given beat, since they share age) get instanced together instead of each
    // costing its own canvas.draw() call. Reused scratch map, cleared each call rather than
    // reallocated.
    CHAIN_RING_GROUPS.with(|groups_cell| -> ggez::GameResult {
        let mut groups = groups_cell.borrow_mut();
        for v in groups.values_mut() {
            v.clear();
        }

        for &(pos, age, color) in rings {
            // age 0..1: radius grows from 8 to 70, alpha fades from bright to zero
            let radius = 8.0 + age * 62.0;
            let alpha = ((1.0 - age) * 0.65).clamp(0.0, 1.0);
            // Stroke thickness tapers as ring expands
            let thickness = 3.5 * (1.0 - age * 0.7);

            // Main ring. Ensures the mesh is built/cached, then groups its DrawParam under the
            // same key cached_stroke_circle used (via the shared stroke_circle_key helper, so
            // the two can never drift out of sync), so this ring instances alongside every other
            // ring sharing that exact (radius, thickness) bucket this frame.
            let key = stroke_circle_key(radius, thickness);
            cached_stroke_circle(ctx, radius, thickness)?;
            groups.entry(key).or_default().push(
                DrawParam::default()
                    .dest(pos)
                    .color(Color::new(color[0], color[1], color[2], alpha)),
            );

            // Soft outer glow ring (larger radius, lower alpha)
            if age < 0.7 {
                let glow_alpha = alpha * 0.3;
                let glow_radius = radius + 4.0;
                let glow_thickness = thickness * 2.0;
                let glow_key = stroke_circle_key(glow_radius, glow_thickness);
                cached_stroke_circle(ctx, glow_radius, glow_thickness)?;
                groups.entry(glow_key).or_default().push(
                    DrawParam::default()
                        .dest(pos)
                        .color(Color::new(color[0], color[1], color[2], glow_alpha)),
                );
            }
        }

        CHAIN_RING_INSTANCES.with(|inst_cell| -> ggez::GameResult {
            let mut instances = inst_cell.borrow_mut();
            for (key, params) in groups.iter() {
                if params.is_empty() {
                    continue;
                }
                // Same mesh cached_stroke_circle() already built above for this key.
                let mesh = STROKE_CIRCLE_CACHE.with(|c| c.borrow().get(key).cloned());
                let Some(mesh) = mesh else { continue };
                let inst = instances
                    .entry(*key)
                    .or_insert_with(|| InstanceArray::new(ctx, None));
                inst.set(params.iter().copied());
                canvas.draw_instanced_mesh(mesh, inst, DrawParam::default());
            }
            Ok(())
        })
    })?;

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

/// Draw the whip-streaks that yank caught crabs into the head of the train. Each `(from, to, age,
/// rgb)` is a bright line from where the crab was caught toward the player; as `age` climbs the
/// streak's tail retracts toward the head (the crab "arriving") and fades, with a white-hot spark
/// riding the retracting tail. Purely visual juice so a catch reads as a snap-in, not a blink-on.
/// Additive-blended and drawn from a single cached unit rectangle so it stays cheap under a swarm.
pub fn draw_catch_trails(
    ctx: &mut Context,
    canvas: &mut Canvas,
    trails: &[(Vec2, Vec2, f32, [f32; 3])],
) -> ggez::GameResult {
    if trails.is_empty() {
        return Ok(());
    }
    let line = unit_line(ctx)?;
    let spark = unit_circle(ctx)?;
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    TRAIL_GLOW_INSTANCES.with(|glow_cell| -> ggez::GameResult {
        TRAIL_CORE_INSTANCES.with(|core_cell| -> ggez::GameResult {
            TRAIL_SPARK_INSTANCES.with(|spark_cell| -> ggez::GameResult {
                let mut glow_slot = glow_cell.borrow_mut();
                let mut core_slot = core_cell.borrow_mut();
                let mut spark_slot = spark_cell.borrow_mut();
                let glow = glow_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                let core = core_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                let sparks = spark_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));

                glow.set(trails.iter().filter_map(|&(from, to, age, color)| {
                    let (tail, seg_len, angle, fade) = trail_geometry(from, to, age)?;
                    let thickness = (2.0 + fade * 5.0).max(1.0);
                    Some(
                        DrawParam::default()
                            .dest(tail)
                            .rotation(angle)
                            .scale(Vec2::new(seg_len, thickness * 2.4))
                            .color(Color::new(color[0], color[1], color[2], fade * 0.30)),
                    )
                }));
                core.set(trails.iter().filter_map(|&(from, to, age, color)| {
                    let (tail, seg_len, angle, fade) = trail_geometry(from, to, age)?;
                    let thickness = (2.0 + fade * 5.0).max(1.0);
                    // Bright core line, blending from the crab color toward white-hot.
                    let cr = (color[0] * 0.5 + 0.5).min(1.0);
                    let cg = (color[1] * 0.5 + 0.5).min(1.0);
                    let cb = (color[2] * 0.5 + 0.5).min(1.0);
                    Some(
                        DrawParam::default()
                            .dest(tail)
                            .rotation(angle)
                            .scale(Vec2::new(seg_len, thickness))
                            .color(Color::new(cr, cg, cb, fade * 0.85)),
                    )
                }));
                sparks.set(trails.iter().filter_map(|&(from, to, age, _)| {
                    let (tail, _, _, fade) = trail_geometry(from, to, age)?;
                    // White-hot spark riding the retracting tail — the crab being reeled in.
                    let spark_r = (2.5 + fade * 5.0).max(1.0);
                    Some(
                        DrawParam::default()
                            .dest(tail)
                            .scale(Vec2::splat(spark_r))
                            .color(Color::new(1.0, 1.0, 1.0, fade * 0.9)),
                    )
                }));

                // Every trail can filter out (short/fully-retracted segments return None from
                // trail_geometry), leaving an InstanceArray that `.set()` shrank to zero capacity.
                // ggez's draw_instanced flush rebuilds the buffer at len and asserts capacity > 0,
                // so drawing an empty array panics — guard each pass to skip when it's empty.
                if !glow.instances().is_empty() {
                    canvas.draw_instanced_mesh(line.clone(), glow, DrawParam::default());
                }
                if !core.instances().is_empty() {
                    canvas.draw_instanced_mesh(line.clone(), core, DrawParam::default());
                }
                if !sparks.instances().is_empty() {
                    canvas.draw_instanced_mesh(spark.clone(), sparks, DrawParam::default());
                }
                Ok(())
            })
        })
    })?;

    canvas.set_blend_mode(original_blend);
    Ok(())
}

/// Shared per-trail geometry for `draw_catch_trails`' three instanced passes: the retracting
/// tail position, the remaining segment length, the line's rotation angle, and the fade (1 =
/// just spawned, 0 = fully arrived). Returns `None` for trails too short/far-retracted to draw
/// (kept as a filter so each pass skips them identically, matching the old per-trail `continue`s).
#[inline]
fn trail_geometry(from: Vec2, to: Vec2, age: f32) -> Option<(Vec2, f32, f32, f32)> {
    // A short lead-in (negative age from on-beat catches) reads as a fully-drawn streak before
    // it starts retracting. Clamp so nothing draws off the front of the animation.
    let a = age.clamp(0.0, 1.0);
    let fade = 1.0 - a;
    let delta = to - from;
    let len = delta.length();
    if len < 1.0 {
        return None;
    }
    let angle = delta.y.atan2(delta.x);
    // The tail retracts toward the head as the crab arrives: at a=0 the whole line shows, near
    // a=1 only the last sliver by the head remains. Ease-in so the snap accelerates inward.
    let head_frac = a * a;
    let tail = from + delta * head_frac;
    let seg_len = len * (1.0 - head_frac);
    if seg_len < 1.0 {
        return None;
    }
    Some((tail, seg_len, angle, fade))
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

/// Draw the Tide Boss's shockwave pulses — a heavy tidal double-ring in deep cyan that sweeps
/// outward from the boss and shoves the herd/train away. `pulses` is (center, current radius);
/// `max_radius` is the pulse reach, used to fade the front as it dissipates.
pub fn draw_tide_pulses(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pulses: &[(Vec2, f32)],
    max_radius: f32,
) -> ggez::GameResult {
    if pulses.is_empty() {
        return Ok(());
    }
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    for &(center, radius) in pulses {
        let frac = (radius / max_radius).clamp(0.0, 1.5);
        let fade = (1.0 - (frac / 1.25)).clamp(0.0, 1.0);
        if fade <= 0.0 {
            continue;
        }
        // Thick leading front — a wall of water.
        let thickness = (7.0 * fade).max(1.5);
        let front = cached_stroke_circle(ctx, radius.max(1.0), thickness)?;
        canvas.draw(
            &front,
            DrawParam::default()
                .dest(center)
                .color(Color::new(0.25, 0.7, 1.0, fade * 0.8)),
        );
        // Trailing echo ring for a churning surge feel.
        let echo = cached_stroke_circle(ctx, (radius - 22.0).max(1.0), thickness * 0.7)?;
        canvas.draw(
            &echo,
            DrawParam::default()
                .dest(center)
                .color(Color::new(0.1, 0.5, 0.9, fade * 0.4)),
        );
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

/// Draw the rhythm "Call" pulse — concentric magenta rings that COLLAPSE inward toward the player,
/// reading as a summon (pull-in), opposite of the whistle's outward horn-blast. `pulse` is 1..0
/// (fresh→gone); `reach` is how far out the outermost ring starts. Additive so it glows on the beat.
pub fn draw_call_ring(
    ctx: &mut Context,
    canvas: &mut Canvas,
    center: Vec2,
    pulse: f32,
    reach: f32,
) -> ggez::GameResult {
    if pulse <= 0.0 {
        return Ok(());
    }
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    // Three rings marching inward as the pulse decays — a beckoning "come here" cadence.
    for (i, phase) in [0.0_f32, 0.33, 0.66].iter().enumerate() {
        let p = (pulse - phase).rem_euclid(1.0);
        let r = reach * p; // collapses toward the player as p → 0
        if r > 4.0 {
            let alpha = (pulse * (1.0 - p) * 0.8).clamp(0.0, 1.0);
            let thickness = 2.0 + 4.0 * (1.0 - p);
            let ring = cached_stroke_circle(ctx, r, thickness)?;
            let hue = 0.5 + 0.5 * i as f32 / 3.0;
            canvas.draw(
                &ring,
                DrawParam::default()
                    .dest(center)
                    .color(Color::new(1.0, 0.3 + 0.2 * hue, 0.9, alpha)),
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

/// Draw the Downbeat Slam shockwave — the rhythm-ultimate blast. A massive, thick gold ring
/// erupting outward with a hot white leading crest and a couple of chasing echo rings, reading as
/// the biggest, most celebratory wave in the game (fitting its full-Groove, on-beat cost). `radius`
/// is the current front, `max_radius` its full reach; additive so it blooms bright on the beat.
pub fn draw_slam_ring(
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
    let fade = 1.0 - frac; // brightest at the burst, gone by full reach

    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    // Thick gold leading wall — the biggest ring in the game.
    let thickness = (16.0 * fade + 3.0).max(3.0);
    let front = cached_stroke_circle(ctx, radius, thickness)?;
    canvas.draw(
        &front,
        DrawParam::default()
            .dest(center)
            .color(Color::new(1.0, 0.85, 0.25, (fade * 0.95).clamp(0.0, 1.0))),
    );
    // Hot white crest riding the very front for a snappy leading edge.
    let crest = cached_stroke_circle(ctx, radius, 2.5)?;
    canvas.draw(
        &crest,
        DrawParam::default()
            .dest(center)
            .color(Color::new(1.0, 1.0, 0.92, (fade * 1.0).clamp(0.0, 1.0))),
    );
    // Two trailing echo rings for a booming, layered "wham".
    for (offset, alpha_scale) in [(40.0_f32, 0.5_f32), (84.0_f32, 0.28_f32)] {
        let er = radius - offset;
        if er > 3.0 {
            let echo = cached_stroke_circle(ctx, er, thickness * 0.6)?;
            canvas.draw(
                &echo,
                DrawParam::default().dest(center).color(Color::new(
                    1.0,
                    0.72,
                    0.3,
                    (fade * alpha_scale).clamp(0.0, 1.0),
                )),
            );
        }
    }

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
#[allow(clippy::too_many_arguments)]
pub fn draw_delivery_pen(
    ctx: &mut Context,
    canvas: &mut Canvas,
    center: Vec2,
    radius: f32,
    time: f32,
    beat_intensity: f32,
    ready: bool,
    // 0..1 anticipation: how big the uncashed haul is (bigger train = a hungrier, hotter, faster
    // pen), further boosted as the loaded train closes in on the pen. Drives the "this is about to
    // be a jackpot" telegraph so the payoff builds *before* the bank, not only after it.
    haul: f32,
    flash: f32,
) -> ggez::GameResult {
    let haul = haul.clamp(0.0, 1.0);
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

    // Breathing pulse — gentle when idle, urgent when there's a train to bank, and faster still the
    // fatter (and closer) the haul, so a big jackpot approach visibly winds the pen up.
    let pulse_speed = if ready { 6.0 + haul * 5.0 } else { 2.2 };
    let pulse = 0.5 + 0.5 * (time * pulse_speed).sin();
    let beat = beat_intensity.clamp(0.0, 1.0);

    // Warm goal-zone fill (normal blend so it reads as a marked patch of ground, not a glow).
    let fill_alpha = if ready { 0.16 + 0.12 * pulse + haul * 0.12 } else { 0.08 + 0.04 * pulse };
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(center)
            .scale(Vec2::splat(radius))
            .color(Color::new(1.0, 0.82, 0.28, fill_alpha)),
    );

    let orig_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    // Outer boundary ring — the "fence line" of the pen. Greenish when ready, and as the haul
    // grows it heats from that go-green toward a hot jackpot gold, so a big incoming train reads as
    // "money" before you even bank it.
    let (rr, rg, rb) = if ready {
        (0.5 + haul * 0.5, 1.0, 0.5 - haul * 0.25)
    } else {
        (1.0, 0.82, 0.35)
    };
    let ring_alpha = if ready { 0.55 + 0.35 * pulse + haul * 0.1 } else { 0.3 + 0.15 * pulse };
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

    // Anticipation "reach" ring — a second boundary that swells outward past the fence and fades,
    // pulsing faster and reaching further the bigger the incoming haul. It's the pen visibly
    // straining toward a fat train, telegraphing the jackpot as you drive it in. Only shows once
    // there's a real haul building (haul > ~a couple crabs' worth) so it stays quiet for small runs.
    if ready && haul > 0.12 {
        let reach_phase = (time * (2.0 + haul * 4.0)).sin() * 0.5 + 0.5; // 0..1
        let reach_r = radius * (1.0 + (0.15 + haul * 0.5) * reach_phase);
        let reach = cached_stroke_circle(ctx, reach_r, 2.0 + haul * 2.0)?;
        canvas.draw(
            &reach,
            DrawParam::default()
                .dest(center)
                .color(Color::new(
                    0.6 + haul * 0.4,
                    1.0,
                    0.45,
                    (haul * 0.55 * (1.0 - reach_phase)).clamp(0.0, 1.0),
                )),
        );
    }

    // Buoy posts around the rim, slowly turning like a rotating corral — spinning up with the haul.
    let post_count = 10;
    let spin = time * if ready { 0.9 + haul * 2.5 } else { 0.35 };
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

/// Draw a directional guide toward the delivery pen while the player has an uncashed train.
///
/// The pen relocates on every bank, so once you've built a conga line the game's biggest payoff
/// decision — "route the train to the pen and cash in" — is only legible if you can actually *find*
/// the pen. The crab radar already points to loose crabs at the screen edge; this is the same idea
/// for the goal zone, so building a train and hunting blindly for where to spend it never happens.
///
/// `urgency` (0..1) scales how insistent the guide reads — feed it the train size normalized against
/// some "big haul" cap so a fat, at-risk train pulls harder toward the pen than a couple of crabs.
/// When the pen is off-screen the arrow pins to the screen edge (like the crab radar); when it's
/// on-screen but not yet reached, a softer floating chevron hovers beside it pointing in. Purely a
/// guide overlay: no gameplay effect, all draws reuse the cached unit line/circle meshes.
#[allow(clippy::too_many_arguments)]
pub fn draw_pen_guide(
    ctx: &mut Context,
    canvas: &mut Canvas,
    player_center: Vec2,
    pen_pos: Vec2,
    pen_radius: f32,
    width: f32,
    height: f32,
    urgency: f32,
    beat_intensity: f32,
    time: f32,
) -> ggez::GameResult {
    let to_pen = pen_pos - player_center;
    let dist = to_pen.length();
    // Already at (or basically on) the pen — the pen's own beacon takes over, no guide needed.
    if dist < pen_radius * 1.2 {
        return Ok(());
    }
    let dir = to_pen.normalize_or_zero();
    if dir == Vec2::ZERO {
        return Ok(());
    }
    let angle = dir.y.atan2(dir.x);

    let u = urgency.clamp(0.0, 1.0);
    let beat = beat_intensity.clamp(0.0, 1.0);
    let unit_line = unit_line(ctx)?;
    let unit_circle = unit_circle(ctx)?;

    let margin = 30.0_f32;
    let on_screen = pen_pos.x > margin
        && pen_pos.x < width - margin
        && pen_pos.y > margin
        && pen_pos.y < height - margin;

    let orig_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    // Warm green-gold, matching the pen's "come cash in" palette. Brightens with urgency + beat.
    let bright = (0.6 + u * 0.35 + beat * 0.15).clamp(0.0, 1.0);
    let col = Color::new(0.55 * bright + 0.25, 1.0 * bright, 0.5 * bright + 0.15, bright);

    // Draw a downward-into-the-pen chevron (two wings) pointing along `angle`, plus a soft dot,
    // at `at` with size `size`. Reused for both the edge-pinned and on-field cases.
    let mut chevron = |at: Vec2, size: f32| {
        let wing = size;
        let core = size * 0.55;
        // Soft glow dot behind the chevron so it reads against busy ground.
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(at)
                .scale(Vec2::splat(core))
                .color(Color::new(col.r, col.g, col.b, col.a * 0.5)),
        );
        for spread in [2.2_f32, -2.2] {
            let wa = angle + spread;
            let d = Vec2::new(wa.cos(), wa.sin()) * wing;
            let len = d.length();
            let a = d.y.atan2(d.x);
            canvas.draw(
                unit_line,
                DrawParam::default()
                    .dest(at)
                    .rotation(a)
                    .scale(Vec2::new(len, (3.0 + u * 3.0).max(1.0)))
                    .color(col),
            );
        }
    };

    if on_screen {
        // Pen is visible: hover a gentle chevron just off the near side of the pen, bobbing on the
        // beat, nudging the eye toward it without cluttering the goal zone itself.
        let bob = (time * (3.0 + u * 3.0)).sin() * (4.0 + u * 4.0);
        let at = pen_pos - dir * (pen_radius + 22.0 + bob);
        chevron(at, 14.0 + u * 6.0);
    } else {
        // Pen is off-screen: pin a bigger, more insistent arrow to the screen edge in the pen's
        // direction (same clamp trick as the crab radar), so you know which way to haul the train.
        let edge = Vec2::new(
            (player_center.x + dir.x * 4000.0).clamp(margin, width - margin),
            (player_center.y + dir.y * 4000.0).clamp(margin, height - margin),
        );
        let pulse = 1.0 + beat * 0.4 + (time * 6.0).sin() * 0.1;
        chevron(edge, (18.0 + u * 10.0) * pulse);
        // A faint trailing tick behind the edge arrow so it reads as "keep going this way".
        let tail = edge - dir * (26.0 + u * 10.0);
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(tail)
                .scale(Vec2::splat(3.0 + u * 2.0))
                .color(Color::new(col.r, col.g, col.b, col.a * 0.4)),
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
    terrain: crate::levels::TerrainKind,
) -> ggez::GameResult {
    use crate::levels::TerrainKind;
    if pools.is_empty() || terrain == TerrainKind::Open {
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

    // Rock and Kelp reuse the same patch geometry as water but read completely differently, so the
    // player sees at a glance what a patch will do to them in this zone.
    match terrain {
        TerrainKind::Rock => return draw_rock_patches(ctx, canvas, pools, unit_circle, beat),
        TerrainKind::Kelp => {
            return draw_kelp_patches(ctx, canvas, pools, unit_circle, time, beat, player_center);
        }
        _ => {}
    }

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

/// Draw the King Crab's enrage-phase floor fissures — cracked, glowing hazard circles that the
/// boss splits the arena into when it enrages, forcing the player to weave the conga tail between
/// them. Each entry is (center, radius, age): `age` counts up from 0 while the crack is still
/// opening (a quick tearing flash) and settles at 1 once it's a steady hazard. Rendered hot
/// orange-red so it reads as danger, not water, with a jagged inner glow that pulses on the beat.
pub fn draw_boss_fissures(
    ctx: &mut Context,
    canvas: &mut Canvas,
    fissures: &[(Vec2, f32, f32)],
    time: f32,
    beat_intensity: f32,
    erupt: f32,
) -> ggez::GameResult {
    if fissures.is_empty() {
        return Ok(());
    }
    // `erupt` (0..1) is the beat-synced geyser pulse: at its peak the pits spout molten and their
    // danger reach swells (see damage_tail_in_fissures). Drive the extra flare/spout off it so the
    // visual matches the widened bite exactly — what looks dangerous *is* dangerous.
    let erupt = erupt.clamp(0.0, 1.0);
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
    let beat = beat_intensity.clamp(0.0, 1.0);

    for (i, &(center, radius, age)) in fissures.iter().enumerate() {
        // `open` eases the crack from a bright hot slit to a settled hazard as it forms.
        let open = age.clamp(0.0, 1.0);
        let phase = i as f32 * 1.9;
        let glow = 0.5 + 0.5 * (time * 4.0 + phase).sin();

        // Dark scorched pit so the ground reads as broken, under an additive molten glow.
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(center)
                .scale(Vec2::splat(radius * open))
                .color(Color::new(0.12, 0.03, 0.02, 0.5)),
        );

        let orig_blend = canvas.blend_mode();
        canvas.set_blend_mode(BlendMode::ADD);

        // Molten inner core, hotter on the beat — the "lava" welling up through the crack. On the
        // geyser pulse the core flares brighter and larger, as if the lava surges up the shaft.
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(center)
                .scale(Vec2::splat(radius * (0.55 + 0.22 * erupt) * open))
                .color(Color::new(
                    1.0,
                    0.35 + 0.2 * glow + 0.15 * beat + 0.25 * erupt,
                    0.08 + 0.15 * erupt,
                    (0.28 + 0.18 * glow + 0.3 * erupt) * open,
                )),
        );

        // Geyser column: on the beat, a bright molten spout jets straight up out of the pit mouth,
        // fading as it rises. Only draws while erupting, so between beats the pit just glows.
        if erupt > 0.02 && open > 0.5 {
            let col_h = radius * (0.9 + 1.4 * erupt);
            let col_w = radius * 0.5;
            canvas.draw(
                unit_line,
                DrawParam::default()
                    .dest(center + Vec2::new(0.0, -col_h * 0.5))
                    .rotation(-std::f32::consts::FRAC_PI_2)
                    .scale(Vec2::new(col_h, col_w))
                    .color(Color::new(1.0, 0.55 + 0.35 * glow, 0.2, 0.35 * erupt)),
            );
            // A hot bright cap where the spout crests, brightest at the peak of the pulse.
            canvas.draw(
                unit_circle,
                DrawParam::default()
                    .dest(center + Vec2::new(0.0, -col_h))
                    .scale(Vec2::splat(radius * 0.28 * erupt))
                    .color(Color::new(1.0, 0.85, 0.5, 0.55 * erupt)),
            );
        }

        // Hard hazard rim so the edge you route around reads clearly, flaring on formation. During
        // the geyser it swells outward to trace the widened bite radius (1.35x at peak) so the
        // "danger zone" the player must clear reads visibly bigger on the beat than off it.
        let reach = 1.0 + 0.35 * erupt;
        let rim_a = (0.4 + 0.35 * glow + 0.3 * erupt) * open + (1.0 - open) * 0.9;
        let rim = cached_stroke_circle(ctx, (radius * reach) * open.max(0.05), 3.0 + 1.5 * erupt)?;
        canvas.draw(
            &rim,
            DrawParam::default()
                .dest(center)
                .color(Color::new(1.0, 0.5 + 0.3 * beat, 0.12, rim_a.clamp(0.0, 1.0))),
        );

        // Jagged radial cracks spidering out from the pit, flickering with the molten glow.
        // Drawn from the cached `unit_line` mesh (rotated/scaled per spoke via DrawParam)
        // instead of a fresh `Mesh::new_line` GPU buffer per spoke — with 5 fissures x 7
        // spokes this was up to 35 brand-new GPU mesh allocations every single frame while
        // a King Crab's enrage phase was open.
        let spokes = 7;
        let thickness = 2.0 + 1.5 * beat;
        for s in 0..spokes {
            let a = s as f32 * std::f32::consts::TAU / spokes as f32 + phase * 0.3;
            let jitter = (time * 3.0 + s as f32 * 2.1).sin() * 0.15;
            let dir = Vec2::new((a + jitter).cos(), (a + jitter).sin());
            let inner = center + dir * radius * 0.35 * open;
            let outer_len = (radius * (0.9 + 0.15 * glow) * open - radius * 0.35 * open).max(0.0);
            canvas.draw(
                unit_line,
                DrawParam::default()
                    .dest(inner)
                    .rotation(dir.y.atan2(dir.x))
                    .scale(Vec2::new(outer_len, thickness))
                    .color(Color::new(
                        1.0,
                        0.55 + 0.25 * glow,
                        0.15,
                        (0.45 + 0.3 * glow) * open,
                    )),
            );
        }

        canvas.set_blend_mode(orig_blend);
    }
    Ok(())
}

/// Rocky Shore terrain: solid stone the player must route around. Rendered as a chunky grey
/// boulder with a lighter top face and a hard rim, so it reads as an obstacle you *can't* enter
/// (unlike the translucent water/kelp patches you can wade into). Reuses the shared pool geometry.
fn draw_rock_patches(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pools: &[(Vec2, f32)],
    unit_circle: &Mesh,
    beat: f32,
) -> ggez::GameResult {
    for (i, (center, radius)) in pools.iter().enumerate() {
        let center = *center;
        let radius = *radius;
        let phase = i as f32 * 2.3;
        // Dark base shadow, offset down a touch to sit the rock on the ground.
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(center + Vec2::new(0.0, radius * 0.12))
                .scale(Vec2::splat(radius))
                .color(Color::new(0.10, 0.11, 0.13, 0.55)),
        );
        // Main stone body — opaque so it reads as impassable.
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(center)
                .scale(Vec2::splat(radius * 0.96))
                .color(Color::new(0.34, 0.36, 0.40, 0.95)),
        );
        // Lighter top face, offset up, for a lit-from-above boulder read.
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(center - Vec2::new(radius * 0.12, radius * 0.16))
                .scale(Vec2::splat(radius * 0.6))
                .color(Color::new(0.52, 0.54, 0.58, 0.9)),
        );
        // Hard rim so the collision edge you route around is unmistakable.
        let rim = cached_stroke_circle(ctx, radius * 0.96, 3.0)?;
        canvas.draw(
            &rim,
            DrawParam::default()
                .dest(center)
                .color(Color::new(0.18, 0.19, 0.22, 0.9)),
        );
        // A faint beat-lit sparkle of mineral flecks on top so rocks aren't dead on the beat.
        if beat > 0.05 {
            let ang = phase;
            let fleck = center + Vec2::new(ang.cos(), ang.sin() * 0.5) * radius * 0.35;
            let orig = canvas.blend_mode();
            canvas.set_blend_mode(BlendMode::ADD);
            canvas.draw(
                unit_circle,
                DrawParam::default()
                    .dest(fleck)
                    .scale(Vec2::splat(4.0 + 3.0 * beat))
                    .color(Color::new(0.7, 0.72, 0.8, 0.25 * beat)),
            );
            canvas.set_blend_mode(orig);
        }
    }
    Ok(())
}

/// Neon Kelp Forest terrain: clinging weed patches that snag the conga tail. Rendered as a
/// translucent green bed with swaying frond strokes and a pulsing neon rim, so it reads as
/// something you *can* enter but shouldn't drag a long train through. Reuses the shared geometry.
fn draw_kelp_patches(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pools: &[(Vec2, f32)],
    unit_circle: &Mesh,
    time: f32,
    beat: f32,
    player_center: Vec2,
) -> ggez::GameResult {
    for (i, (center, radius)) in pools.iter().enumerate() {
        let center = *center;
        let radius = *radius;
        let phase = i as f32 * 1.9;
        let breathe = 0.5 + 0.5 * (time * 1.1 + phase).sin();
        let inside = player_center.distance(center) < radius;

        // Dark weed bed — normal blend, a shade of murky green.
        let fill_a = 0.28 + 0.05 * breathe + if inside { 0.12 } else { 0.0 };
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(center)
                .scale(Vec2::splat(radius))
                .color(Color::new(0.10, 0.30, 0.16, fill_a)),
        );

        let orig = canvas.blend_mode();
        canvas.set_blend_mode(BlendMode::ADD);

        // Swaying frond strokes radiating from the center, drifting with time so the bed feels alive.
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
        let fronds = 7;
        for f in 0..fronds {
            let base_ang = f as f32 / fronds as f32 * std::f32::consts::TAU + phase;
            let sway = (time * 1.6 + f as f32 * 0.9).sin() * 0.25;
            let ang = base_ang + sway;
            let len = radius * (0.55 + 0.25 * breathe);
            let start = center + Vec2::new(base_ang.cos(), base_ang.sin() * 0.6) * radius * 0.15;
            let end = center + Vec2::new(ang.cos(), ang.sin() * 0.6) * len;
            let dir = end - start;
            let dist = dir.length().max(0.001);
            let rot = dir.y.atan2(dir.x);
            canvas.draw(
                unit_line,
                DrawParam::default()
                    .dest(start)
                    .rotation(rot)
                    .scale(Vec2::new(dist, 2.5))
                    .color(Color::new(0.35, 1.0, 0.55, 0.30 + 0.2 * beat)),
            );
        }

        // Pulsing neon rim so the snag-risk edge is legible and on-theme with the disco zone.
        let rim = cached_stroke_circle(ctx, radius, 2.5)?;
        canvas.draw(
            &rim,
            DrawParam::default().dest(center).color(Color::new(
                0.4,
                1.0,
                0.6,
                (0.22 + 0.2 * breathe + if inside { 0.28 } else { 0.0 }).clamp(0.0, 1.0),
            )),
        );

        canvas.set_blend_mode(orig);
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

    // Additively blended — the caller (draw_crabs_with_shake) already has the canvas in ADD
    // mode for this whole per-crab aura pass, so this doesn't toggle blend mode itself; see the
    // comment there for why (per-crab toggling used to cause a GPU pipeline switch per crab).

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

    Ok(())
}

/// Draw the magnetic field aura around a free Magnet crab — rings that sweep *inward* toward the
/// crab, reading as a pull that gathers the herd. `size` is the crab's on-screen size; `pull_radius`
/// is how far the crab's tug reaches (matches MAGNET_RADIUS in main.rs) so the aura shows the player
/// exactly how big the catchment is. `time` is total elapsed seconds.
pub fn draw_magnet_aura(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    size: f32,
    pull_radius: f32,
    time: f32,
    lured: bool,
    charged: bool,
) -> ggez::GameResult {
    // Additively blended — see draw_attracted_crab_glow's comment: the caller already has the
    // canvas in ADD mode for this whole per-crab aura pass, so no toggle here.

    // Lodestone red-orange, matching the crab's own color — but while a Golden's shine has lured
    // this Magnet off its cluster, the aura brightens gold-ward so the "chasing the prize"
    // crossover reads at a glance (the mirror tint of the Thief's snared aura going orange). When
    // it's *charged* — pinning a snared Golden and supercharged into a herd-vacuum — the aura goes
    // full gold and its rings reach out over the widened pull radius so the bigger suck reads.
    let (r, g, b) = if charged {
        (1.0, 0.85, 0.4)
    } else if lured {
        (1.0, 0.78, 0.3)
    } else {
        (1.0, 0.4, 0.2)
    };
    let inner = size * 0.7;
    // Match the 1.4x wider field a charged Magnet actually pulls over (CHARGED_MAGNET_RADIUS in
    // main.rs) so the visual boundary tells the truth about the vacuum's reach.
    let ring_radius = if charged { pull_radius * 1.4 } else { pull_radius };
    // A charged Magnet's rings sweep faster and read brighter to sell the energized state.
    let sweep_speed = if charged { 1.1 } else { 0.6 };
    let alpha_scale = if charged { 0.5 } else { 0.35 };

    // Three rings sweeping inward on a shared phase, staggered a third of a cycle apart, so the
    // aura reads as a steady inward pull rather than a single blip. Brightest as they close in.
    for k in 0..3 {
        let phase = ((time * sweep_speed + k as f32 / 3.0) % 1.0) as f32; // 0..1, 0 = far, 1 = at crab
        let radius = ring_radius - (ring_radius - inner) * phase;
        let alpha = (phase * alpha_scale).clamp(0.0, alpha_scale);
        let ring = cached_stroke_circle(ctx, radius, 2.0)?;
        canvas.draw(
            &ring,
            DrawParam::default()
                .dest(pos)
                .color(Color::new(r, g, b, alpha)),
        );
    }

    // A tight, always-bright core ring so the crab itself reads as "the magnet" at a glance.
    let core_pulse = (time * 4.0).sin() * 0.5 + 0.5;
    let core = cached_stroke_circle(ctx, inner + 4.0 + core_pulse * 4.0, 2.5)?;
    let core_g = if charged || lured { 0.8 } else { 0.55 } + core_pulse * 0.2;
    let core_b = if charged { 0.4 } else if lured { 0.35 } else { 0.3 };
    canvas.draw(
        &core,
        DrawParam::default()
            .dest(pos)
            .color(Color::new(1.0, core_g, core_b, 0.55)),
    );

    Ok(())
}

/// Thief crab marker: a sly poison-green ring so a Thief stands out from the herd as "trouble
/// heading for your tail", plus a sharper jittering gnaw-ring when it's latched and actively
/// peeling links (`latched` = true). The latched state pulses fast and bright so the theft in
/// progress reads at a glance and the player knows to whistle/stomp it off.
pub fn draw_thief_aura(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    size: f32,
    latched: bool,
    snared: bool,
    lured: bool,
    time: f32,
) -> ggez::GameResult {
    // Additively blended — see draw_attracted_crab_glow's comment: the caller already has the
    // canvas in ADD mode for this whole per-crab aura pass, so no toggle here.

    // Poison-green, matching the crab's own color — but while a Magnet has intercepted it, the
    // green bleeds toward the lodestone's orange so the "caught in the field" crossover reads;
    // while a fleeing Golden has lured it off your tail, the green catches a golden gleam instead,
    // so the "the shine drew the raider away" crossover reads distinct from the Magnet interception.
    let (r, g, b) = if snared {
        (0.95, 0.6, 0.2)
    } else if lured {
        (0.85, 0.95, 0.35) // poison-green warmed by the golden prize it's chasing
    } else {
        (0.35, 0.95, 0.5)
    };

    if latched {
        // Actively gnawing: a fast, bright, slightly jittering double ring so the theft screams
        // for attention. The jitter fakes the crab tearing at the link.
        let pulse = (time * 18.0).sin() * 0.5 + 0.5;
        let jitter = (time * 40.0).sin() * 2.5;
        let ring = cached_stroke_circle(ctx, size * 0.9 + 3.0 + jitter, 3.0)?;
        canvas.draw(
            &ring,
            DrawParam::default()
                .dest(pos)
                .color(Color::new(r, g, b, 0.5 + pulse * 0.4)),
        );
        let ring2 = cached_stroke_circle(ctx, size * 1.25 + pulse * 6.0, 2.0)?;
        canvas.draw(
            &ring2,
            DrawParam::default()
                .dest(pos)
                .color(Color::new(0.6, 1.0, 0.5, 0.25 + pulse * 0.25)),
        );
    } else if snared {
        // Intercepted by a Magnet: a brighter, faster orange ring that reads as "the field's got
        // it" — livelier than the calm prowl so the save is legible, calmer than the theft frenzy.
        let pulse = (time * 9.0).sin() * 0.5 + 0.5;
        let ring = cached_stroke_circle(ctx, size * 0.9 + 3.0 + pulse * 4.0, 2.5)?;
        canvas.draw(
            &ring,
            DrawParam::default()
                .dest(pos)
                .color(Color::new(r, g, b, 0.45 + pulse * 0.3)),
        );
    } else if lured {
        // Lured off your tail by a Golden's shine: a brisk, brighter golden-green ring — livelier
        // than the calm prowl so the divert reads as the raider actively chasing the prize.
        let pulse = (time * 7.0).sin() * 0.5 + 0.5;
        let ring = cached_stroke_circle(ctx, size * 0.9 + 3.0 + pulse * 4.0, 2.5)?;
        canvas.draw(
            &ring,
            DrawParam::default()
                .dest(pos)
                .color(Color::new(r, g, b, 0.4 + pulse * 0.3)),
        );
    } else {
        // Prowling: a steady soft ring that just marks it out, calmer than the latched frenzy.
        let pulse = (time * 3.0).sin() * 0.5 + 0.5;
        let ring = cached_stroke_circle(ctx, size * 0.85 + 3.0 + pulse * 3.0, 2.0)?;
        canvas.draw(
            &ring,
            DrawParam::default()
                .dest(pos)
                .color(Color::new(r, g, b, 0.35 + pulse * 0.2)),
        );
    }

    Ok(())
}

/// Golden crab shine — a soft shimmering halo plus a handful of sparkle dots orbiting the crab, so
/// the rare high-value prize catches the eye across the whole field and reads as "chase this one!".
/// Additively blended for a glowy treasure look — the caller (draw_crabs_with_shake) already has
/// the canvas in ADD mode for this whole per-crab aura pass, so this doesn't toggle blend mode
/// itself. Reuses the cached unit-circle and stroke-circle meshes (scaled/positioned per element
/// via DrawParam) so no fresh GPU buffers are allocated.
pub fn draw_golden_sparkle(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    size: f32,
    time: f32,
    snared: bool,
) -> ggez::GameResult {
    // Soft breathing halo so the prize glows even when it's holding still. When a Magnet's field
    // has snared it, the halo warms toward the lodestone's orange so the "trapped by the Magnet"
    // state reads instantly against the ordinary gold shine.
    let pulse = (time * 4.0).sin() * 0.5 + 0.5;
    let (hg, hb) = if snared { (0.6, 0.15) } else { (0.85, 0.3) };
    let halo = cached_stroke_circle(ctx, size * 0.8 + 3.0 + pulse * 4.0, 2.5)?;
    canvas.draw(
        &halo,
        DrawParam::default()
            .dest(pos)
            .color(Color::new(1.0, hg, hb, 0.35 + pulse * 0.3)),
    );

    // While snared, a fast-spinning tether ring cinches in tight around the crab — the visual of
    // the field clamping the prize in place, drawing the eye to "grab it NOW".
    if snared {
        let cinch = 0.5 + 0.5 * (time * 12.0).sin();
        let tether = cached_stroke_circle(ctx, size * 0.55 + 2.0 + cinch * 3.0, 3.0)?;
        canvas.draw(
            &tether,
            DrawParam::default()
                .dest(pos)
                .color(Color::new(1.0, 0.6, 0.15, 0.55 + cinch * 0.35)),
        );
    }

    // A ring of sparkle dots orbiting the crab, each twinkling on its own phase so the whole thing
    // shimmers like a coin catching the light. Snared, the orbit pulls in tighter and spins faster,
    // like filings dragged onto the lodestone.
    let dot = unit_circle(ctx)?;
    const SPARKLES: usize = 5;
    let orbit = if snared { size * 0.55 + 4.0 } else { size * 0.75 + 6.0 };
    let spin = if snared { 3.4 } else { 1.6 };
    for i in 0..SPARKLES {
        let base = i as f32 / SPARKLES as f32 * std::f32::consts::TAU;
        let ang = base + time * spin;
        let twinkle = ((time * 6.0 + i as f32 * 1.7).sin() * 0.5 + 0.5).powf(2.0);
        let dpos = pos + Vec2::new(ang.cos(), ang.sin()) * orbit;
        let r = 1.5 + twinkle * 2.5;
        let (sg, sb) = if snared { (0.75, 0.35) } else { (0.95, 0.55) };
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(dpos)
                .scale(Vec2::splat(r))
                .color(Color::new(1.0, sg, sb, 0.4 + twinkle * 0.6)),
        );
    }

    Ok(())
}
