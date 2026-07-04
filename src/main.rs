mod controls;
mod enemies;
mod graphics;
mod levels;
mod spawnings;

use std::{collections::VecDeque, env, fs, path};

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
    FloatingTextSystem, ParticleSystem, draw_beat_indicator, draw_combo_meter, draw_conga_rope,
    draw_crab, draw_crab_radar, draw_flashlight, draw_floating_texts, draw_grass, draw_particles,
    draw_rustler,
};
use crate::levels::{Level, get_levels};
use crate::spawnings::spawn_enemies;

const PLAYER_SIZE: f32 = 48.0;
const CRAB_SIZE: f32 = 36.0;
const SPEED: f32 = 200.0;
const CHAIN_LINK_FRAMES: usize = 12;
const BEAT_INTERVAL: f32 = 0.5; // 120 BPM, crab rave tempo
const BEAT_WINDOW: f32 = 0.08;  // seconds around a beat that count as "on beat"

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
    // Dash effect
    dash_just_fired: bool,
    dash_flash: f32,
    // Camera shake
    screen_shake: f32,          // current shake magnitude (pixels), decays each frame
    screen_shake_vel: Vec2,     // current shake offset velocity
    screen_shake_offset: Vec2,  // current pixel offset applied to viewport
    chain_join_ripple: bool,       // set true when any crab is caught this frame
    next_milestone: usize,               // Next train-length milestone to celebrate
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
            dash_just_fired: false,
            dash_flash: 0.0,
            screen_shake: 0.0,
            screen_shake_vel: Vec2::ZERO,
            screen_shake_offset: Vec2::ZERO,
            chain_join_ripple: false,
            next_milestone: 5,
        })
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

            // Amplify beat flash
            self.beat_intensity = (self.beat_intensity + 1.5).min(2.0);
            self.on_beat_flash = 0.5;
        }
    }

    fn handle_crab_catching(&mut self, ctx: &mut Context) {
        let mult = self.combo_multiplier();
        let mut any_caught = false;
        for crab in &mut self.crabs {
            if !crab.caught
                && (self.player_pos.x - crab.pos.x).abs() < (PLAYER_SIZE + crab.scale) / 2.0
                && (self.player_pos.y - crab.pos.y).abs() < (PLAYER_SIZE + crab.scale) / 2.0
            {
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

                crab.caught = true;
                self.chain_join_ripple = true;
                any_caught = true;
                crab.chain_index = Some(self.chain_count);
                self.chain_count += 1;
                let on_beat = self.beat_timer < BEAT_WINDOW
                    || self.beat_timer > BEAT_INTERVAL - BEAT_WINDOW;
                if on_beat {
                    self.on_beat_flash = 0.25;
                }
                let bonus = if on_beat { 1 } else { 0 };
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
        if any_caught {
            self.check_milestone(&mut rand::rng());
        }
    }

    fn catch_by_chain(&mut self, ctx: &mut Context) {
        let catch_radius = 45.0 + self.catch_radius_upgrade;
        let chain_positions: Vec<Vec2> = self.crabs.iter()
            .filter(|c| c.caught)
            .map(|c| c.pos)
            .collect();
        if chain_positions.is_empty() {
            return;
        }
        let to_catch: Vec<usize> = self.crabs.iter().enumerate()
            .filter(|(_, c)| {
                !c.caught
                    && chain_positions
                        .iter()
                        .any(|cp| cp.distance(c.pos) < catch_radius)
            })
            .map(|(i, _)| i)
            .collect();
        let mut rng = rand::rng();
        for i in to_catch {
            let pos = self.crabs[i].pos;
            let crab_type = self.crabs[i].crab_type;
            let crab_color = self.crabs[i].crab_color();
            self.particle_system
                .spawn_catch_effect(pos, crab_color, crab_type, &mut rng);
            self.crabs[i].caught = true;
            self.chain_join_ripple = true;
            self.crabs[i].chain_index = Some(self.chain_count);
            self.chain_count += 1;
            self.check_milestone(&mut rand::rng());
            let pos = self.crabs[i].pos;
            self.register_catch(pos, 0);
            self.shake_timer = 0.15;
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

                // Panic flee: crabs that are close but outside the flashlight beam scatter away.
                const FLEE_RADIUS: f32 = 220.0;
                let now_fleeing = !crab_in_light && distance < FLEE_RADIUS;

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
            }
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

        // Move chain crabs to their historical positions (conga train)
        let targets: Vec<(usize, Vec2)> = self.crabs
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                c.chain_index.and_then(|ci| {
                    let history_idx = (ci + 1) * CHAIN_LINK_FRAMES;
                    self.position_history.get(history_idx).map(|&p| (i, p))
                })
            })
            .collect();
        for (i, target) in targets {
            self.crabs[i].pos = self.crabs[i].pos.lerp(target, 0.4);
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
        self.dash_just_fired = false;
        self.dash_flash = 0.0;
        self.screen_shake = 0.0;
        self.screen_shake_vel = Vec2::ZERO;
        self.screen_shake_offset = Vec2::ZERO;
        self.chain_join_ripple = false;
        self.next_milestone = 5;
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
            "Catch all the crabs!\n\nMove: Arrow keys / WASD\nAim flashlight: Mouse\nDash: Space\nThrow lasso: Left click\nBeat wave burst: Q\n\nPress Space or Enter to start.",
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

        // Draw level background.
        draw_grass(
            ctx,
            canvas,
            width,
            height,
            texture,
            &self.shader,
            self.time_elapsed,
        )?;

        // Subtle beat pulse: a green flash on every downbeat
        if self.beat_intensity > 0.0 {
            let pulse_alpha = (self.beat_intensity * 28.0) as u8;
            let pulse = Mesh::new_rectangle(
                ctx,
                ggez::graphics::DrawMode::fill(),
                Rect::new(0.0, 0.0, width, height),
                Color::from_rgba(80, 255, 80, pulse_alpha),
            )?;
            canvas.draw(&pulse, DrawParam::default());
        }

        // Collect chain crabs sorted by chain index
        let mut chain_crabs: Vec<&EnemyCrab> = self.crabs
            .iter()
            .filter(|c| c.caught && c.chain_index.is_some())
            .collect();
        chain_crabs.sort_by_key(|c| c.chain_index.unwrap_or(0));
        draw_conga_rope(ctx, canvas, self.player_pos, &chain_crabs, self.time_elapsed, self.beat_intensity)?;

        // Draw player character.
        draw_rustler(ctx, canvas, self.player_pos, &self.textures.player)?;

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

        // Draw lasso line and tip
        if let Some(tip) = self.lasso_pos {
            let player_center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            // Determine outward progress (0..1) and spin angle
            let elapsed = 0.5 - self.lasso_timer;
            let outward_progress = (elapsed / 0.3).clamp(0.0, 1.0);
            let spin = self.time_elapsed * 18.0; // fast spin in radians/sec

            if player_center.distance(tip) > 1.0 {
                // Glowing rope: thick glow pass + thin bright pass
                let rope_glow = Mesh::new_line(
                    ctx,
                    &[player_center, tip],
                    6.0,
                    Color::from_rgba(230, 160, 30, 60),
                )?;
                let orig_blend = canvas.blend_mode();
                canvas.set_blend_mode(BlendMode::ADD);
                canvas.draw(&rope_glow, DrawParam::default());
                canvas.set_blend_mode(orig_blend);

                let rope = Mesh::new_line(
                    ctx,
                    &[player_center, tip],
                    2.5,
                    Color::from_rgba(220, 160, 50, 220),
                )?;
                canvas.draw(&rope, DrawParam::default());
            }

            // Catch-radius indicator ring (fades in as lasso extends)
            let catch_r = 60.0_f32;
            let ring_alpha = (outward_progress * 80.0) as u8;
            if ring_alpha > 4 {
                let catch_ring = Mesh::new_circle(
                    ctx,
                    ggez::graphics::DrawMode::stroke(1.5),
                    [0.0, 0.0],
                    catch_r,
                    1.5,
                    Color::from_rgba(255, 220, 80, ring_alpha),
                )?;
                canvas.draw(&catch_ring, DrawParam::default().dest(tip));
            }

            // Spinning lasso loop: draw as a partial arc (330° gap = open lasso)
            // We build 11 short line segments tracing a circle, leaving a gap
            let loop_r = 18.0 + outward_progress * 6.0; // grows as it flies
            let segments = 20_usize;
            let arc_fraction = 0.88; // 88% of circle = open loop
            let mut loop_pts: Vec<[f32; 2]> = Vec::with_capacity(segments + 1);
            for s in 0..=segments {
                let angle = spin + (s as f32 / segments as f32) * arc_fraction * std::f32::consts::TAU;
                loop_pts.push([tip.x + angle.cos() * loop_r, tip.y + angle.sin() * loop_r]);
            }
            if loop_pts.len() >= 2 {
                // Glow
                let loop_glow = Mesh::new_line(ctx, &loop_pts, 8.0,
                    Color::from_rgba(255, 200, 60, 80))?;
                let orig_blend = canvas.blend_mode();
                canvas.set_blend_mode(BlendMode::ADD);
                canvas.draw(&loop_glow, DrawParam::default());
                canvas.set_blend_mode(orig_blend);
                // Main loop line
                let loop_line = Mesh::new_line(ctx, &loop_pts, 3.5,
                    Color::from_rgba(255, 210, 70, 230))?;
                canvas.draw(&loop_line, DrawParam::default());
            }

            // Bright center dot at the tip knot
            let knot = Mesh::new_circle(
                ctx,
                ggez::graphics::DrawMode::fill(),
                [0.0, 0.0],
                5.0,
                0.5,
                Color::from_rgba(255, 240, 160, 240),
            )?;
            canvas.draw(&knot, DrawParam::default().dest(tip));
        }

        // Show stats.
        let chain_len = self.crabs.iter().filter(|c| c.caught).count();
        let hud = if self.combo_count >= 3 {
            let mult = self.combo_multiplier();
            format!("Score: {}  |  Train: {}  |  Combo x{}  [{}x pts]", self.score, chain_len, self.combo_count, mult)
        } else {
            format!("Score: {}  |  Train: {}", self.score, chain_len)
        };
        let text = Text::new(hud);
        canvas.draw(
            &text,
            DrawParam::default()
                .dest(Vec2::new(10.0, 10.0))
                .color(Color::from_rgb(255, 255, 00)),
        );

        // Draw stamina bar for boost timer/cooldown
        let bar_x = 10.0;
        let bar_y = 50.0;
        let bar_width = 220.0;
        let bar_height = 18.0;
        let max_boost = 0.18;
        let max_cooldown = 0.08;
        let cooldown_ratio = (self.boost_cooldown / max_cooldown).clamp(0.0, 1.0);

        // Draw background bar
        let bg_bar = Mesh::new_rectangle(
            ctx,
            ggez::graphics::DrawMode::fill(),
            Rect::new(bar_x, bar_y, bar_width, bar_height),
            Color::from_rgb(40, 40, 40),
        )?;
        canvas.draw(&bg_bar, DrawParam::default());

        // Draw boost timer (yellow)
        let ratio = ((max_boost - self.boost_timer) / max_boost).clamp(0.0, 1.0);
        if ratio > 0.0 {
            let boost_bar = Mesh::new_rectangle(
                ctx,
                ggez::graphics::DrawMode::fill(),
                Rect::new(bar_x, bar_y, bar_width * ratio, bar_height),
                Color::from_rgb(255, 220, 40),
            )?;
            canvas.draw(&boost_bar, DrawParam::default());
        }

        // Draw cooldown (red, overlays boost)
        if cooldown_ratio > 0.0 {
            let cooldown_bar = Mesh::new_rectangle(
                ctx,
                ggez::graphics::DrawMode::fill(),
                Rect::new(bar_x, bar_y, bar_width * cooldown_ratio, bar_height),
                Color::from_rgb(220, 60, 60),
            )?;
            canvas.draw(&cooldown_bar, DrawParam::default());
        }

        // Draw stamina bar border
        let border = Mesh::new_rectangle(
            ctx,
            ggez::graphics::DrawMode::stroke(2.0),
            Rect::new(bar_x, bar_y, bar_width, bar_height),
            Color::from_rgb(255, 255, 255),
        )?;
        canvas.draw(&border, DrawParam::default());

        // Draw label
        let label = Text::new("Stamina (Space)");
        canvas.draw(
            &label,
            DrawParam::default()
                .dest(Vec2::new(bar_x, bar_y - 22.0))
                .color(Color::from_rgb(255, 255, 255)),
        );

        // Show current level at the bottom center.
        if self.level_title_timer == 0.0 {
            let mut level_label = Text::new(format!(
                "Level {}: {}\n{} | Difficulty: {}",
                self.current_level + 1,
                self.levels[self.current_level].title,
                self.levels[self.current_level].description,
                self.levels[self.current_level].difficulty
            ));

            level_label.set_scale(18.0);
            let label_width = level_label.measure(ctx)?.x;
            let label_height = level_label.measure(ctx)?.y;
            canvas.draw(
                &level_label,
                DrawParam::default()
                    .dest(Vec2::new(
                        (width - label_width) / 2.0,
                        height - label_height - 18.0,
                    ))
                    .color(Color::from_rgba(220, 220, 220, 120)), // subtle, monochrome, semi-transparent
            );
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

        // Dash flash — cyan burst when Space is pressed
        if self.dash_flash > 0.0 {
            let alpha = (self.dash_flash * 130.0) as u8;
            let flash = Mesh::new_rectangle(
                ctx,
                ggez::graphics::DrawMode::fill(),
                Rect::new(0.0, 0.0, width, height),
                Color::from_rgba(220, 240, 255, alpha),
            )?;
            canvas.draw(&flash, DrawParam::default());
        }

        // On-beat catch flash
        if self.on_beat_flash > 0.0 {
            let fa = (self.on_beat_flash * 180.0) as u8;
            let flash = Mesh::new_rectangle(
                ctx,
                ggez::graphics::DrawMode::fill(),
                Rect::new(0.0, 0.0, width, height),
                Color::from_rgba(255, 220, 80, fa),
            )?;
            canvas.draw(&flash, DrawParam::default());
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
                draw_crab(ctx, canvas, crab, pos, crab_beat, crab.join_pulse, 0.0)?;
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
                draw_crab(ctx, canvas, crab, crab.pos + Vec2::new(sway, bob), chain_beat, crab.join_pulse, lift)?;
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

        // (key, icon, name, description, r, g, b)
        let cards: &[(&str, &str, &str, &str, u8, u8, u8)] = &[
            ("1", ">",  "Wider Cone",   "Flashlight sweeps\na broader arc",       255, 200,  40),
            ("2", "~",  "Longer Range", "Flashlight reaches\nfurther ahead",       80, 160, 255),
            ("3", "*",  "Disco Laser",  "Add another\nrainbow beam",              200,  60, 255),
            ("4", "O",  "Chain Reach",  "Catch crabs from\nfurther with chain",    60, 220, 100),
        ];

        let card_w = 242.0_f32;
        let card_h = 310.0_f32;
        let gap    = 18.0_f32;
        let total_w = cards.len() as f32 * card_w + (cards.len() - 1) as f32 * gap;
        let x0 = (w - total_w) / 2.0;
        let y0 = (h - card_h) / 2.0 + 15.0;

        for (i, &(key, icon, name, desc, r, g, b)) in cards.iter().enumerate() {
            let cx = x0 + i as f32 * (card_w + gap);
            let hovered = self.mouse_pos.x >= cx && self.mouse_pos.x <= cx + card_w
                && self.mouse_pos.y >= y0 && self.mouse_pos.y <= y0 + card_h;

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
        if self.show_instructions || self.game_over || self.pending_upgrade {
            return Ok(());
        }

        let dt = ctx.time.delta().as_secs_f32();
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
        }
        self.beat_intensity = (self.beat_intensity - dt * 5.0).max(0.0);

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

        // Music intensity increases with score
        let target_intensity = (self.score as f32 / 30.0).min(1.0);
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
                        .filter(|(_, c)| !c.caught && tip.distance(c.pos) < 60.0)
                        .map(|(i, _)| i)
                        .collect();
                    let mut rng = rand::rng();
                    for i in to_catch {
                        let pos = self.crabs[i].pos;
                        let crab_type = self.crabs[i].crab_type;
                        let crab_color = self.crabs[i].crab_color();
                        self.particle_system.spawn_catch_effect(pos, crab_color, crab_type, &mut rng);
                        self.crabs[i].caught = true;
                        self.chain_join_ripple = true;
                        self.crabs[i].chain_index = Some(self.chain_count);
                        self.chain_count += 1;
                        self.check_milestone(&mut rand::rng());
                        self.score += self.combo_multiplier();
                        self.shake_timer = 0.15;
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
        canvas.set_screen_coordinates(Rect::new(shake_ox, shake_oy, width, height));
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
        if self.game_over || self.show_instructions || self.pending_upgrade {
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
        .window_mode(WindowMode::default().fullscreen_type(ggez::conf::FullscreenType::Desktop))
        .build()?;
    let state = MainState::new(&mut ctx)?;
    event::run(ctx, event_loop, state)
}
