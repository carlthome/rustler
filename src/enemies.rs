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
    Magnet,  // draws nearby free crabs toward itself as it roams — catching it nets the cluster it gathered (two-for-one)
    Thief,   // skittish parasite: latches onto the conga tail and peels a link loose on a timer unless you catch/dislodge it — pressures the train you've already built
    Golden,  // rare shiny "Golden Crab" — flees fast and sparkles; catching it pays a big lump-sum score bonus. A pure risk/reward chase decision: break off your herding to snag it before it bolts, or let it go.
    Boss,    // rare oversized "King Crab" — never spawns randomly, only via the boss trigger
    TideBoss, // rare oversized "Tide Boss" — drifts and emits shockwave pulses that scatter the train
}

impl CrabType {
    pub fn random(rng: &mut impl rand::Rng) -> Self {
        use CrabType::*;
        // Deliberately excludes Boss — bosses are spawned explicitly, not by the herd roll.
        // Armored crabs are the rarest of the herd (~10%) so they punctuate a run rather than
        // flooding it — enough to make you reach for the Stomp, not so many they gate every catch.
        // Dancer crabs are an uncommon rhythm-flavored catch (~10%) — enough to make a beat-timed
        // grab a recurring skill test without the herd turning into a strobe of hopping crabs.
        // Magnet crabs are the rarest of the herd (~8%): each one reshapes routing by clustering
        // free crabs around it, so a couple per run is plenty of pull without the whole herd
        // collapsing into magnet-led blobs.
        // Thief crabs are rare too (~8%): each one latches onto the conga tail and peels links
        // loose, so a couple per run keeps you defending the train you built without the herd
        // constantly gnawing it apart.
        // Golden crabs are the rarest of the herd (~3%, 1 in ~33): a shiny high-value target that
        // shows up just often enough to be a delightful surprise and a real "chase it or not" call,
        // never so often it becomes the main way to score. Rolled first so its rarity is exact.
        if rng.random_range(0..33) == 0 {
            return Golden;
        }
        match rng.random_range(0..13) {
            0 | 1 => Normal,
            2 | 3 => Fast,
            4 => Big,
            5 | 6 => Sneaky,
            7 => Armored,
            8 | 9 => Dancer,
            10 | 11 => Magnet,
            _ => Thief,
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
            CrabType::Magnet => 26.0..48.0,  // roams steadily — you chase it because the herd trails it
            CrabType::Thief => 55.0..95.0,   // quick and darty — it makes a beeline for your tail
            CrabType::Golden => 85.0..135.0, // skittish and fast — the shiny prize bolts, so you have to commit to the chase
            CrabType::Boss => 18.0..34.0,    // slow and lumbering
            CrabType::TideBoss => 24.0..44.0, // roams a touch quicker, but never charges
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
            CrabType::Magnet => 0.9, // a touch heavy from all the crabs it's dragging along
            CrabType::Thief => 1.3,  // light and skittish — a whistle yanks it off your tail nicely
            CrabType::Golden => 1.6, // flighty featherweight — a whistle is the surest way to reel the shiny prize in before it bolts
            CrabType::Boss => 0.0,  // the King Crab is unshakeable
            CrabType::TideBoss => 0.0, // the Tide Boss is unshakeable
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
            CrabType::Magnet => 0.40..=0.56,  // chunky, so its aura reads at a glance
            CrabType::Thief => 0.26..=0.38,   // small and wiry — easy to lose against the herd until it's on your tail
            CrabType::Golden => 0.34..=0.48,  // a hair bigger than a normal crab so the shine reads at a glance
            CrabType::Boss => 1.7..=2.1,      // towering
            CrabType::TideBoss => 1.7..=2.1,  // just as towering as the King Crab
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
    pub answering_call: f32, // >0 while a Dancer is answering an on-beat player Call: it hops *toward* the player on the beat instead of fleeing
    pub boss_health: f32,    // >0 while a boss still needs wearing down under the beam; 0 for regular crabs
    pub boss_max_health: f32, // starting boss_health, so the fight can reason about health *fraction* (enrage phase)
    pub enraged: bool,        // latched true once a boss crosses into its final enrage phase — drives the one-shot telegraph
    pub charge_state: BossCharge, // King Crab charge phase; always Idle for the herd
    pub charge_cooldown: f32,     // seconds until a roaming boss may wind up its next charge
    pub latch_timer: f32,         // Thief only: >0 while clamped onto the conga tail, counts down to the next link it peels off
    pub panic_amp: f32,           // >=1.0 fear-ripple amplitude carried while startled: a fleeing Golden seeds this high so its panic bomb keeps rippling harder than baseline for a few beats
    pub magnet_snared: f32,       // Golden or Thief: >0 while a roaming Magnet's field has overpowered its movement and tethered it — for a Golden, the "grab the prize now" window; for a homing Thief, an interception that stops it reaching your tail. Counts down; refreshed each frame the crab stays deep in the field. Drives the snare visual + slowed movement.
    pub magnet_lured: f32,        // Magnet only: >0 while this roaming Magnet is being pulled off its cluster toward a nearby fleeing Golden — the shiny prize's shine luring the lodestone. Counts down; refreshed each frame it keeps chasing. Drives the aura shifting gold-ward.
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
            CrabType::Magnet => [0.95, 0.30 + 0.15 * t, 0.20],  // magnetic lodestone red-orange
            CrabType::Thief => [0.30, 0.85, 0.45 + 0.2 * t],    // sly poison-green — reads as "trouble" against the herd
            CrabType::Golden => [1.0, 0.86, 0.28],              // bright treasure-gold — the shiny prize pops against the whole herd
            CrabType::Boss => [0.96, 0.72, 0.16], // regal king-crab gold
            CrabType::TideBoss => [0.20, 0.68, 0.86], // deep tidal cyan-blue
        }
    }

