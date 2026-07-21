pub use crate::floating_text::{
    FloatingTextSystem, PennedMarcherSystem, draw_floating_texts, draw_penned_marchers,
};
use crate::enemies::{BossCharge, CrabType, EnemyCrab};
use crate::skins::{Accessory, FacialHair, Hat, PlayerSkin};
use crate::{CRAB_SIZE, Flashlight, PLAYER_SIZE};
use crevice::std140::AsStd140;
use ggez::Context;
use ggez::glam::Vec2;
use ggez::graphics::{
    BlendMode, Canvas, Color, DrawMode, DrawParam, Image, InstanceArray, Mesh, Rect, Shader,
    ShaderParams, ShaderParamsBuilder, Text,
};
use rand::Rng;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Empty-batch guard for ggez's `Canvas::draw_instanced_mesh`.
///
/// ggez panics with `assertion failed: capacity > 0` (instance.rs:77) if you draw an
/// `InstanceArray` whose param list is empty: `flush_wgpu` rebuilds the GPU buffer at
/// `len` on every draw, and `new_wgpu(.., 0, ..)` asserts `capacity > 0`. Our batched
/// draws routinely `.set()` a filtered iterator that can come out empty (a LOD tier with
/// no crabs, a terrain pool the biome lacks, trails all fully retracted), so any such
/// draw is a latent crash — one that headless playtests can't catch because the bot
/// `draw()` returns before the scene render.
///
/// This trait centralizes the fix: call `draw_instanced_mesh_guarded` everywhere instead
/// of ggez's method and an empty batch simply renders nothing (which is what it would have
/// drawn anyway) rather than panicking. Immunizes every call site at once.
pub(crate) trait InstancedMeshExt {
    fn draw_instanced_mesh_guarded(
        &mut self,
        mesh: Mesh,
        instances: &InstanceArray,
        param: impl Into<DrawParam>,
    );
}

impl InstancedMeshExt for Canvas {
    #[inline]
    fn draw_instanced_mesh_guarded(
        &mut self,
        mesh: Mesh,
        instances: &InstanceArray,
        param: impl Into<DrawParam>,
    ) {
        if instances.instances().is_empty() {
            return;
        }
        self.draw_instanced_mesh(mesh, instances, param);
    }
}

// Terrain / biome ground-layer rendering (tide pools, boss fissures, rock & kelp patches)
// lives in its own file. Re-exported so every `graphics::draw_*` call-site path is unchanged.
mod terrain;
pub use terrain::*;

// Environment / weather backdrop rendering (three-zone ground, grass, ambient motes, sky
// overlay, world-edge fade, rain & puddle ripples) lives in its own file. Re-exported so
// every `graphics::draw_*` call-site path is unchanged.
mod weather;
pub use weather::*;

// Per-archetype crab visual identity (proportions, legs, claws, eyes, shell pattern, accent).
// Pure data consumed by draw_crab so every archetype reads at a glance from its silhouette.
mod crab_style;
use crab_style::ShellPattern;

// Delivery-pen rendering (bank beam, the pen and its penned-marcher slots, and the
// haul-worth / at-risk / kelp-snag / streak / pen-guide HUD tags) lives in its own file.
// Re-exported so every `graphics::draw_*` call-site path is unchanged.
mod delivery;
pub use delivery::*;

// Transient beat-feedback rings/pulses (chain ghost rings, catch shockwaves & trails,
// fear/tide pulses, whistle/bloom rings, call & downbeat pulses, stomp/slam impacts)
// live in their own file. Re-exported so every `graphics::draw_*` call-site path is unchanged.
mod rings;
pub use rings::*;

// Tool-vs-enemy "signature match" reaction effects (beam/whistle/lasso/stomp/magnet flourishes
// keyed to a specific crab archetype: shell-drain, pins, deflects, cluster pulls) live in their
// own file. Re-exported so every `graphics::draw_*` call-site path is unchanged.
mod tool_matches;
pub use tool_matches::*;

// The particle system (Particle/ParticleSystem types, ParticleUniform, spawn/update logic and
// the batched draw_particles pass) lives in its own file. Re-exported so every
// `graphics::ParticleSystem` / `graphics::draw_particles` path is unchanged.
mod particles;
pub use particles::*;

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

// Per-raindrop constants (rx, ry, speed, len) precomputed once — see rain_consts(). Only the
// time-varying vertical position is computed per frame; everything else is baked.
const RAIN_DROP_COUNT: usize = 220;
static RAIN_CONSTS: OnceLock<[(f32, f32, f32, f32); RAIN_DROP_COUNT]> = OnceLock::new();
// Per-puddle-ripple constants (rx, ry, phase, period) precomputed once — see puddle_consts().
const PUDDLE_RIPPLE_COUNT: usize = 26;
static PUDDLE_CONSTS: OnceLock<[(f32, f32, f32, f32); PUDDLE_RIPPLE_COUNT]> = OnceLock::new();

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

    // Cache of rounded-rectangle meshes (fill + stroke variants) keyed by (x, y, w, h, radius)
    // quantized plus mode-specific data (RGBA color for fill; RGBA color + thickness for stroke).
    // draw_tool_roster rebuilt two fresh Mesh::new_rounded_rectangle GPU buffers per slot — 10 a
    // frame for the 5-slot HUD bar — every single frame of gameplay, even though each slot's
    // rect only ever sits at one of two fixed sizes/positions (they only move on window resize)
    // and its border cycles between just two colors (ready vs. on-cooldown). Same pattern as
    // FILL_RECT_CACHE/STROKE_RECT_CACHE above, just with the extra rounding radius baked into
    // the key since ggez has no scale-invariant way to redraw a rounded rect via DrawParam alone
    // (scaling would distort the corner radius same as it does stroke thickness on rings).
    static ROUNDED_FILL_RECT_CACHE: RefCell<HashMap<(i32, i32, i32, i32, i32, u32), Mesh>> =
        RefCell::new(HashMap::new());
    static ROUNDED_STROKE_RECT_CACHE: RefCell<HashMap<(i32, i32, i32, i32, i32, i32, u32), Mesh>> =
        RefCell::new(HashMap::new());

    // Scratch buffer for `draw_conga_rope`'s per-micro-segment geometry (position, rotation,
    // length, rgb), persisted and `clear()`-ed each frame instead of a fresh `Vec` allocation.
    // The rope used to draw its main segment then immediately flip to additive blend for the
    // glow segment and flip back, every single micro-segment (SEGS=14 per link) — on a long
    // conga train that's hundreds of blend-mode switches a frame, each one breaking ggez's
    // draw-call batching. Buffering the geometry lets both passes run back-to-back with only
    // two blend-mode switches total, no matter how long the chain gets.
    // Tuple: (position, rotation, length, rgb, thickness_mult). The trailing thickness_mult is
    // 1.0 for an ordinary micro-segment and bulges >1 where the rope heats up under a rival's
    // splice threat, so the endangered band visibly swells as well as reddens.
    static CONGA_SEGMENT_BUF: RefCell<Vec<(Vec2, f32, f32, [f32; 3], f32)>> = RefCell::new(Vec::new());

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

    // How many crabs the caller is about to draw this pass — set once per drawing pass by
    // set_crab_lod_hint() so draw_crab can pick a level of detail. A calm field renders fully
    // articulated hero crabs; a big swarm/train drops each crab to a cheaper form so the two
    // instanced batches (bodies + legs) stay small and the [perf] frame time doesn't regress on
    // long trains. Combined with a per-crab on-screen-size test so tiny/distant crabs are always
    // cheap regardless of count. Defaults to 0 (→ full detail) for the menu/decorative crabs.
    static CRAB_LOD_COUNT: Cell<usize> = const { Cell::new(0) };

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

    // Scratch buffer for pre-computed trail geometry in draw_catch_trails. The three instanced
    // passes (glow, core, spark) each used to call trail_geometry() independently — that's a
    // Vec2 subtraction, a length() (sqrt), an atan2(), and a few float muls per trail per pass,
    // so three times total. Since the geometry is identical across all three passes we compute it
    // once into this buffer and let the passes index it. At ≤56 active trails (the cap) this is
    // at most 56 avoidable sqrt+atan2 pairs per draw_catch_trails call, called twice per frame
    // (catch_trails + call_streaks) — ~224 saved sqrt/atan2 calls per frame during Groove Call.
    static TRAIL_GEOM_BUF: RefCell<Vec<Option<(Vec2, f32, f32, f32)>>> = RefCell::new(Vec::new());

    // draw_grass tiled its texture across the whole window with one canvas.draw() per tile — at
    // the default 800x600 window and a 4x4 grass tile, that's 200x150 = 30,000 individual GPU
    // submissions every single frame just for the ground, dwarfing every other draw-call cost in
    // the game combined. Same batching technique as the instances above: fill one InstanceArray
    // with a DrawParam per tile position and issue a single draw_instanced_mesh. Same texture,
    // same positions, identical on-screen output.
    static GRASS_TILE_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    // Last (tiles_x, tiles_y, texture_w, texture_h) used to fill GRASS_TILE_INSTANCES. The tile
    // grid is purely a function of window size and texture size (both constant between resizes and
    // level changes), so we skip the `instances.set()` upload when none of these change — the GPU
    // buffer already holds the right data from the previous frame. Resizes and level transitions
    // are rare, so this turns a per-frame O(tiles_x*tiles_y) iterator-to-GPU upload into a
    // near-zero-cost early-out on every normal gameplay frame.
    static GRASS_TILE_LAST_KEY: RefCell<(i32, i32, u32, u32)> = RefCell::new((0, 0, 0, 0));

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

    // Reusable draw-param buffers for draw_catch_shockwaves and draw_fear_rings, mirroring the
    // chain-ring grouping approach: shockwaves/fear-rings emitted in the same frame (burst-spawned
    // by a Downbeat Slam, beat wave, or chain reaction) share the same age and thus the same
    // (radius, thickness) bucket, so grouping them by key collapses the burst into a handful of
    // instanced draws instead of one canvas.draw() per shockwave per pass.  In normal play (staggered
    // individual catches) each group holds one element, so the per-frame overhead is a clear() +
    // small groups.iter() instead of the raw canvas.draw() loop — comparable cost, zero regression.
    static SHOCKWAVE_GROUPS: RefCell<HashMap<(i32, i32), Vec<DrawParam>>> = RefCell::new(HashMap::new());
    static SHOCKWAVE_INSTANCES: RefCell<HashMap<(i32, i32), InstanceArray>> = RefCell::new(HashMap::new());
    // Reusable InstanceArray for the white-hot flash fills in draw_catch_shockwaves (the filled
    // unit-circle burst in the first 0.22 of a shockwave's life). All flashing shockwaves share
    // the same unit-circle mesh and only differ in scale/alpha, so a single InstanceArray handles
    // them all in one GPU submission.
    static FLASH_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    static FEAR_RING_GROUPS: RefCell<HashMap<(i32, i32), Vec<DrawParam>>> = RefCell::new(HashMap::new());
    static FEAR_RING_INSTANCES: RefCell<HashMap<(i32, i32), InstanceArray>> = RefCell::new(HashMap::new());

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

    // Reusable instance buffer for draw_minimap's dots (crabs, NPC followers/leaders, pen, player).
    // `crabs` holds every crab caught this run (never removed, only flagged `caught`), so the old
    // per-crab canvas.draw() loop issued one GPU submission per crab per frame with unbounded
    // growth over a run's lifetime — the one entity-draw loop in this file that hadn't been
    // batched yet. All these dots share the same unit-circle mesh and only differ in
    // dest/scale/color, so one InstanceArray fill + draw_instanced_mesh handles them all, same
    // draw order (and thus same overlap blending) as the original sequential calls.
    static MINIMAP_DOT_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static MINIMAP_DOT_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

    // Magnet aura batching: collect per-ring (mesh_key, DrawParam) pairs from draw_magnet_aura()
    // calls during the per-crab aura pass, then flush them all as instanced batches grouped by
    // mesh key in flush_magnet_auras(). In the Water biome (which now bias-spawns Magnets at
    // ~33%) it's common to have 4-6 Magnets on screen at once; each drew 4 individual ADD-blend
    // canvas.draw() calls for its sweep rings + core, totalling 16-24 GPU submissions per frame
    // just for Magnet auras. Rings at the same sweep-phase quantized bucket share the same mesh
    // key, so rings from several Magnets on screen at once typically collapse to 1-3 batched
    // submissions instead of N×4. Same draw order (all ADD-blended aura pass), same pixels.
    static MAGNET_AURA_RING_PARAMS: RefCell<Vec<((i32, i32), DrawParam)>> = RefCell::new(Vec::new());
    // Per-key instance arrays and group-param vecs, same pattern as FEAR_RING_INSTANCES/CHAIN_RING_INSTANCES.
    static MAGNET_AURA_GROUPS: RefCell<HashMap<(i32, i32), Vec<DrawParam>>> = RefCell::new(HashMap::new());
    static MAGNET_AURA_INSTANCES: RefCell<HashMap<(i32, i32), InstanceArray>> = RefCell::new(HashMap::new());

    // Reusable instance buffers for draw_combo_meter's two arc passes (main arc + soft glow).
    // The combo meter draws up to 32 arc segments per pass, each previously costing its own
    // canvas.draw() call — up to 64 GPU submissions a frame while any x2/x3/x5 combo is live,
    // which is most of active play once a run gets going. Filling one InstanceArray per pass and
    // issuing a single draw_instanced_mesh collapses that to 2 draw calls total regardless of
    // how full the arc is, identical on-screen output (same unit_line mesh, same positions/
    // rotations/scales/colors). The segment DrawParams are rebuilt fresh each frame (arc fill
    // fraction and rotation offset change continuously), so a scratch Vec is cleared-and-filled
    // rather than accumulated across frames.
    static COMBO_ARC_MAIN_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static COMBO_ARC_GLOW_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static COMBO_ARC_MAIN_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    static COMBO_ARC_GLOW_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

    // Cache for the three combo-meter multiplier labels ("x2", "x3", "x5"). draw_combo_meter
    // called Text::new(multiplier_label) every single frame the meter was visible — a glyph-
    // shaping pass ~60 times/sec during any hot run, the same per-frame Text::new pattern the
    // other HUD caches (FRENZY_BANNER_CACHE, GROOVE_LABEL_CACHE, etc.) already fixed. The label
    // is one of exactly three fixed strings keyed by combo tier (0=x2, 1=x3, 2=x5), so build each
    // once on first use and reuse forever. DrawParam::scale still handles the per-frame beat-pulse
    // size, so no re-layout is needed when the pulse changes.
    static COMBO_LABEL_CACHE: RefCell<[Option<Text>; 3]> = RefCell::new([const { None }; 3]);

    // Cache for draw_tool_roster's 15 labels (key/name/hint x 5 slots). Every one of those was a
    // fresh Text::new() + set_scale() call every single frame the roster was visible — i.e. all of
    // active gameplay, the same per-frame glyph-shaping cost COMBO_LABEL_CACHE above already fixed
    // for the combo meter. 14 of the 15 strings are truly static per slot; only the GROOVE slot's
    // hint toggles between "SLAM ready!" and "need groove" as the meter fills, so each cache entry
    // stores the source &'static str alongside its shaped Text and rebuilds only on a content
    // mismatch — a rare event for 14 of 15 slots, and just a two-way flip for the 15th.
    static TOOL_ROSTER_TEXT_CACHE: RefCell<[Option<(&'static str, Text)>; 15]> =
        RefCell::new([const { None }; 15]);

    // Reusable instance buffers for draw_boss_fissures' batched passes. While a King Crab's
    // enrage phase is open (up to 5 fissures with 7 radial crack-spokes each) the old per-spoke,
    // per-cap, per-pit canvas.draw() loop issued up to ~65 individual GPU submissions a frame plus
    // one set_blend_mode toggle per fissure (5 extra pipeline switches). The enrage climax is the
    // most visually dense moment of a run (screen shake + particles + chain rings + boss aura all
    // firing together), so extra draw-call cost there hurts the most. Collapsed into five
    // InstanceArray submissions total with a single blend-mode switch pair for the whole pass.
    // Same unit_circle / unit_line meshes, identical on-screen output — pure batch reduction.
    static FISSURE_PIT_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static FISSURE_CORE_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static FISSURE_SPOKE_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static FISSURE_GEYSER_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static FISSURE_CAP_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static FISSURE_CIRCLE_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    static FISSURE_LINE_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

    // Cached ShaderParams for the two shaders that ran ShaderParamsBuilder::new(...).build(ctx)
    // every single gameplay frame — each build() call allocates a GPU buffer (device.create_buffer
    // inside GrowingBufferArena::new) and builds a fresh bind group, even though only the uniform
    // DATA changes (time, beat, player position) while the buffer layout never does. Caching the
    // ShaderParams object across frames and calling set_uniforms() instead of build() re-uploads
    // the uniform data to the GPU queue each frame as before, but reuses the existing arena and
    // deduplicates the bind group so no fresh GPU buffer is created on the hot path.
    static GRASS_SHADER_PARAMS: RefCell<Option<ShaderParams<ResolutionUniform>>> = RefCell::new(None);
    static FLASHLIGHT_SHADER_PARAMS: RefCell<Option<ShaderParams<FlashlightUniform>>> = RefCell::new(None);

    // Reusable instance buffers for draw_kelp_patches' frond-stroke and fill passes. Each pool
    // used to issue 7 individual canvas.draw(unit_line) calls for the swaying fronds plus one
    // canvas.draw(unit_circle) per fill layer — up to 5 pools means ~35 frond draws + 10 fill
    // draws + 5 rim draws = 50 separate GPU submissions every frame in the Kelp biome. The same
    // batching technique used for legs/bodies/combo-arcs/trails/radar-arrows: collect all DrawParams
    // into a scratch Vec, fill one InstanceArray, and issue a single draw_instanced_mesh. Zero
    // visible difference (same unit_line/unit_circle mesh, same positions/scales/colors/rotations),
    // just far fewer GPU submissions on a biome that already has a lot of other draw-call action
    // (crabs + rope + particles + chain rings all on screen at once in the Kelp zone).
    static KELP_FROND_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static KELP_FROND_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    static KELP_FILL_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static KELP_FILL_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    // Funnel-lane streaks that show where the kelp channels a fleeing crab (crate::KELP_FUNNEL_DIR).
    // Batched into one InstanceArray the same way as the fronds so the routing cue costs one draw.
    static KELP_FUNNEL_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static KELP_FUNNEL_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

    // Reusable instance buffers for draw_rock_patches' fill and sparkle passes. Up to 5 rock
    // patches, each with 3 fill draws (shadow + body + face) and 1 sparkle = up to 20 individual
    // canvas.draw(unit_circle) calls a frame in the Rock biome. Collapsed into one fill batch and
    // one sparkle batch, matching the kelp and fissure batching above.
    static ROCK_FILL_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static ROCK_FILL_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    static ROCK_SPARKLE_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static ROCK_SPARKLE_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    // Dedicated buffer for the Rocky Shore tide water-sheet pass. Must NOT share ROCK_FILL_INSTANCES:
    // the rock-body fill batch is still referenced by the canvas when the water sheet draws in the
    // same frame, and a single InstanceArray is one persistent GPU buffer — a second set() on it
    // would clobber the body params before the frame's draws resolve. Own array, own correctness.
    static ROCK_TIDE_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

    // Reusable instance buffers for draw_tide_pools' fill and additive passes. Water pools
    // previously issued ~10 individual canvas.draw() calls per pool per frame (2 fill discs, 1 rim,
    // 2 ripple rings, 1 glint, 4 current streaks). With 6-10 native pools on the Water biome level
    // that was 60-100 GPU submissions every frame — plus Tide Boss flood pools on top. The fills
    // (base disc + shallow center) and the additive fills (glints + current streaks) each use the
    // same shared unit_circle mesh at different scales, so they're ideal for InstanceArray batching
    // exactly like the Rock/Kelp fills above: collect all DrawParams, fill one InstanceArray, one
    // draw_instanced_mesh.
    //
    // Rims and ripple rings are stroke meshes (not fill), so they can't be scaled from a shared
    // unit mesh — stroke thickness would scale too. Instead they're batched by mesh key, using the
    // same (radius*0.5, thickness) quantisation bucket that cached_stroke_circle() and
    // draw_chain_rings use: pools with the same quantised radius share one InstanceArray submission.
    // In practice 6-10 pools can produce as few as 3-8 distinct rim keys (many share the same
    // pool-radius bucket) and likewise for ripple rings, collapsing up to 30 individual canvas.draw()
    // calls into ~6-16 instanced submissions — the same technique as chain_rings.
    static POOL_FILL_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static POOL_FILL_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    static POOL_ADD_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static POOL_ADD_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    // Rim and ripple-ring batching grouped by stroke_circle_key, mirroring CHAIN_RING_GROUPS.
    static POOL_RIM_GROUPS: RefCell<HashMap<(i32, i32), Vec<DrawParam>>> = RefCell::new(HashMap::new());
    static POOL_RIM_INSTANCES: RefCell<HashMap<(i32, i32), InstanceArray>> = RefCell::new(HashMap::new());
    static POOL_RIPPLE_GROUPS: RefCell<HashMap<(i32, i32), Vec<DrawParam>>> = RefCell::new(HashMap::new());
    static POOL_RIPPLE_INSTANCES: RefCell<HashMap<(i32, i32), InstanceArray>> = RefCell::new(HashMap::new());

    // Reusable instance buffer for draw_speed_lines' 7 wake lines. While the player is dashing
    // all 7 lines share the same unit_line mesh with different dest/rotation/scale/color params —
    // the exact InstanceArray use-case. Collapses 7 individual canvas.draw() calls to 1 GPU
    // submission. The buffer is always exactly 7 entries (one per line), so it never grows.
    static SPEED_LINE_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

    // Reusable instance buffer for draw_groove_vignette's edge-band quads. The vignette draws
    // up to 5 bands × 4 edges = 20 individual canvas.draw(unit_square, ...) calls every frame
    // the groove meter is active (which is most of late-game play). All 20 use the same
    // unit_square mesh with different dest/scale/color DrawParams — ideal for InstanceArray
    // batching: collect all 20 params, fill one InstanceArray, one draw_instanced_mesh.
    static VIGNETTE_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static VIGNETTE_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

    // Weather rain-streak batching: one InstanceArray filled from RAIN_CONSTS each frame and drawn
    // as a single instanced submission (same unit_line mesh), instead of a canvas.draw() per drop.
    static RAIN_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    // Weather puddle-ripple batching: one InstanceArray of expanding unit-circle rings.
    static PUDDLE_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

    // Reusable instance buffer for the orbiting sparkle dots in draw_golden_sparkle. Each Golden
    // crab draws 5 unit-circle dots (or 5 tighter dots when snared) — all using the same
    // UNIT_CIRCLE mesh. With multiple Goldens on screen draw_golden_sparkle was issuing 5 individual
    // canvas.draw() calls per crab per frame. Instead push each sparkle's DrawParam here and flush
    // the whole batch in one draw_instanced_mesh after all crabs' auras are drawn (alongside
    // flush_crab_legs / flush_crab_bodies). Identical on-screen output; just one GPU submission
    // for all sparkles regardless of how many Goldens are in play.
    static GOLDEN_SPARKLE_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static GOLDEN_SPARKLE_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

    // Beat corona glow buffer: each caught crab with beat_phase > 0.3 pushes one DrawParam here
    // (a color-matched soft circle in ADD blend). Flushed once per frame by flush_beat_coronas()
    // in the same ADD-blend pass as the other crab auras — one GPU submission for every corona
    // regardless of conga train length.
    static BEAT_CORONA_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static BEAT_CORONA_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

    // Reusable instance buffer for the coil dots inside draw_hermit_shell. Each shelled Hermit
    // draws up to 5 unit-circle dots (the borrowed-shell whorl) — all using the same UNIT_CIRCLE
    // mesh. With multiple Hermits on screen (they spawn in clusters, ~7% of crabs, and can
    // accumulate before their shells crack) draw_hermit_shell was issuing 5 individual
    // canvas.draw() calls per crab per frame. Instead push each coil dot's DrawParam here and
    // flush them all as one draw_instanced_mesh (flush_hermit_coil_dots) after all auras are drawn
    // — same pattern as GOLDEN_SPARKLE_PARAMS. Identical on-screen output; one GPU submission
    // for all hermit coil dots regardless of how many shelled Hermits are in play.
    static HERMIT_COIL_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static HERMIT_COIL_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

    // Catch-next-hint tick-dot batching: draw_catch_next_hint() draws 4 small orbiting ticks
    // per matching crab (all using cached_stroke_circle(2.2, 1.4) — the same fixed mesh).
    // With 10-15 matching crabs on a full map that's 40-60 individual canvas.draw() calls every
    // frame for dots alone. Instead defer each dot's DrawParam here and flush once per frame in
    // flush_catch_next_ticks(), collapsing all dots to one draw_instanced_mesh call regardless
    // of how many matching crabs are in play. Identical on-screen output; one GPU submission.
    static CATCH_NEXT_TICK_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static CATCH_NEXT_TICK_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

    // Centerpiece bracket-dot batching: draw_centerpiece_ring() draws 10 small orbiting bracket
    // dots per centerpiece link (2 sides × 5 dots each), all using the same fixed
    // cached_stroke_circle(2.2, 1.5) mesh. On a long train with a 5+ same-type run seated across
    // the midpoint (the CENTERPIECE window) that's 10 × run_len individual canvas.draw() calls
    // per frame — up to 50-60+ on a good arrangement run. Instead defer each dot DrawParam here
    // and flush them all as one draw_instanced_mesh call in flush_centerpiece_dots() after the
    // chain-crab loop, identical to the hermit-coil / catch-next-tick batching above.
    static CENTERPIECE_DOT_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static CENTERPIECE_DOT_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

    // Attracted-crab glow batching: draw_attracted_crab_glow() used to issue 2 individual
    // canvas.draw() calls per crab-in-flashlight (outer soft-glow ring + inner bright ring).
    // With 10-30 crabs in the flashlight beam at once that was 20-60 unbatched GPU submissions
    // a frame just for flashlight glow — the same per-crab redundancy the magnet aura rings and
    // hermit coil dots had before batching. Instead defer each ring's DrawParam here (grouped by
    // stroke_circle_key since different crab sizes land in different stroke-mesh buckets) and
    // flush them all grouped into a couple of draw_instanced_mesh calls in
    // flush_attracted_crab_glows() after the per-crab aura pass. Outer and inner rings use
    // different radii and thicknesses but typically cluster into only a few key buckets (most
    // normal crabs share the same scale), so in practice ~10 crabs collapse to 2-4 submissions.
    static ATTRACTED_GLOW_GROUPS: RefCell<HashMap<(i32, i32), Vec<DrawParam>>> = RefCell::new(HashMap::new());
    static ATTRACTED_RING_GROUPS: RefCell<HashMap<(i32, i32), Vec<DrawParam>>> = RefCell::new(HashMap::new());
    static ATTRACTED_GLOW_INSTANCES: RefCell<HashMap<(i32, i32), InstanceArray>> = RefCell::new(HashMap::new());
    static ATTRACTED_RING_INSTANCES: RefCell<HashMap<(i32, i32), InstanceArray>> = RefCell::new(HashMap::new());

    // Archetype-ring batching: draw_thief_aura, draw_splitter_aura's halo/flare, draw_golden_sparkle's
    // halo/tether, and draw_armor_ring's track were still issuing one canvas.draw() per crab per
    // frame for a single stroke-circle ring each — the same per-crab GPU-submission cost the Magnet/
    // Hermit-coil/Golden-sparkle-dot/attracted-glow auras were already fixed for above. They all draw
    // exactly one ring from cached_stroke_circle, so they share this one grouped buffer (keyed by
    // stroke_circle_key, same technique as ATTRACTED_GLOW_GROUPS) via defer_archetype_ring(), flushed
    // together by flush_archetype_rings() after the per-crab aura pass. With several Thief/Splitter/
    // Golden/Armored crabs on screen at once (a late-game herd routinely has all four) this collapses
    // their ring draws from one GPU submission per crab down to one per distinct mesh bucket.
    static ARCHETYPE_RING_GROUPS: RefCell<HashMap<(i32, i32), Vec<DrawParam>>> = RefCell::new(HashMap::new());
    static ARCHETYPE_RING_INSTANCES: RefCell<HashMap<(i32, i32), InstanceArray>> = RefCell::new(HashMap::new());

    // Splitter cleave-dot batching: draw_splitter_aura's two "cleave" dots use the shared UNIT_CIRCLE
    // fill mesh (same technique as GOLDEN_SPARKLE_PARAMS/HERMIT_COIL_PARAMS above), so they get their
    // own small dot buffer rather than the ring groups (which are stroke meshes, not fills).
    static CLEAVE_DOT_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static CLEAVE_DOT_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

    // Cache for the world-map screen's Text objects. draw_world_map rebuilt a fresh Text +
    // measure() for every node label, the title, and the controls hint on every frame the map
    // screen was visible — the same unbounded-idle-time pattern every other menu screen already
    // fixed. Node labels: keyed per-node by (completed, unlocked); those are the only two booleans
    // that change label text (suffix " ✓" and lock " [locked]"). Selection changes fill color only,
    // never the label text, so it's not part of the key. Title and hint are static literals →
    // cached unconditionally. A path-line segment cache is skipped: there are only N-1 ≤ 3 path
    // segments and they're connection-only (two endpoint positions, no text/glyphs), so the per-
    // frame cost of `Mesh::new_line` for three lines is negligible compared to glyph-shaping.
    static WORLD_MAP_NODE_LABELS: RefCell<Vec<Option<((bool, bool), Text, f32)>>> = RefCell::new(Vec::new());
    static WORLD_MAP_TITLE_CACHE: RefCell<Option<(Text, f32)>> = RefCell::new(None);
    static WORLD_MAP_HINT_CACHE: RefCell<Option<(Text, f32)>> = RefCell::new(None);
    static WORLD_MAP_SKIP_CACHE: RefCell<Option<(Text, f32)>> = RefCell::new(None);
    // Per-node biome tint for the world map, built once (the node list is stable for the session).
    // Campaign nodes take their level's `biome.tint`; tutorial nodes get a warm amber on-ramp colour.
    // Cached so we never rebuild the (String-allocating) `get_levels()` list per frame.
    static WORLD_MAP_NODE_TINTS: RefCell<Option<Vec<Color>>> = RefCell::new(None);

    // Player cosmetics mesh cache: pre-built meshes for hat/facial-hair/accessory combos,
    // keyed by (Hat, FacialHair, Accessory). Each entry is a Vec of (Mesh, DrawParam) where
    // the DrawParam's dest is a body-space offset from the crab centre (c = Vec2::ZERO when
    // built). At draw time we translate each param by the actual `c` (centre + beat-hop).
    // draw_player_cosmetics was rebuilding up to ~8 fresh Mesh::new_rectangle/new_polygon/
    // new_circle GPU buffers every frame — constant cost regardless of game state since the
    // player is always drawn. Cached once per session per skin choice: the meshes are
    // dimensioned off `dims` which is constant (sprite size is fixed) and keyed on the
    // enum triple so a skin-picker change invalidates them automatically.
    static COSMETICS_MESH_CACHE: RefCell<Option<(PlayerSkin, Vec<(Mesh, DrawParam)>)>> =
        RefCell::new(None);
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
            canvas.draw_instanced_mesh_guarded(unit_line, instances, DrawParam::default());
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
            canvas.draw_instanced_mesh_guarded(unit_circle, instances, DrawParam::default());
            Ok(())
        })?;
        params.clear();
        Ok(())
    })
}

