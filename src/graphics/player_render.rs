//! Player-avatar rendering: the rustler sprite (beat hop, squash & stretch, lean, drop
//! shadow) and its cosmetic layers (hats, facial hair, accessories), plus the per-skin
//! cosmetics-mesh cache. Extracted from graphics/mod.rs to keep that file navigable — pure
//! structural move, no behaviour change.

use super::{UNIT_CIRCLE, unit_circle, unit_line};
use crate::skins::{Accessory, FacialHair, Hat, PlayerSkin};
use ggez::Context;
use ggez::glam::Vec2;
use ggez::graphics::{Canvas, Color, DrawMode, DrawParam, Image, Mesh, Rect};
use std::cell::RefCell;

thread_local! {
    // Player cosmetics mesh cache: pre-built meshes for hat/facial-hair/accessory combos,
    // keyed by (Hat, FacialHair, Accessory). Each entry is a Vec of (Mesh, DrawParam) where
    // the DrawParam's dest is a body-space offset from the crab centre (c = Vec2::ZERO when
    // built). At draw time we translate each param by the actual `c` (centre + beat-hop).
    // draw_player_cosmetics was rebuilding up to ~8 fresh Mesh::new_rectangle/new_polygon/
    // new_circle GPU buffers every frame — constant cost regardless of game state since the
    // player is always drawn. Cached once per session per skin choice: the meshes are
    // dimensioned off `dims` which is constant (sprite size is fixed) and keyed on the
    // enum triple so a skin-picker change invalidates them automatically.
    static COSMETICS_MESH_CACHE: RefCell<Option<(PlayerSkin, Vec<(Mesh, DrawParam)>)>> =
        RefCell::new(None);
}

pub fn draw_rustler(
    ctx: &mut Context,
    canvas: &mut Canvas,
    pos: Vec2,
    sprite: &Image,
    velocity: Vec2,
    beat_intensity: f32,
    time: f32,
    dashing: bool,
    skin: PlayerSkin,
) -> ggez::GameResult {
    let base = 0.05_f32;
    let dims = Vec2::new(sprite.width() as f32, sprite.height() as f32) * base;
    // Keep the sprite centered on the same point it used to occupy (top-left was
    // pos + (15,15) at 0.05 scale) so transforms can pivot around the center.
    let center = pos + Vec2::new(15.0, 15.0) + dims * 0.5;

    let beat = beat_intensity.clamp(0.0, 1.0);

    // Beat-synced hop: the rustler pops upward on every downbeat like everything else
    // in the conga, plus a gentle idle breathing bob so it's never fully still.
    let hop = beat * 8.0;
    let idle = (time * 2.2).sin() * 1.5;
    let bob = -hop + idle;

    // Squash & stretch: stretch tall on the up-beat, and stretch along the run when
    // moving fast (extra on a dash) for a snappy sense of momentum.
    let hspeed = velocity.x.abs();
    let run_stretch = (hspeed / 200.0).clamp(0.0, 1.0) * if dashing { 0.20 } else { 0.09 };
    let sx = base * (1.0 - beat * 0.08 + run_stretch);
    let sy = base * (1.0 + beat * 0.13 - run_stretch * 0.5);

    // Lean into horizontal movement — tilt forward as if leaning into the run.
    let lean_amt = if dashing { 0.26 } else { 0.16 };
    let lean = (velocity.x / 200.0).clamp(-1.0, 1.0) * lean_amt;

    // Grounding drop shadow that shrinks and fades as the rustler leaves the ground.
    let unit_circle = match UNIT_CIRCLE.get() {
        Some(mesh) => mesh,
        None => {
            let mesh = Mesh::new_circle(ctx, DrawMode::fill(), [0.0, 0.0], 1.0, 0.02, Color::WHITE)?;
            UNIT_CIRCLE.get_or_init(|| mesh)
        }
    };
    let ground_y = center.y + dims.y * 0.42;
    let lift = hop.max(0.0);
    let shadow_shrink = (1.0 - lift * 0.02).clamp(0.55, 1.0);
    let shadow_alpha = (0.32 * shadow_shrink).clamp(0.0, 1.0);
    canvas.draw(
        unit_circle,
        DrawParam::default()
            .dest(Vec2::new(center.x, ground_y))
            .scale(Vec2::new(
                dims.x * 0.34 * shadow_shrink,
                dims.y * 0.13 * shadow_shrink,
            ))
            .color(Color::new(0.0, 0.0, 0.0, shadow_alpha)),
    );

    // Draw the sprite pivoting around its center so the hop, squash and lean all
    // anchor sensibly.
    canvas.draw(
        sprite,
        DrawParam::default()
            .dest(Vec2::new(center.x, center.y + bob))
            .offset(Vec2::new(0.5, 0.5))
            .rotation(lean)
            .scale(Vec2::new(sx, sy))
            .color(Color::from_rgba(255, 255, 255, 255)),
    );

    // Cosmetic layers: hat / facial hair / accessory. Everything is drawn on top of the
    // sprite and anchored to `center` + `bob` (the same hop offset the sprite uses) so it
    // sticks to the crab through the beat hop. All offsets scale off `dims` (the on-screen
    // crab size) so they stay proportional. `w`/`h` are the sprite's on-screen extents.
    draw_player_cosmetics(ctx, canvas, center + Vec2::new(0.0, bob), dims, skin)?;

    Ok(())
}

