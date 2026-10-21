# Car body and paint storage

The offline car decoder reads every CDO/CNO body and every CDP/CNP texture
in USA Simulation Mode Rev 2. It emits metre positions, unit normals,
triangle/quad corner references, normalized UVs, palette selections and
RGBA colors. The body meshes are decoded independently of a captured frame.
The shadow/auxiliary trailer and reflection rendering pass remain unfinished;
these files alone do not establish a finished car renderer. Static procedural
tires, the default wheel image and checkpoint placements are now converted
separately; see [wheel evidence](wheel-model-evidence.md).

The native B-1 scene now attaches the decoded high-detail `t2vzn` body and
observed paint slot 5 to its rendered subject pose. The bumper camera hides
that subject's body and wheels; near/far chase expose them. The rendered
subject and wheel orientations remain at the checkpoint until live body and
wheel dynamics are connected.

## Identity and observation

The B-1 checkpoint `.psx/runs/b1-state1200/state.bin` contains `t2vzn`
body data at `0x8015f894`. Its vertex slice at file `+0x900` matches the
checkpoint RAM byte-for-byte. The CDO and CNO variants are identical for
this particular car, so their geometry cannot distinguish which variant
was requested. Other cars' CDO and CNO files differ.

| Resource | Volume id | Decompressed bytes | SHA-256 |
|---|---:|---:|---|
| `t2vzn.cdo.gz` | 7415 | 16496 | `7574e04a48aafc3ffae7a418dd90cfede36f839dede75f9d1e88591fbe094093` |
| `t2vzn.cno.gz` | 7417 | 16496 | `7574e04a48aafc3ffae7a418dd90cfede36f839dede75f9d1e88591fbe094093` |
| `t2vzn.cdp.gz` | 7416 | 45984 | `1d3f526283493a770e906812e840683fc858ea33035742525353e3af669d9dc2` |
| `t2vzn.cnp.gz` | 7418 | 45984 | `22af6fb0a8277ced74e18202b1c9be662843d2c83ad47913ba22dfac825e483f` |

B-1 starts in a bumper camera, which produces no body mesh reads. R1
switches to a chase camera. The following recording produced **12,045
untruncated CPU reads** from the body meshes:

```sh
psx run --from-state .psx/runs/b1-state1200/state.bin --frames 12 \
  --input '[{"frame":0,"buttons":["R1"],"until":4}]' \
  --watch-reads 80160118-80163804 --max-events 20000 \
  --out .psx/runs/car-r1-reads --summary
psx disasm --ram .psx/runs/b1-state1200/ram.bin --function 80061798 --text
psx disasm --ram .psx/runs/b1-state1200/ram.bin --function 800616C4 --text
psx disasm --ram .psx/runs/b1-state1200/ram.bin --function 80067444 --text
```

The high LOD header starts at `0x80160118`; readers `0x800675d4` and
`0x800675d8` read its radius and scale exponent. `0x800675e8/f4` read the
relocated vertex and normal pointers. `0x80061814/18` read the first
primitive count and pointer. The first source primitive's relative CLUT
`0x40` becomes `0x7868` in RAM; its relative page 0 becomes `0x1a`.
These are ordinary file relocations performed by the original loader,
not values the native engine needs to reproduce.

## Body layout

The first word is `0x00025447` (`GT`, version 2). Four signed-coordinate
attachment entries are at `+0x20`, `+0x28`, `+0x30`, `+0x38`, each four
`s16` components. `0x80061490` copies components 3,1,2 as wheel-centre
x/y/z; component0 is the outer x coordinate (half a tire width from
component3 in B-1). These are not three coordinates plus padding.
The wheel renderer `0x800670f0` shifts the attachment translation left4
to enter the common16.16 transform domain, independently confirming
the attachment 12-bit fractional representation. The large zero-initialized area before `+0x868` is populated
with procedural wheel geometry at runtime.

