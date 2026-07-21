//! Procedural audio — no external audio files.
//!
//! The rhythm is the whole game. This package synthesises every sound at runtime and hands it to
//! ggez as an in-memory WAV, so the tempo is felt (kick thump) as much as seen.
//!
//! Two concerns, two modules:
//!   * [`audio`] — SYNTHESIS: how a sound is *made* (oscillators, envelopes, filters, noise, WAV
//!     baking, the one-off SFX/percussion voices, and the live `BeatSynth` kit).
//!   * [`music`] — STRUCTURE: what notes are played, when, in what key/groove (scales, tempo,
//!     the hand-written themes, and the generative `synth_action_groove`).
//!
//! `music` calls into `audio`; `audio` knows nothing about keys or tempo. Every historical
//! `crate::sounds::…` path is preserved by the re-exports below, so callers elsewhere in the
//! codebase (and the sibling `king_crab_audio` module) are unchanged.

mod audio;
mod music;

// --- Public API (was `pub` in the flat `sounds.rs`) -----------------------------------------
// Synthesis / SFX voices.
pub use audio::{
    synth_ambient_pad, synth_coin_chime, synth_flashlight_toggle, synth_hihat, synth_lasso_throw,
    synth_perfect_sparkle, synth_rival_steal, synth_startup_pling, synth_steal_gain,
    synth_steal_loss, synth_stomp, synth_tool_accent, synth_whistle, BeatSynth, PadPreset,
    Waveform,
};
// Musical structure.
pub use music::{
    biome_rival_motif_tuning, detect_bpm_from_ogg, synth_biome_action_groove, synth_intro_menu,
    synth_theme_deus_ambient, synth_theme_deus_tense, synth_theme_duck_bounce, synth_theme_duck_funky,
    synth_theme_duck_golden, ACTION_KEY_ROOT_MIDI, GROOVE_SWING,
};

// --- Crate-internal helpers (were `pub(crate)` in the flat `sounds.rs`) ----------------------
// `king_crab_audio` reaches these via `crate::sounds::…`, so keep them exposed at this path.
pub(crate) use audio::{
    encode_wav_mono16, encode_wav_stereo16, master_limiter, oscillator_sample, samples_to_pcm,
    SAMPLE_RATE,
};

// King Crab boss / NPC-train audio lives in its own file but is part of the `sounds` public API.
pub use crate::king_crab_audio::{
    synth_king_crab_ambient_spatial, synth_king_crab_rumble, synth_king_crab_spatial,
    synth_rival_motif,
};