/// Draw (and clear) all orbiting sparkle dots accumulated by draw_golden_sparkle() calls since the
/// last flush, as a single instanced batch. Every dot uses the same UNIT_CIRCLE mesh scaled by its
/// DrawParam, so any number of Goldens' sparkles collapse into one GPU submission. Call once per
/// drawing pass alongside flush_crab_legs() / flush_crab_bodies(). The canvas must already be in
/// ADD blend mode (the caller sets this for the whole per-crab aura pass).
pub fn flush_golden_sparkles(ctx: &mut Context, canvas: &mut Canvas) -> ggez::GameResult {
    GOLDEN_SPARKLE_PARAMS.with(|params_cell| -> ggez::GameResult {
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
        GOLDEN_SPARKLE_INSTANCES.with(|inst_cell| -> ggez::GameResult {
            let mut inst_slot = inst_cell.borrow_mut();
            let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
            instances.set(params.iter().copied());
            canvas.draw_instanced_mesh_guarded(unit_circle, instances, DrawParam::default());
            Ok(())
        })?;
        params.clear();
        Ok(())
    })
}

/// Flush all beat-corona DrawParams deferred by draw_crab() for caught crabs during a strong beat.
/// Each corona is a large soft circle in the crab's own color, blended additively for a glow halo
/// that pulses with the music. Call this once per frame inside the ADD blend pass, alongside the
/// other crab-aura flushes (flush_golden_sparkles / flush_hermit_coil_dots / etc.).
pub fn flush_beat_coronas(ctx: &mut Context, canvas: &mut Canvas) -> ggez::GameResult {
    BEAT_CORONA_PARAMS.with(|params_cell| -> ggez::GameResult {
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
        BEAT_CORONA_INSTANCES.with(|inst_cell| -> ggez::GameResult {
            let mut inst_slot = inst_cell.borrow_mut();
            let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
            instances.set(params.iter().copied());
            canvas.draw_instanced_mesh_guarded(unit_circle, instances, DrawParam::default());
            Ok(())
        })?;
        params.clear();
        Ok(())
    })
}

/// Flush all hermit-coil dot DrawParams deferred by draw_hermit_shell() calls this frame into a
/// single draw_instanced_mesh. Call this once after all per-crab aura draws (alongside
/// flush_golden_sparkles / flush_crab_legs / flush_crab_bodies) while still in ADD blend mode.
pub fn flush_hermit_coil_dots(ctx: &mut Context, canvas: &mut Canvas) -> ggez::GameResult {
    HERMIT_COIL_PARAMS.with(|params_cell| -> ggez::GameResult {
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
        HERMIT_COIL_INSTANCES.with(|inst_cell| -> ggez::GameResult {
            let mut inst_slot = inst_cell.borrow_mut();
            let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
            instances.set(params.iter().copied());
            canvas.draw_instanced_mesh_guarded(unit_circle, instances, DrawParam::default());
            Ok(())
        })?;
        params.clear();
        Ok(())
    })
}

/// Flush all catch-next-hint tick-dot DrawParams deferred by draw_catch_next_hint() calls this
/// frame into a single draw_instanced_mesh call. All dots share the same fixed stroke-circle
/// mesh (radius 2.2, thickness 1.4) so no grouping is needed — one instanced draw covers every
/// dot from every matching crab on screen. Call once per frame after the per-crab aura pass,
/// while still in ADD blend mode, alongside flush_hermit_coil_dots / flush_magnet_auras / etc.
pub fn flush_catch_next_ticks(ctx: &mut Context, canvas: &mut Canvas) -> ggez::GameResult {
    CATCH_NEXT_TICK_PARAMS.with(|params_cell| -> ggez::GameResult {
        let mut params = params_cell.borrow_mut();
        if params.is_empty() {
            return Ok(());
        }
        let tick_mesh = cached_stroke_circle(ctx, 2.2, 1.4)?;
        CATCH_NEXT_TICK_INSTANCES.with(|inst_cell| -> ggez::GameResult {
            let mut inst_slot = inst_cell.borrow_mut();
            let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
            instances.set(params.iter().copied());
            canvas.draw_instanced_mesh_guarded(tick_mesh, instances, DrawParam::default());
            Ok(())
        })?;
        params.clear();
        Ok(())
    })
}

/// Flush all centerpiece bracket-dot DrawParams deferred by draw_centerpiece_ring() calls this
/// frame into a single draw_instanced_mesh call. All dots share the same fixed stroke-circle
/// mesh (radius 2.2, thickness 1.5) so no grouping is needed — one instanced draw covers every
/// bracket dot from every centerpiece link on screen. Call once per frame after the chain-crab
/// loop, in ADD blend mode, alongside flush_crab_legs / flush_crab_bodies. On a long run with
/// a centerpiece arrangement this collapses up to 10 × run_len individual canvas.draw() calls
/// (e.g. 60 for a 6-link centerpiece run) down to 1 GPU submission. Identical on-screen output.
pub fn flush_centerpiece_dots(ctx: &mut Context, canvas: &mut Canvas) -> ggez::GameResult {
    CENTERPIECE_DOT_PARAMS.with(|params_cell| -> ggez::GameResult {
        let mut params = params_cell.borrow_mut();
        if params.is_empty() {
            return Ok(());
        }
        let dot_mesh = cached_stroke_circle(ctx, 2.2, 1.5)?;
        let orig_blend = canvas.blend_mode();
        canvas.set_blend_mode(BlendMode::ADD);
        CENTERPIECE_DOT_INSTANCES.with(|inst_cell| -> ggez::GameResult {
            let mut inst_slot = inst_cell.borrow_mut();
            let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
            instances.set(params.iter().copied());
            canvas.draw_instanced_mesh_guarded(dot_mesh, instances, DrawParam::default());
            Ok(())
        })?;
        canvas.set_blend_mode(orig_blend);
        params.clear();
        Ok(())
    })
}

/// Flush all attracted-crab glow ring DrawParams deferred by draw_attracted_crab_glow() calls
/// this frame, grouped by stroke-circle mesh key and drawn as instanced batches. Replaces up
/// to 60 individual canvas.draw() calls (2 per crab-in-flashlight × ~30 crabs) with one
/// draw_instanced_mesh submission per distinct stroke radius bucket — typically 2-4 total.
/// Call this once after all per-crab aura draws while still in ADD blend mode, alongside
/// flush_hermit_coil_dots / flush_magnet_auras / flush_golden_sparkles / flush_crab_legs /
/// flush_crab_bodies.
pub fn flush_attracted_crab_glows(ctx: &mut Context, canvas: &mut Canvas) -> ggez::GameResult {
    // Outer soft-glow rings
    ATTRACTED_GLOW_GROUPS.with(|groups_cell| -> ggez::GameResult {
        let mut groups = groups_cell.borrow_mut();
        ATTRACTED_GLOW_INSTANCES.with(|inst_cell| -> ggez::GameResult {
            let mut instances = inst_cell.borrow_mut();
            for (key, params) in groups.iter() {
                if params.is_empty() {
                    continue;
                }
                let mesh = STROKE_CIRCLE_CACHE.with(|c| c.borrow().get(key).cloned());
                let Some(mesh) = mesh else { continue };
                let inst = instances.entry(*key).or_insert_with(|| InstanceArray::new(ctx, None));
                inst.set(params.iter().copied());
                canvas.draw_instanced_mesh_guarded(mesh, inst, DrawParam::default());
            }
            Ok(())
        })?;
        for v in groups.values_mut() { v.clear(); }
        Ok(())
    })?;
    // Inner bright rings
    ATTRACTED_RING_GROUPS.with(|groups_cell| -> ggez::GameResult {
        let mut groups = groups_cell.borrow_mut();
        ATTRACTED_RING_INSTANCES.with(|inst_cell| -> ggez::GameResult {
            let mut instances = inst_cell.borrow_mut();
            for (key, params) in groups.iter() {
                if params.is_empty() {
                    continue;
                }
                let mesh = STROKE_CIRCLE_CACHE.with(|c| c.borrow().get(key).cloned());
                let Some(mesh) = mesh else { continue };
                let inst = instances.entry(*key).or_insert_with(|| InstanceArray::new(ctx, None));
                inst.set(params.iter().copied());
                canvas.draw_instanced_mesh_guarded(mesh, inst, DrawParam::default());
            }
            Ok(())
        })?;
        for v in groups.values_mut() { v.clear(); }
        Ok(())
    })
}

/// Flush all Magnet aura ring DrawParams deferred by draw_magnet_aura() calls this frame,
/// grouped by stroke-circle mesh key and drawn as instanced batches. Call this once after all
/// per-crab aura draws (alongside flush_golden_sparkles / flush_hermit_coil_dots) while still
/// in ADD blend mode. With N Magnets on screen, the 3 sweep-ring phases each share the same
/// radius across all N Magnets (same time → same phase → same quantized bucket), collapsing
/// N×3 individual draw calls down to 3 batched submissions for the sweep rings plus one
/// per distinct core-ring radius (which varies by crab size, so typically still N calls there,
/// but the sweep rings are the majority).
pub fn flush_magnet_auras(ctx: &mut Context, canvas: &mut Canvas) -> ggez::GameResult {
    MAGNET_AURA_RING_PARAMS.with(|params_cell| -> ggez::GameResult {
        let mut params = params_cell.borrow_mut();
        if params.is_empty() {
            return Ok(());
        }
        MAGNET_AURA_GROUPS.with(|groups_cell| -> ggez::GameResult {
            let mut groups = groups_cell.borrow_mut();
            for v in groups.values_mut() {
                v.clear();
            }
            for &(key, param) in params.iter() {
                groups.entry(key).or_default().push(param);
            }
            MAGNET_AURA_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                let mut instances = inst_cell.borrow_mut();
                for (key, group_params) in groups.iter() {
                    if group_params.is_empty() {
                        continue;
                    }
                    let mesh = STROKE_CIRCLE_CACHE.with(|c| c.borrow().get(key).cloned());
                    let Some(mesh) = mesh else { continue };
                    let inst = instances
                        .entry(*key)
                        .or_insert_with(|| InstanceArray::new(ctx, None));
                    inst.set(group_params.iter().copied());
                    canvas.draw_instanced_mesh_guarded(mesh, inst, DrawParam::default());
                }
                Ok(())
            })
        })?;
        params.clear();
        Ok(())
    })
}

/// Ensure the (radius, thickness) stroke-circle mesh exists in the cache and queue one instance of
/// it (at `pos`, tinted `color`) into `ARCHETYPE_RING_GROUPS` for `flush_archetype_rings()` to draw
/// later, instead of drawing it immediately. Used by draw_thief_aura / draw_splitter_aura /
/// draw_golden_sparkle / draw_armor_ring — each call site previously built the mesh and issued its
/// own `canvas.draw()`, so the shared radius bucket never collapsed multiple crabs' rings together.
fn defer_archetype_ring(
    ctx: &mut Context,
    pos: Vec2,
    radius: f32,
    thickness: f32,
    color: Color,
) -> ggez::GameResult {
    cached_stroke_circle(ctx, radius, thickness)?;
    let key = stroke_circle_key(radius, thickness);
    ARCHETYPE_RING_GROUPS.with(|groups_cell| {
        groups_cell
            .borrow_mut()
            .entry(key)
            .or_default()
            .push(DrawParam::default().dest(pos).color(color));
    });
    Ok(())
}

/// Flush every archetype-ring DrawParam queued by `defer_archetype_ring()` this frame (Thief prowl/
/// latch/snared/lured rings, Splitter halo + beat flare, Golden shine halo + snared tether, Armored
/// crab's shell track), grouped by stroke-circle mesh key and drawn as instanced batches — same
/// technique as flush_magnet_auras / flush_attracted_crab_glows above. Also flushes the Splitter
/// cleave dots queued alongside them. Call once after all per-crab aura draws (alongside those),
/// while still in ADD blend mode.
pub fn flush_archetype_rings(ctx: &mut Context, canvas: &mut Canvas) -> ggez::GameResult {
    ARCHETYPE_RING_GROUPS.with(|groups_cell| -> ggez::GameResult {
        let mut groups = groups_cell.borrow_mut();
        ARCHETYPE_RING_INSTANCES.with(|inst_cell| -> ggez::GameResult {
            let mut instances = inst_cell.borrow_mut();
            for (key, params) in groups.iter() {
                if params.is_empty() {
                    continue;
                }
                let mesh = STROKE_CIRCLE_CACHE.with(|c| c.borrow().get(key).cloned());
                let Some(mesh) = mesh else { continue };
                let inst = instances.entry(*key).or_insert_with(|| InstanceArray::new(ctx, None));
                inst.set(params.iter().copied());
                canvas.draw_instanced_mesh_guarded(mesh, inst, DrawParam::default());
            }
            Ok(())
        })?;
        for v in groups.values_mut() {
            v.clear();
        }
        Ok(())
    })?;
    CLEAVE_DOT_PARAMS.with(|params_cell| -> ggez::GameResult {
        let mut params = params_cell.borrow_mut();
        if params.is_empty() {
            return Ok(());
        }
        let unit_circle = cached_unit_circle(ctx)?;
        CLEAVE_DOT_INSTANCES.with(|inst_cell| -> ggez::GameResult {
            let mut inst_slot = inst_cell.borrow_mut();
            let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
            instances.set(params.iter().copied());
            canvas.draw_instanced_mesh_guarded(unit_circle, instances, DrawParam::default());
            Ok(())
        })?;
        params.clear();
        Ok(())
    })
}

/// Fetch the shared unit filled-circle mesh (radius 1, centered at origin) — the same one the
/// crab-body instanced batch uses. Draw it with `.scale(Vec2::splat(r))` to get an r-radius dot
/// without allocating a fresh circle mesh per call. Lazily initialized once, then cloned (a Mesh
/// clone is a cheap handle clone, not a GPU re-upload).
fn cached_unit_circle(ctx: &mut Context) -> ggez::GameResult<Mesh> {
    Ok(match UNIT_CIRCLE.get() {
        Some(mesh) => mesh.clone(),
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh).clone()
        }
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
    let line = unit_line(ctx)?.clone();
    let wake = -last_dir.normalize();
    let angle = wake.y.atan2(wake.x);
    let perp = Vec2::new(-wake.y, wake.x);
    let alpha = (intensity.clamp(0.0, 1.0) * 110.0) as u8;
    let col = Color::from_rgba(190, 215, 255, alpha);
    // Batch all 7 wake lines into one InstanceArray draw instead of 7 individual canvas.draw()
    // calls — same unit_line mesh, same color, different dest/rotation/scale per line.
    SPEED_LINE_INSTANCES.with(|cell| -> ggez::GameResult {
        let mut slot = cell.borrow_mut();
        let instances = slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
        instances.set((0i32..7).map(|i| {
            let t = (i as f32 - 3.0) / 3.0;
            let origin = center + perp * (t * 14.0);
            let length = 20.0 + (3.0 - (i as f32 - 3.0).abs()) * 8.0;
            DrawParam::default()
                .dest(origin)
                .rotation(angle)
                .scale(Vec2::new(length, 1.5))
                .color(col)
        }));
        canvas.draw_instanced_mesh_guarded(line, instances, DrawParam::default());
        Ok(())
    })
}

