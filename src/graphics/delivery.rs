//! Delivery-pen rendering: the beam that links the train to the pen on a bank, the
//! pen itself (ready/perfect palette, penned-marcher slots), and the surrounding
//! HUD tags — haul-worth, at-risk snap cost, kelp-snag warning, delivery streak, and
//! the pen guide arrow. Extracted from `graphics/mod.rs` to keep that file navigable;
//! these all draw the cash-in loop and lean on the shared cached meshes and per-frame
//! instance buffers defined in the parent module (reached here via `use super::*`).

use super::*;

/// Draw the delivery beam — a bright tapering streak from where the player (the train's head) stood
/// at the instant of a bank to the pen it cashed into, drawn while `flash` (1→0) decays. The pen's
/// own celebration (coin spray, rings, rays) all erupts *at* the pen; this is the one connective
/// beat that links where the conga line departed to the vault it pours into, so a bank reads as the
/// train visibly rushing home rather than the pen popping in isolation. Gold on an on-beat PERFECT
/// bank, go-green otherwise, to match the pen's own ready/perfect palette. A few gold sparks ride
/// the beam toward the pen to sell the "crabs streaming in" flow. All ADD-blended, cached meshes —
/// a handful of tinted draws, no allocation.
pub fn draw_deliver_beam(
    ctx: &mut Context,
    canvas: &mut Canvas,
    from: Vec2,
    to: Vec2,
    flash: f32,
    perfect: bool,
) -> ggez::GameResult {
    if flash <= 0.0 {
        return Ok(());
    }
    let delta = to - from;
    let len = delta.length();
    if len < 1.0 {
        return Ok(());
    }
    let dir = delta / len;
    let angle = dir.y.atan2(dir.x);
    let f = flash.clamp(0.0, 1.0);
    // Base color: gold for a perfect on-beat bank, go-green for a plain one.
    let (r, g, b) = if perfect {
        (1.0, 0.85, 0.35)
    } else {
        (0.5, 1.0, 0.55)
    };

    let line = unit_line(ctx)?;
    let orig = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    // Two stacked beams: a fat soft glow underneath and a bright thin core on top, so the streak
    // reads as a lit tether rather than a flat bar. Both fade with the flash and swell a touch at
    // the bank instant (f near 1) then thin out as it decays.
    let core_thick = 3.0 + 5.0 * f;
    let glow_thick = core_thick * 3.2;
    canvas.draw(
        line,
        DrawParam::default()
            .dest(from)
            .rotation(angle)
            .scale(Vec2::new(len, glow_thick))
            .color(Color::new(r, g, b, 0.22 * f)),
    );
    canvas.draw(
        line,
        DrawParam::default()
            .dest(from)
            .rotation(angle)
            .scale(Vec2::new(len, core_thick))
            .color(Color::new(
                (r + 0.3).min(1.0),
                (g + 0.15).min(1.0),
                (b + 0.2).min(1.0),
                (0.7 * f).clamp(0.0, 1.0),
            )),
    );

    // Sparks streaming along the beam toward the pen — deterministic from the flash timer, so no
    // RNG/state. As the flash decays (f: 1→0) the flow parameter runs 0→1, carrying each spark from
    // the player toward the pen; staggered so they string out rather than clump.
    let circle = unit_circle(ctx)?;
    let flow = 1.0 - f; // 0 at the bank instant, 1 as it finishes
    let spark_count = 7;
    for i in 0..spark_count {
        let stagger = i as f32 / spark_count as f32;
        let t = (flow + stagger) % 1.0;
        let pos = from + dir * (len * t);
        let sr = (2.0 + 3.0 * f) * (1.0 - (t - 0.5).abs()); // fattest mid-flight
        canvas.draw(
            circle,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(sr.max(0.3)))
                .color(Color::new(
                    (r + 0.3).min(1.0),
                    (g + 0.1).min(1.0),
                    b,
                    (0.85 * f).clamp(0.0, 1.0),
                )),
        );
    }

    canvas.set_blend_mode(orig);
    Ok(())
}

