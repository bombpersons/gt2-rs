# Driving camera and projection

The native scene now uses the recorded B-1 bumper pose and projection.
Ordinary driving-camera rules support bumper, near/far chase, cycling on R1
and rear look on L1. Four original recordings test 192 camera updates,
including a held-out sequence recorded after implementation. Intro, replay,
spectator and full split-screen presentation remain unfinished. The scene's
body subject now follows the native controller through the
[live presentation rules](vehicle-presentation-evidence.md); the isolated
camera tests described here supply observed body motion.

## Evidence and cadence

Source: the supplied USA Simulation Mode Rev 2 disc, race code in
`.psx/runs/b1-state1200/ram.bin` and checkpoint `state.bin` (absolute frame
7500). Original per-camera state is at `0x801FF97C` in this call stack.
`0x800294D4` derives that pointer from the render context plus `0xC4`;
`0x800296D0..704` uses it for the main view. The earlier branch copies it
to stack storage and changes the viewport/pose for the rear-view mirror.
These are call-local addresses, not stable globals to reproduce natively.

At `0x800101FC`, the ordinary camera copies the
body pose from outer car `0x800A9688 + 0x81C = 0x800A9EA4` into camera
`+0xA4`: nine signed 12-bit matrix coefficients, padding, then three 16.16 m
translation words. This is the rendered body pose, rather than the vehicle's
20.12 m position triple. The distinction includes suspension/body motion.
`0x800103C0` builds the view from that pose. A 16-field write-watch recording
at `.psx/runs/camera-cycle-cadence` observes updates only at fields
`0,2,4,6,8,10,12,14`; poses are held on the intervening fields. All four
longer recordings corroborate that cadence. The native race now sets
`FIELDS_PER_TICK = 2` (30 Hz NTSC), while presentation remains 60 Hz.
Menus previously observed at 60 Hz will need their own per-field update when
ported; applying race cadence to menus would be wrong.

## Ordinary driving rules

| Rule | Original code/data |
|---|---|
| Advance view once on an effective R1 press, wrapping bumper → near → far → bumper | `0x80010314..33C`; effective pressed flags bit `0x100` |
| Rear view while effective L1 held | `0x800102FC..310`; held flags bit `0x200` |
| Bumper: translate 0.79998779296875 m along body-local up, keeping body rotation/roll | `0x80010424..440`, literal `0xCCCC` in 16.16 m |
| Bumper rear look: rotate 180° around local Y after translating | `0x80010444..46C`, call `0x8007B14C(2048)` |
| Chase: extract body yaw/pitch; subtract outer-car `+0x766` from yaw, add `+0x768` to source pitch; drop roll | `0x800101FC`, `0x800811B0`, `0x80010478..4F4` |
| Chase rear look: add half a turn to yaw, negate pitch | `0x80010498..4AC` |
| Translate along the rebuilt chase axes, height/back selected by view | `0x80010508..568`; tables `0x8002F350/358` |
| Split-screen variant adds 32/4096 turns to source pitch and uses alternate offsets | `0x800104B0..4C0`, tables `0x8002F360/368`; binary evidence only, no runtime split-screen recording yet |

Source pitch has the opposite sign to a Bevy local-X rotation. The chase
matrix is rebuilt as yaw then pitch; bumper keeps body roll. There is no
extra camera smoothing in this ordinary path. Native code follows those
rules with quaternions/radians/metres. It does not emulate lookup tables or
GTE instructions. The original's quantized angle/matrix differences are
bounded in the recorded comparisons below.

Normal offsets (near/far): height 1.79998779296875 m for both; back
5.399993896484375 / 6.79998779296875 m. Split-screen heights are
1.2599945068359375 / 1.6199951171875 m and back distances
6.4799957275390625 / 8.5 m. These values live in `game/assets/race/camera.ron`.

## Projection

`0x80010088` constructs a 320×240 viewport but a 320×264 virtual frustum.
The default B-1 setting from `0x8002F370`, selected by the option byte
`0x801C9992` (value 1 in this checkpoint), is H=216. The other table values
are 277 and 190; native options for selecting them remain unfinished.
`0x8007B320` computes frustum metadata and
`0x8007B374` builds the scaled view matrix:

- X scale `320*4096/320 = 4096`.
- Y scale `floor(240*4096/264) = 3723`, negated for screen coordinates.
- Centre `(160,120)`; effective focal lengths `(216,196.330078125)` pixels.

