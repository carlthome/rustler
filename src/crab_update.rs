//! Per-frame simulation of every crab on the field: flashlight beam catches, boss/shell
//! drain, magnet and golden-lure pulls, downbeat impulses, charm/flee/homing behaviour, and
//! the once-per-frame caches (tail position, steal target) that later systems reuse.
//!
//! Extracted verbatim from `main.rs` as a single `impl MainState` method to keep that file
//! navigable. Pure structural move — no behaviour change.

use ggez::glam::Vec2;
use rand::Rng;

use crate::*;

impl MainState {
    pub(crate) fn update_crabs(&mut self, dt: f32, area: (f32, f32)) {
        // Flashlight auto-aims at the nearest King Crab (set each frame before update_crabs).
        let flashlight_dir = self.flashlight.aim_dir;

        let base_cone_angle = std::f32::consts::FRAC_PI_3;
        let base_range = 320.0;

        let mut flashlight_cone_angle = base_cone_angle + self.flashlight.cone_upgrade;
        let mut flashlight_range = base_range + self.flashlight.range_upgrade;
        // Drum Roll fired blast: while the release window is live, the beam FLARES WIDE and FAR down
        // the aim — the fired charge (drum_roll_power) scales how much. This reuses the existing beam
        // catch path below (the cone/range tests at ~3348 and ~3616) instead of a second scan over
        // the crabs, so every free crab caught in the widened aimed arc snaps in exactly like a
        // normal beam catch — no parallel catch loop, no double-catch. Directional, not radial: it's
        // a big sweep down where you're pointing, distinct from the Downbeat Slam's all-around yank.
        if self.drum_roll_fire > 0.0 {
            let boost = self.drum_roll_fire * (self.drum_roll_power as f32 / DRUM_ROLL_MAX as f32);
            flashlight_cone_angle += boost * std::f32::consts::FRAC_PI_3; // up to +60° half-angle at full power
            flashlight_range += boost * 260.0; // up to +260px reach at full power
        }
        // Squared once here so the per-crab light-range check below can compare against
        // distance_squared and skip a sqrt() for every free crab, every frame.
        let flashlight_range_sq = flashlight_range * flashlight_range;
        // Beam-lane-scaled boss/shell drain, read once so the &mut self.crabs loop can use it.
        let boss_drain = self.boss_drain_rate();
        // Drum Roll fired blast → a boss-shell CRACKER. While the release window is live, the beam
        // doesn't just widen (above) — it hammers a boss shell far harder than a held beam, scaled by
        // the charge power banked at fire. This is the rhythm verb pulled *into* the boss duel: a
        // real reason to spend a bar charging mid-fight instead of only using it to sweep the herd.
        // Read once here so the &mut self.crabs loop can fold it into the existing gated drain path
        // below (line ~3512) rather than a parallel damage pass — crucially, that keeps it *inside*
        // `drain_active`, so against the call-locked Reef DJ the blast only bites on a hot beat and
        // its echo-the-phrase identity is preserved instead of being cracked off-phrase.
        let drum_roll_boss_mult = if self.drum_roll_fire > 0.0 {
            1.0 + 6.0 * (self.drum_roll_power as f32 / DRUM_ROLL_MAX as f32)
        } else {
            1.0
        };

        // Event-collection scratch buffers, reused every frame (see field docs) instead of
        // being freshly allocated here — most frames leave every one of these empty. Taken out
        // (rather than borrowed) so the later celebration loops are free to call back into
        // methods that need a full `&mut self`; the buffers (and their capacity) are restored
        // at the end of this function so next frame reuses the same allocation.
        // Positions of crabs that just entered panic-flee this frame — we'll emit "!" pops after the loop
        let mut flee_pops = std::mem::take(&mut self.flee_pops_buf);
        flee_pops.clear();
        // Golden crabs a roaming Magnet's field just snared this frame — celebrated after the loop.
        let mut golden_snare_pops = std::mem::take(&mut self.golden_snare_pops_buf);
        golden_snare_pops.clear();
        let mut thief_snare_pops = std::mem::take(&mut self.thief_snare_pops_buf);
        thief_snare_pops.clear();
        let mut magnet_lure_pops = std::mem::take(&mut self.magnet_lure_pops_buf);
        magnet_lure_pops.clear();
        // Emergent crossover — Armored shells a charged Magnet's widened vacuum ground open this
        // frame (see the grind branch in the per-crab loop below). Collected here so the chip/crack
        // feedback fires after the &mut self.crabs borrow ends.
        let mut magnet_grind = std::mem::take(&mut self.magnet_grind_buf);
        magnet_grind.clear();
        let mut thief_lure_pops = std::mem::take(&mut self.thief_lure_pops_buf);
        thief_lure_pops.clear();
        // Positions of King Crabs that just got worn down this frame — celebrate after the loop
        let mut boss_broke = std::mem::take(&mut self.boss_broke_buf);
        boss_broke.clear();
        // Positions of Armored crabs whose shell the beam just wore through — pop a "crack" after the loop
        let mut armor_broke = std::mem::take(&mut self.armor_broke_buf);
        armor_broke.clear();
        // Sparkle particles for attracted crabs (collected to avoid borrow conflict)
        let mut attraction_particles = std::mem::take(&mut self.attraction_particles_buf);
        attraction_particles.clear();
        // King Crab charge telegraph events, collected to sidestep the &mut self.crabs borrow.
        let mut boss_windups = std::mem::take(&mut self.boss_windups_buf); // a charge just started winding up
        boss_windups.clear();
        let mut boss_launches = std::mem::take(&mut self.boss_launches_buf); // a wound-up charge just fired
        boss_launches.clear();
        let mut boss_charge_dust = std::mem::take(&mut self.boss_charge_dust_buf); // (pos, vel) trail while lunging
        boss_charge_dust.clear();
        // A boss crossed into its enrage phase this frame — (pos, is_tide). Fired once per boss.
        let mut boss_enrages = std::mem::take(&mut self.boss_enrages_buf);
        boss_enrages.clear();
        // Tide Boss pulse fires this frame (center positions) — processed after the loop so the
        // shockwave can scatter the herd and loosen the train without fighting the &mut borrow.
        // Reused scratch buffers like the other event vecs above: almost always empty (at most
        // one boss pulsing at a time), so taking/restoring avoids a Vec::new() every frame.
        let mut tide_fires = std::mem::take(&mut self.tide_fires_buf);
        tide_fires.clear();
        let mut tide_swells = std::mem::take(&mut self.tide_swells_buf); // a pulse just started swelling — telegraph feedback
        tide_swells.clear();

        // Where the King Crab aims: the exposed tail of the conga train if there is one, else the
        // player — "whoever currently holds the highest chain_index". Folded into the single
        // snapshot pass below (tracked via a running best-chain_index candidate) instead of its own
        // full scan, alongside the Magnet/Golden/Armored position snapshots that used to each walk
        // self.crabs separately: 4 full passes over a struct with 20+ fields collapsed into 1. Same
        // results, same order-independent picks (positions just need membership, tail just needs the
        // max chain_index), a quarter of the cache traffic before the real per-crab loop even starts.
        let mut magnet_positions = std::mem::take(&mut self.magnet_positions_buf);
        magnet_positions.clear();
        let mut golden_lure_positions = std::mem::take(&mut self.golden_lure_positions_buf);
        golden_lure_positions.clear();
        let mut armored_positions = std::mem::take(&mut self.armored_positions_buf);
        armored_positions.clear();
        let mut best_chain: Option<(usize, Vec2, CrabType)> = None;
        let mut free_splitter = false;
        // Splice targeting: when the chain is long enough (>= 4 links), the King Crab aims at a
        // mid-chain crab rather than the tail — this maximizes the stolen count (everything behind
        // the crossing point goes). The target is whichever caught crab sits closest to 1/3 from
        // the tail (low enough to steal a big chunk, high enough to cross the body rather than
        // just nipping the end). Falls back to the tail if the chain is short or no caught crabs exist.
        // target_ci only depends on self.chain_count (unchanged by this loop), so the search is
        // folded into the same pass as best_chain/magnet/golden/armored below instead of its own
        // second full scan over self.crabs.
        let target_ci = if self.chain_count >= 4 {
            Some(self.chain_count * 2 / 3) // aim 2/3 down from head = 1/3 from tail
        } else {
            None
        };
        let mut splice_best_dist = f32::MAX;
        let mut splice_target_pos: Option<Vec2> = None;
        for c in &self.crabs {
            if c.caught {
                if let Some(ci) = c.chain_index {
                    if best_chain.map_or(true, |(bci, ..)| ci > bci) {
                        best_chain = Some((ci, c.pos, c.crab_type));
                    }
                    if let Some(target_ci) = target_ci {
                        let dist = (ci as i32 - target_ci as i32).unsigned_abs() as f32;
                        if dist < splice_best_dist {
                            splice_best_dist = dist;
                            splice_target_pos = Some(c.pos);
                        }
                    }
                }
                continue; // caught crabs can't be a Magnet/Golden/Armored source below
            }
            if c.is_splitter() {
                free_splitter = true;
            } else if c.is_magnet() {
                magnet_positions.push(c.pos);
            } else if c.is_golden() {
                if !c.in_flashlight {
                    golden_lure_positions.push(c.pos);
                }
            } else if c.is_armored() {
                armored_positions.push(c.pos);
            }
        }
        let chain_tail_pos = best_chain.map(|(_, pos, _)| pos);
        let charge_target =
            splice_target_pos.unwrap_or_else(|| chain_tail_pos.unwrap_or(self.player_pos));
        // Captured before the &mut self.crabs loop: while the post-scatter regroup window is live the
        // King Crab can't wind up a fresh charge, so you can't be chain-detonated back-to-back.
        let boss_hit_iframes_active = self.boss_hit_iframes > 0.0;
        // Cache for steal_chain_thief (called later this frame, after update_crabs returns) so it
        // doesn't need its own third O(n) scan over self.crabs for the same "current tail" lookup.
        self.cached_tail_pos = chain_tail_pos;
        // Cache the same back-half thread point the boss aims at (~2/3 down from head) so the ambient
        // rival NPC trains can route deliberately into the body of a long train instead of only nipping
        // the tail — no extra scan, we just reuse splice_target_pos computed above. None on a short chain.
        self.cached_steal_target_pos = splice_target_pos;
        // Cache the tail archetype for the draw-path CATCH-NEXT highlight (same snapshot, no extra scan).
        self.cached_tail_type = best_chain.map(|(_, _, ty)| ty);
        // The cycle preview marker is only meaningful with a real train (>= 2 links) and while the
        // cycle verb is actually available (off cooldown), so it shows exactly when pressing X would
        // do something. The draw path finds the chain_index==1 crab itself.
        self.cycle_preview_active = self.chain_count >= 2 && self.cycle_cooldown <= 0.0;
        // Cache for the draw path: avoids an O(n) .any() scan over all crabs every frame to gate
        // the cleave-stakes tag. Updated here in the snapshot pass we already do over every crab.
        self.free_splitter_present = free_splitter;

        // Magnet-crab pull: free-roaming Magnet crabs each tug nearby uncaught crabs toward
        // themselves, so the herd clumps up around them. Snapshotted above so each ordinary crab
        // can pull toward the nearest one without a nested borrow. Almost always a tiny list
        // (Magnets are ~8% of the herd and rare), so a flat per-crab nearest-magnet scan is cheap.
        const MAGNET_RADIUS: f32 = 240.0; // how far a Magnet's pull reaches
        const MAGNET_RADIUS_SQ: f32 = MAGNET_RADIUS * MAGNET_RADIUS; // avoids a sqrt per candidate below

        // Emergent crossover — a snared Golden supercharges its captor Magnet. The Magnet-snares-
        // Golden pass already traps a straying shiny in a lodestone's field; here that trapped prize
        // feeds back into the field. While a Magnet is pinning a snared Golden, the Golden's shine
        // energizes it, so it vacuums the surrounding herd in over a *wider* radius and with a
        // stronger tug than a plain roaming Magnet. Neither rule authored this: "Magnet snares
        // Golden" and "Magnet pulls the herd" collide to turn trapping the prize into a herd-vacuum
        // — trap the Golden in a wandering Magnet and it also balls up the nearby loose crabs into a
        // tight cluster you can then sweep with one beam pass. Snapshot which Magnets are charged
        // this frame: a Magnet is charged if a snared Golden sits inside its normal pull radius.
        // Cheap — Magnets and snared Goldens are both rare, so this double loop is almost always over
        // near-empty lists. Reuses a scratch Vec to avoid per-frame churn.
        let mut charged_magnet_positions = std::mem::take(&mut self.charged_magnet_positions_buf);
        charged_magnet_positions.clear();
        for c in &self.crabs {
            if c.is_golden() && !c.caught && c.magnet_snared > 0.0 {
                // Attribute this snared Golden to its nearest Magnet (the one that trapped it).
                let mut nearest: Option<(f32, Vec2)> = None;
                for &mp in magnet_positions.iter() {
                    let d2 = c.pos.distance_squared(mp);
                    if d2 < MAGNET_RADIUS_SQ && nearest.map_or(true, |(bd2, _)| d2 < bd2) {
                        nearest = Some((d2, mp));
                    }
                }
                if let Some((_, mp)) = nearest {
                    if !charged_magnet_positions.contains(&mp) {
                        charged_magnet_positions.push(mp);
                    }
                }
            }
        }
        // How many charged positions come from a pinned Golden. Positions past this index are
        // Dancer-thumped Magnets appended below — the refresh pass uses this split so a Golden-pin
        // keeps its charge topped up (it holds as long as the prize is pinned) while a Dancer thump
        // is a one-shot surge that decays on its own timer instead of latching on forever.
        let golden_charged_count = charged_magnet_positions.len();
        for c in &self.crabs {
            // Emergent crossover — a Dancer's on-beat hop just jostled this Magnet into a pull surge
            // (see the Dancer-jolts-Magnet block in the beat handler). Its `magnet_charged` timer,
            // set on the beat, is still live: treat it as a charged Magnet here too so the same
            // wider-reach herd-vacuum that a snared Golden buys also fires when a Dancer thumps it,
            // reusing the exact charged-field pass below instead of authoring a second one. A Magnet
            // that's *both* pinning a Golden and freshly thumped is already in the list — the
            // contains() guard keeps it single (and Golden-attributed, so it keeps refreshing).
            if c.is_magnet()
                && !c.caught
                && c.magnet_charged > 0.0
                && !charged_magnet_positions.contains(&c.pos)
            {
                charged_magnet_positions.push(c.pos);
            }
        }
        // Magnet cluster detection: on-beat only (rhythmic flash), check each free Magnet
        // for ≥3 nearby free crabs — the "pied-piper vacuum" tell. Fires on the beat so it
        // pulses with the music rather than strobing every frame.
        // Single pass over crabs tallying into a per-magnet counter, instead of the old
        // one-full-crab-scan-per-magnet (O(magnets * crabs) with magnets separate closures
        // re-walking the whole herd each time) — same per-magnet-independent counting
        // semantics (a crab in range of two overlapping magnet fields still counts for both),
        // just one cache-friendly walk of self.crabs instead of magnet_positions.len() of them.
        let cluster_on_beat =
            self.beat_timer < BEAT_WINDOW || self.beat_timer > self.beat_interval - BEAT_WINDOW;
        if cluster_on_beat && !magnet_positions.is_empty() {
            let mut cluster_counts = std::mem::take(&mut self.magnet_cluster_counts_buf);
            cluster_counts.clear();
            cluster_counts.resize(magnet_positions.len(), 0);
            for c in &self.crabs {
                if c.caught || c.is_magnet() || c.is_boss() {
                    continue;
                }
                for (mi, &mp) in magnet_positions.iter().enumerate() {
                    if c.pos.distance_squared(mp) < MAGNET_RADIUS_SQ {
                        cluster_counts[mi] += 1;
                    }
                }
            }
            for (mi, &mp) in magnet_positions.iter().enumerate() {
                if cluster_counts[mi] >= 3 && self.magnet_cluster_hits_buf.len() < 8 {
                    self.magnet_cluster_hits_buf.push(mp);
                }
            }
            self.magnet_cluster_counts_buf = cluster_counts;
        }

        // A charged Magnet's field reaches ~40% farther and tugs harder while it holds a prize.
        const CHARGED_MAGNET_RADIUS: f32 = MAGNET_RADIUS * 1.4;
        const CHARGED_MAGNET_RADIUS_SQ: f32 = CHARGED_MAGNET_RADIUS * CHARGED_MAGNET_RADIUS;

        // Emergent crossover — the Golden lures the Magnet. `golden_lure_positions` (every free,
        // un-beamed Golden's position) was snapshotted in the single pass above, so a roaming
        // Magnet can be drawn *off its cluster* toward the shiny prize: the mirror of the
        // Magnet-snares-Golden interaction (there the Magnet traps the Golden; here the Golden's
        // shine pulls the Magnet away from tending its herd).
        const MAGNET_LURE_RADIUS: f32 = 300.0; // a Magnet notices a Golden from a bit farther than its own pull reaches
        const MAGNET_LURE_RADIUS_SQ: f32 = MAGNET_LURE_RADIUS * MAGNET_LURE_RADIUS;

        // Emergent crossover — a free Armored crab body-blocks a charging King Crab. The Armored
        // crab is already established as a wall (its calm-anchor shell shelters the herd from panic
        // ripples); here that same stubborn shell also stops a boss lunge cold. `armored_positions`
        // (every free Armored crab's position) was snapshotted in the single pass above so the King
        // Crab's charge arm below can test whether its lane plows through one — if it does, the
        // shell clangs, the boss skids to a halt on cooldown, and the tail it was aiming for is
        // spared. Parking or leaving an Armored crab between the boss and your train becomes a real
        // defensive routing play — the mirror of a Magnet between your train and an incoming Thief.
        // A charging King Crab that rams a free Armored crab this frame — (boss_pos, shell_pos) so
        // the shell-clang feedback fires after the borrow ends. Almost always empty (needs a boss
        // mid-lunge overlapping a shell), so a reused scratch Vec keeps it allocation-free.
        let mut boss_blocks = std::mem::take(&mut self.boss_blocks_buf);
        boss_blocks.clear();
        // King Crab positions stunned by ramming a parked Armored shell this frame — daze feedback
        // fires after the borrow ends, same deferred pattern as boss_blocks above.
        let mut boss_stuns = std::mem::take(&mut self.boss_stuns_buf);
        boss_stuns.clear();

        // Snapshot the current conga tail position so free Thief crabs can home in on it below
        // (they ignore the herd and beeline for the train's exposed end). Only meaningful once the
        // train is long enough for the Thief's steal to bite; otherwise Thieves just roam. This is
        // the same crab chain_tail_pos already found above (highest chain_index), so reuse it
        // instead of a second scan.
        let thief_tail_pos: Option<Vec2> = if self.chain_count >= 4 {
            chain_tail_pos
        } else {
            None
        };

        // Single RNG for the whole per-crab loop below (attraction sparkles), instead of grabbing
        // a fresh thread-local handle inside the loop for every crab currently in the beam.
        let mut rng = crate::rng::rng();

        // Hermit King escape events this frame: it reached the world edge in its Panicked phase
        // and dragged a fresh shell-house stack back in. Processed after the loop (banner + ring).
        // Vec::new() never allocates until a push, so this is free on every ordinary frame.
        let mut hermit_king_reshells: Vec<Vec2> = Vec::new();

        // Dancer King drift snapshot for the entrancement pass below: spellbound free crabs shadow
        // the King's own movement, so they need its position/velocity before the mutable loop.
        let dancer_king_drift: Option<(Vec2, Vec2)> = self.crabs.iter().find_map(|c| {
            if c.is_dancer_king() && !c.caught {
                Some((c.pos, c.vel))
            } else {
                None
            }
        });

        // Snapshot whether we're inside the on-beat window right now, so the Reef DJ (rhythm boss)
        // can gate its shell-drain on the beat without re-borrowing self mid-loop. Same window the
        // player already feels for PERFECT tool hits and the on-beat Call.
        let on_beat_now =
            self.beat_timer < BEAT_WINDOW || self.beat_timer > self.beat_interval - BEAT_WINDOW;
        // Downbeat herd-pulse strength for this frame, snapshotted so the per-crab loop can apply a
        // gentle player-ward nudge to free crabs without re-borrowing self. Decays over the frames
        // after each downbeat (set to 1.0 in the beat handler), so the tug fades between beats and
        // the herd only visibly clumps on the "1". Player center is read once here too.
        let downbeat_pull = self.downbeat_pull;
        let downbeat_pull_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
        self.downbeat_pull = (self.downbeat_pull - dt * 4.0).max(0.0); // ~0.25s falloff
        // Is *this* on-beat one the Reef DJ called? Its shell only drains on a hot beat of the
        // current phrase (see the phrase roll in the beat handler), so holding light on it during a
        // silent beat does nothing — you have to echo the called pattern back. beat_count is already
        // advanced for this beat (the beat handler runs earlier this frame), so beat_count % 4 is the
        // current beat's slot in the bar. A hit on a hot beat kicks reef_hit_flash for juice.
        let reef_hot_now = on_beat_now && self.reef_phrase[(self.beat_count % 4) as usize];
        let mut reef_hit_landed = false;
        // Recomputed each frame from the live crab list: true while an un-caught Reef DJ is on the
        // field. Gates the phrase roll + HUD telegraph so they only appear during a rhythm-boss fight.
        let mut reef_on_field = false;
        // Live Reef DJ position, captured so we can ring its backup "hype Dancers" out from it.
        let mut reef_boss_pos = Vec2::ZERO;

        // Tide Pool current snapshot: only the Water biome's native pools carry a drift. Precomputed
        // once here (a &self method + a slice bound) so the per-crab loop below can drift free crabs
        // without re-borrowing self. Trailing flood pools are Tide Boss surge water, not native
        // current, so we drift only within `tide_current_pools`. Empty on any non-Water biome, which
        // makes the per-crab check below a cheap `is_empty()` short-circuit on those zones.
        let tide_current_active = self.current_terrain() == TerrainKind::Water;
        let tide_current_native = if tide_current_active {
            self.tide_pools.len().saturating_sub(self.boss_flood_pools)
        } else {
            0
        };
        let tide_current_pools: &[(Vec2, f32)] = &self.tide_pools[..tide_current_native];

        // Neon Kelp funnel snapshot: the Kelp biome's routing mechanic, mirroring the tide-current
        // slice above. Only the Kelp biome's native patches funnel — trailing flood pools are Tide
        // Boss water, not weed. Empty on every non-Kelp biome, so the per-crab check below short-
        // circuits on a cheap `is_empty()` everywhere else. Unlike the water current this only grabs
        // *fleeing* crabs, but the pool slice it channels through is computed the same way.
        let kelp_funnel_active = self.current_terrain() == TerrainKind::Kelp;
        let kelp_funnel_native = if kelp_funnel_active {
            self.tide_pools.len().saturating_sub(self.boss_flood_pools)
        } else {
            0
        };
        let kelp_funnel_pools: &[(Vec2, f32)] = &self.tide_pools[..kelp_funnel_native];

        for crab in &mut self.crabs {
            // King Crab boss runs its own charge AI instead of the herd flee/attract logic.
            if crab.is_boss() && !crab.caught {
                if crab.is_rhythm_boss() {
                    reef_on_field = true;
                    reef_boss_pos = crab.pos;
                }
                crab.spawn_time += dt;
                // Tick down the King Crab's daze from ramming a parked Armored shell (set in the
                // charge-block pass below). While it's >0 the boss can't wind up a new charge and
                // its shell drains faster (see the stunned-drain boost above).
                if crab.stun_timer > 0.0 {
                    crab.stun_timer = (crab.stun_timer - dt).max(0.0);
                }
                let distance = self.player_pos.distance(crab.pos);
                let to_crab = (crab.pos - self.player_pos).normalize_or_zero();
                let angle_to_crab = flashlight_dir.angle_to(to_crab).abs();
                let crab_in_light = self.flashlight.on
                    && distance < flashlight_range
                    && angle_to_crab < flashlight_cone_angle;
                crab.in_flashlight = crab_in_light;

                // Wearing it down under the beam is unchanged for the King Crab and Tide Boss —
                // the beam is still how you catch them. The Reef DJ is the exception: its shell is
                // call-locked, so the beam only bites while you hold the light on it during a *hot*
                // beat of the phrase it called this bar. Off the phrase (off-beat, or an un-called
                // on-beat) the light does nothing — the whole fight is echoing its pattern back with
                // the light. Enraged, it drains faster on a hit so the finale rewards clean timing.
                let drain_active = crab_in_light
                    && !crab.is_hermit_king() // the Hermit King's shell-house stack is beam-proof: only Stomps crack it (see the stomp pass in game_update)
                    && (!crab.is_rhythm_boss() || reef_hot_now);
                if crab.is_rhythm_boss() && crab_in_light && reef_hot_now && crab.boss_health > 0.0
                {
                    reef_hit_landed = true;
                }
                if crab.boss_health > 0.0 && drain_active {
                    let mut rate = if crab.is_rhythm_boss() {
                        // The window is narrow AND only some beats are hot, so per-hit drain is boosted
                        // to keep the fight a comparable length to the other bosses; enrage sharpens it.
                        boss_drain * if crab.enraged { 5.0 } else { 3.5 }
                    } else {
                        boss_drain
                    };
                    // Stunned-drain boost: a King Crab reeling from ramming a parked Armored shell
                    // takes far more beam damage, so baiting the lunge into a shell then holding the
                    // light on the dazed boss is a real damage window — the archetype block fused into
                    // the boss fight (see the block pass below where stun_timer is set).
                    if crab.is_stunned() {
                        rate *= 2.5;
                    }
                    // Fired Drum Roll blast cracks the shell far faster than a plain held beam
                    // (up to 7x at full charge). Multiplies the drain here inside the same
                    // `drain_active` gate — so it stacks with a stun window on a King Crab, and
                    // still only lands on a hot beat against the Reef DJ. The wide fired cone also
                    // makes it easier to keep the light on the boss for the short release window.
                    rate *= drum_roll_boss_mult;
                    crab.boss_health -= rate * dt;
                    if crab.boss_health <= 0.0 {
                        crab.boss_health = 0.0;
                        boss_broke.push(crab.pos);
                    }
                }

                // Multi-phase escalation: the moment its health dips below the enrage threshold, the
                // boss enters its final phase. Latch it once so we fire a single dramatic telegraph;
                // the enraged flag then feeds the charge/pulse cadence below to make the climax ramp.
                if !crab.enraged
                    && crab.boss_health > 0.0
                    && crab.boss_health <= crab.boss_max_health * BOSS_ENRAGE_THRESHOLD
                {
                    crab.enraged = true;
                    crab.charge_cooldown = crab.charge_cooldown.min(1.0); // snap toward its next move — no lull into the finale
                    boss_enrages.push((crab.pos, crab.is_tide_boss()));
                }

                // The Tide Boss doesn't charge — it drifts and pulses. Distinct threat, distinct
                // counterplay: keep the train *away* from it (spacing) rather than routing out of a
                // charge lane. It reuses charge_cooldown as its pulse timer and BossCharge::Winding
                // to mean "swelling before a pulse".
                if crab.is_tide_boss() {
                    let (width, height) = area;
                    match crab.charge_state {
                        BossCharge::Winding(t) => {
                            let nt = t - dt;
                            // Rear up and nearly stop while the swell builds — the telegraph window.
                            crab.vel = crab.vel.lerp(Vec2::ZERO, 0.2);
                            crab.pos += crab.vel * dt;
                            crab.charge_state = if nt <= 0.0 {
                                tide_fires.push(crab.pos);
                                // Enraged: rest far less between pulses so the finale hammers the train.
                                crab.charge_cooldown = if crab.enraged {
                                    TIDE_PULSE_COOLDOWN * BOSS_ENRAGE_COOLDOWN_SCALE
                                } else {
                                    TIDE_PULSE_COOLDOWN
                                };
                                BossCharge::Idle
                            } else {
                                BossCharge::Winding(nt)
                            };
                        }
                        _ => {
                            if crab.charge_cooldown > 0.0 {
                                crab.charge_cooldown -= dt;
                            }
                            // Wander gently toward the train's heart so it stays a looming presence.
                            let dir = (charge_target - crab.pos).normalize_or_zero();
                            crab.vel = crab.vel.lerp(dir * crab.speed, 0.02);
                            crab.pos += crab.vel * dt;
                            // Once rested and there's a train worth scattering, begin swelling a pulse.
                            if crab.charge_cooldown <= 0.0 && self.chain_count >= 3 {
                                crab.charge_state = BossCharge::Winding(TIDE_PULSE_WINDUP);
                                tide_swells.push(crab.pos);
                            }
                        }
                    }
                    // Bounce off walls, face travel direction (shared with the King Crab tail below).
                    if crab.pos.x < 0.0 || crab.pos.x > width - crab.scale {
                        crab.vel.x = -crab.vel.x;
                        crab.pos.x = crab.pos.x.clamp(0.0, width - crab.scale);
                    }
                    if crab.pos.y < 0.0 || crab.pos.y > height - crab.scale {
                        crab.vel.y = -crab.vel.y;
                        crab.pos.y = crab.pos.y.clamp(0.0, height - crab.scale);
                    }
                    let speed = crab.vel.length();
                    if speed > 5.0 {
                        let target_angle = crab.vel.y.atan2(crab.vel.x);
                        let mut delta = target_angle - crab.facing_angle;
                        while delta > std::f32::consts::PI {
                            delta -= std::f32::consts::TAU;
                        }
                        while delta < -std::f32::consts::PI {
                            delta += std::f32::consts::TAU;
                        }
                        crab.facing_angle += delta * (dt * 8.0).min(1.0);
                    }
                    continue;
                }

                // The Reef DJ (rhythm boss) doesn't charge or pulse — it just grooves toward the
                // train's heart as a looming presence while you try to land beat-timed light on it.
                // No hazard state machine at all: the entire threat is the timing test on its shell,
                // so it stays a clean, legible set-piece (hold the light, hit the beat, watch the
                // shell drop a chunk every downbeat).
                if crab.is_rhythm_boss() {
                    let (width, height) = area;
                    let dir = (charge_target - crab.pos).normalize_or_zero();
                    crab.vel = crab.vel.lerp(dir * crab.speed, 0.02);
                    crab.pos += crab.vel * dt;
                    if crab.pos.x < 0.0 || crab.pos.x > width - crab.scale {
                        crab.vel.x = -crab.vel.x;
                        crab.pos.x = crab.pos.x.clamp(0.0, width - crab.scale);
                    }
                    if crab.pos.y < 0.0 || crab.pos.y > height - crab.scale {
                        crab.vel.y = -crab.vel.y;
                        crab.pos.y = crab.pos.y.clamp(0.0, height - crab.scale);
                    }
                    let speed = crab.vel.length();
                    if speed > 5.0 {
                        let target_angle = crab.vel.y.atan2(crab.vel.x);
                        let mut delta = target_angle - crab.facing_angle;
                        while delta > std::f32::consts::PI {
                            delta -= std::f32::consts::TAU;
                        }
                        while delta < -std::f32::consts::PI {
                            delta += std::f32::consts::TAU;
                        }
                        crab.facing_angle += delta * (dt * 8.0).min(1.0);
                    }
                    continue;
                }

                // The Hermit King — the shell-house tank. The beam never touches it (drain is
                // gated out above); the whole fight is the Stomp: crack one shell layer per pound
                // (see the stomp pass in game_update). Its movement escalates with its phase:
                // Sturdy = slow lumber at the train, Rattled = fast erratic darts (and only
                // ON-BEAT stomps land), Panicked = a flat-out sprint for the nearest world edge —
                // escape and it drags a fresh shell back in (shell resets, see the edge check).
                if crab.is_hermit_king() {
                    let (width, height) = area;
                    match hermit_king_phase(crab.boss_health) {
                        HermitKingPhase::Sturdy => {
                            // Slow lumber toward the train's heart, same looming-presence read as
                            // the Reef DJ — a big calm target while any Stomp still cracks it.
                            let dir = (charge_target - crab.pos).normalize_or_zero();
                            crab.vel = crab.vel.lerp(dir * crab.speed, 0.02);
                            crab.pos += crab.vel * dt;
                        }
                        HermitKingPhase::Rattled => {
                            // Rattled: fast erratic darts on an irregular timer (host_swap_timer,
                            // the Hermit's own restless clock, reused for its King). Now only an
                            // ON-BEAT stomp lands, so the player must groove AND chase.
                            crab.host_swap_timer -= dt;
                            if crab.host_swap_timer <= 0.0 {
                                let ang = rng.random_range(0.0_f32..std::f32::consts::TAU);
                                crab.vel = Vec2::new(ang.cos(), ang.sin()) * crab.speed * HERMIT_KING_RATTLED_SPEED_MULT;
                                crab.join_pulse = crab.join_pulse.max(0.7); // squash-pop as it darts
                                crab.host_swap_timer = rng.random_range(0.5..0.9);
                            }
                            crab.pos += crab.vel * dt;
                            // Bleed between darts so it lurches rather than glides.
                            crab.vel *= 1.0 - (1.6 * dt).min(0.8);
                        }
                        HermitKingPhase::Panicked => {
                            // One shell left: it bolts for the nearest world edge. Crack it before
                            // it escapes — reach the edge and it drags a fresh shell back in.
                            let to_left = crab.pos.x;
                            let to_right = width - crab.pos.x;
                            let to_top = crab.pos.y;
                            let to_bottom = height - crab.pos.y;
                            let min_d = to_left.min(to_right).min(to_top).min(to_bottom);
                            let dir = if min_d == to_left {
                                Vec2::new(-1.0, 0.0)
                            } else if min_d == to_right {
                                Vec2::new(1.0, 0.0)
                            } else if min_d == to_top {
                                Vec2::new(0.0, -1.0)
                            } else {
                                Vec2::new(0.0, 1.0)
                            };
                            crab.vel = crab.vel.lerp(dir * crab.speed * HERMIT_KING_PANICKED_SPEED_MULT, (4.0 * dt).min(1.0));
                            crab.pos += crab.vel * dt;
                            // Escaped! It scuttles off the sand line and drags a whole fresh
                            // shell-house stack back in — the panic race lost, start over.
                            if min_d < 12.0 {
                                crab.boss_health = crab.boss_max_health;
                                crab.enraged = false;
                                crab.host_swap_timer = 0.6;
                                // Turn it back toward the arena so it re-enters lumbering.
                                let center = Vec2::new(width * 0.5, height * 0.5);
                                crab.vel = (center - crab.pos).normalize_or_zero() * crab.speed;
                                hermit_king_reshells.push(crab.pos);
                            }
                        }
                    }
                    // Clamp inside the world (the Panicked sprint aims at the border on purpose).
                    crab.pos.x = crab.pos.x.clamp(0.0, width - crab.scale);
                    crab.pos.y = crab.pos.y.clamp(0.0, height - crab.scale);
                    let speed = crab.vel.length();
                    if speed > 5.0 {
                        let target_angle = crab.vel.y.atan2(crab.vel.x);
                        let mut delta = target_angle - crab.facing_angle;
                        while delta > std::f32::consts::PI {
                            delta -= std::f32::consts::TAU;
                        }
                        while delta < -std::f32::consts::PI {
                            delta += std::f32::consts::TAU;
                        }
                        crab.facing_angle += delta * (dt * 8.0).min(1.0);
                    }
                    continue;
                }

                // The Dancer King — the evader. No shell at all (catchable from the first frame);
                // its defence is the beat: every 2 beats it teleports to a mirrored position
                // across the world (see the beat handler), and between blinks it just drifts,
                // keeping a wary distance from the player. Free crabs near it fall entranced and
                // shadow its drift — catch the King ON the beat to bank the whole spellbound court.
                if crab.is_dancer_king() {
                    let (width, height) = area;
                    // Drift: sway away from a close player, otherwise a slow regal wander toward
                    // wherever the herd is thin (world center keeps it on stage).
                    let dir = if distance < 300.0 {
                        to_crab // away from the player — it won't be walked down flatly
                    } else {
                        let center = Vec2::new(width * 0.5, height * 0.5);
                        (center - crab.pos).normalize_or_zero() * 0.4
                            + Vec2::new(
                                (self.time_elapsed * 0.7 + crab.beat_phase_offset).cos(),
                                (self.time_elapsed * 0.7 + crab.beat_phase_offset).sin(),
                            ) * 0.6
                    };
                    crab.vel = crab.vel.lerp(dir.normalize_or_zero() * crab.speed, 0.03);
                    crab.pos += crab.vel * dt;
                    if crab.pos.x < 0.0 || crab.pos.x > width - crab.scale {
                        crab.vel.x = -crab.vel.x;
                        crab.pos.x = crab.pos.x.clamp(0.0, width - crab.scale);
                    }
                    if crab.pos.y < 0.0 || crab.pos.y > height - crab.scale {
                        crab.vel.y = -crab.vel.y;
                        crab.pos.y = crab.pos.y.clamp(0.0, height - crab.scale);
                    }
                    let speed = crab.vel.length();
                    if speed > 5.0 {
                        let target_angle = crab.vel.y.atan2(crab.vel.x);
                        let mut delta = target_angle - crab.facing_angle;
                        while delta > std::f32::consts::PI {
                            delta -= std::f32::consts::TAU;
                        }
                        while delta < -std::f32::consts::PI {
                            delta += std::f32::consts::TAU;
                        }
                        crab.facing_angle += delta * (dt * 8.0).min(1.0);
                    }
                    continue;
                }

                // Charge state machine. Holding the beam can't cancel a wind-up — the counterplay is
                // to move the train out of the lane, which is exactly the "route and protect" tension
                // a long conga line should carry.
                match crab.charge_state {
                    BossCharge::Idle => {
                        if crab.charge_cooldown > 0.0 {
                            crab.charge_cooldown -= dt;
                        }
                        // Lumber toward the train so it stays a closing threat.
                        let dir = (charge_target - crab.pos).normalize_or_zero();
                        crab.vel = crab.vel.lerp(dir * crab.speed, 0.02);
                        crab.pos += crab.vel * dt;
                        // Arm a charge once it's rested, the train is worth scattering, and in range.
                        // A stunned (recently-blocked) King Crab can't wind up until the daze passes.
                        if crab.charge_cooldown <= 0.0
                            && !crab.is_stunned()
                            && !boss_hit_iframes_active
                            && self.chain_count >= 3
                            && crab.pos.distance(charge_target) < BOSS_CHARGE_ARM_RANGE
                        {
                            crab.charge_state = BossCharge::Winding(BOSS_WINDUP_TIME);
                            boss_windups.push(crab.pos);
                        }
                    }
                    BossCharge::Winding(t) => {
                        let nt = t - dt;
                        // Rear back: nearly stop and lean away from the target to sell the wind-up.
                        let away = (crab.pos - charge_target).normalize_or_zero();
                        crab.vel = crab.vel.lerp(away * crab.speed * 0.7, 0.15);
                        crab.pos += crab.vel * dt;
                        crab.charge_state = if nt <= 0.0 {
                            // Lock the heading at launch and commit.
                            let mut dir = (charge_target - crab.pos).normalize_or_zero();
                            if dir == Vec2::ZERO {
                                dir = Vec2::new(0.0, 1.0);
                            }
                            // Enraged King Crab lunges harder — a faster, scarier commit in the finale.
                            let charge_speed = if crab.enraged {
                                BOSS_CHARGE_SPEED * BOSS_ENRAGE_CHARGE_SPEED_SCALE
                            } else {
                                BOSS_CHARGE_SPEED
                            };
                            crab.vel = dir * charge_speed;
                            boss_launches.push(crab.pos);
                            BossCharge::Charging(BOSS_CHARGE_TIME)
                        } else {
                            BossCharge::Winding(nt)
                        };
                    }
                    BossCharge::Charging(t) => {
                        let nt = t - dt;
                        crab.pos += crab.vel * dt; // vel stays locked to the launch heading
                        boss_charge_dust.push((crab.pos, crab.vel));
                        // Emergent crossover: did the lunge just plow into a free Armored crab's
                        // shell? If so the wall wins — the charge aborts here, sparing the tail it
                        // was aimed at, and the boss goes on cooldown as if the lunge had spent
                        // itself. The Armored crab is knocked back but keeps its shell (it's not
                        // caught — it just took the hit). Uses the boss's bulk-widened reach so a
                        // near-miss still counts as a block, matching how the tail-snap gives the
                        // charge a wide hitbox.
                        const BLOCK_REACH: f32 = CRAB_SIZE * 1.1;
                        let block_hit = armored_positions.iter().find(|&&ap| {
                            crab.pos.distance(ap) < BLOCK_REACH + crab.scale * CRAB_SIZE * 0.5
                        });
                        if let Some(&shell_pos) = block_hit {
                            crab.charge_cooldown = if crab.enraged {
                                BOSS_CHARGE_COOLDOWN * BOSS_ENRAGE_COOLDOWN_SCALE
                            } else {
                                BOSS_CHARGE_COOLDOWN
                            };
                            // Slamming a shell doesn't just stop the lunge — the impact DAZES the
                            // King Crab. For the stun window it can't wind up a new charge and its
                            // own shell drains far faster under the beam (see the stunned-drain boost
                            // above), turning the Armored block from a purely defensive save into a
                            // real damage opportunity: bait the lunge into a parked shell, then hold
                            // the light on the reeling boss to chunk it down. Fuses the archetype web
                            // with the boss fight, exactly when the fight peaks. Enraged bosses shake
                            // it off a little quicker.
                            crab.stun_timer = if crab.enraged {
                                BOSS_STUN_DURATION * 0.7
                            } else {
                                BOSS_STUN_DURATION
                            };
                            // Keep it dazed at least as long as it's stunned before it can charge again.
                            crab.charge_cooldown = crab.charge_cooldown.max(crab.stun_timer + 0.3);
                            // Bounce the boss back off the shell so the stop reads as an impact,
                            // not a stall, then let it settle into Idle next.
                            crab.vel = -crab.vel.normalize_or_zero() * crab.speed * 0.6;
                            boss_blocks.push((crab.pos, shell_pos));
                            boss_stuns.push(crab.pos);
                            crab.charge_state = BossCharge::Idle;
                        } else {
                            crab.charge_state = if nt <= 0.0 {
                                // Enraged: shorter rest between lunges so the finale keeps the pressure on.
                                crab.charge_cooldown = if crab.enraged {
                                    BOSS_CHARGE_COOLDOWN * BOSS_ENRAGE_COOLDOWN_SCALE
                                } else {
                                    BOSS_CHARGE_COOLDOWN
                                };
                                crab.vel *= 0.15; // skid to a halt out of the lunge
                                BossCharge::Idle
                            } else {
                                BossCharge::Charging(nt)
                            };
                        }
                    }
                }

                // Bounce off the arena walls just like the herd.
                let (width, height) = area;
                if crab.pos.x < 0.0 || crab.pos.x > width - crab.scale {
                    crab.vel.x = -crab.vel.x;
                    crab.pos.x = crab.pos.x.clamp(0.0, width - crab.scale);
                }
                if crab.pos.y < 0.0 || crab.pos.y > height - crab.scale {
                    crab.vel.y = -crab.vel.y;
                    crab.pos.y = crab.pos.y.clamp(0.0, height - crab.scale);
                }
                // Smoothly rotate to face travel direction.
                let speed = crab.vel.length();
                if speed > 5.0 {
                    let target_angle = crab.vel.y.atan2(crab.vel.x);
                    let mut delta = target_angle - crab.facing_angle;
                    while delta > std::f32::consts::PI {
                        delta -= std::f32::consts::TAU;
                    }
                    while delta < -std::f32::consts::PI {
                        delta += std::f32::consts::TAU;
                    }
                    crab.facing_angle += delta * (dt * 8.0).min(1.0);
                }
                continue;
            }

            if !crab.caught {
                crab.spawn_time += dt;

                // If crab is spooked, it will move towards the player. Squared distance avoids a
                // sqrt() for every free crab every frame; the real distance is only taken further
                // down, lazily, for the (usually much smaller) subset of crabs that are actually
                // lit, fleeing, or mid-surge and so need the linear value.
                let distance_sq = self.player_pos.distance_squared(crab.pos);
                let to_crab = (crab.pos - self.player_pos).normalize_or_zero();
                let angle_to_crab = flashlight_dir.angle_to(to_crab).abs();

                // Check if crab is within flashlight light.
                let crab_in_light = self.flashlight.on
                    && distance_sq < flashlight_range_sq
                    && angle_to_crab < flashlight_cone_angle;

                // Track flashlight state on the crab for rendering
                crab.in_flashlight = crab_in_light;

                // Shelled crabs (King Crab boss + Armored herd crabs) must be worn down before they
                // can be caught: holding the beam on one drains its shell. This is the slow universal
                // path — a Stomp cracks an Armored shell instantly, but the beam always works too, so
                // no crab is ever impossible without the right tool.
                //
                // The Hermit is the deliberate exception: the beam CAN'T touch its borrowed shell, so
                // it forces the ecosystem verbs (Stomp / Dancer-hop / Magnet-rip). That's what makes
                // it a genuinely new target rather than an Armored reskin — Armored = crack it with
                // your own tools; Hermit = crack it with the archetype web.
                if crab.boss_health > 0.0 && crab_in_light && !crab.is_hermit() {
                    crab.boss_health -= boss_drain * dt;
                    if crab.boss_health <= 0.0 {
                        crab.boss_health = 0.0;
                        if crab.is_boss() {
                            boss_broke.push(crab.pos);
                        } else {
                            armor_broke.push(crab.pos);
                        }
                    }
                }

                // Strong-match: beam shining on a shelled Hermit. The beam can't crack its
                // borrowed shell (only ecosystem verbs can), but we still collect the hit so
                // draw_beam_hermit_match can flash amber — a legibility cue telling the player
                // "beam won't work here; use Stomp, Dancer, or Magnet instead".
                if crab.is_shelled_hermit() && crab.boss_health > 0.0 && crab_in_light {
                    let drain_fraction = 1.0 - crab.boss_health / crab.boss_max_health.max(0.001);
                    self.beam_hermit_hits_buf.push((crab.pos, drain_fraction));
                }

                // Panic flee: crabs that are close but outside the flashlight beam scatter away.
                // Bosses are unshakeable — they lumber on rather than panic-bolting.
                const FLEE_RADIUS: f32 = 220.0;
                const FLEE_RADIUS_SQ: f32 = FLEE_RADIUS * FLEE_RADIUS;
                // How far the downbeat herd pulse reaches — a bit past the flee radius so crabs
                // hovering just outside panic range are the ones the beat sweeps in, without yanking
                // the whole screen.
                const DOWNBEAT_PULL_RADIUS: f32 = 300.0;
                const DOWNBEAT_PULL_RADIUS_SQ: f32 = DOWNBEAT_PULL_RADIUS * DOWNBEAT_PULL_RADIUS;
                // A whistle-charmed crab holds its nerve near the player instead of bolting, so a
                // well-timed pulse pins a spooked herd in place long enough to sweep them up.
                // Dancer crabs don't panic-flee continuously — their escape is the beat hop
                // (handled in the beat-fire block), so between beats they hold still instead of
                // streaming away. This is what makes them a rhythm-timed grab rather than a chase.
                // A Thief on the hunt for your tail doesn't panic-flee the player between latches —
                // it's single-minded about reaching the train. (A whistle charm still stops it, and
                // once latched it's handled in steal_chain_thief.) This keeps it a committed threat
                // rather than one more crab that scatters when you sweep the beam past it.
                // A shelled Hermit doesn't panic-flee either: clamped inside its borrowed shell it
                // hunkers and holds ground between its scripted host-swap darts (see the dart block
                // below), so it reads as a hiding lump you have to crack rather than a chaser. Once
                // cracked it's an ordinary crab and flees like anything else.
                // The beam is a boss-pressure tool, not a herd lure. Only SHELLED targets the player
                // is actively burning down (Armored / borrowed-shell Hermit — the ones with a shell
                // the beam is chewing through) get "held" by the light; everything else ignores the
                // beam entirely and wanders/flees on its own. This is what keeps the flashlight's
                // identity clean: it burns hard targets, the whistle pulls the herd. A normal crab
                // caught in the cone drifts as if the light weren't there.
                let beam_holds = crab_in_light && crab.boss_health > 0.0 && !crab.is_hermit();
                let now_fleeing = !beam_holds
                    && distance_sq < FLEE_RADIUS_SQ
                    && !crab.is_boss()
                    && !crab.is_dancer()
                    && !crab.is_shelled_hermit()
                    && crab.entranced <= 0.0
                    && !(crab.is_thief() && self.chain_count >= 4)
                    && crab.charm_timer <= 0.0;

                if beam_holds {
                    // A crab whose shell is being seared holds under the beam — it can't scurry off
                    // while you burn it down, so the pressure reads as "pinned and melting". No pull
                    // toward the player (that was the old herd-lure); it simply stops fleeing.
                    crab.vel = crab.vel.lerp(Vec2::ZERO, 0.04);
                    crab.spooked_timer = 0.7;
                    crab.fleeing = false;
                } else if now_fleeing {
                    // Track first-flee frame so we can emit a "!" pop after the loop
                    if !crab.fleeing {
                        flee_pops.push(crab.pos);
                    }
                    crab.fleeing = true;
                    // Panic: steer sharply away from the player at full type speed.
                    let max_speed = crab.crab_type.speed_range().end;
                    // Real distance only needed for this proximity taper, so it's taken here rather
                    // than unconditionally for every crab above.
                    let distance = distance_sq.sqrt();
                    // Proximity factor: full flee speed when very close, tapering off toward FLEE_RADIUS
                    let flee_factor = 1.0 - (distance / FLEE_RADIUS);
                    let mut flee_speed = max_speed * (1.0 + flee_factor * 1.5);
                    // Beam "pin" — the flashlight's soft-RPS STRONG match against the Fast archetype
                    // (INSPIRATION.md Doom Eternal note: "Beam to melt fast ones"). A sprinting Fast
                    // crab is the ONE herd crab the beam grips: hold the cone on it and the light
                    // drags on its escape so its speed advantage stops mattering — the tool choice
                    // becomes the decision, not a losing footrace. It's a drum pad: pinning ON the
                    // beat clamps the sprinter hard, off the beat only grazes it, so keeping the beam
                    // on a fleeing Fast crab THROUGH the beat is the skill that reels it in. Only Fast
                    // crabs feel it — every other archetype ignores the beam while fleeing, so the
                    // flashlight's identity stays "burns hard targets + pins sprinters", not a herd lure.
                    if crab.is_fast() && crab_in_light {
                        let pin = if on_beat_now { 0.38 } else { 0.62 };
                        flee_speed *= pin;
                        crab.spooked_timer = crab.spooked_timer.max(0.5);
                        if self.beam_fast_hits_buf.len() < 12 {
                            self.beam_fast_hits_buf.push((crab.pos, on_beat_now));
                        }
                    }
                    // Beam × Golden STRONG match — "spotlight the prize" (ROADMAP RPS lane, the
                    // beam-vs-Golden pair). The flashlight is a spotlight: hold it on the fleeing
                    // treasure and the light reveals and reels it, so keeping your beam on a Golden
                    // through the beat is how you land the prize instead of losing the footrace. It's
                    // deliberately a GENTLER grip than the Fast pin (0.70/0.55 vs 0.62/0.38) — the
                    // Golden is the reward, so it stays a premium chase, not a trivial pin — but on
                    // the beat the reel firms up, a drum pad against the prize. A snared Golden (a
                    // Magnet already has it) is skipped so the two grips don't stack. Distinct warm-gold
                    // tell so it never reads as the icy Fast pin or the amber Hermit "wrong tool".
                    if crab.is_golden() && crab_in_light && crab.magnet_snared <= 0.0 {
                        let reel = if on_beat_now { 0.55 } else { 0.70 };
                        flee_speed *= reel;
                        crab.spooked_timer = crab.spooked_timer.max(0.5);
                        if self.beam_golden_hits_buf.len() < 12 {
                            self.beam_golden_hits_buf.push((crab.pos, on_beat_now));
                        }
                    }
                    // Beam × Sneaky STRONG match — "expose the sneak". The Sneaky crab's whole schtick
                    // is darting off readily (enemies.rs) — the ONE common herd crab that bolts clean out
                    // of the cone and, until now, laughed off the beam entirely. The flashlight is a
                    // spotlight: hold it on the fleeing evader and the light catches it in the act, so
                    // keeping your beam on a Sneaky THROUGH the beat pins it long enough to sweep it up.
                    // This does NOT tread on the whistle's flagship — the whistle *gathers* the skittish
                    // HERD (an AOE reel); the beam *pins the ONE* Sneaky you're chasing solo. Two verbs,
                    // one archetype, the player's choice (Doom Eternal soft-RPS). Grip sits between the
                    // Fast clamp (0.62/0.38) and the premium Golden reel (0.70/0.55): firm but not a hard
                    // lock, since a light evader shouldn't pin as cheaply as a straight-line sprinter.
                    // (A whistle-charmed Sneaky never reaches here — the now_fleeing gate above already
                    // requires charm_timer <= 0 — so the beam pin and whistle charm can't stack.)
                    if crab.is_sneaky() && crab_in_light {
                        let pin = if on_beat_now { 0.42 } else { 0.66 };
                        flee_speed *= pin;
                        crab.spooked_timer = crab.spooked_timer.max(0.5);
                        if self.beam_sneaky_hits_buf.len() < 12 {
                            self.beam_sneaky_hits_buf.push((crab.pos, on_beat_now));
                        }
                    }
                    crab.vel = crab.vel.lerp(to_crab * flee_speed, 0.06);
                    crab.speed = 1.0; // vel already encodes speed, keep multiplier neutral
                } else {
                    crab.fleeing = false;
                    // Downbeat herd pulse: a passive, rhythmic routing nudge. A free, un-spooked crab
                    // drifts a little toward the player on the "1" of the bar, so a groove-savvy
                    // player can stand where the next downbeat will sweep loose crabs into their beam.
                    // Deliberately gentle and range-gated (a routing tug, not a yank or a catch), and
                    // skipped for charmed/startled/snared crabs so it never fights the other passes or
                    // turns into an autocatcher next to the on-beat catch bloom. Only meaningful for a
                    // few frames after each downbeat, then it fades and the crab wanders freely again.
                    if downbeat_pull > 0.0
                        && crab.startle_timer <= 0.0
                        && crab.charm_timer <= 0.0
                        && crab.magnet_snared <= 0.0
                        && distance_sq < DOWNBEAT_PULL_RADIUS_SQ
                    {
                        let toward = (downbeat_pull_center - crab.pos).normalize_or_zero();
                        // Pull toward the crab's *top* speed so the clump is visible on the "1" — a
                        // real routing tug, not a decorative ring. The flee/light passes still win
                        // when they apply (this is the wander `else` branch only), and the gates
                        // above (free, un-startled, un-charmed, un-snared) are what keep it from
                        // becoming an autocatcher, not the magnitude — so it can be assertive.
                        let nudge = crab.crab_type.speed_range().end * 1.1 * downbeat_pull;
                        crab.vel = crab.vel.lerp(toward * nudge, 0.35 * downbeat_pull);
                    }
                }

                // Dancer King entrancement: a spellbound free crab stops thinking for itself and
                // shadows the King's own drift — a synchronized court trailing the royal across
                // the sand. Renewed each beat while the crab stays near the King (see the beat
                // handler); snaps out the moment the King is gone. `speed = 1.0` keeps the
                // multiplier neutral so `vel` alone encodes the sway, matching the flee path
                // (never compound vel × speed — see the failure-mode note in AGENTS.md).
                if crab.entranced > 0.0 {
                    crab.entranced = (crab.entranced - dt).max(0.0);
                    if let Some((kpos, kvel)) = dancer_king_drift {
                        // Ease toward the King's drift plus a gentle tether pull, so the court
                        // trails the royal in formation instead of scattering.
                        let tether = (kpos - crab.pos).normalize_or_zero() * 16.0;
                        crab.vel = crab.vel.lerp(kvel + tether, (3.0 * dt).min(1.0));
                        crab.speed = 1.0;
                        crab.fleeing = false;
                    } else {
                        crab.entranced = 0.0; // the King is caught or gone — the spell breaks
                    }
                }

                // Calm down after timer
                if crab.spooked_timer > 0.0 {
                    crab.spooked_timer -= dt;
                    if crab.spooked_timer < 0.0 {
                        crab.spooked_timer = 0.0;
                    }
                }

                // Startle from a nearby catch (stampede ripple): the crab keeps its outward
                // bolt speed for a beat. The light re-attracts it (in_light lerp above wins),
                // so sweeping the beam over a scattering herd holds them.
                if crab.startle_timer > 0.0 {
                    crab.startle_timer -= dt;
                    if crab.startle_timer < 0.0 {
                        crab.startle_timer = 0.0;
                    }
                }

                // Amplified Golden panic bleeds back toward ordinary fear as the crab settles,
                // so the panic bomb's extra kick spans only the next few beats rather than
                // permanently supercharging every crab it touched.
                if crab.panic_amp > 1.0 {
                    crab.panic_amp = (crab.panic_amp - dt * 1.2).max(1.0);
                }

                // The Magnet snare lapses if the Golden isn't re-snared this frame (i.e. it drifted
                // out of a Magnet's deep field, or the Magnet was caught). The pull pass above
                // refreshes it back to 0.25 every frame the tether holds, so this only fires the
                // instant the field releases it.
                if crab.magnet_snared > 0.0 {
                    crab.magnet_snared = (crab.magnet_snared - dt).max(0.0);
                }

                // A Golden fired by a Tide Boss slingshot stays re-snare-immune for a short window so
                // it escapes its captor Magnet before the field can reload it (see the Golden snare pass).
                if crab.slingshot_spent > 0.0 {
                    crab.slingshot_spent = (crab.slingshot_spent - dt).max(0.0);
                }

                // Whistle charm wears off after a beat or two, at which point the crab is fair
                // game for the panic contagion again.
                if crab.charm_timer > 0.0 {
                    crab.charm_timer = (crab.charm_timer - dt).max(0.0);
                }

                // A Dancer answering the player's Call keeps its answer for a few beats, then reverts
                // to normal (fleeing) behavior if it wasn't caught in time.
                if crab.answering_call > 0.0 {
                    crab.answering_call = (crab.answering_call - dt).max(0.0);
                }

                // Hermit host-swap: while shelled, the Hermit hunkers in place, then periodically
                // scurries to a new host spot in a short scripted dart — its signature "hides and
                // swaps hosts" restlessness that keeps it from being a stationary Armored reskin. The
                // dart is a quick directional burst (not sustained flee speed) followed by a reset of
                // its irregular timer. It never darts while lit (the player's beam is a truce — you
                // can't crack the shell but you can pin its position to line up a Stomp/Magnet play).
                if crab.is_shelled_hermit() {
                    crab.host_swap_timer -= dt;
                    if crab.host_swap_timer <= 0.0 && !crab_in_light {
                        // Scurry off at a random heading: a brief burst that carries it a short hop
                        // before the movement below damps it back to a hunker. `speed = 1.0` keeps the
                        // multiplier neutral so `vel` alone encodes the dart, matching the flee path.
                        let ang = rng.random_range(0.0_f32..std::f32::consts::TAU);
                        let dart = crab.crab_type.speed_range().start * 1.4;
                        crab.vel = Vec2::new(ang.cos(), ang.sin()) * dart;
                        crab.speed = 1.0;
                        crab.join_pulse = crab.join_pulse.max(0.6); // little squash-pop as it scuttles
                        crab.host_swap_timer = rng.random_range(1.6..3.2);
                    } else {
                        // Between darts it hunkers: bleed velocity so the shelled lump settles and
                        // holds ground rather than coasting on the last dart's momentum.
                        crab.vel *= 1.0 - (4.0 * dt).min(0.9);
                    }
                }

                // If player is within 150 pixels and crab is in the light, add a small extra speed boost
                let mut speed_multiplier = 1.0;
                if crab_in_light && distance_sq < 150.0 * 150.0 {
                    let distance = distance_sq.sqrt();
                    speed_multiplier = 2.0 - (distance / 150.0);
                    speed_multiplier = speed_multiplier.clamp(1.0, 2.0);
                }

                // Older crabs are faster so the player should catch them early.
                let age_boost = 1.0 + (crab.spawn_time / 10.0).min(1.5);
                crab.pos += crab.vel * crab.speed * speed_multiplier * age_boost * dt;

                // On-beat herd stampede: spend the surge armed by the downbeat (see the beat handler).
                // While surge_timer counts down, the crab DARTS an extra shove along its own heading
                // — a decaying burst that's strongest right on the "1" and eases to nothing before the
                // next bar, so the loose herd visibly lurches forward on the downbeat and glides
                // between beats. This makes the herd's *landing spot* a rhythm read: predict the surge,
                // slide into where the crabs will be on the bar. Not applied to a lit crab (it's already
                // steering to the player) so the beam read isn't disturbed. Decayed at ~4/sec so the
                // dart is spent within a beat at typical tempos; the shove scales with the crab's own
                // speed so fast crabs stampede farther, matching their base pace.
                if !crab_in_light && crab.surge_timer > 0.0 {
                    let heading = crab.vel.normalize_or_zero();
                    let mut dir = if heading == Vec2::ZERO {
                        Vec2::new(crab.facing_angle.cos(), crab.facing_angle.sin())
                    } else {
                        heading
                    };
                    // On-beat clump: a *calm free crab near the player* doesn't just step along its own
                    // heading on the "1" — it leans that beat-step toward you, so the loose herd visibly
                    // gathers around the train on the downbeat and the beat itself reads as an ambient
                    // routing tool. Deliberately WEAK and short-range: it only bends the surge direction
                    // (a gentle lean, not a pull toward you every frame), and it falls off past ~320px so
                    // it's herd texture near the train, not a field-wide stream. That keeps Groove Call (V)
                    // the real on-demand gather — V is a strong, timed, field-wide surge you press for;
                    // this is just the calm herd breathing toward you on the beat, strongest right next to
                    // you and fading to a plain own-heading step for crabs across the map.
                    const CLUMP_RADIUS: f32 = 320.0;
                    const CLUMP_RADIUS_SQ: f32 = CLUMP_RADIUS * CLUMP_RADIUS;
                    if distance_sq < CLUMP_RADIUS_SQ {
                        let to_player = (self.player_pos - crab.pos).normalize_or_zero();
                        if to_player != Vec2::ZERO {
                            // Lean fraction: up to ~0.45 right next to the player, easing to 0 at the
                            // radius edge — a bend, never a full redirect, so the crab still mostly keeps
                            // its own heading and the read stays "the herd drifts your way", not "warps in".
                            let distance = distance_sq.sqrt();
                            let lean = 0.45 * (1.0 - distance / CLUMP_RADIUS);
                            dir = (dir * (1.0 - lean) + to_player * lean).normalize_or_zero();
                        }
                    }
                    // Ease-out envelope: burst hardest at surge_timer≈1, fading to 0. The shove is a
                    // multiple of the crab's own base speed (crab.speed holds the real magnitude; vel
                    // is a unit heading), so at the peak the crab briefly moves ~3x its normal pace and
                    // eases back — the herd reads as *stepping* on the "1", fast crabs stepping farther,
                    // rather than a flat teleport. ~2.5x peak, decaying over the beat.
                    let envelope = crab.surge_timer * crab.surge_timer;
                    crab.pos += dir * crab.speed * 2.5 * envelope * dt;
                    crab.surge_timer = (crab.surge_timer - dt * 4.0).max(0.0);
                }

                // Tide Pool current: a free crab standing in one of the Water biome's native pools is
                // carried along a fixed drift heading — the pools ferry the loose herd downstream. A
                // gentle positional nudge (like the Magnet herd-pull), so it composes with flee/attract
                // rather than overriding: the flashlight still wins (a lit crab is heading to the player),
                // and a fleeing crab still bolts, just curving with the flow. This turns the pools into a
                // routing puzzle — position your train downstream and let the current deliver crabs to it.
                // `tide_current_pools` is empty on every non-Water biome, so this whole block short-
                // circuits to a single `!is_empty()` check outside the Tide Pools. Bosses are handled in
                // their own branch above and never reach here; caught crabs are gated out by `!crab.caught`.
                if !crab_in_light && !tide_current_pools.is_empty() {
                    let center = crab.pos + Vec2::splat(crab.scale / 2.0);
                    if tide_current_pools
                        .iter()
                        .any(|(c, r)| center.distance_squared(*c) < *r * *r)
                    {
                        // Positional drift only — the crab's facing is recomputed from its own
                        // velocity below (the flow streaks in the pool carry the direction cue), so we
                        // don't fight that here.
                        crab.pos += TIDE_CURRENT_DIR * TIDE_CURRENT_STRENGTH * dt;
                    }
                }

                // Neon Kelp funnel: a *fleeing* free crab inside one of the Kelp biome's native
                // weed patches gets channelled along a fixed lane heading, so the weeds catch a
                // panicking bolt and shepherd it sideways into a lane instead of letting it scatter.
                // Deliberately narrower than the Tide current (which sweeps every free crab): the
                // kelp only grabs a crab that's already fleeing, so it reads as "spook the herd near
                // the weeds and they funnel into a catchable lane" rather than an ambient drift. A
                // positional nudge (like the tide/Magnet pulls) so it composes with the bolt rather
                // than overriding it — the crab keeps fleeing, just curving along the lane; the beam
                // still wins (a lit crab is already gated out). `kelp_funnel_pools` is empty on every
                // non-Kelp biome, so this whole block short-circuits to one `!is_empty()` check.
                if crab.fleeing && !crab_in_light && !kelp_funnel_pools.is_empty() {
                    let center = crab.pos + Vec2::splat(crab.scale / 2.0);
                    if kelp_funnel_pools
                        .iter()
                        .any(|(c, r)| center.distance_squared(*c) < *r * *r)
                    {
                        crab.pos += KELP_FUNNEL_DIR * KELP_FUNNEL_STRENGTH * dt;
                    }
                }

                // Magnet pull: an ordinary free crab drifts toward the nearest roaming Magnet crab,
                // so the herd bunches up around Magnets. A gentle positional nudge (not a velocity
                // shove) that composes with the flee/attract behaviour above rather than overriding
                // it — the flashlight still wins (a crab in the beam is heading to the player), and a
                // fleeing crab still bolts, just curving a little toward the cluster. This is what
                // turns "catch the Magnet" into a two-for-one: the crabs it gathered come with it.
                // Squared-distance compare so the per-magnet scan (up to ~8% of the herd, times
                // every ordinary crab) does zero sqrt work until we've already found the winner
                // — a sqrt per pair here was the hottest unnecessary cost in this per-crab,
                // per-frame loop. Computed once per crab and shared below by both the ordinary
                // herd-nudge/Golden-snare check and the Thief-intercept check (a Thief is never
                // a Magnet or a boss, so this covers it too) instead of scanning
                // magnet_positions a second time for Thieves.
                let nearest_magnet: Option<(f32, Vec2)> =
                    if !crab_in_light && !crab.is_magnet() && !crab.is_boss() {
                        let mut nearest: Option<(f32, Vec2)> = None;
                        for &mp in magnet_positions.iter() {
                            let d2 = crab.pos.distance_squared(mp);
                            if d2 < MAGNET_RADIUS_SQ && d2 > 1.0 {
                                if nearest.map_or(true, |(bd2, _)| d2 < bd2) {
                                    nearest = Some((d2, mp));
                                }
                            }
                        }
                        nearest
                    } else {
                        None
                    };
                if !crab_in_light && !crab.is_magnet() && !crab.is_boss() {
                    if let Some((d2, mp)) = nearest_magnet {
                        // Stronger tug up close, fading to nothing at the edge of the pull radius.
                        let d = d2.sqrt();
                        let prox = 1.0 - d / MAGNET_RADIUS; // 0 at the edge, 1 at the magnet
                        let dir = (mp - crab.pos).normalize_or_zero();
                        // Emergent crossover: a roaming Magnet snares a fleeing Golden. The shiny
                        // prize normally bolts too fast to catch by hand, but a lodestone's field
                        // overpowers even that skittish sprint once the Golden strays deep into it
                        // (inner ~60% of the radius). While snared the Golden is dragged hard toward
                        // the Magnet and its bolt is damped, so herding the prize toward a wandering
                        // Magnet becomes a real way to trap it — the Magnet as accidental savior,
                        // the mirror of the Magnet-pry-Thief save. Outside the deep zone it just
                        // gets the ordinary gentle nudge like any other crab.
                        if crab.is_golden() && prox > 0.4 && crab.slingshot_spent <= 0.0 {
                            // Overpowering drag: far stronger than the herd nudge, scaling up as it
                            // sinks deeper so the snare tightens the closer it gets. A Golden just fired
                            // by a Tide Boss slingshot (slingshot_spent > 0) is immune to re-snare for a
                            // beat or two so it actually clears the field instead of reloading in place.
                            let snare_pull = (prox - 0.4) / 0.6 * 260.0;
                            crab.pos += dir * snare_pull * dt;
                            // Damp the Golden's bolt so it can't just sprint back out of the field.
                            crab.vel *= 1.0 - (0.85 * dt).min(0.5);
                            // First frame of the snare fires a celebratory pop; refresh the tether
                            // window each frame it stays deep so the visual/slow persists smoothly.
                            if crab.magnet_snared <= 0.0 {
                                golden_snare_pops.push(crab.pos);
                            }
                            crab.magnet_snared = 0.25;
                        } else if crab.is_shelled_hermit() {
                            // Signature Hermit edge — a roaming Magnet's field RIPS the borrowed shell
                            // clean out. Unlike the Armored crab (which only wears down slowly under a
                            // *charged* Magnet's vacuum), an ordinary Magnet cracks a Hermit the moment
                            // it drags it deep into the field: the lodestone yanks the crab so hard the
                            // shell tears off. This is *the* new crossover the Hermit exists for — the
                            // beam can't touch its shell, but a Magnet you've parked in its path pops it.
                            //
                            // A shelled Hermit is heavy but the Magnet overpowers it: it gets a firm drag
                            // from anywhere in the field (stronger than the 34-unit herd nudge, scaling
                            // with depth) so a hunkered Hermit actually slides into the lodestone rather
                            // than the weak nudge failing to reach it — then the deep zone rips the shell.
                            let drag = (0.4 + prox * 1.6) * 90.0;
                            crab.pos += dir * drag * dt;
                            if prox > 0.45 {
                                let before = crab.boss_health;
                                // ~5 shell/sec at the core — a full 2.0 shell rips in well under half a
                                // second once it's deep, so the Magnet reads as a decisive cracker, not
                                // the slow grind the Armored gets. Reuses the Armored grind pop/visual.
                                crab.boss_health = (crab.boss_health - 5.0 * dt).max(0.0);
                                let broke = crab.boss_health <= 0.0;
                                let step = crab.crab_type.initial_shell().max(0.001) / 2.0;
                                if broke
                                    || (before / step).floor() != (crab.boss_health / step).floor()
                                {
                                    magnet_grind.push((crab.pos, broke, crab.is_hermit()));
                                }
                                if broke {
                                    crab.join_pulse = 1.0; // pop out of the shell with a squash-and-flee
                                }
                            }
                        } else {
                            let pull = prox * 34.0;
                            crab.pos += dir * pull * dt;
                        }
                    }
                }

                // Emergent crossover — a snared Golden supercharges its captor Magnet into a herd
                // vacuum. When a Magnet is pinning a Golden (see the snare pass just above), the
                // prize's shine energizes the lodestone: it now reaches the surrounding loose herd
                // over a wider radius and hauls them in harder than the plain herd-nudge does, so
                // the trapped Golden and the crabs balling up around it become one tight cluster you
                // can sweep with a single beam pass. Only applies to ordinary crabs the *normal*
                // field didn't already grab this frame — a Golden being snared, a crab already
                // caught, or one deep in a Magnet's own radius keeps its existing behaviour; this is
                // purely the extra outer reach the charge buys. Runs off the tiny charged-Magnet
                // snapshot, so almost always over an empty list.
                if !crab_in_light
                    && !crab.is_magnet()
                    && !crab.is_boss()
                    && !charged_magnet_positions.is_empty()
                    && crab.magnet_snared <= 0.0
                {
                    let mut nearest: Option<(f32, Vec2)> = None;
                    for &cmp in charged_magnet_positions.iter() {
                        let d2 = crab.pos.distance_squared(cmp);
                        if d2 < CHARGED_MAGNET_RADIUS_SQ
                            && d2 > 1.0
                            && nearest.map_or(true, |(bd2, _)| d2 < bd2)
                        {
                            nearest = Some((d2, cmp));
                        }
                    }
                    if let Some((d2, cmp)) = nearest {
                        // Strongest at the core, fading to nothing at the widened edge. A firmer
                        // tug than the plain herd-nudge (its 34.0) so the vacuum visibly balls the
                        // herd up while the charge lasts.
                        let prox = 1.0 - d2.sqrt() / CHARGED_MAGNET_RADIUS;
                        let dir = (cmp - crab.pos).normalize_or_zero();
                        crab.pos += dir * (prox * 68.0) * dt;

                        // Emergent crossover — a charged Magnet's vacuum grinds an Armored shell.
                        // The same widened field that balls the loose herd up also drags an Armored
                        // crab against the lodestone hard enough to wear its shell down over time —
                        // so a Golden-supercharged (or Dancer-thumped) Magnet slowly cracks open any
                        // hard-shell it hauls in, softening a stomp-only target you can then finish
                        // with the beam. A three-archetype collision: the Golden/Dancer that charged
                        // the Magnet, the Magnet's vacuum, and the Armored crab caught in its reach.
                        // Reuses the charged-field snapshot and the shell HP the Stomp already wears
                        // down — no new field, just a second thing the charge is worth. Grinds only
                        // near the core (where the drag is strongest), so an Armored crab clipping the
                        // outer edge just gets balled up like the rest.
                        if crab.is_armored() && crab.boss_health > 0.0 && prox > 0.45 {
                            let before = crab.boss_health;
                            // ~3 shell/sec at the core, tapering to nothing by prox 0.45. A full
                            // shell takes a couple seconds of being pinned in the vacuum to open.
                            let grind = (prox - 0.45) / 0.55 * 3.0;
                            crab.boss_health = (crab.boss_health - grind * dt).max(0.0);
                            crab.join_pulse = crab.join_pulse.max(0.4); // faint shudder as it's ground
                            let broke = crab.boss_health <= 0.0;
                            // One chip pop per ~third of the shell worn (or the final crack), so the
                            // grind reads as steady progress without spamming a pop every frame.
                            let step = crab.crab_type.initial_shell().max(0.001) / 3.0;
                            if broke || (before / step).floor() != (crab.boss_health / step).floor()
                            {
                                magnet_grind.push((crab.pos, broke, false)); // Armored, never a Hermit
                            }
                        }
                    }
                }

                // Emergent crossover — the Golden lures the Magnet off its cluster. A roaming Magnet
                // that isn't itself being beamed drifts toward the nearest free, fleeing Golden it can
                // sense: the shiny prize's shine catches the lodestone's attention and pulls it away
                // from the herd it was gathering. This is the mirror of the Magnet-snares-Golden pass
                // above — there the Magnet traps the Golden; here the Golden tugs the Magnet — and it
                // adds a real routing wrinkle: a Magnet you were steering toward your train can go
                // wandering after a Golden, either concentrating the two prizes together (good) or
                // abandoning the cluster you were building (bad). Skipped once the Golden is deep in
                // the Magnet's own field, since the snare pass then takes over and pins it. Uses the
                // Goldens snapshotted before the loop, so no nested borrow.
                if crab.is_magnet() && !crab_in_light && !golden_lure_positions.is_empty() {
                    let mut nearest: Option<(f32, Vec2)> = None;
                    for &gp in golden_lure_positions.iter() {
                        let d2 = crab.pos.distance_squared(gp);
                        // Only chase Goldens that are within lure range but not already inside the
                        // Magnet's own pull radius — once it's that close the snare handles it.
                        if d2 < MAGNET_LURE_RADIUS_SQ && d2 > MAGNET_RADIUS_SQ * 0.36 {
                            if nearest.map_or(true, |(bd2, _)| d2 < bd2) {
                                nearest = Some((d2, gp));
                            }
                        }
                    }
                    if let Some((d2, gp)) = nearest {
                        let d = d2.sqrt();
                        // Stronger tug the closer the prize, fading out at the edge of lure range.
                        let prox = 1.0 - d / MAGNET_LURE_RADIUS; // 0 at edge, ~1 up close
                        let dir = (gp - crab.pos).normalize_or_zero();
                        crab.vel = crab.vel.lerp(dir * crab.crab_type.speed_range().end, 0.05);
                        crab.speed = 1.0;
                        crab.pos += dir * (prox * 30.0) * dt; // small positional nudge on top of the steer
                        if crab.magnet_lured <= 0.0 {
                            magnet_lure_pops.push(crab.pos);
                        }
                        crab.magnet_lured = 0.3; // refreshed each frame the chase holds
                    }
                }
                // The lure fades the instant a Magnet stops chasing (no Golden in range), so the
                // gold-tinted aura only shows while it's actually drifting after a prize.
                if crab.magnet_lured > 0.0 {
                    crab.magnet_lured = (crab.magnet_lured - dt).max(0.0);
                }

                // Flag this Magnet as charged if it's one of the ones pinning a snared Golden this
                // frame (positions were snapshotted just before the loop and nothing has moved a
                // Magnet since, so exact position match is safe). Refresh a short window so the
                // supercharged aura holds smoothly while it keeps the prize, then decays once the
                // Golden slips free or gets caught.
                if crab.is_magnet() {
                    // Only a Golden-pin (the first golden_charged_count entries) tops the charge up
                    // each frame; a Dancer-thumped surge is past that split and must decay on its own
                    // so the pull surge is a brief on-beat flare, not a permanent field.
                    if charged_magnet_positions[..golden_charged_count].contains(&crab.pos) {
                        crab.magnet_charged = 0.2;
                    } else if crab.magnet_charged > 0.0 {
                        crab.magnet_charged = (crab.magnet_charged - dt).max(0.0);
                    }
                }

                // Thief homing: a free Thief that isn't in the beam (being caught) or charmed
                // (whistled off) steers hard toward the conga tail so it can latch on and start
                // peeling links. Only the tail — never the head — so it always attacks the exposed
                // end. Once latched (latch_timer > 0) steal_chain_thief pins it to the tail, so we
                // stop steering here to avoid fighting that.
                if crab.is_thief()
                    && !crab_in_light
                    && crab.charm_timer <= 0.0
                    && crab.latch_timer <= 0.0
                {
                    // Emergent crossover: a roaming Magnet intercepts a homing Thief. Before the
                    // Thief reaches your tail to latch, if it strays deep into a Magnet's field the
                    // lodestone overpowers its beeline and hauls it into the cluster — so parking a
                    // Magnet between your train and an incoming Thief becomes a defensive routing
                    // play, the pre-latch mirror of the Magnet-pry that rips an already-latched
                    // Thief off. Reuses the same deep-field test as the Golden snare — and the
                    // same nearest-magnet lookup computed just above, instead of re-scanning
                    // magnet_positions a second time for every free Thief.
                    let mut intercepted = false;
                    if let Some((d2, mp)) = nearest_magnet {
                        let prox = 1.0 - d2.sqrt() / MAGNET_RADIUS; // 0 at edge, 1 at magnet
                        if prox > 0.4 {
                            let dir = (mp - crab.pos).normalize_or_zero();
                            // Overpowering drag toward the lodestone, tightening as it sinks in.
                            let pull = (prox - 0.4) / 0.6 * 240.0;
                            crab.pos += dir * pull * dt;
                            crab.vel *= 1.0 - (0.85 * dt).min(0.5); // kill its homing momentum
                            if crab.magnet_snared <= 0.0 {
                                thief_snare_pops.push(crab.pos);
                            }
                            crab.magnet_snared = 0.25; // refreshed each frame it stays snared
                            intercepted = true;
                        }
                    }
                    // Emergent crossover: a fleeing Golden lures a homing Thief off your tail. A
                    // thief can't resist a shiny thing — if a free Golden is nearer than the tail
                    // (and inside lure range), its shine overpowers the raider's beeline and it
                    // chases the prize instead of your train. The mirror of the Golden-lures-Magnet
                    // pass above: there gold tugs the lodestone, here gold tugs the raider. It turns
                    // a fleeing Golden into an accidental decoy — a real relief for a train under
                    // raid — but if the Thief catches the shine it just parks a threat right on the
                    // prize you were chasing. Magnet interception still wins (that's a physical drag,
                    // this is only attention), so it only runs when not intercepted. Reuses the
                    // golden_lure_positions snapshot already built for the Magnet lure — no new scan.
                    let mut lured = false;
                    if !intercepted && !golden_lure_positions.is_empty() {
                        const THIEF_LURE_RADIUS: f32 = 260.0;
                        const THIEF_LURE_RADIUS_SQ: f32 = THIEF_LURE_RADIUS * THIEF_LURE_RADIUS;
                        // Only divert to a Golden that's genuinely closer than the tail it's homing
                        // for — a shine across the arena shouldn't pull it off a tail right beside it.
                        let tail_d2 = thief_tail_pos
                            .map_or(f32::INFINITY, |tp| crab.pos.distance_squared(tp));
                        let mut nearest: Option<(f32, Vec2)> = None;
                        for &gp in golden_lure_positions.iter() {
                            let d2 = crab.pos.distance_squared(gp);
                            if d2 < THIEF_LURE_RADIUS_SQ
                                && d2 < tail_d2
                                && nearest.map_or(true, |(bd2, _)| d2 < bd2)
                            {
                                nearest = Some((d2, gp));
                            }
                        }
                        if let Some((d2, gp)) = nearest {
                            let d = d2.sqrt();
                            // Stronger tug the closer the prize; leans hard so the divert reads as
                            // the Thief abandoning the raid, not just wobbling toward the shine.
                            let prox = 1.0 - d / THIEF_LURE_RADIUS; // 0 at edge, ~1 up close
                            let dir = (gp - crab.pos).normalize_or_zero();
                            let chase_speed = crab.crab_type.speed_range().end * 1.3;
                            crab.vel = crab.vel.lerp(dir * chase_speed, 0.10 + prox * 0.10);
                            crab.speed = 1.0;
                            if crab.thief_lured <= 0.0 {
                                thief_lure_pops.push(crab.pos);
                            }
                            crab.thief_lured = 0.3; // refreshed each frame the divert holds
                            lured = true;
                        }
                    }
                    // The lure fades the instant the Thief loses its shiny target, so the gold-tinted
                    // aura only shows while it's actually being pulled off the raid.
                    if crab.thief_lured > 0.0 {
                        crab.thief_lured = (crab.thief_lured - dt).max(0.0);
                    }

                    if !intercepted && !lured {
                        if let Some(tp) = thief_tail_pos {
                            let dir = (tp - crab.pos).normalize_or_zero();
                            // Drive it in at a good clip so a Thief spawning across the arena still
                            // reaches your tail while the train is worth stealing from.
                            let home_speed = crab.crab_type.speed_range().end * 1.4;
                            crab.vel = crab.vel.lerp(dir * home_speed, 0.08);
                            crab.speed = 1.0;
                        }
                    }
                }

                // Beat-synced positional wobble for idle (non-spooked) crabs.
                if crab.spooked_timer == 0.0 {
                    let beat_phase = (1.0 - self.beat_timer / self.beat_interval)
                        * std::f32::consts::TAU
                        + crab.beat_phase_offset;
                    let perp = Vec2::new(-crab.vel.y, crab.vel.x).normalize_or_zero();
                    crab.pos += perp * 10.0 * beat_phase.sin() * dt;
                }

                // Bounce off walls.
                let (width, height) = area;
                if crab.pos.x < 0.0 || crab.pos.x > width - crab.scale {
                    crab.vel.x = -crab.vel.x;
                    crab.pos.x = crab.pos.x.clamp(0.0, width - crab.scale);
                }
                if crab.pos.y < 0.0 || crab.pos.y > height - crab.scale {
                    crab.vel.y = -crab.vel.y;
                    crab.pos.y = crab.pos.y.clamp(0.0, height - crab.scale);
                }

                // Universal speed cap — clamp vel so no compounding force (bounces, scatter
                // kicks, lasso drag) can push a crab to visually broken teleport speeds.
                // vel may carry full speed (crab.speed==1) or be a unit heading (speed in
                // crab.speed); clamp the effective combined magnitude in both cases.
                let effective_speed = crab.vel.length() * crab.speed;
                if effective_speed > MAX_CRAB_SPEED {
                    let scale = MAX_CRAB_SPEED / effective_speed;
                    crab.vel *= scale;
                    // crab.speed is left alone — it's a baseline the AI uses for decisions;
                    // only the instantaneous vel magnitude is capped.
                }

                // Smoothly rotate crab to face its movement direction
                let speed = crab.vel.length();
                if speed > 5.0 {
                    let target_angle = crab.vel.y.atan2(crab.vel.x);
                    let mut delta = target_angle - crab.facing_angle;
                    while delta > std::f32::consts::PI {
                        delta -= std::f32::consts::TAU;
                    }
                    while delta < -std::f32::consts::PI {
                        delta += std::f32::consts::TAU;
                    }
                    crab.facing_angle += delta * (dt * 8.0).min(1.0);
                }

                // Searing sparks — ONLY for a shelled target the beam is actively burning down
                // (drain is live). Not a herd attraction cue anymore: normal crabs in the cone emit
                // nothing. The sparks spray OUTWARD off the scorched shell (not toward the player)
                // and burn harsh white-hot, so the read is "this thing is melting under the beam",
                // reinforcing the flashlight's boss-pressure identity.
                let searing = crab_in_light && crab.boss_health > 0.0 && !crab.is_hermit();
                if searing {
                    // ~14 sparks per second while burning — a dense, unmistakable scorch spray.
                    if rng.random_range(0.0_f32..1.0_f32) < dt * 14.0 {
                        // Spray back along the beam (away from the player, off the hit face).
                        let off_beam = (crab.pos - self.player_pos).normalize_or_zero();
                        let perp = Vec2::new(-off_beam.y, off_beam.x);
                        let spread = rng.random_range(-0.9_f32..0.9_f32);
                        let dir = (off_beam + perp * spread).normalize_or_zero();
                        let speed = rng.random_range(90.0_f32..190.0_f32);
                        let life = rng.random_range(0.25_f32..0.5_f32);
                        // Harsh white-yellow scorch, occasionally flaring to orange ember.
                        let hot = rng.random_range(0.0_f32..1.0_f32) < 0.35;
                        let color = if hot {
                            [1.0, 0.55, 0.15]
                        } else {
                            [1.0, 0.95, 0.7]
                        };
                        attraction_particles.push((crab.pos, dir * speed, life, color));
                    }
                }
            }
        }

        // Sync the Reef DJ phrase state after the &mut self.crabs loop. reef_active gates the phrase
        // roll and HUD telegraph; clearing it when the DJ leaves the field wipes any stale phrase so
        // the next DJ starts fresh. A landed hot-beat hit kicks a juice bloom + a little flash.
        self.reef_active = reef_on_field;
        if !reef_on_field {
            self.reef_phrase = [false; 4];
            self.reef_phrase_bar = u32::MAX;
            self.reef_dancer_timer = 0.0;
        } else if reef_hit_landed {
            self.reef_hit_flash = 1.0;
            self.on_beat_flash = self.on_beat_flash.max(0.3);
        }

        // Reef DJ backup dancers. The boss clears the herd for a clean duel, so bring one archetype
        // back into the arena as a fight mechanic: the DJ summons "hype Dancers" on a timer. They
        // drift and hop on the beat like any Dancer, but catching one *on a called (hot) beat* chips
        // the boss shell (see the catch loop), so herding them onto the phrase is an active second
        // way to crack the DJ beyond just holding light. Cap how many are loose so the duel stays
        // legible — a couple to chase, not a swarm — and only summon while the DJ still has shell.
        if reef_on_field {
            self.reef_dancer_timer -= dt;
            if self.reef_dancer_timer <= 0.0 {
                let loose_dancers = self
                    .crabs
                    .iter()
                    .filter(|c| !c.caught && !c.is_boss() && c.is_dancer())
                    .count();
                if loose_dancers < 3 {
                    let mut rng = crate::rng::rng();
                    let dancer = spawn_hype_dancer(
                        (self.world_width, self.world_height),
                        reef_boss_pos,
                        &mut rng,
                    );
                    let dpos = dancer.pos;
                    self.crabs.push(dancer);
                    // Little violet summon puff so the dancer reads as the DJ's call, not a stray.
                    self.particle_system
                        .spawn_milestone_fireworks(dpos, 5, &mut rng);
                }
                self.reef_dancer_timer = 3.0;
            }
        }

        // Push sparkle particles for attracted crabs (done outside loop to avoid borrow conflict).
        // One rng per batch rather than one per particle — crate::rng::rng() re-seeds on every call
        // and the flashlight can accumulate many attracted crabs at once.
        if !attraction_particles.is_empty() {
            let mut rng = crate::rng::rng();
            for &(pos, vel, life, [cr, cg, cb]) in attraction_particles.iter() {
                self.particle_system.push(crate::graphics::Particle {
                    pos,
                    vel,
                    life,
                    max_life: life,
                    size: rng.random_range(1.5_f32..3.5_f32),
                    color: [
                        (cr * 0.6 + 0.4).min(1.0),
                        (cg * 0.6 + 0.4).min(1.0),
                        (cb * 0.6 + 0.4).min(1.0),
                    ],
                });
            }
        }

        // Celebrate any King Crab worn down to catchable this frame
        // Hermit King escape: it won the race to the world edge and dragged a fresh shell-house
        // stack back in — announce the reset so the player knows the crack progress is gone.
        for &pos in hermit_king_reshells.iter() {
            // Banner text anchors to the player so the message is readable on screen (the escape
            // happens at the world edge, often off-camera); the shockwave fires at the actual spot.
            self.floating_texts.spawn(
                "THE HERMIT KING RE-SHELLS!".to_string(),
                self.player_pos + Vec2::new(-230.0, -190.0),
                40.0,
                [0.95, 0.6, 0.25, 1.0],
            );
            self.floating_texts.spawn(
                "It escaped — crack it faster next time!".to_string(),
                self.player_pos + Vec2::new(-190.0, -145.0),
                24.0,
                [1.0, 0.9, 0.7, 0.9],
            );
            self.spawn_catch_shockwave(pos, [0.85, 0.5, 0.2]);
            self.screen_shake = self.screen_shake.max(12.0);
        }

        for &pos in boss_broke.iter() {
            self.floating_texts.spawn(
                "SHELL CRACKED!".to_string(),
                pos - Vec2::new(96.0, 60.0),
                40.0,
                [1.0, 0.95, 0.6, 1.0],
            );
            self.floating_texts.spawn(
                "CATCH IT!".to_string(),
                pos - Vec2::new(64.0, 20.0),
                30.0,
                [0.4, 1.0, 0.5, 1.0],
            );
            // Telegraphed pop: the shell finally gives under sustained beam pressure. A hot burst of
            // scorch sparks off the crack, a double shockwave (white flash + hot ring), a hard shake,
            // and a brief hitstop that STAGGERS the moment so it lands as a satisfying "pop" beat
            // rather than a health number quietly hitting zero.
            self.spawn_catch_shockwave(pos, [1.0, 0.98, 0.85]);
            self.spawn_catch_shockwave(pos, [1.0, 0.6, 0.15]);
            self.particle_system
                .spawn_milestone_fireworks(pos, 14, &mut crate::rng::rng());
            self.screen_shake = self.screen_shake.max(22.0);
            let a = crate::rng::rng().random_range(0.0_f32..std::f32::consts::TAU);
            self.screen_shake_vel = Vec2::new(a.cos(), a.sin()) * 16.0 * 60.0;
            self.on_beat_flash = self.on_beat_flash.max(0.6);
            // Freeze-frame the crack — a strong hitstop so a boss shell breaking is the single most
            // emphatic pop the beam can produce.
            self.hitstop_timer = self.hitstop_timer.max(0.16);
        }

        // A boss just crossed into its enrage phase — the fight's final act. A hard jolt, a big
        // menacing shockwave in the boss's own color, and an "ENRAGED!" shout mark the turn so the
        // ramp in aggression reads as a deliberate escalation, not random difficulty.
        for &(pos, is_tide) in boss_enrages.iter() {
            let (ring_col, txt_col): ([f32; 3], [f32; 4]) = if is_tide {
                ([0.3, 0.75, 1.0], [0.5, 0.9, 1.0, 1.0])
            } else {
                ([1.0, 0.4, 0.15], [1.0, 0.55, 0.2, 1.0])
            };
            self.floating_texts.spawn(
                "ENRAGED!".to_string(),
                pos - Vec2::new(72.0, 58.0),
                42.0,
                txt_col,
            );
            self.spawn_catch_shockwave(pos, ring_col);
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((pos, 0.0));
            }
            self.particle_system
                .spawn_milestone_fireworks(pos, 10, &mut crate::rng::rng());
            self.screen_shake = self.screen_shake.max(20.0);
            let a = crate::rng::rng().random_range(0.0_f32..std::f32::consts::TAU);
            self.screen_shake_vel = Vec2::new(a.cos(), a.sin()) * 20.0 * 60.0;
            self.on_beat_flash = self.on_beat_flash.max(0.5);

            // Arena-shifting enrage: the boss doesn't just get angrier, it reshapes the duel space
            // for its final act. A King Crab cracks the floor into hazard fissures to weave around;
            // a Tide Boss floods the arena with extra wade-drag pools so routing changes mid-fight.
            if is_tide {
                self.flood_arena(pos);
            } else {
                self.crack_arena_fissures(pos);
            }
        }

