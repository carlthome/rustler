//! Environment / weather backdrop rendering: varied biome ground compositions,
//! grass detail, drifting ambient motes, the day/night sky overlay, the world-edge fade,
//! rain and its puddle ripples. Extracted from `graphics/mod.rs` to keep that file
//! navigable — these draw the backdrop behind the crabs and rope and lean on the shared
//! cached meshes, instanced-draw helpers, and per-frame instance buffers defined in the
//! parent module (reached here via `use super::*`). Pure structural move, no behaviour change.

use super::*;

thread_local! {
    // Three reusable instance arrays so the whole richly-detailed three-zone ground collapses
    // into three batched GPU submissions per frame (square fills/stripes/transitions, rotated
    // line strokes for tufts/ripples/shells, and round dots for pebbles/flowers/foam) rather
    // than the hundreds of individual canvas.draw calls the detail would otherwise cost.
    static ZONE_SQ_INSTANCES: RefCell<Option<InstanceArray>> = const { RefCell::new(None) };
    static ZONE_LINE_INSTANCES: RefCell<Option<InstanceArray>> = const { RefCell::new(None) };
    static ZONE_DOT_INSTANCES: RefCell<Option<InstanceArray>> = const { RefCell::new(None) };
    // Staging buffers, reused frame to frame to avoid per-frame heap churn.
    static ZONE_SQ_BUF: RefCell<Vec<DrawParam>> = const { RefCell::new(Vec::new()) };
    static ZONE_LINE_BUF: RefCell<Vec<DrawParam>> = const { RefCell::new(Vec::new()) };
    static ZONE_DOT_BUF: RefCell<Vec<DrawParam>> = const { RefCell::new(Vec::new()) };
    // Cached static ground detail (everything that ISN'T a function of `time`: base fills,
    // mowing stripes, grass tufts/flowers, pebbles/speckle/shells, sand ripple lines, the water
    // highlight strip, and the zone-blend transition strips), keyed by exact world size. zone_rand()
    // is seeded only by element index (see its doc comment), so this whole set is frame-stable —
    // it used to be fully recomputed (thousands of zone_rand()+trig calls and Vec pushes) on every
    // single draw call for content that's always on screen. Now it's built once per world size
    // (i.e. once per level load) and just copied into the per-frame buffers below, which then only
    // compute the genuinely time-varying water ripples + foam twinkle fresh each frame.
    static ZONE_STATIC_CACHE: RefCell<Option<(u32, u32, u8, Vec<DrawParam>, Vec<DrawParam>, Vec<DrawParam>)>> =
        const { RefCell::new(None) };
}

// Cheap deterministic hash → f32 in [0,1). Frame-stable placement: seed by element index (never
// by time), so tufts/pebbles/foam sit still instead of flickering every frame.
#[inline]
fn zone_rand(seed: u32) -> f32 {
    let mut h = seed.wrapping_mul(2654435761);
    h ^= h >> 15;
    h = h.wrapping_mul(2246822519);
    h ^= h >> 13;
    (h & 0x00ff_ffff) as f32 / 0x0100_0000 as f32
}

