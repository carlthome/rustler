use ggez::glam::Vec2;

pub const PLAYER_SIZE: f32 = 48.0;
pub const CRAB_SIZE: f32 = 36.0;
pub const SPEED: f32 = 200.0;

pub const CHAIN_LINK_FRAMES: usize = 12;
pub const BEAT_INTERVAL: f32 = 0.5; // 120 BPM, crab rave tempo
pub const BEAT_WINDOW: f32 = 0.08;  // seconds around a beat that count as "on beat"
pub const PERFECT_WINDOW: f32 = 0.032; // seconds around a beat that count as a PERFECT hit (tighter than BEAT_WINDOW)

pub const DRUM_ROLL_MAX: u32 = 4;

pub const INTENSITY_STAGES: &[(f32, &str, f32, f32)] = &[
    (0.0, "WARM-UP", 1.0, 1.0),
    (45.0, "BUILDING", 1.25, 1.08),
    (100.0, "HEATED", 1.55, 1.16),
    (170.0, "FEVER", 1.9, 1.26),
    (260.0, "OVERDRIVE", 2.3, 1.38),
];

pub const STAGE_DURATION_SCALE: f32 = 0.92;
pub const STAGE_DURATION_FLOOR: f32 = 0.6;
pub const BOSS_MAX_HEALTH: f32 = 3.0;
pub const BOSS_DRAIN_RATE: f32 = 1.0;
pub const BOSS_SCORE_INTERVAL: usize = 40;
pub const BOSS_CHARGE_COOLDOWN: f32 = 4.5;
pub const BOSS_WINDUP_TIME: f32 = 0.85;
pub const BOSS_CHARGE_TIME: f32 = 0.65;
pub const BOSS_CHARGE_SPEED: f32 = 540.0;
pub const BOSS_CHARGE_ARM_RANGE: f32 = 430.0;
pub const BOSS_ENRAGE_THRESHOLD: f32 = 0.4;
pub const BOSS_ENRAGE_COOLDOWN_SCALE: f32 = 0.5;
pub const BOSS_ENRAGE_CHARGE_SPEED_SCALE: f32 = 1.25;
pub const BOSS_STUN_DURATION: f32 = 1.6;
pub const TIDE_PULSE_COOLDOWN: f32 = 5.0;
pub const TIDE_PULSE_WINDUP: f32 = 1.0;
pub const TIDE_PULSE_RADIUS: f32 = 320.0;
pub const TIDE_PULSE_EXPAND_SPEED: f32 = 900.0;
pub const WHISTLE_COOLDOWN: f32 = 4.5;
pub const WHISTLE_RING_SPEED: f32 = 1000.0;
pub const WHISTLE_MAX_RADIUS: f32 = 360.0;
pub const WHISTLE_PULL_SPEED: f32 = 240.0;
pub const STOMP_COOLDOWN: f32 = 3.0;
pub const STOMP_RING_SPEED: f32 = 900.0;
pub const STOMP_MAX_RADIUS: f32 = 155.0;
pub const PEN_RADIUS: f32 = 90.0;
pub const TIDE_CURRENT_DIR: Vec2 = Vec2::new(0.6, 0.8);
pub const TIDE_CURRENT_STRENGTH: f32 = 46.0;
pub const KELP_FUNNEL_DIR: Vec2 = Vec2::new(0.97, -0.24);
pub const KELP_FUNNEL_STRENGTH: f32 = 58.0;
pub const ROCK_SUBMERGE_LEVEL: f32 = 0.55;
pub const ROCK_TIDE_EASE: f32 = 3.2;
pub const SLAM_RADIUS: f32 = 480.0;
pub const SLAM_RING_SPEED: f32 = 1400.0;
pub const SLOWMO_DURATION: f32 = 0.45;
pub const DELIVER_STREAK_GRACE: f32 = 14.0;
pub const DELIVER_STREAK_MAX: u32 = 8;
pub const PERFECT_DELIVERY_BONUS: f32 = 0.5;
pub const BOND_PAIR_BONUS: usize = 12;
pub const SANDWICH_BONUS: usize = 20;
pub const RUN_STREAK_BONUS: usize = 15;

pub const MAX_START_RANK: u32 = 2;
pub const PERK_COST_STEP: usize = 30;
