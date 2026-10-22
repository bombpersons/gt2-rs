# Engine model

What the original treats as **data** (the engine interprets it) and what as
**code** (per-type controllers in `game/`), worked out from evidence before
writing gameplay code. Keep it current: it defines the engine's scope.
For each answer, cite the evidence (addresses, tables, traces, `psx` runs).

Status of the investigation:

| Question | Status |
|---|---|
| Disc, volumes and the data container | **established** (below) |
| Boot and the segment/overlay model | **established** (below) |
| Frame and tick rate | **established** (60 Hz, one update per vblank in menus) |
| Data formats inside the volume | partial (`docs/formats.md`: transport, TIM, `WSDB` and `ENGN` decoded and imported; `.tro` container, road table, road-surface quads and draw spans decoded, section-2 mesh storage and indices validated, placement and materials open; `GTDT` blocks and `.cdo` not decoded) |
| World and collision | partial (the course file, its loaded image, the road-node table, the distance solver, the draw-span selection and the road-surface quads are decoded; scenery mesh geometry and collision beyond the road table are open) |
| Movement | partial (world position, heading, course distance and node are located and explained; the steering/physics deltas that feed them are not yet decoded) |
| Objects and the type table | provisional (path-keyed asset library, per-frame function-pointer tables found) |
| Level scripting | not yet established |
| Sound | partial (engine samples `ENGN` decoded to WAV + a `SoundTable`; the per-RPM zone table and the `.seq`/`.ins` music are not decoded, and no voice playback is wired) |

Evidence addresses are main-RAM (`0x80000000` onwards). A run directory named
below is under `.psx/runs/` (git-ignored; regenerated from the disc with the
recorded commands in `docs/evidence.md`).

## Disc and volumes

The disc is a single data track, mode-2 raw, volume `GRANTURISMO2`, boot
`cdrom:\SCUS_944.88;1` (`psx disc`). Six files, from `import isofs` and
confirmed against the table the original builds in RAM at `0x80009224`
(entry = `[lba u32][size u32][name 16 bytes]`, 24 bytes):

| File | LBA | Bytes | Role |
|---|---|---|---|
| `SYSTEM.CNF` | 23 | 68 | boot config |
| `SCUS_944.88` | 24 | 628 736 | boot executable (PS-X EXE, entry `0x8005D600`) |
| `GT2.OVL` | 331 | 289 752 | overlay container (6 offset/size pairs of compressed blobs) |
| `GT2.VOL` | 473 | 488 241 152 | the main volume: game data |
| `MUSIC.DAT` | 238 872 | 85 262 336 | streamed XA-ADPCM music |
| `FAULTY.PSX` | 280 504 | 27 648 000 | 27 MB, first bytes zero |

`FAULTY.PSX` and the `.OVL` blob codec are unexamined and may be red herrings
or region/QA leftovers; recorded so they are not lost.

### `GTFS` (inside `GT2.VOL`): the filesystem

The volume starts with a descriptor the original copies to RAM at `0x801e35f0`
(its first `0x66b22` bytes; verified in a RAM dump):

```text
0x000  "GTFS"          magic
0x004  u32             0
0x008  u64             tag: 0x0000_0000_2d64_2d3d observed
0x010  u32 entries[N]  one per resource: sector and padding (below)
0x...  u32 sentinel    equal to the volume's size
0x...  zero padding
0xB800 records[]       32-byte directory records (resource 1)
0x...  data            resources, sector-aligned from 0x66800
```

`N` is **11580**, found by reading the table until an entry equals the volume's
byte size (`0x1d19f800`): the sentinel is entry 11580. An entry packs two
fields, and the loader masks the table's contents exactly this way:

```text
entry >> 11     sector: the resource's data starts at entry & !0x7ff
entry & 0x7ff   pad: bytes of padding after the data in its last sector
```

So resource `id`'s data is the **sector-aligned** span
`[entry[id] & !0x7ff, entry[id+1] & !0x7ff)` — as a CD read must be — and its
true length is that span minus this entry's padding:

- `fn_8005D74C(id)` → `entry[id] >> 11`: the **sector** to read;
- `fn_8005D79C(id)` → `(entry[id+1] & !0x7ff) - entry[id]`: the **length**;
- `fn_8005D848(id, buffer, cb)` → calls `cb(buffer, sector, length)`.

Verified three ways on the disc: **all 3576** `/…*.tim` resources begin with the
TIM magic `10 00 00 00` at their sector start, **all 5114** `.gz` with `1f 8b`
and **all 2747** `/engine/*.es` with `ENGN` — never at `entry[id]` itself. All
5114 gzip streams decode with a clean end-of-stream, and for every one of them
the span minus the entry's low bits is exactly the gzip stream's length
(222 813 744 bytes decompressed from 80 693 248). `tools/import` (`import
volume`, `import extract`) implements this and validates that lengths are never
negative; `import extract --path /sound/sys.ins --out …` writes a resource the
same way the original reads it.

