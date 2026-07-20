use std::cell::RefCell;

use ggez::glam::Vec2;
use ggez::graphics::{BlendMode, Canvas, Color, DrawParam, Mesh, Rect, Text};
use ggez::{Context, GameResult};
use rand::Rng;

use crate::MainState;
use crate::hud_cache::{
    FRENZY_BANNER_CACHE, GAME_OVER_CACHE, LEVEL_TITLE_OVERLAY_CACHE, STAGE_BANNER_CACHE,
    TUTORIAL_OVERLAY_CACHE, UPGRADE_SCREEN_CACHE,
};
use crate::upgrade::{UPGRADE_POOL, UpgradeId};
use crate::{BEAT_WINDOW, BOSS_MAX_HEALTH, CRAB_SIZE};
use crate::graphics::{
    cached_stroke_rect, draw_armor_ring, draw_attracted_crab_glow, draw_boss_health_ring,
    draw_catch_next_hint, draw_centerpiece_ring, draw_crab, draw_cycle_preview_ring,
    draw_golden_sparkle, draw_hermit_shell, draw_magnet_aura, draw_splitter_aura,
    draw_thief_aura, flush_attracted_crab_glows, flush_beat_coronas, flush_catch_next_ticks,
    flush_centerpiece_dots, flush_hermit_coil_dots, flush_magnet_auras, unit_square,
};

// Scratch buffer for centerpiece_link_indices — reused every draw frame so the per-frame
// Vec<usize> allocation that was fired inside draw_crabs_with_shake is eliminated. Same
// grown-but-not-shrunk pattern as BOND_INDEX_BUF in main.rs: reaches steady state at max
// train length and stays there.
thread_local! {
    static CENTERPIECE_OUT_BUF: RefCell<Vec<usize>> = RefCell::new(Vec::new());
}

