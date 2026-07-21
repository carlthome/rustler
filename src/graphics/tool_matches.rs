//! Tool-vs-enemy "signature match" reaction effects — the one-off visual flourish
//! drawn when a specific player tool acts on a specific crab archetype (beam-vs-Hermit
//! amber shell-drain, beam-vs-Fast cyan pin, whistle/lasso/stomp/magnet matches, shell
//! deflects, cluster pulls). Each reads at a glance so the "right tool / wrong tool"
//! feedback lands on the beat. Extracted from `graphics/mod.rs` to keep that file
//! navigable; these lean on the shared cached meshes and helpers in the parent module
//! (reached here via `use super::*`).

use super::*;

pub fn draw_beam_hermit_match(
    ctx: &mut Context,
    canvas: &mut Canvas,
    hits: &[(Vec2, f32)], // (crab_pos, drain_fraction 0..1)
) -> ggez::GameResult {
    let dot = unit_circle(ctx)?;
    for &(pos, drain) in hits {
        // Amber shell-weakness glow — gets brighter as drain increases
        let glow_r = 28.0 + drain * 18.0;
        let glow_a = 0.15 + drain * 0.35;
        // Draw with BlendMode::ADD so it stacks with the beam glow
        canvas.set_blend_mode(BlendMode::ADD);
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(glow_r))
                .color(Color::new(1.0, 0.55 + drain * 0.2, 0.1, glow_a)),
        );
        // Outer halo ring (bigger, dimmer)
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(glow_r * 1.8))
                .color(Color::new(1.0, 0.4, 0.05, glow_a * 0.3)),
        );
        canvas.set_blend_mode(BlendMode::ALPHA);

        // At high drain (>0.6), add 4 short crack-line sparks radiating outward
        if drain > 0.6 {
            let crack_len = 8.0 + drain * 12.0;
            let crack_a = (drain - 0.6) / 0.4;
            let unit_sq = unit_square(ctx)?;
            for i in 0..4 {
                let angle = i as f32 * std::f32::consts::PI / 2.0 + drain * 2.0;
                let tip = pos + Vec2::new(angle.cos(), angle.sin()) * (glow_r + crack_len * 0.5);
                canvas.draw(
                    unit_sq,
                    DrawParam::default()
                        .dest(tip)
                        .scale(Vec2::new(crack_len, 1.5))
                        .rotation(angle)
                        .offset(Vec2::new(0.5, 0.5))
                        .color(Color::new(1.0, 0.7, 0.2, crack_a * 0.8)),
                );
            }
        }
    }
    Ok(())
}

/// Beam-vs-Fast STRONG-match tell: the flashlight pinning a sprinting Fast crab. Where the
/// beam/Hermit tell flashes amber to say "wrong tool", this flashes icy cyan-white to say "right
/// tool, working" — the light has the fast one gripped. Four brackets clamp inward around the crab
/// (a targeting reticle closing), and on the beat the clamp flares brighter with a ring pulse so the
/// on-beat pin (the hard clamp) reads as the drum-hit version of the graze.
pub fn draw_beam_fast_pin(
    ctx: &mut Context,
    canvas: &mut Canvas,
    hits: &[(Vec2, bool)], // (crab_pos, on_beat)
) -> ggez::GameResult {
    let dot = unit_circle(ctx)?;
    let unit_sq = unit_square(ctx)?;
    // Every draw in this loop uses ADD, so set it once for the whole pass instead of once per
    // hit — ggez only switches the GPU pipeline on a transition between consecutive queued draws,
    // so per-hit toggling was real pipeline-state churn when a beam sweep pins several crabs.
    canvas.set_blend_mode(BlendMode::ADD);
    for &(pos, on_beat) in hits {
        // On-beat is the hard clamp — brighter, tighter, with a ring flash.
        let a = if on_beat { 0.85 } else { 0.5 };
        let clamp_r = if on_beat { 15.0 } else { 20.0 };
        // Soft cyan grip glow under the brackets.
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(clamp_r + 6.0))
                .color(Color::new(0.55, 0.95, 1.0, a * 0.3)),
        );
        // Four L-shaped corner brackets closing in — a reticle clamping the sprinter.
        for i in 0..4 {
            let angle = i as f32 * std::f32::consts::PI / 2.0 + std::f32::consts::FRAC_PI_4;
            let corner = pos + Vec2::new(angle.cos(), angle.sin()) * clamp_r;
            // Two short arms per corner, at right angles, pointing back toward the crab.
            for arm in 0..2 {
                let arm_angle =
                    angle + std::f32::consts::PI + arm as f32 * std::f32::consts::FRAC_PI_2;
                canvas.draw(
                    unit_sq,
                    DrawParam::default()
                        .dest(corner)
                        .scale(Vec2::new(9.0, 2.0))
                        .rotation(arm_angle)
                        .offset(Vec2::new(0.0, 0.5))
                        .color(Color::new(0.7, 0.98, 1.0, a)),
                );
            }
        }
        // On-beat ring flash — the "clamped!" pop.
        if on_beat {
            canvas.draw(
                dot,
                DrawParam::default()
                    .dest(pos)
                    .scale(Vec2::splat(clamp_r * 2.4))
                    .color(Color::new(0.6, 0.95, 1.0, 0.18)),
            );
        }
    }
    canvas.set_blend_mode(BlendMode::ALPHA);
    Ok(())
}