Entry 0 is the volume header itself and entry 1 the directory records, so the
first data resource is id 2; six ids are zero-length (`/replay/scea.000`,
`scea.999`, `scee.*`, `scei.*` — replay slots this revision leaves empty).

Each directory record is 32 bytes:

```text
0x00  u32 key        per-record key (0x385f8dcf for every "..")
0x04  u16 ordinal    record index (from the table's start) of the node
0x06  u8  flags      bitfield: 0x01 a directory node starts at ordinal,
                     0x80 the last record of this directory
0x07  char name[25]  NUL-padded ASCII
```

Record 0 is the root node's first entry and the run of records that makes up
resource 1 is the whole tree: 11620 records from `0xB800`, resource 1 spanning
`[0xB800, 0x66480)`. The root node has 28 records: 12 files at the volume root
(`/.carcolor`, `/.usedcar`, …) and 16 directories.

**This is a path-keyed file system, not a numeric namespace.** A directory
record's `ordinal` points at its child node, whose records run until the one
with the `0x80` bit; a file record's `ordinal` **is** its resource id. To open a
path: match the first component in the root node, descend, match the last
component. Corroboration: the boot executable carries a table of 248 `char*`
paths it loads (at `0x8009118c`, strings at `0x8008e024`, normalised as
`/.carcolor` … `/sound/sys.ins`); 228 resolve through this tree, and the 20 that
do not are directory-only entries (`/bgsobj`, `/carwheel`) or language variants
the disc does not carry (`/carparam/fra_arcade_data.dat.gz` is listed but only
`eng_`/`usa_` exist). 11578 files resolve in total.

The verified end-to-end check: resource 62 is `/arcade/license_info_uk` and its
bytes are English text (`" but you go faster and\0need to start braking
sooner. …"`), and 63 `/arcade/license_info_us` likewise.

The compressed resources carry their original name and mtime in their gzip
header (`eng_gtmode_data.dat`, and a 1998 timestamp), which is a second
independent check on the names.

### Content: what the directories hold

Every top-level directory, its file count, and its role (names are the
original's):

| Path | Files | Contents |
|---|---|---|
| `/engine/` | 2747 | `NNNNN.es` with `_n0.._n3`/`_t0.._t3` parts: 305 numbered assets × 9; `ENGN` SPU-ADPCM engine sounds (decoded, 467 distinct payloads) |
| `/carobj/` | 4440 | car models: `NAME.cdo.gz`, `.cdp.gz`, `.cno.gz`, `.cnp.gz` (compressed) |
| `/carlogo/` | 3346 | `NAME--.tim`: car-logo textures |
| `/carwheel/` | 192 | `NAME.tim`: wheel textures |
| `/carparam/` | 29 | `*_data.dat.gz` / `*_unistrdb.dat.gz`: car and menu parameter tables |
| `/crsmap/` | 120 | `NAME.tim.gz`: course map textures |
| `/crsobj/` | 252 | `NAME.tro.gz`, `.trp.gz`: course objects |
| `/dirt/` | 81 | dirt/dust sprite textures |
| `/bgsobj/` | 68 | `NAME.bso.gz`/`.bsp.gz`: sky and background objects |
| `/arcade/` | 66 | `arc_font.tim`, `arc_carlogo`, `demofile*.gmr.gz`, `license_info_*`, arcade-mode UI |
| `/license/` | 180 | `NAME.lgf.gz`: licence-test data |
| `/gtmenu/` + `fra ger ita spa usa` | 22 | GT-mode menu data, per language (`gtmenudat.dat`/`.idx` = 3386 gzip `GM` blobs, `solodata.dat.gz`, `iconimg.dat.gz`); `commonpic.dat`/`.idx` = 458 `GTMP` chunks |
| `/replay/` | 6 | `scex.000`/`.999`: replay slots, all zero-length in this revision |
| `/sound/` | 15 | `NAME.seq` (SPU sequences), `NAME.ins` (instruments) |
| `/font/`, `/.text/`, root | 14 | race font, `data-race.txd`, root tables |

So the volume is a **whole asset library keyed by path**: 222.8 MB of gzip
inside it, plus raw TIM and the numbered `.es` families. The `.gz`, `.cdo.gz`,
`.tro.gz`, `.lgf.gz` payloads decompress as plain gzip and are the next codecs
to read; which of these the engine interprets as data versus which the game
hardcodes per type is the question the controllers answer.

What is inside each family — the gzip transport, TIM, `ENGN`, the `GTDT` and
`WSDB` tables, the `.tro`/`.cdo` geometry, the indexed menus (containers
decoded: 458 `GTMP` chunks, 3386 gzip `GM` blobs; entry payloads need the
overlay decoder), and which directory holds which true extension — is
`docs/formats.md`.

## Boot and the segment model

`SCUS_944.88` is a PS-X EXE linked against **PsyQ 4.70** (`psx symbols` names
85 library functions across `LIBAPI`, `LIBC`, `LIBCARD`, `LIBCD`, `LIBETC`,
`LIBGTE`, `LIBPAD`, `LIBPRESS`, `LIBSN`). `LIBGTE` presence means the original
uses the GTE for geometry.

Boot flow, from disassembly:

- entry `0x8005D600` saves `a0/a1` into `0x80091144..`, zeroes `0x801C93B0..0x801F0D60`, calls `0x8008CE08` (libc init), then
- `0x8005D6E0` (main): `FUN_80085a98()` (SDK/init), `FUN_80010e14()`, `FUN_8005d9f0()`, `FUN_8005da3c(1)`, return 0.
- `0x8005DA3C(1)` calls `0x8005DA7C(1, PTR_DAT_80091174[1], 0,0,0,0)`: the **exec** — it installs a handler via `FUN_8007ad90(0x801c942c, entry)`, sets up `FUN_80078370`/`FUN_800783dc`, picks a base (`DAT_800a8d5c` + an entry from a table, or `0x801ef618 + *piVar2`) and jumps to `FUN_80082fac(base, FUN_80010000)`.

So the game is a small boot executable plus **segments/overlays** launched by
an exec table at `0x801c942c` with entries in `0x80091174`; the bulk of the
game code is not in the EXE. This matters for the engine split: the port does
not need an overlay loader (the port is one image), but the *sequencing*
(boot segment → mode segments: title, GT Mode, race) is the original's
top-level state machine and should be a schedule/state layer in `game/`.

`GT2.OVL` (root directory, 289 752 bytes) holds the segments: a 0x30-byte table
followed by one **gzip member per overlay**, each member starting at a 4-byte
boundary. The table is `u32 0x30`, then per member its compressed size with the
next member's offset stored redundantly after it (always
`align4(offset + size)`; two trailing zero bytes end the file). Six members,
1 133 632 bytes of image in total, all linked at `0x80010000`.