/// Draw a campaign map's terrain composition with procedural texture and character. Each level
/// chooses a distinct broad layout: meadow, beach, underwater, coastal, or a diagonal river.
/// `time` drives the water ripple phase.
///
/// All detail batches into three instanced draws (squares, lines, dots) so the density is cheap.
/// Detail density scales with world area but is kept modest per unit area so the instance counts
/// stay well bounded even on a large scrolling world.
pub fn draw_world_zones(
    ctx: &mut Context,
    canvas: &mut Canvas,
    world_w: f32,
    world_h: f32,
    time: f32,
    layout: crate::levels::MapLayout,
) -> ggez::GameResult {
    let sq = unit_square(ctx)?.clone();
    let line = unit_line(ctx)?.clone();
    let dot = unit_circle(ctx)?.clone();
    let (grass_w, beach_w, water_w) = match layout {
        crate::levels::MapLayout::Meadow | crate::levels::MapLayout::River => (world_w, 0.0, 0.0),
        crate::levels::MapLayout::Beach => (0.0, world_w, 0.0),
        crate::levels::MapLayout::Underwater => (0.0, 0.0, world_w),
        // A narrow, defensible shore rather than the old equal thirds.
        crate::levels::MapLayout::Coast => (world_w * 0.55, world_w * 0.25, world_w * 0.20),
    };
    let beach_x = grass_w;
    let water_x = grass_w + beach_w;
    let cache_key = (world_w.to_bits(), world_h.to_bits(), layout as u8);
    ZONE_STATIC_CACHE.with(|cache| -> ggez::GameResult {
        let mut cache = cache.borrow_mut();
        let stale = !matches!(&*cache, Some((w, h, l, ..)) if *w == cache_key.0 && *h == cache_key.1 && *l == cache_key.2);
        if stale {
            let mut squares = Vec::new();
            let mut lines = Vec::new();
            let mut dots = Vec::new();

            // ---- Base fills ----
            if grass_w > 0.0 {
                squares.push(DrawParam::default().dest([0.0, 0.0]).scale(Vec2::new(grass_w, world_h))
                    .color(Color::from_rgb(38, 82, 30)));
            }
            if beach_w > 0.0 {
                squares.push(DrawParam::default().dest([beach_x, 0.0]).scale(Vec2::new(beach_w, world_h))
                    .color(Color::from_rgb(196, 168, 112)));
            }
            if water_w > 0.0 {
                squares.push(DrawParam::default().dest([water_x, 0.0]).scale(Vec2::new(water_w, world_h))
                    .color(Color::from_rgb(18, 66, 108)));
            }

            // ---- Grass: mowing stripes (broad alternating light/dark bands) ----
            let stripe_h = 90.0_f32.max(world_h / 12.0);
            let n_stripes = (world_h / stripe_h).ceil() as u32;
            for i in 0..n_stripes {
                let y = i as f32 * stripe_h;
                let (c, a) = if i % 2 == 0 { (0.55, 0.06) } else { (0.05, 0.05) };
                squares.push(DrawParam::default().dest([0.0, y])
                    .scale(Vec2::new(grass_w, stripe_h + 1.0))
                    .color(Color::new(c, c + 0.25, c * 0.5, a)));
            }
            // Grass tufts: 2-3 short angled lines each, plus occasional flower dot. Density ~ area.
            let grass_tufts = ((grass_w * world_h) / 5200.0) as u32;
            for i in 0..grass_tufts {
                let x = 6.0 + zone_rand(i * 3 + 1) * (grass_w - 12.0);
                let y = 6.0 + zone_rand(i * 3 + 2) * (world_h - 12.0);
                let shade = 0.35 + zone_rand(i * 3 + 3) * 0.4;
                let col = Color::new(0.20 * shade, 0.55 * shade + 0.1, 0.15 * shade, 0.85);
                let blades = 2 + (zone_rand(i * 7 + 5) * 2.0) as u32; // 2 or 3
                for b in 0..blades {
                    let ang = -1.7 + (zone_rand(i * 11 + b * 13 + 9) - 0.5) * 1.1; // fan upward
                    let len = 5.0 + zone_rand(i * 17 + b + 21) * 6.0;
                    lines.push(DrawParam::default().dest([x, y]).rotation(ang)
                        .scale(Vec2::new(len, 1.6)).color(col));
                }
                // ~1 in 9 tufts gets a tiny flower
                if zone_rand(i * 19 + 4) > 0.88 {
                    let fh = zone_rand(i * 23 + 6);
                    let fcol = if fh < 0.33 { Color::from_rgb(240, 220, 90) }
                        else if fh < 0.66 { Color::from_rgb(235, 130, 200) }
                        else { Color::from_rgb(240, 240, 250) };
                    dots.push(DrawParam::default().dest([x, y - 6.0]).scale(Vec2::splat(2.4)).color(fcol));
                }
            }

            // ---- Beach: pebbles, shells, and sand ripple lines near the water edge ----
            let pebbles = ((beach_w * world_h) / 6000.0) as u32;
            for i in 0..pebbles {
                let seed = i + 5000;
                let x = beach_x + 5.0 + zone_rand(seed * 3 + 1) * (beach_w - 10.0);
                let y = 5.0 + zone_rand(seed * 3 + 2) * (world_h - 10.0);
                let r = 1.6 + zone_rand(seed * 3 + 3) * 2.4;
                let d = 0.4 + zone_rand(seed * 5 + 7) * 0.35; // darker than sand
                dots.push(DrawParam::default().dest([x, y]).scale(Vec2::splat(r))
                    .color(Color::new(0.55 * d + 0.2, 0.48 * d + 0.18, 0.38 * d + 0.12, 0.9)));
            }
            // Speckle to break up flat sand
            let speckle = ((beach_w * world_h) / 3500.0) as u32;
            for i in 0..speckle {
                let seed = i + 9000;
                let x = beach_x + zone_rand(seed * 3 + 1) * beach_w;
                let y = zone_rand(seed * 3 + 2) * world_h;
                let light = zone_rand(seed * 3 + 3) > 0.5;
                let col = if light { Color::new(1.0, 0.95, 0.8, 0.10) } else { Color::new(0.4, 0.32, 0.2, 0.10) };
                dots.push(DrawParam::default().dest([x, y]).scale(Vec2::splat(1.1)).color(col));
            }
            // Shells: a tiny ellipse (short wide dot) + a line for the hinge/ridge
            let shells = ((beach_w * world_h) / 42000.0).max(if beach_w > 0.0 { 3.0 } else { 0.0 }) as u32;
            for i in 0..shells {
                let seed = i + 13000;
                let x = beach_x + 8.0 + zone_rand(seed * 3 + 1) * (beach_w - 16.0);
                let y = 8.0 + zone_rand(seed * 3 + 2) * (world_h - 16.0);
                let scol = Color::from_rgb(232, 214, 196);
                dots.push(DrawParam::default().dest([x, y]).scale(Vec2::new(4.2, 2.6)).color(scol));
                let ang = (zone_rand(seed * 3 + 3) - 0.5) * 1.2;
                lines.push(DrawParam::default().dest([x - 4.0, y]).rotation(ang)
                    .scale(Vec2::new(8.0, 0.8)).color(Color::from_rgb(200, 180, 160)));
            }
            // Damp sand ripple lines hugging the water edge (right portion of the beach band)
            let ripple_x0 = water_x - beach_w * 0.4;
            for i in 0..if beach_w > 0.0 { 14 } else { 0 } {
                let y = zone_rand(i + 21000) * world_h;
                let wob = (zone_rand(i + 22000) - 0.5) * 8.0;
                lines.push(DrawParam::default().dest([ripple_x0, y + wob])
                    .scale(Vec2::new(beach_w * 0.4, 1.4))
                    .color(Color::new(0.55, 0.45, 0.32, 0.18)));
            }

            // ---- Water: bright surface strip (static; ripples/foam are computed fresh below) ----
            // Lighter highlight near the beach edge (the "surface" nearest land).
            if water_w > 0.0 {
                squares.push(DrawParam::default().dest([water_x, 0.0]).scale(Vec2::new(water_w * 0.18, world_h))
                    .color(Color::new(0.55, 0.8, 0.95, 0.22)));
            }

            // ---- Feathered zone transitions (soft blended edges, not hard seams) ----
            let blend = (world_w * 0.02).clamp(18.0, 30.0);
            let steps = 10u32;
            // grass→beach: interpolate green→tan across the seam
            for s in 0..if grass_w > 0.0 && beach_w > 0.0 { steps } else { 0 } {
                let f = s as f32 / steps as f32;
                let x = beach_x - blend + f * (blend * 2.0);
                let seg = blend * 2.0 / steps as f32 + 1.0;
                // fade from grass color to sand across the strip, low alpha so both show through
                let (r, g, b) = (0.15 + f * 0.62, 0.32 + f * 0.34, 0.12 + f * 0.32);
                squares.push(DrawParam::default().dest([x, 0.0]).scale(Vec2::new(seg, world_h))
                    .color(Color::new(r, g, b, 0.30)));
            }
            // beach→water: interpolate tan→blue, and a hint of wet-sand darkening
            for s in 0..if beach_w > 0.0 && water_w > 0.0 { steps } else { 0 } {
                let f = s as f32 / steps as f32;
                let x = water_x - blend + f * (blend * 2.0);
                let seg = blend * 2.0 / steps as f32 + 1.0;
                let (r, g, b) = (0.62 - f * 0.5, 0.55 - f * 0.28, 0.36 + f * 0.12);
                squares.push(DrawParam::default().dest([x, 0.0]).scale(Vec2::new(seg, world_h))
                    .color(Color::new(r, g, b, 0.32)));
            }

            // A diagonal river turns the kelp maps into a crossing problem instead of another
            // shoreline. It remains visual-only; the biome's patch mechanics handle the routing.
            if layout == crate::levels::MapLayout::River {
                squares.push(
                    DrawParam::default()
                        .dest(Vec2::new(world_w * 0.08, -world_h * 0.14))
                        .rotation(0.48)
                        .scale(Vec2::new(world_w * 0.22, world_h * 1.42))
                        .color(Color::from_rgb(20, 79, 120)),
                );
            }
            *cache = Some((cache_key.0, cache_key.1, cache_key.2, squares, lines, dots));
        }
        let (_, _, _, static_squares, static_lines, static_dots) = cache.as_ref().unwrap();

        ZONE_SQ_BUF.with(|sqb| {
        ZONE_LINE_BUF.with(|lb| {
        ZONE_DOT_BUF.with(|db| -> ggez::GameResult {
            let mut squares = sqb.borrow_mut();
            let mut lines = lb.borrow_mut();
            let mut dots = db.borrow_mut();
            squares.clear();
            lines.clear();
            dots.clear();
            squares.extend_from_slice(static_squares);
            lines.extend_from_slice(static_lines);
            dots.extend_from_slice(static_dots);

            // ---- Water: animated ripples + foam — the only parts that actually depend on `time`,
            // so these (and only these) are computed fresh every frame. ----
            // Animated horizontal ripple lines that drift with time via sin(time + offset).
            let ripples = if water_w > 0.0 { (world_h / 34.0) as u32 } else { 0 };
            for i in 0..ripples {
                let base_y = i as f32 * 34.0 + 8.0;
                let off = zone_rand(i + 30000) * 6.28;
                let sway = (time * 0.8 + off).sin() * 5.0;
                let a = 0.10 + ((time * 0.6 + off).sin() * 0.5 + 0.5) * 0.10;
                let inset = 6.0 + (off.sin() * 0.5 + 0.5) * (water_w * 0.25);
                lines.push(DrawParam::default().dest([water_x + inset, base_y + sway])
                    .scale(Vec2::new(water_w - inset - 6.0, 1.5))
                    .color(Color::new(0.5, 0.75, 0.95, a)));
            }
            // Foam dots: small white flecks, gently twinkling with time.
            let foam = ((water_w * world_h) / 9000.0) as u32;
            for i in 0..foam {
                let seed = i + 40000;
                let x = water_x + 4.0 + zone_rand(seed * 3 + 1) * (water_w - 8.0);
                let y = 4.0 + zone_rand(seed * 3 + 2) * (world_h - 8.0);
                let tw = (time * 1.3 + zone_rand(seed * 3 + 3) * 6.28).sin() * 0.5 + 0.5;
                dots.push(DrawParam::default().dest([x, y]).scale(Vec2::splat(1.3 + tw * 0.8))
                    .color(Color::new(0.9, 0.97, 1.0, 0.15 + tw * 0.35)));
            }

            // ---- Flush the three batches ----
            ZONE_SQ_INSTANCES.with(|cell| -> ggez::GameResult {
                let mut slot = cell.borrow_mut();
                let arr = slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                arr.set(squares.iter().copied());
                canvas.draw_instanced_mesh_guarded(sq, arr, DrawParam::default());
                Ok(())
            })?;
            ZONE_LINE_INSTANCES.with(|cell| -> ggez::GameResult {
                let mut slot = cell.borrow_mut();
                let arr = slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                arr.set(lines.iter().copied());
                canvas.draw_instanced_mesh_guarded(line, arr, DrawParam::default());
                Ok(())
            })?;
            ZONE_DOT_INSTANCES.with(|cell| -> ggez::GameResult {
                let mut slot = cell.borrow_mut();
                let arr = slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                arr.set(dots.iter().copied());
                canvas.draw_instanced_mesh_guarded(dot, arr, DrawParam::default());
                Ok(())
            })?;
            Ok(())
        })})})
    })
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
    // Reuse a cached ShaderParams instead of calling ShaderParamsBuilder::build() every frame:
    // build() allocates a fresh GPU buffer (device.create_buffer in GrowingBufferArena::new) and
    // builds a bind group each call. set_uniforms() re-uploads the changed uniform data (time, beat)
    // to the GPU queue and rebuilds the bind group, but reuses the existing arena buffer so no
    // fresh device.create_buffer call fires on the gameplay hot path.
    let uniform = ResolutionUniform { width, height, time, beat };
    GRASS_SHADER_PARAMS.with(|cell| {
        let mut slot = cell.borrow_mut();
        if let Some(params) = slot.as_mut() {
            params.set_uniforms(ctx, &uniform);
        } else {
            *slot = Some(ShaderParamsBuilder::new(&uniform).build(ctx));
        }
    });
    GRASS_SHADER_PARAMS.with(|cell| {
        if let Some(params) = cell.borrow().as_ref() {
            canvas.set_shader_params(params);
        }
    });
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
    let tile_key = (tiles_x, tiles_y, texture.width(), texture.height());
    GRASS_TILE_INSTANCES.with(|inst_cell| -> ggez::GameResult {
        GRASS_TILE_LAST_KEY.with(|key_cell| -> ggez::GameResult {
            let mut inst_slot = inst_cell.borrow_mut();
            let mut last_key = key_cell.borrow_mut();
            let need_rebuild = *last_key != tile_key;
            let instances = match inst_slot.as_mut() {
                Some(arr) if !need_rebuild => arr,
                _ => {
                    // Window size or texture changed (or first frame) — rebuild the InstanceArray
                    // with the current texture and repopulate the tile grid. Happens at most a
                    // handful of times per session (window open, level transitions, resizes);
                    // never during normal steady-state gameplay. This early-out replaces the
                    // per-frame O(tiles_x * tiles_y) iterator-to-GPU upload that fired every frame
                    // regardless of whether anything had changed.
                    *inst_slot = Some(InstanceArray::new(ctx, texture.clone()));
                    *last_key = tile_key;
                    let arr = inst_slot.as_mut().unwrap();
                    arr.set((0..tiles_y).flat_map(|y| (0..tiles_x).map(move |x| (x, y))).map(
                        |(x, y)| DrawParam::default().dest([x as f32 * tile_w, y as f32 * tile_h]),
                    ));
                    arr
                }
            };
            canvas.draw(instances, DrawParam::default());
            Ok(())
        })
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

// Per-mote constants derived from the index alone — rx, ry, drift, size. These are computed via
// sin/floor hashing and never change between frames; recomputing them 46 × 60 = ~2 760 times/sec
// via transcendentals is pure waste. Cache them in a OnceLock so the math runs exactly once at
// first draw and is free on every subsequent frame.
static AMBIENT_MOTE_CONSTS: OnceLock<[(f32, f32, f32, f32); AMBIENT_MOTE_COUNT]> = OnceLock::new();

fn ambient_mote_consts() -> &'static [(f32, f32, f32, f32); AMBIENT_MOTE_COUNT] {
    AMBIENT_MOTE_CONSTS.get_or_init(|| {
        let mut arr = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); AMBIENT_MOTE_COUNT];
        for (i, entry) in arr.iter_mut().enumerate() {
            let fi = i as f32;
            let seed_a = (fi * 12.9898).sin() * 43758.547;
            let seed_b = (fi * 78.233).sin() * 12543.219;
            let rx = seed_a - seed_a.floor(); // 0..1
            let ry = seed_b - seed_b.floor(); // 0..1
            let drift = 9.0 + rx * 14.0;     // px/s, per-mote speed
            let size = 1.4 + ry * 2.2;
            *entry = (rx, ry, drift, size);
        }
        arr
    })
}

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

    let mote_consts = ambient_mote_consts();
    AMBIENT_MOTE_INSTANCES.with(|cell| -> ggez::GameResult {
        let mut slot = cell.borrow_mut();
        let instances = slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
        instances.set(mote_consts.iter().enumerate().map(|(i, &(rx, ry, drift, size))| {
            // Per-mote constants (rx, ry, drift, size) are precomputed once at startup (see
            // ambient_mote_consts()); only the time-varying position and twinkle are computed here.
            let fi = i as f32;
            // Each mote drifts diagonally and wraps around the screen so the field never empties.
            let base_x = rx * width;
            let base_y = ry * height;
            let x = (base_x + time * drift) % width;
            // Slow vertical sway layered on a slow downward drift, both wrapped to the field.
            let sway = (time * (0.4 + ry * 0.5) + fi).sin() * 10.0;
            let y = (base_y + time * (drift * 0.35) + sway) % height - beat_lift;
            // Twinkle: a slow per-mote brightness pulse so the field shimmers subtly.
            let twinkle = 0.45 + 0.4 * (time * (0.8 + rx) + fi * 1.7).sin();
            let alpha = (0.10 + 0.14 * twinkle) + beat.clamp(0.0, 1.0) * 0.06;
            DrawParam::default()
                .dest(Vec2::new(x - width / 2.0, y - height / 2.0))
                .scale(Vec2::splat(size))
                .color(Color::new(accent.r, accent.g, accent.b, alpha))
        }));
        canvas.draw_instanced_mesh_guarded(unit_circle, instances, DrawParam::default());
        Ok(())
    })?;

    canvas.set_blend_mode(original_blend);
    Ok(())
}

