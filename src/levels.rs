use crate::enemies::CrabType;
use crate::spawnings::SpawnPattern;

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
}

/// A visual "zone" a level takes place in. Gives each level a distinct read so runs feel like
/// they travel somewhere instead of one continuous space. `tint` is a multiply grade laid over
/// the whole ground; `pulse` recolors the on-beat flash to match the zone's mood; `terrain` is the
/// mechanical wrinkle its patches carry.
#[derive(Clone, Copy)]
pub struct Biome {
    pub name: &'static str,
    pub tint: (u8, u8, u8),
    pub pulse: (u8, u8, u8),
    pub terrain: TerrainKind,
}

pub struct Level {
    pub title: String,
    pub description: String,
    pub difficulty: usize,
    pub biome: Biome,
    /// The herd archetype this zone leans on — its "second half" of the gear-change. Terrain
    /// (above) changes how the ground routes; `emphasis` changes *what you're catching* so
    /// crossing a boundary visibly shifts play, not just the tint. A fraction of the herd roll is
    /// redirected to this type (see `CrabType::random_emphasized`). Paired thematically to the
    /// terrain: Water→Magnet (routing), Rock→Armored (shells to crack), Kelp→Thief (tail
    /// pressure). `None` on the beginner zone, which stays a clean, unflavored intro.
    pub emphasis: Option<CrabType>,
    pub patterns: Vec<LevelPattern>,
}

/// The player-facing name of a level's emphasized archetype, for the Control-style title banner.
/// Surfacing it on the card is what makes the boundary *read* as a gear-change instead of an
/// invisible probability bump — the zone announces its dominant threat as you cross into it.
pub fn emphasis_label(emphasis: Option<CrabType>) -> Option<&'static str> {
    match emphasis {
        Some(CrabType::Magnet) => Some("MAGNET SWARM"),
        Some(CrabType::Armored) => Some("ARMORED SHELLS"),
        Some(CrabType::Thief) => Some("THIEF INFESTATION"),
        Some(CrabType::Dancer) => Some("DANCER RAVE"),
        Some(CrabType::Hermit) => Some("HERMIT WARREN"),
        _ => None,
    }
}

pub fn get_levels() -> Vec<Level> {
    vec![
        Level {
            title: "Rustler's First Ride".to_string(),
            description: "A beginner's level to get you started with the Rustler game.".to_string(),
            difficulty: 0,
            biome: Biome {
                name: "Sunny Meadow",
                tint: (255, 248, 214),
                pulse: (120, 255, 120),
                terrain: TerrainKind::Open,
            },
            emphasis: None,
            patterns: vec![
                LevelPattern {
                    pattern: SpawnPattern::SingleRandom,
                    count: 3,
                    duration: 14.0,
                    centroid: (0.5, 0.5),
                },
                LevelPattern {
                    pattern: SpawnPattern::SingleRandom,
                    count: 2,
                    duration: 11.2,
                    centroid: (0.2, 0.8),
                },
            ],
        },
        Level {
            title: "Rustler's Challenge".to_string(),
            description: "A challenging level with multiple spawn patterns.".to_string(),
            difficulty: 2,
            biome: Biome {
                name: "Tide Pools",
                tint: (150, 215, 255),
                pulse: (90, 200, 255),
                terrain: TerrainKind::Water,
            },
            // Water routes the herd; the Magnet reroutes it again by clustering free crabs — the
            // zone becomes a routing puzzle where you catch a Magnet to net the blob it gathered.
            emphasis: Some(CrabType::Magnet),
            patterns: vec![
                LevelPattern {
                    pattern: SpawnPattern::UniformRandom,
                    count: 5,
                    duration: 11.2,
                    centroid: (0.7, 0.3),
                },
                LevelPattern {
                    pattern: SpawnPattern::SineWave,
                    count: 7,
                    duration: 14.0,
                    centroid: (0.3, 0.7),
                },
                LevelPattern {
                    pattern: SpawnPattern::Circle,
                    count: 8,
                    duration: 16.8,
                    centroid: (0.5, 0.5),
                },
                LevelPattern {
                    pattern: SpawnPattern::Cluster,
                    count: 10,
                    duration: 14.0,
                    centroid: (0.8, 0.8),
                },
                LevelPattern {
                    pattern: SpawnPattern::Cluster,
                    count: 6,
                    duration: 8.4,
                    centroid: (0.2, 0.2),
                },
            ],
        },
        Level {
            title: "Rustler's Gauntlet".to_string(),
            description: "A gauntlet of spawn patterns to test your skills.".to_string(),
            difficulty: 3,
            biome: Biome {
                name: "Rocky Shore",
                tint: (178, 192, 208),
                pulse: (205, 222, 235),
                terrain: TerrainKind::Rock,
            },
            // Rocky chokepoints already make you thread the train; the Armored emphasis makes you
            // reach for the Stomp constantly — a zone of shells to crack while dodging the rocks.
            emphasis: Some(CrabType::Armored),
            patterns: vec![
                LevelPattern {
                    pattern: SpawnPattern::Cluster,
                    count: 12,
                    duration: 14.0,
                    centroid: (0.5, 0.5),
                },
                LevelPattern {
                    pattern: SpawnPattern::SineWave,
                    count: 10,
                    duration: 16.8,
                    centroid: (0.8, 0.2),
                },
                LevelPattern {
                    pattern: SpawnPattern::Circle,
                    count: 14,
                    duration: 19.6,
                    centroid: (0.2, 0.8),
                },
                LevelPattern {
                    pattern: SpawnPattern::Cluster,
                    count: 8,
                    duration: 11.2,
                    centroid: (0.8, 0.8),
                },
                LevelPattern {
                    pattern: SpawnPattern::SineWave,
                    count: 6,
                    duration: 8.4,
                    centroid: (0.2, 0.2),
                },
            ],
        },
        Level {
            title: "Crab Rave".to_string(),
            description: "The dance floor is packed. Catch them all!".to_string(),
            difficulty: 4,
            biome: Biome {
                name: "Neon Kelp Forest",
                tint: (120, 185, 150),
                pulse: (255, 90, 220),
                terrain: TerrainKind::Kelp,
            },
            // Kelp already snags your tail loose; a Thief infestation gnaws at it too — the whole
            // zone is one long fight to defend the train you've built. Tail pressure squared.
            emphasis: Some(CrabType::Thief),
            patterns: vec![
                LevelPattern {
                    pattern: SpawnPattern::BeatGrid,
                    count: 9,
                    duration: 16.8,
                    centroid: (0.5, 0.5),
                },
                LevelPattern {
                    pattern: SpawnPattern::Spiral,
                    count: 12,
                    duration: 19.6,
                    centroid: (0.5, 0.5),
                },
                LevelPattern {
                    pattern: SpawnPattern::BeatGrid,
                    count: 16,
                    duration: 19.6,
                    centroid: (0.5, 0.5),
                },
                LevelPattern {
                    pattern: SpawnPattern::Spiral,
                    count: 20,
                    duration: 22.4,
                    centroid: (0.5, 0.5),
                },
            ],
        },
    ]
}
