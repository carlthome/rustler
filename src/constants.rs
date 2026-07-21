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
// Pirate treasure is an occasional high-value rhythm opportunity. A roll happens once per
// interval while no chest is active, so the average wait is roughly a minute and a half.
pub const TREASURE_CHEST_ROLL_INTERVAL: f32 = 18.0;
pub const TREASURE_CHEST_SPAWN_CHANCE: f64 = 0.2;
pub const TREASURE_CHEST_PICKUP_RADIUS: f32 = 44.0;
pub const TREASURE_CHEST_SPAWN_MARGIN: f32 = 80.0;
pub const TREASURE_CHEST_MIN_SPAWN_DISTANCE: f32 = 320.0;
pub const TREASURE_CHEST_SPAWN_ATTEMPTS: usize = 12;
pub const TREASURE_CHEST_ON_BEAT_PARTICLES: usize = 20;
pub const TREASURE_CHEST_OFF_BEAT_PARTICLES: usize = 10;
pub const PERFECT_WINDOW: f32 = 0.032; // seconds around a beat that count as a PERFECT hit (tighter than BEAT_WINDOW)
// Wider on-beat window for the *defensive* steal parry only (Stomp/Wave save against an armed
// rival splice). The parry is reactive — you're tracking the rival's telegraph AND the beat at
// once — so it earns more forgiveness than a proactive verb like the dash, which keeps its tight
// BEAT_WINDOW feel. This cuts the "my save was 20 ms late so I lost my tail" false-negatives
// (Carl's feedback: on-beat defense felt too unforgiving) without trivializing it — the window is
// still short enough that a clean on-beat read pays and a lazy mash misses.
pub const DEFEND_BEAT_WINDOW: f32 = 0.12;
// On-beat window for the *proactive ranged tool casts* — whistle / stomp / beat-wave / lasso. These
// are cooldown-gated verbs you fire far less often than you dash, so a missed on-beat cast stings
// more and reads as "the timing is unforgiving" (Carl's #164 feedback). They earn a touch more
// forgiveness than the tight BEAT_WINDOW the dash and catch keep — enough that a slightly-early/late
// cast still reads on-beat — without trivializing it: still short of the reactive DEFEND window, and
// the PERFECT catch sub-window stays tight so precision still pays. The forgiveness ladder is
// PERFECT(0.032) < dash/catch BEAT_WINDOW(0.08) < ranged tools(0.11) < reactive parry DEFEND(0.12).
// The dash deliberately keeps the tight BEAT_WINDOW (Carl: "the dash feels good — do NOT touch it").
pub const ACTION_BEAT_WINDOW: f32 = 0.11;

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
// Cap on how many crabs a single rival splice can rustle away. A steal takes at most this many
// links off the tail (and never more than half the chain — see update_npc_trains), so losing
// crabs stays a recoverable bite you can steal back, not a one-hit train-wipe. This is the
// "tune so it's fun, not punishing" lever (ROADMAP steal headline): the loop should feel like a
// tense back-and-forth, not a random tax. Shared with the npc_steal bot test so the cap can't
// silently regress.
pub const STEAL_MAX_LINKS: usize = 5;
// Revenge window (seconds): after a rival splices your tail it's marked as a revenge target for
// this long. Rustling crabs back off a still-marked rival inside the window pays a bonus (extra
// score + groove, a distinct "GOT 'EM BACK!" cue) and a beat-pulsed marker rings the culprit so
// you know which rival to chase. This is what turns the steal from two disconnected verbs into a
// back-and-forth duel (ROADMAP steal headline: "you steal, they steal back", not a random tax).
pub const REVENGE_WINDOW: f32 = 6.0;
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
