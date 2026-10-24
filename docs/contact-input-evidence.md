# Grip preparation and late contact motion

Two original producers are now native rules:

- `tyre_grip.rs`: `80039A4C..80039DE4`, deriving grip from condition-adjusted
  load, camber and surface before slip/control processing.
- `contact_motion.rs`: `800304DC..8003091C`, projecting corrected world
  motion into body/wheel velocities, deriving slip response, synchronizing
  eligible wheel/axle speeds and advancing wheel spin after movement/collision.

All **380 original car calls (1,520 wheel executions)** match exactly,
including a fresh 54-call holdout. Native grip feeds both slip stages and
tyre-force preparation; contact velocities, body forward speed, wheel spin,
maximum speed and condition scale/indicator carry across consecutive calls.
The motion refresh feeds the **next** tick, preserving the original order.

Suspension/road-contact loads, surfaces, roll/orientation and the corrected
world/drivetrain state between the force phase and late refresh remain
observed inputs. The condition accumulator's later producer is also separate.
These are connected subsystem comparisons, not a closed collision/driving
trajectory. The visible car/wheels still use checkpoint attachment poses.

## Identity and reproduction

USA Simulation Mode Rev 2, Vitz F '99, B-1 at vblank 7500.
State SHA-256 `49fb814844da1b4d160f966c4cb675405fb58eb3d74ebc90702ad58978a2ef5b`;
RAM SHA-256 `ff147faec522bdd3da3f6231a379c4b09e323e6d094186bcb0a71a5a5441ab04`.

```sh
psx disasm --ram .psx/runs/b1-state1200 --addr 80039A4C --count 231 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 800304DC --count 285 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8003E1E4 --count 27 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8003EB48 --count 31 --text
python3 tools/record_contact_inputs.py
python3 tools/record_contact_inputs.py --holdout
cargo test -p gran_turismo_2 --all-features --test contact_inputs
```

Traces, RAM and metadata remain under `.psx/runs/contact-inputs-*`;
numerical CSV/JSON remains in ignored `game/tests/data/original/contact_inputs/`.
The recorder requires complete execution/watches, zero dropped trace lines,
one car and all five boundaries: before/after grip (`80039AB0/39D9C`), before
motion (`800304DC`), after projection (`80030698`) and after synchronization/
spin (`8003090C`). It records actual load-curve inputs, load-curve outputs,
camber responses and the grip consumed by the tyre-force routine. Commands,
checkpoint hashes and toolkit identity accompany the corpus.

## Grip rules and public data

Offsets use physics base `800A96B4`; wheels start at +460, stride 68 hex.

| Source | Native data |
|---|---|
| Wheel+8/+14 | Contact load in original internal units / surface index |
| Axle descriptors +224/+2FC | Signed-word load-to-capacity curves |
| Axle descriptors +250/+328 | Signed-halfword camber-to-response curves, /4096 turn and response |
| +368/+36A | Front/rear camber offsets, /4096 turns |
| +646 minus +6F6 | Roll difference before per-wheel side sign |
| +348..366 | Sixteen signed surface responses, /256 |
| Wheel+34 | Final capacity in original force units |
| Wheel+64/+38/+3F | Condition accumulator, load scale (/4096), signed indicator |
| Global 80046F48/54/5C | Condition maximum, cold limit and warm start |
| Global 80046F4C/58/60 | Maximum/cold/warm load loss, /4096 |
| +78D bit 0x10 | Bypass condition processing and load scaling |

The physical units of load, force and the condition accumulator are not
established as SI/temperature/wear. Names preserve the proven operations.
B-1's condition maximum is zero, disabling that path. Controlled recordings
enable it with authored thresholds to test every branch; actual race-mode
condition settings and accumulator production remain future work.

If condition processing is active, update its scale/indicator and clamp
accumulator at the upper limit. From zero to warm start, interpolate warm
loss; from warm start to maximum, interpolate warm-to-maximum loss. Negative
values through the inclusive cold limit use cold loss. Below that limit,
`80039C8C` restores full scale and the -128 indicator: extrapolating the
neighboring curve would be wrong. If disabled/bypassed, leave retained scale
and indicator untouched **and ignore the retained scale for load**.

