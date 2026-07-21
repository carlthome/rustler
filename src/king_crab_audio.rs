//! Procedural audio synthesis for the King Crab boss and its NPC conga trains.
//!
//! Split out from `sounds.rs` — a cohesive cluster of rumble/ambient generators that all
//! share the same xorshift-noise shell-click / claw-snap / mandible-chitter synthesis
//! recipe, just tuned for different contexts (boss vs. ambient NPC train, near vs. far).

use crate::constants::BEAT_INTERVAL;
use crate::sounds::{
    SAMPLE_RATE, Waveform, encode_wav_mono16, encode_wav_stereo16, master_limiter,
    oscillator_sample, samples_to_pcm,
};
use ggez::audio::{SoundData, SoundSource, Source};
use ggez::{Context, GameResult};

/// Generate the raw mono sample buffer for the King Crab boss rumble.
///
/// Shared by all spatial variants (left-panned, right-panned, soft/far) so they all
/// sound like the same creature. Returns `f32` samples in -1..1 before any panning or
/// brightness shaping, so the callers can apply L/R gain independently.
fn king_crab_rumble_mono_samples() -> Vec<f32> {
    // Exactly one game bar. The claw snaps below land on beats 1 and 3, so this
    // texture shares the master grid rather than carrying an independent tempo.
    let loop_len = BEAT_INTERVAL * 4.0;
    let n = (SAMPLE_RATE as f32 * loop_len) as usize;
    let dt = 1.0 / SAMPLE_RATE as f32;
    let mut samples = vec![0.0_f32; n];

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

    // Low ambient rumble bed — tuned to A (110 Hz) and its harmonics so it sits in
    // the groove's A minor key rather than clashing with it.
    for i in 0..n {
        let t = i as f32 * dt;
        let breathe = 0.55 + 0.25 * (0.7 * t * std::f32::consts::TAU).sin();
        let rumble = 0.55 * oscillator_sample(Waveform::Triangle, 55.0 * t)   // A1 sub
            + 0.28 * oscillator_sample(Waveform::Rect(0.5), 110.0 * t)        // A2
            + 0.12 * oscillator_sample(Waveform::Triangle, 165.0 * t); // E3 (perfect fifth)
        samples[i] += rumble * breathe * 0.35;
    }

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
            let env = (1.0 - (-900.0 * t).exp()) * (-38.0 / dur_s.max(0.001) * t).exp();
            let mut x = *rng_state;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *rng_state = x;
            let raw = (x as f32) / (u32::MAX as f32) * 2.0 - 1.0;
            let hp = raw - prev_noise * 0.85;
            prev_noise = raw;
            let ring = (std::f32::consts::TAU * carrier_hz * t).sin();
            let v = (noise_mix * hp + (1.0 - noise_mix) * ring) * env * amp;
            let idx = (at + k) % samples.len();
            samples[idx] += v;
        }
    };

    let add_claw_snap =
        |samples: &mut Vec<f32>, at: usize, start_hz: f32, end_hz: f32, dur_s: f32, amp: f32| {
            let dur_n = (SAMPLE_RATE as f32 * dur_s) as usize;
            let mut phase = 0.0_f32;
            for k in 0..dur_n {
                let t = k as f32 * dt;
                let slide = (t / dur_s).min(1.0);
                let freq = start_hz + (end_hz - start_hz) * slide;
                phase += freq * dt;
                let env = (1.0 - (-500.0 * t).exp()) * (-28.0 * t).exp();
                let body = (phase * std::f32::consts::TAU).sin();
                let bite = 0.35 * (phase * 2.0 * std::f32::consts::TAU).sin();
                let idx = (at + k) % samples.len();
                samples[idx] += (body + bite) * env * amp;
            }
        };

    // Shell-click pings.
    let mut t_cursor = 0.02_f32;
    while t_cursor < loop_len - 0.05 {
        let at = (t_cursor * SAMPLE_RATE as f32) as usize;
        let carrier = 1800.0 + rand01(&mut rng_state) * 1700.0;
        let dur = 0.008 + rand01(&mut rng_state) * 0.012;
        let amp = 0.18 + rand01(&mut rng_state) * 0.14;
        add_click(&mut samples, at, dur, carrier, 0.75, amp, &mut rng_state);
        let r = rand01(&mut rng_state);
        let gap = if r > 0.85 {
            0.18 + rand01(&mut rng_state) * 0.20
        } else {
            0.05 + rand01(&mut rng_state) * 0.12
        };
        t_cursor += gap;
    }

    // Claw snaps — 2 per bar, on beats 1 and 3 (half-tempo feel).
    let beat = loop_len / 4.0;
    let snap_times = [beat * 0.05, beat * 2.05];
    for &st in &snap_times {
        let at = (st * SAMPLE_RATE as f32) as usize;
        let start_hz = 320.0 + rand01(&mut rng_state) * 180.0;
        let end_hz = start_hz * (0.55 + rand01(&mut rng_state) * 0.15);
        let dur = 0.030 + rand01(&mut rng_state) * 0.020;
        let amp = 0.16 + rand01(&mut rng_state) * 0.08;
        add_claw_snap(&mut samples, at, start_hz, end_hz, dur, amp);
    }

    // Mandible chitter — one sparse burst per bar on beat 3.
    let chitter_starts = [beat * 2.4];
    for &burst_start in &chitter_starts {
        let click_count = 3 + (rand01(&mut rng_state) * 3.0) as usize;
        let burst_span = 0.04 + rand01(&mut rng_state) * 0.02;
        let pitch_centre = 2000.0 + rand01(&mut rng_state) * 800.0;
        for c in 0..click_count {
            let frac = c as f32 / (click_count.max(1) as f32);
            let jitter = (rand01(&mut rng_state) - 0.5) * 0.004;
            let t_click = burst_start + frac * burst_span + jitter;
            if t_click <= 0.0 || t_click >= loop_len - 0.01 {
                continue;
            }
            let at = (t_click * SAMPLE_RATE as f32) as usize;
            let carrier = pitch_centre * (0.85 + rand01(&mut rng_state) * 0.3);
            add_click(
                &mut samples,
                at,
                0.004 + rand01(&mut rng_state) * 0.003,
                carrier,
                0.65,
                0.08 + rand01(&mut rng_state) * 0.05,
                &mut rng_state,
            );
        }
    }

    for v in samples.iter_mut() {
        *v = (*v * 0.85).tanh();
    }
    samples
}

