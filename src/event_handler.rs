//! ggez `EventHandler` trait implementation for `MainState`: the per-frame `update`/`draw`
//! callbacks and the keyboard/mouse input entry points (name entry, upgrade-card picks,
//! flashlight toggle, and the charge-and-release lasso throw).
//!
//! `update` delegates to `tick` (game_update.rs) and `draw` runs the three-pass render
//! (scene → conga trail → post-process). Extracted verbatim from `main.rs` to keep that
//! file focused on setup and `main()`. Pure structural move, no behaviour change.

use ggez::event::EventHandler;
use ggez::glam::Vec2;
use ggez::graphics::{BlendMode, Canvas, Color, DrawParam, Sampler};
use ggez::input::keyboard::{KeyCode, KeyInput};
use ggez::input::mouse::MouseButton;
use ggez::winit::keyboard::PhysicalKey;
use ggez::{Context, GameResult};

use crate::controls::handle_key_down_event;
use crate::*;

impl EventHandler for MainState {
    fn update(&mut self, ctx: &mut Context) -> GameResult {
        self.tick(ctx)
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        // Bot mode: skip all rendering to run at maximum speed — UNLESS we're recording a
        // gameplay clip (RUSTLER_RECORD set), in which case a bot drives real gameplay and we
        // want the scene on screen for a screen-recorder to capture.
        if self.bot.is_some() && std::env::var_os("RUSTLER_RECORD").is_none() {
            let canvas = Canvas::from_frame(ctx, ggez::graphics::Color::BLACK);
            canvas.finish(ctx)?;
            return Ok(());
        }

        // --- Pass 1: render the game scene to an offscreen crisp image ---
        self.draw_scene(ctx)?;

        // --- Pass 1.5: conga trail / echo-afterimage accumulation (ping-pong) ---
        // A single fixed extra full-screen pass: composite the crisp scene as an opaque base,
        // then additively lay the faded bright residue of the PREVIOUS frame on top. The trail
        // shader keeps only bright/additive elements (crab glows, beat rings, rope heat), so the
        // moving conga train leaves a comet of decaying light while the terrain stays clean.
        //
        // `trail_strength` folds the per-frame feedback decay together with a groove curve: it is
        // 0 below groove 0.2 (normal play renders identically to a plain scene blit) and smoothsteps
        // up to ~0.86 at max groove, so the delirium is earned and never obscures the rhythm read.
        let g = self.groove;
        let trail_strength = if g <= 0.2 {
            0.0
        } else {
            let t = ((g - 0.2) / 0.8).clamp(0.0, 1.0);
            (t * t * (3.0 - 2.0 * t)) * 0.86
        };
        // Below the groove threshold this pass is a documented no-op (pixel-exact copy of the
        // scene), so skip the whole offscreen render — canvas bind, blit draw, blend-mode
        // switches, finish()/GPU submit — and feed the scene straight into Pass 2 instead. This
        // is the common case (normal play sits under groove 0.2 most of the time), so avoiding a
        // full extra full-screen pass here is a real per-frame win. The trail ping-pong buffers
        // are simply left untouched; when groove climbs back past 0.2, trail_strength ramps up
        // from ~0 so any staleness in the buffers contributes a negligible first-frame blend.
        let write_img = if trail_strength > 0.0 {
            // Ping-pong: read last frame's accumulation, write this frame's. Both images are
            // allocated once (state.rs) and reused — no per-frame image allocation.
            let (read_img, write_img) = if self.trail_swap {
                (self.trail_image_a.clone(), self.trail_image_b.clone())
            } else {
                (self.trail_image_b.clone(), self.trail_image_a.clone())
            };
            let scene = self.scene_image.clone();
            let tu = TrailUniform {
                strength: trail_strength,
            };
            self.trail_params.set_uniforms(ctx, &tu);
            {
                let mut acc = Canvas::from_image(ctx, write_img.clone(), Color::BLACK);
                acc.set_sampler(Sampler::nearest_clamp());
                acc.draw(&scene, DrawParam::default().dest(Vec2::ZERO));
                acc.set_shader(&self.trail_shader);
                acc.set_shader_params(&self.trail_params);
                acc.set_blend_mode(BlendMode::ADD);
                acc.draw(&read_img, DrawParam::default().dest(Vec2::ZERO));
                acc.set_blend_mode(BlendMode::ALPHA);
                acc.set_default_shader();
                acc.finish(ctx)?;
            }
            self.trail_swap = !self.trail_swap;
            write_img
        } else {
            self.scene_image.clone()
        };

        // --- Pass 2: blit the accumulated scene to screen with post-processing ---
        {
            let (draw_w, draw_h) = ctx.gfx.drawable_size();
            let _scale_x = draw_w / self.width;
            let _scale_y = draw_h / self.height;
            // title_card_t: ease in fast, hold, ease out over the last 0.6s
            let title_t = if self.level_title_timer > 0.0 {
                let hold = 2.5_f32;
                let fade_in = (1.0 - (self.level_title_timer - hold) / 0.3).clamp(0.0, 1.0);
                let fade_out = (self.level_title_timer / 0.6).clamp(0.0, 1.0);
                fade_in.min(fade_out)
            } else {
                0.0
            };
            let uniform = PostProcessUniform {
                groove: self.groove,
                time: self.time_elapsed,
                screen_width: self.width,
                screen_height: self.height,
                title_card_t: title_t,
                menu_bloom: if self.show_instructions && self.menu_page == 0 {
                    crate::menu_intro::presentation(self.menu_intro_time).moon_bloom
                } else {
                    0.0
                },
                menu_moon_x: 0.82,
                menu_moon_y: 0.2,
            };
            // Reuse cached shader params, just update uniforms (avoids per-frame GPU buffer alloc)
            self.postprocess_params.set_uniforms(ctx, &uniform);
            let mut screen_canvas = Canvas::from_frame(ctx, Color::BLACK);
            screen_canvas.set_shader(&self.postprocess_shader);
            screen_canvas.set_shader_params(&self.postprocess_params);
            screen_canvas.draw(&write_img, DrawParam::default().dest(Vec2::ZERO));
            screen_canvas.set_default_shader();
            screen_canvas.finish(ctx)?;
        }

        Ok(())
    }

