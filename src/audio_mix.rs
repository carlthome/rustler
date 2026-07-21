//! Per-frame audio mixing for `MainState`: the spatial King Crab boss rumble, the
//! intensity-scaled music layers, the steal stings, the ambient NPC-train rumble and
//! per-rival beat-locked motifs, and the archetype crab-theme loops.
//!
//! Extracted verbatim from the giant `MainState::update` in `main.rs` (two contiguous
//! audio blocks) into `impl MainState` methods to keep that file navigable. Pure
//! structural move — no behaviour change; the caller still owns the surrounding
//! simulation and invokes these at the same points, with the same `dt`.

use ggez::Context;
use ggez::audio::SoundSource;

use crate::beat::downbeat_started;
use crate::*;

impl MainState {
    pub(crate) fn pause_gameplay_music(&self) {
        let pause_if_playing = |source: &ggez::audio::Source| {
            if source.playing() {
                source.pause();
            }
        };
        for music in &self.sounds.action_music {
            pause_if_playing(music);
        }
        for layer in &self.music_layers {
            pause_if_playing(layer);
        }
        for (left, right) in &self.sounds.king_crab_motif {
            pause_if_playing(left);
            pause_if_playing(right);
        }
        for theme in &self.sounds.crab_themes {
            pause_if_playing(theme);
        }
        for source in [
            &self.sounds.king_crab_l,
            &self.sounds.king_crab_r,
            &self.sounds.king_crab_soft,
            &self.sounds.king_crab_rumble_l,
            &self.sounds.king_crab_rumble_r,
        ] {
            pause_if_playing(source);
        }
    }

