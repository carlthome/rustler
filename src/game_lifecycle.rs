//! Game lifecycle & scene transitions for `MainState`.
//!
//! Extracted verbatim from `main.rs` to keep that file focused on the app entry
//! point, the `EventHandler` impl, and the small terrain/weather helpers. This
//! module owns the run-reset path (`reset_game*`, `resize_world`) and the
//! menu/world-map/campaign/tutorial transitions. Pure structural move: behaviour
//! is unchanged.

use ggez::audio::SoundSource;
use ggez::glam::Vec2;
use ggez::Context;
use rand::Rng;

use crate::constants::*;
use crate::levels::MapSize;
use crate::npc_conga_train::NpcCongaTrain;
use crate::spawnings::spawn_tutorial_crabs;
use crate::state::{LassoPhase, MainState, WeatherState};
use crate::tutorial::{Tutorial, TutorialKind};
use crate::upgrade::UPGRADE_FIRST_AT;
use crate::world_map::WorldMap;
use crate::{pick_pen_pos, pick_tide_pools};

impl MainState {
    pub(crate) fn reset_game(&mut self) {
        // `get_levels()` always supplies campaign content, but retain this guard so a malformed
        // future level source cannot cause a reset-path panic.
        if let Some(level) = self.levels.first() {
            self.reset_game_at(0, level.map_size);
        }
    }

    fn reset_game_at_level(&mut self, requested_level: usize) {
        if !self.levels.is_empty() {
            let level_index = requested_level.min(self.levels.len() - 1);
            self.reset_game_at(level_index, self.levels[level_index].map_size);
        }
    }

    pub(crate) fn resize_world(&mut self, map_size: MapSize) {
        let new_width = self.width * map_size.viewport_multiplier();
        let new_height = self.height * map_size.viewport_multiplier();
        let shift = Vec2::new(
            (new_width - self.world_width) * 0.5,
            (new_height - self.world_height) * 0.5,
        );
        if shift == Vec2::ZERO {
            return;
        }

        // Keep an ongoing train and the nearby NPC ecology centered when a new campaign zone opens;
        // the newly exposed space is then populated by the next waves across the larger bounds.
        self.player_pos += shift;
        for pos in &mut self.position_history {
            *pos += shift;
        }
        for crab in &mut self.crabs {
            crab.pos += shift;
        }
        for train in &mut self.npc_trains {
            train.leader_pos += shift;
            train.target += shift;
            train.territory_center += shift;
            for pos in &mut train.path_history {
                *pos += shift;
            }
        }
        self.world_width = new_width;
        self.world_height = new_height;
    }

