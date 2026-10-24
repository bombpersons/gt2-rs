# Data formats

The bytes in `GT2.VOL`, what wraps them, and what is known so far about each
payload. The engine's own asset formats are derived from this document by
`tools/import`; anything not marked verified is a hypothesis with the evidence
that supports it.

Container and addressing are in `docs/engine-model.md` ("`GTFS`"). Every
observation below is reproducible with

```text
import volume  <image> --out game/assets/gt2/volume.index.json
import extract <image> --path /carparam/eng_gtmode_data.dat.gz --out file.gz
```

## Transport: raw or gzip, two layers

A resource is either stored raw or gzip-compressed, and **the file name in the
volume does not reliably say which**:

| Stored | Count | Note |
|---|---|---|
| gzip stream | 5205 | mostly `*.gz`, but `/gtmenu/*/gtmenudat.dat` too |
| raw | 6373 | `.tim`, `.es`, `.seq`, `.ins`, `.idx`, … |

Deciding by magic, not by name, is mandatory. Verified: feeding the aligned
byte range of each of the 5114 `*.gz` resources to a gzip decoder yields
end-of-stream with all input consumed (80 693 248 bytes in, 222 813 744 out);
no `.gz` payload is itself gzip.

### The gzip header names the file

Every gzip resource carries its **original name and mtime** in the gzip header
(`FLG & 8`). The name is the true name, extension included, and is how the
formats behind the `.gz` names were identified:

```text
/dirt/aa_nn_dirt_2            -> "aa_nn_dirt_2.dgf"    (not ".gz", ".dgf")
/carparam/eng_gtmode_data.dat.gz -> "eng_gtmode_data.dat"
```

The mtimes cluster on 1999-11-13 … 1999-11-26, i.e. the build dates of the
files, not of the disc image.

| Inner extension | Count | Volume paths |
|---|---|---|
| `.cdo` `.cdp` `.cno` `.cnp` | 1110 each | `/carobj/` |
| `.lgf` | 180 | `/license/` |
| `.tim` | 131 | `/crsmap/`, `/bgsobj/` |
| `.tro` `.trp` | 126 each | `/crsobj/` |
| `.dgf` | 81 | `/dirt/` |
| `.bso` `.bsp` | 34 each | `/bgsobj/` |
| `.dat` | 29 | `/carparam/` |
| no name field at all | 15 | `/gtmenu/<lang>/gtmenudat.dat`, `iconimg.dat.gz`, `solodata.dat.gz` |
| `.gmr` 4, `.in` 3, `.tsd` 1, no extension 1 | 9 | `/arcade/`, `/sound/`, `/.crstims.tsd.gz` |

## Resource census

All 11580 ids, grouped by payload (after gunzip) and directory. Leading-bytes
groups are family identities, not full format definitions.

| Payload begins | Count | Where | Read as |
|---|---|---|---|
| `10 00 00 00` | 3707 | `/carlogo` 3346, `/carwheel` 192, `/crsmap` 120, `/arcade` 49 | PS1 TIM texture |
| `45 4e 47 4e` `ENGN` | 2747 | `/engine` | engine sound sample (SPU ADPCM, decoded) |
| `47 54 02 00` `GT\x02` | 2220 | `/carobj` | car model (`.cdo`, `.cno`) |
| `02 00` + 2 ASCII chars | 2220 | `/carobj` | car part pack (`.cdp`, `.cnp`) |
| `47 54 44 54` `GTDT` | 22 | `/carparam` | game data table |
| `47 54 4d 50` `GTMP` | 1 (44.7 MB) | `/gtmenu/commonpic.dat` | menu image archive |
| `47 54 2d 50 53` `GT-PS` | 126 | `/crsobj` | course object (`.tro`) |
| `42 47 00 00` `BG\0\0` | 34 | `/bgsobj` | background object (`.bso`) |
| `49 4e 53 54` `INST` | 5 | `/sound` | SPU instrument |
| `53 45 51 47` `SEQG` | 10 | `/sound` | SPU sequence |
| small byte table | 261 | `/license` 180, `/dirt` 81 | licence slide / dirt graphic |
| offset table + TIMs | 192 | `/crsobj` `.trp`, `/bgsobj` `.bsp`, `/arcade/arc_carlogo` | texture pack |

The remaining raw files are `/font/racefont.dat`, `/.text/data-race.txd`, the
root tables and the six empty `/replay` slots.

## Notes per family

### TIM textures

Standard PS1 TIM: `u32 magic 0x00000010`, `u32 flags` (bits 0–2 pixel mode,
bit 3 CLUT present), then the optional CLUT block and the image block, each
`u32 size` followed by `u16 x, y, w, h` and `size - 12` bytes of data. Verified
on `/arcade/arc_font.tim`: CLUT size `0x6c` (16×3 16-bit entries), image
`0x0f80c`, and `8 + 0x6c + 0x0f80c = 65664` = the resource's true length.

**The width field counts 16-bit VRAM words**, so the pixel width is
`w * 16 / bits-per-pixel`: ×4 at 4bpp, ×2 at 8bpp, ×1 at 16bpp. This is what
makes the declared sizes exact on every family, and reading `w` as pixels
halves the image. Three checks:

| Resource | Header | Pixels | Texel bytes | Accounted |
|---|---|---|---|---|
| `/arcade/arc_font.tim` 4bpp | `w 128, h 256` | 512 × 256 | 65536 | `8 + 0x6c + 0x0f80c` = 65664 = file |
| `/carwheel/bb001--s.tim` 4bpp | `12, 48` | 48 × 48 | 1152 | 1216 = file |
| `/crsmap/2p_autumn.tim.gz` 4bpp | `24, 96` | 96 × 96 | 4608 | 4672 = file |
| `/carlogo/a-a7rl--.tim` 4bpp | `64, 40` | 256 × 40 | 5120 | 5184 = file |
| `/arcade/l-sel.tim.gz` 8bpp | `176, 480` | 352 × 480 | 168960 | 168980 decompressed = 8 + 12 + texels |
| `/arcade/arc_topmenu` 16bpp | `512, 120` | 512 × 120 | 122880 | 122900 = file |

`tools/import`'s `import tim`/`import tims` decode them. All **3707** TIM
payloads in the volume convert: 3668 from their own CLUT or as 16-bit direct
colour, and 39 indexed images with no CLUT at all.

`/crsmap/NAME.tim.gz` is a single gzipped TIM; `/bgsobj` has `.tim` beside its
`.bso`/`.bsp`.

#### The 39 palette-less screens

Every `/arcade` screen panel is an **8bpp image with no CLUT block**:
`arc_panels*.tim` (`w 320` = 640 × 256), `topmenu_panels*.tim` (`128` = 256),
`arc_goodies_*.tim` (`128` = 256 × 248), `arc_maker.tim` (`256` = 512),
`gtmode_font.tim`, `gt_cursor.tim`, `gt_items.tim`, `setting.tim`,
`license_tim.tim`, `arc_key_config.tim`, `champtim.tim`, `langsel.tim.gz`, and
the language variants. Evidence for 8bpp rather than 4bpp or 16bpp:

- The declared image size is exactly `w × 2 × h` bytes for all 39 — consistent
  with any of the three, so the size alone does not decide.
- 4bpp is impossible for `arc_panels` (`w 320` would be 1280 pixels, wider than
  the 1024-pixel VRAM).
- 16-bit direct colour renders as colour noise in flat areas, where a real
  index image keeps flat indices; at 8bpp with a grey ramp (`arc_panels`,
  640 × 256) the screen reads cleanly: "Single Player / Road Race / Dirt Trail /
  Time Attack / Drag Race / Goodies / Option", the "Class-S / Class-A /
  Class-B / Class-C" panels, "Easy / Normal / Hard / Professional", "Home
  Garage / Guest Garage / Load Guest Garage".
- `l-sel.tim.gz` and `title_*.tim.gz` are the same family *with* a 256-entry
  CLUT in the file and the same `w = 176, h = 480`, and they render correctly at
  352 × 480 as 8bpp.

So the palette is uploaded by the game, not stored with the screen. The PNGs
`import tims` writes for these show the indices as a grey ramp; recovering the
real palette needs a VRAM capture from the arcade screens (**open**).

#### One TIM per file

`arc_font.tim` is a single TIM: its header accounts for all 65664 bytes
(ending at `0x10080`). Byte sequences that look like the magic (`10 00 00 00`)
do occur inside the image data — seven of them are at offsets above `0xd8cd` in
this file — but parsing one there gives nonsense flags, so they are coincidence,
not a pack. Nothing in the volume's `.tim` resources is a pack.

### `/engine/NNNNN.es`: `ENGN`

2747 resources: 305 numbered assets (`00000`, `00001`, …, `20910`, …) each with
a `_n0.._n3` and `_t0.._t3` part, plus `ene_n.es` / `ene_t.es`. Decoded, and
verified against all 2747 files (`tools/import/src/engn.rs`):

```text
0x00  "ENGN"          magic
0x04  u32 0
0x08  u32 header      header size: 0x20 + (zones + 1) * 16
0x0c  u32 0
0x10  u32 payload     ADPCM payload size in bytes, a multiple of 16
0x14  u32 header      the same value as 0x08
0x18  u32 zones       header records after the first: 0..=10 observed
0x1c  u32 0x30        constant in every file
0x20  record[0]       16 bytes: 0, 0, 0x7f, 0x7f (as four u32)
0x30  record[1..=zones], 16 bytes each
...   payload         SPU ADPCM, 16-byte frames
```

`header + payload` is the whole resource in every file: `import engines`
reports **0 bytes** of resource padding across all 2747, so nothing is
unaccounted for. The records sit on a fixed 16-byte grid (`0x20 + (zones + 1)
* 16` equals the header size in all of them) but their *fields* are not
established. They plausibly hold a per-RPM zone table — in `00001.es` the
payload is one loop while the second u16 of each record walks 1000, 2000, …
8000, which reads as engine RPM — but naming the fields needs the loader, and
that lives in a `GT2.OVL` overlay: the boot executable names only
`/engine/ene_n.es` and `/engine/ene_t.es` (`0x8008f32c`, `0x8008f340`).