// ===================== WEATHER + DAY/NIGHT AMBIENCE =====================

fn rain_consts() -> &'static [(f32, f32, f32, f32); RAIN_DROP_COUNT] {
    RAIN_CONSTS.get_or_init(|| {
        let mut arr = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); RAIN_DROP_COUNT];
        for (i, entry) in arr.iter_mut().enumerate() {
            let fi = i as f32;
            let a = (fi * 12.9898).sin() * 43758.547;
            let b = (fi * 78.233).sin() * 12543.219;
            let c = (fi * 39.425).sin() * 20214.13;
            let rx = a - a.floor(); // 0..1 column
            let ry = b - b.floor(); // 0..1 initial vertical offset
            let speed = 900.0 + (c - c.floor()) * 700.0; // px/s fall speed
            let len = 14.0 + (a - a.floor()) * 16.0; // streak length
            *entry = (rx, ry, speed, len);
        }
        arr
    })
}

fn puddle_consts() -> &'static [(f32, f32, f32, f32); PUDDLE_RIPPLE_COUNT] {
    PUDDLE_CONSTS.get_or_init(|| {
        let mut arr = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); PUDDLE_RIPPLE_COUNT];
        for (i, entry) in arr.iter_mut().enumerate() {
            let fi = i as f32;
            let a = (fi * 45.164).sin() * 43758.547;
            let b = (fi * 91.117).sin() * 12543.219;
            let c = (fi * 7.311).sin() * 33217.5;
            let rx = a - a.floor();
            let ry = b - b.floor();
            let phase = c - c.floor(); // ripple cycle offset
            let period = 1.4 + (a - a.floor()) * 1.6; // seconds per ripple
            *entry = (rx, ry, phase, period);
        }
        arr
    })
}

