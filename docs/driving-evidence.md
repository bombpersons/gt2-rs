# Vehicle controls and integration

The port has a verified **subset** of car rules: digital throttle, brake
and handbrake travel, speed-dependent digital steering and per-wheel steering,
and integration of explicit translation/yaw steps. The
force, tyre, drivetrain and collision models are still
unfinished. Replaying a measured displacement tests integration; it does not
establish that the port can calculate that displacement from the controls.

`game/src/vehicle/mod.rs` exposes those rules as ordinary Bevy components and
`VehicleRulesPlugin`. A future force model writes `MotionStep` in
`VehicleSystems::Forces`. The step builder consumes `VehicleVelocity` and
`VehicleStepDuration`; without either those inputs or a supplied step the
car does not move. The
plugin runs at **30 Hz**, matching the application's race logic cadence.
Presentation remains 60 Hz. The static native subject does not yet receive
vehicle motion from a complete force model.

## Source identity and reproduction

Original: Gran Turismo 2 (USA), Simulation Mode, Rev 2, Toyota Vitz F '99 in
B-1, AT, from `.psx/runs/b1-state1200/state.bin` at absolute vblank 7500.
The checkpoint route is in `docs/checkpoints/README.md`. This is a moving
checkpoint, around 25.05 m/s, not the standing-start line.

- State SHA-256: `49fb814844da1b4d160f966c4cb675405fb58eb3d74ebc90702ad58978a2ef5b`.
- Its RAM SHA-256: `ff147faec522bdd3da3f6231a379c4b09e323e6d094186bcb0a71a5a5441ab04`.
- Recorder: `tools/record_driving.py`, CLI
  `psx 0.3.4+src-b72afc8e8efe891f-release`.
- Numeric recordings and exact commands:
  `game/tests/data/original/driving/*.{samples.csv,ticks.csv,json}`.
- Raw read watches and reports remain under `.psx/runs/driving-*`. The
  recorder requires complete logs and every required input/delta for each
  exported tick; it fails if the original leaves the active control path.

Regenerate the six initial scenarios, then the held-out scenario:

```sh
python3 tools/record_driving.py --scenario gas --scenario coast --scenario brake \
  --scenario left --scenario right --scenario handbrake
python3 tools/record_driving.py --scenario mixed_holdout
cargo test -p gran_turismo_2 --all-features --test driving
```

The first six scenarios run 60 video fields each (30 physics ticks). The
mixed scenario runs 120 fields (60 ticks) and was first recorded **after**
writing the rules. Its simultaneous/alternating pedals were not used to
derive the implementation. Defaults keep steering runs short because a
long held turn fails this licence test and stops accepting live controls.

For the static evidence, use the checkpoint RAM, not another overlay:

```sh
psx disasm --ram .psx/runs/b1-state1200 --function 8002FB18 --text
psx disasm --ram .psx/runs/b1-state1200 --function 80030330 --text
psx disasm --ram .psx/runs/b1-state1200 --function 80033E6C --text
psx disasm --ram .psx/runs/b1-state1200 --function 8003EBF0 --text
```

The Ghidra RAM analysis is cached at
`.psx/ghidra/6079f305775d90a897af9b385e40f59a0157de7f1a92f471f08ab3a339101b12/analysis.json`.
Its explicit code range is `8002E000-80046D00`, seeded at `8003EBF0`,
`8002FB18`, `80039FC8`, `80030330`, `80034480`, `8003DAA8`. Query results
for those functions are retained beside that manifest. Generic MIPS
decompilation has load-delay/GTE limitations; the rules below are checked
against instructions and execution, not pseudocode alone. The range bounds
this investigation; resident arithmetic helpers outside it were disassembled
separately. `psx disasm --function` can truncate leaf/early-return functions
(notably `8003E0C4`), so the runtime trace is also needed for update order.

## Fields and corrections to earlier interpretations

All offsets below use the physics base `car = 0x800A96B4`. The containing car
record starts at `0x800A9688`, has stride `0xB40`, and adds `0x2C` for physics.