/// Beam-vs-Golden STRONG-match tell: the flashlight *spotlighting the prize*. Where the beam/Fast
/// pin flashes icy cyan (a reticle clamping a sprinter), this glows warm gold — the light has the
/// treasure revealed and reeling. Instead of clamping brackets it draws converging spotlight rays
/// focusing inward on the Golden (the "prize under your beam" read) over a soft gold bloom, and on
/// the beat the rays firm up with a sparkle-ring pop so the on-beat reel reads as the drum-hit
/// version of the graze. Warm gold keeps it distinct from the cyan Fast pin and amber Hermit tell.
pub fn draw_beam_golden_spotlight(
    ctx: &mut Context,
    canvas: &mut Canvas,
    hits: &[(Vec2, bool)], // (crab_pos, on_beat)
) -> ggez::GameResult {
    let dot = unit_circle(ctx)?;
    let unit_sq = unit_square(ctx)?;
    canvas.set_blend_mode(BlendMode::ADD);
    for &(pos, on_beat) in hits {
        // On-beat is the firm reel — brighter, tighter rays, with a sparkle-ring pop.
        let a = if on_beat { 0.8 } else { 0.45 };
        let ray_len = if on_beat { 13.0 } else { 18.0 };
        let ray_from = ray_len + 12.0;
        // Soft gold treasure bloom under the rays.
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(ray_from + 4.0))
                .color(Color::new(1.0, 0.82, 0.3, a * 0.28)),
        );
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(ray_len))
                .color(Color::new(1.0, 0.92, 0.55, a * 0.3)),
        );
        // Six spotlight rays converging inward on the prize — the light "closing on the treasure".
        for i in 0..6 {
            let angle = i as f32 * std::f32::consts::PI / 3.0 + std::f32::consts::FRAC_PI_6;
            let ray_start = pos + Vec2::new(angle.cos(), angle.sin()) * ray_from;
            canvas.draw(
                unit_sq,
                DrawParam::default()
                    .dest(ray_start)
                    .scale(Vec2::new(ray_len - ray_len * 0.15, 2.0))
                    .rotation(angle + std::f32::consts::PI) // point back toward the crab
                    .offset(Vec2::new(0.0, 0.5))
                    .color(Color::new(1.0, 0.88, 0.45, a)),
            );
        }
        // On-beat sparkle-ring pop — the "reeled it!" flash.
        if on_beat {
            canvas.draw(
                dot,
                DrawParam::default()
                    .dest(pos)
                    .scale(Vec2::splat(ray_from * 2.2))
                    .color(Color::new(1.0, 0.85, 0.4, 0.16)),
            );
        }
    }
    canvas.set_blend_mode(BlendMode::ALPHA);
    Ok(())
}

