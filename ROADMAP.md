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
Dancers answer, a full-meter **Downbeat Slam** (G), a **Groove Gamble** cash-out (B), and a
player-driven **Drum Roll** (hold T) that charges over a bar of on-beat holds and releases a wide beam
blast — and every beat now lands as an **audible synthesised kick drum** so the BPM is visceral, not
just visual. Pacing ramps through named intensity stages with a **beat-tempo shift**. Biomes **play
differently** (Rocky Shore chokepoints, Neon Kelp tail-snag, Tide Pool wade-drag). **Six archetypes**
(base, Armored → stomp, Dancer → rhythm, Magnet → routing, Thief → chain pressure, Golden → chase
decision) form a dense **emergent web** — the game's signature fun — where Dancer hops chip shells and
trip Goldens, snared Goldens supercharge Magnets, and shine lures Thieves off your tail. **All three
bosses now fight inside the archetype web**: King Crab fissures + a charge you bait into a parked
Armored crab, Tide Boss flood + a **Golden-slingshot** (lure a Golden into a floor Magnet and fire it
through the surge to crack the shell), Reef DJ call-and-response + hype Dancers to catch on its hot
beats — and the player's **rhythm verbs reach the climax** too (a charged Drum Roll blast cracks a boss
shell far faster than a held beam). A first slice of **meta-progression** is in (persistent career +
perk shop), and an opt-in **How to Play** tutorial ships its first scenario (beat-timing) with a pure,
headless-queryable pass predicate that doubles as an agent-run mechanic regression test. No new Slack
signal this cycle; Carl's standing note is still *"Would be nice to see example videos here!"* — a task
for the diary agent, but it confirms the rhythm/visual-spectacle bet is what he wants to *watch*, so
keep favoring legible, watchable moments. **All four prior "Now" items shipped** (Tide Boss slingshot,
Drum Roll boss-crack, kick drum, tutorial-slice). The inner loop is essentially complete on depth — we
still want Carl's explicit "the core feels done" call before pivoting to outer-loop work, so the
frontier stays depth-first: **grow the archetype web with a seventh archetype** that opens genuinely
new crossover edges, and **deepen biome routing** so environments push the herd around, not just
recolor it.

## Bugs (fix before anything else in Now)

Stability beats new features — an agent picking a task should check here first, before any
item in "Now" below.

- None currently known. (The start-of-run `InstanceArray capacity > 0` crash and the
  windowed-instead-of-fullscreen bug are both fixed.) If you hit a panic or a wrong-looking
  frame while testing, log it here before shipping anything new.

## Now

- **A seventh archetype that opens new crossover edges. (Top Now item.)** The six-archetype web is the
  game's signature fun, and the fastest way to deepen it is one more archetype whose *interactions* with
  the existing six are the point — not a stat variant. Think in edges: e.g. a **Hermit** crab that hides
  in a shell and periodically swaps hosts (Stomp cracks the shell, but a fleeing Dancer's hop can knock it
  loose, and a Magnet can rip it out — three existing verbs, one new target), or a **Splitter** that
  halves into two smaller crabs when caught so grabbing it mid-combo is a Groove-Gamble decision. Pick one
  whose new edges reuse the existing verb systems (Stomp/hop/Magnet/Golden-lure) rather than adding a
  parallel system. Legible and watchable: a new visible reaction the player can learn and exploit.
- **Deepen biome routing — make the environment push the herd, not just recolor it.** Biomes already
  *feel* different (Rocky Shore chokepoints, Neon Kelp tail-snag, Tide Pool wade-drag), but the terrain
  mostly affects *the player*. Add a biome feature that reshapes *where the herd goes*, so routing becomes
  a puzzle you solve with the environment: e.g. a **Tide Pool current** that drifts free crabs downstream
  (herd them into it and let it deliver them toward your train), Neon Kelp **fronds that funnel** panicked
  crabs into a lane, or Rocky Shore **tide that rises and falls** on a bar cycle, opening/closing a
  shortcut on the beat. One biome, one new routing mechanic — ties terrain into the rhythm and the chase.
- **Grow the tutorial past its first slice** *(first scenario shipped)*. The beat-timing session and its
  pure headless-queryable pass predicate are in and already double as an agent regression test. Add the
  next one or two scenarios the same way — one per remaining major mechanic (lasso, chain/delivery, tools,
  bosses) — each a tiny scripted sandbox with one instruction card and one boolean pass condition, so the
  suite of mechanic regression tests grows alongside the teaching. Keep every `passed()` a pure predicate
  over game state (no rendering/input) so a `--tutorial-check` run stays trivial.

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
