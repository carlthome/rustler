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
    Hermit,  // "Hermit Crab": hunkers inside a borrowed shell that the beam CAN'T wear down — only the ecosystem cracks it (a Stomp pops it, a Dancer's on-beat hop chips it, and a roaming Magnet's field RIPS it clean out). While shelled it periodically darts to a new host spot; once cracked it pops out defenceless and bolts, opening a brief catch window. Three existing verbs, one new target — the archetype web's newest edge.
    Golden,  // rare shiny "Golden Crab" — flees fast and sparkles; catching it pays a big lump-sum score bonus. A pure risk/reward chase decision: break off your herding to snag it before it bolts, or let it go.
    Splitter, // "Splitter Crab" — catching it cleaves your conga train at the midpoint and BANKS the back half for points. It's a real arrangement *bet*, gated on timing: catch it ON the beat for a clean cut — full payout plus a Jackpot on Goldens/Magnets/a cashed match-run in the slice; catch it OFF the beat for a sloppy cut — half payout and no jackpot. So time it to the beat to cash a hot tail for its full worth, or dodge it to keep building a run you'd only half-cash off-beat. Reuses the delivery payout verb.

    Boss,    // rare oversized "King Crab" — never spawns randomly, only via the boss trigger
    TideBoss, // rare oversized "Tide Boss" — drifts and emits shockwave pulses that scatter the train
    RhythmBoss, // rare oversized "Reef DJ" — its shell only drops on the beat, so the beam only wears it down when you hold it *on-beat*
    HermitKing, // rare oversized "Hermit King" — drags a stack of shell houses. The beam can't touch it: only Stomps crack it, one shell layer per pound (5 total). After 2 cracks it darts erratically and only ON-BEAT stomps land; after 4 it panics and flees for the world edge — escape and it drags a fresh shell back in (shell resets). The final crack exposes it; catching the big boy pays a triple score bonus (75-a-combo vs the usual 25 — see on_boss_caught).
    DancerKing, // rare golden "Dancer King" — catchable immediately, but it EVADES: every 2 beats it teleports to a mirrored position across the world. Nearby free crabs become ENTRANCED and mirror its movement; catching the King frees them — and catching it exactly ON the beat is a Perfect Catch that banks every entranced crab into your train at once.
}

/// Hermit King fight phase, derived purely from its remaining shell layers so the AI, the Stomp
/// gate, and the HUD all agree. 5 shells → Sturdy (slow lumber, any Stomp cracks), after 2 cracks →
/// Rattled (fast erratic darts, only ON-BEAT Stomps land), after 4 → Panicked (flees for the world
/// edge; escape resets its shell). At 0 shells it's exposed and catchable like any drained boss.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HermitKingPhase {
    Sturdy,   // shells 4-5: slow lumbering tank — any Stomp cracks a layer
    Rattled,  // shells 2-3: darts fast and erratically — only an on-beat Stomp lands
    Panicked, // shell 1: bolts for the nearest world edge — crack it before it escapes
}

