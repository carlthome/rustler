use crate::enemies::CrabType;
use crate::spawnings::SpawnPattern;

/// Playfield size relative to the fixed game viewport. Keeping this on `Level` makes the campaign's
/// sense of travel explicit while all world-space systems continue to use the same bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapSize {
    Tutorial,
    Medium,
    Large,
}

impl MapSize {
    /// Returns the factor by which each world dimension exceeds the viewport: 1.0 fits the
    /// tutorial in one screen, while 2.0 and 4.0 create progressively larger campaign maps.
    pub const fn viewport_multiplier(self) -> f32 {
        match self {
            Self::Tutorial => 1.0,
            Self::Medium => 2.0,
            Self::Large => 4.0,
        }
    }

    /// Tutorial maps are deliberately quiet single-screen lessons, without roaming King Crab trains.
    pub const fn spawns_npc_trains(self) -> bool {
        !matches!(self, Self::Tutorial)
    }
}

/// The completion goal for a campaign level. Each condition tests the mechanic the biome was
/// built for — not just "get X score" — so crossing into the next biome feels like a gear change.
/// Evaluated every frame during a campaign run (see the win-check block in `game_update`); when
/// met, the world-map node completes and the next one unlocks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WinCondition {
    /// Total crabs banked at the pen reaches this count.
    BankCrabs(usize),
    /// Train length simultaneously reaches this count.
    BuildTrain(usize),
    /// Armored/Hermit shells cracked reaches `shells` while the train is at least `min_train` —
    /// both gates at once, so shells can't be farmed from a safe corner with an empty train.
    CrackAndHold { shells: usize, min_train: usize },
    /// Train stays at or above `target` for `seconds` consecutive seconds (the timer resets the
    /// moment the train dips below the target).
    HoldTrain { target: usize, seconds: f32 },
}

impl WinCondition {
    /// Pure evaluation against the live run counters, so the same predicate is reachable from a
    /// headless test as from the frame loop. `hold_secs` is how long the train has continuously
    /// been at or above the HoldTrain target (maintained by the caller).
    pub fn met(&self, banked: usize, train: usize, shells: usize, hold_secs: f32) -> bool {
        match *self {
            WinCondition::BankCrabs(n) => banked >= n,
            WinCondition::BuildTrain(n) => train >= n,
            WinCondition::CrackAndHold { shells: s, min_train } => shells >= s && train >= min_train,
            WinCondition::HoldTrain { seconds, .. } => hold_secs >= seconds,
        }
    }

    /// Short live-progress line for the HUD corner counter, so the player always knows where they
    /// stand against the goal.
    pub fn progress_text(&self, banked: usize, train: usize, shells: usize, hold_secs: f32) -> String {
        match *self {
            WinCondition::BankCrabs(n) => format!("GOAL  Bank crabs: {} / {}", banked.min(n), n),
            WinCondition::BuildTrain(n) => format!("GOAL  Train of {} at once: {} / {}", n, train.min(n), n),
            WinCondition::CrackAndHold { shells: s, min_train } => format!(
                "GOAL  Shells cracked: {} / {}  |  Train: {} (keep \u{2265} {})",
                shells.min(s),
                s,
                train,
                min_train
            ),
            WinCondition::HoldTrain { target, seconds } => {
                if train >= target {
                    format!(
                        "GOAL  Hold train \u{2265} {}: {:.0}s / {:.0}s",
                        target,
                        hold_secs.min(seconds),
                        seconds
                    )
                } else {
                    format!("GOAL  Build a train of {} and hold it {:.0}s", target, seconds)
                }
            }
        }
    }
}

pub struct LevelPattern {
    pub pattern: SpawnPattern,
    pub count: usize,
    pub duration: f32,
    pub centroid: (f32, f32),
}

