//! Bot-test scenario staging for the rival NPC King Crab trains. These `force_*` / `ensure_*`
//! helpers deterministically set up the reverse-Snake steal scenarios (player↔rival crossings,
//! revenge counter-steals, parry/dodge defense, rival-vs-rival splices and hunts) so the headless
//! playtest bot can exercise the real `update_npc_trains` detection/splice/transfer paths without
//! depending on RNG-timed wander lining leaders up by chance. Bot-only: every method here is
//! reachable solely through a `BotAction`. Extracted out of `npc_trains.rs` to keep the runtime
//! train update separate from its test scaffolding — same methods, same behaviour.

use ggez::glam::Vec2;

use crate::constants::*;
use crate::enemies::CrabType;
use crate::npc_conga_train::NpcCongaTrain;
use crate::spawnings::spawn_scattered_crab;
use crate::state::MainState;

impl MainState {
    /// Creates the NPC train fixture needed by bot-only scenarios without reintroducing rivals to
    /// tutorial gameplay.
    fn ensure_bot_npc_trains(&mut self) {
        if self.npc_trains.is_empty() {
            self.npc_trains = (0..3)
                .map(|index| NpcCongaTrain::new_at(self.world_width, self.world_height, index))
                .collect();
        }
    }

    /// Bot-test helper: deterministically top the player's conga chain up to `target` links so the
    /// forced-steal helpers below always have a stealable chain to act on this frame. The staged
    /// steal scenarios grow their chain with the seek-catch autopilot, but each forced splice drives
    /// the chain back down to 1–2 links, and on a slow/loaded headless run RNG catches can't always
    /// regrow it before the next 0.9 s force fires — so a whole run's forces can no-op with the chain
    /// stuck below 2, the source of the intermittent StolenAtLeast / RevengeStealAtLeast flakes (CI
    /// went red on `revenge` + `steal_dodge`; `npc_steal` flaked locally). Enlisting the nearest wild
    /// catchable crabs — the same caught / chain_index / chain_count bump a real catch does — removes
    /// that variance without touching the splice/detach/transfer path the tests actually exercise
    /// (chain *building* is already covered by menu_to_game). Bot-only: only ever reached from the
    /// Force* bot actions. The primed links are tucked in a short row behind the player so the staged
    /// train reads with clean mid/tail geometry rather than teleporting a link across the world.
    fn bot_prime_chain(&mut self, target: usize) {
        let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        while self.chain_count < target {
            let next = self
                .crabs
                .iter()
                .enumerate()
                .filter(|(_, c)| c.is_catchable())
                .min_by(|(_, a), (_, b)| {
                    a.pos
                        .distance_squared(player_center)
                        .partial_cmp(&b.pos.distance_squared(player_center))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i);
            // If the world has been picked clean, mint a fresh catchable crab to enlist instead of
            // bailing. The seek-catch autopilot banks crabs faster than they respawn, so on a slow /
            // loaded headless run the field can dip to 1–2 crabs (see the failing steal_defense perf
            // log: "2 crabs, 1 chained") — leaving the chain stuck below 2 and no-oping every 0.9 s
            // force, the source of the intermittent Parried/Dodged/Revenge flakes (#170). Spawning
            // guarantees the staged chain always reaches `target` regardless of spawn/catch timing.
            // Bot-only: only ever reached from the Force* bot actions, and the crab is caught on the
            // same line below (chained, not free), so it can't trip the overwhelmed game-over.
            let i = match next {
                Some(i) => i,
                None => {
                    let fresh = spawn_scattered_crab(
                        player_center,
                        Vec2::ZERO,
                        CrabType::Normal,
                        &mut crate::rng::rng(),
                    );
                    self.crabs.push(fresh);
                    self.crabs.len() - 1
                }
            };
            let idx = self.chain_count;
            // Drop the fresh link straight onto the conga slot update_crabs lerps chain crab `idx`
            // toward (position_history[(idx+1)*CHAIN_LINK_FRAMES]); placing it anywhere else lets the
            // very next update_crabs yank it a long way toward that slot in one frame, moving it out
            // from under the rival leader the force helper just parked on it and making the detection
            // miss. Sitting it on the slot keeps the staged train stable so the splice lands reliably.
            let slot = self
                .position_history
                .get((idx + 1) * CHAIN_LINK_FRAMES)
                .copied()
                .unwrap_or_else(|| player_center - Vec2::new(0.0, (idx as f32 + 1.0) * CRAB_SIZE));
            let crab = &mut self.crabs[i];
            crab.caught = true;
            crab.chain_index = Some(idx);
            crab.fleeing = false;
            crab.startle_timer = 0.0;
            crab.latch_timer = 0.0;
            crab.pos = slot;
            self.chain_count += 1;
        }
    }

    /// Bot-test helper (see BotAction::ForceNpcCross): deterministically stage the reverse-Snake
    /// steal. Teleport the nearest rival NPC King Crab train's leader onto a mid-chain link of the
    /// player's conga line and clear its steal cooldown, so `update_npc_trains`' splice fires this
    /// frame. A no-op when there's nothing stealable (no NPC trains, or a chain shorter than 2). This
    /// exercises the real detection + detachment + follower-transfer path; only the rival's pathing
    /// (which is RNG-timed and can't be counted on inside a headless budget) is shortcut.
    pub fn force_npc_cross(&mut self) {
        self.ensure_bot_npc_trains();
        // Guarantee a stealable chain regardless of the autopilot's RNG catch timing (see
        // bot_prime_chain); no-op if there were no wild crabs to enlist.
        self.bot_prime_chain(6);
        if self.chain_count < 2 {
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
            let da = self.npc_trains[a]
                .leader_pos
                .distance_squared(player_center);
            let db = self.npc_trains[b]
                .leader_pos
                .distance_squared(player_center);
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
        self.ensure_bot_npc_trains();
        // Guarantee the player has a train regardless of the autopilot's RNG catch timing.
        self.bot_prime_chain(3);
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
        // Walk down from the mid slot to the first follower whose slot is actually recorded in
        // path_history — a rival that hasn't wandered far enough to have sampled its deep mid slot yet
        // still gets threaded at a shallower one, instead of the whole cross silently no-oping.
        let mid_fi = self.npc_trains[ni].follower_types.len() / 2;
        for fi in (0..=mid_fi).rev() {
            if let Some(&fpos) = self.npc_trains[ni].path_history.get((fi + 1) * STEPS) {
                self.player_pos = fpos - Vec2::splat(PLAYER_SIZE / 2.0);
                self.player_steal_cooldown = 0.0;
                // Hold the head on this slot for the frame — the seek-catch autopilot in
                // handle_player_movement runs before the steal detection and would otherwise drift it
                // off (see BotState.hold_position). Guards against a slow-frame drift past steal range.
                if let Some(bot) = self.bot.as_mut() {
                    bot.hold_position = true;
                }
                break;
            }
        }
    }

    /// Bot-test helper (see BotAction::ForceRevengeCross): deterministically stage the *revenge*
    /// steal-back — thread the player's head through the line of the rival whose revenge marker is
    /// live (it just spliced your tail) so the steal-back fires with the revenge bonus this frame.
    /// Mirrors force_player_cross but targets the marked rival specifically, so the revenge path is
    /// exercised without racing which rival a nearest heuristic happens to pick. A no-op only when no
    /// rival is currently revenge-marked at all (the marker's followers are topped up here so an empty
    /// culprit can't silently no-op the steal-back).
    pub fn force_player_revenge(&mut self) {
        self.ensure_bot_npc_trains();
        // Guarantee the player has a train to thread back with (the revenge marker + rival followers
        // come from the preceding ForceNpcCross/Dodge, not from here).
        self.bot_prime_chain(3);
        if self.chain_count < 1 {
            return;
        }
        const STEPS: usize = 14; // must match update_npc_trains / draw_npc_conga_train spacing
        // Pick the freshest revenge-marked rival (most time left on its marker) — the culprit whose
        // counter-steal window is widest open.
        let ni = (0..self.npc_trains.len())
            .filter(|&i| self.npc_trains[i].revenge_timer > 0.0)
            .max_by(|&a, &b| {
                self.npc_trains[a]
                    .revenge_timer
                    .partial_cmp(&self.npc_trains[b].revenge_timer)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        let Some(ni) = ni else {
            return;
        };
        // Aim the head at a mid-follower so the splice takes a meaningful tail section, not one crab —
        // but never past the deepest follower slot that actually exists in path_history. A rival that
        // just gained followers (e.g. right after ForceNpcCross spliced the player's tail onto it, as
        // the `revenge` scenario stages) hasn't trailed a deep enough path yet, so the fixed mid slot
        // can be missing and the crossing silently wouldn't place — the frame-rate-dependent half of
        // the #170 flake in the revenge scenario. Clamping to the deepest resolvable slot makes the
        // crossing land whenever the rival has any followers and any trail, without racing path depth.
        let flen = self.npc_trains[ni].follower_types.len();
        let plen = self.npc_trains[ni].path_history.len();
        // Largest fi whose slot (fi+1)*STEPS is a valid index (<= plen-1).
        let max_slot_fi = (plen.saturating_sub(1) / STEPS).saturating_sub(1);
        let mid_fi = (flen / 2).min(max_slot_fi).min(flen.saturating_sub(1));
        if let Some(&fpos) = self.npc_trains[ni].path_history.get((mid_fi + 1) * STEPS) {
            self.player_pos = fpos - Vec2::splat(PLAYER_SIZE / 2.0);
            self.player_steal_cooldown = 0.0;
        }
    }

    /// Bot-test helper (see BotAction::ForceStealDefense): deterministically stage the defensive
    /// parry. Arm a rival's splice on a mid-chain link, snap the beat into the on-beat window, then
    /// run the real `try_defend_steal` helper (the exact path the Stomp/Wave casts drive) centred on
    /// the rival's leader so the cancel fires this frame. A no-op when there's nothing stealable
    /// (no NPC trains, or a chain shorter than 2). Exercises the real arm → on-beat cancel path; only
    /// the player's tool timing (RNG-fragile headless) is shortcut.
    pub fn force_steal_defense(&mut self) {
        self.ensure_bot_npc_trains();
        self.bot_prime_chain(6);
        if self.chain_count < 2 {
            return;
        }
        // Aim for a mid-chain link (never the head, index 0) — same target the rival's real splice
        // seeks, so the staged threat reads like a genuine tail-thread.
        let mid = self.chain_count / 2;
        let target = self
            .crabs
            .iter()
            .filter(|c| c.caught && c.chain_index.map_or(false, |idx| idx > 0))
            .min_by_key(|c| c.chain_index.unwrap().abs_diff(mid))
            .map(|c| (c.pos, c.chain_index.unwrap()));
        let Some((target_pos, target_idx)) = target else {
            return;
        };
        let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        let ni = (0..self.npc_trains.len()).min_by(|&a, &b| {
            let da = self.npc_trains[a]
                .leader_pos
                .distance_squared(player_center);
            let db = self.npc_trains[b]
                .leader_pos
                .distance_squared(player_center);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
        let Some(ni) = ni else {
            return;
        };
        // Arm a splice on this rival at the mid link (as update_npc_trains would once threaded).
        self.npc_trains[ni].leader_pos = target_pos;
        self.npc_trains[ni].steal_threat = STEAL_FUSE;
        self.npc_trains[ni].steal_target = target_idx;
        self.npc_trains[ni].steal_cooldown = 0.0;
        // Force the beat into the on-beat window and run the real parry helper centred on the rival.
        self.beat_timer = 0.0;
        let center = self.npc_trains[ni].leader_pos;
        self.try_defend_steal(center, 400.0, "STOMP");
    }

    /// Bot-test helper (see BotAction::ForceWaveShove): deterministically stage the Wave's proactive
    /// crowd-control. Place the nearest rival right beside the player (inside the Wave's reach), snap
    /// the beat on, then cast the real `fire_wave` — the exact path the Q key / SPACE+Q chord drive —
    /// so `wave_shove_rivals` shoves it and `rivals_wave_shoved` rises. A no-op with no NPC trains.
    /// Only the rival's placement and beat phase are staged; the shove itself runs the real code.
    pub fn force_wave_shove(&mut self) {
        self.ensure_bot_npc_trains();
        let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        let ni = (0..self.npc_trains.len()).min_by(|&a, &b| {
            let da = self.npc_trains[a]
                .leader_pos
                .distance_squared(player_center);
            let db = self.npc_trains[b]
                .leader_pos
                .distance_squared(player_center);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
        let Some(ni) = ni else {
            return;
        };
        // Beside the player, well inside WAVE_DEFEND_RADIUS so the shove is guaranteed to reach it.
        self.npc_trains[ni].leader_pos = player_center + Vec2::new(120.0, 0.0);
        self.beat_timer = 0.0; // on-beat: the big knockback branch
        self.beat_wave_active = false; // don't let an in-flight ring block the cast
        self.fire_wave();
    }

    /// Bot-test staging helper: guarantee rival `ni` has a stealable train of at least `min` followers
    /// with a `path_history` deep enough that every follower slot `(fi+1)*STEPS` resolves — the exact
    /// layout `force_player_revenge` and the real player steal-back read. The ambient headless rivals
    /// get depleted of followers over a long run, so without this the dodge→revenge counter (steal_dodge
    /// test) can't reliably cash and RevengeStealAtLeast(1) flakes with frame rate (#170). Topping up
    /// followers and synthesising a uniform trailing path is bot-only staging (in the same spirit as the
    /// leader teleports the other Force* helpers do); it leaves the real steal-back code path untouched.
    fn ensure_stealable_train(&mut self, ni: usize, min: usize) {
        const STEPS: usize = 14; // must match update_npc_trains / draw_npc_conga_train spacing
        // Top up the retinue so there's a meaningful tail to rustle back.
        let defaults = [
            CrabType::Normal,
            CrabType::Fast,
            CrabType::Sneaky,
            CrabType::Dancer,
            CrabType::Armored,
        ];
        let mut d = 0usize;
        while self.npc_trains[ni].follower_types.len() < min {
            self.npc_trains[ni]
                .follower_types
                .push(defaults[d % defaults.len()]);
            d += 1;
        }
        // Synthesise a uniform trail trailing the leader toward the world centre (so points stay in
        // bounds) so every follower slot exists. index 0 is the leader; deeper indices are further back,
        // matching path_history's front=newest ordering. Depth carries margin over the deepest slot the
        // steal-back reads, and stays under update_npc_trains' max_len cap (followers*16+20) so the next
        // frame doesn't trim it away.
        let need = (self.npc_trains[ni].follower_types.len() + 2) * STEPS;
        if self.npc_trains[ni].path_history.len() < need {
            let leader = self.npc_trains[ni].leader_pos;
            let center = Vec2::new(self.world_width * 0.5, self.world_height * 0.5);
            let mut dir = (center - leader).normalize_or_zero();
            if dir == Vec2::ZERO {
                dir = Vec2::new(-1.0, 0.0);
            }
            let lo = Vec2::splat(20.0);
            let hi = Vec2::new(self.world_width - 20.0, self.world_height - 20.0);
            self.npc_trains[ni].path_history.clear();
            for k in 0..need {
                let p = (leader + dir * (k as f32 * 10.0)).clamp(lo, hi);
                self.npc_trains[ni].path_history.push_back(p);
            }
        }
    }

    /// Bot-test helper (see BotAction::ForceStealDodge): deterministically stage the movement dodge —
    /// the reroute half of the defense. Arm a rival's splice on a mid-chain link, then teleport the
    /// rival's leader well clear of that link, so the next `update_npc_trains` sees the thread broken
    /// and fizzles the splice (steals_dodged rises). A no-op when there's nothing stealable (no NPC
    /// trains, or a chain shorter than 2). Mirrors force_steal_defense's arm, but instead of running
    /// the tool parry it exercises the geometry-based escape — only the player's fast juke (RNG-fragile
    /// against a wandering rival inside a headless budget) is shortcut. A clean reroute always opens a
    /// counter-steal window (marks the juked rival for revenge), so a following ForceRevengeCross can
    /// assert the dodge flipped into offense.
    pub fn force_steal_dodge(&mut self) {
        self.ensure_bot_npc_trains();
        self.bot_prime_chain(6);
        if self.chain_count < 2 {
            return;
        }
        let mid = self.chain_count / 2;
        let target = self
            .crabs
            .iter()
            .filter(|c| c.caught && c.chain_index.map_or(false, |idx| idx > 0))
            .min_by_key(|c| c.chain_index.unwrap().abs_diff(mid))
            .map(|c| (c.pos, c.chain_index.unwrap()));
        let Some((target_pos, target_idx)) = target else {
            return;
        };
        let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        // Prefer to stage the dodge against a rival that STILL HAS FOLLOWERS, then nearest. A clean
        // dodge marks the juked rival for revenge and opens a counter-steal window — but that window is
        // only *cashable* (a following ForceRevengeCross can rustle its tail back) if the marked rival
        // actually has a train to steal. Picking purely by distance meant the dodge often marked a lone
        // rival with no followers, so the revenge steal-back had nothing to grab and RevengeStealAtLeast
        // depended on the emergent luck of the nearest rival happening to have a train. That coincidence
        // shifts with frame rate (the ggez-0.10 headless stack renders ~3x faster), which is the #170
        // flake. Keying selection on "has followers, then nearest" makes the dodge→revenge counter
        // deterministic and frame-rate-independent; the fallback to nearest-overall keeps the dodge
        // itself (DodgedAtLeast) firing even when no rival currently has a train. This is a bot-staging
        // choice only — the real dodge/revenge game code it exercises is unchanged.
        let ni = (0..self.npc_trains.len()).min_by(|&a, &b| {
            let a_empty = self.npc_trains[a].follower_types.is_empty();
            let b_empty = self.npc_trains[b].follower_types.is_empty();
            a_empty.cmp(&b_empty).then_with(|| {
                let da = self.npc_trains[a]
                    .leader_pos
                    .distance_squared(player_center);
                let db = self.npc_trains[b]
                    .leader_pos
                    .distance_squared(player_center);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
        });
        let Some(ni) = ni else {
            return;
        };
        // Deterministically guarantee the dodge opens a *cashable* counter window. A clean dodge marks
        // the juked rival for revenge, but a revenge steal-back can only fire if that rival actually
        // has a tail to rustle back — and the ambient headless rivals get depleted of followers over a
        // 48s run (natural player/rival steals), emergently and frame-rate-dependently. That depletion
        // is the #170 flake: whether any marked rival happens to still have a train shifts with frame
        // rate (the ggez-0.10 headless stack renders ~3x faster). Stock the staged rival with a stealable
        // train here so the counter is always cashable. Bot-only staging (only reachable via BotAction),
        // in the same spirit as the leader teleports/cooldown clears above — the real dodge/revenge game
        // code it exercises is untouched.
        self.ensure_stealable_train(ni, 5);
        // Arm a splice on this rival at the mid link (as update_npc_trains would once threaded)...
        self.npc_trains[ni].steal_threat = STEAL_FUSE;
        self.npc_trains[ni].steal_target = target_idx;
        self.npc_trains[ni].steal_cooldown = 0.0;
        self.npc_trains[ni].idle_timer = 0.0;
        // ...then place the leader well clear of the threaded link (>ESCAPE_RANGE=145), as if the
        // player had juked the tail away — the next update sees the thread broken and fizzles it.
        // Push the leader *toward the world centre* by a guaranteed margin over ESCAPE_RANGE: pushing
        // inward always has room (the world is far bigger than the push), so unlike the old fixed
        // diagonals it can't clamp back on top of a link sitting near a wall — which left the dodge
        // occasionally not firing (thread never broke) and flaked DodgedAtLeast red. 265 px keeps a
        // comfortable cushion over the 145 px escape threshold even after the next update nudges the
        // link a little.
        let center = Vec2::new(self.world_width / 2.0, self.world_height / 2.0);
        let inward = {
            let d = (center - target_pos).normalize_or_zero();
            if d == Vec2::ZERO {
                Vec2::new(1.0, 0.0)
            } else {
                d
            }
        };
        let margin = 80.0;
        self.npc_trains[ni].leader_pos = (target_pos + inward * 265.0).clamp(
            Vec2::splat(margin),
            Vec2::new(self.world_width - margin, self.world_height - margin),
        );
    }

    /// Bot-test helper (see BotAction::ForceRivalCross): deterministically stage the rival-vs-rival
    /// splice. Pick the train with the most followers as the thief and teleport its leader onto a
    /// mid-follower of a strictly-smaller rival, clearing the thief's rival-steal cooldown so the
    /// whole-beach splice in update_npc_trains fires this frame. A no-op until a smaller rival has
    /// wandered far enough for its mid-follower path slot to exist. Mirrors force_player_cross,
    /// pointed rival→rival: it exercises the real detection + split_off + transfer path; only the
    /// RNG-timed wander that would otherwise have to line the two leaders up is shortcut.
    pub fn force_rival_cross(&mut self) {
        self.ensure_bot_npc_trains();
        const STEPS: usize = 14; // must match update_npc_trains / draw_npc_conga_train spacing
        let thief =
            (0..self.npc_trains.len()).max_by_key(|&i| self.npc_trains[i].follower_types.len());
        let Some(thief) = thief else {
            return;
        };
        let thief_len = self.npc_trains[thief].follower_types.len();
        // Aim the thief's leader at a smaller rival's mid-follower so the splice takes a meaningful
        // tail section, not one crab. Pick the strictly-smaller rival with the MOST followers so the
        // forced cut is deep enough to both transfer (RivalStealAtLeast) *and* spill a loose crumb
        // (RivalSpillAtLeast) — a 1-follower victim is a clean pickpocket with no spill, which used to
        // leave the spill assert flaky when the wander happened to shrink every rival down.
        let victim = (0..self.npc_trains.len())
            .filter(|&v| v != thief)
            .filter(|&v| {
                let l = self.npc_trains[v].follower_types.len();
                l >= 1 && l < thief_len
            })
            .max_by_key(|&v| self.npc_trains[v].follower_types.len());
        let Some(victim) = victim else {
            return;
        };
        let vlen = self.npc_trains[victim].follower_types.len();
        // Guarantee the victim's follower path slot exists regardless of how far it has wandered.
        // Under headless load fewer path samples accrue per sim-second (a bigger dt means fewer of
        // the >6px pushes the live wander uses), so the deep mid slot can be missing and every
        // ForceRivalCross no-ops — the root of the npc_vs_npc flake. Backfilling a straight trail
        // behind the leader makes the slot deterministic; the real splice path (arm → snap →
        // split_off → spill in update_npc_trains) then runs unchanged, so this only shortcuts the
        // RNG wander, exactly as the helper already documents.
        self.ensure_train_path_history(victim, vlen);
        // Splice just ahead of centre — `(vlen-1)/2` — so the cut takes at least two followers for
        // any victim with ≥2 of them (the tail-ward cut_from clamp keeps it ≤STEAL_MAX_LINKS). That
        // guarantees `stolen_count ≥ 2`, which is what makes the collision spill a loose crumb
        // (RivalSpillAtLeast); aiming dead-centre could leave a 1-crab pickpocket that never spills.
        let mid_fi = (vlen - 1) / 2;
        if let Some(&fpos) = self.npc_trains[victim]
            .path_history
            .get((mid_fi + 1) * STEPS)
        {
            self.npc_trains[thief].leader_pos = fpos;
            self.npc_trains[thief].rival_steal_cooldown = 0.0;
        }
    }

    /// Bot-test helper: backfill a train's `path_history` with a straight trail behind its leader so
    /// every follower slot `(fi + 1) * STEPS` up to `follower_count` is populated. Used by
    /// `force_rival_cross` so a forced crossing lands deterministically even when the ambient wander
    /// hasn't sampled a deep enough history yet under headless load. Purely a headless staging aid —
    /// the followers simply snap onto the synthesized trail exactly as they would a wandered one, and
    /// the live wander overwrites the front of the history the next frame anyway.
    fn ensure_train_path_history(&mut self, ni: usize, follower_count: usize) {
        const STEPS: usize = 14;
        // A couple of slots past the deepest follower so the (fi+1)*STEPS lookup can't fall off the end.
        let needed = (follower_count + 1) * STEPS + 2;
        if self.npc_trains[ni].path_history.len() >= needed {
            return;
        }
        // Trail backward from the current head, opposite the leader's heading (fall back to +x when
        // it's parked). ~7px steps mirror the >6px threshold the live wander samples at.
        let head = self.npc_trains[ni]
            .path_history
            .front()
            .copied()
            .unwrap_or(self.npc_trains[ni].leader_pos);
        let mut dir = -self.npc_trains[ni].leader_vel.normalize_or_zero();
        if dir == Vec2::ZERO {
            dir = Vec2::new(1.0, 0.0);
        }
        let mut p = self.npc_trains[ni]
            .path_history
            .back()
            .copied()
            .unwrap_or(head);
        while self.npc_trains[ni].path_history.len() < needed {
            p += dir * 7.0;
            self.npc_trains[ni].path_history.push_back(p);
        }
    }

    /// Deterministically arm the rival-vs-rival "predator closing" telegraph for the bot: park the
    /// biggest train's leader within hunt range of a smaller rival's leader, both in the world corner
    /// farthest from the player so the player-pursuit `hunting` flag stays clear (dist_to_player >
    /// PURSUIT_RANGE) and the natural rival-hunt urge in `update_npc_trains` arms the gold telegraph
    /// this same frame (bot_fire_events runs before update_npc_trains). It exercises the REAL arming
    /// path — the hunt block reads these live positions and applies its own closeness/gap gating — and
    /// only shortcuts the RNG wander that would otherwise line two leaders up far from the player by
    /// chance. No-op with fewer than two trains or no strictly-smaller rival.
    pub fn force_rival_hunt(&mut self) {
        self.ensure_bot_npc_trains();
        let Some(thief) =
            (0..self.npc_trains.len()).max_by_key(|&i| self.npc_trains[i].follower_types.len())
        else {
            return;
        };
        let thief_len = self.npc_trains[thief].follower_types.len();
        if thief_len < 2 {
            return; // need room for a strictly-smaller rival with at least one follower
        }
        let Some(victim) = (0..self.npc_trains.len()).find(|&v| {
            v != thief && {
                let l = self.npc_trains[v].follower_types.len();
                l >= 1 && l < thief_len
            }
        }) else {
            return;
        };
        // Corner diagonally opposite the player — a full world away, so both leaders sit well beyond
        // PURSUIT_RANGE (550) from the player this frame and the rival hunt (not player pursuit) wins.
        let corner_x = if self.player_pos.x < self.world_width * 0.5 {
            self.world_width * 0.9
        } else {
            self.world_width * 0.1
        };
        let corner_y = if self.player_pos.y < self.world_height * 0.5 {
            self.world_height * 0.9
        } else {
            self.world_height * 0.1
        };
        let victim_pos = Vec2::new(corner_x, corner_y);
        self.npc_trains[victim].leader_pos = victim_pos;
        // 200px apart: inside RIVAL_HUNT_RANGE (620), closeness ≈ 0.68 > the 0.35 arm gate, and >80px
        // so the telegraph line itself draws too.
        self.npc_trains[thief].leader_pos = victim_pos + Vec2::new(-200.0, 0.0);
        self.npc_trains[thief].idle_timer = 0.0;
    }

    /// Bot-test helper (see BotAction::ForceHuntCommit): deterministically drive a rival into the
    /// committed STRIKE phase against the player so the real intercept-steering branch in
    /// `update_npc_trains` runs this frame and `hunt_intercepts` rises. This guards #160's signature
    /// "it read my routing" behavior — a committed hunter leads its aim by the player's velocity to cut
    /// off where the vulnerable back half is *heading* — which none of the other Force* helpers exercise
    /// (they all shortcut straight to the splice, bypassing the stalk→strike pursuit). We stage only the
    /// commit (patience is otherwise RNG/exposure-paced and can't be counted on inside a headless
    /// budget); the interception geometry itself runs the real, unchanged game code.
    ///
    /// Fires the same frame it's called: bot_fire_events → update_crabs (caches the tail/steal target) →
    /// update_npc_trains (reads them, applies intercept steering, bumps hunt_intercepts). A no-op with no
    /// rivals or no wild crabs to build the required stealable chain from.
    pub fn force_hunt_commit(&mut self) {
        if self.npc_trains.is_empty() {
            return;
        }
        // The pursuit `hunting` gate needs a stealable chain (>= 2) so cached_tail_pos resolves; prime
        // it regardless of the autopilot's RNG catch timing, exactly like the other steal helpers.
        self.bot_prime_chain(6);
        if self.chain_count < 2 {
            return;
        }
        let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        let ni = (0..self.npc_trains.len()).min_by(|&a, &b| {
            let da = self.npc_trains[a]
                .leader_pos
                .distance_squared(player_center);
            let db = self.npc_trains[b]
                .leader_pos
                .distance_squared(player_center);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
        let Some(ni) = ni else {
            return;
        };
        // Park the leader ~200px off the player: well inside PURSUIT_RANGE (550) so `hunting` holds, yet
        // far enough that the arrival check (dist < 80) can't flip the train into an idle survey and skip
        // the pursuit block. Point its wander target at the player and hold the target timer so the
        // top-of-loop re-target can't fire an idle either — then force the commit and let the real strike
        // steering run.
        self.npc_trains[ni].leader_pos = player_center + Vec2::new(200.0, 0.0);
        self.npc_trains[ni].target = player_center;
        self.npc_trains[ni].target_timer = 30.0;
        self.npc_trains[ni].idle_timer = 0.0;
        self.npc_trains[ni].stalk_patience = 1.0;
        self.npc_trains[ni].hunt_committed = true;
    }
}
