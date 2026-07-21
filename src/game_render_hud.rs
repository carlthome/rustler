//! Screen-space HUD and overlay rendering for `MainState`.
//!
//! Extracted from `game_render.rs`: the HUD pass of `draw_game`, everything that draws in
//! screen space (viewport-pinned) rather than world space — weather overlay, minimap, tool
//! roster, screen-edge crab radar, score/groove/combo HUD, boss and upgrade overlays, and the
//! flashlight shader pass. Pure rendering, no gameplay mutation.

use ggez::glam::Vec2;
use ggez::graphics::{
    Canvas, Color, DrawParam, Rect, Text,
};
use ggez::{Context, GameResult};

use crate::constants::*;
use crate::hud_cache::*;
use crate::spawnings::SpawnPattern;
use crate::state::*;
use crate::graphics::{
    cached_stroke_rect, draw_beat_indicator, draw_crab_radar, draw_flashlight,
    draw_groove_vignette, draw_reef_phrase, draw_wave_telegraph,
    draw_weather, unit_square,
};
use crate::graphics::{
    draw_day_weather_hud, draw_king_loadout, draw_minimap, draw_tool_roster, minimap_dimensions,
};

const BEAT_CLOCK_MAP_CLEARANCE: f32 = 86.0;
const BEAT_CLOCK_MIN_X: f32 = 90.0;
const BEAT_CLOCK_Y: f32 = 60.0;

