//! Per-crab and per-rival "aura" and status overlays drawn over the herd each frame:
//! the Armored crab's steely shell arc, the Hermit's coppery coil, the attracted-crab
//! magnet glow, the magnet/thief/splitter auras, the golden-crab sparkle, and the
//! cleave stakes / tail-run badge / cleave slash. Extracted from `graphics/mod.rs` to
//! keep that file navigable; these all lean on the shared cached meshes, deferred
//! archetype-ring batching, and per-frame instance buffers defined in the parent module
//! (reached here via `use super::*`).

use super::*;

/// Draw a hard-shelled crab's shell indicator — a thin steely arc that depletes as the shell is
/// worn down or cracked, so the player can read at a glance which crabs need a Stomp.
pub fn draw_armor_ring(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    size: f32,
    shell_frac: f32,
    time: f32,
) -> ggez::GameResult {
    let radius = size * 0.8;
    let pulse = (time * 5.0).sin() * 0.5 + 0.5;

    // Faint full track so the drained portion still reads as progress. Deferred into
    // ARCHETYPE_RING_GROUPS via defer_archetype_ring() instead of an immediate canvas.draw() —
    // same batching as draw_thief_aura/draw_splitter_aura/draw_golden_sparkle above, so multiple
    // Armored crabs' tracks (a fixed radius/thickness pair) collapse into one GPU submission. The
    // health arc below stays immediate: its mesh varies per-crab with the live shell fraction, so
    // it rarely shares a bucket with another crab's arc and batching it wouldn't collapse draws.
    defer_archetype_ring(ctx, pos, radius, 3.0, Color::new(0.0, 0.0, 0.0, 0.35))?;

    let segs = 40usize;
    let filled = ((segs as f32) * shell_frac.clamp(0.0, 1.0)).ceil().max(1.0) as usize;
    let arc = cached_stroke_arc(ctx, radius, 3.0, segs, filled)?;
    canvas.draw(
        &arc,
        DrawParam::default()
            .dest(pos)
            .color(Color::new(0.6, 0.72, 0.88, 0.85 + pulse * 0.15)),
    );
    Ok(())
}

/// Draw a Hermit crab's borrowed-shell indicator — a warm coppery coiled shell, visually distinct
/// from the Armored crab's cold steely arc so the player reads at a glance "this shell the beam
/// won't crack; use a Stomp, a Dancer's hop, or a Magnet". The shell depletes as it's chipped, and
/// a slow-rotating coil of dots reads as the spiral of a borrowed conch shell.
pub fn draw_hermit_shell(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    size: f32,
    shell_frac: f32,
    time: f32,
) -> ggez::GameResult {
    let radius = size * 0.82;
    let pulse = (time * 4.0).sin() * 0.5 + 0.5;
    let frac = shell_frac.clamp(0.0, 1.0);

    // Faint full track so the chipped-away portion still reads as progress.
    let track = cached_stroke_circle(ctx, radius, 3.0)?;
    canvas.draw(
        &track,
        DrawParam::default()
            .dest(pos)
            .color(Color::new(0.0, 0.0, 0.0, 0.32)),
    );

    // Depleting coppery arc — the remaining shell.
    let segs = 40usize;
    let filled = ((segs as f32) * frac).ceil().max(1.0) as usize;
    let arc = cached_stroke_arc(ctx, radius, 3.5, segs, filled)?;
    canvas.draw(
        &arc,
        DrawParam::default()
            .dest(pos)
            .color(Color::new(0.85, 0.55, 0.28, 0.82 + pulse * 0.18)),
    );

    // A slow-turning spiral of little coil dots inside the ring — the borrowed-shell whorl. Defers
    // each dot's DrawParam into HERMIT_COIL_PARAMS (same pattern as GOLDEN_SPARKLE_PARAMS) so all
    // hermit coil dots across every shelled Hermit on screen flush as one draw_instanced_mesh call
    // in flush_hermit_coil_dots() instead of up to 5 individual canvas.draw() calls per crab.
    let coil_dots = 5usize;
    let shown = ((coil_dots as f32) * frac).ceil().max(1.0) as usize;
    HERMIT_COIL_PARAMS.with(|params_cell| {
        let mut params = params_cell.borrow_mut();
        for k in 0..shown {
            let f = k as f32 / coil_dots as f32;
            // Tightening spiral: angle winds faster than one turn, radius shrinks toward the center.
            let ang = time * 1.2 + f * std::f32::consts::TAU * 1.6;
            let rr = radius * (0.62 - f * 0.42);
            let d = pos + Vec2::new(ang.cos(), ang.sin()) * rr;
            let dot_r = (2.6 - f * 1.2).max(1.0);
            params.push(
                DrawParam::default()
                    .dest(d)
                    .scale(Vec2::splat(dot_r))
                    .color(Color::new(0.95, 0.68, 0.38, 0.7)),
            );
        }
    });
    Ok(())
}

