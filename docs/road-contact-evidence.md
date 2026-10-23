# Decoded polygon contact and vehicle/road frames

Road queries now run against decoded course collision data, rather than
recorded wheel-height/surface inputs. `engine::course` performs the ordered
node/grid/polygon search and height interpolation; `vehicle::road_contact`
owns height history, surface processing, procedural bumps and road frames.

All **257 original car calls (1,028 wheel queries)** match exactly, including
a fresh 53-call holdout. Tests compare the actual selected node and polygon,
height and metadata, smoothed wheel heights, offsets, attachments, body/road
bases and the resulting **native suspension travel, spring/damper and loads**.
Natural sequences retain heights, node seeds, bases, suspension and tilt histories.
The [vertical body rules](vertical-body-evidence.md) now also produce corrected
pose/angles and velocity natively before the late attachment/frame refresh.
[Aerodynamics](aerodynamics-evidence.md) now produces both axle loads in this
same pipeline; reduction history carries across consecutive ticks.

Movement-entry position/velocity and the body-force inputs to suspension remain
observed boundaries. Broader collision/reset and licence outcome actions are
unfinished. This is a connected road/suspension/body comparison, not a closed
B-1 trajectory or a playable session.

## Identity and reproduction

USA Simulation Mode Rev 2, Vitz F '99, B-1 at vblank 7500.
State SHA-256 `49fb814844da1b4d160f966c4cb675405fb58eb3d74ebc90702ad58978a2ef5b`;
RAM SHA-256 `ff147faec522bdd3da3f6231a379c4b09e323e6d094186bcb0a71a5a5441ab04`.

```sh
psx disasm --ram .psx/runs/b1-state1200 --function 80028830 --text
psx disasm --ram .psx/runs/b1-state1200 --function 80028470 --text
psx disasm --ram .psx/runs/b1-state1200 --function 800279E8 --text
psx disasm --ram .psx/runs/b1-state1200 --function 80027C70 --text
psx disasm --ram .psx/runs/b1-state1200 --function 80043578 --text
psx disasm --ram .psx/runs/b1-state1200 --function 80044EA4 --text
psx disasm --ram .psx/runs/b1-state1200 --addr 8003E7EC --count 62 --text
python3 tools/record_road_contact.py
python3 tools/record_road_contact.py --holdout
cargo test -p gran_turismo_2 --all-features --test road_contact
GT2_TRO_CORPUS=/absolute/path/to/extracted/courses cargo test -p import --all-features validates_extracted_course_corpus -- --ignored --nocapture
```

The recorder requires complete execution/watches, zero dropped trace lines,
one car, four complete queries and all eight stage boundaries, including
the pre-aerodynamic input snapshot. It records
the actual selected polygon/node at `800284BC/C0`, rather than inferring
identity from an equal flat height. Commands, hashes and toolkit identity
accompany the private CSV/JSON in `game/tests/data/original/road_contact/`.
Traces, RAM and reports remain under `.psx/runs/road-contact-*`.

## Collision data and engine ownership

The importer validates all **126 courses: 14,387 collision grids, 268,488
referenced polygons and 660,086 cell references**. These counts describe
structural coverage; runtime comparisons use B-1 and authored interventions.

| Source node field | Meaning |
|---|---|
| +18/+1C/+20 and +24..28 | Exact search point and direction, /65536 metres and /4096 direction |
| +30/+34/+38 masked down to 64 m | Collision-local origin, same framing as node drawing |
| +9C/+A0 | Collision grid start/end |
| +A4/+D0 | Eight-byte local vertices and vertex count; coordinates are (x,file-z,height), /64 m |
| Grid+0/+2, +4/+6 | Local horizontal minimum and two power-of-two cell shifts |
| Grid+8..26, +28..64 hex | Sixteen cell counts, then sixteen file pointers to ordered polygon lists |
| Polygon word 0 | Three 9-bit vertex indices, bit 30 selects a quad |
| Polygon word 1 | Fourth vertex index, surface bits 28..31, variant bits 23..24, flags bits 25..26 |

Every cell list tiles the grid extent exactly. Every reference must name an
ordinary primitive record inside this node, of the correct triangle/quad
kind, with all vertex indices in range. It cannot borrow arbitrary bytes
from a vertex array, procedural primitive or another node. Corruption tests
exercise list bounds, primitive identity, kind and vertex references.

`ContactMesh` exports local metre vertices, a world origin, explicit grid
dimensions/cell sizes, ordered polygon indices and sampling steps. Runtime
code sees no packed source words, pointers or GPU primitives. The sampler
uses normal Bevy world (x,y,z) axes; source wheel-query z is negated at its
offline/observation boundary. Exact node search points and collision origins
use f64 so distant 16.16 coordinates preserve edge ties. Rendering can keep
its existing f32 position fields.

`engine/assets/sandbox/contact.ron` is a complete authored example: a flat
floor and ramp in the same format. The engine-only sandbox renders the ramp
and samples it while walking. Its MCP check verifies that moving right also
raises the character. Unit tests cover grid bounds, edge selection, a
nonplanar quad's sampled centre, signed height sampling and invalid assets.
This needs neither `game` nor original content.

## Polygon selection and height

`80028394` walks backward until the query is ahead of a node, then forward
while it is ahead of the next. Equality advances. `80028470` tests that
node's grid first, then alternating previous/next nodes, up to eight in each
direction. The returned longitudinal seed stays the chosen node even if a
neighbor supplies the polygon. `RoadContact` exposes both identities.

