mod controls;
mod enemies;
mod graphics;
mod levels;
mod spawnings;

use std::{cell::RefCell, collections::HashMap, collections::VecDeque, env, fs, path};

use ggez::audio::SoundSource;
use ggez::audio::Source;
use ggez::conf::WindowMode;
use ggez::event::{self, EventHandler};
use ggez::glam::Vec2;
use ggez::graphics::{
    BlendMode, Canvas, Color, DrawParam, Image, Mesh, Rect, Sampler, ShaderBuilder, Text,
};
use ggez::input::keyboard::{KeyCode, KeyInput};
use ggez::input::mouse::MouseButton;
use ggez::{Context, ContextBuilder, GameResult};
use rand::Rng;
use rand::prelude::IndexedRandom;
use spawnings::SpawnPattern;

use crate::controls::{handle_key_down_event, handle_player_movement};
use crate::enemies::{BossCharge, CrabType, EnemyCrab};
use crate::graphics::{
    FloatingTextSystem, ParticleSystem, cached_stroke_rect, draw_attracted_crab_glow,
    draw_armor_ring, draw_beat_indicator, draw_beat_wave_ring, draw_catch_shockwaves, draw_chain_rings,
    draw_combo_meter, draw_boss_health_ring, draw_conga_rope, draw_crab, draw_crab_radar,
    draw_delivery_pen, draw_fear_rings, draw_flashlight, draw_floating_texts, draw_grass, draw_lasso,
    draw_call_ring, draw_particles, draw_rustler, draw_speed_lines, draw_stomp_ring, draw_tide_pools,
    draw_tide_pulses, draw_wave_telegraph,
    draw_whistle_ring, unit_circle, unit_square,
};
use crate::levels::{Level, get_levels};
use crate::spawnings::{spawn_boss, spawn_tide_boss, spawn_enemies};

const PLAYER_SIZE: f32 = 48.0;
const CRAB_SIZE: f32 = 36.0;
const SPEED: f32 = 200.0;
const CHAIN_LINK_FRAMES: usize = 12;
const BEAT_INTERVAL: f32 = 0.5; // 120 BPM, crab rave tempo
const BEAT_WINDOW: f32 = 0.08;  // seconds around a beat that count as "on beat"
const BOSS_MAX_HEALTH: f32 = 3.0; // seconds of sustained flashlight needed to wear a King Crab down
const BOSS_DRAIN_RATE: f32 = 1.0; // boss health drained per second while held in the beam
const BOSS_SCORE_INTERVAL: usize = 40; // score gap between successive King Crab arrivals
// King Crab charge: it periodically lunges at the conga train to scatter the tail.
const BOSS_CHARGE_COOLDOWN: f32 = 4.5; // roam time between charges
const BOSS_WINDUP_TIME: f32 = 0.85;    // telegraph duration before a charge fires
const BOSS_CHARGE_TIME: f32 = 0.65;    // how long the lunge lasts
const BOSS_CHARGE_SPEED: f32 = 540.0;  // px/s during the lunge (far faster than it roams)
const BOSS_CHARGE_ARM_RANGE: f32 = 430.0; // only wind up when the train is within striking range
// Tide Boss pulse: instead of charging, it swells and releases an expanding shockwave ring that
// scatters nearby free crabs and knocks the train's tail loose if it's clustered too close.
const TIDE_PULSE_COOLDOWN: f32 = 5.0;   // drift time between pulses
const TIDE_PULSE_WINDUP: f32 = 1.0;     // telegraph swell before the pulse fires
const TIDE_PULSE_RADIUS: f32 = 320.0;   // reach of the shockwave — crabs inside get shoved outward
const TIDE_PULSE_EXPAND_SPEED: f32 = 900.0; // how fast the visible ring sweeps outward (px/s)
const WHISTLE_COOLDOWN: f32 = 4.5;     // seconds between whistle casts
const WHISTLE_RING_SPEED: f32 = 1000.0; // how fast the sonic front sweeps outward (px/s)
const WHISTLE_MAX_RADIUS: f32 = 360.0; // reach of the pulse — crabs inside it get yanked in
const WHISTLE_PULL_SPEED: f32 = 240.0; // base inward speed applied to caught-in crabs (× type pull)
const STOMP_COOLDOWN: f32 = 3.0;       // seconds between ground-pound Stomps
const STOMP_RING_SPEED: f32 = 900.0;   // how fast the shockwave slams outward (px/s)
const STOMP_MAX_RADIUS: f32 = 155.0;   // short reach — the Stomp is a close-range melee counter
const PEN_RADIUS: f32 = 90.0;          // delivery-pen goal zone; drive the train in to bank it

// --- Meta-progression shop (spend side) ---------------------------------------------------
// Banked crabs are spent on the title screen for permanent starting tool ranks. Each tool caps
// low so a perk is a head-start, not a run-trivializer (lane behavior milestones sit at rank 2).
const MAX_START_RANK: u32 = 2;
// Cost of buying the NEXT rank of a tool = (rank_being_bought) * PERK_COST_STEP. So rank 1 costs
// 30, rank 2 costs 60 — escalating, and priced against a per-run banking of a few dozen crabs so a
// perk is a handful of runs' worth of savings, not instant.
const PERK_COST_STEP: usize = 30;

thread_local! {
    // Cache for the persistent bottom-of-screen "Level N: Title / description" label. Its text
    // only ever depends on `current_level`, which changes at most a handful of times per run
    // (once per level), yet the label sits on screen for the entire level and was previously
    // rebuilt from scratch — fresh `format!` String, fresh `Text`, and two redundant `.measure()`
    // layout passes — on every single frame draw_game ran. Keyed by level index and bounded to
    // `levels.len()` entries for the life of the process, same pattern as the mesh caches in
    // graphics.rs.
    static LEVEL_LABEL_CACHE: RefCell<HashMap<usize, (Text, f32, f32)>> = RefCell::new(HashMap::new());

    // Cache for the top-left "Score / Train / Combo" HUD line. draw_game runs every frame but
    // takes &self, so — same as LEVEL_LABEL_CACHE above — this lives in a thread_local RefCell
    // rather than a struct field. Keyed by the actual (score, chain_len, combo_count, mult)
    // tuple so the fresh `format!` String + `Text` (glyph shaping) only gets rebuilt when one of
    // those values actually changes, not on every one of the ~60 frames between catches.
    static HUD_TEXT_CACHE: RefCell<Option<(usize, usize, usize, usize, Text)>> = RefCell::new(None);

    // The three ability-bar labels ("Stamina (Space)", "Whistle (E)[/READY]", "Stomp (R)[/READY]")
    // were being rebuilt via a fresh Text::new every single frame even though the stamina label
    // never changes at all and the other two only ever flip between one of two fixed strings.
    // Same fix as above: cache the built Text and only pay for glyph shaping again when the
    // underlying "ready" flag actually flips (or, for stamina, never again after the first frame).
    static STAMINA_LABEL_CACHE: RefCell<Option<Text>> = RefCell::new(None);
    static WHISTLE_LABEL_CACHE: RefCell<Option<(bool, Text)>> = RefCell::new(None);
    static STOMP_LABEL_CACHE: RefCell<Option<(bool, Text)>> = RefCell::new(None);

    // The menu/instructions screen's translucent rounded panel behind the controls text. Its
    // geometry only depends on window width/height (never on `menu_time`), yet
    // draw_instructions_screen was rebuilding a fresh `Mesh::new_rounded_rectangle` GPU buffer
    // every single frame this screen is shown — which can be indefinitely long if a player
    // idles on the title screen. Keyed by (width, height) bit patterns so it only rebuilds on an
    // actual resolution change (in practice: never, after the one-time fullscreen fixup).
    static MENU_PANEL_CACHE: RefCell<Option<(u32, u32, Mesh)>> = RefCell::new(None);

    // The rest of the title screen's text was in the same boat as the panel above: every one of
    // these Text objects is either fully static ("Crab Rustler", the per-character wave glyphs,
    // the instructions block, the start prompt) or changes only a handful of times per run (the
    // subtitle), yet draw_instructions_screen was rebuilding ~15 fresh `Text`s (plus several
    // `.measure()` glyph-layout passes) every single frame the menu sits on screen — which, like
    // the panel, can be indefinitely long if a player idles there. Only position/color/rotation
    // (all DrawParam, not the Text itself) actually change frame to frame, so build each once and
    // reuse it forever (or until the underlying string changes, for the subtitle).
    static MENU_TITLE_CACHE: RefCell<Option<(Text, f32, f32)>> = RefCell::new(None);
    static MENU_TITLE_CHARS_CACHE: RefCell<Option<Vec<Text>>> = RefCell::new(None);
    static MENU_SUBTITLE_CACHE: RefCell<Option<(String, Text, f32)>> = RefCell::new(None);
    static MENU_INSTRUCTIONS_CACHE: RefCell<Option<(Text, f32, f32)>> = RefCell::new(None);
    static MENU_PROMPT_CACHE: RefCell<Option<(Text, f32)>> = RefCell::new(None);

    // The title screen's "Career best ... crabs banked over N runs" line was rebuilt from a
    // fresh `format!` String + `Text::new` (plus a `.measure()` glyph-layout pass) every single
    // frame the title screen sits on-screen — same unbounded-idle-time cost as the panel/prompt
    // caches above. Keyed by the (best, total, runs) tuple, same pattern as HUD_TEXT_CACHE, so it
    // only rebuilds on the rare frame one of those actually changes (i.e. right after a run ends).
    static CAREER_LABEL_CACHE: RefCell<Option<(usize, usize, usize, Text, f32)>> = RefCell::new(None);
    // The perk-shop block (available crabs + the four buyable starting ranks). Rebuilt only when
    // its underlying numbers change — i.e. right after a purchase or a run ending — not every idle
    // frame. Key: (available, beam, lasso, whistle, stomp). Holds a header Text and a per-tool
    // list Text plus their measured widths.
    #[allow(clippy::type_complexity)]
    static SHOP_CACHE: RefCell<Option<((usize, u32, u32, u32, u32), Text, f32, Text, f32)>> =
        RefCell::new(None);

    // Scratch buffer for draw_game's chain-crab ordering. draw_game takes &self and runs every
    // frame, so — same reasoning as the caches above — this lives in a thread_local RefCell
    // instead of a struct field. Chain length grows unbounded over a run (it's the whole point
    // of the conga train), and this collected + sorted a fresh Vec<&EnemyCrab> from scratch every
    // single frame just to hand positions off to draw_conga_rope, which immediately copies them
    // out again. Reusing this buffer (and carrying (chain_index, pos) tuples so the sort key
    // travels with the position, sidestepping the borrow) drops that to zero allocations per
    // frame once the chain's high-water mark is reached.
    static CHAIN_SORT_BUF: RefCell<Vec<(usize, Vec2)>> = RefCell::new(Vec::new());
}

/// Pick a fresh delivery-pen location: somewhere on the field, kept away from the edges and a
/// good stride from `avoid` (usually the player) so banking always means routing the train across
/// open ground rather than the pen landing in your lap.
fn pick_pen_pos(width: f32, height: f32, avoid: Vec2, rng: &mut impl rand::Rng) -> Vec2 {
    let margin = PEN_RADIUS + 60.0;
    let min_dist = 320.0;
    let mut best = Vec2::new(width * 0.5, height * 0.5);
    let mut best_dist = -1.0;
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
fn pick_tide_pools(
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
        if pools.iter().any(|(pc, pr)| c.distance(*pc) < radius + pr + 50.0) {
            continue;
        }
        pools.push((c, radius));
    }
    pools
}

struct GameSounds {
    intro_music: Source,
    action_music: Source,
    outro_music: Source,
    upgrade: Source,
    success: Source,
    success2: Source,
    // Add more sounds here as needed
}

/// Play the catch chime with a touch of random pitch variation so a burst of rapid catches
/// (e.g. a conga chain sweeping up several crabs at once, or a lasso grabbing a cluster) doesn't
/// sound like the exact same note firing on a machine-gun loop. Mostly the regular chime, with
/// an occasional brighter `success2` swapped in for variety — same odds as before this existed.
/// Free function (not a `&mut self` method) so it can be called from inside loops that already
/// hold a disjoint mutable borrow of another field of `MainState` (e.g. `for crab in &mut
/// self.crabs`), where a whole-`self` method call wouldn't type-check.
fn play_catch_sound(sounds: &mut GameSounds, ctx: &mut Context, rng: &mut impl rand::Rng) {
    let pitch = rng.random_range(0.92_f32..1.08);
    if rng.random_range(0..5) == 0 {
        sounds.success2.set_pitch(pitch);
        let _ = sounds.success2.play_detached(ctx);
    } else {
        sounds.success.set_pitch(pitch);
        let _ = sounds.success.play_detached(ctx);
    }
}

struct Flashlight {
    on: bool,
    cone_upgrade: f32,
    range_upgrade: f32,
    laser_level: u32,
}

#[derive(Clone)]
enum LevelTexture {
    Grass,
    Sand,
}

struct GameTextures {
    grass: Image,
    sand: Image,
    player: Image,
}

