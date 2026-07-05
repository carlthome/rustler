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

- **Make the conga train a real risk, not just upside** — right now the chain only ever grows, so
  a long train is pure reward with no downside. Give length stakes: a panicking crab, a King Crab
  charge, or a level hazard can snap the tail and scatter those crabs back into the wild, so a big
  train becomes something you actively protect, route carefully, and can lose. This turns the
  game's central mechanic from a growing counter into a moment-to-moment decision — the single
  highest-leverage way to deepen the inner loop.
- **A second boss that fights the loop, not just absorbs it** — the King Crab shipped, but it's a
  stationary damage-sponge you park the beam on; the encounter tests beam uptime and nothing else.
  Add a boss with an actual threat verb — charges the conga line to scatter it, or rallies/spawns
  nearby crabs into a wall — so boss fights test movement and chain management too. Pairs naturally
  with the chain-risk item above.
- **Deeper music/rhythm integration** — the game already has BPM-synced visuals, rhythm catch
  bonuses, and now a Groove meter; take it further with actual layered music (the `layer1/2/3.ogg`
  progressive-fade hook already exists but nothing populates it) so the soundtrack itself builds as
  the groove/score rises, and tie more gameplay systems (spawns, chain movement, screen effects) to
  the beat rather than just the visuals reacting to it.
- **Menu and art facelift** — the title/menu screens and textures haven't kept pace with how far
  the in-run visual effects have come. Worth a real pass on menu layout and readability, and on
  texture/sprite quality for crabs, sand, and grass, so first impressions match the polish of
  the moment-to-moment gameplay.

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
  of them earns its way up too. Note the new "conga train as a real risk" Now item is itself an
  emergent-interaction play: it wires the flee/boss systems back into the chain.
