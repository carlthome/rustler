//! The particle system — the transient dot/spark effects (milestone fireworks, catch
//! bursts, dash trails, geyser spray, etc.): the `Particle`/`ParticleSystem` types, the
//! `ParticleUniform` shader input, spawn/update logic, and the batched `draw_particles`
//! pass. Extracted from `graphics/mod.rs` to keep that file navigable; leans on the shared
//! cached meshes and helpers in the parent module (reached here via `use super::*`).

use super::*;

thread_local! {
    // Reusable instance buffers for the particle system's two draw passes (main dot + soft glow
    // for larger particles). Milestone fireworks/catch bursts/dash trails can push close to
    // MAX_PARTICLES (900) live particles at once, and each one used to cost its own
    // `canvas.draw` call — a separate uniform-buffer allocation, bind group and `draw_indexed`
    // submission per particle per pass (ggez does NOT batch consecutive `canvas.draw(&same_mesh,
    // ...)` calls; only `InstanceArray` is truly instanced). That was up to ~1800 GPU draw calls
    // a frame just for particles. Filling one `InstanceArray` per pass and issuing a single
    // `draw_instanced_mesh` collapses that to two draw calls total, independent of particle
    // count, with identical on-screen output (same mesh, same blend mode, no rotation to lose).
    static PARTICLE_MAIN_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    static PARTICLE_GLOW_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
}

#[derive(Copy, Clone, Debug, AsStd140)]
pub struct ParticleUniform {
    pub screen_width: f32,
    pub screen_height: f32,
    pub time: f32,
    pub _padding: f32,
}

#[derive(Clone, Debug)]
pub struct Particle {
    pub pos: Vec2,
    pub vel: Vec2,
    pub life: f32,
    pub max_life: f32,
    pub size: f32,
    pub color: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct ParticleSystem {
    pub particles: Vec<Particle>,
}

// Hard ceiling on live particles. Several emitters (beat pulses, conga dust) scale their
// spawn rate with conga-chain length, which grows unbounded over a long run — without a cap,
// a big train can pile up thousands of live sparkles and tank frame time even though each
// individual draw call is cheap on its own. Capping here (instead of per-emitter) keeps every
// effect's look identical for normal-sized trains and only kicks in once the screen is already
// visually saturated with particles, so it reads as "same effect" rather than "less effect."
const MAX_PARTICLES: usize = 900;

impl ParticleSystem {
    pub fn new() -> Self {
        Self {
            particles: Vec::new(),
        }
    }

    /// Push a particle unless the live-particle budget is already spent. Newest triggers (the
    /// catch/beat/dash the player just caused) matter most, so once at the cap we simply drop
    /// further spawns for this frame rather than evicting older ones — cheaper than a shift,
    /// and anything dropped would already be buried in the crowd.
    #[inline]
    pub(crate) fn push(&mut self, particle: Particle) {
        if self.particles.len() < MAX_PARTICLES {
            self.particles.push(particle);
        }
    }

