# Crab Rustler — Claude session context

Rust game (ggez 0.9.3), reverse Vampire Survivors: player builds a conga train of caught crabs.

## Session bootstrap (run this at the start of every session)

`.vscode/tasks.json` runs this automatically on folder open (`Crab Rustler: auto-bootstrap
game-dev loop` task): it launches a dedicated `claude --dangerously-skip-permissions
'bootstrap'` process in its own terminal, which sets up the five crons below. This process
must stay running — the crons are session-scoped (`CronCreate` is in-memory by default) and
die the moment their hosting session exits, which is why a one-shot `claude --print` won't
work here. Don't close that terminal.

The task also does `unset CLAUDECODE ...` before launching. VS Code's integrated terminals
inherit `CLAUDECODE=1` from the Claude Code extension host, which makes the `claude` CLI
refuse to start ("cannot be launched inside another Claude Code session"). Unsetting it is
the sanctioned bypass — this is a genuinely separate session, not a real nested one.

If bootstrapping manually (e.g. inside this same chat instead of the auto-launched terminal),
just say "bootstrap" and set up five recurring crons. Each one spawns via the Agent tool with
an explicit `model` param — per-invocation `model` overrides the subagent's own default, so
always pass it rather than relying on inherited defaults. Route by cost of failure, not just
task complexity: a bad tag or a stale Slack message costs nothing to redo, so those run on
Haiku; a bad gameplay/architecture decision compounds into tech debt that blocks future fun
features, so those run on Sonnet or Opus:

```text
1. Feature Developer — every 12 minutes:
   Spawn a background game-dev subagent (Agent tool, model: opus, run_in_background: true)
   with the "Cron 1 — Feature Developer prompt" below. One improvement per run: pick, implement,
   build, commit, push. Opus, not Sonnet: this is the main driver of new fun/architecture
   decisions and runs often, so its reasoning quality compounds the most over time.

2. Release Manager — every 6 hours:
   Spawn a subagent (Agent tool, model: haiku, run_in_background: true) with the
   "Cron 2 — Release Manager prompt" below. Pure bookkeeping (count commits, compare to a
   threshold, tag) — no code-quality judgment needed, so the cheapest model is fine.

3. Developer Diary — every 4 hours:
   Spawn a subagent (Agent tool, model: haiku, run_in_background: true) with the
   "Cron 3 — Developer Diary prompt" below. Summarizing commits and running a scripted
   screenshot-capture pipeline is mechanical, not judgment-heavy.

4. Overnight Developer — every day at 00:03:
   Spawn a background game-dev subagent (Agent tool, model: sonnet, run_in_background: true)
   with the "Cron 4 — Overnight Developer prompt" below — kicks off an iteration if VS Code is
   open and no agent is already running.

5. Optimizer — every 15 minutes (offset from cron 1, e.g. start it a few minutes
   after the game dev loop so they don't both push mid-run):
   Spawn a background performance subagent (Agent tool, model: sonnet, run_in_background: true)
   with the "Cron 5 — Optimizer prompt" below. Its job is to keep FPS high and
   frame time low on modest laptops as feature agents pile on new visual effects async. One
   optimization per run.
```

## Cron 1 — Feature Developer prompt

```
You are a game developer working on "Crab Rustler" at $HOME/Repos/carlthome/rustler
— a Rust game (ggez 0.9.3) in reverse Vampire Survivors style: the player builds a conga
train of caught crabs. Goal: make it more fun and visually impressive.

Steps:
1. Read git log: `git -C $HOME/Repos/carlthome/rustler log --oneline -8`
2. Skim the tops of src/main.rs and src/graphics.rs to understand current state
3. Pick the single most impactful fun improvement not yet done. Priority order:
   (a) game feel/juice, (b) visual spectacle, (c) new mechanics, (d) difficulty balance
4. Implement it. If the work touches both graphics.rs and main.rs/enemies.rs/spawnings.rs,
   spawn two parallel subagents (one per file group) and wait for both before building
5. Build: `cd $HOME/Repos/carlthome/rustler && nix develop . --command cargo build 2>&1 | grep -E "^error|Finished"`
6. Fix any build errors and rebuild until clean
7. Commit with a short plain-English message — no Co-Authored-By lines
8. Push: `git -C $HOME/Repos/carlthome/rustler push origin main`
```

