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

**Where we are.** The inner loop is deep, closed, and has a spine, a real climax, and genuine routing
hazards. Four catching tools (beam/lasso/whistle/stomp) with upgrade lanes; a conga train with a
chain-snap downside and a delivery-pen jackpot; rhythm/groove scoring that **drives real mechanics**
(downbeat spawn drops, on-beat PERFECT hits, a beat-stepping train, an on-beat Call, a full-meter
Downbeat Slam, a Groove Gamble cash-out, a charged Drum Roll blast, an audible kick drum on every beat,
and a **Groove Dash** that sweeps free crabs when timed to the downbeat). Pacing ramps through named
intensity stages with a beat-tempo shift.

**Seven archetypes** (base, Armored → stomp, Dancer → rhythm, Magnet → routing, Thief → chain pressure,
Golden → chase decision, Hermit → uncrackable shell) form a dense **emergent web** — the signature fun.
Catch-time crossovers now fire visibly: Dancer hops chip shells and trip Goldens, snared Goldens
supercharge Magnets, shine lures Thieves off your tail, a caught Dancer-link pulses a small on-beat
catch aura, and catching a Golden onto a Magnet-link tail arcs a **shine cascade** down the whole train.

Biomes **push the herd, not just recolor it** (Rocky Shore tide-shortcuts, Neon Kelp funnel lanes,
chokepoints/tail-snag/wade-drag). **All three bosses fight inside the archetype web** (King Crab charge
you bait into a parked Armored, Tide Boss Golden-slingshot, Reef DJ call-and-response), and the player's
rhythm verbs reach the climax (a charged Drum Roll cracks a boss shell fast). The opt-in **How to Play**
tutorial ships **four scenarios** (beat-timing, chain-and-deliver, shell-crack, lasso), each with a pure
headless pass predicate that doubles as an agent-run regression test. A first slice of **meta-progression**
+ campaign scaffolding exists (persistent career + perk shop, world-map + player-skin skeletons) but stays
parked in "Later" — the skeleton existing doesn't authorize promoting it; the gate is Carl's explicit
"the core feels done" call, which hasn't come.

**Signal.** No new Slack reactions/replies this cycle; Carl's one standing note is *"Would be nice to see
example videos here!"* (a diary-agent task) — it confirms the rhythm/visual-spectacle bet is what he wants
to *watch*, so keep favoring legible, watchable moments. **Both prior "Now" items shipped** on the
catch-time/adjacency side (shine cascade, Dancer aura) and the moment-to-moment side (Groove Dash). The
frontier stays depth-first: the *reactions* between adjacent links now fire, but the train is still just a
line you grow — **make its shape and catch order a live spatial decision**, and keep deepening how the beat
reshapes ordinary play.

## Bugs (fix before anything else in Now)

Stability beats new features — an agent picking a task should check here first, before any
item in "Now" below.

- None currently known. (The start-of-run `InstanceArray capacity > 0` crash and the
  windowed-instead-of-fullscreen bug are both fixed.) If you hit a panic or a wrong-looking
  frame while testing, log it here before shipping anything new.

## Now

- **Make the train's shape and catch order a live spatial decision. (Top Now item.)** Adjacent-link
  *reactions* now fire (shine cascade, Dancer aura) — done. But the train is still a line you only ever
  *grow*: **where** a link sits and **what order** you caught things in barely matter. Add one mechanic
  that turns arranging the conga into a puzzle the archetype web feeds into — e.g. an **Armored link at
  the tail that actually tanks a Thief's steal** (so you deliberately park armor at the back instead of
  banking it), a **Splitter** archetype that halves your train into two on catch (grabbing it mid-combo
  becomes a Groove-Gamble-style bet), or a **link-adjacency bonus** where same-type neighbors stack a
  small escalating effect (so you catch to *build a pattern*, not just a length). Reuse existing verbs;
  keep it a legible, watchable arrangement the player learns to set up on purpose.
- **Deepen how the beat reshapes ordinary play — one more moment-to-moment rhythm read.** Groove Dash
  put the beat into movement; build on that so a groove-savvy player routes the *whole* run to the bar,
  not just their catches. Add one field-level rhythm tool distinct from the Dash — e.g. a self-triggered
  **beat-phrase call-and-response** in the open field (echo a short pattern for a herd-wide lure, like the
  Reef DJ's but player-initiated), **free crabs that visibly clump toward you on the downbeat** so the
  beat itself becomes a routing tool, or an **on-beat window that briefly widens catch radius** so timing
  your grabs to the bar changes herd management, not just score. Keep it legible — the beat visibly
  reshaping the field.

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

## Blocked (needs a human, not a code agent)

- **The soundtrack builds with the groove** — the `layer{1,2,3}.ogg` progressive-fade hook exists in
  code (main.rs loads them at startup) but no audio files populate it, so it's inert. This needs
  someone to actually author/source three stacking music layers and drop them in `resources/`; a
  headless dev agent can't compose them. Wiring them to the Groove meter once they exist is trivial.
  Parked here so feature agents stop bouncing off it — pick it up when Carl provides the stems.

## Also on our mind (not sequenced — no urgency, just don't lose it)

- **Playful bonus rounds** — Carl's Street Fighter II / Lion King (SNES) itch: a rare, surprising
  mini-challenge dropped into a run purely for spice (not for balance or progression) — a bonus
  catch-everything sprint, a rhythm-only gauntlet, something silly and short. Parked here rather
  than in "Now" since it's a side-system/breadth item by nature, same category as alternate game
  modes — worth revisiting once the core loop itself feels done.
