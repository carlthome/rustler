//! Per-frame movement/charge/drain AI for boss crabs (King Crab, Tide Boss, Reef DJ,
//! Hermit King, Dancer King). Extracted verbatim from `crab_update.rs`'s `update_crabs`
//! per-crab loop to keep that file navigable — the boss branch is a distinct subsystem from
//! the herd flee/attract/magnet logic that surrounds it. Pure structural move, no behaviour
//! change: `update_crabs` builds a `BossUpdateCtx` (the snapshots and event buffers the branch
//! used to read/write inline) and calls [`update_boss_crab`] once per free boss crab, exactly
//! where the inline `if crab.is_boss()` branch used to run.

use ggez::glam::Vec2;
use rand::Rng;

use crate::*;

/// The per-frame inputs and event-collection buffers the boss AI reads and writes. Scalar
/// inputs are snapshots taken once before the per-crab loop (so the boss branch never
/// re-borrows `self` mid-loop); the `&mut` buffers are the same reused scratch vecs the
/// surrounding loop drains after the borrow ends (see the field docs in `state.rs`).
pub(crate) struct BossUpdateCtx<'a> {
    // --- scalar inputs (snapshots) ---
    pub player_pos: Vec2,
    pub flashlight_on: bool,
    pub flashlight_dir: Vec2,
    pub flashlight_range: f32,
    pub flashlight_cone_angle: f32,
    pub boss_drain: f32,
    pub drum_roll_boss_mult: f32,
    pub charge_target: Vec2,
    pub boss_hit_iframes_active: bool,
    pub reef_hot_now: bool,
    pub chain_count: usize,
    pub time_elapsed: f32,
    pub armored_positions: &'a [Vec2],
    // --- outputs written back to the loop ---
    pub reef_on_field: &'a mut bool,
    pub reef_boss_pos: &'a mut Vec2,
    pub reef_hit_landed: &'a mut bool,
    pub boss_broke: &'a mut Vec<Vec2>,
    pub boss_enrages: &'a mut Vec<(Vec2, bool)>,
    pub tide_fires: &'a mut Vec<Vec2>,
    pub tide_swells: &'a mut Vec<Vec2>,
    pub boss_windups: &'a mut Vec<Vec2>,
    pub boss_launches: &'a mut Vec<Vec2>,
    pub boss_charge_dust: &'a mut Vec<(Vec2, Vec2)>,
    pub boss_blocks: &'a mut Vec<(Vec2, Vec2)>,
    pub boss_stuns: &'a mut Vec<Vec2>,
    pub hermit_king_reshells: &'a mut Vec<Vec2>,
}

