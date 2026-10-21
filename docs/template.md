# Template edits

`psx-bevy/` comes from the psx template (`.psx-template.toml` records the
version). Edits to it are allowed under the rules in `psx-bevy/AGENTS.md`;
this file lists them so that `psx template update` stays easy to merge.

```sh
psx template update --dry-run
psx template update
```

Each edited file that the template also changed comes back as
`<file>.psx-new`. Take the new version, reapply the change listed below (or
drop the row if the template now does it), then delete the `.psx-new`.

## Current version

Updated from `0.1.0+src-41f296d388d06491-release` to
`0.3.4+src-b72afc8e8efe891f-release`. The update added seven files and
updated fifteen under `psx-bevy/`, with no conflicts. All template-owned
files use the upstream versions; there are no local edits.

Merged the changed seeded instructions, engine asset lookup, registry
replacement APIs, workspace mod dependency and game plugin group. The
existing `GamePlugin` wrapper remains for callers; mods use `GamePlugins`
to select features. `engine` re-exports the new assets, display, pointer and
settings modules. Cargo regenerated the lockfile for the example mod.

Kept the project's disc/BIOS exclusions in `.gitignore` (the new Ghidra
exclusion was already present). Adapted the README and example mod to the
current course scene. The deleted platformer demo and its tests stay deleted;
the new template's demo changes do not apply to this game's race scene.
The example mod uses a hand-authored course override and the existing game
renderer. There is no driving controller yet, so controller reuse and its
asset contract remain work for the corresponding ported feature.

The updated port guide also requires shared original routines to stay in
shared modules, public independent feature plugins, controller asset
contracts, and editor formats that support creating content. The updated
`engine/AGENTS.md` and `game/AGENTS.md` carry these rules for future work.
This migration adds no gameplay claims or new reverse-engineering evidence.

Validation passed: `cargo test --workspace --all-features` (including
doctests), `cargo test --workspace --all-features --all-targets`,
`cargo clippy --workspace --all-targets --all-features`, formatting, and all
seven opt-in `dev_mcp` GPU tests after building the examples. The sandbox
also ran through MCP offscreen: its walker moved 1.5 m over 30 right-input
ticks. Inspected screenshots of the game, mod, sandbox and resized screen.
A final `psx template update --dry-run` reports 41 unchanged owned files,
no pending updates and no conflicts.

## Local edits

None.

## Workarounds

Template limits handled from `engine/` or `game/` without editing it, and
whether they should be raised upstream.

| Limit | Workaround | Raised upstream |
|---|---|---|
