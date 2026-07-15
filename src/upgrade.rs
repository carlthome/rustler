//! Upgrade system — the "choose an upgrade" moment and everything that feeds it.
//!
//! Extracted from main.rs (part 1 of the main.rs module split). This owns the upgrade *model*:
//! the pool of options, their card metadata, when an upgrade unlocks, how the three-card offer is
//! rolled, and how a chosen card is applied to the player's stat knobs. The upgrade *rendering*
//! (`draw_upgrade_screen` / `upgrade_card_rects`) stays in main.rs for now because it leans on a
//! shared per-frame Text/mesh cache thread_local that lives alongside the other HUD caches there.
//!
//! Because `upgrade` is a child module of the crate root (`mod upgrade;` in main.rs), the
//! `impl MainState` block below can freely read and mutate the struct's private fields and call
//! its private methods — Rust privacy is module-scoped and a child sees its ancestor's privates.

use ggez::Context;
use ggez::audio::SoundSource;

use crate::MainState;

// Upgrade cadence. The first upgrade lands at UPGRADE_FIRST_AT, each subsequent one costs
// UPGRADE_STEP more (a rising threshold), so upgrades are rarer and feel earned as a run goes on.
pub const UPGRADE_FIRST_AT: usize = 25;
pub const UPGRADE_STEP: usize = 1000;

// --- Upgrade pool -----------------------------------------------------------------------------
// The upgrade screen no longer offers the same fixed four lanes every time. Instead a pool of
// options is defined here; when an upgrade is queued, three DISTINCT options are rolled from it
// (see roll_upgrade_offer) and the player picks one. Some are pure lane deepenings (the old
// behaviour, now a subset); others are TRADEOFFS that reshape how the next stretch plays by
// giving AND taking (a nimbler rustler with a shorter reach, a whole-screen net that handles like
// a barge). This makes the upgrade moment a lucky, powerful *choice* rather than a flow-breaking
// pause — the Vampire Survivors note from Carl. It reshapes the existing upgrade system; every
// effect below is expressed through stat knobs the game already reads (the four tool ranks,
// catch_radius_upgrade, the flashlight cone/range, and a single player speed multiplier), so no
// new mechanic is introduced — consistent with the mechanics freeze.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UpgradeId {
    BeamFocus,
    LassoFocus,
    WhistleFocus,
    StompFocus,
    Featherweight, // + move speed, − catch reach
    WideNet,       // ++ catch reach, − move speed
    HeavyHauler,   // + lasso lane & catch reach, − move speed
    Sharpshooter,  // + beam lane & boss melt, − catch reach
    Roadrunner,    // ++ move speed & whistle lane, − beam cone
}

// Every option in the pool, in a stable order. roll_upgrade_offer picks three distinct indices.
pub const UPGRADE_POOL: [UpgradeId; 9] = [
    UpgradeId::BeamFocus,
    UpgradeId::LassoFocus,
    UpgradeId::WhistleFocus,
    UpgradeId::StompFocus,
    UpgradeId::Featherweight,
    UpgradeId::WideNet,
    UpgradeId::HeavyHauler,
    UpgradeId::Sharpshooter,
    UpgradeId::Roadrunner,
];

impl UpgradeId {
    /// Display metadata for a card: (icon, name, description, r, g, b). Description lines are
    /// separated by "\n"; keep the "+" boons and "−" costs legible at a glance.
    pub fn card(self) -> (&'static str, &'static str, &'static str, u8, u8, u8) {
        match self {
            UpgradeId::BeamFocus => (">", "Beam Focus", "Wider, longer beam\n+ faster boss melt", 255, 200, 40),
            UpgradeId::LassoFocus => ("O", "Lasso Focus", "Bigger chain reach\n+ wider lasso grab", 60, 220, 100),
            UpgradeId::WhistleFocus => ("~", "Whistle Focus", "Bigger, stronger pull\n+ faster recharge", 80, 160, 255),
            UpgradeId::StompFocus => ("*", "Stomp Focus", "Wider shockwave\n+ faster recharge", 200, 60, 255),
            UpgradeId::Featherweight => ("^", "Featherweight", "+ Move faster\n\u{2212} Shorter catch reach", 120, 255, 220),
            UpgradeId::WideNet => ("#", "Wide Net", "++ Huge catch reach\n\u{2212} Handle sluggish", 255, 150, 60),
            UpgradeId::HeavyHauler => ("=", "Heavy Hauler", "+ Chain reach & lasso\n\u{2212} Move slower", 90, 200, 140),
            UpgradeId::Sharpshooter => ("!", "Sharpshooter", "+ Beam & boss melt\n\u{2212} Shorter catch reach", 255, 90, 90),
            UpgradeId::Roadrunner => ("%", "Roadrunner", "++ Speed & whistle\n\u{2212} Narrower beam", 255, 235, 70),
        }
    }
}

