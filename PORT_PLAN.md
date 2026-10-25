# Finished-port work plan

User direction: keep going until the Gran Turismo 2 native Bevy port is
finished. The persistent goal is active. Do not equate this checklist's next
milestone with a finished game, and do not replace original rules with fitted
approximations. The disc and PSX runtime/binary evidence are the sources.

## Source-only publication

The public history excludes all disc-derived assets, numerical fixture files,
raw overlays and emulator data. `tools/export_assets.py` reproduces the
current B-1 runtime content locally from the user-supplied disc and BIOS.
See `docs/assets.md`; run the public history audit before every push.
The three named native README screenshots are a documentation exception
requested by the user; see `docs/screenshots.md`. Runtime assets stay private.

## Current baseline

- Native Bevy workspace, engine/game boundaries, extensible plugin group,
  example mod and engine sandbox exist.
- Volume, gzip overlays, TIM images, text and engine samples import offline.
- Course road table and draw spans import to RON. B-1 renders decoded road-node
  polygons and 13 placed scenery meshes with 32 imported texture materials.
  Flat-textured road quads now use recorded adaptive subdivision. Ground and
  sky are provisional; model LOD, other primitive refinement and 106
  procedural primitives remain open.
- Section-2 arrays and references validate across all 126 courses (5,709
  meshes); 6,084 placements decode from their 33-list tables. Rotation and
  nonuniform scale match original matrices exactly, including a held-out case.
  See docs/course-mesh-evidence.md.
- Road-quad refinement matches 330 original splits, all rounded positions/UVs,
  and ordered UVs/counts for 583 leaf quads and 479 stitch triangles over three
  camera recordings, including a final independent far-view holdout. Projected
  corners agree within one pixel. The importer preserves 2,907 B-1 road quads;
  the engine sandbox exercises the same renderer with a handmade panel.
  See docs/subdivision-evidence.md.
- Road material detail now selects on full parent projected area before
  refinement. All 255 original selections and recorded parent pixels match,
  including 80 final holdout records. The importer preserves both variants
  for 3,295 B-1 road parents. The sandbox demonstrates the same rule with
  handmade blue/orange textures. See docs/material-variant-evidence.md.
- Ordinary bumper/chase/rear-look camera rules and the B-1 lens are ported,
  with 192 original pose comparisons including a held-out sequence. Race
  logic now runs at the observed 30 Hz with 60 Hz presentation. The native
  subject now follows the retained vehicle simulation and camera stabilization. Intro,
  replay, mirror and split-screen presentation remain unfinished.
  See docs/camera-evidence.md.
- Gameplay physics, vehicles, full courses, races, menus, progression, save
  games, music and editor are not finished.
- Digital steering limits, spring response and ordinary wheel steering now
  match 300 recorded ticks and 39 independent boundary probes exactly.
  Velocity-to-step construction and integration match all 300 ticks, including
  a new mixed-control holdout. Forces are still measured inputs in those tests.
- Engine torque curve/drag and hysteretic fuel cut match 168 original calls,
  including independent curve, fractional-throttle and limiter holdouts.
  Ordinary torque-to-axle coupling and transmission feedback are ported below;
  body/contact coupling remains open. RPM storage is corrected:
  +0x6AC/+0x6AE are separate halfwords; +0x624 is engine rev/s ×4096.
- The decoded Vitz high-LOD body renders at the reference pose in chase
  views, using checkpoint paint slot 5. The example mod supplies a handmade body
  through the same engine geometry format. Procedural wheel meshes match all
  six original front/rear LOD streams byte-for-byte and render with observed
  default rims and live attachment poses. The mod supplies its
  own wheels and layout. Live steering/camber/spin/suspension is connected;
  dynamic LOD, wheel
  variants, shadows and reflections are still required.
