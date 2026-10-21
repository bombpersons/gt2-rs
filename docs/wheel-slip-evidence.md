# Wheel-local slip preprocessing

`game/src/vehicle/slip.rs` ports the geometric direction and low-speed
activation calculation in `80039DE8`. It takes independently supplied local
wheel velocities, calculates all four wheel outputs, and supplies the front
two directions to the steering controller before control processing. The
wheel/contact model that produces those velocities is still unfinished.
These rules do not calculate tyre force or vehicle acceleration.

## Identity and reproduction

Original: Gran Turismo 2 (USA), Simulation Mode, Rev 2, Toyota Vitz F '99,
B-1 checkpoint at absolute vblank 7500:

- State: `.psx/runs/b1-state1200/state.bin`, SHA-256
  `49fb814844da1b4d160f966c4cb675405fb58eb3d74ebc90702ad58978a2ef5b`.
- RAM: `.psx/runs/b1-state1200/ram.bin`, SHA-256
  `ff147faec522bdd3da3f6231a379c4b09e323e6d094186bcb0a71a5a5441ab04`.
- Numeric original executions and their commands/reports:
  `game/tests/data/original/wheel_slip/{boundaries,holdout}.{csv,json}`.
- Recorder: `tools/record_wheel_slip.py`, using the native `psx` runtime.
  Each JSON records its exact CLI version. Raw watches/reports remain under
  `.psx/runs/wheel-slip-*`.

```sh
psx disasm --ram .psx/runs/b1-state1200 --function 80039DE8 --text
psx disasm --ram .psx/runs/b1-state1200 --function 80081288 --text
psx disasm --ram .psx/runs/b1-state1200 --function 80081AF0 --text
psx disasm --ram .psx/runs/b1-state1200 --function 80082E14 --text
python3 tools/record_wheel_slip.py --export-tuning /tmp/gt2-slip-tuning.ron
cmp game/assets/vehicle/slip.ron /tmp/gt2-slip-tuning.ron
python3 tools/record_wheel_slip.py
# First recorded after writing the native rule; never used to fit it.
python3 tools/record_wheel_slip.py --holdout
cargo test -p gran_turismo_2 --all-features --test slip
```

The tuning export verifies all 4097 original angle-table entries against its
512-threshold compression before writing the RON asset. No executable, disc
content, frame capture or emulator state is shipped as gameplay data. The
exported table is a numerical direction curve, with named physical tuning.

## Fields, units and rule

The first car's physics base is `car = 800A96B4`; wheel records begin at
`car+460`, with stride `68` hex, in front-left, front-right, rear-left,
rear-right order. All offsets below are hexadecimal:

| Wheel field | Role | Units and evidence |
|---|---|---|
| `+2C` | Local forward velocity | Signed word, 4096 per m/s; consumed at `80039E48` |
| `+30` | Local lateral velocity | Signed word, 4096 per m/s; consumed at `80039E4C` |
| `+50` | Geometric slip direction | Signed halfword, 4096 per turn; written at `80039F00` |
| `+52` | Low-speed activation | Signed halfword, 4096 = full; written at `80039EE8` |

The velocity dimensions agree with the wheel-transform/body-velocity route
documented in the driving evidence. The port keeps these as explicit native
inputs in m/s; it does not yet reconstruct that transform. The second local
component's positive direction is the original wheel-local axis: the angle
calculation explicitly negates it. A complete contact model must preserve
that sign when writing `WheelLocalVelocities`.

The rule is: determine how much slip is active at low speed; when active,
take the signed direction of `(forward, -lateral)`. The original writes the
direction itself, **not** the direction multiplied by its activation.

`80039E50..80039EE0` contains these tuning values:

- Dead zone: `1138/4096 = 0.27783203125` m/s.
- Full-speed threshold: `11380/4096 = 2.7783203125` m/s.
- Activation slope: `1638/4096 = 0.39990234375` per (m/s).

