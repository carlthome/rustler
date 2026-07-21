//! Floating score/label text and penned-marcher parade — two purely cosmetic systems that
//! share no logic with the rest of graphics.rs and are extracted here to keep that file smaller.

use crate::constants::CRAB_SIZE;
use crate::graphics::InstancedMeshExt;
use ggez::Context;
use ggez::glam::Vec2;
use ggez::graphics::{BlendMode, Canvas, Color, DrawMode, DrawParam, InstanceArray, Mesh};
use std::cell::RefCell;

// ---------------------------------------------------------------------------
// Thread-local GPU instance buffers for draw_penned_marchers
// ---------------------------------------------------------------------------

thread_local! {
    // Reusable instance buffers for draw_penned_marchers' three passes (shadow, body, rim
    // highlight) — same batching technique as the particle/leg/body/trail instances in graphics.rs.
    // A big bank can queue up to 40 marchers at once, each previously issuing 3 individual
    // canvas.draw() calls (shadow + body + rim), i.e. up to 120 separate GPU submissions for a
    // purely cosmetic parade. Filling one InstanceArray per pass collapses that to 3 draw calls
    // total regardless of marcher count, with identical on-screen output.
    static MARCHER_SHADOW_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    static MARCHER_BODY_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    static MARCHER_RIM_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);
    // Reusable scratch DrawParam buffers — cleared and refilled each call rather than freshly
    // allocated, the same pattern every other batched draw function in this codebase uses.
    static MARCHER_SHADOW_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static MARCHER_BODY_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static MARCHER_RIM_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());

    // Unit circle for marcher bodies — same OnceLock-on-first-use pattern as in graphics.rs.
    static MARCHER_UNIT_CIRCLE: RefCell<Option<Mesh>> = RefCell::new(None);
}

// ---------------------------------------------------------------------------
// FloatingText
// ---------------------------------------------------------------------------

pub struct FloatingText {
    pub text: String,
    pub pos: Vec2,
    pub vel: Vec2,
    pub life: f32,
    pub max_life: f32,
    pub scale: f32,
    pub color: [f32; 4], // rgba 0..1
    // Glyph-shaped Text object built once at spawn — reused every frame so we avoid re-running
    // ggez's layout/shaping pass on every draw call. The scale set here is the logical font
    // size (ft.scale); the per-frame fade-pop factor is applied via DrawParam::scale instead,
    // which only transforms the already-rasterized glyphs without re-shaping.
    pub cached_text: ggez::graphics::Text,
}

pub struct FloatingTextSystem {
    pub texts: Vec<FloatingText>,
}

impl FloatingTextSystem {
    pub fn new() -> Self {
        Self { texts: Vec::new() }
    }

    pub fn spawn(&mut self, text: String, pos: Vec2, scale: f32, color: [f32; 4]) {
        // Build the Text object once at spawn (glyph shaping/layout runs here, not per frame).
        // set_scale bakes the logical font size into the layout; per-frame DrawParam::scale
        // only transforms the rasterized result without re-triggering shaping.
        let mut cached_text = ggez::graphics::Text::new(&text);
        cached_text.set_scale(scale);
        self.texts.push(FloatingText {
            text,
            pos,
            vel: Vec2::new(0.0, -90.0),
            life: 1.1,
            max_life: 1.1,
            scale,
            color,
            cached_text,
        });
    }

    pub fn update(&mut self, dt: f32) {
        self.texts.retain_mut(|t| {
            t.life -= dt;
            t.pos += t.vel * dt;
            t.vel.y *= 0.97;
            t.life > 0.0
        });
    }
}