    pub fn spawn_catch_effect(&mut self, pos: Vec2, crab_color: [f32; 3], crab_type: CrabType, rng: &mut impl Rng) {
        let (particle_count, speed_range, size_range, special_effect) = match crab_type {
            CrabType::Normal => (20, 80.0..180.0, 3.0..6.0, false),
            CrabType::Fast => (35, 120.0..300.0, 2.0..5.0, true), // More particles, faster
            CrabType::Big => (40, 60.0..150.0, 4.0..10.0, false), // Larger particles
            CrabType::Sneaky => (15, 100.0..250.0, 1.5..4.0, true), // Fewer, sneaky particles
            CrabType::Armored => (40, 60.0..150.0, 4.0..10.0, false), // Chunky, shell-cracking burst
            CrabType::Dancer => (30, 110.0..280.0, 2.0..5.0, true), // Lively disco confetti burst
            CrabType::Magnet => (45, 90.0..260.0, 3.0..7.0, true),  // Chunky lodestone burst — the cluster pops with it
            CrabType::Thief => (28, 120.0..300.0, 2.0..5.0, true),  // Wiry poison-green burst — catching it feels like relief
            CrabType::Hermit => (42, 70.0..200.0, 3.0..8.0, true),  // Chunky coppery shell-shard burst — the borrowed shell scatters as it pops out
            CrabType::Golden => (55, 100.0..320.0, 2.5..7.0, true), // Lavish gold coin-burst — the treasure catch pops
            CrabType::Splitter => (48, 130.0..340.0, 2.5..6.0, true), // Fast bright teal cleave-burst that flings apart — reads as the train splitting open
            CrabType::Boss => (70, 90.0..320.0, 4.0..13.0, true),   // Huge celebratory burst
            CrabType::TideBoss => (70, 90.0..320.0, 4.0..13.0, true), // Huge tidal splash burst
            CrabType::RhythmBoss => (70, 90.0..320.0, 4.0..13.0, true), // Huge violet disco burst
        };

        // Create main particles
        for _ in 0..particle_count {
            let angle = rng.random_range(0.0..std::f32::consts::TAU);
            let speed = rng.random_range(speed_range.clone());
            let vel = Vec2::new(angle.cos(), angle.sin()) * speed;
            let life = rng.random_range(0.8..1.8);
            let size = rng.random_range(size_range.clone());

            // Add some color variation and make particles brighter
            let color_variation = rng.random_range(-0.2..0.2);
            let brightness_boost = rng.random_range(0.3..0.7);
            let particle_color = [
                (crab_color[0] + color_variation + brightness_boost).clamp(0.0, 1.0),
                (crab_color[1] + color_variation + brightness_boost).clamp(0.0, 1.0),
                (crab_color[2] + color_variation + brightness_boost).clamp(0.0, 1.0),
            ];

            self.push(Particle {
                pos,
                vel,
                life,
                max_life: life,
                size,
                color: particle_color,
            });
        }

        // Add special sparkly particles for Fast and Sneaky crabs
        if special_effect {
            let sparkle_count = match crab_type {
                CrabType::Fast => 15,
                CrabType::Sneaky => 8,
                CrabType::Dancer => 14,
                CrabType::Magnet => 12,
                CrabType::Thief => 10,
                CrabType::Golden => 20, // a lavish shower of gold sparkles for the treasure catch
                _ => 0,
            };

            for _ in 0..sparkle_count {
                let angle = rng.random_range(0.0..std::f32::consts::TAU);
                let speed = rng.random_range(150.0..400.0);
                let vel = Vec2::new(angle.cos(), angle.sin()) * speed;
                let life = rng.random_range(0.4..1.0);
                let size = rng.random_range(1.0..3.0);

                let sparkle_color = match crab_type {
                    CrabType::Fast => [1.0, 0.8, 0.2], // Golden sparkles for fast crabs
                    CrabType::Sneaky => [0.7, 0.9, 1.0], // Blue sparkles for sneaky crabs
                    CrabType::Dancer => [1.0, 0.5, 0.95], // Hot-pink disco confetti
                    CrabType::Magnet => [1.0, 0.55, 0.2], // Molten lodestone sparks
                    CrabType::Thief => [0.5, 1.0, 0.6],   // Poison-green thief sparks
                    CrabType::Golden => [1.0, 0.85, 0.25], // Bright treasure-gold coin sparks
                    _ => [1.0, 1.0, 0.9],
                };

                self.push(Particle {
                    pos,
                    vel,
                    life,
                    max_life: life,
                    size,
                    color: sparkle_color,
                });
            }
        } else {
            // Regular sparkles for Normal and Big crabs
            for _ in 0..8 {
                let angle = rng.random_range(0.0..std::f32::consts::TAU);
                let speed = rng.random_range(120.0..300.0);
                let vel = Vec2::new(angle.cos(), angle.sin()) * speed;
                let life = rng.random_range(0.4..0.8);
                let size = rng.random_range(1.5..4.0);

                self.push(Particle {
                    pos,
                    vel,
                    life,
                    max_life: life,
                    size,
                    color: [1.0, 1.0, 0.9], // Bright white/yellow sparkles
                });
            }
        }
    }