/// Draw a pulsing attraction halo around a crab that is inside the flashlight beam.
/// `crab_color` is [r, g, b] 0..1. `time` is total elapsed seconds. `beat_intensity` 0..1.
pub fn draw_attracted_crab_glow(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    size: f32,
    crab_color: [f32; 3],
    time: f32,
    beat_intensity: f32,
) -> ggez::GameResult {
    // This is now the SCORCH ring drawn only on a shelled target the beam is burning down (see the
    // gated call site in draw_crabs). It reads as a searing hot-spot on the shell, not a soft lure
    // halo. Fast, jittery flicker (like a flame biting the shell) instead of a lazy breathing pulse.
    let flicker = (time * 6.0 * std::f32::consts::TAU).sin() * 0.5 + 0.5;
    let flicker2 = (time * 13.0 * std::f32::consts::TAU).sin() * 0.5 + 0.5;
    let pulse = (flicker * 0.7 + flicker2 * 0.3) * (0.75 + beat_intensity * 0.25); // 0..1, twitchy

    let base_radius = size * 0.85;
    let outer_radius = base_radius + 4.0 + pulse * 7.0;

    // Harsh white-yellow scorch (ignore the passed crab_color's hue for saturation; the caller
    // passes a hot color, but clamp it toward white-hot so the burn always reads as searing).
    let [r, g, b] = crab_color;

    // Additively blended — the caller (draw_crabs_with_shake) already has the canvas in ADD
    // mode for this whole per-crab aura pass, so this doesn't toggle blend mode itself; see the
    // comment there for why (per-crab toggling used to cause a GPU pipeline switch per crab).

    // Outer soft glow ring and inner bright ring — deferred into per-key scratch maps and
    // flushed as a couple of instanced batches by flush_attracted_crab_glows() after the
    // per-crab aura loop. Replaces 2 individual canvas.draw() calls per attracted crab with
    // one grouped submission per distinct stroke-circle key bucket. Meshes are still built
    // (or cache-hit) here so the key → mesh association stays consistent.
    let glow_alpha = (0.18 + pulse * 0.22).clamp(0.0, 1.0);
    let glow_r = outer_radius + outer_radius * 0.18;
    let glow_th = outer_radius * 0.35;
    let glow_key = stroke_circle_key(glow_r, glow_th);
    cached_stroke_circle(ctx, glow_r, glow_th)?;
    ATTRACTED_GLOW_GROUPS.with(|groups_cell| {
        let mut groups = groups_cell.borrow_mut();
        groups.entry(glow_key).or_default().push(
            DrawParam::default()
                .dest(pos)
                .color(Color::new(r, g, b, glow_alpha)),
        );
    });

    let ring_alpha = (0.45 + pulse * 0.45).clamp(0.0, 1.0);
    let ring_key = stroke_circle_key(outer_radius, 2.5);
    cached_stroke_circle(ctx, outer_radius, 2.5)?;
    ATTRACTED_RING_GROUPS.with(|groups_cell| {
        let mut groups = groups_cell.borrow_mut();
        groups.entry(ring_key).or_default().push(
            DrawParam::default()
                .dest(pos)
                .color(Color::new(
                    (r * 0.5 + 0.5).min(1.0),
                    (g * 0.5 + 0.5).min(1.0),
                    (b * 0.5 + 0.5).min(1.0),
                    ring_alpha,
                )),
        );
    });

    Ok(())
}

