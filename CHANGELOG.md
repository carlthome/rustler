# Changelog

## v0.39.0 — 2026-07-21

### Features
- Rework Q into a space-clearing shockwave
- Add Wave/Call to the tool roster with useful-glow

### Polish
- Give the King Crab clash an on-beat audio cue (win/loss sting)

## v0.38.0 — 2026-07-21

### Features
- Lock gameplay music to a melodic pirate groove
- Unify biome-linked bosses and campaign progression

### Performance
- Cull off-screen crabs and NPC train bodies before drawing

### Refactoring
- Extract the steal/deflect subsystem into chain_steal.rs

### Fixes
- Don't complete a campaign node when you lose the level

## v0.37.1 — 2026-07-21

### Performance
- Cache the chain bond/sandwich crab-index lookup by chain_count

### Refactoring
- Extract game lifecycle & scene transitions from main.rs into game_lifecycle.rs
- Split npc_trains.rs: extract the train draw pass into npc_trains_render.rs

## v0.37.0 — 2026-07-21

### Features
- Groove chord: tap Space on the beat, hold a tool to flavor it

## v0.36.0 — 2026-07-21

### Features
- Add Hermit King and Dancer King boss encounters

### Performance
- Avoid a per-frame String allocation for the campaign goal HUD line

### Refactoring
- Split game_render.rs: extract screen-space HUD pass into game_render_hud.rs
- Extract NpcCongaTrain state into its own module

## v0.35.2 — 2026-07-21

### Fixes
- Simplify X cycle verb: one clear keyboard action instead of mouse-dependent behavior

## v0.35.1 — 2026-07-21

### Fixes
- Fix campaign Escape handling: return to main menu instead of quitting

## v0.35.0 — 2026-07-21

### Features
- Enhance NPC King Crab AI with two-phase hunt behavior: stalking → committed intercept

## v0.34.0 — 2026-07-21

### Features
- Player-anchored beat-keeper ring: keep the beat legible while steering
- Intensify postprocessing color
- Pause menu music on world map

### Refactoring
- Extract bot steal-scenario staging out of npc_trains.rs

## v0.33.5 — 2026-07-21

### Performance
- Hoist per-hit blend-mode toggling out of tool-match draw loops

### Refactoring
- Extract conga-rope and lasso tether rendering from graphics.rs into lasso.rs

## v0.33.4 — 2026-07-21

### Fixes
- macOS window: HiDPI logical size + size-to-monitor at startup

### Refactoring
- Extract world-map/minimap/tool-roster HUD draws from graphics.rs into map_hud.rs
- Extract rhythm/combo/wave HUD draws from graphics.rs into hud_indicators.rs

## v0.33.3 — 2026-07-21

### Performance
- Draw level-title overlay rects from cached unit-square mesh instead of per-frame GPU buffers

### Refactoring
- Extract per-crab body rendering from graphics.rs into crab_draw.rs
- Extract archetype-aura & cleave-effect draws from graphics.rs into auras.rs

## v0.33.2 — 2026-07-21

### Fixes
- Deterministic bot playtests: fixed timestep + seeded RNG

### Refactoring
- Extract particle system from graphics.rs into particles.rs

## v0.33.1 — 2026-07-21

### Performance
- Batch the last unbatched archetype-aura rings (Thief/Splitter/Golden/Armored)

## v0.33.0 — 2026-07-21

### Gameplay
- Beat indicator shows bar position with a punchy downbeat

### Refactoring
- Extract beat-feedback ring/pulse effects from graphics.rs into rings.rs
- Split cron prompts out of AGENTS.md into agents/ files

## v0.32.0 — 2026-07-21

### Features
- Upgrade ggez 0.9.3 to 0.10.0 (wgpu 29, winit 0.30, rodio 0.22)

### Gameplay
- Tool roster pads pulse on the beat

## v0.31.0 — 2026-07-21

### Gameplay
- Make the King Crab clash a legible on-beat move
- World map: allow skipping ahead to any node with a soft warning

### Refactoring
- Extract weather/environment backdrop rendering from graphics.rs into weather.rs
- Extract the per-frame update tick from main.rs into src/game_update.rs
- Extract per-frame audio mixing from main.rs into src/audio_mix.rs

## v0.30.6 — 2026-07-21

