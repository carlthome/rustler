//! Rival NPC King Crab trains: the wandering AI that patrols the world, threads through the
//! player's conga line to splice a steal (and can be threaded back the other way), and collides
//! with the player and with other NPC trains. The draw pass that renders each train lives in the
//! sibling `npc_trains_render` module. Extracted out of `main.rs`'s `impl MainState` — same
//! methods, same behaviour, just grouped by subsystem instead of living in one file.

use ggez::glam::Vec2;
use rand::Rng;

use crate::constants::*;
use crate::enemies::CrabType;
use crate::spawnings::{spawn_scattered_crab, spawn_stolen_crab};
use crate::state::MainState;

impl MainState {

    pub fn update_npc_trains(&mut self, dt: f32) {
        // One shared cooldown gates how often YOU can rustle from a rival, so threading a line
        // takes one clean back-section per window instead of vacuuming a whole train in a frame.
        self.player_steal_cooldown = (self.player_steal_cooldown - dt).max(0.0);
        // Whether we're inside the on-beat window this frame — the rival's steal snaps ON the beat
        // (see the splice block below) so losing crabs is rhythmic, a drum hit rather than a random grab.
        let on_beat = self.beat_timer < BEAT_WINDOW
            || self.beat_timer > self.beat_interval - BEAT_WINDOW;
        // The downbeat (beat 1 of the 4/4 bar) is the big-hit moment — same convention as
        // on_downbeat_now(). A reroute that lands on the downbeat is the "big save" version.
        let downbeat = on_beat && self.beat_count % 4 == 0;
        for i in 0..self.npc_trains.len() {
            // --- Idle pause at destination -------------------------------------------------
            // When idle_timer > 0 the train has just arrived at a target and is "surveying"
            // before picking a new one — gives Rain World-style decisiveness, not dumb wandering.
            if self.npc_trains[i].idle_timer > 0.0 {
                self.npc_trains[i].idle_timer -= dt;
                // Decelerate while idling
                self.npc_trains[i].leader_vel *= (1.0 - 6.0 * dt).max(0.0);
                // Still sample path so followers catch up during the pause
                let cur = self.npc_trains[i].leader_pos;
                let last = self.npc_trains[i]
                    .path_history
                    .front()
                    .copied()
                    .unwrap_or(cur);
                if cur.distance_squared(last) > 36.0 {
                    self.npc_trains[i].path_history.push_front(cur);
                }
                let dist_to_player = cur.distance(self.player_pos);
                self.npc_trains[i].target_vol = ((800.0 - dist_to_player) / 600.0).clamp(0.0, 1.0);
                // A rival surveying at its destination isn't chasing — bleed its hunt intent off.
                self.npc_trains[i].hunt_intent *= (1.0 - 2.2 * dt).max(0.0);
                continue;
            }

            let to_target = self.npc_trains[i].target - self.npc_trains[i].leader_pos;
            let dist = to_target.length();

            // --- Wander target selection ---------------------------------------------------
            // Bias targets strongly toward territory center so rivals patrol distinct regions.
            // Small scouts are fast and range further; large elders are slow and stay local.
            self.npc_trains[i].target_timer -= dt;
            if dist < 80.0 || self.npc_trains[i].target_timer <= 0.0 {
                // Arrived — enter a brief idle before picking the next target.
                let rng = &mut crate::rng::rng();
                let idle_secs = rng.random_range(1.2_f32..3.5);
                self.npc_trains[i].idle_timer = idle_secs;

                // Territory-biased target: blend between a random world point and the territory
                // center. Large elder (scale 2.4) stays very local; small scout (scale 1.2) ranges.
                let scale = self.npc_trains[i].leader_scale;
                // territory_bias 0..1: how strongly the next target is pulled toward territory center
                let territory_bias = ((scale - 1.2) / 1.2).clamp(0.0, 1.0) * 0.65 + 0.2;
                let margin = 160.0;
                // Guard against empty range panic if world is unexpectedly small
                let ww = (self.world_width - margin).max(margin + 1.0);
                let wh = (self.world_height - margin).max(margin + 1.0);
                let rand_pt = Vec2::new(rng.random_range(margin..ww), rng.random_range(margin..wh));
                let tc = self.npc_trains[i].territory_center;
                // Offset from territory center — scouts wander further (larger offset radius)
                let wander_radius = 380.0 - scale * 80.0; // scout=284, medium=236, elder=188
                let angle: f32 = rng.random_range(0.0..std::f32::consts::TAU);
                let territory_pt = tc + Vec2::new(angle.cos(), angle.sin()) * wander_radius;
                let next_target = rand_pt.lerp(territory_pt, territory_bias);
                self.npc_trains[i].target = next_target.clamp(
                    Vec2::splat(margin),
                    Vec2::new(self.world_width - margin, self.world_height - margin),
                );
                // Timer is a fallback; normal flow goes through idle_timer arrival check
                self.npc_trains[i].target_timer = rng.random_range(18.0_f32..35.0);
            }

            // --- Steering ------------------------------------------------------------------
            // Speed inversely proportional to leader_scale: scouts zip, elders lumber.
            let speed = match () {
                _ if self.npc_trains[i].leader_scale < 1.5 => 105.0, // small scout
                _ if self.npc_trains[i].leader_scale < 2.0 => 80.0,  // medium wanderer
                _ => 52.0,                                           // large elder
            };
            // Gentle perpendicular wobble so the path curves naturally instead of beelining.
            let perp = Vec2::new(-to_target.y, to_target.x).normalize_or_zero();
            let wobble_phase = self.time_elapsed * 0.4 + i as f32 * 2.1;
            let wobble = perp * wobble_phase.sin() * 18.0;

            if dist > 1.0 {
                let desired = (to_target / dist + wobble / dist.max(1.0)) * speed;
                let steer_rate = if dist < 200.0 { 4.5 } else { 2.8 }; // tighter turns near target
                let steer = (desired - self.npc_trains[i].leader_vel) * (steer_rate * dt);
                self.npc_trains[i].leader_vel += steer;
                if self.npc_trains[i].leader_vel.length() > speed {
                    self.npc_trains[i].leader_vel =
                        self.npc_trains[i].leader_vel.normalize() * speed;
                }
            }
            let margin = 80.0;
            let vel_step = self.npc_trains[i].leader_vel * dt;
            self.npc_trains[i].leader_pos += vel_step;
            self.npc_trains[i].leader_pos.x = self.npc_trains[i]
                .leader_pos
                .x
                .clamp(margin, self.world_width - margin);
            self.npc_trains[i].leader_pos.y = self.npc_trains[i]
                .leader_pos
                .y
                .clamp(margin, self.world_height - margin);

            // --- Path history for follower trailing ----------------------------------------
            let cur_pos = self.npc_trains[i].leader_pos;
            let last = self.npc_trains[i]
                .path_history
                .front()
                .copied()
                .unwrap_or(cur_pos);
            if cur_pos.distance_squared(last) > 36.0 {
                self.npc_trains[i].path_history.push_front(cur_pos);
                let max_len = self.npc_trains[i].follower_types.len() * 16 + 20;
                while self.npc_trains[i].path_history.len() > max_len {
                    self.npc_trains[i].path_history.pop_back();
                }
            }

            // Scale the King Crab with its conga line: more followers = bigger, scarier leader.
            // Each follower adds 0.09 scale above the tier floor, capped at 3.8 so even a
            // maxed-out elder doesn't become comically huge.
            {
                let n = self.npc_trains[i].follower_types.len() as f32;
                let base = self.npc_trains[i].base_scale;
                self.npc_trains[i].leader_scale = (base + n * 0.09).min(3.8);
            }

            // Compute target rumble volume from distance to player.
            let dist_to_player = self.npc_trains[i].leader_pos.distance(self.player_pos);
            self.npc_trains[i].target_vol = ((800.0 - dist_to_player) / 600.0).clamp(0.0, 1.0);

            // --- Pursuit: a two-phase stalk→strike hunt of the player's train (#160) ---------
            // The NPC behaves like a rival player with intent (INSPIRATION.md "Rivals route
            // deliberately") — but a smart predator doesn't beeline. The hunt has two phases:
            //
            //   STALK — shadow the player at a lurk ring, matching pace from the edge of the fight
            //   (the agar.io "watch the big one creep toward you" dread), while a commit meter
            //   (`stalk_patience`) builds. It builds faster when you're exposed (low groove — off
            //   the beat), when the prize is juicy (a long train), and for bolder tiers (elders
            //   commit sooner than skittish scouts). Staying in the pocket literally keeps the
            //   hunters circling at arm's length; lapsing invites the strike.
            //
            //   STRIKE — at full patience the rival COMMITS: it stops chasing where your vulnerable
            //   back half *is* and intercepts where it's *heading*, leading its aim by your velocity
            //   toward the same ~2/3-down thread point the boss uses (cached_steal_target_pos,
            //   falling back to the tail on a short chain). That's the "it read my routing" scare:
            //   a lazy sprawling spiral gets cut off mid-arc, while tight defensive routing keeps
            //   the reachable links bunched at the far tail (small cut). After the strike resolves
            //   (splice snaps, or you dodge it) patience resets and the predator falls back to
            //   lurking — a stalk→strike rhythm, not a constant chase.
            //
            // Legible and fair, per the issue's hard requirement: stalking drives hunt_intent to
            // ~0.55 (the red marching-dot telegraph burns faint — "it's watching you"), committing
            // drives it to 1.0 (full-intensity dots + reddened name banner) and calls the hunter
            // out by name, and the close-range arm/DEFEND/dodge fight is untouched. Deterministic:
            // pure function of game state + dt — no RNG, no wall clock — so the headless bots stay
            // byte-stable.
            const PURSUIT_RANGE: f32 = 550.0;
            // Opportunism: a rival reads when you're exposed and presses in from farther, like a
            // Rain-World predator that senses weakness. The clearest "exposed" signal is a LOW
            // GROOVE — you've fallen out of the pocket, off the beat — so staying on-beat keeps the
            // hunters at arm's length and lapsing invites the steal (INSPIRATION "steal to win",
            // "keys as drum pads": playing the groove well IS the defense). The wider reach trips
            // the hunt-intent telegraph sooner, so you *see* a rival commit the moment you slip.
            let exposure = (1.0 - self.groove).clamp(0.0, 1.0); // 0 in the pocket, 1 fully off-beat
            let pursuit_range = PURSUIT_RANGE + exposure * 180.0;
            // Boldness by tier: 0 for the skittish scout (base 1.2) up to 1 for the elder (2.4).
            let boldness =
                ((self.npc_trains[i].base_scale - 1.2) / 1.2).clamp(0.0, 1.0);
            // Hunt intent smooths toward its phase goal while this rival is on a steal route and back
            // toward 0 otherwise, so the early-warning tell fades in/out instead of popping. Updated
            // every non-idle frame (goal 0 when not hunting) so it always relaxes once the chase ends.
            let hunting = self.chain_count >= 2
                && dist_to_player < pursuit_range
                && self.cached_steal_target_pos.or(self.cached_tail_pos).is_some();
            if !hunting {
                // Hunt lost (player banked, escaped range, or the chain snapped): drop any commit
                // and bleed patience so the next hunt starts from a fresh stalk, not a hair trigger.
                self.npc_trains[i].hunt_committed = false;
                self.npc_trains[i].stalk_patience =
                    (self.npc_trains[i].stalk_patience - dt * 0.35).max(0.0);
            }
            let hunt_goal = if !hunting {
                0.0
            } else if self.npc_trains[i].hunt_committed {
                1.0 // strike phase — the telegraph burns at full intensity
            } else {
                0.55 // stalk phase — the same tell, faint: "it's watching you"
            };
            let hunt_rate = if hunting { 1.4 } else { 2.4 };
            self.npc_trains[i].hunt_intent +=
                (hunt_goal - self.npc_trains[i].hunt_intent) * (hunt_rate * dt).min(1.0);
            if hunting && self.npc_trains[i].idle_timer <= 0.0 {
                // Both phases route off the back-half thread point when the chain is long enough to
                // have one, else the tail. Both are cached once per frame in update_crabs — no
                // O(n_crabs) scan.
                if let Some(steal_pos) = self.cached_steal_target_pos.or(self.cached_tail_pos) {
                    // Base blend ramps as the rival closes in; a longer train adds up to +0.4 commit
                    // so big trains get pursued with real intent instead of a lazy drift; an exposed
                    // (off-beat) player adds up to +0.3 more.
                    let length_urge = ((self.chain_count as f32 - 2.0) / 8.0).clamp(0.0, 0.4);
                    let pursuit_blend =
                        (((pursuit_range - dist_to_player) / pursuit_range)
                            + length_urge
                            + exposure * 0.3)
                            .clamp(0.0, 1.0);
                    if !self.npc_trains[i].hunt_committed {
                        // STALK: build patience — exposure is the loudest signal, then the prize
                        // and this tier's boldness. Even a flawless player eventually gets tested
                        // (the base term), but slowly enough that the read always comes first:
                        // scout-vs-grooving-player takes ~10s to commit, elder-vs-exposed ~1.5s.
                        let build =
                            0.10 + boldness * 0.10 + exposure * 0.35 + length_urge * 0.5;
                        self.npc_trains[i].stalk_patience =
                            (self.npc_trains[i].stalk_patience + build * dt).min(1.0);
                        if self.npc_trains[i].stalk_patience >= 1.0 {
                            // COMMIT — the strike begins. Call the hunter out by name so the scare
                            // is legible from across the field, matching the "on your tail!" arm
                            // warning's threat language (only fires once per stalk cycle, since
                            // patience must rebuild from 0 after every strike).
                            self.npc_trains[i].hunt_committed = true;
                            let npc_name = self.npc_trains[i].name.clone();
                            self.floating_texts.spawn(
                                format!("⚠ {} is hunting you!", npc_name),
                                self.npc_trains[i].leader_pos - Vec2::new(90.0, 60.0),
                                24.0,
                                [0.95, 0.25, 0.18, 1.0],
                            );
                        }
                        // Shadow point: hold the lurk ring on this rival's side of the player,
                        // drifting with the train — menace expressed through movement. Bolder
                        // tiers lurk closer; everyone edges nearer as the strike ripens, so the
                        // creep itself telegraphs how close the commit is.
                        let away = (self.npc_trains[i].leader_pos - steal_pos).normalize_or_zero();
                        let away = if away == Vec2::ZERO { Vec2::X } else { away };
                        let stalk_radius = (300.0 - boldness * 60.0)
                            * (1.0 - 0.4 * self.npc_trains[i].stalk_patience);
                        let shadow = steal_pos + away * stalk_radius;
                        self.npc_trains[i].target = self.npc_trains[i]
                            .target
                            .lerp(shadow, pursuit_blend * dt * 3.0);
                    } else {
                        // STRIKE: intercept. Lead the aim by the player's velocity, scaled by the
                        // time this rival needs to cover the gap — cutting off where the routing
                        // player is heading, not trailing where they've been. Clamped tight so the
                        // predicted point stays readable (never more than ~a second of lead) and
                        // inside the world, and the existing full-intensity marching dots still
                        // point at the actual threatened link — the tell shows what's at risk, the
                        // movement shows the cutoff.
                        let gap = self.npc_trains[i].leader_pos.distance(steal_pos);
                        let lead_time = (gap / speed.max(1.0)).clamp(0.0, 1.1);
                        let margin = 80.0;
                        let intercept = (steal_pos + self.player_vel * lead_time * 0.8).clamp(
                            Vec2::splat(margin),
                            Vec2::new(self.world_width - margin, self.world_height - margin),
                        );
                        self.npc_trains[i].target = self.npc_trains[i]
                            .target
                            .lerp(intercept, (pursuit_blend + 0.3).min(1.0) * dt * 3.6);
                        // Monotonic tally for the bot guard: a committed rival applied intercept
                        // steering this frame, so the "rival intercepts a routing player" path is live.
                        self.hunt_intercepts = self.hunt_intercepts.saturating_add(1);
                    }
                }
            }

            // --- Rival-hunt urge: steer toward the nearest strictly-smaller rival to bully it -
            // ROADMAP ★ headline, step 2 ("a deliberate urge to hunt the weaker train"): the same
            // per-creature intent that threads the player's line (above), pointed instead at the
            // nearest SMALLER rival train. Without this the rival-vs-rival splice below only fires
            // when two trains happen to cross while wandering — the ecology churns by luck. With it,
            // a bigger train visibly seeks out and slices a smaller one, so the agar.io/Rain World
            // pecking order emerges from a purely local rule (bigger hunts smaller) rather than a
            // global planner. Cheap: an O(n_trains²) nearest scan over a handful of trains, no
            // per-crab work. Player pursuit wins when it's live (the player is the main character),
            // so this only bites when no player prey is near — a gentle fallback urge, not a lock.
            // It deliberately does NOT touch hunt_intent: that drives the telegraph dots that warn
            // the *player* they're being threaded (see draw), and a rival chasing another rival must
            // not paint a false "you're being hunted" tell across the player's line.
            // Cleared every frame; re-armed below only while a rival hunt is genuinely live and
            // imminent, so the gold "predator closing" telegraph (drawn in the render pass) never
            // lingers after the chase ends.
            self.npc_trains[i].rival_hunt_target_pos = None;
            self.npc_trains[i].rival_hunt_intensity = 0.0;
            if !hunting && self.npc_trains[i].idle_timer <= 0.0 {
                let my_len = self.npc_trains[i].follower_types.len();
                if my_len >= 1 {
                    const RIVAL_HUNT_RANGE: f32 = 620.0;
                    let my_pos = self.npc_trains[i].leader_pos;
                    // Nearest strictly-smaller rival with followers — the only train this one can
                    // actually splice, so the urge and the splice rule below agree.
                    let mut best: Option<(usize, f32)> = None;
                    for v in 0..self.npc_trains.len() {
                        if v == i {
                            continue;
                        }
                        let vlen = self.npc_trains[v].follower_types.len();
                        if vlen == 0 || vlen >= my_len {
                            continue;
                        }
                        let d = my_pos.distance(self.npc_trains[v].leader_pos);
                        if d < RIVAL_HUNT_RANGE && best.map_or(true, |(_, bd)| d < bd) {
                            best = Some((v, d));
                        }
                    }
                    if let Some((v, d)) = best {
                        // Aim at the victim's back-half thread point (its mid-follower slot on
                        // path_history, spacing 14 to match the splice pass) so the leader routes to
                        // slice a meaningful section, exactly like the player-pursuit path does.
                        let vlen = self.npc_trains[v].follower_types.len();
                        let thread_fi = vlen.saturating_sub(1) / 2;
                        let hunt_pos = self.npc_trains[v]
                            .path_history
                            .get((thread_fi + 1) * 14)
                            .copied()
                            .unwrap_or(self.npc_trains[v].leader_pos);
                        // Stronger urge the closer the prey and the bigger the size gap, but it stays
                        // a bias layered onto territory patrol — not a beeline — so trains still read
                        // as roaming their regions between kills.
                        let closeness = ((RIVAL_HUNT_RANGE - d) / RIVAL_HUNT_RANGE).clamp(0.0, 1.0);
                        let gap_urge = ((my_len - vlen) as f32 / 6.0).clamp(0.0, 0.5);
                        let blend = (closeness * 0.6 + gap_urge).clamp(0.0, 1.0);
                        self.npc_trains[i].target =
                            self.npc_trains[i].target.lerp(hunt_pos, blend * dt * 2.2);
                        // Arm the gold "predator closing" telegraph toward the prey's *leader* (King→King,
                        // so the read is "that big train is bearing down on that small one"), but only
                        // once the predator is genuinely closing — a wide, lazy urge shouldn't clutter the
                        // field. Gate on real closeness so the tell means "clash incoming, get in position."
                        if closeness > 0.35 {
                            self.npc_trains[i].rival_hunt_target_pos =
                                Some(self.npc_trains[v].leader_pos);
                            self.npc_trains[i].rival_hunt_intensity = blend;
                            // Monotonic tally for the bot guard — bumped here (the draw pass only holds
                            // an immutable borrow of npc_trains). Armed ⇒ drawn, so this tracks the tell.
                            self.rival_hunt_telegraphs = self.rival_hunt_telegraphs.saturating_add(1);
                        }
                    }
                }
            }

            // --- Reverse-Snake chain splice steal (telegraphed + beat-synced) ----------------
            // When the NPC leader threads within range of an exposed tail link it ARMS a steal:
            // a brief telegraph fuse ramps while the threatened crabs tremble in place, then the
            // splice SNAPS on the beat (or when the fuse expires). Everything from the spliced link
            // to the tail detaches from the player and joins the NPC. Making the grab telegraphed and
            // rhythmic — never a silent instant strip — is what makes losing crabs read as *earned*
            // (INSPIRATION.md "Legible risk") and land like a drum hit rather than random loss.
            self.npc_trains[i].steal_cooldown = (self.npc_trains[i].steal_cooldown - dt).max(0.0);
            // Rival-vs-rival steal cooldown burns down independently (see the whole-beach splice pass
            // after this loop) so a train churns crabs with other rivals at its own pace.
            self.npc_trains[i].rival_steal_cooldown =
                (self.npc_trains[i].rival_steal_cooldown - dt).max(0.0);
            // Revenge marker burns down: once it lapses the "chase me" ring fades and a steal-back
            // off this rival is just a normal rustle, not a revenge bonus.
            self.npc_trains[i].revenge_timer = (self.npc_trains[i].revenge_timer - dt).max(0.0);
            if self.npc_trains[i].steal_cooldown <= 0.0 && self.chain_count > 1 {
                const STEAL_RANGE: f32 = 58.0;
                const STEAL_RANGE_SQ: f32 = STEAL_RANGE * STEAL_RANGE;
                // STEAL_FUSE (telegraph window, ~one beat between arming and the snap) lives in
                // constants.rs so the bot defense test arms with the exact same fuse.
                let npc_pos = self.npc_trains[i].leader_pos;
                let armed = self.npc_trains[i].steal_threat > 0.0;
                // Early-out: if the NPC is far from the player and the chain tail — and nothing is
                // already armed — no chain crab can be within STEAL_RANGE. Use cached_tail_pos (the
                // farthest link, already computed by update_crabs) as a lower-bound proxy to avoid the
                // O(n_crabs) scan. Once armed we fall through so the fuse still counts down to its snap.
                let chain_span = self
                    .cached_tail_pos
                    .map_or(0.0_f32, |t| t.distance(self.player_pos));
                let dist_to_chain = dist_to_player - chain_span;
                if dist_to_chain > STEAL_RANGE && !armed {
                    continue; // skip inner per-crab scan entirely this frame for this NPC
                }
                // Find the earliest (closest-to-head) link the NPC is within range of.
                // We splice there so a threading pass takes the maximum tail section.
                let splice_at = self
                    .crabs
                    .iter()
                    .filter(|c| c.caught && c.chain_index.map_or(false, |idx| idx > 0))
                    .filter(|c| npc_pos.distance_squared(c.pos) < STEAL_RANGE_SQ)
                    .map(|c| c.chain_index.unwrap())
                    .min();

                if !armed {
                    // ARM the steal the moment a link comes into range: start the telegraph fuse and
                    // latch the target link so the snap fires from here even if the leader drifts off it.
                    if let Some(splice_idx) = splice_at {
                        self.npc_trains[i].steal_threat = STEAL_FUSE;
                        self.npc_trains[i].steal_target = splice_idx;
                        let npc_name = self.npc_trains[i].name.clone();
                        let warn_pos = self
                            .crabs
                            .iter()
                            .find(|c| c.caught && c.chain_index.map_or(false, |idx| idx >= splice_idx))
                            .map_or(npc_pos, |c| c.pos);
                        // Peripheral threat language: a red warning callout + ring at the threatened tail.
                        self.floating_texts.spawn(
                            format!("⚠ {} is on your tail!", npc_name),
                            warn_pos - Vec2::new(90.0, 42.0),
                            26.0,
                            [0.98, 0.40, 0.16, 1.0],
                        );
                        if self.catch_shockwaves.len() < 48 {
                            self.catch_shockwaves.push((warn_pos, 0.0, [0.98, 0.30, 0.14]));
                        }
                    }
                } else {
                    // Armed: creep the latched target forward if the NPC threaded closer to the head
                    // (a deeper cut steals more), tremble the threatened crabs as the telegraph, and
                    // snap on the beat once the warning has shown a moment — or when the fuse runs out.
                    if let Some(splice_idx) = splice_at {
                        self.npc_trains[i].steal_target =
                            self.npc_trains[i].steal_target.min(splice_idx);
                    }
                    // --- Movement dodge: juke the threaded tail out of the rival's reach ----------
                    // INSPIRATION.md item 2 promises TWO defenses against an armed steal: a tool
                    // parry (try_defend_steal) OR an "on-beat defensive reroute". This is the reroute
                    // — the movement half of the skill. When the rival threaded your line it latched
                    // onto a specific link (steal_target); if you drag that link clear before the snap
                    // (a committed run, or a sprint-juke) the thread breaks and the splice fizzles
                    // with nothing to cut. Geometry, not RNG — the rival has to actually still be on
                    // the link to cut it. An on-beat escape reads as a clean reroute and feeds the
                    // groove: a dodge on the beat is a drum hit too ("keys as drum pads").
                    let thread_idx = self.npc_trains[i].steal_target;
                    let thread_pos = self
                        .crabs
                        .iter()
                        .find(|c| c.caught && c.chain_index == Some(thread_idx))
                        .map(|c| c.pos);
                    if let Some(tp) = thread_pos {
                        // ~2.5× STEAL_RANGE: a committed run or a sprint-juke, not a hair's breadth.
                        const ESCAPE_RANGE: f32 = 145.0;
                        if npc_pos.distance(tp) > ESCAPE_RANGE {
                            // Dodged — the rival lost the thread. Fizzle cleanly and put it on a short
                            // cooldown so it re-pursues rather than instantly re-arming from here. A
                            // downbeat reroute holds it off a beat longer (the "big save" version).
                            self.npc_trains[i].steal_threat = 0.0;
                            self.npc_trains[i].steal_cooldown = if downbeat { 2.0 } else { 1.4 };
                            // Strike resolved (you won it): the predator falls back to lurking —
                            // patience rebuilds from zero before it can commit another intercept.
                            self.npc_trains[i].hunt_committed = false;
                            self.npc_trains[i].stalk_patience = 0.0;
                            self.steals_dodged += 1;
                            // Flip the reroute into offense, mirroring the tool parry (try_defend_steal):
                            // a clean juke leaves the rival strung out and exposed, so mark it for revenge
                            // and open a counter-steal window — thread its line inside the window and the
                            // steal-back pays the revenge bonus (ROADMAP "you steal, they steal back").
                            // The dodge is a *positioning* skill (the geometry escape always works),
                            // unlike the parry's *timing* skill, so the window always opens — but TIMING
                            // scales how long you get to cash it: a downbeat reroute opens the full window
                            // (the big save), on-beat a good one, off-beat a short one. Hitting the beat
                            // still pays without gating the counter on RNG-fragile frame timing
                            // (INSPIRATION.md item 2, "keys as drum pads").
                            self.npc_trains[i].revenge_timer = if downbeat {
                                REVENGE_WINDOW
                            } else if on_beat {
                                REVENGE_WINDOW * 0.7
                            } else {
                                REVENGE_WINDOW * 0.5
                            };
                            let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
                            let (label, size) = if downbeat {
                                ("BIG DODGE — DOWNBEAT!".to_string(), 28.0)
                            } else if on_beat {
                                ("DODGED — ON BEAT!".to_string(), 26.0)
                            } else {
                                ("DODGED!".to_string(), 22.0)
                            };
                            self.floating_texts.spawn(
                                label,
                                player_center - Vec2::new(70.0, 50.0),
                                size,
                                [0.5, 1.0, 0.85, 1.0],
                            );
                            // Point the player at the counter-play the reroute opened.
                            self.floating_texts.spawn(
                                "COUNTER — rustle 'em back!".to_string(),
                                player_center - Vec2::new(70.0, 24.0),
                                18.0,
                                [0.45, 1.0, 0.7, 0.95],
                            );
                            if on_beat {
                                // The on-beat reroute is the skill version — reward the clean read.
                                self.groove =
                                    (self.groove + if downbeat { 0.18 } else { 0.12 }).min(1.0);
                                self.beat_streak = (self.beat_streak + 1).min(99);
                                self.on_beat_flash =
                                    (self.on_beat_flash + if downbeat { 0.5 } else { 0.3 }).min(0.85);
                            }
                            if self.catch_shockwaves.len() < 48 {
                                self.catch_shockwaves.push((tp, 0.0, [0.5, 1.0, 0.85]));
                            }
                            continue; // thread broken — skip the tremble/snap for this rival
                        }
                    }
                    self.npc_trains[i].steal_threat -= dt;
                    // Cap the cut to a recoverable bite: take at most STEAL_MAX_LINKS off the tail,
                    // and never more than half the chain, so a mid-chain thread can't wipe the whole
                    // train in one hit. Clamping the latched target UP (deeper toward the tail) means
                    // the rival grabs fewer links — the trembling tell below and the snap below both
                    // read from this same capped index, so the telegraph shows exactly what's at risk.
                    let max_take = (self.chain_count / 2).max(1).min(STEAL_MAX_LINKS);
                    let cut_floor = self.chain_count.saturating_sub(max_take).max(1);
                    let splice_idx = self.npc_trains[i].steal_target.max(cut_floor);
                    for crab in self.crabs.iter_mut() {
                        if crab.caught && crab.chain_index.map_or(false, |idx| idx >= splice_idx) {
                            crab.spooked_timer = crab.spooked_timer.max(0.22); // trembling "AT RISK" tell
                        }
                    }
                    let telegraph_shown = self.npc_trains[i].steal_threat < STEAL_FUSE - 0.12;
                    let fire = self.npc_trains[i].steal_threat <= 0.0 || (on_beat && telegraph_shown);
                    if fire {
                        self.npc_trains[i].steal_threat = 0.0;
                        // Collect the stolen types before mutating crabs
                        let mut stolen_types: Vec<CrabType> = Vec::new();
                        let mut stolen_count = 0usize;
                        for crab in self.crabs.iter_mut() {
                            if crab.caught
                                && crab.chain_index.map_or(false, |idx| idx >= splice_idx)
                            {
                                crab.caught = false;
                                crab.chain_index = None;
                                crab.fleeing = false;
                                crab.spooked_timer = 1.0;
                                // Cartoony startled hop: scale-pop then fly toward the NPC.
                                crab.join_pulse = 1.0;
                                let toward = (npc_pos - crab.pos).normalize_or_zero();
                                crab.vel = toward * 200.0;
                                crab.vel.y -= 90.0; // brief upward arc before snapping over
                                stolen_types.push(crab.crab_type);
                                stolen_count += 1;
                            }
                        }
                        if stolen_count > 0 {
                            self.chain_count = self.chain_count.saturating_sub(stolen_count);
                            self.crabs_stolen_by_npc += stolen_count;
                            self.max_single_steal_by_npc =
                                self.max_single_steal_by_npc.max(stolen_count);
                            self.steal_loss_sfx = true; // play the descending loss sting (has no ctx here)
                            self.npc_trains[i].follower_types.extend(stolen_types);
                            self.npc_trains[i].steal_cooldown = 2.2;
                            // Strike resolved (it won it): sated, the predator falls back to lurking —
                            // patience rebuilds from zero before the next stalk→strike cycle.
                            self.npc_trains[i].hunt_committed = false;
                            self.npc_trains[i].stalk_patience = 0.0;
                            // Mark the culprit for revenge: chase it down and rustle the crabs back
                            // inside the window for a bonus, so losing crabs opens a duel (ROADMAP
                            // "you steal, they steal back") rather than a flat tax.
                            self.npc_trains[i].revenge_timer = REVENGE_WINDOW;
                            // Visual + audio feedback — this is the key threat moment
                            let npc_name = self.npc_trains[i].name.clone();
                            let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
                            self.floating_texts.spawn(
                                format!("{} stole {} crabs!", npc_name, stolen_count),
                                player_center - Vec2::new(110.0, 55.0),
                                30.0,
                                [0.96, 0.72, 0.16, 1.0],
                            );
                            // A beat below the loss text: point the player at the counter-play.
                            self.floating_texts.spawn(
                                "REVENGE — chase them down!".to_string(),
                                player_center - Vec2::new(110.0, 20.0),
                                20.0,
                                [0.45, 1.0, 0.7, 0.95],
                            );
                            self.screen_shake = self.screen_shake.max(10.0);
                            self.zoom_punch = self.zoom_punch.max(0.08);
                            self.groove = (self.groove - 0.15).max(0.0);
                            self.beat_streak = self.beat_streak.saturating_sub(2);
                            // Shockwave at the splice point so the cut reads on screen
                            if self.catch_shockwaves.len() < 48 {
                                self.catch_shockwaves
                                    .push((npc_pos, 0.0, [0.96, 0.72, 0.16]));
                            }
                        }
                    }
                }
            } else if self.npc_trains[i].steal_threat > 0.0 {
                // Cooldown started, or the chain was banked/snapped out from under the threat —
                // let any armed telegraph lapse cleanly so a stale target can't fire later.
                self.npc_trains[i].steal_threat = 0.0;
            }

            // --- Steal to win: thread YOUR head through a rival's line to rustle it back --------
            // The mirror of the rival's splice above (INSPIRATION.md "The core steal mechanic"):
            // when the player's head crosses a rival's body, the rival splices at the crossing and
            // its back section (that follower → tail) snaps onto YOUR conga line as caught crabs.
            // This is the "Steal to win" verb — the whole prototype has been scaffolding toward it.
            // Rhythmic: crossing ON the beat pays a groove surge + bigger score (skill ceiling).
            if self.player_steal_cooldown <= 0.0 && !self.npc_trains[i].follower_types.is_empty() {
                const P_STEAL_RANGE: f32 = 54.0;
                const P_STEAL_RANGE_SQ: f32 = P_STEAL_RANGE * P_STEAL_RANGE;
                // Follower fi sits at path_history[(fi+1)*STEPS] (same layout draw_npc_conga_train
                // uses). Find the earliest (closest-to-leader) follower the player head is within
                // range of — splicing there takes the largest tail section, like the rival does.
                const STEPS: usize = 14;
                let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
                let mut splice_at: Option<usize> = None;
                for fi in 0..self.npc_trains[i].follower_types.len() {
                    if let Some(&fpos) = self.npc_trains[i].path_history.get((fi + 1) * STEPS) {
                        if player_center.distance_squared(fpos) < P_STEAL_RANGE_SQ {
                            splice_at = Some(fi);
                            break;
                        }
                    }
                }
                if let Some(fi) = splice_at {
                    let on_beat = self.on_beat_now();
                    // Revenge: is this the rival that just spliced your tail? If so the steal-back
                    // closes a duel and pays a bonus (cleared so it only lands once per marker).
                    let revenge = self.npc_trains[i].revenge_timer > 0.0;
                    self.npc_trains[i].revenge_timer = 0.0;
                    // split_off(fi) leaves 0..fi on the rival and returns the back section fi..tail.
                    let stolen = self.npc_trains[i].follower_types.split_off(fi);
                    let stolen_count = stolen.len();
                    let mut rng = crate::rng::rng();
                    for (k, ct) in stolen.into_iter().enumerate() {
                        // Spawn each rustled crab at its old follower slot, flying toward the player.
                        let old_pos = self.npc_trains[i]
                            .path_history
                            .get((fi + k + 1) * STEPS)
                            .copied()
                            .unwrap_or(self.npc_trains[i].leader_pos);
                        let toward = (player_center - old_pos).normalize_or_zero();
                        let mut vel = toward * 230.0;
                        vel.y -= 80.0; // brief upward arc before snapping into line
                        let ci = self.chain_count;
                        let mut stolen_crab = spawn_stolen_crab(old_pos, vel, ct, ci, &mut rng);
                        if self.king_crab_count > 0 {
                            stolen_crab.chain_color = Some(self.conga_tint);
                        }
                        self.crabs.push(stolen_crab);
                        self.chain_count += 1;
                    }
                    self.player_steal_cooldown = 2.2;
                    // Monotonic tally so the bot playtest can assert the steal-back fired without
                    // racing the live chain count (which banks/snaps drop back to zero).
                    self.crabs_stolen_by_player += stolen_count;
                    self.steal_gain_sfx = true; // play the rising triumphant sting (has no ctx here)
                    // Reward: stealing feeds the groove (harder on the beat) and banks score. A
                    // revenge steal-back (off a rival that just spliced you) pays extra — the payoff
                    // for closing the loop, so the exchange feels like a fight you won.
                    if revenge {
                        self.revenge_steals += 1;
                    }
                    let mut score_mult = if on_beat { 3 } else { 2 };
                    let mut groove_gain = if on_beat { 0.22 } else { 0.10 };
                    if revenge {
                        score_mult += 2; // stack the revenge bonus on top of the on-beat bonus
                        groove_gain += 0.14;
                    }
                    self.score += stolen_count * score_mult;
                    self.groove = (self.groove + groove_gain).min(1.0);
                    if on_beat {
                        self.beat_streak = (self.beat_streak + 1).min(99);
                        self.on_beat_flash = (self.on_beat_flash + 0.4).min(0.8);
                        self.beat_intensity = (self.beat_intensity + 1.0).min(2.0);
                    }
                    // Juice — the triumphant counterpart to losing crabs.
                    let npc_name = self.npc_trains[i].name.clone();
                    let label = if revenge {
                        format!("REVENGE! GOT {} BACK!", stolen_count)
                    } else if on_beat {
                        format!("RUSTLED {} — ON BEAT!", stolen_count)
                    } else {
                        format!("RUSTLED {} from {}!", stolen_count, npc_name)
                    };
                    self.floating_texts.spawn(
                        label,
                        player_center - Vec2::new(90.0, 60.0),
                        if revenge { 34.0 } else { 30.0 },
                        [0.35, 1.0, 0.55, 1.0],
                    );
                    self.screen_shake = self.screen_shake.max(if on_beat { 10.0 } else { 6.0 });
                    self.zoom_punch = self.zoom_punch.max(if on_beat { 0.08 } else { 0.05 });
                    if self.catch_shockwaves.len() < 48 {
                        self.catch_shockwaves
                            .push((player_center, 0.0, [0.35, 1.0, 0.55]));
                    }
                }
            }

            // --- Free crab collection --------------------------------------------------------
            // NPCs act like players: they pick up free crabs they wander past.
            self.npc_trains[i].catch_cooldown = (self.npc_trains[i].catch_cooldown - dt).max(0.0);
            if self.npc_trains[i].catch_cooldown <= 0.0 {
                const CATCH_RANGE: f32 = 52.0;
                const CATCH_RANGE_SQ: f32 = CATCH_RANGE * CATCH_RANGE;
                let npc_pos = self.npc_trains[i].leader_pos;
                let caught = self.crabs.iter_mut().find(|c| {
                    !c.caught
                        && !c.is_boss()
                        && c.is_catchable()
                        && npc_pos.distance_squared(c.pos) < CATCH_RANGE_SQ
                });
                if let Some(crab) = caught {
                    let ct = crab.crab_type;
                    // Teleport the crab far off-screen rather than marking it caught=true with
                    // no chain_index — that would corrupt rendering InstanceArray capacity checks.
                    crab.pos = Vec2::new(-9999.0, -9999.0);
                    crab.vel = Vec2::ZERO;
                    crab.fleeing = false;
                    self.npc_trains[i].follower_types.push(ct);
                    self.npc_trains[i].catch_cooldown = 0.7;
                }
            }
        }

        // Audio: use the loudest (nearest) NPC train for the rumble volume — store on [0].
        let max_vol = self
            .npc_trains
            .iter()
            .map(|t| t.target_vol)
            .fold(0.0_f32, f32::max);
        if !self.npc_trains.is_empty() {
            self.npc_trains[0].target_vol = max_vol;
        }

        // Snapshot the player's velocity BEFORE the separation pass below damps it, so the clash
        // gate can read a genuine charge — the separation shaves the approaching component, which
        // would otherwise mask a real ram as a slow graze.
        let player_vel_pre = self.player_vel;

        // --- Continuous overlap separation: prevent player from phasing inside NPC leaders ----
        // Regardless of cooldown, push the player and NPC apart whenever they overlap so
        // you can't stand inside a King Crab — the clash is painful but crisp, not a merge.
        {
            let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            for npc in self.npc_trains.iter_mut() {
                let col_r = CRAB_SIZE * npc.leader_scale * 1.2 + PLAYER_SIZE * 0.5;
                let dist = npc.leader_pos.distance(player_center);
                if dist < col_r && dist > 0.1 {
                    // Positional correction: push both apart so they don't phase through each other
                    let overlap = col_r - dist;
                    let dir = (player_center - npc.leader_pos).normalize_or_zero();
                    self.player_pos += dir * overlap * 0.6;
                    npc.leader_pos -= dir * overlap * 0.4;
                    // Velocity damping so they slide off each other instead of jittering
                    let rel_vel = self.player_vel - npc.leader_vel;
                    let sep_speed = rel_vel.dot(dir);
                    if sep_speed < 0.0 {
                        self.player_vel -= dir * sep_speed * 0.8;
                        npc.leader_vel += dir * sep_speed * 0.8;
                    }
                }
            }
        }

        // --- Player-vs-NPC-leader collision: a timed body-slam counter-attack ------------------
        // Ramming a rival's leader is a deliberate counter-attack — but WHEN you hit it is the whole
        // skill (Carl's #164: the clash felt unforgiving and it wasn't obvious what to time). The
        // rule is now legible and on-beat, matching the parry: RAM ON THE BEAT and you win the
        // exchange (a POWER CLASH — you barge through, stun the King, scatter its followers, keep
        // your own train, and open a revenge steal-back); ram OFF the beat and it's the old painful
        // mutual bounce (a MISTIMED CLASH — you lose tail crabs too). The `on_beat_defend` window is
        // the forgiving reactive one (0.12s, same as the parry) so a slightly-early/late ram still
        // reads on-beat — this is the "widen + clarify" #164 asked for, not a skill removal: the
        // clash telegraph ring (see draw_npc_conga_train) flashes the exact "RAM NOW" frame.
        if self.king_splice_cooldown <= 0.0 {
            let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
            let mut clash_npc: Option<usize> = None;
            // A clash is a DELIBERATE ram, not an incidental graze. Merely brushing a roaming King
            // while you navigate — or being nudged into one by the separation push above — must not
            // silently cost you tail crabs off the beat: with no keypress in play, only your heading
            // decides a clash, so that was the opaque "what was I even timing?" punishment #164 called
            // out. Gate the whole clash on intent: you must be moving with real pace AND driving mostly
            // INTO the King (velocity read from before the separation damping, so a true charge can't be
            // masked). A glancing bump just bounces off via the separation above — no POWER/MISTIMED,
            // no cost, no cooldown burned.
            let ram_speed = player_vel_pre.length();
            for (ni, npc) in self.npc_trains.iter().enumerate() {
                let col_r = CRAB_SIZE * npc.leader_scale * 1.2 + PLAYER_SIZE * 0.5;
                if npc.leader_pos.distance(player_center) < col_r {
                    let toward = (npc.leader_pos - player_center).normalize_or_zero();
                    if ram_speed > CLASH_RAM_MIN_SPEED && player_vel_pre.dot(toward) > ram_speed * 0.5
                    {
                        clash_npc = Some(ni);
                        break;
                    }
                }
            }
            if let Some(ni) = clash_npc {
                self.king_splice_cooldown = 2.0;
                let on_beat = self.on_beat_defend();
                let away_from_npc =
                    (player_center - self.npc_trains[ni].leader_pos).normalize_or_zero();
                let npc_pos = self.npc_trains[ni].leader_pos;
                let npc_name = self.npc_trains[ni].name.clone();
                if on_beat {
                    // POWER CLASH — you win the exchange. Barge through: a big stun-knockback on the
                    // King, a smaller recoil on you. You keep your whole train (no tail loss), scatter
                    // MORE of the King's followers, and mark it for a revenge steal-back (green ring).
                    self.player_vel += away_from_npc * 240.0;
                    self.npc_trains[ni].leader_vel += -away_from_npc * 460.0;
                    self.npc_trains[ni].idle_timer = self.npc_trains[ni].idle_timer.max(0.9);
                    self.npc_trains[ni].steal_cooldown = self.npc_trains[ni].steal_cooldown.max(3.0);
                    self.npc_trains[ni].steal_threat = 0.0; // cancel any splice it was winding up
                    self.npc_trains[ni].revenge_timer = REVENGE_WINDOW; // "chase me — rustle 'em back"
                    // Punchy but triumphant feedback.
                    self.screen_shake = self.screen_shake.max(14.0);
                    self.zoom_punch = self.zoom_punch.max(0.11);
                    self.hitstop_timer = self.hitstop_timer.max(0.12);
                    // Reward the clean timing like an on-beat parry: groove, streak, beat flash.
                    self.groove = (self.groove + 0.18).min(1.0);
                    self.beat_streak = (self.beat_streak + 1).min(99);
                    self.on_beat_flash = (self.on_beat_flash + 0.5).min(0.9);
                    self.beat_intensity = (self.beat_intensity + 1.0).min(2.0);
                    // Land the win as a drum hit: the rising triumphant "gain" sting (#164 — the
                    // clash had rich visuals but was silent). A POWER CLASH scatters the King's
                    // followers and opens a revenge steal-back, so the gain sting fits exactly and
                    // makes "I won the exchange" read by ear too. No ctx here, so latch the flag —
                    // the audio pass plays it (same pattern as the steal-back sting above).
                    self.steal_gain_sfx = true;
                    // The King loses its last 2–3 followers — they scatter as catchable spoils.
                    let npc_lose = 3.min(self.npc_trains[ni].follower_types.len());
                    for k in 0..npc_lose {
                        self.npc_trains[ni].follower_types.pop();
                        let scatter_angle =
                            k as f32 * 2.1 + away_from_npc.y.atan2(away_from_npc.x);
                        let scatter_dir = Vec2::new(scatter_angle.cos(), scatter_angle.sin());
                        if self.catch_shockwaves.len() < 48 {
                            self.catch_shockwaves.push((
                                npc_pos + scatter_dir * 30.0,
                                0.0,
                                [0.4, 1.0, 0.85],
                            ));
                        }
                    }
                    self.floating_texts.spawn(
                        format!("POWER CLASH! {} reeling!", npc_name),
                        player_center - Vec2::new(96.0, 68.0),
                        32.0,
                        [0.4, 1.0, 0.85, 1.0],
                    );
                    self.floating_texts.spawn(
                        "COUNTER — rustle 'em back!".to_string(),
                        player_center - Vec2::new(96.0, 40.0),
                        20.0,
                        [0.45, 1.0, 0.7, 0.95],
                    );
                    self.particle_system
                        .spawn_milestone_fireworks(player_center, 10, &mut crate::rng::rng());
                } else {
                    // MISTIMED CLASH — the old painful mutual bounce. Ram off the beat and you take a
                    // hit too: both sides recoil and you shed 1–2 tail crabs.
                    self.player_vel += away_from_npc * 380.0;
                    self.npc_trains[ni].leader_vel += -away_from_npc * 280.0;
                    self.screen_shake = self.screen_shake.max(16.0);
                    self.zoom_punch = self.zoom_punch.max(0.10);
                    self.hitstop_timer = self.hitstop_timer.max(0.12);
                    // Player loses tail crabs (1–2), NPC loses some followers — mutual damage
                    let player_lose = 2.min(self.chain_count.saturating_sub(1));
                    let mut released = 0;
                    for crab in self.crabs.iter_mut().rev() {
                        if released >= player_lose {
                            break;
                        }
                        if crab.caught {
                            if let Some(idx) = crab.chain_index {
                                if idx > 0 {
                                    crab.caught = false;
                                    crab.chain_index = None;
                                    crab.fleeing = true;
                                    crab.spooked_timer = 2.5;
                                    crab.join_pulse = 1.0; // startled pop
                                    let away = (crab.pos - player_center).normalize_or_zero();
                                    crab.vel = away * 250.0;
                                    crab.vel.y -= 70.0; // hop upward before scattering out
                                    if self.catch_shockwaves.len() < 48 {
                                        self.catch_shockwaves
                                            .push((crab.pos, 0.0, [1.0, 0.6, 0.2]));
                                    }
                                    released += 1;
                                }
                            }
                        }
                    }
                    self.chain_count = self.chain_count.saturating_sub(released);
                    let taunt = crate::rival_taunts::clash_taunt(
                        &npc_name,
                        released,
                        self.chain_count,
                        &mut crate::rng::rng(),
                    );
                    self.floating_texts.spawn(
                        format!("{}: \"{}\"", npc_name, taunt),
                        npc_pos - Vec2::new(110.0, 78.0),
                        22.0,
                        [1.0, 0.82, 0.28, 1.0],
                    );
                    // NPC loses its last 1–2 followers — they scatter as free crabs (Sonic rings)
                    let npc_lose = 2.min(self.npc_trains[ni].follower_types.len());
                    for k in 0..npc_lose {
                        self.npc_trains[ni].follower_types.pop();
                        let scatter_angle =
                            k as f32 * std::f32::consts::PI + away_from_npc.y.atan2(away_from_npc.x);
                        let scatter_dir = Vec2::new(scatter_angle.cos(), scatter_angle.sin());
                        if self.catch_shockwaves.len() < 48 {
                            self.catch_shockwaves.push((
                                npc_pos + scatter_dir * 30.0,
                                0.0,
                                [0.96, 0.72, 0.16],
                            ));
                        }
                    }
                    // Groove penalty for a mistimed head-on hit — you should have hit the beat.
                    self.groove = (self.groove - 0.20).max(0.0);
                    self.beat_streak = self.beat_streak.saturating_sub(1);
                    // Voice the botched ram: the descending "loss" sting — you shed tail crabs on a
                    // mistimed clash, exactly what that sting already means, so the ear reads the
                    // mistake as clearly as the eye. Latched for the audio pass (no ctx here).
                    self.steal_loss_sfx = true;
                    self.floating_texts.spawn(
                        format!("MISTIMED CLASH — {}!", npc_name),
                        player_center - Vec2::new(80.0, 65.0),
                        32.0,
                        [1.0, 0.5, 0.15, 1.0],
                    );
                    self.particle_system
                        .spawn_milestone_fireworks(player_center, 8, &mut crate::rng::rng());
                }
            }
        }

        // --- Rival-vs-rival splicing: the bigger train slices a smaller rival's back half -----
        // The whole-beach ecology step (ROADMAP ★ headline): the same reverse-Snake crossing rule
        // that lets a rival splice YOUR back half now lets the bigger train splice a *smaller* rival's
        // back half when its leader threads through the smaller one's follower line. No new verb — it
        // reuses the player-steal geometry (leader within range of a follower slot on path_history)
        // and the same recoverable-bite cap, so the beach churns on its own: trains gain and lose
        // crabs without the player, a genuine ecosystem (agar.io + Rain World). The pecking order
        // emerges from a purely local rule — only a train with MORE followers can bully a smaller one,
        // so big trains visibly eat small ones. It's made legible (a callout + shockwave at the splice)
        // so the player can read the fight and swoop in to rustle the winner later.
        {
            const STEPS: usize = 14; // matches draw_npc_conga_train / player-steal follower spacing
            const RIVAL_STEAL_RANGE: f32 = 56.0;
            const RIVAL_STEAL_RANGE_SQ: f32 = RIVAL_STEAL_RANGE * RIVAL_STEAL_RANGE;
            let n_trains = self.npc_trains.len();
            for thief in 0..n_trains {
                // --- Armed: wind the telegraph down, then SNAP on the beat ----------------------
                // A rival-vs-rival splice arms a fuse when its leader threads a smaller rival, then
                // fires ON the beat (or on fuse expiry) — the exact rule the player-facing rival
                // steal uses. So the whole beach's steals now land as rhythmic drum hits instead of
                // firing the instant two leaders happen to cross (INSPIRATION "the beat is the
                // mechanic"), and the gold "predator closing" tell (#135) reads as a real wind-up.
                if self.npc_trains[thief].rival_steal_threat > 0.0 {
                    self.npc_trains[thief].rival_steal_threat -= dt;
                    let victim = self.npc_trains[thief].rival_steal_victim;
                    let cut_from = self.npc_trains[thief].rival_steal_cut_from;
                    // Re-validate the snapshot: in bounds, not self, and the victim still has a back
                    // section past the cut. A train despawning or emptied mid-fuse fizzles cleanly
                    // here instead of mis-splicing or panicking on split_off.
                    let valid = victim < self.npc_trains.len()
                        && victim != thief
                        && self.npc_trains[victim].follower_types.len() > cut_from;
                    if !valid {
                        self.npc_trains[thief].rival_steal_threat = 0.0;
                        continue;
                    }
                    // Snap once the telegraph has shown for a moment AND we're on the beat, or on
                    // fuse expiry as the guaranteed fallback — so the theft always lands within
                    // STEAL_FUSE even off-beat (this is what keeps the headless bot deterministic,
                    // exactly like the player-facing steal's `steal_threat <= 0.0` fallback).
                    let telegraph_shown =
                        self.npc_trains[thief].rival_steal_threat < STEAL_FUSE - 0.12;
                    let on_beat_snap = on_beat && telegraph_shown;
                    let fire = self.npc_trains[thief].rival_steal_threat <= 0.0 || on_beat_snap;
                    if !fire {
                        continue;
                    }
                    self.npc_trains[thief].rival_steal_threat = 0.0;
                    let splice_pos = self.npc_trains[thief].rival_steal_splice_pos;
                    let mut stolen = self.npc_trains[victim].follower_types.split_off(cut_from);
                    let stolen_count = stolen.len();
                    if stolen_count > 0 {
                        self.npc_trains[thief].rival_steal_cooldown = 3.0;
                        self.rival_vs_rival_steals += stolen_count;
                        // Swoopable spoils (ROADMAP step 3, agar.io "let the big ones fight, then eat
                        // the crumbs"): the loser doesn't hand the winner a clean pickpocket — the
                        // collision knocks roughly a third of the cut (at least one, whenever ≥2 were
                        // taken) *loose* as free catchable crabs bursting from the splice, so the player
                        // can swoop into a rival-vs-rival collision and rustle the spilled crumbs. The
                        // thief still nets the majority, so the pecking order (big trains eat small ones)
                        // holds and the beach doesn't collapse to one mega-train.
                        let mut rng = crate::rng::rng();
                        let spill = if stolen_count >= 2 {
                            (stolen_count / 3).max(1)
                        } else {
                            0
                        };
                        // Cap the world's free-crab load so a churn of collisions can't shove the run
                        // toward the overwhelmed game-over; the leftover stays with the thief.
                        let room = 150usize.saturating_sub(self.crabs.len());
                        let spill = spill.min(room);
                        for ct in stolen.drain(stolen.len() - spill..) {
                            let angle: f32 = rng.random_range(0.0..std::f32::consts::TAU);
                            let mut vel = Vec2::new(angle.cos(), angle.sin()) * rng.random_range(120.0..200.0);
                            vel.y -= 60.0; // a slight upward arc before it settles into the herd
                            let jitter = Vec2::new(rng.random_range(-14.0..14.0), rng.random_range(-14.0..14.0));
                            self.crabs
                                .push(spawn_scattered_crab(splice_pos + jitter, vel, ct, &mut rng));
                            self.rival_spill_crabs += 1;
                        }
                        // Whatever survived the spill goes to the winner.
                        self.npc_trains[thief].follower_types.extend(stolen);
                        // Legibility (ROADMAP step 3 "make it legible and swoopable"): name the theft
                        // at the splice point and pop a golden shockwave so the player reads which train
                        // just grew, then can swoop in and rustle the fattened winner — or the crumbs.
                        // An on-beat snap flashes a brighter gold than an off-beat (fuse-expiry) one, so
                        // a clean rhythmic steal reads as the stronger hit — the beat rewards the eye too.
                        let thief_name = self.npc_trains[thief].name.clone();
                        let victim_name = self.npc_trains[victim].name.clone();
                        let callout = if spill > 0 {
                            format!("{} rustled {} from {} — {} spilled loose!", thief_name, stolen_count, victim_name, spill)
                        } else {
                            format!("{} rustled {} from {}!", thief_name, stolen_count, victim_name)
                        };
                        self.floating_texts.spawn(
                            callout,
                            splice_pos - Vec2::new(90.0, 30.0),
                            22.0,
                            [1.0, 0.78, 0.25, 1.0],
                        );
                        if self.catch_shockwaves.len() < 48 {
                            let ring = if on_beat_snap {
                                [1.0, 0.86, 0.4] // brighter gold on the beat
                            } else {
                                [1.0, 0.78, 0.25]
                            };
                            self.catch_shockwaves.push((splice_pos, 0.0, ring));
                        }
                        // Audible ecology (INSPIRATION.md "audio IS the radar"): latch the splice
                        // position so the audio pass (which has `ctx`) can play a position-panned,
                        // distance-faded theft clack — a far-off rival steal becomes a faint
                        // directional tick the player looks toward and swoops into for the crumbs.
                        self.rival_steal_sfx = Some(splice_pos);
                    }
                    continue;
                }
                if self.npc_trains[thief].rival_steal_cooldown > 0.0 {
                    continue;
                }
                let thief_pos = self.npc_trains[thief].leader_pos;
                let thief_len = self.npc_trains[thief].follower_types.len();
                // Find a smaller victim whose follower line the thief's leader is threading. Take the
                // earliest (closest-to-leader) follower in range so the cut takes the largest section,
                // exactly like the player's steal-back does against a rival.
                let mut hit: Option<(usize, usize)> = None; // (victim, splice_fi)
                for victim in 0..n_trains {
                    if victim == thief {
                        continue;
                    }
                    let vlen = self.npc_trains[victim].follower_types.len();
                    if vlen == 0 || vlen >= thief_len {
                        continue; // only a strictly bigger train bullies a smaller one
                    }
                    for fi in 0..vlen {
                        if let Some(&fpos) =
                            self.npc_trains[victim].path_history.get((fi + 1) * STEPS)
                        {
                            if thief_pos.distance_squared(fpos) < RIVAL_STEAL_RANGE_SQ {
                                hit = Some((victim, fi));
                                break;
                            }
                        }
                    }
                    if hit.is_some() {
                        break;
                    }
                }
                if let Some((victim, fi)) = hit {
                    // Cap the cut to a recoverable bite (STEAL_MAX_LINKS, the same cap the rival uses
                    // against you) so the beach churns without collapsing into one mega-train — the
                    // front of the victim's line always survives. cut_from clamps toward the tail.
                    let vlen = self.npc_trains[victim].follower_types.len();
                    let cut_from = fi.max(vlen.saturating_sub(STEAL_MAX_LINKS));
                    let splice_pos = self.npc_trains[victim]
                        .path_history
                        .get((cut_from + 1) * STEPS)
                        .copied()
                        .unwrap_or(thief_pos);
                    // ARM the telegraph — don't splice yet. The back section is snapshotted so the
                    // snap fires from the same link on the beat (or fuse expiry) even if the thief's
                    // leader drifts a little off it during the wind-up. A dim gold "winding up" ring
                    // marks the splice point up close; the bright gold snap ring + callout come at
                    // fire — arm→snap now reads on the beat on top of the far-off predator line (#135).
                    self.npc_trains[thief].rival_steal_threat = STEAL_FUSE;
                    self.npc_trains[thief].rival_steal_victim = victim;
                    self.npc_trains[thief].rival_steal_cut_from = cut_from;
                    self.npc_trains[thief].rival_steal_splice_pos = splice_pos;
                    if self.catch_shockwaves.len() < 48 {
                        self.catch_shockwaves.push((splice_pos, 0.0, [0.7, 0.5, 0.15]));
                    }
                }
            }
        }

        // --- NPC-vs-NPC leader collisions: they bounce off each other too --------------------
        for i in 0..self.npc_trains.len() {
            for j in (i + 1)..self.npc_trains.len() {
                let pi = self.npc_trains[i].leader_pos;
                let pj = self.npc_trains[j].leader_pos;
                let si = self.npc_trains[i].leader_scale;
                let sj = self.npc_trains[j].leader_scale;
                let col_r = CRAB_SIZE * (si + sj) * 0.8;
                if pi.distance(pj) < col_r {
                    let dir = (pi - pj).normalize_or_zero();
                    self.npc_trains[i].leader_vel += dir * 200.0;
                    self.npc_trains[j].leader_vel -= dir * 200.0;
                    // Each loses one follower (if they have any)
                    if !self.npc_trains[i].follower_types.is_empty() {
                        self.npc_trains[i].follower_types.pop();
                    }
                    if !self.npc_trains[j].follower_types.is_empty() {
                        self.npc_trains[j].follower_types.pop();
                    }
                }
            }
        }
    }
}
