//! Musical STRUCTURE — what notes are played, when, in what key/groove.
//!
//! Split out of the old monolithic `sounds.rs`. This half owns the musical decisions: BPM/tempo
//! detection, the swing/groove constants, scales/keys and note-frequency tables, the hand-written
//! Game Boy themes and their note sequences, and the generative call-and-response groove engine
//! (`synth_action_groove`). It decides the pitches and rhythms, then calls the synthesis
//! primitives in the sibling `audio` module to render them into a buffer.

use ggez::audio::{SoundData, SoundSource, Source};
use ggez::{Context, GameResult};

use super::audio::{
    bitcrush, compress, encode_wav_mono16, encode_wav_stereo16, gb_pulse_note, master_limiter, mix_into,
    normalize_and_saturate, samples_to_pcm, synth_note, Adsr, Waveform, SAMPLE_RATE,
};

/// Detect the dominant BPM from a raw OGG file and return the beat interval in seconds.
///
/// Algorithm:
/// 1. Decode up to 30 s of OGG/Vorbis to f32 PCM with lewton.
/// 2. Mix to mono, then compute a 100 Hz onset-strength signal: sliding window RMS energy
///    (window ~20 ms, hop ~10 ms), positive-only first derivative (half-wave rectified).
/// 3. Autocorrelate the onset envelope across lag ranges corresponding to 60–180 BPM.
/// 4. Return `Some(60.0 / bpm)` for the dominant peak, or `None` if detection is uncertain.
pub fn detect_bpm_from_ogg(ogg_bytes: &[u8]) -> Option<f32> {
    use lewton::inside_ogg::OggStreamReader;
    use std::io::Cursor;

    let cursor = Cursor::new(ogg_bytes);
    let mut reader = OggStreamReader::new(cursor).ok()?;
    let sample_rate = reader.ident_hdr.audio_sample_rate as f32;
    let channels = reader.ident_hdr.audio_channels as usize;

    // Decode up to 30 seconds of audio into interleaved i16 samples.
    let max_samples = (sample_rate as usize) * 30 * channels;
    let mut interleaved: Vec<f32> = Vec::with_capacity(max_samples);
    while interleaved.len() < max_samples {
        match reader.read_dec_packet_itl() {
            Ok(Some(pkt)) => {
                for s in pkt {
                    interleaved.push(s as f32 / 32768.0);
                    if interleaved.len() >= max_samples {
                        break;
                    }
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    if interleaved.is_empty() {
        return None;
    }

    // Mix to mono.
    let n_frames = interleaved.len() / channels;
    let mut mono: Vec<f32> = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        let mut sum = 0.0_f32;
        for c in 0..channels {
            sum += interleaved[i * channels + c];
        }
        mono.push(sum / channels as f32);
    }

    // Build onset-strength signal at ~100 Hz.
    // Window = 20 ms, hop = 10 ms.
    let hop = (sample_rate * 0.010).round() as usize;
    let win = (sample_rate * 0.020).round() as usize;
    let onset_rate = sample_rate / hop as f32; // ~100 Hz

    let n_frames_total = mono.len();
    let n_onset = (n_frames_total.saturating_sub(win)) / hop;
    if n_onset < 4 {
        return None;
    }

    let mut energy: Vec<f32> = Vec::with_capacity(n_onset);
    for k in 0..n_onset {
        let start = k * hop;
        let end = (start + win).min(mono.len());
        let rms: f32 = mono[start..end].iter().map(|x| x * x).sum::<f32>() / (end - start) as f32;
        energy.push(rms.sqrt());
    }

    // Half-wave rectified first derivative = onset strength.
    let mut onset: Vec<f32> = vec![0.0; energy.len()];
    for i in 1..energy.len() {
        let d = energy[i] - energy[i - 1];
        onset[i] = if d > 0.0 { d } else { 0.0 };
    }

    // Autocorrelation over lags corresponding to 60–180 BPM.
    let lag_min = (onset_rate * 60.0 / 180.0).round() as usize; // 180 BPM
    let lag_max = (onset_rate * 60.0 / 60.0).round() as usize; // 60 BPM
    if lag_max >= onset.len() {
        return None;
    }

    let n_ac = onset.len();
    let mut best_lag = lag_min;
    let mut best_val = f32::NEG_INFINITY;
    for lag in lag_min..=lag_max.min(n_ac - 1) {
        let mut ac = 0.0_f32;
        let n_sum = n_ac - lag;
        for i in 0..n_sum {
            ac += onset[i] * onset[i + lag];
        }
        ac /= n_sum as f32;
        if ac > best_val {
            best_val = ac;
            best_lag = lag;
        }
    }

    if best_val <= 0.0 {
        return None;
    }

    let bpm = 60.0 * onset_rate / best_lag as f32;
    // Sanity check: clamp to plausible range.
    if bpm < 55.0 || bpm > 190.0 {
        return None;
    }
    Some(60.0 / bpm)
}

/// Shuffle amount shared by the generative backing groove (`synth_action_groove`) and the live
/// hi-hat kit (`BeatSynth`), so the two can never drift: odd 1/16 steps land late by
/// `GROOVE_SWING * 0.5` of a 1/16 note. 0.0 = straight, ~0.66 = a loose triplet shuffle.
pub const GROOVE_SWING: f32 = 0.66;
/// Canonical key center for every gameplay music source: A3 / A minor.
pub const ACTION_KEY_ROOT_MIDI: i32 = 57;

/// Build the distant menu loop: a sparse, low-register A-minor motif under a wide, gently
/// shifting bed of filtered wind and shoreline hiss. The deliberately imperfect noise keeps the
/// menu from feeling like a sterile synth pad, while the long attacks and soft stereo movement
/// make the music feel like it is arriving from far down the beach.
pub fn synth_intro_menu(ctx: &mut Context) -> GameResult<Source> {
    const LOOP_SECONDS: f32 = 8.0;
    let n = (SAMPLE_RATE as f32 * LOOP_SECONDS) as usize;
    let dt = 1.0 / SAMPLE_RATE as f32;
    let mut left = vec![0.0_f32; n];
    let mut right = vec![0.0_f32; n];
    // A minor colour: A3, C4, E4, then D4 as a gentle unresolved turn.
    let notes = [220.0_f32, 261.63, 329.63, 293.66];
    // Fixed non-zero seed keeps the shoreline texture deterministic across launches.
    let mut noise_state = 0x51EA_BEEFu32;
    let mut wind_l = 0.0_f32;
    let mut wind_r = 0.0_f32;

    for i in 0..n {
        let t = i as f32 * dt;
        let phrase = (t / 2.0).floor() as usize;
        let phrase_t = t % 2.0;
        let note = notes[phrase % notes.len()];
        let fade_in = (phrase_t / 0.55).min(1.0);
        let fade_out = ((2.0 - phrase_t) / 0.7).min(1.0);
        let env = fade_in * fade_out;
        let phase = std::f32::consts::TAU * note * t;
        let sub = std::f32::consts::TAU * (note * 0.5) * t;
        let music = (phase.sin() * 0.72 + sub.sin() * 0.28) * env * 0.11;

        // A slow one-pole low-pass turns deterministic noise into a soft, irregular sea breeze.
        noise_state ^= noise_state << 13;
        noise_state ^= noise_state >> 17;
        noise_state ^= noise_state << 5;
        let raw = noise_state as f32 / u32::MAX as f32 * 2.0 - 1.0;
        wind_l += (raw - wind_l) * 0.0018;
        wind_r += (raw * 0.73 - wind_r) * 0.0015;
        let gust = 0.72 + 0.28 * (std::f32::consts::TAU * 0.11 * t).sin();
        let hiss = raw * 0.004 * (std::f32::consts::TAU * 0.37 * t).sin().abs();
        let ambience_l = (wind_l * 0.07 + hiss) * gust;
        let ambience_r = (wind_r * 0.07 + hiss * 0.8) * gust;

        // Slow, opposing movement keeps the image broad without sounding like a hard pan.
        let pan = (std::f32::consts::TAU * 0.045 * t).sin() * 0.72;
        let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
        left[i] = ambience_l + music * angle.cos();
        right[i] = ambience_r + music * angle.sin();
    }

    // A quiet cross-channel reflection suggests a distant beach wall without an obvious echo and
    // prevents the dry motif from sitting directly between the speakers.
    let delay = (0.19 * SAMPLE_RATE as f32) as usize;
    for i in delay..n {
        left[i] += right[i - delay] * 0.12;
        right[i] += left[i - delay] * 0.12;
    }
    let wav = encode_wav_stereo16(&left, &right);
    let data = SoundData::from_bytes(&wav)?;
    let mut src = Source::from_data(ctx, data)?;
    src.set_repeat(true);
    src.set_volume(0.34);
    Ok(src)
}

// ---------------------------------------------------------------------------
// Crab-theme melody synthesiser
//
// Game Boy / Deus Ex aesthetic:
//   - Arpeggio-driven harmony: fast 16th-note arpeggios cycle through chord tones instead of
//     held notes, giving the GB "shimmer" (Pokémon Red, Link's Awakening).
//   - Two-voice interplay: pulse channel 1 carries the fast arpeggio riff; pulse channel 2
//     answers with a slower counter-melody a bar later, like the two pulse channels on a DMG.
//   - Pulse-wave character: Rect(0.125) duty = buzzy/bright. Counter-voice uses Rect(0.5).
//   - Deus Ex feel: minor/Phrygian modes, slow-moving bass beneath the arpeggio, a sense of
//     unease. Alexander Brandon vibe: hypnotises rather than excites.
//   - Strict grid: all durations are exact 16th-note multiples; velocity/amplitude nudges give
//     staccato pulse feel (notes rendered slightly shorter than their grid slot).
// ---------------------------------------------------------------------------

// Pre-computed equal-temperament frequencies
const C3: f32 = 130.81;
const D3: f32 = 146.83;
const DS3: f32 = 155.56;
const E3: f32 = 164.81;
const F3: f32 = 174.61;
const G3: f32 = 196.00;
const A3: f32 = 220.00;
const AS3: f32 = 233.08;
const B3: f32 = 246.94;
const C4: f32 = 261.63;
const D4: f32 = 293.66;
const E4: f32 = 329.63;
const F4: f32 = 349.23;
const FS4: f32 = 369.99;
const G4: f32 = 392.00;
const A4: f32 = 440.00;
const AS4: f32 = 466.16;
const B4: f32 = 493.88;
const C5: f32 = 523.25;
const D5: f32 = 587.33;
const E5: f32 = 659.25;
const F5: f32 = 698.46;
const FS5: f32 = 739.99;
const G5: f32 = 783.99;
const A5: f32 = 880.00;
const R: f32 = 0.0; // rest

/// Build a two-voice GB-style theme and return it as a looping `Source`.
///
/// `voice1` = fast arpeggio riff on pulse channel 1 (duty 0.125, bright/buzzy).
/// `voice2` = slower counter-melody on pulse channel 2 (duty 0.5, softer square), mixed at
/// a slightly lower level so it sits behind the main riff.
/// Both sequences are `(hz, 16th_note_count)` pairs; `sixteenth_s` is the duration of one 16th note.
fn synth_two_voice(
    ctx: &mut Context,
    sixteenth_s: f32,
    voice1: &[(f32, u32)],  // (hz, 16ths) — arpeggio riff, Rect(0.125)
    voice2: &[(f32, u32)],  // (hz, 16ths) — counter-melody, Rect(0.5)
    amp1: f32,
    amp2: f32,
) -> GameResult<Source> {
    // Render voice 1 (arpeggio).
    let mut ch1: Vec<f32> = Vec::new();
    for &(hz, n16) in voice1 {
        ch1.extend(gb_pulse_note(hz, sixteenth_s * n16 as f32, 0.125, amp1));
    }
    // Render voice 2 (counter-melody).
    let mut ch2: Vec<f32> = Vec::new();
    for &(hz, n16) in voice2 {
        ch2.extend(gb_pulse_note(hz, sixteenth_s * n16 as f32, 0.5, amp2));
    }
    // Mix: extend shorter voice with silence so they align, then sum.
    let len = ch1.len().max(ch2.len());
    ch1.resize(len, 0.0);
    ch2.resize(len, 0.0);
    let mut mix: Vec<f32> = ch1.iter().zip(ch2.iter()).map(|(a, b)| a + b).collect();
    // Mild bitcrush for GB grit, then normalize.
    bitcrush(&mut mix, 8, 2);
    normalize_and_saturate(&mut mix, 0.82);
    let pcm: Vec<i16> = mix
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect();
    let wav = encode_wav_mono16(&pcm);
    let data = SoundData::from_bytes(&wav)?;
    let mut src = Source::from_data(ctx, data)?;
    src.set_repeat(true);
    Ok(src)
}

/// Theme 0 — "Pallet Town Crab": C major, 160 BPM.
///
/// Voice 1: fast 16th-note arpeggio shimmer cycling C–E–G–C (classic GB Pokémon Red shimmer).
/// Voice 2: slower quarter-note counter-melody descending back down the triad.
/// Feels like an upbeat town theme — bouncy, bright, immediately earwormy.
pub fn synth_theme_duck_bounce(ctx: &mut Context) -> GameResult<Source> {
    // 160 BPM → quarter = 375 ms → 16th = 93.75 ms
    let s = 60.0 / 160.0 / 4.0;
    #[rustfmt::skip]
    // Voice 1: arpeggio riff — 16ths cycling C–E–G (two bars × 2 = 4 bars total, ABA feel)
    let v1: &[(f32, u32)] = &[
        // bar 1 A: C maj arpeggio up and back
        (C5,1),(E5,1),(G5,1),(C5,1), (G5,1),(E5,1),(C5,1),(G4,1),
        // bar 2 A: same riff, slightly displaced
        (E5,1),(G5,1),(C5,1),(E5,1), (C5,1),(G4,1),(E4,1),(C4,1),
        // bar 3 B: F maj arpeggio — colour change
        (F4,1),(A4,1),(C5,1),(F5,1), (C5,1),(A4,1),(F4,1),(C4,1),
        // bar 4 B: G dominant — tension before return
        (G4,1),(B4,1),(D5,1),(G5,1), (D5,1),(B4,1),(G4,1),(D4,1),
        // bar 5–6 A repeat
        (C5,1),(E5,1),(G5,1),(C5,1), (G5,1),(E5,1),(C5,1),(G4,1),
        (E5,1),(G5,1),(C5,1),(E5,1), (C5,1),(G4,1),(E4,1),(C4,2),
    ];
    #[rustfmt::skip]
    // Voice 2: quarter-note counter-melody (each entry = 4 sixteenths = one quarter)
    let v2: &[(f32, u32)] = &[
        // bars 1-2: descending reply
        (G4,4),(E4,4),(C4,4),(D4,4),
        // bars 3-4: answering phrase
        (F4,4),(E4,4),(D4,4),(G3,4),
        // bars 5-6: return + cadence
        (G4,4),(E4,4),(C4,4),(C4,6),
    ];
    synth_two_voice(ctx, s, v1, v2, 0.55, 0.35)
}

/// Theme 1 — "Corridor Funk": D Dorian, 148 BPM.
///
/// Dorian gives the minor feel with a bright 6th (B natural in D Dorian) — feels tense but
/// groovy. Voice 1: syncopated 16th arpeggio. Voice 2: sparse bass-register counter-line.
pub fn synth_theme_duck_funky(ctx: &mut Context) -> GameResult<Source> {
    let s = 60.0 / 148.0 / 4.0;
    #[rustfmt::skip]
    // Voice 1: D–F–A (D minor triad) 16th arpeggios with rhythmic rests for syncopation
    let v1: &[(f32, u32)] = &[
        // bar 1: riff with rest on beat 2 for syncopation
        (D5,1),(F5,1),(A5,1),(R,1), (D5,1),(A4,1),(F4,1),(D4,1),
        // bar 2: answer riff rising
        (R,1),(F4,1),(A4,1),(D5,1), (F5,1),(A5,1),(F5,1),(D5,1),
        // bar 3 B: G minor colour
        (G4,1),(AS4,1),(D5,1),(G5,1), (D5,1),(AS4,1),(G4,1),(R,1),
        // bar 4 B: descend via A (natural 6th = bright Dorian colour)
        (A4,1),(C5,1),(E5,1),(A5,1), (E5,1),(C5,1),(A4,1),(G4,1),
        // bars 5-6: A repeat with held cadence
        (D5,1),(F5,1),(A5,1),(R,1), (D5,1),(A4,1),(F4,1),(D4,1),
        (R,1),(F4,1),(A4,1),(D5,1), (F5,1),(D5,1),(A4,2),
    ];
    #[rustfmt::skip]
    // Voice 2: slow bass line, half-note pulse (8 sixteenths each)
    let v2: &[(f32, u32)] = &[
        (D3,8),(G3,8),(AS3,8),(A3,6),(D3,2),
        (D3,8),(G3,6),(D3,4),(A3,8),
    ];
    synth_two_voice(ctx, s, v1, v2, 0.52, 0.42)
}

/// Theme 2 — "UNATCO Corridor": E Phrygian, 92 BPM, Deus Ex tense darkness.
///
/// Phrygian mode (E–F–G–A–B–C–D) = semitone above root gives the iconic Spanish/dark-minor
/// tension. Voice 1: arpeggio with deliberate stabs (lots of rests). Voice 2: slow
/// chromatic descent answering one bar late. Hypnotises rather than excites.
pub fn synth_theme_deus_tense(ctx: &mut Context) -> GameResult<Source> {
    let s = 60.0 / 92.0 / 4.0;
    #[rustfmt::skip]
    // Voice 1: E minor arpeggio (E–G–B) with plenty of air — less frantic than the upbeat themes
    let v1: &[(f32, u32)] = &[
        // bar 1 A: stab pattern, arpeggio on beat 1 then silence
        (E4,1),(G4,1),(B4,1),(E5,1), (R,2),(B4,1),(G4,1),
        // bar 2 A: half-bar descent
        (E4,1),(F4,1),(E4,1),(R,1), (B3,1),(G3,1),(E3,2),
        // bar 3 B: chromatic tension — move to C (bVI) colour
        (C4,1),(E4,1),(G4,1),(C5,1), (G4,1),(E4,1),(C4,2),
        // bar 4 B: D7 (bVII dominant) resolving back
        (D4,1),(FS4,1),(A4,1),(D5,1), (A4,1),(FS4,1),(D4,1),(R,1),
        // bars 5-6 A: return + long resolution
        (E4,1),(G4,1),(B4,1),(E5,1), (R,2),(B4,1),(G4,1),
        (E4,1),(F4,1),(E4,1),(R,1), (B3,2),(E3,4),
    ];
    #[rustfmt::skip]
    // Voice 2: slow chromatic descent — the Alexander Brandon "unease bass"
    let v2: &[(f32, u32)] = &[
        (E3,8),             // hold root
        (DS3,4),(D3,4),     // semitone steps down
        (C3,8),             // bVI bass
        (B3,4),(AS3,4),     // more descent
        (E3,6),(R,2),       // return with breath
        (E3,8),             // final hold
    ];
    synth_two_voice(ctx, s, v1, v2, 0.58, 0.45)
}

/// Theme 3 — "Biomechanical Hum": A Aeolian (natural minor), 78 BPM.
///
/// Slower and more ambient: voice 1 plays a sparse minor arpeggio with long rests between
/// phrases (Link's Awakening dungeon pacing). Voice 2 answers with a slow two-note motif
/// that hangs in the air, giving a sense of distant, patient unease.
pub fn synth_theme_deus_ambient(ctx: &mut Context) -> GameResult<Source> {
    let s = 60.0 / 78.0 / 4.0;
    #[rustfmt::skip]
    // Voice 1: sparse A minor (A–C–E) — lots of rests let each phrase breathe
    let v1: &[(f32, u32)] = &[
        // bar 1 A: just the upward statement
        (A4,1),(C5,1),(E5,1),(A5,1), (R,4),(E5,2),(C5,2),
        // bar 2 A: answer phrase descending
        (A4,1),(E4,1),(C4,1),(A3,1), (R,4),(A3,4),
        // bar 3 B: G minor colour (bVII)
        (G4,1),(AS4,1),(D5,1),(G5,1), (R,4),(D5,4),
        // bar 4 B: F major (bVI) — the Deus Ex "unease" chord
        (F4,1),(A4,1),(C5,1),(F5,1), (R,6),(C5,1),(A4,1),
        // bars 5-6 A: repeat with held close
        (A4,1),(C5,1),(E5,1),(A5,1), (R,4),(E5,2),(C5,2),
        (A4,2),(E4,2),(A3,8),
    ];
    #[rustfmt::skip]
    // Voice 2: two-note call-response at half-note pace, mostly low register
    let v2: &[(f32, u32)] = &[
        (A3,8),(R,8),
        (G3,8),(R,8),
        (F3,8),(R,8),
        (A3,10),(R,6),
    ];
    synth_two_voice(ctx, s, v1, v2, 0.48, 0.40)
}

/// Theme 4 — "Golden Pentatonic": G major pentatonic, 152 BPM.
///
/// Pentatonic avoids dissonance entirely — pure shimmer. Voice 1: 16th-note pentatonic
/// arpeggio that never stops (Tetris/Pokémon title-screen energy). Voice 2: a short
/// motivic cell (3-note tag) that pops in every other bar as the counter-voice.
pub fn synth_theme_duck_golden(ctx: &mut Context) -> GameResult<Source> {
    let s = 60.0 / 152.0 / 4.0;
    #[rustfmt::skip]
    // Voice 1: G pentatonic (G–A–B–D–E) non-stop shimmer
    let v1: &[(f32, u32)] = &[
        // bar 1 A: up the pentatonic
        (G4,1),(A4,1),(B4,1),(D5,1), (E5,1),(D5,1),(B4,1),(A4,1),
        // bar 2 A: up again, hit the octave top
        (G5,1),(E5,1),(D5,1),(B4,1), (A4,1),(G4,1),(D4,1),(G4,1),
        // bar 3 B: D major arpeggio (V chord, tension)
        (D5,1),(FS5,1),(A5,1),(D5,1), (A4,1),(FS4,1),(D4,1),(A3,1),
        // bar 4 B: resolve back via E minor
        (E5,1),(D5,1),(B4,1),(G4,1), (E4,1),(G4,1),(B4,1),(E5,1),
        // bars 5-6 A: full repeat with longer cadence hold
        (G4,1),(A4,1),(B4,1),(D5,1), (E5,1),(D5,1),(B4,1),(A4,1),
        (G5,1),(E5,1),(D5,1),(B4,1), (G4,2),(D4,2),(G4,4),
    ];
    #[rustfmt::skip]
    // Voice 2: 3-note motivic tag — appears every other bar (padded with rests)
    let v2: &[(f32, u32)] = &[
        (R,8),                          // bar 1: silent
        (D5,2),(E5,2),(G5,4),           // bar 2: tag
        (R,8),                          // bar 3: silent
        (B4,2),(A4,2),(G4,4),           // bar 4: answer tag (lower)
        (R,8),                          // bar 5: silent
        (D5,2),(G5,2),(R,4),            // bar 6: final punctuation
    ];
    synth_two_voice(ctx, s, v1, v2, 0.55, 0.38)
}

// ---------------------------------------------------------------------------
// Generative GROOVE engine  (scale + riff + swing + bass/melody + build)
//
// The two-voice themes above are hand-written GB arpeggios — nice, but fixed.
// This engine *generates* a groove from musical rules so the loop the player
// actually hears (see `synth_action_groove`, wired into `action_music`) has a
// real feel rather than reading as a fixed backing track:
//
//   * Notes come from a named SCALE (pentatonic / blues / dorian), so nothing
//     ever sounds "wrong".
//   * A short MOTIF is the riff; it REPEATS across a phrase with small
//     deterministic VARIATIONS (neighbour-note substitution, octave lift, ghost
//     notes) so the riff evolves instead of looping identically.
//   * Onsets are quantised to a 1/16 BEAT GRID with SWING (odd 1/16s land late)
//     for a shuffle feel — syncopation is intentional, tied to the grid.
//   * CALL-AND-RESPONSE: two-bar phrases — a "question" motif then an "answer"
//     that inverts the contour and resolves onto the root, held long.
//   * LAYERING: a sparse triangle BASS plays root then fifth on the downbeats
//     under the busier square LEAD.
//   * DYNAMIC BUILD: ghost-note density rises across the phrase then resets, so
//     each phrase breathes — sparse start, dense finish.
//
// A deterministic xorshift seed makes each groove reproducible build-to-build;
// the randomness only ever chooses *between musical options*.
// ---------------------------------------------------------------------------

/// A musical scale as semitone offsets from the root across one octave.
#[derive(Clone, Copy)]
enum GrooveScale {
    /// Minor pentatonic — the workhorse: no avoid-notes, always consonant.
    PentatonicMinor,
    /// Blues — minor pentatonic plus the flat-5 "blue note" for grit.
    Blues,
    /// Dorian — minor with a raised 6th, funky and hopeful.
    Dorian,
    /// Major pentatonic — bright and sparkly.
    PentatonicMajor,
}

impl GrooveScale {
    fn degrees(self) -> &'static [i32] {
        match self {
            GrooveScale::PentatonicMinor => &[0, 3, 5, 7, 10],
            GrooveScale::Blues => &[0, 3, 5, 6, 7, 10],
            GrooveScale::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            GrooveScale::PentatonicMajor => &[0, 2, 4, 7, 9],
        }
    }
}

/// Equal-temperament frequency for a MIDI note number (69 = A4 = 440 Hz).
fn groove_midi_to_hz(midi: i32) -> f32 {
    440.0 * 2.0_f32.powf((midi as f32 - 69.0) / 12.0)
}

/// Map a scale *degree index* (0 = root; may be negative or beyond the octave) to a
/// MIDI note by wrapping through the scale and adding octaves — lets a riff walk the
/// scale smoothly up and down without ever leaving it.
fn groove_degree_to_midi(scale: GrooveScale, root_midi: i32, degree: i32) -> i32 {
    let steps = scale.degrees();
    let n = steps.len() as i32;
    let octave = degree.div_euclid(n);
    let idx = degree.rem_euclid(n) as usize;
    root_midi + octave * 12 + steps[idx]
}

/// Deterministic xorshift32 PRNG — reproducible per seed. Used only to choose
/// between musical options (which degree, whether to add a ghost note, etc.).
struct GrooveRng(u32);
impl GrooveRng {
    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    fn f01(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u32() as usize) % n.max(1)
    }
    fn chance(&mut self, p: f32) -> bool {
        self.f01() < p
    }
}

/// Which voice renders a scheduled note — sets its timbre and register.
#[derive(Clone, Copy, PartialEq)]
enum GrooveVoice {
    /// Lead: the singable hook, electric-piano voice, mid/high register.
    Lead,
    /// Bass: root-and-fifth line an octave down, warm triangle.
    Bass,
    /// Pad: soft sustained chord tones under the lead, triangle, low gain.
    Pad,
}

/// One scheduled note on the 1/16 grid.
struct GrooveNote {
    step: u32, // start position in 1/16 units
    len: u32,  // length in 1/16 units
    /// Pitch relative to the root. If `chromatic`, this is an absolute SEMITONE offset from
    /// `root_midi` (used by bass/pads that outline the chord progression, which leaves the scale);
    /// otherwise it's a SCALE-DEGREE index into the pentatonic (used by the melody hook, which
    /// stays in-key and so sounds consonant over every chord).
    degree: i32,
    chromatic: bool,
    voice: GrooveVoice,
    gain: f32,
}

/// Render one voice note with a tight percussive envelope so onsets land crisply
/// on the beat, with a short release tail so consecutive notes stay legato.
fn groove_voice_note(hz: f32, dur_s: f32, waveform: Waveform, gain: f32) -> Vec<f32> {
    let adsr = Adsr {
        attack: 0.004,
        decay: 0.06,
        sustain: 0.55,
        release: 0.09,
    };
    let hold = (dur_s * 0.85).max(0.02); // notes breathe but stay connected
    synth_note(waveform, hz, hold, &adsr, gain)
}

/// A warm Rhodes/DX7-style electric-piano voice for the groove lead — the antidote to the
/// Game Boy square wave. Additive rather than true FM: a sine fundamental carries the round,
/// warm body, and a 2× sine harmonic with its own fast, sustain-to-silence envelope supplies
/// the percussive "tine" clang at the attack that settles away, leaving the pure sine behind.
/// The result reads as Herbie Hancock "Chameleon" rather than a chiptune blip.
fn synth_ep_note(hz: f32, dur_s: f32, gain: f32) -> Vec<f32> {
    let hold = (dur_s * 0.85).max(0.02); // match the existing groove voice's note breathing

    // Fundamental: warm round body, medium decay into a moderate sustain, natural release.
    let body_adsr = Adsr {
        attack: 0.005,
        decay: 0.12,
        sustain: 0.38,
        release: 0.20,
    };
    // A sine's RMS is ~0.71× its amplitude, versus ~1.0× for the square this replaces, so a
    // straight 0.8 gain would sit the lead ~5 dB quieter than the old chiptune lead and bury it
    // under the (unchanged) bass. Push the fundamental to 1.1 to recover roughly the square's
    // perceived loudness; the master limiter still catches the combined peak.
    let mut out = synth_note(Waveform::Sine, hz, hold, &body_adsr, gain * 1.1);

    // 2× harmonic "tine": a pure percussive transient that decays to silence (sustain 0.0),
    // giving the signature Rhodes "tink" without lingering as a steady overtone.
    let tine_adsr = Adsr {
        attack: 0.002,
        decay: 0.08,
        sustain: 0.08,
        release: 0.08,
    };
    let tine = synth_note(Waveform::Sine, hz * 2.0, hold, &tine_adsr, gain * 0.45);

    mix_into(&mut out, &tine, 0);
    out
}

/// A punchy 808-style kick: a sine whose pitch sweeps down from ~120 Hz to a 40 Hz
/// floor under a fast exponential envelope, giving the "thump" without a click.
fn render_kick(mix: &mut Vec<f32>, offset: usize, gain: f32) {
    let sr = SAMPLE_RATE as f32;
    let dur = (sr * 0.12) as usize;
    for k in 0..dur.min(mix.len().saturating_sub(offset)) {
        let t = k as f32 / sr;
        let env = (-t * 40.0_f32).exp();
        let freq = 80.0 * (-t * 25.0_f32).exp() + 40.0;
        let sample = (std::f32::consts::TAU * freq * t).sin() * env * gain;
        mix[offset + k] += sample;
    }
}

/// A dry snare: filtered-free white noise (from the groove RNG) under a very fast
/// exponential decay. Short and bright so the backbeat cuts through the mix.
fn render_snare(mix: &mut Vec<f32>, offset: usize, gain: f32, rng: &mut GrooveRng) {
    let sr = SAMPLE_RATE as f32;
    let dur = (sr * 0.08) as usize;
    for k in 0..dur.min(mix.len().saturating_sub(offset)) {
        let t = k as f32 / sr;
        let env = (-t * 50.0_f32).exp();
        let noise = rng.f01() * 2.0 - 1.0; // xorshift white noise in [-1, 1]
        mix[offset + k] += noise * env * gain;
    }
}

/// Build a repeating call-and-response groove and render it to a looping Source.
/// `bpm` sets tempo; `swing` (0..1) is how late odd 1/16 steps land; `bars` is the
/// phrase length (even numbers alternate question/answer bars).
#[allow(clippy::too_many_arguments)]
fn synth_groove(
    ctx: &mut Context,
    seed: u32,
    scale: GrooveScale,
    root_midi: i32,
    bpm: f32,
    swing: f32,
    bars: u32,
    melody_gain: f32,
    bit_depth: u32,
) -> GameResult<Source> {
    let mut rng = GrooveRng(seed | 1);

    let beat_s = 60.0 / bpm;
    let step_s = beat_s / 4.0; // 1/16-note grid
    let steps_per_bar = 16u32;

    // --- Chord progression: i – VI – III – VII (Am – F – C – G in A minor). ------------------
    // One chord per bar, cycling every four bars. This is the anthemic "four-chord" loop the ear
    // already knows, and — crucially — A-minor-pentatonic sits consonant over ALL FOUR chords, so
    // the melody hook below never has to move to stay in tune. The harmony moves underneath it
    // (bass roots + pad stabs) while the tune stays put: that is what makes the loop sound
    // *composed* rather than like a scale exercise, and what lets you HUM it after one pass.
    // Semitone offsets from the root (A): Am 0, F -4, C +3, G -2 — a smooth low bassline.
    let chord_root_semi: [i32; 4] = [0, -4, 3, -2];
    // Chord thirds relative to each chord root: Am is minor (+3), F/C/G are major (+4).
    let chord_third_semi: [i32; 4] = [3, 4, 4, 4];

    // --- The signature HOOK (the "question"): a fixed, singable one-bar riff. -----------------
    // Pentatonic scale-DEGREE indices (0=A root, 1=C, 2=D, 3=E/the fifth, 4=G, 5=A octave). It
    // rises to the octave on beat 3 — the hook's peak, its identity — then turns and resolves back
    // to the root on the "and" of 4, handing cleanly into the next bar. Identical every question
    // bar, so it lodges in the ear. `(step, degree, len)` on the 1/16 grid.
    let question: [(u32, i32, u32); 9] = [
        (0, 0, 2),  // beat 1   : A  (root — the anchor)
        (3, 2, 1),  // a-of-1   : D  (syncopated pickup)
        (4, 3, 2),  // beat 2   : E  (the fifth)
        (6, 4, 2),  // and-of-2 : G  (reach for the b7)
        (8, 5, 2),  // beat 3   : A' (octave — the signature peak)
        (10, 4, 1), // e-of-3   : G
        (11, 3, 1), // a-of-3   : E  (turn)
        (12, 2, 2), // beat 4   : D  (descending)
        (14, 0, 2), // and-of-4 : A  (resolve, sets up the loop)
    ];
    // The "answer": a complementary lower phrase that settles, so the two-bar unit reads as
    // call-and-response and lands home on a held root.
    let answer: [(u32, i32, u32); 9] = [
        (0, 0, 2),   // A
        (3, -1, 1),  // G (below the root — the answer sits lower)
        (4, 0, 2),   // A
        (6, 2, 2),   // D
        (8, 3, 2),   // beat 3 : E (a calmer peak than the octave)
        (10, 2, 1),  // D
        (11, 0, 1),  // A
        (12, -1, 1), // G below
        (13, 0, 3),  // A — held resolution into the next phrase
    ];

    // --- Assemble the full phrase: hook + answer bars, with the harmony (bass + pad) following
    // the chord progression and a build that layers the pad and ghost notes in across the loop,
    // so each 8-bar pass breathes — sparse intro, full-band peak, then a fill turns it around. ---
    let mut notes: Vec<GrooveNote> = Vec::new();
    for bar in 0..bars {
        let call = bar % 2 == 0;
        let motif: &[(u32, i32, u32)] = if call { &question } else { &answer };
        let build = bar as f32 / bars.max(1) as f32; // 0..1 across the phrase
        let chord = (bar % 4) as usize;
        let bar_start = bar * steps_per_bar;

        // Melody: the fixed hook, note-for-note (no pitch mutation — a stable riff is a hummable
        // riff). The downbeat of every question bar is the phrase anchor: root, full gain.
        for (i, &(st, deg, len)) in motif.iter().enumerate() {
            let phrase_anchor = call && i == 0 && st == 0;
            let note_gain = if phrase_anchor { 1.0 } else { melody_gain };
            notes.push(GrooveNote {
                step: bar_start + st,
                len,
                degree: deg,
                chromatic: false,
                voice: GrooveVoice::Lead,
                gain: note_gain,
            });
            // Ghost note — a quiet extra 1/16 that thickens the pocket as the phrase builds.
            // Sparse at the top of the loop, denser toward the peak, so density is itself a
            // dynamic (the RNG only ever adds a consonant neighbour, never a wrong note).
            if !phrase_anchor && st + len < steps_per_bar && rng.chance(0.02 + 0.4 * build) {
                notes.push(GrooveNote {
                    step: bar_start + st + len,
                    len: 1,
                    degree: deg - 1,
                    chromatic: false,
                    voice: GrooveVoice::Lead,
                    gain: melody_gain * 0.5,
                });
            }
        }

        // Bass: outlines THIS bar's chord — root on beats 1 & 3, fifth on 2 & 4, an octave below
        // the root register. This is what turns "one scale" into "a chord progression": the low
        // end spells Am → F → C → G under the unchanging tune.
        let root = chord_root_semi[chord];
        let bass_pat: [i32; 4] = [root, root + 7, root, root + 7]; // root, 5th, root, 5th
        for (j, &semi) in bass_pat.iter().enumerate() {
            notes.push(GrooveNote {
                step: bar_start + j as u32 * 4,
                len: 3,             // slightly detached for bounce
                degree: -12 + semi, // one octave down, absolute semitones
                chromatic: true,
                voice: GrooveVoice::Bass,
                gain: melody_gain * 0.85,
            });
        }

        // Pad: a soft sustained triad (root + third + fifth) on every bar. Starting the harmony
        // on bar one makes the hook read as a tune over a satisfying progression immediately,
        // rather than as a thin lead that only finds its key two bars later.
        let third = chord_third_semi[chord];
        for &semi in &[root, root + third, root + 7] {
            notes.push(GrooveNote {
                step: bar_start,
                len: 14,      // sustain, re-struck each bar
                degree: semi, // absolute semitones from the root register
                chromatic: true,
                voice: GrooveVoice::Pad,
                gain: melody_gain * 0.27,
            });
        }
    }

    // --- Render every note onto the mix bus at its swung onset time. ---
    let total_steps = bars * steps_per_bar;
    let loop_samples = (total_steps as f32 * step_s * SAMPLE_RATE as f32) as usize;
    let mut mix: Vec<f32> = vec![0.0; loop_samples];

    for note in &notes {
        // Swing: push odd 1/16 steps late by up to half a step × swing.
        let swing_offset = if note.step % 2 == 1 {
            swing * 0.5 * step_s
        } else {
            0.0
        };
        let start_s = note.step as f32 * step_s + swing_offset;
        let dur_s = note.len as f32 * step_s;
        // Melody rides the pentatonic scale (always in-key); bass/pad use absolute semitones so
        // they can spell chord roots/thirds that leave the scale (F, G major) under the tune.
        let midi = if note.chromatic {
            root_midi + note.degree
        } else {
            groove_degree_to_midi(scale, root_midi, note.degree)
        };
        let hz = groove_midi_to_hz(midi);
        let rendered = match note.voice {
            // Lead sings through the warm electric-piano voice; bass and pad are triangle beds
            // (the pad just sits lower-gain and holds far longer, so it reads as sustained chord).
            GrooveVoice::Lead => synth_ep_note(hz, dur_s, note.gain),
            GrooveVoice::Bass | GrooveVoice::Pad => {
                groove_voice_note(hz, dur_s, Waveform::Triangle, note.gain)
            }
        };
        let offset = (start_s * SAMPLE_RATE as f32) as usize;
        mix_into(&mut mix, &rendered, offset);
    }

    // --- Drum pass: a straight backbeat under the melody/bass. Kick on beats 1 & 3
    // (steps 0, 8), snare on beats 2 & 4 (steps 4, 12) of every bar. All land on even
    // steps, so no swing offset applies. Rendered after the melody loop so melody
    // determinism is untouched; the snare's noise consumes RNG here at the very end.
    for bar in 0..bars {
        let bar_start = bar * steps_per_bar;
        let step_offset =
            |st: u32| -> usize { (st as f32 * step_s * SAMPLE_RATE as f32) as usize };
        render_kick(&mut mix, step_offset(bar_start), melody_gain);
        render_kick(&mut mix, step_offset(bar_start + 8), melody_gain);
        render_snare(&mut mix, step_offset(bar_start + 4), melody_gain * 0.8, &mut rng);
        render_snare(&mut mix, step_offset(bar_start + 12), melody_gain * 0.8, &mut rng);
        // Turnaround FILL on the final bar: a snare roll accelerating into the loop point (steps
        // 10, 13, 14, 15), a rising tension that resolves on the downbeat when the phrase restarts.
        // This connects each loop to the next instead of butting two identical bars together —
        // the "fill on the transition" that makes a repeating loop feel like a song coming around.
        if bar + 1 == bars {
            for &st in &[10u32, 13, 14, 15] {
                render_snare(&mut mix, step_offset(bar_start + st), melody_gain * 0.6, &mut rng);
            }
        }
    }

    // Fold release tails into the next phrase, then retain exactly the requested number
    // of bars. Otherwise each loop includes its tails after the final downbeat and drifts.
    if mix.len() > loop_samples {
        for i in loop_samples..mix.len() {
            mix[i - loop_samples] += mix[i];
        }
    }
    mix.truncate(loop_samples);

    // Glue the layered voices and bring up to clean full loudness.
    compress(&mut mix, 0.5, 3.0, 0.005, 0.08);
    master_limiter(&mut mix);

    let pcm = samples_to_pcm(&mut mix, bit_depth, 1);
    let wav = encode_wav_mono16(&pcm);
    let data = SoundData::from_bytes(&wav)?;
    let mut src = Source::from_data(ctx, data)?;
    src.set_repeat(true);
    Ok(src)
}

/// The default in-game action groove — the loop the player hears while rustling.
/// A driving A-minor shuffle over an authored i–VI–III–VII chord progression
/// (Am–F–C–G): a fixed singable hook rides the harmony while the bass and pad
/// spell the chords underneath, with a build and a turnaround fill across the 8
/// bars so each pass feels composed rather than looped.
///
/// `bpm` MUST equal the gameplay beat grid's BASE tempo — `60.0 / BEAT_INTERVAL`,
/// the one source of truth the reset and the intensity ramp both key off — so the
/// groove loops in lock-step with the beats and the on-beat catch window. The ramp
/// speeds the grid up via `tempo_mul`; the music FOLLOWS it by re-pitching at
/// runtime (see `music_pitch` in `EventHandler::update`), never by baking a
/// different tempo here, which would drift. The BPM also seeds the RNG, so the
/// ghost-note variations are reproducible build-to-build.
pub fn synth_action_groove(ctx: &mut Context, bpm: f32) -> GameResult<Source> {
    synth_groove(
        ctx,
        0xC0FFEE ^ (bpm as u32),
        GrooveScale::PentatonicMinor,
        ACTION_KEY_ROOT_MIDI,
        bpm,
        GROOVE_SWING, // shuffle: odd 1/16s land noticeably late — shared with the live hi-hat kit
        8,
        0.56,
        // Bit depth was 6 (64 levels) — a Game Boy crush that turned the warm electric-piano
        // lead back into chiptune. Raised to 11 (2048 levels) so the EP's round body survives
        // to tape; the master limiter still glues the mix.
        11,
    )
}
