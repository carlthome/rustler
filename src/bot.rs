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
    // Deterministically exercise the reverse-Snake steal path: teleport the nearest rival NPC King
    // Crab train's leader onto a mid-chain link of the player's conga line and clear its steal
    // cooldown, so the splice-steal fires this frame if a stealable chain exists. A no-op when the
    // player has no chain (chain_count < 2). Fired repeatedly across a window so at least one attempt
    // lands while a chain is present — the steal AI's natural pursuit is too RNG-dependent to time.
    ForceNpcCross,
    // The mirror of ForceNpcCross for the player's "steal to win" splice: teleport the player's head
    // onto the nearest rival NPC train's mid-follower and clear the steal-back cooldown, so the
    // reciprocal splice (player rustles the rival's back section onto their own line) fires this
    // frame. A no-op when the player has no train or no rival has followers left. Fired repeatedly so
    // at least one attempt lands while a chain is present.
    ForcePlayerCross,
    // Guards the defensive parry (ROADMAP "make the defense a real on-beat play"): arm a rival's
    // splice on a mid-chain link, force the beat into the on-beat window, then run the real
    // try_defend_steal helper (the same one the Stomp/Wave casts call) and confirm it cancels the
    // steal. A no-op when the player has no stealable chain. Deterministic — timing an on-beat tool
    // cast against an RNG-armed steal inside a headless budget isn't reliable, so we stage it.
    ForceStealDefense,
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
    /// Monotonic count of crabs a rival NPC train has spliced away this run (see
    /// MainState::crabs_stolen_by_npc). Asserts the reverse-Snake steal path actually fired.
    StolenAtLeast(usize),
    /// Monotonic count of crabs the player has rustled back off a rival this run (see
    /// MainState::crabs_stolen_by_player). Asserts the "steal to win" steal-back path actually fired.
    StolenByPlayerAtLeast(usize),
    /// Monotonic count of armed rival steals the player has parried this run (see
    /// MainState::steals_parried). Asserts the on-beat defensive counter actually cancelled a steal.
    ParriedAtLeast(usize),
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
    // Drives the campaign on-ramp end to end: title -> world map (C) -> enter the first node, which
    // is the BeatTiming tutorial (world_map.rs) -> clear it -> confirm it hands control back to the
    // world map. The first node's pass condition is 3 ON-BEAT catches, so a blind Right/Up walk (the
    // old script) could never clear it — worse, at 8× time_scale the player teleported past crabs
    // between frames and caught nothing at all, the failure that had this test disabled. We now hand
    // the player to the seek-catch autopilot, which in a BeatTiming tutorial stages just outside
    // catch range and closes the final step on the beat (see handle_player_movement), so the on-beat
    // catches actually land — and we run at 3× (like menu_to_game) so the proximity catch fires
    // often enough to register. This exercises the real world-map -> tutorial -> pass -> world-map
    // transition, the "tutorial->world-map" flow this test exists to guard.
    vec![
        BotEvent { at: 0.1, action: BotAction::Log("Starting campaign tutorial test") },
        BotEvent { at: 0.5, action: BotAction::TapKey(KeyCode::C) },
        BotEvent { at: 1.5, action: BotAction::Assert(BotAssert::ShowWorldMap) },
        BotEvent { at: 2.0, action: BotAction::TapKey(KeyCode::Space) },
        BotEvent { at: 3.5, action: BotAction::Assert(BotAssert::TutorialActive) },
        BotEvent { at: 3.5, action: BotAction::SeekCatch(true) },
        // Mid-run sanity: the tutorial is alive and the autopilot is landing catches (total_caught
        // never drops, unlike the live chain, so this can't race a bank/snap reset).
        BotEvent { at: 16.0, action: BotAction::Assert(BotAssert::GameNotOver) },
        BotEvent { at: 16.0, action: BotAction::Assert(BotAssert::CaughtAtLeast(1)) },
        // By now the 3 on-beat catches are in, the "PASSED!" celebration has played, and the ~2.2s
        // (real-time) exit hold has returned us to the world map. Wide margin so even an unlucky
        // low-on-beat-rate run banks its 3rd on-beat catch and completes the exit hold well before
        // we check — the failure mode we're guarding against is a race, not a missing capability.
        BotEvent { at: 62.0, action: BotAction::Assert(BotAssert::TutorialDone) },
        BotEvent { at: 62.0, action: BotAction::Assert(BotAssert::ShowWorldMap) },
    ]
}

