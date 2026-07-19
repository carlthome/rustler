//! Chain-as-risk mechanics: everything that can knock links loose from the back of the conga
//! train (kelp snags, panic snaps, the King Crab's splice steal, and the latching Thief), plus
//! the deflection wall the caught body of the train presents to fleeing crabs. Extracted from
//! `main.rs` as one cohesive subsystem — these methods all read/write the same tail-of-chain
//! state and call `MainState::recompute_tail_run` after any shrink.

use ggez::glam::Vec2;
use rand::Rng;

use crate::constants::CRAB_SIZE;
use crate::enemies::{BossCharge, CrabType};
use crate::levels::TerrainKind;
use crate::panic_snap_links;
use crate::state::MainState;

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
        if rand::random::<f32>() > SNAG_CHANCE_PER_SEC * dt {
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
            .saturating_sub(panic_snap_links(self.chain_count))
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
        let kick_angle = rand::rng().random_range(0.0_f32..std::f32::consts::TAU);
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
            if c.is_boss() && !c.caught && !c.is_tide_boss() && !c.is_rhythm_boss() {
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
        let kick_angle = rand::rng().random_range(0.0_f32..std::f32::consts::TAU);
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

    /// Thief archetype: a skittish parasite that pressures the *train you've already built* rather
    /// than the herd you're chasing. A free Thief ignores the flee/attract logic and beelines for
    /// your conga tail (its homing is done in update_crabs). Once it reaches the tail it *latches*
    /// on and, on a repeating timer, peels the trailing link loose — that crab reverts to the wild
    /// and bolts, and the Thief keeps gnawing the new tail until you deal with it. Counterplay:
    /// catch the Thief (beam/lasso/chain), whistle it off (whistle_pull is high for Thieves), or
    /// stomp near it — any of those clears the latch. Self-limiting like the other tail risks:
    /// short trains are immune, only the tail goes, never the head, and it shares the chain-snap
    /// cooldown so it can't strip everything in one beat.
    pub(crate) fn steal_chain_thief(&mut self, dt: f32) {
        const MIN_TRAIN_TO_STEAL: usize = 4; // a little shorter than snap — the Thief is a dedicated threat
        const LATCH_DIST: f32 = CRAB_SIZE * 1.1; // how close a Thief must get to the tail to clamp on
        const UNLATCH_DIST: f32 = CRAB_SIZE * 2.4; // if the tail pulls this far away, the clamp breaks
        const LATCH_DIST_SQ: f32 = LATCH_DIST * LATCH_DIST;
        const UNLATCH_DIST_SQ: f32 = UNLATCH_DIST * UNLATCH_DIST;
        const PEEL_INTERVAL: f32 = 1.15; // seconds between links peeled while latched

        // Where's the current tail? (highest chain_index). If the train is too short, no Thief can
        // latch, and any that were latched should let go.
        if self.chain_count < MIN_TRAIN_TO_STEAL {
            for c in &mut self.crabs {
                if c.is_thief() {
                    c.latch_timer = 0.0;
                }
            }
            return;
        }
        // Reuses the tail position update_crabs already computed this frame (same "highest
        // chain_index among caught crabs" lookup) instead of a third O(n) scan over self.crabs.
        let Some(tail_pos) = self.cached_tail_pos else {
            return;
        };

        // Emergent crossover: a roaming Magnet's pull reaches a latched Thief too, and it's
        // stronger than the Thief's grip on your tail. If a clamped Thief drifts inside a free
        // Magnet's radius, the Magnet wins the tug-of-war and rips the parasite clean off the
        // train — the crab you were cursing for gathering a blob becomes an accidental savior.
        // magnet_positions_buf was filled this same frame by update_crabs (runs before us) and
        // holds only *free* Magnets, so a caught Magnet in your own train never triggers this.
        const MAGNET_PRY_RADIUS: f32 = 190.0; // a touch shorter than the herd pull — it has to get close to pry
        const MAGNET_PRY_RADIUS_SQ: f32 = MAGNET_PRY_RADIUS * MAGNET_PRY_RADIUS;
        // Borrow the free-Magnet positions out of self so the &mut self.crabs loop below can call
        // the lookup without an overlapping self borrow; restored at the end of the function.
        let magnet_positions = std::mem::take(&mut self.magnet_positions_buf);
        let nearest_magnet_to = |p: Vec2| -> Option<Vec2> {
            let mut best: Option<(f32, Vec2)> = None;
            for &mp in magnet_positions.iter() {
                let d2 = p.distance_squared(mp);
                if d2 < MAGNET_PRY_RADIUS_SQ && best.map_or(true, |(bd2, _)| d2 < bd2) {
                    best = Some((d2, mp));
                }
            }
            best.map(|(_, mp)| mp)
        };

        // Emergent crossover: a fleeing Golden's panic scares a latched Thief clean off your tail.
        // The Golden's amplified fear (the same GOLDEN_PANIC_AMP-hot ripple that shatters a herd into
        // a stampede) is contagious to the skittish parasite too — a Golden bolting past your train
        // spooks the Thief into bolting itself, letting go of the tail. This is the panic-native
        // mirror of the Magnet-pry save above: there a lodestone rips the Thief off, here a passing
        // prize's fright does it. Only *amplified* carriers (a fleeing Golden, or an ordinary crab
        // still carrying a Golden's hot panic_amp) can do it — a plain panicking crab isn't scary
        // enough to a Thief that's busy raiding. Snapshotted before the &mut self.crabs loop below so
        // the lookup has no overlapping borrow; almost always an empty scan (no Golden mid-flee).
        const GOLDEN_SPOOK_RADIUS: f32 = 130.0;
        const GOLDEN_SPOOK_RADIUS_SQ: f32 = GOLDEN_SPOOK_RADIUS * GOLDEN_SPOOK_RADIUS;
        let mut golden_panic_positions = std::mem::take(&mut self.golden_panic_positions_buf);
        golden_panic_positions.clear();
        golden_panic_positions.extend(self.crabs.iter().filter_map(|c| {
            (!c.caught
                && !c.is_boss()
                && (c.fleeing || c.startle_timer > 0.0)
                && (c.is_golden() || c.panic_amp > 1.05))
                .then_some(c.pos)
        }));
        let nearest_golden_panic_to = |p: Vec2| -> Option<Vec2> {
            let mut best: Option<(f32, Vec2)> = None;
            for &gp in golden_panic_positions.iter() {
                let d2 = p.distance_squared(gp);
                if d2 < GOLDEN_SPOOK_RADIUS_SQ && best.map_or(true, |(bd2, _)| d2 < bd2) {
                    best = Some((d2, gp));
                }
            }
            best.map(|(_, gp)| gp)
        };

        // Emergent crossover: a passing Golden's shine lures a *latched* Thief off your tail. The
        // Golden-lures-Thief pull already diverts a *homing* raider mid-beeline (see update_crabs),
        // but a thief this greedy can't resist a shiny thing even once it's clamped on and gnawing:
        // if a free Golden bolts near a Thief that's already raiding your train, its greed overpowers
        // its grip and it drops the link it was stealing to chase the bigger prize. A third, distinct
        // flavor of latched-Thief save from the two above — the Magnet pry is a physical drag (hauled
        // in), the Golden-panic spook is fright (flees off), and this is pure *greed* (chases away
        // toward the shine, thief_lured aura and all). Softer than both, so it only fires when neither
        // a Magnet nor a fleeing Golden's panic already grabbed the Thief this frame. Reuses the
        // golden_lure_positions_buf snapshot update_crabs already built this frame (free, un-snared
        // Goldens) — no new scan. Almost always an empty check (a free Golden near a raided train is
        // rare), so it costs nothing most frames.
        const GOLDEN_LURE_LATCH_RADIUS: f32 = 220.0;
        const GOLDEN_LURE_LATCH_RADIUS_SQ: f32 =
            GOLDEN_LURE_LATCH_RADIUS * GOLDEN_LURE_LATCH_RADIUS;
        let golden_lure_positions = std::mem::take(&mut self.golden_lure_positions_buf);
        let nearest_golden_lure_to = |p: Vec2| -> Option<Vec2> {
            let mut best: Option<(f32, Vec2)> = None;
            for &gp in golden_lure_positions.iter() {
                let d2 = p.distance_squared(gp);
                if d2 < GOLDEN_LURE_LATCH_RADIUS_SQ && best.map_or(true, |(bd2, _)| d2 < bd2) {
                    best = Some((d2, gp));
                }
            }
            best.map(|(_, gp)| gp)
        };

        // Advance every Thief's latch state; collect whether any peel fired this frame, plus any
        // Thieves a Magnet pried loose, a Golden's panic spooked loose, or a Golden's shine lured
        // off (deferred out of the &mut loop for their freed feedback).
        let mut peel_from: Option<Vec2> = None;
        // Reused scratch buffers (almost always empty — a save firing is rare) instead of three
        // fresh Vec::new() allocations every single frame this unconditionally-run function pays.
        let mut pried_by_magnet = std::mem::take(&mut self.pried_by_magnet_buf);
        pried_by_magnet.clear();
        let mut spooked_by_golden = std::mem::take(&mut self.spooked_by_golden_buf);
        spooked_by_golden.clear();
        let mut lured_by_golden = std::mem::take(&mut self.lured_by_golden_buf);
        lured_by_golden.clear();
        for c in &mut self.crabs {
            if !c.is_thief() || c.caught {
                if c.is_thief() {
                    c.latch_timer = 0.0; // caught Thieves stop stealing
                }
                continue;
            }
            let d_sq = c.pos.distance_squared(tail_pos);
            if c.latch_timer > 0.0 {
                // A nearby Magnet overpowers the clamp: the Thief lets go of the tail and is
                // flung toward the Magnet, joining the loose herd instead of peeling your links.
                if let Some(mp) = nearest_magnet_to(c.pos) {
                    c.latch_timer = 0.0;
                    let dir = (mp - c.pos).normalize_or_zero();
                    let dir = if dir == Vec2::ZERO {
                        Vec2::new(0.0, -1.0)
                    } else {
                        dir
                    };
                    c.vel = dir * c.crab_type.speed_range().end * 1.5;
                    c.speed = 1.0;
                    c.fleeing = false;
                    c.startle_timer = 0.0;
                    pried_by_magnet.push(c.pos);
                    continue;
                }
                // A fleeing Golden's panic washes over the clamped Thief: it spooks and bolts away
                // from the fright, letting go of your tail. It flees the panic source instead of
                // being hauled toward a Magnet, so the crab scatters off into the herd rather than
                // getting balled up — a looser, chaos-flavored save than the Magnet pry.
                if let Some(gp) = nearest_golden_panic_to(c.pos) {
                    c.latch_timer = 0.0;
                    let dir = (c.pos - gp).normalize_or_zero();
                    let dir = if dir == Vec2::ZERO {
                        Vec2::new(0.0, -1.0)
                    } else {
                        dir
                    };
                    c.vel = dir * c.crab_type.speed_range().end * 1.4;
                    c.speed = 1.0;
                    c.fleeing = true;
                    c.startle_timer = 0.5;
                    spooked_by_golden.push(c.pos);
                    continue;
                }
                // A free Golden's shine catches the raiding Thief's eye: greed wins over grip, so it
                // unclamps and darts off toward the prize instead of peeling your links. Unlike the
                // fright spook above it isn't fleeing — it *chases* the shine, so it heads toward the
                // Golden with the same thief_lured gold aura the homing-lure crossover uses. Yields to
                // the Magnet pry and the panic spook (checked first), which are harder pulls.
                if let Some(gp) = nearest_golden_lure_to(c.pos) {
                    c.latch_timer = 0.0;
                    let dir = (gp - c.pos).normalize_or_zero();
                    let dir = if dir == Vec2::ZERO {
                        Vec2::new(0.0, -1.0)
                    } else {
                        dir
                    };
                    c.vel = dir * c.crab_type.speed_range().end * 1.3;
                    c.speed = 1.0;
                    c.fleeing = false;
                    c.startle_timer = 0.0;
                    c.thief_lured = 0.3; // light the gold "chasing shine" aura
                    lured_by_golden.push(c.pos);
                    continue;
                }
                // Already clamped. Ride the tail so it visually hangs off the back of the train.
                if d_sq > UNLATCH_DIST_SQ {
                    c.latch_timer = 0.0; // the train outran it — it drops off
                    continue;
                }
                c.pos = c.pos.lerp(tail_pos, 0.35); // cling to the tail
                c.vel = Vec2::ZERO;
                c.latch_timer -= dt;
                if c.latch_timer <= 0.0 {
                    // Timer fired — this Thief peels a link. Only the first Thief to fire this
                    // frame actually pulls one (peel_from records it); any others just rearm, so a
                    // cluster of Thieves can't strip several links in a single frame.
                    if peel_from.is_none() {
                        peel_from = Some(tail_pos);
                    }
                    c.latch_timer = PEEL_INTERVAL; // rearm for the next peel
                }
            } else if d_sq < LATCH_DIST_SQ {
                // Just reached the tail — clamp on. First peel comes after a full interval so the
                // player gets a beat to react to the latch before losing a link.
                c.latch_timer = PEEL_INTERVAL;
            }
        }
        // The closures (and their borrows of the taken buffers) are done after the loop above, so
        // hand both buffers back to self for next frame's reuse instead of dropping them.
        self.magnet_positions_buf = magnet_positions;
        self.golden_panic_positions_buf = golden_panic_positions;
        self.golden_lure_positions_buf = golden_lure_positions;

        // Feedback for any Thief a Magnet just pried off your tail — a bright orange-green pop and
        // a callout so the save reads as a moment, not a silent stat change. Orange (the Magnet's
        // color) bleeding into thief-green sells the "the Magnet did this" story.
        for pos in pried_by_magnet.drain(..) {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((pos, 0.0));
            }
            self.floating_texts.spawn(
                "MAGNET PRY!".to_string(),
                pos - Vec2::new(52.0, 30.0),
                24.0,
                [0.95, 0.7, 0.3, 1.0],
            );
            self.spawn_catch_shockwave(pos, [0.9, 0.55, 0.25]);
        }

        // Feedback for any Thief a Golden's panic just scared off your tail — a hot-gold fright pop
        // and a callout, so the accidental save reads as a moment. Gold (the prize's color) bleeding
        // into the fright sells the "a passing Golden spooked it loose" story, and distinguishes it
        // from the orange Magnet pry above.
        for pos in spooked_by_golden.drain(..) {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((pos, 0.0));
            }
            self.floating_texts.spawn(
                "SPOOKED OFF!".to_string(),
                pos - Vec2::new(54.0, 30.0),
                24.0,
                [1.0, 0.85, 0.3, 1.0],
            );
            self.spawn_catch_shockwave(pos, [1.0, 0.8, 0.25]);
        }

        // Feedback for any Thief a Golden's shine just lured off your tail — a poison-green "SHINY!"
        // pop matching the homing-lure crossover's cue, so the "it dropped the raid to chase gold"
        // story reads the same whether the Thief was homing or already clamped on. Distinct from the
        // gold "SPOOKED OFF!" fright pop above: this one is greed, not fear.
        for pos in lured_by_golden.drain(..) {
            self.floating_texts.spawn(
                "SHINY!".to_string(),
                pos - Vec2::new(0.0, 30.0),
                22.0,
                [0.7, 0.95, 0.4, 1.0], // Thief's poison-green catching the golden gleam
            );
        }
        // Drained (so empty) either way — hand back for next frame's reuse before any early return.
        self.pried_by_magnet_buf = pried_by_magnet;
        self.spooked_by_golden_buf = spooked_by_golden;
        self.lured_by_golden_buf = lured_by_golden;

        let Some(tail_pos) = peel_from else { return };
        if self.chain_snap_cooldown > 0.0 {
            return; // respect the shared grace period, but the timer already rearmed above
        }

        // Emergent crossover — an Armored crab at the tail is a shell-plated tail-guard. The same
        // stubborn shell that walls off panic ripples and stops a King Crab charge also refuses to
        // be peeled: if the trailing link the Thief is trying to strip is an Armored crab, its shell
        // clangs and the steal is denied outright (the Thief keeps nibbling, but wastes this peel).
        // So deliberately routing an Armored crab to the *back* of your train — where the snap/steal
        // weak point is — turns it into a raid guard, the chain-pressure mirror of parking an Armored
        // crab in a boss's charge lane. Cheap: one scan for the single highest-chain_index crab,
        // only when a peel actually fired this frame.
        let tail_link = self.chain_count.checked_sub(1);
        if let Some(tail_ci) = tail_link {
            let tail_is_armored = self
                .crabs
                .iter()
                .any(|c| c.chain_index == Some(tail_ci) && c.is_armored());
            if tail_is_armored {
                // Shell holds — no link lost. Clang feedback so the save reads as a moment.
                self.chain_snap_cooldown = 0.9; // brief grace before the Thief tries again
                if self.fear_rings.len() < 32 {
                    self.fear_rings.push((tail_pos, 0.0));
                }
                self.floating_texts.spawn(
                    "SHELL HOLDS!".to_string(),
                    tail_pos - Vec2::new(46.0, 30.0),
                    26.0,
                    [0.75, 0.85, 1.0, 1.0],
                );
                self.spawn_catch_shockwave(tail_pos, [0.7, 0.8, 0.95]);
                self.screen_shake = self.screen_shake.max(4.0);
                return;
            }
        }

        // Peel the single trailing link loose — always leave the head attached.
        let keep = self.chain_count.saturating_sub(1).max(1);
        if keep >= self.chain_count {
            return;
        }
        for crab in &mut self.crabs {
            let Some(ci) = crab.chain_index else { continue };
            if ci >= keep {
                crab.caught = false;
                crab.chain_index = None;
                crab.fleeing = true;
                crab.startle_timer = 0.5;
                let outward = (crab.pos - tail_pos).normalize_or_zero();
                let outward = if outward == Vec2::ZERO {
                    Vec2::new(0.0, 1.0)
                } else {
                    outward
                };
                crab.vel = outward * crab.crab_type.speed_range().end * 1.8;
                crab.speed = 1.0;
            }
        }
        self.chain_count = keep;
        self.recompute_tail_run(); // the tail changed — rebuild the same-type run
        self.chain_snap_cooldown = 0.9; // shorter than a panic snap: the Thief keeps nibbling

        // Feedback: a sly green pop and a STOLEN! callout at the tail so the theft reads clearly.
        if self.fear_rings.len() < 32 {
            self.fear_rings.push((tail_pos, 0.0));
        }
        self.floating_texts.spawn(
            "STOLEN! -1".to_string(),
            tail_pos - Vec2::new(28.0, 30.0),
            28.0,
            [0.4, 0.95, 0.5, 1.0],
        );
        self.spawn_catch_shockwave(tail_pos, [0.35, 0.9, 0.45]);
        self.screen_shake = self.screen_shake.max(5.0);
    }

    /// Emergent herding: the solid *body* of the conga train physically deflects panicking wild
    /// crabs, bouncing them off instead of letting them phase through. Slide your line between a
    /// spooked herd and open water and you can corral fleeing crabs back toward your beam for a
    /// free re-catch — turning the train from a number-you-only-grow into a steerable wall you
    /// play the herd against. Mirror of chain-snap: the exposed tail (the same last few links
    /// snap can knock loose) is deliberately *not* a wall, so panic still slips past there. A long
    /// train is a shield up front and a weak point at the back. A charging King Crab bulldozes
    /// through regardless.
    pub(crate) fn deflect_fleeing_off_chain(&mut self) {
        const DEFLECT_DIST: f32 = CRAB_SIZE * 0.85;
        // Only trains long enough to have a snap-vulnerable tail keep that tail soft; shorter
        // trains have no exposed end yet, so their whole body walls.
        let tail_guard = if self.chain_count >= 5 { 3 } else { 0 };
        let body_max = self.chain_count.saturating_sub(tail_guard); // chain_index < body_max = solid wall

        // Gather the solid body segments once into a reused buffer (no per-frame heap churn).
        self.deflect_body_buf.clear();
        for crab in &self.crabs {
            if let Some(ci) = crab.chain_index {
                if ci < body_max {
                    self.deflect_body_buf.push(crab.pos);
                }
            }
        }
        if self.deflect_body_buf.is_empty() {
            return;
        }

        // Bucket body segments into a spatial grid keyed by cell (mirrors catch_by_chain's
        // grid) so each fleeing crab only tests the handful of segments near it instead of
        // scanning the whole chain. Chain length is uncapped and fleeing is common (any wild
        // crab near the player but outside the beam panics), so the old linear scan was an
        // O(fleeing * chain_length) cost that grew for the rest of a long session.
        let cell_size = DEFLECT_DIST.max(1.0);
        let cell_of = |p: Vec2| -> (i32, i32) {
            (
                (p.x / cell_size).floor() as i32,
                (p.y / cell_size).floor() as i32,
            )
        };
        // Same unbounded-key fix as catch_grid_buf: full map clear keeps capacity but bounds
        // iteration to "cells touched this frame", not "cells ever touched over the session".
        self.deflect_grid_buf.clear();
        for (i, &seg) in self.deflect_body_buf.iter().enumerate() {
            self.deflect_grid_buf
                .entry(cell_of(seg))
                .or_default()
                .push(i);
        }

        self.deflect_bounce_buf.clear();
        self.deflect_ricochet_buf.clear();
        let mut rng = rand::rng();
        for (idx, crab) in self.crabs.iter_mut().enumerate() {
            if crab.caught || crab.is_boss() {
                continue;
            }
            if !(crab.fleeing || crab.startle_timer > 0.0) {
                continue;
            }
            // Nearest body segment within collision range, restricted to the 3x3 neighborhood
            // of grid cells around the crab instead of every segment in the chain.
            let (cx, cy) = cell_of(crab.pos);
            let mut hit: Option<(f32, Vec2)> = None;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(candidates) = self.deflect_grid_buf.get(&(cx + dx, cy + dy)) {
                        for &i in candidates {
                            let seg = self.deflect_body_buf[i];
                            let d = seg.distance(crab.pos);
                            if d < DEFLECT_DIST && hit.map_or(true, |(hd, _)| d < hd) {
                                hit = Some((d, seg));
                            }
                        }
                    }
                }
            }
            let Some((_, seg)) = hit else { continue };
            let mut n = (crab.pos - seg).normalize_or_zero();
            if n == Vec2::ZERO {
                n = Vec2::new(0.0, -1.0);
            }
            // Reflect its velocity off the wall only if it's actually heading into the segment,
            // bleeding a little energy so it doesn't ping-pong forever.
            let into = crab.vel.dot(n);
            if into < 0.0 {
                crab.vel = (crab.vel - n * (2.0 * into)) * 0.9;
                crab.speed = 1.0; // vel encodes full speed, matching the flee/startle convention
            }
            // Shove it back out of the wall so it can't tunnel through, and keep it lively.
            crab.pos = seg + n * DEFLECT_DIST;
            crab.startle_timer = crab.startle_timer.max(0.2);
            // Throttled cold ring so the wall-bounce reads without flooding the screen.
            if rng.random::<f32>() < 0.25 {
                self.deflect_bounce_buf.push(crab.pos);
            }
            // Remember it so the ricochet pass below can crash it into other deflected crabs
            // funneled into the same pocket of the wall.
            self.deflect_ricochet_buf.push((idx, crab.pos));
        }
        for &pos in &self.deflect_bounce_buf {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((pos, 0.0));
            }
        }

        // Emergent pile-up: the wall funnels a panicking crowd into its concave pockets, where the
        // crabs it just deflected collide with *each other*. Resolve those pairwise: crabs that
        // overlap ricochet apart and cross-startle, so driving your train into a fleeing herd sets
        // off a self-feeding pinball cascade instead of every crab bouncing off the wall in
        // isolation. Cheap because it only considers crabs deflected *this* frame (usually a
        // handful), bucketed into a grid so each tests just its neighbors.
        self.ricochet_deflected_crabs();
    }

    /// Second half of `deflect_fleeing_off_chain`: crash the crabs the wall just deflected into
    /// each other. Only the small set collected in `deflect_ricochet_buf` participates, so this is
    /// a tiny pass even in a dense herd. Pairs that overlap are pushed apart, have their velocities
    /// swapped along the collision axis (an elastic bounce), and are both freshly startled — the
    /// emergent "the herd panics itself against your train" moment.
    fn ricochet_deflected_crabs(&mut self) {
        const COLLIDE_DIST: f32 = CRAB_SIZE * 0.7;
        if self.deflect_ricochet_buf.len() < 2 {
            return;
        }
        let cell_size = COLLIDE_DIST.max(1.0);
        let cell_of = |p: Vec2| -> (i32, i32) {
            (
                (p.x / cell_size).floor() as i32,
                (p.y / cell_size).floor() as i32,
            )
        };
        // Same unbounded-key fix as the other two grids above.
        self.deflect_ricochet_grid_buf.clear();
        for (bi, &(_, pos)) in self.deflect_ricochet_buf.iter().enumerate() {
            self.deflect_ricochet_grid_buf
                .entry(cell_of(pos))
                .or_default()
                .push(bi);
        }

        self.deflect_collide_buf.clear();
        // Collect the resolved (crab_index, new_pos, new_vel) then apply, so we never hold two
        // mutable borrows into self.crabs at once. Reuses a scratch buffer to avoid per-frame churn.
        let mut resolutions = std::mem::take(&mut self.deflect_resolve_buf);
        resolutions.clear();
        let n = self.deflect_ricochet_buf.len();
        for a in 0..n {
            let (ci_a, pos_a) = self.deflect_ricochet_buf[a];
            let (cx, cy) = cell_of(pos_a);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(candidates) =
                        self.deflect_ricochet_grid_buf.get(&(cx + dx, cy + dy))
                    {
                        for &b in candidates {
                            if b <= a {
                                continue; // resolve each unordered pair once
                            }
                            let (ci_b, pos_b) = self.deflect_ricochet_buf[b];
                            let delta = pos_b - pos_a;
                            let d = delta.length();
                            if d >= COLLIDE_DIST || d <= 0.0001 {
                                continue;
                            }
                            let axis = delta / d;
                            let overlap = COLLIDE_DIST - d;
                            // Read velocities, swap the component along the collision axis (equal-mass
                            // elastic bounce), and separate the pair so they don't stick.
                            let va = self.crabs[ci_a].vel;
                            let vb = self.crabs[ci_b].vel;
                            let van = va.dot(axis);
                            let vbn = vb.dot(axis);
                            let new_va = va + axis * (vbn - van);
                            let new_vb = vb + axis * (van - vbn);
                            let push = axis * (overlap * 0.5 + 1.0);
                            resolutions.push((ci_a, pos_a - push, new_va));
                            resolutions.push((ci_b, pos_b + push, new_vb));
                            // Midpoint cold ring marks the crack; throttled by the len cap below.
                            self.deflect_collide_buf.push(pos_a + axis * (d * 0.5));
                        }
                    }
                }
            }
        }
        for (ci, new_pos, new_vel) in resolutions {
            let crab = &mut self.crabs[ci];
            crab.pos = new_pos;
            crab.vel = new_vel;
            crab.speed = 1.0; // vel carries full speed, matching the flee/startle convention
            crab.startle_timer = crab.startle_timer.max(0.35); // cross-startle: the crash re-panics both
        }
        for &pos in &self.deflect_collide_buf {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((pos, 0.0));
            }
        }
    }
}