/// Build a stereo-panned "near" version of the King Crab boss rumble.
///
/// `pan` is -1.0 (hard left) to +1.0 (hard right). Uses equal-power panning so the
/// total loudness stays constant across the field. This is the bright, harmonics-rich
/// version — used when the boss is close to the player.
fn synth_king_crab_rumble_panned(ctx: &mut Context, pan: f32) -> GameResult<Source> {
    let samples = king_crab_rumble_mono_samples();
    // Equal-power panning: map -1..+1 → 0..π/2, then cos/sin.
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    let gain_l = angle.cos();
    let gain_r = angle.sin();
    let left: Vec<f32> = samples.iter().map(|&s| s * gain_l).collect();
    let right: Vec<f32> = samples.iter().map(|&s| s * gain_r).collect();
    let wav = encode_wav_stereo16(&left, &right);
    let data = SoundData::from_bytes(&wav)?;
    let mut src = Source::from_data(ctx, data)?;
    src.set_repeat(true);
    Ok(src)
}

/// Build the "far/soft" version of the King Crab boss rumble.
///
/// Brightness rolloff: only the low-frequency rumble bed survives — the high-frequency
/// shell clicks and claw snaps are filtered out by blending toward a pure sine-based
/// approximation of the low rumble. A 38 ms comb-filter echo (a quieter copy of the
/// signal delayed one loop-sample slot) is baked in to suggest room acoustics.
/// This version is centered (equal L/R) and crossfades in as the boss moves away.
fn synth_king_crab_rumble_soft(ctx: &mut Context) -> GameResult<Source> {
    let loop_len = 2.0_f32;
    let n = (SAMPLE_RATE as f32 * loop_len) as usize;
    let dt = 1.0 / SAMPLE_RATE as f32;
    let mut samples = vec![0.0_f32; n];

    // Soft version: low rumble bed only (sine-heavy, no high-frequency transients).
    for i in 0..n {
        let t = i as f32 * dt;
        let breathe = 0.55 + 0.25 * (0.7 * t * std::f32::consts::TAU).sin();
        // Pure sine fundamentals — no Rect/Triangle harmonics so it sounds muffled/distant.
        let rumble = 0.65 * (std::f32::consts::TAU * 78.0 * t).sin()
            + 0.25 * (std::f32::consts::TAU * 119.0 * t).sin()
            + 0.10 * (std::f32::consts::TAU * 167.0 * t).sin();
        samples[i] += rumble * breathe * 0.30;
    }

    // Bake a 38 ms comb-filter echo (room sense) — a quieter copy of each sample mixed
    // in 38 ms later. This makes the distant rumble feel like it's bouncing off walls
    // rather than playing in a dead space. 38 ms ≈ a small room reflection.
    let echo_delay_samples = (0.038 * SAMPLE_RATE as f32) as usize;
    let echo_gain = 0.28_f32;
    let samples_clone = samples.clone();
    for i in 0..n {
        let echo_src = if i >= echo_delay_samples {
            samples_clone[i - echo_delay_samples]
        } else {
            // Wrap from end of loop for seamless echo at the loop boundary.
            samples_clone[n - echo_delay_samples + i]
        };
        samples[i] += echo_src * echo_gain;
    }

    for v in samples.iter_mut() {
        *v = (*v * 0.85).tanh();
    }

    // Soft version doesn't benefit from bit-crush — use 16-bit to preserve warmth.
    master_limiter(&mut samples);
    // Centered stereo (equal L/R).
    let left = samples.clone();
    let wav = encode_wav_stereo16(&left, &samples);
    let data = SoundData::from_bytes(&wav)?;
    let mut src = Source::from_data(ctx, data)?;
    src.set_repeat(true);
    Ok(src)
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
    let mut samples = king_crab_ambient_mono_samples();
    // Convert to PCM. Milder bit-crush (8-bit) than before — the taps rely on transient
    // detail that heavy crushing would smear.
    let pcm = samples_to_pcm(&mut samples, 8, 1);
    let wav = encode_wav_mono16(&pcm);
    let data = SoundData::from_bytes(&wav)?;
    let mut src = Source::from_data(ctx, data)?;
    src.set_repeat(true);
    Ok(src)
}