    /// Spatial King Crab boss rumble + intensity-scaled music layers. Runs once per
    /// frame from `update`, right after boss spawning and before the game-over tally.
    pub(crate) fn update_boss_and_music_audio(&mut self, ctx: &mut Context, dt: f32) {
        // Spatial audio for King Crab boss crabs.
        //
        // Three looping stereo sources are blended by boss distance and angle each frame:
        //   king_crab_l  — bright rumble, hard-panned left
        //   king_crab_r  — bright rumble, hard-panned right
        //   king_crab_soft — muffled/sine rumble with room echo, centered
        //
        // Volume rolloff: full brightness within 150 px, fades to zero at 600 px.
        // Panning: boss angle relative to player drives L/R split (equal-power law).
        // Brightness rolloff: soft source crossfades in as distance increases, so
        //   a distant boss sounds muffled (filtered) while a near one sounds present.
        // Player's own action_music is always full-range — the boss is the distant source.
        {
            use ggez::audio::SoundSource;

            // Mute during non-game screens.
            let game_active = !self.show_instructions && !self.game_over && !self.show_world_map;

            // Find the nearest uncaught boss crab position (if any).
            let nearest_boss: Option<Vec2> = if game_active {
                self.crabs.iter()
                    .filter(|c| !c.caught && c.is_boss())
                    .map(|c| c.pos)
                    .min_by(|a, b| {
                        let da = a.distance(self.player_pos);
                        let db = b.distance(self.player_pos);
                        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                    })
            } else {
                None
            };

            let (vol_l, vol_r, vol_soft) = if let Some(boss_pos) = nearest_boss {
                let dist = boss_pos.distance(self.player_pos);
                // Distance factor: full within 150 px, zero at 600 px.
                const FULL_DIST: f32 = 150.0;
                const SILENT_DIST: f32 = 600.0;
                let near_factor = ((SILENT_DIST - dist) / (SILENT_DIST - FULL_DIST)).clamp(0.0, 1.0);
                // Soft/far factor: kicks in beyond FULL_DIST, full at SILENT_DIST.
                let far_factor = (1.0 - near_factor) * near_factor.max(0.15);

                // Angle from player to boss: 0 = right, π = left.
                let delta = boss_pos - self.player_pos;
                // pan in -1..1: negative = left, positive = right.
                let pan = if delta.length_squared() > 1.0 {
                    (delta.x / delta.length()).clamp(-1.0, 1.0)
                } else {
                    0.0
                };
                // Equal-power panning: map -1..+1 → 0..π/2, then cos/sin.
                let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
                let gain_l = angle.cos() * near_factor;
                let gain_r = angle.sin() * near_factor;
                (gain_l, gain_r, far_factor * 0.7)
            } else {
                (0.0, 0.0, 0.0)
            };

            // Smooth toward targets with a ~0.5s time constant so the pan doesn't snap.
            let smooth = |cur: f32, tgt: f32| cur + (tgt - cur) * (dt * 4.0).min(1.0);
            let cur_l = self.sounds.king_crab_l.volume();
            let cur_r = self.sounds.king_crab_r.volume();
            let cur_s = self.sounds.king_crab_soft.volume();
            let new_l = smooth(cur_l, vol_l);
            let new_r = smooth(cur_r, vol_r);
            let new_s = smooth(cur_s, vol_soft);
            self.sounds.king_crab_l.set_volume(new_l);
            self.sounds.king_crab_r.set_volume(new_r);
            self.sounds.king_crab_soft.set_volume(new_s);

            // Start the rhythmic King Crab texture only on the master grid. Its
            // one-bar buffer then stays phase-locked with the player groove.
            // `beat_timer` is reset to a full interval when the beat fires. Use only that
            // post-crossing half of the timing window (and bar beat 1), never the pre-beat half,
            // so a newly audible loop cannot start early or with its phrase shifted by 1–3 beats.
            let downbeat_started =
                downbeat_started(self.beat_count, self.beat_timer, self.beat_interval);
            for (src, vol) in [
                (&mut self.sounds.king_crab_l, new_l),
                (&mut self.sounds.king_crab_r, new_r),
                (&mut self.sounds.king_crab_soft, new_s),
            ] {
                if vol > 0.01 && src.paused() {
                    src.resume();
                } else if vol > 0.01 && !src.playing() && downbeat_started {
                    let _ = src.play();
                } else if vol <= 0.01 && src.playing() {
                    src.pause();
                }
            }
        }

        // Scale music volume with intensity
        // (action_music gets louder, layers fade in)
        // If music is muted, set all music volumes to 0; otherwise use normal intensity curve.
        // Duck the player's music slightly when an NPC King Crab is close — their rumble competes
        // for sonic space, making proximity feel threatening even before visual contact.
        let npc_duck = {
            let nearest_dist = self
                .npc_trains
                .iter()
                .map(|t| t.leader_pos.distance(self.player_pos))
                .fold(f32::MAX, f32::min);
            if nearest_dist < 400.0 {
                1.0 - ((400.0 - nearest_dist) / 400.0) * 0.25
            } else {
                1.0
            }
        };
        let base_vol = if self.music_muted {
            0.0
        } else {
            (0.25 + self.music_intensity * 0.75) * npc_duck * self.tutorial_music_gain()
        };
        let active_music = self.action_music_index();
        for (index, music) in self.sounds.action_music.iter_mut().enumerate() {
            music.set_volume(if index == active_music {
                base_vol.clamp(0.0, 1.0)
            } else {
                0.0
            });
            if index != active_music && music.playing() {
                music.pause();
            }
        }
        let layer_count = self.music_layers.len();
        for (i, layer) in self.music_layers.iter_mut().enumerate() {
            let threshold = (i + 1) as f32 / (layer_count + 1) as f32;
            let vol = if self.music_muted {
                0.0
            } else if self.music_intensity > threshold {
                ((self.music_intensity - threshold) * 2.0).min(1.0)
            } else {
                0.0
            };
            layer.set_volume(vol);
            if layer.paused() && vol > 0.01 {
                layer.resume();
            } else if !layer.playing() && vol > 0.01 {
                let _ = layer.play();
            }
        }
    }

