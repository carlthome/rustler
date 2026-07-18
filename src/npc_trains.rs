/// NPC rival conga train logic — update (movement, stealing, collisions) and draw.
/// Extracted from main.rs so the ecology subsystem is navigable on its own.
use ggez::glam::Vec2;
use ggez::graphics::{Canvas, Color, DrawParam, Text};
use ggez::{Context, GameResult};
use rand::Rng;

use crate::enemies::{BossCharge, CrabType, EnemyCrab};
use crate::graphics::{draw_crab, unit_circle};
use crate::{MainState, NPC_NAME_CACHE, CRAB_SIZE, PLAYER_SIZE};

impl MainState {
    pub(crate) fn update_npc_trains(&mut self, dt: f32) {
        for i in 0..self.npc_trains.len() {
            if self.npc_trains[i].idle_timer > 0.0 {
                self.npc_trains[i].idle_timer -= dt;
                self.npc_trains[i].leader_vel *= (1.0 - 6.0 * dt).max(0.0);
                let cur = self.npc_trains[i].leader_pos;
                let last = self.npc_trains[i].path_history.front().copied().unwrap_or(cur);
                if cur.distance_squared(last) > 36.0 {
                    self.npc_trains[i].path_history.push_front(cur);
                }
                let dist_to_player = cur.distance(self.player_pos);
                self.npc_trains[i].target_vol = ((800.0 - dist_to_player) / 600.0).clamp(0.0, 1.0);
                continue;
            }

            let to_target = self.npc_trains[i].target - self.npc_trains[i].leader_pos;
            let dist = to_target.length();

            self.npc_trains[i].target_timer -= dt;
            if dist < 80.0 || self.npc_trains[i].target_timer <= 0.0 {
                let rng = &mut rand::rng();
                let idle_secs = rng.random_range(1.2_f32..3.5);
                self.npc_trains[i].idle_timer = idle_secs;
                let scale = self.npc_trains[i].leader_scale;
                let territory_bias = ((scale - 1.2) / 1.2).clamp(0.0, 1.0) * 0.65 + 0.2;
                let margin = 160.0;
                let ww = (self.world_width - margin).max(margin + 1.0);
                let wh = (self.world_height - margin).max(margin + 1.0);
                let rand_pt = Vec2::new(rng.random_range(margin..ww), rng.random_range(margin..wh));
                let tc = self.npc_trains[i].territory_center;
                let wander_radius = 380.0 - scale * 80.0;
                let angle: f32 = rng.random_range(0.0..std::f32::consts::TAU);
                let territory_pt = tc + Vec2::new(angle.cos(), angle.sin()) * wander_radius;
                let next_target = rand_pt.lerp(territory_pt, territory_bias);
                self.npc_trains[i].target = next_target.clamp(
                    Vec2::splat(margin),
                    Vec2::new(self.world_width - margin, self.world_height - margin),
                );
                self.npc_trains[i].target_timer = rng.random_range(18.0_f32..35.0);
            }

            let speed = match () {
                _ if self.npc_trains[i].leader_scale < 1.5 => 105.0,
                _ if self.npc_trains[i].leader_scale < 2.0 => 80.0,
                _ => 52.0,
            };
            let perp = Vec2::new(-to_target.y, to_target.x).normalize_or_zero();
            let wobble_phase = self.time_elapsed * 0.4 + i as f32 * 2.1;
            let wobble = perp * wobble_phase.sin() * 18.0;

            if dist > 1.0 {
                let desired = (to_target / dist + wobble / dist.max(1.0)) * speed;
                let steer_rate = if dist < 200.0 { 4.5 } else { 2.8 };
                let steer = (desired - self.npc_trains[i].leader_vel) * (steer_rate * dt);
                self.npc_trains[i].leader_vel += steer;
                if self.npc_trains[i].leader_vel.length() > speed {
                    self.npc_trains[i].leader_vel = self.npc_trains[i].leader_vel.normalize() * speed;
                }
            }
            let margin = 80.0;
            let vel_step = self.npc_trains[i].leader_vel * dt;
            self.npc_trains[i].leader_pos += vel_step;
            self.npc_trains[i].leader_pos.x = self.npc_trains[i].leader_pos.x
                .clamp(margin, self.world_width - margin);
            self.npc_trains[i].leader_pos.y = self.npc_trains[i].leader_pos.y
                .clamp(margin, self.world_height - margin);

            let cur_pos = self.npc_trains[i].leader_pos;
            let last = self.npc_trains[i].path_history.front().copied().unwrap_or(cur_pos);
            if cur_pos.distance_squared(last) > 36.0 {
                self.npc_trains[i].path_history.push_front(cur_pos);
                let max_len = self.npc_trains[i].follower_types.len() * 16 + 20;
                while self.npc_trains[i].path_history.len() > max_len {
                    self.npc_trains[i].path_history.pop_back();
                }
            }

            {
                let n = self.npc_trains[i].follower_types.len() as f32;
                let base = self.npc_trains[i].base_scale;
                self.npc_trains[i].leader_scale = (base + n * 0.09).min(3.8);
            }

            let dist_to_player = self.npc_trains[i].leader_pos.distance(self.player_pos);
            self.npc_trains[i].target_vol = ((800.0 - dist_to_player) / 600.0).clamp(0.0, 1.0);

            const PURSUIT_RANGE: f32 = 550.0;
            if self.chain_count >= 2
                && dist_to_player < PURSUIT_RANGE
                && self.npc_trains[i].idle_timer <= 0.0
            {
                if let Some(tail_pos) = self.cached_tail_pos {
                    let pursuit_blend =
                        ((PURSUIT_RANGE - dist_to_player) / PURSUIT_RANGE).clamp(0.0, 0.8);
                    self.npc_trains[i].target = self.npc_trains[i]
                        .target
                        .lerp(tail_pos, pursuit_blend * dt * 3.0);
                }
            }

            self.npc_trains[i].steal_cooldown = (self.npc_trains[i].steal_cooldown - dt).max(0.0);
            if self.npc_trains[i].steal_cooldown <= 0.0 && self.chain_count > 1 {
                const STEAL_RANGE: f32 = 58.0;
                let npc_pos = self.npc_trains[i].leader_pos;
                let chain_span = self.cached_tail_pos.map_or(0.0_f32, |t| t.distance(self.player_pos));
                let dist_to_chain = dist_to_player - chain_span;
                if dist_to_chain > STEAL_RANGE {
                    continue;
                }
                let splice_at = self.crabs.iter()
                    .filter(|c| c.caught && c.chain_index.map_or(false, |idx| idx > 0))
                    .filter(|c| npc_pos.distance(c.pos) < STEAL_RANGE)
                    .map(|c| c.chain_index.unwrap())
                    .min();

                if let Some(splice_idx) = splice_at {
                    let mut stolen_types: Vec<CrabType> = Vec::new();
                    let mut stolen_count = 0usize;
                    for crab in self.crabs.iter_mut() {
                        if crab.caught && crab.chain_index.map_or(false, |idx| idx >= splice_idx) {
                            crab.caught = false;
                            crab.chain_index = None;
                            crab.fleeing = false;
                            crab.spooked_timer = 1.0;
                            crab.join_pulse = 1.0;
                            let toward = (npc_pos - crab.pos).normalize_or_zero();
                            crab.vel = toward * 200.0;
                            crab.vel.y -= 90.0;
                            stolen_types.push(crab.crab_type);
                            stolen_count += 1;
                        }
                    }
                    if stolen_count > 0 {
                        self.chain_count = self.chain_count.saturating_sub(stolen_count);
                        self.npc_trains[i].follower_types.extend(stolen_types);
                        self.npc_trains[i].steal_cooldown = 2.2;
                        let npc_name = self.npc_trains[i].name.clone();
                        let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
                        self.floating_texts.spawn(
                            format!("{} stole {} crabs!", npc_name, stolen_count),
                            player_center - Vec2::new(110.0, 55.0),
                            30.0,
                            [0.96, 0.72, 0.16, 1.0],
                        );
                        self.screen_shake = self.screen_shake.max(10.0);
                        self.zoom_punch = self.zoom_punch.max(0.08);
                        self.groove = (self.groove - 0.15).max(0.0);
                        self.beat_streak = self.beat_streak.saturating_sub(2);
                        if self.catch_shockwaves.len() < 48 {
                            self.catch_shockwaves.push((npc_pos, 0.0, [0.96, 0.72, 0.16]));
                        }
                    }
                }
            }

            self.npc_trains[i].catch_cooldown = (self.npc_trains[i].catch_cooldown - dt).max(0.0);
            if self.npc_trains[i].catch_cooldown <= 0.0 {
                const CATCH_RANGE: f32 = 52.0;
                let npc_pos = self.npc_trains[i].leader_pos;
                let caught = self.crabs.iter_mut().find(|c| {
                    !c.caught && !c.is_boss() && c.is_catchable() && npc_pos.distance(c.pos) < CATCH_RANGE
                });
                if let Some(crab) = caught {
                    let ct = crab.crab_type;
                    crab.pos = Vec2::new(-9999.0, -9999.0);
                    crab.vel = Vec2::ZERO;
                    crab.fleeing = false;
                    self.npc_trains[i].follower_types.push(ct);
                    self.npc_trains[i].catch_cooldown = 0.7;
                }
            }
        }

        let max_vol = self.npc_trains.iter().map(|t| t.target_vol).fold(0.0_f32, f32::max);
        if !self.npc_trains.is_empty() {
            self.npc_trains[0].target_vol = max_vol;
        }

        {
            let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            for npc in self.npc_trains.iter_mut() {
                let col_r = CRAB_SIZE * npc.leader_scale * 1.2 + PLAYER_SIZE * 0.5;
                let dist = npc.leader_pos.distance(player_center);
                if dist < col_r && dist > 0.1 {
                    let overlap = col_r - dist;
                    let dir = (player_center - npc.leader_pos).normalize_or_zero();
                    self.player_pos += dir * overlap * 0.6;
                    npc.leader_pos -= dir * overlap * 0.4;
                    let rel_vel = self.player_vel - npc.leader_vel;
                    let sep_speed = rel_vel.dot(dir);
                    if sep_speed < 0.0 {
                        self.player_vel -= dir * sep_speed * 0.8;
                        npc.leader_vel += dir * sep_speed * 0.8;
                    }
                }
            }
        }

        if self.king_splice_cooldown <= 0.0 {
            let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            let mut clash_npc: Option<usize> = None;
            for (ni, npc) in self.npc_trains.iter().enumerate() {
                let col_r = CRAB_SIZE * npc.leader_scale * 1.2 + PLAYER_SIZE * 0.5;
                if npc.leader_pos.distance(player_center) < col_r {
                    clash_npc = Some(ni);
                    break;
                }
            }
            if let Some(ni) = clash_npc {
                self.king_splice_cooldown = 2.0;
                let away_from_npc = (player_center - self.npc_trains[ni].leader_pos).normalize_or_zero();
                self.player_vel += away_from_npc * 380.0;
                self.npc_trains[ni].leader_vel += -away_from_npc * 280.0;
                self.screen_shake = self.screen_shake.max(16.0);
                self.zoom_punch = self.zoom_punch.max(0.10);
                self.hitstop_timer = self.hitstop_timer.max(0.12);
                let player_lose = 2.min(self.chain_count.saturating_sub(1));
                let mut released = 0;
                for crab in self.crabs.iter_mut().rev() {
                    if released >= player_lose { break; }
                    if crab.caught {
                        if let Some(idx) = crab.chain_index {
                            if idx > 0 {
                                crab.caught = false;
                                crab.chain_index = None;
                                crab.fleeing = true;
                                crab.spooked_timer = 2.5;
                                crab.join_pulse = 1.0;
                                let away = (crab.pos - player_center).normalize_or_zero();
                                crab.vel = away * 250.0;
                                crab.vel.y -= 70.0;
                                if self.catch_shockwaves.len() < 48 {
                                    self.catch_shockwaves.push((crab.pos, 0.0, [1.0, 0.6, 0.2]));
                                }
                                released += 1;
                            }
                        }
                    }
                }
                self.chain_count = self.chain_count.saturating_sub(released);
                let npc_pos = self.npc_trains[ni].leader_pos;
                let npc_lose = 2.min(self.npc_trains[ni].follower_types.len());
                for k in 0..npc_lose {
                    self.npc_trains[ni].follower_types.pop();
                    let scatter_angle = k as f32 * std::f32::consts::PI + away_from_npc.y.atan2(away_from_npc.x);
                    let scatter_dir = Vec2::new(scatter_angle.cos(), scatter_angle.sin());
                    if self.catch_shockwaves.len() < 48 {
                        self.catch_shockwaves.push((npc_pos + scatter_dir * 30.0, 0.0, [0.96, 0.72, 0.16]));
                    }
                }
                self.groove = (self.groove - 0.20).max(0.0);
                self.beat_streak = self.beat_streak.saturating_sub(1);
                let npc_name = self.npc_trains[ni].name.clone();
                self.floating_texts.spawn(
                    format!("CLASH with {}!", npc_name),
                    player_center - Vec2::new(80.0, 65.0),
                    32.0,
                    [1.0, 0.5, 0.15, 1.0],
                );
                self.particle_system.spawn_milestone_fireworks(player_center, 8, &mut rand::rng());
            }
        }

        for i in 0..self.npc_trains.len() {
            for j in (i + 1)..self.npc_trains.len() {
                let pi = self.npc_trains[i].leader_pos;
                let pj = self.npc_trains[j].leader_pos;
                let si = self.npc_trains[i].leader_scale;
                let sj = self.npc_trains[j].leader_scale;
                let col_r = CRAB_SIZE * (si + sj) * 0.8;
                if pi.distance(pj) < col_r {
                    let dir = (pi - pj).normalize_or_zero();
                    self.npc_trains[i].leader_vel += dir * 200.0;
                    self.npc_trains[j].leader_vel -= dir * 200.0;
                    if !self.npc_trains[i].follower_types.is_empty() {
                        self.npc_trains[i].follower_types.pop();
                    }
                    if !self.npc_trains[j].follower_types.is_empty() {
                        self.npc_trains[j].follower_types.pop();
                    }
                }
            }
        }
    }

