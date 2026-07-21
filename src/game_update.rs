//! The main per-frame `update` tick for `MainState`: fullscreen bring-up, the paused
//! title/menu/world-map clock, and the full in-game simulation step (input, weather, tide,
//! crab/chain/NPC updates, spawns, pattern advance, ambient audio, camera).
//!
//! Extracted verbatim from `main.rs`'s `EventHandler::update` as a single `impl MainState`
//! method (`tick`) to keep that file navigable — `update` now just delegates here. Pure
//! structural move, no behaviour change.

use ggez::conf::FullscreenType;
use ggez::glam::Vec2;
use ggez::winit::keyboard::PhysicalKey;
use rand::Rng;

use crate::*;

pub(crate) fn treasure_groove_level(current: f32, on_beat: bool) -> f32 {
    if on_beat {
        1.0
    } else {
        current.max(0.5)
    }
}

impl MainState {
    pub(crate) fn tick(&mut self, ctx: &mut Context) -> GameResult {
        if !self.fullscreen_applied {
            // current_monitor() can still be None on the very first tick, so keep retrying
            // until it resolves instead of only trying once.
            if ctx.gfx.window().current_monitor().is_some() {
                // FullscreenType::Desktop removes decorations and resizes the window to cover
                // the monitor without using the OS native fullscreen API, so it works the same
                // on macOS, Wayland, and Windows. It also reconfigures the wgpu surface
                // internally so we don't need to call set_drawable_size separately.
                ctx.gfx.set_fullscreen(FullscreenType::Desktop)?;
                self.fullscreen_applied = true;
            }
        }

        if self.show_instructions || self.show_world_map || self.game_over {
            // The run just ended — bank its result into the persistent career exactly once.
            // Every game_over set-site funnels through here on the next tick, so one guarded
            // call covers them all.
            if self.game_over {
                self.record_run();
            }
            // Keep a lightweight clock ticking so the title/menu screen can animate its
            // background, marching crabs, and pulsing prompt even though the main simulation
            // is paused here.
            let mdt = self.frame_dt(ctx);
            self.menu_time += mdt;
            // In bot mode, time_elapsed must advance and bot events must fire even while a paused
            // screen is showing — e.g. TapKey(Space) at t=0.5 dismisses the title screen, and a
            // tutorial that passes hands control back to the world map where the script's remaining
            // asserts still need to run and terminate. This uses the SAME bot tick as the in-game
            // path (fire events incl. asserts, then check done), so completion behaves identically on
            // every screen — the old stripped-down tick here dropped asserts and never terminated,
            // which hung campaign_tutorial the instant its tutorial returned to the world map.
            if self.bot.is_some() {
                self.time_elapsed += mdt.min(0.1) * self.time_scale;
                self.bot_fire_events(ctx);
                self.bot_check_done();
            }
            // Decay the perk-shop buy/deny flashes so they're a brief pop, not a stuck glow.
            self.shop_flash = (self.shop_flash - mdt * 2.5).max(0.0);
            self.shop_denied = (self.shop_denied - mdt * 2.5).max(0.0);
            // Auto-hide the world-map "skip ahead" warning after ~2s of no second Confirm.
            if let Some(map) = &mut self.world_map {
                map.tick_skip_warning(mdt);
            }
            return Ok(());
        }

        // Clamp raw delta before scaling to prevent a large first-frame hitch (shader compile,
        // audio decode, BPM detection) from collapsing the bot script's timed hold/release
        // sequence — and to guard against the general "spiral of death" when the game falls behind.
        // update_weather uses its own raw delta below and is deliberately left unclamped.
        let mut dt = self.frame_dt(ctx).min(0.1) * self.time_scale;

        // Clear strong-match hit buffers so draw_game sees only THIS frame's events.
        self.beam_hermit_hits_buf.clear();
        self.beam_fast_hits_buf.clear();
        self.beam_golden_hits_buf.clear();
        self.beam_sneaky_hits_buf.clear();
        self.stomp_dancer_hits_buf.clear();
        self.lasso_thief_hits_buf.clear();
        self.lasso_magnet_hits_buf.clear();
        self.lasso_big_hits_buf.clear();
        self.lasso_shell_deflect_hits_buf.clear();
        self.whistle_shell_deflect_hits_buf.clear();
        self.magnet_cluster_hits_buf.clear();
        self.stomp_armored_hits_buf.clear();
        self.whistle_golden_hits_buf.clear();
        self.whistle_dancer_hits_buf.clear();
        self.whistle_sneaky_hits_buf.clear();
        self.whistle_thief_hits_buf.clear();

        // Perf instrumentation (debug builds only): track average + worst frame time over a
        // rolling ~2s window and print it, so optimization passes have real numbers instead of
        // guessing from code inspection. Uses the same per-update dt ggez already measured, so
        // this is just a couple of float adds — no extra timing calls or allocations.
        #[cfg(debug_assertions)]
        {
            self.perf_frame_count += 1;
            self.perf_time_accum += dt;
            self.perf_worst_frame = self.perf_worst_frame.max(dt);
            if self.perf_time_accum >= 2.0 {
                let avg_ms = (self.perf_time_accum / self.perf_frame_count as f32) * 1000.0;
                let worst_ms = self.perf_worst_frame * 1000.0;
                // Crab count alongside the timing so a future optimizer pass can correlate a
                // frame-time regression with herd/train size instead of guessing — cheap: reuses
                // self.crabs.len() and self.chain_count, no extra scan. NPC follower total added
                // since train follower count drives both path_history size and draw_npc_conga_train cost.
                let npc_followers: usize =
                    self.npc_trains.iter().map(|n| n.follower_types.len()).sum();
                println!(
                    "[perf] {} frames in {:.1}s — avg {:.2}ms ({:.0} fps), worst {:.2}ms — {} crabs ({} chained, {} npc followers)",
                    self.perf_frame_count,
                    self.perf_time_accum,
                    avg_ms,
                    1000.0 / avg_ms,
                    worst_ms,
                    self.crabs.len(),
                    self.chain_count,
                    npc_followers,
                );
                // Stash for the on-screen overlay (see draw()) so the number is visible during
                // play too, not just in a terminal that may not be in view.
                self.perf_last_avg_ms = avg_ms;
                self.perf_last_worst_ms = worst_ms;
                self.perf_last_fps = 1000.0 / avg_ms;
                self.perf_frame_count = 0;
                self.perf_time_accum = 0.0;
                self.perf_worst_frame = 0.0;
            }
        }

        // Hitstop: freeze the whole simulation for a few frames right after a catch so the
        // impact snaps instead of sliding past. draw() still runs each frame, so the frozen
        // moment is fully rendered — the classic Vampire-Survivors-style "punch". Pause every
        // looping music source with the beat clock; the normal mixers resume them from the same
        // sample afterward, so repeated dash-catches cannot accumulate melody/grid drift.
        if self.hitstop_timer > 0.0 {
            self.pause_gameplay_music();
            self.hitstop_timer = (self.hitstop_timer - dt).max(0.0);
            return Ok(());
        }

        // Advance the master groove before cinematic slow-motion dilates `dt`. World motion can
        // stretch for drama, but the backing loop, live percussion and tool windows stay locked.
        self.update_master_beat(ctx, dt);

        // Cinematic slow-motion on the biggest climax moments (boss catch, Downbeat Slam). The
        // timer decays on REAL time so the effect is always the same wall-clock length, but the
        // whole rest of the sim runs on a dilated `dt` that eases from ~35% speed back up to full
        // as the timer runs out — a smooth bullet-time ramp, not a hard freeze. World animation
        // and particles slow together, while the master groove above deliberately keeps playing
        // at full speed so the player's timing contract never bends with a cinematic effect.
        if self.slowmo_timer > 0.0 {
            self.slowmo_timer = (self.slowmo_timer - dt).max(0.0);
            // Ease-out: strong slow at the start, ramping back to real speed as it clears.
            let ramp = 1.0 - (self.slowmo_timer / SLOWMO_DURATION).clamp(0.0, 1.0); // 0 -> 1
            let scale = 0.35 + 0.65 * ramp * ramp;
            dt *= scale;
        }

        self.time_elapsed += dt;
        self.time_since_catch += dt;

        // Bot playtest harness tick: fire scripted events, check assertions, exit on completion.
        if self.bot.is_some() {
            self.bot_fire_events(ctx);

            // Seek-catch autopilot (see BotAction::SeekCatch): steering toward the nearest target is
            // handled in handle_player_movement; here we fire the tools. The whistle charms a
            // catchable crab out of its flee and yanks it into the player, and a stomp cracks any
            // shell we've walked up to so it becomes catchable — together they drive a real catch
            // through the actual game mechanics.
            if self.bot.as_ref().map_or(false, |b| b.seek_catch)
                && !self.show_instructions
                && !self.game_over
                && !self.show_world_map
            {
                let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
                // Whistle the nearest catchable crab once it's close enough that the ~1.4 s charm
                // window covers the final homing approach (player ~200 px/s), rather than burning the
                // cast at long range and letting the 4.5 s cooldown lapse before we can close. A cast
                // just outside the 220 px flee radius charms a wandering crab and reels it in before
                // it ever bolts — the whole difference between a reliable catch and a hopeless chase.
                if self.whistle_cooldown <= 0.0 {
                    if let Some(target) = self.nearest_catchable_crab_pos() {
                        if center.distance(target) < 260.0 {
                            controls::handle_key_down_event(self, ctx, Some(KeyCode::KeyE));
                        }
                    }
                }
                // Stomp anything within melee range: cracks a shelled crab we've homed onto (turning
                // an Armored/Hermit into a catchable target) so an all-shelled roll can't leave the
                // bot with nothing to catch.
                if self.stomp_cooldown <= 0.0 {
                    if let Some(target) = self.nearest_seek_target_pos() {
                        if center.distance(target) < STOMP_MAX_RADIUS {
                            controls::handle_key_down_event(self, ctx, Some(KeyCode::KeyR));
                        }
                    }
                }
            }

            self.bot_check_done();
        }

        // Weather + day/night ambience. Runs on REAL delta (not the slowmo-dilated dt) so the
        // world clock and weather evolve at a steady wall-clock pace regardless of bullet-time.
        self.update_weather(self.frame_dt(ctx));

        // Tutorial session bookkeeping: keep the sandbox stocked, detect the pass condition, and
        // run a short celebratory hold before handing control back to the title screen. Kept here
        // in the live path (not the paused menu gate) because a rhythm lesson needs the sim ticking.
        if self.tutorial.is_some() {
            // Real (undilated) time for the exit hold so the celebration is a fixed wall-clock
            // length regardless of any slow-mo the catch triggered.
            let real_dt = self.frame_dt(ctx);
            // If the learner clears the whole sandbox before passing, quietly restock so they can
            // keep practising instead of standing in an empty field. The "cleared" test differs by
            // scenario: BeatTiming crabs stay on the field once caught (nothing removes them), so
            // "no free crabs left to catch" means all-caught; ChainDeliver *removes* banked crabs at
            // the pen (retain(!caught) in try_deliver_train), so a fresh train to haul is needed
            // whenever the field is genuinely empty. Keying ChainDeliver off is_empty() is what
            // stops this branch from wiping a train the player is still hauling toward the pen.
            let tut_kind = self.tutorial.as_ref().unwrap().kind;
            let completed = self.tutorial.as_ref().unwrap().completed;
            let needs_restock = match tut_kind {
                TutorialKind::BeatTiming => self.crabs.iter().all(|c| c.caught),
                TutorialKind::ChainDeliver => self.crabs.is_empty(),
                // ShellCrack crabs aren't removed on a crack — their shell just drops to 0. Once
                // every crab has an open (or missing) shell there's nothing hard left to Stomp, so
                // drop in a fresh Armored ring to keep practising.
                TutorialKind::ShellCrack => self.crabs.iter().all(|c| c.boss_health <= 0.0),
                // LassoGrab crabs get roped into the train (marked caught) but aren't hauled to a
                // pen, so nothing removes them — same as BeatTiming, "all caught" means the wide
                // ring is cleared and it's time to fling out a fresh one to keep practising.
                TutorialKind::LassoGrab => self.crabs.iter().all(|c| c.caught),
            };
            if !completed && needs_restock {
                self.crabs =
                    spawn_tutorial_crabs(tut_kind, 6, (self.width, self.height), &mut crate::rng::rng());
            }
            let t = self.tutorial.as_mut().unwrap();
            if t.completed {
                t.pass_glow = (t.pass_glow + real_dt * 2.5).min(1.0);
                t.exit_timer = (t.exit_timer - real_dt).max(0.0);
                if t.exit_timer <= 0.0 {
                    // Opt-in exit: if we got here from a campaign world-map node, return to the
                    // map so the player can pick the next node. Otherwise go back to the title
                    // screen. Either way we never touch game_over, so the career is untouched.
                    self.tutorial = None;
                    if self.in_campaign {
                        // Reached only when the tutorial was PASSED (tutorials have no game-over),
                        // so this is a genuine win — complete the node and unlock the next.
                        self.return_to_world_map(true);
                    } else {
                        self.show_instructions = true;
                        self.show_how_to_play_text = false;
                    }
                }
            } else if t.passed() {
                // Latch the win exactly once: celebrate, then start the return countdown.
                t.completed = true;
                t.pass_glow = 0.0;
                t.exit_timer = 2.2;
                let center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
                self.floating_texts.spawn(
                    "TUTORIAL PASSED!".to_string(),
                    center - Vec2::new(90.0, 70.0),
                    44.0,
                    [0.4, 1.0, 0.5, 1.0],
                );
                self.on_beat_flash = self.on_beat_flash.max(0.85);
                self.screen_shake = self.screen_shake.max(8.0);
            }
        }

        // Staged difficulty ramp: as elapsed time crosses the next stage threshold, climb one
        // stage and make it a telegraphed event — a shout banner plus a musical punch — so the run
        // has a felt rising arc with earned standout moments, not a flat curve. Only ever climbs;
        // the density/duration scaling itself is read per-wave in start_current_pattern.
        self.stage_banner_timer = (self.stage_banner_timer - dt).max(0.0);
        if self.intensity_stage + 1 < INTENSITY_STAGES.len() {
            let (next_threshold, next_name, _, _) = INTENSITY_STAGES[self.intensity_stage + 1];
            if self.time_elapsed >= next_threshold {
                self.intensity_stage += 1;
                self.stage_banner_name = next_name;
                self.stage_banner_timer = 2.0;
                // The master clock applies this stage's tempo on the next bar downbeat, where every
                // looping source can restart on the same "1". Changing the grid here, mid-bar, made
                // the live kick immediately quicken while the melody stayed at its old tempo.
                // Musical punch so the escalation lands as a moment: brighten the beat, flash, a
                // short shake, and a rising-tension chime.
                self.beat_intensity = 2.0;
                self.on_beat_flash = self.on_beat_flash.max(0.6);
                self.screen_shake = self.screen_shake.max(8.0);
                let kick = crate::rng::rng().random_range(0.0_f32..std::f32::consts::TAU);
                self.screen_shake_vel = Vec2::new(kick.cos(), kick.sin()) * 8.0 * 60.0;
                // upgrade.ogg removed — tiresome and crackly; new sound TBD
            }
        }

        // Track player position history for conga chain
        self.position_history.push_front(self.player_pos);
        if self.position_history.len() > 2000 {
            self.position_history.pop_back();
        }

        self.beat_intensity = (self.beat_intensity - dt * 5.0).max(0.0);
        // Bar downbeat accent decays over roughly one beat, so its influence on the train's stomp
        // (and any accent-driven visuals) rides just past the "1" and fades before the next bar.
        self.bar_accent = (self.bar_accent - dt * 4.0).max(0.0);

        // Ease the zoom punch back out — snaps in instantly on catch, smooth spring-out.
        if self.zoom_punch > 0.0 {
            self.zoom_punch *= 0.86_f32.powf(dt * 60.0);
            if self.zoom_punch < 0.0008 {
                self.zoom_punch = 0.0;
            }
        }

        // Decay screen shake — spring back to zero
        if self.screen_shake > 0.0 {
            self.screen_shake_offset += self.screen_shake_vel * dt;
            // Spring: strong restoring force + damping
            self.screen_shake_vel += -self.screen_shake_offset * 800.0 * dt;
            self.screen_shake_vel *= 0.88_f32.powf(dt * 60.0);
            self.screen_shake = (self.screen_shake - dt * 18.0).max(0.0);
            if self.screen_shake < 0.05 {
                self.screen_shake = 0.0;
                self.screen_shake_offset = Vec2::ZERO;
                self.screen_shake_vel = Vec2::ZERO;
            }
        }

        // Combo window — reset streak if no catch for 1.8s
        if self.combo_timer > 0.0 {
            self.combo_timer -= dt;
            if self.combo_timer <= 0.0 {
                self.combo_count = 0;
            }
        }

        if self.on_beat_flash > 0.0 {
            self.on_beat_flash = (self.on_beat_flash - dt * 3.0).max(0.0);
        }
        if self.perfect_flash > 0.0 {
            self.perfect_flash = (self.perfect_flash - dt * 2.5).max(0.0);
        }
        if self.reef_hit_flash > 0.0 {
            self.reef_hit_flash = (self.reef_hit_flash - dt * 3.5).max(0.0);
        }
        // Groove Gamble feedback pulses decay each frame.
        if self.beat_gamble_flash > 0.0 {
            self.beat_gamble_flash = (self.beat_gamble_flash - dt * 3.5).max(0.0);
        }
        if self.rhythm_bonus_flash > 0.0 {
            self.rhythm_bonus_flash = (self.rhythm_bonus_flash - dt * 2.0).max(0.0);
        }
        if self.streak_lost_flash > 0.0 {
            self.streak_lost_flash = (self.streak_lost_flash - dt * 2.2).max(0.0);
        }
        if self.gamble_bank_flash > 0.0 {
            self.gamble_bank_flash = (self.gamble_bank_flash - dt * 2.5).max(0.0);
        }
        // "BANK NOW?" prompt breathes while there's an unbanked stack worth cashing out.
        let bankable = self.beat_gamble_mult > self.beat_gamble_locked + 0.5;
        if bankable {
            self.gamble_bank_pulse = (self.gamble_bank_pulse + dt * 4.0) % (std::f32::consts::TAU);
        } else {
            self.gamble_bank_pulse = 0.0;
        }

        // Frenzy banner fades out over its lifetime after a frenzy wave lands.
        if self.frenzy_banner_timer > 0.0 {
            self.frenzy_banner_timer = (self.frenzy_banner_timer - dt).max(0.0);
        }

        // Rising edge: the frame groove first tops out is the peak of rhythmic play, so announce it
        // loud and once. Fires a field-wide "POCKET LOCKED" celebration — a firework crown at the
        // player, a bloom flash, a beat kick, and a light zoom punch — reusing existing juice paths.
        // Reset when the meter drops out of full so it can re-fire on the next climb back up.
        let groove_full = self.groove >= 0.999;
        if groove_full && !self.groove_was_full {
            self.groove_full_flash = 1.0;
            self.on_beat_flash = self.on_beat_flash.max(0.7);
            self.beat_intensity = self.beat_intensity.max(1.6);
            self.zoom_punch = self.zoom_punch.max(0.06);
            let mut rng = crate::rng::rng();
            self.particle_system.spawn_milestone_fireworks(
                self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0),
                24,
                &mut rng,
            );
            // World-layer banner: anchor near the player so it reads on-screen under the camera.
            let banner_pos = self.player_pos + Vec2::new(-150.0, -220.0);
            self.floating_texts.spawn(
                "POCKET LOCKED".to_string(),
                banner_pos + Vec2::new(2.0, 2.0),
                38.0,
                [0.0, 0.0, 0.0, 0.8],
            );
            self.floating_texts.spawn(
                "POCKET LOCKED".to_string(),
                banner_pos,
                38.0,
                [1.0, 0.55, 0.95, 1.0],
            );
        }
        self.groove_was_full = groove_full;
        if self.groove_full_flash > 0.0 {
            self.groove_full_flash = (self.groove_full_flash - dt * 2.0).max(0.0);
        }