At `+0x868`, `u32 3` declares three LOD entries. The first mesh starts at
`+0x884`. Later meshes immediately follow the preceding mesh's final
primitive. File-relative LOD pointers at `+0x878` and `+0x880` can be zero;
when present, they equal these contiguous starts. Treating a zero as an
absent mesh silently loses authored geometry.

Each LOD begins with an **80-byte header**:

| Offset | Storage | Interpretation |
|---|---|---|
| `+0x00` | `u16` | Vertex count, at most 256 |
| `+0x02` | `u16` | Packed normal count, at most 512 |
| `+0x04..+0x12` | Eight `u16` | Primitive counts |
| `+0x14` | `u32` | Relative vertex start, always 80 |
| `+0x18` | `u32` | Zero in file; becomes the relocated normal pointer |
| `+0x1c` | `u32` | Relative normal start in file |
| `+0x20..+0x34` | Six `u32` | Relative primitive starts; empty arrays can use zero |
| `+0x38` | `u32` | Reserved/zero in observed files |
| `+0x3c..+0x40` | Three `s16` | Minimum bounds |
| `+0x42` | `u16` | Padding |
| `+0x44..+0x48` | Three `s16` | Maximum bounds |
| `+0x4a` | `u16` | Padding |
| `+0x4c` | `u16` | Scale exponent; 16 in the B-1 body |
| `+0x4e` | `u16` | Radius passed to the transform/bounds helper |

Vertices follow the header: three `s16` coordinates and one unused halfword,
8 bytes per entry. Packed normals follow the vertices, 4 bytes per entry.
`0x800616c4` extracts signed 10-bit components from bits 2..11, 12..21,
22..31. B-1's packed vector lengths are 498.52–499.95. The importer
normalizes them to unit vectors, retaining the authored directions.

Primitive groups 0..7 have strides **16, 16, 24, 28, 28, 28, 36, 40**;
only groups 0, 1, 4 and 5 occur in this release's 2,220 body files.
Even groups are triangles; odd groups are quads. The first three/four
bytes are vertex indices. A quad stores perimeter corners and the renderer
reorders them to GPU strip order `0,1,3,2`, giving triangles
`0,1,3 / 3,1,2`. The color/command word is at `+12`; bit 1 of its command
byte enables semi-transparency, bit 0 uses an unmodulated texture.

The second word holds flags and one normal index in bits 5..13. When its
high bit enables the additional reflection pass, the third word supplies
normal indices in bits 1..9, 10..18, 19..27. All referenced normals are in
range. These references describe the reflection pass, not a replacement
for the body's flat texture/color rendering.

Textured primitives contain `uv0,clut` at `+16`, `uv1,page` at `+20`, and
`uv2,uv3` at `+24`; each UV is two bytes. All relative pages are zero.
The palette number is `(clut >> 6) * 4 + (clut & 63)`; all are in range.
All texture V values fit 0..223 and U values fit 0..255.

B-1's mesh starts are `0x884`, `0x2570`, `0x3a28`; vertex/normal/face
counts are respectively **174/198/193**, **117/144/139**, **32/40/34**.
The final mesh ends at `0x3fac`, followed by an unresolved 196-byte
auxiliary block beginning with `u16 24, u16 2`. The decoder reports the
trailer byte count and does not export an opaque blob as authored content.

The native body conversion is `raw_coordinate * 2^(exponent - 28)`
metres. Attachment coordinates use 12 fractional bits. Model-local x/y/z
uses positive y upward. B-1 has exponent16, so its coordinates divide by4096. For
B-1 this gives body bounds **1.910 × 1.541 × 3.611 metres**. The runtime transform corroborates this conversion: `0x800675dc` calls
`0x8007b8a0` with model exponent 16 and target exponent 12. At run frame 2
of `.psx/runs/car-transform-reads`, the camera-space translation entering
`0x8007b2cc` is `(0,107220,353894)` in the established 16.16 world domain.
The body renderer then loads `(0,6701,22118)` from scratchpad
`+0x78/+0x7c/+0x80`, exactly arithmetic right shift by 4. Both express
`(0,1.636,5.400)` metres, so the body vertices share the resulting 12-bit
fractional domain.

