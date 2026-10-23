# Retained B-1 vehicle feedback loop

The native rules now reproduce **120 original physics ticks** across four
natural recordings from the moving B-1 checkpoint: throttle, coast, service
braking and a fresh turning/mixed-pedal holdout, 30 ticks each. Each test
initializes the car from the first snapshot once. Every subsequent tick runs
the native plugins with retained state and the original's consumed logical
controls. No later velocity, force, load, road contact, steering, wheel speed,
engine state or pose is injected.

This establishes a closed vehicle loop for these moving, single-car B-1
sequences. It does not establish the standing start, a complete licence test,
barrier/car collision, other cars/courses or all control modes. The rendered
reference subject remains static until the verified state can be exported in
an authorable initial-state format and attached to live presentation.

## Identity and reproduction

USA Simulation Mode Rev 2, Vitz F '99, B-1, absolute vblank 7500.
The car is already moving at about 25 m/s; this is not the start line.
State SHA-256 `49fb814844da1b4d160f966c4cb675405fb58eb3d74ebc90702ad58978a2ef5b`;
RAM SHA-256 `ff147faec522bdd3da3f6231a379c4b09e323e6d094186bcb0a71a5a5441ab04`.

```sh
python3 tools/record_vehicle_loop.py --scenario gas --scenario coast --scenario brake
# Recorded after the three complete native trajectories passed:
python3 tools/record_vehicle_loop.py --scenario holdout
cargo test -p gran_turismo_2 --all-features --test vehicle_loop
```

The recorder runs the original without pokes or code changes. It requires an
unpaused, single-car checkpoint, a complete run, complete car write and input
read watches, no dropped instruction lines, all five stage boundaries every
two video fields, and every required digital input consumption. The initial
RAM is combined with the ordered original write log to reconstruct snapshots
at instruction boundaries; the native program never supplies expected data.
Consumed input flags must be digital. The physical serial-pad latency remains
outside this logical-control boundary, as in the earlier driving comparisons.

Commands, toolkit version, checkpoint hashes and snapshot paths accompany the
ignored binary fixtures under `game/tests/data/original/vehicle_loop/`.
Emulator reports, writes, input reads and traces remain under `.psx/`.
The native game does not read these raw snapshots; only test boundary code
converts their fields into native components.

| Original boundary | Native boundary | Checked state |
|---|---|---|
| `8003EC24`, before `8003E0C4` | Initial components, first tick only | Retained car, wheel, engine, gearbox, road, suspension and body state |
| `8003EC2C`, after force/step preparation | After `VehicleSystems::Forces` | Controls, slip, tyre forces, engine/gear/coupling, wheel rotation, acceleration and world velocity |
| `8003EC48`, after `80034480` | After `VehicleSystems::Integrate` | New position/heading, retained force and drivetrain state |
| `8003EC68`, after road queries | After `VehicleSystems::SampleRoad` | Decoded polygon heights, surfaces/variants, procedural offsets and retained state |
| `8003EC74`, after `8003E8E4` | After `VehicleSystems::RefreshContact` | Suspension history/load, vertical correction, tilt, body/road bases, local contact motion, tread/shaft synchronization, spin and speed readouts |

The comparisons use zero tolerance on sampled quantities, converting the native
units back to their documented original grids. They check selected physical
state, not every byte of the car record or its unported presentation/race state.
Every native capture must run and produce values. The test stops at the first
divergent tick and reports differences at all four stages, rather than reseeding
state to recover on the next tick.

## Gaps exposed and closed

The first full tick matched through forces, movement and road sampling, but
pitch rate diverged after suspension. `SuspensionInput` still held the prior
forward transfer force. `SuspensionForceTransfer` now opts a car into sampling
current `ForwardDynamics::force` and body lateral acceleration before suspension,
after horizontal body integration. The lateral input is now named
`lateral_acceleration_mps2`; its former raw +728 representation is converted
at this boundary. The reserved vertical +72C channel stays explicit.
Standalone suspension and its authored/per-car input path remain available.

After that connection, the third tick exposed an omitted ground-reference tyre
feedback channel. `8003A3E4` computes a wide, signed, truncated speed-response
product. `8003A420` stores the wheel-reference result to scratch wheel+4;
`8003A424` stores the mutually exclusive ground-reference result to scratch
wheel+8. `8004570C/710` consumes both before applying radius and brakes.
The native tyre result now exposes `ground_response_force`, and the
`TyreWheelFeedback` marker opts wheel reaction into consuming it. Without the
marker, `WheelRotationInput::auxiliary_tread_force` remains externally owned.
The wheel-reference product also uses the original wide helper semantics.
Both response channels compare against all **972 original wheel executions**,
including the separately regenerated 112-execution holdout.

The braking sequence then diverged only during late contact refresh: rear
wheels were being synchronized despite current braking. The original resets
wheel flags from allocated demand at `8003A264` and adds drive/coupling flags
at `80045BB4/BBC`; `80030748..750` permits synchronization only for zero flags.
Contact motion now uses present drivetrain output and brake demand, plus the
current `WheelSteering` angles. If those components are absent, its explicit
input path remains usable. Body brake-stop detection likewise reads current
allocated demand when present. Collision-specific wheel flags are not yet
produced by this port.

The fresh holdout combines left/right steering, throttle, service brake,
handbrake and release. It passed without changes to the native rules after
the three initial complete trajectories passed. Additional assertions then
broadened the checked state to include bases, geometric/longitudinal slip,
force channels, road metadata, wheel spin, compression and airborne state,
fuel cut, engagement and timers; all four trajectories still matched.

## Contracts and remaining work

All rules remain independently selectable feature plugins, with layered tuning
and per-car overrides where already supported. The new bridges use public
components and opt-in markers; no model or animation metadata is required by
these numerical rules. Handmade tests verify the suspension transfer and the
choice between native tyre feedback and explicit reaction inputs. Existing
standalone/mod tests remain part of the routine suite.

The full-loop test's initializer is reference-test code, not a runtime asset
format. Next is an engine-native, authorable initial vehicle description and
public spawn hook, followed by live pose/wheel/camera presentation. The example
mod must drive its handmade car through the same public pieces. Standing-start
input/gear requests, licence flow/HUD, condition/proximity producers,
collision, special controllers, analog controls and broad content parity remain
required. These short moving trajectories do not finish the port.

## Validation

`cargo test --workspace --all-features` passed, including the four continuous
recordings, both tyre response channels across 972 original wheel executions and the existing
subsystem/standalone/mod tests and doctests. The game and example mod passed
their offscreen developer-MCP checks; the engine sandbox passed its movement,
ramp and texture checks. The scene remains the static reference presentation.
Workspace Clippy completed; its existing `wheel_rotation` test still has one
`collapsible_if` suggestion. The changed crate passes formatting, and
`git diff --check`, the working-tree public-content audit and the public-history
audit passed. Original snapshots and captures remain ignored and local.
