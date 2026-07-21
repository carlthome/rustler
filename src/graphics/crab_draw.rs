// Per-crab body rendering: the LOD tiers (Detail), the crowd/size LOD picker, one crab's leg
// geometry and claw geometry, and draw_crab itself — the big instanced-batch renderer that turns
// one EnemyCrab into deferred CRAB_LEG_PARAMS/CRAB_BODY_PARAMS entries (flushed by the parent's
// flush_crab_legs/flush_crab_bodies). Lives in its own file; re-exported so every
// `graphics::draw_crab` / `graphics::set_crab_lod_hint` call-site path is unchanged.
use super::crab_style::{self, ShellPattern};
use super::*;

/// Arena of detail for a crab. Ordered cheap→rich so `min()` picks the cheaper of two caps.
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
pub fn draw_crab(
    ctx: &mut Context,
    _canvas: &mut Canvas,
    crab: &EnemyCrab,
    draw_pos: Vec2,
    beat_phase: f32,
    join_pulse: f32,
    y_lift: f32,
    rotation: f32,
    time: f32,
) -> ggez::GameResult {
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

    // Arena of detail: a calm field renders fully articulated hero crabs; a big swarm or a tiny/
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
    let crab_color = Color::new(
        (r + flash).min(1.0),
        (g + flash).min(1.0),
        (b + flash).min(1.0),
        1.0,
    );
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
    let rim_color = Color::new(
        crab_color.r * 0.32,
        crab_color.g * 0.28,
        crab_color.b * 0.30,
        0.92,
    );
    let belly_color = Color::new(
        crab_color.r * 0.60,
        crab_color.g * 0.53,
        crab_color.b * 0.56,
        0.55,
    );

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
        let center = if side < 0.0 {
            std::f32::consts::PI
        } else {
            0.0
        };
        for j in 0..leg_pairs {
            let frac = (j as f32 + 0.5) / leg_pairs as f32;
            let spread = 0.95 * style.leg_splay;
            let root_ang_body = center + (frac - 0.5) * 2.0 * spread;
            // Leg root on the shell rim, in body space then rotated to world.
            let rb = Vec2::new(
                root_ang_body.cos() * sw * 0.95,
                root_ang_body.sin() * sh * 0.95,
            );
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
            (
                crab.vel.x / vl * eye_radius * 0.4,
                crab.vel.y / vl * eye_radius * 0.4,
            )
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
                    .scale(Vec2::new(
                        size * shadow_scale_x * 0.82,
                        size * shadow_scale_y * 0.82,
                    ))
                    .color(Color::from_rgba(0, 0, 0, shadow_alpha / 2)),
            );
        }
        params.push(
            DrawParam::default()
                .dest(draw_pos + Vec2::new(shadow_offset_x, shadow_offset_y))
                .scale(Vec2::new(
                    size * shadow_scale_x * 0.55,
                    size * shadow_scale_y * 0.55,
                ))
                .color(Color::from_rgba(0, 0, 0, shadow_alpha)),
        );
        // Dark tinted rim just behind the shell — a subtle outline that lifts the crab off busy
        // terrain and off overlapping trainmates. Squashes with the body so it tracks the beat pop.
        params.push(
            DrawParam::default()
                .dest(draw_pos)
                .scale(Vec2::new(
                    sw * shell_squash * 1.15,
                    sh * shell_stretch * 1.15,
                ))
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
                    .scale(Vec2::new(
                        sw * shell_squash * 0.86,
                        sh * shell_stretch * 0.86,
                    ))
                    .rotation(rotation)
                    .color(belly_color),
            );
        }
        // Domed highlight toward the light — the lit crown of the shell.
        params.push(
            DrawParam::default()
                .dest(draw_pos + light_dir * size * 0.15)
                .scale(Vec2::new(
                    sw * 0.62 * shell_squash,
                    sh * 0.62 * shell_stretch,
                ))
                .rotation(rotation)
                .color(dome_color),
        );
        // Rim-light crescent on the lit edge (Full) — the specular sheen of a wet 3D carapace.
        if detail == Detail::Full {
            params.push(
                DrawParam::default()
                    .dest(draw_pos + light_dir * size * 0.30)
                    .scale(Vec2::new(
                        sw * shell_squash * 0.72,
                        sh * shell_stretch * 0.34,
                    ))
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
                    let seam = Color::new(
                        crab_color.r * 0.35,
                        crab_color.g * 0.33,
                        crab_color.b * 0.38,
                        0.85,
                    );
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
                        for (rx, ry) in [
                            (-0.55_f32, -0.4_f32),
                            (0.55, -0.4),
                            (-0.55, 0.4),
                            (0.55, 0.4),
                        ] {
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
                    for (rx, ry) in [
                        (-0.4_f32, -0.35_f32),
                        (0.35, -0.15),
                        (0.0, 0.35),
                        (-0.2, 0.6),
                    ] {
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
                    for (rx, ry, s) in [
                        (-0.3_f32, -0.35_f32, 0.09_f32),
                        (0.25, -0.1, 0.06),
                        (0.1, 0.3, 0.05),
                    ] {
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
        push_claw(
            &mut params,
            wrist_l,
            claw_dir_l,
            crusher_r,
            gape,
            crab_color,
            dome_color,
            light_dir,
            detail,
        );
        push_claw(
            &mut params,
            wrist_r,
            claw_dir_r,
            pincer_r,
            gape,
            crab_color,
            dome_color,
            light_dir,
            detail,
        );

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
                            .dest(
                                ep + rotate_offset(
                                    pdx - eye_radius * 0.25,
                                    pdy - eye_radius * 0.25,
                                ),
                            )
                            .scale(Vec2::splat(catch))
                            .color(Color::new(1.0, 1.0, 1.0, 0.9)),
                    );
                }
            }
        }

        // Planted feet (Full): a small dark bead at each leg tip, shrinking as the leg lifts off
        // the ground mid-step — the read that sells the scuttle.
        if detail == Detail::Full {
            let foot_c = Color::new(
                tibia_color.r * 0.8,
                tibia_color.g * 0.8,
                tibia_color.b * 0.8,
                1.0,
            );
            for lg in legs.iter().take(leg_n) {
                params.push(
                    DrawParam::default()
                        .dest(lg.tibia_tip)
                        .scale(Vec2::splat(
                            size * 0.05 * style.leg_thick * (1.0 - 0.3 * lg.lift),
                        ))
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
            (
                arm_root_r,
                wrist_r,
                (2.4 + 1.6 * style.claw_sym) * style.leg_thick,
            ),
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