Condition-adjusted load uses the signed low multiply word, floored by 12.
The load curve clamps endpoints and truncates the interpolated ordinate
delta. Add the per-side roll difference to axle camber, take its absolute
value, then narrow to a signed halfword before the camber curve. That order
is observed in wrap probes. Apply camber and surface responses with separate
low-word products; the final shift is 8, not 12. This source boundary differs
from the following tyre-force response multiplication.

`TyreGripTuning` / per-car `TyreGripSetup` contain curves, offsets, surfaces
and optional condition parameters. `TyreGripInput` supplies load/surface/roll
and bypass state. `TyreCondition` owns the persistent scale/indicator and
explicit accumulator input. `TyreGrip` exposes capacity and intermediate
results. Optional bridges update `TyreContactState.grip_capacity`, normalized
`BrakeAssistInput.grip_response`, and `ForwardDynamicsInput` load/surface.
No assistance/force/forward plugin is required to run the grip rule.

## Contact-motion rules and public data

| Source | Native data |
|---|---|
| +628/+62C/+630 | Corrected world velocity, /4096 m/s; Bevy order (x,z,-y) |
| +68C/68E/690, +694/696/698 | Local forward/lateral projection axes, /4096 |
| +6A4/+6A8 | Refreshed body forward/lateral velocity, /4096 m/s |
| +64C | Yaw rate, /524288 turns/s |
| +0C/+0E/+18/+1A | Front/rear lever arms, /4096 metres |
| Wheel+2C/+30/+2A | Local contact forward/lateral velocity and slip speed response |
| Wheel+0C/+18 | Steering heading and tread velocity |
| Wheel+63 ==0 | Permission to synchronize tread to local rolling velocity |
| +37A/+37B ==0 | Independent differential: refresh axle even without an eligible wheel |
| +3E8/+3EA | Inverse wheel radius, /4096 per metre |
| +634/+638 | Axle speed, /4096 rad/s |
| Wheel+20 | Spin angle, /4096 turns, wrapped into [0,1) |
| +6AE/+6F8 | Unsigned halfword speed readout and maximum readout |

Project each world component separately: forward products use wide flooring,
whereas lateral products use the signed low word and arithmetic shift
(`8003052C/53C`). Convert yaw to rad/s with the source immediate and its own
sampled boundary. Add/subtract lever-arm velocities to obtain each wheel's
local axes. Speed response uses absolute local forward velocity: above its
strict speed limit select full response; otherwise apply the source's two
sampled gain steps and base response. Exact limit/adjacent probes pass.

Only a zero **whole flag byte** allows tread synchronization. Project that
wheel's contact velocity through its steering basis using separate wide
truncations. Refresh an axle when either wheel synchronized or its
differential is independent; otherwise retain its existing shaft speed.
The signed pair mean truncates before inverse-radius multiplication.

Spin advances from tread velocity times duration with flooring, a low-word
radius product with flooring, then the source's signed-biased turn conversion.
Wrap into [0,1). Collapsing those multiplications changes signed spin.

The speed readout is `floor(abs(forward_q12) * 0xDFBDD / 2^24)`, narrowed to
a halfword; the second halfword retains its unsigned maximum. The producing
stores at `80030594/598` disprove earlier RPM-related labels for +6AE/+6F8.
Final HUD formatting remains separate. Historical driving fixtures keep old
unused column labels; new recordings use correct speed names and widths.

`ContactMotionTuning` / per-car `ContactMotionSetup` contain lever/radius,
conversion and speed-response parameters. `ContactMotionInput` supplies
corrected projection axes, wheel heading and logical synchronization gates.
`ContactMotion` owns spin/readouts and exposes local body velocities and
which axles refreshed. Required `WheelLocalVelocities` and `WheelSurfaceMotion`
receive fresh contact velocities/response/treads. Optional bridges update
`AxleDrive`, `TyreContactState.body_forward_mps`, `SteeringKinematics` forward
speed and `BodyDynamics.local_lateral_mps`.

The new `VehicleSystems::RefreshContact` follows integration and the future
collision correction stages. `ContactMotionSystems::Refresh` runs there.
`TyreGripSystems::Prepare` runs before geometric slip and control input.
The original call sites `8003E228/234` and `8003EB80/EBB8` establish the
separation: moving refresh before controls would use the wrong tick.

