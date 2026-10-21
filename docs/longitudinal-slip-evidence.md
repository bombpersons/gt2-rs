# Longitudinal wheel slip

`game/src/vehicle/longitudinal_slip.rs` implements `80039490..80039774`:
project contact velocity onto the steered wheel axis, compare it with tread
speed, select a normalization velocity and force direction, then calculate
signed slip with stopped-wheel handling and a low-speed fade. The optional
connection to brake assistance replaces its measured longitudinal-slip input.
Wheel rotation, contact motion, speed response, grip and body forces remain
separate work; this feature does not accelerate the live car.

## Identity and reproduction

Original: Gran Turismo 2 USA Simulation Mode Rev 2, B-1, Toyota Vitz F '99.
Checkpoint `.psx/runs/b1-state1200/state.bin` SHA-256
`49fb814844da1b4d160f966c4cb675405fb58eb3d74ebc90702ad58978a2ef5b`;
RAM SHA-256 `ff147faec522bdd3da3f6231a379c4b09e323e6d094186bcb0a71a5a5441ab04`.
All numerical recordings and original direction-table samples stay private
under `game/tests/data/original/longitudinal_slip/`. No disc tables or fixture
payloads are distributed in the public repository.

```sh
psx disasm --ram .psx/runs/b1-state1200 --addr 80039490 --count 186 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8007596C --count 78 --text
python3 tools/record_longitudinal_slip.py
# Recorded after implementation and the initial exact comparisons:
python3 tools/record_longitudinal_slip.py --holdout
cargo test -p gran_turismo_2 --all-features --test longitudinal_slip
```

`tools/record_longitudinal_slip.py` reads inputs at their actual consuming
instructions and outputs from completed stores. It rejects partial runs,
truncated traces, missing wheel executions, conflicting observations and
interventions that did not reach the consumer. It distinguishes an unused
response input from an observed zero. Controlled runs skip live controls by
the established mode-1 branch, while the original wheel-angle builder still
runs. Their contact velocities, tread speeds, response gains and steering
parameters are explicit interventions; they do not claim natural trajectories.
Natural recordings keep the original controls, geometry and dynamics intact.
The recorder also captures the entire downstream brake-assistance and demand
allocation boundary for those natural sequences. All commands, source report
paths, version and hashes are retained beside the private fixtures.

## Fields and units

Physics base `800A96B4`, wheels at physics+460 with stride 68 hex, ordered
front-left/right then rear-left/right. Scratch wheel entries begin at
`1F800004`, stride 1C hex. The original runs this routine after steering
basis construction and before brake assistance / force solving.

| Source | Native meaning | Units / consumer |
|---|---|---|
| Wheel+18 | Tread surface velocity | /4096 m/s, 80039520 |
| Wheel+2C/+30 | Contact velocity in vehicle-local forward/lateral axes | /4096 m/s, 80039504/39514 |
| Wheel+2A | Speed-dependent slip response | /4096, 8003957C/395D0/39618/3966C |
| Scratch+1A/+18 | Steering cosine/sine | /4096, 800394FC/39510 |
| Scratch+10 | Velocity used to normalize slip | +1 wheel, -1 ground |
| Scratch+12 | Prospective longitudinal force direction | -1 reverse, 0 none, +1 forward |
| Scratch+14, wheel+44 | Signed longitudinal slip response | /4096; final wheel store 80039744 |

The velocity-axis clarification matters: the contacts are located at the
wheels, but these two components have **not yet been rotated by wheel
steering**. The two multiplications at 80039508/39518 perform that rotation.
The existing `WheelLocalVelocities` API is retained, with clarified comments.

## Rules and precision

The steering builder at 8003E470..4BC wraps the steering angle to 4096 phases,
then samples sine and cosine at 80093150/80093950. Exhaustive comparison of
all 4096 pairs proves the native formula `trunc(sin(phase)*4096 + 0.5)`.
The asymmetry is observable: 2047 negative sine entries differ from ordinary
nearest rounding. Cosine is the same curve at a quarter-turn phase offset.
The exporter additionally verifies all 5120 source entries touched by those
two overlapping arrays. This uses an ordinary mathematical quantizer, not a
shipped copy of an original table.

Ground rolling speed is `cosine*forward - sine*lateral`. The source calls
80075A5C for each product separately, truncating towards zero onto the
1/4096 m/s grid before subtraction. One combined dot product followed by one
rounding changes observations. The intermediate projected velocity is checked
exactly, in addition to the final ratio.

Within the inclusive stopped-wheel band (70/4096 m/s either way), the wheel
uses ground speed as the reference. Unequal velocities start with full
positive slip, equality with zero. Direction opposes the difference between
ground and tread speed. This path does not consume speed response.

Outside that band, positive tread speed uses the wheel as reference when
`tread >= ground`; negative tread speed does so when `tread <= ground`.
The other cases use ground speed. Absolute-magnitude comparison would change
opposite-rotation cases. The numerator is speed response times
`ground - tread`, quantized towards zero. Divide by the selected signed
velocity, truncate onto a /4096 response grid, **narrow to a signed halfword,
then clamp to [-1,1]**. The order is observable when tread and contact motion
oppose one another; clamping before narrowing loses recorded aliases.
The force direction depends on wheel/reference sign and can remain nonzero
when the slip ratio is zero. The port exposes ordinary enums for both fields.

Below 22760/4096 m/s reference-speed magnitude, multiply by a fade coefficient
formed with slope 737/4096 per m/s. Floor the positive coefficient onto /4096,
then truncate the signed slip product towards zero. At the threshold, bypass
the fade exactly. These three design values are exported into the private
`vehicle/longitudinal_slip.ron`, not compiled as car tuning.

## Verification and Bevy integration

All **268 original wheel executions** match exactly:

| Suite | Executions | Coverage |
|---|---:|---|
| Controlled boundaries | 60 | Inclusive stopped-wheel edges, both rotation signs, equality, opposite motion, fractional/zero/signed response, fade threshold, steering quadrants |
| Natural gas/brake/turn | 144 | Twelve physics ticks each, all four wheels |
| Controlled holdout | 16 | Fresh unequal signed velocities, fractional gains, steering phases and additional threshold / ratio boundaries |
| Natural mixed holdout | 48 | Independent simultaneous throttle, brake and right steering |

Tests also compare every original sine/cosine pair and check standalone use
with handmade tuning and same-tick steering changes. In 36 natural physics
ticks (144 wheel executions), the new output feeds the existing brake rule:
all assisted travel, ordinary demand and final handbrake-inclusive demand
match the original. The test initializes assistance's old slip values with
sentinels and registers plugins in reverse order, proving that fresh values
reach the correct consumer in the same tick.

`LongitudinalSlipRulesPlugin` is an independent `GamePlugins` member. Cars
opt in with `WheelSurfaceMotion`, `WheelLocalVelocities`, `WheelSteering` and
`LongitudinalSlip`. `VehicleSystems::ContactSlip` sits after wheel steering
and before forces. Cars with `BrakeAssistInput` additionally receive the new
slip and speed response; other cars do not require the braking feature.
The source-only example mod disables this unused dynamics feature and keeps
its handmade course/body/camera assets independent of private tuning.

[Tyre-force curves and preparation](tyre-force-evidence.md) now consume these
outputs natively. Wheel surface speed/response generation, contact/rotation
dynamics, grip and yaw signals, coupled drivetrain/body acceleration and a
playable B-1 session are still unfinished. These exact subsystem observations
do not establish whole-vehicle or all-car parity.