/// Draw the longer, greener whoosh used while sprinting. Same batched unit-line wake as the dash
/// effect, but stretched wider and tinted more like wind than impact so it reads as a held speed
/// state instead of a short burst.
pub fn draw_sprint_whoosh(
    ctx: &mut Context,
    canvas: &mut Canvas,
    center: Vec2,
    last_dir: Vec2,
    time: f32,
    intensity: f32,
) -> ggez::GameResult {
    if last_dir.length() < 0.01 {
        return Ok(());
    }
    let line = unit_line(ctx)?.clone();
    let wake = -last_dir.normalize();
    let angle = wake.y.atan2(wake.x);
    let perp = Vec2::new(-wake.y, wake.x);
    let alpha = (intensity.clamp(0.0, 1.0) * 90.0) as u8;
    let col = Color::from_rgba(140, 255, 200, alpha);
    SPEED_LINE_INSTANCES.with(|cell| -> ggez::GameResult {
        let mut slot = cell.borrow_mut();
        let instances = slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
        instances.set((0i32..7).map(|i| {
            let t = (i as f32 - 3.0) / 3.0;
            let wobble_phase = time * 9.0 + i as f32 * 0.85;
            let wobble = wobble_phase.sin() * 5.0;
            let flutter = (time * 14.0 + i as f32 * 1.3).cos() * 0.5 + 0.5;
            let origin = center + perp * (t * 20.0 + wobble) - wake * (4.0 + flutter * 6.0);
            let length = 28.0 + (3.0 - (i as f32 - 3.0).abs()) * 11.0 + flutter * 8.0;
            DrawParam::default()
                .dest(origin)
                .rotation(angle)
                .scale(Vec2::new(length, 1.7 + flutter * 0.5))
                .color(Color::from_rgba(140, 255, 200, alpha.saturating_add((flutter * 25.0) as u8)))
        }));
        canvas.draw_instanced_mesh_guarded(line, instances, DrawParam::default());
        Ok(())
    })
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
    // On-beat streak heat, 0..1: 0 = no live streak, rising as consecutive on-beat catches climb
    // the HEATING UP -> ON FIRE -> BLAZING -> INFERNO tiers. Drives the vignette hotter — wider
    // reach, more opacity, and a color push toward orange/red fire — so the game's most watchable
    // rhythm-escalation moment visibly sets the screen edges ablaze instead of only spawning text.
    heat: f32,
    // Phase across the current beat, 0..1 (0 = on the beat). Used at full groove (>0.8) to add a
    // warm golden spotlight glow that pulses ON the beat — literally flashing in time with the music
    // so a flow-state player sees the whole screen respond to each hit.
    beat_phase: f32,
) -> ggez::GameResult {
    let heat = heat.clamp(0.0, 1.0);
    // Nothing until the player is meaningfully in the groove — keeps it a reward, not clutter,
    // and means zero draws during ordinary cold play. A live streak forces the vignette on even
    // if the groove meter dipped, so the heat always reads while the run is hot.
    if groove < 0.25 && heat <= 0.0 {
        return Ok(());
    }
    // Remap 0.25..1.0 onto 0..1 so the glow eases in from the threshold rather than popping on.
    let t = ((groove - 0.25) / 0.75).clamp(0.0, 1.0);

    // Base color walks cyan -> magenta/gold as the meter fills, matching the corner groove bar.
    let base_r = 0.30 + t * 0.70;
    let base_g = 0.95 - t * 0.45;
    let base_b = 0.90 - t * 0.55;
    // Streak heat blends the whole frame toward fire — orange at first, deep red at INFERNO — so a
    // hot on-beat run reads as the screen literally heating up, not just a text callout.
    let fire_r = 1.0;
    let fire_g = 0.45 - heat * 0.35;
    let fire_b = 0.12 * (1.0 - heat);
    let r = base_r + (fire_r - base_r) * heat;
    let g = base_g + (fire_g - base_g) * heat;
    let b = base_b + (fire_b - base_b) * heat;

    // Breathe on the beat: a maxed groove pulses harder so the frame throbs in time with the music.
    // Heat throbs harder still — a blazing streak makes the frame flare on every beat.
    let pulse = 1.0 + beat_intensity * (0.25 + t * 0.55 + heat * 0.45);
    // How far the glow reaches in from each edge, and its peak opacity — both grow with the meter
    // and are pushed further by streak heat so the fire bloom crowds in from the edges as it climbs.
    let reach = (26.0 + t * 90.0 + heat * 70.0) * pulse;
    let peak = (0.10 + t * 0.32 + heat * 0.26) * pulse;

    // Stack a few bands per edge, fading toward the interior, to fake a smooth gradient falloff.
    // All bands use the same unit_square mesh — batch into one InstanceArray (up to 20 draws
    // collapsed to one GPU submission) instead of issuing 20 individual canvas.draw() calls.
    const BANDS: usize = 5;
    let sq = unit_square(ctx)?.clone();
    VIGNETTE_PARAMS.with(|params_cell| -> ggez::GameResult {
        let mut params = params_cell.borrow_mut();
        params.clear();
        for i in 0..BANDS {
            // Band 0 sits at the very edge (widest/brightest); inner bands are thinner slivers
            // that taper the glow off toward the play area.
            let f = i as f32 / BANDS as f32;
            let band = reach * (1.0 - f);
            if band < 0.5 {
                continue;
            }
            // Alpha falls off quadratically inward so the edge reads as a soft bloom, not a hard bar.
            let a = (peak * (1.0 - f) * (1.0 - f)).clamp(0.0, 0.85);
            let col = Color::new(r, g, b, a);
            // Top edge
            params.push(DrawParam::default()
                .dest(Vec2::new(0.0, 0.0))
                .scale(Vec2::new(width, band))
                .color(col));
            // Bottom edge
            params.push(DrawParam::default()
                .dest(Vec2::new(0.0, height - band))
                .scale(Vec2::new(width, band))
                .color(col));
            // Left edge
            params.push(DrawParam::default()
                .dest(Vec2::new(0.0, 0.0))
                .scale(Vec2::new(band, height))
                .color(col));
            // Right edge
            params.push(DrawParam::default()
                .dest(Vec2::new(width - band, 0.0))
                .scale(Vec2::new(band, height))
                .color(col));
        }
        if !params.is_empty() {
            VIGNETTE_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                let mut inst_slot = inst_cell.borrow_mut();
                let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                instances.set(params.iter().copied());
                canvas.draw_instanced_mesh_guarded(sq, instances, DrawParam::default());
                Ok(())
            })?;
        }
        Ok(())
    })?;

    // Flow-state golden spotlight: at high groove (>0.8) add a warm additive glow around the
    // screen edges that pulses ON the beat — brighter at beat_phase=0 (the hit), fading across
    // the bar. At full groove the whole screen feels like a lit stage, not just the dark-vignette
    // danger-zone look of ordinary play. This is a reward for staying in the pocket.
    if groove > 0.8 {
        let flow = ((groove - 0.8) / 0.2).clamp(0.0, 1.0);
        // beat_phase=0 is the downbeat; remap so glow peaks at 0 and fades by ~0.5 of the beat
        let on_beat = (1.0 - beat_phase.clamp(0.0, 1.0)).powf(1.5);
        let glow_a = flow * (0.10 + 0.12 * on_beat);
        let sq = unit_square(ctx)?.clone();
        let glow_col = Color::new(1.0, 0.85, 0.3, glow_a);
        let band = height * 0.20;
        canvas.set_blend_mode(BlendMode::ADD);
        // top edge
        canvas.draw(&sq, DrawParam::default()
            .dest(Vec2::ZERO)
            .scale(Vec2::new(width, band))
            .color(glow_col));
        // bottom edge
        canvas.draw(&sq, DrawParam::default()
            .dest(Vec2::new(0.0, height - band))
            .scale(Vec2::new(width, band))
            .color(glow_col));
        // left edge
        canvas.draw(&sq, DrawParam::default()
            .dest(Vec2::ZERO)
            .scale(Vec2::new(band, height))
            .color(glow_col));
        // right edge
        canvas.draw(&sq, DrawParam::default()
            .dest(Vec2::new(width - band, 0.0))
            .scale(Vec2::new(band, height))
            .color(glow_col));
        canvas.set_blend_mode(BlendMode::ALPHA);
    }
    Ok(())
}

/// On-beat catch impact punch: a sharp additive flash + expanding ring at the catch position.
/// `beat_quality` is 0.5 for an ordinary on-beat catch and 1.0 for a PERFECT downbeat hit —
/// controls radius and opacity so perfect hits read louder. Called from draw_game for each
/// queued beat_punch_event.
pub fn draw_beat_hit_punch(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    crab_color: [f32; 3],
    beat_quality: f32,
) -> ggez::GameResult {
    let dot = unit_circle(ctx)?.clone();
    let [r, g, b] = crab_color;
    let scale = 18.0 + beat_quality * 28.0;
    canvas.set_blend_mode(BlendMode::ADD);
    // Sharp inner flash — the "hit" impulse
    canvas.draw(&dot, DrawParam::default()
        .dest(pos)
        .offset(Vec2::new(0.5, 0.5))
        .scale(Vec2::splat(scale))
        .color(Color::new(r, g, b, 0.7 * beat_quality)));
    // Expanding outer ring — the "resonance"
    canvas.draw(&dot, DrawParam::default()
        .dest(pos)
        .offset(Vec2::new(0.5, 0.5))
        .scale(Vec2::splat(scale * 2.2))
        .color(Color::new(r * 0.7 + 0.3, g * 0.7 + 0.3, b * 0.7 + 0.3, 0.25 * beat_quality)));
    canvas.set_blend_mode(BlendMode::ALPHA);
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

/// Rounded-rect equivalent of `cached_fill_rect` — see `ROUNDED_FILL_RECT_CACHE` for why this
/// exists (draw_tool_roster was rebuilding this GPU mesh every frame for a rect that only ever
/// takes one of a handful of distinct (position, size, color) combinations).
pub fn cached_rounded_fill_rect(
    ctx: &mut Context,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    color: Color,
) -> ggez::GameResult<Mesh> {
    let key = (
        (x * 2.0).round() as i32,
        (y * 2.0).round() as i32,
        (w * 2.0).round() as i32,
        (h * 2.0).round() as i32,
        (radius * 4.0).round() as i32,
        color.to_rgba_u32(),
    );

    if let Some(mesh) = ROUNDED_FILL_RECT_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return Ok(mesh);
    }

    let mesh = Mesh::new_rounded_rectangle(ctx, DrawMode::fill(), Rect::new(x, y, w, h), radius, color)?;
    ROUNDED_FILL_RECT_CACHE.with(|c| c.borrow_mut().insert(key, mesh.clone()));
    Ok(mesh)
}

/// Rounded-rect equivalent of `cached_stroke_rect`, at a fixed (x, y) offset like
/// `cached_rounded_fill_rect` rather than the origin-relative `cached_stroke_rect` — see
/// `ROUNDED_STROKE_RECT_CACHE`.
pub fn cached_rounded_stroke_rect(
    ctx: &mut Context,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    thickness: f32,
    color: Color,
) -> ggez::GameResult<Mesh> {
    let key = (
        (x * 2.0).round() as i32,
        (y * 2.0).round() as i32,
        (w * 2.0).round() as i32,
        (h * 2.0).round() as i32,
        (radius * 4.0).round() as i32,
        (thickness * 4.0).round() as i32,
        color.to_rgba_u32(),
    );

    if let Some(mesh) = ROUNDED_STROKE_RECT_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return Ok(mesh);
    }

    let mesh = Mesh::new_rounded_rectangle(ctx, DrawMode::stroke(thickness), Rect::new(x, y, w, h), radius, color)?;
    ROUNDED_STROKE_RECT_CACHE.with(|c| c.borrow_mut().insert(key, mesh.clone()));
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

pub fn draw_rustler(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    sprite: &Image,
    velocity: Vec2,
    beat_intensity: f32,
    time: f32,
    dashing: bool,
    skin: PlayerSkin,
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

    // Cosmetic layers: hat / facial hair / accessory. Everything is drawn on top of the
    // sprite and anchored to `center` + `bob` (the same hop offset the sprite uses) so it
    // sticks to the crab through the beat hop. All offsets scale off `dims` (the on-screen
    // crab size) so they stay proportional. `w`/`h` are the sprite's on-screen extents.
    draw_player_cosmetics(ctx, canvas, center + Vec2::new(0.0, bob), dims, skin)?;

    Ok(())
}

/// Draw the player's chosen cosmetics on top of the crab sprite. `c` is the sprite centre
/// (already including the beat hop), `dims` its on-screen size. All offsets are proportional
/// to `dims` so the drip reads correctly at any player scale.
///
/// Meshes for hats/facial-hair/accessories are built once per skin choice (in origin space,
/// with c = Vec2::ZERO) and cached in COSMETICS_MESH_CACHE. On every subsequent frame the
/// function just iterates the cached Vec and translates each mesh's DrawParam by the current
/// `c`. This eliminates up to ~8 Mesh::new_rectangle/new_polygon/new_circle GPU allocations
/// per frame (constant cost, every frame the player is drawn) for all non-default skins.
fn draw_player_cosmetics(
    ctx: &mut Context,
    canvas: &mut Canvas,
    c: Vec2,
    dims: Vec2,
    skin: PlayerSkin,
) -> ggez::GameResult {
    // Try the fast path first: if the cached skin matches, just translate each mesh by `c`
    // and draw. No allocations, no mesh building.
    let cache_hit = COSMETICS_MESH_CACHE.with(|cache| {
        let cache = cache.borrow();
        if let Some((cached_skin, _)) = cache.as_ref() {
            *cached_skin == skin
        } else {
            false
        }
    });

    if !cache_hit {
        // Build the meshes with c = Vec2::ZERO so the DrawParams encode body-local offsets.
        let meshes = build_cosmetics_meshes(ctx, dims, skin)?;
        COSMETICS_MESH_CACHE.with(|cache| {
            *cache.borrow_mut() = Some((skin, meshes));
        });
    }

    // Draw cached meshes, translating each body-local DrawParam by the current `c` (which
    // changes every frame due to the beat hop). Reconstruct the translated DrawParam inline
    // from the cached one so we never allocate: just patch the dest field.
    COSMETICS_MESH_CACHE.with(|cache| -> ggez::GameResult {
        let cache = cache.borrow();
        if let Some((_, meshes)) = cache.as_ref() {
            for (mesh, param) in meshes {
                // Translate the body-local dest by the actual sprite centre `c`.
                let mut p = *param;
                if let ggez::graphics::Transform::Values { ref mut dest, .. } = p.transform {
                    dest.x += c.x;
                    dest.y += c.y;
                }
                canvas.draw(mesh, p);
            }
        }
        Ok(())
    })
}

/// Build the cosmetics meshes for `skin` in body-local space (c = Vec2::ZERO). Returns a
/// Vec of (Mesh, DrawParam) where DrawParam.dest is the body-local offset from the crab
/// centre. Called at most once per skin choice per session.
fn build_cosmetics_meshes(
    ctx: &mut Context,
    dims: Vec2,
    skin: PlayerSkin,
) -> ggez::GameResult<Vec<(Mesh, DrawParam)>> {
    let w = dims.x;
    let h = dims.y;

    // Reference points in body-local coords (c = Vec2::ZERO).
    // The sprite's geometric centre sits in the leg area; the face is well above it.
    let ht = Vec2::new(0.0, -h * 0.40); // head_top
    let fa = Vec2::new(0.0, -h * 0.20); // face / eye-level
    let mo = Vec2::new(0.0, -h * 0.08); // mouth (below eyes, still in upper shell)
    let sh = Vec2::new(0.0,  h * 0.10); // shell / chest

    let col = |r: u8, g: u8, b: u8| Color::from_rgb(r, g, b);

    // Helper: build a Mesh::new_rectangle with body-local coords and return it alongside a
    // zero-dest DrawParam (dest is already baked into the Rect's origin).
    let rect_mesh = |ctx: &mut Context, rect: Rect, color: Color| -> ggez::GameResult<(Mesh, DrawParam)> {
        let m = Mesh::new_rectangle(ctx, DrawMode::fill(), rect, color)?;
        Ok((m, DrawParam::default()))
    };

    let mut out: Vec<(Mesh, DrawParam)> = Vec::new();

    // ---- Hats -------------------------------------------------------------------------
    match skin.hat {
        Hat::None => {}
        Hat::Cowboy => {
            let brim = col(0xC8, 0xA4, 0x6E);
            out.push(rect_mesh(ctx, Rect::new(ht.x - w * 0.32, ht.y + h * 0.04, w * 0.64, h * 0.07), brim)?);
            out.push(rect_mesh(ctx, Rect::new(ht.x - w * 0.15, ht.y - h * 0.10, w * 0.30, h * 0.15), brim)?);
        }
        Hat::TopHat => {
            let black = col(0x1A, 0x1A, 0x2E);
            out.push(rect_mesh(ctx, Rect::new(ht.x - w * 0.28, ht.y + h * 0.06, w * 0.56, h * 0.06), black)?);
            out.push(rect_mesh(ctx, Rect::new(ht.x - w * 0.14, ht.y - h * 0.20, w * 0.28, h * 0.28), black)?);
        }
        Hat::Sombrero => {
            // Unit-circle items: clone the static mesh, encode offset in DrawParam.dest.
            let uc = unit_circle(ctx)?.clone();
            let yellow = col(0xF5, 0xC8, 0x42);
            out.push((uc.clone(), DrawParam::default()
                .dest(Vec2::new(ht.x, ht.y + h * 0.10))
                .scale(Vec2::new(w * 0.48, h * 0.10))
                .color(yellow)));
            out.push((uc, DrawParam::default()
                .dest(Vec2::new(ht.x, ht.y + h * 0.02))
                .scale(Vec2::new(w * 0.16, h * 0.14))
                .color(yellow)));
        }
        Hat::Bucket => {
            let olive = col(0x7A, 0x8C, 0x5E);
            out.push(rect_mesh(ctx, Rect::new(ht.x - w * 0.24, ht.y + h * 0.08, w * 0.48, h * 0.05), olive)?);
            out.push(rect_mesh(ctx, Rect::new(ht.x - w * 0.18, ht.y - h * 0.02, w * 0.36, h * 0.11), olive)?);
        }
        Hat::Bandana => {
            let red = col(0xD9, 0x3B, 0x3B);
            out.push(rect_mesh(ctx, Rect::new(ht.x - w * 0.26, ht.y + h * 0.06, w * 0.52, h * 0.08), red)?);
            let knot = Mesh::new_polygon(ctx, DrawMode::fill(), &[
                [ht.x + w * 0.26, ht.y + h * 0.06],
                [ht.x + w * 0.40, ht.y + h * 0.02],
                [ht.x + w * 0.40, ht.y + h * 0.18],
            ], red)?;
            out.push((knot, DrawParam::default()));
        }
        Hat::Beret => {
            let uc = unit_circle(ctx)?.clone();
            let teal = col(0x2E, 0x7D, 0x6E);
            out.push((uc.clone(), DrawParam::default()
                .dest(Vec2::new(ht.x - w * 0.06, ht.y + h * 0.06))
                .scale(Vec2::new(w * 0.22, h * 0.13))
                .rotation(-0.35)
                .color(teal)));
            out.push((uc, DrawParam::default()
                .dest(Vec2::new(ht.x + w * 0.10, ht.y - h * 0.02))
                .scale(Vec2::splat(w * 0.03))
                .color(teal)));
        }
        Hat::Crown => {
            let gold = col(0xFF, 0xD7, 0x00);
            let base_y = ht.y + h * 0.10;
            let pts = [
                [ht.x - w * 0.22, base_y],
                [ht.x - w * 0.22, ht.y - h * 0.02],
                [ht.x - w * 0.11, base_y - h * 0.06],
                [ht.x,            ht.y - h * 0.06],
                [ht.x + w * 0.11, base_y - h * 0.06],
                [ht.x + w * 0.22, ht.y - h * 0.02],
                [ht.x + w * 0.22, base_y],
            ];
            let crown = Mesh::new_polygon(ctx, DrawMode::fill(), &pts, gold)?;
            out.push((crown, DrawParam::default()));
        }
        Hat::HardHat => {
            let yellow = col(0xFF, 0xD6, 0x00);
            let uc = unit_circle(ctx)?.clone();
            out.push((uc, DrawParam::default()
                .dest(Vec2::new(ht.x, ht.y + h * 0.06))
                .scale(Vec2::new(w * 0.22, h * 0.20))
                .color(yellow)));
            out.push(rect_mesh(ctx, Rect::new(ht.x - w * 0.22, ht.y + h * 0.10, w * 0.44, h * 0.04), yellow)?);
        }
    }

    // ---- Facial hair ------------------------------------------------------------------
    let brown = col(0x6B, 0x3D, 0x1E);
    match skin.facial_hair {
        FacialHair::None => {}
        FacialHair::Mustache => {
            let m = Mesh::new_polygon(ctx, DrawMode::fill(), &[
                [mo.x - w * 0.16, mo.y - h * 0.02],
                [mo.x,            mo.y + h * 0.01],
                [mo.x + w * 0.16, mo.y - h * 0.02],
                [mo.x + w * 0.14, mo.y + h * 0.04],
                [mo.x,            mo.y + h * 0.03],
                [mo.x - w * 0.14, mo.y + h * 0.04],
            ], brown)?;
            out.push((m, DrawParam::default()));
        }
        FacialHair::Handlebar => {
            let m = Mesh::new_polygon(ctx, DrawMode::fill(), &[
                [mo.x - w * 0.26, mo.y - h * 0.06],
                [mo.x - w * 0.18, mo.y + h * 0.02],
                [mo.x,            mo.y + h * 0.03],
                [mo.x + w * 0.18, mo.y + h * 0.02],
                [mo.x + w * 0.26, mo.y - h * 0.06],
                [mo.x + w * 0.20, mo.y + h * 0.02],
                [mo.x,            mo.y + h * 0.06],
                [mo.x - w * 0.20, mo.y + h * 0.02],
            ], brown)?;
            out.push((m, DrawParam::default()));
        }
        FacialHair::Beard => {
            out.push(rect_mesh(ctx, Rect::new(mo.x - w * 0.18, mo.y, w * 0.36, h * 0.22), brown)?);
            let uc = unit_circle(ctx)?.clone();
            out.push((uc, DrawParam::default()
                .dest(Vec2::new(mo.x, mo.y + h * 0.22))
                .scale(Vec2::new(w * 0.18, h * 0.09))
                .color(brown)));
        }
        FacialHair::GoateePatch => {
            let uc = unit_circle(ctx)?.clone();
            out.push((uc, DrawParam::default()
                .dest(Vec2::new(mo.x, mo.y + h * 0.09))
                .scale(Vec2::new(w * 0.07, h * 0.07))
                .color(brown)));
        }
        FacialHair::Mutton => {
            let uc = unit_circle(ctx)?.clone();
            for s in [-1.0_f32, 1.0] {
                out.push((uc.clone(), DrawParam::default()
                    .dest(Vec2::new(fa.x + s * w * 0.24, fa.y + h * 0.06))
                    .scale(Vec2::new(w * 0.06, h * 0.11))
                    .color(brown)));
            }
        }
        FacialHair::FuManchu => {
            // FuManchu uses unit_line + draw_thick_line. Pre-compute the two line meshes as
            // scaled/rotated unit-lines, stored as (unit_line_clone, DrawParam).
            let line = unit_line(ctx)?.clone();
            for s in [-1.0_f32, 1.0] {
                let a = Vec2::new(mo.x + s * w * 0.12, mo.y);
                let b = Vec2::new(mo.x + s * w * 0.16, mo.y + h * 0.24);
                let d = b - a;
                let len = d.length().max(0.0001);
                let ang = d.y.atan2(d.x);
                out.push((line.clone(), DrawParam::default()
                    .dest(a)
                    .rotation(ang)
                    .scale(Vec2::new(len, w * 0.03))
                    .color(brown)));
            }
        }
    }

    // ---- Accessories ------------------------------------------------------------------
    match skin.accessory {
        Accessory::None => {}
        Accessory::StarBadge => {
            let star = star_mesh(ctx, w * 0.11, col(0xFF, 0xD7, 0x00))?;
            // star_mesh builds at origin; dest is the body-local offset from c.
            out.push((star, DrawParam::default().dest(Vec2::new(sh.x - w * 0.14, sh.y))));
        }
        Accessory::Monocle => {
            let ring = Mesh::new_circle(
                ctx, DrawMode::stroke(w * 0.02), [0.0, 0.0], w * 0.09, 0.5, Color::WHITE,
            )?;
            out.push((ring, DrawParam::default().dest(Vec2::new(fa.x + w * 0.13, fa.y - h * 0.02))));
        }
        Accessory::BowTie => {
            let white = Color::WHITE;
            // neck offset = (0, h*0.02) from c
            let nx = 0.0_f32;
            let ny = h * 0.02;
            let left = Mesh::new_polygon(ctx, DrawMode::fill(), &[
                [nx,              ny],
                [nx - w * 0.12,   ny - h * 0.06],
                [nx - w * 0.12,   ny + h * 0.06],
            ], white)?;
            out.push((left, DrawParam::default()));
            let right = Mesh::new_polygon(ctx, DrawMode::fill(), &[
                [nx,              ny],
                [nx + w * 0.12,   ny - h * 0.06],
                [nx + w * 0.12,   ny + h * 0.06],
            ], white)?;
            out.push((right, DrawParam::default()));
            out.push(rect_mesh(ctx, Rect::new(nx - w * 0.02, ny - h * 0.03, w * 0.04, h * 0.06), col(0x22, 0x22, 0x22))?);
        }
        Accessory::NeonChain => {
            let uc = unit_circle(ctx)?.clone();
            let gold = col(0xFF, 0xD7, 0x00);
            let n = 9;
            for i in 0..n {
                let t = i as f32 / (n as f32 - 1.0);
                let ang = std::f32::consts::PI * (0.15 + 0.70 * t);
                // sh = (0, h*0.10) in body-local coords
                let px = sh.x + ang.cos() * w * 0.26;
                let py = sh.y + h * 0.02 + ang.sin() * h * 0.16;
                out.push((uc.clone(), DrawParam::default()
                    .dest(Vec2::new(px, py))
                    .scale(Vec2::splat(w * 0.03))
                    .color(gold)));
            }
        }
        Accessory::Shades => {
            let dark = col(0x15, 0x15, 0x1A);
            // fa = (0, -h*0.08)
            for s in [-1.0_f32, 1.0] {
                out.push(rect_mesh(ctx, Rect::new(
                    fa.x + s * w * 0.13 - w * 0.09,
                    fa.y - h * 0.05,
                    w * 0.18,
                    h * 0.10,
                ), dark)?);
            }
            out.push(rect_mesh(ctx, Rect::new(fa.x - w * 0.05, fa.y - h * 0.02, w * 0.10, h * 0.02), dark)?);
        }
        Accessory::LassoLoop => {
            let tan = col(0xC8, 0xA4, 0x6E);
            // loop centre offset from c: (w*0.30, h*0.14)
            let lo = Vec2::new(w * 0.30, h * 0.14);
            let ring = Mesh::new_circle(ctx, DrawMode::stroke(w * 0.03), [0.0, 0.0], w * 0.11, 0.4, tan)?;
            out.push((ring, DrawParam::default().dest(lo)));
            let inner = Mesh::new_circle(ctx, DrawMode::stroke(w * 0.02), [0.0, 0.0], w * 0.06, 0.4, tan)?;
            out.push((inner, DrawParam::default().dest(lo)));
        }
        Accessory::GoldTooth => {
            // mo = (0, h*0.06)
            out.push(rect_mesh(ctx, Rect::new(mo.x - w * 0.02, mo.y - h * 0.01, w * 0.04, h * 0.05), col(0xFF, 0xD7, 0x00))?);
        }
    }

    Ok(out)
}

/// Draw a thick line between two points using the cached unit line, scaled to the length
/// and given thickness. Avoids a fresh Mesh::new_line per call.
fn draw_thick_line(canvas: &mut Canvas, line: &Mesh, a: Vec2, b: Vec2, thick: f32, color: Color) {
    let d = b - a;
    let len = d.length().max(0.0001);
    let ang = d.y.atan2(d.x);
    canvas.draw(
        line,
        DrawParam::default()
            .dest(a)
            .rotation(ang)
            .scale(Vec2::new(len, thick))
            .color(color),
    );
}

