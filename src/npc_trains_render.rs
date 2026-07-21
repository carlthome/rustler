//! Rendering for rival NPC King Crab trains: draws each train's followers, the golden King
//! leader with its halo, and the beat-synced telegraphs (hunt-intent warning line, rival-vs-rival
//! predator line, armed-steal DEFEND ring, clash RAM cue, revenge chase marker) plus the King's
//! name banner. Extracted out of `npc_trains.rs` so the wandering AI/update logic and the draw
//! pass live in separate modules — same method, same behaviour, split by subsystem.

use ggez::glam::Vec2;
use ggez::graphics::{Canvas, Color, DrawParam, Text};
use ggez::{Context, GameResult};

use crate::constants::*;
use crate::enemies::{BossCharge, CrabType, EnemyCrab};
use crate::graphics::{cached_stroke_circle, draw_crab, unit_circle};
use crate::hud_cache::NPC_NAME_CACHE;
use crate::state::MainState;

impl MainState {
    pub(crate) fn draw_npc_conga_train(&self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        let t = self.time_elapsed;

        // Followers trail the leader using path_history. Each follower sits 14 history-steps
        // behind the previous one (history is sampled ~every 6px, so ~84px spacing between crabs).
        const STEPS: usize = 14;

        // Cull margin for the crab body draw only — the halo/telegraph/name-banner overlays below
        // stay unconditional (they're cheap and some intentionally reach toward the player from an
        // off-screen rival), so this only skips the expensive per-crab geometry when it's fully
        // off camera. A rival train can grow unboundedly by stealing crabs, so this matters more
        // the longer a session runs.
        let view_min = self.camera_origin - Vec2::splat(CULL_MARGIN);
        let view_max = self.camera_origin + Vec2::new(self.width, self.height) + Vec2::splat(CULL_MARGIN);
        let in_view = |p: Vec2| {
            p.x >= view_min.x && p.x <= view_max.x && p.y >= view_min.y && p.y <= view_max.y
        };

        for npc in &self.npc_trains {
            // Draw followers back-to-front so the leader renders on top.
            for i in (0..npc.follower_types.len()).rev() {
                let hist_idx = (i + 1) * STEPS;
                let pos = match npc.path_history.get(hist_idx) {
                    Some(&p) => p,
                    None => continue,
                };
                if !in_view(pos) {
                    continue;
                }
                let bob = (t * 5.5 + i as f32 * 0.85).sin() * 3.5;
                let crab_type = npc.follower_types[i];
                let fake = EnemyCrab {
                    pos,
                    vel: Vec2::ZERO,
                    speed: 0.0,
                    caught: true,
                    chain_index: Some(i),
                    scale: npc.leader_scale * 0.33, // followers scale with leader tier
                    spawn_time: 999.0,
                    crab_type,
                    chain_color: None,
                    spooked_timer: 0.0,
                    beat_phase_offset: i as f32 * 0.4,
                    join_pulse: 0.0,
                    fleeing: false,
                    facing_angle: 0.0,
                    in_flashlight: false,
                    startle_timer: 0.0,
                    charm_timer: 0.0,
                    answering_call: 0.0,
                    boss_health: 0.0,
                    boss_max_health: 0.0001,
                    enraged: false,
                    charge_state: BossCharge::Idle,
                    charge_cooldown: 0.0,
                    latch_timer: 0.0,
                    panic_amp: 1.0,
                    magnet_snared: 0.0,
                    magnet_lured: 0.0,
                    thief_lured: 0.0,
                    magnet_charged: 0.0,
                    slingshot_spent: 0.0,
                    stun_timer: 0.0,
                    host_swap_timer: 0.0,
                    surge_timer: 0.0,
                    entranced: 0.0,
                };
                let beat = (t * 4.0 + i as f32 * 0.5).sin().abs();
                draw_crab(
                    ctx,
                    canvas,
                    &fake,
                    pos + Vec2::new(0.0, -bob),
                    beat,
                    0.0,
                    bob.max(0.0),
                    0.0,
                    t,
                )?;
            }

            // King Crab leader — large, golden, unmistakeable.
            let leader_bob = (t * 4.2).sin() * 6.0;
            let facing = if npc.leader_vel.length_squared() > 4.0 {
                npc.leader_vel.y.atan2(npc.leader_vel.x)
            } else {
                0.0
            };
            if in_view(npc.leader_pos) {
                let king = EnemyCrab {
                    pos: npc.leader_pos,
                    vel: npc.leader_vel,
                    speed: 88.0,
                    caught: false,
                    chain_index: None,
                    scale: npc.leader_scale,
                    spawn_time: 999.0,
                    crab_type: CrabType::Boss,
                    chain_color: None,
                    spooked_timer: 0.0,
                    beat_phase_offset: 0.0,
                    join_pulse: 0.0,
                    fleeing: false,
                    facing_angle: facing,
                    in_flashlight: false,
                    startle_timer: 0.0,
                    charm_timer: 0.0,
                    answering_call: 0.0,
                    boss_health: 0.0,
                    boss_max_health: 0.0001,
                    enraged: false,
                    charge_state: BossCharge::Idle,
                    charge_cooldown: 0.0,
                    latch_timer: 0.0,
                    panic_amp: 1.0,
                    magnet_snared: 0.0,
                    magnet_lured: 0.0,
                    thief_lured: 0.0,
                    magnet_charged: 0.0,
                    slingshot_spent: 0.0,
                    stun_timer: 0.0,
                    host_swap_timer: 0.0,
                    surge_timer: 0.0,
                    entranced: 0.0,
                };
                let king_beat = (t * 4.0).sin().abs();
                draw_crab(
                    ctx,
                    canvas,
                    &king,
                    npc.leader_pos + Vec2::new(0.0, -leader_bob),
                    king_beat,
                    0.0,
                    leader_bob.max(0.0),
                    facing,
                    t,
                )?;
            }

            // A gentle golden halo so the King reads as the leader from across the world.
            let dot = unit_circle(ctx)?;
            for ring in (0..3).rev() {
                let r = 40.0 + ring as f32 * 14.0;
                let a = 0.06 - ring as f32 * 0.015;
                canvas.draw(
                    dot,
                    DrawParam::default()
                        .dest(npc.leader_pos)
                        .scale(Vec2::splat(r))
                        .color(Color::new(1.0, 0.82, 0.2, a)),
                );
            }

            // --- Hunt-intent early warning (peripheral threat language) -----------------------
            // Before a rival gets close enough to ARM a splice (the red DEFEND ring below), it
            // telegraphs *commitment*: while it deliberately routes to thread your back half, a line
            // of beat-marching dots reaches from the King toward the threatened link. This is the
            // early read the steal fight wants — you see a committed rival in time to tighten your
            // line or reroute, instead of only learning once the snap is already armed on top of you
            // (INSPIRATION.md "Legible risk", "peripheral threat language"). Suppressed once armed so
            // it never fights the DEFEND ring for the same frame; dots slide + reset on the beat so
            // the warning itself keeps time with the music.
            if npc.hunt_intent > 0.3 && npc.steal_threat <= 0.0 {
                if let Some(threat_pos) = self.cached_steal_target_pos.or(self.cached_tail_pos) {
                    let to_threat = threat_pos - npc.leader_pos;
                    let len = to_threat.length();
                    if len > 70.0 {
                        let intensity = ((npc.hunt_intent - 0.3) / 0.7).clamp(0.0, 1.0);
                        let dir = to_threat / len;
                        // Keep dots clear of the King body and the targeted crab itself.
                        let start = npc.leader_pos + dir * 34.0;
                        let seg = to_threat - dir * 56.0; // trim both ends
                        let beat_phase =
                            (self.beat_timer / self.beat_interval.max(0.0001)).clamp(0.0, 1.0);
                        let march = 1.0 - beat_phase; // slides 0→1 across the beat, resets on the beat
                        let dot = unit_circle(ctx)?;
                        const DOTS: usize = 4;
                        for d in 0..DOTS {
                            let f = ((d as f32 + march) / DOTS as f32).fract();
                            let p = start + seg * f;
                            let a = (0.55 - f * 0.4).max(0.0) * intensity;
                            let r = 4.5 + (1.0 - f) * 3.5;
                            canvas.draw(
                                dot,
                                DrawParam::default()
                                    .dest(p)
                                    .scale(Vec2::splat(r))
                                    .color(Color::new(1.0, 0.35, 0.12, a)),
                            );
                        }
                    }
                }
            }

            // --- Rival-vs-rival "predator closing" telegraph (gold, King→King) -----------------
            // The whole-beach ecology (ROADMAP ★ step 3 "make it legible and swoopable"): when a bigger
            // King commits to hunting a *smaller* rival, show a distinct GOLD beat-marching line from the
            // hunter toward the prey King, plus a pulsing gold reticle over the marked train. Styled apart
            // from the RED player-hunt line above on purpose — a rival chasing another rival must never
            // read as "you're being hunted." This is the agar.io "watch the big one creep toward the small
            // one" read: the player sees the impending clash from across the field and pre-positions to
            // swoop the crumbs the collision spills (see the rival splice's spill/callout above). Gold ties
            // it to the theft callout + shockwave so the whole rival-vs-rival story shares one colour.
            if let Some(prey_pos) = npc.rival_hunt_target_pos {
                let to_prey = prey_pos - npc.leader_pos;
                let len = to_prey.length();
                if len > 80.0 {
                    let intensity = npc.rival_hunt_intensity.clamp(0.0, 1.0);
                    let dir = to_prey / len;
                    let start = npc.leader_pos + dir * 36.0;
                    let seg = to_prey - dir * 64.0; // trim clear of both Kings
                    let beat_phase =
                        (self.beat_timer / self.beat_interval.max(0.0001)).clamp(0.0, 1.0);
                    let march = 1.0 - beat_phase; // slides 0→1 across the beat, resets on the beat
                    let dot = unit_circle(ctx)?;
                    const DOTS: usize = 4;
                    for d in 0..DOTS {
                        let f = ((d as f32 + march) / DOTS as f32).fract();
                        let p = start + seg * f;
                        // Fade toward the prey end so the line reads as *reaching* for the target.
                        let a = (0.20 + f * 0.35) * intensity;
                        let r = 3.5 + f * 3.5;
                        canvas.draw(
                            dot,
                            DrawParam::default()
                                .dest(p)
                                .scale(Vec2::splat(r))
                                .color(Color::new(1.0, 0.78, 0.25, a)),
                        );
                    }
                    // Pulsing gold reticle over the marked prey King — "this train is next." Swells on
                    // the beat (bigger on the downbeat pulse) so the warning itself keeps time.
                    let pulse = 1.0 + 0.35 * (beat_phase * std::f32::consts::TAU).sin().abs();
                    let ring_r = 26.0 * pulse;
                    canvas.draw(
                        dot,
                        DrawParam::default()
                            .dest(prey_pos)
                            .scale(Vec2::splat(ring_r))
                            .color(Color::new(1.0, 0.72, 0.2, 0.10 * intensity)),
                    );
                    canvas.draw(
                        dot,
                        DrawParam::default()
                            .dest(prey_pos)
                            .scale(Vec2::splat(ring_r * 0.62))
                            .color(Color::new(1.0, 0.85, 0.35, 0.16 * intensity)),
                    );
                }
            }

            // --- Armed-steal DEFEND telegraph -------------------------------------------------
            // While a rival's splice is armed, ring its leader with a beat-synced warning so the
            // player can *read* the parry (ROADMAP: "make contesting it skill"; INSPIRATION.md
            // "Legible risk", "keys as drum pads"). The ring collapses tight onto the rival ON the
            // beat — the "hit now" frame for a Stomp/Wave parry — and springs wide between beats, so
            // it beats like a drum-pad cue. It reddens and thickens as the fuse burns toward the
            // snap, and an on-beat inner flash makes the defend frame unmistakable. Draw-only; the
            // parry itself lives in try_defend_steal.
            if npc.steal_threat > 0.0 {
                let fuse_frac = (npc.steal_threat / STEAL_FUSE).clamp(0.0, 1.0); // 1 armed → 0 snap
                let urgency = 1.0 - fuse_frac; // grows toward the snap
                // Beat pulse: peaks (=1) exactly on the beat, dips (=0) mid-beat.
                let beat_phase = (self.beat_timer / self.beat_interval.max(0.0001)).clamp(0.0, 1.0);
                let pulse = (beat_phase * std::f32::consts::TAU).cos() * 0.5 + 0.5;
                let base_r = 46.0 + npc.leader_scale * 12.0;
                let ring_r = base_r + (1.0 - pulse) * 26.0; // tight on the beat, wide off it
                let alpha = (0.32 + urgency * 0.40 + pulse * 0.24).min(0.95);
                let thickness = 3.0 + pulse * 3.0 + urgency * 2.5;
                let ring = cached_stroke_circle(ctx, ring_r, thickness)?;
                canvas.draw(
                    &ring,
                    DrawParam::default()
                        .dest(npc.leader_pos)
                        .color(Color::new(1.0, 0.22 + pulse * 0.22, 0.12, alpha)),
                );
                // On-beat inner flash — the drum-hit frame where a parry lands cleanly. Keyed to the
                // wider defend window (not the tight BEAT_WINDOW) so the flash lasts exactly as long
                // as a Stomp/Wave parry actually works: what you see is what lands.
                if self.on_beat_defend() {
                    let flash = cached_stroke_circle(ctx, base_r * 0.78, 2.5)?;
                    canvas.draw(
                        &flash,
                        DrawParam::default()
                            .dest(npc.leader_pos)
                            .color(Color::new(1.0, 0.92, 0.42, 0.5 + urgency * 0.3)),
                    );
                }
            }

            // --- Clash "RAM NOW" telegraph ----------------------------------------------------
            // When you close on a King leader and the clash is off cooldown, ring it with an amber
            // opportunity cue that beats like the DEFEND ring — but this one means "ram ON the beat
            // to WIN the collision" (a POWER CLASH), not "defend". It answers Carl's #164 legibility
            // complaint that it "wasn't obvious what to time": the ring snaps tight and flashes teal
            // on the beat — the exact "RAM NOW" frame, keyed to the same forgiving `on_beat_defend`
            // window the clash actually uses (what you see equals what lands). It grows brighter as
            // you approach and is suppressed while a splice is armed (the red DEFEND ring wins that
            // frame — defense reads first). Draw-only; the timed outcome lives in update_npc_trains.
            if self.king_splice_cooldown <= 0.0 && npc.steal_threat <= 0.0 {
                let player_center = self.player_pos + Vec2::splat(PLAYER_SIZE / 2.0);
                let col_r = CRAB_SIZE * npc.leader_scale * 1.2 + PLAYER_SIZE * 0.5;
                let arm_r = col_r + 110.0; // show the cue a beat before contact so the ram is readable
                let dist = npc.leader_pos.distance(player_center);
                if dist < arm_r {
                    // 1 at contact → 0 at the arming edge, so the cue burns in as you commit the ram.
                    let prox = (1.0 - (dist - col_r).max(0.0) / (arm_r - col_r)).clamp(0.0, 1.0);
                    let beat_phase = (self.beat_timer / self.beat_interval.max(0.0001)).clamp(0.0, 1.0);
                    let pulse = (beat_phase * std::f32::consts::TAU).cos() * 0.5 + 0.5;
                    let base_r = 44.0 + npc.leader_scale * 12.0;
                    let ring_r = base_r + (1.0 - pulse) * 24.0; // tight on the beat, wide off it
                    let alpha = (0.18 + prox * 0.34 + pulse * 0.20).min(0.85);
                    let thickness = 3.0 + pulse * 3.0;
                    let ring = cached_stroke_circle(ctx, ring_r, thickness)?;
                    canvas.draw(
                        &ring,
                        DrawParam::default()
                            .dest(npc.leader_pos)
                            .color(Color::new(1.0, 0.62 + pulse * 0.2, 0.18, alpha)),
                    );
                    // On-beat inner flash — the "RAM NOW" frame where a clash wins. Teal (like the
                    // COUNTER cue) so on-beat reads as "good", distinct from the red DEFEND danger.
                    if self.on_beat_defend() {
                        let flash = cached_stroke_circle(ctx, base_r * 0.8, 2.5)?;
                        canvas.draw(
                            &flash,
                            DrawParam::default()
                                .dest(npc.leader_pos)
                                .color(Color::new(0.4, 1.0, 0.85, 0.4 + prox * 0.4)),
                        );
                    }
                }
            }

            // --- Revenge "chase me" marker ----------------------------------------------------
            // For a few seconds after a rival splices your tail it wears a beat-pulsed green ring so
            // you know exactly which train to chase and rustle your crabs back from (ROADMAP: "you
            // steal, they steal back"). Green reads as "your prize is here" against the red DEFEND
            // ring's "danger". It expands and fades as the window burns down, urging a fast chase.
            // Suppressed while a fresh splice is armed so the two rings never fight for the same frame.
            if npc.revenge_timer > 0.0 && npc.steal_threat <= 0.0 {
                let life = (npc.revenge_timer / REVENGE_WINDOW).clamp(0.0, 1.0); // 1 fresh → 0 lapsed
                let beat_phase = (self.beat_timer / self.beat_interval.max(0.0001)).clamp(0.0, 1.0);
                let pulse = (beat_phase * std::f32::consts::TAU).cos() * 0.5 + 0.5;
                let base_r = 40.0 + npc.leader_scale * 12.0;
                let ring_r = base_r + (1.0 - life) * 22.0 + pulse * 8.0; // grows as it lapses, beats on top
                let alpha = (0.25 + life * 0.45 + pulse * 0.2).min(0.9);
                let thickness = 3.0 + pulse * 2.5;
                let ring = cached_stroke_circle(ctx, ring_r, thickness)?;
                canvas.draw(
                    &ring,
                    DrawParam::default()
                        .dest(npc.leader_pos)
                        .color(Color::new(0.3, 1.0, 0.55, alpha)),
                );
            }

            // Name banner floating above the King Crab — a distinct, readable-across-the-field
            // label so rivals tell apart at a glance (agar.io: spot the big one creeping in from
            // the edge). Three signals stack:
            //   • Size by tier — elders' banners are noticeably bigger than scouts', scaled off
            //     base_scale (scout 1.2 / wanderer 1.8 / elder 2.4).
            //   • Colour by tier — pale lime scout, regal gold wanderer, deep-amber apex elder.
            //   • Distance-scaled alpha — a distant rival's name burns in at full opacity so you
            //     can read who's approaching; it eases off as they close on you and the crab
            //     itself is plainly visible.
            // Glyphs are shaped once (cached at a large baseline) and the per-tier size comes from
            // the draw scale, so this stays allocation-free per frame.
            let name_w = NPC_NAME_CACHE.with(|c| -> GameResult<f32> {
                let mut cache = c.borrow_mut();
                if !cache.contains_key(&npc.name) {
                    let mut text = Text::new(npc.name.as_str());
                    text.set_scale(24.0);
                    let w = text.measure(ctx)?.x;
                    cache.insert(npc.name.clone(), (text, w));
                }
                Ok(cache.get(&npc.name).unwrap().1)
            })?;
            // Tier styling from the leader's base size.
            let tier_scale = 0.8 + (npc.base_scale - 1.2) * 0.33;
            let (nr, ng, nb) = if npc.base_scale >= 2.2 {
                (1.0, 0.5, 0.12) // elder — deep amber, the apex train
            } else if npc.base_scale >= 1.6 {
                (0.98, 0.78, 0.28) // wanderer — regal gold
            } else {
                (0.72, 0.95, 0.5) // scout — pale lime, small and fast
            };
            // Reddens the banner while this rival is on the hunt, reinforcing the marching-dot threat
            // line below it; eases back to the tier colour once it's just wandering. Kept mild so tier
            // (lime/gold/amber) still reads at a glance.
            let hunt_t = (npc.hunt_intent * 0.55).clamp(0.0, 0.55);
            let (nr, ng, nb) = (
                nr + (1.0 - nr) * hunt_t,
                ng + (0.30 - ng) * hunt_t,
                nb + (0.12 - nb) * hunt_t,
            );
            // Distance ramp: far rivals read at full opacity, near ones ease back.
            let dist = (npc.leader_pos - self.player_pos).length();
            let dist_alpha = (0.5 + dist / 1000.0 * 0.5).clamp(0.5, 1.0);
            let draw_w = name_w * tier_scale;
            let name_off = 45.0 + npc.leader_scale * 10.0 + leader_bob;
            NPC_NAME_CACHE.with(|c| {
                let cache = c.borrow();
                if let Some((text, _)) = cache.get(&npc.name) {
                    let name_pos = npc.leader_pos - Vec2::new(draw_w / 2.0, name_off);
                    // Drop shadow (scaled with the banner so it tracks tier size)
                    canvas.draw(
                        text,
                        DrawParam::default()
                            .dest(name_pos + Vec2::splat(2.0 * tier_scale))
                            .scale(Vec2::splat(tier_scale))
                            .color(Color::new(0.0, 0.0, 0.0, 0.7 * dist_alpha)),
                    );
                    // Name in its tier colour
                    canvas.draw(
                        text,
                        DrawParam::default()
                            .dest(name_pos)
                            .scale(Vec2::splat(tier_scale))
                            .color(Color::new(nr, ng, nb, dist_alpha)),
                    );
                }
            });
        }

        Ok(())
    }
}
