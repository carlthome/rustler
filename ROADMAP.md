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

- **Deeper music/rhythm integration** — the game already has BPM-synced visuals and rhythm catch
  bonuses; take it further with actual layered music (the `layer1/2/3.ogg` progressive-fade
  hook already exists but nothing populates it) so the soundtrack itself builds as the score
  rises, and tie more gameplay systems (spawns, chain movement, screen effects) to the beat
  rather than just the visuals reacting to it.
- **Menu and art facelift** — the title/menu screens and textures haven't kept pace with how far
  the in-run visual effects have come. Worth a real pass on menu layout and readability, and on
  texture/sprite quality for crabs, sand, and grass, so first impressions match the polish of
  the moment-to-moment gameplay.
- **Grow the tool/counter toolkit into a real decision** — the Whistle shipped as the first
  soft counter (great on skittish Sneaky crabs, weak on heavy Big ones), but with only two verbs
  (lasso + whistle) "pick the right tool" is still a shallow choice. Add a third tool paired with
  a crab archetype that resists *both* current tools — e.g. an armored/burrowing crab the lasso
  slips off and the whistle barely moves, that a new close-range dash-grab or a stomp/ground-pound
  cracks open. Same soft-counter guardrail as before: every tool stays viable against every crab,
  the wrong pick just costs time or style. Three real options is where tool selection starts to
  feel like a skill instead of a formality.
- **Emergent system interaction — beat-startle chain reactions** — the prerequisites this needed
  (flee/panic, beat pulses, chain, catch-triggered stampede) have all shipped, so pull the first
  emergent-interaction experiment into Now: let a beat wave or a catch's alarm ring that hits one
  fleeing crab pass its panic to nearby crabs, rippling a stampede outward crab-to-crab instead of
  every crab reacting only to the player. Cheap to prototype on top of the startle timer that
  already exists, and it's the clearest expression of Carl's Noita itch — systems affecting each
  other, not just the player.

## Later (outer loop — not yet)

- **Meta-progression between runs** — some small persistent unlock or upgrade that carries over
  after a run ends, so a "loss" still feels like progress and pulls the player into one more run.

## Also on our mind (not sequenced — no urgency, just don't lose it)

- **Emergent system interactions** — Carl's Noita-inspired itch: the fun isn't a full physics/
  material simulation (too big a rearchitecture for this game), it's letting the systems we
  already have actually affect each other instead of running in isolation. E.g. a beat pulse
  that startles nearby fleeing crabs into a chain reaction, a lasso catch that ripples fear
  through crabs near the catch point, chain segments that can bump and redirect fleeing crabs
  into each other. The first of these (beat-startle chain reactions) has now graduated into "Now"
  since flee/panic, beat pulses, and chain have all settled — the remaining ideas (fear rippling
  from a lasso catch point, chain segments bumping and redirecting fleeing crabs) stay parked here
  until one of them earns its way up too.
