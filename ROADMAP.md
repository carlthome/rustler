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

**Where we are.** The inner loop is deep, closed, and has a spine, a real climax, and now genuine
routing hazards. Four catching tools (beam/lasso/whistle/stomp) with four upgrade playstyle lanes,
the conga train with a chain-snap downside and a delivery-pen jackpot, and rhythm/groove scoring
that **drives real mechanics**: bar-quantized spawn drops on the downbeat, on-beat PERFECT tool hits,
a beat-stepping train, an on-beat Call that Dancers answer for a Dance Catch bonus, a full-meter
**Downbeat Slam ultimate** (G on the beat), and a **Groove Gamble** risk/reward layer with a real
**cash-out decision** — press B to bank the live multiplier into a safe floor before an off-beat
miss wipes it, banking on the beat locks the whole stack. Pacing is not flat: a **staged difficulty
ramp** climbs through named intensity stages, with Frenzy spikes and a **beat-tempo shift** that
speeds the whole run up. Biomes **play differently** — Rocky Shore chokepoints, Neon Kelp tail-snag,
Tide Pool wade-drag. **Six enemy archetypes** now: base plus Armored → stomp, Dancer → rhythm,
Magnet → routing, **Thief → chain pressure** (latches to the tail and peels links until you deal
with it) and **Golden → chase decision** (rare shiny that bolts fast and pays a big lump sum). Two
bosses fight as **arena-shifting set-pieces**: a boss catch clears the herd for a focused duel, and
below 40% health the boss enrages and reshapes the *space* — King Crab cracks the floor into
beat-pulsing hazard fissures that bite the tail, Tide Boss floods extra wade-drag water — so the
final phase is something you route around, not just tank. A first slice of **meta-progression** is
in (persistent career + banked-crab perk shop). Slack momentum stays positive with no pushback;
Carl's one fresh reply this cycle was on the diary post — *"Would be nice to see example videos
here!"* That's a note for the Developer Diary agent (cron 3) to capture short clips/GIFs of the
spectacle, not a game-direction change — but it confirms the rhythm/visual-spectacle bet is what
he wants to *watch*, so keep leaning into legible, screenshot-worthy moments. Both prior "Now" items
shipped this cycle: **emergent archetype interactions** (Magnet pries a latched Thief off your tail
and snares a fleeing Golden; a fleeing Golden's panic ripples startle contagion through the herd; a
free Armored's shell shelters the herd from the ripple) and the **rhythm-native Thief counterplay**
(on-beat whistle/stomp rips it clean and banks it as a bonus). The archetype overlap proved fun, so
the fun frontier is now: (a) a boss that carries the *rhythm* system, and (b) more collisions between
the six archetypes. The inner loop is now very close to complete — we still want Carl's explicit "the
core feels done" call before pivoting to outer-loop work.

## Bugs (fix before anything else in Now)

Stability beats new features — an agent picking a task should check here first, before any
item in "Now" below.

- None currently known. (The start-of-run `InstanceArray capacity > 0` crash and the
  windowed-instead-of-fullscreen bug are both fixed.) If you hit a panic or a wrong-looking
  frame while testing, log it here before shipping anything new.

## Now

- **A rhythm-duel boss — a third boss whose fight IS the beat. (Top Now item.)** King Crab (charge)
  and Tide Boss (shockwave) are both spatial/hazard fights; the biggest remaining depth gap is a boss
  that makes the game's *best* system carry a whole set-piece instead of just modifying the others.
  It opens a vulnerable window only on the downbeat, or calls a short 2–4 note phrase you have to echo
  back on-beat (with the tools you already have — beam/lasso/whistle/stomp) to damage it, mistimed
  hits do nothing or punish. Promoted now that the archetype-overlap work proved the rhythm layer is
  where this game is most itself, and it's exactly the kind of legible, watchable set-piece Carl said
  he'd like to see on video.
- **Keep the systems colliding — one more emergent crossover between the six archetypes.** The first
  batch landed well (Magnet vs. Thief/Golden, Golden panic contagion, Armored calm-anchor), so keep
  mining Carl's Noita itch: e.g. a Dancer's on-beat hop startling its neighbors into a chain-reaction
  stampede, a Magnet dragging free crabs *into* a Thief's latch range, or a Golden's shine luring a
  Magnet off its cluster. One concrete new interaction per run, no physics rewrite — just let two
  existing rules produce a situation neither authored alone.

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

- **Playful bonus rounds** — Carl's Street Fighter II / Lion King (SNES) itch: a rare, surprising
  mini-challenge dropped into a run purely for spice (not for balance or progression) — a bonus
  catch-everything sprint, a rhythm-only gauntlet, something silly and short. Parked here rather
  than in "Now" since it's a side-system/breadth item by nature, same category as alternate game
  modes — worth revisiting once the core loop itself feels done.