/// What the terrain patches in a biome physically *do* — the mechanical wrinkle that makes each
/// zone route differently, not just look different. The same patch geometry (see `pick_tide_pools`)
/// is reused for all of them; the kind decides how the player and train interact with a patch.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TerrainKind {
    /// No terrain hazard — an open, gentle field. The beginner zone.
    Open,
    /// Shallow water that drags: wading slows the player to a crawl and bleeds momentum.
    Water,
    /// Solid rock that blocks: the player can't enter a patch, so they read as chokepoints to
    /// thread the train around.
    Rock,
    /// Clinging kelp that snags the conga tail: crossing a patch slows the player *and* risks
    /// stripping a trailing crab loose, adding chain-snap tension to the route.
    Kelp,
    /// The fourth-wall "Desktop" biome: the patch geometry renders as rectangular OS-style window
    /// panels that are SOLID WALLS. Crabs and the conga train route around them exactly the way they
    /// route around Rock chokepoints — it reuses that same push-out collision, so there's no new
    /// physics here (see controls.rs). For now the wink is purely presentational: a flat neutral
    /// wallpaper (main.rs) plus draggable-looking windows (graphics::terrain). Once the ggez 0.10
    /// migration lands, the real game window can go transparent and these panels become the player's
    /// actual OS windows — see the `TODO(ggez-0.10)` seams.
    Desktop,
}

/// The broad visual composition of a campaign map. This is deliberately separate from
/// `TerrainKind`: a Sunken Treasury can look fully underwater while retaining the same water-pool
/// movement rules, and a river can cut through an otherwise open field.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapLayout {
    Meadow,
    Beach,
    Underwater,
    Coast,
    River,
}

/// A visual "zone" a level takes place in. Gives each level a distinct read so runs feel like
/// they travel somewhere instead of one continuous space. `tint` is a multiply grade laid over
/// the whole ground; `pulse` recolors the on-beat flash to match the zone's mood; `terrain` is the
/// mechanical wrinkle its patches carry; `layout` gives the ground a distinct broad composition.
#[derive(Clone, Copy)]
pub struct Biome {
    pub name: &'static str,
    pub tint: (u8, u8, u8),
    pub pulse: (u8, u8, u8),
    pub terrain: TerrainKind,
    pub layout: MapLayout,
    pub music: BiomeMusic,
}

/// Authored musical identity for a biome. The gameplay beat remains one shared 4/4 clock, while
/// this selects the loop's timbre, harmony, and arrangement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BiomeMusic {
    SunnyGroove,
    TidalDorian,
    RockShanty,
    KelpDisco,
    MoonlitWaltz,
    WarrenMarch,
    TreasuryRave,
    SplitterShanty,
    DesktopChip,
}

pub struct Level {
    pub title: String,
    pub description: String,
    pub difficulty: usize,
    pub map_size: MapSize,
    pub biome: Biome,
    /// The herd archetype this zone leans on — its "second half" of the gear-change. Terrain
    /// (above) changes how the ground routes; `emphasis` changes *what you're catching* so
    /// crossing a boundary visibly shifts play, not just the tint. A fraction of the herd roll is
    /// redirected to this type (see `CrabType::random_emphasized`). Paired thematically to the
    /// terrain: Water→Magnet (routing), Rock→Armored (shells to crack), Kelp→Thief (tail
    /// pressure). `None` on the beginner zone, which stays a clean, unflavored intro.
    pub emphasis: Option<CrabType>,
    /// Bosses belong to the zone's archetype family. Arcade keeps the same level sequence alive,
    /// while the Desktop deliberately cycles through every boss as its meme finale.
    pub boss_sequence: Vec<CrabType>,
    /// The completion goal for this level during a campaign run. Meeting it completes the
    /// world-map node and unlocks the next one.
    pub win_condition: WinCondition,
    pub patterns: Vec<LevelPattern>,
}

impl Level {
    pub fn boss_for_encounter(&self, encounter: usize) -> CrabType {
        if self.boss_sequence.is_empty() {
            CrabType::Boss
        } else {
            self.boss_sequence[encounter % self.boss_sequence.len()]
        }
    }
}

