# Suspension travel, wheel load and body-force preparation

`game/src/vehicle/suspension.rs` ports `80043B54..80043F18`, including the
per-wheel spring/damper routine `800438F0`. All **221 original car calls
(884 wheels)** match exactly: 166 boundary/natural calls and a fresh 55-call
holdout recorded after implementation. Natural sequences retain native travel,
rate and load; suspension load feeds the next tick's native grip preparation.

Road height, surface, orientation, body height and the intervening collision
state remain observed inputs. Vertical integration and collision correction
are not part of this stage. These are connected subsystem comparisons, not
a closed B-1 driving trajectory. The visible car still has its checkpoint pose.

The following [road-contact stage](road-contact-evidence.md) now derives those
road inputs natively from decoded polygons and retains the original basis
ordering. Its 257 comparisons include this native suspension stage; corrected
body motion/angles and force inputs remain explicit.

## Identity and reproduction

USA Simulation Mode Rev 2, Vitz F '99, B-1 at vblank 7500.
State SHA-256 `49fb814844da1b4d160f966c4cb675405fb58eb3d74ebc90702ad58978a2ef5b`;
RAM SHA-256 `ff147faec522bdd3da3f6231a379c4b09e323e6d094186bcb0a71a5a5441ab04`.

```sh
psx disasm --ram .psx/runs/b1-state1200 --function 800438F0 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 80043AE0 --count 270 --text
psx disasm --ram .psx/runs/b1-state1200 --function 80081288 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8004232C --count 28 --text
python3 tools/record_suspension.py
python3 tools/record_suspension.py --holdout
cargo test -p gran_turismo_2 --all-features --test suspension
```

`tools/record_suspension.py` requires complete execution/watches, zero dropped
trace lines, one car, both early grip boundaries and both suspension boundaries.
It records the original scratch travel and body forces alongside persistent
wheel fields and actual consumed tuning. Traces/RAM/reports remain in
`.psx/runs/suspension-*`; CSV/JSON, commands, checkpoint hashes and toolkit
identity remain in ignored `game/tests/data/original/suspension/`.

## Source boundaries and public data

Offsets use physics base `800A96B4`; wheels start at +460, stride 68 hex.
Front/rear suspension descriptors start at +12C/+160, stride 34 hex.

| Source | Native quantity |
|---|---|
| Physics+664; wheel+0/+28/+42 | Body height, road height, attachment height and road offset, /4096 metres |
| Physics+6A0/+67C | Up components used for travel projection, /4096; producers remain external |
| Descriptor+8 minus physics+3E4/+3E6 | Travel offset minus wheel radius, /4096 metres |
| Descriptor+0/+2/+4 | Bump start, minimum and maximum travel, signed halfwords /4096 metres |
| Descriptor+C/+2C/+28 | Linear spring, progressive bump and anti-roll coefficients, force per metre or metre squared |
| Descriptor+10/+14/+18 | Rebound threshold (/4096 m/s), base force and square-root response |
| Descriptor+1C/+20/+24 | Compression threshold, base force and square-root response |
| Descriptor+32 | Signed preload force, added to tyre load |
| Physics+704 | Reciprocal step duration, /4096 per second |
| Wheel+10/+12 | Retained travel and travel velocity, signed halfwords /4096 |
| Wheel+48/+4C/+8 | Spring force, damper force and nonnegative tyre load |
| Physics+719 | Bits indicating raw travel strictly below each minimum |
| Physics+71C/+720, +728/+72C/+734, +1E/+20 | Axle/body forces and metre levers used for body-force transfer |
| Scratch+18..24 | Body-facing spring forces after anti-roll and transfer |

Force units are original internal units; their SI interpretation remains
unestablished. `SuspensionTuning` and per-car `SuspensionSetup` contain axle
descriptors and levers. The exporter creates `vehicle/suspension.ron` locally,
with the radius subtraction performed once. No tuning table is published.
`SuspensionInput` supplies road/body observations and forces; `Suspension`
owns travel/rate histories, flags, loads and intermediate body forces.

## Rules and rounding

First sample the ratio of the two up components. Project body height plus
attachment height minus road height/offset, then add the axle travel offset.
The compressed mask tests this raw travel against the minimum with a strict
inequality. It does not test the later clamped value or the final tyre load.