/// Draw the player's chosen cosmetics on top of the crab sprite. `c` is the sprite centre
/// (already including the beat hop), `dims` its on-screen size. All offsets are proportional
/// to `dims` so the drip reads correctly at any player scale.
///
/// Meshes for hats/facial-hair/accessories are built once per skin choice (in origin space,
/// with c = Vec2::ZERO) and cached in COSMETICS_MESH_CACHE. On every subsequent frame the
/// function just iterates the cached Vec and translates each mesh's DrawParam by the current
/// `c`. This eliminates up to ~8 Mesh::new_rectangle/new_polygon/new_circle GPU allocations
/// per frame (constant cost, every frame the player is drawn) for all non-default skins.
fn draw_player_cosmetics(
    ctx: &mut Context,
    canvas: &mut Canvas,
    c: Vec2,
    dims: Vec2,
    skin: PlayerSkin,
) -> ggez::GameResult {
    // Try the fast path first: if the cached skin matches, just translate each mesh by `c`
    // and draw. No allocations, no mesh building.
    let cache_hit = COSMETICS_MESH_CACHE.with(|cache| {
        let cache = cache.borrow();
        if let Some((cached_skin, _)) = cache.as_ref() {
            *cached_skin == skin
        } else {
            false
        }
    });

    if !cache_hit {
        // Build the meshes with c = Vec2::ZERO so the DrawParams encode body-local offsets.
        let meshes = build_cosmetics_meshes(ctx, dims, skin)?;
        COSMETICS_MESH_CACHE.with(|cache| {
            *cache.borrow_mut() = Some((skin, meshes));
        });
    }

    // Draw cached meshes, translating each body-local DrawParam by the current `c` (which
    // changes every frame due to the beat hop). Reconstruct the translated DrawParam inline
    // from the cached one so we never allocate: just patch the dest field.
    COSMETICS_MESH_CACHE.with(|cache| -> ggez::GameResult {
        let cache = cache.borrow();
        if let Some((_, meshes)) = cache.as_ref() {
            for (mesh, param) in meshes {
                // Translate the body-local dest by the actual sprite centre `c`.
                let mut p = *param;
                if let ggez::graphics::Transform::Values { ref mut dest, .. } = p.transform {
                    dest.x += c.x;
                    dest.y += c.y;
                }
                canvas.draw(mesh, p);
            }
        }
        Ok(())
    })
}

