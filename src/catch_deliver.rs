//! The delivery side of the catch-and-deliver loop for `MainState`: banking a completed
//! train at the pen (`try_deliver_train`) and the shared catch-reach helper
//! (`catch_radius`). The per-frame catching of free crabs lives in `crab_catching.rs`.
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

    /// Live catch reach applied around every conga link this frame: base + the lasso/upgrade bump +
    /// the transient on-beat bloom (widest on the downbeat, decayed between beats). Kept in one place
    /// so the gameplay value and the drawn ring can't drift apart.
    pub(crate) fn catch_radius(&self) -> f32 {
        (45.0 + self.catch_radius_upgrade + self.beat_catch_bloom) * self.weather_catch_mult()
    }
}