struct MainState {
    player_pos: Vec2,                          // Player position
    player_vel: Vec2,                          // Player velocity (for smooth movement)
    mouse_pos: Vec2,                           // Mouse position for flashlight aiming
    crabs: Vec<EnemyCrab>,                     // List of crabs in the game
    score: usize,                              // Current score
    spawn_timer: f32,                          // Timer for spawning new crabs
    time_elapsed: f32,                         // Time since game start
    menu_time: f32,                            // Free-running clock for the title/menu screen animation
    game_over: bool,                           // Game over flag
    sounds: GameSounds,                        // All game sound effects
    flashlight: Flashlight,                    // Flashlight settings and upgrades
    show_instructions: bool,                   // Show instructions screen
    last_dir: Vec2,                            // Last movement direction for flashlight
    shake_timer: f32,                          // Timer for crab shake effect
    time_since_catch: f32,                     // Time since last crab was caught
    boost_timer: f32,                          // Timer for speed boost
    boost_cooldown: f32,                       // Cooldown to prevent holding space
    levels: Vec<Level>,                        // List of levels with patterns
    current_level: usize,                      // Current level index
    current_pattern: usize,                    // Current pattern index within the level
    pattern_timer: f32,                        // Timer for current pattern duration
    debug_mode: bool,                          // Debug mode flag
    pending_upgrade: bool,                     // Whether upgrade screen should be shown
    best_time: f32,                            // Fastest time to catch all crabs
    // --- Meta-progression: a single persistent thread that survives across runs, so ending a
    // run (win or loss) still banks progress into a career you carry forward. Persisted to
    // career.txt as three whitespace-separated integers: best_score total_score runs.
    career_best_score: usize,                  // Highest single-run score ever reached
    career_total_score: usize,                 // Sum of every run's final score (lifetime crabs banked)
    career_runs: usize,                        // How many runs have ended
    run_recorded: bool,                        // Guard so the current run is banked into career exactly once
    run_is_new_best: bool,                     // Did the just-ended run set a new career best? (for game-over flourish)
    // Spend side of meta-progression: banked crabs (career_total_score) are a currency you spend
    // on the title screen for PERMANENT starting tool ranks — a head-start that persists across
    // runs, so even a losing run buys you closer to your next unlock. `career_spent` is the ledger
    // of crabs already committed; available = career_total_score - career_spent. The four
    // start_*_rank fields are the ranks a fresh run begins each tool at (capped low so it's a
    // leg-up, not a run-trivializer). Persisted alongside best/total/runs in career.txt.
    career_spent: usize,
    start_beam_rank: u32,
    start_lasso_rank: u32,
    start_whistle_rank: u32,
    start_stomp_rank: u32,
    shop_flash: f32,                           // brief green flash on the last-bought perk (title-screen juice)
    shop_denied: f32,                          // brief red flash when a purchase is refused (can't afford / maxed)
    width: f32,                                // Virtual width of the game
    height: f32,                               // Virtual height of the game
    shader: ggez::graphics::Shader,            // Shader for grass rendering
    flashlight_shader: ggez::graphics::Shader, // Shader for flashlight rendering
    particle_system: ParticleSystem,           // Particle effects system
    level_title: String,                       // Title of the current level
    level_title_timer: f32,                    // Timer for displaying level title
    subtitle: String,                          // Random subtitle for instructions screen
    position_history: VecDeque<Vec2>,
    chain_count: usize,
    beat_timer: f32,
    beat_intensity: f32,
    music_intensity: f32,
    on_beat_flash: f32,
    groove: f32,         // 0..=1 on-beat "groove" meter — fills on rhythmic catches, decays over time
    beat_streak: u32,    // consecutive on-beat catches; escalates the score bonus
    music_layers: Vec<Source>,
    catch_radius_upgrade: f32,
    // Upgrade lanes — level-ups deepen ONE of the four tools instead of handing out flat stat
    // bumps, so committing to a lane branches the run into a distinct playstyle (beam boss-hunter,
    // lasso chain-catcher, whistle crowd-control, stomp shell-breaker). Each rank scales the tool
    // and, at milestone ranks, changes how it behaves. Effective per-tool values are derived from
    // these ranks in the helper methods below rather than stored, so they stay in sync everywhere.
    beam_rank: u32,
    lasso_rank: u32,
    whistle_rank: u32,
    stomp_rank: u32,
    floating_texts: FloatingTextSystem,
    combo_count: usize,
    combo_timer: f32,
    textures: GameTextures,                    // Textures for grass, sand, and player
    level_textures: Vec<LevelTexture>,         // Textures for each level
    // Beat Wave ability
    beat_count: u32,                           // Counts beats fired, every 4th triggers wave
    beat_wave_active: bool,                    // Whether beat wave is expanding
    beat_wave_radius: f32,                     // Current radius of expanding wave
    // Bar-quantized spawns: when a pattern ends we don't drop the next wave at an arbitrary
    // instant — we arm it and let it land on the next downbeat (bar boundary), so every fresh
    // herd arrives locked to the music. `wave_armed` is set when the pattern timer lapses (or
    // the field's fully caught), and the beat handler fires the wave on the next `beat_count %
    // 4 == 0`. `wave_telegraph` counts up while armed so the draw layer can flash a "here it
    // comes" pulse in the bottom bar.
    wave_armed: bool,
    wave_telegraph: f32,
    // Lasso Throw ability
    lasso_pos: Option<Vec2>,                   // Current lasso tip position (None = inactive)
    lasso_timer: f32,                          // Time remaining on lasso flight
    lasso_target: Vec2,                        // Target position for lasso
    // Whistle ability — a sonic pulse that yanks nearby crabs toward the player. Soft-counters
    // skittish Sneaky crabs (strong pull) while heavy Big crabs barely budge (see CrabType::whistle_pull).
    whistle_active: f32,                        // >0 while the ring is expanding (seconds remaining)
    whistle_radius: f32,                        // current front radius of the expanding pulse
    whistle_cooldown: f32,                      // >0 while on cooldown; whistle unusable until it hits 0
    whistle_center: Vec2,                       // player center captured at cast time (ring origin)
    whistle_beat_bonus: f32,                     // 1.0 normally, >1 when this cast landed on-beat (bigger reach)
    // Stomp ability — a close-range ground-pound that CRACKS armored crab shells instantly (its
    // dedicated counter; the beam is the slow universal fallback) and shoves nearby free crabs in.
    stomp_active: f32,                          // >0 while the shockwave is expanding (seconds remaining)
    stomp_radius: f32,                          // current front radius of the shockwave
    stomp_cooldown: f32,                        // >0 while on cooldown; Stomp unusable until it hits 0
    stomp_center: Vec2,                         // player center captured at stomp time (ring origin)
    stomp_beat_bonus: f32,                       // 1.0 normally, >1 when this cast landed on-beat (bigger slam)
    // Call ability (F) — a rhythm-native summon aimed at Dancer crabs. An on-beat Call charms every
    // nearby Dancer into "answering": on the next beat they hop TOWARD the player instead of fleeing,
    // opening a catch window you actively play for. Off-beat it fizzles. This is the player's own
    // on-beat action the Dancer answers to, turning rhythm from something you watch into something
    // you play. Purely a control layer over existing Dancer hop logic — no new draw dependency.
    call_cooldown: f32,                          // >0 while on cooldown; Call unusable until it hits 0
    call_pulse: f32,                             // 0..1 visual ring pulse, set to 1 on a successful on-beat Call, decays
    call_pulse_center: Vec2,                     // player center captured when the Call rang out
    // Dash effect
    dash_just_fired: bool,
    dash_flash: f32,
    // Camera shake
    screen_shake: f32,          // current shake magnitude (pixels), decays each frame
    screen_shake_vel: Vec2,     // current shake offset velocity
    screen_shake_offset: Vec2,  // current pixel offset applied to viewport
    hitstop_timer: f32,         // brief whole-sim freeze right after a catch (juice)
    chain_join_ripple: bool,       // set true when any crab is caught this frame
    chain_snap_cooldown: f32,      // >0 briefly after a tail snaps, so one brush can't strip the whole train
    next_milestone: usize,               // Next train-length milestone to celebrate
    next_boss_score: usize,              // score at which the next boss arrives
    next_boss_is_tide: bool,             // alternates King Crab <-> Tide Boss so runs cycle both
    // Delivery pen — the "cash in the train" mechanic. Drive the conga line into the pen to bank
    // the whole train for a super-linear score payout (longer train = disproportionately more) and
    // reset the chain, closing the risk/reward loop the chain-snap risk opened. The pen relocates
    // each level so routing the train there stays a fresh decision.
    pen_pos: Vec2,                       // center of the delivery pen on the field
    deliver_flash: f32,                  // 1..0 bloom timer after a successful bank (visual only)
    // Tide pools — terrain that shapes where the train can go. Each pool is a patch of shallow
    // water (center, radius) that drags on movement: crossing one slows the player to a wade, and
    // because the whole conga tail replays the player's path, hauling a long train through open
    // water costs you real time and exposure. They relocate each level (like the pen) so routing
    // — skirt the pools or dash across them — stays a live, geography-driven decision.
    tide_pools: Vec<(Vec2, f32)>,        // (center, radius) of each shallow-water drag zone
    in_tide_pool: bool,                  // whether the player is wading right now (for splash juice)
    chain_rings: Vec<(Vec2, f32, [f32; 3])>, // (pos, age 0..1, rgb) for beat ghost rings
    catch_shockwaves: Vec<(Vec2, f32, [f32; 3])>, // (pos, age 0..1, rgb) impact ring per catch
    fear_rings: Vec<(Vec2, f32)>,          // (pos, age 0..1) cold alarm ring where a catch startled the herd
    // Tide Boss shockwave pulses — (center, current radius) of each expanding front. Grows to
    // TIDE_PULSE_RADIUS then fades out. Bounded by the one-boss-at-a-time cap plus a hard len guard.
    tide_pulses: Vec<(Vec2, f32)>,
    zoom_punch: f32,            // camera zoom-in kick on catch, springs back to 0 (juice)
    fullscreen_applied: bool, // deferred until the first update tick, see update()
    // Scratch buffers for catch_by_chain, reused every frame instead of being freshly
    // allocated each call. The play area is fixed-size so the grid's cell count (and thus
    // its Vec<usize> bucket count) stabilizes quickly — clearing beats rebuilding from scratch.
    chain_positions_buf: Vec<Vec2>,
    catch_grid_buf: std::collections::HashMap<(i32, i32), Vec<usize>>,
    caught_now_buf: Vec<bool>,
    // Reused buffer of solid conga-body segment positions, rebuilt each frame for the
    // fleeing-crab wall-deflection pass (see deflect_fleeing_off_chain).
    deflect_body_buf: Vec<Vec2>,
    // Spatial grid over deflect_body_buf (same idea as catch_grid_buf below) so each fleeing
    // crab only tests nearby body segments instead of the whole chain — chain length has no
    // cap, so a linear scan there gets slower the longer a session runs.
    deflect_grid_buf: std::collections::HashMap<(i32, i32), Vec<usize>>,
    // Reused scratch buffer for bounce-ring spawn positions collected during the deflection
    // pass, avoiding a fresh Vec allocation every frame.
    deflect_bounce_buf: Vec<Vec2>,
    // Event-collection scratch buffers for update_crabs, reused every frame instead of being
    // freshly allocated on each call. Most frames produce zero events in each of these (no
    // crab started fleeing, no boss broke, etc.), so a per-frame Vec::new() was pure churn —
    // clearing a buffer that's almost always empty costs nothing, while allocating one does.
    flee_pops_buf: Vec<Vec2>,
    boss_broke_buf: Vec<Vec2>,
    armor_broke_buf: Vec<Vec2>,
    attraction_particles_buf: Vec<(Vec2, Vec2, f32, [f32; 3])>,
    boss_windups_buf: Vec<Vec2>,
    boss_launches_buf: Vec<Vec2>,
    boss_charge_dust_buf: Vec<(Vec2, Vec2)>,
    tide_fires_buf: Vec<Vec2>,
    tide_swells_buf: Vec<Vec2>,
    // Lightweight perf instrumentation (debug builds only): accumulate frame times and print an
    // average + worst-case every couple seconds so future optimization passes have real numbers
    // instead of guessing from code inspection alone.
    #[cfg(debug_assertions)]
    perf_frame_count: u32,
    #[cfg(debug_assertions)]
    perf_time_accum: f32,
    #[cfg(debug_assertions)]
    perf_worst_frame: f32,
}