impl MainState {
    /// Screen-space HUD / overlay pass. Called from `draw_game` after the world-space pass;
    /// switches the canvas to viewport coordinates and draws everything that must stay pinned
    /// to the screen as the camera scrolls.
    pub(crate) fn draw_hud(
        &self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        width: f32,
        height: f32,
    ) -> GameResult {
        // ===== SWITCH TO SCREEN SPACE FOR THE HUD =====
        // Everything above draws in world space (the camera-following rect set in draw()); the
        // camera can be scrolled far from the origin. The HUD/overlays below must be pinned to the
        // screen, so re-set the canvas coordinates to a fixed viewport rect (origin 0, plus the
        // same screen-shake offset the world got). ggez allows re-setting coordinates mid-canvas
        // between draws. Every draw after this line lands in screen space.
        canvas.set_screen_coordinates(Rect::new(
            self.screen_shake_offset.x,
            self.screen_shake_offset.y,
            width,
            height,
        ));

        // Weather screen-space pass: rain streaks, heavy-rain edge vignette, and the storm
        // lightning flash. All pinned to the viewport (drawn after the screen-coordinate switch) so
        // rain density and the flash are camera-independent — they don't smear as the world scrolls.
        // beat_intensity drives a subtle on-beat opacity pulse on the streaks.
        draw_weather(
            ctx,
            canvas,
            width,
            height,
            self.time_elapsed,
            self.weather_intensity,
            self.beat_intensity,
            self.lightning_flash,
        )?;

        // Minimap — top-right corner, showing the full scrolling world.
        // Stack-allocate the tiny NPC arrays (≤3 leaders, ≤24 followers) to avoid per-frame heap allocs.
        {
            const MINI_STEPS: usize = 14;
            let mut leader_buf = [(Vec2::ZERO, 0.0_f32); 8];
            let leader_n = self.npc_trains.len().min(8);
            for (i, t) in self.npc_trains.iter().enumerate().take(8) {
                leader_buf[i] = (t.leader_pos, t.leader_scale);
            }
            let mut follower_buf = [Vec2::ZERO; 64];
            let mut follower_n = 0usize;
            for npc in &self.npc_trains {
                for i in 0..npc.follower_types.len() {
                    if follower_n >= follower_buf.len() {
                        break;
                    }
                    if let Some(&p) = npc.path_history.get((i + 1) * MINI_STEPS) {
                        follower_buf[follower_n] = p;
                        follower_n += 1;
                    }
                }
            }
            draw_minimap(
                ctx,
                canvas,
                width,
                height,
                self.world_width,
                self.world_height,
                self.camera_origin,
                self.player_pos,
                self.pen_pos,
                &self.crabs,
                &leader_buf[..leader_n],
                &follower_buf[..follower_n],
                self.time_elapsed,
            )?;
            let (_, map_h) = minimap_dimensions(width, self.world_width, self.world_height);
            draw_day_weather_hud(
                ctx,
                canvas,
                width,
                map_h,
                self.day_phase_t,
                self.weather_intensity,
                self.time_elapsed,
            )?;
        }

        // Tool roster — Zelda-style bar at the bottom centre.
        if !self.show_instructions && !self.game_over && !self.show_world_map {
            // Contextual usefulness: a pad lights up only when firing it now would actually do
            // something (a target is in range). One cheap pass over crabs + one over rival trains,
            // squared-distance so there's no per-frame sqrt.
            let pc = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            let whistle_r2 = {
                let r = self.whistle_max_radius();
                r * r
            };
            let mut whistle_useful = false;
            let mut call_useful = false;
            let mut stomp_useful = false;
            let mut lasso_thief = false;
            let mut lasso_cluster = 0u32;
            for c in &self.crabs {
                if c.caught {
                    continue;
                }
                let d2 = c.pos.distance_squared(pc);
                if c.is_catchable() {
                    if d2 <= whistle_r2 {
                        whistle_useful = true;
                    }
                    if d2 <= 320.0 * 320.0 {
                        lasso_cluster += 1;
                    }
                }
                if c.is_dancer() && d2 <= 420.0 * 420.0 {
                    call_useful = true;
                }
                if !c.is_boss() && c.boss_health > 0.0 && d2 <= STOMP_MAX_RADIUS * STOMP_MAX_RADIUS {
                    stomp_useful = true;
                }
                if c.is_thief() && d2 <= 400.0 * 400.0 {
                    lasso_thief = true;
                }
            }
            let lasso_useful = lasso_thief || lasso_cluster >= 2;
            let mut wave_useful = false;
            for t in &self.npc_trains {
                let d = t.leader_pos.distance(pc);
                if d <= WAVE_DEFEND_RADIUS {
                    wave_useful = true;
                }
                if t.steal_threat > 0.0 && d <= STOMP_DEFEND_RADIUS {
                    stomp_useful = true;
                }
            }
            draw_king_loadout(
                ctx,
                canvas,
                width,
                height,
                self.king_crab_powers,
                [self.beam_rank, self.lasso_rank, self.whistle_rank, self.stomp_rank],
                self.conga_tint,
                self.time_elapsed,
            )?;
            draw_tool_roster(
                ctx,
                canvas,
                width,
                height,
                self.whistle_cooldown,
                crate::WHISTLE_COOLDOWN,
                self.stomp_cooldown,
                crate::STOMP_COOLDOWN,
                self.beat_wave_active,
                self.call_cooldown,
                crate::CALL_COOLDOWN,
                self.boost_cooldown,
                !matches!(self.lasso_phase, LassoPhase::Idle),
                lasso_useful,
                whistle_useful,
                stomp_useful,
                wave_useful,
                call_useful,
                self.groove,
                self.time_elapsed,
                1.0 - (self.beat_timer / self.beat_interval).clamp(0.0, 1.0),
                self.on_beat_action(),
            )?;
        }

        // Screen-edge radar arrows pointing to free crabs — now in the HUD pass so they pin to the
        // viewport border; the camera origin translates each crab's world position into the viewport.
        draw_crab_radar(
            ctx,
            canvas,
            &self.crabs,
            width,
            height,
            self.camera_origin,
            self.beat_intensity,
            self.time_elapsed,
        )?;

        // Show stats. The HUD line changes only on score/combo/tempo events, not every tick, so cache
        // the built Text and only rebuild it (fresh format! String + fresh Text, which re-triggers
        // glyph shaping) when the underlying values actually differ from last frame's.
        // Same pattern as the per-level label cache above. Also use the
        // already-maintained self.chain_count instead of re-scanning every crab for `.caught`
        // every frame just to display the same number (crabs are never removed from the vec —
        // caught state only flips via chain_count-tracked catches/snaps — so the two stay in
        // sync).
        let chain_len = self.chain_count;
        let mult = if self.combo_count >= 3 {
            self.combo_multiplier()
        } else {
            0
        };
        let bpm = if self.beat_interval > 0.0 {
            (60.0 / self.beat_interval).round() as u32
        } else {
            0
        };
        let stats_height = 30.0
            + if self.rhythm_bonus_score > 0 { 20.0 } else { 0.0 }
            + if self.in_campaign && self.tutorial.is_none() {
                20.0
            } else {
                0.0
            };
        canvas.draw(
            unit_square(ctx)?,
            DrawParam::default()
                .dest(Vec2::new(5.0, 5.0))
                .scale(Vec2::new(370.0, stats_height))
                .color(Color::from_rgba(8, 14, 30, 175)),
        );
        let stats_border = cached_stroke_rect(ctx, 370.0, stats_height, 1.0)?;
        canvas.draw(
            &stats_border,
            DrawParam::default()
                .dest(Vec2::new(5.0, 5.0))
                .color(Color::from_rgba(120, 210, 230, 100)),
        );
        HUD_TEXT_CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            let needs_rebuild = match &*cache {
                Some((s, cl, cc, m, cached_bpm, _)) => {
                    *s != self.score || *cl != chain_len || *cc != self.combo_count || *m != mult
                        || *cached_bpm != bpm
                }
                None => true,
            };
            if needs_rebuild {
                let hud = if self.combo_count >= 3 {
                    format!(
                        "Score: {}  |  {} BPM  |  Train: {}  |  Combo x{}  [{}x pts]",
                        self.score, bpm, chain_len, self.combo_count, mult
                    )
                } else {
                    format!("Score: {}  |  {} BPM  |  Train: {}", self.score, bpm, chain_len)
                };
                *cache = Some((
                    self.score,
                    chain_len,
                    self.combo_count,
                    mult,
                    bpm,
                    Text::new(hud),
                ));
            }
            canvas.draw(
                &cache.as_ref().unwrap().5,
                DrawParam::default()
                    .dest(Vec2::new(10.0, 10.0))
                    .color(Color::from_rgb(255, 255, 00)),
            );
        });

        // Rhythm mastery readout, just under the score: the cumulative EXTRA points playing on the
        // beat has earned over a flat-1x run — "how far ahead the beat put you", the legible payoff
        // for flawless on-beat play. Display-only; it never adds score, only reveals what the
        // rhythm multipliers already banked. Hidden until it's nonzero so it doesn't clutter the
        // opening of a run before any groove has landed. Pulses gold on a fat on-beat bank.
        if self.rhythm_bonus_score > 0 {
            RHYTHM_BONUS_CACHE.with(|c| {
                let mut cache = c.borrow_mut();
                let needs_rebuild = match &*cache {
                    Some((n, _)) => *n != self.rhythm_bonus_score,
                    None => true,
                };
                if needs_rebuild {
                    let txt = format!("RHYTHM BONUS  +{}", self.rhythm_bonus_score);
                    *cache = Some((self.rhythm_bonus_score, Text::new(txt)));
                }
                let pop = self.rhythm_bonus_flash;
                // Steady teal, flashing toward bright gold on a bank jump.
                let col = Color::new(0.3 + pop * 0.7, 0.9, 0.7 - pop * 0.5, 1.0);
                canvas.draw(
                    &cache.as_ref().unwrap().1,
                    DrawParam::default()
                        .dest(Vec2::new(10.0, 30.0))
                        .scale(Vec2::splat(1.0 + pop * 0.12))
                        .color(col),
                );
            });
        }

        // Campaign goal counter, under the score/rhythm lines: the live progress toward the
        // level's win condition, so the player always knows where they stand against the goal.
        // Only shows during a campaign run (goal read from the launched world-map node).
        if self.in_campaign && self.tutorial.is_none() {
            if let Some(cond) = self
                .world_map
                .as_ref()
                .and_then(|m| m.selected_level_index())
                .and_then(|i| self.levels.get(i))
                .map(|l| l.win_condition)
            {
                // Bucketed to whole seconds (matches the `{:.0}s` display) so the key only moves
                // as often as the rendered text actually would.
                let hold_key = self.hold_train_timer.round() as i32;
                let key = (
                    self.level_complete,
                    cond,
                    self.banked_crabs_run,
                    self.chain_count,
                    self.shells_cracked_run,
                    hold_key,
                );
                CAMPAIGN_GOAL_CACHE.with(|c| {
                    let mut cache = c.borrow_mut();
                    let needs_rebuild = match &*cache {
                        Some((k, _)) => *k != key,
                        None => true,
                    };
                    if needs_rebuild {
                        let goal = if self.level_complete {
                            "LEVEL COMPLETE!".to_string()
                        } else {
                            cond.progress_text(
                                self.banked_crabs_run,
                                self.chain_count,
                                self.shells_cracked_run,
                                self.hold_train_timer,
                            )
                        };
                        let txt = Text::new(goal);
                        *cache = Some((key, txt));
                    }
                    let col = if self.level_complete {
                        Color::from_rgb(255, 230, 80) // celebratory gold once the goal lands
                    } else {
                        Color::from_rgb(140, 235, 255) // cool goal-teal while it's in progress
                    };
                    canvas.draw(
                        &cache.as_ref().unwrap().1,
                        DrawParam::default().dest(Vec2::new(10.0, 50.0)).color(col),
                    );
                });
            }
        }

        // Debug-only perf overlay, top-right: avg/worst frame time + fps over the last ~2s
        // window (see the accumulation block in update()). Lets a feature/optimizer agent (or
        // Carl) see the cost of whatever just landed without needing a terminal in view.
        #[cfg(debug_assertions)]
        PERF_OVERLAY_CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            // Round to hundredths (matches the displayed precision) so the cache only rebuilds
            // when the printed numbers would actually change, not every frame.
            let avg_key = (self.perf_last_avg_ms * 100.0).round() as i32;
            let worst_key = (self.perf_last_worst_ms * 100.0).round() as i32;
            let crab_key = self.crabs.len() as i32;
            let needs_rebuild = match &*cache {
                Some((a, w, c, _, _)) => *a != avg_key || *w != worst_key || *c != crab_key,
                None => true,
            };
            if needs_rebuild {
                let msg = format!(
                    "avg {:.2}ms ({:.0} fps)  worst {:.2}ms  {} crabs ({} chained)",
                    self.perf_last_avg_ms,
                    self.perf_last_fps,
                    self.perf_last_worst_ms,
                    self.crabs.len(),
                    self.chain_count,
                );
                let text = Text::new(msg);
                let width = text.measure(ctx).map(|m| m.x).unwrap_or(0.0);
                *cache = Some((avg_key, worst_key, crab_key, text, width));
            }
            let (_, _, _, text, width) = cache.as_ref().unwrap();
            canvas.draw(
                text,
                DrawParam::default()
                    .dest(Vec2::new(self.width - width - 10.0, 10.0))
                    .color(Color::from_rgb(120, 255, 120)),
            );
        });

        // Action bars — pushed down so they don't collide with score/rhythm bonus above.
        let bar_x = 10.0;
        let bar_y = 80.0;   // was 50 — now clears score(y=10) + rhythm bonus(y=30) with margin
        let bar_width = 160.0; // was 220 — narrower to feel less heavy
        let bar_height = 10.0; // was 18 — thinner, less dominant
        let max_boost = 0.18;
        let max_cooldown = 0.08;
        let cooldown_ratio = (self.boost_cooldown / max_cooldown).clamp(0.0, 1.0);

        // Dash readiness is already shown in the tool roster, so only surface this temporary
        // meter while the dash is active or recharging.
        if self.boost_timer > 0.0 || cooldown_ratio > 0.0 {
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .dest(Vec2::new(bar_x, bar_y))
                    .scale(Vec2::new(bar_width, bar_height))
                    .color(Color::from_rgb(40, 40, 40)),
            );
            let ratio = ((max_boost - self.boost_timer) / max_boost).clamp(0.0, 1.0);
            if ratio > 0.0 {
                canvas.draw(
                    unit_square(ctx)?,
                    DrawParam::default()
                        .dest(Vec2::new(bar_x, bar_y))
                        .scale(Vec2::new(bar_width * ratio, bar_height))
                        .color(Color::from_rgb(255, 220, 40)),
                );
            }
            if cooldown_ratio > 0.0 {
                canvas.draw(
                    unit_square(ctx)?,
                    DrawParam::default()
                        .dest(Vec2::new(bar_x, bar_y))
                        .scale(Vec2::new(bar_width * cooldown_ratio, bar_height))
                        .color(Color::from_rgb(220, 60, 60)),
                );
            }
            let border = cached_stroke_rect(ctx, bar_width, bar_height, 2.0)?;
            canvas.draw(
                &border,
                DrawParam::default()
                    .dest(Vec2::new(bar_x, bar_y))
                    .color(Color::from_rgb(255, 255, 255)),
            );
            DASH_LABEL_CACHE.with(|c| {
                let mut cache = c.borrow_mut();
                if cache.is_none() {
                    let mut t = Text::new("Space");
                    t.set_scale(13.0);
                    *cache = Some(t);
                }
                canvas.draw(
                    cache.as_ref().unwrap(),
                    DrawParam::default()
                        .dest(Vec2::new(bar_x + bar_width + 5.0, bar_y - 1.0))
                        .color(Color::from_rgba(255, 255, 255, 160)),
                );
            });
        }

        // Draw sprint stamina bar for held Shift sprinting.
        let sprint_y = bar_y + bar_height + 6.0;
        let sprint_height = 10.0;
        let sprint_ratio = (self.sprint_stamina / SPRINT_STAMINA_MAX).clamp(0.0, 1.0);
        canvas.draw(
            unit_square(ctx)?,
            DrawParam::default()
                .dest(Vec2::new(bar_x, sprint_y))
                .scale(Vec2::new(bar_width, sprint_height))
                .color(Color::from_rgb(32, 44, 40)),
        );
        if sprint_ratio > 0.0 {
            let sprint_color = Color::from_rgb(70, 220, 150);
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .dest(Vec2::new(bar_x, sprint_y))
                    .scale(Vec2::new(bar_width * sprint_ratio, sprint_height))
                    .color(sprint_color),
            );
        }
        let sprint_border = cached_stroke_rect(ctx, bar_width, sprint_height, 2.0)?;
        canvas.draw(
            &sprint_border,
            DrawParam::default()
                .dest(Vec2::new(bar_x, sprint_y))
                .color(Color::from_rgb(220, 255, 240)),
        );
        SPRINT_LABEL_CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                let mut t = Text::new("Shift");
                t.set_scale(13.0);
                *cache = Some(t);
            }
            canvas.draw(
                cache.as_ref().unwrap(),
                DrawParam::default()
                    .dest(Vec2::new(bar_x + bar_width + 5.0, sprint_y - 1.0))
                    .color(Color::from_rgba(220, 255, 240, 160)),
            );
        });

        let wbar_y = sprint_y + sprint_height + 6.0;
        let wbar_h = 10.0;
        let ready = self.whistle_cooldown <= 0.0;
        let charge = (1.0 - self.whistle_cooldown / self.whistle_cooldown_dur()).clamp(0.0, 1.0);
        if !ready {
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .dest(Vec2::new(bar_x, wbar_y))
                    .scale(Vec2::new(bar_width, wbar_h))
                    .color(Color::from_rgb(40, 40, 40)),
            );
            let (wr, wg, wb) = (150, 110, 40);
            canvas.draw(
            unit_square(ctx)?,
            DrawParam::default()
                .dest(Vec2::new(bar_x, wbar_y))
                .scale(Vec2::new(bar_width * charge, wbar_h))
                .color(Color::from_rgb(wr, wg, wb)),
            );
            let wborder = cached_stroke_rect(ctx, bar_width, wbar_h, 2.0)?;
            canvas.draw(
            &wborder,
            DrawParam::default()
                .dest(Vec2::new(bar_x, wbar_y))
                .color(Color::from_rgb(255, 255, 255)),
            );
            WHISTLE_LABEL_CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            let needs_rebuild = !matches!(&*cache, Some((r, _)) if *r == ready);
            if needs_rebuild {
                let mut text = Text::new(if ready { "Whistle (E) ✓" } else { "Whistle (E)" });
                text.set_scale(13.0);
                *cache = Some((ready, text));
            }
            canvas.draw(
                &cache.as_ref().unwrap().1,
                DrawParam::default()
                    .dest(Vec2::new(bar_x + bar_width + 5.0, wbar_y - 1.0))
                    .color(Color::from_rgba(255, 230, 150, if ready { 220 } else { 130 })),
            );
            });
        }

        let sbar_y = wbar_y + wbar_h + 6.0;
        let sbar_h = 10.0;
        let sready = self.stomp_cooldown <= 0.0;
        let scharge = (1.0 - self.stomp_cooldown / self.stomp_cooldown_dur()).clamp(0.0, 1.0);
        if !sready {
        canvas.draw(
            unit_square(ctx)?,
            DrawParam::default()
                .dest(Vec2::new(bar_x, sbar_y))
                .scale(Vec2::new(bar_width, sbar_h))
                .color(Color::from_rgb(40, 40, 40)),
        );
        let (sr, sg, sb) = if sready {
            (150, 190, 235)
        } else {
            (80, 105, 135)
        };
        canvas.draw(
            unit_square(ctx)?,
            DrawParam::default()
                .dest(Vec2::new(bar_x, sbar_y))
                .scale(Vec2::new(bar_width * scharge, sbar_h))
                .color(Color::from_rgb(sr, sg, sb)),
        );
        let sborder = cached_stroke_rect(ctx, bar_width, sbar_h, 2.0)?;
        canvas.draw(
            &sborder,
            DrawParam::default()
                .dest(Vec2::new(bar_x, sbar_y))
                .color(Color::from_rgb(255, 255, 255)),
        );
        STOMP_LABEL_CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            let needs_rebuild = !matches!(&*cache, Some((r, _)) if *r == sready);
            if needs_rebuild {
                let mut text = Text::new(if sready { "Stomp (R) ✓" } else { "Stomp (R)" });
                text.set_scale(13.0);
                *cache = Some((sready, text));
            }
            canvas.draw(
                &cache.as_ref().unwrap().1,
                DrawParam::default()
                    .dest(Vec2::new(bar_x + bar_width + 5.0, sbar_y - 1.0))
                    .color(Color::from_rgba(190, 215, 245, if sready { 220 } else { 130 })),
            );
        });
        }

        if self.flashlight.laser_level > 0 || self.flashlight.charge < 1.0 || self.flashlight.on {
            let fbar_y = sbar_y + sbar_h + 6.0;
            let fbar_h = 10.0;
            let fcharge = self.flashlight.charge;
            let fready = fcharge > 0.15;
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .dest(Vec2::new(bar_x, fbar_y))
                    .scale(Vec2::new(bar_width, fbar_h))
                    .color(Color::from_rgb(40, 40, 40)),
            );
            let (fr, fg, fb) = if self.flashlight.on {
                (255, 200, 80)  // bright amber while active
            } else if fready {
                (180, 140, 50)  // dim amber when charged but off
            } else {
                (80, 60, 20)    // dark when drained
            };
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .dest(Vec2::new(bar_x, fbar_y))
                    .scale(Vec2::new(bar_width * fcharge, fbar_h))
                    .color(Color::from_rgb(fr, fg, fb)),
            );
            let fborder = cached_stroke_rect(ctx, bar_width, fbar_h, 2.0)?;
            canvas.draw(
                &fborder,
                DrawParam::default()
                    .dest(Vec2::new(bar_x, fbar_y))
                    .color(Color::from_rgb(255, 255, 255)),
            );
            let fstate: u8 = if self.flashlight.on {
                0
            } else if fready {
                1
            } else {
                2
            };
            FLASHLIGHT_LABEL_CACHE.with(|c| {
                let mut cache = c.borrow_mut();
                let needs_rebuild = !matches!(&*cache, Some((s, _)) if *s == fstate);
                if needs_rebuild {
                    let flabel = match fstate {
                        0 => "Flashlight (F) ON",
                        1 => "Flashlight (F)",
                        _ => "Flashlight (F) recharging...",
                    };
                    let mut text = Text::new(flabel);
                    text.set_scale(13.0);
                    *cache = Some((fstate, text));
                }
                canvas.draw(
                    &cache.as_ref().unwrap().1,
                    DrawParam::default()
                        .dest(Vec2::new(bar_x + bar_width + 8.0, fbar_y - 2.0))
                        .color(Color::from_rgb(255, 200, 100)),
                );
            });
        }

        // Debug info: current level in bottom-left corner, small and unobtrusive. Campaign uses
        // the bounded per-level HashMap cache (levels.len() is small and fixed, so it reaches
        // steady state quickly); endless arcade uses a single-slot cache instead — arcade_stage
        // climbs forever and is never revisited, so keying into the same HashMap would leak one
        // Text entry per stage for the life of the run.
        if self.in_campaign {
            LEVEL_LABEL_CACHE.with(|c| -> GameResult {
                let mut cache = c.borrow_mut();
                let cache_key = self.current_level;
                if !cache.contains_key(&cache_key) {
                    let mut label = Text::new(format!(
                        "Stage {}: {} | {} | Difficulty: {}",
                        self.current_level + 1,
                        self.levels[self.current_level].title,
                        self.levels[self.current_level].description,
                        self.levels[self.current_level].difficulty
                    ));
                    label.set_scale(13.0);
                    let dims = label.measure(ctx)?;
                    cache.insert(cache_key, (label, dims.x, dims.y));
                }
                let (label, _label_width, label_height) = cache.get(&cache_key).unwrap();
                canvas.draw(
                    label,
                    DrawParam::default()
                        .dest(Vec2::new(8.0, height - label_height - 6.0))
                        .color(Color::from_rgba(180, 180, 180, 80)),
                );
                Ok(())
            })?;
        } else {
            ARCADE_STAGE_LABEL_CACHE.with(|c| -> GameResult {
                let mut cache = c.borrow_mut();
                let cache_key = self.arcade_stage;
                let needs_rebuild = !matches!(&*cache, Some((k, ..)) if *k == cache_key);
                if needs_rebuild {
                    let mut label = Text::new(format!(
                        "Stage {}: {} | {} | Difficulty: {}",
                        self.arcade_stage,
                        self.levels[self.current_level].title,
                        self.levels[self.current_level].description,
                        self.levels[self.current_level].difficulty
                    ));
                    label.set_scale(13.0);
                    let dims = label.measure(ctx)?;
                    *cache = Some((cache_key, label, dims.x, dims.y));
                }
                let (_, label, _label_width, label_height) = cache.as_ref().unwrap();
                canvas.draw(
                    label,
                    DrawParam::default()
                        .dest(Vec2::new(8.0, height - label_height - 6.0))
                        .color(Color::from_rgba(180, 180, 180, 80)),
                );
                Ok(())
            })?;
        }

        // Draw level title if timer is active.
        if self.level_title_timer > 0.0 {
            self.draw_level_title(ctx, canvas, width, height)?;
        }

        // Frenzy banner — the staged difficulty spike's on-screen shout. Rides in high so it
        // doesn't collide with the centered level title, fades with its timer, and pulses gold.
        if self.frenzy_banner_timer > 0.0 {
            self.draw_frenzy_banner(ctx, canvas, width, height)?;
        }

        // Stage-up banner — the smooth ramp's on-screen shout when the run climbs into a new
        // intensity stage. Sits a touch lower than the gold Frenzy banner so the two never overlap.
        if self.stage_banner_timer > 0.0 {
            self.draw_stage_banner(ctx, canvas, width, height)?;
        }

        // Tutorial overlay — the "How to Play" instruction card and pass-progress readout, plus the
        // big "PASSED!" celebration once the pass predicate trips. Only present in a tutorial session.
        if self.tutorial.is_some() {
            self.draw_tutorial_overlay(ctx, canvas, width, height)?;
        }

        if self.debug_mode {
            let level = &self.levels[self.current_level];
            let pat = &level.patterns[self.current_pattern];
            let pattern_name = match &pat.pattern {
                SpawnPattern::UniformRandom => "UniformRandom",
                SpawnPattern::SineWave => "SineWave",
                SpawnPattern::Circle => "Circle",
                SpawnPattern::Cluster => "Cluster",
                SpawnPattern::SingleRandom => "SingleRandom",
                SpawnPattern::BeatGrid => "BeatGrid",
                SpawnPattern::Spiral => "Spiral",
            };
            let timer_key = (self.pattern_timer * 100.0).round() as i32;
            DEBUG_TEXT_CACHE.with(|c| {
                let mut cache = c.borrow_mut();
                let needs_rebuild = match &*cache {
                    Some((p, t, _)) => *p != pattern_name || *t != timer_key,
                    None => true,
                };
                if needs_rebuild {
                    let text = Text::new(format!(
                        "[DEBUG] Pattern: {} | Time left: {:.2}s",
                        pattern_name, self.pattern_timer
                    ));
                    *cache = Some((pattern_name, timer_key, text));
                }
                canvas.draw(
                    &cache.as_ref().unwrap().2,
                    DrawParam::default()
                        .dest(Vec2::new(10.0, 200.0))
                        .color(Color::from_rgb(255, 100, 100)),
                );
            });
        }
        // Groove vignette — frame the whole screen in a beat-pulsing edge glow while the player is
        // in the pocket, so "in the groove" reads peripherally, not just from the corner meter.
        // Drawn over the world but under the HUD so it never obscures numbers/readouts.
        // Streak heat: map the on-beat catch streak onto 0..1 so the vignette catches fire as the
        // run climbs the HEATING UP (3) -> ON FIRE (5) -> BLAZING (8) -> INFERNO (12+) tiers. Below
        // the first callout tier there's no heat, so ordinary play stays cool; INFERNO maxes it.
        let streak_heat = if self.beat_streak >= 3 {
            ((self.beat_streak as f32 - 3.0) / 9.0).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let beat_phase = self.beat_timer / self.beat_interval;
        draw_groove_vignette(
            ctx,
            canvas,
            width,
            height,
            self.groove,
            self.beat_intensity,
            streak_heat,
            beat_phase,
        )?;

        // The minimap owns the top-right corner. Keep the beat clock just to its left so its
        // approach and wave rings remain clear instead of drawing over map markers.
        let (map_w, _) = minimap_dimensions(width, self.world_width, self.world_height);
        let beat_center = Vec2::new(
            (width - map_w - BEAT_CLOCK_MAP_CLEARANCE).max(BEAT_CLOCK_MIN_X),
            BEAT_CLOCK_Y,
        );
        // Wave-incoming telegraph: while a spawn is armed, ring the beat indicator so the player
        // sees the next herd will land on the coming downbeat. Anticipation climbs across the
        // couple of beats before the drop; the ring throbs with the beat phase.
        if self.wave_armed {
            let anticipation = (self.wave_telegraph / (self.beat_interval * 4.0)).min(1.0);
            let beat_phase = 1.0 - (self.beat_timer / self.beat_interval).clamp(0.0, 1.0);
            draw_wave_telegraph(
                ctx,
                canvas,
                beat_center,
                anticipation,
                beat_phase,
                self.frenzy_wave,
            )?;
        }
        // beat_timer counts down from beat_interval to 0, so progress toward the next beat is
        // 1 - (timer / interval). Feeds the approach ring so the player can anticipate the downbeat.
        let beat_progress = 1.0 - (self.beat_timer / self.beat_interval).clamp(0.0, 1.0);
        draw_beat_indicator(
            ctx,
            canvas,
            beat_center,
            self.beat_intensity,
            beat_progress,
            self.on_beat_now(),
            (self.beat_count % 4) as usize,
            self.time_elapsed,
        )?;

        // Reef DJ call-and-response phrase — the four beats it called for this bar, drawn just under
        // the beat indicator so it sits with the other rhythm HUD. Only shown during a Reef DJ fight;
        // the player reads which pips are hot and echoes them back with the light on the beat.
        if self.reef_active {
            draw_reef_phrase(
                ctx,
                canvas,
                Vec2::new(width - 50.0, 96.0),
                self.reef_phrase,
                (self.beat_count % 4) as usize,
                self.on_beat_now(),
                self.reef_hit_flash,
            )?;
        }

        // Groove meter (top center) — fills as you catch crabs on the beat, glowing and
        // pulsing to the beat once you're in the pocket. Rewards rhythmic play at a glance.
        if self.groove > 0.01 {
            let gw = 260.0;
            let gh = 14.0;
            let gx = (width - gw) / 2.0;
            let gy = 16.0;
            let maxed = self.groove >= 0.999;
            // The topping-out flash rides on top of the steady maxed pulse, so the bar visibly pops
            // the instant it fills, then settles into its normal in-pocket glow.
            let pulse = if maxed {
                self.beat_intensity * 0.5 + self.groove_full_flash * 0.8
            } else {
                0.0
            };
            // Background track
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .dest(Vec2::new(gx, gy))
                    .scale(Vec2::new(gw, gh))
                    .color(Color::from_rgba(20, 24, 30, 200)),
            );
            // Fill — cyan when building, shifting to hot magenta/gold as it tops out.
            let t = self.groove;
            let r = 0.25 + t * 0.75;
            let g = 0.95 - t * 0.35;
            let b = 0.85 - t * 0.35;
            let bright = 1.0 + pulse;
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .dest(Vec2::new(gx, gy))
                    .scale(Vec2::new(gw * t, gh))
                    .color(Color::new(
                        (r * bright).min(1.0),
                        (g * bright).min(1.0),
                        (b * bright).min(1.0),
                        1.0,
                    )),
            );
            // Border
            let gborder = cached_stroke_rect(ctx, gw, gh, 2.0)?;
            canvas.draw(
                &gborder,
                DrawParam::default()
                    .dest(Vec2::new(gx, gy))
                    .color(Color::from_rgba(
                        255,
                        255,
                        255,
                        if maxed { 255 } else { 160 },
                    )),
            );
            // Label — text/width only change when `maxed` flips, so cache both instead of
            // rebuilding and re-measuring a Text every frame the bar is on screen.
            let lcol = if maxed {
                Color::from_rgb(255, 240, 120)
            } else {
                Color::from_rgba(200, 230, 240, 200)
            };
            GROOVE_LABEL_CACHE.with(|c| -> GameResult {
                let mut cache = c.borrow_mut();
                let needs_rebuild = !matches!(&*cache, Some((m, _, _)) if *m == maxed);
                if needs_rebuild {
                    let mut glabel = Text::new(if maxed {
                        "IN THE GROOVE! — [G] SLAM on beat"
                    } else {
                        "GROOVE"
                    });
                    glabel.set_scale(16.0);
                    let glw = glabel.measure(ctx)?.x;
                    *cache = Some((maxed, glabel, glw));
                }
                let (_, glabel, glw) = cache.as_ref().unwrap();
                canvas.draw(
                    glabel,
                    DrawParam::default()
                        .dest(Vec2::new((width - glw) / 2.0, gy + gh + 3.0))
                        .color(lcol),
                );
                Ok(())
            })?;
        }

        // Groove Gamble multiplier badge — while a hot on-beat streak is live, show the compounding
        // multiplier below the groove meter, glowing hotter the higher it climbs, so the player can
        // see at a glance exactly how much heat is riding on their next catch.
        if self.beat_gamble_mult > 1.01 {
            let t = ((self.beat_gamble_mult - 1.0) / 4.0).clamp(0.0, 1.0); // 0 at 1x, 1 at 5x cap
            // Cyan-green when warming, to gold, to hot red at the cap — matches the callout tiers.
            let (r, g, b) = (0.4 + t * 0.6, 1.0 - t * 0.7, 0.6 - t * 0.5);
            let pop = 1.0 + self.beat_gamble_flash * 0.6 + self.beat_intensity * 0.2;
            // Text/width only change when the multiplier steps (every +0.25) — cache both and
            // apply the per-frame "pop" pulse as a DrawParam scale (cheap) instead of baking it
            // into the font size (forces a re-measure every frame).
            // Cache key folds in both the live multiplier and the locked floor, since the badge text
            // now shows the safe floor too — a bank changes the label without changing the live mult.
            let key = (self.beat_gamble_mult * 100.0).round() as u32
                + ((self.beat_gamble_locked * 100.0).round() as u32) * 1000;
            GAMBLE_BADGE_CACHE.with(|c| -> GameResult {
                let mut cache = c.borrow_mut();
                let needs_rebuild = !matches!(&*cache, Some((k, _, _)) if *k == key);
                if needs_rebuild {
                    // Show the banked floor alongside the live heat when the player has cashed some in.
                    let txt = if self.beat_gamble_locked > 1.01 {
                        format!(
                            "GROOVE GAMBLE  x{:.2}  (x{:.2} safe)",
                            self.beat_gamble_mult, self.beat_gamble_locked
                        )
                    } else {
                        format!("GROOVE GAMBLE  x{:.2}", self.beat_gamble_mult)
                    };
                    let mut badge = Text::new(txt);
                    badge.set_scale(20.0);
                    let bw = badge.measure(ctx)?.x;
                    *cache = Some((key, badge, bw));
                }
                let (_, badge, bw) = cache.as_ref().unwrap();
                let scale = pop.min(1.4);
                let dw = bw * scale;
                // Bank flash washes the badge gold on a successful cash-out.
                let bf = self.gamble_bank_flash;
                let cr = (r * pop + bf * 0.6).min(1.0);
                let cg = (g * pop + bf * 0.5).min(1.0);
                let cb = (b * pop + bf * 0.2).min(1.0);
                canvas.draw(
                    badge,
                    DrawParam::default()
                        .dest(Vec2::new((width - dw) / 2.0, 56.0))
                        .scale(Vec2::new(scale, scale))
                        .color(Color::new(cr, cg, cb, 1.0)),
                );
                Ok(())
            })?;

            // "BANK NOW  [B]" prompt — breathes under the badge while there's an unbanked stack big
            // enough to be worth cashing out, so the player learns the fork is theirs to call.
            // Built once and cached (same static-string-measure pattern as ON_BEAT_TEXT_CACHE /
            // STAMINA_LABEL_CACHE) since it's visible every frame a hot Groove Gamble streak runs.
            if self.beat_gamble_mult > self.beat_gamble_locked + 0.5 {
                let breathe = 0.55 + 0.45 * (self.gamble_bank_pulse.sin() * 0.5 + 0.5);
                BANK_NOW_PROMPT_CACHE.with(|c| -> GameResult {
                    let mut cache = c.borrow_mut();
                    if cache.is_none() {
                        let mut prompt = Text::new("BANK NOW  [B]");
                        prompt.set_scale(18.0);
                        let pw = prompt.measure(ctx)?.x;
                        *cache = Some((prompt, pw));
                    }
                    let (prompt, pw) = cache.as_ref().unwrap();
                    canvas.draw(
                        prompt,
                        DrawParam::default()
                            .dest(Vec2::new((width - pw) / 2.0, 82.0))
                            .color(Color::new(1.0, 0.9, 0.35, breathe)),
                    );
                    Ok(())
                })?;
            }
        }

        // Streak-lost sting — a brief red screen wash when a hot Gamble breaks, so the cost of a
        // greedy off-beat grab lands viscerally, not just as a vanished number.
        if self.streak_lost_flash > 0.0 {
            let alpha = (self.streak_lost_flash * 90.0) as u8;
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .scale(Vec2::new(width, height))
                    .color(Color::from_rgba(200, 40, 40, alpha)),
            );
        }

        // Dash flash — cyan burst when Space is pressed
        if self.dash_flash > 0.0 {
            let alpha = (self.dash_flash * 130.0) as u8;
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .scale(Vec2::new(width, height))
                    .color(Color::from_rgba(220, 240, 255, alpha)),
            );
        }

        // Downbeat Slam flash — warm gold full-screen bloom when the ultimate lands.
        if self.slam_flash > 0.0 {
            let alpha = (self.slam_flash * 150.0) as u8;
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .scale(Vec2::new(width, height))
                    .color(Color::from_rgba(255, 225, 120, alpha)),
            );
        }

        // On-beat catch flash
        if self.on_beat_flash > 0.0 {
            let fa = (self.on_beat_flash * 180.0) as u8;
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .scale(Vec2::new(width, height))
                    .color(Color::from_rgba(255, 220, 80, fa)),
            );
            let btw = ON_BEAT_TEXT_CACHE.with(|c| -> ggez::GameResult<f32> {
                let mut cache = c.borrow_mut();
                if cache.is_none() {
                    let mut bonus_text = Text::new("ON BEAT! +1");
                    bonus_text.set_scale(36.0);
                    let btw = bonus_text.measure(ctx)?.x;
                    *cache = Some((bonus_text, btw));
                }
                Ok(cache.as_ref().unwrap().1)
            })?;
            ON_BEAT_TEXT_CACHE.with(|c| {
                let cache = c.borrow();
                let (bonus_text, _) = cache.as_ref().unwrap();
                canvas.draw(
                    bonus_text,
                    DrawParam::default()
                        .dest(Vec2::new((width - btw) / 2.0, height / 2.0 - 60.0))
                        .color(Color::from_rgba(255, 220, 50, fa)),
                );
            });
        }

        // Flashlight drawn absolutely last — after all instanced mesh draws — because ggez 0.9.3's
        // set_default_shader() doesn't clear the group-3 shader-params bind, and any instanced draw
        // after set_shader_params crashes with a wgpu bind group layout mismatch. The flashlight
        // shader uses center_view (world pos minus camera origin) so it renders correctly in
        // screen-space coordinates.
        if self.flashlight.on {
            draw_flashlight(
                ctx,
                canvas,
                self.player_pos,
                self.flashlight.aim_dir,
                self.time_since_catch,
                &self.flashlight,
                &self.flashlight_shader,
                &self.flashlight_cone_image,
                self.width,
                self.height,
                self.camera_origin,
            )?;
        }
        Ok(())
    }
}
