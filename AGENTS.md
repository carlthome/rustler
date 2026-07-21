# Crab Rustler — Agent & Developer Guide

Rust game (ggez 0.9.3), reverse Vampire Survivors: player builds a conga train of caught crabs.

**INSPIRATION.md** — read before making design decisions. Captures Carl's stated influences and design principles. Game Designer and Gameplay Engineer agents treat it as the design compass.

**ROADMAP.md** — maintained by the Game Designer agent (Cron 6). The Gameplay Engineer reads it for direction; it doesn't edit it.

## Build

See `README.md` for the current development and build instructions.

> **Note:** Run `nix develop` from the repo root before running Cargo or launching the game.
> Agents should use local checkout commands (`nix develop`, `cargo build`, `cargo run`) and avoid `nix run github:...`.

**Cargo-only environments (no Nix).** Nix is the primary path, but the remote
routine sandboxes (see the roster below) have Cargo without Nix. There,
`scripts/ci-deps.sh` installs the system libraries from `default.nix` via apt and
configures a headless null-audio device; a `SessionStart` hook in
`.claude/settings.json` runs it automatically at the start of every session (it
no-ops when Nix is present, so it's harmless locally). `scripts/playtest.sh`
auto-detects Nix and falls back to plain `cargo` + `xvfb` otherwise. In short:
whenever an agent prompt says `nix develop . --command <cmd>`, a cargo-only
session can just run `<cmd>` directly — the hook has already provisioned the
environment.

## Playtests are the ground truth — keep them green

The bot playtests (`scripts/playtest.sh`) are how we know the game still _works_, not
just that it compiles. The **Playtest** GitHub Actions workflow now runs them on every
push and PR to `main`, and it is green — keeping it green is a hard rule for every
code-writing agent (Gameplay Engineer, Performance Engineer, Build Engineer, Software Architect):

- **Run playtests before you push, every time.** `cargo build && bash scripts/playtest.sh`
  must pass locally before you commit. A change that builds but fails a playtest is a broken
  change — don't push it.
- **A red Playtest is the top-priority bug.** If `main`'s Playtest is failing, or your push
  turns it red, fixing it comes before any feature work — ahead of the ROADMAP.
- **Never disable, comment out, skip, or `|| true` a test to force a green result.** That
  hides a real regression. A commented-out `run_script` line in `scripts/playtest.sh` is
  itself a bug: re-enable it and fix the underlying game issue, don't work around it.
- **New mechanics deserve new coverage.** When you add something the bots can meaningfully
  exercise, extend a bot script so future changes can't silently break it.
- **Common failure mode — game entry paths.** Any code path that transitions into in-game
  state (menu "Play", tutorial, world-map entry, etc.) must call `reset_game()` to correctly
  initialize crabs, score, and the spawn pattern. Forgetting it causes the wave-skip bug:
  `advance_pattern()` fires immediately on an empty herd, skipping pattern 0 (crabs near
  the player) and landing on pattern 1 (far corner of the world), so the bot never catches
  anything. The `menu_to_game` bot test catches this regression.
- **Common failure mode — velocity × speed compound.** Crab position updates as
  `pos += vel * speed * dt`. If you set `crab.vel` for a special effect (whistle pull,
  flee, etc.) without also setting `crab.speed = 1.0`, the per-crab speed multiplier
  (30–120) compounds and crabs teleport across the world.

This is the whole point of running as autonomous collaborating agents: development stays
grounded in whether the game actually plays, not just whether it builds.

## Issues + labels

GitHub Issues are how you (Carl) and the Game Designer inject specific work, on top of the agents'
own schedules. Opening an issue is a **routine GitHub trigger** (the routine runs in Anthropic's
cloud like every other one) — **not** a GitHub Actions workflow; no Claude runs in CI. Each code
routine is a **hybrid** — its normal schedule plus an event trigger — and implements the triggering
issue (playtests, opens a PR with `Closes #<issue>`, which auto-merges when green).

Labels route issues to the right engineer (they match the PR auto-labels in `.github/labeler.yml`):

| Label | Wakes | Who files it |
|-------|-------|--------------|
| `gameplay` | Gameplay Engineer (cron 1) | Game Designer (from Slack + ROADMAP) + Carl |
| `build` | Build Engineer (cron 4) | Carl + automated signals (e.g. the release-failure issue) |

The **Performance Engineer** (cron 5) is the exception: instead of an issue label it wakes on each
**GitHub release publish** — a freshly shipped version is the natural moment to make it run smoother.

Only **gameplay** needs a filer to stay busy (its work is direction-dependent) — and it also runs
hourly off the ROADMAP, so an empty `gameplay` queue is fine. Build and Performance **self-discover**
their work (a slow/red workflow; a frame hitch), so their triggers are just an extra lever — no one
has to keep a queue full.

**Keep issues conflict-free:** scope each to a single subsystem (enemies, audio, rendering, spawning).
Land file-moving refactors (their own issue) before feature issues that touch the same area. The
Software Architect (cron 7) keeps files small, which makes overlapping issues far less likely to
conflict.

## File ownership (parallel agent splits)

- `ROADMAP.md` — owned by Game Designer (cron 6) only.
- The Performance Engineer (cron 5) may touch any source file but must `git pull --ff-only` immediately before editing and before pushing. It never edits ROADMAP.md.
- The Build Engineer (cron 4) owns the CI surface — `.github/workflows/*.yml`, `scripts/ci-deps.sh`, `scripts/playtest.sh` provisioning, and `[profile.*]` in `Cargo.toml`. It stays out of game source; the game agents stay out of the CI surface. This keeps the Build Engineer and the Performance Engineer from colliding.
- The Gameplay Engineer (cron 1) works one issue at a time on its own feature branch. If a concurrent
  PR touches the same file, rebase or narrow the change and note the dependency in the PR body.

Never write to the same file from two agents simultaneously.

## Commits

Short plain-English messages. (The `Co-authored-by: Claude` trailer is turned off globally via
`includeCoAuthoredBy: false` in `.claude/settings.json` — don't add it back by hand.) Always push
after committing:

```sh
git -C . push origin main
```

**Merge your green PRs.** The remote routines run on feature branches and open PRs into `main`
(the harness enforces this, opening them as **drafts**). A code-writing routine's job isn't done
when CI passes — it's done when the work is _in `main`_ — but `.github/workflows/auto-merge.yml` now
does the whole hand-off for you. It **auto-readies** any green `claude/*` draft (build + every
`playtest (...)` check passing) via the GraphQL ready-for-review mutation, then **squash-merges** it once
it's non-draft, green, and cleanly mergeable — waiting for every required check and update-branching if
you're behind `main`. So the one thing you owe is: **open the PR and get its checks green.** You don't
have to flip the draft ready or merge by hand (you still may as a nudge — otherwise the workflow does it).

Never leave a red PR: a failing check is your next task — fix and re-push, don't merge red. If your
branch conflicts (`dirty`), rebase onto `main` so the merge gate can take it. If a check is genuinely
stuck/unrelated after a couple of honest tries, say so in a PR comment rather than forcing it.

**Don't touch other roles' PRs.** `auto-merge` squash-merges any green `claude/*` PR within minutes,
so PRs rarely pile up. If you find a genuinely *stale* one (dirty/superseded) that's clearly your own
role's earlier work, close it — but never close or merge a PR that belongs to another role.

## Agent roster

All eight agents now run as **remote routines** (in Anthropic's cloud, surviving
restarts, managed at claude.ai/code/routines). No laptop or "bootstrap" needed.
The code-writing agents (1, 4, 5, 7) build and playtest with cargo in the remote
sandbox — the `SessionStart` hook provisions dependencies (see **Build** above).

Model/effort tuning is deliberate: **Opus 4.8 for decisions that compound, Sonnet 5 for code
correctness, Haiku 4.5 for mechanical work** — and cadence is kept as low as each agent's inputs
actually change, since running an agent more often than its inputs move just burns tokens on empty
runs. The table below is the intended configuration.

> **Setting these is manual, and effort isn't a routine knob.** The routines are created in the web
> UI, so an agent cannot change them programmatically (`update_trigger` refuses http_api-created
> routines) — set **model** and **cadence** by hand at
> [claude.ai/code/routines](https://claude.ai/code/routines). **Reasoning effort is NOT configurable
> per routine** (no UI control; the request for it,
> [claude-code#51549](https://github.com/anthropics/claude-code/issues/51549), was closed as not
> planned). So the **Effort** column below is realised two ways, not by a UI toggle: the model tier,
> plus a **`/effort <level>` step 0 at the top of each cron prompt** (the agent sets its own effort at
> runtime, since the routine can't). If `/effort` isn't honored in a given routine session, the model
> tier still carries the intent.

| # | Agent | Model | Effort | Cadence | Why this tier |
|---|-------|-------|--------|---------|---------------|
| 1 | Gameplay Engineer    | **Opus 4.8** | high   | **hourly + `gameplay` issue** | The engine of player-facing progress — game-feel design + code. Premium spend belongs here. |
| 6 | Game Designer        | **Opus 4.8** | medium | daily        | Direction compounds (Slack → ROADMAP). Cheap at 1 run/day; keep the judgment. |
| 4 | Build Engineer       | Sonnet 5     | medium | daily + `build` issue | CI correctness/upkeep. The big CI work has shipped; maintenance now. |
| 5 | Performance Engineer | Sonnet 5     | medium | daily + on release publish | Game runtime perf. A shipped release is the natural moment for a perf pass. |
| 7 | Software Architect   | **Opus 4.8** | medium | daily        | Shapes the codebase every other agent builds in — structure compounds, so Opus. |
| 8 | Agent Engineer       | **Opus 4.8** | medium | daily        | Shapes the pipeline all agents run on — its calls compound across the whole fleet. |
| 2 | Release Manager      | **Haiku 4.5**| low    | daily        | Pure counting + version bump; releases are now fully automated in CI. |
| 3 | Developer Diary      | **Haiku 4.5**| low    | daily        | Summarise git log + post a Slack GIF. Rote. |

Cron 1 (Gameplay Engineer) runs **hourly plus on any `gameplay` issue** — deliberately the fleet's
biggest, most frequent spend, because player-facing game-feel is what matters most. Every other agent
is at most daily, so keeping that hourly Opus run is the point; the most expensive mistake is putting
the Sonnet/Haiku agents (Perf, Build, Release, Diary) back on Opus.

Crons 4 and 5 are **siblings**: both make things faster, but 5 optimizes the _game at runtime_
(FPS, frame hitches) while 4 optimizes the _pipeline_ (CI wall-clock, build/test speed). Keep them
distinct — 4 never touches game logic for framerate, 5 never edits CI config. Both self-discover work
on their schedule; the Build Engineer also wakes on `build` issues, the Performance Engineer on each
release publish.

> Cadence note: scheduled routines fire at most hourly, and minutes are staggered so concurrent pushes
> to main don't collide. Code routines are hybrids — a schedule plus a labeled-issue trigger; the
> Gameplay Engineer is the only one that runs hourly (nobody's watching overnight, so it prefers small,
> safe, easily-reverted changes — that caution lives in its prompt).

Manage all of them at: [claude.ai/code/routines](https://claude.ai/code/routines)

Token budget principle: Opus on decisions that compound. Haiku for mechanical tasks.
Sonnet for code correctness. Don't run agents more often than their inputs change.

**DO NOT** create duplicate local crons for any of these — they're all running
remotely and duplicates will create conflicting commits.

## Worktree isolation

Local agents that write code (1, 4, 5, 7) should be spawned with `isolation: "worktree"` in the Agent tool call. This gives each agent its own isolated git worktree so they never stomp on each other's uncommitted changes or break each other's builds. The worktree is automatically cleaned up after the agent finishes (or kept if changes were made, with the branch name returned).

Without isolation, concurrent agents share the same working directory — partial lasso work breaks the flashlight agent's build, stashes pile up, conflicts occur on push. With worktrees, each agent works in a clean copy and merges/rebases cleanly when done.

Example spawn call:

```python
Agent(description="...", prompt="...", model="opus", isolation="worktree", run_in_background=True)
```

This worktree advice only applies if you run the code-writing agents locally by
hand. As remote routines, all eight run in Anthropic's cloud, each in its own
fresh sandbox with its own checkout — they're isolated by design, so no worktree
setup is needed. They still `git pull --ff-only`/rebase before pushing to reconcile
concurrent commits to main.

## How the agents work together

1. **Gameplay Engineer** (cron 1) writes game code — **hourly off the ROADMAP, and on any `gameplay` issue** Carl or the Game Designer files. The fleet's biggest, most frequent spend, on purpose.
2. **Performance Engineer** (cron 5) keeps the _game_ smooth — makes whatever landed cheap to run at runtime (FPS, frame hitches). Never touches ROADMAP.md. **Build Engineer** (cron 4) is its sibling: it keeps the _pipeline_ correct and fast — fixes silently-failing workflows, trims CI wall-clock. Never touches game logic.
3. **Software Architect** (cron 7) keeps files small and well-structured — splits files over ~500 lines, extracts shared logic, enforces single responsibility. Runs daily. Never changes game behaviour.
4. **Release Manager** (cron 2) bumps the version whenever unreleased commits represent meaningful progress — early development, so releases are frequent; CI then tags and publishes the GitHub Release automatically.
5. **Developer Diary** (cron 3) summarizes history and posts to Slack with a gameplay GIF — the feedback channel Carl actually sees.
6. **Game Designer** (cron 6) reads Carl's reactions/replies, updates ROADMAP.md, and files issues — which feed the Gameplay Engineer (step 1).

If editing a cron's prompt, check whether another cron reads its output before assuming the change is isolated.

## Cron 1 — Gameplay Engineer prompt

@agents/gameplay-engineer.md

## Cron 2 — Release Manager prompt

@agents/release-manager.md

## Cron 3 — Developer Diary prompt

@agents/developer-diary.md

## Cron 4 — Build Engineer prompt

@agents/build-engineer.md

## Cron 5 — Performance Engineer prompt

@agents/performance-engineer.md

## Cron 6 — Game Designer prompt

@agents/game-designer.md

## Cron 7 — Software Architect prompt

@agents/software-architect.md

## Cron 8 — Agent Engineer prompt

@agents/agent-engineer.md