At run time the boot EXE reads the CD sectors covering a member into RAM and
decompresses it with a **resident DEFLATE decoder** in the EXE: `FUN_800847D0`
(entry `0x800847D0`; the inner Huffman symbol reader is `FUN_80084364`,
`0x80084364`), which walks the raw-deflate block structure (stored / fixed /
dynamic Huffman) and writes to `0x80010000`. Observed at frame 2241 of the
title → licence route: the CD window at `0x800E54E0` held the member at file
offset `0x3C784` beginning `1f 8b 08 00 ff aa 69 38 02 03`, and the decoder's
first output byte at `0x80010000` was `0xD0` (`addiu sp, sp, -0x30`) — the
image `import overlays` writes as `overlay_04.bin`.

So the earlier reading (a gzip-shaped header that would not decode) was wrong:
the members are ordinary gzip and inflate exactly; `import overlays` splits,
inflates and accounts for every byte, and writes the images with a manifest
under `docs/overlays/`. This is the code that was missing: each overlay can now
be read as MIPS at `0x80010000` (the port still needs none of it, since the port
is one image, but the per-mode loaders live here).

The members identify themselves by their own length-prefixed name strings:
0 = **race** (`15RaceDevelopment`, `12RaceViewLoop`), 1 = **title / main menu**
(`8BaseMenu`, `data-title.txd`), 2 = **arcade** (`10ArcadeMenu`,
`data-arcade.txd`), 4 = **GT Mode menu** (`13GTLoadingMenu`, `6GTMenu`,
`data-gt.txd`), 5 = **movie loop** (`12psxMovieLoop`); 3 carries no strings.
That matches the boot state machine: the boot EXE picks one mode segment per
screen. `docs/overlays/README.md` has the buffer layout the GT Mode segment
sets up for its streaming pictures.

## Frame

`VSyncCallback` (`0x8008BD08`) is called once from `fn_8007D0A4`, installing
the game's vblank handler `0x8007D294`. That handler increments two counters
and calls `0x80083134`:

```asm
8007D294  lui   a0, 0x801F
8007D298  addiu a0, a0, 1664      ; a0 = 0x801F0680
8007D2A4  lw    v0, 0(a0)
8007D2A8  lw    v1, 4(a0)
8007D2AC  addiu v0, v0, 1
8007D2B0  addiu v1, v1, 1
8007D2B4  sw    v0, 0(a0)        ; 0x801F0680 += 1 every vblank
8007D2B8  sw    v1, 4(a0)        ; 0x801F0684 += 1 every vblank
```

