# Road quad refinement

Flat-textured road quads now retain their topology through import and use
the original view-dependent refinement rules. B-1 has 2,907 such quads.
The long affine texture streaks visible from the bumper camera are removed.
This covers section-0 node-mesh group 5. Other primitive groups, section-2
mesh refinement, complete clipping/visibility and
pixel parity remain unfinished.

## Original rule and data boundary

Source: USA Simulation Mode Rev 2, the supplied disc, race code in the B-1
checkpoint. The flat-textured quad reader begins at `0x80024C94` in the
ordinary node renderer. Original rules established from that code:

| Rule | Evidence |
|---|---|
| Refine when any corner cannot project safely or twice the projected area exceeds 2,048 pixels squared | `0x80024E44..4EC4`, `0x800250EC..5100`; main-view threshold set at `0x800201E4..1EC` |
| Make four edge midpoints and one centre, flooring signed coordinates after averaging | `0x800251CC..52AC`: half-sums and a four-corner quarter-sum |
| Floor U and V to whole source texels independently at every midpoint/centre | `0x800252C0..5354`; byte stores, not final-only rounding |
| Traverse corners in reverse perimeter order; odd recursive children rotate their corner order | `0x800253F0..5498`, `0x800254BC..55FC` |
| Emit edge stitch triangles when projected rounding makes them nondegenerate | `0x80025418..5484`; these are not extra authored surfaces |
| Reject a child when all four corners share a viewport/near-plane exclusion; otherwise refine an unsafe/oversized child again | `0x800254E8..55CC` |
| Stop recursion after seven refinement levels; discard children still needing another split | `0x800255CC..55F4`: stack starts at `0xE8`, strides by `0x60`, last parent at `0x328` |

The original keeps conservative visibility classifications on existing
corners and uses exact viewport limits on newly projected midpoints.
`0x8007B374` builds the original corner planes with a 16-pixel guard band
(argument supplied by `0x80029924..928`). Recomputing all inherited corner
classifications at every split incorrectly discards a boundary leaf in the
bumper recording. The native refinement keeps those inherited decisions.

The node's bound selects coordinate precision. `0x80026BB4` compares
`distance_to_centre² + radius²` and scales the transform by up to two extra
bits: two below 128 m, one below 256 m, otherwise none. The base coordinate
step is 1/64 m. Close roots with projection failures gain further precision
at `0x80025008..50D4`: the magnitude mask determines a common shift so that
coordinates use 15 bits. This preserves their projection while giving later
midpoints a finer grid. The importer emits the bound centre/radius and these
parameters in normal units; raw node headers do not reach the engine.

Perspective quotient rounding also affects which stitch triangles are
nondegenerate. Plain floating-point division reproduced the split tree and
counts but selected a different stitch at source polygon `0x800CFF14` in
the bumper view. Quantizing the quotient to 16 fractional bits before
coordinate multiplication reproduces the recorded stitch selections and UVs.
Projected integer corners match exactly in the bumper case and differ by at
most one pixel in the other two recordings. This is bounded recorded
projection evidence, not proof of arbitrary-input GTE arithmetic equivalence.

## Recording and comparisons

`tools/record_subdivision.py` traces numeric refinement inputs and emitted
primitive fields through `psx`. It rejects incomplete or truncated runs.
Full traces stay under `.psx/runs/`; fixtures contain coordinates, UVs,
packet corner pixels, counts, build identity, commands and checkpoint hashes.

- Split inputs are loads at `0x800251CC..527C`, with UV loads at
  `0x800252C0..5324`. The recursion depth comes from the source stack slot.
- Leaf commits are observed at `0x800255B8`; the preceding stores supply
  their ordered UVs and screen corners.
- Stitch commits are observed at `0x80025480`, likewise with UVs/pixels
  from their preceding stores.
- Main viewport records are selected by the consumed width at
  `0x8002535C` (320); mirror records are separate and excluded.

