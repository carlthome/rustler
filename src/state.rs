use std::{collections::VecDeque, fs};

use crevice::std140::AsStd140;
use ggez::audio::SoundSource;
use ggez::audio::Source;
use ggez::glam::Vec2;
use ggez::graphics::{Image, ShaderBuilder, ShaderParams, ShaderParamsBuilder};
use ggez::{Context, GameResult};
use rand::Rng;
use rand::prelude::IndexedRandom;

#[derive(Copy, Clone, Debug, AsStd140)]
pub struct PostProcessUniform {
    pub groove: f32,
    pub time: f32,
    pub screen_width: f32,
    pub screen_height: f32,
    /// 0 = normal, 1 = full desaturate/title-card effect
    pub title_card_t: f32,
}

use crate::bot::BotState;
use crate::constants::*;
use crate::enemies::{CrabType, EnemyCrab};
use crate::graphics::{FloatingTextSystem, ParticleSystem, PennedMarcherSystem};
use crate::levels::Level;
use crate::skins::PlayerSkin;
use crate::sounds;
use crate::tutorial::Tutorial;
use crate::upgrade::UPGRADE_FIRST_AT;
use crate::world_map::WorldMap;
use crate::{get_levels, pick_pen_pos, pick_tide_pools};

pub struct GameSounds {
    pub(crate) intro_music: Source,
    pub(crate) action_music: Source,
    pub(crate) outro_music: Source,
    pub(crate) upgrade: Source,
    pub(crate) success: Source,
    pub(crate) success2: Source,
    /// Ambient NPC King Crab conga train rumble — left-panned version.
    /// Volume is driven each frame by distance AND the leader's bearing (equal-power pan),
    /// so the train is heard swelling *and* placed left/right — the "heard before seen" radar.
    pub(crate) king_crab_rumble_l: Source,
    /// Ambient NPC King Crab conga train rumble — right-panned version. Paired with `_l`.
    pub(crate) king_crab_rumble_r: Source,
    pub(crate) hihat: Source,
    /// Short bright chirp for the flashlight toggle (F key) — a snappy UI beep.
    pub(crate) flashlight_toggle: Source,
    /// Synthesized FM-bell arpeggio, an alternative "coin get" chime layered in alongside the
    /// sampled `success`/`success2` catch sounds for extra retro sparkle.
    pub(crate) coin_chime: Source,
    /// Ambient synth pad played on entering the campaign world map — a calm, atmospheric moment
    /// between levels, long swell/tail with a slow filter sweep, delay and stereo auto-pan.
    pub(crate) world_map_pad: Source,
    /// Synthesised finger-whistle for the Whistle tool.
    pub(crate) whistle_sfx: Source,
    /// Synthesised stomp thud (kick + noise crack) for the Stomp tool.
    pub(crate) stomp_sfx: Source,
    /// Synthesised whoosh for the Lasso throw release.
    pub(crate) lasso_sfx: Source,
    /// Five crab-theme loops (Duck Game / Deus Ex ABA melodies), one per archetype group.
    /// 0=normal/fast/big  1=dancer/splitter  2=thief/sneaky  3=boss/armored  4=golden/magnet/hermit
    pub(crate) crab_themes: [Source; 5],
    /// Spatial King Crab boss rumble — left-panned bright version.
    /// Volume driven per-frame by boss distance and angle relative to player.
    pub(crate) king_crab_l: Source,
    /// Spatial King Crab boss rumble — right-panned bright version.
    pub(crate) king_crab_r: Source,
    /// Spatial King Crab boss rumble — soft/distant version with baked room echo.
    /// Crossfades in as the boss moves further away (brightness rolloff approximation).
    pub(crate) king_crab_soft: Source,
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
pub fn play_catch_sound(
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
    // Pure synthesized FM chime — no OGG samples. The synth voice handles rapid
    // multi-catch without the crackling/phase artifacts the sampled files produced
    // when many copies played simultaneously, and fits the retro chiptune direction.
    sounds.coin_chime.set_pitch(pitch);
    let _ = sounds.coin_chime.play_detached(ctx);
}

pub struct Flashlight {
    pub(crate) on: bool,
    pub(crate) cone_upgrade: f32,
    pub(crate) range_upgrade: f32,
    pub(crate) laser_level: u32,
    /// 0..=1 charge level. Drains while on, recharges when off (faster on-beat).
    pub(crate) charge: f32,
    /// Smoothed aim direction toward the auto-targeted King Crab (or last dir if no target).
    pub(crate) aim_dir: ggez::glam::Vec2,
}

#[derive(Clone)]
pub enum LevelTexture {
    Grass,
    Sand,
}

pub struct GameTextures {
    pub(crate) grass: Image,
    pub(crate) sand: Image,
    pub(crate) player: Image,
}

/// Weather ambience state. Transitions are smooth: the discrete `target` a random walk picks
/// each step is what `weather_intensity` eases toward, so the visuals never hard-cut between states.
/// Ordered calm→wild so escalation is just "step the index".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WeatherState {
    Sunny,
    Cloudy,
    Rain,
    HeavyRain,
    Storm,
}

impl WeatherState {
    /// 0..1 "how wild" — the target `weather_intensity` eases toward. Drives streak density,
    /// tint strength, catch-radius reduction and whether lightning can fire.
    pub(crate) fn intensity(self) -> f32 {
        match self {
            WeatherState::Sunny => 0.0,
            WeatherState::Cloudy => 0.28,
            WeatherState::Rain => 0.55,
            WeatherState::HeavyRain => 0.80,
            WeatherState::Storm => 1.0,
        }
    }
}

/// Time-of-day phase over a run (~8 min). `day_phase_t` (0..1) is the continuous clock; this is
/// just the coarse label for readability. Dawn→Day→Dusk→Night.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DayPhase {
    Dawn,
    Day,
    Dusk,
    Night,
}

/// Phases of a single lasso throw. A throw is a real skill-shot now: the loop flies out to the aim
/// point over `LASSO_THROW_TIME` (Throwing), then either bites whatever crabs are clustered under it
/// (Snag — a brief tightening squeeze) and reels them back with visible rope tension (Dragging), or
/// finds nothing and flops down empty with a dust puff (Miss). `Idle` = no throw live.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LassoPhase {
    /// No lasso activity — nothing drawn, nothing blocking the next throw.
    Idle,
    /// Mouse is held down: the rope swirls above the player, growing with charge. Fires on release.
    Winding,
    /// Rope is in the air flying toward the target.
    Throwing,
    /// Loop just landed on a crab — brief tighten/squeeze pop before the drag begins.
    Snag,
    /// Reeling the caught crabs back toward the player.
    Dragging,
    /// Empty throw — loop settling in a dust puff.
    Miss,
}

/// Ambient wandering NPC conga train — a King Crab leading a few followers across the world.
/// Visual-only: it does not steal from or react to the player. It's world life, like weather.
pub struct NpcCongaTrain {
    pub leader_pos: Vec2,
    pub leader_vel: Vec2,
    pub target: Vec2,
    pub target_timer: f32,
    /// Sampled leader positions (pushed when leader moves >6px); followers trail by index offset.
    pub path_history: VecDeque<Vec2>,
    pub follower_types: Vec<CrabType>,
    /// Target volume for the rumble SFX, computed each frame from distance to player.
    /// Smoothed and applied by the EventHandler::update caller which has ctx access.
    pub target_vol: f32,
    /// Procedurally generated name — stable for the session (Shadow of Mordor style individuality).
    pub name: String,
    /// Visual scale of the King Crab leader — grows with conga length; tier sets the floor.
    pub leader_scale: f32,
    /// Minimum scale for this tier — leader can only grow above this, never shrink below it.
    pub base_scale: f32,
    /// Brief idle pause at destination before picking the next wander target (Rain World agency feel).
    pub idle_timer: f32,
    /// Preferred territory centre — each NPC biases its wander targets toward its own quadrant.
    pub territory_center: Vec2,
    /// Cooldown between steal events so one pass doesn't strip the whole chain in a single frame.
    pub steal_cooldown: f32,
    /// Time since this NPC last caught a free crab (throttles free-crab collection).
    pub catch_cooldown: f32,
}

/// Generate a King Crab name. Hits four tones: Dark Souls boss grandiosity, crab rave energy,
/// pirate flair, and a smattering of completely vanilla comedy names ("Kevin").
pub fn gen_king_crab_name(rng: &mut impl rand::Rng) -> String {
    const SOLO_NAMES: &[&str] = &[
        "Kevin", "Sandra", "Dave", "Gerald", "Steve", "Janet", "Barry", "Brenda", "Trevor", "Karen",
    ];
    let solo_roll: f32 = rng.random();
    if solo_roll < 0.15 {
        return SOLO_NAMES.choose(rng).unwrap().to_string();
    }

    const TITLES: &[&str] = &[
        "Gravelord",
        "The Undying",
        "Clawkeeper of the Brackish Deep",
        "Herald of the Eternal Tide",
        "Scuttlefiend,",
        "Devourer of Shores",
        "Ashen",
        "Lord of the Sunken Reef",
        "The Hollow",
        "Keeper of the Last Shell",
        "Sovereign of the Abyssal Shallows",
        "The Forsaken",
        "Bearer of the Cursed Carapace",
        "Watcher of the Drowned Coast",
        "Misterhult",
        "Cap'n",
        "First Mate",
        "Barnacle",
        "The Scurvy",
        "Admiral",
        "Quartermaster",
        "DJ",
        "Rave King",
        "The Eternal",
        "MC",
        "Sideways Champion",
        "Drop Lord",
        "The Eternal Groove of",
        "Shellmaster",
        "The Immortal",
        "Ancient",
    ];

    const NAMES: &[&str] = &[
        "Pinchfeast",
        "Moltveil",
        "Chelicerae",
        "Scuttlegrim",
        "Brinewraith",
        "Tidecurse",
        "Carapace",
        "Saltborn",
        "Shellreaper",
        "Abysswalker",
        "Duskshell",
        "Emberclaw",
        "Grimtide",
        "Voidmolt",
        "Pete",
        "Clawbeard",
        "Snippy",
        "the Saltbitten",
        "Ironpincer",
        "Buccaneers",
        "Moultzilla",
        "Snapsalot",
        "Groove",
        "Bounceback",
        "Sidestep",
        "the Bass Drop",
        "Shellshaker",
        "Clawdrop",
        "the Unbroken",
        "Razorshell",
    ];

    let title = TITLES.choose(rng).unwrap();
    let name = NAMES.choose(rng).unwrap();
    format!("{} {}", title, name)
}