/// Beam-vs-Sneaky STRONG-match tell: the flashlight *exposing the sneak*. The Fast pin clamps icy-cyan
/// brackets INWARD (a reticle trapping a sprinter) and the Golden spotlight draws rays inward (revealing
/// the prize); the Sneaky tell instead reads as a skittish evader caught in the act — teal (its signature
/// colour) short dashes recoiling OUTWARD over a bright exposure flash, so it never reads as the cyan Fast
/// clamp or the warm-gold Golden reel. On the beat the flash firms up with a ring pop — the drum-hit
/// "caught you!" version of the graze.
pub fn draw_beam_sneaky_pin(
    ctx: &mut Context,
    canvas: &mut Canvas,
    hits: &[(Vec2, bool)], // (crab_pos, on_beat)
) -> ggez::GameResult {
    let dot = unit_circle(ctx)?;
    let unit_sq = unit_square(ctx)?;
    canvas.set_blend_mode(BlendMode::ADD);
    for &(pos, on_beat) in hits {
        // On-beat is the firm "exposed!" flash — brighter, with a ring pop.
        let a = if on_beat { 0.8 } else { 0.45 };
        let flash_r = if on_beat { 16.0 } else { 12.0 };
        // Soft teal exposure bloom — the sneak lit up.
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(flash_r + 8.0))
                .color(Color::new(0.47, 0.86, 0.86, a * 0.3)),
        );
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(flash_r))
                .color(Color::new(0.65, 0.98, 0.95, a * 0.35)),
        );
        // Eight short dashes recoiling OUTWARD — the startled sneak flinching in the light.
        for i in 0..8 {
            let angle = i as f32 * std::f32::consts::PI / 4.0;
            let dash_start = pos + Vec2::new(angle.cos(), angle.sin()) * (flash_r + 3.0);
            canvas.draw(
                unit_sq,
                DrawParam::default()
                    .dest(dash_start)
                    .scale(Vec2::new(8.0, 2.0))
                    .rotation(angle) // point outward, away from the crab
                    .offset(Vec2::new(0.0, 0.5))
                    .color(Color::new(0.6, 0.98, 0.92, a)),
            );
        }
        // On-beat ring pop — the "caught you!" flash.
        if on_beat {
            canvas.draw(
                dot,
                DrawParam::default()
                    .dest(pos)
                    .scale(Vec2::splat(flash_r * 2.6))
                    .color(Color::new(0.5, 0.95, 0.9, 0.16)),
            );
        }
    }
    canvas.set_blend_mode(BlendMode::ALPHA);
    Ok(())
}

pub fn draw_stomp_dancer_match(
    ctx: &mut Context,
    canvas: &mut Canvas,
    hits: &[Vec2],
) -> ggez::GameResult {
    let dot = unit_circle(ctx)?;
    let unit_sq = unit_square(ctx)?;
    canvas.set_blend_mode(BlendMode::ADD);
    for &pos in hits {
        // Hot pink disruption ring
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(32.0))
                .color(Color::new(1.0, 0.15, 0.75, 0.5)),
        );
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(20.0))
                .color(Color::new(1.0, 0.3, 0.85, 0.25)),
        );
        // 6 short spikes radiating out — "rhythm broken" symbol
        for i in 0..6 {
            let angle = i as f32 * std::f32::consts::PI / 3.0;
            let spike_start = pos + Vec2::new(angle.cos(), angle.sin()) * 18.0;
            canvas.draw(
                unit_sq,
                DrawParam::default()
                    .dest(spike_start)
                    .scale(Vec2::new(14.0, 2.0))
                    .rotation(angle)
                    .offset(Vec2::new(0.0, 0.5))
                    .color(Color::new(1.0, 0.2, 0.8, 0.7)),
            );
        }
    }
    canvas.set_blend_mode(BlendMode::ALPHA);
    Ok(())
}

pub fn draw_lasso_thief_match(
    ctx: &mut Context,
    canvas: &mut Canvas,
    hits: &[Vec2],
) -> ggez::GameResult {
    let dot = unit_circle(ctx)?;
    canvas.set_blend_mode(BlendMode::ADD);
    for &pos in hits {
        // Bright sly-green central flash
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(22.0))
                .color(Color::new(0.25, 1.0, 0.45, 0.85)),
        );
        // Outer bloom
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(44.0))
                .color(Color::new(0.2, 0.9, 0.4, 0.3)),
        );
        // Inner core pop
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(10.0))
                .color(Color::new(0.8, 1.0, 0.7, 0.95)),
        );
    }
    canvas.set_blend_mode(BlendMode::ALPHA);
    Ok(())
}

