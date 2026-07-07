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

**Where we are.** The inner loop is deep, closed, and now has a spine and a real climax. Four
catching tools (beam/lasso/whistle/stomp) with four upgrade playstyle lanes, the conga train
with a chain-snap downside and a delivery-pen jackpot, and rhythm/groove scoring that **drives
real mechanics**: bar-quantized spawn drops on the downbeat, on-beat PERFECT tool hits, a
beat-stepping train, an on-beat Call that Dancers answer for a Dance Catch bonus, a full-meter
**Downbeat Slam ultimate** (G on the beat), and now a **Groove Gamble** risk/reward layer where
consecutive on-beat catches compound a live global multiplier that one off-beat grab snaps back
to zero with a red sting. Pacing is no longer flat: a **staged difficulty ramp** climbs through
named intensity stages, with Frenzy spikes and a **beat-tempo shift** that speeds the whole run
up. Biomes **play differently** — Rocky Shore chokepoints, Neon Kelp tail-snag, Tide Pool
wade-drag. Four enemy archetypes beyond the base crab (Armored → stomp, Dancer → rhythm, **Magnet
→ routing**, dragging free crabs into a cluster so its catch is a two-for-one) plus **two bosses
that now fight as set-pieces**: catching a boss clears the herd for a focused duel, the boss
enrages into a faster final phase below 40% health, and the catch reads as a distinct victory.
A first slice of **meta-progression** is in (persistent career + banked-crab perk shop). Slack
diary posts read as steady positive momentum with no pushback and no fresh reactions/replies
this cycle; Carl's last recorded steer ("more visual spectacle or a difficulty ramp tweak") is
long since shipped. Both prior "Now" items (boss set-pieces, Groove Gamble rhythm layer) and the
Magnet archetype shipped this cycle. The inner loop is now very close to complete — we still want
Carl's explicit "the core feels done" call before pivoting to outer-loop work.

## Bugs (fix before anything else in Now)

Stability beats new features — an agent picking a task should check here first, before any
item in "Now" below.

- None currently known. (The start-of-run `InstanceArray capacity > 0` crash and the
  windowed-instead-of-fullscreen bug are both fixed.) If you hit a panic or a wrong-looking
  frame while testing, log it here before shipping anything new.

## Now

- **A boss that changes the arena, not just its own health bar** — boss set-pieces landed well
  (herd-clear duel, enrage phase, victory catch). Take the next step in the same set-piece spirit:
  let a boss reshape the *space* of the duel for its final phase — the King Crab cracking the
  floor into hazard lanes you must weave the train around, or the Tide Boss flooding the arena so
  routing changes mid-fight. Make the enrage a moment the player has to adapt to, not just tank.
  Deepens the existing archetype rather than adding a new one. **Top Now item — bosses are the
  standout set-piece and this is where depth compounds most.**
- **Make the Groove Gamble a decision, not just a streak** — the compounding on-beat multiplier is
  live and reads great, but right now the only choice is "keep catching on-beat." Give the player
  a real fork: a cash-out beat where banking the streak now locks in the multiplier before a miss
  can wipe it, versus pushing it higher and risking the whole stack. Layer it onto the delivery
  loop so the tension of *when to bank* rides the beat too — turns the gamble from a passive
  bonus into an active call the player sweats over.

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

- **A fifth archetype that pressures the chain itself** — Magnet (routing), Armored (crack-open),
  Dancer (rhythm) and base are in; the remaining gap is a crab that threatens the *train you've
  already built* rather than the herd you're chasing. E.g. a skittish Runner that bolts along
  walls and can clip the conga tail loose as it passes, or a thief crab that latches to the tail
  and peels links off unless you shake it. Depth via a new pressure on the chain, not breadth.
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
