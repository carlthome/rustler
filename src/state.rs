use std::collections::VecDeque;

use crevice::std140::AsStd140;
use ggez::audio::SoundSource;
use ggez::audio::Source;
use ggez::glam::Vec2;
use ggez::graphics::{Image, ShaderParams};
use ggez::Context;

#[derive(Copy, Clone, Debug, AsStd140)]
pub struct PostProcessUniform {
    pub groove: f32,
    pub time: f32,
    pub screen_width: f32,
    pub screen_height: f32,
    /// 0 = normal, 1 = full desaturate/title-card effect
    pub title_card_t: f32,
}

/// Uniform for the conga trail / echo-afterimage accumulation shader (`trail.wgsl`).
/// `strength` folds the per-frame feedback decay together with a groove curve on the CPU:
/// 0 at low groove (crisp normal play), rising toward ~0.86 at max groove.
// crevice's AsStd140 derive auto-pads the struct up to a 16-byte (vec4) boundary, so the
// single f32 maps to a full vec4 slot — matching `trail.wgsl`'s TrailUniform padding fields.
#[derive(Copy, Clone, Debug, AsStd140)]
pub struct TrailUniform {
    pub strength: f32,
}

use crate::bot::BotState;
// Re-exported so existing `use crate::state::*` consumers keep resolving these after the
// NpcCongaTrain cluster moved to its own module.
pub use crate::npc_conga_train::{NpcCongaTrain, gen_king_crab_name};
use crate::enemies::{CrabType, EnemyCrab};
use crate::graphics::{FloatingTextSystem, ParticleSystem, PennedMarcherSystem};
use crate::levels::Level;
use crate::skins::PlayerSkin;
use crate::sounds;
use crate::tutorial::Tutorial;
use crate::world_map::WorldMap;

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
    /// Descending sting played when a rival train rustles crabs off your tail — the "loss" half of
    /// the core steal moment (paired with `steal_gain_sfx`), so losing crabs reads audibly.
    pub(crate) steal_loss_sfx: Source,
    /// Rising sting played when you rustle crabs back off a rival — the triumphant "gain" half.
    pub(crate) steal_gain_sfx: Source,
    /// Hard-left / hard-right variants of the neutral rival-vs-rival theft clack — a third-party
    /// steal out on the field. The audio pass sets their per-play volumes (equal-power pan by the
    /// collision's bearing, faded by distance) and `play_detached`es both, so a far-off rival steal
    /// reads as a faint directional tick the player looks toward and swoops into (agar.io "radar").
    pub(crate) rival_steal_l: Source,
    pub(crate) rival_steal_r: Source,
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
    /// Per-rival spatial MUSIC — one beat-locked melodic motif per ambient NPC King Crab train,
    /// indexed by train (0 scout / 1 wanderer / 2 elder). Each entry is a hard-left / hard-right
    /// pair like `king_crab_rumble_*`; the audio pass equal-power pans it by the leader's bearing
    /// and scales its volume by distance AND the train's length/tier, so a big rival train
    /// broadcasts a louder, fuller motif from across the field (INSPIRATION.md: "the dominant train
    /// dominates the mix"). Layered on top of the creature rumble — the melodic half of the radar.
    pub(crate) king_crab_motif: Vec<(Source, Source)>,
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
    let _ = sounds.coin_chime.play();
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

