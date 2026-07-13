mod controls;
mod enemies;
mod graphics;
mod levels;
mod sounds;
mod spawnings;
mod tutorial;
mod skins;
mod world_map;

use std::{cell::RefCell, collections::HashMap, collections::VecDeque, env, fs, path};

use ggez::audio::SoundSource;
use ggez::audio::Source;
use ggez::conf::{FullscreenType, WindowMode};
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
    FloatingTextSystem, ParticleSystem, PennedMarcherSystem, cached_stroke_rect, draw_attracted_crab_glow,
    draw_armor_ring, draw_beat_indicator, draw_beat_wave_ring, draw_catch_shockwaves, draw_chain_rings,
    draw_combo_meter, draw_boss_health_ring, draw_conga_rope, draw_crab, draw_crab_radar,
    draw_ambient_motes, draw_delivery_pen, draw_fear_rings, draw_flashlight, draw_floating_texts, draw_grass, draw_lasso, draw_pen_guide,
    draw_boss_fissures, draw_call_ring, draw_catch_trails, draw_golden_sparkle, draw_groove_vignette, draw_magnet_aura, draw_particles, draw_penned_marchers, draw_rustler, draw_slam_ring, draw_speed_lines, draw_stomp_ring, draw_thief_aura, draw_tide_pools,
    draw_reef_phrase, draw_tide_pulses, draw_wave_telegraph,
    draw_whistle_ring, draw_world_map, unit_circle, unit_square,
};
use crate::levels::{Level, TerrainKind, get_levels};
use crate::spawnings::{
    spawn_boss, spawn_enemies, spawn_hype_dancer, spawn_rhythm_boss, spawn_tide_boss,
    spawn_tutorial_crabs,
};
use crate::tutorial::{Tutorial, TutorialKind};
use crate::skins::PlayerSkin;
use crate::world_map::WorldMap;