pub fn script_npc_steal() -> Vec<BotEvent> {
    // Guards the reverse-Snake train-vs-train steal — the core conga-ecology mechanic (see ROADMAP.md
    // headline and INSPIRATION.md "The core steal mechanic"). The steal path (rival NPC King Crab
    // train crosses the player's chain -> back section detaches -> snaps onto the rival) is live in
    // update_npc_trains but had no coverage, so a refactor could silently break it. This test builds a
    // real player chain with the seek-catch autopilot, then repeatedly forces the nearest rival to
    // thread through it (ForceNpcCross) and asserts a splice actually fired (crabs_stolen_by_npc rises)
    // without crashing the run. Forcing keeps it deterministic — the rival's natural pursuit is too
    // RNG-dependent to land inside a headless time budget. Runs at 3x time_scale like menu_to_game so
    // the proximity catch fires often enough for the autopilot to grow a chain.
    let mut script = vec![
        BotEvent { at: 0.1, action: BotAction::Log("Starting NPC steal test") },
        BotEvent { at: 0.5, action: BotAction::TapKey(KeyCode::Space) },
        BotEvent { at: 2.0, action: BotAction::Assert(BotAssert::InGame) },
        BotEvent { at: 2.0, action: BotAction::SeekCatch(true) },
        // Let the autopilot build a chain first. Catching is genuinely slow/RNG (the whistle
        // recharges every 4.5s and the world is 2x the viewport), so give it the same generous
        // window menu_to_game proves reliable before asserting a catch has landed.
        BotEvent { at: 24.0, action: BotAction::Assert(BotAssert::CaughtAtLeast(1)) },
    ];
    // Force a crossing every 0.9s across a wide window. Each attempt is a no-op unless a stealable
    // chain (>= 2 links) exists that frame, so firing many times across ~30s makes it near-certain at
    // least one lands on a chain moment — the seek-catch chain grows and resets as it banks/snaps.
    let mut t = 14.0_f32;
    while t < 46.0 {
        script.push(BotEvent { at: t, action: BotAction::ForceNpcCross });
        t += 0.9;
    }
    script.push(BotEvent { at: 48.0, action: BotAction::Assert(BotAssert::GameNotOver) });
    script.push(BotEvent { at: 48.0, action: BotAction::Assert(BotAssert::StolenAtLeast(1)) });
    script
}

pub fn script_player_steal() -> Vec<BotEvent> {
    // Guards the player's "steal to win" reverse-Snake steal-BACK — driving your train's head through
    // a rival NPC King Crab's line rustles the rival's back section onto your own train (shipped in
    // #32; see ROADMAP.md headline "before the player can steal back" and INSPIRATION.md "The core
    // steal mechanic"). That mechanic landed with no bot coverage, so a refactor could silently break
    // it. Mirrors script_npc_steal: build a real player chain with the seek-catch autopilot, then
    // repeatedly force the player's head onto the nearest rival's mid-follower (ForcePlayerCross) and
    // assert a steal-back actually fired (crabs_stolen_by_player rises) without crashing the run.
    // Forcing keeps it deterministic — threading the head into a wandering rival by chance is too
    // RNG-dependent for a headless budget. Runs at 3x time_scale like menu_to_game so the autopilot's
    // proximity catch fires often enough to grow a chain first.
    let mut script = vec![
        BotEvent { at: 0.1, action: BotAction::Log("Starting player steal-back test") },
        BotEvent { at: 0.5, action: BotAction::TapKey(KeyCode::Space) },
        BotEvent { at: 2.0, action: BotAction::Assert(BotAssert::InGame) },
        BotEvent { at: 2.0, action: BotAction::SeekCatch(true) },
        // Same generous window menu_to_game proves reliable before asserting a catch has landed.
        BotEvent { at: 24.0, action: BotAction::Assert(BotAssert::CaughtAtLeast(1)) },
    ];
    // Force a crossing every 0.9s across a wide window. Each attempt is a no-op unless the player has
    // a train (>= 1 link) and a rival still has followers, so firing many times across ~30s makes it
    // near-certain at least one lands while the seek-catch chain is alive.
    let mut t = 14.0_f32;
    while t < 46.0 {
        script.push(BotEvent { at: t, action: BotAction::ForcePlayerCross });
        t += 0.9;
    }
    script.push(BotEvent { at: 48.0, action: BotAction::Assert(BotAssert::GameNotOver) });
    script.push(BotEvent { at: 48.0, action: BotAction::Assert(BotAssert::StolenByPlayerAtLeast(1)) });
    script
}

pub fn script_steal_defense() -> Vec<BotEvent> {
    // Guards the defensive parry — the skill half of the steal fight (ROADMAP headline "make the
    // defense a real on-beat play"). An on-beat Stomp/Wave cast on a rival threading your tail cancels
    // its armed splice (try_defend_steal). That counter-play had no coverage, so a refactor could
    // silently break it. Mirrors script_npc_steal: build a real player chain with the seek-catch
    // autopilot, then repeatedly stage "arm a steal, then parry it on-beat" (ForceStealDefense) and
    // assert the parry fired (steals_parried rises) without crashing the run. Forcing keeps it
    // deterministic — timing an on-beat cast against an RNG-armed steal isn't reliable headless. Runs
    // at 3x time_scale like npc_steal so the autopilot's proximity catch grows a chain first.
    let mut script = vec![
        BotEvent { at: 0.1, action: BotAction::Log("Starting steal-defense (parry) test") },
        BotEvent { at: 0.5, action: BotAction::TapKey(KeyCode::Space) },
        BotEvent { at: 2.0, action: BotAction::Assert(BotAssert::InGame) },
        BotEvent { at: 2.0, action: BotAction::SeekCatch(true) },
        BotEvent { at: 24.0, action: BotAction::Assert(BotAssert::CaughtAtLeast(1)) },
    ];
    // Stage arm+parry every 0.9s across a wide window. Each attempt is a no-op unless a stealable
    // chain (>= 2 links) exists that frame, so firing many times makes it near-certain at least one
    // lands while the seek-catch chain is alive.
    let mut t = 14.0_f32;
    while t < 46.0 {
        script.push(BotEvent { at: t, action: BotAction::ForceStealDefense });
        t += 0.9;
    }
    script.push(BotEvent { at: 48.0, action: BotAction::Assert(BotAssert::GameNotOver) });
    script.push(BotEvent { at: 48.0, action: BotAction::Assert(BotAssert::ParriedAtLeast(1)) });
    script
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
