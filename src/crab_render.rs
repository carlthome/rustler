use std::cell::RefCell;

use ggez::glam::Vec2;
use ggez::graphics::{BlendMode, Canvas};
use ggez::{Context, GameResult};
use rand::Rng;

use crate::MainState;
use crate::graphics::{
    draw_armor_ring, draw_attracted_crab_glow, draw_boss_health_ring, draw_catch_next_hint,
    draw_centerpiece_ring, draw_crab, draw_cycle_preview_ring, draw_golden_sparkle,
    draw_hermit_shell, draw_magnet_aura, draw_splitter_aura, draw_thief_aura,
    flush_archetype_rings, flush_attracted_crab_glows, flush_beat_coronas, flush_catch_next_ticks,
    flush_centerpiece_dots, flush_hermit_coil_dots, flush_magnet_auras,
};
use crate::{BEAT_WINDOW, CRAB_SIZE, CULL_MARGIN};

// Scratch buffer for centerpiece_link_indices — reused every draw frame so the per-frame
// Vec<usize> allocation that was fired inside draw_crabs_with_shake is eliminated. Same
// grown-but-not-shrunk pattern as BOND_INDEX_BUF in main.rs: reaches steady state at max
// train length and stays there.
thread_local! {
    static CENTERPIECE_OUT_BUF: RefCell<Vec<usize>> = RefCell::new(Vec::new());
}

