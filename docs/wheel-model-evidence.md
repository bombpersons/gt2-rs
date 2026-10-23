# Procedural tire geometry and default wheel image

The native wheel assets reproduce the original B-1 tire profile construction,
default wheel image and checkpoint wheel placements. They are ordinary metre
meshes with PNG texture paths. All six generated front/rear detail streams
match the original's initialized RAM arrays byte-for-byte before conversion.
This establishes geometry construction for this checkpoint. Live steering,
camber, spin and suspension bindings are now verified separately in
[presentation evidence](vehicle-presentation-evidence.md). Upgraded-wheel
selection, dynamic detail selection and a complete car renderer remain open.

## Pointer bases and identity

Every setup offset below is relative to the **outer car record at
`0x800A9688`**, the pointer held in `s0` by `0x80017FA0` and `0x800140A4`.
These are not offsets from the nested physics pointer used by the driving
controller. In particular, outer `+0x418` must not be confused with the
nested controller's unrelated brake parameter at its own `+0x418`.

Source is the USA Simulation Mode Rev 2 checkpoint
`.psx/runs/b1-state1200/ram.bin`; outer `+0x878` points to the identified
`t2vzn` body at `0x8015F894`. Its `+0x40..+0x867` region is zero in the
body file and populated by the loader/setup code. The wheel importer reads
the immutable authored profile tables and setup parameters, reconstructs
the arrays, and compares its results with this initialized region. It does
not export an opaque RAM region or GPU command stream.

## Authored profile and construction

`0x80017FA0` calls `0x80061308(body+0x40, front_setup, rear_setup)`.
The common `0x800611F8` helper applies radial and axial scales to three
immutable profile tables. This is an authored polygon profile, not a newly
invented cylinder or a trigonometric approximation of it.

| Detail | Template address | Bytes | Strip pair counts | Tire quads | Tire triangles |
|---|---:|---:|---|---:|---:|
| High | `0x80091670` | 520 | 17, 17, 8 | 16 + 16 + 7 = 39 | 78 |
| Middle | `0x80091878` | 280 | 9, 9, 4 | 8 + 8 + 3 = 19 | 38 |
| Low | `0x80091990` | 220 | 7, 7, 3 | 6 + 6 + 2 = 14 | 28 |

Each strip has a `u32` pair count followed by that many pairs; a zero count
terminates the table. A pair is six signed halfwords packed
`A.x,A.y,A.z,B.z,B.x,B.y`. The odd ordering supports the original projection
pipeline. The native mesh uses ordinary XYZ positions instead.

Each pair describes two adjacent rings/surfaces in a wheel whose axle is
local Z and rolling plane is local XY. In the first strip, A's radial
coordinates use the outer scale, B's use the inner scale, and both axial
coordinates use the width. This produces the outward-facing annular
sidewall at positive half-width. The second strip uses the outer radial
scale at both ends and connects positive to negative half-width, forming
the tread. The last strip covers the opposite end at negative half-width.
Adjacent pairs produce GPU strip corners `A0,B0,A1,B1` and triangles
`A0,B0,A1 / A1,B0,B1`. The first pair warms the original projection
pipeline, so a strip with N pairs emits N-1 quads. The native constructor
keeps these exact connections and culls tire back faces.

The supplied table coefficients are already polygonal. For example, the
high-detail first radial point is `(4096,814)` and the low-detail first
point `(4096,2364)`, in 12-bit fractional scale. Their radial lengths exceed
4096: the scale parameter must not be used to substitute points on a
unit circle. A modern procedural cylinder would change the silhouette.

The B-1 setup inputs and their actual absolute addresses are:

| Quantity | Front | Rear | Observed value |
|---|---|---|---:|
| Outer radial scale | `0x800A9A98` (outer+410) | `0x800A9A9A` (outer+412) | 1183/4096 m |
| Width | `0x800A9AA0` (outer+418) | `0x800A9AA2` (outer+41A) | 634/4096 m |
| Sidewall radial loss | `0x800A9AA4` (outer+41C) | `0x800A9AA6` (outer+41E) | 507/4096 m |

`0x80018000..038` derives the inner factor as
`4096 - trunc(sidewall * 4096 / outer_scale)` = 2341. The initializer
derives inner scale `floor(outer_scale * inner_factor / 4096)` = 676.
Each signed template coordinate is multiplied by the appropriate scale
and arithmetically shifted right 12. These two rounding stages are kept
offline so the generated array remains byte-exact; the native renderer
only sees metres. The first instantiated high-detail pair is
`(1183,235,317), (676,134,317)` in 1/4096 m.

