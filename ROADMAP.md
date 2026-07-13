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

**Where we are.** The inner loop is deep, closed, and has a spine, a real climax, and now genuine
routing hazards. Four catching tools (beam/lasso/whistle/stomp) with four upgrade playstyle lanes,
the conga train with a chain-snap downside and a delivery-pen jackpot, and rhythm/groove scoring
that **drives real mechanics**: bar-quantized spawn drops on the downbeat, on-beat PERFECT tool hits,
a beat-stepping train, an on-beat Call that Dancers answer for a Dance Catch bonus, a full-meter
**Downbeat Slam ultimate** (G on the beat), and a **Groove Gamble** risk/reward layer with a real
**cash-out decision** — press B to bank the live multiplier into a safe floor before an off-beat
miss wipes it, banking on the beat locks the whole stack. Pacing is not flat: a **staged difficulty
ramp** climbs through named intensity stages, with Frenzy spikes and a **beat-tempo shift** that
speeds the whole run up. Biomes **play differently** — Rocky Shore chokepoints, Neon Kelp tail-snag,
Tide Pool wade-drag. **Six enemy archetypes** now: base plus Armored → stomp, Dancer → rhythm,
Magnet → routing, **Thief → chain pressure** (latches to the tail and peels links until you deal
with it) and **Golden → chase decision** (rare shiny that bolts fast and pays a big lump sum). **Three
bosses now rotate as arena-shifting set-pieces**: a boss catch clears the herd for a focused duel, and
below 40% health the boss enrages and reshapes the *space* — King Crab cracks the floor into
beat-pulsing hazard fissures that bite the tail, Tide Boss floods extra wade-drag water, and the new
**Reef DJ** turns its whole fight into rhythm: a call-and-response duel where it flashes a hot-beat
phrase each bar and its shell only drains when you land the light on a *called* beat, so all three
climaxes now hit different notes (route, dodge, groove). A first slice of **meta-progression** is
in (persistent career + banked-crab perk shop). Slack momentum stays positive with no pushback;
Carl's one fresh reply this cycle is still the diary-post note — *"Would be nice to see example videos
here!"* — a task for the Developer Diary agent (cron 3) to capture short clips/GIFs, not a game-
direction change, but it confirms the rhythm/visual-spectacle bet is what he wants to *watch*, so keep
leaning into legible, screenshot-worthy moments. Both prior "Now" items shipped this cycle: the
**rhythm-duel boss** (Reef DJ, above) and a whole run of new **emergent archetype crossovers** — well
past "one more": Dancer on-beat hops now chip Armored shells, jolt latched Thieves loose, trip fleeing
Goldens, and thump free Magnets into a pull surge; a snared Golden supercharges its captor Magnet into
a herd vacuum whose field grinds an Armored shell open; a passing Golden's shine lures both roaming
Magnets and latched Thieves off their targets. The archetype web is now dense and clearly the game's
signature fun. The inner loop is essentially complete on depth — we still want Carl's explicit "the
core feels done" call before pivoting to outer-loop work, so the frontier stays: give the *archetypes*
a reason to matter inside boss fights, and give the rhythm layer one more move the player actively drives.

## Bugs (fix before anything else in Now)

Stability beats new features — an agent picking a task should check here first, before any
item in "Now" below.

- None currently known. (The start-of-run `InstanceArray capacity > 0` crash and the
  windowed-instead-of-fullscreen bug are both fixed.) If you hit a panic or a wrong-looking
  frame while testing, log it here before shipping anything new.

## Now

- **Make the archetypes matter *inside* boss fights. (Top Now item.)** Right now bosses clear the herd
  for a clean duel, so all the fun archetype interactions we've built go quiet exactly when the fight
  peaks. Bring one archetype into the arena as a fight mechanic: e.g. the Reef DJ spawns Dancers that
  echo its called phrase (herd them onto the hot beat to help crack the shell), the King Crab's charge
  can be baited into a parked Armored crab to stun it, or a Magnet on the floor lets you slingshot a
  Golden into the boss for burst damage. This is the natural next depth step — it fuses our two strongest
  systems (bosses + the archetype web) instead of running them in separate rooms, and it's a legible,
  watchable moment for the videos Carl wants.
- **One more player-driven rhythm move.** The rhythm layer mostly *modifies* things you'd do anyway
  (on-beat hits, Slam, Gamble bank). Give the player one fresh verb they choose to time — e.g. a
  two-beat "drum roll" hold that charges then releases a bigger beam sweep, a syncopated off-beat feint
  that dodges a boss telegraph, or a beat-chained catch combo that rewards catching *on consecutive*
  downbeats. One concrete addition that deepens the groove the player actively performs, not another
  passive multiplier.
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