impl MainState {
    fn new(ctx: &mut Context) -> GameResult<MainState> {
        let width = 1280.0;
        let height = 960.0;

        // Player starts in the center always.
        let player_pos = Vec2::new(
            width / 2.0 - PLAYER_SIZE / 2.0,
            height / 2.0 - PLAYER_SIZE / 2.0,
        );

        // TODO Load all sound effects.
        let sounds = GameSounds {
            intro_music: Source::new(ctx, "/intro.ogg")?,
            action_music: Source::new(ctx, "/action.ogg")?,
            outro_music: Source::new(ctx, "/outro.ogg")?,
            upgrade: Source::new(ctx, "/upgrade.ogg")?,
            success: Source::new(ctx, "/success.ogg")?,
            success2: Source::new(ctx, "/success2.ogg")?,
            // Add more sounds here as needed
        };

        // Load both grass and sand textures.
        let textures = GameTextures {
            grass: Image::from_path(ctx, "/grass.png")?,
            sand: Image::from_path(ctx, "/sand.png")?,
            player: Image::from_path(ctx, "/rustler.png")?,
        };

        // Get levels.
        let levels = get_levels();

        // Delivery pen + tide-pool hazards for the opening level, placed before `levels` is moved
        // into the struct so we can read the first zone's difficulty for the pool count.
        let init_pen = pick_pen_pos(
            width,
            height,
            player_pos + Vec2::splat(PLAYER_SIZE / 2.0),
            &mut rand::rng(),
        );
        let init_tide_pools = pick_tide_pools(
            width,
            height,
            init_pen,
            player_pos + Vec2::splat(PLAYER_SIZE / 2.0),
            levels.first().map(|l| l.difficulty).unwrap_or(0),
            &mut rand::rng(),
        );

        // Randomly select a texture for each level
        let mut rng = rand::rng();
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
        let (
            career_best_score,
            career_total_score,
            career_runs,
            career_spent,
            start_beam_rank,
            start_lasso_rank,
            start_whistle_rank,
            start_stomp_rank,
        ) = fs::read_to_string("career.txt")
            .ok()
            .and_then(|s| {
                let mut it = s.split_whitespace();
                let best = it.next()?.parse::<usize>().ok()?;
                let total = it.next()?.parse::<usize>().ok()?;
                let runs = it.next()?.parse::<usize>().ok()?;
                let spent = it.next().and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
                let clamp_rank =
                    |v: Option<&str>| v.and_then(|s| s.parse::<u32>().ok()).unwrap_or(0).min(MAX_START_RANK);
                let beam = clamp_rank(it.next());
                let lasso = clamp_rank(it.next());
                let whistle = clamp_rank(it.next());
                let stomp = clamp_rank(it.next());
                Some((best, total, runs, spent, beam, lasso, whistle, stomp))
            })
            .unwrap_or((0, 0, 0, 0, 0, 0, 0, 0));

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

        let flashlight = Flashlight {
            on: true,
            cone_upgrade: 0.0,
            range_upgrade: 0.0,
            laser_level: 0,
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
            .choose(&mut rand::rng())
            .unwrap()
            .to_string();

        Ok(MainState {
            player_pos,
            player_vel: Vec2::ZERO,
            mouse_pos: Vec2::ZERO,
            crabs,
            score: 0,
            spawn_timer: 0.0,
            time_elapsed: 0.0,
            menu_time: 0.0,
            game_over: false,
            sounds,
            flashlight,
            show_instructions: true,
            last_dir: Vec2::ZERO,
            shake_timer: 0.0,
            time_since_catch: 0.0,
            boost_timer: 0.0,
            boost_cooldown: 0.0,
            levels,
            current_level: 0,
            current_pattern: 0,
            pattern_timer: 0.0,
            debug_mode: true,
            pending_upgrade: false,
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
            shop_denied: 0.0,
            run_recorded: false,
            run_is_new_best: false,
            width,
            height,
            shader,
            flashlight_shader,
            particle_system: ParticleSystem::new(),
            level_title: String::new(),
            level_title_timer: 0.0,
            textures,
            level_textures,
            subtitle,
            position_history,
            chain_count: 0,
            beat_timer: BEAT_INTERVAL,
            beat_intensity: 0.0,
            music_intensity: 0.0,
            on_beat_flash: 0.0,
            groove: 0.0,
            beat_streak: 0,
            music_layers,
            catch_radius_upgrade: 0.0,
            // Runs begin at the permanent starting ranks bought with banked crabs (the spend side
            // of meta-progression), not flat zero.
            beam_rank: start_beam_rank,
            lasso_rank: start_lasso_rank,
            whistle_rank: start_whistle_rank,
            stomp_rank: start_stomp_rank,
            floating_texts: FloatingTextSystem::new(),
            combo_count: 0,
            combo_timer: 0.0,
            beat_count: 0,
            beat_wave_active: false,
            beat_wave_radius: 0.0,
            wave_armed: false,
            wave_telegraph: 0.0,
            lasso_pos: None,
            lasso_timer: 0.0,
            lasso_target: Vec2::ZERO,
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
            call_pulse: 0.0,
            call_pulse_center: Vec2::ZERO,
            dash_just_fired: false,
            dash_flash: 0.0,
            screen_shake: 0.0,
            screen_shake_vel: Vec2::ZERO,
            screen_shake_offset: Vec2::ZERO,
            hitstop_timer: 0.0,
            chain_join_ripple: false,
            chain_snap_cooldown: 0.0,
            next_milestone: 5,
            next_boss_score: BOSS_SCORE_INTERVAL,
            next_boss_is_tide: false,
            pen_pos: init_pen,
            deliver_flash: 0.0,
            tide_pools: init_tide_pools,
            in_tide_pool: false,
            chain_rings: Vec::new(),
            catch_shockwaves: Vec::new(),
            fear_rings: Vec::new(),
            tide_pulses: Vec::new(),
            zoom_punch: 0.0,
            fullscreen_applied: false,
            chain_positions_buf: Vec::new(),
            catch_grid_buf: std::collections::HashMap::new(),
            caught_now_buf: Vec::new(),
            deflect_body_buf: Vec::new(),
            deflect_grid_buf: std::collections::HashMap::new(),
            deflect_bounce_buf: Vec::new(),
            flee_pops_buf: Vec::new(),
            boss_broke_buf: Vec::new(),
            armor_broke_buf: Vec::new(),
            attraction_particles_buf: Vec::new(),
            boss_windups_buf: Vec::new(),
            boss_launches_buf: Vec::new(),
            boss_charge_dust_buf: Vec::new(),
            tide_fires_buf: Vec::new(),
            tide_swells_buf: Vec::new(),
            #[cfg(debug_assertions)]
            perf_frame_count: 0,
            #[cfg(debug_assertions)]
            perf_time_accum: 0.0,
            #[cfg(debug_assertions)]
            perf_worst_frame: 0.0,
        })
    }

    /// Kick off a punchy impact ring at the exact spot a crab was caught. Color-coded
    /// to the crab so different crab types read differently at a glance.
    fn spawn_catch_shockwave(&mut self, pos: Vec2, crab_color: [f32; 3]) {
        // Cap live shockwaves so a big beat-wave sweep can't unbound the vec.
        if self.catch_shockwaves.len() < 48 {
            self.catch_shockwaves.push((pos, 0.0, crab_color));
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
        let mut startled_pops: Vec<Vec2> = Vec::new();
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
            let outward = if outward == Vec2::ZERO { Vec2::new(0.0, -1.0) } else { outward };
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
        for pos in startled_pops {
            self.floating_texts.spawn(
                "!".to_string(),
                pos - Vec2::new(0.0, 24.0),
                24.0,
                [0.6, 0.9, 1.0, 1.0],
            );
        }
    }

    /// Emergent beat-startle chain reaction: on each beat, crabs that are already panicking
    /// (fleeing the player or mid-stampede) pass their fear to nearby *calm* crabs, so a scare
    /// ripples outward crab-to-crab across the herd on the pulse rather than every crab only ever
    /// reacting to the player directly. Carriers are snapshotted before infection, so the panic
    /// advances just one hop per beat — a visible marching wave, not an instant map-wide cascade.
    /// Self-limiting: only calm crabs can catch it (a crab already panicking isn't re-triggered),
    /// the startle bolt decays in ~one beat, and infections are capped per beat, so the wave dies
    /// down instead of locking the whole herd in permanent flight.
    fn beat_startle_contagion(&mut self) {
        const CONTAGION_RADIUS: f32 = 110.0;
        const MAX_INFECTIONS_PER_BEAT: usize = 8;
        // Snapshot of panicking crabs whose fear can jump to a neighbour this beat.
        let carriers: Vec<Vec2> = self
            .crabs
            .iter()
            .filter(|c| !c.caught && !c.is_boss() && (c.fleeing || c.startle_timer > 0.0))
            .map(|c| c.pos)
            .collect();
        if carriers.is_empty() {
            return;
        }
        let mut infected_pops: Vec<Vec2> = Vec::new();
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
            // Nearest carrier within reach becomes the source the crab bolts away from.
            let mut nearest: Option<(f32, Vec2)> = None;
            for &source in &carriers {
                let d = source.distance(crab.pos);
                if d < CONTAGION_RADIUS && nearest.map_or(true, |(nd, _)| d < nd) {
                    nearest = Some((d, source));
                }
            }
            if let Some((d, source)) = nearest {
                let outward = (crab.pos - source).normalize_or_zero();
                let outward = if outward == Vec2::ZERO { Vec2::new(0.0, -1.0) } else { outward };
                let prox = 1.0 - d / CONTAGION_RADIUS; // 1 right on the carrier, 0 at the rim
                let kick = crab.crab_type.speed_range().end * (1.1 + prox * 0.9);
                crab.vel = outward * kick;
                crab.speed = 1.0; // vel now encodes full speed, matching the flee/startle convention
                crab.startle_timer = 0.45;
                infected_pops.push(crab.pos);
            }
        }
        // Cold alarm rings + "!" pops so the crab-to-crab ripple reads at a glance.
        for pos in infected_pops {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((pos, 0.0));
            }
            self.floating_texts.spawn(
                "!".to_string(),
                pos - Vec2::new(0.0, 24.0),
                22.0,
                [0.6, 0.9, 1.0, 1.0],
            );
        }
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
        const MIN_TRAIN_TO_SNAP: usize = 5;        // short trains are safe — the risk only bites once you've invested
        const SNAP_COLLIDE_DIST: f32 = CRAB_SIZE * 0.9;
        const SNAP_LINKS: usize = 3;               // how many tail links a hit knocks loose
        const SNAP_COOLDOWN: f32 = 1.6;            // grace period so a herd can't strip everything at once

        if self.chain_snap_cooldown > 0.0 || self.chain_count < MIN_TRAIN_TO_SNAP {
            return;
        }
        // The vulnerable end is the most-recently-caught crab (highest chain_index sits at the tail).
        let tail_index = self.chain_count - 1;
        let Some(tail_pos) = self
            .crabs
            .iter()
            .find(|c| c.caught && c.chain_index == Some(tail_index))
            .map(|c| c.pos)
        else {
            return;
        };
        // Did a panicking wild crab — or a King Crab mid-lunge — just slam into the tail?
        let hit = self.crabs.iter().any(|c| {
            if c.caught {
                return false;
            }
            if c.is_boss() {
                // A charging King Crab plows through the tail; its bulk gives it a wider reach.
                matches!(c.charge_state, BossCharge::Charging(_))
                    && c.pos.distance(tail_pos) < SNAP_COLLIDE_DIST + c.scale * CRAB_SIZE * 0.5
            } else {
                (c.fleeing || c.startle_timer > 0.0)
                    && c.pos.distance(tail_pos) < SNAP_COLLIDE_DIST
            }
        });
        if !hit {
            return;
        }

        // Release the last SNAP_LINKS links — always leave at least the head crab attached.
        let keep = self.chain_count.saturating_sub(SNAP_LINKS).max(1);
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
                let outward = if outward == Vec2::ZERO { Vec2::new(0.0, 1.0) } else { outward };
                crab.vel = outward * crab.crab_type.speed_range().end * 2.2;
                crab.speed = 1.0; // vel now encodes full speed, matching the flee/startle convention
                snapped_positions.push(crab.pos);
            }
        }
        // Indices 0..keep stay contiguous, so the shortened train and future catches line up cleanly.
        self.chain_count = keep;
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
            ((p.x / cell_size).floor() as i32, (p.y / cell_size).floor() as i32)
        };
        for bucket in self.deflect_grid_buf.values_mut() {
            bucket.clear();
        }
        for (i, &seg) in self.deflect_body_buf.iter().enumerate() {
            self.deflect_grid_buf.entry(cell_of(seg)).or_default().push(i);
        }

        self.deflect_bounce_buf.clear();
        let mut rng = rand::rng();
        for crab in &mut self.crabs {
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
        }
        for &pos in &self.deflect_bounce_buf {
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
        let score_text = if pts > 1 { format!("+{}  ON BEAT!", pts) } else { format!("+{}", pts) };
        let color = if pts > 1 {
            [1.0, 0.95, 0.3, 1.0]
        } else {
            [1.0, 1.0, 1.0, 0.9]
        };
        self.floating_texts.spawn(score_text, catch_pos - Vec2::new(10.0, 20.0), 28.0, color);

        // Combo pop above the player
        if self.combo_count >= 3 {
            let combo_color = match self.combo_count {
                3..=4  => [1.0, 0.6, 0.1, 1.0],  // orange
                5..=7  => [1.0, 0.2, 0.2, 1.0],  // red
                _      => [0.8, 0.3, 1.0, 1.0],  // purple
            };
            self.floating_texts.spawn(
                format!("x{} COMBO!", self.combo_count),
                self.player_pos - Vec2::new(0.0, 50.0),
                36.0,
                combo_color,
            );
        }
    }

    fn combo_multiplier(&self) -> usize {
        match self.combo_count {
            0..=2 => 1,
            3..=5 => 2,
            6..=9 => 3,
            _ => 5,
        }
    }

    fn check_milestone(&mut self, rng: &mut impl rand::Rng) {
        let chain_len = self.crabs.iter().filter(|c| c.caught).count();
        if chain_len >= self.next_milestone {
            let milestone = self.next_milestone;
            self.next_milestone += 5;

            // Fireworks burst from player center
            let center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            self.particle_system.spawn_milestone_fireworks(center, milestone, rng);

            // Big centered banner text — spawn two: one shadow, one lit
            let banner = format!("{} CRABS!", milestone);
            let screen_center = Vec2::new(self.width / 2.0 - 100.0, self.height / 2.0 - 80.0);
            // Shadow
            self.floating_texts.spawn(banner.clone(), screen_center + Vec2::new(3.0, 3.0), 72.0, [0.0, 0.0, 0.0, 0.85]);
            // Main text — gold/yellow
            self.floating_texts.spawn(banner, screen_center, 72.0, [1.0, 0.92, 0.1, 1.0]);

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
    fn try_deliver_train(&mut self, ctx: &mut Context) {
        if self.chain_count == 0 {
            return;
        }
        let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        if player_center.distance(self.pen_pos) > PEN_RADIUS {
            return;
        }

        // How many crabs are actually banking (defensive count in case any wild state drifted).
        let delivered = self.crabs.iter().filter(|c| c.caught).count().max(self.chain_count);
        if delivered == 0 {
            return;
        }

        // Super-linear payout: triangular sum so crab #n adds n points, times a flat handler.
        let n = delivered;
        let bank = (n * (n + 1) / 2) * 3;
        self.score += bank;

        // The delivered crabs leave the field for good — they've been penned.
        self.crabs.retain(|c| !c.caught);
        self.chain_count = 0;
        self.next_milestone = 5;

        // Big celebratory feedback so banking feels like a real payoff, not just a number ticking.
        let mut rng = rand::rng();
        self.particle_system.spawn_milestone_fireworks(self.pen_pos, n, &mut rng);
        self.spawn_catch_shockwave(self.pen_pos, [0.5, 1.0, 0.5]);
        self.floating_texts.spawn(
            format!("BANKED +{}", bank),
            self.pen_pos - Vec2::new(60.0, 40.0),
            48.0,
            [0.4, 1.0, 0.5, 1.0],
        );
        self.floating_texts.spawn(
            format!("{} crabs delivered!", n),
            self.pen_pos - Vec2::new(70.0, 4.0),
            26.0,
            [1.0, 0.95, 0.6, 1.0],
        );
        self.deliver_flash = 1.0;
        self.zoom_punch = self.zoom_punch.max(0.11);
        self.screen_shake = self.screen_shake.max(18.0);
        let kick_angle = rng.random_range(0.0_f32..std::f32::consts::TAU);
        self.screen_shake_vel = Vec2::new(kick_angle.cos(), kick_angle.sin()) * 18.0 * 60.0;
        self.on_beat_flash = 0.6;
        self.groove = (self.groove + 0.35).min(1.0);
        let _ = self.sounds.success2.play_detached(ctx);

        // Move the pen so the next bank is a fresh routing decision, not a treadmill loop.
        self.pen_pos = pick_pen_pos(self.width, self.height, player_center, &mut rng);
    }

    fn handle_crab_catching(&mut self, ctx: &mut Context) {
        let mult = self.combo_multiplier();
        let mut any_caught = false;
        let mut startle_origins: Vec<Vec2> = Vec::new();
        let mut boss_catches: Vec<(Vec2, bool)> = Vec::new();
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

                crab.caught = true;
                self.chain_join_ripple = true;
                if self.catch_shockwaves.len() < 48 {
                    self.catch_shockwaves.push((shock_pos, 0.0, crab_color));
                }
                startle_origins.push(shock_pos);
                any_caught = true;
                crab.chain_index = Some(self.chain_count);
                self.chain_count += 1;
                let on_beat = self.beat_timer < BEAT_WINDOW
                    || self.beat_timer > BEAT_INTERVAL - BEAT_WINDOW;
                let bonus;
                if on_beat {
                    // On-beat catch: build the groove. Consecutive on-beat catches escalate the
                    // score bonus and fill the groove meter, which in turn swells the music.
                    self.beat_streak += 1;
                    self.groove = (self.groove + 0.22).min(1.0);
                    bonus = self.beat_streak.min(5) as usize;
                    self.on_beat_flash = (0.25 + self.beat_streak as f32 * 0.06).min(0.6);
                    if self.beat_streak >= 3 {
                        self.floating_texts.spawn(
                            format!("GROOVE x{}!", self.beat_streak),
                            self.player_pos - Vec2::new(0.0, 80.0),
                            34.0,
                            [0.4, 1.0, 0.85, 1.0],
                        );
                    }
                } else {
                    // Off-beat catch breaks the streak and drains the groove.
                    self.beat_streak = 0;
                    self.groove = (self.groove - 0.3).max(0.0);
                    bonus = 0;
                }
                let pos = crab.pos;
                let player_pos = self.player_pos;
                // Inline register_catch to avoid &mut self conflict with the crabs loop
                self.score += (1 + bonus) * mult;
                self.combo_count += 1;
                self.combo_timer = 1.8;
                let pts = (1 + bonus) * mult;
                let score_str = if pts > 1 { format!("+{}  ON BEAT!", pts) } else { format!("+{}", pts) };
                let score_col = if pts > 1 { [1.0, 0.95, 0.3, 1.0] } else { [1.0, 1.0, 1.0, 0.9] };
                self.floating_texts.spawn(score_str, pos - Vec2::new(10.0, 20.0), 28.0, score_col);
                if self.combo_count >= 3 {
                    let cc = self.combo_count;
                    let combo_col = match cc { 3..=4 => [1.0, 0.6, 0.1, 1.0], 5..=7 => [1.0, 0.2, 0.2, 1.0], _ => [0.8, 0.3, 1.0, 1.0] };
                    self.floating_texts.spawn(format!("x{} COMBO!", cc), player_pos - Vec2::new(0.0, 50.0), 36.0, combo_col);
                }
                self.shake_timer = 0.4;
                self.time_since_catch = 0.0;
                // Punchy freeze — a touch longer when the catch lands on the beat.
                self.hitstop_timer = self.hitstop_timer.max(if on_beat { 0.08 } else { 0.05 });
                // Snap the camera in a hair on every catch, harder on the beat, for extra impact.
                self.zoom_punch = self.zoom_punch.max(if on_beat { 0.055 } else { 0.035 });
                play_catch_sound(&mut self.sounds, ctx, &mut rng);
                if self.score > 0 && self.score % 10 == 0 {
                    let _ = self.sounds.upgrade.play_detached(ctx);
                    self.pending_upgrade = true;
                }
            }
        }
        for origin in startle_origins {
            self.emit_catch_startle(origin);
        }
        for (bpos, is_tide) in boss_catches {
            self.on_boss_caught(bpos, is_tide);
        }
        if any_caught {
            self.check_milestone(&mut rand::rng());
        }
    }

    /// Big celebratory payoff when a worn-down boss is finally snagged. `is_tide` swaps the callout
    /// and shockwave color so the Tide Boss reads as its own catch, not a reskinned King Crab.
    fn on_boss_caught(&mut self, pos: Vec2, is_tide: bool) {
        let mut rng = rand::rng();
        let bonus = 25 * self.combo_multiplier();
        self.score += bonus;
        self.particle_system
            .spawn_milestone_fireworks(pos, 30, &mut rng);
        let screen_center = Vec2::new(self.width / 2.0 - 200.0, self.height / 2.0 - 90.0);
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
        self.floating_texts.spawn(
            label.to_string(),
            screen_center,
            64.0,
            label_color,
        );
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
        self.beat_intensity = 2.0;
        self.on_beat_flash = 0.6;
        if self.catch_shockwaves.len() < 48 {
            self.catch_shockwaves.push((pos, 0.0, shock_color));
        }
    }

    /// A Tide Boss pulse detonates at `center`: an expanding shockwave ring that shoves every
    /// nearby *free* crab outward into a panic, and — if the conga train's tail is caught inside the
    /// blast — knocks the last few links loose (the Tide Boss's version of a chain snap). The threat
    /// is spacing: keep your train out of the ring and the pulse does nothing, so it rewards reading
    /// the swell telegraph and pulling back rather than routing out of a charge lane.
    fn tide_pulse_burst(&mut self, center: Vec2) {
        const TIDE_SNAP_LINKS: usize = 4; // a solid surge tears off a bit more than a panic-brush snap
        let r2 = TIDE_PULSE_RADIUS * TIDE_PULSE_RADIUS;

        // Spawn the visible expanding ring (bounded so a stall can't grow the Vec without limit).
        if self.tide_pulses.len() < 8 {
            self.tide_pulses.push((center, crate::CRAB_SIZE));
        }

        // Shove every free crab in range outward and startle it into a flee.
        let mut scattered: Vec<Vec2> = Vec::new();
        for crab in &mut self.crabs {
            if crab.caught || crab.is_boss() {
                continue;
            }
            let d2 = crab.pos.distance_squared(center);
            if d2 > r2 {
                continue;
            }
            let outward = (crab.pos - center).normalize_or_zero();
            let outward = if outward == Vec2::ZERO { Vec2::new(0.0, 1.0) } else { outward };
            crab.fleeing = true;
            crab.startle_timer = crab.startle_timer.max(0.7);
            crab.charm_timer = 0.0; // the surge overwhelms a whistle's calm
            crab.vel = outward * crab.crab_type.speed_range().end * 2.0;
            crab.speed = 1.0; // vel encodes full speed, matching the flee/startle convention
            if scattered.len() < 24 {
                scattered.push(crab.pos);
            }
        }

        // Knock the tail loose if any caught link sits inside the blast. Mirrors snap_chain_on_panic
        // but triggered by the pulse's reach rather than a physical tail collision.
        let tail_in_blast = self
            .crabs
            .iter()
            .any(|c| c.caught && c.chain_index.is_some() && c.pos.distance_squared(center) <= r2);
        if tail_in_blast && self.chain_count >= 5 && self.chain_snap_cooldown <= 0.0 {
            let keep = self.chain_count.saturating_sub(TIDE_SNAP_LINKS).max(1);
            let snapped = self.chain_count - keep;
            let mut snapped_positions: Vec<Vec2> = Vec::new();
            for crab in &mut self.crabs {
                let Some(ci) = crab.chain_index else { continue };
                if ci >= keep {
                    crab.caught = false;
                    crab.chain_index = None;
                    crab.fleeing = true;
                    crab.startle_timer = 0.6;
                    let outward = (crab.pos - center).normalize_or_zero();
                    let outward = if outward == Vec2::ZERO { Vec2::new(0.0, 1.0) } else { outward };
                    crab.vel = outward * crab.crab_type.speed_range().end * 2.2;
                    crab.speed = 1.0;
                    snapped_positions.push(crab.pos);
                }
            }
            self.chain_count = keep;
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
        }

        // Feedback for the scattered herd.
        for pos in &scattered {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((*pos, 0.0));
            }
        }
        self.spawn_catch_shockwave(center, [0.3, 0.75, 1.0]);
        self.screen_shake = self.screen_shake.max(16.0);
        let a = rand::rng().random_range(0.0_f32..std::f32::consts::TAU);
        self.screen_shake_vel = Vec2::new(a.cos(), a.sin()) * 12.0 * 60.0;
        self.on_beat_flash = self.on_beat_flash.max(0.35);
    }

    fn catch_by_chain(&mut self, ctx: &mut Context) {
        let catch_radius = 45.0 + self.catch_radius_upgrade;

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
            ((p.x / cell_size).floor() as i32, (p.y / cell_size).floor() as i32)
        };
        for bucket in self.catch_grid_buf.values_mut() {
            bucket.clear();
        }
        for (i, c) in self.crabs.iter().enumerate() {
            if c.is_catchable() {
                self.catch_grid_buf.entry(cell_of(c.pos)).or_default().push(i);
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
            self.crabs[i].caught = true;
            if self.crabs[i].is_boss() {
                self.on_boss_caught(pos, self.crabs[i].is_tide_boss());
            }
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
            play_catch_sound(&mut self.sounds, ctx, &mut rng);
            if self.score > 0 && self.score % 10 == 0 {
                let _ = self.sounds.upgrade.play_detached(ctx);
                self.pending_upgrade = true;
            }
        }
    }

    fn update_crabs(&mut self, dt: f32, area: (f32, f32)) {
        // Calculate flashlight direction.
        let flashlight_dir = (self.mouse_pos - self.player_pos).normalize_or_zero();

        let base_cone_angle = std::f32::consts::FRAC_PI_3;
        let base_range = 320.0;

        let flashlight_cone_angle = base_cone_angle + self.flashlight.cone_upgrade;
        let flashlight_range = base_range + self.flashlight.range_upgrade;
        // Beam-lane-scaled boss/shell drain, read once so the &mut self.crabs loop can use it.
        let boss_drain = self.boss_drain_rate();

        // Event-collection scratch buffers, reused every frame (see field docs) instead of
        // being freshly allocated here — most frames leave every one of these empty. Taken out
        // (rather than borrowed) so the later celebration loops are free to call back into
        // methods that need a full `&mut self`; the buffers (and their capacity) are restored
        // at the end of this function so next frame reuses the same allocation.
        // Positions of crabs that just entered panic-flee this frame — we'll emit "!" pops after the loop
        let mut flee_pops = std::mem::take(&mut self.flee_pops_buf);
        flee_pops.clear();
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
        // Tide Boss pulse fires this frame (center positions) — processed after the loop so the
        // shockwave can scatter the herd and loosen the train without fighting the &mut borrow.
        // Reused scratch buffers like the other event vecs above: almost always empty (at most
        // one boss pulsing at a time), so taking/restoring avoids a Vec::new() every frame.
        let mut tide_fires = std::mem::take(&mut self.tide_fires_buf);
        tide_fires.clear();
        let mut tide_swells = std::mem::take(&mut self.tide_swells_buf); // a pulse just started swelling — telegraph feedback
        tide_swells.clear();

        // Where the King Crab aims: the exposed tail of the conga train if there is one, else the
        // player. Computed before the mutable loop so the boss branch can read it freely.
        let chain_tail_pos = self
            .crabs
            .iter()
            .filter(|c| c.caught && c.chain_index.is_some())
            .max_by_key(|c| c.chain_index)
            .map(|c| c.pos);
        let charge_target = chain_tail_pos.unwrap_or(self.player_pos);

        for crab in &mut self.crabs {
            // King Crab boss runs its own charge AI instead of the herd flee/attract logic.
            if crab.is_boss() && !crab.caught {
                crab.spawn_time += dt;
                let distance = self.player_pos.distance(crab.pos);
                let to_crab = (crab.pos - self.player_pos).normalize_or_zero();
                let angle_to_crab = flashlight_dir.angle_between(to_crab).abs();
                let crab_in_light = self.flashlight.on
                    && distance < flashlight_range
                    && angle_to_crab < flashlight_cone_angle;
                crab.in_flashlight = crab_in_light;

                // Wearing it down under the beam is unchanged — the beam is still how you catch it.
                if crab.boss_health > 0.0 && crab_in_light {
                    crab.boss_health -= boss_drain * dt;
                    if crab.boss_health <= 0.0 {
                        crab.boss_health = 0.0;
                        boss_broke.push(crab.pos);
                    }
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
                                crab.charge_cooldown = TIDE_PULSE_COOLDOWN;
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
                        while delta > std::f32::consts::PI { delta -= std::f32::consts::TAU; }
                        while delta < -std::f32::consts::PI { delta += std::f32::consts::TAU; }
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
                        if crab.charge_cooldown <= 0.0
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
                            crab.vel = dir * BOSS_CHARGE_SPEED;
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
                        crab.charge_state = if nt <= 0.0 {
                            crab.charge_cooldown = BOSS_CHARGE_COOLDOWN;
                            crab.vel *= 0.15; // skid to a halt out of the lunge
                            BossCharge::Idle
                        } else {
                            BossCharge::Charging(nt)
                        };
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
                    while delta > std::f32::consts::PI { delta -= std::f32::consts::TAU; }
                    while delta < -std::f32::consts::PI { delta += std::f32::consts::TAU; }
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
                if crab.boss_health > 0.0 && crab_in_light {
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

                // Panic flee: crabs that are close but outside the flashlight beam scatter away.
                // Bosses are unshakeable — they lumber on rather than panic-bolting.
                const FLEE_RADIUS: f32 = 220.0;
                // A whistle-charmed crab holds its nerve near the player instead of bolting, so a
                // well-timed pulse pins a spooked herd in place long enough to sweep them up.
                // Dancer crabs don't panic-flee continuously — their escape is the beat hop
                // (handled in the beat-fire block), so between beats they hold still instead of
                // streaming away. This is what makes them a rhythm-timed grab rather than a chase.
                let now_fleeing = !crab_in_light
                    && distance < FLEE_RADIUS
                    && !crab.is_boss()
                    && !crab.is_dancer()
                    && crab.charm_timer <= 0.0;

                if crab_in_light {
                    // Crab is gently attracted to the player's position (sauntering, not rocketing)
                    let toward_dir = (self.player_pos - crab.pos).normalize_or_zero();
                    let max_speed = crab.crab_type.speed_range().end;
                    let min_speed = crab.crab_type.speed_range().start;

                    // Instead of instantly max speed, interpolate velocity and use a gentle boost.
                    let gentle_speed = min_speed + (max_speed - min_speed) * 0.10;
                    crab.vel = crab.vel.lerp(toward_dir * gentle_speed, 0.01);
                    crab.speed = gentle_speed;
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

                // If player is within 150 pixels and crab is in the light, add a small extra speed boost
                let mut speed_multiplier = 1.0;
                if crab_in_light && distance < 150.0 {
                    speed_multiplier = 2.0 - (distance / 150.0);
                    speed_multiplier = speed_multiplier.clamp(1.0, 2.0);
                }

                // Older crabs are faster so the player should catch them early.
                let age_boost = 1.0 + (crab.spawn_time / 10.0).min(1.5);
                crab.pos += crab.vel * crab.speed * speed_multiplier * age_boost * dt;

                // Beat-synced positional wobble for idle (non-spooked) crabs.
                if crab.spooked_timer == 0.0 {
                    let beat_phase = (1.0 - self.beat_timer / BEAT_INTERVAL)
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

                // Smoothly rotate crab to face its movement direction
                let speed = crab.vel.length();
                if speed > 5.0 {
                    let target_angle = crab.vel.y.atan2(crab.vel.x);
                    let mut delta = target_angle - crab.facing_angle;
                    while delta > std::f32::consts::PI { delta -= std::f32::consts::TAU; }
                    while delta < -std::f32::consts::PI { delta += std::f32::consts::TAU; }
                    crab.facing_angle += delta * (dt * 8.0).min(1.0);
                }

                // Collect sparkle particles drifting toward player for attracted crabs
                if crab_in_light {
                    let mut rng = rand::rng();
                    // ~2 sparkles per second (probabilistic)
                    if rng.random_range(0.0_f32..1.0_f32) < dt * 2.0 {
                        let toward_player = (self.player_pos - crab.pos).normalize_or_zero();
                        let perp = Vec2::new(-toward_player.y, toward_player.x);
                        let spread = rng.random_range(-0.6_f32..0.6_f32);
                        let dir = (toward_player + perp * spread).normalize();
                        let speed = rng.random_range(40.0_f32..90.0_f32);
                        let life = rng.random_range(0.4_f32..0.8_f32);
                        let color = crab.crab_color();
                        attraction_particles.push((crab.pos, dir * speed, life, color));
                    }
                }
            }
        }

        // Push sparkle particles for attracted crabs (done outside loop to avoid borrow conflict)
        for &(pos, vel, life, [cr, cg, cb]) in attraction_particles.iter() {
            self.particle_system.push(crate::graphics::Particle {
                pos,
                vel,
                life,
                max_life: life,
                size: rand::rng().random_range(1.5_f32..3.5_f32),
                color: [(cr * 0.6 + 0.4).min(1.0), (cg * 0.6 + 0.4).min(1.0), (cb * 0.6 + 0.4).min(1.0)],
            });
        }

        // Celebrate any King Crab worn down to catchable this frame
        for &pos in boss_broke.iter() {
            self.floating_texts.spawn(
                "WORN DOWN — CATCH IT!".to_string(),
                pos - Vec2::new(110.0, 46.0),
                34.0,
                [0.4, 1.0, 0.5, 1.0],
            );
            self.spawn_catch_shockwave(pos, [1.0, 0.85, 0.3]);
            self.screen_shake = self.screen_shake.max(14.0);
            self.on_beat_flash = self.on_beat_flash.max(0.4);
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

        // Armored shells the beam just wore through — a lighter "crack" than the boss fanfare
        for &pos in armor_broke.iter() {
            self.floating_texts.spawn(
                "SHELL CRACKED!".to_string(),
                pos - Vec2::new(70.0, 40.0),
                26.0,
                [0.7, 0.85, 1.0, 1.0],
            );
            self.spawn_catch_shockwave(pos, [0.7, 0.8, 0.95]);
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

        // Hand the scratch buffers back so next frame's std::mem::take reuses this frame's
        // allocation instead of starting from an empty Vec.
        self.flee_pops_buf = flee_pops;
        self.boss_broke_buf = boss_broke;
        self.armor_broke_buf = armor_broke;
        self.attraction_particles_buf = attraction_particles;
        self.boss_windups_buf = boss_windups;
        self.boss_launches_buf = boss_launches;
        self.boss_charge_dust_buf = boss_charge_dust;
        self.tide_fires_buf = tide_fires;
        self.tide_swells_buf = tide_swells;

        // Move chain crabs to their historical positions (conga train). Walking self.crabs
        // mutably and consulting self.position_history in the same pass (rather than
        // collecting an intermediate Vec<(usize, Vec2)> of chain targets first) avoids a
        // per-frame heap allocation that used to scale with conga chain length.
        let mut dust_rng = rand::rng();
        for crab in &mut self.crabs {
            let Some(ci) = crab.chain_index else { continue };
            let history_idx = (ci + 1) * CHAIN_LINK_FRAMES;
            let Some(&target) = self.position_history.get(history_idx) else { continue };
            let old_pos = crab.pos;
            crab.pos = old_pos.lerp(target, 0.4);
            // Rotate caught crab toward the direction it just moved
            let move_dir = crab.pos - old_pos;
            // Kick up a little dust from the crab's feet as the conga train stampedes along.
            let feet = crab.pos + Vec2::new(0.0, CRAB_SIZE * 0.35);
            self.particle_system
                .spawn_conga_dust(feet, move_dir, dt, &mut dust_rng);
            if move_dir.length() > 0.5 {
                let target_angle = move_dir.y.atan2(move_dir.x);
                let mut d = target_angle - crab.facing_angle;
                while d > std::f32::consts::PI { d -= std::f32::consts::TAU; }
                while d < -std::f32::consts::PI { d += std::f32::consts::TAU; }
                crab.facing_angle += d * (dt * 6.0).min(1.0);
            }
            // Beat-synced conga step: the train physically hops forward on each beat, and the
            // hop ripples down the line — each link lags the one ahead by a fixed phase — so the
            // whole train visibly steps to the rhythm instead of just gliding after the player.
            // This is gameplay reacting to the beat, not only visuals: the crabs move to it. The
            // lerp above continuously reels each crab back to its chain target every frame, so
            // this direct forward offset self-corrects and can never accumulate or drift the
            // train off its path.
            let travel = move_dir.normalize_or_zero();
            if travel != Vec2::ZERO {
                let step_phase = (1.0 - self.beat_timer / BEAT_INTERVAL) * std::f32::consts::TAU
                    - ci as f32 * 0.7;
                let hop = step_phase.sin().max(0.0); // forward-only footfall each beat
                crab.pos += travel * hop * 4.0;
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
        let crabs = spawn_enemies(p.pattern.clone(), p.count, area, p.centroid, &mut rng);
        self.crabs.extend(crabs);
        self.pattern_timer = p.duration;
    }

    fn advance_pattern(&mut self) {
        self.current_pattern += 1;
        let level = &self.levels[self.current_level];
        if self.current_pattern >= level.patterns.len() {
            self.current_level += 1;
            self.current_pattern = 0;
            self.level_title = level.title.clone();
            self.level_title_timer = 1.0;
            // Fresh biome, fresh pen location — keep routing the train there a live decision.
            let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            self.pen_pos = pick_pen_pos(self.width, self.height, player_center, &mut rand::rng());
            // New zone, new water: relocate the tide-pool hazards too, scaling with difficulty.
            let difficulty = self
                .levels
                .get(self.current_level.min(self.levels.len() - 1))
                .map(|l| l.difficulty)
                .unwrap_or(0);
            self.tide_pools = pick_tide_pools(
                self.width,
                self.height,
                self.pen_pos,
                player_center,
                difficulty,
                &mut rand::rng(),
            );
        }
        if self.current_level >= self.levels.len() {
            // Game completed, show game over screen.
            self.game_over = true;
        }
        let area = (self.width, self.height);
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
                "{} {} {} {} {} {} {} {}",
                self.career_best_score,
                self.career_total_score,
                self.career_runs,
                self.career_spent,
                self.start_beam_rank,
                self.start_lasso_rank,
                self.start_whistle_rank,
                self.start_stomp_rank,
            ),
        );
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
        let width = self.width;
        let height = self.height;
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
        self.beat_timer = BEAT_INTERVAL;
        self.beat_intensity = 0.0;
        self.music_intensity = 0.0;
        self.on_beat_flash = 0.0;
        self.groove = 0.0;
        self.beat_streak = 0;
        self.catch_radius_upgrade = 0.0;
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
        self.beat_wave_active = false;
        self.beat_wave_radius = 0.0;
        self.wave_armed = false;
        self.wave_telegraph = 0.0;
        self.lasso_pos = None;
        self.lasso_timer = 0.0;
        self.lasso_target = Vec2::ZERO;
        self.whistle_active = 0.0;
        self.whistle_radius = 0.0;
        self.whistle_cooldown = 0.0;
        self.whistle_beat_bonus = 1.0;
        self.stomp_active = 0.0;
        self.stomp_radius = 0.0;
        self.stomp_cooldown = 0.0;
        self.stomp_beat_bonus = 1.0;
        self.call_cooldown = 0.0;
        self.call_pulse = 0.0;
        self.dash_just_fired = false;
        self.dash_flash = 0.0;
        self.screen_shake = 0.0;
        self.screen_shake_vel = Vec2::ZERO;
        self.screen_shake_offset = Vec2::ZERO;
        self.hitstop_timer = 0.0;
        self.chain_join_ripple = false;
        self.next_milestone = 5;
        self.next_boss_score = BOSS_SCORE_INTERVAL;
        self.next_boss_is_tide = false;
        self.deliver_flash = 0.0;
        self.pen_pos = pick_pen_pos(
            self.width,
            self.height,
            player_pos + Vec2::splat(PLAYER_SIZE / 2.0),
            &mut rand::rng(),
        );
        self.tide_pools = pick_tide_pools(
            self.width,
            self.height,
            self.pen_pos,
            player_pos + Vec2::splat(PLAYER_SIZE / 2.0),
            self.levels.first().map(|l| l.difficulty).unwrap_or(0),
            &mut rand::rng(),
        );
        self.in_tide_pool = false;
        self.chain_rings.clear();
        self.catch_shockwaves.clear();
        self.fear_rings.clear();
        self.tide_pulses.clear();
        self.player_pos = player_pos;
        self.score = 0;
        self.spawn_timer = 0.0;
        self.time_elapsed = 0.0;
        self.game_over = false;
        self.run_recorded = false;
        self.run_is_new_best = false;
        self.boost_timer = 0.0;
        self.boost_cooldown = 0.0;
        self.current_level = 0;
        self.current_pattern = 0;
        self.start_current_pattern((width, height));
    }

    fn draw_instructions_screen(
        &mut self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        width: f32,
        height: f32,
    ) -> GameResult {
        use ggez::graphics::DrawMode;
        let t = self.menu_time;

        // --- Moonlit-beach gradient backdrop ------------------------------------------------
        // Stacked horizontal strips fade deep night-navy at the top through a dusky plum into a
        // warm strip of sand at the bottom, so the menu reads as the same seaside world the game
        // is played in rather than a flat black card. ~28 strips is plenty smooth and cheap.
        let strips = 28;
        let top = Color::from_rgb(9, 12, 34); // deep night sky
        let mid = Color::from_rgb(48, 26, 66); // dusky plum horizon
        let sand = Color::from_rgb(74, 58, 78); // muted moonlit sand
        let lerp = |a: Color, b: Color, k: f32| {
            Color::new(
                a.r + (b.r - a.r) * k,
                a.g + (b.g - a.g) * k,
                a.b + (b.b - a.b) * k,
                1.0,
            )
        };
        // Every strip is the same (width x strip_h+1) rectangle, just moved down and recolored,
        // so — same trick as the rest of the codebase's UNIT_SQUARE usage — draw the single
        // cached unit-square mesh 28 times with a per-strip dest/scale/color instead of building
        // 28 fresh `Mesh::new_rectangle` GPU buffers every single frame this screen is up.
        let strip_h = height / strips as f32;
        let strip_square = unit_square(ctx)?;
        for i in 0..strips {
            let k = i as f32 / (strips - 1) as f32;
            // Two-segment gradient: sky->horizon over the top 65%, horizon->sand below it.
            let c = if k < 0.65 {
                lerp(top, mid, k / 0.65)
            } else {
                lerp(mid, sand, (k - 0.65) / 0.35)
            };
            canvas.draw(
                strip_square,
                DrawParam::default()
                    .dest(Vec2::new(0.0, i as f32 * strip_h))
                    .scale(Vec2::new(width, strip_h + 1.0))
                    .color(c),
            );
        }

        // Reusable dot mesh for stars and the moon halo — the same cached unit circle the rest
        // of graphics.rs's particle/ring drawing uses, instead of a fresh `Mesh::new_circle` GPU
        // buffer every frame this screen is up.
        let dot = unit_circle(ctx)?;

        // --- Twinkling stars ----------------------------------------------------------------
        // Deterministic positions from a cheap integer hash so the field is stable frame to
        // frame; each star breathes on its own phase/speed for a lively night sky.
        let hash = |n: u32| {
            let mut x = n.wrapping_mul(2654435761);
            x ^= x >> 15;
            x = x.wrapping_mul(2246822519);
            x ^= x >> 13;
            x
        };
        for i in 0..70u32 {
            let sx = (hash(i) % 1000) as f32 / 1000.0 * width;
            let sy = (hash(i * 7 + 1) % 1000) as f32 / 1000.0 * height * 0.6;
            let phase = (hash(i * 13 + 3) % 628) as f32 / 100.0;
            let speed = 1.2 + (hash(i * 17 + 5) % 200) as f32 / 100.0;
            let twinkle = 0.25 + 0.75 * (t * speed + phase).sin().abs();
            let r = 0.7 + (hash(i * 19 + 7) % 100) as f32 / 100.0 * 1.6;
            canvas.draw(
                dot,
                DrawParam::default()
                    .dest(Vec2::new(sx, sy))
                    .scale(Vec2::splat(r))
                    .color(Color::new(1.0, 1.0, 0.92, twinkle)),
            );
        }

        // --- Soft moon with a glowing halo --------------------------------------------------
        let moon_pos = Vec2::new(width * 0.82, height * 0.2);
        for ring in (0..6).rev() {
            let rr = 34.0 + ring as f32 * 16.0;
            let a = 0.05 + (5 - ring) as f32 * 0.03;
            canvas.draw(
                dot,
                DrawParam::default()
                    .dest(moon_pos)
                    .scale(Vec2::splat(rr))
                    .color(Color::new(0.95, 0.93, 0.8, a)),
            );
        }
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(moon_pos)
                .scale(Vec2::splat(30.0))
                .color(Color::new(0.98, 0.96, 0.86, 1.0)),
        );

        // --- A conga line of crabs marching across the sand ---------------------------------
        // Reuses the in-game crab renderer so the menu previews exactly what you play, forming a
        // little train that scuttles across the bottom and wraps around — the game's whole hook in
        // one glance. Positions/bob/facing are all driven by menu_time so it moves while paused.
        let march_y = height - 66.0;
        let march_speed = 70.0;
        let spacing = 74.0;
        let march_types = [
            CrabType::Normal,
            CrabType::Fast,
            CrabType::Big,
            CrabType::Sneaky,
            CrabType::Armored,
            CrabType::Dancer,
        ];
        for (i, ctype) in march_types.iter().enumerate() {
            // Lead crab walks the parade; each follower trails by `spacing`, all wrapping across
            // a span a bit wider than the screen so they enter/exit smoothly.
            let span = width + spacing * march_types.len() as f32;
            let x = ((t * march_speed + i as f32 * spacing) % span) - spacing;
            let bob = (t * 6.0 + i as f32 * 0.9).sin() * 5.0;
            let deco = EnemyCrab {
                pos: Vec2::new(x, march_y),
                vel: Vec2::new(march_speed, 0.0),
                speed: 60.0,
                caught: true,
                chain_index: Some(i),
                scale: 0.5,
                spawn_time: 10.0,
                crab_type: *ctype,
                spooked_timer: 0.0,
                beat_phase_offset: 0.0,
                join_pulse: 0.0,
                fleeing: false,
                facing_angle: 0.0,
                in_flashlight: false,
                startle_timer: 0.0,
                charm_timer: 0.0,
                answering_call: 0.0,
                boss_health: 0.0,
                charge_state: BossCharge::Idle,
                charge_cooldown: 0.0,
            };
            let beat_phase = (t * 4.0 + i as f32 * 0.5).sin().abs();
            draw_crab(
                ctx,
                canvas,
                &deco,
                Vec2::new(x, march_y - bob),
                beat_phase,
                0.0,
                bob.max(0.0),
                0.0,
            )?;
        }
        // Flush the batched leg/body draws for the march crabs above (see flush_crab_legs and
        // flush_crab_bodies doc comments).
        crate::graphics::flush_crab_legs(ctx, canvas)?;
        crate::graphics::flush_crab_bodies(ctx, canvas)?;

        // --- Title: "Crab Rustler" with an animated colour wave -----------------------------
        let (main_title_width, main_title_height) = MENU_TITLE_CACHE.with(|c| -> GameResult<(f32, f32)> {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                let mut main_title = Text::new("Crab Rustler");
                main_title.set_scale(112.0);
                let dims = main_title.measure(ctx)?;
                *cache = Some((main_title, dims.x, dims.y));
            }
            let (_, w, h) = cache.as_ref().unwrap();
            Ok((*w, *h))
        })?;
        let title_top = height * 0.13;

        // Drop shadow.
        MENU_TITLE_CACHE.with(|c| {
            let cache = c.borrow();
            let (main_title, _, _) = cache.as_ref().unwrap();
            canvas.draw(
                main_title,
                DrawParam::default()
                    .dest(Vec2::new(
                        (width - main_title_width) / 2.0 + 8.0,
                        title_top + 8.0,
                    ))
                    .color(Color::from_rgba(0, 0, 0, 180))
                    .rotation(0.03),
            );
        });

        // Per-character wave that now rolls over time instead of sitting still. The glyphs
        // themselves never change — only position/color/rotation do, all via DrawParam — so
        // build the 12 per-character Text objects once and reuse them forever instead of
        // shaping fresh glyphs every frame.
        MENU_TITLE_CHARS_CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                let chars: Vec<Text> = "Crab Rustler"
                    .chars()
                    .map(|ch| Text::new(ggez::graphics::TextFragment::new(ch).scale(112.0)))
                    .collect();
                *cache = Some(chars);
            }
            for (i, ch_text) in cache.as_ref().unwrap().iter().enumerate() {
                let x = (width - main_title_width) / 2.0 + i as f32 * 60.0;
                let y = title_top + (t * 2.2 + i as f32 * 0.5).sin() * 14.0;
                let hue = t * 0.6 + i as f32 * 0.55;
                let color = Color::from_rgb(
                    (200.0 + hue.sin() * 55.0) as u8,
                    (120.0 + (hue + 2.0).sin() * 110.0) as u8,
                    (200.0 + (hue + 4.0).sin() * 55.0) as u8,
                );
                canvas.draw(
                    ch_text,
                    DrawParam::default()
                        .dest(Vec2::new(x, y))
                        .color(color)
                        .rotation((t * 1.5 + i as f32 * 0.4).sin() * 0.07),
                );
            }
        });

        // Subtitle centred below the title. Rebuilt only when the underlying string changes
        // (it's static for the life of a menu visit, just occasionally different across runs).
        let subtitle_width = MENU_SUBTITLE_CACHE.with(|c| -> GameResult<f32> {
            let mut cache = c.borrow_mut();
            let needs_rebuild = !matches!(&*cache, Some((s, _, _)) if s == &self.subtitle);
            if needs_rebuild {
                let mut subtitle = Text::new(&self.subtitle);
                subtitle.set_scale(22.0);
                let w = subtitle.measure(ctx)?.x;
                *cache = Some((self.subtitle.clone(), subtitle, w));
            }
            Ok(cache.as_ref().unwrap().2)
        })?;
        MENU_SUBTITLE_CACHE.with(|c| {
            let cache = c.borrow();
            let (_, subtitle, _) = cache.as_ref().unwrap();
            canvas.draw(
                subtitle,
                DrawParam::default()
                    .dest(Vec2::new(
                        (width - subtitle_width) / 2.0,
                        title_top + main_title_height + 14.0,
                    ))
                    .color(Color::from_rgb(255, 235, 190)),
            );
        });

        // --- Instructions on a translucent rounded panel for readability -------------------
        let (text_width, text_height) = MENU_INSTRUCTIONS_CACHE.with(|c| -> GameResult<(f32, f32)> {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                let text = Text::new(
                    "Catch all the crabs!\n\nMove: Arrow keys / WASD\nAim flashlight: Mouse\nDash: Space\nThrow lasso: Left click\nBeat wave burst: Q\nWhistle (pulls crabs in): E\nStomp (cracks armored crabs): R\nCall on the beat (Dancers answer): F",
                );
                let dims = text.measure(ctx)?;
                *cache = Some((text, dims.x, dims.y));
            }
            let (_, w, h) = cache.as_ref().unwrap();
            Ok((*w, *h))
        })?;
        let text_x = (width - text_width) / 2.0;
        let text_y = height * 0.44;
        let pad = 26.0;
        let panel_key = (width.to_bits(), height.to_bits());
        let cached_panel = MENU_PANEL_CACHE.with(|c| {
            c.borrow().as_ref().and_then(|(w, h, mesh)| {
                (*w == panel_key.0 && *h == panel_key.1).then(|| mesh.clone())
            })
        });
        let panel = match cached_panel {
            Some(mesh) => mesh,
            None => {
                let mesh = Mesh::new_rounded_rectangle(
                    ctx,
                    DrawMode::fill(),
                    Rect::new(
                        text_x - pad,
                        text_y - pad,
                        text_width + pad * 2.0,
                        text_height + pad * 2.0,
                    ),
                    14.0,
                    Color::from_rgba(10, 14, 30, 170),
                )?;
                MENU_PANEL_CACHE.with(|c| {
                    *c.borrow_mut() = Some((panel_key.0, panel_key.1, mesh.clone()))
                });
                mesh
            }
        };
        canvas.draw(&panel, DrawParam::default());
        MENU_INSTRUCTIONS_CACHE.with(|c| {
            let cache = c.borrow();
            let (text, _, _) = cache.as_ref().unwrap();
            canvas.draw(
                text,
                DrawParam::default()
                    .dest(Vec2::new(text_x, text_y))
                    .color(Color::from_rgb(255, 246, 210)),
            );
        });

        // --- Pulsing "Press Space or Enter to start" prompt --------------------------------
        let pulse = 0.55 + 0.45 * (t * 3.0).sin().abs();
        let prompt_width = MENU_PROMPT_CACHE.with(|c| -> GameResult<f32> {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                let mut prompt = Text::new("Press Space or Enter to start");
                prompt.set_scale(30.0);
                let w = prompt.measure(ctx)?.x;
                *cache = Some((prompt, w));
            }
            Ok(cache.as_ref().unwrap().1)
        })?;
        MENU_PROMPT_CACHE.with(|c| {
            let cache = c.borrow();
            let (prompt, _) = cache.as_ref().unwrap();
            canvas.draw(
                prompt,
                DrawParam::default()
                    .dest(Vec2::new(
                        (width - prompt_width) / 2.0,
                        text_y + text_height + pad * 2.0 + 22.0,
                    ))
                    .color(Color::new(1.0, 0.9, 0.25, pulse)),
            );
        });

        // --- Career line: the persistent thread across runs -------------------------------
        // Only surfaces once there's a career to show, so a brand-new player sees a clean title.
        // Reminds returning players what they're building toward before they hit start.
        if self.career_runs > 0 {
            let cw = CAREER_LABEL_CACHE.with(|c| -> GameResult<f32> {
                let mut cache = c.borrow_mut();
                let needs_rebuild = match cache.as_ref() {
                    Some((best, total, runs, _, _)) => {
                        *best != self.career_best_score
                            || *total != self.career_total_score
                            || *runs != self.career_runs
                    }
                    None => true,
                };
                if needs_rebuild {
                    let mut career = Text::new(format!(
                        "Career best {}   ·   {} crabs banked over {} runs",
                        self.career_best_score, self.career_total_score, self.career_runs
                    ));
                    career.set_scale(22.0);
                    let cw = career.measure(ctx)?.x;
                    *cache = Some((
                        self.career_best_score,
                        self.career_total_score,
                        self.career_runs,
                        career,
                        cw,
                    ));
                }
                Ok(cache.as_ref().unwrap().4)
            })?;
            CAREER_LABEL_CACHE.with(|c| {
                let cache = c.borrow();
                let (_, _, _, career, _) = cache.as_ref().unwrap();
                canvas.draw(
                    career,
                    DrawParam::default()
                        .dest(Vec2::new(
                            (width - cw) / 2.0,
                            text_y + text_height + pad * 2.0 + 62.0,
                        ))
                        .color(Color::from_rgb(200, 190, 230)),
                );
            });

            // --- Perk shop: spend the banked crabs on permanent starting ranks -----------------
            // The spend side of meta-progression. Turns the career total from a passive counter
            // into a currency, so even a losing run buys you closer to a permanent head-start.
            let available = self.career_available();
            let ranks = (
                available,
                self.start_beam_rank,
                self.start_lasso_rank,
                self.start_whistle_rank,
                self.start_stomp_rank,
            );
            let (header_w, list_w) = SHOP_CACHE.with(|c| -> GameResult<(f32, f32)> {
                let mut cache = c.borrow_mut();
                let needs_rebuild = !matches!(cache.as_ref(), Some((k, ..)) if *k == ranks);
                if needs_rebuild {
                    let mut header = Text::new(format!(
                        "SPEND {} banked crabs on permanent gear:",
                        available
                    ));
                    header.set_scale(21.0);
                    let hw = header.measure(ctx)?.x;
                    let perk = |name: &str, key: char, rank: u32| -> String {
                        match Self::perk_cost(rank) {
                            Some(cost) => format!("[{}] {} Lv{} → {}crabs", key, name, rank, cost),
                            None => format!("[{}] {} MAX", key, name),
                        }
                    };
                    let mut list = Text::new(format!(
                        "{}    {}    {}    {}",
                        perk("Beam", '1', self.start_beam_rank),
                        perk("Lasso", '2', self.start_lasso_rank),
                        perk("Whistle", '3', self.start_whistle_rank),
                        perk("Stomp", '4', self.start_stomp_rank),
                    ));
                    list.set_scale(19.0);
                    let lw = list.measure(ctx)?.x;
                    *cache = Some((ranks, header, hw, list, lw));
                }
                let cr = cache.as_ref().unwrap();
                Ok((cr.2, cr.4))
            })?;
            // Green when a buy just landed, red when one was refused, otherwise a calm teal.
            let list_color = if self.shop_flash > 0.0 {
                Color::new(0.5 + 0.5 * self.shop_flash, 1.0, 0.5, 1.0)
            } else if self.shop_denied > 0.0 {
                Color::new(1.0, 0.5 - 0.3 * self.shop_denied, 0.5 - 0.3 * self.shop_denied, 1.0)
            } else {
                Color::from_rgb(150, 220, 210)
            };
            let shop_y = text_y + text_height + pad * 2.0 + 92.0;
            SHOP_CACHE.with(|c| {
                let cache = c.borrow();
                let (_, header, _, list, _) = cache.as_ref().unwrap();
                canvas.draw(
                    header,
                    DrawParam::default()
                        .dest(Vec2::new((width - header_w) / 2.0, shop_y))
                        .color(Color::from_rgb(180, 175, 205)),
                );
                canvas.draw(
                    list,
                    DrawParam::default()
                        .dest(Vec2::new((width - list_w) / 2.0, shop_y + 28.0))
                        .color(list_color),
                );
            });
        }
        Ok(())
    }

    fn draw_game(
        &self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        width: f32,
        height: f32,
    ) -> GameResult {
        // Select texture for current level.
        let texture = match self.level_textures[self.current_level] {
            LevelTexture::Grass => &self.textures.grass,
            LevelTexture::Sand => &self.textures.sand,
        };

        // Biome for the current zone (clamped so a finished run doesn't index past the end).
        let biome = self.levels[self.current_level.min(self.levels.len() - 1)].biome;
        let (tr, tg, tb) = biome.tint;

        // Draw level background, color-graded to the current biome.
        draw_grass(
            ctx,
            canvas,
            width,
            height,
            texture,
            &self.shader,
            self.time_elapsed,
            Color::from_rgb(tr, tg, tb),
        )?;

        // Subtle beat pulse: an on-beat flash tinted to match the current biome's mood.
        if self.beat_intensity > 0.0 {
            let pulse_alpha = (self.beat_intensity * 28.0) as u8;
            let (pr, pg, pb) = biome.pulse;
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .scale(Vec2::new(width, height))
                    .color(Color::from_rgba(pr, pg, pb, pulse_alpha)),
            );
        }

        // Tide pools — terrain hazards on the ground layer, under the crabs/rope, so the train
        // visibly wades through the water it's being routed around.
        draw_tide_pools(
            ctx,
            canvas,
            &self.tide_pools,
            self.time_elapsed,
            self.beat_intensity,
            self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0),
        )?;

        // Delivery pen — drawn on the ground layer under the crabs/rope so the train visibly rolls
        // into it. Lights up green once there's a train to bank (chain_count > 0).
        draw_delivery_pen(
            ctx,
            canvas,
            self.pen_pos,
            PEN_RADIUS,
            self.time_elapsed,
            self.beat_intensity,
            self.chain_count > 0,
            self.deliver_flash,
        )?;

        // Draw beat ghost rings under the rope and crabs
        draw_chain_rings(ctx, canvas, &self.chain_rings)?;
        // Collect chain crab (chain_index, pos) pairs sorted by chain index into a persisted
        // scratch buffer instead of a fresh Vec<&EnemyCrab> every frame (see CHAIN_SORT_BUF).
        CHAIN_SORT_BUF.with(|buf| -> GameResult {
            let mut chain_links = buf.borrow_mut();
            chain_links.clear();
            chain_links.extend(
                self.crabs
                    .iter()
                    .filter(|c| c.caught && c.chain_index.is_some())
                    .map(|c| (c.chain_index.unwrap_or(0), c.pos)),
            );
            chain_links.sort_by_key(|&(idx, _)| idx);
            draw_conga_rope(ctx, canvas, self.player_pos, &chain_links, self.time_elapsed, self.beat_intensity)
        })?;

        // Draw player character.
        draw_rustler(
            ctx,
            canvas,
            self.player_pos,
            &self.textures.player,
            self.player_vel,
            self.beat_intensity,
            self.time_elapsed,
            self.boost_timer > 0.0,
        )?;

        // Speed lines trailing behind player while dashing. Uses the cached unit-line mesh
        // (see draw_speed_lines) instead of building up to 7 fresh Mesh::new_line GPU buffers
        // every single frame of the dash window.
        if self.boost_timer > 0.0 && self.last_dir.length() > 0.01 {
            let center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            let intensity = self.boost_timer / 0.18;
            draw_speed_lines(ctx, canvas, center, self.last_dir, intensity)?;
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
            )?;
        }

        // Draw all crabs.
        self.draw_crabs_with_shake(ctx, canvas)?;

        // Draw screen-edge radar arrows pointing to free crabs
        draw_crab_radar(ctx, canvas, &self.crabs, width, height, self.beat_intensity, self.time_elapsed)?;

        // Draw catch impact shockwaves (over the crabs, under score text)
        draw_catch_shockwaves(ctx, canvas, &self.catch_shockwaves)?;

        // Draw stampede fear rings where catches startled the herd
        draw_fear_rings(ctx, canvas, &self.fear_rings)?;

        // Draw Tide Boss shockwave pulses sweeping outward
        draw_tide_pulses(ctx, canvas, &self.tide_pulses, TIDE_PULSE_RADIUS)?;

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

        // Draw the rhythm Call summon pulse — magenta rings collapsing toward the player.
        if self.call_pulse > 0.0 {
            draw_call_ring(ctx, canvas, self.call_pulse_center, self.call_pulse, 420.0)?;
        }

        // Draw lasso line and tip
        if let Some(tip) = self.lasso_pos {
            let player_center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            // Determine outward progress (0..1) and spin angle
            let elapsed = 0.5 - self.lasso_timer;
            let outward_progress = (elapsed / 0.3).clamp(0.0, 1.0);
            let spin = self.time_elapsed * 18.0; // fast spin in radians/sec
            draw_lasso(ctx, canvas, player_center, tip, outward_progress, spin)?;
        }

        // Show stats. The HUD line (score/train/combo) only changes on catch/combo events, not
        // every tick, so cache the built Text and only rebuild it (fresh format! String + fresh
        // Text, which re-triggers glyph shaping) when the underlying values actually differ from
        // last frame's — same pattern as the per-level label cache above. Also use the
        // already-maintained self.chain_count instead of re-scanning every crab for `.caught`
        // every frame just to display the same number (crabs are never removed from the vec —
        // caught state only flips via chain_count-tracked catches/snaps — so the two stay in
        // sync).
        let chain_len = self.chain_count;
        let mult = if self.combo_count >= 3 { self.combo_multiplier() } else { 0 };
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
                    format!("Score: {}  |  Train: {}  |  Combo x{}  [{}x pts]", self.score, chain_len, self.combo_count, mult)
                } else {
                    format!("Score: {}  |  Train: {}", self.score, chain_len)
                };
                *cache = Some((self.score, chain_len, self.combo_count, mult, Text::new(hud)));
            }
            canvas.draw(
                &cache.as_ref().unwrap().4,
                DrawParam::default()
                    .dest(Vec2::new(10.0, 10.0))
                    .color(Color::from_rgb(255, 255, 00)),
            );
        });

        // Draw stamina bar for boost timer/cooldown
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
        STAMINA_LABEL_CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                *cache = Some(Text::new("Stamina (Space)"));
            }
            canvas.draw(
                cache.as_ref().unwrap(),
                DrawParam::default()
                    .dest(Vec2::new(bar_x, bar_y - 22.0))
                    .color(Color::from_rgb(255, 255, 255)),
            );
        });

        // Whistle cooldown bar (E) — fills back up to amber as it recharges, ready when full.
        let wbar_y = bar_y + bar_height + 26.0;
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
        let (wr, wg, wb) = if ready { (255, 210, 90) } else { (150, 110, 40) };
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
                let text = Text::new(if ready { "Whistle (E) READY" } else { "Whistle (E)" });
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
        let (sr, sg, sb) = if sready { (150, 190, 235) } else { (80, 105, 135) };
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
                let text = Text::new(if sready { "Stomp (R) READY" } else { "Stomp (R)" });
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
            let debug_text = Text::new(format!(
                "[DEBUG] Pattern: {} | Time left: {:.2}s",
                pattern_name, self.pattern_timer
            ));
            canvas.draw(
                &debug_text,
                DrawParam::default()
                    .dest(Vec2::new(10.0, 80.0))
                    .color(Color::from_rgb(255, 100, 100)),
            );
        }
        // Beat indicator (top right)
        let beat_center = Vec2::new(width - 50.0, 50.0);
        // Wave-incoming telegraph: while a spawn is armed, ring the beat indicator so the player
        // sees the next herd will land on the coming downbeat. Anticipation climbs across the
        // couple of beats before the drop; the ring throbs with the beat phase.
        if self.wave_armed {
            let anticipation = (self.wave_telegraph / (BEAT_INTERVAL * 4.0)).min(1.0);
            let beat_phase = 1.0 - (self.beat_timer / BEAT_INTERVAL).clamp(0.0, 1.0);
            draw_wave_telegraph(ctx, canvas, beat_center, anticipation, beat_phase)?;
        }
        draw_beat_indicator(ctx, canvas, beat_center, self.beat_intensity, self.time_elapsed)?;

        // Groove meter (top center) — fills as you catch crabs on the beat, glowing and
        // pulsing to the beat once you're in the pocket. Rewards rhythmic play at a glance.
        if self.groove > 0.01 {
            let gw = 260.0;
            let gh = 14.0;
            let gx = (width - gw) / 2.0;
            let gy = 16.0;
            let maxed = self.groove >= 0.999;
            let pulse = if maxed { self.beat_intensity * 0.5 } else { 0.0 };
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
                    .color(Color::new((r * bright).min(1.0), (g * bright).min(1.0), (b * bright).min(1.0), 1.0)),
            );
            // Border
            let gborder = cached_stroke_rect(ctx, gw, gh, 2.0)?;
            canvas.draw(
                &gborder,
                DrawParam::default()
                    .dest(Vec2::new(gx, gy))
                    .color(Color::from_rgba(255, 255, 255, if maxed { 255 } else { 160 })),
            );
            // Label
            let (ltext, lcol) = if maxed {
                ("IN THE GROOVE!", Color::from_rgb(255, 240, 120))
            } else {
                ("GROOVE", Color::from_rgba(200, 230, 240, 200))
            };
            let mut glabel = Text::new(ltext);
            glabel.set_scale(16.0);
            let glw = glabel.measure(ctx)?.x;
            canvas.draw(
                &glabel,
                DrawParam::default()
                    .dest(Vec2::new((width - glw) / 2.0, gy + gh + 3.0))
                    .color(lcol),
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

        // On-beat catch flash
        if self.on_beat_flash > 0.0 {
            let fa = (self.on_beat_flash * 180.0) as u8;
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .scale(Vec2::new(width, height))
                    .color(Color::from_rgba(255, 220, 80, fa)),
            );
            let mut bonus_text = Text::new("ON BEAT! +1");
            bonus_text.set_scale(36.0);
            let btw = bonus_text.measure(ctx)?.x;
            canvas.draw(
                &bonus_text,
                DrawParam::default()
                    .dest(Vec2::new((width - btw) / 2.0, height / 2.0 - 60.0))
                    .color(Color::from_rgba(255, 220, 50, fa)),
            );
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
        canvas.draw(&bg_rect, DrawParam::default());
        let border_rect = Mesh::new_rectangle(
            ctx,
            ggez::graphics::DrawMode::stroke(3.0),
            Rect::new(rect_x, rect_y, rect_w, rect_h),
            Color::from_rgb(220, 220, 220),
        )?;
        canvas.draw(&border_rect, DrawParam::default());
        let destination = Vec2::new((width - title_width) / 2.0, (height - title_height) / 2.0);
        canvas.draw(
            &title,
            DrawParam::default()
                .dest(destination)
                .color(Color::from_rgb(240, 240, 240)),
        );

        // Announce the biome under the title so the player registers the change of zone.
        let biome = self.levels[self.current_level.min(self.levels.len() - 1)].biome;
        let mut subtitle = Text::new(biome.name);
        subtitle.set_scale(40.0);
        let sub_width = subtitle.measure(ctx)?.x;
        let (pr, pg, pb) = biome.pulse;
        canvas.draw(
            &subtitle,
            DrawParam::default()
                .dest(Vec2::new((width - sub_width) / 2.0, rect_y + rect_h + 12.0))
                .color(Color::from_rgb(pr, pg, pb)),
        );
        Ok(())
    }

    fn draw_crabs_with_shake(&self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        let mut rng = rand::rng();
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
                let crab_beat = (self.beat_intensity * 0.7 + (crab.pos.x * 0.003).sin().abs() * 0.3).clamp(0.0, 1.0);
                draw_crab(ctx, canvas, crab, pos, crab_beat, crab.join_pulse, 0.0, crab.facing_angle)?;
                // Attraction halo for crabs currently being pulled by the flashlight beam
                if crab.in_flashlight {
                    let size = crab.scale * CRAB_SIZE;
                    draw_attracted_crab_glow(ctx, canvas, pos, size, crab.crab_color(), self.time_elapsed, self.beat_intensity)?;
                }
                // Boss aura + wear-down health ring — aura tinted per archetype.
                if crab.is_boss() {
                    let size = crab.scale * CRAB_SIZE;
                    let frac = crab.boss_health / BOSS_MAX_HEALTH;
                    let aura = if crab.is_tide_boss() {
                        [0.25, 0.7, 1.0]
                    } else {
                        [1.0, 0.8, 0.25]
                    };
                    draw_boss_health_ring(ctx, canvas, pos, size, frac, self.time_elapsed, aura)?;
                } else if crab.is_armored() && crab.boss_health > 0.0 {
                    // Armored shell indicator — depletes as the shell is worn or cracked
                    let size = crab.scale * CRAB_SIZE;
                    let frac = crab.boss_health / crab.crab_type.initial_shell().max(0.001);
                    draw_armor_ring(ctx, canvas, pos, size, frac, self.time_elapsed)?;
                }
            }
        }
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
                draw_crab(ctx, canvas, crab, crab.pos + Vec2::new(sway, bob), chain_beat, crab.join_pulse, lift, crab.facing_angle)?;
            }
        }
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
        Ok(())
    }

    fn draw_game_over_screen(&self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        let box_width = 600.0;
        let box_height = 260.0;
        let box_x = 340.0;
        let box_y = 360.0;
        let bg_box = Mesh::new_rectangle(
            ctx,
            ggez::graphics::DrawMode::fill(),
            Rect::new(box_x, box_y, box_width, box_height),
            Color::from_rgba(40, 0, 80, 180),
        )?;
        canvas.draw(&bg_box, DrawParam::default());
        let text = Text::new(format!(
            "Game Over!\nThis run: {} crabs banked\nTime: {:.2}s   Best time: {:.2}s\n\nCareer best: {}\nCareer total: {} over {} runs\n\nPress Space or Enter to try again.  Esc to quit.",
            self.score, self.time_elapsed, self.best_time,
            self.career_best_score, self.career_total_score, self.career_runs,
        ));
        canvas.draw(
            &text,
            DrawParam::default()
                .dest(Vec2::new(370.0, 380.0))
                .color(Color::WHITE),
        );
        // Celebrate a fresh career best with a pulsing banner so beating your record lands.
        if self.run_is_new_best && self.score > 0 {
            let pulse = 0.55 + 0.45 * (self.menu_time * 5.0).sin().abs();
            let mut banner = Text::new("★ NEW CAREER BEST! ★");
            banner.set_scale(34.0);
            let bw = banner.measure(ctx)?.x;
            canvas.draw(
                &banner,
                DrawParam::default()
                    .dest(Vec2::new(box_x + (box_width - bw) / 2.0, box_y - 44.0))
                    .color(Color::new(1.0, 0.85, 0.2, pulse)),
            );
        }
        Ok(())
    }

    /// Screen-space rectangles for the four upgrade cards, in card order (index 0 = card "1").
    /// Shared by the draw code (hover highlight) and the mouse-click handler so they always agree.
    fn upgrade_card_rects(&self) -> [Rect; 4] {
        let w = self.width;
        let h = self.height;
        let card_w = 242.0_f32;
        let card_h = 310.0_f32;
        let gap = 18.0_f32;
        let n = 4usize;
        let total_w = n as f32 * card_w + (n - 1) as f32 * gap;
        let x0 = (w - total_w) / 2.0;
        let y0 = (h - card_h) / 2.0 + 15.0;
        std::array::from_fn(|i| {
            Rect::new(x0 + i as f32 * (card_w + gap), y0, card_w, card_h)
        })
    }

    fn draw_upgrade_screen(&self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        let w = self.width;
        let h = self.height;

        // Dark overlay
        let bg = Mesh::new_rectangle(
            ctx, ggez::graphics::DrawMode::fill(),
            Rect::new(0.0, 0.0, w, h), Color::from_rgba(8, 4, 22, 210),
        )?;
        canvas.draw(&bg, DrawParam::default());

        // Title
        let mut title = Text::new("CHOOSE AN UPGRADE");
        title.set_scale(46.0);
        let tw = title.measure(ctx)?.x;
        canvas.draw(&title, DrawParam::default()
            .dest(Vec2::new((w - tw) / 2.0, 58.0))
            .color(Color::from_rgb(255, 215, 50)));

        // Subtitle: make it obvious the cards are clickable, not just number-key driven.
        let mut hint = Text::new("Click a card or press its number");
        hint.set_scale(20.0);
        let hw = hint.measure(ctx)?.x;
        canvas.draw(&hint, DrawParam::default()
            .dest(Vec2::new((w - hw) / 2.0, 110.0))
            .color(Color::from_rgba(210, 210, 210, 200)));

        // Each card is a build LANE that deepens one tool, not a one-off stat bump. Pouring
        // level-ups into a lane branches the run into a playstyle (see apply_upgrade). The current
        // rank is shown per card so committing feels deliberate.
        // (key, icon, name, description, r, g, b, current_rank)
        let cards: &[(&str, &str, &str, &str, u8, u8, u8, u32)] = &[
            ("1", ">", "Beam Focus",  "Wider, longer beam +\nfaster boss melt",    255, 200,  40, self.beam_rank),
            ("2", "O", "Lasso Focus", "Bigger chain reach +\nwider lasso grab",      60, 220, 100, self.lasso_rank),
            ("3", "~", "Whistle Focus","Bigger, stronger pull +\nfaster recharge",   80, 160, 255, self.whistle_rank),
            ("4", "*", "Stomp Focus", "Wider shockwave +\nfaster recharge",         200,  60, 255, self.stomp_rank),
        ];

        let rects = self.upgrade_card_rects();
        let card_w = rects[0].w;
        let card_h = rects[0].h;

        for (i, &(key, icon, name, desc, r, g, b, rank)) in cards.iter().enumerate() {
            let cx = rects[i].x;
            let y0 = rects[i].y;
            let m = self.mouse_pos;
            let hovered = m.x >= cx && m.x <= cx + card_w && m.y >= y0 && m.y <= y0 + card_h;

            let accent = Color::from_rgb(r, g, b);
            let bg_a   = if hovered { 190u8 } else { 115u8 };
            let bdr_w  = if hovered { 4.0_f32 } else { 2.0_f32 };

            // Card background
            canvas.draw(
                &Mesh::new_rectangle(ctx, ggez::graphics::DrawMode::fill(),
                    Rect::new(cx, y0, card_w, card_h), Color::from_rgba(18, 12, 38, bg_a))?,
                DrawParam::default(),
            );
            // Coloured border
            canvas.draw(
                &Mesh::new_rectangle(ctx, ggez::graphics::DrawMode::stroke(bdr_w),
                    Rect::new(cx, y0, card_w, card_h), accent)?,
                DrawParam::default(),
            );

            // Icon
            let mut ico = Text::new(icon);
            ico.set_scale(82.0);
            let iw = ico.measure(ctx)?.x;
            canvas.draw(&ico, DrawParam::default()
                .dest(Vec2::new(cx + (card_w - iw) / 2.0, y0 + 18.0))
                .color(accent));

            // Name
            let mut nm = Text::new(name);
            nm.set_scale(26.0);
            let nw = nm.measure(ctx)?.x;
            canvas.draw(&nm, DrawParam::default()
                .dest(Vec2::new(cx + (card_w - nw) / 2.0, y0 + 118.0))
                .color(Color::WHITE));

            // Lane rank badge — makes it clear you're committing to (and deepening) a lane, and
            // that a picked lane keeps paying off. Lit in the lane accent once invested.
            let rank_str = if rank == 0 {
                "NEW LANE".to_string()
            } else {
                format!("LV {}  ->  {}", rank, rank + 1)
            };
            let mut rk = Text::new(rank_str);
            rk.set_scale(16.0);
            let rkw = rk.measure(ctx)?.x;
            let rank_col = if rank == 0 {
                Color::from_rgba(180, 180, 180, 200)
            } else {
                accent
            };
            canvas.draw(&rk, DrawParam::default()
                .dest(Vec2::new(cx + (card_w - rkw) / 2.0, y0 + 146.0))
                .color(rank_col));

            // Description
            let mut dsc = Text::new(desc);
            dsc.set_scale(18.0);
            let dw = dsc.measure(ctx)?.x;
            canvas.draw(&dsc, DrawParam::default()
                .dest(Vec2::new(cx + (card_w - dw) / 2.0, y0 + 176.0))
                .color(Color::from_rgba(205, 205, 205, 215)));

            // Key hint
            let mut kh = Text::new(format!("[ {} ]", key));
            kh.set_scale(24.0);
            let kw = kh.measure(ctx)?.x;
            canvas.draw(&kh, DrawParam::default()
                .dest(Vec2::new(cx + (card_w - kw) / 2.0, y0 + card_h - 46.0))
                .color(accent));
        }
        Ok(())
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
        self.beat_timer < BEAT_WINDOW || self.beat_timer > BEAT_INTERVAL - BEAT_WINDOW
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

    fn apply_upgrade(&mut self, choice: u8) {
        match choice {
            // Beam lane (boss hunter): each rank widens + lengthens the cone and speeds the boss
            // drain (see boss_drain_rate); rank 2 also grafts on a disco laser so the lane peaks
            // as a dedicated King-Crab melter rather than a pile of flat numbers.
            1 => {
                self.beam_rank += 1;
                self.flashlight.cone_upgrade += 0.18;
                self.flashlight.range_upgrade += 55.0;
                if self.beam_rank == 2 || self.beam_rank == 4 {
                    self.flashlight.laser_level += 1;
                }
            }
            // Lasso lane (chain catcher): wider passive chain reach AND a bigger lasso grab window
            // (see lasso_tip_radius), so throws sweep whole clusters into the conga train.
            2 => {
                self.lasso_rank += 1;
                self.catch_radius_upgrade += 18.0;
            }
            // Whistle lane (crowd control): everything derives from whistle_rank — bigger pulse,
            // stronger pull, shorter cooldown — so it grows into a screen-wide herd magnet.
            3 => self.whistle_rank += 1,
            // Stomp lane (shell breaker): bigger, faster shockwave via stomp_rank, turning the
            // close-range counter into a reliable area crowd-clear.
            4 => self.stomp_rank += 1,
            _ => {}
        }
        self.pending_upgrade = false;
    }
}

