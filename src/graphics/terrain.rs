//! Terrain and biome ground-layer rendering: tide pools, boss fissures, and the
//! rock/kelp patches that give each biome its distinct footing. Extracted from
//! `graphics.rs` to keep that file navigable — these draw on the ground layer under
//! the crabs and rope, and lean on the shared cached meshes and per-frame instance
//! buffers defined in the parent module (reached here via `use super::*`).

use super::*;

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
    // Whether these pools carry the Tide Pool current (and so should draw flow streaks). True only
    // for the Water biome's *native* pools — the drift the sim actually applies. Tide Boss flood
    // pools also render as Water but carry no current, so they pass false: a flow streak that lies
    // about where the herd drifts is worse than none, especially mid-boss-fight when Carl's watching.
    show_current: bool,
    // Rocky Shore tide level in [0,1] (0 = fully ebbed, 1 = fully flooded). Only the Rock biome uses
    // it — it drives the rising water sheet drawn over the low rocks. Other terrains ignore it.
    rock_tide_fill: f32,
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
        TerrainKind::Rock => {
            return draw_rock_patches(ctx, canvas, pools, unit_circle, beat, rock_tide_fill)
        }
        TerrainKind::Kelp => {
            return draw_kelp_patches(ctx, canvas, pools, unit_circle, time, beat, player_center);
        }
        _ => {}
    }

    // Pass 1 (normal blend): batch all base fills and shallow centers into one InstanceArray draw
    // instead of 2 × pool_count individual canvas.draw() calls — the same technique Rock/Kelp
    // patches use. With 5-10 pools (plus flood pools) this collapses ~20 fill submissions into 1.
    POOL_FILL_PARAMS.with(|fill_cell| -> ggez::GameResult {
        let mut fill_params = fill_cell.borrow_mut();
        fill_params.clear();
        for (i, (center, radius)) in pools.iter().enumerate() {
            let center = *center;
            let radius = *radius;
            let phase = i as f32 * 1.7;
            let breathe = 0.5 + 0.5 * (time * 1.3 + phase).sin();
            let wading = player_center.distance(center) < radius;
            // Base water disc — normal blend so it reads as a darker, cooler patch of ground.
            let fill_a = 0.30 + 0.06 * breathe + if wading { 0.10 } else { 0.0 };
            fill_params.push(
                DrawParam::default()
                    .dest(center)
                    .scale(Vec2::splat(radius))
                    .color(Color::new(0.16, 0.34, 0.52, fill_a)),
            );
            // Lighter shallow center for a bit of depth.
            fill_params.push(
                DrawParam::default()
                    .dest(center)
                    .scale(Vec2::splat(radius * 0.6))
                    .color(Color::new(0.30, 0.55, 0.72, 0.16)),
            );
        }
        if !fill_params.is_empty() {
            POOL_FILL_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                let mut inst_slot = inst_cell.borrow_mut();
                let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                instances.set(fill_params.iter().copied());
                canvas.draw_instanced_mesh(unit_circle.clone(), instances, DrawParam::default());
                Ok(())
            })?;
        }
        Ok(())
    })?;

    // Pass 2 (one ADD switch for all pools): rims and ripple rings stay individual (each has a
    // distinct stroke radius — at most 3 per pool, all cache-hits from cached_stroke_circle).
    // Glints and current streaks are unit_circle fills so they're batched into a second
    // InstanceArray: one draw_instanced_mesh call replaces up to 5 per-pool ADD draws.
    let orig_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    // Rims and ripple rings — batched by stroke_circle_key bucket, same pattern as
    // draw_chain_rings: stroke meshes can't share a single unit mesh (scaling would stretch
    // stroke thickness), but pools with the same quantised radius do share the same cached mesh
    // and therefore collapse into one instanced draw per key group.
    POOL_RIM_GROUPS.with(|rim_groups_cell| -> ggez::GameResult {
        let mut rim_groups = rim_groups_cell.borrow_mut();
        rim_groups.clear();
        POOL_RIPPLE_GROUPS.with(|ripple_groups_cell| -> ggez::GameResult {
            let mut ripple_groups = ripple_groups_cell.borrow_mut();
            ripple_groups.clear();

            // Collect DrawParams grouped by mesh key.
            for (i, (center, radius)) in pools.iter().enumerate() {
                let center = *center;
                let radius = *radius;
                let phase = i as f32 * 1.7;
                let breathe = 0.5 + 0.5 * (time * 1.3 + phase).sin();
                let wading = player_center.distance(center) < radius;

                // Soft rim so the pool edge reads clearly.
                // Ensure the mesh exists in the cache for this key.
                cached_stroke_circle(ctx, radius, 2.5)?;
                let rim_key = stroke_circle_key(radius, 2.5);
                let rim_alpha =
                    (0.22 + 0.18 * breathe + if wading { 0.25 } else { 0.0 }).clamp(0.0, 1.0);
                rim_groups
                    .entry(rim_key)
                    .or_default()
                    .push(DrawParam::default().dest(center).color(Color::new(0.45, 0.8, 1.0, rim_alpha)));

                // Two ripple rings expanding outward from the middle.
                for k in 0..2 {
                    let t = ((time * 0.35 + phase + k as f32 * 0.5).fract()).clamp(0.0, 1.0);
                    let rr = radius * (0.15 + t * 0.85);
                    let a = (1.0 - t) * 0.28;
                    if a > 0.01 {
                        cached_stroke_circle(ctx, rr, 1.5)?;
                        let ripple_key = stroke_circle_key(rr, 1.5);
                        ripple_groups
                            .entry(ripple_key)
                            .or_default()
                            .push(DrawParam::default().dest(center).color(Color::new(0.55, 0.85, 1.0, a)));
                    }
                }
            }

            // Flush rim groups.
            POOL_RIM_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                let mut inst_map = inst_cell.borrow_mut();
                for (key, params) in rim_groups.iter() {
                    if params.is_empty() {
                        continue;
                    }
                    let mesh = STROKE_CIRCLE_CACHE.with(|c| c.borrow().get(key).cloned());
                    if let Some(mesh) = mesh {
                        let instances = inst_map
                            .entry(*key)
                            .or_insert_with(|| InstanceArray::new(ctx, None));
                        instances.set(params.iter().copied());
                        canvas.draw_instanced_mesh(mesh, instances, DrawParam::default());
                    }
                }
                Ok(())
            })?;

            // Flush ripple groups.
            POOL_RIPPLE_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                let mut inst_map = inst_cell.borrow_mut();
                for (key, params) in ripple_groups.iter() {
                    if params.is_empty() {
                        continue;
                    }
                    let mesh = STROKE_CIRCLE_CACHE.with(|c| c.borrow().get(key).cloned());
                    if let Some(mesh) = mesh {
                        let instances = inst_map
                            .entry(*key)
                            .or_insert_with(|| InstanceArray::new(ctx, None));
                        instances.set(params.iter().copied());
                        canvas.draw_instanced_mesh(mesh, instances, DrawParam::default());
                    }
                }
                Ok(())
            })
        })
    })?;

    // Glints and current streaks — batched into one InstanceArray (all unit_circle draws).
    // Precompute flow constants once outside the pool loop instead of per pool.
    POOL_ADD_PARAMS.with(|add_cell| -> ggez::GameResult {
        let mut add_params = add_cell.borrow_mut();
        add_params.clear();
        let flow = crate::TIDE_CURRENT_DIR.normalize_or_zero();
        let perp = Vec2::new(-flow.y, flow.x);
        let streak_angle = flow.y.atan2(flow.x);
        const STREAKS: usize = 4;
        for (i, (center, radius)) in pools.iter().enumerate() {
            let center = *center;
            let radius = *radius;
            let phase = i as f32 * 1.7;
            // A drifting glint highlight, brighter on the beat, to sell the wet surface.
            let g_ang = time * 0.6 + phase;
            let glint = center + Vec2::new(g_ang.cos(), g_ang.sin() * 0.5) * radius * 0.4;
            add_params.push(
                DrawParam::default()
                    .dest(glint)
                    .scale(Vec2::splat(6.0 + 3.0 * beat))
                    .color(Color::new(0.7, 0.95, 1.0, 0.18 + 0.25 * beat)),
            );
            // Current flow streaks: short bright dashes streaming along TIDE_CURRENT_DIR.
            // Only native Water pools carry a current; flood pools skip streaks (show_current).
            if show_current {
                for s in 0..STREAKS {
                    let t = (time * 0.5 + s as f32 / STREAKS as f32 + phase * 0.3).fract();
                    let lateral = ((s as f32 / (STREAKS - 1) as f32) - 0.5) * 1.4 * radius;
                    let along = (t - 0.5) * 2.0 * radius;
                    let p = center + flow * along + perp * lateral;
                    let edge_fade = (t * (1.0 - t) * 4.0).clamp(0.0, 1.0);
                    let a = (0.16 + 0.18 * beat) * edge_fade;
                    if a > 0.01 {
                        add_params.push(
                            DrawParam::default()
                                .dest(p)
                                .rotation(streak_angle)
                                .scale(Vec2::new(9.0 + 4.0 * beat, 2.2))
                                .color(Color::new(0.65, 0.92, 1.0, a)),
                        );
                    }
                }
            }
        }
        if !add_params.is_empty() {
            POOL_ADD_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                let mut inst_slot = inst_cell.borrow_mut();
                let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                instances.set(add_params.iter().copied());
                canvas.draw_instanced_mesh(unit_circle.clone(), instances, DrawParam::default());
                Ok(())
            })?;
        }
        Ok(())
    })?;

    canvas.set_blend_mode(orig_blend);
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

    // Batch all fissure draws into five InstanceArray submissions instead of one canvas.draw()
    // per spoke/cap/pit per fissure. With 5 fissures x (1 pit + 1 core + 7 spokes + conditional
    // geyser column + cap) the old loop issued up to 65 individual GPU submissions plus 5 per-
    // fissure blend-mode switches. The new layout: one ALPHA pass (pit fills), one ADD pass
    // (cores + spokes + geyser columns + geyser caps), and one per-fissure stroke-circle rim
    // (still individual — each fissure's rim has a distinct radius so they can't be grouped by key
    // the way chain rings are; the rim count is bounded by 5, so it's cheap). Total GPU submissions
    // drop from ~65 + 5 blend switches to ~7 + 1 blend switch pair, with identical on-screen output.
    FISSURE_PIT_PARAMS.with(|pp| {
        FISSURE_CORE_PARAMS.with(|cp| {
            FISSURE_SPOKE_PARAMS.with(|sp| {
                FISSURE_GEYSER_PARAMS.with(|gp| {
                    FISSURE_CAP_PARAMS.with(|cap| {
                        let mut pit_params = pp.borrow_mut();
                        let mut core_params = cp.borrow_mut();
                        let mut spoke_params = sp.borrow_mut();
                        let mut geyser_params = gp.borrow_mut();
                        let mut cap_params = cap.borrow_mut();
                        pit_params.clear();
                        core_params.clear();
                        spoke_params.clear();
                        geyser_params.clear();
                        cap_params.clear();

                        for (i, &(center, radius, age)) in fissures.iter().enumerate() {
                            let open = age.clamp(0.0, 1.0);
                            let phase = i as f32 * 1.9;
                            let glow = 0.5 + 0.5 * (time * 4.0 + phase).sin();

                            // Pass 1 (ALPHA): dark scorched pit.
                            pit_params.push(
                                DrawParam::default()
                                    .dest(center)
                                    .scale(Vec2::splat(radius * open))
                                    .color(Color::new(0.12, 0.03, 0.02, 0.5)),
                            );

                            // Pass 2 (ADD): molten core.
                            core_params.push(
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

                            // Pass 2 (ADD): geyser column and cap when erupting.
                            if erupt > 0.02 && open > 0.5 {
                                let col_h = radius * (0.9 + 1.4 * erupt);
                                let col_w = radius * 0.5;
                                geyser_params.push(
                                    DrawParam::default()
                                        .dest(center + Vec2::new(0.0, -col_h * 0.5))
                                        .rotation(-std::f32::consts::FRAC_PI_2)
                                        .scale(Vec2::new(col_h, col_w))
                                        .color(Color::new(1.0, 0.55 + 0.35 * glow, 0.2, 0.35 * erupt)),
                                );
                                cap_params.push(
                                    DrawParam::default()
                                        .dest(center + Vec2::new(0.0, -col_h))
                                        .scale(Vec2::splat(radius * 0.28 * erupt))
                                        .color(Color::new(1.0, 0.85, 0.5, 0.55 * erupt)),
                                );
                            }

                            // Pass 2 (ADD): 7 radial crack spokes per fissure.
                            let spokes = 7;
                            let thickness = 2.0 + 1.5 * beat;
                            for s in 0..spokes {
                                let a = s as f32 * std::f32::consts::TAU / spokes as f32 + phase * 0.3;
                                let jitter = (time * 3.0 + s as f32 * 2.1).sin() * 0.15;
                                let dir = Vec2::new((a + jitter).cos(), (a + jitter).sin());
                                let inner = center + dir * radius * 0.35 * open;
                                let outer_len = (radius * (0.9 + 0.15 * glow) * open
                                    - radius * 0.35 * open)
                                    .max(0.0);
                                spoke_params.push(
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
                        }
                    });
                });
            });
        });
    });

    // Issue the batched draws now that all params are collected.
    // Pass 1: dark pit fills, standard ALPHA blend (canvas is already in ALPHA).
    FISSURE_PIT_PARAMS.with(|pp| {
        FISSURE_CIRCLE_INSTANCES.with(|ci| -> ggez::GameResult {
            let pit_params = pp.borrow();
            let mut inst_slot = ci.borrow_mut();
            let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
            instances.set(pit_params.iter().copied());
            canvas.draw_instanced_mesh(unit_circle.clone(), instances, DrawParam::default());
            Ok(())
        })
    })?;

    // Pass 2 (ADD): cores, spokes, geyser columns, geyser caps — all with the same blend.
    let orig_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    FISSURE_CORE_PARAMS.with(|cp| {
        FISSURE_CIRCLE_INSTANCES.with(|ci| -> ggez::GameResult {
            let core_params = cp.borrow();
            if !core_params.is_empty() {
                let mut inst_slot = ci.borrow_mut();
                let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                instances.set(core_params.iter().copied());
                canvas.draw_instanced_mesh(unit_circle.clone(), instances, DrawParam::default());
            }
            Ok(())
        })
    })?;

    FISSURE_GEYSER_PARAMS.with(|gp| {
        FISSURE_LINE_INSTANCES.with(|li| -> ggez::GameResult {
            let geyser_params = gp.borrow();
            if !geyser_params.is_empty() {
                let mut inst_slot = li.borrow_mut();
                let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                instances.set(geyser_params.iter().copied());
                canvas.draw_instanced_mesh(unit_line.clone(), instances, DrawParam::default());
            }
            Ok(())
        })
    })?;

    FISSURE_CAP_PARAMS.with(|cap| {
        FISSURE_CIRCLE_INSTANCES.with(|ci| -> ggez::GameResult {
            let cap_params = cap.borrow();
            if !cap_params.is_empty() {
                let mut inst_slot = ci.borrow_mut();
                let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                instances.set(cap_params.iter().copied());
                canvas.draw_instanced_mesh(unit_circle.clone(), instances, DrawParam::default());
            }
            Ok(())
        })
    })?;

    FISSURE_SPOKE_PARAMS.with(|sp| {
        FISSURE_LINE_INSTANCES.with(|li| -> ggez::GameResult {
            let spoke_params = sp.borrow();
            if !spoke_params.is_empty() {
                let mut inst_slot = li.borrow_mut();
                let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                instances.set(spoke_params.iter().copied());
                canvas.draw_instanced_mesh(unit_line.clone(), instances, DrawParam::default());
            }
            Ok(())
        })
    })?;

    // Per-fissure hazard rims drawn individually (each has a distinct cached-stroke-circle key
    // — radius and thickness depend on per-fissure `reach` and `erupt` — so they can't be
    // instanced together the way same-age chain rings can). There are at most 5, so this is a
    // bounded-cost tail on an otherwise fully-batched pass.
    for (i, &(center, radius, age)) in fissures.iter().enumerate() {
        let open = age.clamp(0.0, 1.0);
        let phase = i as f32 * 1.9;
        let glow = 0.5 + 0.5 * (time * 4.0 + phase).sin();
        let reach = 1.0 + 0.35 * erupt;
        let rim_a = (0.4 + 0.35 * glow + 0.3 * erupt) * open + (1.0 - open) * 0.9;
        let rim = cached_stroke_circle(ctx, (radius * reach) * open.max(0.05), 3.0 + 1.5 * erupt)?;
        canvas.draw(
            &rim,
            DrawParam::default()
                .dest(center)
                .color(Color::new(1.0, 0.5 + 0.3 * beat, 0.12, rim_a.clamp(0.0, 1.0))),
        );
    }

    canvas.set_blend_mode(orig_blend);
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
    // Rocky Shore tide level in [0,1]: 0 = fully ebbed (all rock exposed), 1 = fully flooded. Drives
    // the rising water sheet drawn over the *low* rocks (see MainState::rock_is_low), so the player
    // can read at a glance which chokepoints are opening this beat and time a dash through them.
    tide_fill: f32,
) -> ggez::GameResult {
    // Pass 1 (normal blend): opaque fills for all rocks, batched into one InstanceArray draw.
    // Each rock previously issued 3 canvas.draw(unit_circle) calls (shadow + body + face) plus
    // a cached_stroke_circle rim — up to 5 pools × 3 = 15 fill submissions collapsed to 1.
    // Rims stay individual (each is a different radius stroke mesh, can't share one InstanceArray).
    ROCK_FILL_PARAMS.with(|fill_cell| -> ggez::GameResult {
        let mut fill_params = fill_cell.borrow_mut();
        fill_params.clear();
        for (_i, (center, radius)) in pools.iter().enumerate() {
            let center = *center;
            let radius = *radius;
            // Dark base shadow, offset down a touch to sit the rock on the ground.
            fill_params.push(
                DrawParam::default()
                    .dest(center + Vec2::new(0.0, radius * 0.12))
                    .scale(Vec2::splat(radius))
                    .color(Color::new(0.10, 0.11, 0.13, 0.55)),
            );
            // Main stone body — opaque so it reads as impassable.
            fill_params.push(
                DrawParam::default()
                    .dest(center)
                    .scale(Vec2::splat(radius * 0.96))
                    .color(Color::new(0.34, 0.36, 0.40, 0.95)),
            );
            // Lighter top face, offset up, for a lit-from-above boulder read.
            fill_params.push(
                DrawParam::default()
                    .dest(center - Vec2::new(radius * 0.12, radius * 0.16))
                    .scale(Vec2::splat(radius * 0.6))
                    .color(Color::new(0.52, 0.54, 0.58, 0.9)),
            );
        }
        if !fill_params.is_empty() {
            ROCK_FILL_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                let mut inst_slot = inst_cell.borrow_mut();
                let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                instances.set(fill_params.iter().copied());
                canvas.draw_instanced_mesh(unit_circle.clone(), instances, DrawParam::default());
                Ok(())
            })?;
        }
        Ok(())
    })?;

    // Hard rim per rock — different stroke radius per patch, so stays individual (at most 5 draws).
    for (_i, (center, radius)) in pools.iter().enumerate() {
        let center = *center;
        let radius = *radius;
        let rim = cached_stroke_circle(ctx, radius * 0.96, 3.0)?;
        canvas.draw(
            &rim,
            DrawParam::default()
                .dest(center)
                .color(Color::new(0.18, 0.19, 0.22, 0.9)),
        );
    }

    // Pass 2 (one ADD switch for all rocks): beat-lit mineral sparkles, batched.
    if beat > 0.05 {
        let orig = canvas.blend_mode();
        canvas.set_blend_mode(BlendMode::ADD);
        ROCK_SPARKLE_PARAMS.with(|sparkle_cell| -> ggez::GameResult {
            let mut sparkle_params = sparkle_cell.borrow_mut();
            sparkle_params.clear();
            for (i, (center, radius)) in pools.iter().enumerate() {
                let center = *center;
                let radius = *radius;
                let phase = i as f32 * 2.3;
                let ang = phase;
                let fleck = center + Vec2::new(ang.cos(), ang.sin() * 0.5) * radius * 0.35;
                sparkle_params.push(
                    DrawParam::default()
                        .dest(fleck)
                        .scale(Vec2::splat(4.0 + 3.0 * beat))
                        .color(Color::new(0.7, 0.72, 0.8, 0.25 * beat)),
                );
            }
            if !sparkle_params.is_empty() {
                ROCK_SPARKLE_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                    let mut inst_slot = inst_cell.borrow_mut();
                    let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                    instances.set(sparkle_params.iter().copied());
                    canvas.draw_instanced_mesh(unit_circle.clone(), instances, DrawParam::default());
                    Ok(())
                })?;
            }
            Ok(())
        })?;
        canvas.set_blend_mode(orig);
    }

    // Rocky Shore tide sheet: a translucent water disc swells over each low rock as the sea comes in,
    // draining away as it goes out — the visible read of which chokepoints are opening this beat.
    // Only the low rocks flood (indices where rock_is_low), matching exactly the patches the movement
    // resolver lets you wade through, so what you see is what you can cross. Drawn last so the water
    // sits on top of the stone; the disc grows with tide_fill and brightens/adds a foam rim once it
    // crosses the passable threshold, giving a clear "OPEN NOW" flash the instant the shortcut unlocks.
    if tide_fill > 0.01 {
        let submerged = tide_fill > crate::ROCK_SUBMERGE_LEVEL;
        // Water body: normal-blend translucent disc, scaled by how far the tide has risen.
        ROCK_FILL_PARAMS.with(|fill_cell| -> ggez::GameResult {
            let mut water = fill_cell.borrow_mut();
            water.clear();
            for (i, (center, radius)) in pools.iter().enumerate() {
                if !crate::MainState::rock_is_low(i) {
                    continue;
                }
                let center = *center;
                let radius = *radius;
                // The disc reaches the rock's edge only near full tide; alpha deepens as it rises.
                let cover = 0.55 + 0.45 * tide_fill;
                let alpha = 0.18 + 0.34 * tide_fill;
                water.push(
                    DrawParam::default()
                        .dest(center)
                        .scale(Vec2::splat(radius * cover))
                        .color(Color::new(0.24, 0.52, 0.72, alpha)),
                );
            }
            if !water.is_empty() {
                ROCK_TIDE_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                    let mut inst_slot = inst_cell.borrow_mut();
                    let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                    instances.set(water.iter().copied());
                    canvas.draw_instanced_mesh(unit_circle.clone(), instances, DrawParam::default());
                    Ok(())
                })?;
            }
            Ok(())
        })?;

        // Foam highlight rim on each flooded low rock — brighter once it's actually passable, so the
        // moment the shortcut opens reads as a clear pop of light rather than a gradual fade.
        let rim_alpha = if submerged { 0.85 } else { 0.35 * tide_fill };
        let rim_col = Color::new(0.65, 0.85, 0.95, rim_alpha);
        for (i, (center, radius)) in pools.iter().enumerate() {
            if !crate::MainState::rock_is_low(i) {
                continue;
            }
            let cover = 0.55 + 0.45 * tide_fill;
            let ring = cached_stroke_circle(ctx, radius * cover, 2.5)?;
            canvas.draw(&ring, DrawParam::default().dest(*center).color(rim_col));
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

    // Pass 1 (normal blend): base weed-bed fills for all patches — batched into one
    // InstanceArray draw instead of one canvas.draw(unit_circle) per patch.
    KELP_FILL_PARAMS.with(|fill_cell| -> ggez::GameResult {
        let mut fill_params = fill_cell.borrow_mut();
        fill_params.clear();
        for (i, (center, radius)) in pools.iter().enumerate() {
            let center = *center;
            let radius = *radius;
            let phase = i as f32 * 1.9;
            let breathe = 0.5 + 0.5 * (time * 1.1 + phase).sin();
            let inside = player_center.distance(center) < radius;
            let fill_a = 0.28 + 0.05 * breathe + if inside { 0.12 } else { 0.0 };
            fill_params.push(
                DrawParam::default()
                    .dest(center)
                    .scale(Vec2::splat(radius))
                    .color(Color::new(0.10, 0.30, 0.16, fill_a)),
            );
        }
        if !fill_params.is_empty() {
            KELP_FILL_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                let mut inst_slot = inst_cell.borrow_mut();
                let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                instances.set(fill_params.iter().copied());
                canvas.draw_instanced_mesh(unit_circle.clone(), instances, DrawParam::default());
                Ok(())
            })?;
        }
        Ok(())
    })?;

    // Pass 2 (one ADD switch for all patches): frond strokes batched into one InstanceArray
    // draw, then per-pool neon rims (each a different radius mesh, so they stay individual).
    // Fronds: up to 5 pools × 7 fronds = 35 individual unit_line draws reduced to 1 instanced call.
    let orig = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    KELP_FROND_PARAMS.with(|frond_cell| -> ggez::GameResult {
        let mut frond_params = frond_cell.borrow_mut();
        frond_params.clear();
        for (i, (center, radius)) in pools.iter().enumerate() {
            let center = *center;
            let radius = *radius;
            let phase = i as f32 * 1.9;
            let breathe = 0.5 + 0.5 * (time * 1.1 + phase).sin();
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
                frond_params.push(
                    DrawParam::default()
                        .dest(start)
                        .rotation(rot)
                        .scale(Vec2::new(dist, 2.5))
                        .color(Color::new(0.35, 1.0, 0.55, 0.30 + 0.2 * beat)),
                );
            }
        }
        if !frond_params.is_empty() {
            KELP_FROND_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                let mut inst_slot = inst_cell.borrow_mut();
                let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                instances.set(frond_params.iter().copied());
                canvas.draw_instanced_mesh(unit_line.clone(), instances, DrawParam::default());
                Ok(())
            })?;
        }
        Ok(())
    })?;

    // Pulsing neon rims — one per pool, each a different stroke radius, so they can't share
    // one InstanceArray and stay as individual draws. There are at most 5 of them per frame.
    for (i, (center, radius)) in pools.iter().enumerate() {
        let center = *center;
        let radius = *radius;
        let phase = i as f32 * 1.9;
        let breathe = 0.5 + 0.5 * (time * 1.1 + phase).sin();
        let inside = player_center.distance(center) < radius;
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
    }

    // Funnel-lane streaks: short bright dashes marching along the fixed funnel heading
    // (crate::KELP_FUNNEL_DIR), the same way the Tide Pool current draws its flow. This makes the
    // Kelp routing mechanic legible — the player can see which way the weeds shepherd a panicking
    // crab and set up a train across the lane. Where the water streaks span the whole pool, the
    // kelp streaks hug a narrow central lane (small lateral spread) to read as a *channel* through
    // the weeds rather than a broad drift, matching that the sim only funnels *fleeing* crabs.
    // Batched into one InstanceArray so all pools' streaks cost a single draw call.
    let flow = crate::KELP_FUNNEL_DIR.normalize_or_zero();
    let perp = Vec2::new(-flow.y, flow.x);
    let flow_rot = flow.y.atan2(flow.x);
    KELP_FUNNEL_PARAMS.with(|streak_cell| -> ggez::GameResult {
        let mut streak_params = streak_cell.borrow_mut();
        streak_params.clear();
        const STREAKS: usize = 3;
        for (i, (center, radius)) in pools.iter().enumerate() {
            let center = *center;
            let radius = *radius;
            let phase = i as f32 * 1.9;
            for s in 0..STREAKS {
                // Progress 0..1 along the lane axis, offset per streak and per pool so they stagger.
                let t = (time * 0.55 + s as f32 / STREAKS as f32 + phase * 0.3).fract();
                // Narrow lateral spread so the dashes hug a central channel, not the full width.
                let lateral = ((s as f32 / (STREAKS - 1) as f32) - 0.5) * 0.7 * radius;
                let along = (t - 0.5) * 2.0 * radius;
                let p = center + flow * along + perp * lateral;
                let edge_fade = (t * (1.0 - t) * 4.0).clamp(0.0, 1.0);
                let a = (0.18 + 0.2 * beat) * edge_fade;
                if a > 0.01 {
                    streak_params.push(
                        DrawParam::default()
                            .dest(p)
                            .rotation(flow_rot)
                            .scale(Vec2::new(10.0 + 4.0 * beat, 2.4))
                            .color(Color::new(0.55, 1.0, 0.7, a)),
                    );
                }
            }
        }
        if !streak_params.is_empty() {
            KELP_FUNNEL_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                let mut inst_slot = inst_cell.borrow_mut();
                let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                instances.set(streak_params.iter().copied());
                canvas.draw_instanced_mesh(unit_circle.clone(), instances, DrawParam::default());
                Ok(())
            })?;
        }
        Ok(())
    })?;

    canvas.set_blend_mode(orig);
    Ok(())
}