    fn key_down_event(&mut self, ctx: &mut Context, input: KeyInput, _repeat: bool) -> GameResult {
        // ggez 0.10 (winit 0.30) no longer exposes `KeyInput::keycode`; derive the physical
        // key code ourselves so the rest of the handling reads exactly as before.
        let keycode = match input.event.physical_key {
            PhysicalKey::Code(code) => Some(code),
            _ => None,
        };
        // Player-name text entry. ggez 0.10 removed the separate `text_input_event` callback and
        // delivers typed text on the key event itself (`input.event.text`). Handled first and
        // unconditionally (like 0.9's independent text callback) so a name character still lands
        // even when a later branch returns early for the same key.
        if self.show_instructions
            && !self.show_world_map
            && !self.game_over
            && !self.pending_upgrade
            && self.menu_page == 1
        {
            if let Some(text) = &input.event.text {
                for character in text.chars() {
                    if !character.is_control() && self.player_name.chars().count() < 24 {
                        self.push_player_name_char(character);
                    }
                }
            }
        }
        if self.pending_upgrade {
            // The choice is a live overlay now, not a freeze: 1/2/3 pick a card, but every other key
            // falls through to normal in-game handling so the player can keep steering and using
            // tools while they decide (and a rival can steal from them mid-decision — the intended
            // pressure to pick fast). 1/2/3 aren't bound to anything in-game (they're loadout-screen
            // only), so consuming them here can't shadow a gameplay action.
            if let Some(key) = keycode {
                match key {
                    KeyCode::Digit1 => {
                        self.apply_upgrade(1);
                        return Ok(());
                    }
                    KeyCode::Digit2 => {
                        self.apply_upgrade(2);
                        return Ok(());
                    }
                    KeyCode::Digit3 => {
                        self.apply_upgrade(3);
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
        if let Some(key) = keycode {
            if key == KeyCode::KeyF {
                self.flashlight.on = !self.flashlight.on;
                use ggez::audio::SoundSource;
                // Slightly higher pitch on, lower on off, so the toggle direction is audible.
                let pitch = if self.flashlight.on { 1.15 } else { 0.85 };
                self.sounds.flashlight_toggle.set_pitch(pitch);
                let _ = self.sounds.flashlight_toggle.play();
                return Ok(());
            }
        }
        if handle_key_down_event(self, ctx, keycode) {
            return Ok(());
        }
        Ok(())
    }

    fn mouse_motion_event(
        &mut self,
        ctx: &mut Context,
        x: f32,
        y: f32,
        _xrel: f32,
        _yrel: f32,
    ) -> GameResult {
        let window_size = ctx.gfx.window().inner_size();
        let scale_x = window_size.width as f32 / self.width;
        let scale_y = window_size.height as f32 / self.height;
        // mouse_pos is used against player/crab positions (world space) for flashlight aim and crab
        // picking, so store it in world space: screen point offset by the camera origin.
        self.mouse_pos = self.camera_origin + Vec2::new(x / scale_x, y / scale_y);
        Ok(())
    }

    fn mouse_button_down_event(
        &mut self,
        ctx: &mut Context,
        button: MouseButton,
        x: f32,
        y: f32,
    ) -> GameResult {
        // Upgrade screen: let the player click a card as an alternative to the number keys.
        if self.pending_upgrade {
            if button == MouseButton::Left {
                let window_size = ctx.gfx.window().inner_size();
                let scale_x = window_size.width as f32 / self.width;
                let scale_y = window_size.height as f32 / self.height;
                let p = Vec2::new(x / scale_x, y / scale_y);
                let rects = self.upgrade_card_rects();
                for (i, r) in rects.iter().enumerate() {
                    if p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h {
                        self.apply_upgrade(i as u8 + 1);
                        break;
                    }
                }
            }
            return Ok(());
        }
        if self.game_over || self.show_instructions {
            return Ok(());
        }
        // Left click: BEGIN winding up the lasso. The throw fires on mouse_button_up.
        if button == MouseButton::Left && self.lasso_phase == LassoPhase::Idle {
            self.lasso_mouse_down = true;
            self.lasso_charge = 0.0;
            self.lasso_spin = 0.0;
            self.lasso_phase = LassoPhase::Winding;
            // Capture player center for the windup origin; target is updated every frame from mouse_pos.
            self.lasso_origin = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
        }
        Ok(())
    }

    fn mouse_button_up_event(
        &mut self,
        _ctx: &mut Context,
        button: MouseButton,
        _x: f32,
        _y: f32,
    ) -> GameResult {
        if button == MouseButton::Left && self.lasso_phase == LassoPhase::Winding {
            self.lasso_mouse_down = false;
            {
                use ggez::audio::SoundSource;
                let _ = self.sounds.lasso_sfx.play();
            }
            // Compute scaled range from charge: tap = MIN_RANGE_FRAC × MAX_RANGE, full = MAX_RANGE.
            let charge_frac = (self.lasso_charge / LASSO_MAX_CHARGE_TIME).min(1.0);
            let range_frac = LASSO_MIN_RANGE_FRAC + (1.0 - LASSO_MIN_RANGE_FRAC) * charge_frac;
            // On-beat release bonus: extra reach + groove reward. Uses the wider ranged-cast window
            // (#164) so a slightly-early/late release still reads on-beat — the lasso is a
            // cooldown-gated throw, not the tight dash.
            let on_beat_bonus = if self.on_beat_action() {
                let center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
                self.reward_on_beat_action(center, "LASSO");
                LASSO_ONBEAT_BONUS
            } else {
                1.0
            };
            self.lasso_on_beat_bonus = on_beat_bonus;
            let throw_range = LASSO_MAX_RANGE * range_frac * on_beat_bonus;
            // Clamp target within throw_range of player center.
            let origin = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            // Auto-aim: snap the throw toward the nearest catchable crab in reach so a well-timed
            // release lands a catch without fiddly aiming. Charge/recharge/on-beat release are
            // unchanged — only WHERE the loop flies is assisted. Empty field falls back to manual aim.
            let aim_point = self.lasso_aim_point(origin, throw_range);
            let to_aim = aim_point - origin;
            let aim_dist = to_aim.length();
            let clamped_target = if aim_dist > throw_range {
                origin + to_aim / aim_dist * throw_range
            } else if aim_dist > 1.0 {
                aim_point
            } else {
                // Mouse right on player — throw in the last-faced direction.
                origin + self.last_dir.normalize_or_zero() * throw_range
            };
            self.lasso_target = clamped_target;
            self.lasso_origin = origin;
            // Throw speed also scales with charge: a full charge is faster than a tap.
            // We achieve this by scaling LASSO_THROW_TIME inversely with range_frac.
            let throw_time = LASSO_THROW_TIME / range_frac.max(0.15);
            self.lasso_timer = throw_time;
            self.lasso_phase = LassoPhase::Throwing;
            self.lasso_pos = Some(origin);
            self.lasso_charge = 0.0;
        }
        Ok(())
    }
}
