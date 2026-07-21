//! Player tool stats & support hooks: the beat-window helpers (on_beat_now/action/defend), the
//! whistle/stomp/lasso reach & cooldown stats, the defensive steal parry, on-beat tool rewards and
//! off-beat coaching, plus the bot-driver hooks (event firing, done-check, seek targets, beam-lane
//! ranking). The on-beat action *verbs* themselves — Drum Roll, Call / Groove Call, Cycle, Downbeat
//! Slam, Whistle/Stomp/Wave casts — live in `tool_actions.rs`. Extracted out of `main.rs`'s
//! `impl MainState` — same methods, same behaviour, just grouped by subsystem.

use ggez::audio::SoundSource;
use ggez::glam::Vec2;
use ggez::Context;

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
    /// The on-beat window for the *proactive ranged tool casts* (whistle/stomp/beat-wave/lasso) — a
    /// touch wider than `on_beat_now` (see `ACTION_BEAT_WINDOW`). Those verbs are cooldown-gated, so
    /// you fire them far less often than the dash and a missed on-beat cast stings more; #164 flagged
    /// that as feeling unforgiving. This softens their on-beat read without touching the dash or the
    /// catch, both of which keep the tight `BEAT_WINDOW`. Keep this the single source of truth for
    /// "a ranged tool cast counts as on-beat now".
    pub(crate) fn on_beat_action(&self) -> bool {
        self.beat_timer < ACTION_BEAT_WINDOW
            || self.beat_timer > self.beat_interval - ACTION_BEAT_WINDOW
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
    /// A tool was fired on the beat (tight `BEAT_WINDOW`): bank a "PERFECT!" flash, feed the groove
    /// meter, and punch up the juice. Returns the on-beat multiplier the caller can apply to the
    /// tool's effect (radius/duration), so an on-beat cast simply hits harder. This is the DASH's
    /// path — kept on the tight window so its feel is untouched (Carl, #164). Cooldown-gated ranged
    /// casts use `reward_on_beat_action` (wider window) instead.
    pub(crate) fn reward_on_beat_tool(&mut self, at: Vec2, label: &str) -> f32 {
        // `audible` false: the dash gets NO added drum-pad accent so its feel is untouched (Carl, #164).
        self.reward_on_beat_windowed(at, label, self.on_beat_now(), false)
    }
    /// Like `reward_on_beat_tool` but keyed off the wider `on_beat_action` window — for the
    /// cooldown-gated ranged tool casts (whistle/stomp/beat-wave/lasso) that #164 flagged as feeling
    /// too unforgiving. Same reward/juice; only the on-beat window differs. The dash deliberately does
    /// NOT use this (it stays on the tight `reward_on_beat_tool`).
    pub(crate) fn reward_on_beat_action(&mut self, at: Vec2, label: &str) -> f32 {
        // `audible` true: a ranged cast on the beat plays the crisp woodblock drum-pad accent — the
        // audible "each tool key is a drum pad" reward these cooldown-gated casts were missing (#164).
        self.reward_on_beat_windowed(at, label, self.on_beat_action(), true)
    }
    /// Shared body for the two on-beat tool rewards above — the `on_beat` gate is decided by the
    /// caller (tight `BEAT_WINDOW` for the dash, wider `ACTION_BEAT_WINDOW` for ranged casts).
    /// `audible` latches the on-beat drum-pad accent (ranged casts only; the dash passes false so
    /// its feel stays unchanged); the sound itself fires from the audio pass, which owns `ctx`.
    fn reward_on_beat_windowed(&mut self, at: Vec2, label: &str, on_beat: bool, audible: bool) -> f32 {
        if on_beat {
            self.groove = (self.groove + 0.14).min(1.0);
            self.on_beat_flash = (self.on_beat_flash + 0.35).min(0.7);
            self.beat_intensity = (self.beat_intensity + 1.0).min(2.0);
            self.zoom_punch = self.zoom_punch.max(0.03);
            if audible {
                self.on_beat_tool_sfx = true;
            }
            self.floating_texts.spawn(
                format!("{} PERFECT!", label),
                at - Vec2::new(52.0, 84.0),
                26.0,
                [1.0, 0.95, 0.3, 1.0],
            );
            1.25
        } else {
            // #164 legibility: an off-beat cast used to be *silent*, so the player never learned the
            // cast was even timed — nor which way they missed ("it's not obvious what you're timing").
            // Coach a NEAR miss with a dim "EARLY"/"LATE" tick: it teaches the on-beat window without
            // punishing (teach-don't-punish, like the wrong-tool deflect tells). Two deliberate limits
            // keep it from ever nagging: (1) only the cooldown-gated ranged casts get it (`audible` —
            // the dash stays silent, honouring Carl's "the dash feels good, do NOT touch it"), and
            // (2) only a genuine near miss just outside the window coaches — a wildly-off cast shows
            // nothing, so the cue reads as "so close, adjust" rather than a running scold.
            if audible {
                self.coach_off_beat_cast(at);
            }
            1.0
        }
    }
    /// Spawn the #164 near-miss timing coach for an off-beat *ranged* cast (whistle/stomp/wave/lasso).
    /// `beat_timer` resets to 0 on each beat and counts up to `beat_interval`, so a small timer means
    /// the beat just passed (the press was LATE) and a near-`beat_interval` timer means the next beat
    /// is imminent (the press was EARLY). Only a press within one extra `ACTION_BEAT_WINDOW` past the
    /// on-beat edge coaches — a genuine near miss — and the cue dims as the miss widens, so a close
    /// call reads brighter than a loose one and a wild cast stays silent.
    fn coach_off_beat_cast(&mut self, at: Vec2) {
        let t = self.beat_timer;
        let interval = self.beat_interval;
        // Which side of the beat did the press fall on, and how far past the on-beat window edge?
        let (past_edge, late) = if t < interval * 0.5 {
            (t - ACTION_BEAT_WINDOW, true) // after the beat → late
        } else {
            (interval - t - ACTION_BEAT_WINDOW, false) // before the next beat → early
        };
        // Coach only a near miss: strictly outside the window, within one more window's width.
        if past_edge <= 0.0 || past_edge > ACTION_BEAT_WINDOW {
            return;
        }
        // Brighter the closer the miss (1.0 at the window edge → 0.0 at the band's far end).
        let closeness = 1.0 - past_edge / ACTION_BEAT_WINDOW;
        let alpha = 0.30 + 0.45 * closeness;
        // Muted, cool-grey label (never the gold of a PERFECT) so it reads as a quiet coach, not a hit.
        let (word, col) = if late {
            ("LATE", [0.72, 0.78, 0.88, alpha])
        } else {
            ("EARLY", [0.88, 0.82, 0.70, alpha])
        };
        self.floating_texts.spawn(
            word.to_string(),
            at - Vec2::new(38.0, 74.0),
            17.0,
            col,
        );
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
                BotAction::SeekLasso(on) => {
                    self.bot.as_mut().unwrap().seek_lasso = on;
                }
                BotAction::FireLasso => {
                    self.bot_fire_lasso();
                }
                BotAction::SeekDelivery(on) => {
                    self.bot.as_mut().unwrap().seek_delivery = on;
                }
                BotAction::ForceDelivery => {
                    if self.chain_count > 0 {
                        self.player_pos = self.pen_pos - Vec2::splat(crate::PLAYER_SIZE / 2.0);
                        self.player_vel = Vec2::ZERO;
                        self.try_deliver_train(ctx);
                    }
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
                BotAction::ForceWaveShove => {
                    self.force_wave_shove();
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
                BotAction::ForceGameOver => {
                    self.game_over = true;
                }
                BotAction::Log(msg) => {
                    println!("[BOT t={:.1}] {}", self.time_elapsed, msg);
                }
                BotAction::Assert(check) => {
                    let ok = match &check {
                        BotAssert::GameNotOver => !self.game_over,
                        BotAssert::ChainAtLeast(n) => self.chain_count >= *n,
                        BotAssert::CaughtAtLeast(n) => self.total_caught >= *n,
                        BotAssert::ChordFiredAtLeast(n) => self.chord_tools_fired >= *n,
                        BotAssert::StolenAtLeast(n) => self.crabs_stolen_by_npc >= *n,
                        BotAssert::MaxSingleStealAtMost(n) => self.max_single_steal_by_npc <= *n,
                        BotAssert::StolenByPlayerAtLeast(n) => self.crabs_stolen_by_player >= *n,
                        BotAssert::ParriedAtLeast(n) => self.steals_parried >= *n,
                        BotAssert::WaveShovedAtLeast(n) => self.rivals_wave_shoved >= *n,
                        BotAssert::DodgedAtLeast(n) => self.steals_dodged >= *n,
                        BotAssert::RevengeStealAtLeast(n) => self.revenge_steals >= *n,
                        BotAssert::RivalStealAtLeast(n) => self.rival_vs_rival_steals >= *n,
                        BotAssert::RivalSpillAtLeast(n) => self.rival_spill_crabs >= *n,
                        BotAssert::RivalHuntTelegraphAtLeast(n) => self.rival_hunt_telegraphs >= *n,
                        BotAssert::ScoreAtLeast(n) => self.score >= *n,
                        BotAssert::SelectedNextUnlocked(want) => {
                            self.world_map.as_ref().map_or(false, |m| {
                                m.nodes.get(m.selected + 1).map_or(false, |n| n.unlocked)
                            }) == *want
                        }
                        BotAssert::ShowWorldMap => self.show_world_map,
                        BotAssert::MainMenu => self.show_instructions && !self.show_world_map,
                        BotAssert::TitleMenuReady => {
                            self.show_instructions
                                && !self.show_world_map
                                && !self.sounds.action_music.iter().any(|music| music.playing())
                                && self.sounds.intro_music.playing()
                        }
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
