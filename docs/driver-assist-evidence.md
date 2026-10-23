# Stability signals and traction throttle

`vehicle::driver_assist` ports the body-motion signals at `8003E4FC..8003E688`
and ordinary traction reduction at `8003DFDC` / `8003DE68`. All **311 original
car executions** match: 197 boundary/natural calls and 114 independently
recorded holdout calls. Native signal outputs feed per-wheel brake assistance,
brake allocation and wheel-torque input; traction output feeds effective
engine throttle. This closes these control producers, not the full vehicle
feedback loop or a playable driving session.

## Identity and reproduction

USA Simulation Mode Rev 2, Vitz F '99, B-1, absolute vblank 7500.
State SHA-256 `49fb814844da1b4d160f966c4cb675405fb58eb3d74ebc90702ad58978a2ef5b`;
RAM SHA-256 `ff147faec522bdd3da3f6231a379c4b09e323e6d094186bcb0a71a5a5441ab04`.

```sh
psx disasm --ram .psx/runs/b1-state1200 --addr 8003E4FC --count 105 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8003E680 --count 76 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8003DE68 --count 93 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8003DFDC --count 17 --text
python3 tools/record_driver_assist.py
python3 tools/record_driver_assist.py --holdout
cargo test -p gran_turismo_2 --all-features --test driver_assist
```

The recorder watches the complete car block and tracks traced scratch writes.
It requires complete runs/watches, no dropped trace lines, one car, both signal
clears on every physics call, and all producer/allocator boundaries. Every
fixture value comes from original execution, with uncalled traction/brake
boundaries explicitly marked. Controlled cases poke inputs before execution;
tests compare the values actually consumed after the original's intervening
processing. Commands, state/RAM hashes and toolkit versions accompany CSV/JSON
under ignored `game/tests/data/original/driver_assist/`; traces/reports stay
in `.psx/`. The updated holdout was recorded after the native implementation.

| Recording | Calls | Coverage |
|---|---:|---|
| Boundaries | 197 | Signed/zero speed, steering and yaw, low-word overflow, seven layouts, five engagement modes, fade/feedback/suppression boundaries, reverse, signed gains, unloaded wheels, mixed pedals and handbrake |
| Holdout | 114 | Independent motion/steering/yaw values, all seven layouts, mixed natural sequences, severe driven-wheel slip |

The boundary recording has 102 traction and 142 brake calls; the holdout has
56 traction and 96 brake calls. Original throttle stores include six complete
and 42 partial reductions in the boundary set, seven complete and 27 partial
reductions in the holdout. The initial holdout lacked complete cuts; seven
fresh severe-slip probes extended its coverage rather than relaxing the test.

## Signals and units

Physics base is `800A96B4`; wheels start at +460, stride 68 hex.

| Original field | Native meaning |
|---|---|
| +6A4/+6A8, words /4096 | Retained forward/lateral motion, metres/s |
| +64C, word /524288 | Yaw rate, turns/s |
| +60C, signed halfword /4096 | Steering angle, turns |
| +10, signed halfword /4096 | Wheelbase, metres |
| Scratch +90/+92, signed halfwords /4096 | Yaw shortfall and lateral correction |
| +64A, signed halfword /4096 | Lateral acceleration feedback, turns |
| +450, word /4096 | Traction strength |
| +454/+456, signed halfwords /4096 | Feedback fade and steering-response suppression gain |
| +700, signed halfword /4096 | Applied steering / signed extended limit, clamped to [0,1] |
| +1FC/+2D4, signed halfwords /4096 | Front/rear traction slip targets |
| Wheel +08/+44 | Nonzero load and signed longitudinal slip (/4096) |
| +458/+45A, signed halfwords /4096 | Ordinary added-braking gain/threshold |
| +708, signed halfword /4096 | Effective throttle |

Both signals reset each call. Positive forward speed permits signal generation.
For aligned steering/yaw, derive desired yaw from forward speed, the original
sampled steering sine and wheelbase circumference. If actual yaw is below the
nonzero desired rate, store one minus their truncated ratio. Alignment uses
the sign of the signed low-word product, not merely the signs of the operands.
The desired rate retains the original narrow/shift boundary; the signal store
narrows to a signed halfword. Those distinctions affect overflow probes.

The shared direction curve measures the angle of forward and negative lateral
motion. When yaw opposes that angle or is zero, lateral correction is the
absolute angle multiplied by forward speed/4, floored on the response grid
and upper-clamped at one. The original direction sampling is shared with slip
and road orientation in `vehicle::direction`; no new approximation is used.

## Traction rule

Ordinary control calls traction only when feedback fade is nonzero. The
routine itself additionally requires coupled engine engagement, positive
available torque and nonzero effective throttle. Feedback within a quarter
turn keeps full target response; beyond it, subtract the quantized fade
product and lower-clamp at zero. Negative authored fade can exceed one.

