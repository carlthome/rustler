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

**Where we are.** The inner loop is deep, closed, with a spine, a real climax, and genuine routing hazards.
Four catching tools (beam/lasso/whistle/stomp) with upgrade lanes; a conga train with a chain-snap downside
and a delivery-pen jackpot; rhythm/groove scoring that **drives real mechanics** (downbeat spawn drops,
on-beat PERFECT hits, a beat-stepping train, an on-beat Call, a Downbeat Slam, a Groove Gamble cash-out, a
charged Drum Roll blast, an audible kick drum, a **Groove Dash**, a **downbeat herd pulse** that clumps free
crabs toward you, and an **on-beat catch bloom** that widens the catch window on the beat). Pacing ramps
through named intensity stages with a beat-tempo shift.

**Eight archetypes** (base, Armored → stomp, Dancer → rhythm, Magnet → routing, Thief → chain pressure,
Golden → chase decision, Hermit → uncrackable shell, Splitter → shape-bet) form a dense **emergent web** —
the signature fun. Catch-time crossovers fire visibly (Dancer hops chip shells and trip Goldens, snared
Goldens supercharge Magnets, shine lures Thieves off your tail, Golden→Magnet-tail arcs a shine cascade),
and both train slots now carry weight: **head** figureheads (Golden boosts match-run bonuses, Dancer
Drum-Major pumps groove economy) and **tail** placement (Armored parked at the tail tanks a Thief steal).

Biomes **push the herd** (Rocky Shore tide-shortcuts, Neon Kelp funnel lanes, chokepoints/tail-snag/wade-drag).
**All three bosses fight inside the archetype web** (King Crab bait-into-Armored, Tide Boss Golden-slingshot,
Reef DJ call-and-response), and rhythm verbs reach the climax (a charged Drum Roll cracks a boss shell fast).
The opt-in **How to Play** tutorial ships four scenarios, each a pure headless pass predicate doubling as a
regression test. A first slice of **meta-progression** + campaign scaffolding exists but stays parked in
"Later" — the gate is Carl's explicit "the core feels done" call, which hasn't come.

**Signal.** No new Slack reactions/replies this cycle; Carl's one standing note is *"Would be nice to see
example videos here!"* (a diary-agent task) — keep favoring legible, watchable moments. **Both prior top "Now"
items shipped**: the **Splitter** delivers the shape-*bet* (halve your conga on catch; grabbing it mid-match-run
is a Groove-Gamble-style wager with a live CLEAVE stakes tag), and two **field-level rhythm reads** landed
(downbeat herd pulse, on-beat catch bloom). Catch *order*, both train *positions*, and now a shape-*bet* all
pay. The remaining frontier per Carl's core direction: the **long-train-vs-bank-often** tension. Upside is
legible (train-worth tag over the pen), but the *downside* of holding a long unbanked train isn't — a snap
reads as an abstract event, not a mounting, visible cost. Make holding long **actively, legibly dangerous**.

## Bugs (fix before anything else in Now)

Stability beats new features — an agent picking a task should check here first, before any
item in "Now" below.

- None currently known. (The start-of-run `InstanceArray capacity > 0` crash and the
  windowed-instead-of-fullscreen bug are both fixed.) If you hit a panic or a wrong-looking
  frame while testing, log it here before shipping anything new.

## Now

- **Make holding a long train legibly dangerous. (Top Now item.)** The bank-now-vs-push-luck choice is
  Carl's core tension, but right now only the *upside* is on screen (the train-worth tag over the pen); the
  *risk* of not banking is abstract. Make the downside mount visibly with length: a live **"at-risk" readout**
  mirroring the train-worth tag (this is what a snap would cost you *right now*), so a long unbanked train
  reads as a loaded gun, not just a bigger number. Reuse the existing chain-snap and pen-worth systems — this
  is about legibility of the risk you're already carrying, so the decision to bank finally has teeth.
- **Give the risk real teeth as the train grows — escalating snap pressure by length.** Pair the readout above
  with a downside that actually scales: hold longer and a snap should hurt more or grow likelier — e.g. Thieves
  hunt longer tails harder, chain-snap chance climbs past a length threshold, or a slow decay nibbles an
  unbanked train. Balance it against a **superlinear bank payoff** so pushing a long train stays tempting, not
  just punishing — the point is to sharpen the gamble, not kill the long-train fantasy. Keep it legible: the
  player should *feel* the tension rising as they refuse to bank.

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
