//! The per-beat handler: everything that fires once each time the master beat clock crosses
//! a beat onset — the synthesised kick/hat groove, the downbeat accent and bar structure,
//! beat-synced spawns and lures, groove/combo bookkeeping, and the on-beat visual punches.
//!
//! Extracted verbatim from the giant `MainState::update` in `main.rs` (it was the single
//! `if self.beat_timer <= 0.0 { ... }` block) into one `impl MainState` method to keep that
//! file navigable. Pure structural move — no behaviour change; the caller still owns the
//! `beat_timer` countdown and only invokes this when a beat actually lands.

use ggez::Context;
use ggez::glam::Vec2;
use rand::Rng;

use crate::*;

pub(crate) fn downbeat_started(beat_count: u32, beat_timer: f32, beat_interval: f32) -> bool {
    beat_interval > 1e-4
        && beat_count % 4 == 0
        && beat_timer <= beat_interval
        && beat_timer > beat_interval - BEAT_WINDOW
}

#[cfg(test)]
mod tests {
    use super::downbeat_started;

    #[test]
    fn loop_start_gate_only_opens_after_bar_downbeat() {
        assert!(downbeat_started(4, 0.49, 0.5));
        assert!(!downbeat_started(4, 0.01, 0.5));
        assert!(!downbeat_started(3, 0.49, 0.5));
        assert!(!downbeat_started(4, 0.51, 0.5));
    }
}

impl MainState {
    /// Advances the master groove from undilated frame time.
    ///
    /// Hitstop and cinematic slow-motion freeze the simulation, not the backing track. Keeping
    /// this clock ahead of those early returns prevents repeated catches and dashes from letting
    /// the live kick/snare grid fall behind the looping melody.
    pub(crate) fn update_master_beat(&mut self, ctx: &mut Context, dt: f32) {
        if self.beat_interval <= 1e-4 {
            return;
        }

        let frac = (1.0 - self.beat_timer / self.beat_interval).clamp(0.0, 1.0);
        let train_fill = (self.chain_count as f32 / 24.0).clamp(0.0, 1.0);
        let stage_span = (INTENSITY_TIERS.len().saturating_sub(1)).max(1) as f32;
        let stage_fill = (self.intensity_tier as f32 / stage_span).clamp(0.0, 1.0);
        let busy = self.chain_count >= 8 || self.intensity_tier >= 1;
        let base_vol = 0.26 + 0.16 * train_fill + 0.10 * stage_fill;
        let swing_late = crate::sounds::GROOVE_SWING * 0.125;
        for local in 1..=3u32 {
            let onset = local as f32 * 0.25 + if local % 2 == 1 { swing_late } else { 0.0 };
            let gstep = self.beat_count as i64 * 4 + local as i64;
            if frac + 1e-6 >= onset && gstep > self.hat_last_step {
                self.hat_last_step = gstep;
                if local == 2 {
                    self.beat_synth.play_hihat(ctx, base_vol);
                } else if busy {
                    self.beat_synth.play_hihat(ctx, base_vol * 0.55);
                }
            }
        }

        self.beat_timer -= dt;
        while self.beat_timer <= 0.0 {
            self.on_beat(ctx);
        }
    }

