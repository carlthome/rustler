# Roadmap

High-level capabilities we believe would make Crab Rustler more fun, kept short and scannable.
Maintained by the Game Director agent (see CLAUDE.md, Cron 6) — it reads Slack feedback on
releases and updates this list accordingly. Feature Developer and Overnight Developer read
this for direction before picking their next task; they don't edit it themselves.

**Sequencing.** Two phases, in order — don't jump ahead to phase 2 while phase 1 items remain:

1. **Now — depth before breadth.** Make the core inner loop (catching, chain, rhythm) excellent
   before the game goes wide. Favor items that deepen existing mechanics over ones that add
   parallel modes or systems. Hold off on anything like alternate game modes until the core
   feels done.
2. **Later — the outer loop.** Once the inner loop is tight and solid, shift attention to
   sustaining player motivation across runs and sessions: meta-progression, unlocks, reasons to
   come back.

**Where we are.** The inner loop is deep and closed: four catching tools (beam/lasso/whistle/stomp) with
upgrade lanes, a conga train with a chain-snap downside and a delivery-pen jackpot, and rhythm/groove scoring
that **drives real mechanics** (downbeat spawn drops, on-beat PERFECT hits, a beat-stepping train, Groove
Dash/Call/Gamble/Slam, a charged Drum Roll, a catch bloom). Pacing ramps through named intensity stages.
**Eight archetypes** form a dense emergent web with visible catch-time crossovers — the signature fun.
**All three train slots now carry weight**: head figureheads, tail placement, and (new this cycle) a
mid-train **CENTERPIECE** that pays a scaling bonus for a deep same-type run straddling the midpoint.
The **bank-now-vs-push-luck** axis is closed (live AT RISK readout, escalating snap teeth, superlinear
`pen_worth = (n·(n+1)/2)·3` bank payoff, BIG/LONG/GRAND HAUL cashout). Biomes each carry a distinct,
telegraphed terrain hazard, all three bosses fight inside the archetype web, and a four-scenario opt-in
tutorial doubles as regression tests. A first slice of meta-progression + campaign scaffolding exists but
stays parked in "Later" — the gate is Carl's explicit "core feels done" call, which hasn't come.

**Signal.** No new Slack reactions/replies this cycle (recent Dev Diary posts unreacted; the only thread
reply is old channel-meta, not direction), so direction holds from Carl's last substantive playtest and his
**mechanics-freeze call**: strengthen what exists so the player feels *agency and control*. Prior playtest
asks are all resolved — the upgrade screen (frequency/timing/3-pick redesign), the level "arrives somewhere
mechanically" ask (rarer-bigger boundaries + per-biome hazards), and a first slice of the beat-mastery ceiling
(super-linear PERFECT streak). The procedural-horde/leaderboard endless vision stays sharpened in "Later".

## Bugs (fix before anything else in Now)

Stability beats new features — an agent picking a task should check here first, before any
item in "Now" below.

- None open. (Fixed: the upgrade screen fired at the wrong time / popped back-to-back — c01b922 loops the
  threshold past the current banked score, 3b17573 fires the check at the pen; also the start-of-run
  `InstanceArray capacity > 0` crash and the windowed-instead-of-fullscreen bug.) If you hit a panic or a
  wrong-looking frame while testing, log it here before shipping anything new.

## Now

- **NOTE — MECHANICS FREEZE (called by Carl this cycle).** Carl: "We might have sufficient game mechanics
  content for now, and should work on strengthening what we have to make the player feel agency and control."
  Do **not** add new mechanics — no new crab archetypes, no new player verbs/tools, no new parallel systems —
  until Carl explicitly lifts the freeze. Any such idea goes to "Also on our mind", not into work. Every live
  Now item below survives the freeze: it *deepens an existing verb* (the Cycle reposition) rather than adding a
  new one — exactly the "make the player feel in control" work Carl asked for. *(Shipped this cycle and checked
  off: the train-middle arrangement frontier — a mid-train CENTERPIECE now pays a scaling bonus for a deep
  same-type run straddling the midpoint, with a live ring on the train and an ARRANGED breakdown in the HAUL
  readout — 60ce8a0/3db854b/e5dc23d. All three train slots now carry arrangement weight, and legibility while
  building is well covered by next-catch rings, cycle-promote preview, and the live readouts.)*

