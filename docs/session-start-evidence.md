# B-1 countdown and driving clock

The optional native session now holds a car through the original start gate,
releases it on the original tick and advances the driving clock after physics.
Three natural recordings add **380 original physics ticks** to the retained
vehicle-loop comparisons: countdown with throttle (120), countdown to idle
(120), and a fresh mixed-control launch holdout (140). All four vehicle stage
boundaries, the countdown and the clock match exactly, with one vehicle
initialization per recording. Combined with the earlier moving and idle-start
recordings, this covers **820 unique original physics ticks**.

This is the ordinary, single-car B-1 session on USA Simulation Mode Rev 2.
Countdown/start numerals and the displayed lap clock are now covered by
[HUD presentation evidence](hud-evidence.md). Intro cameras, sounds, remaining
HUD, licence conditions/outcomes and retry/reset selection remain unported. The normal
asset export now selects the real countdown initial state through native scene
setup; the moving initial state remains a separate regression reference.

## Original boundaries

| Rule | Evidence |
|---|---|
| B-1 initializes the start hold to 120 fields | `800159A4`, stored at `800159D0` to `800A9520`; observed at vblank 6427 |
| Subtract the actual field step, saturating at zero, before physics | `80015CE0..CFC`; step byte `801D5864` is two in these runs |
| Countdown resets the car's world motion and engagement request | `8003E0C8..120`, physics+718; ordinary request producer `8002F92C` |
| Skip pose integration while the countdown is nonzero | `8003EC2C..44` skips `80034480` |
| Skip suspension/vertical correction while held; continue road sampling and late contact feedback | `8003E8E8..928`; composed loop records both sides |
| Advance the global clock only after the countdown reaches zero, after physics/contact/stabilization | `8003D168..1DC`, called at `8003EC80`; writes `80046F64` |
| A two-field tick adds 100 clock units; one unit is 1/3000 second | `8003D1CC..1D8`; B-1 HUD divides by three at `8002CCF8..CD4C` |

The initial countdown checkpoint is vblank 6550, just before the first active
physics call. Countdown values after the first, 59th and 60th native ticks
are 118, 2 and zero. The first 59 clocks stay at zero; the 60th advances to
100 original units. This preserves the original decrement-before-release
order rather than adding a second independent launch delay.

The original screenshots at survey fields 300 and 420 show countdown `2` and
`START` with the clock running. The physics clock is an exact integer count;
the HUD also adds a small periodic phase from `8002F864`. That display phase
is now applied by the HUD rules, separately from the session's physics clock.
Other race-mode gates in `8003D168` are outside this B-1 scope.

## Reproduction

Use the transmission-dialog checkpoint produced by the asset exporter:

```sh
psx run --from-state .psx/asset-export-full/transmission/state.bin --frames 250 \
  --input '[{"frame":10,"buttons":["CROSS"],"until":16}]' \
  --save-state --out .psx/runs/b1-countdown-checkpoint --summary
python3 tools/record_vehicle_loop.py --state .psx/runs/b1-countdown-checkpoint/state.bin \
  --frames 240 --prefix countdown- --scenario gas --scenario coast --session
# Recorded after the first two native trajectories passed:
python3 tools/record_vehicle_loop.py --state .psx/runs/b1-countdown-checkpoint/state.bin \
  --frames 280 --prefix countdown-fresh- --scenario launch-holdout --session
cargo test -p gran_turismo_2 --all-features --test vehicle_loop
cargo test -p gran_turismo_2 --all-features --test dev_mcp \
  native_countdown_to_launch_replays_original_requests -- --ignored --nocapture
```

The fresh holdout changes brake, throttle, reverse, handbrake and steering
across the countdown/release boundary. It passed without a further rule change.
Expected values come only from original execution; no original state is
injected after the first native initialization. Recordings keep the source
commands, state/RAM hashes, toolkit version, complete watch reports and
non-truncated traces in ignored local folders. The recorder observes every
effective control byte afresh per tick and the session clock immediately
after its original writer.

## Native API and content setup

`race::session::SessionRulesPlugin` is listed in `GamePlugins` and can also run
alone without vehicle assets, rendering or `RacePlugin`. It does nothing until
the caller inserts a `DrivingSession` resource. That resource stores a
`DrivingPhase` and elapsed 30 Hz ticks; `elapsed_millis()` performs the exact
integer display-boundary conversion without accumulating floating-point drift.
It does not create a success/failure state machine.

