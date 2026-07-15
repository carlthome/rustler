use crate::enemies::{BossCharge, CrabType, EnemyCrab};
use crate::tutorial::TutorialKind;
use ggez::glam::Vec2;
use rand::Rng;

#[derive(Clone)]
pub enum SpawnPattern {
    UniformRandom,
    SineWave,
    Circle,
    Cluster,
    SingleRandom,
    BeatGrid, // crabs arranged in a grid that pulses
    Spiral,   // crabs laid out in a spiral
}

fn make_crab(
    pos: Vec2,
    vel: Vec2,
    spawn_time: f32,
    emphasis: Option<CrabType>,
    rng: &mut impl Rng,
) -> EnemyCrab {
    let crab_type = CrabType::random_emphasized(emphasis, rng);
    let speed = rng.random_range(crab_type.speed_range());
    let scale = rng.random_range(crab_type.scale_range());
    EnemyCrab {
        pos,
        vel,
        speed,
        caught: false,
        chain_index: None,
        scale,
        spawn_time,
        crab_type,
        spooked_timer: 0.0,
        beat_phase_offset: rng.random_range(0.0..std::f32::consts::TAU),
        join_pulse: 0.0,
        fleeing: false,
        facing_angle: 0.0,
        in_flashlight: false,
        startle_timer: 0.0,
        charm_timer: 0.0,
        answering_call: 0.0,
        // Armored crabs spawn with a shell here; every other herd type gets 0. The Boss's larger
        // health is overwritten explicitly in spawn_boss after this returns.
        boss_health: crab_type.initial_shell(),
        boss_max_health: crab_type.initial_shell().max(0.0001),
        enraged: false,
        charge_state: BossCharge::Idle,
        charge_cooldown: 0.0,
        stun_timer: 0.0,
        latch_timer: 0.0,
        panic_amp: 1.0,
        magnet_snared: 0.0,
        magnet_lured: 0.0,
        thief_lured: 0.0,
        magnet_charged: 0.0,
        slingshot_spent: 0.0,
        // Hermits stagger their first host-swap so a cluster of them doesn't all dart on the same
        // frame. Non-Hermits never read this field. A short-ish irregular window keeps the shelled
        // Hermit visibly restless without teleporting so fast it's impossible to line up a crack.
        host_swap_timer: rng.random_range(1.6..3.2),
        surge_timer: 0.0,
    }
}

/// Spawn a rare "King Crab" boss. It enters from a random screen edge, lumbers toward the
/// play area, and carries `max_health` — the player must hold the flashlight on it to wear it
/// down before it can be caught. Not part of the normal spawn patterns; triggered on score.
pub fn spawn_boss(area: (f32, f32), rng: &mut impl Rng, max_health: f32) -> EnemyCrab {
    let (width, height) = area;
    // Pick a spot on a ring around screen center so the boss makes a visible entrance.
    let angle = rng.random_range(0.0..std::f32::consts::TAU);
    let radius = width.min(height) * 0.42;
    let center = Vec2::new(width * 0.5, height * 0.5);
    let pos = center + Vec2::new(angle.cos(), angle.sin()) * radius;
    // Amble roughly toward the middle of the arena.
    let vel = (center - pos).normalize_or_zero();
    let mut boss = make_crab(pos, vel, 0.0, None, rng);
    boss.crab_type = CrabType::Boss;
    boss.speed = rng.random_range(CrabType::Boss.speed_range());
    boss.scale = rng.random_range(CrabType::Boss.scale_range());
    boss.boss_health = max_health;
    boss.boss_max_health = max_health;
    // Let it make its lumbering entrance before the first charge winds up.
    boss.charge_cooldown = 2.5;
    boss
}

/// Spawn a rare "Tide Boss". Like the King Crab it enters from a ring around center and must be
/// worn down under the flashlight, but instead of charging it drifts and periodically emits an
/// expanding shockwave pulse (see the tide branch in main.rs) that scatters nearby free crabs and
/// knocks loose the tail of any train that's clustered too close — a spacing threat, not a lane one.
pub fn spawn_tide_boss(area: (f32, f32), rng: &mut impl Rng, max_health: f32) -> EnemyCrab {
    let (width, height) = area;
    let angle = rng.random_range(0.0..std::f32::consts::TAU);
    let radius = width.min(height) * 0.42;
    let center = Vec2::new(width * 0.5, height * 0.5);
    let pos = center + Vec2::new(angle.cos(), angle.sin()) * radius;
    let vel = (center - pos).normalize_or_zero();
    let mut boss = make_crab(pos, vel, 0.0, None, rng);
    boss.crab_type = CrabType::TideBoss;
    boss.speed = rng.random_range(CrabType::TideBoss.speed_range());
    boss.scale = rng.random_range(CrabType::TideBoss.scale_range());
    boss.boss_health = max_health;
    boss.boss_max_health = max_health;
    // charge_cooldown doubles as the pulse timer for the Tide Boss — let it drift in first.
    boss.charge_cooldown = 3.0;
    boss
}