    /// Any oversized boss — must be worn down under the flashlight before it can be caught. Covers
    /// both the charging King Crab and the pulsing Tide Boss, so all the shared boss plumbing
    /// (health ring, catchable-only-when-drained, unshakeable, non-fleeing) applies to both.
    pub fn is_boss(&self) -> bool {
        matches!(self.crab_type, CrabType::Boss | CrabType::TideBoss)
    }

    /// The charging "King Crab" boss specifically — the one that winds up and lunges at the train.
    pub fn is_king_crab(&self) -> bool {
        matches!(self.crab_type, CrabType::Boss)
    }

    /// The "Tide Boss" specifically — it never charges; instead it drifts and emits expanding
    /// shockwave pulses that scatter nearby free crabs and knock the train's tail loose.
    pub fn is_tide_boss(&self) -> bool {
        matches!(self.crab_type, CrabType::TideBoss)
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

    /// A "Magnet" crab: while it roams free it drags nearby uncaught crabs toward itself, so the
    /// herd bunches up around it. Catching the Magnet lands you in the middle of the cluster it
    /// gathered — a two-for-one that rewards chasing it (see the magnet-pull pass in main.rs).
    pub fn is_magnet(&self) -> bool {
        matches!(self.crab_type, CrabType::Magnet)
    }

    /// A "Thief" crab: a skittish parasite that ignores the herd and darts straight for your conga
    /// tail. Once it reaches the tail it latches on (`latch_timer` counts down) and peels a link
    /// loose every time the timer fires — pressuring the train you've already built rather than the
    /// herd you're chasing. Catch it or dislodge it (whistle/stomp/beam) to stop the bleed
    /// (see the thief-latch pass in main.rs).
    pub fn is_thief(&self) -> bool {
        matches!(self.crab_type, CrabType::Thief)
    }

    /// True while a Thief is actively clamped onto the tail (used to drive its "gnawing" visual).
    pub fn is_latched(&self) -> bool {
        self.is_thief() && self.latch_timer > 0.0
    }

    /// A rare "Golden Crab": a shiny, skittish high-value target that bolts fast and sparkles.
    /// Catching one pays a big lump-sum score bonus (see the catch block in main.rs) — a pure
    /// risk/reward chase: commit to snagging the prize before it flees, or stay on the herd.
    pub fn is_golden(&self) -> bool {
        matches!(self.crab_type, CrabType::Golden)
    }

    /// A Golden crab currently snared by a roaming Magnet's field: its skittish bolt has been
    /// overpowered by the lodestone pull and it's tethered in place, giving the player a brief
    /// window to snag the shiny prize it would otherwise never catch (see the magnet-pull pass in
    /// main.rs). Drives the snare visual and the slowed flee.
    pub fn is_magnet_snared(&self) -> bool {
        self.is_golden() && self.magnet_snared > 0.0
    }

    /// A free (not-yet-latched) Thief that a roaming Magnet's field has caught and pulled off its
    /// beeline to your conga tail — the lodestone overpowers its homing before it can latch. Parking
    /// a Magnet between your train and an incoming Thief becomes a defensive routing play. Drives the
    /// Thief's aura brightening while intercepted (see the magnet-pull pass in main.rs).
    pub fn is_magnet_intercepted(&self) -> bool {
        self.is_thief() && self.magnet_snared > 0.0
    }

    /// A roaming Magnet currently lured off its cluster by a nearby fleeing Golden — the shiny
    /// prize's shine has caught the lodestone's attention and it's drifting toward the prize instead
    /// of tending its herd. The mirror of `is_magnet_snared`: there the Magnet traps the Golden,
    /// here the Golden pulls the Magnet. Drives the Magnet aura shifting gold-ward while lured
    /// (see the magnet-lure pass in main.rs).
    pub fn is_magnet_lured(&self) -> bool {
        self.is_magnet() && self.magnet_lured > 0.0
    }

    /// Whether the crab can be snagged this frame. Regular crabs are catchable whenever free;
    /// a boss is only catchable once its health has been drained to zero by holding the beam on it.
    pub fn is_catchable(&self) -> bool {
        !self.caught && self.boss_health <= 0.0
    }
}
