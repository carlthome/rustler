use crate::enemies::{BossCharge, CrabType, EnemyCrab};
pub use crate::floating_text::{
    FloatingTextSystem, PennedMarcherSystem, draw_floating_texts, draw_penned_marchers,
};
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

// Per-crab body rendering (LOD tiers, leg/claw geometry, and draw_crab itself) lives in its own
// file. Re-exported so every `graphics::draw_crab` / `graphics::set_crab_lod_hint` path is unchanged.
mod crab_draw;
pub use crab_draw::*;

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

// Per-crab archetype aura & cleave-effect draws (Armored shell arc, Hermit coil, attracted/Magnet
// pull glows, Thief prowl, Golden sparkle, Splitter aura, Cleave stakes/slash, tail-run badge) live
// in their own file. Re-exported so every `graphics::draw_*` call-site path is unchanged.
mod auras;
pub use auras::*;

// Rhythm/combo/wave heads-up displays (beat indicator, reef-phrase readout, wave telegraph,
// combo meter, off-screen crab radar) live in their own file. Re-exported so every
// `graphics::draw_*` call-site path is unchanged.
mod hud_indicators;
pub use hud_indicators::*;

// Menu/world-facing HUD screens (campaign world-map, in-play minimap, day/weather strip, tool
// roster). Re-exported so every `graphics::draw_*` call-site path is unchanged.
mod map_hud;
pub use map_hud::*;

// Rope-and-lasso tether rendering (the persistent conga rope + the thrown lasso wind-up/
// throw/snag/drag/miss beats). Re-exported so every `graphics::draw_*` call-site path is
// unchanged.
mod lasso;
pub use lasso::*;