        // Groove meter decays over time; when it empties the on-beat streak lapses too.
        if self.groove > 0.0 {
            self.groove = (self.groove - dt * 0.18).max(0.0);
            if self.groove <= 0.0 {
                self.beat_streak = 0;
                self.perfect_streak = 0;
                // The Gamble heat fades with the groove — a quiet lapse, not a punished break, so
                // idling loses the unbanked climb gracefully. Whatever was cashed out with B stays.
                self.beat_gamble_mult = self.beat_gamble_locked;
            }
        }

        // Music intensity rises with chain length (not just score) and surges with groove.
        // Chain length directly reflects how well the player is doing right now, so it's a
        // more immediate and readable signal than accumulated score.
        let chain_intensity = match self.chain_count {
            0 => 0.0,
            1..=3 => 0.33,
            4..=8 => 0.67,
            _ => 1.0,
        };
        let groove_boost = if self.groove > 0.7 {
            (self.groove - 0.7) / 0.3 * 0.15
        } else {
            0.0
        };
        let target_intensity = (chain_intensity + groove_boost).min(1.0);
        self.music_intensity += (target_intensity - self.music_intensity) * dt * 0.3;

        if self.shake_timer > 0.0 {
            self.shake_timer -= dt;
            if self.shake_timer < 0.0 {
                self.shake_timer = 0.0;
            }
        }
        if self.boost_timer > 0.0 {
            self.boost_timer -= dt;
            if self.boost_timer < 0.0 {
                self.boost_timer = 0.0;
            }
        }
        if self.boost_cooldown > 0.0 {
            self.boost_cooldown -= dt;
            if self.boost_cooldown < 0.0 {
                self.boost_cooldown = 0.0;
            }
        }
        if self.whistle_cooldown > 0.0 {
            self.whistle_cooldown = (self.whistle_cooldown - dt).max(0.0);
        }
        if self.stomp_cooldown > 0.0 {
            self.stomp_cooldown = (self.stomp_cooldown - dt).max(0.0);
        }
        if self.cycle_cooldown > 0.0 {
            self.cycle_cooldown = (self.cycle_cooldown - dt).max(0.0);
        }
        if self.call_cooldown > 0.0 {
            self.call_cooldown = (self.call_cooldown - dt).max(0.0);
        }
        if self.call_pulse > 0.0 {
            self.call_pulse = (self.call_pulse - dt * 1.6).max(0.0);
        }
        // Groove Call: cooldown ticks down; the surge/pulse envelopes decay between beats (re-kicked
        // in the beat handler) so the field-wide lure pumps to the bar rather than pulling flatly.
        self.jam_timer = (self.jam_timer - dt).max(0.0);
        if self.groove_call_cooldown > 0.0 {
            self.groove_call_cooldown = (self.groove_call_cooldown - dt).max(0.0);
        }
        if self.groove_call_surge > 0.0 {
            self.groove_call_surge = (self.groove_call_surge - dt * 1.4).max(0.0);
        }
        if self.groove_call_pulse > 0.0 {
            self.groove_call_pulse = (self.groove_call_pulse - dt * 1.2).max(0.0);
        }
        if self.groove_call_echo_flash > 0.0 {
            self.groove_call_echo_flash = (self.groove_call_echo_flash - dt * 2.2).max(0.0);
        }
        // Downbeat Slam ring erupts outward, then fades. Purely visual — the catch already happened.
        if self.slam_active > 0.0 {
            self.slam_active = (self.slam_active - dt).max(0.0);
            self.slam_radius = (self.slam_radius + SLAM_RING_SPEED * dt).min(SLAM_RADIUS);
        }
        if self.slam_flash > 0.0 {
            self.slam_flash = (self.slam_flash - dt * 2.2).max(0.0);
        }
        if self.chain_snap_cooldown > 0.0 {
            self.chain_snap_cooldown = (self.chain_snap_cooldown - dt).max(0.0);
        }
        if self.king_splice_cooldown > 0.0 {
            self.king_splice_cooldown = (self.king_splice_cooldown - dt).max(0.0);
        }
        // Update stolen-crab magnetic pull: each stolen crab flies toward the nearest boss position,
        // advancing its timer. When the timer expires the crab is "absorbed" (just removed — the boss
        // train system comes later; for now the visual pull is enough).
        if !self.king_stolen_crabs.is_empty() {
            let boss_pos: Option<Vec2> = self.crabs.iter().find_map(|c| {
                if c.is_king_crab() && !c.caught {
                    Some(c.pos)
                } else {
                    None
                }
            });
            if let Some(bpos) = boss_pos {
                for (pos, timer, _color) in &mut self.king_stolen_crabs {
                    *timer -= dt;
                    // Lerp toward boss — starts slow (magnetic pull builds), accelerates as timer drops.
                    let t = (*timer / 0.9_f32).clamp(0.0, 1.0);
                    let speed = (1.0 - t * t) * dt * 6.0; // quadratic acceleration toward boss
                    let dir = (bpos - *pos).normalize_or_zero();
                    *pos += dir * (bpos - *pos).length() * speed;
                }
                self.king_stolen_crabs.retain(|(_, timer, _)| *timer > 0.0);
            } else {
                // Boss is gone (caught), free the stolen crabs instead of holding them.
                self.king_stolen_crabs.clear();
            }
        }
        if self.boss_hit_iframes > 0.0 {
            self.boss_hit_iframes = (self.boss_hit_iframes - dt).max(0.0);
        }
        if self.dash_flash > 0.0 {
            self.dash_flash = (self.dash_flash - dt * 7.0).max(0.0);
        }

