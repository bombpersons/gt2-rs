# Vehicle dynamics investigation

The central ordinary engine/clutch/gear/centre/axle phase is now ported with
338 original car comparisons; see [drivetrain evidence](drivetrain-evidence.md).
Ordinary engagement preparation, post-coupling engine feedback and shift
timing now match 723 calls with consecutive engine/gear/timer state; see
[transmission evidence](transmission-evidence.md).
Load/camber/surface grip and late world-to-contact motion now match 380 calls,
including the next-tick contact inputs and wheel/axle synchronization; see
[contact-input evidence](contact-input-evidence.md). Body force phases are
covered separately by [body evidence](body-dynamics-evidence.md).
The notes below retain the original investigation context and source map.

This is an evidence ledger for the next dynamics work, not a claim that the
vehicle force model is ported. The original target and RAM digest are the
Rev 2 USA Simulation Mode B-1 checkpoint identified in
`docs/driving-evidence.md`. All offsets here are relative to the **physics**
base `0x800A96B4`, unless explicitly marked as a containing record. Four wheel
records start at `+0x460`, with stride `0x68`; containing car records have
stride `0xB40` and add `0x2C` to reach their physics base.

## Questions and current answers

| Question | Result | Evidence boundary |
|---|---|---|
| What executes under `80039FC8`? | Phased wheel/body/drivetrain processing, mapped below | Instructions in the checkpoint overlay, plus existing two-tick execution trace |
| Which force rules are now ported? | Paired tyre curves, longitudinal channels, lateral force and low-speed correction | 972 original wheel executions; grip/contact inputs remain measured |
| Can wheel braking preparation be ported independently? | Yes: allocate effective brake travel to wheels and add rear handbrake | Exact normal-driver source and calculation identified; tyre/ABS effects still separate |
| Are complete body forces understood? | No | Tyre, transmission, coupled wheel and contact paths remain incomplete |

REA MCP tools were absent and `npx` unavailable in this session. Existing
project `psx` analysis and execution tools are the fallback. The cached
Ghidra identity is
`.psx/ghidra/6079f305775d90a897af9b385e40f59a0157de7f1a92f471f08ab3a339101b12/analysis.json`;
the retained `1792515169087-2-1/decompile-80039FC8.json` is useful navigation,
but conclusions below were checked against instructions. Generic MIPS/GTE
pseudocode is insufficient by itself.

## Correction: engine speed and RPM storage

The old `docs/driving-evidence.md` interpretation of `+0x6AC` as a 16.16
RPM word is disproven by `80039458`, which is a **halfword** store. The two
halves of that word are distinct fields:

| Offset | Type and role | Checkpoint value |
|---|---|---:|
| `+0x624` | signed word, engine revolutions/second ×4096 | 414689 (101.24243 rev/s) |
| `+0x6AC` | signed halfword, calculated integer engine RPM | 6074 |
| `+0x6AE` | unsigned halfword, scaled absolute local forward speed (`80030594`) | 5604 |
| `+0x6F8` | unsigned halfword, maximum of that speed quantity (`80030598`) | 5604 |
| `+0x108` | unsigned halfword, fuel-cut threshold in RPM | 6800 |
| `+0x10A` | unsigned halfword, engine idle RPM parameter | 600 |

`8003941C..80039470` sets:

```text
calculated_rpm = max(floor(engine_rps_q12 * 60 / 4096), idle_rpm - 1)
```

It writes only `+0x6AC`. The lower bound is deliberate: the `slt` at
`80039448` tests calculated RPM **below** `idle_rpm - 1`.
The result is narrowed to a halfword at the store. The current normal engine
range is positive; behavior for implausibly large or negative modded values
has not been established as a useful gameplay contract.

The existing trace `.psx/runs/driving-control-probe/trace.txt`, frame 7500,
shows the real calculation after this tick's drivetrain phase:
`+0x624 = 0x6194E = 399694` -> `5854 RPM = 0x16DE`, stored at `80039458`.
The checkpoint's earlier value 6074 is consistent with its earlier engine
speed, not with treating adjacent `5604` as a fraction. The update mechanism
of `+0x6AE/+0x6F8` is now established by the contact-motion producer and
runtime comparisons: `floor(abs(forward_q12) * 0xDFBDD / 2^24)`, narrowed
to a halfword, followed by an unsigned maximum. These are not RPM fields.
Their final HUD formatting remains separate; see contact-input evidence.

