# Crab Rustler

A toy game created to explore and learn Rust.

## Features

- Exciting gameplay!
- State-of-the-art graphics
- Smooth performance
- Easy to learn, hard to master
- Fun and engaging mechanics for the entire family
- ~~Multiplayer support~~

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