/// Steel-blue shell-crack burst when Stomp instantly cracks an Armored crab's shell.
pub fn draw_stomp_armored_crack(
    ctx: &mut Context,
    canvas: &mut Canvas,
    hits: &[Vec2],
) -> ggez::GameResult {
    let dot = unit_circle(ctx)?;
    let sq = unit_square(ctx)?;
    canvas.set_blend_mode(BlendMode::ADD);
    for &pos in hits {
        // Central impact flash
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(26.0))
                .color(Color::new(0.6, 0.78, 1.0, 0.9)),
        );
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(46.0))
                .color(Color::new(0.5, 0.65, 0.92, 0.35)),
        );
        // 6 crack-spikes at 60° intervals, alternating long/short
        for i in 0..6u32 {
            let angle = i as f32 * std::f32::consts::PI / 3.0 + 0.26;
            let len = if i % 2 == 0 { 36.0_f32 } else { 22.0_f32 };
            let tip = pos + Vec2::new(angle.cos(), angle.sin()) * len;
            canvas.draw(
                sq,
                DrawParam::default()
                    .dest(tip)
                    .scale(Vec2::new(len, 2.5))
                    .rotation(angle)
                    .offset(Vec2::new(1.0, 0.5))
                    .color(Color::new(0.72, 0.87, 1.0, 0.82)),
            );
        }
        // Outer dim halo
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(60.0))
                .color(Color::new(0.55, 0.7, 0.95, 0.14)),
        );
    }
    canvas.set_blend_mode(BlendMode::ALPHA);
    Ok(())
}

/// Gold shimmer burst when the Whistle reels in a Golden crab (highest whistle_pull of any type).
pub fn draw_whistle_golden_pull(
    ctx: &mut Context,
    canvas: &mut Canvas,
    hits: &[Vec2],
) -> ggez::GameResult {
    let dot = unit_circle(ctx)?;
    let sq = unit_square(ctx)?;
    canvas.set_blend_mode(BlendMode::ADD);
    for &pos in hits {
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(18.0))
                .color(Color::new(1.0, 0.88, 0.25, 0.75)),
        );
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(35.0))
                .color(Color::new(1.0, 0.82, 0.2, 0.28)),
        );
        // 8 short glint rays
        for i in 0..8u32 {
            let angle = i as f32 * std::f32::consts::PI / 4.0;
            let len = if i % 2 == 0 { 20.0_f32 } else { 12.0_f32 };
            let tip = pos + Vec2::new(angle.cos(), angle.sin()) * len;
            canvas.draw(
                sq,
                DrawParam::default()
                    .dest(tip)
                    .scale(Vec2::new(len, 1.8))
                    .rotation(angle)
                    .offset(Vec2::new(1.0, 0.5))
                    .color(Color::new(1.0, 0.92, 0.4, 0.72)),
            );
        }
    }
    canvas.set_blend_mode(BlendMode::ALPHA);
    Ok(())
}