/// World-space sky overlay: a soft full-world tint carrying the time-of-day mood plus the
/// cloudy/rain grey dimming. `day_phase_t` (0..1: dawn→day→dusk→night) picks a warm→neutral→
/// orange→deep-blue wash; `weather_intensity` (0..1) layers a cool grey dim on top so heavier
/// weather darkens the world. Kept low-alpha so it grades the scene without hiding gameplay. One
/// draw call (a single tinted full-world quad).
pub fn draw_sky_overlay(
    ctx: &mut Context,
    canvas: &mut Canvas,
    width: f32,
    height: f32,
    day_phase_t: f32,
    weather_intensity: f32,
) -> ggez::GameResult {
    let t = day_phase_t.clamp(0.0, 1.0);
    let wi = weather_intensity.clamp(0.0, 1.0);

    // Time-of-day wash color + strength. Midday is nearly clear; dawn/dusk warm; night cool-blue.
    // (r,g,b,a) with a in 0..1.
    let (r, g, b, base_a) = if t < 0.25 {
        // dawn → day: warm amber fading out toward neutral
        let f = t / 0.25;
        (1.0, 0.72, 0.42, 0.16 * (1.0 - f))
    } else if t < 0.55 {
        // day → dusk: clear ramping into orange-pink
        let f = (t - 0.25) / 0.30;
        (1.0, 0.55, 0.45, 0.02 + 0.16 * f)
    } else if t < 0.80 {
        // dusk → night: orange-pink deepening into blue
        let f = (t - 0.55) / 0.25;
        (
            1.0 - 0.75 * f,
            0.55 - 0.30 * f,
            0.45 + 0.45 * f,
            0.18 + 0.10 * f,
        )
    } else {
        // deep night: steady deep blue. Kept modest because the ground tint is already graded
        // toward blue/dim in parallel — this overlay only needs to add mood, not black out the field.
        (0.20, 0.26, 0.62, 0.22)
    };

    // Weather grey dim, layered as extra alpha of a desaturated cool tone.
    let weather_a = wi * 0.22;

    let square = unit_square(ctx)?.clone();
    if base_a > 0.001 {
        canvas.draw(
            &square,
            DrawParam::default()
                .scale(Vec2::new(width, height))
                .color(Color::new(r, g, b, base_a)),
        );
    }
    if weather_a > 0.001 {
        canvas.draw(
            &square,
            DrawParam::default()
                .scale(Vec2::new(width, height))
                .color(Color::new(0.42, 0.46, 0.52, weather_a)),
        );
    }
    Ok(())
}