The payload is the PS1 SPU's ADPCM: a frame is 1 byte of shift (low nibble)
and filter (high nibble), 1 flags byte, then 14 bytes of 28 four-bit samples,
high nibble first, decoding at the SPU's native 44 100 Hz. Decoding checks out
against the audio itself: `00000.es` is a 441 Hz tone with an autocorrelation
of 0.995, exactly the periodic loop the format implies.

Most `_n*`/`_t*` slots hold one shared placeholder (the 4000-byte 441 Hz
`00000.es` loop), so the 2747 files are only **467 distinct payloads**;
`import engines` writes each once. The four `n*`/`t*` parts per asset are
probably variants (near/far, on/off throttle); which the game picks is
established by the overlay loader, not here.

`import engines <image> --out <dir> [--match <s>]` bakes them: each distinct
payload to `sample_NNNN.wav` and a `table.ron` naming every original path, the
sample it uses and its zone count. Until the overlay loader is ported the bake
covers the two files the executable names. The full set is 467 WAVs (~98 MB,
1148 s); the command is the same without `--match`.

### `/carobj`: `GT\x02` and `02 00 xx`

`/carobj` has 4440 files, 1110 cars × 4: `.cdo`, `.cdp`, `.cno`, `.cnp`, all
gzipped. After gunzip:

```text
a-a7r.cdo  GT\x02\x00 | 00*24 | 16 05 14 04 24 05 14 04 bf f1 99 03 ...
a-a7r.cdp  02 00 | "ls" | 00*20 | 14 00 21 04 03 00 07 00 42 08 0a 00 ...
a-a7r.cno  GT\x02\x00 | identical to the .cdo head
a-a7r.cnp  02 00 | "ls" | identical to the .cdp head
```

So `.cdo`/`.cno` are the same object format and `.cdp`/`.cnp` the same pack
format; the `c`-prefixed pair is likely the "colour"/opponent variant. The
`02 00` version followed by two ASCII characters (`ls` here, `14`, `16`, `s1`,
`no`, `hb`, … across the set) is a chunk identity, which also explains why the
first four bytes differ between otherwise identical files. Not decoded.

### `/carparam`: `WSDB` text and `GTDT` tables

`eng_unistrdb.dat` and its six language siblings are `WSDB`, "world string
database": the game's name list, **indexed by slot**. Decoded in full — every
byte of all seven files is accounted for:

```text
0x00  u32             file length in bytes (equals the real length)
0x04  "WSDB"
0x08  u16             entry count
0x0a  entry[count]    [u16 chars][chars * UTF-16LE][u16 0]
```

The string length is in UTF-16 **code units**, not bytes, and each string ends
with a `u16` zero. `tools/import`'s `import text` writes this to the engine's
RON [`TextTable`](../engine/src/text.rs); the slot order is the identity, so
nothing is sorted or deduplicated and a language's different length is kept.

| File | Bytes | Count | First / last entry |
|---|---|---|---|
| `eng_unistrdb` | 21 194 | 846 | "Active Stability Controller" / "Aero RS(J) '97" |
| `usa_unistrdb` | 21 518 | 851 | same family |
| `jpn_unistrdb` | 16 128 | 764 | UTF-16 (e.g. "ワゴンR エアロRS") |
| `fra` / `ger` / `ita` / `spa` | 21 368–22 020 | 846 | translated part names |

The names are car parts and car names ("Sports Brake Kit", "Brake Balance
Controller", "Single Plate Clutch", …), so this is the GT-Mode shop/garage
string table, not the HUD's (`/.text/data-race.txd`, `docs/formats.md`,
"Text"). Which code reads `WSDB` is not yet traced.

`gtmode_data.dat` / `arcade_data.dat` / `license_data.dat` begin `47 54 44 54`
(`GTDT`) — a header then a table of `[offset, size]` pairs that tile the file:

```text
0x00  "GTDT"
0x04  u16             (108 in eng_gtmode_data)
0x06  u16             block capacity (62)
0x08  [u32 offset, u32 size][capacity]   the first `n` blocks, contiguous
```

Each block's `offset + size` is the next block's `offset`, and the last ends
at the file length. `eng_gtmode_data.dat` uses 31 of its 62 slots; the rest are
zero. The blocks are the game's per-shop/per-event data (the engine model's
closest thing to a type table), but each block's **schema is not yet read out
of the code** — that is the next table investigation, not a byte-format one.

### `.idx` and `.dat`: seekable archives

Two indexed archives exist:

- `/gtmenu/<lang>/gtmenudat.dat` (37 554 980 bytes) with `gtmenudat.idx`;
- `/gtmenu/commonpic.dat` (44 724 224 bytes, `GTMP` magic) with
  `commonpic.idx`.

Both `.idx` files are the same shape — a count, one more word, then one **end**
offset per entry, the last equal to the `.dat` length:

```text
u32 count
u32 field        (0 in commonpic.idx, 2 in gtmenudat.idx; meaning unknown)
u32 end[count]   end offset of entry i; end[count-1] == .dat byte length
```

`commonpic.dat` is `count` (= 458) chunks, each beginning `GTMP`. Chunk 0
starts at 0, every boundary lands on a `GTMP`, and the 458 sizes are all
multiples of 0x1000 (0x5000..0x43000, summing to the file length), so the index
delimits the chunks exactly. A chunk header is `"GTMP"` + `u32` (chunk 0:
`0x00140503`, chunk 2: `0x025e3680`), then its body.