Sampled from the title screen for 120 frames
(`run … --sample 801F0680:u32 801F0684:u32`): `0x801F0680` goes 1017 → 1137,
delta exactly **1 per vblank**, unique delta set `{1}`; `0x801F0684` stays 0.
NTSC-U, so the counter is a 60 Hz frame counter.

One game-side per-frame handler is provably called once per vblank. Sampled
over the same 120 frames, two of the game's own counters advance exactly 1 per
vblank (deltas `{1}`, no other step):

- `0x80092E80` (written by `fn_8007916C`), 1018 → 1138;
- `0x800B1228`, 323 → 443.

`0x800B1228`'s only incrementing writer is `fn_80017984`, which does
`lw`/`addiu +1`/`sw` and clamps at 901 (`slti v0, v0, 901` — a 15-second, 900-
frame limit), so a per-frame state handler ran exactly once per vblank. Neither
counter is written by the vblank interrupt (that handler only touches
`0x801F0680`/`0x801F0684`), and `0x801C9524`/`0x801C9528` advance by a
variable amount per frame (accumulators, not tick counters), so the once-per-
vblank step is the game's own, not a copy of the graphics counter.

Two other candidates change by more than 1 or reset: `0x8004BC48`/`0x8004BC4C`
(plus 1 most frames with a −60 reset — a periodic timer) and `0x801F0694`
(+2 per frame).

`FIELDS_PER_TICK = 1` is therefore supported for the title/menu modes: the
original's update step is one per vblank.

Confirmed inside a race (B-1 license test, psx 0.3.2): the vblank counter
`0x801F0680` and the per-frame handler counter `0x80092E80` still advance
exactly +1 per frame over 900 frames, so presentation and the frame handler
stay 60 Hz; the menu counter `0x800B1228` is not maintained in race mode. The
race adds its own counters: `0x800A8C64` (+1 per frame, 60 Hz) and
`0x800A8D70` (+1.0 fx16.16 per frame). The **car simulation steps at 30 Hz**:
the fields of the car struct (below) are stored on every second vblank, while
other fields of the same struct (e.g. the presentation RPM at `car+0x6F8`) update
every vblank — the original mixes cadences inside one object.

## Car state (race mode)

The player car lives in a struct at **`0x800A96B4`** (main RAM), found from
its writer: `0x8003CE3C` is the car's per-tick function, called with the struct
in `a0`. Verified fields and behaviour:

- `car+0x604`: **course distance travelled, metres, fx16.16**. Recomputed
  every 30 Hz tick by `0x8003CE9C` from `car+0x600` and `car+0x65C/0x660/0x664`
  (each ×16) through `0x80028C6C`. Its per-tick delta tracks the dash speed
  (0.694/tick ≈ 20.8 m/s at 46 mph; integrated segments match the mph curve
  within ~3%). Only the car tick itself reads it (engine/tyre sound at
  `0x8003C70C` and the shift logic).
- `car+0x6A4`, `car+0x6A8`: forward/lateral velocity components, 4096 units
  per metre/second. `0x8003CE3C` compares their sum of squared values against
  thresholds whose purpose remains open. The earlier RPM-band reading was
  wrong; see [driving evidence](driving-evidence.md).
- `car+0x45D`: control-mode selector (0 selects live input, 2 another path);
  its full enum remains open. It is not established as a gear number.
- `car+0x624`: engine speed in revolutions/second ×4096. `car+0x6AC` is
  calculated integer RPM in a signed halfword, written by `0x80039458`
  after drivetrain integration. `+0x6AE` is a separate RPM-related halfword
  and `+0x6F8` a presentation copy. Treating the word at `+0x6AC` as a
  16.16 RPM value was wrong; [dynamics evidence](vehicle-dynamics-evidence.md)
  records the correction and the verified engine torque/limiter boundary.
- `car+0x478..0x5D8`: repeating sub-records (four similar groups) whose
  numeric fields move a few counts per tick — wheel/tyre per-corner state by
  the shape; not yet decoded.
- `car+0x65C`, `car+0x660`, `car+0x664`: **the car's world position**, three
  s32 values in 20.12 fixed metres, stored as `(x, z, y)`. Over 240 frames of
  the B-1 straight (`.psx/runs/car-traj2/samples.csv`) `car+0x65C` falls
  401.44 → 295.84 m while `car+0x604` rises 190.21 → 295.81 m and
  `car+0x660` (z) stays 449.69 m — i.e. the car travels 105.6 m along −x and
  the two agree per frame to rounding. The earlier "local offset" reading of
  these fields was a **unit error** in the sampler: the CSV format `fx16.16`
  divides by 65536, but the fields are 20.12, so the metre value is the CSV
  value × 16 (equivalently raw / 4096).
