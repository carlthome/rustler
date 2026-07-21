//! Catch reward payoffs and boss/arena set-piece effects: the Golden Crab treasure bonus,
//! the Splitter cleave (bank-half-the-train risk/reward), the Magnet+Golden shine cascade,
//! the King Crab/Tide Boss caught celebration, and the boss-fight arena hazards (cracked
//! floor fissures, arena flooding, the Tide Boss pulse shockwave). Extracted out of
//! main.rs's impl MainState — same methods, same behaviour, just grouped by subsystem.

use ggez::glam::Vec2;
use rand::Rng;

use crate::constants::*;
use crate::enemies::CrabType;
use crate::state::MainState;

impl MainState {
    /// Treasure payoff when a rare Golden Crab is snagged. On top of the normal catch award (already
    /// added in the catch loop), this pays a big lump-sum bonus and throws a gold sparkle-burst so
    /// the moment lands like finding treasure. The bonus scales with the combo multiplier so a
    /// golden grab mid-hot-streak is a genuine jackpot — the reward for committing to the chase.
    pub(crate) fn on_golden_caught(&mut self, pos: Vec2, base_pts: usize) {
        let mut rng = crate::rng::rng();
        // Flat treasure bonus scaled by the current combo multiplier, floored so it always feels big.
        let bonus = (30 * self.combo_multiplier()).max(30);
        self.score += bonus;
        // Gold sparkle-burst + shockwave so the catch reads as a jackpot, not a normal snag.
        self.particle_system
            .spawn_milestone_fireworks(pos, 14, &mut rng);
        self.spawn_catch_shockwave(pos, [1.0, 0.85, 0.25]);
        self.floating_texts.spawn(
            format!("GOLDEN! +{}", bonus),
            pos - Vec2::new(60.0, 40.0),
            42.0,
            [1.0, 0.9, 0.3, 1.0],
        );
        // Extra juice: a short freeze, a camera punch, and a groove kick reward the risky chase.
        self.hitstop_timer = self.hitstop_timer.max(0.09);
        self.zoom_punch = self.zoom_punch.max(0.08);
        self.shake_timer = self.shake_timer.max(0.45);
        self.groove = (self.groove + 0.25).min(1.0);
        let _ = base_pts; // base points already banked in the catch loop; kept for future tuning.
    }

    /// Splitter cleave — the arrangement *bet*. Catching a Splitter cleaves the conga train at its
    /// midpoint and instantly BANKS the back half for points (a partial cash-out), leaving the front
    /// half as a shorter, re-indexed train that keeps rolling. It reuses the delivery payout curve
    /// (super-linear triangular sum) so cashing a slice at speed genuinely pays, and the peel-scatter
    /// juice so the cleave reads on screen. The "bet" is a timing gamble: catch the Splitter ON the beat
    /// for a clean cut (full bank + Jackpot on the slice composition), or OFF the beat for a sloppy cut
    /// (half bank, no jackpot). So you sacrifice the length and match-run shape you'd built for a slice of
    /// score — timed to the beat it's the big cash, off-beat it's a mediocre partial, so dodging a
    /// Splitter to keep building a run you'd only half-cash is a live call. Because the Splitter itself is
    /// the freshly-caught tail, it always lands in the banked back half (you never keep the cleaver).
    ///
    /// The Splitter also plugs into the archetype crossover web (its whole point — see the roadmap's
    /// "emergent web"): the *composition* of the cleaved back half pays a Jackpot Cleave — Goldens and
    /// Magnets in the slice, and a live tail match-run the cut cashes, each add a bonus and escalate the
    /// juice. So the bet is over what the tail is *made of*, not just how long it is: a mid-match-run
    /// cleave with a Golden parked in back is the big score; a bare cut is the safe partial cash-out.
    /// The links that would be cleaved: `(keep, banked)` where every chain_index >= keep banks and
    /// the front `keep` links stay attached. Split at the midpoint. Single source of truth for the
    /// cut point, shared by the cleave itself and the pre-catch stakes preview so they can never drift.
    pub(crate) fn cleave_split_point(&self) -> (usize, usize) {
        let keep = self.chain_count / 2;
        (keep, self.chain_count - keep)
    }

    /// What a CLEAN (on-beat) cleave would bank *right now*, base slice payout plus the full Jackpot
    /// crossover (Goldens/Magnets/cashed match-run in the back half). This is the exact number the
    /// clean branch of `split_train_bank` pays — extracted so the floating stakes preview shows the
    /// real bet, not a re-derived guess that silently diverges the next time the formula is edited.
    /// Returns `(worth, jackpot)`: `jackpot` is whether any composition crossover would fire.
    pub(crate) fn cleave_clean_worth(&self) -> (usize, bool) {
        if self.chain_count == 0 {
            return (0, false);
        }
        let (keep, banked) = self.cleave_split_point();
        let base = (banked * (banked + 1) / 2) * 3;
        let combo = self.combo_multiplier();
        let mut worth = (base as f32 * combo as f32 * self.beat_gamble_mult).round() as usize;

        let (golden_in_slice, magnet_in_slice) = self.crabs.iter().fold((0, 0), |(g, m), c| {
            if c.caught && c.chain_index.map_or(false, |ci| ci >= keep) {
                (g + c.is_golden() as usize, m + c.is_magnet() as usize)
            } else {
                (g, m)
            }
        });
        let cashed_run = if self.tail_run_len >= 3 {
            self.tail_run_len
        } else {
            0
        };
        let golden_bonus = golden_in_slice * 120 * combo;
        let magnet_bonus = if magnet_in_slice > 0 {
            magnet_in_slice * banked.max(1) * 6 * combo
        } else {
            0
        };
        let run_bonus = (cashed_run as usize) * (cashed_run as usize) * 5 * combo;
        let crossover = golden_bonus + magnet_bonus + run_bonus;
        worth += crossover;
        (worth, crossover > 0)
    }

