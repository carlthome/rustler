use ggez::glam::Vec2;
use ggez::graphics::{Canvas, Color, DrawMode, DrawParam, Mesh, Rect, Text};
use ggez::{Context, GameResult};

use crate::constants::{MAX_START_RANK, PERK_COST_STEP};
use crate::enemies::{BossCharge, CrabType, EnemyCrab};
use crate::graphics::{
    draw_crab, draw_rustler, flush_crab_bodies, flush_crab_legs, unit_circle, unit_square,
};
use crate::hud_cache::{
    CAREER_LABEL_CACHE, LOADOUT_PAGE_CACHE, MENU_INSTRUCTIONS_CACHE, MENU_PANEL_CACHE,
    MENU_PROMPT_CACHE, MENU_SUBTITLE_CACHE, MENU_TITLE_CACHE, MENU_TITLE_CHARS_CACHE,
    MENU_TUTORIAL_CACHE, SHOP_CACHE,
};
use crate::state::MainState;

fn perk_cost(rank: u32) -> Option<usize> {
    if rank >= MAX_START_RANK {
        None
    } else {
        Some((rank as usize + 1) * PERK_COST_STEP)
    }
}

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

    // --- A conga line of crabs marching across the sand ---------------------------------
    let march_y = height - 66.0;
    let march_speed = 70.0;
    let spacing = 74.0;
    let march_types = [
        CrabType::Normal,
        CrabType::Fast,
        CrabType::Big,
        CrabType::Sneaky,
        CrabType::Armored,
        CrabType::Dancer,
    ];
    for (i, ctype) in march_types.iter().enumerate() {
        let span = width + spacing * march_types.len() as f32;
        let x = ((t * march_speed + i as f32 * spacing) % span) - spacing;
        let bob = (t * 6.0 + i as f32 * 0.9).sin() * 5.0;
        let deco = EnemyCrab {
            pos: Vec2::new(x, march_y),
            vel: Vec2::new(march_speed, 0.0),
            speed: 60.0,
            caught: true,
            chain_index: Some(i),
            scale: 0.5,
            spawn_time: 10.0,
            crab_type: *ctype,
            spooked_timer: 0.0,
            beat_phase_offset: 0.0,
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
        };
        let beat_phase = (t * 4.0 + i as f32 * 0.5).sin().abs();
        draw_crab(
            ctx,
            canvas,
            &deco,
            Vec2::new(x, march_y - bob),
            beat_phase,
            0.0,
            bob.max(0.0),
            0.0,
            t,
        )?;
    }
    flush_crab_legs(ctx, canvas)?;
    flush_crab_bodies(ctx, canvas)?;

    // --- Title: "Crab Rustler" with an animated colour wave -----------------------------
    let (main_title_width, main_title_height) = MENU_TITLE_CACHE.with(|c| -> GameResult<(f32, f32)> {
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

    // --- Tab bar: Home / Loadout page switcher ------------------------------------------
    {
        let tab_y = title_top + main_title_height + 42.0;
        let tab_labels = ["  HOME  ", "  LOADOUT  "];
        let mut tab_x = width / 2.0 - 120.0;
        for (i, label) in tab_labels.iter().enumerate() {
            let active = state.menu_page == i;
            let color = if active {
                Color::from_rgb(80, 220, 200)
            } else {
                Color::from_rgba(180, 180, 200, 100)
            };
            let mut t_text = Text::new(*label);
            t_text.set_scale(20.0);
            let tw = t_text.measure(ctx)?.x;
            let bg_rect = Rect::new(tab_x - 4.0, tab_y - 4.0, tw + 8.0, 28.0);
            let bg_color = if active {
                Color::from_rgba(30, 80, 80, 160)
            } else {
                Color::from_rgba(10, 14, 30, 80)
            };
            let bg = Mesh::new_rounded_rectangle(ctx, DrawMode::fill(), bg_rect, 6.0, bg_color)?;
            canvas.draw(&bg, DrawParam::default());
            canvas.draw(&t_text, DrawParam::default().dest(Vec2::new(tab_x, tab_y)).color(color));
            tab_x += tw + 20.0;
        }
        let hint = if state.menu_page == 0 {
            "Tab — open Loadout"
        } else {
            "Esc — back to Home    Tab — next slot    \u{25C4}/\u{25BA} — change"
        };
        let mut hint_text = Text::new(hint);
        hint_text.set_scale(16.0);
        let hw = hint_text.measure(ctx)?.x;
        canvas.draw(
            &hint_text,
            DrawParam::default()
                .dest(Vec2::new((width - hw) / 2.0, tab_y + 32.0))
                .color(Color::from_rgba(180, 180, 200, 120)),
        );
    }

    // --- Instructions on a translucent rounded panel for readability -------------------
    let (text_width, text_height) = MENU_INSTRUCTIONS_CACHE.with(|c| -> GameResult<(f32, f32)> {
        let mut cache = c.borrow_mut();
        if cache.is_none() {
            let text = Text::new(
                "Catch all the crabs!\n\nMove: Arrow keys / WASD\nAim flashlight: Mouse\nDash: Space (dash ON the beat for a GROOVE DASH that sweeps nearby crabs into your path)\nThrow lasso: Left click\nBeat wave burst: Q\nWhistle (pulls crabs in): E\nStomp (cracks armored crabs): R\nCall on the beat (Dancers answer): F\nGroove Call on the beat (whole herd streams in over the bar): V — tap V again ON each beat to ECHO the call, extending and amplifying the herd flood\nDownbeat Slam (full Groove, on beat): G\nDrum Roll (hold T on the beat, release to fire a beam blast): T\nCycle the train on the beat (X): aim the flashlight at an interior crab to BUBBLE it one slot toward the centre and build a centerpiece; aim at nothing to rotate the whole train one slot",
            );
            let dims = text.measure(ctx)?;
            *cache = Some((text, dims.x, dims.y));
        }
        let (_, w, h) = cache.as_ref().unwrap();
        Ok((*w, *h))
    })?;
    let text_x = (width - text_width) / 2.0;
    let text_y = height * 0.44;
    let pad = 26.0;

    if state.menu_page == 0 {
        let panel_key = (width.to_bits(), height.to_bits());
        let cached_panel = MENU_PANEL_CACHE.with(|c| {
            c.borrow().as_ref().and_then(|(w, h, mesh)| {
                (*w == panel_key.0 && *h == panel_key.1).then(|| mesh.clone())
            })
        });
        let panel = match cached_panel {
            Some(mesh) => mesh,
            None => {
                let mesh = Mesh::new_rounded_rectangle(
                    ctx,
                    DrawMode::fill(),
                    Rect::new(
                        text_x - pad,
                        text_y - pad,
                        text_width + pad * 2.0,
                        text_height + pad * 2.0,
                    ),
                    14.0,
                    Color::from_rgba(10, 14, 30, 170),
                )?;
                MENU_PANEL_CACHE.with(|c| {
                    *c.borrow_mut() = Some((panel_key.0, panel_key.1, mesh.clone()))
                });
                mesh
            }
        };
        canvas.draw(&panel, DrawParam::default());
        MENU_INSTRUCTIONS_CACHE.with(|c| {
            let cache = c.borrow();
            let (text, _, _) = cache.as_ref().unwrap();
            canvas.draw(
                text,
                DrawParam::default()
                    .dest(Vec2::new(text_x, text_y))
                    .color(Color::from_rgb(255, 246, 210)),
            );
        });

        // --- Pulsing "Press Space or Enter to start" prompt --------------------------------
        let pulse = 0.55 + 0.45 * (t * 3.0).sin().abs();
        let prompt_width = MENU_PROMPT_CACHE.with(|c| -> GameResult<f32> {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                let mut prompt = Text::new("Press Space or Enter to start");
                prompt.set_scale(30.0);
                let w = prompt.measure(ctx)?.x;
                *cache = Some((prompt, w));
            }
            Ok(cache.as_ref().unwrap().1)
        })?;
        MENU_PROMPT_CACHE.with(|c| {
            let cache = c.borrow();
            let (prompt, _) = cache.as_ref().unwrap();
            canvas.draw(
                prompt,
                DrawParam::default()
                    .dest(Vec2::new(
                        (width - prompt_width) / 2.0,
                        text_y + text_height + pad * 2.0 + 22.0,
                    ))
                    .color(Color::new(1.0, 0.9, 0.25, pulse)),
            );
        });

        // --- "Press C — Campaign" hint -------------------------------------------------------
        let tut_width = MENU_TUTORIAL_CACHE.with(|c| -> GameResult<f32> {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                let mut prompt = Text::new("Press  C  — Campaign  (tutorials are the first stops)");
                prompt.set_scale(22.0);
                let w = prompt.measure(ctx)?.x;
                *cache = Some((prompt, w));
            }
            Ok(cache.as_ref().unwrap().1)
        })?;
        MENU_TUTORIAL_CACHE.with(|c| {
            let cache = c.borrow();
            let (prompt, _) = cache.as_ref().unwrap();
            canvas.draw(
                prompt,
                DrawParam::default()
                    .dest(Vec2::new(
                        (width - tut_width) / 2.0,
                        text_y + text_height + pad * 2.0 + 58.0,
                    ))
                    .color(Color::new(0.6, 0.85, 1.0, 0.55 + 0.35 * pulse)),
            );
        });

        // --- Career line ------------------------------------------------------------------
        if state.career_runs > 0 {
            let career_base = text_y + text_height + pad * 2.0 + 90.0;
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
                        "Career best {}   ·   {} crabs banked over {} runs",
                        state.career_best_score, state.career_total_score, state.career_runs
                    ));
                    career.set_scale(22.0);
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
                        .dest(Vec2::new((width - cw) / 2.0, career_base))
                        .color(Color::from_rgb(200, 190, 230)),
                );
            });

            // --- Perk shop ----------------------------------------------------------------
            let available = state.career_available();
            let ranks = (
                available,
                state.start_beam_rank,
                state.start_lasso_rank,
                state.start_whistle_rank,
                state.start_stomp_rank,
            );
            let (header_w, list_w) = SHOP_CACHE.with(|c| -> GameResult<(f32, f32)> {
                let mut cache = c.borrow_mut();
                let needs_rebuild = !matches!(cache.as_ref(), Some((k, ..)) if *k == ranks);
                if needs_rebuild {
                    let mut header = Text::new(format!(
                        "SPEND {} banked crabs on permanent gear:",
                        available
                    ));
                    header.set_scale(21.0);
                    let hw = header.measure(ctx)?.x;
                    let perk = |name: &str, key: char, rank: u32| -> String {
                        match perk_cost(rank) {
                            Some(cost) => format!("[{}] {} Lv{} → {}crabs", key, name, rank, cost),
                            None => format!("[{}] {} MAX", key, name),
                        }
                    };
                    let mut list = Text::new(format!(
                        "{}    {}    {}    {}",
                        perk("Beam", '1', state.start_beam_rank),
                        perk("Lasso", '2', state.start_lasso_rank),
                        perk("Whistle", '3', state.start_whistle_rank),
                        perk("Stomp", '4', state.start_stomp_rank),
                    ));
                    list.set_scale(19.0);
                    let lw = list.measure(ctx)?.x;
                    *cache = Some((ranks, header, hw, list, lw));
                }
                let cr = cache.as_ref().unwrap();
                Ok((cr.2, cr.4))
            })?;
            let list_color = if state.shop_flash > 0.0 {
                Color::new(0.5 + 0.5 * state.shop_flash, 1.0, 0.5, 1.0)
            } else if state.shop_denied > 0.0 {
                Color::new(1.0, 0.5 - 0.3 * state.shop_denied, 0.5 - 0.3 * state.shop_denied, 1.0)
            } else {
                Color::from_rgb(150, 220, 210)
            };
            let shop_y = career_base + 30.0;
            SHOP_CACHE.with(|c| {
                let cache = c.borrow();
                let (_, header, _, list, _) = cache.as_ref().unwrap();
                canvas.draw(
                    header,
                    DrawParam::default()
                        .dest(Vec2::new((width - header_w) / 2.0, shop_y))
                        .color(Color::from_rgb(180, 175, 205)),
                );
                canvas.draw(
                    list,
                    DrawParam::default()
                        .dest(Vec2::new((width - list_w) / 2.0, shop_y + 28.0))
                        .color(list_color),
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
        let col_x = [
            cols_center - col_gap,
            cols_center,
            cols_center + col_gap,
        ];
        let labels = ["HAT", "FACIAL HAIR", "ACCESSORY"];
        let names = [skin.hat.name(), skin.facial_hair.name(), skin.accessory.name()];
        let flavours = [skin.hat.flavour(), skin.facial_hair.flavour(), skin.accessory.flavour()];

        let panel_w = col_gap * 2.0 + 300.0;
        let panel_rect = Rect::new(
            cols_center - panel_w / 2.0,
            picker_y - 6.0,
            panel_w,
            122.0,
        );

        let cache_key = (skin.hat, skin.facial_hair, skin.accessory, state.skin_slot,
                         width.to_bits(), height.to_bits());
        LOADOUT_PAGE_CACHE.with(|cell| -> GameResult {
            let mut slot = cell.borrow_mut();
            if slot.as_ref().map_or(true, |(k, _, _, _)| *k != cache_key) {
                let picker_panel = Mesh::new_rounded_rectangle(
                    ctx, DrawMode::fill(), panel_rect, 14.0,
                    Color::from_rgba(10, 14, 30, 150),
                )?;
                let mut tagline = Text::new(skin.tagline());
                tagline.set_scale(15.0);
                let mut build_col = |i: usize| -> GameResult<(Text, f32, Text, f32, Text, f32)> {
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
    } // end menu_page == 1 (Loadout)

    Ok(())
}
