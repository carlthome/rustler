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

**Where we are.** The inner loop is deep and closed: four catching tools (beam/lasso/whistle/
stomp) with four upgrade playstyle lanes, the conga train with chain-snap downside and a
delivery-pen jackpot, rhythm/groove scoring that now **drives real mechanics** (bar-quantized
spawn drops on the downbeat, on-beat PERFECT tool hits, a beat-stepping train, an on-beat Call
that Dancers answer for a Dance Catch bonus), biomes with terrain hazards, and three enemy
archetypes beyond the base crab (Armored → stomp, Dancer → rhythm) plus **two bosses** (King
Crab charge, Tide Boss shockwave that alternate). A first slice of **meta-progression has
shipped too**: runs bank into a persistent career (best score, lifetime crabs, run count) and
banked crabs buy permanent starting tool ranks in a title-screen perk shop, so a loss still buys
progress. The prior "Now" boss/rhythm/upgrade-UI items are all done. We're NOT declaring the
inner loop finished and pivoting wholesale to outer-loop work until Carl says the core feels
done — fresh archetypes and bosses land well, but **pacing** is still flat (static per-zone
difficulty) and biomes are still only color-grading skins. So this run keeps depth-first targets
in Now and leaves the meta-progression expansion in Later.

## Now

- **Staged difficulty ramp with special spikes** — difficulty is currently static per zone (fixed
  values in levels.rs); a run has no rising tension arc within a zone. Make it escalate in stages
  over elapsed time, with occasional standout moments (a tougher wave, a denser spawn, a beat-tempo
  shift) that feel special and earned rather than a flat curve. Deepens pacing of the existing core
  loop without adding a new system. **Highest-leverage Now item — it's the missing spine of a run.**
- **Make biomes matter mechanically, not just visually** — the four biomes (Sunny Meadow, Tide
  Pools, Rocky Shore, Neon Kelp) currently differ only in color grading and beat-pulse tint. Give
  each one a distinct gameplay wrinkle that changes how you catch or route (e.g. Neon Kelp fronds
  that snag the conga tail, Rocky Shore chokepoints, a Tide Pool that periodically floods the
  whole lane on the bar), so moving between zones feels like a real change of terrain, not a
  reskin. Depth: turns existing scenery into a system.
- **A rhythm-native player ability with real teeth** — the on-beat Call + Dance Catch loop proved
  players enjoy actively *playing* the rhythm, not just watching it. Build on it with a chargeable
  on-beat power: e.g. a "Downbeat Slam" that, timed to a PERFECT, yanks and banks a cluster at
  once, or a groove-meter ultimate that only fires clean on the bar. Rewards rhythm mastery with a
  spectacle payoff — leans into the game's most distinctive hook.

## Later (outer loop — not yet)

- **Expand meta-progression past the first slice** — the persistent career + perk shop is in.
  Once Carl signals the inner loop feels done, grow it: more permanent unlocks (a new crab
  archetype, a cosmetic, a starting biome), a run-history readout, small run-to-run goals. Keep
  it a single save file, not a sprawling meta-tree. Deliberately held here so depth-first inner-
  loop work stays first.

## Blocked (needs a human, not a code agent)

- **The soundtrack builds with the groove** — the `layer{1,2,3}.ogg` progressive-fade hook exists in
  code (main.rs loads them at startup) but no audio files populate it, so it's inert. This needs
  someone to actually author/source three stacking music layers and drop them in `resources/`; a
  headless dev agent can't compose them. Wiring them to the Groove meter once they exist is trivial.
  Parked here so feature agents stop bouncing off it — pick it up when Carl provides the stems.

## Also on our mind (not sequenced — no urgency, just don't lose it)

- **Emergent system interactions** — Carl's Noita-inspired itch: the fun isn't a full physics/
  material simulation (too big a rearchitecture for this game), it's letting the systems we
  already have actually affect each other instead of running in isolation. Shipped so far:
  beat-startle chain reactions (panic ripples crab-to-crab on each beat), chain-snap risk (a
  panicking crab that hits the tail knocks the last links loose), the conga body walling off /
  deflecting fleeing crabs back toward the beam, the whistle soothe calming a panic and
  granting startle immunity, and lasso catches now spooking the surrounding herd like beam/chain
  catches do. Still parked here until one earns its way up: fear rippling into new panic archetypes
  (a Dancer's on-beat hop startling neighbors), and chain segments redirecting fleeing crabs into
  each other. Same depth-first spirit — deepen what's there before going wide.
- **Playful bonus rounds** — Carl's Street Fighter II / Lion King (SNES) itch: a rare, surprising
  mini-challenge dropped into a run purely for spice (not for balance or progression) — a bonus
  catch-everything sprint, a rhythm-only gauntlet, something silly and short. Parked here rather
  than in "Now" since it's a side-system/breadth item by nature, same category as alternate game
  modes — worth revisiting once the core loop itself feels done.