    pub(crate) fn split_train_bank(&mut self, at: Vec2) {
        // Nothing to cleave a meaningful chunk out of — a 1-2 link train just banks whatever's there.
        if self.chain_count == 0 {
            return;
        }
        // Cleave point: everything at chain_index >= keep banks. Split at the midpoint, but always
        // bank at least the Splitter link itself (the tail) so the catch always does *something*.
        let (keep, banked) = self.cleave_split_point();

        // THE BET — an on-beat gate turns the cleave from pure upside into a genuine timing gamble.
        // Catch the Splitter ON the beat and the cut lands clean: full bank + the Jackpot payout below.
        // Catch it OFF the beat and the cut is sloppy — the back half still banks, but at HALF value and
        // with NO jackpot (Goldens/Magnets/match-run in the slice pay nothing extra). So grabbing a
        // Splitter mid-run is a real decision: time it to the beat to cash the slice for its full
        // Jackpot worth, or dodge it to keep building a run you'd only half-cash off the beat. We grade
        // it (half payout) rather than wiping the back half outright, because Splitters are usually
        // reeled in with pull tools (whistle/beam) where the player commits to the catch but not the
        // exact frame — a total loss on a ~68%-of-the-bar off-beat window would read as a punish, not a
        // bet. A soft miss keeps it a live risk/reward read without the feel-bad wipe of half your train.
        let clean = self.on_beat_now();

        // Component scan for the JACKPOT tag naming and juice below — WHICH crabs sit in the cleaved
        // back half (Goldens/Magnets, and a live tail match-run). Single pass over the banked slice.
        // The clean-cut TOTAL itself comes from cleave_clean_worth() so the preview tag and the actual
        // payout share exactly one formula and can't drift; here we only need the breakdown for naming.
        let (golden_in_slice, magnet_in_slice) = self.crabs.iter().fold((0, 0), |(g, m), c| {
            if c.caught && c.chain_index.map_or(false, |ci| ci >= keep) {
                (g + c.is_golden() as usize, m + c.is_magnet() as usize)
            } else {
                (g, m)
            }
        });
        // The tail match-run lives at the very back of the train, so the cleave always cashes it in
        // full — capture its length before recompute wipes it. Only counts as a "cashed run" at 3+.
        let cashed_run = if self.tail_run_len >= 3 {
            self.tail_run_len
        } else {
            0
        };
        let _ = magnet_in_slice; // consumed inside cleave_clean_worth; kept here only for parity of the scan

        // THE PAYOUT. On the beat the cut is clean: bank the full cleave_clean_worth() (base slice +
        // Jackpot crossover), the exact figure the pre-catch stakes tag previewed. Off the beat it's a
        // sloppy half-cut: half the base slice, no crossover — so timing is what turns a good tail into
        // a jackpot. Single source of truth for the clean total; off-beat is derived from the same base.
        let (bank, jackpot) = if clean {
            let (worth, jackpot) = self.cleave_clean_worth();
            (worth, jackpot)
        } else {
            let base = (banked * (banked + 1) / 2) * 3;
            let half = (base as f32 * self.combo_multiplier() as f32 * self.beat_gamble_mult * 0.5)
                .round() as usize;
            (half, false)
        };
        self.score += bank;

        // Collect the banked crabs (chain_index >= keep) so they can parade into the pen like a
        // normal delivery, then leave the field. self.crabs isn't index-ordered, so sort the banked
        // slice head-first for a clean parade. Cheap — only runs on the rare Splitter catch.
        let mut ordered: Vec<(usize, Vec2, [f32; 3], f32)> = self
            .crabs
            .iter()
            .filter(|c| c.caught && c.chain_index.map_or(false, |ci| ci >= keep))
            .map(|c| {
                (
                    c.chain_index.unwrap_or(usize::MAX),
                    c.pos,
                    c.crab_color(),
                    c.scale,
                )
            })
            .collect();
        ordered.sort_unstable_by_key(|&(ci, ..)| ci);
        let marching: Vec<(Vec2, [f32; 3], f32)> = ordered
            .into_iter()
            .map(|(_ci, p, col, s)| (p, col, s))
            .collect();
        // March the banked slice into the delivery pen, same as a real bank, so the cleave visibly
        // cashes out toward the pen rather than blinking away at the split point.
        self.penned_marchers.spawn_train(self.pen_pos, &marching);

        // Remove the banked crabs from the field entirely — they've been cashed. The front half
        // (chain_index < keep) stays attached and keeps its indices contiguous (0..keep), so the
        // shortened train and all future catches line up cleanly.
        self.crabs
            .retain(|c| !(c.caught && c.chain_index.map_or(false, |ci| ci >= keep)));
        self.chain_count = keep;
        self.recompute_tail_run(); // the tail changed (the whole back half, incl. any match run, is gone)

        // Feedback: a bright teal cleave-shockwave + fireworks at the split point and a legible
        // SPLIT BANKED callout, so the bet paying off reads on screen. A camera jolt sells the cleave.
        // When the cut lands a crossover (a Golden/Magnet in the slice or a live match-run cashed),
        // the moment escalates — gold shockwave, extra fireworks, a bigger kick, and a JACKPOT
        // CLEAVE callout naming what paid — so "oh, THAT happened" reads at a glance.
        // Cleave slash: a blade stroke from the last kept front link to the split point, drawn for a
        // few frames so the cut visibly bisects the train. The front endpoint is the link that's now
        // the new tail (chain_index == keep-1); if there's no front half left, slash from `at` itself.
        let front_tail = if keep > 0 {
            self.crabs
                .iter()
                .find(|c| c.caught && c.chain_index == Some(keep - 1))
                .map(|c| c.pos)
                .unwrap_or(at)
        } else {
            at
        };
        self.cleave_a = front_tail;
        self.cleave_b = at;
        self.cleave_flash = 1.0;
        self.cleave_gold = jackpot;

        let mut rng = crate::rng::rng();
        let (shock_col, extra_bursts) = if jackpot {
            ([1.0, 0.85, 0.25], banked.max(1) + 6)
        } else {
            ([0.2, 0.95, 0.85], banked.max(1))
        };
        self.particle_system
            .spawn_milestone_fireworks(at, extra_bursts, &mut rng);
        self.spawn_catch_shockwave(at, shock_col);
        if jackpot {
            // Name the payoff so the crossover reads, biggest contributor first.
            let tag = if cashed_run > 0 {
                format!("JACKPOT CLEAVE! RUN x{} +{}", cashed_run, bank)
            } else if golden_in_slice > 0 {
                format!("JACKPOT CLEAVE! GOLD +{}", bank)
            } else {
                format!("JACKPOT CLEAVE! MAGNET +{}", bank)
            };
            self.floating_texts.spawn(
                tag,
                at - Vec2::new(110.0, 42.0),
                46.0,
                [1.0, 0.9, 0.35, 1.0],
            );
            self.screen_shake = self.screen_shake.max(13.0);
            // Directional kick away from the player — the cleave "recoils" outward so the cut
            // has a felt direction, not just omnidirectional rumble.
            {
                let kick_dir = (at - self.player_pos).try_normalize().unwrap_or(Vec2::X);
                let vel = kick_dir * 13.0 * 60.0;
                if self.screen_shake_vel.length_squared() < vel.length_squared() {
                    self.screen_shake_vel = vel;
                }
            }
            self.hitstop_timer = self.hitstop_timer.max(0.1);
            self.zoom_punch = self.zoom_punch.max(0.085);
            self.on_beat_flash = self.on_beat_flash.max(0.4);
            self.groove = (self.groove + 0.15).min(1.0);
        } else if clean {
            self.floating_texts.spawn(
                format!("SPLIT BANKED +{}", bank),
                at - Vec2::new(70.0, 40.0),
                44.0,
                [0.4, 1.0, 0.9, 1.0],
            );
            self.screen_shake = self.screen_shake.max(8.0);
            {
                let kick_dir = (at - self.player_pos).try_normalize().unwrap_or(Vec2::X);
                let vel = kick_dir * 8.0 * 60.0;
                if self.screen_shake_vel.length_squared() < vel.length_squared() {
                    self.screen_shake_vel = vel;
                }
            }
            self.hitstop_timer = self.hitstop_timer.max(0.07);
            self.zoom_punch = self.zoom_punch.max(0.05);
        } else {
            // Off-beat: the sloppy cut reads as a miss — a dimmer, redder callout naming the lost value
            // (half bank, no jackpot) so the player learns to time the Splitter to the beat next time.
            self.floating_texts.spawn(
                format!("SLOPPY CUT +{}", bank),
                at - Vec2::new(70.0, 40.0),
                40.0,
                [1.0, 0.6, 0.45, 1.0],
            );
            self.screen_shake = self.screen_shake.max(5.0);
            {
                let kick_dir = (at - self.player_pos).try_normalize().unwrap_or(Vec2::X);
                let vel = kick_dir * 5.0 * 60.0;
                if self.screen_shake_vel.length_squared() < vel.length_squared() {
                    self.screen_shake_vel = vel;
                }
            }
            self.hitstop_timer = self.hitstop_timer.max(0.05);
            self.zoom_punch = self.zoom_punch.max(0.03);
        }
    }

