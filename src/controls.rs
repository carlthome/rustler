use crate::MainState;
use crate::{
    SPRINT_SPEED_MULT, SPRINT_STAMINA_DRAIN_PER_SEC, SPRINT_STAMINA_MAX,
    SPRINT_STAMINA_REGEN_PER_SEC,
};
use ggez::Context;
use ggez::glam::Vec2;
use ggez::input::keyboard::KeyCode;

pub fn handle_player_movement(
    state: &mut MainState,
    ctx: &mut Context,
    dt: f32,
    speed: f32,
    area: (f32, f32),
) {
    let (width, height) = area;

    // Overlay bot synthetic key state if a bot script is running.
    let bot_up = state
        .bot
        .as_ref()
        .map_or(false, |b| b.keys_held.contains(&KeyCode::Up));
    let bot_down = state
        .bot
        .as_ref()
        .map_or(false, |b| b.keys_held.contains(&KeyCode::Down));
    let bot_left = state
        .bot
        .as_ref()
        .map_or(false, |b| b.keys_held.contains(&KeyCode::Left));
    let bot_right = state
        .bot
        .as_ref()
        .map_or(false, |b| b.keys_held.contains(&KeyCode::Right));

    let mut dir = Vec2::ZERO;
    if ctx.keyboard.is_key_pressed(KeyCode::Up) || ctx.keyboard.is_key_pressed(KeyCode::W) || bot_up
    {
        dir.y -= 1.0;
    }
    if ctx.keyboard.is_key_pressed(KeyCode::Down)
        || ctx.keyboard.is_key_pressed(KeyCode::S)
        || bot_down
    {
        dir.y += 1.0;
    }
    if ctx.keyboard.is_key_pressed(KeyCode::Left)
        || ctx.keyboard.is_key_pressed(KeyCode::A)
        || bot_left
    {
        dir.x -= 1.0;
    }
    if ctx.keyboard.is_key_pressed(KeyCode::Right)
        || ctx.keyboard.is_key_pressed(KeyCode::D)
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
            if toward.length() > 1.0 {
                dir = toward.normalize();
            }
        }
    }

    let sprint_held = ctx.keyboard.is_key_pressed(KeyCode::LShift)
        || ctx.keyboard.is_key_pressed(KeyCode::RShift);
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
    if terrain == TerrainKind::Rock {
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
        if state.show_world_map {
            match key {
                KeyCode::Left | KeyCode::A => {
                    if let Some(map) = &mut state.world_map {
                        map.move_selection(-1);
                    }
                    return true;
                }
                KeyCode::Right | KeyCode::D => {
                    if let Some(map) = &mut state.world_map {
                        map.move_selection(1);
                    }
                    return true;
                }
                KeyCode::Space | KeyCode::Return => {
                    state.enter_campaign_level();
                    return true;
                }
                KeyCode::Escape => {
                    state.show_world_map = false;
                    state.show_instructions = true;
                    state.show_how_to_play_text = false;
                    return true;
                }
                _ => {}
            }
        } else if key == KeyCode::M {
            // M toggles music mute (beats stay, music pauses)
            state.music_muted = !state.music_muted;
            return true;
        } else if state.show_instructions {
            // While the plain-text How To Play card is open, any confirm/back key returns to Home.
            if state.show_how_to_play_text {
                if matches!(key, KeyCode::Escape | KeyCode::Space | KeyCode::Return) {
                    state.show_how_to_play_text = false;
                    state.menu_page = 0;
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
            if state.menu_page == 1 && key == KeyCode::Back {
                state.pop_player_name_char();
                return true;
            }
            // Home page: Up/Down navigate, Space/Enter activates.
            if state.menu_page == 0 {
                const NUM_BUTTONS: usize = 5;
                match key {
                    KeyCode::Up => {
                        state.menu_selection =
                            (state.menu_selection + NUM_BUTTONS - 1) % NUM_BUTTONS;
                        return true;
                    }
                    KeyCode::Down => {
                        state.menu_selection = (state.menu_selection + 1) % NUM_BUTTONS;
                        return true;
                    }
                    KeyCode::Space | KeyCode::Return => {
                        match state.menu_selection {
                            0 => {
                                state.show_instructions = false;
                                state.show_how_to_play_text = false;
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
                    KeyCode::C => {
                        state.enter_world_map(ctx);
                        return true;
                    }
                    _ => {}
                }
            }
            // Loadout-page-only keys: skin picker and perk shop.
            if state.menu_page == 1 {
                if key == KeyCode::Left {
                    state.cycle_skin_option(-1);
                    return true;
                }
                if key == KeyCode::Right {
                    state.cycle_skin_option(1);
                    return true;
                }
                match key {
                    KeyCode::Key1 => {
                        state.buy_start_perk(1);
                        return true;
                    }
                    KeyCode::Key2 => {
                        state.buy_start_perk(2);
                        return true;
                    }
                    KeyCode::Key3 => {
                        state.buy_start_perk(3);
                        return true;
                    }
                    KeyCode::Key4 => {
                        state.buy_start_perk(4);
                        return true;
                    }
                    _ => {}
                }
            }
        } else if state.game_over {
            if key == KeyCode::Space || key == KeyCode::Return {
                if state.in_campaign {
                    state.return_to_world_map();
                } else {
                    state.reset_game();
                }
                return true;
            }
        } else {
            if key == KeyCode::Space {
                if state.boost_cooldown <= 0.0 {
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
            if key == KeyCode::Q {
                if !state.beat_wave_active {
                    state.beat_wave_active = true;
                    state.beat_wave_radius = 0.0;
                    let center = state.player_pos
                        + Vec2::new(crate::PLAYER_SIZE / 2.0, crate::PLAYER_SIZE / 2.0);
                    state.reward_on_beat_tool(center, "WAVE");
                    // Defensive parry: the Wave is the wide ranged save — an on-beat cast repels a
                    // rival mid-steal from clear across the lane.
                    state.try_defend_steal(center, crate::WAVE_DEFEND_RADIUS, "WAVE");
                }
            }
            if key == KeyCode::E {
                // Whistle: yank nearby crabs toward the player. Great for skittish Sneaky crabs.
                if state.whistle_cooldown <= 0.0 {
                    state.whistle_center = state.player_pos
                        + Vec2::new(crate::PLAYER_SIZE / 2.0, crate::PLAYER_SIZE / 2.0);
                    state.whistle_radius = 0.0;
                    state.whistle_active = 0.4;
                    state.whistle_cooldown = state.whistle_cooldown_dur();
                    // On-beat whistle reaches farther and pulls harder this cast.
                    state.whistle_beat_bonus =
                        state.reward_on_beat_tool(state.whistle_center, "WHISTLE");
                    {
                        use ggez::audio::SoundSource;
                        let _ = state.sounds.whistle_sfx.play_detached(ctx);
                    }
                    state.floating_texts.spawn(
                        "WHISTLE!".to_string(),
                        state.whistle_center - Vec2::new(48.0, 60.0),
                        30.0,
                        [1.0, 0.85, 0.35, 1.0],
                    );
                }
            }
            if key == KeyCode::R {
                // Stomp: a close-range ground-pound that cracks armored crab shells wide open.
                if state.stomp_cooldown <= 0.0 {
                    let center = state.player_pos
                        + Vec2::new(crate::PLAYER_SIZE / 2.0, crate::PLAYER_SIZE / 2.0);
                    state.stomp_center = center;
                    state.stomp_radius = 0.0;
                    state.stomp_active = 0.32;
                    state.stomp_cooldown = state.stomp_cooldown_dur();
                    state.screen_shake = 22.0;
                    state.screen_shake_vel = ggez::glam::Vec2::new(0.0, 1.0) * 22.0 * 60.0;
                    state.zoom_punch = state.zoom_punch.max(0.08);
                    // On-beat stomp slams wider this cast.
                    state.stomp_beat_bonus = state.reward_on_beat_tool(center, "STOMP");
                    // Defensive parry: the Stomp is the up-close bodyguard — an on-beat pound cancels
                    // a rival's armed splice if it's threading your tail right on top of you.
                    state.try_defend_steal(center, crate::STOMP_DEFEND_RADIUS, "STOMP");
                    {
                        use ggez::audio::SoundSource;
                        let _ = state.sounds.stomp_sfx.play_detached(ctx);
                    }
                    state.floating_texts.spawn(
                        "STOMP!".to_string(),
                        center - Vec2::new(40.0, 60.0),
                        30.0,
                        [0.85, 0.8, 0.7, 1.0],
                    );
                }
            }
            if key == KeyCode::F {
                // Call: a rhythm summon. On the beat, nearby Dancer crabs answer and hop toward you.
                state.issue_call();
            }
            if key == KeyCode::X {
                // Cycle: the reposition verb. On the beat: aim at an interior link to BUBBLE that
                // crab one slot toward the centre (build a centerpiece on purpose); aim at nothing
                // to rotate the whole train one slot and arrange the coveted head/tail ends.
                state.cycle_train();
            }
            if key == KeyCode::V {
                // Groove Call: a field-wide beat lure. Call on the beat and the WHOLE herd streams
                // toward you over the next couple bars, surging on each downbeat.
                state.issue_groove_call();
            }
            if key == KeyCode::G {
                // Downbeat Slam: the Groove-meter ultimate. Only fires with a full meter on the beat;
                // yanks every nearby free crab into the train at once for a spectacle payoff.
                state.downbeat_slam(ctx);
            }
            if key == KeyCode::B {
                // Bank: cash out the live Groove Gamble streak into a safe multiplier floor. On the
                // beat it locks the whole stack; off-beat takes a haircut. Turns the gamble into an
                // active "when do I bank?" call instead of a passive streak.
                state.bank_gamble();
                // Also: jam emote! Your crab vibes. Plays a hi-hat and does a little shimmy.
                state.jam_timer = 0.55;
                use ggez::audio::SoundSource;
                let _ = state.sounds.hihat.play(ctx);
            }
            if key == KeyCode::Escape {
                if state.tutorial.is_some() {
                    // In a tutorial, Escape backs out to the title screen (opt-in exit) rather than
                    // quitting the game — and never through game_over, so career stats stay clean.
                    state.tutorial = None;
                    state.show_instructions = true;
                    state.show_how_to_play_text = false;
                } else {
                    ctx.request_quit();
                }
            }
            if key == KeyCode::F2 {
                state.debug_mode = !state.debug_mode;
            }
        }
    }
    false
}