    pub fn spawn_movement_trail(&mut self, pos: Vec2, velocity: Vec2, time: f32, rng: &mut impl Rng) {
        let speed = velocity.length();
        if speed < 15.0 {
            return;
        }
        let count = ((speed / 60.0) as usize).clamp(1, 5);
        for _ in 0..count {
            // Cycle hue over time for a rainbow trail
            let hue = (time * 0.6 + pos.x * 0.003 + pos.y * 0.002) % 1.0;
            let r = ((hue * 6.0 - 3.0).abs() - 1.0).clamp(0.0, 1.0);
            let g = (2.0 - (hue * 6.0 - 2.0).abs()).clamp(0.0, 1.0);
            let b = (2.0 - (hue * 6.0 - 4.0).abs()).clamp(0.0, 1.0);
            let spread_angle = rng.random_range(0.0..std::f32::consts::TAU);
            let spread_dist = rng.random_range(0.0..12.0);
            let vel = Vec2::new(spread_angle.cos(), spread_angle.sin()) * spread_dist
                - velocity * 0.08;
            let life = rng.random_range(0.12..0.30);
            self.push(Particle {
                pos: pos + Vec2::new(
                    rng.random_range(-5.0..5.0),
                    rng.random_range(-5.0..5.0),
                ),
                vel,
                life,
                max_life: life,
                size: rng.random_range(2.0..5.5),
                color: [r, g, b],
            });
        }
    }

    /// Kick up a small warm dust puff from a conga-train crab's feet as it scuttles along.
    /// Called once per caught crab per frame; internally throttled so the emission rate is
    /// framerate-independent and only fires while the crab is actually moving. Because every
    /// crab in the train emits, a longer conga line kicks up a bigger, more spectacular cloud
    /// — the visual payoff scales with how many crabs you've caught. `move_delta` is the crab's
    /// per-frame position change; `dt` the frame time.
    /// Kick up conga-train dust. `move_len` is the pre-computed `move_delta.length()` and
    /// `move_speed = move_len / dt` (both already calculated by the caller for the facing-angle
    /// update and normalize), so this function avoids the redundant sqrts that used to run per
    /// chain-link per frame even when the train barely moved.
    pub fn spawn_conga_dust(&mut self, pos: Vec2, move_delta: Vec2, dt: f32, move_len: f32, move_speed: f32, rng: &mut impl Rng) {
        if move_speed < 40.0 {
            return;
        }
        // ~10-18 puffs/sec per crab, a touch faster the quicker it's moving. Probability per
        // frame = rate * dt, so total emission is stable regardless of FPS.
        let rate = (10.0 + move_speed * 0.02).min(18.0);
        if rng.random::<f32>() > rate * dt {
            return;
        }
        // Normalize using the pre-computed length to skip a second sqrt.
        let back = if move_len > 1e-6 { -move_delta / move_len } else { Vec2::ZERO };
        let perp = Vec2::new(-back.y, back.x);
        // Mostly backward, with a little sideways scatter and a gentle upward kick so the puff
        // rises before the particle system's gravity settles it back down.
        let vel = back * rng.random_range(15.0..45.0)
            + perp * rng.random_range(-18.0..18.0)
            + Vec2::new(0.0, rng.random_range(-40.0..-15.0));
        let life = rng.random_range(0.30..0.6);
        // Warm sandy tone; drawn additively so keep it dim — reads as a soft haze, not a blob.
        let shade = rng.random_range(0.0..0.08);
        self.push(Particle {
            pos: pos + Vec2::new(rng.random_range(-4.0..4.0), rng.random_range(-3.0..3.0)),
            vel,
            life,
            max_life: life,
            size: rng.random_range(2.5..3.9),
            color: [0.34 + shade, 0.28 + shade, 0.20 + shade],
        });
    }

    pub fn spawn_dash_burst(&mut self, pos: Vec2, move_dir: Vec2, rng: &mut impl Rng) {
        // shoot particles mostly backward from the movement direction, spread into a fan
        let back = if move_dir.length() > 0.1 { -move_dir.normalize() } else { Vec2::new(0.0, 1.0) };
        for _ in 0..30 {
            let spread = rng.random_range(-0.9_f32..0.9_f32);
            let perp = Vec2::new(-back.y, back.x);
            let dir = (back + perp * spread).normalize();
            let speed = rng.random_range(160.0_f32..480.0_f32);
            let life = rng.random_range(0.18_f32..0.40_f32);
            // Cyan-white colour with slight variation
            let g = rng.random_range(0.85_f32..1.0_f32);
            self.push(Particle {
                pos: pos + Vec2::new(rng.random_range(-6.0_f32..6.0_f32), rng.random_range(-6.0_f32..6.0_f32)),
                vel: dir * speed,
                life,
                max_life: life,
                size: rng.random_range(3.0_f32..7.5_f32),
                color: [0.7, g, 1.0],
            });
        }
    }

