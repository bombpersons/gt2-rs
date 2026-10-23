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
(`psx` 0.3.2 or newer; tested with 0.3.4). The exporter downloads neither
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
and captures a local checkpoint, extracts and decompresses the required
course/model/paint files, then exports:

- B-1 course, collision grids/polygons, ordinary road/scenery geometry and textures;
- Vitz high-detail body, paint slot 5 and all decoded wheel detail levels;
- Checkpoint wheel placements and camera setup;
- Native initial vehicle state with retained simulation histories, exported
  to `vehicle/initial.ron` (see [the public spawn contract](vehicle-initial-state.md));
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

The game reads native RON/PNG files from `game/assets/` and does not use an
emulator at runtime. The exporter's intermediate files and checkpoints stay
under `.psx/asset-export/`. `game/assets/export-manifest.json` records hashes
of generated outputs and checkpoint identity. Unused imported text, sound
and menu archives are not required by the current runtime; their separate
import commands remain available in `tools/import/`.

To reuse an existing supported checkpoint:

```sh
python3 tools/export_assets.py /path/to/game.cue --checkpoint /path/to/ram.bin
```

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
the documented checkpoint. The exporter saves its reproduced checkpoint at
`.psx/asset-export/driving/`; recorders accepting `--state` or `--checkpoint`
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