| Recording | Root quads | Splits | Leaf quads | Stitches | Largest corner-pixel difference |
|---|---:|---:|---:|---:|---:|
| Bumper, checkpoint frame 7500 | 16 | 113 | 194 | 134 | 0 |
| Near chase, frame 7508 | 16 | 93 | 167 | 151 | 1 |
| Far chase holdout, frame 7524 | 15 | 124 | 222 | 194 | 1 |

All 330 splits match in traversal order, depth, all coordinate components
and rounded UVs. Counts and ordered UVs of all 583 leaf quads and 479 stitch
triangles match exactly. Pixel coordinates are checked at every emitted
corner with a one-pixel bound. The far-view holdout was recorded after the
quotient-rounding fix and never used to change the rule. The near-chase
recording predates that final correction and is additional regression evidence.
All fixtures are under `game/tests/data/original/subdivision/` and are tested
by `game/tests/subdivision.rs` using the engine routine, independently of
camera/vehicle motion and the scene loader.
The pixel bound applies to the refinement projection from recorded transformed
corners; it does not include the native camera/world transform or final GPU
vertex snapping.

Reproduce:

```sh
python3 tools/record_subdivision.py
psx run --from-state .psx/runs/b1-state1200/state.bin --frames 8 \
  --input '[{"frame":0,"buttons":["R1"],"until":4}]' --save-state \
  --out .psx/runs/subdivision-chase-start --summary
python3 tools/record_subdivision.py \
  --checkpoint .psx/runs/subdivision-chase-start/state.bin --name chase_holdout
psx run --from-state .psx/runs/b1-state1200/state.bin --frames 24 \
  --input '[{"frame":0,"buttons":["R1"],"until":4},{"frame":10,"buttons":["R1"],"until":14}]' \
  --save-state --out .psx/runs/subdivision-far-start --summary
python3 tools/record_subdivision.py \
  --checkpoint .psx/runs/subdivision-far-start/state.bin --name far_holdout
cargo test -p gran_turismo_2 --all-features --test subdivision -- --nocapture
```

## Native integration and authoring

`GeometryData` now supports perimeter-ordered quads and optional
`QuadSubdivision` data alongside its existing triangles. Existing triangle
assets remain readable. The importer retains flat-textured node quads rather
than flattening them prematurely. It separates their batches from primitive
groups with different rules. B-1 still has 3,883 ordinary source polygons,
6,251 source vertices. The [material variant follow-up](material-variant-evidence.md)
now retains both road variants, with 32 textures and 1,240 source batches.
All 126 source courses still pass their structural/reference checks.

The engine transforms the authored corners into raster space, refines them,
converts the result back to ordinary local-space vertices and then applies
the near-plane clipping path. It emits normal Bevy meshes/materials. Root
facing is checked before refinement, allowing stitch triangles either winding
after pixel rounding. Rebuilds are cached against the camera/projection,
object transform and target size. This is a perspective mesh path, not a
screen capture used as a level or a replay of original GPU packets.

The engine-only sandbox loads `engine/assets/sandbox/panel.ron`, a handmade
quad with different precision, area, UV-grid and guard-band parameters. It
uses the same renderer without game code or imported assets. An author can
add quads or change the profile directly in RON. Geometry validation checks
attributes, references and profile ranges; synthetic tests cover precision
boundaries and the recursion limit. The final desktop authoring application
required by the port guide remains unfinished.

Remaining rendering work includes the other road primitive groups, section-2
refinement, exact transform/pixel rounding,
packet/draw order, per-view geometry for mirror/split-screen, backgrounds and
procedural elements. Numerical tree/packet comparisons do not establish
full framebuffer parity or a finished driving game.

Verification: workspace all-feature tests, all-target Clippy and the 126-course
importer corpus pass. The developer MCP checks render bumper, near/far chase
and rear views and exercise component editing and camera changes. The
engine-only sandbox renders its handmade panel and moves its walker; the
example mod renders its own circuit and follows its subject. These rendered
views were inspected as images. No template code was changed for refinement.
