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
    // Closed-loop autopilot: while active the bot steers the player toward the nearest catchable
    // crab, auto-whistles it into range, and stomps any shelled crab it walks up to, so a catch
    // test exercises the real catching loop (movement + whistle charm/pull + stomp crack + proximity
    // catch) without depending on where the RNG happened to scatter the handful of early crabs.
    // `true` turns it on, `false` off.
    SeekCatch(bool),
}

#[derive(Clone, Debug)]
pub enum BotAssert {
    GameNotOver,
    ChainAtLeast(usize),
    /// Monotonic total catches this run (see MainState::total_caught). Prefer this over ChainAtLeast
    /// to assert "the catching verb works": the live chain drops to 0 whenever the train is banked,
    /// snaps, or is scattered, so a ChainAtLeast(1) check can race a reset and flake even though the
    /// bot caught plenty. total_caught never drops, so the assert is stable.
    CaughtAtLeast(usize),
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
    pub seek_catch: bool,                  // closed-loop autopilot toward the nearest catchable crab
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
            seek_catch: false,
        }
    }
}

pub fn script_menu_to_game() -> Vec<BotEvent> {
    // Verifies the core catching verb still works from a cold start: enter the game, then hand the
    // player to the seek-catch autopilot, which steers toward the nearest catchable crab and
    // auto-whistles it into range. A blind fixed sweep can't reliably find one of only a handful of
    // early crabs scattered randomly across the 2× scrolling world — the failure that had this test
    // disabled — so we close the loop instead of gambling on RNG. This still exercises the real
    // movement, whistle charm/pull, stomp crack, and proximity-catch code; only the pathfinding is
    // automated. menu_to_game runs at 3× time_scale (see the setup in main.rs).
    vec![
        BotEvent { at: 0.1, action: BotAction::Log("Starting menu->game test") },
        BotEvent { at: 0.5, action: BotAction::TapKey(KeyCode::Space) },
        BotEvent { at: 2.0, action: BotAction::Assert(BotAssert::InGame) },
        BotEvent { at: 2.0, action: BotAction::SeekCatch(true) },
        BotEvent { at: 8.0, action: BotAction::Assert(BotAssert::GameNotOver) },
        // Give the autopilot a generous window: the whistle recharges every 4.5 s, so 22 s of
        // seeking guarantees several catch attempts even when the only reachable crab is a far,
        // fast one on the far side of the scrolling world. (menu_to_game runs at 3× time_scale.)
        // Assert on total_caught, not the live chain: by 22 s the autopilot has caught many crabs,
        // but the chain resets to 0 on a bank/snap/scatter, so a ChainAtLeast check here flakes.
        BotEvent { at: 22.0, action: BotAction::Assert(BotAssert::CaughtAtLeast(1)) },
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
