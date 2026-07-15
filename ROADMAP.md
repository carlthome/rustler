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
The **bank-now-vs-push-luck** axis is now fully legible on both sides: a live AT RISK readout, snap teeth that
escalate with length, and a superlinear triangular bank payoff with a BIG/LONG/GRAND HAUL cashout.

Biomes **push the herd** (Rocky Shore tide-shortcuts, Neon Kelp funnel lanes, chokepoints/tail-snag/wade-drag).
**All three bosses fight inside the archetype web** (King Crab bait-into-Armored, Tide Boss Golden-slingshot,
Reef DJ call-and-response), and rhythm verbs reach the climax (a charged Drum Roll cracks a boss shell fast).
The opt-in **How to Play** tutorial ships four scenarios, each a pure headless pass predicate doubling as a
regression test. A first slice of **meta-progression** + campaign scaffolding exists but stays parked in
"Later" — the gate is Carl's explicit "the core feels done" call, which hasn't come.

**Signal.** No new Slack reactions/replies this cycle; Carl's one standing note is *"Would be nice to see
example videos here!"* (a diary-agent task) — keep favoring legible, watchable moments. **Both prior top "Now"
items shipped, and the risk/reward axis is now genuinely balanced.** The long-train-vs-bank-often tension is
served on all three fronts: a live **AT RISK** readout mirrors the pen-worth tag with what a snap costs *right
now* (417c818), snap teeth **escalate by length** (3→4→5→6 tail links torn, b7b7448), and the bank payoff is
**superlinear** — `pen_worth = (n·(n+1)/2)·3`, a triangular sum, so a long train's priciest tail links pay
disproportionately, keeping the gamble tempting rather than pure punishment. The **BIG/LONG/GRAND HAUL** cashout
(36e880d) makes that reward face legible. So the bank-now-vs-push-luck decision now has real, visible teeth on
both sides — treat this axis as closed for now unless Carl says it still feels flat.

**Next frontier: the middle of the train is positionally inert.** Only the **head** (Golden figurehead, Dancer
Drum-Major) and **tail** (Armored tail-guard) slots carry weight; every crab between them is just a number for
pen_worth. A longer train should mean more *arrangement* decisions, not just more banking value — this deepens
the inner loop AND sharpens Carl's tension (holding long becomes a puzzle to set up, not only a risk to carry).

## Bugs (fix before anything else in Now)

Stability beats new features — an agent picking a task should check here first, before any
item in "Now" below.

- None currently known. (The start-of-run `InstanceArray capacity > 0` crash and the
  windowed-instead-of-fullscreen bug are both fixed.) If you hit a panic or a wrong-looking
  frame while testing, log it here before shipping anything new.

## Now

- **Make the middle of the train matter — mid-train arrangement depth. (Top Now item.)** Today only the head
  and tail slots pay; the crabs between are inert. Give the body of the conga positional meaning so a long
  train becomes a thing you *arrange*, not just accumulate. Concrete directions (pick one, reuse existing
  systems): **adjacency combos** — two same-type crabs sitting next to each other in the chain form a small
  bonus link (visible glow on the rope segment), so ordering your catches sets up a chain of pairs; or a
  **"sandwich"** where an archetype between two Goldens/Dancers inherits a slice of the figurehead effect.
  Keep it legible — the rope segment itself should show the bonus, mirroring the RUN xN badge. This directly
  serves Carl's long-train tension: holding long is now a setup puzzle worth the risk, not only a bigger number.
- **Reposition-your-train verb — let the player rearrange the conga mid-run.** Arrangement depth (above) is
  only interesting if the player can *act* on it. Right now catch order is fixed at catch time. Add a light
  way to shuffle the line — e.g. a rhythm-gated "cycle" that rotates the train one slot on the beat, or a
  drag/whistle that pulls a specific archetype toward the head. This turns the whole train into a live
  instrument to tune before banking, and gives the coveted head/tail slots something to *compete* for beyond
  luck of catch order. Keep the cost real (an on-beat window, a groove spend) so it's a decision, not a freebie.

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