/// Draw the delivery pen — the "bank your train" corral the player drives the conga line into.
/// A warm gold goal-zone disc ringed by slowly-turning buoy posts, with a bobbing chevron beacon
/// marking the drop-off. It's dormant-but-visible with no train, and lights up (brighter fill,
/// faster pulse, a green "GO" halo) once the player has crabs to bank. `flash` (0..1, decaying)
/// blooms a bright celebratory ring right after a delivery lands. All geometry reuses the shared
/// cached circle/line meshes, so this costs a handful of tinted draws — no per-frame allocation.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
pub fn draw_delivery_pen(
    ctx: &mut Context,
    canvas: &mut Canvas,
    center: Vec2,
    radius: f32,
    time: f32,
    beat_intensity: f32,
    ready: bool,
    // 0..1 anticipation: how big the uncashed haul is (bigger train = a hungrier, hotter, faster
    // pen), further boosted as the loaded train closes in on the pen. Drives the "this is about to
    // be a jackpot" telegraph so the payoff builds *before* the bank, not only after it.
    haul: f32,
    // Live "what would this train bank right now" preview (base payout at the current combo/groove
    // multipliers, before the on-beat/streak bonuses you only lock in at bank time). `None` when
    // there's no train loaded. Drawn as a floating gold tag above the pen so "keep building vs. bank
    // now" becomes a concrete, watchable number that ticks up as the train grows — not a guess.
    worth: Option<usize>,
    flash: f32,
) -> ggez::GameResult {
    let haul = haul.clamp(0.0, 1.0);
    let unit_circle = match UNIT_CIRCLE.get() {
        Some(mesh) => mesh,
        None => {
            let mesh =
                Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh)
        }
    };
    let unit_line = match UNIT_LINE.get() {
        Some(mesh) => mesh,
        None => {
            let mesh = Mesh::new_rectangle(
                ctx,
                DrawMode::fill(),
                Rect::new(0.0, -0.5, 1.0, 1.0),
                Color::WHITE,
            )?;
            UNIT_LINE.get_or_init(|| mesh)
        }
    };

    // Breathing pulse — gentle when idle, urgent when there's a train to bank, and faster still the
    // fatter (and closer) the haul, so a big jackpot approach visibly winds the pen up.
    let pulse_speed = if ready { 6.0 + haul * 5.0 } else { 2.2 };
    let pulse = 0.5 + 0.5 * (time * pulse_speed).sin();
    let beat = beat_intensity.clamp(0.0, 1.0);

    // Warm goal-zone fill (normal blend so it reads as a marked patch of ground, not a glow).
    let fill_alpha = if ready {
        0.16 + 0.12 * pulse + haul * 0.12
    } else {
        0.08 + 0.04 * pulse
    };
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(center)
            .scale(Vec2::splat(radius))
            .color(Color::new(1.0, 0.82, 0.28, fill_alpha)),
    );

    let orig_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    // Outer boundary ring — the "fence line" of the pen. Greenish when ready, and as the haul
    // grows it heats from that go-green toward a hot jackpot gold, so a big incoming train reads as
    // "money" before you even bank it.
    let (rr, rg, rb) = if ready {
        (0.5 + haul * 0.5, 1.0, 0.5 - haul * 0.25)
    } else {
        (1.0, 0.82, 0.35)
    };
    let ring_alpha = if ready {
        0.55 + 0.35 * pulse + haul * 0.1
    } else {
        0.3 + 0.15 * pulse
    };
    let boundary = cached_stroke_circle(ctx, radius, 3.0)?;
    canvas.draw(
        &boundary,
        DrawParam::default()
            .dest(center)
            .color(Color::new(rr, rg, rb, ring_alpha.clamp(0.0, 1.0))),
    );
    // Inner accent ring, breathing on the beat.
    let inner = cached_stroke_circle(ctx, radius * 0.7, 1.5)?;
    canvas.draw(
        &inner,
        DrawParam::default()
            .dest(center)
            .color(Color::new(rr, rg, rb, (0.2 + beat * 0.5) * 0.6)),
    );

    // Anticipation "reach" ring — a second boundary that swells outward past the fence and fades,
    // pulsing faster and reaching further the bigger the incoming haul. It's the pen visibly
    // straining toward a fat train, telegraphing the jackpot as you drive it in. Only shows once
    // there's a real haul building (haul > ~a couple crabs' worth) so it stays quiet for small runs.
    if ready && haul > 0.12 {
        let reach_phase = (time * (2.0 + haul * 4.0)).sin() * 0.5 + 0.5; // 0..1
        let reach_r = radius * (1.0 + (0.15 + haul * 0.5) * reach_phase);
        let reach = cached_stroke_circle(ctx, reach_r, 2.0 + haul * 2.0)?;
        canvas.draw(
            &reach,
            DrawParam::default().dest(center).color(Color::new(
                0.6 + haul * 0.4,
                1.0,
                0.45,
                (haul * 0.55 * (1.0 - reach_phase)).clamp(0.0, 1.0),
            )),
        );
    }

    // Buoy posts around the rim, slowly turning like a rotating corral — spinning up with the haul.
    let post_count = 10;
    let spin = time * if ready { 0.9 + haul * 2.5 } else { 0.35 };
    for i in 0..post_count {
        let ang = spin + (i as f32 / post_count as f32) * std::f32::consts::TAU;
        let p = center + Vec2::new(ang.cos(), ang.sin()) * radius;
        let post_r = 4.0 + 1.5 * pulse;
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(p)
                .scale(Vec2::splat(post_r))
                .color(Color::new(rr, rg, rb, (0.6 + beat * 0.4).clamp(0.0, 1.0))),
        );
    }

    // Bobbing chevron beacon above the pen pointing down into it — "deliver here".
    let bob = (time * (if ready { 4.0 } else { 2.0 })).sin() * 6.0;
    let apex = center + Vec2::new(0.0, -radius - 26.0 + bob);
    let wing = 13.0;
    let drop = 15.0;
    let bright = (0.7 + 0.3 * pulse).clamp(0.0, 1.0);
    let beacon_col = Color::new(rr, rg, rb, bright);
    for side in [-1.0f32, 1.0] {
        let tip = apex + Vec2::new(side * wing, drop);
        let d = tip - apex;
        let len = d.length();
        let angle = d.y.atan2(d.x);
        canvas.draw(
            unit_line,
            DrawParam::default()
                .dest(apex)
                .rotation(angle)
                .scale(Vec2::new(len, 4.0))
                .color(beacon_col),
        );
    }

    // Delivery bloom — a jackpot flare right after a successful bank. Layered so cashing in the
    // train reads as a real payoff, not just a number ticking: an expanding shockwave ring, a
    // spinning starburst of god-rays, a rising column of light, and a hot core pop that all bloom
    // out of the pen and fade together. Everything except the single shockwave ring reuses the
    // already-fetched cached unit line/circle meshes (scaled via DrawParam), so this stays a
    // handful of draws with no per-frame GPU-buffer allocation.
    if flash > 0.0 {
        let f = flash.clamp(0.0, 1.0);
        let grow = 1.0 - f; // 0 at the instant of banking, 1 as the flare finishes

        // Expanding shockwave ring sweeping outward past the pen boundary.
        let burst_r = radius * (1.0 + grow * 1.4);
        let burst = cached_stroke_circle(ctx, burst_r, 4.0 + f * 8.0)?;
        canvas.draw(
            &burst,
            DrawParam::default()
                .dest(center)
                .color(Color::new(0.6, 1.0, 0.6, f)),
        );

        // Starburst of god-rays firing out of the pen, turning slowly as they stretch and fade.
        let ray_count = 12;
        let ray_spin = time * 1.5;
        let ray_len = radius * (0.5 + grow * 1.6);
        let ray_thick = (2.0 + f * 6.0).max(0.5);
        let ray_alpha = (f * 0.8).clamp(0.0, 1.0);
        for i in 0..ray_count {
            let ang = ray_spin + (i as f32 / ray_count as f32) * std::f32::consts::TAU;
            canvas.draw(
                unit_line,
                DrawParam::default()
                    .dest(center + Vec2::new(ang.cos(), ang.sin()) * radius * 0.25)
                    .rotation(ang)
                    .scale(Vec2::new(ray_len, ray_thick))
                    .color(Color::new(0.8, 1.0, 0.7, ray_alpha)),
            );
        }

        // Rising column of light — a bright shaft climbing out of the pen as the flare peaks.
        let col_h = radius * (1.2 + grow * 2.2);
        let col_w = (radius * 0.5 * f).max(1.0);
        canvas.draw(
            unit_line,
            DrawParam::default()
                .dest(center)
                .rotation(-std::f32::consts::FRAC_PI_2)
                .scale(Vec2::new(col_h, col_w))
                .color(Color::new(0.7, 1.0, 0.75, f * 0.5)),
        );

        // Hot core pop — a white-gold flash at the pen center, fiercest right as you bank.
        let core_r = radius * (0.35 + grow * 0.5);
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(center)
                .scale(Vec2::splat(core_r))
                .color(Color::new(1.0, 1.0, 0.85, f * f * 0.7)),
        );

        // Full-zone gold flare fading out.
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(center)
                .scale(Vec2::splat(radius))
                .color(Color::new(1.0, 0.9, 0.4, f * 0.4)),
        );

        // Cha-ching coin spray — a fountain of gold coin flecks bursting up and out of the pen the
        // instant a delivery banks, arcing under gravity and falling back like cash literally
        // spilling out of the corral. A different visual vocabulary from the rings/rays above
        // (discrete flecks vs. sweeping light), so it reads as "money paid out" rather than adding
        // to the same glow. Fully deterministic from `flash` (no state, no RNG, no new args): each
        // fleck follows a fixed launch angle/speed and the same 0→1 flight parameter the flare
        // decays over, so it's a pure function of the flash timer — just a handful of extra tinted
        // unit-circle draws, no per-frame allocation.
        let coin_count = 16;
        let flight = 1.0 - f; // 0 at the bank instant, 1 as the flare finishes — the arc's progress
        for i in 0..coin_count {
            // Fan the launch angles across an upward spread (straight up ± ~60°) so the spray
            // fountains up and outward rather than sideways into the ground.
            let t = i as f32 / (coin_count - 1) as f32;
            let launch = -std::f32::consts::FRAC_PI_2 + (t - 0.5) * 2.1;
            // Alternate flecks reach further so the fountain has depth instead of a single arc.
            let reach = radius * (1.1 + 0.7 * ((i * 7 % 5) as f32 / 4.0));
            let dist = reach * flight;
            // Parabolic lift: rises then falls back as flight goes 0→1 (peak at the midpoint).
            let lift = radius * 1.3 * (flight * (1.0 - flight)) * 4.0;
            let pos = center
                + Vec2::new(launch.cos() * dist, launch.sin() * dist)
                + Vec2::new(0.0, -lift);
            // Coins twinkle between bright gold and pale gold as they spin, and shrink/fade out.
            let twinkle = 0.75 + 0.25 * (time * 22.0 + i as f32 * 1.7).sin();
            let coin_r = (2.6 + f * 2.4) * (1.0 - flight * 0.4);
            canvas.draw(
                unit_circle,
                DrawParam::default()
                    .dest(pos)
                    .scale(Vec2::splat(coin_r))
                    .color(Color::new(1.0, 0.85 * twinkle, 0.3 * twinkle, f)),
            );
        }
    }

    canvas.set_blend_mode(orig_blend);

    // Live train-worth tag — the "bank now vs. push your luck" decision made legible. Floats a gold
    // "≈ N pts" readout above the pen while a train is loaded, so the player can see what the current
    // conga line is worth without banking to find out. It heats toward hot gold and bobs a little
    // more urgently as the haul grows (same anticipation curve as the pen itself), so a fat train
    // visibly advertises a fat payout. The Text is cached and only re-shaped when the value changes.
    if let Some(worth) = worth {
        thread_local! {
            static PEN_WORTH_CACHE: std::cell::RefCell<Option<(usize, Text, f32)>> =
                const { std::cell::RefCell::new(None) };
        }
        PEN_WORTH_CACHE.with(|cache| -> ggez::GameResult {
            let mut c = cache.borrow_mut();
            let needs = c.as_ref().map_or(true, |(v, _, _)| *v != worth);
            if needs {
                let mut t = Text::new(format!("~ {} pts", worth));
                t.set_scale(20.0);
                let w = t.measure(ctx)?.x;
                *c = Some((worth, t, w));
            }
            let (_, text, w) = c.as_ref().unwrap();
            let w = *w;
            // Bob above the pen, a touch livelier the hotter the haul. Sit clear of the fence ring.
            let bob = (time * (3.5 + haul * 4.0)).sin() * (2.0 + haul * 3.0);
            let base = center - Vec2::new(w * 0.5, radius + 34.0 - bob);
            // Go-green when small, heating to hot jackpot gold as the haul fattens — same palette
            // read as the pen ring, so the number and the corral agree on "this is money".
            let tr = 0.75 + haul * 0.25;
            let tg = 1.0 - haul * 0.28;
            let tb = 0.45 - haul * 0.3;
            // Soft dark backing so the tag stays legible over bright field/particles.
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
        })?;
    }

    Ok(())
}

