use ggez::glam::Vec2;
use ggez::graphics::{Canvas, Color, DrawParam, Mesh, Rect, Text};
use ggez::{Context, GameResult};

use crate::MainState;
use crate::graphics::{cached_stroke_rect, unit_square};
use crate::hud_cache::{
    FRENZY_BANNER_CACHE, GAME_OVER_CACHE, INTENSITY_BANNER_CACHE, LEVEL_TITLE_OVERLAY_CACHE,
    TUTORIAL_OVERLAY_CACHE, UPGRADE_SCREEN_CACHE,
};
use crate::upgrade::{UPGRADE_POOL, UpgradeId};

/// Full-screen overlay and HUD-screen drawing: level title cards, frenzy/stage banners, the
/// tutorial instruction card, and the game-over and upgrade-choice screens. Split out of
/// main.rs to keep that file navigable. (World-space crab rendering lives in crab_render.rs.)
impl MainState {
    pub(crate) fn draw_arena_title(
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
        let t = self.arena_title_timer;
        let alpha = if t > 2.8 {
            ((3.1 - t) / 0.3).clamp(0.0, 1.0)
        } else if t < 0.6 {
            (t / 0.6).clamp(0.0, 1.0)
        } else {
            1.0
        };
        // Slide in from left: during fade-in, title slides right into position.
        let slide_x = (1.0 - alpha) * -80.0;

        let level = &self.arenas[self.current_arena.min(self.arenas.len() - 1)];
        let biome = level.biome;

        LEVEL_TITLE_OVERLAY_CACHE.with(|c| -> Result<(), ggez::GameError> {
            let mut cache = c.borrow_mut();
            let needs_rebuild = match &*cache {
                Some((cached_title, cached_biome, _, _, _, _, _, _, _, _, _, _)) => {
                    cached_title != &self.arena_title || *cached_biome != biome.name
                }
                None => true,
            };
            if needs_rebuild {
                // Control style: large title, smaller biome subtitle, threat tag
                let mut title = Text::new(self.arena_title.to_uppercase());
                title.set_scale(72.0);
                let title_dims = title.measure(ctx)?;

                let mut subtitle = Text::new(biome.name.to_uppercase());
                subtitle.set_scale(22.0);
                let sub_dims = subtitle.measure(ctx)?;

                let emphasis = self.arenas[self.current_arena.min(self.arenas.len() - 1)].emphasis;
                let boss = level.boss_for_encounter(self.next_boss_kind);
                // This string and Text are built only when the title-card cache changes, not per
                // animation frame.
                let threat_text = match crate::arenas::emphasis_label(emphasis) {
                    Some(label) => format!("{}  •  {}", label, crate::arenas::boss_label(boss)),
                    None => crate::arenas::boss_label(boss).to_string(),
                };
                let threat_opt = {
                    let mut threat = Text::new(threat_text);
                    threat.set_scale(18.0);
                    let tw = threat.measure(ctx)?.x;
                    Some((threat, tw))
                };

                *cache = Some((
                    self.arena_title.clone(),
                    biome.name,
                    title,
                    // bg_rect slot — unused now, store a dummy
                    Mesh::new_rectangle(
                        ctx,
                        ggez::graphics::DrawMode::fill(),
                        Rect::new(0.0, 0.0, 1.0, 1.0),
                        Color::from_rgba(0, 0, 0, 0),
                    )?,
                    // border_rect slot — unused now
                    Mesh::new_rectangle(
                        ctx,
                        ggez::graphics::DrawMode::fill(),
                        Rect::new(0.0, 0.0, 1.0, 1.0),
                        Color::from_rgba(0, 0, 0, 0),
                    )?,
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

            // Every rect here (strip/accent/rule) animates each frame (alpha fades, slide_x slides
            // the overlay in), so their geometry can't be cached like the Text above. Draw them by
            // scaling the shared unit-square mesh via DrawParam instead of building three fresh
            // Mesh::new_rectangle GPU buffers every frame the title is on screen — the same trick the
            // beat pulse / world-map background use.
            let sq = unit_square(ctx)?;

            // Dark translucent backing strip — full width, left-anchored
            let strip_h = title_h + sub_h + 28.0;
            canvas.draw(
                sq,
                DrawParam::default()
                    .dest(Vec2::new(0.0, anchor_y - 8.0))
                    .scale(Vec2::new(width, strip_h + 16.0))
                    .color(Color::from_rgba(0, 0, 0, (alpha * 140.0) as u8)),
            );

            // Accent line — thin white vertical bar to the left of the text, Control-style
            canvas.draw(
                sq,
                DrawParam::default()
                    .dest(Vec2::new(margin_left + slide_x - 16.0, anchor_y))
                    .scale(Vec2::new(3.0, strip_h - 12.0))
                    .color(Color::from_rgba(255, 255, 255, a)),
            );

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
            canvas.draw(
                sq,
                DrawParam::default()
                    .dest(Vec2::new(margin_left + slide_x, rule_y))
                    .scale(Vec2::new(title_w * 0.6, 1.5))
                    .color(Color::from_rgba(200, 200, 210, a_dim)),
            );

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
    pub(crate) fn draw_intensity_banner(
        &self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        width: f32,
        height: f32,
    ) -> Result<(), ggez::GameError> {
        let life = (self.intensity_banner_timer / 2.0).clamp(0.0, 1.0);
        let alpha = (life * 3.0).min(1.0); // hold, then fade only in the final third
        let beat_phase = 1.0 - (self.beat_timer / self.beat_interval).clamp(0.0, 1.0);
        let throb = (beat_phase * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        let scale = 1.1 - life * 0.12 + throb * 0.05;

        let name = self.intensity_banner_name;
        let dims = INTENSITY_BANNER_CACHE.with(|cache_cell| -> Result<Vec2, ggez::GameError> {
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
        INTENSITY_BANNER_CACHE.with(|cache_cell| {
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
    /// — the card mesh is keyed by (width, height) bit-waves (same as MENU_PANEL_CACHE)
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

    pub(crate) fn draw_game_over_screen(
        &self,
        ctx: &mut Context,
        canvas: &mut Canvas,
    ) -> GameResult {
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
                    "Game Over!\nThis run: {} crabs banked\nTime: {:.2}s   Best time: {:.2}s\n\nCareer best: {}\nCareer total: {} over {} runs\n\nPress Space or Enter to try again.  Esc for menu.",
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

        // Translucent scrim — this is now a LIVE overlay over the still-running game (see the draw
        // dispatch in game_render.rs), so the fill is deliberately semi-transparent: the world keeps
        // moving visibly behind the cards, giving the "pick fast, a rival could steal from you" urge
        // the ROADMAP calls for, instead of the old opaque world-freeze. Reuse the cached unit square
        // instead of a fresh Mesh::new_rectangle GPU buffer every frame.
        canvas.draw(
            unit_square(ctx)?,
            DrawParam::default()
                .scale(Vec2::new(w, h))
                .color(Color::from_rgba(8, 4, 22, 120)),
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
                let mut hint_text = Text::new(
                    "Pick fast — the beach keeps moving! Click a card or press its number",
                );
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
                // Cards sit over a LIVE, moving scene now (the scrim is only half-opaque), so keep
                // the card fills a touch more solid than before to hold the text legible against
                // whatever is scrolling behind them.
                let bg_a = if hovered { 220u8 } else { 175u8 };
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
