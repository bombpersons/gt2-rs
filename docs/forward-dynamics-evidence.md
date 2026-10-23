# Forward dynamics before drivetrain coupling

`game/src/vehicle/forward_dynamics.rs` ports surface resistance at
`8003A2A0..8003A364` and the complete forward integration phase at
`8003A710..8003A938`. Both native slip stages and tyre-force preparation feed
it in the same tick. Its output is the intermediate body-forward state
consumed by `80046B58`, not final world velocity or a complete driving model.

## Identity and reproduction

Original: USA Simulation Mode Rev 2, Vitz F '99, B-1 checkpoint at absolute
vblank 7500. State SHA-256
`49fb814844da1b4d160f966c4cb675405fb58eb3d74ebc90702ad58978a2ef5b`;
RAM SHA-256 `ff147faec522bdd3da3f6231a379c4b09e323e6d094186bcb0a71a5a5441ab04`.
All original inputs, observations and traces remain in ignored local folders.
The public exporter reads tuning through the source descriptors; no source
coefficient or curve arrays are embedded in the implementation.

```sh
psx disasm --ram .psx/runs/b1-state1200 --addr 8003A2A0 --count 50 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8003A710 --count 139 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8003DAAC --count 80 --text
python3 tools/record_forward_dynamics.py
python3 tools/record_forward_dynamics.py --holdout
cargo test -p gran_turismo_2 --all-features --test forward_dynamics
```

The recorder composes the existing tyre recorder with observations of this
phase. It requires complete original execution and zero dropped trace lines,
checks actual consuming inputs, and rejects missing phases or wheel loads.
CSV/JSON fixtures live in `game/tests/data/original/forward_dynamics/`;
commands, checkpoint digests, toolkit version and report paths accompany them.
Full original traces live in `.psx/runs/forward-dynamics-*`.

## Fields and units

Offsets below use physics base `800A96B4`. Four wheel records start at
physics+460, stride 68 hex. Scratch wheel records start at `1F800004`,
stride 1C hex. Front-left/right precede rear-left/right.

| Source | Native meaning |
|---|---|
| Physics+28/+2A | Body mass / precomputed reciprocal, normalized /4096 |
| Physics+410/+412 | Per-wheel rotational mass on each axle, normalized /4096 |
| Wheel+8/+14 | Contact/load magnitude in original units / surface index |
| Wheel+63 | Initial flags: bit 0 for zero load, bit 1 for brake demand |
| Scratch wheel+18/+1A | Steering sine/cosine, /4096 |
| Wheel+58/+5C/+54 | Wheel-reference, ground-reference and lateral tyre force |
| Physics+71A | Signed-halfword aerodynamic force input |
| Scratch+88/+84 | Surface resistance / effective reciprocal mass |
| Physics+734/+724/+6A4 | Accumulated force / forward acceleration / forward speed |
| Physics+6FE | Per-car duration, /65536 seconds |

Force retains original internal units. Acceleration and speed use /4096
metres/s² and metres/s respectively. Normalized source mass units and their
reciprocals relate these quantities; kilograms and newtons are not claimed.
The signed aerodynamic force comes from `8003DAAC..8003DBA0`, a speed-squared
preparation path with a changing reduction factor. The separate
[aerodynamics feature](aerodynamics-evidence.md) now produces it natively and
feeds this forward rule in 266 original comparisons. This isolated corpus
still supplies the force actually consumed by this phase.

## Rule and exact boundaries

The resistance pass clears the force/acceleration accumulators. For each
wheel it multiplies load by the selected surface coefficient and floors the
product. Sum those results, sample the global clamped resistance curve at
absolute body-forward speed, then multiply and floor again. Apply the sign
opposite body speed; exactly zero speed explicitly produces zero resistance.
The word-coordinate curve at `801C8730` uses `80075D2C`: truncate the signed
interpolated ordinate delta before adding the lower knot, and clamp endpoints.
The eight surface coefficients and complete curve are exported locally into
`vehicle/forward_dynamics.ron`.

Project each wheel-reference tyre force onto the body-forward direction and
sum it with resistance. Include each loaded wheel's axle rotational mass in
the effective mass. The original tests initial wheel flags with mask 0D;
only bits 0/1 have been written at this phase, so eligibility is exactly
nonzero load. Braking alone retains wheel mass. No eligible rotational mass
uses the precomputed body reciprocal; otherwise the reciprocal is recomputed
and truncated to the source /4096 grid. Arbitrary modded masses that overflow
the original signed-short mass sum or yield a nonpositive total are outside
the established contract.

Add aerodynamic force to this first force channel, multiply by effective
reciprocal mass with the wide signed helper `80075A5C`, and truncate to the
source acceleration grid. Aerodynamic force affects acceleration but is not
added to the stored force accumulator. Integrate forward speed with signed
truncation using the per-car duration (`80075A94`). A zero result skips the
second phase entirely. This first update can change direction.

For nonzero speed, project and sum the ground-reference force channel and
subtract the lateral contribution. Two source details are deliberate in the
native rule:

- `8003A898` reads **front-left lateral force on every wheel iteration**.
  The ground-reference force and steering basis still advance per wheel.
  Consuming-load addresses and unequal wheel-force cases establish this.
- The second force/mass product uses the signed **low-word** helper
  `8007596C`; the first uses the wide multiply. The shared `scaled_force`
  helper preserves its sign bias and word boundary.

If the resulting candidate forward speed retains its sign bit, accumulate
the second force and acceleration. If the sign bit changes, set speed to
zero, leave the force accumulator unchanged and apply the stopping correction.
Positive speed arriving at exact zero accumulates the full second force;
negative speed arriving at exact zero takes the stopping branch. The original
uses signed-bit comparison, not a symmetric zero-crossing inequality.

The stopping correction at `8003A8F8..8003A90C` calls the multiply helper with
duration_q16 and negative speed_q12, dividing their product by 4096. It does
**not** divide speed by duration. Native code isolates this observed behavior
rather than replacing it with an ideal physical stopping acceleration.

## Verification and remaining work

All **183 original car executions** match exactly for resistance, effective
reciprocal mass, force, forward acceleration, forward speed and stopping path:

| Suite | Calls | Coverage |
|---|---:|---|
| Controlled boundaries | 93 | Every resistance knot and adjacent value in both directions, interval interiors, all eight surfaces, all sixteen wheel-contact combinations, unequal wheel velocities, forward/reverse stopping and initial zero |
| Natural gas/brake/turn | 60 | Twenty consecutive physics calls each, including braked loaded wheels |
| Independent controlled holdout | 10 | New asymmetric angles/contact inputs, a first-phase reversal, exact positive/negative zero arrivals and their neighbours |
| Independent mixed holdout | 20 | Simultaneous brake, right steering and handbrake |

The holdouts were recorded after the native implementation and initial exact
comparisons. Coverage assertions inspect consumed speeds, contact masks and
branch observations. Each test tick computes both slip stages and tyre forces
natively, checks the required force channels for all four wheels, and then
checks the forward phase. Reverse plugin registration verifies scheduling.
A separate handmade-tuning test runs the new plugin alone without original
assets, other vehicle plugins, controls or rendering.

These are connected subsystem comparisons, not a closed simulation trajectory:
load, grip, contact/tread velocities, aerodynamic force and initial body speed
are measured or controlled inputs for each call. Drivetrain coupling, engine
speed evolution, lateral/yaw/world acceleration, suspension/contact production
and collisions remain unfinished. The live B-1 subject remains static. The
new plugin is independently replaceable through `GamePlugins`; the handmade
example mod disables this unused feature.
