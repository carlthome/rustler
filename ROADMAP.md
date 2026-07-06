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
   come back. Not yet — don't pull items from this phase in while phase 1 is still open.

**Where we are.** The inner loop now feels substantially built out: catching (beam/lasso/whistle/
stomp), the conga train with real chain-snap downside, a delivery pen to bank it, rhythm/groove
scoring, biomes with terrain hazards, and the King Crab boss with a charge that scatters the tail.
The risk/reward loop is closed. Two depth items remain below; once they land, we're likely ready
to promote the outer loop (meta-progression) from Later into Now.

## Now

- **Upgrade choices that branch into playstyles** — the upgrade cards are the roguelite heart of a
  run, but today they're four flat stat bumps (`apply_upgrade`: wider cone, longer range, more
  speed, bigger catch radius). Nothing makes one run play differently from the next. Give the tools
  we already have (beam / lasso / whistle / stomp) real upgrade branches that synergize — a
  lasso-focused build that chains catches, a whistle/crowd-control build, a beam-DPS boss-hunter
  build — so the choices at each level-up steer the run and reward committing to a lane. This is the
  highest-leverage remaining inner-loop beat: it turns level-ups from "numbers go up" into
  meaningful decisions, and it makes the whole toolkit matter instead of defaulting to the beam.
- **The soundtrack builds with the groove** — the `layer{1,2,3}.ogg` progressive-fade hook already
  exists in code (main.rs tries to load them at startup) but no audio files populate it, so it's
  inert. Author those layers and wire them to the Groove meter / score so the music itself thickens
  as the player's run heats up, and tie more gameplay (spawn timing, chain movement) to the beat
  rather than only the visuals reacting to it. Closes the gap between "visuals pulse to the beat"
  and "the beat is the game."

## Later (outer loop — not yet)

- **Meta-progression between runs** — some small persistent unlock or upgrade that carries over
  after a run ends, so a "loss" still feels like progress and pulls the player into one more run.
  _Getting close to promotable:_ with the inner loop nearly done, this is the natural next frontier
  once the two Now items land. Not yet — finish phase 1 first.

## Also on our mind (not sequenced — no urgency, just don't lose it)

- **Emergent system interactions** — Carl's Noita-inspired itch: the fun isn't a full physics/
  material simulation (too big a rearchitecture for this game), it's letting the systems we
  already have actually affect each other instead of running in isolation. Shipped so far:
  beat-startle chain reactions (panic ripples crab-to-crab on each beat) and chain-snap risk (a
  panicking crab that hits the tail knocks the last links loose — wired the flee system back into
  the chain). Still parked here until one earns its way up: fear rippling outward from a lasso
  catch point, and chain segments bumping and redirecting fleeing crabs into each other. The
  playstyle-branch and layered-music Now items are the same depth-first spirit — deepen what's
  there before going wide.