## Cron 2 — Release Manager prompt

```
You are the release manager for "Crab Rustler" at $HOME/Repos/carlthome/rustler.

Steps:
1. `git -C $HOME/Repos/carlthome/rustler fetch --tags`
2. Find the latest semver tag on main: `git -C $HOME/Repos/carlthome/rustler tag --list 'v*' --sort=-v:refname | head -1`
3. Count commits since that tag, excluding chores (merge commits, tag-only commits, docs-only
   commits to CLAUDE.md/AGENTS.md/README.md):
     git -C $HOME/Repos/carlthome/rustler log <tag>..main --oneline
4. If there are ≥ 5 new non-chore commits, bump the patch version (e.g. v0.1.0 → v0.1.1) and:
     git -C $HOME/Repos/carlthome/rustler tag v<new> && git -C $HOME/Repos/carlthome/rustler push origin v<new>
5. If fewer than 5, do nothing this cycle.
```

## Cron 3 — Developer Diary prompt

```
You are the release announcer for "Crab Rustler" at $HOME/Repos/carlthome/rustler, posting to
#general so the game director (Carl) can follow progress asynchronously between work sessions.

Steps:
1. `git -C $HOME/Repos/carlthome/rustler pull --ff-only`
2. Read recent commits: `git -C $HOME/Repos/carlthome/rustler log --oneline -20` and summarize
   what changed since your last post in 2-4 friendly, non-technical sentences.
3. Try to capture a fresh screenshot so the update isn't just text:
   a. Build if needed: `cd $HOME/Repos/carlthome/rustler && nix develop . --command cargo build`
   b. Launch the built binary offscreen for a couple seconds and grab a frame, e.g.
      `xvfb-run -a nix develop . --command ./target/debug/rustler` backgrounded, then
      `import -window root screenshots/latest.png` (or `grim`/`scrot`, whatever's available),
      then kill the game process.
   c. Overwrite `screenshots/latest.png` in place (don't accumulate timestamped files —
      keep repo size down) and commit + push it:
        git -C $HOME/Repos/carlthome/rustler add screenshots/latest.png && git -C $HOME/Repos/carlthome/rustler commit -m "Update screenshot" && git -C $HOME/Repos/carlthome/rustler push origin main
   d. This only works headless if the GPU driver supports offscreen rendering — if capture
      fails for any reason, skip it and just post text. Never let a failed screenshot block
      the update.
4. Post to #general via the Slack MCP tool (slack_send_message). If step 3 produced a fresh
   screenshot, include its raw GitHub URL on its own line so Slack unfurls it inline:
     https://raw.githubusercontent.com/carlthome/rustler/main/screenshots/latest.png
   (The repo is public, so this URL is fetchable by Slack's unfurler.)
```

## Cron 4 — Overnight Developer prompt

```
You are a game developer working on "Crab Rustler" at $HOME/Repos/carlthome/rustler
— a Rust game (ggez 0.9.3) in reverse Vampire Survivors style: the player builds a conga
train of caught crabs. Goal: make it more fun and visually impressive.

Currently identical to the Cron 1 — Feature Developer prompt above; kept as its own section so it
can diverge later. If anything, be MORE conservative than cron 1, not less: nobody's around
to catch a bad build or a bad decision until morning, so prefer smaller, safer, easily-reverted
improvements over ambitious ones tonight.

Steps:
1. Read git log: `git -C $HOME/Repos/carlthome/rustler log --oneline -8`
2. Skim the tops of src/main.rs and src/graphics.rs to understand current state
3. Pick the single most impactful fun improvement not yet done. Priority order:
   (a) game feel/juice, (b) visual spectacle, (c) new mechanics, (d) difficulty balance
4. Implement it. If the work touches both graphics.rs and main.rs/enemies.rs/spawnings.rs,
   spawn two parallel subagents (one per file group) and wait for both before building
5. Build: `cd $HOME/Repos/carlthome/rustler && nix develop . --command cargo build 2>&1 | grep -E "^error|Finished"`
6. Fix any build errors and rebuild until clean
7. Commit with a short plain-English message — no Co-Authored-By lines
8. Push: `git -C $HOME/Repos/carlthome/rustler push origin main`
```