/// Hot-pink spiral burst when the Whistle reels in a Dancer — rhythm tool meets rhythm crab.
/// Distinct from stomp/Dancer (radial spikes) and whistle/Golden (star glints):
/// uses orbiting arcs to suggest the Dancer's spinning, beat-native movement.
pub fn draw_whistle_dancer_match(
    ctx: &mut Context,
    canvas: &mut Canvas,
    hits: &[Vec2],
) -> ggez::GameResult {
    let dot = unit_circle(ctx)?;
    let sq = unit_square(ctx)?;
    canvas.set_blend_mode(BlendMode::ADD);
    for &pos in hits {
        // Hot pink inner bloom
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(16.0))
                .color(Color::new(1.0, 0.25, 0.80, 0.85)),
        );
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(32.0))
                .color(Color::new(1.0, 0.35, 0.85, 0.30)),
        );
        // 3 arc-pairs orbiting at 120° — looks like musical note beams spinning outward
        for k in 0..3u32 {
            let base_angle = k as f32 * std::f32::consts::TAU / 3.0;
            // Two short dashes per arm: inner and outer, slightly offset for the arc feel
            for (offset, radius, len) in [
                (0.18_f32, 20.0_f32, 12.0_f32),
                (-0.18_f32, 28.0_f32, 9.0_f32),
            ] {
                let angle = base_angle + offset;
                let tip = pos + Vec2::new(angle.cos(), angle.sin()) * radius;
                canvas.draw(
                    sq,
                    DrawParam::default()
                        .dest(tip)
                        .scale(Vec2::new(len, 2.2))
                        .rotation(angle)
                        .offset(Vec2::new(0.5, 0.5))
                        .color(Color::new(1.0, 0.4, 0.9, 0.80)),
                );
            }
        }
    }
    canvas.set_blend_mode(BlendMode::ALPHA);
    Ok(())
}

/// Cyan "flushed out and reeled in" burst when the Whistle sweeps a skittish Sneaky crab — the
/// whistle's flagship soft-RPS match (it folds hardest of all but the Golden, whistle_pull 1.5).
/// Deliberately distinct from whistle/Golden (outward star glints) and whistle/Dancer (orbiting
/// arcs): short ticks at the rim pointing INWARD, converging on the crab, so it reads as "yanked
/// out of hiding and reeled toward you." An on-beat cast (`on_beat` true) flares brighter and wider
/// — the beat-synced version, so gathering skittish crabs on the beat lands like a drum hit.
pub fn draw_whistle_sneaky_match(
    ctx: &mut Context,
    canvas: &mut Canvas,
    hits: &[(Vec2, bool)],
) -> ggez::GameResult {
    let dot = unit_circle(ctx)?;
    let sq = unit_square(ctx)?;
    canvas.set_blend_mode(BlendMode::ADD);
    for &(pos, on_beat) in hits {
        let flare = if on_beat { 1.35 } else { 1.0 };
        // Cyan inner bloom — the skittish crab caught in the sweep.
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(13.0 * flare))
                .color(Color::new(0.5, 0.95, 1.0, 0.80)),
        );
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(28.0 * flare))
                .color(Color::new(0.45, 0.9, 1.0, 0.22)),
        );
        // 6 short "reel-in" ticks at the rim, oriented radially so they read as converging inward
        // on the crab — the sweep dragging the skittish thing toward the player.
        for i in 0..6u32 {
            let angle = i as f32 * std::f32::consts::TAU / 6.0;
            let dir = Vec2::new(angle.cos(), angle.sin());
            let outer = 30.0 * flare;
            let len = 12.0 * flare;
            // Place the dash just inside the rim, centred so it points at pos.
            let mid = pos + dir * (outer - len * 0.5);
            canvas.draw(
                sq,
                DrawParam::default()
                    .dest(mid)
                    .scale(Vec2::new(len, 2.0))
                    .rotation(angle)
                    .offset(Vec2::new(0.5, 0.5))
                    .color(Color::new(0.6, 0.96, 1.0, 0.78)),
            );
        }
    }
    canvas.set_blend_mode(BlendMode::ALPHA);
    Ok(())
}