impl EventHandler for MainState {
    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if !self.fullscreen_applied {
            // current_monitor() can still be None on the very first tick, so keep retrying
            // until it resolves instead of only trying once.
            if let Some(monitor_size) = ctx.gfx.window().current_monitor().map(|m| m.size()) {
                ctx.gfx
                    .window()
                    .set_fullscreen(Some(ggez::winit::window::Fullscreen::Borderless(None)));
                // set_fullscreen() alone changes the window's on-screen chrome but doesn't
                // reliably trigger ggez to reconfigure its wgpu surface on this winit/Wayland
                // combo, leaving the swapchain at its old (windowed) size while the compositor
                // letterboxes it. set_drawable_size() goes through ggez's own window-mode path,
                // which resizes the surface synchronously instead of waiting on a resize event.
                ctx.gfx
                    .set_drawable_size(monitor_size.width as f32, monitor_size.height as f32)?;
                self.fullscreen_applied = true;
            }
        }

        if self.show_instructions || self.game_over || self.pending_upgrade {
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
            // Decay the perk-shop buy/deny flashes so they're a brief pop, not a stuck glow.
            self.shop_flash = (self.shop_flash - mdt * 2.5).max(0.0);
            self.shop_denied = (self.shop_denied - mdt * 2.5).max(0.0);
            return Ok(());
        }