- **[TOP PRIORITY — PREREQUISITE FOR EVERYTHING BELOW] Scrolling world: extend the map beyond the fixed viewport.** The competing-conga-lines vision (NPC King Crab trains, BYO music dominating the mix, train-vs-train stealing) cannot work in a fixed viewport — rival trains need space to approach from off-screen, their music needs to be audible before they're visible, and players need room to maneuver. This is also the "explorable maps" item Carl called out (Black Isle / Vampire Survivors exploration dopamine). Concretely: a world larger than the viewport, a camera that follows the player's train, and spawning/NPC logic that works in world-space rather than screen-space. Scaffolding already in place: radar arrows (off-screen awareness), screen/world coordinate separation in draw code, off-screen draw culling. This is the single most load-bearing architectural change before NPC ecology can begin.

- **Give the player active control over train ORDER — the agency gap Carl named.** Arrangement is
  now legible and pays off (CENTERPIECE, sandwiches, figureheads), but the player can barely *shape* it: order is
  dictated by catch order, and the one manipulation verb — Cycle (X) — only rotates the whole train one slot, which
  can move a crab to the head but can't repair the interior. If two matching crabs land on opposite sides of a
  mismatch, the only fix is banking and restarting the run. Deepen the existing Cycle verb so the player can
  actively *build* a centerpiece or sandwich on purpose at speed (e.g. an on-beat local swap / bubble-toward-center,
  reusing the adjacency + beat-gate systems Cycle already uses). Freeze-safe — no new verb, it extends the one that
  exists — and it turns holding a long train into a puzzle you can *solve*, not just a risk you carry and a payout
  you hope catch-order handed you. This is the difference between reading the arrangement and controlling it.

## Later (outer loop — not yet)

- **Expand meta-progression past the first slice** — the persistent career + perk shop is in.
  Once Carl signals the inner loop feels done, grow it: more permanent unlocks (a new crab
  archetype, a cosmetic, a starting biome), a run-history readout, small run-to-run goals. Keep
  it a single save file, not a sprawling meta-tree. Deliberately held here so depth-first inner-
  loop work stays first.
- **Campaign / story mode + world map** *(much later, after meta-progression is solid)*. Carl's
  vision: a campaign mode acts as the mainstream "learn to play the game" path — a world map,
  hand-crafted levels, choices with consequences, narrative stakes. Most players finish it once.
  Then the **arcade mode** (leaderboards, scoring pressure, ruthless difficulty) is the real game
  to master for the players who want to go deep. The two modes complement rather than compete:
  campaign funnels players into arcade, arcade is the prestige path. Don't start this until the
  inner loop and meta-progression feel done — the arcade mode has to be worth mastering before
  the campaign exists to funnel people toward it. *A skeleton has landed (world-map node list +
  navigation, player-skin slots) — treat it as scaffolding parked here, NOT as license to build out
  the campaign; it stays deferred until the "core feels done" call and meta-progression are settled.*

- **Desktop level — fourth-wall terrain.** A special level with a transparent/borderless window
  where the game reads the user's desktop pixels as terrain and treats their OS window borders as
  solid walls/platforms crabs must navigate around. The secret mechanic: players can reshape the
  level mid-run by dragging their browser, Finder, or other windows — not documented anywhere, just
  discovered. In the spirit of old Windows desktop toys and Inscryption-style reality-breaking
  moments. Technically: macOS/Wayland screen-capture API for the background grab, transparent ggez
  window layer, luminance threshold → terrain collision map. Deliberately deferred — needs OS-level
  screen capture permission UX to be unobtrusive. File under "the most delightful thing in the
  whole game when someone stumbles on it."

## Blocked (needs a human, not a code agent)

- **The soundtrack builds with the groove** — the `layer{1,2,3}.ogg` progressive-fade hook exists in
  code (main.rs loads them at startup) but no audio files populate it, so it's inert. This needs
  someone to actually author/source three stacking music layers and drop them in `resources/`; a
  headless dev agent can't compose them. Wiring them to the Groove meter once they exist is trivial.
  Parked here so feature agents stop bouncing off it — pick it up when Carl provides the stems.

## Also on our mind (not sequenced — no urgency, just don't lose it)