/// Lime "snapped off your tail" burst when the Whistle rips a latched Thief loose — the whistle's
/// defensive soft-RPS match (whistle_pull 1.3, "yanks it off your tail nicely"). Deliberately
/// distinct from the other three whistle tells (Golden star glints, Dancer orbit arcs, Sneaky
/// inward-converging ticks): a severed-tether motif — two short dashes flying APART past a snapping
/// ring — so it reads as the parasite's grip breaking and releasing from your train, in the green
/// of your own conga line. `on_beat` flags a clean on-beat RIP (bright, wide, the crab is nabbed
/// into the train); off the beat it's a dimmer LOOSEN (the grip only slips a beat), the one Thief
/// counterplay that was still visually silent off the beat — so a flick shows it bit either way.
pub fn draw_whistle_thief_match(
    ctx: &mut Context,
    canvas: &mut Canvas,
    hits: &[(Vec2, bool)],
) -> ggez::GameResult {
    let dot = unit_circle(ctx)?;
    let sq = unit_square(ctx)?;
    canvas.set_blend_mode(BlendMode::ADD);
    for &(pos, on_beat) in hits {
        let flare = if on_beat { 1.35 } else { 0.85 };
        let alpha = if on_beat { 1.0 } else { 0.6 };
        // Lime inner bloom — the Thief reeled back to YOUR side (green = your train, matching the
        // "THIEF NABBED!" callout) so it reads as a gain, not the golden loss of a rival steal.
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(14.0 * flare))
                .color(Color::new(0.5, 1.0, 0.6, 0.80 * alpha)),
        );
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(30.0 * flare))
                .color(Color::new(0.45, 1.0, 0.55, 0.22 * alpha)),
        );
        // 4 severed-tether dash PAIRS: each pair sits on an axis with the two dashes flying apart
        // from the rim outward, so the whole thing reads as bindings snapping open — the latch
        // grip breaking. Distinct from Sneaky's inward reel and Dancer's orbiting arcs.
        for i in 0..4u32 {
            let angle = i as f32 * std::f32::consts::PI / 2.0 + std::f32::consts::FRAC_PI_4;
            let dir = Vec2::new(angle.cos(), angle.sin());
            let inner = 16.0 * flare;
            let len = 13.0 * flare;
            // The dash points radially outward, anchored just past the rim, so it flies AWAY.
            let mid = pos + dir * (inner + len * 0.5);
            canvas.draw(
                sq,
                DrawParam::default()
                    .dest(mid)
                    .scale(Vec2::new(len, 2.2))
                    .rotation(angle)
                    .offset(Vec2::new(0.5, 0.5))
                    .color(Color::new(0.6, 1.0, 0.65, 0.78 * alpha)),
            );
        }
    }
    canvas.set_blend_mode(BlendMode::ALPHA);
    Ok(())
}

/// Cyan magnetic-surge burst when the Lasso snags a Magnet — tells the player that dragging
/// this Magnet through the herd will vacuum up surrounding crabs (the pied-piper power play).
/// Uses concentric field rings and outward arc-lines to read as "magnetic field energised."
pub fn draw_lasso_magnet_match(
    ctx: &mut Context,
    canvas: &mut Canvas,
    hits: &[Vec2],
) -> ggez::GameResult {
    let dot = unit_circle(ctx)?;
    let sq = unit_square(ctx)?;
    canvas.set_blend_mode(BlendMode::ADD);
    for &pos in hits {
        // Inner cyan core bloom
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(18.0))
                .color(Color::new(0.3, 0.9, 1.0, 0.90)),
        );
        // Outer halo ring
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(36.0))
                .color(Color::new(0.2, 0.75, 1.0, 0.30)),
        );
        // Second wide halo — field-line suggestion
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(56.0))
                .color(Color::new(0.1, 0.55, 1.0, 0.12)),
        );
        // 8 short radial arc-lines — magnetic field lines pulling outward
        for k in 0..8u32 {
            let angle = k as f32 * std::f32::consts::TAU / 8.0;
            let inner = 22.0_f32;
            let len = 14.0_f32;
            let tip = pos + Vec2::new(angle.cos(), angle.sin()) * (inner + len * 0.5);
            canvas.draw(
                sq,
                DrawParam::default()
                    .dest(tip)
                    .scale(Vec2::new(len, 2.0))
                    .rotation(angle)
                    .offset(Vec2::new(0.5, 0.5))
                    .color(Color::new(0.4, 1.0, 1.0, 0.85)),
            );
        }
    }
    canvas.set_blend_mode(BlendMode::ALPHA);
    Ok(())
}

