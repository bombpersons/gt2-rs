# Live body, wheels and chase stabilization

The native B-1 scene now drives its rendered subject with the retained vehicle
controller. Its body origin follows corrected position and orientation;
wheels follow steering, axle camber, suspension travel and rolling spin.
The existing bumper/chase/rear-look camera follows that live subject with
the original's retained yaw/pitch stabilization. Rendering remains 60 Hz,
with the observed poses held between the 30 Hz physics updates.

This starts from the established **moving** Vitz/B-1 checkpoint, not the
standing start. Collision, licence conditions/HUD/outcomes, audio, shadows,
reflections and dynamic detail selection remain
unfinished. It is not yet an end-to-end licence session.

## Source and observation boundaries

USA Simulation Mode Rev 2, outer car `800A9688`, physics base outer+2C.
The checkpoint identity is the same as `vehicle-loop-evidence.md`.

| Rule | Original evidence |
|---|---|
| Convert physics position and retained body basis to the rendering coordinate system | `8001336C..3EC`, called from `80013468`; physics+65C position and +668/+670/+678 basis |
| Add a rotated model-origin offset along body-local Z | `800134BC..1351C`; outer+5A in /4096 metres, helper `800814B4` |
| Wheel X/Z remain authored attachments; Y is axle origin minus suspension travel | `8001356C..588`; outer+160 with axle stride 34 hex, wheel outer+49C with stride 68 hex |
| Wheel yaw is wheel heading minus a quarter-turn, with equal effective yaw on both sides | `80014578..5A8`, `800671AC..1CC`; opposite-side extra half-turn |
| Camber has opposite signs on the two sides; spin rotates about local Z | Same caller and `80081374`; native rotation `Ry(heading-quarter) * Rx(signed camber) * Rz(spin)` |
| Preserve the right-side axial reflection through reflected meshes/winding | `8006725C..2B4`; existing offline wheel construction |
| Chase yaw follows body heading, or world velocity above a speed gate in the alternate option | `8003E978..9F0`; option `801C9990`, shared direction helper and approximate speed norm |
| Yaw spring integrates the mean of previous/current acceleration before advancing angle | `8003E9F4..EA98`; retained physics+748/+74A/+752, shortest signed angle differences |
| Pitch spring integrates current spring/damping force before advancing angle | `8003EA9C..EB1C`; retained physics+74C/+74E |
| Camera corrections are current body angles minus those retained follower angles | Stores physics+73A/+73C, consumed as outer+766/+768 by the existing camera |

The body matrix's native columns are lateral, up and negative forward.
The original translation helper's subpixel output is replaced by the ordinary
rotated metre offset. Native rotations use normalized quaternions, rather
than a GTE or sine-table renderer. State grids are retained at spring
integration/storage boundaries because the follower carries those results
between ticks. No presentation system changes physical position or forces.

## Recordings and tests

```sh
python3 tools/record_vehicle_presentation.py --scenario gas --scenario coast --scenario brake
# Recorded after the first three scenarios passed:
python3 tools/record_vehicle_presentation.py --scenario holdout
python3 tools/record_vehicle_presentation.py --scenario holdout --velocity-tracking
python3 tools/record_vehicle_presentation.py --scenario holdout --camber-probe
cargo test -p gran_turismo_2 --all-features --test vehicle_presentation --test vehicle_loop
```

The recorder observes original stores at the stabilization entry/exit and
completed body/wheel placement boundaries. It also captures the local wheel
matrices actually consumed by `8007B994`, restricted to the call at
`8006724C`; scratch storage is reused elsewhere and a scratch-address match
alone would include unrelated matrices. Axle camber comes from the actual
render-argument reads. It requires complete runs, untruncated writes/reads
and traces, every stage every two fields, and four wheel matrices whenever
the car is drawn. The first bumper field precedes the effective view change.

