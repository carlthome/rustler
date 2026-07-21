//! World backdrop rendering for `MainState`.
//!
//! Extracted from `game_render.rs`: everything `draw_game` paints UNDER the crabs
//! — the ground/terrain layer (world zones, sky, edges, motes, puddles, tide pools,
//! boss fissures, treasure), the delivery pen, and the in-world train readouts
//! (haul/at-risk/streak/cleave/tail-run badges) plus the chain rings, conga rope and
//! on-beat catch-bloom. Pure rendering — no gameplay-logic mutation; `draw_game`
//! calls `draw_world_backdrop` and then draws the crabs and foreground on top.

use ggez::glam::Vec2;
use ggez::graphics::{Canvas, Color, DrawParam};
use ggez::{Context, GameResult};

use crate::constants::*;
use crate::enemies::CrabType;
use crate::graphics::{
    draw_ambient_motes, draw_boss_fissures, draw_catch_bloom_ring, draw_chain_rings,
    draw_cleave_stakes, draw_conga_rope, draw_deliver_beam, draw_delivery_pen,
    draw_delivery_streak, draw_haul_worth, draw_kelp_snag_warning, draw_penned_marchers,
    draw_puddle_ripples, draw_sky_overlay, draw_tail_run_badge, draw_tide_pools,
    draw_train_at_risk, draw_world_edge, draw_world_zones, unit_square,
};
use crate::hud_cache::*;
use crate::panic_snap_links;
use crate::state::*;

impl MainState {
    /// Draws the full world backdrop that sits under the crabs: the ground/terrain
    /// layer, the delivery pen and every in-world train readout. Called first thing
    /// in `draw_game`, before the crabs and player are drawn on top.
    pub(crate) fn draw_world_backdrop(
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

        Ok(())
    }
}
