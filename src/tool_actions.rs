//! Player on-beat action verbs: the tool casts and herd commands the player fires — the Drum Roll
//! beam release, the on-beat Thief snatch, the Call / Groove Call herd lures, the Cycle/Bubble train
//! reposition, the Downbeat Slam ultimate, and the Whistle / Stomp / Lasso / Wave casts (plus the
//! rival-shove the Wave triggers). Split out of `player_tools.rs` so that file keeps the tool *stats*
//! and bot-driver hooks while this one holds the *verbs* — same methods on `impl MainState`, same
//! behaviour, just grouped by subsystem.

use ggez::Context;
use ggez::audio::SoundSource;
use ggez::glam::Vec2;
use rand::Rng;

use crate::constants::*;
use crate::enemies::CrabType;
use crate::state::MainState;

impl MainState {
    /// Fire the Drum Roll: the player released T after banking `drum_roll_hits` on-beat roll hits,
    /// so unleash a focused beam blast down the flashlight's aim. The catch itself is handled by
    /// update_crabs, which widens the flashlight cone/range while `drum_roll_fire` is live (so it
    /// reuses the existing beam catch path rather than a second scan over the crabs) — here we just
    /// arm that window, snapshot the power, and throw the juice/telegraph. Releasing ON the beat
    /// pays a bonus: a fuller charge window and an extra groove/flash kick, so the release is itself
    /// a timed move. Directional (down your aim) and free of the Groove meter, unlike the radial
    /// Downbeat Slam — a skill verb you perform, not a meter you spend.
    pub(crate) fn fire_drum_roll(&mut self) {
        let power = self.drum_roll_hits.min(DRUM_ROLL_MAX);
        if power == 0 {
            return;
        }
        self.drum_roll_power = power;
        let on_beat = self.on_beat_now();
        // A clean release on the beat holds the wide beam open longer (more crabs sweep in) and
        // banks extra groove; a sloppy off-beat release still fires but fades faster.
        self.drum_roll_fire = if on_beat { 1.0 } else { 0.7 };
        let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        // Juice scales with how big the roll was — a full-bar roll released on the beat is a real event.
        let intensity = power as f32 / DRUM_ROLL_MAX as f32;
        self.screen_shake = self.screen_shake.max(8.0 + 10.0 * intensity);
        self.zoom_punch = self.zoom_punch.max(0.05 + 0.06 * intensity);
        self.on_beat_flash = (self.on_beat_flash + if on_beat { 0.6 } else { 0.35 }).min(0.85);
        self.groove = (self.groove + if on_beat { 0.25 } else { 0.12 } * intensity).min(1.0);
        self.beat_intensity = (self.beat_intensity + 1.0).min(2.0);
        // Ring the release so it reads on screen — a gold shockwave down at the player like the Slam.
        let ring_col = if on_beat {
            [1.0, 0.85, 0.35]
        } else {
            [0.9, 0.6, 0.3]
        };
        self.spawn_catch_shockwave(center, ring_col);
        let label = if on_beat {
            format!("DRUM ROLL! x{}", power)
        } else {
            format!("drum roll x{}", power)
        };
        self.floating_texts.spawn(
            label,
            center - Vec2::new(70.0, 70.0),
            30.0 + 6.0 * intensity,
            [1.0, 0.9, 0.4, 1.0],
        );
    }
    /// On-beat Thief shake payoff: an on-beat whistle/stomp that rips a latched Thief loose doesn't
    /// just free the tail — it flings the Thief straight into the train as a bonus catch. Enlists the
    /// crab at `idx` (mark caught, assign the next chain_index, bump chain_count), banks a bonus via
    /// register_catch, and throws celebratory feedback so nailing the timing on the game's newest
    /// chain-threat *reads* and *pays*. Ties the Thief counter into the rhythm layer instead of a flat
    /// toggle. Safe to call after the &mut self.crabs sweep since it takes an index, not a borrow.
    pub(crate) fn snatch_thief_on_beat(&mut self, idx: usize, pos: Vec2) {
        let Some(crab) = self.crabs.get_mut(idx) else {
            return;
        };
        // Guard: only a still-free, still-catchable crab can be enlisted (it may have been grabbed
        // by another effect this same frame).
        if !crab.is_catchable() {
            return;
        }
        let crab_color = crab.crab_color();
        let crab_type = crab.crab_type;
        crab.caught = true;
        crab.chain_index = Some(self.chain_count);
        crab.latch_timer = 0.0;
        crab.fleeing = false;
        crab.startle_timer = 0.0;
        self.chain_count += 1;
        // A meaty bonus — pulling off a rhythm counter on the Thief is worth more than a plain catch.
        self.register_catch(pos, 2);
        let mut rng = crate::rng::rng();
        self.particle_system
            .spawn_catch_effect(pos, crab_color, crab_type, &mut rng);
        self.spawn_catch_shockwave(pos, crab_color);
        self.floating_texts.spawn(
            "THIEF NABBED!".to_string(),
            pos - Vec2::new(60.0, 62.0),
            27.0,
            [0.5, 1.0, 0.6, 1.0],
        );
        // A little extra groove for landing the counter in the pocket.
        self.groove = (self.groove + 0.08).min(1.0);
    }
    /// Issue a rhythm "Call" (F). This is the player's on-beat action that Dancer crabs answer to:
    /// on the beat, it charms every nearby Dancer into hopping TOWARD the player on the next beat
    /// (see the beat-fire Dancer block) instead of fleeing, opening a catch window. Off the beat it
    /// fizzles with a red flash and no charm — the whole point is you have to play in time. A short
    /// cooldown keeps it from being mashed. Turns the rhythm into something the player actively does.
    pub(crate) fn issue_call(&mut self) {
        if self.call_cooldown > 0.0 {
            return;
        }
        let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        self.call_cooldown = crate::CALL_COOLDOWN;
        if self.on_beat_now() {
            // On beat: the Call lands. Charm every nearby free Dancer so it answers on the next beat.
            const CALL_RADIUS: f32 = 420.0;
            let mut answered = 0u32;
            for crab in self.crabs.iter_mut() {
                if crab.caught || !crab.is_dancer() {
                    continue;
                }
                if center.distance(crab.pos) <= CALL_RADIUS {
                    // ~3 beats worth of "come here": won't flee, hops toward the player on the beat.
                    crab.answering_call = 1.6;
                    crab.charm_timer = crab.charm_timer.max(1.6);
                    answered += 1;
                }
            }
            self.call_pulse = 1.0;
            self.call_pulse_center = center;
            self.groove = (self.groove + 0.12).min(1.0);
            self.on_beat_flash = (self.on_beat_flash + 0.3).min(0.7);
            self.beat_intensity = (self.beat_intensity + 0.8).min(2.0);
            let (msg, col) = if answered > 0 {
                ("CALL! Dancers answer".to_string(), [1.0, 0.4, 0.9, 1.0])
            } else {
                ("CALL!".to_string(), [1.0, 0.6, 0.9, 1.0])
            };
            self.floating_texts
                .spawn(msg, center - Vec2::new(70.0, 84.0), 28.0, col);
        } else {
            // Off beat: fizzle. Red flash so the miss reads, no charm applied.
            self.shop_denied = self.shop_denied.max(0.6);
            self.floating_texts.spawn(
                "off beat…".to_string(),
                center - Vec2::new(40.0, 70.0),
                24.0,
                [0.9, 0.4, 0.4, 0.9],
            );
        }
    }
    /// Cycle the train (X) — the reposition verb. Rotates every caught crab one slot toward the
    /// head on the beat: the current head crab wraps around to the tail, and everyone else steps up
    /// one place. This is the player's tool to *arrange* the conga line before banking — it's the
    /// only way to change who rides the two slots that carry weight (the head figurehead: a Golden
    /// gilds the match economy, a Dancer Drum-Major pumps groove; and the tail-guard: an Armored
    /// parked at the tail tanks a Thief steal). A cyclic rotation preserves every same-type
    /// adjacency bond exactly (rotation doesn't break neighbors), so it never scrambles the match-run
    /// rope glow — it only rotates *which* crab occupies the coveted end slots. Rhythm-gated: only
    /// lands on the beat (banks a little groove and reads as a PERFECT), fizzles off-beat, and holds
    /// a short cooldown so it's a timed decision, not a mash. The lerp in the conga-follow pass reels
    /// each crab smoothly to its new trail slot over a few frames, so the rotation slides rather than
    /// teleports — including the head→tail wrap, which sweeps down the line instead of snapping.
    pub(crate) fn cycle_train(&mut self) {
        if self.cycle_cooldown > 0.0 {
            return;
        }
        let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        // Need at least two links for a rotation to mean anything.
        if self.chain_count < 2 {
            return;
        }
        self.cycle_cooldown = 0.7;
        if self.on_beat_now() {
            let n = self.chain_count;
            // One clear, mouse-free action (the game is moving off the mouse entirely). Rotate the
            // whole train one slot toward the head: index i moves to (i + n - 1) % n, so every crab
            // steps up one place and the head (0) wraps around to the tail (n-1). This is the verb
            // in full — the way you choose *which* crab rides the two slots that pay (the head
            // figurehead and the tail-guard), tapped on the beat. A cyclic rotation preserves every
            // same-type adjacency bond, so it never scrambles a match-run; it only slides the ends
            // around. The conga-follow lerp animates the shift (head→tail wrap included) so it sweeps
            // rather than snaps. The head-promote preview ring (see overlays.rs) shows, before you
            // press, exactly which crab this will move up front — no aiming, just read and tap.
            for crab in self.crabs.iter_mut() {
                if let Some(ci) = crab.chain_index {
                    crab.chain_index = Some((ci + n - 1) % n);
                }
            }
            self.groove = (self.groove + 0.1).min(1.0);
            self.on_beat_flash = (self.on_beat_flash + 0.3).min(0.7);
            self.beat_intensity = (self.beat_intensity + 0.8).min(2.0);
            self.zoom_punch = self.zoom_punch.max(0.03);
            self.call_pulse = 1.0;
            self.call_pulse_center = center;
            self.floating_texts.spawn(
                "CYCLE ▸ shift head/tail".to_string(),
                center - Vec2::new(84.0, 84.0),
                28.0,
                [0.4, 0.9, 1.0, 1.0],
            );
        } else {
            // Off beat: fizzle. Red flash so the miss reads, no rotation applied.
            self.shop_denied = self.shop_denied.max(0.6);
            self.floating_texts.spawn(
                "cycle — tap on the beat".to_string(),
                center - Vec2::new(84.0, 70.0),
                24.0,
                [0.9, 0.4, 0.4, 0.9],
            );
        }
    }
    /// Groove Call (V) — arm a FIELD-WIDE, bar-unfolding herd lure. Unlike the whistle (a local,
    /// instant radial yank) or the Dancer Call (F, nearby Dancers only), this reaches the WHOLE field
    /// and its response plays out over the next couple of bars: `groove_call_bars` counts down one per
    /// downbeat, and while it's live every free crab streams toward the player, surging on each beat
    /// (see the pull pass in update_crabs). It's rhythm-quality-gated — a clean on-beat call pulls
    /// harder and lasts longer; an off-beat one barely answers — so timing the call is the skill.
    pub(crate) fn issue_groove_call(&mut self) {
        let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        // Call-and-response ECHO layer: if a call is already streaming the herd in, a fresh V press
        // isn't a new call — it's an *answer* to the DJ. Land it on the beat and the phrase grows:
        // the response extends by a bar and the pull ramps, so keeping the herd flooding in becomes a
        // per-bar rhythm read rather than a one-shot. Miss the beat and the echo just doesn't take
        // (a soft denial) — the live call keeps decaying on its own. This deepens the SAME verb.
        if self.groove_call_bars > 0.0 {
            if self.on_beat_now() {
                self.groove_call_echo += 1;
                // Each clean echo tops the response back up (never past a short cap) and ramps the
                // pull, so a phrase of good answers piles the whole field in harder and longer.
                self.groove_call_bars = (self.groove_call_bars + 1.0).min(3.0);
                self.groove_call_strength = (self.groove_call_strength + 0.35).min(2.0);
                self.groove_call_surge = 1.0;
                self.groove_call_pulse = 1.0;
                self.groove_call_center = center;
                self.groove_call_echo_flash = 1.0;
                self.groove = (self.groove + 0.06).min(1.0);
                self.on_beat_flash = (self.on_beat_flash + 0.25).min(0.7);
                self.beat_intensity = (self.beat_intensity + 0.6).min(2.0);
                self.floating_texts.spawn(
                    format!("ECHO x{}! herd floods in", self.groove_call_echo + 1),
                    center - Vec2::new(110.0, 84.0),
                    26.0,
                    [0.5, 1.0, 0.9, 1.0],
                );
            } else {
                // Off-beat answer: the echo doesn't take. Soft denial, no penalty to the live call.
                self.shop_denied = self.shop_denied.max(0.3);
                self.floating_texts.spawn(
                    "echo… (off beat)".to_string(),
                    center - Vec2::new(60.0, 70.0),
                    20.0,
                    [0.6, 0.75, 0.85, 0.85],
                );
            }
            return;
        }
        if self.groove_call_cooldown > 0.0 {
            return;
        }
        // Gate: need at least some groove to call at all — it's a rhythm skill, not a free button.
        if self.groove < 0.20 {
            self.shop_denied = self.shop_denied.max(0.4);
            self.floating_texts.spawn(
                "need more groove!".to_string(),
                center - Vec2::new(70.0, 70.0),
                20.0,
                [0.6, 0.75, 0.85, 0.9],
            );
            return;
        }
        self.groove_call_center = center;
        self.groove_call_echo = 0;
        // Cooldown spans a few bars so it can't be spammed.
        self.groove_call_cooldown = 4.0;
        self.groove_call_pulse = 1.0;
        // No immediate surge — the surge fires on the next beat, so the call feels rhythmic not instant.
        self.groove_call_surge = 0.0;
        if self.on_beat_now() {
            // Clean on-beat call: lures nearby crabs for two bars. Costs some groove.
            self.groove_call_bars = 2.0;
            self.groove_call_strength = 1.0;
            self.groove = (self.groove - 0.15).max(0.0); // costs groove: rhythm is a resource
            self.on_beat_flash = (self.on_beat_flash + 0.3).min(0.7);
            self.beat_intensity = (self.beat_intensity + 0.8).min(2.0);
            self.floating_texts.spawn(
                "GROOVE CALL! herd answers".to_string(),
                center - Vec2::new(96.0, 84.0),
                28.0,
                [0.4, 0.9, 1.0, 1.0],
            );
        } else {
            // Off beat: very weak pull — barely moves nearby crabs, quick decay, clear miss feedback.
            self.groove_call_bars = 1.0;
            self.groove_call_strength = 0.15; // was 0.4 — enough to see the ring, not flood the field
            self.shop_denied = self.shop_denied.max(0.4);
            self.floating_texts.spawn(
                "call… (off beat)".to_string(),
                center - Vec2::new(60.0, 70.0),
                22.0,
                [0.6, 0.75, 0.85, 0.9],
            );
        }
    }
    /// Downbeat Slam (G) — the Groove-meter ultimate. Only fires when the meter is FULL and the press
    /// lands on the beat: an enormous shockwave erupts from the player and yanks every free crab in a
    /// wide radius straight into the conga train at once (a mass catch), pays out a score bonus, and
    /// drains the whole meter. This is the spectacle payoff for playing in the pocket. Off-beat, or
    /// with a meter that isn't topped out, it fizzles with a distinct message so the miss reads.
    pub(crate) fn downbeat_slam(&mut self, _ctx: &mut Context) {
        let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        // Gate 1: needs high groove (75%+) — earnable without farming to 100%.
        if self.groove < 0.75 {
            self.shop_denied = self.shop_denied.max(0.5);
            self.floating_texts.spawn(
                format!("GROOVE {:.0}% (need 75%)", self.groove * 100.0),
                center - Vec2::new(90.0, 70.0),
                22.0,
                [0.8, 0.85, 0.9, 0.9],
            );
            return;
        }
        // Gate 2: must land on the beat — use a slightly wider window than normal so it feels fair.
        let on_beat_for_slam = self.beat_timer < BEAT_WINDOW * 1.8
            || self.beat_timer > self.beat_interval - BEAT_WINDOW * 1.8;
        if !on_beat_for_slam {
            self.shop_denied = self.shop_denied.max(0.6);
            self.floating_texts.spawn(
                "off beat…".to_string(),
                center - Vec2::new(40.0, 70.0),
                24.0,
                [0.9, 0.4, 0.4, 0.9],
            );
            return;
        }

        // The slam lands. Spend the meter and fire the visuals.
        self.groove = 0.0;
        self.slam_center = center;
        self.slam_radius = 0.0;
        self.slam_active = 0.45;
        self.slam_flash = 1.0;

        // Mass catch: enlist every free, catchable crab within SLAM_RADIUS into the conga train at
        // once. Mirrors the enlist bookkeeping in catch_by_chain (mark caught, assign the next
        // chain_index, bump chain_count) but skips the per-crab spatial-grid scan — the slam is a
        // single big circle, so a flat radius test is fine and only happens on a rare button press.
        let r2 = SLAM_RADIUS * SLAM_RADIUS;
        let mult = self.combo_multiplier();
        let mut rng = crate::rng::rng();
        let mut caught_positions: Vec<Vec2> = Vec::new();
        let mut boss_hits: Vec<(Vec2, CrabType)> = Vec::new();
        let mut golden_hits: Vec<Vec2> = Vec::new();
        for i in 0..self.crabs.len() {
            if !self.crabs[i].is_catchable() {
                continue;
            }
            if self.crabs[i].pos.distance_squared(center) > r2 {
                continue;
            }
            let pos = self.crabs[i].pos;
            let crab_type = self.crabs[i].crab_type;
            let crab_color = self.crabs[i].crab_color();
            self.particle_system
                .spawn_catch_effect(pos, crab_color, crab_type, &mut rng);
            if self.crabs[i].is_boss() {
                boss_hits.push((pos, self.crabs[i].crab_type));
            }
            if self.crabs[i].is_golden() {
                golden_hits.push(pos);
            }
            self.crabs[i].caught = true;
            self.crabs[i].chain_index = Some(self.chain_count);
            self.chain_count += 1;
            caught_positions.push(pos);
        }

        let n = caught_positions.len();
        // Feedback rings for each snatched crab (bounded to keep the vec sane on a huge grab).
        for pos in caught_positions.iter().take(40) {
            self.spawn_catch_shockwave(*pos, [1.0, 0.85, 0.3]);
        }
        for (pos, ctype) in boss_hits {
            self.on_boss_caught(pos, ctype);
        }
        for pos in golden_hits {
            self.on_golden_caught(pos, 0);
        }
        self.chain_join_ripple = n > 0;
        self.check_milestone(&mut rng);

        // Score payout scales with the size of the grab so a well-timed slam into a big herd is a
        // real jackpot, on top of the crabs it adds to your train.
        let bonus = (n as usize * 2).max(1) * mult;
        self.score += bonus;

        // Spectacle: a gold shout, big shake, zoom punch, and a beat-flash bloom.
        self.floating_texts.spawn(
            format!("DOWNBEAT SLAM!  +{}", bonus),
            center - Vec2::new(120.0, 96.0),
            40.0,
            [1.0, 0.9, 0.25, 1.0],
        );
        if n > 0 {
            self.floating_texts.spawn(
                format!("{} snatched!", n),
                center - Vec2::new(60.0, 52.0),
                28.0,
                [1.0, 0.95, 0.6, 1.0],
            );
        }
        self.particle_system
            .spawn_milestone_fireworks(center, n.max(8), &mut rng);
        let a = rng.random_range(0.0_f32..std::f32::consts::TAU);
        self.screen_shake = self.screen_shake.max(28.0);
        self.screen_shake_vel = Vec2::new(a.cos(), a.sin()) * 26.0 * 60.0;
        self.zoom_punch = self.zoom_punch.max(0.12);
        self.hitstop_timer = self.hitstop_timer.max(0.12);
        // Bullet-time the erupting slam ring as it sweeps the field and yanks the herd in.
        self.slowmo_timer = SLOWMO_DURATION;
        self.on_beat_flash = 0.7;
        self.beat_intensity = 2.0;
        let _ = self.sounds.success2.play();
    }