/// Draw the magnetic field aura around a free Magnet crab — rings that sweep *inward* toward the
/// crab, reading as a pull that gathers the herd. `size` is the crab's on-screen size; `pull_radius`
/// is how far the crab's tug reaches (matches MAGNET_RADIUS in main.rs) so the aura shows the player
/// exactly how big the catchment is. `time` is total elapsed seconds.
pub fn draw_magnet_aura(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    size: f32,
    pull_radius: f32,
    time: f32,
    lured: bool,
    charged: bool,
) -> ggez::GameResult {
    // Additively blended — see draw_attracted_crab_glow's comment: the caller already has the
    // canvas in ADD mode for this whole per-crab aura pass, so no toggle here.

    // Lodestone red-orange, matching the crab's own color — but while a Golden's shine has lured
    // this Magnet off its cluster, the aura brightens gold-ward so the "chasing the prize"
    // crossover reads at a glance (the mirror tint of the Thief's snared aura going orange). When
    // it's *charged* — pinning a snared Golden and supercharged into a herd-vacuum — the aura goes
    // full gold and its rings reach out over the widened pull radius so the bigger suck reads.
    let (r, g, b) = if charged {
        (1.0, 0.85, 0.4)
    } else if lured {
        (1.0, 0.78, 0.3)
    } else {
        (1.0, 0.4, 0.2)
    };
    let inner = size * 0.7;
    // Match the 1.4x wider field a charged Magnet actually pulls over (CHARGED_MAGNET_RADIUS in
    // main.rs) so the visual boundary tells the truth about the vacuum's reach.
    let ring_radius = if charged { pull_radius * 1.4 } else { pull_radius };
    // A charged Magnet's rings sweep faster and read brighter to sell the energized state.
    let sweep_speed = if charged { 1.1 } else { 0.6 };
    let alpha_scale = if charged { 0.5 } else { 0.35 };

    // Three rings sweeping inward on a shared phase, staggered a third of a cycle apart, so the
    // aura reads as a steady inward pull rather than a single blip. Brightest as they close in.
    //
    // These rings sweep over a ~215px radius range (ring_radius → inner). The shared
    // stroke-circle cache uses 2px buckets, which would generate ~108 distinct mesh keys per
    // ring per sweep cycle — with multiple Magnets on screen this easily pushes past the 512-entry
    // cap, evicting every other cached ring (chain ghosts, auras, shockwaves) and forcing full
    // rebuilds. Round to 8px buckets here instead: visually indistinguishable at these radii
    // (the sweep is a fluid animation, not a precise size) but reduces key count to ~27 per ring
    // per sweep, keeping the cache far below the cap even with several Magnets in play.
    // Defer all sweep rings and the core into MAGNET_AURA_RING_PARAMS so flush_magnet_auras()
    // can batch all Magnets' rings together by mesh key. In the Water biome (now Magnet-heavy)
    // this collapses N×3 individual ADD-blend draw calls for the sweep rings into at most 3
    // batched draw_instanced_mesh calls, regardless of how many Magnets are on screen.
    MAGNET_AURA_RING_PARAMS.with(|params_cell| -> ggez::GameResult {
        let mut params = params_cell.borrow_mut();
        for k in 0..3u32 {
            let phase = ((time * sweep_speed + k as f32 / 3.0) % 1.0) as f32;
            let radius = ring_radius - (ring_radius - inner) * phase;
            let alpha = (phase * alpha_scale).clamp(0.0, alpha_scale);
            // Snap to 8px bucket — same quantization already in place; ensures rings from
            // different Magnets at the same sweep phase share the same mesh key and can be
            // instanced together.
            let radius_q = ((radius / 8.0).round() * 8.0).max(0.5);
            // Ensure the mesh exists in the cache (cached_stroke_circle builds it if absent).
            cached_stroke_circle(ctx, radius_q, 2.0)?;
            let key = stroke_circle_key(radius_q, 2.0);
            params.push((key, DrawParam::default().dest(pos).color(Color::new(r, g, b, alpha))));
        }

        // Core ring — deferred into the same batch. Core radii vary per crab size so they
        // won't collapse across multiple Magnets as aggressively as the sweep rings, but they're
        // still one fewer canvas.draw() call per Magnet on the hot path.
        let core_pulse = (time * 4.0).sin() * 0.5 + 0.5;
        let core_r = inner + 4.0 + core_pulse * 4.0;
        cached_stroke_circle(ctx, core_r, 2.5)?;
        let core_key = stroke_circle_key(core_r, 2.5);
        let core_g = if charged || lured { 0.8 } else { 0.55 } + core_pulse * 0.2;
        let core_b_val = if charged { 0.4 } else if lured { 0.35 } else { 0.3 };
        params.push((core_key, DrawParam::default().dest(pos).color(Color::new(1.0, core_g, core_b_val, 0.55))));
        Ok(())
    })?;

    Ok(())
}