- `car+0x648`: heading, u16, **4096 units per turn** (`0x400` = 90°); the
  integrate function `0x80033E6C` adds a delta and wraps it into
  [−0x1000, 0x1000). Held at 0x400 on the B-1 straight.
- `car+0x600`: current **road-node index** into the loaded course's section 0
  (78 → 80 over the same 240 frames). `car+0x604` is derived from it and the
  position; see "World".
- `car+0x650`: raw `0x014EFE9A` at the anchor — two u16 (`334`, `−358`);
  plausibly a course/slip angle pair, not interpreted.

The dash-speed calculation is still open. A failed scalar search did not
establish how it is computed; world velocity fields have now been found.
The world-position delta provides an independent speed measurement: 105.6 m in 4.0 s ≈ 26.4 m/s, the same order as the 55 mph
(24.6 m/s) dash. The physics that produce the per-tick position and heading
deltas (`0x80033E6C` inputs) are the remaining movement investigation.

## Update dispatch

The game's per-frame update functions are reached through tables of function
pointers in the executable's data, which is the shape the engine's object/state
registries should mirror:

- `0x8004BC50` holds code pointers, including `0x80017984` (the frame counter
  above) and `0x8001792C`, `0x80017C00`, `0x8001779C`;
- `0x8004BC5C..0x8004BC80` holds seven data pointers at stride `0x48`
  (`0x8004BA08`, `0x8004BA50`, …, `0x8004BBB8`);
- `0x8004BA08..0x8004BC00` is then 7 records × 72 bytes, each six 12-byte
  entries of the form `[f32][u16][u16][u32]` (42 entries, e.g.
  `3e311800 0016 0044 0000000c`).

The records' fields read as geometry/layout data (a float with two small
integers and a small tag); what they describe — screen elements, icons or
track pieces — is not yet established, so this is recorded as the strongest
hint that the original drives per-frame work from a type/state table rather
than falling through a `match` on a mode number.


## World

**Partial.** A race loads exactly one course `.tro` file and keeps it, whole
and relocated, in a course container in main RAM. For the B-1 licence test the
container is at `0x800A9500`; the loaded image is at
`container + 0xB534 = 0x800B4A34`, and the distance code reads its section-0
pointer back from `image + 0x10` (`0x80028C9C`: add `0xB534`, `lw 16(a0)`).

The file is **`/crsobj/TC_lisence.tro`** (resource 8322, gzip, LBA 42455 — the
in-race read span recorded in `docs/evidence.md`). Its section-0 header
`fa867914 7c009301` and the node-78 record are byte-identical to RAM; the
`.tro` files that share the same 12-byte marker, including `test_20`, do not
match. An earlier reading compared only the marker and named `test_20`; that
was wrong.

### `.tro` container

```text
0x00  char[12]  "@(#)GT-PS\0\0\0"   marker, not a length
0x0c  u16       0
0x0e  u16       0x1f                 format tag (31 in every file seen)
0x10  u32[5]    section offsets, file-relative; the loader adds the image
                base to each and stores the result in RAM
```

Sections are contiguous and the last ends at the file length. Their headers
differ by section (verified on all 126 `/crsobj/*.tro` files; `import tro`
checks each one):

- **section 0 (road table)**: `u32 lap_length_fx16.16; u16 node_count;
  u16 record_count; u32 record_base; u32 node_offset[node_count]`, then the
  node records, then `record_count` records of 32 bytes starting at
  `record_base`. The offset table begins at `+12`, and the first offset equals
  the end of the table. `record_count * 32` is exactly
  `section1_offset - record_base` in all 126 files, so the node records fill
  everything between the node offsets and the record table and section 0
  accounts for every byte. The 32-byte records are not decoded.
- **section 1 (mesh records)**: `u32 count; u32 offset[count]`, then
  `4 + 8*kind` byte records (`u32 kind; kind * (u32 value;
  u32 mesh_offset)`). Kinds 1-4 occur; the records tile the section exactly.
  Every `mesh_offset` is a section-2 blob start; the `value` word is probably a
  radius but its meaning is open.
- **section 2 (mesh blobs)**: `u32 count; u32 offset[count]`, then blobs whose
  first 44 bytes are 11 file-relative pointers (the first is always
  `blob + 0x58`). There can be more blobs than section-1 records.
- **section 3 (draw spans)**: `u16 count; u16 level_wrap; u32 offset[count]`,
  then 16-byte span headers (below) with variable bodies.
- **section 4 (event points)**: `u32 length; u32 group_count;
  u32 offset[group_count]` (section-relative, 0 = absent), then groups of
  `u32 count; count * 40-byte records`. Groups tile the section; their
  meaning is open.