// The player avatar (draw_rustler) and its cosmetic layers (hats/facial-hair/accessories) plus
// the per-skin cosmetics-mesh cache live in their own file. Re-exported so every
// `graphics::draw_rustler` call-site path is unchanged.
mod player_render;
pub use player_render::*;

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
                let mesh =
                    Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
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
                let mesh =
                    Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
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
                let mesh =
                    Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
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
                let mesh =
                    Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
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
                let inst = instances
                    .entry(*key)
                    .or_insert_with(|| InstanceArray::new(ctx, None));
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
                let inst = instances
                    .entry(*key)
                    .or_insert_with(|| InstanceArray::new(ctx, None));
                inst.set(params.iter().copied());
                canvas.draw_instanced_mesh_guarded(mesh, inst, DrawParam::default());
            }
            Ok(())
        })?;
        for v in groups.values_mut() {
            v.clear();
        }
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
                let inst = instances
                    .entry(*key)
                    .or_insert_with(|| InstanceArray::new(ctx, None));
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
            let mesh =
                Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
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
    let key = (
        (radius * 2.0).round() as i32,
        (thickness * 4.0).round() as i32,
        filled,
    );

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
pub fn cached_stroke_circle(
    ctx: &mut Context,
    radius: f32,
    thickness: f32,
) -> ggez::GameResult<Mesh> {
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
    let key = (
        (radius * 2.0).round() as i32,
        (thickness * 4.0).round() as i32,
    );

    if let Some(mesh) = LASSO_LOOP_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return Ok(mesh);
    }

    let pts: Vec<[f32; 2]> = (0..=LASSO_LOOP_SEGMENTS)
        .map(|s| {
            let angle = (s as f32 / LASSO_LOOP_SEGMENTS as f32)
                * LASSO_LOOP_ARC_FRACTION
                * std::f32::consts::TAU;
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
            let mesh = Mesh::new_rectangle(
                ctx,
                DrawMode::fill(),
                Rect::new(0.0, 0.0, 1.0, 1.0),
                Color::WHITE,
            )?;
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
            let mesh = Mesh::new_rectangle(
                ctx,
                DrawMode::fill(),
                Rect::new(0.0, -0.5, 1.0, 1.0),
                Color::WHITE,
            )?;
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
            let mesh =
                Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
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
                .color(Color::from_rgba(
                    140,
                    255,
                    200,
                    alpha.saturating_add((flutter * 25.0) as u8),
                ))
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
            params.push(
                DrawParam::default()
                    .dest(Vec2::new(0.0, 0.0))
                    .scale(Vec2::new(width, band))
                    .color(col),
            );
            // Bottom edge
            params.push(
                DrawParam::default()
                    .dest(Vec2::new(0.0, height - band))
                    .scale(Vec2::new(width, band))
                    .color(col),
            );
            // Left edge
            params.push(
                DrawParam::default()
                    .dest(Vec2::new(0.0, 0.0))
                    .scale(Vec2::new(band, height))
                    .color(col),
            );
            // Right edge
            params.push(
                DrawParam::default()
                    .dest(Vec2::new(width - band, 0.0))
                    .scale(Vec2::new(band, height))
                    .color(col),
            );
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
        canvas.draw(
            &sq,
            DrawParam::default()
                .dest(Vec2::ZERO)
                .scale(Vec2::new(width, band))
                .color(glow_col),
        );
        // bottom edge
        canvas.draw(
            &sq,
            DrawParam::default()
                .dest(Vec2::new(0.0, height - band))
                .scale(Vec2::new(width, band))
                .color(glow_col),
        );
        // left edge
        canvas.draw(
            &sq,
            DrawParam::default()
                .dest(Vec2::ZERO)
                .scale(Vec2::new(band, height))
                .color(glow_col),
        );
        // right edge
        canvas.draw(
            &sq,
            DrawParam::default()
                .dest(Vec2::new(width - band, 0.0))
                .scale(Vec2::new(band, height))
                .color(glow_col),
        );
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
    canvas.draw(
        &dot,
        DrawParam::default()
            .dest(pos)
            .offset(Vec2::new(0.5, 0.5))
            .scale(Vec2::splat(scale))
            .color(Color::new(r, g, b, 0.7 * beat_quality)),
    );
    // Expanding outer ring — the "resonance"
    canvas.draw(
        &dot,
        DrawParam::default()
            .dest(pos)
            .offset(Vec2::new(0.5, 0.5))
            .scale(Vec2::splat(scale * 2.2))
            .color(Color::new(
                r * 0.7 + 0.3,
                g * 0.7 + 0.3,
                b * 0.7 + 0.3,
                0.25 * beat_quality,
            )),
    );
    canvas.set_blend_mode(BlendMode::ALPHA);
    Ok(())
}

/// Fetch a cached stroke-rectangle mesh for the given size/thickness (built once per rounded
/// key, reused after that), instead of calling `Mesh::new_rectangle` fresh every draw. Baked at
/// its actual size (not unit-scaled), since scaling would distort the stroke thickness the same
/// way it would for a stroke circle — draw with `.dest((x, y))` only, no `.scale(..)`.
pub fn cached_stroke_rect(
    ctx: &mut Context,
    w: f32,
    h: f32,
    thickness: f32,
) -> ggez::GameResult<Mesh> {
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
pub fn cached_fill_rect(
    ctx: &mut Context,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Color,
) -> ggez::GameResult<Mesh> {
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

    let mesh =
        Mesh::new_rounded_rectangle(ctx, DrawMode::fill(), Rect::new(x, y, w, h), radius, color)?;
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

    let mesh = Mesh::new_rounded_rectangle(
        ctx,
        DrawMode::stroke(thickness),
        Rect::new(x, y, w, h),
        radius,
        color,
    )?;
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
        DrawParam::default().dest(pos).color(Color::new(
            aura_color[0],
            aura_color[1],
            aura_color[2],
            0.30 + pulse * 0.25,
        )),
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
        let filled = ((segs as f32) * health_frac.clamp(0.0, 1.0))
            .ceil()
            .max(1.0) as usize;
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
            let mesh =
                Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
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
        let mut cone_canvas =
            Canvas::from_image(ctx, cone_image.clone(), Color::from_rgba(0, 0, 0, 0));
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