/// Thief crab marker: a sly poison-green ring so a Thief stands out from the herd as "trouble
/// heading for your tail", plus a sharper jittering gnaw-ring when it's latched and actively
/// peeling links (`latched` = true). The latched state pulses fast and bright so the theft in
/// progress reads at a glance and the player knows to whistle/stomp it off.
pub fn draw_thief_aura(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    size: f32,
    latched: bool,
    snared: bool,
    lured: bool,
    time: f32,
) -> ggez::GameResult {
    // Additively blended — see draw_attracted_crab_glow's comment: the caller already has the
    // canvas in ADD mode for this whole per-crab aura pass, so no toggle here.

    // Poison-green, matching the crab's own color — but while a Magnet has intercepted it, the
    // green bleeds toward the lodestone's orange so the "caught in the field" crossover reads;
    // while a fleeing Golden has lured it off your tail, the green catches a golden gleam instead,
    // so the "the shine drew the raider away" crossover reads distinct from the Magnet interception.
    let (r, g, b) = if snared {
        (0.95, 0.6, 0.2)
    } else if lured {
        (0.85, 0.95, 0.35) // poison-green warmed by the golden prize it's chasing
    } else {
        (0.35, 0.95, 0.5)
    };

    // Each branch used to build its own stroke-circle mesh and issue an immediate canvas.draw()
    // per Thief per frame. Deferred into ARCHETYPE_RING_GROUPS via defer_archetype_ring() instead,
    // so multiple Thieves in the same state on screen collapse into one GPU submission per shared
    // radius bucket (flushed by flush_archetype_rings() after the per-crab aura pass) — identical
    // rings, just batched.
    if latched {
        // Actively gnawing: a fast, bright, slightly jittering double ring so the theft screams
        // for attention. The jitter fakes the crab tearing at the link.
        let pulse = (time * 18.0).sin() * 0.5 + 0.5;
        let jitter = (time * 40.0).sin() * 2.5;
        defer_archetype_ring(
            ctx,
            pos,
            size * 0.9 + 3.0 + jitter,
            3.0,
            Color::new(r, g, b, 0.5 + pulse * 0.4),
        )?;
        defer_archetype_ring(
            ctx,
            pos,
            size * 1.25 + pulse * 6.0,
            2.0,
            Color::new(0.6, 1.0, 0.5, 0.25 + pulse * 0.25),
        )?;
    } else if snared {
        // Intercepted by a Magnet: a brighter, faster orange ring that reads as "the field's got
        // it" — livelier than the calm prowl so the save is legible, calmer than the theft frenzy.
        let pulse = (time * 9.0).sin() * 0.5 + 0.5;
        defer_archetype_ring(
            ctx,
            pos,
            size * 0.9 + 3.0 + pulse * 4.0,
            2.5,
            Color::new(r, g, b, 0.45 + pulse * 0.3),
        )?;
    } else if lured {
        // Lured off your tail by a Golden's shine: a brisk, brighter golden-green ring — livelier
        // than the calm prowl so the divert reads as the raider actively chasing the prize.
        let pulse = (time * 7.0).sin() * 0.5 + 0.5;
        defer_archetype_ring(
            ctx,
            pos,
            size * 0.9 + 3.0 + pulse * 4.0,
            2.5,
            Color::new(r, g, b, 0.4 + pulse * 0.3),
        )?;
    } else {
        // Prowling: a steady soft ring that just marks it out, calmer than the latched frenzy.
        let pulse = (time * 3.0).sin() * 0.5 + 0.5;
        defer_archetype_ring(
            ctx,
            pos,
            size * 0.85 + 3.0 + pulse * 3.0,
            2.0,
            Color::new(r, g, b, 0.35 + pulse * 0.2),
        )?;
    }

    Ok(())
}

