# Wheel reaction torque and differential rotation

`game/src/vehicle/wheel_rotation.rs` ports tyre/brake reaction torque
(`80045688`), yaw-sensitive differential preparation (`800457B0`), and
differential tread-speed integration (`800465E0`). These are the three
outer phases of `80046B58`. Its intervening engine/transmission/axle solver
at `80045AE8` remains unported and supplies measured angular speeds and
drive torques in these comparisons. This does not yet move the live car.

## Identity and reproduction

Original: USA Simulation Mode Rev 2, Toyota Vitz F '99, B-1 checkpoint at
absolute vblank 7500. State SHA-256
`49fb814844da1b4d160f966c4cb675405fb58eb3d74ebc90702ad58978a2ef5b`;
RAM SHA-256 `ff147faec522bdd3da3f6231a379c4b09e323e6d094186bcb0a71a5a5441ab04`.

```sh
psx disasm --ram .psx/runs/b1-state1200 --addr 80045688 --count 190 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 800465E0 --count 350 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 80046380 --count 150 --text
python3 tools/record_wheel_rotation.py
python3 tools/record_wheel_rotation.py --holdout
# Fresh mixed-axle probes recorded after implementation and initial checks:
python3 tools/record_wheel_rotation.py --final-holdout
python3 tools/record_wheel_rotation.py --gate-holdout
cargo test -p gran_turismo_2 --all-features --test wheel_rotation
```

Original traces and memory watches stay in `.psx/runs/wheel-rotation-*`.
CSV/JSON reference files stay in ignored
`game/tests/data/original/wheel_rotation/`. The recorder composes the
existing tyre/forward observations, checks completed execution, zero trace
loss and complete watches, and reconstructs snapshots at the actual phase
boundaries by applying watched writes in cycle order. It checks that the
reaction phase consumes the tread speeds and tyre forces observed upstream.
Controlled probes change only original inputs; no native formula generates
expected outputs. Commands, checkpoint hashes, reports and toolkit version
accompany each fixture.

Yaw correction is recorded at its upstream writer as well as its conditional
consumer. Reading only `80045860` would replace unconsumed nonzero signals
with zero below the speed gate and fail to test suppression. The isolated
gate probes remove grip, wheel load and aerodynamic force upstream, keep
wheel bases unsteered, and request body steering to generate a nonzero yaw
signal. Their observed forward speed must equal the intervention exactly.
These are controlled subsystem inputs, not naturally occurring car states.

## Fields and asset contract

Physics base is `800A96B4`; wheel records start at physics+460, stride 68 hex,
ordered front-left/right, rear-left/right. Scratch wheel entries start at
`1F800004`, stride 1C hex. Forces and torques retain original internal units;
their conversion to newtons/newton-metres is not established.

| Source | Native field / meaning |
|---|---|
| Physics+37A/+37B | Differential kind on each axle |
| Physics+37C/+380 | Clutch preload, viscous squared-slip ceiling, or yaw-bias gain, according to kind |
| Physics+384/+388, +38C/+390 | Drive/coast clutch response; mode 5 uses the drive field as its squared-slip gain |
| Physics+3E4/+3E6 | Wheel radius, /4096 metres |
| Physics+3F8/+3FC | Rotational response, /512 for coupled wheels, doubled for independent wheels |
| Scratch wheel+4/+8 | Tyre response / auxiliary tread force |
| Scratch wheel+0/+C | Total brake demand / reaction torque |
| Wheel+16/+18 | Stored integration increment / tread speed, /4096 |
| Physics+61A/+61B | Persistent clutch lock latches |
| Physics+634/+638, scratch+74/+78 | Measured axle angular speed (/4096) and drive torque from `80045AE8` |
| Scratch+90, physics+60C | Yaw correction (/4096) and steering sign for modes 6/7 |
| Physics+6A4/+6FE | Body-forward speed (/4096 m/s) / per-car duration (/65536 seconds) |

The exporter writes `vehicle/wheel_rotation.ron` locally with radii,
responses, differential variants and the forward-speed gate from `80045854`.
Unknown source kinds are rejected. Source modes 3/4 share `Clutch`; modes
6/7 share `YawBiased`, matching the source dispatch and original probes.
Mods can author those same RON variants with their own parameters.

## Rules and observable precision

Prepare reaction torque by multiplying the sum of tyre and auxiliary force
by wheel radius, truncating towards zero, then negating. Subtract total brake
demand for positive tread speed, add it for negative tread speed, and leave
zero tread speed unaffected by brakes in this phase.

Modes 6/7 additionally use yaw correction when body-forward speed reaches
the positive gate; reverse travel does not qualify. Form the correction
with floor rounding. Compare the right tread speed against the sampled
axle mean, shifted by one eighth according to steering sign. Transfer
seven sixteenths to one wheel and subtract nine sixteenths from the other,
with each signed fraction truncated separately. This is deliberately not
an equal-and-opposite transfer.

