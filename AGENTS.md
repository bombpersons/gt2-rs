# Working in this project

The public repository must contain no disc-derived data. Keep all generated
game assets (including converted RON/PNG/WAV and tuning tables) in ignored
`game/assets/`, reference fixtures in ignored `game/tests/data/original/`,
and emulator/analysis outputs in `.psx/`. Never commit binary overlays or
historical development Git data from `.private/`. Only authored engine and
example-mod assets may be published. The three explicitly listed README
screenshots under `docs/images/` are an approved documentation exception;
see `docs/screenshots.md`. This does not allow runtime assets or raw captures
from the original game. Run `python3 tools/check_public_repo.py
--history` before pushing. `tools/export_assets.py` rebuilds required runtime
assets locally from a user-supplied disc; see `docs/assets.md`.

A native Bevy port of a PlayStation game, built as an engine and a game from
the start. Read the guide for the crate you are working in:

- `engine/AGENTS.md`: what belongs in the engine, the placement rules, the
  sandbox, formats people can author.
- `game/AGENTS.md`: turning reverse-engineering evidence into idiomatic Bevy
  code, keeping it extensible, mods (`mods/example`), tick rate, trajectory
  tests, verification.
- `editor/AGENTS.md`: the desktop editor for engine formats, depending on
  `engine/` only. It currently edits object and trigger placements; its MCP
  supports agent edits and UI tests.
- `vendor/`: template-owned psx-media and psx-assets; fix these upstream
  rather than editing the vendored copies.
- `psx-bevy/`: the template's crate, updated by `psx template update`. Build
  on it from `engine/` and `game/`; edit it only as `psx-bevy/AGENTS.md`
  allows, and list every edit in `docs/template.md`. Its README documents
  the PS1 look, input, testing and the developer MCP.

The psx toolkit's `guide` tool (topic `port`) has the porting workflow.
Routine check: `cargo test --workspace --all-features`.