/// A filled 5-point star mesh of the given outer radius, centred on the origin.
fn star_mesh(ctx: &mut Context, r: f32, color: Color) -> ggez::GameResult<Mesh> {
    let mut pts = Vec::with_capacity(10);
    for i in 0..10 {
        let rad = if i % 2 == 0 { r } else { r * 0.42 };
        let ang = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
        pts.push([ang.cos() * rad, ang.sin() * rad]);
    }
    Mesh::new_polygon(ctx, DrawMode::fill(), &pts, color)
}

/// Level of detail for a crab. Ordered cheap→rich so `min()` picks the cheaper of two caps.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Detail {
    /// Swarm / tiny-on-screen: sculpted shell + simple claws + femur-only legs. Silhouette,
    /// accent colour and proportions still read the archetype; the fine articulation is dropped.
    Low,
    /// Mid-field: adds belly shade, jointed legs, claw notch, eye stalks, shell pattern.
    Mid,
    /// Hero / close: full articulation — rim light, soft cast shadow, pincer claws, blinking
    /// eyes, planted feet, antennae, the full shell pattern.
    Full,
}

/// Caller sets how many crabs it's about to draw this pass (see `CRAB_LOD_COUNT`) so the LOD
/// scales with the crowd. Call once at the top of a crab-drawing pass.
pub fn set_crab_lod_hint(count: usize) {
    CRAB_LOD_COUNT.with(|c| c.set(count));
}

/// Pick a crab's detail tier from both the crowd size (set via `set_crab_lod_hint`) and its
/// on-screen radius. The crowd sets a ceiling (a 200-crab swarm forces everyone Low); the size
/// sets its own (a tiny distant crab is Low no matter what), and we take the cheaper of the two.
fn crab_detail(size: f32) -> Detail {
    let count = CRAB_LOD_COUNT.with(|c| c.get());
    let by_count = if count > 170 {
        Detail::Low
    } else if count > 85 {
        Detail::Mid
    } else {
        Detail::Full
    };
    let by_size = if size < 11.0 {
        Detail::Low
    } else if size < 17.0 {
        Detail::Mid
    } else {
        Detail::Full
    };
    by_count.min(by_size)
}

/// One leg's precomputed geometry, filled by draw_crab's gait pass and consumed by both the body
/// batch (planted foot dots) and the leg batch (femur/tibia lines). A fixed `[LegGeo; 8]` (max 4
/// pairs) avoids a per-crab heap allocation.
#[derive(Clone, Copy)]
struct LegGeo {
    root: Vec2,
    femur_ang: f32,
    femur_len: f32,
    femur_tip: Vec2,
    tibia_ang: f32,
    tibia_len: f32,
    tibia_tip: Vec2,
    lift: f32,
}

/// Push one crab claw into the shared body-circle batch. Full detail is two hinged pincer fingers
/// (opened by ±`gape`, so they SNAP shut on the beat) plus a dark inner gap and a lit knuckle;
/// Mid is a knob + notch; Low is a bare knob. Pure world-space geometry — no `rotate_offset` needed.
#[allow(clippy::too_many_arguments)]
fn push_claw(
    params: &mut Vec<DrawParam>,
    wrist: Vec2,
    dir: f32,
    radius: f32,
    gape: f32,
    base: Color,
    highlight: Color,
    light_dir: Vec2,
    detail: Detail,
) {
    let d = Vec2::new(dir.cos(), dir.sin());
    if detail == Detail::Full {
        // Two pincer fingers hinged open by ±gape around the pointing direction.
        for (ang, len_w, wid_w) in [(dir - gape, 1.15_f32, 0.52_f32), (dir + gape, 1.0, 0.44)] {
            let a = Vec2::new(ang.cos(), ang.sin());
            params.push(
                DrawParam::default()
                    .dest(wrist + a * radius * 0.62)
                    .scale(Vec2::new(radius * len_w, radius * wid_w))
                    .rotation(ang)
                    .color(base),
            );
        }
        // Dark inner gap so the open pincer reads.
        params.push(
            DrawParam::default()
                .dest(wrist + d * radius * 0.5)
                .scale(Vec2::new(radius * 0.7, radius * (0.12 + 0.18 * gape)))
                .rotation(dir)
                .color(Color::new(0.08, 0.06, 0.08, 0.8)),
        );
        // Knuckle knob + lit highlight.
        params.push(
            DrawParam::default()
                .dest(wrist)
                .scale(Vec2::splat(radius * 0.52))
                .color(base),
        );
        params.push(
            DrawParam::default()
                .dest(wrist + light_dir * radius * 0.4)
                .scale(Vec2::splat(radius * 0.34))
                .color(highlight),
        );
    } else {
        let c = wrist + d * radius * 0.4;
        params.push(
            DrawParam::default()
                .dest(c)
                .scale(Vec2::new(radius * 1.1, radius * 0.85))
                .rotation(dir)
                .color(base),
        );
        if detail == Detail::Mid {
            params.push(
                DrawParam::default()
                    .dest(c + d * radius * 0.42)
                    .scale(Vec2::new(radius * 0.5, radius * (0.12 + 0.16 * gape)))
                    .rotation(dir)
                    .color(Color::new(0.08, 0.06, 0.08, 0.8)),
            );
            params.push(
                DrawParam::default()
                    .dest(c + light_dir * radius * 0.4)
                    .scale(Vec2::splat(radius * 0.4))
                    .color(highlight),
            );
        }
    }
}

// `canvas` is threaded through but no longer drawn to directly: every part draw_crab() used to
// issue immediately is now deferred into CRAB_LEG_PARAMS/CRAB_BODY_PARAMS and flushed as instanced
// batches by flush_crab_legs()/flush_crab_bodies() (called once per drawing pass by the caller).
// Kept in the signature so call sites don't need to change and so a future direct-draw effect
// (e.g. a one-off overlay) has it on hand without threading it through again.
pub fn draw_crab(ctx: &mut Context, _canvas: &mut Canvas, crab: &EnemyCrab, draw_pos: Vec2, beat_phase: f32, join_pulse: f32, y_lift: f32, rotation: f32, time: f32) -> ggez::GameResult {
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
    // Rotates a body-local offset (x, y) by the crab's facing rotation.
    let rotate_offset = |x: f32, y: f32| Vec2::new(x * cos_r - y * sin_r, x * sin_r + y * cos_r);

    // Per-archetype visual identity — proportions, leg/claw geometry, eyes, shell pattern and an
    // accent colour. This is the *shape* half of a crab's read (a Big crab heavy, a Sneaky one
    // skittish, a Dancer flashy, an armour-plated tank, a masked Thief) layered on top of its hue.
    let style = crab_style::style_for(crab.crab_type);

    // Grow size with age
    let grow_t = (crab.spawn_time / 10.0).min(1.0);
    let base_size = CRAB_SIZE * (0.6 + 0.4 * grow_t) * crab.scale;
    // Scale pop when joining the chain (bell-curve: peak at join_pulse=0.5)
    let pulse_scale = if join_pulse <= 1.0 {
        1.0 + 0.45 * join_pulse * (1.0 - join_pulse) * 4.0
    } else {
        1.0
    };
    // Whole-crab pump on the downbeat — every crab bounces a touch bigger on the beat so a train
    // of them visibly throbs to the music like a row of drum skins. Small (~6%) so it reads as
    // energy, not a size change.
    let beat_bounce = 1.0 + 0.06 * beat_phase;
    let size = base_size * pulse_scale * beat_bounce;

    // Level of detail: a calm field renders fully articulated hero crabs; a big swarm or a tiny/
    // distant crab drops to a cheaper form so the two instanced batches stay small and the [perf]
    // frame time doesn't regress on long trains. Silhouette + accent + pattern survive every tier.
    let detail = crab_detail(size);

    // Drop shadow: shrinks and moves away as the crab lifts off the ground
    let shadow_scale_x = (1.0 - y_lift / 60.0).clamp(0.4, 1.0);
    let shadow_scale_y = shadow_scale_x * 0.45;
    let shadow_offset_y = size * 0.35 + y_lift * 0.6;
    let shadow_offset_x = y_lift * 0.25;
    let shadow_alpha = ((1.0 - y_lift / 55.0) * 100.0).clamp(20.0, 100.0) as u8;

    // Color: more red as crab ages, and different color for type
    let [r, g, b] = crab.crab_color();
    let flash = if join_pulse > 0.0 && join_pulse <= 1.0 {
        join_pulse * (1.0 - join_pulse) * 4.0 * 0.5 // peak 0.5 at pulse=0.5
    } else {
        0.0
    };
    let crab_color = Color::new((r + flash).min(1.0), (g + flash).min(1.0), (b + flash).min(1.0), 1.0);
    // Secondary colour for shell pattern / claw tips / eye rims — the archetype's accent.
    let accent = Color::new(style.accent[0], style.accent[1], style.accent[2], 1.0);

    // Shell shading: give the flat body circle a rounded, lit look. Light comes from a fixed
    // screen-space direction (up and slightly left) so the whole herd reads as lit from the same
    // sky, independent of each crab's facing rotation — hence these offsets are NOT rotated.
    let light_dir = Vec2::new(-0.4, -0.72);
    let hi = |c: f32| (c + (1.0 - c) * 0.34).min(1.0);
    let dome_color = Color::new(hi(crab_color.r), hi(crab_color.g), hi(crab_color.b), 0.85);
    // Bright rim-light crescent on the lit edge — reads as a sculpted 3D dome, not a flat disc.
    let rim_light = Color::new(
        (hi(crab_color.r) + 0.22).min(1.0),
        (hi(crab_color.g) + 0.22).min(1.0),
        (hi(crab_color.b) + 0.22).min(1.0),
        0.5,
    );
    // Glossy specular glint near the top of the shell — pulses faintly with the beat.
    let glint_a = 0.5 + beat_phase * 0.35;

    // Carapace squash-and-stretch on the beat: the shell flattens and widens right on the downbeat.
    let shell_squash = 1.0 + 0.16 * beat_phase; // wider along the shell
    let shell_stretch = 1.0 - 0.11 * beat_phase; // flatter top-to-bottom
    let rim_color = Color::new(crab_color.r * 0.32, crab_color.g * 0.28, crab_color.b * 0.30, 0.92);
    let belly_color = Color::new(crab_color.r * 0.60, crab_color.g * 0.53, crab_color.b * 0.56, 0.55);

    // Shell half-extents actually drawn (the ellipse radii). Everything mounted on the rim — legs,
    // claws, eyes — is placed against these, so a wide Big crab's legs sit wider, a narrow Fast
    // crab's tuck in, etc. The archetype `body_w`/`body_h` factors are what make the silhouettes read.
    let sw = size * 0.62 * style.body_w;
    let sh = size * 0.48 * style.body_h;

    // Leg colours (derived from the crab's colour, darkened so legs sit behind the shell).
    let [lr, lg, lb] = crab.crab_color();
    let leg_color = Color::new(lr * 0.75, lg * 0.65, lb * 0.65, 1.0);
    let tibia_color = Color::new(
        (leg_color.r * 0.80).min(1.0),
        (leg_color.g * 0.80).min(1.0),
        (leg_color.b * 0.80).min(1.0),
        1.0,
    );

    // Scuttle gait: legs plant and lift in a walk cycle whose cadence rises with the crab's actual
    // velocity, so a parked crab barely shuffles and a bolting one visibly scuttles. The beat nudges
    // the cadence too, so the whole herd steps a little to the music. Precomputed into `legs` so both
    // the body batch (planted foot dots) and the leg batch (femur/tibia lines) can read the geometry.
    let speed = crab.vel.length();
    let moving = (speed / 55.0).clamp(0.0, 1.0);
    let gait_cadence = (5.0 + speed * 0.09) * style.gait * (1.0 + beat_phase * 0.25);
    let gait_off = (crab.pos.x + crab.pos.y) * 0.05;
    let leg_pairs = match detail {
        Detail::Low => style.leg_pairs.min(3),
        _ => style.leg_pairs,
    }
    .min(4);
    let mut legs = [LegGeo {
        root: Vec2::ZERO,
        femur_ang: 0.0,
        femur_len: 0.0,
        femur_tip: Vec2::ZERO,
        tibia_ang: 0.0,
        tibia_len: 0.0,
        tibia_tip: Vec2::ZERO,
        lift: 0.0,
    }; 8];
    let mut leg_n = 0usize;
    for side in [-1.0_f32, 1.0] {
        // Left legs radiate toward -x (PI), right toward +x (0), each fanned front-to-back.
        let center = if side < 0.0 { std::f32::consts::PI } else { 0.0 };
        for j in 0..leg_pairs {
            let frac = (j as f32 + 0.5) / leg_pairs as f32;
            let spread = 0.95 * style.leg_splay;
            let root_ang_body = center + (frac - 0.5) * 2.0 * spread;
            // Leg root on the shell rim, in body space then rotated to world.
            let rb = Vec2::new(root_ang_body.cos() * sw * 0.95, root_ang_body.sin() * sh * 0.95);
            let root = draw_pos + rotate_offset(rb.x, rb.y);
            // Contralateral tripod-ish phasing so neighbours step out of sync.
            let leg_i = j + if side < 0.0 { 0 } else { leg_pairs };
            let leg_phase = time * gait_cadence + gait_off + leg_i as f32 * 2.094;
            let swing = leg_phase.sin();
            let lift = swing.max(0.0) * moving; // 0 (planted) .. 1 (mid-step)
            let stride = swing * 0.35 * moving; // sweep the leg forward on the swing
            let idle_tw = (time * 2.0 + leg_i as f32).sin() * 0.05; // tiny twitch when parked
            let femur_ang = rotation + root_ang_body + stride + idle_tw;
            let femur_len = size * 0.42 * style.leg_len * (1.0 - 0.16 * lift);
            let femur_tip = root + Vec2::new(femur_ang.cos(), femur_ang.sin()) * femur_len;
            // Knees bend the same way per side (classic crab posture) with a small walk animation.
            let knee_bend = if side < 0.0 { 0.6_f32 } else { -0.6 };
            let knee_anim = leg_phase.cos() * 0.18 * moving;
            let tibia_ang = femur_ang + knee_bend + knee_anim;
            let tibia_len = size * 0.46 * style.leg_len * (1.0 - 0.22 * lift);
            let tibia_tip = femur_tip + Vec2::new(tibia_ang.cos(), tibia_ang.sin()) * tibia_len;
            if leg_n < 8 {
                legs[leg_n] = LegGeo {
                    root,
                    femur_ang,
                    femur_len,
                    femur_tip,
                    tibia_ang,
                    tibia_len,
                    tibia_tip,
                    lift,
                };
                leg_n += 1;
            }
        }
    }

    // Claws — articulated pincers whose size/symmetry/reach/rest-pose vary by archetype (a Big
    // crab's huge asymmetric crusher, a Splitter's matched scissors, a Dancer's raised arms).
    let claw_phase = (crab.pos.x - crab.pos.y) * 0.07;
    let idle_sine = (time * 1.8 + claw_phase).sin();
    // Bosses raise their claws while winding/charging; some archetypes rest them high.
    let wind_raise = match crab.charge_state {
        BossCharge::Winding(_) => 0.55,
        BossCharge::Charging(_) => 0.9,
        BossCharge::Idle => 0.0,
    };
    let claw_lift = (style.claw_lift + wind_raise).min(1.0);
    let crusher_r = size * 0.23 * style.claw_scale;
    // claw_sym 0 → a tiny opposite pincer, 1 → a matched twin.
    let pincer_r = crusher_r * (0.5 + 0.5 * style.claw_sym);
    // Pincer gape: idle flex + a hard SNAP shut right on the downbeat (clapping to the beat).
    let claw_idle_flex = idle_sine * 0.12;
    let gap_close = 1.0 - 0.72 * (beat_phase * beat_phase);
    let gape = ((0.42 + 0.28 * style.claw_lift + claw_idle_flex) * gap_close).max(0.02);
    // Wrists sit forward-and-out of the shell, raised when claw_lift is high.
    let wrist_x = sw * (1.02 * style.claw_reach);
    let wrist_y = -sh * (0.15 + 0.72 * claw_lift);
    let wrist_l = draw_pos + rotate_offset(-wrist_x, wrist_y);
    let wrist_r = draw_pos + rotate_offset(wrist_x * 0.97, wrist_y);
    // Claws point up-and-out; a forward lean grows with claw_reach (Thief grabs ahead).
    let reach_lean = (style.claw_reach - 1.0) * 0.4;
    let claw_dir_l = rotation - std::f32::consts::FRAC_PI_2 - 0.5 + reach_lean;
    let claw_dir_r = rotation - std::f32::consts::FRAC_PI_2 + 0.5 - reach_lean;

    // Eyes on stalks — bigger/wider/taller per archetype (Sneaky = huge shifty eyes on long stalks,
    // Big/Armored = small beady eyes tucked low).
    let eye_radius = size * 0.15 * style.eye_size;
    let eye_x = size * 0.22 * style.eye_spread;
    let eye_y = -size * 0.18;
    let stalk_len = size * 0.28 * style.stalk_len;
    let stalk_l_root = draw_pos + rotate_offset(-eye_x * 0.6, eye_y * 0.6);
    let stalk_r_root = draw_pos + rotate_offset(eye_x * 0.6, eye_y * 0.6);
    let stalk_angle_l = rotation - std::f32::consts::FRAC_PI_2 - 0.4;
    let stalk_angle_r = rotation - std::f32::consts::FRAC_PI_2 + 0.4;
    let eye_pos_l = stalk_l_root + Vec2::new(stalk_angle_l.cos(), stalk_angle_l.sin()) * stalk_len;
    let eye_pos_r = stalk_r_root + Vec2::new(stalk_angle_r.cos(), stalk_angle_r.sin()) * stalk_len;
    let pupil_r = eye_radius * (0.50 + beat_phase * 0.15);
    // Pupils track where the crab is going (free) or look forward down the train (caught).
    let (pdx, pdy) = if !crab.caught {
        let vl = crab.vel.length();
        if vl > 1.0 {
            (crab.vel.x / vl * eye_radius * 0.4, crab.vel.y / vl * eye_radius * 0.4)
        } else {
            (0.0, 0.0)
        }
    } else {
        (eye_radius * 0.28, 0.0)
    };
    // Occasional blink (Full detail only): a per-crab clock closes the lids for a moment so each
    // crab feels alive rather than dead-eyed.
    let blink_seed = (crab.pos.x * 0.017 + crab.pos.y * 0.011).fract().abs();
    let blink_cycle = (time * 0.33 + blink_seed * 7.0).rem_euclid(1.0);
    let blinking = detail == Detail::Full && blink_cycle < 0.05;

    // Antenna tips: point up-and-out from between the eyes, bobbing gently with the idle sine.
    let ant_ang_l = rotation - std::f32::consts::FRAC_PI_2 - 0.7;
    let ant_ang_r = rotation - std::f32::consts::FRAC_PI_2 + 0.7;
    let ant_tip_l =
        draw_pos + Vec2::new(ant_ang_l.cos(), ant_ang_l.sin()) * (size * (0.55 + 0.04 * idle_sine));
    let ant_tip_r =
        draw_pos + Vec2::new(ant_ang_r.cos(), ant_ang_r.sin()) * (size * (0.55 - 0.04 * idle_sine));

    // All the round crab parts (sculpted shell layers, shell pattern, articulated claws, eyes,
    // planted feet) are collected under a single thread-local borrow and flushed as one instanced
    // UNIT_CIRCLE batch by flush_crab_bodies() — so however lavish the crab gets, it's still one
    // GPU submission for the whole herd. The number of parts pushed scales with `detail`.
    CRAB_BODY_PARAMS.with(|params| {
        let mut params = params.borrow_mut();
        // Soft outer cast shadow (Full only) under the main shadow — grounds the crab on the sand.
        if detail == Detail::Full {
            params.push(
                DrawParam::default()
                    .dest(draw_pos + Vec2::new(shadow_offset_x, shadow_offset_y))
                    .scale(Vec2::new(size * shadow_scale_x * 0.82, size * shadow_scale_y * 0.82))
                    .color(Color::from_rgba(0, 0, 0, shadow_alpha / 2)),
            );
        }
        params.push(
            DrawParam::default()
                .dest(draw_pos + Vec2::new(shadow_offset_x, shadow_offset_y))
                .scale(Vec2::new(size * shadow_scale_x * 0.55, size * shadow_scale_y * 0.55))
                .color(Color::from_rgba(0, 0, 0, shadow_alpha)),
        );
        // Dark tinted rim just behind the shell — a subtle outline that lifts the crab off busy
        // terrain and off overlapping trainmates. Squashes with the body so it tracks the beat pop.
        params.push(
            DrawParam::default()
                .dest(draw_pos)
                .scale(Vec2::new(sw * shell_squash * 1.15, sh * shell_stretch * 1.15))
                .rotation(rotation)
                .color(rim_color),
        );
        // Crab body — elliptical per archetype (sw/sh), squashing on the beat.
        params.push(
            DrawParam::default()
                .dest(draw_pos)
                .scale(Vec2::new(sw * shell_squash, sh * shell_stretch))
                .rotation(rotation)
                .color(crab_color),
        );
        // Belly shade toward the shadow side (Mid+): a shaded underside so the shell reads as a
        // rounded, lit dome rather than a flat disc.
        if detail != Detail::Low {
            params.push(
                DrawParam::default()
                    .dest(draw_pos - light_dir * size * 0.13)
                    .scale(Vec2::new(sw * shell_squash * 0.86, sh * shell_stretch * 0.86))
                    .rotation(rotation)
                    .color(belly_color),
            );
        }
        // Domed highlight toward the light — the lit crown of the shell.
        params.push(
            DrawParam::default()
                .dest(draw_pos + light_dir * size * 0.15)
                .scale(Vec2::new(sw * 0.62 * shell_squash, sh * 0.62 * shell_stretch))
                .rotation(rotation)
                .color(dome_color),
        );
        // Rim-light crescent on the lit edge (Full) — the specular sheen of a wet 3D carapace.
        if detail == Detail::Full {
            params.push(
                DrawParam::default()
                    .dest(draw_pos + light_dir * size * 0.30)
                    .scale(Vec2::new(sw * shell_squash * 0.72, sh * shell_stretch * 0.34))
                    .rotation(rotation)
                    .color(rim_light),
            );
        }

        // Per-archetype shell pattern (Mid+) — the at-a-glance identity: armour plates, disco
        // spots, a cleaver split, a hermit whorl, a magnet polarity band, a bandit mask, gold
        // facets, a boss crown. Skipped at Low (a tiny/swarm crab where it wouldn't read anyway).
        if detail != Detail::Low {
            match style.pattern {
                ShellPattern::Plain => {
                    let ridge = Color::new(
                        (crab_color.r * 0.72).min(1.0),
                        (crab_color.g * 0.72).min(1.0),
                        (crab_color.b * 0.72).min(1.0),
                        0.75,
                    );
                    for ry in [-0.16_f32, 0.30_f32] {
                        params.push(
                            DrawParam::default()
                                .dest(draw_pos + rotate_offset(0.0, ry * sh))
                                .scale(Vec2::new(sw * 0.7, size * 0.06))
                                .rotation(rotation)
                                .color(ridge),
                        );
                    }
                }
                ShellPattern::Plates => {
                    let seam = Color::new(crab_color.r * 0.35, crab_color.g * 0.33, crab_color.b * 0.38, 0.85);
                    for ry in [-0.34_f32, 0.0, 0.34] {
                        params.push(
                            DrawParam::default()
                                .dest(draw_pos + rotate_offset(0.0, ry * sh))
                                .scale(Vec2::new(sw * 0.95, size * 0.03))
                                .rotation(rotation)
                                .color(seam),
                        );
                    }
                    if detail == Detail::Full {
                        for (rx, ry) in [(-0.55_f32, -0.4_f32), (0.55, -0.4), (-0.55, 0.4), (0.55, 0.4)] {
                            params.push(
                                DrawParam::default()
                                    .dest(draw_pos + rotate_offset(rx * sw, ry * sh))
                                    .scale(Vec2::splat(size * 0.04))
                                    .color(accent),
                            );
                        }
                    }
                }
                ShellPattern::Spots => {
                    for (rx, ry) in [(-0.4_f32, -0.35_f32), (0.35, -0.15), (0.0, 0.35), (-0.2, 0.6)] {
                        params.push(
                            DrawParam::default()
                                .dest(draw_pos + rotate_offset(rx * sw, ry * sh))
                                .scale(Vec2::splat(size * 0.07))
                                .color(Color::new(accent.r, accent.g, accent.b, 0.9)),
                        );
                    }
                }
                ShellPattern::Split => {
                    params.push(
                        DrawParam::default()
                            .dest(draw_pos)
                            .scale(Vec2::new(size * 0.045, sh * 1.02))
                            .rotation(rotation)
                            .color(accent),
                    );
                }
                ShellPattern::Whorl => {
                    for k in 0..4 {
                        let kk = k as f32;
                        let ang = rotation + kk * 1.6;
                        let rad = sw * (0.55 - kk * 0.12);
                        params.push(
                            DrawParam::default()
                                .dest(draw_pos + Vec2::new(ang.cos(), ang.sin()) * rad)
                                .scale(Vec2::splat(size * (0.09 - kk * 0.015)))
                                .color(Color::new(accent.r, accent.g, accent.b, 0.85)),
                        );
                    }
                }
                ShellPattern::Bands => {
                    params.push(
                        DrawParam::default()
                            .dest(draw_pos + rotate_offset(0.0, -sh * 0.4))
                            .scale(Vec2::new(sw * 0.95, sh * 0.5))
                            .rotation(rotation)
                            .color(Color::new(accent.r, accent.g, accent.b, 0.7)),
                    );
                }
                ShellPattern::Mask => {
                    params.push(
                        DrawParam::default()
                            .dest(draw_pos + rotate_offset(0.0, -sh * 0.5))
                            .scale(Vec2::new(sw * 1.0, sh * 0.34))
                            .rotation(rotation)
                            .color(Color::new(0.06, 0.12, 0.09, 0.82)),
                    );
                }
                ShellPattern::Shine => {
                    for (rx, ry, s) in [(-0.3_f32, -0.35_f32, 0.09_f32), (0.25, -0.1, 0.06), (0.1, 0.3, 0.05)] {
                        params.push(
                            DrawParam::default()
                                .dest(draw_pos + rotate_offset(rx * sw, ry * sh))
                                .scale(Vec2::splat(size * s))
                                .color(Color::new(1.0, 1.0, 0.9, 0.9)),
                        );
                    }
                }
                ShellPattern::Crown => {
                    for rx in [-0.5_f32, 0.0, 0.5] {
                        params.push(
                            DrawParam::default()
                                .dest(draw_pos + rotate_offset(rx * sw * 0.8, -sh * 1.02))
                                .scale(Vec2::new(size * 0.08, size * 0.13))
                                .rotation(rotation)
                                .color(accent),
                        );
                    }
                }
            }
        }

        // Specular glint (Mid+) — a bright bead near the top of the shell, pulsing with the beat.
        if detail != Detail::Low {
            params.push(
                DrawParam::default()
                    .dest(draw_pos + light_dir * size * 0.26)
                    .scale(Vec2::splat(size * 0.10))
                    .color(Color::new(1.0, 1.0, 1.0, glint_a)),
            );
        }

        // Articulated claws — a big crusher and a smaller (or matched, per claw_sym) pincer, both
        // snapping shut on the downbeat. Full detail hinges two fingers; Mid/Low simplify.
        push_claw(&mut params, wrist_l, claw_dir_l, crusher_r, gape, crab_color, dome_color, light_dir, detail);
        push_claw(&mut params, wrist_r, claw_dir_r, pincer_r, gape, crab_color, dome_color, light_dir, detail);

        // Eyes. When blinking (Full only) the whites become closed lid-slits; otherwise draw the
        // white, a tracking pupil, and (Mid+) a catch-light so the crab reads bright-eyed.
        if blinking {
            for ep in [eye_pos_l, eye_pos_r] {
                params.push(
                    DrawParam::default()
                        .dest(ep)
                        .scale(Vec2::new(eye_radius * 1.05, eye_radius * 0.22))
                        .rotation(rotation)
                        .color(crab_color),
                );
            }
        } else {
            for ep in [eye_pos_l, eye_pos_r] {
                params.push(
                    DrawParam::default()
                        .dest(ep)
                        .scale(Vec2::splat(eye_radius))
                        .color(Color::WHITE),
                );
            }
            for ep in [eye_pos_l, eye_pos_r] {
                params.push(
                    DrawParam::default()
                        .dest(ep + rotate_offset(pdx, pdy))
                        .scale(Vec2::splat(pupil_r))
                        .color(Color::BLACK),
                );
            }
            if detail != Detail::Low {
                let catch = pupil_r * 0.4;
                for ep in [eye_pos_l, eye_pos_r] {
                    params.push(
                        DrawParam::default()
                            .dest(ep + rotate_offset(pdx - eye_radius * 0.25, pdy - eye_radius * 0.25))
                            .scale(Vec2::splat(catch))
                            .color(Color::new(1.0, 1.0, 1.0, 0.9)),
                    );
                }
            }
        }

        // Planted feet (Full): a small dark bead at each leg tip, shrinking as the leg lifts off
        // the ground mid-step — the read that sells the scuttle.
        if detail == Detail::Full {
            let foot_c = Color::new(tibia_color.r * 0.8, tibia_color.g * 0.8, tibia_color.b * 0.8, 1.0);
            for lg in legs.iter().take(leg_n) {
                params.push(
                    DrawParam::default()
                        .dest(lg.tibia_tip)
                        .scale(Vec2::splat(size * 0.05 * style.leg_thick * (1.0 - 0.3 * lg.lift)))
                        .color(foot_c),
                );
            }
        }

        // Antenna tip beads (Full) at the ends of the two antennae drawn in the leg batch.
        if detail == Detail::Full {
            for tip in [ant_tip_l, ant_tip_r] {
                params.push(
                    DrawParam::default()
                        .dest(tip)
                        .scale(Vec2::splat(size * 0.05))
                        .color(Color::new(0.15, 0.10, 0.12, 1.0)),
                );
            }
        }
        // Little mouth (Mid+): a dark speck below the eyes so the face reads.
        if detail != Detail::Low {
            params.push(
                DrawParam::default()
                    .dest(draw_pos + rotate_offset(0.0, -size * 0.02))
                    .scale(Vec2::new(size * 0.10, size * 0.05))
                    .rotation(rotation)
                    .color(Color::new(0.12, 0.08, 0.10, 0.7)),
            );
        }
    });

    // Crab legs, claw arms, eye stalks and antennae are all thin lines, collected under a single
    // thread-local borrow and flushed as one instanced UNIT_LINE batch by flush_crab_legs().
    CRAB_LEG_PARAMS.with(|params| {
        let mut params = params.borrow_mut();
        // Jointed legs with a velocity-driven scuttle gait (geometry precomputed in `legs`): a
        // femur from the shell edge plus a bent tibia, thickness scaled per archetype. Low detail
        // draws the femur only.
        for lg in legs.iter().take(leg_n) {
            params.push(
                DrawParam::default()
                    .dest(lg.root)
                    .rotation(lg.femur_ang)
                    .scale(Vec2::new(lg.femur_len, 2.5 * style.leg_thick))
                    .color(leg_color),
            );
            if detail != Detail::Low {
                params.push(
                    DrawParam::default()
                        .dest(lg.femur_tip)
                        .rotation(lg.tibia_ang)
                        .scale(Vec2::new(lg.tibia_len, 1.8 * style.leg_thick))
                        .color(tibia_color),
                );
            }
        }

        // Claw arms — a segment from the shell edge out to each claw wrist. The crusher arm is
        // chunkier; a symmetric-clawed crab (claw_sym→1) gets matched arm thickness.
        let arm_root_l = draw_pos + rotate_offset(-sw * 0.7, -sh * 0.35);
        let arm_root_r = draw_pos + rotate_offset(sw * 0.7, -sh * 0.35);
        for (root, wrist, thick) in [
            (arm_root_l, wrist_l, 4.0 * style.leg_thick),
            (arm_root_r, wrist_r, (2.4 + 1.6 * style.claw_sym) * style.leg_thick),
        ] {
            let d = wrist - root;
            let len = d.length().max(0.0001);
            let ang = d.y.atan2(d.x);
            params.push(
                DrawParam::default()
                    .dest(root)
                    .rotation(ang)
                    .scale(Vec2::new(len, thick))
                    .color(leg_color),
            );
        }

        // Eye stalks — short lines from the shell to each eye circle.
        params.push(
            DrawParam::default()
                .dest(stalk_l_root)
                .rotation(stalk_angle_l)
                .scale(Vec2::new(stalk_len, 2.0))
                .color(leg_color),
        );
        // Antennae (Full) — two thin lines waving up-and-out from between the eyes to the tip
        // beads pushed into the body batch above. Slightly darker/thinner than the stalks.
        if detail == Detail::Full {
            let ant_root = draw_pos + rotate_offset(0.0, -size * 0.10);
            for tip in [ant_tip_l, ant_tip_r] {
                let d = tip - ant_root;
                let len = d.length().max(0.0001);
                let ang = d.y.atan2(d.x);
                params.push(
                    DrawParam::default()
                        .dest(ant_root)
                        .rotation(ang)
                        .scale(Vec2::new(len, 1.4))
                        .color(tibia_color),
                );
            }
        }
        params.push(
            DrawParam::default()
                .dest(stalk_r_root)
                .rotation(stalk_angle_r)
                .scale(Vec2::new(stalk_len, 2.0))
                .color(leg_color),
        );
    });

    // Beat corona: caught crabs in the conga train get a color-matched additive glow halo that
    // pulses with the music — the brighter the beat, the wider and more vivid the corona, so the
    // train visibly radiates light on every downbeat. Deferred into BEAT_CORONA_PARAMS and flushed
    // once per frame by flush_beat_coronas() in the same ADD blend pass as the other crab auras.
    if crab.caught && beat_phase > 0.3 {
        let glow_a = (beat_phase - 0.3) / 0.7 * 0.18;
        let [r, g, b] = crab.crab_color();
        BEAT_CORONA_PARAMS.with(|params| {
            params.borrow_mut().push(
                DrawParam::default()
                    .dest(draw_pos)
                    .scale(Vec2::splat(CRAB_SIZE * crab.scale * 2.8))
                    .color(Color::new(r, g, b, glow_a)),
            );
        });
    }

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
    cone_image: &ggez::graphics::Image,
    screen_width: f32,
    screen_height: f32,
    cam: Vec2,
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

    // The shader's fragment space is the VIEWPORT (uv → [0, screen_width]×[0, screen_height]), but
    // player_pos/center are WORLD coords and the camera can be scrolled far from the origin. Feed
    // the cone centre in viewport space (world centre minus the camera origin) so the lit cone lands
    // on the player wherever the camera is. The mesh body/motes below still draw in world space
    // (world pass), so they keep the raw world `center`.
    let center_view = center - cam;

    // Create uniform data for the shader
    let uniform_data = FlashlightUniform {
        center_x: center_view.x,
        center_y: center_view.y,
        angle,
        spread,
        range: flashlight_len,
        time,
        time_since_catch,
        laser_level: laser_level as f32,
        screen_width,
        screen_height,
    };

    // --- Volumetric dust motes drifting inside the beam ---
    // Drawn BEFORE the custom shader is applied: ggez 0.9.3's set_default_shader() doesn't
    // clear the group-3 shader-params bind group, so any instanced draw after set_shader_params
    // would see a stale incompatible bind group and crash (wgpu validation error). Drawing motes
    // first (while only the default shader is active) avoids the issue entirely.
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);
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
            // Use center_view (viewport coords) since flashlight is drawn in screen space.
            let pos = center_view + Vec2::new(mote_angle.cos(), mote_angle.sin()) * dist;
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
            canvas.draw_instanced_mesh_guarded(unit_circle, instances, DrawParam::default());
        }
        Ok(())
    })?;

    // Render the cone shader to a separate offscreen canvas so the custom shader's group-3 bind
    // never touches the scene canvas. ggez 0.9.3's set_default_shader() doesn't clear
    // shader_bind_group, so any instanced draw on the *same* canvas after set_shader_params
    // crashes with a wgpu bind-group layout mismatch. Isolated render pass = no leak.
    {
        let mut cone_canvas = Canvas::from_image(ctx, cone_image.clone(), Color::from_rgba(0, 0, 0, 0));
        // The flashlight vertex shader expects NDC positions [-1,+1] and outputs them directly as
        // gl_Position. Set screen coordinates to NDC space and draw a NDC-covering quad so the
        // shader receives the correct vertex positions and computes UV correctly.
        cone_canvas.set_screen_coordinates(ggez::graphics::Rect::new(-1.0, -1.0, 2.0, 2.0));
        FLASHLIGHT_SHADER_PARAMS.with(|cell| {
            let mut slot = cell.borrow_mut();
            if let Some(params) = slot.as_mut() {
                params.set_uniforms(ctx, &uniform_data);
            } else {
                *slot = Some(ShaderParamsBuilder::new(&uniform_data).build(ctx));
            }
        });
        FLASHLIGHT_SHADER_PARAMS.with(|cell| {
            if let Some(params) = cell.borrow().as_ref() {
                cone_canvas.set_shader_params(params);
            }
        });
        cone_canvas.set_shader(shader);
        let flashlight_quad = cached_fill_rect(ctx, -1.0, -1.0, 2.0, 2.0, Color::WHITE)?;
        cone_canvas.draw(&flashlight_quad, DrawParam::default());
        cone_canvas.set_default_shader();
        cone_canvas.finish(ctx)?;
    }
    // Composite the cone image at (0,0) — the shader already rendered in full screen-space coords.
    canvas.set_blend_mode(BlendMode::ADD);
    canvas.draw(cone_image, DrawParam::default().dest(Vec2::ZERO));

    // Draw flashlight body at center_view (viewport/screen coords, not world coords).
    canvas.set_blend_mode(original_blend);
    let rotation = dir.y.atan2(dir.x) + std::f32::consts::PI / 2.0;
    let flashlight_body = cached_fill_rect(ctx, -5.0, 0.0, 10.0, 24.0, Color::BLACK)?;
    canvas.draw(
        &flashlight_body,
        DrawParam::default().dest(center_view).rotation(rotation),
    );

    canvas.set_blend_mode(original_blend);
    Ok(())
}

