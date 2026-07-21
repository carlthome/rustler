pub(crate) const PLING_AT: f32 = 0.62;
pub(crate) const MENU_REVEAL_AT: f32 = 2.85;
pub(crate) const INTRO_END: f32 = 4.25;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MenuIntroPresentation {
    pub(crate) logo_alpha: f32,
    pub(crate) menu_progress: f32,
}

pub(crate) fn presentation(time: f32) -> MenuIntroPresentation {
    let logo_alpha = if time < 0.45 {
        0.0
    } else if time < 0.9 {
        smoothstep((time - 0.45) / 0.45)
    } else if time < 2.15 {
        1.0
    } else if time < 2.65 {
        1.0 - smoothstep((time - 2.15) / 0.5)
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
