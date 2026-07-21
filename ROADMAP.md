# Roadmap

High-level capabilities we believe would make Crab Rustler more fun, kept short and scannable.
Maintained by the Game Designer agent (see CLAUDE.md, Cron 6) — it reads Slack feedback on
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
one at a glance. **The read-check is now cleared:** the rumble pans left/right by the leader's bearing (#25) and each
rival wears a distinct, tier-styled name banner you can read across the field (#26) — so the ambient train reads *and*
sounds like a rival from a distance. **The steal itself has landed:** rivals thread your line and splice off
your back section (#32), the snap is telegraphed and lands on the beat (#34), and both directions are guarded by playtests
(`npc_steal` #28, `player_steal` #33) — the core train-vs-train verb the whole prototype was scaffolding toward is *in*. A
first slice of meta-progression + campaign scaffolding exists but stays parked in "Later" — the gate is Carl's explicit
"core feels done" call, which hasn't come.

**Signal (this cycle) — new human signal is in, via GitHub not Slack.** The #general channel is still only auto Dev Diary
posts (no reactions, no replies to weigh), but on **2026-07-20 Carl filed eight `gameplay` issues directly** — his clearest
direction in weeks, and it overrides the roadmap's own sequencing where it conflicts. Two themes dominate:
- **(a) The moment-to-moment beat is too hard and too opaque to *play*.** #164: the on-beat windows feel **unforgiving** and
  it's **not obvious what you're timing** (the keypress, or a later resolving event?) — the **clash** is the worst offender;
  the **dash feels good, don't touch it**. #165: explore a simpler input model ("always tap SPACE on the beat + tool chords").
  This is direct player-feel feedback and it **becomes the new Now headline** — a game that's frustrating and opaque to play
  in the moment blocks everything else.
- **(b) Make the campaign scaffolding actually function**, plus more depth/content: #182 per-level win conditions (nodes
  never unlock today), #183 biome-tinted world-map nodes, #176 skip-ahead on the map; #160 smarter/scarier Rain-World rival
  AI; #184 two new bosses.
**Shipped since last update.** The prior Now headline — reworking the upgrade screen from a world-freeze into a live real-time
overlay — **landed (#185)**. The defensive-steal parry got a wider on-beat window (**#190** — a first slice of #164). Release
**0.30.0** cut (#189). A recurring steal-playtest flake was made deterministic (**#194**, with #188 pending to formally close
#170). And **rival-vs-rival splicing is already in from earlier cycles (#144/#135)** — the ecology headline's first sub-step is
done, so its remaining work is the *smarter/scarier hunting AI* (#160) and legibility, not the splice verb itself.

## Bugs (fix before anything else in Now)

Stability beats new features — an agent picking a task should check here first, before any
item in "Now" below.

- **None open.** For the first time in a month the disabled-test crisis is fully cleared: `menu_to_game`
  (the core crab-catching verb) was re-enabled with a closed-loop autopilot (#20) and `campaign_tutorial`
  was fixed and re-enabled (#24). All three `run_script` lines in scripts/playtest.sh are live and the
  Playtest CI is green. **Keep it green — a red Playtest is an instant top-priority bug (AGENTS.md rule).**
  The recurring `steal_dodge`/`revenge` frame-rate flake (#170) got a deterministic fix (#194, merged); PR #188 is the
  dedicated close-out. That's a test-robustness flake, **not** a disabled test — every `run_script` line stays live.
- Fixed this cycle: the two disabled playtests above (#20, #24). Previously fixed: the flashlight/wgpu crash
  (draw-order fix, a375f52 / 53b23c3); the upgrade screen firing at the wrong time / popping back-to-back
  (c01b922, 3b17573); the start-of-run `InstanceArray capacity > 0` crash; the windowed-instead-of-fullscreen
  bug. If you hit a panic or a wrong-looking frame while testing, log it here before shipping anything new.

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

- **[★ NEW HEADLINE — PLAYABILITY: make the beat fair and legible before adding more systems].** Carl's freshest, strongest
  signal (2026-07-20): the on-beat timing feels **too unforgiving**, and it's **not obvious what you're supposed to time** —
  the keypress, or a later resolving event? The **clash** is the clearest offender. A game that's frustrating and opaque to
  *play in the moment* blocks everything below it, so this is the top priority this cycle — ahead of more ecology depth. Two
  sequenced slices, both Carl's own issues:
  1. **Relax + clarify the ambiguous on-beat windows (#164).** Widen/soften the windows for the affected mechanics (clash and
     friends) so a slightly-early/late press still reads on-beat, and **telegraph exactly what to hit** — a clear pre-beat cue
     so the player knows whether to press on the keypress or on the resolving event. Keep on-beat meaningful (perfect timing
     still pays more): this is forgiveness + legibility, not removing the skill. **The dash window feels good — do NOT touch
     it** (Carl explicit). The parry widening (#190) already shipped as a first slice; the clash is next.
  2. **Explore the simpler input model (#165) — only after #164 lands.** Prototype "always tap SPACE on the beat, sometimes
     flavor it with a tool chord (SPACE+R/T/E…)" as an *additive, opt-in, reversible* step — keep the existing controls
     working alongside. Goal: obvious to play while steering, deep to master. One coherent playtested slice; don't rip out
     working controls in one pass.
  Both must stay deterministic/headless-safe with all playtests green.

- **[★ CORE GAME — next depth step, after the playability pass] Make the beach a living *multi-train ecology*: rivals that
  hunt *each other*, not just you.** The steal fight against you is solid — deliberate rival routing (#67), on-beat
  parry/dodge/reroute defense (#72/#76/#80/#83), recoverable-bite + revenge-duel tuning (#69/#74), both directions playtested —
  and **rival-vs-rival splicing already landed (#144/#135)**. The thesis (agar.io + Rain World) is a *whole beach* of competing
  conga leaders, so the remaining depth is making those rivals *smart* about the splice they can already do. Still one small
  playtested slice at a time, reusing the splice verb — no new verb:
  1. **Rival-vs-rival splicing — ✅ LANDED (#144/#135).** Two crossing King Crab trains already exchange crabs on the beat,
     with a hunt telegraph (#135) so you can read the clash and swoop the spoils. The splice verb is multi-train; the work
     below is making the rivals *smarter* about using it, not re-doing the verb.
  2. **A deliberate urge to hunt the weaker train (Rain World, not a path-planner).** Give each rival the same simple
     per-creature intent it already uses to thread *your* line (#67), pointed at the nearest *smaller* rival train — so the
     big trains visibly bully the small ones and the pecking order emerges from local rules, agar.io-style. Keep it a cheap
     urge, not global planning. **Carl's #160 (smarter, scarier Rain-World rival AI) is the open issue for this slice.**
  3. **Make it legible and swoopable — the player reads the fight and profits.** The contest has to be *watchable*: a rival
     that just grew from a steal should read as bigger (size/banner already tier-scale — lean on that), and the loser's
     scattered crabs should be catchable so the player can swoop into a rival-vs-rival collision and rustle the spoils.
     That's the agar.io "let the big ones fight, then eat the crumbs" play — pure skill expression, still on the beat.
  4. **Guard it with bots.** Extend `npc_steal` (or add an `npc_vs_npc` scenario) so two NPC trains provably exchange crabs
     and the ecology can't silently break. Tune counts/cooldowns so the beach churns without spiralling to one mega-train.
  This beats the polish lanes below. Keep each step small, safe, and green — overnight nobody's watching, so lean on the
  playtests and prefer the smaller reversible change.

- **✅ Reworked the upgrade screen from a world-freeze into a live real-time overlay (shipped #185).** The cards now render
  as a live overlay while the world keeps running, instead of a shared-world pause — closes the feel/rhythm/multiplayer
  problems the old `pending_upgrade` early-return guard caused. Kept here checked off for one cycle so the win is visible.

- **[Next inner-loop target — after the playability pass + ecology land] Make dash + parry/block feel like the primary rhythm of play, not supplementary mechanics.** Inspired by Darktide: in that game dodge and block aren't reactive afterthoughts — they're the *constant beat* you play to, timed and deliberate. In Crab Rustler the equivalent is: Groove Dash isn't a bonus you cash in when the meter fills — it's *how you move to crabs*, a rhythmic stride you'd naturally do every few beats to close distance and reposition. Parry isn't a defensive option when a rival attacks — it's the counterbeat you're always ready to land, a telegraph window that pulses on the downbeat when rivals are near, a satisfying drum-hit when you time it, opening the counter-steal. The target feel: **dash → catch → dash → parry → counter-steal → repeat**, each hit a drum pad, the whole inner loop playing like a groove you're locked into rather than a set of tools you pick from a menu.
  Concretely: (1) make the Groove Dash available and encouraged more freely — consider a per-beat cooldown rather than meter-gated, so the player is naturally dashing on every other downbeat; (2) give the parry a visible rhythmic telegraph that appears when a rival is threatening (a pulsing ring on the beat, like the steal-arm ring already exists — let the parry window pulse *with* the BPM so the player learns to watch for it); (3) tune the counter-steal window so a clean parry → counter-steal feels as satisfying as a Doom Eternal glory kill — brief, visceral, clearly rewarding the timing. Nothing here is a new verb; it's all existing mechanics elevated to first-class rhythm status. Sequence: don't start until the ecology work (rival-vs-rival) is solid; these are meaningless without rivals pressuring you.

- **Sharpen archetype-tool matchups into a readable soft RPS system.** *(Polish lane — do the ecology slice above first.)*
  *Momentum is real:* six pairs
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
  **Exception — Carl filed concrete slices that make the *existing* skeleton actually function (2026-07-20):** #182 per-level
  win conditions (nodes never unlock today — `complete_selected()` is never called), #176 skip-ahead on the map (locked nodes
  are un-navigable, blocking playtesting), #183 biome-tinted node visuals. These are *fixes to a half-built system*, buildable
  now on Carl's direct ask — distinct from the broad campaign build-out above, which stays gated. Do #182 first (it's the one
  that makes the campaign traversable at all); keep the playability-pass headline ahead of all three.

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
  (6a17026), legible in tiers (d046ae7), and read-check cleared — pan (#25) + banner (#26) landed**; (2) train-vs-train
  stealing (the reverse-Snake crossing rule in INSPIRATION) — **✅ SHIPPED and deepened into a skill-based fight: splice
  verb (#32/#34), deliberate rival routing (#67), on-beat parry/dodge/reroute defense (#72/#76/#80/#83), recoverable-bite +
  revenge-duel tuning (#69/#74), both directions playtested**; (3) ecology from simple per-creature rules à la Rain World —
  **rival-vs-rival splicing has landed (#144/#135); the remaining depth is a hunt-the-weaker urge / smarter-scarier rival AI
  (#160), turning the two-body duel into a whole-beach ecosystem the player can swoop into** (the second Now item, sequenced
  *after* the playability pass); (4) human-vs-human Rustlers competing for the largest, best-arranged train.
  Steps (1), (2), and the splice half of (3) have landed; making the rivals *smart* about it (#160) is the next core-game
  depth step, behind the playability headline.

- **★ THE CORE GAME — BYO-music mashup + spatialized audio.** The other half of the thesis, inseparable from
  the ecology above: the *dominant* train's music dominates the mix, losing trains fade. Each rival train is
  **heard before seen** — its track swells as it nears, like an agar.io circle creeping in from the edge, so
  audio IS the radar. Everyone brings their own track (BYO music); the engine syncs and *mashes them up*, and
  the mashup is a natural consequence of the fight — the winner's song overwhelms, the losers' fade to silence.
  King Crab NPCs carry tracks too for solo play / practice. BPM detection already exists. Defer wiring until
  the NPC ecology is fun against bots first; but this is the whole point of the game, not a stretch goal.
