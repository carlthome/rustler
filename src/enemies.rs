use ggez::glam::Vec2;

/// King Crab charge state machine. Only the Boss archetype ever leaves `Idle`: it roams toward
/// the conga train, `Winding` up a telegraphed charge, then `Charging` in a locked direction that
/// scatters the tail of the train on contact. Every other crab stays `Idle` forever.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BossCharge {
    Idle,          // roaming (or not a boss)
    Winding(f32),  // telegraphing the charge; f32 = seconds of wind-up remaining
    Charging(f32), // lunging along a locked heading; f32 = seconds of lunge remaining
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CrabType {
    Normal,
    Fast,
    Big,
    Sneaky,
    Armored, // hard-shelled: lasso slips off and the whistle barely moves it — crack it with a Stomp
    Dancer,  // rhythm crab: freezes between beats, then lunges a fixed hop on the beat — catch it mid-freeze
    Boss,    // rare oversized "King Crab" — never spawns randomly, only via the boss trigger
}

impl CrabType {
    pub fn random(rng: &mut impl rand::Rng) -> Self {
        use CrabType::*;
        // Deliberately excludes Boss — bosses are spawned explicitly, not by the herd roll.
        // Armored crabs are the rarest of the herd (~10%) so they punctuate a run rather than
        // flooding it — enough to make you reach for the Stomp, not so many they gate every catch.
        // Dancer crabs are an uncommon rhythm-flavored catch (~10%) — enough to make a beat-timed
        // grab a recurring skill test without the herd turning into a strobe of hopping crabs.
        match rng.random_range(0..11) {
            0 | 1 | 2 => Normal,
            3 | 4 => Fast,
            5 | 6 => Big,
            7 | 8 => Sneaky,
            9 => Armored,
            _ => Dancer,
        }
    }
    pub fn speed_range(&self) -> std::ops::Range<f32> {
        match self {
            CrabType::Normal => 30.0..70.0,
            CrabType::Fast => 60.0..120.0,
            CrabType::Big => 20.0..40.0,
            CrabType::Sneaky => 40.0..80.0,
            CrabType::Armored => 22.0..42.0, // heavy shell — trundles along
            CrabType::Dancer => 20.0..40.0,  // drifts slowly between beats; its real speed is the beat hop
            CrabType::Boss => 18.0..34.0,    // slow and lumbering
        }
    }
    /// Shell health an archetype spawns with. While a crab's shell (stored in `boss_health`) is
    /// above zero it can't be lassoed or grabbed by the chain — the beam wears it down slowly, a
    /// Stomp cracks it instantly. Only Armored crabs carry a shell from the herd roll (the Boss
    /// gets its own, larger health set explicitly at spawn).
    pub fn initial_shell(&self) -> f32 {
        match self {
            CrabType::Armored => 2.0,
            _ => 0.0,
        }
    }
    /// How strongly the Whistle ability yanks this crab toward the player — a soft counter, not a
    /// hard requirement. Every archetype still moves at least a little (nothing is whistle-immune
    /// except the boss), but the whistle is *the* tool for gathering skittish Sneaky crabs, while
    /// heavy Big crabs barely budge and are better handled with the lasso/flashlight.
    pub fn whistle_pull(&self) -> f32 {
        match self {
            CrabType::Sneaky => 1.5, // evasive and light — folds hard to a whistle
            CrabType::Normal => 1.0,
            CrabType::Fast => 0.85, // squirrely, harder to herd cleanly
            CrabType::Big => 0.4,   // heavy — shrugs most of it off
            CrabType::Armored => 0.3, // shelled and stubborn — the whistle barely nudges it
            CrabType::Dancer => 1.2, // light and lively — the whistle catches it easily between hops
            CrabType::Boss => 0.0,  // the King Crab is unshakeable
        }
    }

    pub fn scale_range(&self) -> std::ops::RangeInclusive<f32> {
        match self {
            CrabType::Normal => 0.28..=0.48,
            CrabType::Fast => 0.24..=0.36,
            CrabType::Big => 0.50..=0.80,
            CrabType::Sneaky => 0.30..=0.40,
            CrabType::Armored => 0.42..=0.62, // stocky, tank-like
            CrabType::Dancer => 0.30..=0.44,  // sprightly, mid-size
            CrabType::Boss => 1.7..=2.1,      // towering
        }
    }
}

#[derive(Clone, Debug)]
pub struct EnemyCrab {
    pub pos: Vec2,
    pub vel: Vec2,
    pub speed: f32,
    pub caught: bool,
    pub chain_index: Option<usize>,
    pub scale: f32,
    pub spawn_time: f32,
    pub crab_type: CrabType,
    pub spooked_timer: f32,
    pub beat_phase_offset: f32,
    pub join_pulse: f32,
    pub fleeing: bool,  // true when actively panic-fleeing from the player
    pub facing_angle: f32,  // current facing direction in radians (0 = right)
    pub in_flashlight: bool, // true while crab is inside the flashlight cone being attracted
    pub startle_timer: f32,  // >0 while bolting away after a nearby catch spooked it (stampede ripple)
    pub charm_timer: f32,    // >0 while soothed by a whistle pulse: won't flee and is immune to beat-startle panic
    pub boss_health: f32,    // >0 while a boss still needs wearing down under the beam; 0 for regular crabs
    pub charge_state: BossCharge, // King Crab charge phase; always Idle for the herd
    pub charge_cooldown: f32,     // seconds until a roaming boss may wind up its next charge
}

impl EnemyCrab {
    pub fn crab_color(&self) -> [f32; 3] {
        let t = (self.spawn_time / 10.0).min(1.0);
        match self.crab_type {
            CrabType::Normal => [
                0.6 + 0.4 * t,
                100.0 / 255.0 * (1.0 - t),
                100.0 / 255.0 * (1.0 - t),
            ],
            CrabType::Fast => [1.0, 180.0 / 255.0 * (1.0 - t), 40.0 / 255.0],
            CrabType::Big => [180.0 / 255.0, 60.0 / 255.0, 180.0 / 255.0 * (1.0 - t)],
            CrabType::Sneaky => [120.0 / 255.0, 220.0 / 255.0, 220.0 / 255.0],
            CrabType::Armored => [0.52 + 0.18 * t, 0.58, 0.66], // cold steely slate-blue shell
            CrabType::Dancer => [1.0, 0.35 + 0.25 * t, 0.85],   // hot disco magenta-pink
            CrabType::Boss => [0.96, 0.72, 0.16], // regal king-crab gold
        }
    }

    /// A boss "King Crab" — oversized, must be worn down under the flashlight before it can be caught.
    pub fn is_boss(&self) -> bool {
        matches!(self.crab_type, CrabType::Boss)
    }

    /// A hard-shelled crab: its shell (stored in `boss_health`) must be cracked — by a Stomp
    /// (instant) or worn down under the beam (slow) — before the lasso or chain can grab it.
    pub fn is_armored(&self) -> bool {
        matches!(self.crab_type, CrabType::Armored)
    }

    /// A rhythm "Dancer" crab: it drifts slowly between beats and takes a sharp hop on each beat
    /// (see the beat-fire block in main.rs). Catch it during the freeze, not mid-leap.
    pub fn is_dancer(&self) -> bool {
        matches!(self.crab_type, CrabType::Dancer)
    }

    /// Whether the crab can be snagged this frame. Regular crabs are catchable whenever free;
    /// a boss is only catchable once its health has been drained to zero by holding the beam on it.
    pub fn is_catchable(&self) -> bool {
        !self.caught && self.boss_health <= 0.0
    }
}