impl MainState {
    /// If the banked score has crossed the next upgrade threshold, queue an upgrade and advance
    /// the threshold by a rising step so later upgrades are rarer and earned. Uses `>=` because
    /// score can overshoot the threshold in one banked jump (combo-multiplier steps). Call this
    /// after any score increase; it's the single knob for upgrade cadence.
    pub fn check_upgrade_unlock(&mut self, ctx: &mut Context) {
        if self.score >= self.next_upgrade_score {
            // Queue exactly ONE upgrade, then advance the threshold past the *current* score so a
            // single banked jump can never trigger back-to-back screens. Score rises by the combo
            // multiplier per catch (often several points at once) and a fast cluster catch can
            // overshoot the threshold by a lot; the old fixed `+= UPGRADE_STEP` left the new
            // threshold still below the score, so the very next catch popped another upgrade screen
            // immediately — the "fires at a wrong moment / pick one and another pops" bug Carl hit.
            // Looping the step until it clears the current score keeps upgrades one-at-a-time and
            // spaced by real earned progress.
            while self.score >= self.next_upgrade_score {
                self.next_upgrade_score += UPGRADE_STEP;
            }
            let _ = self.sounds.upgrade.play_detached(ctx);
            // Roll the three cards ONCE here, at queue time, not in draw — draw runs every frame
            // and would otherwise reshuffle the offer 60×/sec.
            self.roll_upgrade_offer();
            self.pending_upgrade = true;
        }
    }

    /// Pick three DISTINCT options from UPGRADE_POOL for the pending upgrade screen. Simple
    /// partial Fisher–Yates shuffle of the pool indices, taking the first three — cheap and
    /// guarantees no duplicate cards.
    pub fn roll_upgrade_offer(&mut self) {
        use rand::seq::SliceRandom;
        let mut idx: Vec<usize> = (0..UPGRADE_POOL.len()).collect();
        idx.shuffle(&mut rand::rng());
        self.offered_upgrades = [idx[0], idx[1], idx[2]];
    }

    pub fn apply_upgrade(&mut self, choice: u8) {
        let slot = choice as usize;
        if slot < 1 || slot > 3 {
            self.pending_upgrade = false;
            return;
        }
        let id = UPGRADE_POOL[self.offered_upgrades[slot - 1]];
        match id {
            UpgradeId::BeamFocus => self.rank_beam_lane(),
            // Lasso lane (chain catcher): wider passive chain reach AND a bigger lasso grab window.
            UpgradeId::LassoFocus => {
                self.lasso_rank += 1;
                self.catch_radius_upgrade += 18.0;
            }
            // Whistle lane (crowd control): bigger pulse, stronger pull, shorter cooldown.
            UpgradeId::WhistleFocus => self.whistle_rank += 1,
            // Stomp lane (shell breaker): bigger, faster shockwave.
            UpgradeId::StompFocus => self.stomp_rank += 1,
            // Tradeoffs — give and take, expressed through existing stat knobs.
            UpgradeId::Featherweight => {
                self.speed_mult = (self.speed_mult + 0.18).min(2.2);
                self.catch_radius_upgrade -= 12.0;
            }
            UpgradeId::WideNet => {
                self.catch_radius_upgrade += 34.0;
                self.speed_mult = (self.speed_mult - 0.14).max(0.55);
            }
            UpgradeId::HeavyHauler => {
                self.lasso_rank += 1;
                self.catch_radius_upgrade += 20.0;
                self.speed_mult = (self.speed_mult - 0.12).max(0.55);
            }
            UpgradeId::Sharpshooter => {
                self.rank_beam_lane();
                self.catch_radius_upgrade -= 14.0;
            }
            UpgradeId::Roadrunner => {
                self.speed_mult = (self.speed_mult + 0.24).min(2.2);
                self.whistle_rank += 1;
                self.flashlight.cone_upgrade -= 0.12;
            }
        }
        // Keep catch reach from going negative no matter how the tradeoffs stack.
        self.catch_radius_upgrade = self.catch_radius_upgrade.max(-20.0);
        self.pending_upgrade = false;
    }
}
