# Vertical body motion and suspension-stop correction

`vehicle::vertical_body` ports `80043F18..80044898`: pitch/roll rates and
angles, suspension impulses, gravity, airborne feedback, reset height/heading,
and the correction when suspension travel crosses a compression stop.
All **221 original body calls** match exactly (161 boundary/natural calls and
a fresh 60-call holdout), both in isolation and with native suspension.
The **257 road-contact calls** also now run native road sampling, suspension,
body integration/correction and late frame/attachment refresh. The previous
injected corrected pose/angles have been removed from those tests.

These are connected subsystem comparisons. Position/velocity at the earlier
movement boundary and the force channels entering suspension remain observed.
A closed B-1 trajectory, live driving and live body/wheel presentation remain
unfinished. Reset-event selection and licence outcomes are explicit boundaries.

## Identity and reproduction

USA Simulation Mode Rev 2, Vitz F '99, B-1 at vblank 7500.
State SHA-256 `49fb814844da1b4d160f966c4cb675405fb58eb3d74ebc90702ad58978a2ef5b`;
RAM SHA-256 `ff147faec522bdd3da3f6231a379c4b09e323e6d094186bcb0a71a5a5441ab04`.

```sh
psx disasm --ram .psx/runs/b1-state1200 --addr 80043F18 --count 615 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 800357C8 --count 43 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 800450E0 --count 22 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 800431A0 --count 122 --text
python3 tools/record_vertical_body.py
python3 tools/record_vertical_body.py --holdout
python3 tools/record_road_contact.py
python3 tools/record_road_contact.py --holdout
cargo test -p gran_turismo_2 --all-features --test vertical_body --test road_contact
```

The recorder requires complete execution, complete watches, no dropped trace
lines, one car and all four boundaries: suspension entry, body entry, after
ordinary integration and after compression correction. It records consumed
tuning, wheel forces and the original scratch body-force stores. CSV/JSON,
commands, checkpoint hashes and toolkit identity stay in ignored
`game/tests/data/original/vertical_body/`; traces/RAM/reports stay in `.psx/`.
Pokes act on histories, geometry, tuning and reset requests. Expected outputs
come only from original execution. The reset countdown is decremented before
this stage, so tests consume the recorded count, not the initial poke.

## Quantities and rules

Offsets are from physics base `800A96B4`; wheel records start at +460,
stride 68 hex. Force units remain original internal units, not SI newtons.

| Source | Native quantity |
|---|---|
| +644/+646, signed halfwords /4096 | Pitch/roll turns in `BodyOrientation` |
| +650/+652, signed halfwords /65536 | Pitch/roll turns per second in `BodyDynamics::tilt_turns_per_second` |
| +6C/+70, +74/+78, words /4096 /65536 | Front/rear pitch and roll acceleration response per force |
| +2C, signed halfword /4096 | Vertical reciprocal mass, distinct from horizontal +2A |
| +628/+62C/+630, words /4096 | World velocity, converted once to Bevy (x, up, -source y) |
| +6FE, signed halfword /65536 | Step seconds |
| Wheel+8 and +6B1 | Total tyre load determines airborne feedback for next tick's tyre-acceleration gate |
| +719 | Compression bits before correction move to the high nibble; new tests occupy the low nibble |
| +6A0/+67C, +68C..698 | Earlier suspension projection and retained road axes |
| +78E, +688, event record+28 | Reset countdown, road centre height and target heading |

`BodyDynamics` previously labelled +650/+652 as unknown secondary angular
quantities at /4096. Their integrator establishes /65536 turns/s. Existing
horizontal/reset comparisons now use this corrected boundary conversion.

Pitch acceleration uses front-minus-rear body forces; roll uses left-minus-right
forces for both axles. Sample each response product, then duration, before
adding to the old rate. The spring rate narrows to a signed halfword **before**
the damper increment. Damping cannot reverse the spring rate's sign. Otherwise
clamp rate to [-16384,16383]/65536 turns/s. Integrate and narrow the angle,
clamp ordinary tilt to ±512/4096 turns and clear its rate if clamped.
Roll spring uses +74/+78; roll **damping uses +6C/+70**, like pitch
(`800441E8/44200`). This asymmetry is preserved.