Containment floors the local horizontal query to 1/64 m before grid lookup
and tests clockwise edges inclusively, preserving signed-halfword query and
signed area-word sampling. Ordered cells preserve first-hit selection where
polygons overlap. Missing contact is `None`, rather than a fake floor.

Triangles use their plane. Quads use a four-triangle fan about the mean of
all four vertices, floored independently to 1/64 m; this includes centre
height. The source's sector/tie choices select the fan triangle. Plane height
uses the unrounded horizontal query, signed wide numerator/division and a
separately truncated 1/65536 m delta added to the third vertex/centre height.
A fixed diagonal or bilinear patch would differ on nonplanar quads.

## Game rules and temporal boundaries

Offsets below use physics base `800A96B4`, wheels at +460, stride 68 hex.

- `800431A0/4323C` project wheel levers through the retained body basis.
  Each product truncates separately, then the attachment narrows to a signed
  halfword. `80043388` adds the body position to form each wheel query.
- `80043624..34` retains the new raw height at wheel+4 and stores the floored
  mean of new/previous raw heights at wheel+0, /4096 metres. A missing query
  leaves height, surface and variant unchanged and clears only road offset.
- Surface/variant copy to wheel+14/+15. Surface 6 counts as a reset surface
  and aliases to 5 after the original off-road count. Flag bit 1 contributes
  to the special-surface count and the mode-gated condition bypass.
- `8003C360` approximates speed as max plus rounded-up half of min, using
  absolute contact velocities. Its separately sampled gain caps wheel+40
  at full bump response. It is not a Euclidean length.
- Globals `80046F88/6F98/6FA8` supply amplitude, spatial frequency and an
  optional speed multiplier. The waveform sums source sine(x) and cosine(y),
  preserving phase flooring, signed sine bias, low-word speed multiplication
  and final floored halfword offset. Exported frequency is cycles/metre.
- `80044EA4` builds body/road Euler bases with separately sampled intermediate
  products. `8003E7EC` interpolates smoothed heights along each side, derives
  centre height, pitch and roll, then builds the road basis. Procedural bump
  offsets do not enter that road-plane fit.

`RoadContactTuning` / per-car `RoadContactSetup` hold levers, plane-fit
dimensions/fraction, bump settings and direction-curve thresholds. The
exporter creates `vehicle/road_contact.ron` locally. The shared sampled
direction routine now lives in `vehicle::direction`; wheel-slip trajectories
still pass unchanged. No original tuning arrays are published.

`RoadContactState` owns node/raw-height history and **retained** body/road
bases. `SampleRoad` follows horizontal integration and precedes suspension.
It supplies actual road heights/surfaces/offsets/attachments, but suspension
still uses the prior road basis. `RefreshFrame` follows native body
correction and precedes contact-motion refresh: it first refreshes attachments
with the retained integrated body basis (`8004335C`), then rebuilds body/road
bases from corrected body angles and current heights. Refreshing the road
basis during sampling would use the wrong tick.

Optional bridges update `SuspensionInput`, `TyreGripInput`,
`BodyDynamicsInput` and `ContactMotionInput`. `BodyOrientation` is explicit
pitch/roll state, updated by the vertical body feature, with heading from `VehiclePose`.
`RoadContactInput::condition_surfaces_enabled` represents `80041AB8`'s mode
gate; it is false in B-1. True gating has binary evidence and an authored
test, not an alternate race-mode runtime comparison. The standalone authored
test exercises height history, surface aliasing, enabled/disabled gating,
per-car wheel placement and the bridges without other rules or private data.

## Runtime coverage and remaining work

| Suite | Calls | Evidence |
|---|---:|---|
| Spatial probes | 33 | Signed longitudinal/lateral positions, grid limits and missing polygons |
| Neighbor fallback | 3 | Empty current grid at actual overlapping mesh seams; actual neighbor/polygon identity recorded |
| Surface metadata | 32 | All eight surface indices, all four flag combinations and all four variants |
| Warped height/history | 3 | Authored nonplanar vertex heights and signed previous raw heights |
| Body angle probes | 6 | Signed pitch/roll/heading with source basis retention and late rebuild |
| Bump motion probes | 7 | Signed, zero, tiny and full-response contact velocities |
| Natural gas/brake/turn | 120 | Forty calls each, retaining node, height, basis and suspension histories |
| Fresh holdout | 53 | New warp/lateral/metadata/angle/fallback probes plus forty alternating-control calls |

Interventions alter the source grid, vertex or polygon metadata producer,
not wheel outputs that sampling overwrites. Native tests alter the equivalent
authorable engine data and compare actual selected polygon identity and
results. The seam probes also freeze horizontal motion at its verified
bypass boundary so the intended seam survives initial integration. No native
reference model supplies expected outputs. The fresh holdout passes without
changing the road/suspension rules.

Workspace tests, importer corpus checks, native game/example-mod MCP render
checks and the engine sandbox pass. Screenshots were inspected. Clippy has
only the existing wheel-rotation test warning. Current-source/history audits
pass; all generated assets and numerical evidence remain ignored.

Next: close the full B-1 feedback loop, remaining body-force/proximity and
condition producers, and live driving/presentation.
The licence reset/outcome code after `80043780`, live car/wheel animation,
alternate torque/induction paths, request production, audio, wider game flow
and authoring editor remain unfinished. The native scene still displays its
checkpoint body pose; these comparisons do not claim live visual parity.