### Gameplay
- Widen on-beat window for ranged tool casts (whistle, stomp, beat-wave, lasso), keep dash and catch tight per player feedback

## v0.30.5 — 2026-07-21

### Fixes
- Fix npc_vs_npc playtest flake (deterministic forced rival-vs-rival cross)

### Refactoring
- Extract the per-beat handler from main.rs into src/beat.rs

## v0.30.4 — 2026-07-21

### Gameplay
- Rebalance King Crab names toward pirate and crab-rave flavor

### Refactoring
- Extract catch-and-deliver loop from main.rs into src/catch_deliver.rs
- Extract startle/panic-contagion effects from main.rs into src/startle.rs

## v0.30.3 — 2026-07-21

### Fixes
- Fix steal_defense/steal_dodge playtest flake (chain starvation in bot_prime_chain)

## v0.30.2 — 2026-07-21

### Performance
- Cache flashlight HUD label instead of rebuilding it every frame

## v0.30.1 — 2026-07-21

### Fixes
- Fix flaky steal playtests (deterministic chain, followers, and steal-back geometry)
- Widen the on-beat window for the defensive steal parry

## v0.30.0 — 2026-07-21

### Features
- Deepen the music with chord progression and singable hook; fix beat-lock timing
- Rework upgrade screen from a world-freeze into a live real-time overlay

### Fixes
- Guard empty InstanceArray draws to fix second-tutorial capacity>0 crash

## v0.29.0 — 2026-07-20

### Performance
- Count NPC train followers when deciding crab LOD tiers, fixing detail drops on large stolen trains

### Refactoring
- Extract update_crabs into its own module to reduce main.rs from ~6800 to ~4975 lines

## v0.28.0 — 2026-07-20

### Features
- Per-rival spatial music: each King Crab train broadcasts a beat-locked motif that swells with its train size

### Fixes
- Fix flaky campaign_tutorial: beat-time the autopilot's final catch approach

## v0.27.0 — 2026-07-20

### Features
- Major crab visual overhaul: per-archetype silhouettes, scuttle gait, articulated claws, expressive eyes, LOD
- Lasso auto-aims at the nearest catchable crab on release

### Performance
- Avoid unconditional per-crab sqrt in the herd update loop

### Fixes
- Fix menu_to_game bot test — re-enable it and fix the three root causes

### Refactoring
- Extract scene rendering from main.rs into game_render.rs

## v0.26.0 — 2026-07-20

### Features
- Add beat-reactive conga rope ribbon with risk heat
- Lasso vs Big crab strong-match tell: amber cinch-and-heave burst
- Add discoverable Desktop level: window-rectangle terrain and threshold entry

### Performance
- Skip the trail accumulation pass when it's a no-op
- Avoid sqrt in per-follower steal-range checks

## v0.25.0 — 2026-07-20

### Features
- Rival-vs-rival steals snap on the beat, not on random leader crossings
- Add groove-scaled conga trail / echo-afterimage layer
- Add a swung hi-hat layer locked to the game beat clock
- Richer, beat-reactive crab shells: rim outline, shaded dome, squash and claw-snap on the downbeat
- Beam pins fleeing Sneaky crabs — the flashlight exposes the skittish evader

### Refactoring
- Extract terrain/biome ground rendering from graphics.rs into a submodule

## v0.24.0 — 2026-07-20

### Features
- Whistle-vs-Thief strong-match tell: snap the tail-parasite off on the beat
- Rival-vs-rival hunt telegraph: read the impending clash and swoop the spoils
- Whistle-vs-Sneaky strong-match tell: flush the skittish crab out on the beat
- Beam spotlights fleeing Golden crabs — the flashlight's soft-RPS match against the prize

### Refactoring
- CI: drop dead docs-only waiver from auto-merge
- CI: unblock docs PRs (step-gate instead of paths-ignore) + build labels

## v0.23.0 — 2026-07-20

### Features
- Audible rival-vs-rival steals: play distinct sounds when NPCs steal from each other's trains

### Fixes
- CI: check out the tag in the release job so gh can publish the release correctly
- CI: open a tracking issue when a release fails to publish
- Steady the flaky defense playtests (more robust bot assertions)