/// The player-facing name of a level's emphasized archetype, for the Control-style title banner.
/// Surfacing it on the card is what makes the boundary *read* as a gear-change instead of an
/// invisible probability bump — the zone announces its dominant threat as you cross into it.
pub fn emphasis_label(emphasis: Option<CrabType>) -> Option<&'static str> {
    match emphasis {
        Some(CrabType::Big) => Some("BIG CRABS"),
        Some(CrabType::Magnet) => Some("MAGNET SWARM"),
        Some(CrabType::Armored) => Some("ARMORED SHELLS"),
        Some(CrabType::Thief) => Some("THIEF INFESTATION"),
        Some(CrabType::Dancer) => Some("DANCER RAVE"),
        Some(CrabType::Hermit) => Some("HERMIT WARREN"),
        Some(CrabType::Golden) => Some("GOLDEN HUNT"),
        Some(CrabType::Splitter) => Some("SPLITTER RUN"),
        _ => None,
    }
}

pub fn boss_label(boss: CrabType) -> &'static str {
    match boss {
        CrabType::Boss => "KING CRAB",
        CrabType::TideBoss => "TIDE BOSS",
        CrabType::RhythmBoss => "REEF DJ",
        CrabType::HermitKing => "HERMIT KING",
        CrabType::DancerKing => "DANCER KING",
        _ => "KING CRAB",
    }
}

