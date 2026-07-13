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
//! This slice ships two scenarios (beat-timing, chain-and-deliver). More live here as their own
//! `TutorialKind` variants; each stays a tiny sandbox with one card and one counter.

/// Which mechanic a tutorial session teaches. One variant per major mechanic. Each is a tiny
/// scripted sandbox with one instruction card and one pure boolean pass predicate.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TutorialKind {
    /// Catch crabs *on the beat*: teaches the core rhythm timing that drives the groove meter.
    BeatTiming,
    /// Build a conga train and drive it into the delivery pen: teaches catching to grow the
    /// chain and banking it for points — the game's central risk/reward loop.
    ChainDeliver,
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
        };
        Tutorial {
            kind,
            on_beat_catches: 0,
            deliveries: 0,
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
        }
    }

    /// The pass condition, as a pure boolean predicate over game state. This is the piece a
    /// headless regression harness queries; keep it free of rendering/input so it stays trivially
    /// callable off the main loop.
    pub fn passed(&self) -> bool {
        match self.kind {
            TutorialKind::BeatTiming => self.on_beat_catches >= self.target,
            TutorialKind::ChainDeliver => self.deliveries >= self.target,
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
}
