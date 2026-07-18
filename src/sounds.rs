//! Procedural audio synthesis — no external audio files.
//!
//! The rhythm is the whole game, but until now the beat was purely *visual* (flashes, pulses,
//! the stepping conga train). This module synthesises a kick drum at runtime and hands it to
//! ggez as an in-memory WAV, so every beat tick lands as a physical *thump* you feel as much as
//! see. Carl asked for the BPM to be visceral, not just visual — this is that.
//!
//! Why WAV bytes and not raw samples: ggez's `SoundData` feeds a `rodio::Decoder`, which expects
//! an encoded container (WAV/OGG/…), not a bare PCM buffer. So we generate 16-bit mono PCM and
//! wrap it in a canonical 44-byte WAV header. The `Source` is built once at startup from these
//! bytes and simply replayed (`play_detached`) on each beat — nothing is re-synthesised per frame.

use ggez::audio::{SoundData, SoundSource, Source};
use ggez::{Context, GameResult};

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

const SAMPLE_RATE: u32 = 44_100;

// ---------------------------------------------------------------------------------------------
// General-purpose synth engine: oscillators, ADSR, FM voices, and lo-fi retro FX.
//
// The kick/snare/hihat/rumble above are bespoke one-off percussion generators. This section adds
// reusable building blocks for *pitched* sounds — melodies, arpeggios, chimes — so future SFX
// (and the coin-collect chime below) don't need to hand-roll a phase accumulator every time.
// Everything here works in plain `f32` sample buffers (-1..1) so effects can be chained before
// the final 16-bit WAV encode.
// ---------------------------------------------------------------------------------------------

/// Basic oscillator shapes for the additive synth. Sine is smooth/pure, triangle is a softer
/// buzz, rectangle (square, with adjustable pulse width) is the classic hard 8-bit chip tone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Waveform {
    Sine,
    Triangle,
    /// Rectangle/pulse wave. The `f32` is pulse width (duty cycle), 0..1; 0.5 = square.
    Rect(f32),
}

/// Sample a bandlimited-enough (naive, but fine at these pitches/durations) oscillator at a given
/// phase, where `phase` is in cycles (0.0..1.0 repeating), not radians.
fn oscillator_sample(waveform: Waveform, phase: f32) -> f32 {
    let p = phase.rem_euclid(1.0);
    match waveform {
        Waveform::Sine => (std::f32::consts::TAU * p).sin(),
        // Triangle: linear ramp up 0..0.5, down 0.5..1, mapped to -1..1.
        Waveform::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
        Waveform::Rect(duty) => {
            if p < duty.clamp(0.01, 0.99) {
                1.0
            } else {
                -1.0
            }
        }
    }
}

/// Classic four-stage envelope: linear attack up to full amplitude, linear decay down to the
/// sustain level, hold at sustain, then linear release to silence. Times are in seconds;
/// `sustain` is a level 0..1, not a duration.
#[derive(Clone, Copy, Debug)]
pub struct Adsr {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Adsr {
    /// Amplitude (0..1) at time `t` seconds into a note that is held for `note_duration` seconds
    /// before release begins (so short-held notes still get a full release tail, e.g. `t` may run
    /// past `note_duration` — callers should render `note_duration + release` samples total).
    pub fn amplitude(&self, t: f32, note_duration: f32) -> f32 {
        if t < 0.0 {
            return 0.0;
        }
        if t < self.attack {
            if self.attack <= 0.0 {
                return 1.0;
            }
            return (t / self.attack).min(1.0);
        }
        let t_decay = t - self.attack;
        if t_decay < self.decay {
            if self.decay <= 0.0 {
                return self.sustain;
            }
            let frac = (t_decay / self.decay).min(1.0);
            return 1.0 + (self.sustain - 1.0) * frac;
        }
        if t < note_duration {
            return self.sustain;
        }
        // Release: fade from sustain to zero over `release` seconds.
        let t_release = t - note_duration;
        if self.release <= 0.0 || t_release >= self.release {
            return 0.0;
        }
        self.sustain * (1.0 - t_release / self.release)
    }

