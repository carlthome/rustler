//! Startle, scatter, and panic-contagion effects for `MainState`: the per-catch
//! impact shockwave, the Hermit-pop set piece, the catch-startle stampede, and the
//! emergent beat-driven panic contagion (with Golden-crab amplification and Armored-crab
//! calm anchors). These are the "the herd reacts" behaviours that ripple out from a catch.
//!
//! Extracted verbatim from `main.rs` as `impl MainState` methods — pure structural move,
//! no behaviour change.

use ggez::glam::Vec2;

use crate::enemies::CrabType;
use crate::state::MainState;

impl MainState {
    /// Kick off a punchy impact ring at the exact spot a crab was caught. Color-coded
    /// to the crab so different crab types read differently at a glance.
    pub(crate) fn spawn_catch_shockwave(&mut self, pos: Vec2, crab_color: [f32; 3]) {
        // Cap live shockwaves so a big beat-wave sweep can't unbound the vec.
        if self.catch_shockwaves.len() < 48 {
            self.catch_shockwaves.push((pos, 0.0, crab_color));
        }
    }

    /// The signature Hermit-crack moment: fired the frame a shelled Hermit is popped open by any of
    /// its three intended ecosystem verbs (Stomp / Dancer hop / charged Magnet rip). Unlike a plain
    /// Armored "SHELL CRACKED!" — which the beam can also wear down — cracking a Hermit is a pure
    /// archetype-web payoff (the beam can't touch it), so it earns its own watchable beat: the
    /// borrowed shell scatters as a coppery shard-burst, a warm copper shockwave, a "HERMIT POPPED!"
    /// callout, and a startle ring telegraphing the brief catch window as the defenceless crab bolts.
    pub(crate) fn spawn_hermit_pop(&mut self, pos: Vec2) {
        let mut rng = crate::rng::rng();
        // The coppery shell-shard burst (same profile the catch uses) — the borrowed shell flying apart.
        self.particle_system.spawn_catch_effect(
            pos,
            [0.72, 0.44, 0.24],
            CrabType::Hermit,
            &mut rng,
        );
        // Warm copper shockwave — reads distinct from the cold blue Armored crack at a glance.
        self.spawn_catch_shockwave(pos, [0.85, 0.55, 0.28]);
        self.floating_texts.spawn(
            "HERMIT POPPED!".to_string(),
            pos - Vec2::new(66.0, 36.0),
            26.0,
            [0.95, 0.68, 0.38, 1.0], // coppery-orange so the "the ecosystem cracked it" story reads
        );
        // Startle ring telegraphs the short catch window: the popped Hermit is defenceless and bolts.
        if self.fear_rings.len() < 32 {
            self.fear_rings.push((pos, 0.0));
        }
    }

    /// Emergent stampede: the shock of a catch ripples outward and startles nearby *uncaught*
    /// crabs that aren't safely inside the flashlight beam, scattering them away from the catch
    /// point. Most noticeable when the trailing conga tail brushes through a distant cluster —
    /// nab one and the rest bolt. Keep your beam on the herd to hold them (the counterplay).
    pub(crate) fn emit_catch_startle(&mut self, origin: Vec2) {
        const STARTLE_RADIUS: f32 = 135.0;
        // Cold alarm ring so the scatter reads at a glance, distinct from the warm catch pop.
        if self.fear_rings.len() < 32 {
            self.fear_rings.push((origin, 0.0));
        }
        // Reused scratch buffer instead of a fresh Vec::new() on every single catch — a catch
        // that lands mid-herd is exactly the busiest moment for allocator churn to matter.
        let mut startled_pops = std::mem::take(&mut self.startled_pops_buf);
        startled_pops.clear();
        for crab in &mut self.crabs {
            if crab.caught || crab.in_flashlight {
                continue;
            }
            let dist = origin.distance(crab.pos);
            if dist >= STARTLE_RADIUS {
                continue;
            }
            let outward = (crab.pos - origin).normalize_or_zero();
            // Degenerate case: crab sits exactly on the origin — shove it in a stable direction.
            let outward = if outward == Vec2::ZERO {
                Vec2::new(0.0, -1.0)
            } else {
                outward
            };
            let prox = 1.0 - dist / STARTLE_RADIUS; // 1 at the epicenter, 0 at the rim
            let kick = crab.crab_type.speed_range().end * (1.3 + prox * 1.2);
            crab.vel = outward * kick;
            crab.speed = 1.0; // vel now encodes full speed, matching the flee branch's convention
            crab.startle_timer = 0.45;
            // Only pop a fresh "!" if it wasn't already panicking, so we don't spam text.
            if !crab.fleeing {
                startled_pops.push(crab.pos);
            }
        }
        for &pos in &startled_pops {
            self.floating_texts.spawn(
                "!".to_string(),
                pos - Vec2::new(0.0, 24.0),
                24.0,
                [0.6, 0.9, 1.0, 1.0],
            );
        }
        self.startled_pops_buf = startled_pops;
    }