/// Golden crab shine — a soft shimmering halo plus a handful of sparkle dots orbiting the crab, so
/// the rare high-value prize catches the eye across the whole field and reads as "chase this one!".
/// Additively blended for a glowy treasure look — the caller (draw_crabs_with_shake) already has
/// the canvas in ADD mode for this whole per-crab aura pass, so this doesn't toggle blend mode
/// itself. Reuses the cached unit-circle and stroke-circle meshes (scaled/positioned per element
/// via DrawParam) so no fresh GPU buffers are allocated.
pub fn draw_golden_sparkle(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    size: f32,
    time: f32,
    snared: bool,
) -> ggez::GameResult {
    // Soft breathing halo so the prize glows even when it's holding still. When a Magnet's field
    // has snared it, the halo warms toward the lodestone's orange so the "trapped by the Magnet"
    // state reads instantly against the ordinary gold shine.
    // Both rings deferred into ARCHETYPE_RING_GROUPS via defer_archetype_ring() instead of an
    // immediate canvas.draw() — same batching as draw_thief_aura above, so multiple Goldens'
    // halos/tethers collapse into shared GPU submissions (flushed by flush_archetype_rings()).
    let pulse = (time * 4.0).sin() * 0.5 + 0.5;
    let (hg, hb) = if snared { (0.6, 0.15) } else { (0.85, 0.3) };
    defer_archetype_ring(
        ctx,
        pos,
        size * 0.8 + 3.0 + pulse * 4.0,
        2.5,
        Color::new(1.0, hg, hb, 0.35 + pulse * 0.3),
    )?;

    // While snared, a fast-spinning tether ring cinches in tight around the crab — the visual of
    // the field clamping the prize in place, drawing the eye to "grab it NOW".
    if snared {
        let cinch = 0.5 + 0.5 * (time * 12.0).sin();
        defer_archetype_ring(
            ctx,
            pos,
            size * 0.55 + 2.0 + cinch * 3.0,
            3.0,
            Color::new(1.0, 0.6, 0.15, 0.55 + cinch * 0.35),
        )?;
    }

    // A ring of sparkle dots orbiting the crab, each twinkling on its own phase so the whole thing
    // shimmers like a coin catching the light. Snared, the orbit pulls in tighter and spins faster,
    // like filings dragged onto the lodestone.
    // Instead of issuing 5 individual canvas.draw() calls here (one per dot), push each dot's
    // DrawParam into GOLDEN_SPARKLE_PARAMS and let flush_golden_sparkles() drain them all as one
    // instanced batch after every crab's aura pass — identical output, one GPU submission total.
    const SPARKLES: usize = 5;
    let orbit = if snared { size * 0.55 + 4.0 } else { size * 0.75 + 6.0 };
    let spin = if snared { 3.4 } else { 1.6 };
    GOLDEN_SPARKLE_PARAMS.with(|params_cell| {
        let mut params = params_cell.borrow_mut();
        let (sg, sb) = if snared { (0.75, 0.35) } else { (0.95, 0.55) };
        for i in 0..SPARKLES {
            let base = i as f32 / SPARKLES as f32 * std::f32::consts::TAU;
            let ang = base + time * spin;
            let twinkle = ((time * 6.0 + i as f32 * 1.7).sin() * 0.5 + 0.5).powf(2.0);
            let dpos = pos + Vec2::new(ang.cos(), ang.sin()) * orbit;
            let r = 1.5 + twinkle * 2.5;
            params.push(
                DrawParam::default()
                    .dest(dpos)
                    .scale(Vec2::splat(r))
                    .color(Color::new(1.0, sg, sb, 0.4 + twinkle * 0.6)),
            );
        }
    });

    Ok(())
}

/// Splitter crab aura — a bright teal ring that pulses open into two halves, telegraphing that
/// catching this one cleaves your train in two. Two short arcs sweep apart on opposite sides of a
/// vertical "cleave line" so the split reads at a glance, distinct from every other archetype aura.
/// Additively blended; the caller (the per-crab aura pass) already has the canvas in ADD mode, so
/// this doesn't toggle blend mode itself. Reuses cached meshes so no fresh GPU buffers are uploaded.
pub fn draw_splitter_aura(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    size: f32,
    time: f32,
    beat_prox: f32,
) -> ggez::GameResult {
    // Breathing halo so the cleaver reads even while it's holding still — teal, the archetype tint.
    // Deferred into ARCHETYPE_RING_GROUPS via defer_archetype_ring() instead of an immediate
    // canvas.draw(), same batching as draw_thief_aura/draw_golden_sparkle above.
    let pulse = (time * 3.5).sin() * 0.5 + 0.5;
    defer_archetype_ring(
        ctx,
        pos,
        size * 0.75 + 3.0 + pulse * 4.0,
        2.5,
        Color::new(0.2, 0.95, 0.85, 0.30 + pulse * 0.28),
    )?;

    // Beat telegraph — the Splitter's whole gimmick is a timing bet (catch it ON the beat for a
    // clean, full-jackpot cut; off-beat is a sloppy half-cut). `beat_prox` (0..1, peaking on the
    // beat) drives a gold "grab NOW" flare so the clean-cut window is legible BEFORE the catch, not
    // just afterward: as the beat lands the teal aura blooms into a bright gold ring that snaps in
    // and fades between beats. This is the anticipation cue that lets a player set the cleave up on
    // purpose instead of grabbing blind and hoping.
    if beat_prox > 0.01 {
        defer_archetype_ring(
            ctx,
            pos,
            size * 0.75 + 6.0 + beat_prox * 10.0,
            2.0 + beat_prox * 2.5,
            // Teal→gold as the beat approaches, so the aura visibly "goes hot" in the window.
            Color::new(
                0.4 + 0.6 * beat_prox,
                0.95,
                0.85 - 0.55 * beat_prox,
                0.25 + 0.55 * beat_prox,
            ),
        )?;
    }

    // The "cleave" tell: two small dots split apart from center along the horizontal, snapping back
    // on each pulse cycle — the visual shorthand for "I halve your train". The spread pulses so the
    // two halves visibly separate and rejoin, drawing the eye. On the beat the split snaps WIDER
    // (beat_prox term) so the two halves fling apart exactly when a clean cut is available.
    // Deferred into CLEAVE_DOT_PARAMS (same UNIT_CIRCLE-batching technique as GOLDEN_SPARKLE_PARAMS)
    // instead of two immediate canvas.draw() calls, flushed by flush_archetype_rings().
    let spread = (size * 0.35 + 4.0) * (0.4 + 0.6 * pulse) + beat_prox * size * 0.3;
    CLEAVE_DOT_PARAMS.with(|params_cell| {
        let mut params = params_cell.borrow_mut();
        for &dir in &[-1.0_f32, 1.0] {
            let dpos = pos + Vec2::new(dir * spread, 0.0);
            params.push(
                DrawParam::default()
                    .dest(dpos)
                    .scale(Vec2::splat(2.0 + pulse * 2.0 + beat_prox * 2.5))
                    .color(Color::new(
                        0.5 + 0.5 * beat_prox,
                        1.0,
                        0.9 - 0.5 * beat_prox,
                        0.45 + pulse * 0.5,
                    )),
            );
        }
    });

    Ok(())
}

