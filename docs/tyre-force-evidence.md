# Tyre response curves and wheel forces

`game/src/vehicle/tyre_forces.rs` implements the paired response selection in
`800397D0 -> 8003D940` and the complete per-wheel force preparation in
`8003A380..8003A6F4`. It samples longitudinal response and cornering limits
from signed slip, produces the two longitudinal solver channels, calculates
lateral force from steering/slip angle and activation, and adds the separate
low-speed lateral correction. It runs with the existing native geometric
and longitudinal slip rules in the same tick.

This establishes wheel-force preparation, not a complete tyre/contact/body
simulation. The load/camber/surface response that supplies grip capacity,
wheel rotation dynamics and drivetrain/body integration remain unfinished.
Forces retain explicit **original internal force units**; a conversion to
newtons has not been established.

## Source identity and reproduction

Original: USA Simulation Mode Rev 2, Toyota Vitz F '99, B-1 checkpoint at
absolute vblank 7500. State SHA-256
`49fb814844da1b4d160f966c4cb675405fb58eb3d74ebc90702ad58978a2ef5b`;
RAM SHA-256 `ff147faec522bdd3da3f6231a379c4b09e323e6d094186bcb0a71a5a5441ab04`.
State and RAM live locally at `.psx/runs/b1-state1200/`.

```sh
psx disasm --ram .psx/runs/b1-state1200 --addr 8003D940 --count 90 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 80039F4C --count 31 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8003A380 --count 221 --text
python3 tools/record_tyre_forces.py
# Recorded after the initial native implementation and exact comparisons:
python3 tools/record_tyre_forces.py --holdout
cargo test -p gran_turismo_2 --all-features --test tyre_forces
```

`tools/record_tyre_forces.py` records actual consuming loads and completed
stores. It rejects partial execution, dropped trace lines, missing wheel
observations, conflicting repeated input loads and interventions that fail
to reach their consumers. All commands, versions, identities and source
report paths accompany the numerical fixtures. These fixtures and all source
curve tables remain private under ignored `game/tests/data/original/` and
`game/assets/`; none are distributed with source.

## Source fields and native units

Physics base `800A96B4`; four wheels begin at physics+460, stride 68 hex.
Scratch wheel records begin at `1F800004`, stride 1C hex. Order is front
left/right then rear left/right. Axle blocks begin at physics+194/+26C.

| Source | Meaning | Native boundary |
|---|---|---|
| Axle+0 | Absolute folded slip-angle curve | Angle in turns, response /4096 |
| Axle+2C/+6C | Longitudinal response / cornering limit, sharing slip coordinates | Signed slip and responses /4096 |
| Wheel+34 | Load/camber/surface grip capacity | Original internal force units, not divided by 4096 here |
| Wheel+2A | Speed-dependent response | /4096 |
| Wheel+18 | Tread velocity | /4096 m/s |
| Wheel+2C/+30 | Contact velocity, vehicle-local forward/lateral | /4096 m/s |
| Wheel+0C, +50/+52 | Steering, geometric slip angle / activation | Turns / normalized activation |
| Scratch wheel+10/+12 | Longitudinal velocity reference / prospective force direction | Native enums from the slip feature |
| Scratch wheel+16, wheel+46 | Paired curve outputs | Normalized responses |
| Wheel+58/+5C | Wheel-reference / ground-reference longitudinal channels | Internal force units |
| Scratch wheel+4 | Speed-response-scaled wheel-reference channel | Internal force units |
| Wheel+3C | Absolute activated/clamped cornering response | Normalized response |
| Wheel+54 | Final lateral force, including low-speed correction | Internal force units |
| Physics+6A4 | Body forward velocity for correction gating | /4096 m/s |

The previously exported brake-assistance `grip_response` divides wheel+34
by 4096 for its own normalized response arithmetic. `TyreContactState` instead
exposes that word directly as force capacity. Confusing those boundaries
would reduce forces by a factor of 4096.

## Curve selection and longitudinal channels