- Wheel slip preprocessing matches 100 original boundary/holdout executions
  and feeds steering correctly in all 300 driving ticks. Brake allocation
  matches 133 original calls, including asymmetric inputs and signed rear
  handbrake edge cases. The intervening per-wheel brake assistance now matches 130 original calls,
  including independent holdouts and complete pedal-to-demand integration.
  Longitudinal wheel slip now matches 268 original wheel executions, with
  all 4096 steering bases verified, and supplies assistance in 36 natural
  physics ticks exactly. Contact motion, speed response and upstream yaw
  signals remain measured inputs; wheel integration is ported separately
  below. Coupled braking deceleration remains unfinished.
  See docs/longitudinal-slip-evidence.md.
- Paired tyre responses and full per-wheel longitudinal/lateral force
  preparation now match 972 original wheel executions, including low-speed
  correction and independent holdouts. Both slip stages feed forces natively;
  grip capacity and contact/rotation/body inputs remain measured. Body and
  drivetrain integration remain unfinished. See docs/tyre-force-evidence.md.
- Surface resistance and the complete forward phase before drivetrain
  coupling now match 183 original car executions. Native slip and tyre forces
  feed the phase; all contact masks, surfaces and signed stopping branches
  are covered. Load/grip, aerodynamic preparation and final body/drivetrain
  coupling remain unfinished. See docs/forward-dynamics-evidence.md.
- Wheel reaction torque, yaw-biased preparation and all eight original
  differential modes now match 230 original car executions (920 wheels),
  including consecutive clutch history, an independent mixed-axle
  holdout and exact consumed yaw-gate boundaries. Both slip stages, tyre force, brake allocation and forward
  dynamics feed this phase natively. Those standalone tests retain measured
  axle-drive inputs; the intervening solver is now ported below.
  Contact/load/grip, auxiliary/yaw inputs, body coupling and live wheel
  animation remain open. See docs/wheel-rotation-evidence.md.
- Engine engagement, gear multiplication and all seven centre/axle layouts
  now match 338 original car executions (1,352 wheels), including native
  ordinary engine torque, clutch-speed feedback, persistent centre/axle/wheel
  history and independent angle/brake/stopping holdouts. Separate contact
  resets at 80030830 remain observed inputs. Alternate torque controllers and
  induction remain external. Ordinary post-coupling engine feedback,
  engagement preparation/transitions and gear timing are now ported below.
  See docs/drivetrain-evidence.md.
- Ordinary engine feedback, engagement request/transition rules, RPM refresh,
  automatic/manual selection and first-gear/repeat/reengagement timing match
  723 original car calls (2,892 wheels). The tests retain engine, gear,
  engagement, request, timers and fuel-cut state across consecutive ticks,
  including a final independent held-out sequence. All seven layouts permit
  and block shifts at their actual wheel-slip/contact boundaries. The rules
  run alone with handmade data and support per-car transmission setups.
  Logical request production/physical bindings and alternate torque paths
  remain separate. Lateral/yaw/world acceleration is now ported below;
  contact/load/grip production and collision remain next.
  See docs/transmission-evidence.md.
- Lateral force, lever-arm yaw torque, signed yaw clamps, lateral feedback,
  reset/bypass/brake-stop branches and horizontal world velocity now match
  513 original car calls (2,052 wheels), including a fresh holdout and a
  combined transmission/body sequence. Native lateral feedback carries into
  the next drivetrain call. Per-car setups and a rules-only handmade test
  prove the extension boundary. Body/contact velocities are still observed
  inputs at this boundary. Next: contact/orientation/load/grip production
  and collision, then a closed B-1 trajectory and live driving.
  See docs/body-dynamics-evidence.md.
- Load/camber/surface grip (including optional condition scaling), corrected
  world-to-contact motion, speed response, free-wheel/axle synchronization and
  signed wheel spin now match 380 original car calls (1,520 wheels), including
  a fresh holdout. Contact motion refreshes after movement/collision and
  carries into the next tick's slip/forces. Native grip feeds tyre forces and
  optional assistance/forward inputs. Speed-derived +6AE/+6F8 halfwords are
  corrected from earlier RPM labels. Next: road/suspension load, surface and
  orientation producers (the observed wheel-load writer is 80043A74), collision
  and the condition accumulator, then a
  closed B-1 trajectory and live presentation. See docs/contact-input-evidence.md.