    fn reset_game_at(&mut self, level_index: usize, map_size: MapSize) {
        self.current_level = level_index;
        self.level_title = self
            .levels
            .get(level_index)
            .map(|level| format!("Stage {} — {}", level_index + 1, level.title))
            .unwrap_or_default();
        // Both modes use the same title-card language: campaign starts a fresh node, while arcade
        // starts a fresh session. Only arcade transitions between cards without resetting the run.
        self.level_title_timer = 3.1;
        self.world_width = self.width * map_size.viewport_multiplier();
        self.world_height = self.height * map_size.viewport_multiplier();
        self.npc_trains = if map_size.spawns_npc_trains() {
            (0..3)
                .map(|index| NpcCongaTrain::new_at(self.world_width, self.world_height, index))
                .collect()
        } else {
            Vec::new()
        };
        // Reset places the player at the WORLD centre (the playfield is larger than the viewport;
        // the camera follows). pen/pool placement below is world-space too.
        let width = self.world_width;
        let height = self.world_height;
        let player_pos = Vec2::new(
            width / 2.0 - PLAYER_SIZE / 2.0,
            height / 2.0 - PLAYER_SIZE / 2.0,
        );
        self.crabs = Vec::default();
        self.chain_snap_cooldown = 0.0;
        self.position_history.clear();
        let center = Vec2::new(
            width / 2.0 - PLAYER_SIZE / 2.0,
            height / 2.0 - PLAYER_SIZE / 2.0,
        );
        for _ in 0..2000 {
            self.position_history.push_back(center);
        }
        self.chain_count = 0;
        self.king_crab_count = 0;
        self.conga_tint = [0.0, 0.0, 0.0];
        self.total_caught = 0;
        self.chord_tools_fired = 0;
        self.banked_crabs_run = 0;
        self.shells_cracked_run = 0;
        self.hold_train_timer = 0.0;
        self.level_complete = false;
        self.level_complete_timer = 0.0;
        self.crabs_stolen_by_npc = 0;
        self.max_single_steal_by_npc = 0;
        self.crabs_stolen_by_player = 0;
        self.steals_parried = 0;
        self.player_steal_cooldown = 0.0;
        self.tail_run_len = 0;
        self.kelp_snag_warn = 0.0;
        self.beat_timer = BEAT_INTERVAL;
        self.beat_intensity = 0.0;
        self.music_intensity = 0.0;
        // Reset the music tempo to the base (WARM-UP) speed so a fresh run starts locked to the
        // grid. set_pitch only takes effect on the next play(), which the draw-side state machine
        // fires on game entry, so applying it here (no ctx needed) is enough.
        self.music_pitch = 1.0;
        for music in &mut self.sounds.action_music {
            music.stop();
            music.set_pitch(1.0);
        }
        for layer in self.music_layers.iter_mut() {
            layer.set_pitch(1.0);
        }
        // Motifs are beat-started by the ambient mixer. Stop a phrase left from the
        // prior run so its next start is a fresh downbeat at base tempo.
        for (left, right) in self.sounds.king_crab_motif.iter_mut() {
            left.stop();
            right.stop();
            left.set_pitch(1.0);
            right.set_pitch(1.0);
        }
        self.on_beat_flash = 0.0;
        self.groove = 0.0;
        self.slam_active = 0.0;
        self.slam_radius = 0.0;
        self.slam_flash = 0.0;
        self.beat_streak = 0;
        self.perfect_streak = 0;
        self.perfect_flash = 0.0;
        self.rhythm_bonus_score = 0;
        self.rhythm_bonus_flash = 0.0;
        self.beat_gamble_mult = 1.0;
        self.beat_gamble_flash = 0.0;
        self.streak_lost_flash = 0.0;
        self.beat_gamble_locked = 1.0;
        self.gamble_bank_flash = 0.0;
        self.gamble_bank_pulse = 0.0;
        self.deliver_streak = 0;
        self.deliver_streak_timer = 0.0;
        self.catch_radius_upgrade = 0.0;
        self.beat_catch_bloom = 0.0;
        // Seed tool ranks from the permanently-purchased starting ranks, not zero, so bought perks
        // carry into every fresh run.
        self.beam_rank = self.start_beam_rank;
        self.lasso_rank = self.start_lasso_rank;
        self.whistle_rank = self.start_whistle_rank;
        self.stomp_rank = self.start_stomp_rank;
        self.floating_texts.texts.clear();
        self.combo_count = 0;
        self.combo_timer = 0.0;
        self.beat_count = 0;
        self.hat_last_step = -1;
        self.bar_accent = 0.0;
        self.drum_roll_held = false;
        self.drum_roll_hits = 0;
        self.drum_roll_charge = 0.0;
        self.drum_roll_fire = 0.0;
        self.drum_roll_power = 0;
        self.beat_wave_active = false;
        self.beat_wave_radius = 0.0;
        self.wave_armed = false;
        self.wave_telegraph = 0.0;
        self.waves_cleared = 0;
        self.frenzy_wave = false;
        self.frenzy_banner_timer = 0.0;
        self.intensity_stage = 0;
        self.beat_interval = BEAT_INTERVAL;
        self.stage_banner_timer = 0.0;
        self.stage_banner_name = "";
        self.lasso_phase = LassoPhase::Idle;
        self.lasso_pos = None;
        self.lasso_timer = 0.0;
        self.lasso_target = Vec2::ZERO;
        self.lasso_origin = Vec2::ZERO;
        self.lasso_charge = 0.0;
        self.lasso_mouse_down = false;
        self.lasso_spin = 0.0;
        self.lasso_on_beat_bonus = 1.0;
        self.whistle_active = 0.0;
        self.whistle_radius = 0.0;
        self.whistle_cooldown = 0.0;
        self.whistle_beat_bonus = 1.0;
        self.stomp_active = 0.0;
        self.stomp_radius = 0.0;
        self.stomp_cooldown = 0.0;
        self.stomp_beat_bonus = 1.0;
        self.call_cooldown = 0.0;
        self.cycle_cooldown = 0.0;
        self.call_pulse = 0.0;
        self.groove_call_cooldown = 0.0;
        self.groove_call_bars = 0.0;
        self.groove_call_strength = 0.0;
        self.groove_call_pulse = 0.0;
        self.groove_call_surge = 0.0;
        self.groove_call_echo = 0;
        self.groove_call_echo_flash = 0.0;
        self.call_streaks.clear();
        self.dash_just_fired = false;
        self.dash_flash = 0.0;
        self.groove_dash_timer = 0.0;
        self.groove_dash_center = Vec2::ZERO;
        self.groove_dash_dir = Vec2::ZERO;
        self.downbeat_pull = 0.0;
        self.downbeat_pull_center = Vec2::ZERO;
        self.downbeat_pull_haul = 0.0;
        // Weather starts at a random light state — cloudy or sunny — and escalates from there.
        // Runs start calm (no heavy rain) but vary each time so weather isn't always invisible.
        self.weather_target = if crate::rng::rng().random_bool(0.45) {
            WeatherState::Cloudy
        } else {
            WeatherState::Sunny
        };
        self.weather_intensity = 0.0;
        self.weather_step_timer = 8.0; // first step soon so weather kicks in early
        self.lightning_flash = 0.0;
        self.lightning_timer = 4.0;
        self.day_phase_t = 0.0;
        self.screen_shake = 0.0;
        self.screen_shake_vel = Vec2::ZERO;
        self.screen_shake_offset = Vec2::ZERO;
        self.hitstop_timer = 0.0;
        self.slowmo_timer = 0.0;
        self.boss_hit_iframes = 0.0;
        self.chain_join_ripple = false;
        self.next_milestone = 5;
        self.next_boss_score = BOSS_SCORE_INTERVAL;
        self.next_boss_kind = 0;
        self.reef_phrase = [false; 4];
        self.reef_phrase_bar = u32::MAX;
        self.reef_active = false;
        self.reef_dancer_timer = 0.0;
        self.reef_hit_flash = 0.0;
        self.deliver_flash = 0.0;
        self.penned_marchers.marchers.clear();
        self.pen_pos = pick_pen_pos(
            self.world_width,
            self.world_height,
            player_pos + Vec2::splat(PLAYER_SIZE / 2.0),
            &mut crate::rng::rng(),
        );
        self.tide_pools = pick_tide_pools(
            self.world_width,
            self.world_height,
            self.pen_pos,
            player_pos + Vec2::splat(PLAYER_SIZE / 2.0),
            self.levels.first().map(|l| l.difficulty).unwrap_or(0),
            &mut crate::rng::rng(),
        );
        self.in_tide_pool = false;
        self.boss_fissures.clear();
        self.boss_fissure_erupt = 0.0;
        self.boss_flood_pools = 0;
        self.chain_rings.clear();
        self.catch_shockwaves.clear();
        self.catch_trails.clear();
        self.fear_rings.clear();
        self.tide_pulses.clear();
        self.player_pos = player_pos;
        self.score = 0;
        self.next_upgrade_score = UPGRADE_FIRST_AT;
        self.speed_mult = 1.0;
        self.spawn_timer = 0.0;
        self.treasure_chest = None;
        self.treasure_chest_timer = TREASURE_CHEST_ROLL_INTERVAL;
        self.time_elapsed = 0.0;
        self.game_over = false;
        self.run_recorded = false;
        self.run_is_new_best = false;
        self.boost_timer = 0.0;
        self.boost_cooldown = 0.0;
        self.sprint_stamina = SPRINT_STAMINA_MAX;
        self.current_pattern = 0;
        if !self.in_campaign {
            self.arcade_stage = 1;
        }
        self.start_current_pattern((width, height));
    }

