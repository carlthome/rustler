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

use ggez::audio::{Source, SoundData};
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
    let lag_max = (onset_rate * 60.0 / 60.0).round() as usize;  // 60 BPM
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
