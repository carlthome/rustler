//! Transient beat-feedback "juice" rings and pulses drawn over the crabs each frame:
//! chain ghost rings, catch shockwaves and trails, fear/tide pulses, whistle and
//! catch-bloom rings, the catch-next / cycle-preview / centerpiece hints, the call and
//! groove-call rings, the downbeat pulse, and the stomp/slam impact rings. Extracted
//! from `graphics/mod.rs` to keep that file navigable; these all lean on the shared
//! cached meshes, per-frame instance buffers, and deferred-draw thread-locals defined
//! in the parent module (reached here via `use super::*`).

use super::*;

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
                canvas.draw_instanced_mesh_guarded(mesh, inst, DrawParam::default());
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

    // Flash pass: filled circle for the white-hot impact burst (only shockwaves in their first
    // fraction, age < 0.22). Uses the unit circle scaled per-instance so no new mesh per frame.
    FLASH_INSTANCES.with(|flash_cell| -> ggez::GameResult {
        let mut flash_slot = flash_cell.borrow_mut();
        let flash = flash_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
        flash.set(shockwaves.iter().filter_map(|&(pos, age, _)| {
            if age >= 0.22 { return None; }
            let flash_t = age / 0.22;
            let flash_alpha = (1.0 - flash_t) * 0.9;
            let flash_r = 10.0 + flash_t * 26.0;
            Some(DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(flash_r))
                .color(Color::new(1.0, 1.0, 1.0, flash_alpha)))
        }));
        if !flash.instances().is_empty() {
            canvas.draw_instanced_mesh_guarded(unit_circle.clone(), flash, DrawParam::default());
        }
        Ok(())
    })?;

    // Leading-edge and inner-glow ring passes: group by (radius, thickness) bucket so burst-spawned
    // shockwaves (Downbeat Slam, beat wave, chain reaction) sharing the same age share a mesh and
    // collapse into a handful of instanced draws instead of one canvas.draw() per shockwave per pass.
    // Mirrors the chain-ring grouping approach exactly (same panic-guard on empty InstanceArray).
    SHOCKWAVE_GROUPS.with(|groups_cell| -> ggez::GameResult {
        let mut groups = groups_cell.borrow_mut();
        // Keep the key set bounded to crabs touched this frame rather than all ages ever seen.
        for v in groups.values_mut() { v.clear(); }

        // Ensure all meshes are cached and collect DrawParams grouped by (radius, thickness) key.
        // Two sub-groups per shockwave: leading edge (key) and inner glow (glow_key, age < 0.8).
        // We store (DrawParam, pass) pairs and split below.  Simpler: two separate group maps —
        // but that doubles the HashMap lookups.  Instead encode pass as a sign bit on the key x:
        // positive key = leading edge, negative key = inner glow.  Same HashMap, two namespaces.
        for &(pos, age, color) in shockwaves {
            let ease = 1.0 - (1.0 - age).powi(2);
            let radius = 6.0 + ease * 120.0;
            let fade = (1.0 - age).clamp(0.0, 1.0);
            let thickness = (5.0 * fade).max(1.0);

            // Leading edge
            let edge_r = (color[0] * age + (1.0 - age)).min(1.0);
            let edge_g = (color[1] * age + (1.0 - age)).min(1.0);
            let edge_b = (color[2] * age + (1.0 - age)).min(1.0);
            let key = stroke_circle_key(radius, thickness);
            cached_stroke_circle(ctx, radius, thickness)?;
            groups.entry(key).or_default().push(
                DrawParam::default()
                    .dest(pos)
                    .color(Color::new(edge_r, edge_g, edge_b, fade * 0.95)),
            );

            // Inner glow (only while young enough to show)
            if age < 0.8 {
                let glow_r = (radius - 6.0).max(1.0);
                let glow_t = thickness * 2.2;
                let glow_key = stroke_circle_key(glow_r, glow_t);
                cached_stroke_circle(ctx, glow_r, glow_t)?;
                // Encode glow pass as (-(x+1), y) so it shares the map without colliding with
                // the leading-edge key.  The glow radius is always smaller so x is always
                // non-negative; negating and subtracting 1 guarantees a distinct key range.
                let signed_glow_key = (-(glow_key.0 + 1), glow_key.1);
                groups.entry(signed_glow_key).or_default().push(
                    DrawParam::default()
                        .dest(pos)
                        .color(Color::new(color[0], color[1], color[2], fade * 0.28)),
                );
            }
        }

        SHOCKWAVE_INSTANCES.with(|inst_cell| -> ggez::GameResult {
            let mut instances = inst_cell.borrow_mut();
            for (key, params) in groups.iter() {
                if params.is_empty() { continue; }
                // Recover the real stroke-circle key: glow keys were stored negated/offset.
                let real_key = if key.0 < 0 { (-(key.0 + 1), key.1) } else { *key };
                let mesh = STROKE_CIRCLE_CACHE.with(|c| c.borrow().get(&real_key).cloned());
                let Some(mesh) = mesh else { continue };
                let inst = instances.entry(*key).or_insert_with(|| InstanceArray::new(ctx, None));
                inst.set(params.iter().copied());
                canvas.draw_instanced_mesh_guarded(mesh, inst, DrawParam::default());
            }
            Ok(())
        })
    })?;

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

    // Pre-compute geometry once per trail — tail pos, seg_len, angle, fade — so each of the
    // three instanced passes below can index it rather than recomputing the sqrt + atan2
    // independently. Avoids ~2 redundant sqrt+atan2 per trail per draw_catch_trails call (up to
    // 56 trails × 2 saved pairs × 2 calls/frame = 224 avoided sqrt/atan2 during peak Groove Call).
    TRAIL_GEOM_BUF.with(|geom_cell| {
        let mut geom = geom_cell.borrow_mut();
        geom.clear();
        geom.extend(trails.iter().map(|&(from, to, age, _)| trail_geometry(from, to, age)));
    });

    TRAIL_GLOW_INSTANCES.with(|glow_cell| -> ggez::GameResult {
        TRAIL_CORE_INSTANCES.with(|core_cell| -> ggez::GameResult {
            TRAIL_SPARK_INSTANCES.with(|spark_cell| -> ggez::GameResult {
                TRAIL_GEOM_BUF.with(|geom_cell| -> ggez::GameResult {
                let geom = geom_cell.borrow();

                let mut glow_slot = glow_cell.borrow_mut();
                let mut core_slot = core_cell.borrow_mut();
                let mut spark_slot = spark_cell.borrow_mut();
                let glow = glow_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                let core = core_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                let sparks = spark_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));

                glow.set(trails.iter().zip(geom.iter()).filter_map(|(&(_, _, _, color), g)| {
                    let (tail, seg_len, angle, fade) = (*g)?;
                    let thickness = (2.0 + fade * 5.0).max(1.0);
                    Some(
                        DrawParam::default()
                            .dest(tail)
                            .rotation(angle)
                            .scale(Vec2::new(seg_len, thickness * 2.4))
                            .color(Color::new(color[0], color[1], color[2], fade * 0.30)),
                    )
                }));
                core.set(trails.iter().zip(geom.iter()).filter_map(|(&(_, _, _, color), g)| {
                    let (tail, seg_len, angle, fade) = (*g)?;
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
                sparks.set(geom.iter().filter_map(|g| {
                    let (tail, _, _, fade) = (*g)?;
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
                    canvas.draw_instanced_mesh_guarded(line.clone(), glow, DrawParam::default());
                }
                if !core.instances().is_empty() {
                    canvas.draw_instanced_mesh_guarded(line.clone(), core, DrawParam::default());
                }
                if !sparks.instances().is_empty() {
                    canvas.draw_instanced_mesh_guarded(spark.clone(), sparks, DrawParam::default());
                }
                Ok(())
                })
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

    // Group rings by (radius, thickness) bucket, same approach as draw_catch_shockwaves and
    // draw_chain_rings: burst-spawned fear rings (stampede chain reaction, beat contagion) share
    // the same age and thus the same key each frame, so they collapse into one instanced draw.
    FEAR_RING_GROUPS.with(|groups_cell| -> ggez::GameResult {
        let mut groups = groups_cell.borrow_mut();
        for v in groups.values_mut() { v.clear(); }

        for &(pos, age) in rings {
            let ease = 1.0 - (1.0 - age).powi(2);
            let radius = 8.0 + ease * 135.0;
            let fade = (1.0 - age).clamp(0.0, 1.0);
            let thickness = (4.0 * fade).max(1.0);

            // Leading edge (cyan-white)
            let key = stroke_circle_key(radius, thickness);
            cached_stroke_circle(ctx, radius, thickness)?;
            groups.entry(key).or_default().push(
                DrawParam::default()
                    .dest(pos)
                    .color(Color::new(0.55, 0.9, 1.0, fade * 0.85)),
            );

            // Inner echo (age < 0.75), encoded with negated key to share the map without collision
            if age < 0.75 {
                let echo_r = (radius - 14.0).max(1.0);
                let echo_t = thickness * 1.6;
                let echo_key = stroke_circle_key(echo_r, echo_t);
                cached_stroke_circle(ctx, echo_r, echo_t)?;
                let signed_echo_key = (-(echo_key.0 + 1), echo_key.1);
                groups.entry(signed_echo_key).or_default().push(
                    DrawParam::default()
                        .dest(pos)
                        .color(Color::new(0.35, 0.7, 1.0, fade * 0.3)),
                );
            }
        }

        FEAR_RING_INSTANCES.with(|inst_cell| -> ggez::GameResult {
            let mut instances = inst_cell.borrow_mut();
            for (key, params) in groups.iter() {
                if params.is_empty() { continue; }
                let real_key = if key.0 < 0 { (-(key.0 + 1), key.1) } else { *key };
                let mesh = STROKE_CIRCLE_CACHE.with(|c| c.borrow().get(&real_key).cloned());
                let Some(mesh) = mesh else { continue };
                let inst = instances.entry(*key).or_insert_with(|| InstanceArray::new(ctx, None));
                inst.set(params.iter().copied());
                canvas.draw_instanced_mesh_guarded(mesh, inst, DrawParam::default());
            }
            Ok(())
        })
    })?;

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

/// Draw the on-beat catch-bloom ring at the player's head: a soft teal halo that snaps wide on the
/// beat (widest on the downbeat) and fades to nothing between beats, so the player can SEE the scoop
/// window breathe with the bar. The bloom itself widens catch reach around the whole train (see
/// catch_radius in main.rs); this ring is the head-anchored indicator of it. `radius` is the live
/// catch radius (base + upgrade + bloom); `bloom` is how much of that is the transient beat bloom
/// (0 = resting) and drives brightness so the ring only shows while the window is actually widened.
/// Additive teal to match the rhythm-verb palette (Call/whistle) while staying distinct from the
/// warm herd tones.
pub fn draw_catch_bloom_ring(
    ctx: &mut Context,
    canvas: &mut Canvas,
    center: Vec2,
    radius: f32,
    bloom: f32,
    beat_intensity: f32,
) -> ggez::GameResult {
    // The ring breathes with the bar: it flares on the beat and fades to nothing between beats, so
    // it reads as the scoop window opening and closing — not a permanent catch-radius indicator.
    let flare = (bloom / 30.0).clamp(0.0, 1.0); // 30.0 is the downbeat peak set in the beat handler
    let base_alpha = 0.65 * flare;
    if base_alpha <= 0.02 {
        return Ok(());
    }
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    // The beat's brightness pulse rides on top so the "1" of the bar reads as the strongest scoop.
    let beat = 1.0 + 0.4 * beat_intensity.clamp(0.0, 1.0);
    let thickness = 1.5 + 2.5 * flare;
    let ring = cached_stroke_circle(ctx, radius, thickness)?;
    canvas.draw(
        &ring,
        DrawParam::default()
            .dest(center)
            .color(Color::new(0.20, 0.90, 0.80, (base_alpha * beat).clamp(0.0, 1.0))),
    );

    // A brighter leading dashed hint just inside the edge while the window is wide open, so the
    // moment of "the mouth is open now" pops even at a glance.
    if flare > 0.1 {
        let inner = cached_stroke_circle(ctx, (radius - 4.0).max(1.0), 1.2)?;
        canvas.draw(
            &inner,
            DrawParam::default().dest(center).color(Color::new(
                0.55,
                1.0,
                0.92,
                (0.35 * flare * beat).clamp(0.0, 1.0),
            )),
        );
    }

    canvas.set_blend_mode(original_blend);
    Ok(())
}

/// Highlight a free crab that would EXTEND the tail match-run if caught next — the one arrangement
/// lever the player can actually pull (interior chain order is frozen; only catch order at the tail
/// is steerable). A soft rotating dashed ring in the crab's own archetype `color` (the color of the
/// run it would continue), pulsing with the beat so the "grab me next to keep the run going" cue
/// reads at a glance. Purely legibility — it changes no odds and adds no mechanic, it just surfaces
/// the tail_run_len decision that already exists. `run_len` (the current unbroken same-type tail run)
/// scales the emphasis so a longer hot run shouts louder about protecting it.
pub fn draw_catch_next_hint(
    ctx: &mut Context,
    canvas: &mut Canvas,
    center: Vec2,
    radius: f32,
    color: [f32; 3],
    time: f32,
    beat_intensity: f32,
    run_len: u32,
) -> ggez::GameResult {
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    // A hot run (already 2+) makes the hint brighter and a touch wider, so the crab that *protects*
    // a building streak is the loudest pickup on the field.
    let heat = ((run_len as f32 - 1.0) / 4.0).clamp(0.0, 1.0);
    let beat = 1.0 + 0.5 * beat_intensity.clamp(0.0, 1.0);
    let pulse = 0.55 + 0.25 * (time * 3.0).sin();
    let alpha = (pulse * (0.45 + 0.35 * heat) * beat).clamp(0.0, 1.0);
    let r = radius + 4.0 + 3.0 * heat + 1.5 * (time * 3.0).sin();

    let ring = cached_stroke_circle(ctx, r.max(1.0), 1.6 + 1.2 * heat)?;
    canvas.draw(
        &ring,
        DrawParam::default()
            .dest(center)
            .rotation(time * 1.5)
            .color(Color::new(color[0], color[1], color[2], alpha)),
    );
    // Four little orbiting ticks so it reads as an active "target" marker, not a static aura.
    // Defer into the shared CATCH_NEXT_TICK_PARAMS buffer (all ticks share the same fixed
    // stroke-circle mesh) so flush_catch_next_ticks() can emit them all as one instanced draw
    // after the per-crab aura pass — same technique as the hermit coil / golden sparkle batching.
    let tick_alpha = (alpha * 1.2).clamp(0.0, 1.0);
    let tick_color = Color::new(color[0], color[1], color[2], tick_alpha);
    CATCH_NEXT_TICK_PARAMS.with(|params_cell| {
        let mut params = params_cell.borrow_mut();
        for k in 0..4 {
            let a = time * 1.5 + k as f32 * std::f32::consts::FRAC_PI_2;
            let p = center + Vec2::new(a.cos(), a.sin()) * r;
            params.push(DrawParam::default().dest(p).color(tick_color));
        }
    });

    canvas.set_blend_mode(original_blend);
    Ok(())
}

/// CYCLE PREVIEW ring — marks the train link that a Cycle (X) would promote to the HEAD figurehead
/// slot, so the player can SEE the outcome before pressing the button instead of cycling blind. Drawn
/// on the crab currently at chain_index 1 (rotation lands it at the head). A double chevron/arrow ring
/// sweeping toward the head-crown reads as "this one steps up next". `is_figurehead` = the promoted
/// crab is a Golden or Dancer, i.e. the cycle would actually seat a figurehead into its payoff slot —
/// draw it brighter and gold so a *worthwhile* cycle shouts, while a neutral promotion whispers.
/// Purely legibility: it changes no odds and adds no mechanic, it surfaces the arrangement decision
/// the Cycle verb already offers.
pub fn draw_cycle_preview_ring(
    ctx: &mut Context,
    canvas: &mut Canvas,
    center: Vec2,
    radius: f32,
    color: [f32; 3],
    time: f32,
    beat_intensity: f32,
    is_figurehead: bool,
) -> ggez::GameResult {
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    let beat = 1.0 + 0.5 * beat_intensity.clamp(0.0, 1.0);
    let pulse = 0.6 + 0.3 * (time * 4.0).sin();
    // A figurehead promotion glows gold and brighter; a neutral one stays in the crab's own color.
    let (tint, emphasis) = if is_figurehead {
        ([1.0, 0.85, 0.3], 1.0)
    } else {
        (color, 0.55)
    };
    let alpha = (pulse * (0.4 + 0.35 * emphasis) * beat).clamp(0.0, 1.0);
    let r = radius + 5.0 + 2.0 * (time * 4.0).sin();

    let ring = cached_stroke_circle(ctx, r.max(1.0), 1.8 + 1.0 * emphasis)?;
    canvas.draw(
        &ring,
        DrawParam::default()
            .dest(center)
            .color(Color::new(tint[0], tint[1], tint[2], alpha)),
    );
    // Chevron ticks that climb upward (toward the head of the screen-space train), reading as
    // "promote": three dots marching up the top arc, offset by the beat.
    let dot = cached_stroke_circle(ctx, 2.4, 1.4)?;
    for k in 0..3 {
        let climb = ((time * 2.0 + k as f32 * 0.33).fract()) - 0.5; // -0.5..0.5, wraps
        let a = -std::f32::consts::FRAC_PI_2 + climb * 0.9; // near the top of the ring
        let p = center + Vec2::new(a.cos(), a.sin()) * r;
        let da = (alpha * (1.0 - climb.abs() * 1.4)).clamp(0.0, 1.0);
        canvas.draw(
            &dot,
            DrawParam::default()
                .dest(p)
                .color(Color::new(tint[0], tint[1], tint[2], da)),
        );
    }

    canvas.set_blend_mode(original_blend);
    Ok(())
}

/// CENTERPIECE ring — marks a seated chain link that currently belongs to a *paying* centerpiece
/// run (a same-type run of length >= 3 straddling the train's midpoint, safe from tail snaps). Its
/// job is to make the protected mid-run legible while it's being *built*, so a long train becomes a
/// puzzle the player sets up on purpose rather than a bonus they only discover at the pen. The set
/// of links this is drawn on is computed by `centerpiece_link_indices`, which reuses the exact pen
/// scoring predicate, so what glows is exactly what pays.
///
/// Visually distinct from the cycle-preview and match-run rings: a steady warm-amber laurel — two
/// facing brackets hugging the sides of the crab — reads as "protected / enshrined" rather than the
/// upward "promote" chevrons or the pulsing catch-next dots. Kin to the Golden figurehead economy's
/// gold, but calmer and bracketed, so it can't be mistaken for a Golden sparkle. `endpoint` brightens
/// the two links that bookend the run so the player can read the run's extent at a glance.
pub fn draw_centerpiece_ring(
    ctx: &mut Context,
    canvas: &mut Canvas,
    center: Vec2,
    radius: f32,
    time: f32,
    beat_intensity: f32,
    endpoint: bool,
) -> ggez::GameResult {
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    let beat = 1.0 + 0.4 * beat_intensity.clamp(0.0, 1.0);
    // Slow, steady breathe — protection reads as calm, not urgent.
    let breathe = 0.7 + 0.15 * (time * 2.2).sin();
    let amber = [1.0, 0.78, 0.28];
    let base_a = if endpoint { 0.55 } else { 0.4 };
    let alpha = (breathe * base_a * beat).clamp(0.0, 1.0);
    let r = (radius + 4.0).max(1.0);

    // Facing brackets: short arcs on the left and right of the crab, drawn as small dots stepping
    // along each side arc. Reads as "held in place / enshrined" — a laurel hugging the link.
    // Defer into the shared CENTERPIECE_DOT_PARAMS buffer (all dots share the same fixed
    // stroke-circle mesh) so flush_centerpiece_dots() emits them all as one instanced draw
    // after the chain-crab loop — same technique as hermit-coil / catch-next-tick batching.
    CENTERPIECE_DOT_PARAMS.with(|params_cell| {
        let mut params = params_cell.borrow_mut();
        for side in [0.0_f32, std::f32::consts::PI] {
            for k in 0..5 {
                // Sweep roughly +/- 55deg around the horizontal on each side.
                let spread = (k as f32 / 4.0 - 0.5) * (110.0_f32.to_radians());
                let a = side + spread;
                let p = center + Vec2::new(a.cos(), a.sin()) * r;
                // Endpoints of each bracket a touch dimmer so the middle of the arc leads.
                let da = (alpha * (1.0 - (k as f32 / 4.0 - 0.5).abs() * 0.6)).clamp(0.0, 1.0);
                params.push(
                    DrawParam::default()
                        .dest(p)
                        .color(Color::new(amber[0], amber[1], amber[2], da)),
                );
            }
        }
    });
    // Faint full ring underneath so the brackets read as attached to a whole, not two loose arcs.
    let ring = cached_stroke_circle(ctx, r, 1.4)?;
    canvas.draw(
        &ring,
        DrawParam::default()
            .dest(center)
            .color(Color::new(amber[0], amber[1], amber[2], alpha * 0.35)),
    );

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

/// Draw the Groove Call broadcast — cyan rings sweeping OUTWARD across the whole field, the visual
/// counterpart to the Dancer Call's inward-collapsing "come here". Where that one beckons a few
/// nearby Dancers, this reads as a field-wide summons rippling out to the entire herd, re-kicked on
/// each downbeat while the call's response is live. `pulse` (0..1) is the fade; `reach` is how far
/// the outermost ring sweeps (large — the call is arena-scale).
pub fn draw_groove_call_ring(
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

    // Three rings marching OUTWARD as the pulse decays — a broadcast rippling across the field.
    for (i, phase) in [0.0_f32, 0.33, 0.66].iter().enumerate() {
        let p = (pulse - phase).rem_euclid(1.0);
        let r = reach * (1.0 - p); // expands outward as p → 0
        if r > 8.0 {
            let alpha = (pulse * (1.0 - p) * 0.55).clamp(0.0, 1.0);
            let thickness = 2.5 + 3.5 * p;
            let ring = cached_stroke_circle(ctx, r, thickness)?;
            let g = 0.75 + 0.15 * i as f32 / 3.0;
            canvas.draw(
                &ring,
                DrawParam::default()
                    .dest(center)
                    .color(Color::new(0.35, g, 1.0, alpha)),
            );
        }
    }

    canvas.set_blend_mode(original_blend);
    Ok(())
}

/// Draw the passive downbeat herd-pulse cue — warm rings that snap INWARD toward the player on the
/// "1" of the bar, the visual tell that the beat itself is sweeping loose crabs toward you. Inward
/// motion (opposite the Groove Call's outward broadcast) reads as "the herd is being drawn in", not
/// "a signal going out". `pulse` is 1.0 on the downbeat and decays; `reach` is the pull radius.
pub fn draw_downbeat_pulse_ring(
    ctx: &mut Context,
    canvas: &mut Canvas,
    center: Vec2,
    pulse: f32,
    reach: f32,
    haul: f32,
) -> ggez::GameResult {
    if pulse <= 0.0 {
        return Ok(());
    }
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    // `haul` (0..1) is how big a herd this downbeat is actually sweeping. It blooms the ring so the
    // routing tool's *power* reads: a fat scoop flares brighter, thicker, and shifts from a faint
    // amber thump toward hot gold; an empty-field downbeat stays subtle. Color lerps amber→gold and
    // alpha/thickness scale up with the haul.
    let h = haul.clamp(0.0, 1.0);
    let g = 0.72 + 0.18 * h; // amber (0.72) → gold (0.90)
    let alpha_scale = 0.5 + 0.5 * h; // faint on an empty field, bold over a big herd

    // Two rings collapsing inward as the pulse decays — arrows of the herd being scooped in.
    for phase in [0.0_f32, 0.4] {
        let p = (pulse - phase).clamp(0.0, 1.0);
        // r shrinks from `reach` toward the player as the pulse fades (p: 1 → 0).
        let r = reach * p.max(0.05);
        let alpha = (pulse * alpha_scale).clamp(0.0, 1.0);
        let thickness = 2.0 + 3.0 * (1.0 - p) + 3.0 * h;
        let ring = cached_stroke_circle(ctx, r, thickness)?;
        canvas.draw(
            &ring,
            DrawParam::default()
                .dest(center)
                .color(Color::new(1.0, g, 0.3, alpha)),
        );
    }

    canvas.set_blend_mode(original_blend);
    Ok(())
}

/// Draw the player-anchored beat-keeper: an anticipatory ring that contracts onto the rustler and
/// snaps bright exactly on each beat, so a player whose eyes are on the herd can still *see the beat
/// arrive* and tap in time — the "obvious to play while steering" cue (#164/#165). The ring is widest
/// just after a beat and collapses to a tight cuff on the player right as the next beat lands, like a
/// visual metronome you catch on the "1". Gold and bigger on the bar downbeat, cooler teal between.
/// `guide` (0..1) fades the whole cue: bold while the train is short (learning the tap), a faint tick
/// once you're grooving with a big train, so it never fights the catch-bloom on a veteran's screen.
pub fn draw_beat_keeper_ring(
    ctx: &mut Context,
    canvas: &mut Canvas,
    center: Vec2,
    beat_progress: f32, // 0 just after a beat → 1 as the next beat lands
    on_beat_flash: f32,
    downbeat: bool,
    guide: f32,
) -> ggez::GameResult {
    let guide = guide.clamp(0.0, 1.0);
    if guide <= 0.01 {
        return Ok(());
    }
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    let p = beat_progress.clamp(0.0, 1.0);
    // Contracting radius: reach (widest, just after a beat) → inner cuff (tight, on the beat).
    let inner = crate::PLAYER_SIZE * 0.62;
    let reach = crate::PLAYER_SIZE * (if downbeat { 1.7 } else { 1.35 });
    let r = inner + (reach - inner) * (1.0 - p);
    // Gold on the downbeat (the "1"), teal on the off-beats so the bar's shape reads.
    let (cr, cg, cb) = if downbeat {
        (1.0, 0.82, 0.32)
    } else {
        (0.35, 0.85, 0.95)
    };
    // Brightens as it closes (anticipation) and pops on the on-beat snap frame.
    let approach = 0.10 + 0.22 * p;
    let alpha = ((approach + on_beat_flash * 0.9) * guide).clamp(0.0, 0.85);
    let thickness = 1.5 + 2.5 * on_beat_flash + if downbeat { 1.0 } else { 0.0 };
    let ring = cached_stroke_circle(ctx, r, thickness)?;
    canvas.draw(
        &ring,
        DrawParam::default()
            .dest(center)
            .color(Color::new(cr, cg, cb, alpha)),
    );
    // A tight inner cuff that flashes on the snap frame so the exact "tap now" moment reads.
    if on_beat_flash > 0.15 {
        let cuff = cached_stroke_circle(ctx, inner, 2.0 + 2.0 * on_beat_flash)?;
        canvas.draw(
            &cuff,
            DrawParam::default().dest(center).color(Color::new(
                cr,
                cg,
                cb,
                (on_beat_flash * 0.8 * guide).clamp(0.0, 0.85),
            )),
        );
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