/// Generate the raw mono sample buffer for the ambient NPC King Crab conga train.
///
/// Split out from [`synth_king_crab_rumble`] so the same buffer can also be baked into
/// hard-left / hard-right panned stereo sources (see [`synth_king_crab_ambient_spatial`]),
/// giving the ambient train the same directional pan the boss rumble already has.
fn king_crab_ambient_mono_samples() -> Vec<f32> {
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

    samples
}

/// Build the hard-left / hard-right panned stereo variants of the ambient NPC King Crab
/// train rumble. The caller drives their volumes per-frame from the train leader's bearing
/// relative to the player (equal-power split), so the train pans left/right as it circles —
/// the directional half of the "heard before seen" radar. Distance swell is applied on top
/// by scaling both channels together.
///
/// ggez 0.9.3 has no per-source pan/filter API, so panning is baked into two sources exactly
/// like the boss rumble (`synth_king_crab_spatial`).
pub fn synth_king_crab_ambient_spatial(ctx: &mut Context) -> GameResult<(Source, Source)> {
    let mono = king_crab_ambient_mono_samples();
    // Hard-left: all signal in L. Hard-right: all signal in R. The per-frame equal-power
    // gains applied by the caller do the actual pan sweep between these two extremes.
    // Raw f32 samples (no bit-crush) exactly like the boss panned rumble.
    let silence = vec![0.0_f32; mono.len()];
    let left_wav = encode_wav_stereo16(&mono, &silence);
    let right_wav = encode_wav_stereo16(&silence, &mono);
    let mut left = Source::from_data(ctx, SoundData::from_bytes(&left_wav)?)?;
    let mut right = Source::from_data(ctx, SoundData::from_bytes(&right_wav)?)?;
    left.set_repeat(true);
    right.set_repeat(true);
    Ok((left, right))
}

/// Build the three spatial variants of the King Crab boss rumble used for spatialization:
/// - `left`: bright version panned hard left (used when boss is to player's left)
/// - `right`: bright version panned hard right (used when boss is to player's right)
/// - `soft`: muffled/far version with room echo, centered (crossfades in with distance)
///
/// The caller drives their volumes per-frame from boss position relative to player, producing
/// distance-based volume rolloff, stereo panning, and brightness rolloff without any runtime
/// filtering (ggez 0.9.3 has no per-source filter API).
pub fn synth_king_crab_spatial(ctx: &mut Context) -> GameResult<(Source, Source, Source)> {
    let left = synth_king_crab_rumble_panned(ctx, -1.0)?;
    let right = synth_king_crab_rumble_panned(ctx, 1.0)?;
    let soft = synth_king_crab_rumble_soft(ctx)?;
    Ok((left, right, soft))
}