/// Spawn a rare "Reef DJ" rhythm boss. Like the other bosses it enters from a ring around center
/// and must be worn down under the flashlight, but its shell is beat-locked: the beam only drains
/// its health while the on-beat window is open (see the rhythm-boss branch in main.rs). It drifts
/// steadily toward the train's heart but never charges or pulses — the whole fight is a timing test.
pub fn spawn_rhythm_boss(area: (f32, f32), rng: &mut impl Rng, max_health: f32) -> EnemyCrab {
    let (width, height) = area;
    let angle = rng.random_range(0.0..std::f32::consts::TAU);
    let radius = width.min(height) * 0.42;
    let center = Vec2::new(width * 0.5, height * 0.5);
    let pos = center + Vec2::new(angle.cos(), angle.sin()) * radius;
    let vel = (center - pos).normalize_or_zero();
    let mut boss = make_crab(pos, vel, 0.0, None, rng);
    boss.crab_type = CrabType::RhythmBoss;
    boss.speed = rng.random_range(CrabType::RhythmBoss.speed_range());
    boss.scale = rng.random_range(CrabType::RhythmBoss.scale_range());
    boss.boss_health = max_health;
    boss.boss_max_health = max_health;
    boss
}

/// Spawn a single "hype Dancer" for the Reef DJ fight. The rhythm boss clears the herd for a
/// clean duel, which normally silences the whole archetype web — this brings one archetype back
/// into the arena as a fight mechanic. It's a normal Dancer (drifts between beats, hops on the
/// beat) forced to spawn near the boss, but catching one *on a called (hot) beat* chips the DJ's
/// shell (see the catch loop in main.rs). So the boss's own backup dancers become ammunition:
/// herd them onto the hot beat and snap them up to help crack the shell faster than light alone.
pub fn spawn_hype_dancer(area: (f32, f32), boss_pos: Vec2, rng: &mut impl Rng) -> EnemyCrab {
    let (width, height) = area;
    // Ring out from the boss so the dancer reads as *its* summon, not a stray herd crab.
    let angle = rng.random_range(0.0..std::f32::consts::TAU);
    let dist = rng.random_range(80.0..160.0);
    let pos = (boss_pos + Vec2::new(angle.cos(), angle.sin()) * dist).clamp(
        Vec2::splat(20.0),
        Vec2::new(width - 20.0, height - 20.0),
    );
    let vel = Vec2::new(angle.cos(), angle.sin());
    let mut crab = make_crab(pos, vel, 0.0, None, rng);
    crab.crab_type = CrabType::Dancer;
    crab.speed = rng.random_range(CrabType::Dancer.speed_range());
    crab.scale = rng.random_range(CrabType::Dancer.scale_range());
    crab.boss_health = 0.0;
    crab.boss_max_health = 0.0001;
    crab
}

/// Spawn a small, calm set of plain Normal crabs for the "How to Play" tutorial sandbox. Forced
/// to `CrabType::Normal` (no Armored/Dancer/Golden wrinkles) and laid out in a gentle ring around
/// the arena center so the beat-timing lesson isn't muddied by any archetype behaviour. The player
/// starts in the middle, so every crab is a short, unhurried stroll away.
pub fn spawn_tutorial_crabs(
    kind: TutorialKind,
    count: usize,
    area: (f32, f32),
    rng: &mut impl Rng,
) -> Vec<EnemyCrab> {
    let (width, height) = area;
    let center = Vec2::new(width * 0.5, height * 0.5);
    // The LassoGrab lesson wants crabs out of walking reach so the learner has to fling the rope
    // rather than stroll onto them — push them out to a wide ring. Every other scenario keeps the
    // calm mid-ring where crabs are a short, unhurried stroll away.
    let radius = match kind {
        TutorialKind::LassoGrab => width.min(height) * 0.42,
        _ => width.min(height) * 0.28,
    };
    // The ShellCrack lesson needs a hard target the beam can't clear, so it spawns Armored crabs
    // (shell HP from initial_shell) that the learner must Stomp open. Every other scenario uses a
    // calm ring of shell-less Normals easy to intercept on the beat or chain into a train.
    let crab_type = match kind {
        TutorialKind::ShellCrack => CrabType::Armored,
        _ => CrabType::Normal,
    };
    (0..count)
        .map(|i| {
            let angle = std::f32::consts::TAU * (i as f32 + 0.5) / count as f32;
            let pos = center + Vec2::new(angle.cos(), angle.sin()) * radius;
            // Drift slowly so they read as alive but stay easy to intercept on the beat.
            let vel = Vec2::new(angle.cos(), angle.sin()) * 0.2;
            let mut crab = make_crab(pos, vel, 0.0, None, rng);
            crab.crab_type = crab_type;
            crab.speed = 30.0;
            crab.scale = 1.0;
            // Armored tutorial crabs carry a shell to crack; everything else is shell-less.
            let shell = crab_type.initial_shell();
            crab.boss_health = shell;
            crab.boss_max_health = shell.max(0.0001);
            crab
        })
        .collect()
}