- **Level system rethink — direction for when the full rework lands in "Now".** Carl's playtest feedback pulls
  in one direction: levels should feel like *travelling somewhere that matters*, less often but with more
  impact, and the whole levels/campaign/tutorial split should be woven together more holistically. Context for
  a future Feature Developer, not a spec:
  - **Fewer, more impactful biome shifts.** Lean into the Control-style floor-banner aesthetic (Carl likes the
    font/transition), but make each boundary a genuine gear-change — new biome + new terrain hazard + new
    enemy-archetype emphasis together, not a tint swap. Rarer transitions each carrying real weight beats
    frequent shallow ones.
  - **Beat-mastery skill ceiling.** Carl wants the "if you play the mechanics PERFECTLY you go much further"
    payoff of precision platformers, built around the *beat*. The endless path should reward flawless on-beat
    play with escalating scoring/reach, so mastery visibly pays off — easy to learn, hard to master.
  - **Procedural horde / leaderboard endless mode.** Carl explicitly likes a Diablo-style procedurally-generated
    horde mode with a world-record leaderboard for longevity. This is the *same* arcade mode already parked in
    "Later" (leaderboards, scoring pressure, ruthless difficulty) — treat it as one vision, not two: the
    endless mode IS the mastery ceiling above.
  - **Campaign / tutorial / endless coexist without being one mode.** Threaded, not merged: the hand-authored
    campaign (world-map nodes) is the "learn the game" path most players finish once and funnels into endless;
    the tutorial sandboxes teach individual verbs; the procedural endless/horde mode is the prestige game to
    master. Weave them so they hand off cleanly rather than duplicating each other.
  - **Explorable maps, not viewport-locked.** Unlike Vampire Survivors' single-screen lock, allow the player to move around the map — strategy becomes choosing *where* to position your train, not just *what* to catch. Inspired by older games like Asteroids where exploration and positioning matter. This changes pacing (less constant spawning in your face, more "where am I going next?") and opens room for terrain/hazard placement that doesn't spam you but invites routing decisions. Smaller viewport, larger playfield = risk/reward on where you farm vs. where you bank.
  - **Rewards for exploration.** Don't just make the map bigger — make it rewarding to venture to the edges. Scatter curated secrets/easter eggs (rare archetype encounters, hidden spawn patterns, seasonal events in specific biome corners) so that exploring feels like *discovery*, not just running away. Inspired by Black Isle games' exploration depth + Vampire Survivors' dopamine hits from finding weird things in far corners. The player should feel: "I went exploring and found this cool thing."
  - **Sequencing note.** The upgrade-frequency fix and the "rarer, bigger boundaries" step (both in "Now") are
    the safe, buildable slices of this now. The full endless/leaderboard rework stays deferred until the inner
    loop feels done (Carl's call) — don't promote it to "Now" ahead of that gate.

- **Playful bonus rounds** — Carl's Street Fighter II / Lion King (SNES) itch: a rare, surprising
  mini-challenge dropped into a run purely for spice (not for balance or progression) — a bonus
  catch-everything sprint, a rhythm-only gauntlet, something silly and short. Parked here rather
  than in "Now" since it's a side-system/breadth item by nature, same category as alternate game
  modes — worth revisiting once the core loop itself feels done.

- **NPC conga ecology (agar.io + Rain World) → multiplayer endgame.** Carl's vision: King Crabs
  have their own conga trains of followers; they steal crabs from the player's train and from each
  other. The beach becomes a living ecosystem of competing conga leaders, not just a static arena.
  The player starts as the smallest and must out-arrange (not just out-catch) larger NPC trains.
  Sequencing: (1) NPC conga trains for King Crabs; (2) train-stealing interactions between NPC
  and player trains; (3) ecology emerges from simple per-creature rules à la Rain World (see
  INSPIRATION.md); (4) multiplayer where human Rustlers compete for the largest train and thus
  the dominant audio share. Size is legible from across the field — a well-arranged shorter train
  should beat a larger, poorly-arranged one (arrangement depth matters more than raw length).

- **Spatialized audio + bring-your-own-music (competing DJ mode).** The dominant conga train's
  music takes up the bulk of the audio mix; smaller/losing trains fade. Each approaching NPC
  King Crab train is *heard* before seen — their music gets louder as they near, like agar.io
  circles creeping in from the edge. BPM detection already exists. In multiplayer: each player
  supplies their own track, the game syncs to it, and the mashup is a natural consequence of
  competition — the winner's music overwhelms the mix while losers' tracks fade to silence.
  Spatialized sound means audio IS the radar. Defer until NPC ecology is fun against bots first.

- **Environmental ambience — day/night cycles and weather.** Pure visual storytelling, no gameplay
  impact: time of day shifts sky/lighting (dawn → noon → dusk → night over ~5-10 min per run);
  weather layers add atmosphere (occasional rain, fog, clear skies, rare storms). The beach feels
  alive and inhabited rather than a static arena, matching the Control-inspired sense of arrival.
  No mechanical effects — rain doesn't slow you, fog doesn't hide enemies. This is aesthetic
  layering on top of the existing game, the way HYPER DEMON layers delirium visuals on top of core
  mechanics. Deferred until core loop feels complete and visual polish is the focus.
