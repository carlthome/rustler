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

**Where we are.** The inner loop is deep and closed, and it now has a spine. Four catching
tools (beam/lasso/whistle/stomp) with four upgrade playstyle lanes, the conga train with a
chain-snap downside and a delivery-pen jackpot, and rhythm/groove scoring that **drives real
mechanics**: bar-quantized spawn drops on the downbeat, on-beat PERFECT tool hits, a
beat-stepping train, an on-beat Call that Dancers answer for a Dance Catch bonus, and now a
full-meter **Downbeat Slam ultimate** (G on the beat) that erupts a gold shockwave and yanks a
whole cluster into the train. Pacing is no longer flat: a **staged difficulty ramp** climbs
through named intensity stages over elapsed time, with Frenzy spikes and a **beat-tempo shift**
that speeds the whole run up as it escalates. Biomes now **play differently** — Rocky Shore
chokepoints, Neon Kelp tail-snag, Tide Pool wade-drag — not just color skins. Three enemy
archetypes beyond the base crab (Armored → stomp, Dancer → rhythm) plus **two bosses** (King
Crab charge, Tide Boss shockwave, alternating). A first slice of **meta-progression** is in
(persistent career + banked-crab perk shop). The recent Slack diary posts read as steady
positive momentum with no pushback; Carl's own recorded steer was "more visual spectacle or a
difficulty ramp tweak," both now shipped — so the rhythm-spectacle and pacing bets are landing.
All three prior "Now" items (staged ramp, biome mechanics, rhythm ultimate) shipped this cycle,
and the start-of-run crash + fullscreen bugs are fixed. The inner loop is close to done but not
declared finished — a couple of depth targets remain before we'd pivot to outer-loop work, and
we still want Carl's explicit "the core feels done" call first.

## Bugs (fix before anything else in Now)

Stability beats new features — an agent picking a task should check here first, before any
item in "Now" below.

- None currently known. (The start-of-run `InstanceArray capacity > 0` crash and the
  windowed-instead-of-fullscreen bug are both fixed.) If you hit a panic or a wrong-looking
  frame while testing, log it here before shipping anything new.

## Now

- **Boss fights that feel like fights, not obstacles** — the King Crab and Tide Boss alternate
  and land telegraphed attacks, but a boss is still just a tankier crab on the same field. Give
  a boss encounter real structure: a distinct arena moment or wave lull when it arrives, a
  multi-phase escalation as you wear it down, and a payoff catch that reads as a genuine victory
  (bigger than a normal delivery). Deepens an existing archetype into a set-piece rather than
  adding a new one. **Top Now item — bosses land well and this is where depth compounds most.**
- **A rhythm risk/reward layer on the beat window** — PERFECT on-beat hits and the Downbeat Slam
  proved players enjoy actively *playing* the beat. Push it further into decision-making, not
  just timing: e.g. an on-beat "gamble" where nailing consecutive downbeats compounds a multiplier
  that a single miss resets, or a beat-locked window where holding the train through a hazard on
  the bar pays out but mistiming scatters it. Turns the rhythm from a bonus into a live tension
  the player is managing — the game's most distinctive hook, made deeper.

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

- **A fourth enemy archetype that reshapes routing** — three archetypes (Armored, Dancer, base)
  cover crack-open and rhythm; the gap is a crab that changes *how you move* the train, not just
  how you catch. E.g. a skittish Runner that bolts along walls, or a Magnet crab that drags
  nearby free crabs with it so catching it is a two-for-one. Depth via a new pressure on the
  chain, not breadth. Promote when a boss set-piece and the rhythm-risk layer are done.
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
