//! Player tool & ability actions: the on-beat verbs the player performs — whistle/stomp reach
//! stats, the Drum Roll beam release, the Downbeat Slam ultimate, the Call / Groove Call herd
//! lures, Cycle/Bubble train reposition, the defensive steal parry, on-beat tool rewards, plus
//! the bot-driver hooks (event firing, done-check, seek targets). Extracted out of `main.rs`'s
//! `impl MainState` — same methods, same behaviour, just grouped by subsystem.

use ggez::audio::SoundSource;
use ggez::glam::Vec2;
use ggez::Context;
use rand::Rng;

use crate::constants::*;
use crate::controls;
use crate::state::MainState;

impl MainState {
    /// How fast the beam wears down a King Crab / cracks a shell. Ranking the beam lane turns it
    /// into a boss-hunter tool.
    pub(crate) fn boss_drain_rate(&self) -> f32 {
        BOSS_DRAIN_RATE * (1.0 + 0.6 * self.beam_rank as f32)
    }
    /// Grab radius around the lasso tip. Ranking the lasso lane widens each throw so it sweeps up
    /// whole clusters — a chain-catch build.
    pub(crate) fn lasso_tip_radius(&self) -> f32 {
        60.0 + self.lasso_rank as f32 * 22.0
    }
    /// Is *right now* inside the on-beat window? Used to reward firing a tool on the beat —
    /// the same window that gates on-beat catches, so the timing the player already feels for
    /// catching also pays off for whistle/stomp/dash/beat-wave.
    pub(crate) fn on_beat_now(&self) -> bool {
        self.beat_timer < BEAT_WINDOW || self.beat_timer > self.beat_interval - BEAT_WINDOW
    }
    /// The defensive-parry on-beat window: a touch wider than `on_beat_now` (see `DEFEND_BEAT_WINDOW`).
    /// The parry is the one reactive on-beat verb — you're reading a rival's steal telegraph AND the
    /// beat simultaneously — so it gets more forgiveness than the proactive verbs (dash/whistle/stomp
    /// catch), which keep the tight `BEAT_WINDOW`. Keep this the single source of truth for "a parry
    /// works now" so the DEFEND telegraph's hit-now flash can key off the same window (what you see
    /// equals what works).
    pub(crate) fn on_beat_defend(&self) -> bool {
        self.beat_timer < DEFEND_BEAT_WINDOW
            || self.beat_timer > self.beat_interval - DEFEND_BEAT_WINDOW
    }
    /// Downbeat inside the wider defend window — the "big save" parry. `beat_count % 4 == 0` is
    /// beat 1 of a 4/4 bar (same convention as `bar_phase`), gated on the forgiving defend window.
    fn on_downbeat_defend(&self) -> bool {
        self.on_beat_defend() && self.beat_count % 4 == 0
    }
    /// Defensive counter to an armed rival steal — the skill half of the steal fight
    /// (ROADMAP "make the defense a real on-beat play"). When a reach-out tool (Stomp/Wave) is cast
    /// while a rival's splice is armed and its leader sits within `radius` of `center`:
    ///   • ON-BEAT  → PARRY: the telegraph is cancelled, the rival is shoved back off your tail and
    ///     put on a recovery cooldown so it can't instantly re-arm, and the save pays groove + juice.
    ///     A DOWNBEAT cast is the big save — a longer shove and a fuller groove kick. A clean parry
    ///     also flips the exchange: it marks the shoved rival for revenge (the green "chase me" ring),
    ///     so a good defense opens an offensive window — thread the stunned rival's line inside it and
    ///     the steal-back pays the revenge bonus (ROADMAP "a tense back-and-forth... you steal, they
    ///     steal back"). Defense becomes the setup for offense, not just damage prevention.
    ///   • OFF-BEAT → GRAZE: no cancel, but the splice is nudged toward the tail (fewer crabs taken)
    ///     and the rival gets a small shove — sloppy defense still helps, the clean cancel is on-beat.
    /// Returns true if any armed steal was cancelled. "Keys as drum pads": defending is a timed hit.
    pub(crate) fn try_defend_steal(&mut self, center: Vec2, radius: f32, label: &str) -> bool {
        let on_beat = self.on_beat_defend();
        let downbeat = self.on_downbeat_defend();
        let mut parried = false;
        let margin = 80.0;
        for i in 0..self.npc_trains.len() {
            if self.npc_trains[i].steal_threat <= 0.0 {
                continue; // nothing armed on this rival
            }
            let lead = self.npc_trains[i].leader_pos;
            if lead.distance(center) > radius {
                continue; // out of reach of this cast
            }
            let away = (lead - center).normalize_or_zero();
            if on_beat {
                // PARRY: cancel the splice and repel the rival.
                self.npc_trains[i].steal_threat = 0.0;
                self.npc_trains[i].steal_cooldown = if downbeat { 3.4 } else { 2.6 };
                let knock = if downbeat { 170.0 } else { 100.0 };
                let mut pushed = lead + away * knock;
                pushed.x = pushed.x.clamp(margin, self.world_width - margin);
                pushed.y = pushed.y.clamp(margin, self.world_height - margin);
                self.npc_trains[i].leader_pos = pushed;
                self.npc_trains[i].leader_vel = away * (knock * 2.5);
                self.npc_trains[i].idle_timer = if downbeat { 0.9 } else { 0.5 };
                self.steals_parried += 1;
                parried = true;
                // Flip the exchange into offense: mark the shoved rival with the green "chase me"
                // revenge window so a clean parry opens a counter-steal — thread its stunned line
                // inside the window and the steal-back pays the revenge bonus. A downbeat "big save"
                // opens the full window; a normal on-beat parry a shorter one, so the premium save is
                // also the better opening (ROADMAP "you steal, they steal back").
                self.npc_trains[i].revenge_timer = if downbeat {
                    REVENGE_WINDOW
                } else {
                    REVENGE_WINDOW * 0.7
                };
                // Reward: a clean defensive read feeds the groove and streak, like an on-beat catch.
                self.groove = (self.groove + if downbeat { 0.24 } else { 0.16 }).min(1.0);
                self.beat_streak = (self.beat_streak + 1).min(99);
                self.on_beat_flash = (self.on_beat_flash + if downbeat { 0.6 } else { 0.4 }).min(0.9);
                self.beat_intensity = (self.beat_intensity + 1.0).min(2.0);
                self.zoom_punch = self.zoom_punch.max(if downbeat { 0.09 } else { 0.06 });
                self.screen_shake = self.screen_shake.max(if downbeat { 12.0 } else { 8.0 });
                let npc_name = self.npc_trains[i].name.clone();
                let text = if downbeat {
                    format!("BIG SAVE! {} repelled!", npc_name)
                } else {
                    format!("{} SAVE! {} off your tail!", label, npc_name)
                };
                self.floating_texts.spawn(
                    text,
                    center - Vec2::new(96.0, 72.0),
                    if downbeat { 30.0 } else { 26.0 },
                    [0.35, 1.0, 0.85, 1.0],
                );
                // A beat under the save text: point the player at the counter-play the parry opened.
                self.floating_texts.spawn(
                    "COUNTER — rustle 'em back!".to_string(),
                    center - Vec2::new(96.0, 44.0),
                    20.0,
                    [0.45, 1.0, 0.7, 0.95],
                );
                if self.catch_shockwaves.len() < 48 {
                    self.catch_shockwaves.push((lead, 0.0, [0.35, 1.0, 0.85]));
                }
            } else {
                // GRAZE: no cancel, but shove the splice deeper so the rival grabs less, plus a nudge.
                self.npc_trains[i].steal_target = self.npc_trains[i].steal_target.saturating_add(2);
                let mut pushed = lead + away * 34.0;
                pushed.x = pushed.x.clamp(margin, self.world_width - margin);
                pushed.y = pushed.y.clamp(margin, self.world_height - margin);
                self.npc_trains[i].leader_pos = pushed;
                self.floating_texts.spawn(
                    "grazed!".to_string(),
                    center - Vec2::new(42.0, 60.0),
                    18.0,
                    [0.7, 0.95, 0.85, 0.9],
                );
            }
        }
        parried
    }
    /// A tool was fired on the beat: bank a "PERFECT!" flash, feed the groove meter, and punch up
    /// the juice (extra beat flash + a hair of zoom). Returns the on-beat multiplier the caller can
    /// apply to the tool's effect (radius/duration), so an on-beat cast simply hits harder.
    pub(crate) fn reward_on_beat_tool(&mut self, at: Vec2, label: &str) -> f32 {
        if self.on_beat_now() {
            self.groove = (self.groove + 0.14).min(1.0);
            self.on_beat_flash = (self.on_beat_flash + 0.35).min(0.7);
            self.beat_intensity = (self.beat_intensity + 1.0).min(2.0);
            self.zoom_punch = self.zoom_punch.max(0.03);
            self.floating_texts.spawn(
                format!("{} PERFECT!", label),
                at - Vec2::new(52.0, 84.0),
                26.0,
                [1.0, 0.95, 0.3, 1.0],
            );
            1.25
        } else {
            1.0
        }
    }
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
        let mut rng = rand::rng();
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
        self.call_cooldown = 1.5;
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
    /// Find the INTERIOR train link the flashlight is aimed at, if any — the crab a bubble-swap
    /// (X, on beat) would move one slot toward the midpoint. Only interior slots qualify (chain_index
    /// in 1..n-1): the head and tail are the ends the classic rotate already arranges, so aiming at
    /// them (or at nothing) keeps the fallback whole-train rotation. Returns the nearest such link
    /// within a generous pick radius so casual aim lands on the crab you obviously mean.
    pub(crate) fn aimed_interior_link(&self) -> Option<usize> {
        let n = self.chain_count;
        if n < 3 {
            // Fewer than 3 links has no interior slot to repair — nothing to bubble.
            return None;
        }
        const PICK_R2: f32 = 70.0 * 70.0;
        let mut best: Option<(usize, f32)> = None;
        for crab in self.crabs.iter() {
            if let Some(ci) = crab.chain_index {
                if ci >= 1 && ci <= n - 2 {
                    let d2 = (crab.pos - self.mouse_pos).length_squared();
                    if d2 <= PICK_R2 && best.map_or(true, |(_, bd)| d2 < bd) {
                        best = Some((ci, d2));
                    }
                }
            }
        }
        best.map(|(ci, _)| ci)
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
            // CONTEXT-SENSITIVE reposition (same verb, same key, same beat-gate — deepened so the
            // player can *repair the interior*, not just rotate the ends). Aim the flashlight at an
            // interior train link and X BUBBLES that crab one slot toward the midpoint, swapping it
            // with its inward neighbour. That's the missing agency: catch-order can strand two
            // matching crabs on opposite sides of a mismatch, and a whole-train rotation can't fix
            // it — a local swap can, one on-beat press at a time, letting you actively BUILD a
            // centerpiece/sandwich instead of hoping catch-order handed you one. With no interior
            // crab under the aim it falls back to the classic whole-train rotate (arrange the ends).
            let target = self.aimed_interior_link();
            if let Some(ci) = target {
                // Bubble toward the midpoint: below centre step up (toward head), above centre step
                // down (toward tail). Swap the chain_index with the neighbour in that direction so
                // both crabs slide one slot; the conga-follow lerp animates it smoothly for free.
                let mid = (n as f32 - 1.0) / 2.0;
                let other = if (ci as f32) < mid { ci - 1 } else { ci + 1 };
                for crab in self.crabs.iter_mut() {
                    match crab.chain_index {
                        Some(x) if x == ci => crab.chain_index = Some(other),
                        Some(x) if x == other => crab.chain_index = Some(ci),
                        _ => {}
                    }
                }
                self.groove = (self.groove + 0.08).min(1.0);
                self.on_beat_flash = (self.on_beat_flash + 0.3).min(0.7);
                self.beat_intensity = (self.beat_intensity + 0.7).min(2.0);
                self.zoom_punch = self.zoom_punch.max(0.03);
                self.call_pulse = 1.0;
                self.call_pulse_center = center;
                self.floating_texts.spawn(
                    "BUBBLE!".to_string(),
                    center - Vec2::new(56.0, 84.0),
                    28.0,
                    [0.5, 1.0, 0.7, 1.0],
                );
            } else {
                // Rotate one slot toward the head: index i moves to (i + n - 1) % n, i.e. every crab
                // steps up one and the head (0) wraps to the tail (n-1). Preserves adjacency bonds.
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
                    "CYCLE!".to_string(),
                    center - Vec2::new(52.0, 84.0),
                    28.0,
                    [0.4, 0.9, 1.0, 1.0],
                );
            }
        } else {
            // Off beat: fizzle. Red flash so the miss reads, no rotation applied.
            self.shop_denied = self.shop_denied.max(0.6);
            self.floating_texts.spawn(
                "off beat…".to_string(),
                center - Vec2::new(40.0, 70.0),
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
    pub(crate) fn downbeat_slam(&mut self, ctx: &mut Context) {
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
        let mut rng = rand::rng();
        let mut caught_positions: Vec<Vec2> = Vec::new();
        let mut boss_hits: Vec<(Vec2, bool)> = Vec::new();
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
                boss_hits.push((pos, self.crabs[i].is_tide_boss()));
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
        for (pos, is_tide) in boss_hits {
            self.on_boss_caught(pos, is_tide);
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
        let _ = self.sounds.success2.play_detached(ctx);
    }
    /// Reach of the whistle pulse. Ranking the whistle lane grows it toward a full-screen gather.
    pub(crate) fn whistle_max_radius(&self) -> f32 {
        WHISTLE_MAX_RADIUS * (1.0 + 0.28 * self.whistle_rank as f32)
    }
    /// Whistle recharge time. Ranking the whistle lane shortens it (floored so it can't hit zero).
    pub(crate) fn whistle_cooldown_dur(&self) -> f32 {
        WHISTLE_COOLDOWN * (1.0 - 0.14 * self.whistle_rank as f32).max(0.35)
    }
    /// Inward yank speed of the whistle. Ranking the whistle lane pulls even heavy crabs harder.
    pub(crate) fn whistle_pull_speed(&self) -> f32 {
        WHISTLE_PULL_SPEED * (1.0 + 0.2 * self.whistle_rank as f32)
    }
    /// Fire every bot-script event whose timestamp has arrived, releasing last frame's tap keys
    /// first and auto-dismissing any upgrade overlay after. Shared verbatim by the paused-screen
    /// tick (title / world map / game over) and the in-game tick, so assertions and every action
    /// behave identically on every screen. The paused-screen tick used to run a stripped-down copy
    /// that silently dropped Assert events (and never terminated), which hung campaign_tutorial the
    /// instant its tutorial passed and handed control back to the world map.
    pub(crate) fn bot_fire_events(&mut self, ctx: &mut Context) {
        use crate::bot::{BotAction, BotAssert};
        // Release tap keys queued last frame.
        let taps: Vec<_> = self.bot.as_mut().unwrap().tap_release_queue.drain(..).collect();
        for k in taps {
            self.bot.as_mut().unwrap().keys_held.remove(&k);
        }
        // Fire all events whose timestamp has arrived.
        loop {
            let cursor = self.bot.as_ref().unwrap().cursor;
            let len = self.bot.as_ref().unwrap().script.len();
            if cursor >= len {
                break;
            }
            let ev = self.bot.as_ref().unwrap().script[cursor].clone();
            if ev.at > self.time_elapsed {
                break;
            }
            self.bot.as_mut().unwrap().cursor += 1;
            match ev.action {
                BotAction::HoldKey(k) => {
                    self.bot.as_mut().unwrap().keys_held.insert(k);
                }
                BotAction::ReleaseKey(k) => {
                    self.bot.as_mut().unwrap().keys_held.remove(&k);
                }
                BotAction::TapKey(k) => {
                    self.bot.as_mut().unwrap().keys_held.insert(k);
                    self.bot.as_mut().unwrap().tap_release_queue.push(k);
                    // Fire as a synthetic key-down event for menu/dash/campaign actions.
                    controls::handle_key_down_event(self, ctx, Some(k));
                }
                BotAction::MouseMove(p) => {
                    self.bot.as_mut().unwrap().mouse_pos = p;
                }
                BotAction::SeekCatch(on) => {
                    self.bot.as_mut().unwrap().seek_catch = on;
                }
                BotAction::ForceNpcCross => {
                    self.force_npc_cross();
                }
                BotAction::ForcePlayerCross => {
                    self.force_player_cross();
                }
                BotAction::ForceRevengeCross => {
                    self.force_player_revenge();
                }
                BotAction::ForceStealDefense => {
                    self.force_steal_defense();
                }
                BotAction::ForceStealDodge => {
                    self.force_steal_dodge();
                }
                BotAction::ForceRivalCross => {
                    self.force_rival_cross();
                }
                BotAction::ForceRivalHunt => {
                    self.force_rival_hunt();
                }
                BotAction::Log(msg) => {
                    println!("[BOT t={:.1}] {}", self.time_elapsed, msg);
                }
                BotAction::Assert(check) => {
                    let ok = match &check {
                        BotAssert::GameNotOver => !self.game_over,
                        BotAssert::ChainAtLeast(n) => self.chain_count >= *n,
                        BotAssert::CaughtAtLeast(n) => self.total_caught >= *n,
                        BotAssert::StolenAtLeast(n) => self.crabs_stolen_by_npc >= *n,
                        BotAssert::MaxSingleStealAtMost(n) => self.max_single_steal_by_npc <= *n,
                        BotAssert::StolenByPlayerAtLeast(n) => self.crabs_stolen_by_player >= *n,
                        BotAssert::ParriedAtLeast(n) => self.steals_parried >= *n,
                        BotAssert::DodgedAtLeast(n) => self.steals_dodged >= *n,
                        BotAssert::RevengeStealAtLeast(n) => self.revenge_steals >= *n,
                        BotAssert::RivalStealAtLeast(n) => self.rival_vs_rival_steals >= *n,
                        BotAssert::RivalSpillAtLeast(n) => self.rival_spill_crabs >= *n,
                        BotAssert::RivalHuntTelegraphAtLeast(n) => self.rival_hunt_telegraphs >= *n,
                        BotAssert::ScoreAtLeast(n) => self.score >= *n,
                        BotAssert::ShowWorldMap => self.show_world_map,
                        BotAssert::TutorialActive => self.tutorial.is_some(),
                        BotAssert::TutorialDone => self.tutorial.is_none() && self.show_world_map,
                        BotAssert::InGame => {
                            !self.show_instructions && !self.game_over && !self.show_world_map
                        }
                    };
                    if !ok {
                        let msg = format!("ASSERT FAILED at t={:.1}: {:?}", self.time_elapsed, check);
                        println!("FAIL: {}", msg);
                        self.bot.as_mut().unwrap().failed = Some(msg);
                        self.bot.as_mut().unwrap().done = true;
                    }
                }
            }
        }
        // A bot drives input through controls::handle_key_down_event, which doesn't cover the
        // upgrade overlay (its number-key handler lives in key_down_event). So once a catch spree
        // pops the upgrade screen, the bot can't dismiss it and the run stalls. Auto-pick the first
        // upgrade to clear the overlay and let the script finish.
        if self.pending_upgrade {
            self.apply_upgrade(1);
        }
    }

    /// Terminate the bot run: PASS once the script is exhausted, FAIL once the time budget is spent.
    /// Exits the process when done, so it never returns in that case. Shared by both bot ticks.
    pub(crate) fn bot_check_done(&mut self) {
        let t = self.time_elapsed;
        let bot = self.bot.as_mut().unwrap();
        if bot.cursor >= bot.script.len() && !bot.done {
            println!("PASS: script complete at t={:.1}", t);
            bot.done = true;
        }
        if t >= bot.time_limit && !bot.done {
            println!("FAIL: time limit {:.1}s reached", bot.time_limit);
            bot.failed = Some("time limit exceeded".into());
            bot.done = true;
        }
        if bot.done {
            std::process::exit(if bot.failed.is_some() { 1 } else { 0 });
        }
    }

    /// Position of the nearest free, catchable, non-boss crab, if any. The seek-catch bot autopilot
    /// (see BotAction::SeekCatch) whistles this crab into range — driving a reliable catch through
    /// the real game mechanics rather than a blind RNG-dependent sweep.
    pub(crate) fn nearest_catchable_crab_pos(&self) -> Option<Vec2> {
        let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        self.crabs
            .iter()
            .filter(|c| c.is_catchable() && !c.is_boss())
            .min_by(|a, b| {
                center
                    .distance_squared(a.pos)
                    .total_cmp(&center.distance_squared(b.pos))
            })
            .map(|c| c.pos)
    }
    /// Auto-aim point for a lasso throw of the given reach. Snaps the throw toward the nearest
    /// catchable crab within `throw_range` of `origin` so a well-timed release lands a catch
    /// without pixel-perfect aiming — the charge/recharge/on-beat-release mechanic is untouched,
    /// only WHERE the loop flies is assisted. Reuses the same eligibility as the seek-catch
    /// autopilot (free, catchable, non-boss — never the player's own chained crabs). Falls back to
    /// the manual mouse aim point when no catchable crab is in reach, so an empty field still
    /// throws exactly where the player pointed. Mirrors the flashlight's nearest-King-Crab
    /// auto-target: aiming is assisted, timing the release stays the skill.
    pub(crate) fn lasso_aim_point(&self, origin: Vec2, throw_range: f32) -> Vec2 {
        self.nearest_catchable_crab_pos()
            .filter(|p| origin.distance(*p) <= throw_range)
            .unwrap_or(self.mouse_pos)
    }
    /// Where the seek-catch autopilot should walk: a free catchable crab if any exist, otherwise the
    /// nearest crackable shell (Armored / shelled Hermit) so a stomp can pop it open first. Guarantees
    /// the bot always has a target even on the rare all-shelled early roll, so the catch test can't
    /// stall out with nothing catchable in reach.
    pub(crate) fn nearest_seek_target_pos(&self) -> Option<Vec2> {
        self.nearest_catchable_crab_pos().or_else(|| {
            let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            self.crabs
                .iter()
                .filter(|c| {
                    !c.caught
                        && c.boss_health > 0.0
                        && (c.is_armored() || c.is_shelled_hermit())
                })
                .min_by(|a, b| {
                    center
                        .distance_squared(a.pos)
                        .total_cmp(&center.distance_squared(b.pos))
                })
                .map(|c| c.pos)
        })
    }
    /// Reach of the stomp shockwave. Ranking the stomp lane turns a melee tap into a wide slam.
    pub(crate) fn stomp_max_radius(&self) -> f32 {
        STOMP_MAX_RADIUS * (1.0 + 0.3 * self.stomp_rank as f32)
    }
    /// Stomp recharge time. Ranking the stomp lane shortens it (floored) toward spammable.
    pub(crate) fn stomp_cooldown_dur(&self) -> f32 {
        STOMP_COOLDOWN * (1.0 - 0.16 * self.stomp_rank as f32).max(0.3)
    }

    // Beam lane (boss hunter): widens + lengthens the cone and speeds the boss drain (see
    // boss_drain_rate); milestone ranks graft on a disco laser so the lane peaks as a dedicated
    // King-Crab melter rather than a pile of flat numbers. Shared by Beam Focus and the tradeoff
    // cards that also feed the beam lane so the disco-laser milestone fires no matter how it ranks.
    pub(crate) fn rank_beam_lane(&mut self) {
        self.beam_rank += 1;
        self.flashlight.cone_upgrade += 0.18;
        self.flashlight.range_upgrade += 55.0;
        if self.beam_rank == 2 || self.beam_rank == 4 {
            self.flashlight.laser_level += 1;
        }
    }
}