/// Synthesise a short, beat-locked musical MOTIF for one ambient NPC King Crab conga train — the
/// per-rival "music" half of the audio scoreboard (INSPIRATION.md agar.io: "the dominant train
/// dominates the mix"). Where [`synth_king_crab_ambient_spatial`] gives each train a creature
/// RUMBLE (sub-bass presence), this layers a melodic arpeggio in A natural-minor on top so a rival
/// train broadcasts actual *music* that harmonises with the action groove.
///
/// `tier` (0 = scout, 1 = wanderer, 2 = elder) picks register, note density and richness: a scout
/// is a faint high pluck, an elder is a low, full, busy motif. `bpm` MUST be the game's live tempo
/// so the baked loop is an exact two-bar length; the caller (re)starts the pair on the beat, which
/// keeps every note in the pocket with no drift (ggez 0.9.3 has no runtime resync, so a bar-length
/// buffer + start-on-beat IS the lock).
///
/// Returned as a hard-left / hard-right stereo pair exactly like [`synth_king_crab_ambient_spatial`]
/// so the caller equal-power pans it by the leader's bearing and scales both channels together by
/// distance and train length each frame.
pub fn synth_rival_motif(
    ctx: &mut Context,
    bpm: f32,
    root_midi: i32,
    note_offsets: [i32; 11],
    tier: usize,
) -> GameResult<(Source, Source)> {
    let mono = rival_motif_mono_samples(bpm, root_midi, note_offsets, tier);
    let silence = vec![0.0_f32; mono.len()];
    let left_wav = encode_wav_stereo16(&mono, &silence);
    let right_wav = encode_wav_stereo16(&silence, &mono);
    let mut left = Source::from_data(ctx, SoundData::from_bytes(&left_wav)?)?;
    let mut right = Source::from_data(ctx, SoundData::from_bytes(&right_wav)?)?;
    left.set_repeat(true);
    right.set_repeat(true);
    Ok((left, right))
}

#[derive(Clone, Copy)]
enum PirateVoice {
    TinWhistle,
    Concertina,
    PluckedString,
}

fn midi_to_hz(midi: i32) -> f32 {
    440.0 * 2.0_f32.powf((midi as f32 - 69.0) / 12.0)
}

fn rival_note_bank(root_midi: i32, note_offsets: [i32; 11]) -> [f32; 11] {
    note_offsets.map(|offset| midi_to_hz(root_midi + offset))
}

#[allow(clippy::too_many_arguments)]
fn add_pirate_note(
    samples: &mut [f32],
    start: usize,
    duration: usize,
    hz: f32,
    voice: PirateVoice,
    gain: f32,
    sample_rate: f32,
) {
    for k in 0..duration {
        let t = k as f32 / sample_rate;
        let attack = (1.0 - (-180.0 * t).exp()).clamp(0.0, 1.0);
        let (tone, decay) = match voice {
            // Breath-like fundamental with gentle vibrato: a small pirate whistle, not a sine beep.
            PirateVoice::TinWhistle => {
                let phase = std::f32::consts::TAU * hz * t
                    + 0.035 * (std::f32::consts::TAU * 5.2 * t).sin();
                (
                    phase.sin() + (phase * 2.0).sin() * 0.16 + (phase * 3.0).sin() * 0.05,
                    3.0,
                )
            }
            // Additive reed harmonics and a slower envelope evoke a compact concertina.
            PirateVoice::Concertina => {
                let phase = std::f32::consts::TAU * hz * t;
                (
                    phase.sin()
                        + (phase * 2.0).sin() * 0.42
                        + (phase * 3.0).sin() * 0.24
                        + (phase * 4.0).sin() * 0.10,
                    2.0,
                )
            }
            // Quickly decaying harmonics give a woody mandolin/bouzouki-like pluck.
            PirateVoice::PluckedString => {
                let phase = std::f32::consts::TAU * hz * t;
                (
                    phase.sin() + (phase * 2.0).sin() * 0.38 + (phase * 3.0).sin() * 0.20,
                    5.5,
                )
            }
        };
        let env = attack * (-decay * t).exp();
        // Fold release tails across the loop boundary instead of clipping their decay.
        samples[(start + k) % samples.len()] += tone * env * gain;
    }
}

