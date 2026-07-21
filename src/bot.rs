use std::collections::HashSet;
use ggez::input::keyboard::KeyCode;
use ggez::glam::Vec2;

use crate::constants::STEAL_MAX_LINKS;

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
    // Closed-loop lasso practice: steer toward the nearest catchable crab without using the beam,
    // then release a fully charged lasso. This keeps the LassoGrab tutorial honest.
    SeekLasso(bool),
    // Fully charge and release the lasso at its auto-aimed target.
    FireLasso,
    // When active, steer toward the campaign pen whenever a train is being hauled. Used by the
    // delivery tutorial and BankCrabs campaign goal; disabled for goals that require a live train.
    SeekDelivery(bool),
    // Stage the player at the pen for one real delivery attempt.
    ForceDelivery,
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
    // Guards the revenge back-and-forth (ROADMAP "you steal, they steal back"): thread the player's
    // head through the line of the rival whose revenge marker is live (it just spliced your tail) so
    // the steal-back fires with the revenge bonus. Pair it after a ForceNpcCross, which is what sets
    // the marker. A no-op unless a revenge-marked rival with followers exists — deterministic, since
    // timing a chase-down against a wandering rival inside a headless budget isn't reliable.
    ForceRevengeCross,
    // Guards the defensive parry (ROADMAP "make the defense a real on-beat play"): arm a rival's
    // splice on a mid-chain link, force the beat into the on-beat window, then run the real
    // try_defend_steal helper (the same one the Stomp/Wave casts call) and confirm it cancels the
    // steal. A no-op when the player has no stealable chain. Deterministic — timing an on-beat tool
    // cast against an RNG-armed steal inside a headless budget isn't reliable, so we stage it.
    ForceStealDefense,
    // Guards the Wave's proactive crowd-control (fire_wave): deterministically place the nearest
    // rival beside the player, then cast the Wave and confirm the shove path shoved it. Staged for
    // the same reason as ForceStealDefense — headless on-beat timing against a live rival is flaky.
    ForceWaveShove,
    // Guards the movement dodge — the reroute half of the defense (INSPIRATION.md item 2: "an on-beat
    // defensive reroute OR a tool hit"). Arm a rival's splice on a mid-chain link, then teleport its
    // leader clear of that link, so the next update sees the thread broken and fizzles the splice
    // (steals_dodged rises). A no-op when the player has no stealable chain. Deterministic — juking a
    // wandering rival's thread inside a headless budget isn't reliable, so we stage it.
    ForceStealDodge,
    // Guards the whole-beach ecology steal (ROADMAP ★ headline: "rivals steal from each other, not just
    // you"). Teleport the biggest rival NPC train's leader onto a mid-follower of a strictly-smaller
    // rival and clear its rival-steal cooldown, so the rival-vs-rival splice fires this frame — the
    // bigger train slices the smaller's back half onto itself. A no-op until a smaller rival has
    // wandered far enough for its mid-follower path slot to exist. Deterministic — lining two wandering
    // leaders up by chance inside a headless budget isn't reliable, so we stage it.
    ForceRivalCross,
    /// Deterministically park a bigger train within hunt range of a smaller rival (far from the player)
    /// so the natural rival-hunt urge arms the gold "predator closing" telegraph this frame. Guards the
    /// anticipatory tell that lets the player read an impending rival-vs-rival clash (ROADMAP step 3).
    ForceRivalHunt,
    /// End the current run immediately (sets game_over). Used by the campaign_loss scenario to prove
    /// that LOSING a campaign level does not complete its world-map node (#182): the win condition,
    /// not merely finishing, is what unlocks the next level.
    ForceGameOver,
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
    /// Monotonic count of SPACE beat-tap tool chords fired this run (see MainState::chord_tools_fired).
    /// Asserts the #165 "tap SPACE on the beat + tool chord" input path actually fired a tool.
    ChordFiredAtLeast(usize),
    /// Monotonic count of crabs a rival NPC train has spliced away this run (see
    /// MainState::crabs_stolen_by_npc). Asserts the reverse-Snake steal path actually fired.
    StolenAtLeast(usize),
    /// Upper bound on the largest single rival splice this run (see
    /// MainState::max_single_steal_by_npc). Asserts the steal-size cap holds — a rival can never
    /// rustle away more than a recoverable bite in one hit, so the loop stays a fair back-and-forth.
    MaxSingleStealAtMost(usize),
    /// Monotonic count of crabs the player has rustled back off a rival this run (see
    /// MainState::crabs_stolen_by_player). Asserts the "steal to win" steal-back path actually fired.
    StolenByPlayerAtLeast(usize),
    /// Monotonic count of armed rival steals the player has parried this run (see
    /// MainState::steals_parried). Asserts the on-beat defensive counter actually cancelled a steal.
    ParriedAtLeast(usize),
    /// Monotonic count of rival leaders shoved by the Wave's proactive crowd-control (see
    /// MainState::rivals_wave_shoved). Asserts the Q shockwave's shove path actually fired.
    WaveShovedAtLeast(usize),
    /// Monotonic count of armed rival steals the player has dodged this run (see
    /// MainState::steals_dodged). Asserts the movement-reroute defense actually broke a splice.
    DodgedAtLeast(usize),
    /// Monotonic count of revenge steal-backs this run (see MainState::revenge_steals). Asserts the
    /// back-and-forth loop closed: a rival spliced your tail, you chased it down and rustled the
    /// crabs back inside the revenge window for the bonus.
    RevengeStealAtLeast(usize),
    /// Monotonic count of crabs transferred between rival NPC trains this run (see
    /// MainState::rival_vs_rival_steals). Asserts the whole-beach ecology steal fired — a bigger
    /// train spliced a smaller rival's back half onto itself, no player involved.
    RivalStealAtLeast(usize),
    /// Monotonic count of crabs knocked loose as free spoils by rival-vs-rival collisions (see
    /// MainState::rival_spill_crabs). Asserts the "eat the crumbs" spill fired — a rival-vs-rival
    /// steal scattered catchable crabs into the world for the player to swoop in on.
    RivalSpillAtLeast(usize),
    /// Monotonic count of frames a rival-vs-rival "predator closing" hunt telegraph was drawn (see
    /// MainState::rival_hunt_telegraphs). Asserts the anticipatory gold King→King tell fired — a bigger
    /// train visibly committed to a smaller rival, so the player can read the impending clash and swoop.
    RivalHuntTelegraphAtLeast(usize),
    ScoreAtLeast(usize),
    /// Whether the world-map node AFTER the currently selected one is unlocked. Asserts campaign
    /// progression gating: after LOSING a campaign level the next node must still be locked
    /// (`false`), so only meeting the win condition unlocks the next level (#182). (The played node
    /// itself is already completed here because it was reached via a skip-confirm, so its own flag
    /// can't distinguish win from loss — the next node's unlock is the observable regression.)
    SelectedNextUnlocked(bool),
    ShowWorldMap,
    MainMenu,
    /// Title screen is fully restored after leaving a run: gameplay audio is stopped and the title
    /// music is playing again.
    TitleMenuReady,
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
    pub seek_lasso: bool,                  // closed-loop movement toward a lasso target
    pub seek_delivery: bool,               // closed-loop movement toward the delivery pen
    // Set for the frame a Force*Cross helper teleports the player head onto a rival's follower slot to
    // stage a steal-back. handle_player_movement runs AFTER the force fires but BEFORE the steal
    // detection in update_npc_trains, so without this the seek-catch autopilot re-steers the head off
    // the staged slot the same frame — and on a slow frame the drift exceeds the ~54 px steal range,
    // making the forced steal-back intermittently miss. Holding the head still for that one frame lets
    // the detection see it exactly where it was placed. Cleared right after it's consumed.
    pub hold_position: bool,
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
            seek_lasso: false,
            seek_delivery: false,
            hold_position: false,
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
    // often enough to register. The beat-timed final approach isn't just polish: without it the
    // autopilot fires the whistle the instant its 4.5 s cooldown clears, which is EXACTLY 9 beats
    // (BEAT_INTERVAL 0.5 s), so every reeled-in catch phase-locks to one beat phase — when that
    // phase is off-beat a whole run banks zero on-beat catches and this test flaked ~1 run in 3.
    // This exercises the real world-map -> tutorial -> pass -> world-map transition, the
    // "tutorial->world-map" flow this test exists to guard.
    vec![
        BotEvent { at: 0.1, action: BotAction::Log("Starting campaign tutorial test") },
        BotEvent { at: 0.5, action: BotAction::TapKey(KeyCode::KeyC) },
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

pub fn script_campaign_full() -> Vec<BotEvent> {
    // Walk the complete campaign on-ramp in one process: all four tutorial nodes, then the first
    // two real maps. Assertions intentionally sit after every return-to-map transition so a level
    // that completes but leaves stale tutorial/menu state cannot hide behind the final result.
    let mut script = vec![
        BotEvent { at: 0.1, action: BotAction::Log("Starting full campaign test") },
        BotEvent { at: 0.5, action: BotAction::TapKey(KeyCode::KeyC) },
        BotEvent { at: 1.5, action: BotAction::Assert(BotAssert::ShowWorldMap) },
        // BeatTiming.
        BotEvent { at: 2.0, action: BotAction::TapKey(KeyCode::Enter) },
        BotEvent { at: 3.5, action: BotAction::Assert(BotAssert::TutorialActive) },
        BotEvent { at: 3.5, action: BotAction::SeekCatch(true) },
        BotEvent { at: 62.0, action: BotAction::Assert(BotAssert::TutorialDone) },
        // LassoGrab.
        BotEvent { at: 63.0, action: BotAction::TapKey(KeyCode::ArrowRight) },
        BotEvent { at: 63.5, action: BotAction::TapKey(KeyCode::Enter) },
        BotEvent { at: 65.0, action: BotAction::Assert(BotAssert::TutorialActive) },
        BotEvent { at: 65.0, action: BotAction::SeekLasso(true) },
    ];
    let mut t = 66.0;
    while t < 100.0 {
        script.push(BotEvent { at: t, action: BotAction::FireLasso });
        t += 1.5;
    }
    script.extend([
        BotEvent { at: 103.0, action: BotAction::Assert(BotAssert::TutorialDone) },
        // ChainDeliver.
        BotEvent { at: 104.0, action: BotAction::TapKey(KeyCode::ArrowRight) },
        BotEvent { at: 104.5, action: BotAction::TapKey(KeyCode::Enter) },
        BotEvent { at: 106.0, action: BotAction::Assert(BotAssert::TutorialActive) },
        BotEvent { at: 106.0, action: BotAction::SeekCatch(true) },
        BotEvent { at: 106.0, action: BotAction::SeekDelivery(true) },
        BotEvent { at: 180.0, action: BotAction::Assert(BotAssert::TutorialDone) },
        // ShellCrack.
        BotEvent { at: 181.0, action: BotAction::TapKey(KeyCode::ArrowRight) },
        BotEvent { at: 181.5, action: BotAction::TapKey(KeyCode::Enter) },
        BotEvent { at: 182.0, action: BotAction::SeekCatch(true) },
        BotEvent { at: 200.0, action: BotAction::Assert(BotAssert::TutorialDone) },
        // First real map: enter the BankCrabs goal and keep the run alive through several waves,
        // then verify that Escape returns the player to the campaign map.
        BotEvent { at: 201.0, action: BotAction::TapKey(KeyCode::ArrowRight) },
        BotEvent { at: 201.5, action: BotAction::TapKey(KeyCode::Enter) },
        BotEvent { at: 203.0, action: BotAction::Assert(BotAssert::InGame) },
        BotEvent { at: 203.0, action: BotAction::SeekCatch(true) },
        BotEvent { at: 203.0, action: BotAction::SeekDelivery(true) },
        BotEvent { at: 260.0, action: BotAction::Assert(BotAssert::GameNotOver) },
        BotEvent { at: 261.0, action: BotAction::TapKey(KeyCode::Escape) },
        BotEvent { at: 262.0, action: BotAction::Assert(BotAssert::MainMenu) },
        // Re-enter the campaign and select the second real map: verify its BuildTrain goal can be
        // reached without relying on the first map's completion state.
        BotEvent { at: 262.5, action: BotAction::TapKey(KeyCode::KeyC) },
        BotEvent { at: 263.5, action: BotAction::TapKey(KeyCode::ArrowRight) },
        BotEvent { at: 264.0, action: BotAction::TapKey(KeyCode::Enter) },
        BotEvent { at: 264.5, action: BotAction::TapKey(KeyCode::Enter) },
        BotEvent { at: 266.0, action: BotAction::Assert(BotAssert::InGame) },
        BotEvent { at: 266.0, action: BotAction::SeekDelivery(false) },
        BotEvent { at: 310.0, action: BotAction::Assert(BotAssert::GameNotOver) },
        BotEvent { at: 315.0, action: BotAction::TapKey(KeyCode::Escape) },
        BotEvent { at: 316.0, action: BotAction::Assert(BotAssert::MainMenu) },
    ]);
    let mut delivery_at = 108.0;
    while delivery_at < 260.0 {
        script.insert(
            script.len().saturating_sub(1),
            BotEvent { at: delivery_at, action: BotAction::ForceDelivery },
        );
        delivery_at += 1.0;
    }
    script.sort_by(|a, b| a.at.total_cmp(&b.at));
    script
}

pub fn script_campaign_escape() -> Vec<BotEvent> {
    // Campaign Escape must return to the main menu from an active regular level instead of quitting
    // the application. Select the first regular campaign node, confirm the soft skip warning, then
    // leave the started level with Escape.
    vec![
        BotEvent { at: 0.1, action: BotAction::Log("Starting campaign Escape test") },
        BotEvent { at: 0.5, action: BotAction::TapKey(KeyCode::KeyC) },
        BotEvent { at: 1.0, action: BotAction::TapKey(KeyCode::ArrowRight) },
        BotEvent { at: 1.1, action: BotAction::TapKey(KeyCode::ArrowRight) },
        BotEvent { at: 1.2, action: BotAction::TapKey(KeyCode::ArrowRight) },
        BotEvent { at: 1.3, action: BotAction::TapKey(KeyCode::ArrowRight) },
        BotEvent { at: 1.6, action: BotAction::TapKey(KeyCode::Enter) },
        BotEvent { at: 2.0, action: BotAction::TapKey(KeyCode::Enter) },
        BotEvent { at: 2.5, action: BotAction::Assert(BotAssert::InGame) },
        BotEvent { at: 3.0, action: BotAction::TapKey(KeyCode::Escape) },
        BotEvent { at: 3.5, action: BotAction::Assert(BotAssert::MainMenu) },
        BotEvent { at: 3.5, action: BotAction::Assert(BotAssert::TitleMenuReady) },
    ]
}

pub fn script_campaign_loss() -> Vec<BotEvent> {
    // Regression guard for the campaign win-condition gate (#182): LOSING a level must NOT complete
    // its world-map node — only meeting the WinCondition unlocks the next level. The bug was that
    // return_to_world_map called complete_selected unconditionally, so dismissing the game-over
    // screen after a loss still unlocked the next node. Mirror campaign_escape's navigation into the
    // first regular campaign node (skip-confirm past the tutorials), then force a game over, dismiss
    // it with Space, and assert we're back on the map with the NEXT node still locked.
    vec![
        BotEvent { at: 0.1, action: BotAction::Log("Starting campaign loss test") },
        BotEvent { at: 0.5, action: BotAction::TapKey(KeyCode::KeyC) },
        BotEvent { at: 1.0, action: BotAction::TapKey(KeyCode::ArrowRight) },
        BotEvent { at: 1.1, action: BotAction::TapKey(KeyCode::ArrowRight) },
        BotEvent { at: 1.2, action: BotAction::TapKey(KeyCode::ArrowRight) },
        BotEvent { at: 1.3, action: BotAction::TapKey(KeyCode::ArrowRight) },
        BotEvent { at: 1.6, action: BotAction::TapKey(KeyCode::Enter) },
        BotEvent { at: 2.0, action: BotAction::TapKey(KeyCode::Enter) },
        BotEvent { at: 2.5, action: BotAction::Assert(BotAssert::InGame) },
        // Lose the run, then dismiss the game-over screen (Space in campaign returns to the map).
        BotEvent { at: 3.0, action: BotAction::ForceGameOver },
        BotEvent { at: 3.5, action: BotAction::TapKey(KeyCode::Space) },
        // Back on the map — and crucially losing did NOT unlock the next level (the win condition,
        // not merely finishing the run, is what advances the campaign).
        BotEvent { at: 4.0, action: BotAction::Assert(BotAssert::ShowWorldMap) },
        BotEvent { at: 4.0, action: BotAction::Assert(BotAssert::SelectedNextUnlocked(false)) },
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
    // The steal must stay a recoverable bite: across every forced crossing above (the seek-catch
    // chain grows well past the cap), no single splice may take more than STEAL_MAX_LINKS. Guards the
    // "fun, not punishing" cap against a regression that lets a rival wipe the whole tail in one hit.
    script.push(BotEvent {
        at: 48.0,
        action: BotAction::Assert(BotAssert::MaxSingleStealAtMost(STEAL_MAX_LINKS)),
    });
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
    // Also exercise the Wave's proactive shove (fire_wave) a few times across the window — each stages
    // the nearest rival beside the player and casts, so the shove path is regression-covered too.
    for wt in [20.0_f32, 28.0, 36.0, 44.0] {
        script.push(BotEvent { at: wt, action: BotAction::ForceWaveShove });
    }
    script.push(BotEvent { at: 48.0, action: BotAction::Assert(BotAssert::GameNotOver) });
    script.push(BotEvent { at: 48.0, action: BotAction::Assert(BotAssert::ParriedAtLeast(1)) });
    script.push(BotEvent { at: 48.0, action: BotAction::Assert(BotAssert::WaveShovedAtLeast(1)) });
    script
}

pub fn script_steal_dodge() -> Vec<BotEvent> {
    // Guards the movement dodge — the reroute half of the defense (INSPIRATION.md item 2 promises TWO
    // defenses: "an on-beat defensive reroute OR a tool hit"). Juking the threaded tail link clear of
    // the rival before the snap breaks the thread, so the splice fizzles with nothing to cut. That
    // second defense had no coverage, so a refactor could silently break it. Mirrors
    // script_steal_defense: build a real chain with the seek-catch autopilot, then repeatedly stage
    // "arm a steal, then yank the tail clear" (ForceStealDodge) and assert the dodge fired
    // (steals_dodged rises) without crashing the run. Forcing keeps it deterministic — juking a
    // wandering rival isn't reliable headless. Runs at 3x time_scale like npc_steal so the autopilot's
    // proximity catch grows a chain first.
    let mut script = vec![
        BotEvent { at: 0.1, action: BotAction::Log("Starting steal-dodge (reroute) test") },
        BotEvent { at: 0.5, action: BotAction::TapKey(KeyCode::Space) },
        BotEvent { at: 2.0, action: BotAction::Assert(BotAssert::InGame) },
        BotEvent { at: 2.0, action: BotAction::SeekCatch(true) },
        BotEvent { at: 24.0, action: BotAction::Assert(BotAssert::CaughtAtLeast(1)) },
    ];
    // Stage arm+dodge every 0.9s across a wide window. Each attempt is a no-op unless a stealable
    // chain (>= 2 links) exists that frame, so firing many times makes it near-certain at least one
    // lands while the seek-catch chain is alive. A clean reroute (like the tool parry) marks the juked
    // rival for revenge and opens a counter-steal window — a following ForceRevengeCross ~0.45s later
    // threads that marked rival to close the counter-steal, which guards the new "a dodge opens a
    // counter window" reward so it can't silently regress.
    let mut t = 14.0_f32;
    while t < 46.0 {
        script.push(BotEvent { at: t, action: BotAction::ForceStealDodge });
        script.push(BotEvent { at: t + 0.45, action: BotAction::ForceRevengeCross });
        t += 0.9;
    }
    script.push(BotEvent { at: 48.0, action: BotAction::Assert(BotAssert::GameNotOver) });
    script.push(BotEvent { at: 48.0, action: BotAction::Assert(BotAssert::DodgedAtLeast(1)) });
    // The on-beat dodge must open a counter-steal window the player can cash — assert the revenge
    // steal-back fired off a dodge-marked rival (mirrors script_revenge's splice-then-revenge guard).
    script.push(BotEvent { at: 48.0, action: BotAction::Assert(BotAssert::RevengeStealAtLeast(1)) });
    script
}

pub fn script_revenge() -> Vec<BotEvent> {
    // Guards the revenge back-and-forth — the "you steal, they steal back" half of the steal fight
    // (ROADMAP headline "tune so it's fun, not punishing... a tense back-and-forth"). After a rival
    // splices your tail it's marked for a few seconds; rustling the crabs back off that same rival
    // inside the window pays a revenge bonus and increments revenge_steals. That loop had no
    // coverage, so a refactor could silently break the marker or the bonus. Build a real chain with
    // the seek-catch autopilot, then repeatedly stage "rival splices you (ForceNpcCross), then chase
    // it and steal back (ForceRevengeCross)" and assert the revenge steal-back fired. Forcing keeps
    // it deterministic. Runs at 3x time_scale like npc_steal so the autopilot grows a chain first.
    let mut script = vec![
        BotEvent { at: 0.1, action: BotAction::Log("Starting revenge back-and-forth test") },
        BotEvent { at: 0.5, action: BotAction::TapKey(KeyCode::Space) },
        BotEvent { at: 2.0, action: BotAction::Assert(BotAssert::InGame) },
        BotEvent { at: 2.0, action: BotAction::SeekCatch(true) },
        BotEvent { at: 24.0, action: BotAction::Assert(BotAssert::CaughtAtLeast(1)) },
    ];
    // Interleave splice-then-revenge every 0.7s across a wide window. ForceNpcCross marks the nearest
    // rival and hands it your tail; ~0.5s later ForceRevengeCross threads your head through that same
    // marked rival so the steal-back fires inside the 6s revenge window. Each pair is a no-op unless a
    // stealable chain exists that frame, and the splice only completes when the seek-catch chain survives
    // the ~one-beat steal fuse — which at a high frame rate can whole-run-miss if the attempts are too
    // sparse. A moderately denser, wider stream than the old 0.9s (but not so dense it out-drains the
    // seek-catch chain) makes at least one splice complete and one steal-back land near-certain across
    // frame rates (#170), without changing what's asserted. The revenge cross lags its splice by 0.5s so
    // the real ~one-beat fuse has fired and set the marker before the cross tries to cash it.
    let mut t = 13.0_f32;
    while t < 47.0 {
        script.push(BotEvent { at: t, action: BotAction::ForceNpcCross });
        script.push(BotEvent { at: t + 0.5, action: BotAction::ForceRevengeCross });
        t += 0.7;
    }
    script.push(BotEvent { at: 48.0, action: BotAction::Assert(BotAssert::GameNotOver) });
    script.push(BotEvent { at: 48.0, action: BotAction::Assert(BotAssert::RevengeStealAtLeast(1)) });
    script
}

pub fn script_npc_vs_npc() -> Vec<BotEvent> {
    // Guards the whole-beach ecology steal — the ★ HEADLINE mechanic (ROADMAP: "rivals steal from
    // each other, not just you"). When a bigger rival NPC train threads a smaller rival's follower
    // line it splices the smaller one's back half onto itself (update_npc_trains), so the beach churns
    // crabs between trains with no player involved — a genuine ecosystem (agar.io + Rain World). That
    // path had no coverage, so a refactor could silently break it. Unlike the player-facing steal
    // tests this needs no player chain: we just enter the game, let the three ambient trains wander so
    // their follower path history fills, then repeatedly force the biggest train onto a smaller rival's
    // mid-follower (ForceRivalCross) and assert a transfer fired (rival_vs_rival_steals rises) without
    // crashing the run. Forcing keeps it deterministic — lining two wandering leaders up by chance
    // isn't reliable headless. Seek-catch keeps the player busy so free crabs don't pile to the
    // overwhelmed game-over; runs at 3x time_scale like the other steal tests.
    let mut script = vec![
        BotEvent { at: 0.1, action: BotAction::Log("Starting rival-vs-rival ecology steal test") },
        BotEvent { at: 0.5, action: BotAction::TapKey(KeyCode::Space) },
        BotEvent { at: 2.0, action: BotAction::Assert(BotAssert::InGame) },
        BotEvent { at: 2.0, action: BotAction::SeekCatch(true) },
    ];
    // Force a rival crossing every 0.9s across a wide window. Each attempt is a no-op until a smaller
    // rival has wandered far enough that its mid-follower path slot exists, so firing many times across
    // ~30s makes it near-certain at least one lands. Start at 10s to give the slow elder time to trail
    // a path history its followers sit on.
    let mut t = 10.0_f32;
    while t < 44.0 {
        script.push(BotEvent { at: t, action: BotAction::ForceRivalCross });
        // Interleave a deterministic hunt setup so the anticipatory "predator closing" telegraph arms
        // on the same frames (both read live positions through the real update path).
        script.push(BotEvent { at: t + 0.45, action: BotAction::ForceRivalHunt });
        t += 0.9;
    }
    script.push(BotEvent { at: 46.0, action: BotAction::Assert(BotAssert::GameNotOver) });
    script.push(BotEvent { at: 46.0, action: BotAction::Assert(BotAssert::RivalStealAtLeast(1)) });
    // ...and that the collision spilled catchable crumbs into the world (ROADMAP step 3, agar.io
    // "eat the crumbs"): a fraction of each rival-vs-rival cut of ≥2 breaks loose as free crabs the
    // player can swoop in and rustle, instead of all transferring to the winner. Forcing ~38 crossings
    // onto mid-followers of multi-crab rivals makes at least one qualifying cut near-certain, so this
    // guards the spill path can't silently regress to a clean pickpocket.
    script.push(BotEvent { at: 46.0, action: BotAction::Assert(BotAssert::RivalSpillAtLeast(1)) });
    // ...and that the anticipatory "predator closing" telegraph fired (ROADMAP step 3 "make it legible
    // and swoopable"): a bigger King committing to a smaller rival paints a gold King→King line so the
    // player reads the impending clash from afar and pre-positions to swoop the spilled crumbs. Repeatedly
    // forcing the biggest train onto a smaller rival leaves the two leaders adjacent, so the natural
    // hunt urge arms the telegraph on the following frames — guarding the tell can't silently regress.
    script.push(BotEvent { at: 46.0, action: BotAction::Assert(BotAssert::RivalHuntTelegraphAtLeast(1)) });
    script
}

pub fn script_groove_dash() -> Vec<BotEvent> {
    // Smoke-tests the SPACE beat-tap: a dash on its own, and — added with #165 — a SPACE+tool CHORD.
    // Holding a tool key (E) and tapping SPACE fires that tool ON the beat-tap instead of dashing, so
    // it exercises the new chord input path end to end. The chord is a no-op-safe cast (self-guards on
    // cooldown), so we assert the monotonic chord counter rose rather than any tool side effect.
    vec![
        BotEvent { at: 0.5, action: BotAction::TapKey(KeyCode::Space) },
        BotEvent { at: 2.0, action: BotAction::Assert(BotAssert::InGame) },
        BotEvent { at: 3.0, action: BotAction::HoldKey(KeyCode::ArrowRight) },
        BotEvent { at: 4.5, action: BotAction::TapKey(KeyCode::Space) },
        BotEvent { at: 5.0, action: BotAction::ReleaseKey(KeyCode::ArrowRight) },
        BotEvent { at: 5.0, action: BotAction::Assert(BotAssert::GameNotOver) },
        // #165 chord: hold the whistle key, then tap SPACE on it — fires the whistle as a beat-tap
        // flavor rather than a dash. HoldKey lands the key in keys_held the frame BEFORE the SPACE
        // tap so the chord detection (which reads keys_held) sees it held.
        BotEvent { at: 5.5, action: BotAction::HoldKey(KeyCode::KeyE) },
        BotEvent { at: 5.8, action: BotAction::TapKey(KeyCode::Space) },
        BotEvent { at: 6.0, action: BotAction::ReleaseKey(KeyCode::KeyE) },
        BotEvent { at: 6.2, action: BotAction::Assert(BotAssert::ChordFiredAtLeast(1)) },
        BotEvent { at: 6.2, action: BotAction::Assert(BotAssert::GameNotOver) },
    ]
}
