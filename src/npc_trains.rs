//! Rival NPC King Crab trains: the wandering AI that patrols the world, threads through the
//! player's conga line to splice a steal (and can be threaded back the other way), collides with
//! the player and with other NPC trains, and the rendering that draws each train's followers and
//! golden leader. Extracted out of `main.rs`'s `impl MainState` — same methods, same behaviour,
//! just grouped by subsystem instead of living in one file.

use ggez::glam::Vec2;
use ggez::graphics::{Canvas, Color, DrawParam, Text};
use ggez::{Context, GameResult};
use rand::Rng;

use crate::constants::*;
use crate::enemies::{BossCharge, CrabType, EnemyCrab};
use crate::graphics::{draw_crab, unit_circle};
use crate::hud_cache::NPC_NAME_CACHE;
use crate::spawnings::spawn_stolen_crab;
use crate::state::MainState;

impl MainState {
    /// Bot-test helper (see BotAction::ForceNpcCross): deterministically stage the reverse-Snake
    /// steal. Teleport the nearest rival NPC King Crab train's leader onto a mid-chain link of the
    /// player's conga line and clear its steal cooldown, so `update_npc_trains`' splice fires this
    /// frame. A no-op when there's nothing stealable (no NPC trains, or a chain shorter than 2). This
    /// exercises the real detection + detachment + follower-transfer path; only the rival's pathing
    /// (which is RNG-timed and can't be counted on inside a headless budget) is shortcut.
    pub fn force_npc_cross(&mut self) {
        if self.npc_trains.is_empty() || self.chain_count < 2 {
            return;
        }
        // Aim for a mid-chain link (never the head, index 0 — the head can't be spliced). Collect the
        // caught links with index > 0 and pick the one nearest the middle so the splice takes a
        // meaningful tail section rather than a single crab.
        let mid = self.chain_count / 2;
        let target = self
            .crabs
            .iter()
            .filter(|c| c.caught && c.chain_index.map_or(false, |idx| idx > 0))
            .min_by_key(|c| {
                let idx = c.chain_index.unwrap();
                idx.abs_diff(mid)
            })
            .map(|c| c.pos);
        let Some(target) = target else {
            return;
        };
        // Pick the rival nearest the player so the staged crossing reads like a real pursuit steal.
        let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        let ni = (0..self.npc_trains.len()).min_by(|&a, &b| {
            let da = self.npc_trains[a].leader_pos.distance_squared(player_center);
            let db = self.npc_trains[b].leader_pos.distance_squared(player_center);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
        let Some(ni) = ni else {
            return;
        };
        self.npc_trains[ni].leader_pos = target;
        self.npc_trains[ni].steal_cooldown = 0.0;
        self.npc_trains[ni].idle_timer = 0.0;
    }

    /// Bot-test helper (see BotAction::ForcePlayerCross): deterministically stage the player's "steal
    /// to win" splice. Teleport the player's head onto the nearest rival NPC train's mid-follower and
    /// clear the steal-back cooldown, so the reciprocal splice in update_npc_trains fires this frame.
    /// A no-op when there's nothing to steal (no rival with followers, or the player has no train).
    /// Mirrors force_npc_cross, only pointed the other way — it exercises the real detection +
    /// split_off + stolen-crab transfer path; only the head's threading (RNG-timed against a wandering
    /// rival) is shortcut.
    pub fn force_player_cross(&mut self) {
        if self.chain_count < 1 {
            return;
        }
        const STEPS: usize = 14; // must match update_npc_trains / draw_npc_conga_train spacing
        let head = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        // Pick the nearest rival that actually has followers to splice.
        let ni = (0..self.npc_trains.len())
            .filter(|&i| !self.npc_trains[i].follower_types.is_empty())
            .min_by(|&a, &b| {
                let da = self.npc_trains[a].leader_pos.distance_squared(head);
                let db = self.npc_trains[b].leader_pos.distance_squared(head);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            });
        let Some(ni) = ni else {
            return;
        };
        // Aim the head at a mid-follower so the splice takes a meaningful tail section, not one crab.
        let mid_fi = self.npc_trains[ni].follower_types.len() / 2;
        if let Some(&fpos) = self.npc_trains[ni].path_history.get((mid_fi + 1) * STEPS) {
            self.player_pos = fpos - Vec2::splat(PLAYER_SIZE / 2.0);
            self.player_steal_cooldown = 0.0;
        }
    }

    pub fn update_npc_trains(&mut self, dt: f32) {
        // One shared cooldown gates how often YOU can rustle from a rival, so threading a line
        // takes one clean back-section per window instead of vacuuming a whole train in a frame.
        self.player_steal_cooldown = (self.player_steal_cooldown - dt).max(0.0);
        // Whether we're inside the on-beat window this frame — the rival's steal snaps ON the beat
        // (see the splice block below) so losing crabs is rhythmic, a drum hit rather than a random grab.
        let on_beat = self.beat_timer < BEAT_WINDOW
            || self.beat_timer > self.beat_interval - BEAT_WINDOW;
        for i in 0..self.npc_trains.len() {
            // --- Idle pause at destination -------------------------------------------------
            // When idle_timer > 0 the train has just arrived at a target and is "surveying"
            // before picking a new one — gives Rain World-style decisiveness, not dumb wandering.
            if self.npc_trains[i].idle_timer > 0.0 {
                self.npc_trains[i].idle_timer -= dt;
                // Decelerate while idling
                self.npc_trains[i].leader_vel *= (1.0 - 6.0 * dt).max(0.0);
                // Still sample path so followers catch up during the pause
                let cur = self.npc_trains[i].leader_pos;
                let last = self.npc_trains[i]
                    .path_history
                    .front()
                    .copied()
                    .unwrap_or(cur);
                if cur.distance_squared(last) > 36.0 {
                    self.npc_trains[i].path_history.push_front(cur);
                }
                let dist_to_player = cur.distance(self.player_pos);
                self.npc_trains[i].target_vol = ((800.0 - dist_to_player) / 600.0).clamp(0.0, 1.0);
                continue;
            }

            let to_target = self.npc_trains[i].target - self.npc_trains[i].leader_pos;
            let dist = to_target.length();

            // --- Wander target selection ---------------------------------------------------
            // Bias targets strongly toward territory center so rivals patrol distinct regions.
            // Small scouts are fast and range further; large elders are slow and stay local.
            self.npc_trains[i].target_timer -= dt;
            if dist < 80.0 || self.npc_trains[i].target_timer <= 0.0 {
                // Arrived — enter a brief idle before picking the next target.
                let rng = &mut rand::rng();
                let idle_secs = rng.random_range(1.2_f32..3.5);
                self.npc_trains[i].idle_timer = idle_secs;

                // Territory-biased target: blend between a random world point and the territory
                // center. Large elder (scale 2.4) stays very local; small scout (scale 1.2) ranges.
                let scale = self.npc_trains[i].leader_scale;
                // territory_bias 0..1: how strongly the next target is pulled toward territory center
                let territory_bias = ((scale - 1.2) / 1.2).clamp(0.0, 1.0) * 0.65 + 0.2;
                let margin = 160.0;
                // Guard against empty range panic if world is unexpectedly small
                let ww = (self.world_width - margin).max(margin + 1.0);
                let wh = (self.world_height - margin).max(margin + 1.0);
                let rand_pt = Vec2::new(rng.random_range(margin..ww), rng.random_range(margin..wh));
                let tc = self.npc_trains[i].territory_center;
                // Offset from territory center — scouts wander further (larger offset radius)
                let wander_radius = 380.0 - scale * 80.0; // scout=284, medium=236, elder=188
                let angle: f32 = rng.random_range(0.0..std::f32::consts::TAU);
                let territory_pt = tc + Vec2::new(angle.cos(), angle.sin()) * wander_radius;
                let next_target = rand_pt.lerp(territory_pt, territory_bias);
                self.npc_trains[i].target = next_target.clamp(
                    Vec2::splat(margin),
                    Vec2::new(self.world_width - margin, self.world_height - margin),
                );
                // Timer is a fallback; normal flow goes through idle_timer arrival check
                self.npc_trains[i].target_timer = rng.random_range(18.0_f32..35.0);
            }

            // --- Steering ------------------------------------------------------------------
            // Speed inversely proportional to leader_scale: scouts zip, elders lumber.
            let speed = match () {
                _ if self.npc_trains[i].leader_scale < 1.5 => 105.0, // small scout
                _ if self.npc_trains[i].leader_scale < 2.0 => 80.0,  // medium wanderer
                _ => 52.0,                                           // large elder
            };
            // Gentle perpendicular wobble so the path curves naturally instead of beelining.
            let perp = Vec2::new(-to_target.y, to_target.x).normalize_or_zero();
            let wobble_phase = self.time_elapsed * 0.4 + i as f32 * 2.1;
            let wobble = perp * wobble_phase.sin() * 18.0;

            if dist > 1.0 {
                let desired = (to_target / dist + wobble / dist.max(1.0)) * speed;
                let steer_rate = if dist < 200.0 { 4.5 } else { 2.8 }; // tighter turns near target
                let steer = (desired - self.npc_trains[i].leader_vel) * (steer_rate * dt);
                self.npc_trains[i].leader_vel += steer;
                if self.npc_trains[i].leader_vel.length() > speed {
                    self.npc_trains[i].leader_vel =
                        self.npc_trains[i].leader_vel.normalize() * speed;
                }
            }
            let margin = 80.0;
            let vel_step = self.npc_trains[i].leader_vel * dt;
            self.npc_trains[i].leader_pos += vel_step;
            self.npc_trains[i].leader_pos.x = self.npc_trains[i]
                .leader_pos
                .x
                .clamp(margin, self.world_width - margin);
            self.npc_trains[i].leader_pos.y = self.npc_trains[i]
                .leader_pos
                .y
                .clamp(margin, self.world_height - margin);

            // --- Path history for follower trailing ----------------------------------------
            let cur_pos = self.npc_trains[i].leader_pos;
            let last = self.npc_trains[i]
                .path_history
                .front()
                .copied()
                .unwrap_or(cur_pos);
            if cur_pos.distance_squared(last) > 36.0 {
                self.npc_trains[i].path_history.push_front(cur_pos);
                let max_len = self.npc_trains[i].follower_types.len() * 16 + 20;
                while self.npc_trains[i].path_history.len() > max_len {
                    self.npc_trains[i].path_history.pop_back();
                }
            }

            // Scale the King Crab with its conga line: more followers = bigger, scarier leader.
            // Each follower adds 0.09 scale above the tier floor, capped at 3.8 so even a
            // maxed-out elder doesn't become comically huge.
            {
                let n = self.npc_trains[i].follower_types.len() as f32;
                let base = self.npc_trains[i].base_scale;
                self.npc_trains[i].leader_scale = (base + n * 0.09).min(3.8);
            }

            // Compute target rumble volume from distance to player.
            let dist_to_player = self.npc_trains[i].leader_pos.distance(self.player_pos);
            self.npc_trains[i].target_vol = ((800.0 - dist_to_player) / 600.0).clamp(0.0, 1.0);

            // --- Pursuit: when the player has a train, bias steering toward their tail --------
            // The NPC behaves like a rival player: it wants to get BEHIND the player's chain to
            // thread through it for a steal, not charge the head where the player is watching.
            const PURSUIT_RANGE: f32 = 550.0;
            if self.chain_count >= 2
                && dist_to_player < PURSUIT_RANGE
                && self.npc_trains[i].idle_timer <= 0.0
            {
                // Use cached tail pos (updated once per frame in update_crabs) instead of
                // an O(n_crabs) scan here — saves ~160 iterations × 3 NPCs every frame.
                if let Some(tail_pos) = self.cached_tail_pos {
                    // Blend the wander target toward the tail — NPC steers into range naturally
                    let pursuit_blend =
                        ((PURSUIT_RANGE - dist_to_player) / PURSUIT_RANGE).clamp(0.0, 0.8);
                    self.npc_trains[i].target = self.npc_trains[i]
                        .target
                        .lerp(tail_pos, pursuit_blend * dt * 3.0);
                }
            }

            // --- Reverse-Snake chain splice steal (telegraphed + beat-synced) ----------------
            // When the NPC leader threads within range of an exposed tail link it ARMS a steal:
            // a brief telegraph fuse ramps while the threatened crabs tremble in place, then the
            // splice SNAPS on the beat (or when the fuse expires). Everything from the spliced link
            // to the tail detaches from the player and joins the NPC. Making the grab telegraphed and
            // rhythmic — never a silent instant strip — is what makes losing crabs read as *earned*
            // (INSPIRATION.md "Legible risk") and land like a drum hit rather than random loss.
            self.npc_trains[i].steal_cooldown = (self.npc_trains[i].steal_cooldown - dt).max(0.0);
            if self.npc_trains[i].steal_cooldown <= 0.0 && self.chain_count > 1 {
                const STEAL_RANGE: f32 = 58.0;
                const STEAL_FUSE: f32 = 0.55; // telegraph window (~one beat) between arming and the snap
                let npc_pos = self.npc_trains[i].leader_pos;
                let armed = self.npc_trains[i].steal_threat > 0.0;
                // Early-out: if the NPC is far from the player and the chain tail — and nothing is
                // already armed — no chain crab can be within STEAL_RANGE. Use cached_tail_pos (the
                // farthest link, already computed by update_crabs) as a lower-bound proxy to avoid the
                // O(n_crabs) scan. Once armed we fall through so the fuse still counts down to its snap.
                let chain_span = self
                    .cached_tail_pos
                    .map_or(0.0_f32, |t| t.distance(self.player_pos));
                let dist_to_chain = dist_to_player - chain_span;
                if dist_to_chain > STEAL_RANGE && !armed {
                    continue; // skip inner per-crab scan entirely this frame for this NPC
                }
                // Find the earliest (closest-to-head) link the NPC is within range of.
                // We splice there so a threading pass takes the maximum tail section.
                let splice_at = self
                    .crabs
                    .iter()
                    .filter(|c| c.caught && c.chain_index.map_or(false, |idx| idx > 0))
                    .filter(|c| npc_pos.distance(c.pos) < STEAL_RANGE)
                    .map(|c| c.chain_index.unwrap())
                    .min();

                if !armed {
                    // ARM the steal the moment a link comes into range: start the telegraph fuse and
                    // latch the target link so the snap fires from here even if the leader drifts off it.
                    if let Some(splice_idx) = splice_at {
                        self.npc_trains[i].steal_threat = STEAL_FUSE;
                        self.npc_trains[i].steal_target = splice_idx;
                        let npc_name = self.npc_trains[i].name.clone();
                        let warn_pos = self
                            .crabs
                            .iter()
                            .find(|c| c.caught && c.chain_index.map_or(false, |idx| idx >= splice_idx))
                            .map_or(npc_pos, |c| c.pos);
                        // Peripheral threat language: a red warning callout + ring at the threatened tail.
                        self.floating_texts.spawn(
                            format!("⚠ {} is on your tail!", npc_name),
                            warn_pos - Vec2::new(90.0, 42.0),
                            26.0,
                            [0.98, 0.40, 0.16, 1.0],
                        );
                        if self.catch_shockwaves.len() < 48 {
                            self.catch_shockwaves.push((warn_pos, 0.0, [0.98, 0.30, 0.14]));
                        }
                    }
                } else {
                    // Armed: creep the latched target forward if the NPC threaded closer to the head
                    // (a deeper cut steals more), tremble the threatened crabs as the telegraph, and
                    // snap on the beat once the warning has shown a moment — or when the fuse runs out.
                    if let Some(splice_idx) = splice_at {
                        self.npc_trains[i].steal_target =
                            self.npc_trains[i].steal_target.min(splice_idx);
                    }
                    self.npc_trains[i].steal_threat -= dt;
                    let splice_idx = self.npc_trains[i].steal_target;
                    for crab in self.crabs.iter_mut() {
                        if crab.caught && crab.chain_index.map_or(false, |idx| idx >= splice_idx) {
                            crab.spooked_timer = crab.spooked_timer.max(0.22); // trembling "AT RISK" tell
                        }
                    }
                    let telegraph_shown = self.npc_trains[i].steal_threat < STEAL_FUSE - 0.12;
                    let fire = self.npc_trains[i].steal_threat <= 0.0 || (on_beat && telegraph_shown);
                    if fire {
                        self.npc_trains[i].steal_threat = 0.0;
                        // Collect the stolen types before mutating crabs
                        let mut stolen_types: Vec<CrabType> = Vec::new();
                        let mut stolen_count = 0usize;
                        for crab in self.crabs.iter_mut() {
                            if crab.caught
                                && crab.chain_index.map_or(false, |idx| idx >= splice_idx)
                            {
                                crab.caught = false;
                                crab.chain_index = None;
                                crab.fleeing = false;
                                crab.spooked_timer = 1.0;
                                // Cartoony startled hop: scale-pop then fly toward the NPC.
                                crab.join_pulse = 1.0;
                                let toward = (npc_pos - crab.pos).normalize_or_zero();
                                crab.vel = toward * 200.0;
                                crab.vel.y -= 90.0; // brief upward arc before snapping over
                                stolen_types.push(crab.crab_type);
                                stolen_count += 1;
                            }
                        }
                        if stolen_count > 0 {
                            self.chain_count = self.chain_count.saturating_sub(stolen_count);
                            self.crabs_stolen_by_npc += stolen_count;
                            self.steal_loss_sfx = true; // play the descending loss sting (has no ctx here)
                            self.npc_trains[i].follower_types.extend(stolen_types);
                            self.npc_trains[i].steal_cooldown = 2.2;
                            // Visual + audio feedback — this is the key threat moment
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
                            // Shockwave at the splice point so the cut reads on screen
                            if self.catch_shockwaves.len() < 48 {
                                self.catch_shockwaves
                                    .push((npc_pos, 0.0, [0.96, 0.72, 0.16]));
                            }
                        }
                    }
                }
            } else if self.npc_trains[i].steal_threat > 0.0 {
                // Cooldown started, or the chain was banked/snapped out from under the threat —
                // let any armed telegraph lapse cleanly so a stale target can't fire later.
                self.npc_trains[i].steal_threat = 0.0;
            }

            // --- Steal to win: thread YOUR head through a rival's line to rustle it back --------
            // The mirror of the rival's splice above (INSPIRATION.md "The core steal mechanic"):
            // when the player's head crosses a rival's body, the rival splices at the crossing and
            // its back section (that follower → tail) snaps onto YOUR conga line as caught crabs.
            // This is the "Steal to win" verb — the whole prototype has been scaffolding toward it.
            // Rhythmic: crossing ON the beat pays a groove surge + bigger score (skill ceiling).
            if self.player_steal_cooldown <= 0.0 && !self.npc_trains[i].follower_types.is_empty() {
                const P_STEAL_RANGE: f32 = 54.0;
                // Follower fi sits at path_history[(fi+1)*STEPS] (same layout draw_npc_conga_train
                // uses). Find the earliest (closest-to-leader) follower the player head is within
                // range of — splicing there takes the largest tail section, like the rival does.
                const STEPS: usize = 14;
                let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
                let mut splice_at: Option<usize> = None;
                for fi in 0..self.npc_trains[i].follower_types.len() {
                    if let Some(&fpos) = self.npc_trains[i].path_history.get((fi + 1) * STEPS) {
                        if player_center.distance(fpos) < P_STEAL_RANGE {
                            splice_at = Some(fi);
                            break;
                        }
                    }
                }
                if let Some(fi) = splice_at {
                    let on_beat = self.on_beat_now();
                    // split_off(fi) leaves 0..fi on the rival and returns the back section fi..tail.
                    let stolen = self.npc_trains[i].follower_types.split_off(fi);
                    let stolen_count = stolen.len();
                    let mut rng = rand::rng();
                    for (k, ct) in stolen.into_iter().enumerate() {
                        // Spawn each rustled crab at its old follower slot, flying toward the player.
                        let old_pos = self.npc_trains[i]
                            .path_history
                            .get((fi + k + 1) * STEPS)
                            .copied()
                            .unwrap_or(self.npc_trains[i].leader_pos);
                        let toward = (player_center - old_pos).normalize_or_zero();
                        let mut vel = toward * 230.0;
                        vel.y -= 80.0; // brief upward arc before snapping into line
                        let ci = self.chain_count;
                        self.crabs
                            .push(spawn_stolen_crab(old_pos, vel, ct, ci, &mut rng));
                        self.chain_count += 1;
                    }
                    self.player_steal_cooldown = 2.2;
                    // Monotonic tally so the bot playtest can assert the steal-back fired without
                    // racing the live chain count (which banks/snaps drop back to zero).
                    self.crabs_stolen_by_player += stolen_count;
                    self.steal_gain_sfx = true; // play the rising triumphant sting (has no ctx here)
                    // Reward: stealing feeds the groove (harder on the beat) and banks score.
                    self.score += stolen_count * if on_beat { 3 } else { 2 };
                    self.groove = (self.groove + if on_beat { 0.22 } else { 0.10 }).min(1.0);
                    if on_beat {
                        self.beat_streak = (self.beat_streak + 1).min(99);
                        self.on_beat_flash = (self.on_beat_flash + 0.4).min(0.8);
                        self.beat_intensity = (self.beat_intensity + 1.0).min(2.0);
                    }
                    // Juice — the triumphant counterpart to losing crabs.
                    let npc_name = self.npc_trains[i].name.clone();
                    let label = if on_beat {
                        format!("RUSTLED {} — ON BEAT!", stolen_count)
                    } else {
                        format!("RUSTLED {} from {}!", stolen_count, npc_name)
                    };
                    self.floating_texts.spawn(
                        label,
                        player_center - Vec2::new(90.0, 60.0),
                        30.0,
                        [0.35, 1.0, 0.55, 1.0],
                    );
                    self.screen_shake = self.screen_shake.max(if on_beat { 10.0 } else { 6.0 });
                    self.zoom_punch = self.zoom_punch.max(if on_beat { 0.08 } else { 0.05 });
                    if self.catch_shockwaves.len() < 48 {
                        self.catch_shockwaves
                            .push((player_center, 0.0, [0.35, 1.0, 0.55]));
                    }
                }
            }

            // --- Free crab collection --------------------------------------------------------
            // NPCs act like players: they pick up free crabs they wander past.
            self.npc_trains[i].catch_cooldown = (self.npc_trains[i].catch_cooldown - dt).max(0.0);
            if self.npc_trains[i].catch_cooldown <= 0.0 {
                const CATCH_RANGE: f32 = 52.0;
                let npc_pos = self.npc_trains[i].leader_pos;
                let caught = self.crabs.iter_mut().find(|c| {
                    !c.caught
                        && !c.is_boss()
                        && c.is_catchable()
                        && npc_pos.distance(c.pos) < CATCH_RANGE
                });
                if let Some(crab) = caught {
                    let ct = crab.crab_type;
                    // Teleport the crab far off-screen rather than marking it caught=true with
                    // no chain_index — that would corrupt rendering InstanceArray capacity checks.
                    crab.pos = Vec2::new(-9999.0, -9999.0);
                    crab.vel = Vec2::ZERO;
                    crab.fleeing = false;
                    self.npc_trains[i].follower_types.push(ct);
                    self.npc_trains[i].catch_cooldown = 0.7;
                }
            }
        }

        // Audio: use the loudest (nearest) NPC train for the rumble volume — store on [0].
        let max_vol = self
            .npc_trains
            .iter()
            .map(|t| t.target_vol)
            .fold(0.0_f32, f32::max);
        if !self.npc_trains.is_empty() {
            self.npc_trains[0].target_vol = max_vol;
        }

        // --- Continuous overlap separation: prevent player from phasing inside NPC leaders ----
        // Regardless of cooldown, push the player and NPC apart whenever they overlap so
        // you can't stand inside a King Crab — the clash is painful but crisp, not a merge.
        {
            let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            for npc in self.npc_trains.iter_mut() {
                let col_r = CRAB_SIZE * npc.leader_scale * 1.2 + PLAYER_SIZE * 0.5;
                let dist = npc.leader_pos.distance(player_center);
                if dist < col_r && dist > 0.1 {
                    // Positional correction: push both apart so they don't phase through each other
                    let overlap = col_r - dist;
                    let dir = (player_center - npc.leader_pos).normalize_or_zero();
                    self.player_pos += dir * overlap * 0.6;
                    npc.leader_pos -= dir * overlap * 0.4;
                    // Velocity damping so they slide off each other instead of jittering
                    let rel_vel = self.player_vel - npc.leader_vel;
                    let sep_speed = rel_vel.dot(dir);
                    if sep_speed < 0.0 {
                        self.player_vel -= dir * sep_speed * 0.8;
                        npc.leader_vel += dir * sep_speed * 0.8;
                    }
                }
            }
        }

        // --- Player-vs-NPC-leader collision: painful bounce, both sides take damage ----------
        // Deliberately touching a rival's leader is a desperate counter-attack move — you can
        // stun them and scatter some of their followers, but you take a hit too. Painful enough
        // to avoid unless intentional, spectacular enough to feel like a real clash.
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
                // Both sides bounce apart — physics punch
                let away_from_npc =
                    (player_center - self.npc_trains[ni].leader_pos).normalize_or_zero();
                self.player_vel += away_from_npc * 380.0;
                self.npc_trains[ni].leader_vel += -away_from_npc * 280.0;
                // Camera + screen feedback — this should feel painful
                self.screen_shake = self.screen_shake.max(16.0);
                self.zoom_punch = self.zoom_punch.max(0.10);
                self.hitstop_timer = self.hitstop_timer.max(0.12);
                // Player loses tail crabs (1–2), NPC loses some followers — mutual damage
                let player_lose = 2.min(self.chain_count.saturating_sub(1));
                let mut released = 0;
                for crab in self.crabs.iter_mut().rev() {
                    if released >= player_lose {
                        break;
                    }
                    if crab.caught {
                        if let Some(idx) = crab.chain_index {
                            if idx > 0 {
                                crab.caught = false;
                                crab.chain_index = None;
                                crab.fleeing = true;
                                crab.spooked_timer = 2.5;
                                crab.join_pulse = 1.0; // startled pop
                                let away = (crab.pos - player_center).normalize_or_zero();
                                crab.vel = away * 250.0;
                                crab.vel.y -= 70.0; // hop upward before scattering out
                                if self.catch_shockwaves.len() < 48 {
                                    self.catch_shockwaves.push((crab.pos, 0.0, [1.0, 0.6, 0.2]));
                                }
                                released += 1;
                            }
                        }
                    }
                }
                self.chain_count = self.chain_count.saturating_sub(released);
                // NPC loses its last 1–2 followers — they scatter as free crabs (Sonic rings)
                let npc_pos = self.npc_trains[ni].leader_pos;
                let npc_lose = 2.min(self.npc_trains[ni].follower_types.len());
                for k in 0..npc_lose {
                    self.npc_trains[ni].follower_types.pop();
                    // Spawn a fleeing crab from around the NPC leader position
                    let scatter_angle =
                        k as f32 * std::f32::consts::PI + away_from_npc.y.atan2(away_from_npc.x);
                    let scatter_dir = Vec2::new(scatter_angle.cos(), scatter_angle.sin());
                    // Find a free slot in crabs if any normal crab can represent the ejected follower
                    // (simplified: just spawn a particle burst at the NPC position)
                    if self.catch_shockwaves.len() < 48 {
                        self.catch_shockwaves.push((
                            npc_pos + scatter_dir * 30.0,
                            0.0,
                            [0.96, 0.72, 0.16],
                        ));
                    }
                }
                // Groove penalty for taking a head-on hit — you should have dodged
                self.groove = (self.groove - 0.20).max(0.0);
                self.beat_streak = self.beat_streak.saturating_sub(1);
                let npc_name = self.npc_trains[ni].name.clone();
                self.floating_texts.spawn(
                    format!("CLASH with {}!", npc_name),
                    player_center - Vec2::new(80.0, 65.0),
                    32.0,
                    [1.0, 0.5, 0.15, 1.0],
                );
                self.particle_system
                    .spawn_milestone_fireworks(player_center, 8, &mut rand::rng());
            }
        }

        // --- NPC-vs-NPC leader collisions: they bounce off each other too --------------------
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
                    // Each loses one follower (if they have any)
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

        // Followers trail the leader using path_history. Each follower sits 14 history-steps
        // behind the previous one (history is sampled ~every 6px, so ~84px spacing between crabs).
        const STEPS: usize = 14;

        for npc in &self.npc_trains {
            // Draw followers back-to-front so the leader renders on top.
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
                    scale: npc.leader_scale * 0.33, // followers scale with leader tier
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
                draw_crab(
                    ctx,
                    canvas,
                    &fake,
                    pos + Vec2::new(0.0, -bob),
                    beat,
                    0.0,
                    bob.max(0.0),
                    0.0,
                    t,
                )?;
            }

            // King Crab leader — large, golden, unmistakeable.
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
                ctx,
                canvas,
                &king,
                npc.leader_pos + Vec2::new(0.0, -leader_bob),
                king_beat,
                0.0,
                leader_bob.max(0.0),
                facing,
                t,
            )?;