pub fn get_levels() -> Vec<Level> {
    vec![
        Level {
            title: "First Landing".to_string(),
            description: "Learn the full catch, train, and bank loop on open sand.".to_string(),
            difficulty: 0,
            map_size: MapSize::Tutorial,
            biome: Biome {
                name: "Sunny Meadow",
                tint: (255, 248, 214),
                pulse: (120, 255, 120),
                terrain: TerrainKind::Open,
                layout: MapLayout::Meadow,
                music: BiomeMusic::SunnyGroove,
            },
            emphasis: None,
            boss_sequence: vec![CrabType::Boss],
            // Clean intro: teaches the full catch -> train -> bank loop with no hazards.
            win_condition: WinCondition::BankCrabs(25),
            patterns: vec![
                LevelPattern {
                    pattern: SpawnPattern::SingleRandom,
                    count: 6,
                    duration: 14.0,
                    centroid: (0.5, 0.5),
                },
                LevelPattern {
                    pattern: SpawnPattern::SingleRandom,
                    count: 4,
                    duration: 11.2,
                    centroid: (0.2, 0.8),
                },
            ],
        },
        Level {
            title: "Undertow Shuffle".to_string(),
            description: "Route a growing train through dragging tidal pools.".to_string(),
            difficulty: 2,
            map_size: MapSize::Tutorial,
            biome: Biome {
                name: "Tide Pools",
                tint: (150, 215, 255),
                pulse: (90, 200, 255),
                terrain: TerrainKind::Water,
                layout: MapLayout::Coast,
                music: BiomeMusic::TidalDorian,
            },
            // Water routes the herd; the Magnet reroutes it again by clustering free crabs — the
            // zone becomes a routing puzzle where you catch a Magnet to net the blob it gathered.
            emphasis: Some(CrabType::Magnet),
            boss_sequence: vec![CrabType::TideBoss],
            // One gross catching move: a well-timed Magnet catch scoops the clustered herd, so the
            // win fires mid-wave the instant the train hits 15 — no banking, no patience required.
            win_condition: WinCondition::BuildTrain(15),
            patterns: vec![
                LevelPattern {
                    pattern: SpawnPattern::UniformRandom,
                    count: 10,
                    duration: 11.2,
                    centroid: (0.7, 0.3),
                },
                LevelPattern {
                    pattern: SpawnPattern::SineWave,
                    count: 13,
                    duration: 14.0,
                    centroid: (0.3, 0.7),
                },
                LevelPattern {
                    pattern: SpawnPattern::Circle,
                    count: 15,
                    duration: 16.8,
                    centroid: (0.5, 0.5),
                },
                LevelPattern {
                    pattern: SpawnPattern::Cluster,
                    count: 18,
                    duration: 14.0,
                    centroid: (0.8, 0.8),
                },
                LevelPattern {
                    pattern: SpawnPattern::Cluster,
                    count: 11,
                    duration: 8.4,
                    centroid: (0.2, 0.2),
                },
            ],
        },
        Level {
            title: "Breaker's Passage".to_string(),
            description: "Crack shells while threading the rocky chokepoints.".to_string(),
            difficulty: 3,
            map_size: MapSize::Tutorial,
            biome: Biome {
                name: "Rocky Shore",
                tint: (178, 192, 208),
                pulse: (205, 222, 235),
                terrain: TerrainKind::Rock,
                layout: MapLayout::Coast,
                music: BiomeMusic::RockShanty,
            },
            // Rocky chokepoints already make you thread the train; the Armored emphasis makes you
            // reach for the Stomp constantly — a zone of shells to crack while dodging the rocks.
            emphasis: Some(CrabType::Armored),
            boss_sequence: vec![CrabType::HermitKing],
            // Two gates force both verbs: stomp shells open in the rock chokepoints AND hold a
            // real train — no cheesing shells from a safe corner while ignoring the herd.
            win_condition: WinCondition::CrackAndHold { shells: 8, min_train: 15 },
            patterns: vec![
                LevelPattern {
                    pattern: SpawnPattern::Cluster,
                    count: 22,
                    duration: 14.0,
                    centroid: (0.5, 0.5),
                },
                LevelPattern {
                    pattern: SpawnPattern::SineWave,
                    count: 18,
                    duration: 16.8,
                    centroid: (0.8, 0.2),
                },
                LevelPattern {
                    pattern: SpawnPattern::Circle,
                    count: 26,
                    duration: 19.6,
                    centroid: (0.2, 0.8),
                },
                LevelPattern {
                    pattern: SpawnPattern::Cluster,
                    count: 15,
                    duration: 11.2,
                    centroid: (0.8, 0.8),
                },
                LevelPattern {
                    pattern: SpawnPattern::SineWave,
                    count: 11,
                    duration: 8.4,
                    centroid: (0.2, 0.2),
                },
            ],
        },
        Level {
            title: "Kelp After Dark".to_string(),
            description: "Defend a packed conga line on a snagging neon dance floor.".to_string(),
            difficulty: 4,
            map_size: MapSize::Tutorial,
            biome: Biome {
                name: "Neon Kelp Forest",
                tint: (120, 185, 150),
                pulse: (255, 90, 220),
                terrain: TerrainKind::Kelp,
                layout: MapLayout::River,
                music: BiomeMusic::KelpDisco,
            },
            // Kelp already snags your tail loose; a Thief infestation gnaws at it too — the whole
            // zone is one long fight to defend the train you've built. Tail pressure squared.
            emphasis: Some(CrabType::Thief),
            boss_sequence: vec![CrabType::Boss],
            // Pure defense: getting to 20 is easy, keeping them against kelp snags and Thieves is
            // the whole game. The 30s timer resets the moment the train dips below 20.
            win_condition: WinCondition::HoldTrain { target: 20, seconds: 30.0 },
            patterns: vec![
                LevelPattern {
                    pattern: SpawnPattern::BeatGrid,
                    count: 16,
                    duration: 16.8,
                    centroid: (0.5, 0.5),
                },
                LevelPattern {
                    pattern: SpawnPattern::Spiral,
                    count: 22,
                    duration: 19.6,
                    centroid: (0.5, 0.5),
                },
                LevelPattern {
                    pattern: SpawnPattern::BeatGrid,
                    count: 30,
                    duration: 19.6,
                    centroid: (0.5, 0.5),
                },
                LevelPattern {
                    pattern: SpawnPattern::Spiral,
                    count: 38,
                    duration: 22.4,
                    centroid: (0.5, 0.5),
                },
            ],
        },
        Level {
            title: "Lunar Waltz".to_string(),
            description: "Follow the beat through a moonlit dance floor.".to_string(),
            difficulty: 5,
            map_size: MapSize::Large,
            biome: Biome {
                name: "Moonlit Ballroom",
                tint: (126, 118, 190),
                pulse: (255, 170, 245),
                terrain: TerrainKind::Open,
                layout: MapLayout::Beach,
                music: BiomeMusic::MoonlitWaltz,
            },
            emphasis: Some(CrabType::Dancer),
            boss_sequence: vec![CrabType::RhythmBoss],
            win_condition: WinCondition::BuildTrain(24),
            patterns: vec![
                LevelPattern { pattern: SpawnPattern::BeatGrid, count: 22, duration: 16.8, centroid: (0.5, 0.5) },
                LevelPattern { pattern: SpawnPattern::Spiral, count: 28, duration: 19.6, centroid: (0.3, 0.7) },
            ],
        },
        Level {
            title: "Hermit's March".to_string(),
            description: "Crack the borrowed shells before the Warren closes in.".to_string(),
            difficulty: 6,
            map_size: MapSize::Large,
            biome: Biome {
                name: "Shellgrave Warren",
                tint: (184, 146, 112),
                pulse: (255, 205, 125),
                terrain: TerrainKind::Rock,
                layout: MapLayout::Beach,
                music: BiomeMusic::WarrenMarch,
            },
            emphasis: Some(CrabType::Hermit),
            boss_sequence: vec![CrabType::HermitKing],
            win_condition: WinCondition::CrackAndHold { shells: 12, min_train: 18 },
            patterns: vec![
                LevelPattern { pattern: SpawnPattern::Cluster, count: 24, duration: 16.8, centroid: (0.4, 0.4) },
                LevelPattern { pattern: SpawnPattern::Circle, count: 30, duration: 19.6, centroid: (0.7, 0.6) },
            ],
        },
        Level {
            title: "Gilded Current".to_string(),
            description: "Chase the shine before the tide hides the prize.".to_string(),
            difficulty: 7,
            map_size: MapSize::Large,
            biome: Biome {
                name: "Sunken Treasury",
                tint: (214, 180, 106),
                pulse: (255, 245, 130),
                terrain: TerrainKind::Water,
                layout: MapLayout::Underwater,
                music: BiomeMusic::TreasuryRave,
            },
            emphasis: Some(CrabType::Golden),
            boss_sequence: vec![CrabType::Boss],
            win_condition: WinCondition::BankCrabs(55),
            patterns: vec![
                LevelPattern { pattern: SpawnPattern::UniformRandom, count: 26, duration: 16.8, centroid: (0.6, 0.3) },
                LevelPattern { pattern: SpawnPattern::Cluster, count: 34, duration: 22.4, centroid: (0.3, 0.7) },
            ],
        },
        Level {
            title: "Cutlass Causeway".to_string(),
            description: "Shape the train carefully: every catch can cut it in two.".to_string(),
            difficulty: 8,
            map_size: MapSize::Large,
            biome: Biome {
                name: "The Splitter's Causeway",
                tint: (190, 132, 156),
                pulse: (255, 125, 180),
                terrain: TerrainKind::Kelp,
                layout: MapLayout::River,
                music: BiomeMusic::SplitterShanty,
            },
            emphasis: Some(CrabType::Splitter),
            boss_sequence: vec![CrabType::Boss],
            win_condition: WinCondition::HoldTrain { target: 24, seconds: 36.0 },
            patterns: vec![
                LevelPattern { pattern: SpawnPattern::SineWave, count: 28, duration: 19.6, centroid: (0.5, 0.3) },
                LevelPattern { pattern: SpawnPattern::Spiral, count: 36, duration: 22.4, centroid: (0.5, 0.7) },
            ],
        },
        // The fourth-wall surprise (Inscryption / old Windows PowerToys): a special level that
        // "shouldn't be in the game." The playfield becomes a flat OS wallpaper and the terrain
        // patches render as rectangular application windows you route the conga train around. It
        // sits last so it's *discovered* by getting this far, per INSPIRATION — the big Control-style
        // title card does the wink. For this first slice the windows are solid walls (reusing the
        // Rock push-out collision); the real transparent-window hookup is deferred to ggez 0.10.
        Level {
            title: "Unauthorized Encore".to_string(),
            description: "Wait — this isn't part of the game. Route the train around the windows."
                .to_string(),
            difficulty: 9,
            map_size: MapSize::Large,
            biome: Biome {
                name: "You Shouldn't Be Here",
                // Flat neutral desktop wallpaper (classic teal). main.rs paints this opaque over the
                // ground so the beach texture reads as a plain screen — the transparency seam.
                tint: (58, 110, 128),
                // Cool window-highlight blue for the on-beat pulse / accents.
                pulse: (150, 190, 235),
                terrain: TerrainKind::Desktop,
                layout: MapLayout::Meadow,
                music: BiomeMusic::DesktopChip,
            },
            // No archetype emphasis — the wink is the whole hook; keep the herd plain so the terrain
            // (the windows) is what reads as different, not the crabs.
            emphasis: None,
            boss_sequence: vec![
                CrabType::Boss,
                CrabType::TideBoss,
                CrabType::RhythmBoss,
                CrabType::HermitKing,
                CrabType::DancerKing,
            ],
            // The hardest banking challenge: the window panels force long, risky routes to the pen.
            // (The issue's BankUnderPressure escape-tracking variant is deferred — there's no
            // "escaped off-world" concept in the sim yet — so this takes its sanctioned fallback.)
            win_condition: WinCondition::BankCrabs(40),
            patterns: vec![
                LevelPattern {
                    pattern: SpawnPattern::UniformRandom,
                    count: 16,
                    duration: 16.8,
                    centroid: (0.5, 0.5),
                },
                LevelPattern {
                    pattern: SpawnPattern::Cluster,
                    count: 22,
                    duration: 19.6,
                    centroid: (0.3, 0.4),
                },
                LevelPattern {
                    pattern: SpawnPattern::BeatGrid,
                    count: 28,
                    duration: 19.6,
                    centroid: (0.7, 0.6),
                },
                LevelPattern {
                    pattern: SpawnPattern::SineWave,
                    count: 34,
                    duration: 22.4,
                    centroid: (0.4, 0.7),
                },
                LevelPattern {
                    pattern: SpawnPattern::Circle,
                    count: 40,
                    duration: 25.2,
                    centroid: (0.6, 0.3),
                },
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_four_campaign_levels_are_tutorial_sized() {
        let levels = get_levels();
        assert!(levels[..4]
            .iter()
            .all(|level| level.map_size == MapSize::Tutorial));
        assert!(levels[4..]
            .iter()
            .all(|level| level.map_size == MapSize::Large));
    }

    #[test]
    fn campaign_biomes_use_distinct_map_layouts() {
        let levels = get_levels();
        assert_eq!(levels[0].biome.layout, MapLayout::Meadow);
        assert_eq!(levels[4].biome.layout, MapLayout::Beach);
        assert_eq!(levels[6].biome.layout, MapLayout::Underwater);
        assert_eq!(levels[3].biome.layout, MapLayout::River);
        assert_eq!(levels[7].biome.layout, MapLayout::River);
        assert!(levels.iter().any(|level| level.biome.layout == MapLayout::Coast));
    }

    #[test]
    fn map_size_multipliers_cover_tutorial_to_campaign() {
        assert_eq!(MapSize::Tutorial.viewport_multiplier(), 1.0);
        assert_eq!(MapSize::Medium.viewport_multiplier(), 2.0);
        assert_eq!(MapSize::Large.viewport_multiplier(), 4.0);
    }

    #[test]
    fn only_tutorial_maps_skip_npc_trains() {
        assert!(!MapSize::Tutorial.spawns_npc_trains());
        assert!(MapSize::Medium.spawns_npc_trains());
        assert!(MapSize::Large.spawns_npc_trains());
    }

    #[test]
    fn every_campaign_level_has_the_designed_win_condition() {
        let levels = get_levels();
        assert_eq!(levels.len(), 9);
        assert_eq!(levels[0].win_condition, WinCondition::BankCrabs(25));
        assert_eq!(levels[1].win_condition, WinCondition::BuildTrain(15));
        assert_eq!(
            levels[2].win_condition,
            WinCondition::CrackAndHold { shells: 8, min_train: 15 }
        );
        assert_eq!(
            levels[3].win_condition,
            WinCondition::HoldTrain { target: 20, seconds: 30.0 }
        );
        assert_eq!(levels[4].win_condition, WinCondition::BuildTrain(24));
        assert_eq!(
            levels[5].win_condition,
            WinCondition::CrackAndHold { shells: 12, min_train: 18 }
        );
        assert_eq!(levels[6].win_condition, WinCondition::BankCrabs(55));
        assert_eq!(
            levels[7].win_condition,
            WinCondition::HoldTrain { target: 24, seconds: 36.0 }
        );
        assert_eq!(levels[8].win_condition, WinCondition::BankCrabs(40));
    }

    #[test]
    fn bosses_follow_their_biome_families() {
        let levels = get_levels();
        assert_eq!(levels[0].boss_for_encounter(0), CrabType::Boss);
        assert_eq!(levels[1].boss_for_encounter(0), CrabType::TideBoss);
        assert_eq!(levels[4].emphasis, Some(CrabType::Dancer));
        assert_eq!(levels[4].boss_for_encounter(0), CrabType::RhythmBoss);
        assert_eq!(levels[5].emphasis, Some(CrabType::Hermit));
        assert_eq!(levels[5].boss_for_encounter(0), CrabType::HermitKing);
        let desktop = levels.last().unwrap();
        assert_eq!(desktop.boss_for_encounter(4), CrabType::DancerKing);
        assert_eq!(desktop.boss_for_encounter(5), CrabType::Boss);
    }

    #[test]
    fn campaign_progression_has_unique_music_and_rising_difficulty() {
        use std::collections::HashSet;

        let levels = get_levels();
        let themes: HashSet<_> = levels.iter().map(|level| level.biome.music).collect();
        assert_eq!(themes.len(), levels.len());
        assert!(levels
            .windows(2)
            .all(|pair| pair[0].difficulty <= pair[1].difficulty));
    }

    #[test]
    fn win_condition_predicates_gate_correctly() {
        // BankCrabs cares only about the banked total.
        assert!(WinCondition::BankCrabs(25).met(25, 0, 0, 0.0));
        assert!(!WinCondition::BankCrabs(25).met(24, 99, 99, 99.0));
        // BuildTrain fires the instant the live train hits the target.
        assert!(WinCondition::BuildTrain(15).met(0, 15, 0, 0.0));
        assert!(!WinCondition::BuildTrain(15).met(99, 14, 0, 0.0));
        // CrackAndHold needs BOTH gates at once.
        let cah = WinCondition::CrackAndHold { shells: 8, min_train: 15 };
        assert!(cah.met(0, 15, 8, 0.0));
        assert!(!cah.met(0, 14, 8, 0.0));
        assert!(!cah.met(0, 15, 7, 0.0));
        // HoldTrain is satisfied purely by the accumulated hold time (the caller resets it when
        // the train dips below target).
        let hold = WinCondition::HoldTrain { target: 20, seconds: 30.0 };
        assert!(hold.met(0, 20, 0, 30.0));
        assert!(!hold.met(0, 20, 0, 29.9));
    }
}