/// Full-screen overlay and HUD-screen drawing: level title cards, frenzy/stage banners, the
/// tutorial instruction card, the free/chain crab aura+shake pass, and the game-over and
/// upgrade-choice screens. Split out of main.rs to keep that file navigable.
impl MainState {
    pub(crate) fn draw_level_title(
        &self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        width: f32,
        height: f32,
    ) -> Result<(), ggez::GameError> {
        // Timing: timer counts down from 3.1 → 0.
        //   3.1..2.8  fade in  (0.3s)
        //   2.8..0.6  hold
        //   0.6..0.0  fade out
        let t = self.level_title_timer;
        let alpha = if t > 2.8 {
            ((3.1 - t) / 0.3).clamp(0.0, 1.0)
        } else if t < 0.6 {
            (t / 0.6).clamp(0.0, 1.0)
        } else {
            1.0
        };
        // Slide in from left: during fade-in, title slides right into position.
        let slide_x = (1.0 - alpha) * -80.0;

        let biome = self.levels[self.current_level.min(self.levels.len() - 1)].biome;

        LEVEL_TITLE_OVERLAY_CACHE.with(|c| -> Result<(), ggez::GameError> {
            let mut cache = c.borrow_mut();
            let needs_rebuild = match &*cache {
                Some((cached_title, cached_biome, _, _, _, _, _, _, _, _, _, _)) => {
                    cached_title != &self.level_title || *cached_biome != biome.name
                }
                None => true,
            };
            if needs_rebuild {
                // Control style: large title, smaller biome subtitle, threat tag
                let mut title = Text::new(self.level_title.to_uppercase());
                title.set_scale(72.0);
                let title_dims = title.measure(ctx)?;

                let mut subtitle = Text::new(biome.name.to_uppercase());
                subtitle.set_scale(22.0);
                let sub_dims = subtitle.measure(ctx)?;

                let emphasis = self.levels[self.current_level.min(self.levels.len() - 1)].emphasis;
                let threat_opt = if let Some(label) = crate::levels::emphasis_label(emphasis) {
                    let mut threat = Text::new(label.to_uppercase());
                    threat.set_scale(18.0);
                    let tw = threat.measure(ctx)?.x;
                    Some((threat, tw))
                } else {
                    None
                };

                *cache = Some((
                    self.level_title.clone(),
                    biome.name,
                    title,
                    // bg_rect slot — unused now, store a dummy
                    Mesh::new_rectangle(ctx, ggez::graphics::DrawMode::fill(),
                        Rect::new(0.0, 0.0, 1.0, 1.0), Color::from_rgba(0,0,0,0))?,
                    // border_rect slot — unused now
                    Mesh::new_rectangle(ctx, ggez::graphics::DrawMode::fill(),
                        Rect::new(0.0, 0.0, 1.0, 1.0), Color::from_rgba(0,0,0,0))?,
                    subtitle,
                    title_dims.x,
                    title_dims.y,
                    sub_dims.y,
                    sub_dims.x,
                    sub_dims.x, // reuse slot
                    threat_opt,
                ));
            }

            let (_, _, title, _, _, subtitle, title_w, title_h, sub_h, sub_w, _, threat_opt) =
                cache.as_ref().unwrap();

            // Layout: anchored to lower-left, ~35% up from bottom — Control style
            let margin_left = 72.0;
            let anchor_y = height * 0.62;

            let a = (alpha * 255.0) as u8;
            let a_dim = (alpha * 120.0) as u8;

            // Dark translucent backing strip — full width, left-anchored
            let strip_h = title_h + sub_h + 28.0;
            let strip = Mesh::new_rectangle(
                ctx,
                ggez::graphics::DrawMode::fill(),
                Rect::new(0.0, anchor_y - 8.0, width, strip_h + 16.0),
                Color::from_rgba(0, 0, 0, (alpha * 140.0) as u8),
            )?;
            canvas.draw(&strip, DrawParam::default());

            // Accent line — thin white vertical bar to the left of the text, Control-style
            let accent = Mesh::new_rectangle(
                ctx,
                ggez::graphics::DrawMode::fill(),
                Rect::new(margin_left + slide_x - 16.0, anchor_y, 3.0, strip_h - 12.0),
                Color::from_rgba(255, 255, 255, a),
            )?;
            canvas.draw(&accent, DrawParam::default());

            // Subtitle (biome) ABOVE the title — small caps, dimmed
            let (pr, pg, pb) = biome.pulse;
            canvas.draw(
                subtitle,
                DrawParam::default()
                    .dest(Vec2::new(margin_left + slide_x, anchor_y))
                    .color(Color::from_rgba(pr, pg, pb, a_dim)),
            );

            // Main title — large, white
            canvas.draw(
                title,
                DrawParam::default()
                    .dest(Vec2::new(margin_left + slide_x, anchor_y + sub_h + 4.0))
                    .color(Color::from_rgba(245, 245, 248, a)),
            );

            // Horizontal rule under title
            let rule_y = anchor_y + sub_h + 4.0 + title_h + 6.0;
            let rule = Mesh::new_rectangle(
                ctx,
                ggez::graphics::DrawMode::fill(),
                Rect::new(margin_left + slide_x, rule_y, title_w * 0.6, 1.5),
                Color::from_rgba(200, 200, 210, a_dim),
            )?;
            canvas.draw(&rule, DrawParam::default());

            // Threat tag below rule
            if let Some((threat, tw)) = threat_opt {
                canvas.draw(
                    threat,
                    DrawParam::default()
                        .dest(Vec2::new(margin_left + slide_x, rule_y + 10.0))
                        .color(Color::from_rgba(255, 160, 60, a)),
                );
                let _ = tw;
            }

            Ok(())
        })
    }