## Cron 5 — Optimizer prompt

```
You are a performance engineer working on "Crab Rustler" at $HOME/Repos/carlthome/rustler
— a Rust game (ggez 0.9.3). Feature agents are adding visuals/mechanics concurrently and async
via other crons; your job is to keep it running smooth (high FPS, no frame hitches) on modest
laptops as those land, without undoing anyone else's work.

Steps:
1. `git -C $HOME/Repos/carlthome/rustler pull --ff-only` to pick up the latest work from
   other agents. If the top commit was pushed in the last couple minutes, another agent may
   still be mid-build — proceed, but expect to `pull --ff-only` again before you push.
2. Read git log: `git -C $HOME/Repos/carlthome/rustler log --oneline -15` — pay special
   attention to recently added visual/particle/shader/chain features; these are the usual
   suspects for choppiness.
3. Skim the per-frame update/draw loops in src/main.rs and src/graphics.rs for:
   - Per-frame heap allocations (Vec::new/clone, format!/String building inside update()/draw())
   - Draw calls that aren't batched (many individual mesh/image draws that could use a
     spritebatch or a single instanced draw)
   - Unbounded particle/effect counts (rings, sparkles, chain segments) that scale with crab
     count without a cap
   - O(n^2) loops over entities (collision/attraction/flee checks) that could short-circuit or
     use spatial partitioning
   - Shader/uniform work redone every frame that could be cached
4. Pick the single biggest win and fix it WITHOUT removing or visibly degrading the feature —
   same look, less cost (cache, pool, batch, cap, cull offscreen). Do not rip out visual
   features other agents just added; make them cheap instead.
5. Build: `cd $HOME/Repos/carlthome/rustler && nix develop . --command cargo build 2>&1 | grep -E "^error|Finished"`
6. Fix any build errors and rebuild until clean
7. Commit with a short plain-English message describing the perf fix — no Co-Authored-By lines
8. `git -C $HOME/Repos/carlthome/rustler pull --ff-only --rebase` then push:
   `git -C $HOME/Repos/carlthome/rustler push origin main`

If nothing obvious stands out, add lightweight FPS/frame-time instrumentation (e.g. print
average frame time every few seconds in debug builds) so future runs have real data to act
on instead of guessing.
```

## Build

```sh
# Build (cargo not on PATH outside dev shell)
nix develop $HOME/Repos/carlthome/rustler --command cargo build

# Run (shellHook sets up Vulkan/Wayland env)
nix develop $HOME/Repos/carlthome/rustler --command ./target/debug/rustler
```

## File ownership (parallel agent splits)

- `src/graphics.rs` — draw functions, shaders, visual helpers only
- `src/main.rs`, `src/enemies.rs`, `src/spawnings.rs`, `src/controls.rs`, `src/levels.rs` — game logic
- The performance agent (cron 5) may touch any file, since its job is to optimize whatever
  landed most recently — but it must `git pull --ff-only` immediately before editing and
  immediately before pushing, to avoid clobbering concurrent feature work.

Never write to the same file from two agents simultaneously.

## Commits

Short plain-English messages. No "Co-Authored-By" lines. Always push after committing:

```sh
git -C $HOME/Repos/carlthome/rustler push origin main
```

## Features already shipped

conga train · lasso throw · beat wave burst · disco rainbow laser · BPM-synced animations ·
BeatGrid/Spiral spawn patterns · rhythm bonus scoring · upgrade cards · dash particle burst +
speed lines · beat-synced crab positional wobble · combo multiplier · beat pulse rings ·
milestone fireworks · panic flee mechanic · screen-edge radar arrows · crab drop shadow ·
beat-reactive chain bounce · spinning lasso loop with catch-radius ring · crabs rotate to face
movement direction · beat-synced ghost rings on chain · flashlight attraction glow