/// Advance one free boss crab (`crab.is_boss() && !crab.caught`) for this frame: shell drain
/// under the beam, enrage latch, and the per-boss movement state machine (Tide pulse, Reef DJ
/// groove, Hermit King phases, Dancer King drift, King Crab charge). Returns early for each
/// boss subtype exactly where the original inline branch used `continue`.
pub(crate) fn update_boss_crab(
    crab: &mut EnemyCrab,
    dt: f32,
    area: (f32, f32),
    rng: &mut crate::rng::GameRng,
    ctx: &mut BossUpdateCtx,
) {
    if crab.is_rhythm_boss() {
        *ctx.reef_on_field = true;
        *ctx.reef_boss_pos = crab.pos;
    }
    crab.spawn_time += dt;
    // Tick down the King Crab's daze from ramming a parked Armored shell (set in the
    // charge-block pass below). While it's >0 the boss can't wind up a new charge and
    // its shell drains faster (see the stunned-drain boost above).
    if crab.stun_timer > 0.0 {
        crab.stun_timer = (crab.stun_timer - dt).max(0.0);
    }
    let distance = ctx.player_pos.distance(crab.pos);
    let to_crab = (crab.pos - ctx.player_pos).normalize_or_zero();
    let angle_to_crab = ctx.flashlight_dir.angle_to(to_crab).abs();
    let crab_in_light = ctx.flashlight_on
        && distance < ctx.flashlight_range
        && angle_to_crab < ctx.flashlight_cone_angle;
    crab.in_flashlight = crab_in_light;

    // Wearing it down under the beam is unchanged for the King Crab and Tide Boss —
    // the beam is still how you catch them. The Reef DJ is the exception: its shell is
    // call-locked, so the beam only bites while you hold the light on it during a *hot*
    // beat of the phrase it called this bar. Off the phrase (off-beat, or an un-called
    // on-beat) the light does nothing — the whole fight is echoing its pattern back with
    // the light. Enraged, it drains faster on a hit so the finale rewards clean timing.
    let drain_active = crab_in_light
        && !crab.is_hermit_king() // the Hermit King's shell-house stack is beam-proof: only Stomps crack it (see the stomp pass in game_update)
        && (!crab.is_rhythm_boss() || ctx.reef_hot_now);
    if crab.is_rhythm_boss() && crab_in_light && ctx.reef_hot_now && crab.boss_health > 0.0
    {
        *ctx.reef_hit_landed = true;
    }
    if crab.boss_health > 0.0 && drain_active {
        let mut rate = if crab.is_rhythm_boss() {
            // The window is narrow AND only some beats are hot, so per-hit drain is boosted
            // to keep the fight a comparable length to the other bosses; enrage sharpens it.
            ctx.boss_drain * if crab.enraged { 5.0 } else { 3.5 }
        } else {
            ctx.boss_drain
        };
        // Stunned-drain boost: a King Crab reeling from ramming a parked Armored shell
        // takes far more beam damage, so baiting the lunge into a shell then holding the
        // light on the dazed boss is a real damage window — the archetype block fused into
        // the boss fight (see the block pass below where stun_timer is set).
        if crab.is_stunned() {
            rate *= 2.5;
        }
        // Fired Drum Roll blast cracks the shell far faster than a plain held beam
        // (up to 7x at full charge). Multiplies the drain here inside the same
        // `drain_active` gate — so it stacks with a stun window on a King Crab, and
        // still only lands on a hot beat against the Reef DJ. The wide fired cone also
        // makes it easier to keep the light on the boss for the short release window.
        rate *= ctx.drum_roll_boss_mult;
        crab.boss_health -= rate * dt;
        if crab.boss_health <= 0.0 {
            crab.boss_health = 0.0;
            ctx.boss_broke.push(crab.pos);
        }
    }

    // Multi-phase escalation: the moment its health dips below the enrage threshold, the
    // boss enters its final phase. Latch it once so we fire a single dramatic telegraph;
    // the enraged flag then feeds the charge/pulse cadence below to make the climax ramp.
    if !crab.enraged
        && crab.boss_health > 0.0
        && crab.boss_health <= crab.boss_max_health * BOSS_ENRAGE_THRESHOLD
    {
        crab.enraged = true;
        crab.charge_cooldown = crab.charge_cooldown.min(1.0); // snap toward its next move — no lull into the finale
        ctx.boss_enrages.push((crab.pos, crab.is_tide_boss()));
    }

    // The Tide Boss doesn't charge — it drifts and pulses. Distinct threat, distinct
    // counterplay: keep the train *away* from it (spacing) rather than routing out of a
    // charge lane. It reuses charge_cooldown as its pulse timer and BossCharge::Winding
    // to mean "swelling before a pulse".
    if crab.is_tide_boss() {
        let (width, height) = area;
        match crab.charge_state {
            BossCharge::Winding(t) => {
                let nt = t - dt;
                // Rear up and nearly stop while the swell builds — the telegraph window.
                crab.vel = crab.vel.lerp(Vec2::ZERO, 0.2);
                crab.pos += crab.vel * dt;
                crab.charge_state = if nt <= 0.0 {
                    ctx.tide_fires.push(crab.pos);
                    // Enraged: rest far less between pulses so the finale hammers the train.
                    crab.charge_cooldown = if crab.enraged {
                        TIDE_PULSE_COOLDOWN * BOSS_ENRAGE_COOLDOWN_SCALE
                    } else {
                        TIDE_PULSE_COOLDOWN
                    };
                    BossCharge::Idle
                } else {
                    BossCharge::Winding(nt)
                };
            }
            _ => {
                if crab.charge_cooldown > 0.0 {
                    crab.charge_cooldown -= dt;
                }
                // Wander gently toward the train's heart so it stays a looming presence.
                let dir = (ctx.charge_target - crab.pos).normalize_or_zero();
                crab.vel = crab.vel.lerp(dir * crab.speed, 0.02);
                crab.pos += crab.vel * dt;
                // Once rested and there's a train worth scattering, begin swelling a pulse.
                if crab.charge_cooldown <= 0.0 && ctx.chain_count >= 3 {
                    crab.charge_state = BossCharge::Winding(TIDE_PULSE_WINDUP);
                    ctx.tide_swells.push(crab.pos);
                }
            }
        }
        // Bounce off walls, face travel direction (shared with the King Crab tail below).
        if crab.pos.x < 0.0 || crab.pos.x > width - crab.scale {
            crab.vel.x = -crab.vel.x;
            crab.pos.x = crab.pos.x.clamp(0.0, width - crab.scale);
        }
        if crab.pos.y < 0.0 || crab.pos.y > height - crab.scale {
            crab.vel.y = -crab.vel.y;
            crab.pos.y = crab.pos.y.clamp(0.0, height - crab.scale);
        }
        let speed = crab.vel.length();
        if speed > 5.0 {
            let target_angle = crab.vel.y.atan2(crab.vel.x);
            let mut delta = target_angle - crab.facing_angle;
            while delta > std::f32::consts::PI {
                delta -= std::f32::consts::TAU;
            }
            while delta < -std::f32::consts::PI {
                delta += std::f32::consts::TAU;
            }
            crab.facing_angle += delta * (dt * 8.0).min(1.0);
        }
        return;
    }

    // The Reef DJ (rhythm boss) doesn't charge or pulse — it just grooves toward the
    // train's heart as a looming presence while you try to land beat-timed light on it.
    // No hazard state machine at all: the entire threat is the timing test on its shell,
    // so it stays a clean, legible set-piece (hold the light, hit the beat, watch the
    // shell drop a chunk every downbeat).
    if crab.is_rhythm_boss() {
        let (width, height) = area;
        let dir = (ctx.charge_target - crab.pos).normalize_or_zero();
        crab.vel = crab.vel.lerp(dir * crab.speed, 0.02);
        crab.pos += crab.vel * dt;
        if crab.pos.x < 0.0 || crab.pos.x > width - crab.scale {
            crab.vel.x = -crab.vel.x;
            crab.pos.x = crab.pos.x.clamp(0.0, width - crab.scale);
        }
        if crab.pos.y < 0.0 || crab.pos.y > height - crab.scale {
            crab.vel.y = -crab.vel.y;
            crab.pos.y = crab.pos.y.clamp(0.0, height - crab.scale);
        }
        let speed = crab.vel.length();
        if speed > 5.0 {
            let target_angle = crab.vel.y.atan2(crab.vel.x);
            let mut delta = target_angle - crab.facing_angle;
            while delta > std::f32::consts::PI {
                delta -= std::f32::consts::TAU;
            }
            while delta < -std::f32::consts::PI {
                delta += std::f32::consts::TAU;
            }
            crab.facing_angle += delta * (dt * 8.0).min(1.0);
        }
        return;
    }

    // The Hermit King — the shell-house tank. The beam never touches it (drain is
    // gated out above); the whole fight is the Stomp: crack one shell layer per pound
    // (see the stomp pass in game_update). Its movement escalates with its phase:
    // Sturdy = slow lumber at the train, Rattled = fast erratic darts (and only
    // ON-BEAT stomps land), Panicked = a flat-out sprint for the nearest world edge —
    // escape and it drags a fresh shell back in (shell resets, see the edge check).
    if crab.is_hermit_king() {
        let (width, height) = area;
        match hermit_king_phase(crab.boss_health) {
            HermitKingPhase::Sturdy => {
                // Slow lumber toward the train's heart, same looming-presence read as
                // the Reef DJ — a big calm target while any Stomp still cracks it.
                let dir = (ctx.charge_target - crab.pos).normalize_or_zero();
                crab.vel = crab.vel.lerp(dir * crab.speed, 0.02);
                crab.pos += crab.vel * dt;
            }
            HermitKingPhase::Rattled => {
                // Rattled: fast erratic darts on an irregular timer (host_swap_timer,
                // the Hermit's own restless clock, reused for its King). Now only an
                // ON-BEAT stomp lands, so the player must groove AND chase.
                crab.host_swap_timer -= dt;
                if crab.host_swap_timer <= 0.0 {
                    let ang = rng.random_range(0.0_f32..std::f32::consts::TAU);
                    crab.vel = Vec2::new(ang.cos(), ang.sin()) * crab.speed * HERMIT_KING_RATTLED_SPEED_MULT;
                    crab.join_pulse = crab.join_pulse.max(0.7); // squash-pop as it darts
                    crab.host_swap_timer = rng.random_range(0.5..0.9);
                }
                crab.pos += crab.vel * dt;
                // Bleed between darts so it lurches rather than glides.
                crab.vel *= 1.0 - (1.6 * dt).min(0.8);
            }
            HermitKingPhase::Panicked => {
                // One shell left: it bolts for the nearest world edge. Crack it before
                // it escapes — reach the edge and it drags a fresh shell back in.
                let to_left = crab.pos.x;
                let to_right = width - crab.pos.x;
                let to_top = crab.pos.y;
                let to_bottom = height - crab.pos.y;
                let min_d = to_left.min(to_right).min(to_top).min(to_bottom);
                let dir = if min_d == to_left {
                    Vec2::new(-1.0, 0.0)
                } else if min_d == to_right {
                    Vec2::new(1.0, 0.0)
                } else if min_d == to_top {
                    Vec2::new(0.0, -1.0)
                } else {
                    Vec2::new(0.0, 1.0)
                };
                crab.vel = crab.vel.lerp(dir * crab.speed * HERMIT_KING_PANICKED_SPEED_MULT, (4.0 * dt).min(1.0));
                crab.pos += crab.vel * dt;
                // Escaped! It scuttles off the sand line and drags a whole fresh
                // shell-house stack back in — the panic race lost, start over.
                if min_d < 12.0 {
                    crab.boss_health = crab.boss_max_health;
                    crab.enraged = false;
                    crab.host_swap_timer = 0.6;
                    // Turn it back toward the arena so it re-enters lumbering.
                    let center = Vec2::new(width * 0.5, height * 0.5);
                    crab.vel = (center - crab.pos).normalize_or_zero() * crab.speed;
                    ctx.hermit_king_reshells.push(crab.pos);
                }
            }
        }
        // Clamp inside the world (the Panicked sprint aims at the border on purpose).
        crab.pos.x = crab.pos.x.clamp(0.0, width - crab.scale);
        crab.pos.y = crab.pos.y.clamp(0.0, height - crab.scale);
        let speed = crab.vel.length();
        if speed > 5.0 {
            let target_angle = crab.vel.y.atan2(crab.vel.x);
            let mut delta = target_angle - crab.facing_angle;
            while delta > std::f32::consts::PI {
                delta -= std::f32::consts::TAU;
            }
            while delta < -std::f32::consts::PI {
                delta += std::f32::consts::TAU;
            }
            crab.facing_angle += delta * (dt * 8.0).min(1.0);
        }
        return;
    }

    // The Dancer King — the evader. No shell at all (catchable from the first frame);
    // its defence is the beat: every 2 beats it teleports to a mirrored position
    // across the world (see the beat handler), and between blinks it just drifts,
    // keeping a wary distance from the player. Free crabs near it fall entranced and
    // shadow its drift — catch the King ON the beat to bank the whole spellbound court.
    if crab.is_dancer_king() {
        let (width, height) = area;
        // Drift: sway away from a close player, otherwise a slow regal wander toward
        // wherever the herd is thin (world center keeps it on stage).
        let dir = if distance < 300.0 {
            to_crab // away from the player — it won't be walked down flatly
        } else {
            let center = Vec2::new(width * 0.5, height * 0.5);
            (center - crab.pos).normalize_or_zero() * 0.4
                + Vec2::new(
                    (ctx.time_elapsed * 0.7 + crab.beat_phase_offset).cos(),
                    (ctx.time_elapsed * 0.7 + crab.beat_phase_offset).sin(),
                ) * 0.6
        };
        crab.vel = crab.vel.lerp(dir.normalize_or_zero() * crab.speed, 0.03);
        crab.pos += crab.vel * dt;
        if crab.pos.x < 0.0 || crab.pos.x > width - crab.scale {
            crab.vel.x = -crab.vel.x;
            crab.pos.x = crab.pos.x.clamp(0.0, width - crab.scale);
        }
        if crab.pos.y < 0.0 || crab.pos.y > height - crab.scale {
            crab.vel.y = -crab.vel.y;
            crab.pos.y = crab.pos.y.clamp(0.0, height - crab.scale);
        }
        let speed = crab.vel.length();
        if speed > 5.0 {
            let target_angle = crab.vel.y.atan2(crab.vel.x);
            let mut delta = target_angle - crab.facing_angle;
            while delta > std::f32::consts::PI {
                delta -= std::f32::consts::TAU;
            }
            while delta < -std::f32::consts::PI {
                delta += std::f32::consts::TAU;
            }
            crab.facing_angle += delta * (dt * 8.0).min(1.0);
        }
        return;
    }

    // Charge state machine. Holding the beam can't cancel a wind-up — the counterplay is
    // to move the train out of the lane, which is exactly the "route and protect" tension
    // a long conga line should carry.
    match crab.charge_state {
        BossCharge::Idle => {
            if crab.charge_cooldown > 0.0 {
                crab.charge_cooldown -= dt;
            }
            // Lumber toward the train so it stays a closing threat.
            let dir = (ctx.charge_target - crab.pos).normalize_or_zero();
            crab.vel = crab.vel.lerp(dir * crab.speed, 0.02);
            crab.pos += crab.vel * dt;
            // Arm a charge once it's rested, the train is worth scattering, and in range.
            // A stunned (recently-blocked) King Crab can't wind up until the daze passes.
            if crab.charge_cooldown <= 0.0
                && !crab.is_stunned()
                && !ctx.boss_hit_iframes_active
                && ctx.chain_count >= 3
                && crab.pos.distance(ctx.charge_target) < BOSS_CHARGE_ARM_RANGE
            {
                crab.charge_state = BossCharge::Winding(BOSS_WINDUP_TIME);
                ctx.boss_windups.push(crab.pos);
            }
        }
        BossCharge::Winding(t) => {
            let nt = t - dt;
            // Rear back: nearly stop and lean away from the target to sell the wind-up.
            let away = (crab.pos - ctx.charge_target).normalize_or_zero();
            crab.vel = crab.vel.lerp(away * crab.speed * 0.7, 0.15);
            crab.pos += crab.vel * dt;
            crab.charge_state = if nt <= 0.0 {
                // Lock the heading at launch and commit.
                let mut dir = (ctx.charge_target - crab.pos).normalize_or_zero();
                if dir == Vec2::ZERO {
                    dir = Vec2::new(0.0, 1.0);
                }
                // Enraged King Crab lunges harder — a faster, scarier commit in the finale.
                let charge_speed = if crab.enraged {
                    BOSS_CHARGE_SPEED * BOSS_ENRAGE_CHARGE_SPEED_SCALE
                } else {
                    BOSS_CHARGE_SPEED
                };
                crab.vel = dir * charge_speed;
                ctx.boss_launches.push(crab.pos);
                BossCharge::Charging(BOSS_CHARGE_TIME)
            } else {
                BossCharge::Winding(nt)
            };
        }
        BossCharge::Charging(t) => {
            let nt = t - dt;
            crab.pos += crab.vel * dt; // vel stays locked to the launch heading
            ctx.boss_charge_dust.push((crab.pos, crab.vel));
            // Emergent crossover: did the lunge just plow into a free Armored crab's
            // shell? If so the wall wins — the charge aborts here, sparing the tail it
            // was aimed at, and the boss goes on cooldown as if the lunge had spent
            // itself. The Armored crab is knocked back but keeps its shell (it's not
            // caught — it just took the hit). Uses the boss's bulk-widened reach so a
            // near-miss still counts as a block, matching how the tail-snap gives the
            // charge a wide hitbox.
            const BLOCK_REACH: f32 = CRAB_SIZE * 1.1;
            let block_hit = ctx.armored_positions.iter().find(|&&ap| {
                crab.pos.distance(ap) < BLOCK_REACH + crab.scale * CRAB_SIZE * 0.5
            });
            if let Some(&shell_pos) = block_hit {
                crab.charge_cooldown = if crab.enraged {
                    BOSS_CHARGE_COOLDOWN * BOSS_ENRAGE_COOLDOWN_SCALE
                } else {
                    BOSS_CHARGE_COOLDOWN
                };
                // Slamming a shell doesn't just stop the lunge — the impact DAZES the
                // King Crab. For the stun window it can't wind up a new charge and its
                // own shell drains far faster under the beam (see the stunned-drain boost
                // above), turning the Armored block from a purely defensive save into a
                // real damage opportunity: bait the lunge into a parked shell, then hold
                // the light on the reeling boss to chunk it down. Fuses the archetype web
                // with the boss fight, exactly when the fight peaks. Enraged bosses shake
                // it off a little quicker.
                crab.stun_timer = if crab.enraged {
                    BOSS_STUN_DURATION * 0.7
                } else {
                    BOSS_STUN_DURATION
                };
                // Keep it dazed at least as long as it's stunned before it can charge again.
                crab.charge_cooldown = crab.charge_cooldown.max(crab.stun_timer + 0.3);
                // Bounce the boss back off the shell so the stop reads as an impact,
                // not a stall, then let it settle into Idle next.
                crab.vel = -crab.vel.normalize_or_zero() * crab.speed * 0.6;
                ctx.boss_blocks.push((crab.pos, shell_pos));
                ctx.boss_stuns.push(crab.pos);
                crab.charge_state = BossCharge::Idle;
            } else {
                crab.charge_state = if nt <= 0.0 {
                    // Enraged: shorter rest between lunges so the finale keeps the pressure on.
                    crab.charge_cooldown = if crab.enraged {
                        BOSS_CHARGE_COOLDOWN * BOSS_ENRAGE_COOLDOWN_SCALE
                    } else {
                        BOSS_CHARGE_COOLDOWN
                    };
                    crab.vel *= 0.15; // skid to a halt out of the lunge
                    BossCharge::Idle
                } else {
                    BossCharge::Charging(nt)
                };
            }
        }
    }

    // Bounce off the arena walls just like the herd.
    let (width, height) = area;
    if crab.pos.x < 0.0 || crab.pos.x > width - crab.scale {
        crab.vel.x = -crab.vel.x;
        crab.pos.x = crab.pos.x.clamp(0.0, width - crab.scale);
    }
    if crab.pos.y < 0.0 || crab.pos.y > height - crab.scale {
        crab.vel.y = -crab.vel.y;
        crab.pos.y = crab.pos.y.clamp(0.0, height - crab.scale);
    }
    // Smoothly rotate to face travel direction.
    let speed = crab.vel.length();
    if speed > 5.0 {
        let target_angle = crab.vel.y.atan2(crab.vel.x);
        let mut delta = target_angle - crab.facing_angle;
        while delta > std::f32::consts::PI {
            delta -= std::f32::consts::TAU;
        }
        while delta < -std::f32::consts::PI {
            delta += std::f32::consts::TAU;
        }
        crab.facing_angle += delta * (dt * 8.0).min(1.0);
    }
}