    pub(crate) fn draw_npc_conga_train(&self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        let t = self.time_elapsed;
        const STEPS: usize = 14;

        for npc in &self.npc_trains {
            for i in (0..npc.follower_types.len()).rev() {
                let hist_idx = (i + 1) * STEPS;
                let pos = match npc.path_history.get(hist_idx) {
                    Some(&p) => p,
                    None => continue,
                };
                let bob = (t * 5.5 + i as f32 * 0.85).sin() * 3.5;
                let crab_type = npc.follower_types[i];
                let fake = EnemyCrab {
                    pos,
                    vel: Vec2::ZERO,
                    speed: 0.0,
                    caught: true,
                    chain_index: Some(i),
                    scale: npc.leader_scale * 0.33,
                    spawn_time: 999.0,
                    crab_type,
                    spooked_timer: 0.0,
                    beat_phase_offset: i as f32 * 0.4,
                    join_pulse: 0.0,
                    fleeing: false,
                    facing_angle: 0.0,
                    in_flashlight: false,
                    startle_timer: 0.0,
                    charm_timer: 0.0,
                    answering_call: 0.0,
                    boss_health: 0.0,
                    boss_max_health: 0.0001,
                    enraged: false,
                    charge_state: BossCharge::Idle,
                    charge_cooldown: 0.0,
                    latch_timer: 0.0,
                    panic_amp: 1.0,
                    magnet_snared: 0.0,
                    magnet_lured: 0.0,
                    thief_lured: 0.0,
                    magnet_charged: 0.0,
                    slingshot_spent: 0.0,
                    stun_timer: 0.0,
                    host_swap_timer: 0.0,
                    surge_timer: 0.0,
                };
                let beat = (t * 4.0 + i as f32 * 0.5).sin().abs();
                draw_crab(ctx, canvas, &fake, pos + Vec2::new(0.0, -bob), beat, 0.0, bob.max(0.0), 0.0, t)?;
            }

            let leader_bob = (t * 4.2).sin() * 6.0;
            let facing = if npc.leader_vel.length_squared() > 4.0 {
                npc.leader_vel.y.atan2(npc.leader_vel.x)
            } else {
                0.0
            };
            let king = EnemyCrab {
                pos: npc.leader_pos,
                vel: npc.leader_vel,
                speed: 88.0,
                caught: false,
                chain_index: None,
                scale: npc.leader_scale,
                spawn_time: 999.0,
                crab_type: CrabType::Boss,
                spooked_timer: 0.0,
                beat_phase_offset: 0.0,
                join_pulse: 0.0,
                fleeing: false,
                facing_angle: facing,
                in_flashlight: false,
                startle_timer: 0.0,
                charm_timer: 0.0,
                answering_call: 0.0,
                boss_health: 0.0,
                boss_max_health: 0.0001,
                enraged: false,
                charge_state: BossCharge::Idle,
                charge_cooldown: 0.0,
                latch_timer: 0.0,
                panic_amp: 1.0,
                magnet_snared: 0.0,
                magnet_lured: 0.0,
                thief_lured: 0.0,
                magnet_charged: 0.0,
                slingshot_spent: 0.0,
                stun_timer: 0.0,
                host_swap_timer: 0.0,
                surge_timer: 0.0,
            };
            let king_beat = (t * 4.0).sin().abs();
            draw_crab(
                ctx, canvas, &king,
                npc.leader_pos + Vec2::new(0.0, -leader_bob),
                king_beat, 0.0, leader_bob.max(0.0), facing, t,
            )?;

            // Golden halo so the King reads as leader from across the world.
            let dot = unit_circle(ctx)?;
            for ring in (0..3).rev() {
                let r = 40.0 + ring as f32 * 14.0;
                let a = 0.06 - ring as f32 * 0.015;
                canvas.draw(dot, DrawParam::default()
                    .dest(npc.leader_pos)
                    .scale(Vec2::splat(r))
                    .color(Color::new(1.0, 0.82, 0.2, a)));
            }

            // Name plate floating above the King Crab — cached so glyphs aren't reshaped every frame.
            let name_w = NPC_NAME_CACHE.with(|c| -> GameResult<f32> {
                let mut cache = c.borrow_mut();
                let needs_rebuild = cache.as_ref().map_or(true, |(n, _, _)| n != &npc.name);
                if needs_rebuild {
                    let mut text = Text::new(npc.name.as_str());
                    text.set_scale(16.0);
                    let w = text.measure(ctx)?.x;
                    *cache = Some((npc.name.clone(), text, w));
                }
                Ok(cache.as_ref().unwrap().2)
            })?;
            NPC_NAME_CACHE.with(|c| {
                let cache = c.borrow();
                if let Some((_, text, _)) = cache.as_ref() {
                    let name_pos = npc.leader_pos - Vec2::new(name_w / 2.0, 55.0 + leader_bob);
                    canvas.draw(text, DrawParam::default()
                        .dest(name_pos + Vec2::splat(1.5))
                        .color(Color::from_rgba(0, 0, 0, 180)));
                    canvas.draw(text, DrawParam::default()
                        .dest(name_pos)
                        .color(Color::new(0.96, 0.82, 0.3, 0.95)));
                }
            });
        }
        Ok(())
    }
}
