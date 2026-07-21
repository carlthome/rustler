//! Rhythm/combo/wave heads-up displays drawn over the play field each frame: the beat
//! indicator (approach ring + bar-position pips + on-beat flash), the reef-phrase readout,
//! the wave telegraph, the combo meter, and the off-screen crab radar arrows. Extracted from
//! `graphics/mod.rs` to keep that file navigable; these lean on the shared cached meshes,
//! per-frame instance buffers, and deferred-draw thread-locals defined in the parent module
//! (reached here via `use super::*`).

use super::*;

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
            let mesh =
                Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
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
    let pulse_r = if downbeat_hit {
        pulse_r * 1.35
    } else {
        pulse_r
    };
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
            let mesh =
                Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
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
            let mesh =
                Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
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
            .color(Color::from_rgba(
                halo_rgb.0, halo_rgb.1, halo_rgb.2, halo_alpha,
            )),
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
            DrawParam::default().dest(center).color(Color::from_rgba(
                255,
                170,
                40,
                ((70.0 + a * 120.0) as u8).min(210),
            )),
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
    let glow_color = Color::new(
        tier_color.r,
        tier_color.g,
        tier_color.b,
        tier_color.a * 0.35,
    );

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
                let angle1 =
                    rotation_offset + t1.min(fill_fraction) * fill_fraction * std::f32::consts::TAU;

                // Main arc segment
                let p0 = center + Vec2::new(angle0.cos(), angle0.sin()) * radius;
                let p1 = center + Vec2::new(angle1.cos(), angle1.sin()) * radius;
                let d = p0.distance(p1);
                if d > 0.5 {
                    let rot = (p1 - p0) / d;
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
                    canvas.draw_instanced_mesh_guarded(
                        line.clone(),
                        instances,
                        DrawParam::default(),
                    );
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
                let glow_color = Color::new(
                    r.min(1.0),
                    g.min(1.0),
                    b.min(1.0),
                    0.35 + beat_intensity * 0.15,
                );
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
                    canvas.draw_instanced_mesh_guarded(
                        triangle.clone(),
                        instances,
                        DrawParam::default(),
                    );
                    Ok(())
                })?;
                RADAR_GLOW_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                    let mut inst_slot = inst_cell.borrow_mut();
                    let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                    instances.set(glow_params.iter().copied());
                    canvas.draw_instanced_mesh_guarded(
                        triangle.clone(),
                        instances,
                        DrawParam::default(),
                    );
                    Ok(())
                })?;
            }
            Ok(())
        })
    })?;

    canvas.set_blend_mode(original_blend);
    Ok(())
}