### Local-axis check

The same recording captures the body's actual GTE matrix loads, rather
than a later reused scratchpad snapshot. At run frame 2, reader PCs
`0x800617EC/F0/F8`, `0x80061800/08` read scratchpad `+0x84..+0x94`.
The first nine signed halfwords form these rows:

```text
 4096      0      0
    0  -3723     11
    0    -13  -4096
```

The first row maps file-local +X to screen-right. The second includes the
already-established negative 3723/4096 viewport Y scale; positive file Y
therefore points upward. The negative third diagonal maps local +Z toward
the chase eye, so local -Z is forward. This matches the native camera
subject's local axes without a corrective rotation, reflection or scale.
Small off-diagonal coefficients represent body/camera pitch. The native
test `file_local_axes_project_as_the_original_chase_body` compares every
axis under the checkpoint subject and native near-chase camera to these
observed coefficients, allowing 0.002 for the existing quaternion-versus-
quantized-camera difference. It does not assert pixel parity.

The runtime camera-space translation `(0,6701,22118)` is also consistent
with the recorded 1.8 m height and 5.4 m back offset: the viewport scales
1.8 m to about 1.636 m and the source depth faces +Z. The original body
screen to compare against is `.psx/runs/car-r1-reads/screen.png` (12 fields
after R1). The four-field transform recording's final screenshot still
shows the older bumper frame, so it is not used as body-image evidence.

## Paint layout

Every CDP/CNP file is exactly **45,984 bytes**:

| Span | Meaning |
|---|---|
| `0..2` | `u16` paint count, 1..30 |
| `2..32` | One-byte paint names/ids; remaining header bytes |
| `32..0x43a0` | 30 paint slots, each 18 palettes ×16 colors ×2 bytes |
| `0x43a0..0xb3a0` | 256×224 image, 4 bits per texel, low nibble first |

Palette entries are PS1 RGB555 with STP in bit 15. Native RGBA preserves
transparent zero as alpha 0 and STP as alpha 128; other colors are opaque.
The converter can produce a normal RGBA image for any paint/palette pair.

Flat RGB uses byte/255 in native geometry; textured modulation uses
byte/128, because PS1 texture colour 0x80 is neutral. Raw-texture commands
are converted to neutral modulation explicitly. All 171 textured B-1
high-detail faces use raw-texture commands (`0x25`/`0x2d`), and the remaining
22 flat faces are black. There is no invented paint tint or reflection.

The checkpoint's full image rows match the CDP indices at VRAM `(640,256)`.
The active palettes match paint slot **5** (`0xb60`), uploaded at `(640,480)`
with four CLUTs per row. Distinctive palette rows at offsets `0xb80..0xd40`
match the corresponding VRAM grid exactly. CDP and CNP differ in pixel and
palette data; their broader day/night selection semantics are not established
by this one checkpoint and are not hardcoded in the decoder.

## Native body and authoring contract

The importer writes the selected high-detail body into the reusable engine
geometry format, with one native material bucket per palette/blend selection:

```sh
cargo run -p import -- car /tmp/gt2-cars/t2vzn.cdo \
  --textures /tmp/gt2-cars/t2vzn.cdp --paint 5 --lod 0 \
  --out game/assets/gt2/vehicle/t2vzn.ron --texture-prefix gt2/vehicle
```

This emits 13 material parts and 333 triangles from the 174 authored
positions/193 faces, plus 12 ordinary 256x224 RGBA palette images and source
provenance sidecars. Geometry and images are baked; no raw body parser, VRAM
address or GPU command list enters the native application. Reflection normal
references remain offline evidence, not invented body lighting.