Using a stock symmetric 45° perspective and a nine-metre chase offset was
provisional and did not match the original. `engine::camera::LensData` now
expresses independent focal lengths and optical centre in reference pixels,
with ordinary Bevy projection and transforms. Resizing preserves vertical
framing; subviews and frustum corners use the same lens. Engine tests check
projected points, off-centre lenses, subviews and a wider target. The game's
numeric test checks the measured focal scaling independently of the camera
controller. Near/far clipping distances in the asset are current renderer
choices, not yet the original's clipping/visibility parity.

## Original recordings and comparison

Reproduce with `tools/record_camera.py --scenario <name>` for `coast`,
`views`, `turn`, `holdout`. Each runs 96 fields from the checkpoint. Exact
commands and checkpoint hash are in `game/tests/data/original/camera/*.json`.
The script records eight body-pose word loads at `0x8001023C..268`, the two
stabilizing angles at `0x80010484/494`, effective pad words at
`0x800102FC/300`, and stores into the camera pose. Reconstructing stores at
each call boundary avoids relying on a later reused stack snapshot. Input
serial-pad latency is outside this comparison: native tests replay the
effective controls consumed by the original camera.

| Recording | Updates | Maximum eye-position difference | Maximum matrix-coefficient difference |
|---|---:|---:|---:|
| Coasting bumper | 48 | 0.000008 m | 0.000011 |
| All views and rear look | 48 | 0.000600 m | 0.000244 |
| Turning with chase stabilization | 48 | 0.007208 m | 0.001318 |
| Independent view/turn/rear-look holdout | 48 | 0.004020 m | 0.000566 |

Every view transition matches exactly. Tests bound positions to 0.025 m and
matrix coefficients to 0.002. The holdout was recorded after the rule passed
the other scenarios and was not used to adjust it. Tests use the public
CameraRulesPlugin and ordinary subject Transforms, without the course,
rendering, HUD or vehicle force model.

## Native rendering and extension

The low bumper view exposed an existing affine-clipping defect: triangles
with vertices behind the camera had already multiplied UVs/colours by a
negative clip W before hardware clipping. They sampled transparent atlas
padding and exposed the ground across the near road. The optional engine
`GeometryClippingPlugin` now clips source triangles against the selected
camera's near plane before projecting, interpolates their source UVs/colours
at cut vertices, and rebuilds only when the view/object transform changes.
It retains the source mesh for future views. Tests check new corner positions,
UV interpolation, rejected geometry and source preservation. The MCP test
also checks that the near-road region does not show those ground holes.
This corrects the native mesh path; it does not establish original adaptive
subdivision, packet order, clipping thresholds or pixel parity. Large road
triangles and other unported primitive groups can still stretch. Flat-textured
road quads now use the source refinement rules, as recorded in
[subdivision evidence](subdivision-evidence.md).
Only one selected clipping camera is supported by this scene mesh set; mirror
or split-screen views need per-view mesh sets.

The native B-1 screenshot, chase and rear views were inspected alongside the
original checkpoint. Sky/background, fog, other primitive refinement, dynamic LOD,
procedural elements, HUD, mirror and playable car session remain incomplete.
The application now drives the subject and stabilization through the native
vehicle controller; those producer comparisons are documented separately in
[presentation evidence](vehicle-presentation-evidence.md).

The example mod supplies its own camera.ron, uses the same public camera
controller on its handmade course, cycles to its authored far view and follows
a moved subject through MCP. It needs no imported car model or physics.
The engine sandbox loads a hand-authored LensData file through engine alone.
Controller contract: any subject Entity with Transform and CameraSubject;
unit quaternion, metre translation and optional stabilization angles in
radians. PlayerCamera binds the logical pad; without it a mod/AI can supply
CameraInput directly. No required joints, animation clips, sounds or HUD.

Verification: `cargo test --workspace --all-features`, workspace Clippy and
formatting pass. Game and mod MCP checks render, step, query and exercise
view changes. Game near-road coverage also guards the clipping regression.
The engine sandbox builds/runs with its authorable lens and moves its Walker
1.5 m over 30 right-input ticks. No template files changed for this feature.
Logs: `/tmp/gt2-camera-{workspace-tests,clippy-final,gpu-final}.log`.
Screenshots from the final game run are under `/tmp/psx-template-headless-497/`
(`before.png`, `chase.png`, `rear.png`), the mod under
`/tmp/psx-example-mod-423/`, and the sandbox under
`/tmp/gt2-template-sandbox/`. The screenshots were inspected, not treated as
proof of pixel parity.