// World-edge band width as a fraction of the shorter world dimension. The edge treatment
// (a soft darkening that fades inward from the true playfield boundary) occupies this much
// of the world on each side.
const WORLD_EDGE_FRAC: f32 = 0.09;
// How many nested quads approximate the inward fade. More = smoother gradient; a handful reads
// as a soft band without meaningfully touching the frame budget (one batched instance draw).
const WORLD_EDGE_STEPS: usize = 8;

thread_local! {
    // One reusable instance array for the world-edge fade band, so the whole four-sided border
    // is a single batched GPU submission per frame instead of dozens of individual quads.
    static WORLD_EDGE_INSTANCES: RefCell<Option<InstanceArray>> = const { RefCell::new(None) };
    // Reusable DrawParam staging buffer for world-edge — avoids a Vec::with_capacity heap
    // allocation every frame (32 entries at 60 fps = ~1920 allocs/s on a modest machine).
    static WORLD_EDGE_PARAMS_BUF: RefCell<Vec<DrawParam>> = const { RefCell::new(Vec::new()) };
}

/// Draw a soft, darkening border that fades inward from the true edges of the (larger-than-
/// viewport) world, so scrolling to the playfield limit reads as arriving at a tangible shore/
/// boundary rather than an abrupt camera clamp. Drawn in WORLD space (over the ground, under the
/// action), tinted to the biome accent so each zone's edge feels like part of its place. The band
/// only becomes visible when the camera actually reaches an edge, since off-edge slices sit outside
/// the viewport.
pub fn draw_world_edge(
    ctx: &mut Context,
    canvas: &mut Canvas,
    world_w: f32,
    world_h: f32,
    tint: Color,
    night_factor: f32,
) -> ggez::GameResult {
    let band = world_w.min(world_h) * WORLD_EDGE_FRAC;
    if band <= 1.0 {
        return Ok(());
    }
    // The edge reads a touch deeper at night, matching the dimmed field, so the boundary still
    // frames the space when everything else has gone dark.
    let peak_a = 0.34 + night_factor.clamp(0.0, 1.0) * 0.20;
    let steps = WORLD_EDGE_STEPS.max(1);
    let sq = unit_square(ctx)?.clone();

    WORLD_EDGE_INSTANCES.with(|cell| -> ggez::GameResult {
        WORLD_EDGE_PARAMS_BUF.with(|pbuf| -> ggez::GameResult {
            let mut slot = cell.borrow_mut();
            let arr = slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
            let mut params = pbuf.borrow_mut();
            params.clear();
            for i in 0..steps {
                // f: 0 at the outermost slice (full strength), →1 at the innermost (transparent).
                let f = i as f32 / steps as f32;
                let inset = band * f;
                let seg = band / steps as f32;
                // Quadratic falloff so the darkening hugs the true edge and feathers gently inward.
                let a = peak_a * (1.0 - f) * (1.0 - f);
                let col = Color::new(tint.r * 0.45, tint.g * 0.45, tint.b * 0.55, a);
                // Top band
                params.push(
                    DrawParam::default()
                        .dest([0.0, inset])
                        .scale(Vec2::new(world_w, seg))
                        .color(col),
                );
                // Bottom band
                params.push(
                    DrawParam::default()
                        .dest([0.0, world_h - inset - seg])
                        .scale(Vec2::new(world_w, seg))
                        .color(col),
                );
                // Left band
                params.push(
                    DrawParam::default()
                        .dest([inset, 0.0])
                        .scale(Vec2::new(seg, world_h))
                        .color(col),
                );
                // Right band
                params.push(
                    DrawParam::default()
                        .dest([world_w - inset - seg, 0.0])
                        .scale(Vec2::new(seg, world_h))
                        .color(col),
                );
            }
            arr.set(params.iter().copied());
            canvas.draw_instanced_mesh_guarded(sq, arr, DrawParam::default());
            Ok(())
        })
    })
}