    /// Crossover payoff — the Magnet-link shine cascade. Fires when a Golden is caught directly
    /// behind a Magnet link in the train (a catch *order* the player sets up on purpose: park a
    /// Magnet at the tail, then chase a Golden onto it). The Magnet's field conducts the Golden's
    /// shine down the entire conga line, paying a bonus that scales with how long the train is —
    /// so the longer the line you've routed the shine through, the bigger the reward — and firing a
    /// gold whip-streak that visibly ripples from the tail up to the head so the cascade reads on
    /// screen. Reuses the existing catch-trail whip streak (no new draw path) per the "reuse
    /// existing verbs, make it a legible watchable reaction" spirit of the roadmap item.
    pub(crate) fn on_magnet_shine_cascade(&mut self, golden_pos: Vec2) {
        // Bonus scales with the number of links the shine travels through — the whole point is that
        // a longer train you've deliberately built pays off more. Floored so even a short line feels
        // worth the setup, and scaled by the live combo multiplier like the other catch rewards.
        let links = self.chain_count.max(1);
        let bonus = (8 * links * self.combo_multiplier()).max(40);
        self.score += bonus;

        // Collect the caught-train positions ordered head->tail so we can chain gold whip-streaks
        // link-to-link. O(n) + sort, but this only runs on the rare engineered Magnet+Golden catch,
        // so it's off the hot path. Reuses the pooled deflect_body_buf's sibling pattern via a fresh
        // small local — the cascade is rare enough that a one-off Vec here is fine.
        let mut links_sorted: Vec<(usize, Vec2)> = self
            .crabs
            .iter()
            .filter_map(|c| c.chain_index.map(|ci| (ci, c.pos)))
            .collect();
        links_sorted.sort_unstable_by_key(|&(ci, _)| ci);

        // Whip-streaks hopping from each link to the next, staggered so the shine visibly travels
        // from the tail (where the Golden joined) up toward the head. Later hops start "younger"
        // (more negative age) so they light up after the ones nearer the tail — a rolling cascade.
        const SHINE: [f32; 3] = [1.0, 0.9, 0.35];
        let n = links_sorted.len();
        for i in (1..n).rev() {
            if self.catch_trails.len() >= 48 {
                break;
            }
            let from = links_sorted[i].1;
            let to = links_sorted[i - 1].1;
            // Tail hop starts now; each hop toward the head is delayed a hair for the ripple.
            let stagger = -0.04 * (n - i) as f32;
            self.catch_trails.push((from, to, stagger.max(-0.6), SHINE));
        }

        // Punchy feedback so the cascade lands as a moment, not a silent score bump: a gold
        // shockwave at the tail, a length-aware callout, fireworks, and a beat/camera kick.
        self.spawn_catch_shockwave(golden_pos, SHINE);
        self.particle_system
            .spawn_milestone_fireworks(golden_pos, 12, &mut crate::rng::rng());
        self.floating_texts.spawn(
            format!("SHINE CASCADE! +{}  ({} links)", bonus, links),
            golden_pos - Vec2::new(90.0, 58.0),
            40.0,
            [1.0, 0.92, 0.4, 1.0],
        );
        self.zoom_punch = self.zoom_punch.max(0.09);
        self.hitstop_timer = self.hitstop_timer.max(0.08);
        self.screen_shake = self.screen_shake.max(10.0);
        self.on_beat_flash = self.on_beat_flash.max(0.4);
        self.groove = (self.groove + 0.2).min(1.0);
    }

