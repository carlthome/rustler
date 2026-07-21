# Crab Rustler

A toy game created to explore and learn Rust.

## Features

- Exciting gameplay!
- State-of-the-art graphics
- Smooth performance
- Easy to learn, hard to master
- Fun and engaging mechanics for the entire family
- ~~Multiplayer support~~

## Game terminology

- **Campaign map:** the screen for selecting a destination.
- **Arena:** a physical destination with a biome, terrain rules, music, objective, and wave set.
- **Wave:** one timed crab encounter inside an arena. A wave chooses a formation, herd size,
  duration, and location.
- **Wave formation:** the geometric arrangement used to place a wave's crabs.
- **Stage:** reserved for the arcade-style wave roles planned for the encounter loop: familiar
  waves, escalating riffs on them, then a Challenge miniboss wave. It is not another name for
  an arena.

## Run

To run the game just build and launch it from the repo root:

```sh
cargo run
```

Or, for a more reproducible environment, you can use Nix to run the game without needing to install Rust or Cargo:

```sh
nix run github:carlthome/dotfiles#rustler
```

## Develop

After cloning the repository, enter a development shell and build the project:

```sh
# Enter a development shell.
nix develop

# Build the project.
cargo build

# Run tests.
cargo test

# Launch the game.
cargo run
```

**Tip:** You can also run one-off commands directly, for example: `nix develop --command cargo check`

### Without Nix (cargo only)

Nix is optional. On a machine with Rust/Cargo already installed you can build and
playtest with cargo directly — `scripts/ci-deps.sh` installs the system libraries
(the same ones listed in `default.nix`) and configures a headless audio device so
the game builds and the bot playtests can run offscreen:

```sh
# One-time: install system libraries + headless audio (Ubuntu/Debian, idempotent).
bash scripts/ci-deps.sh

# Build, and run the bot playtests.
cargo build
bash scripts/playtest.sh
```

`scripts/playtest.sh` auto-detects Nix: it uses the dev shell when present and
falls back to plain cargo (with `xvfb` for offscreen rendering) otherwise. This is
what lets the feature-development agents run as Claude cloud routines without a
local machine.

## Release

To do a release, just git tag the code and push the tag, and a GitHub Actions workflow will build and package the game for all platforms. The release artifacts will be attached to a subsequent GitHub release with the tag.
