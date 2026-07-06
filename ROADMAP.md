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
spawn drops on the downbeat, on-beat PERFECT tool hits, a beat-stepping train), biomes with
terrain hazards, and two enemy archetypes beyond the base crab (Armored → stomp, Dancer →
rhythm). A first slice of **meta-progression has shipped too**: runs bank into a persistent
career (best score, lifetime crabs, run count) and banked crabs buy permanent starting tool
ranks in a title-screen perk shop, so a loss still buys progress. Both prior "Now" items are
done. We're NOT declaring the inner loop finished and pivoting wholesale to outer-loop work
until Carl says the core feels done — the Dancer crab shows fresh archetypes land well, and
boss/biome variety is still thin. So this run keeps depth-first targets in Now and leaves the
meta-progression expansion in Later where it can grow once Carl weighs in.

## Now

- **A second boss, not just a second King Crab** — the King Crab charge is the game's only
  climax beat and it's a single pattern. Add one more boss archetype with a distinct threat and
  counter-play (e.g. a Hermit Crab that hides in a shell you must beat-crack, or a Tide Boss that
  floods lanes and forces routing), so the run has more than one memorable spike. Depth: makes
  every long run's peak moment less repetitive.
- **Deepen the Dancer/rhythm enemy line into an ability, not just a foe** — the Dancer crab that
  freezes off-beat and hops on-beat is the most rhythm-native thing in the game. Push it further
  into player-facing mechanics: a catch window that only opens on the beat for certain crabs, or
  a "call" the player issues on-beat that a Dancer answers, so the rhythm is something the player
  actively plays with, not just watches. Doubles down on what's most distinctive about this game.

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