    // --- Tool casts, extracted so both the standalone tool keys (E/R/Q) and the SPACE beat-tap
    // chord (#165) fire the exact same cast. Each self-guards on its own cooldown, so calling it
    // while the tool is recharging is a safe no-op.

    /// Whistle: yank nearby crabs toward the player. Great for skittish Sneaky crabs. On-beat casts
    /// reach farther and pull harder (see reward_on_beat_action).
    pub(crate) fn fire_whistle(&mut self) {
        if self.whistle_cooldown > 0.0 {
            return;
        }
        self.whistle_center =
            self.player_pos + Vec2::new(crate::PLAYER_SIZE / 2.0, crate::PLAYER_SIZE / 2.0);
        self.whistle_radius = 0.0;
        self.whistle_active = 0.4;
        self.whistle_cooldown = self.whistle_cooldown_dur();
        self.whistle_beat_bonus = self.reward_on_beat_action(self.whistle_center, "WHISTLE");
        let _ = self.sounds.whistle_sfx.play();
        self.floating_texts.spawn(
            "WHISTLE!".to_string(),
            self.whistle_center - Vec2::new(48.0, 60.0),
            30.0,
            [1.0, 0.85, 0.35, 1.0],
        );
    }

    /// Stomp: a close-range ground-pound that cracks armored shells wide open. On-beat casts slam
    /// wider and parry a rival splice threading your tail on top of you (see try_defend_steal).
    pub(crate) fn fire_stomp(&mut self) {
        if self.stomp_cooldown > 0.0 {
            return;
        }

        let center =
            self.player_pos + Vec2::new(crate::PLAYER_SIZE / 2.0, crate::PLAYER_SIZE / 2.0);
        self.stomp_center = center;
        self.stomp_radius = 0.0;
        self.stomp_active = 0.32;
        self.stomp_cooldown = self.stomp_cooldown_dur();
        self.screen_shake = 22.0;
        self.screen_shake_vel = Vec2::new(0.0, 1.0) * 22.0 * 60.0;
        self.zoom_punch = self.zoom_punch.max(0.08);
        self.stomp_beat_bonus = self.reward_on_beat_action(center, "STOMP");
        self.try_defend_steal(center, crate::STOMP_DEFEND_RADIUS, "STOMP");
        let _ = self.sounds.stomp_sfx.play();
        self.floating_texts.spawn(
            "STOMP!".to_string(),
            center - Vec2::new(40.0, 60.0),
            30.0,
            [0.85, 0.8, 0.7, 1.0],
        );
    }