        // King Crab winding up a charge: red alarm ring + shouted warning so the player has time
        // to route the tail out of the lane before the lunge commits.
        for &pos in boss_windups.iter() {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((pos, 0.0));
            }
            self.floating_texts.spawn(
                "CHARGE INCOMING!".to_string(),
                pos - Vec2::new(96.0, 52.0),
                30.0,
                [1.0, 0.45, 0.2, 1.0],
            );
            self.on_beat_flash = self.on_beat_flash.max(0.25);
        }

        // The lunge fires: a jolt and a hot shockwave sell the commitment.
        for &pos in boss_launches.iter() {
            self.spawn_catch_shockwave(pos, [1.0, 0.5, 0.2]);
            self.screen_shake = self.screen_shake.max(10.0);
            let kick_angle = crate::rng::rng().random_range(0.0_f32..std::f32::consts::TAU);
            self.screen_shake_vel = Vec2::new(kick_angle.cos(), kick_angle.sin()) * 8.0 * 60.0;
        }

        // Emergent crossover feedback: a charging King Crab just rammed a free Armored crab's shell.
        // The wall held — the boss's lunge is spent and the tail it was aimed at is spared. Sell it
        // as a hard impact (shell-clang shockwave in Armored slate-blue, a jolt, a proud "BLOCKED!"
        // callout) and shove the shell crab back off the boss so the collision reads physically.
        for &(boss_pos, shell_pos) in boss_blocks.iter() {
            let knock_dir = (shell_pos - boss_pos).normalize_or_zero();
            let knock_dir = if knock_dir == Vec2::ZERO {
                Vec2::new(0.0, -1.0)
            } else {
                knock_dir
            };
            for crab in self.crabs.iter_mut() {
                if crab.is_armored() && !crab.caught && crab.pos.distance(shell_pos) < 1.0 {
                    // Knock the shell crab back along the charge line — a solid shove, not a panic
                    // flee: Armored stays calm (it's a wall), it just gets bumped.
                    crab.vel = knock_dir * crab.crab_type.speed_range().end * 1.8;
                    crab.speed = 1.0;
                    break;
                }
            }
            self.spawn_catch_shockwave(shell_pos, [0.55, 0.62, 0.72]); // Armored slate-blue clang
            self.floating_texts.spawn(
                "BLOCKED!".to_string(),
                shell_pos - Vec2::new(40.0, 40.0),
                30.0,
                [0.7, 0.82, 0.95, 1.0],
            );
            self.screen_shake = self.screen_shake.max(8.0);
            let kick_angle = crate::rng::rng().random_range(0.0_f32..std::f32::consts::TAU);
            self.screen_shake_vel = Vec2::new(kick_angle.cos(), kick_angle.sin()) * 7.0 * 60.0;
        }

        // Feedback for a King Crab dazed by the shell ram above: a woozy callout on top of the
        // BLOCKED! pop, so the stun window (see stun_timer/is_stunned in enemies.rs) reads as a
        // real payoff moment, not a silent state flip.
        for &pos in boss_stuns.iter() {
            self.floating_texts.spawn(
                "DAZED!".to_string(),
                pos - Vec2::new(36.0, 70.0),
                26.0,
                [1.0, 0.9, 0.4, 1.0],
            );
        }

        // Tide Boss starting to swell a pulse: a cold warning ring + shout so the player can pull
        // the train back out of range before the shockwave lands.
        for &pos in tide_swells.iter() {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((pos, 0.0));
            }
            self.floating_texts.spawn(
                "TIDE SURGE — BACK AWAY!".to_string(),
                pos - Vec2::new(130.0, 52.0),
                30.0,
                [0.4, 0.85, 1.0, 1.0],
            );
            self.on_beat_flash = self.on_beat_flash.max(0.25);
        }

        // The pulse fires: spawn the expanding shockwave, scatter nearby free crabs, and knock the
        // train's tail loose if it's clustered too close.
        for &center in tide_fires.iter() {
            self.tide_pulse_burst(center);
        }

        // Dust kicked up behind the charging boss — sprayed opposite the lunge heading.
        {
            let mut rng = crate::rng::rng();
            for &(pos, vel) in boss_charge_dust.iter() {
                if rng.random_range(0.0_f32..1.0_f32) >= dt * 90.0 {
                    continue; // throttle so a long lunge doesn't flood the particle pool
                }
                let back = (-vel).normalize_or_zero();
                let perp = Vec2::new(-back.y, back.x);
                let spread = rng.random_range(-0.5_f32..0.5_f32);
                let dir = (back + perp * spread).normalize_or_zero();
                let speed = rng.random_range(50.0_f32..140.0_f32);
                let life = rng.random_range(0.3_f32..0.6_f32);
                self.particle_system.push(crate::graphics::Particle {
                    pos,
                    vel: dir * speed,
                    life,
                    max_life: life,
                    size: rng.random_range(2.0_f32..4.5_f32),
                    color: [0.85, 0.7, 0.5],
                });
            }
        }

        // Armored shells the beam just wore through — a lighter "crack" than the boss fanfare, but
        // still a scorch pop: a hot-tinted shockwave, a short shake and a light hitstop so burning a
        // shell open with the beam reads as a satisfying break rather than a silent state flip.
        for &pos in armor_broke.iter() {
            self.floating_texts.spawn(
                "SHELL CRACKED!".to_string(),
                pos - Vec2::new(70.0, 40.0),
                26.0,
                [1.0, 0.92, 0.6, 1.0],
            );
            self.spawn_catch_shockwave(pos, [1.0, 0.8, 0.35]);
            self.screen_shake = self.screen_shake.max(9.0);
            self.hitstop_timer = self.hitstop_timer.max(0.06);
        }

        // Emit "!" floating texts for crabs that just started fleeing this frame
        for &pos in flee_pops.iter() {
            self.floating_texts.spawn(
                "!".to_string(),
                pos - Vec2::new(0.0, 24.0),
                28.0,
                [1.0, 0.9, 0.1, 1.0],
            );
        }

        // Celebrate any Golden a Magnet just snared this frame — a bright gold-into-magnet-orange
        // pop and a shockwave so "the Magnet trapped the prize" reads as a moment, the same way the
        // Magnet-pry-Thief save does.
        for pos in golden_snare_pops.drain(..) {
            self.floating_texts.spawn(
                "SNARED!".to_string(),
                pos - Vec2::new(0.0, 30.0),
                26.0,
                [1.0, 0.7, 0.2, 1.0], // Magnet's lodestone orange claiming the golden prize
            );
            self.spawn_catch_shockwave(pos, [1.0, 0.78, 0.25]);
        }

        // Celebrate any homing Thief a Magnet just intercepted this frame — a green-into-magnet-
        // orange pop and a shockwave so "the Magnet caught the raider before it reached your tail"
        // reads as the defensive save it is, mirroring the Golden snare's callout.
        for pos in thief_snare_pops.drain(..) {
            self.floating_texts.spawn(
                "INTERCEPTED!".to_string(),
                pos - Vec2::new(0.0, 30.0),
                24.0,
                [0.55, 0.9, 0.4, 1.0], // Thief's poison-green pulled into the Magnet's field
            );
            self.spawn_catch_shockwave(pos, [0.7, 0.85, 0.35]);
        }

        // Note when a Magnet first breaks off after a Golden — a small gold-orange callout so the
        // lure reads as a moment ("the prize pulled the lodestone off your herd") rather than the
        // Magnet silently wandering. Gentler than the snare/intercept saves (no shockwave): this is
        // a wrinkle in routing, not a rescue, and firing a big burst every time a Golden drifts past
        // a Magnet would be noisy.
        for pos in magnet_lure_pops.drain(..) {
            self.floating_texts.spawn(
                "LURED!".to_string(),
                pos - Vec2::new(0.0, 30.0),
                22.0,
                [1.0, 0.8, 0.35, 1.0], // gold prize bleeding into the Magnet's lodestone orange
            );
        }

        // Note when a fleeing Golden first pulls a homing Thief off your tail — a small green-into-
        // gold callout so the relief reads as a moment ("the shine drew the raider off your train")
        // rather than the Thief silently wandering. Gentler than the Magnet saves (no shockwave):
        // like the Magnet lure, it's a routing wrinkle, not a rescue, and the Golden decoy is
        // accidental, so a big burst every time would be noisy.
        for pos in thief_lure_pops.drain(..) {
            self.floating_texts.spawn(
                "SHINY!".to_string(),
                pos - Vec2::new(0.0, 30.0),
                22.0,
                [0.7, 0.95, 0.4, 1.0], // Thief's poison-green catching the golden gleam
            );
        }

        // Note when a charged Magnet's vacuum grinds an Armored shell — same CHIPPED!/SHELL CRACKED!
        // cues as the Dancer-chip and Stomp crack so the shell-progress language stays consistent,
        // but tinted the Magnet's lodestone orange so the "the charged pull did this" story reads.
        for (pos, broke, was_hermit) in magnet_grind.drain(..) {
            // A charged Magnet ripping a Hermit clean out fires the signature copper Hermit-pop — a
            // pure archetype-web crack (the beam can't do it), so it earns its own watchable beat.
            if broke && was_hermit {
                self.spawn_hermit_pop(pos);
                continue;
            }
            let (label, burst) = if broke {
                ("SHELL CRACKED!", [0.7, 0.8, 0.95]) // fully open — matches the Stomp/Dancer crack cue
            } else {
                ("CHIPPED!", [0.62, 0.68, 0.78]) // a chink ground loose, more shell to go
            };
            self.floating_texts.spawn(
                label.to_string(),
                pos - Vec2::new(52.0, 30.0),
                24.0,
                [1.0, 0.7, 0.3, 1.0], // Magnet's lodestone orange so the source reads at a glance
            );
            self.spawn_catch_shockwave(pos, burst);
        }

        // Hand the scratch buffers back so next frame's std::mem::take reuses this frame's
        // allocation instead of starting from an empty Vec.
        self.magnet_grind_buf = magnet_grind;
        self.flee_pops_buf = flee_pops;
        self.golden_snare_pops_buf = golden_snare_pops;
        self.thief_snare_pops_buf = thief_snare_pops;
        self.magnet_lure_pops_buf = magnet_lure_pops;
        self.thief_lure_pops_buf = thief_lure_pops;
        self.boss_broke_buf = boss_broke;
        self.armor_broke_buf = armor_broke;
        self.attraction_particles_buf = attraction_particles;
        self.boss_windups_buf = boss_windups;
        self.boss_launches_buf = boss_launches;
        self.boss_charge_dust_buf = boss_charge_dust;
        self.boss_enrages_buf = boss_enrages;
        self.tide_fires_buf = tide_fires;
        self.tide_swells_buf = tide_swells;
        self.magnet_positions_buf = magnet_positions;
        self.golden_lure_positions_buf = golden_lure_positions;
        self.charged_magnet_positions_buf = charged_magnet_positions;
        self.armored_positions_buf = armored_positions;
        self.boss_blocks_buf = boss_blocks;
        self.boss_stuns_buf = boss_stuns;

        // Move chain crabs to their historical positions (conga train). Walking self.crabs
        // mutably and consulting self.position_history in the same pass (rather than
        // collecting an intermediate Vec<(usize, Vec2)> of chain targets first) avoids a
        // per-frame heap allocation that used to scale with conga chain length.
        let mut dust_rng = crate::rng::rng();
        for crab in &mut self.crabs {
            let Some(ci) = crab.chain_index else { continue };
            let history_idx = (ci + 1) * CHAIN_LINK_FRAMES;
            let Some(&target) = self.position_history.get(history_idx) else {
                continue;
            };
            let old_pos = crab.pos;
            crab.pos = old_pos.lerp(target, 0.4);
            // Rotate caught crab toward the direction it just moved
            let move_dir = crab.pos - old_pos;
            // Compute the length once; reuse it for the facing-angle threshold, dust speed, and
            // normalize — three operations that each used to call sqrt independently per chain link.
            let move_len = move_dir.length();
            let move_speed = move_len / dt.max(1e-4);
            // Kick up a little dust from the crab's feet as the conga train stampedes along.
            let feet = crab.pos + Vec2::new(0.0, CRAB_SIZE * 0.35);
            self.particle_system.spawn_conga_dust(
                feet,
                move_dir,
                dt,
                move_len,
                move_speed,
                &mut dust_rng,
            );
            if move_len > 0.5 {
                let target_angle = move_dir.y.atan2(move_dir.x);
                let mut d = target_angle - crab.facing_angle;
                while d > std::f32::consts::PI {
                    d -= std::f32::consts::TAU;
                }
                while d < -std::f32::consts::PI {
                    d += std::f32::consts::TAU;
                }
                crab.facing_angle += d * (dt * 6.0).min(1.0);
            }
            // Beat-synced conga step: the train physically hops forward on each beat, and the
            // hop ripples down the line — each link lags the one ahead by a fixed phase — so the
            // whole train visibly steps to the rhythm instead of just gliding after the player.
            // This is gameplay reacting to the beat, not only visuals: the crabs move to it. The
            // lerp above continuously reels each crab back to its chain target every frame, so
            // this direct forward offset self-corrects and can never accumulate or drift the
            // train off its path.
            // Reuse the pre-computed length for normalize: if the move was large enough to face-
            // update (len > 0.5), divide directly instead of calling normalize_or_zero (another sqrt).
            let travel = if move_len > 1e-6 {
                move_dir / move_len
            } else {
                Vec2::ZERO
            };
            if travel != Vec2::ZERO {
                let step_phase = (1.0 - self.beat_timer / self.beat_interval)
                    * std::f32::consts::TAU
                    - ci as f32 * 0.7;
                let hop = step_phase.sin().max(0.0); // forward-only footfall each beat
                // The bar's "1" stomps forward noticeably farther than the three beats between it,
                // so the train lands the downbeat as a bigger unified lunge. bar_accent decays over
                // a beat, so the boost tapers off by the next between-beat footfall.
                let stomp = 4.0 * (1.0 + self.bar_accent * 1.6);
                crab.pos += travel * hop * stomp;
            }
        }
    }
}
