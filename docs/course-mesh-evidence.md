# Course mesh storage

The B-1 scene now renders decoded road-node polygons and all 13 placed
section-2 scenery instances with imported textures. Section-2 arrays validate
across all 126 courses, and all 6,084 scenery placements decode. Dynamic LOD,
procedural elements, background and intro/replay camera behavior remain open;
this is not complete scene or gameplay parity.

The corpus is all 126 `/crsobj/*.tro` resources from USA Simulation Mode Rev 2,
decompressed with the existing volume importer. No external format reference
was used. `TC_lisence.tro` SHA-256 is
`421deb2da6ba3837fcf1093a0e1945de226f831d865bb2c5678feef5fd56e182`.

## Storage established from files and code

Each section-2 table entry is a file-relative offset to a mesh. Its extent
ends at the next entry, or at section 3 for the last mesh. Inside each mesh:

| Offset | Storage | Evidence / meaning |
|---|---|---|
| `+0x00` | `u32` | File-relative vertex-array offset; always mesh + `0x58`. Renderer `0x80019B6C` loads it into scratchpad + `0x70`. |
| `+0x04..+0x28` | Ten `u32` | File-relative primitive-array offsets, in the order below. Empty arrays share an offset. |
| `+0x2C` | `u16` | Vertex count; each vertex is 8 bytes. Loaded with `lhu` at `0x8001C1A4` by the alternate renderer. |
| `+0x2E` | `u16` | Zero in all meshes; meaning unassigned. |
| `+0x30..+0x42` | Ten `u16` | Primitive counts, paired with the ten pointers. The renderer loads the first at `0x80019B78`, the sixth at `0x8001A930`. |
| `+0x44..+0x49` | Three `s16` | Minimum x/y/z bounds; read by `0x8007B8F8` at `0x8007B900/904/908`. |
| `+0x4A` | `u16` | Zero in all 5,709 meshes; unused by these bound loads. |
| `+0x4C..+0x51` | Three `s16` | Maximum x/y/z bounds; read at `0x8007B90C/910/914`. Every vertex lies within these bounds. |
| `+0x52` | `u16` | Zero in all meshes. |
| `+0x54` | `s16` | Scale exponent passed to `0x8007B8A0` by `0x8001F998`; observed range 16..26. One coordinate unit is `2^(exponent-28)` metres (see below). |
| `+0x56` | `u16` | Zero in all meshes. |
| `+0x58..` | Vertex array | Each entry is three signed 16-bit coordinates and a fourth 16-bit word. Renderer loads each as two words into GTE vector registers. The fourth word is outside the coordinates; its meaning is not assigned. |

The first eight primitive groups are triangles (even group) and quads (odd
group). The first word packs three vertex indices in bits 0..9, 10..19 and
20..29. Quads take a fourth index from bits 0..9 of the second word. Bits
30..31 and the rest of the second word include rendering flags; they must
be masked before range checks. For example, `0x8001A964..0x8001A9C0` extracts
these indices and multiplies each by eight to load vertices.

| Group | Record bytes | Records across corpus | Rendering shape |
|---|---:|---:|---|
| 0 | 12 | 827 | Flat triangle |
| 1 | 12 | 5,731 | Flat quad |
| 2 | 20 | 218 | Gouraud triangle |
| 3 | 24 | 1,038 | Gouraud quad |
| 4 | 24 | 18,183 | Textured flat triangle |
| 5 | 24 | 107,634 | Textured flat quad |
| 6 | 32 | 2,355 | Textured Gouraud triangle |
| 7 | 36 | 3,160 | Textured Gouraud quad |
| 8 | 28 | 6,027 | Procedurally expanded quads; positions and sizes, not vertex indices |
| 9 | 20 | 488 | Procedurally expanded screen-facing elements; positions and sizes, not vertex indices |

All strides are corroborated by the array extents in every mesh and the
count/stride calculations in the renderer starting at `0x80019B58`. For
example, group 2 uses count × 20 at `0x80019EF4..0x80019F00`, group 3 uses
count × 24 at `0x8001A08C..0x8001A098`, and group 7 uses count × 36 at
`0x8001B884..0x8001B890`. Groups 8/9 are expanded by the separate routine
`0x8001F7F8`: group 8 at `0x8001F9E8..0x8001FB78`, group 9 from
`0x8001FBA8`. Their full field interpretation is pending. Colour/UV words
in the polygon records are not yet converted to engine materials.

## Runtime corroboration

From the existing B-1 checkpoint, a four-field read watch over section 2
recorded 2,170 reads without truncation (`psx` 0.3.4 build
`src-b72afc8e8efe891f-release`):

