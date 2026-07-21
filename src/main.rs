mod audio_mix;
mod beat;
mod bot;
mod catch_deliver;
mod catch_effects;
mod chain_mechanics;
mod constants;
mod controls;
mod crab_update;
mod enemies;
mod floating_text;
mod game_render;
mod graphics;
mod hud_cache;
mod king_crab_audio;
mod levels;
mod menu;
mod npc_trains;
mod overlays;
mod player_tools;
mod skins;
mod sounds;
mod spawnings;
mod startle;
mod state;
mod tutorial;
mod upgrade;
mod world_map;

pub use constants::*;
pub use hud_cache::*;
pub use state::*;

use std::{cell::RefCell, env, fs, path};

// Scratch buffer for count_chain_bonds — reused across calls to avoid a per-call heap alloc
// every frame. The Vec is grown-but-not-shrunk, so it reaches steady state after the first
// run at max chain length and never allocates again during normal gameplay.
thread_local! {
    static BOND_INDEX_BUF: RefCell<Vec<Option<CrabType>>> = RefCell::new(Vec::new());
    // Scratch buffer for centerpiece_link_indices — reused every draw frame so the per-frame
    // Vec<usize> allocation that was fired inside draw_crabs_with_shake is eliminated. Same
    // grown-but-not-shrunk pattern: reaches steady state at max train length and stays there.
    static CENTERPIECE_OUT_BUF: RefCell<Vec<usize>> = RefCell::new(Vec::new());
}

pub(crate) fn normalize_player_name(name: &str) -> String {
    let cleaned = sanitize_player_name(name);
    if cleaned.is_empty() {
        "Crabby".to_string()
    } else {
        cleaned
    }
}

pub(crate) fn sanitize_player_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|ch| !ch.is_control())
        .take(24)
        .collect();
    cleaned.trim().to_string()
}

/// Returns the instructions shown on the "How to Play" menu card.
pub(crate) fn how_to_play_body_text() -> String {
    [
        "1. Move with WASD or arrow keys (hold Shift to sprint).",
        "2. Keep crabs inside your flashlight beam.",
        "3. Catch crabs on the beat for better rewards.",
        "4. Bring caught crabs to the pen to bank points.",
        "5. Avoid losing your train before banking.",
        "",
        "Controls:",
        "- Left click hold/release: lasso",
        "- Space: dash",
        "- Q: wave",
        "- E: whistle",
        "- R: stomp",
        "- F: call",
        "- X: cycle",
        "- V: groove call",
        "- G: downbeat slam",
        "- B: bank (+ jam)",
        "",
        "Press Enter, Space, or Esc to go back.",
    ]
    .join("\n")
}

use ggez::audio::SoundSource;
use ggez::conf::{FullscreenType, WindowMode};
use ggez::event::{self, EventHandler};
use ggez::glam::Vec2;
use ggez::graphics::{BlendMode, Canvas, Color, DrawParam, Sampler};
use ggez::input::keyboard::{KeyCode, KeyInput};
use ggez::input::mouse::MouseButton;
use ggez::{Context, ContextBuilder, GameResult};
use rand::Rng;

use crate::controls::{handle_key_down_event, handle_player_movement};
use crate::enemies::{BossCharge, CrabType, EnemyCrab};
use crate::levels::{TerrainKind, get_levels};
use crate::spawnings::{
    spawn_boss, spawn_enemies, spawn_hype_dancer, spawn_rhythm_boss,
    spawn_tide_boss, spawn_tutorial_crabs,
};
use crate::tutorial::{Tutorial, TutorialKind};
use crate::upgrade::UPGRADE_FIRST_AT;
use crate::world_map::WorldMap;

/// How many tail links a *panic* snap tears loose for a train of length `n`.
///
/// The second half of the bank-now-vs-push-luck tension: a snap on a short train nibbles a few
/// links, but a long unbanked train bleeds MORE per hit — the downside scales with the length
/// you refuse to bank, so holding long is actively (not just abstractly) dangerous. Because
/// `pen_worth` is triangular the tail links are the priciest, so tearing more of them off a long
/// train makes the points-lost climb superlinearly on its own — no separate punishment curve
/// needed. Stepped rather than continuous so the ramp reads as clear tiers (3 → 4 → 5 → 6).
/// The head is never in scope here (callers clamp `keep` to >= 1), so even a big hit leaves a
/// long train alive to route away and bank.
///
/// This is the single source of truth for panic-snap severity: `snap_chain_on_panic` uses it to
/// decide how many links to release, and the live "AT RISK" readout uses the SAME function to
/// compute its marginal-loss number, so the tag can never lie about what a snap costs. The other
/// snap sites (kelp-snag, tide surge, blast) have their own fixed severities and are deliberately
/// NOT routed through here — the readout mirrors the panic snap only.
pub(crate) fn panic_snap_links(n: usize) -> usize {
    match n {
        0..=7 => 3,
        8..=11 => 4,
        12..=15 => 5,
        _ => 6,
    }
}

/// Pick a fresh delivery-pen location: somewhere on the field, kept away from the edges and a
/// good stride from `avoid` (usually the player) so banking always means routing the train across
/// open ground rather than the pen landing in your lap.
pub(crate) fn pick_pen_pos(width: f32, height: f32, avoid: Vec2, rng: &mut impl rand::Rng) -> Vec2 {
    let margin = PEN_RADIUS + 60.0;
    let min_dist = 320.0;
    let mut best = Vec2::new(width * 0.5, height * 0.5);
    let mut best_dist = -1.0;
    // Guard: if world is smaller than 2*margin, fall back to centre immediately
    if width <= margin * 2.0 || height <= margin * 2.0 {
        return best;
    }
    for _ in 0..12 {
        let candidate = Vec2::new(
            rng.random_range(margin..(width - margin)),
            rng.random_range(margin..(height - margin)),
        );
        let d = candidate.distance(avoid);
        if d >= min_dist {
            return candidate;
        }
        // Fall back to the farthest candidate we saw if none clears the threshold.
        if d > best_dist {
            best_dist = d;
            best = candidate;
        }
    }
    best
}

/// Scatter a handful of tide pools across the field for the current level. Pools are kept clear of
/// the delivery pen (so banking never means wading), off the player's current spot, and apart from
/// each other, so they read as distinct hazards to route between rather than one big swamp. Count
/// scales gently with `difficulty` so later zones have more water to thread the train through.
pub(crate) fn pick_tide_pools(
    width: f32,
    height: f32,
    avoid_pen: Vec2,
    avoid_player: Vec2,
    difficulty: usize,
    rng: &mut impl rand::Rng,
) -> Vec<(Vec2, f32)> {
    let count = (2 + difficulty / 2).min(5);
    let mut pools: Vec<(Vec2, f32)> = Vec::with_capacity(count);
    let mut attempts = 0;
    while pools.len() < count && attempts < 80 {
        attempts += 1;
        let radius = rng.random_range(66.0..112.0);
        let margin = radius + 30.0;
        if width <= margin * 2.0 || height <= margin * 2.0 {
            break;
        }
        let c = Vec2::new(
            rng.random_range(margin..(width - margin)),
            rng.random_range(margin..(height - margin)),
        );
        // Never let a pool swallow the pen or land on the player, and keep pools spaced apart.
        if c.distance(avoid_pen) < radius + PEN_RADIUS + 40.0 {
            continue;
        }
        if c.distance(avoid_player) < radius + 120.0 {
            continue;
        }
        if pools
            .iter()
            .any(|(pc, pr)| c.distance(*pc) < radius + pr + 50.0)
        {
            continue;
        }
        pools.push((c, radius));
    }
    pools
}

