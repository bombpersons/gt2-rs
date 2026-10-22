# Engine, gearbox and centre/axle rotation

`game/src/vehicle/drivetrain.rs` implements the coupled rotation phase at
`80045AE8`: choose transmitted engine/clutch torque, integrate untransmitted
torque into engine rotation, multiply through the selected gear, distribute
torque between axles, and evolve their common/split or separate angular
speeds. It now connects native engine torque to the previously ported wheel
differentials. Clutch-speed feedback at `8003932C` and fixed-clutch throttle
selection at `8003A178..194` are native too.

The source's alternate torque controllers `80045138` / `8004530C`, induction
response, engagement/gear selection, post-coupling engine feedback and body
motion remain separate work. The live scene still uses its checkpoint pose;
these subsystem comparisons do not establish playable driving.

## Identity and reproduction

Original: USA Simulation Mode Rev 2, Vitz F '99, B-1 checkpoint at vblank
7500. State SHA-256
`49fb814844da1b4d160f966c4cb675405fb58eb3d74ebc90702ad58978a2ef5b`;
RAM SHA-256 `ff147faec522bdd3da3f6231a379c4b09e323e6d094186bcb0a71a5a5441ab04`.

```sh
psx disasm --ram .psx/runs/b1-state1200 --addr 80045AE8 --count 700 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 800459A8 --count 80 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 800392D8 --count 82 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8003A000 --count 110 --text
python3 tools/record_drivetrain.py
python3 tools/record_drivetrain.py --holdout
# Recorded after the coupling and clutch-speed rules were implemented:
python3 tools/record_drivetrain.py --coupling-holdout
cargo test -p gran_turismo_2 --all-features --test drivetrain
```

The recorder captures phase boundaries, consuming values and completed
stores from original execution. It composes the slip/tyre/forward/wheel
recorders, requires completed runs, complete RAM watches and zero dropped
trace lines, and checks that controlled layouts, engagement states and axle
speeds reached the phase. Scratchpad stores come from the instruction trace:
the toolkit's RAM write watch does not emit scratchpad writes, even when
requested. Treating those absent events as zero would invalidate torque and
clutch-speed observations. The capture includes upstream scratch producers
as well as the solver itself.

All numerical CSVs and command/report/hash metadata stay in ignored
`game/tests/data/original/drivetrain/`; original traces and watches stay in
`.psx/runs/drivetrain-*`. The public exporter regenerates tuning locally
from the supported source checkpoint. No source table or fixture payload is
compiled into the public code.

## Fields and units

Offsets are relative to physics base `800A96B4`. Wheels begin at +460,
stride 68 hex; front-left/right precede rear-left/right. Torques and inertias
retain original normalized internal units; SI torque/inertia calibration is
not claimed. Angular velocities are rad/s; the sampled inverse-tau conversion
at `80039400` relates axle angular speed to engine revolutions/second.

| Source | Native meaning |
|---|---|
| +370 | Rear, front, viscous, open, clutch, adaptive-clutch or fixed-split layout (source kinds 0..6) |
| +372 | Torque-controller selector; ordinary kind 5 is covered here, alternate producers remain external |
| +373 | Reference axle to retain when centre slip crosses zero |
| +374 | Centre viscosity, or front torque fraction for clutch/split layouts |
| +376 | Centre lock threshold; nonzero also enables adaptive hard locking |
| +378, +400/+404 | Reflected engine / front/rear axle inertias, /16 |
| +3A4 + gear*4 | Gear ratio, /4096; slot zero has the separate reverse sign |
| +3F4/+3F6 | Fixed-clutch / nonpositive-torque clutch capacity |
| +408, +40C | Centre split rotation response (/256) / engine rotation response (/4096) |
| +618/+619 | Gear slot / engine engagement state, separate bytes |
| +624 | Engine rev/s, /4096 |
| +634/+638 | Front/rear axle angular velocity, /4096 rad/s |
| +63C | Persistent centre lock latch |
| +64A | Adaptive response angle, /4096 turn; its physical interpretation is still open |
| +4DE/+5AE | Signed-halfword angular integration history, /4096 |
| Scratch+10/+2C/+48/+64 | Prepared wheel reaction torques feeding the axle sums |
| Scratch+74/+78 | Distributed axle drive torque |
| Scratch+7C/+80 | Engine-side / geared shaft speed at the clutch, /4096 rev/s |
| Scratch+8C | Variable-ratio controller's reflected-inertia fraction; ordinary source uses one |
| Wheel+63 | Existing flags plus coupled/driven bits 2/3 |

`vehicle/drivetrain.ron` contains layout, reference axle, eight stored gear
slots, inertias, response/capacity parameters and source-immediate thresholds.
The native schema uses a vector of gear ratios so authored content can supply
its own transmission. The tuning resource is a default; an entity can attach
its own `DrivetrainSetup(DrivetrainTuning)` component to use different parameters through the
same shared systems. A handmade test runs front- and rear-driven cars with
different setups together. Zero unused slots in this car do not establish usable
extra gears. Unknown imported layouts/reference axles are rejected. Booleans
and enum variants export as native RON syntax.

## Rules and precision

The basic engine producer now has an explicit schedule boundary. It runs
after fixed-clutch throttle selection and before drivetrain integration.
Disconnected ordinary engines skip this torque call, preserving the fuel-cut
latch; external torque controllers can supply their own `EngineTorque`.
Existing standalone engine-torque use remains independent of the drivetrain.

