/// Seconds from launch when the logo's sparkle chime sounds.
pub(crate) const PLING_AT: f32 = 1.0;
/// Seconds from launch when the main menu begins scrolling onto the screen.
pub(crate) const MENU_REVEAL_AT: f32 = 4.7;
/// Seconds from launch when the startup cinematic hands over to the normal menu.
pub(crate) const INTRO_END: f32 = 6.8;
const LOGO_FADE_IN_START: f32 = 0.4;
const LOGO_FADE_IN_END: f32 = 1.4;
const LOGO_FADE_OUT_START: f32 = 3.4;
const LOGO_FADE_OUT_END: f32 = 4.4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MenuIntroPresentation {
    pub(crate) logo_alpha: f32,
    pub(crate) menu_progress: f32,
    pub(crate) moon_rise: f32,
    pub(crate) moon_bloom: f32,
    pub(crate) menu_flash: f32,
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
    let moon_rise = menu_progress;
    let moon_bloom = smoothstep(
        (time - (MENU_REVEAL_AT + 0.18)) / (INTRO_END - (MENU_REVEAL_AT + 0.18)),
    );
    let menu_flash = (1.0 - (time - MENU_REVEAL_AT) / 0.16).clamp(0.0, 1.0);
    MenuIntroPresentation {
        logo_alpha,
        menu_progress,
        moon_rise,
        moon_bloom,
        menu_flash,
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
        assert_eq!(presentation(1.4).logo_alpha, 1.0);
        assert_eq!(presentation(4.4).logo_alpha, 0.0);
        assert_eq!(presentation(INTRO_END).menu_progress, 1.0);
        assert_eq!(presentation(INTRO_END).moon_rise, 1.0);
        assert_eq!(presentation(INTRO_END).moon_bloom, 1.0);
    }

    #[test]
    fn moon_rises_and_blooms_after_the_menu_flash() {
        let reveal = presentation(MENU_REVEAL_AT);
        assert_eq!(reveal.moon_rise, 0.0);
        assert_eq!(reveal.moon_bloom, 0.0);
        assert_eq!(reveal.menu_flash, 1.0);

        let rising = presentation(MENU_REVEAL_AT + 0.1);
        assert!(rising.moon_rise > 0.0);
        assert_eq!(rising.moon_bloom, 0.0);
        assert!(rising.menu_flash > 0.0);

        let settling = presentation(MENU_REVEAL_AT + 0.2);
        assert!(settling.moon_rise > 0.0);
        assert!(settling.moon_bloom > 0.0);
        assert_eq!(settling.menu_flash, 0.0);
    }

    #[test]
    fn logo_and_menu_reveal_each_have_time_to_read() {
        assert!(LOGO_FADE_IN_END - LOGO_FADE_IN_START >= 1.0);
        assert!(LOGO_FADE_OUT_END - LOGO_FADE_OUT_START >= 1.0);
        assert!(INTRO_END - MENU_REVEAL_AT >= 2.0);
        assert!(LOGO_FADE_IN_END <= LOGO_FADE_OUT_START);
        assert!(LOGO_FADE_OUT_END <= MENU_REVEAL_AT);
    }
}