impl MainState {
    /// The terrain wrinkle of the zone currently in play — decides what the terrain patches do
    /// (open field, wade-drag water, solid rock chokepoints, or crab-snagging kelp). Clamped so a
    /// finished run doesn't index past the last level.
    fn current_terrain(&self) -> TerrainKind {
        self.levels[self.current_level.min(self.levels.len() - 1)]
            .biome
            .terrain
    }

    /// Rocky Shore tide: is the native rock patch at `index` a *low rock* the tide can submerge?
    /// Every other patch (even index) counts as low, so at any given tide there's a mix of covered
    /// shortcuts and still-solid high rocks to thread between — the tide reshapes the route, it never
    /// clears it. Pure function of the index so both the movement resolver (controls.rs) and the draw
    /// pass (graphics.rs) classify the same patches identically without sharing extra state.
    pub fn rock_is_low(index: usize) -> bool {
        index % 2 == 0
    }

    /// Is a low rock currently under enough water to walk through? True once the smoothed tide level
    /// has risen past the submerge threshold. The one boolean behind the whole mechanic: while true,
    /// low rocks stop blocking and wade-drag instead; while false they're solid stone again.
    pub fn rock_tide_open(&self) -> bool {
        self.rock_tide_fill > ROCK_SUBMERGE_LEVEL
    }

    /// Advance the Rocky Shore tide one frame. The sea's *target* level is driven by the 4-beat bar
    /// phase — it swells to full over the first half of the bar (beats "1" and "2") and drains back
    /// over the second half (beats "3" and "4"), so the flood peaks around the bar's midpoint and the
    /// shortcut is open on a predictable, on-beat cadence you can learn and time a dash to. The
    /// smoothed `rock_tide_fill` eases toward that target so the water visibly rises and falls rather
    /// than snapping. Only ticks on the Rock biome; every other zone holds it at 0 (no low rocks to
    /// flood there anyway) so nothing else pays for it.
    fn update_rock_tide(&mut self, dt: f32) {
        if self.current_terrain() != TerrainKind::Rock {
            // Ebb any leftover level back out so re-entering a Rock zone starts fully drained.
            self.rock_tide_fill = (self.rock_tide_fill - ROCK_TIDE_EASE * dt).max(0.0);
            return;
        }
        // Continuous bar phase in [0,1): which fraction of the current 4-beat bar we're in, using the
        // live beat clock so the tide keeps pace with the difficulty-ramp tempo shifts like everything
        // else. beat_count advances on each beat; beat_timer counts down within a beat.
        let within_beat = 1.0 - (self.beat_timer / self.beat_interval).clamp(0.0, 1.0);
        let bar_phase = ((self.beat_count % 4) as f32 + within_beat) / 4.0;
        // Triangle wave over the bar: 0 at the downbeat, up to 1 at the midpoint, back to 0 — a clean
        // in-and-out swell that peaks once per bar.
        let target = 1.0 - (bar_phase * 2.0 - 1.0).abs();
        let step = ROCK_TIDE_EASE * dt;
        if self.rock_tide_fill < target {
            self.rock_tide_fill = (self.rock_tide_fill + step).min(target);
        } else {
            self.rock_tide_fill = (self.rock_tide_fill - step).max(target);
        }
    }

    // try_deliver_train, handle_crab_catching, and catch_radius now live in src/catch_deliver.rs (impl MainState there).

    /// Ambience multiplier on the catch radius — subtle, never punishing. Rain/Storm make crabs
    /// harder to spot (down to ~-13% at full Storm), night dims the beam a touch (~-6% at deep
    /// night), and a Storm lightning flash briefly floods light back in (a short catch-radius
    /// spike). All three fold into one factor so the gameplay number and the drawn ring stay in
    /// lockstep. Clamped so upgrades always dominate.
    pub(crate) fn weather_catch_mult(&self) -> f32 {
        let rain = self.weather_intensity.clamp(0.0, 1.0) * 0.13;
        let night = self.night_factor() * 0.06;
        // Lightning flash illuminates a wider area for its ~0.5s life.
        let flash = self.lightning_flash.clamp(0.0, 1.0) * 0.30;
        (1.0 - rain - night + flash).clamp(0.80, 1.35)
    }

    /// 0 in daylight, ramping to 1 at deepest night — shared by the catch-radius dim and the
    /// beat-pulse brighten so "night" reads consistently in feel and visuals.
    pub(crate) fn night_factor(&self) -> f32 {
        // day_phase_t: 0=dawn .25=day .5=dusk .75→1=night. Night ramps from dusk onward.
        ((self.day_phase_t - 0.5) / 0.5).clamp(0.0, 1.0)
    }

    /// Day/night ground+sky tint: a warm→bright→orange→deep-blue color the world is graded toward.
    /// Returned as (r,g,b) multipliers in 0..1 applied on top of the biome tint, plus an ambient
    /// brightness scalar. Kept subtle so gameplay reads clearly at every phase.
    pub(crate) fn day_tint(&self) -> (f32, f32, f32) {
        // Keyframes across day_phase_t: dawn(amber) → day(neutral bright) → dusk(orange-pink) → night(deep blue).
        // Each is an RGB multiplier centered near 1.0 so the shift is a grade, not a repaint.
        let keys = [
            (0.00, (1.06, 0.92, 0.78)), // dawn — warm amber
            (0.25, (1.00, 1.00, 1.00)), // midday — bright neutral
            (0.55, (1.08, 0.82, 0.72)), // dusk — orange-pink
            (0.80, (0.72, 0.78, 1.05)), // night — deep blue
            (1.00, (0.66, 0.74, 1.08)), // deep night
        ];
        let t = self.day_phase_t.clamp(0.0, 1.0);
        let mut lo = keys[0];
        let mut hi = keys[keys.len() - 1];
        for w in keys.windows(2) {
            if t >= w[0].0 && t <= w[1].0 {
                lo = w[0];
                hi = w[1];
                break;
            }
        }
        let span = (hi.0 - lo.0).max(1e-4);
        let f = ((t - lo.0) / span).clamp(0.0, 1.0);
        (
            lo.1.0 + (hi.1.0 - lo.1.0) * f,
            lo.1.1 + (hi.1.1 - lo.1.1) * f,
            lo.1.2 + (hi.1.2 - lo.1.2) * f,
        )
    }