    /// Big gold "FRENZY!" shout when a frenzy wave lands. Pops in with a scale punch and fades
    /// out with `frenzy_banner_timer`; sits high on screen so it never fights the level title.
    pub(crate) fn draw_frenzy_banner(
        &self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        width: f32,
        height: f32,
    ) -> Result<(), ggez::GameError> {
        // Normalized life 0..1 (1 = just landed). Fade over the last third; punch scale early.
        let life = (self.frenzy_banner_timer / 1.6).clamp(0.0, 1.0);
        let alpha = (life * 3.0).min(1.0); // hold, then fade only in the final third
        // Beat-synced throb so it pulses with the music like everything else.
        let beat_phase = 1.0 - (self.beat_timer / self.beat_interval).clamp(0.0, 1.0);
        let throb = (beat_phase * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        // Slightly larger right as it lands, settling to a gently throbbing size.
        let scale = 1.15 - life * 0.15 + throb * 0.06;

        let dims = FRENZY_BANNER_CACHE.with(|cache_cell| -> Result<Vec2, ggez::GameError> {
            let mut cache = cache_cell.borrow_mut();
            if cache.is_none() {
                let mut banner = Text::new("FRENZY!");
                banner.set_scale(84.0);
                let dims: Vec2 = banner.measure(ctx)?.into();
                *cache = Some((banner, dims));
            }
            Ok(cache.as_ref().unwrap().1)
        })?;
        let dest = Vec2::new(
            width / 2.0 - dims.x * scale / 2.0,
            height * 0.16 - dims.y * scale / 2.0,
        );
        let a = (alpha * 255.0) as u8;
        let g = (200.0 + throb * 55.0) as u8;
        FRENZY_BANNER_CACHE.with(|cache_cell| {
            let cache = cache_cell.borrow();
            let banner = &cache.as_ref().unwrap().0;
            // Dark drop-shadow behind for legibility over any biome.
            canvas.draw(
                banner,
                DrawParam::default()
                    .dest(dest + Vec2::splat(3.0))
                    .scale(Vec2::splat(scale))
                    .color(Color::from_rgba(20, 12, 0, (a as f32 * 0.7) as u8)),
            );
            // Gold body, brightening on the beat.
            canvas.draw(
                banner,
                DrawParam::default()
                    .dest(dest)
                    .scale(Vec2::splat(scale))
                    .color(Color::from_rgba(255, g, 60, a)),
            );
        });
        Ok(())
    }

    /// Cyan "BUILDING / HEATED / FEVER …" shout when the run climbs into a new intensity stage.
    /// Same pop-and-fade feel as the Frenzy banner but a cool color and a slightly lower slot, so
    /// the two read as distinct events (spike vs. rising tide) if they ever land close together.
    pub(crate) fn draw_stage_banner(
        &self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        width: f32,
        height: f32,
    ) -> Result<(), ggez::GameError> {
        let life = (self.stage_banner_timer / 2.0).clamp(0.0, 1.0);
        let alpha = (life * 3.0).min(1.0); // hold, then fade only in the final third
        let beat_phase = 1.0 - (self.beat_timer / self.beat_interval).clamp(0.0, 1.0);
        let throb = (beat_phase * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        let scale = 1.1 - life * 0.12 + throb * 0.05;

        let name = self.stage_banner_name;
        let dims = STAGE_BANNER_CACHE.with(|cache_cell| -> Result<Vec2, ggez::GameError> {
            let mut cache = cache_cell.borrow_mut();
            let needs_rebuild = match cache.as_ref() {
                Some((cached_name, _, _)) => *cached_name != name,
                None => true,
            };
            if needs_rebuild {
                let mut banner = Text::new(name);
                banner.set_scale(64.0);
                let dims: Vec2 = banner.measure(ctx)?.into();
                *cache = Some((name, banner, dims));
            }
            Ok(cache.as_ref().unwrap().2)
        })?;
        let dest = Vec2::new(
            width / 2.0 - dims.x * scale / 2.0,
            height * 0.27 - dims.y * scale / 2.0,
        );
        let a = (alpha * 255.0) as u8;
        let b = (200.0 + throb * 55.0) as u8;
        STAGE_BANNER_CACHE.with(|cache_cell| {
            let cache = cache_cell.borrow();
            let banner = &cache.as_ref().unwrap().1;
            canvas.draw(
                banner,
                DrawParam::default()
                    .dest(dest + Vec2::splat(3.0))
                    .scale(Vec2::splat(scale))
                    .color(Color::from_rgba(4, 16, 20, (a as f32 * 0.7) as u8)),
            );
            // Cyan body, brightening on the beat.
            canvas.draw(
                banner,
                DrawParam::default()
                    .dest(dest)
                    .scale(Vec2::splat(scale))
                    .color(Color::from_rgba(90, 230, b, a)),
            );
        });
        Ok(())
    }

    /// Draw the tutorial session's instruction card (title + what-to-do + live progress) pinned to
    /// the top of the screen, plus a big centered "PASSED!" flourish once the session is cleared.
    /// Previously rebuilt a Mesh::new_rounded_rectangle (GPU buffer) + 4-6 Text objects (glyph-
    /// shaping) every single frame the tutorial was active. Now uses TUTORIAL_OVERLAY_CACHE:
    /// — the card mesh is keyed by (width, height) bit-patterns (same as MENU_PANEL_CACHE)
    /// — the static texts (title, instruction, "Esc" hint, "PASSED!") are cached once per kind
    /// — the progress counter text rebuilds only when the counter actually advances (once per catch)
    pub(crate) fn draw_tutorial_overlay(
        &self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        width: f32,
        height: f32,
    ) -> Result<(), ggez::GameError> {
        let t = match &self.tutorial {
            Some(t) => t,
            None => return Ok(()),
        };

        // The counter that drives the progress line. Different fields track progress per kind.
        let progress_key = match t.kind {
            crate::tutorial::TutorialKind::BeatTiming => t.on_beat_catches,
            crate::tutorial::TutorialKind::ChainDeliver => t.deliveries,
            crate::tutorial::TutorialKind::ShellCrack => t.shells_cracked,
            crate::tutorial::TutorialKind::LassoGrab => t.lasso_catches,
        };
        let title_key = t.title(); // &'static str — also serves as the kind discriminant
        let wbits = width.to_bits();
        let hbits = height.to_bits();

        TUTORIAL_OVERLAY_CACHE.with(|cell| -> ggez::GameResult {
            let mut cache = cell.borrow_mut();

            // Invalidate if kind changed, screen resized, or progress counter advanced.
            let stale = match &*cache {
                None => true,
                Some((tk, wb, hb, _, _, _, _, _, _, _, _, _, _, _, pk, _, _)) => {
                    *tk != title_key || *wb != wbits || *hb != hbits || *pk != progress_key
                }
            };

            if stale {
                let card = Mesh::new_rounded_rectangle(
                    ctx,
                    ggez::graphics::DrawMode::fill(),
                    Rect::new(width * 0.5 - 360.0, 24.0, 720.0, 132.0),
                    14.0,
                    Color::from_rgba(8, 14, 26, 200),
                )?;

                let mut title_text = Text::new(t.title());
                title_text.set_scale(30.0);
                let tdims: Vec2 = title_text.measure(ctx)?.into();

                let mut instr_text = Text::new(t.instruction());
                instr_text.set_scale(20.0);
                let idims: Vec2 = instr_text.measure(ctx)?.into();

                let mut hint_text = Text::new("Esc — back to menu");
                hint_text.set_scale(18.0);
                let hw = hint_text.measure(ctx).map(|m| m.x).unwrap_or(0.0);

                let mut passed_text = Text::new("PASSED!");
                passed_text.set_scale(80.0);
                let pdims: Vec2 = passed_text.measure(ctx)?.into();

                let prog_str = t.progress_line();
                let mut prog_text = Text::new(prog_str);
                prog_text.set_scale(24.0);
                let prog_w = prog_text.measure(ctx).map(|m| m.x).unwrap_or(0.0);

                *cache = Some((
                    title_key,
                    wbits,
                    hbits,
                    card,
                    title_text,
                    tdims.x,
                    tdims.y,
                    instr_text,
                    idims.x,
                    hint_text,
                    hw,
                    passed_text,
                    pdims.x,
                    pdims.y,
                    progress_key,
                    prog_text,
                    prog_w,
                ));
            }

            let (
                _,
                _,
                _,
                card,
                title_text,
                tw,
                _,
                instr_text,
                iw,
                hint_text,
                hw,
                passed_text,
                pasw,
                pash,
                _,
                prog_text,
                prog_w,
            ) = cache.as_ref().unwrap();

            // Translucent card backdrop across the top so the instruction text reads over any terrain.
            canvas.draw(card, DrawParam::default());

            canvas.draw(
                title_text,
                DrawParam::default()
                    .dest(Vec2::new(width * 0.5 - tw / 2.0, 38.0))
                    .color(Color::from_rgb(255, 226, 120)),
            );

            canvas.draw(
                instr_text,
                DrawParam::default()
                    .dest(Vec2::new(width * 0.5 - iw / 2.0, 76.0))
                    .color(Color::from_rgb(220, 232, 245)),
            );

            canvas.draw(
                prog_text,
                DrawParam::default()
                    .dest(Vec2::new(width * 0.5 - prog_w / 2.0, 124.0))
                    .color(Color::from_rgb(120, 255, 150)),
            );

            // Bottom hint so a player who wants out knows how — this is opt-in teaching, no gating.
            canvas.draw(
                hint_text,
                DrawParam::default()
                    .dest(Vec2::new(width * 0.5 - hw / 2.0, height - 40.0))
                    .color(Color::from_rgba(200, 210, 225, 180)),
            );

            // Cleared: a big pulsing "PASSED!" centered while the exit hold runs out.
            // `pass_glow` and `scale` are per-frame so they stay outside the cache.
            if t.completed {
                let scale = 1.0 + t.pass_glow * 0.15;
                let dest = Vec2::new(
                    width / 2.0 - pasw * scale / 2.0,
                    height * 0.42 - pash * scale / 2.0,
                );
                canvas.draw(
                    passed_text,
                    DrawParam::default()
                        .dest(dest + Vec2::splat(3.0))
                        .scale(Vec2::splat(scale))
                        .color(Color::from_rgba(4, 20, 8, 180)),
                );
                canvas.draw(
                    passed_text,
                    DrawParam::default()
                        .dest(dest)
                        .scale(Vec2::splat(scale))
                        .color(Color::from_rgb(110, 255, 140)),
                );
            }

            Ok(())
        })
    }

    pub(crate) fn draw_crabs_with_shake(&self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        let mut rng = rand::rng();
        // Level-of-detail hint for draw_crab: the more crabs on the beach (wild herd + conga train
        // + NPC trains drawn in this same pass), the cheaper each crab renders, so a big train stays
        // smooth. Full articulation is reserved for calm fields and hero-sized crabs; tiny/distant
        // crabs are always cheap regardless. Set once per pass.
        crate::graphics::set_crab_lod_hint(self.crabs.len());
        // Every free crab's aura below (flashlight glow, Magnet/Thief/Golden rings) additively
        // blends, and used to flip the canvas's blend mode ADD -> ALPHA -> ADD per crab (each aura
        // helper toggled it around itself). ggez only actually switches the GPU pipeline on a
        // transition between consecutive queued draws, so that per-crab toggling was a real
        // per-crab pipeline-state churn. Setting ADD once for this whole aura pass and restoring
        // once after collapses that into a single transition in, one out — same visuals (draw_crab
        // itself defers into batched buffers and isn't blended here, so it's unaffected).
        let original_blend = canvas.blend_mode();
        canvas.set_blend_mode(BlendMode::ADD);
        for (i, crab) in self.crabs.iter().enumerate() {
            if !crab.caught {
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
                    let frac = crab.boss_health / BOSS_MAX_HEALTH;
                    let base_aura = if crab.is_tide_boss() {
                        [0.25, 0.7, 1.0]
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
        // Interior link under the flashlight aim right now — the one a bubble-swap (X on beat) would
        // move toward the centre. Computed once so the per-crab draw loop can ring it as a preview.
        let aimed_bubble_link = if self.cycle_preview_active {
            self.aimed_interior_link()
        } else {
            None
        };
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
                // so the marker appears exactly when pressing X would land this crab up front — letting
                // the player choose a cycle for its arrangement outcome instead of mashing blind.
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
                // BUBBLE PREVIEW: when the flashlight is aimed at an interior link, ring THAT crab so
                // the player sees which one X (on beat) will bubble one slot toward the centre — the
                // legibility that turns the local swap from a blind guess into a placed decision. Green
                // tint distinguishes it from the head-promote cyan cycle ring above.
                if self.cycle_preview_active
                    && aimed_bubble_link.is_some()
                    && crab.chain_index == aimed_bubble_link
                {
                    draw_cycle_preview_ring(
                        ctx,
                        canvas,
                        crab.pos + Vec2::new(sway, bob) + Vec2::splat(crab.scale * CRAB_SIZE * 0.5),
                        crab.scale * CRAB_SIZE * 0.7,
                        [0.5, 1.0, 0.7],
                        self.time_elapsed,
                        self.beat_intensity,
                        true,
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

    pub(crate) fn draw_game_over_screen(&self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        const BOX_WIDTH: f32 = 600.0;
        const BOX_HEIGHT: f32 = 260.0;
        const BOX_X: f32 = 340.0;
        const BOX_Y: f32 = 360.0;

        // All inputs that drive the text are frozen once game_over is set (update() returns
        // early, record_run() fires once) — so build the Mesh and Text objects once and reuse
        // them every subsequent frame rather than paying a GPU buffer upload + glyph-shaping
        // pass ~60 times/second for however long the player sits on the results screen.
        let cache_key = (
            self.score,
            self.time_elapsed.to_bits(),
            self.best_time.to_bits(),
            self.career_best_score,
            self.career_total_score,
            self.career_runs,
            self.run_is_new_best,
        );
        GAME_OVER_CACHE.with(|c| -> GameResult {
            let mut cache = c.borrow_mut();
            let stale = cache.as_ref().map_or(true, |(k, _, _, _)| *k != cache_key);
            if stale {
                let bg_box = Mesh::new_rectangle(
                    ctx,
                    ggez::graphics::DrawMode::fill(),
                    Rect::new(BOX_X, BOX_Y, BOX_WIDTH, BOX_HEIGHT),
                    Color::from_rgba(40, 0, 80, 180),
                )?;
                let text = Text::new(format!(
                    "Game Over!\nThis run: {} crabs banked\nTime: {:.2}s   Best time: {:.2}s\n\nCareer best: {}\nCareer total: {} over {} runs\n\nPress Space or Enter to try again.  Esc to quit.",
                    self.score, self.time_elapsed, self.best_time,
                    self.career_best_score, self.career_total_score, self.career_runs,
                ));
                let banner = if self.run_is_new_best && self.score > 0 {
                    let mut b = Text::new("★ NEW CAREER BEST! ★");
                    b.set_scale(34.0);
                    let bw = b.measure(ctx)?.x;
                    Some((b, bw))
                } else {
                    None
                };
                *cache = Some((cache_key, bg_box, text, banner));
            }
            let (_, bg_box, text, banner) = cache.as_ref().unwrap();
            canvas.draw(bg_box, DrawParam::default());
            canvas.draw(
                text,
                DrawParam::default()
                    .dest(Vec2::new(370.0, 380.0))
                    .color(Color::WHITE),
            );
            // Celebrate a fresh career best with a pulsing banner so beating your record lands.
            // The Text and its width are cached; only the per-frame alpha pulse is computed fresh.
            if let Some((banner_text, bw)) = banner {
                let pulse = 0.55 + 0.45 * (self.menu_time * 5.0).sin().abs();
                canvas.draw(
                    banner_text,
                    DrawParam::default()
                        .dest(Vec2::new(BOX_X + (BOX_WIDTH - bw) / 2.0, BOX_Y - 44.0))
                        .color(Color::new(1.0, 0.85, 0.2, pulse)),
                );
            }
            Ok(())
        })
    }

    /// Screen-space rectangles for the three upgrade cards, in card order (index 0 = card "1").
    /// Shared by the draw code (hover highlight) and the mouse-click handler so they always agree.
    pub(crate) fn upgrade_card_rects(&self) -> [Rect; 3] {
        let w = self.width;
        let h = self.height;
        let card_w = 268.0_f32;
        let card_h = 330.0_f32;
        let gap = 26.0_f32;
        let n = 3usize;
        let total_w = n as f32 * card_w + (n - 1) as f32 * gap;
        let x0 = (w - total_w) / 2.0;
        let y0 = (h - card_h) / 2.0 + 15.0;
        std::array::from_fn(|i| Rect::new(x0 + i as f32 * (card_w + gap), y0, card_w, card_h))
    }

    pub(crate) fn draw_upgrade_screen(&self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        let w = self.width;
        let h = self.height;

        // Dark overlay — reuse the cached unit square instead of a fresh Mesh::new_rectangle GPU
        // buffer every frame (same fix used for every other full-screen flash/fill in draw_game).
        canvas.draw(
            unit_square(ctx)?,
            DrawParam::default()
                .scale(Vec2::new(w, h))
                .color(Color::from_rgba(8, 4, 22, 210)),
        );

        // Three options rolled at queue time (see roll_upgrade_offer). Some deepen a tool lane
        // (rank shown, so committing feels deliberate); some are tradeoffs (a "TRADEOFF" tag
        // instead of a rank). Build a per-card descriptor: (key, icon, name, desc, r,g,b, sub-label,
        // is_lit) where sub-label is the rank line for lanes or "TRADEOFF" for tradeoffs.
        let sub_for = |id: UpgradeId| -> (String, bool) {
            let lane_line = |rank: u32| -> (String, bool) {
                if rank == 0 {
                    ("NEW LANE".to_string(), false)
                } else {
                    (format!("LV {}  ->  {}", rank, rank + 1), true)
                }
            };
            match id {
                UpgradeId::BeamFocus | UpgradeId::Sharpshooter => lane_line(self.beam_rank),
                UpgradeId::LassoFocus | UpgradeId::HeavyHauler => lane_line(self.lasso_rank),
                UpgradeId::WhistleFocus | UpgradeId::Roadrunner => lane_line(self.whistle_rank),
                UpgradeId::StompFocus => lane_line(self.stomp_rank),
                UpgradeId::Featherweight | UpgradeId::WideNet => ("TRADEOFF".to_string(), false),
            }
        };
        let cards: Vec<(String, &str, &str, &str, u8, u8, u8, String, bool)> = (0..3)
            .map(|slot| {
                let id = UPGRADE_POOL[self.offered_upgrades[slot]];
                let (icon, name, desc, r, g, b) = id.card();
                let (sub, lit) = sub_for(id);
                ((slot + 1).to_string(), icon, name, desc, r, g, b, sub, lit)
            })
            .collect();

        let rects = self.upgrade_card_rects();
        let card_w = rects[0].w;
        let card_h = rects[0].h;

        // Build or reuse cached Text objects (title, hint, and all per-card labels). Every
        // Text::new + measure() is a glyph-shaping pass; the card border Mesh::new_rectangle calls
        // were also GPU buffer allocations. The texts only change when a rank changes, which is
        // what dismisses this screen — so in practice the cache hits every frame after the first.
        // The hover highlight is applied as DrawParam color below; no re-layout needed for that.
        let cache_key = (
            self.offered_upgrades,
            self.beam_rank,
            self.lasso_rank,
            self.whistle_rank,
            self.stomp_rank,
        );
        UPGRADE_SCREEN_CACHE.with(|c| -> GameResult {
            let mut cache = c.borrow_mut();
            let needs_rebuild = !matches!(&*cache, Some((k, ..)) if *k == cache_key);
            if needs_rebuild {
                // Title
                let mut title_text = Text::new("CHOOSE AN UPGRADE");
                title_text.set_scale(46.0);
                let title_w = title_text.measure(ctx)?.x;
                // Subtitle
                let mut hint_text = Text::new("Click a card or press its number");
                hint_text.set_scale(20.0);
                let hint_w = hint_text.measure(ctx)?.x;
                // Per-card texts — built explicitly for each of the 3 cards (try_from_fn is not
                // stable yet on this toolchain) and stored as a fixed-size array.
                let mut build_card = |i: usize| -> ggez::GameResult<(
                    Text,
                    f32,
                    Text,
                    f32,
                    Text,
                    f32,
                    Text,
                    f32,
                    Text,
                    f32,
                )> {
                    let (key, icon, name, desc, _, _, _, sub, _) = &cards[i];
                    let mut ico = Text::new(*icon);
                    ico.set_scale(82.0);
                    let iw = ico.measure(ctx)?.x;
                    let mut nm = Text::new(*name);
                    nm.set_scale(26.0);
                    let nw = nm.measure(ctx)?.x;
                    let mut rk = Text::new(sub.clone());
                    rk.set_scale(16.0);
                    let rkw = rk.measure(ctx)?.x;
                    let mut dsc = Text::new(*desc);
                    dsc.set_scale(18.0);
                    let dw = dsc.measure(ctx)?.x;
                    let mut kh = Text::new(format!("[ {} ]", key));
                    kh.set_scale(24.0);
                    let kw = kh.measure(ctx)?.x;
                    Ok((ico, iw, nm, nw, rk, rkw, dsc, dw, kh, kw))
                };
                let card_texts: [(Text, f32, Text, f32, Text, f32, Text, f32, Text, f32); 3] =
                    [build_card(0)?, build_card(1)?, build_card(2)?];
                *cache = Some((
                    cache_key, title_text, title_w, hint_text, hint_w, card_texts,
                ));
            }
            let (_, title_text, title_w, hint_text, hint_w, card_texts) = cache.as_ref().unwrap();

            // Title
            canvas.draw(
                title_text,
                DrawParam::default()
                    .dest(Vec2::new((w - title_w) / 2.0, 58.0))
                    .color(Color::from_rgb(255, 215, 50)),
            );

            // Subtitle: make it obvious the cards are clickable, not just number-key driven.
            canvas.draw(
                hint_text,
                DrawParam::default()
                    .dest(Vec2::new((w - hint_w) / 2.0, 110.0))
                    .color(Color::from_rgba(210, 210, 210, 200)),
            );

            for (i, (_, _, _, _, r, g, b, _, lit)) in cards.iter().enumerate() {
                let (r, g, b, lit) = (*r, *g, *b, *lit);
                let cx = rects[i].x;
                let y0 = rects[i].y;
                let m = self.mouse_pos;
                let hovered = m.x >= cx && m.x <= cx + card_w && m.y >= y0 && m.y <= y0 + card_h;

                let accent = Color::from_rgb(r, g, b);
                let bg_a = if hovered { 190u8 } else { 115u8 };
                let bdr_w = if hovered { 4.0_f32 } else { 2.0_f32 };

                // Card background — unit square scaled to card size, no per-frame GPU buffer alloc.
                canvas.draw(
                    unit_square(ctx)?,
                    DrawParam::default()
                        .dest(Vec2::new(cx, y0))
                        .scale(Vec2::new(card_w, card_h))
                        .color(Color::from_rgba(18, 12, 38, bg_a)),
                );
                // Coloured border — cached stroke rect, same mesh reused per bdr_w key.
                canvas.draw(
                    &cached_stroke_rect(ctx, card_w, card_h, bdr_w)?,
                    DrawParam::default().dest(Vec2::new(cx, y0)).color(accent),
                );

                let (ico, iw, nm, nw, rk, rkw, dsc, dw, kh, kw) = &card_texts[i];
                let rank_col = if lit {
                    accent
                } else {
                    Color::from_rgba(180, 180, 180, 200)
                };
                // Lane rank badge — lit in the lane accent once invested.
                // All elements centered on the card's fixed midline (cx + card_w/2) so no element
                // shifts when rank text width changes between sequential upgrade screens.
                let mid = cx + card_w / 2.0;
                canvas.draw(
                    ico,
                    DrawParam::default()
                        .dest(Vec2::new(mid - iw / 2.0, y0 + 18.0))
                        .color(accent),
                );
                canvas.draw(
                    nm,
                    DrawParam::default()
                        .dest(Vec2::new(mid - nw / 2.0, y0 + 118.0))
                        .color(Color::WHITE),
                );
                canvas.draw(
                    rk,
                    DrawParam::default()
                        .dest(Vec2::new(mid - rkw / 2.0, y0 + 146.0))
                        .color(rank_col),
                );
                canvas.draw(
                    dsc,
                    DrawParam::default()
                        .dest(Vec2::new(mid - dw / 2.0, y0 + 176.0))
                        .color(Color::from_rgba(205, 205, 205, 215)),
                );
                canvas.draw(
                    kh,
                    DrawParam::default()
                        .dest(Vec2::new(mid - kw / 2.0, y0 + card_h - 46.0))
                        .color(accent),
                );
            }
            Ok(())
        })
    }
}