All three front streams appear at body-relative `+0x70`, `+0x480`,
`+0x6B0`; rear streams at `+0x278`, `+0x598`, `+0x78C`.
`0x80067144..1A8` selects these streams by axle and detail. All six full
streams (520/280/220 bytes per axle) match the offline constructor.
The current native layout selects high detail; original distance-dependent
wheel detail selection is not yet ported.

## Default wheel image and rim surface

The original wheel image is in the body's CDP texture, not a separately
selected resource in this observed B-1 configuration. The original renderer
`0x80067328` reads body+0x40 = `0x7828001A` on all 20 observed wheel draws:
page `0x1A` selects image origin `(640,256)`; CLUT `0x7828` selects
`(640,480)`. This is the first palette of the active body paint, already
identified as paint slot 5 by the full image/palette comparison in
[body evidence](car-model-evidence.md). Thus the baked wheel uses CDP
paint 5, palette 0, preserving RGB555 zero transparency/STP alpha semantics.
The body's other texture palettes remain independent.

`0x80061308` builds four rim corner coordinates from the inner radial scale
and sets the axial plane to `width/2 - inset`. The inset comes from
`0x80091A70 = [0,81,163,245]`, selected by
`(setup_record.flags_at_0x64 >> 7) & 6` as a byte offset. Setup records
start at `0x801D585C` with stride 208 and use outer+0xC as the record index.
B-1 selects inset 81/4096 m, putting the rim at 236/4096 m along local Z.
Both front/rear rim extent is +/-676/4096 m in X and Y.

Let the corners be p0=(-r,+r), p1=(+r,+r), p2=(+r,-r), p3=(-r,-r)
and c=(0,0). `0x800672D8..374` sets up the following two GPU quads:

| Quad | GPU strip corner order | Pixel UVs |
|---|---|---|
| First | p1, p0, c, p3 | (47,0), (0,0), (23,23), (0,47) |
| Second | p1, p2, c, p3 | (47,0), (47,47), (23,23), (0,47) |

The native renderer normalizes U by 256, V by 224 and retains those four
triangles. Modulation is neutral `0x808080`, command `0x2C`: no fabricated
lighting. Within the wheel routine the rim path performs no NCLIP
back-face rejection, and the two source quad windings differ, so the rim
material is double-sided. This is not a claim that rims always render:
the routine can reject exhausted packet space or projection overflow,
and its caller can omit the car. Tire face rejection is separate.
For the B-1 tire pass, body+8's colour word is zero; the emitted native
tire polygons are black. Their raw geometry is retained even where the
body occludes it.

## Placement, spin and opposite-side reflection

`0x800140A4` builds race render arguments. `0x80014568/56C` read four
outer+0x7C4+i*16 position triples; `0x80014578..5A8` construct yaw/pitch/spin
from wheel state. The checkpoint values, in 1/4096 m, are:

| Wheel | Position XYZ | Spin, 4096/turn |
|---|---|---:|
| Front left | (-3010,557,-4773) | 1172 |
| Front right | (3010,557,-4773) | 1128 |
| Rear left | (-3006,601,4830) | 1703 |
| Rear right | (3006,601,4830) | 1657 |

These are the actual initialized wheel positions, including the observed
suspension/body relationship. They differ slightly from simply copying
the body file's wheel-centre attachments. Spin is outer+0x4AC+i*104;
outer+0x498+i*104 and the front/rear halfwords at outer+0x394 are zero
here (the extra orientation inputs). Their broader dynamic rules remain
unported. Wheel transforms are relative to the same rendered body pose
used by the body/camera, not independent world coordinates.

At this zero-steer/camber state, the wheel rotation is
`Ry(-pi/2) * Rz(spin)`. This was corroborated against actual matrix loads:
`.psx/runs/wheel-pose-matrix-reads` records four scratch+0x9C matrices
consumed by `0x8007B994` at run frame 2, before side reflection. For example,
the first matrix rows are `(0,0,-4095)`, `(3948,1092,0)`,
`(1091,-3948,0)`, in /4096 units. Its spin is now 848/4096 turn; the other
draw spins are 804/1377/1331. The changes from the checkpoint are observed
live rolling over the first two fields, not tuning adjustments. Other
three consumed matrices match the same rotation construction. Native
quaternions use normal radians/unit rotations; the table's 4095/4096
quantization is not emulated.

