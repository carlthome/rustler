use crate::enemies::{CrabType, EnemyCrab};
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

fn make_crab(pos: Vec2, vel: Vec2, spawn_time: f32, rng: &mut impl Rng) -> EnemyCrab {
    let crab_type = CrabType::random(rng);
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
    }
}

pub fn spawn_enemies(
    pattern: SpawnPattern,
    count: usize,
    area: (f32, f32),
    centroid: (f32, f32),
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
                make_crab(pos, vel, 0.0, rng)
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
                    make_crab(pos, vel, 0.0, rng)
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
                    make_crab(pos, vel, 0.0, rng)
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
                    make_crab(pos, vel, 0.0, rng)
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
                    make_crab(pos, vel, i as f32 * delay, rng)
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
                    make_crab(pos, vel, 0.0, rng)
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
                make_crab(pos, vel, 0.0, rng)
            })
            .collect(),
    }
}