    /// Total length in seconds needed to render a note of `note_duration` fully to silence.
    pub fn total_duration(&self, note_duration: f32) -> f32 {
        note_duration + self.release
    }
}

/// Render a single additive-synth note (sine/triangle/rect) with an ADSR envelope into a raw
/// `-1..1` sample buffer. `freq` is constant across the note (no pitch glide) — this is the
/// plain melodic building block; layer several calls at different frequencies/waveforms and mix
/// for richer chords/timbres.
pub fn synth_note(
    waveform: Waveform,
    freq: f32,
    note_duration: f32,
    adsr: &Adsr,
    gain: f32,
) -> Vec<f32> {
    let total = adsr.total_duration(note_duration).max(0.0);
    let n_samples = (SAMPLE_RATE as f32 * total) as usize;
    let dt = 1.0 / SAMPLE_RATE as f32;
    let mut out = Vec::with_capacity(n_samples);
    let mut phase = 0.0_f32;
    for i in 0..n_samples {
        let t = i as f32 * dt;
        phase += freq * dt;
        phase = phase.rem_euclid(1.0); // Keep the accumulator bounded for long (pad-length) notes.
        let env = adsr.amplitude(t, note_duration);
        out.push(oscillator_sample(waveform, phase) * env * gain);
    }
    out
}

/// Render a single two-operator FM voice: a sine carrier phase-modulated by a sine modulator,
/// classic Chowning FM synthesis. The modulation index (how much the modulator's amplitude
/// distorts the carrier) decays independently and faster than the overall note envelope, which
/// is what gives DX7-style electric-piano/bell tones their characteristic bright "pluck" attack
/// that mellows into a purer tone — perfect for a crisp, high, fast "coin" ping.
///
/// * `carrier_hz` — the fundamental pitch.
/// * `mod_ratio` — modulator frequency as a multiple of the carrier (e.g. 2.0, 3.5, 7.0 all give
///   different bell/metallic characters; non-integer ratios sound more inharmonic/metallic).
/// * `mod_index` — peak modulation index (higher = brighter/more overtones at the attack).
/// * `mod_index_decay` — how fast the modulation index decays (per second, exponential); a large
///   value makes the "clang" collapse to a near-pure tone quickly, like a plucked string.
/// * `adsr` — overall amplitude envelope for the note.
pub fn synth_fm_note(
    carrier_hz: f32,
    mod_ratio: f32,
    mod_index: f32,
    mod_index_decay: f32,
    note_duration: f32,
    adsr: &Adsr,
    gain: f32,
) -> Vec<f32> {
    synth_fm_note_inner(
        carrier_hz,
        mod_ratio,
        mod_index,
        mod_index_decay,
        note_duration,
        adsr,
        gain,
        false,
    )
}

/// FM note variant with the short upward pitch bend used by NES-style hit-confirm sounds.
fn synth_fm_note_pitch_attack(
    carrier_hz: f32,
    mod_ratio: f32,
    mod_index: f32,
    mod_index_decay: f32,
    note_duration: f32,
    adsr: &Adsr,
    gain: f32,
) -> Vec<f32> {
    synth_fm_note_inner(
        carrier_hz,
        mod_ratio,
        mod_index,
        mod_index_decay,
        note_duration,
        adsr,
        gain,
        true,
    )
}

fn synth_fm_note_inner(
    carrier_hz: f32,
    mod_ratio: f32,
    mod_index: f32,
    mod_index_decay: f32,
    note_duration: f32,
    adsr: &Adsr,
    gain: f32,
    pitch_attack: bool,
) -> Vec<f32> {
    let total = adsr.total_duration(note_duration).max(0.0);
    let n_samples = (SAMPLE_RATE as f32 * total) as usize;
    let dt = 1.0 / SAMPLE_RATE as f32;
    let mut out = Vec::with_capacity(n_samples);
    let mut carrier_phase = 0.0_f32;
    let mut mod_phase = 0.0_f32;
    for i in 0..n_samples {
        let t = i as f32 * dt;
        // Start a catch-confirm note 10% sharp, settling to pitch in 30 ms.
        let pitch = if pitch_attack {
            1.0 + 0.1 * (1.0 - (t / 0.03).min(1.0))
        } else {
            1.0
        };
        mod_phase += carrier_hz * pitch * mod_ratio * dt;
        mod_phase = mod_phase.rem_euclid(1.0); // Bound the accumulator for long pad-length notes.
        // Modulation index decays exponentially from its peak so the "clang" settles fast.
        let idx = mod_index * (-mod_index_decay * t).exp();
        let modulator = (std::f32::consts::TAU * mod_phase).sin() * idx;
        carrier_phase += carrier_hz * pitch * dt;
        carrier_phase = carrier_phase.rem_euclid(1.0);
        let env = adsr.amplitude(t, note_duration);
        let sample = (std::f32::consts::TAU * carrier_phase + modulator).sin();
        out.push(sample * env * gain);
    }
    out
}

/// Mix a buffer into a destination at a given sample offset, extending `dest` as needed. Used to
/// layer/overlap notes (e.g. a fast arpeggio where consecutive notes slightly overlap) without
/// clipping the tail of the previous one.
fn mix_into(dest: &mut Vec<f32>, src: &[f32], offset_samples: usize) {
    let needed = offset_samples + src.len();
    if dest.len() < needed {
        dest.resize(needed, 0.0);
    }
    for (i, &s) in src.iter().enumerate() {
        dest[offset_samples + i] += s;
    }
}

/// Lo-fi "bitcrush" effect, 8/16-bit console style: quantizes amplitude to `bit_depth` bits and
/// holds each output sample for `sample_hold` input samples (sample-and-hold decimation, i.e. a
/// crude sample-rate reduction). Both together give that dirty, aliased retro chiptune crunch —
/// a `bit_depth` of 8 with `sample_hold` of 2-4 reads as distinctly "old console", while
/// `bit_depth` 16 / `sample_hold` 1 is a no-op (transparent passthrough).
fn bitcrush(samples: &mut [f32], bit_depth: u32, sample_hold: usize) {
    let levels = (1u32 << bit_depth.clamp(2, 16)) as f32;
    let half_levels = levels * 0.5;
    let hold = sample_hold.max(1);
    let mut held_value = 0.0_f32;
    for (i, s) in samples.iter_mut().enumerate() {
        if i % hold == 0 {
            // Quantize to `levels` steps across -1..1.
            held_value = (s.clamp(-1.0, 1.0) * half_levels).round() / half_levels;
        }
        *s = held_value;
    }
}

/// Lightweight mastering pass: peak-normalize to -1.5 dBFS then apply a tanh soft-knee
/// limiter so peaks never clip and simultaneous sounds stay controlled when played together.
/// Works on any f32 slice; stereo callers should pass both channels concatenated so the
/// gain decision is made on the combined peak (not per-channel, which would alter panning).
fn master_limiter(samples: &mut [f32]) {
    let peak = samples.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
    if peak < 1e-6 {
        return;
    }
    // Target -1.5 dBFS ≈ 0.841 — a hair below full-scale so the tanh knee has room.
    const TARGET: f32 = 0.841;
    let gain = TARGET / peak;
    for s in samples.iter_mut() {
        // Soft-knee via tanh: transparent below ±TARGET, smoothly compresses above.
        // tanh(gain·x) / tanh(gain) remaps so x=±peak → ±TARGET exactly.
        let drive = *s * gain;
        *s = drive.tanh() / gain.tanh() * TARGET;
    }
}

fn samples_to_pcm(samples: &mut [f32], bit_depth: u32, sample_hold: usize) -> Vec<i16> {
    bitcrush(samples, bit_depth, sample_hold);
    master_limiter(samples);
    samples
        .iter()
        .map(|&sample| (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect()
}

/// Deterministic 16-bit linear-feedback shift-register noise, suitable for a console noise
/// channel. The maximal-length taps make the sequence read as noise without allocating a buffer.
fn lfsr_noise(state: &mut u32) -> f32 {
    let bit = (*state ^ (*state >> 1)) & 1;
    *state = (*state >> 1) | (bit << 15);
    ((*state & 0xffff) as f32 / 32767.5) - 1.0
}

/// Simple feed-forward dynamics compressor with an envelope follower (separate attack/release
/// smoothing). Above `threshold`, gain is reduced by `ratio` (e.g. 4.0 = 4:1); below it, signal
/// passes untouched. Squashes the loudest transient peaks so a busy mix of layered notes stays
/// punchy without clipping, in the "loud but controlled" style of retro console audio (everything
/// slammed to feel bigger than it is).
fn compress(samples: &mut [f32], threshold: f32, ratio: f32, attack_s: f32, release_s: f32) {
    let dt = 1.0 / SAMPLE_RATE as f32;
    let attack_coeff = (-dt / attack_s.max(0.0001)).exp();
    let release_coeff = (-dt / release_s.max(0.0001)).exp();
    let mut envelope = 0.0_f32;
    for s in samples.iter_mut() {
        let input_level = s.abs();
        let coeff = if input_level > envelope {
            attack_coeff
        } else {
            release_coeff
        };
        envelope = coeff * envelope + (1.0 - coeff) * input_level;

        if envelope > threshold {
            // Linear amplitude ratio (not decibels) of how far the envelope sits above threshold.
            let over_threshold_ratio = envelope / threshold;
            let reduced_ratio = over_threshold_ratio.powf(1.0 / ratio - 1.0);
            *s *= reduced_ratio;
        }
    }
}

/// Normalize a buffer so its peak absolute sample hits `target_peak` (0..1), then soft-clip with
/// `tanh` for a touch of warm saturation. Run this last, after any compression, so the final
/// output uses the available headroom without harsh digital clipping.
fn normalize_and_saturate(samples: &mut [f32], target_peak: f32) {
    // Slight overdrive before the tanh soft-clip so the curve's knee rounds off the loudest
    // peaks a touch (warmth/drive), instead of leaving `tanh` almost linear near the target peak.
    const SATURATION_OVERDRIVE: f32 = 1.15;
    let peak = samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
    if peak > 0.0001 {
        let total_gain = (target_peak / peak) * SATURATION_OVERDRIVE;
        for s in samples.iter_mut() {
            *s = (*s * total_gain).tanh();
        }
    }
}

/// Synthesise a fast, bright arpeggio for "you caught something" feedback — e.g. a coin/crab
/// collection chime. Three short FM-bell notes climb a major triad in rapid succession (each
/// note starts before the last fully decays, so they blend into one bright upward flourish
/// rather than sounding like three separate blips), then the whole thing gets bitcrushed and
/// gently compressed for that 8/16-bit console "coin get" flavor.
///
/// `root_hz` sets the pitch of the first note (the other two are a major third and a fifth
/// above); `gain` is overall peak loudness 0..1.
fn synth_coin_arpeggio_wav(root_hz: f32, gain: f32) -> Vec<u8> {
    // Major triad ratios: root, major third, perfect fifth — an unambiguously "happy" collection
    // jingle, reused an octave up for a bright topping note.
    let ratios = [1.0_f32, 5.0 / 4.0, 3.0 / 2.0, 2.0];
    let note_duration = 0.045; // Each note is short and fast — "coins", not "melody".
    let note_spacing = 0.032; // Notes overlap slightly (spacing < duration) for a fluid run.
    let adsr = Adsr {
        attack: 0.002,
        decay: 0.05,
        sustain: 0.15,
        release: 0.06,
    };

    let mut mix: Vec<f32> = Vec::new();
    for (i, &ratio) in ratios.iter().enumerate() {
        let freq = root_hz * ratio;
        // A slightly lower modulation index/decay on the topping note keeps it bright but not
        // harsh — the classic FM "electric piano" gets duller as it climbs into the high range.
        let bell = synth_fm_note_pitch_attack(freq, 3.0, 3.5, 22.0, note_duration, &adsr, gain);
        // Layer a quiet triangle-wave sub-oscillator, one octave down, under the FM bell using
        // the plain additive synth — gives the ping a bit of chip-tune "body" beneath the bright
        // FM overtones, so it doesn't sound like a bare sine/FM blip.
        let body = synth_note(
            Waveform::Triangle,
            freq * 0.5,
            note_duration,
            &adsr,
            gain * 0.35,
        );
        let offset = (SAMPLE_RATE as f32 * note_spacing * i as f32) as usize;
        mix_into(&mut mix, &bell, offset);
        mix_into(&mut mix, &body, offset);
    }

    // Retro FX chain: mild bitcrush for chip-tune grit, then compress to glue the overlapping
    // notes together and keep the peak of the run under control, then saturate to full loudness.
    bitcrush(&mut mix, 10, 2);
    compress(&mut mix, 0.5, 3.0, 0.002, 0.08);
    normalize_and_saturate(&mut mix, 0.9);

    let pcm = samples_to_pcm(&mut mix, 6, 2);
    encode_wav_mono16(&pcm)
}

/// Build a playable `Source` for the synthesized coin/collection chime (see
/// `synth_coin_arpeggio_wav`). Constructed once at startup, like the other percussion voices, and
/// replayed with `play_detached`/pitch variation on each catch.
pub fn synth_coin_chime(ctx: &mut Context) -> GameResult<Source> {
    let wav = synth_coin_arpeggio_wav(660.0, 0.8); // E5-ish root: high and bright.
    let data = SoundData::from_bytes(&wav);
    Source::from_data(ctx, data)
}

// ---------------------------------------------------------------------------------------------
// Ambient synth pads: long-swell drones with a sweeping resonant filter, a feedback delay, and
// slow stereo auto-panning, for a calm/atmospheric moment (e.g. opening the campaign world map)
// rather than the percussive/melodic voices above. Built from the same oscillator/ADSR/FM
// primitives, just with much longer envelopes and a stereo FX chain layered on top.
// ---------------------------------------------------------------------------------------------

/// Sweep a resonant bandpass filter's center frequency slowly back and forth and blend the
/// filtered "peak" back in on top of the dry signal — like someone slowly working an EQ's
/// resonance/frequency knobs by hand, adding movement and subtle emphasis without fully carving
/// away the rest of the tone. Uses a Chamberlin state-variable filter (SVF): cheap, stable, and
/// (unlike a fixed biquad) safe to modulate every sample since it needs no coefficient recompute.
fn apply_resonant_sweep(
    samples: &mut [f32],
    center_hz: f32,
    sweep_hz: f32,
    sweep_rate_hz: f32,
    resonance: f32,
) {
    let dt = 1.0 / SAMPLE_RATE as f32;
    // Chamberlin's damping factor: smaller = sharper, more pronounced resonance peak.
    let q = 1.0 / resonance.max(0.5);
    let mut low = 0.0_f32;
    let mut band = 0.0_f32;
    for (i, s) in samples.iter_mut().enumerate() {
        let t = i as f32 * dt;
        let fc =
            (center_hz + sweep_hz * (std::f32::consts::TAU * sweep_rate_hz * t).sin()).max(20.0);
        // SVF stability limit: keep the frequency coefficient comfortably below the point where
        // the filter would start ringing out of control at audio sample rates.
        let f = (2.0 * (std::f32::consts::PI * fc / SAMPLE_RATE as f32).sin()).min(1.2);
        let input = *s;
        let high = input - low - q * band;
        band += f * high;
        low += f * band;
        // Blend the resonant bandpass "peak" back in at a modest level — an emphasis sweep on
        // top of the dry tone, not a full filter replacing it.
        *s = input + band * 0.5;
    }
}

/// Feedback delay line: `y[n] = x[n] + feedback * y[n - delay_samples]`, then cross-faded against
/// the dry signal by `mix` (0 = dry only, 1 = fully wet i.e. the delayed signal, which itself
/// still carries the original hit as its very first "tap"). Extends the buffer with a silent
/// tail so the echoes ring out fully instead of being cut off at the note's original length.
fn apply_delay(dry: &[f32], delay_time_s: f32, feedback: f32, mix: f32) -> Vec<f32> {
    let delay_samples = ((SAMPLE_RATE as f32) * delay_time_s).max(1.0) as usize;
    // A handful of extra repeats' worth of silence so the feedback trail has room to decay away.
    let tail_len = delay_samples * 6;
    let total_len = dry.len() + tail_len;

    let mut wet = vec![0.0_f32; total_len];
    for i in 0..total_len {
        let input = if i < dry.len() { dry[i] } else { 0.0 };
        let echo = if i >= delay_samples {
            wet[i - delay_samples] * feedback
        } else {
            0.0
        };
        wet[i] = input + echo;
    }

    let mut out = Vec::with_capacity(total_len);
    for i in 0..total_len {
        let dry_sample = if i < dry.len() { dry[i] } else { 0.0 };
        out.push(dry_sample * (1.0 - mix) + wet[i] * mix);
    }
    out
}

/// Split a mono signal into left/right channels with a slow auto-pan: the stereo position drifts
/// sinusoidally between hard left and hard right at `pan_rate_hz` (typically well under 1 Hz, so
/// it reads as a gentle drift rather than a tremolo). Uses the equal-power panning law (quarter
/// sine/cosine curve) so perceived loudness stays constant as the sound moves across the stereo
/// field, instead of dipping in the center like a naive linear crossfade would.
fn apply_stereo_pan(mono: &[f32], pan_rate_hz: f32) -> (Vec<f32>, Vec<f32>) {
    let dt = 1.0 / SAMPLE_RATE as f32;
    let mut left = Vec::with_capacity(mono.len());
    let mut right = Vec::with_capacity(mono.len());
    for (i, &s) in mono.iter().enumerate() {
        let t = i as f32 * dt;
        let pan = (std::f32::consts::TAU * pan_rate_hz * t).sin(); // -1 (left) .. +1 (right)
        let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4; // maps -1..1 to 0..pi/2
        left.push(s * angle.cos());
        right.push(s * angle.sin());
    }
    (left, right)
}

/// Wrap interleaved stereo `-1..1` sample pairs in a canonical 44-byte WAV header (2-channel,
/// 16-bit PCM), mirroring `encode_wav_mono16` but for the panned pad output.
fn encode_wav_stereo16(left: &[f32], right: &[f32]) -> Vec<u8> {
    let n_frames = left.len().min(right.len());
    // Normalize both channels together so the gain decision is made on the combined peak,
    // preserving relative panning while keeping the output at a consistent level.
    let mut left = left[..n_frames].to_vec();
    let mut right = right[..n_frames].to_vec();
    {
        // Find combined stereo peak so L/R stay in proportion.
        let peak_l = left.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        let peak_r = right.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        let peak = peak_l.max(peak_r);
        if peak > 1e-6 {
            const TARGET: f32 = 0.841;
            let gain = TARGET / peak;
            for s in left.iter_mut().chain(right.iter_mut()) {
                let drive = *s * gain;
                *s = drive.tanh() / gain.tanh() * TARGET;
            }
        }
    }
    let num_channels: u16 = 2;
    let bits_per_sample: u16 = 16;
    let byte_rate = SAMPLE_RATE * num_channels as u32 * (bits_per_sample as u32 / 8);
    let block_align = num_channels * (bits_per_sample / 8);
    let data_len = (n_frames * 2 * 2) as u32;
    let riff_len = 36 + data_len;

    let mut out = Vec::with_capacity(44 + n_frames * 4);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_len.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&num_channels.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..n_frames {
        let l = (left[i].clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        let r = (right[i].clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&l.to_le_bytes());
        out.extend_from_slice(&r.to_le_bytes());
    }
    out
}

/// Named ambient pad presets. Each is a fixed bundle of oscillator/ADSR/filter/delay/pan
/// parameters producing a distinct mood — callers just pick a mood and a root pitch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PadPreset {
    /// Mellow and warm: detuned triangle layers, a slow low filter sweep, a soft long tail.
    WarmPad,
    /// Brighter and glassy: sine + a touch of bell-like FM, a wider/faster filter sweep.
    CrystalPad,
}

/// Parameters bundled per `PadPreset`. Not exposed publicly — presets are meant to be picked by
/// name, not hand-tuned by callers; add a new `PadPreset` variant instead of exposing this.
struct PadParams {
    waveform: Waveform,
    detune_cents: f32,
    fm_layer: bool,
    adsr: Adsr,
    filter_center_hz: f32,
    filter_sweep_hz: f32,
    filter_sweep_rate_hz: f32,
    filter_resonance: f32,
    delay_time_s: f32,
    delay_feedback: f32,
    delay_mix: f32,
    pan_rate_hz: f32,
}

impl PadPreset {
    fn params(self) -> PadParams {
        match self {
            PadPreset::WarmPad => PadParams {
                waveform: Waveform::Triangle,
                detune_cents: 7.0,
                fm_layer: false,
                // Long attack/release — the defining trait of a "pad": a slow swell in, a long
                // fade out, no percussive transient at all.
                adsr: Adsr {
                    attack: 1.2,
                    decay: 0.6,
                    sustain: 0.7,
                    release: 2.5,
                },
                filter_center_hz: 700.0,
                filter_sweep_hz: 350.0,
                filter_sweep_rate_hz: 0.08,
                filter_resonance: 6.0,
                delay_time_s: 0.35,
                delay_feedback: 0.35,
                delay_mix: 0.25,
                pan_rate_hz: 0.06,
            },
            PadPreset::CrystalPad => PadParams {
                waveform: Waveform::Sine,
                detune_cents: 12.0,
                fm_layer: true,
                adsr: Adsr {
                    attack: 0.6,
                    decay: 0.8,
                    sustain: 0.55,
                    release: 3.2,
                },
                filter_center_hz: 1600.0,
                filter_sweep_hz: 900.0,
                filter_sweep_rate_hz: 0.15,
                filter_resonance: 9.0,
                delay_time_s: 0.28,
                delay_feedback: 0.4,
                delay_mix: 0.3,
                pan_rate_hz: 0.1,
            },
        }
    }
}

/// Render a full ambient pad through the whole chain: three detuned additive-synth voices (root
/// + up/down a few cents, the classic "supersaw" width trick) optionally topped with an FM bell
/// layer, a slow sweeping resonant filter, a feedback delay, gentle compression to glue it all
/// together, then slow stereo auto-panning — producing a stereo 16-bit WAV byte buffer.
fn synth_pad_wav(preset: PadPreset, root_hz: f32, note_duration: f32, gain: f32) -> Vec<u8> {
    let p = preset.params();
    let cents_to_ratio = |cents: f32| 2.0_f32.powf(cents / 1200.0);

    let mut mono = synth_note(p.waveform, root_hz, note_duration, &p.adsr, gain * 0.5);
    let detuned_up = synth_note(
        p.waveform,
        root_hz * cents_to_ratio(p.detune_cents),
        note_duration,
        &p.adsr,
        gain * 0.3,
    );
    let detuned_down = synth_note(
        p.waveform,
        root_hz * cents_to_ratio(-p.detune_cents),
        note_duration,
        &p.adsr,
        gain * 0.3,
    );
    for (i, v) in detuned_up.iter().enumerate() {
        mono[i] += v;
    }
    for (i, v) in detuned_down.iter().enumerate() {
        mono[i] += v;
    }

    if p.fm_layer {
        // A quiet, slowly-decaying FM bell an octave up adds glassy overtones to the crystal
        // preset without overpowering the underlying additive layers.
        let bell = synth_fm_note(
            root_hz * 2.0,
            2.0,
            1.2,
            0.6,
            note_duration,
            &p.adsr,
            gain * 0.25,
        );
        for (i, v) in bell.iter().enumerate() {
            mono[i] += v;
        }
    }

    // A deliberately stark 16-step minor tracker line gives the map pad a darker, Deus Ex-era
    // industrial pulse: clipped square notes, a low repeating bass ostinato, and a couple of
    // strategically empty steps. It is a musical pattern rather than a modern sustained chord,
    // so the ambience still has a machine-like groove underneath the long pad.
    let tracker_adsr = Adsr {
        attack: 0.001,
        decay: 0.035,
        sustain: 0.0,
        release: 0.045,
    };
    let tracker_pattern = [0, 3, 7, 10, 7, 3, 0, -2, 0, 3, 7, 12, 10, 7, 3, -2];
    let step_duration = 0.125;
    for (step, semitone) in tracker_pattern.iter().enumerate() {
        // Leave two off-beats empty, as a tracker pattern would, instead of filling every slot.
        if step == 5 || step == 13 {
            continue;
        }
        let ratio = 2.0_f32.powf(*semitone as f32 / 12.0);
        let voice = synth_note(
            Waveform::Rect(0.125),
            root_hz * ratio,
            0.075,
            &tracker_adsr,
            gain * 0.16,
        );
        mix_into(
            &mut mono,
            &voice,
            (SAMPLE_RATE as f32 * step_duration * step as f32) as usize,
        );
    }
    // A quieter sub-bass line pins the motif to the root, like a second tracker channel.
    let bass = synth_note(
        Waveform::Rect(0.5),
        root_hz * 0.5,
        note_duration,
        &tracker_adsr,
        gain * 0.12,
    );
    mix_into(&mut mono, &bass, 0);

    apply_resonant_sweep(
        &mut mono,
        p.filter_center_hz,
        p.filter_sweep_hz,
        p.filter_sweep_rate_hz,
        p.filter_resonance,
    );

    let mut wet = apply_delay(&mono, p.delay_time_s, p.delay_feedback, p.delay_mix);
    // Gentle glue compression (long attack/release suits a slow-moving pad, unlike the punchy
    // settings used for the coin chime) so the layered voices + delay don't get too peaky.
    compress(&mut wet, 0.6, 2.5, 0.05, 0.3);
    bitcrush(&mut wet, 8, 2);
    normalize_and_saturate(&mut wet, 0.75);

    let (left, right) = apply_stereo_pan(&wet, p.pan_rate_hz);
    encode_wav_stereo16(&left, &right)
}

/// Build a playable ambient pad `Source` from a preset. `root_hz` sets the pad's fundamental
/// pitch; `note_duration` is how long the note is held before release begins — the audible tail
/// runs considerably longer than that once the long release and the delay's echo trail are
/// included, so this suits a calm, atmospheric moment (e.g. opening the campaign world map)
/// rather than a rhythm-locked SFX.
pub fn synth_ambient_pad(
    ctx: &mut Context,
    preset: PadPreset,
    root_hz: f32,
    note_duration: f32,
) -> GameResult<Source> {
    let wav = synth_pad_wav(preset, root_hz, note_duration, 0.7);
    let data = SoundData::from_bytes(&wav);
    Source::from_data(ctx, data)
}

/// Synthesise a single kick-drum hit as a mono 16-bit WAV byte buffer.
///
/// A kick is a sine whose pitch drops fast from a punchy attack transient down to a low body,
/// under an exponential amplitude decay — the classic 808/909 "boom". Parameters let the caller
/// make a heavier, lower kick for the downbeat vs. the three beats between it.
///
/// * `start_hz` / `end_hz` — pitch sweeps from the attack click down to the body over the hit.
/// * `duration` — total length in seconds (~0.12s reads as a tight kick, not a lingering tom).
/// * `gain` — peak amplitude 0..1; the downbeat gets a touch more so the "1" lands hardest.
fn synth_kick_wav(start_hz: f32, end_hz: f32, duration: f32, gain: f32) -> Vec<u8> {
    let n_samples = (SAMPLE_RATE as f32 * duration) as usize;
    let mut samples = Vec::with_capacity(n_samples);

    // Integrate the (falling) instantaneous frequency into a continuous phase so there's no click
    // from a discontinuity when the pitch slides. Phase in radians, advanced each sample.
    let mut phase = 0.0_f32;
    let dt = 1.0 / SAMPLE_RATE as f32;
    for i in 0..n_samples {
        let t = i as f32 * dt;
        let progress = t / duration; // 0..1 across the hit

        // Pitch drop: exponential glide from start_hz to end_hz feels punchier than linear.
        let freq = end_hz + (start_hz - end_hz) * (-6.0 * progress).exp();
        phase += freq * dt;
        phase = phase.rem_euclid(1.0);

        // Amplitude: fast exponential decay so the hit is percussive, plus a very short attack
        // ramp (first ~2ms) to avoid a hard click at sample 0.
        let attack = (t / 0.002).min(1.0);
        let env = attack * (-5.0 * progress).exp();

        // Triangle body plus a narrow pulse click gives the kick a distinctly synthetic console
        // attack instead of a smooth sine boom.
        let body = oscillator_sample(Waveform::Triangle, phase);
        let click = oscillator_sample(Waveform::Rect(0.125), phase * 2.0);
        let sample = (body * 0.82 + click * 0.18) * env * gain;
        samples.push((sample * 1.4).tanh());
    }

    let pcm = samples_to_pcm(&mut samples, 6, 2);
    encode_wav_mono16(&pcm)
}

/// Synthesise a snare hit: filtered noise burst with a brief tonal body (200 Hz sine), giving
/// the classic crack-and-sizzle of a snare without any sample files.
///
/// * `duration` — total length (~0.09s = tight snare crack).
/// * `gain` — peak amplitude 0..1.
fn synth_snare_wav(duration: f32, gain: f32) -> Vec<u8> {
    let n_samples = (SAMPLE_RATE as f32 * duration) as usize;
    let mut samples = Vec::with_capacity(n_samples);

    let mut noise_state: u32 = 0xace1;

    let dt = 1.0 / SAMPLE_RATE as f32;
    let mut tone_phase = 0.0_f32;
    // One-pole highpass state — bleeds low freqs out of the noise so it reads as sizzle.
    let mut hp_prev_in = 0.0_f32;
    let mut hp_prev_out = 0.0_f32;
    // Highpass RC coefficient: cutoff ~800 Hz.
    let rc = 1.0 / (2.0 * std::f32::consts::PI * 800.0 * dt + 1.0);

    for i in 0..n_samples {
        let t = i as f32 * dt;
        let progress = t / duration;

        // Fast attack (2 ms), then exponential decay.
        let attack = (t / 0.002).min(1.0);
        let env = attack * (-12.0 * progress).exp();

        // Tonal body: a 200 Hz sine gives the snare its crack (predominant in the first ~10 ms).
        tone_phase += std::f32::consts::TAU * 200.0 * dt;
        let tone = tone_phase.sin() * (-30.0 * progress).exp(); // dies fast

        // Noise component: highpass-filtered to remove the muddy low end.
        let noise_in = lfsr_noise(&mut noise_state);
        let hp = rc * (hp_prev_out + noise_in - hp_prev_in);
        hp_prev_in = noise_in;
        hp_prev_out = hp;

        // Mix: 30% tone crack + 70% sizzle noise, under the shared envelope.
        let sample = (0.3 * tone + 0.7 * hp) * env * gain;
        let driven = (sample * 1.2).tanh();
        samples.push(driven);
    }

    let pcm = samples_to_pcm(&mut samples, 4, 2);
    encode_wav_mono16(&pcm)
}

/// Wrap 16-bit mono PCM samples in a canonical 44-byte WAV header (PCM format code 1). Must be
/// byte-exact or rodio's decoder rejects it and the `Source` fails to build.
/// A crisp hi-hat click — white noise with a very short exponential decay.
/// Used for the B-key "jam emote" so the player crab can vibe.
pub fn synth_hihat(ctx: &mut Context) -> GameResult<Source> {
    let n = (SAMPLE_RATE as f32 * 0.08) as usize; // 80ms
    let mut noise_state: u32 = 0x5eed;
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / SAMPLE_RATE as f32;
        let noise = lfsr_noise(&mut noise_state);
        // Short sharp decay + high-pass via mixing two noise phases
        let env = (-80.0 * t).exp();
        let v = noise * env * 0.55;
        samples.push(v);
    }
    let pcm = samples_to_pcm(&mut samples, 4, 2);
    let wav = encode_wav_mono16(&pcm);
    let data = SoundData::from_bytes(&wav);
    Source::from_data(ctx, data)
}

/// A short bright chirp for the flashlight toggle (F key). ~120ms sine sweep with a snappy
/// exponential decay so it reads as a crisp "UI click" without being intrusive.
pub fn synth_flashlight_toggle(ctx: &mut Context) -> GameResult<Source> {
    let dur = 0.12_f32;
    let n = (SAMPLE_RATE as f32 * dur) as usize;
    let dt = 1.0 / SAMPLE_RATE as f32;
    let mut samples = Vec::with_capacity(n);
    let start_hz = 1800.0_f32;
    let end_hz = 2600.0_f32;
    let mut phase = 0.0_f32;
    for i in 0..n {
        let t = i as f32 * dt;
        let k = t / dur;
        let hz = start_hz + (end_hz - start_hz) * k;
        phase += 2.0 * std::f32::consts::PI * hz * dt;
        // Fast attack, exponential decay
        let attack = (t / 0.005).min(1.0);
        let env = attack * (-22.0 * t).exp();
        let v = phase.sin() * env * 0.35;
        samples.push(v);
    }
    let pcm = samples_to_pcm(&mut samples, 8, 1);
    let wav = encode_wav_mono16(&pcm);
    let data = SoundData::from_bytes(&wav);
    Source::from_data(ctx, data)
}

fn encode_wav_mono16(pcm: &[i16]) -> Vec<u8> {
    let num_channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    let byte_rate = SAMPLE_RATE * num_channels as u32 * (bits_per_sample as u32 / 8);
    let block_align = num_channels * (bits_per_sample / 8);
    let data_len = (pcm.len() * 2) as u32;
    let riff_len = 36 + data_len;

    let mut out = Vec::with_capacity(44 + pcm.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_len.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    // fmt subchunk
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size for PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // audio format 1 = PCM
    out.extend_from_slice(&num_channels.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());
    // data subchunk
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Build a playable `Source` from freshly synthesised kick bytes. Constructed once at startup so a
/// bad WAV header surfaces immediately (as an error here) rather than as silent nothing on the
/// first beat.
fn kick_source(
    ctx: &mut Context,
    start_hz: f32,
    end_hz: f32,
    duration: f32,
    gain: f32,
) -> GameResult<Source> {
    let bytes = synth_kick_wav(start_hz, end_hz, duration, gain);
    let data = SoundData::from_bytes(&bytes);
    Source::from_data(ctx, data)
}

fn snare_source(ctx: &mut Context, duration: f32, gain: f32) -> GameResult<Source> {
    let bytes = synth_snare_wav(duration, gain);
    let data = SoundData::from_bytes(&bytes);
    Source::from_data(ctx, data)
}

/// Synthesise a looping "living creature" ambient for the NPC King Crab conga train.
///
/// A quiet low rumble sits underneath as a bass bed, but the character is carried by three
/// organic percussion layers baked into the buffer:
///   * metallic shell-click transients (very short filtered-noise pings, ~1.8–3.5 kHz)
///   * claw-snap chirps (brief resonant bursts, ~300–600 Hz)
///   * mandible chitter bursts (rapid 60–90 ms clusters of tiny clicks)
/// All events are scattered pseudo-randomly across a ~2 s loop with varying density and pitch
/// so it reads as a creature moving nearby rather than a repeating synth pad.
/// Wrapped in a WAV so `Source::from_data` / rodio can decode it normally.
/// The caller sets `repeat(true)` so it loops; volume is driven by distance each frame.
pub fn synth_king_crab_rumble(ctx: &mut Context) -> GameResult<Source> {
    // Longer loop (~2s) so the tap pattern doesn't feel obviously cyclic.
    let loop_len = 2.0_f32;
    let n = (SAMPLE_RATE as f32 * loop_len) as usize;
    let dt = 1.0 / SAMPLE_RATE as f32;
    let mut samples = vec![0.0_f32; n];

    // Deterministic PRNG (xorshift32) so the loop is reproducible from build to build.
    let mut rng_state: u32 = 0xC0FFEE_u32;
    fn xorshift(s: &mut u32) -> u32 {
        let mut x = *s;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *s = x;
        x
    }
    fn rand01(s: &mut u32) -> f32 {
        (xorshift(s) as f32) / (u32::MAX as f32)
    }

    // --- Layer 1: low ambient rumble bed (soft, so taps sit on top clearly) ---
    for i in 0..n {
        let t = i as f32 * dt;
        // Slow amplitude undulation so the bed breathes.
        let breathe = 0.55 + 0.25 * (0.7 * t * std::f32::consts::TAU).sin();
        let rumble = 0.55 * oscillator_sample(Waveform::Triangle, 78.0 * t)
            + 0.22 * oscillator_sample(Waveform::Rect(0.5), 119.0 * t)
            + 0.12 * oscillator_sample(Waveform::Triangle, 167.0 * t);
        samples[i] += rumble * breathe * 0.35;
    }

    // Helper: add a filtered-noise "click" transient at sample offset `at` (in samples)
    // with a short exponential envelope and a resonant carrier frequency. Uses a simple
    // one-pole highpass on white noise (via successive-difference) to bias toward brightness.
    // `carrier_hz` gives it a metallic pitch; noise gives it grit.
    let add_click = |samples: &mut Vec<f32>,
                     at: usize,
                     dur_s: f32,
                     carrier_hz: f32,
                     noise_mix: f32,
                     amp: f32,
                     rng_state: &mut u32| {
        let dur_n = (SAMPLE_RATE as f32 * dur_s) as usize;
        let mut prev_noise = 0.0_f32;
        for k in 0..dur_n {
            let t = k as f32 * dt;
            // Fast attack, exponential decay — very short so it reads as a "tick".
            let env = (1.0 - (-900.0 * t).exp()) * (-38.0 / dur_s.max(0.001) * t).exp();
            // xorshift white noise -> [-1, 1]
            let mut x = *rng_state;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *rng_state = x;
            let raw = (x as f32) / (u32::MAX as f32) * 2.0 - 1.0;
            // Highpass via first-difference: brightens the noise.
            let hp = raw - prev_noise * 0.85;
            prev_noise = raw;
            // Ringing resonant carrier at carrier_hz for the metallic ping quality.
            let ring = (std::f32::consts::TAU * carrier_hz * t).sin();
            let v = (noise_mix * hp + (1.0 - noise_mix) * ring) * env * amp;
            let idx = (at + k) % samples.len(); // wrap into loop for seamless boundary
            samples[idx] += v;
        }
    };

    // Helper: brief resonant chirp for claw-snap (300-600Hz, slight downward slide,
    // fast decay). This gives the "snap" more body than a pure click.
    let add_claw_snap =
        |samples: &mut Vec<f32>, at: usize, start_hz: f32, end_hz: f32, dur_s: f32, amp: f32| {
            let dur_n = (SAMPLE_RATE as f32 * dur_s) as usize;
            let mut phase = 0.0_f32;
            for k in 0..dur_n {
                let t = k as f32 * dt;
                let slide = (t / dur_s).min(1.0);
                let freq = start_hz + (end_hz - start_hz) * slide;
                phase += freq * dt;
                // Sharp attack, fast decay — snappy but with a hint of sustain from the sine.
                let env = (1.0 - (-500.0 * t).exp()) * (-28.0 * t).exp();
                let body = (phase * std::f32::consts::TAU).sin();
                // A hair of second-harmonic gives it a woodier bite than a pure sine.
                let bite = 0.35 * (phase * 2.0 * std::f32::consts::TAU).sin();
                let idx = (at + k) % samples.len();
                samples[idx] += (body + bite) * env * amp;
            }
        };

    // --- Layer 2: sparse shell-click pings scattered across the loop ---
    // Density varies: some pockets busy, others quiet.
    let mut t_cursor = 0.02_f32;
    while t_cursor < loop_len - 0.05 {
        let at = (t_cursor * SAMPLE_RATE as f32) as usize;
        // Pitch drifts across the loop for variety (1.8–3.5 kHz range).
        let carrier = 1800.0 + rand01(&mut rng_state) * 1700.0;
        let dur = 0.008 + rand01(&mut rng_state) * 0.012;
        let amp = 0.18 + rand01(&mut rng_state) * 0.14;
        // Noise-heavy so it sounds like a shell tick, not a tone.
        add_click(&mut samples, at, dur, carrier, 0.75, amp, &mut rng_state);
        // Gap: mostly short, occasionally long "pockets of silence" so it feels alive.
        let r = rand01(&mut rng_state);
        let gap = if r > 0.85 {
            0.18 + rand01(&mut rng_state) * 0.20
        } else {
            0.05 + rand01(&mut rng_state) * 0.12
        };
        t_cursor += gap;
    }

    // --- Layer 3: a few claw-snap chirps, sparser and louder than shell clicks ---
    let snap_times = [0.18_f32, 0.55, 0.92, 1.34, 1.71];
    for &st in &snap_times {
        let at = (st * SAMPLE_RATE as f32) as usize;
        // Each snap picks a slightly different pitch region so they don't sound identical.
        let start_hz = 320.0 + rand01(&mut rng_state) * 260.0;
        let end_hz = start_hz * (0.55 + rand01(&mut rng_state) * 0.15);
        let dur = 0.030 + rand01(&mut rng_state) * 0.025;
        let amp = 0.22 + rand01(&mut rng_state) * 0.10;
        add_claw_snap(&mut samples, at, start_hz, end_hz, dur, amp);
    }

    // --- Layer 4: mandible chitter bursts — rapid 60–90ms clusters of tiny clicks ---
    // Three bursts placed in the loop, each a dense micro-pattern of 5–9 clicks.
    let chitter_starts = [0.30_f32, 1.05, 1.55];
    for &burst_start in &chitter_starts {
        let click_count = 5 + (rand01(&mut rng_state) * 5.0) as usize; // 5..=9
        let burst_span = 0.055 + rand01(&mut rng_state) * 0.035; // 55–90 ms
        // Pitch centre for this creature-voice burst, varied per burst.
        let pitch_centre = 2600.0 + rand01(&mut rng_state) * 1400.0;
        for c in 0..click_count {
            // Slight timing jitter within the burst so it doesn't sound machine-gunned.
            let frac = c as f32 / (click_count.max(1) as f32);
            let jitter = (rand01(&mut rng_state) - 0.5) * 0.006;
            let t_click = burst_start + frac * burst_span + jitter;
            if t_click <= 0.0 || t_click >= loop_len - 0.01 {
                continue;
            }
            let at = (t_click * SAMPLE_RATE as f32) as usize;
            // Tiny per-click pitch wobble around the burst's centre.
            let carrier = pitch_centre * (0.85 + rand01(&mut rng_state) * 0.3);
            // Each chitter click is very short and quieter than shell clicks.
            add_click(
                &mut samples,
                at,
                0.005 + rand01(&mut rng_state) * 0.004,
                carrier,
                0.65,
                0.11 + rand01(&mut rng_state) * 0.07,
                &mut rng_state,
            );
        }
    }

    // --- Final pass: soft clip + normalise to i16 range with headroom ---
    for v in samples.iter_mut() {
        *v = (*v * 0.85).tanh();
    }

    // Convert to PCM. Milder bit-crush (8-bit) than before — the taps rely on transient
    // detail that heavy crushing would smear.
    let pcm = samples_to_pcm(&mut samples, 8, 1);
    let wav = encode_wav_mono16(&pcm);
    let data = SoundData::from_bytes(&wav);
    let mut src = Source::from_data(ctx, data)?;
    src.set_repeat(true);
    Ok(src)
}

/// The synthesised percussion voices, built once and replayed on the beat.
pub struct BeatSynth {
    /// The heavier, lower kick for the downbeat ("1" of the bar).
    downbeat_kick: Source,
    /// The lighter kick for the three beats between downbeats.
    offbeat_kick: Source,
    /// Snare hit — played on beats 2 & 4 (the backbeat) during boss fights.
    snare: Source,
    /// Current snare volume, 0..1. Fades in when a boss is present, fades out when cleared.
    /// Smoothly interpolated each beat so it never pops in or disappears abruptly.
    pub snare_volume: f32,
}

impl BeatSynth {
    pub fn new(ctx: &mut Context) -> GameResult<BeatSynth> {
        Ok(BeatSynth {
            // Downbeat: lower, longer, louder — the "1" you feel in your chest.
            downbeat_kick: kick_source(ctx, 150.0, 45.0, 0.14, 0.9)?,
            // Offbeat: higher pitched, tighter, quieter so the bar has a clear accent structure.
            offbeat_kick: kick_source(ctx, 130.0, 55.0, 0.10, 0.55)?,
            // Snare: tight crack, full gain baked in — volume is controlled via snare_volume.
            snare: snare_source(ctx, 0.09, 0.75)?,
            snare_volume: 0.0,
        })
    }

    /// Fade snare volume toward target each beat (call once per beat tick).
    /// `boss_present` drives the target; the rate is ~4 beats to full so the entry
    /// feels like a DJ bringing a layer in, not a sudden switch.
    pub fn update_snare_volume(&mut self, boss_present: bool) {
        let target = if boss_present { 1.0 } else { 0.0 };
        // Move ~25% of the remaining gap each beat — smooth exponential approach.
        self.snare_volume += (target - self.snare_volume) * 0.25;
        // Clamp so floating-point drift never escapes the valid range.
        self.snare_volume = self.snare_volume.clamp(0.0, 1.0);
    }

    /// Play a kick for this beat. `downbeat` picks the heavier voice on the "1".
    pub fn play_kick(&mut self, ctx: &mut Context, downbeat: bool) {
        use ggez::audio::SoundSource;
        let src = if downbeat {
            &mut self.downbeat_kick
        } else {
            &mut self.offbeat_kick
        };
        let _ = src.play_detached(ctx);
    }

    /// Play the snare if it has audible volume. `beat_index` is the beat position within the bar
    /// (0-based); the snare lands on beats 1 and 3 (the "2" and "4" of the bar in 1-based terms).
    pub fn play_snare(&mut self, ctx: &mut Context, beat_index: u32) {
        use ggez::audio::SoundSource;
        // Only fire on the backbeat (beats 2 & 4 in musical 1-based terms).
        if beat_index % 4 != 1 && beat_index % 4 != 3 {
            return;
        }
        if self.snare_volume < 0.01 {
            return;
        }
        self.snare.set_volume(self.snare_volume);
        let _ = self.snare.play_detached(ctx);
    }
}

/// Synthesise a sharp finger-whistle: a pure sine wave with vibrato that slides from a lower
/// note up to the target pitch in the first ~30 ms (the "blow-in" attack), holds with light
/// vibrato for the sustain, then fades via an exponential decay.  Bit-crushed lightly so it
/// sits in the retro chiptune palette without sounding too clean.
pub fn synth_whistle(ctx: &mut Context) -> GameResult<Source> {
    let duration = 0.38_f32;
    let n = (SAMPLE_RATE as f32 * duration) as usize;
    let mut samples = Vec::with_capacity(n);

    let target_hz = 1600.0_f32; // piercing high whistle
    let start_hz = 900.0_f32; // slide-in from a lower note

    let attack = 0.03_f32; // pitch-slide / volume attack
    let decay = 0.10_f32; // amplitude decay begins here
    let vibrato_rate = 6.5_f32;
    let vibrato_depth = 0.012_f32; // fraction of freq

    let mut phase = 0.0_f32;
    for i in 0..n {
        let t = i as f32 / SAMPLE_RATE as f32;

        // Pitch: slide from start_hz to target_hz in the first `attack` seconds.
        let slide = (t / attack).min(1.0);
        let vibrato = 1.0 + vibrato_depth * (vibrato_rate * t * std::f32::consts::TAU).sin();
        let freq = (start_hz + (target_hz - start_hz) * slide) * vibrato;

        // Amplitude envelope: quick linear attack then exponential decay.
        let amp = if t < attack {
            t / attack
        } else {
            let t_decay = t - attack;
            let decay_len = duration - attack;
            1.0 - (t_decay / decay_len).powi(2)
        }
        .max(0.0);

        phase += freq / SAMPLE_RATE as f32;
        samples.push((phase * std::f32::consts::TAU).sin() * amp * 0.7);
    }

    let pcm = samples_to_pcm(&mut samples, 12, 1); // mild bit-crush, no sample-hold
    let wav = encode_wav_mono16(&pcm);
    let data = SoundData::from_bytes(&wav);
    Source::from_data(ctx, data)
}

/// Synthesise a deep stomp thud: a pitched kick (80→30 Hz pitch sweep) layered with a short
/// burst of LFSR noise for the "crack" transient, then fast exponential decay.
pub fn synth_stomp(ctx: &mut Context) -> GameResult<Source> {
    let duration = 0.28_f32;
    let n = (SAMPLE_RATE as f32 * duration) as usize;
    let mut samples = Vec::with_capacity(n);

    let mut lfsr: u32 = 0xACE1;
    let mut phase = 0.0_f32;

    for i in 0..n {
        let t = i as f32 / SAMPLE_RATE as f32;

        // Pitch sweep: 120 Hz → 30 Hz over ~60 ms
        let sweep_len = 0.06_f32;
        let freq = if t < sweep_len {
            120.0 - 90.0 * (t / sweep_len)
        } else {
            30.0
        };
        phase += freq / SAMPLE_RATE as f32;
        let kick = (phase * std::f32::consts::TAU).sin();

        // LFSR noise burst in first 12 ms for the crack transient
        let noise = if t < 0.012 {
            lfsr ^= lfsr << 13;
            lfsr ^= lfsr >> 17;
            lfsr ^= lfsr << 5;
            let n = ((lfsr & 0xFF) as f32 / 128.0) - 1.0;
            n * (1.0 - t / 0.012) // fade noise quickly
        } else {
            0.0
        };

        // Amplitude: very fast decay
        let amp = (-t * 18.0_f32).exp();
        samples.push((kick * 0.8 + noise * 0.35) * amp);
    }

    let pcm = samples_to_pcm(&mut samples, 6, 2);
    let wav = encode_wav_mono16(&pcm);
    let data = SoundData::from_bytes(&wav);
    Source::from_data(ctx, data)
}

/// Synthesise a lasso whoosh: band-passed noise swept from low to high frequency,
/// short (120 ms), giving the impression of something spinning then releasing.
pub fn synth_lasso_throw(ctx: &mut Context) -> GameResult<Source> {
    let duration = 0.14_f32;
    let n = (SAMPLE_RATE as f32 * duration) as usize;
    let mut samples = Vec::with_capacity(n);

    let mut lfsr: u32 = 0xDEAD;

    for i in 0..n {
        let t = i as f32 / SAMPLE_RATE as f32;

        // White noise from LFSR
        lfsr ^= lfsr << 13;
        lfsr ^= lfsr >> 17;
        lfsr ^= lfsr << 5;
        let noise = ((lfsr & 0xFF) as f32 / 128.0) - 1.0;

        // Amplitude envelope: sharp attack (0-10 ms) then exponential decay
        let amp = if t < 0.01 {
            t / 0.01
        } else {
            (-t * 22.0_f32).exp()
        };

        // Very simple "tone" sweep: mix in a sine swept from 400→2000 Hz
        // to give the "whipping" sense of pitch-rise on release
        let freq = 400.0 + 1600.0 * (t / duration);
        let sine_phase = (freq * t * std::f32::consts::TAU).sin();

        samples.push((noise * 0.55 + sine_phase * 0.45) * amp * 0.75);
    }

    let pcm = samples_to_pcm(&mut samples, 8, 1);
    let wav = encode_wav_mono16(&pcm);
    let data = SoundData::from_bytes(&wav);
    Source::from_data(ctx, data)
}

// ---------------------------------------------------------------------------
// Crab-theme melody synthesiser  (Duck Game / Deus Ex inspired ABA loops)
// ---------------------------------------------------------------------------

fn melody_note(hz: f32, dur_s: f32, amp: f32) -> Vec<f32> {
    let n = (SAMPLE_RATE as f32 * dur_s) as usize;
    let mut out = Vec::with_capacity(n);
    if hz < 1.0 {
        out.resize(n, 0.0);
        return out;
    }
    let mut phase = 0.0_f32;
    for i in 0..n {
        let t = i as f32 / SAMPLE_RATE as f32;
        phase += hz / SAMPLE_RATE as f32;
        let sq = if (phase % 1.0) < 0.5 { 1.0_f32 } else { -1.0 };
        let frac = t / dur_s;
        let env = if frac < 0.05 {
            frac / 0.05
        } else if frac < 0.15 {
            1.0 - (frac - 0.05) / 0.10 * 0.3
        } else if frac < 0.82 {
            0.7
        } else {
            0.7 * (1.0 - (frac - 0.82) / 0.18)
        };
        out.push(sq * amp * env.clamp(0.0, 1.0));
    }
    out
}

fn synth_theme_loop(
    ctx: &mut Context,
    beat_s: f32,
    sequence: &[(f32, f32)], // (hz, beats)
    amp: f32,
    bit_depth: u32,
) -> GameResult<Source> {
    let mut all: Vec<f32> = Vec::new();
    for &(hz, beats) in sequence {
        all.extend(melody_note(hz, beat_s * beats, amp));
    }
    let pcm = samples_to_pcm(&mut all, bit_depth, 1);
    let wav = encode_wav_mono16(&pcm);
    let data = SoundData::from_bytes(&wav);
    let mut src = Source::from_data(ctx, data)?;
    src.set_repeat(true);
    Ok(src)
}

// Pre-computed equal-temperament frequencies
const C3: f32 = 130.81;
const E3: f32 = 164.81;
const F3: f32 = 174.61;
const G3: f32 = 196.00;
const A3: f32 = 220.00;
const B3: f32 = 246.94;
const C4: f32 = 261.63;
const D4: f32 = 293.66;
const DS4: f32 = 311.13;
const E4: f32 = 329.63;
const F4: f32 = 349.23;
const FS4: f32 = 369.99;
const G4: f32 = 392.00;
const GS4: f32 = 415.30;
const A4: f32 = 440.00;
const B4: f32 = 493.88;
const C5: f32 = 523.25;
const D5: f32 = 587.33;
const E5: f32 = 659.25;
const FS5: f32 = 739.99;
const G5: f32 = 783.99;
const A5: f32 = 880.00;
const R: f32 = 0.0; // rest

/// Theme 0 — "Happy Crab Stomp": C major, 170 BPM, Duck-Game bouncy arpeggios. ABA.
pub fn synth_theme_duck_bounce(ctx: &mut Context) -> GameResult<Source> {
    let b = 60.0 / 170.0;
    #[rustfmt::skip]
    let seq: &[(f32, f32)] = &[
        // A
        (C5,0.5),(E5,0.5),(G5,0.5),(E5,0.5),(C5,0.5),(G4,0.5),(A4,0.5),(C5,0.5),
        // B
        (G4,1.0),(F4,0.5),(E4,0.5),(D4,1.0),(C4,1.0),
        // A
        (C5,0.5),(E5,0.5),(G5,0.5),(E5,0.5),(C5,0.5),(G4,0.5),(A4,0.5),(C5,1.0),
    ];
    synth_theme_loop(ctx, b, seq, 0.55, 6)
}

/// Theme 1 — "Funky Dancer": D major, 145 BPM, syncopated Duck-Game groove. ABA.
pub fn synth_theme_duck_funky(ctx: &mut Context) -> GameResult<Source> {
    let b = 60.0 / 145.0;
    #[rustfmt::skip]
    let seq: &[(f32, f32)] = &[
        // A — offbeat groove
        (D5,0.5),(R,0.25),(D5,0.25),(A4,0.5),(FS4,0.5),
        (D5,0.5),(E5,0.25),(FS5,0.25),(E5,0.5),(D5,0.5),
        // B — slower hook
        (G4,1.0),(A4,1.0),(FS4,0.5),(E4,0.5),(D4,1.0),
        // A
        (D5,0.5),(R,0.25),(D5,0.25),(A4,0.5),(FS4,0.5),
        (D5,0.5),(E5,0.25),(FS5,0.25),(E5,0.5),(D5,1.0),
    ];
    synth_theme_loop(ctx, b, seq, 0.50, 6)
}

/// Theme 2 — "UNATCO Corridor": E Phrygian, 95 BPM, Deus Ex tense darkness. ABA.
pub fn synth_theme_deus_tense(ctx: &mut Context) -> GameResult<Source> {
    let b = 60.0 / 95.0;
    #[rustfmt::skip]
    let seq: &[(f32, f32)] = &[
        // A — descending Phrygian figure
        (E4,1.0),(F4,0.5),(E4,0.5),(B3,1.0),(G3,1.0),(F3,0.5),(E3,0.5),(R,1.0),
        // B — chromatic tension
        (A3,1.0),(GS4,1.0),(F4,0.5),(E4,1.0),(DS4,0.5),(R,1.0),
        // A
        (E4,1.0),(F4,0.5),(E4,0.5),(B3,1.0),(G3,1.0),(F3,0.5),(E3,1.5),
    ];
    synth_theme_loop(ctx, b, seq, 0.60, 8)
}

/// Theme 3 — "Ambient Biomech": A minor, 80 BPM, sparse Deus Ex atmosphere. ABA.
pub fn synth_theme_deus_ambient(ctx: &mut Context) -> GameResult<Source> {
    let b = 60.0 / 80.0;
    #[rustfmt::skip]
    let seq: &[(f32, f32)] = &[
        // A — long notes with space
        (A3,2.0),(R,0.5),(C4,1.5),(E4,2.0),(R,0.5),(G4,1.5),
        // B — chromatic float
        (F4,2.0),(GS4,1.5),(R,0.5),(B4,2.0),(A4,2.0),
        // A
        (A3,2.0),(R,0.5),(C4,1.5),(E4,2.0),(R,0.5),(A4,2.0),
    ];
    synth_theme_loop(ctx, b, seq, 0.50, 10)
}

/// Theme 4 — "Golden Rush": G major pentatonic, 155 BPM, sparkly Duck-Game runs. ABA.
pub fn synth_theme_duck_golden(ctx: &mut Context) -> GameResult<Source> {
    let b = 60.0 / 155.0;
    #[rustfmt::skip]
    let seq: &[(f32, f32)] = &[
        // A — rapid pentatonic run
        (G4,0.5),(A4,0.5),(B4,0.5),(D5,0.5),(G5,0.5),(D5,0.5),(B4,0.5),(G4,0.5),
        // B — octave leap surprise
        (D5,1.0),(G5,0.5),(A5,0.5),(G5,1.0),(D5,1.0),
        // A
        (G4,0.5),(A4,0.5),(B4,0.5),(D5,0.5),(G5,0.5),(D5,0.5),(B4,0.5),(G4,1.0),
    ];
    synth_theme_loop(ctx, b, seq, 0.55, 6)
}
