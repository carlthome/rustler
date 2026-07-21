//! Menu/world-facing HUD screens drawn outside the core per-crab render pass: the campaign
//! world-map screen (node-and-path layout with biome tints), the in-play minimap, the
//! day/night + weather indicator strip, and the bottom-centre tool roster. Extracted from
//! `graphics/mod.rs` to keep that file navigable; these lean on the shared cached meshes and
//! helpers in the parent module (reached via `use super::*`), and own the thread-local Text/
//! instance caches that only they use.

use super::*;

thread_local! {
    // Reusable instance buffer for draw_minimap's dots (crabs, NPC followers/leaders, pen, player).
    // `crabs` holds every crab caught this run (never removed, only flagged `caught`), so the old
    // per-crab canvas.draw() loop issued one GPU submission per crab per frame with unbounded
    // growth over a run's lifetime — the one entity-draw loop in this file that hadn't been
    // batched yet. All these dots share the same unit-circle mesh and only differ in
    // dest/scale/color, so one InstanceArray fill + draw_instanced_mesh handles them all, same
    // draw order (and thus same overlap blending) as the original sequential calls.
    static MINIMAP_DOT_PARAMS: RefCell<Vec<DrawParam>> = RefCell::new(Vec::new());
    static MINIMAP_DOT_INSTANCES: RefCell<Option<InstanceArray>> = RefCell::new(None);

    // Cache for draw_tool_roster's 15 labels (key/name/hint x 5 slots). Every one of those was a
    // fresh Text::new() + set_scale() call every single frame the roster was visible — i.e. all of
    // active gameplay, the same per-frame glyph-shaping cost COMBO_LABEL_CACHE above already fixed
    // for the combo meter. 14 of the 15 strings are truly static per slot; only the GROOVE slot's
    // hint toggles between "SLAM ready!" and "need groove" as the meter fills, so each cache entry
    // stores the source &'static str alongside its shaped Text and rebuilds only on a content
    // mismatch — a rare event for 14 of 15 slots, and just a two-way flip for the 15th.
    static TOOL_ROSTER_TEXT_CACHE: RefCell<[Option<(&'static str, Text)>; 15]> =
        RefCell::new([const { None }; 15]);

    // Cache for the world-map screen's Text objects. draw_world_map rebuilt a fresh Text +
    // measure() for every node label, the title, and the controls hint on every frame the map
    // screen was visible — the same unbounded-idle-time pattern every other menu screen already
    // fixed. Node labels: keyed per-node by (completed, unlocked); those are the only two booleans
    // that change label text (suffix " ✓" and lock " [locked]"). Selection changes fill color only,
    // never the label text, so it's not part of the key. Title and hint are static literals →
    // cached unconditionally. A path-line segment cache is skipped: there are only N-1 ≤ 3 path
    // segments and they're connection-only (two endpoint positions, no text/glyphs), so the per-
    // frame cost of `Mesh::new_line` for three lines is negligible compared to glyph-shaping.
    static WORLD_MAP_NODE_LABELS: RefCell<Vec<Option<((bool, bool), Text, f32)>>> = RefCell::new(Vec::new());
    static WORLD_MAP_TITLE_CACHE: RefCell<Option<(Text, f32)>> = RefCell::new(None);
    static WORLD_MAP_HINT_CACHE: RefCell<Option<(Text, f32)>> = RefCell::new(None);
    static WORLD_MAP_SKIP_CACHE: RefCell<Option<(Text, f32)>> = RefCell::new(None);
    // Per-node biome tint for the world map, built once (the node list is stable for the session).
    // Campaign nodes take their level's `biome.tint`; tutorial nodes get a warm amber on-ramp colour.
    // Cached so we never rebuild the (String-allocating) `get_levels()` list per frame.
    static WORLD_MAP_NODE_TINTS: RefCell<Option<Vec<Color>>> = RefCell::new(None);
}

/// Campaign world map screen. Draws a simple node-and-path layout over a dark backdrop.
/// Nodes are colored by state: locked=dim gray, unlocked=white, completed=teal, selected=gold ring.
/// Call this instead of the game/title draw when `show_world_map` is true.
pub fn draw_world_map(
    ctx: &mut Context,
    canvas: &mut Canvas,
    map: &crate::world_map::WorldMap,
    width: f32,
    height: f32,
    menu_time: f32,
) -> ggez::GameResult {
    // Dark sea background — unit square scaled to screen size, same pattern as the beat pulse in
    // draw_game. Avoids a fresh Mesh::new_rectangle GPU buffer upload on every frame.
    let sq = unit_square(ctx)?;
    canvas.draw(
        sq,
        DrawParam::default()
            .scale(Vec2::new(width, height))
            .color(Color::new(0.04, 0.07, 0.12, 1.0)),
    );

    let (sx, sy) = (width, height);

    let node_to_screen = |(nx, ny): (f32, f32)| -> Vec2 {
        Vec2::new(nx * sx, 0.25 * sy + ny * sy * 0.5)
    };

    // Biome tint per node — built once (see the cache note above). Cloned out cheaply (≤ a handful
    // of Copy `Color`s) so the draw loops below can read tints without holding the RefCell borrow.
    let node_tints: Vec<Color> = WORLD_MAP_NODE_TINTS.with(|c| {
        let mut cache = c.borrow_mut();
        if cache.is_none() {
            use crate::world_map::NodeKind;
            let levels = crate::levels::get_levels();
            let tints = map
                .nodes
                .iter()
                .map(|n| match &n.kind {
                    // Tutorials are the welcoming on-ramp — a warm amber, distinct from any biome.
                    NodeKind::Tutorial(_) => Color::new(0.90, 0.70, 0.35, 1.0),
                    NodeKind::Level(i) => {
                        let (r, g, b) = levels
                            .get(*i)
                            .map(|l| l.biome.tint)
                            .unwrap_or((200, 200, 200));
                        Color::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
                    }
                })
                .collect::<Vec<_>>();
            *cache = Some(tints);
        }
        cache.clone().unwrap()
    });

    // Connecting path lines between consecutive nodes. Gradient each unlocked leg between the two
    // nodes' biome tints so the path itself telegraphs the tonal shift (warm beach → cold water).
    // A few short segments per leg is plenty on this static menu screen (no gameplay frame budget).
    for i in 0..map.nodes.len().saturating_sub(1) {
        let a = node_to_screen(map.nodes[i].position);
        let b = node_to_screen(map.nodes[i + 1].position);
        if map.nodes[i + 1].unlocked {
            let (ca, cb) = (node_tints[i], node_tints[i + 1]);
            const SEGS: usize = 6;
            for s in 0..SEGS {
                let t0 = s as f32 / SEGS as f32;
                let t1 = (s + 1) as f32 / SEGS as f32;
                let tm = (t0 + t1) * 0.5;
                let col = Color::new(
                    ca.r + (cb.r - ca.r) * tm,
                    ca.g + (cb.g - ca.g) * tm,
                    ca.b + (cb.b - ca.b) * tm,
                    0.6,
                );
                canvas.draw(
                    &Mesh::new_line(ctx, &[a.lerp(b, t0), a.lerp(b, t1)], 3.0, col)?,
                    DrawParam::default(),
                );
            }
        } else {
            // Locked leg stays a dim, colourless thread — the colour "unlocks" with the node.
            canvas.draw(
                &Mesh::new_line(ctx, &[a, b], 3.0, Color::new(0.3, 0.3, 0.3, 0.4))?,
                DrawParam::default(),
            );
        }
    }

    // Reuse UNIT_CIRCLE (built once, scaled via DrawParam) instead of a fresh Mesh::new_circle
    // every frame. Same technique used for all other fill-circle draws in this file.
    let circle = unit_circle(ctx)?;

    for (i, node) in map.nodes.iter().enumerate() {
        let pos = node_to_screen(node.position);
        let is_selected = i == map.selected;

        // Selection ring — gentle pulse. Scale and alpha are per-frame (menu_time-driven), so they
        // stay as DrawParam; only the mesh itself is reused.
        if is_selected {
            let pulse = (menu_time * 3.0).sin() * 0.15 + 0.85;
            canvas.draw(
                circle,
                DrawParam::default()
                    .dest(pos)
                    .scale(Vec2::splat(28.0 * pulse))
                    .color(Color::new(1.0, 0.85, 0.2, 0.35)),
            );
        }

        // Biome glow — a soft radial halo behind each unlocked node in its zone's colour, so the map
        // reads as an illustrated world at a glance. Locked nodes get none (their colour is hidden
        // until earned). Drawn before the fill so the solid dot sits on top of its halo.
        let tint = node_tints[i];
        if node.unlocked {
            canvas.draw(
                circle,
                DrawParam::default()
                    .dest(pos)
                    .scale(Vec2::splat(27.0))
                    .color(Color::new(tint.r, tint.g, tint.b, 0.18)),
            );
        }

        // Node fill — biome-tinted so each zone reads by colour. Computed as a DrawParam (not baked
        // into a mesh) so a single cached unit circle covers all states. Locked → desaturated to a
        // dim grey (colour "unlocks" as a reward); completed → full tint, slightly brightened;
        // selected → tint brightened (the gold selection ring above still marks the cursor).
        let fill_color = if !node.unlocked {
            let g = (tint.r + tint.g + tint.b) / 3.0 * 0.4 + 0.12;
            Color::new(g, g, g, 1.0)
        } else if node.completed {
            Color::new(
                (tint.r * 1.1).min(1.0),
                (tint.g * 1.1).min(1.0),
                (tint.b * 1.1).min(1.0),
                1.0,
            )
        } else if is_selected {
            Color::new(
                (tint.r * 1.25 + 0.1).min(1.0),
                (tint.g * 1.25 + 0.1).min(1.0),
                (tint.b * 1.25 + 0.1).min(1.0),
                1.0,
            )
        } else {
            tint
        };
        canvas.draw(
            circle,
            DrawParam::default()
                .dest(pos)
                .scale(Vec2::splat(18.0))
                .color(fill_color),
        );

        // Node label — cached per-node by (completed, unlocked). Selection changes fill color
        // only, never the label text (suffix " ✓" derives from completed, lock " [locked]" from
        // unlocked), so it's not part of the cache key. Same rebuild-on-change pattern as
        // WHISTLE_LABEL_CACHE, STOMP_LABEL_CACHE, and all the other HUD caches in main.rs.
        let label_color = if node.unlocked {
            Color::WHITE
        } else {
            Color::new(0.4, 0.4, 0.4, 1.0)
        };
        let label_key = (node.completed, node.unlocked);
        WORLD_MAP_NODE_LABELS.with(|c| -> ggez::GameResult {
            let mut labels = c.borrow_mut();
            // Grow the Vec to cover this node index if needed (the map never shrinks mid-session).
            if labels.len() <= i {
                labels.resize_with(i + 1, || None);
            }
            let entry = &mut labels[i];
            // Rebuild only when the node's (completed, unlocked) state actually changes.
            if entry.as_ref().map(|(k, _, _)| *k) != Some(label_key) {
                let suffix = if node.completed { " ✓" } else { "" };
                let lock = if !node.unlocked { " [locked]" } else { "" };
                let mut label = Text::new(format!("{}{}{}", node.name, suffix, lock));
                label.set_scale(16.0);
                let w = label.measure(ctx)?.x;
                *entry = Some((label_key, label, w));
            }
            if let Some((_, label, w)) = entry.as_ref() {
                canvas.draw(
                    label,
                    DrawParam::default()
                        .dest(Vec2::new(pos.x - w * 0.5, pos.y + 24.0))
                        .color(label_color),
                );
            }
            Ok(())
        })?;
    }

    // Title — static literal, built once and reused forever. Same pattern as MENU_PROMPT_CACHE.
    WORLD_MAP_TITLE_CACHE.with(|c| -> ggez::GameResult {
        let mut cache = c.borrow_mut();
        if cache.is_none() {
            let mut title = Text::new("— World Map —");
            title.set_scale(28.0);
            let w = title.measure(ctx)?.x;
            *cache = Some((title, w));
        }
        if let Some((title, w)) = cache.as_ref() {
            canvas.draw(
                title,
                DrawParam::default()
                    .dest(Vec2::new((sx - w) * 0.5, sy * 0.08))
                    .color(Color::new(0.8, 0.9, 1.0, 1.0)),
            );
        }
        Ok(())
    })?;

    // Controls hint — static literal, built once and reused forever.
    WORLD_MAP_HINT_CACHE.with(|c| -> ggez::GameResult {
        let mut cache = c.borrow_mut();
        if cache.is_none() {
            let mut hint = Text::new("Left / Right: Navigate     Space / Enter: Play     Esc: Back");
            hint.set_scale(15.0);
            let w = hint.measure(ctx)?.x;
            *cache = Some((hint, w));
        }
        if let Some((hint, w)) = cache.as_ref() {
            canvas.draw(
                hint,
                DrawParam::default()
                    .dest(Vec2::new((sx - w) * 0.5, sy * 0.88))
                    .color(Color::new(0.55, 0.65, 0.75, 1.0)),
            );
        }
        Ok(())
    })?;

    // Soft "skip ahead" warning — shown while a skip confirm is armed (locked node, one Confirm
    // pressed). A brief, non-judgmental inline line just above the controls hint; press Confirm
    // again to go, or move/Esc to back out. Alpha eases in so it doesn't pop.
    if map.skip_warn_timer > 0.0 {
        WORLD_MAP_SKIP_CACHE.with(|c| -> ggez::GameResult {
            let mut cache = c.borrow_mut();
            if cache.is_none() {
                let mut warn =
                    Text::new("Skipping ahead — earlier nodes will be marked complete. Confirm again to go.");
                warn.set_scale(16.0);
                let w = warn.measure(ctx)?.x;
                *cache = Some((warn, w));
            }
            if let Some((warn, w)) = cache.as_ref() {
                // Fade in over the first ~0.25s of the 2s window, then hold full.
                let a = ((2.0 - map.skip_warn_timer) * 4.0).min(1.0);
                canvas.draw(
                    warn,
                    DrawParam::default()
                        .dest(Vec2::new((sx - w) * 0.5, sy * 0.82))
                        .color(Color::new(1.0, 0.8, 0.35, a)),
                );
            }
            Ok(())
        })?;
    }

    Ok(())
}

/// Minimap in the top-right corner showing the full 2× world: player, pen, NPC trains, and crabs.
pub fn draw_minimap(
    ctx: &mut Context,
    canvas: &mut Canvas,
    viewport_w: f32,
    viewport_h: f32,
    world_w: f32,
    world_h: f32,
    camera_origin: Vec2,
    player_pos: Vec2,
    pen_pos: Vec2,
    crabs: &[EnemyCrab],
    npc_leaders: &[(Vec2, f32)],
    npc_followers: &[Vec2],
    time: f32,
) -> ggez::GameResult {
    let map_w = 180.0_f32;
    let map_h = map_w * (world_h / world_w);
    let map_x = viewport_w - map_w - 10.0;
    let map_y = 10.0;
    let sp = |pos: Vec2| Vec2::new(map_x + (pos.x / world_w) * map_w, map_y + (pos.y / world_h) * map_h);
    let dot = unit_circle(ctx)?;
    let sq = unit_square(ctx)?;
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(map_x - 2.0, map_y - 2.0)).scale(Vec2::new(map_w + 4.0, map_h + 4.0)).color(Color::from_rgba(0, 0, 0, 150)));
    MINIMAP_DOT_PARAMS.with(|params_cell| -> ggez::GameResult {
        let mut params = params_cell.borrow_mut();
        params.clear();
        for crab in crabs.iter().filter(|c| !c.caught && !c.is_boss()) {
            let [r, g, b] = crab.crab_color();
            params.push(DrawParam::default().dest(sp(crab.pos)).scale(Vec2::splat(2.5)).color(Color::new(r, g, b, 0.45)));
        }
        for crab in crabs.iter().filter(|c| c.caught) {
            let [r, g, b] = crab.crab_color();
            params.push(DrawParam::default().dest(sp(crab.pos)).scale(Vec2::splat(3.0)).color(Color::new(r, g, b, 0.85)));
        }
        for &pos in npc_followers {
            params.push(DrawParam::default().dest(sp(pos)).scale(Vec2::splat(2.0)).color(Color::new(0.96, 0.72, 0.16, 0.6)));
        }
        for &(pos, ls) in npc_leaders {
            let pulse = 0.6 + 0.4 * (time * 3.0).sin().abs();
            params.push(DrawParam::default().dest(sp(pos)).scale(Vec2::splat((3.0 + (ls - 1.2) * 2.0) * pulse)).color(Color::new(0.96, 0.72, 0.16, 0.9)));
        }
        params.push(DrawParam::default().dest(sp(pen_pos)).scale(Vec2::splat(4.0)).color(Color::new(0.3, 1.0, 0.4, 0.85)));
        params.push(DrawParam::default().dest(sp(player_pos)).scale(Vec2::splat(5.0)).color(Color::WHITE));

        MINIMAP_DOT_INSTANCES.with(|inst_cell| -> ggez::GameResult {
            let mut inst_slot = inst_cell.borrow_mut();
            let instances = inst_slot.get_or_insert_with(|| InstanceArray::new(ctx, None));
            instances.set(params.iter().copied());
            canvas.draw_instanced_mesh_guarded(dot.clone(), instances, DrawParam::default());
            Ok(())
        })
    })?;
    let vx = map_x + (camera_origin.x / world_w) * map_w;
    let vy = map_y + (camera_origin.y / world_h) * map_h;
    let vw = (viewport_w / world_w) * map_w;
    let vh = (viewport_h / world_h) * map_h;
    let vc = Color::new(1.0, 1.0, 1.0, 0.45);
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(vx, vy)).scale(Vec2::new(vw, 1.0)).color(vc));
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(vx, vy + vh)).scale(Vec2::new(vw, 1.0)).color(vc));
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(vx, vy)).scale(Vec2::new(1.0, vh)).color(vc));
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(vx + vw, vy)).scale(Vec2::new(1.0, vh)).color(vc));
    let bc = Color::from_rgba(200, 200, 200, 80);
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(map_x - 1.0, map_y - 1.0)).scale(Vec2::new(map_w + 2.0, 1.0)).color(bc));
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(map_x - 1.0, map_y + map_h)).scale(Vec2::new(map_w + 2.0, 1.0)).color(bc));
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(map_x - 1.0, map_y)).scale(Vec2::new(1.0, map_h)).color(bc));
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(map_x + map_w, map_y)).scale(Vec2::new(1.0, map_h)).color(bc));
    crate::hud_cache::MINIMAP_LABEL_CACHE.with(|c| {
        let mut slot = c.borrow_mut();
        if slot.is_none() {
            let mut t = Text::new("MAP");
            t.set_scale(11.0);
            *slot = Some(t);
        }
        canvas.draw(slot.as_ref().unwrap(), DrawParam::default().dest(Vec2::new(map_x, map_y - 13.0)).color(Color::from_rgba(200, 200, 200, 110)));
    });
    Ok(())
}