If either absolute component strictly exceeds the full-speed threshold,
activation is one. Otherwise the source floors each component's squared
velocity onto a `1/4096 (m/s)^2` grid, adds them, and floors the square root
onto a `1/4096 m/s` grid. `80081288(..., 6)` is an integer square-root
routine with six fractional output bits, so this is not an exact Euclidean
length followed by one final rounding. Below full speed, activation is the
positive speed above the dead zone times the slope, floored to `1/4096`.
If that quantized activation is zero, direction is zero too.

This rounding changes real boundaries. On the forward axis, original raw
speed 1141 still produces zero activation; 1142 produces one raw activation
unit. Raw speed 11380 produces 4095, while 11381 bypasses the square-root
path and produces 4096. With equal components, `(8046,8046)` produces 4095
but `(8047,8047)` produces 4096. These are original runtime observations,
all compared exactly by the tests.

## Exact direction curve

`80081AF0` first reduces large signed velocity pairs to 14 magnitude bits
using one common scale. It chooses that scale by counting leading sign
bits. Negative exact powers of two therefore fit one additional bit.
Discarded negative fractions floor, as an arithmetic right shift does;
using truncation towards zero changes some angles. The native implementation
isolates this integer sampling boundary in `direction_sample`.

`80082E14` takes the absolute minor/major component ratio, truncates it onto
a `1/4096` grid, and indexes the signed-halfword table at `800A4AC8`.
Its 4097 first-octant values increase monotonically from 0 to 512 in
4096-per-turn units. The table's SHA-256 is retained in the asset and each
recording's metadata. **133 entries differ from standard nearest-rounded
atan** on the same ratio grid; replacing it with ordinary trigonometry
does not reproduce the original.

`game/assets/vehicle/slip.ron` stores the minimum ratio for each successive
angle as 512 dimensionless thresholds. Counting passed thresholds gives
exactly the original value at every index. The curve remains readable and
replaceable through the normal asset layers. Its contract is 512 ascending
thresholds in 0..=1, corresponding to angles 1..=512 in 4096-per-turn units.

The sign/offset tables at `800A2A8C` and `800A2A94` reflect that first
octant into a complete direction. The native code expresses ordinary octant
reflection directly in turns. Exactly backward is +half a turn; a vector
with negative sideways component uses the negative angle representative.
Neither the angle table nor the source's sign-bit sampling is claimed to be
a generic physical tyre model.

## Native boundary and verification

`WheelSlipRulesPlugin` is an independently selectable member of
`GamePlugins`. It has no renderer or controller-resource dependency and runs
on cars with `WheelLocalVelocities` and `WheelSlip`. Its system joins
`VehicleSystems::Prepare`, ordered before `ReadInput`, at the 30 Hz game
cadence. A car with `SteeringKinematics` additionally receives the new front
slip angles during that phase. Cars without wheel-local state can supply
their steering kinematics independently, as the original driving replay
tests currently do.

The recorder restores the same checkpoint for each four-wheel group,
intervenes in the local velocity words at run frame zero, and reads the
actual consumed inputs at `80039E48/4C`. It exports outputs only from the
completed writes at `80039EE8` and `80039F00`. It requires complete read and
write logs and exactly one input/output per wheel; frame-end samples cannot
silently substitute for values consumed earlier in the tick.

The 80 initial executions cover zero, dead-zone quantization, both signs of
the axis full-speed threshold, diagonal full-speed transitions, every angle
quadrant, axes, diagonals, known atan-table differences and sign-sensitive
normalization near powers of two. For example, `(16383,1099)` and
`(16384,1099)` produce distinct angles through the sampling boundary.
The 20 holdout executions were recorded after the native rule, with mixed
unequal components and additional normalization boundaries. Both suites
compare every output with zero tolerance. Headless integration additionally
checks all four wheel slots, fresh-slip steering extension in the same tick,
and use of the slip plugin without the other vehicle features.

These are controlled original routine executions, not free-driving contact
trajectories. They establish this preprocessing boundary for the recorded
overlay; they do not establish how tyres generate those velocities, the
later force use of activation, suspension/contact parity, or parity for
every car and driving mode. The broader port remains unfinished.
