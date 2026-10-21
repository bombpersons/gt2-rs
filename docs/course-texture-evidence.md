# Course textures and material bindings

The course importer can reconstruct ordinary RGBA PNG materials from a `.trp`
texture pack and the course polygon material words. Source GPU addresses stay
inside the offline importer; the runtime receives texture filenames and UVs.
This establishes texture storage and bindings, not complete scene fidelity.

## B-1 source and independent runtime check

USA Simulation Mode Rev 2 `/crsobj/TC_lisence.trp.gz` is volume resource 8323.
The decompressed file is 68,676 bytes, SHA-256
`4e0303885b8e9d27d9109ef38ca94e5c3ae190edaf423abe6db8f07209942c94`.
Its matching `.tro` SHA-256 is
`421deb2da6ba3837fcf1093a0e1945de226f831d865bb2c5678feef5fd56e182`.

The pack starts with `u32 count = 44`, immediately followed by 44 complete
TIMs, without an offset table or alignment padding. Their declared lengths
tile the file exactly. All 44 are 4-bit indexed images with a 16-entry CLUT.
TIM width counts 16-bit words, so image pixel width is four times that field.
Image and palette rectangles declare their upload coordinates explicitly.

Applying those rectangles in file order produces 33,376 occupied 16-bit
words. **Every occupied word equals the independently captured original VRAM**
in `.psx/runs/b1-gas-1200/vram.bin`. That capture's SHA-256 is
`472bd04dece5d49b4a668a3e2c9ff789b25a1b7b628b1f5e83401421403c2ab8`.
The comparison includes palette overlap: 512 byte writes replace existing
bytes, one of them with a different value. Upload order therefore matters.
This is exact evidence for these source assets and this runtime capture;
it is not a claim about texture animation at other times.

## Section-2 polygon material layout

The renderer starting at `0x80019B58` copies these words into GPU primitives.
Groups 4/5 are textured flat triangles/quads; groups 6/7 are their Gouraud
versions. All four groups use this shared material layout:

| Record offset | Storage |
|---|---|
| `+0x0C` | UV0: `u8 u, u8 v`; `u16 CLUT` |
| `+0x10` | UV1: `u8 u, u8 v`; `u16 texture page` |
| `+0x14` | UV2: `u8 u, u8 v`; quad UV3 in upper two bytes |

UV0/1/2/3 correspond to the source's packed vertex indices 0/1/2/3.
The GPU quad draw order is 0,1,3,2, while triangles draw 0,1,2. Evidence:
triangle loads `0x8001A3D0..0x8001A3D8`, direct output
`0x8001A8E8..0x8001A904`; quad loads `0x8001AADC..0x8001AAE4`, direct
output `0x8001B0AC..0x8001B0D8`. The quad's fourth UV is moved from the upper
half of the last source word with `swl`, hence its placement differs from
simple word-for-word copying. Groups 6/7 share these loads at
`0x8001B25C..0x8001B264` and the later Gouraud quad path.

A fresh original GPU capture at `.psx/runs/course-texture-shading` corroborates
the source RGB and UV interpretation. GPU command 147 is a modulated textured
quad (`op = 0x2C`) whose page, palette and ordered UVs uniquely match section-2
record `0x31C0C`: page `(896,256)`, palette `(608,500)`, UVs
`(0,191),(63,191),(0,160),(63,160)`. Its source RGB `(67,67,64)` is emitted
unchanged in the captured primitive. The source's vertex order 0,1,2,3 maps
to the GPU's 0,1,3,2 as described above. The capture was made with:

```sh
psx run --from-state .psx/runs/b1-state1200/state.bin --frames 2 \
  --gpu-frame 0 --gpu-next-draw --gpu-draw-filter textured \
  --out .psx/runs/course-texture-shading --summary
```

Texture modulation uses RGB/128 as the encoded colour gain (128 is neutral),
while untextured colours use RGB/255. When passing these through the template's
vertex-colour attribute, the gain must first be converted from sRGB to linear:
`psx_material.wesl` calls `linear_to_srgb(v.color.rgb)` before multiplication.
Setting the raw gain directly in a linear vertex attribute would brighten
the result. This conversion detail comes from the template shader, distinct
from the captured original primitive values.