Inspect loaded wheels on the driven axles: rear for layouts 0/5, front for 1,
both otherwise. Retain the smallest slip strictly below one and that wheel's
axle target. Equal slip retains the earlier target. Reverse has no speed
suppression; other gears use steering-extension response times suppression gain.

```text
strength = traction_strength * (1 - steering_response * suppression_gain)
scale = clamp(1 + truncate((lowest_slip - target * feedback) * strength), 0, 1)
effective_throttle = scale * effective_throttle
```

Products retain the signed low word and floor at their own /4096 boundaries,
except the final error/strength product, whose original helper truncates toward
zero. The final throttle store narrows to a signed halfword. Boundary tests
compare exact original signals, consumed throttle, all assisted brake channels
and all four allocated demands in the same native tick.

## Steering-extension response producer

`+700` is not a speed-dependent gain. It is written twice by the control
routine. `8002FC30` clears it outside the slip-extension branch. Inside that
branch, `8002FC70` / `8002FCAC` first store the signed extended steering limit,
positive for the left side and negative for the right side. After the spring
has advanced the steering command, `8002FF30..8002FF78` divides the current
command by that signed limit, truncates on the /4096 grid, narrows the result
to a signed halfword, then clamps it to [0,1]. This final dimensionless fraction
is what `8003E71C..8003E730` multiplies by the suppression coefficient. The
legacy fixture column `speed_response` retains its name to keep the recording
format recognizable; code now names the consumed quantity explicitly.

`SteeringTuning::traction_extension_turns` exposes the intermediate limit;
`traction_response` performs final normalization. `DriverAssistInput` calls it
`steering_extension_response`, and the tuning coefficient is now
`steering_response_suppression`. Existing exported files using the former
`speed_suppression` tuning key still load through a serde alias. The exporter
writes the corrected key.

The expanded recording includes both signed limits, zero extension, the strict
forward-speed gate, opposite steering (lower clamp), above-limit steering
(upper clamp), and a ratio that wraps at the halfword store before clamping.
There are 142 ordinary control calls in the boundary recording and 96 in the
holdout. In the same headless tick, tests now run native wheel direction from
observed local contact motion, digital steering from the observed logical
request and starting spring state, extension normalization, stability signals,
traction, brake assistance and allocation. Intermediate signed limits, final
response, steering angle/rate, wheel directions and downstream outputs all
compare exactly. The final steering and response are no longer injected
observations on these calls. Source-1 probes still bypass controls as the
original does.

```sh
psx disasm --ram .psx/runs/b1-state1200 --addr 8002FB18 --count 105 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8002FF30 --count 20 --text
```

## Bevy contract and remaining work

`DriverAssistRulesPlugin` is public and independently selectable in `GamePlugins`.
It needs `DriverAssistInput` and `DriverAssist` on a car and a tuning resource
(layered `vehicle/driver_assist.ron` by default). `DriverAssistSetup` overrides
that resource per car. It requires no model, animation, rendering or other
feature plugin; there are no joint/clip/state metadata requirements.

Optional body, contact, velocity, steering, force and longitudinal-slip
components supply their corresponding inputs. When steering tuning and
kinematics are present, the plugin computes the extension response from the
current steering command instead of consuming an observed response. Optional drivetrain setup/state
supplies layout and engagement; available engine tuning supplies current curve
torque. Output bridges update effective engine throttle, brake correction inputs
and wheel yaw input. `PedalThrottle` explicitly opts into copying fresh pedal
travel before assistance; replay/AI owners omit it and supply their own request.
Ordinary processing runs after pedal copying and before brake assistance,
allocation, transmission preparation and engine torque. Explicit engine and
allocation ordering remains valid when intermediate plugins are absent.

Handmade tests exercise the rules alone, per-car replacement, optional bridges,
and pedal -> assistance -> torque with no drivetrain or brake plugins.
A handmade extension test also checks both turn signs, reverse bypass,
opposite steering, and externally owned response without kinematics. A second
car proves that omitting the pedal marker preserves external request ownership.
The tuning exporter was rerun from the documented checkpoint with the corrected
RON key, and the full comparison suite loaded that rebuilt tuning.

The special source-2/profile caller and its timer gate at +786/+78E are not
implemented. Source-1 probes verify only shared signal production and do not
claim those skipped traction/brake paths. Physical/analog requests,
condition/proximity producers, collision and a closed B-1
trajectory remain separate work. Body/load/slip inputs are observed boundaries
in the original comparison tests, even where optional native bridges exist.

## Validation

`cargo test --workspace --all-features` passed, including all private original
comparisons and doctests. Both game/mod offscreen MCP checks and the engine
sandbox movement/ramp check passed; inspected game/mod frames (the engine sandbox uses unchanged engine code).
`cargo clippy -p gran_turismo_2 -p example-mod --all-targets --all-features --offline`
completed with the existing `wheel_rotation` test's `collapsible_if` suggestion.
Formatting for the changed Rust crates, `git diff --check`, and working-tree
and public-history audits passed. The template's previously documented upstream
formatting differences remain unchanged.
