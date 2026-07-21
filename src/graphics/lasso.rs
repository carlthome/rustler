//! Rope-and-lasso tether rendering: the persistent rainbow conga rope that links the
//! player to every caught crab (with its on-beat travelling pulse, same-type bond glow,
//! Groove-Gamble overheat, and rival-splice heat band), plus the thrown lasso itself —
//! the wind-up loop, the flying throw/snag/drag/miss beats, and the reel-in rope.
//! Extracted from `graphics/mod.rs` to keep that file navigable; these lean on the shared
//! cached meshes, per-frame instance buffers, and deferred-draw thread-locals defined in
//! the parent module (reached here via `use super::*`).

use super::*;

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
        (MAX_TOTAL_SEGS as f32 / total_links)
            .floor()
            .clamp(4.0, 14.0) as usize
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
    let alpha_base: f32 =
        (0.55 + beat_intensity * 0.4 + heat * 0.25 + length_power * 0.12).min(1.0);

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
                    let phase =
                        t * std::f32::consts::TAU * 1.5 + link_idx as f32 * 0.9 - time * wave_speed;
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
                            let flicker =
                                0.85 + 0.15 * (time * 11.0 + link_idx as f32 * 2.3 + t * 6.0).sin();
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
                            seg_buf.push((
                                prev_point,
                                seg_angle,
                                seg_len,
                                [rr, gg, bb],
                                seg_thick_mult,
                            ));
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
                canvas.draw_instanced_mesh_guarded(
                    unit_line.clone(),
                    instances,
                    DrawParam::default(),
                );
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
                canvas.draw_instanced_mesh_guarded(
                    unit_line.clone(),
                    instances,
                    DrawParam::default(),
                );
                Ok(())
            })?;
            canvas.set_blend_mode(BlendMode::ALPHA);
            Ok(())
        })
    })
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
            let mesh =
                Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh)
        }
    };

    // Rope tension: the reel-in phase strains the rope taut and bright; a miss lets it go slack.
    let (rope_thick, rope_bright): (f32, u8) = match phase {
        LassoDrawPhase::Drag => (3.6, 245), // straining under the weight of the haul
        LassoDrawPhase::Snag => (3.2, 240),
        LassoDrawPhase::Throw => (2.5, 220),
        LassoDrawPhase::Miss => (1.6, 120), // gone limp
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
    let knot_scale = if phase == LassoDrawPhase::Snag {
        5.0 + (1.0 - phase_t) * 5.0
    } else {
        5.0
    };
    let knot_alpha = if phase == LassoDrawPhase::Miss {
        ((1.0 - phase_t) * 240.0) as u8
    } else {
        240
    };
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
            let mesh =
                Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
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