    /// Runs once per beat, immediately after `beat_timer` wraps. `ctx` is needed for the
    /// synthesised percussion voices; all other state lives on `self`.
    pub(crate) fn on_beat(&mut self, ctx: &mut Context) {
        self.beat_timer += self.beat_interval;
        self.beat_intensity = 1.0;
        self.beat_count = self.beat_count.wrapping_add(1);
        let downbeat = self.beat_count % 4 == 0;
        // Visceral beat: thump a synthesised kick drum on every beat so the tempo is *felt*,
        // not just seen. The heavier, lower voice lands on the downbeat so the bar has a clear
        // accent structure. This block only runs during live gameplay (the update guard returns
        // early on menu/upgrade/game-over screens), so the kick never thumps through menus.
        self.beat_synth.play_kick(ctx, downbeat);
        // Keep the music loop tempo- AND phase-locked to the master beat clock. The intensity
        // ramp speeds the clock up (`beat_interval = BEAT_INTERVAL / tempo_mul`), but the groove
        // is a pre-baked loop that can't re-pitch itself — so without this it drifts off the beat
        // exactly when the party peaks (the reported bug). We DJ it: the playback speed the loop
        // needs to line back up is `BEAT_INTERVAL / beat_interval` (= the stage's tempo_mul), so
        // we turntable it to match. `set_pitch` only bites on the next `play()`, and we restart
        // ON the downbeat so the loop's bar-1 re-anchors to the grid's "1" — phase-aligned, not
        // just tempo-matched. It only fires at the 4 stage boundaries (a fresh "drop" as the run
        // escalates), never per-beat. Null-audio-safe (no-ops on a headless device) and
        // deterministic (stage transitions are), so the bots see byte-identical behaviour.
        if downbeat && self.beat_interval > 1e-4 {
            let desired_pitch = INTENSITY_TIERS[self.intensity_tier.min(INTENSITY_TIERS.len() - 1)]
                .3
                .clamp(0.5, 3.0);
            if (desired_pitch - self.music_pitch).abs() > 1e-3 {
                let old_interval = self.beat_interval;
                self.beat_interval = BEAT_INTERVAL / desired_pitch;
                self.beat_timer *= self.beat_interval / old_interval;
                self.music_pitch = desired_pitch;
                let active_music = self.action_music_index();
                for (index, music) in self.sounds.action_music.iter_mut().enumerate() {
                    let was_playing = index == active_music && music.playing();
                    music.set_pitch(desired_pitch);
                    if was_playing {
                        let _ = music.play();
                    }
                }
                for layer in self.music_layers.iter_mut() {
                    let was_playing = layer.playing();
                    layer.set_pitch(desired_pitch);
                    if was_playing {
                        let _ = layer.play();
                    }
                }
                for (left, right) in self.sounds.king_crab_motif.iter_mut() {
                    let was_playing = left.playing() || right.playing();
                    left.set_pitch(desired_pitch);
                    right.set_pitch(desired_pitch);
                    if was_playing {
                        let _ = left.play();
                        let _ = right.play();
                    }
                }
            }
        }
        // Snare: fades in on the backbeat (beats 2 & 4) while a boss is alive, raising the
        // stakes audibly as the fight escalates. Fades back out once the boss is caught.
        let boss_present = self.crabs.iter().any(|c| c.is_boss() && !c.caught);
        self.beat_synth.update_snare_volume(boss_present);
        self.beat_synth.play_snare(ctx, self.beat_count);
        // On-beat catch bloom: every beat the train's catch window blooms wide, then settles back
        // before the next hit (decayed in update_crabs). The downbeat blooms hardest so the "1"
        // is the widest scoop of the bar — a groove-savvy player learns to cross a drifting crab
        // exactly on the beat to hoover it in, while an off-beat pass just misses. This reshapes
        // ordinary catching around the bar without adding a new key to press.
        self.beat_catch_bloom = if downbeat { 30.0 } else { 20.0 };
        // Downbeat herd pulse: on the "1" of the bar, nudge the whole free herd toward the
        // player so the beat itself becomes a routing tool. Light it up only on the downbeat so
        // it reads as a rhythmic thump, not a constant tug; the impulse is applied per-crab in
        // update_crabs and decays over the frames after. Captured center drives the visual ring.
        if downbeat {
            self.downbeat_pull = 1.0;
            let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            self.downbeat_pull_center = center;
            // Count the herd this downbeat is actually about to sweep — free, un-spooked crabs
            // inside the same 300px radius the per-crab pull uses — so the ring's flare reflects
            // real routing payoff, not just that a beat happened. Normalized against a "full
            // scoop" of ~10 crabs; standing in a fat loose herd on the "1" flares the ring gold.
            let swept = self
                .crabs
                .iter()
                .filter(|c| {
                    !c.caught
                        && !c.is_boss()
                        && c.startle_timer <= 0.0
                        && c.charm_timer <= 0.0
                        && c.magnet_snared <= 0.0
                        && c.pos.distance_squared(center) < 300.0 * 300.0
                })
                .count();
            self.downbeat_pull_haul = (swept as f32 / 10.0).clamp(0.0, 1.0);
        }
        // Drum Roll: if T is being held as this beat fires, bank a roll hit (the charge). The
        // beat handler runs at most once per beat, so a held key naturally counts exactly one
        // hit per beat. A hit kicks a tick of feedback (beat flash + a bump of groove) so each
        // roll lands audibly/visibly, building tension toward the release blast. The held flag
        // is set by the update poll before update_crabs, so it's current for this beat.
        if self.drum_roll_held {
            self.drum_roll_hits = (self.drum_roll_hits + 1).min(DRUM_ROLL_MAX);
            self.on_beat_flash = (self.on_beat_flash + 0.2).min(0.7);
            self.groove = (self.groove + 0.05).min(1.0);
            let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            self.floating_texts.spawn(
                "ROLL!".to_string(),
                center - Vec2::new(28.0, 96.0),
                22.0 + self.drum_roll_hits as f32 * 3.0,
                [1.0, 0.8, 0.4, 1.0],
            );
        }
        // Reef DJ call-and-response: on every downbeat while the rhythm boss is on the field,
        // it CALLS a fresh phrase for the coming bar — a random subset of the four beats that
        // are "hot" (its shell is only vulnerable on those). Rolled once per bar, always with
        // at least one hot beat and never all four, so there's a pattern to read and echo back
        // rather than a constant open window. The downbeat is always hot so the "1" anchors the
        // phrase and reads as the boss's call.
        if downbeat && self.reef_active {
            let bar = self.beat_count / 4;
            if bar != self.reef_phrase_bar {
                self.reef_phrase_bar = bar;
                let mut rng = crate::rng::rng();
                let mut phrase = [false; 4];
                phrase[0] = true; // the "1" always calls, anchoring the bar
                for slot in phrase.iter_mut().skip(1) {
                    *slot = rng.random_bool(0.4);
                }
                self.reef_phrase = phrase;
            }
        }
        // Groove Call response: while a call is live, the herd LUNGES toward the player on each
        // beat and drifts between — kick the surge envelope here so the field-wide pull (applied
        // in update_crabs) pulses to the bar. Bars of response are spent one per downbeat, so a
        // clean 2-bar call unfolds over eight beats before the herd relaxes. The downbeat surge
        // lands hardest so the "1" is the big group lunge — the watchable, on-the-beat gather.
        if self.groove_call_bars > 0.0 {
            self.groove_call_surge = if downbeat { 1.0 } else { 0.7 };
            self.groove_call_pulse = if downbeat { 1.0 } else { 0.7 };
            // Answer streaks: on each beat of a live call, fling comet trails from free crabs
            // toward the player so the herd-flood reads as an on-the-beat lunge, not just drift.
            // The downbeat throws the big group streak (whole field), the between-beats a lighter
            // one — the "1" is visibly the largest gather. Cyan-tinted to match the call ring.
            let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            let cap = if downbeat { 40 } else { 22 };
            // Nearer, more-susceptible crabs streak more strongly; scale count by call quality.
            let want = ((cap as f32) * self.groove_call_strength.min(1.5)).round() as usize;
            let start = if downbeat { -0.45 } else { -0.2 }; // downbeat streak reads a touch longer
            let mut spawned = 0usize;
            for crab in self.crabs.iter() {
                if spawned >= want || self.call_streaks.len() >= 56 {
                    break;
                }
                if crab.caught
                    || crab.is_boss()
                    || crab.crab_type.whistle_pull() <= 0.0
                    || crab.is_latched()
                {
                    continue;
                }
                let d = center - crab.pos;
                let dist = d.length();
                // Pull radius scales with groove: more groove = wider reach (max 500px).
                let call_reach = 280.0 + self.groove * 220.0;
                if dist < 40.0 || dist > call_reach {
                    continue; // skip crabs on top of the player or too far to read as answering
                }
                // A short streak from the crab pointing at the player — a fixed lead so the tail
                // shows the answering direction without teleporting the crab.
                let head = crab.pos + d.normalize_or_zero() * dist.min(120.0);
                // Cyan call tint, brightened by how eagerly this archetype answers.
                let eager = crab.crab_type.whistle_pull().min(1.0);
                let color = [0.35 + 0.25 * eager, 0.9, 1.0];
                self.call_streaks.push((crab.pos, head, start, color));
                spawned += 1;
            }
            if downbeat {
                self.groove_call_bars -= 1.0;
                // A small groove tick each bar the call keeps working, so leaning on the beat to
                // route the herd is itself rewarded like the other rhythm verbs.
                self.groove = (self.groove + 0.04).min(1.0);
                // Call fully spent this bar — reset the echo phrase so the next call starts fresh.
                if self.groove_call_bars <= 0.0 {
                    self.groove_call_echo = 0;
                }
            }
        }
        // The "1" of the bar lands harder than the three beats between it. Kick the accent so
        // the beat-stepping conga train stomps forward as one on the downbeat (see the step
        // code in update_crabs, which scales its hop by bar_accent), and give a fresh unified
        // squash-pop that ripples down the line so the whole train visibly lands the one.
        if downbeat {
            self.bar_accent = 1.0;
            // Restart the join squash-pop on every caught crab, staggered by chain index so
            // the pop rolls head-to-tail — the same ripple used when a crab joins, reused here
            // as a musical "bar landed" bounce. Cheap: just sets a decaying timer per crab.
            let mut ci = 0.0_f32;
            for crab in self.crabs.iter_mut().filter(|c| c.caught) {
                crab.join_pulse = (1.0 - ci * 0.04).max(0.4);
                ci += 1.0;
            }
        }
        // King Crab finale: the cracked floor GEYSERS on the beat. Kick the eruption pulse so
        // every open fissure spouts molten in time with the music — its danger swells on the
        // hit and recedes in the gap, turning a static pit into a rhythmic hazard the player
        // times crossings against. A tiny extra flare on the downbeat so it groups by the bar.
        if !self.boss_fissures.is_empty() {
            self.boss_fissure_erupt = if downbeat { 1.0 } else { 0.85 };
            self.screen_shake = self.screen_shake.max(if downbeat { 8.0 } else { 5.0 });
            // Spit a few molten sparks up out of each pit so the geyser reads as real debris,
            // not just a glow — capped by the particle system's own budget.
            for &(c, r, age) in self.boss_fissures.iter() {
                if age > 0.6 {
                    self.particle_system
                        .spawn_fissure_geyser(c, r, &mut crate::rng::rng());
                }
            }
        }
        // Every 4th beat, auto-fire beat wave when score >= 20
        if downbeat && self.score >= 20 && !self.beat_wave_active {
            self.beat_wave_active = true;
            self.beat_wave_radius = 0.0;
        }
        // Bar-quantized spawn: an armed wave lands exactly here, on the downbeat, so a fresh
        // herd always arrives in time with the music instead of at an arbitrary tick.
        if downbeat && self.wave_armed {
            self.wave_armed = false;
            self.wave_telegraph = 0.0;
            let was_frenzy = self.frenzy_wave;
            self.advance_wave();
            // Punch the downbeat that births a wave so the arrival reads as a musical hit.
            // A frenzy drop punches noticeably harder — bigger flash, screen shake, and a
            // banner — so the staged spike lands as a genuine event, not just more crabs.
            if was_frenzy {
                self.beat_intensity = 2.0;
                self.on_beat_flash = self.on_beat_flash.max(0.75);
                self.frenzy_banner_timer = 1.6;
                self.screen_shake = self.screen_shake.max(11.0);
                let kick = crate::rng::rng().random_range(0.0_f32..std::f32::consts::TAU);
                self.screen_shake_vel = Vec2::new(kick.cos(), kick.sin()) * 11.0 * 60.0;
                // upgrade.ogg removed — tiresome and crackly; new sound TBD
            } else {
                self.beat_intensity = (self.beat_intensity + 0.6).min(2.0);
                self.on_beat_flash = self.on_beat_flash.max(0.4);
            }
        }
        // Collect caught-crab positions for the beat-pulse sparkle rings just below: both
        // used to run their own separate `.filter(|c| c.caught)` pass over self.crabs (two
        // counts + a fresh Vec::collect() every single beat), so fold them into one pass
        // that reuses the persistent chain_positions_buf (already used later this frame by
        // catch_by_chain, and not read in between) instead of allocating a new Vec.
        self.chain_positions_buf.clear();
        self.chain_positions_buf
            .extend(self.crabs.iter().filter(|c| c.caught).map(|c| c.pos));
        let chain_len = self.chain_positions_buf.len();
        // Beat-pulse sparkle rings from all caught crabs — brighter on the bar downbeat so
        // the "1" of the bar pops harder than the beats between it.
        let pulse_strength = if downbeat { 1.5 } else { 1.0 };
        self.particle_system.spawn_beat_pulse(
            &self.chain_positions_buf,
            pulse_strength,
            chain_len,
            &mut crate::rng::rng(),
        );
        // Spawn ghost rings at each chain crab position. Unlike catch_shockwaves (capped at
        // 48) and fear_rings (capped at 32), this loop had no ceiling — a long conga train
        // (chain_count grows unbounded over a run, see MAX_PARTICLES's comment) would push
        // one ring per caught crab every single beat, each drawing two more mesh draws in
        // draw_chain_rings. Cap it the same way the sibling effect buffers are capped: once
        // the live count hits the ceiling, stop adding for this beat rather than growing
        // without bound. Only affects trains long enough to have hit the cap already.
        const MAX_CHAIN_RINGS: usize = 64;
        for crab in self.crabs.iter().filter(|c| c.caught) {
            if self.chain_rings.len() >= MAX_CHAIN_RINGS {
                break;
            }
            let color = crab.crab_color();
            self.chain_rings.push((crab.pos, 0.0, color));
        }
        // Emergent beat-startle chain reaction: panic ripples crab-to-crab on the pulse.
        self.beat_startle_contagion();

        // Dancer crabs hop on the beat. Between beats they barely drift (their speed_range is
        // low), so their real motion is this quantized leap — making them a rhythm-reading
        // catch: the beat that just fired is exactly when they bolt, so you grab them during
        // the freeze, not mid-leap. Close ones hop away from the player (a rhythmic flee);
        // distant ones keep their heading, wandering in beat-timed skips.
        const DANCER_HOP: f32 = 74.0;
        let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        // Where each *fleeing* (not answering) Dancer landed this beat. A jittery Dancer
        // leaping away from the player is a startle source of its own — its on-beat hop
        // spooks the calm crabs around it (see the ripple pass below). Reuse the scratch
        // buffer rather than allocating a Vec every beat.
        let mut dancer_hops = std::mem::take(&mut self.dancer_hop_scratch);
        dancer_hops.clear();
        for crab in self.crabs.iter_mut() {
            if crab.caught || !crab.is_dancer() {
                continue;
            }
            let dist = player_center.distance(crab.pos);
            // An answering Dancer that's already in arm's reach holds still (its answer is spent)
            // rather than hopping the default fallback direction and skittering off.
            if crab.answering_call > 0.0 && dist < 90.0 {
                crab.answering_call = 0.0;
                crab.join_pulse = 1.0;
                continue;
            }
            let dir = if crab.answering_call > 0.0 {
                // Answering the player's Call: hop TOWARD the player on the beat.
                (player_center - crab.pos).normalize_or_zero()
            } else if dist < 240.0 {
                // Rhythmic flee: leap away from the player.
                (crab.pos - player_center).normalize_or_zero()
            } else {
                // Wander: keep heading, or fall back to current facing if idle.
                let v = crab.vel.normalize_or_zero();
                if v == Vec2::ZERO {
                    Vec2::new(crab.facing_angle.cos(), crab.facing_angle.sin())
                } else {
                    v
                }
            };
            let dir = if dir == Vec2::ZERO {
                Vec2::new(0.0, -1.0)
            } else {
                dir
            };
            crab.pos += dir * DANCER_HOP;
            crab.pos.x = crab.pos.x.clamp(0.0, self.world_width - crab.scale);
            crab.pos.y = crab.pos.y.clamp(0.0, self.world_height - crab.scale);
            crab.vel = dir; // face the hop; unit vel so the drift branch stays gentle
            crab.join_pulse = 1.0; // reuse the join squash-pop as a little "landed" bounce
            // A Dancer bolting away from the player becomes a fear source; note where it
            // landed so the ripple pass below can spook nearby calm crabs. Answering Dancers
            // (hopping toward the player, charmed) don't scare anyone — only fleeing ones do.
            if crab.answering_call <= 0.0 && dist < 240.0 {
                dancer_hops.push(crab.pos);
            }
        }

        // Dancer King evasion + entrancement. Every 2 beats the King TELEPORTS to a mirrored
        // position across the world — its whole defence, since (unlike the shelled bosses) it's
        // catchable from the very first frame. And on every beat it renews its spell: free crabs
        // near it become ENTRANCED and shadow its drift (see the entrancement pass in
        // update_crabs) until the King is caught — catch it exactly ON the beat and the whole
        // spellbound court banks into your train at once (the Perfect Catch payoff).
        let king: Option<(usize, Vec2)> = self.crabs.iter().enumerate().find_map(|(i, c)| {
            if c.is_dancer_king() && !c.caught {
                Some((i, c.pos))
            } else {
                None
            }
        });
        if let Some((ki, kpos)) = king {
            const ENTRANCE_RADIUS_SQ: f32 =
                DANCER_KING_ENTRANCE_RADIUS * DANCER_KING_ENTRANCE_RADIUS;
            for crab in self.crabs.iter_mut() {
                if !crab.caught
                    && !crab.is_boss()
                    && crab.pos.distance_squared(kpos) < ENTRANCE_RADIUS_SQ
                {
                    if crab.entranced <= 0.0 {
                        crab.join_pulse = crab.join_pulse.max(0.8); // a "spellbound" squash-pop as the trance takes
                    }
                    // Holds through a couple of missed beats, so the court stays synchronized even
                    // when the King's own teleport briefly carries it out of range.
                    crab.entranced = self.beat_interval * DANCER_KING_ENTRANCE_BEATS;
                }
            }
            if self.beat_count % 2 == 0 {
                let crab = &mut self.crabs[ki];
                let old = crab.pos;
                // Mirror across the world center — the far side of wherever it just was, so
                // chasing it flatly never works: read the 2-beat cadence and cut it off instead.
                crab.pos = Vec2::new(
                    (self.world_width - old.x).clamp(20.0, self.world_width - 20.0),
                    (self.world_height - old.y).clamp(20.0, self.world_height - 20.0),
                );
                crab.join_pulse = 1.2; // landing pop so the arrival reads
                // Vanish/arrive rings at both ends so the blink is legible, not a lost sprite.
                if self.catch_shockwaves.len() < 48 {
                    self.catch_shockwaves.push((old, 0.0, [1.0, 0.62, 0.45]));
                }
                if self.catch_shockwaves.len() < 48 {
                    self.catch_shockwaves
                        .push((crab.pos, 0.0, [1.0, 0.62, 0.45]));
                }
            }
        }

        // On-beat herd stampede: on the DOWNBEAT (the bar's "1") the whole loose herd lurches
        // forward along its own heading, then coasts through the three off-beats — so *where a
        // free crab will be* becomes a rhythm read. A groove-savvy player reads the surge and
        // slides into the herd's landing spot on the bar rather than chasing crabs flatly; the
        // beat reshapes routing across the whole field, not just around the player. Only the
        // downbeat surges (the off-beats stay a quiet coast) so the "1" reads as the herd's step,
        // matching the heavier downbeat kick drum and bar accent. We only ARM the surge here
        // (kick surge_timer); update_crabs spends it as an extra positional shove that decays
        // over the beat, so the motion eases out instead of teleporting. Excludes anything that
        // already has its own on-beat motion or a reason to hold still: Dancers (their own hop
        // above), bosses, spooked/startled/charmed/answering crabs, snared/lured crabs under a
        // Magnet, and Hermits (their own host-swap hop) — the surge is the *calm* herd's beat-step.
        if downbeat {
            for crab in self.crabs.iter_mut() {
                if crab.caught
                    || crab.is_dancer()
                    || crab.is_boss()
                    || crab.spooked_timer > 0.0
                    || crab.startle_timer > 0.0
                    || crab.charm_timer > 0.0
                    || crab.answering_call > 0.0
                    || crab.magnet_snared > 0.0
                    || crab.thief_lured > 0.0
                    || crab.is_hermit()
                {
                    continue;
                }
                crab.surge_timer = 1.0;
            }
        }

        // Emergent interaction: a fleeing Dancer's on-beat hop ripples out into five separate
        // effects depending on what it lands near — startling a calm crab, jolting a latched
        // Thief loose, staggering a bolting Golden, chipping an Armored crab's shell, or kicking
        // a roaming Magnet into a pull surge. These used to be five independent
        // `self.crabs.iter_mut()` passes, each rebuilding the same grid-lookup closure and
        // re-scanning the whole herd — on a long train that's 5x redundant O(n) work every
        // single beat. Since the five target predicates (calm non-Dancer / free latched Thief /
        // free Golden / free Armored-with-shell / free Magnet) are mutually exclusive per crab,
        // fold them into one pass over self.crabs that dispatches by crab type, sharing one grid
        // lookup and one nearest/hit search per crab instead of up to five.
        if !dancer_hops.is_empty() {
            const DANCER_STARTLE_RADIUS: f32 = 78.0;
            const MAX_DANCER_STARTLES: usize = 5;
            const DANCER_JOLT_RADIUS_SQ: f32 = 70.0 * 70.0; // Thief
            const DANCER_TRIP_RADIUS_SQ: f32 = 68.0 * 68.0; // Golden
            const DANCER_CHIP_RADIUS_SQ: f32 = 66.0 * 66.0; // Armored
            const DANCER_KICK_RADIUS_SQ: f32 = 72.0 * 72.0; // Magnet

            // Bucket the (usually small, but unbounded as Dancer count grows) set of hop
            // sources so each crab only tests nearby ones instead of every Dancer that hopped
            // this beat. Built once at the widest radius (the startle ripple's) and reused by
            // all five checks below, each with its own (smaller) trigger radius.
            let cell_size = DANCER_STARTLE_RADIUS.max(1.0);
            let cell_of = |p: Vec2| -> (i32, i32) {
                (
                    (p.x / cell_size).floor() as i32,
                    (p.y / cell_size).floor() as i32,
                )
            };
            // Same unbounded-key fix as contagion_grid_buf/armored_anchor_grid_buf: a plain
            // per-bucket clear left one entry per grid cell ever visited by a hopping Dancer,
            // which only grows over a session as the herd roams the whole level. A full
            // clear() keeps the map's allocated capacity (still avoids a realloc most beats)
            // but bounds the key count to "cells touched this beat".
            self.dancer_startle_grid_buf.clear();
            for (i, &pos) in dancer_hops.iter().enumerate() {
                self.dancer_startle_grid_buf
                    .entry(cell_of(pos))
                    .or_default()
                    .push(i);
            }

            let mut spooked = std::mem::take(&mut self.dancer_spooked_buf);
            let mut jolted = std::mem::take(&mut self.dancer_jolt_buf);
            let mut tripped = std::mem::take(&mut self.dancer_trip_buf);
            let mut chipped = std::mem::take(&mut self.dancer_chip_buf);
            let mut kicked = std::mem::take(&mut self.dancer_kick_buf);
            spooked.clear();
            jolted.clear();
            tripped.clear();
            chipped.clear();
            kicked.clear();

            for crab in self.crabs.iter_mut() {
                if crab.caught {
                    continue;
                }
                if crab.is_thief() {
                    if crab.latch_timer <= 0.0 {
                        continue;
                    }
                    let (cx, cy) = cell_of(crab.pos);
                    let mut hop_src: Option<Vec2> = None;
                    'search_thief: for dx in -1..=1 {
                        for dy in -1..=1 {
                            if let Some(candidates) =
                                self.dancer_startle_grid_buf.get(&(cx + dx, cy + dy))
                            {
                                for &i in candidates {
                                    let hp = dancer_hops[i];
                                    if crab.pos.distance_squared(hp) < DANCER_JOLT_RADIUS_SQ {
                                        hop_src = Some(hp);
                                        break 'search_thief;
                                    }
                                }
                            }
                        }
                    }
                    if let Some(src) = hop_src {
                        // Break the clamp and fling the Thief away from the Dancer that thumped
                        // it, matching how the Magnet-pry sends it off toward the lodestone.
                        crab.latch_timer = 0.0;
                        let dir = (crab.pos - src).normalize_or_zero();
                        let dir = if dir == Vec2::ZERO {
                            Vec2::new(0.0, -1.0)
                        } else {
                            dir
                        };
                        crab.vel = dir * crab.crab_type.speed_range().end * 1.5;
                        crab.speed = 1.0;
                        crab.fleeing = false;
                        crab.startle_timer = 0.0;
                        jolted.push(crab.pos);
                    }
                } else if crab.is_golden() {
                    if crab.magnet_snared > 0.0 {
                        continue;
                    }
                    let (cx, cy) = cell_of(crab.pos);
                    let mut hop_src: Option<Vec2> = None;
                    'search_golden: for dx in -1..=1 {
                        for dy in -1..=1 {
                            if let Some(candidates) =
                                self.dancer_startle_grid_buf.get(&(cx + dx, cy + dy))
                            {
                                for &i in candidates {
                                    let hp = dancer_hops[i];
                                    if crab.pos.distance_squared(hp) < DANCER_TRIP_RADIUS_SQ {
                                        hop_src = Some(hp);
                                        break 'search_golden;
                                    }
                                }
                            }
                        }
                    }
                    if hop_src.is_some() {
                        // Trip it: kill the bolt so it wobbles in place, opening a short catch
                        // window. No magnet_snared flag (keeps the orange snare visual for the
                        // Magnet path); the stalled prize plus the pink burst tell the story.
                        crab.vel *= 0.15;
                        crab.speed = 1.0;
                        crab.fleeing = false;
                        crab.startle_timer = 0.0;
                        crab.join_pulse = 1.0;
                        tripped.push(crab.pos);
                    }
                } else if crab.is_armored() || crab.is_shelled_hermit() {
                    // A Dancer's on-beat hop chips a hard shell — Armored or Hermit alike. For the
                    // Hermit this is one of its three intended cracks (the beam can't touch it), so
                    // herding a hopping Dancer next to a hunkered Hermit is a real way to pop it.
                    if crab.boss_health <= 0.0 {
                        continue;
                    }
                    let (cx, cy) = cell_of(crab.pos);
                    let mut hit = false;
                    'search_armored: for dx in -1..=1 {
                        for dy in -1..=1 {
                            if let Some(candidates) =
                                self.dancer_startle_grid_buf.get(&(cx + dx, cy + dy))
                            {
                                for &i in candidates {
                                    if crab.pos.distance_squared(dancer_hops[i])
                                        < DANCER_CHIP_RADIUS_SQ
                                    {
                                        hit = true;
                                        break 'search_armored;
                                    }
                                }
                            }
                        }
                    }
                    if hit {
                        crab.boss_health = (crab.boss_health - 1.0).max(0.0);
                        crab.join_pulse = 1.0;
                        crab.fleeing = false;
                        crab.spooked_timer = crab.spooked_timer.max(0.3);
                        chipped.push((crab.pos, crab.boss_health <= 0.0, crab.is_hermit()));
                    }
                } else if crab.is_magnet() {
                    if crab.in_flashlight || crab.magnet_charged > 0.0 {
                        continue;
                    }
                    let (cx, cy) = cell_of(crab.pos);
                    let mut hit = false;
                    'search_magnet: for dx in -1..=1 {
                        for dy in -1..=1 {
                            if let Some(candidates) =
                                self.dancer_startle_grid_buf.get(&(cx + dx, cy + dy))
                            {
                                for &i in candidates {
                                    if crab.pos.distance_squared(dancer_hops[i])
                                        < DANCER_KICK_RADIUS_SQ
                                    {
                                        hit = true;
                                        break 'search_magnet;
                                    }
                                }
                            }
                        }
                    }
                    if hit {
                        crab.magnet_charged = 0.45;
                        crab.join_pulse = 1.0;
                        kicked.push(crab.pos);
                    }
                } else if crab.is_boss()
                    || crab.is_dancer()
                    || crab.in_flashlight
                    || crab.fleeing
                    || crab.startle_timer > 0.0
                    || crab.charm_timer > 0.0
                {
                    continue;
                } else {
                    if spooked.len() >= MAX_DANCER_STARTLES {
                        continue;
                    }
                    let (cx, cy) = cell_of(crab.pos);
                    let mut nearest: Option<(f32, Vec2)> = None;
                    for dx in -1..=1 {
                        for dy in -1..=1 {
                            if let Some(candidates) =
                                self.dancer_startle_grid_buf.get(&(cx + dx, cy + dy))
                            {
                                for &i in candidates {
                                    let src = dancer_hops[i];
                                    let d = src.distance(crab.pos);
                                    if d < DANCER_STARTLE_RADIUS
                                        && nearest.map_or(true, |(nd, _)| d < nd)
                                    {
                                        nearest = Some((d, src));
                                    }
                                }
                            }
                        }
                    }
                    if let Some((d, src)) = nearest {
                        let outward = (crab.pos - src).normalize_or_zero();
                        let outward = if outward == Vec2::ZERO {
                            Vec2::new(0.0, -1.0)
                        } else {
                            outward
                        };
                        let prox = 1.0 - d / DANCER_STARTLE_RADIUS;
                        let kick = crab.crab_type.speed_range().end * (1.0 + prox * 0.7);
                        crab.vel = outward * kick;
                        crab.speed = 1.0;
                        crab.startle_timer = 0.4;
                        spooked.push(crab.pos);
                    }
                }
            }

            for &pos in &spooked {
                if self.fear_rings.len() < 32 {
                    self.fear_rings.push((pos, 0.0));
                }
                self.floating_texts.spawn(
                    "!".to_string(),
                    pos - Vec2::new(0.0, 24.0),
                    20.0,
                    [1.0, 0.55, 0.9, 1.0], // hot Dancer-pink "!" so the source reads at a glance
                );
            }
            for &pos in jolted.iter() {
                if self.fear_rings.len() < 32 {
                    self.fear_rings.push((pos, 0.0));
                }
                self.floating_texts.spawn(
                    "SHAKEN LOOSE!".to_string(),
                    pos - Vec2::new(58.0, 30.0),
                    24.0,
                    [1.0, 0.55, 0.9, 1.0], // hot Dancer-pink so the "a Dancer did this" story reads
                );
                self.spawn_catch_shockwave(pos, [1.0, 0.45, 0.85]);
            }
            for &pos in tripped.iter() {
                if self.fear_rings.len() < 32 {
                    self.fear_rings.push((pos, 0.0));
                }
                self.floating_texts.spawn(
                    "STAGGERED!".to_string(),
                    pos - Vec2::new(52.0, 30.0),
                    24.0,
                    [1.0, 0.55, 0.9, 1.0], // hot Dancer-pink so the "a Dancer tripped it" story reads
                );
                self.spawn_catch_shockwave(pos, [1.0, 0.75, 0.3]); // gold burst — it's the prize wobbling
            }
            for &(pos, broke, was_hermit) in chipped.iter() {
                // Campaign win tracking: a Dancer hop that fully opens a shell counts toward a
                // CrackAndHold goal, same as a Stomp crack.
                if broke {
                    self.shells_cracked_run += 1;
                }
                // A Dancer hop that pops a Hermit clean open earns the signature copper Hermit-pop
                // instead of the generic blue crack — it's a pure archetype-web crack (the beam
                // can't do it), so the emergent play reads as the win it is.
                if broke && was_hermit {
                    self.spawn_hermit_pop(pos);
                    continue;
                }
                let (label, burst) = if broke {
                    ("SHELL CRACKED!", [0.7, 0.8, 0.95]) // fully open — matches the Stomp crack cue
                } else {
                    ("CHIPPED!", [0.62, 0.68, 0.78]) // a chink knocked loose, more shell to go
                };
                self.floating_texts.spawn(
                    label.to_string(),
                    pos - Vec2::new(58.0, 32.0),
                    24.0,
                    [1.0, 0.55, 0.9, 1.0], // hot Dancer-pink so the "a Dancer did this" story reads
                );
                self.spawn_catch_shockwave(pos, burst);
            }
            for &pos in kicked.iter() {
                if self.fear_rings.len() < 32 {
                    self.fear_rings.push((pos, 0.0));
                }
                self.floating_texts.spawn(
                    "MAGNET SURGE!".to_string(),
                    pos - Vec2::new(58.0, 32.0),
                    24.0,
                    [1.0, 0.55, 0.9, 1.0], // hot Dancer-pink so the "a Dancer did this" story reads
                );
                self.spawn_catch_shockwave(pos, [0.95, 0.7, 0.3]); // orange-gold burst — the Magnet flaring charged
            }

            self.dancer_spooked_buf = spooked;
            self.dancer_jolt_buf = jolted;
            self.dancer_trip_buf = tripped;
            self.dancer_chip_buf = chipped;
            self.dancer_kick_buf = kicked;
        }

        self.dancer_hop_scratch = dancer_hops; // hand the buffer back for reuse next beat

        // Dancer-link on-beat catch aura — "train position matters." A Dancer you've caught
        // keeps its rhythm even in the conga line: on every beat, each caught Dancer link
        // pulses a small on-beat catch aura that snags any free, catchable crab pressed up
        // against that spot in the train. Where the Dancer *sits* in the line — set purely by
        // the order you caught it — decides what its pulse sweeps up: a Dancer near the head
        // vacuums crabs by where you're actively herding, one further back cleans up whatever
        // the trailing tail brushes past. So catch order and train shape become a live
        // decision, the rhythm-native mirror of routing an Armored crab to the guarded tail.
        // On-beat only + small radius = a positioning *reward*, not an autocatch; the downbeat
        // reaches a hair wider so the "1" of the bar lands the biggest sweep.
        const DANCER_AURA_RADIUS: f32 = 58.0;
        let aura_radius = if downbeat {
            DANCER_AURA_RADIUS * 1.2
        } else {
            DANCER_AURA_RADIUS
        };
        let aura_r2 = aura_radius * aura_radius;
        // Snapshot where the caught Dancer links sit this beat (usually a small handful), so
        // the enlist loop below can borrow &mut self.crabs without an overlapping borrow.
        let mut dancer_links = std::mem::take(&mut self.dancer_link_buf);
        dancer_links.clear();
        dancer_links.extend(
            self.crabs
                .iter()
                .filter(|c| c.caught && c.is_dancer())
                .map(|c| c.pos),
        );
        if !dancer_links.is_empty() {
            let mult = self.combo_multiplier();
            let mut rng = crate::rng::rng();
            let mut aura_caught = std::mem::take(&mut self.dancer_aura_caught_buf);
            aura_caught.clear();
            for i in 0..self.crabs.len() {
                // Free, catchable, ordinary herd crabs only — never a boss, a shelled
                // Armored/Hermit (its shell isn't the aura's to crack), or an already-caught
                // link. A Golden is fair game: parking a Dancer link where a snared Golden
                // sits is a legit way to bank the prize on the beat.
                if self.crabs[i].caught || !self.crabs[i].is_catchable() || self.crabs[i].is_boss()
                {
                    continue;
                }
                let pos = self.crabs[i].pos;
                if !dancer_links
                    .iter()
                    .any(|&d| d.distance_squared(pos) <= aura_r2)
                {
                    continue;
                }
                let crab_type = self.crabs[i].crab_type;
                let crab_color = self.crabs[i].crab_color();
                let is_golden = self.crabs[i].is_golden();
                self.particle_system
                    .spawn_catch_effect(pos, crab_color, crab_type, &mut rng);
                self.crabs[i].caught = true;
                self.crabs[i].chain_index = Some(self.chain_count);
                self.chain_count += 1;
                aura_caught.push((pos, is_golden));
            }
            let n = aura_caught.len();
            if n > 0 {
                // Score the sweep like a small on-beat catch: each snag pays a base point at
                // the live combo multiplier, and the grab bumps the combo so a well-placed
                // Dancer link keeps a groove streak alive between your own catches.
                let bonus = n * mult;
                self.score += bonus;
                self.combo_count += n;
                self.combo_timer = 1.8;
                self.on_beat_flash = self.on_beat_flash.max(if downbeat { 0.45 } else { 0.35 });
                self.chain_join_ripple = true;
                for &(pos, is_golden) in aura_caught.iter() {
                    // Hot Dancer-pink burst so the "your Dancer link did this" story reads at a
                    // glance, matching every other Dancer-crossover cue's color.
                    self.spawn_catch_shockwave(pos, [1.0, 0.45, 0.85]);
                    if is_golden {
                        // Fold in the full Golden payout — the aura banked the prize on the beat.
                        self.on_golden_caught(pos, 0);
                    }
                }
                // One shared "GROOVE PULL!" shout at the first snag so a multi-catch beat reads
                // as a single moment, not a stack of overlapping pops.
                let (label_pos, _) = aura_caught[0];
                self.floating_texts.spawn(
                    if n > 1 {
                        format!("GROOVE PULL!  x{}", n)
                    } else {
                        "GROOVE PULL!".to_string()
                    },
                    label_pos - Vec2::new(56.0, 30.0),
                    26.0,
                    [1.0, 0.55, 0.9, 1.0],
                );
                self.check_milestone(&mut rng);
            }
            self.dancer_aura_caught_buf = aura_caught;
        }
        self.dancer_link_buf = dancer_links; // hand the buffer back for reuse next beat

        // Flashlight on-beat recharge bonus: each on-beat action already boosts groove,
        // so tie a small extra charge tick to the beat so playing rhythmically keeps the
        // flashlight topped up longer than passive recharge alone.
        if self.flashlight.charge < 1.0 && !self.flashlight.on {
            self.flashlight.charge = (self.flashlight.charge + 0.08).min(1.0);
        }
    }
}