/// Cleave stakes tag — the pre-catch readout of the Splitter bet. While a free Splitter is on the
/// field and the player has a train worth cleaving, this floats a live "CLEAVE ~N" number at the
/// train's split point (the midpoint where the cut would land), so the player can read what a clean
/// on-beat cut would bank *before* committing — the same "make the bet legible before, not just
/// after" idea as the splitter aura's beat flare, but as an actual score figure over the train.
///
/// `worth` is the clean-cut value (from `cleave_clean_worth`, so it can't drift from the real
/// payout). `jackpot` marks that a Golden/Magnet/cashed-run crossover would fire — the tag reads
/// "JACKPOT" then. `beat_prox` (0..1, peaking on the beat) heats the tag teal→gold in the clean-cut
/// window so "grab NOW" reads on the number itself, matching the aura. The Text is cached and only
/// re-shaped when the value or jackpot state changes, so no per-frame allocation on the draw path.
#[allow(clippy::too_many_arguments)]
pub fn draw_cleave_stakes(
    ctx: &mut Context,
    canvas: &mut Canvas,
    at: Vec2,
    worth: usize,
    jackpot: bool,
    beat_prox: f32,
    time: f32,
) -> ggez::GameResult {
    thread_local! {
        static CLEAVE_STAKES_CACHE: std::cell::RefCell<Option<(usize, bool, Text, f32)>> =
            const { std::cell::RefCell::new(None) };
    }
    CLEAVE_STAKES_CACHE.with(|cache| -> ggez::GameResult {
        let mut c = cache.borrow_mut();
        let needs = c
            .as_ref()
            .map_or(true, |(v, j, _, _)| *v != worth || *j != jackpot);
        if needs {
            let label = if jackpot {
                format!("JACKPOT CLEAVE ~ {}", worth)
            } else {
                format!("CLEAVE ~ {}", worth)
            };
            let mut t = Text::new(label);
            t.set_scale(18.0);
            let w = t.measure(ctx)?.x;
            *c = Some((worth, jackpot, t, w));
        }
        let (_, _, text, w) = c.as_ref().unwrap();
        let w = *w;
        // Bob above the split point, a touch livelier in the beat window so the tag "leans in" as the
        // clean-cut window opens — the anticipation cue on the number itself.
        let bob = (time * (4.0 + beat_prox * 5.0)).sin() * (2.0 + beat_prox * 3.0);
        let base = at - Vec2::new(w * 0.5, 30.0 - bob);
        // Teal at rest, heating to gold as the beat lands (matching the splitter aura flare), and a
        // touch hotter still when a jackpot crossover is on the line.
        let hot = beat_prox.max(if jackpot { 0.25 } else { 0.0 });
        let tr = 0.35 + 0.65 * hot;
        let tg = 0.95;
        let tb = 0.85 - 0.6 * hot;
        // Dark backing keeps it legible over bright field/particles.
        canvas.draw(
            text,
            DrawParam::default()
                .dest(base + Vec2::splat(1.5))
                .color(Color::new(0.0, 0.0, 0.0, 0.55)),
        );
        canvas.draw(
            text,
            DrawParam::default()
                .dest(base)
                .color(Color::new(tr, tg, tb.max(0.0), 0.95)),
        );
        Ok(())
    })
}