/// Day/night cycle progress bar and weather indicator — sits just below the minimap.
pub fn draw_day_weather_hud(
    ctx: &mut Context,
    canvas: &mut Canvas,
    viewport_w: f32,
    map_h: f32,
    day_phase_t: f32,
    weather_intensity: f32,
    time: f32,
) -> ggez::GameResult {
    let map_w = 180.0_f32;
    let x = viewport_w - map_w - 10.0;
    let y = 10.0 + map_h + 8.0;
    let dot = unit_circle(ctx)?;
    let sq = unit_square(ctx)?;
    let night = ((day_phase_t - 0.5) / 0.5).clamp(0.0, 1.0);
    let day_bright = 1.0 - night;
    canvas.draw(dot, DrawParam::default().dest(Vec2::new(x + 8.0, y + 8.0)).scale(Vec2::splat(8.0 * day_bright.max(0.2))).color(Color::new(1.0, 0.85 + 0.1 * day_bright, 0.3, day_bright.max(0.15))));
    if night > 0.1 {
        canvas.draw(dot, DrawParam::default().dest(Vec2::new(x + 8.0, y + 8.0)).scale(Vec2::splat(7.0 * night)).color(Color::new(0.88, 0.9, 1.0, night * 0.85)));
    }
    let phase_label: &'static str = match (day_phase_t * 4.0) as u32 { 0 => "DAWN", 1 => "DAY", 2 => "DUSK", _ => "NIGHT" };
    crate::hud_cache::WEATHER_PHASE_CACHE.with(|c| {
        let mut slot = c.borrow_mut();
        if slot.as_ref().map_or(true, |(cached, _)| *cached != phase_label) {
            let mut t = Text::new(phase_label); t.set_scale(10.0);
            *slot = Some((phase_label, t));
        }
        canvas.draw(&slot.as_ref().unwrap().1, DrawParam::default().dest(Vec2::new(x + 20.0, y + 2.0)).color(Color::new(0.85, 0.85, 0.95, 0.7)));
    });
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(x, y + 18.0)).scale(Vec2::new(map_w, 3.0)).color(Color::from_rgba(30, 30, 60, 180)));
    let fc = if night > 0.5 { Color::new(0.5, 0.55, 0.9, 0.8) } else if day_phase_t < 0.15 || (day_phase_t > 0.45 && day_phase_t < 0.6) { Color::new(1.0, 0.6, 0.2, 0.8) } else { Color::new(1.0, 0.92, 0.4, 0.8) };
    canvas.draw(sq, DrawParam::default().dest(Vec2::new(x, y + 18.0)).scale(Vec2::new(map_w * day_phase_t, 3.0)).color(fc));
    if weather_intensity > 0.05 {
        let da = weather_intensity * 0.8;
        for i in 0..4 {
            let dx = x + map_w - 30.0 + i as f32 * 7.0;
            let dy = y + 2.0 + ((time * 3.0 + i as f32 * 0.7).sin() * 3.0).abs();
            canvas.draw(sq, DrawParam::default().dest(Vec2::new(dx, dy)).scale(Vec2::new(2.0, 6.0)).color(Color::new(0.5, 0.7, 1.0, da)));
        }
        let is_storm = weather_intensity > 0.5;
        crate::hud_cache::WEATHER_STATE_CACHE.with(|c| {
            let mut slot = c.borrow_mut();
            if slot.as_ref().map_or(true, |(cached_storm, _)| *cached_storm != is_storm) {
                let mut t = Text::new(if is_storm { "STORM" } else { "RAIN" }); t.set_scale(10.0);
                *slot = Some((is_storm, t));
            }
            canvas.draw(&slot.as_ref().unwrap().1, DrawParam::default().dest(Vec2::new(x + map_w - 38.0, y + 12.0)).color(Color::new(0.6, 0.8, 1.0, da)));
        });
    }
    Ok(())
}

