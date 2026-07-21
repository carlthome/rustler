//! Conga-chain mechanics: the risk/reward machinery around the train the player is building.
//! Covers the ways the trailing end can be knocked loose (kelp snags, panic snaps, the King
//! Crab's splice-and-steal, the Thief's latch-and-peel), the train's body deflecting fleeing
//! crabs, catch/combo/milestone scoring, and the bond/sandwich/run arrangement bonuses paid out
//! when the train is delivered. Extracted out of `main.rs`'s `impl MainState` — same methods,
//! same behaviour, just grouped by subsystem instead of living in one file.

use ggez::glam::Vec2;
use rand::Rng;

use crate::constants::*;
use crate::enemies::{BossCharge, CrabType};
use crate::levels::TerrainKind;
use crate::state::MainState;

use std::cell::RefCell;

// Scratch buffer for the rare keep > chain_count fallback in with_bond_index (see below) — reused
// across calls to avoid a per-call heap alloc. The Vec is grown-but-not-shrunk, so it reaches
// steady state after the first run at max chain length and never allocates again during normal
// gameplay.
thread_local! {
    static BOND_INDEX_BUF: RefCell<Vec<Option<CrabType>>> = RefCell::new(Vec::new());
    // Cache of the chain_index -> crab_type lookup, keyed by chain_count. Every event that changes
    // a caught crab's chain_index (catch, release, steal, snap) also reassigns self.chain_count
    // (see the assignments across chain_mechanics.rs, catch_effects.rs, npc_trains.rs) — the same
    // invalidation contract CHAIN_ORDER_CACHE (game_render.rs) already relies on. So on an
    // unchanged-chain frame, every caller below reuses this cached lookup instead of re-scanning
    // self.crabs (which includes free crabs too — a scan over the WHOLE herd, not just the ~keep
    // caught ones this lookup needs).
    static BOND_INDEX_CACHE: RefCell<Option<(usize, Vec<Option<CrabType>>)>> = RefCell::new(None);
}