/// Tail-run badge — the persistent, watchable face of the same-type match run at the tail of the
/// train. `tail_run_len` only ever flashed for a frame at catch time, so the player could never
/// *set up* the every-4th-link Match-Run Milestone — they couldn't see how long their current run
/// was or how close the next x4 flourish was. This floats a live "RUN xN" over the tail link with a
/// 4-pip meter filling toward the next milestone, color-matched to the run's crab type, and heats +
/// bobs harder in the beat window so committing to a single-type run reads as a live decision, not a
/// silent counter. `col` is the run's crab color; `beat_prox` (0..1) rises as a beat nears.
pub fn draw_tail_run_badge(
    ctx: &mut Context,
    canvas: &mut Canvas,
    at: Vec2,
    run: u32,
    col: [f32; 3],
    beat_prox: f32,
    time: f32,
) -> ggez::GameResult {
    thread_local! {
        static TAIL_RUN_CACHE: std::cell::RefCell<Option<(u32, Text, f32)>> =
            const { std::cell::RefCell::new(None) };
    }
    TAIL_RUN_CACHE.with(|cache| -> ggez::GameResult {
        let mut c = cache.borrow_mut();
        let needs = c.as_ref().map_or(true, |(v, _, _)| *v != run);
        if needs {
            let mut t = Text::new(format!("RUN x{}", run));
            t.set_scale(16.0);
            let w = t.measure(ctx)?.x;
            *c = Some((run, t, w));
        }
        let (_, text, w) = c.as_ref().unwrap();
        let w = *w;
        // Bob above the tail link, leaning in as a beat nears — same anticipation cue the cleave tag
        // uses so the rhythm HUD reads consistently.
        let bob = (time * (3.5 + beat_prox * 4.0)).sin() * (1.5 + beat_prox * 2.5);
        let base = at - Vec2::new(w * 0.5, 34.0 - bob);
        // Text tinted toward the run's crab color, brightened so it stays legible; heats a touch on
        // the beat.
        let hot = beat_prox;
        let tr = (col[0] * 0.5 + 0.5 + 0.15 * hot).min(1.0);
        let tg = (col[1] * 0.5 + 0.5 + 0.15 * hot).min(1.0);
        let tb = (col[2] * 0.5 + 0.5 + 0.15 * hot).min(1.0);
        canvas.draw(
            text,
            DrawParam::default()
                .dest(base + Vec2::splat(1.5))
                .color(Color::new(0.0, 0.0, 0.0, 0.55)),
        );
        canvas.draw(
            text,
            DrawParam::default()
                .dest(base)
                .color(Color::new(tr, tg, tb, 0.95)),
        );
        // 4-pip milestone meter under the label: how many links into the current group of four, so
        // the next Match-Run Milestone (4, 8, 12…) is a visible target you close on. Full row lit
        // means the flourish fires on the next same-type catch.
        let filled = if run == 0 { 0 } else { ((run - 1) % 4) + 1 };
        let pip_r = 3.0;
        let gap = 10.0;
        let row_w = gap * 3.0;
        let py = base.y + 20.0;
        let px0 = at.x - row_w * 0.5;
        // The pip that's about to complete the group pulses on the beat so the "one more lands it"
        // moment is legible.
        let about_to_land = filled == 4;
        // Reuse the cached unit-circle mesh (radius 1.0, built once) and push all variation —
        // position, radius, color — into DrawParam. This replaces 4 Mesh::new_circle GPU buffer
        // allocations per frame with 4 cheap DrawParam draws.
        let uc = unit_circle(ctx)?;
        for i in 0..4u32 {
            let lit = i < filled;
            let cx = px0 + gap * i as f32;
            let (r, g, b, a) = if lit {
                let boost = if about_to_land { 0.3 * hot } else { 0.0 };
                (
                    (col[0] + boost).min(1.0),
                    (col[1] + boost).min(1.0),
                    (col[2] + boost).min(1.0),
                    0.95,
                )
            } else {
                (0.4, 0.4, 0.45, 0.5)
            };
            let rr = if lit && about_to_land {
                pip_r + hot * 1.5
            } else {
                pip_r
            };
            canvas.draw(
                uc,
                DrawParam::default()
                    .dest(Vec2::new(cx, py))
                    .scale(Vec2::splat(rr))
                    .color(Color::new(r, g, b, a)),
            );
        }
        Ok(())
    })
}