const PLAYER_SIZE: f32 = 48.0;
const CRAB_SIZE: f32 = 36.0;
const SPEED: f32 = 200.0;
const CHAIN_LINK_FRAMES: usize = 12;
const BEAT_INTERVAL: f32 = 0.5; // 120 BPM, crab rave tempo
const BEAT_WINDOW: f32 = 0.08;  // seconds around a beat that count as "on beat"
// Drum Roll (hold T): a full bar of clean on-beat holds (4 hits) maxes the charge for the biggest
// fired blast; beyond that it caps so you can't hold forever for infinite reach.
const DRUM_ROLL_MAX: u32 = 4;
// Staged difficulty ramp: a run escalates in named stages over elapsed time so tension rises
// within a zone instead of staying flat. Each entry is (elapsed-seconds threshold to enter the
// stage, its shout name, its density multiplier applied to every wave's crab count, its tempo
// multiplier applied to the beat rate). Durations shrink in step (see STAGE_DURATION_SCALE) so
// later stages also arrive faster, AND the music/beat literally speeds up — the "beat-tempo
// shift" standout the roadmap calls for. A higher tempo mul = faster beats = everything synced to
// the beat (spawns, train step, wobble, pulses) quickens, so a late-run stage physically feels
// more frantic, not just denser. Stage 0 is the warm-up baseline (1.0x count, 1.0x tempo, no
// banner); crossing into any later stage fires a telegraphed banner and a musical punch, a
// standout moment distinct from the every-4th Frenzy spike.
const INTENSITY_STAGES: &[(f32, &str, f32, f32)] = &[
    (0.0, "WARM-UP", 1.0, 1.0),
    (45.0, "BUILDING", 1.25, 1.08),
    (100.0, "HEATED", 1.55, 1.16),
    (170.0, "FEVER", 1.9, 1.26),
    (260.0, "OVERDRIVE", 2.3, 1.38),
];
// How much each stage past the first shortens wave durations (multiplied per stage index), so a
// rising run also gives less breathing room between waves. Floored so it never gets frantic.
const STAGE_DURATION_SCALE: f32 = 0.92;
const STAGE_DURATION_FLOOR: f32 = 0.6;
const BOSS_MAX_HEALTH: f32 = 3.0; // seconds of sustained flashlight needed to wear a King Crab down
const BOSS_DRAIN_RATE: f32 = 1.0; // boss health drained per second while held in the beam
const BOSS_SCORE_INTERVAL: usize = 40; // score gap between successive King Crab arrivals
// King Crab charge: it periodically lunges at the conga train to scatter the tail.
const BOSS_CHARGE_COOLDOWN: f32 = 4.5; // roam time between charges
const BOSS_WINDUP_TIME: f32 = 0.85;    // telegraph duration before a charge fires
const BOSS_CHARGE_TIME: f32 = 0.65;    // how long the lunge lasts
const BOSS_CHARGE_SPEED: f32 = 540.0;  // px/s during the lunge (far faster than it roams)
const BOSS_CHARGE_ARM_RANGE: f32 = 430.0; // only wind up when the train is within striking range
// Multi-phase escalation: once a boss is worn below this fraction of its max health it "enrages" —
// the fight's final phase. The King Crab charges harder and rests less; the Tide Boss pulses faster.
// This turns a flat drain into a genuine climax that ramps as you close in on the catch.
const BOSS_ENRAGE_THRESHOLD: f32 = 0.4; // fraction of BOSS_MAX_HEALTH below which the boss enrages
const BOSS_ENRAGE_COOLDOWN_SCALE: f32 = 0.5; // charge/pulse rest time multiplier while enraged (shorter)
const BOSS_ENRAGE_CHARGE_SPEED_SCALE: f32 = 1.25; // King Crab lunges this much faster while enraged
const BOSS_STUN_DURATION: f32 = 1.6; // seconds a King Crab is dazed after ramming a parked Armored shell
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
const SLAM_RADIUS: f32 = 480.0;        // reach of the Downbeat Slam — every free crab inside gets yanked into the train
const SLAM_RING_SPEED: f32 = 1400.0;   // how fast the slam ring erupts outward (px/s)
// Cinematic slow-motion (bullet time) on the biggest climax moments. Kept short so it punctuates
// a victory without dragging the pace — the sim eases from ~35% speed back to full over this many
// real-time seconds. Triggered by boss catches and the Downbeat Slam ultimate.
const SLOWMO_DURATION: f32 = 0.45;
// Delivery streak: banking crabs in quick succession stacks a payout multiplier. Each bank bumps
// the streak (capped) and refreshes a grace window; letting the window lapse decays it a notch.
const DELIVER_STREAK_GRACE: f32 = 14.0; // seconds a bank buys before the streak decays a notch
const DELIVER_STREAK_MAX: u32 = 8;      // cap so the multiplier can't run away
// Banking on the beat lands a PERFECT DELIVERY: a flat percentage bonus on the bank, stacking on
// top of the streak multiplier — the game's rhythm hook applied to its biggest payoff.
const PERFECT_DELIVERY_BONUS: f32 = 0.5; // +50% on a bank that lands on the beat

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

    // The FRENZY!/stage-name shout banners (draw_frenzy_banner / draw_stage_banner) rebuilt a
    // fresh `Text` plus a `.measure()` glyph-layout pass every single frame they're visible —
    // 1.6-2.0s each, i.e. ~100 frames of unnecessary text shaping per banner. The frenzy banner's
    // text is always the literal "FRENZY!" (never varies), and the stage banner's text only
    // changes on the rare frame the run climbs to a new named stage. Cache the built Text plus
    // its measured size; only the throb/fade (DrawParam scale/color, computed fresh every frame
    // as before) actually needs to change per frame.
    static FRENZY_BANNER_CACHE: RefCell<Option<(Text, Vec2)>> = RefCell::new(None);
    static STAGE_BANNER_CACHE: RefCell<Option<(&'static str, Text, Vec2)>> = RefCell::new(None);

    // Cache for the top-left "Score / Train / Combo" HUD line. draw_game runs every frame but
    // takes &self, so — same as LEVEL_LABEL_CACHE above — this lives in a thread_local RefCell
    // rather than a struct field. Keyed by the actual (score, chain_len, combo_count, mult)
    // tuple so the fresh `format!` String + `Text` (glyph shaping) only gets rebuilt when one of
    // those values actually changes, not on every one of the ~60 frames between catches.
    static HUD_TEXT_CACHE: RefCell<Option<(usize, usize, usize, usize, Text)>> = RefCell::new(None);

    // Debug-build-only perf overlay ("avg X.XXms (YY fps), worst Z.ZZms") in the top-right
    // corner, so frame-time regressions are visible during actual play instead of only in a
    // terminal that may not be in view. Same rebuild-on-change pattern as HUD_TEXT_CACHE above:
    // keyed by the last-printed avg/worst (rounded to hundredths, as displayed) plus the live
    // crab count, so the Text is only rebuilt when one of those actually changes, not every frame
    // — the crab count rides along so a frame-time spike can be correlated with herd/train size
    // at a glance instead of guessing from code inspection alone.
    #[cfg(debug_assertions)]
    static PERF_OVERLAY_CACHE: RefCell<Option<(i32, i32, i32, Text, f32)>> = RefCell::new(None);

    // The debug-mode "[DEBUG] Pattern: X | Time left: Y.YYs" overlay was rebuilding a fresh
    // `format!` String + `Text` every single frame debug_mode is on, even though the pattern
    // name only changes a handful of times per level and the countdown only visibly changes
    // at the displayed hundredth-of-a-second precision. Same rebuild-on-change idiom as the
    // perf overlay above: keyed on the pattern name plus the timer rounded to hundredths, so
    // it's only rebuilt when the printed text would actually differ.
    static DEBUG_TEXT_CACHE: RefCell<Option<(&'static str, i32, Text)>> = RefCell::new(None);

    // The three ability-bar labels ("Stamina (Space)", "Whistle (E)[/READY]", "Stomp (R)[/READY]")
    // were being rebuilt via a fresh Text::new every single frame even though the stamina label
    // never changes at all and the other two only ever flip between one of two fixed strings.
    // Same fix as above: cache the built Text and only pay for glyph shaping again when the
    // underlying "ready" flag actually flips (or, for stamina, never again after the first frame).
    static STAMINA_LABEL_CACHE: RefCell<Option<Text>> = RefCell::new(None);
    static WHISTLE_LABEL_CACHE: RefCell<Option<(bool, Text)>> = RefCell::new(None);
    static STOMP_LABEL_CACHE: RefCell<Option<(bool, Text)>> = RefCell::new(None);

    // The groove-meter label ("GROOVE" / "IN THE GROOVE! — [G] SLAM on beat") was rebuilding a
    // fresh Text plus a .measure() glyph-layout pass every single frame the groove bar is visible
    // — which is most of active play, since the bar shows as soon as groove > 0.01. Its text only
    // flips between two fixed strings based on whether the meter is maxed, so cache it the same
    // way as the whistle/stomp ability labels above: keyed by the `maxed` bool, only rebuilt (and
    // re-measured) on the rare frame that flag actually changes.
    static GROOVE_LABEL_CACHE: RefCell<Option<(bool, Text, f32)>> = RefCell::new(None);

    // The Groove Gamble multiplier badge ("GROOVE GAMBLE  xN.NN") was rebuilding a fresh Text
    // AND baking a pulsing "pop" scale into the font size itself (Text::set_scale) every frame
    // the multiplier is live — a glyph-layout pass ~60 times/sec during any hot streak, the exact
    // pattern already fixed for the groove meter label above. The multiplier only steps in fixed
    // +0.25 increments, so key the cache on that rounded value (in hundredths) and only re-measure
    // when it actually changes; the per-frame "pop" pulse is applied as a DrawParam scale instead,
    // which is free (no re-layout) since it scales the already-rasterized glyphs.
    static GAMBLE_BADGE_CACHE: RefCell<Option<(u32, Text, f32)>> = RefCell::new(None);

    // The "ON BEAT! +1" bonus-catch popup rebuilt a fresh Text and re-measured its width every
    // single frame `on_beat_flash` is active — up to ~17 frames per on-beat catch (0.85 down to
    // 0 at dt*3.0/s), and on-beat catches are common during a hot run, so this glyph-shaping
    // cost was firing constantly in exactly the moments the frame budget matters most. The
    // string is a fixed literal that never changes, so build and measure it once and reuse the
    // cached Text/width forever, same pattern as the other HUD label caches above.
    static ON_BEAT_TEXT_CACHE: RefCell<Option<(Text, f32)>> = RefCell::new(None);

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

    // The title screen's "Press H — How to Play" tutorial prompt, cached like the start prompt
    // above so the idle menu doesn't rebuild the glyph layout every frame.
    static MENU_TUTORIAL_CACHE: RefCell<Option<(Text, f32)>> = RefCell::new(None);

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

    // The "BANK NOW  [B]" prompt appears every frame that `beat_gamble_mult > beat_gamble_locked
    // + 0.5`, which is most of active play once a Groove Gamble streak builds — it was
    // previously rebuilding a fresh Text plus a `.measure()` glyph-layout pass every single
    // one of those frames. The string is a fixed literal that never changes, so build and measure
    // it once and reuse the cached Text/width forever, same pattern as ON_BEAT_TEXT_CACHE.
    static BANK_NOW_PROMPT_CACHE: RefCell<Option<(Text, f32)>> = RefCell::new(None);

    // Scratch buffer for draw_game's chain-crab ordering. draw_game takes &self and runs every
    // frame, so — same reasoning as the caches above — this lives in a thread_local RefCell
    // instead of a struct field. Chain length grows unbounded over a run (it's the whole point
    // of the conga train), and this collected + sorted a fresh Vec<&EnemyCrab> from scratch every
    // single frame just to hand positions off to draw_conga_rope, which immediately copies them
    // out again. Reusing this buffer (and carrying (chain_index, pos) tuples so the sort key
    // travels with the position, sidestepping the borrow) drops that to zero allocations per
    // frame once the chain's high-water mark is reached.
    static CHAIN_SORT_BUF: RefCell<Vec<(usize, Vec2)>> = RefCell::new(Vec::new());

    // Cache for the level-title overlay (the "Level Up!" card shown for ~1s at each level
    // transition). draw_level_title was building 4 GPU/glyph objects per frame for every
    // one of those ~60 frames: two Mesh::new_rectangle GPU buffers (fill + stroke), one
    // Text + set_scale + two .measure() calls (title), and another Text + set_scale + .measure()
    // (biome subtitle). None of those values change while level_title_timer counts down — only
    // the fade/position (computed as DrawParam, not baked into the objects) varies. Keyed by
    // (level_title, biome_name) so it invalidates if those ever differ (in practice: once per
    // level transition), matching the MENU_PANEL_CACHE pattern for mesh storage.
    #[allow(clippy::type_complexity)]
    static LEVEL_TITLE_OVERLAY_CACHE: RefCell<Option<(String, &'static str, Text, Mesh, Mesh, Text, f32, f32, f32, f32, f32)>> = RefCell::new(None);
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
///
/// `beat_streak` is the current run of consecutive on-beat catches: as it climbs, the chime walks
/// up a pentatonic scale so a hot in-the-pocket streak *sounds* like a rising musical run instead
/// of a flat repeat, then wraps to a bright octave-up at the top so it never runs off into
/// chipmunk territory. A cold/off-beat streak (0) stays at the neutral root note with only the
/// small random detune, so ordinary catches are unchanged. This makes the rhythm reward audible,
/// not just numeric.
///
/// Free function (not a `&mut self` method) so it can be called from inside loops that already
/// hold a disjoint mutable borrow of another field of `MainState` (e.g. `for crab in &mut
/// self.crabs`), where a whole-`self` method call wouldn't type-check.
fn play_catch_sound(
    sounds: &mut GameSounds,
    ctx: &mut Context,
    rng: &mut impl rand::Rng,
    beat_streak: u32,
) {
    // Major pentatonic ratios (root, 2nd, 3rd, 5th, 6th) — a scale that sounds pleasant no matter
    // which step a rapid multi-catch lands on. Steps climb an octave every 5 catches, and each
    // higher octave doubles the ratio, so a long streak sweeps upward and resolves cleanly.
    const PENTATONIC: [f32; 5] = [1.0, 9.0 / 8.0, 5.0 / 4.0, 3.0 / 2.0, 5.0 / 3.0];
    let step = (beat_streak as usize) % PENTATONIC.len();
    let octave = (beat_streak / PENTATONIC.len() as u32).min(2); // cap at +2 octaves
    let scale = PENTATONIC[step] * 2.0_f32.powi(octave as i32);
    // Small random detune on top of the scale note so simultaneous catches still don't phase-lock.
    let pitch = scale * rng.random_range(0.98_f32..1.02);
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
    beat_synth: sounds::BeatSynth,             // Procedural kick drum played on every beat tick
    flashlight: Flashlight,                    // Flashlight settings and upgrades
    show_instructions: bool,                   // Show instructions screen
    // Active cosmetic loadout for the player character (hat, facial hair, accessory).
    // Loaded from career.txt on startup; changed from the title screen customisation menu.
    // Purely visual — never affects gameplay.
    player_skin: PlayerSkin,
    // Campaign world map — `Some` once the player has entered campaign mode from the title.
    // Persists across runs so node completion carries over. `show_world_map` gates whether the
    // map screen is currently visible; `in_campaign` is true during an active campaign run.
    world_map: Option<WorldMap>,
    show_world_map: bool,
    in_campaign: bool,
    // Active "How to Play" tutorial session, if any. `Some` while a scripted learn-session runs;
    // it uses the normal live update/draw path but constrains the run (no bosses, no wave
    // escalation, no level advance) and tracks its own machine-readable pass condition. `None`
    // during a real run or on the menus.
    tutorial: Option<Tutorial>,
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
    // Live beat interval in seconds, = BEAT_INTERVAL / current stage's tempo multiplier. Recomputed
    // whenever the intensity stage climbs so the whole game (beat cadence, every phase animation
    // keyed off beat_timer, spawn quantization) speeds up in step with the difficulty ramp. All
    // per-frame reads use this, not the BEAT_INTERVAL const, so tempo shifts stay in sync.
    beat_interval: f32,
    beat_intensity: f32,
    music_intensity: f32,
    on_beat_flash: f32,
    groove: f32,         // 0..=1 on-beat "groove" meter — fills on rhythmic catches, decays over time
    beat_streak: u32,    // consecutive on-beat catches; escalates the score bonus
    // Groove Gamble — the rhythm risk/reward layer. Consecutive on-beat catches compound a live
    // GLOBAL score multiplier (beat_streak drives beat_gamble_mult); a single off-beat catch breaks
    // the run and resets it to 1x. It's a tension the player is actively managing: keep nailing the
    // beat and every point is worth more, but one greedy off-beat grab throws the whole heat away.
    beat_gamble_mult: f32,   // current compounding multiplier from the on-beat streak (>= 1.0)
    beat_gamble_flash: f32,  // green pulse when the multiplier steps up
    streak_lost_flash: f32,  // red pulse + callout when an off-beat catch breaks a hot streak
    // Cash-out fork: pressing B banks the live streak. Banking ON the beat locks the whole
    // multiplier into a safe floor that an off-beat miss can no longer wipe; banking off-beat
    // takes a haircut. After a bank the live climb resets to the locked floor and keeps rising,
    // so the choice is "bank now and keep it safe" vs "push higher and risk the whole stack".
    beat_gamble_locked: f32, // safe multiplier floor secured by a cash-out (>= 1.0)
    gamble_bank_flash: f32,  // gold pulse when a cash-out banks the streak
    gamble_bank_pulse: f32,  // "BANK NOW?" prompt pulse while a bankable streak is live
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
    // Cosmetic parade of just-banked crabs filing into the delivery pen (see try_deliver_train).
    penned_marchers: PennedMarcherSystem,
    combo_count: usize,
    combo_timer: f32,
    textures: GameTextures,                    // Textures for grass, sand, and player
    level_textures: Vec<LevelTexture>,         // Textures for each level
    // Beat Wave ability
    beat_count: u32,                           // Counts beats fired, every 4th triggers wave
    // Bar downbeat accent: the musical "1" of every 4-beat bar lands harder than the three
    // beats between it, so the rhythm reads as structured bars instead of a flat metronome.
    // Kicked to 1.0 on each `beat_count % 4 == 0` beat and decayed each frame; the beat-stepping
    // conga train amplifies its forward stomp while this is high, so the whole train visibly
    // "lands the one" together — a big unified footfall on the downbeat, smaller steps between.
    bar_accent: f32,
    // Drum Roll (hold T): the one player-driven rhythm verb that's a fresh VERB, not a passive
    // multiplier. Hold T across consecutive beats to build a charge; each beat that T is held
    // while on-beat counts as a "roll hit" and stacks. Release to FIRE a focused beam blast down
    // the flashlight's aim — a short window where the cone widens and reaches far, snapping every
    // free crab in that aimed arc into the train at once. It's directional (down your aim, unlike
    // the radial Slam), timing-gated (only pays if you land the beats), and costs no Groove meter,
    // so it's a skill move you perform rather than a meter you spend. Missing a beat while holding
    // resets the stack, so the tension is holding the roll clean through a full bar for the big pop.
    drum_roll_held: bool,       // was T held last frame — edge-detects press/release in update
    drum_roll_hits: u32,        // consecutive on-beat "roll hits" banked while holding (the charge)
    drum_roll_charge: f32,      // 0..1 visual charge level, eased toward drum_roll_hits for a smooth telegraph
    drum_roll_fire: f32,        // 1..0 timer while a fired blast's wide beam is live (drives the catch boost + glow)
    drum_roll_power: u32,       // roll hits captured at the moment of firing — scales the fired blast's reach/arc
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
    // Staged difficulty spike: instead of a flat rising curve, every Nth cleared wave is a
    // "Frenzy" — a denser-than-normal drop with a gold telegraph, an extra downbeat punch, and a
    // banner, so the run has recurring standout moments that feel earned rather than a smooth ramp.
    // `waves_cleared` counts patterns cleared this run; `frenzy_wave` marks the currently-armed
    // drop as a frenzy so the telegraph and the spawn both know. `frenzy_banner_timer` drives the
    // "FRENZY!" flash when one lands.
    waves_cleared: u32,
    frenzy_wave: bool,
    frenzy_banner_timer: f32,
    // Staged difficulty ramp over elapsed time (the smooth rising spine of a run, orthogonal to
    // the every-4th Frenzy spike above). `intensity_stage` indexes INTENSITY_STAGES and only ever
    // climbs; crossing into a new stage fires `stage_banner_timer` with `stage_banner_name` set to
    // the stage's shout. Every spawned wave reads the current stage to scale its count/duration.
    intensity_stage: usize,
    stage_banner_timer: f32,
    stage_banner_name: &'static str,
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
    // Downbeat Slam (G) — the rhythm ultimate. It only fires when the Groove meter is full AND the
    // press lands on the beat: a huge shockwave erupts from the player that yanks every free crab in
    // a wide radius straight into the conga train at once, then drains the whole meter. This is the
    // spectacle payoff for playing in the pocket — the groove meter finally *does* something instead
    // of only swelling the (currently silent) music. Off-beat or an empty meter fizzles with feedback
    // so mistiming reads clearly. The slam ring below is purely visual; the catch happens instantly.
    slam_active: f32,                            // >0 while the slam ring is expanding (seconds remaining)
    slam_radius: f32,                            // current front radius of the expanding slam ring
    slam_center: Vec2,                           // player center captured when the slam fired (ring origin)
    slam_flash: f32,                             // 1..0 gold screen bloom on a successful slam
    // Dash effect
    dash_just_fired: bool,
    dash_flash: f32,
    // Camera shake
    screen_shake: f32,          // current shake magnitude (pixels), decays each frame
    screen_shake_vel: Vec2,     // current shake offset velocity
    screen_shake_offset: Vec2,  // current pixel offset applied to viewport
    hitstop_timer: f32,         // brief whole-sim freeze right after a catch (juice)
    slowmo_timer: f32,          // 1..0 cinematic slow-motion ramp on the biggest climax moments
                                // (boss catches, Downbeat Slam). Unlike hitstop's hard freeze, the
                                // sim keeps running but time is dilated, easing back to full speed
                                // as the timer decays — so a set-piece victory lands in bullet-time
                                // instead of just snapping past.
    chain_join_ripple: bool,       // set true when any crab is caught this frame
    chain_snap_cooldown: f32,      // >0 briefly after a tail snaps, so one brush can't strip the whole train
    cached_tail_pos: Option<Vec2>, // position of the highest-chain_index caught crab, refreshed once per frame in update_crabs and reused by steal_chain_thief instead of a second O(n) scan
    next_milestone: usize,               // Next train-length milestone to celebrate
    next_boss_score: usize,              // score at which the next boss arrives
    next_boss_kind: usize,               // cycles 0=King Crab, 1=Tide Boss, 2=Reef DJ so runs rotate through all three climax beats
    // Reef DJ call-and-response phrase. The rhythm boss doesn't open its shell on *every* beat —
    // it CALLS a short phrase: each bar it flashes a random subset of the four beats as "hot", and
    // its shell only drains while you hold the light on it during one of those called beats. Off
    // the phrase (both off-beat and on un-called beats) the light does nothing, so the fight is a
    // real echo-the-pattern duel instead of a hold-and-tap-the-beat one. `reef_phrase[i]` is true
    // when beat `i` of the current bar (beat_count % 4) is a called/hot beat.
    reef_phrase: [bool; 4],
    reef_phrase_bar: u32,                // beat_count/4 of the bar the current phrase was rolled for, so we re-roll once per bar
    reef_active: bool,                   // true while a Reef DJ is on the field, gating the phrase HUD/telegraph
    // Reef DJ backup dancers: the fight otherwise silences the whole archetype web, so the DJ
    // summons its own "hype Dancers" into the arena as a fight mechanic. Catching one on a called
    // (hot) beat chips the boss shell — herd them onto the phrase to crack it faster than light
    // alone. This timer counts down while the DJ is on the field; on zero it spawns one and resets.
    reef_dancer_timer: f32,
    reef_hit_flash: f32,                 // 1..0 juice bloom kicked when the player lands a hot beat on the DJ's shell
    // Delivery pen — the "cash in the train" mechanic. Drive the conga line into the pen to bank
    // the whole train for a super-linear score payout (longer train = disproportionately more) and
    // reset the chain, closing the risk/reward loop the chain-snap risk opened. The pen relocates
    // each level so routing the train there stays a fresh decision.
    pen_pos: Vec2,                       // center of the delivery pen on the field
    deliver_flash: f32,                  // 1..0 bloom timer after a successful bank (visual only)
    // Delivery streak — consecutive banks escalate a payout multiplier so cashing in repeatedly
    // (rather than hoarding one giant train) builds its own rising reward, and banking *on the
    // beat* stacks a "PERFECT DELIVERY" rhythm bonus on top. Closes the rhythm hook over the
    // game's single biggest payoff moment. `deliver_streak` counts banks in a row; it never
    // resets on its own (there's no fail state for banking) but a long dry spell decays it via
    // `deliver_streak_timer` so the multiplier reflects *recent* cashing tempo, not lifetime.
    deliver_streak: u32,
    deliver_streak_timer: f32,           // seconds of grace left before an idle streak decays a notch
    // Tide pools — terrain that shapes where the train can go. Each pool is a patch of shallow
    // water (center, radius) that drags on movement: crossing one slows the player to a wade, and
    // because the whole conga tail replays the player's path, hauling a long train through open
    // water costs you real time and exposure. They relocate each level (like the pen) so routing
    // — skirt the pools or dash across them — stays a live, geography-driven decision.
    tide_pools: Vec<(Vec2, f32)>,        // (center, radius) of each shallow-water drag zone
    in_tide_pool: bool,                  // whether the player is wading right now (for splash juice)
    // Arena-shifting boss enrage: when a boss crosses its enrage threshold it reshapes the space of
    // the duel. A King Crab CRACKS THE FLOOR into these fissures — (center, radius, age) hazard pits
    // that snap the conga tail if it lingers in one, so the finale is a routing gauntlet, not just a
    // faster charger. `age` counts up from 0 (crack tearing open) toward 1 (settled hazard). The
    // Tide Boss instead FLOODS the arena by appending extra drag pools to `tide_pools`; we remember
    // how many it added in `boss_flood_pools` so `on_boss_caught` can drain exactly those back off
    // without disturbing the level's own water. Both clear when the boss is caught.
    boss_fissures: Vec<(Vec2, f32, f32)>,
    // Beat-synced eruption pulse for the King Crab fissures: kicked to 1.0 on each beat while
    // fissures are open, then decays toward 0. On the peak the molten pits GEYSER — a spout bursts
    // up, the pit's glow flares, and its tail-snap radius briefly swells so the hazard breathes
    // with the music. Between beats the fissures settle and the widened bite recedes, so a fissure
    // is only fully dangerous *on the beat* — the player learns to thread the tail across in the
    // gaps, tying the arena-crack finale into the game's rhythm spine instead of being a static pit.
    boss_fissure_erupt: f32,
    boss_flood_pools: usize,             // count of extra pools a Tide Boss flooded in on enrage
    chain_rings: Vec<(Vec2, f32, [f32; 3])>, // (pos, age 0..1, rgb) for beat ghost rings
    catch_shockwaves: Vec<(Vec2, f32, [f32; 3])>, // (pos, age 0..1, rgb) impact ring per catch
    // A bright whip-streak that arcs from where a crab was caught to the head of the train, so a
    // catch reads as the crab being *yanked* in rather than just blinking onto the tail. Each entry
    // is (from, to, age 0..1, rgb); brighter/thicker when the catch landed on the beat.
    catch_trails: Vec<(Vec2, Vec2, f32, [f32; 3])>,
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
    // Emergent pile-up: crabs the wall just deflected get funneled into the train's concave
    // pockets, where they collide with *each other*. This pass ricochets colliding fleeing crabs
    // apart and cross-startles them, so herding a panicking crowd into the conga wall sets off a
    // pinball cascade. deflect_ricochet_buf holds the (index, pos) of crabs deflected this frame;
    // deflect_ricochet_grid_buf buckets them so each only tests nearby neighbors, not all of them.
    deflect_ricochet_buf: Vec<(usize, Vec2)>,
    deflect_ricochet_grid_buf: std::collections::HashMap<(i32, i32), Vec<usize>>,
    // Cold ring positions where two deflected crabs cracked into each other, spawned after the
    // ricochet pass so the collision reads without a per-frame allocation.
    deflect_collide_buf: Vec<Vec2>,
    // Resolved (crab index, new pos, new vel) from the ricochet pass, staged here so we apply
    // them after scanning (no double mutable borrow) without allocating a fresh Vec each frame.
    deflect_resolve_buf: Vec<(usize, Vec2, Vec2)>,
    // Event-collection scratch buffers for update_crabs, reused every frame instead of being
    // freshly allocated on each call. Most frames produce zero events in each of these (no
    // crab started fleeing, no boss broke, etc.), so a per-frame Vec::new() was pure churn —
    // clearing a buffer that's almost always empty costs nothing, while allocating one does.
    flee_pops_buf: Vec<Vec2>,
    // Positions of crabs a catch-shock startle just spooked this frame (see emit_catch_startle),
    // reused across calls instead of a fresh Vec::new() every single catch.
    startled_pops_buf: Vec<Vec2>,
    // Positions where a Magnet's field just snared a fleeing Golden this frame (first-snare only),
    // so the "SNARED!" pop and shockwave fire once rather than every frame the tether holds.
    golden_snare_pops_buf: Vec<Vec2>,
    // Positions where a Magnet's field just intercepted a homing Thief this frame (first-catch
    // only), so the "INTERCEPTED!" pop and shockwave fire once rather than every frame it's held.
    thief_snare_pops_buf: Vec<Vec2>,
    // Positions where a roaming Magnet first began chasing a nearby fleeing Golden this frame
    // (first-lure only), so the "LURED!" pop fires once rather than every frame the chase holds.
    magnet_lure_pops_buf: Vec<Vec2>,
    // Positions where a homing Thief first got diverted off your tail by a nearby fleeing Golden
    // this frame (first-divert only), so the "SHINY!" pop fires once rather than every frame it holds.
    thief_lure_pops_buf: Vec<Vec2>,
    boss_broke_buf: Vec<Vec2>,
    armor_broke_buf: Vec<Vec2>,
    attraction_particles_buf: Vec<(Vec2, Vec2, f32, [f32; 3])>,
    boss_windups_buf: Vec<Vec2>,
    boss_launches_buf: Vec<Vec2>,
    boss_charge_dust_buf: Vec<(Vec2, Vec2)>,
    // A boss just crossed into its enrage phase this frame — (pos, is_tide) so the callout/burst
    // can color itself. Almost always empty; reused like the other event buffers.
    boss_enrages_buf: Vec<(Vec2, bool)>,
    tide_fires_buf: Vec<Vec2>,
    tide_swells_buf: Vec<Vec2>,
    // Free Magnet-crab positions each frame, reused instead of reallocating — drives the
    // magnet-pull pass in update_crabs (ordinary crabs drift toward the nearest one).
    magnet_positions_buf: Vec<Vec2>,
    // Free-roaming Golden positions each frame, reused instead of reallocating — drives the
    // Golden-lures-Magnet pass in update_crabs (a roaming Magnet drifts toward the nearest one).
    golden_lure_positions_buf: Vec<Vec2>,
    // Positions of "charged" Magnets each frame — a Magnet currently pinning a snared Golden deep
    // in its field. Reused instead of reallocating. Drives the Golden-supercharges-Magnet crossover
    // in update_crabs: the shine energizes the lodestone so it vacuums the surrounding herd in
    // harder while it holds the prize (see the charged-radius branch of the magnet-pull pass).
    charged_magnet_positions_buf: Vec<Vec2>,
    // Free Armored crab positions each frame, reused instead of reallocating — drives the
    // Armored-body-blocks-King-Crab-charge crossover (a shell in the lunge's lane stops it cold).
    armored_positions_buf: Vec<Vec2>,
    // (boss_pos, shell_pos) for each King Crab charge blocked by an Armored shell this frame, so
    // the shell-clang feedback and shell knockback fire after the &mut self.crabs loop ends.
    boss_blocks_buf: Vec<(Vec2, Vec2)>,
    // King Crab positions stunned by ramming a parked Armored shell this frame, reused instead of
    // reallocating — mirrors boss_blocks_buf above (almost always empty).
    boss_stuns_buf: Vec<Vec2>,
    // Positions of fleeing/amplified Golden panic sources each frame, reused instead of
    // reallocating — drives the Golden-panic-spooks-Thief crossover in steal_chain_thief. Almost
    // always empty (a Golden mid-flee is rare), so this used to be a wasted per-frame Vec
    // allocation before it was pooled like the buffers above.
    golden_panic_positions_buf: Vec<Vec2>,
    // Event buffers for steal_chain_thief's three latched-Thief saves (Magnet pry, Golden panic
    // spook, Golden lure), reused instead of reallocating three fresh Vecs every single frame —
    // this function runs unconditionally whenever the train is long enough to be raidable, so an
    // unpooled Vec::new() here paid an allocation every frame even though a save firing on any
    // given frame is rare. Same pattern as the other event buffers on this struct.
    pried_by_magnet_buf: Vec<Vec2>,
    spooked_by_golden_buf: Vec<Vec2>,
    lured_by_golden_buf: Vec<Vec2>,
    // Landing spots of fleeing Dancers each beat, reused instead of reallocating — drives the
    // per-beat Dancer-hop startle ripple (see the beat block in update).
    dancer_hop_scratch: Vec<Vec2>,
    // Scratch buffers for beat_startle_contagion, mirroring the catch_by_chain/
    // deflect_fleeing_off_chain grid pattern: carriers are bucketed into a spatial grid so each
    // calm crab only tests nearby carriers instead of every panicking crab in the herd.
    // Each carrier is (pos, panic amplitude): a fleeing Golden crab carries an amplified fear
    // that ripples through the herd harder than an ordinary panicking crab (see below).
    contagion_carriers_buf: Vec<(Vec2, f32)>,
    contagion_grid_buf: std::collections::HashMap<(i32, i32), Vec<usize>>,
    // Emergent crossover: free Armored crabs act as calm anchors that shelter the herd from the
    // panic ripple. Their positions are snapshotted each beat into this reused buffer so the
    // contagion pass can spare any calm crab sheltering in an Armored shell's shadow from
    // infection — see beat_startle_contagion.
    armored_anchors_buf: Vec<Vec2>,
    // Spatial grid over armored_anchors_buf (same pattern as contagion_grid_buf) so the shelter
    // check only tests nearby anchors instead of every free Armored crab in the herd — without
    // this a session salted with several Armored crabs turned the per-crab shelter check into a
    // flat scan multiplied across every calm crab evaluated that beat.
    armored_anchor_grid_buf: std::collections::HashMap<(i32, i32), Vec<usize>>,
    // (pos, amplified?) — amplified pops came from a Golden's panic bomb and get a hot golden
    // "!" so the player sees the shiny prize detonating the herd, not just an ordinary scare.
    contagion_pops_buf: Vec<(Vec2, bool)>,
    // Same grid treatment for the Dancer-hop startle ripple (see the beat block in update) —
    // dancer_hop_scratch above supplies the fear sources, this buckets them for a fast lookup.
    dancer_startle_grid_buf: std::collections::HashMap<(i32, i32), Vec<usize>>,
    dancer_spooked_buf: Vec<Vec2>,
    // Scratch buffers for the Dancer-jolts-Thief and Dancer-trips-Golden crossovers below, reused
    // instead of a fresh Vec::new() every beat. Both also reuse dancer_startle_grid_buf (built just
    // above from the same dancer_hops) instead of linear-scanning every hop per crab, so a herd
    // salted with several fleeing Dancers doesn't turn this into a flat per-crab-per-hop scan.
    dancer_jolt_buf: Vec<Vec2>,
    dancer_trip_buf: Vec<Vec2>,
    // Scratch buffer for the Dancer-chips-Armored-shell crossover below, same reuse pattern as the
    // two above — (crab_pos, cracked_clean) so the after-loop feedback can tell a chip apart from a
    // full shatter. Also reuses dancer_startle_grid_buf, so a herd of Dancers by an Armored crab
    // doesn't turn this into a per-crab-per-hop scan.
    dancer_chip_buf: Vec<(Vec2, bool)>,
    // Scratch buffer for the Dancer-jolts-Magnet crossover below, same reuse pattern as the ones
    // above — holds the positions of free Magnets a Dancer's on-beat hop thumped into a pull surge
    // this beat, for the after-loop feedback pop. Reuses dancer_startle_grid_buf like its siblings.
    dancer_kick_buf: Vec<Vec2>,
    // Scratch buffers for the Whistle/Stomp/Lasso ability loops in update(), reused every frame
    // instead of a fresh Vec::new() each tick these abilities are active. Each ability is active
    // for a fraction of a second to a couple seconds per use, so without reuse this was a
    // per-frame allocation for the whole duration of every whistle/stomp/lasso.
    whistle_soothed_buf: Vec<Vec2>,
    stomp_cracked_buf: Vec<Vec2>,
    lasso_catch_buf: Vec<usize>,
    lasso_startle_buf: Vec<Vec2>,
    // On-beat Thief-shake catches collected during the whistle/stomp loops (see
    // snatch_thief_on_beat) — almost always empty (at most one latched Thief at a time), but
    // these loops run every frame the ability is active, so reuse instead of a fresh Vec::new().
    whistle_thief_snatch_buf: Vec<(usize, Vec2)>,
    stomp_thief_snatch_buf: Vec<(usize, Vec2)>,
    // Event-collection scratch buffers for handle_crab_catching, reused every frame instead of
    // three fresh Vec::new() calls per tick. The vast majority of frames catch zero crabs (no
    // startle origin, no boss catch, no dance catch), so this was pure per-frame allocation
    // churn on the hottest possible path (runs unconditionally in update() every tick).
    startle_origins_buf: Vec<Vec2>,
    boss_catches_buf: Vec<(Vec2, bool)>,
    dance_catches_buf: Vec<Vec2>,
    // Golden crabs snapped up this frame — (pos, its base catch points) so the big lump-sum bonus
    // is paid out after the catch loop (needs &mut self for particles/floating text/score).
    golden_catches_buf: Vec<(Vec2, usize)>,
    // Reef DJ hype-Dancer catches this frame (see handle_crab_catching) — pooled like its sibling
    // catch-event buffers above instead of a fresh Vec::new() every frame; almost always empty
    // (needs a Reef DJ fight in progress plus a hot-beat catch), same reasoning as golden_catches_buf.
    hype_dancer_hits_buf: Vec<Vec2>,
    // Emergent crossover scratch: free Armored crabs whose shell a charged Magnet's widened vacuum
    // ground down this frame — (pos, whether that grind fully cracked the shell open) so the
    // chip/crack feedback fires after the per-crab borrow ends. Almost always empty (needs a
    // charged Magnet — itself rare, born of a snared Golden or a Dancer thump — plus an Armored
    // crab caught in its outer field), so a reused scratch Vec keeps it allocation-free.
    magnet_grind_buf: Vec<(Vec2, bool)>,
    // Lightweight perf instrumentation (debug builds only): accumulate frame times and print an
    // average + worst-case every couple seconds so future optimization passes have real numbers
    // instead of guessing from code inspection alone.
    #[cfg(debug_assertions)]
    perf_frame_count: u32,
    #[cfg(debug_assertions)]
    perf_time_accum: f32,
    #[cfg(debug_assertions)]
    perf_worst_frame: f32,
    // Last computed avg/worst frame time (ms), so the on-screen overlay always has a number to
    // show instead of blanking between the ~2s print windows above. Updated alongside the
    // println! so both stay in lockstep; drawn every frame but only rebuilt on that same cadence.
    #[cfg(debug_assertions)]
    perf_last_avg_ms: f32,
    #[cfg(debug_assertions)]
    perf_last_worst_ms: f32,
    #[cfg(debug_assertions)]
    perf_last_fps: f32,
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

        // Synthesise the on-beat kick drum at startup so a bad WAV header fails loudly here rather
        // than as silence on the first beat.
        let beat_synth = sounds::BeatSynth::new(ctx)?;

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
            beat_synth,
            flashlight,
            show_instructions: true,
            player_skin: PlayerSkin::default_skin(),
            world_map: None,
            show_world_map: false,
            in_campaign: false,
            tutorial: None,
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
            beat_interval: BEAT_INTERVAL,
            beat_intensity: 0.0,
            music_intensity: 0.0,
            on_beat_flash: 0.0,
            beat_gamble_mult: 1.0,
            beat_gamble_flash: 0.0,
            streak_lost_flash: 0.0,
            beat_gamble_locked: 1.0,
            gamble_bank_flash: 0.0,
            gamble_bank_pulse: 0.0,
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
            penned_marchers: PennedMarcherSystem::new(),
            combo_count: 0,
            combo_timer: 0.0,
            beat_count: 0,
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
            slam_active: 0.0,
            slam_radius: 0.0,
            slam_center: Vec2::ZERO,
            slam_flash: 0.0,
            dash_just_fired: false,
            dash_flash: 0.0,
            screen_shake: 0.0,
            screen_shake_vel: Vec2::ZERO,
            screen_shake_offset: Vec2::ZERO,
            hitstop_timer: 0.0,
            slowmo_timer: 0.0,
            chain_join_ripple: false,
            chain_snap_cooldown: 0.0,
            cached_tail_pos: None,
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
            deliver_streak: 0,
            deliver_streak_timer: 0.0,
            tide_pools: init_tide_pools,
            in_tide_pool: false,
            boss_fissures: Vec::new(),
            boss_fissure_erupt: 0.0,
            boss_flood_pools: 0,
            chain_rings: Vec::new(),
            catch_shockwaves: Vec::new(),
            catch_trails: Vec::new(),
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
            whistle_soothed_buf: Vec::new(),
            stomp_cracked_buf: Vec::new(),
            lasso_catch_buf: Vec::new(),
            lasso_startle_buf: Vec::new(),
            whistle_thief_snatch_buf: Vec::new(),
            stomp_thief_snatch_buf: Vec::new(),
            startle_origins_buf: Vec::new(),
            boss_catches_buf: Vec::new(),
            dance_catches_buf: Vec::new(),
            golden_catches_buf: Vec::new(),
            hype_dancer_hits_buf: Vec::new(),
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
                    let amp = if c.is_golden() { GOLDEN_PANIC_AMP } else { c.panic_amp.max(1.0) };
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
            ((p.x / cell_size).floor() as i32, (p.y / cell_size).floor() as i32)
        };
        // Clear the whole map, not just each bucket's contents — keeping only the values cleared
        // let the key set (one entry per grid cell ever visited by a carrier) grow unbounded over
        // a long session as the herd wanders the full level, slowly bloating the hash table and
        // its load factor even though the actual per-beat working set stays tiny. A full clear()
        // still keeps the map's allocated capacity (same pooling win, no realloc most beats) but
        // resets the key count to "cells touched this beat" instead of "cells touched ever".
        self.contagion_grid_buf.clear();
        for (i, &(pos, _)) in carriers.iter().enumerate() {
            self.contagion_grid_buf.entry(cell_of(pos)).or_default().push(i);
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
                let shelter_r = if amp > 1.05 { SHELTER_RADIUS * 0.55 } else { SHELTER_RADIUS };
                // Shelter radius is always <= CONTAGION_RADIUS (the grid's cell size), so any
                // anchor within range is guaranteed to fall in the crab's own cell or one of its
                // 8 neighbours — the same 3x3 sweep used for carriers above.
                let sheltered = (-1..=1).any(|dx| {
                    (-1..=1).any(|dy| {
                        anchor_grid.get(&(cx + dx, cy + dy)).is_some_and(|bucket| {
                            bucket.iter().any(|&i| anchors[i].distance(crab.pos) < shelter_r)
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
                let outward = if outward == Vec2::ZERO { Vec2::new(0.0, -1.0) } else { outward };
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
            self.floating_texts.spawn(
                "!".to_string(),
                pos - Vec2::new(0.0, 24.0),
                size,
                color,
            );
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

        if self.current_terrain() != TerrainKind::Kelp {
            return;
        }
        if self.chain_snap_cooldown > 0.0 || self.chain_count < MIN_TRAIN_TO_SNAG {
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
            .any(|(c, r)| tail_pos.distance(*c) < *r);
        if !tail_in_kelp {
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
                let outward = if outward == Vec2::ZERO { Vec2::new(0.0, 1.0) } else { outward };
                crab.vel = outward * crab.crab_type.speed_range().end * 1.8;
                crab.speed = 1.0;
                snapped_positions.push(crab.pos);
            }
        }
        self.chain_count = keep;
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
        const MIN_TRAIN_TO_SNAP: usize = 5;        // short trains are safe — the risk only bites once you've invested
        const SNAP_COLLIDE_DIST: f32 = CRAB_SIZE * 0.9;
        const SNAP_LINKS: usize = 3;               // how many tail links a hit knocks loose
        const SNAP_COOLDOWN: f32 = 1.6;            // grace period so a herd can't strip everything at once

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
        const GOLDEN_LURE_LATCH_RADIUS_SQ: f32 = GOLDEN_LURE_LATCH_RADIUS * GOLDEN_LURE_LATCH_RADIUS;
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
            let d = c.pos.distance(tail_pos);
            if c.latch_timer > 0.0 {
                // A nearby Magnet overpowers the clamp: the Thief lets go of the tail and is
                // flung toward the Magnet, joining the loose herd instead of peeling your links.
                if let Some(mp) = nearest_magnet_to(c.pos) {
                    c.latch_timer = 0.0;
                    let dir = (mp - c.pos).normalize_or_zero();
                    let dir = if dir == Vec2::ZERO { Vec2::new(0.0, -1.0) } else { dir };
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
                    let dir = if dir == Vec2::ZERO { Vec2::new(0.0, -1.0) } else { dir };
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
                    let dir = if dir == Vec2::ZERO { Vec2::new(0.0, -1.0) } else { dir };
                    c.vel = dir * c.crab_type.speed_range().end * 1.3;
                    c.speed = 1.0;
                    c.fleeing = false;
                    c.startle_timer = 0.0;
                    c.thief_lured = 0.3; // light the gold "chasing shine" aura
                    lured_by_golden.push(c.pos);
                    continue;
                }
                // Already clamped. Ride the tail so it visually hangs off the back of the train.
                if d > UNLATCH_DIST {
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
            } else if d < LATCH_DIST {
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
                let outward = if outward == Vec2::ZERO { Vec2::new(0.0, 1.0) } else { outward };
                crab.vel = outward * crab.crab_type.speed_range().end * 1.8;
                crab.speed = 1.0;
            }
        }
        self.chain_count = keep;
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
            ((p.x / cell_size).floor() as i32, (p.y / cell_size).floor() as i32)
        };
        for bucket in self.deflect_grid_buf.values_mut() {
            bucket.clear();
        }
        for (i, &seg) in self.deflect_body_buf.iter().enumerate() {
            self.deflect_grid_buf.entry(cell_of(seg)).or_default().push(i);
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
            ((p.x / cell_size).floor() as i32, (p.y / cell_size).floor() as i32)
        };
        for bucket in self.deflect_ricochet_grid_buf.values_mut() {
            bucket.clear();
        }
        for (bi, &(_, pos)) in self.deflect_ricochet_buf.iter().enumerate() {
            self.deflect_ricochet_grid_buf.entry(cell_of(pos)).or_default().push(bi);
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
                    if let Some(candidates) = self.deflect_ricochet_grid_buf.get(&(cx + dx, cy + dy)) {
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
        let on_beat = self.beat_timer < BEAT_WINDOW
            || self.beat_timer > self.beat_interval - BEAT_WINDOW;
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

        // Super-linear base payout: triangular sum so crab #n adds n points, times a flat handler.
        let n = delivered;
        let base = (n * (n + 1) / 2) * 3;

        // A bank in quick succession bumps the delivery streak (capped) and refreshes its grace
        // window; the streak multiplier escalates the payout so cashing in repeatedly at tempo pays
        // off, not just hoarding one giant train.
        self.deliver_streak = (self.deliver_streak + 1).min(DELIVER_STREAK_MAX);
        self.deliver_streak_timer = DELIVER_STREAK_GRACE;
        // Streak 1 = 1.0x, then +0.25x per bank: 1.25x, 1.5x, ... up to 2.75x at the cap.
        let streak_mult = 1.0 + (self.deliver_streak.saturating_sub(1) as f32) * 0.25;

        // Banking on the beat lands a PERFECT DELIVERY: a flat percentage bonus on top of the streak.
        let perfect = self.on_beat_now();
        let perfect_mult = if perfect { 1.0 + PERFECT_DELIVERY_BONUS } else { 1.0 };

        // The Groove Gamble multiplier rides through to the bank too — a hot on-beat streak makes
        // the delivery jackpot pay out even bigger, so it's worth protecting the heat right up to
        // the pen instead of grabbing sloppily on the way in.
        let bank = (base as f32 * streak_mult * perfect_mult * self.beat_gamble_mult).round() as usize;
        self.score += bank;

        // Before the delivered crabs leave the field, snapshot them (in chain order, head first)
        // so they can visibly march into the pen instead of blinking out — the parade is purely
        // cosmetic; the score above is already banked.
        let mut delivered_crabs: Vec<&EnemyCrab> =
            self.crabs.iter().filter(|c| c.caught).collect();
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
        self.next_milestone = 5;

        // Big celebratory feedback so banking feels like a real payoff, not just a number ticking.
        let mut rng = rand::rng();
        self.particle_system.spawn_milestone_fireworks(self.pen_pos, n, &mut rng);
        // A perfect on-beat bank gets a gold rhythm ring; a plain bank stays green.
        self.spawn_catch_shockwave(
            self.pen_pos,
            if perfect { [1.0, 0.85, 0.3] } else { [0.5, 1.0, 0.5] },
        );
        // A hot streak throws a second, larger firework burst so the escalation reads on screen.
        if self.deliver_streak >= 3 {
            self.particle_system
                .spawn_milestone_fireworks(self.pen_pos, n + self.deliver_streak as usize * 4, &mut rng);
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
        self.floating_texts.spawn(
            format!("{} crabs delivered!", n),
            self.pen_pos - Vec2::new(70.0, callout_y),
            26.0,
            [1.0, 0.95, 0.6, 1.0],
        );
        self.deliver_flash = 1.0;
        // A perfect / hot-streak bank hits harder: more zoom, more shake, a fuller groove kick.
        let intensity = streak_mult * perfect_mult;
        self.zoom_punch = self.zoom_punch.max(0.11 * intensity);
        self.screen_shake = self.screen_shake.max(18.0 * intensity);
        let kick_angle = rng.random_range(0.0_f32..std::f32::consts::TAU);
        self.screen_shake_vel = Vec2::new(kick_angle.cos(), kick_angle.sin()) * 18.0 * intensity * 60.0;
        self.on_beat_flash = if perfect { 0.85 } else { 0.6 };
        self.groove = (self.groove + if perfect { 0.5 } else { 0.35 }).min(1.0);
        let _ = self.sounds.success2.play_detached(ctx);

        // Move the pen so the next bank is a fresh routing decision, not a treadmill loop.
        self.pen_pos = pick_pen_pos(self.width, self.height, player_center, &mut rng);
    }

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
                self.chain_count += 1;
                let on_beat = self.beat_timer < BEAT_WINDOW
                    || self.beat_timer > self.beat_interval - BEAT_WINDOW;
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
                    self.groove = (self.groove + 0.22).min(1.0);
                    bonus = self.beat_streak.min(5) as usize;
                    self.on_beat_flash = (0.25 + self.beat_streak as f32 * 0.06).min(0.6);
                    // Groove Gamble: the streak compounds a live global score multiplier. Each
                    // on-beat catch bumps it +0.25x (capped at 5x), so the deeper you ride the beat
                    // the more every point — catches AND deliveries — is worth. The catch mid-streak
                    // feels louder: the multiplier only exists while the run is unbroken.
                    let prev_mult = self.beat_gamble_mult;
                    self.beat_gamble_mult = (self.beat_gamble_mult + 0.25).min(5.0);
                    if self.beat_gamble_mult > prev_mult {
                        self.beat_gamble_flash = 1.0;
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
                            format!("STREAK LOST!  x{:.2} gone — x{:.2} safe", lost, self.beat_gamble_locked)
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
                // Golden crab: on top of the normal catch award, queue a big lump-sum treasure bonus
                // (paid out after the loop). This is the payoff for breaking off the herd to chase it.
                if crab.is_golden() {
                    golden_catches.push((pos, pts));
                }
                self.combo_count += 1;
                self.combo_timer = 1.8;
                let score_str = if self.beat_gamble_mult > 1.01 {
                    format!("+{}  x{:.2}!", pts, self.beat_gamble_mult)
                } else if pts > 1 {
                    format!("+{}  ON BEAT!", pts)
                } else {
                    format!("+{}", pts)
                };
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
                play_catch_sound(&mut self.sounds, ctx, &mut rng, self.beat_streak);
                if self.score > 0 && self.score % 10 == 0 {
                    let _ = self.sounds.upgrade.play_detached(ctx);
                    self.pending_upgrade = true;
                }
            }
        }
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
        // Hand the scratch buffers back for reuse next frame.
        self.startle_origins_buf = startle_origins;
        self.boss_catches_buf = boss_catches;
        self.dance_catches_buf = dance_catches;
        self.golden_catches_buf = golden_catches;
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
        self.particle_system.spawn_milestone_fireworks(pos, 14, &mut rng);
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
            let c = Vec2::new(
                rng.random_range(margin..(self.width - margin)),
                rng.random_range(margin..(self.height - margin)),
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
            let c = Vec2::new(
                rng.random_range(margin..(self.width - margin)),
                rng.random_range(margin..(self.height - margin)),
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
        let in_fissure = self
            .boss_fissures
            .iter()
            .any(|(c, r, age)| *age > 0.6 && tail_pos.distance(*c) < *r * reach);
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
                let outward = if outward == Vec2::ZERO { Vec2::new(0.0, 1.0) } else { outward };
                crab.vel = outward * crab.crab_type.speed_range().end * 1.8;
                crab.speed = 1.0;
                snapped_positions.push(crab.pos);
            }
        }
        self.chain_count = keep;
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
        let mut slingshots: Vec<(Vec2, Vec2)> = Vec::new(); // (magnet_pos, golden_pos) that fired this pulse
        {
            // A Magnet is "loaded" if it's charged (pinning shine) and a snared Golden sits inside its
            // reach — the same pairing the charged-magnet pass already recognizes elsewhere. Collect
            // loaded pairs the surge washes over, sparing them from the scatter below (they fire, not flee).
            let mut loaded_magnets: Vec<Vec2> = Vec::new();
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
                        crab.boss_health = (crab.boss_health - SLINGSHOT_CHIP * slingshots.len() as f32).max(0.0);
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
                            self.catch_trails.push((gpos, bpos, -0.25, [1.0, 0.85, 0.25]));
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
                        if crab.is_golden() && !crab.caught && crab.magnet_snared > 0.0 && crab.pos == gpos {
                            crab.magnet_snared = 0.0;
                            crab.slingshot_spent = 1.2; // ~a couple beats of no-reload while it clears the field
                            crab.fleeing = true;
                            crab.startle_timer = crab.startle_timer.max(0.5);
                            let away = (crab.pos - center).normalize_or_zero();
                            let away = if away == Vec2::ZERO { Vec2::new(0.0, 1.0) } else { away };
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
        }

        // First pass: supercharge every free Magnet the surge washes over, and remember where each
        // anchoring field sits so the shove and the snap below can spare crabs inside it.
        let mut anchor_positions: Vec<Vec2> = Vec::new();
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
        let mut scattered: Vec<Vec2> = Vec::new();
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

        // A Magnet field over the tail calls off the wash-out entirely — feedback for the save.
        let tail_anchored = !anchor_positions.is_empty()
            && self.crabs.iter().any(|c| {
                c.caught && c.chain_index.is_some() && c.pos.distance_squared(center) <= r2 && anchored(c.pos)
            });
        if tail_anchored {
            self.floating_texts.spawn(
                "ANCHORED!".to_string(),
                center - Vec2::new(50.0, 34.0),
                30.0,
                [0.95, 0.55, 0.2, 1.0],
            );
        }

        // Knock the tail loose if any caught link sits inside the blast. Mirrors snap_chain_on_panic
        // but triggered by the pulse's reach rather than a physical tail collision. A Magnet anchoring
        // the tail (tail_anchored) pins the links and cancels the snap.
        let tail_in_blast = !tail_anchored
            && self
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
        let mut best_chain: Option<(usize, Vec2)> = None;
        for c in &self.crabs {
            if c.caught {
                if let Some(ci) = c.chain_index {
                    if best_chain.map_or(true, |(bci, _)| ci > bci) {
                        best_chain = Some((ci, c.pos));
                    }
                }
                continue; // caught crabs can't be a Magnet/Golden/Armored source below
            }
            if c.is_magnet() {
                magnet_positions.push(c.pos);
            } else if c.is_golden() {
                if !c.in_flashlight {
                    golden_lure_positions.push(c.pos);
                }
            } else if c.is_armored() {
                armored_positions.push(c.pos);
            }
        }
        let chain_tail_pos = best_chain.map(|(_, pos)| pos);
        let charge_target = chain_tail_pos.unwrap_or(self.player_pos);
        // Cache for steal_chain_thief (called later this frame, after update_crabs returns) so it
        // doesn't need its own third O(n) scan over self.crabs for the same "current tail" lookup.
        self.cached_tail_pos = chain_tail_pos;

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
            if c.is_magnet() && !c.caught && c.magnet_charged > 0.0
                && !charged_magnet_positions.contains(&c.pos)
            {
                charged_magnet_positions.push(c.pos);
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
        let thief_tail_pos: Option<Vec2> = if self.chain_count >= 4 { chain_tail_pos } else { None };

        // Single RNG for the whole per-crab loop below (attraction sparkles), instead of grabbing
        // a fresh thread-local handle inside the loop for every crab currently in the beam.
        let mut rng = rand::rng();

        // Snapshot whether we're inside the on-beat window right now, so the Reef DJ (rhythm boss)
        // can gate its shell-drain on the beat without re-borrowing self mid-loop. Same window the
        // player already feels for PERFECT tool hits and the on-beat Call.
        let on_beat_now =
            self.beat_timer < BEAT_WINDOW || self.beat_timer > self.beat_interval - BEAT_WINDOW;
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
                let drain_active = crab_in_light
                    && (!crab.is_rhythm_boss() || reef_hot_now);
                if crab.is_rhythm_boss() && crab_in_light && reef_hot_now && crab.boss_health > 0.0 {
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
                        while delta > std::f32::consts::PI { delta -= std::f32::consts::TAU; }
                        while delta < -std::f32::consts::PI { delta += std::f32::consts::TAU; }
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
                        // A stunned (recently-blocked) King Crab can't wind up until the daze passes.
                        if crab.charge_cooldown <= 0.0
                            && !crab.is_stunned()
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
                // A Thief on the hunt for your tail doesn't panic-flee the player between latches —
                // it's single-minded about reaching the train. (A whistle charm still stops it, and
                // once latched it's handled in steal_chain_thief.) This keeps it a committed threat
                // rather than one more crab that scatters when you sweep the beam past it.
                let now_fleeing = !crab_in_light
                    && distance < FLEE_RADIUS
                    && !crab.is_boss()
                    && !crab.is_dancer()
                    && !(crab.is_thief() && self.chain_count >= 4)
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

                // If player is within 150 pixels and crab is in the light, add a small extra speed boost
                let mut speed_multiplier = 1.0;
                if crab_in_light && distance < 150.0 {
                    speed_multiplier = 2.0 - (distance / 150.0);
                    speed_multiplier = speed_multiplier.clamp(1.0, 2.0);
                }

                // Older crabs are faster so the player should catch them early.
                let age_boost = 1.0 + (crab.spawn_time / 10.0).min(1.5);
                crab.pos += crab.vel * crab.speed * speed_multiplier * age_boost * dt;

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
                let nearest_magnet: Option<(f32, Vec2)> = if !crab_in_light && !crab.is_magnet() && !crab.is_boss() {
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
                        if d2 < CHARGED_MAGNET_RADIUS_SQ && d2 > 1.0
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
                            if broke || (before / step).floor() != (crab.boss_health / step).floor() {
                                magnet_grind.push((crab.pos, broke));
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
                        let tail_d2 = thief_tail_pos.map_or(f32::INFINITY, |tp| crab.pos.distance_squared(tp));
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
                    let dancer = spawn_hype_dancer((self.width, self.height), reef_boss_pos, &mut rng);
                    let dpos = dancer.pos;
                    self.crabs.push(dancer);
                    // Little violet summon puff so the dancer reads as the DJ's call, not a stray.
                    self.particle_system
                        .spawn_milestone_fireworks(dpos, 5, &mut rng);
                }
                self.reef_dancer_timer = 3.0;
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
            let knock_dir = if knock_dir == Vec2::ZERO { Vec2::new(0.0, -1.0) } else { knock_dir };
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
        for (pos, broke) in magnet_grind.drain(..) {
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
                let step_phase = (1.0 - self.beat_timer / self.beat_interval) * std::f32::consts::TAU
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
        let stage_dur = STAGE_DURATION_SCALE.powi(stage as i32).max(STAGE_DURATION_FLOOR);
        let base_count = (p.count as f32 * stage_mul).round() as usize;
        let frenzy = self.frenzy_wave;
        let count = if frenzy {
            ((base_count as f32 * 1.7).ceil() as usize).max(base_count + 4)
        } else {
            base_count
        };
        let base_duration = p.duration * stage_dur;
        let duration = if frenzy { base_duration * 0.85 } else { base_duration };
        let crabs = spawn_enemies(p.pattern.clone(), count, area, p.centroid, &mut rng);
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
            // New zone wipes any boss-flooded water/fissures — the fresh pools are the level's own.
            self.boss_flood_pools = 0;
            self.boss_fissures.clear();
            self.boss_fissure_erupt = 0.0;
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
        self.slam_active = 0.0;
        self.slam_radius = 0.0;
        self.slam_flash = 0.0;
        self.beat_streak = 0;
        self.beat_gamble_mult = 1.0;
        self.beat_gamble_flash = 0.0;
        self.streak_lost_flash = 0.0;
        self.beat_gamble_locked = 1.0;
        self.gamble_bank_flash = 0.0;
        self.gamble_bank_pulse = 0.0;
        self.deliver_streak = 0;
        self.deliver_streak_timer = 0.0;
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
        self.slowmo_timer = 0.0;
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
    fn enter_world_map(&mut self) {
        if self.world_map.is_none() {
            self.world_map = Some(WorldMap::new());
        }
        self.show_instructions = false;
        self.show_world_map = true;
        self.game_over = false;
        self.in_campaign = false;
    }

    /// Start a campaign run from the currently selected world map node.
    fn enter_campaign_level(&mut self) {
        let level_index = self
            .world_map
            .as_ref()
            .map(|m| m.selected_level_index())
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
        self.crabs = spawn_tutorial_crabs(6, (self.width, self.height), &mut rand::rng());
        // A tutorial isn't a scored run — keep bosses far away and never advance the level.
        self.next_boss_score = usize::MAX;
        self.wave_armed = false;
        self.wave_telegraph = 0.0;
        self.show_instructions = false;
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
                    "Catch all the crabs!\n\nMove: Arrow keys / WASD\nAim flashlight: Mouse\nDash: Space\nThrow lasso: Left click\nBeat wave burst: Q\nWhistle (pulls crabs in): E\nStomp (cracks armored crabs): R\nCall on the beat (Dancers answer): F\nDownbeat Slam (full Groove, on beat): G\nDrum Roll (hold T on the beat, release to fire a beam blast): T",
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

        // --- "Press H — How to Play" tutorial prompt, sitting just under the start prompt --------
        let tut_width = MENU_TUTORIAL_CACHE.with(|c| -> GameResult<f32> {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                let mut prompt = Text::new("Press  H  — How to Play     C  — Campaign");
                prompt.set_scale(22.0);
                let w = prompt.measure(ctx)?.x;
                *cache = Some((prompt, w));
            }
            Ok(cache.as_ref().unwrap().1)
        })?;
        MENU_TUTORIAL_CACHE.with(|c| {
            let cache = c.borrow();
            let (prompt, _) = cache.as_ref().unwrap();
            canvas.draw(
                prompt,
                DrawParam::default()
                    .dest(Vec2::new(
                        (width - tut_width) / 2.0,
                        text_y + text_height + pad * 2.0 + 58.0,
                    ))
                    .color(Color::new(0.6, 0.85, 1.0, 0.55 + 0.35 * pulse)),
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
            // Beat phase 0→1 across a beat (0 the instant one lands), so the grass shader can
            // fire a ripple of light outward from center on every downbeat.
            (1.0 - self.beat_timer / self.beat_interval).clamp(0.0, 1.0),
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

        // Ambient atmosphere: a field of slow-drifting motes over the ground (sea spray / drifting
        // spores) that give the space between the action depth and life, tinted to the biome's accent
        // and bobbing gently on the beat. Stateless and cheap (one batched draw), sits above the
        // ground flash but under the tide pools and all the action.
        {
            let (ar, ag, ab) = biome.pulse;
            draw_ambient_motes(
                ctx,
                canvas,
                width,
                height,
                self.time_elapsed,
                self.beat_intensity,
                Color::from_rgb(ar, ag, ab),
            )?;
        }

        // Tide pools — terrain hazards on the ground layer, under the crabs/rope, so the train
        // visibly wades through the water it's being routed around. When a Tide Boss has flooded the
        // arena, the last `boss_flood_pools` entries are its surge water: they always read as water
        // regardless of the biome's native terrain skin (rock/kelp/open), so we draw the biome's own
        // pools with the biome terrain, then the flood slice explicitly as water on top.
        let native_pool_count = self.tide_pools.len().saturating_sub(self.boss_flood_pools);
        draw_tide_pools(
            ctx,
            canvas,
            &self.tide_pools[..native_pool_count],
            self.time_elapsed,
            self.beat_intensity,
            self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0),
            biome.terrain,
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
        draw_delivery_pen(
            ctx,
            canvas,
            self.pen_pos,
            PEN_RADIUS,
            self.time_elapsed,
            self.beat_intensity,
            self.chain_count > 0,
            haul,
            self.deliver_flash,
        )?;

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
            chain_links.extend(
                self.crabs
                    .iter()
                    .filter(|c| c.caught && c.chain_index.is_some())
                    .map(|c| (c.chain_index.unwrap_or(0), c.pos)),
            );
            chain_links.sort_by_key(|&(idx, _)| idx);
            // Only the at-risk gain (live multiplier above the banked-safe floor) heats the rope,
            // so cashing out with B visibly cools it — the risk you're carrying reads on the train.
            let gamble_heat = ((self.beat_gamble_mult - self.beat_gamble_locked) / 2.0).clamp(0.0, 1.0);
            draw_conga_rope(ctx, canvas, self.player_pos, &chain_links, self.time_elapsed, self.beat_intensity, gamble_heat)
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
                urgency,
                self.beat_intensity,
                self.time_elapsed,
            )?;
        }

        // Draw the whip-streaks that yank caught crabs into the train (under the impact rings).
        draw_catch_trails(ctx, canvas, &self.catch_trails)?;

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

        // Draw the Downbeat Slam shockwave — the big gold rhythm-ultimate blast.
        if self.slam_active > 0.0 && self.slam_radius > 0.0 {
            draw_slam_ring(ctx, canvas, self.slam_center, self.slam_radius, SLAM_RADIUS)?;
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
                    self.perf_last_avg_ms, self.perf_last_fps, self.perf_last_worst_ms,
                    self.crabs.len(), self.chain_count,
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
        draw_groove_vignette(ctx, canvas, width, height, self.groove, self.beat_intensity)?;

        // Beat indicator (top right)
        let beat_center = Vec2::new(width - 50.0, 50.0);
        // Wave-incoming telegraph: while a spawn is armed, ring the beat indicator so the player
        // sees the next herd will land on the coming downbeat. Anticipation climbs across the
        // couple of beats before the drop; the ring throbs with the beat phase.
        if self.wave_armed {
            let anticipation = (self.wave_telegraph / (self.beat_interval * 4.0)).min(1.0);
            let beat_phase = 1.0 - (self.beat_timer / self.beat_interval).clamp(0.0, 1.0);
            draw_wave_telegraph(ctx, canvas, beat_center, anticipation, beat_phase, self.frenzy_wave)?;
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
                Some((cached_title, cached_biome, _, _, _, _, _, _, _, _, _)) => {
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
                ));
            }
            let (_, _, title, bg_rect, border_rect, subtitle, title_width, title_height, rect_y, rect_h, sub_width) =
                cache.as_ref().unwrap();
            canvas.draw(bg_rect, DrawParam::default());
            canvas.draw(border_rect, DrawParam::default());
            canvas.draw(
                title,
                DrawParam::default()
                    .dest(Vec2::new((width - title_width) / 2.0, (height - title_height) / 2.0))
                    .color(Color::from_rgb(240, 240, 240)),
            );
            let (pr, pg, pb) = biome.pulse;
            canvas.draw(
                subtitle,
                DrawParam::default()
                    .dest(Vec2::new((width - sub_width) / 2.0, rect_y + rect_h + 12.0))
                    .color(Color::from_rgb(pr, pg, pb)),
            );
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
    /// Text is rebuilt each frame here (cheap: three short strings, only ever on-screen during an
    /// opt-in tutorial, never during a scored run) so the progress line can update live.
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

        // Translucent card backdrop across the top so the instruction text reads over any terrain.
        let card = Mesh::new_rounded_rectangle(
            ctx,
            ggez::graphics::DrawMode::fill(),
            Rect::new(width * 0.5 - 360.0, 24.0, 720.0, 132.0),
            14.0,
            Color::from_rgba(8, 14, 26, 200),
        )?;
        canvas.draw(&card, DrawParam::default());

        let mut title = Text::new(t.title());
        title.set_scale(30.0);
        let tdims: Vec2 = title.measure(ctx)?.into();
        canvas.draw(
            &title,
            DrawParam::default()
                .dest(Vec2::new(width * 0.5 - tdims.x / 2.0, 38.0))
                .color(Color::from_rgb(255, 226, 120)),
        );

        let mut instr = Text::new(t.instruction());
        instr.set_scale(20.0);
        let idims: Vec2 = instr.measure(ctx)?.into();
        canvas.draw(
            &instr,
            DrawParam::default()
                .dest(Vec2::new(width * 0.5 - idims.x / 2.0, 76.0))
                .color(Color::from_rgb(220, 232, 245)),
        );

        let mut prog = Text::new(t.progress_line());
        prog.set_scale(24.0);
        let pdims: Vec2 = prog.measure(ctx)?.into();
        canvas.draw(
            &prog,
            DrawParam::default()
                .dest(Vec2::new(width * 0.5 - pdims.x / 2.0, 124.0))
                .color(Color::from_rgb(120, 255, 150)),
        );

        // Bottom hint so a player who wants out knows how — this is opt-in teaching, no gating.
        let mut hint = Text::new("Esc — back to menu");
        hint.set_scale(18.0);
        let hdims: Vec2 = hint.measure(ctx)?.into();
        canvas.draw(
            &hint,
            DrawParam::default()
                .dest(Vec2::new(width * 0.5 - hdims.x / 2.0, height - 40.0))
                .color(Color::from_rgba(200, 210, 225, 180)),
        );

        // Cleared: a big pulsing "PASSED!" centered while the exit hold runs out.
        if t.completed {
            let scale = 1.0 + t.pass_glow * 0.15;
            let mut passed = Text::new("PASSED!");
            passed.set_scale(80.0);
            let dims: Vec2 = passed.measure(ctx)?.into();
            let dest = Vec2::new(
                width / 2.0 - dims.x * scale / 2.0,
                height * 0.42 - dims.y * scale / 2.0,
            );
            canvas.draw(
                &passed,
                DrawParam::default()
                    .dest(dest + Vec2::splat(3.0))
                    .scale(Vec2::splat(scale))
                    .color(Color::from_rgba(4, 20, 8, 180)),
            );
            canvas.draw(
                &passed,
                DrawParam::default()
                    .dest(dest)
                    .scale(Vec2::splat(scale))
                    .color(Color::from_rgb(110, 255, 140)),
            );
        }
        Ok(())
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
                } else if crab.is_magnet() {
                    // Magnetic field aura — inward-sweeping rings showing its pull radius, so the
                    // player can see the catchment and chase it for the two-for-one cluster catch.
                    let size = crab.scale * CRAB_SIZE;
                    draw_magnet_aura(ctx, canvas, pos, size, 240.0, self.time_elapsed, crab.is_magnet_lured(), crab.is_magnet_charged())?;
                } else if crab.is_thief() {
                    // Thief marker — a sly green ring while it prowls, flaring into a fast gnaw-ring
                    // once it's latched onto the tail so the theft-in-progress reads at a glance.
                    let size = crab.scale * CRAB_SIZE;
                    draw_thief_aura(ctx, canvas, pos, size, crab.is_latched(), crab.is_magnet_intercepted(), crab.is_thief_lured(), self.time_elapsed)?;
                } else if crab.is_golden() {
                    // Golden crab shine — a shimmering ring of orbiting sparkles so the rare prize
                    // catches the eye across the whole field and reads as "chase this one!".
                    let size = crab.scale * CRAB_SIZE;
                    draw_golden_sparkle(ctx, canvas, pos, size, self.time_elapsed, crab.is_magnet_snared())?;
                }
            }
        }
        canvas.set_blend_mode(original_blend);
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
        let ring_col = if on_beat { [1.0, 0.85, 0.35] } else { [0.9, 0.6, 0.3] };
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
        let Some(crab) = self.crabs.get_mut(idx) else { return };
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
    /// Downbeat Slam (G) — the Groove-meter ultimate. Only fires when the meter is FULL and the press
    /// lands on the beat: an enormous shockwave erupts from the player and yanks every free crab in a
    /// wide radius straight into the conga train at once (a mass catch), pays out a score bonus, and
    /// drains the whole meter. This is the spectacle payoff for playing in the pocket. Off-beat, or
    /// with a meter that isn't topped out, it fizzles with a distinct message so the miss reads.
    fn downbeat_slam(&mut self, ctx: &mut Context) {
        let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        // Gate 1: the meter must be full. This is what makes the groove meter finally *do* something.
        if self.groove < 0.999 {
            self.shop_denied = self.shop_denied.max(0.5);
            self.floating_texts.spawn(
                "GROOVE not full".to_string(),
                center - Vec2::new(70.0, 70.0),
                24.0,
                [0.8, 0.85, 0.9, 0.9],
            );
            return;
        }
        // Gate 2: it must land on the beat — the whole point is rhythm mastery.
        if !self.on_beat_now() {
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
        self.particle_system.spawn_milestone_fireworks(center, n.max(8), &mut rng);
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
            // Decay the perk-shop buy/deny flashes so they're a brief pop, not a stuck glow.
            self.shop_flash = (self.shop_flash - mdt * 2.5).max(0.0);
            self.shop_denied = (self.shop_denied - mdt * 2.5).max(0.0);
            return Ok(());
        }

        let mut dt = ctx.time.delta().as_secs_f32();

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
                // self.crabs.len() and self.chain_count, no extra scan.
                println!(
                    "[perf] {} frames in {:.1}s — avg {:.2}ms ({:.0} fps), worst {:.2}ms — {} crabs ({} chained)",
                    self.perf_frame_count,
                    self.perf_time_accum,
                    avg_ms,
                    1000.0 / avg_ms,
                    worst_ms,
                    self.crabs.len(),
                    self.chain_count,
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

        // Tutorial session bookkeeping: keep the sandbox stocked, detect the pass condition, and
        // run a short celebratory hold before handing control back to the title screen. Kept here
        // in the live path (not the paused menu gate) because a rhythm lesson needs the sim ticking.
        if self.tutorial.is_some() {
            // Real (undilated) time for the exit hold so the celebration is a fixed wall-clock
            // length regardless of any slow-mo the catch triggered.
            let real_dt = ctx.time.delta().as_secs_f32();
            // If the learner clears the whole sandbox before passing, quietly restock so they can
            // keep practising instead of standing in an empty field.
            if !self.tutorial.as_ref().unwrap().completed && self.crabs.iter().all(|c| c.caught) {
                self.crabs = spawn_tutorial_crabs(6, (self.width, self.height), &mut rand::rng());
            }
            let t = self.tutorial.as_mut().unwrap();
            if t.completed {
                t.pass_glow = (t.pass_glow + real_dt * 2.5).min(1.0);
                t.exit_timer = (t.exit_timer - real_dt).max(0.0);
                if t.exit_timer <= 0.0 {
                    // Opt-in exit: back to the title screen, never through game_over, so this
                    // teaching run leaves the persistent career untouched.
                    self.tutorial = None;
                    self.show_instructions = true;
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
                let _ = self.sounds.upgrade.play_detached(ctx);
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
                        self.particle_system.spawn_fissure_geyser(c, r, &mut rand::rng());
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
                    let _ = self.sounds.upgrade.play_detached(ctx);
                } else {
                    self.beat_intensity = (self.beat_intensity + 0.6).min(2.0);
                    self.on_beat_flash = self.on_beat_flash.max(0.4);
                }
            }
            // Beat camera shake — strength grows with chain length. Also collects caught-crab
            // positions for the beat-pulse sparkle rings just below: both used to run their own
            // separate `.filter(|c| c.caught)` pass over self.crabs (two counts + a fresh
            // Vec::collect() every single beat), so fold them into one pass that reuses the
            // persistent chain_positions_buf (already used later this frame by catch_by_chain,
            // and not read in between) instead of allocating a new Vec.
            self.chain_positions_buf.clear();
            self.chain_positions_buf
                .extend(self.crabs.iter().filter(|c| c.caught).map(|c| c.pos));
            let chain_len = self.chain_positions_buf.len();
            if chain_len > 0 {
                // The downbeat footfall lands heavier than the between-beats — a bigger, capped
                // shake so the bar's "1" feels like the whole train stomping down together.
                let downbeat_scale = if downbeat { 1.5 } else { 1.0 };
                let shake_mag = (2.0 + chain_len as f32 * 0.8).min(14.0) * downbeat_scale;
                self.screen_shake = self.screen_shake.max(shake_mag);
                // Random kick direction
                let kick_angle = rand::rng().random_range(0.0_f32..std::f32::consts::TAU);
                self.screen_shake_vel = Vec2::new(kick_angle.cos(), kick_angle.sin()) * shake_mag * 60.0;
            }
            // Beat-pulse sparkle rings from all caught crabs — brighter on the bar downbeat so
            // the "1" of the bar pops harder than the beats between it.
            let pulse_strength = if downbeat { 1.5 } else { 1.0 };
            self.particle_system
                .spawn_beat_pulse(&self.chain_positions_buf, pulse_strength, chain_len, &mut rand::rng());
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
                let dir = if dir == Vec2::ZERO { Vec2::new(0.0, -1.0) } else { dir };
                crab.pos += dir * DANCER_HOP;
                crab.pos.x = crab.pos.x.clamp(0.0, self.width - crab.scale);
                crab.pos.y = crab.pos.y.clamp(0.0, self.height - crab.scale);
                crab.vel = dir; // face the hop; unit vel so the drift branch stays gentle
                crab.join_pulse = 1.0; // reuse the join squash-pop as a little "landed" bounce
                // A Dancer bolting away from the player becomes a fear source; note where it
                // landed so the ripple pass below can spook nearby calm crabs. Answering Dancers
                // (hopping toward the player, charmed) don't scare anyone — only fleeing ones do.
                if crab.answering_call <= 0.0 && dist < 240.0 {
                    dancer_hops.push(crab.pos);
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
                    ((p.x / cell_size).floor() as i32, (p.y / cell_size).floor() as i32)
                };
                // Same unbounded-key fix as contagion_grid_buf/armored_anchor_grid_buf: a plain
                // per-bucket clear left one entry per grid cell ever visited by a hopping Dancer,
                // which only grows over a session as the herd roams the whole level. A full
                // clear() keeps the map's allocated capacity (still avoids a realloc most beats)
                // but bounds the key count to "cells touched this beat".
                self.dancer_startle_grid_buf.clear();
                for (i, &pos) in dancer_hops.iter().enumerate() {
                    self.dancer_startle_grid_buf.entry(cell_of(pos)).or_default().push(i);
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
                                if let Some(candidates) = self.dancer_startle_grid_buf.get(&(cx + dx, cy + dy)) {
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
                            let dir = if dir == Vec2::ZERO { Vec2::new(0.0, -1.0) } else { dir };
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
                                if let Some(candidates) = self.dancer_startle_grid_buf.get(&(cx + dx, cy + dy)) {
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
                    } else if crab.is_armored() {
                        if crab.boss_health <= 0.0 {
                            continue;
                        }
                        let (cx, cy) = cell_of(crab.pos);
                        let mut hit = false;
                        'search_armored: for dx in -1..=1 {
                            for dy in -1..=1 {
                                if let Some(candidates) = self.dancer_startle_grid_buf.get(&(cx + dx, cy + dy)) {
                                    for &i in candidates {
                                        if crab.pos.distance_squared(dancer_hops[i]) < DANCER_CHIP_RADIUS_SQ {
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
                            chipped.push((crab.pos, crab.boss_health <= 0.0));
                        }
                    } else if crab.is_magnet() {
                        if crab.in_flashlight || crab.magnet_charged > 0.0 {
                            continue;
                        }
                        let (cx, cy) = cell_of(crab.pos);
                        let mut hit = false;
                        'search_magnet: for dx in -1..=1 {
                            for dy in -1..=1 {
                                if let Some(candidates) = self.dancer_startle_grid_buf.get(&(cx + dx, cy + dy)) {
                                    for &i in candidates {
                                        if crab.pos.distance_squared(dancer_hops[i]) < DANCER_KICK_RADIUS_SQ {
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
                                if let Some(candidates) = self.dancer_startle_grid_buf.get(&(cx + dx, cy + dy)) {
                                    for &i in candidates {
                                        let src = dancer_hops[i];
                                        let d = src.distance(crab.pos);
                                        if d < DANCER_STARTLE_RADIUS && nearest.map_or(true, |(nd, _)| d < nd) {
                                            nearest = Some((d, src));
                                        }
                                    }
                                }
                            }
                        }
                        if let Some((d, src)) = nearest {
                            let outward = (crab.pos - src).normalize_or_zero();
                            let outward = if outward == Vec2::ZERO { Vec2::new(0.0, -1.0) } else { outward };
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
                for &(pos, broke) in chipped.iter() {
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
        if self.reef_hit_flash > 0.0 {
            self.reef_hit_flash = (self.reef_hit_flash - dt * 3.5).max(0.0);
        }
        // Groove Gamble feedback pulses decay each frame.
        if self.beat_gamble_flash > 0.0 {
            self.beat_gamble_flash = (self.beat_gamble_flash - dt * 3.5).max(0.0);
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

        // Groove meter decays over time; when it empties the on-beat streak lapses too.
        if self.groove > 0.0 {
            self.groove = (self.groove - dt * 0.18).max(0.0);
            if self.groove <= 0.0 {
                self.beat_streak = 0;
                // The Gamble heat fades with the groove — a quiet lapse, not a punished break, so
                // idling loses the unbanked climb gracefully. Whatever was cashed out with B stays.
                self.beat_gamble_mult = self.beat_gamble_locked;
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

        // Drum Roll (hold T): poll the held key here rather than off the key-down event, since the
        // event fires unreliably on key-repeat and we need a clean "held across beats" charge. The
        // per-beat hit counting lives in the beat handler; here we only edge-detect press/release
        // and drive the timers. Releasing after landing at least one on-beat roll hit FIRES a
        // focused beam blast; releasing with nothing charged just cancels quietly.
        let t_held = !self.show_instructions
            && !self.game_over
            && ctx.keyboard.is_key_pressed(ggez::input::keyboard::KeyCode::T);
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

        // Biome wrinkle (Neon Kelp Forest): clinging fronds can snag and strip the tail if you
        // route a long train through the weeds instead of around them.
        self.snag_chain_on_kelp(dt);

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
        for (pos, color) in self.penned_marchers.update(dt) {
            self.particle_system
                .spawn_catch_effect(pos, color, CrabType::Normal, &mut rand::rng());
        }
        // Idle-decay the delivery streak: if too long passes between banks, drop a notch so the
        // multiplier tracks recent cashing tempo. Each notch grants a fresh grace window.
        if self.deliver_streak > 0 {
            self.deliver_streak_timer = (self.deliver_streak_timer - dt).max(0.0);
            if self.deliver_streak_timer <= 0.0 {
                self.deliver_streak -= 1;
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
            // The beat_bonus is only >1.0 when this cast landed on the beat (see reward_on_beat_tool),
            // so it doubles as our "was this an on-beat cast?" flag for the rhythm-native Thief shake.
            let on_beat_cast = self.whistle_beat_bonus > 1.0;
            self.whistle_active = (self.whistle_active - dt).max(0.0);
            self.whistle_radius = (self.whistle_radius + WHISTLE_RING_SPEED * dt).min(whistle_max_r);
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
                // Crack an armored shell wide open the instant the shockwave reaches it.
                if crab.is_armored() && crab.boss_health > 0.0 {
                    crab.boss_health = 0.0;
                    cracked.push(crab.pos);
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
            for &pos in cracked.iter() {
                self.floating_texts.spawn(
                    "SHELL CRACKED!".to_string(),
                    pos - Vec2::new(70.0, 40.0),
                    26.0,
                    [0.7, 0.85, 1.0, 1.0],
                );
                self.spawn_catch_shockwave(pos, [0.7, 0.8, 0.95]);
            }
            self.stomp_cracked_buf = cracked; // hand the buffer back for reuse next frame
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
                    let mut to_catch = std::mem::take(&mut self.lasso_catch_buf);
                    to_catch.clear();
                    to_catch.extend(
                        self.crabs.iter().enumerate()
                            .filter(|(_, c)| c.is_catchable() && tip.distance(c.pos) < grab_r)
                            .map(|(i, _)| i),
                    );
                    let mut rng = rand::rng();
                    // Yanking a crab off the sand spooks the herd around the snatch point, same as
                    // a beam or chain catch — collected here and fired after the loop so the lasso
                    // stampede reads as fear rippling outward from where the rope bit.
                    let mut lasso_startle_origins = std::mem::take(&mut self.lasso_startle_buf);
                    lasso_startle_origins.clear();
                    for i in to_catch.iter().copied() {
                        let pos = self.crabs[i].pos;
                        let crab_type = self.crabs[i].crab_type;
                        let crab_color = self.crabs[i].crab_color();
                        self.particle_system.spawn_catch_effect(pos, crab_color, crab_type, &mut rng);
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
                        if self.score > 0 && self.score % 10 == 0 {
                            let _ = self.sounds.upgrade.play_detached(ctx);
                            self.pending_upgrade = true;
                        }
                    }
                    for &origin in lasso_startle_origins.iter() {
                        self.emit_catch_startle(origin);
                    }
                    self.lasso_catch_buf = to_catch; // hand buffers back for reuse next frame
                    self.lasso_startle_buf = lasso_startle_origins;
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
                    spawn_tide_boss((self.width, self.height), &mut rand::rng(), BOSS_MAX_HEALTH),
                    "A TIDE BOSS SURGES IN!",
                    "Hold your light — but keep your train clear of its pulse!",
                    [0.35, 0.8, 1.0, 1.0],
                ),
                2 => (
                    spawn_rhythm_boss((self.width, self.height), &mut rand::rng(), BOSS_MAX_HEALTH),
                    "THE REEF DJ DROPS IN!",
                    "Echo the lit pips with light — or catch its dancers on a hot beat!",
                    [0.75, 0.4, 1.0, 1.0],
                ),
                _ => (
                    spawn_boss((self.width, self.height), &mut rand::rng(), BOSS_MAX_HEALTH),
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

        // Game over if too many free crabs accumulate (overwhelmed). Reuses the single-pass tally
        // from above (plus the +1 for a boss spawned this frame) instead of a fresh linear scan.
        if free_crab_count >= 80 {
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

        if self.show_world_map {
            if let Some(map) = &self.world_map {
                self.sounds.action_music.pause();
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
