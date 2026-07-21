//! Ambient wandering NPC conga trains — a King Crab leading followers across the world.
//!
//! The `NpcCongaTrain` state, its King Crab name generator, and its tier-based constructors.
//! Extracted from `state.rs` to keep that file focused on the core `MainState`.

use std::collections::VecDeque;

use ggez::glam::Vec2;
use rand::prelude::IndexedRandom;

use crate::enemies::CrabType;

/// Ambient wandering NPC conga train — a King Crab leading a few followers across the world.
/// Visual-only: it does not steal from or react to the player. It's world life, like weather.
pub struct NpcCongaTrain {
    pub leader_pos: Vec2,
    pub leader_vel: Vec2,
    pub target: Vec2,
    pub target_timer: f32,
    /// Sampled leader positions (pushed when leader moves >6px); followers trail by index offset.
    pub path_history: VecDeque<Vec2>,
    pub follower_types: Vec<CrabType>,
    /// Target volume for the rumble SFX, computed each frame from distance to player.
    /// Smoothed and applied by the EventHandler::update caller which has ctx access.
    pub target_vol: f32,
    /// Procedurally generated name — stable for the session (Shadow of Mordor style individuality).
    pub name: String,
    /// Visual scale of the King Crab leader — grows with conga length; tier sets the floor.
    pub leader_scale: f32,
    /// Minimum scale for this tier — leader can only grow above this, never shrink below it.
    pub base_scale: f32,
    /// Brief idle pause at destination before picking the next wander target (Rain World agency feel).
    pub idle_timer: f32,
    /// Preferred territory centre — each NPC biases its wander targets toward its own quadrant.
    pub territory_center: Vec2,
    /// Cooldown between steal events so one pass doesn't strip the whole chain in a single frame.
    pub steal_cooldown: f32,
    /// Cooldown gating how often this train can splice a *rival* train's back half (the whole-beach
    /// ecology steal), kept separate from `steal_cooldown` (which gates stealing from the player) so
    /// the two contests never starve each other. Lets the beach churn steadily without one train
    /// vacuuming another in a single frame.
    pub rival_steal_cooldown: f32,
    /// Steal telegraph fuse: >0 while a splice is armed and building toward its on-beat snap.
    /// Gives the player a brief, legible warning window (the threatened crabs tremble) before the
    /// rival actually takes the tail — losing crabs reads as earned, and the snap lands on the beat.
    pub steal_threat: f32,
    /// Latched splice index while a steal is armed, so the snap still fires from the same link even
    /// if the leader drifts a little off it during the telegraph window.
    pub steal_target: usize,
    /// Rival-vs-rival steal telegraph fuse (mirrors `steal_threat`, which arms the steal against the
    /// player): >0 while a whole-beach splice of a *smaller rival's* back half is armed and winding
    /// toward its on-beat snap. Makes the ecology's steals land ON the beat (INSPIRATION "the beat is
    /// the mechanic") instead of firing the instant two leaders cross. Snaps on the beat once shown,
    /// or on fuse expiry as a guaranteed fallback (which also keeps the headless bot deterministic).
    pub rival_steal_threat: f32,
    /// Victim train index snapshotted when the rival-vs-rival splice armed, so the snap fires against
    /// the same target even if this leader drifts during the telegraph. Re-validated at fire (bounds
    /// + still enough followers) so a train despawning mid-fuse fizzles cleanly instead of mis-splicing.
    pub rival_steal_victim: usize,
    /// Follower index the armed rival-vs-rival splice cuts from — the stolen back section is
    /// `victim.follower_types[cut_from..]`. Snapshotted at arm alongside `rival_steal_victim`.
    pub rival_steal_cut_from: usize,
    /// World position of the armed rival-vs-rival splice point, for the on-beat snap's shockwave/spill.
    pub rival_steal_splice_pos: Vec2,
    /// Time since this NPC last caught a free crab (throttles free-crab collection).
    pub catch_cooldown: f32,
    /// Revenge marker: >0 for a few seconds after this rival splices your tail. While it's live the
    /// rival wears a beat-pulsed "chase me" ring and a steal-back off it pays a revenge bonus, so
    /// losing crabs opens a duel instead of just taxing you (see REVENGE_WINDOW).
    pub revenge_timer: f32,
    /// Smoothed 0..1 "on the hunt" commitment: ramps up while this rival is deliberately routing to
    /// thread the player's back half (long-range pursuit, before it's close enough to ARM a splice),
    /// decays back to 0 when it's just wandering. Drives the early-warning threat-line tell so the
    /// player reads a committed rival in time to reroute — the legible-risk read the steal fight wants.
    pub hunt_intent: f32,
    /// When this train commits to hunting a *smaller rival* (not the player), the leader position of
    /// that prey — else `None`. Kept separate from `hunt_intent` (which is the player-hunt tell) on
    /// purpose: a rival chasing another rival must NOT paint a red "you're being hunted" line across
    /// the player's train. Instead it drives a distinct *gold* "predator closing" telegraph between the
    /// two Kings so the player can read an impending rival-vs-rival clash from afar and pre-position to
    /// swoop the spilled crumbs (ROADMAP step 3 "make it legible and swoopable"; agar.io "watch the big
    /// one creep toward the small one, lurk for the aftermath"). Set/cleared every frame in the hunt
    /// block, so it only shows while the urge is genuinely live and imminent.
    pub rival_hunt_target_pos: Option<Vec2>,
    /// 0..1 commitment behind `rival_hunt_target_pos` — the blend the hunt urge applied this frame,
    /// used to fade the gold telegraph up as the predator commits harder to the closing prey.
    pub rival_hunt_intensity: f32,
    /// 0..1 commit meter for the two-phase player hunt (#160 "smarter, scarier rival AI"). While a
    /// rival is stalking the player — shadowing at a lurk ring instead of beelining — patience builds
    /// deterministically: faster when the player is exposed (low groove), the chain is a juicy prize
    /// (long), and this tier is bold (elders over scouts). At 1.0 the rival COMMITS to the strike
    /// (see `hunt_committed`). Resets after every steal attempt (snap, dodge fizzle, or lost hunt) so
    /// the predator falls back to lurking between strikes — a stalk→strike rhythm, not a constant chase.
    pub stalk_patience: f32,
    /// Latched while this rival is in the committed strike phase of the player hunt: instead of
    /// aiming at where the vulnerable back half *is*, it leads its aim by the player's velocity and
    /// cuts off where the train is *heading* — an interception, the "it read my routing" scare.
    /// Fully legible: commitment drives `hunt_intent` to 1.0 so the existing red marching-dot
    /// telegraph burns at full intensity (a stalking rival shows the same tell faint), and the
    /// commit moment itself is called out by name. Cleared when the hunt ends or the strike resolves.
    pub hunt_committed: bool,
}

