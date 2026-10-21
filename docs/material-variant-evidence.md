# Road texture detail selection

The native course renderer selects both authored road texture variants by the
source polygon's projected area, before subdivision or clipping. This covers
the compact textured road groups, not scenery model LOD or complete rendering
parity. The source is USA Simulation Mode Rev 2, using the B-1 checkpoint at
`.psx/runs/b1-state1200/state.bin`.

## Original rule

Each 32-byte material template holds two 16-byte variants. The unsigned
16-bit threshold at template+12 selects large detail when `threshold < area`;
otherwise it selects the second variant, including equality. UVs, texture
page and palette come from the selected variant.

Let `C(a,b,c) = cross(b-a,c-a)` on projected screen pixels in compact source
corner order, X right and Y down. Triangles use `C(p0,p2,p1)` reinterpreted as
unsigned 32 bits. Quads use the absolute value of
`-C(p1,p2,p0) + C(p2,p0,p3)`, equivalent to the absolute perimeter shoelace
sum. A negative-facing double-sided triangle therefore selects large detail;
the triangle path does not take an absolute value.

| Primitive | Area computation | Threshold, unsigned comparison, selected loads |
|---|---|---|
| Flat triangle, group 4 | `800244FC`, read `8002451C` | `800245B4`, `800245BC`, `800245CC..D4` |
| Flat quad, group 5 | `80024DFC`, `80024E1C`, negate `E14`, sum/abs `E98..EA4` | `80024E88`, `80024EA8`, `80024EB8..C0` |
| Gouraud triangle, group 6 | `800257A0`, read `800257C0` | `8002586C`, `80025874`, `80025888..90` |
| Gouraud quad, group 7 | `80026178`, `80026198`, negate `190`, sum/abs `80026230..3C` | `80026220`, `80026240`, `80026250..58` |

The selected material loads precede the failed-projection branches at
`800245EC`, `80024EC4`, `800258A8` and `8002625C`. Failure does not override
the selector. The choice belongs to the original parent and remains fixed
through its entire subdivision tree.

## Original recordings

`tools/record_material_variants.py` traces the four readers and viewport setup
for two fields. The selection oracle is the actual material load address,
not a prediction from the threshold. The recorder verifies all three loaded
words against RAM and saves source corners, transformed coordinates,
projected pixels, threshold, flags and both material variants. Each fixture
retains the full command, checkpoint SHA-256, toolkit build metadata and
trace counts. Incomplete or truncated traces are rejected.

| Fixture | Parents | Triangles / quads | Large / small | Projection failures |
|---|---:|---:|---:|---:|
| Bumper | 91 | 17 / 74 | 26 / 65 | 14 |
| Near chase | 84 | 17 / 67 | 19 / 65 | 10 |
| Far holdout | 80 | 17 / 63 | 21 / 59 | 9 |

All **255 material selections match exactly** in
`game/tests/material_variants.rs`. The shared rounded projection also matches
every recorded parent pixel exactly from its recorded transformed coordinates.
This does not establish arbitrary GTE arithmetic or native world-transform
equivalence. Bumper includes ten 120-pixel mirror records; the other 245 are
320-pixel main-view records. Native per-view geometry for mirrors remains work.

The selector rule was frozen before recording the final far holdout, and no
rule was changed from that result. Its checkpoint had already been used for
subdivision evidence; this is an independent selector trace at that pose,
not a new course/session. B-1 does not execute Gouraud road groups or negative
triangles here: those paths have static evidence and synthetic boundary tests,
not runtime trajectory coverage. All 33 recorded projection failures select
large naturally; the absence of an override is established by the code order.

Reproduce after creating the chase/far checkpoints with the commands in
[subdivision evidence](subdivision-evidence.md):

```sh
python3 tools/record_material_variants.py
python3 tools/record_material_variants.py \
  --checkpoint .psx/runs/subdivision-chase-start/state.bin --name chase
python3 tools/record_material_variants.py \
  --checkpoint .psx/runs/subdivision-far-start/state.bin --name far_holdout
cargo test -p gran_turismo_2 --all-features --test material_variants -- --nocapture
```

Across all 126 source courses, 127,711 template pairs include 74,463 texture
page-word differences, with zero blend-bit or texture-depth-bit differences
between each pair. That is a corpus observation, not a runtime invariant;
the native format allows a separate blend mode for the alternate material.
The existing paired corpus test validates all UV rectangles against uploads.

## Native data and verification

`MeshPart.small_texture` contains an ordinary image path, blend mode,
normalized UVs aligned with the source vertices, and a maximum twice-area
threshold in target pixels squared. `MeshPart.precision` supplies the shared
coordinate grid and distance-dependent precision bound, separately from the
optional quad refinement settings. No GPU page addresses, palette addressing
or packed original records enter runtime data.

The importer preserves all 3,295 textured B-1 road parents and their alternate
UVs. It keeps quad topology until selection, including groups that do not yet
use adaptive refinement. The current scene contains 1,240 source material
batches, 32 exported texture bindings, 6,251 source vertices and 3,883 ordinary
polygons. Runtime large/small material buckets receive only selected parents;
the selected UVs then pass through refinement and near clipping.

The handmade sandbox panel has independent thresholds, grid, UVs and two
small checker textures. Its live camera demonstrates large blue and small
orange detail. `tools/check_sandbox.py` moves the walker, edits the camera,
captures both views and checks their colours through developer MCP:

```sh
cargo build -p engine --example sandbox --all-features
python3 tools/check_sandbox.py
```

Workspace tests, all-target Clippy, the road and texture corpus checks, and
the game/mod GPU checks pass. Bumper/chase/far/rear course views and both
sandbox detail views were visually inspected. No template code changed.
Other primitive refinement, scenery LOD, visibility lists, exact transforms,
draw order, backgrounds and procedural elements remain unfinished.
