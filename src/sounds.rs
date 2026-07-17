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
pub fn synth_note(waveform: Waveform, freq: f32, note_duration: f32, adsr: &Adsr, gain: f32) -> Vec<f32> {
    let total = adsr.total_duration(note_duration).max(0.0);
    let n_samples = (SAMPLE_RATE as f32 * total) as usize;
    let dt = 1.0 / SAMPLE_RATE as f32;
    let mut out = Vec::with_capacity(n_samples);
    let mut phase = 0.0_f32;
    for i in 0..n_samples {
        let t = i as f32 * dt;
        phase += freq * dt;
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
    let total = adsr.total_duration(note_duration).max(0.0);
    let n_samples = (SAMPLE_RATE as f32 * total) as usize;
    let dt = 1.0 / SAMPLE_RATE as f32;
    let mut out = Vec::with_capacity(n_samples);
    let mut carrier_phase = 0.0_f32;
    let mut mod_phase = 0.0_f32;
    for i in 0..n_samples {
        let t = i as f32 * dt;
        mod_phase += carrier_hz * mod_ratio * dt;
        // Modulation index decays exponentially from its peak so the "clang" settles fast.
        let idx = mod_index * (-mod_index_decay * t).exp();
        let modulator = (std::f32::consts::TAU * mod_phase).sin() * idx;
        carrier_phase += carrier_hz * dt;
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
    let hold = sample_hold.max(1);
    let mut held_value = 0.0_f32;
    for (i, s) in samples.iter_mut().enumerate() {
        if i % hold == 0 {
            // Quantize to `levels` steps across -1..1.
            held_value = (s.clamp(-1.0, 1.0) * levels * 0.5).round() / (levels * 0.5);
        }
        *s = held_value;
    }
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
        let gain = target_peak / peak;
        for s in samples.iter_mut() {
            *s = (*s * gain * SATURATION_OVERDRIVE).tanh();
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
        let bell = synth_fm_note(freq, 3.0, 3.5, 22.0, note_duration, &adsr, gain);
        // Layer a quiet triangle-wave sub-oscillator, one octave down, under the FM bell using
        // the plain additive synth — gives the ping a bit of chip-tune "body" beneath the bright
        // FM overtones, so it doesn't sound like a bare sine/FM blip.
        let body = synth_note(Waveform::Triangle, freq * 0.5, note_duration, &adsr, gain * 0.35);
        let offset = (SAMPLE_RATE as f32 * note_spacing * i as f32) as usize;
        mix_into(&mut mix, &bell, offset);
        mix_into(&mut mix, &body, offset);
    }

    // Retro FX chain: mild bitcrush for chip-tune grit, then compress to glue the overlapping
    // notes together and keep the peak of the run under control, then saturate to full loudness.
    bitcrush(&mut mix, 10, 2);
    compress(&mut mix, 0.5, 3.0, 0.002, 0.08);
    normalize_and_saturate(&mut mix, 0.9);

    let pcm: Vec<i16> = mix.iter().map(|&s| (s * i16::MAX as f32) as i16).collect();
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
    let mut pcm: Vec<i16> = Vec::with_capacity(n_samples);

    // Integrate the (falling) instantaneous frequency into a continuous phase so there's no click
    // from a discontinuity when the pitch slides. Phase in radians, advanced each sample.
    let mut phase = 0.0_f32;
    let dt = 1.0 / SAMPLE_RATE as f32;
    for i in 0..n_samples {
        let t = i as f32 * dt;
        let progress = t / duration; // 0..1 across the hit

        // Pitch drop: exponential glide from start_hz to end_hz feels punchier than linear.
        let freq = end_hz + (start_hz - end_hz) * (-6.0 * progress).exp();
        phase += std::f32::consts::TAU * freq * dt;

        // Amplitude: fast exponential decay so the hit is percussive, plus a very short attack
        // ramp (first ~2ms) to avoid a hard click at sample 0.
        let attack = (t / 0.002).min(1.0);
        let env = attack * (-5.0 * progress).exp();

        let sample = phase.sin() * env * gain;
        // Soft clip for a hair of drive/warmth, then to 16-bit range.
        let driven = (sample * 1.4).tanh();
        pcm.push((driven * i16::MAX as f32) as i16);
    }

    encode_wav_mono16(&pcm)
}

/// Synthesise a snare hit: filtered noise burst with a brief tonal body (200 Hz sine), giving
/// the classic crack-and-sizzle of a snare without any sample files.
///
/// * `duration` — total length (~0.09s = tight snare crack).
/// * `gain` — peak amplitude 0..1.
fn synth_snare_wav(duration: f32, gain: f32) -> Vec<u8> {
    let n_samples = (SAMPLE_RATE as f32 * duration) as usize;
    let mut pcm: Vec<i16> = Vec::with_capacity(n_samples);

    // Simple LCG for deterministic noise without pulling in rand here.
    let mut rng_state: u32 = 0xdeadbeef;
    let mut white_noise = || -> f32 {
        rng_state = rng_state.wrapping_mul(1664525).wrapping_add(1013904223);
        // Map u32 to -1..1
        (rng_state as i32 as f32) / i32::MAX as f32
    };

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
        let noise_in = white_noise();
        let hp = rc * (hp_prev_out + noise_in - hp_prev_in);
        hp_prev_in = noise_in;
        hp_prev_out = hp;

        // Mix: 30% tone crack + 70% sizzle noise, under the shared envelope.
        let sample = (0.3 * tone + 0.7 * hp) * env * gain;
        let driven = (sample * 1.2).tanh();
        pcm.push((driven * i16::MAX as f32) as i16);
    }

    encode_wav_mono16(&pcm)
}

/// Wrap 16-bit mono PCM samples in a canonical 44-byte WAV header (PCM format code 1). Must be
/// byte-exact or rodio's decoder rejects it and the `Source` fails to build.
/// A crisp hi-hat click — white noise with a very short exponential decay.
/// Used for the B-key "jam emote" so the player crab can vibe.
pub fn synth_hihat(ctx: &mut Context) -> GameResult<Source> {
    use rand::Rng;
    let n = (SAMPLE_RATE as f32 * 0.08) as usize; // 80ms
    let mut rng = rand::rng();
    let mut pcm: Vec<i16> = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / SAMPLE_RATE as f32;
        let noise: f32 = rng.random_range(-1.0_f32..1.0_f32);
        // Short sharp decay + high-pass via mixing two noise phases
        let env = (-80.0 * t).exp();
        let v = noise * env * 0.55;
        pcm.push((v * 18000.0) as i16);
    }
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

/// Synthesise a looping low rumble for the NPC King Crab conga train.
///
/// The bed is still a low 80 Hz growl, but instead of a smooth electronic tremolo it uses an
/// uneven "tippy-tappy" stutter envelope per half-second cycle (two quick pulses, short breath,
/// then two heavier pulses). A faint high-click layer adds claw-ish texture while keeping the
/// overall tone ominous.
/// Wrapped in a WAV so `Source::from_data` / rodio can decode it normally.
/// The caller sets `repeat(true)` so it loops; volume is driven by distance each frame.
pub fn synth_king_crab_rumble(ctx: &mut Context) -> GameResult<Source> {
    let n = (SAMPLE_RATE as f32 * 0.5) as usize;
    let mut pcm: Vec<i16> = Vec::with_capacity(n);
    let dt = 1.0 / SAMPLE_RATE as f32;
    // Pulse starts within each 0.5s loop: ti-ppy ... ta-ppy
    let pulse_starts = [0.00_f32, 0.065_f32, 0.225_f32, 0.305_f32];
    for i in 0..n {
        let t = i as f32 * dt;
        let mut gate = 0.08_f32;
        for &start in &pulse_starts {
            let d = t - start;
            if d >= 0.0 {
                // Quick attack / short decay per pulse, stronger on the latter pair.
                let amp = if start < 0.2 { 0.65 } else { 0.95 };
                gate += amp * (1.0 - (-95.0 * d).exp()) * (-18.0 * d).exp();
            }
        }

        let rumble = 0.62 * (std::f32::consts::TAU * 80.0 * t).sin()
            + 0.26 * (std::f32::consts::TAU * 121.0 * t).sin()
            + 0.16 * (std::f32::consts::TAU * 173.0 * t).sin();
        // Subtle, short high component so pulses read as "claw taps" instead of pure hum.
        let tap = 0.12 * (std::f32::consts::TAU * 420.0 * t).sin();
        let v = ((rumble + tap) * gate * 0.72).tanh();
        pcm.push((v * 17000.0) as i16);
    }
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