`VehiclePresentationPlugin` is a separate public feature. Insert a
`VehiclePresentation { geometry: Some(asset_path), wheels: None }` before
adding it to select another engine-native body without inherited wheels, or
set both fields to `None` to disable dressing. The optional `wheels` path
selects a readable `WheelLayout` keyed to the selected body path; its local
metre positions/quaternions attach any authored wheel meshes. Public
`VehicleWheel { subject, index }` exposes those attachments for animation.
`VehicleBody` opts any ordinary subject Transform into dressing. Its child
`VehicleBodyRoot` points back to the subject. Local axes are X right, Y up,
-Z forward; units are metres. There are no required joints, animations,
controllers, sounds or imported files for a handmade replacement. A matching
ordinary `VehicleCamera` in bumper mode hides its own body. Without such a
camera, the body stays visible. Mirror/multiple-view body visibility needs
per-view rendering and remains open alongside the scene's single clipping
camera limitation.

The example mod overrides this asset path with an independently authored
coral box deck and blue box cabin, using only ordinary colours and triangles.
It demonstrates the public presentation and asset-layer boundary without
the original car art. The presentation test validates the imported body
topology, dimensions, paint slot, colour neutrality and file-axis projection;
another test disables the feature without renderer or original assets.

The initial body-only headless MCP check passed. The inspected native chase image
`/tmp/psx-template-headless-748369/chase.png` shows the imported body upright,
centred at the same scale as `.psx/runs/car-r1-reads/screen.png`: the roof,
rear-window and rear-body outline align closely by visual inspection. This
is a bounded placement/framing check, not pixel or complete car parity. The
native windows/paint lack the original's view-dependent reflections; tires,
shadow and live body motion are absent, as expected from this body-only pass.
The native screenshot follows the static checkpoint pose; the original image
is 12 fields later, so these images do not establish identical simulation
state. The earlier native image under `/tmp/psx-template-headless-747850/`
had the test road lifted 1 m and is excluded from body-placement evidence.

`cargo test -p example-mod --all-features` and its ignored `dev_mcp` test pass.
The inspected handmade mod image `/tmp/psx-example-mod-748664/circuit.png`
shows its independently authored deck/cabin body on its authored road, and
`camera-follow.png` confirms subject following remains functional. These
images check the extension path, not original-game visual parity.

The subsequent procedural-wheel integration was inspected at
`/tmp/gt2-vehicle-view/three-quarter.png` and `wheel-close.png`. It shows
the original default rim image facing outward on both visible wheels,
black polygonal tires, and attachment locations consistent with the body.
The current mod image `/tmp/psx-example-mod-756506/circuit.png` renders its
own independently authored car and cuboid wheels. Detailed construction,
static-pose limits and reproduction commands are in
[wheel evidence](wheel-model-evidence.md). Shadows, reflections and live
body/wheel motion are still absent.

## Decoder validation and remaining work

The private corpus check uses resources extracted through the existing GTFS
reader and decompressed without changing the source bytes:

```sh
GT2_CAR_CORPUS=/tmp/gt2-cars cargo test -p import car_model \
  -- --include-ignored --nocapture
```

All **2,220 bodies, 6,660 LODs, 765,313 vertices, 945,187 packed normals,
842,731 faces and 2,220 paint files** pass the decoder. Checks include
contiguous array extents, optional LOD pointers, vertex bounds, face and
normal references, palette selection, relative page values and UV bounds.
All but one body per variant have 196 auxiliary bytes; the outlier has 364.
Synthetic tests reject bad vertex/normal references, inconsistent array
pointers, out-of-bounds coordinates and invalid/truncated paint data.

Still required for car parity: wheel selection beyond the observed default,
dynamic wheel detail; the auxiliary/shadow geometry; view-dependent
reflection rendering; proof of CDO/CDP versus CNO/CNP selection; animation
of steering and wheel rotation from vehicle state. These are distinct from
the validated body topology and texture storage.
