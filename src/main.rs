mod bot;
mod constants;
mod controls;
mod enemies;
mod floating_text;
mod graphics;
mod hud_cache;
mod levels;
mod menu;
mod skins;
mod sounds;
mod spawnings;
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
fn how_to_play_body_text() -> String {
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
use ggez::graphics::{BlendMode, Canvas, Color, DrawParam, Mesh, Rect, Sampler, Text};
use ggez::input::keyboard::{KeyCode, KeyInput};
use ggez::input::mouse::MouseButton;
use ggez::{Context, ContextBuilder, GameResult};
use rand::Rng;
use spawnings::SpawnPattern;

use crate::controls::{handle_key_down_event, handle_player_movement};
use crate::enemies::{BossCharge, CrabType, EnemyCrab};
use crate::graphics::{
    LassoDrawPhase, cached_stroke_rect, draw_ambient_motes, draw_armor_ring,
    draw_attracted_crab_glow, draw_beat_hit_punch, draw_beat_indicator, draw_beat_wave_ring,
    draw_boss_fissures, draw_boss_health_ring, draw_call_ring, draw_catch_bloom_ring,
    draw_catch_next_hint, draw_catch_shockwaves, draw_catch_trails, draw_centerpiece_ring,
    draw_chain_rings, draw_cleave_slash, draw_cleave_stakes, draw_combo_meter, draw_conga_rope,
    draw_crab, draw_crab_radar, draw_cycle_preview_ring, draw_deliver_beam, draw_delivery_pen,
    draw_delivery_streak, draw_downbeat_pulse_ring, draw_fear_rings, draw_flashlight,
    draw_floating_texts, draw_golden_sparkle, draw_grass, draw_groove_call_ring,
    draw_groove_vignette, draw_haul_worth, draw_hermit_shell, draw_kelp_snag_warning, draw_lasso,
    draw_lasso_windup, draw_magnet_aura, draw_particles, draw_pen_guide, draw_penned_marchers,
    draw_puddle_ripples, draw_reef_phrase, draw_rustler, draw_sky_overlay, draw_slam_ring,
    draw_speed_lines, draw_splitter_aura, draw_sprint_whoosh, draw_stomp_ring, draw_tail_run_badge,
    draw_thief_aura, draw_tide_pools, draw_tide_pulses, draw_train_at_risk, draw_wave_telegraph,
    draw_weather, draw_whistle_ring, draw_world_edge, draw_world_map, flush_attracted_crab_glows,
    flush_beat_coronas, flush_catch_next_ticks, flush_centerpiece_dots, flush_hermit_coil_dots,
    flush_magnet_auras, unit_circle, unit_line, unit_square,
};
use crate::graphics::{
    draw_beam_hermit_match, draw_day_weather_hud, draw_lasso_magnet_match,
    draw_lasso_shell_deflect, draw_lasso_thief_match, draw_magnet_cluster_pull, draw_minimap,
    draw_stomp_armored_crack, draw_stomp_dancer_match, draw_tool_roster, draw_whistle_dancer_match,
    draw_whistle_golden_pull,
};
use crate::hud_cache::{
    CAREER_LABEL_CACHE, LOADOUT_PAGE_CACHE, MENU_BUTTONS_CACHE, MENU_SUBTITLE_CACHE,
    MENU_TITLE_CACHE, MENU_TITLE_CHARS_CACHE, PLAYER_NAME_CACHE,
};
use crate::levels::{TerrainKind, get_levels};
use crate::spawnings::{
    spawn_boss, spawn_enemies, spawn_hype_dancer, spawn_rhythm_boss, spawn_tide_boss,
    spawn_tutorial_crabs,
};
use crate::tutorial::{Tutorial, TutorialKind};
use crate::upgrade::{UPGRADE_FIRST_AT, UPGRADE_POOL, UpgradeId};
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
fn panic_snap_links(n: usize) -> usize {
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
    /// Kick off a punchy impact ring at the exact spot a crab was caught. Color-coded
    /// to the crab so different crab types read differently at a glance.
    fn spawn_catch_shockwave(&mut self, pos: Vec2, crab_color: [f32; 3]) {
        // Cap live shockwaves so a big beat-wave sweep can't unbound the vec.
        if self.catch_shockwaves.len() < 48 {
            self.catch_shockwaves.push((pos, 0.0, crab_color));
        }
    }

    /// The signature Hermit-crack moment: fired the frame a shelled Hermit is popped open by any of
    /// its three intended ecosystem verbs (Stomp / Dancer hop / charged Magnet rip). Unlike a plain
    /// Armored "SHELL CRACKED!" — which the beam can also wear down — cracking a Hermit is a pure
    /// archetype-web payoff (the beam can't touch it), so it earns its own watchable beat: the
    /// borrowed shell scatters as a coppery shard-burst, a warm copper shockwave, a "HERMIT POPPED!"
    /// callout, and a startle ring telegraphing the brief catch window as the defenceless crab bolts.
    fn spawn_hermit_pop(&mut self, pos: Vec2) {
        let mut rng = rand::rng();
        // The coppery shell-shard burst (same profile the catch uses) — the borrowed shell flying apart.
        self.particle_system.spawn_catch_effect(
            pos,
            [0.72, 0.44, 0.24],
            CrabType::Hermit,
            &mut rng,
        );
        // Warm copper shockwave — reads distinct from the cold blue Armored crack at a glance.
        self.spawn_catch_shockwave(pos, [0.85, 0.55, 0.28]);
        self.floating_texts.spawn(
            "HERMIT POPPED!".to_string(),
            pos - Vec2::new(66.0, 36.0),
            26.0,
            [0.95, 0.68, 0.38, 1.0], // coppery-orange so the "the ecosystem cracked it" story reads
        );
        // Startle ring telegraphs the short catch window: the popped Hermit is defenceless and bolts.
        if self.fear_rings.len() < 32 {
            self.fear_rings.push((pos, 0.0));
        }
    }

    /// Emergent stampede: the shock of a catch ripples outward and startles nearby *uncaught*
    /// crabs that aren't safely inside the flashlight beam, scattering them away from the catch
    /// point. Most noticeable when the trailing conga tail brushes through a distant cluster —
    /// nab one and the rest bolt. Keep your beam on the herd to hold them (the counterplay).
    fn emit_catch_startle(&mut self, origin: Vec2) {
        const STARTLE_RADIUS: f32 = 135.0;
        // Cold alarm ring so the scatter reads at a glance, distinct from the warm catch pop.
        if self.fear_rings.len() < 32 {
            self.fear_rings.push((origin, 0.0));
        }
        // Reused scratch buffer instead of a fresh Vec::new() on every single catch — a catch
        // that lands mid-herd is exactly the busiest moment for allocator churn to matter.
        let mut startled_pops = std::mem::take(&mut self.startled_pops_buf);
        startled_pops.clear();
        for crab in &mut self.crabs {
            if crab.caught || crab.in_flashlight {
                continue;
            }
            let dist = origin.distance(crab.pos);
            if dist >= STARTLE_RADIUS {
                continue;
            }
            let outward = (crab.pos - origin).normalize_or_zero();
            // Degenerate case: crab sits exactly on the origin — shove it in a stable direction.
            let outward = if outward == Vec2::ZERO {
                Vec2::new(0.0, -1.0)
            } else {
                outward
            };
            let prox = 1.0 - dist / STARTLE_RADIUS; // 1 at the epicenter, 0 at the rim
            let kick = crab.crab_type.speed_range().end * (1.3 + prox * 1.2);
            crab.vel = outward * kick;
            crab.speed = 1.0; // vel now encodes full speed, matching the flee branch's convention
            crab.startle_timer = 0.45;
            // Only pop a fresh "!" if it wasn't already panicking, so we don't spam text.
            if !crab.fleeing {
                startled_pops.push(crab.pos);
            }
        }
        for &pos in &startled_pops {
            self.floating_texts.spawn(
                "!".to_string(),
                pos - Vec2::new(0.0, 24.0),
                24.0,
                [0.6, 0.9, 1.0, 1.0],
            );
        }
        self.startled_pops_buf = startled_pops;
    }

    /// Emergent beat-startle chain reaction: on each beat, crabs that are already panicking
    /// (fleeing the player or mid-stampede) pass their fear to nearby *calm* crabs, so a scare
    /// ripples outward crab-to-crab across the herd on the pulse rather than every crab only ever
    /// reacting to the player directly. Carriers are snapshotted before infection, so the panic
    /// advances just one hop per beat — a visible marching wave, not an instant map-wide cascade.
    /// Self-limiting: only calm crabs can catch it (a crab already panicking isn't re-triggered),
    /// the startle bolt decays in ~one beat, and infections are capped per beat, so the wave dies
    /// down instead of locking the whole herd in permanent flight.
    ///
    /// Emergent crossover — the Golden Crab is a panic bomb: when the rare shiny prize is on the
    /// run its fear carries an amplified amplitude (`GOLDEN_PANIC_AMP`), reaching farther and kicking
    /// harder, and it *tags the crabs it infects as amplified carriers too*, so a fleeing Golden
    /// shatters a tight herd into a rolling stampede over the next few beats. This gives the
    /// chase-or-let-it-go decision real teeth: sprinting after the Golden through a packed crowd
    /// can scatter the very herd you were building.
    fn beat_startle_contagion(&mut self) {
        const CONTAGION_RADIUS: f32 = 110.0;
        const MAX_INFECTIONS_PER_BEAT: usize = 8;
        // How much harder a fleeing Golden crab's fear ripples than an ordinary panicking crab.
        const GOLDEN_PANIC_AMP: f32 = 1.6;
        // Snapshot of panicking crabs whose fear can jump to a neighbour this beat, into a
        // reused buffer instead of a fresh collect() every beat. Each carrier remembers a panic
        // amplitude so a Golden's amplified fear (and the amplified crabs it already startled)
        // keeps rippling harder than the baseline as the wave marches on.
        let mut carriers = std::mem::take(&mut self.contagion_carriers_buf);
        carriers.clear();
        carriers.extend(
            self.crabs
                .iter()
                .filter(|c| !c.caught && !c.is_boss() && (c.fleeing || c.startle_timer > 0.0))
                .map(|c| {
                    let amp = if c.is_golden() {
                        GOLDEN_PANIC_AMP
                    } else {
                        c.panic_amp.max(1.0)
                    };
                    (c.pos, amp)
                }),
        );
        if carriers.is_empty() {
            self.contagion_carriers_buf = carriers;
            return;
        }

        // Emergent crossover: free Armored crabs are calm anchors. A calm crab sheltering in the
        // shadow of an Armored shell shrugs off the panic ripple, so a herd salted with Armored
        // crabs settles instead of stampeding — and corralling a spooked crowd toward an Armored
        // crab becomes a real crowd-control play, the flipside of the Golden/Dancer chaos engines.
        // The Armored crab earns a role in the herd beyond "shell you have to crack".
        const SHELTER_RADIUS: f32 = 82.0;
        let mut anchors = std::mem::take(&mut self.armored_anchors_buf);
        anchors.clear();
        anchors.extend(
            self.crabs
                .iter()
                .filter(|c| !c.caught && !c.is_boss() && c.is_armored())
                .map(|c| c.pos),
        );

        // Bucket carriers into a spatial grid (same pattern as catch_by_chain and
        // deflect_fleeing_off_chain) so each calm crab only tests nearby carriers instead of the
        // whole panicking set — the herd has no size cap, so a flat scan here got slower the
        // longer a session ran and the bigger a stampede got, which is exactly when frame time
        // matters most for game feel.
        let cell_size = CONTAGION_RADIUS.max(1.0);
        let cell_of = |p: Vec2| -> (i32, i32) {
            (
                (p.x / cell_size).floor() as i32,
                (p.y / cell_size).floor() as i32,
            )
        };
        // Clear the whole map, not just each bucket's contents — keeping only the values cleared
        // let the key set (one entry per grid cell ever visited by a carrier) grow unbounded over
        // a long session as the herd wanders the full level, slowly bloating the hash table and
        // its load factor even though the actual per-beat working set stays tiny. A full clear()
        // still keeps the map's allocated capacity (same pooling win, no realloc most beats) but
        // resets the key count to "cells touched this beat" instead of "cells touched ever".
        self.contagion_grid_buf.clear();
        for (i, &(pos, _)) in carriers.iter().enumerate() {
            self.contagion_grid_buf
                .entry(cell_of(pos))
                .or_default()
                .push(i);
        }

        // Bucket anchors into the same grid pattern, so the shelter check below only tests
        // Armored crabs near this calm crab instead of every free Armored crab in the herd —
        // without this a session salted with several Armored crabs turned the shelter check
        // into a flat scan re-run per calm crab evaluated that beat.
        // Same unbounded-key fix as contagion_grid_buf above: clear the whole map (keeps its
        // capacity, resets its key count) instead of only clearing each bucket's Vec.
        let mut anchor_grid = std::mem::take(&mut self.armored_anchor_grid_buf);
        anchor_grid.clear();
        for (i, &pos) in anchors.iter().enumerate() {
            anchor_grid.entry(cell_of(pos)).or_default().push(i);
        }

        let mut infected_pops = std::mem::take(&mut self.contagion_pops_buf);
        infected_pops.clear();
        // Crabs an Armored anchor sheltered from the ripple this beat — drives a calm-puff cue.
        // Beat-gated (not per-frame), so a plain local Vec is fine, matching pried_by_magnet.
        let mut sheltered_pops: Vec<Vec2> = Vec::new();
        for crab in &mut self.crabs {
            if infected_pops.len() >= MAX_INFECTIONS_PER_BEAT {
                break;
            }
            // Only calm, catchable crabs outside the beam can be freshly infected.
            // A crab still soothed by a recent whistle pulse shrugs off the panic — this is what
            // makes the whistle a real crowd-control counter to a spreading stampede.
            if crab.caught
                || crab.is_boss()
                || crab.in_flashlight
                || crab.fleeing
                || crab.startle_timer > 0.0
                || crab.charm_timer > 0.0
            {
                continue;
            }
            // Nearest carrier within reach becomes the source the crab bolts away from,
            // restricted to the 3x3 neighbourhood of grid cells around the crab.
            // A Golden's amplified fear reaches beyond the baseline radius, so the closest carrier
            // is scored by how far its own reach extends, not just raw distance — an amplified
            // carrier can out-pull a nearer ordinary one and grab crabs an ordinary crab couldn't.
            let (cx, cy) = cell_of(crab.pos);
            let mut nearest: Option<(f32, Vec2, f32)> = None; // (reach-score, source pos, amp)
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(candidates) = self.contagion_grid_buf.get(&(cx + dx, cy + dy)) {
                        for &i in candidates {
                            let (source, amp) = carriers[i];
                            let d = source.distance(crab.pos);
                            let reach = CONTAGION_RADIUS * amp;
                            if d < reach {
                                // Lower score = stronger pull: normalize distance by the carrier's
                                // own reach so amplified carriers win ties within their bigger radius.
                                let score = d / amp;
                                if nearest.map_or(true, |(ns, _, _)| score < ns) {
                                    nearest = Some((score, source, amp));
                                }
                            }
                        }
                    }
                }
            }
            if let Some((score, source, amp)) = nearest {
                // Calm-anchor shelter: if an Armored crab is standing between this crab and the
                // rest of the herd, its shell settles the panic and the ripple stops here. An
                // amplified (Golden-driven) wave is only partly dampened — its fear is hot enough
                // to leak past a shell it's right on top of — so an Armored crab tames an ordinary
                // stampede outright but merely blunts a Golden panic bomb.
                let shelter_r = if amp > 1.05 {
                    SHELTER_RADIUS * 0.55
                } else {
                    SHELTER_RADIUS
                };
                // Shelter radius is always <= CONTAGION_RADIUS (the grid's cell size), so any
                // anchor within range is guaranteed to fall in the crab's own cell or one of its
                // 8 neighbours — the same 3x3 sweep used for carriers above.
                let sheltered = (-1..=1).any(|dx| {
                    (-1..=1).any(|dy| {
                        anchor_grid.get(&(cx + dx, cy + dy)).is_some_and(|bucket| {
                            bucket
                                .iter()
                                .any(|&i| anchors[i].distance(crab.pos) < shelter_r)
                        })
                    })
                });
                if sheltered {
                    // Sheltered: the crab shrugs the ripple off entirely. Deliberately leave its
                    // calm state untouched (no startle_timer bump) so it doesn't turn into a phantom
                    // carrier next beat and spread a panic it never actually felt.
                    sheltered_pops.push(crab.pos);
                    continue;
                }
                let outward = (crab.pos - source).normalize_or_zero();
                let outward = if outward == Vec2::ZERO {
                    Vec2::new(0.0, -1.0)
                } else {
                    outward
                };
                // score is d/amp in [0, CONTAGION_RADIUS); turn it back into a 1-at-source proximity.
                let prox = 1.0 - (score / CONTAGION_RADIUS).clamp(0.0, 1.0);
                let kick = crab.crab_type.speed_range().end * (1.1 + prox * 0.9) * amp;
                crab.vel = outward * kick;
                crab.speed = 1.0; // vel now encodes full speed, matching the flee/startle convention
                crab.startle_timer = 0.45;
                // Carry a decayed slice of the source's amplitude forward, so the Golden's panic
                // stays hotter than baseline for a couple more hops before fading to ordinary fear.
                crab.panic_amp = (1.0 + (amp - 1.0) * 0.7).max(1.0);
                infected_pops.push((crab.pos, amp > 1.05));
            }
        }
        // Alarm rings + "!" pops so the crab-to-crab ripple reads at a glance. Amplified
        // (Golden-driven) infections get a bigger, hot-gold "!" so a panic bomb looks like one.
        for &(pos, amplified) in &infected_pops {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((pos, 0.0));
            }
            let (size, color) = if amplified {
                (28.0, [1.0, 0.82, 0.24, 1.0])
            } else {
                (22.0, [0.6, 0.9, 1.0, 1.0])
            };
            self.floating_texts
                .spawn("!".to_string(), pos - Vec2::new(0.0, 24.0), size, color);
        }
        // Warm calming puffs off crabs an Armored anchor just sheltered — the same soothe cue the
        // whistle throws, so "the shell settled them" reads with the game's existing calm vocabulary
        // rather than needing a new effect. Capped so a big herd around an anchor doesn't spew.
        if !sheltered_pops.is_empty() {
            let mut rng = rand::rng();
            for pos in sheltered_pops.into_iter().take(6) {
                self.particle_system.spawn_soothe_puff(pos, &mut rng);
            }
        }
        self.contagion_carriers_buf = carriers;
        self.contagion_pops_buf = infected_pops;
        self.armored_anchors_buf = anchors;
        self.armored_anchor_grid_buf = anchor_grid;
    }

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

    /// Kelp snag: while the conga tail sits in a kelp patch, the fronds can catch and strip a link
    /// or two loose — the Neon Kelp Forest's take on chain-snap. Rolls probabilistically (dt-scaled
    /// so it's framerate-independent) and is gated by the shared chain-snap cooldown, so routing a
    /// long train through the weeds is a real risk to weigh rather than a guaranteed loss. Mirrors
    /// `snap_chain_on_panic`: only long trains are vulnerable, only the tail goes, never the head.
    fn snag_chain_on_kelp(&mut self, dt: f32) {
        const MIN_TRAIN_TO_SNAG: usize = 5;
        const SNAG_LINKS: usize = 2; // gentler than a panic snap — the weeds nibble, they don't tear
        const SNAG_COOLDOWN: f32 = 2.2;
        const SNAG_CHANCE_PER_SEC: f32 = 0.6; // expected snags/sec while the tail sits in kelp

        // Ease the telegraph tension DOWN by default; the danger checks below raise it back up when
        // the tail is actually exposed. Doing it here (a per-frame call) keeps the warning ring
        // fading out smoothly the instant the player routes clear.
        self.kelp_snag_warn = (self.kelp_snag_warn - dt * 1.6).max(0.0);

        if self.current_terrain() != TerrainKind::Kelp || self.chain_count < MIN_TRAIN_TO_SNAG {
            return;
        }

        // Only bite if the tail link is actually inside a kelp patch — route around and you're safe.
        // Reuses the tail position update_crabs already computed this frame instead of rescanning.
        let Some(tail_pos) = self.cached_tail_pos else {
            return;
        };
        // Only the biome's native kelp patches snag — trailing flood pools are Tide Boss water.
        let native_count = self.tide_pools.len().saturating_sub(self.boss_flood_pools);
        let tail_in_kelp = self.tide_pools[..native_count]
            .iter()
            .any(|(c, r)| tail_pos.distance_squared(*c) < *r * *r);
        if !tail_in_kelp {
            return;
        }

        // The tail IS exposed — ramp the telegraph up so the warning ring builds visibly. It fills
        // faster than it fades (above), so lingering in the weeds clearly escalates toward a snag.
        self.kelp_snag_warn = (self.kelp_snag_warn + dt * 2.4).min(1.0);

        // Still gate the actual bite on the shared cooldown — but only AFTER the telegraph has been
        // updated, so the warning keeps pulsing through the grace period between nibbles.
        if self.chain_snap_cooldown > 0.0 {
            return;
        }

        // Probabilistic per-frame roll scaled by dt so the risk is framerate-independent.
        if rand::random::<f32>() > SNAG_CHANCE_PER_SEC * dt {
            return;
        }

        let keep = self.chain_count.saturating_sub(SNAG_LINKS).max(1);
        let snapped = self.chain_count - keep;
        let mut snapped_positions: Vec<Vec2> = Vec::new();
        for crab in &mut self.crabs {
            let Some(ci) = crab.chain_index else { continue };
            if ci >= keep {
                crab.caught = false;
                crab.chain_index = None;
                crab.fleeing = true;
                crab.startle_timer = 0.6;
                let outward = (crab.pos - tail_pos).normalize_or_zero();
                let outward = if outward == Vec2::ZERO {
                    Vec2::new(0.0, 1.0)
                } else {
                    outward
                };
                crab.vel = outward * crab.crab_type.speed_range().end * 1.8;
                crab.speed = 1.0;
                snapped_positions.push(crab.pos);
            }
        }
        self.chain_count = keep;
        self.recompute_tail_run(); // the tail changed — rebuild the same-type run
        self.chain_snap_cooldown = SNAG_COOLDOWN;

        // Feedback: green weed-tinted pops on the stripped crabs and a SNAGGED! callout at the tail.
        for pos in &snapped_positions {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((*pos, 0.0));
            }
            self.floating_texts.spawn(
                "!".to_string(),
                *pos - Vec2::new(0.0, 24.0),
                24.0,
                [0.5, 1.0, 0.6, 1.0],
            );
        }
        self.floating_texts.spawn(
            format!("SNAGGED!  -{}", snapped),
            tail_pos - Vec2::new(30.0, 32.0),
            30.0,
            [0.5, 1.0, 0.6, 1.0],
        );
        self.spawn_catch_shockwave(tail_pos, [0.4, 0.95, 0.5]);
        self.screen_shake = self.screen_shake.max(5.0);
    }

    /// Chain-as-risk: the trailing end of the conga train is exposed and can be knocked loose.
    /// Once the train is long enough to matter, a panicking wild crab (fleeing the beam or
    /// mid-stampede) that barrels into the tail snaps the last few links free — they revert to the
    /// wild and scatter outward. This flips the central mechanic from a pure-upside growing counter
    /// into a moment-to-moment decision: a long conga line is now a bigger, more exposed target you
    /// have to route around spooked herds and actively protect, and can lose the end of.
    /// Self-limiting: short trains are immune, only the tail chunk goes (never the head), and a
    /// cooldown means one brush can't strip the whole train in a single pass.
    fn snap_chain_on_panic(&mut self) {
        const MIN_TRAIN_TO_SNAP: usize = 5; // short trains are safe — the risk only bites once you've invested
        const SNAP_COLLIDE_DIST: f32 = CRAB_SIZE * 0.9;
        const SNAP_COOLDOWN: f32 = 1.6; // grace period so a herd can't strip everything at once

        if self.chain_snap_cooldown > 0.0 || self.chain_count < MIN_TRAIN_TO_SNAP {
            return;
        }
        // The vulnerable end is the most-recently-caught crab (highest chain_index sits at the tail).
        // Reuses the tail position update_crabs already computed this frame instead of rescanning.
        let Some(tail_pos) = self.cached_tail_pos else {
            return;
        };
        // Did a panicking wild crab — or a King Crab mid-lunge — just slam into the tail?
        let hit = self.crabs.iter().any(|c| {
            if c.caught {
                return false;
            }
            if c.is_boss() {
                // A charging King Crab plows through the tail; its bulk gives it a wider reach.
                let boss_reach = SNAP_COLLIDE_DIST + c.scale * CRAB_SIZE * 0.5;
                matches!(c.charge_state, BossCharge::Charging(_))
                    && c.pos.distance_squared(tail_pos) < boss_reach * boss_reach
            } else {
                (c.fleeing || c.startle_timer > 0.0)
                    && c.pos.distance_squared(tail_pos) < SNAP_COLLIDE_DIST * SNAP_COLLIDE_DIST
            }
        });
        if !hit {
            return;
        }

        // Release the tail links — count scales with train length (longer = a bigger, pricier
        // bite), always leaving at least the head crab attached.
        let keep = self
            .chain_count
            .saturating_sub(panic_snap_links(self.chain_count))
            .max(1);
        let snapped = self.chain_count - keep;
        let mut snapped_positions: Vec<Vec2> = Vec::new();
        for crab in &mut self.crabs {
            let Some(ci) = crab.chain_index else { continue };
            if ci >= keep {
                // Revert to the wild and bolt outward from the tail so the break reads clearly.
                crab.caught = false;
                crab.chain_index = None;
                crab.fleeing = true;
                crab.startle_timer = 0.6;
                let outward = (crab.pos - tail_pos).normalize_or_zero();
                let outward = if outward == Vec2::ZERO {
                    Vec2::new(0.0, 1.0)
                } else {
                    outward
                };
                crab.vel = outward * crab.crab_type.speed_range().end * 2.2;
                crab.speed = 1.0; // vel now encodes full speed, matching the flee/startle convention
                snapped_positions.push(crab.pos);
            }
        }
        // Indices 0..keep stay contiguous, so the shortened train and future catches line up cleanly.
        self.chain_count = keep;
        self.recompute_tail_run(); // the tail changed — rebuild the same-type run
        self.chain_snap_cooldown = SNAP_COOLDOWN;

        // Feedback: cold alarm rings + "!" pops on the scattering crabs, a SNAP! callout, and a jolt.
        for pos in &snapped_positions {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((*pos, 0.0));
            }
            self.floating_texts.spawn(
                "!".to_string(),
                *pos - Vec2::new(0.0, 24.0),
                24.0,
                [1.0, 0.5, 0.4, 1.0],
            );
        }
        self.floating_texts.spawn(
            format!("SNAP!  -{}", snapped),
            tail_pos - Vec2::new(24.0, 32.0),
            32.0,
            [1.0, 0.4, 0.3, 1.0],
        );
        self.spawn_catch_shockwave(tail_pos, [1.0, 0.4, 0.3]);
        self.screen_shake = self.screen_shake.max(9.0);
        let kick_angle = rand::rng().random_range(0.0_f32..std::f32::consts::TAU);
        self.screen_shake_vel = Vec2::new(kick_angle.cos(), kick_angle.sin()) * 9.0 * 60.0;
    }

    /// King Crab splice: when a charging boss CROSSES the player's conga line (passes through any
    /// chain segment, not just the tail), it splices the chain at that point. Everything behind the
    /// crossing (higher chain_index) detaches and magnetically flies toward the boss — stolen.
    ///
    /// This is the reverse-Snake + Agar.io core mechanic: the King Crab routes deliberately through
    /// your line to steal crabs, making the chain a high-stakes spatial puzzle to protect.
    fn check_king_crab_splice(&mut self) {
        const SPLICE_COLLIDE_DIST: f32 = CRAB_SIZE * 1.4;
        const SPLICE_COOLDOWN: f32 = 2.0;
        const MAGNET_DURATION: f32 = 0.9;

        if self.king_splice_cooldown > 0.0 || self.chain_count < 2 {
            return;
        }

        let boss_pos: Option<Vec2> = self.crabs.iter().find_map(|c| {
            if c.is_boss() && !c.caught && !c.is_tide_boss() && !c.is_rhythm_boss() {
                if matches!(c.charge_state, crate::enemies::BossCharge::Charging(_)) {
                    Some(c.pos)
                } else {
                    None
                }
            } else {
                None
            }
        });
        let Some(boss_pos) = boss_pos else {
            return;
        };

        let splice_at: Option<usize> = {
            let mut best: Option<usize> = None;
            let d2_thresh = SPLICE_COLLIDE_DIST * SPLICE_COLLIDE_DIST;
            for c in &self.crabs {
                let Some(ci) = c.chain_index else { continue };
                if boss_pos.distance_squared(c.pos) < d2_thresh {
                    if best.map_or(true, |b| ci < b) {
                        best = Some(ci);
                    }
                }
            }
            best
        };
        let Some(cut_ci) = splice_at else {
            return;
        };

        let mut stolen: Vec<(Vec2, [f32; 4])> = Vec::new();
        for c in &self.crabs {
            if c.caught && c.chain_index.map_or(false, |ci| ci >= cut_ci) {
                let [r, g, b] = c.crab_color();
                stolen.push((c.pos, [r, g, b, 1.0]));
            }
        }
        if stolen.is_empty() {
            return;
        }
        let stolen_count = stolen.len();

        for c in &mut self.crabs {
            if c.caught && c.chain_index.map_or(false, |ci| ci >= cut_ci) {
                c.caught = false;
                c.chain_index = None;
                c.vel = Vec2::ZERO;
            }
        }
        self.chain_count = cut_ci;
        self.recompute_tail_run();
        self.king_splice_cooldown = SPLICE_COOLDOWN;

        for (pos, color) in &stolen {
            self.king_stolen_crabs.push((*pos, MAGNET_DURATION, *color));
        }

        let cut_pos = stolen.first().map(|(p, _)| *p).unwrap_or(boss_pos);
        self.spawn_catch_shockwave(cut_pos, [1.0, 0.2, 0.8]);
        self.screen_shake = self.screen_shake.max(10.0);
        let kick_angle = rand::rng().random_range(0.0_f32..std::f32::consts::TAU);
        self.screen_shake_vel = Vec2::new(kick_angle.cos(), kick_angle.sin()) * 10.0 * 60.0;
        self.floating_texts.spawn(
            format!("STOLEN! -{}", stolen_count),
            cut_pos - Vec2::new(40.0, 40.0),
            36.0,
            [1.0, 0.3, 0.9, 1.0],
        );
        for (pos, _) in &stolen {
            if self.fear_rings.len() < 48 {
                self.fear_rings.push((*pos, 0.0));
            }
            self.floating_texts.spawn(
                "!".to_string(),
                *pos - Vec2::new(0.0, 20.0),
                20.0,
                [1.0, 0.4, 0.9, 1.0],
            );
        }
    }

    /// Rebuild `tail_run_len` — the length of the unbroken run of same-type links at the tail — by
    /// walking backward from the current tail. Called after any peel/snap shrinks the train, since
    /// removing tail links can change what the tail is (and thus the run). A no-op-cheap O(n) scan
    /// that only runs on the rare frames a link is actually lost, not every frame. An empty train
    /// has a run of 0.
    fn recompute_tail_run(&mut self) {
        if self.chain_count == 0 {
            self.tail_run_len = 0;
            return;
        }
        // Build a chain_index → CrabType lookup in one O(n) pass so we don't scan self.crabs
        // once per position from the tail toward the head (the old approach was O(run × chain)
        // in the worst case when the whole train is one archetype).
        // Indices are 0..chain_count and contiguous by invariant; the Vec is sized exactly.
        let mut by_index: Vec<Option<CrabType>> = vec![None; self.chain_count];
        for c in &self.crabs {
            if let Some(ci) = c.chain_index {
                if ci < by_index.len() {
                    by_index[ci] = Some(c.crab_type);
                }
            }
        }
        let tail_ci = self.chain_count - 1;
        let Some(tail_type) = by_index[tail_ci] else {
            self.tail_run_len = 0;
            return;
        };
        let mut run = 1u32;
        let mut ci = tail_ci;
        while ci > 0 {
            ci -= 1;
            if by_index[ci] == Some(tail_type) {
                run += 1;
            } else {
                break;
            }
        }
        self.tail_run_len = run;
    }

    /// Thief archetype: a skittish parasite that pressures the *train you've already built* rather
    /// than the herd you're chasing. A free Thief ignores the flee/attract logic and beelines for
    /// your conga tail (its homing is done in update_crabs). Once it reaches the tail it *latches*
    /// on and, on a repeating timer, peels the trailing link loose — that crab reverts to the wild
    /// and bolts, and the Thief keeps gnawing the new tail until you deal with it. Counterplay:
    /// catch the Thief (beam/lasso/chain), whistle it off (whistle_pull is high for Thieves), or
    /// stomp near it — any of those clears the latch. Self-limiting like the other tail risks:
    /// short trains are immune, only the tail goes, never the head, and it shares the chain-snap
    /// cooldown so it can't strip everything in one beat.
    fn steal_chain_thief(&mut self, dt: f32) {
        const MIN_TRAIN_TO_STEAL: usize = 4; // a little shorter than snap — the Thief is a dedicated threat
        const LATCH_DIST: f32 = CRAB_SIZE * 1.1; // how close a Thief must get to the tail to clamp on
        const UNLATCH_DIST: f32 = CRAB_SIZE * 2.4; // if the tail pulls this far away, the clamp breaks
        const LATCH_DIST_SQ: f32 = LATCH_DIST * LATCH_DIST;
        const UNLATCH_DIST_SQ: f32 = UNLATCH_DIST * UNLATCH_DIST;
        const PEEL_INTERVAL: f32 = 1.15; // seconds between links peeled while latched

        // Where's the current tail? (highest chain_index). If the train is too short, no Thief can
        // latch, and any that were latched should let go.
        if self.chain_count < MIN_TRAIN_TO_STEAL {
            for c in &mut self.crabs {
                if c.is_thief() {
                    c.latch_timer = 0.0;
                }
            }
            return;
        }
        // Reuses the tail position update_crabs already computed this frame (same "highest
        // chain_index among caught crabs" lookup) instead of a third O(n) scan over self.crabs.
        let Some(tail_pos) = self.cached_tail_pos else {
            return;
        };

        // Emergent crossover: a roaming Magnet's pull reaches a latched Thief too, and it's
        // stronger than the Thief's grip on your tail. If a clamped Thief drifts inside a free
        // Magnet's radius, the Magnet wins the tug-of-war and rips the parasite clean off the
        // train — the crab you were cursing for gathering a blob becomes an accidental savior.
        // magnet_positions_buf was filled this same frame by update_crabs (runs before us) and
        // holds only *free* Magnets, so a caught Magnet in your own train never triggers this.
        const MAGNET_PRY_RADIUS: f32 = 190.0; // a touch shorter than the herd pull — it has to get close to pry
        const MAGNET_PRY_RADIUS_SQ: f32 = MAGNET_PRY_RADIUS * MAGNET_PRY_RADIUS;
        // Borrow the free-Magnet positions out of self so the &mut self.crabs loop below can call
        // the lookup without an overlapping self borrow; restored at the end of the function.
        let magnet_positions = std::mem::take(&mut self.magnet_positions_buf);
        let nearest_magnet_to = |p: Vec2| -> Option<Vec2> {
            let mut best: Option<(f32, Vec2)> = None;
            for &mp in magnet_positions.iter() {
                let d2 = p.distance_squared(mp);
                if d2 < MAGNET_PRY_RADIUS_SQ && best.map_or(true, |(bd2, _)| d2 < bd2) {
                    best = Some((d2, mp));
                }
            }
            best.map(|(_, mp)| mp)
        };

        // Emergent crossover: a fleeing Golden's panic scares a latched Thief clean off your tail.
        // The Golden's amplified fear (the same GOLDEN_PANIC_AMP-hot ripple that shatters a herd into
        // a stampede) is contagious to the skittish parasite too — a Golden bolting past your train
        // spooks the Thief into bolting itself, letting go of the tail. This is the panic-native
        // mirror of the Magnet-pry save above: there a lodestone rips the Thief off, here a passing
        // prize's fright does it. Only *amplified* carriers (a fleeing Golden, or an ordinary crab
        // still carrying a Golden's hot panic_amp) can do it — a plain panicking crab isn't scary
        // enough to a Thief that's busy raiding. Snapshotted before the &mut self.crabs loop below so
        // the lookup has no overlapping borrow; almost always an empty scan (no Golden mid-flee).
        const GOLDEN_SPOOK_RADIUS: f32 = 130.0;
        const GOLDEN_SPOOK_RADIUS_SQ: f32 = GOLDEN_SPOOK_RADIUS * GOLDEN_SPOOK_RADIUS;
        let mut golden_panic_positions = std::mem::take(&mut self.golden_panic_positions_buf);
        golden_panic_positions.clear();
        golden_panic_positions.extend(self.crabs.iter().filter_map(|c| {
            (!c.caught
                && !c.is_boss()
                && (c.fleeing || c.startle_timer > 0.0)
                && (c.is_golden() || c.panic_amp > 1.05))
                .then_some(c.pos)
        }));
        let nearest_golden_panic_to = |p: Vec2| -> Option<Vec2> {
            let mut best: Option<(f32, Vec2)> = None;
            for &gp in golden_panic_positions.iter() {
                let d2 = p.distance_squared(gp);
                if d2 < GOLDEN_SPOOK_RADIUS_SQ && best.map_or(true, |(bd2, _)| d2 < bd2) {
                    best = Some((d2, gp));
                }
            }
            best.map(|(_, gp)| gp)
        };

        // Emergent crossover: a passing Golden's shine lures a *latched* Thief off your tail. The
        // Golden-lures-Thief pull already diverts a *homing* raider mid-beeline (see update_crabs),
        // but a thief this greedy can't resist a shiny thing even once it's clamped on and gnawing:
        // if a free Golden bolts near a Thief that's already raiding your train, its greed overpowers
        // its grip and it drops the link it was stealing to chase the bigger prize. A third, distinct
        // flavor of latched-Thief save from the two above — the Magnet pry is a physical drag (hauled
        // in), the Golden-panic spook is fright (flees off), and this is pure *greed* (chases away
        // toward the shine, thief_lured aura and all). Softer than both, so it only fires when neither
        // a Magnet nor a fleeing Golden's panic already grabbed the Thief this frame. Reuses the
        // golden_lure_positions_buf snapshot update_crabs already built this frame (free, un-snared
        // Goldens) — no new scan. Almost always an empty check (a free Golden near a raided train is
        // rare), so it costs nothing most frames.
        const GOLDEN_LURE_LATCH_RADIUS: f32 = 220.0;
        const GOLDEN_LURE_LATCH_RADIUS_SQ: f32 =
            GOLDEN_LURE_LATCH_RADIUS * GOLDEN_LURE_LATCH_RADIUS;
        let golden_lure_positions = std::mem::take(&mut self.golden_lure_positions_buf);
        let nearest_golden_lure_to = |p: Vec2| -> Option<Vec2> {
            let mut best: Option<(f32, Vec2)> = None;
            for &gp in golden_lure_positions.iter() {
                let d2 = p.distance_squared(gp);
                if d2 < GOLDEN_LURE_LATCH_RADIUS_SQ && best.map_or(true, |(bd2, _)| d2 < bd2) {
                    best = Some((d2, gp));
                }
            }
            best.map(|(_, gp)| gp)
        };

        // Advance every Thief's latch state; collect whether any peel fired this frame, plus any
        // Thieves a Magnet pried loose, a Golden's panic spooked loose, or a Golden's shine lured
        // off (deferred out of the &mut loop for their freed feedback).
        let mut peel_from: Option<Vec2> = None;
        // Reused scratch buffers (almost always empty — a save firing is rare) instead of three
        // fresh Vec::new() allocations every single frame this unconditionally-run function pays.
        let mut pried_by_magnet = std::mem::take(&mut self.pried_by_magnet_buf);
        pried_by_magnet.clear();
        let mut spooked_by_golden = std::mem::take(&mut self.spooked_by_golden_buf);
        spooked_by_golden.clear();
        let mut lured_by_golden = std::mem::take(&mut self.lured_by_golden_buf);
        lured_by_golden.clear();
        for c in &mut self.crabs {
            if !c.is_thief() || c.caught {
                if c.is_thief() {
                    c.latch_timer = 0.0; // caught Thieves stop stealing
                }
                continue;
            }
            let d_sq = c.pos.distance_squared(tail_pos);
            if c.latch_timer > 0.0 {
                // A nearby Magnet overpowers the clamp: the Thief lets go of the tail and is
                // flung toward the Magnet, joining the loose herd instead of peeling your links.
                if let Some(mp) = nearest_magnet_to(c.pos) {
                    c.latch_timer = 0.0;
                    let dir = (mp - c.pos).normalize_or_zero();
                    let dir = if dir == Vec2::ZERO {
                        Vec2::new(0.0, -1.0)
                    } else {
                        dir
                    };
                    c.vel = dir * c.crab_type.speed_range().end * 1.5;
                    c.speed = 1.0;
                    c.fleeing = false;
                    c.startle_timer = 0.0;
                    pried_by_magnet.push(c.pos);
                    continue;
                }
                // A fleeing Golden's panic washes over the clamped Thief: it spooks and bolts away
                // from the fright, letting go of your tail. It flees the panic source instead of
                // being hauled toward a Magnet, so the crab scatters off into the herd rather than
                // getting balled up — a looser, chaos-flavored save than the Magnet pry.
                if let Some(gp) = nearest_golden_panic_to(c.pos) {
                    c.latch_timer = 0.0;
                    let dir = (c.pos - gp).normalize_or_zero();
                    let dir = if dir == Vec2::ZERO {
                        Vec2::new(0.0, -1.0)
                    } else {
                        dir
                    };
                    c.vel = dir * c.crab_type.speed_range().end * 1.4;
                    c.speed = 1.0;
                    c.fleeing = true;
                    c.startle_timer = 0.5;
                    spooked_by_golden.push(c.pos);
                    continue;
                }
                // A free Golden's shine catches the raiding Thief's eye: greed wins over grip, so it
                // unclamps and darts off toward the prize instead of peeling your links. Unlike the
                // fright spook above it isn't fleeing — it *chases* the shine, so it heads toward the
                // Golden with the same thief_lured gold aura the homing-lure crossover uses. Yields to
                // the Magnet pry and the panic spook (checked first), which are harder pulls.
                if let Some(gp) = nearest_golden_lure_to(c.pos) {
                    c.latch_timer = 0.0;
                    let dir = (gp - c.pos).normalize_or_zero();
                    let dir = if dir == Vec2::ZERO {
                        Vec2::new(0.0, -1.0)
                    } else {
                        dir
                    };
                    c.vel = dir * c.crab_type.speed_range().end * 1.3;
                    c.speed = 1.0;
                    c.fleeing = false;
                    c.startle_timer = 0.0;
                    c.thief_lured = 0.3; // light the gold "chasing shine" aura
                    lured_by_golden.push(c.pos);
                    continue;
                }
                // Already clamped. Ride the tail so it visually hangs off the back of the train.
                if d_sq > UNLATCH_DIST_SQ {
                    c.latch_timer = 0.0; // the train outran it — it drops off
                    continue;
                }
                c.pos = c.pos.lerp(tail_pos, 0.35); // cling to the tail
                c.vel = Vec2::ZERO;
                c.latch_timer -= dt;
                if c.latch_timer <= 0.0 {
                    // Timer fired — this Thief peels a link. Only the first Thief to fire this
                    // frame actually pulls one (peel_from records it); any others just rearm, so a
                    // cluster of Thieves can't strip several links in a single frame.
                    if peel_from.is_none() {
                        peel_from = Some(tail_pos);
                    }
                    c.latch_timer = PEEL_INTERVAL; // rearm for the next peel
                }
            } else if d_sq < LATCH_DIST_SQ {
                // Just reached the tail — clamp on. First peel comes after a full interval so the
                // player gets a beat to react to the latch before losing a link.
                c.latch_timer = PEEL_INTERVAL;
            }
        }
        // The closures (and their borrows of the taken buffers) are done after the loop above, so
        // hand both buffers back to self for next frame's reuse instead of dropping them.
        self.magnet_positions_buf = magnet_positions;
        self.golden_panic_positions_buf = golden_panic_positions;
        self.golden_lure_positions_buf = golden_lure_positions;

        // Feedback for any Thief a Magnet just pried off your tail — a bright orange-green pop and
        // a callout so the save reads as a moment, not a silent stat change. Orange (the Magnet's
        // color) bleeding into thief-green sells the "the Magnet did this" story.
        for pos in pried_by_magnet.drain(..) {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((pos, 0.0));
            }
            self.floating_texts.spawn(
                "MAGNET PRY!".to_string(),
                pos - Vec2::new(52.0, 30.0),
                24.0,
                [0.95, 0.7, 0.3, 1.0],
            );
            self.spawn_catch_shockwave(pos, [0.9, 0.55, 0.25]);
        }

        // Feedback for any Thief a Golden's panic just scared off your tail — a hot-gold fright pop
        // and a callout, so the accidental save reads as a moment. Gold (the prize's color) bleeding
        // into the fright sells the "a passing Golden spooked it loose" story, and distinguishes it
        // from the orange Magnet pry above.
        for pos in spooked_by_golden.drain(..) {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((pos, 0.0));
            }
            self.floating_texts.spawn(
                "SPOOKED OFF!".to_string(),
                pos - Vec2::new(54.0, 30.0),
                24.0,
                [1.0, 0.85, 0.3, 1.0],
            );
            self.spawn_catch_shockwave(pos, [1.0, 0.8, 0.25]);
        }

        // Feedback for any Thief a Golden's shine just lured off your tail — a poison-green "SHINY!"
        // pop matching the homing-lure crossover's cue, so the "it dropped the raid to chase gold"
        // story reads the same whether the Thief was homing or already clamped on. Distinct from the
        // gold "SPOOKED OFF!" fright pop above: this one is greed, not fear.
        for pos in lured_by_golden.drain(..) {
            self.floating_texts.spawn(
                "SHINY!".to_string(),
                pos - Vec2::new(0.0, 30.0),
                22.0,
                [0.7, 0.95, 0.4, 1.0], // Thief's poison-green catching the golden gleam
            );
        }
        // Drained (so empty) either way — hand back for next frame's reuse before any early return.
        self.pried_by_magnet_buf = pried_by_magnet;
        self.spooked_by_golden_buf = spooked_by_golden;
        self.lured_by_golden_buf = lured_by_golden;

        let Some(tail_pos) = peel_from else { return };
        if self.chain_snap_cooldown > 0.0 {
            return; // respect the shared grace period, but the timer already rearmed above
        }

        // Emergent crossover — an Armored crab at the tail is a shell-plated tail-guard. The same
        // stubborn shell that walls off panic ripples and stops a King Crab charge also refuses to
        // be peeled: if the trailing link the Thief is trying to strip is an Armored crab, its shell
        // clangs and the steal is denied outright (the Thief keeps nibbling, but wastes this peel).
        // So deliberately routing an Armored crab to the *back* of your train — where the snap/steal
        // weak point is — turns it into a raid guard, the chain-pressure mirror of parking an Armored
        // crab in a boss's charge lane. Cheap: one scan for the single highest-chain_index crab,
        // only when a peel actually fired this frame.
        let tail_link = self.chain_count.checked_sub(1);
        if let Some(tail_ci) = tail_link {
            let tail_is_armored = self
                .crabs
                .iter()
                .any(|c| c.chain_index == Some(tail_ci) && c.is_armored());
            if tail_is_armored {
                // Shell holds — no link lost. Clang feedback so the save reads as a moment.
                self.chain_snap_cooldown = 0.9; // brief grace before the Thief tries again
                if self.fear_rings.len() < 32 {
                    self.fear_rings.push((tail_pos, 0.0));
                }
                self.floating_texts.spawn(
                    "SHELL HOLDS!".to_string(),
                    tail_pos - Vec2::new(46.0, 30.0),
                    26.0,
                    [0.75, 0.85, 1.0, 1.0],
                );
                self.spawn_catch_shockwave(tail_pos, [0.7, 0.8, 0.95]);
                self.screen_shake = self.screen_shake.max(4.0);
                return;
            }
        }

        // Peel the single trailing link loose — always leave the head attached.
        let keep = self.chain_count.saturating_sub(1).max(1);
        if keep >= self.chain_count {
            return;
        }
        for crab in &mut self.crabs {
            let Some(ci) = crab.chain_index else { continue };
            if ci >= keep {
                crab.caught = false;
                crab.chain_index = None;
                crab.fleeing = true;
                crab.startle_timer = 0.5;
                let outward = (crab.pos - tail_pos).normalize_or_zero();
                let outward = if outward == Vec2::ZERO {
                    Vec2::new(0.0, 1.0)
                } else {
                    outward
                };
                crab.vel = outward * crab.crab_type.speed_range().end * 1.8;
                crab.speed = 1.0;
            }
        }
        self.chain_count = keep;
        self.recompute_tail_run(); // the tail changed — rebuild the same-type run
        self.chain_snap_cooldown = 0.9; // shorter than a panic snap: the Thief keeps nibbling

        // Feedback: a sly green pop and a STOLEN! callout at the tail so the theft reads clearly.
        if self.fear_rings.len() < 32 {
            self.fear_rings.push((tail_pos, 0.0));
        }
        self.floating_texts.spawn(
            "STOLEN! -1".to_string(),
            tail_pos - Vec2::new(28.0, 30.0),
            28.0,
            [0.4, 0.95, 0.5, 1.0],
        );
        self.spawn_catch_shockwave(tail_pos, [0.35, 0.9, 0.45]);
        self.screen_shake = self.screen_shake.max(5.0);
    }

    /// Emergent herding: the solid *body* of the conga train physically deflects panicking wild
    /// crabs, bouncing them off instead of letting them phase through. Slide your line between a
    /// spooked herd and open water and you can corral fleeing crabs back toward your beam for a
    /// free re-catch — turning the train from a number-you-only-grow into a steerable wall you
    /// play the herd against. Mirror of chain-snap: the exposed tail (the same last few links
    /// snap can knock loose) is deliberately *not* a wall, so panic still slips past there. A long
    /// train is a shield up front and a weak point at the back. A charging King Crab bulldozes
    /// through regardless.
    fn deflect_fleeing_off_chain(&mut self) {
        const DEFLECT_DIST: f32 = CRAB_SIZE * 0.85;
        // Only trains long enough to have a snap-vulnerable tail keep that tail soft; shorter
        // trains have no exposed end yet, so their whole body walls.
        let tail_guard = if self.chain_count >= 5 { 3 } else { 0 };
        let body_max = self.chain_count.saturating_sub(tail_guard); // chain_index < body_max = solid wall

        // Gather the solid body segments once into a reused buffer (no per-frame heap churn).
        self.deflect_body_buf.clear();
        for crab in &self.crabs {
            if let Some(ci) = crab.chain_index {
                if ci < body_max {
                    self.deflect_body_buf.push(crab.pos);
                }
            }
        }
        if self.deflect_body_buf.is_empty() {
            return;
        }

        // Bucket body segments into a spatial grid keyed by cell (mirrors catch_by_chain's
        // grid) so each fleeing crab only tests the handful of segments near it instead of
        // scanning the whole chain. Chain length is uncapped and fleeing is common (any wild
        // crab near the player but outside the beam panics), so the old linear scan was an
        // O(fleeing * chain_length) cost that grew for the rest of a long session.
        let cell_size = DEFLECT_DIST.max(1.0);
        let cell_of = |p: Vec2| -> (i32, i32) {
            (
                (p.x / cell_size).floor() as i32,
                (p.y / cell_size).floor() as i32,
            )
        };
        // Same unbounded-key fix as catch_grid_buf: full map clear keeps capacity but bounds
        // iteration to "cells touched this frame", not "cells ever touched over the session".
        self.deflect_grid_buf.clear();
        for (i, &seg) in self.deflect_body_buf.iter().enumerate() {
            self.deflect_grid_buf
                .entry(cell_of(seg))
                .or_default()
                .push(i);
        }

        self.deflect_bounce_buf.clear();
        self.deflect_ricochet_buf.clear();
        let mut rng = rand::rng();
        for (idx, crab) in self.crabs.iter_mut().enumerate() {
            if crab.caught || crab.is_boss() {
                continue;
            }
            if !(crab.fleeing || crab.startle_timer > 0.0) {
                continue;
            }
            // Nearest body segment within collision range, restricted to the 3x3 neighborhood
            // of grid cells around the crab instead of every segment in the chain.
            let (cx, cy) = cell_of(crab.pos);
            let mut hit: Option<(f32, Vec2)> = None;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(candidates) = self.deflect_grid_buf.get(&(cx + dx, cy + dy)) {
                        for &i in candidates {
                            let seg = self.deflect_body_buf[i];
                            let d = seg.distance(crab.pos);
                            if d < DEFLECT_DIST && hit.map_or(true, |(hd, _)| d < hd) {
                                hit = Some((d, seg));
                            }
                        }
                    }
                }
            }
            let Some((_, seg)) = hit else { continue };
            let mut n = (crab.pos - seg).normalize_or_zero();
            if n == Vec2::ZERO {
                n = Vec2::new(0.0, -1.0);
            }
            // Reflect its velocity off the wall only if it's actually heading into the segment,
            // bleeding a little energy so it doesn't ping-pong forever.
            let into = crab.vel.dot(n);
            if into < 0.0 {
                crab.vel = (crab.vel - n * (2.0 * into)) * 0.9;
                crab.speed = 1.0; // vel encodes full speed, matching the flee/startle convention
            }
            // Shove it back out of the wall so it can't tunnel through, and keep it lively.
            crab.pos = seg + n * DEFLECT_DIST;
            crab.startle_timer = crab.startle_timer.max(0.2);
            // Throttled cold ring so the wall-bounce reads without flooding the screen.
            if rng.random::<f32>() < 0.25 {
                self.deflect_bounce_buf.push(crab.pos);
            }
            // Remember it so the ricochet pass below can crash it into other deflected crabs
            // funneled into the same pocket of the wall.
            self.deflect_ricochet_buf.push((idx, crab.pos));
        }
        for &pos in &self.deflect_bounce_buf {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((pos, 0.0));
            }
        }

        // Emergent pile-up: the wall funnels a panicking crowd into its concave pockets, where the
        // crabs it just deflected collide with *each other*. Resolve those pairwise: crabs that
        // overlap ricochet apart and cross-startle, so driving your train into a fleeing herd sets
        // off a self-feeding pinball cascade instead of every crab bouncing off the wall in
        // isolation. Cheap because it only considers crabs deflected *this* frame (usually a
        // handful), bucketed into a grid so each tests just its neighbors.
        self.ricochet_deflected_crabs();
    }

    /// Second half of `deflect_fleeing_off_chain`: crash the crabs the wall just deflected into
    /// each other. Only the small set collected in `deflect_ricochet_buf` participates, so this is
    /// a tiny pass even in a dense herd. Pairs that overlap are pushed apart, have their velocities
    /// swapped along the collision axis (an elastic bounce), and are both freshly startled — the
    /// emergent "the herd panics itself against your train" moment.
    fn ricochet_deflected_crabs(&mut self) {
        const COLLIDE_DIST: f32 = CRAB_SIZE * 0.7;
        if self.deflect_ricochet_buf.len() < 2 {
            return;
        }
        let cell_size = COLLIDE_DIST.max(1.0);
        let cell_of = |p: Vec2| -> (i32, i32) {
            (
                (p.x / cell_size).floor() as i32,
                (p.y / cell_size).floor() as i32,
            )
        };
        // Same unbounded-key fix as the other two grids above.
        self.deflect_ricochet_grid_buf.clear();
        for (bi, &(_, pos)) in self.deflect_ricochet_buf.iter().enumerate() {
            self.deflect_ricochet_grid_buf
                .entry(cell_of(pos))
                .or_default()
                .push(bi);
        }

        self.deflect_collide_buf.clear();
        // Collect the resolved (crab_index, new_pos, new_vel) then apply, so we never hold two
        // mutable borrows into self.crabs at once. Reuses a scratch buffer to avoid per-frame churn.
        let mut resolutions = std::mem::take(&mut self.deflect_resolve_buf);
        resolutions.clear();
        let n = self.deflect_ricochet_buf.len();
        for a in 0..n {
            let (ci_a, pos_a) = self.deflect_ricochet_buf[a];
            let (cx, cy) = cell_of(pos_a);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(candidates) =
                        self.deflect_ricochet_grid_buf.get(&(cx + dx, cy + dy))
                    {
                        for &b in candidates {
                            if b <= a {
                                continue; // resolve each unordered pair once
                            }
                            let (ci_b, pos_b) = self.deflect_ricochet_buf[b];
                            let delta = pos_b - pos_a;
                            let d = delta.length();
                            if d >= COLLIDE_DIST || d <= 0.0001 {
                                continue;
                            }
                            let axis = delta / d;
                            let overlap = COLLIDE_DIST - d;
                            // Read velocities, swap the component along the collision axis (equal-mass
                            // elastic bounce), and separate the pair so they don't stick.
                            let va = self.crabs[ci_a].vel;
                            let vb = self.crabs[ci_b].vel;
                            let van = va.dot(axis);
                            let vbn = vb.dot(axis);
                            let new_va = va + axis * (vbn - van);
                            let new_vb = vb + axis * (van - vbn);
                            let push = axis * (overlap * 0.5 + 1.0);
                            resolutions.push((ci_a, pos_a - push, new_va));
                            resolutions.push((ci_b, pos_b + push, new_vb));
                            // Midpoint cold ring marks the crack; throttled by the len cap below.
                            self.deflect_collide_buf.push(pos_a + axis * (d * 0.5));
                        }
                    }
                }
            }
        }
        for (ci, new_pos, new_vel) in resolutions {
            let crab = &mut self.crabs[ci];
            crab.pos = new_pos;
            crab.vel = new_vel;
            crab.speed = 1.0; // vel carries full speed, matching the flee/startle convention
            crab.startle_timer = crab.startle_timer.max(0.35); // cross-startle: the crash re-panics both
        }
        for &pos in &self.deflect_collide_buf {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((pos, 0.0));
            }
        }
    }

    fn register_catch(&mut self, catch_pos: Vec2, bonus_points: usize) {
        let mult = self.combo_multiplier();
        self.score += (1 + bonus_points) * mult;
        self.combo_count += 1;
        self.combo_timer = 1.8;

        // Score pop at catch position
        let pts = (1 + bonus_points) * mult;
        let score_text = if pts > 1 {
            format!("+{}  ON BEAT!", pts)
        } else {
            format!("+{}", pts)
        };
        let color = if pts > 1 {
            [1.0, 0.95, 0.3, 1.0]
        } else {
            [1.0, 1.0, 1.0, 0.9]
        };
        self.floating_texts
            .spawn(score_text, catch_pos - Vec2::new(10.0, 20.0), 28.0, color);

        // Combo pop above the player
        if self.combo_count >= 3 {
            let combo_color = match self.combo_count {
                3..=4 => [1.0, 0.6, 0.1, 1.0], // orange
                5..=7 => [1.0, 0.2, 0.2, 1.0], // red
                _ => [0.8, 0.3, 1.0, 1.0],     // purple
            };
            self.floating_texts.spawn(
                format!("x{} COMBO!", self.combo_count),
                self.player_pos - Vec2::new(0.0, 50.0),
                36.0,
                combo_color,
            );
        }
    }

    /// Payoff for catching a Dancer that's actively answering the player's Call. This closes the
    /// Call loop — an on-beat Call summons Dancers toward you, and snapping one up while it's still
    /// answering pays out extra score, a groove surge, and a distinct magenta "DANCE CATCH!" pop
    /// plus a juice punch, so the rhythm summon is worth engaging rather than incidental. Call with
    /// the crab's pre-catch `answering_call` timer and position; a no-op if the crab wasn't answering.
    fn reward_dance_catch(&mut self, was_answering: bool, pos: Vec2) {
        if !was_answering {
            return;
        }
        let mult = self.combo_multiplier();
        let bonus = 3 * mult;
        self.score += bonus;
        self.groove = (self.groove + 0.2).min(1.0);
        self.beat_intensity = (self.beat_intensity + 0.6).min(2.0);
        self.on_beat_flash = (self.on_beat_flash + 0.3).min(0.7);
        self.zoom_punch = self.zoom_punch.max(0.06);
        self.floating_texts.spawn(
            format!("DANCE CATCH! +{}", bonus),
            pos - Vec2::new(60.0, 46.0),
            30.0,
            [1.0, 0.4, 0.9, 1.0],
        );
    }

    fn combo_multiplier(&self) -> usize {
        match self.combo_count {
            0..=2 => 1,
            3..=5 => 2,
            6..=9 => 3,
            _ => 5,
        }
    }

    /// Cash out the live Groove Gamble streak. The player presses B to lock in what they've
    /// built rather than risk it on the next catch. Banking ON the beat secures the FULL current
    /// multiplier as a safe floor; banking off-beat takes a haircut — so the cash-out itself rides
    /// the rhythm. After banking, the live climb continues from the locked floor, so a savvy player
    /// can ratchet a stack safe one bank at a time. Nothing to bank if the live gain over the
    /// existing floor is negligible.
    fn bank_gamble(&mut self) {
        // Only bankable if there's meaningful live gain sitting above the already-locked floor.
        if self.beat_gamble_mult <= self.beat_gamble_locked + 0.24 {
            return;
        }
        let on_beat =
            self.beat_timer < BEAT_WINDOW || self.beat_timer > self.beat_interval - BEAT_WINDOW;
        // On-beat bank locks the whole thing; off-beat only banks 60% of the gain over the floor.
        let gain = self.beat_gamble_mult - self.beat_gamble_locked;
        let banked = if on_beat {
            self.beat_gamble_mult
        } else {
            self.beat_gamble_locked + gain * 0.6
        };
        self.beat_gamble_locked = banked.min(5.0);
        // The live multiplier can't drop below its own new floor; keep climbing from here.
        self.beat_gamble_mult = self.beat_gamble_locked;
        self.gamble_bank_flash = 1.0;
        self.zoom_punch = self.zoom_punch.max(0.045);
        let center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
        let (label, col) = if on_beat {
            ("BANKED ON BEAT!", [0.4, 1.0, 0.6, 1.0])
        } else {
            ("BANKED", [0.7, 0.9, 0.5, 1.0])
        };
        self.floating_texts.spawn(
            format!("{}  x{:.2} SAFE", label, self.beat_gamble_locked),
            center - Vec2::new(0.0, 96.0),
            36.0,
            col,
        );
    }

    fn check_milestone(&mut self, rng: &mut impl rand::Rng) {
        // chain_count is incremented on every catch and decremented on every snap/steal/deliver,
        // so it exactly equals the count of caught crabs — no need to rescan the whole vec.
        let chain_len = self.chain_count;
        if chain_len >= self.next_milestone {
            let milestone = self.next_milestone;
            self.next_milestone += 5;

            // Fireworks burst from player center
            let center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            self.particle_system
                .spawn_milestone_fireworks(center, milestone, rng);

            // Big centered banner text — spawn two: one shadow, one lit
            let banner = format!("{} CRABS!", milestone);
            // Floating texts live in the world layer (drawn before the HUD pass), so anchor the
            // banner near the player so it reads on-screen wherever the camera is, not at a fixed
            // world coordinate that the scrolling camera may have left behind.
            let screen_center = self.player_pos + Vec2::new(-100.0, -160.0);
            // Shadow
            self.floating_texts.spawn(
                banner.clone(),
                screen_center + Vec2::new(3.0, 3.0),
                72.0,
                [0.0, 0.0, 0.0, 0.85],
            );
            // Main text — gold/yellow
            self.floating_texts
                .spawn(banner, screen_center, 72.0, [1.0, 0.92, 0.1, 1.0]);

            // Extra-strong screen shake
            let kick_angle = rng.random_range(0.0_f32..std::f32::consts::TAU);
            self.screen_shake = 25.0;
            self.screen_shake_vel = Vec2::new(kick_angle.cos(), kick_angle.sin()) * 25.0 * 60.0;

            // Big celebratory zoom punch on every milestone.
            self.zoom_punch = self.zoom_punch.max(0.09);

            // Amplify beat flash
            self.beat_intensity = (self.beat_intensity + 1.5).min(2.0);
            self.on_beat_flash = 0.5;
        }
    }

    /// Cash in the train: if the player has a conga line and drives its head into the delivery pen,
    /// bank the whole train for a super-linear score payout (each extra crab is worth more than the
    /// last, so a longer, riskier train pays off disproportionately), then clear the chain and
    /// relocate the pen. This is the "bank now vs. push your luck" beat that closes the risk/reward
    /// loop chain-snap opened.
    /// Count same-type adjacent pairs in the caught train — the arrangement bonus tally. A "bond"
    /// is a caught crab whose immediate predecessor by chain_index is the same archetype. The rope
    /// glow (CHAIN_TYPE_BUF) lights these segments — plus, separately, any sandwich filling (see
    /// count_sandwiches) — so glowing segments equal bonds PLUS non-overlapping sandwiches, not just
    /// bonds. Optionally restricted to chain_index < `keep` so the cleave/snap payouts can count
    /// only the bonds that actually stay attached. O(n): builds a chain_index→type lookup, then
    /// walks it comparing each link to the one ahead.
    fn count_chain_bonds(&self, keep: usize) -> usize {
        self.count_bonds_and_sandwiches(keep).0
    }

    fn count_sandwiches(&self, keep: usize) -> usize {
        self.count_bonds_and_sandwiches(keep).1
    }

    /// Combined bond + sandwich + run-streak tally in a single O(n) scan. Fills BOND_INDEX_BUF once
    /// and returns (bonds, sandwiches, run_bonus_points) — callers that need several avoid a second
    /// full walk over self.crabs. `run_bonus_points` is already in points (RUN_STREAK_BONUS summed
    /// over every same-type run beyond length 2), not a count, so callers add it directly. The
    /// individual wrappers above exist for call sites that only need one value.
    fn count_bonds_and_sandwiches(&self, keep: usize) -> (usize, usize, usize, usize) {
        if keep < 2 {
            return (0, 0, 0, 0);
        }
        BOND_INDEX_BUF.with(|buf| {
            let mut by_index = buf.borrow_mut();
            // Grow-only: resize to `keep` slots, or clear+resize if the buffer is already large
            // enough (cheaper than realloc for small trains after a long one). Either way no
            // shrink — we keep the capacity for future calls.
            by_index.clear();
            by_index.resize(keep, None);
            for c in self.crabs.iter().filter(|c| c.caught) {
                if let Some(ci) = c.chain_index {
                    if ci < keep {
                        by_index[ci] = Some(c.crab_type);
                    }
                }
            }
            let mut bonds = 0;
            for i in 1..keep {
                if by_index[i].is_some() && by_index[i] == by_index[i - 1] {
                    bonds += 1;
                }
            }
            let mut sandwiches = 0;
            if keep >= 3 {
                for i in 1..keep - 1 {
                    // Both neighbors must be the SAME figurehead archetype (Golden or Dancer). The
                    // filling itself can be anything — including another figurehead, so a G-G-G run
                    // makes the middle a sandwich too (and still pays its two adjacency bonds; that's a
                    // deliberately-arranged cluster, so paying both is intended).
                    let left = by_index[i - 1];
                    let right = by_index[i + 1];
                    if left == right
                        && matches!(left, Some(CrabType::Golden) | Some(CrabType::Dancer))
                        && by_index[i].is_some()
                    {
                        sandwiches += 1;
                    }
                }
            }
            // Deep-run escalator: walk the contiguous same-type runs and pay RUN_STREAK_BONUS for
            // every crab beyond the third in each run (a run of length L pays L-2 kickers). Same
            // by_index lookup as bonds/sandwiches — one more linear pass, no extra crab scan.
            let mut run_bonus_points = 0;
            // CENTERPIECE: a same-type run of length >= 3 that straddles the train's midpoint pays
            // a flat bonus once per qualifying run — positional identity for the MIDDLE of the line
            // (a deep run seated in the protected center beats one dangling at the snappable tail).
            // The midpoint is a link boundary at keep/2; a run [start..=end] straddles it when it
            // spans that boundary, i.e. start <= mid-1 and end >= mid (using half-open indices).
            let mut centerpiece_bonus = 0;
            let mid = keep / 2;
            let mut run_len = 0usize; // length of the current same-type run ending at i-1
            let mut run_start = 0usize; // chain_index where the current run began
            let close_run = |len: usize, start: usize, end_exclusive: usize| -> usize {
                // Runs of length >= 3 straddling the midpoint earn the centerpiece kicker.
                if len >= 3 && start < mid && end_exclusive > mid {
                    CENTERPIECE_BONUS
                } else {
                    0
                }
            };
            for i in 0..keep {
                let extends = i > 0 && by_index[i].is_some() && by_index[i] == by_index[i - 1];
                if extends {
                    run_len += 1;
                } else {
                    // A run just ended (or the chain begins). Score the run we were building, then
                    // start a fresh one at this link (length 1 if occupied, 0 if a gap).
                    run_bonus_points += run_len.saturating_sub(2) * RUN_STREAK_BONUS;
                    if run_len > 0 {
                        centerpiece_bonus += close_run(run_len, run_start, i);
                    }
                    run_len = if by_index[i].is_some() { 1 } else { 0 };
                    run_start = i;
                }
            }
            // Score the final trailing run, which never hit a boundary to close it above.
            run_bonus_points += run_len.saturating_sub(2) * RUN_STREAK_BONUS;
            if run_len > 0 {
                centerpiece_bonus += close_run(run_len, run_start, keep);
            }
            (bonds, sandwiches, run_bonus_points, centerpiece_bonus)
        })
    }

    /// Which seated chain_index links currently belong to a PAYING centerpiece run, so the live
    /// draw pass can ring them and the player sees the protected mid-run *forming* instead of only
    /// learning about it at the pen. The predicate here is deliberately identical to `close_run`
    /// inside `count_bonds_and_sandwiches` (same-type run of `len >= 3` straddling the midpoint at
    /// `keep/2`): if the two ever drifted, we'd highlight a "centerpiece" that doesn't pay (or hide
    /// one that does), teaching the player the wrong arrangement. Returns a small owned Vec of the
    /// qualifying indices (trains are short; typically 0-1 runs); empty when nothing qualifies.
    /// Fill `out` with the chain indices that belong to a paying CENTERPIECE run.
    /// Uses a reused scratch buffer (`out`) rather than allocating a fresh Vec every call —
    /// this runs once per draw frame; at 60 fps on a long train that was a Vec::new() + heap
    /// alloc every 16 ms. Caller must clear `out` before calling.
    fn centerpiece_link_indices(&self, keep: usize, out: &mut Vec<usize>) {
        if keep < 3 {
            return;
        }
        BOND_INDEX_BUF.with(|buf| {
            let mut by_index = buf.borrow_mut();
            by_index.clear();
            by_index.resize(keep, None);
            for c in self.crabs.iter().filter(|c| c.caught) {
                if let Some(ci) = c.chain_index {
                    if ci < keep {
                        by_index[ci] = Some(c.crab_type);
                    }
                }
            }
            let mid = keep / 2;
            let mut run_len = 0usize;
            let mut run_start = 0usize;
            let mut flush =
                |len: usize, start: usize, end_exclusive: usize, out: &mut Vec<usize>| {
                    if len >= 3 && start < mid && end_exclusive > mid {
                        out.extend(start..end_exclusive);
                    }
                };
            for i in 0..keep {
                let extends = i > 0 && by_index[i].is_some() && by_index[i] == by_index[i - 1];
                if extends {
                    run_len += 1;
                } else {
                    if run_len > 0 {
                        flush(run_len, run_start, i, out);
                    }
                    run_len = if by_index[i].is_some() { 1 } else { 0 };
                    run_start = i;
                }
            }
            if run_len > 0 {
                flush(run_len, run_start, keep, out);
            }
        });
    }

    fn try_deliver_train(&mut self, ctx: &mut Context) {
        if self.chain_count == 0 {
            return;
        }
        let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        if player_center.distance(self.pen_pos) > PEN_RADIUS {
            return;
        }

        // How many crabs are actually banking (defensive count in case any wild state drifted).
        let delivered = self
            .crabs
            .iter()
            .filter(|c| c.caught)
            .count()
            .max(self.chain_count);
        if delivered == 0 {
            return;
        }

        // Super-linear base payout: triangular sum so crab #n adds n points, times a flat handler.
        let n = delivered;
        // Arrangement bonus: every same-type adjacent pair still intact at bank pays a flat kicker.
        // This is the reward for HOLDING an ordering to the pen (distinct from the catch-time MATCH
        // run). Folded into `base` BEFORE the multipliers so it rides the streak/perfect/gamble
        // stack exactly like the triangular sum, and so the pen_worth preview (which recomputes the
        // same base+bonds) can't disagree with what actually banks.
        let (bonds, sandwiches, run_bonus, centerpiece) = self.count_bonds_and_sandwiches(n);
        let base = (n * (n + 1) / 2) * 3
            + bonds * BOND_PAIR_BONUS
            + sandwiches * SANDWICH_BONUS
            + run_bonus
            + centerpiece;

        // A bank in quick succession bumps the delivery streak (capped) and refreshes its grace
        // window; the streak multiplier escalates the payout so cashing in repeatedly at tempo pays
        // off, not just hoarding one giant train.
        self.deliver_streak = (self.deliver_streak + 1).min(DELIVER_STREAK_MAX);
        self.deliver_streak_timer = DELIVER_STREAK_GRACE;
        // Streak 1 = 1.0x, then +0.25x per bank: 1.25x, 1.5x, ... up to 2.75x at the cap.
        let streak_mult = 1.0 + (self.deliver_streak.saturating_sub(1) as f32) * 0.25;

        // Banking on the beat lands a PERFECT DELIVERY: a flat percentage bonus on top of the streak.
        let perfect = self.on_beat_now();
        let perfect_mult = if perfect {
            1.0 + PERFECT_DELIVERY_BONUS
        } else {
            1.0
        };

        // The Groove Gamble multiplier rides through to the bank too — a hot on-beat streak makes
        // the delivery jackpot pay out even bigger, so it's worth protecting the heat right up to
        // the pen instead of grabbing sloppily on the way in.
        let bank =
            (base as f32 * streak_mult * perfect_mult * self.beat_gamble_mult).round() as usize;
        self.score += bank;
        // Attribute the rhythm-driven extra of this bank: the delivery streak is a pace reward that
        // survives without the beat, so the baseline keeps it — but the PERFECT (on-beat) delivery
        // bonus and the Groove Gamble multiplier are pure rhythm, so strip only those for the flat
        // reference. The difference is the mastery the beat bought at the pen, added to the tally.
        let flat_bank = (base as f32 * streak_mult).round() as usize;
        let jump = bank.saturating_sub(flat_bank);
        if jump > 0 {
            self.rhythm_bonus_score += jump;
            self.rhythm_bonus_flash = 1.0;
        }

        // Tutorial pass tracking: count real train deliveries for the chain-and-deliver learn-
        // session. This is the one write behind that tutorial's pure pass predicate
        // (`Tutorial::passed` for ChainDeliver), so a headless run of the scenario reaches the same
        // boolean without any rendering.
        if let Some(t) = self.tutorial.as_mut() {
            if t.kind == TutorialKind::ChainDeliver {
                t.deliveries += 1;
            }
        }

        // Before the delivered crabs leave the field, snapshot them (in chain order, head first)
        // so they can visibly march into the pen instead of blinking out — the parade is purely
        // cosmetic; the score above is already banked.
        let mut delivered_crabs: Vec<&EnemyCrab> = self.crabs.iter().filter(|c| c.caught).collect();
        // File them in in chain order (head of the train first) so the parade rolls down the line.
        delivered_crabs.sort_by_key(|c| c.chain_index.unwrap_or(usize::MAX));
        let marching: Vec<(Vec2, [f32; 3], f32)> = delivered_crabs
            .iter()
            .map(|c| (c.pos, c.crab_color(), c.scale))
            .collect();
        self.penned_marchers.spawn_train(self.pen_pos, &marching);

        // The delivered crabs leave the field for good — they've been penned.
        self.crabs.retain(|c| !c.caught);
        self.chain_count = 0;
        self.tail_run_len = 0; // whole train banked — the match run at the tail is gone
        self.next_milestone = 5;

        // Big celebratory feedback so banking feels like a real payoff, not just a number ticking.
        let mut rng = rand::rng();
        self.particle_system
            .spawn_milestone_fireworks(self.pen_pos, n, &mut rng);
        // A perfect on-beat bank gets a gold rhythm ring; a plain bank stays green.
        self.spawn_catch_shockwave(
            self.pen_pos,
            if perfect {
                [1.0, 0.85, 0.3]
            } else {
                [0.5, 1.0, 0.5]
            },
        );
        // A hot streak throws a second, larger firework burst so the escalation reads on screen.
        if self.deliver_streak >= 3 {
            self.particle_system.spawn_milestone_fireworks(
                self.pen_pos,
                n + self.deliver_streak as usize * 4,
                &mut rng,
            );
        }
        self.floating_texts.spawn(
            format!("BANKED +{}", bank),
            self.pen_pos - Vec2::new(60.0, 40.0),
            48.0,
            [0.4, 1.0, 0.5, 1.0],
        );
        // Perfect-on-beat and streak callouts stack above the bank number so the player sees *why*
        // this bank paid more.
        let mut callout_y = 4.0;
        if perfect {
            self.floating_texts.spawn(
                "PERFECT DELIVERY!".to_string(),
                self.pen_pos - Vec2::new(95.0, callout_y),
                30.0,
                [1.0, 0.9, 0.35, 1.0],
            );
            callout_y += 30.0;
        }
        if self.deliver_streak >= 2 {
            self.floating_texts.spawn(
                format!("x{} STREAK  ({:.2}x)", self.deliver_streak, streak_mult),
                self.pen_pos - Vec2::new(85.0, callout_y),
                26.0,
                [1.0, 0.55, 0.9, 1.0],
            );
            callout_y += 26.0;
        }
        // ARRANGED — the arrangement bonus made legible. Every same-type adjacent pair held intact
        // to the pen (each a glowing rope segment on the way in) paid BOND_PAIR_BONUS; naming it
        // here tells the player their *ordering*, not just their length, earned this — the payoff
        // face of making the middle of the train matter. Cyan so it reads distinct from the gold
        // perfect / pink streak callouts.
        if bonds > 0 {
            self.floating_texts.spawn(
                format!("ARRANGED x{}  (+{})", bonds, bonds * BOND_PAIR_BONUS),
                self.pen_pos - Vec2::new(90.0, callout_y),
                26.0,
                [0.4, 0.95, 1.0, 1.0],
            );
            callout_y += 26.0;
        }
        // SANDWICH — the mid-train figurehead-flanking bonus made legible. Warm gold so it reads as
        // kin to the Golden figurehead economy while staying distinct from the cyan ARRANGED tag.
        if sandwiches > 0 {
            self.floating_texts.spawn(
                format!(
                    "SANDWICH x{}  (+{})",
                    sandwiches,
                    sandwiches * SANDWICH_BONUS
                ),
                self.pen_pos - Vec2::new(90.0, callout_y),
                26.0,
                [1.0, 0.8, 0.35, 1.0],
            );
            callout_y += 26.0;
        }
        // BLOCK — the deep-run escalator made legible. A same-type run of 3+ held to the pen paid
        // run_bonus on top of its adjacency bonds; naming it tells the player that stacking a LONG
        // matched block (not just scattered pairs) is what earned this. Vivid green so it reads as a
        // third, distinct arrangement tier next to cyan ARRANGED and gold SANDWICH.
        if run_bonus > 0 {
            self.floating_texts.spawn(
                format!("BLOCK!  (+{})", run_bonus),
                self.pen_pos - Vec2::new(80.0, callout_y),
                26.0,
                [0.5, 1.0, 0.5, 1.0],
            );
            callout_y += 26.0;
        }
        // CENTERPIECE — positional identity for the MIDDLE of the train. A deep run seated across
        // the train's midpoint (safe from tail snaps) earned this; naming it tells the player that
        // WHERE they parked their best block, not just that they built one, is what paid. Bright
        // magenta so it reads as the top arrangement tier above cyan ARRANGED / gold SANDWICH / green BLOCK.
        if centerpiece > 0 {
            self.floating_texts.spawn(
                format!("CENTERPIECE!  (+{})", centerpiece),
                self.pen_pos - Vec2::new(105.0, callout_y),
                28.0,
                [1.0, 0.45, 0.95, 1.0],
            );
            callout_y += 28.0;
        }
        // LONG HAUL — the payoff face of the AT RISK gamble. It fires at the SAME length tiers the
        // risk escalates at (the panic_snap_links steps: 8, 12, 16), so a train that was flashing
        // AT RISK on the way in now cashes out as a named reward. This adds NO new multiplier — the
        // bank is already superlinear via the triangular base curve. Instead it *names* how much of
        // that base the priciest tail links (everything past the tier threshold) actually earned,
        // so the upside of holding long reads as loudly on screen as the downside did. The number
        // shown is the marginal triangular value of links past `thresh`: base(n) - base(thresh).
        let long_haul_tier = match n {
            16.. => Some(("GRAND HAUL!", 16usize, [1.0, 0.55, 0.2, 1.0])),
            12..=15 => Some(("LONG HAUL!", 12, [1.0, 0.75, 0.25, 1.0])),
            8..=11 => Some(("BIG HAUL!", 8, [1.0, 0.9, 0.4, 1.0])),
            _ => None,
        };
        if let Some((label, thresh, color)) = long_haul_tier {
            // Marginal points the tail links past the tier threshold contributed to the base payout,
            // carried through the same multipliers the whole bank got — real earned score attributed
            // to the length you refused to bank, not a bolt-on bonus.
            let tail_base = (n * (n + 1) / 2).saturating_sub(thresh * (thresh + 1) / 2) * 3;
            let tail_bank = (tail_base as f32 * streak_mult * perfect_mult * self.beat_gamble_mult)
                .round() as usize;
            self.floating_texts.spawn(
                format!("{}  +{} FROM THE TAIL", label, tail_bank),
                self.pen_pos - Vec2::new(120.0, callout_y),
                30.0,
                color,
            );
            callout_y += 30.0;
            // A held-long bank earns extra celebration so the risk you carried pays off viscerally.
            self.particle_system.spawn_milestone_fireworks(
                self.pen_pos,
                n + (n - thresh) * 3,
                &mut rng,
            );
            self.screen_shake = self.screen_shake.max(24.0);
        }
        self.floating_texts.spawn(
            format!("{} crabs delivered!", n),
            self.pen_pos - Vec2::new(70.0, callout_y),
            26.0,
            [1.0, 0.95, 0.6, 1.0],
        );
        self.deliver_flash = 1.0;
        // Anchor the delivery beam at the player (train head) as it stood this bank, before the pen
        // relocates below — the beam is drawn to the OLD pen this frame's flash decays over.
        self.deliver_beam_from = player_center;
        self.deliver_beam_to = self.pen_pos;
        self.deliver_beam_perfect = perfect;
        // A perfect / hot-streak bank hits harder: more zoom, more shake, a fuller groove kick.
        let intensity = streak_mult * perfect_mult;
        self.zoom_punch = self.zoom_punch.max(0.11 * intensity);
        self.screen_shake = self.screen_shake.max(18.0 * intensity);
        let kick_angle = rng.random_range(0.0_f32..std::f32::consts::TAU);
        self.screen_shake_vel =
            Vec2::new(kick_angle.cos(), kick_angle.sin()) * 18.0 * intensity * 60.0;
        self.on_beat_flash = if perfect { 0.85 } else { 0.6 };
        self.groove = (self.groove + if perfect { 0.5 } else { 0.35 }).min(1.0);
        let _ = self.sounds.success2.play_detached(ctx);

        // Move the pen so the next bank is a fresh routing decision, not a treadmill loop.
        self.pen_pos = pick_pen_pos(self.world_width, self.world_height, player_center, &mut rng);

        // Banking is the single biggest score jump in the game, so it's the most likely place to
        // cross an upgrade threshold — check HERE, at the pen, so the upgrade screen lands on the
        // natural pause right after a delivery (the moment the player earned it). Previously the
        // check ran only from the three catch sites, so a threshold crossed by a big bank sat
        // silent until some unrelated mid-field catch popped the screen out of nowhere — the
        // "fires at an odd moment" bug Carl hit in playtest. A bank is a lull, not mid-action, so
        // it's exactly when a menu is least disruptive.
        self.check_upgrade_unlock(ctx);
    }

    // check_upgrade_unlock and roll_upgrade_offer now live in src/upgrade.rs (impl MainState there).

    fn handle_crab_catching(&mut self, ctx: &mut Context) {
        let mult = self.combo_multiplier();
        let mut any_caught = false;
        // Reused scratch buffers instead of fresh Vec::new() every frame — this function runs
        // unconditionally every tick and the overwhelming majority of frames catch zero crabs,
        // so allocating three empty Vecs per call was pure per-frame churn on the hottest path.
        let mut startle_origins = std::mem::take(&mut self.startle_origins_buf);
        startle_origins.clear();
        let mut boss_catches = std::mem::take(&mut self.boss_catches_buf);
        boss_catches.clear();
        // Dancers snapped up while still answering a Call — paid out after the loop (needs &mut self).
        let mut dance_catches = std::mem::take(&mut self.dance_catches_buf);
        dance_catches.clear();
        // Golden crabs snapped up this frame — the big lump-sum bonus is paid out after the loop.
        let mut golden_catches = std::mem::take(&mut self.golden_catches_buf);
        golden_catches.clear();
        // Goldens caught directly behind a Magnet link this frame — the "shine conducts down the
        // train" cascade, paid out after the loop. See the adjacency check inside the loop below.
        let mut magnet_shine_catches = std::mem::take(&mut self.magnet_shine_catches_buf);
        magnet_shine_catches.clear();
        // Same-type "match run" events this frame — a catch that extends a run of matching-archetype
        // links at the tail. Paid out (escalating bonus + callout) after the loop.
        let mut match_run_catches = std::mem::take(&mut self.match_run_catches_buf);
        match_run_catches.clear();
        // Splitter crabs snapped up this frame — each one cleaves the train at the midpoint and banks
        // the back half. Deferred to after the loop (the cleave/bank borrows &mut self and mutates
        // chain_index across all crabs, which we can't do mid-loop holding a &mut into self.crabs).
        // At most one split per frame matters (they stack chaotically otherwise), so we just record
        // whether a Splitter landed and where.
        let mut splitter_catch: Option<Vec2> = None;
        // Type of the crab that currently sits at the *tail* of the train (highest chain_index),
        // snapshotted before the catch loop so we can tell what a newly-caught crab links onto. As
        // each catch lands the new crab becomes the tail, so we roll this forward per catch instead
        // of re-scanning self.crabs mid-loop (which we can't, holding a &mut into it). None if the
        // train is empty. This is what makes catch *order* a live decision: whether a Magnet is the
        // link directly ahead of a just-caught Golden depends on the sequence the player caught in.
        // Single O(n) snapshot pass over the caught-crab list for three per-frame reads that
        // used to be three separate scans:
        //   • prev_tail_type  — the type at the current tail (highest chain_index, == chain_count-1)
        //   • head_is_golden  — whether chain_index 0 is a Golden (figurehead bonus)
        //   • head_is_dancer  — whether chain_index 0 is a Dancer (Drum-Major bonus)
        // chain_index 0 can't be the tail at the same time (only true when chain_count == 1, in
        // which case prev_tail_type and both head flags all still get set correctly in one pass).
        let tail_ci = self.chain_count.checked_sub(1);
        let mut prev_tail_type: Option<CrabType> = None;
        let mut prev_tail_pos: Vec2 = Vec2::ZERO;
        let mut head_is_golden = false;
        let mut head_is_dancer = false;
        for c in &self.crabs {
            match c.chain_index {
                Some(0) => {
                    // Head of the train.
                    // Golden Figurehead — the head-position mirror of the Armored tail-guard. A
                    // Golden crab riding at the head (chain_index 0) acts as a gilded figurehead:
                    // every same-type match run pays a bigger bonus while it leads. This gives the
                    // *front* of the train real positional value — until now only the tail paid.
                    head_is_golden = c.is_golden();
                    // Dancer Drum-Major — the rhythm-economy sibling of the Golden figurehead,
                    // competing for the same coveted head slot. On-beat catches fill the groove
                    // meter faster and bump the Groove Gamble harder while it leads.
                    head_is_dancer = c.is_dancer();
                    // Could also be the tail if chain_count == 1.
                    if tail_ci == Some(0) {
                        prev_tail_type = Some(c.crab_type);
                        prev_tail_pos = c.pos;
                    }
                }
                Some(ci) if Some(ci) == tail_ci => {
                    prev_tail_type = Some(c.crab_type);
                    prev_tail_pos = c.pos;
                }
                _ => {}
            }
        }
        // Reef DJ backup dancers caught this frame on a *called (hot) beat* — each one chips the
        // boss shell. Collected here and applied after the loop so we don't need a second &mut
        // borrow of self.crabs mid-loop. `reef_hot_now` is the same window the DJ's own shell uses.
        let reef_hot_now = (self.beat_timer < BEAT_WINDOW
            || self.beat_timer > self.beat_interval - BEAT_WINDOW)
            && self.reef_phrase[(self.beat_count % 4) as usize];
        let mut hype_dancer_hits = std::mem::take(&mut self.hype_dancer_hits_buf);
        hype_dancer_hits.clear();
        for crab in &mut self.crabs {
            if crab.is_catchable()
                && (self.player_pos.x - crab.pos.x).abs() < (PLAYER_SIZE + crab.scale) / 2.0
                && (self.player_pos.y - crab.pos.y).abs() < (PLAYER_SIZE + crab.scale) / 2.0
            {
                if crab.is_boss() {
                    boss_catches.push((crab.pos, crab.is_tide_boss()));
                }
                // Get crab color before marking as caught
                let crab_color = crab.crab_color();

                // Spawn particle effect
                let mut rng = rand::rng();
                self.particle_system.spawn_catch_effect(
                    crab.pos,
                    crab_color,
                    crab.crab_type,
                    &mut rng,
                );
                let shock_pos = crab.pos;

                if crab.answering_call > 0.0 {
                    dance_catches.push(crab.pos);
                }
                // Reef DJ backup dancer snapped up on a called (hot) beat: queue a shell chip. This
                // is the archetype's job inside the boss fight — a Dancer caught in time with the
                // DJ's phrase helps crack it, so herding its own hype crew onto the beat pays off.
                if self.reef_active && reef_hot_now && crab.is_dancer() {
                    hype_dancer_hits.push(crab.pos);
                }
                crab.caught = true;
                self.chain_join_ripple = true;
                if self.catch_shockwaves.len() < 48 {
                    self.catch_shockwaves.push((shock_pos, 0.0, crab_color));
                }
                startle_origins.push(shock_pos);
                any_caught = true;
                crab.chain_index = Some(self.chain_count);
                // Bond-forming flash: if this catch links a same-type neighbor, emit a brief
                // connecting arc so the player sees the bond click into place (legibility of the
                // arrangement system — makes the chain structure readable in motion).
                if prev_tail_type == Some(crab.crab_type) && self.chain_count > 0 {
                    if self.bond_flash_events.len() < 24 {
                        self.bond_flash_events
                            .push((prev_tail_pos, crab.pos, crab_color, 1.0));
                    }
                }
                // Roll prev_tail forward so the NEXT catch in the same frame (multi-catch) sees
                // the freshly-linked crab as the tail.
                prev_tail_type = Some(crab.crab_type);
                prev_tail_pos = crab.pos;
                self.chain_count += 1;
                let on_beat = self.beat_timer < BEAT_WINDOW
                    || self.beat_timer > self.beat_interval - BEAT_WINDOW;
                // PERFECT: the catch landed inside the tight sub-window at the very center of the
                // beat. This is the skill ceiling — strictly harder than on_beat, and only it feeds
                // the super-linear flawless-run bonus below.
                let perfect = self.beat_timer < PERFECT_WINDOW
                    || self.beat_timer > self.beat_interval - PERFECT_WINDOW;
                let bonus;
                if on_beat {
                    // Tutorial pass tracking: count real on-beat catches for the beat-timing
                    // learn-session. This is the one write behind the tutorial's pure pass
                    // predicate (`Tutorial::passed`), so a headless run of the same scenario reaches
                    // the same boolean without any rendering.
                    if let Some(t) = self.tutorial.as_mut() {
                        if t.kind == TutorialKind::BeatTiming {
                            t.on_beat_catches += 1;
                        }
                    }
                    // On-beat catch: build the groove. Consecutive on-beat catches escalate the
                    // score bonus and fill the groove meter, which in turn swells the music.
                    self.beat_streak += 1;
                    // Precision ladder: a PERFECT hit extends the flawless run; an on-beat-but-not-
                    // perfect catch keeps beat_streak alive (streak isn't broken) but resets the
                    // flawless run — precision is a bonus lane, never a punishment for near-misses.
                    if perfect {
                        self.perfect_streak += 1;
                        self.perfect_flash = 1.0;
                    } else {
                        self.perfect_streak = 0;
                    }
                    // A Dancer Drum-Major at the head keeps the whole train on time: a fatter groove
                    // fill per on-beat catch so the meter swells (and the music with it) faster.
                    let groove_fill = if head_is_dancer { 0.30 } else { 0.22 };
                    self.groove = (self.groove + groove_fill).min(1.0);
                    bonus = self.beat_streak.min(5) as usize;
                    self.on_beat_flash = (0.25 + self.beat_streak as f32 * 0.06).min(0.6);
                    // Beat-hit punch: additive impact flash at the catch site. Quality 1.0 on a
                    // PERFECT downbeat hit, 0.5 on an ordinary on-beat catch.
                    let beat_quality = if perfect { 1.0_f32 } else { 0.5_f32 };
                    self.beat_punch_events
                        .push((shock_pos, crab_color, beat_quality));
                    // Groove Gamble: the streak compounds a live global score multiplier. Each
                    // on-beat catch bumps it +0.25x (capped at 5x), so the deeper you ride the beat
                    // the more every point — catches AND deliveries — is worth. The catch mid-streak
                    // feels louder: the multiplier only exists while the run is unbroken.
                    let prev_mult = self.beat_gamble_mult;
                    // Drum-Major at the head bumps the gamble harder (+0.35x vs +0.25x): the rhythm
                    // economy the Dancer leads scales the whole run faster, the counterweight to the
                    // Golden figurehead's match-run amplification. One head slot, two ways to spend it.
                    let gamble_step = if head_is_dancer { 0.35 } else { 0.25 };
                    self.beat_gamble_mult = (self.beat_gamble_mult + gamble_step).min(5.0);
                    if self.beat_gamble_mult > prev_mult {
                        self.beat_gamble_flash = 1.0;
                    }
                    // Drum-Major assist reads on screen so the head-slot choice pays visibly, not just
                    // in the meter — a teal rhythm shine on the newly-linked tail, the counterpart to
                    // the Golden figurehead's gild. Fires on every on-beat catch while a Dancer leads.
                    if head_is_dancer {
                        self.floating_texts.spawn(
                            "DRUM-MAJOR!".to_string(),
                            crab.pos - Vec2::new(56.0, 46.0),
                            24.0,
                            [0.4, 1.0, 0.85, 1.0],
                        );
                    }
                    // Escalating callouts as the heat tiers up, so the rising stakes read on screen.
                    if self.beat_streak >= 3 {
                        let (label, col, size) = match self.beat_streak {
                            3..=4 => ("HEATING UP", [0.4, 1.0, 0.85, 1.0], 34.0),
                            5..=7 => ("ON FIRE!", [1.0, 0.7, 0.2, 1.0], 40.0),
                            8..=11 => ("BLAZING!", [1.0, 0.35, 0.15, 1.0], 46.0),
                            _ => ("INFERNO!!", [1.0, 0.2, 0.5, 1.0], 52.0),
                        };
                        self.floating_texts.spawn(
                            format!("{}  x{:.2}", label, self.beat_gamble_mult),
                            self.player_pos - Vec2::new(0.0, 80.0),
                            size,
                            col,
                        );
                    }
                } else {
                    // Off-beat catch breaks the streak and drains the groove. Only the UNBANKED gain
                    // above the locked floor is lost — whatever the player cashed out with B stays
                    // safe. If a hot unbanked stack was riding, punch a red flash + callout so the
                    // greedy grab stings; then fall back to the banked floor, not all the way to 1x.
                    if self.beat_gamble_mult > self.beat_gamble_locked + 0.5 {
                        self.streak_lost_flash = 1.0;
                        self.shake_timer = self.shake_timer.max(0.3);
                        let lost = self.beat_gamble_mult - self.beat_gamble_locked;
                        let msg = if self.beat_gamble_locked > 1.01 {
                            format!(
                                "STREAK LOST!  x{:.2} gone — x{:.2} safe",
                                lost, self.beat_gamble_locked
                            )
                        } else {
                            format!("STREAK LOST!  x{:.2} gone", self.beat_gamble_mult)
                        };
                        self.floating_texts.spawn(
                            msg,
                            self.player_pos - Vec2::new(0.0, 80.0),
                            40.0,
                            [1.0, 0.35, 0.3, 1.0],
                        );
                    }
                    self.beat_streak = 0;
                    self.perfect_streak = 0;
                    self.beat_gamble_mult = self.beat_gamble_locked;
                    self.groove = (self.groove - 0.3).max(0.0);
                    bonus = 0;
                }
                let pos = crab.pos;
                let player_pos = self.player_pos;
                // Whip-streak from the catch point to the head of the train, so the crab reads as
                // yanked in. Brighter/faster-fading trails happen on-beat via the draw's age curve.
                if self.catch_trails.len() < 48 {
                    let head = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
                    let start = if on_beat { -0.25 } else { 0.0 }; // on-beat trails linger a hair longer
                    self.catch_trails.push((crab.pos, head, start, crab_color));
                }
                // Inline register_catch to avoid &mut self conflict with the crabs loop.
                // The Groove Gamble multiplier scales the whole award, so a hot streak makes every
                // catch worth dramatically more — the payoff for riding the beat unbroken.
                let pts = (((1 + bonus) * mult) as f32 * self.beat_gamble_mult).round() as usize;
                self.score += pts;
                // Attribute the rhythm-driven extra: what this catch would have paid at neutral
                // rhythm (no streak bonus, no gamble multiplier) vs. what it actually paid. The gap
                // is the mastery the beat bought, tallied for the "how far ahead" readout.
                let flat = (1 * mult) as usize; // bonus=0, gamble=1x
                self.rhythm_bonus_score += pts.saturating_sub(flat);
                // PERFECT precision bonus — the legible skill ceiling. Awarded on top of everything
                // above, but SEPARATELY from the gamble multiplier (which we deliberately don't
                // touch, so banking stays balanced): a flat, super-linear score kicker that scales
                // with the flawless run. perfect_streak grows the reward quadratically (n*(n+1)/2,
                // the same triangular shape as the pen jackpot) so a sustained in-the-pocket run
                // pulls dramatically ahead of a merely-good one — and the callout shows how far.
                if perfect && self.perfect_streak > 0 {
                    let n = self.perfect_streak.min(24) as usize; // cap so it can't run away
                    // Triangular growth, scaled by the level multiplier: 5, 15, 30, 50, ... per hit.
                    let perfect_pts = (n * (n + 1) / 2) * 5 * mult as usize;
                    self.score += perfect_pts;
                    self.rhythm_bonus_score += perfect_pts;
                    // Legible payoff: the flawless tier and its running rhythm-bonus total, so the
                    // player sees precision compounding. Only fire the loud callout once the run is
                    // deep enough to matter, so early perfects don't spam the screen.
                    if self.perfect_streak >= 3 {
                        let (label, size) = match self.perfect_streak {
                            3..=5 => ("PERFECT!", 34.0),
                            6..=9 => ("FLAWLESS!", 42.0),
                            _ => ("IN THE POCKET!!", 50.0),
                        };
                        self.floating_texts.spawn(
                            format!("{}  x{}  +{}", label, self.perfect_streak, perfect_pts),
                            self.player_pos - Vec2::new(0.0, 116.0),
                            size,
                            [0.6, 0.95, 1.0, 1.0],
                        );
                    }
                }
                // Golden crab: on top of the normal catch award, queue a big lump-sum treasure bonus
                // (paid out after the loop). This is the payoff for breaking off the herd to chase it.
                // Splitter: record the catch so the after-loop cleave can bank the back half. The
                // Splitter has just become the tail (highest chain_index) this catch; the split
                // block below decides where to cleave. Only the last Splitter caught this frame
                // wins — one cleave per frame keeps the moment legible.
                if crab.is_splitter() {
                    splitter_catch = Some(pos);
                }
                if crab.is_golden() {
                    golden_catches.push((pos, pts));
                    // Crossover — the shine conducts down the train. If the link this Golden just
                    // snapped onto (the previous tail) is a Magnet, the Magnet's field carries the
                    // Golden's shine along the whole conga line, paying a length-scaled cascade.
                    // Whether this fires depends purely on catch ORDER: park a Magnet at the tail,
                    // then chase a Golden onto it. Deferred so the cascade payout can borrow &mut self.
                    if prev_tail_type == Some(CrabType::Magnet) {
                        magnet_shine_catches.push(pos);
                    }
                }
                // Same-type match run — the arrangement mechanic. If this crab is the same archetype
                // as the link it just snapped onto (the previous tail), it extends a run of matching
                // neighbors and each additional link pays an escalating bonus; a mismatched catch
                // resets the run to a single link. Whether a run builds depends purely on catch ORDER,
                // so the player catches to *build a pattern* of same-type links, not just length.
                // Deferred payout (bonus + callout borrows &mut self) collected into match_run_catches.
                if prev_tail_type == Some(crab.crab_type) {
                    self.tail_run_len += 1;
                } else {
                    self.tail_run_len = 1;
                }
                if self.tail_run_len >= 2 {
                    // The run length itself is the escalation: link 2 pays a little, deeper runs pay
                    // more, capped so a very long single-type train can't runaway-score. Scaled by the
                    // same combo/gamble multipliers as the base catch so it rides a hot streak too.
                    let run = self.tail_run_len.min(8);
                    // A Golden figurehead at the head amplifies the whole match economy: +50% on
                    // every run bonus while it leads. Legible reward for choosing to park the prize
                    // up front instead of cashing it — the front of the train finally pays.
                    let figurehead_mult = if head_is_golden { 1.5 } else { 1.0 };
                    let match_bonus =
                        ((run as usize) * mult) as f32 * self.beat_gamble_mult * figurehead_mult;
                    self.score += match_bonus.round() as usize;
                    match_run_catches.push((crab.pos, self.tail_run_len, crab.crab_color()));
                    // Match-Run Milestone: crossing every 4th same-type link (4, 8, 12…) is a big,
                    // watchable payoff on top of the incremental run bonus — a bold callout, a
                    // color-matched shockwave down the tail, and a chunky score kicker. Makes
                    // committing to a long single-type run (the order-as-bet) climax visibly
                    // instead of just ticking a counter. Inlined (shockwave/floating_texts fields
                    // are disjoint from the active &mut self.crabs borrow in this loop).
                    if self.tail_run_len >= 4 && self.tail_run_len % 4 == 0 {
                        let tier = self.tail_run_len / 4; // 1 at 4, 2 at 8, …
                        let col = crab.crab_color();
                        // Score kicker scales with the run tier and rides the same hot-streak mults.
                        let kicker = ((self.tail_run_len as usize * 6 * mult) as f32
                            * self.beat_gamble_mult
                            * figurehead_mult)
                            .round() as usize;
                        self.score += kicker;
                        self.floating_texts.spawn(
                            format!("MATCH x{}!  +{}", self.tail_run_len, kicker),
                            crab.pos - Vec2::new(60.0, 64.0),
                            34.0 + tier as f32 * 4.0,
                            [col[0], col[1], col[2], 1.0],
                        );
                        if self.catch_shockwaves.len() < 48 {
                            self.catch_shockwaves.push((crab.pos, 0.0, col));
                        }
                        self.on_beat_flash = self.on_beat_flash.max(0.4);
                        self.shake_timer = self.shake_timer.max(0.5);
                        self.zoom_punch = self.zoom_punch.max(0.06);
                    }
                    if head_is_golden {
                        // Gild the run callout so the figurehead's assist reads on screen, not just
                        // in the score — a small golden shine on the newly-linked tail.
                        self.floating_texts.spawn(
                            "FIGUREHEAD!".to_string(),
                            crab.pos - Vec2::new(52.0, 46.0),
                            24.0,
                            [1.0, 0.86, 0.28, 1.0],
                        );
                        // Inlined shockwave push (a &mut self method call would conflict with the
                        // active &mut borrow of self.crabs in this loop; the field is disjoint).
                        if self.catch_shockwaves.len() < 48 {
                            self.catch_shockwaves
                                .push((crab.pos, 0.0, [1.0, 0.85, 0.3]));
                        }
                    }
                }
                // Roll the tail-type snapshot forward: this freshly-caught crab is now the tail, so
                // it's what the *next* catch this frame will link onto. Keeps the adjacency check O(1)
                // per catch with no mid-loop rescan of self.crabs.
                prev_tail_type = Some(crab.crab_type);
                self.combo_count += 1;
                self.combo_timer = 1.8;
                let score_str = if self.beat_gamble_mult > 1.01 {
                    format!("+{}  x{:.2}!", pts, self.beat_gamble_mult)
                } else if pts > 1 {
                    format!("+{}  ON BEAT!", pts)
                } else {
                    format!("+{}", pts)
                };
                let score_col = if pts > 1 {
                    [1.0, 0.95, 0.3, 1.0]
                } else {
                    [1.0, 1.0, 1.0, 0.9]
                };
                self.floating_texts
                    .spawn(score_str, pos - Vec2::new(10.0, 20.0), 28.0, score_col);
                if self.combo_count >= 3 {
                    let cc = self.combo_count;
                    let combo_col = match cc {
                        3..=4 => [1.0, 0.6, 0.1, 1.0],
                        5..=7 => [1.0, 0.2, 0.2, 1.0],
                        _ => [0.8, 0.3, 1.0, 1.0],
                    };
                    self.floating_texts.spawn(
                        format!("x{} COMBO!", cc),
                        player_pos - Vec2::new(0.0, 50.0),
                        36.0,
                        combo_col,
                    );
                }
                self.shake_timer = 0.4;
                self.time_since_catch = 0.0;
                // Punchy freeze — a touch longer when the catch lands on the beat.
                self.hitstop_timer = self.hitstop_timer.max(if on_beat { 0.08 } else { 0.05 });
                // Snap the camera in a hair on every catch, harder on the beat, for extra impact.
                self.zoom_punch = self.zoom_punch.max(if on_beat { 0.055 } else { 0.035 });
                play_catch_sound(&mut self.sounds, ctx, &mut rng, self.beat_streak);
            }
        }
        // Deferred out of the `&mut self.crabs` loop above: check_upgrade_unlock borrows all of
        // self, which conflicts with the live crab iterator. Score only rises inside the loop, so
        // running the threshold check once afterward is equivalent.
        self.check_upgrade_unlock(ctx);
        for &origin in &startle_origins {
            self.emit_catch_startle(origin);
        }
        for &pos in &dance_catches {
            self.reward_dance_catch(true, pos);
        }
        for &(bpos, is_tide) in &boss_catches {
            self.on_boss_caught(bpos, is_tide);
        }
        // Apply Reef DJ shell chips from hype dancers caught on a hot beat. Find the live DJ and
        // knock a chunk off its shell per dancer, with a legible callout + juice so the assist
        // reads on screen. If a chip finishes the boss, queue its catch payoff like a beam kill.
        if !hype_dancer_hits.is_empty() {
            let mut broke_at: Option<Vec2> = None;
            for crab in &mut self.crabs {
                if crab.is_rhythm_boss() && !crab.caught && crab.boss_health > 0.0 {
                    for _ in &hype_dancer_hits {
                        crab.boss_health -= 0.4;
                    }
                    if crab.boss_health <= 0.0 {
                        crab.boss_health = 0.0;
                        broke_at = Some(crab.pos);
                    }
                    break;
                }
            }
            for &dpos in &hype_dancer_hits {
                self.floating_texts.spawn(
                    "HYPE! shell cracked".to_string(),
                    dpos - Vec2::new(40.0, 40.0),
                    28.0,
                    [0.85, 0.5, 1.0, 1.0],
                );
                self.particle_system
                    .spawn_milestone_fireworks(dpos, 8, &mut rand::rng());
            }
            self.reef_hit_flash = 1.0;
            self.screen_shake = self.screen_shake.max(6.0);
            // A dancer chip that empties the shell worns the DJ down (it doesn't catch it — the
            // player still snaps it up). Fire the same "worn down, catch it!" juice as the beam path.
            if let Some(bpos) = broke_at {
                self.floating_texts.spawn(
                    "WORN DOWN — CATCH IT!".to_string(),
                    bpos - Vec2::new(110.0, 46.0),
                    34.0,
                    [0.4, 1.0, 0.5, 1.0],
                );
                self.spawn_catch_shockwave(bpos, [1.0, 0.85, 0.3]);
                self.screen_shake = self.screen_shake.max(14.0);
                self.on_beat_flash = self.on_beat_flash.max(0.4);
            }
        }
        for &(gpos, base_pts) in &golden_catches {
            self.on_golden_caught(gpos, base_pts);
        }
        // Magnet-shine cascade: a Golden caught directly behind a Magnet link conducts its shine
        // down the whole train. Paid out here so it can borrow &mut self for score/particles/trails.
        for &spos in &magnet_shine_catches {
            self.on_magnet_shine_cascade(spos);
        }
        // Splitter cleave: catching a Splitter halves the train at the midpoint and instantly banks
        // the back half for points — the arrangement *bet*. Done here (after the catch loop) so it
        // can borrow &mut self to rewrite chain_index across the whole train and pay out.
        if let Some(spos) = splitter_catch {
            self.split_train_bank(spos);
        }
        // Same-type match runs: a legible, escalating callout in the matched archetype's own color
        // so the player sees the arrangement paying off — "MATCH x3!" grows and brightens with the
        // run, and a matching-hued ring/shockwave marks the newly-linked tail so the bond reads on
        // screen, not just in the score. This is the watchable feedback for catching to build a
        // pattern; the colored rope bond (see draw_conga_rope) is the persistent version of it.
        for &(pos, run, col) in &match_run_catches {
            let size = (26.0 + run as f32 * 4.0).min(52.0);
            self.floating_texts.spawn(
                format!("MATCH x{}!", run),
                pos - Vec2::new(0.0, 44.0),
                size,
                [col[0], col[1], col[2], 1.0],
            );
            self.spawn_catch_shockwave(pos, col);
            // A deep run lands harder — a little shake + on-beat flash so a long same-type streak
            // feels like a real escalation, matching how combos/streaks escalate their juice.
            if run >= 4 {
                // Cap the shake against the same ceiling the score uses so a very long single-type
                // run can't escalate screen shake without bound (visual spam) every catch.
                self.screen_shake = self.screen_shake.max(3.0 + run.min(8) as f32);
                self.on_beat_flash = self.on_beat_flash.max(0.3);
            }
        }
        // Hand the scratch buffers back for reuse next frame.
        self.startle_origins_buf = startle_origins;
        self.boss_catches_buf = boss_catches;
        self.dance_catches_buf = dance_catches;
        self.golden_catches_buf = golden_catches;
        self.magnet_shine_catches_buf = magnet_shine_catches;
        self.match_run_catches_buf = match_run_catches;
        self.hype_dancer_hits_buf = hype_dancer_hits;
        if any_caught {
            self.check_milestone(&mut rand::rng());
        }
    }

    /// Treasure payoff when a rare Golden Crab is snagged. On top of the normal catch award (already
    /// added in the catch loop), this pays a big lump-sum bonus and throws a gold sparkle-burst so
    /// the moment lands like finding treasure. The bonus scales with the combo multiplier so a
    /// golden grab mid-hot-streak is a genuine jackpot — the reward for committing to the chase.
    fn on_golden_caught(&mut self, pos: Vec2, base_pts: usize) {
        let mut rng = rand::rng();
        // Flat treasure bonus scaled by the current combo multiplier, floored so it always feels big.
        let bonus = (30 * self.combo_multiplier()).max(30);
        self.score += bonus;
        // Gold sparkle-burst + shockwave so the catch reads as a jackpot, not a normal snag.
        self.particle_system
            .spawn_milestone_fireworks(pos, 14, &mut rng);
        self.spawn_catch_shockwave(pos, [1.0, 0.85, 0.25]);
        self.floating_texts.spawn(
            format!("GOLDEN! +{}", bonus),
            pos - Vec2::new(60.0, 40.0),
            42.0,
            [1.0, 0.9, 0.3, 1.0],
        );
        // Extra juice: a short freeze, a camera punch, and a groove kick reward the risky chase.
        self.hitstop_timer = self.hitstop_timer.max(0.09);
        self.zoom_punch = self.zoom_punch.max(0.08);
        self.shake_timer = self.shake_timer.max(0.45);
        self.groove = (self.groove + 0.25).min(1.0);
        let _ = base_pts; // base points already banked in the catch loop; kept for future tuning.
    }

    /// Splitter cleave — the arrangement *bet*. Catching a Splitter cleaves the conga train at its
    /// midpoint and instantly BANKS the back half for points (a partial cash-out), leaving the front
    /// half as a shorter, re-indexed train that keeps rolling. It reuses the delivery payout curve
    /// (super-linear triangular sum) so cashing a slice at speed genuinely pays, and the peel-scatter
    /// juice so the cleave reads on screen. The "bet" is a timing gamble: catch the Splitter ON the beat
    /// for a clean cut (full bank + Jackpot on the slice composition), or OFF the beat for a sloppy cut
    /// (half bank, no jackpot). So you sacrifice the length and match-run shape you'd built for a slice of
    /// score — timed to the beat it's the big cash, off-beat it's a mediocre partial, so dodging a
    /// Splitter to keep building a run you'd only half-cash is a live call. Because the Splitter itself is
    /// the freshly-caught tail, it always lands in the banked back half (you never keep the cleaver).
    ///
    /// The Splitter also plugs into the archetype crossover web (its whole point — see the roadmap's
    /// "emergent web"): the *composition* of the cleaved back half pays a Jackpot Cleave — Goldens and
    /// Magnets in the slice, and a live tail match-run the cut cashes, each add a bonus and escalate the
    /// juice. So the bet is over what the tail is *made of*, not just how long it is: a mid-match-run
    /// cleave with a Golden parked in back is the big score; a bare cut is the safe partial cash-out.
    /// The links that would be cleaved: `(keep, banked)` where every chain_index >= keep banks and
    /// the front `keep` links stay attached. Split at the midpoint. Single source of truth for the
    /// cut point, shared by the cleave itself and the pre-catch stakes preview so they can never drift.
    fn cleave_split_point(&self) -> (usize, usize) {
        let keep = self.chain_count / 2;
        (keep, self.chain_count - keep)
    }

    /// What a CLEAN (on-beat) cleave would bank *right now*, base slice payout plus the full Jackpot
    /// crossover (Goldens/Magnets/cashed match-run in the back half). This is the exact number the
    /// clean branch of `split_train_bank` pays — extracted so the floating stakes preview shows the
    /// real bet, not a re-derived guess that silently diverges the next time the formula is edited.
    /// Returns `(worth, jackpot)`: `jackpot` is whether any composition crossover would fire.
    fn cleave_clean_worth(&self) -> (usize, bool) {
        if self.chain_count == 0 {
            return (0, false);
        }
        let (keep, banked) = self.cleave_split_point();
        let base = (banked * (banked + 1) / 2) * 3;
        let combo = self.combo_multiplier();
        let mut worth = (base as f32 * combo as f32 * self.beat_gamble_mult).round() as usize;

        let (golden_in_slice, magnet_in_slice) = self.crabs.iter().fold((0, 0), |(g, m), c| {
            if c.caught && c.chain_index.map_or(false, |ci| ci >= keep) {
                (g + c.is_golden() as usize, m + c.is_magnet() as usize)
            } else {
                (g, m)
            }
        });
        let cashed_run = if self.tail_run_len >= 3 {
            self.tail_run_len
        } else {
            0
        };
        let golden_bonus = golden_in_slice * 120 * combo;
        let magnet_bonus = if magnet_in_slice > 0 {
            magnet_in_slice * banked.max(1) * 6 * combo
        } else {
            0
        };
        let run_bonus = (cashed_run as usize) * (cashed_run as usize) * 5 * combo;
        let crossover = golden_bonus + magnet_bonus + run_bonus;
        worth += crossover;
        (worth, crossover > 0)
    }

    fn split_train_bank(&mut self, at: Vec2) {
        // Nothing to cleave a meaningful chunk out of — a 1-2 link train just banks whatever's there.
        if self.chain_count == 0 {
            return;
        }
        // Cleave point: everything at chain_index >= keep banks. Split at the midpoint, but always
        // bank at least the Splitter link itself (the tail) so the catch always does *something*.
        let (keep, banked) = self.cleave_split_point();

        // THE BET — an on-beat gate turns the cleave from pure upside into a genuine timing gamble.
        // Catch the Splitter ON the beat and the cut lands clean: full bank + the Jackpot payout below.
        // Catch it OFF the beat and the cut is sloppy — the back half still banks, but at HALF value and
        // with NO jackpot (Goldens/Magnets/match-run in the slice pay nothing extra). So grabbing a
        // Splitter mid-run is a real decision: time it to the beat to cash the slice for its full
        // Jackpot worth, or dodge it to keep building a run you'd only half-cash off the beat. We grade
        // it (half payout) rather than wiping the back half outright, because Splitters are usually
        // reeled in with pull tools (whistle/beam) where the player commits to the catch but not the
        // exact frame — a total loss on a ~68%-of-the-bar off-beat window would read as a punish, not a
        // bet. A soft miss keeps it a live risk/reward read without the feel-bad wipe of half your train.
        let clean = self.on_beat_now();

        // Component scan for the JACKPOT tag naming and juice below — WHICH crabs sit in the cleaved
        // back half (Goldens/Magnets, and a live tail match-run). Single pass over the banked slice.
        // The clean-cut TOTAL itself comes from cleave_clean_worth() so the preview tag and the actual
        // payout share exactly one formula and can't drift; here we only need the breakdown for naming.
        let (golden_in_slice, magnet_in_slice) = self.crabs.iter().fold((0, 0), |(g, m), c| {
            if c.caught && c.chain_index.map_or(false, |ci| ci >= keep) {
                (g + c.is_golden() as usize, m + c.is_magnet() as usize)
            } else {
                (g, m)
            }
        });
        // The tail match-run lives at the very back of the train, so the cleave always cashes it in
        // full — capture its length before recompute wipes it. Only counts as a "cashed run" at 3+.
        let cashed_run = if self.tail_run_len >= 3 {
            self.tail_run_len
        } else {
            0
        };
        let _ = magnet_in_slice; // consumed inside cleave_clean_worth; kept here only for parity of the scan

        // THE PAYOUT. On the beat the cut is clean: bank the full cleave_clean_worth() (base slice +
        // Jackpot crossover), the exact figure the pre-catch stakes tag previewed. Off the beat it's a
        // sloppy half-cut: half the base slice, no crossover — so timing is what turns a good tail into
        // a jackpot. Single source of truth for the clean total; off-beat is derived from the same base.
        let (bank, jackpot) = if clean {
            let (worth, jackpot) = self.cleave_clean_worth();
            (worth, jackpot)
        } else {
            let base = (banked * (banked + 1) / 2) * 3;
            let half = (base as f32 * self.combo_multiplier() as f32 * self.beat_gamble_mult * 0.5)
                .round() as usize;
            (half, false)
        };
        self.score += bank;

        // Collect the banked crabs (chain_index >= keep) so they can parade into the pen like a
        // normal delivery, then leave the field. self.crabs isn't index-ordered, so sort the banked
        // slice head-first for a clean parade. Cheap — only runs on the rare Splitter catch.
        let mut ordered: Vec<(usize, Vec2, [f32; 3], f32)> = self
            .crabs
            .iter()
            .filter(|c| c.caught && c.chain_index.map_or(false, |ci| ci >= keep))
            .map(|c| {
                (
                    c.chain_index.unwrap_or(usize::MAX),
                    c.pos,
                    c.crab_color(),
                    c.scale,
                )
            })
            .collect();
        ordered.sort_unstable_by_key(|&(ci, ..)| ci);
        let marching: Vec<(Vec2, [f32; 3], f32)> = ordered
            .into_iter()
            .map(|(_ci, p, col, s)| (p, col, s))
            .collect();
        // March the banked slice into the delivery pen, same as a real bank, so the cleave visibly
        // cashes out toward the pen rather than blinking away at the split point.
        self.penned_marchers.spawn_train(self.pen_pos, &marching);

        // Remove the banked crabs from the field entirely — they've been cashed. The front half
        // (chain_index < keep) stays attached and keeps its indices contiguous (0..keep), so the
        // shortened train and all future catches line up cleanly.
        self.crabs
            .retain(|c| !(c.caught && c.chain_index.map_or(false, |ci| ci >= keep)));
        self.chain_count = keep;
        self.recompute_tail_run(); // the tail changed (the whole back half, incl. any match run, is gone)

        // Feedback: a bright teal cleave-shockwave + fireworks at the split point and a legible
        // SPLIT BANKED callout, so the bet paying off reads on screen. A camera jolt sells the cleave.
        // When the cut lands a crossover (a Golden/Magnet in the slice or a live match-run cashed),
        // the moment escalates — gold shockwave, extra fireworks, a bigger kick, and a JACKPOT
        // CLEAVE callout naming what paid — so "oh, THAT happened" reads at a glance.
        // Cleave slash: a blade stroke from the last kept front link to the split point, drawn for a
        // few frames so the cut visibly bisects the train. The front endpoint is the link that's now
        // the new tail (chain_index == keep-1); if there's no front half left, slash from `at` itself.
        let front_tail = if keep > 0 {
            self.crabs
                .iter()
                .find(|c| c.caught && c.chain_index == Some(keep - 1))
                .map(|c| c.pos)
                .unwrap_or(at)
        } else {
            at
        };
        self.cleave_a = front_tail;
        self.cleave_b = at;
        self.cleave_flash = 1.0;
        self.cleave_gold = jackpot;

        let mut rng = rand::rng();
        let (shock_col, extra_bursts) = if jackpot {
            ([1.0, 0.85, 0.25], banked.max(1) + 6)
        } else {
            ([0.2, 0.95, 0.85], banked.max(1))
        };
        self.particle_system
            .spawn_milestone_fireworks(at, extra_bursts, &mut rng);
        self.spawn_catch_shockwave(at, shock_col);
        if jackpot {
            // Name the payoff so the crossover reads, biggest contributor first.
            let tag = if cashed_run > 0 {
                format!("JACKPOT CLEAVE! RUN x{} +{}", cashed_run, bank)
            } else if golden_in_slice > 0 {
                format!("JACKPOT CLEAVE! GOLD +{}", bank)
            } else {
                format!("JACKPOT CLEAVE! MAGNET +{}", bank)
            };
            self.floating_texts.spawn(
                tag,
                at - Vec2::new(110.0, 42.0),
                46.0,
                [1.0, 0.9, 0.35, 1.0],
            );
            self.screen_shake = self.screen_shake.max(13.0);
            // Directional kick away from the player — the cleave "recoils" outward so the cut
            // has a felt direction, not just omnidirectional rumble.
            {
                let kick_dir = (at - self.player_pos).try_normalize().unwrap_or(Vec2::X);
                let vel = kick_dir * 13.0 * 60.0;
                if self.screen_shake_vel.length_squared() < vel.length_squared() {
                    self.screen_shake_vel = vel;
                }
            }
            self.hitstop_timer = self.hitstop_timer.max(0.1);
            self.zoom_punch = self.zoom_punch.max(0.085);
            self.on_beat_flash = self.on_beat_flash.max(0.4);
            self.groove = (self.groove + 0.15).min(1.0);
        } else if clean {
            self.floating_texts.spawn(
                format!("SPLIT BANKED +{}", bank),
                at - Vec2::new(70.0, 40.0),
                44.0,
                [0.4, 1.0, 0.9, 1.0],
            );
            self.screen_shake = self.screen_shake.max(8.0);
            {
                let kick_dir = (at - self.player_pos).try_normalize().unwrap_or(Vec2::X);
                let vel = kick_dir * 8.0 * 60.0;
                if self.screen_shake_vel.length_squared() < vel.length_squared() {
                    self.screen_shake_vel = vel;
                }
            }
            self.hitstop_timer = self.hitstop_timer.max(0.07);
            self.zoom_punch = self.zoom_punch.max(0.05);
        } else {
            // Off-beat: the sloppy cut reads as a miss — a dimmer, redder callout naming the lost value
            // (half bank, no jackpot) so the player learns to time the Splitter to the beat next time.
            self.floating_texts.spawn(
                format!("SLOPPY CUT +{}", bank),
                at - Vec2::new(70.0, 40.0),
                40.0,
                [1.0, 0.6, 0.45, 1.0],
            );
            self.screen_shake = self.screen_shake.max(5.0);
            {
                let kick_dir = (at - self.player_pos).try_normalize().unwrap_or(Vec2::X);
                let vel = kick_dir * 5.0 * 60.0;
                if self.screen_shake_vel.length_squared() < vel.length_squared() {
                    self.screen_shake_vel = vel;
                }
            }
            self.hitstop_timer = self.hitstop_timer.max(0.05);
            self.zoom_punch = self.zoom_punch.max(0.03);
        }
    }

    /// Crossover payoff — the Magnet-link shine cascade. Fires when a Golden is caught directly
    /// behind a Magnet link in the train (a catch *order* the player sets up on purpose: park a
    /// Magnet at the tail, then chase a Golden onto it). The Magnet's field conducts the Golden's
    /// shine down the entire conga line, paying a bonus that scales with how long the train is —
    /// so the longer the line you've routed the shine through, the bigger the reward — and firing a
    /// gold whip-streak that visibly ripples from the tail up to the head so the cascade reads on
    /// screen. Reuses the existing catch-trail whip streak (no new draw path) per the "reuse
    /// existing verbs, make it a legible watchable reaction" spirit of the roadmap item.
    fn on_magnet_shine_cascade(&mut self, golden_pos: Vec2) {
        // Bonus scales with the number of links the shine travels through — the whole point is that
        // a longer train you've deliberately built pays off more. Floored so even a short line feels
        // worth the setup, and scaled by the live combo multiplier like the other catch rewards.
        let links = self.chain_count.max(1);
        let bonus = (8 * links * self.combo_multiplier()).max(40);
        self.score += bonus;

        // Collect the caught-train positions ordered head->tail so we can chain gold whip-streaks
        // link-to-link. O(n) + sort, but this only runs on the rare engineered Magnet+Golden catch,
        // so it's off the hot path. Reuses the pooled deflect_body_buf's sibling pattern via a fresh
        // small local — the cascade is rare enough that a one-off Vec here is fine.
        let mut links_sorted: Vec<(usize, Vec2)> = self
            .crabs
            .iter()
            .filter_map(|c| c.chain_index.map(|ci| (ci, c.pos)))
            .collect();
        links_sorted.sort_unstable_by_key(|&(ci, _)| ci);

        // Whip-streaks hopping from each link to the next, staggered so the shine visibly travels
        // from the tail (where the Golden joined) up toward the head. Later hops start "younger"
        // (more negative age) so they light up after the ones nearer the tail — a rolling cascade.
        const SHINE: [f32; 3] = [1.0, 0.9, 0.35];
        let n = links_sorted.len();
        for i in (1..n).rev() {
            if self.catch_trails.len() >= 48 {
                break;
            }
            let from = links_sorted[i].1;
            let to = links_sorted[i - 1].1;
            // Tail hop starts now; each hop toward the head is delayed a hair for the ripple.
            let stagger = -0.04 * (n - i) as f32;
            self.catch_trails.push((from, to, stagger.max(-0.6), SHINE));
        }

        // Punchy feedback so the cascade lands as a moment, not a silent score bump: a gold
        // shockwave at the tail, a length-aware callout, fireworks, and a beat/camera kick.
        self.spawn_catch_shockwave(golden_pos, SHINE);
        self.particle_system
            .spawn_milestone_fireworks(golden_pos, 12, &mut rand::rng());
        self.floating_texts.spawn(
            format!("SHINE CASCADE! +{}  ({} links)", bonus, links),
            golden_pos - Vec2::new(90.0, 58.0),
            40.0,
            [1.0, 0.92, 0.4, 1.0],
        );
        self.zoom_punch = self.zoom_punch.max(0.09);
        self.hitstop_timer = self.hitstop_timer.max(0.08);
        self.screen_shake = self.screen_shake.max(10.0);
        self.on_beat_flash = self.on_beat_flash.max(0.4);
        self.groove = (self.groove + 0.2).min(1.0);
    }

    /// Big celebratory payoff when a worn-down boss is finally snagged. `is_tide` swaps the callout
    /// and shockwave color so the Tide Boss reads as its own catch, not a reskinned King Crab.
    fn on_boss_caught(&mut self, pos: Vec2, is_tide: bool) {
        let mut rng = rand::rng();
        let bonus = 25 * self.combo_multiplier();
        self.score += bonus;
        self.particle_system
            .spawn_milestone_fireworks(pos, 30, &mut rng);
        // World-layer text: anchor to the player so the boss-caught banner reads on-screen under
        // the scrolling camera rather than at a fixed world coordinate.
        let screen_center = self.player_pos + Vec2::new(-200.0, -170.0);
        let (label, label_color, shock_color): (&str, [f32; 4], [f32; 3]) = if is_tide {
            ("TIDE BOSS CAUGHT!", [0.4, 0.85, 1.0, 1.0], [0.3, 0.75, 1.0])
        } else {
            ("KING CRAB CAUGHT!", [1.0, 0.85, 0.2, 1.0], [1.0, 0.8, 0.2])
        };
        self.floating_texts.spawn(
            label.to_string(),
            screen_center + Vec2::new(3.0, 3.0),
            64.0,
            [0.0, 0.0, 0.0, 0.85],
        );
        self.floating_texts
            .spawn(label.to_string(), screen_center, 64.0, label_color);
        self.floating_texts.spawn(
            format!("+{}", bonus),
            pos - Vec2::new(20.0, 30.0),
            40.0,
            [1.0, 0.95, 0.3, 1.0],
        );
        let a = rng.random_range(0.0_f32..std::f32::consts::TAU);
        self.screen_shake = 30.0;
        self.screen_shake_vel = Vec2::new(a.cos(), a.sin()) * 30.0 * 60.0;
        self.zoom_punch = self.zoom_punch.max(0.11);
        self.hitstop_timer = self.hitstop_timer.max(0.12);
        // The hard-freeze punch lands first; once it clears, bullet-time takes over so the whole
        // victory — fireworks, the boss's last flail, the arena healing — plays out in slow motion.
        self.slowmo_timer = SLOWMO_DURATION;
        self.beat_intensity = 2.0;
        self.on_beat_flash = 0.6;
        if self.catch_shockwaves.len() < 48 {
            self.catch_shockwaves.push((pos, 0.0, shock_color));
        }

        // The duel's over: the arena the boss reshaped heals. King Crab fissures seal (with a puff
        // of receding light) and any flood water the Tide Boss surged in recedes back off, leaving
        // only the biome's own pools. Recede exactly `boss_flood_pools` from the tail of the vec —
        // flood pools are always appended, so they're the last N entries.
        for &(fc, _, _) in &self.boss_fissures {
            if self.catch_shockwaves.len() < 48 {
                self.catch_shockwaves.push((fc, 0.0, [1.0, 0.6, 0.2]));
            }
        }
        self.boss_fissures.clear();
        self.boss_fissure_erupt = 0.0;
        if self.boss_flood_pools > 0 {
            let drain = self.boss_flood_pools.min(self.tide_pools.len());
            let new_len = self.tide_pools.len() - drain;
            self.tide_pools.truncate(new_len);
            self.boss_flood_pools = 0;
        }
    }

    /// King Crab enrage set-piece: the boss slams the seabed and CRACKS THE FLOOR, splitting the
    /// arena into a scatter of glowing fissures the player must weave the conga tail around for the
    /// rest of the duel (see `damage_tail_in_fissures`). Fissures are kept off the delivery pen (so
    /// banking never becomes a coin flip), off the boss's own spot, and spaced apart so they read as
    /// distinct lanes to thread rather than one big kill zone. Cleared when the boss is caught.
    fn crack_arena_fissures(&mut self, boss_pos: Vec2) {
        let mut rng = rand::rng();
        let count = 5;
        let mut placed = 0;
        let mut attempts = 0;
        while placed < count && attempts < 60 {
            attempts += 1;
            let radius = rng.random_range(56.0..92.0);
            let margin = radius + 30.0;
            // Cracks well up around the boss/fight, not across the whole (now larger-than-viewport)
            // world — sample a ring around the boss so the set-piece reshapes the arena the player
            // is standing in, clamped to world bounds.
            let ang = rng.random_range(0.0..std::f32::consts::TAU);
            let dist = rng.random_range(0.0..self.height * 0.45);
            let c = Vec2::new(
                (boss_pos.x + ang.cos() * dist).clamp(margin, self.world_width - margin),
                (boss_pos.y + ang.sin() * dist).clamp(margin, self.world_height - margin),
            );
            if c.distance(self.pen_pos) < radius + PEN_RADIUS + 50.0 {
                continue;
            }
            if c.distance(boss_pos) < radius + 90.0 {
                continue;
            }
            if self
                .boss_fissures
                .iter()
                .any(|(fc, fr, _)| c.distance(*fc) < radius + fr + 60.0)
            {
                continue;
            }
            self.boss_fissures.push((c, radius, 0.0));
            placed += 1;
        }
        // A loud callout so the player reads the arena change, not just "the boss got faster".
        self.floating_texts.spawn(
            "THE FLOOR CRACKS!".to_string(),
            boss_pos - Vec2::new(120.0, 92.0),
            34.0,
            [1.0, 0.5, 0.15, 1.0],
        );
    }

    /// Tide Boss enrage set-piece: the arena FLOODS. The boss surges the water level, appending a
    /// handful of extra wade-drag pools to the level's own `tide_pools` so the whole space suddenly
    /// routes differently — the safe lanes you'd learned are underwater now. We remember how many we
    /// added (`boss_flood_pools`) so catching the boss can recede exactly the flood water without
    /// disturbing the biome's native pools. Flood pools avoid the pen and the boss's own position.
    fn flood_arena(&mut self, boss_pos: Vec2) {
        let mut rng = rand::rng();
        let count = 4;
        let mut placed = 0;
        let mut attempts = 0;
        while placed < count && attempts < 60 {
            attempts += 1;
            let radius = rng.random_range(80.0..130.0);
            let margin = radius + 30.0;
            // Flood wells up around the boss/fight, clamped to world bounds — see spawn_boss_fissures.
            let ang = rng.random_range(0.0..std::f32::consts::TAU);
            let dist = rng.random_range(0.0..self.height * 0.5);
            let c = Vec2::new(
                (boss_pos.x + ang.cos() * dist).clamp(margin, self.world_width - margin),
                (boss_pos.y + ang.sin() * dist).clamp(margin, self.world_height - margin),
            );
            if c.distance(self.pen_pos) < radius + PEN_RADIUS + 40.0 {
                continue;
            }
            if c.distance(boss_pos) < radius + 80.0 {
                continue;
            }
            if self
                .tide_pools
                .iter()
                .any(|(pc, pr)| c.distance(*pc) < radius + pr + 40.0)
            {
                continue;
            }
            self.tide_pools.push((c, radius));
            self.boss_flood_pools += 1;
            placed += 1;
            // A cold burst of splash where each new pool wells up.
            self.spawn_catch_shockwave(c, [0.3, 0.7, 1.0]);
        }
        self.floating_texts.spawn(
            "THE ARENA FLOODS!".to_string(),
            boss_pos - Vec2::new(120.0, 92.0),
            34.0,
            [0.4, 0.85, 1.0, 1.0],
        );
    }

    /// While a King Crab's enrage fissures are open, the conga tail is at risk if it's dragged
    /// through one — the cracked floor bites off the last few links, the same self-limiting way the
    /// panic snap and kelp snag do (only long trains, only the tail, gated by the shared cooldown).
    /// This is the teeth behind the arena-crack set-piece: the fissures aren't decoration, they make
    /// routing the train the thing you sweat over in the boss's final phase.
    fn damage_tail_in_fissures(&mut self, dt: f32) {
        const MIN_TRAIN_TO_SNAP: usize = 5;
        const SNAP_LINKS: usize = 2;
        const SNAP_COOLDOWN: f32 = 1.8;
        const SNAP_CHANCE_PER_SEC: f32 = 0.8;

        if self.boss_fissures.is_empty()
            || self.chain_snap_cooldown > 0.0
            || self.chain_count < MIN_TRAIN_TO_SNAP
        {
            return;
        }
        // Reuses the tail position update_crabs already computed this frame instead of rescanning.
        let Some(tail_pos) = self.cached_tail_pos else {
            return;
        };
        // The geyser makes the hazard breathe with the beat: while a fissure is erupting its bite
        // reach swells past the rim (so a tail merely skirting the edge gets caught mid-spout) and
        // the snap becomes far likelier. Between beats the reach recedes to the rim and the bite
        // goes nearly dormant — so the safe move is to thread the tail across in the gaps, not on
        // the hit. `erupt` is the shared beat pulse; its peak is right on the beat.
        let erupt = self.boss_fissure_erupt.clamp(0.0, 1.0);
        let reach = 1.0 + 0.35 * erupt; // danger radius grows up to 1.35x on the beat
        // Only bite if the tail is inside a (possibly geyser-widened) open fissure — weave and you're safe.
        let in_fissure = self.boss_fissures.iter().any(|(c, r, age)| {
            *age > 0.6 && tail_pos.distance_squared(*c) < (*r * reach) * (*r * reach)
        });
        if !in_fissure {
            return;
        }
        // Between beats the pit is nearly dormant (a small baseline bite), on the beat it snaps
        // hard — so the eruption is what the player actually dodges.
        let snap_chance = SNAP_CHANCE_PER_SEC * (0.15 + 0.85 * erupt);
        if rand::random::<f32>() > snap_chance * dt {
            return;
        }

        let keep = self.chain_count.saturating_sub(SNAP_LINKS).max(1);
        let snapped = self.chain_count - keep;
        let mut snapped_positions: Vec<Vec2> = Vec::new();
        for crab in &mut self.crabs {
            let Some(ci) = crab.chain_index else { continue };
            if ci >= keep {
                crab.caught = false;
                crab.chain_index = None;
                crab.fleeing = true;
                crab.startle_timer = 0.6;
                let outward = (crab.pos - tail_pos).normalize_or_zero();
                let outward = if outward == Vec2::ZERO {
                    Vec2::new(0.0, 1.0)
                } else {
                    outward
                };
                crab.vel = outward * crab.crab_type.speed_range().end * 1.8;
                crab.speed = 1.0;
                snapped_positions.push(crab.pos);
            }
        }
        self.chain_count = keep;
        self.recompute_tail_run(); // the tail changed — rebuild the same-type run
        self.chain_snap_cooldown = SNAP_COOLDOWN;

        for pos in &snapped_positions {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((*pos, 0.0));
            }
            self.floating_texts.spawn(
                "!".to_string(),
                *pos - Vec2::new(0.0, 24.0),
                24.0,
                [1.0, 0.55, 0.2, 1.0],
            );
        }
        self.floating_texts.spawn(
            format!("SWALLOWED!  -{}", snapped),
            tail_pos - Vec2::new(40.0, 32.0),
            30.0,
            [1.0, 0.5, 0.15, 1.0],
        );
        self.spawn_catch_shockwave(tail_pos, [1.0, 0.5, 0.15]);
        self.screen_shake = self.screen_shake.max(6.0);
    }

    /// A Tide Boss pulse detonates at `center`: an expanding shockwave ring that shoves every
    /// nearby *free* crab outward into a panic, and — if the conga train's tail is caught inside the
    /// blast — knocks the last few links loose (the Tide Boss's version of a chain snap). The threat
    /// is spacing: keep your train out of the ring and the pulse does nothing, so it rewards reading
    /// the swell telegraph and pulling back rather than routing out of a charge lane.
    fn tide_pulse_burst(&mut self, center: Vec2) {
        const TIDE_SNAP_LINKS: usize = 4; // a solid surge tears off a bit more than a panic-brush snap
        // Archetype-in-boss crossover: a Magnet ANCHORS against the surge. A free Magnet caught in the
        // blast isn't flung out like everything else — the wall of water charges its lodestone (the same
        // supercharge a snared Golden buys it), and its widened vacuum re-balls the herd the pulse just
        // scattered next frame. The payoff is defensive too: if that supercharged field covers your
        // conga tail, it pins those links against the shove and the chain-snap is called off. So parking
        // a Magnet by your train turns the Tide Boss's own crowd-scatter into a re-gather and a shield —
        // the Magnet (routing) archetype finally matters inside the water fight.
        const MAGNET_ANCHOR_RADIUS: f32 = 240.0; // matches the Magnet's normal pull reach
        const MAGNET_ANCHOR_RADIUS_SQ: f32 = MAGNET_ANCHOR_RADIUS * MAGNET_ANCHOR_RADIUS;
        let r2 = TIDE_PULSE_RADIUS * TIDE_PULSE_RADIUS;

        // Spawn the visible expanding ring (bounded so a stall can't grow the Vec without limit).
        if self.tide_pulses.len() < 8 {
            self.tide_pulses.push((center, crate::CRAB_SIZE));
        }

        // OFFENSIVE archetype-in-boss crossover — the GOLDEN SLINGSHOT. The Tide Boss is otherwise
        // fought in a bubble; this is the player's active play *against* it, the mirror of the King
        // Crab's bait-into-Armored stun and the Reef DJ's hype-Dancer chip. Setup: lure a fleeing
        // Golden into a free Magnet's field (the existing snare→supercharge crossover) and park that
        // loaded Magnet where the boss's swell will wash over it. When the surge hits, instead of
        // scattering the Magnet's catch, the wall of water FIRES the pinned Golden's shine straight
        // through the lodestone and into the boss — a bright lance that cracks a big chunk off the
        // shell far faster than the beam ever could. It's a real reason to spend the whole telegraph
        // wrangling a Golden into position rather than just backing the train out of the ring, and
        // it's a legible, watchable moment (gold streak → boss stagger) for the videos Carl wants.
        const SLINGSHOT_CHIP: f32 = 0.7; // ~a bar of beam per shot — a deliberate setup deserves a real dent
        // Reused scratch buffers instead of fresh Vec::new() per pulse — see field docs.
        let mut slingshots = std::mem::take(&mut self.pulse_slingshots_buf);
        slingshots.clear();
        {
            // A Magnet is "loaded" if it's charged (pinning shine) and a snared Golden sits inside its
            // reach — the same pairing the charged-magnet pass already recognizes elsewhere. Collect
            // loaded pairs the surge washes over, sparing them from the scatter below (they fire, not flee).
            let mut loaded_magnets = std::mem::take(&mut self.pulse_loaded_magnets_buf);
            loaded_magnets.clear();
            for m in &self.crabs {
                if m.caught || m.is_boss() || !m.is_magnet() || m.magnet_charged <= 0.0 {
                    continue;
                }
                if m.pos.distance_squared(center) > r2 {
                    continue; // only Magnets the swell actually reaches can be fired by it
                }
                // Find a snared Golden this Magnet is holding (nearest inside its pull reach).
                let mut fired_golden: Option<Vec2> = None;
                for g in &self.crabs {
                    if g.caught || !g.is_golden() || g.magnet_snared <= 0.0 {
                        continue;
                    }
                    if g.pos.distance_squared(m.pos) <= MAGNET_ANCHOR_RADIUS_SQ {
                        fired_golden = Some(g.pos);
                        break;
                    }
                }
                if let Some(gpos) = fired_golden {
                    loaded_magnets.push(m.pos);
                    slingshots.push((m.pos, gpos));
                }
            }
            // Chip the live Tide Boss once per shot, and consume the Golden the surge spent (it's
            // flung out of the snare into a flee — the shot expends the prize, so the play is a
            // trade: give up the Golden catch for a big crack in the shell).
            if !slingshots.is_empty() {
                let mut broke_at: Option<Vec2> = None;
                let mut boss_pos: Option<Vec2> = None;
                for crab in &mut self.crabs {
                    if crab.is_tide_boss() && !crab.caught && crab.boss_health > 0.0 {
                        boss_pos = Some(crab.pos);
                        crab.boss_health =
                            (crab.boss_health - SLINGSHOT_CHIP * slingshots.len() as f32).max(0.0);
                        if crab.boss_health <= 0.0 {
                            broke_at = Some(crab.pos);
                        }
                        break;
                    }
                }
                // A bright gold lance streaks from each fired Golden into the boss — the reused
                // catch-trail plumbing (from → to, retracting, self-expiring) gives it the watchable
                // "shot connects" beat for free. Only fires when a live boss actually took the hit.
                if let Some(bpos) = boss_pos {
                    for &(_, gpos) in &slingshots {
                        if self.catch_trails.len() < 48 {
                            self.catch_trails
                                .push((gpos, bpos, -0.25, [1.0, 0.85, 0.25]));
                        }
                    }
                }
                // Spend each fired Golden — the shot expends the prize (the whole point of the trade).
                // Release the snare AND set slingshot_spent so the Magnet field can't re-snare it next
                // frame (see the Golden re-snare pass), and fling it outward from the boss under its own
                // velocity so it visibly leaves the field rather than reloading in place. Without the
                // spent-window the anchor/re-snare passes would keep it loaded and the chip would repeat
                // every pulse from one setup — turning a deliberate one-shot into a beam-free boss kill.
                for &(_, gpos) in &slingshots {
                    for crab in &mut self.crabs {
                        if crab.is_golden()
                            && !crab.caught
                            && crab.magnet_snared > 0.0
                            && crab.pos == gpos
                        {
                            crab.magnet_snared = 0.0;
                            crab.slingshot_spent = 1.2; // ~a couple beats of no-reload while it clears the field
                            crab.fleeing = true;
                            crab.startle_timer = crab.startle_timer.max(0.5);
                            let away = (crab.pos - center).normalize_or_zero();
                            let away = if away == Vec2::ZERO {
                                Vec2::new(0.0, 1.0)
                            } else {
                                away
                            };
                            crab.vel = away * crab.crab_type.speed_range().end * 2.0;
                            crab.speed = 1.0;
                            break;
                        }
                    }
                }
                for &(mpos, _) in &slingshots {
                    self.floating_texts.spawn(
                        "SLINGSHOT!".to_string(),
                        mpos - Vec2::new(55.0, 40.0),
                        30.0,
                        [1.0, 0.85, 0.3, 1.0],
                    );
                    self.particle_system
                        .spawn_milestone_fireworks(mpos, 10, &mut rand::rng());
                }
                self.screen_shake = self.screen_shake.max(10.0);
                self.on_beat_flash = self.on_beat_flash.max(0.35);
                if let Some(bpos) = broke_at {
                    self.floating_texts.spawn(
                        "WASHED DOWN — CATCH IT!".to_string(),
                        bpos - Vec2::new(120.0, 46.0),
                        34.0,
                        [0.4, 1.0, 0.5, 1.0],
                    );
                    self.spawn_catch_shockwave(bpos, [0.3, 0.75, 1.0]);
                    self.screen_shake = self.screen_shake.max(14.0);
                }
            }
            self.pulse_loaded_magnets_buf = loaded_magnets;
        }

        // First pass: supercharge every free Magnet the surge washes over, and remember where each
        // anchoring field sits so the shove and the snap below can spare crabs inside it.
        let mut anchor_positions = std::mem::take(&mut self.pulse_anchor_positions_buf);
        anchor_positions.clear();
        for crab in &mut self.crabs {
            if crab.caught || crab.is_boss() || !crab.is_magnet() {
                continue;
            }
            if crab.pos.distance_squared(center) > r2 {
                continue;
            }
            // The wall of water charges the lodestone — same state a snared Golden grants, so the
            // existing charged-radius vacuum pass re-gathers the scattered herd and the aura flares gold.
            crab.magnet_charged = crab.magnet_charged.max(1.6);
            if anchor_positions.len() < 8 {
                anchor_positions.push(crab.pos);
            }
        }
        let anchored = |pos: Vec2| {
            anchor_positions
                .iter()
                .any(|a| a.distance_squared(pos) <= MAGNET_ANCHOR_RADIUS_SQ)
        };

        // Shove every free crab in range outward and startle it into a flee — unless a Magnet's
        // charged field holds it in place.
        let mut scattered = std::mem::take(&mut self.pulse_scattered_buf);
        scattered.clear();
        for crab in &mut self.crabs {
            if crab.caught || crab.is_boss() {
                continue;
            }
            let d2 = crab.pos.distance_squared(center);
            if d2 > r2 {
                continue;
            }
            if !crab.is_magnet() && anchored(crab.pos) {
                continue; // pinned by a nearby anchoring Magnet — the vacuum holds it against the surge
            }
            let outward = (crab.pos - center).normalize_or_zero();
            let outward = if outward == Vec2::ZERO {
                Vec2::new(0.0, 1.0)
            } else {
                outward
            };
            crab.fleeing = true;
            crab.startle_timer = crab.startle_timer.max(0.7);
            crab.charm_timer = 0.0; // the surge overwhelms a whistle's calm
            crab.vel = outward * crab.crab_type.speed_range().end * 2.0;
            crab.speed = 1.0; // vel encodes full speed, matching the flee/startle convention
            if scattered.len() < 24 {
                scattered.push(crab.pos);
            }
        }

        // A Magnet field over the tail calls off the wash-out entirely — feedback for the save.
        let tail_anchored = !anchor_positions.is_empty()
            && self.crabs.iter().any(|c| {
                c.caught
                    && c.chain_index.is_some()
                    && c.pos.distance_squared(center) <= r2
                    && anchored(c.pos)
            });
        if tail_anchored {
            self.floating_texts.spawn(
                "ANCHORED!".to_string(),
                center - Vec2::new(50.0, 34.0),
                30.0,
                [0.95, 0.55, 0.2, 1.0],
            );
        }
        // Release the borrow on anchor_positions so we can move it back at the end.
        drop(anchored);

        // Knock the tail loose if any caught link sits inside the blast. Mirrors snap_chain_on_panic
        // but triggered by the pulse's reach rather than a physical tail collision. A Magnet anchoring
        // the tail (tail_anchored) pins the links and cancels the snap.
        let tail_in_blast = !tail_anchored
            && self.crabs.iter().any(|c| {
                c.caught && c.chain_index.is_some() && c.pos.distance_squared(center) <= r2
            });
        if tail_in_blast && self.chain_count >= 5 && self.chain_snap_cooldown <= 0.0 {
            let keep = self.chain_count.saturating_sub(TIDE_SNAP_LINKS).max(1);
            let snapped = self.chain_count - keep;
            let mut snapped_positions = std::mem::take(&mut self.pulse_snapped_positions_buf);
            snapped_positions.clear();
            for crab in &mut self.crabs {
                let Some(ci) = crab.chain_index else { continue };
                if ci >= keep {
                    crab.caught = false;
                    crab.chain_index = None;
                    crab.fleeing = true;
                    crab.startle_timer = 0.6;
                    let outward = (crab.pos - center).normalize_or_zero();
                    let outward = if outward == Vec2::ZERO {
                        Vec2::new(0.0, 1.0)
                    } else {
                        outward
                    };
                    crab.vel = outward * crab.crab_type.speed_range().end * 2.2;
                    crab.speed = 1.0;
                    snapped_positions.push(crab.pos);
                }
            }
            self.chain_count = keep;
            self.recompute_tail_run(); // the tail changed — rebuild the same-type run
            self.chain_snap_cooldown = 1.6;
            for pos in &snapped_positions {
                if self.fear_rings.len() < 32 {
                    self.fear_rings.push((*pos, 0.0));
                }
            }
            self.floating_texts.spawn(
                format!("WASHED OUT!  -{}", snapped),
                center - Vec2::new(60.0, 34.0),
                32.0,
                [0.5, 0.85, 1.0, 1.0],
            );
            self.pulse_snapped_positions_buf = snapped_positions;
        }

        // Feedback for the scattered herd.
        for pos in &scattered {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((*pos, 0.0));
            }
        }
        // Hand the scratch buffers back to self so next pulse reuses their capacity.
        self.pulse_slingshots_buf = slingshots;
        self.pulse_anchor_positions_buf = anchor_positions;
        self.pulse_scattered_buf = scattered;
        self.spawn_catch_shockwave(center, [0.3, 0.75, 1.0]);
        self.screen_shake = self.screen_shake.max(16.0);
        let a = rand::rng().random_range(0.0_f32..std::f32::consts::TAU);
        self.screen_shake_vel = Vec2::new(a.cos(), a.sin()) * 12.0 * 60.0;
        self.on_beat_flash = self.on_beat_flash.max(0.35);
    }

    /// Live catch reach applied around every conga link this frame: base + the lasso/upgrade bump +
    /// the transient on-beat bloom (widest on the downbeat, decayed between beats). Kept in one place
    /// so the gameplay value and the drawn ring can't drift apart.
    fn catch_radius(&self) -> f32 {
        (45.0 + self.catch_radius_upgrade + self.beat_catch_bloom) * self.weather_catch_mult()
    }

    /// Ambience multiplier on the catch radius — subtle, never punishing. Rain/Storm make crabs
    /// harder to spot (down to ~-13% at full Storm), night dims the beam a touch (~-6% at deep
    /// night), and a Storm lightning flash briefly floods light back in (a short catch-radius
    /// spike). All three fold into one factor so the gameplay number and the drawn ring stay in
    /// lockstep. Clamped so upgrades always dominate.
    fn weather_catch_mult(&self) -> f32 {
        let rain = self.weather_intensity.clamp(0.0, 1.0) * 0.13;
        let night = self.night_factor() * 0.06;
        // Lightning flash illuminates a wider area for its ~0.5s life.
        let flash = self.lightning_flash.clamp(0.0, 1.0) * 0.30;
        (1.0 - rain - night + flash).clamp(0.80, 1.35)
    }

    /// 0 in daylight, ramping to 1 at deepest night — shared by the catch-radius dim and the
    /// beat-pulse brighten so "night" reads consistently in feel and visuals.
    fn night_factor(&self) -> f32 {
        // day_phase_t: 0=dawn .25=day .5=dusk .75→1=night. Night ramps from dusk onward.
        ((self.day_phase_t - 0.5) / 0.5).clamp(0.0, 1.0)
    }

    /// Day/night ground+sky tint: a warm→bright→orange→deep-blue color the world is graded toward.
    /// Returned as (r,g,b) multipliers in 0..1 applied on top of the biome tint, plus an ambient
    /// brightness scalar. Kept subtle so gameplay reads clearly at every phase.
    fn day_tint(&self) -> (f32, f32, f32) {
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

    fn update_crabs(&mut self, dt: f32, area: (f32, f32)) {
        // Calculate flashlight direction.
        let flashlight_dir = (self.mouse_pos - self.player_pos).normalize_or_zero();

        let base_cone_angle = std::f32::consts::FRAC_PI_3;
        let base_range = 320.0;

        let mut flashlight_cone_angle = base_cone_angle + self.flashlight.cone_upgrade;
        let mut flashlight_range = base_range + self.flashlight.range_upgrade;
        // Drum Roll fired blast: while the release window is live, the beam FLARES WIDE and FAR down
        // the aim — the fired charge (drum_roll_power) scales how much. This reuses the existing beam
        // catch path below (the cone/range tests at ~3348 and ~3616) instead of a second scan over
        // the crabs, so every free crab caught in the widened aimed arc snaps in exactly like a
        // normal beam catch — no parallel catch loop, no double-catch. Directional, not radial: it's
        // a big sweep down where you're pointing, distinct from the Downbeat Slam's all-around yank.
        if self.drum_roll_fire > 0.0 {
            let boost = self.drum_roll_fire * (self.drum_roll_power as f32 / DRUM_ROLL_MAX as f32);
            flashlight_cone_angle += boost * std::f32::consts::FRAC_PI_3; // up to +60° half-angle at full power
            flashlight_range += boost * 260.0; // up to +260px reach at full power
        }
        // Beam-lane-scaled boss/shell drain, read once so the &mut self.crabs loop can use it.
        let boss_drain = self.boss_drain_rate();
        // Drum Roll fired blast → a boss-shell CRACKER. While the release window is live, the beam
        // doesn't just widen (above) — it hammers a boss shell far harder than a held beam, scaled by
        // the charge power banked at fire. This is the rhythm verb pulled *into* the boss duel: a
        // real reason to spend a bar charging mid-fight instead of only using it to sweep the herd.
        // Read once here so the &mut self.crabs loop can fold it into the existing gated drain path
        // below (line ~3512) rather than a parallel damage pass — crucially, that keeps it *inside*
        // `drain_active`, so against the call-locked Reef DJ the blast only bites on a hot beat and
        // its echo-the-phrase identity is preserved instead of being cracked off-phrase.
        let drum_roll_boss_mult = if self.drum_roll_fire > 0.0 {
            1.0 + 6.0 * (self.drum_roll_power as f32 / DRUM_ROLL_MAX as f32)
        } else {
            1.0
        };

        // Event-collection scratch buffers, reused every frame (see field docs) instead of
        // being freshly allocated here — most frames leave every one of these empty. Taken out
        // (rather than borrowed) so the later celebration loops are free to call back into
        // methods that need a full `&mut self`; the buffers (and their capacity) are restored
        // at the end of this function so next frame reuses the same allocation.
        // Positions of crabs that just entered panic-flee this frame — we'll emit "!" pops after the loop
        let mut flee_pops = std::mem::take(&mut self.flee_pops_buf);
        flee_pops.clear();
        // Golden crabs a roaming Magnet's field just snared this frame — celebrated after the loop.
        let mut golden_snare_pops = std::mem::take(&mut self.golden_snare_pops_buf);
        golden_snare_pops.clear();
        let mut thief_snare_pops = std::mem::take(&mut self.thief_snare_pops_buf);
        thief_snare_pops.clear();
        let mut magnet_lure_pops = std::mem::take(&mut self.magnet_lure_pops_buf);
        magnet_lure_pops.clear();
        // Emergent crossover — Armored shells a charged Magnet's widened vacuum ground open this
        // frame (see the grind branch in the per-crab loop below). Collected here so the chip/crack
        // feedback fires after the &mut self.crabs borrow ends.
        let mut magnet_grind = std::mem::take(&mut self.magnet_grind_buf);
        magnet_grind.clear();
        let mut thief_lure_pops = std::mem::take(&mut self.thief_lure_pops_buf);
        thief_lure_pops.clear();
        // Positions of King Crabs that just got worn down this frame — celebrate after the loop
        let mut boss_broke = std::mem::take(&mut self.boss_broke_buf);
        boss_broke.clear();
        // Positions of Armored crabs whose shell the beam just wore through — pop a "crack" after the loop
        let mut armor_broke = std::mem::take(&mut self.armor_broke_buf);
        armor_broke.clear();
        // Sparkle particles for attracted crabs (collected to avoid borrow conflict)
        let mut attraction_particles = std::mem::take(&mut self.attraction_particles_buf);
        attraction_particles.clear();
        // King Crab charge telegraph events, collected to sidestep the &mut self.crabs borrow.
        let mut boss_windups = std::mem::take(&mut self.boss_windups_buf); // a charge just started winding up
        boss_windups.clear();
        let mut boss_launches = std::mem::take(&mut self.boss_launches_buf); // a wound-up charge just fired
        boss_launches.clear();
        let mut boss_charge_dust = std::mem::take(&mut self.boss_charge_dust_buf); // (pos, vel) trail while lunging
        boss_charge_dust.clear();
        // A boss crossed into its enrage phase this frame — (pos, is_tide). Fired once per boss.
        let mut boss_enrages = std::mem::take(&mut self.boss_enrages_buf);
        boss_enrages.clear();
        // Tide Boss pulse fires this frame (center positions) — processed after the loop so the
        // shockwave can scatter the herd and loosen the train without fighting the &mut borrow.
        // Reused scratch buffers like the other event vecs above: almost always empty (at most
        // one boss pulsing at a time), so taking/restoring avoids a Vec::new() every frame.
        let mut tide_fires = std::mem::take(&mut self.tide_fires_buf);
        tide_fires.clear();
        let mut tide_swells = std::mem::take(&mut self.tide_swells_buf); // a pulse just started swelling — telegraph feedback
        tide_swells.clear();

        // Where the King Crab aims: the exposed tail of the conga train if there is one, else the
        // player — "whoever currently holds the highest chain_index". Folded into the single
        // snapshot pass below (tracked via a running best-chain_index candidate) instead of its own
        // full scan, alongside the Magnet/Golden/Armored position snapshots that used to each walk
        // self.crabs separately: 4 full passes over a struct with 20+ fields collapsed into 1. Same
        // results, same order-independent picks (positions just need membership, tail just needs the
        // max chain_index), a quarter of the cache traffic before the real per-crab loop even starts.
        let mut magnet_positions = std::mem::take(&mut self.magnet_positions_buf);
        magnet_positions.clear();
        let mut golden_lure_positions = std::mem::take(&mut self.golden_lure_positions_buf);
        golden_lure_positions.clear();
        let mut armored_positions = std::mem::take(&mut self.armored_positions_buf);
        armored_positions.clear();
        let mut best_chain: Option<(usize, Vec2, CrabType)> = None;
        let mut free_splitter = false;
        for c in &self.crabs {
            if c.caught {
                if let Some(ci) = c.chain_index {
                    if best_chain.map_or(true, |(bci, ..)| ci > bci) {
                        best_chain = Some((ci, c.pos, c.crab_type));
                    }
                }
                continue; // caught crabs can't be a Magnet/Golden/Armored source below
            }
            if c.is_splitter() {
                free_splitter = true;
            } else if c.is_magnet() {
                magnet_positions.push(c.pos);
            } else if c.is_golden() {
                if !c.in_flashlight {
                    golden_lure_positions.push(c.pos);
                }
            } else if c.is_armored() {
                armored_positions.push(c.pos);
            }
        }
        let chain_tail_pos = best_chain.map(|(_, pos, _)| pos);
        // Splice targeting: when the chain is long enough (>= 4 links), the King Crab aims at a
        // mid-chain crab rather than the tail — this maximizes the stolen count (everything behind
        // the crossing point goes). The target is whichever caught crab sits closest to 1/3 from
        // the tail (low enough to steal a big chunk, high enough to cross the body rather than
        // just nipping the end). Falls back to the tail if the chain is short or no caught crabs exist.
        let splice_target_pos: Option<Vec2> = if self.chain_count >= 4 {
            let target_ci = self.chain_count * 2 / 3; // aim 2/3 down from head = 1/3 from tail
            let mut best_dist = f32::MAX;
            let mut found: Option<Vec2> = None;
            for c in &self.crabs {
                if c.caught {
                    if let Some(ci) = c.chain_index {
                        let dist = (ci as i32 - target_ci as i32).unsigned_abs() as f32;
                        if dist < best_dist {
                            best_dist = dist;
                            found = Some(c.pos);
                        }
                    }
                }
            }
            found
        } else {
            None
        };
        let charge_target =
            splice_target_pos.unwrap_or_else(|| chain_tail_pos.unwrap_or(self.player_pos));
        // Captured before the &mut self.crabs loop: while the post-scatter regroup window is live the
        // King Crab can't wind up a fresh charge, so you can't be chain-detonated back-to-back.
        let boss_hit_iframes_active = self.boss_hit_iframes > 0.0;
        // Cache for steal_chain_thief (called later this frame, after update_crabs returns) so it
        // doesn't need its own third O(n) scan over self.crabs for the same "current tail" lookup.
        self.cached_tail_pos = chain_tail_pos;
        // Cache the tail archetype for the draw-path CATCH-NEXT highlight (same snapshot, no extra scan).
        self.cached_tail_type = best_chain.map(|(_, _, ty)| ty);
        // The cycle preview marker is only meaningful with a real train (>= 2 links) and while the
        // cycle verb is actually available (off cooldown), so it shows exactly when pressing X would
        // do something. The draw path finds the chain_index==1 crab itself.
        self.cycle_preview_active = self.chain_count >= 2 && self.cycle_cooldown <= 0.0;
        // Cache for the draw path: avoids an O(n) .any() scan over all crabs every frame to gate
        // the cleave-stakes tag. Updated here in the snapshot pass we already do over every crab.
        self.free_splitter_present = free_splitter;

        // Magnet-crab pull: free-roaming Magnet crabs each tug nearby uncaught crabs toward
        // themselves, so the herd clumps up around them. Snapshotted above so each ordinary crab
        // can pull toward the nearest one without a nested borrow. Almost always a tiny list
        // (Magnets are ~8% of the herd and rare), so a flat per-crab nearest-magnet scan is cheap.
        const MAGNET_RADIUS: f32 = 240.0; // how far a Magnet's pull reaches
        const MAGNET_RADIUS_SQ: f32 = MAGNET_RADIUS * MAGNET_RADIUS; // avoids a sqrt per candidate below

        // Emergent crossover — a snared Golden supercharges its captor Magnet. The Magnet-snares-
        // Golden pass already traps a straying shiny in a lodestone's field; here that trapped prize
        // feeds back into the field. While a Magnet is pinning a snared Golden, the Golden's shine
        // energizes it, so it vacuums the surrounding herd in over a *wider* radius and with a
        // stronger tug than a plain roaming Magnet. Neither rule authored this: "Magnet snares
        // Golden" and "Magnet pulls the herd" collide to turn trapping the prize into a herd-vacuum
        // — trap the Golden in a wandering Magnet and it also balls up the nearby loose crabs into a
        // tight cluster you can then sweep with one beam pass. Snapshot which Magnets are charged
        // this frame: a Magnet is charged if a snared Golden sits inside its normal pull radius.
        // Cheap — Magnets and snared Goldens are both rare, so this double loop is almost always over
        // near-empty lists. Reuses a scratch Vec to avoid per-frame churn.
        let mut charged_magnet_positions = std::mem::take(&mut self.charged_magnet_positions_buf);
        charged_magnet_positions.clear();
        for c in &self.crabs {
            if c.is_golden() && !c.caught && c.magnet_snared > 0.0 {
                // Attribute this snared Golden to its nearest Magnet (the one that trapped it).
                let mut nearest: Option<(f32, Vec2)> = None;
                for &mp in magnet_positions.iter() {
                    let d2 = c.pos.distance_squared(mp);
                    if d2 < MAGNET_RADIUS_SQ && nearest.map_or(true, |(bd2, _)| d2 < bd2) {
                        nearest = Some((d2, mp));
                    }
                }
                if let Some((_, mp)) = nearest {
                    if !charged_magnet_positions.contains(&mp) {
                        charged_magnet_positions.push(mp);
                    }
                }
            }
        }
        // How many charged positions come from a pinned Golden. Positions past this index are
        // Dancer-thumped Magnets appended below — the refresh pass uses this split so a Golden-pin
        // keeps its charge topped up (it holds as long as the prize is pinned) while a Dancer thump
        // is a one-shot surge that decays on its own timer instead of latching on forever.
        let golden_charged_count = charged_magnet_positions.len();
        for c in &self.crabs {
            // Emergent crossover — a Dancer's on-beat hop just jostled this Magnet into a pull surge
            // (see the Dancer-jolts-Magnet block in the beat handler). Its `magnet_charged` timer,
            // set on the beat, is still live: treat it as a charged Magnet here too so the same
            // wider-reach herd-vacuum that a snared Golden buys also fires when a Dancer thumps it,
            // reusing the exact charged-field pass below instead of authoring a second one. A Magnet
            // that's *both* pinning a Golden and freshly thumped is already in the list — the
            // contains() guard keeps it single (and Golden-attributed, so it keeps refreshing).
            if c.is_magnet()
                && !c.caught
                && c.magnet_charged > 0.0
                && !charged_magnet_positions.contains(&c.pos)
            {
                charged_magnet_positions.push(c.pos);
            }
        }
        // Magnet cluster detection: on-beat only (rhythmic flash), check each free Magnet
        // for ≥3 nearby free crabs — the "pied-piper vacuum" tell. Fires on the beat so it
        // pulses with the music rather than strobing every frame.
        let cluster_on_beat =
            self.beat_timer < BEAT_WINDOW || self.beat_timer > self.beat_interval - BEAT_WINDOW;
        if cluster_on_beat {
            for &mp in &magnet_positions {
                let nearby = self
                    .crabs
                    .iter()
                    .filter(|c| {
                        !c.caught
                            && !c.is_magnet()
                            && !c.is_boss()
                            && c.pos.distance_squared(mp) < MAGNET_RADIUS_SQ
                    })
                    .count();
                if nearby >= 3 && self.magnet_cluster_hits_buf.len() < 8 {
                    self.magnet_cluster_hits_buf.push(mp);
                }
            }
        }

        // A charged Magnet's field reaches ~40% farther and tugs harder while it holds a prize.
        const CHARGED_MAGNET_RADIUS: f32 = MAGNET_RADIUS * 1.4;
        const CHARGED_MAGNET_RADIUS_SQ: f32 = CHARGED_MAGNET_RADIUS * CHARGED_MAGNET_RADIUS;

        // Emergent crossover — the Golden lures the Magnet. `golden_lure_positions` (every free,
        // un-beamed Golden's position) was snapshotted in the single pass above, so a roaming
        // Magnet can be drawn *off its cluster* toward the shiny prize: the mirror of the
        // Magnet-snares-Golden interaction (there the Magnet traps the Golden; here the Golden's
        // shine pulls the Magnet away from tending its herd).
        const MAGNET_LURE_RADIUS: f32 = 300.0; // a Magnet notices a Golden from a bit farther than its own pull reaches
        const MAGNET_LURE_RADIUS_SQ: f32 = MAGNET_LURE_RADIUS * MAGNET_LURE_RADIUS;

        // Emergent crossover — a free Armored crab body-blocks a charging King Crab. The Armored
        // crab is already established as a wall (its calm-anchor shell shelters the herd from panic
        // ripples); here that same stubborn shell also stops a boss lunge cold. `armored_positions`
        // (every free Armored crab's position) was snapshotted in the single pass above so the King
        // Crab's charge arm below can test whether its lane plows through one — if it does, the
        // shell clangs, the boss skids to a halt on cooldown, and the tail it was aiming for is
        // spared. Parking or leaving an Armored crab between the boss and your train becomes a real
        // defensive routing play — the mirror of a Magnet between your train and an incoming Thief.
        // A charging King Crab that rams a free Armored crab this frame — (boss_pos, shell_pos) so
        // the shell-clang feedback fires after the borrow ends. Almost always empty (needs a boss
        // mid-lunge overlapping a shell), so a reused scratch Vec keeps it allocation-free.
        let mut boss_blocks = std::mem::take(&mut self.boss_blocks_buf);
        boss_blocks.clear();
        // King Crab positions stunned by ramming a parked Armored shell this frame — daze feedback
        // fires after the borrow ends, same deferred pattern as boss_blocks above.
        let mut boss_stuns = std::mem::take(&mut self.boss_stuns_buf);
        boss_stuns.clear();

        // Snapshot the current conga tail position so free Thief crabs can home in on it below
        // (they ignore the herd and beeline for the train's exposed end). Only meaningful once the
        // train is long enough for the Thief's steal to bite; otherwise Thieves just roam. This is
        // the same crab chain_tail_pos already found above (highest chain_index), so reuse it
        // instead of a second scan.
        let thief_tail_pos: Option<Vec2> = if self.chain_count >= 4 {
            chain_tail_pos
        } else {
            None
        };

        // Single RNG for the whole per-crab loop below (attraction sparkles), instead of grabbing
        // a fresh thread-local handle inside the loop for every crab currently in the beam.
        let mut rng = rand::rng();

        // Snapshot whether we're inside the on-beat window right now, so the Reef DJ (rhythm boss)
        // can gate its shell-drain on the beat without re-borrowing self mid-loop. Same window the
        // player already feels for PERFECT tool hits and the on-beat Call.
        let on_beat_now =
            self.beat_timer < BEAT_WINDOW || self.beat_timer > self.beat_interval - BEAT_WINDOW;
        // Downbeat herd-pulse strength for this frame, snapshotted so the per-crab loop can apply a
        // gentle player-ward nudge to free crabs without re-borrowing self. Decays over the frames
        // after each downbeat (set to 1.0 in the beat handler), so the tug fades between beats and
        // the herd only visibly clumps on the "1". Player center is read once here too.
        let downbeat_pull = self.downbeat_pull;
        let downbeat_pull_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        self.downbeat_pull = (self.downbeat_pull - dt * 4.0).max(0.0); // ~0.25s falloff
        // Is *this* on-beat one the Reef DJ called? Its shell only drains on a hot beat of the
        // current phrase (see the phrase roll in the beat handler), so holding light on it during a
        // silent beat does nothing — you have to echo the called pattern back. beat_count is already
        // advanced for this beat (the beat handler runs earlier this frame), so beat_count % 4 is the
        // current beat's slot in the bar. A hit on a hot beat kicks reef_hit_flash for juice.
        let reef_hot_now = on_beat_now && self.reef_phrase[(self.beat_count % 4) as usize];
        let mut reef_hit_landed = false;
        // Recomputed each frame from the live crab list: true while an un-caught Reef DJ is on the
        // field. Gates the phrase roll + HUD telegraph so they only appear during a rhythm-boss fight.
        let mut reef_on_field = false;
        // Live Reef DJ position, captured so we can ring its backup "hype Dancers" out from it.
        let mut reef_boss_pos = Vec2::ZERO;

        // Tide Pool current snapshot: only the Water biome's native pools carry a drift. Precomputed
        // once here (a &self method + a slice bound) so the per-crab loop below can drift free crabs
        // without re-borrowing self. Trailing flood pools are Tide Boss surge water, not native
        // current, so we drift only within `tide_current_pools`. Empty on any non-Water biome, which
        // makes the per-crab check below a cheap `is_empty()` short-circuit on those zones.
        let tide_current_active = self.current_terrain() == TerrainKind::Water;
        let tide_current_native = if tide_current_active {
            self.tide_pools.len().saturating_sub(self.boss_flood_pools)
        } else {
            0
        };
        let tide_current_pools: &[(Vec2, f32)] = &self.tide_pools[..tide_current_native];

        // Neon Kelp funnel snapshot: the Kelp biome's routing mechanic, mirroring the tide-current
        // slice above. Only the Kelp biome's native patches funnel — trailing flood pools are Tide
        // Boss water, not weed. Empty on every non-Kelp biome, so the per-crab check below short-
        // circuits on a cheap `is_empty()` everywhere else. Unlike the water current this only grabs
        // *fleeing* crabs, but the pool slice it channels through is computed the same way.
        let kelp_funnel_active = self.current_terrain() == TerrainKind::Kelp;
        let kelp_funnel_native = if kelp_funnel_active {
            self.tide_pools.len().saturating_sub(self.boss_flood_pools)
        } else {
            0
        };
        let kelp_funnel_pools: &[(Vec2, f32)] = &self.tide_pools[..kelp_funnel_native];

        for crab in &mut self.crabs {
            // King Crab boss runs its own charge AI instead of the herd flee/attract logic.
            if crab.is_boss() && !crab.caught {
                if crab.is_rhythm_boss() {
                    reef_on_field = true;
                    reef_boss_pos = crab.pos;
                }
                crab.spawn_time += dt;
                // Tick down the King Crab's daze from ramming a parked Armored shell (set in the
                // charge-block pass below). While it's >0 the boss can't wind up a new charge and
                // its shell drains faster (see the stunned-drain boost above).
                if crab.stun_timer > 0.0 {
                    crab.stun_timer = (crab.stun_timer - dt).max(0.0);
                }
                let distance = self.player_pos.distance(crab.pos);
                let to_crab = (crab.pos - self.player_pos).normalize_or_zero();
                let angle_to_crab = flashlight_dir.angle_between(to_crab).abs();
                let crab_in_light = self.flashlight.on
                    && distance < flashlight_range
                    && angle_to_crab < flashlight_cone_angle;
                crab.in_flashlight = crab_in_light;

                // Wearing it down under the beam is unchanged for the King Crab and Tide Boss —
                // the beam is still how you catch them. The Reef DJ is the exception: its shell is
                // call-locked, so the beam only bites while you hold the light on it during a *hot*
                // beat of the phrase it called this bar. Off the phrase (off-beat, or an un-called
                // on-beat) the light does nothing — the whole fight is echoing its pattern back with
                // the light. Enraged, it drains faster on a hit so the finale rewards clean timing.
                let drain_active = crab_in_light && (!crab.is_rhythm_boss() || reef_hot_now);
                if crab.is_rhythm_boss() && crab_in_light && reef_hot_now && crab.boss_health > 0.0
                {
                    reef_hit_landed = true;
                }
                if crab.boss_health > 0.0 && drain_active {
                    let mut rate = if crab.is_rhythm_boss() {
                        // The window is narrow AND only some beats are hot, so per-hit drain is boosted
                        // to keep the fight a comparable length to the other bosses; enrage sharpens it.
                        boss_drain * if crab.enraged { 5.0 } else { 3.5 }
                    } else {
                        boss_drain
                    };
                    // Stunned-drain boost: a King Crab reeling from ramming a parked Armored shell
                    // takes far more beam damage, so baiting the lunge into a shell then holding the
                    // light on the dazed boss is a real damage window — the archetype block fused into
                    // the boss fight (see the block pass below where stun_timer is set).
                    if crab.is_stunned() {
                        rate *= 2.5;
                    }
                    // Fired Drum Roll blast cracks the shell far faster than a plain held beam
                    // (up to 7x at full charge). Multiplies the drain here inside the same
                    // `drain_active` gate — so it stacks with a stun window on a King Crab, and
                    // still only lands on a hot beat against the Reef DJ. The wide fired cone also
                    // makes it easier to keep the light on the boss for the short release window.
                    rate *= drum_roll_boss_mult;
                    crab.boss_health -= rate * dt;
                    if crab.boss_health <= 0.0 {
                        crab.boss_health = 0.0;
                        boss_broke.push(crab.pos);
                    }
                }

                // Multi-phase escalation: the moment its health dips below the enrage threshold, the
                // boss enters its final phase. Latch it once so we fire a single dramatic telegraph;
                // the enraged flag then feeds the charge/pulse cadence below to make the climax ramp.
                if !crab.enraged
                    && crab.boss_health > 0.0
                    && crab.boss_health <= crab.boss_max_health * BOSS_ENRAGE_THRESHOLD
                {
                    crab.enraged = true;
                    crab.charge_cooldown = crab.charge_cooldown.min(1.0); // snap toward its next move — no lull into the finale
                    boss_enrages.push((crab.pos, crab.is_tide_boss()));
                }

                // The Tide Boss doesn't charge — it drifts and pulses. Distinct threat, distinct
                // counterplay: keep the train *away* from it (spacing) rather than routing out of a
                // charge lane. It reuses charge_cooldown as its pulse timer and BossCharge::Winding
                // to mean "swelling before a pulse".
                if crab.is_tide_boss() {
                    let (width, height) = area;
                    match crab.charge_state {
                        BossCharge::Winding(t) => {
                            let nt = t - dt;
                            // Rear up and nearly stop while the swell builds — the telegraph window.
                            crab.vel = crab.vel.lerp(Vec2::ZERO, 0.2);
                            crab.pos += crab.vel * dt;
                            crab.charge_state = if nt <= 0.0 {
                                tide_fires.push(crab.pos);
                                // Enraged: rest far less between pulses so the finale hammers the train.
                                crab.charge_cooldown = if crab.enraged {
                                    TIDE_PULSE_COOLDOWN * BOSS_ENRAGE_COOLDOWN_SCALE
                                } else {
                                    TIDE_PULSE_COOLDOWN
                                };
                                BossCharge::Idle
                            } else {
                                BossCharge::Winding(nt)
                            };
                        }
                        _ => {
                            if crab.charge_cooldown > 0.0 {
                                crab.charge_cooldown -= dt;
                            }
                            // Wander gently toward the train's heart so it stays a looming presence.
                            let dir = (charge_target - crab.pos).normalize_or_zero();
                            crab.vel = crab.vel.lerp(dir * crab.speed, 0.02);
                            crab.pos += crab.vel * dt;
                            // Once rested and there's a train worth scattering, begin swelling a pulse.
                            if crab.charge_cooldown <= 0.0 && self.chain_count >= 3 {
                                crab.charge_state = BossCharge::Winding(TIDE_PULSE_WINDUP);
                                tide_swells.push(crab.pos);
                            }
                        }
                    }
                    // Bounce off walls, face travel direction (shared with the King Crab tail below).
                    if crab.pos.x < 0.0 || crab.pos.x > width - crab.scale {
                        crab.vel.x = -crab.vel.x;
                        crab.pos.x = crab.pos.x.clamp(0.0, width - crab.scale);
                    }
                    if crab.pos.y < 0.0 || crab.pos.y > height - crab.scale {
                        crab.vel.y = -crab.vel.y;
                        crab.pos.y = crab.pos.y.clamp(0.0, height - crab.scale);
                    }
                    let speed = crab.vel.length();
                    if speed > 5.0 {
                        let target_angle = crab.vel.y.atan2(crab.vel.x);
                        let mut delta = target_angle - crab.facing_angle;
                        while delta > std::f32::consts::PI {
                            delta -= std::f32::consts::TAU;
                        }
                        while delta < -std::f32::consts::PI {
                            delta += std::f32::consts::TAU;
                        }
                        crab.facing_angle += delta * (dt * 8.0).min(1.0);
                    }
                    continue;
                }

                // The Reef DJ (rhythm boss) doesn't charge or pulse — it just grooves toward the
                // train's heart as a looming presence while you try to land beat-timed light on it.
                // No hazard state machine at all: the entire threat is the timing test on its shell,
                // so it stays a clean, legible set-piece (hold the light, hit the beat, watch the
                // shell drop a chunk every downbeat).
                if crab.is_rhythm_boss() {
                    let (width, height) = area;
                    let dir = (charge_target - crab.pos).normalize_or_zero();
                    crab.vel = crab.vel.lerp(dir * crab.speed, 0.02);
                    crab.pos += crab.vel * dt;
                    if crab.pos.x < 0.0 || crab.pos.x > width - crab.scale {
                        crab.vel.x = -crab.vel.x;
                        crab.pos.x = crab.pos.x.clamp(0.0, width - crab.scale);
                    }
                    if crab.pos.y < 0.0 || crab.pos.y > height - crab.scale {
                        crab.vel.y = -crab.vel.y;
                        crab.pos.y = crab.pos.y.clamp(0.0, height - crab.scale);
                    }
                    let speed = crab.vel.length();
                    if speed > 5.0 {
                        let target_angle = crab.vel.y.atan2(crab.vel.x);
                        let mut delta = target_angle - crab.facing_angle;
                        while delta > std::f32::consts::PI {
                            delta -= std::f32::consts::TAU;
                        }
                        while delta < -std::f32::consts::PI {
                            delta += std::f32::consts::TAU;
                        }
                        crab.facing_angle += delta * (dt * 8.0).min(1.0);
                    }
                    continue;
                }

                // Charge state machine. Holding the beam can't cancel a wind-up — the counterplay is
                // to move the train out of the lane, which is exactly the "route and protect" tension
                // a long conga line should carry.
                match crab.charge_state {
                    BossCharge::Idle => {
                        if crab.charge_cooldown > 0.0 {
                            crab.charge_cooldown -= dt;
                        }
                        // Lumber toward the train so it stays a closing threat.
                        let dir = (charge_target - crab.pos).normalize_or_zero();
                        crab.vel = crab.vel.lerp(dir * crab.speed, 0.02);
                        crab.pos += crab.vel * dt;
                        // Arm a charge once it's rested, the train is worth scattering, and in range.
                        // A stunned (recently-blocked) King Crab can't wind up until the daze passes.
                        if crab.charge_cooldown <= 0.0
                            && !crab.is_stunned()
                            && !boss_hit_iframes_active
                            && self.chain_count >= 3
                            && crab.pos.distance(charge_target) < BOSS_CHARGE_ARM_RANGE
                        {
                            crab.charge_state = BossCharge::Winding(BOSS_WINDUP_TIME);
                            boss_windups.push(crab.pos);
                        }
                    }
                    BossCharge::Winding(t) => {
                        let nt = t - dt;
                        // Rear back: nearly stop and lean away from the target to sell the wind-up.
                        let away = (crab.pos - charge_target).normalize_or_zero();
                        crab.vel = crab.vel.lerp(away * crab.speed * 0.7, 0.15);
                        crab.pos += crab.vel * dt;
                        crab.charge_state = if nt <= 0.0 {
                            // Lock the heading at launch and commit.
                            let mut dir = (charge_target - crab.pos).normalize_or_zero();
                            if dir == Vec2::ZERO {
                                dir = Vec2::new(0.0, 1.0);
                            }
                            // Enraged King Crab lunges harder — a faster, scarier commit in the finale.
                            let charge_speed = if crab.enraged {
                                BOSS_CHARGE_SPEED * BOSS_ENRAGE_CHARGE_SPEED_SCALE
                            } else {
                                BOSS_CHARGE_SPEED
                            };
                            crab.vel = dir * charge_speed;
                            boss_launches.push(crab.pos);
                            BossCharge::Charging(BOSS_CHARGE_TIME)
                        } else {
                            BossCharge::Winding(nt)
                        };
                    }
                    BossCharge::Charging(t) => {
                        let nt = t - dt;
                        crab.pos += crab.vel * dt; // vel stays locked to the launch heading
                        boss_charge_dust.push((crab.pos, crab.vel));
                        // Emergent crossover: did the lunge just plow into a free Armored crab's
                        // shell? If so the wall wins — the charge aborts here, sparing the tail it
                        // was aimed at, and the boss goes on cooldown as if the lunge had spent
                        // itself. The Armored crab is knocked back but keeps its shell (it's not
                        // caught — it just took the hit). Uses the boss's bulk-widened reach so a
                        // near-miss still counts as a block, matching how the tail-snap gives the
                        // charge a wide hitbox.
                        const BLOCK_REACH: f32 = CRAB_SIZE * 1.1;
                        let block_hit = armored_positions.iter().find(|&&ap| {
                            crab.pos.distance(ap) < BLOCK_REACH + crab.scale * CRAB_SIZE * 0.5
                        });
                        if let Some(&shell_pos) = block_hit {
                            crab.charge_cooldown = if crab.enraged {
                                BOSS_CHARGE_COOLDOWN * BOSS_ENRAGE_COOLDOWN_SCALE
                            } else {
                                BOSS_CHARGE_COOLDOWN
                            };
                            // Slamming a shell doesn't just stop the lunge — the impact DAZES the
                            // King Crab. For the stun window it can't wind up a new charge and its
                            // own shell drains far faster under the beam (see the stunned-drain boost
                            // above), turning the Armored block from a purely defensive save into a
                            // real damage opportunity: bait the lunge into a parked shell, then hold
                            // the light on the reeling boss to chunk it down. Fuses the archetype web
                            // with the boss fight, exactly when the fight peaks. Enraged bosses shake
                            // it off a little quicker.
                            crab.stun_timer = if crab.enraged {
                                BOSS_STUN_DURATION * 0.7
                            } else {
                                BOSS_STUN_DURATION
                            };
                            // Keep it dazed at least as long as it's stunned before it can charge again.
                            crab.charge_cooldown = crab.charge_cooldown.max(crab.stun_timer + 0.3);
                            // Bounce the boss back off the shell so the stop reads as an impact,
                            // not a stall, then let it settle into Idle next.
                            crab.vel = -crab.vel.normalize_or_zero() * crab.speed * 0.6;
                            boss_blocks.push((crab.pos, shell_pos));
                            boss_stuns.push(crab.pos);
                            crab.charge_state = BossCharge::Idle;
                        } else {
                            crab.charge_state = if nt <= 0.0 {
                                // Enraged: shorter rest between lunges so the finale keeps the pressure on.
                                crab.charge_cooldown = if crab.enraged {
                                    BOSS_CHARGE_COOLDOWN * BOSS_ENRAGE_COOLDOWN_SCALE
                                } else {
                                    BOSS_CHARGE_COOLDOWN
                                };
                                crab.vel *= 0.15; // skid to a halt out of the lunge
                                BossCharge::Idle
                            } else {
                                BossCharge::Charging(nt)
                            };
                        }
                    }
                }

                // Bounce off the arena walls just like the herd.
                let (width, height) = area;
                if crab.pos.x < 0.0 || crab.pos.x > width - crab.scale {
                    crab.vel.x = -crab.vel.x;
                    crab.pos.x = crab.pos.x.clamp(0.0, width - crab.scale);
                }
                if crab.pos.y < 0.0 || crab.pos.y > height - crab.scale {
                    crab.vel.y = -crab.vel.y;
                    crab.pos.y = crab.pos.y.clamp(0.0, height - crab.scale);
                }
                // Smoothly rotate to face travel direction.
                let speed = crab.vel.length();
                if speed > 5.0 {
                    let target_angle = crab.vel.y.atan2(crab.vel.x);
                    let mut delta = target_angle - crab.facing_angle;
                    while delta > std::f32::consts::PI {
                        delta -= std::f32::consts::TAU;
                    }
                    while delta < -std::f32::consts::PI {
                        delta += std::f32::consts::TAU;
                    }
                    crab.facing_angle += delta * (dt * 8.0).min(1.0);
                }
                continue;
            }

            if !crab.caught {
                crab.spawn_time += dt;

                // If crab is spooked, it will move towards the player.
                let distance = self.player_pos.distance(crab.pos);
                let to_crab = (crab.pos - self.player_pos).normalize_or_zero();
                let angle_to_crab = flashlight_dir.angle_between(to_crab).abs();

                // Check if crab is within flashlight light.
                let crab_in_light = self.flashlight.on
                    && distance < flashlight_range
                    && angle_to_crab < flashlight_cone_angle;

                // Track flashlight state on the crab for rendering
                crab.in_flashlight = crab_in_light;

                // Shelled crabs (King Crab boss + Armored herd crabs) must be worn down before they
                // can be caught: holding the beam on one drains its shell. This is the slow universal
                // path — a Stomp cracks an Armored shell instantly, but the beam always works too, so
                // no crab is ever impossible without the right tool.
                //
                // The Hermit is the deliberate exception: the beam CAN'T touch its borrowed shell, so
                // it forces the ecosystem verbs (Stomp / Dancer-hop / Magnet-rip). That's what makes
                // it a genuinely new target rather than an Armored reskin — Armored = crack it with
                // your own tools; Hermit = crack it with the archetype web.
                if crab.boss_health > 0.0 && crab_in_light && !crab.is_hermit() {
                    crab.boss_health -= boss_drain * dt;
                    if crab.boss_health <= 0.0 {
                        crab.boss_health = 0.0;
                        if crab.is_boss() {
                            boss_broke.push(crab.pos);
                        } else {
                            armor_broke.push(crab.pos);
                        }
                    }
                }

                // Strong-match: beam shining on a shelled Hermit. The beam can't crack its
                // borrowed shell (only ecosystem verbs can), but we still collect the hit so
                // draw_beam_hermit_match can flash amber — a legibility cue telling the player
                // "beam won't work here; use Stomp, Dancer, or Magnet instead".
                if crab.is_shelled_hermit() && crab.boss_health > 0.0 && crab_in_light {
                    let drain_fraction = 1.0 - crab.boss_health / crab.boss_max_health.max(0.001);
                    self.beam_hermit_hits_buf.push((crab.pos, drain_fraction));
                }

                // Panic flee: crabs that are close but outside the flashlight beam scatter away.
                // Bosses are unshakeable — they lumber on rather than panic-bolting.
                const FLEE_RADIUS: f32 = 220.0;
                // How far the downbeat herd pulse reaches — a bit past the flee radius so crabs
                // hovering just outside panic range are the ones the beat sweeps in, without yanking
                // the whole screen.
                const DOWNBEAT_PULL_RADIUS: f32 = 300.0;
                // A whistle-charmed crab holds its nerve near the player instead of bolting, so a
                // well-timed pulse pins a spooked herd in place long enough to sweep them up.
                // Dancer crabs don't panic-flee continuously — their escape is the beat hop
                // (handled in the beat-fire block), so between beats they hold still instead of
                // streaming away. This is what makes them a rhythm-timed grab rather than a chase.
                // A Thief on the hunt for your tail doesn't panic-flee the player between latches —
                // it's single-minded about reaching the train. (A whistle charm still stops it, and
                // once latched it's handled in steal_chain_thief.) This keeps it a committed threat
                // rather than one more crab that scatters when you sweep the beam past it.
                // A shelled Hermit doesn't panic-flee either: clamped inside its borrowed shell it
                // hunkers and holds ground between its scripted host-swap darts (see the dart block
                // below), so it reads as a hiding lump you have to crack rather than a chaser. Once
                // cracked it's an ordinary crab and flees like anything else.
                // The beam is a boss-pressure tool, not a herd lure. Only SHELLED targets the player
                // is actively burning down (Armored / borrowed-shell Hermit — the ones with a shell
                // the beam is chewing through) get "held" by the light; everything else ignores the
                // beam entirely and wanders/flees on its own. This is what keeps the flashlight's
                // identity clean: it burns hard targets, the whistle pulls the herd. A normal crab
                // caught in the cone drifts as if the light weren't there.
                let beam_holds = crab_in_light && crab.boss_health > 0.0 && !crab.is_hermit();
                let now_fleeing = !beam_holds
                    && distance < FLEE_RADIUS
                    && !crab.is_boss()
                    && !crab.is_dancer()
                    && !crab.is_shelled_hermit()
                    && !(crab.is_thief() && self.chain_count >= 4)
                    && crab.charm_timer <= 0.0;

                if beam_holds {
                    // A crab whose shell is being seared holds under the beam — it can't scurry off
                    // while you burn it down, so the pressure reads as "pinned and melting". No pull
                    // toward the player (that was the old herd-lure); it simply stops fleeing.
                    crab.vel = crab.vel.lerp(Vec2::ZERO, 0.04);
                    crab.spooked_timer = 0.7;
                    crab.fleeing = false;
                } else if now_fleeing {
                    // Track first-flee frame so we can emit a "!" pop after the loop
                    if !crab.fleeing {
                        flee_pops.push(crab.pos);
                    }
                    crab.fleeing = true;
                    // Panic: steer sharply away from the player at full type speed.
                    let max_speed = crab.crab_type.speed_range().end;
                    // Proximity factor: full flee speed when very close, tapering off toward FLEE_RADIUS
                    let flee_factor = 1.0 - (distance / FLEE_RADIUS);
                    let flee_speed = max_speed * (1.0 + flee_factor * 1.5);
                    crab.vel = crab.vel.lerp(to_crab * flee_speed, 0.06);
                    crab.speed = 1.0; // vel already encodes speed, keep multiplier neutral
                } else {
                    crab.fleeing = false;
                    // Downbeat herd pulse: a passive, rhythmic routing nudge. A free, un-spooked crab
                    // drifts a little toward the player on the "1" of the bar, so a groove-savvy
                    // player can stand where the next downbeat will sweep loose crabs into their beam.
                    // Deliberately gentle and range-gated (a routing tug, not a yank or a catch), and
                    // skipped for charmed/startled/snared crabs so it never fights the other passes or
                    // turns into an autocatcher next to the on-beat catch bloom. Only meaningful for a
                    // few frames after each downbeat, then it fades and the crab wanders freely again.
                    if downbeat_pull > 0.0
                        && crab.startle_timer <= 0.0
                        && crab.charm_timer <= 0.0
                        && crab.magnet_snared <= 0.0
                        && distance < DOWNBEAT_PULL_RADIUS
                    {
                        let toward = (downbeat_pull_center - crab.pos).normalize_or_zero();
                        // Pull toward the crab's *top* speed so the clump is visible on the "1" — a
                        // real routing tug, not a decorative ring. The flee/light passes still win
                        // when they apply (this is the wander `else` branch only), and the gates
                        // above (free, un-startled, un-charmed, un-snared) are what keep it from
                        // becoming an autocatcher, not the magnitude — so it can be assertive.
                        let nudge = crab.crab_type.speed_range().end * 1.1 * downbeat_pull;
                        crab.vel = crab.vel.lerp(toward * nudge, 0.35 * downbeat_pull);
                    }
                }

                // Calm down after timer
                if crab.spooked_timer > 0.0 {
                    crab.spooked_timer -= dt;
                    if crab.spooked_timer < 0.0 {
                        crab.spooked_timer = 0.0;
                    }
                }

                // Startle from a nearby catch (stampede ripple): the crab keeps its outward
                // bolt speed for a beat. The light re-attracts it (in_light lerp above wins),
                // so sweeping the beam over a scattering herd holds them.
                if crab.startle_timer > 0.0 {
                    crab.startle_timer -= dt;
                    if crab.startle_timer < 0.0 {
                        crab.startle_timer = 0.0;
                    }
                }

                // Amplified Golden panic bleeds back toward ordinary fear as the crab settles,
                // so the panic bomb's extra kick spans only the next few beats rather than
                // permanently supercharging every crab it touched.
                if crab.panic_amp > 1.0 {
                    crab.panic_amp = (crab.panic_amp - dt * 1.2).max(1.0);
                }

                // The Magnet snare lapses if the Golden isn't re-snared this frame (i.e. it drifted
                // out of a Magnet's deep field, or the Magnet was caught). The pull pass above
                // refreshes it back to 0.25 every frame the tether holds, so this only fires the
                // instant the field releases it.
                if crab.magnet_snared > 0.0 {
                    crab.magnet_snared = (crab.magnet_snared - dt).max(0.0);
                }

                // A Golden fired by a Tide Boss slingshot stays re-snare-immune for a short window so
                // it escapes its captor Magnet before the field can reload it (see the Golden snare pass).
                if crab.slingshot_spent > 0.0 {
                    crab.slingshot_spent = (crab.slingshot_spent - dt).max(0.0);
                }

                // Whistle charm wears off after a beat or two, at which point the crab is fair
                // game for the panic contagion again.
                if crab.charm_timer > 0.0 {
                    crab.charm_timer = (crab.charm_timer - dt).max(0.0);
                }

                // A Dancer answering the player's Call keeps its answer for a few beats, then reverts
                // to normal (fleeing) behavior if it wasn't caught in time.
                if crab.answering_call > 0.0 {
                    crab.answering_call = (crab.answering_call - dt).max(0.0);
                }

                // Hermit host-swap: while shelled, the Hermit hunkers in place, then periodically
                // scurries to a new host spot in a short scripted dart — its signature "hides and
                // swaps hosts" restlessness that keeps it from being a stationary Armored reskin. The
                // dart is a quick directional burst (not sustained flee speed) followed by a reset of
                // its irregular timer. It never darts while lit (the player's beam is a truce — you
                // can't crack the shell but you can pin its position to line up a Stomp/Magnet play).
                if crab.is_shelled_hermit() {
                    crab.host_swap_timer -= dt;
                    if crab.host_swap_timer <= 0.0 && !crab_in_light {
                        // Scurry off at a random heading: a brief burst that carries it a short hop
                        // before the movement below damps it back to a hunker. `speed = 1.0` keeps the
                        // multiplier neutral so `vel` alone encodes the dart, matching the flee path.
                        let ang = rng.random_range(0.0_f32..std::f32::consts::TAU);
                        let dart = crab.crab_type.speed_range().start * 1.4;
                        crab.vel = Vec2::new(ang.cos(), ang.sin()) * dart;
                        crab.speed = 1.0;
                        crab.join_pulse = crab.join_pulse.max(0.6); // little squash-pop as it scuttles
                        crab.host_swap_timer = rng.random_range(1.6..3.2);
                    } else {
                        // Between darts it hunkers: bleed velocity so the shelled lump settles and
                        // holds ground rather than coasting on the last dart's momentum.
                        crab.vel *= 1.0 - (4.0 * dt).min(0.9);
                    }
                }

                // If player is within 150 pixels and crab is in the light, add a small extra speed boost
                let mut speed_multiplier = 1.0;
                if crab_in_light && distance < 150.0 {
                    speed_multiplier = 2.0 - (distance / 150.0);
                    speed_multiplier = speed_multiplier.clamp(1.0, 2.0);
                }

                // Older crabs are faster so the player should catch them early.
                let age_boost = 1.0 + (crab.spawn_time / 10.0).min(1.5);
                crab.pos += crab.vel * crab.speed * speed_multiplier * age_boost * dt;

                // On-beat herd stampede: spend the surge armed by the downbeat (see the beat handler).
                // While surge_timer counts down, the crab DARTS an extra shove along its own heading
                // — a decaying burst that's strongest right on the "1" and eases to nothing before the
                // next bar, so the loose herd visibly lurches forward on the downbeat and glides
                // between beats. This makes the herd's *landing spot* a rhythm read: predict the surge,
                // slide into where the crabs will be on the bar. Not applied to a lit crab (it's already
                // steering to the player) so the beam read isn't disturbed. Decayed at ~4/sec so the
                // dart is spent within a beat at typical tempos; the shove scales with the crab's own
                // speed so fast crabs stampede farther, matching their base pace.
                if !crab_in_light && crab.surge_timer > 0.0 {
                    let heading = crab.vel.normalize_or_zero();
                    let mut dir = if heading == Vec2::ZERO {
                        Vec2::new(crab.facing_angle.cos(), crab.facing_angle.sin())
                    } else {
                        heading
                    };
                    // On-beat clump: a *calm free crab near the player* doesn't just step along its own
                    // heading on the "1" — it leans that beat-step toward you, so the loose herd visibly
                    // gathers around the train on the downbeat and the beat itself reads as an ambient
                    // routing tool. Deliberately WEAK and short-range: it only bends the surge direction
                    // (a gentle lean, not a pull toward you every frame), and it falls off past ~320px so
                    // it's herd texture near the train, not a field-wide stream. That keeps Groove Call (V)
                    // the real on-demand gather — V is a strong, timed, field-wide surge you press for;
                    // this is just the calm herd breathing toward you on the beat, strongest right next to
                    // you and fading to a plain own-heading step for crabs across the map.
                    const CLUMP_RADIUS: f32 = 320.0;
                    if distance < CLUMP_RADIUS {
                        let to_player = (self.player_pos - crab.pos).normalize_or_zero();
                        if to_player != Vec2::ZERO {
                            // Lean fraction: up to ~0.45 right next to the player, easing to 0 at the
                            // radius edge — a bend, never a full redirect, so the crab still mostly keeps
                            // its own heading and the read stays "the herd drifts your way", not "warps in".
                            let lean = 0.45 * (1.0 - distance / CLUMP_RADIUS);
                            dir = (dir * (1.0 - lean) + to_player * lean).normalize_or_zero();
                        }
                    }
                    // Ease-out envelope: burst hardest at surge_timer≈1, fading to 0. The shove is a
                    // multiple of the crab's own base speed (crab.speed holds the real magnitude; vel
                    // is a unit heading), so at the peak the crab briefly moves ~3x its normal pace and
                    // eases back — the herd reads as *stepping* on the "1", fast crabs stepping farther,
                    // rather than a flat teleport. ~2.5x peak, decaying over the beat.
                    let envelope = crab.surge_timer * crab.surge_timer;
                    crab.pos += dir * crab.speed * 2.5 * envelope * dt;
                    crab.surge_timer = (crab.surge_timer - dt * 4.0).max(0.0);
                }

                // Tide Pool current: a free crab standing in one of the Water biome's native pools is
                // carried along a fixed drift heading — the pools ferry the loose herd downstream. A
                // gentle positional nudge (like the Magnet herd-pull), so it composes with flee/attract
                // rather than overriding: the flashlight still wins (a lit crab is heading to the player),
                // and a fleeing crab still bolts, just curving with the flow. This turns the pools into a
                // routing puzzle — position your train downstream and let the current deliver crabs to it.
                // `tide_current_pools` is empty on every non-Water biome, so this whole block short-
                // circuits to a single `!is_empty()` check outside the Tide Pools. Bosses are handled in
                // their own branch above and never reach here; caught crabs are gated out by `!crab.caught`.
                if !crab_in_light && !tide_current_pools.is_empty() {
                    let center = crab.pos + Vec2::splat(crab.scale / 2.0);
                    if tide_current_pools
                        .iter()
                        .any(|(c, r)| center.distance_squared(*c) < *r * *r)
                    {
                        // Positional drift only — the crab's facing is recomputed from its own
                        // velocity below (the flow streaks in the pool carry the direction cue), so we
                        // don't fight that here.
                        crab.pos += TIDE_CURRENT_DIR * TIDE_CURRENT_STRENGTH * dt;
                    }
                }

                // Neon Kelp funnel: a *fleeing* free crab inside one of the Kelp biome's native
                // weed patches gets channelled along a fixed lane heading, so the weeds catch a
                // panicking bolt and shepherd it sideways into a lane instead of letting it scatter.
                // Deliberately narrower than the Tide current (which sweeps every free crab): the
                // kelp only grabs a crab that's already fleeing, so it reads as "spook the herd near
                // the weeds and they funnel into a catchable lane" rather than an ambient drift. A
                // positional nudge (like the tide/Magnet pulls) so it composes with the bolt rather
                // than overriding it — the crab keeps fleeing, just curving along the lane; the beam
                // still wins (a lit crab is already gated out). `kelp_funnel_pools` is empty on every
                // non-Kelp biome, so this whole block short-circuits to one `!is_empty()` check.
                if crab.fleeing && !crab_in_light && !kelp_funnel_pools.is_empty() {
                    let center = crab.pos + Vec2::splat(crab.scale / 2.0);
                    if kelp_funnel_pools
                        .iter()
                        .any(|(c, r)| center.distance_squared(*c) < *r * *r)
                    {
                        crab.pos += KELP_FUNNEL_DIR * KELP_FUNNEL_STRENGTH * dt;
                    }
                }

                // Magnet pull: an ordinary free crab drifts toward the nearest roaming Magnet crab,
                // so the herd bunches up around Magnets. A gentle positional nudge (not a velocity
                // shove) that composes with the flee/attract behaviour above rather than overriding
                // it — the flashlight still wins (a crab in the beam is heading to the player), and a
                // fleeing crab still bolts, just curving a little toward the cluster. This is what
                // turns "catch the Magnet" into a two-for-one: the crabs it gathered come with it.
                // Squared-distance compare so the per-magnet scan (up to ~8% of the herd, times
                // every ordinary crab) does zero sqrt work until we've already found the winner
                // — a sqrt per pair here was the hottest unnecessary cost in this per-crab,
                // per-frame loop. Computed once per crab and shared below by both the ordinary
                // herd-nudge/Golden-snare check and the Thief-intercept check (a Thief is never
                // a Magnet or a boss, so this covers it too) instead of scanning
                // magnet_positions a second time for Thieves.
                let nearest_magnet: Option<(f32, Vec2)> =
                    if !crab_in_light && !crab.is_magnet() && !crab.is_boss() {
                        let mut nearest: Option<(f32, Vec2)> = None;
                        for &mp in magnet_positions.iter() {
                            let d2 = crab.pos.distance_squared(mp);
                            if d2 < MAGNET_RADIUS_SQ && d2 > 1.0 {
                                if nearest.map_or(true, |(bd2, _)| d2 < bd2) {
                                    nearest = Some((d2, mp));
                                }
                            }
                        }
                        nearest
                    } else {
                        None
                    };
                if !crab_in_light && !crab.is_magnet() && !crab.is_boss() {
                    if let Some((d2, mp)) = nearest_magnet {
                        // Stronger tug up close, fading to nothing at the edge of the pull radius.
                        let d = d2.sqrt();
                        let prox = 1.0 - d / MAGNET_RADIUS; // 0 at the edge, 1 at the magnet
                        let dir = (mp - crab.pos).normalize_or_zero();
                        // Emergent crossover: a roaming Magnet snares a fleeing Golden. The shiny
                        // prize normally bolts too fast to catch by hand, but a lodestone's field
                        // overpowers even that skittish sprint once the Golden strays deep into it
                        // (inner ~60% of the radius). While snared the Golden is dragged hard toward
                        // the Magnet and its bolt is damped, so herding the prize toward a wandering
                        // Magnet becomes a real way to trap it — the Magnet as accidental savior,
                        // the mirror of the Magnet-pry-Thief save. Outside the deep zone it just
                        // gets the ordinary gentle nudge like any other crab.
                        if crab.is_golden() && prox > 0.4 && crab.slingshot_spent <= 0.0 {
                            // Overpowering drag: far stronger than the herd nudge, scaling up as it
                            // sinks deeper so the snare tightens the closer it gets. A Golden just fired
                            // by a Tide Boss slingshot (slingshot_spent > 0) is immune to re-snare for a
                            // beat or two so it actually clears the field instead of reloading in place.
                            let snare_pull = (prox - 0.4) / 0.6 * 260.0;
                            crab.pos += dir * snare_pull * dt;
                            // Damp the Golden's bolt so it can't just sprint back out of the field.
                            crab.vel *= 1.0 - (0.85 * dt).min(0.5);
                            // First frame of the snare fires a celebratory pop; refresh the tether
                            // window each frame it stays deep so the visual/slow persists smoothly.
                            if crab.magnet_snared <= 0.0 {
                                golden_snare_pops.push(crab.pos);
                            }
                            crab.magnet_snared = 0.25;
                        } else if crab.is_shelled_hermit() {
                            // Signature Hermit edge — a roaming Magnet's field RIPS the borrowed shell
                            // clean out. Unlike the Armored crab (which only wears down slowly under a
                            // *charged* Magnet's vacuum), an ordinary Magnet cracks a Hermit the moment
                            // it drags it deep into the field: the lodestone yanks the crab so hard the
                            // shell tears off. This is *the* new crossover the Hermit exists for — the
                            // beam can't touch its shell, but a Magnet you've parked in its path pops it.
                            //
                            // A shelled Hermit is heavy but the Magnet overpowers it: it gets a firm drag
                            // from anywhere in the field (stronger than the 34-unit herd nudge, scaling
                            // with depth) so a hunkered Hermit actually slides into the lodestone rather
                            // than the weak nudge failing to reach it — then the deep zone rips the shell.
                            let drag = (0.4 + prox * 1.6) * 90.0;
                            crab.pos += dir * drag * dt;
                            if prox > 0.45 {
                                let before = crab.boss_health;
                                // ~5 shell/sec at the core — a full 2.0 shell rips in well under half a
                                // second once it's deep, so the Magnet reads as a decisive cracker, not
                                // the slow grind the Armored gets. Reuses the Armored grind pop/visual.
                                crab.boss_health = (crab.boss_health - 5.0 * dt).max(0.0);
                                let broke = crab.boss_health <= 0.0;
                                let step = crab.crab_type.initial_shell().max(0.001) / 2.0;
                                if broke
                                    || (before / step).floor() != (crab.boss_health / step).floor()
                                {
                                    magnet_grind.push((crab.pos, broke, crab.is_hermit()));
                                }
                                if broke {
                                    crab.join_pulse = 1.0; // pop out of the shell with a squash-and-flee
                                }
                            }
                        } else {
                            let pull = prox * 34.0;
                            crab.pos += dir * pull * dt;
                        }
                    }
                }

                // Emergent crossover — a snared Golden supercharges its captor Magnet into a herd
                // vacuum. When a Magnet is pinning a Golden (see the snare pass just above), the
                // prize's shine energizes the lodestone: it now reaches the surrounding loose herd
                // over a wider radius and hauls them in harder than the plain herd-nudge does, so
                // the trapped Golden and the crabs balling up around it become one tight cluster you
                // can sweep with a single beam pass. Only applies to ordinary crabs the *normal*
                // field didn't already grab this frame — a Golden being snared, a crab already
                // caught, or one deep in a Magnet's own radius keeps its existing behaviour; this is
                // purely the extra outer reach the charge buys. Runs off the tiny charged-Magnet
                // snapshot, so almost always over an empty list.
                if !crab_in_light
                    && !crab.is_magnet()
                    && !crab.is_boss()
                    && !charged_magnet_positions.is_empty()
                    && crab.magnet_snared <= 0.0
                {
                    let mut nearest: Option<(f32, Vec2)> = None;
                    for &cmp in charged_magnet_positions.iter() {
                        let d2 = crab.pos.distance_squared(cmp);
                        if d2 < CHARGED_MAGNET_RADIUS_SQ
                            && d2 > 1.0
                            && nearest.map_or(true, |(bd2, _)| d2 < bd2)
                        {
                            nearest = Some((d2, cmp));
                        }
                    }
                    if let Some((d2, cmp)) = nearest {
                        // Strongest at the core, fading to nothing at the widened edge. A firmer
                        // tug than the plain herd-nudge (its 34.0) so the vacuum visibly balls the
                        // herd up while the charge lasts.
                        let prox = 1.0 - d2.sqrt() / CHARGED_MAGNET_RADIUS;
                        let dir = (cmp - crab.pos).normalize_or_zero();
                        crab.pos += dir * (prox * 68.0) * dt;

                        // Emergent crossover — a charged Magnet's vacuum grinds an Armored shell.
                        // The same widened field that balls the loose herd up also drags an Armored
                        // crab against the lodestone hard enough to wear its shell down over time —
                        // so a Golden-supercharged (or Dancer-thumped) Magnet slowly cracks open any
                        // hard-shell it hauls in, softening a stomp-only target you can then finish
                        // with the beam. A three-archetype collision: the Golden/Dancer that charged
                        // the Magnet, the Magnet's vacuum, and the Armored crab caught in its reach.
                        // Reuses the charged-field snapshot and the shell HP the Stomp already wears
                        // down — no new field, just a second thing the charge is worth. Grinds only
                        // near the core (where the drag is strongest), so an Armored crab clipping the
                        // outer edge just gets balled up like the rest.
                        if crab.is_armored() && crab.boss_health > 0.0 && prox > 0.45 {
                            let before = crab.boss_health;
                            // ~3 shell/sec at the core, tapering to nothing by prox 0.45. A full
                            // shell takes a couple seconds of being pinned in the vacuum to open.
                            let grind = (prox - 0.45) / 0.55 * 3.0;
                            crab.boss_health = (crab.boss_health - grind * dt).max(0.0);
                            crab.join_pulse = crab.join_pulse.max(0.4); // faint shudder as it's ground
                            let broke = crab.boss_health <= 0.0;
                            // One chip pop per ~third of the shell worn (or the final crack), so the
                            // grind reads as steady progress without spamming a pop every frame.
                            let step = crab.crab_type.initial_shell().max(0.001) / 3.0;
                            if broke || (before / step).floor() != (crab.boss_health / step).floor()
                            {
                                magnet_grind.push((crab.pos, broke, false)); // Armored, never a Hermit
                            }
                        }
                    }
                }

                // Emergent crossover — the Golden lures the Magnet off its cluster. A roaming Magnet
                // that isn't itself being beamed drifts toward the nearest free, fleeing Golden it can
                // sense: the shiny prize's shine catches the lodestone's attention and pulls it away
                // from the herd it was gathering. This is the mirror of the Magnet-snares-Golden pass
                // above — there the Magnet traps the Golden; here the Golden tugs the Magnet — and it
                // adds a real routing wrinkle: a Magnet you were steering toward your train can go
                // wandering after a Golden, either concentrating the two prizes together (good) or
                // abandoning the cluster you were building (bad). Skipped once the Golden is deep in
                // the Magnet's own field, since the snare pass then takes over and pins it. Uses the
                // Goldens snapshotted before the loop, so no nested borrow.
                if crab.is_magnet() && !crab_in_light && !golden_lure_positions.is_empty() {
                    let mut nearest: Option<(f32, Vec2)> = None;
                    for &gp in golden_lure_positions.iter() {
                        let d2 = crab.pos.distance_squared(gp);
                        // Only chase Goldens that are within lure range but not already inside the
                        // Magnet's own pull radius — once it's that close the snare handles it.
                        if d2 < MAGNET_LURE_RADIUS_SQ && d2 > MAGNET_RADIUS_SQ * 0.36 {
                            if nearest.map_or(true, |(bd2, _)| d2 < bd2) {
                                nearest = Some((d2, gp));
                            }
                        }
                    }
                    if let Some((d2, gp)) = nearest {
                        let d = d2.sqrt();
                        // Stronger tug the closer the prize, fading out at the edge of lure range.
                        let prox = 1.0 - d / MAGNET_LURE_RADIUS; // 0 at edge, ~1 up close
                        let dir = (gp - crab.pos).normalize_or_zero();
                        crab.vel = crab.vel.lerp(dir * crab.crab_type.speed_range().end, 0.05);
                        crab.speed = 1.0;
                        crab.pos += dir * (prox * 30.0) * dt; // small positional nudge on top of the steer
                        if crab.magnet_lured <= 0.0 {
                            magnet_lure_pops.push(crab.pos);
                        }
                        crab.magnet_lured = 0.3; // refreshed each frame the chase holds
                    }
                }
                // The lure fades the instant a Magnet stops chasing (no Golden in range), so the
                // gold-tinted aura only shows while it's actually drifting after a prize.
                if crab.magnet_lured > 0.0 {
                    crab.magnet_lured = (crab.magnet_lured - dt).max(0.0);
                }

                // Flag this Magnet as charged if it's one of the ones pinning a snared Golden this
                // frame (positions were snapshotted just before the loop and nothing has moved a
                // Magnet since, so exact position match is safe). Refresh a short window so the
                // supercharged aura holds smoothly while it keeps the prize, then decays once the
                // Golden slips free or gets caught.
                if crab.is_magnet() {
                    // Only a Golden-pin (the first golden_charged_count entries) tops the charge up
                    // each frame; a Dancer-thumped surge is past that split and must decay on its own
                    // so the pull surge is a brief on-beat flare, not a permanent field.
                    if charged_magnet_positions[..golden_charged_count].contains(&crab.pos) {
                        crab.magnet_charged = 0.2;
                    } else if crab.magnet_charged > 0.0 {
                        crab.magnet_charged = (crab.magnet_charged - dt).max(0.0);
                    }
                }

                // Thief homing: a free Thief that isn't in the beam (being caught) or charmed
                // (whistled off) steers hard toward the conga tail so it can latch on and start
                // peeling links. Only the tail — never the head — so it always attacks the exposed
                // end. Once latched (latch_timer > 0) steal_chain_thief pins it to the tail, so we
                // stop steering here to avoid fighting that.
                if crab.is_thief()
                    && !crab_in_light
                    && crab.charm_timer <= 0.0
                    && crab.latch_timer <= 0.0
                {
                    // Emergent crossover: a roaming Magnet intercepts a homing Thief. Before the
                    // Thief reaches your tail to latch, if it strays deep into a Magnet's field the
                    // lodestone overpowers its beeline and hauls it into the cluster — so parking a
                    // Magnet between your train and an incoming Thief becomes a defensive routing
                    // play, the pre-latch mirror of the Magnet-pry that rips an already-latched
                    // Thief off. Reuses the same deep-field test as the Golden snare — and the
                    // same nearest-magnet lookup computed just above, instead of re-scanning
                    // magnet_positions a second time for every free Thief.
                    let mut intercepted = false;
                    if let Some((d2, mp)) = nearest_magnet {
                        let prox = 1.0 - d2.sqrt() / MAGNET_RADIUS; // 0 at edge, 1 at magnet
                        if prox > 0.4 {
                            let dir = (mp - crab.pos).normalize_or_zero();
                            // Overpowering drag toward the lodestone, tightening as it sinks in.
                            let pull = (prox - 0.4) / 0.6 * 240.0;
                            crab.pos += dir * pull * dt;
                            crab.vel *= 1.0 - (0.85 * dt).min(0.5); // kill its homing momentum
                            if crab.magnet_snared <= 0.0 {
                                thief_snare_pops.push(crab.pos);
                            }
                            crab.magnet_snared = 0.25; // refreshed each frame it stays snared
                            intercepted = true;
                        }
                    }
                    // Emergent crossover: a fleeing Golden lures a homing Thief off your tail. A
                    // thief can't resist a shiny thing — if a free Golden is nearer than the tail
                    // (and inside lure range), its shine overpowers the raider's beeline and it
                    // chases the prize instead of your train. The mirror of the Golden-lures-Magnet
                    // pass above: there gold tugs the lodestone, here gold tugs the raider. It turns
                    // a fleeing Golden into an accidental decoy — a real relief for a train under
                    // raid — but if the Thief catches the shine it just parks a threat right on the
                    // prize you were chasing. Magnet interception still wins (that's a physical drag,
                    // this is only attention), so it only runs when not intercepted. Reuses the
                    // golden_lure_positions snapshot already built for the Magnet lure — no new scan.
                    let mut lured = false;
                    if !intercepted && !golden_lure_positions.is_empty() {
                        const THIEF_LURE_RADIUS: f32 = 260.0;
                        const THIEF_LURE_RADIUS_SQ: f32 = THIEF_LURE_RADIUS * THIEF_LURE_RADIUS;
                        // Only divert to a Golden that's genuinely closer than the tail it's homing
                        // for — a shine across the arena shouldn't pull it off a tail right beside it.
                        let tail_d2 = thief_tail_pos
                            .map_or(f32::INFINITY, |tp| crab.pos.distance_squared(tp));
                        let mut nearest: Option<(f32, Vec2)> = None;
                        for &gp in golden_lure_positions.iter() {
                            let d2 = crab.pos.distance_squared(gp);
                            if d2 < THIEF_LURE_RADIUS_SQ
                                && d2 < tail_d2
                                && nearest.map_or(true, |(bd2, _)| d2 < bd2)
                            {
                                nearest = Some((d2, gp));
                            }
                        }
                        if let Some((d2, gp)) = nearest {
                            let d = d2.sqrt();
                            // Stronger tug the closer the prize; leans hard so the divert reads as
                            // the Thief abandoning the raid, not just wobbling toward the shine.
                            let prox = 1.0 - d / THIEF_LURE_RADIUS; // 0 at edge, ~1 up close
                            let dir = (gp - crab.pos).normalize_or_zero();
                            let chase_speed = crab.crab_type.speed_range().end * 1.3;
                            crab.vel = crab.vel.lerp(dir * chase_speed, 0.10 + prox * 0.10);
                            crab.speed = 1.0;
                            if crab.thief_lured <= 0.0 {
                                thief_lure_pops.push(crab.pos);
                            }
                            crab.thief_lured = 0.3; // refreshed each frame the divert holds
                            lured = true;
                        }
                    }
                    // The lure fades the instant the Thief loses its shiny target, so the gold-tinted
                    // aura only shows while it's actually being pulled off the raid.
                    if crab.thief_lured > 0.0 {
                        crab.thief_lured = (crab.thief_lured - dt).max(0.0);
                    }

                    if !intercepted && !lured {
                        if let Some(tp) = thief_tail_pos {
                            let dir = (tp - crab.pos).normalize_or_zero();
                            // Drive it in at a good clip so a Thief spawning across the arena still
                            // reaches your tail while the train is worth stealing from.
                            let home_speed = crab.crab_type.speed_range().end * 1.4;
                            crab.vel = crab.vel.lerp(dir * home_speed, 0.08);
                            crab.speed = 1.0;
                        }
                    }
                }

                // Beat-synced positional wobble for idle (non-spooked) crabs.
                if crab.spooked_timer == 0.0 {
                    let beat_phase = (1.0 - self.beat_timer / self.beat_interval)
                        * std::f32::consts::TAU
                        + crab.beat_phase_offset;
                    let perp = Vec2::new(-crab.vel.y, crab.vel.x).normalize_or_zero();
                    crab.pos += perp * 10.0 * beat_phase.sin() * dt;
                }

                // Bounce off walls.
                let (width, height) = area;
                if crab.pos.x < 0.0 || crab.pos.x > width - crab.scale {
                    crab.vel.x = -crab.vel.x;
                    crab.pos.x = crab.pos.x.clamp(0.0, width - crab.scale);
                }
                if crab.pos.y < 0.0 || crab.pos.y > height - crab.scale {
                    crab.vel.y = -crab.vel.y;
                    crab.pos.y = crab.pos.y.clamp(0.0, height - crab.scale);
                }

                // Universal speed cap — clamp vel so no compounding force (bounces, scatter
                // kicks, lasso drag) can push a crab to visually broken teleport speeds.
                // vel may carry full speed (crab.speed==1) or be a unit heading (speed in
                // crab.speed); clamp the effective combined magnitude in both cases.
                let effective_speed = crab.vel.length() * crab.speed;
                if effective_speed > MAX_CRAB_SPEED {
                    let scale = MAX_CRAB_SPEED / effective_speed;
                    crab.vel *= scale;
                    // crab.speed is left alone — it's a baseline the AI uses for decisions;
                    // only the instantaneous vel magnitude is capped.
                }

                // Smoothly rotate crab to face its movement direction
                let speed = crab.vel.length();
                if speed > 5.0 {
                    let target_angle = crab.vel.y.atan2(crab.vel.x);
                    let mut delta = target_angle - crab.facing_angle;
                    while delta > std::f32::consts::PI {
                        delta -= std::f32::consts::TAU;
                    }
                    while delta < -std::f32::consts::PI {
                        delta += std::f32::consts::TAU;
                    }
                    crab.facing_angle += delta * (dt * 8.0).min(1.0);
                }

                // Searing sparks — ONLY for a shelled target the beam is actively burning down
                // (drain is live). Not a herd attraction cue anymore: normal crabs in the cone emit
                // nothing. The sparks spray OUTWARD off the scorched shell (not toward the player)
                // and burn harsh white-hot, so the read is "this thing is melting under the beam",
                // reinforcing the flashlight's boss-pressure identity.
                let searing = crab_in_light && crab.boss_health > 0.0 && !crab.is_hermit();
                if searing {
                    // ~14 sparks per second while burning — a dense, unmistakable scorch spray.
                    if rng.random_range(0.0_f32..1.0_f32) < dt * 14.0 {
                        // Spray back along the beam (away from the player, off the hit face).
                        let off_beam = (crab.pos - self.player_pos).normalize_or_zero();
                        let perp = Vec2::new(-off_beam.y, off_beam.x);
                        let spread = rng.random_range(-0.9_f32..0.9_f32);
                        let dir = (off_beam + perp * spread).normalize_or_zero();
                        let speed = rng.random_range(90.0_f32..190.0_f32);
                        let life = rng.random_range(0.25_f32..0.5_f32);
                        // Harsh white-yellow scorch, occasionally flaring to orange ember.
                        let hot = rng.random_range(0.0_f32..1.0_f32) < 0.35;
                        let color = if hot {
                            [1.0, 0.55, 0.15]
                        } else {
                            [1.0, 0.95, 0.7]
                        };
                        attraction_particles.push((crab.pos, dir * speed, life, color));
                    }
                }
            }
        }

        // Sync the Reef DJ phrase state after the &mut self.crabs loop. reef_active gates the phrase
        // roll and HUD telegraph; clearing it when the DJ leaves the field wipes any stale phrase so
        // the next DJ starts fresh. A landed hot-beat hit kicks a juice bloom + a little flash.
        self.reef_active = reef_on_field;
        if !reef_on_field {
            self.reef_phrase = [false; 4];
            self.reef_phrase_bar = u32::MAX;
            self.reef_dancer_timer = 0.0;
        } else if reef_hit_landed {
            self.reef_hit_flash = 1.0;
            self.on_beat_flash = self.on_beat_flash.max(0.3);
        }

        // Reef DJ backup dancers. The boss clears the herd for a clean duel, so bring one archetype
        // back into the arena as a fight mechanic: the DJ summons "hype Dancers" on a timer. They
        // drift and hop on the beat like any Dancer, but catching one *on a called (hot) beat* chips
        // the boss shell (see the catch loop), so herding them onto the phrase is an active second
        // way to crack the DJ beyond just holding light. Cap how many are loose so the duel stays
        // legible — a couple to chase, not a swarm — and only summon while the DJ still has shell.
        if reef_on_field {
            self.reef_dancer_timer -= dt;
            if self.reef_dancer_timer <= 0.0 {
                let loose_dancers = self
                    .crabs
                    .iter()
                    .filter(|c| !c.caught && !c.is_boss() && c.is_dancer())
                    .count();
                if loose_dancers < 3 {
                    let mut rng = rand::rng();
                    let dancer = spawn_hype_dancer(
                        (self.world_width, self.world_height),
                        reef_boss_pos,
                        &mut rng,
                    );
                    let dpos = dancer.pos;
                    self.crabs.push(dancer);
                    // Little violet summon puff so the dancer reads as the DJ's call, not a stray.
                    self.particle_system
                        .spawn_milestone_fireworks(dpos, 5, &mut rng);
                }
                self.reef_dancer_timer = 3.0;
            }
        }

        // Push sparkle particles for attracted crabs (done outside loop to avoid borrow conflict).
        // One rng per batch rather than one per particle — rand::rng() re-seeds on every call
        // and the flashlight can accumulate many attracted crabs at once.
        if !attraction_particles.is_empty() {
            let mut rng = rand::rng();
            for &(pos, vel, life, [cr, cg, cb]) in attraction_particles.iter() {
                self.particle_system.push(crate::graphics::Particle {
                    pos,
                    vel,
                    life,
                    max_life: life,
                    size: rng.random_range(1.5_f32..3.5_f32),
                    color: [
                        (cr * 0.6 + 0.4).min(1.0),
                        (cg * 0.6 + 0.4).min(1.0),
                        (cb * 0.6 + 0.4).min(1.0),
                    ],
                });
            }
        }

        // Celebrate any King Crab worn down to catchable this frame
        for &pos in boss_broke.iter() {
            self.floating_texts.spawn(
                "SHELL CRACKED!".to_string(),
                pos - Vec2::new(96.0, 60.0),
                40.0,
                [1.0, 0.95, 0.6, 1.0],
            );
            self.floating_texts.spawn(
                "CATCH IT!".to_string(),
                pos - Vec2::new(64.0, 20.0),
                30.0,
                [0.4, 1.0, 0.5, 1.0],
            );
            // Telegraphed pop: the shell finally gives under sustained beam pressure. A hot burst of
            // scorch sparks off the crack, a double shockwave (white flash + hot ring), a hard shake,
            // and a brief hitstop that STAGGERS the moment so it lands as a satisfying "pop" beat
            // rather than a health number quietly hitting zero.
            self.spawn_catch_shockwave(pos, [1.0, 0.98, 0.85]);
            self.spawn_catch_shockwave(pos, [1.0, 0.6, 0.15]);
            self.particle_system
                .spawn_milestone_fireworks(pos, 14, &mut rand::rng());
            self.screen_shake = self.screen_shake.max(22.0);
            let a = rand::rng().random_range(0.0_f32..std::f32::consts::TAU);
            self.screen_shake_vel = Vec2::new(a.cos(), a.sin()) * 16.0 * 60.0;
            self.on_beat_flash = self.on_beat_flash.max(0.6);
            // Freeze-frame the crack — a strong hitstop so a boss shell breaking is the single most
            // emphatic pop the beam can produce.
            self.hitstop_timer = self.hitstop_timer.max(0.16);
        }

        // A boss just crossed into its enrage phase — the fight's final act. A hard jolt, a big
        // menacing shockwave in the boss's own color, and an "ENRAGED!" shout mark the turn so the
        // ramp in aggression reads as a deliberate escalation, not random difficulty.
        for &(pos, is_tide) in boss_enrages.iter() {
            let (ring_col, txt_col): ([f32; 3], [f32; 4]) = if is_tide {
                ([0.3, 0.75, 1.0], [0.5, 0.9, 1.0, 1.0])
            } else {
                ([1.0, 0.4, 0.15], [1.0, 0.55, 0.2, 1.0])
            };
            self.floating_texts.spawn(
                "ENRAGED!".to_string(),
                pos - Vec2::new(72.0, 58.0),
                42.0,
                txt_col,
            );
            self.spawn_catch_shockwave(pos, ring_col);
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((pos, 0.0));
            }
            self.particle_system
                .spawn_milestone_fireworks(pos, 10, &mut rand::rng());
            self.screen_shake = self.screen_shake.max(20.0);
            let a = rand::rng().random_range(0.0_f32..std::f32::consts::TAU);
            self.screen_shake_vel = Vec2::new(a.cos(), a.sin()) * 20.0 * 60.0;
            self.on_beat_flash = self.on_beat_flash.max(0.5);

            // Arena-shifting enrage: the boss doesn't just get angrier, it reshapes the duel space
            // for its final act. A King Crab cracks the floor into hazard fissures to weave around;
            // a Tide Boss floods the arena with extra wade-drag pools so routing changes mid-fight.
            if is_tide {
                self.flood_arena(pos);
            } else {
                self.crack_arena_fissures(pos);
            }
        }

        // King Crab winding up a charge: red alarm ring + shouted warning so the player has time
        // to route the tail out of the lane before the lunge commits.
        for &pos in boss_windups.iter() {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((pos, 0.0));
            }
            self.floating_texts.spawn(
                "CHARGE INCOMING!".to_string(),
                pos - Vec2::new(96.0, 52.0),
                30.0,
                [1.0, 0.45, 0.2, 1.0],
            );
            self.on_beat_flash = self.on_beat_flash.max(0.25);
        }

        // The lunge fires: a jolt and a hot shockwave sell the commitment.
        for &pos in boss_launches.iter() {
            self.spawn_catch_shockwave(pos, [1.0, 0.5, 0.2]);
            self.screen_shake = self.screen_shake.max(10.0);
            let kick_angle = rand::rng().random_range(0.0_f32..std::f32::consts::TAU);
            self.screen_shake_vel = Vec2::new(kick_angle.cos(), kick_angle.sin()) * 8.0 * 60.0;
        }

        // Emergent crossover feedback: a charging King Crab just rammed a free Armored crab's shell.
        // The wall held — the boss's lunge is spent and the tail it was aimed at is spared. Sell it
        // as a hard impact (shell-clang shockwave in Armored slate-blue, a jolt, a proud "BLOCKED!"
        // callout) and shove the shell crab back off the boss so the collision reads physically.
        for &(boss_pos, shell_pos) in boss_blocks.iter() {
            let knock_dir = (shell_pos - boss_pos).normalize_or_zero();
            let knock_dir = if knock_dir == Vec2::ZERO {
                Vec2::new(0.0, -1.0)
            } else {
                knock_dir
            };
            for crab in self.crabs.iter_mut() {
                if crab.is_armored() && !crab.caught && crab.pos.distance(shell_pos) < 1.0 {
                    // Knock the shell crab back along the charge line — a solid shove, not a panic
                    // flee: Armored stays calm (it's a wall), it just gets bumped.
                    crab.vel = knock_dir * crab.crab_type.speed_range().end * 1.8;
                    crab.speed = 1.0;
                    break;
                }
            }
            self.spawn_catch_shockwave(shell_pos, [0.55, 0.62, 0.72]); // Armored slate-blue clang
            self.floating_texts.spawn(
                "BLOCKED!".to_string(),
                shell_pos - Vec2::new(40.0, 40.0),
                30.0,
                [0.7, 0.82, 0.95, 1.0],
            );
            self.screen_shake = self.screen_shake.max(8.0);
            let kick_angle = rand::rng().random_range(0.0_f32..std::f32::consts::TAU);
            self.screen_shake_vel = Vec2::new(kick_angle.cos(), kick_angle.sin()) * 7.0 * 60.0;
        }

        // Feedback for a King Crab dazed by the shell ram above: a woozy callout on top of the
        // BLOCKED! pop, so the stun window (see stun_timer/is_stunned in enemies.rs) reads as a
        // real payoff moment, not a silent state flip.
        for &pos in boss_stuns.iter() {
            self.floating_texts.spawn(
                "DAZED!".to_string(),
                pos - Vec2::new(36.0, 70.0),
                26.0,
                [1.0, 0.9, 0.4, 1.0],
            );
        }

        // Tide Boss starting to swell a pulse: a cold warning ring + shout so the player can pull
        // the train back out of range before the shockwave lands.
        for &pos in tide_swells.iter() {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((pos, 0.0));
            }
            self.floating_texts.spawn(
                "TIDE SURGE — BACK AWAY!".to_string(),
                pos - Vec2::new(130.0, 52.0),
                30.0,
                [0.4, 0.85, 1.0, 1.0],
            );
            self.on_beat_flash = self.on_beat_flash.max(0.25);
        }

        // The pulse fires: spawn the expanding shockwave, scatter nearby free crabs, and knock the
        // train's tail loose if it's clustered too close.
        for &center in tide_fires.iter() {
            self.tide_pulse_burst(center);
        }

        // Dust kicked up behind the charging boss — sprayed opposite the lunge heading.
        {
            let mut rng = rand::rng();
            for &(pos, vel) in boss_charge_dust.iter() {
                if rng.random_range(0.0_f32..1.0_f32) >= dt * 90.0 {
                    continue; // throttle so a long lunge doesn't flood the particle pool
                }
                let back = (-vel).normalize_or_zero();
                let perp = Vec2::new(-back.y, back.x);
                let spread = rng.random_range(-0.5_f32..0.5_f32);
                let dir = (back + perp * spread).normalize_or_zero();
                let speed = rng.random_range(50.0_f32..140.0_f32);
                let life = rng.random_range(0.3_f32..0.6_f32);
                self.particle_system.push(crate::graphics::Particle {
                    pos,
                    vel: dir * speed,
                    life,
                    max_life: life,
                    size: rng.random_range(2.0_f32..4.5_f32),
                    color: [0.85, 0.7, 0.5],
                });
            }
        }

        // Armored shells the beam just wore through — a lighter "crack" than the boss fanfare, but
        // still a scorch pop: a hot-tinted shockwave, a short shake and a light hitstop so burning a
        // shell open with the beam reads as a satisfying break rather than a silent state flip.
        for &pos in armor_broke.iter() {
            self.floating_texts.spawn(
                "SHELL CRACKED!".to_string(),
                pos - Vec2::new(70.0, 40.0),
                26.0,
                [1.0, 0.92, 0.6, 1.0],
            );
            self.spawn_catch_shockwave(pos, [1.0, 0.8, 0.35]);
            self.screen_shake = self.screen_shake.max(9.0);
            self.hitstop_timer = self.hitstop_timer.max(0.06);
        }

        // Emit "!" floating texts for crabs that just started fleeing this frame
        for &pos in flee_pops.iter() {
            self.floating_texts.spawn(
                "!".to_string(),
                pos - Vec2::new(0.0, 24.0),
                28.0,
                [1.0, 0.9, 0.1, 1.0],
            );
        }

        // Celebrate any Golden a Magnet just snared this frame — a bright gold-into-magnet-orange
        // pop and a shockwave so "the Magnet trapped the prize" reads as a moment, the same way the
        // Magnet-pry-Thief save does.
        for pos in golden_snare_pops.drain(..) {
            self.floating_texts.spawn(
                "SNARED!".to_string(),
                pos - Vec2::new(0.0, 30.0),
                26.0,
                [1.0, 0.7, 0.2, 1.0], // Magnet's lodestone orange claiming the golden prize
            );
            self.spawn_catch_shockwave(pos, [1.0, 0.78, 0.25]);
        }

        // Celebrate any homing Thief a Magnet just intercepted this frame — a green-into-magnet-
        // orange pop and a shockwave so "the Magnet caught the raider before it reached your tail"
        // reads as the defensive save it is, mirroring the Golden snare's callout.
        for pos in thief_snare_pops.drain(..) {
            self.floating_texts.spawn(
                "INTERCEPTED!".to_string(),
                pos - Vec2::new(0.0, 30.0),
                24.0,
                [0.55, 0.9, 0.4, 1.0], // Thief's poison-green pulled into the Magnet's field
            );
            self.spawn_catch_shockwave(pos, [0.7, 0.85, 0.35]);
        }

        // Note when a Magnet first breaks off after a Golden — a small gold-orange callout so the
        // lure reads as a moment ("the prize pulled the lodestone off your herd") rather than the
        // Magnet silently wandering. Gentler than the snare/intercept saves (no shockwave): this is
        // a wrinkle in routing, not a rescue, and firing a big burst every time a Golden drifts past
        // a Magnet would be noisy.
        for pos in magnet_lure_pops.drain(..) {
            self.floating_texts.spawn(
                "LURED!".to_string(),
                pos - Vec2::new(0.0, 30.0),
                22.0,
                [1.0, 0.8, 0.35, 1.0], // gold prize bleeding into the Magnet's lodestone orange
            );
        }

        // Note when a fleeing Golden first pulls a homing Thief off your tail — a small green-into-
        // gold callout so the relief reads as a moment ("the shine drew the raider off your train")
        // rather than the Thief silently wandering. Gentler than the Magnet saves (no shockwave):
        // like the Magnet lure, it's a routing wrinkle, not a rescue, and the Golden decoy is
        // accidental, so a big burst every time would be noisy.
        for pos in thief_lure_pops.drain(..) {
            self.floating_texts.spawn(
                "SHINY!".to_string(),
                pos - Vec2::new(0.0, 30.0),
                22.0,
                [0.7, 0.95, 0.4, 1.0], // Thief's poison-green catching the golden gleam
            );
        }

        // Note when a charged Magnet's vacuum grinds an Armored shell — same CHIPPED!/SHELL CRACKED!
        // cues as the Dancer-chip and Stomp crack so the shell-progress language stays consistent,
        // but tinted the Magnet's lodestone orange so the "the charged pull did this" story reads.
        for (pos, broke, was_hermit) in magnet_grind.drain(..) {
            // A charged Magnet ripping a Hermit clean out fires the signature copper Hermit-pop — a
            // pure archetype-web crack (the beam can't do it), so it earns its own watchable beat.
            if broke && was_hermit {
                self.spawn_hermit_pop(pos);
                continue;
            }
            let (label, burst) = if broke {
                ("SHELL CRACKED!", [0.7, 0.8, 0.95]) // fully open — matches the Stomp/Dancer crack cue
            } else {
                ("CHIPPED!", [0.62, 0.68, 0.78]) // a chink ground loose, more shell to go
            };
            self.floating_texts.spawn(
                label.to_string(),
                pos - Vec2::new(52.0, 30.0),
                24.0,
                [1.0, 0.7, 0.3, 1.0], // Magnet's lodestone orange so the source reads at a glance
            );
            self.spawn_catch_shockwave(pos, burst);
        }

        // Hand the scratch buffers back so next frame's std::mem::take reuses this frame's
        // allocation instead of starting from an empty Vec.
        self.magnet_grind_buf = magnet_grind;
        self.flee_pops_buf = flee_pops;
        self.golden_snare_pops_buf = golden_snare_pops;
        self.thief_snare_pops_buf = thief_snare_pops;
        self.magnet_lure_pops_buf = magnet_lure_pops;
        self.thief_lure_pops_buf = thief_lure_pops;
        self.boss_broke_buf = boss_broke;
        self.armor_broke_buf = armor_broke;
        self.attraction_particles_buf = attraction_particles;
        self.boss_windups_buf = boss_windups;
        self.boss_launches_buf = boss_launches;
        self.boss_charge_dust_buf = boss_charge_dust;
        self.boss_enrages_buf = boss_enrages;
        self.tide_fires_buf = tide_fires;
        self.tide_swells_buf = tide_swells;
        self.magnet_positions_buf = magnet_positions;
        self.golden_lure_positions_buf = golden_lure_positions;
        self.charged_magnet_positions_buf = charged_magnet_positions;
        self.armored_positions_buf = armored_positions;
        self.boss_blocks_buf = boss_blocks;
        self.boss_stuns_buf = boss_stuns;

        // Move chain crabs to their historical positions (conga train). Walking self.crabs
        // mutably and consulting self.position_history in the same pass (rather than
        // collecting an intermediate Vec<(usize, Vec2)> of chain targets first) avoids a
        // per-frame heap allocation that used to scale with conga chain length.
        let mut dust_rng = rand::rng();
        for crab in &mut self.crabs {
            let Some(ci) = crab.chain_index else { continue };
            let history_idx = (ci + 1) * CHAIN_LINK_FRAMES;
            let Some(&target) = self.position_history.get(history_idx) else {
                continue;
            };
            let old_pos = crab.pos;
            crab.pos = old_pos.lerp(target, 0.4);
            // Rotate caught crab toward the direction it just moved
            let move_dir = crab.pos - old_pos;
            // Compute the length once; reuse it for the facing-angle threshold, dust speed, and
            // normalize — three operations that each used to call sqrt independently per chain link.
            let move_len = move_dir.length();
            let move_speed = move_len / dt.max(1e-4);
            // Kick up a little dust from the crab's feet as the conga train stampedes along.
            let feet = crab.pos + Vec2::new(0.0, CRAB_SIZE * 0.35);
            self.particle_system.spawn_conga_dust(
                feet,
                move_dir,
                dt,
                move_len,
                move_speed,
                &mut dust_rng,
            );
            if move_len > 0.5 {
                let target_angle = move_dir.y.atan2(move_dir.x);
                let mut d = target_angle - crab.facing_angle;
                while d > std::f32::consts::PI {
                    d -= std::f32::consts::TAU;
                }
                while d < -std::f32::consts::PI {
                    d += std::f32::consts::TAU;
                }
                crab.facing_angle += d * (dt * 6.0).min(1.0);
            }
            // Beat-synced conga step: the train physically hops forward on each beat, and the
            // hop ripples down the line — each link lags the one ahead by a fixed phase — so the
            // whole train visibly steps to the rhythm instead of just gliding after the player.
            // This is gameplay reacting to the beat, not only visuals: the crabs move to it. The
            // lerp above continuously reels each crab back to its chain target every frame, so
            // this direct forward offset self-corrects and can never accumulate or drift the
            // train off its path.
            // Reuse the pre-computed length for normalize: if the move was large enough to face-
            // update (len > 0.5), divide directly instead of calling normalize_or_zero (another sqrt).
            let travel = if move_len > 1e-6 {
                move_dir / move_len
            } else {
                Vec2::ZERO
            };
            if travel != Vec2::ZERO {
                let step_phase = (1.0 - self.beat_timer / self.beat_interval)
                    * std::f32::consts::TAU
                    - ci as f32 * 0.7;
                let hop = step_phase.sin().max(0.0); // forward-only footfall each beat
                // The bar's "1" stomps forward noticeably farther than the three beats between it,
                // so the train lands the downbeat as a bigger unified lunge. bar_accent decays over
                // a beat, so the boost tapers off by the next between-beat footfall.
                let stomp = 4.0 * (1.0 + self.bar_accent * 1.6);
                crab.pos += travel * hop * stomp;
            }
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
            self.level_title_timer = 1.0;
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

    fn reset_game(&mut self) {
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
        self.tail_run_len = 0;
        self.kelp_snag_warn = 0.0;
        self.beat_timer = BEAT_INTERVAL;
        self.beat_intensity = 0.0;
        self.music_intensity = 0.0;
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

    fn draw_instructions_screen(
        &mut self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        width: f32,
        height: f32,
    ) -> GameResult {
        if self.show_how_to_play_text {
            let mut title = Text::new("HOW TO PLAY");
            title.set_scale(56.0);
            let title_w = title.measure(ctx)?.x;
            canvas.draw(
                &title,
                DrawParam::default()
                    .dest(Vec2::new((width - title_w) * 0.5, height * 0.12))
                    .color(Color::from_rgb(235, 235, 220)),
            );

            let body = how_to_play_body_text();
            let mut text = Text::new(body);
            text.set_scale(28.0);
            canvas.draw(
                &text,
                DrawParam::default()
                    .dest(Vec2::new(width * 0.16, height * 0.27))
                    .color(Color::from_rgb(215, 215, 215)),
            );
            return Ok(());
        }
        menu::draw_menu(self, ctx, canvas, width, height)
    }

    /// Top-left world coordinate of the visible viewport: centre the player, then clamp so the
    /// camera never shows past the world's edges (no void beyond the playfield). When the world is
    /// smaller than the viewport in a dimension the clamp collapses to 0 (whole world visible).
    fn compute_camera_origin(&self) -> Vec2 {
        let focus = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        let x = (focus.x - self.width / 2.0).clamp(0.0, (self.world_width - self.width).max(0.0));
        let y =
            (focus.y - self.height / 2.0).clamp(0.0, (self.world_height - self.height).max(0.0));
        Vec2::new(x, y)
    }

    fn draw_game(
        &self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        width: f32,
        height: f32,
    ) -> GameResult {
        // The world (playfield) is larger than the viewport (width/height). Ground-layer fills
        // (grass, biome pulse, ambient motes) must cover the whole world so no void shows past the
        // viewport edge as the camera scrolls; the HUD later switches back to viewport dims.
        let world_w = self.world_width;
        let world_h = self.world_height;

        // Select texture for current level.
        let texture = match self.level_textures[self.current_level] {
            LevelTexture::Grass => &self.textures.grass,
            LevelTexture::Sand => &self.textures.sand,
        };

        // Biome for the current zone (clamped so a finished run doesn't index past the end).
        let biome = self.levels[self.current_level.min(self.levels.len() - 1)].biome;
        let (tr, tg, tb) = biome.tint;

        // Fold the day/night grade into the ground tint so the whole world shifts together with the
        // sky overlay below — dawn amber → midday bright → dusk orange-pink → night deep blue. A
        // lightning flash briefly floods the ground bright white to match the sky flash.
        let (dr, dg, db) = self.day_tint();
        let flash = self.lightning_flash.clamp(0.0, 1.0);
        let ground_r = ((tr as f32 * dr) + 255.0 * flash * 0.25).min(255.0) as u8;
        let ground_g = ((tg as f32 * dg) + 255.0 * flash * 0.25).min(255.0) as u8;
        let ground_b = ((tb as f32 * db) + 255.0 * flash * 0.25).min(255.0) as u8;

        // Draw level background, color-graded to the current biome and time of day.
        draw_grass(
            ctx,
            canvas,
            world_w,
            world_h,
            texture,
            &self.shader,
            self.time_elapsed,
            // Beat phase 0→1 across a beat (0 the instant one lands), so the grass shader can
            // fire a ripple of light outward from center on every downbeat.
            (1.0 - self.beat_timer / self.beat_interval).clamp(0.0, 1.0),
            Color::from_rgb(ground_r, ground_g, ground_b),
        )?;

        // World-space sky overlay: a soft full-world tint carrying the day/night mood plus the
        // cloudy/rain grey dimming. Sits over the ground but under the action. Rain streaks, the
        // edge vignette and the lightning full-screen flash draw later in SCREEN space so they're
        // camera-independent.
        draw_sky_overlay(
            ctx,
            canvas,
            world_w,
            world_h,
            self.day_phase_t,
            self.weather_intensity,
        )?;

        // World-edge boundary: a soft darkening that fades inward from the true playfield limits,
        // so scrolling to the edge of the larger-than-viewport world reads as arriving at a shore
        // rather than an abrupt camera clamp. World space, tinted to the biome accent, under the
        // action. Only visible when the camera actually reaches an edge.
        {
            let (er, eg, eb) = biome.pulse;
            draw_world_edge(
                ctx,
                canvas,
                world_w,
                world_h,
                Color::from_rgb(er, eg, eb),
                self.night_factor(),
            )?;
        }

        // Subtle beat pulse: an on-beat flash tinted to match the current biome's mood. At night the
        // pulse glows brighter — the beat is the one thing that reads MORE in the dark, trading the
        // dimmed base visibility for a stronger rhythmic cue.
        if self.beat_intensity > 0.0 {
            let night_glow = 1.0 + self.night_factor() * 0.6;
            let pulse_alpha = (self.beat_intensity * 28.0 * night_glow).min(255.0) as u8;
            let (pr, pg, pb) = biome.pulse;
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .scale(Vec2::new(world_w, world_h))
                    .color(Color::from_rgba(pr, pg, pb, pulse_alpha)),
            );
        }

        // Ambient atmosphere: a field of slow-drifting motes over the ground (sea spray / drifting
        // spores) that give the space between the action depth and life, tinted to the biome's accent
        // and bobbing gently on the beat. Stateless and cheap (one batched draw), sits above the
        // ground flash but under the tide pools and all the action.
        {
            let (ar, ag, ab) = biome.pulse;
            draw_ambient_motes(
                ctx,
                canvas,
                world_w,
                world_h,
                self.time_elapsed,
                self.beat_intensity,
                Color::from_rgb(ar, ag, ab),
            )?;
        }

        // Rain puddle ripples on the ground (world space, under the action) — expanding rings that
        // pop where rain "lands", scaled up with weather intensity. Only visible once it's actually
        // raining; sits over the ambient motes but under the tide pools/crabs. Camera origin lets it
        // cover just the visible viewport slice of the world so it isn't wasted off-screen.
        if self.weather_intensity > 0.35 {
            draw_puddle_ripples(
                ctx,
                canvas,
                self.camera_origin,
                width,
                height,
                self.time_elapsed,
                self.weather_intensity,
            )?;
        }

        // Tide pools — terrain hazards on the ground layer, under the crabs/rope, so the train
        // visibly wades through the water it's being routed around. When a Tide Boss has flooded the
        // arena, the last `boss_flood_pools` entries are its surge water: they always read as water
        // regardless of the biome's native terrain skin (rock/kelp/open), so we draw the biome's own
        // pools with the biome terrain, then the flood slice explicitly as water on top.
        let native_pool_count = self.tide_pools.len().saturating_sub(self.boss_flood_pools);
        // Only the Water biome's native pools carry the Tide Pool current, so only they draw the flow
        // streaks — matching exactly where the sim applies the drift (see update_crabs).
        let native_has_current = biome.terrain == crate::levels::TerrainKind::Water;
        draw_tide_pools(
            ctx,
            canvas,
            &self.tide_pools[..native_pool_count],
            self.time_elapsed,
            self.beat_intensity,
            self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0),
            biome.terrain,
            native_has_current,
            self.rock_tide_fill,
        )?;
        if self.boss_flood_pools > 0 {
            draw_tide_pools(
                ctx,
                canvas,
                &self.tide_pools[native_pool_count..],
                self.time_elapsed,
                self.beat_intensity,
                self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0),
                crate::levels::TerrainKind::Water,
                // Flood pools render as water but carry no current — never streak them.
                false,
                // Flood pools draw as water, not rock, so the tide level is irrelevant here.
                0.0,
            )?;
        }

        // King Crab enrage set-piece: the cracked-floor fissures the boss split the arena into.
        // Drawn over the water so they read as hot hazards welling up through the ground.
        draw_boss_fissures(
            ctx,
            canvas,
            &self.boss_fissures,
            self.time_elapsed,
            self.beat_intensity,
            self.boss_fissure_erupt,
        )?;

        // Delivery pen — drawn on the ground layer under the crabs/rope so the train visibly rolls
        // into it. Lights up green once there's a train to bank (chain_count > 0). The "haul"
        // anticipation (0..1) scales the pen's excitement to the size of the incoming jackpot and
        // ramps up further as the loaded train closes in, so the biggest payoff moment in the game
        // — driving a fat conga line into the pen — builds visible tension *before* the bank.
        let haul = if self.chain_count > 0 {
            // Train size normalized against a "big haul" reference (~24 crabs reads as a jackpot),
            // then boosted as the player carries it into the pen's neighborhood so the pen strains
            // toward an approaching train rather than only reacting to its length.
            let size_term = (self.chain_count as f32 / 24.0).min(1.0);
            let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            let dist = player_center.distance(self.pen_pos);
            // 0 far away, ramps to 1 as the train enters ~2.5 pen-radii of the goal.
            let approach = (1.0 - (dist / (PEN_RADIUS * 2.5)).min(1.0)).max(0.0);
            (size_term * (0.55 + 0.45 * approach)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        // Live "what would this train bank right now" preview shown floating over the pen. Mirrors
        // the base delivery payout in try_deliver_train — the super-linear triangular sum times the
        // current combo + Groove-Gamble multipliers — but deliberately EXCLUDES the on-beat PERFECT
        // and delivery-streak bonuses, since those are only earned at the moment you actually bank on
        // beat. So it reads as the honest floor ("at least this much"), and timing the bank well pays
        // even more, keeping the on-beat delivery worth engaging rather than spoiling it.
        //
        // Precompute bonds for the full chain once — count_chain_bonds walks all crabs every call, and
        // it used to be called twice with the same argument (pen_worth preview + at-risk readout).
        // Cache it here so the second call is a free integer read instead of another O(n) scan.
        // Single scan for both bonds and sandwiches — avoids two separate O(n) walks over the
        // caught crabs for what is effectively the same chain_index→type lookup.
        let (bonds_n, sandwiches_n, run_bonus_n, centerpiece_n) = if self.chain_count > 0 {
            self.count_bonds_and_sandwiches(self.chain_count)
        } else {
            (0, 0, 0, 0)
        };
        let pen_worth = if self.chain_count > 0 {
            let n = self.chain_count;
            // Include the same arrangement (same-type adjacent pair) AND sandwich bonuses
            // try_deliver_train pays, so the live preview stays honest — holding a well-arranged
            // train visibly raises the pen worth, which is the whole point of making the middle of
            // the train matter.
            let base = (n * (n + 1) / 2) * 3
                + bonds_n * BOND_PAIR_BONUS
                + sandwiches_n * SANDWICH_BONUS
                + run_bonus_n
                + centerpiece_n;
            Some(
                (base as f32 * self.combo_multiplier() as f32 * self.beat_gamble_mult).round()
                    as usize,
            )
        } else {
            None
        };
        // Live HAUL readout floating over the PLAYER (where their eyes are) — the positive twin of the
        // red AT RISK tag on the tail. pen_worth already shows the total over the pen, but the
        // arrangement value is baked invisibly into it: the player can't tell how much of their haul is
        // raw length vs. the bonds/sandwiches/runs they deliberately arranged. Surfacing the arrangement
        // slice explicitly ("ARRANGED +N") is the agency/control the arrangement system was missing —
        // it lets the player SEE arranging pay off in the moment and steer to complete more of it,
        // instead of only discovering it at the pen. Shown from a 2-crab train up (arrangement can pay
        // before the snap-risk threshold), and only carrying the readout past the pen guide's near zone
        // so the two don't stack on top of each other.
        if let Some(worth) = pen_worth {
            if self.chain_count >= 2 {
                let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
                if player_center.distance(self.pen_pos) > PEN_RADIUS * 1.4 {
                    // The arrangement-only slice of the haul, run through the same live multipliers the
                    // total uses so the two numbers stay honest with each other.
                    let arr_base = bonds_n * BOND_PAIR_BONUS
                        + sandwiches_n * SANDWICH_BONUS
                        + run_bonus_n
                        + centerpiece_n;
                    let arranged =
                        (arr_base as f32 * self.combo_multiplier() as f32 * self.beat_gamble_mult)
                            .round() as usize;
                    draw_haul_worth(
                        ctx,
                        canvas,
                        player_center,
                        self.time_elapsed,
                        self.beat_intensity,
                        worth,
                        arranged,
                    )?;
                }
            }
        }

        // Delivery beam — a lit tether from where the train's head departed to the pen it cashed
        // into, drawn under the pen's own bloom while the deliver flash decays. Connects the two
        // ends of the bank so the payoff reads as the conga line rushing home, not just the pen
        // popping in place. Uses the snapshotted pen the bank actually paid into (the live pen has
        // relocated by now).
        if self.deliver_flash > 0.0 {
            draw_deliver_beam(
                ctx,
                canvas,
                self.deliver_beam_from,
                self.deliver_beam_to,
                self.deliver_flash,
                self.deliver_beam_perfect,
            )?;
        }
        draw_delivery_pen(
            ctx,
            canvas,
            self.pen_pos,
            PEN_RADIUS,
            self.time_elapsed,
            self.beat_intensity,
            self.chain_count > 0,
            haul,
            pen_worth,
            self.deliver_flash,
        )?;

        // Live "at-risk" readout — the downside half of the bank-now-vs-push-luck decision, mirroring
        // the gold pen-worth tag but for what a snap would cost you RIGHT NOW. A panic snap strips the
        // last panic_snap_links(n) tail links (snap_chain_on_panic), and because pen_worth is triangular
        // the tail links are the priciest ones, so the honest number is the MARGINAL loss pen_worth(n) -
        // pen_worth(keep), computed with the same combo/gamble multipliers so the two tags agree. That
        // link count now GROWS with train length, so a long unbanked train's at-risk number jumps at
        // each severity tier — the downside visibly mounts the longer you hold. Gated
        // to the same length threshold (MIN_TRAIN_TO_SNAP=5) at which a panic snap can actually fire —
        // below that there's genuinely no risk, so no tag; it appears exactly when holding turns
        // dangerous and the number climbs the longer you refuse to bank. Anchored on the tail in warning
        // red so it contrasts with the gold reward tag over the pen instead of blurring into it.
        if self.chain_count >= 5 {
            if let Some(tail_pos) = self.cached_tail_pos {
                let mult = self.combo_multiplier() as f32 * self.beat_gamble_mult;
                let tri = |m: usize| (m * (m + 1) / 2) * 3;
                let n = self.chain_count;
                // Use the SAME severity function the panic snap uses, so the readout can't lie:
                // a longer train shows a bigger at-risk number precisely because a snap tears more
                // (and pricier, since tri() is triangular) tail links off it.
                let keep = n.saturating_sub(panic_snap_links(n)).max(1);
                // Marginal loss folds in the arrangement bonus too: a snap tears off tail links,
                // which destroys every same-type bond in the torn region (and the one straddling the
                // cut), so the pricier a train's tail arrangement, the more a snap costs — mirroring
                // how the bank pays those same bonds. bonds(n) - bonds(keep) is what the cut erases.
                // bonds_n (bonds for the full chain) was precomputed above — reuse it instead of a
                // second O(n) crab scan with the same argument.
                let (bonds_keep, sandwiches_keep, run_bonus_keep, centerpiece_keep) =
                    self.count_bonds_and_sandwiches(keep);
                let bonds_lost = bonds_n.saturating_sub(bonds_keep);
                // Sandwiches destroyed by the cut too — any sandwich straddling or inside the torn
                // tail region is gone, so the at-risk number folds in its lost value the same way
                // the bank pays it. Mirrors bonds_lost exactly.
                let sandwiches_lost = sandwiches_n.saturating_sub(sandwiches_keep);
                // Deep-run block value lost to the cut, same logic: chopping the tail shortens (or
                // erases) any long matched run there, so the at-risk number reflects the run bonus
                // the bank would no longer pay — keeping the two tags honest with each other.
                let run_bonus_lost = run_bonus_n.saturating_sub(run_bonus_keep);
                // Centerpiece bonus lost to the cut: a straddling deep run in the full train may no
                // longer straddle the shortened train's new midpoint (or gets chopped below length 3),
                // so the at-risk number folds in the centerpiece the bank would no longer pay.
                let centerpiece_lost = centerpiece_n.saturating_sub(centerpiece_keep);
                let marginal = tri(n).saturating_sub(tri(keep))
                    + bonds_lost * BOND_PAIR_BONUS
                    + sandwiches_lost * SANDWICH_BONUS
                    + run_bonus_lost
                    + centerpiece_lost;
                let at_risk = (marginal as f32 * mult).round() as usize;
                // Danger ramps from the snap threshold up to a long train (~12), so color/pulse escalate.
                let danger01 = ((n.saturating_sub(5)) as f32 / 7.0).clamp(0.0, 1.0);
                draw_train_at_risk(ctx, canvas, tail_pos, self.time_elapsed, at_risk, danger01)?;
            }
        }

        // Kelp-snag telegraph: while the tail sits in the weeds and is long enough to snag, ring the
        // tail crab with a rising green warning so an imminent snag is seen coming and the player can
        // route out. Legibility only — the odds live in `snag_chain_on_kelp`.
        if self.kelp_snag_warn > 0.02 {
            if let Some(tail_pos) = self.cached_tail_pos {
                draw_kelp_snag_warning(
                    ctx,
                    canvas,
                    tail_pos,
                    self.time_elapsed,
                    self.kelp_snag_warn,
                )?;
            }
        }

        // Delivery-streak heat badge — the persistent, watchable face of the streak multiplier that
        // otherwise only flashed for a frame at bank time and then decayed silently. Shows the live
        // multiplier under the pen and pulses toward an alarm color as the grace window runs down, so
        // "bank again before you drop a notch" is a visible tension. Gated to streak >= 2 (streak 1 is
        // 1.0x — nothing at stake). Kept SEPARATE from pen_worth on purpose: pen_worth is the honest
        // floor excluding streak/on-beat bonuses, and folding the streak in would spoil that read.
        if self.deliver_streak >= 2 {
            let streak_mult = 1.0 + (self.deliver_streak.saturating_sub(1) as f32) * 0.25;
            let decay01 = (self.deliver_streak_timer / DELIVER_STREAK_GRACE).clamp(0.0, 1.0);
            draw_delivery_streak(
                ctx,
                canvas,
                self.pen_pos,
                PEN_RADIUS,
                self.time_elapsed,
                streak_mult,
                decay01,
            )?;
        }

        // Cleave stakes tag — the Splitter bet made legible BEFORE the catch. While a free Splitter is
        // loose and the player has a train worth cleaving, float a live "CLEAVE ~N" figure at the split
        // point (the midpoint where the cut lands) showing what a clean on-beat cut would bank, heating
        // gold in the beat window like the splitter aura. Reuses cleave_clean_worth so the previewed
        // number can't drift from the actual payout. Only shows when there's both a free Splitter and a
        // train (≥2 links) to meaningfully halve, so it's naturally transient and never HUD clutter.
        if self.chain_count >= 2 && self.free_splitter_present {
            let (keep, banked) = self.cleave_split_point();
            // Single O(n) scan: find both split-point positions and tally the back-half
            // composition (Goldens/Magnets) for the jackpot check — avoids three separate
            // O(n) passes (two .find() + cleave_clean_worth's .fold()) that this block used
            // to issue every frame whenever a free Splitter and a live train are both present.
            let front_idx = keep.saturating_sub(1);
            let mut front: Option<Vec2> = None;
            let mut back: Option<Vec2> = None;
            let mut golden_in_slice = 0usize;
            let mut magnet_in_slice = 0usize;
            for c in &self.crabs {
                if !c.caught {
                    continue;
                }
                if let Some(ci) = c.chain_index {
                    if ci == front_idx {
                        front = Some(c.pos);
                    }
                    if ci == keep {
                        back = Some(c.pos);
                    }
                    if ci >= keep {
                        if c.is_golden() {
                            golden_in_slice += 1;
                        }
                        if c.is_magnet() {
                            magnet_in_slice += 1;
                        }
                    }
                }
            }
            let combo = self.combo_multiplier();
            let base = (banked * (banked + 1) / 2) * 3;
            let cashed_run = if self.tail_run_len >= 3 {
                self.tail_run_len
            } else {
                0
            };
            let golden_bonus = golden_in_slice * 120 * combo;
            let magnet_bonus = if magnet_in_slice > 0 {
                magnet_in_slice * banked.max(1) * 6 * combo
            } else {
                0
            };
            let run_bonus = (cashed_run as usize) * (cashed_run as usize) * 5 * combo;
            let crossover = golden_bonus + magnet_bonus + run_bonus;
            let worth =
                (base as f32 * combo as f32 * self.beat_gamble_mult).round() as usize + crossover;
            let jackpot = crossover > 0;
            if let Some(split_pt) = match (front, back) {
                (Some(f), Some(b)) => Some((f + b) * 0.5),
                (Some(f), None) => Some(f),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            } {
                if worth > 0 {
                    // Same beat-proximity curve the splitter aura uses, so the tag and the aura go hot
                    // together in the clean-cut window.
                    let to_beat = self.beat_timer.min(self.beat_interval - self.beat_timer);
                    let beat_prox = (1.0 - to_beat / (BEAT_WINDOW * 1.5)).clamp(0.0, 1.0);
                    draw_cleave_stakes(
                        ctx,
                        canvas,
                        split_pt,
                        worth,
                        jackpot,
                        beat_prox,
                        self.time_elapsed,
                    )?;
                }
            }
        }

        // Tail-run badge — the persistent readout of the same-type match run at the tail. Shows
        // "RUN xN" plus a 4-pip meter over the tail link so the player can *set up* the every-4th
        // Match-Run Milestone instead of only seeing the count flash for a frame at catch time.
        // Only shown for a run worth committing to (>=2) — a lone link isn't a run yet.
        if self.tail_run_len >= 2 {
            let tail_idx = self.chain_count.saturating_sub(1);
            if let Some(tail) = self
                .crabs
                .iter()
                .find(|c| c.caught && c.chain_index == Some(tail_idx))
            {
                let to_beat = self.beat_timer.min(self.beat_interval - self.beat_timer);
                let beat_prox = (1.0 - to_beat / (BEAT_WINDOW * 1.5)).clamp(0.0, 1.0);
                draw_tail_run_badge(
                    ctx,
                    canvas,
                    tail.pos,
                    self.tail_run_len,
                    tail.crab_color(),
                    beat_prox,
                    self.time_elapsed,
                )?;
            }
        }

        // Just-banked crabs marching into the pen — drawn over the pen ground so the parade files
        // in on top of the corral. Empty and free when no bank just happened.
        draw_penned_marchers(ctx, canvas, &self.penned_marchers, self.time_elapsed)?;

        // Draw beat ghost rings under the rope and crabs
        draw_chain_rings(ctx, canvas, &self.chain_rings)?;
        // Collect chain crab (chain_index, pos) pairs sorted by chain index into a persisted
        // scratch buffer instead of a fresh Vec<&EnemyCrab> every frame (see CHAIN_SORT_BUF).
        CHAIN_SORT_BUF.with(|buf| -> GameResult {
            let mut chain_links = buf.borrow_mut();
            chain_links.clear();
            // First collect (index, pos, type, color) so we can, after sorting by index, tag each
            // link with a same-type bond color relative to the link ahead of it. The type is dropped
            // once the bond is computed — only (index, pos, bond_color) travels on to the rope draw.
            //
            // Optimization: the sorted order and bond colors are stable as long as chain_count
            // doesn't change — only catches/releases mutate the chain structure. On the common case
            // (no catch/release this frame) we skip the O(n log n) sort and O(n) bond-color scan
            // and instead do a single O(n) pass to read current crab positions by stored index.
            CHAIN_ORDER_CACHE.with(|ocache| {
                let mut order_cache = ocache.borrow_mut();
                let chain_count = self.chain_count;
                let needs_rebuild = order_cache
                    .as_ref()
                    .map_or(true, |(cc, _)| *cc != chain_count);
                if needs_rebuild {
                    // (Re)build the sorted order and bond colors — only on catch/release events.
                    CHAIN_TYPE_BUF.with(|tbuf| {
                        let mut typed = tbuf.borrow_mut();
                        typed.clear();
                        typed.extend(
                            self.crabs
                                .iter()
                                .enumerate()
                                .filter(|(_, c)| c.caught && c.chain_index.is_some())
                                .map(|(i, c)| {
                                    (c.chain_index.unwrap_or(0), i, c.crab_type, c.crab_color())
                                }),
                        );
                        typed.sort_unstable_by_key(|&(idx, ..)| idx);
                        let mut prev_type: Option<CrabType> = None;
                        // Reuse the Vec already stored in order_cache (if any) to avoid a heap
                        // allocation on every catch/release event. Taking the stored Vec out,
                        // clearing it, and pushing into it preserves its capacity across rebuilds
                        // instead of dropping it and collecting a fresh one each time.
                        let mut sorted = order_cache.take().map(|(_, v)| v).unwrap_or_default();
                        sorted.clear();
                        sorted.extend(typed.iter().enumerate().map(|(pos, &(_, ci, ty, col))| {
                            // Same-type adjacency bond (unchanged): the link inherits the shared
                            // type color so its rope segment glows.
                            let mut bond = if prev_type == Some(ty) {
                                Some(col)
                            } else {
                                None
                            };
                            // Sandwich highlight: if THIS crab is flanked in the sorted chain by two
                            // of the same figurehead archetype (Golden/Dancer), light its rope
                            // segment with the flanking figurehead's color so the arrangement reads
                            // live on the train, not only as a bank callout. `typed` is sorted by
                            // chain_index, so pos-1 / pos+1 are the true chain neighbors.
                            if pos > 0 && pos + 1 < typed.len() {
                                let (_, _, lty, lcol) = typed[pos - 1];
                                let (_, _, rty, _) = typed[pos + 1];
                                if lty == rty && matches!(lty, CrabType::Golden | CrabType::Dancer)
                                {
                                    bond = Some(lcol);
                                }
                            }
                            prev_type = Some(ty);
                            (ci, bond)
                        }));
                        *order_cache = Some((chain_count, sorted));
                    });
                }
                // Fast path: read current positions from self.crabs using the cached order.
                if let Some((_, ref order)) = *order_cache {
                    for &(crabs_idx, bond) in order {
                        let crab = &self.crabs[crabs_idx];
                        chain_links.push((crab.chain_index.unwrap_or(0), crab.pos, bond));
                    }
                }
            });
            // Only the at-risk gain (live multiplier above the banked-safe floor) heats the rope,
            // so cashing out with B visibly cools it — the risk you're carrying reads on the train.
            let gamble_heat =
                ((self.beat_gamble_mult - self.beat_gamble_locked) / 2.0).clamp(0.0, 1.0);
            // Phase across the current bar (0 at the downbeat, →1 across four beats): drives the
            // pulse of light that sweeps down the rope once per bar so the train "feels the beat".
            let within_beat = 1.0 - (self.beat_timer / self.beat_interval).clamp(0.0, 1.0);
            let bar_phase = ((self.beat_count % 4) as f32 + within_beat) / 4.0;
            draw_conga_rope(
                ctx,
                canvas,
                self.player_pos,
                &chain_links,
                self.time_elapsed,
                self.beat_intensity,
                gamble_heat,
                bar_phase,
            )
        })?;

        // On-beat catch-bloom ring around the head: shows the scoop window breathing with the bar
        // (widest on the downbeat) so timing a plain grab to the beat becomes a legible, watchable
        // read. Only meaningful once there's a train to catch onto, so gate on chain_count (the
        // cached caught-crab count) instead of scanning the whole herd every draw frame.
        if self.chain_count > 0 {
            let head = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            let catch_radius = self.catch_radius();
            draw_catch_bloom_ring(
                ctx,
                canvas,
                head,
                catch_radius,
                self.beat_catch_bloom,
                self.beat_intensity,
            )?;
        }

        // Calculate flashlight direction from player to mouse.
        if self.flashlight.on {
            let flashlight_dir = (self.mouse_pos - self.player_pos).normalize_or_zero();
            draw_flashlight(
                ctx,
                canvas,
                self.player_pos,
                flashlight_dir,
                self.time_since_catch,
                &self.flashlight,
                &self.flashlight_shader,
                self.width,
                self.height,
                self.camera_origin,
            )?;
        }

        // Draw all crabs.
        self.draw_crabs_with_shake(ctx, canvas)?;

        // Draw player character after crabs so the rustler always renders on top of the conga
        // train rather than being occluded by crabs that overlap its position.
        // Jam emote (B key): shimmy the player position side-to-side for a fun wiggle.
        let jam_shimmy = if self.jam_timer > 0.0 {
            let phase = (self.jam_timer / 0.55) * std::f32::consts::TAU * 4.0;
            Vec2::new(
                phase.sin() * 6.0 * self.jam_timer / 0.55,
                (phase * 0.7).cos() * 3.0,
            )
        } else {
            Vec2::ZERO
        };
        draw_rustler(
            ctx,
            canvas,
            self.player_pos + jam_shimmy,
            &self.textures.player,
            self.player_vel,
            self.beat_intensity,
            self.time_elapsed,
            self.boost_timer > 0.0,
            self.player_skin,
        )?;

        let player_name = crate::normalize_player_name(&self.player_name);
        let player_name_w = PLAYER_NAME_CACHE.with(|c| -> GameResult<f32> {
            let mut cache = c.borrow_mut();
            let needs_rebuild = cache
                .as_ref()
                .map_or(true, |(name, _, _)| name != &player_name);
            if needs_rebuild {
                let mut text = Text::new(player_name.as_str());
                text.set_scale(16.0);
                let w = text.measure(ctx)?.x;
                *cache = Some((player_name.clone(), text, w));
            }
            Ok(cache.as_ref().unwrap().2)
        })?;
        PLAYER_NAME_CACHE.with(|c| {
            let cache = c.borrow();
            if let Some((_, text, _)) = cache.as_ref() {
                let player_center =
                    self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
                let name_pos = player_center - Vec2::new(player_name_w / 2.0, 42.0);
                canvas.draw(
                    text,
                    DrawParam::default()
                        .dest(name_pos + Vec2::splat(1.5))
                        .color(Color::from_rgba(0, 0, 0, 180)),
                );
                canvas.draw(
                    text,
                    DrawParam::default()
                        .dest(name_pos)
                        .color(Color::new(0.96, 0.82, 0.3, 0.95)),
                );
            }
        });

        let sprinting = (ctx.keyboard.is_key_pressed(KeyCode::LShift)
            || ctx.keyboard.is_key_pressed(KeyCode::RShift))
            && self.sprint_stamina > 0.0
            && self.boost_timer <= 0.0;

        // Sprint whoosh: a longer green wake behind the crab while Shift is held, so the extra
        // speed reads as motion instead of just a number change.
        if sprinting && self.last_dir.length() > 0.01 {
            let center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            let intensity = (self.sprint_stamina / SPRINT_STAMINA_MAX).clamp(0.25, 1.0);
            draw_sprint_whoosh(
                ctx,
                canvas,
                center,
                self.last_dir,
                self.time_elapsed,
                intensity,
            )?;
        }

        // Speed lines trailing behind player while dashing. Uses the cached unit-line mesh
        // (see draw_speed_lines) instead of building up to 7 fresh Mesh::new_line GPU buffers
        // every single frame of the dash window.
        if self.boost_timer > 0.0 && self.last_dir.length() > 0.01 {
            let center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            let intensity = self.boost_timer / 0.18;
            draw_speed_lines(ctx, canvas, center, self.last_dir, intensity)?;
        }

        // (Radar arrows are screen-edge indicators — drawn in the HUD pass below, after the switch
        // to screen space, so they pin to the viewport border rather than scrolling with the world.)

        // Point the player at the delivery pen while there's a train to cash in. The pen jumps on
        // every bank, so this keeps its "route the train here" decision legible instead of a hunt.
        // Urgency scales with train size (normalized against a fat-haul cap of 12) so a big, at-risk
        // conga line pulls harder toward the pen than a couple of crabs.
        if self.chain_count > 0 {
            let urgency = (self.chain_count as f32 / 12.0).min(1.0);
            draw_pen_guide(
                ctx,
                canvas,
                self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0),
                self.pen_pos,
                PEN_RADIUS,
                width,
                height,
                self.camera_origin,
                urgency,
                self.beat_intensity,
                self.time_elapsed,
            )?;
        }

        // Draw the whip-streaks that yank caught crabs into the train (under the impact rings).
        draw_catch_trails(ctx, canvas, &self.catch_trails)?;

        // Draw catch impact shockwaves (over the crabs, under score text)
        draw_catch_shockwaves(ctx, canvas, &self.catch_shockwaves)?;

        // Beat-hit punch flashes — additive impact + resonance ring at each on-beat catch position
        for &(pos, color, quality) in &self.beat_punch_events {
            draw_beat_hit_punch(ctx, canvas, pos, color, quality)?;
        }

        // Bond-forming flash arcs: bright connecting flash between newly-bonded same-type neighbors.
        // Draw the arc using a scaled unit_line between the two positions.
        {
            let unit_ln = unit_line(ctx)?;
            for &(from, to, color, age) in &self.bond_flash_events {
                let diff = to - from;
                let len = diff.length();
                if len < 1.0 {
                    continue;
                }
                let angle = diff.y.atan2(diff.x);
                let alpha = age * 0.85; // fades from 0.85 to 0 as age goes 1→0
                let thickness = 3.5 * age;
                // Main arc line
                canvas.set_blend_mode(BlendMode::ADD);
                canvas.draw(
                    unit_ln,
                    DrawParam::default()
                        .dest(from)
                        .scale(Vec2::new(len, thickness))
                        .rotation(angle)
                        .color(Color::new(color[0], color[1], color[2], alpha)),
                );
                // Bright center spine
                canvas.draw(
                    unit_ln,
                    DrawParam::default()
                        .dest(from)
                        .scale(Vec2::new(len, thickness * 0.4))
                        .rotation(angle)
                        .color(Color::new(
                            (color[0] * 0.5 + 0.5).min(1.0),
                            (color[1] * 0.5 + 0.5).min(1.0),
                            (color[2] * 0.5 + 0.5).min(1.0),
                            alpha * 0.9,
                        )),
                );
                // End-caps
                let dot = unit_circle(ctx)?;
                canvas.draw(
                    dot,
                    DrawParam::default()
                        .dest(from)
                        .scale(Vec2::splat(thickness * 1.8))
                        .color(Color::new(color[0], color[1], color[2], alpha * 0.8)),
                );
                canvas.draw(
                    dot,
                    DrawParam::default()
                        .dest(to)
                        .scale(Vec2::splat(thickness * 1.8))
                        .color(Color::new(color[0], color[1], color[2], alpha * 0.8)),
                );
                canvas.set_blend_mode(BlendMode::ALPHA);
            }
        }

        // Draw stampede fear rings where catches startled the herd
        draw_fear_rings(ctx, canvas, &self.fear_rings)?;

        // Draw Tide Boss shockwave pulses sweeping outward
        draw_tide_pulses(ctx, canvas, &self.tide_pulses, TIDE_PULSE_RADIUS)?;

        // Draw King Crab stolen crabs — magnetically flying toward the boss.
        // Reuses unit_circle scaled via DrawParam instead of building two Mesh::new_circle GPU
        // buffers per stolen crab per frame (the previous approach).
        {
            let dot = unit_circle(ctx)?;
            for (pos, timer, color) in &self.king_stolen_crabs {
                let t = (*timer / 0.9_f32).clamp(0.0, 1.0);
                let alpha = t;
                let size = CRAB_SIZE * (0.6 + 0.4 * t);
                let draw_pos = *pos - self.camera_origin;
                let r = color[0] * 0.6 + 0.6 * (1.0 - t);
                let g = color[1] * t * 0.5;
                let gb = color[2] * t * 0.8 + 0.5 * (1.0 - t);
                // Fill disc
                canvas.draw(
                    dot,
                    DrawParam::default()
                        .dest(draw_pos)
                        .scale(Vec2::splat(size * 0.5))
                        .color(Color::new(r, g, gb, alpha)),
                );
                // Outer magenta ring — use a slightly larger fill at lower alpha to fake a stroke ring
                canvas.draw(
                    dot,
                    DrawParam::default()
                        .dest(draw_pos)
                        .scale(Vec2::splat(size * 0.7))
                        .color(Color::new(1.0, 0.3, 0.9, alpha * 0.35)),
                );
            }
        }

        // Draw particle effects
        draw_particles(ctx, canvas, &self.particle_system)?;
        draw_floating_texts(ctx, canvas, &self.floating_texts)?;

        // Draw combo meter around player
        draw_combo_meter(
            ctx,
            canvas,
            self.player_pos,
            PLAYER_SIZE,
            self.combo_count,
            self.combo_timer,
            self.beat_intensity,
            self.time_elapsed,
        )?;

        // Draw beat wave circle outline. Uses cached_stroke_circle (via draw_beat_wave_ring)
        // instead of building a fresh Mesh::new_circle GPU buffer every frame the wave expands.
        if self.beat_wave_active && self.beat_wave_radius > 0.0 {
            let player_center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            draw_beat_wave_ring(ctx, canvas, player_center, self.beat_wave_radius)?;
        }

        // Groove Dash gather ring — a ring that contracts toward the dash's landing point over the
        // gather window, reading as the herd being hoovered into your slipstream. Drawn at the point
        // ahead of the dash (origin + heading*reach) so the tell lines up with where crabs funnel.
        if self.groove_dash_timer > 0.0 && self.groove_dash_dir.length() > 0.01 {
            let reach = 170.0;
            let t = (self.groove_dash_timer / 0.22).clamp(0.0, 1.0); // 1 → 0 over the window
            let ring_r = 30.0 + reach * t; // contracts inward as the wake finishes
            let target = self.groove_dash_center + self.groove_dash_dir * reach;
            draw_beat_wave_ring(ctx, canvas, target, ring_r)?;
        }

        // Draw the whistle sonic pulse
        if self.whistle_active > 0.0 && self.whistle_radius > 0.0 {
            draw_whistle_ring(
                ctx,
                canvas,
                self.whistle_center,
                self.whistle_radius,
                self.whistle_max_radius() * self.whistle_beat_bonus,
            )?;
        }

        // Draw the stomp ground-pound shockwave
        if self.stomp_active > 0.0 && self.stomp_radius > 0.0 {
            draw_stomp_ring(
                ctx,
                canvas,
                self.stomp_center,
                self.stomp_radius,
                self.stomp_max_radius() * self.stomp_beat_bonus,
            )?;
        }

        // Strong-match archetype-tool visual feedback.
        if !self.beam_hermit_hits_buf.is_empty() {
            draw_beam_hermit_match(ctx, canvas, &self.beam_hermit_hits_buf)?;
        }
        if !self.stomp_dancer_hits_buf.is_empty() {
            draw_stomp_dancer_match(ctx, canvas, &self.stomp_dancer_hits_buf)?;
        }
        if !self.lasso_thief_hits_buf.is_empty() {
            draw_lasso_thief_match(ctx, canvas, &self.lasso_thief_hits_buf)?;
        }
        if !self.lasso_magnet_hits_buf.is_empty() {
            draw_lasso_magnet_match(ctx, canvas, &self.lasso_magnet_hits_buf)?;
        }
        if !self.lasso_shell_deflect_hits_buf.is_empty() {
            draw_lasso_shell_deflect(ctx, canvas, &self.lasso_shell_deflect_hits_buf)?;
        }
        if !self.magnet_cluster_hits_buf.is_empty() {
            draw_magnet_cluster_pull(ctx, canvas, &self.magnet_cluster_hits_buf)?;
        }
        if !self.stomp_armored_hits_buf.is_empty() {
            draw_stomp_armored_crack(ctx, canvas, &self.stomp_armored_hits_buf)?;
        }
        if !self.whistle_golden_hits_buf.is_empty() {
            draw_whistle_golden_pull(ctx, canvas, &self.whistle_golden_hits_buf)?;
        }
        if !self.whistle_dancer_hits_buf.is_empty() {
            draw_whistle_dancer_match(ctx, canvas, &self.whistle_dancer_hits_buf)?;
        }

        // Draw the rhythm Call summon pulse — magenta rings collapsing toward the player.
        if self.call_pulse > 0.0 {
            draw_call_ring(ctx, canvas, self.call_pulse_center, self.call_pulse, 420.0)?;
        }

        // Groove-Call answer streaks — comet trails from the answering herd toward the player, thrown
        // on each beat, so the field-wide lunge reads in a single frame. Drawn before the ring so the
        // ring's broadcast wash sits on top. Reuses the catch-trail draw (additive comet streaks).
        if !self.call_streaks.is_empty() {
            draw_catch_trails(ctx, canvas, &self.call_streaks)?;
        }

        // Draw the Groove Call broadcast — cyan rings sweeping outward across the field while the
        // field-wide herd lure is answering (re-kicked each downbeat), so the arena-scale summons reads.
        if self.groove_call_pulse > 0.0 {
            // Each chained echo reaches the ring further across the field, so a longer call-and-response
            // phrase reads as the whole arena answering — the watchable payoff for staying in the pocket.
            let reach = 720.0 + 120.0 * self.groove_call_echo as f32;
            draw_groove_call_ring(
                ctx,
                canvas,
                self.groove_call_center,
                self.groove_call_pulse,
                reach,
            )?;
            // A brief bright secondary ring snaps out the instant an echo lands, so the answered beat pops.
            if self.groove_call_echo_flash > 0.0 {
                draw_groove_call_ring(
                    ctx,
                    canvas,
                    self.groove_call_center,
                    self.groove_call_echo_flash,
                    reach * 0.55,
                )?;
            }
        }

        // Draw the passive downbeat herd-pulse cue — warm rings collapsing toward the player on the
        // "1" of the bar, so the always-on rhythmic routing tug is legible without a keypress.
        if self.downbeat_pull > 0.0 {
            draw_downbeat_pulse_ring(
                ctx,
                canvas,
                self.downbeat_pull_center,
                self.downbeat_pull,
                300.0, // matches DOWNBEAT_PULL_RADIUS in update_crabs
                self.downbeat_pull_haul,
            )?;
        }

        // Draw the Downbeat Slam shockwave — the big gold rhythm-ultimate blast.
        if self.slam_active > 0.0 && self.slam_radius > 0.0 {
            draw_slam_ring(ctx, canvas, self.slam_center, self.slam_radius, SLAM_RADIUS)?;
        }

        // Cleave slash — the blade stroke bisecting the train the instant a Splitter cuts it.
        if self.cleave_flash > 0.0 {
            draw_cleave_slash(
                ctx,
                canvas,
                self.cleave_a,
                self.cleave_b,
                self.cleave_flash,
                self.cleave_gold,
            )?;
        }

        // Drum Roll telegraph: while holding T and building a charge, pulse tightening rings at the
        // player (reuses the Call-ring draw) so the roll reads as a visible wind-up before release —
        // the more hits banked, the tighter/brighter. On the fired blast the ring flashes out wide.
        if self.drum_roll_charge > 0.02 || self.drum_roll_fire > 0.0 {
            let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            if self.drum_roll_fire > 0.0 {
                draw_call_ring(ctx, canvas, center, self.drum_roll_fire, 340.0)?;
            } else {
                // Charging: a small, growing beckon-ring — pulse tracks the charge, reach grows with it.
                let reach = 60.0 + 120.0 * self.drum_roll_charge;
                draw_call_ring(ctx, canvas, center, self.drum_roll_charge.min(1.0), reach)?;
            }
        }

        // Draw lasso: winding-up OR in-flight (Throwing/Snag/Dragging/Miss).
        {
            let player_center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            match self.lasso_phase {
                LassoPhase::Winding => {
                    // Windup: spinning rope loop above/around the player, grows with charge.
                    // Pulses brighter on each beat so the player can time the release.
                    let charge_frac = (self.lasso_charge / LASSO_MAX_CHARGE_TIME).min(1.0);
                    // Beat-proximity pulse: brighter the closer to the beat edge.
                    let to_beat = self.beat_timer.min(self.beat_interval - self.beat_timer);
                    let beat_prox = (1.0 - to_beat / (BEAT_WINDOW * 1.5)).clamp(0.0, 1.0);
                    draw_lasso_windup(
                        ctx,
                        canvas,
                        player_center,
                        charge_frac,
                        beat_prox,
                        self.lasso_spin,
                    )?;
                }
                LassoPhase::Throwing
                | LassoPhase::Snag
                | LassoPhase::Dragging
                | LassoPhase::Miss => {
                    if let Some(tip) = self.lasso_pos {
                        let (dur, draw_phase) = match self.lasso_phase {
                            LassoPhase::Throwing => (LASSO_THROW_TIME, LassoDrawPhase::Throw),
                            LassoPhase::Snag => (LASSO_SNAG_TIME, LassoDrawPhase::Snag),
                            LassoPhase::Dragging => (LASSO_DRAG_TIME, LassoDrawPhase::Drag),
                            LassoPhase::Miss => (LASSO_MISS_TIME, LassoDrawPhase::Miss),
                            _ => (LASSO_THROW_TIME, LassoDrawPhase::Throw),
                        };
                        let phase_t = (1.0 - self.lasso_timer / dur).clamp(0.0, 1.0);
                        draw_lasso(
                            ctx,
                            canvas,
                            player_center,
                            tip,
                            draw_phase,
                            phase_t,
                            self.lasso_spin,
                        )?;
                    }
                }
                LassoPhase::Idle => {}
            }
        }

        // ===== SWITCH TO SCREEN SPACE FOR THE HUD =====
        // Everything above draws in world space (the camera-following rect set in draw()); the
        // camera can be scrolled far from the origin. The HUD/overlays below must be pinned to the
        // screen, so re-set the canvas coordinates to a fixed viewport rect (origin 0, plus the
        // same screen-shake offset the world got). ggez allows re-setting coordinates mid-canvas
        // between draws. Every draw after this line lands in screen space.
        canvas.set_screen_coordinates(Rect::new(
            self.screen_shake_offset.x,
            self.screen_shake_offset.y,
            width,
            height,
        ));

        // Weather screen-space pass: rain streaks, heavy-rain edge vignette, and the storm
        // lightning flash. All pinned to the viewport (drawn after the screen-coordinate switch) so
        // rain density and the flash are camera-independent — they don't smear as the world scrolls.
        // beat_intensity drives a subtle on-beat opacity pulse on the streaks.
        draw_weather(
            ctx,
            canvas,
            width,
            height,
            self.time_elapsed,
            self.weather_intensity,
            self.beat_intensity,
            self.lightning_flash,
        )?;

        // Minimap — top-right corner, showing the full scrolling world.
        // Stack-allocate the tiny NPC arrays (≤3 leaders, ≤24 followers) to avoid per-frame heap allocs.
        {
            const MINI_STEPS: usize = 14;
            let mut leader_buf = [(Vec2::ZERO, 0.0_f32); 8];
            let leader_n = self.npc_trains.len().min(8);
            for (i, t) in self.npc_trains.iter().enumerate().take(8) {
                leader_buf[i] = (t.leader_pos, t.leader_scale);
            }
            let mut follower_buf = [Vec2::ZERO; 64];
            let mut follower_n = 0usize;
            for npc in &self.npc_trains {
                for i in 0..npc.follower_types.len() {
                    if follower_n >= follower_buf.len() {
                        break;
                    }
                    if let Some(&p) = npc.path_history.get((i + 1) * MINI_STEPS) {
                        follower_buf[follower_n] = p;
                        follower_n += 1;
                    }
                }
            }
            draw_minimap(
                ctx,
                canvas,
                width,
                height,
                self.world_width,
                self.world_height,
                self.camera_origin,
                self.player_pos,
                self.pen_pos,
                &self.crabs,
                &leader_buf[..leader_n],
                &follower_buf[..follower_n],
                self.time_elapsed,
            )?;
            let map_h = 180.0_f32 * (self.world_height / self.world_width);
            draw_day_weather_hud(
                ctx,
                canvas,
                width,
                map_h,
                self.day_phase_t,
                self.weather_intensity,
                self.time_elapsed,
            )?;
        }

        // Tool roster — Zelda-style 5-slot bar at the bottom centre.
        if !self.show_instructions && !self.game_over && !self.show_world_map {
            draw_tool_roster(
                ctx,
                canvas,
                width,
                height,
                self.whistle_cooldown,
                crate::WHISTLE_COOLDOWN,
                self.stomp_cooldown,
                crate::STOMP_COOLDOWN,
                self.boost_cooldown,
                !matches!(self.lasso_phase, LassoPhase::Idle),
                self.groove,
                self.time_elapsed,
            )?;
        }

        // Screen-edge radar arrows pointing to free crabs — now in the HUD pass so they pin to the
        // viewport border; the camera origin translates each crab's world position into the viewport.
        draw_crab_radar(
            ctx,
            canvas,
            &self.crabs,
            width,
            height,
            self.camera_origin,
            self.beat_intensity,
            self.time_elapsed,
        )?;

        // Show stats. The HUD line (score/train/combo) only changes on catch/combo events, not
        // every tick, so cache the built Text and only rebuild it (fresh format! String + fresh
        // Text, which re-triggers glyph shaping) when the underlying values actually differ from
        // last frame's — same pattern as the per-level label cache above. Also use the
        // already-maintained self.chain_count instead of re-scanning every crab for `.caught`
        // every frame just to display the same number (crabs are never removed from the vec —
        // caught state only flips via chain_count-tracked catches/snaps — so the two stay in
        // sync).
        let chain_len = self.chain_count;
        let mult = if self.combo_count >= 3 {
            self.combo_multiplier()
        } else {
            0
        };
        HUD_TEXT_CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            let needs_rebuild = match &*cache {
                Some((s, cl, cc, m, _)) => {
                    *s != self.score || *cl != chain_len || *cc != self.combo_count || *m != mult
                }
                None => true,
            };
            if needs_rebuild {
                let hud = if self.combo_count >= 3 {
                    format!(
                        "Score: {}  |  Train: {}  |  Combo x{}  [{}x pts]",
                        self.score, chain_len, self.combo_count, mult
                    )
                } else {
                    format!("Score: {}  |  Train: {}", self.score, chain_len)
                };
                *cache = Some((
                    self.score,
                    chain_len,
                    self.combo_count,
                    mult,
                    Text::new(hud),
                ));
            }
            canvas.draw(
                &cache.as_ref().unwrap().4,
                DrawParam::default()
                    .dest(Vec2::new(10.0, 10.0))
                    .color(Color::from_rgb(255, 255, 00)),
            );
        });

        // Rhythm mastery readout, just under the score: the cumulative EXTRA points playing on the
        // beat has earned over a flat-1x run — "how far ahead the beat put you", the legible payoff
        // for flawless on-beat play. Display-only; it never adds score, only reveals what the
        // rhythm multipliers already banked. Hidden until it's nonzero so it doesn't clutter the
        // opening of a run before any groove has landed. Pulses gold on a fat on-beat bank.
        if self.rhythm_bonus_score > 0 {
            RHYTHM_BONUS_CACHE.with(|c| {
                let mut cache = c.borrow_mut();
                let needs_rebuild = match &*cache {
                    Some((n, _)) => *n != self.rhythm_bonus_score,
                    None => true,
                };
                if needs_rebuild {
                    let txt = format!("RHYTHM BONUS  +{}", self.rhythm_bonus_score);
                    *cache = Some((self.rhythm_bonus_score, Text::new(txt)));
                }
                let pop = self.rhythm_bonus_flash;
                // Steady teal, flashing toward bright gold on a bank jump.
                let col = Color::new(0.3 + pop * 0.7, 0.9, 0.7 - pop * 0.5, 1.0);
                canvas.draw(
                    &cache.as_ref().unwrap().1,
                    DrawParam::default()
                        .dest(Vec2::new(10.0, 30.0))
                        .scale(Vec2::splat(1.0 + pop * 0.12))
                        .color(col),
                );
            });
        }

        // Debug-only perf overlay, top-right: avg/worst frame time + fps over the last ~2s
        // window (see the accumulation block in update()). Lets a feature/optimizer agent (or
        // Carl) see the cost of whatever just landed without needing a terminal in view.
        #[cfg(debug_assertions)]
        PERF_OVERLAY_CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            // Round to hundredths (matches the displayed precision) so the cache only rebuilds
            // when the printed numbers would actually change, not every frame.
            let avg_key = (self.perf_last_avg_ms * 100.0).round() as i32;
            let worst_key = (self.perf_last_worst_ms * 100.0).round() as i32;
            let crab_key = self.crabs.len() as i32;
            let needs_rebuild = match &*cache {
                Some((a, w, c, _, _)) => *a != avg_key || *w != worst_key || *c != crab_key,
                None => true,
            };
            if needs_rebuild {
                let msg = format!(
                    "avg {:.2}ms ({:.0} fps)  worst {:.2}ms  {} crabs ({} chained)",
                    self.perf_last_avg_ms,
                    self.perf_last_fps,
                    self.perf_last_worst_ms,
                    self.crabs.len(),
                    self.chain_count,
                );
                let text = Text::new(msg);
                let width = text.measure(ctx).map(|m| m.x).unwrap_or(0.0);
                *cache = Some((avg_key, worst_key, crab_key, text, width));
            }
            let (_, _, _, text, width) = cache.as_ref().unwrap();
            canvas.draw(
                text,
                DrawParam::default()
                    .dest(Vec2::new(self.width - width - 10.0, 10.0))
                    .color(Color::from_rgb(120, 255, 120)),
            );
        });

        // Draw dash bar for boost timer/cooldown.
        let bar_x = 10.0;
        let bar_y = 50.0;
        let bar_width = 220.0;
        let bar_height = 18.0;
        let max_boost = 0.18;
        let max_cooldown = 0.08;
        let cooldown_ratio = (self.boost_cooldown / max_cooldown).clamp(0.0, 1.0);

        // Draw background bar
        canvas.draw(
            unit_square(ctx)?,
            DrawParam::default()
                .dest(Vec2::new(bar_x, bar_y))
                .scale(Vec2::new(bar_width, bar_height))
                .color(Color::from_rgb(40, 40, 40)),
        );

        // Draw boost timer (yellow)
        let ratio = ((max_boost - self.boost_timer) / max_boost).clamp(0.0, 1.0);
        if ratio > 0.0 {
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .dest(Vec2::new(bar_x, bar_y))
                    .scale(Vec2::new(bar_width * ratio, bar_height))
                    .color(Color::from_rgb(255, 220, 40)),
            );
        }

        // Draw cooldown (red, overlays boost)
        if cooldown_ratio > 0.0 {
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .dest(Vec2::new(bar_x, bar_y))
                    .scale(Vec2::new(bar_width * cooldown_ratio, bar_height))
                    .color(Color::from_rgb(220, 60, 60)),
            );
        }

        // Draw stamina bar border
        let border = cached_stroke_rect(ctx, bar_width, bar_height, 2.0)?;
        canvas.draw(
            &border,
            DrawParam::default()
                .dest(Vec2::new(bar_x, bar_y))
                .color(Color::from_rgb(255, 255, 255)),
        );

        // Draw label (static text — build once and reuse forever, same pattern as the HUD/level
        // label caches above).
        DASH_LABEL_CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                *cache = Some(Text::new("Dash (Space)"));
            }
            canvas.draw(
                cache.as_ref().unwrap(),
                DrawParam::default()
                    .dest(Vec2::new(bar_x, bar_y - 22.0))
                    .color(Color::from_rgb(255, 255, 255)),
            );
        });

        // Draw sprint stamina bar for held Shift sprinting.
        let sprint_y = bar_y + bar_height + 12.0;
        let sprint_height = 12.0;
        let sprint_ratio = (self.sprint_stamina / SPRINT_STAMINA_MAX).clamp(0.0, 1.0);
        canvas.draw(
            unit_square(ctx)?,
            DrawParam::default()
                .dest(Vec2::new(bar_x, sprint_y))
                .scale(Vec2::new(bar_width, sprint_height))
                .color(Color::from_rgb(32, 44, 40)),
        );
        if sprint_ratio > 0.0 {
            let sprint_color = Color::from_rgb(70, 220, 150);
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .dest(Vec2::new(bar_x, sprint_y))
                    .scale(Vec2::new(bar_width * sprint_ratio, sprint_height))
                    .color(sprint_color),
            );
        }
        let sprint_border = cached_stroke_rect(ctx, bar_width, sprint_height, 2.0)?;
        canvas.draw(
            &sprint_border,
            DrawParam::default()
                .dest(Vec2::new(bar_x, sprint_y))
                .color(Color::from_rgb(220, 255, 240)),
        );
        SPRINT_LABEL_CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                *cache = Some(Text::new("Sprint (Shift)"));
            }
            canvas.draw(
                cache.as_ref().unwrap(),
                DrawParam::default()
                    .dest(Vec2::new(bar_x, sprint_y - 20.0))
                    .color(Color::from_rgb(220, 255, 240)),
            );
        });

        // Whistle cooldown bar (E) — fills back up to amber as it recharges, ready when full.
        let wbar_y = sprint_y + sprint_height + 18.0;
        let wbar_h = 12.0;
        let ready = self.whistle_cooldown <= 0.0;
        let charge = (1.0 - self.whistle_cooldown / self.whistle_cooldown_dur()).clamp(0.0, 1.0);
        canvas.draw(
            unit_square(ctx)?,
            DrawParam::default()
                .dest(Vec2::new(bar_x, wbar_y))
                .scale(Vec2::new(bar_width, wbar_h))
                .color(Color::from_rgb(40, 40, 40)),
        );
        let (wr, wg, wb) = if ready {
            (255, 210, 90)
        } else {
            (150, 110, 40)
        };
        canvas.draw(
            unit_square(ctx)?,
            DrawParam::default()
                .dest(Vec2::new(bar_x, wbar_y))
                .scale(Vec2::new(bar_width * charge, wbar_h))
                .color(Color::from_rgb(wr, wg, wb)),
        );
        let wborder = cached_stroke_rect(ctx, bar_width, wbar_h, 2.0)?;
        canvas.draw(
            &wborder,
            DrawParam::default()
                .dest(Vec2::new(bar_x, wbar_y))
                .color(Color::from_rgb(255, 255, 255)),
        );
        WHISTLE_LABEL_CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            let needs_rebuild = !matches!(&*cache, Some((r, _)) if *r == ready);
            if needs_rebuild {
                let text = Text::new(if ready {
                    "Whistle (E) READY"
                } else {
                    "Whistle (E)"
                });
                *cache = Some((ready, text));
            }
            canvas.draw(
                &cache.as_ref().unwrap().1,
                DrawParam::default()
                    .dest(Vec2::new(bar_x + bar_width + 8.0, wbar_y - 2.0))
                    .color(Color::from_rgb(255, 230, 150)),
            );
        });

        // Stomp cooldown bar (R) — steely blue, refills as the ground-pound recharges.
        let sbar_y = wbar_y + wbar_h + 20.0;
        let sbar_h = 12.0;
        let sready = self.stomp_cooldown <= 0.0;
        let scharge = (1.0 - self.stomp_cooldown / self.stomp_cooldown_dur()).clamp(0.0, 1.0);
        canvas.draw(
            unit_square(ctx)?,
            DrawParam::default()
                .dest(Vec2::new(bar_x, sbar_y))
                .scale(Vec2::new(bar_width, sbar_h))
                .color(Color::from_rgb(40, 40, 40)),
        );
        let (sr, sg, sb) = if sready {
            (150, 190, 235)
        } else {
            (80, 105, 135)
        };
        canvas.draw(
            unit_square(ctx)?,
            DrawParam::default()
                .dest(Vec2::new(bar_x, sbar_y))
                .scale(Vec2::new(bar_width * scharge, sbar_h))
                .color(Color::from_rgb(sr, sg, sb)),
        );
        let sborder = cached_stroke_rect(ctx, bar_width, sbar_h, 2.0)?;
        canvas.draw(
            &sborder,
            DrawParam::default()
                .dest(Vec2::new(bar_x, sbar_y))
                .color(Color::from_rgb(255, 255, 255)),
        );
        STOMP_LABEL_CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            let needs_rebuild = !matches!(&*cache, Some((r, _)) if *r == sready);
            if needs_rebuild {
                let text = Text::new(if sready {
                    "Stomp (R) READY"
                } else {
                    "Stomp (R)"
                });
                *cache = Some((sready, text));
            }
            canvas.draw(
                &cache.as_ref().unwrap().1,
                DrawParam::default()
                    .dest(Vec2::new(bar_x + bar_width + 8.0, sbar_y - 2.0))
                    .color(Color::from_rgb(190, 215, 245)),
            );
        });

        // Show current level at the bottom center. Text/layout is cached per level index (see
        // LEVEL_LABEL_CACHE) since it's static for the whole level but this branch runs every
        // frame — only the very first frame after a level change pays for building/measuring it.
        if self.level_title_timer == 0.0 {
            LEVEL_LABEL_CACHE.with(|c| -> GameResult {
                let mut cache = c.borrow_mut();
                if !cache.contains_key(&self.current_level) {
                    let mut label = Text::new(format!(
                        "Level {}: {}\n{} | Difficulty: {}",
                        self.current_level + 1,
                        self.levels[self.current_level].title,
                        self.levels[self.current_level].description,
                        self.levels[self.current_level].difficulty
                    ));
                    label.set_scale(18.0);
                    let dims = label.measure(ctx)?;
                    cache.insert(self.current_level, (label, dims.x, dims.y));
                }
                let (label, label_width, label_height) = cache.get(&self.current_level).unwrap();
                canvas.draw(
                    label,
                    DrawParam::default()
                        .dest(Vec2::new(
                            (width - label_width) / 2.0,
                            height - label_height - 18.0,
                        ))
                        .color(Color::from_rgba(220, 220, 220, 120)), // subtle, monochrome, semi-transparent
                );
                Ok(())
            })?;
        }

        // Draw level title if timer is active.
        if self.level_title_timer > 0.0 {
            self.draw_level_title(ctx, canvas, width, height)?;
        }

        // Frenzy banner — the staged difficulty spike's on-screen shout. Rides in high so it
        // doesn't collide with the centered level title, fades with its timer, and pulses gold.
        if self.frenzy_banner_timer > 0.0 {
            self.draw_frenzy_banner(ctx, canvas, width, height)?;
        }

        // Stage-up banner — the smooth ramp's on-screen shout when the run climbs into a new
        // intensity stage. Sits a touch lower than the gold Frenzy banner so the two never overlap.
        if self.stage_banner_timer > 0.0 {
            self.draw_stage_banner(ctx, canvas, width, height)?;
        }

        // Tutorial overlay — the "How to Play" instruction card and pass-progress readout, plus the
        // big "PASSED!" celebration once the pass predicate trips. Only present in a tutorial session.
        if self.tutorial.is_some() {
            self.draw_tutorial_overlay(ctx, canvas, width, height)?;
        }

        if self.debug_mode {
            let level = &self.levels[self.current_level];
            let pat = &level.patterns[self.current_pattern];
            let pattern_name = match &pat.pattern {
                SpawnPattern::UniformRandom => "UniformRandom",
                SpawnPattern::SineWave => "SineWave",
                SpawnPattern::Circle => "Circle",
                SpawnPattern::Cluster => "Cluster",
                SpawnPattern::SingleRandom => "SingleRandom",
                SpawnPattern::BeatGrid => "BeatGrid",
                SpawnPattern::Spiral => "Spiral",
            };
            let timer_key = (self.pattern_timer * 100.0).round() as i32;
            DEBUG_TEXT_CACHE.with(|c| {
                let mut cache = c.borrow_mut();
                let needs_rebuild = match &*cache {
                    Some((p, t, _)) => *p != pattern_name || *t != timer_key,
                    None => true,
                };
                if needs_rebuild {
                    let text = Text::new(format!(
                        "[DEBUG] Pattern: {} | Time left: {:.2}s",
                        pattern_name, self.pattern_timer
                    ));
                    *cache = Some((pattern_name, timer_key, text));
                }
                canvas.draw(
                    &cache.as_ref().unwrap().2,
                    DrawParam::default()
                        .dest(Vec2::new(10.0, 80.0))
                        .color(Color::from_rgb(255, 100, 100)),
                );
            });
        }
        // Groove vignette — frame the whole screen in a beat-pulsing edge glow while the player is
        // in the pocket, so "in the groove" reads peripherally, not just from the corner meter.
        // Drawn over the world but under the HUD so it never obscures numbers/readouts.
        // Streak heat: map the on-beat catch streak onto 0..1 so the vignette catches fire as the
        // run climbs the HEATING UP (3) -> ON FIRE (5) -> BLAZING (8) -> INFERNO (12+) tiers. Below
        // the first callout tier there's no heat, so ordinary play stays cool; INFERNO maxes it.
        let streak_heat = if self.beat_streak >= 3 {
            ((self.beat_streak as f32 - 3.0) / 9.0).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let beat_phase = self.beat_timer / self.beat_interval;
        draw_groove_vignette(
            ctx,
            canvas,
            width,
            height,
            self.groove,
            self.beat_intensity,
            streak_heat,
            beat_phase,
        )?;

        // Beat indicator (top right)
        let beat_center = Vec2::new(width - 50.0, 50.0);
        // Wave-incoming telegraph: while a spawn is armed, ring the beat indicator so the player
        // sees the next herd will land on the coming downbeat. Anticipation climbs across the
        // couple of beats before the drop; the ring throbs with the beat phase.
        if self.wave_armed {
            let anticipation = (self.wave_telegraph / (self.beat_interval * 4.0)).min(1.0);
            let beat_phase = 1.0 - (self.beat_timer / self.beat_interval).clamp(0.0, 1.0);
            draw_wave_telegraph(
                ctx,
                canvas,
                beat_center,
                anticipation,
                beat_phase,
                self.frenzy_wave,
            )?;
        }
        // beat_timer counts down from beat_interval to 0, so progress toward the next beat is
        // 1 - (timer / interval). Feeds the approach ring so the player can anticipate the downbeat.
        let beat_progress = 1.0 - (self.beat_timer / self.beat_interval).clamp(0.0, 1.0);
        draw_beat_indicator(
            ctx,
            canvas,
            beat_center,
            self.beat_intensity,
            beat_progress,
            self.on_beat_now(),
            self.time_elapsed,
        )?;

        // Reef DJ call-and-response phrase — the four beats it called for this bar, drawn just under
        // the beat indicator so it sits with the other rhythm HUD. Only shown during a Reef DJ fight;
        // the player reads which pips are hot and echoes them back with the light on the beat.
        if self.reef_active {
            draw_reef_phrase(
                ctx,
                canvas,
                Vec2::new(width - 50.0, 96.0),
                self.reef_phrase,
                (self.beat_count % 4) as usize,
                self.on_beat_now(),
                self.reef_hit_flash,
            )?;
        }

        // Groove meter (top center) — fills as you catch crabs on the beat, glowing and
        // pulsing to the beat once you're in the pocket. Rewards rhythmic play at a glance.
        if self.groove > 0.01 {
            let gw = 260.0;
            let gh = 14.0;
            let gx = (width - gw) / 2.0;
            let gy = 16.0;
            let maxed = self.groove >= 0.999;
            // The topping-out flash rides on top of the steady maxed pulse, so the bar visibly pops
            // the instant it fills, then settles into its normal in-pocket glow.
            let pulse = if maxed {
                self.beat_intensity * 0.5 + self.groove_full_flash * 0.8
            } else {
                0.0
            };
            // Background track
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .dest(Vec2::new(gx, gy))
                    .scale(Vec2::new(gw, gh))
                    .color(Color::from_rgba(20, 24, 30, 200)),
            );
            // Fill — cyan when building, shifting to hot magenta/gold as it tops out.
            let t = self.groove;
            let r = 0.25 + t * 0.75;
            let g = 0.95 - t * 0.35;
            let b = 0.85 - t * 0.35;
            let bright = 1.0 + pulse;
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .dest(Vec2::new(gx, gy))
                    .scale(Vec2::new(gw * t, gh))
                    .color(Color::new(
                        (r * bright).min(1.0),
                        (g * bright).min(1.0),
                        (b * bright).min(1.0),
                        1.0,
                    )),
            );
            // Border
            let gborder = cached_stroke_rect(ctx, gw, gh, 2.0)?;
            canvas.draw(
                &gborder,
                DrawParam::default()
                    .dest(Vec2::new(gx, gy))
                    .color(Color::from_rgba(
                        255,
                        255,
                        255,
                        if maxed { 255 } else { 160 },
                    )),
            );
            // Label — text/width only change when `maxed` flips, so cache both instead of
            // rebuilding and re-measuring a Text every frame the bar is on screen.
            let lcol = if maxed {
                Color::from_rgb(255, 240, 120)
            } else {
                Color::from_rgba(200, 230, 240, 200)
            };
            GROOVE_LABEL_CACHE.with(|c| -> GameResult {
                let mut cache = c.borrow_mut();
                let needs_rebuild = !matches!(&*cache, Some((m, _, _)) if *m == maxed);
                if needs_rebuild {
                    let mut glabel = Text::new(if maxed {
                        "IN THE GROOVE! — [G] SLAM on beat"
                    } else {
                        "GROOVE"
                    });
                    glabel.set_scale(16.0);
                    let glw = glabel.measure(ctx)?.x;
                    *cache = Some((maxed, glabel, glw));
                }
                let (_, glabel, glw) = cache.as_ref().unwrap();
                canvas.draw(
                    glabel,
                    DrawParam::default()
                        .dest(Vec2::new((width - glw) / 2.0, gy + gh + 3.0))
                        .color(lcol),
                );
                Ok(())
            })?;
        }

        // Groove Gamble multiplier badge — while a hot on-beat streak is live, show the compounding
        // multiplier below the groove meter, glowing hotter the higher it climbs, so the player can
        // see at a glance exactly how much heat is riding on their next catch.
        if self.beat_gamble_mult > 1.01 {
            let t = ((self.beat_gamble_mult - 1.0) / 4.0).clamp(0.0, 1.0); // 0 at 1x, 1 at 5x cap
            // Cyan-green when warming, to gold, to hot red at the cap — matches the callout tiers.
            let (r, g, b) = (0.4 + t * 0.6, 1.0 - t * 0.7, 0.6 - t * 0.5);
            let pop = 1.0 + self.beat_gamble_flash * 0.6 + self.beat_intensity * 0.2;
            // Text/width only change when the multiplier steps (every +0.25) — cache both and
            // apply the per-frame "pop" pulse as a DrawParam scale (cheap) instead of baking it
            // into the font size (forces a re-measure every frame).
            // Cache key folds in both the live multiplier and the locked floor, since the badge text
            // now shows the safe floor too — a bank changes the label without changing the live mult.
            let key = (self.beat_gamble_mult * 100.0).round() as u32
                + ((self.beat_gamble_locked * 100.0).round() as u32) * 1000;
            GAMBLE_BADGE_CACHE.with(|c| -> GameResult {
                let mut cache = c.borrow_mut();
                let needs_rebuild = !matches!(&*cache, Some((k, _, _)) if *k == key);
                if needs_rebuild {
                    // Show the banked floor alongside the live heat when the player has cashed some in.
                    let txt = if self.beat_gamble_locked > 1.01 {
                        format!(
                            "GROOVE GAMBLE  x{:.2}  (x{:.2} safe)",
                            self.beat_gamble_mult, self.beat_gamble_locked
                        )
                    } else {
                        format!("GROOVE GAMBLE  x{:.2}", self.beat_gamble_mult)
                    };
                    let mut badge = Text::new(txt);
                    badge.set_scale(20.0);
                    let bw = badge.measure(ctx)?.x;
                    *cache = Some((key, badge, bw));
                }
                let (_, badge, bw) = cache.as_ref().unwrap();
                let scale = pop.min(1.4);
                let dw = bw * scale;
                // Bank flash washes the badge gold on a successful cash-out.
                let bf = self.gamble_bank_flash;
                let cr = (r * pop + bf * 0.6).min(1.0);
                let cg = (g * pop + bf * 0.5).min(1.0);
                let cb = (b * pop + bf * 0.2).min(1.0);
                canvas.draw(
                    badge,
                    DrawParam::default()
                        .dest(Vec2::new((width - dw) / 2.0, 56.0))
                        .scale(Vec2::new(scale, scale))
                        .color(Color::new(cr, cg, cb, 1.0)),
                );
                Ok(())
            })?;

            // "BANK NOW  [B]" prompt — breathes under the badge while there's an unbanked stack big
            // enough to be worth cashing out, so the player learns the fork is theirs to call.
            // Built once and cached (same static-string-measure pattern as ON_BEAT_TEXT_CACHE /
            // STAMINA_LABEL_CACHE) since it's visible every frame a hot Groove Gamble streak runs.
            if self.beat_gamble_mult > self.beat_gamble_locked + 0.5 {
                let breathe = 0.55 + 0.45 * (self.gamble_bank_pulse.sin() * 0.5 + 0.5);
                BANK_NOW_PROMPT_CACHE.with(|c| -> GameResult {
                    let mut cache = c.borrow_mut();
                    if cache.is_none() {
                        let mut prompt = Text::new("BANK NOW  [B]");
                        prompt.set_scale(18.0);
                        let pw = prompt.measure(ctx)?.x;
                        *cache = Some((prompt, pw));
                    }
                    let (prompt, pw) = cache.as_ref().unwrap();
                    canvas.draw(
                        prompt,
                        DrawParam::default()
                            .dest(Vec2::new((width - pw) / 2.0, 82.0))
                            .color(Color::new(1.0, 0.9, 0.35, breathe)),
                    );
                    Ok(())
                })?;
            }
        }

        // Streak-lost sting — a brief red screen wash when a hot Gamble breaks, so the cost of a
        // greedy off-beat grab lands viscerally, not just as a vanished number.
        if self.streak_lost_flash > 0.0 {
            let alpha = (self.streak_lost_flash * 90.0) as u8;
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .scale(Vec2::new(width, height))
                    .color(Color::from_rgba(200, 40, 40, alpha)),
            );
        }

        // Dash flash — cyan burst when Space is pressed
        if self.dash_flash > 0.0 {
            let alpha = (self.dash_flash * 130.0) as u8;
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .scale(Vec2::new(width, height))
                    .color(Color::from_rgba(220, 240, 255, alpha)),
            );
        }

        // Downbeat Slam flash — warm gold full-screen bloom when the ultimate lands.
        if self.slam_flash > 0.0 {
            let alpha = (self.slam_flash * 150.0) as u8;
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .scale(Vec2::new(width, height))
                    .color(Color::from_rgba(255, 225, 120, alpha)),
            );
        }

        // On-beat catch flash
        if self.on_beat_flash > 0.0 {
            let fa = (self.on_beat_flash * 180.0) as u8;
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .scale(Vec2::new(width, height))
                    .color(Color::from_rgba(255, 220, 80, fa)),
            );
            let btw = ON_BEAT_TEXT_CACHE.with(|c| -> ggez::GameResult<f32> {
                let mut cache = c.borrow_mut();
                if cache.is_none() {
                    let mut bonus_text = Text::new("ON BEAT! +1");
                    bonus_text.set_scale(36.0);
                    let btw = bonus_text.measure(ctx)?.x;
                    *cache = Some((bonus_text, btw));
                }
                Ok(cache.as_ref().unwrap().1)
            })?;
            ON_BEAT_TEXT_CACHE.with(|c| {
                let cache = c.borrow();
                let (bonus_text, _) = cache.as_ref().unwrap();
                canvas.draw(
                    bonus_text,
                    DrawParam::default()
                        .dest(Vec2::new((width - btw) / 2.0, height / 2.0 - 60.0))
                        .color(Color::from_rgba(255, 220, 50, fa)),
                );
            });
        }

        return Ok(());
    }

    fn draw_level_title(
        &self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        width: f32,
        height: f32,
    ) -> Result<(), ggez::GameError> {
        // The title overlay shows for ~1s (~60 frames) each level transition. All 4 objects
        // (title Text, biome Text, fill Mesh, stroke Mesh) are constant for the entire window
        // — only the fade alpha (a DrawParam, not baked into the objects) varies per frame.
        // Build and cache them on the first frame, reuse for the remaining ~59.
        let biome = self.levels[self.current_level.min(self.levels.len() - 1)].biome;
        LEVEL_TITLE_OVERLAY_CACHE.with(|c| -> Result<(), ggez::GameError> {
            let mut cache = c.borrow_mut();
            let needs_rebuild = match &*cache {
                Some((cached_title, cached_biome, _, _, _, _, _, _, _, _, _, _)) => {
                    cached_title != &self.level_title || *cached_biome != biome.name
                }
                None => true,
            };
            if needs_rebuild {
                let mut title = Text::new(&self.level_title);
                title.set_scale(96.0);
                let title_width = title.measure(ctx)?.x;
                let title_height = title.measure(ctx)?.y;
                let rect_x = (width - title_width) / 2.0 - 32.0;
                let rect_y = (height - title_height) / 2.0 - 16.0;
                let rect_w = title_width + 64.0;
                let rect_h = title_height + 32.0;
                let bg_rect = Mesh::new_rectangle(
                    ctx,
                    ggez::graphics::DrawMode::fill(),
                    Rect::new(rect_x, rect_y, rect_w, rect_h),
                    Color::from_rgb(30, 30, 30),
                )?;
                let border_rect = Mesh::new_rectangle(
                    ctx,
                    ggez::graphics::DrawMode::stroke(3.0),
                    Rect::new(rect_x, rect_y, rect_w, rect_h),
                    Color::from_rgb(220, 220, 220),
                )?;
                let mut subtitle = Text::new(biome.name);
                subtitle.set_scale(40.0);
                let sub_width = subtitle.measure(ctx)?.x;
                let emphasis = self.levels[self.current_level.min(self.levels.len() - 1)].emphasis;
                let threat_opt = if let Some(label) = crate::levels::emphasis_label(emphasis) {
                    let mut threat = Text::new(format!("!! {label} !!"));
                    threat.set_scale(30.0);
                    let tw = threat.measure(ctx)?.x;
                    Some((threat, tw))
                } else {
                    None
                };
                *cache = Some((
                    self.level_title.clone(),
                    biome.name,
                    title,
                    bg_rect,
                    border_rect,
                    subtitle,
                    title_width,
                    title_height,
                    rect_y,
                    rect_h,
                    sub_width,
                    threat_opt,
                ));
            }
            let (
                _,
                _,
                title,
                bg_rect,
                border_rect,
                subtitle,
                title_width,
                title_height,
                rect_y,
                rect_h,
                sub_width,
                threat_opt,
            ) = cache.as_ref().unwrap();
            canvas.draw(bg_rect, DrawParam::default());
            canvas.draw(border_rect, DrawParam::default());
            canvas.draw(
                title,
                DrawParam::default()
                    .dest(Vec2::new(
                        (width - title_width) / 2.0,
                        (height - title_height) / 2.0,
                    ))
                    .color(Color::from_rgb(240, 240, 240)),
            );
            let (pr, pg, pb) = biome.pulse;
            canvas.draw(
                subtitle,
                DrawParam::default()
                    .dest(Vec2::new((width - sub_width) / 2.0, rect_y + rect_h + 12.0))
                    .color(Color::from_rgb(pr, pg, pb)),
            );
            // Zone threat banner — built and measured once in the cache above, reused every frame.
            if let Some((threat, tw)) = threat_opt {
                canvas.draw(
                    threat,
                    DrawParam::default()
                        .dest(Vec2::new((width - tw) / 2.0, rect_y + rect_h + 60.0))
                        .color(Color::from_rgb(255, 170, 60)),
                );
            }
            Ok(())
        })
    }

    /// Big gold "FRENZY!" shout when a frenzy wave lands. Pops in with a scale punch and fades
    /// out with `frenzy_banner_timer`; sits high on screen so it never fights the level title.
    fn draw_frenzy_banner(
        &self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        width: f32,
        height: f32,
    ) -> Result<(), ggez::GameError> {
        // Normalized life 0..1 (1 = just landed). Fade over the last third; punch scale early.
        let life = (self.frenzy_banner_timer / 1.6).clamp(0.0, 1.0);
        let alpha = (life * 3.0).min(1.0); // hold, then fade only in the final third
        // Beat-synced throb so it pulses with the music like everything else.
        let beat_phase = 1.0 - (self.beat_timer / self.beat_interval).clamp(0.0, 1.0);
        let throb = (beat_phase * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        // Slightly larger right as it lands, settling to a gently throbbing size.
        let scale = 1.15 - life * 0.15 + throb * 0.06;

        let dims = FRENZY_BANNER_CACHE.with(|cache_cell| -> Result<Vec2, ggez::GameError> {
            let mut cache = cache_cell.borrow_mut();
            if cache.is_none() {
                let mut banner = Text::new("FRENZY!");
                banner.set_scale(84.0);
                let dims: Vec2 = banner.measure(ctx)?.into();
                *cache = Some((banner, dims));
            }
            Ok(cache.as_ref().unwrap().1)
        })?;
        let dest = Vec2::new(
            width / 2.0 - dims.x * scale / 2.0,
            height * 0.16 - dims.y * scale / 2.0,
        );
        let a = (alpha * 255.0) as u8;
        let g = (200.0 + throb * 55.0) as u8;
        FRENZY_BANNER_CACHE.with(|cache_cell| {
            let cache = cache_cell.borrow();
            let banner = &cache.as_ref().unwrap().0;
            // Dark drop-shadow behind for legibility over any biome.
            canvas.draw(
                banner,
                DrawParam::default()
                    .dest(dest + Vec2::splat(3.0))
                    .scale(Vec2::splat(scale))
                    .color(Color::from_rgba(20, 12, 0, (a as f32 * 0.7) as u8)),
            );
            // Gold body, brightening on the beat.
            canvas.draw(
                banner,
                DrawParam::default()
                    .dest(dest)
                    .scale(Vec2::splat(scale))
                    .color(Color::from_rgba(255, g, 60, a)),
            );
        });
        Ok(())
    }

    /// Cyan "BUILDING / HEATED / FEVER …" shout when the run climbs into a new intensity stage.
    /// Same pop-and-fade feel as the Frenzy banner but a cool color and a slightly lower slot, so
    /// the two read as distinct events (spike vs. rising tide) if they ever land close together.
    fn draw_stage_banner(
        &self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        width: f32,
        height: f32,
    ) -> Result<(), ggez::GameError> {
        let life = (self.stage_banner_timer / 2.0).clamp(0.0, 1.0);
        let alpha = (life * 3.0).min(1.0); // hold, then fade only in the final third
        let beat_phase = 1.0 - (self.beat_timer / self.beat_interval).clamp(0.0, 1.0);
        let throb = (beat_phase * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        let scale = 1.1 - life * 0.12 + throb * 0.05;

        let name = self.stage_banner_name;
        let dims = STAGE_BANNER_CACHE.with(|cache_cell| -> Result<Vec2, ggez::GameError> {
            let mut cache = cache_cell.borrow_mut();
            let needs_rebuild = match cache.as_ref() {
                Some((cached_name, _, _)) => *cached_name != name,
                None => true,
            };
            if needs_rebuild {
                let mut banner = Text::new(name);
                banner.set_scale(64.0);
                let dims: Vec2 = banner.measure(ctx)?.into();
                *cache = Some((name, banner, dims));
            }
            Ok(cache.as_ref().unwrap().2)
        })?;
        let dest = Vec2::new(
            width / 2.0 - dims.x * scale / 2.0,
            height * 0.27 - dims.y * scale / 2.0,
        );
        let a = (alpha * 255.0) as u8;
        let b = (200.0 + throb * 55.0) as u8;
        STAGE_BANNER_CACHE.with(|cache_cell| {
            let cache = cache_cell.borrow();
            let banner = &cache.as_ref().unwrap().1;
            canvas.draw(
                banner,
                DrawParam::default()
                    .dest(dest + Vec2::splat(3.0))
                    .scale(Vec2::splat(scale))
                    .color(Color::from_rgba(4, 16, 20, (a as f32 * 0.7) as u8)),
            );
            // Cyan body, brightening on the beat.
            canvas.draw(
                banner,
                DrawParam::default()
                    .dest(dest)
                    .scale(Vec2::splat(scale))
                    .color(Color::from_rgba(90, 230, b, a)),
            );
        });
        Ok(())
    }

    /// Draw the tutorial session's instruction card (title + what-to-do + live progress) pinned to
    /// the top of the screen, plus a big centered "PASSED!" flourish once the session is cleared.
    /// Previously rebuilt a Mesh::new_rounded_rectangle (GPU buffer) + 4-6 Text objects (glyph-
    /// shaping) every single frame the tutorial was active. Now uses TUTORIAL_OVERLAY_CACHE:
    /// — the card mesh is keyed by (width, height) bit-patterns (same as MENU_PANEL_CACHE)
    /// — the static texts (title, instruction, "Esc" hint, "PASSED!") are cached once per kind
    /// — the progress counter text rebuilds only when the counter actually advances (once per catch)
    fn draw_tutorial_overlay(
        &self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        width: f32,
        height: f32,
    ) -> Result<(), ggez::GameError> {
        let t = match &self.tutorial {
            Some(t) => t,
            None => return Ok(()),
        };

        // The counter that drives the progress line. Different fields track progress per kind.
        let progress_key = match t.kind {
            crate::tutorial::TutorialKind::BeatTiming => t.on_beat_catches,
            crate::tutorial::TutorialKind::ChainDeliver => t.deliveries,
            crate::tutorial::TutorialKind::ShellCrack => t.shells_cracked,
            crate::tutorial::TutorialKind::LassoGrab => t.lasso_catches,
        };
        let title_key = t.title(); // &'static str — also serves as the kind discriminant
        let wbits = width.to_bits();
        let hbits = height.to_bits();

        TUTORIAL_OVERLAY_CACHE.with(|cell| -> ggez::GameResult {
            let mut cache = cell.borrow_mut();

            // Invalidate if kind changed, screen resized, or progress counter advanced.
            let stale = match &*cache {
                None => true,
                Some((tk, wb, hb, _, _, _, _, _, _, _, _, _, _, _, pk, _, _)) => {
                    *tk != title_key || *wb != wbits || *hb != hbits || *pk != progress_key
                }
            };

            if stale {
                let card = Mesh::new_rounded_rectangle(
                    ctx,
                    ggez::graphics::DrawMode::fill(),
                    Rect::new(width * 0.5 - 360.0, 24.0, 720.0, 132.0),
                    14.0,
                    Color::from_rgba(8, 14, 26, 200),
                )?;

                let mut title_text = Text::new(t.title());
                title_text.set_scale(30.0);
                let tdims: Vec2 = title_text.measure(ctx)?.into();

                let mut instr_text = Text::new(t.instruction());
                instr_text.set_scale(20.0);
                let idims: Vec2 = instr_text.measure(ctx)?.into();

                let mut hint_text = Text::new("Esc — back to menu");
                hint_text.set_scale(18.0);
                let hw = hint_text.measure(ctx).map(|m| m.x).unwrap_or(0.0);

                let mut passed_text = Text::new("PASSED!");
                passed_text.set_scale(80.0);
                let pdims: Vec2 = passed_text.measure(ctx)?.into();

                let prog_str = t.progress_line();
                let mut prog_text = Text::new(prog_str);
                prog_text.set_scale(24.0);
                let prog_w = prog_text.measure(ctx).map(|m| m.x).unwrap_or(0.0);

                *cache = Some((
                    title_key,
                    wbits,
                    hbits,
                    card,
                    title_text,
                    tdims.x,
                    tdims.y,
                    instr_text,
                    idims.x,
                    hint_text,
                    hw,
                    passed_text,
                    pdims.x,
                    pdims.y,
                    progress_key,
                    prog_text,
                    prog_w,
                ));
            }

            let (
                _,
                _,
                _,
                card,
                title_text,
                tw,
                _,
                instr_text,
                iw,
                hint_text,
                hw,
                passed_text,
                pasw,
                pash,
                _,
                prog_text,
                prog_w,
            ) = cache.as_ref().unwrap();

            // Translucent card backdrop across the top so the instruction text reads over any terrain.
            canvas.draw(card, DrawParam::default());

            canvas.draw(
                title_text,
                DrawParam::default()
                    .dest(Vec2::new(width * 0.5 - tw / 2.0, 38.0))
                    .color(Color::from_rgb(255, 226, 120)),
            );

            canvas.draw(
                instr_text,
                DrawParam::default()
                    .dest(Vec2::new(width * 0.5 - iw / 2.0, 76.0))
                    .color(Color::from_rgb(220, 232, 245)),
            );

            canvas.draw(
                prog_text,
                DrawParam::default()
                    .dest(Vec2::new(width * 0.5 - prog_w / 2.0, 124.0))
                    .color(Color::from_rgb(120, 255, 150)),
            );

            // Bottom hint so a player who wants out knows how — this is opt-in teaching, no gating.
            canvas.draw(
                hint_text,
                DrawParam::default()
                    .dest(Vec2::new(width * 0.5 - hw / 2.0, height - 40.0))
                    .color(Color::from_rgba(200, 210, 225, 180)),
            );

            // Cleared: a big pulsing "PASSED!" centered while the exit hold runs out.
            // `pass_glow` and `scale` are per-frame so they stay outside the cache.
            if t.completed {
                let scale = 1.0 + t.pass_glow * 0.15;
                let dest = Vec2::new(
                    width / 2.0 - pasw * scale / 2.0,
                    height * 0.42 - pash * scale / 2.0,
                );
                canvas.draw(
                    passed_text,
                    DrawParam::default()
                        .dest(dest + Vec2::splat(3.0))
                        .scale(Vec2::splat(scale))
                        .color(Color::from_rgba(4, 20, 8, 180)),
                );
                canvas.draw(
                    passed_text,
                    DrawParam::default()
                        .dest(dest)
                        .scale(Vec2::splat(scale))
                        .color(Color::from_rgb(110, 255, 140)),
                );
            }

            Ok(())
        })
    }

    fn draw_crabs_with_shake(&self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        let mut rng = rand::rng();
        // Every free crab's aura below (flashlight glow, Magnet/Thief/Golden rings) additively
        // blends, and used to flip the canvas's blend mode ADD -> ALPHA -> ADD per crab (each aura
        // helper toggled it around itself). ggez only actually switches the GPU pipeline on a
        // transition between consecutive queued draws, so that per-crab toggling was a real
        // per-crab pipeline-state churn. Setting ADD once for this whole aura pass and restoring
        // once after collapses that into a single transition in, one out — same visuals (draw_crab
        // itself defers into batched buffers and isn't blended here, so it's unaffected).
        let original_blend = canvas.blend_mode();
        canvas.set_blend_mode(BlendMode::ADD);
        for (i, crab) in self.crabs.iter().enumerate() {
            if !crab.caught {
                let mut pos = crab.pos;
                let mut shake_strength = 0.0;
                if crab.spooked_timer > 0.0 {
                    shake_strength = 18.0 * crab.spooked_timer;
                } else if self.shake_timer > 0.0 {
                    shake_strength = 18.0 * self.shake_timer;
                }
                if shake_strength > 0.0 {
                    let t = self.time_elapsed * 30.0 + i as f32 * 2.0;
                    pos.x += (t).sin() * shake_strength
                        + rng.random_range(-shake_strength..=shake_strength) * 0.3;
                    pos.y += (t * 1.3).cos() * shake_strength
                        + rng.random_range(-shake_strength..=shake_strength) * 0.3;
                }
                let crab_beat = (self.beat_intensity * 0.7
                    + (crab.pos.x * 0.003).sin().abs() * 0.3)
                    .clamp(0.0, 1.0);
                // The wild herd grooves too. Free crabs bob with the music, but with a spatial phase
                // offset from screen position so the field reads as several organic ripples rolling
                // through the crowd rather than a lockstep jump — the party the player recruits from
                // is alive, not a static pickup field. Only the *amplitude* is beat-gated (the hop
                // swells on the downbeat and settles between beats), so the whole beach breathes with
                // the pulse. Kept smaller than the conga train's dramatic wave (amplitude ~10-26) so
                // caught crabs still read as the liveliest dancers. Bosses don't dance — a bopping
                // King Crab would undercut its menace — and fleeing/spooked crabs sit it out too
                // (panic, not party), so the hop reads as mood rather than a global clock.
                let wild_lift = if crab.is_boss()
                    || crab.fleeing
                    || crab.spooked_timer > 0.0
                    || crab.startle_timer > 0.0
                {
                    0.0
                } else {
                    let ripple = (crab.pos.x + crab.pos.y) * 0.012;
                    // Positive bump only — a hop, never a dip into the ground.
                    (self.beat_intensity * (ripple - self.time_elapsed * 5.0).sin()).max(0.0) * 7.0
                };
                // Raise the body by the hop (draw_pos moves up); pass the same amount as y_lift so
                // the drop shadow shrinks/detaches underneath, matching how the conga train hops.
                let hop_pos = pos - Vec2::new(0.0, wild_lift);
                draw_crab(
                    ctx,
                    canvas,
                    crab,
                    hop_pos,
                    crab_beat,
                    crab.join_pulse,
                    wild_lift,
                    crab.facing_angle,
                    self.time_elapsed,
                )?;
                // CATCH-NEXT hint: if this free crab shares the current tail's archetype, catching it
                // next would extend the tail match-run (tail_run_len). Interior chain order is frozen,
                // so this catch-order choice is the one arrangement lever the player actually controls —
                // surface it as a ring in the crab's own type color so "grab me to keep the run going"
                // reads live in the field. Skip bosses and spooked/fleeing crabs (not sensible grabs),
                // and only bother once a train exists to extend. Purely legibility, no odds change.
                if self.chain_count > 0
                    && !crab.is_boss()
                    && !crab.fleeing
                    && crab.spooked_timer <= 0.0
                    && crab.startle_timer <= 0.0
                    && self.cached_tail_type == Some(crab.crab_type)
                {
                    draw_catch_next_hint(
                        ctx,
                        canvas,
                        hop_pos + Vec2::splat(crab.scale * CRAB_SIZE * 0.5),
                        crab.scale * CRAB_SIZE * 0.7,
                        crab.crab_color(),
                        self.time_elapsed,
                        self.beat_intensity,
                        self.tail_run_len,
                    )?;
                }
                // Scorch ring — ONLY for a shelled target the beam is actively burning down (a boss
                // or an Armored crab still holding shell, lit up in the cone). This replaced the old
                // "attraction halo" that used to ring every crab the light touched: the beam no
                // longer herds normal crabs, so there's nothing to halo. A crab burning under the
                // beam gets a harsh white-hot searing ring so the read is unmistakably "melting".
                if crab.in_flashlight
                    && crab.boss_health > 0.0
                    && (crab.is_boss() || crab.is_armored())
                {
                    let size = crab.scale * CRAB_SIZE;
                    draw_attracted_crab_glow(
                        ctx,
                        canvas,
                        pos,
                        size,
                        [1.0, 0.9, 0.55],
                        self.time_elapsed,
                        self.beat_intensity,
                    )?;
                }
                // Boss aura + wear-down health ring — aura tinted per archetype.
                if crab.is_boss() {
                    let size = crab.scale * CRAB_SIZE;
                    let frac = crab.boss_health / BOSS_MAX_HEALTH;
                    let base_aura = if crab.is_tide_boss() {
                        [0.25, 0.7, 1.0]
                    } else if crab.is_rhythm_boss() {
                        // The Reef DJ pulses violet, and flares bright only on a *hot* beat of the
                        // phrase it called this bar — that's the window its shell is open, so the aura
                        // flash IS the "hit now" cue. A landed hot beat adds an extra bloom via
                        // reef_hit_flash so a clean echo reads as a satisfying pop of light.
                        let on_beat = self.beat_timer < BEAT_WINDOW
                            || self.beat_timer > self.beat_interval - BEAT_WINDOW;
                        let hot = on_beat && self.reef_phrase[(self.beat_count % 4) as usize];
                        let flare = if hot { 0.45 } else { 0.0 } + self.reef_hit_flash * 0.35;
                        [(0.72 + flare * 0.3).min(1.0), (0.30 + flare).min(1.0), 0.95]
                    } else {
                        [1.0, 0.8, 0.25]
                    };
                    // Enraged bosses glow hot: shift the aura toward an angry pulsing red so the final
                    // phase reads instantly, matching the ramped-up charge/pulse behavior.
                    let aura = if crab.enraged {
                        let p = 0.5 + 0.5 * (self.time_elapsed * 9.0).sin();
                        [
                            (base_aura[0] * 0.4 + 0.6_f32).min(1.0),
                            base_aura[1] * (0.35 + 0.15 * p),
                            base_aura[2] * (0.35 + 0.15 * p),
                        ]
                    } else {
                        base_aura
                    };
                    draw_boss_health_ring(ctx, canvas, pos, size, frac, self.time_elapsed, aura)?;
                } else if crab.is_armored() && crab.boss_health > 0.0 {
                    // Armored shell indicator — depletes as the shell is worn or cracked
                    let size = crab.scale * CRAB_SIZE;
                    let frac = crab.boss_health / crab.crab_type.initial_shell().max(0.001);
                    draw_armor_ring(ctx, canvas, pos, size, frac, self.time_elapsed)?;
                } else if crab.is_shelled_hermit() {
                    // Hermit borrowed-shell indicator — a warm coppery coiled ring, visually distinct
                    // from the Armored crab's cold steely arc, so the player learns "this one the beam
                    // won't crack; use the ecosystem" at a glance. Depletes as the shell is chipped.
                    let size = crab.scale * CRAB_SIZE;
                    let frac = crab.boss_health / crab.crab_type.initial_shell().max(0.001);
                    draw_hermit_shell(ctx, canvas, pos, size, frac, self.time_elapsed)?;
                } else if crab.is_magnet() {
                    // Magnetic field aura — inward-sweeping rings showing its pull radius, so the
                    // player can see the catchment and chase it for the two-for-one cluster catch.
                    let size = crab.scale * CRAB_SIZE;
                    draw_magnet_aura(
                        ctx,
                        canvas,
                        pos,
                        size,
                        240.0,
                        self.time_elapsed,
                        crab.is_magnet_lured(),
                        crab.is_magnet_charged(),
                    )?;
                } else if crab.is_thief() {
                    // Thief marker — a sly green ring while it prowls, flaring into a fast gnaw-ring
                    // once it's latched onto the tail so the theft-in-progress reads at a glance.
                    let size = crab.scale * CRAB_SIZE;
                    draw_thief_aura(
                        ctx,
                        canvas,
                        pos,
                        size,
                        crab.is_latched(),
                        crab.is_magnet_intercepted(),
                        crab.is_thief_lured(),
                        self.time_elapsed,
                    )?;
                } else if crab.is_golden() {
                    // Golden crab shine — a shimmering ring of orbiting sparkles so the rare prize
                    // catches the eye across the whole field and reads as "chase this one!".
                    let size = crab.scale * CRAB_SIZE;
                    draw_golden_sparkle(
                        ctx,
                        canvas,
                        pos,
                        size,
                        self.time_elapsed,
                        crab.is_magnet_snared(),
                    )?;
                } else if crab.is_splitter() {
                    // Splitter cleave aura — a teal ring with two halves pulsing apart, so the
                    // player reads "this one splits my train" and can decide to set it up or dodge.
                    // `beat_prox` peaks (→1) as the beat lands so the aura flares gold in the
                    // clean-cut window, telegraphing the timing bet BEFORE the catch: grab it while
                    // it's hot for the full jackpot cut, or it's a sloppy half-cut. Distance to the
                    // nearest beat edge, scaled by the same BEAT_WINDOW the clean-cut gate uses, so
                    // the flare and the actual reward window agree.
                    let size = crab.scale * CRAB_SIZE;
                    let to_beat = self.beat_timer.min(self.beat_interval - self.beat_timer);
                    let beat_prox = (1.0 - to_beat / (BEAT_WINDOW * 1.5)).clamp(0.0, 1.0);
                    draw_splitter_aura(ctx, canvas, pos, size, self.time_elapsed, beat_prox)?;
                }
            }
        }
        // Flush all Golden-sparkle dots that draw_golden_sparkle() deferred into GOLDEN_SPARKLE_PARAMS
        // during the per-crab aura pass above. Still in ADD blend mode here (restored right after),
        // so the sparkle dots land in the same blend state they always did.
        crate::graphics::flush_golden_sparkles(ctx, canvas)?;
        // Flush hermit coil dots deferred by draw_hermit_shell() calls above — same pattern as
        // the golden sparkles: up to 5 unit-circle draws per shelled Hermit, now one GPU submission.
        flush_hermit_coil_dots(ctx, canvas)?;
        // Flush catch-next-hint tick dots deferred by draw_catch_next_hint() calls above. All
        // dots share the same fixed stroke-circle mesh, so the entire per-crab-per-tick payload
        // collapses to one draw_instanced_mesh — from up to 60 calls (15 matching crabs × 4 dots)
        // down to 1. Same blend mode (still in ADD), identical on-screen output.
        flush_catch_next_ticks(ctx, canvas)?;
        // Flush Magnet aura rings deferred by draw_magnet_aura() calls above. In the Water biome
        // (Magnet-heavy after the biome archetype redirect) this collapses N×3 individual sweep-ring
        // draw calls into at most 3 batched draw_instanced_mesh calls — one per phase bucket — plus
        // up to N core-ring calls. Net: from ~20 GPU submissions for 5 Magnets to ~8.
        flush_magnet_auras(ctx, canvas)?;
        // Flush attracted-crab glow rings deferred by draw_attracted_crab_glow() above. Each crab
        // in the flashlight beam deferred 2 canvas.draw() calls (outer soft-glow + inner ring) into
        // key-grouped scratch maps; now collapsed to one draw_instanced_mesh per distinct stroke
        // radius bucket. With ~10-30 crabs in beam range this trims 20-60 individual GPU submissions
        // down to ~2-4 batched ones. Same blend mode (caller already in ADD), same pixels.
        flush_attracted_crab_glows(ctx, canvas)?;
        // Flush beat-corona halos deferred by draw_crab() for caught (conga-train) crabs during
        // a strong beat pulse. Each corona is one soft circle in the crab's own color, drawn here
        // while the canvas is still in ADD blend so they addively light up the train on every
        // downbeat — one GPU submission for the entire conga train's glow regardless of length.
        flush_beat_coronas(ctx, canvas)?;
        canvas.set_blend_mode(original_blend);
        // Which seated links are part of a paying CENTERPIECE run right now, so we can ring them
        // live (see draw_centerpiece_ring). Computed once per frame from the same predicate the pen
        // pays on. `keep` mirrors the delivered count used at bank time (chain_count == train len).
        // Uses a reused thread-local scratch buffer (take/fill/put-back) instead of allocating a
        // fresh Vec every frame — eliminates a ~60 Hz heap alloc on any frame a train is present.
        let mut centerpiece_set =
            CENTERPIECE_OUT_BUF.with(|buf| std::mem::take(&mut *buf.borrow_mut()));
        centerpiece_set.clear();
        self.centerpiece_link_indices(self.chain_count, &mut centerpiece_set);
        // Interior link under the flashlight aim right now — the one a bubble-swap (X on beat) would
        // move toward the centre. Computed once so the per-crab draw loop can ring it as a preview.
        let aimed_bubble_link = if self.cycle_preview_active {
            self.aimed_interior_link()
        } else {
            None
        };
        // Draw chain crabs with a groovy wave bob that travels through the train
        for crab in self.crabs.iter() {
            if crab.caught {
                let (bob, sway) = if let Some(ci) = crab.chain_index {
                    let amplitude = 10.0 + self.beat_intensity * 16.0;
                    let wave_phase = self.time_elapsed * 6.0 - ci as f32 * 0.55;
                    let b = wave_phase.sin() * amplitude;
                    let s = (wave_phase + std::f32::consts::FRAC_PI_2).sin() * amplitude * 0.5;
                    (b, s)
                } else {
                    (0.0, 0.0)
                };
                let chain_beat = self.beat_intensity.clamp(0.0, 1.0);
                let lift = bob.min(0.0).abs(); // lift = how much the crab is up (bob is negative = up)
                draw_crab(
                    ctx,
                    canvas,
                    crab,
                    crab.pos + Vec2::new(sway, bob),
                    chain_beat,
                    crab.join_pulse,
                    lift,
                    crab.facing_angle,
                    self.time_elapsed,
                )?;
                // CYCLE PREVIEW: ring the crab a Cycle (X) would promote to the head (the link at
                // chain_index 1). Only when the verb is actually available (cache is None otherwise),
                // so the marker appears exactly when pressing X would land this crab up front — letting
                // the player choose a cycle for its arrangement outcome instead of mashing blind.
                if self.cycle_preview_active && crab.chain_index == Some(1) {
                    draw_cycle_preview_ring(
                        ctx,
                        canvas,
                        crab.pos + Vec2::new(sway, bob) + Vec2::splat(crab.scale * CRAB_SIZE * 0.5),
                        crab.scale * CRAB_SIZE * 0.7,
                        crab.crab_color(),
                        self.time_elapsed,
                        self.beat_intensity,
                        crab.is_golden() || crab.is_dancer(),
                    )?;
                }
                // BUBBLE PREVIEW: when the flashlight is aimed at an interior link, ring THAT crab so
                // the player sees which one X (on beat) will bubble one slot toward the centre — the
                // legibility that turns the local swap from a blind guess into a placed decision. Green
                // tint distinguishes it from the head-promote cyan cycle ring above.
                if self.cycle_preview_active
                    && aimed_bubble_link.is_some()
                    && crab.chain_index == aimed_bubble_link
                {
                    draw_cycle_preview_ring(
                        ctx,
                        canvas,
                        crab.pos + Vec2::new(sway, bob) + Vec2::splat(crab.scale * CRAB_SIZE * 0.5),
                        crab.scale * CRAB_SIZE * 0.7,
                        [0.5, 1.0, 0.7],
                        self.time_elapsed,
                        self.beat_intensity,
                        true,
                    )?;
                }
                // CENTERPIECE: ring this link if it's part of a paying mid-train run. Reads as an
                // amber laurel so the player sees the protected centerpiece forming as they build,
                // turning "hold a long train" into an arrangement puzzle they set up on purpose.
                if let Some(ci) = crab.chain_index {
                    if !centerpiece_set.is_empty() && centerpiece_set.binary_search(&ci).is_ok() {
                        // An endpoint is a link at the start/end of its own contiguous run, i.e.
                        // a neighbouring index isn't also in the set — works even if two runs
                        // qualify at once (the vec concatenates them but they're non-adjacent).
                        // centerpiece_set is always sorted (built from extend(start..end_exclusive)
                        // ranges in ascending order), so binary_search replaces the O(n) contains().
                        let is_endpoint =
                            centerpiece_set.binary_search(&ci.wrapping_sub(1)).is_err()
                                || centerpiece_set.binary_search(&(ci + 1)).is_err();
                        draw_centerpiece_ring(
                            ctx,
                            canvas,
                            crab.pos
                                + Vec2::new(sway, bob)
                                + Vec2::splat(crab.scale * CRAB_SIZE * 0.5),
                            crab.scale * CRAB_SIZE * 0.7,
                            self.time_elapsed,
                            self.beat_intensity,
                            is_endpoint,
                        )?;
                    }
                }
            }
        }
        // Ambient NPC conga train — drawn into the same deferred leg/body buffers as player crabs.
        self.draw_npc_conga_train(ctx, canvas)?;

        // Every draw_crab() call above deferred its 6 leg draws and 12 body-part (shadow, shell,
        // claws, eyes) draws into shared buffers instead of issuing them individually (up to
        // 18 x 50+ crabs = 900+ draw calls). Flush them both here as two instanced batches — same
        // parts, same positions/rotations/colors, two GPU submissions instead of hundreds. This
        // does mean legs and body parts across all crabs now draw as two groups after every crab's
        // glow/ring this frame, instead of interleaved per-crab; since legs are thin lines mostly
        // beside the body and the glow/rings are soft translucent overlays, the reordering isn't
        // perceptible in motion.
        crate::graphics::flush_crab_legs(ctx, canvas)?;
        crate::graphics::flush_crab_bodies(ctx, canvas)?;
        // Flush centerpiece bracket-dot DrawParams deferred by draw_centerpiece_ring() calls
        // above — same technique as hermit-coil and catch-next-tick batching. Up to 10 dots per
        // centerpiece link (a 6-link run → 60 individual canvas.draw() calls) collapsed to one
        // instanced draw regardless of how long the qualifying run gets.
        flush_centerpiece_dots(ctx, canvas)?;
        // Return the scratch buffer to the thread-local so it keeps its allocated capacity for
        // next frame instead of freeing and reallocating it each draw call.
        CENTERPIECE_OUT_BUF.with(|buf| *buf.borrow_mut() = centerpiece_set);
        Ok(())
    }

    fn draw_game_over_screen(&self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        const BOX_WIDTH: f32 = 600.0;
        const BOX_HEIGHT: f32 = 260.0;
        const BOX_X: f32 = 340.0;
        const BOX_Y: f32 = 360.0;

        // All inputs that drive the text are frozen once game_over is set (update() returns
        // early, record_run() fires once) — so build the Mesh and Text objects once and reuse
        // them every subsequent frame rather than paying a GPU buffer upload + glyph-shaping
        // pass ~60 times/second for however long the player sits on the results screen.
        let cache_key = (
            self.score,
            self.time_elapsed.to_bits(),
            self.best_time.to_bits(),
            self.career_best_score,
            self.career_total_score,
            self.career_runs,
            self.run_is_new_best,
        );
        GAME_OVER_CACHE.with(|c| -> GameResult {
            let mut cache = c.borrow_mut();
            let stale = cache.as_ref().map_or(true, |(k, _, _, _)| *k != cache_key);
            if stale {
                let bg_box = Mesh::new_rectangle(
                    ctx,
                    ggez::graphics::DrawMode::fill(),
                    Rect::new(BOX_X, BOX_Y, BOX_WIDTH, BOX_HEIGHT),
                    Color::from_rgba(40, 0, 80, 180),
                )?;
                let text = Text::new(format!(
                    "Game Over!\nThis run: {} crabs banked\nTime: {:.2}s   Best time: {:.2}s\n\nCareer best: {}\nCareer total: {} over {} runs\n\nPress Space or Enter to try again.  Esc to quit.",
                    self.score, self.time_elapsed, self.best_time,
                    self.career_best_score, self.career_total_score, self.career_runs,
                ));
                let banner = if self.run_is_new_best && self.score > 0 {
                    let mut b = Text::new("★ NEW CAREER BEST! ★");
                    b.set_scale(34.0);
                    let bw = b.measure(ctx)?.x;
                    Some((b, bw))
                } else {
                    None
                };
                *cache = Some((cache_key, bg_box, text, banner));
            }
            let (_, bg_box, text, banner) = cache.as_ref().unwrap();
            canvas.draw(bg_box, DrawParam::default());
            canvas.draw(
                text,
                DrawParam::default()
                    .dest(Vec2::new(370.0, 380.0))
                    .color(Color::WHITE),
            );
            // Celebrate a fresh career best with a pulsing banner so beating your record lands.
            // The Text and its width are cached; only the per-frame alpha pulse is computed fresh.
            if let Some((banner_text, bw)) = banner {
                let pulse = 0.55 + 0.45 * (self.menu_time * 5.0).sin().abs();
                canvas.draw(
                    banner_text,
                    DrawParam::default()
                        .dest(Vec2::new(BOX_X + (BOX_WIDTH - bw) / 2.0, BOX_Y - 44.0))
                        .color(Color::new(1.0, 0.85, 0.2, pulse)),
                );
            }
            Ok(())
        })
    }

    /// Screen-space rectangles for the three upgrade cards, in card order (index 0 = card "1").
    /// Shared by the draw code (hover highlight) and the mouse-click handler so they always agree.
    fn upgrade_card_rects(&self) -> [Rect; 3] {
        let w = self.width;
        let h = self.height;
        let card_w = 268.0_f32;
        let card_h = 330.0_f32;
        let gap = 26.0_f32;
        let n = 3usize;
        let total_w = n as f32 * card_w + (n - 1) as f32 * gap;
        let x0 = (w - total_w) / 2.0;
        let y0 = (h - card_h) / 2.0 + 15.0;
        std::array::from_fn(|i| Rect::new(x0 + i as f32 * (card_w + gap), y0, card_w, card_h))
    }

    fn draw_upgrade_screen(&self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        let w = self.width;
        let h = self.height;

        // Dark overlay — reuse the cached unit square instead of a fresh Mesh::new_rectangle GPU
        // buffer every frame (same fix used for every other full-screen flash/fill in draw_game).
        canvas.draw(
            unit_square(ctx)?,
            DrawParam::default()
                .scale(Vec2::new(w, h))
                .color(Color::from_rgba(8, 4, 22, 210)),
        );

        // Three options rolled at queue time (see roll_upgrade_offer). Some deepen a tool lane
        // (rank shown, so committing feels deliberate); some are tradeoffs (a "TRADEOFF" tag
        // instead of a rank). Build a per-card descriptor: (key, icon, name, desc, r,g,b, sub-label,
        // is_lit) where sub-label is the rank line for lanes or "TRADEOFF" for tradeoffs.
        let sub_for = |id: UpgradeId| -> (String, bool) {
            let lane_line = |rank: u32| -> (String, bool) {
                if rank == 0 {
                    ("NEW LANE".to_string(), false)
                } else {
                    (format!("LV {}  ->  {}", rank, rank + 1), true)
                }
            };
            match id {
                UpgradeId::BeamFocus | UpgradeId::Sharpshooter => lane_line(self.beam_rank),
                UpgradeId::LassoFocus | UpgradeId::HeavyHauler => lane_line(self.lasso_rank),
                UpgradeId::WhistleFocus | UpgradeId::Roadrunner => lane_line(self.whistle_rank),
                UpgradeId::StompFocus => lane_line(self.stomp_rank),
                UpgradeId::Featherweight | UpgradeId::WideNet => ("TRADEOFF".to_string(), false),
            }
        };
        let cards: Vec<(String, &str, &str, &str, u8, u8, u8, String, bool)> = (0..3)
            .map(|slot| {
                let id = UPGRADE_POOL[self.offered_upgrades[slot]];
                let (icon, name, desc, r, g, b) = id.card();
                let (sub, lit) = sub_for(id);
                ((slot + 1).to_string(), icon, name, desc, r, g, b, sub, lit)
            })
            .collect();

        let rects = self.upgrade_card_rects();
        let card_w = rects[0].w;
        let card_h = rects[0].h;

        // Build or reuse cached Text objects (title, hint, and all per-card labels). Every
        // Text::new + measure() is a glyph-shaping pass; the card border Mesh::new_rectangle calls
        // were also GPU buffer allocations. The texts only change when a rank changes, which is
        // what dismisses this screen — so in practice the cache hits every frame after the first.
        // The hover highlight is applied as DrawParam color below; no re-layout needed for that.
        let cache_key = (
            self.offered_upgrades,
            self.beam_rank,
            self.lasso_rank,
            self.whistle_rank,
            self.stomp_rank,
        );
        UPGRADE_SCREEN_CACHE.with(|c| -> GameResult {
            let mut cache = c.borrow_mut();
            let needs_rebuild = !matches!(&*cache, Some((k, ..)) if *k == cache_key);
            if needs_rebuild {
                // Title
                let mut title_text = Text::new("CHOOSE AN UPGRADE");
                title_text.set_scale(46.0);
                let title_w = title_text.measure(ctx)?.x;
                // Subtitle
                let mut hint_text = Text::new("Click a card or press its number");
                hint_text.set_scale(20.0);
                let hint_w = hint_text.measure(ctx)?.x;
                // Per-card texts — built explicitly for each of the 3 cards (try_from_fn is not
                // stable yet on this toolchain) and stored as a fixed-size array.
                let mut build_card = |i: usize| -> ggez::GameResult<(
                    Text,
                    f32,
                    Text,
                    f32,
                    Text,
                    f32,
                    Text,
                    f32,
                    Text,
                    f32,
                )> {
                    let (key, icon, name, desc, _, _, _, sub, _) = &cards[i];
                    let mut ico = Text::new(*icon);
                    ico.set_scale(82.0);
                    let iw = ico.measure(ctx)?.x;
                    let mut nm = Text::new(*name);
                    nm.set_scale(26.0);
                    let nw = nm.measure(ctx)?.x;
                    let mut rk = Text::new(sub.clone());
                    rk.set_scale(16.0);
                    let rkw = rk.measure(ctx)?.x;
                    let mut dsc = Text::new(*desc);
                    dsc.set_scale(18.0);
                    let dw = dsc.measure(ctx)?.x;
                    let mut kh = Text::new(format!("[ {} ]", key));
                    kh.set_scale(24.0);
                    let kw = kh.measure(ctx)?.x;
                    Ok((ico, iw, nm, nw, rk, rkw, dsc, dw, kh, kw))
                };
                let card_texts: [(Text, f32, Text, f32, Text, f32, Text, f32, Text, f32); 3] =
                    [build_card(0)?, build_card(1)?, build_card(2)?];
                *cache = Some((
                    cache_key, title_text, title_w, hint_text, hint_w, card_texts,
                ));
            }
            let (_, title_text, title_w, hint_text, hint_w, card_texts) = cache.as_ref().unwrap();

            // Title
            canvas.draw(
                title_text,
                DrawParam::default()
                    .dest(Vec2::new((w - title_w) / 2.0, 58.0))
                    .color(Color::from_rgb(255, 215, 50)),
            );

            // Subtitle: make it obvious the cards are clickable, not just number-key driven.
            canvas.draw(
                hint_text,
                DrawParam::default()
                    .dest(Vec2::new((w - hint_w) / 2.0, 110.0))
                    .color(Color::from_rgba(210, 210, 210, 200)),
            );

            for (i, (_, _, _, _, r, g, b, _, lit)) in cards.iter().enumerate() {
                let (r, g, b, lit) = (*r, *g, *b, *lit);
                let cx = rects[i].x;
                let y0 = rects[i].y;
                let m = self.mouse_pos;
                let hovered = m.x >= cx && m.x <= cx + card_w && m.y >= y0 && m.y <= y0 + card_h;

                let accent = Color::from_rgb(r, g, b);
                let bg_a = if hovered { 190u8 } else { 115u8 };
                let bdr_w = if hovered { 4.0_f32 } else { 2.0_f32 };

                // Card background — unit square scaled to card size, no per-frame GPU buffer alloc.
                canvas.draw(
                    unit_square(ctx)?,
                    DrawParam::default()
                        .dest(Vec2::new(cx, y0))
                        .scale(Vec2::new(card_w, card_h))
                        .color(Color::from_rgba(18, 12, 38, bg_a)),
                );
                // Coloured border — cached stroke rect, same mesh reused per bdr_w key.
                canvas.draw(
                    &cached_stroke_rect(ctx, card_w, card_h, bdr_w)?,
                    DrawParam::default().dest(Vec2::new(cx, y0)).color(accent),
                );

                let (ico, iw, nm, nw, rk, rkw, dsc, dw, kh, kw) = &card_texts[i];
                let rank_col = if lit {
                    accent
                } else {
                    Color::from_rgba(180, 180, 180, 200)
                };
                // Lane rank badge — lit in the lane accent once invested.
                // All elements centered on the card's fixed midline (cx + card_w/2) so no element
                // shifts when rank text width changes between sequential upgrade screens.
                let mid = cx + card_w / 2.0;
                canvas.draw(
                    ico,
                    DrawParam::default()
                        .dest(Vec2::new(mid - iw / 2.0, y0 + 18.0))
                        .color(accent),
                );
                canvas.draw(
                    nm,
                    DrawParam::default()
                        .dest(Vec2::new(mid - nw / 2.0, y0 + 118.0))
                        .color(Color::WHITE),
                );
                canvas.draw(
                    rk,
                    DrawParam::default()
                        .dest(Vec2::new(mid - rkw / 2.0, y0 + 146.0))
                        .color(rank_col),
                );
                canvas.draw(
                    dsc,
                    DrawParam::default()
                        .dest(Vec2::new(mid - dw / 2.0, y0 + 176.0))
                        .color(Color::from_rgba(205, 205, 205, 215)),
                );
                canvas.draw(
                    kh,
                    DrawParam::default()
                        .dest(Vec2::new(mid - kw / 2.0, y0 + card_h - 46.0))
                        .color(accent),
                );
            }
            Ok(())
        })
    }

    // --- Effective per-tool values, derived from the chosen upgrade lanes ---
    // These fold each lane's rank into the base constants at the point of use, so a run that pours
    // level-ups into one tool visibly transforms it (a whistle build sweeps the whole screen; a
    // stomp build fires almost on demand) instead of every build feeling the same.

    /// How fast the beam wears down a King Crab / cracks a shell. Ranking the beam lane turns it
    /// into a boss-hunter tool.
    fn boss_drain_rate(&self) -> f32 {
        BOSS_DRAIN_RATE * (1.0 + 0.6 * self.beam_rank as f32)
    }
    /// Grab radius around the lasso tip. Ranking the lasso lane widens each throw so it sweeps up
    /// whole clusters — a chain-catch build.
    fn lasso_tip_radius(&self) -> f32 {
        60.0 + self.lasso_rank as f32 * 22.0
    }
    /// Is *right now* inside the on-beat window? Used to reward firing a tool on the beat —
    /// the same window that gates on-beat catches, so the timing the player already feels for
    /// catching also pays off for whistle/stomp/dash/beat-wave.
    fn on_beat_now(&self) -> bool {
        self.beat_timer < BEAT_WINDOW || self.beat_timer > self.beat_interval - BEAT_WINDOW
    }
    /// A tool was fired on the beat: bank a "PERFECT!" flash, feed the groove meter, and punch up
    /// the juice (extra beat flash + a hair of zoom). Returns the on-beat multiplier the caller can
    /// apply to the tool's effect (radius/duration), so an on-beat cast simply hits harder.
    fn reward_on_beat_tool(&mut self, at: Vec2, label: &str) -> f32 {
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
    fn fire_drum_roll(&mut self) {
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
    fn snatch_thief_on_beat(&mut self, idx: usize, pos: Vec2) {
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
    fn issue_call(&mut self) {
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
    fn aimed_interior_link(&self) -> Option<usize> {
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
    fn cycle_train(&mut self) {
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
    fn issue_groove_call(&mut self) {
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
    fn downbeat_slam(&mut self, ctx: &mut Context) {
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
    fn whistle_max_radius(&self) -> f32 {
        WHISTLE_MAX_RADIUS * (1.0 + 0.28 * self.whistle_rank as f32)
    }
    /// Whistle recharge time. Ranking the whistle lane shortens it (floored so it can't hit zero).
    fn whistle_cooldown_dur(&self) -> f32 {
        WHISTLE_COOLDOWN * (1.0 - 0.14 * self.whistle_rank as f32).max(0.35)
    }
    /// Inward yank speed of the whistle. Ranking the whistle lane pulls even heavy crabs harder.
    fn whistle_pull_speed(&self) -> f32 {
        WHISTLE_PULL_SPEED * (1.0 + 0.2 * self.whistle_rank as f32)
    }
    /// Reach of the stomp shockwave. Ranking the stomp lane turns a melee tap into a wide slam.
    fn stomp_max_radius(&self) -> f32 {
        STOMP_MAX_RADIUS * (1.0 + 0.3 * self.stomp_rank as f32)
    }
    /// Stomp recharge time. Ranking the stomp lane shortens it (floored) toward spammable.
    fn stomp_cooldown_dur(&self) -> f32 {
        STOMP_COOLDOWN * (1.0 - 0.16 * self.stomp_rank as f32).max(0.3)
    }

    // Beam lane (boss hunter): widens + lengthens the cone and speeds the boss drain (see
    // boss_drain_rate); milestone ranks graft on a disco laser so the lane peaks as a dedicated
    // King-Crab melter rather than a pile of flat numbers. Shared by Beam Focus and the tradeoff
    // cards that also feed the beam lane so the disco-laser milestone fires no matter how it ranks.
    fn rank_beam_lane(&mut self) {
        self.beam_rank += 1;
        self.flashlight.cone_upgrade += 0.18;
        self.flashlight.range_upgrade += 55.0;
        if self.beam_rank == 2 || self.beam_rank == 4 {
            self.flashlight.laser_level += 1;
        }
    }

    // apply_upgrade now lives in src/upgrade.rs (impl MainState there).
}

impl MainState {
    fn update_npc_trains(&mut self, dt: f32) {
        for i in 0..self.npc_trains.len() {
            // --- Idle pause at destination -------------------------------------------------
            // When idle_timer > 0 the train has just arrived at a target and is "surveying"
            // before picking a new one — gives Rain World-style decisiveness, not dumb wandering.
            if self.npc_trains[i].idle_timer > 0.0 {
                self.npc_trains[i].idle_timer -= dt;
                // Decelerate while idling
                self.npc_trains[i].leader_vel *= (1.0 - 6.0 * dt).max(0.0);
                // Still sample path so followers catch up during the pause
                let cur = self.npc_trains[i].leader_pos;
                let last = self.npc_trains[i]
                    .path_history
                    .front()
                    .copied()
                    .unwrap_or(cur);
                if cur.distance_squared(last) > 36.0 {
                    self.npc_trains[i].path_history.push_front(cur);
                }
                let dist_to_player = cur.distance(self.player_pos);
                self.npc_trains[i].target_vol = ((800.0 - dist_to_player) / 600.0).clamp(0.0, 1.0);
                continue;
            }

            let to_target = self.npc_trains[i].target - self.npc_trains[i].leader_pos;
            let dist = to_target.length();

            // --- Wander target selection ---------------------------------------------------
            // Bias targets strongly toward territory center so rivals patrol distinct regions.
            // Small scouts are fast and range further; large elders are slow and stay local.
            self.npc_trains[i].target_timer -= dt;
            if dist < 80.0 || self.npc_trains[i].target_timer <= 0.0 {
                // Arrived — enter a brief idle before picking the next target.
                let rng = &mut rand::rng();
                let idle_secs = rng.random_range(1.2_f32..3.5);
                self.npc_trains[i].idle_timer = idle_secs;

                // Territory-biased target: blend between a random world point and the territory
                // center. Large elder (scale 2.4) stays very local; small scout (scale 1.2) ranges.
                let scale = self.npc_trains[i].leader_scale;
                // territory_bias 0..1: how strongly the next target is pulled toward territory center
                let territory_bias = ((scale - 1.2) / 1.2).clamp(0.0, 1.0) * 0.65 + 0.2;
                let margin = 160.0;
                // Guard against empty range panic if world is unexpectedly small
                let ww = (self.world_width - margin).max(margin + 1.0);
                let wh = (self.world_height - margin).max(margin + 1.0);
                let rand_pt = Vec2::new(rng.random_range(margin..ww), rng.random_range(margin..wh));
                let tc = self.npc_trains[i].territory_center;
                // Offset from territory center — scouts wander further (larger offset radius)
                let wander_radius = 380.0 - scale * 80.0; // scout=284, medium=236, elder=188
                let angle: f32 = rng.random_range(0.0..std::f32::consts::TAU);
                let territory_pt = tc + Vec2::new(angle.cos(), angle.sin()) * wander_radius;
                let next_target = rand_pt.lerp(territory_pt, territory_bias);
                self.npc_trains[i].target = next_target.clamp(
                    Vec2::splat(margin),
                    Vec2::new(self.world_width - margin, self.world_height - margin),
                );
                // Timer is a fallback; normal flow goes through idle_timer arrival check
                self.npc_trains[i].target_timer = rng.random_range(18.0_f32..35.0);
            }

            // --- Steering ------------------------------------------------------------------
            // Speed inversely proportional to leader_scale: scouts zip, elders lumber.
            let speed = match () {
                _ if self.npc_trains[i].leader_scale < 1.5 => 105.0, // small scout
                _ if self.npc_trains[i].leader_scale < 2.0 => 80.0,  // medium wanderer
                _ => 52.0,                                           // large elder
            };
            // Gentle perpendicular wobble so the path curves naturally instead of beelining.
            let perp = Vec2::new(-to_target.y, to_target.x).normalize_or_zero();
            let wobble_phase = self.time_elapsed * 0.4 + i as f32 * 2.1;
            let wobble = perp * wobble_phase.sin() * 18.0;

            if dist > 1.0 {
                let desired = (to_target / dist + wobble / dist.max(1.0)) * speed;
                let steer_rate = if dist < 200.0 { 4.5 } else { 2.8 }; // tighter turns near target
                let steer = (desired - self.npc_trains[i].leader_vel) * (steer_rate * dt);
                self.npc_trains[i].leader_vel += steer;
                if self.npc_trains[i].leader_vel.length() > speed {
                    self.npc_trains[i].leader_vel =
                        self.npc_trains[i].leader_vel.normalize() * speed;
                }
            }
            let margin = 80.0;
            let vel_step = self.npc_trains[i].leader_vel * dt;
            self.npc_trains[i].leader_pos += vel_step;
            self.npc_trains[i].leader_pos.x = self.npc_trains[i]
                .leader_pos
                .x
                .clamp(margin, self.world_width - margin);
            self.npc_trains[i].leader_pos.y = self.npc_trains[i]
                .leader_pos
                .y
                .clamp(margin, self.world_height - margin);

            // --- Path history for follower trailing ----------------------------------------
            let cur_pos = self.npc_trains[i].leader_pos;
            let last = self.npc_trains[i]
                .path_history
                .front()
                .copied()
                .unwrap_or(cur_pos);
            if cur_pos.distance_squared(last) > 36.0 {
                self.npc_trains[i].path_history.push_front(cur_pos);
                let max_len = self.npc_trains[i].follower_types.len() * 16 + 20;
                while self.npc_trains[i].path_history.len() > max_len {
                    self.npc_trains[i].path_history.pop_back();
                }
            }

            // Scale the King Crab with its conga line: more followers = bigger, scarier leader.
            // Each follower adds 0.09 scale above the tier floor, capped at 3.8 so even a
            // maxed-out elder doesn't become comically huge.
            {
                let n = self.npc_trains[i].follower_types.len() as f32;
                let base = self.npc_trains[i].base_scale;
                self.npc_trains[i].leader_scale = (base + n * 0.09).min(3.8);
            }

            // Compute target rumble volume from distance to player.
            let dist_to_player = self.npc_trains[i].leader_pos.distance(self.player_pos);
            self.npc_trains[i].target_vol = ((800.0 - dist_to_player) / 600.0).clamp(0.0, 1.0);

            // --- Pursuit: when the player has a train, bias steering toward their tail --------
            // The NPC behaves like a rival player: it wants to get BEHIND the player's chain to
            // thread through it for a steal, not charge the head where the player is watching.
            const PURSUIT_RANGE: f32 = 550.0;
            if self.chain_count >= 2
                && dist_to_player < PURSUIT_RANGE
                && self.npc_trains[i].idle_timer <= 0.0
            {
                // Use cached tail pos (updated once per frame in update_crabs) instead of
                // an O(n_crabs) scan here — saves ~160 iterations × 3 NPCs every frame.
                if let Some(tail_pos) = self.cached_tail_pos {
                    // Blend the wander target toward the tail — NPC steers into range naturally
                    let pursuit_blend =
                        ((PURSUIT_RANGE - dist_to_player) / PURSUIT_RANGE).clamp(0.0, 0.8);
                    self.npc_trains[i].target = self.npc_trains[i]
                        .target
                        .lerp(tail_pos, pursuit_blend * dt * 3.0);
                }
            }

            // --- Reverse-Snake chain splice steal --------------------------------------------
            // When the NPC leader draws close to a player chain link it splices the chain:
            // everything from that link to the tail detaches from the player and joins the NPC.
            // This is the core conga-ecology mechanic — rivals deliberately thread behind you.
            self.npc_trains[i].steal_cooldown = (self.npc_trains[i].steal_cooldown - dt).max(0.0);
            if self.npc_trains[i].steal_cooldown <= 0.0 && self.chain_count > 1 {
                const STEAL_RANGE: f32 = 58.0;
                let npc_pos = self.npc_trains[i].leader_pos;
                // Early-out: if the NPC is far from the player and the chain tail, no chain crab
                // can be within STEAL_RANGE. Use cached_tail_pos (the farthest link, already
                // computed by update_crabs) as a lower-bound proxy to avoid the O(n_crabs) scan.
                // The chain spans between player_pos and cached_tail_pos; if the NPC is more than
                // STEAL_RANGE beyond the tail it definitely can't reach any link.
                let chain_span = self
                    .cached_tail_pos
                    .map_or(0.0_f32, |t| t.distance(self.player_pos));
                let dist_to_chain = dist_to_player - chain_span;
                if dist_to_chain > STEAL_RANGE {
                    continue; // skip inner per-crab scan entirely this frame for this NPC
                }
                // Find the earliest (closest-to-head) link the NPC is within range of.
                // We splice there so a threading pass takes the maximum tail section.
                let splice_at = self
                    .crabs
                    .iter()
                    .filter(|c| c.caught && c.chain_index.map_or(false, |idx| idx > 0))
                    .filter(|c| npc_pos.distance(c.pos) < STEAL_RANGE)
                    .map(|c| c.chain_index.unwrap())
                    .min();

                if let Some(splice_idx) = splice_at {
                    // Collect the stolen types before mutating crabs
                    let mut stolen_types: Vec<CrabType> = Vec::new();
                    let mut stolen_count = 0usize;
                    for crab in self.crabs.iter_mut() {
                        if crab.caught {
                            if crab.chain_index.map_or(false, |idx| idx >= splice_idx) {
                                crab.caught = false;
                                crab.chain_index = None;
                                crab.fleeing = false;
                                crab.spooked_timer = 1.0;
                                // Cartoony startled hop: scale-pop then fly toward the NPC.
                                crab.join_pulse = 1.0;
                                let toward = (npc_pos - crab.pos).normalize_or_zero();
                                crab.vel = toward * 200.0;
                                crab.vel.y -= 90.0; // brief upward arc before snapping over
                                stolen_types.push(crab.crab_type);
                                stolen_count += 1;
                            }
                        }
                    }
                    if stolen_count > 0 {
                        self.chain_count = self.chain_count.saturating_sub(stolen_count);
                        self.npc_trains[i].follower_types.extend(stolen_types);
                        self.npc_trains[i].steal_cooldown = 2.2;
                        // Visual + audio feedback — this is the key threat moment
                        let npc_name = self.npc_trains[i].name.clone();
                        let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
                        self.floating_texts.spawn(
                            format!("{} stole {} crabs!", npc_name, stolen_count),
                            player_center - Vec2::new(110.0, 55.0),
                            30.0,
                            [0.96, 0.72, 0.16, 1.0],
                        );
                        self.screen_shake = self.screen_shake.max(10.0);
                        self.zoom_punch = self.zoom_punch.max(0.08);
                        self.groove = (self.groove - 0.15).max(0.0);
                        self.beat_streak = self.beat_streak.saturating_sub(2);
                        // Shockwave at the splice point so the cut reads on screen
                        if self.catch_shockwaves.len() < 48 {
                            self.catch_shockwaves
                                .push((npc_pos, 0.0, [0.96, 0.72, 0.16]));
                        }
                    }
                }
            }

            // --- Free crab collection --------------------------------------------------------
            // NPCs act like players: they pick up free crabs they wander past.
            self.npc_trains[i].catch_cooldown = (self.npc_trains[i].catch_cooldown - dt).max(0.0);
            if self.npc_trains[i].catch_cooldown <= 0.0 {
                const CATCH_RANGE: f32 = 52.0;
                let npc_pos = self.npc_trains[i].leader_pos;
                let caught = self.crabs.iter_mut().find(|c| {
                    !c.caught
                        && !c.is_boss()
                        && c.is_catchable()
                        && npc_pos.distance(c.pos) < CATCH_RANGE
                });
                if let Some(crab) = caught {
                    let ct = crab.crab_type;
                    // Teleport the crab far off-screen rather than marking it caught=true with
                    // no chain_index — that would corrupt rendering InstanceArray capacity checks.
                    crab.pos = Vec2::new(-9999.0, -9999.0);
                    crab.vel = Vec2::ZERO;
                    crab.fleeing = false;
                    self.npc_trains[i].follower_types.push(ct);
                    self.npc_trains[i].catch_cooldown = 0.7;
                }
            }
        }

        // Audio: use the loudest (nearest) NPC train for the rumble volume — store on [0].
        let max_vol = self
            .npc_trains
            .iter()
            .map(|t| t.target_vol)
            .fold(0.0_f32, f32::max);
        if !self.npc_trains.is_empty() {
            self.npc_trains[0].target_vol = max_vol;
        }

        // --- Continuous overlap separation: prevent player from phasing inside NPC leaders ----
        // Regardless of cooldown, push the player and NPC apart whenever they overlap so
        // you can't stand inside a King Crab — the clash is painful but crisp, not a merge.
        {
            let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            for npc in self.npc_trains.iter_mut() {
                let col_r = CRAB_SIZE * npc.leader_scale * 1.2 + PLAYER_SIZE * 0.5;
                let dist = npc.leader_pos.distance(player_center);
                if dist < col_r && dist > 0.1 {
                    // Positional correction: push both apart so they don't phase through each other
                    let overlap = col_r - dist;
                    let dir = (player_center - npc.leader_pos).normalize_or_zero();
                    self.player_pos += dir * overlap * 0.6;
                    npc.leader_pos -= dir * overlap * 0.4;
                    // Velocity damping so they slide off each other instead of jittering
                    let rel_vel = self.player_vel - npc.leader_vel;
                    let sep_speed = rel_vel.dot(dir);
                    if sep_speed < 0.0 {
                        self.player_vel -= dir * sep_speed * 0.8;
                        npc.leader_vel += dir * sep_speed * 0.8;
                    }
                }
            }
        }

        // --- Player-vs-NPC-leader collision: painful bounce, both sides take damage ----------
        // Deliberately touching a rival's leader is a desperate counter-attack move — you can
        // stun them and scatter some of their followers, but you take a hit too. Painful enough
        // to avoid unless intentional, spectacular enough to feel like a real clash.
        if self.king_splice_cooldown <= 0.0 {
            let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            let mut clash_npc: Option<usize> = None;
            for (ni, npc) in self.npc_trains.iter().enumerate() {
                let col_r = CRAB_SIZE * npc.leader_scale * 1.2 + PLAYER_SIZE * 0.5;
                if npc.leader_pos.distance(player_center) < col_r {
                    clash_npc = Some(ni);
                    break;
                }
            }
            if let Some(ni) = clash_npc {
                self.king_splice_cooldown = 2.0;
                // Both sides bounce apart — physics punch
                let away_from_npc =
                    (player_center - self.npc_trains[ni].leader_pos).normalize_or_zero();
                self.player_vel += away_from_npc * 380.0;
                self.npc_trains[ni].leader_vel += -away_from_npc * 280.0;
                // Camera + screen feedback — this should feel painful
                self.screen_shake = self.screen_shake.max(16.0);
                self.zoom_punch = self.zoom_punch.max(0.10);
                self.hitstop_timer = self.hitstop_timer.max(0.12);
                // Player loses tail crabs (1–2), NPC loses some followers — mutual damage
                let player_lose = 2.min(self.chain_count.saturating_sub(1));
                let mut released = 0;
                for crab in self.crabs.iter_mut().rev() {
                    if released >= player_lose {
                        break;
                    }
                    if crab.caught {
                        if let Some(idx) = crab.chain_index {
                            if idx > 0 {
                                crab.caught = false;
                                crab.chain_index = None;
                                crab.fleeing = true;
                                crab.spooked_timer = 2.5;
                                crab.join_pulse = 1.0; // startled pop
                                let away = (crab.pos - player_center).normalize_or_zero();
                                crab.vel = away * 250.0;
                                crab.vel.y -= 70.0; // hop upward before scattering out
                                if self.catch_shockwaves.len() < 48 {
                                    self.catch_shockwaves.push((crab.pos, 0.0, [1.0, 0.6, 0.2]));
                                }
                                released += 1;
                            }
                        }
                    }
                }
                self.chain_count = self.chain_count.saturating_sub(released);
                // NPC loses its last 1–2 followers — they scatter as free crabs (Sonic rings)
                let npc_pos = self.npc_trains[ni].leader_pos;
                let npc_lose = 2.min(self.npc_trains[ni].follower_types.len());
                for k in 0..npc_lose {
                    self.npc_trains[ni].follower_types.pop();
                    // Spawn a fleeing crab from around the NPC leader position
                    let scatter_angle =
                        k as f32 * std::f32::consts::PI + away_from_npc.y.atan2(away_from_npc.x);
                    let scatter_dir = Vec2::new(scatter_angle.cos(), scatter_angle.sin());
                    // Find a free slot in crabs if any normal crab can represent the ejected follower
                    // (simplified: just spawn a particle burst at the NPC position)
                    if self.catch_shockwaves.len() < 48 {
                        self.catch_shockwaves.push((
                            npc_pos + scatter_dir * 30.0,
                            0.0,
                            [0.96, 0.72, 0.16],
                        ));
                    }
                }
                // Groove penalty for taking a head-on hit — you should have dodged
                self.groove = (self.groove - 0.20).max(0.0);
                self.beat_streak = self.beat_streak.saturating_sub(1);
                let npc_name = self.npc_trains[ni].name.clone();
                self.floating_texts.spawn(
                    format!("CLASH with {}!", npc_name),
                    player_center - Vec2::new(80.0, 65.0),
                    32.0,
                    [1.0, 0.5, 0.15, 1.0],
                );
                self.particle_system
                    .spawn_milestone_fireworks(player_center, 8, &mut rand::rng());
            }
        }

        // --- NPC-vs-NPC leader collisions: they bounce off each other too --------------------
        for i in 0..self.npc_trains.len() {
            for j in (i + 1)..self.npc_trains.len() {
                let pi = self.npc_trains[i].leader_pos;
                let pj = self.npc_trains[j].leader_pos;
                let si = self.npc_trains[i].leader_scale;
                let sj = self.npc_trains[j].leader_scale;
                let col_r = CRAB_SIZE * (si + sj) * 0.8;
                if pi.distance(pj) < col_r {
                    let dir = (pi - pj).normalize_or_zero();
                    self.npc_trains[i].leader_vel += dir * 200.0;
                    self.npc_trains[j].leader_vel -= dir * 200.0;
                    // Each loses one follower (if they have any)
                    if !self.npc_trains[i].follower_types.is_empty() {
                        self.npc_trains[i].follower_types.pop();
                    }
                    if !self.npc_trains[j].follower_types.is_empty() {
                        self.npc_trains[j].follower_types.pop();
                    }
                }
            }
        }
    }

    fn draw_npc_conga_train(&self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        let t = self.time_elapsed;

        // Followers trail the leader using path_history. Each follower sits 14 history-steps
        // behind the previous one (history is sampled ~every 6px, so ~84px spacing between crabs).
        const STEPS: usize = 14;

        for npc in &self.npc_trains {
            // Draw followers back-to-front so the leader renders on top.
            for i in (0..npc.follower_types.len()).rev() {
                let hist_idx = (i + 1) * STEPS;
                let pos = match npc.path_history.get(hist_idx) {
                    Some(&p) => p,
                    None => continue,
                };
                let bob = (t * 5.5 + i as f32 * 0.85).sin() * 3.5;
                let crab_type = npc.follower_types[i];
                let fake = EnemyCrab {
                    pos,
                    vel: Vec2::ZERO,
                    speed: 0.0,
                    caught: true,
                    chain_index: Some(i),
                    scale: npc.leader_scale * 0.33, // followers scale with leader tier
                    spawn_time: 999.0,
                    crab_type,
                    spooked_timer: 0.0,
                    beat_phase_offset: i as f32 * 0.4,
                    join_pulse: 0.0,
                    fleeing: false,
                    facing_angle: 0.0,
                    in_flashlight: false,
                    startle_timer: 0.0,
                    charm_timer: 0.0,
                    answering_call: 0.0,
                    boss_health: 0.0,
                    boss_max_health: 0.0001,
                    enraged: false,
                    charge_state: BossCharge::Idle,
                    charge_cooldown: 0.0,
                    latch_timer: 0.0,
                    panic_amp: 1.0,
                    magnet_snared: 0.0,
                    magnet_lured: 0.0,
                    thief_lured: 0.0,
                    magnet_charged: 0.0,
                    slingshot_spent: 0.0,
                    stun_timer: 0.0,
                    host_swap_timer: 0.0,
                    surge_timer: 0.0,
                };
                let beat = (t * 4.0 + i as f32 * 0.5).sin().abs();
                draw_crab(
                    ctx,
                    canvas,
                    &fake,
                    pos + Vec2::new(0.0, -bob),
                    beat,
                    0.0,
                    bob.max(0.0),
                    0.0,
                    t,
                )?;
            }

            // King Crab leader — large, golden, unmistakeable.
            let leader_bob = (t * 4.2).sin() * 6.0;
            let facing = if npc.leader_vel.length_squared() > 4.0 {
                npc.leader_vel.y.atan2(npc.leader_vel.x)
            } else {
                0.0
            };
            let king = EnemyCrab {
                pos: npc.leader_pos,
                vel: npc.leader_vel,
                speed: 88.0,
                caught: false,
                chain_index: None,
                scale: npc.leader_scale,
                spawn_time: 999.0,
                crab_type: CrabType::Boss,
                spooked_timer: 0.0,
                beat_phase_offset: 0.0,
                join_pulse: 0.0,
                fleeing: false,
                facing_angle: facing,
                in_flashlight: false,
                startle_timer: 0.0,
                charm_timer: 0.0,
                answering_call: 0.0,
                boss_health: 0.0,
                boss_max_health: 0.0001,
                enraged: false,
                charge_state: BossCharge::Idle,
                charge_cooldown: 0.0,
                latch_timer: 0.0,
                panic_amp: 1.0,
                magnet_snared: 0.0,
                magnet_lured: 0.0,
                thief_lured: 0.0,
                magnet_charged: 0.0,
                slingshot_spent: 0.0,
                stun_timer: 0.0,
                host_swap_timer: 0.0,
                surge_timer: 0.0,
            };
            let king_beat = (t * 4.0).sin().abs();
            draw_crab(
                ctx,
                canvas,
                &king,
                npc.leader_pos + Vec2::new(0.0, -leader_bob),
                king_beat,
                0.0,
                leader_bob.max(0.0),
                facing,
                t,
            )?;

            // A gentle golden halo so the King reads as the leader from across the world.
            let dot = unit_circle(ctx)?;
            for ring in (0..3).rev() {
                let r = 40.0 + ring as f32 * 14.0;
                let a = 0.06 - ring as f32 * 0.015;
                canvas.draw(
                    dot,
                    DrawParam::default()
                        .dest(npc.leader_pos)
                        .scale(Vec2::splat(r))
                        .color(Color::new(1.0, 0.82, 0.2, a)),
                );
            }

            // Name plate floating above the King Crab — cached so glyphs aren't reshaped every frame.
            let name_w = NPC_NAME_CACHE.with(|c| -> GameResult<f32> {
                let mut cache = c.borrow_mut();
                let needs_rebuild = cache.as_ref().map_or(true, |(n, _, _)| n != &npc.name);
                if needs_rebuild {
                    let mut text = Text::new(npc.name.as_str());
                    text.set_scale(16.0);
                    let w = text.measure(ctx)?.x;
                    *cache = Some((npc.name.clone(), text, w));
                }
                Ok(cache.as_ref().unwrap().2)
            })?;
            NPC_NAME_CACHE.with(|c| {
                let cache = c.borrow();
                if let Some((_, text, _)) = cache.as_ref() {
                    let name_pos = npc.leader_pos - Vec2::new(name_w / 2.0, 55.0 + leader_bob);
                    // Drop shadow
                    canvas.draw(
                        text,
                        DrawParam::default()
                            .dest(name_pos + Vec2::splat(1.5))
                            .color(Color::from_rgba(0, 0, 0, 180)),
                    );
                    // Name in regal gold
                    canvas.draw(
                        text,
                        DrawParam::default()
                            .dest(name_pos)
                            .color(Color::new(0.96, 0.82, 0.3, 0.95)),
                    );
                }
            });
        }

        Ok(())
    }

    fn draw_scene(&mut self, ctx: &mut Context) -> GameResult {
        let width = self.width;
        let height = self.height;
        let mut canvas = Canvas::from_image(
            ctx,
            self.scene_image.clone(),
            Color::from_rgb(100, 200, 100),
        );
        let shake_ox = self.screen_shake_offset.x;
        let shake_oy = self.screen_shake_offset.y;
        // Zoom punch: shrink the visible world rect (magnify) around the player so they stay
        // pixel-locked while the world snaps in on a catch. z == 0 leaves the view untouched.
        let z = self.zoom_punch.clamp(0.0, 0.2);
        let focus = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
        // Camera scrolls across the larger-than-viewport world, following the player (clamped to
        // world bounds so no void edge shows). The zoom punch magnifies toward the focus point:
        // origin = camera + z·(focus − camera). At camera == 0 this collapses to the old focus·z.
        let cam = self.camera_origin;
        let vw = width * (1.0 - z);
        let vh = height * (1.0 - z);
        canvas.set_screen_coordinates(Rect::new(
            cam.x + z * (focus.x - cam.x) + shake_ox,
            cam.y + z * (focus.y - cam.y) + shake_oy,
            vw,
            vh,
        ));
        canvas.set_blend_mode(BlendMode::ALPHA);
        canvas.set_sampler(Sampler::nearest_clamp());

        if self.show_world_map {
            if let Some(map) = &self.world_map {
                self.sounds.action_music.pause();
                if self.sounds.outro_music.playing() {
                    self.sounds.outro_music.pause();
                }
                if !self.sounds.intro_music.playing() {
                    self.sounds.intro_music.play(ctx)?;
                }
                draw_world_map(ctx, &mut canvas, map, width, height, self.menu_time)?;
                canvas.finish(ctx)?;
                return Ok(());
            }
        }

        if self.show_instructions {
            if self.sounds.outro_music.playing() {
                self.sounds.outro_music.pause();
            }
            if self.sounds.action_music.playing() {
                self.sounds.action_music.pause();
            }
            if !self.sounds.intro_music.playing() {
                self.sounds.intro_music.play(ctx)?;
            }
            self.draw_instructions_screen(ctx, &mut canvas, width, height)?;
            canvas.finish(ctx)?;
            return Ok(());
        } else if self.pending_upgrade {
            self.sounds.action_music.pause();
            // Reset to screen space (the canvas may still hold the camera-offset world rect from
            // the set_screen_coordinates call above). Upgrade cards are laid out in [0, width] x
            // [0, height] so they need a clean viewport origin.
            canvas.set_screen_coordinates(Rect::new(0.0, 0.0, width, height));
            self.draw_upgrade_screen(ctx, &mut canvas)?;
            canvas.finish(ctx)?;
            return Ok(());
        } else if self.game_over {
            self.sounds.action_music.pause();
            if !self.sounds.outro_music.playing() {
                self.sounds.outro_music.play(ctx)?;
            }
            self.draw_game_over_screen(ctx, &mut canvas)?;
        } else {
            if self.sounds.intro_music.playing() {
                self.sounds.intro_music.pause();
            }
            if !self.sounds.action_music.playing() {
                self.sounds.action_music.play(ctx)?;
            } else {
                self.sounds.action_music.resume();
            }
            self.draw_game(ctx, &mut canvas, width, height)?;
        }
        canvas.finish(ctx)?;
        Ok(())
    }
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

        if self.show_instructions || self.show_world_map || self.game_over || self.pending_upgrade {
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
            // In bot mode, time_elapsed must advance and bot events must fire even while
            // the menu is showing — e.g. TapKey(Space) at t=0.5 dismisses the title screen.
            // We run a stripped-down bot tick here (advance clock, fire due events, handle
            // done/failed), then fall through to the normal return.
            if self.bot.is_some() {
                self.time_elapsed += mdt.min(0.1) * self.time_scale;
                // Fire events (same logic as the in-game bot tick below).
                use crate::bot::{BotAction, BotAssert};
                loop {
                    let cursor = self.bot.as_ref().unwrap().cursor;
                    if cursor >= self.bot.as_ref().unwrap().script.len() {
                        break;
                    }
                    let ev = self.bot.as_ref().unwrap().script[cursor].clone();
                    if ev.at > self.time_elapsed {
                        break;
                    }
                    self.bot.as_mut().unwrap().cursor += 1;
                    match ev.action {
                        BotAction::TapKey(k) => {
                            self.bot.as_mut().unwrap().keys_held.insert(k);
                            self.bot.as_mut().unwrap().tap_release_queue.push(k);
                            // Simulate the key press directly.
                            crate::controls::handle_key_down_event(self, ctx, Some(k));
                        }
                        BotAction::Log(msg) => println!("[BOT t={:.1}] {}", self.time_elapsed, msg),
                        _ => {} // other actions handled in the in-game tick
                    }
                }
                if let Some(bot) = self.bot.as_ref() {
                    if bot.done {
                        std::process::exit(if bot.failed.is_some() { 1 } else { 0 });
                    }
                }
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
        self.stomp_dancer_hits_buf.clear();
        self.lasso_thief_hits_buf.clear();
        self.lasso_magnet_hits_buf.clear();
        self.lasso_shell_deflect_hits_buf.clear();
        self.magnet_cluster_hits_buf.clear();
        self.stomp_armored_hits_buf.clear();
        self.whistle_golden_hits_buf.clear();
        self.whistle_dancer_hits_buf.clear();

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
            use crate::bot::{BotAction, BotAssert};

            // Release tap keys from previous frame.
            let taps: Vec<_> = self
                .bot
                .as_mut()
                .unwrap()
                .tap_release_queue
                .drain(..)
                .collect();
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
                    BotAction::Log(msg) => {
                        println!("[BOT t={:.1}] {}", self.time_elapsed, msg);
                    }
                    BotAction::Assert(check) => {
                        let ok = match &check {
                            BotAssert::GameNotOver => !self.game_over,
                            BotAssert::ChainAtLeast(n) => self.chain_count >= *n,
                            BotAssert::ScoreAtLeast(n) => self.score >= *n,
                            BotAssert::ShowWorldMap => self.show_world_map,
                            BotAssert::TutorialActive => self.tutorial.is_some(),
                            BotAssert::TutorialDone => {
                                self.tutorial.is_none() && self.show_world_map
                            }
                            BotAssert::InGame => {
                                !self.show_instructions && !self.game_over && !self.show_world_map
                            }
                        };
                        if !ok {
                            let msg =
                                format!("ASSERT FAILED at t={:.1}: {:?}", self.time_elapsed, check);
                            println!("FAIL: {}", msg);
                            self.bot.as_mut().unwrap().failed = Some(msg);
                            self.bot.as_mut().unwrap().done = true;
                        }
                    }
                }
            }

            // Check completion / time limit.
            {
                let bot = self.bot.as_mut().unwrap();
                if bot.cursor >= bot.script.len() && !bot.done {
                    println!("PASS: script complete at t={:.1}", self.time_elapsed);
                    bot.done = true;
                }
                if self.time_elapsed >= bot.time_limit && !bot.done {
                    println!("FAIL: time limit {:.1}s reached", bot.time_limit);
                    bot.failed = Some("time limit exceeded".into());
                    bot.done = true;
                }
                if bot.done {
                    if bot.failed.is_some() {
                        std::process::exit(1);
                    } else {
                        std::process::exit(0);
                    }
                }
            }
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

        // Beat timer — interval speeds up with the intensity stage (see beat_interval).
        self.beat_timer -= dt;
        if self.beat_timer <= 0.0 {
            self.beat_timer += self.beat_interval;
            self.beat_intensity = 1.0;
            self.beat_count = self.beat_count.wrapping_add(1);
            let downbeat = self.beat_count % 4 == 0;
            // Visceral beat: thump a synthesised kick drum on every beat so the tempo is *felt*,
            // not just seen. The heavier, lower voice lands on the downbeat so the bar has a clear
            // accent structure. This block only runs during live gameplay (the update guard returns
            // early on menu/upgrade/game-over screens), so the kick never thumps through menus.
            self.beat_synth.play_kick(ctx, downbeat);
            // Snare: fades in on the backbeat (beats 2 & 4) while a boss is alive, raising the
            // stakes audibly as the fight escalates. Fades back out once the boss is caught.
            let boss_present = self.crabs.iter().any(|c| c.is_boss() && !c.caught);
            self.beat_synth.update_snare_volume(boss_present);
            self.beat_synth.play_snare(ctx, self.beat_count);
            // On-beat catch bloom: every beat the train's catch window blooms wide, then settles back
            // before the next hit (decayed in update_crabs). The downbeat blooms hardest so the "1"
            // is the widest scoop of the bar — a groove-savvy player learns to cross a drifting crab
            // exactly on the beat to hoover it in, while an off-beat pass just misses. This reshapes
            // ordinary catching around the bar without adding a new key to press.
            self.beat_catch_bloom = if downbeat { 30.0 } else { 20.0 };
            // Downbeat herd pulse: on the "1" of the bar, nudge the whole free herd toward the
            // player so the beat itself becomes a routing tool. Light it up only on the downbeat so
            // it reads as a rhythmic thump, not a constant tug; the impulse is applied per-crab in
            // update_crabs and decays over the frames after. Captured center drives the visual ring.
            if downbeat {
                self.downbeat_pull = 1.0;
                let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
                self.downbeat_pull_center = center;
                // Count the herd this downbeat is actually about to sweep — free, un-spooked crabs
                // inside the same 300px radius the per-crab pull uses — so the ring's flare reflects
                // real routing payoff, not just that a beat happened. Normalized against a "full
                // scoop" of ~10 crabs; standing in a fat loose herd on the "1" flares the ring gold.
                let swept = self
                    .crabs
                    .iter()
                    .filter(|c| {
                        !c.caught
                            && !c.is_boss()
                            && c.startle_timer <= 0.0
                            && c.charm_timer <= 0.0
                            && c.magnet_snared <= 0.0
                            && c.pos.distance_squared(center) < 300.0 * 300.0
                    })
                    .count();
                self.downbeat_pull_haul = (swept as f32 / 10.0).clamp(0.0, 1.0);
            }
            // Drum Roll: if T is being held as this beat fires, bank a roll hit (the charge). The
            // beat handler runs at most once per beat, so a held key naturally counts exactly one
            // hit per beat. A hit kicks a tick of feedback (beat flash + a bump of groove) so each
            // roll lands audibly/visibly, building tension toward the release blast. The held flag
            // is set by the update poll before update_crabs, so it's current for this beat.
            if self.drum_roll_held {
                self.drum_roll_hits = (self.drum_roll_hits + 1).min(DRUM_ROLL_MAX);
                self.on_beat_flash = (self.on_beat_flash + 0.2).min(0.7);
                self.groove = (self.groove + 0.05).min(1.0);
                let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
                self.floating_texts.spawn(
                    "ROLL!".to_string(),
                    center - Vec2::new(28.0, 96.0),
                    22.0 + self.drum_roll_hits as f32 * 3.0,
                    [1.0, 0.8, 0.4, 1.0],
                );
            }
            // Reef DJ call-and-response: on every downbeat while the rhythm boss is on the field,
            // it CALLS a fresh phrase for the coming bar — a random subset of the four beats that
            // are "hot" (its shell is only vulnerable on those). Rolled once per bar, always with
            // at least one hot beat and never all four, so there's a pattern to read and echo back
            // rather than a constant open window. The downbeat is always hot so the "1" anchors the
            // phrase and reads as the boss's call.
            if downbeat && self.reef_active {
                let bar = self.beat_count / 4;
                if bar != self.reef_phrase_bar {
                    self.reef_phrase_bar = bar;
                    let mut rng = rand::rng();
                    let mut phrase = [false; 4];
                    phrase[0] = true; // the "1" always calls, anchoring the bar
                    for slot in phrase.iter_mut().skip(1) {
                        *slot = rng.random_bool(0.4);
                    }
                    self.reef_phrase = phrase;
                }
            }
            // Groove Call response: while a call is live, the herd LUNGES toward the player on each
            // beat and drifts between — kick the surge envelope here so the field-wide pull (applied
            // in update_crabs) pulses to the bar. Bars of response are spent one per downbeat, so a
            // clean 2-bar call unfolds over eight beats before the herd relaxes. The downbeat surge
            // lands hardest so the "1" is the big group lunge — the watchable, on-the-beat gather.
            if self.groove_call_bars > 0.0 {
                self.groove_call_surge = if downbeat { 1.0 } else { 0.7 };
                self.groove_call_pulse = if downbeat { 1.0 } else { 0.7 };
                // Answer streaks: on each beat of a live call, fling comet trails from free crabs
                // toward the player so the herd-flood reads as an on-the-beat lunge, not just drift.
                // The downbeat throws the big group streak (whole field), the between-beats a lighter
                // one — the "1" is visibly the largest gather. Cyan-tinted to match the call ring.
                let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
                let cap = if downbeat { 40 } else { 22 };
                // Nearer, more-susceptible crabs streak more strongly; scale count by call quality.
                let want = ((cap as f32) * self.groove_call_strength.min(1.5)).round() as usize;
                let start = if downbeat { -0.45 } else { -0.2 }; // downbeat streak reads a touch longer
                let mut spawned = 0usize;
                for crab in self.crabs.iter() {
                    if spawned >= want || self.call_streaks.len() >= 56 {
                        break;
                    }
                    if crab.caught
                        || crab.is_boss()
                        || crab.crab_type.whistle_pull() <= 0.0
                        || crab.is_latched()
                    {
                        continue;
                    }
                    let d = center - crab.pos;
                    let dist = d.length();
                    // Pull radius scales with groove: more groove = wider reach (max 500px).
                    let call_reach = 280.0 + self.groove * 220.0;
                    if dist < 40.0 || dist > call_reach {
                        continue; // skip crabs on top of the player or too far to read as answering
                    }
                    // A short streak from the crab pointing at the player — a fixed lead so the tail
                    // shows the answering direction without teleporting the crab.
                    let head = crab.pos + d.normalize_or_zero() * dist.min(120.0);
                    // Cyan call tint, brightened by how eagerly this archetype answers.
                    let eager = crab.crab_type.whistle_pull().min(1.0);
                    let color = [0.35 + 0.25 * eager, 0.9, 1.0];
                    self.call_streaks.push((crab.pos, head, start, color));
                    spawned += 1;
                }
                if downbeat {
                    self.groove_call_bars -= 1.0;
                    // A small groove tick each bar the call keeps working, so leaning on the beat to
                    // route the herd is itself rewarded like the other rhythm verbs.
                    self.groove = (self.groove + 0.04).min(1.0);
                    // Call fully spent this bar — reset the echo phrase so the next call starts fresh.
                    if self.groove_call_bars <= 0.0 {
                        self.groove_call_echo = 0;
                    }
                }
            }
            // The "1" of the bar lands harder than the three beats between it. Kick the accent so
            // the beat-stepping conga train stomps forward as one on the downbeat (see the step
            // code in update_crabs, which scales its hop by bar_accent), and give a fresh unified
            // squash-pop that ripples down the line so the whole train visibly lands the one.
            if downbeat {
                self.bar_accent = 1.0;
                // Restart the join squash-pop on every caught crab, staggered by chain index so
                // the pop rolls head-to-tail — the same ripple used when a crab joins, reused here
                // as a musical "bar landed" bounce. Cheap: just sets a decaying timer per crab.
                let mut ci = 0.0_f32;
                for crab in self.crabs.iter_mut().filter(|c| c.caught) {
                    crab.join_pulse = (1.0 - ci * 0.04).max(0.4);
                    ci += 1.0;
                }
            }
            // King Crab finale: the cracked floor GEYSERS on the beat. Kick the eruption pulse so
            // every open fissure spouts molten in time with the music — its danger swells on the
            // hit and recedes in the gap, turning a static pit into a rhythmic hazard the player
            // times crossings against. A tiny extra flare on the downbeat so it groups by the bar.
            if !self.boss_fissures.is_empty() {
                self.boss_fissure_erupt = if downbeat { 1.0 } else { 0.85 };
                self.screen_shake = self.screen_shake.max(if downbeat { 8.0 } else { 5.0 });
                // Spit a few molten sparks up out of each pit so the geyser reads as real debris,
                // not just a glow — capped by the particle system's own budget.
                for &(c, r, age) in self.boss_fissures.iter() {
                    if age > 0.6 {
                        self.particle_system
                            .spawn_fissure_geyser(c, r, &mut rand::rng());
                    }
                }
            }
            // Every 4th beat, auto-fire beat wave when score >= 20
            if downbeat && self.score >= 20 && !self.beat_wave_active {
                self.beat_wave_active = true;
                self.beat_wave_radius = 0.0;
            }
            // Bar-quantized spawn: an armed wave lands exactly here, on the downbeat, so a fresh
            // herd always arrives in time with the music instead of at an arbitrary tick.
            if downbeat && self.wave_armed {
                self.wave_armed = false;
                self.wave_telegraph = 0.0;
                let was_frenzy = self.frenzy_wave;
                self.advance_pattern();
                // Punch the downbeat that births a wave so the arrival reads as a musical hit.
                // A frenzy drop punches noticeably harder — bigger flash, screen shake, and a
                // banner — so the staged spike lands as a genuine event, not just more crabs.
                if was_frenzy {
                    self.beat_intensity = 2.0;
                    self.on_beat_flash = self.on_beat_flash.max(0.75);
                    self.frenzy_banner_timer = 1.6;
                    self.screen_shake = self.screen_shake.max(11.0);
                    let kick = rand::rng().random_range(0.0_f32..std::f32::consts::TAU);
                    self.screen_shake_vel = Vec2::new(kick.cos(), kick.sin()) * 11.0 * 60.0;
                    // upgrade.ogg removed — tiresome and crackly; new sound TBD
                } else {
                    self.beat_intensity = (self.beat_intensity + 0.6).min(2.0);
                    self.on_beat_flash = self.on_beat_flash.max(0.4);
                }
            }
            // Collect caught-crab positions for the beat-pulse sparkle rings just below: both
            // used to run their own separate `.filter(|c| c.caught)` pass over self.crabs (two
            // counts + a fresh Vec::collect() every single beat), so fold them into one pass
            // that reuses the persistent chain_positions_buf (already used later this frame by
            // catch_by_chain, and not read in between) instead of allocating a new Vec.
            self.chain_positions_buf.clear();
            self.chain_positions_buf
                .extend(self.crabs.iter().filter(|c| c.caught).map(|c| c.pos));
            let chain_len = self.chain_positions_buf.len();
            // Beat-pulse sparkle rings from all caught crabs — brighter on the bar downbeat so
            // the "1" of the bar pops harder than the beats between it.
            let pulse_strength = if downbeat { 1.5 } else { 1.0 };
            self.particle_system.spawn_beat_pulse(
                &self.chain_positions_buf,
                pulse_strength,
                chain_len,
                &mut rand::rng(),
            );
            // Spawn ghost rings at each chain crab position. Unlike catch_shockwaves (capped at
            // 48) and fear_rings (capped at 32), this loop had no ceiling — a long conga train
            // (chain_count grows unbounded over a run, see MAX_PARTICLES's comment) would push
            // one ring per caught crab every single beat, each drawing two more mesh draws in
            // draw_chain_rings. Cap it the same way the sibling effect buffers are capped: once
            // the live count hits the ceiling, stop adding for this beat rather than growing
            // without bound. Only affects trains long enough to have hit the cap already.
            const MAX_CHAIN_RINGS: usize = 64;
            for crab in self.crabs.iter().filter(|c| c.caught) {
                if self.chain_rings.len() >= MAX_CHAIN_RINGS {
                    break;
                }
                let color = crab.crab_color();
                self.chain_rings.push((crab.pos, 0.0, color));
            }
            // Emergent beat-startle chain reaction: panic ripples crab-to-crab on the pulse.
            self.beat_startle_contagion();

            // Dancer crabs hop on the beat. Between beats they barely drift (their speed_range is
            // low), so their real motion is this quantized leap — making them a rhythm-reading
            // catch: the beat that just fired is exactly when they bolt, so you grab them during
            // the freeze, not mid-leap. Close ones hop away from the player (a rhythmic flee);
            // distant ones keep their heading, wandering in beat-timed skips.
            const DANCER_HOP: f32 = 74.0;
            let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            // Where each *fleeing* (not answering) Dancer landed this beat. A jittery Dancer
            // leaping away from the player is a startle source of its own — its on-beat hop
            // spooks the calm crabs around it (see the ripple pass below). Reuse the scratch
            // buffer rather than allocating a Vec every beat.
            let mut dancer_hops = std::mem::take(&mut self.dancer_hop_scratch);
            dancer_hops.clear();
            for crab in self.crabs.iter_mut() {
                if crab.caught || !crab.is_dancer() {
                    continue;
                }
                let dist = player_center.distance(crab.pos);
                // An answering Dancer that's already in arm's reach holds still (its answer is spent)
                // rather than hopping the default fallback direction and skittering off.
                if crab.answering_call > 0.0 && dist < 90.0 {
                    crab.answering_call = 0.0;
                    crab.join_pulse = 1.0;
                    continue;
                }
                let dir = if crab.answering_call > 0.0 {
                    // Answering the player's Call: hop TOWARD the player on the beat.
                    (player_center - crab.pos).normalize_or_zero()
                } else if dist < 240.0 {
                    // Rhythmic flee: leap away from the player.
                    (crab.pos - player_center).normalize_or_zero()
                } else {
                    // Wander: keep heading, or fall back to current facing if idle.
                    let v = crab.vel.normalize_or_zero();
                    if v == Vec2::ZERO {
                        Vec2::new(crab.facing_angle.cos(), crab.facing_angle.sin())
                    } else {
                        v
                    }
                };
                let dir = if dir == Vec2::ZERO {
                    Vec2::new(0.0, -1.0)
                } else {
                    dir
                };
                crab.pos += dir * DANCER_HOP;
                crab.pos.x = crab.pos.x.clamp(0.0, self.world_width - crab.scale);
                crab.pos.y = crab.pos.y.clamp(0.0, self.world_height - crab.scale);
                crab.vel = dir; // face the hop; unit vel so the drift branch stays gentle
                crab.join_pulse = 1.0; // reuse the join squash-pop as a little "landed" bounce
                // A Dancer bolting away from the player becomes a fear source; note where it
                // landed so the ripple pass below can spook nearby calm crabs. Answering Dancers
                // (hopping toward the player, charmed) don't scare anyone — only fleeing ones do.
                if crab.answering_call <= 0.0 && dist < 240.0 {
                    dancer_hops.push(crab.pos);
                }
            }

            // On-beat herd stampede: on the DOWNBEAT (the bar's "1") the whole loose herd lurches
            // forward along its own heading, then coasts through the three off-beats — so *where a
            // free crab will be* becomes a rhythm read. A groove-savvy player reads the surge and
            // slides into the herd's landing spot on the bar rather than chasing crabs flatly; the
            // beat reshapes routing across the whole field, not just around the player. Only the
            // downbeat surges (the off-beats stay a quiet coast) so the "1" reads as the herd's step,
            // matching the heavier downbeat kick drum and bar accent. We only ARM the surge here
            // (kick surge_timer); update_crabs spends it as an extra positional shove that decays
            // over the beat, so the motion eases out instead of teleporting. Excludes anything that
            // already has its own on-beat motion or a reason to hold still: Dancers (their own hop
            // above), bosses, spooked/startled/charmed/answering crabs, snared/lured crabs under a
            // Magnet, and Hermits (their own host-swap hop) — the surge is the *calm* herd's beat-step.
            if downbeat {
                for crab in self.crabs.iter_mut() {
                    if crab.caught
                        || crab.is_dancer()
                        || crab.is_boss()
                        || crab.spooked_timer > 0.0
                        || crab.startle_timer > 0.0
                        || crab.charm_timer > 0.0
                        || crab.answering_call > 0.0
                        || crab.magnet_snared > 0.0
                        || crab.thief_lured > 0.0
                        || crab.is_hermit()
                    {
                        continue;
                    }
                    crab.surge_timer = 1.0;
                }
            }

            // Emergent interaction: a fleeing Dancer's on-beat hop ripples out into five separate
            // effects depending on what it lands near — startling a calm crab, jolting a latched
            // Thief loose, staggering a bolting Golden, chipping an Armored crab's shell, or kicking
            // a roaming Magnet into a pull surge. These used to be five independent
            // `self.crabs.iter_mut()` passes, each rebuilding the same grid-lookup closure and
            // re-scanning the whole herd — on a long train that's 5x redundant O(n) work every
            // single beat. Since the five target predicates (calm non-Dancer / free latched Thief /
            // free Golden / free Armored-with-shell / free Magnet) are mutually exclusive per crab,
            // fold them into one pass over self.crabs that dispatches by crab type, sharing one grid
            // lookup and one nearest/hit search per crab instead of up to five.
            if !dancer_hops.is_empty() {
                const DANCER_STARTLE_RADIUS: f32 = 78.0;
                const MAX_DANCER_STARTLES: usize = 5;
                const DANCER_JOLT_RADIUS_SQ: f32 = 70.0 * 70.0; // Thief
                const DANCER_TRIP_RADIUS_SQ: f32 = 68.0 * 68.0; // Golden
                const DANCER_CHIP_RADIUS_SQ: f32 = 66.0 * 66.0; // Armored
                const DANCER_KICK_RADIUS_SQ: f32 = 72.0 * 72.0; // Magnet

                // Bucket the (usually small, but unbounded as Dancer count grows) set of hop
                // sources so each crab only tests nearby ones instead of every Dancer that hopped
                // this beat. Built once at the widest radius (the startle ripple's) and reused by
                // all five checks below, each with its own (smaller) trigger radius.
                let cell_size = DANCER_STARTLE_RADIUS.max(1.0);
                let cell_of = |p: Vec2| -> (i32, i32) {
                    (
                        (p.x / cell_size).floor() as i32,
                        (p.y / cell_size).floor() as i32,
                    )
                };
                // Same unbounded-key fix as contagion_grid_buf/armored_anchor_grid_buf: a plain
                // per-bucket clear left one entry per grid cell ever visited by a hopping Dancer,
                // which only grows over a session as the herd roams the whole level. A full
                // clear() keeps the map's allocated capacity (still avoids a realloc most beats)
                // but bounds the key count to "cells touched this beat".
                self.dancer_startle_grid_buf.clear();
                for (i, &pos) in dancer_hops.iter().enumerate() {
                    self.dancer_startle_grid_buf
                        .entry(cell_of(pos))
                        .or_default()
                        .push(i);
                }

                let mut spooked = std::mem::take(&mut self.dancer_spooked_buf);
                let mut jolted = std::mem::take(&mut self.dancer_jolt_buf);
                let mut tripped = std::mem::take(&mut self.dancer_trip_buf);
                let mut chipped = std::mem::take(&mut self.dancer_chip_buf);
                let mut kicked = std::mem::take(&mut self.dancer_kick_buf);
                spooked.clear();
                jolted.clear();
                tripped.clear();
                chipped.clear();
                kicked.clear();

                for crab in self.crabs.iter_mut() {
                    if crab.caught {
                        continue;
                    }
                    if crab.is_thief() {
                        if crab.latch_timer <= 0.0 {
                            continue;
                        }
                        let (cx, cy) = cell_of(crab.pos);
                        let mut hop_src: Option<Vec2> = None;
                        'search_thief: for dx in -1..=1 {
                            for dy in -1..=1 {
                                if let Some(candidates) =
                                    self.dancer_startle_grid_buf.get(&(cx + dx, cy + dy))
                                {
                                    for &i in candidates {
                                        let hp = dancer_hops[i];
                                        if crab.pos.distance_squared(hp) < DANCER_JOLT_RADIUS_SQ {
                                            hop_src = Some(hp);
                                            break 'search_thief;
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(src) = hop_src {
                            // Break the clamp and fling the Thief away from the Dancer that thumped
                            // it, matching how the Magnet-pry sends it off toward the lodestone.
                            crab.latch_timer = 0.0;
                            let dir = (crab.pos - src).normalize_or_zero();
                            let dir = if dir == Vec2::ZERO {
                                Vec2::new(0.0, -1.0)
                            } else {
                                dir
                            };
                            crab.vel = dir * crab.crab_type.speed_range().end * 1.5;
                            crab.speed = 1.0;
                            crab.fleeing = false;
                            crab.startle_timer = 0.0;
                            jolted.push(crab.pos);
                        }
                    } else if crab.is_golden() {
                        if crab.magnet_snared > 0.0 {
                            continue;
                        }
                        let (cx, cy) = cell_of(crab.pos);
                        let mut hop_src: Option<Vec2> = None;
                        'search_golden: for dx in -1..=1 {
                            for dy in -1..=1 {
                                if let Some(candidates) =
                                    self.dancer_startle_grid_buf.get(&(cx + dx, cy + dy))
                                {
                                    for &i in candidates {
                                        let hp = dancer_hops[i];
                                        if crab.pos.distance_squared(hp) < DANCER_TRIP_RADIUS_SQ {
                                            hop_src = Some(hp);
                                            break 'search_golden;
                                        }
                                    }
                                }
                            }
                        }
                        if hop_src.is_some() {
                            // Trip it: kill the bolt so it wobbles in place, opening a short catch
                            // window. No magnet_snared flag (keeps the orange snare visual for the
                            // Magnet path); the stalled prize plus the pink burst tell the story.
                            crab.vel *= 0.15;
                            crab.speed = 1.0;
                            crab.fleeing = false;
                            crab.startle_timer = 0.0;
                            crab.join_pulse = 1.0;
                            tripped.push(crab.pos);
                        }
                    } else if crab.is_armored() || crab.is_shelled_hermit() {
                        // A Dancer's on-beat hop chips a hard shell — Armored or Hermit alike. For the
                        // Hermit this is one of its three intended cracks (the beam can't touch it), so
                        // herding a hopping Dancer next to a hunkered Hermit is a real way to pop it.
                        if crab.boss_health <= 0.0 {
                            continue;
                        }
                        let (cx, cy) = cell_of(crab.pos);
                        let mut hit = false;
                        'search_armored: for dx in -1..=1 {
                            for dy in -1..=1 {
                                if let Some(candidates) =
                                    self.dancer_startle_grid_buf.get(&(cx + dx, cy + dy))
                                {
                                    for &i in candidates {
                                        if crab.pos.distance_squared(dancer_hops[i])
                                            < DANCER_CHIP_RADIUS_SQ
                                        {
                                            hit = true;
                                            break 'search_armored;
                                        }
                                    }
                                }
                            }
                        }
                        if hit {
                            crab.boss_health = (crab.boss_health - 1.0).max(0.0);
                            crab.join_pulse = 1.0;
                            crab.fleeing = false;
                            crab.spooked_timer = crab.spooked_timer.max(0.3);
                            chipped.push((crab.pos, crab.boss_health <= 0.0, crab.is_hermit()));
                        }
                    } else if crab.is_magnet() {
                        if crab.in_flashlight || crab.magnet_charged > 0.0 {
                            continue;
                        }
                        let (cx, cy) = cell_of(crab.pos);
                        let mut hit = false;
                        'search_magnet: for dx in -1..=1 {
                            for dy in -1..=1 {
                                if let Some(candidates) =
                                    self.dancer_startle_grid_buf.get(&(cx + dx, cy + dy))
                                {
                                    for &i in candidates {
                                        if crab.pos.distance_squared(dancer_hops[i])
                                            < DANCER_KICK_RADIUS_SQ
                                        {
                                            hit = true;
                                            break 'search_magnet;
                                        }
                                    }
                                }
                            }
                        }
                        if hit {
                            crab.magnet_charged = 0.45;
                            crab.join_pulse = 1.0;
                            kicked.push(crab.pos);
                        }
                    } else if crab.is_boss()
                        || crab.is_dancer()
                        || crab.in_flashlight
                        || crab.fleeing
                        || crab.startle_timer > 0.0
                        || crab.charm_timer > 0.0
                    {
                        continue;
                    } else {
                        if spooked.len() >= MAX_DANCER_STARTLES {
                            continue;
                        }
                        let (cx, cy) = cell_of(crab.pos);
                        let mut nearest: Option<(f32, Vec2)> = None;
                        for dx in -1..=1 {
                            for dy in -1..=1 {
                                if let Some(candidates) =
                                    self.dancer_startle_grid_buf.get(&(cx + dx, cy + dy))
                                {
                                    for &i in candidates {
                                        let src = dancer_hops[i];
                                        let d = src.distance(crab.pos);
                                        if d < DANCER_STARTLE_RADIUS
                                            && nearest.map_or(true, |(nd, _)| d < nd)
                                        {
                                            nearest = Some((d, src));
                                        }
                                    }
                                }
                            }
                        }
                        if let Some((d, src)) = nearest {
                            let outward = (crab.pos - src).normalize_or_zero();
                            let outward = if outward == Vec2::ZERO {
                                Vec2::new(0.0, -1.0)
                            } else {
                                outward
                            };
                            let prox = 1.0 - d / DANCER_STARTLE_RADIUS;
                            let kick = crab.crab_type.speed_range().end * (1.0 + prox * 0.7);
                            crab.vel = outward * kick;
                            crab.speed = 1.0;
                            crab.startle_timer = 0.4;
                            spooked.push(crab.pos);
                        }
                    }
                }

                for &pos in &spooked {
                    if self.fear_rings.len() < 32 {
                        self.fear_rings.push((pos, 0.0));
                    }
                    self.floating_texts.spawn(
                        "!".to_string(),
                        pos - Vec2::new(0.0, 24.0),
                        20.0,
                        [1.0, 0.55, 0.9, 1.0], // hot Dancer-pink "!" so the source reads at a glance
                    );
                }
                for &pos in jolted.iter() {
                    if self.fear_rings.len() < 32 {
                        self.fear_rings.push((pos, 0.0));
                    }
                    self.floating_texts.spawn(
                        "SHAKEN LOOSE!".to_string(),
                        pos - Vec2::new(58.0, 30.0),
                        24.0,
                        [1.0, 0.55, 0.9, 1.0], // hot Dancer-pink so the "a Dancer did this" story reads
                    );
                    self.spawn_catch_shockwave(pos, [1.0, 0.45, 0.85]);
                }
                for &pos in tripped.iter() {
                    if self.fear_rings.len() < 32 {
                        self.fear_rings.push((pos, 0.0));
                    }
                    self.floating_texts.spawn(
                        "STAGGERED!".to_string(),
                        pos - Vec2::new(52.0, 30.0),
                        24.0,
                        [1.0, 0.55, 0.9, 1.0], // hot Dancer-pink so the "a Dancer tripped it" story reads
                    );
                    self.spawn_catch_shockwave(pos, [1.0, 0.75, 0.3]); // gold burst — it's the prize wobbling
                }
                for &(pos, broke, was_hermit) in chipped.iter() {
                    // A Dancer hop that pops a Hermit clean open earns the signature copper Hermit-pop
                    // instead of the generic blue crack — it's a pure archetype-web crack (the beam
                    // can't do it), so the emergent play reads as the win it is.
                    if broke && was_hermit {
                        self.spawn_hermit_pop(pos);
                        continue;
                    }
                    let (label, burst) = if broke {
                        ("SHELL CRACKED!", [0.7, 0.8, 0.95]) // fully open — matches the Stomp crack cue
                    } else {
                        ("CHIPPED!", [0.62, 0.68, 0.78]) // a chink knocked loose, more shell to go
                    };
                    self.floating_texts.spawn(
                        label.to_string(),
                        pos - Vec2::new(58.0, 32.0),
                        24.0,
                        [1.0, 0.55, 0.9, 1.0], // hot Dancer-pink so the "a Dancer did this" story reads
                    );
                    self.spawn_catch_shockwave(pos, burst);
                }
                for &pos in kicked.iter() {
                    if self.fear_rings.len() < 32 {
                        self.fear_rings.push((pos, 0.0));
                    }
                    self.floating_texts.spawn(
                        "MAGNET SURGE!".to_string(),
                        pos - Vec2::new(58.0, 32.0),
                        24.0,
                        [1.0, 0.55, 0.9, 1.0], // hot Dancer-pink so the "a Dancer did this" story reads
                    );
                    self.spawn_catch_shockwave(pos, [0.95, 0.7, 0.3]); // orange-gold burst — the Magnet flaring charged
                }

                self.dancer_spooked_buf = spooked;
                self.dancer_jolt_buf = jolted;
                self.dancer_trip_buf = tripped;
                self.dancer_chip_buf = chipped;
                self.dancer_kick_buf = kicked;
            }

            self.dancer_hop_scratch = dancer_hops; // hand the buffer back for reuse next beat

            // Dancer-link on-beat catch aura — "train position matters." A Dancer you've caught
            // keeps its rhythm even in the conga line: on every beat, each caught Dancer link
            // pulses a small on-beat catch aura that snags any free, catchable crab pressed up
            // against that spot in the train. Where the Dancer *sits* in the line — set purely by
            // the order you caught it — decides what its pulse sweeps up: a Dancer near the head
            // vacuums crabs by where you're actively herding, one further back cleans up whatever
            // the trailing tail brushes past. So catch order and train shape become a live
            // decision, the rhythm-native mirror of routing an Armored crab to the guarded tail.
            // On-beat only + small radius = a positioning *reward*, not an autocatch; the downbeat
            // reaches a hair wider so the "1" of the bar lands the biggest sweep.
            const DANCER_AURA_RADIUS: f32 = 58.0;
            let aura_radius = if downbeat {
                DANCER_AURA_RADIUS * 1.2
            } else {
                DANCER_AURA_RADIUS
            };
            let aura_r2 = aura_radius * aura_radius;
            // Snapshot where the caught Dancer links sit this beat (usually a small handful), so
            // the enlist loop below can borrow &mut self.crabs without an overlapping borrow.
            let mut dancer_links = std::mem::take(&mut self.dancer_link_buf);
            dancer_links.clear();
            dancer_links.extend(
                self.crabs
                    .iter()
                    .filter(|c| c.caught && c.is_dancer())
                    .map(|c| c.pos),
            );
            if !dancer_links.is_empty() {
                let mult = self.combo_multiplier();
                let mut rng = rand::rng();
                let mut aura_caught = std::mem::take(&mut self.dancer_aura_caught_buf);
                aura_caught.clear();
                for i in 0..self.crabs.len() {
                    // Free, catchable, ordinary herd crabs only — never a boss, a shelled
                    // Armored/Hermit (its shell isn't the aura's to crack), or an already-caught
                    // link. A Golden is fair game: parking a Dancer link where a snared Golden
                    // sits is a legit way to bank the prize on the beat.
                    if self.crabs[i].caught
                        || !self.crabs[i].is_catchable()
                        || self.crabs[i].is_boss()
                    {
                        continue;
                    }
                    let pos = self.crabs[i].pos;
                    if !dancer_links
                        .iter()
                        .any(|&d| d.distance_squared(pos) <= aura_r2)
                    {
                        continue;
                    }
                    let crab_type = self.crabs[i].crab_type;
                    let crab_color = self.crabs[i].crab_color();
                    let is_golden = self.crabs[i].is_golden();
                    self.particle_system
                        .spawn_catch_effect(pos, crab_color, crab_type, &mut rng);
                    self.crabs[i].caught = true;
                    self.crabs[i].chain_index = Some(self.chain_count);
                    self.chain_count += 1;
                    aura_caught.push((pos, is_golden));
                }
                let n = aura_caught.len();
                if n > 0 {
                    // Score the sweep like a small on-beat catch: each snag pays a base point at
                    // the live combo multiplier, and the grab bumps the combo so a well-placed
                    // Dancer link keeps a groove streak alive between your own catches.
                    let bonus = n * mult;
                    self.score += bonus;
                    self.combo_count += n;
                    self.combo_timer = 1.8;
                    self.on_beat_flash = self.on_beat_flash.max(if downbeat { 0.45 } else { 0.35 });
                    self.chain_join_ripple = true;
                    for &(pos, is_golden) in aura_caught.iter() {
                        // Hot Dancer-pink burst so the "your Dancer link did this" story reads at a
                        // glance, matching every other Dancer-crossover cue's color.
                        self.spawn_catch_shockwave(pos, [1.0, 0.45, 0.85]);
                        if is_golden {
                            // Fold in the full Golden payout — the aura banked the prize on the beat.
                            self.on_golden_caught(pos, 0);
                        }
                    }
                    // One shared "GROOVE PULL!" shout at the first snag so a multi-catch beat reads
                    // as a single moment, not a stack of overlapping pops.
                    let (label_pos, _) = aura_caught[0];
                    self.floating_texts.spawn(
                        if n > 1 {
                            format!("GROOVE PULL!  x{}", n)
                        } else {
                            "GROOVE PULL!".to_string()
                        },
                        label_pos - Vec2::new(56.0, 30.0),
                        26.0,
                        [1.0, 0.55, 0.9, 1.0],
                    );
                    self.check_milestone(&mut rng);
                }
                self.dancer_aura_caught_buf = aura_caught;
            }
            self.dancer_link_buf = dancer_links; // hand the buffer back for reuse next beat
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
                    // Golden crab being reeled in by whistle — its highest-pull matchup, show it.
                    if crab.is_golden() && self.whistle_golden_hits_buf.len() < 12 {
                        self.whistle_golden_hits_buf.push(crab.pos);
                    }
                    // Dancer pulled by whistle — rhythm tool meets rhythm crab, show the harmony.
                    if crab.is_dancer() && self.whistle_dancer_hits_buf.len() < 10 {
                        self.whistle_dancer_hits_buf.push(crab.pos);
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

        // Scale music volume with intensity
        // (action_music gets louder, layers fade in)
        // If music is muted, set all music volumes to 0; otherwise use normal intensity curve.
        // Duck the player's music slightly when an NPC King Crab is close — their rumble competes
        // for sonic space, making proximity feel threatening even before visual contact.
        let npc_duck = {
            let nearest_dist = self
                .npc_trains
                .iter()
                .map(|t| t.leader_pos.distance(self.player_pos))
                .fold(f32::MAX, f32::min);
            if nearest_dist < 400.0 {
                1.0 - ((400.0 - nearest_dist) / 400.0) * 0.25
            } else {
                1.0
            }
        };
        let base_vol = if self.music_muted {
            0.0
        } else {
            (0.25 + self.music_intensity * 0.75) * npc_duck
        };
        self.sounds
            .action_music
            .set_volume(base_vol.clamp(0.0, 1.0));
        let layer_count = self.music_layers.len();
        for (i, layer) in self.music_layers.iter_mut().enumerate() {
            let threshold = (i + 1) as f32 / (layer_count + 1) as f32;
            let vol = if self.music_muted {
                0.0
            } else if self.music_intensity > threshold {
                ((self.music_intensity - threshold) * 2.0).min(1.0)
            } else {
                0.0
            };
            layer.set_volume(vol);
            if !layer.playing() && vol > 0.01 {
                let _ = layer.play(ctx);
            }
        }

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

        // Spatial audio: smooth the king crab rumble volume toward the train's computed target.
        // Full volume within 200px, silent beyond 800px; muted on menu/game-over screens.
        {
            use ggez::audio::SoundSource;
            let target = if self.show_instructions || self.game_over || self.show_world_map {
                0.0
            } else {
                self.npc_trains.first().map_or(0.0, |t| t.target_vol)
            };
            let cur = self.sounds.king_crab_rumble.volume();
            let smoothed = (cur + (target - cur) * (dt * 2.0).min(1.0)).clamp(0.0, 1.0);
            self.sounds.king_crab_rumble.set_volume(smoothed);
            if smoothed > 0.02 && !self.sounds.king_crab_rumble.playing() {
                let _ = self.sounds.king_crab_rumble.play(ctx);
            } else if smoothed <= 0.02 && self.sounds.king_crab_rumble.playing() {
                self.sounds.king_crab_rumble.stop(ctx);
            }
        }

        // Crab-theme music loops: count how many of each archetype group are free on the field,
        // then smoothly ramp each theme's volume so the soundscape reflects what's out there.
        // Max volume is low (0.13) so they layer as ambient texture without drowning the game.
        if !self.show_instructions && !self.game_over && !self.show_world_map {
            use ggez::audio::SoundSource;
            // Count free crabs per theme group (caught crabs are "with you" — silence their theme).
            let mut counts = [0usize; 5];
            for c in &self.crabs {
                if c.caught {
                    continue;
                }
                let theme = match c.crab_type {
                    crate::enemies::CrabType::Normal
                    | crate::enemies::CrabType::Fast
                    | crate::enemies::CrabType::Big => 0,
                    crate::enemies::CrabType::Dancer | crate::enemies::CrabType::Splitter => 1,
                    crate::enemies::CrabType::Thief | crate::enemies::CrabType::Sneaky => 2,
                    crate::enemies::CrabType::Boss
                    | crate::enemies::CrabType::Armored
                    | crate::enemies::CrabType::Hermit => 3,
                    crate::enemies::CrabType::Golden | crate::enemies::CrabType::Magnet => 4,
                    _ => 0,
                };
                counts[theme] += 1;
            }
            let dt_audio = ctx.time.delta().as_secs_f32();
            for (i, theme) in self.sounds.crab_themes.iter_mut().enumerate() {
                let target = if counts[i] == 0 {
                    0.0
                } else {
                    // Scales from 0.05 (1 crab) up to 0.13 (8+ crabs)
                    (0.05 + (counts[i] as f32 - 1.0) * 0.01).min(0.13)
                };
                let cur = theme.volume();
                let smoothed = (cur + (target - cur) * (dt_audio * 2.5).min(1.0)).clamp(0.0, 0.2);
                theme.set_volume(smoothed);
                if smoothed > 0.01 && !theme.playing() {
                    let _ = theme.play(ctx);
                } else if smoothed <= 0.01 && theme.playing() {
                    theme.stop(ctx);
                }
            }
        }

        // Recompute the camera every frame so both draw() and the mouse handlers (which run outside
        // draw) agree on the screen<->world mapping this frame.
        self.camera_origin = self.compute_camera_origin();
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        // Bot mode: skip all rendering to run at maximum speed.
        if self.bot.is_some() {
            let mut canvas = Canvas::from_frame(ctx, ggez::graphics::Color::BLACK);
            canvas.finish(ctx)?;
            return Ok(());
        }

        // --- Pass 1: render the game scene to an offscreen image ---
        self.draw_scene(ctx)?;

        // --- Pass 2: blit the scene image to screen with post-processing ---
        {
            // Use drawable (physical pixel) size for the shader uniform so UV→pixel math is correct
            // on HiDPI displays, and scale the image to cover the full canvas.
            let (draw_w, draw_h) = ctx.gfx.drawable_size();
            let uniform = PostProcessUniform {
                groove: self.groove,
                time: self.time_elapsed,
                screen_width: draw_w,
                screen_height: draw_h,
            };
            self.postprocess_params.set_uniforms(ctx, &uniform);
            let mut screen_canvas = Canvas::from_frame(ctx, Color::BLACK);
            //screen_canvas.set_shader(&self.postprocess_shader);
            screen_canvas.set_shader_params(&self.postprocess_params);
            // Draw the image covering the full drawable area; scale(1,1) is correct because the
            // image was created at drawable size in state.rs.
            screen_canvas.draw(
                &self.scene_image,
                DrawParam::default().dest(Vec2::ZERO).scale(Vec2::ONE),
            );
            screen_canvas.set_default_shader();
            screen_canvas.finish(ctx)?;
        }

        Ok(())
    }

    fn key_down_event(&mut self, ctx: &mut Context, input: KeyInput, _repeat: bool) -> GameResult {
        if self.pending_upgrade {
            if let Some(key) = input.keycode {
                match key {
                    KeyCode::Key1 => self.apply_upgrade(1),
                    KeyCode::Key2 => self.apply_upgrade(2),
                    KeyCode::Key3 => self.apply_upgrade(3),
                    _ => {}
                }
            }
            return Ok(());
        }
        if let Some(key) = input.keycode {
            if key == KeyCode::F {
                self.flashlight.on = !self.flashlight.on;
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
            let to_aim = self.mouse_pos - origin;
            let aim_dist = to_aim.length();
            let clamped_target = if aim_dist > throw_range {
                origin + to_aim / aim_dist * throw_range
            } else if aim_dist > 1.0 {
                self.mouse_pos
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
    let mut state = MainState::new(&mut ctx)?;

    if let Some(ref name) = bot_script {
        use bot::{BotState, script_campaign_tutorial, script_groove_dash, script_menu_to_game};
        // menu_to_game runs at 3× so the proximity catch check fires frequently enough for
        // the bot walk to register catches (at 8× the player teleports past crabs between frames).
        state.time_scale = match name.as_str() {
            "menu_to_game" => 3.0,
            _ => 8.0,
        };
        state.bot = Some(match name.as_str() {
            "menu_to_game" => BotState::new(script_menu_to_game(), 60.0),
            "campaign_tutorial" => BotState::new(script_campaign_tutorial(), 30.0),
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
