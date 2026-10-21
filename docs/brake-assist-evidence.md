# Per-wheel brake assistance

`game/src/vehicle/brake_assist.rs` ports the complete arithmetic and branches
of `8003DBE8..8003DE64`: adjust wheel slip targets for yaw shortfall, add
front-wheel correction braking, then attenuate each nonzero brake channel
using slip and wheel response. This runs after pedal copying and before the
existing demand allocator. It does not produce wheel/contact inputs or body
acceleration.

## Identity and recordings

Original: USA Simulation Mode Rev 2, Toyota Vitz F '99, B-1 checkpoint at
absolute vblank 7500. State `.psx/runs/b1-state1200/state.bin` SHA-256
`49fb814844da1b4d160f966c4cb675405fb58eb3d74ebc90702ad58978a2ef5b`;
RAM SHA-256 `ff147faec522bdd3da3f6231a379c4b09e323e6d094186bcb0a71a5a5441ab04`.

`tools/record_brake_assist.py` records original consuming loads and completed
stores, not predictions. It requires complete, untruncated instruction traces,
exactly one call every two fields, all four wheel observations, and agreement
between assisted output and the subsequent allocator's consumed input. An
explicit flag distinguishes a branch's unconsumed input from observed zero.
Commands, runtime version, hashes, and reports accompany every local fixture.
These generated reference fixtures remain private and are not distributed.

```sh
psx disasm --ram .psx/runs/b1-state1200 --addr 8003DBE8 --count 160 --text
python3 tools/record_brake_assist.py
# Recorded after the native implementation and initial trajectory checks:
python3 tools/record_brake_assist.py --holdout
cargo test -p gran_turismo_2 --all-features --test brake_assist
```

All **130 original calls** compare exactly, including four adjusted travel
outputs and all ordinary/final demand outputs in the same tick:

| Recording | Calls | Evidence |
|---|---:|---|
| Brake / combined | 30 / 12 | Natural service-brake ramp and additive handbrake |
| Left/right yaw | 12 each | Controlled small signed yaw, target adjustment on both sides, front addition and clamp |
| Upper targets | 6 | Controlled axle targets above full scale; conditional target clamp |
| Zero factor | 6 | Controlled zero grip gain; minimum nonzero brake command |
| Mixed holdout | 24 | Independent control changes, releases, throttle/brake and handbrake |
| Yaw holdout | 12 | Independent yaw, percentage, correction gain and threshold |
| Zero-strength holdout | 8 | Disabled target correction with positive lateral signal and enabled added braking |
| Feedback holdout | 8 | Independent fractional gains, negative/upper attenuation clamps |

Controlled probes alter parameters or velocities at run-frame boundaries.
The recording reads the actual values reaching the routine after intervening
original processing. Scratch correction signals are produced by original
upstream code; they are not patched into the routine. These experiments prove
this consumer boundary, not natural body/contact trajectories for every car.

## Fields and rule

Physics base is `800A96B4`; four wheels start at `+460`, stride `68` hex.
All response quantities below use /4096, converted to dimensionless native
fractions. The physical calibration of the grip response remains open.

| Source | Native meaning | Producer/consumer |
|---|---|---|
| Physics +1FE/+2D6 | Front/rear longitudinal slip target | Loaded at 8003DBF4/DC08 |
| Physics +424/+428 | Axle slip feedback gain | Loaded at 8003DDA4 |
| Physics +42C/+42E | Axle grip feedback gain | Loaded at 8003DDBC |
| Wheel +2A | Speed-dependent response | Writer 80030680; consumer 8003DDA8 |
| Wheel +34 | Load/camber/surface response | Writer 80039D98; consumer 8003DDC4 |
| Wheel +44 | Signed longitudinal slip | Writer 80039744; consumer 8003DDE4 |
| Scratch +90/+92 | Yaw shortfall / lateral correction signals | Writers 8003E5E4/E678 |
| Physics +64C | Yaw sign selecting a side | Consumers 8003DC7C/DD40 |
| Caller a1/a2 | Added-braking gain / threshold | Normal path physics +458/+45A |
| Global 80046EE8 | Target-correction strength, integer percent | Consumer 8003DBF8 |

When percentage and yaw shortfall are nonzero, subtract their product
(truncated after division by 100) from full response, lower-clamp at zero,
and multiply the selected side's two targets by it. Positive yaw selects
indices 1/3; zero or negative selects 0/2. Only this branch upper-clamps all
four targets at full scale. Omitting that conditional changes controlled
above-range targets.

When added-braking gain is nonzero and lateral response exceeds threshold,
add eight times their quantized product to the selected front wheel's
channel and upper-clamp at full travel. This selects index 1 for positive
yaw, otherwise 0. The source does not lower-clamp this addition.

For each nonzero channel:

```text
slip_gain = speed_response * axle_slip_feedback
grip_gain = grip_response * axle_grip_feedback
slip_error = (longitudinal_slip - adjusted_target) * slip_gain
factor = clamp(grip_gain * (1 - slip_error), 0, 1)
travel = factor * travel
if travel == 0: travel = 1/4096
```

Each product floors at its own /4096 boundary. Intermediate products retain
the signed low word; `response_product` isolates that conversion. Negative
products floor, not truncate. The eightfold correction product uses the
source's separate shift-by-nine boundary. Zero input skips attenuation and
stays zero. A nonzero command yielding zero becomes one raw travel unit;
replacing that rule with an unconditional zero clamp loses observed outputs.
Handbrake is independent and reaches the additive rear allocator unchanged.

## Bevy boundary and remaining work

`BrakeAssistRulesPlugin` is independently selectable in `GamePlugins` and
runs in `BrakingSystems::Assistance` at 30 Hz. `BrakeAssistInput` opts a car
in; other cars retain their effective per-wheel inputs. The layered
`vehicle/brake_assist.ron` contains the original axle parameters and strength.
Tests prove standalone use and copy -> assistance -> allocation order even
when plugins are registered in the opposite order. A separate car in the
same tests proves independent AI/replay channels remain unchanged.

[Longitudinal-slip generation](longitudinal-slip-evidence.md) is now ported
and connected. Wheel response generation, the upstream yaw-signal rules,
other control writers, coupled tyre/drivetrain/body forces and actual braking
deceleration remain unfinished. These rules alone cannot drive the live car.