/// World-space rendering of every crab on the beach: the free-herd aura+shake pass (flashlight
/// scorch, boss/armor/hermit/magnet/thief/golden/splitter rings, per-crab shake and beat hop),
/// the conga-train wave-bob draw with cycle-preview and centerpiece rings, and the ambient NPC
/// conga trains — all batched into the shared deferred leg/body/ring buffers. Split out of
/// overlays.rs so that file holds only the HUD banners and full-screen menus.
impl MainState {
    pub(crate) fn draw_crabs_with_shake(&self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        let mut rng = crate::rng::rng();
        // Level-of-detail hint for draw_crab: the more crabs on the beach (wild herd + conga train
        // + NPC trains drawn in this same pass), the cheaper each crab renders, so a big train stays
        // smooth. Full articulation is reserved for calm fields and hero-sized crabs; tiny/distant
        // crabs are always cheap regardless. Set once per pass. Must include NPC train followers
        // (drawn later in this same pass by draw_npc_conga_train) — a rival train that's grown fat
        // on stolen crabs is exactly the swarm case this LOD system exists to keep cheap, and
        // self.crabs.len() alone never sees that growth.
        let npc_follower_total: usize = self
            .npc_trains
            .iter()
            .map(|n| n.follower_types.len() + 1) // +1 for the King Crab leader
            .sum();
        crate::graphics::set_crab_lod_hint(self.crabs.len() + npc_follower_total);
        // Every free crab's aura below (flashlight glow, Magnet/Thief/Golden rings) additively
        // blends, and used to flip the canvas's blend mode ADD -> ALPHA -> ADD per crab (each aura
        // helper toggled it around itself). ggez only actually switches the GPU pipeline on a
        // transition between consecutive queued draws, so that per-crab toggling was a real
        // per-crab pipeline-state churn. Setting ADD once for this whole aura pass and restoring
        // once after collapses that into a single transition in, one out — same visuals (draw_crab
        // itself defers into batched buffers and isn't blended here, so it's unaffected).
        let original_blend = canvas.blend_mode();
        canvas.set_blend_mode(BlendMode::ADD);
        // Skip the whole per-crab draw (body + auras) for free crabs fully off-screen — the world
        // can be up to 4x the viewport per axis, so a big fraction of the herd sits off camera on
        // large maps every frame. CULL_MARGIN is wider than any aura anchored on the crab (Magnet's
        // 240px ring is the widest), so nothing actually visible is ever skipped.
        let view_min = self.camera_origin - Vec2::splat(CULL_MARGIN);
        let view_max = self.camera_origin + Vec2::new(self.width, self.height) + Vec2::splat(CULL_MARGIN);
        for (i, crab) in self.crabs.iter().enumerate() {
            if !crab.caught {
                if crab.pos.x < view_min.x
                    || crab.pos.x > view_max.x
                    || crab.pos.y < view_min.y
                    || crab.pos.y > view_max.y
                {
                    continue;
                }
                let mut pos = crab.pos;
                let mut shake_strength = 0.0;
                if crab.spooked_timer > 0.0 {
                    shake_strength = 18.0 * crab.spooked_timer;
                } else if self.shake_timer > 0.0 {
                    shake_strength = 18.0 * self.shake_timer;
                }
                if shake_strength > 0.0 {
                    let t = self.time_elapsed * 30.0 + i as f32 * 2.0;
                    pos.x += (t).sin() * shake_strength
                        + rng.random_range(-shake_strength..=shake_strength) * 0.3;
                    pos.y += (t * 1.3).cos() * shake_strength
                        + rng.random_range(-shake_strength..=shake_strength) * 0.3;
                }
                let crab_beat = (self.beat_intensity * 0.7
                    + (crab.pos.x * 0.003).sin().abs() * 0.3)
                    .clamp(0.0, 1.0);
                // The wild herd grooves too. Free crabs bob with the music, but with a spatial phase
                // offset from screen position so the field reads as several organic ripples rolling
                // through the crowd rather than a lockstep jump — the party the player recruits from
                // is alive, not a static pickup field. Only the *amplitude* is beat-gated (the hop
                // swells on the downbeat and settles between beats), so the whole beach breathes with
                // the pulse. Kept smaller than the conga train's dramatic wave (amplitude ~10-26) so
                // caught crabs still read as the liveliest dancers. Bosses don't dance — a bopping
                // King Crab would undercut its menace — and fleeing/spooked crabs sit it out too
                // (panic, not party), so the hop reads as mood rather than a global clock.
                let wild_lift = if crab.is_boss()
                    || crab.fleeing
                    || crab.spooked_timer > 0.0
                    || crab.startle_timer > 0.0
                {
                    0.0
                } else {
                    let ripple = (crab.pos.x + crab.pos.y) * 0.012;
                    // Positive bump only — a hop, never a dip into the ground.
                    (self.beat_intensity * (ripple - self.time_elapsed * 5.0).sin()).max(0.0) * 7.0
                };
                // Raise the body by the hop (draw_pos moves up); pass the same amount as y_lift so
                // the drop shadow shrinks/detaches underneath, matching how the conga train hops.
                let hop_pos = pos - Vec2::new(0.0, wild_lift);
                draw_crab(
                    ctx,
                    canvas,
                    crab,
                    hop_pos,
                    crab_beat,
                    crab.join_pulse,
                    wild_lift,
                    crab.facing_angle,
                    self.time_elapsed,
                )?;
                // CATCH-NEXT hint: if this free crab shares the current tail's archetype, catching it
                // next would extend the tail match-run (tail_run_len). Interior chain order is frozen,
                // so this catch-order choice is the one arrangement lever the player actually controls —
                // surface it as a ring in the crab's own type color so "grab me to keep the run going"
                // reads live in the field. Skip bosses and spooked/fleeing crabs (not sensible grabs),
                // and only bother once a train exists to extend. Purely legibility, no odds change.
                if self.chain_count > 0
                    && !crab.is_boss()
                    && !crab.fleeing
                    && crab.spooked_timer <= 0.0
                    && crab.startle_timer <= 0.0
                    && self.cached_tail_type == Some(crab.crab_type)
                {
                    draw_catch_next_hint(
                        ctx,
                        canvas,
                        hop_pos + Vec2::splat(crab.scale * CRAB_SIZE * 0.5),
                        crab.scale * CRAB_SIZE * 0.7,
                        crab.crab_color(),
                        self.time_elapsed,
                        self.beat_intensity,
                        self.tail_run_len,
                    )?;
                }
                // Scorch ring — ONLY for a shelled target the beam is actively burning down (a boss
                // or an Armored crab still holding shell, lit up in the cone). This replaced the old
                // "attraction halo" that used to ring every crab the light touched: the beam no
                // longer herds normal crabs, so there's nothing to halo. A crab burning under the
                // beam gets a harsh white-hot searing ring so the read is unmistakably "melting".
                if crab.in_flashlight
                    && crab.boss_health > 0.0
                    && (crab.is_boss() || crab.is_armored())
                {
                    let size = crab.scale * CRAB_SIZE;
                    draw_attracted_crab_glow(
                        ctx,
                        canvas,
                        pos,
                        size,
                        [1.0, 0.9, 0.55],
                        self.time_elapsed,
                        self.beat_intensity,
                    )?;
                }
                // Boss aura + wear-down health ring — aura tinted per archetype.
                if crab.is_boss() {
                    let size = crab.scale * CRAB_SIZE;
                    let frac = crab.boss_health / crab.boss_max_health.max(0.001);
                    let base_aura = if crab.is_tide_boss() {
                        [0.25, 0.7, 1.0]
                    } else if crab.is_hermit_king() {
                        // The Hermit King's shell-fortress: warm old-copper aura, matching the
                        // Hermit line's coiled-shell palette so the family resemblance reads.
                        [0.85, 0.5, 0.18]
                    } else if crab.is_dancer_king() {
                        // The Dancer King: rose-gold spotlight that swells on the beat — the aura
                        // itself keeps time, cueing the on-beat catch that banks its court.
                        let on_beat = self.beat_timer < BEAT_WINDOW
                            || self.beat_timer > self.beat_interval - BEAT_WINDOW;
                        let flare: f32 = if on_beat { 0.35 } else { 0.0 };
                        [1.0, (0.55 + flare).min(1.0), (0.4 + flare).min(1.0)]
                    } else if crab.is_rhythm_boss() {
                        // The Reef DJ pulses violet, and flares bright only on a *hot* beat of the
                        // phrase it called this bar — that's the window its shell is open, so the aura
                        // flash IS the "hit now" cue. A landed hot beat adds an extra bloom via
                        // reef_hit_flash so a clean echo reads as a satisfying pop of light.
                        let on_beat = self.beat_timer < BEAT_WINDOW
                            || self.beat_timer > self.beat_interval - BEAT_WINDOW;
                        let hot = on_beat && self.reef_phrase[(self.beat_count % 4) as usize];
                        let flare = if hot { 0.45 } else { 0.0 } + self.reef_hit_flash * 0.35;
                        [(0.72 + flare * 0.3).min(1.0), (0.30 + flare).min(1.0), 0.95]
                    } else {
                        [1.0, 0.8, 0.25]
                    };
                    // Enraged bosses glow hot: shift the aura toward an angry pulsing red so the final
                    // phase reads instantly, matching the ramped-up charge/pulse behavior.
                    let aura = if crab.enraged {
                        let p = 0.5 + 0.5 * (self.time_elapsed * 9.0).sin();
                        [
                            (base_aura[0] * 0.4 + 0.6_f32).min(1.0),
                            base_aura[1] * (0.35 + 0.15 * p),
                            base_aura[2] * (0.35 + 0.15 * p),
                        ]
                    } else {
                        base_aura
                    };
                    draw_boss_health_ring(ctx, canvas, pos, size, frac, self.time_elapsed, aura)?;
                } else if crab.is_armored() && crab.boss_health > 0.0 {
                    // Armored shell indicator — depletes as the shell is worn or cracked
                    let size = crab.scale * CRAB_SIZE;
                    let frac = crab.boss_health / crab.crab_type.initial_shell().max(0.001);
                    draw_armor_ring(ctx, canvas, pos, size, frac, self.time_elapsed)?;
                } else if crab.is_shelled_hermit() {
                    // Hermit borrowed-shell indicator — a warm coppery coiled ring, visually distinct
                    // from the Armored crab's cold steely arc, so the player learns "this one the beam
                    // won't crack; use the ecosystem" at a glance. Depletes as the shell is chipped.
                    let size = crab.scale * CRAB_SIZE;
                    let frac = crab.boss_health / crab.crab_type.initial_shell().max(0.001);
                    draw_hermit_shell(ctx, canvas, pos, size, frac, self.time_elapsed)?;
                } else if crab.is_magnet() {
                    // Magnetic field aura — inward-sweeping rings showing its pull radius, so the
                    // player can see the catchment and chase it for the two-for-one cluster catch.
                    let size = crab.scale * CRAB_SIZE;
                    draw_magnet_aura(
                        ctx,
                        canvas,
                        pos,
                        size,
                        240.0,
                        self.time_elapsed,
                        crab.is_magnet_lured(),
                        crab.is_magnet_charged(),
                    )?;
                } else if crab.is_thief() {
                    // Thief marker — a sly green ring while it prowls, flaring into a fast gnaw-ring
                    // once it's latched onto the tail so the theft-in-progress reads at a glance.
                    let size = crab.scale * CRAB_SIZE;
                    draw_thief_aura(
                        ctx,
                        canvas,
                        pos,
                        size,
                        crab.is_latched(),
                        crab.is_magnet_intercepted(),
                        crab.is_thief_lured(),
                        self.time_elapsed,
                    )?;
                } else if crab.is_golden() {
                    // Golden crab shine — a shimmering ring of orbiting sparkles so the rare prize
                    // catches the eye across the whole field and reads as "chase this one!".
                    let size = crab.scale * CRAB_SIZE;
                    draw_golden_sparkle(
                        ctx,
                        canvas,
                        pos,
                        size,
                        self.time_elapsed,
                        crab.is_magnet_snared(),
                    )?;
                } else if crab.is_splitter() {
                    // Splitter cleave aura — a teal ring with two halves pulsing apart, so the
                    // player reads "this one splits my train" and can decide to set it up or dodge.
                    // `beat_prox` peaks (→1) as the beat lands so the aura flares gold in the
                    // clean-cut window, telegraphing the timing bet BEFORE the catch: grab it while
                    // it's hot for the full jackpot cut, or it's a sloppy half-cut. Distance to the
                    // nearest beat edge, scaled by the same BEAT_WINDOW the clean-cut gate uses, so
                    // the flare and the actual reward window agree.
                    let size = crab.scale * CRAB_SIZE;
                    let to_beat = self.beat_timer.min(self.beat_interval - self.beat_timer);
                    let beat_prox = (1.0 - to_beat / (BEAT_WINDOW * 1.5)).clamp(0.0, 1.0);
                    draw_splitter_aura(ctx, canvas, pos, size, self.time_elapsed, beat_prox)?;
                }
            }
        }
        // Flush all Golden-sparkle dots that draw_golden_sparkle() deferred into GOLDEN_SPARKLE_PARAMS
        // during the per-crab aura pass above. Still in ADD blend mode here (restored right after),
        // so the sparkle dots land in the same blend state they always did.
        crate::graphics::flush_golden_sparkles(ctx, canvas)?;
        // Flush hermit coil dots deferred by draw_hermit_shell() calls above — same pattern as
        // the golden sparkles: up to 5 unit-circle draws per shelled Hermit, now one GPU submission.
        flush_hermit_coil_dots(ctx, canvas)?;
        // Flush catch-next-hint tick dots deferred by draw_catch_next_hint() calls above. All
        // dots share the same fixed stroke-circle mesh, so the entire per-crab-per-tick payload
        // collapses to one draw_instanced_mesh — from up to 60 calls (15 matching crabs × 4 dots)
        // down to 1. Same blend mode (still in ADD), identical on-screen output.
        flush_catch_next_ticks(ctx, canvas)?;
        // Flush Magnet aura rings deferred by draw_magnet_aura() calls above. In the Water biome
        // (Magnet-heavy after the biome archetype redirect) this collapses N×3 individual sweep-ring
        // draw calls into at most 3 batched draw_instanced_mesh calls — one per phase bucket — plus
        // up to N core-ring calls. Net: from ~20 GPU submissions for 5 Magnets to ~8.
        flush_magnet_auras(ctx, canvas)?;
        // Flush attracted-crab glow rings deferred by draw_attracted_crab_glow() above. Each crab
        // in the flashlight beam deferred 2 canvas.draw() calls (outer soft-glow + inner ring) into
        // key-grouped scratch maps; now collapsed to one draw_instanced_mesh per distinct stroke
        // radius bucket. With ~10-30 crabs in beam range this trims 20-60 individual GPU submissions
        // down to ~2-4 batched ones. Same blend mode (caller already in ADD), same pixels.
        flush_attracted_crab_glows(ctx, canvas)?;
        // Flush beat-corona halos deferred by draw_crab() for caught (conga-train) crabs during
        // a strong beat pulse. Each corona is one soft circle in the crab's own color, drawn here
        // while the canvas is still in ADD blend so they addively light up the train on every
        // downbeat — one GPU submission for the entire conga train's glow regardless of length.
        flush_beat_coronas(ctx, canvas)?;
        // Flush the Thief/Splitter/Golden/Armored archetype rings (and Splitter's cleave dots)
        // deferred by draw_thief_aura/draw_splitter_aura/draw_golden_sparkle/draw_armor_ring above
        // — the last aura draws that were still one canvas.draw() per crab per frame, now batched
        // the same way as the flushes above. Still in ADD blend mode here (restored right after).
        flush_archetype_rings(ctx, canvas)?;
        canvas.set_blend_mode(original_blend);
        // Which seated links are part of a paying CENTERPIECE run right now, so we can ring them
        // live (see draw_centerpiece_ring). Computed once per frame from the same predicate the pen
        // pays on. `keep` mirrors the delivered count used at bank time (chain_count == train len).
        // Uses a reused thread-local scratch buffer (take/fill/put-back) instead of allocating a
        // fresh Vec every frame — eliminates a ~60 Hz heap alloc on any frame a train is present.
        let mut centerpiece_set =
            CENTERPIECE_OUT_BUF.with(|buf| std::mem::take(&mut *buf.borrow_mut()));
        centerpiece_set.clear();
        self.centerpiece_link_indices(self.chain_count, &mut centerpiece_set);
        // Draw chain crabs with a groovy wave bob that travels through the train
        for crab in self.crabs.iter() {
            if crab.caught {
                let (bob, sway) = if let Some(ci) = crab.chain_index {
                    let amplitude = 10.0 + self.beat_intensity * 16.0;
                    let wave_phase = self.time_elapsed * 6.0 - ci as f32 * 0.55;
                    let b = wave_phase.sin() * amplitude;
                    let s = (wave_phase + std::f32::consts::FRAC_PI_2).sin() * amplitude * 0.5;
                    (b, s)
                } else {
                    (0.0, 0.0)
                };
                let chain_beat = self.beat_intensity.clamp(0.0, 1.0);
                let lift = bob.min(0.0).abs(); // lift = how much the crab is up (bob is negative = up)
                draw_crab(
                    ctx,
                    canvas,
                    crab,
                    crab.pos + Vec2::new(sway, bob),
                    chain_beat,
                    crab.join_pulse,
                    lift,
                    crab.facing_angle,
                    self.time_elapsed,
                )?;
                // CYCLE PREVIEW: ring the crab a Cycle (X) would promote to the head (the link at
                // chain_index 1). Only when the verb is actually available (cache is None otherwise),
                // so the marker appears exactly when pressing X would land this crab up front — a
                // mouse-free read: it shows what the next on-beat X does, so the player arranges the
                // head/tail on purpose instead of mashing blind.
                if self.cycle_preview_active && crab.chain_index == Some(1) {
                    draw_cycle_preview_ring(
                        ctx,
                        canvas,
                        crab.pos + Vec2::new(sway, bob) + Vec2::splat(crab.scale * CRAB_SIZE * 0.5),
                        crab.scale * CRAB_SIZE * 0.7,
                        crab.crab_color(),
                        self.time_elapsed,
                        self.beat_intensity,
                        crab.is_golden() || crab.is_dancer(),
                    )?;
                }
                // CENTERPIECE: ring this link if it's part of a paying mid-train run. Reads as an
                // amber laurel so the player sees the protected centerpiece forming as they build,
                // turning "hold a long train" into an arrangement puzzle they set up on purpose.
                if let Some(ci) = crab.chain_index {
                    if !centerpiece_set.is_empty() && centerpiece_set.binary_search(&ci).is_ok() {
                        // An endpoint is a link at the start/end of its own contiguous run, i.e.
                        // a neighbouring index isn't also in the set — works even if two runs
                        // qualify at once (the vec concatenates them but they're non-adjacent).
                        // centerpiece_set is always sorted (built from extend(start..end_exclusive)
                        // ranges in ascending order), so binary_search replaces the O(n) contains().
                        let is_endpoint =
                            centerpiece_set.binary_search(&ci.wrapping_sub(1)).is_err()
                                || centerpiece_set.binary_search(&(ci + 1)).is_err();
                        draw_centerpiece_ring(
                            ctx,
                            canvas,
                            crab.pos
                                + Vec2::new(sway, bob)
                                + Vec2::splat(crab.scale * CRAB_SIZE * 0.5),
                            crab.scale * CRAB_SIZE * 0.7,
                            self.time_elapsed,
                            self.beat_intensity,
                            is_endpoint,
                        )?;
                    }
                }
            }
        }
        // Ambient NPC conga train — drawn into the same deferred leg/body buffers as player crabs.
        self.draw_npc_conga_train(ctx, canvas)?;

        // Every draw_crab() call above deferred its 6 leg draws and 12 body-part (shadow, shell,
        // claws, eyes) draws into shared buffers instead of issuing them individually (up to
        // 18 x 50+ crabs = 900+ draw calls). Flush them both here as two instanced batches — same
        // parts, same positions/rotations/colors, two GPU submissions instead of hundreds. This
        // does mean legs and body parts across all crabs now draw as two groups after every crab's
        // glow/ring this frame, instead of interleaved per-crab; since legs are thin lines mostly
        // beside the body and the glow/rings are soft translucent overlays, the reordering isn't
        // perceptible in motion.
        crate::graphics::flush_crab_legs(ctx, canvas)?;
        crate::graphics::flush_crab_bodies(ctx, canvas)?;
        // Flush centerpiece bracket-dot DrawParams deferred by draw_centerpiece_ring() calls
        // above — same technique as hermit-coil and catch-next-tick batching. Up to 10 dots per
        // centerpiece link (a 6-link run → 60 individual canvas.draw() calls) collapsed to one
        // instanced draw regardless of how long the qualifying run gets.
        flush_centerpiece_dots(ctx, canvas)?;
        // Return the scratch buffer to the thread-local so it keeps its allocated capacity for
        // next frame instead of freeing and reallocating it each draw call.
        CENTERPIECE_OUT_BUF.with(|buf| *buf.borrow_mut() = centerpiece_set);
        Ok(())
    }
}
