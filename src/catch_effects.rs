//! Ordinary catch reward payoffs: the Golden Crab treasure bonus, the Splitter cleave
//! (bank-half-the-train risk/reward), and the Magnet+Golden shine cascade. Extracted out of
//! main.rs's impl MainState — same methods, same behaviour, just grouped by subsystem. The
//! King Crab/Tide Boss caught celebration and the boss-fight arena hazards live in the
//! sibling `boss_catch` module.

use ggez::glam::Vec2;

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
}