```sh
psx run --from-state .psx/runs/b1-state1200/state.bin --frames 4 \
  --watch-reads 800E47A8-800E7738 --max-events 10000 \
  --out .psx/runs/course-mesh-readers --summary
psx disasm --ram .psx/runs/b1-gas-1200/ram.bin \
  --function 80019B58 --text
psx disasm --ram .psx/runs/b1-gas-1200/ram.bin \
  --function 8001F7F8 --text
```

The loaded file base is `0x800B4A34`. Concrete observations in `reads.jsonl`:

| Reader PC | Address read | File location | Value |
|---|---|---|---|
| `0x8001F998` | `0x800E5B9C` | Mesh 4 + `0x54`, file `0x31168` | `0x0015` (scale exponent 21) |
| `0x8001A964` | `0x800E63D0` | Mesh 6 group 5 first record, file `0x3199C` | `0x40100C02` (indices 2, 3, 1 plus flags) |
| `0x8001A968` | `0x800E63D4` | Same record + 4 | `0x00000000` (fourth index 0) |

This observes the original consuming section-2 geometry. It does not establish
complete scene reconstruction or pixel parity.

## Checks and remaining work

`tools/import/src/tro/mesh.rs` validates the mesh-local header, exact vertex
and primitive extents, ordered bounds and every polygon vertex reference.
Previously pointers were allowed anywhere up to section 3, including into
later meshes; each array must now fit its own mesh. Counts and strides tile
all section-2 payloads exactly: **5,709 meshes, 280,301 vertices, 145,661
primitives**. The B-1 course has **13 meshes, 532 vertices, 280 primitives**.

Normal tests use synthetic meshes with all ten groups, upper index flags,
bad triangle/quad references, inconsistent counts/pointers, truncated blobs
and out-of-bounds vertices. Recheck the private corpus with:

```sh
cargo test -p import
GT2_TRO_CORPUS=/tmp/tro cargo test -p import \
  validates_extracted_course_corpus -- --ignored --nocapture
```

The corpus test requires this revision's complete 126-file corpus and checks
the totals per group. The static scene importer now emits both road-node and
placed mesh polygons to engine-native geometry, as recorded below.

Verification for this change: all 37 importer tests passed including the
private corpus check; `cargo test --workspace --all-features`, workspace
formatting and importer Clippy with warnings denied passed. The engine
sandbox also rendered offscreen and its Walker moved 1.5 metres over 30
right-input ticks (`/tmp/gt2-mesh-sandbox/`).

The first baseline workspace run failed
`display::tests::saved_settings_load_on_the_next_start` (426×240 instead of
640×480). A fresh workspace run and 30 isolated reruns passed. The cause
has not been established; no template files were changed in this investigation.

## Scenery lists and world transforms

Placement lists are **before section 0**, not in section 3's span bodies.
File `+0x118` contains 33 file-relative pointers. Each list is `u32 count`
then `count` 28-byte records. All lists tile `0x19C..section0` exactly in
all 126 files, including four-byte headers for empty lists. There are 6,084
placements total. B-1 has 13 entries in list 0; all other lists are empty.

| Record offset | Storage | Interpretation |
|---|---|---|
| `+0x00/+0x02/+0x04` | Three `s16` | Rotation angles, 4096 per turn |
| `+0x06` | `u16` | Section-1 model index (a model references one or more LOD meshes) |
| `+0x08/+0x0A/+0x0C` | Three `s16` | Axis scale, 4096 = 1 |
| `+0x0E` | `s16` | LOD-distance divisor, 4096 = 1; not a fourth axis scale |
| `+0x10/+0x14/+0x18` | Three `s32` | World x/y/z translation, 16.16 metres |

`0x8002993C` passes the loaded image (`0x800B4A34`) into `0x8002002C`.
That routine visits lists 0..31 under the road renderer's visibility mask
and list 32 unconditionally. `0x8001F858..868` indexes each entry by 28;
`0x8001F874..888` copies translations to the local transform. At
`0x8001F88C..898`, the angles enter `0x80081374` as `(-angle1, angle0,
angle2)`, giving `Ry(-angle1) * Rx(-angle0) * Rz(angle2)`. Then
`0x8001F8A8..B8` postmultiplies `diag(scale0, scale1, -scale2)` through
`0x8007B25C`/`0x8007B994`. The third-axis reflection applies to local
coordinates, not world translations. The conversion retains the course axes,
with negative z on the B-1 straight.

At `0x8001F990..9A0`, the mesh exponent enters `0x8007B8A0` with base
exponent 12. `0x8007B778` shifts translations by `exponent-12` while GTE
vertices remain signed integers, so one coordinate unit is
`2^(exponent-28)` metres. Instance 1 references model 4, mesh at file
`0x31114`, exponent 21: one local unit is 1/128 m. Model index differs from
mesh-table index: B-1 placements use models `3,4,5,6,7,8,9,10,11,12,2,0,1`.