pub struct MainState {
    pub(crate) player_pos: Vec2,              // Player position
    pub(crate) player_vel: Vec2,              // Player velocity (for smooth movement)
    pub(crate) mouse_pos: Vec2,               // Mouse position for flashlight aiming
    pub(crate) crabs: Vec<EnemyCrab>,         // List of crabs in the game
    pub(crate) score: usize,                  // Current score
    pub(crate) spawn_timer: f32,              // Timer for spawning new crabs
    /// A rare pirate chest waiting to be collected. Its groove reward is graded at pickup time.
    pub(crate) treasure_chest: Option<Vec2>,
    /// Counts down to the next rare-chest spawn roll while no chest is on the field.
    pub(crate) treasure_chest_timer: f32,
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
    // --- Campaign win-condition tracking (see `Level::win_condition`). All per-run counters,
    // reset in reset_game_at. `banked_crabs_run` counts crabs delivered to the pen this run;
    // `shells_cracked_run` counts Armored/Hermit shells fully cracked by ANY verb (stomp, dancer
    // hop, beam wear-down, magnet grind); `hold_train_timer` accumulates seconds the train has
    // continuously been at/above a HoldTrain target (reset the instant it dips below).
    pub(crate) banked_crabs_run: usize,
    pub(crate) shells_cracked_run: usize,
    pub(crate) hold_train_timer: f32,
    // Latch + celebration countdown once the level goal is met: the win fires exactly once, a
    // short "LEVEL COMPLETE!" beat plays out, then the run returns to the world map (which marks
    // the node complete and unlocks the next).
    pub(crate) level_complete: bool,
    pub(crate) level_complete_timer: f32,
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
    pub(crate) trail_shader: ggez::graphics::Shader, // Conga trail / echo-afterimage accumulation shader
    pub(crate) trail_params: ShaderParams<TrailUniform>, // Params for the trail shader
    pub(crate) trail_image_a: ggez::graphics::Image, // Ping-pong accumulation target A
    pub(crate) trail_image_b: ggez::graphics::Image, // Ping-pong accumulation target B
    pub(crate) trail_swap: bool, // Toggles which trail image is read vs written each frame
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
    /// Monotonic count of SPACE beat-tap tool chords fired this run (#165) — a SPACE tap while a tool
    /// key (E/R/Q) is held fires that tool on the beat-tap instead of dashing. Never drops, so the
    /// `groove_dash` playtest can assert the chord input path fired without racing any live counter.
    pub(crate) chord_tools_fired: usize,
    /// Monotonic count of crabs a rival NPC King Crab train has spliced away from the player this run
    /// (the reverse-Snake steal). Like `total_caught` it never drops, so the bot playtests can assert
    /// "the steal path fired" without racing the live chain count, which the steal itself lowers.
    pub(crate) crabs_stolen_by_npc: usize,
    /// Largest number of crabs a rival stole in a *single* splice this run. The steal is capped to a
    /// recoverable bite (`STEAL_MAX_LINKS`, and never more than half the chain — see
    /// `update_npc_trains`), so this stays low; the `npc_steal` bot test asserts it never exceeds the
    /// cap, guarding the "fun, not punishing" tuning against silent regression. Never drops.
    pub(crate) max_single_steal_by_npc: usize,
    /// Monotonic count of crabs the *player* has rustled back off a rival NPC train this run — the
    /// reciprocal "steal to win" splice (drive your train's head through a rival's line and its back
    /// section snaps onto yours). Never drops, so the bot playtests can assert the steal-back fired
    /// without racing the live chain count.
    pub(crate) crabs_stolen_by_player: usize,
    /// Monotonic count of armed rival steals the player has parried this run — an on-beat Stomp/Wave
    /// cast on a threatened tail cancels the splice (see `try_defend_steal`). Never drops, so the bot
    /// playtests can assert the defensive counter fired without racing the live chain count.
    pub(crate) steals_parried: usize,
    /// Monotonic count of armed rival steals the player has *dodged* this run — juking the threaded
    /// tail link clear of the rival before the snap breaks the thread, so the splice fizzles with
    /// nothing to cut (the movement half of the defense, alongside the tool parry). Never drops, so
    /// the bot playtests can assert the reroute defense fired without racing the live chain count.
    pub(crate) steals_dodged: usize,
    /// Monotonic count of revenge steal-backs this run — a steal-back rustled off a rival while its
    /// revenge marker was still live (it had just spliced your tail). Never drops, so the bot
    /// playtests can assert the back-and-forth revenge loop fired without racing the live chain.
    pub(crate) revenge_steals: usize,
    /// Monotonic count of crabs transferred between *rival* NPC trains this run — the whole-beach
    /// ecology steal where a bigger train splices a smaller rival's back half onto itself (agar.io /
    /// Rain World: big trains bully small ones). Never drops, so the bot playtest can assert the
    /// rival-vs-rival splice fired without racing the live follower counts, which churn constantly.
    pub(crate) rival_vs_rival_steals: usize,
    /// Monotonic count of crabs knocked *loose* as free catchable spoils by rival-vs-rival collisions
    /// (ROADMAP step 3, agar.io "eat the crumbs") — a fraction of each rival-vs-rival cut spills into
    /// the world instead of all transferring to the winner, so the player can swoop in and rustle them.
    /// Never drops, so the bot playtest can assert the spill fired without racing the live crab count.
    pub(crate) rival_spill_crabs: usize,
    /// Monotonic count of frames a rival-vs-rival "predator closing" hunt telegraph was drawn (ROADMAP
    /// step 3 "make it legible and swoopable"): the gold beat-marching line shown from a bigger King
    /// toward the smaller rival it's committed to hunting, so the player can read the impending clash
    /// and pre-position to swoop the spoils. Never drops, so the bot playtest can assert the anticipatory
    /// tell fired without racing the live hunt state, which flickers as trains close and separate.
    pub(crate) rival_hunt_telegraphs: usize,
    /// Monotonic count of frames a COMMITTED rival applied intercept steering against the player's
    /// train (#160 "smarter, scarier rival AI"): the strike phase of the stalk→strike player hunt,
    /// where the rival leads its aim by the player's velocity to cut off the vulnerable back half
    /// instead of chasing where it currently is. Never drops, so the bot playtest can assert the
    /// interception path fired without racing the live hunt state, which resets after each strike.
    pub(crate) hunt_intercepts: usize,
    /// One-frame flag: a rival spliced crabs off your tail this frame — play the "loss" steal sting.
    /// Set inside `update_npc_trains` (which has no `ctx`), consumed with `ctx` right after the call.
    pub(crate) steal_loss_sfx: bool,
    /// One-frame flag: you rustled crabs back off a rival this frame — play the "gain" steal sting.
    pub(crate) steal_gain_sfx: bool,
    /// One-frame latch: two rival trains collided and one rustled the other this frame, carrying the
    /// splice world position. Set inside `update_npc_trains` (no `ctx`), consumed with `ctx` right
    /// after so the audio pass can play the position-panned, distance-faded rival-steal clack.
    pub(crate) rival_steal_sfx: Option<Vec2>,
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
    // Playback speed currently applied to the music sources (action groove + layers), = the
    // gameplay tempo multiplier so the loop stays tempo-locked to the beat grid as the intensity
    // stage ramps `beat_interval`. 1.0 at WARM-UP; rises with each stage. Re-applied (set_pitch +
    // restart) only when it changes, so the music turntables up with the run instead of drifting.
    pub(crate) music_pitch: f32,
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
    // Live hi-hat kit: index of the last swung 1/16 sub-step whose hi-hat has fired, as a global
    // step id (`beat_count * 4 + local`). Lets the sub-beat scheduler fire each offbeat hat exactly
    // once as the beat clock crosses its swung onset, without double-firing or skipping across
    // frames (even at low fps). Initialised to -1 so the very first offbeat can fire.
    pub(crate) hat_last_step: i64,
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
    pub(crate) cached_steal_target_pos: Option<Vec2>, // position of the back-half chain link a rival should thread to slice (~2/3 down from head), refreshed once per frame in update_crabs from the same scan the boss uses. Ambient NPC trains route toward this so they deliberately cut the body, not just nip the tail. None on a short chain (< 4 links).
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
    // Positions where the beam is pinning a fleeing Fast crab — the beam's soft-RPS STRONG match
    // (INSPIRATION.md "Beam to melt fast ones"). The bool flags an on-beat pin (a harder clamp) so
    // draw_beam_fast_pin can flare the tell brighter on the beat. Mirrors the beam/Hermit buffer.
    pub(crate) beam_fast_hits_buf: Vec<(Vec2, bool)>,
    // Positions where the beam is spotlighting a fleeing Golden — the beam's soft-RPS STRONG match
    // against the prize crab: the flashlight *reveals and reels* the treasure, a gentler grip than the
    // Fast pin so the Golden stays a premium chase. The bool flags an on-beat hold (a firmer reel) so
    // draw_beam_golden_spotlight can flare the warm-gold tell brighter on the beat. Mirrors beam_fast.
    pub(crate) beam_golden_hits_buf: Vec<(Vec2, bool)>,
    // Positions where the beam is pinning a fleeing Sneaky crab — the beam's soft-RPS STRONG match
    // against the skittish evader. The whistle *gathers* the Sneaky herd (its flagship, enemies.rs);
    // the beam instead *exposes and pins* the lone Sneaky that darts out of the cone — a different verb
    // (single-target pin vs AOE gather), so the two tools stay complementary. A middling grip: firmer
    // than the premium-chase Golden reel, gentler than the definitive Fast clamp. The bool flags an
    // on-beat pin so draw_beam_sneaky_pin can flare the teal tell brighter on the beat. Mirrors beam_fast.
    pub(crate) beam_sneaky_hits_buf: Vec<(Vec2, bool)>,
    pub(crate) stomp_dancer_hits_buf: Vec<Vec2>,
    pub(crate) stomp_armored_hits_buf: Vec<Vec2>,
    pub(crate) whistle_golden_hits_buf: Vec<Vec2>,
    pub(crate) whistle_dancer_hits_buf: Vec<Vec2>,
    // Positions where the whistle's sweep is reeling in a skittish Sneaky crab — the whistle's
    // flagship soft-RPS strong match (enemies.rs whistle_pull 1.5, "folds hard to a whistle"),
    // and the one whistle matchup that was still visually silent while Golden and Dancer both had
    // tells. The bool flags an on-beat cast so draw_whistle_sneaky_match flares the tell brighter
    // on the beat (a herd-on-the-beat reward). Mirrors the whistle/Golden + whistle/Dancer buffers.
    pub(crate) whistle_sneaky_hits_buf: Vec<(Vec2, bool)>,
    // Positions where the whistle's sweep just ripped (on-beat) or loosened (off-beat) a latched
    // Thief off the conga tail — the whistle's defensive strong match (enemies.rs whistle_pull 1.3,
    // "yanks it off your tail nicely"). The one whistle strong-match still without a dedicated tell,
    // and the only Thief counterplay that was visually silent OFF the beat (the on-beat rip already
    // pops "THIEF NABBED!"). The bool flags an on-beat rip so draw_whistle_thief_match flares bright
    // and wide vs a dim off-beat loosen. Mirrors the whistle/Sneaky buffer.
    pub(crate) whistle_thief_hits_buf: Vec<(Vec2, bool)>,
    pub(crate) lasso_thief_hits_buf: Vec<Vec2>,
    pub(crate) lasso_magnet_hits_buf: Vec<Vec2>,
    // Positions where the lasso hauled in a heavy Big crab — its flagship soft-RPS match (the whistle
    // "shrugs most off", so the loop's physical drag is the answer). Carries an on-beat flag so an
    // on-beat throw flares the "heave" tell brighter and wider. Mirrors the whistle/Sneaky buffer.
    pub(crate) lasso_big_hits_buf: Vec<(Vec2, bool)>,
    // Positions where a lasso throw landed on a still-shelled crab (Armored / shelled Hermit) and
    // the loop slipped straight off — a WRONG-TOOL tell. Mirrors the beam/Hermit amber "can't-crack"
    // cue: teaches "crack the shell first (Stomp), then lasso" instead of failing silently.
    pub(crate) lasso_shell_deflect_hits_buf: Vec<Vec2>,
    // Positions where the whistle's sonic pulse swept over a still-shelled crab (Armored / shelled
    // Hermit) and pinged straight off — the whistle "barely nudges it" (enemies.rs whistle_pull 0.3).
    // The mirror of lasso_shell_deflect for the whistle: a WRONG-TOOL cue teaching "the shell shrugs
    // the whistle — crack it first (Stomp), then herd it," so the wrong tool reads as clearly as a match.
    pub(crate) whistle_shell_deflect_hits_buf: Vec<Vec2>,
    pub(crate) magnet_cluster_hits_buf: Vec<Vec2>,
    // Scratch per-magnet nearby-crab tally reused by the cluster-detection pass in update_crabs —
    // sized to magnet_positions_buf and reset to 0 each on-beat frame instead of being freshly
    // allocated, so the O(magnets) counting loop that replaced the old O(magnets * crabs) scan
    // doesn't itself become a per-beat-frame Vec allocation.
    pub(crate) magnet_cluster_counts_buf: Vec<u32>,
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
    pub(crate) boss_catches_buf: Vec<(Vec2, CrabType)>,
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
    // Cooldown so threading your head through a rival train can't strip every follower in one
    // frame — one clean steal per window, mirroring the rival's steal_cooldown against you.
    pub(crate) player_steal_cooldown: f32,
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
    // Fixed simulation timestep for deterministic bot runs (see MainState::frame_dt). `Some(dt)`
    // in headless bot mode pins every frame to the same `dt` so the sim is frame-count-driven and
    // reproducible; `None` in real gameplay keeps the variable wall-clock delta for smooth
    // rendering. Set once at startup in `main`.
    pub(crate) bot_fixed_dt: Option<f32>,
}

impl MainState {
    /// The per-frame simulation delta, in seconds. In a deterministic bot run this is a fixed
    /// constant (so the sim advances identically regardless of machine speed or ggez version);
    /// in real gameplay it's the true wall-clock frame delta for smooth motion. Every in-game
    /// `ctx.time.delta()` used to drive the sim funnels through here.
    #[inline]
    pub(crate) fn frame_dt(&self, ctx: &Context) -> f32 {
        self.bot_fixed_dt
            .unwrap_or_else(|| ctx.time.delta().as_secs_f32())
    }
}