- Suspension travel, linear/progressive spring and damper forces, compression
  flags, anti-roll and body-force transfer now match 221 original calls (884
  wheels), including a fresh holdout. Natural sequences retain native travel,
  rate and load and feed next tick's native grip. Tuning exports locally and
  supports per-car overrides; an authored test runs the plugin alone. Road
  sampling/surface/orientation, vertical body integration and collision remain
  observed boundaries. Next: decode those producers/consumers, then close
  the B-1 trajectory and animate the live body/wheels.
  See docs/suspension-evidence.md.

- Decoded course collision now samples ordered node grids/polygons natively.
  All 126 courses validate: 14,387 grids, 268,488 referenced polygons and
  660,086 cell references. Road height history, surfaces/variants, procedural
  bumps, attachments and body/road bases feed native suspension and match
  257 original calls (1,028 wheel queries), including a fresh holdout.
  Actual selected polygon identity and neighbor fallback are checked. Native
  histories and bases carry over natural sequences. Native vertical body
  correction now replaces the observed corrected-pose boundary in these tests.
  Suspension body-force inputs remain observed. The engine sandbox authors
  and walks a ramp in this collision format. Next: remaining force producers
  and the closed B-1 session.
  See docs/road-contact-evidence.md.


- Pitch/roll integration, vertical spring/damper impulses, gravity, airborne
  feedback, reset height/heading and suspension-stop correction now match
  221 original body calls, including a fresh 60-call holdout. All 257 road
  comparisons now connect decoded polygons -> suspension -> native body
  correction -> late attachment/frame refresh without injected corrected
  pose/angles. Tilt rates are correctly interpreted as /65536 turns/s.
  The plugin runs alone with authored/per-car tuning and optional bridges.
  Movement-entry velocity/position, suspension force channels and reset event
  selection remain observed. Next: aerodynamic/body-force and condition
  producers, then a closed B-1 trajectory and live vehicle presentation.
  See docs/vertical-body-evidence.md.


- Speed-squared aerodynamic drag and front/rear loads, retained reduction
  growth/recovery and force storage now match 266 original calls, including
  a fresh 74-call holdout. Native forward/suspension/body rules consume those
  outputs. All 257 decoded-road comparisons now also use native aerodynamic
  axle loads and carry reduction history, replacing that observed boundary.
  Standalone and per-car/optional-bridge tests use handmade tuning. The
  upstream proximity growth producer and other force/control boundaries remain
  explicit. Next: close the full B-1 vehicle feedback loop, then live driving
  and presentation, condition and licence flow.
  See docs/aerodynamics-evidence.md.

- Body-motion yaw/lateral stability signals and ordinary traction throttle
  now match 311 original car executions, including 114 fresh holdout calls.
  Native signals feed per-wheel assistance, brake allocation and wheel yaw
  inputs. Handmade tests prove independent/per-car rules and optional
  pedal-to-engine integration without drivetrain or brake plugins. Special
  profile/timer paths remain separate. Native wheel direction and digital
  steering now also produce the normalized steering-extension response used
  by traction, including both limit signs, normalization clamps and narrowing.
  Next: close remaining control/force producers and the full B-1 feedback loop,
  collision, live vehicle presentation and licence flow.
  See docs/driver-assist-evidence.md.

- The composed moving B-1 loop now matches 120 original ticks across four
  30-tick trajectories: gas, coast, braking and a fresh turning/mixed-pedal
  holdout. State is initialized once per recording and retained thereafter;
  later velocity/force/contact/steering/engine/pose values are assertions only.
  Current body/forward force transfer, ground-reference wheel feedback and
  late drive/brake synchronization now connect the existing features. All
  sampled physical state matches at four stage boundaries with zero tolerance.
  The 972 tyre comparisons also check both speed-response force channels.
  The authorable `VehicleInitialState` and public spawn hook now preserve
  these trajectories through RON round trips; the independently exported
  checkpoint asset also reproduces all four sequences. The handmade driving
  mod now uses the same public controller. Next:
  standing-start B-1 session, collision, licence flow and broad content parity.
  See docs/vehicle-loop-evidence.md.
