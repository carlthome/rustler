//! Opt-in "How to Play" tutorial mode — isolated, scripted learn-sessions reachable from the
//! title screen. Unlike the main run, a tutorial spawns only the objects relevant to the one
//! mechanic it teaches, shows a plain-language instruction card, and defines a *machine-readable
//! pass condition*: a pure boolean predicate over game state.
//!
//! Why the predicate is pure: the same scenario can then be driven headlessly (deterministic
//! spawn, simulated inputs) and used by dev agents as a mechanic regression test — "does the
//! beat-timing tutorial still pass?" is a far tighter signal than "does it build?". Keeping
//! `passed()` a plain function of counters (no rendering, no input polling) is what makes that
//! future harness trivial to wire, so resist entangling it with draw/input state.
//!
//! This slice ships four scenarios (beat-timing, chain-and-deliver, shell-cracking, lasso-grab).
//! More live here as their own `TutorialKind` variants; each stays a tiny sandbox with one card
//! and one counter.

/// Which mechanic a tutorial session teaches. One variant per major mechanic. Each is a tiny
/// scripted sandbox with one instruction card and one pure boolean pass predicate.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TutorialKind {
    /// Catch crabs *on the beat*: teaches the core rhythm timing that drives the groove meter.
    BeatTiming,
    /// Build a conga train and drive it into the delivery pen: teaches catching to grow the
    /// chain and banking it for points — the game's central risk/reward loop.
    ChainDeliver,
    /// Ground-pound the Stomp (R) to crack Armored shells the beam can't wear down: teaches the
    /// right tool for a hard target — pick the verb the herd needs, not just the beam.
    ShellCrack,
    /// Throw the lasso (left click) to snatch crabs at range: teaches that you don't have to walk
    /// onto a crab to catch it — fling the rope out and reel one in from across the field.
    LassoGrab,
}

/// A live, scripted tutorial session. It runs the *normal* update/draw path (the beat clock and
/// catches must actually tick), but the run is constrained: no bosses, no wave escalation, no
/// level advance — just the handful of crabs this session spawned and one goal to fulfil.
pub struct Tutorial {
    pub kind: TutorialKind,
    /// On-beat catches banked so far this session. Bumped only at the on-beat catch branch in
    /// `update_crabs`, guarded by the tutorial being active — so it counts real, timed catches.
    pub on_beat_catches: u32,
    /// Successful train deliveries banked so far this session (ChainDeliver only). Bumped at the
    /// delivery branch in `try_deliver_train`, guarded by the tutorial being active — so it counts
    /// real banks at the pen.
    pub deliveries: u32,
    /// Armored shells cracked open by a Stomp so far this session (ShellCrack only). Bumped at the
    /// shell-crack branch in the Stomp loop in `update`, guarded by the tutorial being active — so
    /// it counts real Stomp cracks, not beam wear-down.
    pub shells_cracked: u32,
    /// Crabs snatched by a thrown lasso so far this session (LassoGrab only). Bumped at the
    /// lasso-catch branch in `update`, guarded by the tutorial being active — so it counts real
    /// rope grabs, not walk-into-them beam catches.
    pub lasso_catches: u32,
    /// How many of the session's tracked action (on-beat catches, or deliveries) clear it.
    pub target: u32,
    /// Eases 0->1 once the pass condition trips, so the draw layer can play a "PASSED!" beat
    /// before the session hands control back to the title screen.
    pub pass_glow: f32,
    /// True once `passed()` first became true — latched so we celebrate exactly once and start
    /// the return-to-menu countdown, even if a later catch would still satisfy the predicate.
    pub completed: bool,
    /// Real-time seconds left on the celebratory hold before returning to the title screen.
    pub exit_timer: f32,
}

impl Tutorial {
    pub fn new(kind: TutorialKind) -> Self {
        let target = match kind {
            TutorialKind::BeatTiming => 3,
            TutorialKind::ChainDeliver => 2,
            TutorialKind::ShellCrack => 3,
            TutorialKind::LassoGrab => 3,
        };
        Tutorial {
            kind,
            on_beat_catches: 0,
            deliveries: 0,
            shells_cracked: 0,
            lasso_catches: 0,
            target,
            pass_glow: 0.0,
            completed: false,
            exit_timer: 0.0,
        }
    }