    /// Bot-only full-charge lasso release. It uses the same target selection and throw state as a
    /// real mouse release, while keeping the input harness independent of window coordinates.
    pub(crate) fn bot_fire_lasso(&mut self) {
        if self.lasso_phase != crate::state::LassoPhase::Idle {
            return;
        }
        self.lasso_charge = LASSO_MAX_CHARGE_TIME;
        let origin =
            self.player_pos + Vec2::new(crate::PLAYER_SIZE / 2.0, crate::PLAYER_SIZE / 2.0);
        let throw_range = LASSO_MAX_RANGE;
        let aim_point = self.lasso_aim_point(origin, throw_range);
        let to_aim = aim_point - origin;
        let aim_dist = to_aim.length();
        self.lasso_target = if aim_dist > throw_range {
            origin + to_aim / aim_dist * throw_range
        } else if aim_dist > 1.0 {
            aim_point
        } else {
            origin + self.last_dir.normalize_or_zero() * throw_range
        };
        self.lasso_origin = origin;
        self.lasso_timer = LASSO_THROW_TIME;
        self.lasso_phase = crate::state::LassoPhase::Throwing;
        self.lasso_pos = Some(origin);
        self.lasso_charge = 0.0;
    }

    /// Wave: the wide ranged beat pulse. An on-beat cast is the ranged parry — it repels a rival
    /// mid-steal from clear across the lane (see try_defend_steal). Self-guards on beat_wave_active.
    pub(crate) fn fire_wave(&mut self) {
        if self.beat_wave_active {
            return;
        }
        self.beat_wave_active = true;
        self.beat_wave_radius = 0.0;
        let center =
            self.player_pos + Vec2::new(crate::PLAYER_SIZE / 2.0, crate::PLAYER_SIZE / 2.0);
        let on_beat = self.on_beat_action();
        self.reward_on_beat_action(center, "WAVE");
        // Reactive save (unchanged): a rival mid-splice inside reach gets its steal cancelled and a
        // counter-steal window opened — the Wave keeps its "get off my tail!" utility, and the steal
        // playtests drive this helper directly.
        self.try_defend_steal(center, crate::WAVE_DEFEND_RADIUS, "WAVE");
        // Proactive identity (Q rework): the Wave is a SPACE-CLEARING SHOCKWAVE, not only a parry.
        // On the beat it shoves EVERY nearby rival leader outward and briefly stuns it, so you can
        // fire it pre-emptively to break up a crowd or buy room — a distinct job from the Stomp,
        // which stays the precise up-close parry. Off the beat it only nudges (timing is the skill).
        self.wave_shove_rivals(center, crate::WAVE_DEFEND_RADIUS, on_beat);
    }