## Verification and remaining boundaries

| Suite | Calls | Coverage |
|---|---:|---|
| Load curves | 15 | Both axles' knots, adjacent and intermediate loads, endpoint clamps |
| Camber | 24 | Both signs, curve knots/adjacent samples and unequal axle offsets |
| Surface table | 16 | Each grip-table entry; no claim that every index is used by a real course |
| Camber narrowing | 5 | Signed-halfword wrap after absolute value |
| Condition | 56 | Enabled/disabled and bypass combinations, all thresholds/adjacencies, retained unequal scales and indicators |
| Wheel/axle synchronization | 72 | Both drive layouts, each free wheel's zero/no-contact/braking gate, all independent-axle combinations |
| Speed response / yaw / spin | 18 | Signed zero/limits/adjacencies, seven yaw rates, spin around wrap boundaries |
| Natural baseline | 120 | Forty calls each of gas, braking and turning |
| Fresh holdout | 54 | Six asymmetric grip states, eight mixed world/yaw states and forty alternating-control calls |

Tests compare all intermediate grip values and condition state, every native
tyre-force channel, local body/wheel velocities, speed responses, tread/axle
speeds, spin and both readouts. Natural sequences retain native local motion
for the next slip/force tick, spin, maximum speed and condition scale/indicator.
The gap between tyre forces and late refresh remains an explicit observed
world/tread/shaft boundary, covering the existing drivetrain and unported
collision/orientation code rather than substituting a closed-loop model.

Interventions are checked at the original inputs. Synchronization flags are
rewritten at `8003A264` and by drive layout at `80045BA4..C10`; the recorder
changes load/brake/layout producers and verifies consumed free-wheel flags.
It does not treat a flag poke overwritten by the game as a valid probe.
The fresh holdout was recorded after implementation and did not change the
native rules. Standalone handmade tests prove each rules plugin without
private assets, renderer, controls or other vehicle plugins, and exercise
their optional bridges. The source-only example mod disables both features.

Workspace tests, native game/example-mod MCP render checks and the engine
sandbox pass; screenshots were inspected. No live wheel-animation parity is
claimed. Clippy retains only the existing wheel-rotation test warning.
Source/history publication audits pass with all generated data ignored.

Next: road sampling/suspension load, surface and orientation production,
collision correction, the condition accumulator producer and a closed B-1
trajectory, then live wheel animation and the playable session. The full
game, alternate torque controllers, induction, audio, licence flow and editor
remain unfinished.
The seed run's load producer at `80043A74` is now ported, together with
travel sampling and anti-roll/body-force preparation. See
[suspension evidence](suspension-evidence.md) for 221 original comparisons,
next-tick load carry and the remaining road/orientation/collision boundaries.

## Current drive/brake synchronization

When drivetrain output and brake demand are present, contact motion now derives
its synchronization gate from their current wheel flags/demands, and uses
current `WheelSteering` angles. It also retains the pre-force unloaded-wheel
gate from `8003A250..264`: a zero contact load sets flag bit 0 and prevents
tread synchronization. The gate is sampled before forces, rather than from
the new suspension load computed later in the tick. `ContactMotionInput`
exposes `unloaded_wheels`; the optional suspension bridge supplies it.
Explicit inputs still work without these
components. The [retained B-1 loop](vehicle-loop-evidence.md) verifies braking,
release and turning without injecting late wheel/axle speeds each tick.
Collision-specific flags remain outside this completed boundary.

A sustained right turn from the standing-start countdown exposed this gate
at tick 132, when the right rear wheel became unloaded. The original retains
its tread speed; resetting it changed the retained axle speed and later path.
`countdown-right-start` now matches all **734** uninterrupted original ticks
in all four physics stages, stopping just before the off-road result. The
authored contact test separately changes load between the early and late
stages to prove that the captured gate belongs to the correct tick.

```sh
python3 tools/record_vehicle_loop.py --state .psx/default-start-export/start/state.bin \
  --frames 1468 --prefix countdown- --scenario right-start --session
cargo test -p gran_turismo_2 --all-features --test vehicle_loop \
  original_standing_start_with_continuous_steering
```
