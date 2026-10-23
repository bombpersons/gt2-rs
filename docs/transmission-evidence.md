# Ordinary engine feedback and transmission timing

`game/src/vehicle/transmission.rs` implements the ordinary transmission path:
advance the first-gear countdown, prepare engagement, complete engine-speed
feedback after axle/wheel coupling, refresh integer RPM, choose a gear and
apply its engagement transition. Both automatic and manual selection reuse
the original layout, wheel-contact and longitudinal-slip rules.

The connected tests now retain engine rotation, RPM, fuel-cut state, gear,
engagement, engagement request and both timers across consecutive physics
calls. Contact/body motion is composed separately in the retained loop;
[ordinary driver requests and idle launches](driver-request-evidence.md)
now supply the digital producer boundary as well. A complete licence session
is unfinished. Special one-/two-ratio torque controllers and induction remain
separate producers, as recorded in the drivetrain evidence.

## Identity and reproduction

Original: USA Simulation Mode Rev 2, Vitz F '99, B-1 at vblank 7500.
State SHA-256 `49fb814844da1b4d160f966c4cb675405fb58eb3d74ebc90702ad58978a2ef5b`;
RAM SHA-256 `ff147faec522bdd3da3f6231a379c4b09e323e6d094186bcb0a71a5a5441ab04`.

```sh
psx disasm --ram .psx/runs/b1-state1200 --addr 8003E168 --count 5 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8003A060 --count 80 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8003AC24 --count 160 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 800448C8 --count 375 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8003991C --count 30 --text
python3 tools/record_transmission.py
python3 tools/record_transmission.py --holdout
python3 tools/record_transmission.py --shift-holdout
# Fresh controlled inputs and alternating natural controls after rule fixes:
python3 tools/record_transmission.py --final-holdout
cargo test -p gran_turismo_2 --all-features --test transmission
```

Original traces, watches and reports stay in `.psx/runs/transmission-*`;
CSV/JSON fixtures stay in ignored `game/tests/data/original/transmission/`.
The recorder requires complete execution, complete RAM watches and zero
dropped trace lines. Commands, source hashes and toolkit version accompany
each recording. It checks interventions at their consumers and rejects
missing phase boundaries or unobserved input bytes.

Six boundaries are recorded: timer before decrement (`8003E168`), initial
engagement state (`8003A060`), completed preparation (`8003A198`), completed
coupling (`8003AC24`), pre-selection after RPM refresh (`8003991C`) and
completed selection/application (`8003AE94`). Native test observers compare
the intermediate states as well as the final state, so a later phase cannot
hide an earlier error.

Scratchpad requests are captured at their producers (`8002FFEC/FFF0`,
`80030058/30064`, `80030170/30174`), even if a cooldown prevents their later
consumption. Controlled runs use the established mode-1 control bypass and
explicit scratchpad pokes. No absent observation is treated as a zero request.

## State, tuning and public API

Offsets use physics base `800A96B4`; wheels start at +460, stride 68 hex.

| Source | Native meaning |
|---|---|
| +60A | Nonzero gearbox-engagement request; preparation can clear it |
| +618/+619 | Separate gear-slot and engagement bytes |
| +61C | Shared shift/reengagement cooldown byte |
| +78E | First-gear override countdown, a halfword decremented before input |
| +624/+6AC | Engine rev/s (/4096) and separately refreshed signed-halfword RPM |
| +610/+708/+612 | Accelerator travel, effective engine throttle and brake travel, /4096 |
| +642/+786 | Selection option / force-automatic flag |
| Scratch+364/+365 | Reverse request / signed manual shift direction |
| +372 | Forward-gear count; also selects the special one-/two-ratio torque paths |
| +394/+396/+398… | Minimum/high shift RPM and per-gear lower RPM boundaries |
| +3C4 + gear*4 | Engine rev/s per m/s for body-speed gear limits, /4096 |
| Axle +1FC/+1FE, +2D4/+2D6 | Upshift/downshift slip limits, /4096 |
| Wheel +8/+2A/+44 | Contact load, speed response and signed longitudinal slip |

`TransmissionState` owns the request and both counters; `DrivetrainInput`
retains gear and engagement. `EngineTorqueInput` retains rotation and RPM.
`TransmissionInput` supplies logical requests and options. Physical pad
mapping, gear-option menus and the other writers of these fields are not
claimed by this feature. `Disabled` names the selector's no-selection
fallback for option values other than its automatic/manual branches.

The exporter writes `vehicle/transmission.ron` with RPM bands, speed
coefficients, slip limits and source-immediate thresholds. The engagement
delay is the original tick-rate word at `801C8570` divided by five and
narrowed to a byte. Native ticking uses the actual fixed update, with no
frame-count gate that could lose input edges.

The resource supplies a default setup; `TransmissionSetup` selects different
authored parameters for one entity. A handmade test runs two different shift
setups together using only this rules plugin, explicit axle/engine components
and handmade tuning. Engine and drivetrain tuning are explicit data
dependencies; their other plugins, the renderer, controls and private assets
are not required by that test. The example mod disables this unused feature.

## Rules and source details