    pub fn spawn_beat_pulse(&mut self, positions: &[Vec2], beat_intensity: f32, chain_len: usize, rng: &mut impl Rng) {
        if positions.is_empty() { return; }
        // Scale ring size with chain length — bigger train = bigger burst
        let base_count = (4 + chain_len / 3).min(16) as usize;
        let ring_speed = 180.0 + beat_intensity * 120.0;
        for &center in positions {
            for j in 0..base_count {
                let angle = (j as f32 / base_count as f32) * std::f32::consts::TAU;
                // Slight random spread on the angle so rings aren't perfectly geometric
                let spread = rng.random_range(-0.18_f32..0.18_f32);
                let dir = Vec2::new((angle + spread).cos(), (angle + spread).sin());
                let speed = ring_speed * rng.random_range(0.7_f32..1.3_f32);
                let life = rng.random_range(0.25_f32..0.55_f32);
                // Rainbow hue per particle, offset by position for variety
                let hue = (angle / std::f32::consts::TAU + center.x * 0.001 + center.y * 0.0007) % 1.0;
                let r = ((hue * 6.0 - 3.0).abs() - 1.0).clamp(0.0, 1.0);
                let g = (2.0 - (hue * 6.0 - 2.0).abs()).clamp(0.0, 1.0);
                let b = (2.0 - (hue * 6.0 - 4.0).abs()).clamp(0.0, 1.0);
                self.push(Particle {
                    pos: center,
                    vel: dir * speed,
                    life,
                    max_life: life,
                    size: rng.random_range(2.0_f32..5.5_f32),
                    color: [r, g, b],
                });
            }
        }
    }

    /// The biggest hit in the game: a King Crab charge landed a DIRECT player hit and the whole
    /// conga line just exploded outward like Sonic dropping his rings. Throws a dense radial burst
    /// of hot alarm-coloured motes plus a fast expanding debris ring so the moment reads as a
    /// catastrophe you can still recover from. `radius_scale` widens the blast on a downbeat hit.
    pub fn spawn_train_break_burst(&mut self, center: Vec2, radius_scale: f32, rng: &mut impl Rng) {
        // Dense outward shockwave of debris.
        let count = 64;
        for j in 0..count {
            let angle = (j as f32 / count as f32) * std::f32::consts::TAU
                + rng.random_range(-0.15_f32..0.15_f32);
            let dir = Vec2::new(angle.cos(), angle.sin());
            let speed = rng.random_range(280.0_f32..640.0_f32) * radius_scale;
            let life = rng.random_range(0.35_f32..0.75_f32);
            // Hot red→orange→yellow embers — danger, not celebration.
            let t = rng.random_range(0.0_f32..1.0_f32);
            let color = [1.0, 0.35 + t * 0.5, 0.2 + t * 0.2];
            self.push(Particle {
                pos: center,
                vel: dir * speed,
                life,
                max_life: life,
                size: rng.random_range(3.5_f32..9.0_f32),
                color,
            });
        }
        // A brighter, tighter inner flash ring for the impact core.
        for j in 0..24 {
            let angle = (j as f32 / 24.0) * std::f32::consts::TAU;
            let dir = Vec2::new(angle.cos(), angle.sin());
            let life = rng.random_range(0.18_f32..0.32_f32);
            self.push(Particle {
                pos: center,
                vel: dir * rng.random_range(120.0_f32..260.0_f32),
                life,
                max_life: life,
                size: rng.random_range(4.0_f32..8.0_f32),
                color: [1.0, 0.95, 0.7],
            });
        }
    }

    /// A soft warm puff of gently-rising motes off a crab the whistle just talked down out of a
    /// panic — the calming counterpart to the cold "!" alarm rings the stampede throws.
    pub fn spawn_soothe_puff(&mut self, pos: Vec2, rng: &mut impl Rng) {
        for _ in 0..6 {
            let angle = rng.random_range(0.0_f32..std::f32::consts::TAU);
            let speed = rng.random_range(12.0_f32..40.0_f32);
            // Drift outward but bias upward so the puff wafts off like a settling sigh.
            let vel = Vec2::new(angle.cos() * speed, angle.sin() * speed - rng.random_range(20.0_f32..55.0_f32));
            let life = rng.random_range(0.5_f32..0.95_f32);
            self.push(Particle {
                pos,
                vel,
                life,
                max_life: life,
                size: rng.random_range(2.5_f32..5.0_f32),
                color: [1.0, 0.82 + rng.random_range(-0.08_f32..0.08_f32), 0.42],
            });
        }
    }

