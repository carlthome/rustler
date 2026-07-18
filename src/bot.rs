use std::collections::HashSet;
use ggez::input::keyboard::KeyCode;
use ggez::glam::Vec2;

#[derive(Clone, Debug)]
pub enum BotAction {
    HoldKey(KeyCode),
    ReleaseKey(KeyCode),
    TapKey(KeyCode),        // hold for 1 frame then release
    MouseMove(Vec2),
    Assert(BotAssert),
    Log(&'static str),
}

#[derive(Clone, Debug)]
pub enum BotAssert {
    GameNotOver,
    ChainAtLeast(usize),
    ScoreAtLeast(usize),
    ShowWorldMap,
    TutorialActive,
    TutorialDone,           // tutorial field is None and show_world_map is true
    InGame,                 // not on menu, not game_over, not world_map
}

#[derive(Clone, Debug)]
pub struct BotEvent {
    pub at: f32,            // game-time seconds (after time_scale applied)
    pub action: BotAction,
}

pub struct BotState {
    pub script: Vec<BotEvent>,
    pub cursor: usize,
    pub time_limit: f32,
    pub keys_held: HashSet<KeyCode>,
    pub mouse_pos: Vec2,
    pub tap_release_queue: Vec<KeyCode>,   // keys to release next frame
    pub failed: Option<String>,
    pub done: bool,
}

impl BotState {
    pub fn new(script: Vec<BotEvent>, time_limit: f32) -> Self {
        Self {
            script,
            cursor: 0,
            time_limit,
            keys_held: HashSet::new(),
            mouse_pos: Vec2::ZERO,
            tap_release_queue: Vec::new(),
            failed: None,
            done: false,
        }
    }
}

pub fn script_menu_to_game() -> Vec<BotEvent> {
    // Sweep in all four directions to guarantee the flashlight covers nearby crabs regardless
    // of where they spawn. At 8× time_scale, each 1.5 s game-time segment = ~0.19 s wall-clock.
    vec![
        BotEvent { at: 0.1, action: BotAction::Log("Starting menu->game test") },
        BotEvent { at: 0.5, action: BotAction::TapKey(KeyCode::Space) },
        BotEvent { at: 2.0, action: BotAction::Assert(BotAssert::InGame) },
        BotEvent { at: 2.0,  action: BotAction::HoldKey(KeyCode::Right) },
        BotEvent { at: 3.5,  action: BotAction::ReleaseKey(KeyCode::Right) },
        BotEvent { at: 3.5,  action: BotAction::HoldKey(KeyCode::Down) },
        BotEvent { at: 5.0,  action: BotAction::ReleaseKey(KeyCode::Down) },
        BotEvent { at: 5.0,  action: BotAction::HoldKey(KeyCode::Left) },
        BotEvent { at: 6.5,  action: BotAction::ReleaseKey(KeyCode::Left) },
        BotEvent { at: 6.5,  action: BotAction::HoldKey(KeyCode::Up) },
        BotEvent { at: 8.0,  action: BotAction::ReleaseKey(KeyCode::Up) },
        BotEvent { at: 8.0,  action: BotAction::Assert(BotAssert::GameNotOver) },
        BotEvent { at: 15.0, action: BotAction::Assert(BotAssert::ChainAtLeast(1)) },
    ]
}

pub fn script_campaign_tutorial() -> Vec<BotEvent> {
    vec![
        BotEvent { at: 0.1, action: BotAction::Log("Starting campaign tutorial test") },
        BotEvent { at: 0.5, action: BotAction::TapKey(KeyCode::C) },
        BotEvent { at: 1.5, action: BotAction::Assert(BotAssert::ShowWorldMap) },
        BotEvent { at: 2.0, action: BotAction::TapKey(KeyCode::Space) },
        BotEvent { at: 3.5, action: BotAction::Assert(BotAssert::TutorialActive) },
        BotEvent { at: 4.0, action: BotAction::HoldKey(KeyCode::Right) },
        BotEvent { at: 6.0, action: BotAction::ReleaseKey(KeyCode::Right) },
        BotEvent { at: 6.0, action: BotAction::HoldKey(KeyCode::Up) },
        BotEvent { at: 8.0, action: BotAction::ReleaseKey(KeyCode::Up) },
        BotEvent { at: 10.0, action: BotAction::Assert(BotAssert::GameNotOver) },
        BotEvent { at: 10.0, action: BotAction::Assert(BotAssert::ChainAtLeast(1)) },
        BotEvent { at: 25.0, action: BotAction::Assert(BotAssert::TutorialDone) },
        BotEvent { at: 25.0, action: BotAction::Assert(BotAssert::ShowWorldMap) },
    ]
}

pub fn script_groove_dash() -> Vec<BotEvent> {
    vec![
        BotEvent { at: 0.5, action: BotAction::TapKey(KeyCode::Space) },
        BotEvent { at: 2.0, action: BotAction::Assert(BotAssert::InGame) },
        BotEvent { at: 3.0, action: BotAction::HoldKey(KeyCode::Right) },
        BotEvent { at: 4.5, action: BotAction::TapKey(KeyCode::Space) },
        BotEvent { at: 5.0, action: BotAction::ReleaseKey(KeyCode::Right) },
        BotEvent { at: 5.0, action: BotAction::Assert(BotAssert::GameNotOver) },
    ]
}