impl MainState {
    /// Kelp snag: while the conga tail sits in a kelp patch, the fronds can catch and strip a link
    /// or two loose — the Neon Kelp Forest's take on chain-snap. Rolls probabilistically (dt-scaled
    /// so it's framerate-independent) and is gated by the shared chain-snap cooldown, so routing a
    /// long train through the weeds is a real risk to weigh rather than a guaranteed loss. Mirrors
    /// `snap_chain_on_panic`: only long trains are vulnerable, only the tail goes, never the head.
    pub(crate) fn snag_chain_on_kelp(&mut self, dt: f32) {
        const MIN_TRAIN_TO_SNAG: usize = 5;
        const SNAG_LINKS: usize = 2; // gentler than a panic snap — the weeds nibble, they don't tear
        const SNAG_COOLDOWN: f32 = 2.2;
        const SNAG_CHANCE_PER_SEC: f32 = 0.6; // expected snags/sec while the tail sits in kelp

        // Ease the telegraph tension DOWN by default; the danger checks below raise it back up when
        // the tail is actually exposed. Doing it here (a per-frame call) keeps the warning ring
        // fading out smoothly the instant the player routes clear.
        self.kelp_snag_warn = (self.kelp_snag_warn - dt * 1.6).max(0.0);

        if self.current_terrain() != TerrainKind::Kelp || self.chain_count < MIN_TRAIN_TO_SNAG {
            return;
        }

        // Only bite if the tail link is actually inside a kelp patch — route around and you're safe.
        // Reuses the tail position update_crabs already computed this frame instead of rescanning.
        let Some(tail_pos) = self.cached_tail_pos else {
            return;
        };
        // Only the biome's native kelp patches snag — trailing flood pools are Tide Boss water.
        let native_count = self.tide_pools.len().saturating_sub(self.boss_flood_pools);
        let tail_in_kelp = self.tide_pools[..native_count]
            .iter()
            .any(|(c, r)| tail_pos.distance_squared(*c) < *r * *r);
        if !tail_in_kelp {
            return;
        }

        // The tail IS exposed — ramp the telegraph up so the warning ring builds visibly. It fills
        // faster than it fades (above), so lingering in the weeds clearly escalates toward a snag.
        self.kelp_snag_warn = (self.kelp_snag_warn + dt * 2.4).min(1.0);

        // Still gate the actual bite on the shared cooldown — but only AFTER the telegraph has been
        // updated, so the warning keeps pulsing through the grace period between nibbles.
        if self.chain_snap_cooldown > 0.0 {
            return;
        }

        // Probabilistic per-frame roll scaled by dt so the risk is framerate-independent.
        if crate::rng::rng().random::<f32>() > SNAG_CHANCE_PER_SEC * dt {
            return;
        }

        let keep = self.chain_count.saturating_sub(SNAG_LINKS).max(1);
        let snapped = self.chain_count - keep;
        let mut snapped_positions: Vec<Vec2> = Vec::new();
        for crab in &mut self.crabs {
            let Some(ci) = crab.chain_index else { continue };
            if ci >= keep {
                crab.caught = false;
                crab.chain_index = None;
                crab.fleeing = true;
                crab.startle_timer = 0.6;
                let outward = (crab.pos - tail_pos).normalize_or_zero();
                let outward = if outward == Vec2::ZERO {
                    Vec2::new(0.0, 1.0)
                } else {
                    outward
                };
                crab.vel = outward * crab.crab_type.speed_range().end * 1.8;
                crab.speed = 1.0;
                snapped_positions.push(crab.pos);
            }
        }
        self.chain_count = keep;
        self.recompute_tail_run(); // the tail changed — rebuild the same-type run
        self.chain_snap_cooldown = SNAG_COOLDOWN;

        // Feedback: green weed-tinted pops on the stripped crabs and a SNAGGED! callout at the tail.
        for pos in &snapped_positions {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((*pos, 0.0));
            }
            self.floating_texts.spawn(
                "!".to_string(),
                *pos - Vec2::new(0.0, 24.0),
                24.0,
                [0.5, 1.0, 0.6, 1.0],
            );
        }
        self.floating_texts.spawn(
            format!("SNAGGED!  -{}", snapped),
            tail_pos - Vec2::new(30.0, 32.0),
            30.0,
            [0.5, 1.0, 0.6, 1.0],
        );
        self.spawn_catch_shockwave(tail_pos, [0.4, 0.95, 0.5]);
        self.screen_shake = self.screen_shake.max(5.0);
    }

    /// Chain-as-risk: the trailing end of the conga train is exposed and can be knocked loose.
    /// Once the train is long enough to matter, a panicking wild crab (fleeing the beam or
    /// mid-stampede) that barrels into the tail snaps the last few links free — they revert to the
    /// wild and scatter outward. This flips the central mechanic from a pure-upside growing counter
    /// into a moment-to-moment decision: a long conga line is now a bigger, more exposed target you
    /// have to route around spooked herds and actively protect, and can lose the end of.
    /// Self-limiting: short trains are immune, only the tail chunk goes (never the head), and a
    /// cooldown means one brush can't strip the whole train in a single pass.
    pub(crate) fn snap_chain_on_panic(&mut self) {
        const MIN_TRAIN_TO_SNAP: usize = 5; // short trains are safe — the risk only bites once you've invested
        const SNAP_COLLIDE_DIST: f32 = CRAB_SIZE * 0.9;
        const SNAP_COOLDOWN: f32 = 1.6; // grace period so a herd can't strip everything at once

        if self.chain_snap_cooldown > 0.0 || self.chain_count < MIN_TRAIN_TO_SNAP {
            return;
        }
        // The vulnerable end is the most-recently-caught crab (highest chain_index sits at the tail).
        // Reuses the tail position update_crabs already computed this frame instead of rescanning.
        let Some(tail_pos) = self.cached_tail_pos else {
            return;
        };
        // Did a panicking wild crab — or a King Crab mid-lunge — just slam into the tail?
        let hit = self.crabs.iter().any(|c| {
            if c.caught {
                return false;
            }
            if c.is_boss() {
                // A charging King Crab plows through the tail; its bulk gives it a wider reach.
                let boss_reach = SNAP_COLLIDE_DIST + c.scale * CRAB_SIZE * 0.5;
                matches!(c.charge_state, BossCharge::Charging(_))
                    && c.pos.distance_squared(tail_pos) < boss_reach * boss_reach
            } else {
                (c.fleeing || c.startle_timer > 0.0)
                    && c.pos.distance_squared(tail_pos) < SNAP_COLLIDE_DIST * SNAP_COLLIDE_DIST
            }
        });
        if !hit {
            return;
        }

        // Release the tail links — count scales with train length (longer = a bigger, pricier
        // bite), always leaving at least the head crab attached.
        let keep = self
            .chain_count
            .saturating_sub(crate::panic_snap_links(self.chain_count))
            .max(1);
        let snapped = self.chain_count - keep;
        let mut snapped_positions: Vec<Vec2> = Vec::new();
        for crab in &mut self.crabs {
            let Some(ci) = crab.chain_index else { continue };
            if ci >= keep {
                // Revert to the wild and bolt outward from the tail so the break reads clearly.
                crab.caught = false;
                crab.chain_index = None;
                crab.fleeing = true;
                crab.startle_timer = 0.6;
                let outward = (crab.pos - tail_pos).normalize_or_zero();
                let outward = if outward == Vec2::ZERO {
                    Vec2::new(0.0, 1.0)
                } else {
                    outward
                };
                crab.vel = outward * crab.crab_type.speed_range().end * 2.2;
                crab.speed = 1.0; // vel now encodes full speed, matching the flee/startle convention
                snapped_positions.push(crab.pos);
            }
        }
        // Indices 0..keep stay contiguous, so the shortened train and future catches line up cleanly.
        self.chain_count = keep;
        self.recompute_tail_run(); // the tail changed — rebuild the same-type run
        self.chain_snap_cooldown = SNAP_COOLDOWN;

        // Feedback: cold alarm rings + "!" pops on the scattering crabs, a SNAP! callout, and a jolt.
        for pos in &snapped_positions {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((*pos, 0.0));
            }
            self.floating_texts.spawn(
                "!".to_string(),
                *pos - Vec2::new(0.0, 24.0),
                24.0,
                [1.0, 0.5, 0.4, 1.0],
            );
        }
        self.floating_texts.spawn(
            format!("SNAP!  -{}", snapped),
            tail_pos - Vec2::new(24.0, 32.0),
            32.0,
            [1.0, 0.4, 0.3, 1.0],
        );
        self.spawn_catch_shockwave(tail_pos, [1.0, 0.4, 0.3]);
        self.screen_shake = self.screen_shake.max(9.0);
        let kick_angle = crate::rng::rng().random_range(0.0_f32..std::f32::consts::TAU);
        self.screen_shake_vel = Vec2::new(kick_angle.cos(), kick_angle.sin()) * 9.0 * 60.0;
    }

    /// King Crab splice: when a charging boss CROSSES the player's conga line (passes through any
    /// chain segment, not just the tail), it splices the chain at that point. Everything behind the
    /// crossing (higher chain_index) detaches and magnetically flies toward the boss — stolen.
    ///
    /// This is the reverse-Snake + Agar.io core mechanic: the King Crab routes deliberately through
    /// your line to steal crabs, making the chain a high-stakes spatial puzzle to protect.
    pub(crate) fn check_king_crab_splice(&mut self) {
        const SPLICE_COLLIDE_DIST: f32 = CRAB_SIZE * 1.4;
        const SPLICE_COOLDOWN: f32 = 2.0;
        const MAGNET_DURATION: f32 = 0.9;

        if self.king_splice_cooldown > 0.0 || self.chain_count < 2 {
            return;
        }

        let boss_pos: Option<Vec2> = self.crabs.iter().find_map(|c| {
            if c.is_king_crab() && !c.caught {
                if matches!(c.charge_state, crate::enemies::BossCharge::Charging(_)) {
                    Some(c.pos)
                } else {
                    None
                }
            } else {
                None
            }
        });
        let Some(boss_pos) = boss_pos else {
            return;
        };

        let splice_at: Option<usize> = {
            let mut best: Option<usize> = None;
            let d2_thresh = SPLICE_COLLIDE_DIST * SPLICE_COLLIDE_DIST;
            for c in &self.crabs {
                let Some(ci) = c.chain_index else { continue };
                if boss_pos.distance_squared(c.pos) < d2_thresh {
                    if best.map_or(true, |b| ci < b) {
                        best = Some(ci);
                    }
                }
            }
            best
        };
        let Some(cut_ci) = splice_at else {
            return;
        };

        let mut stolen: Vec<(Vec2, [f32; 4])> = Vec::new();
        for c in &self.crabs {
            if c.caught && c.chain_index.map_or(false, |ci| ci >= cut_ci) {
                let [r, g, b] = c.crab_color();
                stolen.push((c.pos, [r, g, b, 1.0]));
            }
        }
        if stolen.is_empty() {
            return;
        }
        let stolen_count = stolen.len();

        for c in &mut self.crabs {
            if c.caught && c.chain_index.map_or(false, |ci| ci >= cut_ci) {
                c.caught = false;
                c.chain_index = None;
                c.vel = Vec2::ZERO;
            }
        }
        self.chain_count = cut_ci;
        self.recompute_tail_run();
        self.king_splice_cooldown = SPLICE_COOLDOWN;

        for (pos, color) in &stolen {
            self.king_stolen_crabs.push((*pos, MAGNET_DURATION, *color));
        }

        let cut_pos = stolen.first().map(|(p, _)| *p).unwrap_or(boss_pos);
        self.spawn_catch_shockwave(cut_pos, [1.0, 0.2, 0.8]);
        self.screen_shake = self.screen_shake.max(10.0);
        let kick_angle = crate::rng::rng().random_range(0.0_f32..std::f32::consts::TAU);
        self.screen_shake_vel = Vec2::new(kick_angle.cos(), kick_angle.sin()) * 10.0 * 60.0;
        self.floating_texts.spawn(
            format!("STOLEN! -{}", stolen_count),
            cut_pos - Vec2::new(40.0, 40.0),
            36.0,
            [1.0, 0.3, 0.9, 1.0],
        );
        for (pos, _) in &stolen {
            if self.fear_rings.len() < 48 {
                self.fear_rings.push((*pos, 0.0));
            }
            self.floating_texts.spawn(
                "!".to_string(),
                *pos - Vec2::new(0.0, 20.0),
                20.0,
                [1.0, 0.4, 0.9, 1.0],
            );
        }
    }

    /// Rebuild `tail_run_len` — the length of the unbroken run of same-type links at the tail — by
    /// walking backward from the current tail. Called after any peel/snap shrinks the train, since
    /// removing tail links can change what the tail is (and thus the run). A no-op-cheap O(n) scan
    /// that only runs on the rare frames a link is actually lost, not every frame. An empty train
    /// has a run of 0.
    pub(crate) fn recompute_tail_run(&mut self) {
        if self.chain_count == 0 {
            self.tail_run_len = 0;
            return;
        }
        // Build a chain_index → CrabType lookup in one O(n) pass so we don't scan self.crabs
        // once per position from the tail toward the head (the old approach was O(run × chain)
        // in the worst case when the whole train is one archetype).
        // Indices are 0..chain_count and contiguous by invariant; the Vec is sized exactly.
        let mut by_index: Vec<Option<CrabType>> = vec![None; self.chain_count];
        for c in &self.crabs {
            if let Some(ci) = c.chain_index {
                if ci < by_index.len() {
                    by_index[ci] = Some(c.crab_type);
                }
            }
        }
        let tail_ci = self.chain_count - 1;
        let Some(tail_type) = by_index[tail_ci] else {
            self.tail_run_len = 0;
            return;
        };
        let mut run = 1u32;
        let mut ci = tail_ci;
        while ci > 0 {
            ci -= 1;
            if by_index[ci] == Some(tail_type) {
                run += 1;
            } else {
                break;
            }
        }
        self.tail_run_len = run;
    }

    pub(crate) fn register_catch(&mut self, catch_pos: Vec2, bonus_points: usize) {
        let mult = self.combo_multiplier();
        self.score += (1 + bonus_points) * mult;
        self.combo_count += 1;
        self.combo_timer = 1.8;

        // Score pop at catch position
        let pts = (1 + bonus_points) * mult;
        let score_text = if pts > 1 {
            format!("+{}  ON BEAT!", pts)
        } else {
            format!("+{}", pts)
        };
        let color = if pts > 1 {
            [1.0, 0.95, 0.3, 1.0]
        } else {
            [1.0, 1.0, 1.0, 0.9]
        };
        self.floating_texts
            .spawn(score_text, catch_pos - Vec2::new(10.0, 20.0), 28.0, color);

        // Combo pop above the player
        if self.combo_count >= 3 {
            let combo_color = match self.combo_count {
                3..=4 => [1.0, 0.6, 0.1, 1.0], // orange
                5..=7 => [1.0, 0.2, 0.2, 1.0], // red
                _ => [0.8, 0.3, 1.0, 1.0],     // purple
            };
            self.floating_texts.spawn(
                format!("x{} COMBO!", self.combo_count),
                self.player_pos - Vec2::new(0.0, 50.0),
                36.0,
                combo_color,
            );
        }
    }

    /// Payoff for catching a Dancer that's actively answering the player's Call. This closes the
    /// Call loop — an on-beat Call summons Dancers toward you, and snapping one up while it's still
    /// answering pays out extra score, a groove surge, and a distinct magenta "DANCE CATCH!" pop
    /// plus a juice punch, so the rhythm summon is worth engaging rather than incidental. Call with
    /// the crab's pre-catch `answering_call` timer and position; a no-op if the crab wasn't answering.
    pub(crate) fn reward_dance_catch(&mut self, was_answering: bool, pos: Vec2) {
        if !was_answering {
            return;
        }
        let mult = self.combo_multiplier();
        let bonus = 3 * mult;
        self.score += bonus;
        self.groove = (self.groove + 0.2).min(1.0);
        self.beat_intensity = (self.beat_intensity + 0.6).min(2.0);
        self.on_beat_flash = (self.on_beat_flash + 0.3).min(0.7);
        self.zoom_punch = self.zoom_punch.max(0.06);
        self.floating_texts.spawn(
            format!("DANCE CATCH! +{}", bonus),
            pos - Vec2::new(60.0, 46.0),
            30.0,
            [1.0, 0.4, 0.9, 1.0],
        );
    }

    pub(crate) fn combo_multiplier(&self) -> usize {
        match self.combo_count {
            0..=2 => 1,
            3..=5 => 2,
            6..=9 => 3,
            _ => 5,
        }
    }

    /// Cash out the live Groove Gamble streak. The player presses B to lock in what they've
    /// built rather than risk it on the next catch. Banking ON the beat secures the FULL current
    /// multiplier as a safe floor; banking off-beat takes a haircut — so the cash-out itself rides
    /// the rhythm. After banking, the live climb continues from the locked floor, so a savvy player
    /// can ratchet a stack safe one bank at a time. Nothing to bank if the live gain over the
    /// existing floor is negligible.
    pub(crate) fn bank_gamble(&mut self) {
        // Only bankable if there's meaningful live gain sitting above the already-locked floor.
        if self.beat_gamble_mult <= self.beat_gamble_locked + 0.24 {
            return;
        }
        let on_beat =
            self.beat_timer < BEAT_WINDOW || self.beat_timer > self.beat_interval - BEAT_WINDOW;
        // On-beat bank locks the whole thing; off-beat only banks 60% of the gain over the floor.
        let gain = self.beat_gamble_mult - self.beat_gamble_locked;
        let banked = if on_beat {
            self.beat_gamble_mult
        } else {
            self.beat_gamble_locked + gain * 0.6
        };
        self.beat_gamble_locked = banked.min(5.0);
        // The live multiplier can't drop below its own new floor; keep climbing from here.
        self.beat_gamble_mult = self.beat_gamble_locked;
        self.gamble_bank_flash = 1.0;
        self.zoom_punch = self.zoom_punch.max(0.045);
        let center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
        let (label, col) = if on_beat {
            ("BANKED ON BEAT!", [0.4, 1.0, 0.6, 1.0])
        } else {
            ("BANKED", [0.7, 0.9, 0.5, 1.0])
        };
        self.floating_texts.spawn(
            format!("{}  x{:.2} SAFE", label, self.beat_gamble_locked),
            center - Vec2::new(0.0, 96.0),
            36.0,
            col,
        );
    }

    pub(crate) fn check_milestone(&mut self, rng: &mut impl rand::Rng) {
        // chain_count is incremented on every catch and decremented on every snap/steal/deliver,
        // so it exactly equals the count of caught crabs — no need to rescan the whole vec.
        let chain_len = self.chain_count;
        if chain_len >= self.next_milestone {
            let milestone = self.next_milestone;
            self.next_milestone += 5;

            // Fireworks burst from player center
            let center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            self.particle_system
                .spawn_milestone_fireworks(center, milestone, rng);

            // Big centered banner text — spawn two: one shadow, one lit
            let banner = format!("{} CRABS!", milestone);
            // Floating texts live in the world layer (drawn before the HUD pass), so anchor the
            // banner near the player so it reads on-screen wherever the camera is, not at a fixed
            // world coordinate that the scrolling camera may have left behind.
            let screen_center = self.player_pos + Vec2::new(-100.0, -160.0);
            // Shadow
            self.floating_texts.spawn(
                banner.clone(),
                screen_center + Vec2::new(3.0, 3.0),
                72.0,
                [0.0, 0.0, 0.0, 0.85],
            );
            // Main text — gold/yellow
            self.floating_texts
                .spawn(banner, screen_center, 72.0, [1.0, 0.92, 0.1, 1.0]);

            // Extra-strong screen shake
            let kick_angle = rng.random_range(0.0_f32..std::f32::consts::TAU);
            self.screen_shake = 25.0;
            self.screen_shake_vel = Vec2::new(kick_angle.cos(), kick_angle.sin()) * 25.0 * 60.0;

            // Big celebratory zoom punch on every milestone.
            self.zoom_punch = self.zoom_punch.max(0.09);

            // Amplify beat flash
            self.beat_intensity = (self.beat_intensity + 1.5).min(2.0);
            self.on_beat_flash = 0.5;
        }
    }

    /// Cash in the train: if the player has a conga line and drives its head into the delivery pen,
    /// bank the whole train for a super-linear score payout (each extra crab is worth more than the
    /// last, so a longer, riskier train pays off disproportionately), then clear the chain and
    /// relocate the pen. This is the "bank now vs. push your luck" beat that closes the risk/reward
    /// loop chain-snap opened.
    /// Count same-type adjacent pairs in the caught train — the arrangement bonus tally. A "bond"
    /// is a caught crab whose immediate predecessor by chain_index is the same archetype. The rope
    /// glow (CHAIN_TYPE_BUF) lights these segments — plus, separately, any sandwich filling (see
    /// count_sandwiches) — so glowing segments equal bonds PLUS non-overlapping sandwiches, not just
    /// bonds. Optionally restricted to chain_index < `keep` so the cleave/snap payouts can count
    /// only the bonds that actually stay attached. O(n): builds a chain_index→type lookup, then
    /// walks it comparing each link to the one ahead.
    fn count_chain_bonds(&self, keep: usize) -> usize {
        self.count_bonds_and_sandwiches(keep).0
    }

    fn count_sandwiches(&self, keep: usize) -> usize {
        self.count_bonds_and_sandwiches(keep).1
    }

    /// Combined bond + sandwich + run-streak tally in a single O(keep) pass over a cached
    /// chain_index->type lookup (see with_bond_index) — returns (bonds, sandwiches,
    /// run_bonus_points, centerpiece_bonus). `run_bonus_points` is already in points
    /// (RUN_STREAK_BONUS summed over every same-type run beyond length 2), not a count, so callers
    /// add it directly. The individual wrappers above exist for call sites that only need one value.
    pub(crate) fn count_bonds_and_sandwiches(&self, keep: usize) -> (usize, usize, usize, usize) {
        if keep < 2 {
            return (0, 0, 0, 0);
        }
        self.with_bond_index(keep, Self::tally_bond_index)
    }

    /// Hand `f` a slice of the chain_index->crab_type lookup for links `0..keep`. The lookup is
    /// cached by chain_count (BOND_INDEX_CACHE) so repeated calls in the same frame — or across
    /// frames where the chain hasn't changed — reuse it instead of re-scanning self.crabs. Falls
    /// back to a direct, uncached build when `keep` exceeds `self.chain_count` (the defensive
    /// "drift" case in try_deliver_train, where a stale chain_count undercounts the actually-caught
    /// crabs) — that's rare enough it isn't worth complicating the cache to cover.
    fn with_bond_index<R>(&self, keep: usize, f: impl FnOnce(&[Option<CrabType>]) -> R) -> R {
        let chain_count = self.chain_count;
        if keep > chain_count {
            return BOND_INDEX_BUF.with(|buf| {
                let mut by_index = buf.borrow_mut();
                by_index.clear();
                by_index.resize(keep, None);
                for c in self.crabs.iter().filter(|c| c.caught) {
                    if let Some(ci) = c.chain_index {
                        if ci < keep {
                            by_index[ci] = Some(c.crab_type);
                        }
                    }
                }
                f(&by_index)
            });
        }
        BOND_INDEX_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            let needs_rebuild = cache.as_ref().map_or(true, |(cc, _)| *cc != chain_count);
            if needs_rebuild {
                // Reuse the Vec already stored in the cache (if any) to avoid a heap allocation on
                // every catch/release event — grow-only, never shrunk.
                let mut by_index = cache.take().map(|(_, v)| v).unwrap_or_default();
                by_index.clear();
                by_index.resize(chain_count, None);
                for c in self.crabs.iter().filter(|c| c.caught) {
                    if let Some(ci) = c.chain_index {
                        if ci < chain_count {
                            by_index[ci] = Some(c.crab_type);
                        }
                    }
                }
                *cache = Some((chain_count, by_index));
            }
            f(&cache.as_ref().unwrap().1[..keep])
        })
    }

    /// Arithmetic half of count_bonds_and_sandwiches: given the chain_index->type lookup for links
    /// `0..keep` (keep == by_index.len()), tally (bonds, sandwiches, run_bonus_points,
    /// centerpiece_bonus). Split out so with_bond_index's cached lookup can feed it directly.
    fn tally_bond_index(by_index: &[Option<CrabType>]) -> (usize, usize, usize, usize) {
        let keep = by_index.len();
        let mut bonds = 0;
        for i in 1..keep {
            if by_index[i].is_some() && by_index[i] == by_index[i - 1] {
                bonds += 1;
            }
        }
        let mut sandwiches = 0;
        if keep >= 3 {
            for i in 1..keep - 1 {
                // Both neighbors must be the SAME figurehead archetype (Golden or Dancer). The
                // filling itself can be anything — including another figurehead, so a G-G-G run
                // makes the middle a sandwich too (and still pays its two adjacency bonds; that's a
                // deliberately-arranged cluster, so paying both is intended).
                let left = by_index[i - 1];
                let right = by_index[i + 1];
                if left == right
                    && matches!(left, Some(CrabType::Golden) | Some(CrabType::Dancer))
                    && by_index[i].is_some()
                {
                    sandwiches += 1;
                }
            }
        }
        // Deep-run escalator: walk the contiguous same-type runs and pay RUN_STREAK_BONUS for
        // every crab beyond the third in each run (a run of length L pays L-2 kickers). Same
        // by_index lookup as bonds/sandwiches — one more linear pass, no extra crab scan.
        let mut run_bonus_points = 0;
        // CENTERPIECE: a same-type run of length >= 3 that straddles the train's midpoint pays
        // a flat bonus once per qualifying run — positional identity for the MIDDLE of the line
        // (a deep run seated in the protected center beats one dangling at the snappable tail).
        // The midpoint is a link boundary at keep/2; a run [start..=end] straddles it when it
        // spans that boundary, i.e. start <= mid-1 and end >= mid (using half-open indices).
        let mut centerpiece_bonus = 0;
        let mid = keep / 2;
        let mut run_len = 0usize; // length of the current same-type run ending at i-1
        let mut run_start = 0usize; // chain_index where the current run began
        let close_run = |len: usize, start: usize, end_exclusive: usize| -> usize {
            // Runs of length >= 3 straddling the midpoint earn the centerpiece kicker.
            if len >= 3 && start < mid && end_exclusive > mid {
                CENTERPIECE_BONUS
            } else {
                0
            }
        };
        for i in 0..keep {
            let extends = i > 0 && by_index[i].is_some() && by_index[i] == by_index[i - 1];
            if extends {
                run_len += 1;
            } else {
                // A run just ended (or the chain begins). Score the run we were building, then
                // start a fresh one at this link (length 1 if occupied, 0 if a gap).
                run_bonus_points += run_len.saturating_sub(2) * RUN_STREAK_BONUS;
                if run_len > 0 {
                    centerpiece_bonus += close_run(run_len, run_start, i);
                }
                run_len = if by_index[i].is_some() { 1 } else { 0 };
                run_start = i;
            }
        }
        // Score the final trailing run, which never hit a boundary to close it above.
        run_bonus_points += run_len.saturating_sub(2) * RUN_STREAK_BONUS;
        if run_len > 0 {
            centerpiece_bonus += close_run(run_len, run_start, keep);
        }
        (bonds, sandwiches, run_bonus_points, centerpiece_bonus)
    }

    /// Which seated chain_index links currently belong to a PAYING centerpiece run, so the live
    /// draw pass can ring them and the player sees the protected mid-run *forming* instead of only
    /// learning about it at the pen. The predicate here is deliberately identical to `close_run`
    /// inside `count_bonds_and_sandwiches` (same-type run of `len >= 3` straddling the midpoint at
    /// `keep/2`): if the two ever drifted, we'd highlight a "centerpiece" that doesn't pay (or hide
    /// one that does), teaching the player the wrong arrangement. Returns a small owned Vec of the
    /// qualifying indices (trains are short; typically 0-1 runs); empty when nothing qualifies.
    /// Fill `out` with the chain indices that belong to a paying CENTERPIECE run.
    /// Uses a reused scratch buffer (`out`) rather than allocating a fresh Vec every call —
    /// this runs once per draw frame; at 60 fps on a long train that was a Vec::new() + heap
    /// alloc every 16 ms. Caller must clear `out` before calling.
    pub(crate) fn centerpiece_link_indices(&self, keep: usize, out: &mut Vec<usize>) {
        if keep < 3 {
            return;
        }
        // Shares BOND_INDEX_CACHE with count_bonds_and_sandwiches — this is the same lookup at the
        // same `keep` (both called once per draw frame with self.chain_count), so reusing it here
        // means the chain_index->type build happens at most once per frame instead of twice.
        self.with_bond_index(keep, |by_index| {
            let mid = keep / 2;
            let mut run_len = 0usize;
            let mut run_start = 0usize;
            let flush =
                |len: usize, start: usize, end_exclusive: usize, out: &mut Vec<usize>| {
                    if len >= 3 && start < mid && end_exclusive > mid {
                        out.extend(start..end_exclusive);
                    }
                };
            for i in 0..keep {
                let extends = i > 0 && by_index[i].is_some() && by_index[i] == by_index[i - 1];
                if extends {
                    run_len += 1;
                } else {
                    if run_len > 0 {
                        flush(run_len, run_start, i, out);
                    }
                    run_len = if by_index[i].is_some() { 1 } else { 0 };
                    run_start = i;
                }
            }
            if run_len > 0 {
                flush(run_len, run_start, keep, out);
            }
        });
    }
}