Rebuild the body basis from those angles, sum the adjusted spring forces and
apply vertical reciprocal mass, then duration, at separate sampling boundaries.
Project into all three world velocity components using the new body normal.
Gravity is 40141/4096 m/s² (`800443A0..C8`), sampled downward over the duration.
Apply the separate damper impulse, preserving its horizontal components but
clearing vertical velocity if it crosses signed zero. Ordinary integration
changes velocity for the next step; it does not move position here.

When an initial compression bit is set, rebuild vertical wheel attachments
using the integrated body basis and sample travel again with the **earlier**
road-normal/body-up projection. Do not recompute that ratio with the new tilt.
Narrow minimum-minus-travel to a signed halfword before clamping penetration
to zero. Blend front/rear penetration on each side and average each axle.
Unequal side/axle penetrations add the sampled direction-curve angle to roll
or pitch. There is no second ±512 clamp. Half the smaller side penetration
plus half the smaller axle penetration lifts the body, with signed-halfword
narrowing. Suspension travel/rate histories remain those sampled before this
correction; diagnostic penetration/lift are separate fields.

Even if all new compression bits clear, an initially compressed body receives
a downward-velocity floor. Project current velocity on the retained road
forward/lateral axes with their distinct wide-floor/low-word sampling. Project
those scalars onto vertical, cap the floor at zero, and raise vertical velocity
to it if needed. Horizontal velocity stays unchanged.

The later `8004335C` attachment refresh runs with the retained integrated body
basis, before `8003E7EC` rebuilds the body/road frames from corrected angles.
The connected road tests exposed this separate producer. Native road rules now
share the same attachment calculation at early sampling and late refresh.

## Reset and gates

For reset counts at least two, `800357C8` supplies a height above the prior
road centre: a half-second rising ramp near the start of the five-second
countdown, the 1228/4096 m plateau, and zero for the final half-second.
`80043AA4` snaps only when current body height is at or below that target.
Snapping clears tilt, tilt rates and vertical velocity, and skips ordinary
force integration. During the plateau, heading slews toward the event's target
by floor(dt × 341/4096) turns. The shortest direction is [-half turn,half turn),
with a negative exact-half-turn tie. Compression correction still follows.
The actual event record selection remains supplied by the race feature.

Mode 7 and the caller's global countdown skip the feature through
`SuspensionInput::disabled`, preserving persistent state. Runtime mode-7 and an
authored skip test cover this mapping; broader race/countdown flow is unported.

## Tests and extension boundary

Boundary runs cover signed rates and spring narrowing, rate/angle limits and
neighbors, signed/extreme body heights, reset phases with height equality and
snap/ordinary alternatives, heading slew neighbors and half-turn wrap, varied
reciprocal mass, asymmetric compression, inclined retained road frames and
damper forces. Sixty natural calls carry tilt/rates, airborne state and native
suspension travel/rate. The 60-call holdout uses 18 new signed height/angle/rate
probes and 42 alternating-control calls recorded after the rules were written.
The road corpus also carries native heights, bases, suspension and tilt history
through 156 transitions. It compares final position, angles, velocity,
attachments, frames, road metadata and wheel loads without corrected-pose input.

`VerticalBodyRulesPlugin` uses existing shared vehicle components and has no
rules-plugin dependency. Authored tuning can be inserted before the plugin;
`VerticalBodySetup` overrides it per car. The local exporter writes
`vehicle/vertical_body.ron`, including the shared car geometry/stop dimensions;
mods changing those dimensions should keep their suspension/road/body setups
consistent. No tables are committed. Optional bridges consume transmission
reset ticks and publish the airborne tyre-acceleration gate. A handmade test
runs the plugin alone with two setups, the optional bridge and a disabled car,
using no renderer or private assets. The source-only example disables it.

Workspace tests, the complete local asset rebuild, native game/example-mod
MCP render checks, and the engine sandbox movement/material checks pass.
Screenshots were inspected. Clippy reports only the pre-existing
wheel-rotation test warning. The current-source and reachable-history audits
pass; generated assets and original recordings remain private and ignored.

The [aerodynamics feature](aerodynamics-evidence.md) now supplies drag and axle
loads in connected forward/suspension/body and decoded-road comparisons.
Remaining work includes other body-force/proximity and condition producers,
licence reset/outcome flow, closing the full vehicle feedback loop and live
presentation, then the wider game and authoring editor in `PORT_PLAN.md`.