The paired tyre response and per-wheel force phases described below are now
ported and checked against 972 original wheel executions. See
[tyre force evidence](tyre-force-evidence.md). Their grip/contact inputs and
subsequent drivetrain/body coupling remain unfinished.

## Dispatcher phases and ordering

`80039FC8(a0=containing_car_records, a1=count)` loops over all cars in each
phase, rather than completing every phase for one car first. It uses a
temporary block of stride `0x90` per car at scratchpad `0x1F800000`; wheel
temporary entries start at `+4` with stride `0x1C`. `+0` is the global
current integration duration copied from the current car's `+0x6FE`.

| Instruction region | Established operation | Remaining uncertainty |
|---|---|---|
| `8003A014..A198` | Dispatch drivetrain state preparation by `+0x372` and `+0x619`; some paths call `8003932C` or `80039470` | Complete state enum and all transmission variants |
| `8003A19C..A278` | Per-wheel brake allocation, rear handbrake, initial wheel flags | Subsequent assists may alter wheel flags |
| `8003A2A0..A364` | Clear force/acceleration accumulators; calculate signed longitudinal surface resistance into temporary `+0x88` | Physical unit calibration and surface enum names |
| `8003A368` -> `800397D0` | Sample longitudinal response and cornering limit from signed longitudinal slip | Ported; tyre-force evidence |
| `8003A380..A450` | Scale response by grip capacity; split wheel-reference/ground-reference force channels using the longitudinal slip enum | Ported; these flags do not mean grounded/airborne |
| `8003A45C..A6F4` | Calculate lateral force from folded relative steering/slip angle, activation and cornering limit, plus low-speed correction | Ported; grip/contact production remains separate |
| `8003A710..A938` | Sum wheel-reference force/resistance, derive effective reciprocal mass, update forward speed, apply the ground-reference/lateral channel with a signed stopping clamp | Ported; drivetrain coupling remains separate |
| `8003A93C` -> `80046B58` | Calls `80045688`, `800457B0`, `80045AE8`, `800465E0` in order | Outer wheel and central ordinary drivetrain phases ported; alternate torque controllers remain external |
| `8003A964..AEA0` | Update engine state; mode `+0x372 == 1` uses `80039994`, mode 2/other have separate branches; update integer RPM then gear-selection path | Ordinary feedback, RPM refresh and automatic/manual gear logic ported; special torque-controller producers remain external |
| `8003AEB8..B0C8` | Accumulate lateral/yaw effects from wheel forces; update angular-motion input `+0x64C` | Ported; lever/yaw tuning, both clamps and lateral feedback verified; body-dynamics evidence |
| `8003B0D0..B30C` | Transform body acceleration into world horizontal velocity `+0x628/+0x62C`; special immobilization and zero-speed branches | Ported; reset/bypass/brake-stop/external-force gates verified; orientation/contact producers remain separate |
| `8003B310..B444` | Conditional per-wheel accumulation at wheel `+0x64` and byte output `+0x3E` | Wear/heat interpretation not proven |
| `8003B448..B564` | Conditional checks and updates to `+0x78D` flags | Meaning of end-state flags |

The existing trace confirms `800397D0`, `80046B58`, `8003941C` and
`8003991C` on the real B-1 path at frames 7500 and 7502. Static reachability
of alternate branches does not mean those branches were exercised.
The caller later constructs motion steps in `80030330`; these phases must
precede that step construction.

## Cohesive engine torque and fuel-cut rule

The recognizable rule is: **sample available engine torque at current
engine speed, subtract speed-dependent engine drag at closed throttle, and
cut combustion above redline until RPM drops more than 500 RPM.**

### Available torque and drag

Before controls and `80039FC8`, preparation at `8003E1F4..8003E220` computes:

```text
full_torque = linear_curve(engine_torque_curve, engine_rps_q12)
drag = floor(drag_coefficient_q12 * engine_rps_q12 / 4096)
```

`full_torque` is stored at `+0x710`; `drag` at `+0x714`. The torque curve
descriptor is at physics `+0x7C` (containing record `+0xA8`), with `u16`
point count, pointer to signed-word x coordinates at `+4`, and pointer to
signed-word y coordinates at `+8`. `80075D2C` clamps outside the end knots
and linearly interpolates inside. Its interpolation uses a signed 64-bit
product and signed division through helper `80086084`, so the interpolated
delta truncates towards zero. Endpoint clamping is part of this curve's
contract; extrapolation would change behavior.

