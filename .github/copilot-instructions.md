# Crab Rustler: Copilot Cloud Agent Guide

## Project and direction

Crab Rustler is a Rust 2024 game built with ggez 0.9.3. The player catches crabs into a conga train in a rhythm-driven, reverse-Vampire-Survivors loop.

Before selecting gameplay work, read these files in this order:

1. `AGENTS.md` for repository ownership, coordination, and commit conventions.
2. `INSPIRATION.md` for the design compass: rhythm is the mechanic, tools are drum pads, and soft tool/archetype matchups must be legible.
3. `ROADMAP.md` for the current, sequenced work. Do not edit it unless explicitly acting as the Game Director.

Favor depth and readable feedback over new player verbs. Changes should make on-beat actions, the rival-conga objective, or player-visible risk more satisfying and understandable.

## Layout

- `src/main.rs` is the ggez entry point, event loop, and high-level game orchestration.
- `src/state.rs` owns `MainState` and shared game state.
- `src/enemies.rs`, `src/spawnings.rs`, and `src/levels.rs` contain crab behavior, spawning, and biome logic.
- `src/controls.rs`, `src/upgrade.rs`, `src/tutorial.rs`, `src/world_map.rs`, and `src/menu.rs` cover their named gameplay/UI systems.
- `src/graphics.rs` contains rendering and effects; `src/sounds.rs` contains synthesis and audio helpers.
- `src/bot.rs` and `scripts/playtest.sh` provide bot-driven regression playtests.
- `resources/` contains runtime art, audio, and WGSL shaders. Keep resource paths valid and do not modify vendored `vendor/winit-0.28.7` unless the task specifically requires the patched dependency.

`src/main.rs` and `src/graphics.rs` are intentionally large and frequently touched. Keep work scoped to a subsystem, avoid unrelated refactors, and check open PRs before editing shared areas.

## Build and validation

Nix is the canonical Linux development environment because it supplies ggez's graphics and audio dependencies:

```sh
nix develop . --command cargo build
nix develop . --command cargo test
```

For behavior, rendering, input, or gameplay changes, also run the bot playtests using the same headless setup as CI:

```sh
printf '%s\n' 'pcm.!default {' '  type null' '}' > "$HOME/.asoundrc"
xvfb-run -a env LIBGL_ALWAYS_INDIRECT=1 WGPU_BACKEND=gl,gles bash scripts/playtest.sh
```

The script rebuilds the game and runs the `menu_to_game` and `groove_dash` scenarios. Bot mode still initializes a window backend, so do not run it unwrapped on a headless machine. CI also performs `nix build .#packages.x86_64-linux.default` and `nix develop . --command cargo test`.

## Environment issues observed during onboarding

- This onboarding environment did not have `nix` installed, so the canonical commands failed immediately with `nix: command not found`.
- A direct `cargo build` was attempted as a fallback, but stopped in `alsa-sys` because `pkg-config` could not find the ALSA development package (`alsa.pc`).

Use the Nix development shell rather than changing Rust dependencies to work around missing system libraries. If Nix is unavailable, provision the CI-equivalent native dependencies first (at minimum ALSA development headers, `pkg-config`, Xvfb, and the graphics libraries in `.github/workflows/playtest.yml`), configure the null ALSA device above, then rerun `cargo build` and `cargo test`. `scripts/playtest.sh` itself invokes Nix; in a native fallback, run the built binary's `--bot menu_to_game` and `--bot groove_dash` scenarios under Xvfb instead. Record any environment-only failure separately from a code failure.

## Change discipline

- Preserve existing behavior outside the requested scope; use the smallest complete change.
- Update or add focused tests when changing deterministic gameplay rules. Keep bot scenarios passing for player-flow changes.
- Do not add generated build output or temporary files to commits.
- Use short, plain-English commit messages without `Co-Authored-By` trailers.
