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

**Signal.** Carl gave substantive playtest feedback this cycle (see "Now" + "Level system rethink"): (1) the
upgrade screen fires too often and reads as a flow-breaking *pause* — cut its frequency (**SHIPPED**, rising
threshold 39daa76); (2) the level title cards look cool (Control-style aesthetic) but
levels don't vary or feel impactful and happen too often — make boundaries rarer and bigger (**first pass
SHIPPED**: longer levels + per-biome archetype emphasis + threat banner, 22caa05/f83c755; terrain-hazard half
still open), and weave levels/campaign/tutorial together more holistically; (3) he wants a procedural horde / world-record leaderboard
endless mode (Diablo-style) with a beat-mastery skill ceiling (precision-platformer "play it perfectly and go
further") for longevity — this is the arcade mode already parked in "Later", now sharpened. **Both prior top
"Now" items (mid-train arrangement depth, reposition verb) shipped, and the risk/reward axis is genuinely
balanced.** The long-train-vs-bank-often tension is
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

- **Upgrade screen fires at wrong time / bugged behavior reported by Carl during playtesting —
  investigate and fix before shipping anything new.** Carl hit a wrong-feeling upgrade screen in a
  fresh playtest this cycle (fires at an odd moment and/or misbehaves). Reproduce and fix it before
  the upgrade redesign below or any other Now item — a broken upgrade flow blocks the redesign that
  depends on it.
- (The start-of-run `InstanceArray capacity > 0` crash and the windowed-instead-of-fullscreen bug
  are both fixed.) If you hit a panic or a wrong-looking frame while testing, log it here before
  shipping anything new.

## Now

- **NOTE — MECHANICS FREEZE (called by Carl this cycle).** Carl: "We might have sufficient game mechanics
  content for now, and should work on strengthening what we have to make the player feel agency and control."
  Do **not** add new mechanics — no new crab archetypes, no new player verbs/tools, no new parallel systems —
  until Carl explicitly lifts the freeze. Any such idea goes to "Also on our mind", not into work. The two live
  Now items below (biome terrain hazard, perfect-on-beat payoff) survive the freeze: they *deepen and polish
  existing* mechanics rather than adding new ones, which is exactly the work Carl is asking for. Deepen and
  polish what exists; make the player feel in control of it.

- **Redesign the upgrade screen from "more of everything" to a meaningful choice.** Offer 3 random upgrades per
  screen (pick 1), where each upgrade meaningfully reshapes how the next few minutes play rather than just
  increments a rank. Tradeoffs and specialization over pure power addition. See INSPIRATION.md Vampire Survivors
  and precision platformer notes. This is the NEXT item after the upgrade bug (see Bugs) is fixed — it reshapes
  the *existing* upgrade system (consistent with the mechanics freeze), it does not add a new one.

- **~~Make the middle of the train matter — mid-train arrangement depth.~~ SHIPPED** — adjacency pairs pay
  a banked bonus with a glowing rope segment + ARRANGED xN callout (d68f252), and the SANDWICH bonus rewards
  a crab flanked by two matched figureheads (78623b5). Mid-train is no longer inert.
- **~~Reposition-your-train verb.~~ SHIPPED** — X cycles the whole train one slot on the beat (13be12e), a
  rhythm-gated setup move that preserves match-run bonds. The train is now a live instrument to tune.
- **~~Fix upgrade-screen frequency.~~ SHIPPED** — the buggy `score % 10` trigger is replaced with a rising
  threshold (first upgrade at 25 banked points, +15 each after, reset on new run; 39daa76), so upgrades land
  noticeably rarer and never skip. A later bolder fix — a non-blocking pick that doesn't freeze the run — stays
  unbuilt; the frequency cut may be enough on its own. Revisit only if Carl still reads upgrades as a flow-break.
  **REOPENED (this cycle):** the frequency cut was NOT enough — Carl still hit a wrong-feeling upgrade screen in
  playtest (see Bugs), and separately flagged the upgrade *content* as shallow ("more more more"). Both are now
  live: the bug is logged in Bugs, and the content problem is the redesign item at the top of Now below.
- **~~Rarer, bigger level boundaries — first pass.~~ SHIPPED** — levels are lengthened so boundaries land rarer
  (22caa05), each biome now carries a dominant crab archetype (Water→Magnet, Rock→Armored, Kelp→Thief) plus a
  threat banner on the title card, and the redirect is eased to ~33% so zones stay buildable (f83c755). The
  *who-you-catch* half of the gear-change landed; the terrain-hazard half is the next item.
- **Pair each biome with its own terrain hazard — finish the gear-change. (Next step of the level rethink.)**
  Boundaries now shift *who you catch* (archetype emphasis) but the ground is still mostly a tint. Per
  INSPIRATION's Control note, a boundary should read as arriving somewhere mechanically: give each biome a
  distinct terrain mechanic that changes routing (some already exist — Rocky Shore tide-shortcuts, Neon Kelp
  funnel lanes — so extend/assign the pattern per biome) so archetype shift + terrain hazard land *together*.
  Depth, not breadth: reuse the existing hazard systems rather than inventing parallel ones. Still NOT the
  endless/procedural/leaderboard rework — that stays gated in "Later" until Carl calls the inner loop done.
- **Make perfect on-beat play pay off dramatically, not marginally — a legible skill ceiling inside the inner
  loop.** Per INSPIRATION's precision-platformer note: a player who nails every beat should score *much* further
  than one who ignores rhythm, and the gap should be visible. The scaffolding exists (PERFECT catches, groove
  meter, on-beat multipliers, streak tiers) — deepen one of these so flawless on-beat play compounds noticeably
  (e.g. a sustained-perfect streak that ramps scoring/reach and shows how far ahead it puts you). This is depth
  inside the existing loop, NOT the gated arcade/leaderboard mode — build the mastery, not the scoreboard.

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

- **Desktop level — fourth-wall terrain.** A special level with a transparent/borderless window
  where the game reads the user's desktop pixels as terrain and treats their OS window borders as
  solid walls/platforms crabs must navigate around. The secret mechanic: players can reshape the
  level mid-run by dragging their browser, Finder, or other windows — not documented anywhere, just
  discovered. In the spirit of old Windows desktop toys and Inscryption-style reality-breaking
  moments. Technically: macOS/Wayland screen-capture API for the background grab, transparent ggez
  window layer, luminance threshold → terrain collision map. Deliberately deferred — needs OS-level
  screen capture permission UX to be unobtrusive. File under "the most delightful thing in the
  whole game when someone stumbles on it."

## Blocked (needs a human, not a code agent)

- **The soundtrack builds with the groove** — the `layer{1,2,3}.ogg` progressive-fade hook exists in
  code (main.rs loads them at startup) but no audio files populate it, so it's inert. This needs
  someone to actually author/source three stacking music layers and drop them in `resources/`; a
  headless dev agent can't compose them. Wiring them to the Groove meter once they exist is trivial.
  Parked here so feature agents stop bouncing off it — pick it up when Carl provides the stems.

## Also on our mind (not sequenced — no urgency, just don't lose it)

- **Level system rethink — direction for when the full rework lands in "Now".** Carl's playtest feedback pulls
  in one direction: levels should feel like *travelling somewhere that matters*, less often but with more
  impact, and the whole levels/campaign/tutorial split should be woven together more holistically. Context for
  a future Feature Developer, not a spec:
  - **Fewer, more impactful biome shifts.** Lean into the Control-style floor-banner aesthetic (Carl likes the
    font/transition), but make each boundary a genuine gear-change — new biome + new terrain hazard + new
    enemy-archetype emphasis together, not a tint swap. Rarer transitions each carrying real weight beats
    frequent shallow ones.
  - **Beat-mastery skill ceiling.** Carl wants the "if you play the mechanics PERFECTLY you go much further"
    payoff of precision platformers, built around the *beat*. The endless path should reward flawless on-beat
    play with escalating scoring/reach, so mastery visibly pays off — easy to learn, hard to master.
  - **Procedural horde / leaderboard endless mode.** Carl explicitly likes a Diablo-style procedurally-generated
    horde mode with a world-record leaderboard for longevity. This is the *same* arcade mode already parked in
    "Later" (leaderboards, scoring pressure, ruthless difficulty) — treat it as one vision, not two: the
    endless mode IS the mastery ceiling above.
  - **Campaign / tutorial / endless coexist without being one mode.** Threaded, not merged: the hand-authored
    campaign (world-map nodes) is the "learn the game" path most players finish once and funnels into endless;
    the tutorial sandboxes teach individual verbs; the procedural endless/horde mode is the prestige game to
    master. Weave them so they hand off cleanly rather than duplicating each other.
  - **Sequencing note.** The upgrade-frequency fix and the "rarer, bigger boundaries" step (both in "Now") are
    the safe, buildable slices of this now. The full endless/leaderboard rework stays deferred until the inner
    loop feels done (Carl's call) — don't promote it to "Now" ahead of that gate.

- **Playful bonus rounds** — Carl's Street Fighter II / Lion King (SNES) itch: a rare, surprising
  mini-challenge dropped into a run purely for spice (not for balance or progression) — a bonus
  catch-everything sprint, a rhythm-only gauntlet, something silly and short. Parked here rather
  than in "Now" since it's a side-system/breadth item by nature, same category as alternate game
  modes — worth revisiting once the core loop itself feels done.

- **NPC conga ecology (agar.io + Rain World) → multiplayer endgame.** Carl's vision: King Crabs
  have their own conga trains of followers; they steal crabs from the player's train and from each
  other. The beach becomes a living ecosystem of competing conga leaders, not just a static arena.
  The player starts as the smallest and must out-arrange (not just out-catch) larger NPC trains.
  Sequencing: (1) NPC conga trains for King Crabs; (2) train-stealing interactions between NPC
  and player trains; (3) ecology emerges from simple per-creature rules à la Rain World (see
  INSPIRATION.md); (4) multiplayer where human Rustlers compete for the largest train and thus
  the dominant audio share. Size is legible from across the field — a well-arranged shorter train
  should beat a larger, poorly-arranged one (arrangement depth matters more than raw length).

- **Spatialized audio + bring-your-own-music (competing DJ mode).** The dominant conga train's
  music takes up the bulk of the audio mix; smaller/losing trains fade. Each approaching NPC
  King Crab train is *heard* before seen — their music gets louder as they near, like agar.io
  circles creeping in from the edge. BPM detection already exists. In multiplayer: each player
  supplies their own track, the game syncs to it, and the mashup is a natural consequence of
  competition — the winner's music overwhelms the mix while losers' tracks fade to silence.
  Spatialized sound means audio IS the radar. Defer until NPC ecology is fun against bots first.