    /// A dry sandy puff kicked up where an EMPTY lasso loop slaps down. This is the miss-feedback:
    /// a throw that finds no crab still lands with a legible little dust burst, so whiffing reads as
    /// a real (if fruitless) throw rather than the loop silently vanishing. Sand flings outward and
    /// low — a flat ring hugging the ground, distinct from the warm rising motes of a catch.
    pub fn spawn_lasso_dust(&mut self, pos: Vec2, rng: &mut impl Rng) {
        for _ in 0..14 {
            let angle = rng.random_range(0.0_f32..std::f32::consts::TAU);
            let speed = rng.random_range(40.0_f32..130.0_f32);
            // Fling outward, mostly flat, with only a small upward hop so it settles fast like sand.
            let vel = Vec2::new(angle.cos() * speed, angle.sin() * speed * 0.5 - rng.random_range(5.0_f32..30.0_f32));
            let life = rng.random_range(0.22_f32..0.45_f32);
            let shade = rng.random_range(0.0_f32..0.1_f32);
            self.push(Particle {
                pos: pos + Vec2::new(rng.random_range(-5.0..5.0), rng.random_range(-4.0..4.0)),
                vel,
                life,
                max_life: life,
                size: rng.random_range(2.0_f32..4.2_f32),
                color: [0.72 + shade, 0.63 + shade, 0.44 + shade],
            });
        }
    }

    /// A King Crab fissure GEYSERS on the beat: fling a short burst of molten debris up out of the
    /// pit. Launched near-vertically with a little spread so gravity (applied in `update`) arcs it
    /// back down like sparks off lava — a rhythmic spout that reads as the hazard surging, not just
    /// glowing. Particle count scales gently with pit radius; the shared MAX_PARTICLES cap guards it.
    pub fn spawn_fissure_geyser(&mut self, center: Vec2, radius: f32, rng: &mut impl Rng) {
        let count = (6.0 + radius * 0.08) as usize;
        for _ in 0..count {
            // Spawn from somewhere inside the pit mouth so the column has width, not a single jet.
            let off_a = rng.random_range(0.0_f32..std::f32::consts::TAU);
            let off_r = rng.random_range(0.0_f32..radius * 0.5);
            let pos = center + Vec2::new(off_a.cos() * off_r, off_a.sin() * off_r);
            // Mostly upward with a slight sideways fan.
            let up = rng.random_range(180.0_f32..340.0_f32);
            let side = rng.random_range(-70.0_f32..70.0_f32);
            let life = rng.random_range(0.35_f32..0.75_f32);
            // Hot molten palette: orange core flecked toward yellow-white.
            let heat = rng.random_range(0.0_f32..1.0);
            let color = [1.0, 0.4 + 0.45 * heat, 0.08 + 0.25 * heat];
            self.push(Particle {
                pos,
                vel: Vec2::new(side, -up),
                life,
                max_life: life,
                size: rng.random_range(2.0_f32..4.5_f32),
                color,
            });
        }
    }