    /// Big celebratory payoff when a worn-down boss is finally snagged. `is_tide` swaps the callout
    /// and shockwave color so the Tide Boss reads as its own catch, not a reskinned King Crab.
    pub(crate) fn on_boss_caught(&mut self, pos: Vec2, crab_type: CrabType) {
        let mut rng = crate::rng::rng();
        // The Hermit King "counts as 3 chain links" — the big boy pays a triple-size lump sum
        // (its slot in the chain stays one crab so chain bookkeeping stays simple; the payoff is
        // the fat bank, not the geometry).
        let base: usize = if matches!(crab_type, CrabType::HermitKing) {
            75
        } else {
            25
        };
        let bonus = base * self.combo_multiplier();
        self.score += bonus;
        self.particle_system
            .spawn_milestone_fireworks(pos, 30, &mut rng);
        // World-layer text: anchor to the player so the boss-caught banner reads on-screen under
        // the scrolling camera rather than at a fixed world coordinate.
        let screen_center = self.player_pos + Vec2::new(-200.0, -170.0);
        let (label, label_color, shock_color): (&str, [f32; 4], [f32; 3]) = match crab_type {
            CrabType::TideBoss => ("TIDE BOSS CAUGHT!", [0.4, 0.85, 1.0, 1.0], [0.3, 0.75, 1.0]),
            CrabType::RhythmBoss => ("REEF DJ CAUGHT!", [0.8, 0.5, 1.0, 1.0], [0.72, 0.3, 0.95]),
            CrabType::HermitKing => (
                "HERMIT KING CAUGHT!",
                [1.0, 0.65, 0.3, 1.0],
                [0.85, 0.5, 0.2],
            ),
            CrabType::DancerKing => (
                "DANCER KING CAUGHT!",
                [1.0, 0.68, 0.55, 1.0],
                [1.0, 0.62, 0.45],
            ),
            _ => ("KING CRAB CAUGHT!", [1.0, 0.85, 0.2, 1.0], [1.0, 0.8, 0.2]),
        };
        self.floating_texts.spawn(
            label.to_string(),
            screen_center + Vec2::new(3.0, 3.0),
            64.0,
            [0.0, 0.0, 0.0, 0.85],
        );
        self.floating_texts
            .spawn(label.to_string(), screen_center, 64.0, label_color);
        self.floating_texts.spawn(
            format!("+{}", bonus),
            pos - Vec2::new(20.0, 30.0),
            40.0,
            [1.0, 0.95, 0.3, 1.0],
        );
        let a = rng.random_range(0.0_f32..std::f32::consts::TAU);
        self.screen_shake = 30.0;
        self.screen_shake_vel = Vec2::new(a.cos(), a.sin()) * 30.0 * 60.0;
        self.zoom_punch = self.zoom_punch.max(0.11);
        self.hitstop_timer = self.hitstop_timer.max(0.12);
        // The hard-freeze punch lands first; once it clears, bullet-time takes over so the whole
        // victory — fireworks, the boss's last flail, the arena healing — plays out in slow motion.
        self.slowmo_timer = SLOWMO_DURATION;
        self.beat_intensity = 2.0;
        self.on_beat_flash = 0.6;
        if self.catch_shockwaves.len() < 48 {
            self.catch_shockwaves.push((pos, 0.0, shock_color));
        }

        // The duel's over: the arena the boss reshaped heals. King Crab fissures seal (with a puff
        // of receding light) and any flood water the Tide Boss surged in recedes back off, leaving
        // only the biome's own pools. Recede exactly `boss_flood_pools` from the tail of the vec —
        // flood pools are always appended, so they're the last N entries.
        for &(fc, _, _) in &self.boss_fissures {
            if self.catch_shockwaves.len() < 48 {
                self.catch_shockwaves.push((fc, 0.0, [1.0, 0.6, 0.2]));
            }
        }
        self.boss_fissures.clear();
        self.boss_fissure_erupt = 0.0;
        if self.boss_flood_pools > 0 {
            let drain = self.boss_flood_pools.min(self.tide_pools.len());
            let new_len = self.tide_pools.len() - drain;
            self.tide_pools.truncate(new_len);
            self.boss_flood_pools = 0;
        }
    }