For page word `p`, page origin is `((p & 15) * 64, ((p >> 4) & 1) * 256)`
in word/line coordinates. Bits 7..8 give depth (0=4-bit, 1=8-bit, 2=16-bit).
For CLUT word `c`, palette origin is `((c & 63) * 16, c >> 6)`. The B-1
section-2 polygons use 11 unique page/palette pairs, all 4-bit. Every texel
in every polygon's UV bounding rectangle is backed by a pack upload:
280 primitives, 709,632 checked positions, zero missing positions.

## Road-node material templates

Section 0's `record_base` points at 403 32-byte material templates. Each
holds two 16-byte variants. Within each variant, the first 12 bytes have
the same three UV/material words as the table above. The first variant's
`+0x0C u16` is the projected-area threshold; all templates' final words at
`+0x1C/+0x1E` are zero in B-1.

The compact primitive selects its template with `(word1 >> 9) & 0x3FFF`.
The renderer multiplies this by 32 using `(word1 >> 4) & 0x7FFE0` and adds
the template table pointer. At `0x800245A8..0x800245CC`, a strict unsigned
comparison of threshold < projected area uses the base variant when true,
and adds 16 when false. The quad path at `0x80024E7C..0x80024EB8` agrees.
Thus the base variant supplies larger-screen detail, and the second variant
is selected when area <= threshold. This branch direction matters: the
smaller images are the second variants.

The native renderer now applies this rule to source parents before
subdivision, preserving each variant's UVs and texture binding. The follow-up
[material variant evidence](material-variant-evidence.md) records signedness,
projection-failure ordering, exact runtime selections and native integration.

All 806 template variants have present palettes and complete backing over
their four-UV bounding rectangles. They use 27 unique page/palette pairs.
Together with section 2, B-1 uses 37 unique pairs. The private evidence test
exports all 37 PNGs as a concrete check of the full conversion pipeline.

## Export and verification

`tools/import/src/course_textures.rs` validates count, complete TIM extents,
rectangle dimensions and upload bounds. It exports a full 256x256 RGBA image
per requested page/palette pair, sorted into deterministic names such as
`material_000.png`. UVs convert to normalized coordinates by division by 256.
Unuploaded padding is transparent, and `validate_uv_rect` rejects polygons
that would sample it. Source colour word zero becomes alpha 0; nonzero STP
words become alpha 128, ordinary colours alpha 255. STP is retained for the
engine's material semantics; enabling semitransparency still depends on the
polygon command flags and blend mode.

Synthetic tests cover low-nibble-first indices, transparent zero, STP,
overwritten palettes, missing texels/palettes, truncation, trailing bytes,
out-of-bounds rectangles and impossible counts. Recheck private B-1 evidence
with absolute paths (Cargo runs tests from the importer crate):

```sh
GT2_B1_TRP=/tmp/TC_lisence.trp \
GT2_B1_TRO=/tmp/tro/TC_lisence.tro \
GT2_B1_VRAM="$PWD/.psx/runs/b1-gas-1200/vram.bin" \
cargo test -p import b1_materials_match_the_original_texture_uploads \
  -- --ignored --nocapture
```

This test validates both geometry sources' UV rectangles, compares every
occupied source upload word against the captured runtime, and writes actual
PNGs beneath `/tmp/gt2-b1-texture-materials-<pid>/`.

All 126 decompressed course packs validate: 14,138 TIMs, every entry 4-bit
with a 16x1 CLUT. The corpus test accepts `GT2_TRP_CORPUS` pointing at those
files and checks storage independently of material bindings.

The separate paired-corpus test validates **131,332 textured section-2 faces
and 255,422 road-node template variants** across all 126 matching `.tro` files.
There are 13,023 distinct `(course, page, palette)` bindings. Every palette
and every texel within every conservative UV bounding rectangle is backed
by its matching texture pack; **zero exceptions**. All referenced pages have
4-bit depth. Recheck both complete private corpora with:

```sh
GT2_TRP_CORPUS=/tmp/gt2-course-texture-packs \
GT2_TRO_CORPUS=/tmp/tro \
cargo test -p import validates_material_bindings_across_the_course_corpus \
  -- --ignored --nocapture
```

Runtime upload comparison remains specific to the captured B-1 session.
The corpus checks establish source storage and material coverage, not all
courses' placement, lighting, animation, draw ordering or pixel fidelity.