`8003D940` searches one signed-short x array and interpolates two independent
y arrays on the selected interval, clamping at both endpoints. The lateral
sampler `8003D848` uses the same arithmetic for one ordinate. In both cases,
truncate the signed **ordinate delta** towards zero, then add the lower knot.
Rounding the complete interpolated ordinate changes descending segments.
The exported paired format stores one slip coordinate with both ordinates;
the importer validates matching source coordinate arrays and nonnegative
cornering limits. Source tables are exported locally to `vehicle/tyres.ron`.

Multiply longitudinal response by grip capacity and floor to internal force
units (`8003A3C0`). Apply the stored prospective force direction afterwards.
When longitudinal slip uses wheel speed as its reference, put that signed
force into wheel+58 and truncate its product with speed response into the
scratch response channel. When ground speed is the reference, put it into
wheel+5C and clear the wheel-reference channels. These channels are solver
inputs; calling their sum body acceleration would skip subsequent coupling.

## Lateral force and low-speed correction

When geometric activation is zero, clear base lateral force and cornering
response. Otherwise subtract geometric slip direction from wheel steering,
adding half a turn to steering for nonpositive tread velocity. Wrap the
signed difference into [-half,half), fold it about the quarter-turn axes,
sample the axle's cornering curve and restore its sign. Multiply by
activation with signed truncation, clamp to the paired cornering limit,
store its absolute magnitude and multiply by grip capacity with truncation.
Nonpositive tread velocity negates that base force after the multiplication.
`scaled_force` isolates the source's signed low-word multiplication/bias
boundary (8007596C); wide longitudinal multiplication remains separate.

The correction at `8003A5CC..6D0` runs independently of activation or grip.
It requires tread-speed magnitude below 5690/4096 m/s and body-forward minus
tread speed within the inclusive +/-1137/4096 m/s band. Project contact
velocity onto the wheel's lateral axis using the verified steering basis,
truncating the cosine/lateral and sine/forward products separately before
adding them. Below 2276/4096 m/s magnitude, multiply by 14742 force units per
m/s and truncate; at or above the threshold use signed 8192 force units.
Fade this by the remaining tread-speed headroom with slope 2948/4096 per m/s,
flooring the positive fade coefficient before its signed force product.
Add the result to base lateral force. The exporter recovers the design
values from the source instructions into private tuning.

## Verification, integration and limits

All **972 original wheel-force executions** match with zero tolerance:

| Suite | Wheel executions | Coverage |
|---|---:|---|
| Controlled boundaries | 620 | Every front/rear longitudinal and cornering knot and adjacent sample, interval midpoints, fractional activation below the cornering cap, angle-fold boundaries, reverse tread, all low-speed tread/body-difference/lateral saturation edges, zero velocity/activation |
| Natural gas/brake/turn | 240 | Twenty original physics ticks each, all four wheels |
| Independent controlled holdout | 32 | New steering phases, unequal signed velocity pairs and additional low-speed cases |
| Independent mixed holdout | 80 | Simultaneous brake, right steering and handbrake |

Each execution compares all seven response/force outputs. The same headless
app computes geometric angle/activation and longitudinal ratio/reference/
direction with the existing native plugins, then checks those intermediate
inputs against the original too. Thus the force calculation consumes fresh
native slip rather than measured slip outputs. Grip capacity, tread/contact
velocities, speed response, steering and body velocity remain measured or
explicitly intervened inputs. No native formula generates expected data. Coverage guards inspect consumed
original slip values and unclipped, fractionally activated angular cases,
so an upstream rule cannot silently replace a requested boundary case.

The independent handmade-tuning test runs only `TyreForcesRulesPlugin`, with
no disc assets, other vehicle plugins or renderer. It checks force-channel
selection and a nonzero low-speed correction with zero activation and grip.
The example mod disables this unused feature, retaining its authored assets.

The plugin runs after geometric preprocessing, steering, longitudinal slip
and brake allocation, before step construction. Source operations are grouped
as a pure per-car calculation because these phases read no other car's tyre
outputs. Public tuning, components and the plugin remain independently usable.
Wheel load/contact response, suspension, rotation/drivetrain coupling, body
acceleration and yaw, collision and a playable driving session remain open.
The live B-1 subject is still static; these tests do not establish free-driving
or all-car parity.
