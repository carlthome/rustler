use crate::enemies::CrabType;
use crate::levels::WinCondition;
use ggez::glam::Vec2;
use ggez::graphics::{Mesh, Text};
use std::{cell::RefCell, collections::HashMap};

thread_local! {
    pub static LEVEL_LABEL_CACHE: RefCell<HashMap<usize, (Text, f32, f32)>> = RefCell::new(HashMap::new());

    /// Single-slot label cache for the endless arcade stage counter — a HashMap keyed by stage
    /// (like LEVEL_LABEL_CACHE, which is fine for campaign's small fixed level count) would grow
    /// one Text entry per stage forever, since arcade progression never wraps or revisits a stage.
    /// A single slot holds exactly the current stage's shaped Text and gets overwritten on advance.
    pub static ARCADE_STAGE_LABEL_CACHE: RefCell<Option<(usize, Text, f32, f32)>> = RefCell::new(None);

    pub static FRENZY_BANNER_CACHE: RefCell<Option<(Text, Vec2)>> = RefCell::new(None);
    pub static STAGE_BANNER_CACHE: RefCell<Option<(&'static str, Text, Vec2)>> = RefCell::new(None);

    pub static HUD_TEXT_CACHE: RefCell<Option<(usize, usize, usize, usize, u32, Text)>> = RefCell::new(None);

    pub static RHYTHM_BONUS_CACHE: RefCell<Option<(usize, Text)>> = RefCell::new(None);

    /// Cache for the campaign goal progress line — keyed by the raw counters that feed
    /// `progress_text` (bucketed to the same whole-second resolution it's displayed at) instead
    /// of the rendered string itself, so formatting the line only happens on an actual rebuild
    /// rather than every frame just to build a comparison key (mirrors PERF_OVERLAY_CACHE below).
    #[allow(clippy::type_complexity)]
    pub static CAMPAIGN_GOAL_CACHE: RefCell<Option<((bool, WinCondition, usize, usize, usize, i32), Text)>> =
        RefCell::new(None);

    #[cfg(debug_assertions)]
    pub static PERF_OVERLAY_CACHE: RefCell<Option<(i32, i32, i32, Text, f32)>> = RefCell::new(None);

    pub static DEBUG_TEXT_CACHE: RefCell<Option<(&'static str, i32, Text)>> = RefCell::new(None);

    pub static DASH_LABEL_CACHE: RefCell<Option<Text>> = RefCell::new(None);
    pub static SPRINT_LABEL_CACHE: RefCell<Option<Text>> = RefCell::new(None);
    pub static WHISTLE_LABEL_CACHE: RefCell<Option<(bool, Text)>> = RefCell::new(None);
    pub static STOMP_LABEL_CACHE: RefCell<Option<(bool, Text)>> = RefCell::new(None);
    pub static FLASHLIGHT_LABEL_CACHE: RefCell<Option<(u8, Text)>> = RefCell::new(None);

    pub static GROOVE_LABEL_CACHE: RefCell<Option<(bool, Text, f32)>> = RefCell::new(None);

    pub static GAMBLE_BADGE_CACHE: RefCell<Option<(u32, Text, f32)>> = RefCell::new(None);

    pub static ON_BEAT_TEXT_CACHE: RefCell<Option<(Text, f32)>> = RefCell::new(None);

    pub static MENU_PANEL_CACHE: RefCell<Option<(u32, u32, Mesh)>> = RefCell::new(None);

    pub static MENU_TITLE_CACHE: RefCell<Option<(Text, f32, f32)>> = RefCell::new(None);
    pub static MENU_TITLE_CHARS_CACHE: RefCell<Option<Vec<Text>>> = RefCell::new(None);
    pub static MENU_SUBTITLE_CACHE: RefCell<Option<(String, Text, f32)>> = RefCell::new(None);
    pub static MENU_INSTRUCTIONS_CACHE: RefCell<Option<(Text, f32, f32)>> = RefCell::new(None);
    pub static MENU_PROMPT_CACHE: RefCell<Option<(Text, f32)>> = RefCell::new(None);

    pub static MENU_TUTORIAL_CACHE: RefCell<Option<(Text, f32)>> = RefCell::new(None);

    /// Startup-cinematic labels ("CARLTHOME", "P R E S E N T S", "SPACE TO SKIP") — fully static
    /// strings drawn every frame for the ~2.85s logo/reveal sequence at launch. Build and measure
    /// once, then reuse for the rest of the intro instead of re-shaping glyphs every frame (same
    /// pattern as MENU_TITLE_CACHE and friends).
    pub static STARTUP_LOGO_TEXT_CACHE: RefCell<Option<(Text, f32)>> = RefCell::new(None);
    pub static STARTUP_PRESENTS_TEXT_CACHE: RefCell<Option<(Text, f32)>> = RefCell::new(None);
    pub static STARTUP_SKIP_TEXT_CACHE: RefCell<Option<(Text, f32)>> = RefCell::new(None);

    // Cache for the Home-page menu button label texts: Vec of (Text, measured_width) per button.
    pub static MENU_BUTTONS_CACHE: RefCell<Option<Vec<(Text, f32)>>> = RefCell::new(None);

    pub static CAREER_LABEL_CACHE: RefCell<Option<(usize, usize, usize, Text, f32)>> = RefCell::new(None);

    #[allow(clippy::type_complexity)]
    pub static SHOP_CACHE: RefCell<Option<((usize, u32, u32, u32, u32), Text, f32, Text, f32)>> =
        RefCell::new(None);

    pub static BANK_NOW_PROMPT_CACHE: RefCell<Option<(Text, f32)>> = RefCell::new(None);

    /// Cache for the NPC King Crab name plates — keyed by name → (shaped_Text, measured_width).
    /// A HashMap (not a single slot) because several rival trains, each with a distinct name, are
    /// drawn every frame; a single slot would evict-and-reshape on every rival every frame. Each
    /// unique name is glyph-shaped once and reused for the rest of the session (mirrors LEVEL_LABEL_CACHE).
    pub static NPC_NAME_CACHE: RefCell<HashMap<String, (Text, f32)>> = RefCell::new(HashMap::new());

    /// Cache for the player crab name plate — same shape as the NPC name plate.
    pub static PLAYER_NAME_CACHE: RefCell<Option<(String, Text, f32)>> = RefCell::new(None);

    /// Static label caches for the minimap and day/weather HUD — these strings never change
    /// mid-frame, so building them once and reusing avoids per-frame glyph-shaping passes.
    pub static MINIMAP_LABEL_CACHE: RefCell<Option<Text>> = RefCell::new(None);
    pub static WEATHER_PHASE_CACHE: RefCell<Option<(&'static str, Text)>> = RefCell::new(None);
    pub static WEATHER_STATE_CACHE: RefCell<Option<(bool, Text)>> = RefCell::new(None);

    pub static CHAIN_SORT_BUF: RefCell<Vec<(usize, Vec2, Option<[f32; 3]>)>> = RefCell::new(Vec::new());

    pub static CHAIN_TYPE_BUF: RefCell<Vec<(usize, usize, CrabType, [f32; 3])>> = RefCell::new(Vec::new());

    pub static CHAIN_ORDER_CACHE: RefCell<Option<(usize, Vec<(usize, Option<[f32; 3]>)>)>> = RefCell::new(None);

    #[allow(clippy::type_complexity)]
    pub static LEVEL_TITLE_OVERLAY_CACHE: RefCell<Option<(String, &'static str, Text, Mesh, Mesh, Text, f32, f32, f32, f32, f32, Option<(Text, f32)>)>> = RefCell::new(None);

    #[allow(clippy::type_complexity)]
    pub static UPGRADE_SCREEN_CACHE: RefCell<Option<(
        ([usize; 3], u32, u32, u32, u32),
        Text, f32,
        Text, f32,
        [(Text, f32, Text, f32, Text, f32, Text, f32, Text, f32); 3],
    )>> = RefCell::new(None);

    #[allow(clippy::type_complexity)]
    pub static TUTORIAL_OVERLAY_CACHE: RefCell<Option<(
        &'static str,
        u32, u32,
        Mesh,
        Text, f32, f32,
        Text, f32,
        Text, f32,
        Text, f32, f32,
        u32,
        Text, f32,
    )>> = RefCell::new(None);

    #[allow(clippy::type_complexity)]
    pub static GAME_OVER_CACHE: RefCell<Option<(
        (usize, u32, u32, usize, usize, usize, bool),
        Mesh,
        Text,
        Option<(Text, f32)>,
    )>> = RefCell::new(None);

    #[allow(clippy::type_complexity)]
    pub static LOADOUT_PAGE_CACHE: RefCell<Option<(
        (crate::skins::Hat, crate::skins::FacialHair, crate::skins::Accessory, usize, u32, u32),
        Mesh,
        Text,
        [(Text, f32, Text, f32, Text, f32); 3],
    )>> = RefCell::new(None);
}