pub fn draw_conga_rope(
    ctx: &mut Context,
    canvas: &mut Canvas,
    player_pos: Vec2,
    // (chain_index, pos, bond_color) tuples, already sorted by chain_index by the caller. The
    // index just rides along because the caller sorts by it before this is called (see
    // CHAIN_SORT_BUF in main.rs). bond_color is Some(type_color) when this link is the same
    // archetype as the link ahead of it — the segment *entering* such a link is tinted and glowed
    // in that color so a run of matching neighbors reads as a persistent colored tether (the
    // visible face of the same-type match-run arrangement mechanic). None = ordinary rainbow rope.
    chain_links: &[(usize, Vec2, Option<[f32; 3]>)],
    time: f32,
    beat_intensity: f32,
    // 0..1 "on fire" factor driven by the live Groove Gamble multiplier: at 0 the rope is its
    // usual rainbow neon; as the risked streak climbs it visibly overheats — wider hotter glow,
    // more energetic wiggle, and the segment colors bleed toward white-hot amber so the reward at
    // stake reads directly on the conga train the player is staring at.
    gamble_heat: f32,
    // 0..1 phase across the current musical bar (0 at the downbeat "1", wrapping back to 0 on the
    // next downbeat). Drives a bright pulse of light that launches from the head on every downbeat
    // and sweeps tail-ward down the whole rope over the bar, so the conga train visibly "feels the
    // beat" as a travelling wave — a legible, watchable rhythm read on top of the rope's own wiggle.
    bar_phase: f32,
    // 0..1 rival-splice threat on THIS train, taken from the same committed-hunt / armed-steal
    // state that already drives the DEFEND ring + early-warning dots (npc hunt_intent / steal_threat).
    // The rope reddens and swells locally around `splice_center_frac` when this rises, so "you're
    // about to be sliced HERE" reads directly on the rope — no new risk logic, just visualizing it.
    splice_risk: f32,
    // 0..1 position along the rope (0 = head, 1 = tail) of the link a rival is targeting — the
    // ~2/3-down thread point the splice aims at, or the tail on a short chain. Centers the heat band.
    splice_center_frac: f32,
) -> ggez::GameResult {
    if chain_links.is_empty() {
        return Ok(());
    }
    let heat = gamble_heat.clamp(0.0, 1.0);
    let risk = splice_risk.clamp(0.0, 1.0);
    // Where along the rope the downbeat pulse currently sits, in link-space (0 = head, total_links
    // = tail). It sweeps the whole train once per bar. The head fraction of the bar is where the
    // flash is brightest; we let it run slightly past the tail so it fully exits rather than
    // lingering, then the next downbeat relaunches it.
    let pulse_head_links = bar_phase * (chain_links.len() as f32 + 2.0);

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
    // "The dominant train dominates": a longer conga's rope reads subtly thicker and brighter, so a
    // big powerful train's tether looks powerful across the field. Ramps from the ~4-link snap
    // threshold up to a long haul (~30 links) and saturates, so it never balloons without bound.
    let length_power = ((total_links - 4.0) / 26.0).clamp(0.0, 1.0);
    // Splice target in link-space: the heat band centers here (the ~2/3-down thread point, or tail).
    let splice_center_links = splice_center_frac.clamp(0.0, 1.0) * total_links;
    // Half-width (in links) of the heated band around the splice point.
    const RISK_BAND: f32 = 3.0;
    // A hot streak whips the rope harder and thicker so it looks like it's straining with energy.
    // Amplitude of the sine-wave wiggle (pixels perpendicular to the link)
    let wiggle_amp = 5.0 + beat_intensity * 8.0 + heat * 5.0;
    // Speed of the wave traveling along the rope (faster on beat, faster still when overheating)
    let wave_speed = 3.5 + beat_intensity * 2.5 + heat * 3.0;
    let thickness = 3.0 + beat_intensity * 4.5 + heat * 2.5 + length_power * 2.5;
    let alpha_base: f32 = (0.55 + beat_intensity * 0.4 + heat * 0.25 + length_power * 0.12).min(1.0);

    // Build the full ordered list of waypoints: player → crab0 → crab1 → …
    let player_center = player_pos + Vec2::new(24.0, 24.0);

    CONGA_WAYPOINT_BUF.with(|wbuf| -> ggez::GameResult {
        let mut waypoints = wbuf.borrow_mut();
        waypoints.clear();
        waypoints.push(player_center);
        for &(_, pos, _) in chain_links {
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

                // Same-type match bond: the segment entering link `link_idx` (the window's `end`)
                // corresponds to chain_links[link_idx] (waypoints[0] is the player, so link i lives
                // at waypoints[i+1] = window end of segment i). If that link carries a bond color, the
                // whole segment is pulled toward it and pulsed so the matched pair reads as a glowing
                // colored tether — a longer same-type run makes a longer continuous glow.
                let bond = chain_links.get(link_idx).and_then(|&(_, _, b)| b);
                // Gentle pulse so the bond looks alive rather than a flat recolor.
                let bond_pulse = 0.7 + 0.3 * (time * 4.0 + link_idx as f32 * 0.7).sin();

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
                        // Downbeat pulse: a bright crest that launched from the head on the last
                        // downbeat and is sweeping tail-ward. `along` is this micro-segment's
                        // position down the rope in link units; when the travelling pulse head is
                        // within a link or so of it, flash it toward white so a band of light rides
                        // the whole train once per bar. Falls off smoothly on both sides so it reads
                        // as a moving crest, not a hard edge.
                        let along = link_idx as f32 + t;
                        let d = (along - pulse_head_links).abs();
                        let pulse = (1.0 - d / 1.1).max(0.0);
                        if pulse > 0.0 {
                            let p = pulse * pulse; // sharpen the crest
                            rr = rr + (1.0 - rr) * p;
                            gg = gg + (1.0 - gg) * p;
                            bb = bb + (1.0 - bb) * p;
                        }
                        // Matched same-type bond: blend this micro-segment strongly toward the run's
                        // archetype color, pulsing, so the tether reads as "these links belong
                        // together". Applied on top of heat so a hot matched run still glows amber-lit.
                        if let Some(bc) = bond {
                            let mix = 0.72 * bond_pulse;
                            rr = rr + (bc[0] - rr) * mix;
                            gg = gg + (bc[1] - gg) * mix;
                            bb = bb + (bc[2] - bb) * mix;
                        }

                        // Rope heat — the legible-risk read. Where a rival is committed to slicing
                        // (splice_risk, from the live hunt_intent / armed steal_threat), the band of
                        // rope around the targeted link (splice_center_links) glows angry orange-red
                        // and physically swells. It throbs on the beat so the danger pulses like a
                        // strained tendon rather than sitting as a flat stain, and falls off smoothly
                        // to either side so it reads as "sliced HERE" — the same 2/3-down thread point
                        // the splice actually aims at. Applied last so heat wins over rainbow/bond.
                        let mut seg_thick_mult = 1.0;
                        if risk > 0.0 {
                            let dr = (along - splice_center_links).abs();
                            let band = (1.0 - dr / RISK_BAND).max(0.0);
                            if band > 0.0 {
                                let throb = 0.72 + 0.28 * (time * 9.0).sin();
                                let hot = (risk * band * band * throb).clamp(0.0, 1.0);
                                rr += (1.0 - rr) * hot;
                                gg += (0.24 - gg) * hot;
                                bb += (0.08 - bb) * hot;
                                seg_thick_mult += hot * 0.9; // the endangered body bulges
                            }
                        }

                        let seg_delta = point - prev_point;
                        let seg_len = seg_delta.length();
                        if seg_len > 0.5 {
                            let seg_angle = seg_delta.y.atan2(seg_delta.x);
                            seg_buf.push((prev_point, seg_angle, seg_len, [rr, gg, bb], seg_thick_mult));
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
                instances.set(seg_buf.iter().map(|&(pos, angle, len, rgb, tmult)| {
                    let color = Color::new(rgb[0], rgb[1], rgb[2], alpha_base);
                    DrawParam::default()
                        .dest(pos)
                        .rotation(angle)
                        .scale(Vec2::new(len, thickness * tmult))
                        .color(color)
                }));
                canvas.draw_instanced_mesh_guarded(unit_line.clone(), instances, DrawParam::default());
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
                instances.set(seg_buf.iter().map(|&(pos, angle, len, rgb, tmult)| {
                    let glow_color = Color::new(rgb[0], rgb[1], rgb[2], glow_alpha);
                    DrawParam::default()
                        .dest(pos)
                        .rotation(angle)
                        .scale(Vec2::new(len, glow_width * tmult))
                        .color(glow_color)
                }));
                canvas.draw_instanced_mesh_guarded(unit_line.clone(), instances, DrawParam::default());
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
    // Which beat of the current 4/4 bar is sounding (0..=3, 0 = the downbeat). Drives the bar-position
    // pip row and the extra downbeat punch, so the player can read *where* in the bar they are — the
    // "it's not obvious what you're timing" legibility gap (#164) — and feels beat 1 land like the fill
    // it is ("downbeats are the biggest moment", INSPIRATION.md).
    beat_in_bar: usize,
    _time: f32,
) -> ggez::GameResult {
    let is_downbeat = beat_in_bar % 4 == 0;
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
    // The downbeat's approach ring is drawn thicker so bar 1 reads as the heavier beat even before
    // it lands — the eye catches the fatter ring closing in and knows "the big one is coming".
    let ring_w = if is_downbeat { 3.5 } else { 2.5 };
    let approach = cached_stroke_circle(ctx, cache_r, ring_w)?;
    canvas.draw(&approach, DrawParam::default().dest(center).color(ring_col));

    let pulse_r = base_r + beat_intensity * 14.0;
    // The downbeat punches ~35% bigger and flashes white-hot on the hit, so beat 1 feels like the
    // fill it is rather than one of four identical ticks. Off-beat 2/3/4 keep the normal size/colour.
    let downbeat_hit = is_downbeat && on_beat;
    let pulse_r = if downbeat_hit { pulse_r * 1.35 } else { pulse_r };
    let alpha = ((80.0 + beat_intensity * 175.0) as u8).min(255);
    // The marker flashes green in the on-beat window (white-hot on the downbeat), otherwise warm amber.
    let marker_col = if downbeat_hit {
        Color::from_rgba(230, 255, 210, 255)
    } else if on_beat {
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

    // Bar-position tracker: four pips under the marker showing which beat of the 4/4 bar is sounding,
    // so the beat clock reads as "1 · 2 · 3 · 4" instead of an undifferentiated pulse. This is the
    // legibility half of #164 ("not obvious what you're timing") and the groundwork for #165's
    // "tap on beats 1/2/3/4": the downbeat pip (0) is drawn larger and gold so the bar's "1" is always
    // findable, and the pip for the beat sounding now brightens/rings so you can read your place at a
    // glance. Reuses the already-fetched unit circle + shared stroke-circle cache — no per-frame mesh.
    let pip_spacing = 13.0;
    let pip_y = center.y + base_r + 20.0;
    let pip_start_x = center.x - pip_spacing * 1.5;
    for i in 0..4 {
        let pip = Vec2::new(pip_start_x + pip_spacing * i as f32, pip_y);
        let is_here = i == beat_in_bar % 4;
        let is_one = i == 0;
        // Base size: the downbeat pip sits a touch larger so "1" anchors the row; the active beat
        // swells and (on-beat) blooms so the moving playhead is unmistakable.
        let r = if is_one { 4.2 } else { 3.2 }
            + if is_here { 2.6 } else { 0.0 }
            + if is_here && on_beat { 1.8 } else { 0.0 };
        let col = if is_here && on_beat {
            // Active beat landed on-time: green (white-hot on the downbeat), matching the marker.
            if is_one {
                Color::from_rgba(230, 255, 210, 255)
            } else {
                Color::from_rgba(150, 255, 160, 255)
            }
        } else if is_here {
            // Sounding now but between windows — bright amber cursor.
            Color::from_rgba(255, 210, 90, 235)
        } else if is_one {
            // Idle downbeat pip — dim gold so the bar's "1" is still readable when it's not playing.
            Color::from_rgba(210, 170, 70, 150)
        } else {
            // Idle off-beat pip — a faint dot.
            Color::from_rgba(150, 140, 130, 120)
        };
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(pip)
                .scale(Vec2::splat(r))
                .color(col),
        );
    }
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
    if combo_count < 3 {
        return Ok(());
    }

    // Determine multiplier tier (0=x2, 1=x3, 2=x5) for the label cache index.
    let (tier_idx, multiplier_label, tier_color) = if combo_count >= 10 {
        (2usize, "x5", Color::new(0.8, 0.3, 1.0, 1.0))
    } else if combo_count >= 6 {
        (1usize, "x3", Color::new(1.0, 0.2, 0.2, 1.0))
    } else {
        (0usize, "x2", Color::new(1.0, 0.6, 0.1, 1.0))
    };

    let center = player_pos + Vec2::new(player_size / 2.0, player_size / 2.0);
    let radius = 36.0 + beat_intensity * 8.0;
    let fill_fraction = (combo_timer / 1.8).clamp(0.0, 1.0);
    let rotation_offset = time * 0.5;

    const SEGMENTS: usize = 32;
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    // Reuse the cached unit-line mesh for all arc segments, same as the conga rope and catch
    // trails — no per-segment GPU buffer allocation.
    let line = unit_line(ctx)?.clone();

    // Build both arc passes into scratch DrawParam buffers, then flush each as a single
    // draw_instanced_mesh call. The combo meter draws up to 32 segments per pass; the old
    // per-segment canvas.draw() loop was up to 64 GPU submissions a frame while a combo was
    // live (most of active play). Two instanced draws is the same technique already used for
    // particles/legs/bodies/rope/trails/marchers/radar.
    let glow_radius = radius + 5.0;
    let glow_color = Color::new(tier_color.r, tier_color.g, tier_color.b, tier_color.a * 0.35);

    COMBO_ARC_MAIN_PARAMS.with(|main_cell| -> ggez::GameResult {
        COMBO_ARC_GLOW_PARAMS.with(|glow_cell| -> ggez::GameResult {
            let mut main_params = main_cell.borrow_mut();
            let mut glow_params = glow_cell.borrow_mut();
            main_params.clear();
            glow_params.clear();

            for i in 0..SEGMENTS {
                let t0 = i as f32 / SEGMENTS as f32;
                let t1 = (i + 1) as f32 / SEGMENTS as f32;
                if t0 >= fill_fraction {
                    break;
                }
                let angle0 = rotation_offset + t0 * fill_fraction * std::f32::consts::TAU;
                let angle1 = rotation_offset + t1.min(fill_fraction) * fill_fraction * std::f32::consts::TAU;

                // Main arc segment
                let p0 = center + Vec2::new(angle0.cos(), angle0.sin()) * radius;
                let p1 = center + Vec2::new(angle1.cos(), angle1.sin()) * radius;
                let d = p0.distance(p1);
                if d > 0.5 {
                    let rot = ((p1 - p0) / d);
                    main_params.push(
                        DrawParam::default()
                            .dest(p0)
                            .rotation(rot.y.atan2(rot.x))
                            .scale(Vec2::new(d, 3.0))
                            .color(tier_color),
                    );
                }

                // Glow arc segment (slightly larger radius, softer alpha)
                let g0 = center + Vec2::new(angle0.cos(), angle0.sin()) * glow_radius;
                let g1 = center + Vec2::new(angle1.cos(), angle1.sin()) * glow_radius;
                let dg = g0.distance(g1);
                if dg > 0.5 {
                    let grot = (g1 - g0) / dg;
                    glow_params.push(
                        DrawParam::default()
                            .dest(g0)
                            .rotation(grot.y.atan2(grot.x))
                            .scale(Vec2::new(dg, 6.0))
                            .color(glow_color),
                    );
                }
            }

            if !main_params.is_empty() {
                COMBO_ARC_MAIN_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                    let mut inst_slot = inst_cell.borrow_mut();
                    let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                    instances.set(main_params.iter().copied());
                    canvas.draw_instanced_mesh_guarded(line.clone(), instances, DrawParam::default());
                    Ok(())
                })?;
            }
            if !glow_params.is_empty() {
                COMBO_ARC_GLOW_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                    let mut inst_slot = inst_cell.borrow_mut();
                    let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                    instances.set(glow_params.iter().copied());
                    canvas.draw_instanced_mesh_guarded(line, instances, DrawParam::default());
                    Ok(())
                })?;
            }
            Ok(())
        })
    })?;

    canvas.set_blend_mode(original_blend);

    // Draw multiplier label above the player. The label is one of three fixed strings ("x2",
    // "x3", "x5") that never change for a given tier, so cache the built Text (glyph shaping
    // runs once per tier per session) and reuse it forever — same pattern as the other HUD label
    // caches (FRENZY_BANNER_CACHE, GROOVE_LABEL_CACHE, etc.).
    let text_alpha = (0.7 + 0.3 * beat_intensity).clamp(0.0, 1.0);
    let text_color = Color::new(tier_color.r, tier_color.g, tier_color.b, text_alpha);
    let text_pos = center - Vec2::new(14.0, radius + 20.0);
    COMBO_LABEL_CACHE.with(|cache_cell| -> ggez::GameResult {
        let mut cache = cache_cell.borrow_mut();
        let label = cache[tier_idx].get_or_insert_with(|| {
            let mut t = Text::new(multiplier_label);
            t.set_scale(22.0);
            t
        });
        canvas.draw(label, DrawParam::default().dest(text_pos).color(text_color));
        Ok(())
    })
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
    cam: Vec2,
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
                // Crab positions are world-space; the radar draws in screen space (HUD pass), so
                // translate by the camera origin to get the crab's position within the viewport.
                // Only show arrow if crab is near an edge (within margin*5) or fully off-screen.
                let cx = crab.pos.x - cam.x;
                let cy = crab.pos.y - cam.y;
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
                    canvas.draw_instanced_mesh_guarded(triangle.clone(), instances, DrawParam::default());
                    Ok(())
                })?;
                RADAR_GLOW_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                    let mut inst_slot = inst_cell.borrow_mut();
                    let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                    instances.set(glow_params.iter().copied());
                    canvas.draw_instanced_mesh_guarded(triangle.clone(), instances, DrawParam::default());
                    Ok(())
                })?;
            }
            Ok(())
        })
    })?;

    canvas.set_blend_mode(original_blend);
    Ok(())
}