pub fn spawn_enemies(
    pattern: SpawnPattern,
    count: usize,
    area: (f32, f32),
    centroid: (f32, f32),
    emphasis: Option<CrabType>,
    rng: &mut impl Rng,
) -> Vec<EnemyCrab> {
    let (width, height) = area;
    let centroid_vec = Vec2::from(centroid) * Vec2::from(area);
    match pattern {
        SpawnPattern::UniformRandom => (0..count)
            .map(|_| {
                let pos = centroid_vec
                    + Vec2::new(
                        rng.random_range(-width * 0.3..width * 0.3),
                        rng.random_range(-height * 0.3..height * 0.3),
                    );
                let angle = rng.random_range(0.0..std::f32::consts::TAU);
                let vel = Vec2::new(angle.cos(), angle.sin());
                make_crab(pos, vel, 0.0, emphasis, rng)
            })
            .collect(),
        SpawnPattern::SineWave => {
            let amplitude = height * 0.3;
            let freq = 2.0 * std::f32::consts::PI / width;
            (0..count)
                .map(|i| {
                    let x = centroid_vec.x + width * (i as f32 + 0.5) / count as f32 * 0.5;
                    let y = centroid_vec.y + amplitude * (freq * x).sin();
                    let pos = Vec2::new(x, y);
                    let angle = std::f32::consts::FRAC_PI_2;
                    let vel = Vec2::new(angle.cos(), angle.sin());
                    make_crab(pos, vel, 0.0, emphasis, rng)
                })
                .collect()
        }
        SpawnPattern::Circle => {
            let center = centroid_vec;
            let radius = width.min(height) * 0.35;
            (0..count)
                .map(|i| {
                    let angle = i as f32 * std::f32::consts::TAU / count as f32;
                    let pos = center + Vec2::new(angle.cos(), angle.sin()) * radius;
                    let vel = Vec2::new(angle.cos(), angle.sin());
                    make_crab(pos, vel, 0.0, emphasis, rng)
                })
                .collect()
        }
        SpawnPattern::Cluster => {
            let cluster_center = centroid_vec;
            (0..count)
                .map(|_| {
                    let angle = rng.random_range(0.0..std::f32::consts::TAU);
                    let dist = rng.random_range(0.0..(width.min(height) * 0.1));
                    let pos = cluster_center + Vec2::new(angle.cos(), angle.sin()) * dist;
                    let vel = Vec2::new(angle.cos(), angle.sin());
                    make_crab(pos, vel, 0.0, emphasis, rng)
                })
                .collect()
        }
        SpawnPattern::SingleRandom => {
            let count = count.max(1);
            let delay = 0.5;
            (0..count)
                .map(|i| {
                    let angle = rng.random_range(0.0..std::f32::consts::TAU);
                    let vel = Vec2::new(angle.cos(), angle.sin());
                    let pos = centroid_vec
                        + Vec2::new(rng.random_range(-50.0..50.0), rng.random_range(-50.0..50.0));
                    make_crab(pos, vel, i as f32 * delay, emphasis, rng)
                })
                .collect()
        }
        SpawnPattern::BeatGrid => {
            let cols = ((count as f32).sqrt().ceil() as usize).max(1);
            let rows = (count + cols - 1) / cols;
            let spacing_x = width * 0.12;
            let spacing_y = height * 0.10;
            (0..count)
                .map(|i| {
                    let col = (i % cols) as f32 - (cols as f32 - 1.0) / 2.0;
                    let row = (i / cols) as f32 - (rows as f32 - 1.0) / 2.0;
                    let pos = centroid_vec + Vec2::new(col * spacing_x, row * spacing_y);
                    let angle = rng.random_range(0.0..std::f32::consts::TAU);
                    let vel = Vec2::new(angle.cos(), angle.sin());
                    make_crab(pos, vel, 0.0, emphasis, rng)
                })
                .collect()
        }
        SpawnPattern::Spiral => (0..count)
            .map(|i| {
                let t = i as f32 / count.max(1) as f32;
                let angle = t * std::f32::consts::TAU * 2.5;
                let radius = t * width.min(height) * 0.38;
                let pos = centroid_vec + Vec2::new(angle.cos() * radius, angle.sin() * radius);
                let vel = Vec2::new(-angle.sin(), angle.cos()); // tangent direction
                make_crab(pos, vel, 0.0, emphasis, rng)
            })
            .collect(),
    }
}
