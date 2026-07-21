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
use crate::spawnings::SpawnPattern;
use crate::state::*;
use crate::{how_to_play_body_text, menu, panic_snap_links};
use crate::graphics::{
    LassoDrawPhase, cached_stroke_rect, draw_ambient_motes, draw_beat_hit_punch, draw_beat_indicator, draw_beat_wave_ring,
    draw_boss_fissures, draw_call_ring, draw_catch_bloom_ring, draw_catch_shockwaves, draw_catch_trails,
    draw_chain_rings, draw_cleave_slash, draw_cleave_stakes, draw_combo_meter, draw_conga_rope, draw_crab_radar, draw_deliver_beam, draw_delivery_pen,
    draw_delivery_streak, draw_downbeat_pulse_ring, draw_fear_rings, draw_flashlight,
    draw_floating_texts, draw_groove_call_ring,
    draw_groove_vignette, draw_haul_worth, draw_kelp_snag_warning, draw_lasso,
    draw_lasso_windup, draw_particles, draw_pen_guide, draw_penned_marchers,
    draw_puddle_ripples, draw_reef_phrase, draw_rustler, draw_sky_overlay, draw_slam_ring,
    draw_speed_lines, draw_sprint_whoosh, draw_stomp_ring, draw_tail_run_badge, draw_tide_pools, draw_tide_pulses, draw_train_at_risk, draw_wave_telegraph,
    draw_weather, draw_whistle_ring, draw_world_edge, draw_world_map, draw_world_zones, unit_circle, unit_line, unit_square,
};
use crate::graphics::{
    draw_beam_fast_pin, draw_beam_golden_spotlight, draw_beam_hermit_match, draw_beam_sneaky_pin,
    draw_day_weather_hud,
    draw_lasso_big_match, draw_lasso_magnet_match,
    draw_lasso_shell_deflect, draw_lasso_thief_match, draw_magnet_cluster_pull, draw_minimap,
    draw_stomp_armored_crack, draw_stomp_dancer_match, draw_tool_roster, draw_whistle_dancer_match,
    draw_whistle_golden_pull, draw_whistle_shell_deflect, draw_whistle_sneaky_match,
    draw_whistle_thief_match,
};