### Refactoring
- CI: skip build/playtest/nix workflows on pure-Markdown PRs to save CI minutes
- CI: auto-label PRs by content and by which agent opened them

## v0.22.0 — 2026-07-20

### Features
- Developer Diary now records real gameplay GIFs by screen-recording the e2e bot under Xvfb

### Fixes
- Fix rival name-plate cache thrashing across multiple NPC trains (HashMap-keyed cache)
- Fix release creation — drop unsupported --repo flag, self-heal unreleased tags

### Performance
- Avoid per-crab sqrt in NPC train catch/steal range checks

### Refactoring
- CI: fire tag-and-release after auto-merge, not just on push

## v0.21.1 — 2026-07-20

### Performance
- Speed up magnet cluster detection: one crab pass instead of one per magnet
- Cache the tool roster's HUD meshes and text instead of rebuilding every frame
- Batch minimap dots into one InstanceArray draw call
- Fold King Crab splice-target search into the existing per-crab snapshot pass

### Refactoring
- Make releases and draft-merging fully autonomous in CI

## v0.21.0 — 2026-07-20

### Features
- Rival-vs-rival splicing: bigger NPC trains steal from smaller ones
- Rivals steer toward the nearest smaller rival to hunt it
- Rival-vs-rival collisions spill catchable crabs the player can swoop in and rustle

### Performance
- Speed up magnet cluster detection: one crab pass instead of one per magnet
- Cache the tool roster's HUD meshes and text instead of rebuilding every frame
- Batch minimap dots into one InstanceArray draw call
- Fold King Crab splice-target search into the existing per-crab snapshot pass

## v0.20.0 — 2026-07-20

### Features
- Add movement dodge as a second steal defense
- Dodging a rival steal opens a counter-steal window
- Rival trains telegraph a hunt before they arm a steal

### Refactoring
- Extract player tool/ability actions from main.rs into player_tools.rs
- Add auto-merge workflow to drain the bot-PR queue

## v0.19.0 — 2026-07-20

### Features
- Parry a rival steal to open a counter-steal window
- Revenge window: turn a rival's steal into a back-and-forth duel
- Beat-synced DEFEND telegraph ring on armed rival steals

### Fixes
- Cap a single rival steal to a recoverable bite, not a train-wipe

## v0.18.0 — 2026-07-20

### Features
- Rival NPC trains deliberately route to thread the back half of your chain
- Make defending a rival steal a real on-beat play
- Add distinct steal stings to the core steal moment

### Performance
- Trim ci-deps.sh apt install to packages Cargo.lock actually links

### Fixes
- Fix apt cache never saving in CI
- Fix git tag generation with `--notes-from-tag` and `--generate-notes`

### Refactoring
- Split catch reward / boss arena effects out of main.rs into catch_effects.rs
- Split NPC train simulation and rendering out of main.rs into npc_trains.rs

## v0.17.0 — 2026-07-19

### Features
- Add generative groove engine with kick/snare drums and walking bass
- Flashlight mechanic: auto-target nearest King Crab with charge meter
- Control-style level title cards with slide-in animation and desaturation postprocess
- Enrich three-zone world with procedural textures (grass tufts, pebbles, animated water)
- Add electric piano voice to groove lead with FM-style synthesis
- Spatial audio for King Crab boss with distance rolloff and stereo pan
- Make crabs more detailed with asymmetric claws, antennae, and eye catch-lights
- Rewrite synth themes with Game Boy / Deus Ex two-voice arpeggio architecture

### Performance
- Optimize postprocess shader by caching params and only updating uniforms per frame
- Support cargo-only builds without Nix (provisioning via apt for headless CI)
- Fix Playtest CI to work without Nix dependency

### Fixes
- Fix flashlight NDC coordinates and crash when rendering cone
- Fix postprocess shader to properly convert NDC coordinates
- Fix vertex/fragment shaders for correct screen coverage
- Fix music BPM sync and improve groove phrasing
- Enhance king crab audio with tippy-tappy shell clicks and mandible chitter
- Hide mouse cursor on game window
- Properly bind scene_image texture through ShaderParamsBuilder

### Refactoring
- Bump rand dependency from 0.9.1 to 0.9.3
- Add VS Code launch config for debugging
- Move level info to debug text panel
- Balance electric-piano lead frequency to match square-wave RMS
