use crate::enemies::EnemyCrab;
use crate::{CRAB_SIZE, Flashlight, PLAYER_SIZE};
use crevice::std140::AsStd140;
use ggez::Context;
use ggez::glam::Vec2;
use ggez::graphics::{
    BlendMode, Canvas, Color, DrawMode, DrawParam, Image, Mesh, Rect, Shader,
    ShaderParamsBuilder,
};

#[derive(Copy, Clone, Debug, AsStd140)]
pub struct ResolutionUniform {
    pub width: f32,
    pub height: f32,
    pub time: f32,
}

#[derive(Copy, Clone, Debug, AsStd140)]
pub struct FlashlightUniform {
    pub center_x: f32,
    pub center_y: f32,
    pub angle: f32,
    pub spread: f32,
    pub range: f32,
    pub time: f32,
    pub time_since_catch: f32,
    pub laser_level: f32,
    pub screen_width: f32,
    pub screen_height: f32,
}

pub fn draw_grass(
    ctx: &mut Context,
    canvas: &mut Canvas,
    width: f32,
    height: f32,
    texture: &Image,
    shader: &Shader,
    time: f32,
) -> ggez::GameResult {
    let blend_mode = canvas.blend_mode();
    let solid_bg = Mesh::new_rectangle(
        ctx,
        DrawMode::fill(),
        Rect::new(0.0, 0.0, width, height),
        Color::from_rgb(0, 100, 0),
    )?;
    canvas.draw(&solid_bg, DrawParam::default());

    // Draw a full-screen quad using the grass shader.
    let params = ShaderParamsBuilder::new(&ResolutionUniform {
        width,
        height,
        time,
    })
    .build(ctx);
    canvas.set_shader_params(&params);
    canvas.set_shader(shader);
    let quad = Mesh::new_rectangle(
        ctx,
        DrawMode::fill(),
        Rect::new(-width / 2.0, -height / 2.0, width, height),
        Color::RED,
    )?;
    canvas.draw(&quad, DrawParam::default());
    canvas.set_default_shader();
    canvas.set_blend_mode(BlendMode::MULTIPLY);

    // Repeat a tiled grass texture across the screen.
    let tile_w = texture.width() as f32;
    let tile_h = texture.height() as f32;
    let tiles_x = (width / tile_w).ceil() as i32;
    let tiles_y = (height / tile_h).ceil() as i32;
    for y in 0..tiles_y {
        for x in 0..tiles_x {
            let dest = [x as f32 * tile_w, y as f32 * tile_h];
            canvas.draw(texture, DrawParam::default().dest(dest));
        }
    }
    canvas.set_blend_mode(blend_mode);
    Ok(())
}

pub fn draw_rustler(ctx: &mut Context, canvas: &mut Canvas, pos: Vec2) -> ggez::GameResult {
    // Head
    let head = Mesh::new_circle(
        ctx,
        DrawMode::fill(),
        [PLAYER_SIZE / 2.0, PLAYER_SIZE / 3.0],
        PLAYER_SIZE / 4.0,
        0.5,
        Color::from_rgb(160, 82, 45),
    )?;
    canvas.draw(&head, DrawParam::default().dest(pos));

    // Body
    let body = Mesh::new_rectangle(
        ctx,
        DrawMode::fill(),
        Rect::new(
            PLAYER_SIZE / 2.5,
            PLAYER_SIZE / 2.0,
            PLAYER_SIZE / 5.0,
            PLAYER_SIZE / 2.0,
        ),
        Color::from_rgb(139, 69, 19),
    )?;
    canvas.draw(&body, DrawParam::default().dest(pos));

    // Hat brim
    let hat_brim = Mesh::new_rectangle(
        ctx,
        DrawMode::fill(),
        Rect::new(
            PLAYER_SIZE / 2.0 - PLAYER_SIZE / 4.0,
            PLAYER_SIZE / 4.5,
            PLAYER_SIZE / 2.0,
            PLAYER_SIZE / 10.0,
        ),
        Color::from_rgb(80, 40, 20),
    )?;
    canvas.draw(&hat_brim, DrawParam::default().dest(pos));

    // Hat top
    let hat_top = Mesh::new_rectangle(
        ctx,
        DrawMode::fill(),
        Rect::new(
            PLAYER_SIZE / 2.0 - PLAYER_SIZE / 8.0,
            PLAYER_SIZE / 7.0,
            PLAYER_SIZE / 4.0,
            PLAYER_SIZE / 6.0,
        ),
        Color::from_rgb(80, 40, 20),
    )?;
    canvas.draw(&hat_top, DrawParam::default().dest(pos));

    Ok(())
}