`gtmenudat.dat` is **3386 concatenated gzip members**, not one. Each member
decompresses to a `GM\x03` blob (member 0: 4962 compressed → 17012 bytes,
`47 4d 03 00 …`). Members are 4-byte aligned, with 0–3 zero bytes between
them; the compressed sizes plus padding sum to the file length exactly. The
`.idx` end offsets track those boundaries to within 0..5 bytes but not
exactly, so the `.dat` is the authority and the `.idx` is not needed to split
it.

The bodies are still encoded, not plain TIMs. In `commonpic.dat`, the `10 00 00
00` TIM magic occurs 8989 times but only 281 occurrences parse as a strict TIM
header, so it is not a TIM pack. Chunk 0's body is 2026 unique 32-bit ids —
mostly 32-value groups `(N<<8)|(0x9f-j)` with N falling from 0x3e — then a short
Shift-JIS label, then zero padding to the chunk end. The `GM` blobs are a short
header followed by high-entropy data.

Neither payload is a gzip or raw-deflate stream: no start offset in a chunk
yields inflatable output, and high-entropy 16-byte windows of a chunk do not
appear in the VRAM dump of a frame that displays them, so a chunk is not copied
to VRAM verbatim either. The chunk body is also never read by the CPU — a
`watch_reads` over the whole licence route only ever catches the overlay
decompressor borrowing the same buffer as Huffman-tree scratch. That is
consistent with the CD controller loading the chunk into RAM and a GPU DMA
uploading it, but the destination and the frame it happens on are not pinned
down yet.

The code that reads either payload is **not in the boot EXE**; it is in
`GT2.OVL` (above), which is now extracted, so the search is over 1.1 MB of
readable MIPS rather than blocked on an unreadable overlay.

`import menu <dat> --idx <idx>` splits either archive and accounts for every
byte (458 chunks / 3386 members, 0 unaccounted), so a different disc revision
fails loudly instead of silently decoding wrong.

Two compressed companions sit beside them, one per menu group:

- `iconimg.dat.gz` → 32768 bytes, 252 distinct byte values, mostly `00`/`FF`:
  a 4bpp icon sheet.
- `solodata.dat.gz` → 4564 bytes; starts `u32 50`, then 50 `u16` that are
  `{0} ∪ 761..809`; the remaining 4460 bytes are not decoded.

### `GT2.OVL`: the gzip overlay container

The root directory's `GT2.OVL` (289 752 bytes) is the game's **segment**
container: the part of the program that is not in the boot EXE.

```text
u32 0x30                  table size
per member:  u32 compressed size, then, after all but the last, the next
             member's offset (always align4(previous offset + previous size);
             redundant with the sizes, and checked)
member images             gzip members, 4-byte aligned
2 trailing 00 bytes       after the last member
```

Six members, every one linked at `0x80010000`:

| # | offset | compressed | image | first word |
| --- | --- | --- | --- | --- |
| 0 | `0x30` | 144 709 | 316 920 | `27bdffe0` `addiu sp,sp,-0x20` |
| 1 | `0x23578` | 44 333 | 248 004 | `00a4102a` `slt v0,a1,a0` |
| 2 | `0x2e2a8` | 53 389 | 275 780 | `27bdffe8` `addiu sp,sp,-0x18` |
| 3 | `0x3b338` | 5 195 | 11 500 | `27bdffe8` `addiu sp,sp,-0x18` |
| 4 | `0x3c784` | 38 461 | 273 012 | `27bdffd0` `addiu sp,sp,-0x30` |
| 5 | `0x45dc4` | 3 602 | 8 416 | `27bdffe0` `addiu sp,sp,-0x20` |

The members are **ordinary gzip** and inflate exactly (member 4 was also caught
loading at run time, below). `import overlays <image> --out <dir>` splits the
container, inflates each member, and writes `overlay_NN.bin` plus a
`manifest.json` of offsets, sizes and hashes; `docs/overlays/` holds the six
images for analysis.

**The decompressor is resident EXE code, not part of an overlay.** The boot EXE
contains a DEFLATE decoder — a wrapper `FUN_800847D0` at `0x800847D0` and the
inner Huffman symbol reader `FUN_80084364` at `0x80084364`, 283 instructions —
which walks the raw-deflate block structure (stored / fixed / dynamic Huffman,
`LEN`/`NLEN` and the 19 code-length codes with the 16/17/18 repeats). It reads
the stream through a CD-backed refill callback (`FUN_8007ABB8`, set up by
`FUN_8007ABFC` at `0x8007ABFC`) and writes the result to `0x80010000`.

Evidence: a `--trace` run from the title state along the licence route hit
`0x80084364` at absolute frame 2241 with `a0 = 0x801FFBC8`, input pointer
`0x800E5574`, output pointer `0x80010000`. The RAM at `0x800E54E0` is a gzip
header `1f 8b 08 00 ff aa 69 38 02 03` — file offset `0x3C784` of `GT2.OVL`,
this module's member 4 — and the first bytes the decoder wrote, `d0 ff bd 27`,
are that member's image. So the EXE loader decompresses each overlay from its
gzip member over the EXE's own text region at `0x80010000`.

