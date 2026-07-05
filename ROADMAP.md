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

- **Boss crabs** — a rare, oversized crab with its own attack pattern and a real "catch" moment
  (multi-stage lasso, or it needs to be worn down first). Gives runs a memorable peak instead
  of just more of the same enemy.
- **Biome/level variety** — the play area currently reads as one continuous space; distinct
  zones (tide pools, rocky shore, kelp forest) with their own hazards and spawn flavor would
  make runs feel like they're going somewhere.
- **New player abilities, with real counters** — right now the core verb is lasso-and-catch,
  and one tool can't be "wrong" because there's nothing else to reach for. Design any new tool
  (dash-grab, a whistle that startles a cluster of crabs toward you) alongside crab archetypes
  it's specifically good or bad against (a fast/small crab that dodges the lasso but folds to a
  whistle, an armored one that shrugs off a whistle but is an easy lasso target), Doom Eternal
  style — so picking the right tool for the crab in front of you is the skill, not just spamming
  one favorite. Important guardrail: soft counters, not hard requirements. Every tool should
  stay viable (if suboptimal) against every crab at normal difficulty — being bad at something
  costs time or style, it never means "impossible without the right tool." A tool being
  strictly mandatory against some crab is only acceptable, if ever, as an escalation at the
  highest difficulty tiers, not baseline design. Player choice matters more than optimal play.
- **Deeper music/rhythm integration** — the game already has BPM-synced visuals and rhythm catch
  bonuses; take it further with actual layered music (the `layer1/2/3.ogg` progressive-fade
  hook already exists but nothing populates it) so the soundtrack itself builds as the score
  rises, and tie more gameplay systems (spawns, chain movement, screen effects) to the beat
  rather than just the visuals reacting to it.
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
  into each other. Worth watching for a natural moment to pull one of these into "Now" once a
  couple of the systems it needs (flee/panic, beat pulses, chain) have settled down.