The Vitz checkpoint has 16 curve points. The exporter reads both arrays
through the curve descriptor into the ignored `vehicle/engine.ron`; the
original coordinate/ordinate tables are not reproduced in this repository.

These are 800..6800 RPM knots at 400 RPM intervals, with original x
quantization preserved. The checkpoint pointers are `800A973C` and
`800A977C`. The speed-dependent drag coefficient at `+0x10C` is raw `9`.
For the checkpoint engine speed 414689, this gives drag 911 after the
preparation pass (the stored checkpoint value 945 is from an earlier pass).
**No conversion to N·m or kgf·m has been proven.** A port can expose a named
original-torque unit at this boundary until the inertia/force scales are
decoded; it must not label these ordinates N·m.

### Hysteresis and effective throttle

`8003533C` takes the car physics base and effective normalized throttle in
Q12. Its persistent state is a byte at `+0x61D`, which has the following
recognizable boolean meaning:

```text
if not fuel_cut:
    if integer_engine_rpm >= redline_rpm:
        fuel_cut = true
else:
    if integer_engine_rpm < redline_rpm - 500:
        fuel_cut = false

combustion = 0 if fuel_cut else floor(throttle_q12 * (full_torque + drag) / 4096)
net_torque = combustion - drag
```

The cut decision happens **before** torque is produced on that call.
At exactly redline-500, the existing cut stays latched. At full throttle
and no cut the result is `full_torque`; at zero throttle or with cut it is
`-drag`. `80075BF4` computes signed product >>12 without adding a rounding
bias, so negative products floor, not truncate towards zero.

`800353DC` wraps this basic rule with optional induction-response work.
At this checkpoint its halfword `physics+0x128` is zero; the wrapper simply
copies its effective throttle to `+0x61E`, clears a byte effect, and calls
`8003533C`. Nonzero induction settings require the other branch to be
ported before claiming parity for those cars. The effective throttle here
is `+0x708`, copied from pedal travel at `8003E2DC` but modifiable by
intervening control/shift routines; using pedal travel directly throughout
the whole drivetrain would skip those rules.

Original runtime boundary probes were performed after deriving the rule:
all restore the original checkpoint, run two fields, and poke **only**
integer RPM `800A9D60` (width 2) and cut state `800A9CD1` (width 1) at frame 0.
The trace proves each tested branch executes. These are controlled branch
experiments, not natural acceleration trajectories.

| Initial RPM | Initial cut | Result on call | Evidence directory |
|---:|---:|---:|---|
| 6799 | 0 | stays clear | `/tmp/gt2-limiter-below-redline` |
| 6800 | 0 | latches at `80035380` | `/tmp/gt2-limiter-at-redline` |
| 6300 | 1 | stays latched | `/tmp/gt2-limiter-at-release` |
| 6299 | 1 | clears at `8003539C` | `/tmp/gt2-limiter-below-release` |

Each report has `status=complete`, complete write watches, and zero trace
lines dropped. Example reproduction (other cases change RPM/cut/output):

```sh
psx run --from-state .psx/runs/b1-state1200/state.bin --frames 2 --input '[]' \
  --poke 800A9D60=6800:2 --poke 800A9CD1=0:1 \
  --trace --trace-start 8003533C --trace-range 8003533C-800353DC \
  --max-events 2000 --watch 800A9CD1:1 \
  --out /tmp/gt2-limiter-at-redline --summary
```

The existing natural trace also observes the closed-throttle call at frame
7500: effective throttle 0, available torque 324, drag 911, returned torque
`-911`. All four controlled cases use the checkpoint's intervening throttle
behavior; a future trajectory recorder should record the effective throttle
**at this function's consumer**, not assume it equals CROSS or pedal travel.

## Independent wheel-brake preparation

Normal live controls copy pedal brake travel `+0x612` to all four effective
wheel brake channels `+0x4C0+i*0x68` in `8003E2E0..8003E2F4`. Other writers
exist (including `800374EC`, `800377B8`, `80038438`, `800384F8`), so these
channels should remain independent from the pedal for AI/assist extensions.

The copy is **not** the final effective input even on this normal-driver
path. `8003E794` calls `8003DBE8..8003DE60` before the force allocator;
`8003DE4C` rewrites wheel `+0x60`, which is the same effective channel.
The original SQUARE run at frame 7502 copies 1365 to all four channels,
then the allocator consumes 469 at both front wheels and 383 at both rear
wheels. Replaying the pedal directly into the final allocator would skip
this adjustment and overstate the prepared brake demand.