/// Draw the live "at-risk" readout floating over the train's tail — the mirror of the gold pen-worth
/// tag, but for the *downside* of not banking. Where the pen tag says "this is what you'd bank", this
/// says "this is what a snap would cost you right now": a panic snap strips the last few (highest-value)
/// tail links, and because delivery value is triangular, those are exactly the ones worth the most. So
/// a long unbanked train reads as a loaded gun — the number climbs as you refuse to bank, in warning
/// red on the train itself (not gold over the pen), so the two tags contrast instead of blurring into
/// one. `at_risk` is the marginal pts a snap would remove (caller computes it with the same multipliers
/// as pen-worth, so the two agree). `tail` is where to anchor it. `danger01` is 0..1 = how close the
/// train length is to the deep-risk end, driving color/pulse urgency. Only call when a snap can actually
/// fire (train past the snap threshold) — below that there's genuinely no risk to show.
pub fn draw_train_at_risk(
    ctx: &mut Context,
    canvas: &mut Canvas,
    tail: Vec2,
    time: f32,
    at_risk: usize,
    danger01: f32,
) -> ggez::GameResult {
    thread_local! {
        static RISK_CACHE: std::cell::RefCell<Option<(usize, Text, f32)>> =
            const { std::cell::RefCell::new(None) };
    }
    RISK_CACHE.with(|cache| -> ggez::GameResult {
        let mut c = cache.borrow_mut();
        let needs = c.as_ref().map_or(true, |(v, _, _)| *v != at_risk);
        if needs {
            let mut t = Text::new(format!("AT RISK  -{} pts", at_risk));
            t.set_scale(17.0);
            let w = t.measure(ctx)?.x;
            *c = Some((at_risk, t, w));
        }
        let (_, text, w) = c.as_ref().unwrap();
        let w = *w;
        // A tense flicker that quickens with danger — the tag jitters harder the longer you hold.
        let pulse = 0.5 + 0.5 * (time * (6.0 + danger01 * 8.0)).sin();
        let jitter = pulse * danger01 * 1.5;
        // Sit just above the tail so it reads as attached to the train, not the pen.
        let base = tail - Vec2::new(w * 0.5, 30.0) + Vec2::new(jitter, 0.0);
        // Amber warning heating to angry red as the danger climbs — unmistakably NOT the gold reward tag.
        let rr = 1.0;
        let rg = 0.55 - danger01 * 0.45;
        let rb = 0.15;
        let alpha = 0.7 + 0.3 * pulse;
        canvas.draw(
            text,
            DrawParam::default()
                .dest(base + Vec2::splat(1.5))
                .color(Color::new(0.0, 0.0, 0.0, 0.6)),
        );
        canvas.draw(
            text,
            DrawParam::default()
                .dest(base)
                .color(Color::new(rr, rg.max(0.0), rb, alpha)),
        );
        Ok(())
    })
}

