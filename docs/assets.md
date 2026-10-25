# Local assets and source-only distribution

This repository distributes original Rust/Python source, investigation notes,
replay instructions and handmade engine/example-mod content. It distributes
no game assets, BIOS, disc images, extracted overlays, memory dumps, saved
emulator states, generated numerical tables or original reference fixtures.
All `game/assets/` and `game/tests/data/original/` content is private and ignored.
The old development history contained imported data and must never be pushed;
the public repository starts with a fresh source-only history.
The audit also rejects known tuning-array copies formerly embedded in an
investigation note, including historical and whitespace-only copies. Describing
formats and rules is public; complete source parameter arrays stay in local
exports. The automated checks complement manual provenance review.

## Export the current runtime assets

Supply your own **Gran Turismo 2 (USA), Simulation Mode, Rev 2** disc dump and
an NTSC-U PlayStation BIOS. Install Rust/Cargo and the `psx` toolkit locally
(`psx` 0.3.2 or newer; tested with 0.3.8 and 0.4.2). The exporter downloads neither
media nor tools. Python uses only its standard library.

From the project root:

```sh
python3 tools/export_assets.py /path/to/game.cue --bios /path/to/scph5501.bin
cargo run -- --software
```

The CUE's referenced BIN must be beside it. Raw BIN and cooked ISO are also
accepted by the importer. This currently supports the identified revision
and B-1 scene, not every car, region or race mode.

The script builds the importer, replays the original from power-on into B-1
and captures local countdown and moving checkpoints, extracts and decompresses the required
course/model/paint files, then exports:

- B-1 course, collision grids/polygons, ordinary road/scenery geometry and textures;
- Vitz high-detail body, paint slot 5 and all decoded wheel detail levels;
- Checkpoint wheel placements and camera setup;
- Live wheel bindings, body-origin offset and chase-stabilization tuning;
- Native initial vehicle state with retained simulation histories, exported
  to `vehicle/start.ron` for startup and `vehicle/initial.ron` for the moving
  regression reference (see [the public spawn contract](vehicle-initial-state.md));
- Scene selection in `race/vehicle.ron`, with the observed countdown and clock.
  The normal game now holds at the line, then releases the car; hold Cross to
  accelerate. The remaining HUD, sounds and licence outcomes remain open;
- Small/medium/large bitmap-font atlases and native glyph/kerning metrics from
  `/font/racefont.dat` and the original program, plus `race/hud.ron` for
  countdown digits, the start announcement and lap clock. See
  [HUD evidence and verification](hud-evidence.md);
- `race/licence-results.ron` supplies exact result confirmation waits;
  `race/result-presentation.ron` supplies Finish/FAIL and prize animation/layout.
  Four prize-logo PNGs come from `/arcade/license_tim.tim`. See
  [result presentation](result-presentation-evidence.md);
- Gauge atlas and `race/gauges.ron` from `/arcade/game_status_files.gz`
  and the original gauge tables: selected tachometer dial, speed and gear
  sprites, needle shapes, layout, colour and RPM settings. See
  [driving-gauge evidence](gauge-evidence.md);
- Selected native licence definition, initial personal-record state and
  `race/records-panel.ron` layout, decoded from GTDT/session data and shared
  numeric/font routines. See [records-panel evidence](records-evidence.md).
  Initial record content is separate from the future native save system;
- `race/licence-progress.initial.ron` stores native saved achievements and
  qualifying-attempt counters. The selected definition also carries its
  fourth-band qualification setting. See [award rules](licence-award-evidence.md);
- `race/licence-stop.ron` supplies the selected stopping zone in metres and
  its strict speed threshold. Course distances retain precise metre values
  and sampling steps for wheel-boundary decisions. See [stopping evidence](licence-stop-evidence.md);
- `vehicle/automatic_stop.ron` supplies shared post-result steering slew,
  linear/angular settling thresholds and automatic engagement bands. See
  [automatic stopping evidence](automatic-stop-evidence.md);
- Pedal, steering, engine, geometric/longitudinal wheel-slip, brake and
  brake-assistance and ordinary traction/stability tuning, axle tyre curves, low-speed force settings, and
  body/wheel mass, surface resistance, differential/wheel-rotation tuning,
  gear ratios, engine/axle inertias, centre coupling parameters, transmission
  RPM/speed/slip bands and engagement/timer thresholds, body force lever arms,
  yaw response, lateral feedback and world-motion thresholds, load/camber grip
  curves, surface responses, optional condition parameters, inverse wheel
  radii, contact speed response and wheel-spin/readout conversion factors,
  suspension travel limits, spring/damper/preload/anti-roll responses and
  body-force transfer levers, vertical mass/tilt response and compression-stop/reset
  tuning, aerodynamic force coefficients and retained reduction/recovery,
  road-height plane-fit dimensions, procedural
  surface-bump settings and shared direction-curve thresholds.

