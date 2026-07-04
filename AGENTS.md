# Agent best practices for this project

## Building

Always build inside the nix dev shell — `cargo` is not on the system PATH:

```sh
nix develop /home/carl/Repos/carlthome/dotfiles#rustler --command cargo build
```

## Parallel agent workflow

Split work strictly by file so agents never write to the same file simultaneously:

- **Graphics agent** → `src/graphics.rs` only (draw functions, shaders, visual helpers)
- **Logic agent** → `src/main.rs`, `src/enemies.rs`, `src/spawnings.rs`, `src/controls.rs`, `src/levels.rs`

The logic agent depends on data-model changes (e.g. new fields on `EnemyCrab`) and new graphics
functions (e.g. `draw_conga_rope`). Agents can code in parallel because they own different files;
just run the build after both finish to catch cross-file type errors.

## Prompting agents

- Paste the current file contents into the prompt so the agent has full context.
- Give exact text replacements, not vague instructions — include the surrounding lines as anchors.
- Note borrow-checker gotchas explicitly: collect iterators into a `Vec` before mutating the same struct.
- Tell the agent which functions a parallel agent will add, so it can reference them without stubs.

## Commits and pushing

Commit as the existing git user. No "Co-Authored-By" lines. Short plain-English
messages describing the mechanic or fix, e.g.:

```
Add conga train - caught crabs follow player in a chain
Add beat system with rhythm catch bonus
Add crab eyes with directional pupils
```

**Always push after committing:**

```sh
git -C /home/carl/Repos/carlthome/rustler push origin main
```

This keeps the remote in sync so the release pipeline can tag new versions.

## Audio layers

The game supports optional layered music: place `layer1.ogg`, `layer2.ogg`, `layer3.ogg` in
`resources/` and they will fade in progressively as the score rises. The game runs fine without them.