/// The positive twin of `draw_train_at_risk`: a live "HAUL WORTH" readout floating above the player
/// while they carry a train, showing what banking *right now* would pay — so the value you're building
/// is legible in the moment, not a surprise revealed only at the pen. When the train carries live
/// arrangement bonuses (same-type bonds / figurehead sandwiches / deep runs), it appends a compact
/// "ARRANGED +N" so the player can *see* their arrangement paying off and steer to complete more of it
/// — the agency/control the arrangement system was missing. `worth` is the total banked-now points
/// (caller computes it with the same helpers as the pen payout, so the two agree). `arranged` is the
/// arrangement-only slice of that worth (0 hides the suffix). `beat` (0..=1) gives it a gentle on-beat
/// bob so it feels alive. Anchored above `at` in warm gold — kin to the pen reward palette, the
/// opposite pole from the red AT RISK tag. Purely legibility; changes no odds.
pub fn draw_haul_worth(
    ctx: &mut Context,
    canvas: &mut Canvas,
    at: Vec2,
    time: f32,
    beat: f32,
    worth: usize,
    arranged: usize,
) -> ggez::GameResult {
    thread_local! {
        static HAUL_CACHE: std::cell::RefCell<Option<(usize, usize, Text, f32)>> =
            const { std::cell::RefCell::new(None) };
    }
    HAUL_CACHE.with(|cache| -> ggez::GameResult {
        let mut c = cache.borrow_mut();
        let needs = c
            .as_ref()
            .map_or(true, |(w, a, _, _)| *w != worth || *a != arranged);
        if needs {
            let label = if arranged > 0 {
                format!("HAUL  {}  ◆ ARRANGED +{}", worth, arranged)
            } else {
                format!("HAUL  {}", worth)
            };
            let mut t = Text::new(label);
            t.set_scale(16.0);
            let tw = t.measure(ctx)?.x;
            *c = Some((worth, arranged, t, tw));
        }
        let (_, _, text, tw) = c.as_ref().unwrap();
        let tw = *tw;
        // Gentle on-beat bob so it breathes with the groove without jittering like the risk tag.
        let bob = (time * 2.2).sin() * 2.0 - beat.clamp(0.0, 1.0) * 3.0;
        let base = at - Vec2::new(tw * 0.5, 42.0) + Vec2::new(0.0, bob);
        // Warm green-gold — the "come cash this in" palette, the opposite pole from the red AT RISK tag.
        let glow = 0.85 + beat.clamp(0.0, 1.0) * 0.15;
        canvas.draw(
            text,
            DrawParam::default()
                .dest(base + Vec2::splat(1.5))
                .color(Color::new(0.0, 0.0, 0.0, 0.6)),
        );
        canvas.draw(
            text,
            DrawParam::default().dest(base).color(Color::new(
                0.65 * glow + 0.2,
                1.0 * glow,
                0.45 * glow + 0.1,
                0.92,
            )),
        );
        Ok(())
    })
}

