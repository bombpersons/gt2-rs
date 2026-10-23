# Aerodynamic force and retained reduction

`vehicle::aerodynamics` ports `8003DAA8..8003DBE4`: sampled speed squared,
retained reduction growth/recovery, signed drag and separate front/rear loads.
All **266 original calls** match exactly, including 74 fresh holdout calls.
Tests compare the original intermediate square, multiplier and effective square
as well as every stored force and retained reduction. Native forward dynamics,
suspension and vertical body rules consume the resulting forces in the same
headless schedule. The existing **257 decoded-road comparisons** also now use
native aerodynamic axle loads, alongside native suspension/body correction and
late attachment/frame refresh.

The proximity producer of the reduction-growth input remains separate. Other
force channels, movement-entry velocity/position and driver-request boundaries
are still observed in these subsystem comparisons. This does not establish a
closed B-1 driving trajectory or live driving/presentation parity.

## Identity and reproduction

USA Simulation Mode Rev 2, Vitz F '99, B-1 at vblank 7500.
State SHA-256 `49fb814844da1b4d160f966c4cb675405fb58eb3d74ebc90702ad58978a2ef5b`;
RAM SHA-256 `ff147faec522bdd3da3f6231a379c4b09e323e6d094186bcb0a71a5a5441ab04`.

```sh
psx disasm --ram .psx/runs/b1-state1200 --addr 8003DAA8 --count 80 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8003E19C --count 38 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 80034480 --count 65 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 80033BB4 --count 84 --text
python3 tools/record_aerodynamics.py
python3 tools/record_aerodynamics.py --holdout
python3 tools/record_road_contact.py
python3 tools/record_road_contact.py --holdout
cargo test -p gran_turismo_2 --all-features --test aerodynamics --test vertical_body --test road_contact
```

The recorder requires complete execution and watches, no dropped trace lines,
one car and all eight aero/forward/suspension/body boundaries. It records actual
consumed speed, growth, reduction, duration, coefficients and wheel-force
inputs, plus original scratch body forces and intermediate aero registers.
Expected values come exclusively from original execution. Commands, checkpoint
hashes and toolkit identity accompany CSV/JSON in ignored
`game/tests/data/original/aerodynamics/`; traces/RAM/reports stay in `.psx/`.

## Rules and quantities

Offsets are from physics base `800A96B4`. Force units remain original internal
units; their SI newton interpretation has not been established.

| Source | Native quantity |
|---|---|
| +6A4, word /4096 | Retained contact-forward velocity, metres/s |
| +774, signed halfword /4096 | Reduction growth per second, produced by the prior proximity pass |
| +776, signed halfword /4096 | Retained reduction fraction |
| +6FE, signed halfword /65536 | Step seconds |
| Global 80046EF4, word /4096 | Minimum aerodynamic response at full reduction |
| +43C, +440/+444, words /1024 | Drag and front/rear force per (m/s)² |
| +71A, signed halfword | Signed longitudinal drag |
| +71C/+720, words | Front/rear aerodynamic loads consumed by suspension |

`8003DAC0` floors absolute speed to 1/128 m/s. Squaring its sampled integer
with a signed low-word product and shifting by twelve gives four times (m/s)².
Retain this quarter-(m/s)² sample, including low-word overflow at extreme
authored speeds. A direct unquantized floating speed square would differ.

When growth is nonzero, add floor(growth × duration) to the retained fraction
and narrow to a signed halfword **before** applying an upper-only cap at one.
When growth is zero, subtract the sampled recovery, narrow, then apply only
the zero lower bound. Recovery is **a quarter fraction per second**:
q16 seconds shifted by six produces q12 fraction. The growth and recovery
branches intentionally have different bounds; signed/overflowing histories
can remain outside [0,1]. Their consequences are compared with original runs.

Interpolate the response from one toward the minimum using the updated
reduction, preserving the signed-low-word floor. Apply that response to the
sampled square with another low-word floor, then apply each force coefficient
separately. Drag opposes the sign of contact-forward speed; its final signed
halfword store can wrap. Axle loads use full words and do not restore the
speed sign. The native component exposes the intermediate physical quantities
and the three original force outputs.

## Ordering and remaining producer

The caller at `8003E1D4` runs aerodynamic preparation after duration production
and before engine/grip/control/tyre evaluation. `80034480` later clears +774
through `8003360C` and rebuilds it through the pairwise proximity routine
`8003373C`. Thus a nonzero initial growth poke is actually consumed by aero
before the later clear. The recorder verifies the consumed value; it does not
patch instructions or disable an output writer. Natural B-1 has one car and
zero growth. The pair routine's behind-car/high-speed gates are binary evidence
for the proximity interpretation; that traffic producer itself remains unported.

`AerodynamicsRulesPlugin` joins `VehicleSystems::Prepare` before controls,
forces and suspension. It needs only `AerodynamicsInput`, `Aerodynamics` and
`VehicleStepDuration`. A present `TyreContactState` supplies retained speed.
Optional bridges write the drag field of `ForwardDynamicsInput` and the axle
loads of `SuspensionInput`. No other rules plugin is required. Per-car
`AerodynamicsSetup` overrides the default tuning, which the exporter writes
locally to `vehicle/aerodynamics.ron`. No original table or fixture is published.

## Verification coverage

| Suite | Calls | Coverage |
|---|---:|---|
| Speed probes | 25 | Forward/reverse, zero/tiny speeds, 1/128 sampling neighbors and signed low-square overflow |
| Reduction probes | 80 | Positive/negative/zero growth, recovery equality/neighbors, signed halfword histories, both caps and growth wrap |
| Multiplier/duration/coefficient probes | 17 | Seven minimum responses, four original duration producers and six signed/asymmetric force coefficient sets |
| Natural controls | 50 | Retained histories with throttle, braking and steering |
| Recovery sequence | 20 | Nonzero initial growth followed by the original proximity clear and consecutive recovery |
| Fresh holdout | 74 | Sixteen new speed/growth/history probes, 42 mixed-control calls and 16 new consecutive recovery calls |

The connected forward check uses observed tyre forces/steering/load/surfaces,
but its aerodynamic drag is produced natively with a sentinel at the input
bridge. The connected suspension/body check similarly replaces observed axle
forces with native outputs and compares individual adjusted body forces,
travel/rate histories, final velocity, tilt and compression correction.
The remaining lateral/forward body-force inputs are observed after the original
drivetrain/body stage; no full feedback-loop claim follows from this test.
The body comparison helper is shared with the unchanged 221-call body corpus.

The road corpus retains decoded polygon queries, smoothed heights, bases,
suspension and tilt history and now also retains aero reduction history.
Its eight-boundary recorder captures pre-aero inputs; original axle-load input
injection has been removed. The same 257 original road calls pass with native
aero, road, suspension, body correction and late frames/attachments together.
A handmade standalone test proves per-car coefficients, recovery, the optional
contact-speed input and force bridges without private data or other plugins.
The source-only example mod disables the unused feature.

Workspace tests, the complete local asset rebuild, native game/example-mod
MCP render checks and sandbox movement/material checks pass. Screenshots were
inspected. Clippy reports only the existing wheel-rotation test warning.
Current-source and reachable-history publication audits pass; all generated
assets, numerical fixtures and original traces remain private and ignored.

Next: close the full vehicle feedback loop and port remaining steering/force
input producers, condition accumulation and licence outcomes. Live vehicle
presentation, the wider game and engine editor remain required by `PORT_PLAN.md`.
