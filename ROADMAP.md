# Roadmap

High-level capabilities we believe would make Crab Rustler more fun, kept short and scannable.
Maintained by the Game Director agent (see CLAUDE.md, Cron 6) — it reads Slack feedback on
releases and updates this list accordingly. Feature Developer and Overnight Developer read
this for direction before picking their next task; they don't edit it themselves.

**The thesis (Carl, 2026-07-16).** The real game is *competing conga lines*: crab leaders (NPC King
Crabs, and eventually human Rustlers) steal each other's crabs to grow their own train, and whoever's
train dominates dominates the **music mix** — everyone brings their own track (BYO music) and the engine
mashes them into one musical brawl (Crab Rave meme: more crabs in sync = more music; Rustler theme:
stealing dancing crabs). This is the destination, not a side feature. The current single-player arena is
**prototype scaffolding** toward it — a place to make catching/arranging/rhythm feel great before rivals
show up to steal from you.

**Sequencing.** The path to the thesis, in order — don't jump ahead while an earlier item remains:

1. **Now — make the inner loop excellent, then open the world.** A single train's catching, arranging,
   and rhythm must feel great before rivals arrive. Then the scrolling world (top of Now) is the
   architectural gate that lets rival trains exist at all.
2. **Then — the competing-conga ecology + BYO-music mashup.** The core game above: NPC King Crab trains
   that steal crabs, spatialized audio where the winning train's music dominates the mix, and ultimately
   human-vs-human. Currently sharpened in "Also on our mind" — gated behind the scrolling world, promote
   into Now once that lands.