For `TC_lisence.tro` (216 984 bytes) the sections start at `0x38c`, `0x2fca0`,
`0x2fd74`, `0x32d04` and `0x34ea4`; section 0 has `node_count = 124`,
`record_count = 403`, `record_base = 0x2ca40` (403 × 32 bytes runs exactly to
section 1), and `lap_length = 5241.527 m`; sections 1 and 2 have 13
records/blobs, section 3 has 37 spans (`level_wrap = 3`), section 4 is 244
bytes with one present group. A node at file offset `o` becomes
`0x800B4A34 + o` in RAM. Node 78's two link words are **zero in the file** and
the loader fills them with the addresses of the nodes named by its `u16` link
indices (`node[77]`, `node[79]`), so the indices are the authoritative link
data.

### Road nodes (section 0)

Section 0 is the road as an ordered table of nodes. `TC_lisence` has 124.
Following the `u16` at record `+2` from node 0 visits all 124 and returns to
node 0, and the cumulative distance at `+0x10` increases on 122 of the 123
steps, with a single wrap (node 74 → 75: 5188.9 m → 0.0 m). So `+2` is the
**next node along the course** and the table is the lap in driving order,
rotated so node 0 is not the start line; `+0` is the reverse neighbour. That is
why the car's index advances 78 → 80 as it drives.

Measured record fields (node 78, checked against both the file and RAM):

```text
+0x00  u16       link index A                       77
+0x02  u16       link index B                       79
+0x04  u32       link address A    file 0; RAM 0x800CED68 = node[77]
+0x08  u32       link address B    file 0; RAM 0x800D0384 = node[79]
+0x0c  u32       flags/type                         1
+0x10  s32       cumulative course distance, fx16.16 m   157.9487
+0x14  s16       scale, -4096..=4096 (4096 = 1.0)  ~4096
+0x16  s16       scale                              ~4095
+0x18  s32       point x, 16.16 m                    433.7085
+0x1c  s32       point y, 16.16 m                    0
+0x20  s32       point z, 16.16 m                    -449.6918  (-world z)
+0x24  3 x s16   direction                           (-4096, 0, 0) = -x
+0x2a  ...       further point/direction records (not decoded)
```

The two signed scale words are not 12-bit unsigned: `-4096..=4096` occurs
across the 126 files, and 4096 = 1.0. Their meaning is still open.

Node records are variable length (nodes 77-80 run 0xad8-0xbd4 bytes) and carry
two 4-corner road quads after the header. The second quad, at `+0x6c`, is the
road surface on `TC_lisence`:

```text
+0x6c/+0x70   near corner 0  (x, z) 16.16      this node's cross-section
+0x74/+0x78   near corner 1  (x, z) 16.16
+0x7c/+0x80   far corner 0   (x, z) 16.16      the next node's cross-section
+0x84/+0x88   far corner 1   (x, z) 16.16
+0x8c         metadata
+0x90         tag (11618 for node 78)
+0x92         count (15 for node 78)
+0x94         count file-relative offsets into primitive/vertex lists
```

Verified over all 124 nodes: each end pair's midpoint is the node's centreline
point (max 0.49 m near, 0.52 m far), the cross-section is perpendicular to the
node direction, and the width is 19.45–22.58 m (22.58 m on the start straight).
Node 78's near corners are z = 438.44/460.95 m at x = 433.70 and its far
corners the same z at x = 381.04 (node 79). The file stores only `(x, z)`, so
the height comes from the node points (near from this node, far from the next);
on `TC_lisence` every node height is within 1.2 cm of zero.

Adjacent quads do not share an edge exactly (up to 2.0 m corner difference):
the original stores each node's far edge itself rather than deriving it from
the next node's cross-section, so a renderer draws them independently and
keeps the small overlaps/cracks. The first quad at `+0x44` is 0–73 m wide,
offset up to 26 m from the centreline, and degenerate on many nodes; its role
is still open. The same `+0x6c` field exists in all 126 courses, but degenerate
and far-offset quads occur there, so the road-surface reading is established
for `TC_lisence` only. The node's primitive/vertex lists use 8-byte vertex/UV
records and 12-byte primitives ending in PS1 GPU colour/command bytes (`0x24`,
`0x28`, `0x2c`); the mesh decode is still open.

### Draw spans (section 3)

A span selects the road geometry to draw for the car's course distance and
level of detail. Its 16-byte header is

```text
u16 kind; u16 min_level; u16 max_level; u16 node;
s32 start_fx16.16; s32 end_fx16.16          (metres)
```

`start`/`end` are on the same course-distance scale as `car+0x604`; the earlier
appearance of a separate parameterisation was a span group whose tiling simply
starts at 4877.216 m and wraps, not a different origin. `TC_lisence` has three
complete 12-span groups plus one final wrap record.

Selection is `0x800117C4`, called with section 3, `car+0x608` (the level) and
`car+0x604` (the distance). Its rule, quirks included:

1. If `level_wrap != 0 && level > level_wrap`, set
   `level = (level - 1) % level_wrap + 1` (levels wrap to 1..=3, never 0).
2. Skip a span unless `min_level <= level <= max_level`.
3. Add one lap to the distance if `min_level < level`, then accept the span if
   `start <= distance < end`, adding one lap to `end` when `end < start`.

Step 3 is what makes the overlapping level ranges select one group per level.
At the B-1 start `car+0x604 = 190.2086 m`, `car+0x608 = 1`; the rule returns
the span with `node = 78`, the same node the car stores at `car+0x600`. The
engine's `CourseData::span_at` reproduces this exactly.

The distance solver is `0x80028C6C` → `0x80028588` → `0x80028394`. It starts
from the car's node (`car+0x600`), compares the car's position with each
node's point and direction (`0x80027BBC` computes `dot(position - point,
direction)`), and returns

```text
course distance = node[+0x10] + dot(position - point, direction)
```

in fx16.16 metres. Verified numerically: at `car+0x600 = 78`, car x =
401.4431 m, node 78 x = 433.7085 m and `node+0x10 = 157.9487`, so 157.9487 +
(433.7085 - 401.4431) = 190.214 m against the stored `car+0x604` = 190.2086 m.
Nodes 78/79/80 have distances 157.95/210.59/263.24 m (52.64 m apart), and the
car's x falls by the same amount as it passes them. The car's z = 449.69 m is
exactly the node table's centreline z (stored −449.69), and x = 401.44 lies
between node 78 (433.71) and node 79 (381.06), so the triple really is a world
position on the road, not a local offset.

The car's position triple is stored as `(x, z, y)` in 20.12 metres at
`car+0x65C/0x660/0x664`. The distance solver reads it as `(x, y, z)` and
negates the third component before comparing with the node table, whose point
triples are `(x, y, -z)`. The renderer `0x80043388` adds the per-wheel offsets
to the same triple and shifts each by 4 into GTE 16.16, which is what fixes the
20.12 unit.

The engine's baked course data (`engine/src/course.rs`, written by `import tro`)
contains the lap length, the node table in world metres, the node's road-surface
quad as four world corners (`CourseNode::surface_m`) and the span headers, plus
the `level_wrap` word. `CourseData::surface_quads` skips degenerate quads and
unwraps the far distance across the lap; `CourseData::point_at` interpolates the
centreline at a course distance. The game's B-1 scene builds the road ribbon
from the quads and puts the camera at the recorded start (190.2086 m, node 78,
level 1). How the rest of the course is **drawn** (sections 1-2 and 4, and the
node bodies' primitive lists), how collision is derived beyond the node table,
and what the 32-byte records and the first `+0x44` quad mean are still open.
`NAME.trp.gz` / `NAME.bsp.gz` are TIM packs (a `u32` count then standard TIMs)
and already decode with `import tim`/`tims`; they are the textures.

The car models `/carobj/NAME.cdo.gz` (`.cno` the same format) are also custom,
not TMD: `GT\x02\0` then 24 zero bytes then packed data. `.cdp.gz` (`.cnp`) is
the companion `02 00 "XX"` pack. Their loader is also in the race overlay and
is still undecoded.

## Movement

**Partial.** The car's world state is three position components and a heading:

- `car+0x65C` x, `car+0x660` z, `car+0x664` y (height): 20.12 fixed metres,
  stored `(x, z, y)`, updated by the 30 Hz car tick.
- `car+0x648`: heading, u16, 4096 units/turn (`0x400` = 90°), wrapped into
  [-0x1000, 0x1000) by `0x80033E6C`.
- `car+0x604`: distance along the course, fx16.16 m, derived from the
  position and the road nodes (above).
- `car+0x600`: current road node.

`0x80033E6C` adds a three-component delta into `car+0x65C/0x660/0x664` and a
heading delta into `car+0x648` before the wrap. `0x80030330` constructs
those steps from post-force world velocity and yaw rate. The native step
builder and integrator now match 300 consecutive original ticks, given
measured force outputs. Digital steering, wheel steering commands, engine
torque and fuel cut are also ported. Native slip, tyre force, forward integration,
axle/wheel coupling and ordinary transmission feedback now connect those
component rules; transmission comparisons retain engine/gear/timer state over
consecutive ticks. Lateral/yaw/world acceleration now feeds VehicleVelocity,
including original reset/brake-stop branches, and lateral feedback carries
into the next drivetrain call. Road/suspension load, surface/orientation and
collision remain unfinished; body velocity is still initialized at observed
boundaries in these tests. See [body evidence](body-dynamics-evidence.md).
Load/camber/surface-to-grip conversion and the later corrected-world-to-contact
motion refresh are now ported, including wheel/axle synchronization and spin.
The refresh runs after movement/collision and supplies the next tick's local
inputs. Road/suspension load, surface/orientation and collision corrections
remain separate producers. +6AE/+6F8 are speed-derived halfwords, not RPM.
See [contact input evidence](contact-input-evidence.md).
`0x80043388` and
`0x8003D498` build four per-wheel GTE matrices from the position, the heading
and per-wheel offsets.

The tick is 30 Hz (fields stored on every second vblank) while the RPM copies
and presentation run at 60 Hz. The dash-speed calculation remains open; the
observed position delta (105.61 m over 240 vblanks = 4.0 s ≈ 26.4 m/s) matches
it within a few percent.

## Objects

Provisional. Assets are addressed **by path** out of the `GTFS` tree (see
"Content"), not by a numeric id the game invents, and each car contributes a
fixed family of files (`/carobj/NAME.cdo.gz` … , `/carwheel/NAME.tim`,
`/carlogo/NAME--.tim`) so a car's parts are selected by name, i.e. by an id
string the original's car tables hold. The engine's object-type registry still
fits: a type maps to a controller and to the asset paths it loads. The per-type
control functions and which fields are tabled are not yet read out of the code.
The per-frame dispatch tables under "Update dispatch" are the other half of
this model: work is reached through function pointers looked up by an index, not
a `match` on a mode. Which index selects which handler is the next step.

## Level scripting

_Not yet established._

## Road quad refinement follow-up

The geometry format retains perimeter quads alongside triangles. Flat-textured
node quads carry their original bound and refinement parameters in normal
units. Engine code applies screen-area/depth decisions, preserves inherited
corner visibility, rounds UVs at every split and emits leaf quads plus stitch
triangles before near-plane clipping. Three original recordings verify all
330 splits and 1,062 emitted primitives, including a final holdout. See
[subdivision evidence](subdivision-evidence.md) for commands and limits.
Other primitive groups and complete pixel/draw-order parity remain unfinished.
The sandbox authors and renders the same quad format from handmade data.

## Road texture detail follow-up

Source material templates supply both UV/texture variants and an area
threshold. A parent selects its material before refinement, using unsigned
triangle area or absolute quad area. Engine geometry keeps both sets of UVs
and ordinary image paths, builds separate large/small material buckets and
then refines/clips the selected parents. Coordinate precision is shared data
beside the optional refinement profile. All 255 recorded selections and
parent pixels match, including a final holdout. The handmade sandbox panel
uses independent blue/orange checker textures with the same selector. See
[material variant evidence](material-variant-evidence.md).

## Camera and race cadence follow-up

The original driving camera, like the body/physics update, writes once every
two video fields. Native race logic now uses FIELDS_PER_TICK=2; the earlier
per-field menu observations above still apply to menus. Rendering remains
60 Hz. Camera subjects provide a rendered body transform and stabilizing
angles; ordinary bumper/chase rules consume those without depending on tyres,
HUD, course or sound. Intro/replay and mirror/split-screen presentation are
separate unfinished paths. [Camera evidence](camera-evidence.md) records the
rules, parameters and original numeric comparisons.

Projection is reusable engine data: reference image dimensions, independent
focal lengths, optical centre and clip distances. A Bevy custom projection
preserves those across target resizing. Engine geometry can opt into clipping
source triangles before affine projection, retaining source attributes for
new intersections. This avoids the negative-W interpolation defect exposed
by the low bumper view. Other primitive refinement and per-view
geometry for mirror/split-screen rendering remain unfinished.

## Course mesh conversion follow-up

The current scene supersedes the earlier ribbon-only status above. Section 2
has exact mesh-local framing: an 88-byte header, signed 8-byte vertices and
ten primitive groups. All 5,709 meshes validate. The 33 placement lists at
file +0x118 tile the pre-section-0 region and contain 6,084 instances across
126 courses. Rotation, per-axis scale, world translation and mesh exponents
now convert offline to metre geometry; original matrix recordings include a
held-out rotation. Source sine-table asymmetry and intermediate matrix
truncations are preserved in the importer.

Road-node mesh headers at +0xA4 point to 1/64 m local geometry and compact
9-bit-index polygons. All 14,387 nodes validate. The B-1 runtime renders
their ordinary polygons plus 13 placed scenery meshes, with 32 imported
PNG material bindings. The engine's `GeometryData` expresses ordinary
triangles, metre positions, normalized UVs, colours, side/blend modes and
image paths; another game can author it directly. No original GPU addresses
or raw packed records are interpreted at runtime.

[Course mesh evidence](course-mesh-evidence.md) records addresses, exact
commands, corpus totals and rendering review. Model LOD,
visibility lists, procedural groups, original background/camera, collision
beyond the road table and complete rendering parity remain unverified.