    /// Emergent beat-startle chain reaction: on each beat, crabs that are already panicking
    /// (fleeing the player or mid-stampede) pass their fear to nearby *calm* crabs, so a scare
    /// ripples outward crab-to-crab across the herd on the pulse rather than every crab only ever
    /// reacting to the player directly. Carriers are snapshotted before infection, so the panic
    /// advances just one hop per beat — a visible marching wave, not an instant map-wide cascade.
    /// Self-limiting: only calm crabs can catch it (a crab already panicking isn't re-triggered),
    /// the startle bolt decays in ~one beat, and infections are capped per beat, so the wave dies
    /// down instead of locking the whole herd in permanent flight.
    ///
    /// Emergent crossover — the Golden Crab is a panic bomb: when the rare shiny prize is on the
    /// run its fear carries an amplified amplitude (`GOLDEN_PANIC_AMP`), reaching farther and kicking
    /// harder, and it *tags the crabs it infects as amplified carriers too*, so a fleeing Golden
    /// shatters a tight herd into a rolling stampede over the next few beats. This gives the
    /// chase-or-let-it-go decision real teeth: sprinting after the Golden through a packed crowd
    /// can scatter the very herd you were building.
    pub(crate) fn beat_startle_contagion(&mut self) {
        const CONTAGION_RADIUS: f32 = 110.0;
        const MAX_INFECTIONS_PER_BEAT: usize = 8;
        // How much harder a fleeing Golden crab's fear ripples than an ordinary panicking crab.
        const GOLDEN_PANIC_AMP: f32 = 1.6;
        // Snapshot of panicking crabs whose fear can jump to a neighbour this beat, into a
        // reused buffer instead of a fresh collect() every beat. Each carrier remembers a panic
        // amplitude so a Golden's amplified fear (and the amplified crabs it already startled)
        // keeps rippling harder than the baseline as the wave marches on.
        let mut carriers = std::mem::take(&mut self.contagion_carriers_buf);
        carriers.clear();
        carriers.extend(
            self.crabs
                .iter()
                .filter(|c| !c.caught && !c.is_boss() && (c.fleeing || c.startle_timer > 0.0))
                .map(|c| {
                    let amp = if c.is_golden() {
                        GOLDEN_PANIC_AMP
                    } else {
                        c.panic_amp.max(1.0)
                    };
                    (c.pos, amp)
                }),
        );
        if carriers.is_empty() {
            self.contagion_carriers_buf = carriers;
            return;
        }

        // Emergent crossover: free Armored crabs are calm anchors. A calm crab sheltering in the
        // shadow of an Armored shell shrugs off the panic ripple, so a herd salted with Armored
        // crabs settles instead of stampeding — and corralling a spooked crowd toward an Armored
        // crab becomes a real crowd-control play, the flipside of the Golden/Dancer chaos engines.
        // The Armored crab earns a role in the herd beyond "shell you have to crack".
        const SHELTER_RADIUS: f32 = 82.0;
        let mut anchors = std::mem::take(&mut self.armored_anchors_buf);
        anchors.clear();
        anchors.extend(
            self.crabs
                .iter()
                .filter(|c| !c.caught && !c.is_boss() && c.is_armored())
                .map(|c| c.pos),
        );

        // Bucket carriers into a spatial grid (same pattern as catch_by_chain and
        // deflect_fleeing_off_chain) so each calm crab only tests nearby carriers instead of the
        // whole panicking set — the herd has no size cap, so a flat scan here got slower the
        // longer a session ran and the bigger a stampede got, which is exactly when frame time
        // matters most for game feel.
        let cell_size = CONTAGION_RADIUS.max(1.0);
        let cell_of = |p: Vec2| -> (i32, i32) {
            (
                (p.x / cell_size).floor() as i32,
                (p.y / cell_size).floor() as i32,
            )
        };
        // Clear the whole map, not just each bucket's contents — keeping only the values cleared
        // let the key set (one entry per grid cell ever visited by a carrier) grow unbounded over
        // a long session as the herd wanders the full level, slowly bloating the hash table and
        // its load factor even though the actual per-beat working set stays tiny. A full clear()
        // still keeps the map's allocated capacity (same pooling win, no realloc most beats) but
        // resets the key count to "cells touched this beat" instead of "cells touched ever".
        self.contagion_grid_buf.clear();
        for (i, &(pos, _)) in carriers.iter().enumerate() {
            self.contagion_grid_buf
                .entry(cell_of(pos))
                .or_default()
                .push(i);
        }

        // Bucket anchors into the same grid pattern, so the shelter check below only tests
        // Armored crabs near this calm crab instead of every free Armored crab in the herd —
        // without this a session salted with several Armored crabs turned the shelter check
        // into a flat scan re-run per calm crab evaluated that beat.
        // Same unbounded-key fix as contagion_grid_buf above: clear the whole map (keeps its
        // capacity, resets its key count) instead of only clearing each bucket's Vec.
        let mut anchor_grid = std::mem::take(&mut self.armored_anchor_grid_buf);
        anchor_grid.clear();
        for (i, &pos) in anchors.iter().enumerate() {
            anchor_grid.entry(cell_of(pos)).or_default().push(i);
        }

        let mut infected_pops = std::mem::take(&mut self.contagion_pops_buf);
        infected_pops.clear();
        // Crabs an Armored anchor sheltered from the ripple this beat — drives a calm-puff cue.
        // Beat-gated (not per-frame), so a plain local Vec is fine, matching pried_by_magnet.
        let mut sheltered_pops: Vec<Vec2> = Vec::new();
        for crab in &mut self.crabs {
            if infected_pops.len() >= MAX_INFECTIONS_PER_BEAT {
                break;
            }
            // Only calm, catchable crabs outside the beam can be freshly infected.
            // A crab still soothed by a recent whistle pulse shrugs off the panic — this is what
            // makes the whistle a real crowd-control counter to a spreading stampede.
            if crab.caught
                || crab.is_boss()
                || crab.in_flashlight
                || crab.fleeing
                || crab.startle_timer > 0.0
                || crab.charm_timer > 0.0
            {
                continue;
            }
            // Nearest carrier within reach becomes the source the crab bolts away from,
            // restricted to the 3x3 neighbourhood of grid cells around the crab.
            // A Golden's amplified fear reaches beyond the baseline radius, so the closest carrier
            // is scored by how far its own reach extends, not just raw distance — an amplified
            // carrier can out-pull a nearer ordinary one and grab crabs an ordinary crab couldn't.
            let (cx, cy) = cell_of(crab.pos);
            let mut nearest: Option<(f32, Vec2, f32)> = None; // (reach-score, source pos, amp)
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(candidates) = self.contagion_grid_buf.get(&(cx + dx, cy + dy)) {
                        for &i in candidates {
                            let (source, amp) = carriers[i];
                            let d = source.distance(crab.pos);
                            let reach = CONTAGION_RADIUS * amp;
                            if d < reach {
                                // Lower score = stronger pull: normalize distance by the carrier's
                                // own reach so amplified carriers win ties within their bigger radius.
                                let score = d / amp;
                                if nearest.map_or(true, |(ns, _, _)| score < ns) {
                                    nearest = Some((score, source, amp));
                                }
                            }
                        }
                    }
                }
            }
            if let Some((score, source, amp)) = nearest {
                // Calm-anchor shelter: if an Armored crab is standing between this crab and the
                // rest of the herd, its shell settles the panic and the ripple stops here. An
                // amplified (Golden-driven) wave is only partly dampened — its fear is hot enough
                // to leak past a shell it's right on top of — so an Armored crab tames an ordinary
                // stampede outright but merely blunts a Golden panic bomb.
                let shelter_r = if amp > 1.05 {
                    SHELTER_RADIUS * 0.55
                } else {
                    SHELTER_RADIUS
                };
                // Shelter radius is always <= CONTAGION_RADIUS (the grid's cell size), so any
                // anchor within range is guaranteed to fall in the crab's own cell or one of its
                // 8 neighbours — the same 3x3 sweep used for carriers above.
                let sheltered = (-1..=1).any(|dx| {
                    (-1..=1).any(|dy| {
                        anchor_grid.get(&(cx + dx, cy + dy)).is_some_and(|bucket| {
                            bucket
                                .iter()
                                .any(|&i| anchors[i].distance(crab.pos) < shelter_r)
                        })
                    })
                });
                if sheltered {
                    // Sheltered: the crab shrugs the ripple off entirely. Deliberately leave its
                    // calm state untouched (no startle_timer bump) so it doesn't turn into a phantom
                    // carrier next beat and spread a panic it never actually felt.
                    sheltered_pops.push(crab.pos);
                    continue;
                }
                let outward = (crab.pos - source).normalize_or_zero();
                let outward = if outward == Vec2::ZERO {
                    Vec2::new(0.0, -1.0)
                } else {
                    outward
                };
                // score is d/amp in [0, CONTAGION_RADIUS); turn it back into a 1-at-source proximity.
                let prox = 1.0 - (score / CONTAGION_RADIUS).clamp(0.0, 1.0);
                let kick = crab.crab_type.speed_range().end * (1.1 + prox * 0.9) * amp;
                crab.vel = outward * kick;
                crab.speed = 1.0; // vel now encodes full speed, matching the flee/startle convention
                crab.startle_timer = 0.45;
                // Carry a decayed slice of the source's amplitude forward, so the Golden's panic
                // stays hotter than baseline for a couple more hops before fading to ordinary fear.
                crab.panic_amp = (1.0 + (amp - 1.0) * 0.7).max(1.0);
                infected_pops.push((crab.pos, amp > 1.05));
            }
        }
        // Alarm rings + "!" pops so the crab-to-crab ripple reads at a glance. Amplified
        // (Golden-driven) infections get a bigger, hot-gold "!" so a panic bomb looks like one.
        for &(pos, amplified) in &infected_pops {
            if self.fear_rings.len() < 32 {
                self.fear_rings.push((pos, 0.0));
            }
            let (size, color) = if amplified {
                (28.0, [1.0, 0.82, 0.24, 1.0])
            } else {
                (22.0, [0.6, 0.9, 1.0, 1.0])
            };
            self.floating_texts
                .spawn("!".to_string(), pos - Vec2::new(0.0, 24.0), size, color);
        }
        // Warm calming puffs off crabs an Armored anchor just sheltered — the same soothe cue the
        // whistle throws, so "the shell settled them" reads with the game's existing calm vocabulary
        // rather than needing a new effect. Capped so a big herd around an anchor doesn't spew.
        if !sheltered_pops.is_empty() {
            let mut rng = crate::rng::rng();
            for pos in sheltered_pops.into_iter().take(6) {
                self.particle_system.spawn_soothe_puff(pos, &mut rng);
            }
        }
        self.contagion_carriers_buf = carriers;
        self.contagion_pops_buf = infected_pops;
        self.armored_anchors_buf = anchors;
        self.armored_anchor_grid_buf = anchor_grid;
    }
}
