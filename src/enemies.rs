use ggez::glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CrabType {
    Normal,
    Fast,
    Big,
    Sneaky,
    Boss, // rare oversized "King Crab" — never spawns randomly, only via the boss trigger
}

impl CrabType {
    pub fn random(rng: &mut impl rand::Rng) -> Self {
        use CrabType::*;
        // Deliberately excludes Boss — bosses are spawned explicitly, not by the herd roll.
        match rng.random_range(0..4) {
            0 => Normal,
            1 => Fast,
            2 => Big,
            _ => Sneaky,
        }
    }
    pub fn speed_range(&self) -> std::ops::Range<f32> {
        match self {
            CrabType::Normal => 30.0..70.0,
            CrabType::Fast => 60.0..120.0,
            CrabType::Big => 20.0..40.0,
            CrabType::Sneaky => 40.0..80.0,
            CrabType::Boss => 18.0..34.0, // slow and lumbering
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
            CrabType::Big => 0.4,   // armored and heavy — shrugs most of it off
            CrabType::Boss => 0.0,  // the King Crab is unshakeable
        }
    }

    pub fn scale_range(&self) -> std::ops::RangeInclusive<f32> {
        match self {
            CrabType::Normal => 0.28..=0.48,
            CrabType::Fast => 0.24..=0.36,
            CrabType::Big => 0.50..=0.80,
            CrabType::Sneaky => 0.30..=0.40,
            CrabType::Boss => 1.7..=2.1, // towering
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
    pub boss_health: f32,    // >0 while a boss still needs wearing down under the beam; 0 for regular crabs
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
            CrabType::Boss => [0.96, 0.72, 0.16], // regal king-crab gold
        }
    }

    /// A boss "King Crab" — oversized, must be worn down under the flashlight before it can be caught.
    pub fn is_boss(&self) -> bool {
        matches!(self.crab_type, CrabType::Boss)
    }

    /// Whether the crab can be snagged this frame. Regular crabs are catchable whenever free;
    /// a boss is only catchable once its health has been drained to zero by holding the beam on it.
    pub fn is_catchable(&self) -> bool {
        !self.caught && self.boss_health <= 0.0
    }
}
