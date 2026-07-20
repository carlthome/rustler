# Changelog

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