- Live body/wheel/camera presentation now runs from that native controller.
  Body origins, wheel attachment/steering/camber/spin and retained chase
  stabilization match 180 original poses and 696 consumed wheel matrices,
  including fresh turning, alternate velocity tracking and asymmetric camber
  probes. The complete-loop tests also check all five retained follower
  channels exactly. The real binary's 30-tick throttle body origin matches
  the original; live chase/turning frames were inspected.
  See docs/vehicle-presentation-evidence.md.
- The example mod now drives its handmade box car using unchanged
  `GamePlugins` and 22 independently authored numerical assets, with its own
  course collision, initial state and live octagonal wheels. Headless tests
  verify throttle/coast/braking/handbrake, both steering directions and
  retained road loads/contact. The real binary also passes those driving,
  wheel and camera checks with its assets copied to a fresh tree containing
  no base-game assets. No game or engine code changes were needed. This is
  an extension/authoring proof, not additional original-game parity.
  See mods/example/README.md.
- Ordinary digital reverse/shift and engagement-request production now feeds
  the retained loop. All 440 original ticks match at four boundaries, with
  320 new idle-start ticks covering throttle, brake/release, reverse/forward,
  manual shifts and a fresh mixed-control holdout. Fourteen original probes
  verify engagement gates, including idle/full-pedal/countdown boundaries.
  Disconnected engines now retain prepared curve/drag values while combustion
  stays at the late feedback boundary. The real binary's idle reverse/forward
  replay matches final position exactly. The handmade mod can stop, re-engage
  and reverse. See docs/driver-request-evidence.md.
- Optional B-1 session rules now drive countdown gates and the post-physics
  clock. Three natural countdown recordings add 380 ticks, bringing retained
  full-loop coverage to 820 unique original ticks with exact countdown/clock
  comparisons. The real binary's configured countdown launch matches final
  position exactly; the handmade mod can join a timed start. Native scene
  setup is available through layered race/vehicle.ron. See docs/session-start-evidence.md.
- Normal export now captures and selects the actual countdown start, while
  retaining the moving checkpoint for tuning/regression tests. Full power-on
  export reproduced both reference RAM dumps byte for byte. Generated startup
  content matches all three countdown trajectories; the default real binary
  matches the launch position exactly without a scene override. Both reuse
  paths select their intended startup mode.
- Countdown/start and lap-clock presentation now uses decoded native bitmap
  fonts and ordinary sprites. Nineteen original GPU frames match in glyphs,
  placement, colours and blending, including a fresh longer-clock holdout.
  Native opaque-pixel checks use source-derived masks. The engine exposes
  authorable font metrics/kerning; the handmade mod disables this game's HUD
  while keeping the shared controller/session. Next: remaining
  mirror, HUD sounds, licence outcomes/retry, collision and broader content.
  See docs/hud-evidence.md.
- Driving gauges now use a decoded disc TIM pack, ordinary sprites and
  shaded 2D meshes. Native RPM, speed and transmission state drive the dial,
  needle, redline, speed and gear. Twenty-four original GPU frames match
  geometry, colour, blend and draw order, including controlled boundaries
  and seven fresh driving frames. Native vehicle-state/pixel replay checks
  and the handmade mod preserve the feature boundary. See docs/gauge-evidence.md.
- The records panel now shows the saved best time/speed, label underline and
  three medal targets. Native licence definitions/record state replace the
  original tables; time-cell layout is shared with the clock. Twenty-one
  original saved-time frames and sixty licence target-row probes match.
  The original award routine has been located; B-1 stopping decisions are
  now covered separately. The award/attempt consumer is verified below;
  results/replay/return, name entry, record insertion, retry/reset and saving
  remain unfinished.
  See docs/records-evidence.md.

## Work order