/// Screen-space weather pass: diagonal rain streaks (density/opacity scale with intensity, with a
/// subtle on-beat opacity pulse), a heavy-rain edge vignette that closes visibility in at the
/// screen border, and the storm lightning full-screen flash. Drawn in viewport space so nothing
/// smears as the world scrolls. Rain is one instanced draw regardless of drop count.
#[allow(clippy::too_many_arguments)]
pub fn draw_weather(
    ctx: &mut Context,
    canvas: &mut Canvas,
    width: f32,
    height: f32,
    time: f32,
    weather_intensity: f32,
    beat: f32,
    lightning_flash: f32,
) -> ggez::GameResult {
    let wi = weather_intensity.clamp(0.0, 1.0);

    // Rain only reads once it's actually raining (>~Rain). Below that, skip the whole pass except
    // any active lightning flash (a storm flash can linger as intensity eases).
    let raining = wi > 0.35;

    if raining {
        // How many of the precomputed drops are active, scaled with intensity (calm rain is sparse,
        // heavy rain/storm fills the screen).
        let active = ((wi - 0.35) / 0.65).clamp(0.0, 1.0);
        let drop_count = (((RAIN_DROP_COUNT as f32) * active) as usize).max(1);
        // On-beat opacity pulse so the rain breathes with the music like everything else.
        let beat_pulse = 1.0 + beat.clamp(0.0, 1.0) * 0.25;
        let base_alpha = (0.18 + 0.35 * active) * beat_pulse;
        // Diagonal fall: slight rightward slant. dir is (slant, 1) normalized-ish for the streak angle.
        let slant: f32 = 0.28;
        let angle = slant.atan2(1.0) + std::f32::consts::FRAC_PI_2; // near-vertical, tilted

        let unit_line = unit_line(ctx)?.clone();
        let consts = rain_consts();
        let original_blend = canvas.blend_mode();
        canvas.set_blend_mode(BlendMode::ALPHA);
        RAIN_INSTANCES.with(|cell| -> ggez::GameResult {
            let mut slot = cell.borrow_mut();
            let instances = slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
            instances.set(consts[..drop_count].iter().map(|&(rx, ry, speed, len)| {
                let x_base = rx * (width + 120.0) - 60.0;
                // Wrap the fall over the screen height plus the streak length so drops recycle.
                let span = height + 80.0;
                let y = ((ry * span) + time * speed) % span - 40.0;
                // Horizontal drift matches the slant so the streak angle and travel agree.
                let x = x_base + y * slant;
                DrawParam::default()
                    .dest(Vec2::new(x, y))
                    .rotation(angle)
                    .scale(Vec2::new(len, 1.4))
                    .color(Color::new(0.72, 0.80, 0.92, base_alpha.min(0.7)))
            }));
            canvas.draw_instanced_mesh_guarded(unit_line.clone(), instances, DrawParam::default());
            Ok(())
        })?;
        canvas.set_blend_mode(original_blend);

        // Heavy-rain edge vignette: darken the screen border so visibility closes in as it pours.
        // Four thin gradient-ish bands (top/bottom/left/right) via alpha quads — cheap, and only
        // once it's heavy (>~HeavyRain).
        let vig = ((wi - 0.6) / 0.4).clamp(0.0, 1.0);
        if vig > 0.01 {
            let square = unit_square(ctx)?.clone();
            let band = (width.min(height)) * 0.14;
            let a = vig * 0.34;
            let dark = Color::new(0.05, 0.07, 0.12, a);
            // top
            canvas.draw(&square, DrawParam::default().dest(Vec2::new(0.0, 0.0)).scale(Vec2::new(width, band)).color(dark));
            // bottom
            canvas.draw(&square, DrawParam::default().dest(Vec2::new(0.0, height - band)).scale(Vec2::new(width, band)).color(dark));
            // left
            canvas.draw(&square, DrawParam::default().dest(Vec2::new(0.0, 0.0)).scale(Vec2::new(band, height)).color(dark));
            // right
            canvas.draw(&square, DrawParam::default().dest(Vec2::new(width - band, 0.0)).scale(Vec2::new(band, height)).color(dark));
        }
    }

    // Lightning flash: a full-screen white brighten that decays with lightning_flash (1→0). Uses ADD
    // so it floods light in rather than washing to grey. Thunder (screen shake) is fired from the
    // sim off the same event, so the flash and the shake land together.
    // At peak intensity (> 0.5) an additional INVERT pass inverts the scene colors for a single
    // brief window — the photographic "negative" of a real lightning strike. The ADD layer then
    // re-brightens on top, so the final read is: invert → flood white → fade, rather than just fade.
    let lf = lightning_flash.clamp(0.0, 1.0);
    if lf > 0.001 {
        let square = unit_square(ctx)?.clone();
        let original_blend = canvas.blend_mode();
        // Invert layer: only at peak (lf > 0.5) and ramps up quickly toward the spike so it reads
        // as a stark photographic flash rather than a long color-shifted linger.
        if lf > 0.5 {
            let invert_alpha = ((lf - 0.5) * 2.0).min(1.0);
            canvas.set_blend_mode(BlendMode::INVERT);
            canvas.draw(
                &square,
                DrawParam::default()
                    .scale(Vec2::new(width, height))
                    .color(Color::new(1.0, 1.0, 1.0, invert_alpha)),
            );
        }
        canvas.set_blend_mode(BlendMode::ADD);
        canvas.draw(
            &square,
            DrawParam::default()
                .scale(Vec2::new(width, height))
                // Sharp peak that falls off fast so it reads as a strobe, not a fade.
                .color(Color::new(0.9, 0.93, 1.0, lf * lf * 0.55)),
        );
        canvas.set_blend_mode(original_blend);
    }

    Ok(())
}