    /// Open the campaign world map. Creates it on first visit; subsequent visits reuse the same
    /// instance so node completion persists across runs.
    pub(crate) fn enter_world_map(&mut self, _ctx: &mut Context) {
        if self.world_map.is_none() {
            self.world_map = Some(WorldMap::new());
        }
        self.stop_level_audio();
        self.show_instructions = false;
        self.show_how_to_play_text = false;
        self.show_world_map = true;
        self.game_over = false;
        self.in_campaign = false;
        // A calm ambient pad for the campaign map — a breather moment between levels.
        let _ = self.sounds.world_map_pad.play();
    }

    /// Stop sounds that belong to a campaign level before returning to the world map.
    fn stop_level_audio(&mut self) {
        for music in &self.sounds.action_music {
            music.pause();
        }
        self.sounds.outro_music.pause();
        for layer in &mut self.music_layers {
            layer.stop();
        }
        for theme in &mut self.sounds.crab_themes {
            theme.stop();
        }
        for source in [
            &mut self.sounds.king_crab_rumble_l,
            &mut self.sounds.king_crab_rumble_r,
            &mut self.sounds.king_crab_l,
            &mut self.sounds.king_crab_r,
            &mut self.sounds.king_crab_soft,
            &mut self.sounds.whistle_sfx,
            &mut self.sounds.stomp_sfx,
            &mut self.sounds.lasso_sfx,
            &mut self.sounds.steal_loss_sfx,
            &mut self.sounds.steal_gain_sfx,
            &mut self.sounds.rival_steal_l,
            &mut self.sounds.rival_steal_r,
            &mut self.sounds.hihat,
            &mut self.sounds.coin_chime,
        ] {
            source.stop();
        }
        for (left, right) in &mut self.sounds.king_crab_motif {
            left.stop();
            right.stop();
        }
    }