        if self.level_title_timer > 0.0 {
            self.level_title_timer -= dt;
            if self.level_title_timer < 0.0 {
                self.level_title_timer = 0.0;
            }
        }

        // The playfield (world) is larger than the viewport; movement, spawning and clamping all
        // happen in world space. The camera (computed below and in draw) maps it back to the screen.
        let area = (self.world_width, self.world_height);
        handle_player_movement(self, ctx, dt, SPEED, area);

        // Pirate treasure is a rare detour: it appears far enough away to route toward, then grades
        // the pickup with the same tight window as catches. A late grab still protects half a meter,
        // while landing on the beat locks the whole groove in.
        if let Some(pos) = self.treasure_chest {
            let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            if player_center.distance_squared(pos)
                <= TREASURE_CHEST_PICKUP_RADIUS * TREASURE_CHEST_PICKUP_RADIUS
            {
                let on_beat = self.on_beat_now();
                self.groove = treasure_groove_level(self.groove, on_beat);
                self.treasure_chest = None;
                self.treasure_chest_timer = TREASURE_CHEST_ROLL_INTERVAL;
                self.particle_system
                    .spawn_milestone_fireworks(
                        pos,
                        if on_beat {
                            TREASURE_CHEST_ON_BEAT_PARTICLES
                        } else {
                            TREASURE_CHEST_OFF_BEAT_PARTICLES
                        },
                        &mut crate::rng::rng(),
                    );
                self.spawn_catch_shockwave(
                    pos,
                    if on_beat {
                        [1.0, 0.82, 0.2]
                    } else {
                        [0.85, 0.55, 0.2]
                    },
                );
                self.floating_texts.spawn(
                    if on_beat {
                        "TREASURE! GROOVE FULL".to_string()
                    } else {
                        "TREASURE! GROOVE HALF".to_string()
                    },
                    pos - Vec2::new(116.0, 48.0),
                    34.0,
                    if on_beat {
                        [1.0, 0.88, 0.25, 1.0]
                    } else {
                        [1.0, 0.62, 0.25, 1.0]
                    },
                );
            }
        } else {
            self.treasure_chest_timer -= dt;
            if self.treasure_chest_timer <= 0.0 {
                self.treasure_chest_timer = TREASURE_CHEST_ROLL_INTERVAL;
                let mut rng = crate::rng::rng();
                if rng.random_bool(TREASURE_CHEST_SPAWN_CHANCE) {
                    let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
                    let mut pos = player_center;
                    for _ in 0..TREASURE_CHEST_SPAWN_ATTEMPTS {
                        let candidate = Vec2::new(
                            rng.random_range(
                                TREASURE_CHEST_SPAWN_MARGIN
                                    ..self.world_width - TREASURE_CHEST_SPAWN_MARGIN,
                            ),
                            rng.random_range(
                                TREASURE_CHEST_SPAWN_MARGIN
                                    ..self.world_height - TREASURE_CHEST_SPAWN_MARGIN,
                            ),
                        );
                        if candidate.distance(player_center) >= TREASURE_CHEST_MIN_SPAWN_DISTANCE {
                            pos = candidate;
                            break;
                        }
                    }
                    self.treasure_chest = Some(pos);
                }
            }
        }