    /// King Crab enrage set-piece: the boss slams the seabed and CRACKS THE FLOOR, splitting the
    /// arena into a scatter of glowing fissures the player must weave the conga tail around for the
    /// rest of the duel (see `damage_tail_in_fissures`). Fissures are kept off the delivery pen (so
    /// banking never becomes a coin flip), off the boss's own spot, and spaced apart so they read as
    /// distinct lanes to thread rather than one big kill zone. Cleared when the boss is caught.
    pub(crate) fn crack_arena_fissures(&mut self, boss_pos: Vec2) {
        let mut rng = crate::rng::rng();
        let count = 5;
        let mut placed = 0;
        let mut attempts = 0;
        while placed < count && attempts < 60 {
            attempts += 1;
            let radius = rng.random_range(56.0..92.0);
            let margin = radius + 30.0;
            // Cracks well up around the boss/fight, not across the whole (now larger-than-viewport)
            // world — sample a ring around the boss so the set-piece reshapes the arena the player
            // is standing in, clamped to world bounds.
            let ang = rng.random_range(0.0..std::f32::consts::TAU);
            let dist = rng.random_range(0.0..self.height * 0.45);
            let c = Vec2::new(
                (boss_pos.x + ang.cos() * dist).clamp(margin, self.world_width - margin),
                (boss_pos.y + ang.sin() * dist).clamp(margin, self.world_height - margin),
            );
            if c.distance(self.pen_pos) < radius + PEN_RADIUS + 50.0 {
                continue;
            }
            if c.distance(boss_pos) < radius + 90.0 {
                continue;
            }
            if self
                .boss_fissures
                .iter()
                .any(|(fc, fr, _)| c.distance(*fc) < radius + fr + 60.0)
            {
                continue;
            }
            self.boss_fissures.push((c, radius, 0.0));
            placed += 1;
        }
        // A loud callout so the player reads the arena change, not just "the boss got faster".
        self.floating_texts.spawn(
            "THE FLOOR CRACKS!".to_string(),
            boss_pos - Vec2::new(120.0, 92.0),
            34.0,
            [1.0, 0.5, 0.15, 1.0],
        );
    }

    /// Tide Boss enrage set-piece: the arena FLOODS. The boss surges the water level, appending a
    /// handful of extra wade-drag pools to the level's own `tide_pools` so the whole space suddenly
    /// routes differently — the safe lanes you'd learned are underwater now. We remember how many we
    /// added (`boss_flood_pools`) so catching the boss can recede exactly the flood water without
    /// disturbing the biome's native pools. Flood pools avoid the pen and the boss's own position.
    pub(crate) fn flood_arena(&mut self, boss_pos: Vec2) {
        let mut rng = crate::rng::rng();
        let count = 4;
        let mut placed = 0;
        let mut attempts = 0;
        while placed < count && attempts < 60 {
            attempts += 1;
            let radius = rng.random_range(80.0..130.0);
            let margin = radius + 30.0;
            // Flood wells up around the boss/fight, clamped to world bounds — see spawn_boss_fissures.
            let ang = rng.random_range(0.0..std::f32::consts::TAU);
            let dist = rng.random_range(0.0..self.height * 0.5);
            let c = Vec2::new(
                (boss_pos.x + ang.cos() * dist).clamp(margin, self.world_width - margin),
                (boss_pos.y + ang.sin() * dist).clamp(margin, self.world_height - margin),
            );
            if c.distance(self.pen_pos) < radius + PEN_RADIUS + 40.0 {
                continue;
            }
            if c.distance(boss_pos) < radius + 80.0 {
                continue;
            }
            if self
                .tide_pools
                .iter()
                .any(|(pc, pr)| c.distance(*pc) < radius + pr + 40.0)
            {
                continue;
            }
            self.tide_pools.push((c, radius));
            self.boss_flood_pools += 1;
            placed += 1;
            // A cold burst of splash where each new pool wells up.
            self.spawn_catch_shockwave(c, [0.3, 0.7, 1.0]);
        }
        self.floating_texts.spawn(
            "THE ARENA FLOODS!".to_string(),
            boss_pos - Vec2::new(120.0, 92.0),
            34.0,
            [0.4, 0.85, 1.0, 1.0],
        );
    }

