//! Scene rendering for `MainState`.
//!
//! Extracted from `main.rs`: the top-level draw path (`draw_scene` -> `draw_game`),
//! the how-to-play/menu screen (`draw_instructions_screen`), and the camera helper
//! (`compute_camera_origin`). Pure rendering — no gameplay-logic mutation. The heavy
//! per-widget drawing lives in `graphics.rs` and `overlays.rs`; this module just
//! orchestrates them into a frame.

use ggez::audio::SoundSource;
use ggez::glam::Vec2;
use ggez::graphics::{
    BlendMode, Canvas, Color, DrawParam, Rect, Sampler, Text,
};
use ggez::input::keyboard::KeyCode;
use ggez::winit::keyboard::PhysicalKey;
use ggez::{Context, GameResult};

use crate::constants::*;
use crate::hud_cache::*;
use crate::state::*;
use crate::{how_to_play_body_text, menu};
use crate::graphics::{
    LassoDrawPhase, draw_beat_hit_punch, draw_beat_wave_ring,
    draw_call_ring, draw_catch_shockwaves, draw_catch_trails,
    draw_beat_keeper_ring, draw_cleave_slash, draw_combo_meter,
    draw_downbeat_pulse_ring, draw_fear_rings,
    draw_floating_texts, draw_groove_call_ring, draw_lasso,
    draw_lasso_windup, draw_particles, draw_pen_guide,
    draw_rustler, draw_slam_ring,
    draw_speed_lines, draw_sprint_whoosh, draw_stomp_ring, draw_tide_pulses, draw_whistle_ring, draw_world_map, unit_circle, unit_line, unit_square,
};
use crate::graphics::{
    draw_beam_fast_pin, draw_beam_golden_spotlight, draw_beam_hermit_match, draw_beam_sneaky_pin,
    draw_lasso_big_match, draw_lasso_magnet_match,
    draw_lasso_shell_deflect, draw_lasso_thief_match, draw_magnet_cluster_pull,
    draw_stomp_armored_crack, draw_stomp_dancer_match, draw_whistle_dancer_match,
    draw_whistle_golden_pull, draw_whistle_shell_deflect, draw_whistle_sneaky_match,
    draw_whistle_thief_match,
};