/// Generate a King Crab name. Leans hard into pirate swagger and crab-rave energy, with the
/// occasional Dark Souls boss title for grandiose laughs and a smattering of completely vanilla
/// names ("Kevin") for comedic deflation.
pub fn gen_king_crab_name(rng: &mut impl rand::Rng) -> String {
    const SOLO_NAMES: &[&str] = &[
        "Kevin", "Sandra", "Dave", "Gerald", "Steve", "Janet", "Barry", "Brenda", "Trevor", "Karen",
        "Gary", "Susan", "Nigel", "Deborah", "Keith", "Linda", "Wayne", "Sharon",
    ];
    let solo_roll: f32 = rng.random();
    if solo_roll < 0.15 {
        return SOLO_NAMES.choose(rng).unwrap().to_string();
    }

    const TITLES: &[&str] = &[
        // Pirate flair — the new backbone
        "Cap'n",
        "Captain",
        "First Mate",
        "Admiral",
        "Commodore",
        "Quartermaster",
        "Bosun",
        "Dread Pirate",
        "Ol'",
        "Peg-Leg",
        "One-Eyed",
        "Barnacle",
        "The Scurvy",
        "Corsair",
        "Buccaneer",
        // Crab rave energy
        "DJ",
        "MC",
        "Rave King",
        "Drop Lord",
        "Shellmaster",
        "Selecta",
        "Sir Bass-a-Lot",
        "The Beat-Droppin'",
        "Neon",
        "Sideways Champion",
        "The Eternal Groove of",
        // Dark Souls grandiosity — rare, funny in small doses
        "Gravelord",
        "Scuttlefiend,",
        "Devourer of Shores",
        "Lord of the Sunken Reef",
        // Oddball comedy
        "Misterhult",
        "Uncle",
    ];

    const NAMES: &[&str] = &[
        // Pirate
        "Clawbeard",
        "Blackclaw",
        "Saltbeard",
        "Redbeard",
        "Barnacle Bill",
        "Pegleg Pete",
        "Silverclaw",
        "Ironpincer",
        "the Saltbitten",
        "Bootstrap",
        "Flintclaw",
        "Longshanks",
        "Moultzilla",
        // Crab rave
        "Groove",
        "Bounceback",
        "Sidestep",
        "the Bass Drop",
        "Shellshaker",
        "Clawdrop",
        "Beatpincer",
        "Glowclaw",
        "Ravescuttle",
        "Bassline",
        "Snapsalot",
        "Neonshell",
        // Dark Souls — rare
        "Moltveil",
        "Brinewraith",
        "Abysswalker",
        "Shellreaper",
        // Vanilla comedy inline
        "Pete",
        "Snippy",
        "Dave",
    ];

    let title = TITLES.choose(rng).unwrap();
    let name = NAMES.choose(rng).unwrap();
    format!("{} {}", title, name)
}