### Packs: offset table then TIMs

Verified pattern, in three places:

- `/arcade/arc_carlogo` (no extension): `u32 count = 63`, then `count` `u32`
  offsets, each pointing at a `10 00 00 00` TIM (63 = the manufacturers).
- `/crsobj/NAME.trp` and `/bgsobj/NAME.bsp`: a leading `u32` count then TIM
  data from offset 4 (no separate offset table observed).
- `/crsobj/NAME.tro`: the marker `40 28 23 29 47 54 2d 50 53 00 00 00`
  (`"@(#)GT-PS\0\0\0"`) — the first four bytes are the string `"@(#)"`, **not**
  a length — then `u16 0`, `u16 0x1f`, then five file-relative `u32` section
  offsets. Sections are contiguous and the last ends at the file length. The
  layout below is verified on all 126 `/crsobj/*.tro` files; `import tro`
  enforces the structural checks and refuses a file that does not account for
  every byte.

  ```text
  section 0 (road table)
    u32 lap_length_fx16.16    one lap in metres (5241.527 for TC_lisence)
    u16 node_count
    u16 record_count          number of trailing 32-byte records
    u32 record_base           file-relative
    u32 node_offset[node_count]   file-relative; node 0 starts after the table
    node records              variable length, tiled by the offsets
    record[record_count]      32 bytes each; not decoded

  section 1 (mesh records)
    u32 count
    u32 offset[count]         file-relative; records are contiguous
    record: u32 kind; kind * (u32 value; u32 mesh_offset)
            size 4 + 8*kind; kind 1..4 seen (kind 1 is 12 bytes)

  section 2 (mesh blobs)
    u32 count
    u32 offset[count]         file-relative; blob 0 starts after the table
    blob: 0x58-byte header, vertex array, ten primitive arrays
          11 file-relative pointers; first == blob + 0x58
          u16 vertex count at +0x2c, ten u16 counts at +0x30
          s16 min/max bounds at +0x44/+0x4c, scale exponent at +0x54

  section 3 (draw spans)
    u16 count
    u16 level_wrap            wraps levels above it into 1..=level_wrap
    u32 offset[count]         file-relative
    span: u16 kind; u16 min_level; u16 max_level; u16 node;
          s32 start_fx16.16; s32 end_fx16.16; variable body (not decoded)

  section 4 (event points)
    u32 length                equals the section's byte length
    u32 group_count
    u32 offset[group_count]   section-relative; 0 = absent
    group: u32 count; count * 40-byte records
  ```

  Section 0's node-offset table starts at `+12`, not `+16`: the first offset
  equals `section0 + 12 + 4*node_count`, and `record_count * 32` equals
  `section1_offset - record_base` in all 126 files. Section 1's records tile
  their section exactly (`4 + 8*kind` bytes each). Section 2 can hold more
  blobs than section 1 has records (unreferenced blobs are common), but every
  section-1 `mesh_offset` is a section-2 blob start in all 126 files. Section
  4's groups tile the section from the first present group to its end (offsets
  are 4-byte aligned, not necessarily 8). The first pointer in every section-2
  blob is `blob + 0x58`.

  Section-2 meshes now validate down to vertices and polygon indices in all
  126 courses: 5,709 meshes, 280,301 vertices and 145,661 primitives. The
  8-byte vertex array and ten primitive arrays tile every mesh exactly;
  polygon indices are packed as three 10-bit fields plus a fourth in the
  next word for quads. Mesh bounds contain every vertex. See
  [course mesh evidence](course-mesh-evidence.md) for strides, renderer
  addresses, the runtime read watch and the corpus test. Scale conversion,
  placements and materials still need decoding before engine asset output.

`TC_lisence.tro` (the B-1 licence course) is 216 984 bytes with section starts
`0x38c`, `0x2fca0`, `0x2fd74`, `0x32d04`, `0x34ea4`; section 0 has 124 nodes
and 403 32-byte records at `0x2ca40`, sections 1 and 2 have 13 records/blobs,
section 3 has 37 spans with `level_wrap = 3`, and section 4 is 244 bytes with
one present group (5 records). Each node starts

```text
u16 linkA; u16 linkB; u32 addrA; u32 addrB; u32 flags;
s32 distance_fx16.16; s16 scale; s16 scale;
s32 x; s32 y; s32 z; s16 dirx; s16 diry; s16 dirz   (42 bytes, then a body)
```

The link addresses are zero in the file and are filled from the link indices by
the loader. `distance` is cumulative along the course in fx16.16 metres; the
point and direction are 16.16/4096 fixed and use `(x, y, -z)` in the file, i.e.
the engine negates z. The two `s16` values are in `-4096..=4096` (4096 = 1.0);
they weight the far and near boundary-plane distance projections, respectively
(`80028614..710`). Following `linkB` (`+2`) from node 0 visits every
node once and returns, so it is driving order; `linkA` is the reverse
neighbour. This is the table the car's course-distance solver walks
(`docs/engine-model.md`, "World").

Each node body carries two 4-corner `(x, z)` 16.16 quads. The second is the
road surface on `TC_lisence`; each corner is 8 bytes (`x` then `z`):