impl NpcCongaTrain {
    pub fn new(world_width: f32, world_height: f32) -> Self {
        Self::new_at(world_width, world_height, 0)
    }

    pub fn new_at(world_width: f32, world_height: f32, index: usize) -> Self {
        // Three distinct tiers: small scout, medium wanderer, large elder.
        // Scale, speed, and follower count all differ so they read instantly at a glance.
        let (sx, sy, tc_x, tc_y, leader_scale, speed_hint) = match index {
            0 => (0.2, 0.3, 0.25, 0.3, 1.2_f32, 110.0_f32), // small/fast scout, top-left territory
            1 => (0.8, 0.2, 0.75, 0.25, 1.8_f32, 80.0_f32), // medium wanderer, top-right territory
            _ => (0.5, 0.8, 0.5, 0.75, 2.4_f32, 55.0_f32),  // large elder, bottom territory
        };
        let _ = speed_hint; // stored per-train would need another field; use leader_scale as proxy in update
        let start = Vec2::new(world_width * sx, world_height * sy);
        let territory_center = Vec2::new(world_width * tc_x, world_height * tc_y);
        // Initial target biased toward territory center
        let target = territory_center + Vec2::new(world_width * 0.1, world_height * 0.05);
        let follower_types = match index {
            // Small scout: fast light crabs
            0 => vec![CrabType::Fast, CrabType::Sneaky, CrabType::Normal],
            // Medium wanderer: balanced mix
            1 => vec![
                CrabType::Armored,
                CrabType::Normal,
                CrabType::Fast,
                CrabType::Magnet,
                CrabType::Dancer,
            ],
            // Large elder: heavy diverse retinue
            _ => vec![
                CrabType::Big,
                CrabType::Dancer,
                CrabType::Golden,
                CrabType::Normal,
                CrabType::Sneaky,
                CrabType::Hermit,
                CrabType::Fast,
            ],
        };
        let mut history = VecDeque::new();
        history.push_back(start);
        let name = gen_king_crab_name(&mut rand::rng());
        Self {
            leader_pos: start,
            leader_vel: Vec2::ZERO,
            target,
            target_timer: 8.0 + index as f32 * 5.0,
            path_history: history,
            follower_types,
            target_vol: 0.0,
            name,
            leader_scale,
            base_scale: leader_scale,
            idle_timer: 0.0,
            territory_center,
            steal_cooldown: 0.0,
            catch_cooldown: 0.0,
        }
    }
}

