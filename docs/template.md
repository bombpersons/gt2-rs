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

The workspace is now on the installed `0.4.2+src-4c4c84345897a7b9-release`.
This continuation
preserved the earlier in-progress 0.4.0 template/editor migration, then applied
the current update: two vendor Cargo metadata changes, no owned-file
conflicts. Cargo resolved the editor's HTTP/MCP dependencies into the project
lockfile; the seeded generic lockfile was not copied over the game's lockfile.
The template dry run reports 68 unchanged owned files and no pending updates.
The final switch from the temporary debug build to `~/.local/bin/psx` changes
only the template version stamp; all 68 owned files are identical. Recording
scripts use the installed command by default.
The public-content audit now permits the template's exact `LICENSE-MIT` and
`LICENSE-APACHE` paths; both match the already approved vendor licences.

The 0.4.0 migration already in the working tree adds editor MCP support,
removes the old runtime raw TIM/TMD/model-viewer paths and updates vendored
offline parsers. GT2's game still reads converted native assets through its
existing importer. No new local edits were made to template or vendor source.

Workspace tests, formatting and both public-source audits pass. Clippy's
remaining suggestion is the existing nested condition in
`game/tests/wheel_rotation.rs`. The editor's stdio and HTTP/MCP tests pass;
its offscreen UI/MCP verification is recorded under `.psx/`.

## Previous 0.3.8 migration

Updated from `0.3.4+src-b72afc8e8efe891f-release` to
`0.3.8+src-0b26c73b885b1004-release`. The update changed ten files under
`psx-bevy/`, added the corridor rendering example and vendored psx-media and
psx-assets with their licenses. There were no conflicts; all template-owned
files use the upstream versions.

The rendering update stabilises affine interpolation for triangles crossing
the camera plane and computes ordinary distance fog from the per-pixel world
position. Controls and display menus now expose hooks for game-owned views;
the built-in views remain enabled here until the game's front end is ported.

Merged the editor seed into `editor/`, added its workspace dependency and
kept the engine's Bevy features aligned with it. The editor handles the
existing `LevelData` object/trigger placement format, with undo/redo and
byte-exact untouched saves. Its example path and round-trip test use the
hand-authored standalone placement example in `engine/assets/sandbox/`,
since the sandbox room wraps `LevelData` and the template demo has been removed. It does
not yet edit the racing engine's course, vehicle or tuning formats.

Merged the applicable project instructions, menu integration guidance and
future imported-directory exclusions, preserving the stricter project rules
that exclude all generated game assets. The public-source audit now permits
the editor's source and the template's vendored Rust source/licenses.

The new seeded first-launch disc-import flow is deferred: this project's
`tools/import` already contains GT2 decoders, and `tools/export_assets.py`
currently combines those with original runtime captures for tuning. The seed's
metadata-only importer would not build the assets the game requires. Kept the
existing importer, exporter, game startup and tests; the vendored libraries are
available for a later migration. Players still follow `docs/assets.md`.
The deleted platformer demo stays deleted. Existing driver-assist working-tree
changes are preserved. This update adds no gameplay or parity claims.

Validation:

- `cargo test --workspace --all-features` reached a failure in the existing
  untracked `fresh_original_driver_assistance_holdout` test
  (`game/tests/driver_assist.rs:183`): the fixture does not exercise both full
  and partial throttle cuts. The driver-assist work was left unchanged in
  this migration; the subsequent [assistance feature](driver-assist-evidence.md)
  extended the original holdout and resolved that coverage failure.
- `cargo test --workspace --all-features -- --skip fresh_original_driver_assistance_holdout`
  passed the remaining tests and doctests, including the editor's document
  round trips, nonempty authored example, and undo/redo tests.
- All eight opt-in `dev_mcp` GPU checks passed across the game, example mod
  and template, including the corridor near-plane regression. The engine
  sandbox passed its MCP movement, ramp and texture checks (1.5 m in 30 ticks).
  Inspected game, mod, sandbox and corridor screenshots. The editor's desktop
  UI was compiled but not exercised interactively in this headless session.
- `cargo clippy --workspace --all-targets --all-features --offline` completed
  with one existing `collapsible_if` suggestion in `game/tests/wheel_rotation.rs`.
- Formatting checks pass for the project-owned crates. `cargo fmt --all -- --check`
  reports upstream formatting in `psx-bevy/src/controls/mod.rs`,
  `psx-bevy/src/controls/view.rs`, `psx-bevy/src/display/mod.rs` and
  `psx-bevy/tests/dev_mcp.rs`. Kept the template files byte-identical to upstream,
  per the instruction against local reformatting in `psx-bevy/AGENTS.md`.
- Working-tree and public-history audits passed, as did `git diff --check`.
  The final update dry run reports 69 unchanged owned files, no pending
  updates and no conflicts.

## Local edits

None.

## Workarounds

Template limits handled from `engine/` or `game/` without editing it, and
whether they should be raised upstream.

| Limit | Workaround | Raised upstream |
|---|---|---|