pub fn draw_crab(ctx: &mut Context, canvas: &mut Canvas, crab: &EnemyCrab) -> ggez::GameResult {
    // Grow size with age
    let grow_t = (crab.spawn_time / 10.0).min(1.0);
    let size = CRAB_SIZE * (0.6 + 0.4 * grow_t) * crab.scale;

    // Color: more red as crab ages, and different color for type
    let t = (crab.spawn_time / 10.0).min(1.0);
    let (r, g, b) = match crab.crab_type {
        crate::enemies::CrabType::Normal => (
            (255.0 * (0.6 + 0.4 * t)),
            (100.0 * (1.0 - t)),
            (100.0 * (1.0 - t)),
        ),
        crate::enemies::CrabType::Fast => (255.0, 180.0 * (1.0 - t), 40.0),
        crate::enemies::CrabType::Big => (180.0, 60.0, 180.0 * (1.0 - t)),
        crate::enemies::CrabType::Sneaky => (120.0, 220.0, 220.0),
    };
    let crab_color = Color::from_rgb(r as u8, g as u8, b as u8);

    // Crab body
    let crab_body = Mesh::new_circle(
        ctx,
        DrawMode::fill(),
        [0.0, 0.0],
        size / 2.0,
        0.5,
        crab_color,
    )?;

    // Crab legs (6 lines)
    let mut leg_meshes = Vec::new();
    let leg_len = size * 0.7;
    let leg_color = Color::from_rgb(200, 50, 50);
    for i in 0..6 {
        let base_angle = std::f32::consts::PI * (0.25 + i as f32 / 6.0);
        let time = ctx.time.time_since_start().as_secs_f32();
        let phase = (crab.pos.x + crab.pos.y) * 0.05;
        let wiggle_speed = 2.0 + crab.speed * 0.08; // scale with crab speed
        let wiggle = (time * wiggle_speed + phase + i as f32).sin() * 0.18;
        let angle = base_angle + wiggle;
        let x1 = (size / 2.0) * angle.cos();
        let y1 = (size / 2.0) * angle.sin();
        let x2 = (size / 2.0 + leg_len) * angle.cos();
        let y2 = (size / 2.0 + leg_len) * angle.sin();
        let leg = Mesh::new_line(ctx, &[[x1, y1], [x2, y2]], 2.0, leg_color)?;
        leg_meshes.push(leg);
    }

    // Crab claws (small circles)
    let claw_offset = size * 0.7;
    let claw_radius = size * 0.18;
    let left_claw = Mesh::new_circle(
        ctx,
        DrawMode::fill(),
        [-(claw_offset), -(claw_offset * 0.3)],
        claw_radius,
        0.5,
        crab_color,
    )?;
    let right_claw = Mesh::new_circle(
        ctx,
        DrawMode::fill(),
        [claw_offset, -(claw_offset * 0.3)],
        claw_radius,
        0.5,
        crab_color,
    )?;

    // Draw all parts at crab.pos
    canvas.draw(&crab_body, DrawParam::default().dest(crab.pos));
    for leg in &leg_meshes {
        canvas.draw(leg, DrawParam::default().dest(crab.pos));
    }
    canvas.draw(&left_claw, DrawParam::default().dest(crab.pos));
    canvas.draw(&right_claw, DrawParam::default().dest(crab.pos));

    Ok(())
}

pub fn draw_flashlight(
    ctx: &mut Context,
    canvas: &mut Canvas,
    player_pos: Vec2,
    dir: Vec2,
    time_since_catch: f32,
    flashlight: &Flashlight,
    shader: &Shader,
    screen_width: f32,
    screen_height: f32,
) -> ggez::GameResult {
    // Flicker logic (calculations are done in the shader)
    let time = ctx.time.time_since_start().as_secs_f32();

    // Flashlight parameters
    let laser_level = flashlight.laser_level;
    let cone_angle = flashlight.cone_upgrade;
    let range = flashlight.range_upgrade;

    // Calculate flashlight properties
    let flashlight_len = range.max(80.0);
    let spread = cone_angle.max(0.15);
    let center = Vec2::new(
        player_pos.x + PLAYER_SIZE / 2.0,
        player_pos.y + PLAYER_SIZE / 2.0,
    );
    let angle = dir.y.atan2(dir.x);

    // Create uniform data for the shader
    let uniform_data = FlashlightUniform {
        center_x: center.x,
        center_y: center.y,
        angle,
        spread,
        range: flashlight_len,
        time,
        time_since_catch,
        laser_level: laser_level as f32,
        screen_width,
        screen_height,
    };

    // Set up shader parameters
    let params = ShaderParamsBuilder::new(&uniform_data).build(ctx);
    canvas.set_shader_params(&params);
    canvas.set_shader(shader);

    // Draw a full-screen quad that the shader will render the flashlight onto
    // Use the same pattern as the grass shader
    let flashlight_quad = Mesh::new_rectangle(
        ctx,
        DrawMode::fill(),
        Rect::new(-screen_width / 2.0, -screen_height / 2.0, screen_width, screen_height),
        Color::WHITE,
    )?;
    
    // Set additive blend mode for the flashlight effect
    let original_blend = canvas.blend_mode();
    canvas.set_blend_mode(BlendMode::ADD);
    
    canvas.draw(&flashlight_quad, DrawParam::default());
    
    // Restore original blend mode and shader
    canvas.set_blend_mode(original_blend);
    canvas.set_default_shader();
    Ok(())
}
