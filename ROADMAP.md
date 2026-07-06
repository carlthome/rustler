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

**Where we are.** The inner loop now feels substantially DONE. Everything that made the core loop
tense and expressive has landed: catching across four distinct tools (beam/lasso/whistle/stomp),
the conga train with real chain-snap downside, a delivery pen to bank it, rhythm/groove scoring,
biomes with terrain hazards, the King Crab boss with a tail-scattering charge, upgrade cards that
now **branch into four playstyle lanes** (Beam/Lasso/Whistle/Stomp Focus, each deepening with
rank), and a whistle **soothe** that talks a panicking herd down and makes them immune to
beat-startle contagion. The risk/reward loop is closed and the toolkit all matters. Depth-first
work is running out of high-leverage targets — so this run **promotes meta-progression from Later
into Now**. The one remaining pure-inner-loop item (layered music) is half-blocked on human audio
authoring; its actionable code half is called out below.

## Now

- **Meta-progression between runs** (promoted from Later) — the inner loop is tight enough that the
  biggest remaining fun-per-effort now lives across runs, not inside one. Add some small persistent
  thread that carries over after a run ends — a currency banked from cash-ins, a handful of
  permanent unlocks (a starting tool rank, a new crab archetype, a cosmetic), a run-history / best-
  score record on the title screen — so a "loss" still feels like progress and pulls the player
  into one more run. Start minimal and persistent (a single save file), not a sprawling meta-tree.
- **Tie more gameplay to the beat** — the actionable half of the old layered-music item, which a
  code agent can do without any new audio assets. Today the beat mostly drives visuals; push it
  into mechanics: quantize spawn waves to the bar, make chain movement / catch windows / groove
  bonuses land harder exactly on-beat, reward on-beat tool use. Closes the gap between "visuals
  pulse to the beat" and "the beat is the game."

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
  deflecting fleeing crabs back toward the beam, and the whistle soothe calming a panic and
  granting startle immunity. Still parked here until one earns its way up: fear rippling outward
  from a lasso catch point, and chain segments redirecting fleeing crabs into each other. Same
  depth-first spirit — deepen what's there before going wide.
</content>
</invoke>