For odd wheels, `0x800671B8` adds a half-turn to the yaw, after the caller
has added the opposite quarter-turn. The result again has yaw -pi/2.
`0x8006725C..27C` then negates the resulting third matrix column and
`0x8006729C..2B4` toggles the tire culling sign. This is a local axial
reflection, not a different tire silhouette. The importer bakes right-side
meshes with local Z reflected and triangle winding reversed, giving the
same placement with ordinary positive-scale native rotations and correct
front-face culling. No runtime negative-scale workaround is required.

## Native assets and public authoring contract

`tools/import/src/car_wheels.rs` contains the offline construction. Rebuild:

```sh
cargo run -p import -- wheels --ram .psx/runs/b1-state1200/ram.bin \
  --textures /tmp/gt2-cars/t2vzn.cdp --paint 5 \
  --out game/assets/gt2/vehicle/wheels --texture-prefix gt2/vehicle/wheels
```

It rejects a different identified body, palette/page selection, paint slot,
nonzero checkpoint orientation extras or any generated-stream mismatch.
It writes front/rear high/middle/low ordinary `GeometryData` RONs, reflected
right-side equivalents, the default palette image, and provenance sidecars.
`game/assets/gt2/vehicle/t2vzn-wheels.ron` is a readable placement asset:

```text
(body_asset: "gt2/vehicle/t2vzn.ron", wheels: [
    (name: "front left wheel", geometry: "gt2/vehicle/wheels/front-lod0.ron",
     position_m: (...), rotation_xyzw: (...)), ...
])
```

It is keyed to the body path so an unrelated replacement body cannot
silently inherit these attachments. Each wheel is a geometry asset plus
its local position and unit quaternion. The canonical profile has Y up,
Z as the axle and XY as the rolling plane; after the authored placement
its axis lies across the car. Right-side source reflection is already in
the geometry. A replacement can author arbitrary meshes and local poses;
no particular vertex count, radius, tire shape, texture, joints, clips or
original disc data is required. The native presentation loads the geometry
under the body subject and exposes the wheel index/subject for future
animation. The wheel layout can be disabled or replaced separately.

The example mod replaces both the body and layout with handmade geometry:
four charcoal cuboid wheels on the coral/blue box car. It uses its own
dimensions/poses and no original wheel profiles, source vertices or image.

## Recordings and remaining verification

Commands producing the new runtime evidence:

```sh
psx run --from-state .psx/runs/b1-state1200/state.bin --frames 12 \
  --input '[{"frame":0,"buttons":["R1"],"until":4}]' \
  --watch-reads 8015F8D4-80160118 --max-events 80000 \
  --out .psx/runs/wheel-draw-reads --summary
psx run --from-state .psx/runs/b1-state1200/state.bin --frames 4 \
  --input '[{"frame":0,"buttons":["R1"],"until":4}]' \
  --watch-reads 1F80009C-1F8000BC --watch-reads 800A9E4C-800A9E8C \
  --max-events 30000 --out .psx/runs/wheel-pose-matrix-reads --summary
```

The first contains 3,025 untruncated reads, including every source strip
and the wheel image selection over 20 wheel draws. The second contains
5,590 untruncated reads, including the consumed matrix and actual race
wheel positions. The earlier `wheel-pose-reads` attempted scratch write
watch yielded zero writes; it is not evidence for matrix values.

Native GPU/MCP rendering passes. Inspected
`/tmp/gt2-vehicle-view/three-quarter.png` and `wheel-close.png` show the
correct outward-facing default rim image on both visible wheels, black
polygonal tire surfaces and plausible attachment to the decoded body.
The same scene's chase/rear checks pass at
`/tmp/psx-template-headless-222/chase.png` and `rear.png`. These additional
oblique views were produced by `/tmp/check_gt2_vehicle.py`, moving the
native camera without changing the decoded car. This is geometry/material
and framing QA, not a same-camera pixel comparison with the original.
The inspected mod image `/tmp/psx-example-mod-756506/circuit.png` uses the
independently authored box car and charcoal cuboid wheel assets; its GPU/MCP
test and camera-follow checks pass.

`cargo test -p import car_wheels -- --nocapture` passes three tests: source
scaling/rounding, invalid dimensions, and outward tire winding after the
right-side reflection while preserving rim UVs. The actual checkpoint bake
also verifies every byte of all six reconstructed source streams before
writing native geometry, rather than only comparing bounds or counts.

Still required: held-out wheel construction/selection on other cars and
upgrades; source-wheel image
selection beyond this default; live steering/camber/spin/suspension rules;
dynamic wheel detail and occlusion/packet-order parity. The car's reflection
and auxiliary/shadow rendering are separate unfinished passes.