Clutch-speed feedback selects the driven shaft according to layout, including
the open-centre mean and fixed-split weighted speed. Apply reverse, reject
nonpositive shaft rotation, then apply the gear and sampled inverse tau with
separate floor operations. Fixed-clutch launch control compares this speed
with current engine speed to choose zero/full effective throttle. Tests seed
that throttle with a sentinel, so observed engine torque proves the producer
ran before its consumer.

Direct engagement transmits net engine torque. Fixed-clutch engagement uses
its capacity; the other slipping states widen positive-torque capacity with
clutch-speed difference and use the coast capacity for nonpositive torque.
Capacity opposes the engine/shaft speed mismatch, and untransmitted torque
changes engine rotation using the source response and per-car duration.
Gear multiplication follows this choice, with the explicit reverse sign.
Engagement states 2 and 4 share this slip calculation, but state 2 has an
additional adaptive-centre lock condition and remains distinct.

Axle rotation follows the original drive layouts:

- Front/rear layouts distribute all gearbox torque to the driven axle;
  the other axle has zero effective inertia in this integration path.
- Viscous centre coupling transfers according to angular-speed difference.
- Open centre coupling evolves common rotation from summed torque/inertia,
  and the split from torque difference minus viscous resistance.
- Clutch and adaptive layouts retain a hard lock within the source torque
  threshold, otherwise transfer torque and latch when the speed split crosses.
- Fixed-split layout uses its authored front fraction through that same
  shared transfer/lock machinery, preserving the source's state handling.

The adaptive mode tapers response with absolute +64A angle and implements the
reverse, engagement, torque-direction and strict brake-threshold hard-lock
conditions in `800459A8`. A separate global-mode exception at `80046024..60`
uses the new increment directly for a locked adaptive centre under heavy
braking. The final holdout exercises its actual store, rather than inferring
coverage from input labels.

Reflected engine inertia is included only for direct engagement in the
appropriate source modes. Each division samples angular acceleration before
duration multiplication. Ordinary integration averages old/new increments;
signed-halfword history narrows after the full increment affects the tick.
The open-centre split clamp at `80046254` uses a signed low product word;
its replacement increment is **negative old half-split**, not zero.

Braked zero/crossing clamps use the returned word of the floored wide product
from `80075BF4`. Very high controlled shaft speeds can wrap that result and
stop despite retaining their mathematical sign. The final original probes
exercise these aliases in both directions. Keeping only a floating-point
sign comparison would fail them.

## Verification and boundaries

All **338 original car executions (1,352 wheel executions)** match exactly
for basic engine torque/fuel cut, clutch-speed feedback, engine rotation at
the coupling boundary, gearbox and axle torque, axle angular velocity,
integration histories, centre latch, wheel flags and downstream tread speeds:

| Suite | Car calls | Coverage |
|---|---:|---|
| Controlled boundaries | 142 | All seven layouts and five engagement states, both initial lock states, signed/reverse gearing, explicit lock acquisition |
| Natural gas/brake/turn | 60 | Twenty consecutive original physics calls each |
| Centre sequence | 20 | Consecutive centre lock acquisition and holding with nonzero history; release is covered by controlled calls |
| Independent signed holdout | 21 | Every layout with fresh small asymmetric shaft speeds and large signed histories |
| Independent mixed holdout | 20 | Simultaneous brake, right steering and handbrake |
| Final coupling holdout | 75 | Adaptive angle and strict brake-gate boundaries, alternate reference axles, additional gears, zero/crossing braking and high-speed product-word aliases |

Both slip stages, tyre force, brake allocation, forward integration, ordinary
engine torque and outer wheel rotation run natively in the same test tick.
Reverse plugin registration checks schedule order. Axle and wheel histories
and the centre latch carry natively across consecutive calls. A separate
handmade test runs the drivetrain plugin alone without assets, controls or
the other physics plugins.

The centre sequence exposes a genuine external reset: `80030830` can replace
an axle's speed between these phases. The recorder marks those actual writes,
rejects other unexpected upstream shaft writers, and the test applies them
explicitly while retaining native angular history and centre latch. Independent
wheel slots also retain their separate meaning. These are connected subsystem
comparisons, not a closed whole-car trajectory: contact/grip/load, auxiliary
forces, body velocities, ordinary throttle/gear/engagement decisions and such
contact corrections still enter at observed boundaries.

The plugin is independently replaceable in `GamePlugins`; the source-only
example mod disables this unused feature. `WheelRotationSystems::AxleDrive`
places it between reaction preparation and wheel integration. No template or
engine format change was needed.

The full workspace tests and opt-in native game/example-mod render checks
pass. The engine-only sandbox movement/material check passes, and its output
and both game screenshots were inspected. Clippy completes; its existing
collapsible-if warning in the earlier wheel-rotation test remains unchanged.
Current-source and reachable-history publication audits pass, with all
original assets, numerical fixtures and emulator outputs ignored.

Next: port ordinary post-coupling engine feedback and engagement transitions
(`8003AC24..AE90` and the corresponding preparation), actual gear-selection
timing, then lateral/yaw/world acceleration and contact production. Alternate
torque controllers, induction and their effects remain unverified producers.
Live wheel animation, a playable B-1 session and the rest of the game remain
unfinished.