    /// Advance the weather random-walk and the day/night clock. Called only from the live sim
    /// update (after the pause early-return), so a paused menu doesn't age the world.
    fn update_weather(&mut self, dt: f32) {
        let mut rng = rand::rng();
        // Day/night: one full run ≈ 8 minutes covers dawn→night. Clamped at 1 so a long run just
        // sits in night rather than wrapping back to dawn mid-run.
        const RUN_SECONDS: f32 = 480.0;
        self.day_phase_t = (self.day_phase_t + dt / RUN_SECONDS).min(1.0);

        // Ease the visible intensity toward the current target so state changes cross-fade
        // instead of snapping.
        let target = self.weather_target.intensity();
        let ease = 1.0 - (-dt * 0.6).exp();
        self.weather_intensity += (target - self.weather_intensity) * ease;

        // Random walk over the discrete states. Early in a run it tends to calm; past the midpoint
        // it tends to escalate, so a run builds from a clear sky toward rain/storm.
        self.weather_step_timer -= dt;
        if self.weather_step_timer <= 0.0 {
            // Shorter step interval and stronger escalation so rain/storms appear in playtests.
            self.weather_step_timer = rng.random_range(8.0..18.0);
            let cur = self.weather_target as i32; // Sunny=0 .. Storm=4
            // Escalation bias starts higher and grows faster — rain is common, storms happen.
            let escalate_bias = 0.55 + self.day_phase_t * 0.35; // 0.55 → 0.90
            let roll: f32 = rng.random();
            let next = if roll < escalate_bias {
                cur + 1
            } else if roll < escalate_bias + 0.20 {
                cur - 1
            } else {
                cur
            };
            self.weather_target = match next.clamp(0, 4) {
                0 => WeatherState::Sunny,
                1 => WeatherState::Cloudy,
                2 => WeatherState::Rain,
                3 => WeatherState::HeavyRain,
                _ => WeatherState::Storm,
            };
        }

        // Decay any active lightning flash (1→0 over ~0.5s).
        if self.lightning_flash > 0.0 {
            self.lightning_flash = (self.lightning_flash - dt * 2.0).max(0.0);
        }

        // Storm-only lightning: countdown to the next strike. On strike, fire ONE event that drives
        // all three responses — visual brighten (lightning_flash), thunder (screen_shake) and the
        // catch-radius spike (also via lightning_flash in weather_catch_mult) — so they stay synced.
        if self.weather_target == WeatherState::Storm && self.weather_intensity > 0.7 {
            self.lightning_timer -= dt;
            if self.lightning_timer <= 0.0 {
                self.lightning_timer = rng.random_range(3.5..9.0);
                self.lightning_flash = 1.0;
                // Thunder: a sharp kick through the existing screen-shake system.
                self.screen_shake = self.screen_shake.max(12.0);
                let a: f32 = rng.random_range(0.0..std::f32::consts::TAU);
                self.screen_shake_vel = Vec2::new(a.cos(), a.sin()) * 12.0 * 60.0;
            }
        } else {
            // Keep the timer primed so a strike can't fire the instant a storm begins.
            self.lightning_timer = self.lightning_timer.max(2.5);
        }
    }

    /// Coarse day/night label for the current `day_phase_t`. Purely for readability/debug.
    #[allow(dead_code)]
    fn day_phase(&self) -> DayPhase {
        match self.day_phase_t {
            t if t < 0.20 => DayPhase::Dawn,
            t if t < 0.45 => DayPhase::Day,
            t if t < 0.70 => DayPhase::Dusk,
            _ => DayPhase::Night,
        }
    }

