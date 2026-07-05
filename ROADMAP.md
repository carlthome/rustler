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

## Now

- **A place to cash in the train — decide when to bank vs. push your luck** — now that the tail can
  snap (chain-snap risk shipped), the train has downside but nowhere to convert it. Give the player
  a delivery/pen where dropping off the train banks big score and resets tension, so every run
  becomes a running "bank now or grow it bigger and riskier" decision. This is the highest-leverage
  next beat: it closes the risk/reward loop that chain-snap just opened, turning the train from a
  number that only climbs into a resource you weigh, protect, and spend.
- **Terrain that shapes where the train can go** — the biomes are pure color grading today; make
  them matter to play. Tide pools that slow the tail, rocks the train can snag on, kelp that hides
  crabs — hazards you route a long train around rather than through. Wires the already-shipped biome
  system into gameplay and gives the new chain-snap risk real geography to play against.
- **Deeper music/rhythm integration** — the game already has BPM-synced visuals, rhythm catch
  bonuses, and now a Groove meter; take it further with actual layered music (the `layer1/2/3.ogg`
  progressive-fade hook already exists but nothing populates it) so the soundtrack itself builds as
  the groove/score rises, and tie more gameplay systems (spawns, chain movement, screen effects) to
  the beat rather than just the visuals reacting to it.
- **Menu and art facelift** — the title/menu screens and textures haven't kept pace with how far
  the in-run visual effects have come. Worth a real pass on menu layout and readability, and on
  texture/sprite quality for crabs, sand, and grass, so first impressions match the polish of
  the moment-to-moment gameplay.

_In flight (uncommitted WIP as of this update): a boss with a real threat verb — the King Crab now
charges the conga line to scatter the tail instead of just sitting there soaking beam. Leave it be;
if it lands and sticks, it's done and this note goes away._

## Later (outer loop — not yet)

- **Meta-progression between runs** — some small persistent unlock or upgrade that carries over
  after a run ends, so a "loss" still feels like progress and pulls the player into one more run.

## Also on our mind (not sequenced — no urgency, just don't lose it)

- **Emergent system interactions** — Carl's Noita-inspired itch: the fun isn't a full physics/
  material simulation (too big a rearchitecture for this game), it's letting the systems we
  already have actually affect each other instead of running in isolation. E.g. a beat pulse
  that startles nearby fleeing crabs into a chain reaction, a lasso catch that ripples fear
  through crabs near the catch point, chain segments that can bump and redirect fleeing crabs
  into each other. The first of these — beat-startle chain reactions — has now *shipped*
  (panic ripples crab-to-crab on each beat). The remaining ideas (fear rippling from a lasso
  catch point, chain segments bumping and redirecting fleeing crabs) stay parked here until one
  of them earns its way up too. The "conga train as a real risk" play has now *shipped* as
  chain-snap risk (a panicking crab that hits the tail knocks the last links loose) — an
  emergent-interaction win that wired the flee system back into the chain. The new "cash in the
  train" and "terrain shapes routing" Now items are the same spirit: give that risk somewhere to
  pay off and somewhere to play out.