/// Which beat of the lasso throw a frame is rendering — mirrors main's `LassoPhase` but is the
/// draw-side view (Idle never reaches here). Lets `draw_lasso` give each beat its own read:
/// a spinning loop stretching outward (Throw), a hard tightening squeeze-pop (Snag), a taut
/// straining rope reeling the haul home (Drag), and an empty loop flattening into the sand (Miss).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LassoDrawPhase {
    Throw,
    Snag,
    Drag,
    Miss,
}

/// Draw the thrown lasso for the given phase. `phase_t` is 0..1 progress through the *current*
/// phase and `spin` is the loop's spin in radians. All geometry reuses cached meshes
/// (`UNIT_LINE`/`UNIT_CIRCLE` scaled via `DrawParam`, plus the stroke-circle and lasso-loop caches)
/// rather than allocating fresh GPU buffers each frame — the lasso is thrown on nearly every catch
/// attempt, so this stays hot.
pub fn draw_lasso(
    ctx: &mut Context,
    canvas: &mut Canvas,
    player_center: Vec2,
    tip: Vec2,
    phase: LassoDrawPhase,
    phase_t: f32,
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
    let unit_circle = match UNIT_CIRCLE.get() {
        Some(mesh) => mesh,
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh)
        }
    };

    // Rope tension: the reel-in phase strains the rope taut and bright; a miss lets it go slack.
    let (rope_thick, rope_bright): (f32, u8) = match phase {
        LassoDrawPhase::Drag => (3.6, 245),   // straining under the weight of the haul
        LassoDrawPhase::Snag => (3.2, 240),
        LassoDrawPhase::Throw => (2.5, 220),
        LassoDrawPhase::Miss => (1.6, 120),   // gone limp
    };
    let rope_delta = tip - player_center;
    let rope_len = rope_delta.length();
    if rope_len > 1.0 {
        let rope_angle = rope_delta.y.atan2(rope_delta.x);
        let orig_blend = canvas.blend_mode();
        canvas.set_blend_mode(BlendMode::ADD);
        canvas.draw(
            unit_line,
            DrawParam::default()
                .dest(player_center)
                .rotation(rope_angle)
                .scale(Vec2::new(rope_len, rope_thick + 3.5))
                .color(Color::from_rgba(230, 160, 30, 60)),
        );
        canvas.set_blend_mode(orig_blend);
        canvas.draw(
            unit_line,
            DrawParam::default()
                .dest(player_center)
                .rotation(rope_angle)
                .scale(Vec2::new(rope_len, rope_thick))
                .color(Color::from_rgba(220, 160, 50, rope_bright)),
        );
    }

    // Catch-radius indicator ring: only meaningful while the loop is still flying out to show
    // where it will bite. Fades in as the throw extends, gone once it lands.
    if phase == LassoDrawPhase::Throw {
        let catch_r = 60.0_f32;
        let ring_alpha = (phase_t * 80.0) as u8;
        if ring_alpha > 4 {
            let catch_ring = cached_stroke_circle(ctx, catch_r, 1.5)?;
            canvas.draw(
                &catch_ring,
                DrawParam::default()
                    .dest(tip)
                    .color(Color::from_rgba(255, 220, 80, ring_alpha)),
            );
        }
    }

    // The spinning open loop (noose). Its radius tells the story of the throw:
    //  - Throw: grows a touch as it flies out.
    //  - Snag: SNAPS shut fast — the tightening squeeze — then a bright pop flash over the knot.
    //  - Drag: stays cinched small around the haul, quivering slightly under tension.
    //  - Miss: flattens/expands and fades as it flops empty onto the sand.
    let (loop_r, loop_alpha, loop_glow_alpha): (f32, u8, u8) = match phase {
        LassoDrawPhase::Throw => (18.0 + phase_t * 6.0, 230, 80),
        LassoDrawPhase::Snag => {
            // Ease the loop from ~24 down to ~11 as it bites shut.
            let r = 24.0 - phase_t * 13.0;
            (r, 240, 150)
        }
        LassoDrawPhase::Drag => {
            let quiver = (phase_t * 40.0).sin() * 0.8;
            (11.0 + quiver, 230, 90)
        }
        LassoDrawPhase::Miss => {
            // Open out and fade — a spent loop settling.
            let a = ((1.0 - phase_t) * 200.0) as u8;
            (20.0 + phase_t * 10.0, a, (a as f32 * 0.4) as u8)
        }
    };
    if loop_alpha > 4 {
        let loop_glow = cached_lasso_loop(ctx, loop_r, 8.0)?;
        let orig_blend = canvas.blend_mode();
        canvas.set_blend_mode(BlendMode::ADD);
        canvas.draw(
            &loop_glow,
            DrawParam::default()
                .dest(tip)
                .rotation(spin)
                .color(Color::from_rgba(255, 200, 60, loop_glow_alpha)),
        );
        canvas.set_blend_mode(orig_blend);
        let loop_line = cached_lasso_loop(ctx, loop_r, 3.5)?;
        canvas.draw(
            &loop_line,
            DrawParam::default()
                .dest(tip)
                .rotation(spin)
                .color(Color::from_rgba(255, 210, 70, loop_alpha)),
        );
    }

    // Snag pop: a bright expanding flash the instant the loop bites, so a catch reads as a distinct
    // "gotcha!" beat rather than the loop just shrinking.
    if phase == LassoDrawPhase::Snag {
        let orig_blend = canvas.blend_mode();
        canvas.set_blend_mode(BlendMode::ADD);
        let pop_r = 6.0 + phase_t * 26.0;
        let pop_a = ((1.0 - phase_t) * 200.0) as u8;
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(tip)
                .scale(Vec2::splat(pop_r))
                .color(Color::from_rgba(255, 240, 170, pop_a / 3)),
        );
        canvas.set_blend_mode(orig_blend);
    }

    // Bright center dot at the tip knot — swells on the snag pop, steady otherwise.
    let knot_scale = if phase == LassoDrawPhase::Snag { 5.0 + (1.0 - phase_t) * 5.0 } else { 5.0 };
    let knot_alpha = if phase == LassoDrawPhase::Miss { ((1.0 - phase_t) * 240.0) as u8 } else { 240 };
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(tip)
            .scale(Vec2::splat(knot_scale))
            .color(Color::from_rgba(255, 240, 160, knot_alpha)),
    );

    Ok(())
}

/// Draw the lasso wind-up animation while the player is holding the mouse button.
///
/// `charge_frac` is 0..1 (how full the charge is), `beat_prox` is 0..1 (closeness to the next
/// beat — 1 at the exact beat edge). The rope loop spins above the player, growing as charge
/// builds and pulsing brighter on each beat so the player can time the release.
pub fn draw_lasso_windup(
    ctx: &mut Context,
    canvas: &mut Canvas,
    player_center: Vec2,
    charge_frac: f32,
    beat_prox: f32,
    spin: f32,
) -> ggez::GameResult {
    let unit_circle = match UNIT_CIRCLE.get() {
        Some(mesh) => mesh,
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh)
        }
    };

    // Loop radius grows from ~14 up to ~38 as charge builds.
    let loop_r = 14.0 + charge_frac * 24.0;
    // Vertical hover offset: the loop circles above the player, not on top of it.
    let hover = Vec2::new(0.0, -(22.0 + charge_frac * 14.0));
    let loop_center = player_center + hover;

    // Spin the loop: use the accumulated spin angle. Spins faster as charge builds.
    // (spin is driven by the update loop)

    // Beat pulse: alpha spikes toward 255 near the beat so "time your release" reads.
    let base_alpha = (120.0 + charge_frac * 100.0) as u8;
    let pulse_alpha = (base_alpha as f32 + beat_prox * 80.0).min(255.0) as u8;
    let glow_alpha = (beat_prox * 60.0 + charge_frac * 30.0).min(100.0) as u8;

    // Glow layer (additive).
    let orig_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);
    if glow_alpha > 4 {
        let loop_glow = cached_lasso_loop(ctx, loop_r, 10.0)?;
        canvas.draw(
            &loop_glow,
            DrawParam::default()
                .dest(loop_center)
                .rotation(spin)
                .color(Color::from_rgba(255, 200, 60, glow_alpha)),
        );
    }
    canvas.set_blend_mode(orig_blend);

    // Main loop line.
    if pulse_alpha > 4 {
        let loop_line = cached_lasso_loop(ctx, loop_r, 3.5)?;
        canvas.draw(
            &loop_line,
            DrawParam::default()
                .dest(loop_center)
                .rotation(spin)
                .color(Color::from_rgba(255, 210, 70, pulse_alpha)),
        );
    }

    // Dot at the knot.
    let knot_alpha = pulse_alpha;
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(loop_center)
            .scale(Vec2::splat(4.5))
            .color(Color::from_rgba(255, 240, 160, knot_alpha)),
    );

    // Charge fill arc underneath the loop: shows how much is loaded (thin arc that grows as charge
    // accumulates, so a glance down tells you "almost full / half-loaded / quick tap").
    if charge_frac > 0.03 {
        let segs = 32usize;
        let filled = ((segs as f32) * charge_frac).ceil().max(1.0) as usize;
        let arc = cached_stroke_arc(ctx, loop_r + 7.0, 2.5, segs, filled)?;
        let arc_a = (60.0 + charge_frac * 140.0 + beat_prox * 40.0).min(220.0) as u8;
        canvas.draw(
            &arc,
            DrawParam::default()
                .dest(loop_center)
                .color(Color::from_rgba(255, 230, 80, arc_a)),
        );
    }

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

    // Faint full track so the drained portion still reads as progress. Deferred into
    // ARCHETYPE_RING_GROUPS via defer_archetype_ring() instead of an immediate canvas.draw() —
    // same batching as draw_thief_aura/draw_splitter_aura/draw_golden_sparkle above, so multiple
    // Armored crabs' tracks (a fixed radius/thickness pair) collapse into one GPU submission. The
    // health arc below stays immediate: its mesh varies per-crab with the live shell fraction, so
    // it rarely shares a bucket with another crab's arc and batching it wouldn't collapse draws.
    defer_archetype_ring(ctx, pos, radius, 3.0, Color::new(0.0, 0.0, 0.0, 0.35))?;

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