Travel **above** the maximum sets retained travel to the maximum, spring force
to negative preload, and damper/load to zero. It leaves retained velocity
alone. At or below the maximum, clamp travel against the minimum for spring
response, but use **raw** travel for the new velocity. The linear spring
uses the signed low-word product with operand-sign truncation (`8007596C`).
Below bump start, narrow the signed travel gap before squaring, sample the
square and then apply the progressive coefficient with wide flooring.

Derive velocity from raw travel minus previous travel and the explicit
reciprocal duration. Average its full result with the previous signed-halfword
velocity, truncate the signed mean, then clamp that mean to ±1 m/s. The new
rate's halfword store does not narrow the value used in this average.

Choose rebound for a negative mean and compression for a nonnegative mean.
Subtract that branch's threshold from speed, apply the original square-root
sampling, multiply by its response with wide truncation, and add base force.
Compression negates the result. The square-root helper interprets a negative
radicand as an unsigned word, rather than clamping it to zero; authored
threshold probes exercise this case. Add spring, damper and preload, then
clamp only tyre load to zero. Force outputs use f64 so large progressive
responses retain exact integer results beyond f32's consecutive-integer range.

Finally derive signed anti-roll force from squared left/right travel
difference. Subtract half of the axle force with signed truncation and apply
front/rear and left/right lever transfers separately. These adjustments alter
the body-facing spring force, **not** the tyre load. Dampers remain a separate
channel for the later body integrator. The native stage exposes both.

`80043B44/43CC4/43D6C` skip cars in mode 7. The caller at `8003E924` skips
the entire stage during the global countdown. `SuspensionInput::disabled`
represents those gates and preserves state. The skip mapping has binary
evidence and an authored test; the corpus covers enabled execution.

## Verification and remaining boundaries

| Suite | Calls | Coverage |
|---|---:|---|
| Velocity history | 13 | Both mean-rate clamps and exact adjacent values, signed averaging and retained velocity |
| Travel limits | 12 | Minimum/maximum equality and neighbors on both axle travel values |
| Progressive spring | 7 | Bump equality/neighbors and signed gap narrowing around the halfword boundary |
| Height/history/preload | 14 | Lower-stop and extension paths, asymmetric histories, signed preload and zero tyre load |
| Natural gas/brake/turn | 120 | Forty consecutive calls each; nontrivial projection, anti-roll and signed body forces |
| Fresh holdout | 55 | Fifteen new asymmetric history/height/threshold probes and forty alternating-control calls |

Tests compare raw travel, retained travel/rate, compression flags, every
spring/damper/load result and every adjusted body force. In consecutive
sequences they carry native histories and loads, and compare the next early
grip capacity using the native grip rule. They still inject observed road,
orientation and body/force inputs at the suspension boundary. No closed-loop
motion or unobserved vertical-force producer is implied.

Controlled runs use the established mode-1 preprocessing boundary and verify
actual histories and tuning at consumption. The inverse-duration poke is
overwritten at `80042360`; the recorder changes its global producer
`801C8570` and verifies +704 instead. Real baseline histories and tuning-based
probe coordinates are read locally from the checkpoint, not copied into source.
The fresh holdout passed without changing suspension behavior. Nonzero +72C
transfer has binary evidence and an authored test, not original runtime coverage.

The handmade test runs only `SuspensionRulesPlugin`, with authored tuning,
per-car overrides and an optional grip-input bridge. No controls, renderer,
other dynamics plugins or private assets are needed. The source-only example
mod disables this unused feature. `VehicleSystems::Suspension` follows
integration and precedes late contact refresh, so its load bridge changes the
next grip tick. No engine/template edit is needed.

Workspace tests, native game/example-mod MCP render checks and the engine
sandbox movement/material checks pass. Screenshots were inspected; no live
suspension-animation parity is claimed. Clippy retains only the pre-existing
wheel-rotation test warning. Current-source and history publication audits
pass; all generated assets and reference recordings remain ignored.

Next: decode the road-height/surface and orientation producers, then vertical
body integration and collision correction. A closed B-1 trajectory and live
wheel/body presentation still require those stages. Condition accumulation,
alternate torque controllers, induction, input-request production, audio,
licence flow, the wider game and the authoring editor remain unfinished.

The new watch corpus identifies the next producers directly: `80043634`
writes wheel road height, `80043640` writes its surface, `80043764/4376C`
write its road offset, and `80043204` writes attachment height. Both up
components are written by `8004506C` through the orientation helper. These
writer observations locate the next investigation; their complete rules are
not established here.
