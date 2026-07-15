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

**Where we are.** The inner loop is deep, closed, with a spine, a real climax, and genuine routing hazards.
Four catching tools (beam/lasso/whistle/stomp) with upgrade lanes; a conga train with a chain-snap downside
and a delivery-pen jackpot; rhythm/groove scoring that **drives real mechanics** (downbeat spawn drops,
on-beat PERFECT hits, a beat-stepping train, an on-beat Call, a Downbeat Slam, a Groove Gamble cash-out, a
charged Drum Roll blast, an audible kick drum, a **Groove Dash**, a **downbeat herd pulse** that clumps free
crabs toward you, and an **on-beat catch bloom** that widens the catch window on the beat). Pacing ramps
through named intensity stages with a beat-tempo shift.

**Eight archetypes** (base, Armored → stomp, Dancer → rhythm, Magnet → routing, Thief → chain pressure,
Golden → chase decision, Hermit → uncrackable shell, Splitter → shape-bet) form a dense **emergent web** —
the signature fun. Catch-time crossovers fire visibly (Dancer hops chip shells and trip Goldens, snared
Goldens supercharge Magnets, shine lures Thieves off your tail, Golden→Magnet-tail arcs a shine cascade),
and both train slots now carry weight: **head** figureheads (Golden boosts match-run bonuses, Dancer
Drum-Major pumps groove economy) and **tail** placement (Armored parked at the tail tanks a Thief steal).
The **bank-now-vs-push-luck** axis is now fully legible on both sides: a live AT RISK readout, snap teeth that
escalate with length, and a superlinear triangular bank payoff with a BIG/LONG/GRAND HAUL cashout.

Biomes **push the herd** (Rocky Shore tide-shortcuts, Neon Kelp funnel lanes, chokepoints/tail-snag/wade-drag).
**All three bosses fight inside the archetype web** (King Crab bait-into-Armored, Tide Boss Golden-slingshot,
Reef DJ call-and-response), and rhythm verbs reach the climax (a charged Drum Roll cracks a boss shell fast).
The opt-in **How to Play** tutorial ships four scenarios, each a pure headless pass predicate doubling as a
regression test. A first slice of **meta-progression** + campaign scaffolding exists but stays parked in
"Later" — the gate is Carl's explicit "the core feels done" call, which hasn't come.

**Signal.** No new Slack reactions/replies this cycle — Slack was unreadable this run and the recent Dev Diary
posts are unreacted, so direction holds from Carl's last substantive playtest: (1) upgrade screen fired too often
and back-to-back and read as a flow-breaking *pause* — **fully resolved**: frequency cut (39daa76), timing bugs
fixed (c01b922/3b17573), and the redesign into a meaningful 3-pick choice with tradeoffs **SHIPPED** (b43045c);
(2) level title cards look cool (Control aesthetic) but levels don't vary or feel impactful — boundaries-rarer-
and-bigger **first pass SHIPPED** (longer levels + per-biome archetype emphasis + threat banner, 22caa05/f83c755;
**terrain-hazard half still open** — top live Now item); (3) he wants a procedural horde / world-record
leaderboard endless mode (Diablo-style) with a beat-mastery skill ceiling — parked in "Later", sharpened; a first
slice of that skill ceiling landed as the **super-linear PERFECT streak** payoff (043a480). **The risk/reward
axis is closed** — live AT RISK readout, snap teeth escalating by length, and a superlinear triangular bank payoff
(`pen_worth = (n·(n+1)/2)·3`) with a BIG/LONG/GRAND HAUL cashout. Treat as closed unless Carl says it feels flat.

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
  Now item below (biome terrain hazard, train-middle arrangement) survives the freeze: each *deepens and polishes
  existing* mechanics rather than adding new ones — the arrangement item reshapes the conga train that already
  exists, it doesn't add a new system — which is exactly the work Carl is asking for. Deepen and polish what
  exists; make the player feel in control of it. *(Two Now items shipped this cycle and were checked off: the
  upgrade-screen redesign into a 3-pick meaningful choice — b43045c — and the super-linear PERFECT streak payoff
  as a first legible skill ceiling — 043a480.)*

- **[TOP PRIORITY] Pair each biome with its own terrain hazard — finish the gear-change. (Next step of the level rethink.)**
  Boundaries now shift *who you catch* (archetype emphasis) but the ground is still mostly a tint. Per
  INSPIRATION's Control note, a boundary should read as arriving somewhere mechanically: give each biome a
  distinct terrain mechanic that changes routing (some already exist — Rocky Shore tide-shortcuts, Neon Kelp
  funnel lanes — so extend/assign the pattern per biome) so archetype shift + terrain hazard land *together*.
  Depth, not breadth: reuse the existing hazard systems rather than inventing parallel ones. Still NOT the
  endless/procedural/leaderboard rework — that stays gated in "Later" until Carl calls the inner loop done.
- **Give the middle of the train arrangement weight — the inner loop's next frontier.** Right now only the
  **head** (Golden figurehead, Dancer Drum-Major) and **tail** (Armored tail-guard) slots carry meaning; every
  crab between them is just a number for pen_worth. Make a longer train mean more *arrangement* decisions, not
  just more banking value — reuse the existing adjacency/sandwich/figurehead systems so that where a crab sits in
  the line matters (mid-train combos, run-length synergies, positional archetype interactions). This deepens the
  existing chain mechanic (freeze-safe — no new archetype or verb) AND sharpens Carl's hold-vs-bank tension:
  holding long becomes a puzzle to *set up*, not only a risk to carry.

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