impl MainState {
    fn draw_startup_logo(
        &self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        width: f32,
        height: f32,
        alpha: f32,
    ) -> GameResult {
        const LOGO_SCALE: f32 = 62.0;
        const PRESENTS_SCALE: f32 = 16.0;
        const LOGO_Y: f32 = 0.43;
        const SKIP_Y: f32 = 0.9;
        const SPARKLE_ROTATION_SPEED: f32 = 0.32;

        let square = unit_square(ctx)?;
        canvas.draw(
            square,
            DrawParam::default()
                .scale(Vec2::new(width, height))
                .color(Color::BLACK),
        );

        if alpha > 0.0 {
            // Static strings, drawn every frame for the whole intro — cache the shaped Text (and
            // its measured width) once instead of rebuilding + re-shaping glyphs each frame.
            let logo_width = STARTUP_LOGO_TEXT_CACHE.with(|c| -> GameResult<f32> {
                let mut cache = c.borrow_mut();
                if cache.is_none() {
                    let mut logo = Text::new("CARLTHOME");
                    logo.set_scale(LOGO_SCALE);
                    let w = logo.measure(ctx)?.x;
                    *cache = Some((logo, w));
                }
                let (text, w) = cache.as_ref().unwrap();
                canvas.draw(
                    text,
                    DrawParam::default()
                        .dest(Vec2::new((width - w) * 0.5, height * LOGO_Y))
                        .color(Color::new(0.88, 0.94, 1.0, alpha)),
                );
                Ok(*w)
            })?;

            STARTUP_PRESENTS_TEXT_CACHE.with(|c| -> GameResult {
                let mut cache = c.borrow_mut();
                if cache.is_none() {
                    let mut presents = Text::new("P R E S E N T S");
                    presents.set_scale(PRESENTS_SCALE);
                    let w = presents.measure(ctx)?.x;
                    *cache = Some((presents, w));
                }
                let (text, w) = cache.as_ref().unwrap();
                canvas.draw(
                    text,
                    DrawParam::default()
                        .dest(Vec2::new((width - w) * 0.5, height * LOGO_Y + 82.0))
                        .color(Color::new(0.55, 0.68, 0.82, alpha * 0.8)),
                );
                Ok(())
            })?;

            let sparkle = unit_circle(ctx)?;
            let center = Vec2::new(width * 0.5 + logo_width * 0.55, height * LOGO_Y + 8.0);
            for ray in 0..8 {
                let angle = ray as f32 * std::f32::consts::FRAC_PI_4
                    + self.menu_intro_time * SPARKLE_ROTATION_SPEED;
                let radius = if ray % 2 == 0 { 23.0 } else { 14.0 };
                canvas.draw(
                    sparkle,
                    DrawParam::default()
                        .dest(center + Vec2::from_angle(angle) * radius)
                        .scale(Vec2::splat(if ray % 2 == 0 { 2.2 } else { 1.2 }))
                        .color(Color::new(0.82, 0.94, 1.0, alpha)),
                );
            }
        }

        STARTUP_SKIP_TEXT_CACHE.with(|c| -> GameResult {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                let mut skip = Text::new("SPACE TO SKIP");
                skip.set_scale(15.0);
                let w = skip.measure(ctx)?.x;
                *cache = Some((skip, w));
            }
            let (text, w) = cache.as_ref().unwrap();
            canvas.draw(
                text,
                DrawParam::default()
                    .dest(Vec2::new((width - w) * 0.5, height * SKIP_Y))
                    .color(Color::new(0.5, 0.56, 0.66, 0.7)),
            );
            Ok(())
        })?;
        Ok(())
    }

    fn draw_instructions_screen(
        &mut self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        width: f32,
        height: f32,
    ) -> GameResult {
        if self.show_how_to_play_text {
            // Moonlit-beach backdrop — the same night mood as the main menu, so the two screens
            // read as one place instead of the flat green clear behind this page. Draw-only: a
            // vertical gradient, a scatter of twinkling stars, a soft moon, and a translucent panel
            // behind the text so the words stay crisp over the gradient.
            let t = self.menu_time;
            let strips = 28;
            let top = Color::from_rgb(9, 12, 34);
            let mid = Color::from_rgb(48, 26, 66);
            let sand = Color::from_rgb(74, 58, 78);
            let lerp = |a: Color, b: Color, k: f32| {
                Color::new(
                    a.r + (b.r - a.r) * k,
                    a.g + (b.g - a.g) * k,
                    a.b + (b.b - a.b) * k,
                    1.0,
                )
            };
            let strip_h = height / strips as f32;
            let sq = unit_square(ctx)?;
            for i in 0..strips {
                let k = i as f32 / (strips - 1) as f32;
                let c = if k < 0.65 {
                    lerp(top, mid, k / 0.65)
                } else {
                    lerp(mid, sand, (k - 0.65) / 0.35)
                };
                canvas.draw(
                    sq,
                    DrawParam::default()
                        .dest(Vec2::new(0.0, i as f32 * strip_h))
                        .scale(Vec2::new(width, strip_h + 1.0))
                        .color(c),
                );
            }
            let dot = unit_circle(ctx)?;
            let hash = |n: u32| {
                let mut x = n.wrapping_mul(2654435761);
                x ^= x >> 15;
                x = x.wrapping_mul(2246822519);
                x ^= x >> 13;
                x
            };
            for i in 0..60u32 {
                let sx = (hash(i) % 1000) as f32 / 1000.0 * width;
                let sy = (hash(i * 7 + 1) % 1000) as f32 / 1000.0 * height * 0.55;
                let phase = (hash(i * 13 + 3) % 628) as f32 / 100.0;
                let speed = 1.2 + (hash(i * 17 + 5) % 200) as f32 / 100.0;
                let twinkle = 0.25 + 0.75 * (t * speed + phase).sin().abs();
                let r = 0.7 + (hash(i * 19 + 7) % 100) as f32 / 100.0 * 1.6;
                canvas.draw(
                    dot,
                    DrawParam::default()
                        .dest(Vec2::new(sx, sy))
                        .scale(Vec2::splat(r))
                        .color(Color::new(1.0, 1.0, 0.92, twinkle)),
                );
            }
            let moon = Vec2::new(width * 0.84, height * 0.18);
            for ring in (0..6).rev() {
                let rr = 30.0 + ring as f32 * 14.0;
                let a = 0.05 + (5 - ring) as f32 * 0.03;
                canvas.draw(
                    dot,
                    DrawParam::default()
                        .dest(moon)
                        .scale(Vec2::splat(rr))
                        .color(Color::new(0.95, 0.93, 0.8, a)),
                );
            }
            canvas.draw(
                dot,
                DrawParam::default()
                    .dest(moon)
                    .scale(Vec2::splat(26.0))
                    .color(Color::new(0.98, 0.96, 0.86, 1.0)),
            );
            // Translucent panel behind the text so it reads cleanly over the gradient.
            canvas.draw(
                sq,
                DrawParam::default()
                    .dest(Vec2::new(width * 0.08, height * 0.2))
                    .scale(Vec2::new(width * 0.84, height * 0.66))
                    .color(Color::from_rgba(10, 14, 30, 170)),
            );

            let mut title = Text::new("HOW TO PLAY");
            title.set_scale(56.0);
            let title_w = title.measure(ctx)?.x;
            canvas.draw(
                &title,
                DrawParam::default()
                    .dest(Vec2::new((width - title_w) * 0.5, height * 0.1))
                    .color(Color::from_rgb(245, 238, 210)),
            );

            let body = how_to_play_body_text();
            let mut text = Text::new(body);
            text.set_scale(27.0);
            canvas.draw(
                &text,
                DrawParam::default()
                    .dest(Vec2::new(width * 0.13, height * 0.23))
                    .color(Color::from_rgb(226, 226, 226)),
            );
            return Ok(());
        }
        if !self.menu_intro_complete {
            let intro = crate::menu_intro::presentation(self.menu_intro_time);
            if intro.menu_progress > 0.0 {
                let offset = height * (1.0 - intro.menu_progress);
                canvas.set_screen_coordinates(Rect::new(0.0, -offset, width, height));
                menu::draw_menu(self, ctx, canvas, width, height)?;
                canvas.set_screen_coordinates(Rect::new(0.0, 0.0, width, height));
                canvas.draw(
                    unit_square(ctx)?,
                    DrawParam::default()
                        .scale(Vec2::new(width, height))
                        .color(Color::new(0.0, 0.0, 0.0, 1.0 - intro.menu_progress)),
                );
            } else {
                canvas.set_screen_coordinates(Rect::new(0.0, 0.0, width, height));
                self.draw_startup_logo(ctx, canvas, width, height, intro.logo_alpha)?;
            }
            return Ok(());
        }
        menu::draw_menu(self, ctx, canvas, width, height)
    }

    /// Top-left world coordinate of the visible viewport: centre the player, then clamp so the
    /// camera never shows past the world's edges (no void beyond the playfield). When the world is
    /// smaller than the viewport in a dimension the clamp collapses to 0 (whole world visible).
    pub(crate) fn compute_camera_origin(&self) -> Vec2 {
        let focus = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        let x = (focus.x - self.width / 2.0).clamp(0.0, (self.world_width - self.width).max(0.0));
        let y =
            (focus.y - self.height / 2.0).clamp(0.0, (self.world_height - self.height).max(0.0));
        Vec2::new(x, y)
    }

    fn draw_game(
        &self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        width: f32,
        height: f32,
    ) -> GameResult {
        // World backdrop (ground/terrain, delivery pen, in-world train readouts) — everything under the crabs.
        self.draw_world_backdrop(ctx, canvas, width, height)?;

        // Draw all crabs.
        self.draw_crabs_with_shake(ctx, canvas)?;

        // Player-anchored beat-keeper: an anticipatory metronome ring that contracts onto the
        // rustler and snaps on each beat, so the beat stays legible while your eyes are on the herd
        // rather than the top-right indicator (#164/#165 — obvious to play while steering). Fades as
        // the train grows: bold while you're learning to tap the beat, a faint tick once you're
        // grooving with a big train, so it never clutters or fights the catch-bloom.
        {
            let pc = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            let beat_progress = 1.0 - (self.beat_timer / self.beat_interval).clamp(0.0, 1.0);
            let downbeat = self.beat_count % 4 == 0;
            let guide = (1.0 - self.chain_count as f32 / 14.0).clamp(0.3, 1.0);
            draw_beat_keeper_ring(
                ctx,
                canvas,
                pc,
                beat_progress,
                self.on_beat_flash,
                downbeat,
                guide,
            )?;
        }

        // Draw player character after crabs so the rustler always renders on top of the conga
        // train rather than being occluded by crabs that overlap its position.
        // Jam emote (B key): shimmy the player position side-to-side for a fun wiggle.
        let jam_shimmy = if self.jam_timer > 0.0 {
            let phase = (self.jam_timer / 0.55) * std::f32::consts::TAU * 4.0;
            Vec2::new(
                phase.sin() * 6.0 * self.jam_timer / 0.55,
                (phase * 0.7).cos() * 3.0,
            )
        } else {
            Vec2::ZERO
        };
        draw_rustler(
            ctx,
            canvas,
            self.player_pos + jam_shimmy,
            &self.textures.player,
            self.player_vel,
            self.beat_intensity,
            self.time_elapsed,
            self.boost_timer > 0.0,
            self.player_skin,
        )?;

        let player_name = crate::normalize_player_name(&self.player_name);
        let player_name_w = PLAYER_NAME_CACHE.with(|c| -> GameResult<f32> {
            let mut cache = c.borrow_mut();
            let needs_rebuild = cache
                .as_ref()
                .map_or(true, |(name, _, _)| name != &player_name);
            if needs_rebuild {
                let mut text = Text::new(player_name.as_str());
                text.set_scale(16.0);
                let w = text.measure(ctx)?.x;
                *cache = Some((player_name.clone(), text, w));
            }
            Ok(cache.as_ref().unwrap().2)
        })?;
        PLAYER_NAME_CACHE.with(|c| {
            let cache = c.borrow();
            if let Some((_, text, _)) = cache.as_ref() {
                let player_center =
                    self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
                let name_pos = player_center - Vec2::new(player_name_w / 2.0, 42.0);
                canvas.draw(
                    text,
                    DrawParam::default()
                        .dest(name_pos + Vec2::splat(1.5))
                        .color(Color::from_rgba(0, 0, 0, 180)),
                );
                canvas.draw(
                    text,
                    DrawParam::default()
                        .dest(name_pos)
                        .color(Color::new(0.96, 0.82, 0.3, 0.95)),
                );
            }
        });

        let sprinting = (ctx.keyboard.is_physical_key_pressed(&PhysicalKey::Code(KeyCode::ShiftLeft))
            || ctx.keyboard.is_physical_key_pressed(&PhysicalKey::Code(KeyCode::ShiftRight)))
            && self.sprint_stamina > 0.0
            && self.boost_timer <= 0.0;

        // Sprint whoosh: a longer green wake behind the crab while Shift is held, so the extra
        // speed reads as motion instead of just a number change.
        if sprinting && self.last_dir.length() > 0.01 {
            let center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            let intensity = (self.sprint_stamina / SPRINT_STAMINA_MAX).clamp(0.25, 1.0);
            draw_sprint_whoosh(
                ctx,
                canvas,
                center,
                self.last_dir,
                self.time_elapsed,
                intensity,
            )?;
        }

        // Speed lines trailing behind player while dashing. Uses the cached unit-line mesh
        // (see draw_speed_lines) instead of building up to 7 fresh Mesh::new_line GPU buffers
        // every single frame of the dash window.
        if self.boost_timer > 0.0 && self.last_dir.length() > 0.01 {
            let center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            let intensity = self.boost_timer / 0.18;
            draw_speed_lines(ctx, canvas, center, self.last_dir, intensity)?;
        }

        // (Radar arrows are screen-edge indicators — drawn in the HUD pass below, after the switch
        // to screen space, so they pin to the viewport border rather than scrolling with the world.)

        // Point the player at the delivery pen while there's a train to cash in. The pen jumps on
        // every bank, so this keeps its "route the train here" decision legible instead of a hunt.
        // Urgency scales with train size (normalized against a fat-haul cap of 12) so a big, at-risk
        // conga line pulls harder toward the pen than a couple of crabs.
        if self.chain_count > 0 {
            let urgency = (self.chain_count as f32 / 12.0).min(1.0);
            draw_pen_guide(
                ctx,
                canvas,
                self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0),
                self.pen_pos,
                PEN_RADIUS,
                width,
                height,
                self.camera_origin,
                urgency,
                self.beat_intensity,
                self.time_elapsed,
            )?;
        }

        // Draw the whip-streaks that yank caught crabs into the train (under the impact rings).
        draw_catch_trails(ctx, canvas, &self.catch_trails)?;

        // Draw catch impact shockwaves (over the crabs, under score text)
        draw_catch_shockwaves(ctx, canvas, &self.catch_shockwaves)?;

        // Beat-hit punch flashes — additive impact + resonance ring at each on-beat catch position
        for &(pos, color, quality) in &self.beat_punch_events {
            draw_beat_hit_punch(ctx, canvas, pos, color, quality)?;
        }

        // Bond-forming flash arcs: bright connecting flash between newly-bonded same-type neighbors.
        // Draw the arc using a scaled unit_line between the two positions.
        {
            let unit_ln = unit_line(ctx)?;
            for &(from, to, color, age) in &self.bond_flash_events {
                let diff = to - from;
                let len = diff.length();
                if len < 1.0 {
                    continue;
                }
                let angle = diff.y.atan2(diff.x);
                let alpha = age * 0.85; // fades from 0.85 to 0 as age goes 1→0
                let thickness = 3.5 * age;
                // Main arc line
                canvas.set_blend_mode(BlendMode::ADD);
                canvas.draw(
                    unit_ln,
                    DrawParam::default()
                        .dest(from)
                        .scale(Vec2::new(len, thickness))
                        .rotation(angle)
                        .color(Color::new(color[0], color[1], color[2], alpha)),
                );
                // Bright center spine
                canvas.draw(
                    unit_ln,
                    DrawParam::default()
                        .dest(from)
                        .scale(Vec2::new(len, thickness * 0.4))
                        .rotation(angle)
                        .color(Color::new(
                            (color[0] * 0.5 + 0.5).min(1.0),
                            (color[1] * 0.5 + 0.5).min(1.0),
                            (color[2] * 0.5 + 0.5).min(1.0),
                            alpha * 0.9,
                        )),
                );
                // End-caps
                let dot = unit_circle(ctx)?;
                canvas.draw(
                    dot,
                    DrawParam::default()
                        .dest(from)
                        .scale(Vec2::splat(thickness * 1.8))
                        .color(Color::new(color[0], color[1], color[2], alpha * 0.8)),
                );
                canvas.draw(
                    dot,
                    DrawParam::default()
                        .dest(to)
                        .scale(Vec2::splat(thickness * 1.8))
                        .color(Color::new(color[0], color[1], color[2], alpha * 0.8)),
                );
                canvas.set_blend_mode(BlendMode::ALPHA);
            }
        }

        // Draw stampede fear rings where catches startled the herd
        draw_fear_rings(ctx, canvas, &self.fear_rings)?;

        // Draw Tide Boss shockwave pulses sweeping outward
        draw_tide_pulses(ctx, canvas, &self.tide_pulses, TIDE_PULSE_RADIUS)?;

        // Draw King Crab stolen crabs — magnetically flying toward the boss.
        // Reuses unit_circle scaled via DrawParam instead of building two Mesh::new_circle GPU
        // buffers per stolen crab per frame (the previous approach).
        {
            let dot = unit_circle(ctx)?;
            for (pos, timer, color) in &self.king_stolen_crabs {
                let t = (*timer / 0.9_f32).clamp(0.0, 1.0);
                let alpha = t;
                let size = CRAB_SIZE * (0.6 + 0.4 * t);
                let draw_pos = *pos - self.camera_origin;
                let r = color[0] * 0.6 + 0.6 * (1.0 - t);
                let g = color[1] * t * 0.5;
                let gb = color[2] * t * 0.8 + 0.5 * (1.0 - t);
                // Fill disc
                canvas.draw(
                    dot,
                    DrawParam::default()
                        .dest(draw_pos)
                        .scale(Vec2::splat(size * 0.5))
                        .color(Color::new(r, g, gb, alpha)),
                );
                // Outer magenta ring — use a slightly larger fill at lower alpha to fake a stroke ring
                canvas.draw(
                    dot,
                    DrawParam::default()
                        .dest(draw_pos)
                        .scale(Vec2::splat(size * 0.7))
                        .color(Color::new(1.0, 0.3, 0.9, alpha * 0.35)),
                );
            }
        }

        // Draw particle effects
        draw_particles(ctx, canvas, &self.particle_system)?;
        draw_floating_texts(ctx, canvas, &self.floating_texts)?;

        // Draw combo meter around player
        draw_combo_meter(
            ctx,
            canvas,
            self.player_pos,
            PLAYER_SIZE,
            self.combo_count,
            self.combo_timer,
            self.beat_intensity,
            self.time_elapsed,
        )?;

        // Draw beat wave circle outline. Uses cached_stroke_circle (via draw_beat_wave_ring)
        // instead of building a fresh Mesh::new_circle GPU buffer every frame the wave expands.
        if self.beat_wave_active && self.beat_wave_radius > 0.0 {
            let player_center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            draw_beat_wave_ring(ctx, canvas, player_center, self.beat_wave_radius)?;
        }

        // Groove Dash gather ring — a ring that contracts toward the dash's landing point over the
        // gather window, reading as the herd being hoovered into your slipstream. Drawn at the point
        // ahead of the dash (origin + heading*reach) so the tell lines up with where crabs funnel.
        if self.groove_dash_timer > 0.0 && self.groove_dash_dir.length() > 0.01 {
            let reach = 170.0;
            let t = (self.groove_dash_timer / 0.22).clamp(0.0, 1.0); // 1 → 0 over the window
            let ring_r = 30.0 + reach * t; // contracts inward as the wake finishes
            let target = self.groove_dash_center + self.groove_dash_dir * reach;
            draw_beat_wave_ring(ctx, canvas, target, ring_r)?;
        }

        // Draw the whistle sonic pulse
        if self.whistle_active > 0.0 && self.whistle_radius > 0.0 {
            draw_whistle_ring(
                ctx,
                canvas,
                self.whistle_center,
                self.whistle_radius,
                self.whistle_max_radius() * self.whistle_beat_bonus,
            )?;
        }

        // Draw the stomp ground-pound shockwave
        if self.stomp_active > 0.0 && self.stomp_radius > 0.0 {
            draw_stomp_ring(
                ctx,
                canvas,
                self.stomp_center,
                self.stomp_radius,
                self.stomp_max_radius() * self.stomp_beat_bonus,
            )?;
        }

        // Strong-match archetype-tool visual feedback.
        if !self.beam_hermit_hits_buf.is_empty() {
            draw_beam_hermit_match(ctx, canvas, &self.beam_hermit_hits_buf)?;
        }
        if !self.beam_golden_hits_buf.is_empty() {
            draw_beam_golden_spotlight(ctx, canvas, &self.beam_golden_hits_buf)?;
        }
        if !self.beam_fast_hits_buf.is_empty() {
            draw_beam_fast_pin(ctx, canvas, &self.beam_fast_hits_buf)?;
        }
        if !self.beam_sneaky_hits_buf.is_empty() {
            draw_beam_sneaky_pin(ctx, canvas, &self.beam_sneaky_hits_buf)?;
        }
        if !self.stomp_dancer_hits_buf.is_empty() {
            draw_stomp_dancer_match(ctx, canvas, &self.stomp_dancer_hits_buf)?;
        }
        if !self.lasso_thief_hits_buf.is_empty() {
            draw_lasso_thief_match(ctx, canvas, &self.lasso_thief_hits_buf)?;
        }
        if !self.lasso_magnet_hits_buf.is_empty() {
            draw_lasso_magnet_match(ctx, canvas, &self.lasso_magnet_hits_buf)?;
        }
        if !self.lasso_big_hits_buf.is_empty() {
            draw_lasso_big_match(ctx, canvas, &self.lasso_big_hits_buf)?;
        }
        if !self.lasso_shell_deflect_hits_buf.is_empty() {
            draw_lasso_shell_deflect(ctx, canvas, &self.lasso_shell_deflect_hits_buf)?;
        }
        if !self.whistle_shell_deflect_hits_buf.is_empty() {
            draw_whistle_shell_deflect(ctx, canvas, &self.whistle_shell_deflect_hits_buf)?;
        }
        if !self.magnet_cluster_hits_buf.is_empty() {
            draw_magnet_cluster_pull(ctx, canvas, &self.magnet_cluster_hits_buf)?;
        }
        if !self.stomp_armored_hits_buf.is_empty() {
            draw_stomp_armored_crack(ctx, canvas, &self.stomp_armored_hits_buf)?;
        }
        if !self.whistle_golden_hits_buf.is_empty() {
            draw_whistle_golden_pull(ctx, canvas, &self.whistle_golden_hits_buf)?;
        }
        if !self.whistle_dancer_hits_buf.is_empty() {
            draw_whistle_dancer_match(ctx, canvas, &self.whistle_dancer_hits_buf)?;
        }
        if !self.whistle_sneaky_hits_buf.is_empty() {
            draw_whistle_sneaky_match(ctx, canvas, &self.whistle_sneaky_hits_buf)?;
        }
        if !self.whistle_thief_hits_buf.is_empty() {
            draw_whistle_thief_match(ctx, canvas, &self.whistle_thief_hits_buf)?;
        }

        // Draw the rhythm Call summon pulse — magenta rings collapsing toward the player.
        if self.call_pulse > 0.0 {
            draw_call_ring(ctx, canvas, self.call_pulse_center, self.call_pulse, 420.0)?;
        }

        // Groove-Call answer streaks — comet trails from the answering herd toward the player, thrown
        // on each beat, so the field-wide lunge reads in a single frame. Drawn before the ring so the
        // ring's broadcast wash sits on top. Reuses the catch-trail draw (additive comet streaks).
        if !self.call_streaks.is_empty() {
            draw_catch_trails(ctx, canvas, &self.call_streaks)?;
        }

        // Draw the Groove Call broadcast — cyan rings sweeping outward across the field while the
        // field-wide herd lure is answering (re-kicked each downbeat), so the arena-scale summons reads.
        if self.groove_call_pulse > 0.0 {
            // Each chained echo reaches the ring further across the field, so a longer call-and-response
            // phrase reads as the whole arena answering — the watchable payoff for staying in the pocket.
            let reach = 720.0 + 120.0 * self.groove_call_echo as f32;
            draw_groove_call_ring(
                ctx,
                canvas,
                self.groove_call_center,
                self.groove_call_pulse,
                reach,
            )?;
            // A brief bright secondary ring snaps out the instant an echo lands, so the answered beat pops.
            if self.groove_call_echo_flash > 0.0 {
                draw_groove_call_ring(
                    ctx,
                    canvas,
                    self.groove_call_center,
                    self.groove_call_echo_flash,
                    reach * 0.55,
                )?;
            }
        }

        // Draw the passive downbeat herd-pulse cue — warm rings collapsing toward the player on the
        // "1" of the bar, so the always-on rhythmic routing tug is legible without a keypress.
        if self.downbeat_pull > 0.0 {
            draw_downbeat_pulse_ring(
                ctx,
                canvas,
                self.downbeat_pull_center,
                self.downbeat_pull,
                300.0, // matches DOWNBEAT_PULL_RADIUS in update_crabs
                self.downbeat_pull_haul,
            )?;
        }

        // Draw the Downbeat Slam shockwave — the big gold rhythm-ultimate blast.
        if self.slam_active > 0.0 && self.slam_radius > 0.0 {
            draw_slam_ring(ctx, canvas, self.slam_center, self.slam_radius, SLAM_RADIUS)?;
        }

        // Cleave slash — the blade stroke bisecting the train the instant a Splitter cuts it.
        if self.cleave_flash > 0.0 {
            draw_cleave_slash(
                ctx,
                canvas,
                self.cleave_a,
                self.cleave_b,
                self.cleave_flash,
                self.cleave_gold,
            )?;
        }

        // Drum Roll telegraph: while holding T and building a charge, pulse tightening rings at the
        // player (reuses the Call-ring draw) so the roll reads as a visible wind-up before release —
        // the more hits banked, the tighter/brighter. On the fired blast the ring flashes out wide.
        if self.drum_roll_charge > 0.02 || self.drum_roll_fire > 0.0 {
            let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            if self.drum_roll_fire > 0.0 {
                draw_call_ring(ctx, canvas, center, self.drum_roll_fire, 340.0)?;
            } else {
                // Charging: a small, growing beckon-ring — pulse tracks the charge, reach grows with it.
                let reach = 60.0 + 120.0 * self.drum_roll_charge;
                draw_call_ring(ctx, canvas, center, self.drum_roll_charge.min(1.0), reach)?;
            }
        }

        // Draw lasso: winding-up OR in-flight (Throwing/Snag/Dragging/Miss).
        {
            let player_center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            match self.lasso_phase {
                LassoPhase::Winding => {
                    // Windup: spinning rope loop above/around the player, grows with charge.
                    // Pulses brighter on each beat so the player can time the release.
                    let charge_frac = (self.lasso_charge / LASSO_MAX_CHARGE_TIME).min(1.0);
                    // Beat-proximity pulse: brighter the closer to the beat edge.
                    let to_beat = self.beat_timer.min(self.beat_interval - self.beat_timer);
                    let beat_prox = (1.0 - to_beat / (BEAT_WINDOW * 1.5)).clamp(0.0, 1.0);
                    draw_lasso_windup(
                        ctx,
                        canvas,
                        player_center,
                        charge_frac,
                        beat_prox,
                        self.lasso_spin,
                    )?;
                }
                LassoPhase::Throwing
                | LassoPhase::Snag
                | LassoPhase::Dragging
                | LassoPhase::Miss => {
                    if let Some(tip) = self.lasso_pos {
                        let (dur, draw_phase) = match self.lasso_phase {
                            LassoPhase::Throwing => (LASSO_THROW_TIME, LassoDrawPhase::Throw),
                            LassoPhase::Snag => (LASSO_SNAG_TIME, LassoDrawPhase::Snag),
                            LassoPhase::Dragging => (LASSO_DRAG_TIME, LassoDrawPhase::Drag),
                            LassoPhase::Miss => (LASSO_MISS_TIME, LassoDrawPhase::Miss),
                            _ => (LASSO_THROW_TIME, LassoDrawPhase::Throw),
                        };
                        let phase_t = (1.0 - self.lasso_timer / dur).clamp(0.0, 1.0);
                        draw_lasso(
                            ctx,
                            canvas,
                            player_center,
                            tip,
                            draw_phase,
                            phase_t,
                            self.lasso_spin,
                        )?;
                    }
                }
                LassoPhase::Idle => {}
            }
        }

        // Screen-space HUD / overlay pass (minimap, tool roster, radar, stats, groove
        // meter, boss/upgrade overlays, flashlight). Extracted to game_render_hud.rs.
        self.draw_hud(ctx, canvas, width, height)?;

        return Ok(());
    }

    pub(crate) fn draw_scene(&mut self, ctx: &mut Context) -> GameResult {
        let width = self.width;
        let height = self.height;
        let mut canvas = Canvas::from_image(
            ctx,
            self.scene_image.clone(),
            Color::from_rgb(100, 200, 100),
        );
        let shake_ox = self.screen_shake_offset.x;
        let shake_oy = self.screen_shake_offset.y;
        // Zoom punch: shrink the visible world rect (magnify) around the player so they stay
        // pixel-locked while the world snaps in on a catch. z == 0 leaves the view untouched.
        let z = self.zoom_punch.clamp(0.0, 0.2);
        let focus = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
        // Camera scrolls across the larger-than-viewport world, following the player (clamped to
        // world bounds so no void edge shows). The zoom punch magnifies toward the focus point:
        // origin = camera + z·(focus − camera). At camera == 0 this collapses to the old focus·z.
        let cam = self.camera_origin;
        let vw = width * (1.0 - z);
        let vh = height * (1.0 - z);
        canvas.set_screen_coordinates(Rect::new(
            cam.x + z * (focus.x - cam.x) + shake_ox,
            cam.y + z * (focus.y - cam.y) + shake_oy,
            vw,
            vh,
        ));
        canvas.set_blend_mode(BlendMode::ALPHA);
        canvas.set_sampler(Sampler::nearest_clamp());

        if self.show_world_map {
            if let Some(map) = &self.world_map {
                for music in &self.sounds.action_music {
                    music.pause();
                }
                if self.sounds.outro_music.playing() {
                    self.sounds.outro_music.pause();
                }
                if self.sounds.intro_music.playing() {
                    self.sounds.intro_music.pause();
                }
                draw_world_map(ctx, &mut canvas, map, width, height, self.menu_time)?;
                canvas.finish(ctx)?;
                return Ok(());
            }
        }

        if self.show_instructions {
            if self.sounds.outro_music.playing() {
                self.sounds.outro_music.pause();
            }
            for music in &self.sounds.action_music {
                if music.playing() {
                    music.pause();
                }
            }
            let menu_music_ready = self.menu_intro_complete
                || self.menu_intro_time >= crate::menu_intro::MENU_REVEAL_AT;
            if menu_music_ready && !self.sounds.intro_music.playing() {
                self.sounds.intro_music.play();
            } else if !menu_music_ready && self.sounds.intro_music.playing() {
                self.sounds.intro_music.pause();
            }
            self.draw_instructions_screen(ctx, &mut canvas, width, height)?;
            canvas.finish(ctx)?;
            return Ok(());
        } else if self.game_over {
            for music in &self.sounds.action_music {
                music.pause();
            }
            if !self.sounds.outro_music.playing() {
                self.sounds.outro_music.play();
            }
            self.draw_game_over_screen(ctx, &mut canvas)?;
        } else {
            if self.sounds.intro_music.playing() {
                self.sounds.intro_music.pause();
            }
            if self.hitstop_timer <= 0.0 {
                let active_music = self.action_music_index();
                let music = &mut self.sounds.action_music[active_music];
                if music.stopped() {
                    music.play();
                } else if music.paused() {
                    music.resume();
                }
            }
            self.draw_game(ctx, &mut canvas, width, height)?;
            // Upgrade choice is a LIVE overlay now, not a world-freeze: the game above keeps
            // simulating and drawing (crabs, rivals and music all still moving), and the cards float
            // on top as a translucent layer so the player picks under real pressure. draw_game leaves
            // the canvas in screen space offset by the screen shake; reset to a clean viewport origin
            // so the cards — laid out in [0,width]x[0,height] — sit exactly where the click handler
            // and hover test expect them.
            if self.pending_upgrade {
                canvas.set_screen_coordinates(Rect::new(0.0, 0.0, width, height));
                self.draw_upgrade_screen(ctx, &mut canvas)?;
            }
        }
        canvas.finish(ctx)?;
        Ok(())
    }
}
