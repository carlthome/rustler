mod audio_mix;
mod beat;
mod bot;
mod catch_deliver;
mod catch_effects;
mod chain_mechanics;
mod chain_steal;
mod constants;
mod controls;
mod crab_boss_update;
mod crab_render;
mod crab_update;
mod enemies;
mod floating_text;
mod game_lifecycle;
mod game_render;
mod game_render_hud;
mod game_update;
mod graphics;
mod hud_cache;
mod king_crab_audio;
mod levels;
mod menu;
mod npc_conga_train;
mod npc_scenarios;
mod npc_trains;
mod npc_trains_render;
mod overlays;
mod player_tools;
mod rng;
mod skins;
mod sounds;
mod spawnings;
mod startle;
mod state;
mod state_init;
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
        "Gather wild crabs into a conga train, then bank it at the pen.",
        "Everything you do pays more ON THE BEAT.",
        "Move with WASD / arrows  ·  hold Shift to sprint.",
        "Tap Space on the beat to Dash — or hold a tool (E/R/Q) and tap",
        "Space to 'chord' that tool onto the beat instead of dashing.",
        "",
        "Your tools — each is for a different job:",
        "- Space  Dash: burst to a crab, or shake off a King Crab",
        "- E  Whistle: yank skittish crabs toward you",
        "- R  Stomp: crack armored shells, and guard your tail",
        "- Q  Wave: on-beat shockwave — shove nearby rivals back to clear space",
        "- F  Flashlight: toggle it on to auto-melt the nearest King Crab catchable",
        "- T  Call: charm nearby Dancer crabs to hop over to you on the beat",
        "- X  Cycle: rotate the train — tuck your best crabs up front",
        "- V  Groove Call: lure the whole field in over a few bars",
        "- G  Slam: full-groove finisher — mass-catch everything near",
        "- B  Bank: lock in your gamble streak (and jam!)",
        "- Mouse (hold / release): lasso a cluster and reel it in",
        "",
        "Press Enter, Space, or Esc to go back.",
    ]
    .join("\n")
}

// Re-exported at the crate root so the `use crate::*;` globs in sibling modules
// (beat, catch_deliver, …) resolve `SoundSource`'s methods. main.rs no longer calls
// them directly since the lifecycle/reset audio moved to game_lifecycle.rs, but the
// trait must stay in scope here for those glob consumers.
#[allow(unused_imports)]
use ggez::audio::SoundSource;
use ggez::conf::{FullscreenType, WindowMode};
use ggez::event::{self, EventHandler};
use ggez::winit::dpi::LogicalSize;
use ggez::glam::Vec2;
use ggez::graphics::{BlendMode, Canvas, Color, DrawParam, Sampler};
use ggez::input::keyboard::{KeyCode, KeyInput};
use ggez::winit::keyboard::PhysicalKey;
use ggez::input::mouse::MouseButton;
use ggez::{Context, ContextBuilder, GameResult};
use rand::Rng;

use crate::controls::{handle_key_down_event, handle_player_movement};
use crate::enemies::{BossCharge, CrabType, EnemyCrab, HermitKingPhase, hermit_king_phase};
use crate::levels::{TerrainKind, get_levels};
use crate::spawnings::{
    spawn_boss, spawn_dancer_king, spawn_enemies, spawn_hermit_king, spawn_hype_dancer,
    spawn_rhythm_boss, spawn_tide_boss, spawn_tutorial_crabs,
};
use crate::tutorial::TutorialKind;

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
        let mut rng = crate::rng::rng();
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
        let mut rng = crate::rng::rng();
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
                self.on_boss_caught(pos, self.crabs[i].crab_type);
            }
            if self.crabs[i].is_golden() {
                self.on_golden_caught(pos, 0);
            }
            self.reward_dance_catch(was_answering, pos);
            self.emit_catch_startle(pos);
            self.chain_join_ripple = true;
            self.crabs[i].chain_index = Some(self.chain_count);
            self.chain_count += 1;
            self.check_milestone(&mut crate::rng::rng());
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
        let mut rng = crate::rng::rng();
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
            if let Some(next_level) = self.levels.get(self.current_level) {
                self.resize_world(next_level.map_size);
            }
            // Fresh biome, fresh pen location — keep routing the train there a live decision.
            let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            self.pen_pos = pick_pen_pos(
                self.world_width,
                self.world_height,
                player_center,
                &mut crate::rng::rng(),
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
                &mut crate::rng::rng(),
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




    // --- Effective per-tool values, derived from the chosen upgrade lanes ---
    // These fold each lane's rank into the base constants at the point of use, so a run that pours
    // level-ups into one tool visibly transforms it (a whistle build sweeps the whole screen; a
    // stomp build fires almost on demand) instead of every build feeling the same.


    // apply_upgrade now lives in src/upgrade.rs (impl MainState there).
}


