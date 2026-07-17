// TEMP stubs — will be replaced by graphics.rs agent's real implementations.
// At merge time: remove this file, remove `mod matchup_fx;` from main.rs,
// and switch the imports to `use crate::graphics::{draw_beam_hermit_match, ...}`.
use ggez::glam::Vec2;
use ggez::graphics::Canvas;
use ggez::{Context, GameResult};

pub fn draw_beam_hermit_match(
    _ctx: &mut Context,
    _canvas: &mut Canvas,
    _hits: &[(Vec2, f32)],
) -> GameResult {
    Ok(())
}

pub fn draw_stomp_dancer_match(
    _ctx: &mut Context,
    _canvas: &mut Canvas,
    _hits: &[Vec2],
) -> GameResult {
    Ok(())
}

pub fn draw_lasso_thief_match(
    _ctx: &mut Context,
    _canvas: &mut Canvas,
    _hits: &[Vec2],
) -> GameResult {
    Ok(())
}