/// Telegraph an imminent kelp snag around the conga tail. `warn` (0..=1) is the rising tension the
/// sim raises while the tail sits in a kelp patch and eases back down once it routes clear (see
/// `kelp_snag_warn` / `snag_chain_on_kelp`). Draws two pulsing green fronds-warning rings that
/// tighten inward as the tension climbs, so a snag is *seen coming* and the player can dash out —
/// turning a random-feeling tail loss into a fair "route out NOW" call. Purely a legibility overlay;
/// it reuses the cached stroke-circle mesh and changes no gameplay odds. Skip entirely at warn≈0.
pub fn draw_kelp_snag_warning(
    ctx: &mut Context,
    canvas: &mut Canvas,
    tail: Vec2,
    time: f32,
    warn: f32,
) -> ggez::GameResult {
    if warn <= 0.02 {
        return Ok(());
    }
    // Pulse quickens with the tension so an about-to-snap tail visibly throbs harder.
    let pulse = 0.5 + 0.5 * (time * (5.0 + warn * 9.0)).sin();
    // Two rings that tighten inward as warn climbs — the "weeds closing in" read. The outer ring
    // starts wide and clamps toward the tail; a fainter inner ring trails it for depth.
    let outer_r = 34.0 - warn * 10.0 + pulse * 4.0;
    let inner_r = outer_r * 0.62;
    // Green kelp warning that deepens toward a hot lime as it peaks, unmistakably a hazard cue.
    let g = 0.85 + warn * 0.15;
    let alpha = (0.25 + 0.55 * warn) * (0.55 + 0.45 * pulse);
    for (r, a, th) in [(outer_r, alpha, 2.6), (inner_r, alpha * 0.6, 2.0)] {
        let mesh = cached_stroke_circle(ctx, r.max(4.0), th)?;
        canvas.draw(
            &mesh,
            DrawParam::default()
                .dest(tail)
                .color(Color::new(0.35, g, 0.4, a.min(1.0))),
        );
    }
    Ok(())
}