impl MainState {
    fn draw_instructions_screen(
        &mut self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        width: f32,
        height: f32,
    ) -> GameResult {
        if self.show_how_to_play_text {
            let mut title = Text::new("HOW TO PLAY");
            title.set_scale(56.0);
            let title_w = title.measure(ctx)?.x;
            canvas.draw(
                &title,
                DrawParam::default()
                    .dest(Vec2::new((width - title_w) * 0.5, height * 0.12))
                    .color(Color::from_rgb(235, 235, 220)),
            );

            let body = how_to_play_body_text();
            let mut text = Text::new(body);
            text.set_scale(28.0);
            canvas.draw(
                &text,
                DrawParam::default()
                    .dest(Vec2::new(width * 0.16, height * 0.27))
                    .color(Color::from_rgb(215, 215, 215)),
            );
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
        draw_world_zones(ctx, canvas, world_w, world_h, self.time_elapsed)?;

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
            let map_h = 180.0_f32 * (self.world_height / self.world_width);
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

        // Tool roster — Zelda-style 5-slot bar at the bottom centre.
        if !self.show_instructions && !self.game_over && !self.show_world_map {
            draw_tool_roster(
                ctx,
                canvas,
                width,
                height,
                self.whistle_cooldown,
                crate::WHISTLE_COOLDOWN,
                self.stomp_cooldown,
                crate::STOMP_COOLDOWN,
                self.boost_cooldown,
                !matches!(self.lasso_phase, LassoPhase::Idle),
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

        // Show stats. The HUD line (score/train/combo) only changes on catch/combo events, not
        // every tick, so cache the built Text and only rebuild it (fresh format! String + fresh
        // Text, which re-triggers glyph shaping) when the underlying values actually differ from
        // last frame's — same pattern as the per-level label cache above. Also use the
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
        HUD_TEXT_CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            let needs_rebuild = match &*cache {
                Some((s, cl, cc, m, _)) => {
                    *s != self.score || *cl != chain_len || *cc != self.combo_count || *m != mult
                }
                None => true,
            };
            if needs_rebuild {
                let hud = if self.combo_count >= 3 {
                    format!(
                        "Score: {}  |  Train: {}  |  Combo x{}  [{}x pts]",
                        self.score, chain_len, self.combo_count, mult
                    )
                } else {
                    format!("Score: {}  |  Train: {}", self.score, chain_len)
                };
                *cache = Some((
                    self.score,
                    chain_len,
                    self.combo_count,
                    mult,
                    Text::new(hud),
                ));
            }
            canvas.draw(
                &cache.as_ref().unwrap().4,
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

        // Draw background bar
        canvas.draw(
            unit_square(ctx)?,
            DrawParam::default()
                .dest(Vec2::new(bar_x, bar_y))
                .scale(Vec2::new(bar_width, bar_height))
                .color(Color::from_rgb(40, 40, 40)),
        );

        // Draw boost timer (yellow)
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

        // Draw cooldown (red, overlays boost)
        if cooldown_ratio > 0.0 {
            canvas.draw(
                unit_square(ctx)?,
                DrawParam::default()
                    .dest(Vec2::new(bar_x, bar_y))
                    .scale(Vec2::new(bar_width * cooldown_ratio, bar_height))
                    .color(Color::from_rgb(220, 60, 60)),
            );
        }

        // Draw stamina bar border
        let border = cached_stroke_rect(ctx, bar_width, bar_height, 2.0)?;
        canvas.draw(
            &border,
            DrawParam::default()
                .dest(Vec2::new(bar_x, bar_y))
                .color(Color::from_rgb(255, 255, 255)),
        );

        // Key hint to the right of the bar — compact, no vertical label overhead.
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
        canvas.draw(
            unit_square(ctx)?,
            DrawParam::default()
                .dest(Vec2::new(bar_x, wbar_y))
                .scale(Vec2::new(bar_width, wbar_h))
                .color(Color::from_rgb(40, 40, 40)),
        );
        let (wr, wg, wb) = if ready {
            (255, 210, 90)
        } else {
            (150, 110, 40)
        };
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

        let sbar_y = wbar_y + wbar_h + 6.0;
        let sbar_h = 10.0;
        let sready = self.stomp_cooldown <= 0.0;
        let scharge = (1.0 - self.stomp_cooldown / self.stomp_cooldown_dur()).clamp(0.0, 1.0);
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

        // Debug info: current level in bottom-left corner, small and unobtrusive.
        LEVEL_LABEL_CACHE.with(|c| -> GameResult {
            let mut cache = c.borrow_mut();
            if !cache.contains_key(&self.current_level) {
                let mut label = Text::new(format!(
                    "Level {}: {} | {} | Difficulty: {}",
                    self.current_level + 1,
                    self.levels[self.current_level].title,
                    self.levels[self.current_level].description,
                    self.levels[self.current_level].difficulty
                ));
                label.set_scale(13.0);
                let dims = label.measure(ctx)?;
                cache.insert(self.current_level, (label, dims.x, dims.y));
            }
            let (label, _label_width, label_height) = cache.get(&self.current_level).unwrap();
            canvas.draw(
                label,
                DrawParam::default()
                    .dest(Vec2::new(8.0, height - label_height - 6.0))
                    .color(Color::from_rgba(180, 180, 180, 80)),
            );
            Ok(())
        })?;

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

        // Beat indicator (top right)
        let beat_center = Vec2::new(width - 50.0, 50.0);
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
                self.sounds.action_music.pause();
                if self.sounds.outro_music.playing() {
                    self.sounds.outro_music.pause();
                }
                if !self.sounds.intro_music.playing() {
                    self.sounds.intro_music.play();
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
            if self.sounds.action_music.playing() {
                self.sounds.action_music.pause();
            }
            if !self.sounds.intro_music.playing() {
                self.sounds.intro_music.play();
            }
            self.draw_instructions_screen(ctx, &mut canvas, width, height)?;
            canvas.finish(ctx)?;
            return Ok(());
        } else if self.game_over {
            self.sounds.action_music.pause();
            if !self.sounds.outro_music.playing() {
                self.sounds.outro_music.play();
            }
            self.draw_game_over_screen(ctx, &mut canvas)?;
        } else {
            if self.sounds.intro_music.playing() {
                self.sounds.intro_music.pause();
            }
            if !self.sounds.action_music.playing() {
                self.sounds.action_music.play();
            } else {
                self.sounds.action_music.resume();
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