| Offset | Observed role | Units / source |
|---|---|---|
| `+0x60C`, `+0x60E` | Steering command and response rate | signed halfwords; 4096 units/turn and 4096 units/(turn/s); digital spring at `8002FCB0..8002FE20` |
| `+0x46C`, `+0x4D4`, `+0x53C`, `+0x5A4` | Per-wheel steering | signed halfwords; 4096 units/turn, front left/right then rear left/right; `8003E340..8003E408` |
| `+0x610` | Throttle travel | signed halfword, 4096 = full; `80030008..80030044` |
| `+0x612` | Brake travel | signed halfword, 4096 = full; `800300A8..80030158` |
| `+0x614` | Handbrake travel | signed halfword, 4096 = full; `80030178..800301F4` |
| `+0x628/+0x62C/+0x630` | World velocity `(x,z,y)` | signed words, 4096 units/m/s; used by step builder `80030330` |
| `+0x648` | Heading | signed halfword, 4096 units/turn; integrated by `80033ED0..80033F10` |
| `+0x64C` | Yaw rate | signed word, 524288 units/(turn/s), inferred dimensionally from the step builder dividing its product with 16.16 dt by `2^23` into 1/4096-turn steps |
| `+0x65C/+0x660/+0x664` | World position `(x,z,y)` | signed words, 4096 units/m; `80033EAC..80033ECC` |
| `+0x6A4/+0x6A8` | Forward/lateral velocity components | signed words, 4096 units/m/s; `+0x6A4` starts at 102605, close to the opposite world-x velocity on this straight |
| `+0x6AC`, `+0x6AE`, `+0x6F8` | Integer RPM, separate RPM-related state, display RPM | first two are separate halfwords, not a 16.16 word; see [dynamics corrections](vehicle-dynamics-evidence.md) |
| `+0x6FE` | Physics integration duration | signed halfword, 16.16 seconds; 2184 = 0.0333251953125 s at this checkpoint |

The old interpretation of `+0x6A4/+0x6A8` as RPM was wrong. `8003CEC8..CEF4`
calls `80075BF4(v,v)` on each component and adds the results. That helper is
a signed product shifted by 12, **not an identity**. Thus the comparisons
against 2845/7905 are on a sum of squared velocity components. The purpose
of these thresholds and the downstream flags has not been established.
They are not evidence for an automatic gear-change RPM band.

Likewise `+0x45D` was previously called the gear. In `8003E270..8003E2CC`,
value 0 selects live control processing, while value 2 takes another path.
The exact control-mode enum is still open; the evidence does not identify
this as a gear number. The earlier 16.16 interpretation of the word at
`+0x6AC` was also wrong: the original writes a halfword, with separate RPM
state at `+0x6AE`. Finding velocity components also means an earlier exhaustive failed
search for a scalar dash-speed value cannot establish that the dash works
by differentiating position. The actual dash-speed calculation is open.

## Digital controls

The pad route is `8003C250` -> `80013EF0` -> `80013C90`. It produces a
12-byte normalized control record passed to `8002FB18`:

| Input record offset | Meaning established here |
|---|---|
| `+0` | Input format flags: bits 1/2/4 select analog steering/throttle/brake paths; zero in these digital runs |
| `+2` | Signed steering request: +2 for LEFT, -2 for RIGHT, 0 at rest |
| `+4` | Brake request: 1 while SQUARE is held |
| `+6` | Throttle request: 1 while CROSS is held |
| `+10` | Handbrake request: 1 while CIRCLE is held |

The record is transient at stack address `801FF750` on this route. The
recorder reads it **at the consuming load instructions**, filtered by PCs
`8002FCB0`, `8002FCC4`, `8002FFF4`, `800300A8`, `80030178`. A frame-end
snapshot of the stack would not establish these inputs.

Digital throttle/brake each move by **1365/4096** of full travel every
physics tick, then clamp to 0..1. They take four ticks to reach the opposite
limit: 0 -> 1365 -> 2730 -> 4095 -> 4096. Replacing this with an exact third
would lose that final tick. Handbrake moves its full travel in one tick.
Press/release rates are independent table entries:

| Travel | Press rate table | Release rate table | Slot 0 raw value |
|---|---|---|---|
| Throttle | `80046DB0` | `80046DB4` | 1365 / 1365 |
| Brake | `80046DB8` | `80046DBC` | 1365 / 1365 |
| Handbrake | `80046DC0` | `80046DC4` | 4096 / 4096 |

The game stores normalized rates in `game/assets/vehicle/pedals.ron`.
Every normalized fraction is exactly representable as `f32`, so no
fixed-point emulation is needed for these rules. Simultaneous pedal requests
are independent; the held-out recording verifies brake+throttle and
brake+handbrake combinations. The port covers the digital path and this
car's normal control mode. Analog response curves, transmission-specific
overrides and other driver slots need separate evidence.