    fn catch_by_chain(&mut self, ctx: &mut Context) {
        // On-beat catch bloom: the train's catch window widens on the beat (widest on the downbeat)
        // and settles back before the next hit, so crossing a drifting crab ON the beat scoops it
        // while an off-beat pass just misses. Set in the beat handler, decayed in update_crabs, drawn
        // as a pulsing ring at the head — this is the line that turns the beat into herd management.
        let catch_radius = self.catch_radius();

        self.chain_positions_buf.clear();
        self.chain_positions_buf
            .extend(self.crabs.iter().filter(|c| c.caught).map(|c| c.pos));
        if self.chain_positions_buf.is_empty() {
            return;
        }
        // Bucket uncaught crabs into a spatial grid keyed by cell so each chain link only
        // tests the handful of crabs near it instead of the whole uncaught set. Without this,
        // the scan below is O(caught * uncaught) and gets noticeably slower as the conga
        // train — and the crab count — grow.
        //
        // The grid (and its per-cell Vec<usize> buckets) live in a persistent buffer and are
        // cleared-and-refilled rather than reallocated every frame: the play area is a fixed
        // size, so distinct cell keys stabilize almost immediately and this stops rebuilding a
        // fresh HashMap plus dozens of small Vecs on every single tick.
        let cell_size = catch_radius.max(1.0);
        let cell_of = |p: Vec2| -> (i32, i32) {
            (
                (p.x / cell_size).floor() as i32,
                (p.y / cell_size).floor() as i32,
            )
        };
        // Clear only the cells touched last frame (via catch_grid_keys_buf) rather than calling
        // HashMap::clear(), which drops every inner Vec<usize> and forces a fresh heap alloc when
        // the same cell is re-inserted next frame. Crabs move slowly so they typically stay in the
        // same cells frame-to-frame; reusing the Vec allocation avoids ~40-50 small allocs/frame.
        // We still visit only live cells (not "every cell ever"), so the bounded-iteration goal
        // from the original fix is preserved — this is strictly cheaper than the HashMap::clear() path.
        for &k in &self.catch_grid_keys_buf {
            if let Some(v) = self.catch_grid_buf.get_mut(&k) {
                v.clear();
            }
        }
        self.catch_grid_keys_buf.clear();
        for (i, c) in self.crabs.iter().enumerate() {
            if c.is_catchable() {
                let k = cell_of(c.pos);
                let bucket = self.catch_grid_buf.entry(k).or_default();
                if bucket.is_empty() {
                    // Only record the key the first time we insert into this cell this frame,
                    // so catch_grid_keys_buf has one entry per cell (not per crab).
                    self.catch_grid_keys_buf.push(k);
                }
                bucket.push(i);
            }
        }
        let catch_radius_sq = catch_radius * catch_radius;
        self.caught_now_buf.clear();
        self.caught_now_buf.resize(self.crabs.len(), false);
        for &cp in &self.chain_positions_buf {
            let (cx, cy) = cell_of(cp);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(candidates) = self.catch_grid_buf.get(&(cx + dx, cy + dy)) {
                        for &i in candidates {
                            if !self.caught_now_buf[i]
                                && cp.distance_squared(self.crabs[i].pos) < catch_radius_sq
                            {
                                self.caught_now_buf[i] = true;
                            }
                        }
                    }
                }
            }
        }
        let mut rng = rand::rng();
        for i in 0..self.caught_now_buf.len() {
            if !self.caught_now_buf[i] {
                continue;
            }
            let pos = self.crabs[i].pos;
            let crab_type = self.crabs[i].crab_type;
            let crab_color = self.crabs[i].crab_color();
            self.particle_system
                .spawn_catch_effect(pos, crab_color, crab_type, &mut rng);
            self.spawn_catch_shockwave(pos, crab_color);
            let was_answering = self.crabs[i].answering_call > 0.0;
            self.crabs[i].caught = true;
            if self.crabs[i].is_boss() {
                self.on_boss_caught(pos, self.crabs[i].is_tide_boss());
            }
            if self.crabs[i].is_golden() {
                self.on_golden_caught(pos, 0);
            }
            self.reward_dance_catch(was_answering, pos);
            self.emit_catch_startle(pos);
            self.chain_join_ripple = true;
            self.crabs[i].chain_index = Some(self.chain_count);
            self.chain_count += 1;
            self.check_milestone(&mut rand::rng());
            let pos = self.crabs[i].pos;
            self.register_catch(pos, 0);
            self.shake_timer = 0.15;
            self.hitstop_timer = self.hitstop_timer.max(0.04);
            self.zoom_punch = self.zoom_punch.max(0.03);
            self.time_since_catch = 0.0;
            play_catch_sound(&mut self.sounds, ctx, &mut rng, self.beat_streak);
            self.check_upgrade_unlock(ctx);
        }
    }

    fn start_current_pattern(&mut self, area: (f32, f32)) {
        let mut rng = rand::rng();
        if self.current_level >= self.levels.len() {
            // No levels left, finish game.
            self.game_over = true;
            return;
        }
        let level = &self.levels[self.current_level];
        let p = &level.patterns[self.current_pattern];
        // Frenzy waves drop a denser herd than the pattern normally calls for — the staged spike.
        // ~1.7x the count (min +4) so it reads as a real surge, and give a touch less time to
        // clear it so the pressure is felt. `frenzy_wave` was set during arming and is consumed
        // here (the flag is what the gold telegraph read); reset it once the drop is spent.
        // Staged ramp: denser herds and less breathing room the further into the run we are. This
        // is the smooth rising spine; the Frenzy bump below stacks on top of it for the periodic
        // standout spike. `stage` is clamped in-bounds since intensity_stage only climbs.
        let stage = self.intensity_stage.min(INTENSITY_STAGES.len() - 1);
        let stage_mul = INTENSITY_STAGES[stage].2;
        let stage_dur = STAGE_DURATION_SCALE
            .powi(stage as i32)
            .max(STAGE_DURATION_FLOOR);
        let base_count = (p.count as f32 * stage_mul).round() as usize;
        let frenzy = self.frenzy_wave;
        let count = if frenzy {
            ((base_count as f32 * 1.7).ceil() as usize).max(base_count + 4)
        } else {
            base_count
        };
        let base_duration = p.duration * stage_dur;
        let duration = if frenzy {
            base_duration * 0.85
        } else {
            base_duration
        };
        let crabs = spawn_enemies(
            p.pattern.clone(),
            count,
            area,
            p.centroid,
            level.emphasis,
            &mut rng,
        );
        self.crabs.extend(crabs);
        self.pattern_timer = duration;
        self.frenzy_wave = false;
    }

    fn advance_pattern(&mut self) {
        // Count every wave the player clears this run — drives the every-4th Frenzy cadence.
        self.waves_cleared = self.waves_cleared.wrapping_add(1);
        self.current_pattern += 1;
        let level = &self.levels[self.current_level];
        if self.current_pattern >= level.patterns.len() {
            self.current_level += 1;
            self.current_pattern = 0;
            // Name the level we just *entered* (biome + emphasis threat on the card also read from
            // current_level), not the one we left — otherwise the title says one zone while the
            // biome subtitle and threat banner name the next, an internally-mismatched card.
            self.level_title = self
                .levels
                .get(self.current_level)
                .map(|l| l.title.clone())
                .unwrap_or_else(|| level.title.clone());
            self.level_title_timer = 3.1; // 0.3s fade-in + 2.2s hold + 0.6s fade-out
            // Fresh biome, fresh pen location — keep routing the train there a live decision.
            let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            self.pen_pos = pick_pen_pos(
                self.world_width,
                self.world_height,
                player_center,
                &mut rand::rng(),
            );
            // New zone, new water: relocate the tide-pool hazards too, scaling with difficulty.
            let difficulty = self
                .levels
                .get(self.current_level.min(self.levels.len() - 1))
                .map(|l| l.difficulty)
                .unwrap_or(0);
            self.tide_pools = pick_tide_pools(
                self.world_width,
                self.world_height,
                self.pen_pos,
                player_center,
                difficulty,
                &mut rand::rng(),
            );
            // New zone wipes any boss-flooded water/fissures — the fresh pools are the level's own.
            self.boss_flood_pools = 0;
            self.boss_fissures.clear();
            self.boss_fissure_erupt = 0.0;
        }
        if self.current_level >= self.levels.len() {
            // Game completed, show game over screen.
            self.game_over = true;
        }
        let area = (self.world_width, self.world_height);
        self.start_current_pattern(area);
    }

    /// Bank the just-ended run into the persistent career and write it to disk. Called exactly
    /// once per run (guarded by `run_recorded`) the moment the game enters its game-over state,
    /// so even a losing run adds to a lifetime total the player carries forward — a "loss" still
    /// feels like progress. Cheap and best-effort: a failed write never disrupts play.
    fn record_run(&mut self) {
        if self.run_recorded {
            return;
        }
        self.run_recorded = true;
        self.run_is_new_best = self.score > self.career_best_score;
        if self.run_is_new_best {
            self.career_best_score = self.score;
        }
        self.career_total_score += self.score;
        self.career_runs += 1;
        self.save_career();
    }

    /// Crabs available to spend in the title-screen perk shop: everything ever banked, minus what's
    /// already been committed to permanent perks.
    fn career_available(&self) -> usize {
        self.career_total_score.saturating_sub(self.career_spent)
    }

    /// Cost of buying the next rank of a tool currently at `rank`. `None` if already maxed.
    fn perk_cost(rank: u32) -> Option<usize> {
        if rank >= MAX_START_RANK {
            None
        } else {
            Some((rank as usize + 1) * PERK_COST_STEP)
        }
    }

    /// Persist the whole career ledger (best/total/runs + spend side) to disk. Best-effort: a
    /// failed write never disrupts play.
    fn save_career(&self) {
        let _ = fs::write(
            "career.txt",
            format!(
                "{} {} {} {} {} {} {} {}\n{}\nname {}",
                self.career_best_score,
                self.career_total_score,
                self.career_runs,
                self.career_spent,
                self.start_beam_rank,
                self.start_lasso_rank,
                self.start_whistle_rank,
                self.start_stomp_rank,
                self.player_skin.to_save_line(),
                crate::normalize_player_name(&self.player_name),
            ),
        );
    }

    fn push_player_name_char(&mut self, ch: char) {
        let mut name = self.player_name.clone();
        name.push(ch);
        self.player_name = crate::sanitize_player_name(&name);
        self.save_career();
    }

    fn pop_player_name_char(&mut self) {
        let mut name = self.player_name.clone();
        name.pop();
        self.player_name = crate::sanitize_player_name(&name);
        self.save_career();
    }

    /// Title-screen skin picker: step the option in the currently focused cosmetic column
    /// (`skin_slot`: 0=Hat, 1=FacialHair, 2=Accessory) by `dir` (+1/-1), wrapping around its
    /// `::ALL` list. The change is applied to `player_skin` immediately (so the live preview
    /// and flavour text update at once) and persisted to career.txt right away.
    fn cycle_skin_option(&mut self, dir: i32) {
        let step = |len: usize, cur: usize| -> usize {
            ((cur as i32 + dir).rem_euclid(len as i32)) as usize
        };
        match self.skin_slot {
            0 => {
                let all = crate::skins::Hat::ALL;
                let cur = all
                    .iter()
                    .position(|h| *h == self.player_skin.hat)
                    .unwrap_or(0);
                self.player_skin.hat = all[step(all.len(), cur)];
            }
            1 => {
                let all = crate::skins::FacialHair::ALL;
                let cur = all
                    .iter()
                    .position(|h| *h == self.player_skin.facial_hair)
                    .unwrap_or(0);
                self.player_skin.facial_hair = all[step(all.len(), cur)];
            }
            _ => {
                let all = crate::skins::Accessory::ALL;
                let cur = all
                    .iter()
                    .position(|a| *a == self.player_skin.accessory)
                    .unwrap_or(0);
                self.player_skin.accessory = all[step(all.len(), cur)];
            }
        }
        self.save_career();
    }

    /// Title-screen purchase: buy the next permanent starting rank of one tool (1=beam, 2=lasso,
    /// 3=whistle, 4=stomp) with banked crabs. Refused (with a red flash) if the tool is maxed or
    /// there aren't enough banked crabs. On success the spend is committed to disk immediately so
    /// the perk survives even if the game closes before the next run ends.
    fn buy_start_perk(&mut self, tool: u32) {
        let rank = match tool {
            1 => self.start_beam_rank,
            2 => self.start_lasso_rank,
            3 => self.start_whistle_rank,
            4 => self.start_stomp_rank,
            _ => return,
        };
        match Self::perk_cost(rank) {
            Some(cost) if cost <= self.career_available() => {
                self.career_spent += cost;
                match tool {
                    1 => self.start_beam_rank += 1,
                    2 => self.start_lasso_rank += 1,
                    3 => self.start_whistle_rank += 1,
                    4 => self.start_stomp_rank += 1,
                    _ => {}
                }
                self.shop_flash = 1.0;
                self.save_career();
            }
            _ => {
                // Maxed out, or can't afford it: brief denial flash, no spend.
                self.shop_denied = 1.0;
            }
        }
    }

    pub(crate) fn reset_game(&mut self) {
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
        self.total_caught = 0;
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
        self.sounds.action_music.set_pitch(1.0);
        for layer in self.music_layers.iter_mut() {
            layer.set_pitch(1.0);
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
        self.weather_target = if rand::rng().random_bool(0.45) {
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
            &mut rand::rng(),
        );
        self.tide_pools = pick_tide_pools(
            self.world_width,
            self.world_height,
            self.pen_pos,
            player_pos + Vec2::splat(PLAYER_SIZE / 2.0),
            self.levels.first().map(|l| l.difficulty).unwrap_or(0),
            &mut rand::rng(),
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
        self.time_elapsed = 0.0;
        self.game_over = false;
        self.run_recorded = false;
        self.run_is_new_best = false;
        self.boost_timer = 0.0;
        self.boost_cooldown = 0.0;
        self.sprint_stamina = SPRINT_STAMINA_MAX;
        self.current_level = 0;
        self.current_pattern = 0;
        self.start_current_pattern((width, height));
    }

    /// Enter a scripted "How to Play" tutorial session from the title screen. Starts from a clean
    /// run state (so no leftover herd/boss), then constrains it into a tiny sandbox: leave the
    /// spawn patterns alone (the tutorial gates them off in update) and drop in just a handful of
    /// plain crabs to catch. The session runs the normal LIVE update/draw path — the beat clock and
    /// catches have to actually tick for a rhythm lesson — so we clear `show_instructions` and set
    /// `self.tutorial` instead of staying on the paused menu screen. Exit is opt-in: passing (or
    /// pressing Escape) returns to the menu without ever touching `game_over`, so tutorial runs
    /// never pollute the persistent career.
    /// Open the campaign world map. Creates it on first visit; subsequent visits reuse the same
    /// instance so node completion persists across runs.
    fn enter_world_map(&mut self, ctx: &mut Context) {
        if self.world_map.is_none() {
            self.world_map = Some(WorldMap::new());
        }
        self.show_instructions = false;
        self.show_how_to_play_text = false;
        self.show_world_map = true;
        self.game_over = false;
        self.in_campaign = false;
        // A calm ambient pad for the campaign map — a breather moment between levels.
        let _ = self.sounds.world_map_pad.play_detached(ctx);
    }

    /// Start a campaign run (or tutorial) from the currently selected world map node.
    /// Tutorial nodes enter a scripted sandbox; campaign nodes load a regular Level.
    fn enter_campaign_level(&mut self) {
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
        self.reset_game();
        self.current_level = level_index.min(self.levels.len().saturating_sub(1));
        self.current_pattern = 0;
        let (w, h) = (self.width, self.height);
        self.start_current_pattern((w, h));
        self.show_world_map = false;
        self.in_campaign = true;
    }

    /// Called when a campaign run ends — marks the level done, unlocks the next, and returns to
    /// the world map screen. Career stats are NOT updated here (that path stays in `record_run`).
    fn return_to_world_map(&mut self) {
        if let Some(map) = &mut self.world_map {
            map.complete_selected();
        }
        self.game_over = false;
        self.show_world_map = true;
        self.in_campaign = false;
    }

    fn enter_tutorial(&mut self, kind: TutorialKind) {
        self.reset_game();
        // reset_game seeded a normal first wave; wipe it and drop in the calm tutorial set instead.
        self.crabs.clear();
        self.crabs = spawn_tutorial_crabs(kind, 6, (self.width, self.height), &mut rand::rng());
        // Tutorial crabs spawn in a ring around the VIEWPORT centre (self.width/2, self.height/2),
        // but reset_game() parks the player at WORLD centre (the world is larger than the viewport).
        // Relocate the tutorial player onto the viewport-centre ring so its crabs are on-screen and
        // in reach — keeps the tutorial (which doubles as a regression test) self-contained rather
        // than off-screen from a world-centred player.
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
            &mut rand::rng(),
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



    // --- Effective per-tool values, derived from the chosen upgrade lanes ---
    // These fold each lane's rank into the base constants at the point of use, so a run that pours
    // level-ups into one tool visibly transforms it (a whistle build sweeps the whole screen; a
    // stomp build fires almost on demand) instead of every build feeling the same.


    // apply_upgrade now lives in src/upgrade.rs (impl MainState there).
}


impl EventHandler for MainState {
    fn update(&mut self, ctx: &mut Context) -> GameResult {
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
            let mdt = ctx.time.delta().as_secs_f32();
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
            return Ok(());
        }

        // Clamp raw delta before scaling to prevent a large first-frame hitch (shader compile,
        // audio decode, BPM detection) from collapsing the bot script's timed hold/release
        // sequence — and to guard against the general "spiral of death" when the game falls behind.
        // update_weather uses its own raw delta below and is deliberately left unclamped.
        let mut dt = ctx.time.delta().as_secs_f32().min(0.1) * self.time_scale;

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
        // moment is fully rendered — the classic Vampire-Survivors-style "punch".
        if self.hitstop_timer > 0.0 {
            self.hitstop_timer = (self.hitstop_timer - dt).max(0.0);
            return Ok(());
        }

        // Cinematic slow-motion on the biggest climax moments (boss catch, Downbeat Slam). The
        // timer decays on REAL time so the effect is always the same wall-clock length, but the
        // whole rest of the sim runs on a dilated `dt` that eases from ~35% speed back up to full
        // as the timer runs out — a smooth bullet-time ramp, not a hard freeze. `time_elapsed`
        // and everything downstream of it (beat clock, animations, particles) slow together, so
        // the moment reads as one coherent slowed frame rather than some systems stalling.
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
                            controls::handle_key_down_event(self, ctx, Some(KeyCode::E));
                        }
                    }
                }
                // Stomp anything within melee range: cracks a shelled crab we've homed onto (turning
                // an Armored/Hermit into a catchable target) so an all-shelled roll can't leave the
                // bot with nothing to catch.
                if self.stomp_cooldown <= 0.0 {
                    if let Some(target) = self.nearest_seek_target_pos() {
                        if center.distance(target) < STOMP_MAX_RADIUS {
                            controls::handle_key_down_event(self, ctx, Some(KeyCode::R));
                        }
                    }
                }
            }

            self.bot_check_done();
        }

        // Weather + day/night ambience. Runs on REAL delta (not the slowmo-dilated dt) so the
        // world clock and weather evolve at a steady wall-clock pace regardless of bullet-time.
        self.update_weather(ctx.time.delta().as_secs_f32());

        // Tutorial session bookkeeping: keep the sandbox stocked, detect the pass condition, and
        // run a short celebratory hold before handing control back to the title screen. Kept here
        // in the live path (not the paused menu gate) because a rhythm lesson needs the sim ticking.
        if self.tutorial.is_some() {
            // Real (undilated) time for the exit hold so the celebration is a fixed wall-clock
            // length regardless of any slow-mo the catch triggered.
            let real_dt = ctx.time.delta().as_secs_f32();
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
                    spawn_tutorial_crabs(tut_kind, 6, (self.width, self.height), &mut rand::rng());
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
                        self.return_to_world_map();
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
                // Speed the music/beat up for this stage — the felt "beat-tempo shift". Everything
                // synced to the beat (spawns, train step, wobble, pulses) quickens with it. Rescale
                // the in-flight beat_timer by the same ratio so the current beat's phase is preserved
                // (no jarring skip) but the next beat arrives sooner.
                let tempo_mul = INTENSITY_STAGES[self.intensity_stage].3;
                let new_interval = BEAT_INTERVAL / tempo_mul;
                if self.beat_interval > 0.0 {
                    self.beat_timer *= new_interval / self.beat_interval;
                }
                self.beat_interval = new_interval;
                // Musical punch so the escalation lands as a moment: brighten the beat, flash, a
                // short shake, and a rising-tension chime.
                self.beat_intensity = 2.0;
                self.on_beat_flash = self.on_beat_flash.max(0.6);
                self.screen_shake = self.screen_shake.max(8.0);
                let kick = rand::rng().random_range(0.0_f32..std::f32::consts::TAU);
                self.screen_shake_vel = Vec2::new(kick.cos(), kick.sin()) * 8.0 * 60.0;
                // upgrade.ogg removed — tiresome and crackly; new sound TBD
            }
        }

        // Track player position history for conga chain
        self.position_history.push_front(self.player_pos);
        if self.position_history.len() > 2000 {
            self.position_history.pop_back();
        }

        // Swung hi-hat kit — locks the LIVE percussion to the same 1/16 grid the backing groove
        // shuffles on, instead of clicking straight quarter-note kicks against a shuffling loop.
        // The kick already lands the downbeat (local step 0); here we fill the offbeats between
        // kicks: the "and" (step 2, a straight 1/8) always ticks, and the swung 1/16 "e"/"a"
        // (steps 1 & 3, pushed late by the shared GROOVE_SWING) come in only once the run is busy
        // (a longer train / higher intensity stage), so the pocket thickens as the party grows —
        // "more crabs in sync = more music" (INSPIRATION.md, Crab Rave). Edge-detected off the
        // master beat clock via a global step id so each hat fires exactly once as the clock
        // crosses its onset, never double-firing or skipping even at low fps.
        if self.beat_interval > 1e-4 {
            let frac = (1.0 - self.beat_timer / self.beat_interval).clamp(0.0, 1.0);
            // Busy = a fat train or an escalated stage; drives both hat density and loudness.
            let train_fill = (self.chain_count as f32 / 24.0).clamp(0.0, 1.0);
            let stage_span = (INTENSITY_STAGES.len().saturating_sub(1)).max(1) as f32;
            let stage_fill = (self.intensity_stage as f32 / stage_span).clamp(0.0, 1.0);
            let busy = self.chain_count >= 8 || self.intensity_stage >= 1;
            // Base hat loudness rises with the party; the swung ghost 1/16s sit quieter than the
            // "and" so the offbeat pulse stays legible instead of a wash of noise.
            let base_vol = 0.26 + 0.16 * train_fill + 0.10 * stage_fill;
            let swing_late = crate::sounds::GROOVE_SWING * 0.125; // odd 1/16 late, in beat fractions
            for local in 1..=3u32 {
                let onset = local as f32 * 0.25 + if local % 2 == 1 { swing_late } else { 0.0 };
                let gstep = self.beat_count as i64 * 4 + local as i64;
                if frac + 1e-6 >= onset && gstep > self.hat_last_step {
                    self.hat_last_step = gstep;
                    // Step 2 (the straight "and") always plays; the swung 1/16 ghosts only when busy.
                    if local == 2 {
                        self.beat_synth.play_hihat(ctx, base_vol);
                    } else if busy {
                        self.beat_synth.play_hihat(ctx, base_vol * 0.55);
                    }
                }
            }
        }

        // Beat timer — interval speeds up with the intensity stage (see beat_interval).
        self.beat_timer -= dt;
        if self.beat_timer <= 0.0 {
            self.on_beat(ctx);
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
            let mut rng = rand::rng();
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
                if c.is_boss() && !c.caught && !c.is_tide_boss() && !c.is_rhythm_boss() {
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

        // Drum Roll (hold T): poll the held key here rather than off the key-down event, since the
        // event fires unreliably on key-repeat and we need a clean "held across beats" charge. The
        // per-beat hit counting lives in the beat handler; here we only edge-detect press/release
        // and drive the timers. Releasing after landing at least one on-beat roll hit FIRES a
        // focused beam blast; releasing with nothing charged just cancels quietly.
        let t_held = !self.show_instructions
            && !self.game_over
            && ctx
                .keyboard
                .is_key_pressed(ggez::input::keyboard::KeyCode::T);
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
                .spawn_dash_burst(center, self.last_dir, &mut rand::rng());
            // A GROOVE DASH (on-beat, gather-wake armed this same frame) throws an extra, brighter
            // burst so a watcher can instantly tell the timed dash apart from the plain escape dash.
            if self.groove_dash_timer > 0.0 {
                let rng = &mut rand::rng();
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
                .spawn_catch_effect(pos, color, CrabType::Normal, &mut rand::rng());
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
                &mut rand::rng(),
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
                let mut rng = rand::rng();
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
            for (i, crab) in self.crabs.iter_mut().enumerate() {
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
                        let mut rng = rand::rng();
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
                                self.on_boss_caught(pos, self.crabs[i].is_tide_boss());
                            }
                            if self.crabs[i].is_golden() {
                                self.on_golden_caught(pos, 0);
                            }
                            self.reward_dance_catch(was_answering, pos);
                            lasso_startle_origins.push(pos);
                            self.chain_join_ripple = true;
                            self.crabs[i].chain_index = Some(self.chain_count);
                            self.chain_count += 1;
                            self.check_milestone(&mut rand::rng());
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
            // Rotate the three boss archetypes so every run cycles through all three climax beats:
            // the King Crab (charge — route the train out of the lane), the Tide Boss (pulse — pull
            // the train back out of range), and the Reef DJ (rhythm — its shell only drops when you
            // hold the light on it *on the beat*). Cycling guarantees variety instead of RNG streaks.
            let (boss, title, hint, title_color) = match self.next_boss_kind {
                1 => (
                    spawn_tide_boss(
                        (self.world_width, self.world_height),
                        &mut rand::rng(),
                        BOSS_MAX_HEALTH,
                    ),
                    "A TIDE BOSS SURGES IN!",
                    "Hold your light — but keep your train clear of its pulse!",
                    [0.35, 0.8, 1.0, 1.0],
                ),
                2 => (
                    spawn_rhythm_boss(
                        (self.world_width, self.world_height),
                        &mut rand::rng(),
                        BOSS_MAX_HEALTH,
                    ),
                    "THE REEF DJ DROPS IN!",
                    "Echo the lit pips with light — or catch its dancers on a hot beat!",
                    [0.75, 0.4, 1.0, 1.0],
                ),
                _ => (
                    spawn_boss(
                        (self.world_width, self.world_height),
                        &mut rand::rng(),
                        BOSS_MAX_HEALTH,
                    ),
                    "A KING CRAB APPROACHES!",
                    "Hold your light on it!",
                    [1.0, 0.8, 0.2, 1.0],
                ),
            };
            self.next_boss_kind = (self.next_boss_kind + 1) % 3;
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
                .spawn_milestone_fireworks(bpos, 12, &mut rand::rng());
            let a = rand::rng().random_range(0.0_f32..std::f32::consts::TAU);
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
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        // Bot mode: skip all rendering to run at maximum speed — UNLESS we're recording a
        // gameplay clip (RUSTLER_RECORD set), in which case a bot drives real gameplay and we
        // want the scene on screen for a screen-recorder to capture.
        if self.bot.is_some() && std::env::var_os("RUSTLER_RECORD").is_none() {
            let canvas = Canvas::from_frame(ctx, ggez::graphics::Color::BLACK);
            canvas.finish(ctx)?;
            return Ok(());
        }

        // --- Pass 1: render the game scene to an offscreen crisp image ---
        self.draw_scene(ctx)?;

        // --- Pass 1.5: conga trail / echo-afterimage accumulation (ping-pong) ---
        // A single fixed extra full-screen pass: composite the crisp scene as an opaque base,
        // then additively lay the faded bright residue of the PREVIOUS frame on top. The trail
        // shader keeps only bright/additive elements (crab glows, beat rings, rope heat), so the
        // moving conga train leaves a comet of decaying light while the terrain stays clean.
        //
        // `trail_strength` folds the per-frame feedback decay together with a groove curve: it is
        // 0 below groove 0.2 (normal play renders identically to a plain scene blit) and smoothsteps
        // up to ~0.86 at max groove, so the delirium is earned and never obscures the rhythm read.
        let g = self.groove;
        let trail_strength = if g <= 0.2 {
            0.0
        } else {
            let t = ((g - 0.2) / 0.8).clamp(0.0, 1.0);
            (t * t * (3.0 - 2.0 * t)) * 0.86
        };
        // Below the groove threshold this pass is a documented no-op (pixel-exact copy of the
        // scene), so skip the whole offscreen render — canvas bind, blit draw, blend-mode
        // switches, finish()/GPU submit — and feed the scene straight into Pass 2 instead. This
        // is the common case (normal play sits under groove 0.2 most of the time), so avoiding a
        // full extra full-screen pass here is a real per-frame win. The trail ping-pong buffers
        // are simply left untouched; when groove climbs back past 0.2, trail_strength ramps up
        // from ~0 so any staleness in the buffers contributes a negligible first-frame blend.
        let write_img = if trail_strength > 0.0 {
            // Ping-pong: read last frame's accumulation, write this frame's. Both images are
            // allocated once (state.rs) and reused — no per-frame image allocation.
            let (read_img, write_img) = if self.trail_swap {
                (self.trail_image_a.clone(), self.trail_image_b.clone())
            } else {
                (self.trail_image_b.clone(), self.trail_image_a.clone())
            };
            let scene = self.scene_image.clone();
            let tu = TrailUniform {
                strength: trail_strength,
            };
            self.trail_params.set_uniforms(ctx, &tu);
            {
                let mut acc = Canvas::from_image(ctx, write_img.clone(), Color::BLACK);
                acc.set_sampler(Sampler::nearest_clamp());
                acc.draw(&scene, DrawParam::default().dest(Vec2::ZERO));
                acc.set_shader(&self.trail_shader);
                acc.set_shader_params(&self.trail_params);
                acc.set_blend_mode(BlendMode::ADD);
                acc.draw(&read_img, DrawParam::default().dest(Vec2::ZERO));
                acc.set_blend_mode(BlendMode::ALPHA);
                acc.set_default_shader();
                acc.finish(ctx)?;
            }
            self.trail_swap = !self.trail_swap;
            write_img
        } else {
            self.scene_image.clone()
        };

        // --- Pass 2: blit the accumulated scene to screen with post-processing ---
        {
            let (draw_w, draw_h) = ctx.gfx.drawable_size();
            let _scale_x = draw_w / self.width;
            let _scale_y = draw_h / self.height;
            // title_card_t: ease in fast, hold, ease out over the last 0.6s
            let title_t = if self.level_title_timer > 0.0 {
                let hold = 2.5_f32;
                let fade_in = (1.0 - (self.level_title_timer - hold) / 0.3).clamp(0.0, 1.0);
                let fade_out = (self.level_title_timer / 0.6).clamp(0.0, 1.0);
                fade_in.min(fade_out)
            } else {
                0.0
            };
            let uniform = PostProcessUniform {
                groove: self.groove,
                time: self.time_elapsed,
                screen_width: self.width,
                screen_height: self.height,
                title_card_t: title_t,
            };
            // Reuse cached shader params, just update uniforms (avoids per-frame GPU buffer alloc)
            self.postprocess_params.set_uniforms(ctx, &uniform);
            let mut screen_canvas = Canvas::from_frame(ctx, Color::BLACK);
            screen_canvas.set_shader(&self.postprocess_shader);
            screen_canvas.set_shader_params(&self.postprocess_params);
            screen_canvas.draw(
                &write_img,
                DrawParam::default().dest(Vec2::ZERO),
            );
            screen_canvas.set_default_shader();
            screen_canvas.finish(ctx)?;
        }

        Ok(())
    }

    fn key_down_event(&mut self, ctx: &mut Context, input: KeyInput, _repeat: bool) -> GameResult {
        if self.pending_upgrade {
            // The choice is a live overlay now, not a freeze: 1/2/3 pick a card, but every other key
            // falls through to normal in-game handling so the player can keep steering and using
            // tools while they decide (and a rival can steal from them mid-decision — the intended
            // pressure to pick fast). 1/2/3 aren't bound to anything in-game (they're loadout-screen
            // only), so consuming them here can't shadow a gameplay action.
            if let Some(key) = input.keycode {
                match key {
                    KeyCode::Key1 => {
                        self.apply_upgrade(1);
                        return Ok(());
                    }
                    KeyCode::Key2 => {
                        self.apply_upgrade(2);
                        return Ok(());
                    }
                    KeyCode::Key3 => {
                        self.apply_upgrade(3);
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
        if let Some(key) = input.keycode {
            if key == KeyCode::F {
                self.flashlight.on = !self.flashlight.on;
                use ggez::audio::SoundSource;
                // Slightly higher pitch on, lower on off, so the toggle direction is audible.
                let pitch = if self.flashlight.on { 1.15 } else { 0.85 };
                self.sounds.flashlight_toggle.set_pitch(pitch);
                let _ = self.sounds.flashlight_toggle.play_detached(ctx);
                return Ok(());
            }
        }
        if handle_key_down_event(self, ctx, input.keycode) {
            return Ok(());
        }
        Ok(())
    }

    fn text_input_event(&mut self, _ctx: &mut Context, character: char) -> GameResult {
        if self.show_instructions
            && !self.show_world_map
            && !self.game_over
            && !self.pending_upgrade
        {
            if self.menu_page == 1 && !character.is_control() {
                if self.player_name.chars().count() < 24 {
                    self.push_player_name_char(character);
                }
            }
        }
        Ok(())
    }

    fn mouse_motion_event(
        &mut self,
        ctx: &mut Context,
        x: f32,
        y: f32,
        _xrel: f32,
        _yrel: f32,
    ) -> GameResult {
        let window_size = ctx.gfx.window().inner_size();
        let scale_x = window_size.width as f32 / self.width;
        let scale_y = window_size.height as f32 / self.height;
        // mouse_pos is used against player/crab positions (world space) for flashlight aim and crab
        // picking, so store it in world space: screen point offset by the camera origin.
        self.mouse_pos = self.camera_origin + Vec2::new(x / scale_x, y / scale_y);
        Ok(())
    }

    fn mouse_button_down_event(
        &mut self,
        ctx: &mut Context,
        button: MouseButton,
        x: f32,
        y: f32,
    ) -> GameResult {
        // Upgrade screen: let the player click a card as an alternative to the number keys.
        if self.pending_upgrade {
            if button == MouseButton::Left {
                let window_size = ctx.gfx.window().inner_size();
                let scale_x = window_size.width as f32 / self.width;
                let scale_y = window_size.height as f32 / self.height;
                let p = Vec2::new(x / scale_x, y / scale_y);
                let rects = self.upgrade_card_rects();
                for (i, r) in rects.iter().enumerate() {
                    if p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h {
                        self.apply_upgrade(i as u8 + 1);
                        break;
                    }
                }
            }
            return Ok(());
        }
        if self.game_over || self.show_instructions {
            return Ok(());
        }
        // Left click: BEGIN winding up the lasso. The throw fires on mouse_button_up.
        if button == MouseButton::Left && self.lasso_phase == LassoPhase::Idle {
            self.lasso_mouse_down = true;
            self.lasso_charge = 0.0;
            self.lasso_spin = 0.0;
            self.lasso_phase = LassoPhase::Winding;
            // Capture player center for the windup origin; target is updated every frame from mouse_pos.
            self.lasso_origin = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
        }
        Ok(())
    }

    fn mouse_button_up_event(
        &mut self,
        ctx: &mut Context,
        button: MouseButton,
        _x: f32,
        _y: f32,
    ) -> GameResult {
        if button == MouseButton::Left && self.lasso_phase == LassoPhase::Winding {
            self.lasso_mouse_down = false;
            {
                use ggez::audio::SoundSource;
                let _ = self.sounds.lasso_sfx.play_detached(ctx);
            }
            // Compute scaled range from charge: tap = MIN_RANGE_FRAC × MAX_RANGE, full = MAX_RANGE.
            let charge_frac = (self.lasso_charge / LASSO_MAX_CHARGE_TIME).min(1.0);
            let range_frac = LASSO_MIN_RANGE_FRAC + (1.0 - LASSO_MIN_RANGE_FRAC) * charge_frac;
            // On-beat release bonus: extra reach + groove reward.
            let on_beat_bonus = if self.on_beat_now() {
                let center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
                self.reward_on_beat_tool(center, "LASSO");
                LASSO_ONBEAT_BONUS
            } else {
                1.0
            };
            self.lasso_on_beat_bonus = on_beat_bonus;
            let throw_range = LASSO_MAX_RANGE * range_frac * on_beat_bonus;
            // Clamp target within throw_range of player center.
            let origin = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            // Auto-aim: snap the throw toward the nearest catchable crab in reach so a well-timed
            // release lands a catch without fiddly aiming. Charge/recharge/on-beat release are
            // unchanged — only WHERE the loop flies is assisted. Empty field falls back to manual aim.
            let aim_point = self.lasso_aim_point(origin, throw_range);
            let to_aim = aim_point - origin;
            let aim_dist = to_aim.length();
            let clamped_target = if aim_dist > throw_range {
                origin + to_aim / aim_dist * throw_range
            } else if aim_dist > 1.0 {
                aim_point
            } else {
                // Mouse right on player — throw in the last-faced direction.
                origin + self.last_dir.normalize_or_zero() * throw_range
            };
            self.lasso_target = clamped_target;
            self.lasso_origin = origin;
            // Throw speed also scales with charge: a full charge is faster than a tap.
            // We achieve this by scaling LASSO_THROW_TIME inversely with range_frac.
            let throw_time = LASSO_THROW_TIME / range_frac.max(0.15);
            self.lasso_timer = throw_time;
            self.lasso_phase = LassoPhase::Throwing;
            self.lasso_pos = Some(origin);
            self.lasso_charge = 0.0;
        }
        Ok(())
    }
}

fn main() -> GameResult {
    let resource_dir = if let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") {
        let mut path = path::PathBuf::from(manifest_dir);
        path.push("resources");
        path
    } else {
        path::PathBuf::from("./resources")
    };

    let args: Vec<String> = std::env::args().collect();
    let bot_script: Option<String> = args
        .windows(2)
        .find(|w| w[0] == "--bot")
        .map(|w| w[1].clone());

    let (mut ctx, event_loop) = ContextBuilder::new("rustler", "carlthome")
        .add_resource_path(resource_dir)
        .window_mode(WindowMode::default())
        .build()?;
    ctx.gfx.window().set_cursor_visible(false);
    let mut state = MainState::new(&mut ctx)?;

    if let Some(ref name) = bot_script {
        use bot::{
            BotState, script_campaign_tutorial, script_groove_dash, script_menu_to_game,
            script_npc_steal, script_npc_vs_npc, script_player_steal, script_revenge,
            script_steal_defense, script_steal_dodge,
        };
        // menu_to_game and campaign_tutorial run at 3× so the proximity catch check fires frequently
        // enough for the seek-catch autopilot to register catches (at 8× the player teleports past
        // crabs between frames, catching nothing). campaign_tutorial's BeatTiming lesson clears on
        // ON-BEAT catches, which the autopilot lands by volume (a steady stream of whistle catches at
        // a ~30% on-beat rate); its script leaves a wide time margin so even an unlucky low-rate run
        // banks 3 on-beat catches and returns to the world map before the final assert.
        state.time_scale = if std::env::var_os("RUSTLER_RECORD").is_some() {
            // Recording a shareable clip: run at real time so the captured gameplay looks
            // natural rather than the sped-up pace the headless playtests use.
            1.0
        } else {
            match name.as_str() {
                // The chain-dependent defense scenarios (parry/dodge/revenge) need the seek-catch
                // autopilot to reliably hold a >=2-link chain for their ForceStealDefense/Dodge/
                // RevengeCross attempts to have anything to act on. At 3x the *effective* per-frame
                // step (real_dt * time_scale) grows large on a slow/loaded CI runner, and — as the
                // 8x comment below notes — the player then teleports past crabs and catches nothing,
                // so the chain stalls at 1 link and the parry/revenge asserts flake red. 2x keeps the
                // step small enough that catches register reliably, trading a little wall-clock (still
                // a parallel matrix leg) for a green that isn't a coin-flip.
                "steal_defense" | "steal_dodge" | "revenge" => 2.0,
                "menu_to_game" | "campaign_tutorial" | "npc_steal" | "player_steal"
                | "npc_vs_npc" => 3.0,
                _ => 8.0,
            }
        };
        state.bot = Some(match name.as_str() {
            "menu_to_game" => BotState::new(script_menu_to_game(), 60.0),
            "campaign_tutorial" => BotState::new(script_campaign_tutorial(), 76.0),
            "npc_steal" => BotState::new(script_npc_steal(), 58.0),
            "player_steal" => BotState::new(script_player_steal(), 58.0),
            "steal_defense" => BotState::new(script_steal_defense(), 58.0),
            "steal_dodge" => BotState::new(script_steal_dodge(), 58.0),
            "revenge" => BotState::new(script_revenge(), 58.0),
            "npc_vs_npc" => BotState::new(script_npc_vs_npc(), 56.0),
            "groove_dash" => BotState::new(script_groove_dash(), 10.0),
            other => {
                eprintln!("Unknown bot script: {}", other);
                std::process::exit(1);
            }
        });
    }

    event::run(ctx, event_loop, state)
}

#[cfg(test)]
mod how_to_play_tests {
    use super::how_to_play_body_text;

    #[test]
    fn how_to_play_text_matches_current_controls() {
        let text = how_to_play_body_text();
        for expected in [
            "Shift",
            "Space: dash",
            "Q: wave",
            "E: whistle",
            "R: stomp",
            "F: call",
            "X: cycle",
            "V: groove call",
            "G: downbeat slam",
            "B: bank",
        ] {
            assert!(
                text.contains(expected),
                "missing expected control text: {expected}"
            );
        }
        assert!(!text.contains("Z: whistle"));
        assert!(!text.contains("C: cycle"));
    }
}

#[cfg(test)]
mod player_name_tests {
    use super::{normalize_player_name, sanitize_player_name};

    #[test]
    fn editing_name_can_be_empty() {
        assert_eq!(sanitize_player_name("Crabby"), "Crabby");
        assert_eq!(sanitize_player_name(""), "");
        assert_eq!(sanitize_player_name("   "), "");
    }

    #[test]
    fn empty_name_gets_default_when_used_as_a_display_name() {
        assert_eq!(normalize_player_name(""), "Crabby");
    }
}