/// World-space puddle ripples: expanding rings that pop where rain "lands" on the ground, only
/// while it's raining. Positions are spread across the visible viewport slice (offset by the
/// camera origin) so they follow the player without being wasted off-screen. One instanced draw.
pub fn draw_puddle_ripples(
    ctx: &mut Context,
    canvas: &mut Canvas,
    camera_origin: Vec2,
    view_w: f32,
    view_h: f32,
    time: f32,
    weather_intensity: f32,
) -> ggez::GameResult {
    let wi = weather_intensity.clamp(0.0, 1.0);
    let active = ((wi - 0.35) / 0.65).clamp(0.0, 1.0);
    if active <= 0.01 {
        return Ok(());
    }

    let unit = unit_circle(ctx)?.clone();
    let consts = puddle_consts();
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);
    PUDDLE_INSTANCES.with(|cell| -> ggez::GameResult {
        let mut slot = cell.borrow_mut();
        let instances = slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
        instances.set(consts.iter().map(|&(rx, ry, phase, period)| {
            let cx = camera_origin.x + rx * view_w;
            let cy = camera_origin.y + ry * view_h;
            // Ripple cycle 0..1: ring grows and fades over `period` seconds.
            let cyc = ((time / period) + phase).fract();
            let radius = 2.0 + cyc * (10.0 + active * 14.0);
            // Ring alpha: brightest just after the drop lands, fading to nothing as it spreads.
            let alpha = (1.0 - cyc) * 0.22 * active;
            // Draw a thin ring by scaling the filled unit circle small — a faint disc reads as a
            // ripple highlight under the ADD blend without needing a stroked mesh.
            DrawParam::default()
                .dest(Vec2::new(cx, cy))
                .scale(Vec2::splat(radius))
                .color(Color::new(0.70, 0.82, 0.95, alpha))
        }));
        canvas.draw_instanced_mesh_guarded(unit, instances, DrawParam::default());
        Ok(())
    })?;
    canvas.set_blend_mode(original_blend);
    Ok(())
}
