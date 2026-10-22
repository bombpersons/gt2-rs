# Lateral/yaw forces and horizontal world velocity

`game/src/vehicle/body_dynamics.rs` ports `8003AEDC..8003B304`: rotate
each wheel's force channels into body axes, accumulate lateral force and
yaw torque, integrate/clamp yaw rate, prepare lateral feedback and apply
body acceleration to horizontal world velocity. It retains the separate
reset, bypass, brake-stop and external-force branches.

All **513 original car calls (2,052 wheel executions)** match exactly.
The larger connected run includes native engine torque, axle/wheel coupling,
transmission timing and next-tick lateral feedback into the drivetrain.
This is still a subsystem boundary comparison: contact/grip/load, contact
orientation, collision corrections and upstream control requests remain
observed inputs. World velocity is initialized at the body boundary for each
call; these checks do not establish a closed driving trajectory. The rendered
car still uses a checkpoint pose, and playable B-1 remains unfinished.

## Identity and reproduction

USA Simulation Mode Rev 2, Vitz F '99, B-1 at vblank 7500.
State SHA-256 `49fb814844da1b4d160f966c4cb675405fb58eb3d74ebc90702ad58978a2ef5b`;
RAM SHA-256 `ff147faec522bdd3da3f6231a379c4b09e323e6d094186bcb0a71a5a5441ab04`.

```sh
psx disasm --ram .psx/runs/b1-state1200 --addr 8003AEDC --count 269 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8007596C --count 88 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8003E0C4 --count 47 --text
python3 tools/record_body_dynamics.py
python3 tools/record_body_dynamics.py --holdout
python3 tools/record_transmission.py --body-holdout
cargo test -p gran_turismo_2 --all-features --test body_dynamics --test transmission
```

Raw output stays in `.psx/runs/body-dynamics-*` and
`.psx/runs/transmission-body_holdout-*`. Numerical CSV/JSON stays in ignored
`game/tests/data/original/body_dynamics/` and `.../transmission/`.
Metadata records commands, checkpoint hashes and toolkit identity. Export
requires completed runs/watches, zero dropped trace lines and all four
body boundaries: before (`8003AEDC`), after lateral (`8003B0A4`), before
world application (`8003B0E4`) and after world application (`8003B304`).
The recorder checks actual wheel-force consuming addresses and input gates.

## State and units

Offsets below use physics base `800A96B4`. Wheels start at +460, stride 68 hex.

| Source | Native meaning |
|---|---|
| Wheel +54/+58/+5C | Lateral and two longitudinal force channels, original internal force units |
| +18/+1A, +0C/+0E | Front/rear side and forward lever arms, /4096 metres |
| +68 | Yaw response, converted from /4096 to turns/s² per internal torque unit |
| +2A | Body reciprocal mass, /4096; excludes wheel rotational masses |
| +34 | Lateral acceleration feedback multiplier, /4096 |
| +64C | Yaw rate, /524288 turns/s, clamped to [-1,1] |
| +64A | Lateral feedback, signed halfword /4096; its exact physical angle interpretation remains open |
| +724/+728 | Forward/lateral acceleration, /4096 metres/s² |
| +628/+62C/+630 | Original world velocity, /4096 metres/s; Bevy maps (x,z,-y) |
| +668/+66A, +670/+672 | Horizontal forward/lateral axes, /4096; supplied by orientation/contact processing |
| +6A4/+6A8 | Local forward/lateral velocity, /4096 metres/s |
| +650/+652 | Secondary angular quantities, /4096; their producer/physical interpretation remains unported |
| +6B1 | Nonzero bypass of tyre acceleration; upstream state meaning remains open |
| +718, +786 ==7, +78E !=0 | World-motion reset gates; the first-gear countdown retains vertical/secondary angular motion |
| Wheel +52, +63 bit 1 | Slip activation and nonzero allocated brake demand |
| +778/+77C | External horizontal world force, applied after the tyre/brake-stop gate |
| +730 | Reserved opposing lateral force accumulator, cleared by `8003A2C8`; all observed ordinary calls consume zero |

The exporter writes `vehicle/body_dynamics.ron`. `BodyDynamicsTuning` is
the default resource; `BodyDynamicsSetup` selects a per-car override.
`BodyDynamicsInput` exposes the upstream gates, axes and external forces.
`BodyDynamics` owns lateral results, feedback, local lateral velocity and
the secondary angular quantities needed by reset rules. `VehicleVelocity`
owns world velocity and yaw rate.

Force/torque remain original internal units; SI force and inertia have not
been established. The yaw response is normalized to turn units at import,
while the two original sampling boundaries remain 1/524288 turn/s² and
turn/s. `+64A` feedback is narrowed to a signed halfword before publication.

## Established rules

For each wheel, add its two longitudinal channels. With steering cosine c
and sine s, sideways force is `-c*lateral - s*longitudinal`; forward force is
`c*longitudinal - s*lateral`. Each multiply truncates separately toward zero
through `80075A5C`. Yaw torque is side-lever times forward force minus
forward-lever times sideways force, again with separate sampled products.
Both lateral and longitudinal channels therefore contribute to yaw.

