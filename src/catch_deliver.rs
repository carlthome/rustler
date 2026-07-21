//! The core catch-and-deliver loop for `MainState`: banking a completed train at the
//! pen (`try_deliver_train`) and the per-frame proximity/beat catch of free crabs into
//! the train (`handle_crab_catching`).
//!
//! Extracted verbatim from `main.rs` as `impl MainState` methods to keep that file
//! navigable. Pure structural move — no behaviour change.

use ggez::Context;
use ggez::audio::SoundSource;
use ggez::glam::Vec2;
use rand::Rng;

use crate::*;

impl MainState {
    pub(crate) fn try_deliver_train(&mut self, ctx: &mut Context) {
        if self.chain_count == 0 {
            return;
        }
        let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        if player_center.distance(self.pen_pos) > PEN_RADIUS {
            return;
        }

        // How many crabs are actually banking (defensive count in case any wild state drifted).
        let delivered = self
            .crabs
            .iter()
            .filter(|c| c.caught)
            .count()
            .max(self.chain_count);
        if delivered == 0 {
            return;
        }

        // Super-linear base payout: triangular sum so crab #n adds n points, times a flat handler.
        let n = delivered;
        // Arrangement bonus: every same-type adjacent pair still intact at bank pays a flat kicker.
        // This is the reward for HOLDING an ordering to the pen (distinct from the catch-time MATCH
        // run). Folded into `base` BEFORE the multipliers so it rides the streak/perfect/gamble
        // stack exactly like the triangular sum, and so the pen_worth preview (which recomputes the
        // same base+bonds) can't disagree with what actually banks.
        let (bonds, sandwiches, run_bonus, centerpiece) = self.count_bonds_and_sandwiches(n);
        let base = (n * (n + 1) / 2) * 3
            + bonds * BOND_PAIR_BONUS
            + sandwiches * SANDWICH_BONUS
            + run_bonus
            + centerpiece;

        // A bank in quick succession bumps the delivery streak (capped) and refreshes its grace
        // window; the streak multiplier escalates the payout so cashing in repeatedly at tempo pays
        // off, not just hoarding one giant train.
        self.deliver_streak = (self.deliver_streak + 1).min(DELIVER_STREAK_MAX);
        self.deliver_streak_timer = DELIVER_STREAK_GRACE;
        // Streak 1 = 1.0x, then +0.25x per bank: 1.25x, 1.5x, ... up to 2.75x at the cap.
        let streak_mult = 1.0 + (self.deliver_streak.saturating_sub(1) as f32) * 0.25;

        // Banking on the beat lands a PERFECT DELIVERY: a flat percentage bonus on top of the streak.
        let perfect = self.on_beat_now();
        let perfect_mult = if perfect {
            1.0 + PERFECT_DELIVERY_BONUS
        } else {
            1.0
        };

        // The Groove Gamble multiplier rides through to the bank too — a hot on-beat streak makes
        // the delivery jackpot pay out even bigger, so it's worth protecting the heat right up to
        // the pen instead of grabbing sloppily on the way in.
        let bank =
            (base as f32 * streak_mult * perfect_mult * self.beat_gamble_mult).round() as usize;
        self.score += bank;
        // Raw crab-count tally for the campaign win conditions (BankCrabs) — score is multiplied
        // points, so the goal needs its own honest headcount of what actually filed into the pen.
        self.banked_crabs_run += delivered;
        // Attribute the rhythm-driven extra of this bank: the delivery streak is a pace reward that
        // survives without the beat, so the baseline keeps it — but the PERFECT (on-beat) delivery
        // bonus and the Groove Gamble multiplier are pure rhythm, so strip only those for the flat
        // reference. The difference is the mastery the beat bought at the pen, added to the tally.
        let flat_bank = (base as f32 * streak_mult).round() as usize;
        let jump = bank.saturating_sub(flat_bank);
        if jump > 0 {
            self.rhythm_bonus_score += jump;
            self.rhythm_bonus_flash = 1.0;
        }

        // Tutorial pass tracking: count real train deliveries for the chain-and-deliver learn-
        // session. This is the one write behind that tutorial's pure pass predicate
        // (`Tutorial::passed` for ChainDeliver), so a headless run of the scenario reaches the same
        // boolean without any rendering.
        if let Some(t) = self.tutorial.as_mut() {
            if t.kind == TutorialKind::ChainDeliver {
                t.deliveries += 1;
            }
        }

        // Before the delivered crabs leave the field, snapshot them (in chain order, head first)
        // so they can visibly march into the pen instead of blinking out — the parade is purely
        // cosmetic; the score above is already banked.
        let mut delivered_crabs: Vec<&EnemyCrab> = self.crabs.iter().filter(|c| c.caught).collect();
        // File them in in chain order (head of the train first) so the parade rolls down the line.
        delivered_crabs.sort_by_key(|c| c.chain_index.unwrap_or(usize::MAX));
        let marching: Vec<(Vec2, [f32; 3], f32)> = delivered_crabs
            .iter()
            .map(|c| (c.pos, c.crab_color(), c.scale))
            .collect();
        self.penned_marchers.spawn_train(self.pen_pos, &marching);

        // The delivered crabs leave the field for good — they've been penned.
        self.crabs.retain(|c| !c.caught);
        self.chain_count = 0;
        self.tail_run_len = 0; // whole train banked — the match run at the tail is gone
        self.next_milestone = 5;

        // Big celebratory feedback so banking feels like a real payoff, not just a number ticking.
        let mut rng = crate::rng::rng();
        self.particle_system
            .spawn_milestone_fireworks(self.pen_pos, n, &mut rng);
        // A perfect on-beat bank gets a gold rhythm ring; a plain bank stays green.
        self.spawn_catch_shockwave(
            self.pen_pos,
            if perfect {
                [1.0, 0.85, 0.3]
            } else {
                [0.5, 1.0, 0.5]
            },
        );
        // A hot streak throws a second, larger firework burst so the escalation reads on screen.
        if self.deliver_streak >= 3 {
            self.particle_system.spawn_milestone_fireworks(
                self.pen_pos,
                n + self.deliver_streak as usize * 4,
                &mut rng,
            );
        }
        self.floating_texts.spawn(
            format!("BANKED +{}", bank),
            self.pen_pos - Vec2::new(60.0, 40.0),
            48.0,
            [0.4, 1.0, 0.5, 1.0],
        );
        // Perfect-on-beat and streak callouts stack above the bank number so the player sees *why*
        // this bank paid more.
        let mut callout_y = 4.0;
        if perfect {
            self.floating_texts.spawn(
                "PERFECT DELIVERY!".to_string(),
                self.pen_pos - Vec2::new(95.0, callout_y),
                30.0,
                [1.0, 0.9, 0.35, 1.0],
            );
            callout_y += 30.0;
        }
        if self.deliver_streak >= 2 {
            self.floating_texts.spawn(
                format!("x{} STREAK  ({:.2}x)", self.deliver_streak, streak_mult),
                self.pen_pos - Vec2::new(85.0, callout_y),
                26.0,
                [1.0, 0.55, 0.9, 1.0],
            );
            callout_y += 26.0;
        }
        // ARRANGED — the arrangement bonus made legible. Every same-type adjacent pair held intact
        // to the pen (each a glowing rope segment on the way in) paid BOND_PAIR_BONUS; naming it
        // here tells the player their *ordering*, not just their length, earned this — the payoff
        // face of making the middle of the train matter. Cyan so it reads distinct from the gold
        // perfect / pink streak callouts.
        if bonds > 0 {
            self.floating_texts.spawn(
                format!("ARRANGED x{}  (+{})", bonds, bonds * BOND_PAIR_BONUS),
                self.pen_pos - Vec2::new(90.0, callout_y),
                26.0,
                [0.4, 0.95, 1.0, 1.0],
            );
            callout_y += 26.0;
        }
        // SANDWICH — the mid-train figurehead-flanking bonus made legible. Warm gold so it reads as
        // kin to the Golden figurehead economy while staying distinct from the cyan ARRANGED tag.
        if sandwiches > 0 {
            self.floating_texts.spawn(
                format!(
                    "SANDWICH x{}  (+{})",
                    sandwiches,
                    sandwiches * SANDWICH_BONUS
                ),
                self.pen_pos - Vec2::new(90.0, callout_y),
                26.0,
                [1.0, 0.8, 0.35, 1.0],
            );
            callout_y += 26.0;
        }
        // BLOCK — the deep-run escalator made legible. A same-type run of 3+ held to the pen paid
        // run_bonus on top of its adjacency bonds; naming it tells the player that stacking a LONG
        // matched block (not just scattered pairs) is what earned this. Vivid green so it reads as a
        // third, distinct arrangement tier next to cyan ARRANGED and gold SANDWICH.
        if run_bonus > 0 {
            self.floating_texts.spawn(
                format!("BLOCK!  (+{})", run_bonus),
                self.pen_pos - Vec2::new(80.0, callout_y),
                26.0,
                [0.5, 1.0, 0.5, 1.0],
            );
            callout_y += 26.0;
        }
        // CENTERPIECE — positional identity for the MIDDLE of the train. A deep run seated across
        // the train's midpoint (safe from tail snaps) earned this; naming it tells the player that
        // WHERE they parked their best block, not just that they built one, is what paid. Bright
        // magenta so it reads as the top arrangement tier above cyan ARRANGED / gold SANDWICH / green BLOCK.
        if centerpiece > 0 {
            self.floating_texts.spawn(
                format!("CENTERPIECE!  (+{})", centerpiece),
                self.pen_pos - Vec2::new(105.0, callout_y),
                28.0,
                [1.0, 0.45, 0.95, 1.0],
            );
            callout_y += 28.0;
        }
        // LONG HAUL — the payoff face of the AT RISK gamble. It fires at the SAME length tiers the
        // risk escalates at (the panic_snap_links steps: 8, 12, 16), so a train that was flashing
        // AT RISK on the way in now cashes out as a named reward. This adds NO new multiplier — the
        // bank is already superlinear via the triangular base curve. Instead it *names* how much of
        // that base the priciest tail links (everything past the tier threshold) actually earned,
        // so the upside of holding long reads as loudly on screen as the downside did. The number
        // shown is the marginal triangular value of links past `thresh`: base(n) - base(thresh).
        let long_haul_tier = match n {
            16.. => Some(("GRAND HAUL!", 16usize, [1.0, 0.55, 0.2, 1.0])),
            12..=15 => Some(("LONG HAUL!", 12, [1.0, 0.75, 0.25, 1.0])),
            8..=11 => Some(("BIG HAUL!", 8, [1.0, 0.9, 0.4, 1.0])),
            _ => None,
        };
        if let Some((label, thresh, color)) = long_haul_tier {
            // Marginal points the tail links past the tier threshold contributed to the base payout,
            // carried through the same multipliers the whole bank got — real earned score attributed
            // to the length you refused to bank, not a bolt-on bonus.
            let tail_base = (n * (n + 1) / 2).saturating_sub(thresh * (thresh + 1) / 2) * 3;
            let tail_bank = (tail_base as f32 * streak_mult * perfect_mult * self.beat_gamble_mult)
                .round() as usize;
            self.floating_texts.spawn(
                format!("{}  +{} FROM THE TAIL", label, tail_bank),
                self.pen_pos - Vec2::new(120.0, callout_y),
                30.0,
                color,
            );
            callout_y += 30.0;
            // A held-long bank earns extra celebration so the risk you carried pays off viscerally.
            self.particle_system.spawn_milestone_fireworks(
                self.pen_pos,
                n + (n - thresh) * 3,
                &mut rng,
            );
            self.screen_shake = self.screen_shake.max(24.0);
        }
        self.floating_texts.spawn(
            format!("{} crabs delivered!", n),
            self.pen_pos - Vec2::new(70.0, callout_y),
            26.0,
            [1.0, 0.95, 0.6, 1.0],
        );
        self.deliver_flash = 1.0;
        // Anchor the delivery beam at the player (train head) as it stood this bank, before the pen
        // relocates below — the beam is drawn to the OLD pen this frame's flash decays over.
        self.deliver_beam_from = player_center;
        self.deliver_beam_to = self.pen_pos;
        self.deliver_beam_perfect = perfect;
        // A perfect / hot-streak bank hits harder: more zoom, more shake, a fuller groove kick.
        let intensity = streak_mult * perfect_mult;
        self.zoom_punch = self.zoom_punch.max(0.11 * intensity);
        self.screen_shake = self.screen_shake.max(18.0 * intensity);
        let kick_angle = rng.random_range(0.0_f32..std::f32::consts::TAU);
        self.screen_shake_vel =
            Vec2::new(kick_angle.cos(), kick_angle.sin()) * 18.0 * intensity * 60.0;
        self.on_beat_flash = if perfect { 0.85 } else { 0.6 };
        self.groove = (self.groove + if perfect { 0.5 } else { 0.35 }).min(1.0);
        let _ = self.sounds.success2.play();

        // Move the pen so the next bank is a fresh routing decision, not a treadmill loop.
        self.pen_pos = pick_pen_pos(self.world_width, self.world_height, player_center, &mut rng);

        // Banking is the single biggest score jump in the game, so it's the most likely place to
        // cross an upgrade threshold — check HERE, at the pen, so the upgrade screen lands on the
        // natural pause right after a delivery (the moment the player earned it). Previously the
        // check ran only from the three catch sites, so a threshold crossed by a big bank sat
        // silent until some unrelated mid-field catch popped the screen out of nowhere — the
        // "fires at an odd moment" bug Carl hit in playtest. A bank is a lull, not mid-action, so
        // it's exactly when a menu is least disruptive.
        self.check_upgrade_unlock(ctx);
    }