impl EventHandler for MainState {
    fn update(&mut self, ctx: &mut Context) -> GameResult {
        self.tick(ctx)
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
        // ggez 0.10 (winit 0.30) no longer exposes `KeyInput::keycode`; derive the physical
        // key code ourselves so the rest of the handling reads exactly as before.
        let keycode = match input.event.physical_key {
            PhysicalKey::Code(code) => Some(code),
            _ => None,
        };
        // Player-name text entry. ggez 0.10 removed the separate `text_input_event` callback and
        // delivers typed text on the key event itself (`input.event.text`). Handled first and
        // unconditionally (like 0.9's independent text callback) so a name character still lands
        // even when a later branch returns early for the same key.
        if self.show_instructions
            && !self.show_world_map
            && !self.game_over
            && !self.pending_upgrade
            && self.menu_page == 1
        {
            if let Some(text) = &input.event.text {
                for character in text.chars() {
                    if !character.is_control() && self.player_name.chars().count() < 24 {
                        self.push_player_name_char(character);
                    }
                }
            }
        }
        if self.pending_upgrade {
            // The choice is a live overlay now, not a freeze: 1/2/3 pick a card, but every other key
            // falls through to normal in-game handling so the player can keep steering and using
            // tools while they decide (and a rival can steal from them mid-decision — the intended
            // pressure to pick fast). 1/2/3 aren't bound to anything in-game (they're loadout-screen
            // only), so consuming them here can't shadow a gameplay action.
            if let Some(key) = keycode {
                match key {
                    KeyCode::Digit1 => {
                        self.apply_upgrade(1);
                        return Ok(());
                    }
                    KeyCode::Digit2 => {
                        self.apply_upgrade(2);
                        return Ok(());
                    }
                    KeyCode::Digit3 => {
                        self.apply_upgrade(3);
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
        if let Some(key) = keycode {
            if key == KeyCode::KeyF {
                self.flashlight.on = !self.flashlight.on;
                use ggez::audio::SoundSource;
                // Slightly higher pitch on, lower on off, so the toggle direction is audible.
                let pitch = if self.flashlight.on { 1.15 } else { 0.85 };
                self.sounds.flashlight_toggle.set_pitch(pitch);
                let _ = self.sounds.flashlight_toggle.play();
                return Ok(());
            }
        }
        if handle_key_down_event(self, ctx, keycode) {
            return Ok(());
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
        _ctx: &mut Context,
        button: MouseButton,
        _x: f32,
        _y: f32,
    ) -> GameResult {
        if button == MouseButton::Left && self.lasso_phase == LassoPhase::Winding {
            self.lasso_mouse_down = false;
            {
                use ggez::audio::SoundSource;
                let _ = self.sounds.lasso_sfx.play();
            }
            // Compute scaled range from charge: tap = MIN_RANGE_FRAC × MAX_RANGE, full = MAX_RANGE.
            let charge_frac = (self.lasso_charge / LASSO_MAX_CHARGE_TIME).min(1.0);
            let range_frac = LASSO_MIN_RANGE_FRAC + (1.0 - LASSO_MIN_RANGE_FRAC) * charge_frac;
            // On-beat release bonus: extra reach + groove reward. Uses the wider ranged-cast window
            // (#164) so a slightly-early/late release still reads on-beat — the lasso is a
            // cooldown-gated throw, not the tight dash.
            let on_beat_bonus = if self.on_beat_action() {
                let center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
                self.reward_on_beat_action(center, "LASSO");
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

    // Seed the deterministic bot RNG BEFORE anything (incl. MainState::new's initial king-crab
    // name generation) draws from it, so the ENTIRE bot run — construction included — is
    // reproducible. Bot-only and skipped for RUSTLER_RECORD; see the fuller note at the bot setup
    // below. Real interactive play never reaches this branch, so its RNG stays entropy-seeded.
    if let Some(ref name) = bot_script {
        if std::env::var_os("RUSTLER_RECORD").is_none() {
            // Per-scenario constant seed: distinct streams keep scenarios independent while each
            // stays reproducible. A hash of the name gives a stable, unique-per-scenario u64.
            let seed = name.bytes().fold(0xC5AB_1234_5678_9ABC_u64, |h, b| {
                h.rotate_left(7) ^ (b as u64).wrapping_mul(0x100000001B3)
            });
            rng::seed(seed);
        }
    }

    let (mut ctx, event_loop) = ContextBuilder::new("rustler", "carlthome")
        .add_resource_path(resource_dir)
        .window_mode(WindowMode {
            fullscreen_type: FullscreenType::Desktop,
            logical_size: Some(LogicalSize::new(1280.0, 960.0)),
            ..WindowMode::default()
        })
        .build()?;
    ctx.gfx.window().set_cursor_visible(false);
    // Skip real fullscreen: with ggez 0.10 + winit 0.30 on macOS every fullscreen
    // path (ggez's Desktop, winit's Borderless, WindowExtMacOS's simple_fullscreen,
    // even the OS green-button transition) either fails to activate or hangs the
    // wgpu surface with a beachball. Instead, size the window to (roughly) the
    // current monitor so it *looks* fullscreen without touching the fullscreen API.
    if let Some(monitor) = ctx.gfx.window().current_monitor() {
        let size = monitor.size();
        let scale = monitor.scale_factor();
        let logical_w = (size.width as f64 / scale) as f32;
        let logical_h = (size.height as f64 / scale) as f32;
        let _ = ctx.gfx.window().request_inner_size(LogicalSize::new(logical_w, logical_h));
    }
    let mut state = MainState::new(&mut ctx)?;

    if let Some(ref name) = bot_script {
        use bot::{
            BotState, script_campaign_escape, script_campaign_loss, script_campaign_tutorial,
            script_groove_dash, script_menu_to_game, script_npc_steal, script_npc_vs_npc,
            script_player_steal, script_revenge, script_steal_defense, script_steal_dodge,
        };
        // ── Determinism, root-cause fix for playtest flakiness ────────────────────────────────
        // The bot asserts on emergent outcomes ("a revenge steal happened"), which are only a
        // stable pass/fail if every run is reproducible. Two things make a run vary:
        //
        //   1. A wall-clock timestep. The sim advances by `ctx.time.delta() * time_scale`, so the
        //      SAME scenario takes different numbers of steps — and reaches different emergent
        //      states — at 30 fps vs 95 fps (exactly why steal_dodge passed on ggez 0.9.3 @~30fps
        //      but flaked on 0.10 @~95fps). We pin a FIXED per-frame dt below so the sim is
        //      frame-count-driven and identical regardless of machine speed or ggez version.
        //   2. An entropy-seeded RNG. Spawns, rival/NPC AI, and name generation all draw from
        //      `crate::rng::rng()`. It's seeded from a per-scenario constant above (before
        //      MainState::new) so the draw sequence is the same every run.
        //
        // Both are strictly bot-only: interactive play never seeds the RNG and never sets a fixed
        // dt, so its variable-dt smooth rendering and entropy randomness are completely unchanged.
        // RUSTLER_RECORD (the shareable-GIF path) is deliberately excluded so the captured clip
        // still plays at natural wall-clock speed.
        if std::env::var_os("RUSTLER_RECORD").is_none() {
            // Fixed simulation timestep (default 1/60 s). RUSTLER_BOT_DT overrides it so a run can
            // be replayed at a different effective frame rate to prove the outcome is truly
            // frame-rate independent (e.g. RUSTLER_BOT_DT=0.0333 ~ 30 fps, =0.00833 ~ 120 fps).
            let fixed_dt = std::env::var("RUSTLER_BOT_DT")
                .ok()
                .and_then(|s| s.parse::<f32>().ok())
                .filter(|d| *d > 0.0)
                .unwrap_or(1.0 / 60.0);
            state.bot_fixed_dt = Some(fixed_dt);
        }
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
                "campaign_escape" | "campaign_loss" | "menu_to_game" | "campaign_tutorial"
                | "npc_steal" | "player_steal" | "npc_vs_npc" => 3.0,
                _ => 8.0,
            }
        };
        state.bot = Some(match name.as_str() {
            "menu_to_game" => BotState::new(script_menu_to_game(), 60.0),
            "campaign_escape" => BotState::new(script_campaign_escape(), 8.0),
            "campaign_loss" => BotState::new(script_campaign_loss(), 8.0),
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
            "Space  Dash",
            "Q  Wave",
            "E  Whistle",
            "R  Stomp",
            "F  Flashlight",
            "T  Call",
            "X  Cycle",
            "V  Groove Call",
            "G  Slam",
            "B  Bank",
        ] {
            assert!(
                text.contains(expected),
                "missing expected control text: {expected}"
            );
        }
        // F is the flashlight toggle, not Call — the Call summon now lives on T.
        assert!(!text.contains("F  Call"));
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