pub fn draw_floating_texts(
    _ctx: &mut Context,
    canvas: &mut Canvas,
    system: &FloatingTextSystem,
) -> ggez::GameResult {
    for ft in &system.texts {
        let ratio = ft.life / ft.max_life;
        let alpha = (ft.color[3] * ratio).clamp(0.0, 1.0);
        let color = Color::new(ft.color[0], ft.color[1], ft.color[2], alpha);
        // Slight upward scale pop at start, shrinks as it fades. Applied via DrawParam::scale
        // so we transform the already-rasterized glyphs from ft.cached_text rather than
        // rebuilding the Text (glyph shaping) every frame. Factor ≤1.0 so the downscale of
        // full-size glyphs stays clean even under nearest-clamp sampling.
        let pop = 0.8 + 0.2 * ratio;
        canvas.draw(
            &ft.cached_text,
            DrawParam::default()
                .dest(ft.pos)
                .scale(Vec2::splat(pop))
                .color(color),
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// PennedMarcher — delivered crabs parading into the pen after a bank
// ---------------------------------------------------------------------------

/// One caught crab marching into the delivery pen after a bank. Instead of the banked train
/// vanishing instantly, each delivered crab spawns a marcher that skips from where it was into
/// the pen center, staggered by its old chain index so the whole conga line files in one-by-one
/// like a little parade, then pops in a sparkle on arrival. Purely cosmetic — the score is already
/// awarded the instant the bank happens.
pub struct PennedMarcher {
    pub start: Vec2,     // where the crab was when the train banked
    pub target: Vec2,    // pen center it's marching toward
    pub color: [f32; 3], // the crab's own body color
    pub size: f32,       // body radius (from its scale)
    pub delay: f32,      // seconds before it starts marching (stagger by chain index)
    pub t: f32,          // 0..1 march progress once delay elapses
    pub speed: f32,      // 1/seconds to cover the march
    pub done: bool,      // true once it has arrived (its arrival pop was emitted)
}

pub struct PennedMarcherSystem {
    pub marchers: Vec<PennedMarcher>,
}

impl PennedMarcherSystem {
    pub fn new() -> Self {
        Self {
            marchers: Vec::new(),
        }
    }

    /// Queue the just-banked train to march into the pen. `crabs` is (pos, color, size) per
    /// delivered crab in chain order (head first) so they file in staggered down the line.
    pub fn spawn_train(&mut self, pen: Vec2, crabs: &[(Vec2, [f32; 3], f32)]) {
        // Cap the queue so an enormous bank can't spawn hundreds of marchers at once.
        const MAX_MARCHERS: usize = 40;
        let count = crabs.len().min(MAX_MARCHERS);
        for (i, &(pos, color, size)) in crabs.iter().take(count).enumerate() {
            // Later links in the line start marching a beat behind the head — a rolling parade.
            let delay = i as f32 * 0.045;
            // Shorter marches finish a touch quicker so nothing drags; keeps the parade snappy.
            let dist = pos.distance(pen).max(1.0);
            let speed = (260.0 / dist).clamp(0.9, 2.4);
            self.marchers.push(PennedMarcher {
                start: pos,
                target: pen,
                color,
                size,
                delay,
                t: 0.0,
                speed,
                done: false,
            });
        }
    }

    /// Advance every marcher. Appends the arrival (pos, color) of any that reached the pen this
    /// frame into `arrivals` so the caller can pop a sparkle burst there via the particle system.
    /// Takes a scratch buffer instead of returning a fresh Vec so no heap allocation fires on
    /// frames when marchers are active — the caller clears it before the call, then iterates it.
    pub fn update(&mut self, dt: f32, arrivals: &mut Vec<(Vec2, [f32; 3])>) {
        arrivals.clear();
        for m in self.marchers.iter_mut() {
            if m.delay > 0.0 {
                m.delay -= dt;
                continue;
            }
            m.t = (m.t + dt * m.speed).min(1.0);
            if m.t >= 1.0 && !m.done {
                m.done = true;
                arrivals.push((m.target, m.color));
            }
        }
        // Drop marchers that have arrived (they popped their sparkle already).
        self.marchers.retain(|m| !m.done);
    }

    /// Current drawn position of a marcher: eased march from start toward the pen with a small
    /// arc lift so it hops in rather than sliding flat.
    fn marcher_draw(m: &PennedMarcher) -> (Vec2, f32, f32) {
        // Ease-in-out so it accelerates off the mark and settles into the pen.
        let t = m.t;
        let eased = t * t * (3.0 - 2.0 * t);
        let pos = m.start.lerp(m.target, eased);
        // A parabolic hop: peaks mid-march, zero at both ends.
        let lift = (t * (1.0 - t)) * 4.0 * 26.0;
        // Shrink as it nears the pen so it reads as "filing away" into the corral.
        let shrink = 1.0 - 0.35 * eased;
        (pos - Vec2::new(0.0, lift), lift, shrink)
    }
}

/// Draw the crabs currently marching into the pen. Cheap: a shadow + colored body disc + a
/// bright rim per marcher, reusing the shared unit circle — it evokes the crab silhouette
/// without rigging full legs, since these are only on screen for a fraction of a second.
/// All three passes are batched into one InstanceArray draw call each (MARCHER_*_INSTANCES)
/// instead of issuing a canvas.draw() per marcher per pass, the same technique used for crab
/// legs/bodies and catch trails in graphics.rs.
pub fn draw_penned_marchers(
    ctx: &mut Context,
    canvas: &mut Canvas,
    system: &PennedMarcherSystem,
    time: f32,
) -> ggez::GameResult {
    if system.marchers.is_empty() {
        return Ok(());
    }

    // Initialise (or reuse) the local unit circle for marcher draws.
    let unit_circle = MARCHER_UNIT_CIRCLE.with(|cell| -> ggez::GameResult<Mesh> {
        let mut slot = cell.borrow_mut();
        if let Some(ref mesh) = *slot {
            return Ok(mesh.clone());
        }
        let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
        *slot = Some(mesh.clone());
        Ok(mesh)
    })?;

    MARCHER_SHADOW_PARAMS.with(|shadow_cell| -> ggez::GameResult {
        MARCHER_BODY_PARAMS.with(|body_cell| -> ggez::GameResult {
            MARCHER_RIM_PARAMS.with(|rim_cell| -> ggez::GameResult {
                let mut shadow_params = shadow_cell.borrow_mut();
                let mut body_params = body_cell.borrow_mut();
                let mut rim_params = rim_cell.borrow_mut();
                shadow_params.clear();
                body_params.clear();
                rim_params.clear();

                for m in &system.marchers {
                    if m.delay > 0.0 {
                        continue;
                    }
                    let (pos, lift, shrink) = PennedMarcherSystem::marcher_draw(m);
                    let body_r = CRAB_SIZE * 0.5 * m.size * shrink;

                    let sh_scale = (1.0 - lift / 60.0).clamp(0.4, 1.0);
                    let sh_alpha = ((1.0 - lift / 55.0) * 90.0).clamp(15.0, 90.0) as u8;
                    shadow_params.push(
                        DrawParam::default()
                            .dest(pos + Vec2::new(0.0, body_r * 0.7 + lift * 0.6))
                            .scale(Vec2::new(body_r * sh_scale, body_r * 0.45 * sh_scale))
                            .color(Color::from_rgba(0, 0, 0, sh_alpha)),
                    );

                    // A little beat-independent bob so the parade feels alive.
                    let bob = 1.0 + 0.08 * (time * 9.0 + m.start.x).sin();
                    let [r, g, b] = m.color;
                    body_params.push(
                        DrawParam::default()
                            .dest(pos)
                            .scale(Vec2::splat(body_r * bob))
                            .color(Color::new(r, g, b, 1.0)),
                    );

                    rim_params.push(
                        DrawParam::default()
                            .dest(pos - Vec2::new(0.0, body_r * 0.3))
                            .scale(Vec2::splat(body_r * 0.45))
                            .color(Color::new(
                                (r + 0.4).min(1.0),
                                (g + 0.4).min(1.0),
                                (b + 0.4).min(1.0),
                                0.7,
                            )),
                    );
                }

                // Shadows first (normal blend so they read as ground contact).
                MARCHER_SHADOW_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                    let mut inst_slot = inst_cell.borrow_mut();
                    let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                    instances.set(shadow_params.iter().copied());
                    canvas.draw_instanced_mesh_guarded(
                        unit_circle.clone(),
                        instances,
                        DrawParam::default(),
                    );
                    Ok(())
                })?;

                // Bodies + rims in additive so they glow warm as they file in.
                let orig_blend = canvas.blend_mode();
                MARCHER_BODY_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                    let mut inst_slot = inst_cell.borrow_mut();
                    let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                    instances.set(body_params.iter().copied());
                    canvas.draw_instanced_mesh_guarded(
                        unit_circle.clone(),
                        instances,
                        DrawParam::default(),
                    );
                    Ok(())
                })?;

                // Bright rim highlight, additive, so the marchers pop against the pen glow.
                canvas.set_blend_mode(BlendMode::ADD);
                MARCHER_RIM_INSTANCES.with(|inst_cell| -> ggez::GameResult {
                    let mut inst_slot = inst_cell.borrow_mut();
                    let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
                    instances.set(rim_params.iter().copied());
                    canvas.draw_instanced_mesh_guarded(
                        unit_circle,
                        instances,
                        DrawParam::default(),
                    );
                    Ok(())
                })?;
                canvas.set_blend_mode(orig_blend);
                Ok(())
            })
        })
    })
}