There are **180 body/stabilization poses and 696 consumed wheel matrices**:
four natural 30-tick sequences, one alternate tracking-option sequence and
one asymmetric nonzero-camber probe. The latter two deliberately change
setup/option inputs with documented original-run pokes; they do not supply
expected poses. The fresh natural holdout combines left/right steering,
throttle, service brake, handbrake and release. Its results passed without
changes to the implemented rules.

`vehicle_presentation.rs` retains the chase follower across each sequence,
supplying observed physical pose/velocity at this subsystem boundary. Follower
state and correction angles match exactly on the original grids. Body origins
are within 1 mm; body/wheel rotation coefficients are within 0.002 of the
consumed quantized matrices. Wheel attachment positions match exactly on
the 1/4096 metre grid, including changed suspension travel and camber signs.

The separate complete-loop tests also run the new stabilization plugin from
the initial state alone. All five retained follower channels match exactly
at the end of every tick across all **120 existing physical ticks**, alongside
the existing force/pose/contact comparisons. No later physical or follower
state is injected into those tests. This distinguishes the closed physical
trajectory from the isolated render-boundary comparisons.

The alternate velocity option is observed on the moving sequence; its exact
low-speed gate is binary evidence, not a recorded threshold sweep. Other
cars/setups, extreme authored spring states, replay/intro/mirror/split-screen
presentation and complete draw-order/pixel parity remain unverified.

All numerical fixtures stay in ignored `game/tests/data/original/` and
emulator outputs in `.psx/`. Only the authored recorder and native code are
published.

## Native integration and authoring

`VehicleVisualMotionPlugin` joins the ordinary fixed schedule after contact
refresh and before the camera. It reads `VehicleVisualTuning` from layered
`vehicle/visual.ron` unless supplied by the caller. `VehicleVisualSetup` is a
per-car override. `ChaseStabilization` is public retained state; the initial
state exporter now includes it. When omitted in a handmade initial state,
the spawn hook initializes it to that car's heading/pitch. Velocity tracking
uses the shared direction curve in `SlipTuning`.

Body synchronization opts in through `VehicleBody` plus physical pose, road
state and `Transform`. A `CameraSubject`, if present, receives the fresh
corrections. The numerical feature needs no geometry or rendering resources.

An optional `WheelPlacement::motion` binds a layout entry to simulation wheel
0..3. It specifies the suspension-origin Y, axle camber in turns and an extra
mesh-local unit rotation. Its X/Z attachment coordinates remain authored.
The canonical wheel uses local Z as axle, with XY the rolling plane; the
extra rotation accommodates other authored meshes. Without a binding, the
layout retains its static translation/rotation. Bindings validate indices,
finite values and unit rotations before spawning geometry.

The wheel importer exports those bindings alongside the existing reference
poses; re-export assets after updating the source. No runtime matrix packets
or RAM layouts are introduced. `RaceVehicle::initial_state` selects a native
initial-state asset; `None` keeps a course-only subject. The
[example mod](../mods/example/README.md) now supplies handmade tuning and
an initial state to the unchanged feature group, with live octagonal wheels
and authored course collision. Its real executable runs against a fresh copy
of those assets without a base-game asset folder.

## Visual validation

The real game binary passes its offscreen MCP render/edit/step checks and
live camera/wheel checks. A second MCP check steps 30 throttle ticks and
compares the resulting body origin with the original recording within 1 mm.
Inspected `/tmp/gt2-live-reference-282/driving-30.png` alongside
`.psx/runs/vehicle-presentation-gas/screen.png`: body framing and attachment
agree; the absent HUD, environment reflection, shadow and original sky are
visible remaining work. This is visual QA, not a pixel-parity claim.
The chase and turning images under `/tmp/psx-template-headless-283/` were also
inspected. Generated images remain local and are not documentation exports.

`cargo test --workspace --all-features` passed with the private fixtures
present. The game and example-mod MCP checks passed; the engine sandbox
passed movement, ramp and texture-detail checks, and its image and the mod's
image were inspected. Workspace Clippy completed with the existing
`wheel_rotation` test's `collapsible_if` suggestion. Changed-crate formatting,
Python compilation and `git diff --check` passed. No template files changed.