        let dt = ctx.time.delta().as_secs_f32();

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
                println!(
                    "[perf] {} frames in {:.1}s — avg {:.2}ms ({:.0} fps), worst {:.2}ms",
                    self.perf_frame_count,
                    self.perf_time_accum,
                    avg_ms,
                    1000.0 / avg_ms,
                    worst_ms,
                );
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

        self.time_elapsed += dt;
        self.time_since_catch += dt;

        // Track player position history for conga chain
        self.position_history.push_front(self.player_pos);
        if self.position_history.len() > 2000 {
            self.position_history.pop_back();
        }

        // Beat timer (120 BPM)
        self.beat_timer -= dt;
        if self.beat_timer <= 0.0 {
            self.beat_timer += BEAT_INTERVAL;
            self.beat_intensity = 1.0;
            self.beat_count = self.beat_count.wrapping_add(1);
            let downbeat = self.beat_count % 4 == 0;
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
                self.advance_pattern();
                // Punch the downbeat that births a wave so the arrival reads as a musical hit.
                self.beat_intensity = (self.beat_intensity + 0.6).min(2.0);
                self.on_beat_flash = self.on_beat_flash.max(0.4);
            }
            // Beat camera shake — strength grows with chain length
            let chain_len = self.crabs.iter().filter(|c| c.caught).count();
            if chain_len > 0 {
                let shake_mag = (2.0 + chain_len as f32 * 0.8).min(14.0);
                self.screen_shake = shake_mag;
                // Random kick direction
                let kick_angle = rand::rng().random_range(0.0_f32..std::f32::consts::TAU);
                self.screen_shake_vel = Vec2::new(kick_angle.cos(), kick_angle.sin()) * shake_mag * 60.0;
            }
            // Beat-pulse sparkle rings from all caught crabs
            {
                let chain_len = self.crabs.iter().filter(|c| c.caught).count();
                let positions: Vec<Vec2> = self.crabs.iter().filter(|c| c.caught).map(|c| c.pos).collect();
                self.particle_system.spawn_beat_pulse(&positions, 1.0, chain_len, &mut rand::rng());
            }
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
                let dir = if dir == Vec2::ZERO { Vec2::new(0.0, -1.0) } else { dir };
                crab.pos += dir * DANCER_HOP;
                crab.pos.x = crab.pos.x.clamp(0.0, self.width - crab.scale);
                crab.pos.y = crab.pos.y.clamp(0.0, self.height - crab.scale);
                crab.vel = dir; // face the hop; unit vel so the drift branch stays gentle
                crab.join_pulse = 1.0; // reuse the join squash-pop as a little "landed" bounce
            }
        }
        self.beat_intensity = (self.beat_intensity - dt * 5.0).max(0.0);

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

        // Groove meter decays over time; when it empties the on-beat streak lapses too.
        if self.groove > 0.0 {
            self.groove = (self.groove - dt * 0.18).max(0.0);
            if self.groove <= 0.0 {
                self.beat_streak = 0;
            }
        }

        // Music intensity rises with score, and surges while the player is in the groove.
        let target_intensity = ((self.score as f32 / 30.0) + self.groove * 0.4).min(1.0);
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
        if self.call_cooldown > 0.0 {
            self.call_cooldown = (self.call_cooldown - dt).max(0.0);
        }
        if self.call_pulse > 0.0 {
            self.call_pulse = (self.call_pulse - dt * 1.6).max(0.0);
        }
        if self.chain_snap_cooldown > 0.0 {
            self.chain_snap_cooldown = (self.chain_snap_cooldown - dt).max(0.0);
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

        let area = (self.width, self.height);
        handle_player_movement(self, ctx, dt, SPEED, area);

        // Dash particle burst — fires only in the first frame (threshold near 1.0)
        if self.dash_flash > 0.95 {
            let center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            self.particle_system.spawn_dash_burst(center, self.last_dir, &mut rand::rng());
        }

        self.handle_crab_catching(ctx);
        self.update_crabs(dt, area);

        // Emergent herding: the conga body walls off panicking crabs, bouncing them back toward
        // the beam. Runs before the snap check so a crab deflected by the body never reaches the
        // tail, while one aimed straight at the soft tail still slips past to snap it.
        self.deflect_fleeing_off_chain();

        // Chain-as-risk: a spooked wild crab barreling into the exposed tail can snap links loose.
        self.snap_chain_on_panic();

        // Cash in the train: drive the conga head into the delivery pen to bank it for score.
        self.try_deliver_train(ctx);
        if self.deliver_flash > 0.0 {
            self.deliver_flash = (self.deliver_flash - dt * 1.6).max(0.0);
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

        // Advance catch impact shockwaves; a bit faster than ghost rings so they read as a snap
        let shock_speed = 2.6; // age 0..1 in ~0.38 seconds
        self.catch_shockwaves.retain_mut(|(_, age, _)| {
            *age += dt * shock_speed;
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
                let player_center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
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

        // Whistle: an expanding sonic pulse from the player that yanks free crabs inward. The pull
        // strength is per-archetype (CrabType::whistle_pull) so it's the go-to tool for skittish
        // Sneaky crabs but only nudges the heavy Big ones — a soft counter, never a hard requirement.
        if self.whistle_active > 0.0 {
            // Whistle-lane-scaled reach + pull, read once so the &mut self.crabs loop can use them.
            let whistle_max_r = self.whistle_max_radius() * self.whistle_beat_bonus;
            let whistle_pull = self.whistle_pull_speed() * self.whistle_beat_bonus;
            self.whistle_active = (self.whistle_active - dt).max(0.0);
            self.whistle_radius = (self.whistle_radius + WHISTLE_RING_SPEED * dt).min(whistle_max_r);
            let center = self.whistle_center;
            // The whistle doubles as crowd control: sweeping it over a panicking herd soothes the
            // fear. Charm lasts a beat or two (longer as the whistle lane is ranked up) and blocks
            // both fresh flee and the beat-startle contagion, so it genuinely quells a stampede.
            let charm_dur = 1.4 + 0.5 * self.whistle_rank as f32;
            let mut soothed: Vec<Vec2> = Vec::new();
            for crab in &mut self.crabs {
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
                }
            }
            // Warm puffs rising off the crabs the pulse just calmed — the visual counterpart to
            // the cold "!" alarm rings the panic contagion throws.
            if !soothed.is_empty() {
                let mut rng = rand::rng();
                for pos in soothed.into_iter().take(8) {
                    self.particle_system.spawn_soothe_puff(pos, &mut rng);
                }
            }
        }

        // Stomp: a close-range ground-pound shockwave. It CRACKS Armored crab shells instantly (its
        // dedicated counter — the beam is the slow universal fallback) and gives any free crab the
        // front passes a light inward shove. Its short reach makes it a melee tool, not a ranged
        // gather like the whistle/lasso, so choosing the right verb per herd is a real decision.
        if self.stomp_active > 0.0 {
            // Stomp-lane-scaled reach, read once so the &mut self.crabs loop can use it.
            let stomp_max_r = self.stomp_max_radius() * self.stomp_beat_bonus;
            self.stomp_active = (self.stomp_active - dt).max(0.0);
            self.stomp_radius = (self.stomp_radius + STOMP_RING_SPEED * dt).min(stomp_max_r);
            let center = self.stomp_center;
            let mut cracked: Vec<Vec2> = Vec::new();
            for crab in &mut self.crabs {
                if crab.caught || crab.is_boss() {
                    continue; // the King Crab shrugs off a Stomp — it needs the beam
                }
                let dist = center.distance(crab.pos);
                if dist >= self.stomp_radius {
                    continue; // only crabs the front has already swept past are hit this frame
                }
                // Crack an armored shell wide open the instant the shockwave reaches it.
                if crab.is_armored() && crab.boss_health > 0.0 {
                    crab.boss_health = 0.0;
                    cracked.push(crab.pos);
                }
                // Light inward shove + brief calm so the shaken crab doesn't immediately bolt.
                let toward = (center - crab.pos).normalize_or_zero();
                crab.vel = toward * (WHISTLE_PULL_SPEED * 0.6);
                crab.spooked_timer = crab.spooked_timer.max(0.4);
                crab.fleeing = false;
            }
            for pos in cracked {
                self.floating_texts.spawn(
                    "SHELL CRACKED!".to_string(),
                    pos - Vec2::new(70.0, 40.0),
                    26.0,
                    [0.7, 0.85, 1.0, 1.0],
                );
                self.spawn_catch_shockwave(pos, [0.7, 0.8, 0.95]);
            }
        }

        // Lasso Throw: advance lasso along path, catch crabs near tip
        if self.lasso_timer > 0.0 && self.lasso_pos.is_some() {
            self.lasso_timer -= dt;
            let elapsed = 0.5 - self.lasso_timer;
            let progress = elapsed / 0.3;
            let player_center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            let new_pos = if progress <= 1.0 {
                player_center.lerp(self.lasso_target, progress)
            } else {
                // Return trip: map progress from (1..=5/3) back to (0..=1)
                let return_progress = (progress - 1.0) / (0.2 / 0.3);
                self.lasso_target.lerp(player_center, return_progress.min(1.0))
            };
            self.lasso_pos = Some(new_pos);

            if self.lasso_timer <= 0.0 {
                self.lasso_pos = None;
            } else {
                // Catch crabs while lasso is traveling outward (timer > 0.2 means elapsed < 0.3)
                if elapsed < 0.3 {
                    let tip = new_pos;
                    // Lasso-lane-scaled grab window: higher ranks sweep whole clusters per throw.
                    let grab_r = self.lasso_tip_radius();
                    let to_catch: Vec<usize> = self.crabs.iter().enumerate()
                        .filter(|(_, c)| c.is_catchable() && tip.distance(c.pos) < grab_r)
                        .map(|(i, _)| i)
                        .collect();
                    let mut rng = rand::rng();
                    // Yanking a crab off the sand spooks the herd around the snatch point, same as
                    // a beam or chain catch — collected here and fired after the loop so the lasso
                    // stampede reads as fear rippling outward from where the rope bit.
                    let mut lasso_startle_origins: Vec<Vec2> = Vec::new();
                    for i in to_catch {
                        let pos = self.crabs[i].pos;
                        let crab_type = self.crabs[i].crab_type;
                        let crab_color = self.crabs[i].crab_color();
                        self.particle_system.spawn_catch_effect(pos, crab_color, crab_type, &mut rng);
                        self.spawn_catch_shockwave(pos, crab_color);
                        self.crabs[i].caught = true;
                        if self.crabs[i].is_boss() {
                            self.on_boss_caught(pos, self.crabs[i].is_tide_boss());
                        }
                        lasso_startle_origins.push(pos);
                        self.chain_join_ripple = true;
                        self.crabs[i].chain_index = Some(self.chain_count);
                        self.chain_count += 1;
                        self.check_milestone(&mut rand::rng());
                        self.score += self.combo_multiplier();
                        self.shake_timer = 0.15;
                        self.hitstop_timer = self.hitstop_timer.max(0.06);
                        self.time_since_catch = 0.0;
                        play_catch_sound(&mut self.sounds, ctx, &mut rng);
                        if self.score > 0 && self.score % 10 == 0 {
                            let _ = self.sounds.upgrade.play_detached(ctx);
                            self.pending_upgrade = true;
                        }
                    }
                    for origin in lasso_startle_origins {
                        self.emit_catch_startle(origin);
                    }
                }
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

        // King Crab boss: once the player is rolling, send in a rare oversized crab that must be
        // worn down under the flashlight before it can be caught. Only one at a time.
        if self.score >= self.next_boss_score
            && !self.crabs.iter().any(|c| c.is_boss() && !c.caught)
        {
            self.next_boss_score = self.score + BOSS_SCORE_INTERVAL;
            // Alternate the two boss archetypes so every run cycles through both climax beats: the
            // King Crab (charge — route the train out of the lane) and the Tide Boss (pulse — pull
            // the train back out of range). Toggling guarantees variety instead of RNG streaks.
            let (boss, title, hint, title_color) = if self.next_boss_is_tide {
                (
                    spawn_tide_boss((self.width, self.height), &mut rand::rng(), BOSS_MAX_HEALTH),
                    "A TIDE BOSS SURGES IN!",
                    "Hold your light — but keep your train clear of its pulse!",
                    [0.35, 0.8, 1.0, 1.0],
                )
            } else {
                (
                    spawn_boss((self.width, self.height), &mut rand::rng(), BOSS_MAX_HEALTH),
                    "A KING CRAB APPROACHES!",
                    "Hold your light on it!",
                    [1.0, 0.8, 0.2, 1.0],
                )
            };
            self.next_boss_is_tide = !self.next_boss_is_tide;
            let bpos = boss.pos;
            self.crabs.push(boss);
            self.floating_texts.spawn(
                title.to_string(),
                Vec2::new(self.width / 2.0 - 230.0, 80.0),
                46.0,
                title_color,
            );
            self.floating_texts.spawn(
                hint.to_string(),
                Vec2::new(self.width / 2.0 - 180.0, 130.0),
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
        let base_vol = 0.25 + self.music_intensity * 0.75;
        self.sounds.action_music.set_volume(base_vol);
        let layer_count = self.music_layers.len();
        for (i, layer) in self.music_layers.iter_mut().enumerate() {
            let threshold = (i + 1) as f32 / (layer_count + 1) as f32;
            let vol = if self.music_intensity > threshold {
                ((self.music_intensity - threshold) * 2.0).min(1.0)
            } else {
                0.0
            };
            layer.set_volume(vol);
            if !layer.playing() && vol > 0.01 {
                let _ = layer.play(ctx);
            }
        }

        // Game over if too many free crabs accumulate (overwhelmed).
        if self.crabs.iter().filter(|c| !c.caught).count() >= 80 {
            self.game_over = true;
            return Ok(());
        }

        // Bar-quantized spawns: a lapsed pattern doesn't spawn the next wave right away — it arms
        // it, and the beat handler drops the herd on the next downbeat so waves arrive locked to
        // the music. Whole field caught still counts, so the player is never left waiting with
        // nothing to chase. `wave_telegraph` counts up while armed to drive the draw-side flash.
        self.pattern_timer -= dt;
        if !self.wave_armed && (self.crabs.iter().all(|c| c.caught) || self.pattern_timer <= 0.0) {
            self.wave_armed = true;
            self.wave_telegraph = 0.0;
        }
        if self.wave_armed {
            self.wave_telegraph += dt;
            // Safety valve: if a downbeat somehow doesn't arrive within two bars (e.g. the beat
            // clock is paused), fire anyway so the run can't stall.
            if self.wave_telegraph > BEAT_INTERVAL * 8.0 {
                self.wave_armed = false;
                self.wave_telegraph = 0.0;
                self.advance_pattern();
            }
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let width = self.width;
        let height = self.height;
        let mut canvas = Canvas::from_frame(ctx, Color::from_rgb(100, 200, 100));
        let shake_ox = self.screen_shake_offset.x;
        let shake_oy = self.screen_shake_offset.y;
        // Zoom punch: shrink the visible world rect (magnify) around the player so they stay
        // pixel-locked while the world snaps in on a catch. z == 0 leaves the view untouched.
        let z = self.zoom_punch.clamp(0.0, 0.2);
        let focus = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
        let vw = width * (1.0 - z);
        let vh = height * (1.0 - z);
        canvas.set_screen_coordinates(Rect::new(
            focus.x * z + shake_ox,
            focus.y * z + shake_oy,
            vw,
            vh,
        ));
        canvas.set_blend_mode(BlendMode::ALPHA);
        canvas.set_sampler(Sampler::nearest_clamp());

        if self.show_instructions {
            if self.sounds.outro_music.playing() {
                self.sounds.outro_music.pause();
            }
            if !self.sounds.intro_music.playing() {
                self.sounds.intro_music.play(ctx)?;
            }
            self.draw_instructions_screen(ctx, &mut canvas, width, height)?;
            canvas.finish(ctx)?;
            return Ok(());
        } else if self.pending_upgrade {
            self.sounds.action_music.pause();
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

    fn key_down_event(&mut self, ctx: &mut Context, input: KeyInput, _repeat: bool) -> GameResult {
        if self.pending_upgrade {
            if let Some(key) = input.keycode {
                match key {
                    KeyCode::Key1 => self.apply_upgrade(1),
                    KeyCode::Key2 => self.apply_upgrade(2),
                    KeyCode::Key3 => self.apply_upgrade(3),
                    KeyCode::Key4 => self.apply_upgrade(4),
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
        self.mouse_pos = Vec2::new(x / scale_x, y / scale_y);
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
        if button == MouseButton::Left && self.lasso_pos.is_none() {
            let window_size = ctx.gfx.window().inner_size();
            let scale_x = window_size.width as f32 / self.width;
            let scale_y = window_size.height as f32 / self.height;
            let target = Vec2::new(x / scale_x, y / scale_y);
            self.lasso_target = target;
            self.lasso_timer = 0.5;
            self.lasso_pos = Some(self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0));
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

    let (mut ctx, event_loop) = ContextBuilder::new("rustler", "carlthome")
        .add_resource_path(resource_dir)
        .window_mode(WindowMode::default())
        .build()?;
    let state = MainState::new(&mut ctx)?;
    event::run(ctx, event_loop, state)
}
