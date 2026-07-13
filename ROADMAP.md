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

**Where we are.** The inner loop is deep, closed, and has a spine, a real climax, and genuine
routing hazards. Four catching tools (beam/lasso/whistle/stomp) with upgrade lanes, the conga train
with a chain-snap downside and a delivery-pen jackpot, and rhythm/groove scoring that **drives real
mechanics**: downbeat spawn drops, on-beat PERFECT hits, a beat-stepping train, an on-beat Call that
Dancers answer, a full-meter **Downbeat Slam** (G), a **Groove Gamble** cash-out (B), and now a
player-driven **Drum Roll** (hold T) that charges over a bar of on-beat holds and releases a wide beam
blast. Pacing ramps through named intensity stages with a **beat-tempo shift**. Biomes **play
differently** (Rocky Shore chokepoints, Neon Kelp tail-snag, Tide Pool wade-drag). **Six archetypes**
(base, Armored → stomp, Dancer → rhythm, Magnet → routing, Thief → chain pressure, Golden → chase
decision) form a dense **emergent web** — the game's signature fun — where Dancer hops chip shells and
trip Goldens, snared Goldens supercharge Magnets, and shine lures Thieves off your tail. **Three bosses
rotate as arena-shifting set-pieces** (King Crab fissures, Tide Boss flood, Reef DJ call-and-response),
and the arena now keeps the archetype web alive mid-fight: the **Reef DJ summons hype Dancers** to catch
on its hot beats, and a **King Crab charge can be baited into a parked Armored crab to stun it**. A first
slice of **meta-progression** is in (persistent career + perk shop). No new Slack signal this cycle;
Carl's standing note is still *"Would be nice to see example videos here!"* — a task for the diary agent,
but it confirms the rhythm/visual-spectacle bet is what he wants to *watch*, so keep favoring legible,
watchable moments. Both prior "Now" items shipped: the **boss-archetype fusion** (above) and the
player-driven **Drum Roll** rhythm verb. The inner loop is essentially complete on depth — still want
Carl's explicit "the core feels done" call before pivoting to outer-loop work, so the frontier now:
finish the boss-archetype fusion on the one boss still fought in a bubble (Tide Boss), and pull the
player's new rhythm verbs *into* the boss fights.

## Bugs (fix before anything else in Now)

Stability beats new features — an agent picking a task should check here first, before any
item in "Now" below.

- None currently known. (The start-of-run `InstanceArray capacity > 0` crash and the
  windowed-instead-of-fullscreen bug are both fixed.) If you hit a panic or a wrong-looking
  frame while testing, log it here before shipping anything new.

## Now

- **Finish the boss-archetype fusion — give the Tide Boss its archetype play. (Top Now item.)** Two of
  the three bosses now keep the archetype web alive mid-fight (Reef DJ hype Dancers, King Crab bait-into-
  Armored), but the Tide Boss is still fought in a bubble — its surge only touches a caught Magnet
  defensively. Bring an archetype into its arena as an *offensive* play the player performs: e.g. a floor
  Magnet the player lures a Golden into and slingshots at the boss for burst shell damage on the beat, or
  Dancers whose on-beat hops ride the surge to chip it. Completes the "every climax uses the web" symmetry
  and lands another legible, watchable moment for the videos Carl wants.
- **Pull the player's rhythm verbs *into* the boss fights.** The new Drum Roll (hold T), Downbeat Slam,
  and Groove Gamble mostly shine while herding the open field, then go quiet or feel unfocused once a boss
  clears the herd. Make one of them matter at the climax: e.g. a charged Drum Roll blast that cracks a
  boss shell far faster than a held beam (a real reason to spend a bar charging mid-duel), or a Slam that
  staggers a King Crab mid-charge. Fuses the two systems that just shipped — the rhythm verbs and the
  boss climaxes — instead of leaving them in separate rooms.
- **Synthesise a kick drum on the beat.** Carl wants the BPM to be visceral, not just visual. Add a
  new `sounds.rs` (or similar) with a simple procedural audio synthesiser — a short sine-wave thump
  with a fast pitch drop, generated at runtime and played through ggez's audio at every beat tick.
  No external audio files needed; pure synthesis. This gives every player instant tactile confirmation
  of the beat and makes the rhythm mechanics legible without reading any UI. Once in, it can be
  layered or filtered with groove intensity for extra juice.
- **Tutorial mode — per-mechanic learn sessions from the main menu.** Carl wants players to actually
  understand the mechanics before a run. Add a "How to Play" entry on the title screen that
  presents isolated tutorial levels — one per major mechanic (lasso, chain/delivery, beat timing,
  tools, bosses). Each session: a short plain-language instruction card, a tiny scripted sandbox
  level with only the relevant enemies/objects, and a clear pass condition the player must fulfil
  (e.g. "catch 3 crabs on the beat", "deliver a chain of 5 to the pen") before advancing. No
  progression gating — purely opt-in teaching. New `src/tutorial.rs` (or a level-type flag in
  `levels.rs`) is the natural home for the scripted scenario logic.
  **Bonus — agent feedback loop.** Because each tutorial has a machine-readable pass condition,
  the same scenario can be driven headlessly (simulated inputs, deterministic RNG seed) and used
  by dev agents to verify that a mechanic still works after they change it — "does the beat-timing
  tutorial still pass?" is a much tighter signal than "does it build?". Design the pass conditions
  as plain boolean predicates on game state so they're trivially queryable from a test harness or a
  short `--tutorial-check` CLI flag, with no rendering required. This turns every tutorial into a
  living mechanic regression test the agents can run themselves.

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