Recorded digital driver tracks can separately be converted with
`tools/import_replay.py --ram /path/to/ram.bin --out game/assets/race/replay-inputs.ron`.
Optional `replay_inputs: Some("race/replay-inputs.ron")` in `race/vehicle.ron`
drives that scene from the native track; its initial vehicle/session boundary
must match the recording. See [replay inputs](replay-input-evidence.md).

The game reads native RON/PNG files from `game/assets/` and does not use an
emulator at runtime. The exporter's intermediate files and checkpoints stay
under `.psx/asset-export/`. `game/assets/export-manifest.json` records hashes
of generated outputs, both checkpoint identities, font-resource identity and
the selected startup mode. Unused imported text, sound
and menu archives are not required by the current runtime; their separate
import commands remain available in `tools/import/`.

To reuse existing supported moving/tuning and countdown checkpoints:

```sh
python3 tools/export_assets.py /path/to/game.cue \
  --checkpoint /path/to/driving/ram.bin --start-checkpoint /path/to/start/ram.bin
```

The full replay captures these under `.psx/asset-export/driving/` and `start/`.
The countdown checkpoint must be an ordinary single-car B-1 state before its
hold expires; other modes and partial clock ticks are rejected. Omitting
`--start-checkpoint` when reusing `--checkpoint` explicitly selects free driving
from that checkpoint and writes `session: None`. This replaces a prior scene
selection too, so a reuse run cannot accidentally keep a stale countdown.
Without `--checkpoint`, the full power-on replay always captures both states.

`--psx /path/to/psx` selects a particular local toolkit build. With the 0.4
core the exporter uses a longer menu replay, observes the first initialized
countdown, and captures immediately before it advances. It rejects loading
RAM with an uninitialized car. Older 0.3 builds retain their verified replay.
Emulator save-state formats differ across these core versions; rebuild
checkpoints from power-on instead of loading an incompatible old state.

`--assets` selects another asset output folder; `--work` selects an
intermediate folder. Project-local outputs are restricted to the ignored
`game/assets/` and `.psx/` trees. Re-running refreshes outputs and records a
new manifest. Keep generated files local.

## Source-only checks and private evidence

```sh
cargo test --workspace --all-features
cargo run -p engine --example sandbox
cargo run -p example-mod
python3 tools/check_public_repo.py --history
```

The engine and example mod use handmade assets and run without the disc.
Game tests requiring private assets/reference recordings are explicitly
reported as ignored when those inputs are absent; this does not claim
original-game parity. `game/build.rs` detects local assets/fixtures, enabling
those tests automatically when present. Missing individual inputs then fail
normally. Compile-time fixture embedding is intentionally avoided.

`tools/record_*.py` regenerates original reference recordings locally from
the documented checkpoint. The exporter saves its moving reference checkpoint
at `.psx/asset-export/driving/` and its start at `.psx/asset-export/start/`;
recorders accepting `--state` or `--checkpoint`
can use that path. For recorders using the original default location, copy
that generated directory locally to `.psx/runs/b1-state1200/` first. These remain under the ignored
`game/tests/data/original/` directory. The evidence notes describe each
recorder and its coverage. Full completion still requires the unfinished
systems listed in `PORT_PLAN.md`.

## Keeping the public repository free of generated content

`.gitignore` excludes disc/raw files, converted game assets, analysis outputs
and reference fixtures. `tools/check_public_repo.py` additionally uses an
explicit source/authored-content path list, rejects symlinks, unexpected
binary content and long encoded payloads, and audits every reachable commit
with `--history`. CI runs the history audit on pushes and pull requests.
This guard complements source/provenance review; extensions alone cannot
identify copied proprietary data hidden inside source code.

Published PNGs are the two small, handmade engine sandbox panels and the
three explicitly approved [README screenshots](screenshots.md). Those native
presentation images are a documentation exception, not runtime assets or
original-game captures. The example mod's RON geometry and tuning are
hand-authored. Do not add imports, captures or original tables to those
authored folders, or broaden the screenshot exception to generated assets.