Automatic digital licence recording now matches original writer/finalizer
state across natural attempts, capacity probes and long holds. Native B-1
records its own matching track and storage count; cancelled recordings expose
no playback clip. See [recording evidence](docs/replay-recording-evidence.md).

Native recorded replay inputs now match 1,375 original decoder/normalizer
calls, including complete B-1 playback and fresh schedules. Four retained car
trajectories pass unchanged through that driver, and the actual binary drives
the imported track to its original time and final position. Analog recording,
result-to-replay reset, replay views/menus and return remain. See
[replay inputs](docs/replay-input-evidence.md).

Licence Finish/FAIL, time/prize labels, animated logos and result darkening
now match 946 original glyph draws and their associated logo/quad draws
across 37 cases, including fresh and natural-wait captures. Native gold/failure
font-pixel checks pass; result sounds and the automatic replay consumer remain.
See [result presentation](docs/result-presentation-evidence.md).

Licence result timing now matches 543 original ticks, including fresh
hold/edge schedules and a natural finish-to-timer boundary. The native result
state emits one replay request after confirmation; sounds and the
automatic replay consumer remain. See [result timing](docs/licence-result-evidence.md).

The licence achievement consumer now matches 110 original executions and
424 group-completion checks across all sixty definitions, with native grade
and qualifying-attempt state and a verified importer. It commits at an
explicit return-to-menu event; the native result/replay front end must still
produce that event. Name entry, best-record insertion and persistent saves
remain separate. See [award evidence](docs/licence-award-evidence.md).

The B-1 stopping decision slice now matches 5,800 original calls and 22,616
wheel projections. It latches completed times and overshoot/off-road failures,
with the finished lap clock retained or hidden as appropriate. Course projection
is shared engine machinery with authorable sampling. Result presentation,
awards/attempts, retry/reset and saves remain. Shared post-result braking and
parking now match 510 retained original ticks plus 26 boundary probes, including
a fresh 180-tick run on the updated core. The complete vehicle corpus covers
1,330 original ticks. See [automatic stopping](docs/automatic-stop-evidence.md) and
docs/licence-stop-evidence.md.

1. Decode and import real course geometry, materials, textures, object
   placements and backgrounds. Render the B-1 reference from its recorded
   camera, then broaden to all course assets. Keep every source region
   accounted for and compare rendered frames with the original.
2. Decode vehicle models/parameters and 30 Hz car simulation. Record throttle,
   brake, steering, gears, road contact and edge cases. Port cohesive rules
   with original trajectories, including held-out recordings. Preserve 60 Hz
   presentation and explicit physics cadence.
3. Reproduce the B-1 driving session end to end: car, camera, HUD, input,
   sounds, licence conditions, outcomes and retry flow. Grow the example mod
   alongside the public controller API.
4. Expand the decoded systems to all licences and race modes: cars, tracks,
   opponents, race rules, replays, options, menus, garage/dealership/economy,
   tuning, progression and native saves. Inventory the actual game features
   from its overlays/assets/runtime so missing features are explicit.
5. Complete audio and presentation against reference checkpoints. Exercise
   start-to-finish routes, content variations and performance in shipping
   builds. Port all behaviour required by the supplied disc.
6. Grow an engine-only desktop editor as formats settle: author assets from
   nothing, geometry/collision, placement, triggers and tuning, common-format
   imports and validation. The 0.3.8 template shell now edits `LevelData`
   placements and triggers; racing formats still need editor support.
   Make content-folder overrides usable for total
   conversions and prove extension through the example mod.

## Verification and continuity

Run cargo test --workspace --all-features for routine checks. Use the
private corpus test with GT2_TRO_CORPUS when changing the course importer.
For changed visuals, render and inspect screenshots through developer MCP;
run the engine sandbox and example mod. Document template edits individually
in docs/template.md. Preserve all existing working-tree changes.

Maintain evidence and small, reviewable completed changes. A completed port
must match original behaviour and content, remain extensible, and provide the
authoring workflow required by AGENTS.md; passing structural import tests
alone does not establish parity.