    /// While a King Crab's enrage fissures are open, the conga tail is at risk if it's dragged
    /// through one — the cracked floor bites off the last few links, the same self-limiting way the
    /// panic snap and kelp snag do (only long trains, only the tail, gated by the shared cooldown).
    /// This is the teeth behind the arena-crack set-piece: the fissures aren't decoration, they make
    /// routing the train the thing you sweat over in the boss's final phase.
    pub(crate) fn damage_tail_in_fissures(&mut self, dt: f32) {
        const MIN_TRAIN_TO_SNAP: usize = 5;
        const SNAP_LINKS: usize = 2;
        const SNAP_COOLDOWN: f32 = 1.8;
        const SNAP_CHANCE_PER_SEC: f32 = 0.8;

        if self.boss_fissures.is_empty()
            || self.chain_snap_cooldown > 0.0
            || self.chain_count < MIN_TRAIN_TO_SNAP
        {
            return;
        }
        // Reuses the tail position update_crabs already computed this frame instead of rescanning.
        let Some(tail_pos) = self.cached_tail_pos else {
            return;
        };
        // The geyser makes the hazard breathe with the beat: while a fissure is erupting its bite
        // reach swells past the rim (so a tail merely skirting the edge gets caught mid-spout) and
        // the snap becomes far likelier. Between beats the reach recedes to the rim and the bite
        // goes nearly dormant — so the safe move is to thread the tail across in the gaps, not on
        // the hit. `erupt` is the shared beat pulse; its peak is right on the beat.
        let erupt = self.boss_fissure_erupt.clamp(0.0, 1.0);
        let reach = 1.0 + 0.35 * erupt; // danger radius grows up to 1.35x on the beat
        // Only bite if the tail is inside a (possibly geyser-widened) open fissure — weave and you're safe.
        let in_fissure = self.boss_fissures.iter().any(|(c, r, age)| {
            *age > 0.6 && tail_pos.distance_squared(*c) < (*r * reach) * (*r * reach)
        });
        if !in_fissure {
            return;
        }
        // Between beats the pit is nearly dormant (a small baseline bite), on the beat it snaps
        // hard — so the eruption is what the player actually dodges.
        let snap_chance = SNAP_CHANCE_PER_SEC * (0.15 + 0.85 * erupt);
        if crate::rng::rng().random::<f32>() > snap_chance * dt {
            return;
        }

        let keep = self.chain_count.saturating_sub(SNAP_LINKS).max(1);
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
        self.chain_snap_cooldown = SNAP_COOLDOWN;

        for pos in &snapped_positions {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((*pos, 0.0));
            }
            self.floating_texts.spawn(
                "!".to_string(),
                *pos - Vec2::new(0.0, 24.0),
                24.0,
                [1.0, 0.55, 0.2, 1.0],
            );
        }
        self.floating_texts.spawn(
            format!("SWALLOWED!  -{}", snapped),
            tail_pos - Vec2::new(40.0, 32.0),
            30.0,
            [1.0, 0.5, 0.15, 1.0],
        );
        self.spawn_catch_shockwave(tail_pos, [1.0, 0.5, 0.15]);
        self.screen_shake = self.screen_shake.max(6.0);
    }

    /// A Tide Boss pulse detonates at `center`: an expanding shockwave ring that shoves every
    /// nearby *free* crab outward into a panic, and — if the conga train's tail is caught inside the
    /// blast — knocks the last few links loose (the Tide Boss's version of a chain snap). The threat
    /// is spacing: keep your train out of the ring and the pulse does nothing, so it rewards reading
    /// the swell telegraph and pulling back rather than routing out of a charge lane.
    pub(crate) fn tide_pulse_burst(&mut self, center: Vec2) {
        const TIDE_SNAP_LINKS: usize = 4; // a solid surge tears off a bit more than a panic-brush snap
        // Archetype-in-boss crossover: a Magnet ANCHORS against the surge. A free Magnet caught in the
        // blast isn't flung out like everything else — the wall of water charges its lodestone (the same
        // supercharge a snared Golden buys it), and its widened vacuum re-balls the herd the pulse just
        // scattered next frame. The payoff is defensive too: if that supercharged field covers your
        // conga tail, it pins those links against the shove and the chain-snap is called off. So parking
        // a Magnet by your train turns the Tide Boss's own crowd-scatter into a re-gather and a shield —
        // the Magnet (routing) archetype finally matters inside the water fight.
        const MAGNET_ANCHOR_RADIUS: f32 = 240.0; // matches the Magnet's normal pull reach
        const MAGNET_ANCHOR_RADIUS_SQ: f32 = MAGNET_ANCHOR_RADIUS * MAGNET_ANCHOR_RADIUS;
        let r2 = TIDE_PULSE_RADIUS * TIDE_PULSE_RADIUS;

        // Spawn the visible expanding ring (bounded so a stall can't grow the Vec without limit).
        if self.tide_pulses.len() < 8 {
            self.tide_pulses.push((center, crate::CRAB_SIZE));
        }

        // OFFENSIVE archetype-in-boss crossover — the GOLDEN SLINGSHOT. The Tide Boss is otherwise
        // fought in a bubble; this is the player's active play *against* it, the mirror of the King
        // Crab's bait-into-Armored stun and the Reef DJ's hype-Dancer chip. Setup: lure a fleeing
        // Golden into a free Magnet's field (the existing snare→supercharge crossover) and park that
        // loaded Magnet where the boss's swell will wash over it. When the surge hits, instead of
        // scattering the Magnet's catch, the wall of water FIRES the pinned Golden's shine straight
        // through the lodestone and into the boss — a bright lance that cracks a big chunk off the
        // shell far faster than the beam ever could. It's a real reason to spend the whole telegraph
        // wrangling a Golden into position rather than just backing the train out of the ring, and
        // it's a legible, watchable moment (gold streak → boss stagger) for the videos Carl wants.
        const SLINGSHOT_CHIP: f32 = 0.7; // ~a bar of beam per shot — a deliberate setup deserves a real dent
        // Reused scratch buffers instead of fresh Vec::new() per pulse — see field docs.
        let mut slingshots = std::mem::take(&mut self.pulse_slingshots_buf);
        slingshots.clear();
        {
            // A Magnet is "loaded" if it's charged (pinning shine) and a snared Golden sits inside its
            // reach — the same pairing the charged-magnet pass already recognizes elsewhere. Collect
            // loaded pairs the surge washes over, sparing them from the scatter below (they fire, not flee).
            let mut loaded_magnets = std::mem::take(&mut self.pulse_loaded_magnets_buf);
            loaded_magnets.clear();
            for m in &self.crabs {
                if m.caught || m.is_boss() || !m.is_magnet() || m.magnet_charged <= 0.0 {
                    continue;
                }
                if m.pos.distance_squared(center) > r2 {
                    continue; // only Magnets the swell actually reaches can be fired by it
                }
                // Find a snared Golden this Magnet is holding (nearest inside its pull reach).
                let mut fired_golden: Option<Vec2> = None;
                for g in &self.crabs {
                    if g.caught || !g.is_golden() || g.magnet_snared <= 0.0 {
                        continue;
                    }
                    if g.pos.distance_squared(m.pos) <= MAGNET_ANCHOR_RADIUS_SQ {
                        fired_golden = Some(g.pos);
                        break;
                    }
                }
                if let Some(gpos) = fired_golden {
                    loaded_magnets.push(m.pos);
                    slingshots.push((m.pos, gpos));
                }
            }
            // Chip the live Tide Boss once per shot, and consume the Golden the surge spent (it's
            // flung out of the snare into a flee — the shot expends the prize, so the play is a
            // trade: give up the Golden catch for a big crack in the shell).
            if !slingshots.is_empty() {
                let mut broke_at: Option<Vec2> = None;
                let mut boss_pos: Option<Vec2> = None;
                for crab in &mut self.crabs {
                    if crab.is_tide_boss() && !crab.caught && crab.boss_health > 0.0 {
                        boss_pos = Some(crab.pos);
                        crab.boss_health =
                            (crab.boss_health - SLINGSHOT_CHIP * slingshots.len() as f32).max(0.0);
                        if crab.boss_health <= 0.0 {
                            broke_at = Some(crab.pos);
                        }
                        break;
                    }
                }
                // A bright gold lance streaks from each fired Golden into the boss — the reused
                // catch-trail plumbing (from → to, retracting, self-expiring) gives it the watchable
                // "shot connects" beat for free. Only fires when a live boss actually took the hit.
                if let Some(bpos) = boss_pos {
                    for &(_, gpos) in &slingshots {
                        if self.catch_trails.len() < 48 {
                            self.catch_trails
                                .push((gpos, bpos, -0.25, [1.0, 0.85, 0.25]));
                        }
                    }
                }
                // Spend each fired Golden — the shot expends the prize (the whole point of the trade).
                // Release the snare AND set slingshot_spent so the Magnet field can't re-snare it next
                // frame (see the Golden re-snare pass), and fling it outward from the boss under its own
                // velocity so it visibly leaves the field rather than reloading in place. Without the
                // spent-window the anchor/re-snare passes would keep it loaded and the chip would repeat
                // every pulse from one setup — turning a deliberate one-shot into a beam-free boss kill.
                for &(_, gpos) in &slingshots {
                    for crab in &mut self.crabs {
                        if crab.is_golden()
                            && !crab.caught
                            && crab.magnet_snared > 0.0
                            && crab.pos == gpos
                        {
                            crab.magnet_snared = 0.0;
                            crab.slingshot_spent = 1.2; // ~a couple beats of no-reload while it clears the field
                            crab.fleeing = true;
                            crab.startle_timer = crab.startle_timer.max(0.5);
                            let away = (crab.pos - center).normalize_or_zero();
                            let away = if away == Vec2::ZERO {
                                Vec2::new(0.0, 1.0)
                            } else {
                                away
                            };
                            crab.vel = away * crab.crab_type.speed_range().end * 2.0;
                            crab.speed = 1.0;
                            break;
                        }
                    }
                }
                for &(mpos, _) in &slingshots {
                    self.floating_texts.spawn(
                        "SLINGSHOT!".to_string(),
                        mpos - Vec2::new(55.0, 40.0),
                        30.0,
                        [1.0, 0.85, 0.3, 1.0],
                    );
                    self.particle_system
                        .spawn_milestone_fireworks(mpos, 10, &mut crate::rng::rng());
                }
                self.screen_shake = self.screen_shake.max(10.0);
                self.on_beat_flash = self.on_beat_flash.max(0.35);
                if let Some(bpos) = broke_at {
                    self.floating_texts.spawn(
                        "WASHED DOWN — CATCH IT!".to_string(),
                        bpos - Vec2::new(120.0, 46.0),
                        34.0,
                        [0.4, 1.0, 0.5, 1.0],
                    );
                    self.spawn_catch_shockwave(bpos, [0.3, 0.75, 1.0]);
                    self.screen_shake = self.screen_shake.max(14.0);
                }
            }
            self.pulse_loaded_magnets_buf = loaded_magnets;
        }

        // First pass: supercharge every free Magnet the surge washes over, and remember where each
        // anchoring field sits so the shove and the snap below can spare crabs inside it.
        let mut anchor_positions = std::mem::take(&mut self.pulse_anchor_positions_buf);
        anchor_positions.clear();
        for crab in &mut self.crabs {
            if crab.caught || crab.is_boss() || !crab.is_magnet() {
                continue;
            }
            if crab.pos.distance_squared(center) > r2 {
                continue;
            }
            // The wall of water charges the lodestone — same state a snared Golden grants, so the
            // existing charged-radius vacuum pass re-gathers the scattered herd and the aura flares gold.
            crab.magnet_charged = crab.magnet_charged.max(1.6);
            if anchor_positions.len() < 8 {
                anchor_positions.push(crab.pos);
            }
        }
        let anchored = |pos: Vec2| {
            anchor_positions
                .iter()
                .any(|a| a.distance_squared(pos) <= MAGNET_ANCHOR_RADIUS_SQ)
        };

        // Shove every free crab in range outward and startle it into a flee — unless a Magnet's
        // charged field holds it in place.
        let mut scattered = std::mem::take(&mut self.pulse_scattered_buf);
        scattered.clear();
        for crab in &mut self.crabs {
            if crab.caught || crab.is_boss() {
                continue;
            }
            let d2 = crab.pos.distance_squared(center);
            if d2 > r2 {
                continue;
            }
            if !crab.is_magnet() && anchored(crab.pos) {
                continue; // pinned by a nearby anchoring Magnet — the vacuum holds it against the surge
            }
            let outward = (crab.pos - center).normalize_or_zero();
            let outward = if outward == Vec2::ZERO {
                Vec2::new(0.0, 1.0)
            } else {
                outward
            };
            crab.fleeing = true;
            crab.startle_timer = crab.startle_timer.max(0.7);
            crab.charm_timer = 0.0; // the surge overwhelms a whistle's calm
            crab.vel = outward * crab.crab_type.speed_range().end * 2.0;
            crab.speed = 1.0; // vel encodes full speed, matching the flee/startle convention
            if scattered.len() < 24 {
                scattered.push(crab.pos);
            }
        }

        // A Magnet field over the tail calls off the wash-out entirely — feedback for the save.
        let tail_anchored = !anchor_positions.is_empty()
            && self.crabs.iter().any(|c| {
                c.caught
                    && c.chain_index.is_some()
                    && c.pos.distance_squared(center) <= r2
                    && anchored(c.pos)
            });
        if tail_anchored {
            self.floating_texts.spawn(
                "ANCHORED!".to_string(),
                center - Vec2::new(50.0, 34.0),
                30.0,
                [0.95, 0.55, 0.2, 1.0],
            );
        }
        // Release the borrow on anchor_positions so we can move it back at the end.
        drop(anchored);

        // Knock the tail loose if any caught link sits inside the blast. Mirrors snap_chain_on_panic
        // but triggered by the pulse's reach rather than a physical tail collision. A Magnet anchoring
        // the tail (tail_anchored) pins the links and cancels the snap.
        let tail_in_blast = !tail_anchored
            && self.crabs.iter().any(|c| {
                c.caught && c.chain_index.is_some() && c.pos.distance_squared(center) <= r2
            });
        if tail_in_blast && self.chain_count >= 5 && self.chain_snap_cooldown <= 0.0 {
            let keep = self.chain_count.saturating_sub(TIDE_SNAP_LINKS).max(1);
            let snapped = self.chain_count - keep;
            let mut snapped_positions = std::mem::take(&mut self.pulse_snapped_positions_buf);
            snapped_positions.clear();
            for crab in &mut self.crabs {
                let Some(ci) = crab.chain_index else { continue };
                if ci >= keep {
                    crab.caught = false;
                    crab.chain_index = None;
                    crab.fleeing = true;
                    crab.startle_timer = 0.6;
                    let outward = (crab.pos - center).normalize_or_zero();
                    let outward = if outward == Vec2::ZERO {
                        Vec2::new(0.0, 1.0)
                    } else {
                        outward
                    };
                    crab.vel = outward * crab.crab_type.speed_range().end * 2.2;
                    crab.speed = 1.0;
                    snapped_positions.push(crab.pos);
                }
            }
            self.chain_count = keep;
            self.recompute_tail_run(); // the tail changed — rebuild the same-type run
            self.chain_snap_cooldown = 1.6;
            for pos in &snapped_positions {
                if self.fear_rings.len() < 32 {
                    self.fear_rings.push((*pos, 0.0));
                }
            }
            self.floating_texts.spawn(
                format!("WASHED OUT!  -{}", snapped),
                center - Vec2::new(60.0, 34.0),
                32.0,
                [0.5, 0.85, 1.0, 1.0],
            );
            self.pulse_snapped_positions_buf = snapped_positions;
        }

        // Feedback for the scattered herd.
        for pos in &scattered {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((*pos, 0.0));
            }
        }
        // Hand the scratch buffers back to self so next pulse reuses their capacity.
        self.pulse_slingshots_buf = slingshots;
        self.pulse_anchor_positions_buf = anchor_positions;
        self.pulse_scattered_buf = scattered;
        self.spawn_catch_shockwave(center, [0.3, 0.75, 1.0]);
        self.screen_shake = self.screen_shake.max(16.0);
        let a = crate::rng::rng().random_range(0.0_f32..std::f32::consts::TAU);
        self.screen_shake_vel = Vec2::new(a.cos(), a.sin()) * 12.0 * 60.0;
        self.on_beat_flash = self.on_beat_flash.max(0.35);
    }
}