The first physics tick after the replay starts still sees the checkpoint's
held CROSS input; new pad input reaches the next physics tick. For example,
brake's effective requests are `(throttle=1, brake=0)` at run frame 0, then
`(0,1)` at run frame 2. Tests use the **measured effective inputs**, not an
invented delay in the rules. The serial controller/presentation boundary is
outside the tested subset.

## Digital steering and wheel commands

`game/src/vehicle/steering.rs` ports `8002FB18..8002FE20` and the ordinary
wheel-angle setup at `8003E340..8003E408`. The Vitz's six-knot speed curve,
spring coefficients and wheel ratios are readable in
`game/assets/vehicle/steering.ron`. No speed-dependent maximum is fitted
from the observed trajectory.

`8002FB7C..8002FBB0` converts absolute forward velocity to a 1/128 km/h
grid using the original approximate ratio 461/128, capped at 32767/128.
`8003D848` interpolates the curve with a signed truncated increment before
adding the lower knot. The normal targets are opposite signs of that value.
At forward speeds strictly above 34139/4096 m/s, sufficiently large mean
front-wheel slip extends the target on that side, capped at 398/4096 turn.
The threshold is the smaller of the curve limit and 170/4096 turn.
Reverse speed still selects the curve by magnitude but cannot enable this
extension. Slip averaging truncates toward zero.

The digital input selects positive target for LEFT, negative for RIGHT and
zero for neither. The response is a damped spring with stiffness 100/s²,
damping 5/s and an additional 50/s² centering spring for a zero target.
These coefficients are the original words at `80046F3C/40/44` divided by
4096. Both rate and angle changes use the original duration
2184/65536 second. Each change truncates to the 1/4096-turn grid. The stored
rate is retained even when it exceeds the applied ±1024/4096 turn/s limit.
Crossing the target snaps the angle and clears the rate. A reduced positive
target also snaps any resulting angle above it (likewise a negative target
below it), even without crossing during that tick.

`WheelSteering` then applies per-car inner/outer wheel ratios, 5040/4096
and 3400/4096 here, swapping them for negative turns. It truncates the
magnitude before restoring the sign. Front/rear toe parameters at
`car+0x36C/+0x36E` subtract on the left wheel and add on the right; both are
zero for this Vitz. Additional assistance corrections at
`8003E414..8003E45C` remain unported. These wheel commands feed the future
tyre model and can also animate authored wheel meshes.

The normal runs now compare both steering state values, both limits and
all four wheel commands for **300 consecutive ticks**, in addition to
pedals and measured-step integration. The reversal/centering run is an
independent holdout. `tools/record_steering_probes.py`, recorded after these
rules, additionally exports **39 original boundary executions**. They
restore the same checkpoint and explicitly intervene in speed, steering
state and wheel-local velocities. Test inputs come from read watches at
the consuming instructions; outputs come from the completed original tick.
Coverage includes both sides of every nonzero curve knot, the speed cap,
reverse, slip extension and its cap, the strict forward-speed threshold,
rate saturation and target crossing. All comparisons have zero tolerance.
These probes verify controller behaviour under controlled state; they do
not establish that the native force model can produce those states.

```sh
python3 tools/record_driving.py
python3 tools/record_steering_probes.py
cargo test -p gran_turismo_2 --all-features --test driving
```

The native API takes `SteeringKinematics` (forward velocity and both front
slip angles) from dynamics before controls. [Wheel-slip preprocessing](wheel-slip-evidence.md)
now computes those angles from wheel-local velocities before steering in
the same tick, matching `80039DE8`. The 300-tick trajectory tests supply
measured wheel-local velocities and verify the computed front slip against
the original consuming reads, before comparing the resulting steering.
The analog input path remains unported.

## Step construction, integration and update order

The untruncated four-field trace
`.psx/runs/driving-control-probe/trace.txt` shows two complete ticks, at
absolute frames 7500 and 7502. The parent dispatcher `8003EBF0` runs the
following relevant sequence:

1. `8003E0C4`: prepare car state, construct normalized input, update control
   travel in `8002FB18`, process wheel/drivetrain/force work in `80039FC8`,
   then build motion steps with `80030330`.
2. `80034480`: integrate through `80034320` -> `80033E6C`, then perform
   additional contact/collision passes (`800400CC`, `800407A0`, `80040924`,
   `80040F30`, `800412D4`).