The resident sine table at `0x80093150` has 5,120 halfwords (a full turn
plus a quarter for cosine). Every entry equals
`trunc(sin(angle * tau/4096) * 4096 + 0.5)`. Negative values round
asymmetrically, e.g. `sin(-pi/2)` is -4095. The importer reproduces the
table and intermediate matrix truncations offline, then emits float metre
positions. Plain floating-point rotations failed the original matrix check.

`tools/record_course_scenery.py` regenerates three two-field traces, checking
that the original visits all 13 instances and the full sine table matches.
The committed fixture `game/tests/data/original/course/scenery_transforms.json`
holds matrices extracted from stores to `0x1F80009C` after scaling,
checkpoint/table hashes and exact reproduction commands. Cases cover identity,
a rotation regression, and a second rotated/nonuniformly scaled case recorded
after quantization was implemented and never used to change it. All nine
matrix coefficients match exactly in all three cases. The held-out rotation
is `(-67,731,-317)` with scales `(4055,4096,3907)`.

## Native geometry conversion and current render

`import tro-scene` combines the eight ordinary polygon groups from road-node
meshes and placed section-2 scenery. Node meshes have a header at `node+0xA4`:
eleven pointers, a `u32` vertex count at `+0xD0`, ten `u16` primitive counts
at `+0xD4`. Vertices start at `+0xE8`. The earlier reading of `+0x92` as a
pointer count was incorrect. Node renderer `0x80020110`, local transform
`0x80026BB4` and compact polygon reader `0x80023504` establish 1/64 m
vertices relative to the node's centre rounded down to 64 m, with stored
`(x,z,height)` reordered into engine `(x,y,z)`. Across the corpus, all 14,387
nodes validate: 1,129,468 vertices and 699,211 ordinary polygons. Compact
node indices use 9 bits; section-2 indices use 10. Textured node polygons
select the 32-byte templates in [texture evidence](course-texture-evidence.md).

Section-2 textured Gouraud records keep the first colour at `+8`, UV words
at `+12/+16/+20`, and additional colours at `+24/+28[/+32]`. The importer
retains per-corner colours, UVs, source side flags and command blend modes.
The engine receives metre positions, normalized UVs, sRGB colour gains,
triangles and ordinary PNG asset paths. Source GPU addresses stay offline.

Regenerate B-1 with:

```sh
cargo run -p import -- tro-scene /tmp/tro/TC_lisence.tro \
  --textures /tmp/TC_lisence.trp \
  --out game/assets/gt2/course/TC_lisence/scene.ron \
  --texture-prefix gt2/course/TC_lisence
```

Output: 124 road nodes + 13 scenery instances, 6,251 source vertices, 3,883
ordinary polygons, 32 texture bindings from 44 TIMs. With flat-textured quads
retained for view-dependent refinement and both road material variants, the
current asset has 1,240 source material batches.
The offscreen MCP render was
visually inspected alongside `.psx/runs/b1-state1200/screen.png`: road,
curbs, trees, barriers, gantries and distance signs render in the native
scene. This is a visual review, not a pixel-parity claim. The original HUD,
car session, backgrounds and additional camera paths differ or remain missing.
The camera/lens and clipping follow-up is in [camera evidence](camera-evidence.md).

The scene uses the highest-detail mesh for every placement. Road texture
variants now follow [parent projected-area selection](material-variant-evidence.md).
Visibility masks, distance-dependent model LOD, procedural groups (106 B-1
records), background, intro/replay cameras and
draw-order parity remain work. The modern editor is also unfinished.
Flat-textured road quads now use [recorded subdivision](subdivision-evidence.md);
other primitive groups' refinement remains separate work.

Normal tests cover all eight polygon layouts and reject invalid list extents,
model references and LOD divisors. The game validates baked geometry and
checks the 52 nonempty node names, 13 scenery names and 32 texture files.
The other 72 source nodes contain no ordinary polygons. Recheck:

```sh
GT2_TRO_CORPUS=/tmp/tro cargo test -p import tro:: -- --include-ignored
python3 tools/record_course_scenery.py
```

Verification for this follow-up: workspace tests with all features, workspace
Clippy, formatting and diff whitespace checks passed. All 14 course-importer
tests passed with the full private corpus enabled. Game and example-mod
offscreen MCP tests passed, including stepping and screenshots; both images
were inspected. The engine sandbox rendered and its Walker advanced from
x=0 to x=1.5 m over 30 right-input ticks. Logs are in
`/tmp/gt2-scenery-{workspace-tests,corpus-tests,clippy-final,gpu-final}.log`;
sandbox evidence is in `/tmp/gt2-template-sandbox/`. No template files were
edited for this follow-up. The baseline engine test still expected reflected
z/source winding and was corrected to agree with current imported coordinates;
the mod's headless test now makes the same explicit ribbon choice as its
binary, and ribbon rendering no longer requires an unused AssetServer.
