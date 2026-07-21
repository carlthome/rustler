use ggez::glam::Vec2;
use ggez::graphics::{Canvas, Color, DrawMode, DrawParam, Mesh, Rect, Text};
use ggez::{Context, GameResult};
use std::cell::RefCell;

use crate::enemies::{BossCharge, CrabType, EnemyCrab};
use crate::graphics::{
    draw_crab, draw_rustler, flush_crab_bodies, flush_crab_legs, unit_circle, unit_square,
};
use crate::hud_cache::{
    CAREER_LABEL_CACHE, LOADOUT_PAGE_CACHE, MENU_BUTTONS_CACHE, MENU_SUBTITLE_CACHE,
    MENU_TITLE_CACHE, MENU_TITLE_CHARS_CACHE,
};
use crate::skins::PlayerSkin;
use crate::state::MainState;

pub fn draw_menu(
    state: &MainState,
    ctx: &mut Context,
    canvas: &mut Canvas,
    width: f32,
    height: f32,
) -> GameResult {
    let t = state.menu_time;

    // --- Moonlit-beach gradient backdrop ------------------------------------------------
    let strips = 28;
    let top = Color::from_rgb(9, 12, 34);
    let mid = Color::from_rgb(48, 26, 66);
    let sand = Color::from_rgb(74, 58, 78);
    let lerp = |a: Color, b: Color, k: f32| {
        Color::new(
            a.r + (b.r - a.r) * k,
            a.g + (b.g - a.g) * k,
            a.b + (b.b - a.b) * k,
            1.0,
        )
    };
    let strip_h = height / strips as f32;
    let strip_square = unit_square(ctx)?;
    for i in 0..strips {
        let k = i as f32 / (strips - 1) as f32;
        let c = if k < 0.65 {
            lerp(top, mid, k / 0.65)
        } else {
            lerp(mid, sand, (k - 0.65) / 0.35)
        };
        canvas.draw(
            strip_square,
            DrawParam::default()
                .dest(Vec2::new(0.0, i as f32 * strip_h))
                .scale(Vec2::new(width, strip_h + 1.0))
                .color(c),
        );
    }

    let dot = unit_circle(ctx)?;

    // --- Twinkling stars ----------------------------------------------------------------
    let hash = |n: u32| {
        let mut x = n.wrapping_mul(2654435761);
        x ^= x >> 15;
        x = x.wrapping_mul(2246822519);
        x ^= x >> 13;
        x
    };
    for i in 0..70u32 {
        let sx = (hash(i) % 1000) as f32 / 1000.0 * width;
        let sy = (hash(i * 7 + 1) % 1000) as f32 / 1000.0 * height * 0.6;
        let phase = (hash(i * 13 + 3) % 628) as f32 / 100.0;
        let speed = 1.2 + (hash(i * 17 + 5) % 200) as f32 / 100.0;
        let twinkle = 0.25 + 0.75 * (t * speed + phase).sin().abs();
        let r = 0.7 + (hash(i * 19 + 7) % 100) as f32 / 100.0 * 1.6;
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(Vec2::new(sx, sy))
                .scale(Vec2::splat(r))
                .color(Color::new(1.0, 1.0, 0.92, twinkle)),
        );
    }

    // --- Soft moon with a glowing halo --------------------------------------------------
    let moon_pos = Vec2::new(width * 0.82, height * 0.2);
    for ring in (0..6).rev() {
        let rr = 34.0 + ring as f32 * 16.0;
        let a = 0.05 + (5 - ring) as f32 * 0.03;
        canvas.draw(
            dot,
            DrawParam::default()
                .dest(moon_pos)
                .scale(Vec2::splat(rr))
                .color(Color::new(0.95, 0.93, 0.8, a)),
        );
    }
    canvas.draw(
        dot,
        DrawParam::default()
            .dest(moon_pos)
            .scale(Vec2::splat(30.0))
            .color(Color::new(0.98, 0.96, 0.86, 1.0)),
    );

    // --- Grass ground at the bottom so crabs have a surface to walk on ------------------
    let grass_start = height - 66.0;
    let grass_h = 66.0;
    let grass_square = unit_square(ctx)?;
    // Darker grass base layer
    canvas.draw(
        grass_square,
        DrawParam::default()
            .dest(Vec2::new(0.0, grass_start))
            .scale(Vec2::new(width, grass_h))
            .color(Color::from_rgb(42, 68, 38)),
    );
    // Grass texture: wavy highlight stripes for depth
    let grass_stripes = 6;
    for i in 0..grass_stripes {
        let stripe_h = grass_h / grass_stripes as f32;
        let y = grass_start + i as f32 * stripe_h;
        let alpha = 0.15 - (i as f32 / grass_stripes as f32) * 0.12;
        canvas.draw(
            grass_square,
            DrawParam::default()
                .dest(Vec2::new(0.0, y))
                .scale(Vec2::new(width, stripe_h + 2.0))
                .color(Color::new(0.6, 0.8, 0.4, alpha)),
        );
    }

    // --- Two rival King Crab conga trains marching across the sand -----------------------
    // Front train (bottom row): King Crab leads right with a retinue — moody, authoritative.
    // Back train (slightly higher): rival leads left, smaller and scrappier.
    // This previews the actual ecology: competing conga leaders, not a generic herd.
    let make_crab = |crab_type: CrabType, x: f32, y: f32, speed: f32, scale: f32, idx: usize| EnemyCrab {
        pos: Vec2::new(x, y),
        vel: Vec2::new(speed, 0.0),
        speed: speed.abs(),
        caught: true,
        chain_index: Some(idx),
        scale,
        spawn_time: 10.0,
        crab_type,
        chain_color: None,
        spooked_timer: 0.0,
        beat_phase_offset: idx as f32 * 0.4,
        join_pulse: 0.0,
        fleeing: false,
        facing_angle: if speed < 0.0 { std::f32::consts::PI } else { 0.0 },
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

    // Only a handful of decorative crabs on the menu, so render them at full detail (reset the LOD
    // hint in case a prior gameplay pass left a high crowd count set).
    crate::graphics::set_crab_lod_hint(0);

    // Train A: marches right at the bottom, led by a large King Crab
    {
        let speed = 72.0_f32;
        let spacing = 68.0_f32;
        let y_base = height - 60.0;
        // King Crab leader (Boss type, 1.8× scale)
        let train_a_types = [
            (CrabType::Boss, 1.8_f32),
            (CrabType::Dancer, 0.7_f32),
            (CrabType::Normal, 0.6_f32),
            (CrabType::Fast, 0.55_f32),
            (CrabType::Golden, 0.6_f32),
            (CrabType::Sneaky, 0.55_f32),
        ];
        let span = width + spacing * train_a_types.len() as f32;
        for (i, &(ctype, scale)) in train_a_types.iter().enumerate() {
            let x = ((t * speed + i as f32 * spacing) % span) - spacing;
            let bob = (t * 5.0 + i as f32 * 0.8).sin() * if i == 0 { 7.0 } else { 4.0 };
            let deco = make_crab(ctype, x, y_base, speed, scale, i);
            let beat_phase = (t * 4.0 + i as f32 * 0.5).sin().abs();
            draw_crab(ctx, canvas, &deco, Vec2::new(x, y_base - bob), beat_phase, 0.0, bob.max(0.0), 0.0, t)?;
        }
    }

    // Train B: marches left, slightly higher — scrappier rival with fewer followers
    {
        let speed = -55.0_f32; // negative = moving left
        let spacing = 60.0_f32;
        let y_base = height - 80.0;
        let train_b_types = [
            (CrabType::Boss, 1.4_f32),
            (CrabType::Armored, 0.65_f32),
            (CrabType::Magnet, 0.6_f32),
            (CrabType::Big, 0.65_f32),
        ];
        let span = width + spacing * train_b_types.len() as f32;
        for (i, &(ctype, scale)) in train_b_types.iter().enumerate() {
            // Travel right-to-left: start off right edge, wrap around
            let x = width - ((t * speed.abs() + i as f32 * spacing) % span) + spacing;
            let bob = (t * 4.5 + i as f32 * 1.1).sin() * if i == 0 { 5.0 } else { 3.0 };
            let deco = make_crab(ctype, x, y_base, speed, scale, i);
            let beat_phase = (t * 3.8 + i as f32 * 0.6).sin().abs();
            draw_crab(ctx, canvas, &deco, Vec2::new(x, y_base - bob), beat_phase, 0.0, bob.max(0.0), 0.0, t)?;
        }
    }

    flush_crab_legs(ctx, canvas)?;
    flush_crab_bodies(ctx, canvas)?;

    // --- Title: "Crab Rustler" with an animated colour wave -----------------------------
    let (main_title_width, main_title_height) =
        MENU_TITLE_CACHE.with(|c| -> GameResult<(f32, f32)> {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                let mut main_title = Text::new("Crab Rustler");
                main_title.set_scale(112.0);
                let dims = main_title.measure(ctx)?;
                *cache = Some((main_title, dims.x, dims.y));
            }
            let (_, w, h) = cache.as_ref().unwrap();
            Ok((*w, *h))
        })?;
    let title_top = height * 0.13;

    // Drop shadow.
    MENU_TITLE_CACHE.with(|c| {
        let cache = c.borrow();
        let (main_title, _, _) = cache.as_ref().unwrap();
        canvas.draw(
            main_title,
            DrawParam::default()
                .dest(Vec2::new(
                    (width - main_title_width) / 2.0 + 8.0,
                    title_top + 8.0,
                ))
                .color(Color::from_rgba(0, 0, 0, 180))
                .rotation(0.03),
        );
    });

    MENU_TITLE_CHARS_CACHE.with(|c| {
        let mut cache = c.borrow_mut();
        if cache.is_none() {
            let chars: Vec<Text> = "Crab Rustler"
                .chars()
                .map(|ch| Text::new(ggez::graphics::TextFragment::new(ch).scale(112.0)))
                .collect();
            *cache = Some(chars);
        }
        for (i, ch_text) in cache.as_ref().unwrap().iter().enumerate() {
            let x = (width - main_title_width) / 2.0 + i as f32 * 60.0;
            let y = title_top + (t * 2.2 + i as f32 * 0.5).sin() * 14.0;
            let hue = t * 0.6 + i as f32 * 0.55;
            let color = Color::from_rgb(
                (200.0 + hue.sin() * 55.0) as u8,
                (120.0 + (hue + 2.0).sin() * 110.0) as u8,
                (200.0 + (hue + 4.0).sin() * 55.0) as u8,
            );
            canvas.draw(
                ch_text,
                DrawParam::default()
                    .dest(Vec2::new(x, y))
                    .color(color)
                    .rotation((t * 1.5 + i as f32 * 0.4).sin() * 0.07),
            );
        }
    });

    let subtitle_width = MENU_SUBTITLE_CACHE.with(|c| -> GameResult<f32> {
        let mut cache = c.borrow_mut();
        let needs_rebuild = !matches!(&*cache, Some((s, _, _)) if s == &state.subtitle);
        if needs_rebuild {
            let mut subtitle = Text::new(&state.subtitle);
            subtitle.set_scale(22.0);
            let w = subtitle.measure(ctx)?.x;
            *cache = Some((state.subtitle.clone(), subtitle, w));
        }
        Ok(cache.as_ref().unwrap().2)
    })?;
    MENU_SUBTITLE_CACHE.with(|c| {
        let cache = c.borrow();
        let (_, subtitle, _) = cache.as_ref().unwrap();
        canvas.draw(
            subtitle,
            DrawParam::default()
                .dest(Vec2::new(
                    (width - subtitle_width) / 2.0,
                    title_top + main_title_height + 14.0,
                ))
                .color(Color::from_rgb(255, 235, 190)),
        );
    });

    if state.menu_page == 0 {
        let skin = state.player_skin;
        let preview_top = title_top + main_title_height + 36.0;
        let preview_w = 458.0;
        let preview_h = 88.0;
        let preview_x = (width - preview_w) / 2.0;

        // The preview panel's size never changes (fixed constants above), only its screen
        // position does — so the rounded-rect mesh is built once at the origin and every frame
        // just offsets it via DrawParam, instead of re-uploading a fresh GPU buffer for a panel
        // that sits on screen for as long as the player looks at the Home page.
        thread_local! {
            static PREVIEW_PANEL_MESH: RefCell<Option<Mesh>> = const { RefCell::new(None) };
        }
        PREVIEW_PANEL_MESH.with(|c| -> GameResult {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                *cache = Some(Mesh::new_rounded_rectangle(
                    ctx,
                    DrawMode::fill(),
                    Rect::new(0.0, 0.0, preview_w, preview_h),
                    16.0,
                    Color::from_rgba(10, 14, 30, 120),
                )?);
            }
            canvas.draw(
                cache.as_ref().unwrap(),
                DrawParam::default().dest(Vec2::new(preview_x, preview_top)),
            );
            Ok(())
        })?;

        thread_local! {
            static PREVIEW_LABEL_CACHE: RefCell<Option<Text>> = const { RefCell::new(None) };
        }
        PREVIEW_LABEL_CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                let mut t = Text::new("CURRENT CRAB");
                t.set_scale(15.0);
                *cache = Some(t);
            }
            canvas.draw(
                cache.as_ref().unwrap(),
                DrawParam::default()
                    .dest(Vec2::new(preview_x + 20.0, preview_top + 10.0))
                    .color(Color::from_rgb(140, 220, 210)),
            );
        });

        thread_local! {
            static PREVIEW_TAGLINE_CACHE: RefCell<Option<(PlayerSkin, Text)>> = const { RefCell::new(None) };
        }
        let tagline_y = preview_top + 34.0;
        PREVIEW_TAGLINE_CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            let needs_rebuild = !matches!(&*cache, Some((s, _)) if *s == skin);
            if needs_rebuild {
                let mut t = Text::new(skin.tagline());
                t.set_scale(19.0);
                *cache = Some((skin, t));
            }
            canvas.draw(
                &cache.as_ref().unwrap().1,
                DrawParam::default()
                    .dest(Vec2::new(preview_x + 108.0, tagline_y))
                    .color(Color::from_rgb(255, 235, 190)),
            );
        });

        draw_rustler(
            ctx,
            canvas,
            Vec2::new(preview_x + 26.0, preview_top + 16.0),
            &state.textures.player,
            Vec2::ZERO,
            0.2 + 0.2 * (t * 3.0).sin().abs(),
            t,
            false,
            skin,
        )?;

        thread_local! {
            static PREVIEW_NAME_CACHE: RefCell<Option<(String, Text)>> = const { RefCell::new(None) };
        }
        PREVIEW_NAME_CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            let needs_rebuild = !matches!(&*cache, Some((n, _)) if n == &state.player_name);
            if needs_rebuild {
                let mut t = Text::new(state.player_name.as_str());
                t.set_scale(18.0);
                *cache = Some((state.player_name.clone(), t));
            }
            canvas.draw(
                &cache.as_ref().unwrap().1,
                DrawParam::default()
                    .dest(Vec2::new(preview_x + 108.0, preview_top + 56.0))
                    .color(Color::from_rgb(240, 224, 180)),
            );
        });
    }

    // --- Home page: traditional centered menu buttons ----------------------------------
    if state.menu_page == 0 {
        const BUTTON_LABELS: [&str; 5] = ["PLAY", "CAMPAIGN", "LOADOUT", "HOW TO PLAY", "QUIT"];
        let btn_w = 320.0_f32;
        let btn_h = 54.0_f32;
        let btn_gap = 18.0_f32;
        let total_h = BUTTON_LABELS.len() as f32 * (btn_h + btn_gap) - btn_gap;
        let btn_start_y = height * 0.42;
        let btn_x = (width - btn_w) / 2.0;
        let pulse = 0.55 + 0.45 * (t * 3.0).sin().abs();

        // Build button text cache once.
        MENU_BUTTONS_CACHE.with(|c| -> GameResult {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                let mut buttons = Vec::new();
                for label in BUTTON_LABELS.iter() {
                    let mut txt = Text::new(*label);
                    txt.set_scale(30.0);
                    let w = txt.measure(ctx)?.x;
                    buttons.push((txt, w));
                }
                *cache = Some(buttons);
            }
            Ok(())
        })?;

        // Button size/radius/colors are all fixed constants — only the "which index is
        // selected" state varies — so the 4 possible rounded-rect meshes (bg/border ×
        // selected/unselected) are built once at the local origin and reused every frame via
        // a DrawParam offset, instead of re-uploading up to 10 fresh GPU buffers a frame while
        // the Home page just sits on screen.
        thread_local! {
            static BUTTON_MESH_CACHE: RefCell<Option<(Mesh, Mesh, Mesh, Mesh)>> = const { RefCell::new(None) };
        }
        BUTTON_MESH_CACHE.with(|c| -> GameResult {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                let rect = Rect::new(0.0, 0.0, btn_w, btn_h);
                let bg_selected = Mesh::new_rounded_rectangle(
                    ctx,
                    DrawMode::fill(),
                    rect,
                    10.0,
                    Color::from_rgba(60, 180, 160, 200),
                )?;
                let bg_unselected = Mesh::new_rounded_rectangle(
                    ctx,
                    DrawMode::fill(),
                    rect,
                    10.0,
                    Color::from_rgba(10, 20, 40, 160),
                )?;
                let border_selected = Mesh::new_rounded_rectangle(
                    ctx,
                    DrawMode::stroke(2.0),
                    rect,
                    10.0,
                    Color::from_rgba(120, 255, 230, 220),
                )?;
                let border_unselected = Mesh::new_rounded_rectangle(
                    ctx,
                    DrawMode::stroke(2.0),
                    rect,
                    10.0,
                    Color::from_rgba(80, 100, 130, 120),
                )?;
                *cache = Some((bg_selected, bg_unselected, border_selected, border_unselected));
            }
            let (bg_selected, bg_unselected, border_selected, border_unselected) =
                cache.as_ref().unwrap();
            MENU_BUTTONS_CACHE.with(|bc| {
                let bcache = bc.borrow();
                let buttons = bcache.as_ref().unwrap();
                for (i, (txt, tw)) in buttons.iter().enumerate() {
                    let by = btn_start_y + i as f32 * (btn_h + btn_gap);
                    let selected = state.menu_selection == i;
                    let dest = Vec2::new(btn_x, by);
                    canvas.draw(
                        if selected { bg_selected } else { bg_unselected },
                        DrawParam::default().dest(dest),
                    );
                    canvas.draw(
                        if selected { border_selected } else { border_unselected },
                        DrawParam::default().dest(dest),
                    );
                    let label_x = btn_x + (btn_w - tw) / 2.0;
                    let label_y = by + (btn_h - 30.0) / 2.0;
                    let text_color = if selected {
                        Color::new(1.0, 1.0, 0.9, pulse.max(0.85))
                    } else {
                        Color::from_rgba(200, 210, 230, 200)
                    };
                    canvas.draw(
                        txt,
                        DrawParam::default()
                            .dest(Vec2::new(label_x, label_y))
                            .color(text_color),
                    );
                }
            });
            Ok(())
        })?;

        // Navigation hint below the buttons — constant string, shaped once.
        let hint_y = btn_start_y + total_h + 28.0;
        thread_local! {
            static HINT_TEXT_CACHE: RefCell<Option<(Text, f32)>> = const { RefCell::new(None) };
        }
        let hw = HINT_TEXT_CACHE.with(|c| -> GameResult<f32> {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                let mut t = Text::new("\u{25B2}/\u{25BC} navigate    Space/Enter select");
                t.set_scale(16.0);
                let w = t.measure(ctx)?.x;
                *cache = Some((t, w));
            }
            Ok(cache.as_ref().unwrap().1)
        })?;
        HINT_TEXT_CACHE.with(|c| {
            let cache = c.borrow();
            canvas.draw(
                &cache.as_ref().unwrap().0,
                DrawParam::default()
                    .dest(Vec2::new((width - hw) / 2.0, hint_y))
                    .color(Color::from_rgba(160, 170, 200, 130)),
            );
        });

        // Career stats — only shown once there is a career to show.
        if state.career_runs > 0 {
            let career_y = hint_y + 32.0;
            let cw = CAREER_LABEL_CACHE.with(|c| -> GameResult<f32> {
                let mut cache = c.borrow_mut();
                let needs_rebuild = match cache.as_ref() {
                    Some((best, total, runs, _, _)) => {
                        *best != state.career_best_score
                            || *total != state.career_total_score
                            || *runs != state.career_runs
                    }
                    None => true,
                };
                if needs_rebuild {
                    let mut career = Text::new(format!(
                        "Career best {}   \u{00B7}   {} crabs over {} runs",
                        state.career_best_score, state.career_total_score, state.career_runs
                    ));
                    career.set_scale(20.0);
                    let cw = career.measure(ctx)?.x;
                    *cache = Some((
                        state.career_best_score,
                        state.career_total_score,
                        state.career_runs,
                        career,
                        cw,
                    ));
                }
                Ok(cache.as_ref().unwrap().4)
            })?;
            CAREER_LABEL_CACHE.with(|c| {
                let cache = c.borrow();
                let (_, _, _, career, _) = cache.as_ref().unwrap();
                canvas.draw(
                    career,
                    DrawParam::default()
                        .dest(Vec2::new((width - cw) / 2.0, career_y))
                        .color(Color::from_rgb(180, 170, 210)),
                );
            });
        }
    } // end menu_page == 0 (Home)

    // --- Loadout page: skin picker + perk shop -----------------------------------------
    if state.menu_page == 1 {
        let skin = state.player_skin;
        let parade_top = height - 66.0 - 40.0;
        let picker_y = (height * 0.38).min(parade_top - 200.0);
        let col_gap = (width * 0.20).min(300.0);
        let cols_center = width * 0.62;
        let col_x = [cols_center - col_gap, cols_center, cols_center + col_gap];
        let labels = ["HAT", "FACIAL HAIR", "ACCESSORY"];
        let names = [
            skin.hat.name(),
            skin.facial_hair.name(),
            skin.accessory.name(),
        ];
        let flavours = [
            skin.hat.flavour(),
            skin.facial_hair.flavour(),
            skin.accessory.flavour(),
        ];

        let panel_w = col_gap * 2.0 + 300.0;
        let panel_rect = Rect::new(cols_center - panel_w / 2.0, picker_y - 6.0, panel_w, 122.0);

        let cache_key = (
            skin.hat,
            skin.facial_hair,
            skin.accessory,
            state.skin_slot,
            width.to_bits(),
            height.to_bits(),
        );
        LOADOUT_PAGE_CACHE.with(|cell| -> GameResult {
            let mut slot = cell.borrow_mut();
            if slot.as_ref().map_or(true, |(k, _, _, _)| *k != cache_key) {
                let picker_panel = Mesh::new_rounded_rectangle(
                    ctx,
                    DrawMode::fill(),
                    panel_rect,
                    14.0,
                    Color::from_rgba(10, 14, 30, 150),
                )?;
                let mut tagline = Text::new(skin.tagline());
                tagline.set_scale(15.0);
                let build_col = |i: usize| -> GameResult<(Text, f32, Text, f32, Text, f32)> {
                    let focused = state.skin_slot == i;
                    let mut lbl = Text::new(labels[i]);
                    lbl.set_scale(17.0);
                    let lw = lbl.measure(ctx)?.x;
                    let name_str = if focused {
                        format!("\u{25C4} {} \u{25BA}", names[i])
                    } else {
                        names[i].to_string()
                    };
                    let mut nm = Text::new(name_str);
                    nm.set_scale(22.0);
                    let nw = nm.measure(ctx)?.x;
                    let mut fl = Text::new(flavours[i]);
                    fl.set_scale(13.0);
                    let fw = fl.measure(ctx)?.x;
                    Ok((lbl, lw, nm, nw, fl, fw))
                };
                let cols: [(Text, f32, Text, f32, Text, f32); 3] =
                    [build_col(0)?, build_col(1)?, build_col(2)?];
                *slot = Some((cache_key, picker_panel, tagline, cols));
            }
            let (_, panel_mesh, tagline, cols) = slot.as_ref().unwrap();
            canvas.draw(panel_mesh, DrawParam::default());
            canvas.draw(
                tagline,
                DrawParam::default()
                    .dest(Vec2::new(panel_rect.x + 108.0, picker_y + 4.0))
                    .color(Color::from_rgb(255, 220, 140)),
            );
            for (i, (lbl, lw, nm, nw, fl, fw)) in cols.iter().enumerate() {
                let focused = state.skin_slot == i;
                let label_color = if focused {
                    Color::from_rgb(120, 255, 220)
                } else {
                    Color::from_rgb(150, 150, 175)
                };
                let name_color = if focused {
                    Color::new(1.0, 1.0, 0.6, 0.85 + 0.15 * (t * 4.0).sin().abs())
                } else {
                    Color::from_rgb(220, 220, 235)
                };
                let fl_alpha = if focused { 0.95 } else { 0.5 };
                canvas.draw(
                    lbl,
                    DrawParam::default()
                        .dest(Vec2::new(col_x[i] - lw / 2.0, picker_y + 2.0))
                        .color(label_color),
                );
                canvas.draw(
                    nm,
                    DrawParam::default()
                        .dest(Vec2::new(col_x[i] - nw / 2.0, picker_y + 26.0))
                        .color(name_color),
                );
                canvas.draw(
                    fl,
                    DrawParam::default()
                        .dest(Vec2::new(col_x[i] - fw / 2.0, picker_y + 56.0))
                        .color(Color::new(0.85, 0.85, 0.95, fl_alpha)),
                );
            }
            Ok(())
        })?;

        // Live crab preview — bob/beat are per-frame animated, drawn after the cached panel.
        let preview_center = Vec2::new(panel_rect.x + 70.0, picker_y + 46.0);
        let bob = (t * 3.0).sin() * 3.0;
        draw_rustler(
            ctx,
            canvas,
            preview_center - Vec2::new(15.0, 15.0) + Vec2::new(0.0, bob),
            &state.textures.player,
            Vec2::ZERO,
            0.4 + 0.4 * (t * 3.0).sin().abs(),
            t,
            false,
            skin,
        )?;

        let name_field_top = picker_y - 74.0;
        let name_field_rect = Rect::new(cols_center - 176.0, name_field_top, 352.0, 48.0);
        let name_field = Mesh::new_rounded_rectangle(
            ctx,
            DrawMode::fill(),
            name_field_rect,
            12.0,
            Color::from_rgba(10, 14, 30, 155),
        )?;
        canvas.draw(&name_field, DrawParam::default());
        let outline = Mesh::new_rounded_rectangle(
            ctx,
            DrawMode::stroke(2.0),
            name_field_rect,
            12.0,
            Color::from_rgba(120, 255, 220, 170),
        )?;
        canvas.draw(&outline, DrawParam::default());

        let mut name_label = Text::new("CRAB NAME");
        name_label.set_scale(15.0);
        canvas.draw(
            &name_label,
            DrawParam::default()
                .dest(Vec2::new(name_field_rect.x + 18.0, name_field_top + 8.0))
                .color(Color::from_rgb(140, 220, 210)),
        );

        let mut name_value_text = Text::new(format!("{}_", state.player_name));
        name_value_text.set_scale(20.0);
        canvas.draw(
            &name_value_text,
            DrawParam::default()
                .dest(Vec2::new(name_field_rect.x + 18.0, name_field_top + 20.0))
                .color(Color::from_rgb(255, 235, 190)),
        );

        let mut name_hint = Text::new("Type to rename    Backspace to erase");
        name_hint.set_scale(14.0);
        canvas.draw(
            &name_hint,
            DrawParam::default()
                .dest(Vec2::new(name_field_rect.x + 18.0, name_field_top + 52.0))
                .color(Color::from_rgba(180, 180, 200, 120)),
        );
    } // end menu_page == 1 (Loadout)

    Ok(())
}