/// Warm amber "cinch and heave" burst when the lasso hauls in a heavy Big crab — the Big crab's
/// flagship soft-RPS match. The whistle "shrugs most off" (whistle_pull 0.4), so the loop's physical
/// drag is its intended counter, and this tell says "yes, the lasso is what hauls the heavy one."
/// Deliberately styled HEAVY and earthy — thick amber bars, a tightening double cinch-ring around the
/// big body — so it reads as WEIGHT, distinct from the light spinning magnet field-lines and the
/// converging Sneaky reel-in. `on_beat` throws flare it brighter and wider (an on-beat haul lands
/// like a drum hit).
pub fn draw_lasso_big_match(
    ctx: &mut Context,
    canvas: &mut Canvas,
    hits: &[(Vec2, bool)],
) -> ggez::GameResult {
    let dot = unit_circle(ctx)?;
    let sq = unit_square(ctx)?;
    canvas.set_blend_mode(BlendMode::ADD);
    for &(pos, on_beat) in hits {
        let flare = if on_beat { 1.35 } else { 1.0 };
        // Warm amber inner bloom — the heavy crab caught in the tightening loop.
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(17.0 * flare))
                .color(Color::new(1.0, 0.72, 0.30, 0.85)),
        );
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(34.0 * flare))
                .color(Color::new(0.95, 0.6, 0.22, 0.24)),
        );
        // Double cinch-ring — two concentric rope loops drawn as short chunky arc segments biting
        // in around the big body, the outer slightly wider so it reads as the loop tightening.
        for (ring_r, seg_len, alpha) in [
            (26.0_f32 * flare, 11.0_f32 * flare, 0.85_f32),
            (36.0_f32 * flare, 9.0_f32 * flare, 0.45_f32),
        ] {
            for k in 0..8u32 {
                let angle = k as f32 * std::f32::consts::TAU / 8.0;
                // Tangential segments (rotated +90°) so they trace the ring, not spokes.
                let tip = pos + Vec2::new(angle.cos(), angle.sin()) * ring_r;
                canvas.draw(
                    sq,
                    DrawParam::default()
                        .dest(tip)
                        .scale(Vec2::new(seg_len, 3.5))
                        .rotation(angle + std::f32::consts::FRAC_PI_2)
                        .offset(Vec2::new(0.5, 0.5))
                        .color(Color::new(1.0, 0.66, 0.26, alpha)),
                );
            }
        }
    }
    canvas.set_blend_mode(BlendMode::ALPHA);
    Ok(())
}

/// Hard grey-steel ricochet burst when a lasso throw lands on a still-shelled crab (Armored /
/// shelled Hermit) and the loop slips straight off. This is a WRONG-TOOL "denied" cue — the mirror
/// of the additive-glow strong-match tells: instead of a warm bloom that says "yes, this pairing
/// works," it reads as a cold, hard deflection that says "no, crack the shell first (Stomp), then
/// lasso." Deliberately styled differently — a tight ring plus outward ricochet ticks — so the
/// player instantly distinguishes "wrong tool" from a plain empty whiff. No scolding "X" mark: like
/// the amber beam/Hermit cue it says "try another tool," not "WRONG" (teach, don't punish).
pub fn draw_lasso_shell_deflect(
    ctx: &mut Context,
    canvas: &mut Canvas,
    hits: &[Vec2],
) -> ggez::GameResult {
    let dot = unit_circle(ctx)?;
    let sq = unit_square(ctx)?;
    for &pos in hits {
        // Non-additive so it reads as a hard, matte deflection rather than a glowing "hit."
        // Tight steel ring — the loop bouncing off the shell.
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(30.0))
                .color(Color::new(0.72, 0.76, 0.82, 0.55)),
        );
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(22.0))
                .color(Color::new(0.20, 0.22, 0.26, 0.80)),
        );
        // 6 short ricochet ticks flying outward — the rope snapping back off the shell.
        for i in 0..6u32 {
            let angle = i as f32 * std::f32::consts::PI / 3.0 + 0.5;
            let inner = 24.0_f32;
            let len = 12.0_f32;
            let tip = pos + Vec2::new(angle.cos(), angle.sin()) * (inner + len * 0.5);
            canvas.draw(
                sq,
                DrawParam::default()
                    .dest(tip)
                    .scale(Vec2::new(len, 2.6))
                    .rotation(angle)
                    .offset(Vec2::new(0.5, 0.5))
                    .color(Color::new(0.80, 0.83, 0.88, 0.70)),
            );
        }
    }
    Ok(())
}

