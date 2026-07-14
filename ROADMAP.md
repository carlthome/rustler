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
just visual. Pacing ramps through named intensity stages with a **beat-tempo shift**. **Seven archetypes**
(base, Armored → stomp, Dancer → rhythm, Magnet → routing, Thief → chain pressure, Golden → chase
decision, Hermit → shell the beam can't crack) form a dense **emergent web** — the game's signature fun —
where Dancer hops chip shells and trip Goldens, snared Goldens supercharge Magnets, shine lures Thieves
off your tail, and the Hermit only pops to a Stomp, a Dancer's hop, or a passing Magnet's rip (three
existing verbs, one new target). Biomes now **push the herd, not just recolor it**: Rocky Shore's tide
floods low rocks into beat-timed shortcuts, Neon Kelp fronds **funnel** fleeing crabs into a lane, plus
the earlier chokepoints/tail-snag/wade-drag feel. **All three bosses fight inside the archetype web**:
King Crab fissures + a charge you bait into a parked Armored crab, Tide Boss flood + a **Golden-slingshot**
(lure a Golden into a floor Magnet and fire it through the surge to crack the shell), Reef DJ
call-and-response + hype Dancers to catch on its hot beats — and the player's **rhythm verbs reach the
climax** too (a charged Drum Roll blast cracks a boss shell far faster than a held beam). The opt-in
**How to Play** tutorial now ships **three scenarios** (beat-timing, chain-and-deliver, shell-crack),
each with a pure headless-queryable pass predicate that doubles as an agent-run mechanic regression test.
A first slice of **meta-progression** and campaign scaffolding exists (persistent career + perk shop,
world-map + player-skin skeletons) but stays parked in "Later" — building the skeleton doesn't authorize
promoting it; the gate for pivoting to the outer loop is Carl's explicit "the core feels done" call, which
hasn't come. No new Slack signal this cycle (the v0.1.7 diary post has no reactions or replies); Carl's
standing note is still *"Would be nice to see example videos here!"* — a task for the diary agent, but it
confirms the rhythm/visual-spectacle bet is what he wants to *watch*, so keep favoring legible, watchable
moments. **All three prior "Now" items shipped** (Hermit archetype, two biome-routing mechanics, two more
tutorial scenarios). The inner loop is essentially complete on depth, so the frontier stays depth-first:
**add a crossover edge that makes catch order and train shape a live decision**, and **give the rhythm
system a between-boss expression** so the beat drives moment-to-moment play, not just climaxes.

## Bugs (fix before anything else in Now)

Stability beats new features — an agent picking a task should check here first, before any
item in "Now" below.

- None currently known. (The start-of-run `InstanceArray capacity > 0` crash and the
  windowed-instead-of-fullscreen bug are both fixed.) If you hit a panic or a wrong-looking
  frame while testing, log it here before shipping anything new.

## Now

- **A crossover edge that makes catch order and train shape a live decision. (Top Now item.)** The seven
  archetypes interact, but the *sequence* you catch them in and *where* they sit in the train barely
  matter yet. Add one mechanic that turns the train into a spatial puzzle the rhythm/archetype web feeds
  into — e.g. a **Splitter** that halves into two on catch, so grabbing it mid-combo is a Groove-Gamble
  bet; a **train position that matters** (an Armored link at the tail actually blocks a Thief's steal, a
  Dancer link pulses a small on-beat catch aura), so you *arrange* your conga rather than just growing it;
  or a **chain reaction** where catching one archetype next to another triggers a visible combo (snag a
  Golden beside a Magnet-link and the shine arcs down the train). Reuse existing verbs; make the new edge
  a legible, watchable reaction the player learns to set up on purpose.
- **Give the rhythm system a between-boss expression — the beat should drive minute-to-minute play, not
  just climaxes.** The strongest rhythm verbs (Downbeat Slam, Groove Gamble, Drum Roll, kick drum) shine
  in bursts or at bosses; the *ordinary* stretch between them leans more on chasing than on the beat. Add
  one rhythm mechanic that lives in the moment-to-moment herd, so a groove-savvy player plays the whole
  run differently — e.g. an **on-beat dash** that only fires clean on the downbeat (reward timing your
  movement, not just your catches), a **beat-phrase call-and-response** in the open field like the Reef
  DJ's but self-triggered for a herd-wide lure, or **free crabs that briefly clump on the downbeat** so
  the beat itself becomes a routing tool. Keep it legible and watchable — the beat visibly reshaping play.

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