    /// The plain-language instruction card headline shown at the top of the sandbox.
    pub fn title(&self) -> &'static str {
        match self.kind {
            TutorialKind::BeatTiming => "How to Play — Catching on the Beat",
            TutorialKind::ChainDeliver => "How to Play — Building & Banking a Train",
            TutorialKind::ShellCrack => "How to Play — Cracking Armored Shells",
            TutorialKind::LassoGrab => "How to Play — Throwing the Lasso",
        }
    }

    /// One or two lines telling the player exactly what to do to pass.
    pub fn instruction(&self) -> &'static str {
        match self.kind {
            TutorialKind::BeatTiming => {
                "Watch the beat pulse. Steer into a crab right as the beat lands — an on-beat catch\n\
                 flashes and builds your groove. Land 3 on-beat catches to finish."
            }
            TutorialKind::ChainDeliver => {
                "Catch a few crabs to grow your conga train, then drive the train into the glowing\n\
                 delivery pen to bank them for points. Bank 2 trains to finish."
            }
            TutorialKind::ShellCrack => {
                "These Armored crabs shrug off your beam. Get close and press R to STOMP — the\n\
                 shockwave cracks their shells wide open. Crack 3 shells to finish."
            }
            TutorialKind::LassoGrab => {
                "These crabs are too far to walk to. Left-click near one to fling your lasso and\n\
                 snatch it from across the field. Rope in 3 crabs to finish."
            }
        }
    }

    /// The pass condition, as a pure boolean predicate over game state. This is the piece a
    /// headless regression harness queries; keep it free of rendering/input so it stays trivially
    /// callable off the main loop.
    pub fn passed(&self) -> bool {
        match self.kind {
            TutorialKind::BeatTiming => self.on_beat_catches >= self.target,
            TutorialKind::ChainDeliver => self.deliveries >= self.target,
            TutorialKind::ShellCrack => self.shells_cracked >= self.target,
            TutorialKind::LassoGrab => self.lasso_catches >= self.target,
        }
    }

    /// A short scored-progress line for the HUD, e.g. "On-beat catches: 2 / 3".
    pub fn progress_line(&self) -> String {
        match self.kind {
            TutorialKind::BeatTiming => format!(
                "On-beat catches: {} / {}",
                self.on_beat_catches.min(self.target),
                self.target
            ),
            TutorialKind::ChainDeliver => format!(
                "Trains banked: {} / {}",
                self.deliveries.min(self.target),
                self.target
            ),
            TutorialKind::ShellCrack => format!(
                "Shells cracked: {} / {}",
                self.shells_cracked.min(self.target),
                self.target
            ),
            TutorialKind::LassoGrab => format!(
                "Lasso grabs: {} / {}",
                self.lasso_catches.min(self.target),
                self.target
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beat_timing_not_passed_before_target() {
        let mut t = Tutorial::new(TutorialKind::BeatTiming);
        assert!(!t.passed());
        t.on_beat_catches = t.target - 1;
        assert!(!t.passed());
    }

    #[test]
    fn beat_timing_passes_at_target() {
        let mut t = Tutorial::new(TutorialKind::BeatTiming);
        t.on_beat_catches = t.target;
        assert!(t.passed());
    }

    #[test]
    fn beat_timing_passes_when_over_target() {
        let mut t = Tutorial::new(TutorialKind::BeatTiming);
        t.on_beat_catches = t.target + 5;
        assert!(t.passed());
    }

    #[test]
    fn progress_line_format() {
        let mut t = Tutorial::new(TutorialKind::BeatTiming);
        t.on_beat_catches = 2;
        let line = t.progress_line();
        assert!(line.contains("2"), "progress line should show current count");
        assert!(line.contains(&t.target.to_string()), "progress line should show target");
    }

    #[test]
    fn chain_deliver_not_passed_before_target() {
        let mut t = Tutorial::new(TutorialKind::ChainDeliver);
        assert!(!t.passed());
        t.deliveries = t.target - 1;
        assert!(!t.passed());
    }

    #[test]
    fn chain_deliver_passes_at_target() {
        let mut t = Tutorial::new(TutorialKind::ChainDeliver);
        t.deliveries = t.target;
        assert!(t.passed());
    }

    #[test]
    fn chain_deliver_ignores_on_beat_catches() {
        // A delivery session must not be cleared by catching alone — only banked trains count.
        let mut t = Tutorial::new(TutorialKind::ChainDeliver);
        t.on_beat_catches = 99;
        assert!(!t.passed());
    }

    #[test]
    fn chain_deliver_progress_line_format() {
        let mut t = Tutorial::new(TutorialKind::ChainDeliver);
        t.deliveries = 1;
        let line = t.progress_line();
        assert!(line.contains('1'), "progress line should show current count");
        assert!(line.contains(&t.target.to_string()), "progress line should show target");
    }

    #[test]
    fn shell_crack_not_passed_before_target() {
        let mut t = Tutorial::new(TutorialKind::ShellCrack);
        assert!(!t.passed());
        t.shells_cracked = t.target - 1;
        assert!(!t.passed());
    }

    #[test]
    fn shell_crack_passes_at_target() {
        let mut t = Tutorial::new(TutorialKind::ShellCrack);
        t.shells_cracked = t.target;
        assert!(t.passed());
    }

    #[test]
    fn shell_crack_ignores_other_counters() {
        // A shell-cracking session must not be cleared by catching or banking — only Stomp cracks
        // count, so a learner can't skip the lesson by leaning on the beam or the train.
        let mut t = Tutorial::new(TutorialKind::ShellCrack);
        t.on_beat_catches = 99;
        t.deliveries = 99;
        assert!(!t.passed());
    }

    #[test]
    fn shell_crack_progress_line_format() {
        let mut t = Tutorial::new(TutorialKind::ShellCrack);
        t.shells_cracked = 2;
        let line = t.progress_line();
        assert!(line.contains('2'), "progress line should show current count");
        assert!(line.contains(&t.target.to_string()), "progress line should show target");
    }

    #[test]
    fn lasso_grab_not_passed_before_target() {
        let mut t = Tutorial::new(TutorialKind::LassoGrab);
        assert!(!t.passed());
        t.lasso_catches = t.target - 1;
        assert!(!t.passed());
    }

    #[test]
    fn lasso_grab_passes_at_target() {
        let mut t = Tutorial::new(TutorialKind::LassoGrab);
        t.lasso_catches = t.target;
        assert!(t.passed());
    }

    #[test]
    fn lasso_grab_ignores_other_counters() {
        // A lasso session must not be cleared by walking into crabs or banking — only rope grabs
        // count, so the learner actually practices the throw instead of leaning on the beam.
        let mut t = Tutorial::new(TutorialKind::LassoGrab);
        t.on_beat_catches = 99;
        t.deliveries = 99;
        t.shells_cracked = 99;
        assert!(!t.passed());
    }

    #[test]
    fn lasso_grab_progress_line_format() {
        let mut t = Tutorial::new(TutorialKind::LassoGrab);
        t.lasso_catches = 2;
        let line = t.progress_line();
        assert!(line.contains('2'), "progress line should show current count");
        assert!(line.contains(&t.target.to_string()), "progress line should show target");
    }
}