pub struct MainState {
    pub(crate) player_pos: Vec2,              // Player position
    pub(crate) player_vel: Vec2,              // Player velocity (for smooth movement)
    pub(crate) mouse_pos: Vec2,               // Mouse position for flashlight aiming
    pub(crate) crabs: Vec<EnemyCrab>,         // List of crabs in the game
    pub(crate) score: usize,                  // Current score
    pub(crate) spawn_timer: f32,              // Timer for spawning new crabs
    pub(crate) time_elapsed: f32,             // Time since game start
    pub(crate) menu_time: f32, // Free-running clock for the title/menu screen animation
    pub(crate) game_over: bool, // Game over flag
    pub(crate) sounds: GameSounds, // All game sound effects
    pub(crate) beat_synth: sounds::BeatSynth, // Procedural kick drum played on every beat tick
    pub(crate) flashlight: Flashlight, // Flashlight settings and upgrades
    pub(crate) show_instructions: bool, // Show instructions screen
    pub(crate) show_how_to_play_text: bool, // Show plain-text How to Play card instead of Home menu
    // Active cosmetic loadout for the player character (hat, facial hair, accessory).
    // Loaded from career.txt on startup; changed from the title screen customisation menu.
    // Purely visual — never affects gameplay.
    pub(crate) player_skin: PlayerSkin,
    // Player crab name shown on the title screen and above the crab in-game.
    pub(crate) player_name: String,
    // Which cosmetic column the title-screen skin picker currently focuses: 0=Hat, 1=FacialHair, 2=Accessory.
    pub(crate) skin_slot: usize,
    // Which menu page is active on the title screen: 0=Home, 1=Loadout.
    pub(crate) menu_page: usize,
    // Which button is highlighted in the Home page button list (0..NUM_MENU_BUTTONS).
    pub(crate) menu_selection: usize,
    // Campaign world map — `Some` once the player has entered campaign mode from the title.
    // Persists across runs so node completion carries over. `show_world_map` gates whether the
    // map screen is currently visible; `in_campaign` is true during an active campaign run.
    pub(crate) world_map: Option<WorldMap>,
    pub(crate) show_world_map: bool,
    pub(crate) in_campaign: bool,
    // Active "How to Play" tutorial session, if any. `Some` while a scripted learn-session runs;
    // it uses the normal live update/draw path but constrains the run (no bosses, no wave
    // escalation, no level advance) and tracks its own machine-readable pass condition. `None`
    // during a real run or on the menus.
    pub(crate) tutorial: Option<Tutorial>,
    pub(crate) last_dir: Vec2,   // Last movement direction for flashlight
    pub(crate) shake_timer: f32, // Timer for crab shake effect
    pub(crate) time_since_catch: f32, // Time since last crab was caught
    pub(crate) boost_timer: f32, // Timer for speed boost
    pub(crate) boost_cooldown: f32, // Cooldown to prevent holding space
    pub(crate) sprint_stamina: f32, // Shift sprint meter: drains while sprinting, refills after
    pub(crate) levels: Vec<Level>, // List of levels with patterns
    pub(crate) current_level: usize, // Current level index
    pub(crate) current_pattern: usize, // Current pattern index within the level
    pub(crate) pattern_timer: f32, // Timer for current pattern duration
    pub(crate) debug_mode: bool, // Debug mode flag
    pub(crate) pending_upgrade: bool, // Whether upgrade screen should be shown
    // The three options rolled for the CURRENT upgrade screen (indices into UPGRADE_POOL). Rolled
    // once when the upgrade is queued (see roll_upgrade_offer / check_upgrade_unlock), NOT in draw,
    // so the cards stay stable instead of reshuffling every frame. Read by draw_upgrade_screen and
    // apply_upgrade so both always agree on which three are on offer.
    pub(crate) offered_upgrades: [usize; 3],
    // Persistent player top-speed multiplier, folded into base_speed in controls.rs. Tradeoff
    // upgrades push it up (nimbler) or down (sluggish); 1.0 is neutral. A stat knob, not a new
    // mechanic — the movement it scales already exists.
    pub(crate) speed_mult: f32,
    pub(crate) next_upgrade_score: usize, // Score threshold that triggers the next upgrade (rises each unlock)
    pub(crate) best_time: f32,            // Fastest time to catch all crabs
    // --- Meta-progression: a single persistent thread that survives across runs, so ending a
    // run (win or loss) still banks progress into a career you carry forward. Persisted to
    // career.txt as three whitespace-separated integers: best_score total_score runs.
    pub(crate) career_best_score: usize, // Highest single-run score ever reached
    pub(crate) career_total_score: usize, // Sum of every run's final score (lifetime crabs banked)
    pub(crate) career_runs: usize,       // How many runs have ended
    pub(crate) run_recorded: bool, // Guard so the current run is banked into career exactly once
    pub(crate) run_is_new_best: bool, // Did the just-ended run set a new career best? (for game-over flourish)
    // Spend side of meta-progression: banked crabs (career_total_score) are a currency you spend
    // on the title screen for PERMANENT starting tool ranks — a head-start that persists across
    // runs, so even a losing run buys you closer to your next unlock. `career_spent` is the ledger
    // of crabs already committed; available = career_total_score - career_spent. The four
    // start_*_rank fields are the ranks a fresh run begins each tool at (capped low so it's a
    // leg-up, not a run-trivializer). Persisted alongside best/total/runs in career.txt.
    pub(crate) career_spent: usize,
    pub(crate) start_beam_rank: u32,
    pub(crate) start_lasso_rank: u32,
    pub(crate) start_whistle_rank: u32,
    pub(crate) start_stomp_rank: u32,
    pub(crate) shop_flash: f32, // brief green flash on the last-bought perk (title-screen juice)
    pub(crate) shop_denied: f32, // brief red flash when a purchase is refused (can't afford / maxed)
    pub(crate) jam_timer: f32,   // B-key jam emote: >0 while the crab is vibing (drives animation)
    pub(crate) width: f32,       // Virtual width of the game (viewport)
    pub(crate) height: f32,      // Virtual height of the game (viewport)
    pub(crate) world_width: f32, // Full playfield width — larger than the viewport; the camera scrolls across it
    pub(crate) world_height: f32, // Full playfield height — larger than the viewport
    pub(crate) camera_origin: Vec2, // Top-left world coord of the visible viewport this frame (player-following, clamped to world bounds). Read by draw() and the mouse handlers to map screen<->world.
    pub(crate) shader: ggez::graphics::Shader, // Shader for grass rendering
    pub(crate) flashlight_shader: ggez::graphics::Shader, // Shader for flashlight rendering
    pub(crate) flashlight_cone_image: ggez::graphics::Image, // Offscreen target for flashlight cone (isolated from scene canvas to avoid wgpu group-3 bind leak)
    pub(crate) scene_image: ggez::graphics::Image, // Offscreen render target for post-processing
    pub(crate) postprocess_shader: ggez::graphics::Shader, // Screen-space post-process shader
    pub(crate) postprocess_params: ShaderParams<PostProcessUniform>, // Params for post-process shader
    pub(crate) particle_system: ParticleSystem,                      // Particle effects system
    pub(crate) level_title: String,                                  // Title of the current level
    pub(crate) level_title_timer: f32, // Timer for displaying level title
    pub(crate) subtitle: String,       // Random subtitle for instructions screen
    pub(crate) position_history: VecDeque<Vec2>,
    pub(crate) chain_count: usize,
    /// Monotonic count of every crab caught this run — unlike `chain_count`, it never drops when the
    /// train is banked at the pen, snaps, or gets scattered by a King Crab hit. Used by the bot
    /// playtests to assert "the catching verb produced a catch" without racing a chain reset.
    pub(crate) total_caught: usize,
    pub(crate) beat_timer: f32,
    // Live beat interval in seconds, = BEAT_INTERVAL / current stage's tempo multiplier. Recomputed
    // whenever the intensity stage climbs so the whole game (beat cadence, every phase animation
    // keyed off beat_timer, spawn quantization) speeds up in step with the difficulty ramp. All
    // per-frame reads use this, not the BEAT_INTERVAL const, so tempo shifts stay in sync.
    pub(crate) beat_interval: f32,
    pub(crate) beat_intensity: f32,
    pub(crate) music_intensity: f32,
    pub(crate) on_beat_flash: f32,
    pub(crate) groove: f32, // 0..=1 on-beat "groove" meter — fills on rhythmic catches, decays over time
    pub(crate) beat_streak: u32, // consecutive on-beat catches; escalates the score bonus
    // Consecutive PERFECT (tight-window) catches. Distinct from beat_streak only in that it counts
    // ONLY the flawless hits inside PERFECT_WINDOW, not the looser on-beat ones — so it can drive a
    // super-linear dedicated bonus that rewards a sustained flawless run far out of proportion to a
    // merely-good one. A single non-perfect catch (on-beat or off) resets it. It does NOT touch the
    // global gamble multiplier or banking — its payoff is additive to score and flows into the
    // RHYTHM BONUS readout, keeping the banking risk/reward axis untouched.
    pub(crate) perfect_streak: u32,
    pub(crate) perfect_flash: f32, // 1→0 one-shot flash when a PERFECT lands, so the flawless hit reads on screen
    // Cumulative rhythm-attributable score: the running total of EXTRA points playing in the pocket
    // has earned over a hypothetical flat-1x run. On every award (catch and bank) we add the delta
    // between what actually paid out and what the same event would have paid at neutral rhythm
    // multipliers. It's the "how far ahead the beat put you" readout the roadmap asks for — mastery
    // made legible — and it is display-only: it accumulates score already awarded, never adds any.
    pub(crate) rhythm_bonus_score: usize,
    pub(crate) rhythm_bonus_flash: f32, // brief pulse when the tally jumps (a fat on-beat bank)
    // Groove Gamble — the rhythm risk/reward layer. Consecutive on-beat catches compound a live
    // GLOBAL score multiplier (beat_streak drives beat_gamble_mult); a single off-beat catch breaks
    // the run and resets it to 1x. It's a tension the player is actively managing: keep nailing the
    // beat and every point is worth more, but one greedy off-beat grab throws the whole heat away.
    pub(crate) beat_gamble_mult: f32, // current compounding multiplier from the on-beat streak (>= 1.0)
    pub(crate) beat_gamble_flash: f32, // green pulse when the multiplier steps up
    pub(crate) streak_lost_flash: f32, // red pulse + callout when an off-beat catch breaks a hot streak
    // Cash-out fork: pressing B banks the live streak. Banking ON the beat locks the whole
    // multiplier into a safe floor that an off-beat miss can no longer wipe; banking off-beat
    // takes a haircut. After a bank the live climb resets to the locked floor and keeps rising,
    // so the choice is "bank now and keep it safe" vs "push higher and risk the whole stack".
    pub(crate) beat_gamble_locked: f32, // safe multiplier floor secured by a cash-out (>= 1.0)
    pub(crate) gamble_bank_flash: f32,  // gold pulse when a cash-out banks the streak
    pub(crate) gamble_bank_pulse: f32,  // "BANK NOW?" prompt pulse while a bankable streak is live
    // Rising-edge tracking for the groove meter topping out. `groove_was_full` remembers whether
    // the meter was already maxed last frame, so the "POCKET LOCKED" spectacle fires exactly once
    // on the frame groove first reaches full — the watchable peak of rhythmic play, not a per-frame
    // repeat. `groove_full_flash` is the one-shot celebration timer it lights.
    pub(crate) groove_was_full: bool,
    pub(crate) groove_full_flash: f32,
    pub(crate) music_muted: bool, // Whether music playback is muted (M key toggle)
    pub(crate) music_layers: Vec<Source>,
    pub(crate) catch_radius_upgrade: f32,
    // On-beat catch bloom — a rhythm read on *ordinary catching*, not a discrete ability. Every
    // beat the train's catch radius blooms wider (widest on the downbeat) and settles back before the
    // next hit, so a crab drifting just out of reach gets scooped if you cross it ON the beat but
    // slips past between beats. The widening applies around every conga link (see catch_radius()), so
    // the whole train hoovers harder on the beat. Timing plain grabs to the bar becomes live herd
    // management, distinct from the Dash (movement), Call (Dancer lure), and whistle (radial pulse).
    // Set in the beat handler, decayed each frame in update_crabs, folded into catch_radius(), and
    // drawn as a ring at the head that flares on the beat and fades between beats.
    pub(crate) beat_catch_bloom: f32,
    // Cleave slash — a short-lived blade stroke drawn across the split point the instant a Splitter
    // cleaves the train, so the "cut" reads as an actual stroke bisecting the conga line rather than
    // just a shockwave. Endpoints are the last kept front link and the first banked back link (or the
    // splitter itself); the timer counts 1→0 and drives the slash's length/brightness. `cleave_gold`
    // gates the color so a Jackpot Cleave slashes gold, a plain cut slashes teal. Set in
    // on_splitter_cleave, decayed each frame, drawn in the world pass.
    pub(crate) cleave_flash: f32,
    pub(crate) cleave_a: Vec2,
    pub(crate) cleave_b: Vec2,
    pub(crate) cleave_gold: bool,
    // Upgrade lanes — level-ups deepen ONE of the four tools instead of handing out flat stat
    // bumps, so committing to a lane branches the run into a distinct playstyle (beam boss-hunter,
    // lasso chain-catcher, whistle crowd-control, stomp shell-breaker). Each rank scales the tool
    // and, at milestone ranks, changes how it behaves. Effective per-tool values are derived from
    // these ranks in the helper methods below rather than stored, so they stay in sync everywhere.
    pub(crate) beam_rank: u32,
    pub(crate) lasso_rank: u32,
    pub(crate) whistle_rank: u32,
    pub(crate) stomp_rank: u32,
    pub(crate) floating_texts: FloatingTextSystem,
    // Cosmetic parade of just-banked crabs filing into the delivery pen (see try_deliver_train).
    pub(crate) penned_marchers: PennedMarcherSystem,
    // Scratch buffer for PennedMarcherSystem::update() arrivals — reused every frame while a
    // parade is active instead of a fresh Vec allocation on each of those frames.
    pub(crate) marcher_arrivals_buf: Vec<(Vec2, [f32; 3])>,
    pub(crate) combo_count: usize,
    pub(crate) combo_timer: f32,
    pub(crate) textures: GameTextures, // Textures for grass, sand, and player
    pub(crate) level_textures: Vec<LevelTexture>, // Textures for each level
    // Beat Wave ability
    pub(crate) beat_count: u32, // Counts beats fired, every 4th triggers wave
    // Bar downbeat accent: the musical "1" of every 4-beat bar lands harder than the three
    // beats between it, so the rhythm reads as structured bars instead of a flat metronome.
    // Kicked to 1.0 on each `beat_count % 4 == 0` beat and decayed each frame; the beat-stepping
    // conga train amplifies its forward stomp while this is high, so the whole train visibly
    // "lands the one" together — a big unified footfall on the downbeat, smaller steps between.
    pub(crate) bar_accent: f32,
    // Drum Roll (hold T): the one player-driven rhythm verb that's a fresh VERB, not a passive
    // multiplier. Hold T across consecutive beats to build a charge; each beat that T is held
    // while on-beat counts as a "roll hit" and stacks. Release to FIRE a focused beam blast down
    // the flashlight's aim — a short window where the cone widens and reaches far, snapping every
    // free crab in that aimed arc into the train at once. It's directional (down your aim, unlike
    // the radial Slam), timing-gated (only pays if you land the beats), and costs no Groove meter,
    // so it's a skill move you perform rather than a meter you spend. Missing a beat while holding
    // resets the stack, so the tension is holding the roll clean through a full bar for the big pop.
    pub(crate) drum_roll_held: bool, // was T held last frame — edge-detects press/release in update
    pub(crate) drum_roll_hits: u32, // consecutive on-beat "roll hits" banked while holding (the charge)
    pub(crate) drum_roll_charge: f32, // 0..1 visual charge level, eased toward drum_roll_hits for a smooth telegraph
    pub(crate) drum_roll_fire: f32, // 1..0 timer while a fired blast's wide beam is live (drives the catch boost + glow)
    pub(crate) drum_roll_power: u32, // roll hits captured at the moment of firing — scales the fired blast's reach/arc
    pub(crate) beat_wave_active: bool, // Whether beat wave is expanding
    pub(crate) beat_wave_radius: f32, // Current radius of expanding wave
    // Bar-quantized spawns: when a pattern ends we don't drop the next wave at an arbitrary
    // instant — we arm it and let it land on the next downbeat (bar boundary), so every fresh
    // herd arrives locked to the music. `wave_armed` is set when the pattern timer lapses (or
    // the field's fully caught), and the beat handler fires the wave on the next `beat_count %
    // 4 == 0`. `wave_telegraph` counts up while armed so the draw layer can flash a "here it
    // comes" pulse in the bottom bar.
    pub(crate) wave_armed: bool,
    pub(crate) wave_telegraph: f32,
    // Staged difficulty spike: instead of a flat rising curve, every Nth cleared wave is a
    // "Frenzy" — a denser-than-normal drop with a gold telegraph, an extra downbeat punch, and a
    // banner, so the run has recurring standout moments that feel earned rather than a smooth ramp.
    // `waves_cleared` counts patterns cleared this run; `frenzy_wave` marks the currently-armed
    // drop as a frenzy so the telegraph and the spawn both know. `frenzy_banner_timer` drives the
    // "FRENZY!" flash when one lands.
    pub(crate) waves_cleared: u32,
    pub(crate) frenzy_wave: bool,
    pub(crate) frenzy_banner_timer: f32,
    // Staged difficulty ramp over elapsed time (the smooth rising spine of a run, orthogonal to
    // the every-4th Frenzy spike above). `intensity_stage` indexes INTENSITY_STAGES and only ever
    // climbs; crossing into a new stage fires `stage_banner_timer` with `stage_banner_name` set to
    // the stage's shout. Every spawned wave reads the current stage to scale its count/duration.
    pub(crate) intensity_stage: usize,
    pub(crate) stage_banner_timer: f32,
    pub(crate) stage_banner_name: &'static str,
    // Lasso Throw ability
    pub(crate) lasso_phase: LassoPhase, // Throw state machine (see LassoPhase)
    pub(crate) lasso_pos: Option<Vec2>, // Current lasso tip position (None = inactive)
    pub(crate) lasso_timer: f32,        // Time remaining in the CURRENT phase
    pub(crate) lasso_target: Vec2, // Aim point the loop flies toward (world space, set on release)
    pub(crate) lasso_origin: Vec2, // Player center captured at throw time (arc anchor)
    // Charge-throw fields: the player holds the mouse to wind up, releasing fires the throw.
    pub(crate) lasso_charge: f32, // 0..LASSO_MAX_CHARGE_TIME, grows while mouse is held
    pub(crate) lasso_mouse_down: bool, // True while left mouse button is held (winding)
    pub(crate) lasso_spin: f32,   // Accumulated rope spin angle in radians, for visual
    pub(crate) lasso_on_beat_bonus: f32, // 1.0 normally; LASSO_ONBEAT_BONUS if released on-beat
    // Crabs bitten by the current throw, mid-reel-in: (crab index, snag point, per-crab age seconds).
    // Driven each Dragging frame to yank the crab from where the rope bit it toward the train with
    // visible tension. Reused (drained, not reallocated) per throw.
    pub(crate) lasso_drag_buf: Vec<(usize, Vec2, f32)>,
    // Whistle ability — a sonic pulse that yanks nearby crabs toward the player. Soft-counters
    // skittish Sneaky crabs (strong pull) while heavy Big crabs barely budge (see CrabType::whistle_pull).
    pub(crate) whistle_active: f32, // >0 while the ring is expanding (seconds remaining)
    pub(crate) whistle_radius: f32, // current front radius of the expanding pulse
    pub(crate) whistle_cooldown: f32, // >0 while on cooldown; whistle unusable until it hits 0
    pub(crate) whistle_center: Vec2, // player center captured at cast time (ring origin)
    pub(crate) whistle_beat_bonus: f32, // 1.0 normally, >1 when this cast landed on-beat (bigger reach)
    // Stomp ability — a close-range ground-pound that CRACKS armored crab shells instantly (its
    // dedicated counter; the beam is the slow universal fallback) and shoves nearby free crabs in.
    pub(crate) stomp_active: f32, // >0 while the shockwave is expanding (seconds remaining)
    pub(crate) stomp_radius: f32, // current front radius of the shockwave
    pub(crate) stomp_cooldown: f32, // >0 while on cooldown; Stomp unusable until it hits 0
    pub(crate) stomp_center: Vec2, // player center captured at stomp time (ring origin)
    pub(crate) stomp_beat_bonus: f32, // 1.0 normally, >1 when this cast landed on-beat (bigger slam)
    // Call ability (F) — a rhythm-native summon aimed at Dancer crabs. An on-beat Call charms every
    // nearby Dancer into "answering": on the next beat they hop TOWARD the player instead of fleeing,
    // opening a catch window you actively play for. Off-beat it fizzles. This is the player's own
    // on-beat action the Dancer answers to, turning rhythm from something you watch into something
    // you play. Purely a control layer over existing Dancer hop logic — no new draw dependency.
    pub(crate) call_cooldown: f32, // >0 while on cooldown; Call unusable until it hits 0
    pub(crate) cycle_cooldown: f32, // >0 while on cooldown; Cycle (X) unusable until it hits 0
    pub(crate) call_pulse: f32, // 0..1 visual ring pulse, set to 1 on a successful on-beat Call, decays
    pub(crate) call_pulse_center: Vec2, // player center captured when the Call rang out
    // Groove Call (V) — a player-initiated, FIELD-WIDE beat-phrase lure. Distinct from every other
    // pull verb: the whistle is a local, instant radial yank; the Dancer Call (F) charms only nearby
    // Dancers; Groove Dash pulls with a movement input. This one is the Reef DJ's call-and-response
    // handed to the player — you CALL this bar, and the response UNFOLDS over the next couple bars:
    // EVERY free crab on the whole field visibly streams toward you, surging hardest right on each
    // downbeat and easing between beats, so the beat itself becomes a herd-routing tool across the
    // arena, not just around the player. Rhythm-quality-gated — an on-beat call pulls harder and
    // for more bars than a sloppy off-beat one, which barely answers. The unfolding stream over the
    // bar is the watchable moment no shipped verb produces.
    pub(crate) groove_call_cooldown: f32, // >0 while on cooldown; Groove Call unusable until it hits 0
    pub(crate) groove_call_bars: f32, // bars of "response" left — counts DOWN each downbeat while the herd streams in
    pub(crate) groove_call_strength: f32, // pull scale set at call time — bigger on a clean on-beat call
    pub(crate) groove_call_pulse: f32, // 0..1 visual ring pulse, re-kicked on each downbeat while active, decays
    pub(crate) groove_call_center: Vec2, // player center captured when the call rang out (visual ring origin)
    pub(crate) groove_call_surge: f32, // 1→0 per-downbeat surge envelope — the herd lunges on the beat, drifts between
    // Call-and-response ECHO: while a call is live, re-pressing V ON a beat "echoes" the phrase —
    // extending the response by a bar and ramping the pull. It's a skill layer on the SAME verb, not
    // a new button: a groove-savvy player keeps the herd streaming by answering the DJ every bar
    // (nail the phrase → the whole field piles in harder and longer; miss the beat → nothing, and the
    // call decays on its own). echo_count tracks the phrase length purely for the on-screen readout.
    pub(crate) groove_call_echo: u32, // echoes chained this call (0 = the opening call, no echoes yet)
    pub(crate) groove_call_echo_flash: f32, // 1→0 flash kicked on a clean echo so the answered beat reads
    // Downbeat Slam (G) — the rhythm ultimate. It only fires when the Groove meter is full AND the
    // press lands on the beat: a huge shockwave erupts from the player that yanks every free crab in
    // a wide radius straight into the conga train at once, then drains the whole meter. This is the
    // spectacle payoff for playing in the pocket — the groove meter finally *does* something instead
    // of only swelling the (currently silent) music. Off-beat or an empty meter fizzles with feedback
    // so mistiming reads clearly. The slam ring below is purely visual; the catch happens instantly.
    pub(crate) slam_active: f32, // >0 while the slam ring is expanding (seconds remaining)
    pub(crate) slam_radius: f32, // current front radius of the expanding slam ring
    pub(crate) slam_center: Vec2, // player center captured when the slam fired (ring origin)
    pub(crate) slam_flash: f32,  // 1..0 gold screen bloom on a successful slam
    // Dash effect
    pub(crate) dash_just_fired: bool,
    pub(crate) dash_flash: f32,
    // Groove Dash — an on-beat dash gathers nearby free crabs toward you as you punch through,
    // turning a well-timed movement into a routing tool. `groove_dash_timer` counts down while the
    // gather window is live; `groove_dash_center` is the player center captured at fire time so the
    // pull ring reads from where the dash started. Off-beat dashes leave this at zero (full escape,
    // no penalty); only on-beat dashes light it up, so the beat visibly reshapes the herd.
    pub(crate) groove_dash_timer: f32,
    pub(crate) groove_dash_center: Vec2,
    pub(crate) groove_dash_dir: Vec2,
    // Downbeat herd pulse — a PASSIVE, no-keypress routing tool: on every downbeat the whole free
    // herd gets a brief nudge toward the player, so the beat *itself* clumps loose crabs around you.
    // Distinct from Groove Dash (movement-triggered), the Dancer Call (F, nearby Dancers), and the
    // Groove Call (V, a full field-wide yank you fire): this is always-on, tiny, and rhythmic — a
    // groove-savvy player learns to stand where the next downbeat will sweep drifting crabs into
    // their beam. Set to 1.0 on each downbeat, decayed each frame in update_crabs; the impulse is a
    // gentle tug (a routing nudge, not a catch), applied only to free, non-fleeing crabs so it never
    // fights the flee/lure passes or becomes an autocatcher next to the on-beat catch bloom.
    pub(crate) downbeat_pull: f32,
    pub(crate) downbeat_pull_center: Vec2, // player center captured on the downbeat, for the visual clump ring
    // How big a herd the last downbeat actually swept — 0..1, normalized against a "full scoop" count.
    // Captured on the downbeat by counting free, non-fleeing crabs inside the pull radius, and used to
    // bloom the visual clump ring: a downbeat that hoovers a fat loose herd flares brighter and gold,
    // a downbeat over an empty field stays a faint thump. Makes the passive routing tool's *power* read
    // on screen so a groove-savvy player sees when standing in the herd on the "1" paid off.
    pub(crate) downbeat_pull_haul: f32,
    // Camera shake
    // Weather + day/night ambience. Routes only through catch_radius/screen_shake/particles —
    // no new mechanics, just mood. `weather_intensity` eases toward `weather_target.intensity()`
    // so states cross-fade; `weather_step_timer` gates the random-walk retarget cadence.
    pub(crate) weather_target: WeatherState,
    pub(crate) weather_intensity: f32, // 0..1, eased toward the target's intensity() each frame
    pub(crate) weather_step_timer: f32, // counts down to the next random-walk retarget
    pub(crate) lightning_flash: f32, // 1→0 storm flash: brightens screen, spikes catch radius, kicks shake
    pub(crate) lightning_timer: f32, // counts down to the next possible strike (only while Storm)
    pub(crate) day_phase_t: f32,     // 0..1 across a run: dawn→day→dusk→night
    pub(crate) screen_shake: f32,    // current shake magnitude (pixels), decays each frame
    pub(crate) screen_shake_vel: Vec2, // current shake offset velocity
    pub(crate) screen_shake_offset: Vec2, // current pixel offset applied to viewport
    pub(crate) hitstop_timer: f32,   // brief whole-sim freeze right after a catch (juice)
    pub(crate) slowmo_timer: f32, // 1..0 cinematic slow-motion ramp on the biggest climax moments
    // (boss catches, Downbeat Slam). Unlike hitstop's hard freeze, the
    // sim keeps running but time is dilated, easing back to full speed
    // as the timer decays — so a set-piece victory lands in bullet-time
    // instead of just snapping past.
    pub(crate) boss_hit_iframes: f32, // >0 briefly after a King Crab charge lands a DIRECT player hit (the full-train scatter). While it ticks the boss can't chain-charge you, giving a regroup window. Decays each frame.
    pub(crate) chain_join_ripple: bool, // set true when any crab is caught this frame
    pub(crate) chain_snap_cooldown: f32, // >0 briefly after a tail snaps, so one brush can't strip the whole train
    pub(crate) cached_tail_pos: Option<Vec2>, // position of the highest-chain_index caught crab, refreshed once per frame in update_crabs and reused by steal_chain_thief instead of a second O(n) scan
    pub(crate) cached_tail_type: Option<CrabType>, // archetype of that same tail crab, refreshed in the same snapshot pass. Drives the field "CATCH-NEXT" highlight: a free crab of this type would extend the tail_run_len match run, so it's lit as the arrangement-smart grab. Purely legibility.
    // CYCLE PREVIEW: the crab currently at chain_index == 1 — the one that WOULD become the new head
    // if the player cycled (rotation maps ci → (ci + n - 1) % n, so ci=1 lands at the head slot 0).
    // Cached in the same snapshot pass as cached_tail_type. The draw path rings this crab so the
    // player can SEE which figurehead a cycle would promote before pressing X, turning a blind mash
    // into an informed arrangement decision. Purely legibility — changes no odds. True when a train
    // of >= 2 links exists and the cycle verb is off cooldown (i.e. pressing X would do something).
    pub(crate) cycle_preview_active: bool,
    pub(crate) free_splitter_present: bool, // true when at least one uncaught Splitter is on the field; refreshed in update_crabs to avoid an O(n) scan in the draw path every frame
    pub(crate) tail_run_len: u32, // length of the current unbroken run of *same-type* links at the tail of the train. Every catch that matches the previous tail's type extends it, escalating a "match" bonus (see handle_crab_catching); a mismatched catch resets it to 1. This is what makes catch ORDER a live spatial decision: catch A-A-A-A and each same-type link pays more, catch A-B-A-B and it never builds. Reset to 0 on delivery, and recomputed from the new tail whenever a peel/snap strips links.
    pub(crate) next_milestone: usize, // Next train-length milestone to celebrate
    pub(crate) next_boss_score: usize, // score at which the next boss arrives
    pub(crate) next_boss_kind: usize, // cycles 0=King Crab, 1=Tide Boss, 2=Reef DJ so runs rotate through all three climax beats
    // Reef DJ call-and-response phrase. The rhythm boss doesn't open its shell on *every* beat —
    // it CALLS a short phrase: each bar it flashes a random subset of the four beats as "hot", and
    // its shell only drains while you hold the light on it during one of those called beats. Off
    // the phrase (both off-beat and on un-called beats) the light does nothing, so the fight is a
    // real echo-the-pattern duel instead of a hold-and-tap-the-beat one. `reef_phrase[i]` is true
    // when beat `i` of the current bar (beat_count % 4) is a called/hot beat.
    pub(crate) reef_phrase: [bool; 4],
    pub(crate) reef_phrase_bar: u32, // beat_count/4 of the bar the current phrase was rolled for, so we re-roll once per bar
    pub(crate) reef_active: bool, // true while a Reef DJ is on the field, gating the phrase HUD/telegraph
    // Reef DJ backup dancers: the fight otherwise silences the whole archetype web, so the DJ
    // summons its own "hype Dancers" into the arena as a fight mechanic. Catching one on a called
    // (hot) beat chips the boss shell — herd them onto the phrase to crack it faster than light
    // alone. This timer counts down while the DJ is on the field; on zero it spawns one and resets.
    pub(crate) reef_dancer_timer: f32,
    pub(crate) reef_hit_flash: f32, // 1..0 juice bloom kicked when the player lands a hot beat on the DJ's shell
    // Delivery pen — the "cash in the train" mechanic. Drive the conga line into the pen to bank
    // the whole train for a super-linear score payout (longer train = disproportionately more) and
    // reset the chain, closing the risk/reward loop the chain-snap risk opened. The pen relocates
    // each level so routing the train there stays a fresh decision.
    pub(crate) pen_pos: Vec2,      // center of the delivery pen on the field
    pub(crate) deliver_flash: f32, // 1..0 bloom timer after a successful bank (visual only)
    // Where the player (and so the train's head) stood at the instant of the last bank. The delivery
    // beam is drawn from here to the pen while deliver_flash decays, connecting where the conga line
    // departed to the vault it cashes into — the one connective beat the pen's own coin-spray/rings
    // don't cover, since those all erupt at the pen. Set in try_deliver_train, purely visual.
    pub(crate) deliver_beam_from: Vec2, // player center at the last bank (source of the delivery beam)
    pub(crate) deliver_beam_to: Vec2, // pen center the last bank cashed into (the pen relocates after a bank, so snapshot it)
    pub(crate) deliver_beam_perfect: bool, // whether the last bank was on-beat (beam runs gold vs. green)
    // Delivery streak — consecutive banks escalate a payout multiplier so cashing in repeatedly
    // (rather than hoarding one giant train) builds its own rising reward, and banking *on the
    // beat* stacks a "PERFECT DELIVERY" rhythm bonus on top. Closes the rhythm hook over the
    // game's single biggest payoff moment. `deliver_streak` counts banks in a row; it never
    // resets on its own (there's no fail state for banking) but a long dry spell decays it via
    // `deliver_streak_timer` so the multiplier reflects *recent* cashing tempo, not lifetime.
    pub(crate) deliver_streak: u32,
    pub(crate) deliver_streak_timer: f32, // seconds of grace left before an idle streak decays a notch
    // Tide pools — terrain that shapes where the train can go. Each pool is a patch of shallow
    // water (center, radius) that drags on movement: crossing one slows the player to a wade, and
    // because the whole conga tail replays the player's path, hauling a long train through open
    // water costs you real time and exposure. They relocate each level (like the pen) so routing
    // — skirt the pools or dash across them — stays a live, geography-driven decision.
    pub(crate) tide_pools: Vec<(Vec2, f32)>, // (center, radius) of each shallow-water drag zone
    pub(crate) in_tide_pool: bool, // whether the player is wading right now (for splash juice)
    // Kelp-snag telegraph: a 0..=1 tension that RISES while the conga tail sits in a kelp patch and
    // is long enough to snag, and eases back down once the tail routes clear. It drives a pulsing
    // green warning ring around the tail crab so an imminent snag is *seen coming* — the loss stops
    // feeling like a random tax and becomes a "route out NOW" decision the player has agency over.
    // Purely a legibility layer over the existing `snag_chain_on_kelp` roll; it changes no odds.
    pub(crate) kelp_snag_warn: f32,
    // Rocky Shore tide: on the Rock biome the sea rises and falls on the 4-beat bar cycle. Every
    // other native rock patch (see `rock_is_low` — the even-indexed ones) is a *low rock* that the
    // rising tide submerges: while covered it stops blocking and instead wade-drags like water, so a
    // chokepoint you can't cross at low tide opens into a shortcut you dash through on the beat. The
    // three-and-below rocks and odd-indexed high rocks never submerge, so there's always a solid
    // wall to thread — the tide reshapes the route, it doesn't erase it. `rock_tide_fill` is the
    // smoothed 0->1 water level (0 = fully ebbed/exposed, 1 = fully flooded/covered), eased toward
    // its target each frame so the sea visibly swells and drains rather than snapping; the crossover
    // where a low rock flips passable is `rock_tide_fill > ROCK_SUBMERGE_LEVEL`. Only meaningful on
    // the Rock biome; parked at 0 elsewhere so no other zone pays for it.
    pub(crate) rock_tide_fill: f32,
    // Arena-shifting boss enrage: when a boss crosses its enrage threshold it reshapes the space of
    // the duel. A King Crab CRACKS THE FLOOR into these fissures — (center, radius, age) hazard pits
    // that snap the conga tail if it lingers in one, so the finale is a routing gauntlet, not just a
    // faster charger. `age` counts up from 0 (crack tearing open) toward 1 (settled hazard). The
    // Tide Boss instead FLOODS the arena by appending extra drag pools to `tide_pools`; we remember
    // how many it added in `boss_flood_pools` so `on_boss_caught` can drain exactly those back off
    // without disturbing the level's own water. Both clear when the boss is caught.
    pub(crate) boss_fissures: Vec<(Vec2, f32, f32)>,
    // Beat-synced eruption pulse for the King Crab fissures: kicked to 1.0 on each beat while
    // fissures are open, then decays toward 0. On the peak the molten pits GEYSER — a spout bursts
    // up, the pit's glow flares, and its tail-snap radius briefly swells so the hazard breathes
    // with the music. Between beats the fissures settle and the widened bite recedes, so a fissure
    // is only fully dangerous *on the beat* — the player learns to thread the tail across in the
    // gaps, tying the arena-crack finale into the game's rhythm spine instead of being a static pit.
    pub(crate) boss_fissure_erupt: f32,
    pub(crate) boss_flood_pools: usize, // count of extra pools a Tide Boss flooded in on enrage
    pub(crate) chain_rings: Vec<(Vec2, f32, [f32; 3])>, // (pos, age 0..1, rgb) for beat ghost rings
    pub(crate) catch_shockwaves: Vec<(Vec2, f32, [f32; 3])>, // (pos, age 0..1, rgb) impact ring per catch
    // Queued beat-hit punch effects — (pos, rgb, beat_quality) — pushed during update when an
    // on-beat catch fires and drained in draw. Cleared at the top of each update tick.
    pub(crate) beat_punch_events: Vec<(Vec2, [f32; 3], f32)>,
    /// Same-type bond-forming flash: (tail_pos, new_crab_pos, rgb, age 0..1).
    /// Emitted when a catch links a same-type neighbor; drawn as a brief bright arc between the two.
    pub(crate) bond_flash_events: Vec<(Vec2, Vec2, [f32; 3], f32)>,
    // A bright whip-streak that arcs from where a crab was caught to the head of the train, so a
    // catch reads as the crab being *yanked* in rather than just blinking onto the tail. Each entry
    // is (from, to, age 0..1, rgb); brighter/thicker when the catch landed on the beat.
    pub(crate) catch_trails: Vec<(Vec2, Vec2, f32, [f32; 3])>,
    // Groove-Call answer streaks — comet trails from free crabs toward the player, spawned on each
    // beat while a call is live so the whole herd visibly *streams in on the beat*. Kept in its own
    // capped Vec (drawn with draw_catch_trails) so it never starves real catch-snap trails. Same
    // (from, to, age, color) tuple as catch_trails; ages out on its own decay pass.
    pub(crate) call_streaks: Vec<(Vec2, Vec2, f32, [f32; 3])>,
    pub(crate) fear_rings: Vec<(Vec2, f32)>, // (pos, age 0..1) cold alarm ring where a catch startled the herd
    // Tide Boss shockwave pulses — (center, current radius) of each expanding front. Grows to
    // TIDE_PULSE_RADIUS then fades out. Bounded by the one-boss-at-a-time cap plus a hard len guard.
    pub(crate) tide_pulses: Vec<(Vec2, f32)>,
    pub(crate) zoom_punch: f32, // camera zoom-in kick on catch, springs back to 0 (juice)
    pub(crate) fullscreen_applied: bool, // deferred until the first update tick, see update()
    // Scratch buffers for catch_by_chain, reused every frame instead of being freshly
    // allocated each call. The play area is fixed-size so the grid's cell count (and thus
    // its Vec<usize> bucket count) stabilizes quickly — clearing beats rebuilding from scratch.
    pub(crate) chain_positions_buf: Vec<Vec2>,
    pub(crate) catch_grid_buf: std::collections::HashMap<(i32, i32), Vec<usize>>,
    // Tracks which cells were written to in catch_grid_buf this frame so the next frame's
    // "clear" can call .clear() on each touched Vec rather than dropping it via HashMap::clear().
    // Reusing the inner Vecs avoids ~40-50 small heap allocations per frame (one per crab-occupied
    // cell), since crabs move slowly and typically revisit the same cells frame-to-frame.
    pub(crate) catch_grid_keys_buf: Vec<(i32, i32)>,
    pub(crate) caught_now_buf: Vec<bool>,
    // Reused buffer of solid conga-body segment positions, rebuilt each frame for the
    // fleeing-crab wall-deflection pass (see deflect_fleeing_off_chain).
    pub(crate) deflect_body_buf: Vec<Vec2>,
    // Spatial grid over deflect_body_buf (same idea as catch_grid_buf below) so each fleeing
    // crab only tests nearby body segments instead of the whole chain — chain length has no
    // cap, so a linear scan there gets slower the longer a session runs.
    pub(crate) deflect_grid_buf: std::collections::HashMap<(i32, i32), Vec<usize>>,
    // Reused scratch buffer for bounce-ring spawn positions collected during the deflection
    // pass, avoiding a fresh Vec allocation every frame.
    pub(crate) deflect_bounce_buf: Vec<Vec2>,
    // Emergent pile-up: crabs the wall just deflected get funneled into the train's concave
    // pockets, where they collide with *each other*. This pass ricochets colliding fleeing crabs
    // apart and cross-startles them, so herding a panicking crowd into the conga wall sets off a
    // pinball cascade. deflect_ricochet_buf holds the (index, pos) of crabs deflected this frame;
    // deflect_ricochet_grid_buf buckets them so each only tests nearby neighbors, not all of them.
    pub(crate) deflect_ricochet_buf: Vec<(usize, Vec2)>,
    pub(crate) deflect_ricochet_grid_buf: std::collections::HashMap<(i32, i32), Vec<usize>>,
    // Cold ring positions where two deflected crabs cracked into each other, spawned after the
    // ricochet pass so the collision reads without a per-frame allocation.
    pub(crate) deflect_collide_buf: Vec<Vec2>,
    // Resolved (crab index, new pos, new vel) from the ricochet pass, staged here so we apply
    // them after scanning (no double mutable borrow) without allocating a fresh Vec each frame.
    pub(crate) deflect_resolve_buf: Vec<(usize, Vec2, Vec2)>,
    // Event-collection scratch buffers for update_crabs, reused every frame instead of being
    // freshly allocated on each call. Most frames produce zero events in each of these (no
    // crab started fleeing, no boss broke, etc.), so a per-frame Vec::new() was pure churn —
    // clearing a buffer that's almost always empty costs nothing, while allocating one does.
    pub(crate) flee_pops_buf: Vec<Vec2>,
    // Positions of crabs a catch-shock startle just spooked this frame (see emit_catch_startle),
    // reused across calls instead of a fresh Vec::new() every single catch.
    pub(crate) startled_pops_buf: Vec<Vec2>,
    // Positions where a Magnet's field just snared a fleeing Golden this frame (first-snare only),
    // so the "SNARED!" pop and shockwave fire once rather than every frame the tether holds.
    pub(crate) golden_snare_pops_buf: Vec<Vec2>,
    // Positions where a Magnet's field just intercepted a homing Thief this frame (first-catch
    // only), so the "INTERCEPTED!" pop and shockwave fire once rather than every frame it's held.
    pub(crate) thief_snare_pops_buf: Vec<Vec2>,
    // Positions where a roaming Magnet first began chasing a nearby fleeing Golden this frame
    // (first-lure only), so the "LURED!" pop fires once rather than every frame the chase holds.
    pub(crate) magnet_lure_pops_buf: Vec<Vec2>,
    // Positions where a homing Thief first got diverted off your tail by a nearby fleeing Golden
    // this frame (first-divert only), so the "SHINY!" pop fires once rather than every frame it holds.
    pub(crate) thief_lure_pops_buf: Vec<Vec2>,
    pub(crate) boss_broke_buf: Vec<Vec2>,
    pub(crate) armor_broke_buf: Vec<Vec2>,
    pub(crate) attraction_particles_buf: Vec<(Vec2, Vec2, f32, [f32; 3])>,
    pub(crate) boss_windups_buf: Vec<Vec2>,
    pub(crate) boss_launches_buf: Vec<Vec2>,
    pub(crate) boss_charge_dust_buf: Vec<(Vec2, Vec2)>,
    // A boss just crossed into its enrage phase this frame — (pos, is_tide) so the callout/burst
    // can color itself. Almost always empty; reused like the other event buffers.
    pub(crate) boss_enrages_buf: Vec<(Vec2, bool)>,
    pub(crate) tide_fires_buf: Vec<Vec2>,
    pub(crate) tide_swells_buf: Vec<Vec2>,
    // Free Magnet-crab positions each frame, reused instead of reallocating — drives the
    // magnet-pull pass in update_crabs (ordinary crabs drift toward the nearest one).
    pub(crate) magnet_positions_buf: Vec<Vec2>,
    // Free-roaming Golden positions each frame, reused instead of reallocating — drives the
    // Golden-lures-Magnet pass in update_crabs (a roaming Magnet drifts toward the nearest one).
    pub(crate) golden_lure_positions_buf: Vec<Vec2>,
    // Positions of "charged" Magnets each frame — a Magnet currently pinning a snared Golden deep
    // in its field. Reused instead of reallocating. Drives the Golden-supercharges-Magnet crossover
    // in update_crabs: the shine energizes the lodestone so it vacuums the surrounding herd in
    // harder while it holds the prize (see the charged-radius branch of the magnet-pull pass).
    pub(crate) charged_magnet_positions_buf: Vec<Vec2>,
    // Free Armored crab positions each frame, reused instead of reallocating — drives the
    // Armored-body-blocks-King-Crab-charge crossover (a shell in the lunge's lane stops it cold).
    pub(crate) armored_positions_buf: Vec<Vec2>,
    // (boss_pos, shell_pos) for each King Crab charge blocked by an Armored shell this frame, so
    // the shell-clang feedback and shell knockback fire after the &mut self.crabs loop ends.
    pub(crate) boss_blocks_buf: Vec<(Vec2, Vec2)>,
    // King Crab positions stunned by ramming a parked Armored shell this frame, reused instead of
    // reallocating — mirrors boss_blocks_buf above (almost always empty).
    pub(crate) boss_stuns_buf: Vec<Vec2>,
    // Positions of fleeing/amplified Golden panic sources each frame, reused instead of
    // reallocating — drives the Golden-panic-spooks-Thief crossover in steal_chain_thief. Almost
    // always empty (a Golden mid-flee is rare), so this used to be a wasted per-frame Vec
    // allocation before it was pooled like the buffers above.
    pub(crate) golden_panic_positions_buf: Vec<Vec2>,
    // Event buffers for steal_chain_thief's three latched-Thief saves (Magnet pry, Golden panic
    // spook, Golden lure), reused instead of reallocating three fresh Vecs every single frame —
    // this function runs unconditionally whenever the train is long enough to be raidable, so an
    // unpooled Vec::new() here paid an allocation every frame even though a save firing on any
    // given frame is rare. Same pattern as the other event buffers on this struct.
    pub(crate) pried_by_magnet_buf: Vec<Vec2>,
    pub(crate) spooked_by_golden_buf: Vec<Vec2>,
    pub(crate) lured_by_golden_buf: Vec<Vec2>,
    // Landing spots of fleeing Dancers each beat, reused instead of reallocating — drives the
    // per-beat Dancer-hop startle ripple (see the beat block in update).
    pub(crate) dancer_hop_scratch: Vec<Vec2>,
    // Scratch buffers for beat_startle_contagion, mirroring the catch_by_chain/
    // deflect_fleeing_off_chain grid pattern: carriers are bucketed into a spatial grid so each
    // calm crab only tests nearby carriers instead of every panicking crab in the herd.
    // Each carrier is (pos, panic amplitude): a fleeing Golden crab carries an amplified fear
    // that ripples through the herd harder than an ordinary panicking crab (see below).
    pub(crate) contagion_carriers_buf: Vec<(Vec2, f32)>,
    pub(crate) contagion_grid_buf: std::collections::HashMap<(i32, i32), Vec<usize>>,
    // Emergent crossover: free Armored crabs act as calm anchors that shelter the herd from the
    // panic ripple. Their positions are snapshotted each beat into this reused buffer so the
    // contagion pass can spare any calm crab sheltering in an Armored shell's shadow from
    // infection — see beat_startle_contagion.
    pub(crate) armored_anchors_buf: Vec<Vec2>,
    // Spatial grid over armored_anchors_buf (same pattern as contagion_grid_buf) so the shelter
    // check only tests nearby anchors instead of every free Armored crab in the herd — without
    // this a session salted with several Armored crabs turned the per-crab shelter check into a
    // flat scan multiplied across every calm crab evaluated that beat.
    pub(crate) armored_anchor_grid_buf: std::collections::HashMap<(i32, i32), Vec<usize>>,
    // (pos, amplified?) — amplified pops came from a Golden's panic bomb and get a hot golden
    // "!" so the player sees the shiny prize detonating the herd, not just an ordinary scare.
    pub(crate) contagion_pops_buf: Vec<(Vec2, bool)>,
    // Same grid treatment for the Dancer-hop startle ripple (see the beat block in update) —
    // dancer_hop_scratch above supplies the fear sources, this buckets them for a fast lookup.
    pub(crate) dancer_startle_grid_buf: std::collections::HashMap<(i32, i32), Vec<usize>>,
    pub(crate) dancer_spooked_buf: Vec<Vec2>,
    // Scratch buffers for the Dancer-jolts-Thief and Dancer-trips-Golden crossovers below, reused
    // instead of a fresh Vec::new() every beat. Both also reuse dancer_startle_grid_buf (built just
    // above from the same dancer_hops) instead of linear-scanning every hop per crab, so a herd
    // salted with several fleeing Dancers doesn't turn this into a flat per-crab-per-hop scan.
    pub(crate) dancer_jolt_buf: Vec<Vec2>,
    pub(crate) dancer_trip_buf: Vec<Vec2>,
    // Scratch buffer for the Dancer-chips-Armored-shell crossover below, same reuse pattern as the
    // two above — (crab_pos, cracked_clean) so the after-loop feedback can tell a chip apart from a
    // full shatter. Also reuses dancer_startle_grid_buf, so a herd of Dancers by an Armored crab
    // doesn't turn this into a per-crab-per-hop scan.
    pub(crate) dancer_chip_buf: Vec<(Vec2, bool, bool)>, // (pos, cracked-fully, was-hermit)
    // Scratch buffer for the Dancer-jolts-Magnet crossover below, same reuse pattern as the ones
    // above — holds the positions of free Magnets a Dancer's on-beat hop thumped into a pull surge
    // this beat, for the after-loop feedback pop. Reuses dancer_startle_grid_buf like its siblings.
    pub(crate) dancer_kick_buf: Vec<Vec2>,
    // Scratch buffers for the Dancer-link on-beat catch aura (in the beat handler): dancer_link_buf
    // snapshots where the caught Dancer links sit this beat, dancer_aura_caught_buf collects the
    // free crabs each pulse snagged plus whether each was a Golden (for its bonus payout). Same
    // std::mem::take / clear / hand-back reuse as the sibling Dancer buffers — the beat handler runs
    // every beat, so a fresh Vec each time would be a per-beat allocation for a usually-empty scan.
    pub(crate) dancer_link_buf: Vec<Vec2>,
    pub(crate) dancer_aura_caught_buf: Vec<(Vec2, bool)>,
    // Scratch buffers for the Whistle/Stomp/Lasso ability loops in update(), reused every frame
    // instead of a fresh Vec::new() each tick these abilities are active. Each ability is active
    // for a fraction of a second to a couple seconds per use, so without reuse this was a
    // per-frame allocation for the whole duration of every whistle/stomp/lasso.
    pub(crate) whistle_soothed_buf: Vec<Vec2>,
    // Strong-match hit positions collected each frame for archetype-tool visual feedback.
    // Cleared at the start of update() and read (immutably) in draw_game().
    pub(crate) beam_hermit_hits_buf: Vec<(Vec2, f32)>,
    pub(crate) stomp_dancer_hits_buf: Vec<Vec2>,
    pub(crate) stomp_armored_hits_buf: Vec<Vec2>,
    pub(crate) whistle_golden_hits_buf: Vec<Vec2>,
    pub(crate) whistle_dancer_hits_buf: Vec<Vec2>,
    pub(crate) lasso_thief_hits_buf: Vec<Vec2>,
    pub(crate) lasso_magnet_hits_buf: Vec<Vec2>,
    // Positions where a lasso throw landed on a still-shelled crab (Armored / shelled Hermit) and
    // the loop slipped straight off — a WRONG-TOOL tell. Mirrors the beam/Hermit amber "can't-crack"
    // cue: teaches "crack the shell first (Stomp), then lasso" instead of failing silently.
    pub(crate) lasso_shell_deflect_hits_buf: Vec<Vec2>,
    pub(crate) magnet_cluster_hits_buf: Vec<Vec2>,
    pub(crate) stomp_cracked_buf: Vec<Vec2>,
    // Positions where a shelled Hermit was cracked open THIS frame, from any of its three intended
    // ecosystem verbs (Stomp / Dancer hop / charged Magnet rip). Collected inside the &mut crabs
    // loops (where `crab.is_hermit()` is known) and drained in the after-loop into the signature
    // Hermit-pop moment — a coppery shell-shard scatter distinct from a plain Armored crack, so the
    // archetype-web play that produced it reads as the watchable emergent win it's designed to be.
    pub(crate) hermit_popped_buf: Vec<Vec2>,
    pub(crate) lasso_catch_buf: Vec<usize>,
    pub(crate) lasso_startle_buf: Vec<Vec2>,
    // On-beat Thief-shake catches collected during the whistle/stomp loops (see
    // snatch_thief_on_beat) — almost always empty (at most one latched Thief at a time), but
    // these loops run every frame the ability is active, so reuse instead of a fresh Vec::new().
    pub(crate) whistle_thief_snatch_buf: Vec<(usize, Vec2)>,
    pub(crate) stomp_thief_snatch_buf: Vec<(usize, Vec2)>,
    // Event-collection scratch buffers for handle_crab_catching, reused every frame instead of
    // three fresh Vec::new() calls per tick. The vast majority of frames catch zero crabs (no
    // startle origin, no boss catch, no dance catch), so this was pure per-frame allocation
    // churn on the hottest possible path (runs unconditionally in update() every tick).
    pub(crate) startle_origins_buf: Vec<Vec2>,
    pub(crate) boss_catches_buf: Vec<(Vec2, bool)>,
    pub(crate) dance_catches_buf: Vec<Vec2>,
    // Golden crabs snapped up this frame — (pos, its base catch points) so the big lump-sum bonus
    // is paid out after the catch loop (needs &mut self for particles/floating text/score).
    pub(crate) golden_catches_buf: Vec<(Vec2, usize)>,
    // Positions where a Golden was caught directly behind a Magnet link this frame — the
    // "shine conducts down the train" crossover. Deferred out of the &mut self.crabs catch loop
    // so the cascade payout (bonus + whip-streak cascade + callout) can borrow &mut self after.
    // Almost always empty (needs the player to have engineered a Magnet-then-Golden catch order),
    // so pooling it keeps the hottest path allocation-free. See handle_crab_catching.
    pub(crate) magnet_shine_catches_buf: Vec<Vec2>,
    // Same-type "match run" catch events this frame: (pos, run_len, type_color). Fires when a
    // freshly-caught crab is the same archetype as the link it snapped onto, extending a run of
    // matching neighbors at the tail. Deferred out of the &mut self.crabs catch loop so the
    // escalating-bonus payout + callout can borrow &mut self after. Pooled like its sibling
    // catch-event buffers — usually empty, since it needs the player to deliberately catch two of
    // the same archetype back to back. See handle_crab_catching.
    pub(crate) match_run_catches_buf: Vec<(Vec2, u32, [f32; 3])>,
    // Reef DJ hype-Dancer catches this frame (see handle_crab_catching) — pooled like its sibling
    // catch-event buffers above instead of a fresh Vec::new() every frame; almost always empty
    // (needs a Reef DJ fight in progress plus a hot-beat catch), same reasoning as golden_catches_buf.
    pub(crate) hype_dancer_hits_buf: Vec<Vec2>,
    // Emergent crossover scratch: free Armored crabs whose shell a charged Magnet's widened vacuum
    // ground down this frame — (pos, whether that grind fully cracked the shell open) so the
    // chip/crack feedback fires after the per-crab borrow ends. Almost always empty (needs a
    // charged Magnet — itself rare, born of a snared Golden or a Dancer thump — plus an Armored
    // crab caught in its outer field), so a reused scratch Vec keeps it allocation-free.
    pub(crate) magnet_grind_buf: Vec<(Vec2, bool, bool)>, // (pos, cracked-fully, was-hermit)
    // Scratch buffers for tide_pulse_burst, reused across pulse calls instead of being freshly
    // allocated each time a Tide Boss fires. The `pulse_scattered_buf` is the most impactful:
    // it grows with herd size (one entry per free crab inside the blast radius) and the pulse
    // fires repeatedly — every ~5s baseline, faster when the boss enrages — in exactly the moments
    // when screen-shake + fireworks + particles are all firing together. The others are smaller
    // (at most a handful of Magnets/Goldens/snapped links) but each a genuine Vec::new per pulse.
    pub(crate) pulse_slingshots_buf: Vec<(Vec2, Vec2)>,
    pub(crate) pulse_loaded_magnets_buf: Vec<Vec2>,
    pub(crate) pulse_anchor_positions_buf: Vec<Vec2>,
    pub(crate) pulse_scattered_buf: Vec<Vec2>,
    pub(crate) pulse_snapped_positions_buf: Vec<Vec2>,
    // King Crab splice mechanic: crabs stolen by crossing through the conga line. Each entry is
    // (world_pos, magnet_timer) — the crab visually flies toward the boss via magnetic pull until
    // the timer expires, at which point it joins the boss as a visual follower.
    // (pos, timer, color) — pos is current world position, timer counts down from 1.0→0.0,
    // color is the crab's original body color for continuity.
    pub(crate) king_stolen_crabs: Vec<(Vec2, f32, [f32; 4])>,
    // Cooldown so the splice can't fire every frame as the boss lingers on a segment.
    pub(crate) king_splice_cooldown: f32,
    // Ambient NPC conga trains: three King Crabs each leading followers that wander the world.
    pub(crate) npc_trains: Vec<NpcCongaTrain>,
    // Lightweight perf instrumentation (debug builds only): accumulate frame times and print an
    // average + worst-case every couple seconds so future optimization passes have real numbers
    // instead of guessing from code inspection alone.
    #[cfg(debug_assertions)]
    pub(crate) perf_frame_count: u32,
    #[cfg(debug_assertions)]
    pub(crate) perf_time_accum: f32,
    #[cfg(debug_assertions)]
    pub(crate) perf_worst_frame: f32,
    // Last computed avg/worst frame time (ms), so the on-screen overlay always has a number to
    // show instead of blanking between the ~2s print windows above. Updated alongside the
    // println! so both stay in lockstep; drawn every frame but only rebuilt on that same cadence.
    #[cfg(debug_assertions)]
    pub(crate) perf_last_avg_ms: f32,
    #[cfg(debug_assertions)]
    pub(crate) perf_last_worst_ms: f32,
    #[cfg(debug_assertions)]
    pub(crate) perf_last_fps: f32,