The yaw response and duration apply in separate steps (`8003B028..40`),
followed by the inclusive signed clamp at `8003B048..6C`. Lateral acceleration
uses the body reciprocal mass and the reserved opposing-force accumulator;
it does not reuse the forward phase's effective mass. The feedback multiply
and signed-halfword store happen at `8003B098..A0`.

Nonzero +6B1 clears lateral acceleration/feedback and leaves yaw unchanged.
It also skips the later tyre-acceleration path, but still admits external
world force. Its producer is not included here.

The world reset gates clear horizontal velocity, yaw and the accumulated
forward force, then skip external force. A nonzero first-gear countdown
preserves vertical velocity and the secondary angular quantities; the other
reset paths clear them. Forward/lateral acceleration are retained on this
reset path, matching its stores rather than applying a generic full reset.

With no reset/bypass, the brake-stop branch requires **all** wheel slip
activations zero, **any** allocated brake flag set and local forward speed
within inclusive ±1137/4096 m/s. It clears horizontal/yaw/local motion,
forward force, both accelerations and lateral feedback. Vertical/secondary
angular motion survives. External world force is still applied afterwards.

Otherwise, sample each body acceleration times duration, project through
the two horizontal axes and add to world velocity. The projection uses the
signed low-word `8007596C` helper, distinct from the wide force/lever
products. External force applies body reciprocal mass and duration with
separate wide truncations (`8003B2D4..E8`). Vertical integration/collision
are not performed by this stage.

## Verification

| Suite | Calls | Evidence |
|---|---:|---|
| Steering/yaw probes | 70 | Ten signed steering axes and seven initial rates around both yaw clamps |
| Reset/bypass combinations | 16 | +718, forced mode 7, first-gear override and signed bypass, with nonzero external forces and secondary/vertical motion |
| Brake-stop gates | 175 | Seven exact speeds, each individual brake/no brake, each individual activated wheel/no activation |
| World axes/external force | 16 | Four axes including signed/nonorthogonal pairs, zero/tiny/asymmetric external forces |
| Natural gas/brake/turn | 120 | Forty calls each with native slip, tyre and forward phases |
| Fresh body holdout | 64 | Eight asymmetric states, six exact clamp probes, four new steering/external states, four feedback-narrowing variants, two external-force-after-stop probes, forty alternating natural calls |
| Combined transmission/body | 52 | Twelve controlled states plus forty alternating calls; native engine, gearbox, axle/wheel history and next-tick feedback |

Native tests compare intermediate and final yaw, all three world velocities,
both secondary angular values, lateral acceleration/feedback/local velocity,
forward force/acceleration/speed and the original lateral-force/yaw-torque
accumulators. Native slip activation and all three tyre force channels match
their original producers and consumers too. Coverage assertions require
both yaw clamps, all reset combinations, each braked/activated wheel, exact
brake-stop endpoints, external force after stopping and signed feedback wrap.

Controlled runs freeze contact preprocessing through the established mode-1
boundary. They do not assume a poke survives: +718 is overwritten by
`8003E120` from the countdown at `800A9520`, itself decremented at
`80015CFC`. Reset probes change that producer and check the consumed flag.
Likewise, grip capacity is rebuilt at `80039D98`; tests use the observed
producer output, not a requested wheel-capacity poke. Opposing force +730
is explicitly observed as zero after accumulator initialization. The
nonzero authored opposing-force path has binary evidence and a handmade
test, not an original nonzero runtime comparison.

The fresh holdout was recorded after implementation and did not change the
native rules. Feedback narrowing uses two altered +34 tuning values, verified
at the original boundary and supplied through native per-car setups.
The combined run retains engine/gear/engagement/timer/fuel-cut/axle/wheel
state as in the transmission tests, and additionally retains native lateral
feedback into the next drivetrain call. Contact/body velocity inputs remain
fresh observations; closed body/collision trajectories are not claimed.

The handmade test adds only `BodyDynamicsRulesPlugin` with authored tuning
and explicit components: nonzero tyre forces, lever torque, world acceleration,
external force, two per-car setups and first-gear reset preservation. It
also proves optional transmission-state input and drivetrain-feedback output
without those plugins, renderer, controls or original assets.

`BodyDynamicsSystems::{Accumulate, ApplyWorld}` run after forward dynamics
and transmission selection, before motion-step construction. The example mod
disables this unused feature. No template or engine edit is needed.

Workspace tests, native game/example-mod MCP render checks and engine sandbox
movement/material checks pass; screenshots were inspected. No new visual
parity is claimed. Clippy has only the existing wheel-rotation test warning.
Current-source and history audits pass; all generated/reference data stays ignored.

Grip conversion and late contact-motion refresh are now covered by
[contact inputs](contact-input-evidence.md). Next work is road/suspension
load, surface/orientation production and collision, followed
by a closed B-1 trajectory and live presentation. Alternate torque controllers,
induction, physical request production, wheel animation, audio, licence flow,
the wider game and the authoring editor remain unfinished.
