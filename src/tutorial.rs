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
//! This first slice ships one scenario (beat-timing). More live here as their own `TutorialKind`
//! variants; each stays a tiny sandbox with one card and one counter.

/// Which mechanic a tutorial session teaches. One variant per major mechanic; only the
/// beat-timing slice is scripted so far — the rest are placeholders for future sessions.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TutorialKind {
    /// Catch crabs *on the beat*: teaches the core rhythm timing that drives the groove meter.
    BeatTiming,
}

/// A live, scripted tutorial session. It runs the *normal* update/draw path (the beat clock and
/// catches must actually tick), but the run is constrained: no bosses, no wave escalation, no
/// level advance — just the handful of crabs this session spawned and one goal to fulfil.
pub struct Tutorial {
    pub kind: TutorialKind,
    /// On-beat catches banked so far this session. Bumped only at the on-beat catch branch in
    /// `update_crabs`, guarded by the tutorial being active — so it counts real, timed catches.
    pub on_beat_catches: u32,
    /// How many on-beat catches clear the session.
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
        };
        Tutorial {
            kind,
            on_beat_catches: 0,
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
        }
    }

    /// One or two lines telling the player exactly what to do to pass.
    pub fn instruction(&self) -> &'static str {
        match self.kind {
            TutorialKind::BeatTiming => {
                "Watch the beat pulse. Steer into a crab right as the beat lands — an on-beat catch\n\
                 flashes and builds your groove. Land 3 on-beat catches to finish."
            }
        }
    }

    /// The pass condition, as a pure boolean predicate over game state. This is the piece a
    /// headless regression harness queries; keep it free of rendering/input so it stays trivially
    /// callable off the main loop.
    pub fn passed(&self) -> bool {
        self.on_beat_catches >= self.target
    }

    /// A short scored-progress line for the HUD, e.g. "On-beat catches: 2 / 3".
    pub fn progress_line(&self) -> String {
        format!("On-beat catches: {} / {}", self.on_beat_catches.min(self.target), self.target)
    }
}