`SessionParticipant` opts a vehicle into the session's reset, suspension and
motion gates. `VehicleInitialState::spawn` includes `VehicleMotionGate`;
vehicles assembled manually can supply it too. Unmarked vehicles retain
ownership of their subsystem inputs. Marked vehicles currently use the
ordinary controller mode; special car modes and reset/retry producers remain
separate work. The session clock joins the engine's Objects phase after
stabilization, while the start gate joins Control before vehicle preparation.

`RacePlugin` can load optional layered `race/vehicle.ron` content:

```ron
(
    initial_state: Some("vehicle/start.ron"),
    session: Some((phase: Countdown(fields_left: 120), elapsed_ticks: 0)),
)
```

`vehicle/start.ron` must be a native initial state at the start, authored by
the content pack or converted offline from the user's original checkpoint.
The optional setup is a `RaceVehicle`; inserting that resource directly takes
precedence over the file. Without the setup file/resource, existing content
keeps its default `vehicle/initial.ron` and free driving. The scene marks its
spawned vehicle as a participant when a session resource is present. Mods may
also insert the resource and marker themselves without replacing the vehicle
controller or editing game code.

The real-binary test copies native assets into a temporary content root,
converts only the countdown initial state offline, writes this ordinary RON
setup, and replays the consumed original controls through normal buttons.
All three final position axes and the gear match exactly. The resulting
`/tmp/gt2-start-countdown-gas-22/after-launch.png` was inspected; the car is
driving along the decoded course. The native process loads no emulator state.
The handmade mod independently verifies a held start, clock release and launch
using its own assets and unchanged vehicle controller.

Validation passed: workspace tests with all features, all three native driving
MCP replays, the handmade mod's rules/GPU checks, the engine sandbox check,
formatting and the public-repository history audit. Workspace Clippy passed
with the existing `wheel_rotation` test's `collapsible_if` warning. Original
fixtures, emulator outputs, converted runtime assets and new screenshots all
remain private and ignored.

## Default scene export

A full `tools/export_assets.py` run now branches from the original transmission
dialog to capture `start/` at vblank 6550, while preserving the separate
`driving/` capture at vblank 7500. A fresh power-on run under
`.psx/default-start-export/` reproduced both earlier RAM checkpoints byte for
byte. Tuning and the historical moving `vehicle/initial.ron` still use the
driving capture; startup uses converted `vehicle/start.ron` plus
`race/vehicle.ron`. The manifest records both RAM hashes and `startup` mode.

The exported scene setup and start state independently reproduce all three
countdown recordings at every vehicle stage and clock boundary. This adds
initialization/selection coverage to the same 820-tick corpus, not additional
unique original executions. The normal binary, launched directly against
`game/assets/`, passes `native_default_scene_starts_from_the_original_countdown`
and matches the final original launch position exactly. Its before/after
images under `/tmp/gt2-default-start-276/` were inspected. That test performs
no offline conversion or temporary scene override. Moving-scene rendering and
body-origin checks explicitly select a copied free-driving reference setup.

A fresh exporter-only folder under `.psx/default-start-reuse/assets/` also
passed a direct native MCP launch replay, with identical final position and
an inspected image at `.psx/default-start-clean-runtime/launch.png`. This
checks that older files left in the development asset tree are not supplying
missing runtime dependencies. All 168 entries in the development export's
manifest were hash-checked; the fresh folder contains only its own export.

Both checkpoint-reuse paths were exercised through the complete exporter:
providing `--start-checkpoint` selects the countdown; omitting it writes
free-driving setup and clears a prior session selection. Generated runtime
content is the same for the reused checkpoints; provenance sidecars retain
their actual intermediate source paths. See [asset export](assets.md) for the
commands. The handmade mod explicitly ships its own free-driving setup and
continues to override the base game's scene selection.

The workspace test suite, all five native MCP checks and the handmade mod's
GPU check passed after the default selection changed. Clippy passed with the
existing wheel-rotation test warning; formatting, Python syntax and the public
history audit passed. No generated asset or new screenshot is published.