    // Bot playtest harness: scripted inputs + time acceleration.
    pub(crate) bot: Option<BotState>,
    pub(crate) time_scale: f32,
}

impl MainState {
    pub fn new(ctx: &mut Context) -> GameResult<MainState> {
        let width = 1280.0;
        let height = 960.0;
        // The playfield is larger than the viewport so rival conga trains (roadmap thesis) have
        // room to approach from off-screen and the player has somewhere to route to. The camera
        // follows the player across it. 2x each dimension for now — density is a separate tuning pass.
        let world_width = width * 2.0;
        let world_height = height * 2.0;

        // Player starts in the center of the WORLD always.
        let player_pos = Vec2::new(
            world_width / 2.0 - PLAYER_SIZE / 2.0,
            world_height / 2.0 - PLAYER_SIZE / 2.0,
        );

        // Detect the actual BPM of action.ogg FIRST, so the beat grid AND the
        // procedurally-generated action groove are both built at the same tempo.
        // The groove is synthesised from this value below — a hardcoded groove BPM
        // would loop against the visual beats and beat-synced mechanics.
        let detected_beat_interval: f32 = {
            use std::io::Read as _;
            let mut bytes = Vec::new();
            let result = ggez::filesystem::open(ctx, "/action.ogg")
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
        let action_bpm = 60.0 / detected_beat_interval;

        // TODO Load all sound effects.
        let (king_crab_l, king_crab_r, king_crab_soft) = sounds::synth_king_crab_spatial(ctx)?;
        let (king_crab_rumble_l, king_crab_rumble_r) =
            sounds::synth_king_crab_ambient_spatial(ctx)?;
        let sounds = GameSounds {
            intro_music: Source::new(ctx, "/intro.ogg")?,
            // Procedurally generated action groove — a driving pentatonic shuffle
            // with a generative riff, swing, call-and-response phrasing, and a
            // layered bass line (see sounds::synth_action_groove). Replaces the
            // static /action.ogg so the in-game loop is real, foot-tapping music
            // rather than a fixed backing track.
            action_music: sounds::synth_action_groove(ctx, action_bpm)?,
            outro_music: Source::new(ctx, "/outro.ogg")?,
            upgrade: Source::new(ctx, "/upgrade.ogg")?,
            success: Source::new(ctx, "/success.ogg")?,
            success2: Source::new(ctx, "/success2.ogg")?,
            king_crab_rumble_l,
            king_crab_rumble_r,
            hihat: sounds::synth_hihat(ctx)?,
            flashlight_toggle: sounds::synth_flashlight_toggle(ctx)?,
            coin_chime: sounds::synth_coin_chime(ctx)?,
            world_map_pad: sounds::synth_ambient_pad(ctx, sounds::PadPreset::WarmPad, 220.0, 2.0)?,
            whistle_sfx: sounds::synth_whistle(ctx)?,
            stomp_sfx: sounds::synth_stomp(ctx)?,
            lasso_sfx: sounds::synth_lasso_throw(ctx)?,
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
            world_width,
            world_height,
            player_pos + Vec2::splat(PLAYER_SIZE / 2.0),
            &mut rand::rng(),
        );
        let init_tide_pools = pick_tide_pools(
            world_width,
            world_height,
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
            ggez::graphics::ImageFormat::Rgba8UnormSrgb,
            width as u32,
            height as u32,
            1,
        );

        // Use logical size (1280x960) for the offscreen render target, consistent with the viewport.
        // The postprocess pass will handle any HiDPI scaling when blitting to screen.
        let scene_image = ggez::graphics::Image::new_canvas_image(
            ctx,
            ggez::graphics::ImageFormat::Rgba8UnormSrgb,
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
        };
        let postprocess_params = ShaderParamsBuilder::new(&initial_pp_uniform).build(ctx);

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
            show_how_to_play_text: false,
            player_skin,
            player_name,
            skin_slot: 0,
            menu_page: 0,
            menu_selection: 0,
            world_map: None,
            show_world_map: false,
            in_campaign: false,
            tutorial: None,
            last_dir: Vec2::ZERO,
            shake_timer: 0.0,
            time_since_catch: 0.0,
            boost_timer: 0.0,
            boost_cooldown: 0.0,
            sprint_stamina: SPRINT_STAMINA_MAX,
            levels,
            current_level: 0,
            current_pattern: 0,
            pattern_timer: 0.0,
            debug_mode: true,
            pending_upgrade: false,
            offered_upgrades: [0, 1, 2],
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
            particle_system: ParticleSystem::new(),
            level_title: String::new(),
            level_title_timer: 0.0,
            textures,
            level_textures,
            subtitle,
            position_history,
            chain_count: 0,
            total_caught: 0,
            beat_timer: detected_beat_interval,
            beat_interval: detected_beat_interval,
            beat_intensity: 0.0,
            music_intensity: 0.0,
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
            stomp_dancer_hits_buf: Vec::new(),
            stomp_armored_hits_buf: Vec::new(),
            whistle_golden_hits_buf: Vec::new(),
            whistle_dancer_hits_buf: Vec::new(),
            lasso_thief_hits_buf: Vec::new(),
            lasso_magnet_hits_buf: Vec::new(),
            lasso_shell_deflect_hits_buf: Vec::new(),
            magnet_cluster_hits_buf: Vec::new(),
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
        })
    }
}