        // Drum Roll (hold T): poll the held key here rather than off the key-down event, since the
        // event fires unreliably on key-repeat and we need a clean "held across beats" charge. The
        // per-beat hit counting lives in the beat handler; here we only edge-detect press/release
        // and drive the timers. Releasing after landing at least one on-beat roll hit FIRES a
        // focused beam blast; releasing with nothing charged just cancels quietly.
        let t_held = !self.show_instructions
            && !self.game_over
            && ctx
                .keyboard
                .is_physical_key_pressed(&PhysicalKey::Code(ggez::input::keyboard::KeyCode::KeyT));
        if !t_held && self.drum_roll_held {
            // Release edge: fire if we banked any roll hits, otherwise drop the (empty) charge.
            if self.drum_roll_hits > 0 {
                self.fire_drum_roll();
            }

            self.drum_roll_hits = 0;
        }
        self.drum_roll_held = t_held;
        // Ease the visual charge toward the banked hit count (capped for the telegraph), and decay
        // the fired-blast window. drum_roll_fire gates the widened beam in update_crabs + the glow.
        let charge_target = if t_held {
            (self.drum_roll_hits as f32 / DRUM_ROLL_MAX as f32).min(1.0)
        } else {
            0.0
        };
        self.drum_roll_charge += (charge_target - self.drum_roll_charge) * (dt * 12.0).min(1.0);
        if self.drum_roll_fire > 0.0 {
            // ~0.5s window so the widened, yanking beam has time to actually reel the arc in.
            self.drum_roll_fire = (self.drum_roll_fire - dt * 2.0).max(0.0);
        }

        // Dash particle burst — fires only in the first frame (threshold near 1.0)
        if self.dash_flash > 0.95 {
            let center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            self.particle_system
                .spawn_dash_burst(center, self.last_dir, &mut crate::rng::rng());
            // A GROOVE DASH (on-beat, gather-wake armed this same frame) throws an extra, brighter
            // burst so a watcher can instantly tell the timed dash apart from the plain escape dash.
            if self.groove_dash_timer > 0.0 {
                let rng = &mut crate::rng::rng();
                self.particle_system
                    .spawn_dash_burst(center, self.groove_dash_dir, rng);
                self.particle_system
                    .spawn_beat_pulse(&[center], 2.0, self.chain_count, rng);
            }
        }

        // Flashlight auto-targeting: aim at the nearest King Crab — NPC train leaders first,
        // then any uncaught boss crab in self.crabs. NPC trains are the primary targets since
        // boss fight crabs only exist during boss encounters.
        {
            let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);

            // Collect candidate positions: NPC train leaders + uncaught boss crabs.
            let npc_target = self.npc_trains.iter()
                .map(|t| t.leader_pos)
                .min_by_key(|p| (p.distance(player_center) * 100.0) as i32);
            let boss_target = self.crabs.iter()
                .filter(|c| !c.caught && c.is_boss())
                .min_by_key(|c| (c.pos.distance(player_center) * 100.0) as i32)
                .map(|c| c.pos);

            // Pick whichever is closer.
            let target = match (npc_target, boss_target) {
                (Some(n), Some(b)) => Some(if n.distance(player_center) < b.distance(player_center) { n } else { b }),
                (Some(n), None) => Some(n),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            };

            if let Some(t) = target {
                let desired = (t - player_center).normalize_or_zero();
                if desired.length() > 0.1 {
                    let speed = 6.0 * dt;
                    self.flashlight.aim_dir = (self.flashlight.aim_dir + (desired - self.flashlight.aim_dir) * speed).normalize_or_zero();
                }
            }

