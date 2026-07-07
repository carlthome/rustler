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
in (persistent career + banked-crab perk shop). Slack diary posts read as steady positive momentum
with no pushback and no fresh reactions/replies this cycle; Carl's last recorded steer ("more visual
spectacle or a difficulty ramp tweak") is long since shipped. Both prior "Now" items (arena-shifting
boss, Groove Gamble cash-out) shipped this cycle, along with the Thief and Golden archetypes. The
inner loop is now very close to complete — we still want Carl's explicit "the core feels done" call
before pivoting to outer-loop work.

## Bugs (fix before anything else in Now)

Stability beats new features — an agent picking a task should check here first, before any
item in "Now" below.

- None currently known. (The start-of-run `InstanceArray capacity > 0` crash and the
  windowed-instead-of-fullscreen bug are both fixed.) If you hit a panic or a wrong-looking
  frame while testing, log it here before shipping anything new.

## Now

- **Let the systems collide — emergent interactions between the archetypes we already have.**
  This is Carl's stated Noita-inspired itch, and it's where depth compounds now that six archetypes
  exist but mostly act in isolation. Pick one concrete crossover and make it real: a Magnet crab
  that drags a *latched Thief* off your tail (or, worse, drags free crabs into the Thief's reach); a
  Golden crab whose panic-flee ripples startle contagion through a tight herd on the beat; a Dancer's
  on-beat hop startling its neighbors into a chain-reaction stampede. Not a physics rewrite — just
  let two existing systems affect each other so the field produces situations no single rule
  authored. **Top Now item — the archetype roster is full; the fun frontier is now their overlap.**
- **A rhythm-native counterplay for the Thief — shake it off on the beat.** The Thief pressures the
  train you've built, but the counter is currently a flat catch/whistle. Make dislodging it a
  rhythm beat too: an on-beat whistle or stomp yanks it clean (and maybe flings it back into the
  herd as a bonus catch), an off-beat one only loosens its grip. Ties the game's newest chain-threat
  into its core rhythm layer instead of sitting beside it, so dealing with the Thief *plays* like the
  rest of the game.

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

- **A rhythm-duel boss — a third boss whose fight IS the beat.** King Crab (charge) and Tide Boss
  (shockwave) are spatial/hazard fights. The gap is a boss you beat by *playing the rhythm*: it
  opens a vulnerable window only on the downbeat, or mirrors a short call-and-response phrase you
  have to echo back on-beat to damage it. Would make the game's best system (rhythm) carry a whole
  set-piece rather than just modifying the others. Promote once the emergent-interaction item above
  has proven the archetype overlap is fun.
- **Playful bonus rounds** — Carl's Street Fighter II / Lion King (SNES) itch: a rare, surprising
  mini-challenge dropped into a run purely for spice (not for balance or progression) — a bonus
  catch-everything sprint, a rhythm-only gauntlet, something silly and short. Parked here rather
  than in "Now" since it's a side-system/breadth item by nature, same category as alternate game
  modes — worth revisiting once the core loop itself feels done.
