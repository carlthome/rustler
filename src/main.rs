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
use crate::enemies::EnemyCrab;
use crate::graphics::{
    FloatingTextSystem, ParticleSystem, cached_stroke_rect, draw_attracted_crab_glow,
    draw_armor_ring, draw_beat_indicator, draw_catch_shockwaves, draw_chain_rings,
    draw_combo_meter, draw_boss_health_ring, draw_conga_rope, draw_crab, draw_crab_radar,
    draw_fear_rings, draw_flashlight, draw_floating_texts, draw_grass, draw_lasso, draw_particles,
    draw_rustler, draw_stomp_ring, draw_whistle_ring, unit_square,
};
use crate::levels::{Level, get_levels};
use crate::spawnings::{spawn_boss, spawn_enemies};

const PLAYER_SIZE: f32 = 48.0;
const CRAB_SIZE: f32 = 36.0;
const SPEED: f32 = 200.0;
const CHAIN_LINK_FRAMES: usize = 12;
const BEAT_INTERVAL: f32 = 0.5; // 120 BPM, crab rave tempo
const BEAT_WINDOW: f32 = 0.08;  // seconds around a beat that count as "on beat"
const BOSS_MAX_HEALTH: f32 = 3.0; // seconds of sustained flashlight needed to wear a King Crab down
const BOSS_DRAIN_RATE: f32 = 1.0; // boss health drained per second while held in the beam
const BOSS_SCORE_INTERVAL: usize = 40; // score gap between successive King Crab arrivals
const WHISTLE_COOLDOWN: f32 = 4.5;     // seconds between whistle casts
const WHISTLE_RING_SPEED: f32 = 1000.0; // how fast the sonic front sweeps outward (px/s)
const WHISTLE_MAX_RADIUS: f32 = 360.0; // reach of the pulse — crabs inside it get yanked in
const WHISTLE_PULL_SPEED: f32 = 240.0; // base inward speed applied to caught-in crabs (× type pull)
const STOMP_COOLDOWN: f32 = 3.0;       // seconds between ground-pound Stomps
const STOMP_RING_SPEED: f32 = 900.0;   // how fast the shockwave slams outward (px/s)
const STOMP_MAX_RADIUS: f32 = 155.0;   // short reach — the Stomp is a close-range melee counter

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
    floating_texts: FloatingTextSystem,
    combo_count: usize,
    combo_timer: f32,
    textures: GameTextures,                    // Textures for grass, sand, and player
    level_textures: Vec<LevelTexture>,         // Textures for each level
    // Beat Wave ability
    beat_count: u32,                           // Counts beats fired, every 4th triggers wave
    beat_wave_active: bool,                    // Whether beat wave is expanding
    beat_wave_radius: f32,                     // Current radius of expanding wave
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
    // Stomp ability — a close-range ground-pound that CRACKS armored crab shells instantly (its
    // dedicated counter; the beam is the slow universal fallback) and shoves nearby free crabs in.
    stomp_active: f32,                          // >0 while the shockwave is expanding (seconds remaining)
    stomp_radius: f32,                          // current front radius of the shockwave
    stomp_cooldown: f32,                        // >0 while on cooldown; Stomp unusable until it hits 0
    stomp_center: Vec2,                         // player center captured at stomp time (ring origin)
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
    next_boss_score: usize,              // score at which the next King Crab boss arrives
    chain_rings: Vec<(Vec2, f32, [f32; 3])>, // (pos, age 0..1, rgb) for beat ghost rings
    catch_shockwaves: Vec<(Vec2, f32, [f32; 3])>, // (pos, age 0..1, rgb) impact ring per catch
    fear_rings: Vec<(Vec2, f32)>,          // (pos, age 0..1) cold alarm ring where a catch startled the herd
    zoom_punch: f32,            // camera zoom-in kick on catch, springs back to 0 (juice)
    fullscreen_applied: bool, // deferred until the first update tick, see update()
    // Scratch buffers for catch_by_chain, reused every frame instead of being freshly
    // allocated each call. The play area is fixed-size so the grid's cell count (and thus
    // its Vec<usize> bucket count) stabilizes quickly — clearing beats rebuilding from scratch.
    chain_positions_buf: Vec<Vec2>,
    catch_grid_buf: std::collections::HashMap<(i32, i32), Vec<usize>>,
    caught_now_buf: Vec<bool>,
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
            floating_texts: FloatingTextSystem::new(),
            combo_count: 0,
            combo_timer: 0.0,
            beat_count: 0,
            beat_wave_active: false,
            beat_wave_radius: 0.0,
            lasso_pos: None,
            lasso_timer: 0.0,
            lasso_target: Vec2::ZERO,
            whistle_active: 0.0,
            whistle_radius: 0.0,
            whistle_cooldown: 0.0,
            whistle_center: Vec2::ZERO,
            stomp_active: 0.0,
            stomp_radius: 0.0,
            stomp_cooldown: 0.0,
            stomp_center: Vec2::ZERO,
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
            chain_rings: Vec::new(),
            catch_shockwaves: Vec::new(),
            fear_rings: Vec::new(),
            zoom_punch: 0.0,
            fullscreen_applied: false,
            chain_positions_buf: Vec::new(),
            catch_grid_buf: std::collections::HashMap::new(),
            caught_now_buf: Vec::new(),
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
            if crab.caught
                || crab.is_boss()
                || crab.in_flashlight
                || crab.fleeing
                || crab.startle_timer > 0.0
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
        // Did a panicking wild crab just run into the tail?
        let hit = self.crabs.iter().any(|c| {
            !c.caught
                && !c.is_boss()
                && (c.fleeing || c.startle_timer > 0.0)
                && c.pos.distance(tail_pos) < SNAP_COLLIDE_DIST
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

    fn handle_crab_catching(&mut self, ctx: &mut Context) {
        let mult = self.combo_multiplier();
        let mut any_caught = false;
        let mut startle_origins: Vec<Vec2> = Vec::new();
        let mut boss_catches: Vec<Vec2> = Vec::new();
        for crab in &mut self.crabs {
            if crab.is_catchable()
                && (self.player_pos.x - crab.pos.x).abs() < (PLAYER_SIZE + crab.scale) / 2.0
                && (self.player_pos.y - crab.pos.y).abs() < (PLAYER_SIZE + crab.scale) / 2.0
            {
                if crab.is_boss() {
                    boss_catches.push(crab.pos);
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
                if rng.random_range(0..5) == 0 {
                    let _ = self.sounds.success2.play_detached(ctx);
                } else {
                    let _ = self.sounds.success.play_detached(ctx);
                }
                if self.score > 0 && self.score % 10 == 0 {
                    let _ = self.sounds.upgrade.play_detached(ctx);
                    self.pending_upgrade = true;
                }
            }
        }
        for origin in startle_origins {
            self.emit_catch_startle(origin);
        }
        for bpos in boss_catches {
            self.on_boss_caught(bpos);
        }
        if any_caught {
            self.check_milestone(&mut rand::rng());
        }
    }

    /// Big celebratory payoff when a worn-down King Crab is finally snagged.
    fn on_boss_caught(&mut self, pos: Vec2) {
        let mut rng = rand::rng();
        let bonus = 25 * self.combo_multiplier();
        self.score += bonus;
        self.particle_system
            .spawn_milestone_fireworks(pos, 30, &mut rng);
        let screen_center = Vec2::new(self.width / 2.0 - 200.0, self.height / 2.0 - 90.0);
        self.floating_texts.spawn(
            "KING CRAB CAUGHT!".to_string(),
            screen_center + Vec2::new(3.0, 3.0),
            64.0,
            [0.0, 0.0, 0.0, 0.85],
        );
        self.floating_texts.spawn(
            "KING CRAB CAUGHT!".to_string(),
            screen_center,
            64.0,
            [1.0, 0.85, 0.2, 1.0],
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
            self.catch_shockwaves.push((pos, 0.0, [1.0, 0.8, 0.2]));
        }
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
                self.on_boss_caught(pos);
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
            if rng.random_range(0..5) == 0 {
                let _ = self.sounds.success2.play_detached(ctx);
            } else {
                let _ = self.sounds.success.play_detached(ctx);
            }
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

        // Positions of crabs that just entered panic-flee this frame — we'll emit "!" pops after the loop
        let mut flee_pops: Vec<Vec2> = Vec::new();
        // Positions of King Crabs that just got worn down this frame — celebrate after the loop
        let mut boss_broke: Vec<Vec2> = Vec::new();
        // Positions of Armored crabs whose shell the beam just wore through — pop a "crack" after the loop
        let mut armor_broke: Vec<Vec2> = Vec::new();
        // Sparkle particles for attracted crabs (collected to avoid borrow conflict)
        let mut attraction_particles: Vec<(Vec2, Vec2, f32, [f32; 3])> = Vec::new();

        for crab in &mut self.crabs {
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
                    crab.boss_health -= BOSS_DRAIN_RATE * dt;
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
                let now_fleeing = !crab_in_light && distance < FLEE_RADIUS && !crab.is_boss();

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
        for (pos, vel, life, [cr, cg, cb]) in attraction_particles {
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
        for pos in boss_broke {
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

        // Armored shells the beam just wore through — a lighter "crack" than the boss fanfare
        for pos in armor_broke {
            self.floating_texts.spawn(
                "SHELL CRACKED!".to_string(),
                pos - Vec2::new(70.0, 40.0),
                26.0,
                [0.7, 0.85, 1.0, 1.0],
            );
            self.spawn_catch_shockwave(pos, [0.7, 0.8, 0.95]);
        }

        // Emit "!" floating texts for crabs that just started fleeing this frame
        for pos in flee_pops {
            self.floating_texts.spawn(
                "!".to_string(),
                pos - Vec2::new(0.0, 24.0),
                28.0,
                [1.0, 0.9, 0.1, 1.0],
            );
        }

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
        }
        if self.current_level >= self.levels.len() {
            // Game completed, show game over screen.
            self.game_over = true;
        }
        let area = (self.width, self.height);
        self.start_current_pattern(area);
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
        self.floating_texts.texts.clear();
        self.combo_count = 0;
        self.combo_timer = 0.0;
        self.beat_count = 0;
        self.beat_wave_active = false;
        self.beat_wave_radius = 0.0;
        self.lasso_pos = None;
        self.lasso_timer = 0.0;
        self.lasso_target = Vec2::ZERO;
        self.whistle_active = 0.0;
        self.whistle_radius = 0.0;
        self.whistle_cooldown = 0.0;
        self.stomp_active = 0.0;
        self.stomp_radius = 0.0;
        self.stomp_cooldown = 0.0;
        self.dash_just_fired = false;
        self.dash_flash = 0.0;
        self.screen_shake = 0.0;
        self.screen_shake_vel = Vec2::ZERO;
        self.screen_shake_offset = Vec2::ZERO;
        self.hitstop_timer = 0.0;
        self.chain_join_ripple = false;
        self.next_milestone = 5;
        self.next_boss_score = BOSS_SCORE_INTERVAL;
        self.chain_rings.clear();
        self.catch_shockwaves.clear();
        self.fear_rings.clear();
        self.player_pos = player_pos;
        self.score = 0;
        self.spawn_timer = 0.0;
        self.time_elapsed = 0.0;
        self.game_over = false;
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
        // Draw a solid background to hide all graphics
        let bg = Mesh::new_rectangle(
            ctx,
            ggez::graphics::DrawMode::fill(),
            Rect::new(0.0, 0.0, width, height),
            Color::BLACK,
        )?;
        canvas.draw(&bg, DrawParam::default());

        // Draw game title (split into main title and subtitle)
        let mut main_title = Text::new("Crab Rustler");
        main_title.set_scale(112.0);
        let main_title_width = main_title.measure(ctx)?.x;
        let main_title_height = main_title.measure(ctx)?.y;

        // Use the stored subtitle
        let mut subtitle = Text::new(&self.subtitle);
        subtitle.set_scale(20.0);
        let subtitle_width = subtitle.measure(ctx)?.x;
        let _subtitle_height = subtitle.measure(ctx)?.y;

        // Draw shadow for main title
        canvas.draw(
            &main_title,
            DrawParam::default()
                .dest(Vec2::new(
                    (width - main_title_width) / 2.0 + 8.0,
                    (height - main_title_height) / 4.0 + 8.0,
                ))
                .color(Color::from_rgba(0, 0, 0, 180))
                .rotation(0.05),
        );

        // Draw main title with a wavy color effect
        for (i, ch) in "Crab Rustler".chars().enumerate() {
            let frag = ggez::graphics::TextFragment::new(ch).scale(112.0);
            let ch_text = Text::new(frag);
            let x = (width - main_title_width) / 2.0 + i as f32 * 60.0;
            let y = (height - main_title_height) / 4.0 + (i as f32 * 0.5).sin() * 16.0;

            let color = Color::from_rgb(
                220 + ((i as f32 * 0.7).sin() * 35.0) as u8,
                80 + ((i as f32 * 1.3).cos() * 140.0) as u8,
                255 - (i as u8 * 7),
            );
            canvas.draw(
                &ch_text,
                DrawParam::default()
                    .dest(Vec2::new(x, y))
                    .color(color)
                    .rotation((i as f32 * 0.1).sin() * 0.08),
            );
        }

        // Draw subtitle centered below the main title.
        canvas.draw(
            &subtitle,
            DrawParam::default()
                .dest(Vec2::new(
                    (width - subtitle_width) / 2.0,
                    (height - main_title_height) / 4.0 + main_title_height + 16.0,
                ))
                .color(Color::from_rgb(255, 255, 255)),
        );

        // Draw instructions text centered.
        let text = Text::new(
            "Catch all the crabs!\n\nMove: Arrow keys / WASD\nAim flashlight: Mouse\nDash: Space\nThrow lasso: Left click\nBeat wave burst: Q\nWhistle (pulls crabs in): E\nStomp (cracks armored crabs): R\n\nPress Space or Enter to start.",
        );
        let text_width = text.measure(ctx)?.x;
        let text_height = text.measure(ctx)?.y;
        canvas.draw(
            &text,
            DrawParam::default()
                .dest(Vec2::new(
                    (width - text_width) / 2.0,
                    (height - text_height) / 2.0 + 100.0,
                ))
                .color(Color::from_rgb(255, 255, 0)),
        );
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

        // Collect chain crabs sorted by chain index
        let mut chain_crabs: Vec<&EnemyCrab> = self.crabs
            .iter()
            .filter(|c| c.caught && c.chain_index.is_some())
            .collect();
        chain_crabs.sort_by_key(|c| c.chain_index.unwrap_or(0));
        // Draw beat ghost rings under the rope and crabs
        draw_chain_rings(ctx, canvas, &self.chain_rings)?;
        draw_conga_rope(ctx, canvas, self.player_pos, &chain_crabs, self.time_elapsed, self.beat_intensity)?;

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

        // Speed lines trailing behind player while dashing
        if self.boost_timer > 0.0 && self.last_dir.length() > 0.01 {
            let center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            let wake = -self.last_dir.normalize();
            let perp = Vec2::new(-wake.y, wake.x);
            let intensity = self.boost_timer / 0.18;
            for i in 0i32..7 {
                let t = (i as f32 - 3.0) / 3.0;
                let origin = center + perp * (t * 14.0);
                let length = 20.0 + (3.0 - (i as f32 - 3.0).abs()) * 8.0;
                let end = origin + wake * length;
                let alpha = (intensity * 110.0) as u8;
                let line = Mesh::new_line(ctx,
                    &[[origin.x, origin.y], [end.x, end.y]],
                    1.5, Color::from_rgba(190, 215, 255, alpha))?;
                canvas.draw(&line, DrawParam::default());
            }
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

        // Draw beat wave circle outline
        if self.beat_wave_active && self.beat_wave_radius > 0.0 {
            let player_center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            let alpha = ((1.0 - self.beat_wave_radius / 300.0) * 150.0) as u8;
            let wave_circle = Mesh::new_circle(
                ctx,
                ggez::graphics::DrawMode::stroke(3.0),
                [0.0, 0.0],
                self.beat_wave_radius,
                1.0,
                Color::from_rgba(255, 200, 100, alpha),
            )?;
            canvas.draw(&wave_circle, DrawParam::default().dest(player_center));
        }

        // Draw the whistle sonic pulse
        if self.whistle_active > 0.0 && self.whistle_radius > 0.0 {
            draw_whistle_ring(
                ctx,
                canvas,
                self.whistle_center,
                self.whistle_radius,
                WHISTLE_MAX_RADIUS,
            )?;
        }

        // Draw the stomp ground-pound shockwave
        if self.stomp_active > 0.0 && self.stomp_radius > 0.0 {
            draw_stomp_ring(
                ctx,
                canvas,
                self.stomp_center,
                self.stomp_radius,
                STOMP_MAX_RADIUS,
            )?;
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

        // Draw label
        let label = Text::new("Stamina (Space)");
        canvas.draw(
            &label,
            DrawParam::default()
                .dest(Vec2::new(bar_x, bar_y - 22.0))
                .color(Color::from_rgb(255, 255, 255)),
        );

        // Whistle cooldown bar (E) — fills back up to amber as it recharges, ready when full.
        let wbar_y = bar_y + bar_height + 26.0;
        let wbar_h = 12.0;
        let ready = self.whistle_cooldown <= 0.0;
        let charge = (1.0 - self.whistle_cooldown / WHISTLE_COOLDOWN).clamp(0.0, 1.0);
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
        let wlabel = Text::new(if ready { "Whistle (E) READY" } else { "Whistle (E)" });
        canvas.draw(
            &wlabel,
            DrawParam::default()
                .dest(Vec2::new(bar_x + bar_width + 8.0, wbar_y - 2.0))
                .color(Color::from_rgb(255, 230, 150)),
        );

        // Stomp cooldown bar (R) — steely blue, refills as the ground-pound recharges.
        let sbar_y = wbar_y + wbar_h + 20.0;
        let sbar_h = 12.0;
        let sready = self.stomp_cooldown <= 0.0;
        let scharge = (1.0 - self.stomp_cooldown / STOMP_COOLDOWN).clamp(0.0, 1.0);
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
        let slabel = Text::new(if sready { "Stomp (R) READY" } else { "Stomp (R)" });
        canvas.draw(
            &slabel,
            DrawParam::default()
                .dest(Vec2::new(bar_x + bar_width + 8.0, sbar_y - 2.0))
                .color(Color::from_rgb(190, 215, 245)),
        );

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
                // King Crab aura + wear-down health ring
                if crab.is_boss() {
                    let size = crab.scale * CRAB_SIZE;
                    let frac = crab.boss_health / BOSS_MAX_HEALTH;
                    draw_boss_health_ring(ctx, canvas, pos, size, frac, self.time_elapsed)?;
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
        Ok(())
    }

    fn draw_game_over_screen(&self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        let box_width = 600.0;
        let box_height = 200.0;
        let box_x = 340.0;
        let box_y = 380.0;
        let bg_box = Mesh::new_rectangle(
            ctx,
            ggez::graphics::DrawMode::fill(),
            Rect::new(box_x, box_y, box_width, box_height),
            Color::from_rgba(40, 0, 80, 180),
        )?;
        canvas.draw(&bg_box, DrawParam::default());
        let text = Text::new(format!(
            "Game Over!\nTime: {:.2} seconds\nBest Time: {:.2} seconds\nPress Esc to quit.\n\nPress Space or Enter to try again.",
            self.time_elapsed, self.best_time
        ));
        canvas.draw(
            &text,
            DrawParam::default()
                .dest(Vec2::new(370.0, 400.0))
                .color(Color::WHITE),
        );
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

        // (key, icon, name, description, r, g, b)
        let cards: &[(&str, &str, &str, &str, u8, u8, u8)] = &[
            ("1", ">",  "Wider Cone",   "Flashlight sweeps\na broader arc",       255, 200,  40),
            ("2", "~",  "Longer Range", "Flashlight reaches\nfurther ahead",       80, 160, 255),
            ("3", "*",  "Disco Laser",  "Add another\nrainbow beam",              200,  60, 255),
            ("4", "O",  "Chain Reach",  "Catch crabs from\nfurther with chain",    60, 220, 100),
        ];

        let rects = self.upgrade_card_rects();
        let card_w = rects[0].w;
        let card_h = rects[0].h;

        for (i, &(key, icon, name, desc, r, g, b)) in cards.iter().enumerate() {
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

            // Description
            let mut dsc = Text::new(desc);
            dsc.set_scale(18.0);
            let dw = dsc.measure(ctx)?.x;
            canvas.draw(&dsc, DrawParam::default()
                .dest(Vec2::new(cx + (card_w - dw) / 2.0, y0 + 156.0))
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

    fn apply_upgrade(&mut self, choice: u8) {
        match choice {
            1 => self.flashlight.cone_upgrade += 0.25,
            2 => self.flashlight.range_upgrade += 60.0,
            3 => self.flashlight.laser_level += 1,
            4 => self.catch_radius_upgrade += 25.0,
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
            // Every 4th beat, auto-fire beat wave when score >= 20
            if self.beat_count % 4 == 0 && self.score >= 20 && !self.beat_wave_active {
                self.beat_wave_active = true;
                self.beat_wave_radius = 0.0;
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
            // Spawn ghost rings at each chain crab position
            for crab in self.crabs.iter().filter(|c| c.caught) {
                let color = crab.crab_color();
                self.chain_rings.push((crab.pos, 0.0, color));
            }
            // Emergent beat-startle chain reaction: panic ripples crab-to-crab on the pulse.
            self.beat_startle_contagion();
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

        // Chain-as-risk: a spooked wild crab barreling into the exposed tail can snap links loose.
        self.snap_chain_on_panic();

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
            self.whistle_active = (self.whistle_active - dt).max(0.0);
            self.whistle_radius = (self.whistle_radius + WHISTLE_RING_SPEED * dt).min(WHISTLE_MAX_RADIUS);
            let center = self.whistle_center;
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
                    let proximity = 1.0 - (dist / WHISTLE_MAX_RADIUS).clamp(0.0, 1.0);
                    let speed = WHISTLE_PULL_SPEED * pull * (0.5 + proximity * 0.5);
                    crab.vel = toward * speed;
                    // Count as attracted so the flee/wobble logic doesn't fight the pull next frame.
                    crab.spooked_timer = crab.spooked_timer.max(0.6);
                    crab.fleeing = false;
                }
            }
        }

        // Stomp: a close-range ground-pound shockwave. It CRACKS Armored crab shells instantly (its
        // dedicated counter — the beam is the slow universal fallback) and gives any free crab the
        // front passes a light inward shove. Its short reach makes it a melee tool, not a ranged
        // gather like the whistle/lasso, so choosing the right verb per herd is a real decision.
        if self.stomp_active > 0.0 {
            self.stomp_active = (self.stomp_active - dt).max(0.0);
            self.stomp_radius = (self.stomp_radius + STOMP_RING_SPEED * dt).min(STOMP_MAX_RADIUS);
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
                    let to_catch: Vec<usize> = self.crabs.iter().enumerate()
                        .filter(|(_, c)| c.is_catchable() && tip.distance(c.pos) < 60.0)
                        .map(|(i, _)| i)
                        .collect();
                    let mut rng = rand::rng();
                    for i in to_catch {
                        let pos = self.crabs[i].pos;
                        let crab_type = self.crabs[i].crab_type;
                        let crab_color = self.crabs[i].crab_color();
                        self.particle_system.spawn_catch_effect(pos, crab_color, crab_type, &mut rng);
                        self.spawn_catch_shockwave(pos, crab_color);
                        self.crabs[i].caught = true;
                        if self.crabs[i].is_boss() {
                            self.on_boss_caught(pos);
                        }
                        self.chain_join_ripple = true;
                        self.crabs[i].chain_index = Some(self.chain_count);
                        self.chain_count += 1;
                        self.check_milestone(&mut rand::rng());
                        self.score += self.combo_multiplier();
                        self.shake_timer = 0.15;
                        self.hitstop_timer = self.hitstop_timer.max(0.06);
                        self.time_since_catch = 0.0;
                        if rng.random_range(0..5) == 0 {
                            let _ = self.sounds.success2.play_detached(ctx);
                        } else {
                            let _ = self.sounds.success.play_detached(ctx);
                        }
                        if self.score > 0 && self.score % 10 == 0 {
                            let _ = self.sounds.upgrade.play_detached(ctx);
                            self.pending_upgrade = true;
                        }
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
            let boss = spawn_boss((self.width, self.height), &mut rand::rng(), BOSS_MAX_HEALTH);
            let bpos = boss.pos;
            self.crabs.push(boss);
            self.floating_texts.spawn(
                "A KING CRAB APPROACHES!".to_string(),
                Vec2::new(self.width / 2.0 - 230.0, 80.0),
                46.0,
                [1.0, 0.8, 0.2, 1.0],
            );
            self.floating_texts.spawn(
                "Hold your light on it!".to_string(),
                Vec2::new(self.width / 2.0 - 120.0, 130.0),
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

        self.pattern_timer -= dt;
        if self.crabs.iter().all(|c| c.caught) || self.pattern_timer <= 0.0 {
            self.advance_pattern();
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
