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
use crate::enemies::CrabType;
use crate::hud_cache::*;
use crate::state::*;
use crate::{how_to_play_body_text, menu, panic_snap_links};
use crate::graphics::{
    LassoDrawPhase, draw_ambient_motes, draw_beat_hit_punch, draw_beat_wave_ring,
    draw_boss_fissures, draw_call_ring, draw_catch_bloom_ring, draw_catch_shockwaves, draw_catch_trails,
    draw_beat_keeper_ring, draw_chain_rings, draw_cleave_slash, draw_cleave_stakes, draw_combo_meter, draw_conga_rope, draw_deliver_beam, draw_delivery_pen,
    draw_delivery_streak, draw_downbeat_pulse_ring, draw_fear_rings,
    draw_floating_texts, draw_groove_call_ring, draw_haul_worth, draw_kelp_snag_warning, draw_lasso,
    draw_lasso_windup, draw_particles, draw_pen_guide, draw_penned_marchers,
    draw_puddle_ripples, draw_rustler, draw_sky_overlay, draw_slam_ring,
    draw_speed_lines, draw_sprint_whoosh, draw_stomp_ring, draw_tail_run_badge, draw_tide_pools, draw_tide_pulses, draw_train_at_risk, draw_whistle_ring, draw_world_edge, draw_world_map, draw_world_zones, unit_circle, unit_line, unit_square,
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
                if intro.menu_flash > 0.0 {
                    canvas.draw(
                        unit_square(ctx)?,
                        DrawParam::default()
                            .scale(Vec2::new(width, height))
                            .color(Color::new(0.9, 0.95, 1.0, intro.menu_flash * 0.45)),
                    );
                }
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
        // The world (playfield) is larger than the viewport (width/height). Ground-layer fills
        // (grass, biome pulse, ambient motes) must cover the whole world so no void shows past the
        // viewport edge as the camera scrolls; the HUD later switches back to viewport dims.
        let world_w = self.world_width;
        let world_h = self.world_height;

        // Select texture for current level.
        let _texture = match self.level_textures[self.current_level] {
            LevelTexture::Grass => &self.textures.grass,
            LevelTexture::Sand => &self.textures.sand,
        };

        // Biome for the current zone (clamped so a finished run doesn't index past the end).
        let biome = self.levels[self.current_level.min(self.levels.len() - 1)].biome;
        let (tr, tg, tb) = biome.tint;

        // Fold the day/night grade into the ground tint so the whole world shifts together with the
        // sky overlay below — dawn amber → midday bright → dusk orange-pink → night deep blue. A
        // lightning flash briefly floods the ground bright white to match the sky flash.
        let (dr, dg, db) = self.day_tint();
        let flash = self.lightning_flash.clamp(0.0, 1.0);
        let _ground_r = ((tr as f32 * dr) + 255.0 * flash * 0.25).min(255.0) as u8;
        let _ground_g = ((tg as f32 * dg) + 255.0 * flash * 0.25).min(255.0) as u8;
        let _ground_b = ((tb as f32 * db) + 255.0 * flash * 0.25).min(255.0) as u8;

        // Draw world zones: grass (left), beach (middle), water (right)
        draw_world_zones(
            ctx,
            canvas,
            world_w,
            world_h,
            self.time_elapsed,
            biome.layout,
        )?;

        // World-space sky overlay: a soft full-world tint carrying the day/night mood plus the
        // cloudy/rain grey dimming. Sits over the ground but under the action. Rain streaks, the
        // edge vignette and the lightning full-screen flash draw later in SCREEN space so they're
        // camera-independent.
        draw_sky_overlay(
            ctx,
            canvas,
            world_w,
            world_h,
            self.day_phase_t,
            self.weather_intensity,
        )?;

        // World-edge boundary: a soft darkening that fades inward from the true playfield limits,
        // so scrolling to the edge of the larger-than-viewport world reads as arriving at a shore
        // rather than an abrupt camera clamp. World space, tinted to the biome accent, under the
        // action. Only visible when the camera actually reaches an edge.
        {
            let (er, eg, eb) = biome.pulse;
            draw_world_edge(
                ctx,
                canvas,
                world_w,
                world_h,
                Color::from_rgb(er, eg, eb),
                self.night_factor(),
            )?;
        }

        // Subtle beat pulse: an on-beat flash tinted to match the current biome's mood. At night the
        // pulse glows brighter — the beat is the one thing that reads MORE in the dark, trading the
        // dimmed base visibility for a stronger rhythmic cue.
        if self.beat_intensity > 0.0 {
            let night_glow = 1.0 + self.night_factor() * 0.6;
            let pulse_alpha = (self.beat_intensity * 28.0 * night_glow).min(255.0) as u8;
            let (pr, pg, pb) = biome.pulse;
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .scale(Vec2::new(world_w, world_h))
                    .color(Color::from_rgba(pr, pg, pb, pulse_alpha)),
            );
        }

        // Ambient atmosphere: a field of slow-drifting motes over the ground (sea spray / drifting
        // spores) that give the space between the action depth and life, tinted to the biome's accent
        // and bobbing gently on the beat. Stateless and cheap (one batched draw), sits above the
        // ground flash but under the tide pools and all the action.
        {
            let (ar, ag, ab) = biome.pulse;
            draw_ambient_motes(
                ctx,
                canvas,
                world_w,
                world_h,
                self.time_elapsed,
                self.beat_intensity,
                Color::from_rgb(ar, ag, ab),
            )?;
        }

        // Rain puddle ripples on the ground (world space, under the action) — expanding rings that
        // pop where rain "lands", scaled up with weather intensity. Only visible once it's actually
        // raining; sits over the ambient motes but under the tide pools/crabs. Camera origin lets it
        // cover just the visible viewport slice of the world so it isn't wasted off-screen.
        if self.weather_intensity > 0.35 {
            draw_puddle_ripples(
                ctx,
                canvas,
                self.camera_origin,
                width,
                height,
                self.time_elapsed,
                self.weather_intensity,
            )?;
        }

        // Desktop biome: paint a flat, opaque OS wallpaper over the whole world so the beach texture
        // (grass/sand, zones, motes) reads as a plain neutral screen — the fourth-wall stage on which
        // the window panels sit. Drawn here, after the ground/atmosphere but before the terrain
        // patches, so the windows and all the action land on top of it.
        //
        // TODO(ggez-0.10): this opaque fill is the transparency seam. Once the game window itself can
        // go transparent (ggez 0.10), delete this fill so the player's REAL desktop shows through the
        // frame here instead of a painted wallpaper — the drawn window panels (graphics::terrain)
        // then become handles over the actual OS windows behind the transparent game window.
        if biome.terrain == crate::levels::TerrainKind::Desktop {
            // Use the raw biome tint (not the day/night-graded ground color) so the wallpaper stays
            // constant like a real screen, indifferent to the beach's dawn/dusk cycle.
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .scale(Vec2::new(world_w, world_h))
                    .color(Color::from_rgb(tr, tg, tb)),
            );
        }

        // Tide pools — terrain hazards on the ground layer, under the crabs/rope, so the train
        // visibly wades through the water it's being routed around. When a Tide Boss has flooded the
        // arena, the last `boss_flood_pools` entries are its surge water: they always read as water
        // regardless of the biome's native terrain skin (rock/kelp/open), so we draw the biome's own
        // pools with the biome terrain, then the flood slice explicitly as water on top.
        let native_pool_count = self.tide_pools.len().saturating_sub(self.boss_flood_pools);
        // Only the Water biome's native pools carry the Tide Pool current, so only they draw the flow
        // streaks — matching exactly where the sim applies the drift (see update_crabs).
        let native_has_current = biome.terrain == crate::levels::TerrainKind::Water;
        draw_tide_pools(
            ctx,
            canvas,
            &self.tide_pools[..native_pool_count],
            self.time_elapsed,
            self.beat_intensity,
            self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0),
            biome.terrain,
            native_has_current,
            self.rock_tide_fill,
        )?;
        if self.boss_flood_pools > 0 {
            draw_tide_pools(
                ctx,
                canvas,
                &self.tide_pools[native_pool_count..],
                self.time_elapsed,
                self.beat_intensity,
                self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0),
                crate::levels::TerrainKind::Water,
                // Flood pools render as water but carry no current — never streak them.
                false,
                // Flood pools draw as water, not rock, so the tide level is irrelevant here.
                0.0,
            )?;
        }

        // King Crab enrage set-piece: the cracked-floor fissures the boss split the arena into.
        // Drawn over the water so they read as hot hazards welling up through the ground.
        draw_boss_fissures(
            ctx,
            canvas,
            &self.boss_fissures,
            self.time_elapsed,
            self.beat_intensity,
            self.boss_fissure_erupt,
        )?;

        // Rare pirate treasure: a simple high-contrast chest and lid shine, pulsing on the beat so
        // its timing payoff is readable before the player reaches it.
        if let Some(pos) = self.treasure_chest {
            let beat_pulse = 1.0 + self.beat_intensity * 0.12;
            let size = 34.0 * beat_pulse;
            let top_left = pos - Vec2::splat(size * 0.5);
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .dest(top_left)
                    .scale(Vec2::splat(size))
                    .color(Color::from_rgb(112, 57, 20)),
            );
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .dest(top_left + Vec2::new(0.0, size * 0.18))
                    .scale(Vec2::new(size, size * 0.16))
                    .color(Color::from_rgb(255, 196, 48)),
            );
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .dest(pos - Vec2::new(size * 0.1, size * 0.5))
                    .scale(Vec2::new(size * 0.2, size))
                    .color(Color::from_rgb(255, 218, 85)),
            );
        }

        // Delivery pen — drawn on the ground layer under the crabs/rope so the train visibly rolls
        // into it. Lights up green once there's a train to bank (chain_count > 0). The "haul"
        // anticipation (0..1) scales the pen's excitement to the size of the incoming jackpot and
        // ramps up further as the loaded train closes in, so the biggest payoff moment in the game
        // — driving a fat conga line into the pen — builds visible tension *before* the bank.
        let haul = if self.chain_count > 0 {
            // Train size normalized against a "big haul" reference (~24 crabs reads as a jackpot),
            // then boosted as the player carries it into the pen's neighborhood so the pen strains
            // toward an approaching train rather than only reacting to its length.
            let size_term = (self.chain_count as f32 / 24.0).min(1.0);
            let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            let dist = player_center.distance(self.pen_pos);
            // 0 far away, ramps to 1 as the train enters ~2.5 pen-radii of the goal.
            let approach = (1.0 - (dist / (PEN_RADIUS * 2.5)).min(1.0)).max(0.0);
            (size_term * (0.55 + 0.45 * approach)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        // Live "what would this train bank right now" preview shown floating over the pen. Mirrors
        // the base delivery payout in try_deliver_train — the super-linear triangular sum times the
        // current combo + Groove-Gamble multipliers — but deliberately EXCLUDES the on-beat PERFECT
        // and delivery-streak bonuses, since those are only earned at the moment you actually bank on
        // beat. So it reads as the honest floor ("at least this much"), and timing the bank well pays
        // even more, keeping the on-beat delivery worth engaging rather than spoiling it.
        //
        // Precompute bonds for the full chain once — count_chain_bonds walks all crabs every call, and
        // it used to be called twice with the same argument (pen_worth preview + at-risk readout).
        // Cache it here so the second call is a free integer read instead of another O(n) scan.
        // Single scan for both bonds and sandwiches — avoids two separate O(n) walks over the
        // caught crabs for what is effectively the same chain_index→type lookup.
        let (bonds_n, sandwiches_n, run_bonus_n, centerpiece_n) = if self.chain_count > 0 {
            self.count_bonds_and_sandwiches(self.chain_count)
        } else {
            (0, 0, 0, 0)
        };
        let pen_worth = if self.chain_count > 0 {
            let n = self.chain_count;
            // Include the same arrangement (same-type adjacent pair) AND sandwich bonuses
            // try_deliver_train pays, so the live preview stays honest — holding a well-arranged
            // train visibly raises the pen worth, which is the whole point of making the middle of
            // the train matter.
            let base = (n * (n + 1) / 2) * 3
                + bonds_n * BOND_PAIR_BONUS
                + sandwiches_n * SANDWICH_BONUS
                + run_bonus_n
                + centerpiece_n;
            Some(
                (base as f32 * self.combo_multiplier() as f32 * self.beat_gamble_mult).round()
                    as usize,
            )
        } else {
            None
        };
        // Live HAUL readout floating over the PLAYER (where their eyes are) — the positive twin of the
        // red AT RISK tag on the tail. pen_worth already shows the total over the pen, but the
        // arrangement value is baked invisibly into it: the player can't tell how much of their haul is
        // raw length vs. the bonds/sandwiches/runs they deliberately arranged. Surfacing the arrangement
        // slice explicitly ("ARRANGED +N") is the agency/control the arrangement system was missing —
        // it lets the player SEE arranging pay off in the moment and steer to complete more of it,
        // instead of only discovering it at the pen. Shown from a 2-crab train up (arrangement can pay
        // before the snap-risk threshold), and only carrying the readout past the pen guide's near zone
        // so the two don't stack on top of each other.
        if let Some(worth) = pen_worth {
            if self.chain_count >= 2 {
                let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
                if player_center.distance(self.pen_pos) > PEN_RADIUS * 1.4 {
                    // The arrangement-only slice of the haul, run through the same live multipliers the
                    // total uses so the two numbers stay honest with each other.
                    let arr_base = bonds_n * BOND_PAIR_BONUS
                        + sandwiches_n * SANDWICH_BONUS
                        + run_bonus_n
                        + centerpiece_n;
                    let arranged =
                        (arr_base as f32 * self.combo_multiplier() as f32 * self.beat_gamble_mult)
                            .round() as usize;
                    draw_haul_worth(
                        ctx,
                        canvas,
                        player_center,
                        self.time_elapsed,
                        self.beat_intensity,
                        worth,
                        arranged,
                    )?;
                }
            }
        }

        // Delivery beam — a lit tether from where the train's head departed to the pen it cashed
        // into, drawn under the pen's own bloom while the deliver flash decays. Connects the two
        // ends of the bank so the payoff reads as the conga line rushing home, not just the pen
        // popping in place. Uses the snapshotted pen the bank actually paid into (the live pen has
        // relocated by now).
        if self.deliver_flash > 0.0 {
            draw_deliver_beam(
                ctx,
                canvas,
                self.deliver_beam_from,
                self.deliver_beam_to,
                self.deliver_flash,
                self.deliver_beam_perfect,
            )?;
        }
        draw_delivery_pen(
            ctx,
            canvas,
            self.pen_pos,
            PEN_RADIUS,
            self.time_elapsed,
            self.beat_intensity,
            self.chain_count > 0,
            haul,
            pen_worth,
            self.deliver_flash,
        )?;

        // Live "at-risk" readout — the downside half of the bank-now-vs-push-luck decision, mirroring
        // the gold pen-worth tag but for what a snap would cost you RIGHT NOW. A panic snap strips the
        // last panic_snap_links(n) tail links (snap_chain_on_panic), and because pen_worth is triangular
        // the tail links are the priciest ones, so the honest number is the MARGINAL loss pen_worth(n) -
        // pen_worth(keep), computed with the same combo/gamble multipliers so the two tags agree. That
        // link count now GROWS with train length, so a long unbanked train's at-risk number jumps at
        // each severity tier — the downside visibly mounts the longer you hold. Gated
        // to the same length threshold (MIN_TRAIN_TO_SNAP=5) at which a panic snap can actually fire —
        // below that there's genuinely no risk, so no tag; it appears exactly when holding turns
        // dangerous and the number climbs the longer you refuse to bank. Anchored on the tail in warning
        // red so it contrasts with the gold reward tag over the pen instead of blurring into it.
        if self.chain_count >= 5 {
            if let Some(tail_pos) = self.cached_tail_pos {
                let mult = self.combo_multiplier() as f32 * self.beat_gamble_mult;
                let tri = |m: usize| (m * (m + 1) / 2) * 3;
                let n = self.chain_count;
                // Use the SAME severity function the panic snap uses, so the readout can't lie:
                // a longer train shows a bigger at-risk number precisely because a snap tears more
                // (and pricier, since tri() is triangular) tail links off it.
                let keep = n.saturating_sub(panic_snap_links(n)).max(1);
                // Marginal loss folds in the arrangement bonus too: a snap tears off tail links,
                // which destroys every same-type bond in the torn region (and the one straddling the
                // cut), so the pricier a train's tail arrangement, the more a snap costs — mirroring
                // how the bank pays those same bonds. bonds(n) - bonds(keep) is what the cut erases.
                // bonds_n (bonds for the full chain) was precomputed above — reuse it instead of a
                // second O(n) crab scan with the same argument.
                let (bonds_keep, sandwiches_keep, run_bonus_keep, centerpiece_keep) =
                    self.count_bonds_and_sandwiches(keep);
                let bonds_lost = bonds_n.saturating_sub(bonds_keep);
                // Sandwiches destroyed by the cut too — any sandwich straddling or inside the torn
                // tail region is gone, so the at-risk number folds in its lost value the same way
                // the bank pays it. Mirrors bonds_lost exactly.
                let sandwiches_lost = sandwiches_n.saturating_sub(sandwiches_keep);
                // Deep-run block value lost to the cut, same logic: chopping the tail shortens (or
                // erases) any long matched run there, so the at-risk number reflects the run bonus
                // the bank would no longer pay — keeping the two tags honest with each other.
                let run_bonus_lost = run_bonus_n.saturating_sub(run_bonus_keep);
                // Centerpiece bonus lost to the cut: a straddling deep run in the full train may no
                // longer straddle the shortened train's new midpoint (or gets chopped below length 3),
                // so the at-risk number folds in the centerpiece the bank would no longer pay.
                let centerpiece_lost = centerpiece_n.saturating_sub(centerpiece_keep);
                let marginal = tri(n).saturating_sub(tri(keep))
                    + bonds_lost * BOND_PAIR_BONUS
                    + sandwiches_lost * SANDWICH_BONUS
                    + run_bonus_lost
                    + centerpiece_lost;
                let at_risk = (marginal as f32 * mult).round() as usize;
                // Danger ramps from the snap threshold up to a long train (~12), so color/pulse escalate.
                let danger01 = ((n.saturating_sub(5)) as f32 / 7.0).clamp(0.0, 1.0);
                draw_train_at_risk(ctx, canvas, tail_pos, self.time_elapsed, at_risk, danger01)?;
            }
        }

        // Kelp-snag telegraph: while the tail sits in the weeds and is long enough to snag, ring the
        // tail crab with a rising green warning so an imminent snag is seen coming and the player can
        // route out. Legibility only — the odds live in `snag_chain_on_kelp`.
        if self.kelp_snag_warn > 0.02 {
            if let Some(tail_pos) = self.cached_tail_pos {
                draw_kelp_snag_warning(
                    ctx,
                    canvas,
                    tail_pos,
                    self.time_elapsed,
                    self.kelp_snag_warn,
                )?;
            }
        }

        // Delivery-streak heat badge — the persistent, watchable face of the streak multiplier that
        // otherwise only flashed for a frame at bank time and then decayed silently. Shows the live
        // multiplier under the pen and pulses toward an alarm color as the grace window runs down, so
        // "bank again before you drop a notch" is a visible tension. Gated to streak >= 2 (streak 1 is
        // 1.0x — nothing at stake). Kept SEPARATE from pen_worth on purpose: pen_worth is the honest
        // floor excluding streak/on-beat bonuses, and folding the streak in would spoil that read.
        if self.deliver_streak >= 2 {
            let streak_mult = 1.0 + (self.deliver_streak.saturating_sub(1) as f32) * 0.25;
            let decay01 = (self.deliver_streak_timer / DELIVER_STREAK_GRACE).clamp(0.0, 1.0);
            draw_delivery_streak(
                ctx,
                canvas,
                self.pen_pos,
                PEN_RADIUS,
                self.time_elapsed,
                streak_mult,
                decay01,
            )?;
        }

        // Cleave stakes tag — the Splitter bet made legible BEFORE the catch. While a free Splitter is
        // loose and the player has a train worth cleaving, float a live "CLEAVE ~N" figure at the split
        // point (the midpoint where the cut lands) showing what a clean on-beat cut would bank, heating
        // gold in the beat window like the splitter aura. Reuses cleave_clean_worth so the previewed
        // number can't drift from the actual payout. Only shows when there's both a free Splitter and a
        // train (≥2 links) to meaningfully halve, so it's naturally transient and never HUD clutter.
        if self.chain_count >= 2 && self.free_splitter_present {
            let (keep, banked) = self.cleave_split_point();
            // Single O(n) scan: find both split-point positions and tally the back-half
            // composition (Goldens/Magnets) for the jackpot check — avoids three separate
            // O(n) passes (two .find() + cleave_clean_worth's .fold()) that this block used
            // to issue every frame whenever a free Splitter and a live train are both present.
            let front_idx = keep.saturating_sub(1);
            let mut front: Option<Vec2> = None;
            let mut back: Option<Vec2> = None;
            let mut golden_in_slice = 0usize;
            let mut magnet_in_slice = 0usize;
            for c in &self.crabs {
                if !c.caught {
                    continue;
                }
                if let Some(ci) = c.chain_index {
                    if ci == front_idx {
                        front = Some(c.pos);
                    }
                    if ci == keep {
                        back = Some(c.pos);
                    }
                    if ci >= keep {
                        if c.is_golden() {
                            golden_in_slice += 1;
                        }
                        if c.is_magnet() {
                            magnet_in_slice += 1;
                        }
                    }
                }
            }
            let combo = self.combo_multiplier();
            let base = (banked * (banked + 1) / 2) * 3;
            let cashed_run = if self.tail_run_len >= 3 {
                self.tail_run_len
            } else {
                0
            };
            let golden_bonus = golden_in_slice * 120 * combo;
            let magnet_bonus = if magnet_in_slice > 0 {
                magnet_in_slice * banked.max(1) * 6 * combo
            } else {
                0
            };
            let run_bonus = (cashed_run as usize) * (cashed_run as usize) * 5 * combo;
            let crossover = golden_bonus + magnet_bonus + run_bonus;
            let worth =
                (base as f32 * combo as f32 * self.beat_gamble_mult).round() as usize + crossover;
            let jackpot = crossover > 0;
            if let Some(split_pt) = match (front, back) {
                (Some(f), Some(b)) => Some((f + b) * 0.5),
                (Some(f), None) => Some(f),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            } {
                if worth > 0 {
                    // Same beat-proximity curve the splitter aura uses, so the tag and the aura go hot
                    // together in the clean-cut window.
                    let to_beat = self.beat_timer.min(self.beat_interval - self.beat_timer);
                    let beat_prox = (1.0 - to_beat / (BEAT_WINDOW * 1.5)).clamp(0.0, 1.0);
                    draw_cleave_stakes(
                        ctx,
                        canvas,
                        split_pt,
                        worth,
                        jackpot,
                        beat_prox,
                        self.time_elapsed,
                    )?;
                }
            }
        }

        // Tail-run badge — the persistent readout of the same-type match run at the tail. Shows
        // "RUN xN" plus a 4-pip meter over the tail link so the player can *set up* the every-4th
        // Match-Run Milestone instead of only seeing the count flash for a frame at catch time.
        // Only shown for a run worth committing to (>=2) — a lone link isn't a run yet.
        if self.tail_run_len >= 2 {
            let tail_idx = self.chain_count.saturating_sub(1);
            if let Some(tail) = self
                .crabs
                .iter()
                .find(|c| c.caught && c.chain_index == Some(tail_idx))
            {
                let to_beat = self.beat_timer.min(self.beat_interval - self.beat_timer);
                let beat_prox = (1.0 - to_beat / (BEAT_WINDOW * 1.5)).clamp(0.0, 1.0);
                draw_tail_run_badge(
                    ctx,
                    canvas,
                    tail.pos,
                    self.tail_run_len,
                    tail.crab_color(),
                    beat_prox,
                    self.time_elapsed,
                )?;
            }
        }

        // Just-banked crabs marching into the pen — drawn over the pen ground so the parade files
        // in on top of the corral. Empty and free when no bank just happened.
        draw_penned_marchers(ctx, canvas, &self.penned_marchers, self.time_elapsed)?;

        // Draw beat ghost rings under the rope and crabs
        draw_chain_rings(ctx, canvas, &self.chain_rings)?;
        // Collect chain crab (chain_index, pos) pairs sorted by chain index into a persisted
        // scratch buffer instead of a fresh Vec<&EnemyCrab> every frame (see CHAIN_SORT_BUF).
        CHAIN_SORT_BUF.with(|buf| -> GameResult {
            let mut chain_links = buf.borrow_mut();
            chain_links.clear();
            // First collect (index, pos, type, color) so we can, after sorting by index, tag each
            // link with a same-type bond color relative to the link ahead of it. The type is dropped
            // once the bond is computed — only (index, pos, bond_color) travels on to the rope draw.
            //
            // Optimization: the sorted order and bond colors are stable as long as chain_count
            // doesn't change — only catches/releases mutate the chain structure. On the common case
            // (no catch/release this frame) we skip the O(n log n) sort and O(n) bond-color scan
            // and instead do a single O(n) pass to read current crab positions by stored index.
            CHAIN_ORDER_CACHE.with(|ocache| {
                let mut order_cache = ocache.borrow_mut();
                let chain_count = self.chain_count;
                let needs_rebuild = order_cache
                    .as_ref()
                    .map_or(true, |(cc, _)| *cc != chain_count);
                if needs_rebuild {
                    // (Re)build the sorted order and bond colors — only on catch/release events.
                    CHAIN_TYPE_BUF.with(|tbuf| {
                        let mut typed = tbuf.borrow_mut();
                        typed.clear();
                        typed.extend(
                            self.crabs
                                .iter()
                                .enumerate()
                                .filter(|(_, c)| c.caught && c.chain_index.is_some())
                                .map(|(i, c)| {
                                    (c.chain_index.unwrap_or(0), i, c.crab_type, c.crab_color())
                                }),
                        );
                        typed.sort_unstable_by_key(|&(idx, ..)| idx);
                        let mut prev_type: Option<CrabType> = None;
                        // Reuse the Vec already stored in order_cache (if any) to avoid a heap
                        // allocation on every catch/release event. Taking the stored Vec out,
                        // clearing it, and pushing into it preserves its capacity across rebuilds
                        // instead of dropping it and collecting a fresh one each time.
                        let mut sorted = order_cache.take().map(|(_, v)| v).unwrap_or_default();
                        sorted.clear();
                        sorted.extend(typed.iter().enumerate().map(|(pos, &(_, ci, ty, col))| {
                            // Same-type adjacency bond (unchanged): the link inherits the shared
                            // type color so its rope segment glows.
                            let mut bond = if prev_type == Some(ty) {
                                Some(col)
                            } else {
                                None
                            };
                            // Sandwich highlight: if THIS crab is flanked in the sorted chain by two
                            // of the same figurehead archetype (Golden/Dancer), light its rope
                            // segment with the flanking figurehead's color so the arrangement reads
                            // live on the train, not only as a bank callout. `typed` is sorted by
                            // chain_index, so pos-1 / pos+1 are the true chain neighbors.
                            if pos > 0 && pos + 1 < typed.len() {
                                let (_, _, lty, lcol) = typed[pos - 1];
                                let (_, _, rty, _) = typed[pos + 1];
                                if lty == rty && matches!(lty, CrabType::Golden | CrabType::Dancer)
                                {
                                    bond = Some(lcol);
                                }
                            }
                            prev_type = Some(ty);
                            (ci, bond)
                        }));
                        *order_cache = Some((chain_count, sorted));
                    });
                }
                // Fast path: read current positions from self.crabs using the cached order.
                if let Some((_, ref order)) = *order_cache {
                    for &(crabs_idx, bond) in order {
                        let crab = &self.crabs[crabs_idx];
                        chain_links.push((crab.chain_index.unwrap_or(0), crab.pos, bond));
                    }
                }
            });
            // Only the at-risk gain (live multiplier above the banked-safe floor) heats the rope,
            // so cashing out with B visibly cools it — the risk you're carrying reads on the train.
            let gamble_heat =
                ((self.beat_gamble_mult - self.beat_gamble_locked) / 2.0).clamp(0.0, 1.0);
            // Phase across the current bar (0 at the downbeat, →1 across four beats): drives the
            // pulse of light that sweeps down the rope once per bar so the train "feels the beat".
            let within_beat = 1.0 - (self.beat_timer / self.beat_interval).clamp(0.0, 1.0);
            let bar_phase = ((self.beat_count % 4) as f32 + within_beat) / 4.0;
            // Rival-splice heat: reuse the SAME committed-hunt / armed-steal state that already drives
            // the DEFEND ring and early-warning threat dots (npc hunt_intent / steal_threat). An armed
            // steal is peak danger; otherwise the smoothed hunt commitment. Take the worst rival so the
            // rope reddens exactly when a rival is threading your back half — no new risk logic.
            let mut splice_risk = 0.0f32;
            for npc in &self.npc_trains {
                let threat = if npc.steal_threat > 0.0 {
                    1.0
                } else {
                    npc.hunt_intent
                };
                if threat > splice_risk {
                    splice_risk = threat;
                }
            }
            // The splice aims ~2/3 down the chain (cached_steal_target_pos); on a short chain it falls
            // back to the tail. Match that so the heat band centers on the link actually threatened.
            let splice_center_frac = if self.chain_count >= 4 { 2.0 / 3.0 } else { 1.0 };
            draw_conga_rope(
                ctx,
                canvas,
                self.player_pos,
                &chain_links,
                self.time_elapsed,
                self.beat_intensity,
                gamble_heat,
                bar_phase,
                splice_risk,
                splice_center_frac,
            )
        })?;

        // On-beat catch-bloom ring around the head: shows the scoop window breathing with the bar
        // (widest on the downbeat) so timing a plain grab to the beat becomes a legible, watchable
        // read. Only meaningful once there's a train to catch onto, so gate on chain_count (the
        // cached caught-crab count) instead of scanning the whole herd every draw frame.
        if self.chain_count > 0 {
            let head = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            let catch_radius = self.catch_radius();
            draw_catch_bloom_ring(
                ctx,
                canvas,
                head,
                catch_radius,
                self.beat_catch_bloom,
                self.beat_intensity,
            )?;
        }

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
                // The world camera above belongs to the last played map. Campaign and title screens
                // are screen-space UI, so they must not inherit its world-space viewport after Escape.
                canvas.set_screen_coordinates(Rect::new(0.0, 0.0, width, height));
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
            // A run can return here while its scrolling camera is offset from the world origin.
            // Restore the UI viewport before drawing the main menu so it remains screen-centered.
            canvas.set_screen_coordinates(Rect::new(0.0, 0.0, width, height));
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