    // check_upgrade_unlock and roll_upgrade_offer now live in src/upgrade.rs (impl MainState there).

    pub(crate) fn handle_crab_catching(&mut self, ctx: &mut Context) {
        let mult = self.combo_multiplier();
        let mut any_caught = false;
        // Reused scratch buffers instead of fresh Vec::new() every frame — this function runs
        // unconditionally every tick and the overwhelming majority of frames catch zero crabs,
        // so allocating three empty Vecs per call was pure per-frame churn on the hottest path.
        let mut startle_origins = std::mem::take(&mut self.startle_origins_buf);
        startle_origins.clear();
        let mut boss_catches = std::mem::take(&mut self.boss_catches_buf);
        boss_catches.clear();
        // Dancer King caught this frame: (position, whether the grab landed on the beat).
        // Resolved after the loop — an on-beat royal catch banks its whole entranced court.
        let mut dancer_king_catch: Option<(Vec2, bool)> = None;
        // Dancers snapped up while still answering a Call — paid out after the loop (needs &mut self).
        let mut dance_catches = std::mem::take(&mut self.dance_catches_buf);
        dance_catches.clear();
        // Golden crabs snapped up this frame — the big lump-sum bonus is paid out after the loop.
        let mut golden_catches = std::mem::take(&mut self.golden_catches_buf);
        golden_catches.clear();
        // Goldens caught directly behind a Magnet link this frame — the "shine conducts down the
        // train" cascade, paid out after the loop. See the adjacency check inside the loop below.
        let mut magnet_shine_catches = std::mem::take(&mut self.magnet_shine_catches_buf);
        magnet_shine_catches.clear();
        // Same-type "match run" events this frame — a catch that extends a run of matching-archetype
        // links at the tail. Paid out (escalating bonus + callout) after the loop.
        let mut match_run_catches = std::mem::take(&mut self.match_run_catches_buf);
        match_run_catches.clear();
        // Splitter crabs snapped up this frame — each one cleaves the train at the midpoint and banks
        // the back half. Deferred to after the loop (the cleave/bank borrows &mut self and mutates
        // chain_index across all crabs, which we can't do mid-loop holding a &mut into self.crabs).
        // At most one split per frame matters (they stack chaotically otherwise), so we just record
        // whether a Splitter landed and where.
        let mut splitter_catch: Option<Vec2> = None;
        // Type of the crab that currently sits at the *tail* of the train (highest chain_index),
        // snapshotted before the catch loop so we can tell what a newly-caught crab links onto. As
        // each catch lands the new crab becomes the tail, so we roll this forward per catch instead
        // of re-scanning self.crabs mid-loop (which we can't, holding a &mut into it). None if the
        // train is empty. This is what makes catch *order* a live decision: whether a Magnet is the
        // link directly ahead of a just-caught Golden depends on the sequence the player caught in.
        // Single O(n) snapshot pass over the caught-crab list for three per-frame reads that
        // used to be three separate scans:
        //   • prev_tail_type  — the type at the current tail (highest chain_index, == chain_count-1)
        //   • head_is_golden  — whether chain_index 0 is a Golden (figurehead bonus)
        //   • head_is_dancer  — whether chain_index 0 is a Dancer (Drum-Major bonus)
        // chain_index 0 can't be the tail at the same time (only true when chain_count == 1, in
        // which case prev_tail_type and both head flags all still get set correctly in one pass).
        let tail_ci = self.chain_count.checked_sub(1);
        let mut prev_tail_type: Option<CrabType> = None;
        let mut prev_tail_pos: Vec2 = Vec2::ZERO;
        let mut head_is_golden = false;
        let mut head_is_dancer = false;
        for c in &self.crabs {
            match c.chain_index {
                Some(0) => {
                    // Head of the train.
                    // Golden Figurehead — the head-position mirror of the Armored tail-guard. A
                    // Golden crab riding at the head (chain_index 0) acts as a gilded figurehead:
                    // every same-type match run pays a bigger bonus while it leads. This gives the
                    // *front* of the train real positional value — until now only the tail paid.
                    head_is_golden = c.is_golden();
                    // Dancer Drum-Major — the rhythm-economy sibling of the Golden figurehead,
                    // competing for the same coveted head slot. On-beat catches fill the groove
                    // meter faster and bump the Groove Gamble harder while it leads.
                    head_is_dancer = c.is_dancer();
                    // Could also be the tail if chain_count == 1.
                    if tail_ci == Some(0) {
                        prev_tail_type = Some(c.crab_type);
                        prev_tail_pos = c.pos;
                    }
                }
                Some(ci) if Some(ci) == tail_ci => {
                    prev_tail_type = Some(c.crab_type);
                    prev_tail_pos = c.pos;
                }
                _ => {}
            }
        }
        // Reef DJ backup dancers caught this frame on a *called (hot) beat* — each one chips the
        // boss shell. Collected here and applied after the loop so we don't need a second &mut
        // borrow of self.crabs mid-loop. `reef_hot_now` is the same window the DJ's own shell uses.
        let reef_hot_now = (self.beat_timer < BEAT_WINDOW
            || self.beat_timer > self.beat_interval - BEAT_WINDOW)
            && self.reef_phrase[(self.beat_count % 4) as usize];
        let mut hype_dancer_hits = std::mem::take(&mut self.hype_dancer_hits_buf);
        hype_dancer_hits.clear();
        for crab in &mut self.crabs {
            if crab.is_catchable()
                && (self.player_pos.x - crab.pos.x).abs() < PLAYER_SIZE * 0.6 + crab.scale * CRAB_SIZE
                && (self.player_pos.y - crab.pos.y).abs() < PLAYER_SIZE * 0.6 + crab.scale * CRAB_SIZE
            {
                if crab.is_boss() {
                    boss_catches.push((crab.pos, crab.crab_type));
                    if crab.crab_type == CrabType::DancerKing {
                        let on_beat_now = self.beat_timer < BEAT_WINDOW
                            || self.beat_timer > self.beat_interval - BEAT_WINDOW;
                        dancer_king_catch = Some((crab.pos, on_beat_now));
                    }
                }
                // Get crab color before marking as caught
                let crab_color = crab.crab_color();

                // Spawn particle effect
                let mut rng = crate::rng::rng();
                self.particle_system.spawn_catch_effect(
                    crab.pos,
                    crab_color,
                    crab.crab_type,
                    &mut rng,
                );
                let shock_pos = crab.pos;

                if crab.answering_call > 0.0 {
                    dance_catches.push(crab.pos);
                }
                // Reef DJ backup dancer snapped up on a called (hot) beat: queue a shell chip. This
                // is the archetype's job inside the boss fight — a Dancer caught in time with the
                // DJ's phrase helps crack it, so herding its own hype crew onto the beat pays off.
                if self.reef_active && reef_hot_now && crab.is_dancer() {
                    hype_dancer_hits.push(crab.pos);
                }
                crab.caught = true;
                self.chain_join_ripple = true;
                if self.catch_shockwaves.len() < 48 {
                    self.catch_shockwaves.push((shock_pos, 0.0, crab_color));
                }
                startle_origins.push(shock_pos);
                any_caught = true;
                crab.chain_index = Some(self.chain_count);
                // Bond-forming flash: if this catch links a same-type neighbor, emit a brief
                // connecting arc so the player sees the bond click into place (legibility of the
                // arrangement system — makes the chain structure readable in motion).
                if prev_tail_type == Some(crab.crab_type) && self.chain_count > 0 {
                    if self.bond_flash_events.len() < 24 {
                        self.bond_flash_events
                            .push((prev_tail_pos, crab.pos, crab_color, 1.0));
                    }
                }
                // Roll prev_tail forward so the NEXT catch in the same frame (multi-catch) sees
                // the freshly-linked crab as the tail.
                prev_tail_type = Some(crab.crab_type);
                prev_tail_pos = crab.pos;
                self.chain_count += 1;
                self.total_caught += 1;
                let on_beat = self.beat_timer < BEAT_WINDOW
                    || self.beat_timer > self.beat_interval - BEAT_WINDOW;
                // PERFECT: the catch landed inside the tight sub-window at the very center of the
                // beat. This is the skill ceiling — strictly harder than on_beat, and only it feeds
                // the super-linear flawless-run bonus below.
                let perfect = self.beat_timer < PERFECT_WINDOW
                    || self.beat_timer > self.beat_interval - PERFECT_WINDOW;
                let bonus;
                if on_beat {
                    // Tutorial pass tracking: count real on-beat catches for the beat-timing
                    // learn-session. This is the one write behind the tutorial's pure pass
                    // predicate (`Tutorial::passed`), so a headless run of the same scenario reaches
                    // the same boolean without any rendering.
                    if let Some(t) = self.tutorial.as_mut() {
                        if t.kind == TutorialKind::BeatTiming {
                            t.on_beat_catches += 1;
                        }
                    }
                    // On-beat catch: build the groove. Consecutive on-beat catches escalate the
                    // score bonus and fill the groove meter, which in turn swells the music.
                    self.beat_streak += 1;
                    // Precision ladder: a PERFECT hit extends the flawless run; an on-beat-but-not-
                    // perfect catch keeps beat_streak alive (streak isn't broken) but resets the
                    // flawless run — precision is a bonus lane, never a punishment for near-misses.
                    if perfect {
                        self.perfect_streak += 1;
                        self.perfect_flash = 1.0;
                    } else {
                        self.perfect_streak = 0;
                    }
                    // A Dancer Drum-Major at the head keeps the whole train on time: a fatter groove
                    // fill per on-beat catch so the meter swells (and the music with it) faster.
                    let groove_fill = if head_is_dancer { 0.30 } else { 0.22 };
                    self.groove = (self.groove + groove_fill).min(1.0);
                    bonus = self.beat_streak.min(5) as usize;
                    self.on_beat_flash = (0.25 + self.beat_streak as f32 * 0.06).min(0.6);
                    // Beat-hit punch: additive impact flash at the catch site. Quality 1.0 on a
                    // PERFECT downbeat hit, 0.5 on an ordinary on-beat catch.
                    let beat_quality = if perfect { 1.0_f32 } else { 0.5_f32 };
                    self.beat_punch_events
                        .push((shock_pos, crab_color, beat_quality));
                    // Groove Gamble: the streak compounds a live global score multiplier. Each
                    // on-beat catch bumps it +0.25x (capped at 5x), so the deeper you ride the beat
                    // the more every point — catches AND deliveries — is worth. The catch mid-streak
                    // feels louder: the multiplier only exists while the run is unbroken.
                    let prev_mult = self.beat_gamble_mult;
                    // Drum-Major at the head bumps the gamble harder (+0.35x vs +0.25x): the rhythm
                    // economy the Dancer leads scales the whole run faster, the counterweight to the
                    // Golden figurehead's match-run amplification. One head slot, two ways to spend it.
                    let gamble_step = if head_is_dancer { 0.35 } else { 0.25 };
                    self.beat_gamble_mult = (self.beat_gamble_mult + gamble_step).min(5.0);
                    if self.beat_gamble_mult > prev_mult {
                        self.beat_gamble_flash = 1.0;
                    }
                    // Drum-Major assist reads on screen so the head-slot choice pays visibly, not just
                    // in the meter — a teal rhythm shine on the newly-linked tail, the counterpart to
                    // the Golden figurehead's gild. Fires on every on-beat catch while a Dancer leads.
                    if head_is_dancer {
                        self.floating_texts.spawn(
                            "DRUM-MAJOR!".to_string(),
                            crab.pos - Vec2::new(56.0, 46.0),
                            24.0,
                            [0.4, 1.0, 0.85, 1.0],
                        );
                    }
                    // Escalating callouts as the heat tiers up, so the rising stakes read on screen.
                    if self.beat_streak >= 3 {
                        let (label, col, size) = match self.beat_streak {
                            3..=4 => ("HEATING UP", [0.4, 1.0, 0.85, 1.0], 34.0),
                            5..=7 => ("ON FIRE!", [1.0, 0.7, 0.2, 1.0], 40.0),
                            8..=11 => ("BLAZING!", [1.0, 0.35, 0.15, 1.0], 46.0),
                            _ => ("INFERNO!!", [1.0, 0.2, 0.5, 1.0], 52.0),
                        };
                        self.floating_texts.spawn(
                            format!("{}  x{:.2}", label, self.beat_gamble_mult),
                            self.player_pos - Vec2::new(0.0, 80.0),
                            size,
                            col,
                        );
                    }
                } else {
                    // Off-beat catch breaks the streak and drains the groove. Only the UNBANKED gain
                    // above the locked floor is lost — whatever the player cashed out with B stays
                    // safe. If a hot unbanked stack was riding, punch a red flash + callout so the
                    // greedy grab stings; then fall back to the banked floor, not all the way to 1x.
                    if self.beat_gamble_mult > self.beat_gamble_locked + 0.5 {
                        self.streak_lost_flash = 1.0;
                        self.shake_timer = self.shake_timer.max(0.3);
                        let lost = self.beat_gamble_mult - self.beat_gamble_locked;
                        let msg = if self.beat_gamble_locked > 1.01 {
                            format!(
                                "STREAK LOST!  x{:.2} gone — x{:.2} safe",
                                lost, self.beat_gamble_locked
                            )
                        } else {
                            format!("STREAK LOST!  x{:.2} gone", self.beat_gamble_mult)
                        };
                        self.floating_texts.spawn(
                            msg,
                            self.player_pos - Vec2::new(0.0, 80.0),
                            40.0,
                            [1.0, 0.35, 0.3, 1.0],
                        );
                    }
                    self.beat_streak = 0;
                    self.perfect_streak = 0;
                    self.beat_gamble_mult = self.beat_gamble_locked;
                    self.groove = (self.groove - 0.3).max(0.0);
                    bonus = 0;
                }
                let pos = crab.pos;
                let player_pos = self.player_pos;
                // Whip-streak from the catch point to the head of the train, so the crab reads as
                // yanked in. Brighter/faster-fading trails happen on-beat via the draw's age curve.
                if self.catch_trails.len() < 48 {
                    let head = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
                    let start = if on_beat { -0.25 } else { 0.0 }; // on-beat trails linger a hair longer
                    self.catch_trails.push((crab.pos, head, start, crab_color));
                }
                // Inline register_catch to avoid &mut self conflict with the crabs loop.
                // The Groove Gamble multiplier scales the whole award, so a hot streak makes every
                // catch worth dramatically more — the payoff for riding the beat unbroken.
                let pts = (((1 + bonus) * mult) as f32 * self.beat_gamble_mult).round() as usize;
                self.score += pts;
                // Attribute the rhythm-driven extra: what this catch would have paid at neutral
                // rhythm (no streak bonus, no gamble multiplier) vs. what it actually paid. The gap
                // is the mastery the beat bought, tallied for the "how far ahead" readout.
                let flat = (1 * mult) as usize; // bonus=0, gamble=1x
                self.rhythm_bonus_score += pts.saturating_sub(flat);
                // PERFECT precision bonus — the legible skill ceiling. Awarded on top of everything
                // above, but SEPARATELY from the gamble multiplier (which we deliberately don't
                // touch, so banking stays balanced): a flat, super-linear score kicker that scales
                // with the flawless run. perfect_streak grows the reward quadratically (n*(n+1)/2,
                // the same triangular shape as the pen jackpot) so a sustained in-the-pocket run
                // pulls dramatically ahead of a merely-good one — and the callout shows how far.
                if perfect && self.perfect_streak > 0 {
                    let n = self.perfect_streak.min(24) as usize; // cap so it can't run away
                    // Triangular growth, scaled by the level multiplier: 5, 15, 30, 50, ... per hit.
                    let perfect_pts = (n * (n + 1) / 2) * 5 * mult as usize;
                    self.score += perfect_pts;
                    self.rhythm_bonus_score += perfect_pts;
                    // Legible payoff: the flawless tier and its running rhythm-bonus total, so the
                    // player sees precision compounding. Only fire the loud callout once the run is
                    // deep enough to matter, so early perfects don't spam the screen.
                    if self.perfect_streak >= 3 {
                        let (label, size) = match self.perfect_streak {
                            3..=5 => ("PERFECT!", 34.0),
                            6..=9 => ("FLAWLESS!", 42.0),
                            _ => ("IN THE POCKET!!", 50.0),
                        };
                        self.floating_texts.spawn(
                            format!("{}  x{}  +{}", label, self.perfect_streak, perfect_pts),
                            self.player_pos - Vec2::new(0.0, 116.0),
                            size,
                            [0.6, 0.95, 1.0, 1.0],
                        );
                    }
                }
                // Golden crab: on top of the normal catch award, queue a big lump-sum treasure bonus
                // (paid out after the loop). This is the payoff for breaking off the herd to chase it.
                // Splitter: record the catch so the after-loop cleave can bank the back half. The
                // Splitter has just become the tail (highest chain_index) this catch; the split
                // block below decides where to cleave. Only the last Splitter caught this frame
                // wins — one cleave per frame keeps the moment legible.
                if crab.is_splitter() {
                    splitter_catch = Some(pos);
                }
                if crab.is_golden() {
                    golden_catches.push((pos, pts));
                    // Crossover — the shine conducts down the train. If the link this Golden just
                    // snapped onto (the previous tail) is a Magnet, the Magnet's field carries the
                    // Golden's shine along the whole conga line, paying a length-scaled cascade.
                    // Whether this fires depends purely on catch ORDER: park a Magnet at the tail,
                    // then chase a Golden onto it. Deferred so the cascade payout can borrow &mut self.
                    if prev_tail_type == Some(CrabType::Magnet) {
                        magnet_shine_catches.push(pos);
                    }
                }
                // Same-type match run — the arrangement mechanic. If this crab is the same archetype
                // as the link it just snapped onto (the previous tail), it extends a run of matching
                // neighbors and each additional link pays an escalating bonus; a mismatched catch
                // resets the run to a single link. Whether a run builds depends purely on catch ORDER,
                // so the player catches to *build a pattern* of same-type links, not just length.
                // Deferred payout (bonus + callout borrows &mut self) collected into match_run_catches.
                if prev_tail_type == Some(crab.crab_type) {
                    self.tail_run_len += 1;
                } else {
                    self.tail_run_len = 1;
                }
                if self.tail_run_len >= 2 {
                    // The run length itself is the escalation: link 2 pays a little, deeper runs pay
                    // more, capped so a very long single-type train can't runaway-score. Scaled by the
                    // same combo/gamble multipliers as the base catch so it rides a hot streak too.
                    let run = self.tail_run_len.min(8);
                    // A Golden figurehead at the head amplifies the whole match economy: +50% on
                    // every run bonus while it leads. Legible reward for choosing to park the prize
                    // up front instead of cashing it — the front of the train finally pays.
                    let figurehead_mult = if head_is_golden { 1.5 } else { 1.0 };
                    let match_bonus =
                        ((run as usize) * mult) as f32 * self.beat_gamble_mult * figurehead_mult;
                    self.score += match_bonus.round() as usize;
                    match_run_catches.push((crab.pos, self.tail_run_len, crab.crab_color()));
                    // Match-Run Milestone: crossing every 4th same-type link (4, 8, 12…) is a big,
                    // watchable payoff on top of the incremental run bonus — a bold callout, a
                    // color-matched shockwave down the tail, and a chunky score kicker. Makes
                    // committing to a long single-type run (the order-as-bet) climax visibly
                    // instead of just ticking a counter. Inlined (shockwave/floating_texts fields
                    // are disjoint from the active &mut self.crabs borrow in this loop).
                    if self.tail_run_len >= 4 && self.tail_run_len % 4 == 0 {
                        let tier = self.tail_run_len / 4; // 1 at 4, 2 at 8, …
                        let col = crab.crab_color();
                        // Score kicker scales with the run tier and rides the same hot-streak mults.
                        let kicker = ((self.tail_run_len as usize * 6 * mult) as f32
                            * self.beat_gamble_mult
                            * figurehead_mult)
                            .round() as usize;
                        self.score += kicker;
                        self.floating_texts.spawn(
                            format!("MATCH x{}!  +{}", self.tail_run_len, kicker),
                            crab.pos - Vec2::new(60.0, 64.0),
                            34.0 + tier as f32 * 4.0,
                            [col[0], col[1], col[2], 1.0],
                        );
                        if self.catch_shockwaves.len() < 48 {
                            self.catch_shockwaves.push((crab.pos, 0.0, col));
                        }
                        self.on_beat_flash = self.on_beat_flash.max(0.4);
                        self.shake_timer = self.shake_timer.max(0.5);
                        self.zoom_punch = self.zoom_punch.max(0.06);
                    }
                    if head_is_golden {
                        // Gild the run callout so the figurehead's assist reads on screen, not just
                        // in the score — a small golden shine on the newly-linked tail.
                        self.floating_texts.spawn(
                            "FIGUREHEAD!".to_string(),
                            crab.pos - Vec2::new(52.0, 46.0),
                            24.0,
                            [1.0, 0.86, 0.28, 1.0],
                        );
                        // Inlined shockwave push (a &mut self method call would conflict with the
                        // active &mut borrow of self.crabs in this loop; the field is disjoint).
                        if self.catch_shockwaves.len() < 48 {
                            self.catch_shockwaves
                                .push((crab.pos, 0.0, [1.0, 0.85, 0.3]));
                        }
                    }
                }
                // Roll the tail-type snapshot forward: this freshly-caught crab is now the tail, so
                // it's what the *next* catch this frame will link onto. Keeps the adjacency check O(1)
                // per catch with no mid-loop rescan of self.crabs.
                prev_tail_type = Some(crab.crab_type);
                self.combo_count += 1;
                self.combo_timer = 1.8;
                let score_str = if self.beat_gamble_mult > 1.01 {
                    format!("+{}  x{:.2}!", pts, self.beat_gamble_mult)
                } else if pts > 1 {
                    format!("+{}  ON BEAT!", pts)
                } else {
                    format!("+{}", pts)
                };
                let score_col = if pts > 1 {
                    [1.0, 0.95, 0.3, 1.0]
                } else {
                    [1.0, 1.0, 1.0, 0.9]
                };
                self.floating_texts
                    .spawn(score_str, pos - Vec2::new(10.0, 20.0), 28.0, score_col);
                if self.combo_count >= 3 {
                    let cc = self.combo_count;
                    let combo_col = match cc {
                        3..=4 => [1.0, 0.6, 0.1, 1.0],
                        5..=7 => [1.0, 0.2, 0.2, 1.0],
                        _ => [0.8, 0.3, 1.0, 1.0],
                    };
                    self.floating_texts.spawn(
                        format!("x{} COMBO!", cc),
                        player_pos - Vec2::new(0.0, 50.0),
                        36.0,
                        combo_col,
                    );
                }
                self.shake_timer = 0.4;
                self.time_since_catch = 0.0;
                // Punchy freeze — a touch longer when the catch lands on the beat.
                self.hitstop_timer = self.hitstop_timer.max(if on_beat { 0.08 } else { 0.05 });
                // Snap the camera in a hair on every catch, harder on the beat, for extra impact.
                self.zoom_punch = self.zoom_punch.max(if on_beat { 0.055 } else { 0.035 });
                play_catch_sound(&mut self.sounds, ctx, &mut rng, self.beat_streak);
                // A PERFECT (tight-window) catch also fires the bright sparkle on top, so nailing
                // the precise window is audible, not just a screen flash — the "satisfying drum
                // hit" the rhythm loop wants. perfect_streak was just bumped above, so the pitch
                // climbs with the flawless run. Ordinary on-beat catches skip this entirely.
                if perfect {
                    play_perfect_sparkle(&mut self.sounds, self.perfect_streak);
                }
            }
        }
        // Deferred out of the `&mut self.crabs` loop above: check_upgrade_unlock borrows all of
        // self, which conflicts with the live crab iterator. Score only rises inside the loop, so
        // running the threshold check once afterward is equivalent.
        self.check_upgrade_unlock(ctx);
        for &origin in &startle_origins {
            self.emit_catch_startle(origin);
        }
        for &pos in &dance_catches {
            self.reward_dance_catch(true, pos);
        }
        for &(bpos, ctype) in &boss_catches {
            self.on_boss_caught(bpos, ctype);
        }
        // Dancer King payoff: the timing test IS the fight. Catch the royal ON the beat and its
        // whole entranced court joins your train in one flourish; catch it off-beat and the spell
        // breaks — the court scatters free, and you only keep the King itself.
        if let Some((kpos, on_beat)) = dancer_king_catch {
            if on_beat {
                let mut banked: u32 = 0;
                // Safe direct mutation: this loop only writes scalar fields on each crab and
                // disjoint self fields (chain_count/total_caught/catch_shockwaves) — it never
                // inserts into or removes from self.crabs, mirroring the main catch loop above.
                for crab in &mut self.crabs {
                    if !crab.caught && !crab.is_boss() && crab.entranced > 0.0 {
                        crab.entranced = 0.0;
                        crab.caught = true;
                        crab.chain_index = Some(self.chain_count);
                        self.chain_count += 1;
                        self.total_caught += 1;
                        crab.join_pulse = 1.0;
                        if self.catch_shockwaves.len() < 48 {
                            self.catch_shockwaves
                                .push((crab.pos, 0.0, crab.crab_color()));
                        }
                        banked += 1;
                    }
                }
                if banked > 0 {
                    self.floating_texts.spawn(
                        format!("PERFECT CATCH — THE COURT FOLLOWS! +{banked}"),
                        kpos - Vec2::new(200.0, 90.0),
                        36.0,
                        [1.0, 0.62, 0.45, 1.0],
                    );
                    self.spawn_catch_shockwave(kpos, [1.0, 0.62, 0.45]);
                    self.screen_shake = self.screen_shake.max(14.0);
                }
            } else {
                let mut scattered = false;
                for crab in &mut self.crabs {
                    if crab.entranced > 0.0 {
                        crab.entranced = 0.0;
                        scattered = true;
                    }
                }
                if scattered {
                    self.floating_texts.spawn(
                        "OFF-BEAT — THE COURT SCATTERS!".to_string(),
                        kpos - Vec2::new(160.0, 70.0),
                        28.0,
                        [0.8, 0.75, 0.9, 1.0],
                    );
                }
            }
        }
        // Apply Reef DJ shell chips from hype dancers caught on a hot beat. Find the live DJ and
        // knock a chunk off its shell per dancer, with a legible callout + juice so the assist
        // reads on screen. If a chip finishes the boss, queue its catch payoff like a beam kill.
        if !hype_dancer_hits.is_empty() {
            let mut broke_at: Option<Vec2> = None;
            for crab in &mut self.crabs {
                if crab.is_rhythm_boss() && !crab.caught && crab.boss_health > 0.0 {
                    for _ in &hype_dancer_hits {
                        crab.boss_health -= 0.4;
                    }
                    if crab.boss_health <= 0.0 {
                        crab.boss_health = 0.0;
                        broke_at = Some(crab.pos);
                    }
                    break;
                }
            }
            for &dpos in &hype_dancer_hits {
                self.floating_texts.spawn(
                    "HYPE! shell cracked".to_string(),
                    dpos - Vec2::new(40.0, 40.0),
                    28.0,
                    [0.85, 0.5, 1.0, 1.0],
                );
                self.particle_system
                    .spawn_milestone_fireworks(dpos, 8, &mut crate::rng::rng());
            }
            self.reef_hit_flash = 1.0;
            self.screen_shake = self.screen_shake.max(6.0);
            // A dancer chip that empties the shell worns the DJ down (it doesn't catch it — the
            // player still snaps it up). Fire the same "worn down, catch it!" juice as the beam path.
            if let Some(bpos) = broke_at {
                self.floating_texts.spawn(
                    "WORN DOWN — CATCH IT!".to_string(),
                    bpos - Vec2::new(110.0, 46.0),
                    34.0,
                    [0.4, 1.0, 0.5, 1.0],
                );
                self.spawn_catch_shockwave(bpos, [1.0, 0.85, 0.3]);
                self.screen_shake = self.screen_shake.max(14.0);
                self.on_beat_flash = self.on_beat_flash.max(0.4);
            }
        }
        for &(gpos, base_pts) in &golden_catches {
            self.on_golden_caught(gpos, base_pts);
        }
        // Magnet-shine cascade: a Golden caught directly behind a Magnet link conducts its shine
        // down the whole train. Paid out here so it can borrow &mut self for score/particles/trails.
        for &spos in &magnet_shine_catches {
            self.on_magnet_shine_cascade(spos);
        }
        // Splitter cleave: catching a Splitter halves the train at the midpoint and instantly banks
        // the back half for points — the arrangement *bet*. Done here (after the catch loop) so it
        // can borrow &mut self to rewrite chain_index across the whole train and pay out.
        if let Some(spos) = splitter_catch {
            self.split_train_bank(spos);
        }
        // Same-type match runs: a legible, escalating callout in the matched archetype's own color
        // so the player sees the arrangement paying off — "MATCH x3!" grows and brightens with the
        // run, and a matching-hued ring/shockwave marks the newly-linked tail so the bond reads on
        // screen, not just in the score. This is the watchable feedback for catching to build a
        // pattern; the colored rope bond (see draw_conga_rope) is the persistent version of it.
        for &(pos, run, col) in &match_run_catches {
            let size = (26.0 + run as f32 * 4.0).min(52.0);
            self.floating_texts.spawn(
                format!("MATCH x{}!", run),
                pos - Vec2::new(0.0, 44.0),
                size,
                [col[0], col[1], col[2], 1.0],
            );
            self.spawn_catch_shockwave(pos, col);
            // A deep run lands harder — a little shake + on-beat flash so a long same-type streak
            // feels like a real escalation, matching how combos/streaks escalate their juice.
            if run >= 4 {
                // Cap the shake against the same ceiling the score uses so a very long single-type
                // run can't escalate screen shake without bound (visual spam) every catch.
                self.screen_shake = self.screen_shake.max(3.0 + run.min(8) as f32);
                self.on_beat_flash = self.on_beat_flash.max(0.3);
            }
        }
        // Hand the scratch buffers back for reuse next frame.
        self.startle_origins_buf = startle_origins;
        self.boss_catches_buf = boss_catches;
        self.dance_catches_buf = dance_catches;
        self.golden_catches_buf = golden_catches;
        self.magnet_shine_catches_buf = magnet_shine_catches;
        self.match_run_catches_buf = match_run_catches;
        self.hype_dancer_hits_buf = hype_dancer_hits;
        if any_caught {
            self.check_milestone(&mut crate::rng::rng());
        }
    }

    /// Live catch reach applied around every conga link this frame: base + the lasso/upgrade bump +
    /// the transient on-beat bloom (widest on the downbeat, decayed between beats). Kept in one place
    /// so the gameplay value and the drawn ring can't drift apart.
    pub(crate) fn catch_radius(&self) -> f32 {
        (45.0 + self.catch_radius_upgrade + self.beat_catch_bloom) * self.weather_catch_mult()
    }
}