/// Compact tool roster at the bottom centre — shows each tool's key, name, matchup hint, and
/// cooldown bar so the player always knows what's ready and what each key does.
pub fn draw_tool_roster(
    ctx: &mut Context,
    canvas: &mut Canvas,
    width: f32,
    height: f32,
    // Cooldowns (0 = ready, >0 = on cooldown)
    whistle_cd: f32,
    whistle_max: f32,
    stomp_cd: f32,
    stomp_max: f32,
    boost_cd: f32,       // dash cooldown
    lasso_busy: bool,    // true when lasso is in flight/dragging
    // Groove/G state
    groove: f32,         // 0..1 groove meter level (for V/G readiness hint)
    time: f32,
    // Rhythm sync: progress toward the next beat (0 = just landed, 1 = about to land) and whether
    // the current instant is inside the on-beat cast window. A READY pad breathes with the beat
    // instead of a free-running sine — it swells as the beat approaches and flashes brightest right
    // in the on-beat window, so the roster reads as a row of drum pads telling you *when* to hit for
    // the on-beat bonus (#164 legibility; the ROADMAP "each tool key is a drum pad" vision).
    beat_progress: f32,
    on_beat: bool,
) -> ggez::GameResult {
    struct ToolSlot {
        key: &'static str,
        name: &'static str,
        hint: &'static str,
        color: [f32; 3],
        cooldown_ratio: f32,
    }

    let whistle_max_safe = if whistle_max <= 0.0 { 1.0 } else { whistle_max };
    let stomp_max_safe   = if stomp_max   <= 0.0 { 1.0 } else { stomp_max };
    let groove_clamped   = groove.clamp(0.0, 1.0);
    let groove_hint: &str = if groove_clamped >= 0.75 { "SLAM ready!" } else { "need groove" };

    let slots = [
        ToolSlot { key: "click",  name: "LASSO",   hint: "snags Thieves", color: [0.3, 0.85, 0.45], cooldown_ratio: if lasso_busy { 0.6 } else { 0.0 } },
        ToolSlot { key: "E",      name: "WHISTLE",  hint: "pulls Dancers",  color: [0.4, 0.85, 1.0],  cooldown_ratio: (whistle_cd / whistle_max_safe).clamp(0.0, 1.0) },
        ToolSlot { key: "R",      name: "STOMP",    hint: "cracks shells",  color: [0.6, 0.7, 1.0],   cooldown_ratio: (stomp_cd   / stomp_max_safe).clamp(0.0, 1.0) },
        ToolSlot { key: "Space",  name: "DASH",     hint: "on beat = +",    color: [1.0, 0.9, 0.5],   cooldown_ratio: (boost_cd   / 0.08_f32).clamp(0.0, 1.0) },
        ToolSlot { key: "V · G", name: "GROOVE",   hint: groove_hint,      color: [0.45, 1.0, 0.85], cooldown_ratio: 1.0 - groove_clamped },
    ];

    let slot_w: f32 = 88.0;
    let slot_h: f32 = 52.0;
    let slot_gap: f32 = 6.0;
    let total_w = 5.0 * slot_w + 4.0 * slot_gap;
    let x0 = (width - total_w) / 2.0;
    let y0 = height - slot_h - 10.0;

    let sq = unit_square(ctx)?;

    // Beat-synced pad glow (0..1): eases up as the next beat approaches and snaps to full inside
    // the on-beat window, so a ready pad pulses ON the beat rather than to a free-running clock.
    // This is the timing cue for on-beat tool casts — the pads light up when it pays to hit them.
    let beat_glow = if on_beat {
        1.0
    } else {
        let p = beat_progress.clamp(0.0, 1.0);
        p * p * 0.7
    };

    for (i, slot) in slots.iter().enumerate() {
        let sx = x0 + i as f32 * (slot_w + slot_gap);
        let sy = y0;
        let ready = slot.cooldown_ratio < 0.05;

        // Slot background — dark rounded rect. Cached by (position, size, color) instead of a
        // fresh Mesh::new_rounded_rectangle GPU buffer every frame — see ROUNDED_FILL_RECT_CACHE /
        // ROUNDED_STROKE_RECT_CACHE. Position/size only change on window resize and border_color
        // only ever takes one of two values (ready vs. on-cooldown), so this settles into a tiny,
        // fixed-size cache after the first couple of frames.
        let border_color = if ready {
            Color::from_rgba(
                (slot.color[0] * 180.0) as u8,
                (slot.color[1] * 180.0) as u8,
                (slot.color[2] * 180.0) as u8,
                200,
            )
        } else {
            Color::from_rgba(60, 65, 90, 160)
        };
        let bg_mesh = cached_rounded_fill_rect(
            ctx,
            sx,
            sy,
            slot_w,
            slot_h,
            5.0,
            Color::from_rgba(10, 14, 30, 180),
        )?;
        canvas.draw(&bg_mesh, DrawParam::default());
        let border_mesh = cached_rounded_stroke_rect(ctx, sx, sy, slot_w, slot_h, 5.0, 1.5, border_color)?;
        canvas.draw(&border_mesh, DrawParam::default());

        // Key label — small, top-left. Cached per slot (see TOOL_ROSTER_TEXT_CACHE) instead of a
        // fresh Text::new() + glyph-shaping pass every frame.
        TOOL_ROSTER_TEXT_CACHE.with(|cache_cell| -> ggez::GameResult {
            let mut cache = cache_cell.borrow_mut();
            let entry = &mut cache[i * 3];
            if entry.as_ref().map_or(true, |(s, _)| *s != slot.key) {
                let mut t = Text::new(slot.key);
                t.set_scale(12.0);
                *entry = Some((slot.key, t));
            }
            canvas.draw(
                &entry.as_ref().unwrap().1,
                DrawParam::default()
                    .dest(Vec2::new(sx + 4.0, sy + 3.0))
                    .color(Color::from_rgba(200, 200, 200, 180)),
            );
            Ok(())
        })?;

        // Tool name — centred, accent color, pulsing ON the beat when ready (drum-pad feel) so the
        // player reads the moment to fire for the on-beat bonus, rather than a free-running blink.
        let pulse = if ready {
            0.55 + beat_glow * 0.45
        } else {
            0.75
        };
        let [r, g, b] = slot.color;
        let name_color = Color::new(r * pulse, g * pulse, b * pulse, 1.0);
        let name_x = sx + slot_w / 2.0 - (slot.name.len() as f32 * 4.2);
        TOOL_ROSTER_TEXT_CACHE.with(|cache_cell| -> ggez::GameResult {
            let mut cache = cache_cell.borrow_mut();
            let entry = &mut cache[i * 3 + 1];
            if entry.as_ref().map_or(true, |(s, _)| *s != slot.name) {
                let mut t = Text::new(slot.name);
                t.set_scale(14.0);
                *entry = Some((slot.name, t));
            }
            canvas.draw(
                &entry.as_ref().unwrap().1,
                DrawParam::default()
                    .dest(Vec2::new(name_x.max(sx + 2.0), sy + 17.0))
                    .color(name_color),
            );
            Ok(())
        })?;

        // Hint text — tiny, dim white, below name. Only the GROOVE slot's hint ever changes value
        // at runtime (toggles between "SLAM ready!" and "need groove"); the content check above
        // re-shapes it on that flip and leaves the other four slots untouched forever.
        let hint_x = sx + slot_w / 2.0 - (slot.hint.len() as f32 * 3.2);
        TOOL_ROSTER_TEXT_CACHE.with(|cache_cell| -> ggez::GameResult {
            let mut cache = cache_cell.borrow_mut();
            let entry = &mut cache[i * 3 + 2];
            if entry.as_ref().map_or(true, |(s, _)| *s != slot.hint) {
                let mut t = Text::new(slot.hint);
                t.set_scale(11.0);
                *entry = Some((slot.hint, t));
            }
            canvas.draw(
                &entry.as_ref().unwrap().1,
                DrawParam::default()
                    .dest(Vec2::new(hint_x.max(sx + 2.0), sy + 33.0))
                    .color(Color::from_rgba(200, 200, 200, 140)),
            );
            Ok(())
        })?;

        // Cooldown / fill bar — 4px tall strip at bottom of slot, inset 4px each side
        let bar_x = sx + 4.0;
        let bar_y = sy + slot_h - 8.0;
        let bar_w = slot_w - 8.0;
        let bar_h = 4.0;

        // Background track
        canvas.draw(
            sq,
            DrawParam::default()
                .dest(Vec2::new(bar_x, bar_y))
                .scale(Vec2::new(bar_w, bar_h))
                .color(Color::from_rgba(20, 20, 40, 200)),
        );

        // Fill — groove slot shows groove level; other slots show readiness
        let fill_ratio = if slot.name == "GROOVE" {
            groove_clamped
        } else {
            1.0 - slot.cooldown_ratio
        };

        if fill_ratio > 0.0 {
            let fill_color = if slot.name == "GROOVE" {
                if groove_clamped >= 0.75 {
                    let glow = (time * 6.0).sin() * 0.5 + 0.5;
                    Color::new(1.0 * (0.7 + glow * 0.3), 0.85 * (0.7 + glow * 0.3), 0.2, 1.0)
                } else {
                    Color::new(slot.color[0], slot.color[1], slot.color[2], 0.9)
                }
            } else if ready {
                // Ready pad's fill brightens on the beat too, in lockstep with the name pulse.
                Color::new(r * (0.8 + beat_glow * 0.2), g * (0.8 + beat_glow * 0.2), b * (0.8 + beat_glow * 0.2), 1.0)
            } else {
                Color::new(r * 0.75, g * 0.75, b * 0.75, 0.85)
            };

            canvas.draw(
                sq,
                DrawParam::default()
                    .dest(Vec2::new(bar_x, bar_y))
                    .scale(Vec2::new(bar_w * fill_ratio, bar_h))
                    .color(fill_color),
            );
        }
    }

    Ok(())
}
