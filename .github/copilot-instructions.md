# Crab Rustler: Copilot Cloud Agent Guide

`AGENTS.md` is the central source of truth for repository guidance. Read and follow it first for project direction, file ownership, coordination, change discipline, commit conventions, and build and validation requirements. Read `INSPIRATION.md` before making design decisions and `ROADMAP.md` before selecting gameplay work; do not edit `ROADMAP.md` unless explicitly acting as the Game Director.

This file only records Copilot cloud-agent-specific onboarding details that are not maintained in `AGENTS.md`.

## Cloud environment notes

Nix is the canonical Linux development environment because it supplies ggez's graphics and audio dependencies. For behavior, rendering, input, or gameplay changes, run the bot playtests with CI's headless configuration:

```sh
printf '%s\n' 'pcm.!default {' '  type null' '}' > "$HOME/.asoundrc"
xvfb-run -a env LIBGL_ALWAYS_INDIRECT=1 WGPU_BACKEND=gl,gles bash scripts/playtest.sh
```

The script rebuilds the game and runs the `menu_to_game` and `groove_dash` scenarios. Bot mode still initializes a window backend, so do not run it unwrapped on a headless machine.

## Environment issues observed during onboarding

- This onboarding environment did not have `nix` installed, so the canonical commands failed immediately with `nix: command not found`.
- A direct `cargo build` was attempted as a fallback, but stopped in `alsa-sys` because `pkg-config` could not find the ALSA development package (`alsa.pc`).

Use the Nix development shell rather than changing Rust dependencies to work around missing system libraries. If Nix is unavailable, provision the CI-equivalent native dependencies first (at minimum ALSA development headers, `pkg-config`, Xvfb, and the graphics libraries in `.github/workflows/playtest.yml`), configure the null ALSA device above, then rerun `cargo build` and `cargo test`. `scripts/playtest.sh` itself invokes Nix; in a native fallback, run the built binary's `--bot menu_to_game` and `--bot groove_dash` scenarios under Xvfb instead. Record any environment-only failure separately from a code failure.
