# Gran Turismo 2 native Bevy port

A standalone Rust / **Bevy 0.20.0** workspace for a native port of a
PlayStation game, created by `psx template create`. This public repository
contains source and handmade examples only: **no disc-derived assets or
original reference fixtures are distributed**. After local asset export,
the native game doesn't need an emulator, a disc or the psx toolkit at runtime.
Requires Rust 1.97.1+ and a
desktop GPU driver (or Mesa lavapipe with `--software`).

Use your own USA Simulation Mode Rev 2 disc and NTSC-U BIOS to generate the
current B-1 runtime assets. The exporter needs a locally installed `psx` CLI;
it downloads neither media nor tools. See [asset setup](docs/assets.md).

```sh
python3 tools/export_assets.py /path/to/game.cue --bios /path/to/scph5501.bin
cargo run                                # the game (default member: game/)
cargo run -- --software                  # force a software adapter
cargo run -- --region pal                # override REGION (display + tick rate)
cargo run -- --controls                  # open the controls menu (F1 also opens it)
cargo run -- --display                   # open the display menu (F2 also opens it)
cargo run -- --fullscreen                # borderless fullscreen (F11 toggles)
cargo run -- --aspect window             # screen width follows the window's shape
cargo run -p engine --example sandbox    # the engine's own tiny game
cargo run -p example-mod                 # a mod: a tiny game built on game/
cargo test --workspace                   # private-data checks report ignored if absent
cargo run --features dev-mcp -- --mcp --headless --software  # developer MCP, offscreen
cargo test --workspace --all-features    # routine agent check (dev-mcp everywhere)
cargo build --release                    # shipping build; developer MCP is excluded
```

The sandbox and example mod run from handmade assets without exporting any
game data. Generated files under `game/assets/`, original fixtures and
analysis outputs are ignored; CI audits the complete public Git history.

## Layout

| Path | Owner | Contents |
|---|---|---|
| `psx-bevy/` | psx template | PS1 look, ticks and input, controls and display menus, testing, viewers, developer MCP, launcher. Updated by `psx template update`; edit sparingly and list edits in `docs/template.md` ([AGENTS.md](psx-bevy/AGENTS.md), [README](psx-bevy/README.md)) |
| `engine/` | this project | This game's engine family: object type and action registries, update-order sets, level and tuning formats, `examples/sandbox.rs`. No names, numbers or assets from this game ([AGENTS.md](engine/AGENTS.md)) |
| `game/` | this project | This title: per-type controllers, action handlers, HUD, menus, baked assets in `game/assets/`, trajectory tests ([AGENTS.md](game/AGENTS.md)) |
| `mods/example/` | this project | An example mod: a small game built on `game` from hand-made assets, proving the game can be extended without editing it ([game/AGENTS.md](game/AGENTS.md), "Mods are crates") |
| `tools/import/` | this project | Offline importers: files extracted with `psx` → engine formats, with provenance |
| `docs/` | this project | `engine-model.md` (what the original treats as data vs code), `evidence.md`, `template.md` (edits to `psx-bevy/`) |
| `.psx-template.toml` | psx template | Which template version this is and which files it owns |

The game renders the B-1 licence course from decoded road-node polygons,
13 placed scenery meshes and 32 imported texture materials. Its bumper,
near/far chase and rear-look camera rules use original recordings, with
R1 cycling views and L1 looking back. Race logic is 30 Hz with 60 Hz
presentation. Road textures switch by original projected-area rules, and
flat-textured road quads use the original subdivision rules.
Model LOD, other primitive refinement, procedural elements, backgrounds,
mirror and intro/replay cameras remain unfinished.
The decoded Vitz body and procedural wheels now appear in chase view, with
hand-authored replacements in the example mod. Live wheel animation,
reflections and shadows remain open.
Verified pedals, digital/per-wheel steering, geometric and longitudinal slip, brake
assistance/allocation, per-wheel tyre forces, surface resistance and forward
integration before drivetrain coupling, wheel reaction/differential rotation,
engine torque/fuel cut, engine/clutch/gear/centre/axle coupling and
velocity-to-step integration have original recordings; the coupled force
model and playable driving session are still unfinished. See
[the evidence](docs/evidence.md), [cameras](docs/camera-evidence.md), [road subdivision](docs/subdivision-evidence.md), [texture detail](docs/material-variant-evidence.md), [course meshes](docs/course-mesh-evidence.md)
and [drivetrain coupling](docs/drivetrain-evidence.md) for coverage and remaining work.

`game::GamePlugins` exposes the feature plugins so another crate can disable
or replace them. [The example mod](mods/example/README.md) reuses the course
renderer and driving-camera controller with a hand-authored circuit and
camera tuning at the same asset paths. The mod's
`assets/` takes priority over the game's fallback folder for both RON and
Bevy asset loads. No imported content is needed to run the example. It will
grow to exercise the real driving controller when that controller is ported.

## Template updates

```sh
psx template update --dry-run   # what would change
psx template update             # replace unmodified template files
```

Files under `psx-bevy/` that you edited are left alone and the new version is
written beside them as `<file>.psx-new`; reapply your change from
`docs/template.md` to it. Files the template only seeded
(everything outside `psx-bevy/`) are never touched; when their template
version changed, the update reports a diff to apply by hand.