/// Draw a Hermit crab's borrowed-shell indicator — a warm coppery coiled shell, visually distinct
/// from the Armored crab's cold steely arc so the player reads at a glance "this shell the beam
/// won't crack; use a Stomp, a Dancer's hop, or a Magnet". The shell depletes as it's chipped, and
/// a slow-rotating coil of dots reads as the spiral of a borrowed conch shell.
pub fn draw_hermit_shell(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    size: f32,
    shell_frac: f32,
    time: f32,
) -> ggez::GameResult {
    let radius = size * 0.82;
    let pulse = (time * 4.0).sin() * 0.5 + 0.5;
    let frac = shell_frac.clamp(0.0, 1.0);

    // Faint full track so the chipped-away portion still reads as progress.
    let track = cached_stroke_circle(ctx, radius, 3.0)?;
    canvas.draw(
        &track,
        DrawParam::default()
            .dest(pos)
            .color(Color::new(0.0, 0.0, 0.0, 0.32)),
    );

    // Depleting coppery arc — the remaining shell.
    let segs = 40usize;
    let filled = ((segs as f32) * frac).ceil().max(1.0) as usize;
    let arc = cached_stroke_arc(ctx, radius, 3.5, segs, filled)?;
    canvas.draw(
        &arc,
        DrawParam::default()
            .dest(pos)
            .color(Color::new(0.85, 0.55, 0.28, 0.82 + pulse * 0.18)),
    );

    // A slow-turning spiral of little coil dots inside the ring — the borrowed-shell whorl. Defers
    // each dot's DrawParam into HERMIT_COIL_PARAMS (same pattern as GOLDEN_SPARKLE_PARAMS) so all
    // hermit coil dots across every shelled Hermit on screen flush as one draw_instanced_mesh call
    // in flush_hermit_coil_dots() instead of up to 5 individual canvas.draw() calls per crab.
    let coil_dots = 5usize;
    let shown = ((coil_dots as f32) * frac).ceil().max(1.0) as usize;
    HERMIT_COIL_PARAMS.with(|params_cell| {
        let mut params = params_cell.borrow_mut();
        for k in 0..shown {
            let f = k as f32 / coil_dots as f32;
            // Tightening spiral: angle winds faster than one turn, radius shrinks toward the center.
            let ang = time * 1.2 + f * std::f32::consts::TAU * 1.6;
            let rr = radius * (0.62 - f * 0.42);
            let d = pos + Vec2::new(ang.cos(), ang.sin()) * rr;
            let dot_r = (2.6 - f * 1.2).max(1.0);
            params.push(
                DrawParam::default()
                    .dest(d)
                    .scale(Vec2::splat(dot_r))
                    .color(Color::new(0.95, 0.68, 0.38, 0.7)),
            );
        }
    });
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
    // This is now the SCORCH ring drawn only on a shelled target the beam is burning down (see the
    // gated call site in draw_crabs). It reads as a searing hot-spot on the shell, not a soft lure
    // halo. Fast, jittery flicker (like a flame biting the shell) instead of a lazy breathing pulse.
    let flicker = (time * 6.0 * std::f32::consts::TAU).sin() * 0.5 + 0.5;
    let flicker2 = (time * 13.0 * std::f32::consts::TAU).sin() * 0.5 + 0.5;
    let pulse = (flicker * 0.7 + flicker2 * 0.3) * (0.75 + beat_intensity * 0.25); // 0..1, twitchy

    let base_radius = size * 0.85;
    let outer_radius = base_radius + 4.0 + pulse * 7.0;

    // Harsh white-yellow scorch (ignore the passed crab_color's hue for saturation; the caller
    // passes a hot color, but clamp it toward white-hot so the burn always reads as searing).
    let [r, g, b] = crab_color;

    // Additively blended — the caller (draw_crabs_with_shake) already has the canvas in ADD
    // mode for this whole per-crab aura pass, so this doesn't toggle blend mode itself; see the
    // comment there for why (per-crab toggling used to cause a GPU pipeline switch per crab).

    // Outer soft glow ring and inner bright ring — deferred into per-key scratch maps and
    // flushed as a couple of instanced batches by flush_attracted_crab_glows() after the
    // per-crab aura loop. Replaces 2 individual canvas.draw() calls per attracted crab with
    // one grouped submission per distinct stroke-circle key bucket. Meshes are still built
    // (or cache-hit) here so the key → mesh association stays consistent.
    let glow_alpha = (0.18 + pulse * 0.22).clamp(0.0, 1.0);
    let glow_r = outer_radius + outer_radius * 0.18;
    let glow_th = outer_radius * 0.35;
    let glow_key = stroke_circle_key(glow_r, glow_th);
    cached_stroke_circle(ctx, glow_r, glow_th)?;
    ATTRACTED_GLOW_GROUPS.with(|groups_cell| {
        let mut groups = groups_cell.borrow_mut();
        groups.entry(glow_key).or_default().push(
            DrawParam::default()
                .dest(pos)
                .color(Color::new(r, g, b, glow_alpha)),
        );
    });

    let ring_alpha = (0.45 + pulse * 0.45).clamp(0.0, 1.0);
    let ring_key = stroke_circle_key(outer_radius, 2.5);
    cached_stroke_circle(ctx, outer_radius, 2.5)?;
    ATTRACTED_RING_GROUPS.with(|groups_cell| {
        let mut groups = groups_cell.borrow_mut();
        groups.entry(ring_key).or_default().push(
            DrawParam::default()
                .dest(pos)
                .color(Color::new(
                    (r * 0.5 + 0.5).min(1.0),
                    (g * 0.5 + 0.5).min(1.0),
                    (b * 0.5 + 0.5).min(1.0),
                    ring_alpha,
                )),
        );
    });

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
    //
    // These rings sweep over a ~215px radius range (ring_radius → inner). The shared
    // stroke-circle cache uses 2px buckets, which would generate ~108 distinct mesh keys per
    // ring per sweep cycle — with multiple Magnets on screen this easily pushes past the 512-entry
    // cap, evicting every other cached ring (chain ghosts, auras, shockwaves) and forcing full
    // rebuilds. Round to 8px buckets here instead: visually indistinguishable at these radii
    // (the sweep is a fluid animation, not a precise size) but reduces key count to ~27 per ring
    // per sweep, keeping the cache far below the cap even with several Magnets in play.
    // Defer all sweep rings and the core into MAGNET_AURA_RING_PARAMS so flush_magnet_auras()
    // can batch all Magnets' rings together by mesh key. In the Water biome (now Magnet-heavy)
    // this collapses N×3 individual ADD-blend draw calls for the sweep rings into at most 3
    // batched draw_instanced_mesh calls, regardless of how many Magnets are on screen.
    MAGNET_AURA_RING_PARAMS.with(|params_cell| -> ggez::GameResult {
        let mut params = params_cell.borrow_mut();
        for k in 0..3u32 {
            let phase = ((time * sweep_speed + k as f32 / 3.0) % 1.0) as f32;
            let radius = ring_radius - (ring_radius - inner) * phase;
            let alpha = (phase * alpha_scale).clamp(0.0, alpha_scale);
            // Snap to 8px bucket — same quantization already in place; ensures rings from
            // different Magnets at the same sweep phase share the same mesh key and can be
            // instanced together.
            let radius_q = ((radius / 8.0).round() * 8.0).max(0.5);
            // Ensure the mesh exists in the cache (cached_stroke_circle builds it if absent).
            cached_stroke_circle(ctx, radius_q, 2.0)?;
            let key = stroke_circle_key(radius_q, 2.0);
            params.push((key, DrawParam::default().dest(pos).color(Color::new(r, g, b, alpha))));
        }

        // Core ring — deferred into the same batch. Core radii vary per crab size so they
        // won't collapse across multiple Magnets as aggressively as the sweep rings, but they're
        // still one fewer canvas.draw() call per Magnet on the hot path.
        let core_pulse = (time * 4.0).sin() * 0.5 + 0.5;
        let core_r = inner + 4.0 + core_pulse * 4.0;
        cached_stroke_circle(ctx, core_r, 2.5)?;
        let core_key = stroke_circle_key(core_r, 2.5);
        let core_g = if charged || lured { 0.8 } else { 0.55 } + core_pulse * 0.2;
        let core_b_val = if charged { 0.4 } else if lured { 0.35 } else { 0.3 };
        params.push((core_key, DrawParam::default().dest(pos).color(Color::new(1.0, core_g, core_b_val, 0.55))));
        Ok(())
    })?;

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

    // Each branch used to build its own stroke-circle mesh and issue an immediate canvas.draw()
    // per Thief per frame. Deferred into ARCHETYPE_RING_GROUPS via defer_archetype_ring() instead,
    // so multiple Thieves in the same state on screen collapse into one GPU submission per shared
    // radius bucket (flushed by flush_archetype_rings() after the per-crab aura pass) — identical
    // rings, just batched.
    if latched {
        // Actively gnawing: a fast, bright, slightly jittering double ring so the theft screams
        // for attention. The jitter fakes the crab tearing at the link.
        let pulse = (time * 18.0).sin() * 0.5 + 0.5;
        let jitter = (time * 40.0).sin() * 2.5;
        defer_archetype_ring(
            ctx,
            pos,
            size * 0.9 + 3.0 + jitter,
            3.0,
            Color::new(r, g, b, 0.5 + pulse * 0.4),
        )?;
        defer_archetype_ring(
            ctx,
            pos,
            size * 1.25 + pulse * 6.0,
            2.0,
            Color::new(0.6, 1.0, 0.5, 0.25 + pulse * 0.25),
        )?;
    } else if snared {
        // Intercepted by a Magnet: a brighter, faster orange ring that reads as "the field's got
        // it" — livelier than the calm prowl so the save is legible, calmer than the theft frenzy.
        let pulse = (time * 9.0).sin() * 0.5 + 0.5;
        defer_archetype_ring(
            ctx,
            pos,
            size * 0.9 + 3.0 + pulse * 4.0,
            2.5,
            Color::new(r, g, b, 0.45 + pulse * 0.3),
        )?;
    } else if lured {
        // Lured off your tail by a Golden's shine: a brisk, brighter golden-green ring — livelier
        // than the calm prowl so the divert reads as the raider actively chasing the prize.
        let pulse = (time * 7.0).sin() * 0.5 + 0.5;
        defer_archetype_ring(
            ctx,
            pos,
            size * 0.9 + 3.0 + pulse * 4.0,
            2.5,
            Color::new(r, g, b, 0.4 + pulse * 0.3),
        )?;
    } else {
        // Prowling: a steady soft ring that just marks it out, calmer than the latched frenzy.
        let pulse = (time * 3.0).sin() * 0.5 + 0.5;
        defer_archetype_ring(
            ctx,
            pos,
            size * 0.85 + 3.0 + pulse * 3.0,
            2.0,
            Color::new(r, g, b, 0.35 + pulse * 0.2),
        )?;
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
    // Both rings deferred into ARCHETYPE_RING_GROUPS via defer_archetype_ring() instead of an
    // immediate canvas.draw() — same batching as draw_thief_aura above, so multiple Goldens'
    // halos/tethers collapse into shared GPU submissions (flushed by flush_archetype_rings()).
    let pulse = (time * 4.0).sin() * 0.5 + 0.5;
    let (hg, hb) = if snared { (0.6, 0.15) } else { (0.85, 0.3) };
    defer_archetype_ring(
        ctx,
        pos,
        size * 0.8 + 3.0 + pulse * 4.0,
        2.5,
        Color::new(1.0, hg, hb, 0.35 + pulse * 0.3),
    )?;

    // While snared, a fast-spinning tether ring cinches in tight around the crab — the visual of
    // the field clamping the prize in place, drawing the eye to "grab it NOW".
    if snared {
        let cinch = 0.5 + 0.5 * (time * 12.0).sin();
        defer_archetype_ring(
            ctx,
            pos,
            size * 0.55 + 2.0 + cinch * 3.0,
            3.0,
            Color::new(1.0, 0.6, 0.15, 0.55 + cinch * 0.35),
        )?;
    }

    // A ring of sparkle dots orbiting the crab, each twinkling on its own phase so the whole thing
    // shimmers like a coin catching the light. Snared, the orbit pulls in tighter and spins faster,
    // like filings dragged onto the lodestone.
    // Instead of issuing 5 individual canvas.draw() calls here (one per dot), push each dot's
    // DrawParam into GOLDEN_SPARKLE_PARAMS and let flush_golden_sparkles() drain them all as one
    // instanced batch after every crab's aura pass — identical output, one GPU submission total.
    const SPARKLES: usize = 5;
    let orbit = if snared { size * 0.55 + 4.0 } else { size * 0.75 + 6.0 };
    let spin = if snared { 3.4 } else { 1.6 };
    GOLDEN_SPARKLE_PARAMS.with(|params_cell| {
        let mut params = params_cell.borrow_mut();
        let (sg, sb) = if snared { (0.75, 0.35) } else { (0.95, 0.55) };
        for i in 0..SPARKLES {
            let base = i as f32 / SPARKLES as f32 * std::f32::consts::TAU;
            let ang = base + time * spin;
            let twinkle = ((time * 6.0 + i as f32 * 1.7).sin() * 0.5 + 0.5).powf(2.0);
            let dpos = pos + Vec2::new(ang.cos(), ang.sin()) * orbit;
            let r = 1.5 + twinkle * 2.5;
            params.push(
                DrawParam::default()
                    .dest(dpos)
                    .scale(Vec2::splat(r))
                    .color(Color::new(1.0, sg, sb, 0.4 + twinkle * 0.6)),
            );
        }
    });

    Ok(())
}

/// Splitter crab aura — a bright teal ring that pulses open into two halves, telegraphing that
/// catching this one cleaves your train in two. Two short arcs sweep apart on opposite sides of a
/// vertical "cleave line" so the split reads at a glance, distinct from every other archetype aura.
/// Additively blended; the caller (the per-crab aura pass) already has the canvas in ADD mode, so
/// this doesn't toggle blend mode itself. Reuses cached meshes so no fresh GPU buffers are uploaded.
pub fn draw_splitter_aura(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    size: f32,
    time: f32,
    beat_prox: f32,
) -> ggez::GameResult {
    // Breathing halo so the cleaver reads even while it's holding still — teal, the archetype tint.
    // Deferred into ARCHETYPE_RING_GROUPS via defer_archetype_ring() instead of an immediate
    // canvas.draw(), same batching as draw_thief_aura/draw_golden_sparkle above.
    let pulse = (time * 3.5).sin() * 0.5 + 0.5;
    defer_archetype_ring(
        ctx,
        pos,
        size * 0.75 + 3.0 + pulse * 4.0,
        2.5,
        Color::new(0.2, 0.95, 0.85, 0.30 + pulse * 0.28),
    )?;

    // Beat telegraph — the Splitter's whole gimmick is a timing bet (catch it ON the beat for a
    // clean, full-jackpot cut; off-beat is a sloppy half-cut). `beat_prox` (0..1, peaking on the
    // beat) drives a gold "grab NOW" flare so the clean-cut window is legible BEFORE the catch, not
    // just afterward: as the beat lands the teal aura blooms into a bright gold ring that snaps in
    // and fades between beats. This is the anticipation cue that lets a player set the cleave up on
    // purpose instead of grabbing blind and hoping.
    if beat_prox > 0.01 {
        defer_archetype_ring(
            ctx,
            pos,
            size * 0.75 + 6.0 + beat_prox * 10.0,
            2.0 + beat_prox * 2.5,
            // Teal→gold as the beat approaches, so the aura visibly "goes hot" in the window.
            Color::new(
                0.4 + 0.6 * beat_prox,
                0.95,
                0.85 - 0.55 * beat_prox,
                0.25 + 0.55 * beat_prox,
            ),
        )?;
    }

    // The "cleave" tell: two small dots split apart from center along the horizontal, snapping back
    // on each pulse cycle — the visual shorthand for "I halve your train". The spread pulses so the
    // two halves visibly separate and rejoin, drawing the eye. On the beat the split snaps WIDER
    // (beat_prox term) so the two halves fling apart exactly when a clean cut is available.
    // Deferred into CLEAVE_DOT_PARAMS (same UNIT_CIRCLE-batching technique as GOLDEN_SPARKLE_PARAMS)
    // instead of two immediate canvas.draw() calls, flushed by flush_archetype_rings().
    let spread = (size * 0.35 + 4.0) * (0.4 + 0.6 * pulse) + beat_prox * size * 0.3;
    CLEAVE_DOT_PARAMS.with(|params_cell| {
        let mut params = params_cell.borrow_mut();
        for &dir in &[-1.0_f32, 1.0] {
            let dpos = pos + Vec2::new(dir * spread, 0.0);
            params.push(
                DrawParam::default()
                    .dest(dpos)
                    .scale(Vec2::splat(2.0 + pulse * 2.0 + beat_prox * 2.5))
                    .color(Color::new(
                        0.5 + 0.5 * beat_prox,
                        1.0,
                        0.9 - 0.5 * beat_prox,
                        0.45 + pulse * 0.5,
                    )),
            );
        }
    });

    Ok(())
}

/// Cleave stakes tag — the pre-catch readout of the Splitter bet. While a free Splitter is on the
/// field and the player has a train worth cleaving, this floats a live "CLEAVE ~N" number at the
/// train's split point (the midpoint where the cut would land), so the player can read what a clean
/// on-beat cut would bank *before* committing — the same "make the bet legible before, not just
/// after" idea as the splitter aura's beat flare, but as an actual score figure over the train.
///
/// `worth` is the clean-cut value (from `cleave_clean_worth`, so it can't drift from the real
/// payout). `jackpot` marks that a Golden/Magnet/cashed-run crossover would fire — the tag reads
/// "JACKPOT" then. `beat_prox` (0..1, peaking on the beat) heats the tag teal→gold in the clean-cut
/// window so "grab NOW" reads on the number itself, matching the aura. The Text is cached and only
/// re-shaped when the value or jackpot state changes, so no per-frame allocation on the draw path.
#[allow(clippy::too_many_arguments)]
pub fn draw_cleave_stakes(
    ctx: &mut Context,
    canvas: &mut Canvas,
    at: Vec2,
    worth: usize,
    jackpot: bool,
    beat_prox: f32,
    time: f32,
) -> ggez::GameResult {
    thread_local! {
        static CLEAVE_STAKES_CACHE: std::cell::RefCell<Option<(usize, bool, Text, f32)>> =
            const { std::cell::RefCell::new(None) };
    }
    CLEAVE_STAKES_CACHE.with(|cache| -> ggez::GameResult {
        let mut c = cache.borrow_mut();
        let needs = c
            .as_ref()
            .map_or(true, |(v, j, _, _)| *v != worth || *j != jackpot);
        if needs {
            let label = if jackpot {
                format!("JACKPOT CLEAVE ~ {}", worth)
            } else {
                format!("CLEAVE ~ {}", worth)
            };
            let mut t = Text::new(label);
            t.set_scale(18.0);
            let w = t.measure(ctx)?.x;
            *c = Some((worth, jackpot, t, w));
        }
        let (_, _, text, w) = c.as_ref().unwrap();
        let w = *w;
        // Bob above the split point, a touch livelier in the beat window so the tag "leans in" as the
        // clean-cut window opens — the anticipation cue on the number itself.
        let bob = (time * (4.0 + beat_prox * 5.0)).sin() * (2.0 + beat_prox * 3.0);
        let base = at - Vec2::new(w * 0.5, 30.0 - bob);
        // Teal at rest, heating to gold as the beat lands (matching the splitter aura flare), and a
        // touch hotter still when a jackpot crossover is on the line.
        let hot = beat_prox.max(if jackpot { 0.25 } else { 0.0 });
        let tr = 0.35 + 0.65 * hot;
        let tg = 0.95;
        let tb = 0.85 - 0.6 * hot;
        // Dark backing keeps it legible over bright field/particles.
        canvas.draw(
            text,
            DrawParam::default()
                .dest(base + Vec2::splat(1.5))
                .color(Color::new(0.0, 0.0, 0.0, 0.55)),
        );
        canvas.draw(
            text,
            DrawParam::default()
                .dest(base)
                .color(Color::new(tr, tg, tb.max(0.0), 0.95)),
        );
        Ok(())
    })
}

/// Tail-run badge — the persistent, watchable face of the same-type match run at the tail of the
/// train. `tail_run_len` only ever flashed for a frame at catch time, so the player could never
/// *set up* the every-4th-link Match-Run Milestone — they couldn't see how long their current run
/// was or how close the next x4 flourish was. This floats a live "RUN xN" over the tail link with a
/// 4-pip meter filling toward the next milestone, color-matched to the run's crab type, and heats +
/// bobs harder in the beat window so committing to a single-type run reads as a live decision, not a
/// silent counter. `col` is the run's crab color; `beat_prox` (0..1) rises as a beat nears.
pub fn draw_tail_run_badge(
    ctx: &mut Context,
    canvas: &mut Canvas,
    at: Vec2,
    run: u32,
    col: [f32; 3],
    beat_prox: f32,
    time: f32,
) -> ggez::GameResult {
    thread_local! {
        static TAIL_RUN_CACHE: std::cell::RefCell<Option<(u32, Text, f32)>> =
            const { std::cell::RefCell::new(None) };
    }
    TAIL_RUN_CACHE.with(|cache| -> ggez::GameResult {
        let mut c = cache.borrow_mut();
        let needs = c.as_ref().map_or(true, |(v, _, _)| *v != run);
        if needs {
            let mut t = Text::new(format!("RUN x{}", run));
            t.set_scale(16.0);
            let w = t.measure(ctx)?.x;
            *c = Some((run, t, w));
        }
        let (_, text, w) = c.as_ref().unwrap();
        let w = *w;
        // Bob above the tail link, leaning in as a beat nears — same anticipation cue the cleave tag
        // uses so the rhythm HUD reads consistently.
        let bob = (time * (3.5 + beat_prox * 4.0)).sin() * (1.5 + beat_prox * 2.5);
        let base = at - Vec2::new(w * 0.5, 34.0 - bob);
        // Text tinted toward the run's crab color, brightened so it stays legible; heats a touch on
        // the beat.
        let hot = beat_prox;
        let tr = (col[0] * 0.5 + 0.5 + 0.15 * hot).min(1.0);
        let tg = (col[1] * 0.5 + 0.5 + 0.15 * hot).min(1.0);
        let tb = (col[2] * 0.5 + 0.5 + 0.15 * hot).min(1.0);
        canvas.draw(
            text,
            DrawParam::default()
                .dest(base + Vec2::splat(1.5))
                .color(Color::new(0.0, 0.0, 0.0, 0.55)),
        );
        canvas.draw(
            text,
            DrawParam::default()
                .dest(base)
                .color(Color::new(tr, tg, tb, 0.95)),
        );
        // 4-pip milestone meter under the label: how many links into the current group of four, so
        // the next Match-Run Milestone (4, 8, 12…) is a visible target you close on. Full row lit
        // means the flourish fires on the next same-type catch.
        let filled = if run == 0 { 0 } else { ((run - 1) % 4) + 1 };
        let pip_r = 3.0;
        let gap = 10.0;
        let row_w = gap * 3.0;
        let py = base.y + 20.0;
        let px0 = at.x - row_w * 0.5;
        // The pip that's about to complete the group pulses on the beat so the "one more lands it"
        // moment is legible.
        let about_to_land = filled == 4;
        // Reuse the cached unit-circle mesh (radius 1.0, built once) and push all variation —
        // position, radius, color — into DrawParam. This replaces 4 Mesh::new_circle GPU buffer
        // allocations per frame with 4 cheap DrawParam draws.
        let uc = unit_circle(ctx)?;
        for i in 0..4u32 {
            let lit = i < filled;
            let cx = px0 + gap * i as f32;
            let (r, g, b, a) = if lit {
                let boost = if about_to_land { 0.3 * hot } else { 0.0 };
                (
                    (col[0] + boost).min(1.0),
                    (col[1] + boost).min(1.0),
                    (col[2] + boost).min(1.0),
                    0.95,
                )
            } else {
                (0.4, 0.4, 0.45, 0.5)
            };
            let rr = if lit && about_to_land {
                pip_r + hot * 1.5
            } else {
                pip_r
            };
            canvas.draw(
                uc,
                DrawParam::default()
                    .dest(Vec2::new(cx, py))
                    .scale(Vec2::splat(rr))
                    .color(Color::new(r, g, b, a)),
            );
        }
        Ok(())
    })
}

/// Cleave slash — the blade stroke drawn the instant a Splitter cuts the conga train. Runs from the
/// last kept front link (`a`) to the split point (`b`), overshooting both ends so it reads as a
/// swung stroke rather than a connecting line. `flash` is a 1→0 life: the stroke starts long and
/// bright and retracts/fades as it decays. `gold` tints it gold on a Jackpot Cleave, teal on a plain
/// cut, matching the shockwave color so the two feedbacks agree.
pub fn draw_cleave_slash(
    ctx: &mut Context,
    canvas: &mut Canvas,
    a: Vec2,
    b: Vec2,
    flash: f32,
    gold: bool,
) -> ggez::GameResult {
    let mid = (a + b) * 0.5;
    let mut dir = b - a;
    if dir.length() < 1.0 {
        dir = Vec2::new(0.0, 1.0); // degenerate (1-link cut) — slash vertically through the point
    }
    let dir = dir.normalize();
    // Overshoot: the stroke reaches beyond both endpoints early in its life, retracting as it fades
    // so it snaps through the train. Half-length in pixels.
    let base = (b - a).length() * 0.5 + 26.0;
    let half = base * (0.55 + 0.45 * flash);
    let p0 = mid - dir * half;
    let p1 = mid + dir * half;

    let (r, g, bl) = if gold { (1.0, 0.88, 0.3) } else { (0.35, 1.0, 0.9) };
    let perp = Vec2::new(-dir.y, dir.x);

    // Tapered blade body — a filled quad that's fat at the leading tip (p1) and tapers to nothing at
    // the trailing tip (p0), so the slash reads as a swung blade with a heavy edge rather than a flat
    // line. Bowed slightly along `perp` so the swing has an arc. Built once per fire (rare event).
    let tip_w = 9.0 * flash + 2.0;
    let bow = perp * (6.0 * flash);
    let blade = [
        p0,
        mid + bow + perp * tip_w * 0.5,
        p1,
        mid + bow - perp * tip_w * 0.5,
    ];
    if let Ok(body) = Mesh::new_polygon(
        ctx,
        DrawMode::fill(),
        &blade,
        Color::new(r, g, bl, 0.28 * flash),
    ) {
        canvas.draw(&body, DrawParam::default());
    }

    // Three stacked strokes: a wide dim glow, a mid teal/gold core, a thin white-hot centerline —
    // so the slash has depth. Use the cached UNIT_LINE mesh (scaled/rotated via DrawParam) instead
    // of Mesh::new_line so these don't allocate a fresh GPU buffer every frame the flash is live.
    let line = unit_line(ctx)?;
    let angle = dir.y.atan2(dir.x);
    let seg_len = (p1 - p0).length();
    let strokes: [(f32, [f32; 4]); 3] = [
        (7.0, [r, g, bl, 0.30 * flash]),
        (3.5, [r, g, bl, 0.70 * flash]),
        (1.4, [1.0, 1.0, 1.0, 0.85 * flash]),
    ];
    for (w, col) in strokes {
        canvas.draw(
            line,
            DrawParam::default()
                .dest(p0)
                .rotation(angle)
                .scale(Vec2::new(seg_len, w))
                .color(Color::new(col[0], col[1], col[2], col[3])),
        );
    }

    let dot = unit_circle(ctx)?;

    // Parting shockline — a short bright bar drawn ACROSS the cut (perpendicular to the blade) at the
    // split point, splitting into two halves that push apart along the blade as the flash decays. This
    // is the "the train comes apart HERE" beat: the eye lands on the seam, not just the swing.
    // Use UNIT_LINE scaled via DrawParam (perpendicular rotation = angle + PI/2) instead of
    // Mesh::new_line to avoid fresh GPU buffer allocations every frame the flash is active.
    let seam_push = (1.0 - flash) * 30.0 + 2.0;
    let seam_half = 20.0 * flash + 5.0;
    let seam_angle = angle + std::f32::consts::FRAC_PI_2;
    let seam_len = seam_half * 2.0;
    for &s in &[-1.0_f32, 1.0] {
        let c = mid + dir * s * seam_push;
        let e0 = c - perp * seam_half; // left end of the perpendicular bar
        canvas.draw(
            line,
            DrawParam::default()
                .dest(e0)
                .rotation(seam_angle)
                .scale(Vec2::new(seam_len, 2.5))
                .color(Color::new(1.0, 1.0, 1.0, 0.7 * flash)),
        );
        // A glow dot riding each parting half.
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(c)
                .scale(Vec2::splat(4.0 + 4.0 * flash))
                .color(Color::new(r, g, bl, 0.55 * flash)),
        );
    }

    // Spark dots flung along the blade, staggered down its length and kicked out perpendicular as the
    // flash decays — the two halves visibly separating along the cut. Fade with the stroke.
    let push = (1.0 - flash) * 22.0 + 4.0;
    for i in 0..5 {
        let t = (i as f32 / 4.0) - 0.5; // -0.5..0.5 along the blade
        let along = mid + dir * (t * half * 1.6);
        for &s in &[-1.0_f32, 1.0] {
            let dpos = along + perp * s * push;
            canvas.draw(
                dot,
                DrawParam::default()
                    .dest(dpos)
                    .scale(Vec2::splat(2.0 + 3.0 * flash))
                    .color(Color::new(r, g, bl, 0.55 * flash)),
            );
        }
    }

    Ok(())
}