/// Build the cosmetics meshes for `skin` in body-local space (c = Vec2::ZERO). Returns a
/// Vec of (Mesh, DrawParam) where DrawParam.dest is the body-local offset from the crab
/// centre. Called at most once per skin choice per session.
fn build_cosmetics_meshes(
    ctx: &mut Context,
    dims: Vec2,
    skin: PlayerSkin,
) -> ggez::GameResult<Vec<(Mesh, DrawParam)>> {
    let w = dims.x;
    let h = dims.y;

    // Reference points in body-local coords (c = Vec2::ZERO).
    // The sprite's geometric centre sits in the leg area; the face is well above it.
    let ht = Vec2::new(0.0, -h * 0.40); // head_top
    let fa = Vec2::new(0.0, -h * 0.20); // face / eye-level
    let mo = Vec2::new(0.0, -h * 0.08); // mouth (below eyes, still in upper shell)
    let sh = Vec2::new(0.0,  h * 0.10); // shell / chest

    let col = |r: u8, g: u8, b: u8| Color::from_rgb(r, g, b);

    // Helper: build a Mesh::new_rectangle with body-local coords and return it alongside a
    // zero-dest DrawParam (dest is already baked into the Rect's origin).
    let rect_mesh = |ctx: &mut Context, rect: Rect, color: Color| -> ggez::GameResult<(Mesh, DrawParam)> {
        let m = Mesh::new_rectangle(ctx, DrawMode::fill(), rect, color)?;
        Ok((m, DrawParam::default()))
    };

    let mut out: Vec<(Mesh, DrawParam)> = Vec::new();

    // ---- Hats -------------------------------------------------------------------------
    match skin.hat {
        Hat::None => {}
        Hat::Cowboy => {
            let brim = col(0xC8, 0xA4, 0x6E);
            out.push(rect_mesh(ctx, Rect::new(ht.x - w * 0.32, ht.y + h * 0.04, w * 0.64, h * 0.07), brim)?);
            out.push(rect_mesh(ctx, Rect::new(ht.x - w * 0.15, ht.y - h * 0.10, w * 0.30, h * 0.15), brim)?);
        }
        Hat::TopHat => {
            let black = col(0x1A, 0x1A, 0x2E);
            out.push(rect_mesh(ctx, Rect::new(ht.x - w * 0.28, ht.y + h * 0.06, w * 0.56, h * 0.06), black)?);
            out.push(rect_mesh(ctx, Rect::new(ht.x - w * 0.14, ht.y - h * 0.20, w * 0.28, h * 0.28), black)?);
        }
        Hat::Sombrero => {
            // Unit-circle items: clone the static mesh, encode offset in DrawParam.dest.
            let uc = unit_circle(ctx)?.clone();
            let yellow = col(0xF5, 0xC8, 0x42);
            out.push((uc.clone(), DrawParam::default()
                .dest(Vec2::new(ht.x, ht.y + h * 0.10))
                .scale(Vec2::new(w * 0.48, h * 0.10))
                .color(yellow)));
            out.push((uc, DrawParam::default()
                .dest(Vec2::new(ht.x, ht.y + h * 0.02))
                .scale(Vec2::new(w * 0.16, h * 0.14))
                .color(yellow)));
        }
        Hat::Bucket => {
            let olive = col(0x7A, 0x8C, 0x5E);
            out.push(rect_mesh(ctx, Rect::new(ht.x - w * 0.24, ht.y + h * 0.08, w * 0.48, h * 0.05), olive)?);
            out.push(rect_mesh(ctx, Rect::new(ht.x - w * 0.18, ht.y - h * 0.02, w * 0.36, h * 0.11), olive)?);
        }
        Hat::Bandana => {
            let red = col(0xD9, 0x3B, 0x3B);
            out.push(rect_mesh(ctx, Rect::new(ht.x - w * 0.26, ht.y + h * 0.06, w * 0.52, h * 0.08), red)?);
            let knot = Mesh::new_polygon(ctx, DrawMode::fill(), &[
                [ht.x + w * 0.26, ht.y + h * 0.06],
                [ht.x + w * 0.40, ht.y + h * 0.02],
                [ht.x + w * 0.40, ht.y + h * 0.18],
            ], red)?;
            out.push((knot, DrawParam::default()));
        }
        Hat::Beret => {
            let uc = unit_circle(ctx)?.clone();
            let teal = col(0x2E, 0x7D, 0x6E);
            out.push((uc.clone(), DrawParam::default()
                .dest(Vec2::new(ht.x - w * 0.06, ht.y + h * 0.06))
                .scale(Vec2::new(w * 0.22, h * 0.13))
                .rotation(-0.35)
                .color(teal)));
            out.push((uc, DrawParam::default()
                .dest(Vec2::new(ht.x + w * 0.10, ht.y - h * 0.02))
                .scale(Vec2::splat(w * 0.03))
                .color(teal)));
        }
        Hat::Crown => {
            let gold = col(0xFF, 0xD7, 0x00);
            let base_y = ht.y + h * 0.10;
            let pts = [
                [ht.x - w * 0.22, base_y],
                [ht.x - w * 0.22, ht.y - h * 0.02],
                [ht.x - w * 0.11, base_y - h * 0.06],
                [ht.x,            ht.y - h * 0.06],
                [ht.x + w * 0.11, base_y - h * 0.06],
                [ht.x + w * 0.22, ht.y - h * 0.02],
                [ht.x + w * 0.22, base_y],
            ];
            let crown = Mesh::new_polygon(ctx, DrawMode::fill(), &pts, gold)?;
            out.push((crown, DrawParam::default()));
        }
        Hat::HardHat => {
            let yellow = col(0xFF, 0xD6, 0x00);
            let uc = unit_circle(ctx)?.clone();
            out.push((uc, DrawParam::default()
                .dest(Vec2::new(ht.x, ht.y + h * 0.06))
                .scale(Vec2::new(w * 0.22, h * 0.20))
                .color(yellow)));
            out.push(rect_mesh(ctx, Rect::new(ht.x - w * 0.22, ht.y + h * 0.10, w * 0.44, h * 0.04), yellow)?);
        }
    }

    // ---- Facial hair ------------------------------------------------------------------
    let brown = col(0x6B, 0x3D, 0x1E);
    match skin.facial_hair {
        FacialHair::None => {}
        FacialHair::Mustache => {
            let m = Mesh::new_polygon(ctx, DrawMode::fill(), &[
                [mo.x - w * 0.16, mo.y - h * 0.02],
                [mo.x,            mo.y + h * 0.01],
                [mo.x + w * 0.16, mo.y - h * 0.02],
                [mo.x + w * 0.14, mo.y + h * 0.04],
                [mo.x,            mo.y + h * 0.03],
                [mo.x - w * 0.14, mo.y + h * 0.04],
            ], brown)?;
            out.push((m, DrawParam::default()));
        }
        FacialHair::Handlebar => {
            let m = Mesh::new_polygon(ctx, DrawMode::fill(), &[
                [mo.x - w * 0.26, mo.y - h * 0.06],
                [mo.x - w * 0.18, mo.y + h * 0.02],
                [mo.x,            mo.y + h * 0.03],
                [mo.x + w * 0.18, mo.y + h * 0.02],
                [mo.x + w * 0.26, mo.y - h * 0.06],
                [mo.x + w * 0.20, mo.y + h * 0.02],
                [mo.x,            mo.y + h * 0.06],
                [mo.x - w * 0.20, mo.y + h * 0.02],
            ], brown)?;
            out.push((m, DrawParam::default()));
        }
        FacialHair::Beard => {
            out.push(rect_mesh(ctx, Rect::new(mo.x - w * 0.18, mo.y, w * 0.36, h * 0.22), brown)?);
            let uc = unit_circle(ctx)?.clone();
            out.push((uc, DrawParam::default()
                .dest(Vec2::new(mo.x, mo.y + h * 0.22))
                .scale(Vec2::new(w * 0.18, h * 0.09))
                .color(brown)));
        }
        FacialHair::GoateePatch => {
            let uc = unit_circle(ctx)?.clone();
            out.push((uc, DrawParam::default()
                .dest(Vec2::new(mo.x, mo.y + h * 0.09))
                .scale(Vec2::new(w * 0.07, h * 0.07))
                .color(brown)));
        }
        FacialHair::Mutton => {
            let uc = unit_circle(ctx)?.clone();
            for s in [-1.0_f32, 1.0] {
                out.push((uc.clone(), DrawParam::default()
                    .dest(Vec2::new(fa.x + s * w * 0.24, fa.y + h * 0.06))
                    .scale(Vec2::new(w * 0.06, h * 0.11))
                    .color(brown)));
            }
        }
        FacialHair::FuManchu => {
            // FuManchu uses unit_line + draw_thick_line. Pre-compute the two line meshes as
            // scaled/rotated unit-lines, stored as (unit_line_clone, DrawParam).
            let line = unit_line(ctx)?.clone();
            for s in [-1.0_f32, 1.0] {
                let a = Vec2::new(mo.x + s * w * 0.12, mo.y);
                let b = Vec2::new(mo.x + s * w * 0.16, mo.y + h * 0.24);
                let d = b - a;
                let len = d.length().max(0.0001);
                let ang = d.y.atan2(d.x);
                out.push((line.clone(), DrawParam::default()
                    .dest(a)
                    .rotation(ang)
                    .scale(Vec2::new(len, w * 0.03))
                    .color(brown)));
            }
        }
    }

    // ---- Accessories ------------------------------------------------------------------
    match skin.accessory {
        Accessory::None => {}
        Accessory::StarBadge => {
            let star = star_mesh(ctx, w * 0.11, col(0xFF, 0xD7, 0x00))?;
            // star_mesh builds at origin; dest is the body-local offset from c.
            out.push((star, DrawParam::default().dest(Vec2::new(sh.x - w * 0.14, sh.y))));
        }
        Accessory::Monocle => {
            let ring = Mesh::new_circle(
                ctx, DrawMode::stroke(w * 0.02), [0.0, 0.0], w * 0.09, 0.5, Color::WHITE,
            )?;
            out.push((ring, DrawParam::default().dest(Vec2::new(fa.x + w * 0.13, fa.y - h * 0.02))));
        }
        Accessory::BowTie => {
            let white = Color::WHITE;
            // neck offset = (0, h*0.02) from c
            let nx = 0.0_f32;
            let ny = h * 0.02;
            let left = Mesh::new_polygon(ctx, DrawMode::fill(), &[
                [nx,              ny],
                [nx - w * 0.12,   ny - h * 0.06],
                [nx - w * 0.12,   ny + h * 0.06],
            ], white)?;
            out.push((left, DrawParam::default()));
            let right = Mesh::new_polygon(ctx, DrawMode::fill(), &[
                [nx,              ny],
                [nx + w * 0.12,   ny - h * 0.06],
                [nx + w * 0.12,   ny + h * 0.06],
            ], white)?;
            out.push((right, DrawParam::default()));
            out.push(rect_mesh(ctx, Rect::new(nx - w * 0.02, ny - h * 0.03, w * 0.04, h * 0.06), col(0x22, 0x22, 0x22))?);
        }
        Accessory::NeonChain => {
            let uc = unit_circle(ctx)?.clone();
            let gold = col(0xFF, 0xD7, 0x00);
            let n = 9;
            for i in 0..n {
                let t = i as f32 / (n as f32 - 1.0);
                let ang = std::f32::consts::PI * (0.15 + 0.70 * t);
                // sh = (0, h*0.10) in body-local coords
                let px = sh.x + ang.cos() * w * 0.26;
                let py = sh.y + h * 0.02 + ang.sin() * h * 0.16;
                out.push((uc.clone(), DrawParam::default()
                    .dest(Vec2::new(px, py))
                    .scale(Vec2::splat(w * 0.03))
                    .color(gold)));
            }
        }
        Accessory::Shades => {
            let dark = col(0x15, 0x15, 0x1A);
            // fa = (0, -h*0.08)
            for s in [-1.0_f32, 1.0] {
                out.push(rect_mesh(ctx, Rect::new(
                    fa.x + s * w * 0.13 - w * 0.09,
                    fa.y - h * 0.05,
                    w * 0.18,
                    h * 0.10,
                ), dark)?);
            }
            out.push(rect_mesh(ctx, Rect::new(fa.x - w * 0.05, fa.y - h * 0.02, w * 0.10, h * 0.02), dark)?);
        }
        Accessory::LassoLoop => {
            let tan = col(0xC8, 0xA4, 0x6E);
            // loop centre offset from c: (w*0.30, h*0.14)
            let lo = Vec2::new(w * 0.30, h * 0.14);
            let ring = Mesh::new_circle(ctx, DrawMode::stroke(w * 0.03), [0.0, 0.0], w * 0.11, 0.4, tan)?;
            out.push((ring, DrawParam::default().dest(lo)));
            let inner = Mesh::new_circle(ctx, DrawMode::stroke(w * 0.02), [0.0, 0.0], w * 0.06, 0.4, tan)?;
            out.push((inner, DrawParam::default().dest(lo)));
        }
        Accessory::GoldTooth => {
            // mo = (0, h*0.06)
            out.push(rect_mesh(ctx, Rect::new(mo.x - w * 0.02, mo.y - h * 0.01, w * 0.04, h * 0.05), col(0xFF, 0xD7, 0x00))?);
        }
    }

    Ok(out)
}

/// A filled 5-point star mesh of the given outer radius, centred on the origin.
fn star_mesh(ctx: &mut Context, r: f32, color: Color) -> ggez::GameResult<Mesh> {
    let mut pts = Vec::with_capacity(10);
    for i in 0..10 {
        let rad = if i % 2 == 0 { r } else { r * 0.42 };
        let ang = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
        pts.push([ang.cos() * rad, ang.sin() * rad]);
    }
    Mesh::new_polygon(ctx, DrawMode::fill(), &pts, color)
}
