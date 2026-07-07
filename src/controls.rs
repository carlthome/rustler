use crate::MainState;
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
    let mut dir = Vec2::ZERO;
    if ctx.keyboard.is_key_pressed(KeyCode::Up) || ctx.keyboard.is_key_pressed(KeyCode::W) {
        dir.y -= 1.0;
    }
    if ctx.keyboard.is_key_pressed(KeyCode::Down) || ctx.keyboard.is_key_pressed(KeyCode::S) {
        dir.y += 1.0;
    }
    if ctx.keyboard.is_key_pressed(KeyCode::Left) || ctx.keyboard.is_key_pressed(KeyCode::A) {
        dir.x -= 1.0;
    }
    if ctx.keyboard.is_key_pressed(KeyCode::Right) || ctx.keyboard.is_key_pressed(KeyCode::D) {
        dir.x += 1.0;
    }

    // Increase player speed and speed boost based on score.
    let base_speed = speed * (1.0 + state.score as f32 * 0.1);
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

    // Update player position with velocity and clamp to screen bounds.
    state.player_pos += state.player_vel * dt;
    state.player_pos.x = state.player_pos.x.clamp(0.0, width - crate::PLAYER_SIZE);
    state.player_pos.y = state.player_pos.y.clamp(0.0, height - crate::PLAYER_SIZE);

    // Rock chokepoints: patches are solid on the Rocky Shore. Push the player back out of any rock
    // they've overlapped and kill the inward velocity, so rocks read as walls to thread between
    // rather than terrain you can wade through.
    if terrain == TerrainKind::Rock {
        let mut center = state.player_pos + Vec2::splat(crate::PLAYER_SIZE / 2.0);
        // Only the biome's native patches are solid rock; trailing flood pools are water, not walls.
        for (c, r) in &state.tide_pools[..native_count] {
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
    }
}

pub fn handle_key_down_event(
    state: &mut MainState,
    ctx: &mut Context,
    keycode: Option<KeyCode>,
) -> bool {
    if let Some(key) = keycode {
        if state.show_instructions {
            if key == KeyCode::Space || key == KeyCode::Return {
                state.show_instructions = false;
                return true;
            }
            // Perk shop: spend banked crabs on permanent starting tool ranks before a run.
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
        } else if state.game_over {
            if key == KeyCode::Space || key == KeyCode::Return {
                state.reset_game();
                return true;
            }
        } else {
            if key == KeyCode::Space {
                if state.boost_cooldown <= 0.0 {
                    state.boost_timer = 0.18;
                    state.boost_cooldown = 0.08;
                    state.dash_just_fired = true;
                    state.dash_flash = 1.0;
                    // On-beat dash: reward the timing with groove + juice (no radius to scale).
                    let center = state.player_pos
                        + Vec2::new(crate::PLAYER_SIZE / 2.0, crate::PLAYER_SIZE / 2.0);
                    state.reward_on_beat_tool(center, "DASH");
                }
            }
            if key == KeyCode::Q {
                if !state.beat_wave_active {
                    state.beat_wave_active = true;
                    state.beat_wave_radius = 0.0;
                    let center = state.player_pos
                        + Vec2::new(crate::PLAYER_SIZE / 2.0, crate::PLAYER_SIZE / 2.0);
                    state.reward_on_beat_tool(center, "WAVE");
                }
            }
            if key == KeyCode::E {
                // Whistle: yank nearby crabs toward the player. Great for skittish Sneaky crabs.
                if state.whistle_cooldown <= 0.0 {
                    state.whistle_center =
                        state.player_pos + Vec2::new(crate::PLAYER_SIZE / 2.0, crate::PLAYER_SIZE / 2.0);
                    state.whistle_radius = 0.0;
                    state.whistle_active = 0.4;
                    state.whistle_cooldown = state.whistle_cooldown_dur();
                    // On-beat whistle reaches farther and pulls harder this cast.
                    state.whistle_beat_bonus =
                        state.reward_on_beat_tool(state.whistle_center, "WHISTLE");
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
                    let center =
                        state.player_pos + Vec2::new(crate::PLAYER_SIZE / 2.0, crate::PLAYER_SIZE / 2.0);
                    state.stomp_center = center;
                    state.stomp_radius = 0.0;
                    state.stomp_active = 0.32;
                    state.stomp_cooldown = state.stomp_cooldown_dur();
                    state.screen_shake = state.screen_shake.max(16.0);
                    state.zoom_punch = state.zoom_punch.max(0.05);
                    // On-beat stomp slams wider this cast.
                    state.stomp_beat_bonus = state.reward_on_beat_tool(center, "STOMP");
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
            }
            if key == KeyCode::Escape {
                ctx.request_quit();
            }
            if key == KeyCode::F2 {
                state.debug_mode = !state.debug_mode;
            }
        }
    }
    false
}