/// Campaign world map screen. Draws a simple node-and-path layout over a dark backdrop.
/// Nodes are colored by state: locked=dim gray, unlocked=white, completed=teal, selected=gold ring.
/// Call this instead of the game/title draw when `show_world_map` is true.
pub fn draw_world_map(
    ctx: &mut Context,
    canvas: &mut Canvas,
    map: &crate::world_map::WorldMap,
    width: f32,
    height: f32,
    menu_time: f32,
) -> ggez::GameResult {
    // Dark sea background — unit square scaled to screen size, same pattern as the beat pulse in
    // draw_game. Avoids a fresh Mesh::new_rectangle GPU buffer upload on every frame.
    let sq = unit_square(ctx)?;
    canvas.draw(
        sq,
        DrawParam::default()
            .scale(Vec2::new(width, height))
            .color(Color::new(0.04, 0.07, 0.12, 1.0)),
    );

    let (sx, sy) = (width, height);

    let node_to_screen = |(nx, ny): (f32, f32)| -> Vec2 {
        Vec2::new(nx * sx, 0.25 * sy + ny * sy * 0.5)
    };

    // Biome tint per node — built once (see the cache note above). Cloned out cheaply (≤ a handful
    // of Copy `Color`s) so the draw loops below can read tints without holding the RefCell borrow.
    let node_tints: Vec<Color> = WORLD_MAP_NODE_TINTS.with(|c| {
        let mut cache = c.borrow_mut();
        if cache.is_none() {
            use crate::world_map::NodeKind;
            let levels = crate::levels::get_levels();
            let tints = map
                .nodes
                .iter()
                .map(|n| match &n.kind {
                    // Tutorials are the welcoming on-ramp — a warm amber, distinct from any biome.
                    NodeKind::Tutorial(_) => Color::new(0.90, 0.70, 0.35, 1.0),
                    NodeKind::Level(i) => {
                        let (r, g, b) = levels
                            .get(*i)
                            .map(|l| l.biome.tint)
                            .unwrap_or((200, 200, 200));
                        Color::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
                    }
                })
                .collect::<Vec<_>>();
            *cache = Some(tints);
        }
        cache.clone().unwrap()
    });

    // Connecting path lines between consecutive nodes. Gradient each unlocked leg between the two
    // nodes' biome tints so the path itself telegraphs the tonal shift (warm beach → cold water).
    // A few short segments per leg is plenty on this static menu screen (no gameplay frame budget).
    for i in 0..map.nodes.len().saturating_sub(1) {
        let a = node_to_screen(map.nodes[i].position);
        let b = node_to_screen(map.nodes[i + 1].position);
        if map.nodes[i + 1].unlocked {
            let (ca, cb) = (node_tints[i], node_tints[i + 1]);
            const SEGS: usize = 6;
            for s in 0..SEGS {
                let t0 = s as f32 / SEGS as f32;
                let t1 = (s + 1) as f32 / SEGS as f32;
                let tm = (t0 + t1) * 0.5;
                let col = Color::new(
                    ca.r + (cb.r - ca.r) * tm,
                    ca.g + (cb.g - ca.g) * tm,
                    ca.b + (cb.b - ca.b) * tm,
                    0.6,
                );
                canvas.draw(
                    &Mesh::new_line(ctx, &[a.lerp(b, t0), a.lerp(b, t1)], 3.0, col)?,
                    DrawParam::default(),
                );
            }
        } else {
            // Locked leg stays a dim, colourless thread — the colour "unlocks" with the node.
            canvas.draw(
                &Mesh::new_line(ctx, &[a, b], 3.0, Color::new(0.3, 0.3, 0.3, 0.4))?,
                DrawParam::default(),
            );
        }
    }

    // Reuse UNIT_CIRCLE (built once, scaled via DrawParam) instead of a fresh Mesh::new_circle
    // every frame. Same technique used for all other fill-circle draws in this file.
    let circle = unit_circle(ctx)?;

    for (i, node) in map.nodes.iter().enumerate() {
        let pos = node_to_screen(node.position);
        let is_selected = i == map.selected;

        // Selection ring — gentle pulse. Scale and alpha are per-frame (menu_time-driven), so they
        // stay as DrawParam; only the mesh itself is reused.
        if is_selected {
            let pulse = (menu_time * 3.0).sin() * 0.15 + 0.85;
            canvas.draw(
                circle,
                DrawParam::default()
                    .dest(pos)
                    .scale(Vec2::splat(28.0 * pulse))
                    .color(Color::new(1.0, 0.85, 0.2, 0.35)),
            );
        }

        // Biome glow — a soft radial halo behind each unlocked node in its zone's colour, so the map
        // reads as an illustrated world at a glance. Locked nodes get none (their colour is hidden
        // until earned). Drawn before the fill so the solid dot sits on top of its halo.
        let tint = node_tints[i];
        if node.unlocked {
            canvas.draw(
                circle,
                DrawParam::default()
                    .dest(pos)
                    .scale(Vec2::splat(27.0))
                    .color(Color::new(tint.r, tint.g, tint.b, 0.18)),
            );
        }

        // Node fill — biome-tinted so each zone reads by colour. Computed as a DrawParam (not baked
        // into a mesh) so a single cached unit circle covers all states. Locked → desaturated to a
        // dim grey (colour "unlocks" as a reward); completed → full tint, slightly brightened;
        // selected → tint brightened (the gold selection ring above still marks the cursor).
        let fill_color = if !node.unlocked {
            let g = (tint.r + tint.g + tint.b) / 3.0 * 0.4 + 0.12;
            Color::new(g, g, g, 1.0)
        } else if node.completed {
            Color::new(
                (tint.r * 1.1).min(1.0),
                (tint.g * 1.1).min(1.0),
                (tint.b * 1.1).min(1.0),
                1.0,
            )
        } else if is_selected {
            Color::new(
                (tint.r * 1.25 + 0.1).min(1.0),
                (tint.g * 1.25 + 0.1).min(1.0),
                (tint.b * 1.25 + 0.1).min(1.0),
                1.0,
            )
        } else {
            tint
        };
        canvas.draw(
            circle,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(18.0))
                .color(fill_color),
        );

        // Node label — cached per-node by (completed, unlocked). Selection changes fill color
        // only, never the label text (suffix " ✓" derives from completed, lock " [locked]" from
        // unlocked), so it's not part of the cache key. Same rebuild-on-change pattern as
        // WHISTLE_LABEL_CACHE, STOMP_LABEL_CACHE, and all the other HUD caches in main.rs.
        let label_color = if node.unlocked {
            Color::WHITE
        } else {
            Color::new(0.4, 0.4, 0.4, 1.0)
        };
        let label_key = (node.completed, node.unlocked);
        WORLD_MAP_NODE_LABELS.with(|c| -> ggez::GameResult {
            let mut labels = c.borrow_mut();
            // Grow the Vec to cover this node index if needed (the map never shrinks mid-session).
            if labels.len() <= i {
                labels.resize_with(i + 1, || None);
            }
            let entry = &mut labels[i];
            // Rebuild only when the node's (completed, unlocked) state actually changes.
            if entry.as_ref().map(|(k, _, _)| *k) != Some(label_key) {
                let suffix = if node.completed { " ✓" } else { "" };
                let lock = if !node.unlocked { " [locked]" } else { "" };
                let mut label = Text::new(format!("{}{}{}", node.name, suffix, lock));
                label.set_scale(16.0);
                let w = label.measure(ctx)?.x;
                *entry = Some((label_key, label, w));
            }
            if let Some((_, label, w)) = entry.as_ref() {
                canvas.draw(
                    label,
                    DrawParam::default()
                        .dest(Vec2::new(pos.x - w * 0.5, pos.y + 24.0))
                        .color(label_color),
                );
            }
            Ok(())
        })?;
    }

    // Title — static literal, built once and reused forever. Same pattern as MENU_PROMPT_CACHE.
    WORLD_MAP_TITLE_CACHE.with(|c| -> ggez::GameResult {
        let mut cache = c.borrow_mut();
        if cache.is_none() {
            let mut title = Text::new("— World Map —");
            title.set_scale(28.0);
            let w = title.measure(ctx)?.x;
            *cache = Some((title, w));
        }
        if let Some((title, w)) = cache.as_ref() {
            canvas.draw(
                title,
                DrawParam::default()
                    .dest(Vec2::new((sx - w) * 0.5, sy * 0.08))
                    .color(Color::new(0.8, 0.9, 1.0, 1.0)),
            );
        }
        Ok(())
    })?;

    // Controls hint — static literal, built once and reused forever.
    WORLD_MAP_HINT_CACHE.with(|c| -> ggez::GameResult {
        let mut cache = c.borrow_mut();
        if cache.is_none() {
            let mut hint = Text::new("Left / Right: Navigate     Space / Enter: Play     Esc: Back");
            hint.set_scale(15.0);
            let w = hint.measure(ctx)?.x;
            *cache = Some((hint, w));
        }
        if let Some((hint, w)) = cache.as_ref() {
            canvas.draw(
                hint,
                DrawParam::default()
                    .dest(Vec2::new((sx - w) * 0.5, sy * 0.88))
                    .color(Color::new(0.55, 0.65, 0.75, 1.0)),
            );
        }
        Ok(())
    })?;

    // Soft "skip ahead" warning — shown while a skip confirm is armed (locked node, one Confirm
    // pressed). A brief, non-judgmental inline line just above the controls hint; press Confirm
    // again to go, or move/Esc to back out. Alpha eases in so it doesn't pop.
    if map.skip_warn_timer > 0.0 {
        WORLD_MAP_SKIP_CACHE.with(|c| -> ggez::GameResult {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                let mut warn =
                    Text::new("Skipping ahead — earlier nodes will be marked complete. Confirm again to go.");
                warn.set_scale(16.0);
                let w = warn.measure(ctx)?.x;
                *cache = Some((warn, w));
            }
            if let Some((warn, w)) = cache.as_ref() {
                // Fade in over the first ~0.25s of the 2s window, then hold full.
                let a = ((2.0 - map.skip_warn_timer) * 4.0).min(1.0);
                canvas.draw(
                    warn,
                    DrawParam::default()
                        .dest(Vec2::new((sx - w) * 0.5, sy * 0.82))
                        .color(Color::new(1.0, 0.8, 0.35, a)),
                );
            }
            Ok(())
        })?;
    }

    Ok(())
}

/// Minimap in the top-right corner showing the full 2× world: player, pen, NPC trains, and crabs.
pub fn draw_minimap(
    ctx: &mut Context,
    canvas: &mut Canvas,
    viewport_w: f32,
    viewport_h: f32,
    world_w: f32,
    world_h: f32,
    camera_origin: Vec2,
    player_pos: Vec2,
    pen_pos: Vec2,
    crabs: &[EnemyCrab],
    npc_leaders: &[(Vec2, f32)],
    npc_followers: &[Vec2],
    time: f32,
) -> ggez::GameResult {
    let map_w = 180.0_f32;
    let map_h = map_w * (world_h / world_w);
    let map_x = viewport_w - map_w - 10.0;
    let map_y = 10.0;
    let sp = |pos: Vec2| Vec2::new(map_x + (pos.x / world_w) * map_w, map_y + (pos.y / world_h) * map_h);
    let dot = unit_circle(ctx)?;
    let sq = unit_square(ctx)?;
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(map_x - 2.0, map_y - 2.0)).scale(Vec2::new(map_w + 4.0, map_h + 4.0)).color(Color::from_rgba(0, 0, 0, 150)));
    MINIMAP_DOT_PARAMS.with(|params_cell| -> ggez::GameResult {
        let mut params = params_cell.borrow_mut();
        params.clear();
        for crab in crabs.iter().filter(|c| !c.caught && !c.is_boss()) {
            let [r, g, b] = crab.crab_color();
            params.push(DrawParam::default().dest(sp(crab.pos)).scale(Vec2::splat(2.5)).color(Color::new(r, g, b, 0.45)));
        }
        for crab in crabs.iter().filter(|c| c.caught) {
            let [r, g, b] = crab.crab_color();
            params.push(DrawParam::default().dest(sp(crab.pos)).scale(Vec2::splat(3.0)).color(Color::new(r, g, b, 0.85)));
        }
        for &pos in npc_followers {
            params.push(DrawParam::default().dest(sp(pos)).scale(Vec2::splat(2.0)).color(Color::new(0.96, 0.72, 0.16, 0.6)));
        }
        for &(pos, ls) in npc_leaders {
            let pulse = 0.6 + 0.4 * (time * 3.0).sin().abs();
            params.push(DrawParam::default().dest(sp(pos)).scale(Vec2::splat((3.0 + (ls - 1.2) * 2.0) * pulse)).color(Color::new(0.96, 0.72, 0.16, 0.9)));
        }
        params.push(DrawParam::default().dest(sp(pen_pos)).scale(Vec2::splat(4.0)).color(Color::new(0.3, 1.0, 0.4, 0.85)));
        params.push(DrawParam::default().dest(sp(player_pos)).scale(Vec2::splat(5.0)).color(Color::WHITE));

        MINIMAP_DOT_INSTANCES.with(|inst_cell| -> ggez::GameResult {
            let mut inst_slot = inst_cell.borrow_mut();
            let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
            instances.set(params.iter().copied());
            canvas.draw_instanced_mesh_guarded(dot.clone(), instances, DrawParam::default());
            Ok(())
        })
    })?;
    let vx = map_x + (camera_origin.x / world_w) * map_w;
    let vy = map_y + (camera_origin.y / world_h) * map_h;
    let vw = (viewport_w / world_w) * map_w;
    let vh = (viewport_h / world_h) * map_h;
    let vc = Color::new(1.0, 1.0, 1.0, 0.45);
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(vx, vy)).scale(Vec2::new(vw, 1.0)).color(vc));
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(vx, vy + vh)).scale(Vec2::new(vw, 1.0)).color(vc));
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(vx, vy)).scale(Vec2::new(1.0, vh)).color(vc));
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(vx + vw, vy)).scale(Vec2::new(1.0, vh)).color(vc));
    let bc = Color::from_rgba(200, 200, 200, 80);
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(map_x - 1.0, map_y - 1.0)).scale(Vec2::new(map_w + 2.0, 1.0)).color(bc));
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(map_x - 1.0, map_y + map_h)).scale(Vec2::new(map_w + 2.0, 1.0)).color(bc));
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(map_x - 1.0, map_y)).scale(Vec2::new(1.0, map_h)).color(bc));
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(map_x + map_w, map_y)).scale(Vec2::new(1.0, map_h)).color(bc));
    crate::hud_cache::MINIMAP_LABEL_CACHE.with(|c| {
        let mut slot = c.borrow_mut();
        if slot.is_none() {
            let mut t = Text::new("MAP");
            t.set_scale(11.0);
            *slot = Some(t);
        }
        canvas.draw(slot.as_ref().unwrap(), DrawParam::default().dest(Vec2::new(map_x, map_y - 13.0)).color(Color::from_rgba(200, 200, 200, 110)));
    });
    Ok(())
}

/// Day/night cycle progress bar and weather indicator — sits just below the minimap.
pub fn draw_day_weather_hud(
    ctx: &mut Context,
    canvas: &mut Canvas,
    viewport_w: f32,
    map_h: f32,
    day_phase_t: f32,
    weather_intensity: f32,
    time: f32,
) -> ggez::GameResult {
    let map_w = 180.0_f32;
    let x = viewport_w - map_w - 10.0;
    let y = 10.0 + map_h + 8.0;
    let dot = unit_circle(ctx)?;
    let sq = unit_square(ctx)?;
    let night = ((day_phase_t - 0.5) / 0.5).clamp(0.0, 1.0);
    let day_bright = 1.0 - night;
    canvas.draw(dot, DrawParam::default().dest(Vec2::new(x + 8.0, y + 8.0)).scale(Vec2::splat(8.0 * day_bright.max(0.2))).color(Color::new(1.0, 0.85 + 0.1 * day_bright, 0.3, day_bright.max(0.15))));
    if night > 0.1 {
        canvas.draw(dot, DrawParam::default().dest(Vec2::new(x + 8.0, y + 8.0)).scale(Vec2::splat(7.0 * night)).color(Color::new(0.88, 0.9, 1.0, night * 0.85)));
    }
    let phase_label: &'static str = match (day_phase_t * 4.0) as u32 { 0 => "DAWN", 1 => "DAY", 2 => "DUSK", _ => "NIGHT" };
    crate::hud_cache::WEATHER_PHASE_CACHE.with(|c| {
        let mut slot = c.borrow_mut();
        if slot.as_ref().map_or(true, |(cached, _)| *cached != phase_label) {
            let mut t = Text::new(phase_label); t.set_scale(10.0);
            *slot = Some((phase_label, t));
        }
        canvas.draw(&slot.as_ref().unwrap().1, DrawParam::default().dest(Vec2::new(x + 20.0, y + 2.0)).color(Color::new(0.85, 0.85, 0.95, 0.7)));
    });
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(x, y + 18.0)).scale(Vec2::new(map_w, 3.0)).color(Color::from_rgba(30, 30, 60, 180)));
    let fc = if night > 0.5 { Color::new(0.5, 0.55, 0.9, 0.8) } else if day_phase_t < 0.15 || (day_phase_t > 0.45 && day_phase_t < 0.6) { Color::new(1.0, 0.6, 0.2, 0.8) } else { Color::new(1.0, 0.92, 0.4, 0.8) };
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(x, y + 18.0)).scale(Vec2::new(map_w * day_phase_t, 3.0)).color(fc));
    if weather_intensity > 0.05 {
        let da = weather_intensity * 0.8;
        for i in 0..4 {
            let dx = x + map_w - 30.0 + i as f32 * 7.0;
            let dy = y + 2.0 + ((time * 3.0 + i as f32 * 0.7).sin() * 3.0).abs();
            canvas.draw(sq, DrawParam::default().dest(Vec2::new(dx, dy)).scale(Vec2::new(2.0, 6.0)).color(Color::new(0.5, 0.7, 1.0, da)));
        }
        let is_storm = weather_intensity > 0.5;
        crate::hud_cache::WEATHER_STATE_CACHE.with(|c| {
            let mut slot = c.borrow_mut();
            if slot.as_ref().map_or(true, |(cached_storm, _)| *cached_storm != is_storm) {
                let mut t = Text::new(if is_storm { "STORM" } else { "RAIN" }); t.set_scale(10.0);
                *slot = Some((is_storm, t));
            }
            canvas.draw(&slot.as_ref().unwrap().1, DrawParam::default().dest(Vec2::new(x + map_w - 38.0, y + 12.0)).color(Color::new(0.6, 0.8, 1.0, da)));
        });
    }
    Ok(())
}

/// Compact tool roster at the bottom centre — shows each tool's key, name, matchup hint, and
/// cooldown bar so the player always knows what's ready and what each key does.
pub fn draw_tool_roster(
    ctx: &mut Context,
    canvas: &mut Canvas,
    width: f32,
    height: f32,
    // Cooldowns (0 = ready, >0 = on cooldown)
    whistle_cd: f32,
    whistle_max: f32,
    stomp_cd: f32,
    stomp_max: f32,
    boost_cd: f32,       // dash cooldown
    lasso_busy: bool,    // true when lasso is in flight/dragging
    // Groove/G state
    groove: f32,         // 0..1 groove meter level (for V/G readiness hint)
    time: f32,
    // Rhythm sync: progress toward the next beat (0 = just landed, 1 = about to land) and whether
    // the current instant is inside the on-beat cast window. A READY pad breathes with the beat
    // instead of a free-running sine — it swells as the beat approaches and flashes brightest right
    // in the on-beat window, so the roster reads as a row of drum pads telling you *when* to hit for
    // the on-beat bonus (#164 legibility; the ROADMAP "each tool key is a drum pad" vision).
    beat_progress: f32,
    on_beat: bool,
) -> ggez::GameResult {
    struct ToolSlot {
        key: &'static str,
        name: &'static str,
        hint: &'static str,
        color: [f32; 3],
        cooldown_ratio: f32,
    }

    let whistle_max_safe = if whistle_max <= 0.0 { 1.0 } else { whistle_max };
    let stomp_max_safe   = if stomp_max   <= 0.0 { 1.0 } else { stomp_max };
    let groove_clamped   = groove.clamp(0.0, 1.0);
    let groove_hint: &str = if groove_clamped >= 0.75 { "SLAM ready!" } else { "need groove" };

    let slots = [
        ToolSlot { key: "click",  name: "LASSO",   hint: "snags Thieves", color: [0.3, 0.85, 0.45], cooldown_ratio: if lasso_busy { 0.6 } else { 0.0 } },
        ToolSlot { key: "E",      name: "WHISTLE",  hint: "pulls Dancers",  color: [0.4, 0.85, 1.0],  cooldown_ratio: (whistle_cd / whistle_max_safe).clamp(0.0, 1.0) },
        ToolSlot { key: "R",      name: "STOMP",    hint: "cracks shells",  color: [0.6, 0.7, 1.0],   cooldown_ratio: (stomp_cd   / stomp_max_safe).clamp(0.0, 1.0) },
        ToolSlot { key: "Space",  name: "DASH",     hint: "on beat = +",    color: [1.0, 0.9, 0.5],   cooldown_ratio: (boost_cd   / 0.08_f32).clamp(0.0, 1.0) },
        ToolSlot { key: "V · G", name: "GROOVE",   hint: groove_hint,      color: [0.45, 1.0, 0.85], cooldown_ratio: 1.0 - groove_clamped },
    ];

    let slot_w: f32 = 88.0;
    let slot_h: f32 = 52.0;
    let slot_gap: f32 = 6.0;
    let total_w = 5.0 * slot_w + 4.0 * slot_gap;
    let x0 = (width - total_w) / 2.0;
    let y0 = height - slot_h - 10.0;

    let sq = unit_square(ctx)?;

    // Beat-synced pad glow (0..1): eases up as the next beat approaches and snaps to full inside
    // the on-beat window, so a ready pad pulses ON the beat rather than to a free-running clock.
    // This is the timing cue for on-beat tool casts — the pads light up when it pays to hit them.
    let beat_glow = if on_beat {
        1.0
    } else {
        let p = beat_progress.clamp(0.0, 1.0);
        p * p * 0.7
    };

    for (i, slot) in slots.iter().enumerate() {
        let sx = x0 + i as f32 * (slot_w + slot_gap);
        let sy = y0;
        let ready = slot.cooldown_ratio < 0.05;

        // Slot background — dark rounded rect. Cached by (position, size, color) instead of a
        // fresh Mesh::new_rounded_rectangle GPU buffer every frame — see ROUNDED_FILL_RECT_CACHE /
        // ROUNDED_STROKE_RECT_CACHE. Position/size only change on window resize and border_color
        // only ever takes one of two values (ready vs. on-cooldown), so this settles into a tiny,
        // fixed-size cache after the first couple of frames.
        let border_color = if ready {
            Color::from_rgba(
                (slot.color[0] * 180.0) as u8,
                (slot.color[1] * 180.0) as u8,
                (slot.color[2] * 180.0) as u8,
                200,
            )
        } else {
            Color::from_rgba(60, 65, 90, 160)
        };
        let bg_mesh = cached_rounded_fill_rect(
            ctx,
            sx,
            sy,
            slot_w,
            slot_h,
            5.0,
            Color::from_rgba(10, 14, 30, 180),
        )?;
        canvas.draw(&bg_mesh, DrawParam::default());
        let border_mesh = cached_rounded_stroke_rect(ctx, sx, sy, slot_w, slot_h, 5.0, 1.5, border_color)?;
        canvas.draw(&border_mesh, DrawParam::default());

        // Key label — small, top-left. Cached per slot (see TOOL_ROSTER_TEXT_CACHE) instead of a
        // fresh Text::new() + glyph-shaping pass every frame.
        TOOL_ROSTER_TEXT_CACHE.with(|cache_cell| -> ggez::GameResult {
            let mut cache = cache_cell.borrow_mut();
            let entry = &mut cache[i * 3];
            if entry.as_ref().map_or(true, |(s, _)| *s != slot.key) {
                let mut t = Text::new(slot.key);
                t.set_scale(12.0);
                *entry = Some((slot.key, t));
            }
            canvas.draw(
                &entry.as_ref().unwrap().1,
                DrawParam::default()
                    .dest(Vec2::new(sx + 4.0, sy + 3.0))
                    .color(Color::from_rgba(200, 200, 200, 180)),
            );
            Ok(())
        })?;

        // Tool name — centred, accent color, pulsing ON the beat when ready (drum-pad feel) so the
        // player reads the moment to fire for the on-beat bonus, rather than a free-running blink.
        let pulse = if ready {
            0.55 + beat_glow * 0.45
        } else {
            0.75
        };
        let [r, g, b] = slot.color;
        let name_color = Color::new(r * pulse, g * pulse, b * pulse, 1.0);
        let name_x = sx + slot_w / 2.0 - (slot.name.len() as f32 * 4.2);
        TOOL_ROSTER_TEXT_CACHE.with(|cache_cell| -> ggez::GameResult {
            let mut cache = cache_cell.borrow_mut();
            let entry = &mut cache[i * 3 + 1];
            if entry.as_ref().map_or(true, |(s, _)| *s != slot.name) {
                let mut t = Text::new(slot.name);
                t.set_scale(14.0);
                *entry = Some((slot.name, t));
            }
            canvas.draw(
                &entry.as_ref().unwrap().1,
                DrawParam::default()
                    .dest(Vec2::new(name_x.max(sx + 2.0), sy + 17.0))
                    .color(name_color),
            );
            Ok(())
        })?;

        // Hint text — tiny, dim white, below name. Only the GROOVE slot's hint ever changes value
        // at runtime (toggles between "SLAM ready!" and "need groove"); the content check above
        // re-shapes it on that flip and leaves the other four slots untouched forever.
        let hint_x = sx + slot_w / 2.0 - (slot.hint.len() as f32 * 3.2);
        TOOL_ROSTER_TEXT_CACHE.with(|cache_cell| -> ggez::GameResult {
            let mut cache = cache_cell.borrow_mut();
            let entry = &mut cache[i * 3 + 2];
            if entry.as_ref().map_or(true, |(s, _)| *s != slot.hint) {
                let mut t = Text::new(slot.hint);
                t.set_scale(11.0);
                *entry = Some((slot.hint, t));
            }
            canvas.draw(
                &entry.as_ref().unwrap().1,
                DrawParam::default()
                    .dest(Vec2::new(hint_x.max(sx + 2.0), sy + 33.0))
                    .color(Color::from_rgba(200, 200, 200, 140)),
            );
            Ok(())
        })?;

        // Cooldown / fill bar — 4px tall strip at bottom of slot, inset 4px each side
        let bar_x = sx + 4.0;
        let bar_y = sy + slot_h - 8.0;
        let bar_w = slot_w - 8.0;
        let bar_h = 4.0;

        // Background track
        canvas.draw(
            sq,
            DrawParam::default()
                .dest(Vec2::new(bar_x, bar_y))
                .scale(Vec2::new(bar_w, bar_h))
                .color(Color::from_rgba(20, 20, 40, 200)),
        );

        // Fill — groove slot shows groove level; other slots show readiness
        let fill_ratio = if slot.name == "GROOVE" {
            groove_clamped
        } else {
            1.0 - slot.cooldown_ratio
        };

        if fill_ratio > 0.0 {
            let fill_color = if slot.name == "GROOVE" {
                if groove_clamped >= 0.75 {
                    let glow = (time * 6.0).sin() * 0.5 + 0.5;
                    Color::new(1.0 * (0.7 + glow * 0.3), 0.85 * (0.7 + glow * 0.3), 0.2, 1.0)
                } else {
                    Color::new(slot.color[0], slot.color[1], slot.color[2], 0.9)
                }
            } else if ready {
                // Ready pad's fill brightens on the beat too, in lockstep with the name pulse.
                Color::new(r * (0.8 + beat_glow * 0.2), g * (0.8 + beat_glow * 0.2), b * (0.8 + beat_glow * 0.2), 1.0)
            } else {
                Color::new(r * 0.75, g * 0.75, b * 0.75, 0.85)
            };

            canvas.draw(
                sq,
                DrawParam::default()
                    .dest(Vec2::new(bar_x, bar_y))
                    .scale(Vec2::new(bar_w * fill_ratio, bar_h))
                    .color(fill_color),
            );
        }
    }

    Ok(())
}
