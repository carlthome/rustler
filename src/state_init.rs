//! `MainState::new` — the one-time game/world constructor.
//!
//! Extracted verbatim from `state.rs` so that file stays focused on the `MainState`
//! struct definition and the small per-frame helpers. Pure structural move: the
//! constructor's behaviour is unchanged.

use std::{collections::VecDeque, fs};

use ggez::audio::{SoundSource, Source};
use ggez::glam::Vec2;
use ggez::graphics::{Image, ShaderBuilder, ShaderParamsBuilder};
use ggez::{Context, GameResult};
use rand::Rng;
use rand::prelude::IndexedRandom;

use crate::constants::*;
use crate::enemies::EnemyCrab;
use crate::graphics::{FloatingTextSystem, ParticleSystem, PennedMarcherSystem};
use crate::levels::MapSize;
use crate::npc_conga_train::NpcCongaTrain;
use crate::skins::PlayerSkin;
use crate::sounds;
use crate::state::{
    Flashlight, GameSounds, GameTextures, LassoPhase, LevelTexture, MainState, PostProcessUniform,
    TrailUniform, WeatherState,
};
use crate::upgrade::UPGRADE_FIRST_AT;
use crate::{get_levels, pick_pen_pos, pick_tide_pools};

impl MainState {
    pub fn new(ctx: &mut Context) -> GameResult<MainState> {
        let width = 1280.0;
        let height = 960.0;
        // The opening campaign level uses the medium map; individual levels and tutorials replace
        // these bounds when they begin.
        let world_width = width * MapSize::Medium.viewport_multiplier();
        let world_height = height * MapSize::Medium.viewport_multiplier();

        // Player starts in the center of the WORLD always.
        let player_pos = Vec2::new(
            world_width / 2.0 - PLAYER_SIZE / 2.0,
            world_height / 2.0 - PLAYER_SIZE / 2.0,
        );

        // BPM detection is kept only for the informational startup log line below. The
        // groove is NOT baked at this tempo — see `action_bpm` after the block: the music
        // must match the gameplay beat grid, whose base is the BEAT_INTERVAL constant
        // (120 BPM), not whatever tempo a (now-unused) action.ogg happens to have.
        let detected_beat_interval: f32 = {
            use std::io::Read as _;
            let mut bytes = Vec::new();
            let result = ctx.fs.open("/action.ogg")
                .and_then(|mut f| {
                    f.read_to_end(&mut bytes)
                        .map_err(|e| ggez::GameError::CustomError(e.to_string()))
                })
                .map(|_| bytes);
            match result {
                Ok(bytes) => match sounds::detect_bpm_from_ogg(&bytes) {
                    Some(interval) => {
                        println!(
                            "Detected BPM: {:.1} (interval {:.4}s)",
                            60.0 / interval,
                            interval
                        );
                        interval
                    }
                    None => {
                        println!(
                            "BPM detection failed, using default {}BPM",
                            (60.0 / BEAT_INTERVAL) as u32
                        );
                        BEAT_INTERVAL
                    }
                },
                Err(e) => {
                    println!("Could not read action.ogg for BPM detection: {e}");
                    BEAT_INTERVAL
                }
            }
        };
        // Bake the music at the gameplay grid's canonical base tempo — the BEAT_INTERVAL the
        // reset and the intensity ramp both key off (`beat_interval = BEAT_INTERVAL / tempo_mul`).
        // A groove baked at any other base drifts against the on-beat catch window from the very
        // first frame; the live tempo ramp is then followed by re-pitching the music each stage
        // (see `music_pitch` in EventHandler::update), so the loop stays locked to the clock.
        let action_bpm = 60.0 / BEAT_INTERVAL;
        // detected_beat_interval still seeds the pre-game beat clock (reset_game overrides it to
        // BEAT_INTERVAL on entry) and the log line above; it no longer drives the music tempo.
        let levels = get_levels();

        // TODO Load all sound effects.
        let (king_crab_l, king_crab_r, king_crab_soft) = sounds::synth_king_crab_spatial(ctx)?;
        let (king_crab_rumble_l, king_crab_rumble_r) =
            sounds::synth_king_crab_ambient_spatial(ctx)?;
        let (rival_steal_l, rival_steal_r) = sounds::synth_rival_steal(ctx)?;
        // One beat-locked musical motif per ambient NPC King Crab train tier (0 scout / 1 wanderer
        // / 2 elder), baked at the live tempo so its two-bar loop stays in the pocket. Driven
        // per-frame in EventHandler::update (pan by bearing, swell by distance + train length).
        let mut king_crab_motif = Vec::with_capacity(3);
        for tier in 0..3 {
            king_crab_motif.push(sounds::synth_rival_motif(
                ctx,
                action_bpm,
                sounds::ACTION_KEY_ROOT_MIDI,
                tier,
            )?);
        }
        let intro_music = {
            use std::io::Read as _;
            let mut bytes = Vec::new();
            ctx.fs.open("/intro.ogg")?.read_to_end(&mut bytes)?;
            sounds::synth_intro_menu(ctx, &bytes)?
        };
        let sounds = GameSounds {
            intro_music,
            // One authored procedural loop per biome. They share the gameplay grid but vary their
            // harmony, lead timbre, and arrangement as the map changes.
            action_music: levels
                .iter()
                .map(|level| sounds::synth_biome_action_groove(ctx, action_bpm, level.biome.music))
                .collect::<GameResult<Vec<_>>>()?,
            outro_music: Source::new(ctx, "/outro.ogg")?,
            upgrade: Source::new(ctx, "/upgrade.ogg")?,
            success: Source::new(ctx, "/success.ogg")?,
            success2: Source::new(ctx, "/success2.ogg")?,
            king_crab_rumble_l,
            king_crab_rumble_r,
            hihat: sounds::synth_hihat(ctx)?,
            flashlight_toggle: sounds::synth_flashlight_toggle(ctx)?,
            startup_pling: sounds::synth_startup_pling(ctx)?,
            coin_chime: sounds::synth_coin_chime(ctx)?,
            perfect_chime: sounds::synth_perfect_sparkle(ctx)?,
            tool_accent: sounds::synth_tool_accent(ctx)?,
            world_map_pad: sounds::synth_ambient_pad(ctx, sounds::PadPreset::WarmPad, 220.0, 2.0)?,
            whistle_sfx: sounds::synth_whistle(ctx)?,
            stomp_sfx: sounds::synth_stomp(ctx)?,
            lasso_sfx: sounds::synth_lasso_throw(ctx)?,
            steal_loss_sfx: sounds::synth_steal_loss(ctx)?,
            steal_gain_sfx: sounds::synth_steal_gain(ctx)?,
            rival_steal_l,
            rival_steal_r,
            crab_themes: [
                sounds::synth_theme_duck_bounce(ctx)?,  // 0 — normal/fast/big
                sounds::synth_theme_duck_funky(ctx)?,   // 1 — dancer/splitter
                sounds::synth_theme_deus_tense(ctx)?,   // 2 — thief/sneaky
                sounds::synth_theme_deus_ambient(ctx)?, // 3 — boss/armored/hermit
                sounds::synth_theme_duck_golden(ctx)?,  // 4 — golden/magnet
            ],
            king_crab_l,
            king_crab_r,
            king_crab_soft,
            king_crab_motif,
        };

        // Synthesise the on-beat kick drum at startup so a bad WAV header fails loudly here rather
        // than as silence on the first beat.
        let beat_synth = sounds::BeatSynth::new(ctx)?;

        // Load both grass and sand textures.
        let textures = GameTextures {
            grass: Image::from_path(ctx, "/grass.png")?,
            sand: Image::from_path(ctx, "/sand.png")?,
            player: Image::from_path(ctx, "/rustler.png")?,
        };

        // Delivery pen + tide-pool hazards for the opening level, placed before `levels` is moved
        // into the struct so we can read the first zone's difficulty for the pool count.
        let init_pen = pick_pen_pos(
            world_width,
            world_height,
            player_pos + Vec2::splat(PLAYER_SIZE / 2.0),
            &mut crate::rng::rng(),
        );
        let init_tide_pools = pick_tide_pools(
            world_width,
            world_height,
            init_pen,
            player_pos + Vec2::splat(PLAYER_SIZE / 2.0),
            levels.first().map(|l| l.difficulty).unwrap_or(0),
            &mut crate::rng::rng(),
        );

        // Randomly select a texture for each level
        let mut rng = crate::rng::rng();
        let level_textures: Vec<LevelTexture> = (0..levels.len())
            .map(|_| {
                if rng.random_range(0..2) == 0 {
                    LevelTexture::Grass
                } else {
                    LevelTexture::Sand
                }
            })
            .collect();

        // Load best time from file.
        let best_time = fs::read_to_string("best_time.txt")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(f32::MAX);

        // Load the persistent career (meta-progression). Missing/garbled file just starts a
        // fresh career at zero — the game must never fail to launch over a save file.
        // Format: best total runs [spent beam lasso whistle stomp]. The trailing spend-side
        // fields were added later, so an old three-number save still parses — the extras just
        // default to 0 (no perks purchased yet). Starting ranks are clamped to their cap on load
        // so a hand-edited or future save can never over-buy a run.
        let career_text = fs::read_to_string("career.txt").ok();
        let (
            career_best_score,
            career_total_score,
            career_runs,
            career_spent,
            start_beam_rank,
            start_lasso_rank,
            start_whistle_rank,
            start_stomp_rank,
        ) = career_text
            .as_deref()
            .and_then(|s| {
                let mut it = s.split_whitespace().take(8);
                let best = it.next()?.parse::<usize>().ok()?;
                let total = it.next()?.parse::<usize>().ok()?;
                let runs = it.next()?.parse::<usize>().ok()?;
                let spent = it.next().and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
                let clamp_rank = |v: Option<&str>| {
                    v.and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(0)
                        .min(MAX_START_RANK)
                };
                let beam = clamp_rank(it.next());
                let lasso = clamp_rank(it.next());
                let whistle = clamp_rank(it.next());
                let stomp = clamp_rank(it.next());
                Some((best, total, runs, spent, beam, lasso, whistle, stomp))
            })
            .unwrap_or((0, 0, 0, 0, 0, 0, 0, 0));

        // Load the saved cosmetic loadout — stored as its own `skin ...` line in career.txt.
        let player_skin = career_text
            .as_deref()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.trim_start().starts_with("skin "))
                    .map(|l| PlayerSkin::from_save_line(l.trim()))
            })
            .unwrap_or_else(PlayerSkin::default_skin);

        let player_name = career_text
            .as_deref()
            .and_then(|s| {
                s.lines().find_map(|line| {
                    line.trim_start()
                        .strip_prefix("name ")
                        .map(crate::normalize_player_name)
                })
            })
            .unwrap_or_else(|| "Crabby".to_string());

        let crabs: Vec<EnemyCrab> = [].to_vec();

        // Pre-fill position history with initial player position
        let mut position_history: VecDeque<Vec2> = VecDeque::new();
        for _ in 0..2000 {
            position_history.push_back(player_pos);
        }

        // Try to load optional music layers (graceful — game works without them)
        // Place layer1.ogg, layer2.ogg, layer3.ogg in resources/ for layered crab rave
        let mut music_layers: Vec<Source> = Vec::new();
        for i in 1..=3usize {
            if let Ok(mut src) = Source::new(ctx, &format!("/layer{}.ogg", i)) {
                src.set_repeat(true);
                src.set_volume(0.0);
                music_layers.push(src);
            }
        }

        let shader = ShaderBuilder::new()
            .vertex_path("/grass.wgsl")
            .fragment_path("/grass.wgsl")
            .build(&ctx.gfx)?;

        let flashlight_shader = ShaderBuilder::new()
            .vertex_path("/flashlight.wgsl")
            .fragment_path("/flashlight.wgsl")
            .build(&ctx.gfx)?;

        // Offscreen target for the flashlight cone shader — kept separate so the custom shader's
        // group-3 bind never touches the scene canvas (ggez 0.9.3 set_default_shader doesn't clear
        // shader_bind_group, which would poison every subsequent instanced draw on the same canvas).
        let flashlight_cone_image = ggez::graphics::Image::new_canvas_image(
            ctx,
            width as u32,
            height as u32,
            1,
        );

        // Use logical size (1280x960) for the offscreen render target, consistent with the viewport.
        // The postprocess pass will handle any HiDPI scaling when blitting to screen.
        let scene_image = ggez::graphics::Image::new_canvas_image(
            ctx,
            width as u32,
            height as u32,
            1,
        );
        let postprocess_shader = ShaderBuilder::new()
            .vertex_path("/postprocess.wgsl")
            .fragment_path("/postprocess.wgsl")
            .build(&ctx.gfx)?;
        let initial_pp_uniform = PostProcessUniform {
            groove: 0.0,
            time: 0.0,
            screen_width: width,
            screen_height: height,
            title_card_t: 0.0,
            menu_bloom: 0.0,
            menu_moon_x: 0.82,
            menu_moon_y: 0.2,
        };
        let postprocess_params = ShaderParamsBuilder::new(&initial_pp_uniform).build(ctx);

        // Conga trail / echo-afterimage layer: a ping-pong pair of accumulation targets, same
        // logical size as the scene, allocated once here and reused every frame (no per-frame
        // image allocation). The trail shader lays the faded bright residue of the previous
        // frame back over the crisp scene, groove-scaled.
        let trail_shader = ShaderBuilder::new()
            .vertex_path("/trail.wgsl")
            .fragment_path("/trail.wgsl")
            .build(&ctx.gfx)?;
        let initial_trail_uniform = TrailUniform { strength: 0.0 };
        let trail_params = ShaderParamsBuilder::new(&initial_trail_uniform).build(ctx);
        let trail_image_a = ggez::graphics::Image::new_canvas_image(
            ctx,
            width as u32,
            height as u32,
            1,
        );
        let trail_image_b = ggez::graphics::Image::new_canvas_image(
            ctx,
            width as u32,
            height as u32,
            1,
        );

        let flashlight = Flashlight {
            on: false,
            cone_upgrade: 0.0,
            range_upgrade: 0.0,
            laser_level: 0,
            charge: 1.0,
            aim_dir: ggez::glam::Vec2::new(1.0, 0.0),
        };

        // Select a random subtitle for instructions screen
        let candidate_subtitles = [
            "Even the smallest claw can make big waves when we dance together.",
            "A lone crab scuttles, but many crabs make a rave.",
            "When crabs move as one, the ocean listens.",
            "Shiny lights bring crabs together, but the beat keeps them close.",
            "Follow your light, and you’ll find your clawsome crew.",
            "No crab too small, no groove too deep.",
            "One claw can’t clap, but two can drop the beat.",
            "The tide brings change, but crabs rave on.",
            "It takes many shells to build a real party.",
            "Crabs that groove together, grow together.",
        ];
        let subtitle = candidate_subtitles
            .choose(&mut crate::rng::rng())
            .unwrap()
            .to_string();

        Ok(MainState {
            player_pos,
            player_vel: Vec2::ZERO,
            mouse_pos: Vec2::ZERO,
            crabs,
            score: 0,
            spawn_timer: 0.0,
            treasure_chest: None,
            treasure_chest_timer: TREASURE_CHEST_ROLL_INTERVAL,
            time_elapsed: 0.0,
            menu_time: 0.0,
            menu_intro_time: 0.0,
            menu_intro_complete: false,
            menu_intro_pling_played: false,
            game_over: false,
            sounds,
            beat_synth,
            flashlight,
            show_instructions: true,
            show_how_to_play_text: false,
            show_play_recommendation: false,
            play_recommendation_continue_selected: true,
            player_skin,
            player_name,
            skin_slot: 0,
            menu_page: 0,
            menu_selection: 0,
            world_map: None,
            show_world_map: false,
            in_campaign: false,
            banked_crabs_run: 0,
            shells_cracked_run: 0,
            hold_train_timer: 0.0,
            level_complete: false,
            level_complete_timer: 0.0,
            tutorial: None,
            last_dir: Vec2::ZERO,
            shake_timer: 0.0,
            time_since_catch: 0.0,
            boost_timer: 0.0,
            boost_cooldown: 0.0,
            sprint_stamina: SPRINT_STAMINA_MAX,
            levels,
            current_level: 0,
            arcade_stage: 1,
            current_pattern: 0,
            pattern_timer: 0.0,
            debug_mode: true,
            pending_upgrade: false,
            offered_upgrades: [0, 1, 2],
            king_crab_count: 0,
            conga_tint: [0.0, 0.0, 0.0],
            speed_mult: 1.0,
            next_upgrade_score: UPGRADE_FIRST_AT,
            best_time,
            career_best_score,
            career_total_score,
            career_runs,
            career_spent,
            start_beam_rank,
            start_lasso_rank,
            start_whistle_rank,
            start_stomp_rank,
            shop_flash: 0.0,
            jam_timer: 0.0,
            shop_denied: 0.0,
            run_recorded: false,
            run_is_new_best: false,
            width,
            height,
            world_width,
            world_height,
            camera_origin: Vec2::ZERO,
            shader,
            flashlight_shader,
            flashlight_cone_image,
            scene_image,
            postprocess_shader,
            postprocess_params,
            trail_shader,
            trail_params,
            trail_image_a,
            trail_image_b,
            trail_swap: false,
            particle_system: ParticleSystem::new(),
            level_title: String::new(),
            level_title_timer: 0.0,
            textures,
            level_textures,
            subtitle,
            position_history,
            chain_count: 0,
            total_caught: 0,
            chord_tools_fired: 0,
            crabs_stolen_by_npc: 0,
            max_single_steal_by_npc: 0,
            crabs_stolen_by_player: 0,
            steals_parried: 0,
            steals_dodged: 0,
            revenge_steals: 0,
            rival_vs_rival_steals: 0,
            rival_spill_crabs: 0,
            rival_hunt_telegraphs: 0,
            hunt_intercepts: 0,
            rivals_wave_shoved: 0,
            on_beat_tool_sfx: false,
            steal_loss_sfx: false,
            steal_gain_sfx: false,
            rival_steal_sfx: None,
            beat_timer: detected_beat_interval,
            beat_interval: detected_beat_interval,
            beat_intensity: 0.0,
            music_intensity: 0.0,
            music_pitch: 1.0,
            on_beat_flash: 0.0,
            beat_gamble_mult: 1.0,
            beat_gamble_flash: 0.0,
            streak_lost_flash: 0.0,
            beat_gamble_locked: 1.0,
            gamble_bank_flash: 0.0,
            gamble_bank_pulse: 0.0,
            groove_was_full: false,
            groove_full_flash: 0.0,
            music_muted: false,
            groove: 0.0,
            beat_streak: 0,
            perfect_streak: 0,
            perfect_flash: 0.0,
            rhythm_bonus_score: 0,
            rhythm_bonus_flash: 0.0,
            music_layers,
            catch_radius_upgrade: 0.0,
            beat_catch_bloom: 0.0,
            cleave_flash: 0.0,
            cleave_a: Vec2::ZERO,
            cleave_b: Vec2::ZERO,
            cleave_gold: false,
            // Runs begin at the permanent starting ranks bought with banked crabs (the spend side
            // of meta-progression), not flat zero.
            beam_rank: start_beam_rank,
            lasso_rank: start_lasso_rank,
            whistle_rank: start_whistle_rank,
            stomp_rank: start_stomp_rank,
            floating_texts: FloatingTextSystem::new(),
            penned_marchers: PennedMarcherSystem::new(),
            marcher_arrivals_buf: Vec::new(),
            combo_count: 0,
            combo_timer: 0.0,
            beat_count: 0,
            hat_last_step: -1,
            bar_accent: 0.0,
            drum_roll_held: false,
            drum_roll_hits: 0,
            drum_roll_charge: 0.0,
            drum_roll_fire: 0.0,
            drum_roll_power: 0,
            beat_wave_active: false,
            beat_wave_radius: 0.0,
            wave_armed: false,
            wave_telegraph: 0.0,
            waves_cleared: 0,
            frenzy_wave: false,
            frenzy_banner_timer: 0.0,
            intensity_stage: 0,
            stage_banner_timer: 0.0,
            stage_banner_name: "",
            lasso_phase: LassoPhase::Idle,
            lasso_pos: None,
            lasso_timer: 0.0,
            lasso_target: Vec2::ZERO,
            lasso_origin: Vec2::ZERO,
            lasso_charge: 0.0,
            lasso_mouse_down: false,
            lasso_spin: 0.0,
            lasso_on_beat_bonus: 1.0,
            lasso_drag_buf: Vec::new(),
            whistle_active: 0.0,
            whistle_radius: 0.0,
            whistle_cooldown: 0.0,
            whistle_center: Vec2::ZERO,
            whistle_beat_bonus: 1.0,
            stomp_active: 0.0,
            stomp_radius: 0.0,
            stomp_cooldown: 0.0,
            stomp_center: Vec2::ZERO,
            stomp_beat_bonus: 1.0,
            call_cooldown: 0.0,
            cycle_cooldown: 0.0,
            call_pulse: 0.0,
            call_pulse_center: Vec2::ZERO,
            groove_call_cooldown: 0.0,
            groove_call_bars: 0.0,
            groove_call_strength: 0.0,
            groove_call_pulse: 0.0,
            groove_call_center: Vec2::ZERO,
            groove_call_surge: 0.0,
            groove_call_echo: 0,
            groove_call_echo_flash: 0.0,
            slam_active: 0.0,
            slam_radius: 0.0,
            slam_center: Vec2::ZERO,
            slam_flash: 0.0,
            dash_just_fired: false,
            dash_flash: 0.0,
            groove_dash_timer: 0.0,
            groove_dash_center: Vec2::ZERO,
            groove_dash_dir: Vec2::ZERO,
            downbeat_pull: 0.0,
            downbeat_pull_center: Vec2::ZERO,
            downbeat_pull_haul: 0.0,
            weather_target: WeatherState::Sunny,
            weather_intensity: 0.0,
            weather_step_timer: 18.0,
            lightning_flash: 0.0,
            lightning_timer: 4.0,
            day_phase_t: 0.0,
            screen_shake: 0.0,
            screen_shake_vel: Vec2::ZERO,
            screen_shake_offset: Vec2::ZERO,
            hitstop_timer: 0.0,
            slowmo_timer: 0.0,
            boss_hit_iframes: 0.0,
            chain_join_ripple: false,
            chain_snap_cooldown: 0.0,
            cached_tail_pos: None,
            cached_steal_target_pos: None,
            cached_tail_type: None,
            cycle_preview_active: false,
            free_splitter_present: false,
            tail_run_len: 0,
            next_milestone: 5,
            next_boss_score: BOSS_SCORE_INTERVAL,
            next_boss_kind: 0,
            reef_phrase: [false; 4],
            reef_phrase_bar: u32::MAX,
            reef_active: false,
            reef_dancer_timer: 0.0,
            reef_hit_flash: 0.0,
            pen_pos: init_pen,
            deliver_flash: 0.0,
            deliver_beam_from: Vec2::ZERO,
            deliver_beam_to: Vec2::ZERO,
            deliver_beam_perfect: false,
            deliver_streak: 0,
            deliver_streak_timer: 0.0,
            kelp_snag_warn: 0.0,
            tide_pools: init_tide_pools,
            rock_tide_fill: 0.0,
            in_tide_pool: false,
            boss_fissures: Vec::new(),
            boss_fissure_erupt: 0.0,
            boss_flood_pools: 0,
            chain_rings: Vec::new(),
            catch_shockwaves: Vec::new(),
            beat_punch_events: Vec::new(),
            bond_flash_events: Vec::new(),
            catch_trails: Vec::new(),
            call_streaks: Vec::new(),
            fear_rings: Vec::new(),
            tide_pulses: Vec::new(),
            zoom_punch: 0.0,
            fullscreen_applied: false,
            chain_positions_buf: Vec::new(),
            catch_grid_buf: std::collections::HashMap::new(),
            catch_grid_keys_buf: Vec::new(),
            caught_now_buf: Vec::new(),
            deflect_body_buf: Vec::new(),
            deflect_grid_buf: std::collections::HashMap::new(),
            deflect_bounce_buf: Vec::new(),
            deflect_ricochet_buf: Vec::new(),
            deflect_ricochet_grid_buf: std::collections::HashMap::new(),
            deflect_collide_buf: Vec::new(),
            deflect_resolve_buf: Vec::new(),
            flee_pops_buf: Vec::new(),
            startled_pops_buf: Vec::new(),
            golden_snare_pops_buf: Vec::new(),
            thief_snare_pops_buf: Vec::new(),
            magnet_lure_pops_buf: Vec::new(),
            thief_lure_pops_buf: Vec::new(),
            boss_broke_buf: Vec::new(),
            armor_broke_buf: Vec::new(),
            attraction_particles_buf: Vec::new(),
            boss_windups_buf: Vec::new(),
            boss_launches_buf: Vec::new(),
            boss_charge_dust_buf: Vec::new(),
            boss_enrages_buf: Vec::new(),
            tide_fires_buf: Vec::new(),
            tide_swells_buf: Vec::new(),
            magnet_positions_buf: Vec::new(),
            golden_lure_positions_buf: Vec::new(),
            charged_magnet_positions_buf: Vec::new(),
            magnet_grind_buf: Vec::new(),
            armored_positions_buf: Vec::new(),
            boss_blocks_buf: Vec::new(),
            boss_stuns_buf: Vec::new(),
            golden_panic_positions_buf: Vec::new(),
            pried_by_magnet_buf: Vec::new(),
            spooked_by_golden_buf: Vec::new(),
            lured_by_golden_buf: Vec::new(),
            dancer_hop_scratch: Vec::new(),
            contagion_carriers_buf: Vec::new(),
            contagion_grid_buf: std::collections::HashMap::new(),
            contagion_pops_buf: Vec::new(),
            armored_anchors_buf: Vec::new(),
            armored_anchor_grid_buf: std::collections::HashMap::new(),
            dancer_startle_grid_buf: std::collections::HashMap::new(),
            dancer_spooked_buf: Vec::new(),
            dancer_jolt_buf: Vec::new(),
            dancer_trip_buf: Vec::new(),
            dancer_chip_buf: Vec::new(),
            dancer_kick_buf: Vec::new(),
            dancer_link_buf: Vec::new(),
            dancer_aura_caught_buf: Vec::new(),
            whistle_soothed_buf: Vec::new(),
            beam_hermit_hits_buf: Vec::new(),
            beam_fast_hits_buf: Vec::new(),
            beam_golden_hits_buf: Vec::new(),
            beam_sneaky_hits_buf: Vec::new(),
            stomp_dancer_hits_buf: Vec::new(),
            stomp_armored_hits_buf: Vec::new(),
            whistle_golden_hits_buf: Vec::new(),
            whistle_dancer_hits_buf: Vec::new(),
            whistle_sneaky_hits_buf: Vec::new(),
            whistle_thief_hits_buf: Vec::new(),
            lasso_thief_hits_buf: Vec::new(),
            lasso_magnet_hits_buf: Vec::new(),
            lasso_big_hits_buf: Vec::new(),
            lasso_shell_deflect_hits_buf: Vec::new(),
            whistle_shell_deflect_hits_buf: Vec::new(),
            magnet_cluster_hits_buf: Vec::new(),
            magnet_cluster_counts_buf: Vec::new(),
            stomp_cracked_buf: Vec::new(),
            hermit_popped_buf: Vec::new(),
            lasso_catch_buf: Vec::new(),
            lasso_startle_buf: Vec::new(),
            whistle_thief_snatch_buf: Vec::new(),
            stomp_thief_snatch_buf: Vec::new(),
            startle_origins_buf: Vec::new(),
            boss_catches_buf: Vec::new(),
            dance_catches_buf: Vec::new(),
            golden_catches_buf: Vec::new(),
            magnet_shine_catches_buf: Vec::new(),
            match_run_catches_buf: Vec::new(),
            hype_dancer_hits_buf: Vec::new(),
            pulse_slingshots_buf: Vec::new(),
            pulse_loaded_magnets_buf: Vec::new(),
            pulse_anchor_positions_buf: Vec::new(),
            pulse_scattered_buf: Vec::new(),
            pulse_snapped_positions_buf: Vec::new(),
            king_stolen_crabs: Vec::new(),
            king_splice_cooldown: 0.0,
            player_steal_cooldown: 0.0,
            npc_trains: vec![
                NpcCongaTrain::new_at(world_width, world_height, 0),
                NpcCongaTrain::new_at(world_width, world_height, 1),
                NpcCongaTrain::new_at(world_width, world_height, 2),
            ],
            #[cfg(debug_assertions)]
            perf_frame_count: 0,
            #[cfg(debug_assertions)]
            perf_time_accum: 0.0,
            #[cfg(debug_assertions)]
            perf_worst_frame: 0.0,
            #[cfg(debug_assertions)]
            perf_last_avg_ms: 0.0,
            #[cfg(debug_assertions)]
            perf_last_worst_ms: 0.0,
            #[cfg(debug_assertions)]
            perf_last_fps: 0.0,
            bot: None,
            time_scale: 1.0,
            bot_fixed_dt: None,
        })
    }
}