    /// Ambient field audio: steal stings, the NPC-train rumble + per-rival motifs, and
    /// the archetype crab-theme loops. Runs once per frame from `update`, right after
    /// `update_npc_trains` and before the camera recompute.
    pub(crate) fn update_ambient_audio(&mut self, ctx: &mut Context, dt: f32) {
        // Steal stings: the splice logic above runs without `ctx`, so it just latches a one-frame
        // flag when crabs change hands. Play the matching sting here — a descending thud when a
        // rival rustles from you, a rising sparkle when you rustle back — so the core steal moment
        // reads in the audio too (INSPIRATION.md "Audio IS the scoreboard" / "Steal to win").
        // On-beat tool drum-pad accent: a ranged cast (whistle/stomp/wave/lasso) landed on the beat,
        // so layer the crisp woodblock "tok" over its own SFX — the audible half of "each tool key is
        // a drum pad" (INSPIRATION.md). Pitched up per on-beat streak so a hot run of casts climbs.
        if self.on_beat_tool_sfx {
            self.on_beat_tool_sfx = false;
            crate::state::play_tool_accent(&mut self.sounds, self.beat_streak);
        }
        if self.steal_loss_sfx {
            self.steal_loss_sfx = false;
            let _ = self.sounds.steal_loss_sfx.play();
        }
        if self.steal_gain_sfx {
            self.steal_gain_sfx = false;
            let _ = self.sounds.steal_gain_sfx.play();
        }
        // Rival-vs-rival theft clack (ROADMAP whole-beach ecology): a third-party steal happened out
        // on the field, so place it in the mix — pan by the collision's bearing and fade by distance
        // so a far-off steal is a faint directional tick the player looks toward and swoops into for
        // the spilled crumbs (agar.io "eat the crumbs"). `play_detached` preserves the per-play
        // volume and detaches, so simultaneous thefts don't cut each other off. Muted off-field.
        if let Some(splice_pos) = self.rival_steal_sfx.take() {
            use ggez::audio::SoundSource as _;
            let game_active =
                !self.show_instructions && !self.game_over && !self.show_world_map;
            if game_active {
                let delta = splice_pos - self.player_pos;
                let dist = delta.length();
                // Distance fade: full within ~250px, easing to a faint floor by ~1000px so a theft
                // anywhere on the beach still ticks while a close one clearly reads as "right here."
                // Capped at 0.5 so this ambient ecology event sits under the player-centric stings.
                let near = 1.0 - ((dist - 250.0) / 750.0).clamp(0.0, 1.0);
                let vol = (0.12 + 0.88 * near) * 0.5;
                // Equal-power L/R pan from the bearing, matching the King Crab rumble's panning.
                let pan = if delta.length_squared() > 1.0 {
                    (delta.x / dist).clamp(-1.0, 1.0)
                } else {
                    0.0
                };
                let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
                self.sounds.rival_steal_l.set_volume(angle.cos() * vol);
                self.sounds.rival_steal_r.set_volume(angle.sin() * vol);
                let _ = self.sounds.rival_steal_l.play();
                let _ = self.sounds.rival_steal_r.play();
            }
        }

        // Spatial audio: smooth the ambient King Crab train rumble AND pan it by the leader's
        // bearing, so a rival train is not just heard swelling with distance but *placed*
        // left/right — the directional radar (agar.io "heard before seen"). Distance swell is
        // `target_vol` (full within 200px, silent beyond 800px); an equal-power pan splits it
        // into L/R by the leader's angle. Muted on menu/game-over screens.
        {
            use ggez::audio::SoundSource as _;
            let game_active =
                !self.show_instructions && !self.game_over && !self.show_world_map;
            let (target_l, target_r) = if game_active {
                self.npc_trains.first().map_or((0.0, 0.0), |t| {
                    // pan in -1..1: negative = left, positive = right, from leader bearing.
                    let delta = t.leader_pos - self.player_pos;
                    let pan = if delta.length_squared() > 1.0 {
                        (delta.x / delta.length()).clamp(-1.0, 1.0)
                    } else {
                        0.0
                    };
                    // Equal-power law: -1..+1 → 0..π/2, then cos/sin so total loudness is
                    // constant across the sweep (matches the boss rumble's panning).
                    let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
                    (angle.cos() * t.target_vol, angle.sin() * t.target_vol)
                })
            } else {
                (0.0, 0.0)
            };
            let smooth = |src: &mut ggez::audio::Source, tgt: f32| {
                let cur = src.volume();
                let v = (cur + (tgt - cur) * (dt * 2.0).min(1.0)).clamp(0.0, 1.0);
                src.set_volume(v);
                if v > 0.02 && src.paused() {
                    src.resume();
                } else if v > 0.02 && !src.playing() {
                    let _ = src.play();
                } else if v <= 0.02 && src.playing() {
                    src.stop();
                }
            };
            smooth(&mut self.sounds.king_crab_rumble_l, target_l);
            smooth(&mut self.sounds.king_crab_rumble_r, target_r);
        }

        // Per-rival spatial MUSIC: on top of the shared creature rumble above, each ambient NPC
        // King Crab train broadcasts its OWN beat-locked musical motif — panned by the leader's
        // bearing, swelling with distance AND with the train's length/tier. "More crabs in sync =
        // more music": a lone scout is a faint high tick, an elder dragging a long conga is a loud,
        // full low motif. When several rivals are audible the loudest keeps the mix and the quieter
        // ones duck, so trains read as distinct voices rather than mush (INSPIRATION.md agar.io:
        // "the dominant train dominates the mix"). Beat-lock: the buffers are an exact two-bar loop
        // at the live tempo and are (re)started only on the beat, so every note sits in the pocket
        // with the player's groove. Null-audio-safe — set_volume/play/stop guard exactly like the
        // rumble path, so the headless playtests are unaffected.
        {
            use ggez::audio::SoundSource as _;
            let game_active = !self.show_instructions && !self.game_over && !self.show_world_map;
            let downbeat_started =
                downbeat_started(self.beat_count, self.beat_timer, self.beat_interval);
            const RIVAL_MOTIF_TIERS: usize = 3;
            let motif_start = self.action_music_index() * RIVAL_MOTIF_TIERS;
            let n = self
                .sounds
                .king_crab_motif
                .len()
                .saturating_sub(motif_start)
                .min(RIVAL_MOTIF_TIERS)
                .min(self.npc_trains.len());

            // Pass 1: raw target loudness (distance swell x size/tier presence) + bearing per train.
            let mut raw = [0.0_f32; 8];
            let mut pan = [0.0_f32; 8];
            for i in 0..n {
                if !game_active {
                    continue;
                }
                let t = &self.npc_trains[i];
                let dist = t.leader_pos.distance(self.player_pos);
                // Rival motifs should emerge only at close range: keep them inaudible past 500px,
                // then use a squared swell so the competing grooves stay faint until the train is
                // genuinely nearby. The creature rumble remains the longer-range directional cue.
                const FULL_MOTIF_DIST: f32 = 120.0;
                const SILENT_MOTIF_DIST: f32 = 500.0;
                let distance_swell =
                    ((SILENT_MOTIF_DIST - dist) / (SILENT_MOTIF_DIST - FULL_MOTIF_DIST))
                        .clamp(0.0, 1.0);
                let dist_factor = distance_swell * distance_swell;
                // Tier from the scale floor: scout 1.2 -> 0, wanderer 1.8 -> 0.5, elder 2.4 -> 1.0.
                let tier_floor = ((t.base_scale - 1.2) / 1.2).clamp(0.0, 1.0);
                let len = t.follower_types.len() as f32;
                // Size/tier presence: tier floor + a per-follower ramp so a growing conga sounds
                // fuller. Capped at 1.0 (a maxed elder saturates its slot).
                let presence =
                    (0.10 + tier_floor * 0.20 + len * (0.04 + tier_floor * 0.02)).min(1.0);
                raw[i] = dist_factor * presence;
                let delta = t.leader_pos - self.player_pos;
                pan[i] = if delta.length_squared() > 1.0 {
                    (delta.x / delta.length()).clamp(-1.0, 1.0)
                } else {
                    0.0
                };
            }

            // Pass 2: duck by loudness rank so the dominant train dominates the mix and the rest
            // recede (keeps 2-3 simultaneous trains legible instead of a wash). The top slot also
            // caps the motif well under the player's own groove — it's an ambient scoreboard layer.
            let mut order = [0usize, 1, 2, 3, 4, 5, 6, 7];
            let order = &mut order[..n];
            order.sort_by(|&a, &b| {
                raw[b].partial_cmp(&raw[a]).unwrap_or(std::cmp::Ordering::Equal)
            });
            const DUCK: [f32; 3] = [0.5, 0.28, 0.15];
            let mut ducked = [0.0_f32; 8];
            for (rank, &i) in order.iter().enumerate() {
                ducked[i] = raw[i] * DUCK.get(rank).copied().unwrap_or(0.12);
            }

            // Pass 3: apply. Equal-power L/R split by bearing; smooth the volume; (re)start the L/R
            // pair TOGETHER on a beat so they stay phase-locked with each other and with the grid.
            for i in 0..n {
                let angle = (pan[i] + 1.0) * std::f32::consts::FRAC_PI_4;
                let motif_volume = if self.music_muted { 0.0 } else { ducked[i] };
                let tgt_l = angle.cos() * motif_volume;
                let tgt_r = angle.sin() * motif_volume;
                let (src_l, src_r) = &mut self.sounds.king_crab_motif[motif_start + i];
                let new_l = {
                    let cur = src_l.volume();
                    (cur + (tgt_l - cur) * (dt * 2.0).min(1.0)).clamp(0.0, 1.0)
                };
                let new_r = {
                    let cur = src_r.volume();
                    (cur + (tgt_r - cur) * (dt * 2.0).min(1.0)).clamp(0.0, 1.0)
                };
                src_l.set_volume(new_l);
                src_r.set_volume(new_r);
                // Hysteresis on the raw target (not the smoothed volume) so a train hovering near
                // the audible edge doesn't chatter start/stop every frame.
                let want_play = motif_volume > 0.03;
                let playing = src_l.playing() || src_r.playing();
                if want_play && (src_l.paused() || src_r.paused()) {
                    src_l.resume();
                    src_r.resume();
                } else if want_play && !playing && downbeat_started {
                    let _ = src_l.play();
                    let _ = src_r.play();
                } else if !want_play && motif_volume < 0.008 && playing {
                    src_l.pause();
                    src_r.pause();
                }
            }
        }

        // Crab-theme music loops: count how many of each archetype group are free on the field,
        // then smoothly ramp each theme's volume so the soundscape reflects what's out there.
        // Max volume is low (0.13) so they layer as ambient texture without drowning the game.
        if !self.show_instructions && !self.game_over && !self.show_world_map {
            use ggez::audio::SoundSource;
            // Count free crabs per theme group (caught crabs are "with you" — silence their theme).
            let mut counts = [0usize; 5];
            for c in &self.crabs {
                if c.caught {
                    continue;
                }
                let theme = match c.crab_type {
                    crate::enemies::CrabType::Normal
                    | crate::enemies::CrabType::Fast
                    | crate::enemies::CrabType::Big => 0,
                    crate::enemies::CrabType::Dancer | crate::enemies::CrabType::Splitter => 1,
                    crate::enemies::CrabType::Thief | crate::enemies::CrabType::Sneaky => 2,
                    crate::enemies::CrabType::Boss
                    | crate::enemies::CrabType::Armored
                    | crate::enemies::CrabType::Hermit => 3,
                    crate::enemies::CrabType::Golden | crate::enemies::CrabType::Magnet => 4,
                    _ => 0,
                };
                counts[theme] += 1;
            }
            let dt_audio = self.frame_dt(ctx);
            for (i, theme) in self.sounds.crab_themes.iter_mut().enumerate() {
                let target = if self.music_muted || counts[i] == 0 {
                    0.0
                } else {
                    // Scales from 0.05 (1 crab) up to 0.13 (8+ crabs)
                    (0.05 + (counts[i] as f32 - 1.0) * 0.01).min(0.13)
                };
                let cur = theme.volume();
                let smoothed = (cur + (target - cur) * (dt_audio * 2.5).min(1.0)).clamp(0.0, 0.2);
                theme.set_volume(smoothed);
                let downbeat_started =
                    downbeat_started(self.beat_count, self.beat_timer, self.beat_interval);
                if smoothed > 0.01 && theme.paused() {
                    theme.resume();
                } else if smoothed > 0.01 && !theme.playing() && downbeat_started {
                    let _ = theme.play();
                } else if smoothed <= 0.01 && theme.playing() {
                    theme.stop();
                }
            }
        }
    }
}
