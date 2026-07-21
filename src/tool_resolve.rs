//! Per-frame resolution of the player's *active* tool effects — the ongoing simulation of a verb
//! that's already been fired. `player_tools.rs` holds the discrete on-beat triggers (fire_whistle,
//! fire_stomp, issue_groove_call, the lasso throw) and their stat helpers; this module advances
//! whatever those triggers left running: the Whistle's expanding sonic pull, the field-wide Groove
//! Call lure, the Stomp shockwave (shell cracks, Hermit King pounds, Thief shakes), and the Lasso
//! phase state machine (Winding → Throwing → Snag → Dragging | Miss → Idle).
//!
//! Extracted verbatim from `game_update.rs`'s `tick` — same code, same order, same behaviour, just
//! grouped by subsystem so the per-frame update stays navigable.

use ggez::glam::Vec2;

use crate::*;

impl MainState {
    /// Advance every active player-tool effect one frame. Called from `tick` after the visual-effect
    /// decay pass and before the chain-tail catch, exactly where the inline blocks used to run.
    pub(crate) fn resolve_active_tools(&mut self, ctx: &mut Context, dt: f32) {
        // Whistle: an expanding sonic pulse from the player that yanks free crabs inward. The pull
        // strength is per-archetype (CrabType::whistle_pull) so it's the go-to tool for skittish
        // Sneaky crabs but only nudges the heavy Big ones — a soft counter, never a hard requirement.
        if self.whistle_active > 0.0 {
            // Whistle-lane-scaled reach + pull, read once so the &mut self.crabs loop can use them.
            let whistle_max_r = self.whistle_max_radius() * self.whistle_beat_bonus;
            let whistle_pull = self.whistle_pull_speed() * self.whistle_beat_bonus;
            // The beat_bonus is only >1.0 when this cast landed on the beat (see reward_on_beat_tool),
            // so it doubles as our "was this an on-beat cast?" flag for the rhythm-native Thief shake.
            let on_beat_cast = self.whistle_beat_bonus > 1.0;
            self.whistle_active = (self.whistle_active - dt).max(0.0);
            self.whistle_radius =
                (self.whistle_radius + WHISTLE_RING_SPEED * dt).min(whistle_max_r);
            // Where the ring's leading edge sat last frame — a crab in the thin band between this and
            // whistle_radius was just swept by the front, so the shell-deflect ping fires once (crisp,
            // not a per-frame smear) as the pulse passes it. Zero-width once the ring clamps to max.
            let whistle_ring_prev = (self.whistle_radius - WHISTLE_RING_SPEED * dt).max(0.0);
            let center = self.whistle_center;
            // The whistle doubles as crowd control: sweeping it over a panicking herd soothes the
            // fear. Charm lasts a beat or two (longer as the whistle lane is ranked up) and blocks
            // both fresh flee and the beat-startle contagion, so it genuinely quells a stampede.
            let charm_dur = 1.4 + 0.5 * self.whistle_rank as f32;
            let mut soothed = std::mem::take(&mut self.whistle_soothed_buf);
            soothed.clear();
            // On-beat casts that rip a latched Thief clean off get to CATCH it as a bonus — collected
            // here (index + pos) and processed after the &mut self.crabs loop, like `soothed`/`cracked`.
            // Reused scratch buffer (almost always empty) instead of a fresh Vec::new() every frame
            // the whistle is active.
            let mut thief_snatched = std::mem::take(&mut self.whistle_thief_snatch_buf);
            thief_snatched.clear();
            for (i, crab) in self.crabs.iter_mut().enumerate() {
                if crab.caught {
                    continue;
                }
                let pull = crab.crab_type.whistle_pull();
                if pull <= 0.0 {
                    continue; // boss shrugs it off entirely
                }
                let dist = center.distance(crab.pos);
                // Only crabs the sweeping front has already passed get grabbed this frame.
                if dist < self.whistle_radius {
                    let toward = (center - crab.pos).normalize_or_zero();
                    // Stronger yank the closer the crab is, scaled by its archetype's susceptibility.
                    let proximity = 1.0 - (dist / whistle_max_r).clamp(0.0, 1.0);
                    let speed = whistle_pull * pull * (0.5 + proximity * 0.5);
                    crab.vel = toward * speed;
                    crab.speed = 1.0; // vel encodes full speed; keep multiplier neutral (matches flee convention)
                    // Golden crab being reeled in by whistle — its highest-pull matchup, show it.
                    if crab.is_golden() && self.whistle_golden_hits_buf.len() < 12 {
                        self.whistle_golden_hits_buf.push(crab.pos);
                    }
                    // Dancer pulled by whistle — rhythm tool meets rhythm crab, show the harmony.
                    if crab.is_dancer() && self.whistle_dancer_hits_buf.len() < 10 {
                        self.whistle_dancer_hits_buf.push(crab.pos);
                    }
                    // Sneaky flushed out and reeled in — the whistle's FLAGSHIP match (folds hardest
                    // of all but the Golden, whistle_pull 1.5). This was the one whistle strong-match
                    // still missing a tell; show it, and flag on-beat casts so the burst flares
                    // brighter on the beat ("gather skittish crabs on the beat" reads as a drum hit).
                    if crab.is_sneaky() && self.whistle_sneaky_hits_buf.len() < 12 {
                        self.whistle_sneaky_hits_buf.push((crab.pos, on_beat_cast));
                    }
                    // WRONG-TOOL tell: the sonic pulse pings off a still-shelled crab (Armored /
                    // shelled Hermit) instead of charming it — pull is only a token 0.3 ("barely
                    // nudges it", enemies.rs). Mirror of the lasso/shell deflect: teaches "the shell
                    // shrugs the whistle — crack it first (Stomp), then herd it." Fired once from the
                    // ring's leading edge so it reads as a crisp shell-ping, not a lingering glow.
                    if crab.boss_health > 0.0
                        && (crab.is_armored() || crab.is_shelled_hermit())
                        && dist >= whistle_ring_prev
                        && self.whistle_shell_deflect_hits_buf.len() < 12
                    {
                        self.whistle_shell_deflect_hits_buf.push(crab.pos);
                    }
                    // Count as attracted so the flee/wobble logic doesn't fight the pull next frame.
                    crab.spooked_timer = crab.spooked_timer.max(0.6);
                    // Note the crabs we actually talked down out of a panic so the "soothed" note
                    // only pops where it reads (not on already-calm crabs the pulse merely gathers).
                    if crab.fleeing || crab.startle_timer > 0.0 {
                        soothed.push(crab.pos);
                    }
                    crab.fleeing = false;
                    crab.startle_timer = 0.0;
                    crab.charm_timer = crab.charm_timer.max(charm_dur);
                    // Rhythm-native Thief counterplay: shaking off a latched Thief now *plays* like
                    // the rest of the game rather than being a flat toggle.
                    //   - ON BEAT: the whistle rips it clean off AND flings it into the train as a
                    //     bonus catch — the peak payoff for timing the counter.
                    //   - OFF BEAT: it only loosens the grip — the latch timer is pushed back so you
                    //     buy a beat, but the Thief stays on your tail and will bite again.
                    if crab.is_latched() {
                        // Strong-match tell (whistle_pull 1.3, "yanks it off your tail nicely"): the
                        // one whistle strong-match without a dedicated burst, and — off the beat — the
                        // only Thief counterplay that was visually silent (the on-beat rip already pops
                        // "THIEF NABBED!"). Show a severed-tether burst on EVERY flick at a latched
                        // Thief so the grip breaking reads either way, bright on-beat vs dim off-beat.
                        if self.whistle_thief_hits_buf.len() < 12 {
                            self.whistle_thief_hits_buf.push((crab.pos, on_beat_cast));
                        }
                        if on_beat_cast {
                            crab.latch_timer = 0.0;
                            thief_snatched.push((i, crab.pos));
                        } else {
                            // Loosen: delay the next peel without removing the threat.
                            crab.latch_timer = crab.latch_timer.max(0.75);
                        }
                    }
                }
            }
            // On-beat whistle catches its shaken Thieves: enlist each into the train and pay a bonus.
            for (i, pos) in thief_snatched.drain(..) {
                self.snatch_thief_on_beat(i, pos);
            }
            self.whistle_thief_snatch_buf = thief_snatched; // hand the buffer back for reuse next frame
            // Warm puffs rising off the crabs the pulse just calmed — the visual counterpart to
            // the cold "!" alarm rings the panic contagion throws.
            if !soothed.is_empty() {
                let mut rng = crate::rng::rng();
                for &pos in soothed.iter().take(8) {
                    self.particle_system.spawn_soothe_puff(pos, &mut rng);
                }
            }
            self.whistle_soothed_buf = soothed; // hand the buffer back for reuse next frame
        }

        // Groove Call: a FIELD-WIDE, beat-pumping herd lure. While a call is live (bars remaining),
        // every free crab across the WHOLE arena drifts toward the player — no radius gate, unlike the
        // whistle — with the pull surging on the beat and easing between (groove_call_surge, kicked in
        // the beat handler). This is the watchable payoff: the entire herd visibly streams in, lunging
        // together on each downbeat, so the beat itself becomes an arena-wide routing tool. A clean
        // on-beat call (groove_call_strength 1.0, 2 bars) pulls the herd hard and long; an off-beat one
        // (0.4, 1 bar) barely leans them in. Cheap: one extra pass over the crabs only while active.
        if self.groove_call_bars > 0.0 {
            let center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            // Base drift speed, scaled by call quality and the on-beat surge. Between beats the surge
            // decays toward ~0 so the herd coasts; on the beat it snaps back to full for the lunge.
            // Tuned against WHISTLE_PULL_SPEED (240) and the ~1280×960 view: at ~150 a crab covers a
            // few hundred units across the 2-bar (~4s) window, so even far-side crabs visibly stream
            // most of the way in — genuinely field-wide — while staying a gentle current, not the
            // whistle's hard instant yank, which is what keeps this a distinct verb.
            let base = 150.0 * self.groove_call_strength;
            let surge = 0.35 + 0.65 * self.groove_call_surge; // never fully stops, but pumps on-beat
            for crab in self.crabs.iter_mut() {
                if crab.caught {
                    continue;
                }
                // Bosses shrug it off entirely, matching the whistle's carve-out — a rhythm lure can't
                // drag a lumbering boss around. Latched Thieves and answering Dancers keep their own
                // scripted motion so the call layers over ordinary crabs without fighting other verbs.
                if crab.is_boss()
                    || crab.crab_type.whistle_pull() <= 0.0
                    || crab.is_latched()
                    || crab.answering_call > 0.0
                {
                    continue;
                }
                let toward = (center - crab.pos).normalize_or_zero();
                // Per-archetype susceptibility reuses whistle_pull so the call reads consistently with
                // the whistle (skittish crabs answer eagerly, heavy ones lean in only a little).
                let pull = crab.crab_type.whistle_pull();
                let speed = base * surge * pull;
                // Blend toward the call heading rather than overwriting velocity outright, so the herd
                // streams as a smooth current instead of teleporting — the legible "answering" flow.
                crab.vel = crab.vel.lerp(toward * speed, 0.12);
                // Hold their nerve so the flee/wobble logic doesn't fight the lure the same frame.
                crab.spooked_timer = crab.spooked_timer.max(0.5);
                crab.fleeing = false;
            }
        }

        // Stomp: a close-range ground-pound shockwave. It CRACKS Armored crab shells instantly (its
        // dedicated counter — the beam is the slow universal fallback) and gives any free crab the
        // front passes a light inward shove. Its short reach makes it a melee tool, not a ranged
        // gather like the whistle/lasso, so choosing the right verb per herd is a real decision.
        if self.stomp_active > 0.0 {
            // Stomp-lane-scaled reach, read once so the &mut self.crabs loop can use it.
            let stomp_max_r = self.stomp_max_radius() * self.stomp_beat_bonus;
            // beat_bonus >1.0 only on an on-beat cast — same on-beat flag the whistle uses.
            let on_beat_cast = self.stomp_beat_bonus > 1.0;
            self.stomp_active = (self.stomp_active - dt).max(0.0);
            self.stomp_radius = (self.stomp_radius + STOMP_RING_SPEED * dt).min(stomp_max_r);
            let center = self.stomp_center;
            let mut cracked = std::mem::take(&mut self.stomp_cracked_buf);
            cracked.clear();
            let mut hermit_popped = std::mem::take(&mut self.hermit_popped_buf);
            hermit_popped.clear();
            // Reused scratch buffer (almost always empty) instead of a fresh Vec::new() every
            // frame the stomp is active — same pattern as the whistle loop above.
            let mut thief_snatched = std::mem::take(&mut self.stomp_thief_snatch_buf);
            thief_snatched.clear();
            // Hermit King crack/deflect events this frame (rare — at most one King on the field).
            let mut king_cracks: Vec<(Vec2, f32)> = Vec::new();
            let mut king_deflects: Vec<Vec2> = Vec::new();
            for (i, crab) in self.crabs.iter_mut().enumerate() {
                // The Hermit King is the one boss the Stomp DOES touch — it's the whole fight.
                // One shell layer per pound, gated by phase: Sturdy takes any Stomp, Rattled and
                // Panicked only crack to an ON-BEAT Stomp (the same beat window every tool uses).
                // stun_timer doubles as a per-pound i-frame so one expanding ring can't peel the
                // whole stack in a few frames (it ticks down in the boss branch of update_crabs).
                if crab.is_hermit_king() && !crab.caught {
                    let dist = center.distance(crab.pos);
                    if dist < self.stomp_radius && crab.boss_health > 0.0 && crab.stun_timer <= 0.0
                    {
                        let lands =
                            matches!(hermit_king_phase(crab.boss_health), HermitKingPhase::Sturdy)
                                || on_beat_cast;
                        if lands {
                            crab.boss_health = (crab.boss_health - 1.0).max(0.0);
                            crab.stun_timer = HERMIT_KING_CRACK_IFRAME;
                            crab.join_pulse = 1.2; // reel from the pound
                            king_cracks.push((crab.pos, crab.boss_health));
                        } else {
                            crab.stun_timer = 0.5; // brief lull so the deflect cue fires once, not every frame
                            king_deflects.push(crab.pos);
                        }
                    }
                    continue;
                }
                if crab.caught || crab.is_boss() {
                    continue; // the King Crab shrugs off a Stomp — it needs the beam
                }
                let dist = center.distance(crab.pos);
                if dist >= self.stomp_radius {
                    continue; // only crabs the front has already swept past are hit this frame
                }
                // Crack a hard shell wide open the instant the shockwave reaches it — an Armored
                // crab, or a shelled Hermit (whose shell the beam can't touch, so the Stomp is one of
                // its three intended cracks). A cracked Hermit pops out defenceless and bolts.
                if (crab.is_armored() || crab.is_shelled_hermit()) && crab.boss_health > 0.0 {
                    let was_hermit = crab.is_hermit();
                    crab.boss_health = 0.0;
                    if was_hermit {
                        hermit_popped.push(crab.pos);
                    } else {
                        cracked.push(crab.pos);
                        self.stomp_armored_hits_buf.push(crab.pos);
                    }
                }
                // Strong-match: stomp cracking a Dancer's shell (Dancer is a rhythm-native target
                // for Stomp, so this hit is the archetype-tool pairing working as designed).
                if crab.is_dancer() && !crab.caught {
                    self.stomp_dancer_hits_buf.push(crab.pos);
                }
                // A Stomp near the tail is the second, close-range Thief counter — and it plays the
                // same rhythm-native way the whistle does: on-beat rips a latched Thief clean off and
                // banks it as a bonus catch; off-beat only loosens its grip so it bites again.
                if crab.is_latched() {
                    if on_beat_cast {
                        crab.latch_timer = 0.0;
                        thief_snatched.push((i, crab.pos));
                    } else {
                        crab.latch_timer = crab.latch_timer.max(0.75);
                    }
                }
                // Light inward shove + brief calm so the shaken crab doesn't immediately bolt.
                let toward = (center - crab.pos).normalize_or_zero();
                crab.vel = toward * (WHISTLE_PULL_SPEED * 0.6);
                crab.spooked_timer = crab.spooked_timer.max(0.4);
                crab.fleeing = false;
            }
            for (i, pos) in thief_snatched.drain(..) {
                self.snatch_thief_on_beat(i, pos);
            }
            self.stomp_thief_snatch_buf = thief_snatched; // hand the buffer back for reuse next frame
            // Tutorial pass tracking: count real Stomp shell-cracks for the shell-cracking learn-
            // session. Bumped only here (the crack event), guarded by the tutorial being active and
            // its kind, so a headless run of the same scenario reaches the same `passed()` predicate
            // — and it can't be satisfied by beam wear-down, since that never enters this Stomp loop.
            if let Some(t) = self.tutorial.as_mut() {
                if t.kind == TutorialKind::ShellCrack {
                    t.shells_cracked = t
                        .shells_cracked
                        .saturating_add((cracked.len() + hermit_popped.len()) as u32);
                }
            }
            // Campaign win tracking: every full shell crack (Armored or Hermit) counts toward a
            // CrackAndHold goal, whatever verb did the cracking — this is the Stomp site.
            self.shells_cracked_run += cracked.len() + hermit_popped.len();
            for &pos in cracked.iter() {
                self.floating_texts.spawn(
                    "SHELL CRACKED!".to_string(),
                    pos - Vec2::new(70.0, 40.0),
                    26.0,
                    [0.7, 0.85, 1.0, 1.0],
                );
                self.spawn_catch_shockwave(pos, [0.7, 0.8, 0.95]);
            }
            for pos in hermit_popped.drain(..) {
                self.spawn_hermit_pop(pos);
            }
            for &(pos, shells_left) in king_cracks.iter() {
                if shells_left <= 0.0 {
                    self.floating_texts.spawn(
                        "THE KING IS EXPOSED — CATCH IT!".to_string(),
                        pos - Vec2::new(150.0, 70.0),
                        34.0,
                        [0.4, 1.0, 0.5, 1.0],
                    );
                    self.spawn_catch_shockwave(pos, [1.0, 0.95, 0.6]);
                    self.spawn_catch_shockwave(pos, [0.95, 0.6, 0.25]);
                    self.screen_shake = self.screen_shake.max(18.0);
                } else {
                    self.floating_texts.spawn(
                        format!("SHELL HOUSE CRACKED! {} LEFT", shells_left as u32),
                        pos - Vec2::new(120.0, 55.0),
                        28.0,
                        [0.95, 0.7, 0.35, 1.0],
                    );
                    self.spawn_catch_shockwave(pos, [0.9, 0.6, 0.25]);
                    self.screen_shake = self.screen_shake.max(10.0);
                }
            }
            for &pos in king_deflects.iter() {
                self.floating_texts.spawn(
                    "DEFLECTED — STOMP ON THE BEAT!".to_string(),
                    pos - Vec2::new(130.0, 50.0),
                    26.0,
                    [0.7, 0.8, 1.0, 1.0],
                );
            }
            self.stomp_cracked_buf = cracked; // hand the buffer back for reuse next frame
            self.hermit_popped_buf = hermit_popped; // hand the buffer back for reuse next frame
        }

        // Lasso: phase-driven state machine (Winding → Throwing → Snag → Dragging | Miss → Idle).
        // Winding charges while the mouse is held; Throwing advances each frame.
        {
            let player_center = self.player_pos + Vec2::new(PLAYER_SIZE / 2.0, PLAYER_SIZE / 2.0);
            match self.lasso_phase {
                LassoPhase::Winding => {
                    // Grow charge and spin faster as it builds; cap at max.
                    self.lasso_charge = (self.lasso_charge + dt).min(LASSO_MAX_CHARGE_TIME);
                    let charge_frac = self.lasso_charge / LASSO_MAX_CHARGE_TIME;
                    // Loop spins faster as charge builds (cowboy wind-up feel).
                    self.lasso_spin += dt * (8.0 + charge_frac * 20.0);
                    // Keep lasso tip parked at player center while winding.
                    self.lasso_pos = Some(player_center);
                    // If mouse was released (fire_lasso_throw called), phase will already be Throwing.
                }
                LassoPhase::Throwing => {
                    self.lasso_timer -= dt;
                    // Charge fraction drives speed: a full charge covers max-range in LASSO_THROW_TIME;
                    // a tap covers only MIN_RANGE_FRAC of that (scales both range and tip travel).
                    let progress = (1.0 - self.lasso_timer / LASSO_THROW_TIME).clamp(0.0, 1.0);
                    let new_pos = self.lasso_origin.lerp(self.lasso_target, progress);
                    self.lasso_pos = Some(new_pos);
                    self.lasso_spin += dt * 18.0; // keep spinning during flight

                    if self.lasso_timer <= 0.0 {
                        // The throw has reached its target — check for catches.
                        let tip = self.lasso_target;
                        let grab_r = self.lasso_tip_radius();
                        let mut to_catch = std::mem::take(&mut self.lasso_catch_buf);
                        to_catch.clear();
                        to_catch.extend(
                            self.crabs
                                .iter()
                                .enumerate()
                                .filter(|(_, c)| c.is_catchable() && tip.distance(c.pos) < grab_r)
                                .map(|(i, _)| i),
                        );
                        if to_catch.is_empty() {
                            // Miss: loop flops empty with a dust puff.
                            self.lasso_pos = Some(self.lasso_target);
                            self.lasso_phase = LassoPhase::Miss;
                            self.lasso_timer = LASSO_MISS_TIME;
                            // WRONG-TOOL tell: if the loop actually landed *on* a still-shelled crab
                            // (Armored, or a Hermit with its borrowed shell up), the shell slipped it
                            // off — that's the "lasso slips off Armored" rule (enemies.rs). Without a
                            // cue this reads as a plain whiff; flag it so draw_lasso_shell_deflect can
                            // flash a hard grey-steel ricochet, teaching "crack the shell first (Stomp),
                            // then lasso." Mirrors the beam/Hermit amber can't-crack cue.
                            for c in self.crabs.iter() {
                                if c.boss_health > 0.0
                                    && (c.is_armored() || c.is_shelled_hermit())
                                    && tip.distance(c.pos) < grab_r
                                {
                                    self.lasso_shell_deflect_hits_buf.push(c.pos);
                                }
                            }
                        } else {
                            // Snag: loop tightens/squeezes before dragging.
                            self.lasso_pos = Some(self.lasso_target);
                            self.lasso_phase = LassoPhase::Snag;
                            self.lasso_timer = LASSO_SNAG_TIME;
                        }
                        let mut rng = crate::rng::rng();
                        let mut lasso_startle_origins = std::mem::take(&mut self.lasso_startle_buf);
                        lasso_startle_origins.clear();
                        for i in to_catch.iter().copied() {
                            let pos = self.crabs[i].pos;
                            let crab_type = self.crabs[i].crab_type;
                            let crab_color = self.crabs[i].crab_color();
                            self.particle_system
                                .spawn_catch_effect(pos, crab_color, crab_type, &mut rng);
                            self.spawn_catch_shockwave(pos, crab_color);
                            let was_answering = self.crabs[i].answering_call > 0.0;
                            // Strong-match: lasso catching a Thief (lasso is the intended counter
                            // to the Thief — so this hit is the archetype-tool pairing paying off).
                            if self.crabs[i].is_thief() {
                                self.lasso_thief_hits_buf.push(self.crabs[i].pos);
                            }
                            // Strong-match: lasso snagging a Magnet — the loop then drags it through
                            // the herd, turning the Magnet's pull field into a pied-piper sweep.
                            // Show a magnetic surge burst so the player reads "lasso + Magnet = cluster pull."
                            if self.crabs[i].is_magnet() {
                                self.lasso_magnet_hits_buf.push(self.crabs[i].pos);
                            }
                            // Strong-match: lasso hauling in a heavy Big crab. The whistle "shrugs
                            // most off" (whistle_pull 0.4), so the loop's physical drag is the Big
                            // crab's intended counter — show a straining "heave" so the pairing reads.
                            // On-beat throws (lasso_on_beat_bonus > 1.0) flare it brighter and wider,
                            // so timing the haul to the beat lands like a drum hit.
                            if self.crabs[i].is_big() && self.lasso_big_hits_buf.len() < 8 {
                                let on_beat = self.lasso_on_beat_bonus > 1.0;
                                self.lasso_big_hits_buf.push((self.crabs[i].pos, on_beat));
                            }
                            self.crabs[i].caught = true;
                            if let Some(t) = self.tutorial.as_mut() {
                                if t.kind == TutorialKind::LassoGrab {
                                    t.lasso_catches += 1;
                                }
                            }
                            if self.crabs[i].is_boss() {
                                self.on_boss_caught(pos, self.crabs[i].crab_type);
                            }
                            if self.crabs[i].is_golden() {
                                self.on_golden_caught(pos, 0);
                            }
                            self.reward_dance_catch(was_answering, pos);
                            lasso_startle_origins.push(pos);
                            self.chain_join_ripple = true;
                            self.crabs[i].chain_index = Some(self.chain_count);
                            self.chain_count += 1;
                            self.check_milestone(&mut crate::rng::rng());
                            self.score += self.combo_multiplier();
                            self.shake_timer = 0.15;
                            self.hitstop_timer = self.hitstop_timer.max(0.06);
                            self.time_since_catch = 0.0;
                            play_catch_sound(&mut self.sounds, ctx, &mut rng, self.beat_streak);
                            self.check_upgrade_unlock(ctx);
                        }
                        for &origin in lasso_startle_origins.iter() {
                            self.emit_catch_startle(origin);
                        }
                        self.lasso_catch_buf = to_catch;
                        self.lasso_startle_buf = lasso_startle_origins;
                    }
                }
                LassoPhase::Snag => {
                    self.lasso_timer -= dt;
                    self.lasso_spin += dt * 8.0;
                    if self.lasso_timer <= 0.0 {
                        self.lasso_phase = LassoPhase::Dragging;
                        self.lasso_timer = LASSO_DRAG_TIME;
                    }
                }
                LassoPhase::Dragging => {
                    self.lasso_timer -= dt;
                    let drag_t = (1.0 - self.lasso_timer / LASSO_DRAG_TIME).clamp(0.0, 1.0);
                    // Tip reels back from target to player center.
                    let new_pos = self.lasso_target.lerp(player_center, drag_t);
                    self.lasso_pos = Some(new_pos);
                    self.lasso_spin += dt * 6.0;
                    if self.lasso_timer <= 0.0 {
                        self.lasso_phase = LassoPhase::Idle;
                        self.lasso_pos = None;
                    }
                }
                LassoPhase::Miss => {
                    self.lasso_timer -= dt;
                    self.lasso_spin += dt * 4.0;
                    if self.lasso_timer <= 0.0 {
                        self.lasso_phase = LassoPhase::Idle;
                        self.lasso_pos = None;
                    }
                }
                LassoPhase::Idle => {}
            }
        }
    }
}