/// Draw the delivery-streak heat badge anchored under the pen — the persistent, watchable face of
/// the streak multiplier that until now only ever flashed for a frame at bank time and then decayed
/// silently. Banking crabs in quick succession stacks a payout multiplier (up to 2.75x); if too long
/// passes between banks the streak drops a notch (see `try_deliver_train` / the idle decay in
/// `update`). This badge makes both halves legible: the live multiplier reads at a glance, and as the
/// grace window runs down it heats and pulses with rising urgency so "bank again before you lose a
/// notch" becomes a tension the player can see and play to, instead of an invisible timer.
///
/// `mult` is the live streak multiplier (1.25x .. 2.75x). `decay01` is 0..1 = fraction of the grace
/// window remaining (1 just after a bank, 0 the instant before a notch drops), so the caller owns the
/// timer and this stays a pure readout. Only draw this when the streak is worth showing (>= 2 banks);
/// at streak 1 the multiplier is 1.0x and there's nothing at stake. Reuses the cached unit circle.
#[allow(clippy::too_many_arguments)]
pub fn draw_delivery_streak(
    ctx: &mut Context,
    canvas: &mut Canvas,
    center: Vec2,
    radius: f32,
    time: f32,
    mult: f32,
    decay01: f32,
) -> ggez::GameResult {
    let unit_circle = match UNIT_CIRCLE.get() {
        Some(mesh) => mesh,
        None => {
            let mesh =
                Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh)
        }
    };

    // Urgency ramps only in the last stretch of the grace window (below ~30% remaining), so the badge
    // sits calm most of the time and then visibly panics right before a notch drops — the SNAP-loss of
    // the delivery loop. 0 = safe, 1 = about to lose a notch.
    let urgency = (1.0 - decay01 / 0.3).clamp(0.0, 1.0);
    // Fast, insistent flash when urgent; a slow calm breath otherwise.
    let pulse = 0.5 + 0.5 * (time * (3.0 + urgency * 22.0)).sin();

    // Hot pink when safe (matches the "x{} STREAK" bank callout at [1.0,0.55,0.9]), flaring toward an
    // alarm red-orange as the streak is about to slip — the same warm-danger read as the "!" SNAP pops.
    let cr = 1.0;
    let cg = 0.55 - urgency * 0.35 + pulse * 0.1 * urgency;
    let cb = 0.9 - urgency * 0.75;

    let orig_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);
    // A pulsing halo ring beneath the pen — grows and brightens with urgency so a streak on the brink
    // throbs a warning without adding any HUD clutter (it lives on the pen the player's already watching).
    let ring_r = radius * (0.72 + 0.10 * pulse + urgency * 0.18);
    let ring_a = 0.18 + 0.22 * pulse * (0.4 + urgency);
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(center)
            .scale(Vec2::splat(ring_r))
            .color(Color::new(cr, cg.max(0.0), cb.max(0.0), ring_a)),
    );
    canvas.set_blend_mode(orig_blend);

    // The multiplier readout itself — cached, re-shaped only when the displayed value changes.
    thread_local! {
        static STREAK_MULT_CACHE: std::cell::RefCell<Option<(u32, Text, f32)>> =
            const { std::cell::RefCell::new(None) };
    }
    // Key on the two-decimal centi-multiplier so the Text rebuilds only on an actual value change.
    let key = (mult * 100.0).round() as u32;
    STREAK_MULT_CACHE.with(|cache| -> ggez::GameResult {
        let mut c = cache.borrow_mut();
        let needs = c.as_ref().map_or(true, |(k, _, _)| *k != key);
        if needs {
            let mut t = Text::new(format!("STREAK {:.2}x", mult));
            t.set_scale(18.0);
            let w = t.measure(ctx)?.x;
            *c = Some((key, t, w));
        }
        let (_, text, w) = c.as_ref().unwrap();
        let w = *w;
        // Sit just below the pen, opposite the worth tag above it. A tiny urgency jitter shakes the
        // tag when a notch-drop is imminent so the warning reads even without color.
        let jitter = if urgency > 0.5 {
            (time * 40.0).sin() * urgency * 2.0
        } else {
            0.0
        };
        let base = center + Vec2::new(-w * 0.5 + jitter, radius + 12.0);
        canvas.draw(
            text,
            DrawParam::default()
                .dest(base + Vec2::splat(1.5))
                .color(Color::new(0.0, 0.0, 0.0, 0.6)),
        );
        canvas.draw(
            text,
            DrawParam::default().dest(base).color(Color::new(
                cr,
                (cg + 0.2).min(1.0),
                cb.max(0.15),
                0.7 + 0.3 * pulse,
            )),
        );
        Ok(())
    })?;

    Ok(())
}