/// Bake one rival train's two-bar pirate motif at the master tempo and player key.
fn rival_motif_mono_samples(
    bpm: f32,
    root_midi: i32,
    note_offsets: [i32; 11],
    tier: usize,
) -> Vec<f32> {
    let beat_s = 60.0 / bpm.clamp(40.0, 220.0);
    let step_s = beat_s / 4.0; // 16th-note grid
    const STEPS: usize = 32; // 2 bars x 16 sixteenths
    let loop_len = step_s * STEPS as f32;
    let n = (SAMPLE_RATE as f32 * loop_len).ceil() as usize;
    let mut samples = vec![0.0_f32; n];
    let notes = rival_note_bank(root_midi, note_offsets);

    // Player-key note bank. Index legend at the default A root:
    // 0:A2 1:C3 2:E3 3:G3 4:A3 5:C4 6:E4 7:G4 8:A4 9:C5 10:E5
    let add_note = |samples: &mut Vec<f32>,
                    at_step: usize,
                    len_steps: f32,
                    note_hz: f32,
                    voice: PirateVoice,
                    amp: f32| {
        let start = (at_step as f32 * step_s * SAMPLE_RATE as f32) as usize;
        let dur_n = (len_steps * step_s * SAMPLE_RATE as f32) as usize;
        add_pirate_note(
            samples,
            start,
            dur_n,
            note_hz,
            voice,
            amp,
            SAMPLE_RATE as f32,
        );
    };

    match tier {
        0 => {
            // Scout — a sparse high tin-whistle call.
            let pat = [(0usize, 8usize), (6, 10), (12, 9), (20, 8), (26, 10)];
            for &(s, ni) in &pat {
                add_note(
                    &mut samples,
                    s,
                    2.0,
                    notes[ni],
                    PirateVoice::TinWhistle,
                    0.12,
                );
            }
        }
        1 => {
            // Wanderer — a jaunty mid-register concertina phrase on the offbeat pulse only, leaving
            // space for the player's hook instead of doubling its fast 16th-note movement.
            let pat = [
                (0usize, 4usize),
                (4, 7),
                (8, 6),
                (12, 3),
                (16, 4),
                (20, 7),
                (24, 7),
                (28, 2),
            ];
            for &(s, ni) in &pat {
                add_note(
                    &mut samples,
                    s,
                    1.6,
                    notes[ni],
                    PirateVoice::Concertina,
                    0.09,
                );
            }
        }
        _ => {
            // Elder — a woody low bouzouki pulse under a full concertina answer. Its quarter-note
            // arpeggio gives the largest rival presence without becoming a competing scale run.
            let bass = [(0usize, 0usize), (8, 2), (16, 0), (24, 3)]; // A2 E3 A2 G3
            for &(s, ni) in &bass {
                add_note(
                    &mut samples,
                    s,
                    8.0,
                    notes[ni],
                    PirateVoice::PluckedString,
                    0.24,
                );
            }
            let arp = [
                (0usize, 4usize),
                (4, 8),
                (8, 2),
                (12, 8),
                (16, 4),
                (20, 8),
                (24, 3),
                (28, 8),
            ];
            for &(s, ni) in &arp {
                add_note(
                    &mut samples,
                    s,
                    1.8,
                    notes[ni],
                    PirateVoice::Concertina,
                    0.08,
                );
            }
        }
    }

    // Soft-clip so summed voices never wrap; leaves headroom for the per-frame volume scaling.
    for v in samples.iter_mut() {
        *v = (*v * 0.9).tanh();
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rival_motifs_are_exactly_two_bars() {
        let bpm = 120.0;
        let samples = rival_motif_mono_samples(
            bpm,
            57,
            crate::sounds::biome_rival_motif_tuning(crate::levels::BiomeMusic::SunnyGroove).1,
            2,
        );
        let expected = (SAMPLE_RATE as f32 * 8.0 * 60.0 / bpm).ceil() as usize;
        assert_eq!(samples.len(), expected);
    }

    #[test]
    fn rival_note_bank_transposes_with_player_key() {
        let offsets =
            crate::sounds::biome_rival_motif_tuning(crate::levels::BiomeMusic::SunnyGroove).1;
        let a_minor = rival_note_bank(57, offsets);
        let b_minor = rival_note_bank(59, offsets);
        let ratio = 2.0_f32.powf(2.0 / 12.0);
        for (a, b) in a_minor.into_iter().zip(b_minor) {
            assert!((b / a - ratio).abs() < 1e-5);
        }
    }

    #[test]
    fn pirate_motifs_remain_bounded() {
        let offsets =
            crate::sounds::biome_rival_motif_tuning(crate::levels::BiomeMusic::SunnyGroove).1;
        for tier in 0..3 {
            let samples = rival_motif_mono_samples(120.0, 57, offsets, tier);
            assert!(samples.iter().all(|sample| sample.is_finite()));
            assert!(samples.iter().all(|sample| sample.abs() <= 1.0));
        }
    }
}
