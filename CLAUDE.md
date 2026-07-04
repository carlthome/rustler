# Crab Rustler — Claude session context

Rust game (ggez 0.9.3), reverse Vampire Survivors: player builds a conga train of caught crabs.

## Session bootstrap (run this at the start of every session)

Set up four recurring crons:

```text
1. Game dev loop — every 12 minutes:
   Spawn a background game-dev subagent (Agent tool, run_in_background: true) with the
   "game dev prompt" below. One improvement per run: pick, implement, build, commit, push.

2. Release judge — every 6 hours:
   Count commits since the last semver tag on main. If ≥ 5 new non-chore commits, create
   the next patch tag (e.g. v0.1.0 → v0.1.1) with:
     git -C /home/carl/Repos/carlthome/rustler tag v<new> && git -C /home/carl/Repos/carlthome/rustler push origin v<new>

3. Slack update — every 4 hours:
   Read the last few commits, post a short friendly summary of what was built to the
   #general Slack channel via the Slack MCP tool.

4. Midnight fallback — every day at 00:03:
   Same as game dev loop — kicks off an iteration if VS Code is open and no agent is
   already running.
```

## Game dev prompt (use for crons 1 and 4)

```
You are a game developer working on "Crab Rustler" at /home/carl/Repos/carlthome/rustler
— a Rust game (ggez 0.9.3) in reverse Vampire Survivors style: the player builds a conga
train of caught crabs. Goal: make it more fun and visually impressive.

Steps:
1. Read git log: `git -C /home/carl/Repos/carlthome/rustler log --oneline -8`
2. Skim the tops of src/main.rs and src/graphics.rs to understand current state
3. Pick the single most impactful fun improvement not yet done. Priority order:
   (a) game feel/juice, (b) visual spectacle, (c) new mechanics, (d) difficulty balance
4. Implement it. If the work touches both graphics.rs and main.rs/enemies.rs/spawnings.rs,
   spawn two parallel subagents (one per file group) and wait for both before building
5. Build: `cd /home/carl/Repos/carlthome/rustler && nix develop /home/carl/Repos/carlthome/dotfiles#rustler --command cargo build 2>&1 | grep -E "^error|Finished"`
6. Fix any build errors and rebuild until clean
7. Commit with a short plain-English message — no Co-Authored-By lines
8. Push: `git -C /home/carl/Repos/carlthome/rustler push origin main`
```

## Build

```sh
# Build (cargo not on PATH outside dev shell)
nix develop /home/carl/Repos/carlthome/dotfiles#rustler --command cargo build

# Run (shellHook sets up Vulkan/Wayland env)
nix develop /home/carl/Repos/carlthome/rustler --command ./target/debug/rustler
```

## File ownership (parallel agent splits)

- `src/graphics.rs` — draw functions, shaders, visual helpers only
- `src/main.rs`, `src/enemies.rs`, `src/spawnings.rs`, `src/controls.rs`, `src/levels.rs` — game logic

Never write to the same file from two agents simultaneously.

## Commits

Short plain-English messages. No "Co-Authored-By" lines. Always push after committing:

```sh
git -C /home/carl/Repos/carlthome/rustler push origin main
```

## Features already shipped

conga train · lasso throw · beat wave burst · disco rainbow laser · BPM-synced animations ·
BeatGrid/Spiral spawn patterns · rhythm bonus scoring · upgrade cards · dash particle burst +
speed lines · beat-synced crab positional wobble · combo multiplier · beat pulse rings ·
milestone fireworks · panic flee mechanic · screen-edge radar arrows · crab drop shadow ·
beat-reactive chain bounce · spinning lasso loop with catch-radius ring · crabs rotate to face
movement direction · beat-synced ghost rings on chain · flashlight attraction glow