3. **Later — the outer loop.** Separate from the thesis: meta-progression, unlocks, campaign/story —
   sustaining motivation across runs. Held until the inner loop feels done (Carl's call).

**Where we are.** The inner loop is deep and closed: four catching tools (beam/whistle/stomp plus a
lasso now reworked into a charged skill-shot — hold to wind up, release to throw an arc that snags and
drags a cluster) with upgrade lanes, a conga train with a chain-snap downside and a delivery-pen jackpot,
and rhythm/groove scoring
that **drives real mechanics** (downbeat spawn drops, on-beat PERFECT hits, a beat-stepping train, Groove
Dash/Call/Gamble/Slam, a charged Drum Roll, a catch bloom). Pacing ramps through named intensity stages.
**Eight archetypes** form a dense emergent web with visible catch-time crossovers — the signature fun.
**All three train slots now carry weight**: head figureheads, tail placement, and (new this cycle) a
mid-train **CENTERPIECE** that pays a scaling bonus for a deep same-type run straddling the midpoint.
The **bank-now-vs-push-luck** axis is closed (live AT RISK readout, escalating snap teeth, superlinear
`pen_worth = (n·(n+1)/2)·3` bank payoff, BIG/LONG/GRAND HAUL cashout). Biomes each carry a distinct,
telegraphed terrain hazard, and all three bosses fight inside the archetype web. A King Crab *direct hit*
now scatters your entire conga line into catchable crabs (Sonic-rings burst) — a first taste of the
steal-and-recover tension the ecology will run on. The four-scenario opt-in tutorials have been folded into
the first world-map nodes (removed from the main menu) and still double as regression tests. The beach is now a
**scrolling world** (2× viewport, player-following camera) carrying a **day/night cycle + weather** (sunny → storm,
ambient visuals) — the "world feels alive and inhabited" aesthetic layer is in, now with **density tuned for the
larger field** (~1.8× spawn counts, 40e2455) so it reads as inhabited rather than sparse, and a **three-zone
environment** (grass / beach / water, 8a8145b) carrying procedural terrain texture — tufts, pebbles, animated water
ripples/foam, feathered transitions, batched into three instanced draws (ae95f50). Music got a real pass too: a
**generative groove engine** drives the action music (2486e58) over rewritten Game Boy / Deus Ex two-voice arpeggio
themes (844010a) — early scaffolding toward the BYO-music mashup. Legibility got a pass:
a **Zelda-style 5-slot tool-roster HUD** with cooldowns (4dbfd84), a **minimap + day/night + weather indicators**
(467655a). **The first ecology slice has landed**: an ambient wandering NPC King Crab conga line (6a17026) that
trails followers and roams the world on its own, *heard before seen* via a spatial-audio rumble that swells as it
nears (2200964, agar.io-style), with randomly-generated names (38201e5) and now **three visually distinct tiers**
(scout/wanderer/elder — size, speed, territory, idle pauses, d046ae7) so a small train reads differently from a huge
one at a glance. Visual-only — it doesn't yet steal or react. A first slice of meta-progression +
campaign scaffolding exists but stays parked in "Later" — the gate is Carl's explicit "core feels done" call, which hasn't come.

**Signal (this cycle).** Still no new human signal on Slack — every post in #general is an auto Dev Diary,
no replies, no reactions to weigh; the one standing ask (Carl, 2026-07-07: "would be nice to see example videos
here") is a Dev Diary *format* request, not a roadmap item, and belongs to the diary agent. This was a **polish/audio
cycle that inched the ecology forward but left the core-verb regression untouched.** The landings: the groove engine
grew a real rhythm bed — **kick/snare drums + a walking bass** (7598b14) on top of the electric-piano lead
(c80c96a) — more BYO-music scaffolding. The ambient rival train got two concrete nudges toward its read-check: the
**flashlight now targets NPC train leaders, not just boss crabs** (28452dc) — the first time a player *tool* reaches a
rival leader at all — and its **rumble was tuned to snap to one bar at the game BPM with event density halved for a
calmer, more musical swell** (e571ce1, plus an NDC coord fix + A-minor key, 5b9b3ee). Level transitions got the
Control-style **slide-in title cards** Carl likes (cd0cc39), and the HUD tightened (1cce79d). CI moved off Nix to a
cargo+apt path (#16/#17) and the mouse cursor is hidden in-window (6bef4f8). **The down side is unchanged and now
glaring:** the two playtests disabled in 477f7e6 — `menu_to_game` (a **crab-catching** regression, the core verb) and
`campaign_tutorial` — are *still commented out*, ~14 commits later. Per the Supervisor's ruling (621d07e) a disabled
test *is* a FAIL; agents keep choosing softer audio/HUD work over it. It beats everything until green (top of Bugs).
**The ecology read-check is now half-cleared:** the music-swell radar's *smooth distance swell* is in and tuned
(e571ce1) — but it's still **mono**: no directional stereo pan, so you hear the train approach without hearing *which
way* it is. The boss already has the pan-by-angle + rolloff machinery (2101cef); porting it onto the ambient train's
rumble is the one remaining audio task for the radar, alongside the distinct name banner — and *it still has not been
playtested in motion*. The **core steal rule** stays parked in "Also on our mind" (reverse-Snake crossing in
INSPIRATION) until that read-check passes. Carl's mechanics-freeze is **lifted** (2026-07-16) but its spirit holds:
sharpen/distinguish/interact, don't bolt on a pile of new player verbs. No new Now items this run — fix the
disabled-test bugs first, then finish the radar's directional pan.

## Bugs (fix before anything else in Now)

Stability beats new features — an agent picking a task should check here first, before any
item in "Now" below.

- **[TOP BUG — a full feature cycle has passed without a fix] `menu_to_game` playtest is disabled to hide a
  crab-catching regression.** `scripts/playtest.sh` line 48 has `run_script menu_to_game` commented out "pending
  crab catching fix" (477f7e6). Catching is the *core verb* — a masked regression here is the worst kind, and
  ~14 commits of audio/HUD polish have landed on top of it without touching it. **Feature/Overnight agents keep
  bouncing off this into softer work — stop.** Fix the underlying catch detection until the test passes, then
  re-enable the line. Never leave it commented as a workaround (Supervisor ruling, 621d07e). This beats every
  feature and every ecology item below until it is green.
- **[BUG] `campaign_tutorial` playtest is disabled pending a tutorial→world-map bug.** Same file, line 49,
  commented "enable once tutorial->world-map bug is fixed." Re-enable and fix once the crab-catching bug above
  is cleared (they may share a root cause in the menu/level transition).
- Fixed this cycle: the flashlight/wgpu crash (draw-order fix — flashlight drawn last, after all instanced
  meshes, a375f52 / 53b23c3). Previously fixed: upgrade screen fired at the wrong time / popped back-to-back
  (c01b922 loops the threshold past the current banked score, 3b17573 fires the check at the pen); the
  start-of-run `InstanceArray capacity > 0` crash; the windowed-instead-of-fullscreen bug. If you hit a panic
  or a wrong-looking frame while testing, log it here before shipping anything new.

## Now

- **Direction (Carl, 2026-07-16): sharpen, distinguish, interact — and keep everything rhythmic.** Mechanics
  freeze is lifted. Don't add a bunch of new stuff — but DO make everything visually distinct, legible, and
  rich with interaction effects. Think Doom Eternal's soft rock/paper/scissors: each archetype has a clear
  *role*, each tool a clear *strength against certain targets*, every meaningful interaction *shows* that it
  happened. The player should read the field and make smart decisions, not learn through opaque trial and error.
  **Crucially: everything must fit the rhythm-game flavor.** Tool throws, interaction effects, boss burns,
  lasso snags — all should have beat-synced bonuses (on-beat throws go further/faster/stronger, downbeats
  trigger bigger effects). The beat is the mechanic; new polish deepens it, doesn't work around it.
  The ideal player feel: hammering keys like drum pads to their own music, crabs caught as the *consequence*
  of playing the groove well. Each tool key is a drum pad. Ask of every mechanic: "does hitting this on the
  beat feel like a satisfying drum hit? Does the downbeat version feel like a fill?"

- **[TOP PRIORITY] Sharpen archetype-tool matchups into a readable soft RPS system.** *Momentum is real:* six pairs
  now draw their moment — the three flagship strong-matches beam/Hermit, stomp/Dancer, lasso/Thief (e819849), plus
  Magnet-vs-herd-cluster (01b8573) and lasso-vs-Magnet (b35db97), plus the first *negative* tell: a grey-steel
  ricochet when the lasso slips off a shelled crab, so "wrong tool" reads as clearly as a strong match (01c7877).
  That's the pattern proven in both directions; keep extending it. Still-unread pairs to pick up next: whistle vs
  Dancer, stomp vs Armored, beam vs fast/Golden, and the rest of the 8×4 web that's still implied — each wants a
  brief, distinctive, beat-synced tell. Each archetype should telegraph its role with a clear visual identity, and
  each tool should feel like it has a *purpose* on the field beyond "catch things." See INSPIRATION.md Doom Eternal
  note. Keep the six shipped tells sharp — don't regress them while adding new ones.

- **Interaction effects: make every meaningful event read clearly.** Catch-time crossovers exist (Dancer
  trips Goldens, Magnets supercharge on Golden-catch) but the visual feedback is sparse. Add small but
  distinct effect bursts for: archetype-tool strong matches, chain crossover triggers, bond-forming catches,
  boss phase transitions. Each effect should be *brief, distinctive, and informative* — the player learns
  the system by watching it, not by reading a tutorial.

- **[ECOLOGY — validate the first slice] Make the ambient King Crab train read as a genuine rival.** The ambient
  wandering train (6a17026 + spatial rumble 2200964 + names 38201e5) reads in **three visually distinct tiers** —
  scout/wanderer/elder differ in size, speed, territory, and idle pauses (d046ae7) — and the flashlight now *targets*
  its leader (28452dc), the first player-tool contact with a rival. The music-swell radar is **half done**: the
  distance swell is smooth and tuned (e571ce1), but the rumble is **still mono** — you hear the train approach without
  hearing *which way* it is. Two concrete tasks remain, no new player verb:
  1. **Directional pan (the one remaining audio task).** Port the boss's stereo-pan-by-angle + rolloff (2101cef) onto
     the ambient train's rumble so it pans left/right by the leader's bearing, agar.io-style. Distance swell is
     already there; this adds the *direction*.
  2. **Distinct name banner.** A larger, distance-scaled-alpha name label you can read across the field to tell
     rivals apart.
  Then **playtest it in motion** — nobody has yet confirmed the tiers and rumble actually read while moving. Still
  visual-only: does NOT steal, splice, or react to you. Passing this read-check unblocks the steal rule below.

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
  campaign funnels players into arcade, arcade is the prestige path.
  **Tutorials are folded into the first world map nodes — not a separate menu (shipped: eb86756).** The
  opening nodes are short, hand-crafted mechanic introductions (catch a chain → feel the beat → one tool
  per node → first rival train). The "How to Play" menu item is gone; the world map IS the tutorial funnel,
  and the TutorialKind scenarios now live as the first world-map levels rather than menu sandboxes.
  Players who want to skip go straight to arcade. Don't start this until the
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

- **★ THE CORE GAME — competing conga ecology (agar.io + Rain World).** This is the destination the
  whole prototype is scaffolding toward (see thesis at top). King Crabs run their own conga trains and
  *steal* crabs from the player's train and each other; the beach is a living ecosystem of rival conga
  leaders. The player starts smallest and must **out-arrange, not just out-catch** — a well-arranged short
  train should beat a larger, sloppy one. Sequencing: (1) NPC conga trains for King Crabs — **✅ ambient slice shipped
  (6a17026), now legible in tiers (d046ae7) and being validated in Now**; (2) train-vs-train stealing (the reverse-Snake crossing rule in INSPIRATION)
  — **next up, promote to Now once the ambient train passes its read-check**; (3) ecology from simple per-creature rules
  à la Rain World (see INSPIRATION.md); (4) human-vs-human Rustlers competing for the largest, best-arranged train.
  The scrolling-world gate has landed; step (2) is now gated only on the ambient train reading right first.

- **★ THE CORE GAME — BYO-music mashup + spatialized audio.** The other half of the thesis, inseparable from
  the ecology above: the *dominant* train's music dominates the mix, losing trains fade. Each rival train is
  **heard before seen** — its track swells as it nears, like an agar.io circle creeping in from the edge, so
  audio IS the radar. Everyone brings their own track (BYO music); the engine syncs and *mashes them up*, and
  the mashup is a natural consequence of the fight — the winner's song overwhelms, the losers' fade to silence.
  King Crab NPCs carry tracks too for solo play / practice. BPM detection already exists. Defer wiring until
  the NPC ecology is fun against bots first; but this is the whole point of the game, not a stretch goal.
