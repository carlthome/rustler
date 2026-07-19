use ggez::glam::Vec2;

pub const PLAYER_SIZE: f32 = 48.0;

// Lasso skill-shot tuning. The loop flies out to the (range-clamped) aim point over
// LASSO_THROW_TIME — a real throw with travel time, so crabs can dodge the path. On landing it
// pauses briefly to tighten on a catch (LASSO_SNAG_TIME — the squeeze/pop), then reels crabs back
// in over LASSO_DRAG_TIME with visible rope tension. An empty throw flops down over LASSO_MISS_TIME
// with a dust puff. LASSO_MAX_RANGE is the fixed reach so near/far throws share a consistent speed.
pub const LASSO_MAX_RANGE: f32 = 340.0;
pub const LASSO_THROW_TIME: f32 = 0.15;
pub const LASSO_SNAG_TIME: f32 = 0.08;
pub const LASSO_DRAG_TIME: f32 = 0.22;
pub const LASSO_MISS_TIME: f32 = 0.18;
// Charge-throw tuning. The player holds the mouse button to wind up; LASSO_MAX_CHARGE_TIME is the
// full-charge cap (beyond which holding longer doesn't help). A quick tap (below MIN_THROW_FRAC of
// the cap) fires a weak short throw; full charge reaches the full LASSO_MAX_RANGE at max speed.
// LASSO_MIN_RANGE_FRAC is the fraction of max range a tap-throw reaches (so even a quick flick
// still does something). LASSO_ONBEAT_BONUS is the range×speed multiplier when the release lands
// on the beat — releasing in the pocket gives extra reach, deepening the rhythm layer.
pub const LASSO_MAX_CHARGE_TIME: f32 = 1.2;
pub const LASSO_MIN_RANGE_FRAC: f32 = 0.28; // quick tap reaches 28% of max range
pub const LASSO_ONBEAT_BONUS: f32 = 1.35; // 35% extra range+speed when released on-beat

pub const CRAB_SIZE: f32 = 36.0;
// Universal crab velocity cap — prevents runaway speed from compounding forces (wall
// bounces, scatter kicks, lasso drag) from producing visually broken teleport-level movement.
// 600 px/s is well above any intentional top speed (boss charge is 540, scatter kicks ~280–300)
// so this only fires on genuinely broken compounding, never on normal fast movement.
pub const MAX_CRAB_SPEED: f32 = 600.0;
pub const SPEED: f32 = 200.0;
pub const SPRINT_STAMINA_MAX: f32 = 6.0;
pub const SPRINT_STAMINA_DRAIN_PER_SEC: f32 = 0.85;
pub const SPRINT_STAMINA_REGEN_PER_SEC: f32 = 0.55;
pub const SPRINT_SPEED_MULT: f32 = 10.0;

pub const CHAIN_LINK_FRAMES: usize = 12;
pub const BEAT_INTERVAL: f32 = 0.5; // 120 BPM, crab rave tempo
pub const BEAT_WINDOW: f32 = 0.08; // seconds around a beat that count as "on beat"
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
// Steal telegraph fuse (seconds): how long a rival's armed splice trembles before it snaps.
// Shared by update_npc_trains (arming) and the bot defense test so the two can't drift apart.
pub const STEAL_FUSE: f32 = 0.55;
// Defensive parry reach: how close a rival leader must be to a tool cast to be repelled. Stomp is
// the up-close bodyguard (short, punchy); the Beat Wave is the wide ranged save. This is the tool
// identity — Stomp defends what's on top of you, the Wave sweeps a threat off from across the lane.
pub const STOMP_DEFEND_RADIUS: f32 = 175.0;
pub const WAVE_DEFEND_RADIUS: f32 = 300.0;
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
// CENTERPIECE: a same-type run of length >= 3 that straddles the train's midpoint pays this
// once. Positional identity for the MIDDLE of the train — a deep run is worth more parked in
// the protected center (safe from tail snaps) than dangling at the snappable tail, so WHERE you
// seat your best run matters, not just that you built one. Deepens the existing run vocabulary.
pub const CENTERPIECE_BONUS: usize = 40;

pub const MAX_START_RANK: u32 = 2;
pub const PERK_COST_STEP: usize = 30;