impl NpcCongaTrain {
    pub fn new(world_width: f32, world_height: f32) -> Self {
        Self::new_at(world_width, world_height, 0)
    }

    pub fn new_at(world_width: f32, world_height: f32, index: usize) -> Self {
        // Three distinct tiers: small scout, medium wanderer, large elder.
        // Scale, speed, and follower count all differ so they read instantly at a glance.
        let (sx, sy, tc_x, tc_y, leader_scale, speed_hint) = match index {
            0 => (0.2, 0.3, 0.25, 0.3, 1.2_f32, 110.0_f32), // small/fast scout, top-left territory
            1 => (0.8, 0.2, 0.75, 0.25, 1.8_f32, 80.0_f32), // medium wanderer, top-right territory
            _ => (0.5, 0.8, 0.5, 0.75, 2.4_f32, 55.0_f32),  // large elder, bottom territory
        };
        let _ = speed_hint; // stored per-train would need another field; use leader_scale as proxy in update
        let start = Vec2::new(world_width * sx, world_height * sy);
        let territory_center = Vec2::new(world_width * tc_x, world_height * tc_y);
        // Initial target biased toward territory center
        let target = territory_center + Vec2::new(world_width * 0.1, world_height * 0.05);
        let follower_types = match index {
            // Small scout: fast light crabs
            0 => vec![CrabType::Fast, CrabType::Sneaky, CrabType::Normal],
            // Medium wanderer: balanced mix
            1 => vec![
                CrabType::Armored,
                CrabType::Normal,
                CrabType::Fast,
                CrabType::Magnet,
                CrabType::Dancer,
            ],
            // Large elder: heavy diverse retinue
            _ => vec![
                CrabType::Big,
                CrabType::Dancer,
                CrabType::Golden,
                CrabType::Normal,
                CrabType::Sneaky,
                CrabType::Hermit,
                CrabType::Fast,
            ],
        };
        let mut history = VecDeque::new();
        history.push_back(start);
        let name = gen_king_crab_name(&mut crate::rng::rng());
        Self {
            leader_pos: start,
            leader_vel: Vec2::ZERO,
            target,
            target_timer: 8.0 + index as f32 * 5.0,
            path_history: history,
            follower_types,
            target_vol: 0.0,
            name,
            leader_scale,
            base_scale: leader_scale,
            idle_timer: 0.0,
            territory_center,
            steal_cooldown: 0.0,
            rival_steal_cooldown: 0.0,
            steal_threat: 0.0,
            steal_target: 0,
            rival_steal_threat: 0.0,
            rival_steal_victim: 0,
            rival_steal_cut_from: 0,
            rival_steal_splice_pos: Vec2::ZERO,
            catch_cooldown: 0.0,
            revenge_timer: 0.0,
            hunt_intent: 0.0,
            rival_hunt_target_pos: None,
            rival_hunt_intensity: 0.0,
            stalk_patience: 0.0,
            hunt_committed: false,
        }
    }
}