            // A gentle golden halo so the King reads as the leader from across the world.
            let dot = unit_circle(ctx)?;
            for ring in (0..3).rev() {
                let r = 40.0 + ring as f32 * 14.0;
                let a = 0.06 - ring as f32 * 0.015;
                canvas.draw(
                    dot,
                    DrawParam::default()
                        .dest(npc.leader_pos)
                        .scale(Vec2::splat(r))
                        .color(Color::new(1.0, 0.82, 0.2, a)),
                );
            }

            // Name banner floating above the King Crab — a distinct, readable-across-the-field
            // label so rivals tell apart at a glance (agar.io: spot the big one creeping in from
            // the edge). Three signals stack:
            //   • Size by tier — elders' banners are noticeably bigger than scouts', scaled off
            //     base_scale (scout 1.2 / wanderer 1.8 / elder 2.4).
            //   • Colour by tier — pale lime scout, regal gold wanderer, deep-amber apex elder.
            //   • Distance-scaled alpha — a distant rival's name burns in at full opacity so you
            //     can read who's approaching; it eases off as they close on you and the crab
            //     itself is plainly visible.
            // Glyphs are shaped once (cached at a large baseline) and the per-tier size comes from
            // the draw scale, so this stays allocation-free per frame.
            let name_w = NPC_NAME_CACHE.with(|c| -> GameResult<f32> {
                let mut cache = c.borrow_mut();
                let needs_rebuild = cache.as_ref().map_or(true, |(n, _, _)| n != &npc.name);
                if needs_rebuild {
                    let mut text = Text::new(npc.name.as_str());
                    text.set_scale(24.0);
                    let w = text.measure(ctx)?.x;
                    *cache = Some((npc.name.clone(), text, w));
                }
                Ok(cache.as_ref().unwrap().2)
            })?;
            // Tier styling from the leader's base size.
            let tier_scale = 0.8 + (npc.base_scale - 1.2) * 0.33;
            let (nr, ng, nb) = if npc.base_scale >= 2.2 {
                (1.0, 0.5, 0.12) // elder — deep amber, the apex train
            } else if npc.base_scale >= 1.6 {
                (0.98, 0.78, 0.28) // wanderer — regal gold
            } else {
                (0.72, 0.95, 0.5) // scout — pale lime, small and fast
            };
            // Distance ramp: far rivals read at full opacity, near ones ease back.
            let dist = (npc.leader_pos - self.player_pos).length();
            let dist_alpha = (0.5 + dist / 1000.0 * 0.5).clamp(0.5, 1.0);
            let draw_w = name_w * tier_scale;
            let name_off = 45.0 + npc.leader_scale * 10.0 + leader_bob;
            NPC_NAME_CACHE.with(|c| {
                let cache = c.borrow();
                if let Some((_, text, _)) = cache.as_ref() {
                    let name_pos = npc.leader_pos - Vec2::new(draw_w / 2.0, name_off);
                    // Drop shadow (scaled with the banner so it tracks tier size)
                    canvas.draw(
                        text,
                        DrawParam::default()
                            .dest(name_pos + Vec2::splat(2.0 * tier_scale))
                            .scale(Vec2::splat(tier_scale))
                            .color(Color::new(0.0, 0.0, 0.0, 0.7 * dist_alpha)),
                    );
                    // Name in its tier colour
                    canvas.draw(
                        text,
                        DrawParam::default()
                            .dest(name_pos)
                            .scale(Vec2::splat(tier_scale))
                            .color(Color::new(nr, ng, nb, dist_alpha)),
                    );
                }
            });
        }

        Ok(())
    }
}