`8003A1A4..A1E4` multiplies each effective wheel channel by its axle's
coefficient, shifted right 12. Axle is `i/2`; indices 0/1 use `+0x414`,
2/3 use `+0x418`. `8003A1E8..A22C` adds handbrake `+0x614 * +0x41C /4096`
to indices 2/3 only. There is no subtraction, override, or clamp to a maximum
at this preparation stage: normal braking and handbrake add on the rear.

Checkpoint coefficients are `31752`, `27048`, `23520`, respectively. Their
absolute physical units are open. Dividing them by 4096 would give 7.751953125,
6.603515625 and 5.7421875, but this is not a proven physical-unit conversion.
The native allocation module keeps the original integer demand unit.
The output is at scratchpad `+4+i*0x1C`, then used downstream together with
wheel-contact/drive forces. A complete braking model requires that downstream
coupling, not just this allocation.

Exact arithmetic warning: normal brake calculation uses low-32-bit product
then arithmetic >>12. The handbrake path uses low product, left shift 4 then
arithmetic >>16, effectively narrowing the result to a signed halfword.
For normal channel values 0..4096 and these coefficients, neither changes
the positive mathematical product/4096 result; arbitrary oversized modded
coefficients are not covered by that equivalence.

At `8003A23C..A274`, initial wheel flag byte `wheel+0x63` gets bit 1 when
the allocated brake output is nonzero, and bit 0 when wheel `+8` is zero.
This supports the interpretation that `+8` is contact/load-related but does
not prove all semantics of that field. Later drivetrain phases add flags.

## Surface resistance and remaining dependency map

`8003A2D0..A2FC` sums, for each wheel, `floor(wheel+8 * surface_coefficient /
4096)`, selecting its coefficient with byte `wheel+0x14` into `80046E00`.
The bounded eight-entry coefficient array is exported locally; its values
are not distributed with source.
`8003A300..A354` evaluates the curve descriptor `801C8730` at
`abs(forward_velocity_q12)` and multiplies it by this sum, negating the
result for positive forward speed and keeping it positive for negative
speed. Zero speed produces zero. This is a signed resistance term, rather
than an automatic shift band or RPM rule.

The curve coordinates/ordinates are exported locally through that descriptor.
The same clamped linear-interpolation helper `80075D2C` is used.
This resistance rule and the complete pre-drivetrain forward phase are now
ported and checked against 183 original car executions; see
[forward dynamics evidence](forward-dynamics-evidence.md). Surface names,
contact/load production and final body/drivetrain coupling remain open.

The next broad dependency is `80046B58` and its four callees. This path
couples effective engine torque, gear ratios, wheel state and body forward
motion. Preserve the phased update order, especially that engine torque
curve sampling happens before this tick's engine-speed update and that
`8003941C` refreshes integer RPM after the update. Full force-model parity
also needs load/contact, lateral tyre forces, yaw and stationary/reverse
cases. Neither the engine subsystem nor wheel-brake allocation alone can
generate the recorded `MotionStep` trajectories.

Focused static reproduction:

```sh
psx disasm --ram .psx/runs/b1-state1200 --function 80039FC8 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8003E1E4 --count 76 --text
psx disasm --ram .psx/runs/b1-state1200 --function 8003533C --text
psx disasm --ram .psx/runs/b1-state1200 --function 800353DC --text
psx disasm --ram .psx/runs/b1-state1200 --function 8003941C --text
psx disasm --ram .psx/runs/b1-state1200 --function 80075D2C --text
psx disasm --ram .psx/runs/b1-state1200 --function 80046B58 --text
```

## Native subsystem and trajectory coverage

`game/src/vehicle/engine_torque.rs` now implements this bounded engine
subsystem as a rules-only feature with ordinary Bevy components. It samples
the asset curve, computes engine drag, maintains the fuel-cut latch and
produces net torque. `EngineTorqueTuning::refresh_rpm` provides the original
integer refresh rule for a future drivetrain integration phase. It is not
automatically run at the torque-sampling boundary, because that would use
the wrong phase's engine speed.

`EngineTorqueInput` explicitly accepts engine revolutions/second, last
integer RPM and effective throttle. `EngineFuelCut` preserves the stateful
latch; `EngineTorque` exposes available torque, drag and net torque. The
system runs in `VehicleSystems::Forces` and `GameSystems::Game`, and has no
rendering dependency. The module uses floating-point natural speed units;
the original one-unit torque quantization is kept at its named output
boundaries. Tuning lives in `game/assets/vehicle/engine.ron`, with speed
knots converted once to revolutions/second. Torque is honestly named in
original internal units, without an invented N·m conversion.

