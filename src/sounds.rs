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

/// The synthesised percussion voices, built once and replayed on the beat.
pub struct BeatSynth {
    /// The heavier, lower kick for the downbeat ("1" of the bar).
    downbeat_kick: Source,
    /// The lighter kick for the three beats between downbeats.
    offbeat_kick: Source,
}

impl BeatSynth {
    pub fn new(ctx: &mut Context) -> GameResult<BeatSynth> {
        Ok(BeatSynth {
            // Downbeat: lower, longer, louder — the "1" you feel in your chest.
            downbeat_kick: kick_source(ctx, 150.0, 45.0, 0.14, 0.9)?,
            // Offbeat: higher pitched, tighter, quieter so the bar has a clear accent structure.
            offbeat_kick: kick_source(ctx, 130.0, 55.0, 0.10, 0.55)?,
        })
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
}