```text
+0x6c x, +0x70 z    near corner 0   this node's cross-section
+0x74 x, +0x78 z    near corner 1
+0x7c x, +0x80 z    far corner 0    the next node's cross-section
+0x84 x, +0x88 z    far corner 1
+0x8c               metadata
+0x90               tag
+0x92               count (15 in node 78)
+0x94               count file-relative offsets into primitive/vertex lists
```

The file stores no height for the corners: the importer uses the node's own `y`
for the near pair and the next node's for the far pair, then negates `z` like
the other points. On `TC_lisence` the near-edge midpoint is the node point
(within 0.49 m) and the quad is 19.45–22.58 m wide; adjacent quads do not share
an edge exactly (up to 2.0 m), so each is kept as stored. The first quad at
`+0x44` (same 8-byte corner layout) is undecoded. The lists use 8-byte
vertex/UV records and 12-byte primitives ending in PS1 GPU colour and command
bytes (`0x24`/`0x28`/`0x2c`); decoding the scenery mesh is still open.
`.trp`/`.bsp` are the companion TIM packs.

`import tro <file.tro> --out <course.ron> [--name <disc path>]` writes the
engine's `CourseData` (metres, including each node's road-surface quad) plus
provenance, and reports the section and record counts it checked.

### Sound

The engine samples (`/engine/**/*.es`) are decoded: see the `ENGN` section
above, and `import engines` / `import engen`. The full set is baked —
`import engines` writes the **467 distinct samples** (1148 s, 44 100 Hz) of the
volume's 2747 `ENGN` resources as `game/assets/gt2/sound/sample_NNNN.wav` plus
`table.ron` (`engine::sound::SoundTable`); the numbered `_n*`/`_t*` slots that
repeat one placeholder share a file. `/sound/*.seq` begin `53 45 51 47`
(`SEQG`); `/sound/*.ins` begin `49 4e 53 54` (`INST`). Both are PS1 sound
formats, not decoded. `SEEQ`/`INST` are the standard PS1 sequencer and
instrument banks, but the tag bytes here are `SEQG`/`INST`, so verify the exact
variant before converting.

### Text

`/.text/data-race.txd` is a string table in the clear (`"Lap\0\0\0\0Total
Time\0…"`), as is `/arcade/license_info_uk` (English licence text, raw). The
UTF-16 `WSDB` above is the third text container. Which one the HUD uses is not
yet established.

### Licence and dirt: a shared leading table

`/license/NAME.lgf` and `/dirt/NAME.dgf` (both gzipped) begin with a small byte
table followed by a NUL-padded name, then data:

```text
a_lia00.lgf  00 00 02 01 01 02 00 01 02 02 03 01 00 01 00 02 "LIA00" ... "L_circle80"
```

`LIA00` / `L_circle80` are internal asset names, so these files carry their own
labels. Not decoded.

## What this means for the engine

- The engine reads **engine-native** assets only: PNG (textures), WAV (engine
  and effect samples), glTF or a documented mesh format (cars, course), RON
  (tables, text). `GTFS`, gzip, TIM, `GT\x02`, `GTDT`, `ENGN` and the rest are
  `tools/import` concerns and must not appear in `engine/` or `game/`.
- `WSDB` is now an engine asset: `import text` → `engine::text::TextTable`
  (RON), baked per language under `game/assets/gt2/text/`. Textures are baked as
  PNG under `game/assets/gt2/textures/` (3707 files). The engine sounds are a
  third: `import engines` → `engine::sound::SoundTable` (RON) plus WAVs under
  `game/assets/gt2/sound/`.
- `GTDT` is data with a known container but unknown block schemas; the
  `.seq`/`.ins` sound is data with an obvious engine-side counterpart (a
  sequencer).
- The pack/index shapes (`.idx` + `.dat`, `trp`/`bsp`, `arc_carlogo`) are
  containers; the engine's asset layer should be able to load a set of assets
  from a group, but the *conversion* happens offline so the runtime never reads
  a `GTFS` volume.

## Native polygon geometry and refinement

`engine::geometry::GeometryData` is RON containing material batches in
`parts`. Each part has a name, optional ordinary image path, blend/side mode,
metre positions, normalized UVs, sRGB modulation colours and triangle indices.
It now also accepts `quads` in perimeter order, with four indices each.
Attribute arrays must have equal length; all indices and numeric values are
validated. Existing triangle assets load with an empty default quad list.

Optional `precision` data specifies the coordinate grid in metres and the
bound/distances that control extra precision bits. Optional `subdivision`
specifies maximum twice-projected area in target pixels squared, depth limit,
UV grid step and conservative guard band. Optional `small_texture` supplies
an image path, blend, aligned UV array and maximum twice-area threshold.
Refinement and small textures require an explicit precision profile.
The view-dependent renderer selects textures on source triangles/quads, then
refines quads before affine projection and near-plane clipping;
it does not interpret original GPU words. See
[subdivision evidence](subdivision-evidence.md) for conversion and verified
rules and [texture detail evidence](material-variant-evidence.md) for selection.
`engine/assets/sandbox/panel.ron` is a complete handmade example using
this format. Other primitive-group refinements and the editor remain work.

## Vehicle rules and body assets