3. `80043388` / `800434DC`: per-wheel transform work.
4. `80043578`: road-node/contact state.
5. `8003E8E4`: further state updates.
6. `8003CF94` -> `8003CE3C`: refresh course distance and downstream flags.

The step builder multiplies each world-velocity component by `+0x6FE` with
`8007598C` (signed product, division by 65536 truncated towards zero) and
writes three 20.12 displacements to scratch `1F8000B4/B8/BC`. It multiplies
`+0x64C` by the same duration with `80075B54(...,7)` for the yaw step at
`1F8000C0`. The integrator reads those values at `80033EAC` and `80033ED4`;
those reads are the recorder's source for `dx/dz/dy/dheading`.

The native `VehicleVelocity::step` now constructs these steps from the
measured velocities and duration, with signed truncation to 1/4096 metre
and turn. `VehicleSystems::BuildStep` runs after forces and before
integration. The recorder additionally reads velocities at `800303A8`,
yaw rate at `800303D8` and duration at `80030390`, so the input values are
established at the actual step-construction boundary. All 300 steps and
their resulting continuous trajectories match with zero tolerance,
including the fresh `motion_holdout` mixed steering/pedal run recorded
after the implementation. The tests exercise this builder through the
real plugin. They do not replay captured step values into the plugin.

`80033E6C` adds translation, adds yaw, subtracts 4096 if yaw >=4096, then
adds 4096 if yaw <=-4096. The port preserves this signed representative
using turns rather than integers. This is only the initial integration
part of that function: later code can roll the step back for contact,
change position using contact offsets and invoke further updates. The
recorded scenarios below have no such pose corrections: the **entire
consecutive trajectories** equal the previous pose plus measured step.
That bounded result does not establish contact/collision parity elsewhere.

Frame-boundary velocities can differ from the values used by the step
builder because later phases change them. Therefore tests must not replace
the observed scratch deltas with a product of frame-end velocity samples.
RPM copies also change at a separate presentation cadence; no single
fixed-rate struct update should be inferred from one field's cadence.

## Numeric results and tests

All runs begin at x=401.443115234375 m, z=449.686767578125 m,
heading=1024/4096 turn, throttle=1 and brake/handbrake=0.

| Scenario | Physics ticks | Change x (m) | Change z (m) | Final forward velocity (m/s) | Final heading (raw /4096) |
|---|---:|---:|---:|---:|---:|
| CROSS | 30 | -25.183594 | 0 | 25.547119 | 1024 |
| Coast | 30 | -24.635986 | 0 | 23.844971 | 1024 |
| SQUARE | 30 | -20.730225 | 0 | 15.792236 | 1024 |
| CIRCLE | 30 | -24.026367 | 0 | 22.715332 | 1024 |
| CROSS + LEFT | 30 | -24.228516 | -4.979736 | 24.750732 | 1400 |
| CROSS + RIGHT | 30 | -23.850830 | +5.003662 | 23.882080 | 624 |
| Mixed held-out pedals | 60 | -39.671387 | 0 | 16.528564 | 1024 |

These speeds and positions are observations of the original, not outputs
claimed for an implemented force model. `game/tests/driving.rs` reproduces
all **300 physics ticks**, comparing three pedal channels, three position
channels, heading and steering with **zero tolerance**. The tests run the real plugin
at 30 Hz, read `ControllerState`, construct steps from measured velocities,
and compare every tick with the recordings. They also check that a consumed
step is not repeated when the future force model supplies no new step.

## Remaining questions

- Decode the force/drivetrain/tyre flow under `80039FC8` and its parameter
  tables; derive rules from those instructions before implementing them.
- Decode analog curves and the assistance correction to individual wheel
  commands from `8002FB18` and its caller.
- Trace gear selection and actual shift timing instead of relying on the
  disproven `+0x45D`/velocity-as-RPM interpretation.
- Establish contact, suspension, collision, reverse, stationary launch,
  gear-change and off-track cases with separate original recordings.
- Complete the mixed 30 Hz physics / 60 Hz presentation integration; race
  logic now has the right cadence, but body/tyre motion is still missing.
- Trace the actual HUD speed calculation and corroborate the dimensional
  yaw-rate conversion against the upstream force calculation.

This evidence does not cover other cars, other courses, multiplayer,
automatic shifts, analog controls or race completion rules.