    /// Return to the title menu without terminating the application.
    pub(crate) fn return_to_main_menu(&mut self) {
        if let Some(map) = &mut self.world_map {
            map.cancel_skip();
        }
        self.stop_level_audio();
        self.sounds.world_map_pad.pause();
        self.reset_game();
        self.show_world_map = false;
        self.show_instructions = true;
        self.show_how_to_play_text = false;
        self.game_over = false;
        self.in_campaign = false;
        self.tutorial = None;
        self.menu_page = 0;
        self.sounds.intro_music.stop();
        let _ = self.sounds.intro_music.play();
    }

    /// Start a campaign run (or tutorial) from the currently selected world map node.
    /// Tutorial nodes enter a scripted sandbox; campaign nodes load a regular Level from scratch.
    /// Arcade never enters this path, so its title-card progression skips the tutorials and keeps
    /// the live train, upgrades, and escalating run state intact.
    pub(crate) fn enter_campaign_level(&mut self) {
        self.sounds.world_map_pad.pause();
        // Check if the selected node is a tutorial sandbox.
        let tutorial_kind = self
            .world_map
            .as_ref()
            .and_then(|m| m.selected_tutorial_kind());

        if let Some(kind) = tutorial_kind {
            // Tutorial nodes run the scripted sandbox instead of a normal level.
            self.enter_tutorial(kind);
            self.show_world_map = false;
            self.in_campaign = true;
            return;
        }

        let level_index = self
            .world_map
            .as_ref()
            .and_then(|m| m.selected_level_index())
            .unwrap_or(0);
        self.reset_game_at_level(level_index);
        self.show_world_map = false;
        self.in_campaign = true;
    }

    /// Called when a campaign run ends — returns to the world map screen. `won` gates progression:
    /// on a genuine win (the level's `WinCondition` was met, or a tutorial node was passed) the node
    /// is marked complete and the next one unlocks; on a loss (dismissing the game-over screen) the
    /// node stays incomplete so the win condition still gates the campaign and the player can retry
    /// it. Without this gate a loss also unlocked the next level, defeating the point of #182.
    /// Career stats are NOT updated here (that path stays in `record_run`).
    pub(crate) fn return_to_world_map(&mut self, won: bool) {
        if won {
            if let Some(map) = &mut self.world_map {
                map.complete_selected();
            }
        }
        self.game_over = false;
        self.show_world_map = true;
        self.in_campaign = false;
        self.stop_level_audio();
        let _ = self.sounds.world_map_pad.play();
    }

    /// Enter a scripted "How to Play" tutorial session. Starts from a clean run state (so no
    /// leftover herd/boss), then constrains it into a tiny sandbox: leave the spawn patterns alone
    /// (the tutorial gates them off in update) and drop in just a handful of plain crabs to catch.
    /// The session runs the normal LIVE update/draw path — the beat clock and catches have to
    /// actually tick for a rhythm lesson — so we clear `show_instructions` and set `self.tutorial`
    /// instead of staying on the paused menu screen. Exit is opt-in: pressing Escape returns to the
    /// menu without ever touching `game_over`, so tutorial runs never pollute the persistent career.
    fn enter_tutorial(&mut self, kind: TutorialKind) {
        self.reset_game_at(0, MapSize::Tutorial);
        // reset_game seeded a normal first wave; wipe it and drop in the calm tutorial set instead.
        self.crabs.clear();
        self.crabs = spawn_tutorial_crabs(kind, 6, (self.width, self.height), &mut crate::rng::rng());
        // Tutorial worlds are exactly one viewport, so the player and scripted crab ring start
        // together in the centre without any camera travel.
        let tut_center = Vec2::new(
            self.width / 2.0 - PLAYER_SIZE / 2.0,
            self.height / 2.0 - PLAYER_SIZE / 2.0,
        );
        self.player_pos = tut_center;
        self.position_history.clear();
        for _ in 0..2000 {
            self.position_history.push_back(tut_center);
        }
        // Pen for the tutorial belongs near the learner too, not at a random world corner.
        self.pen_pos = pick_pen_pos(
            self.width,
            self.height,
            tut_center + Vec2::splat(PLAYER_SIZE / 2.0),
            &mut crate::rng::rng(),
        );
        // Stomp is gated only by its cooldown (not by rank), so a rank-0 career can still Stomp in
        // the ShellCrack lesson — clear the cooldown so the very first press lands immediately.
        self.stomp_cooldown = 0.0;
        // A tutorial isn't a scored run — keep bosses far away and never advance the level.
        self.next_boss_score = usize::MAX;
        self.wave_armed = false;
        self.wave_telegraph = 0.0;
        self.show_instructions = false;
        self.show_how_to_play_text = false;
        self.game_over = false;
        self.tutorial = Some(Tutorial::new(kind));
    }
}