Vehicle rules load layered tuning from `vehicle/pedals.ron`,
`vehicle/steering.ron`, `vehicle/slip.ron`, `vehicle/braking.ron`,
`vehicle/brake_assist.ron`, `vehicle/longitudinal_slip.ron` and
`vehicle/engine.ron` and `vehicle/tyres.ron`. A mod can replace these
files or insert their public tuning resources before adding the plugins.
Pedal rates are fractions of full travel per 30 Hz tick. Steering uses
turns, turns/second, a speed curve in km/h, spring coefficients, per-wheel
ratios and front/rear toe. Engine speed uses revolutions/second and RPM;
torque values retain an explicitly documented internal unit until the
downstream force/inertia conversion is established. These are ordinary
editable RON structures; they contain no instruction or memory images.

`VehicleRulesPlugin` accepts explicit inputs and state components. Dynamics
supplies forward speed and slip to steering, engine speed/last RPM/effective
throttle to torque, and world velocity/yaw rate to the step builder.
`VehicleStepDuration` is in seconds; its original value is slightly below
1/30 second. The host still schedules one logic tick every two NTSC fields.
The named `VehicleSystems` sets preserve preparation → controls → wheel
commands → forces → step construction → integration, all before the engine's
camera phase. No complete tyre/drivetrain model is implied by these interfaces.

`LongitudinalSlipRulesPlugin` reads wheel surface speed, contact velocities
and wheel steering, producing signed slip, a velocity-reference enum and
prospective force direction. Its optional assistance connection supplies fresh
slip before brake adjustment. Tuning specifies stopped-wheel and full-response
speeds in m/s and a low-speed response slope. See
[longitudinal slip evidence](longitudinal-slip-evidence.md).

`BrakeAssistRulesPlugin` opts cars with `BrakeAssistInput` into per-wheel
slip/yaw adjustment after shared-pedal copying and before demand allocation.
Its tuning contains axle slip targets, slip/grip feedback gains and yaw
strength. Its state supplies wheel speed/grip/slip responses and yaw correction
signals; those producers remain work. All response units are dimensionless,
with quantized product boundaries. See [assistance evidence](brake-assist-evidence.md).

`TyreForcesRulesPlugin` consumes the two slip components, wheel motion,
steering, contact velocities and `TyreContactState`. It exposes paired tyre
responses, separate longitudinal solver channels, absolute cornering response
and final lateral force. `TyreTuning` has two axle curves with shared slip
coordinates and longitudinal/cornering-limit ordinates, folded slip-angle
curves, and named low-speed correction settings. Force capacity and outputs
retain explicit original internal units; correction velocity thresholds are
m/s. Source tables are exported locally. See
[tyre-force evidence](tyre-force-evidence.md).

`ForwardDynamicsRulesPlugin` consumes tyre forces, steering, body-forward
speed from `TyreContactState`, per-car duration and `ForwardDynamicsInput`
(wheel load, surface indices and prepared aerodynamic force). It produces
`ForwardDynamics`: surface resistance, effective reciprocal mass, accumulated
force, acceleration and body-forward speed before drivetrain coupling.
`ForwardDynamicsTuning` names body mass/reciprocal, per-axle rotational mass,
surface coefficients and a clamped speed/response curve. Mass is in normalized
original units, without a claimed kilogram conversion; forces retain original
internal units. Tuning is exported locally to `vehicle/forward_dynamics.ron`.
This intermediate output does not yet supply world `VehicleVelocity`. See
[forward dynamics evidence](forward-dynamics-evidence.md).

`VehiclePresentationPlugin` renders an ordinary `GeometryData` body asset
on a subject marked `VehicleBody`, as a child under that subject's metre
Transform. Model-local X is right, Y up and -Z forward. Its public resource
`VehiclePresentation` selects body and optional wheel-layout paths, or
disables the body with `geometry: None`; it only loads geometry for opted-in
entities. The B-1 body is at
`gt2/vehicle/t2vzn.ron`. The example mod supplies a handmade asset at the
same path. A `WheelLayout` names its `body_asset` and a list of wheel names,
geometry paths, metre positions and unit quaternion rotations. The renderer
creates ordinary child entities marked `VehicleWheel`, exposing subject
and wheel index for future animation. The current B-1 layout uses static
checkpoint steering/camber/spin/suspension poses and mirrored right-side
meshes, resolved offline. The example mod authors its own layout and wheels.
There are no required animation clips for this contract; live wheel motion
and reflection rendering remain work.

## Next

1. TIM decoder → PNG: **done** (`import tim` / `import tims`; textures baked).
2. `WSDB` text → RON: **done** (`import text`; `engine::text::TextTable`).
3. `GTDT` block schemas → tables: the container is known, the meaning of each
   block needs the code that reads it.
4. `.cdo` geometry and `.tro` course objects → meshes.
5. `ENGN` → WAV: **done** (`import engen` / `import engines`;
   `engine::sound::SoundTable`). `.seq`/`.ins` → the engine's music/sfx.
6. Menu archives: containers **decoded** (`commonpic` = 458 `GTMP` chunks;
   `gtmenudat` = 3386 gzip `GM` blobs). The `GT2.OVL` decoder is no longer the
   blocker — the six overlays are extracted (`docs/overlays/`) — so the
   remaining work is reading the per-screen loaders inside them. The `.tro`/
   `.cdo` geometry loaders are in the same place.


