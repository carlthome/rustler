use crate::spawnings::SpawnPattern;

pub struct LevelPattern {
    pub pattern: SpawnPattern,
    pub count: usize,
    pub duration: f32,
    pub centroid: (f32, f32),
}

pub struct Level {
    pub title: String,
    pub description: String,
    pub difficulty: usize,
    pub patterns: Vec<LevelPattern>,
}

pub fn get_levels() -> Vec<Level> {
    vec![
        Level {
            title: "Rustler's First Ride".to_string(),
            description: "A beginner's level to get you started with the Rustler game.".to_string(),
            difficulty: 0,
            patterns: vec![
                LevelPattern {
                    pattern: SpawnPattern::SingleRandom,
                    count: 3,
                    duration: 10.0,
                    centroid: (0.5, 0.5),
                },
                LevelPattern {
                    pattern: SpawnPattern::SingleRandom,
                    count: 2,
                    duration: 8.0,
                    centroid: (0.2, 0.8),
                },
            ],
        },
        Level {
            title: "Rustler's Challenge".to_string(),
            description: "A challenging level with multiple spawn patterns.".to_string(),
            difficulty: 2,
            patterns: vec![
                LevelPattern {
                    pattern: SpawnPattern::UniformRandom,
                    count: 5,
                    duration: 8.0,
                    centroid: (0.7, 0.3),
                },
                LevelPattern {
                    pattern: SpawnPattern::SineWave,
                    count: 7,
                    duration: 10.0,
                    centroid: (0.3, 0.7),
                },
                LevelPattern {
                    pattern: SpawnPattern::Circle,
                    count: 8,
                    duration: 12.0,
                    centroid: (0.5, 0.5),
                },
                LevelPattern {
                    pattern: SpawnPattern::Cluster,
                    count: 10,
                    duration: 10.0,
                    centroid: (0.8, 0.8),
                },
                LevelPattern {
                    pattern: SpawnPattern::Cluster,
                    count: 6,
                    duration: 6.0,
                    centroid: (0.2, 0.2),
                },
            ],
        },
        Level {
            title: "Rustler's Gauntlet".to_string(),
            description: "A gauntlet of spawn patterns to test your skills.".to_string(),
            difficulty: 3,
            patterns: vec![
                LevelPattern {
                    pattern: SpawnPattern::Cluster,
                    count: 12,
                    duration: 10.0,
                    centroid: (0.5, 0.5),
                },
                LevelPattern {
                    pattern: SpawnPattern::SineWave,
                    count: 10,
                    duration: 12.0,
                    centroid: (0.8, 0.2),
                },
                LevelPattern {
                    pattern: SpawnPattern::Circle,
                    count: 14,
                    duration: 14.0,
                    centroid: (0.2, 0.8),
                },
                LevelPattern {
                    pattern: SpawnPattern::Cluster,
                    count: 8,
                    duration: 8.0,
                    centroid: (0.8, 0.8),
                },
                LevelPattern {
                    pattern: SpawnPattern::SineWave,
                    count: 6,
                    duration: 6.0,
                    centroid: (0.2, 0.2),
                },
            ],
        },
    ]
}