Preparation preserves the source's direction and strict threshold tests.
A disconnected requested drivetrain moving in the selected gear's direction
uses the fixed-clutch launch path only above the throttle gate. Otherwise it
enters synchronization above the minimum RPM, or clears the request. A
coupled drivetrain disconnects when that request is zero. Fixed-clutch
throttle selection and clutch-speed preparation then run before engine torque.

After wheel coupling, disconnected ordinary engines request torque once,
integrate rotation and **raise it to idle**. This branch has no redline
ceiling. The idle field is +10A, addressed as +8E relative to the engine-curve
descriptor; interpreting it as redline or reversing the comparison changes
the original results. Connected engines follow geared shaft speed, or
complete synchronization when the old/new speed differences cross with the
original inclusive equality tests. Reengagement starts the shared cooldown.
Remaining launch/synchronization states can disconnect at the idle/brake
boundaries. The connected path alone clamps rotation to zero through its
redline-plus-margin ceiling. RPM is refreshed and narrowed before selection.

First-gear override precedes mode selection and both cooldown paths. Manual
selection handles reverse and recovery from reverse before checking its
repeat timer. Its speed helper returns zero for negative motion, but its
caller explicitly substitutes first gear. A held shift request rearms the
one-tick repeat latch without another shift; release clears it. Automatic
selection decrements a nonzero cooldown and immediately returns, then handles
direction/low-speed choices and body-speed safety shifts before RPM/slip
selection.

Ordinary RPM shifts require direct engagement. High throttle lowers the
downshift boundary, bounded by minimum RPM plus margin. The first and top
gears retain their separate sentinel bands. Contact and slip checks use only
the source-selected axles: rear for rear/clutch/adaptive layouts, front for
front/**viscous** layouts, both for open/fixed-split layouts. A zero-load wheel
blocks the shift. Downshift slip uses a strict upper comparison; upshift uses
the signed low-word product of response and the negated limit, then negates
and narrows the result to a halfword before its strict lower comparison.
Equality remains eligible in both directions.

Applying a selected gear leaves a disconnected drivetrain disconnected.
Otherwise, transitions to or from reverse use synchronization; other changes
use fixed clutch. The branch delay slot at `80039964` supplies the reverse
case's state value. Gear selection and engagement application are tested as
separate outcomes.

## Verification and remaining work

All **723 original car calls (2,892 wheel executions)** match exactly at the
recorded boundaries, with native slip, tyre forces, braking allocation,
forward integration, ordinary engine torque, axle and wheel coupling feeding
the new rules in the same tick:

| Suite | Calls | Coverage |
|---|---:|---|
| Controlled preparation/selection | 84 | All five engagement states, three selection modes, strict launch speed/throttle/RPM boundaries, forced automatic and first-gear behavior |
| Natural gas/brake/turn | 120 | Forty consecutive calls each, with native engine/gear/engagement/timer/fuel-cut state |
| Held manual requests | 40 | Both directions, one shift per held request and shared cooldown behavior |
| First-gear countdown | 10 | Override expiry, including subsequent reverse selection |
| Signed controlled holdout | 24 | New asymmetric speeds, requests, gears and cooldown state |
| Natural mixed holdout | 40 | Simultaneous throttle/brake/right steering |
| Exact shift/feedback holdout | 353 | Exact RPM bands, all seven layouts' wheel slip/contact gates, safety/manual speed boundaries, direction gates, idle/ceiling/equality/braking feedback boundaries |
| Final independent holdout | 52 | Twelve fresh controlled states plus forty alternating natural throttle/brake/steering/handbrake calls, recorded after rule fixes and not used to fit them |

The exact-boundary suite sets source torque ordinates, drag, clutch capacities,
grip and aerodynamic input to zero and controls shaft rotation. It calibrates
input shaft speed until the **original consumer** reaches the requested RPM
or clutch-speed boundary; expected gear/state outputs still come exclusively
from original execution. This isolates thresholds, not a natural vehicle
trajectory. All calibration attempts are retained. The suite includes 224
slip/contact probes; coverage checks require both allowed and blocked shifts
for every layout, and the recorder verifies the requested slip at selection.

Consecutive tests retain owned state instead of reseeding it from every
original row. Fresh driver throttle/requests, contact inputs and explicitly
observed `80030830` shaft resets remain external. Both timers and the request
state, as well as engine rotation, RPM, fuel cut, gear and engagement, carry
natively. The handmade test separately proves held/released manual requests,
override timing and per-car setups without original data.

Workspace tests and the native game/example-mod MCP render checks pass;
the engine-only sandbox remains usable. No new visual parity is claimed.
Source/history audits keep all generated tuning, numerical originals and
emulator outputs private. Clippy completes with the unchanged earlier
wheel-rotation test's collapsible-if warning.

Lateral/yaw/world acceleration (`8003AEB8..8003B30C`) is now covered by
[body dynamics](body-dynamics-evidence.md), including 52 additional combined
transmission/body calls and next-tick feedback. Contact/load/grip production
and collision remain next.
Special torque-controller/induction behavior, logical request production and
their physical bindings, live wheel animation, a playable B-1 session and
the wider game remain unfinished.