## Course collision grids and native format

The node +9C/+A0 collision grid, cell lists and polygon metadata are now
decoded across all 126 courses. [Road contact evidence](road-contact-evidence.md)
records source offsets, counts, sampling and runtime comparisons.

`CourseNode::contact` is an optional engine `ContactMesh`: a local metre
origin, horizontal grid origin/cell sizes/dimensions, ordered polygon-index
lists and triangle/quad metre vertices with surface/variant/flag metadata.
`horizontal_step_m` expresses containment/quad-centre precision;
`height_step_m` expresses the signed height interpolation boundary. There are
no file pointers, packed primitive words or GPU commands at runtime.
`contact_search_origin_m` retains the exact centreline point for edge ties.

Author those fields directly in RON, validate with `ContactMesh::validate`,
and use `sample` or `CourseData::road_contact`. A quad's vertices are clockwise
in the horizontal (x,z) plane; its height uses a centre fan. Cell order is
significant for overlapping polygons. `engine/assets/sandbox/contact.ron`
is a complete hand-authored flat floor/ramp example, used by the sandbox's
ordinary walking controller and rendered at the same vertices.

## Native bitmap fonts

`engine::bitmap_font::BitmapFont` holds a native image path, Unicode-character
glyph map and pair-spacing adjustments. Rectangles are `(x, y, width, height)`
in image pixels. Bearings are pixels right/down from a text baseline, and
advances/kerning are pixels. Zero-size rectangles allow spaces to advance
without drawing. Missing characters are reported as errors. No original font
tables or texture-page conventions appear in this format.

An independently authored example:

```ron
(
    image: "my-font.png",
    glyphs: {
        'A': (rect: (0, 0, 5, 7), bearing_px: (-1.0, -7.0), advance_px: 6.0),
        'V': (rect: (5, 0, 5, 7), bearing_px: (0.0, -7.0), advance_px: 6.0),
        ' ': (rect: (0, 0, 0, 0), bearing_px: (0.0, 0.0), advance_px: 3.0),
    },
    kerning: [(pair: ('A', 'V'), adjustment_px: -2.0)],
)
```

Optional `cell_inset_px` on each glyph supports fixed-width numeric cells.
`BitmapFont::layout` returns image rectangles, positioned glyphs and text width;
a renderer uses ordinary sprites. The [B-1 HUD converter](hud-evidence.md)
currently exports printable ASCII from two original fonts into this format.
Other games/mods can supply their own image and metrics directly. A native
font editor remains future work; RON and an image editor already author the
format without any original-game data.

`BitmapFont::layout_number` accepts `NumberCells` (digit, punctuation, decimal
inset and origin-inset pixels) for right-aligned fixed cells and narrower
spacing on both sides of `:`/`.`. The ordinary glyph bearings and cell insets
still apply; missing characters return errors. The lap clock and licence
records share this implementation.

## Driving-gauge content

The game's `race/gauges.ron` deserializes to public `GaugeSettings`. It holds
an ordinary RGBA atlas path and pixel rectangles, centre/offset/colour values,
maximum and redline RPM, division count, sweep/zero angles in turns, radii in
pixels, and needle-strip distances, half-widths and edge colours. Sprite slots
are dial, digits 0–9, units, reverse, then forward gears. Maximum RPM and
division count must be positive; all referenced sprite slots must exist.
Coordinates are right/down screen pixels relative to a lower-right anchor.
An image editor and RON can author replacement artwork/layout without disc
data. Mark one vehicle `GaugeSubject`; its engine, contact-motion and
transmission components supply the displayed state. See the complete
[gauge contract and evidence](gauge-evidence.md).

## Licence definitions and record panels

`race/licence.ron` is a public `LicenceDefinition`: an authored string id and
four millisecond time limits. The records panel shows the first three in
gold/silver/bronze order. `LicenceRecords.best` maps ids to `LicenceRecord`
values (millisecond time and speed readout in hundredths of the selected
display unit); a missing key means no record. `race/records.initial.ron`
initializes this resource, independently from any later persistent save.

`RecordsSettings` in `race/records-panel.ron` holds the font path, labels,
pixel offsets, marker sizes/colours, underline gradient and numeric spacing.
It uses the same native font and ordinary 2D sprites/meshes. New content can
author all these files directly in RON with its own font. See the public
[records API and evidence](records-evidence.md).

`CourseData::distance_at_position` projects a world-space point to cumulative
course metres. Node/lap distances use `f64`; the optional `DistanceSampling`
contains projection-step metres and an interpolation-fraction step. Omit it
for continuous authored geometry. Imported course RON supplies the source's
sampling grid, while rendering still uses ordinary `f32` geometry.

`race/licence-stop.ron` is `LicenceStopSettings { start_m, end_m,
stop_speed_mps }`. `LicenceStopInput` accepts four projected wheel distances,
world velocity, surface ids, explicit off-road/collision faults and a time
origin. `LicenceRun` is an active, passed-with-time or failed-with-reason
component. `LicenceParticipant` opts a native car into deriving those inputs
from the decoded course and road contact. See [stopping rules](licence-stop-evidence.md).