    pub fn spawn_milestone_fireworks(&mut self, center: Vec2, milestone: usize, rng: &mut impl Rng) {
        // Scale particle count with milestone tier, capped at 200
        let count = (120 + (milestone / 5).min(8) * 10).min(200);

        // --- Color burst pass ---
        for i in 0..count {
            let angle = rng.random_range(0.0_f32..std::f32::consts::TAU);
            let speed = rng.random_range(200.0_f32..600.0_f32);
            // Bias direction upward: subtract from y so particles tend to shoot upward
            let upward_bias = rng.random_range(100.0_f32..300.0_f32);
            let vel = Vec2::new(angle.cos() * speed, angle.sin() * speed - upward_bias);
            let life = rng.random_range(1.2_f32..2.8_f32);
            // Full rainbow: spread hue evenly across particles with random jitter
            let hue = ((i as f32 / count as f32) + rng.random_range(-0.05_f32..0.05_f32)).rem_euclid(1.0);
            let r = ((hue * 6.0 - 3.0).abs() - 1.0).clamp(0.0, 1.0);
            let g = (2.0 - (hue * 6.0 - 2.0).abs()).clamp(0.0, 1.0);
            let b = (2.0 - (hue * 6.0 - 4.0).abs()).clamp(0.0, 1.0);
            self.push(Particle {
                pos: center,
                vel,
                life,
                max_life: life,
                size: rng.random_range(4.0_f32..12.0_f32),
                color: [r, g, b],
            });
        }

        // --- Sparkle pass: 30 bright white/yellow "star" particles ---
        for _ in 0..30 {
            let angle = rng.random_range(0.0_f32..std::f32::consts::TAU);
            let speed = rng.random_range(300.0_f32..700.0_f32);
            let upward_bias = rng.random_range(100.0_f32..250.0_f32);
            let vel = Vec2::new(angle.cos() * speed, angle.sin() * speed - upward_bias);
            let life = rng.random_range(0.6_f32..1.2_f32);
            // Alternate between pure white and bright yellow for sparkle variety
            let color = if rng.random_range(0.0_f32..1.0_f32) < 0.5 {
                [1.0, 1.0, 1.0] // white
            } else {
                [1.0, 0.95, 0.3] // bright yellow
            };
            self.push(Particle {
                pos: center,
                vel,
                life,
                max_life: life,
                size: rng.random_range(2.0_f32..5.0_f32),
                color,
            });
        }
    }

    pub fn update(&mut self, dt: f32) {
        self.particles.retain_mut(|particle| {
            particle.life -= dt;
            particle.pos += particle.vel * dt;

            // Add gravity effect
            particle.vel.y += 200.0 * dt;

            // Add air resistance
            particle.vel *= 0.96;

            // Shrink particles over time
            let life_ratio = particle.life / particle.max_life;
            particle.size = particle.size * (0.95 + 0.05 * life_ratio);

            particle.life > 0.0
        });
    }
}

pub fn draw_particles(
    ctx: &mut Context,
    canvas: &mut Canvas,
    particle_system: &ParticleSystem,
) -> ggez::GameResult {
    let unit_circle = match UNIT_CIRCLE.get() {
        Some(mesh) => mesh.clone(),
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh).clone()
        }
    };

    if particle_system.particles.is_empty() {
        return Ok(());
    }

    // Set additive blend mode for glowing effect
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    PARTICLE_MAIN_INSTANCES.with(|main_cell| -> ggez::GameResult {
        PARTICLE_GLOW_INSTANCES.with(|glow_cell| -> ggez::GameResult {
            let mut main_slot = main_cell.borrow_mut();
            let main = main_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
            let mut glow_slot = glow_cell.borrow_mut();
            let glow = glow_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));

            main.set(particle_system.particles.iter().map(|particle| {
                let life_ratio = particle.life / particle.max_life;
                let alpha = (life_ratio * 0.8).clamp(0.0, 1.0);
                let color = Color::new(
                    particle.color[0],
                    particle.color[1],
                    particle.color[2],
                    alpha,
                );
                DrawParam::default()
                    .dest(particle.pos)
                    .scale(Vec2::splat(particle.size))
                    .color(color)
            }));

            glow.set(particle_system.particles.iter().filter(|p| p.size > 4.0).map(|particle| {
                let life_ratio = particle.life / particle.max_life;
                let alpha = (life_ratio * 0.8).clamp(0.0, 1.0);
                let glow_color = Color::new(
                    particle.color[0],
                    particle.color[1],
                    particle.color[2],
                    alpha * 0.3,
                );
                DrawParam::default()
                    .dest(particle.pos)
                    .scale(Vec2::splat(particle.size * 1.5))
                    .color(glow_color)
            }));

            // Both passes guarded: ggez's flush_wgpu asserts capacity > 0 if called on an
            // InstanceArray that was set() with 0 items. Always skip the draw when empty.
            if !main.instances().is_empty() {
                canvas.draw_instanced_mesh_guarded(unit_circle.clone(), main, DrawParam::default());
            }
            if !glow.instances().is_empty() {
                canvas.draw_instanced_mesh_guarded(unit_circle, glow, DrawParam::default());
            }
            Ok(())
        })
    })?;

    // Restore original blend mode
    canvas.set_blend_mode(original_blend);
    Ok(())
}
