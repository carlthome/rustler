/// Seconds from launch when the logo's sparkle chime sounds.
pub(crate) const PLING_AT: f32 = 0.62;
/// Seconds from launch when the main menu begins scrolling onto the screen.
pub(crate) const MENU_REVEAL_AT: f32 = 2.85;
/// Seconds from launch when the startup cinematic hands over to the normal menu.
pub(crate) const INTRO_END: f32 = 4.25;
const LOGO_FADE_IN_START: f32 = 0.45;
const LOGO_FADE_IN_END: f32 = 0.9;
const LOGO_FADE_OUT_START: f32 = 2.15;
const LOGO_FADE_OUT_END: f32 = 2.65;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MenuIntroPresentation {
    pub(crate) logo_alpha: f32,
    pub(crate) menu_progress: f32,
}

pub(crate) fn presentation(time: f32) -> MenuIntroPresentation {
    let logo_alpha = if time < LOGO_FADE_IN_START {
        0.0
    } else if time < LOGO_FADE_IN_END {
        smoothstep(
            (time - LOGO_FADE_IN_START) / (LOGO_FADE_IN_END - LOGO_FADE_IN_START),
        )
    } else if time < LOGO_FADE_OUT_START {
        1.0
    } else if time < LOGO_FADE_OUT_END {
        1.0 - smoothstep(
            (time - LOGO_FADE_OUT_START) / (LOGO_FADE_OUT_END - LOGO_FADE_OUT_START),
        )
    } else {
        0.0
    };
    let menu_progress = smoothstep((time - MENU_REVEAL_AT) / (INTRO_END - MENU_REVEAL_AT));
    MenuIntroPresentation {
        logo_alpha,
        menu_progress,
    }
}

fn smoothstep(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intro_moves_from_logo_to_fully_revealed_menu() {
        assert_eq!(presentation(0.0).logo_alpha, 0.0);
        assert_eq!(presentation(0.0).menu_progress, 0.0);
        assert_eq!(presentation(1.0).logo_alpha, 1.0);
        assert_eq!(presentation(2.7).logo_alpha, 0.0);
        assert_eq!(presentation(INTRO_END).menu_progress, 1.0);
    }
}