/// Draw a directional guide toward the delivery pen while the player has an uncashed train.
///
/// The pen relocates on every bank, so once you've built a conga line the game's biggest payoff
/// decision — "route the train to the pen and cash in" — is only legible if you can actually *find*
/// the pen. The crab radar already points to loose crabs at the screen edge; this is the same idea
/// for the goal zone, so building a train and hunting blindly for where to spend it never happens.
///
/// `urgency` (0..1) scales how insistent the guide reads — feed it the train size normalized against
/// some "big haul" cap so a fat, at-risk train pulls harder toward the pen than a couple of crabs.
/// When the pen is off-screen the arrow pins to the screen edge (like the crab radar); when it's
/// on-screen but not yet reached, a softer floating chevron hovers beside it pointing in. Purely a
/// guide overlay: no gameplay effect, all draws reuse the cached unit line/circle meshes.
#[allow(clippy::too_many_arguments)]
pub fn draw_pen_guide(
    ctx: &mut Context,
    canvas: &mut Canvas,
    player_center: Vec2,
    pen_pos: Vec2,
    pen_radius: f32,
    width: f32,
    height: f32,
    cam: Vec2,
    urgency: f32,
    beat_intensity: f32,
    time: f32,
) -> ggez::GameResult {
    let to_pen = pen_pos - player_center;
    let dist = to_pen.length();
    // Already at (or basically on) the pen — the pen's own beacon takes over, no guide needed.
    if dist < pen_radius * 1.2 {
        return Ok(());
    }
    let dir = to_pen.normalize_or_zero();
    if dir == Vec2::ZERO {
        return Ok(());
    }
    let angle = dir.y.atan2(dir.x);

    let u = urgency.clamp(0.0, 1.0);
    let beat = beat_intensity.clamp(0.0, 1.0);
    let unit_line = unit_line(ctx)?;
    let unit_circle = unit_circle(ctx)?;

    // The pen lives in world space; the viewport is offset by the camera origin. Test on-screen
    // against the viewport (world coord minus camera), and — since this draws in the world pass —
    // build any edge-pinned arrow as a world coordinate at the viewport border (cam + screen edge).
    let margin = 30.0_f32;
    let pen_screen = pen_pos - cam;
    let on_screen = pen_screen.x > margin
        && pen_screen.x < width - margin
        && pen_screen.y > margin
        && pen_screen.y < height - margin;

    let orig_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);

    // Warm green-gold, matching the pen's "come cash in" palette. Brightens with urgency + beat.
    let bright = (0.6 + u * 0.35 + beat * 0.15).clamp(0.0, 1.0);
    let col = Color::new(
        0.55 * bright + 0.25,
        1.0 * bright,
        0.5 * bright + 0.15,
        bright,
    );

    // Draw a downward-into-the-pen chevron (two wings) pointing along `angle`, plus a soft dot,
    // at `at` with size `size`. Reused for both the edge-pinned and on-field cases.
    let mut chevron = |at: Vec2, size: f32| {
        let wing = size;
        let core = size * 0.55;
        // Soft glow dot behind the chevron so it reads against busy ground.
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(at)
                .scale(Vec2::splat(core))
                .color(Color::new(col.r, col.g, col.b, col.a * 0.5)),
        );
        for spread in [2.2_f32, -2.2] {
            let wa = angle + spread;
            let d = Vec2::new(wa.cos(), wa.sin()) * wing;
            let len = d.length();
            let a = d.y.atan2(d.x);
            canvas.draw(
                unit_line,
                DrawParam::default()
                    .dest(at)
                    .rotation(a)
                    .scale(Vec2::new(len, (3.0 + u * 3.0).max(1.0)))
                    .color(col),
            );
        }
    };

    if on_screen {
        // Pen is visible: hover a gentle chevron just off the near side of the pen, bobbing on the
        // beat, nudging the eye toward it without cluttering the goal zone itself.
        let bob = (time * (3.0 + u * 3.0)).sin() * (4.0 + u * 4.0);
        let at = pen_pos - dir * (pen_radius + 22.0 + bob);
        chevron(at, 14.0 + u * 6.0);
    } else {
        // Pen is off-screen: pin a bigger, more insistent arrow to the screen edge in the pen's
        // direction (same clamp trick as the crab radar), so you know which way to haul the train.
        // Compute the edge in SCREEN space (player projected to viewport, clamped to the border),
        // then add the camera origin back so it lands at the right world coord in this world pass.
        let player_screen = player_center - cam;
        let edge = cam
            + Vec2::new(
                (player_screen.x + dir.x * 4000.0).clamp(margin, width - margin),
                (player_screen.y + dir.y * 4000.0).clamp(margin, height - margin),
            );
        let pulse = 1.0 + beat * 0.4 + (time * 6.0).sin() * 0.1;
        chevron(edge, (18.0 + u * 10.0) * pulse);
        // A faint trailing tick behind the edge arrow so it reads as "keep going this way".
        let tail = edge - dir * (26.0 + u * 10.0);
        canvas.draw(
            unit_circle,
            DrawParam::default()
                .dest(tail)
                .scale(Vec2::splat(3.0 + u * 2.0))
                .color(Color::new(col.r, col.g, col.b, col.a * 0.4)),
        );
    }

    canvas.set_blend_mode(orig_blend);
    Ok(())
}