`tools/record_engine.py` records every required value at its consumer or
writer, rather than using frame-end snapshots for transient arguments.
`80035348` exposes effective throttle and `800353D0` the returned torque;
read/write watches identify engine-speed input, curve output, drag, RPM,
and the latch. The exporter requires exactly one complete engine call per
two fields and rejects dropped trace lines or incomplete watch logs.
Commands and checkpoint digests are exported beside the numeric data in
`game/tests/data/original/engine/`; full artifacts stay in `.psx/runs/engine-*`.

The runtime tests compare **168 consecutive original calls**, in seven
scenarios, with zero tolerance for available torque, drag, net torque,
the stateful latch, and refreshed integer RPM:

| Scenario | Calls | Coverage |
|---|---:|---|
| Gas / coast / brake | 30 each | Natural speed changes and shift-related effective throttle changes |
| Limiter | 18 | Natural shift finishes before full-throttle controlled RPM probes; cut and recovery preserve state across calls |
| Independent limiter holdout | 20 | 6801/6301/6300/6299/6799/6800 boundaries, with cut suppressing full-throttle combustion |
| Independent mixed holdout | 30 | Effective throttle includes 0, 1, 1365, 2730 and 4096 in original Q12 units |
| Independent curve holdout | 10 | Controlled speed probes below/at/above curve endpoints, interior knots and interpolation on rising/descending segments; idle RPM lower bound |

The holdouts were recorded after the implementation was written. Limiter
probes change **integer RPM only** beginning at run frame 24, allowing the
checkpoint's existing shift to finish first; the latch is initialized once
and thereafter remains the original program's persistent state. The curve
holdout changes engine revolutions/second at the start of each tick, with
raw probe values `4096,54613,70000,109226,273066,286000,382293,450000,464213,491520`.
These controlled branch experiments supplement natural trajectories; they
do not establish how the original drivetrain reaches those states.

```sh
python3 tools/record_engine.py --scenario gas --scenario coast --scenario brake --scenario limiter
python3 tools/record_engine.py --scenario limiter_holdout --scenario mixed_holdout --scenario curve_holdout
cargo test -p gran_turismo_2 --all-features --test engine
```

The step builder also settles the angular-input unit dimensionally:
`+0x64C * dt_q16 /2^23` produces a yaw increment in 1/4096 turn, so the
angular input is `+0x64C /524288` turns/second. This dimensional inference
does not yet identify the force/inertia parameters that produce yaw.

## Native wheel-brake preparation and independent holdouts

`game/src/vehicle/braking.rs` now implements ordinary demand per effective
wheel channel, additive rear handbrake demand and the initial braking-state
predicate. The model is deliberately a preparation step for the original
tyre/drivetrain rules, without inventing a relationship from brake demand
to car deceleration. `game/assets/vehicle/braking.ron` supplies the Vitz's
front, rear and handbrake capacities in **original integer demand units**.

`WheelBrakeInput` has four normalized effective brake channels and a
normalized handbrake channel. `WheelBrakeDemand` exposes the ordinary and
final arrays in front-left, front-right, rear-left, rear-right order.
`WheelBrakeDemand::is_braking` means nonzero final demand, matching initial
wheel-state bit 1. Optional `PedalBrakes` copies `PedalTravel::brake` to all
four channels initially; unported wheel assistance then has a separate
`BrakingSystems::Assistance` slot before demand allocation. Without that
marker a replay or AI rule can own the four effective channels directly.
The rules-only feature runs in
`VehicleSystems::Forces` and `GameSystems::Game`.

Demand quantization is mathematical floor, as in arithmetic product >>12.
The handbrake's narrower result is observable: a controlled original
experiment using full handbrake and coefficient 32768 produces **-32768**
rear handbrake demand. This follows `8003A204` left-shifting the low product
by 4, then `8003A208` shifting it arithmetically right by 16, retaining only
a signed 16-bit result. The native module preserves that outcome in a small
named `handbrake_demand` unit-boundary helper; it does not spread original
bit shifts through the rule. Ordinary Vitz demand never overflows its
full-width product for travel in 0..1.