/// Which phase a Hermit King with `shells` layers remaining is in. Pure so it's unit-testable.
pub fn hermit_king_phase(shells: f32) -> HermitKingPhase {
    if shells <= 1.0 {
        HermitKingPhase::Panicked
    } else if shells <= 3.0 {
        HermitKingPhase::Rattled
    } else {
        HermitKingPhase::Sturdy
    }
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
        // Hermit crabs are rare too (~7%): a shelled target the beam can't touch, so each one is a
        // little puzzle to solve with the ecosystem (Stomp / Dancer-hop / Magnet-rip). A couple per
        // run keep those crossover verbs in play without gating every catch behind a shell.
        // Golden crabs are the rarest of the herd (~3%, 1 in ~33): a shiny high-value target that
        // shows up just often enough to be a delightful surprise and a real "chase it or not" call,
        // never so often it becomes the main way to score. Rolled first so its rarity is exact.
        if rng.random_range(0..33) == 0 {
            return Golden;
        }
        // Splitter crabs are rare too (~6%, rolled just after Golden): each one cleaves your train
        // for a partial cash-out, so a couple per run makes train shape a live bet without turning
        // every catch into a coin flip on your conga line.
        if rng.random_range(0..17) == 0 {
            return Splitter;
        }
        match rng.random_range(0..14) {
            0 | 1 => Normal,
            2 | 3 => Fast,
            4 => Big,
            5 | 6 => Sneaky,
            7 => Armored,
            8 | 9 => Dancer,
            10 | 11 => Magnet,
            12 => Hermit,
            _ => Thief,
        }
    }

    /// A biome-emphasized herd roll. Most crabs still roll the normal tuned distribution above —
    /// the rarity comments there are load-bearing and stay intact. But with `emphasis` set (each
    /// level names one dominant herd archetype, see levels.rs), a fraction of the roll is
    /// *redirected* to that archetype so the zone visibly plays around it: a Kelp forest crawling
    /// with Thieves gnawing your tail, a Rocky shore studded with Armored shells, a Tide zone
    /// swarmed by routing Magnets. The Golden/Splitter roll-first rarities are preserved (those
    /// stay a delightful global surprise, never a biome's bread and butter) — only the plain herd
    /// share is what emphasis biases. ~33% redirect: strong enough that the zone's flavor is
    /// unmistakable (a third of the herd is the dominant type, several times its normal share)
    /// without drowning out the rest of the web — and without making the Kelp→Thief / Rock→Armored
    /// zones so single-note they're impossible to build a train in.
    pub fn random_emphasized(emphasis: Option<CrabType>, rng: &mut impl rand::Rng) -> Self {
        let base = Self::random(rng);
        let Some(emph) = emphasis else {
            return base;
        };
        // Never override the rare roll-first standouts, and never re-roll a crab that already
        // landed on the emphasized type — leave the tuned distribution's own hits intact.
        if matches!(base, CrabType::Golden | CrabType::Splitter) || base == emph {
            return base;
        }
        if rng.random_range(0..100) < 33 {
            emph
        } else {
            base
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
            CrabType::Hermit => 70.0..120.0, // the *popped* Hermit bolts fast once cracked; while shelled it barely drifts and darts in scripted hops (see the host-swap block in main.rs)
            CrabType::Golden => 85.0..135.0, // skittish and fast — the shiny prize bolts, so you have to commit to the chase
            CrabType::Splitter => 48.0..88.0, // darts at a lively clip — quick enough that snagging it is a deliberate move you set up, not an accident
            CrabType::Boss => 18.0..34.0,    // slow and lumbering
            CrabType::TideBoss => 24.0..44.0, // roams a touch quicker, but never charges
            CrabType::RhythmBoss => 20.0..38.0, // grooves around at a steady mid-pace, bobbing to the beat
            CrabType::HermitKing => 16.0..26.0, // a dragging shell-house tank — its Rattled/Panicked phases multiply this (see the hermit-king branch in crab_update.rs)
            CrabType::DancerKing => 22.0..36.0, // drifts between beats like a Dancer — its real evasion is the mirrored beat-teleport
        }
    }
    /// Shell health an archetype spawns with. While a crab's shell (stored in `boss_health`) is
    /// above zero it can't be lassoed or grabbed by the chain — the beam wears it down slowly, a
    /// Stomp cracks it instantly. Only Armored crabs carry a shell from the herd roll (the Boss
    /// gets its own, larger health set explicitly at spawn).
    pub fn initial_shell(&self) -> f32 {
        match self {
            CrabType::Armored => 2.0,
            // The Hermit hunkers in a borrowed shell. Same shell HP as Armored, but unlike Armored
            // the beam can't wear it down (gated out in main.rs): only a Stomp, a Dancer's on-beat
            // hop, or a Magnet's field cracks it — so its shell is a puzzle for the ecosystem verbs.
            CrabType::Hermit => 2.0,
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
            CrabType::Hermit => 0.35, // the borrowed shell is heavy and it clamps to the ground — a whistle barely budges a shelled Hermit (crack it first, then it's catchable)
            CrabType::Golden => 1.6, // flighty featherweight — a whistle is the surest way to reel the shiny prize in before it bolts
            CrabType::Splitter => 1.1, // light and jittery — the whistle reels it in cleanly when you decide to commit to the cleave
            CrabType::Boss => 0.0,  // the King Crab is unshakeable
            CrabType::TideBoss => 0.0, // the Tide Boss is unshakeable
            CrabType::RhythmBoss => 0.0, // the Reef DJ is unshakeable
            CrabType::HermitKing => 0.0, // the Hermit King is unshakeable
            CrabType::DancerKing => 0.0, // the Dancer King is unshakeable
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
            CrabType::Hermit => 0.40..=0.56,  // stocky like the Armored — the borrowed shell reads as a chunky lump the herd could bunch near
            CrabType::Golden => 0.34..=0.48,  // a hair bigger than a normal crab so the shine reads at a glance
            CrabType::Splitter => 0.34..=0.46, // mid-size, but its split-halves aura is what reads at a glance, not its bulk
            CrabType::Boss => 1.7..=2.1,      // towering
            CrabType::TideBoss => 1.7..=2.1,  // just as towering as the King Crab
            CrabType::RhythmBoss => 1.7..=2.1, // just as towering as the other bosses
            CrabType::HermitKing => 1.9..=2.3, // the biggest of the bunch — it IS a big boy (counts as 3 in the chain)
            CrabType::DancerKing => 1.5..=1.8, // a touch lighter than the tanks — it's a dancer, after all
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
    /// Tint inherited from the player's captured King Crab power-up.
    pub chain_color: Option<[f32; 3]>,
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
    pub stun_timer: f32,          // King Crab only: >0 while dazed after ramming a parked Armored shell — can't charge and its shell drains far faster under the beam, turning the block into a damage window
    pub latch_timer: f32,         // Thief only: >0 while clamped onto the conga tail, counts down to the next link it peels off
    pub panic_amp: f32,           // >=1.0 fear-ripple amplitude carried while startled: a fleeing Golden seeds this high so its panic bomb keeps rippling harder than baseline for a few beats
    pub magnet_snared: f32,       // Golden or Thief: >0 while a roaming Magnet's field has overpowered its movement and tethered it — for a Golden, the "grab the prize now" window; for a homing Thief, an interception that stops it reaching your tail. Counts down; refreshed each frame the crab stays deep in the field. Drives the snare visual + slowed movement.
    pub magnet_lured: f32,        // Magnet only: >0 while this roaming Magnet is being pulled off its cluster toward a nearby fleeing Golden — the shiny prize's shine luring the lodestone. Counts down; refreshed each frame it keeps chasing. Drives the aura shifting gold-ward.
    pub thief_lured: f32,         // Thief only: >0 while a homing Thief has been lured off its beeline toward your tail by a nearby fleeing Golden — a thief can't resist a shiny thing, so it chases the prize instead of raiding your train. Counts down; refreshed each frame the divert holds. Drives the Thief aura bleeding gold-ward.
    pub magnet_charged: f32,      // Magnet only: >0 while this Magnet is pinning a snared Golden — the prize's shine supercharges the lodestone into a wider, stronger herd-vacuum. Counts down; refreshed each frame it holds a snared Golden. Drives the aura flaring gold and wide.
    pub slingshot_spent: f32,     // Golden only: >0 for a brief window after a Tide Boss surge FIRED this Golden through a loaded Magnet at the boss. While it counts down the Magnet field can't re-snare the Golden, so the shot genuinely spends the prize (the trade the slingshot promises) instead of it reloading in place next frame.
    pub host_swap_timer: f32,     // Hermit only: counts down while shelled; when it fires the Hermit darts to a new host spot (a scripted hop, like the Dancer's beat hop but on its own irregular timer), then resets. Gives the shelled Hermit its signature "hides and swaps hosts" movement so it isn't a stationary Armored reskin.
    pub surge_timer: f32,         // On-beat herd stampede: kicked to 1.0 on every downbeat for idle (non-spooked, non-caught) free crabs, decays over the beat. While it's live the crab DARTS along its own heading (an extra positional shove on top of base drift), then coasts between beats — so the whole loose herd visibly lurches on the "1" and glides between. Makes *where a free crab lands* a rhythm read: a groove-savvy player predicts the on-beat surge and positions to intercept the herd on the bar, rather than chasing it flatly. Distinct from every pull tool (Groove Call/Slam/Dash/whistle) — it shoves nothing toward the player, it just moves the herd on the beat.
    pub entranced: f32,           // >0 while a free crab is spellbound by the Dancer King: it stops fleeing and shadows the King's drift instead. Refreshed each beat the crab is near the King; catching the King frees them all — and catching it exactly ON the beat banks every entranced crab into the train at once (the Perfect Catch payoff).
}

impl EnemyCrab {
    pub fn crab_color(&self) -> [f32; 3] {
        let t = (self.spawn_time / 10.0).min(1.0);
        let base = match self.crab_type {
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
            CrabType::Hermit => [0.72, 0.44, 0.24],             // warm coppery-brown borrowed shell — reads as an earthy shelled lump, distinct from Armored's cold steel
            CrabType::Golden => [1.0, 0.86, 0.28],              // bright treasure-gold — the shiny prize pops against the whole herd
            CrabType::Splitter => [0.20, 0.90, 0.80],           // bright split-cyan/teal — reads as "cleaver", distinct from every warm herd tone
            CrabType::Boss => [0.96, 0.72, 0.16], // regal king-crab gold
            CrabType::TideBoss => [0.20, 0.68, 0.86], // deep tidal cyan-blue
            CrabType::RhythmBoss => [0.72, 0.30, 0.95], // pulsing disco violet
            CrabType::HermitKing => [0.82, 0.48, 0.20], // burnished royal copper — the Hermit's earthy brown crowned into a gleaming shell-house king
            CrabType::DancerKing => [1.0, 0.62, 0.45], // golden-rose disco royalty — the Dancer's hot pink gilded into a shimmering king
        };
        if let Some(tint) = self.chain_color {
            [
                base[0] * 0.62 + tint[0] * 0.38,
                base[1] * 0.62 + tint[1] * 0.38,
                base[2] * 0.62 + tint[2] * 0.38,
            ]
        } else {
            base
        }
    }

    /// Any oversized boss — must be worn down under the flashlight before it can be caught. Covers
    /// both the charging King Crab and the pulsing Tide Boss, so all the shared boss plumbing
    /// (health ring, catchable-only-when-drained, unshakeable, non-fleeing) applies to both.
    pub fn is_boss(&self) -> bool {
        matches!(
            self.crab_type,
            CrabType::Boss
                | CrabType::TideBoss
                | CrabType::RhythmBoss
                | CrabType::HermitKing
                | CrabType::DancerKing
        )
    }

    /// The "Hermit King" specifically — the shell-house tank whose stack of shells the beam can't
    /// touch: only Stomps crack it, one layer per pound, with its Rattled phase demanding ON-BEAT
    /// stomps and its Panicked phase racing you to the world edge (escape resets its shell).
    pub fn is_hermit_king(&self) -> bool {
        matches!(self.crab_type, CrabType::HermitKing)
    }

    /// The "Dancer King" specifically — catchable immediately, but every 2 beats it teleports to a
    /// mirrored position across the world. Nearby free crabs become entranced and shadow its drift;
    /// catching it frees them, and an exactly ON-BEAT catch banks every entranced crab at once.
    pub fn is_dancer_king(&self) -> bool {
        matches!(self.crab_type, CrabType::DancerKing)
    }

    /// The "Reef DJ" rhythm boss specifically — it never charges or pulses; instead its shell only
    /// drops on the beat, so the beam only wears it down while the on-beat window is open. The
    /// whole fight is carried by the game's rhythm system: hold the light AND land it on the beat.
    pub fn is_rhythm_boss(&self) -> bool {
        matches!(self.crab_type, CrabType::RhythmBoss)
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

    /// A "Hermit" crab: it hunkers in a borrowed shell (stored in `boss_health`) that the beam
    /// can't wear down — only the ecosystem cracks it. A Stomp pops it instantly, a Dancer's on-beat
    /// hop chips it, and a roaming Magnet's field rips it clean out. While shelled it periodically
    /// darts to a new host spot (see the host-swap block in main.rs); once its shell is cracked it
    /// pops out defenceless and bolts fast, opening a brief catch window.
    pub fn is_hermit(&self) -> bool {
        matches!(self.crab_type, CrabType::Hermit)
    }

    /// A Hermit that still has its shell up — the state where the beam/lasso/chain can't touch it and
    /// only the Stomp/Dancer-hop/Magnet-rip verbs get through. Once cracked (`boss_health <= 0`) it's
    /// an ordinary skittish catchable crab, just a fast-fleeing one.
    pub fn is_shelled_hermit(&self) -> bool {
        self.is_hermit() && self.boss_health > 0.0
    }

    /// A rhythm "Dancer" crab: it drifts slowly between beats and takes a sharp hop on each beat
    /// (see the beat-fire block in main.rs). Catch it during the freeze, not mid-leap.
    pub fn is_dancer(&self) -> bool {
        matches!(self.crab_type, CrabType::Dancer)
    }

    /// A skittish "Sneaky" crab: evasive and light — it darts off readily but folds hardest of all
    /// but the Golden to a Whistle sweep (enemies.rs whistle_pull 1.5, "folds hard to a whistle").
    /// The Whistle's flagship soft-RPS target: the tool that "gathers skittish crabs" (INSPIRATION.md
    /// Doom Eternal note) flushes it out of hiding and reels it in.
    pub fn is_sneaky(&self) -> bool {
        matches!(self.crab_type, CrabType::Sneaky)
    }

    /// A "Fast" crab: an ordinary skittish crab with a high top speed, so it out-runs a plain chase.
    /// It's the beam's soft-RPS target — the flashlight cone drags on a fleeing Fast crab so the
    /// tool that "melts fast ones" (INSPIRATION.md Doom Eternal note) can actually pin it down
    /// (see the beam-pin branch in update_crabs).
    pub fn is_fast(&self) -> bool {
        matches!(self.crab_type, CrabType::Fast)
    }

    /// A "Magnet" crab: while it roams free it drags nearby uncaught crabs toward itself, so the
    /// herd bunches up around it. Catching the Magnet lands you in the middle of the cluster it
    /// gathered — a two-for-one that rewards chasing it (see the magnet-pull pass in main.rs).
    pub fn is_magnet(&self) -> bool {
        matches!(self.crab_type, CrabType::Magnet)
    }

    /// A "Big" crab: an oversized, heavy, slow trundler. The whistle "shrugs most off" (whistle_pull
    /// 0.4), so herding it barely works — the lasso is its intended counter: the loop physically
    /// snags and *hauls* the heavy crab in where a sonic nudge can't. That's its soft-RPS role.
    pub fn is_big(&self) -> bool {
        matches!(self.crab_type, CrabType::Big)
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

    /// True while a King Crab is dazed after ramming a parked Armored shell mid-charge. While
    /// stunned it can't wind up a new charge and its own shell drains far faster under the beam,
    /// so baiting the lunge into a shell opens a real damage window (see the block pass in main.rs).
    pub fn is_stunned(&self) -> bool {
        self.stun_timer > 0.0
    }

    /// A rare "Golden Crab": a shiny, skittish high-value target that bolts fast and sparkles.
    /// Catching one pays a big lump-sum score bonus (see the catch block in main.rs) — a pure
    /// risk/reward chase: commit to snagging the prize before it flees, or stay on the herd.
    pub fn is_golden(&self) -> bool {
        matches!(self.crab_type, CrabType::Golden)
    }

    /// A "Splitter" crab: catching it cleaves the conga train at the midpoint and instantly banks
    /// the back half for a partial cash-out (see the split block in handle_crab_catching). It turns
    /// the train's shape into a live bet — grab it mid-match-run to cash a slice at speed, or dodge
    /// it to keep your run and length intact.
    pub fn is_splitter(&self) -> bool {
        matches!(self.crab_type, CrabType::Splitter)
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

    pub fn is_magnet_charged(&self) -> bool {
        self.is_magnet() && self.magnet_charged > 0.0
    }

    /// A homing Thief currently lured off its beeline by a nearby fleeing Golden — a thief can't
    /// resist a shiny thing, so the prize's shine pulls it away from raiding your conga tail and it
    /// chases the Golden instead. Cross-archetype mirror of the Magnet-lure: there gold tugs the
    /// lodestone, here gold tugs the raider. A fleeing Golden becomes an accidental decoy that
    /// draws a Thief off your train. Drives the Thief aura bleeding gold-ward while lured (see the
    /// thief-lure pass in main.rs).
    pub fn is_thief_lured(&self) -> bool {
        self.is_thief() && self.thief_lured > 0.0
    }

    /// Whether the crab can be snagged this frame. Regular crabs are catchable whenever free;
    /// a boss is only catchable once its health has been drained to zero by holding the beam on it.
    pub fn is_catchable(&self) -> bool {
        !self.caught && self.boss_health <= 0.0
    }
}
