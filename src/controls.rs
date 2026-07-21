use crate::MainState;
use crate::{
    SPRINT_SPEED_MULT, SPRINT_STAMINA_DRAIN_PER_SEC, SPRINT_STAMINA_MAX,
    SPRINT_STAMINA_REGEN_PER_SEC,
};
use ggez::Context;
use ggez::glam::Vec2;
use ggez::input::keyboard::KeyCode;
use ggez::winit::keyboard::PhysicalKey;

pub fn handle_player_movement(
    state: &mut MainState,
    ctx: &mut Context,
    dt: f32,
    speed: f32,
    area: (f32, f32),
) {
    let (width, height) = area;

    // Bot steal-back hold: a Force*Cross helper teleported the head onto a rival's follower slot this
    // frame to stage a steal-back, and the detection that reads it (update_npc_trains) hasn't run yet.
    // Freeze the head for exactly this frame — consuming the one-shot flag — so the seek-catch autopilot
    // can't re-steer it off the staged slot before the steal detection sees it (see BotState.hold_position).
    if let Some(bot) = state.bot.as_mut() {
        if bot.hold_position {
            bot.hold_position = false;
            state.player_vel = Vec2::ZERO;
            return;
        }
    }

    // Overlay bot synthetic key state if a bot script is running.
    let bot_up = state
        .bot
        .as_ref()
        .map_or(false, |b| b.keys_held.contains(&KeyCode::ArrowUp));
    let bot_down = state
        .bot
        .as_ref()
        .map_or(false, |b| b.keys_held.contains(&KeyCode::ArrowDown));
    let bot_left = state
        .bot
        .as_ref()
        .map_or(false, |b| b.keys_held.contains(&KeyCode::ArrowLeft));
    let bot_right = state
        .bot
        .as_ref()
        .map_or(false, |b| b.keys_held.contains(&KeyCode::ArrowRight));

    let mut dir = Vec2::ZERO;
    if ctx.keyboard.is_physical_key_pressed(&PhysicalKey::Code(KeyCode::ArrowUp)) || ctx.keyboard.is_physical_key_pressed(&PhysicalKey::Code(KeyCode::KeyW)) || bot_up
    {
        dir.y -= 1.0;
    }
    if ctx.keyboard.is_physical_key_pressed(&PhysicalKey::Code(KeyCode::ArrowDown))
        || ctx.keyboard.is_physical_key_pressed(&PhysicalKey::Code(KeyCode::KeyS))
        || bot_down
    {
        dir.y += 1.0;
    }
    if ctx.keyboard.is_physical_key_pressed(&PhysicalKey::Code(KeyCode::ArrowLeft))
        || ctx.keyboard.is_physical_key_pressed(&PhysicalKey::Code(KeyCode::KeyA))
        || bot_left
    {
        dir.x -= 1.0;
    }
    if ctx.keyboard.is_physical_key_pressed(&PhysicalKey::Code(KeyCode::ArrowRight))
        || ctx.keyboard.is_physical_key_pressed(&PhysicalKey::Code(KeyCode::KeyD))
        || bot_right
    {
        dir.x += 1.0;
    }

    // Seek-catch autopilot (see BotAction::SeekCatch): steer straight at the nearest catchable crab,
    // overriding the scripted keys. Paired with the auto-whistle in main.rs, this drives a reliable
    // catch through the real movement/charm/pull loop instead of a blind RNG-dependent sweep.
    if state.bot.as_ref().map_or(false, |b| b.seek_catch) {
        if let Some(target) = state.nearest_seek_target_pos() {
            let center = state.player_pos + Vec2::splat(crate::PLAYER_SIZE / 2.0);
            let toward = target - center;
            let dist = toward.length();
            if dist > 1.0 {
                dir = toward.normalize();
            }
            if state.bot.as_ref().map_or(false, |b| b.seek_lasso) {
                if let Some(target) = state.nearest_catchable_crab_pos() {
                    let center = state.player_pos + Vec2::splat(crate::PLAYER_SIZE / 2.0);
                    let toward = target - center;
                    if toward.length_squared() > 1.0 {
                        dir = toward.normalize();
                    }
                }
            }
            if state.bot.as_ref().map_or(false, |b| b.seek_delivery) && state.chain_count > 0 {
                // The campaign bot is checking the real delivery transition, not pathfinding. Stage the
                // head in the pen so large-map camera travel and terrain cannot make this regression test
                // depend on a route or frame budget.
                state.player_pos = state.pen_pos - Vec2::splat(crate::PLAYER_SIZE / 2.0);
                state.player_vel = Vec2::ZERO;
                dir = Vec2::ZERO;
            }
            // Beat-timed final approach — BeatTiming tutorial only. A skilled player holds just
            // outside catch range and closes the last step ON the beat so the catch counts.
            // Otherwise the autopilot fires the whistle the instant its 4.5 s cooldown clears, and
            // because 4.5 s is exactly 9 beats (BEAT_INTERVAL 0.5 s) every reeled-in catch
            // phase-locks to one beat phase; when that phase is off-beat a whole run banks zero
            // on-beat catches and the tutorial never passes — the campaign_tutorial playtest flake.
            // Gating the final step on the beat decorrelates the catch from that grid so on-beat
            // catches reliably land. Scoped to the un-completed BeatTiming tutorial, so the steal and
            // menu scenarios (which don't check on-beat) keep their fast straight-in seek unchanged.
            let in_beat_tutorial = state.tutorial.as_ref().map_or(false, |t| {
                t.kind == crate::tutorial::TutorialKind::BeatTiming && !t.completed
            });
            if in_beat_tutorial {
                let on_beat = state.beat_timer < crate::BEAT_WINDOW
                    || state.beat_timer > state.beat_interval - crate::BEAT_WINDOW;
                // Approximate half-extent of the catch box (see the proximity check in update()).
                let catch_reach = crate::PLAYER_SIZE * 0.6 + 26.0;
                if !on_beat && dist < catch_reach + 44.0 {
                    // Off the beat with a crab at the doorstep. The whistle reels the charmed crab
                    // TO the player, so merely holding still lets it drift into the catch box and
                    // register an off-beat catch that doesn't count. Keep it just outside catch
                    // range instead — back off if it has already crept inside, otherwise hold — and
                    // brake so momentum can't carry us in. On the next on-beat frame `dir` again
                    // points at the crab and we close the final step, landing the catch on-beat.
                    dir = if dist < catch_reach {
                        (-toward).normalize_or_zero()
                    } else {
                        Vec2::ZERO
                    };
                    state.player_vel *= 0.35;
                }
            }
        }
    }

    let sprint_held = ctx.keyboard.is_physical_key_pressed(&PhysicalKey::Code(KeyCode::ShiftLeft))
        || ctx.keyboard.is_physical_key_pressed(&PhysicalKey::Code(KeyCode::ShiftRight));
    let sprinting =
        sprint_held && dir != Vec2::ZERO && state.boost_timer <= 0.0 && state.sprint_stamina > 0.0;

    // Increase player speed and speed boost based on score.
    let base_speed = speed * (1.0 + state.score as f32 * 0.1) * state.speed_mult;
    let speed_boost_multiplier = 30.0 + state.score as f32 * 0.2;
    let mut move_speed = base_speed;

    // Apply speed boost if available.
    if state.boost_timer > 0.0 {
        move_speed *= speed_boost_multiplier;
    }

    // Handle player movement direction and velocity.
    let mut acceleration = if state.boost_timer > 0.0 {
        4000.0
    } else {
        1000.0
    };
    let mut friction = if state.boost_timer > 0.0 { 0.9 } else { 0.9 };

    // Train weight: a longer conga line handles heavier — it shaves top speed and makes
    // acceleration/turning lazier, so hauling a big, valuable train to the pen is a real
    // handling tradeoff you feel in your hands, not just abstract chain-snap risk. Weight is
    // correlated with score (which speeds you up), so it never fully stalls you; and because
    // banking at the pen resets the chain, a successful delivery rewards you with an immediate
    // burst of nimbleness. Dashes ignore the weight entirely (boost branch skipped), keeping the
    // dash a punchy escape you can still fire to shed a charging King Crab even with a huge tail.
    if state.boost_timer <= 0.0 {
        let weight = state.chain_count as f32;
        let handling = (1.0 / (1.0 + weight * 0.035)).max(0.55);
        move_speed *= handling;
        acceleration *= handling;
    }

    if sprinting {
        move_speed *= SPRINT_SPEED_MULT;
        acceleration *= 1.3;
    }

    // Biome terrain: the same patch geometry means different things per zone (see levels.rs).
    // Water drags you to a wade, Kelp clings (a lighter drag, plus a tail-snag risk handled in
    // main.rs), Rock is a solid obstacle that shoves you out, and Open has no patches at all. A
    // dash still punches through drag faster than a wade (its speed cap is huge), rewarding routing
    // skill. `in_tide_pool` is remembered so the draw pass can splash on wet terrain.
    use crate::levels::TerrainKind;
    let terrain = state.current_terrain();
    let player_center = state.player_pos + Vec2::splat(crate::PLAYER_SIZE / 2.0);
    // The biome's native patches are all but the last `boss_flood_pools` entries; those trailing
    // entries are Tide Boss surge water (see main.rs) and always drag like water regardless of the
    // biome terrain, so a flood on a Rock/Open zone is a real routing change, not just a visual.
    let native_count = state
        .tide_pools
        .len()
        .saturating_sub(state.boss_flood_pools);
    let touching = state.tide_pools[..native_count]
        .iter()
        .any(|(c, r)| player_center.distance(*c) < *r);
    let in_flood = state.tide_pools[native_count..]
        .iter()
        .any(|(c, r)| player_center.distance(*c) < *r);
    state.in_tide_pool =
        in_flood || (touching && matches!(terrain, TerrainKind::Water | TerrainKind::Kelp));
    match terrain {
        TerrainKind::Water if touching => {
            // Cut top speed and bleed off momentum faster while submerged.
            move_speed *= 0.5;
            friction *= 0.82;
        }
        TerrainKind::Kelp if touching => {
            // Weeds cling — a lighter drag than open water; the real bite is the tail-snag risk.
            move_speed *= 0.7;
            friction *= 0.9;
        }
        _ => {}
    }
    // Tide Boss flood water: same wade-drag as native water, applied on any biome.
    if in_flood {
        move_speed *= 0.5;
        friction *= 0.82;
    }

    if dir != Vec2::ZERO {
        let dir = dir.normalize();
        // Apply strong acceleration when boosting, like a rocket
        state.player_vel = state.player_vel * friction + dir * acceleration * dt;
        state.last_dir = dir;
    } else {
        // Decelerate player if no input is given.
        state.player_vel *= friction;
    }

    // Apply speed limit to player velocity.
    if state.player_vel.length() > move_speed {
        state.player_vel = state.player_vel.normalize() * move_speed;
    }

    if sprinting {
        state.sprint_stamina = (state.sprint_stamina - SPRINT_STAMINA_DRAIN_PER_SEC * dt).max(0.0);
    } else {
        state.sprint_stamina =
            (state.sprint_stamina + SPRINT_STAMINA_REGEN_PER_SEC * dt).min(SPRINT_STAMINA_MAX);
    }

    // Update player position with velocity and clamp to screen bounds.
    state.player_pos += state.player_vel * dt;
    state.player_pos.x = state.player_pos.x.clamp(0.0, width - crate::PLAYER_SIZE);
    state.player_pos.y = state.player_pos.y.clamp(0.0, height - crate::PLAYER_SIZE);

    // Rock chokepoints: patches are solid on the Rocky Shore. Push the player back out of any rock
    // they've overlapped and kill the inward velocity, so rocks read as walls to thread between
    // rather than terrain you can wade through. The tide wrinkle: while the sea is in (rock_tide_open),
    // the *low* rocks (see MainState::rock_is_low) are submerged — they stop blocking and instead
    // wade-drag the player like shallow water, opening a beat-timed shortcut through the chokepoint.
    // High rocks stay solid regardless, so there's always a wall to route around.
    //
    // The Desktop biome reuses this exact push-out collision: its "window" panels are solid walls the
    // player (and so the conga train they lead) routes around, just like rocks. It has no tide, so
    // `rock_tide_open()` stays false there (update_rock_tide only ticks on Rock, leaving rock_tide_fill
    // at 0) — every window is always solid, no submerge shortcut. Sharing this branch is why the
    // Desktop level needs no new physics.
    if matches!(terrain, TerrainKind::Rock | TerrainKind::Desktop) {
        let tide_open = state.rock_tide_open();
        // First resolve solid collisions, skipping any low rock that's currently under water.
        let mut center = state.player_pos + Vec2::splat(crate::PLAYER_SIZE / 2.0);
        // Only the biome's native patches are solid rock; trailing flood pools are water, not walls.
        for (i, (c, r)) in state.tide_pools[..native_count].iter().enumerate() {
            if tide_open && crate::MainState::rock_is_low(i) {
                continue; // submerged low rock — passable this beat, handled as wade-drag below
            }
            let to_player = center - *c;
            let dist = to_player.length();
            if dist < *r && dist > 0.0001 {
                let push = to_player / dist * (*r - dist);
                state.player_pos += push;
                // Cancel the component of velocity heading into the rock so you don't stick to it.
                let normal = to_player / dist;
                let into = state.player_vel.dot(normal);
                if into < 0.0 {
                    state.player_vel -= normal * into;
                }
                center = state.player_pos + Vec2::splat(crate::PLAYER_SIZE / 2.0);
            }
        }
        state.player_pos.x = state.player_pos.x.clamp(0.0, width - crate::PLAYER_SIZE);
        state.player_pos.y = state.player_pos.y.clamp(0.0, height - crate::PLAYER_SIZE);
        // If the player is standing in a submerged low rock, it wades like water: mark it wet so the
        // splash juice fires, and bleed a touch of speed so crossing the flooded gap still costs
        // something (a dash still blows through it — routing skill rewarded, same as the water biome).
        if tide_open {
            let here = state.player_pos + Vec2::splat(crate::PLAYER_SIZE / 2.0);
            let in_low_rock = state.tide_pools[..native_count]
                .iter()
                .enumerate()
                .any(|(i, (c, r))| crate::MainState::rock_is_low(i) && here.distance(*c) < *r);
            if in_low_rock {
                state.in_tide_pool = true;
                state.player_vel *= 0.78; // gentle wade drag on the flooded shortcut
            }
        }
    }
}