/// Cleave slash — the blade stroke drawn the instant a Splitter cuts the conga train. Runs from the
/// last kept front link (`a`) to the split point (`b`), overshooting both ends so it reads as a
/// swung stroke rather than a connecting line. `flash` is a 1→0 life: the stroke starts long and
/// bright and retracts/fades as it decays. `gold` tints it gold on a Jackpot Cleave, teal on a plain
/// cut, matching the shockwave color so the two feedbacks agree.
pub fn draw_cleave_slash(
    ctx: &mut Context,
    canvas: &mut Canvas,
    a: Vec2,
    b: Vec2,
    flash: f32,
    gold: bool,
) -> ggez::GameResult {
    let mid = (a + b) * 0.5;
    let mut dir = b - a;
    if dir.length() < 1.0 {
        dir = Vec2::new(0.0, 1.0); // degenerate (1-link cut) — slash vertically through the point
    }
    let dir = dir.normalize();
    // Overshoot: the stroke reaches beyond both endpoints early in its life, retracting as it fades
    // so it snaps through the train. Half-length in pixels.
    let base = (b - a).length() * 0.5 + 26.0;
    let half = base * (0.55 + 0.45 * flash);
    let p0 = mid - dir * half;
    let p1 = mid + dir * half;

    let (r, g, bl) = if gold { (1.0, 0.88, 0.3) } else { (0.35, 1.0, 0.9) };
    let perp = Vec2::new(-dir.y, dir.x);

    // Tapered blade body — a filled quad that's fat at the leading tip (p1) and tapers to nothing at
    // the trailing tip (p0), so the slash reads as a swung blade with a heavy edge rather than a flat
    // line. Bowed slightly along `perp` so the swing has an arc. Built once per fire (rare event).
    let tip_w = 9.0 * flash + 2.0;
    let bow = perp * (6.0 * flash);
    let blade = [
        p0,
        mid + bow + perp * tip_w * 0.5,
        p1,
        mid + bow - perp * tip_w * 0.5,
    ];
    if let Ok(body) = Mesh::new_polygon(
        ctx,
        DrawMode::fill(),
        &blade,
        Color::new(r, g, bl, 0.28 * flash),
    ) {
        canvas.draw(&body, DrawParam::default());
    }

    // Three stacked strokes: a wide dim glow, a mid teal/gold core, a thin white-hot centerline —
    // so the slash has depth. Use the cached UNIT_LINE mesh (scaled/rotated via DrawParam) instead
    // of Mesh::new_line so these don't allocate a fresh GPU buffer every frame the flash is live.
    let line = unit_line(ctx)?;
    let angle = dir.y.atan2(dir.x);
    let seg_len = (p1 - p0).length();
    let strokes: [(f32, [f32; 4]); 3] = [
        (7.0, [r, g, bl, 0.30 * flash]),
        (3.5, [r, g, bl, 0.70 * flash]),
        (1.4, [1.0, 1.0, 1.0, 0.85 * flash]),
    ];
    for (w, col) in strokes {
        canvas.draw(
            line,
            DrawParam::default()
                .dest(p0)
                .rotation(angle)
                .scale(Vec2::new(seg_len, w))
                .color(Color::new(col[0], col[1], col[2], col[3])),
        );
    }

    let dot = unit_circle(ctx)?;

    // Parting shockline — a short bright bar drawn ACROSS the cut (perpendicular to the blade) at the
    // split point, splitting into two halves that push apart along the blade as the flash decays. This
    // is the "the train comes apart HERE" beat: the eye lands on the seam, not just the swing.
    // Use UNIT_LINE scaled via DrawParam (perpendicular rotation = angle + PI/2) instead of
    // Mesh::new_line to avoid fresh GPU buffer allocations every frame the flash is active.
    let seam_push = (1.0 - flash) * 30.0 + 2.0;
    let seam_half = 20.0 * flash + 5.0;
    let seam_angle = angle + std::f32::consts::FRAC_PI_2;
    let seam_len = seam_half * 2.0;
    for &s in &[-1.0_f32, 1.0] {
        let c = mid + dir * s * seam_push;
        let e0 = c - perp * seam_half; // left end of the perpendicular bar
        canvas.draw(
            line,
            DrawParam::default()
                .dest(e0)
                .rotation(seam_angle)
                .scale(Vec2::new(seam_len, 2.5))
                .color(Color::new(1.0, 1.0, 1.0, 0.7 * flash)),
        );
        // A glow dot riding each parting half.
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(c)
                .scale(Vec2::splat(4.0 + 4.0 * flash))
                .color(Color::new(r, g, bl, 0.55 * flash)),
        );
    }

    // Spark dots flung along the blade, staggered down its length and kicked out perpendicular as the
    // flash decays — the two halves visibly separating along the cut. Fade with the stroke.
    let push = (1.0 - flash) * 22.0 + 4.0;
    for i in 0..5 {
        let t = (i as f32 / 4.0) - 0.5; // -0.5..0.5 along the blade
        let along = mid + dir * (t * half * 1.6);
        for &s in &[-1.0_f32, 1.0] {
            let dpos = along + perp * s * push;
            canvas.draw(
                dot,
                DrawParam::default()
                    .dest(dpos)
                    .scale(Vec2::splat(2.0 + 3.0 * flash))
                    .color(Color::new(r, g, bl, 0.55 * flash)),
            );
        }
    }

    Ok(())
}