    /// The Wave's proactive crowd-control: shove every rival King Crab leader within `radius` of
    /// `center` outward and briefly stun it. On-beat casts hit hard (a real knockback + stun that
    /// interrupts a committed hunt so the rival has to re-stalk); off-beat casts barely nudge. This
    /// is what gives Q an identity separate from the Stomp — a readable "get back!" burst you can
    /// throw before a steal even starts, matching the beat-wave ring the player already sees expand.
    fn wave_shove_rivals(&mut self, center: Vec2, radius: f32, on_beat: bool) {
        let margin = 80.0;
        let mut shoved = 0u32;
        for i in 0..self.npc_trains.len() {
            let lead = self.npc_trains[i].leader_pos;
            let d = lead.distance(center);
            if d > radius {
                continue;
            }
            let away = {
                let a = (lead - center).normalize_or_zero();
                if a == Vec2::ZERO { Vec2::X } else { a }
            };
            // Closer rivals get shoved harder; on-beat roughly triples the push.
            let falloff = 0.5 + 0.5 * (1.0 - d / radius).clamp(0.0, 1.0);
            let knock = if on_beat { 150.0 } else { 45.0 } * falloff;
            let mut pushed = lead + away * knock;
            pushed.x = pushed.x.clamp(margin, self.world_width - margin);
            pushed.y = pushed.y.clamp(margin, self.world_height - margin);
            self.npc_trains[i].leader_pos = pushed;
            self.npc_trains[i].leader_vel = away * (knock * 2.0);
            let stun = if on_beat { 0.7 } else { 0.3 };
            self.npc_trains[i].idle_timer = self.npc_trains[i].idle_timer.max(stun);
            self.npc_trains[i].hunt_committed = false;
            if self.catch_shockwaves.len() < 48 {
                self.catch_shockwaves.push((lead, 0.0, [0.4, 0.85, 1.0]));
            }
            shoved += 1;
        }
        // Monotonic tally so a bot can prove the shove path ran (a rival was in range and pushed).
        self.rivals_wave_shoved = self.rivals_wave_shoved.saturating_add(shoved as usize);
        if on_beat {
            if shoved > 0 {
                self.zoom_punch = self.zoom_punch.max(0.05);
                self.screen_shake = self.screen_shake.max(9.0);
                let msg = if shoved == 1 {
                    "WAVE! rival shoved back".to_string()
                } else {
                    format!("WAVE! {shoved} rivals shoved back")
                };
                self.floating_texts.spawn(
                    msg,
                    center - Vec2::new(96.0, 40.0),
                    24.0,
                    [0.45, 0.9, 1.0, 1.0],
                );
            } else {
                // A dead press explains itself instead of feeling broken.
                self.floating_texts.spawn(
                    "WAVE — no rivals in range".to_string(),
                    center - Vec2::new(88.0, 56.0),
                    20.0,
                    [0.6, 0.8, 0.95, 0.85],
                );
            }
        }
    }
}