pub fn handle_key_down_event(
    state: &mut MainState,
    ctx: &mut Context,
    keycode: Option<KeyCode>,
) -> bool {
    if let Some(key) = keycode {
        if state.show_instructions && !state.menu_intro_complete {
            if key == KeyCode::Space {
                state.skip_menu_intro();
            }
            return true;
        }
        if state.show_world_map {
            match key {
                KeyCode::ArrowLeft | KeyCode::KeyA => {
                    if let Some(map) = &mut state.world_map {
                        map.move_selection(-1);
                    }
                    return true;
                }
                KeyCode::ArrowRight | KeyCode::KeyD => {
                    if let Some(map) = &mut state.world_map {
                        map.move_selection(1);
                    }
                    return true;
                }
                KeyCode::Space | KeyCode::Enter => {
                    if let Some(map) = &mut state.world_map {
                        if map.selected_unlocked() {
                            // Already unlocked — launch straight into it.
                            map.cancel_skip();
                            state.enter_campaign_level();
                        } else if map.skip_pending() {
                            // Second Confirm on a locked node — commit the skip and launch.
                            map.unlock_through_selected();
                            state.enter_campaign_level();
                        } else {
                            // First Confirm on a locked node — arm the soft warning.
                            map.arm_skip_warning();
                        }
                    }
                    return true;
                }
                KeyCode::Escape => {
                    // If a skip warning is armed, the first Esc just cancels it (back out of the
                    // skip); otherwise Esc leaves the map back to the menu.
                    if let Some(map) = &mut state.world_map {
                        if map.skip_pending() {
                            map.cancel_skip();
                            return true;
                        }
                    }
                    state.return_to_main_menu();
                    return true;
                }
                _ => {}
            }
        } else if key == KeyCode::KeyM {
            // M toggles music mute (beats stay, music pauses)
            state.music_muted = !state.music_muted;
            return true;
        } else if state.show_instructions {
            // While the plain-text How To Play card is open, any confirm/back key returns to Home.
            if state.show_how_to_play_text {
                if matches!(key, KeyCode::Escape | KeyCode::Space | KeyCode::Enter) {
                    state.show_how_to_play_text = false;
                    state.menu_page = 0;
                }
                return true;
            }
            if state.show_play_recommendation {
                match key {
                    KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::ArrowLeft | KeyCode::ArrowRight => {
                        state.play_recommendation_continue_selected =
                            !state.play_recommendation_continue_selected;
                    }
                    KeyCode::Space | KeyCode::Enter => {
                        if state.play_recommendation_continue_selected {
                            state.reset_game();
                            state.show_instructions = false;
                        }
                        state.show_play_recommendation = false;
                    }
                    KeyCode::Escape => {
                        state.show_play_recommendation = false;
                    }
                    _ => {}
                }
                return true;
            }
            // Escape: from Loadout go back to Home; from Home do nothing (use Quit button).
            if key == KeyCode::Escape {
                if state.menu_page == 1 {
                    state.menu_page = 0;
                    return true;
                }
            }
            // Tab: on Loadout, cycle the skin slot.
            if key == KeyCode::Tab {
                if state.menu_page == 1 {
                    state.skin_slot = (state.skin_slot + 1) % 3;
                    return true;
                }
            }
            if state.menu_page == 1 && key == KeyCode::Backspace {
                state.pop_player_name_char();
                return true;
            }
            // Home page: Up/Down navigate, Space/Enter activates.
            if state.menu_page == 0 {
                const NUM_BUTTONS: usize = 5;
                match key {
                    KeyCode::ArrowUp => {
                        state.menu_selection =
                            (state.menu_selection + NUM_BUTTONS - 1) % NUM_BUTTONS;
                        return true;
                    }
                    KeyCode::ArrowDown => {
                        state.menu_selection = (state.menu_selection + 1) % NUM_BUTTONS;
                        return true;
                    }
                    KeyCode::Space | KeyCode::Enter => {
                        match state.menu_selection {
                            0 => {
                                state.show_how_to_play_text = false;
                                state.show_play_recommendation = true;
                                state.play_recommendation_continue_selected = true;
                            } // Play
                            1 => {
                                state.enter_world_map(ctx);
                            } // Campaign
                            2 => {
                                state.menu_page = 1;
                                state.menu_selection = 0;
                                state.show_how_to_play_text = false;
                            } // Loadout
                            3 => {
                                state.show_how_to_play_text = true;
                                state.menu_page = 0;
                            } // How to Play
                            4 => {
                                ctx.request_quit();
                            } // Quit
                            _ => {}
                        }
                        return true;
                    }
                    // Legacy shortcut: C still opens campaign.
                    KeyCode::KeyC => {
                        state.enter_world_map(ctx);
                        return true;
                    }
                    _ => {}
                }
            }
            // Loadout-page-only keys: skin picker and perk shop.
            if state.menu_page == 1 {
                if key == KeyCode::ArrowLeft {
                    state.cycle_skin_option(-1);
                    return true;
                }
                if key == KeyCode::ArrowRight {
                    state.cycle_skin_option(1);
                    return true;
                }
                match key {
                    KeyCode::Digit1 => {
                        state.buy_start_perk(1);
                        return true;
                    }
                    KeyCode::Digit2 => {
                        state.buy_start_perk(2);
                        return true;
                    }
                    KeyCode::Digit3 => {
                        state.buy_start_perk(3);
                        return true;
                    }
                    KeyCode::Digit4 => {
                        state.buy_start_perk(4);
                        return true;
                    }
                    _ => {}
                }
            }
        } else if state.game_over {
            if matches!(key, KeyCode::Space | KeyCode::Enter | KeyCode::Escape) {
                if key == KeyCode::Escape {
                    state.return_to_main_menu();
                } else if state.in_campaign {
                    // Dismissing the game-over screen after LOSING a campaign run: return to the
                    // map but don't complete the node — the win condition still gates the next level.
                    state.return_to_world_map(false);
                } else {
                    state.reset_game();
                }
                return true;
            }
        } else {
            if key == KeyCode::Space {
                // #165 groove chord — SPACE is the unified beat-tap. Tapped alone it dashes
                // (unchanged, and Carl's explicit "don't touch the dash"). Tapped while a tool key
                // is held it fires that tool ON this beat-tap instead of dashing — so the player
                // keeps ONE timing to learn ("tap SPACE on the beat") and flavors the beat with a
                // chord rather than juggling each tool's own independent on-beat window. Fully
                // additive and reversible: the standalone E/R/Q keys still fire their tools on their
                // own, and SPACE with no tool held still dashes byte-for-byte as before.
                // A tool held down counts whether it's a real key or a bot's synthetic key (the
                // groove_dash playtest drives the chord this way), mirroring handle_player_movement.
                let held = |code: KeyCode| -> bool {
                    ctx.keyboard
                        .is_physical_key_pressed(&PhysicalKey::Code(code))
                        || state
                            .bot
                            .as_ref()
                            .map_or(false, |b| b.keys_held.contains(&code))
                };
                let whistle_chord = held(KeyCode::KeyE);
                let stomp_chord = held(KeyCode::KeyR);
                let wave_chord = held(KeyCode::KeyQ);
                if whistle_chord || stomp_chord || wave_chord {
                    // Flavor this beat with the held tool(s) — a chord may layer more than one.
                    if whistle_chord {
                        state.fire_whistle();
                    }
                    if stomp_chord {
                        state.fire_stomp();
                    }
                    if wave_chord {
                        state.fire_wave();
                    }
                    state.chord_tools_fired += 1;
                } else if state.boost_cooldown <= 0.0 {
                    state.boost_timer = 0.18;
                    state.boost_cooldown = 0.08;
                    state.dash_just_fired = true;
                    state.dash_flash = 1.0;
                    let center = state.player_pos
                        + Vec2::new(crate::PLAYER_SIZE / 2.0, crate::PLAYER_SIZE / 2.0);
                    // On-beat dash → GROOVE DASH: punches farther, sweeps nearby crabs into your path.
                    // Off-beat dash → GROOVE PENALTY: the dash fires, but poor timing bleeds the meter.
                    // The dash is never blocked (it's still an escape tool) but the rhythm cost is real.
                    let bonus = state.reward_on_beat_tool(center, "GROOVE DASH");
                    if bonus > 1.0 {
                        state.boost_timer = 0.26; // punch a touch farther on the beat
                        state.groove_dash_timer = 0.22;
                        state.groove_dash_center = center;
                        // Gather-wake follows the dash heading: current momentum if you're moving,
                        // else the last-faced direction so a standing on-beat dash still sweeps.
                        let d = if state.player_vel.length() > 5.0 {
                            state.player_vel.normalize_or_zero()
                        } else {
                            state.last_dir.normalize_or_zero()
                        };
                        state.groove_dash_dir = d;
                    } else {
                        // Off-beat: bleed groove and reset the beat streak — sloppy rhythm costs something.
                        state.groove = (state.groove - 0.09).max(0.0);
                        state.beat_streak = state.beat_streak.saturating_sub(1);
                        state.shop_denied = state.shop_denied.max(0.35); // red flash so the miss reads
                        state.floating_texts.spawn(
                            "off-beat dash".to_string(),
                            center - Vec2::new(42.0, 60.0),
                            18.0,
                            [0.9, 0.4, 0.4, 0.85],
                        );
                    }
                }
            }
            if key == KeyCode::KeyQ {
                // Wave: an on-beat space-clearing shockwave — shoves nearby rival leaders back and
                // stuns them (and still cancels a rival mid-steal as a save). Distinct from the
                // Stomp's precise close parry. Same cast as the SPACE+Q chord.
                state.fire_wave();
            }
            if key == KeyCode::KeyE {
                // Whistle: yank nearby crabs toward the player. Same cast as the SPACE+E chord.
                state.fire_whistle();
            }
            if key == KeyCode::KeyR {
                // Stomp: cracks armored shells / up-close parry. Same cast as the SPACE+R chord.
                state.fire_stomp();
            }
            if key == KeyCode::KeyT {
                // Call (T): a rhythm summon. On the beat, nearby Dancer crabs answer and hop toward
                // you. Lives on T because F is the flashlight toggle (handled in main.rs, which
                // returns before this runs) — so this was previously dead-keyed on F and unreachable.
                state.issue_call();
            }
            if key == KeyCode::KeyX {
                // Cycle: the reposition verb. On the beat it rotates the whole train one slot,
                // arranging the coveted head/tail ends (mouse-free; the old interior-bubble mode was
                // removed with the mouse dependency).
                state.cycle_train();
            }
            if key == KeyCode::KeyV {
                // Groove Call: a field-wide beat lure. Call on the beat and the WHOLE herd streams
                // toward you over the next couple bars, surging on each downbeat.
                state.issue_groove_call();
            }
            if key == KeyCode::KeyG {
                // Downbeat Slam: the Groove-meter ultimate. Only fires with a full meter on the beat;
                // yanks every nearby free crab into the train at once for a spectacle payoff.
                state.downbeat_slam(ctx);
            }
            if key == KeyCode::KeyB {
                // Bank: cash out the live Groove Gamble streak into a safe multiplier floor. On the
                // beat it locks the whole stack; off-beat takes a haircut. Turns the gamble into an
                // active "when do I bank?" call instead of a passive streak.
                state.bank_gamble();
                // Also: jam emote! Your crab vibes. Plays a hi-hat and does a little shimmy.
                state.jam_timer = 0.55;
                use ggez::audio::SoundSource;
                let _ = state.sounds.hihat.play();
            }
            if key == KeyCode::Escape {
                state.return_to_main_menu();
            }
            if key == KeyCode::F2 {
                state.debug_mode = !state.debug_mode;
            }
        }
    }
    false
}