The alias experiment was repeated as a fixture holdout after the native
rule was written. Once the natural CIRCLE input has reached the physics
consumer, the coefficients 32768, 65535, 65536, 32767 produce rear handbrake
demands -32768, -1, 0, 32767. The original's initial brake-state predicate
is nonzero, rather than positive, so the negative aliases still set it.
This probe changes a tuning coefficient to establish arithmetic behavior;
it is not a claim that the original Vitz normally uses those coefficients.

`tools/record_braking.py` records short original instruction traces of
`8003A19C..8003A278` and the initial pedal-copy region
`8003E2D0..8003E2F8`. The toolkit's scratchpad write watches do not report
these stores, so the recorder reads the trace's actual store effects at
`8003A1E4` and `8003A22C`, rather than a later scratchpad snapshot. Reads at
`8003A1B8/A1BC/A1E8/A1EC` establish each travel channel and coefficient.
The initial flags come from `8003A264`; wheel `+8` is recorded at
`8003A250` to distinguish its independent zero-load bit. The exporter
requires every observed input/output and the complete 30 Hz call sequence,
and rejects dropped trace lines.

Tests check the initial pedal-copy boundary on a separate entity, using the
actual copies at `8003E2E8`; the tested allocator receives effective wheel
channels observed at `8003A1B8` after assistance. That distinction emerged
from a failing original-trajectory check and is preserved in the fixtures.

`game/tests/braking.rs` compares **133 original calls** across six scenarios,
with zero tolerance for each wheel's ordinary demand, final demand and
initial braking predicate:

| Scenario | Calls | Coverage |
|---|---:|---|
| Service brake / handbrake / combined | 30 each | Natural input travel, fractional service-brake rise, additive rear demand |
| Independent mixed holdout | 30 | Release/reapply, handbrake-only, service-only and simultaneous controls |
| Independent wheel-input holdout | 8 | Asymmetric wheel channels; fractional service and handbrake travel; zero/nonzero demand |
| Independent signed-handbrake holdout | 5 | Controlled handbrake-capacity alias boundaries |

For the wheel-input holdout, `+0x45D` is set to 1 to take the established
`8003E270` branch that skips live-control copying, then the effective input
channels are supplied with explicit pokes at each tick. The consuming
instructions confirm the exact independent values reached the allocator.
This is a controlled subsystem-input experiment; it does not assign a name
to the original mode enum or claim to port that mode's control behavior.
The per-wheel vectors include `[1000,2000,3000,4000]`,
`[4096,0,1,4095]` and `[1,2,3,4]` in original travel/4096 units. Handbrake
fractions include `1/4096`, `1365/4096`, `2730/4096` and `4095/4096`.

All three holdouts were recorded after the native rule and tests were
written. Numeric fixtures and exact command/checkpoint metadata are in
`game/tests/data/original/braking/`, with full traces in
`.psx/runs/braking-*`. Reproduction:

```sh
python3 tools/record_braking.py --scenario brake --scenario handbrake --scenario combined
python3 tools/record_braking.py --scenario mixed_holdout --scenario wheel_holdout --scenario handbrake_alias_holdout
cargo test -p gran_turismo_2 --all-features --test braking
```

The initial flags' bit 0 agrees with `wheel+8 == 0` in these observations;
the native braking feature does not implement the unported load/contact
model. Its `is_braking` predicate covers only bit 1, before later drivetrain
phases add flags. ABS, other per-wheel control writers, reverse/locked-wheel
forces and actual braking deceleration still require their own original
rules and trajectory evidence.

The investigation below is superseded by the verified native implementation
and 130 original calls in [brake-assistance evidence](brake-assist-evidence.md).
The wheel-assistance source was bounded as follows: `8003DBE8..8003DE60`
constructs per-wheel targets from halfwords `+0x1FE/+0x2D6`, optionally
adjusts targets and one front-wheel channel using yaw/lateral correction
scratch `+0x90/+0x92`, then modifies each effective brake travel using wheel
halfwords `+0x2A/+0x44`, wheel word `+0x34`, and axle parameters
`+0x424/+0x428` and `+0x42C/+0x42E`. A nonzero channel is multiplied by a
clamped 0..4096 factor, then raised to 1 if the multiplication gives zero
(`8003DE40..4C`). This is a useful next investigation boundary; the physical
names of its intermediate gains and complete correction rules are not yet
established here. `psx disasm --function 8003DBE8` truncates this leaf
function before its late stack prologue, so use
`psx disasm --ram .psx/runs/b1-state1200 --addr 8003DBE8 --count 160 --text`
to inspect the full body.