Differential behavior follows the original modes:

- Independent wheels integrate each reaction torque, using the mean of the
  previous and new velocity increments. Brake demand prevents a sign reversal
  and keeps an initially stopped wheel at zero. Reaching exact zero without
  crossing retains the new increment.
- Locked wheels both take axle angular speed times wheel radius. Only the
  left wheel's stored increment is cleared (`800469D8`).
- Open wheels evolve half their speed difference from the reaction-torque
  difference and the previous left-wheel increment, around the supplied axle
  speed times radius.
- Clutch differentials use the larger of preload and absolute drive torque
  times the appropriate drive/coast response. A latched axle stays locked
  while the reaction-torque difference is within twice that capacity,
  including equality. Otherwise release it and transfer torque against slip.
- Viscous differentials transfer torque against slip according to its squared
  magnitude, with the source's intermediate floor and ceiling.
- Yaw-biased differentials use their prepared torques in the open-wheel
  integration path.

For all coupled unlocked wheels, the reversal check at `80046A30` tests the
**signed low product word** of the old speed difference and the proposed
half difference. It can therefore clear the split even when a mathematical
sign comparison would retain it. The delay-slot instruction at `80046A4C`
clears the new increment for every differential kind taking this branch;
only clutch kinds additionally set the persistent latch. The native helper
isolates that word boundary. Signed /4096 truncation remains at each multiply
and average. Increment halfword narrowing happens after the full increment
has affected the current tick.

The coupled axle solver reuses each right wheel's increment slot to retain
an **angular** increment (`80046444/464E8`), whereas this wheel rule uses
only its left increment. Tests supply those intervening writes explicitly;
they must not be described as continuous native axle dynamics. Independent
wheel increments and both clutch latches persist natively across consecutive
calls. Upstream code can also reset undriven tread speeds between calls, so
tread inputs are observed afresh at the consuming boundary.

## Verification and remaining work

All **230 original car executions (920 wheel executions)** match exactly
for native tyre response, forward-phase speed, brake demand, prepared and
final torque, tread speeds, stored increments and clutch latches:

| Suite | Car calls | Coverage |
|---|---:|---|
| Controlled boundaries | 88 | All eight source modes, signed/zero motion, both initial latch states, steering signs, yaw-bias speed gate and adjacent forward/reverse inputs |
| Natural gas/brake/turn | 60 | Twenty consecutive calls each, original controls and dynamics |
| Clutch sequence | 20 | Consecutive original clutch state changes and nonzero history |
| Independent signed holdout | 16 | Every mode and latch state with fresh asymmetric small signed velocities and large stored increments |
| Independent mixed holdout | 20 | Simultaneous brake, right steering and handbrake |
| Final mixed-axle holdout | 16 | Different front/rear modes, both yaw signs, small opposite tread speeds and unequal histories |
| Exact yaw-gate holdout | 10 | Both yaw-biased modes at the gate and adjacent consumed speeds, plus zero/reverse; nonzero upstream correction below the gate |

The tests register plugins in reverse order and compute both slip stages,
tyre force, brake allocation and forward dynamics in the same tick. They
retain native left-wheel history and clutch latches across each recording,
assert continuity against the original, and count exercised modes, positive
and negative clutch drive torques, latch changes, yaw preparation and actual
product-word clamp branches. A separate handmade-tuning test runs this plugin
alone and inserts an axle-drive system between torque preparation and wheel
integration, proving the public scheduling boundary without disc assets.

`cargo test --workspace --all-features` passes with the private originals.
The opt-in native game and example-mod MCP render/step checks pass, as does
the engine-only sandbox movement/material check. Their screenshots were
inspected; this change does not claim any new visual parity. Exporting tuning
again from the source checkpoint reproduces the local wheel-rotation RON.
Both current-source and reachable-history publication audits pass; numerical
fixtures, generated tuning and emulator outputs remain ignored.

`WheelRotationRulesPlugin` is a separately replaceable `GamePlugins` member.
`WheelRotationSystems::{Prepare, AxleDrive, Integrate}` provides the original
phase order after forward dynamics and before motion-step construction.
The handmade example mod disables this unused physics feature.

These are connected subsystem comparisons with explicit external inputs,
not a closed vehicle trajectory. The intervening engine/transmission/axle
solver, auxiliary-force/yaw production, contact and load/grip production,
aerodynamics, lateral/yaw/world acceleration and collisions remain
unfinished. Live wheel steering/spin/suspension presentation and a playable
B-1 driving session also remain open. The next coupling boundary is
`80045AE8`, including its right-slot angular history and undriven-axle paths.