/// Cold grey-steel "shell ping" when the whistle's sonic pulse sweeps over a still-shelled crab
/// (Armored / shelled Hermit) and shrugs off — the whistle only "barely nudges it" (enemies.rs).
/// The whistle-side mirror of draw_lasso_shell_deflect: same matte grey-steel "wrong-tool / shelled"
/// vocabulary, so the player learns one read — "grey ping = the shell shrugged the tool, crack it
/// first (Stomp)." Styled distinctly from the lasso version (which throws ricochet ticks *outward*):
/// here the sound waves fold *inward*, arrested at the shell, to read as a pulse repelled rather than
/// a rope snapping back. Teach, don't punish — no scolding "X", just "try another tool."
pub fn draw_whistle_shell_deflect(
    ctx: &mut Context,
    canvas: &mut Canvas,
    hits: &[Vec2],
) -> ggez::GameResult {
    let dot = unit_circle(ctx)?;
    let sq = unit_square(ctx)?;
    for &pos in hits {
        // Matte (non-additive) so it reads as a hard deflection, not a glowing catch.
        // Faint outer sonic ring — the whistle pulse arriving, about to be repelled.
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(38.0))
                .color(Color::new(0.70, 0.74, 0.80, 0.22)),
        );
        // Hard steel shell dome the pulse pings off.
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(26.0))
                .color(Color::new(0.68, 0.72, 0.78, 0.55)),
        );
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(19.0))
                .color(Color::new(0.20, 0.22, 0.26, 0.80)),
        );
        // 5 short sonic chevrons folding INWARD — the sound waves arrested at the shell and
        // bouncing back toward their source, the opposite of the lasso deflect's outward ticks.
        for i in 0..5u32 {
            let angle = i as f32 * std::f32::consts::TAU / 5.0 + 0.3;
            let outer = 34.0_f32;
            let len = 11.0_f32;
            let tip = pos + Vec2::new(angle.cos(), angle.sin()) * outer;
            canvas.draw(
                sq,
                DrawParam::default()
                    .dest(tip)
                    .scale(Vec2::new(len, 2.4))
                    .rotation(angle + std::f32::consts::PI) // point back toward center
                    .offset(Vec2::new(0.0, 0.5))
                    .color(Color::new(0.80, 0.83, 0.88, 0.65)),
            );
        }
    }
    Ok(())
}

pub fn draw_magnet_cluster_pull(
    ctx: &mut Context,
    canvas: &mut Canvas,
    hits: &[Vec2],
) -> ggez::GameResult {
    let dot = unit_circle(ctx)?;
    let sq = unit_square(ctx)?;
    canvas.set_blend_mode(BlendMode::ADD);
    for &pos in hits {
        // Inner core — brighter than the lasso/Magnet burst to read as "active pull"
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(20.0))
                .color(Color::new(0.2, 0.85, 1.0, 0.80)),
        );
        // Outer field boundary ring
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(44.0))
                .color(Color::new(0.15, 0.65, 1.0, 0.22)),
        );
        // Wide soft halo
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(70.0))
                .color(Color::new(0.1, 0.5, 1.0, 0.08)),
        );
        // 8 inward-pointing dashes — start at radius 40, point toward center
        for k in 0..8u32 {
            let angle = k as f32 * std::f32::consts::TAU / 8.0;
            // The dash sits at radius 40 and points inward (rotation = angle + PI)
            let outer = 40.0_f32;
            let len = 16.0_f32;
            let tip = pos + Vec2::new(angle.cos(), angle.sin()) * outer;
            canvas.draw(
                sq,
                DrawParam::default()
                    .dest(tip)
                    .scale(Vec2::new(len, 2.2))
                    .rotation(angle + std::f32::consts::PI) // point toward center
                    .offset(Vec2::new(0.0, 0.5)) // start from outer radius, extend inward
                    .color(Color::new(0.3, 0.95, 1.0, 0.90)),
            );
        }
    }
    canvas.set_blend_mode(BlendMode::ALPHA);
    Ok(())
}