            // Charge drain while on, passive regen while off.
            const DRAIN_PER_SEC: f32 = 0.18;   // ~5.5s full charge
            const REGEN_PER_SEC: f32 = 0.055;  // ~18s passive regen (on-beat adds on top)
            if self.flashlight.on {
                self.flashlight.charge = (self.flashlight.charge - DRAIN_PER_SEC * dt).max(0.0);
                if self.flashlight.charge <= 0.0 {
                    self.flashlight.on = false; // auto-off when drained
                }
            } else {
                self.flashlight.charge = (self.flashlight.charge + REGEN_PER_SEC * dt).min(1.0);
            }
        }

        self.handle_crab_catching(ctx);
        self.update_crabs(dt, area);

        // Emergent herding: the conga body walls off panicking crabs, bouncing them back toward
        // the beam. Runs before the snap check so a crab deflected by the body never reaches the
        // tail, while one aimed straight at the soft tail still slips past to snap it.
        self.deflect_fleeing_off_chain();

        // Chain-as-risk: a spooked wild crab barreling into the exposed tail can snap links loose.
        self.snap_chain_on_panic();

        // King Crab splice: a charging boss that crosses ANY chain segment steals the back section,
        // pulling it magnetically toward itself (reverse-Snake mechanic).
        self.check_king_crab_splice();

        // Biome wrinkle (Neon Kelp Forest): clinging fronds can snag and strip the tail if you
        // route a long train through the weeds instead of around them.
        self.snag_chain_on_kelp(dt);

        // Biome wrinkle (Rocky Shore): the tide rises and falls on the bar cycle, submerging the
        // low rocks into passable shortcuts on the beat and draining them back to solid walls.
        self.update_rock_tide(dt);

        // Thief archetype: a parasite crab clamped onto the tail steadily peels links loose on a
        // timer until you catch or dislodge it — pressure on the train you've already built.
        self.steal_chain_thief(dt);
        // A whistle or a nearby stomp shakes a latched Thief off the tail (both raise/consume
        // charm below); handled inside update_crabs' charm application for the whistle, and the
        // stomp clears it via its blast radius. The latch state is otherwise self-limiting.

        // Boss enrage set-piece (King Crab): the cracked-floor fissures bite the tail if you drag it
        // through one, so the arena reshape has real teeth. Fissures also finish opening here.
        for (_, _, age) in self.boss_fissures.iter_mut() {
            *age = (*age + dt * 2.5).min(1.0);
        }
        // The beat-synced geyser pulse fades between beats (kicked back to ~1 in the beat-fire
        // block above). Fast decay so the eruption is a sharp on-beat spike, not a lingering glow.
        if self.boss_fissure_erupt > 0.0 {
            self.boss_fissure_erupt = (self.boss_fissure_erupt - dt * 3.2).max(0.0);
        }
        self.damage_tail_in_fissures(dt);

        // Cash in the train: drive the conga head into the delivery pen to bank it for score.
        self.try_deliver_train(ctx);
        if self.deliver_flash > 0.0 {
            self.deliver_flash = (self.deliver_flash - dt * 1.6).max(0.0);
        }
        // Advance the pen parade: each marcher that reaches the pen this frame pops a small
        // sparkle burst in its own color, so the train files in one crab at a time.
        // Reuse the persistent arrivals buffer to avoid a Vec allocation every frame while a
        // parade is active (up to ~2s after each bank, capped at 40 marchers).
        let mut arrivals = std::mem::take(&mut self.marcher_arrivals_buf);
        self.penned_marchers.update(dt, &mut arrivals);
        for &(pos, color) in arrivals.iter() {
            self.particle_system
                .spawn_catch_effect(pos, color, CrabType::Normal, &mut crate::rng::rng());
        }
        self.marcher_arrivals_buf = arrivals;
        // Idle-decay the delivery streak: if too long passes between banks, drop a notch so the
        // multiplier tracks recent cashing tempo. Each notch grants a fresh grace window.
        if self.deliver_streak > 0 {
            self.deliver_streak_timer = (self.deliver_streak_timer - dt).max(0.0);
            if self.deliver_streak_timer <= 0.0 {
                self.deliver_streak -= 1;
                // Losing a streak notch is a real (if gentle) setback — give it the SNAP-style loss
                // feedback so heat draining away reads on screen, not just silently in the pen badge.
                // Fires per notch (the decay is gradual, not a cliff), and only while a multiplier is
                // still at stake (>= 1 remaining bank = >= 1.25x), so a fizzle from streak 1 stays quiet.
                if self.deliver_streak >= 1 {
                    let lost_mult = 1.0 + self.deliver_streak as f32 * 0.25;
                    self.floating_texts.spawn(
                        format!("STREAK -1  ({:.2}x)", lost_mult),
                        self.pen_pos - Vec2::new(70.0, PEN_RADIUS + 8.0),
                        24.0,
                        [1.0, 0.45, 0.55, 1.0],
                    );
                }
                if self.deliver_streak > 0 {
                    self.deliver_streak_timer = DELIVER_STREAK_GRACE;
                }
            }
        }

        // Decay join_pulse ripple timers
        for crab in &mut self.crabs {
            if crab.join_pulse > 0.0 {
                crab.join_pulse = (crab.join_pulse - dt * 3.5).max(0.0);
            }
        }

        // Rainbow trail behind player when moving
        if self.player_vel.length() > 15.0 {
            let center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            self.particle_system.spawn_movement_trail(
                center,
                self.player_vel,
                self.time_elapsed,
                &mut crate::rng::rng(),
            );
        }

        // Advance ghost ring ages; remove fully faded rings
        let ring_speed = 1.4; // age 0..1 in ~0.71 seconds (fast enough to clear before next beat)
        self.chain_rings.retain_mut(|(_, age, _)| {
            *age += dt * ring_speed;
            *age < 1.0
        });

        // Beat-hit punch events are single-frame instantaneous flashes — clear at the start of
        // each tick so stale punches from last frame never leak into the draw call.
        self.beat_punch_events.clear();

        // Bond-forming flash arcs: age them out over 0.35 seconds then remove.
        self.bond_flash_events.retain_mut(|(_, _, _, age)| {
            *age -= dt * 2.86; // 0.35s lifetime
            *age > 0.0
        });

        // Advance catch impact shockwaves; a bit faster than ghost rings so they read as a snap
        let shock_speed = 2.6; // age 0..1 in ~0.38 seconds
        self.catch_shockwaves.retain_mut(|(_, age, _)| {
            *age += dt * shock_speed;
            *age < 1.0
        });

        // Advance catch whip-trails — a fast fade so they read as a snap, not a lingering line.
        let trail_speed = 3.4; // age 0..1 in ~0.29 seconds
        self.catch_trails.retain_mut(|(_, _, age, _)| {
            *age += dt * trail_speed;
            *age < 1.0
        });

        // Groove-Call answer streaks fade a touch slower than a catch snap so the whole herd's
        // on-beat lunge lingers long enough to read across a big field, but still clears before the
        // next beat throws a fresh set.
        let call_streak_speed = 2.2; // age 0..1 in ~0.45s
        self.call_streaks.retain_mut(|(_, _, age, _)| {
            *age += dt * call_streak_speed;
            *age < 1.0
        });

        // Advance stampede fear rings — a touch slower/wider than the catch pop so the scatter reads.
        let fear_speed = 2.0; // age 0..1 in ~0.5 seconds
        self.fear_rings.retain_mut(|(_, age)| {
            *age += dt * fear_speed;
            *age < 1.0
        });

        // Advance Tide Boss shockwave rings — expand outward, drop once past their reach.
        self.tide_pulses.retain_mut(|(_, radius)| {
            *radius += TIDE_PULSE_EXPAND_SPEED * dt;
            *radius < TIDE_PULSE_RADIUS * 1.25
        });

        // Update particle system
        self.particle_system.update(dt);
        self.floating_texts.update(dt);

        // Beat Wave: expand outward, attract crabs toward player
        if self.beat_wave_active {
            self.beat_wave_radius += 600.0 * dt;
            if self.beat_wave_radius > 300.0 {
                self.beat_wave_active = false;
                self.beat_wave_radius = 0.0;
            } else {
                let player_center =
                    self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
                for crab in &mut self.crabs {
                    if !crab.caught {
                        let dist = player_center.distance(crab.pos);
                        if dist < self.beat_wave_radius {
                            crab.spooked_timer = 1.0;
                            let toward = (player_center - crab.pos).normalize_or_zero();
                            let speed = crab.speed.max(60.0);
                            crab.vel = toward * speed;
                        }
                    }
                }
            }
        }

        // On-beat catch bloom settles back toward zero between beats: it's punched wide on each beat
        // (widest on the downbeat) and eases off before the next hit, so the widened scoop is a
        // rhythmic pulse tied to the bar rather than a permanent radius buff. Tuned to fade over most
        // of a beat at typical tempo so there's a clear on-beat/off-beat difference.
        self.beat_catch_bloom = (self.beat_catch_bloom - 90.0 * dt).max(0.0);

        // Cleave slash fades fast — it's a single stroke, not a lingering aura. ~0.35s life.
        self.cleave_flash = (self.cleave_flash - 2.9 * dt).max(0.0);

        // Groove Dash gather-wake: a dash fired ON the beat drags free crabs into your slipstream as
        // you punch through, so timing your movement to the beat becomes a live routing tool between
        // climaxes (not just a juicier escape). Only crabs in front of the dash heading get swept —
        // it's a directional wake, not the radial whistle — so a groove-savvy player learns to line
        // up a clump and dash *through* it to hoover it into the train's path. Off-beat dashes never
        // arm this (see controls.rs), so the plain escape dash is untouched.
        if self.groove_dash_timer > 0.0 {
            self.groove_dash_timer = (self.groove_dash_timer - dt).max(0.0);
            let heading = self.groove_dash_dir;
            let reach = 170.0;
            let pull = 340.0;
            // Follow the LIVE player position, not the captured fire point: the boost punches at
            // ~30x speed, so the player blows well past any fixed target within a frame or two.
            // Pulling toward where the player actually is each frame keeps the herd funnelling into
            // your slipstream instead of toward a spot you've already left. The forward-cone gate
            // still uses the captured heading so the wake reads as "the crabs I dashed into".
            let player_center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            if heading.length() > 0.01 {
                for crab in &mut self.crabs {
                    if crab.caught {
                        continue;
                    }
                    let to_crab = crab.pos - player_center;
                    let dist = to_crab.length();
                    if dist < 1.0 || dist > reach {
                        continue;
                    }
                    // Forward cone: only sweep crabs roughly ahead of the dash (dot > ~0.2), so the
                    // wake reads as "the herd I dashed into" rather than an omnidirectional yank.
                    let forward = to_crab.normalize_or_zero().dot(heading);
                    if forward < 0.2 {
                        continue;
                    }
                    let toward = (player_center - crab.pos).normalize_or_zero();
                    let proximity = 1.0 - (dist / reach).clamp(0.0, 1.0);
                    crab.vel = toward * (pull * (0.5 + proximity * 0.5));
                    crab.spooked_timer = crab.spooked_timer.max(0.5);
                    // Soothe any panic the sweep catches, like the whistle does — a gather, not a scare.
                    crab.fleeing = false;
                    crab.startle_timer = 0.0;
                }
            }
        }

        // Whistle: an expanding sonic pulse from the player that yanks free crabs inward. The pull
        // strength is per-archetype (CrabType::whistle_pull) so it's the go-to tool for skittish
        // Sneaky crabs but only nudges the heavy Big ones — a soft counter, never a hard requirement.
        if self.whistle_active > 0.0 {
            // Whistle-lane-scaled reach + pull, read once so the &mut self.crabs loop can use them.
            let whistle_max_r = self.whistle_max_radius() * self.whistle_beat_bonus;
            let whistle_pull = self.whistle_pull_speed() * self.whistle_beat_bonus;
            // The beat_bonus is only >1.0 when this cast landed on the beat (see reward_on_beat_tool),
            // so it doubles as our "was this an on-beat cast?" flag for the rhythm-native Thief shake.
            let on_beat_cast = self.whistle_beat_bonus > 1.0;
            self.whistle_active = (self.whistle_active - dt).max(0.0);
            self.whistle_radius =
                (self.whistle_radius + WHISTLE_RING_SPEED * dt).min(whistle_max_r);
            // Where the ring's leading edge sat last frame — a crab in the thin band between this and
            // whistle_radius was just swept by the front, so the shell-deflect ping fires once (crisp,
            // not a per-frame smear) as the pulse passes it. Zero-width once the ring clamps to max.
            let whistle_ring_prev = (self.whistle_radius - WHISTLE_RING_SPEED * dt).max(0.0);
            let center = self.whistle_center;
            // The whistle doubles as crowd control: sweeping it over a panicking herd soothes the
            // fear. Charm lasts a beat or two (longer as the whistle lane is ranked up) and blocks
            // both fresh flee and the beat-startle contagion, so it genuinely quells a stampede.
            let charm_dur = 1.4 + 0.5 * self.whistle_rank as f32;
            let mut soothed = std::mem::take(&mut self.whistle_soothed_buf);
            soothed.clear();
            // On-beat casts that rip a latched Thief clean off get to CATCH it as a bonus — collected
            // here (index + pos) and processed after the &mut self.crabs loop, like `soothed`/`cracked`.
            // Reused scratch buffer (almost always empty) instead of a fresh Vec::new() every frame
            // the whistle is active.
            let mut thief_snatched = std::mem::take(&mut self.whistle_thief_snatch_buf);
            thief_snatched.clear();
            for (i, crab) in self.crabs.iter_mut().enumerate() {
                if crab.caught {
                    continue;
                }
                let pull = crab.crab_type.whistle_pull();
                if pull <= 0.0 {
                    continue; // boss shrugs it off entirely
                }
                let dist = center.distance(crab.pos);
                // Only crabs the sweeping front has already passed get grabbed this frame.
                if dist < self.whistle_radius {
                    let toward = (center - crab.pos).normalize_or_zero();
                    // Stronger yank the closer the crab is, scaled by its archetype's susceptibility.
                    let proximity = 1.0 - (dist / whistle_max_r).clamp(0.0, 1.0);
                    let speed = whistle_pull * pull * (0.5 + proximity * 0.5);
                    crab.vel = toward * speed;
                    crab.speed = 1.0; // vel encodes full speed; keep multiplier neutral (matches flee convention)
                    // Golden crab being reeled in by whistle — its highest-pull matchup, show it.
                    if crab.is_golden() && self.whistle_golden_hits_buf.len() < 12 {
                        self.whistle_golden_hits_buf.push(crab.pos);
                    }
                    // Dancer pulled by whistle — rhythm tool meets rhythm crab, show the harmony.
                    if crab.is_dancer() && self.whistle_dancer_hits_buf.len() < 10 {
                        self.whistle_dancer_hits_buf.push(crab.pos);
                    }
                    // Sneaky flushed out and reeled in — the whistle's FLAGSHIP match (folds hardest
                    // of all but the Golden, whistle_pull 1.5). This was the one whistle strong-match
                    // still missing a tell; show it, and flag on-beat casts so the burst flares
                    // brighter on the beat ("gather skittish crabs on the beat" reads as a drum hit).
                    if crab.is_sneaky() && self.whistle_sneaky_hits_buf.len() < 12 {
                        self.whistle_sneaky_hits_buf.push((crab.pos, on_beat_cast));
                    }
                    // WRONG-TOOL tell: the sonic pulse pings off a still-shelled crab (Armored /
                    // shelled Hermit) instead of charming it — pull is only a token 0.3 ("barely
                    // nudges it", enemies.rs). Mirror of the lasso/shell deflect: teaches "the shell
                    // shrugs the whistle — crack it first (Stomp), then herd it." Fired once from the
                    // ring's leading edge so it reads as a crisp shell-ping, not a lingering glow.
                    if crab.boss_health > 0.0
                        && (crab.is_armored() || crab.is_shelled_hermit())
                        && dist >= whistle_ring_prev
                        && self.whistle_shell_deflect_hits_buf.len() < 12
                    {
                        self.whistle_shell_deflect_hits_buf.push(crab.pos);
                    }
                    // Count as attracted so the flee/wobble logic doesn't fight the pull next frame.
                    crab.spooked_timer = crab.spooked_timer.max(0.6);
                    // Note the crabs we actually talked down out of a panic so the "soothed" note
                    // only pops where it reads (not on already-calm crabs the pulse merely gathers).
                    if crab.fleeing || crab.startle_timer > 0.0 {
                        soothed.push(crab.pos);
                    }
                    crab.fleeing = false;
                    crab.startle_timer = 0.0;
                    crab.charm_timer = crab.charm_timer.max(charm_dur);
                    // Rhythm-native Thief counterplay: shaking off a latched Thief now *plays* like
                    // the rest of the game rather than being a flat toggle.
                    //   - ON BEAT: the whistle rips it clean off AND flings it into the train as a
                    //     bonus catch — the peak payoff for timing the counter.
                    //   - OFF BEAT: it only loosens the grip — the latch timer is pushed back so you
                    //     buy a beat, but the Thief stays on your tail and will bite again.
                    if crab.is_latched() {
                        // Strong-match tell (whistle_pull 1.3, "yanks it off your tail nicely"): the
                        // one whistle strong-match without a dedicated burst, and — off the beat — the
                        // only Thief counterplay that was visually silent (the on-beat rip already pops
                        // "THIEF NABBED!"). Show a severed-tether burst on EVERY flick at a latched
                        // Thief so the grip breaking reads either way, bright on-beat vs dim off-beat.
                        if self.whistle_thief_hits_buf.len() < 12 {
                            self.whistle_thief_hits_buf.push((crab.pos, on_beat_cast));
                        }
                        if on_beat_cast {
                            crab.latch_timer = 0.0;
                            thief_snatched.push((i, crab.pos));
                        } else {
                            // Loosen: delay the next peel without removing the threat.
                            crab.latch_timer = crab.latch_timer.max(0.75);
                        }
                    }
                }
            }
            // On-beat whistle catches its shaken Thieves: enlist each into the train and pay a bonus.
            for (i, pos) in thief_snatched.drain(..) {
                self.snatch_thief_on_beat(i, pos);
            }
            self.whistle_thief_snatch_buf = thief_snatched; // hand the buffer back for reuse next frame
            // Warm puffs rising off the crabs the pulse just calmed — the visual counterpart to
            // the cold "!" alarm rings the panic contagion throws.
            if !soothed.is_empty() {
                let mut rng = crate::rng::rng();
                for &pos in soothed.iter().take(8) {
                    self.particle_system.spawn_soothe_puff(pos, &mut rng);
                }
            }
            self.whistle_soothed_buf = soothed; // hand the buffer back for reuse next frame
        }

        // Groove Call: a FIELD-WIDE, beat-pumping herd lure. While a call is live (bars remaining),
        // every free crab across the WHOLE arena drifts toward the player — no radius gate, unlike the
        // whistle — with the pull surging on the beat and easing between (groove_call_surge, kicked in
        // the beat handler). This is the watchable payoff: the entire herd visibly streams in, lunging
        // together on each downbeat, so the beat itself becomes an arena-wide routing tool. A clean
        // on-beat call (groove_call_strength 1.0, 2 bars) pulls the herd hard and long; an off-beat one
        // (0.4, 1 bar) barely leans them in. Cheap: one extra pass over the crabs only while active.
        if self.groove_call_bars > 0.0 {
            let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            // Base drift speed, scaled by call quality and the on-beat surge. Between beats the surge
            // decays toward ~0 so the herd coasts; on the beat it snaps back to full for the lunge.
            // Tuned against WHISTLE_PULL_SPEED (240) and the ~1280×960 view: at ~150 a crab covers a
            // few hundred units across the 2-bar (~4s) window, so even far-side crabs visibly stream
            // most of the way in — genuinely field-wide — while staying a gentle current, not the
            // whistle's hard instant yank, which is what keeps this a distinct verb.
            let base = 150.0 * self.groove_call_strength;
            let surge = 0.35 + 0.65 * self.groove_call_surge; // never fully stops, but pumps on-beat
            for crab in self.crabs.iter_mut() {
                if crab.caught {
                    continue;
                }
                // Bosses shrug it off entirely, matching the whistle's carve-out — a rhythm lure can't
                // drag a lumbering boss around. Latched Thieves and answering Dancers keep their own
                // scripted motion so the call layers over ordinary crabs without fighting other verbs.
                if crab.is_boss()
                    || crab.crab_type.whistle_pull() <= 0.0
                    || crab.is_latched()
                    || crab.answering_call > 0.0
                {
                    continue;
                }
                let toward = (center - crab.pos).normalize_or_zero();
                // Per-archetype susceptibility reuses whistle_pull so the call reads consistently with
                // the whistle (skittish crabs answer eagerly, heavy ones lean in only a little).
                let pull = crab.crab_type.whistle_pull();
                let speed = base * surge * pull;
                // Blend toward the call heading rather than overwriting velocity outright, so the herd
                // streams as a smooth current instead of teleporting — the legible "answering" flow.
                crab.vel = crab.vel.lerp(toward * speed, 0.12);
                // Hold their nerve so the flee/wobble logic doesn't fight the lure the same frame.
                crab.spooked_timer = crab.spooked_timer.max(0.5);
                crab.fleeing = false;
            }
        }

        // Stomp: a close-range ground-pound shockwave. It CRACKS Armored crab shells instantly (its
        // dedicated counter — the beam is the slow universal fallback) and gives any free crab the
        // front passes a light inward shove. Its short reach makes it a melee tool, not a ranged
        // gather like the whistle/lasso, so choosing the right verb per herd is a real decision.
        if self.stomp_active > 0.0 {
            // Stomp-lane-scaled reach, read once so the &mut self.crabs loop can use it.
            let stomp_max_r = self.stomp_max_radius() * self.stomp_beat_bonus;
            // beat_bonus >1.0 only on an on-beat cast — same on-beat flag the whistle uses.
            let on_beat_cast = self.stomp_beat_bonus > 1.0;
            self.stomp_active = (self.stomp_active - dt).max(0.0);
            self.stomp_radius = (self.stomp_radius + STOMP_RING_SPEED * dt).min(stomp_max_r);
            let center = self.stomp_center;
            let mut cracked = std::mem::take(&mut self.stomp_cracked_buf);
            cracked.clear();
            let mut hermit_popped = std::mem::take(&mut self.hermit_popped_buf);
            hermit_popped.clear();
            // Reused scratch buffer (almost always empty) instead of a fresh Vec::new() every
            // frame the stomp is active — same pattern as the whistle loop above.
            let mut thief_snatched = std::mem::take(&mut self.stomp_thief_snatch_buf);
            thief_snatched.clear();
            // Hermit King crack/deflect events this frame (rare — at most one King on the field).
            let mut king_cracks: Vec<(Vec2, f32)> = Vec::new();
            let mut king_deflects: Vec<Vec2> = Vec::new();
            for (i, crab) in self.crabs.iter_mut().enumerate() {
                // The Hermit King is the one boss the Stomp DOES touch — it's the whole fight.
                // One shell layer per pound, gated by phase: Sturdy takes any Stomp, Rattled and
                // Panicked only crack to an ON-BEAT Stomp (the same beat window every tool uses).
                // stun_timer doubles as a per-pound i-frame so one expanding ring can't peel the
                // whole stack in a few frames (it ticks down in the boss branch of update_crabs).
                if crab.is_hermit_king() && !crab.caught {
                    let dist = center.distance(crab.pos);
                    if dist < self.stomp_radius && crab.boss_health > 0.0 && crab.stun_timer <= 0.0
                    {
                        let lands = matches!(
                            hermit_king_phase(crab.boss_health),
                            HermitKingPhase::Sturdy
                        ) || on_beat_cast;
                        if lands {
                            crab.boss_health = (crab.boss_health - 1.0).max(0.0);
                            crab.stun_timer = HERMIT_KING_CRACK_IFRAME;
                            crab.join_pulse = 1.2; // reel from the pound
                            king_cracks.push((crab.pos, crab.boss_health));
                        } else {
                            crab.stun_timer = 0.5; // brief lull so the deflect cue fires once, not every frame
                            king_deflects.push(crab.pos);
                        }
                    }
                    continue;
                }
                if crab.caught || crab.is_boss() {
                    continue; // the King Crab shrugs off a Stomp — it needs the beam
                }
                let dist = center.distance(crab.pos);
                if dist >= self.stomp_radius {
                    continue; // only crabs the front has already swept past are hit this frame
                }
                // Crack a hard shell wide open the instant the shockwave reaches it — an Armored
                // crab, or a shelled Hermit (whose shell the beam can't touch, so the Stomp is one of
                // its three intended cracks). A cracked Hermit pops out defenceless and bolts.
                if (crab.is_armored() || crab.is_shelled_hermit()) && crab.boss_health > 0.0 {
                    let was_hermit = crab.is_hermit();
                    crab.boss_health = 0.0;
                    if was_hermit {
                        hermit_popped.push(crab.pos);
                    } else {
                        cracked.push(crab.pos);
                        self.stomp_armored_hits_buf.push(crab.pos);
                    }
                }
                // Strong-match: stomp cracking a Dancer's shell (Dancer is a rhythm-native target
                // for Stomp, so this hit is the archetype-tool pairing working as designed).
                if crab.is_dancer() && !crab.caught {
                    self.stomp_dancer_hits_buf.push(crab.pos);
                }
                // A Stomp near the tail is the second, close-range Thief counter — and it plays the
                // same rhythm-native way the whistle does: on-beat rips a latched Thief clean off and
                // banks it as a bonus catch; off-beat only loosens its grip so it bites again.
                if crab.is_latched() {
                    if on_beat_cast {
                        crab.latch_timer = 0.0;
                        thief_snatched.push((i, crab.pos));
                    } else {
                        crab.latch_timer = crab.latch_timer.max(0.75);
                    }
                }
                // Light inward shove + brief calm so the shaken crab doesn't immediately bolt.
                let toward = (center - crab.pos).normalize_or_zero();
                crab.vel = toward * (WHISTLE_PULL_SPEED * 0.6);
                crab.spooked_timer = crab.spooked_timer.max(0.4);
                crab.fleeing = false;
            }
            for (i, pos) in thief_snatched.drain(..) {
                self.snatch_thief_on_beat(i, pos);
            }
            self.stomp_thief_snatch_buf = thief_snatched; // hand the buffer back for reuse next frame
            // Tutorial pass tracking: count real Stomp shell-cracks for the shell-cracking learn-
            // session. Bumped only here (the crack event), guarded by the tutorial being active and
            // its kind, so a headless run of the same scenario reaches the same `passed()` predicate
            // — and it can't be satisfied by beam wear-down, since that never enters this Stomp loop.
            if let Some(t) = self.tutorial.as_mut() {
                if t.kind == TutorialKind::ShellCrack {
                    t.shells_cracked = t
                        .shells_cracked
                        .saturating_add((cracked.len() + hermit_popped.len()) as u32);
                }
            }
            // Campaign win tracking: every full shell crack (Armored or Hermit) counts toward a
            // CrackAndHold goal, whatever verb did the cracking — this is the Stomp site.
            self.shells_cracked_run += cracked.len() + hermit_popped.len();
            for &pos in cracked.iter() {
                self.floating_texts.spawn(
                    "SHELL CRACKED!".to_string(),
                    pos - Vec2::new(70.0, 40.0),
                    26.0,
                    [0.7, 0.85, 1.0, 1.0],
                );
                self.spawn_catch_shockwave(pos, [0.7, 0.8, 0.95]);
            }
            for pos in hermit_popped.drain(..) {
                self.spawn_hermit_pop(pos);
            }
            for &(pos, shells_left) in king_cracks.iter() {
                if shells_left <= 0.0 {
                    self.floating_texts.spawn(
                        "THE KING IS EXPOSED — CATCH IT!".to_string(),
                        pos - Vec2::new(150.0, 70.0),
                        34.0,
                        [0.4, 1.0, 0.5, 1.0],
                    );
                    self.spawn_catch_shockwave(pos, [1.0, 0.95, 0.6]);
                    self.spawn_catch_shockwave(pos, [0.95, 0.6, 0.25]);
                    self.screen_shake = self.screen_shake.max(18.0);
                } else {
                    self.floating_texts.spawn(
                        format!("SHELL HOUSE CRACKED! {} LEFT", shells_left as u32),
                        pos - Vec2::new(120.0, 55.0),
                        28.0,
                        [0.95, 0.7, 0.35, 1.0],
                    );
                    self.spawn_catch_shockwave(pos, [0.9, 0.6, 0.25]);
                    self.screen_shake = self.screen_shake.max(10.0);
                }
            }
            for &pos in king_deflects.iter() {
                self.floating_texts.spawn(
                    "DEFLECTED — STOMP ON THE BEAT!".to_string(),
                    pos - Vec2::new(130.0, 50.0),
                    26.0,
                    [0.7, 0.8, 1.0, 1.0],
                );
            }
            self.stomp_cracked_buf = cracked; // hand the buffer back for reuse next frame
            self.hermit_popped_buf = hermit_popped; // hand the buffer back for reuse next frame
        }

        // Lasso: phase-driven state machine (Winding → Throwing → Snag → Dragging | Miss → Idle).
        // Winding charges while the mouse is held; Throwing advances each frame.
        {
            let player_center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            match self.lasso_phase {
                LassoPhase::Winding => {
                    // Grow charge and spin faster as it builds; cap at max.
                    self.lasso_charge = (self.lasso_charge + dt).min(LASSO_MAX_CHARGE_TIME);
                    let charge_frac = self.lasso_charge / LASSO_MAX_CHARGE_TIME;
                    // Loop spins faster as charge builds (cowboy wind-up feel).
                    self.lasso_spin += dt * (8.0 + charge_frac * 20.0);
                    // Keep lasso tip parked at player center while winding.
                    self.lasso_pos = Some(player_center);
                    // If mouse was released (fire_lasso_throw called), phase will already be Throwing.
                }
                LassoPhase::Throwing => {
                    self.lasso_timer -= dt;
                    // Charge fraction drives speed: a full charge covers max-range in LASSO_THROW_TIME;
                    // a tap covers only MIN_RANGE_FRAC of that (scales both range and tip travel).
                    let progress = (1.0 - self.lasso_timer / LASSO_THROW_TIME).clamp(0.0, 1.0);
                    let new_pos = self.lasso_origin.lerp(self.lasso_target, progress);
                    self.lasso_pos = Some(new_pos);
                    self.lasso_spin += dt * 18.0; // keep spinning during flight

                    if self.lasso_timer <= 0.0 {
                        // The throw has reached its target — check for catches.
                        let tip = self.lasso_target;
                        let grab_r = self.lasso_tip_radius();
                        let mut to_catch = std::mem::take(&mut self.lasso_catch_buf);
                        to_catch.clear();
                        to_catch.extend(
                            self.crabs
                                .iter()
                                .enumerate()
                                .filter(|(_, c)| c.is_catchable() && tip.distance(c.pos) < grab_r)
                                .map(|(i, _)| i),
                        );
                        if to_catch.is_empty() {
                            // Miss: loop flops empty with a dust puff.
                            self.lasso_pos = Some(self.lasso_target);
                            self.lasso_phase = LassoPhase::Miss;
                            self.lasso_timer = LASSO_MISS_TIME;
                            // WRONG-TOOL tell: if the loop actually landed *on* a still-shelled crab
                            // (Armored, or a Hermit with its borrowed shell up), the shell slipped it
                            // off — that's the "lasso slips off Armored" rule (enemies.rs). Without a
                            // cue this reads as a plain whiff; flag it so draw_lasso_shell_deflect can
                            // flash a hard grey-steel ricochet, teaching "crack the shell first (Stomp),
                            // then lasso." Mirrors the beam/Hermit amber can't-crack cue.
                            for c in self.crabs.iter() {
                                if c.boss_health > 0.0
                                    && (c.is_armored() || c.is_shelled_hermit())
                                    && tip.distance(c.pos) < grab_r
                                {
                                    self.lasso_shell_deflect_hits_buf.push(c.pos);
                                }
                            }
                        } else {
                            // Snag: loop tightens/squeezes before dragging.
                            self.lasso_pos = Some(self.lasso_target);
                            self.lasso_phase = LassoPhase::Snag;
                            self.lasso_timer = LASSO_SNAG_TIME;
                        }
                        let mut rng = crate::rng::rng();
                        let mut lasso_startle_origins = std::mem::take(&mut self.lasso_startle_buf);
                        lasso_startle_origins.clear();
                        for i in to_catch.iter().copied() {
                            let pos = self.crabs[i].pos;
                            let crab_type = self.crabs[i].crab_type;
                            let crab_color = self.crabs[i].crab_color();
                            self.particle_system
                                .spawn_catch_effect(pos, crab_color, crab_type, &mut rng);
                            self.spawn_catch_shockwave(pos, crab_color);
                            let was_answering = self.crabs[i].answering_call > 0.0;
                            // Strong-match: lasso catching a Thief (lasso is the intended counter
                            // to the Thief — so this hit is the archetype-tool pairing paying off).
                            if self.crabs[i].is_thief() {
                                self.lasso_thief_hits_buf.push(self.crabs[i].pos);
                            }
                            // Strong-match: lasso snagging a Magnet — the loop then drags it through
                            // the herd, turning the Magnet's pull field into a pied-piper sweep.
                            // Show a magnetic surge burst so the player reads "lasso + Magnet = cluster pull."
                            if self.crabs[i].is_magnet() {
                                self.lasso_magnet_hits_buf.push(self.crabs[i].pos);
                            }
                            // Strong-match: lasso hauling in a heavy Big crab. The whistle "shrugs
                            // most off" (whistle_pull 0.4), so the loop's physical drag is the Big
                            // crab's intended counter — show a straining "heave" so the pairing reads.
                            // On-beat throws (lasso_on_beat_bonus > 1.0) flare it brighter and wider,
                            // so timing the haul to the beat lands like a drum hit.
                            if self.crabs[i].is_big() && self.lasso_big_hits_buf.len() < 8 {
                                let on_beat = self.lasso_on_beat_bonus > 1.0;
                                self.lasso_big_hits_buf.push((self.crabs[i].pos, on_beat));
                            }
                            self.crabs[i].caught = true;
                            if let Some(t) = self.tutorial.as_mut() {
                                if t.kind == TutorialKind::LassoGrab {
                                    t.lasso_catches += 1;
                                }
                            }
                            if self.crabs[i].is_boss() {
                                self.on_boss_caught(pos, self.crabs[i].crab_type);
                            }
                            if self.crabs[i].is_golden() {
                                self.on_golden_caught(pos, 0);
                            }
                            self.reward_dance_catch(was_answering, pos);
                            lasso_startle_origins.push(pos);
                            self.chain_join_ripple = true;
                            self.crabs[i].chain_index = Some(self.chain_count);
                            self.chain_count += 1;
                            self.check_milestone(&mut crate::rng::rng());
                            self.score += self.combo_multiplier();
                            self.shake_timer = 0.15;
                            self.hitstop_timer = self.hitstop_timer.max(0.06);
                            self.time_since_catch = 0.0;
                            play_catch_sound(&mut self.sounds, ctx, &mut rng, self.beat_streak);
                            self.check_upgrade_unlock(ctx);
                        }
                        for &origin in lasso_startle_origins.iter() {
                            self.emit_catch_startle(origin);
                        }
                        self.lasso_catch_buf = to_catch;
                        self.lasso_startle_buf = lasso_startle_origins;
                    }
                }
                LassoPhase::Snag => {
                    self.lasso_timer -= dt;
                    self.lasso_spin += dt * 8.0;
                    if self.lasso_timer <= 0.0 {
                        self.lasso_phase = LassoPhase::Dragging;
                        self.lasso_timer = LASSO_DRAG_TIME;
                    }
                }
                LassoPhase::Dragging => {
                    self.lasso_timer -= dt;
                    let drag_t = (1.0 - self.lasso_timer / LASSO_DRAG_TIME).clamp(0.0, 1.0);
                    // Tip reels back from target to player center.
                    let new_pos = self.lasso_target.lerp(player_center, drag_t);
                    self.lasso_pos = Some(new_pos);
                    self.lasso_spin += dt * 6.0;
                    if self.lasso_timer <= 0.0 {
                        self.lasso_phase = LassoPhase::Idle;
                        self.lasso_pos = None;
                    }
                }
                LassoPhase::Miss => {
                    self.lasso_timer -= dt;
                    self.lasso_spin += dt * 4.0;
                    if self.lasso_timer <= 0.0 {
                        self.lasso_phase = LassoPhase::Idle;
                        self.lasso_pos = None;
                    }
                }
                LassoPhase::Idle => {}
            }
        }

        // Chain tail can catch nearby free crabs
        self.catch_by_chain(ctx);

        // Fire join-pulse ripple through the conga train on every new catch
        if self.chain_join_ripple {
            self.chain_join_ripple = false;
            for crab in &mut self.crabs {
                if crab.caught {
                    if let Some(ci) = crab.chain_index {
                        crab.join_pulse = 1.0 + ci as f32 * 0.21;
                    }
                }
            }
        }

        // Single pass over the herd covers every per-frame tally below (free-crab count for the
        // overwhelmed check, and whether a boss is alive) instead of scanning `self.crabs` three
        // separate times with overlapping predicates.
        let mut free_crab_count = 0usize;
        let mut boss_active = false;
        for c in &self.crabs {
            if !c.caught {
                free_crab_count += 1;
                if c.is_boss() {
                    boss_active = true;
                }
            }
        }

        // King Crab boss: once the player is rolling, send in a rare oversized crab that must be
        // worn down under the flashlight before it can be caught. Only one at a time.
        if self.score >= self.next_boss_score && !boss_active {
            self.next_boss_score = self.score + BOSS_SCORE_INTERVAL;
            // Rotate the boss archetypes so every run cycles through all five climax beats:
            // the King Crab (charge — route the train out of the lane), the Tide Boss (pulse — pull
            // the train back out of range), the Reef DJ (rhythm — its shell only drops when you
            // hold the light on it *on the beat*), the Hermit King (stomp — crack its shell-house
            // stack one pound at a time before it escapes), and the Dancer King (chase — pin down
            // the beat-teleporting evader and bank its entranced court with an on-beat catch).
            // Cycling guarantees variety instead of RNG streaks.
            let (boss, title, hint, title_color) = match self.next_boss_kind {
                1 => (
                    spawn_tide_boss(
                        (self.world_width, self.world_height),
                        &mut crate::rng::rng(),
                        BOSS_MAX_HEALTH,
                    ),
                    "A TIDE BOSS SURGES IN!",
                    "Hold your light — but keep your train clear of its pulse!",
                    [0.35, 0.8, 1.0, 1.0],
                ),
                2 => (
                    spawn_rhythm_boss(
                        (self.world_width, self.world_height),
                        &mut crate::rng::rng(),
                        BOSS_MAX_HEALTH,
                    ),
                    "THE REEF DJ DROPS IN!",
                    "Echo the lit pips with light — or catch its dancers on a hot beat!",
                    [0.75, 0.4, 1.0, 1.0],
                ),
                3 => (
                    spawn_hermit_king(
                        (self.world_width, self.world_height),
                        &mut crate::rng::rng(),
                        HERMIT_KING_SHELLS,
                    ),
                    "THE HERMIT KING LUMBERS IN!",
                    "Your light can't touch it — STOMP its shell houses, one crack at a time!",
                    [0.95, 0.6, 0.25, 1.0],
                ),
                4 => (
                    spawn_dancer_king(
                        (self.world_width, self.world_height),
                        &mut crate::rng::rng(),
                    ),
                    "THE DANCER KING TWIRLS IN!",
                    "Catch it before it teleports — ON the beat to bank its entranced court!",
                    [1.0, 0.65, 0.5, 1.0],
                ),
                _ => (
                    spawn_boss(
                        (self.world_width, self.world_height),
                        &mut crate::rng::rng(),
                        BOSS_MAX_HEALTH,
                    ),
                    "A KING CRAB APPROACHES!",
                    "Hold your light on it!",
                    [1.0, 0.8, 0.2, 1.0],
                ),
            };
            self.next_boss_kind = (self.next_boss_kind + 1) % 5;
            let bpos = boss.pos;
            self.crabs.push(boss);
            boss_active = true;
            free_crab_count += 1;
            // World-layer boss intro banners: anchor near the player so they read on-screen.
            self.floating_texts.spawn(
                title.to_string(),
                self.player_pos + Vec2::new(-230.0, -200.0),
                46.0,
                title_color,
            );
            self.floating_texts.spawn(
                hint.to_string(),
                self.player_pos + Vec2::new(-180.0, -150.0),
                26.0,
                [1.0, 0.95, 0.7, 0.9],
            );
            self.particle_system
                .spawn_milestone_fireworks(bpos, 12, &mut crate::rng::rng());
            let a = crate::rng::rng().random_range(0.0_f32..std::f32::consts::TAU);
            self.screen_shake = 18.0;
            self.screen_shake_vel = Vec2::new(a.cos(), a.sin()) * 18.0 * 60.0;
        }

        // Spatial King Crab boss rumble + intensity-scaled music layers.
        self.update_boss_and_music_audio(ctx, dt);

        // Game over if too many free crabs accumulate (overwhelmed). Reuses the single-pass tally
        // from above (plus the +1 for a boss spawned this frame) instead of a fresh linear scan.
        if free_crab_count >= 160 {
            self.game_over = true;
            return Ok(());
        }

        // Campaign win condition: evaluate the entered level's goal every frame during a campaign
        // run. The goal comes from the world-map node the player launched (NOT current_level,
        // which auto-advances when patterns run out). On win: latch once, celebrate briefly, then
        // return to the world map — which marks the node complete and unlocks the next one.
        if self.in_campaign && self.tutorial.is_none() && !self.game_over {
            if self.level_complete {
                self.level_complete_timer = (self.level_complete_timer - dt).max(0.0);
                if self.level_complete_timer <= 0.0 {
                    // The WinCondition was met — complete the node and unlock the next level.
                    self.return_to_world_map(true);
                }
            } else if let Some(cond) = self
                .world_map
                .as_ref()
                .and_then(|m| m.selected_level_index())
                .and_then(|i| self.levels.get(i))
                .map(|l| l.win_condition)
            {
                // HoldTrain streak clock: accumulate while the train holds the target, reset the
                // instant it dips below — a single bad moment resets a long streak, by design.
                if let crate::levels::WinCondition::HoldTrain { target, .. } = cond {
                    if self.chain_count >= target {
                        self.hold_train_timer += dt;
                    } else {
                        self.hold_train_timer = 0.0;
                    }
                }
                if cond.met(
                    self.banked_crabs_run,
                    self.chain_count,
                    self.shells_cracked_run,
                    self.hold_train_timer,
                ) {
                    self.level_complete = true;
                    self.level_complete_timer = 2.5;
                    let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
                    self.floating_texts.spawn(
                        "LEVEL COMPLETE!".to_string(),
                        center - Vec2::new(120.0, 80.0),
                        48.0,
                        [1.0, 0.9, 0.3, 1.0],
                    );
                    self.on_beat_flash = self.on_beat_flash.max(0.9);
                    self.screen_shake = self.screen_shake.max(10.0);
                    self.slowmo_timer = SLOWMO_DURATION;
                }
            }
        }

        // Bar-quantized spawns: a lapsed pattern doesn't spawn the next wave right away — it arms
        // it, and the beat handler drops the herd on the next downbeat so waves arrive locked to
        // the music. Whole field caught still counts, so the player is never left waiting with
        // nothing to chase. `wave_telegraph` counts up while armed to drive the draw-side flash.
        self.pattern_timer -= dt;
        // Boss set-piece: while a boss is on the field, hold the herd back so the encounter becomes
        // a focused duel instead of another crab lost in the crowd. The pattern timer keeps counting
        // down (clamped so it doesn't run away), so the instant the boss is caught the next wave
        // arms immediately and the run resumes without a dead beat. `boss_active` is the same
        // single-pass tally computed above (still valid — no crab was caught/removed since).
        if boss_active {
            self.pattern_timer = self.pattern_timer.max(-1.0);
        }
        if self.tutorial.is_none()
            && !self.wave_armed
            && !boss_active
            && (self.crabs.iter().all(|c| c.caught) || self.pattern_timer <= 0.0)
        {
            self.wave_armed = true;
            self.wave_telegraph = 0.0;
            // Decide up front whether the drop we're arming is a Frenzy: every 4th cleared wave,
            // but not the very first drop of the run. Set here (not at spawn time) so the gold
            // telegraph can warn the player through the whole arm window before it lands.
            self.frenzy_wave = self.waves_cleared > 0 && (self.waves_cleared + 1) % 4 == 0;
        }
        if self.wave_armed {
            self.wave_telegraph += dt;
            // Safety valve: if a downbeat somehow doesn't arrive within two bars (e.g. the beat
            // clock is paused), fire anyway so the run can't stall.
            if self.wave_telegraph > self.beat_interval * 8.0 {
                self.wave_armed = false;
                self.wave_telegraph = 0.0;
                self.advance_pattern();
            }
        }

        // Advance the ambient NPC conga train.
        self.update_npc_trains(dt);

        // Ambient field audio: steal stings, NPC-train rumble/motifs, crab-theme loops.
        self.update_ambient_audio(ctx, dt);

        // Recompute the camera every frame so both draw() and the mouse handlers (which run outside
        // draw) agree on the screen<->world mapping this frame.
        self.camera_origin = self.compute_camera_origin();
        // A catch can arm hitstop anywhere in the update above. Pause immediately rather than
        // waiting for the next frame, keeping the sample clock and frozen beat timer exact.
        if self.hitstop_timer > 0.0 {
            self.pause_gameplay_music();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::treasure_groove_level;

    #[test]
    fn treasure_fills_groove_only_on_beat() {
        assert_eq!(treasure_groove_level(0.0, true), 1.0);
        assert_eq!(treasure_groove_level(0.0, false), 0.5);
        assert_eq!(treasure_groove_level(0.8, false), 0.8);
    }
}
