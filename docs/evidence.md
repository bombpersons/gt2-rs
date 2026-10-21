# Evidence

Current vehicle work: [driving evidence](driving-evidence.md) records digital
steering limits/spring, wheel steering and velocity-to-step construction.
All 300 consecutive original ticks and 39 controlled boundary probes match
exactly. [Dynamics evidence](vehicle-dynamics-evidence.md) records engine
torque, drag and fuel cut with 168 original calls and independent holdouts.
The decoded [car body](car-model-evidence.md) and [procedural wheels](wheel-model-evidence.md)
now render in chase views, and the example mod replaces both with authored
geometry. [Slip preprocessing](wheel-slip-evidence.md) matches 100 original
boundary/holdout executions; brake allocation matches 133 original calls.
[Brake assistance](brake-assist-evidence.md) matches 130 original calls,
including independent holdouts and pedal-to-demand integration.
Tyre/drivetrain/body force coupling, upstream wheel/yaw inputs, live wheel
animation, reflections, shadows and a playable session remain
unfinished. These checks cover component rules, not complete vehicle parity.

Current road material detail: [material variant evidence](material-variant-evidence.md)
records the original unsigned projected-area selector before subdivision.
All 255 selected load addresses and parent pixels match, including an 80-parent
final holdout. The native asset retains both variants for 3,295 textured road
parents and exports 32 texture bindings. The handmade sandbox demonstrates
the same large/small material switch without disc content. Scenery model LOD
and full framebuffer parity remain unfinished.

Current road refinement: [subdivision evidence](subdivision-evidence.md) records
flat-textured quad recursion, coordinate/UV rounding, conservative corner
visibility, depth limits and stitch triangles. Bumper and independent chase
recordings, plus a final far-view holdout, match all 330 splits and the ordered
UVs/counts for 583 leaf quads and 479 stitches. Projected corners agree within
one pixel. The runtime
uses this on 2,907 imported B-1 road quads. This supersedes the earlier
blanket "adaptive subdivision unfinished" status; other primitive groups,
scenery LOD, final transform/pixel rounding and full rendering parity remain
unfinished.

Current camera follow-up: [camera evidence](camera-evidence.md) records the
ordinary bumper/near/far chase controller, rear look, the original projection
and 30 Hz race cadence. All 192 recorded camera updates pass bounded numeric
comparisons, including an independent holdout; view transitions match exactly.
The native scene uses the checkpoint body pose and original lens, with engine
near-plane clipping before affine projection. The live subject remains static
until vehicle forces/body motion are ported. Intro/replay cameras, mirrors,
split-screen rendering and exact subdivision/clipping parity remain open.

Current course-rendering follow-up: [course mesh evidence](course-mesh-evidence.md)
records the decoded 33-list scenery placement table, original transform
matrix regression/holdout recordings, and native road/scenery rendering.
The importer validates 6,084 placements across 126 courses; B-1 now renders
124 road-node meshes plus 13 scenery instances (3,883 polygons, 32 textures).
This supersedes older ribbon-only descriptions below. Model LOD,
procedural primitives, intro/replay cameras, backgrounds and gameplay parity remain
unfinished.

What is verified, against what evidence, and what is still provisional. One
entry per ported feature: the `psx` recordings or binary evidence behind it,
the tests that check it (trajectory, screenshot), and known gaps.

Commands below use the project's disc at
`extracted/Gran Turismo 2 (USA) (Simulation Mode) (Rev 2).cue` (raw `.bin`
beside it) and the NTSC-U BIOS the project selects. Run directories are under
`.psx/runs/` and are not committed: regenerate them with the recorded command.

## Disc, volumes, boot

| Finding | Evidence | Checked by | Status |
|---|---|---|---|
| Disc has six files; boot `SCUS_944.88`, entry `0x8005D600` | `psx disc`; `psx ls` | `import isofs` | verified |
| The original's own file table, in RAM: `[lba][size][name 16]` at `0x80009224`, six entries matching the ISO9660 table exactly | RAM dump, `.psx/runs/1792329474756-1428115-native-1/ram.bin` | `tools/import/src/iso9660.rs::file_table_matches_the_runtime_evidence` | verified |
| `GT2.VOL` starts with the `GTFS` descriptor: `"GTFS"`, a `u32` 0, an 8-byte tag, then the offset table at `0x10`; copy kept in RAM at `0x801e35f0` | volume bytes + `.psx/runs/1792329474756-1428115-native-1/ram.bin` | `tools/import` `volume` (validates monotonic, in-range, tiles the volume) | verified |
| The table is **11580** `u32` entries (11 580 resources, ids 0..=11579) terminated by a sentinel equal to the volume size (`0x1d19f800 = 488241152`); entry `id` packs `(sector << 11) \| pad` | volume bytes; the same RAM copy at `0x801e35f0 + 0x10` | `tools/import/src/gtfs.rs::parses_entries_terminated_by_the_volume_size`; `import volume` | verified (supersedes the earlier 187-entry reading: `entries[0] = 0x2fc` is not a table length) |
| Resource data is **sector-aligned**: data(id) = `[entry[id] & !0x7ff, entry[id+1] & !0x7ff)`, true length = that span minus `entry[id] & 0x7ff` (the entry's own trailing padding) | the volume bytes | `gtfs::resource_ranges_are_sectors_and_sizes_drop_the_padding`; `import volume` rejects negative lengths | verified |
| The loader reads exactly these entries: `fn_8005D74C` = `entry[id] >> 11` (sector), `fn_8005D79C` = `(entry[id+1] & 0xfffff800) - entry[id]` (length), `fn_8005D848` passes both to a callback; `FUN_80068310` then reads through the CD layer into a 2 KiB buffer | `psx ghidra decompile` of `SCUS_944.88` at `0x8005D74C`, `0x8005D79C`, `0x8005D848`, `0x80068310` | — | verified |
| Every `.tim` starts with the TIM magic, every `.gz` with the gzip magic, every `.es` with `ENGN`, at the sector start — never at the raw entry | the 3576 + 5114 + 2747 resources | ad-hoc scan in this pass; `import extract` output | verified 3576/3576, 5114/5114, 2747/2747 |
| The low bits really are padding: for all 5114 gzip resources the span minus the entry's low bits equals the decoded gzip stream's length, and all 5114 decode to end-of-stream (80 693 248 → 222 813 744 bytes) | the volume bytes | ad-hoc `zlib` pass; `import extract` + `gzip -t` | verified 5114/5114 |
| Volume directory records are 32 bytes `[u32 key][u16 ordinal][u8 flags][char name[25]]`; flags `0x01` = a node starts at `ordinal`, `0x80` = last record of this directory; 11620 records parse | `import volume` output `game/assets/gt2/volume.index.json` | `tools/import/src/gtfs.rs::reads_records_and_their_flags` | verified |
| The tree is **path-keyed**: the root node (28 records) names 12 root files and 16 directories, each directory record's `ordinal` points at its child node; 11578 files resolve; resource 1 is the record table, `[0xB800, 0x66480)` | the same index; a walker over it | `gtfs::resolves_paths_through_the_tree`; ad-hoc walk | verified |
| The disc's whole asset library resolves from the EXE's own path table: 248 `char*` paths at `0x8009118c` (strings from `0x8008e024`), 228 resolve through the tree | RAM dump (path table + strings) | ad-hoc resolution over the index | verified; the 20 misses are directory-only paths (`/bgsobj`, `/carwheel`) and language variants absent from the disc (`/carparam/fra_arcade_data.dat.gz`) |
| Resource 62 `/arcade/license_info_uk` decodes to English licence text; resource 63 `/arcade/license_info_us` likewise | the volume bytes; the index | `import extract --path /arcade/license_info_uk` | verified |
| `/gtmenu` has `fra ger ita spa usa` subdirectories matching the EXE's `/gtmenu/<lang>/…` paths | the index + the same RAM dump | ad-hoc | verified |
| `/replay/scea.000`, `scea.999`, `scee.*`, `scei.*` are the only six zero-length resources | the index | `import volume` (`6 empty`) | verified |
| Game code is not in the EXE: main runs an exec/segment launch (`0x8005D6E0` → `0x8005DA3C(1)` → `0x8005DA7C`) | `psx disasm`/`ghidra decompile` of `SCUS_944.88` | — | verified |
| SDK is PsyQ 4.70 (LIBAPI/LIBC/LIBCARD/LIBCD/LIBETC/LIBGTE/LIBPAD/LIBPRESS/LIBSN) | `psx symbols .psx/exe/SCUS_944.88` | — | verified |
| `GT2.OVL` is the segment container: `u32 0x30`, then per member its gzip compressed size with the next member's `align4` offset stored after it, 2 trailing zero bytes; **six** gzip members, 1 133 632 bytes of image, all linked at `0x80010000` | the disc's `GT2.OVL` (LBA 331, 142 sectors); all six members inflate to end-of-stream and 0 bytes are unaccounted for | `tools/import/src/overlay.rs` tests; `import overlays` (6 overlays, 1 133 632 bytes) | verified |
| The gzip members are decompressed by a **resident** EXE DEFLATE decoder, not by overlay code: `FUN_800847D0` (`0x800847D0`) with inner symbol reader `FUN_80084364` (`0x80084364`), fed by a CD refill callback (`FUN_8007ABB8`/`FUN_8007ABFC`) | `--trace` from the title state along the licence route: `0x80084364` entered at frame 2241 with input `0x800E5574` (gzip header at `0x800E54E0` = `GT2.OVL` offset `0x3C784`) and output `0x80010000`; RAM at both addresses | `ghidra decompile` of `0x800847D0`, `0x80084364`, `0x8007ABFC`; the trace's register dump | verified (member 4; the other members are assumed to share the load address) |

## Data formats

Inside the volume (`docs/formats.md` has the full notes and hexdumps).

| Finding | Evidence | Checked by | Status |
|---|---|---|---|
| 5205 of the 11578 files are gzip streams and 6373 raw; the volume's file name does not say which (`/gtmenu/*/gtmenudat.dat` is gzip, `/dirt/*` are gzip with no extension) | the volume bytes | ad-hoc magic census over every resource | verified |
| Every `*.gz` resource is one complete gzip stream (5114/5114 end-of-stream with all input consumed, no nesting) and decodes to 222 813 744 bytes from 80 693 248 | the volume bytes | ad-hoc `zlib` pass; `import extract` + `gzip -t` | verified |
| The gzip header's `FNAME` names the true file, extension included (`aa_nn_dirt_2.dgf`, `eng_gtmode_data.dat`), and its mtime is a 1999-11-13…26 build stamp; 5190 of 5205 carry one | an extracted resource's first bytes | `import extract`, read the header | verified |
| `.tim` is PS1 TIM: `arc_font.tim` = 8 + CLUT `0x6c` + image `0x0f80c` = 65664 = its true length; the file holds seven TIM headers | extracted `arc_font.tim` | ad-hoc parse | verified; some GT2 TIMs set `flags = 0` and their image size does not match `w x h`, not decoded |
| `/engine/NNNNN.es` is `ENGN`: header `"ENGN", u32 0, u32 header, u32 0, u32 payload, u32 header, u32 zones, u32 0x30`, header = `0x20 + (zones + 1) * 16`, then a `payload`-byte SPU ADPCM stream; header + payload is the whole resource (0 bytes of padding across all 2747) | extracted `00000.es`, `00001.es`, `ene_n.es`, `ene_t.es`; a pass over all 2747 | `tools/import/src/engn.rs::parses_the_header_and_slices_the_payload` + the `rejects_*` test; `import engines` (parses all 2747, reports 0 padding) | verified 2747/2747 |
| The `ENGN` payload is PS1 SPU ADPCM (16-byte frames, 28 samples, 44 100 Hz); decoding is checked against the audio itself: `00000.es` is a 441 Hz tone (autocorrelation 0.995) and most numbered assets land on plausible engine fundamentals | extracted payloads; decoded WAVs | `engn::decodes_a_frame_of_bare_nibbles`, `engn::shift_scales_down_and_the_filter_decays_the_previous_sample`; ad-hoc autocorrelation of the decoded WAVs | verified |
| The 2747 `ENGN` files hold only **467 distinct payloads**: most `_n*`/`_t*` slots repeat one 4000-byte 441 Hz placeholder (`source_fnv1a64` dedup). All 467 are baked (1148 s, 98 MB) as `game/assets/gt2/sound/sample_NNNN.wav` + `table.ron` | a pass over all 2747 | `import engines` (reports "2747 sounds (467 distinct samples, 1148 s) ... 0 bytes of resource padding") | verified |
| The `ENGN` zone records sit on a 16-byte grid (count = `zones`, region `0x30..header`), but their fields are not interpreted; for `00001.es` the second u16 of each walks 1000, 2000, … 8000, reading as engine RPM | extracted headers; `psx` search of `SCUS_944.88` (the executable names only `/engine/ene_n.es`/`/engine/ene_t.es` at `0x8008f32c`/`0x8008f340`) | `import engines` records each file's zone count | provisional (the reader is in an extracted `GT2.OVL` image, `docs/overlays/`) |
| `/carparam/*_unistrdb.dat` is `WSDB`: `u32 length, "WSDB", u16 count` then `[u16 units][UTF-16LE][u16 0]` per entry; every byte of all seven language files is accounted for (846/851/764 entries) | extracted resources | `tools/import/src/wsdb.rs::parses_slots_in_release_order` + the `rejects_*` tests; `import text` (parses all 7) | verified |
| `/carparam/*_data.dat` is `GTDT`: `"GTDT", u16, u16 capacity`, then `capacity` `[u32 offset, u32 size]` pairs that tile the file (contiguous, last ends at the file length); block schemas not decoded | extracted resources | ad-hoc parse | container verified; block meanings not decoded |
| `/gtmenu/<lang>/gtmenudat.dat` is **3386 concatenated gzip members** on 4-byte boundaries (0..3 zero bytes between), each inflating to a `GM\3` blob: 35.8 MB packed → 99.0 MB, 5134 bytes of zero padding, every byte accounted for. `commonpic.dat` is 458 `GTMP` chunks delimited exactly by `commonpic.idx` (sizes `0x5000..0x43000`, all multiples of `0x1000`). Both `.idx` are `u32 count, u32 field, count end offsets`, the last equal to the `.dat` length | extracted resources | `import menu` splits both, reports 0 unaccounted | containers verified; entry payloads not decoded |
| The `GTMP`/`GM` payloads are neither gzip nor raw deflate (no start offset in a chunk inflates), and no high-entropy 16-byte window of a displayed chunk appears in that frame's VRAM, so a chunk is not uploaded verbatim; the CPU never reads a chunk during the licence route (only the overlay decompressor borrows the buffer as tree scratch), which points at a CD load plus a GPU DMA upload | `.psx/runs/1792443775682-5534-native-1` (`watch_reads`/writes over both buffers, 10974 reads, all from `0x800843E4`); `.psx/runs/1792435595073-76390-native` RAM+VRAM at the licence list; zlib scans over `commonpic.dat` | ad-hoc | verified negative; the upload path is open |
| `/gtmenu/*/iconimg.dat.gz` → 32768 bytes (4bpp icon sheet; 252 distinct byte values, mostly `00`/`FF`); `solodata.dat.gz` → 4564 bytes beginning `u32 50` then 50 `u16` = `{0} ∪ 761..809` | extracted resources | ad-hoc parse | provisional |
| `.trp`/`.bsp` are `u32` count + TIMs; `/arcade/arc_carlogo` is `u32 count = 63` + 63 offsets, each at a TIM | extracted resources | ad-hoc parse | verified |
| `/sound/*.seq` start `SEQG`, `*.ins` start `INST`; `/.text/data-race.txd` is a plain string table | extracted resources | ad-hoc parse | verified magic only |

## Frame

| Finding | Evidence | Checked by | Status |
|---|---|---|---|
| Vblank handler `0x8007D294` (installed via `VSyncCallback` from `fn_8007D0A4`) increments `0x801F0680` and `0x801F0684` once per vblank | `psx disasm` of RAM, `.psx/runs/1792329474756-1428115-native-1` | — | verified |
| `0x801F0680` advances exactly 1 per vblank over 120 frames from the title (`1017 → 1137`, deltas `{1}`) | `run --from-state state_f002100 --sample 801F0680:u32 801F0684:u32`, `.psx/runs/1792330788329-1428115-native-37` | — | verified |
| Display is 320x240 in the title/GT-Mode modes | run screenshots | — | verified |
| A game per-frame handler runs once per vblank: `0x800B1228` (only incrementing writer `fn_80017984`: `lw`/`+1`/`sw`, clamp 901) and `0x80092E80` each advance exactly 1 per vblank (deltas `{1}`) | `run --from-state state_f002100 --sample 80092E80:u32 800B1228:u32`, `.psx/runs/1792333019134-1428115-native-39`; `psx xrefs 0x800B1228 --ram`; `psx disasm --addr 0x80017AB8` | — | verified |
| Logic advances once per vblank (`FIELDS_PER_TICK = 1`) | the row above | — | verified for menus; race to confirm |
| Per-frame update functions are dispatched through function-pointer tables in EXE data (`0x8004BC50`, seven data pointers at `0x8004BC5C` stride `0x48` into a 7×72-byte record table at `0x8004BA08`) | RAM dump; `psx xrefs 0x80017984 --ram` | — | verified structure; meaning of the records provisional |

## Reference checkpoints

Replays are in `docs/checkpoints/`. The observed screens are:

| Checkpoint | Reproduce | Observed |
|---|---|---|
| Boot to the GT2 title screen | `psx run --media <cue> --frames 2100 --snapshot-every 300` | BIOS SCEA/SCE logos 300-900, "Sony Computer Entertainment America Presents" 1200-1500, title with `▼ Start Game` from ~1800 |
| Title → GT Mode map | from the frame-2100 state, `--replay docs/checkpoints/title-to-gtmap.json` | mode switch ~300-500, world map with HOME/GO RACE/LICENSE/CAR WASH/… from ~600 |

`CROSS` (not `START`) selects "Start Game"; the transition takes about 300
frames of mode switch before the map appears.

The GT Mode map settles with the cursor on HOME; `RIGHT` then `CROSS` enters
the License Test menu and a second `CROSS` the B-License list
(`title-to-license-list.json`, 900 frames from the same state, ends on the
list). The map's icons are reached spatially: HOME → LEFT is GO RACE (GTF /
GT League / Nationals), HOME → RIGHT is LICENSE, HOME → DOWN is MACHINE TEST.
The map is drawn as a full-screen background object, so its cursor moves do
not touch the mode's frame counters.

**Entering a driving session: resolved.** The 0.1.0 stall (`Setmode 0xA0` CD
loop, see `docs/psx-bug-license-test-stall.md`) is fixed in psx 0.3.2: the
confirming `CROSS` on B-License test 1 → `Start` → `AT` loads the test (load
black ends by ~frame 300) and the session runs. The verified route is the
four-replay chain in `docs/checkpoints/README.md`
(`poweron-to-b1-list` → `b1-list-to-b1-menu` → `b1-menu-to-transmission` →
`transmission-to-race`), reproduced end to end from power-on; holding `CROSS`
from ~frame 400 of the last hop launches the car.

First in-race recordings (all from the B-1 session, psx 0.3.2):

- The load is bounded and readable: after the `AT` confirm the game reads
  sequential LBA spans 103842-103910 and 108130-108216, then short reads at
  35434-35444, 39152, 42455-42512, 3376-3388, 915-929 and 2610-2616, and the
  CD goes idle from ~140 frames after the press; the scene appears ~240 frames
  after the press (LBA → volume offset: `473 + LBA×2048` bytes into `GT2.VOL`).
- Race-mode frame counters: `0x801F0680` (vblank) and `0x80092E80` still
  advance exactly +1 per frame; the menu counter `0x800B1228` is dead in race
  mode (constant). A race-local counter at `0x800A8C64` advances +1 per frame,
  and `0x800A8D70` +1.0 (fx16.16) per frame.
- Car struct at **`0x800A96B4`** (found from its tick function): its per-tick
  function is `0x8003CE3C`, which recomputes `car+0x604` each tick via
  `0x80028C6C` and then checks squared velocity components in
  `car+0x6A4/+0x6A8`. Earlier RPM/shift-band and `car+0x45D` gear claims
  were wrong: [driving evidence](driving-evidence.md) records the correction. `car+0x604` is **course distance travelled in metres**
  (fx16.16): its per-tick delta tracks the dash speed (e.g. 0.694/tick ≈
  20.8 m/s ≈ 46 mph at one anchor; segment integrals match the mph curve
  within ~3%), and it is stored every **2nd vblank** — the car fields tick at
  30 Hz while display-side counters tick at 60 Hz. `car+0x6AC` is calculated
  integer RPM in a halfword, refreshed after drivetrain integration;
  `+0x6AE` is separate RPM-related state and `+0x6F8` the presentation copy.
  The earlier interpretation of the word at `+0x6AC` as fixed-point RPM was
  wrong; see [dynamics evidence](vehicle-dynamics-evidence.md).
- An earlier dash-speed search scanned all RAM
  (u8/u16/u32, any alignment) across four snapshots of a known acceleration
  (19/34/46/55 mph at frames 600/800/1000/1200) found no value proportional to
  it. That negative search did not establish the display calculation.
  **The world position/heading were subsequently
  found**: see "World and movement" below. `car+0x65C/0x660/0x664` is the
  world position in 20.12 metres and `car+0x648` the heading. Velocity fields
  have since been found; the actual dash-speed calculation is still open.

## World and movement

All addresses are main RAM; the run directories are under `.psx/runs/`.

| Finding | Evidence | Checked by | Status |
|---|---|---|---|
| A race loads exactly one `.tro`; for B-1 it is `/crsobj/TC_lisence.tro` (resource 8322, gzip, volume LBA 42455), and it is already in the in-race read span 42455-42512 | RAM `0x800B4A34` vs the extracted resource; `b1-gas-1200`; the in-race CD read spans recorded above include 42455-42512 | `cargo run -p import -- extract --id 8322`; byte comparison | verified |
| The course container starts at `0x800A9500`; the `.tro` image is `container + 0xB534 = 0x800B4A34`, and the distance code reads section 0 as `[image + 0x10]` (`0x80028C9C`: `ori v0,0xB534` / `addu` / `lw 16(a0)`) | `psx disasm --ram .psx/runs/b1-gas-1200/ram.bin --function 0x80028C6C`; RAM dump | — | verified |
| `.tro` header: `"@(#)GT-PS\0\0\0"`, `u16 0`, `u16 0x1f`, five file-relative `u32` section offsets; sections contiguous; last ends at file length | all 126 `/crsobj/*.tro` files | `import tro` checks every file | verified 126/126 |
| Section 0: `u32 lap_length_fx16.16; u16 node_count; u16 record_count; u32 record_base; u32 node_offset[node_count]` (offsets start at `+12`, first = table end); `TC_lisence` has 124 nodes, 403 records at `0x2ca40`, `lap_length = 5241.527 m`; sections 1/2 have 13 records/blobs, section 3 has 37 spans (`level_wrap = 3`), section 4 is 244 bytes with one present group | `TC_lisence.tro` at `0x38c` etc.; the same layout in all 126 courses | `import tro` | verified structurally |
| Section 0 accounts for every byte: in all 126 `/crsobj/*.tro` files `record_count * 32 == section1_offset - record_base`, and the node records fill the space after the node-offset table | all 126 decompressed `.tro` files | `import tro` over every file | verified 126/126 |
| Section 1 records are `u32 kind; kind * (u32 value; u32 mesh_offset)`, sizes `4 + 8*kind` (kinds 1-4), contiguous and tiling section 1; every mesh offset is a section-2 blob start | all 126 files | `import tro` | verified 126/126 |
| Section 2 is `u32 count; u32 offset[count]` then meshes with 88-byte headers, 8-byte signed vertices, bounds and ten primitive arrays; polygon references are packed as 10-bit indices. Arrays tile each mesh, all vertices fit bounds, all polygon references are in range: 5,709 meshes, 280,301 vertices, 145,661 primitives | all 126 files; original renderer `0x80019B58` and procedural expansion at `0x8001F7F8`; 2,170 untruncated reads from B-1 section 2 | `import tro`, synthetic corruption tests and opt-in corpus test; [details](course-mesh-evidence.md) | storage/references verified 126/126 and runtime corroborated; world transforms/material conversion open |
| Section 3 is `u16 count; u16 level_wrap; u32 offset[count]`; each span starts `u16 kind; u16 min_level; u16 max_level; u16 node; s32 start_fx16.16; s32 end_fx16.16`. `TC_lisence`: 37 spans, `level_wrap = 3`, three 12-span groups plus a final wrap record | `TC_lisence.tro` at `0x32d04`; all 126 files | `import tro`; `docs/engine-model.md` "Draw spans" | header verified; bodies open |
| Section 4 is `u32 length; u32 group_count; u32 offset[group_count]` (section-relative, 0 = absent) then `u32 count; count * 40-byte records`; groups tile the section. 125 files have 7 group slots, one has 6 | all 126 files | `import tro` | framing verified 126/126; record meaning open |
| Span selection `0x800117C4(section3, car+0x608 level, car+0x604 distance)`: wrap levels above `level_wrap` into `1..=level_wrap`, require `min_level <= level <= max_level`, then add one lap to the distance when `min_level < level` and test `start <= distance < end` (wrapping `end` when `end < start`). At the B-1 start (distance 190.2086 m, level 1) it returns the span with `node = 78`, equal to `car+0x600` | `0x800117C4` disassembly; `b1-gas-1200` RAM (`car+0x608 = 1`, `car+0x600 = 78`); `TC_lisence` section 3; engine `span_at` test | disassembly + numeric reconstruction | verified |
| Node offsets are file-relative and are relocated to RAM pointers (node 0 = `0x800B4FBC` = image + `0x588`); the two link words are zero in the file and the loader fills them from the `u16` indices (node 78: `+4 = 0x800CED68 = node[77]`, `+8 = 0x800D0384 = node[79]`) | file vs RAM at `0x800CF848` | byte-for-byte diff apart from the two link words | verified |
| Node record: `u16 linkA; u16 linkB; u32 addrA; u32 addrB; u32 flags; s32 distance(fx16.16 m); s16 scale; s16 scale; s32 x; s32 y; s32 z(16.16 m); s16 dx; s16 dy; s16 dz(4096 = 1.0)`; node 78: links 77/79, distance 157.9487 m, point (433.7085, 0, -449.6918), direction (-4096,0,0). The signed scale words span `-4096..=4096` (4096 = 1.0) | RAM; `TC_lisence.tro` | `import tro` parses all 124 nodes | verified for the fields listed; later records provisional |
| The 124 `u16` links at `+2` form one cycle from node 0 through all 124 nodes, in increasing course-distance order (122 of 123 steps increase; one wrap at node 74 → 75, 5188.9 m → 0.0 m); `+0` is the reverse neighbour. So `+2` is the next node along the course (node 78/79/80 are 52.64 m apart along -x at z 449.69) | RAM | ad-hoc graph walk over all 124 nodes | verified |
| Course distance `car+0x604` = `node[+0x10] + dot(position - point, direction)`; at node 78: 157.9487 + (433.7085 - 401.4431) = 190.214 m, against `car+0x604` = 190.2086 m | `0x80028C6C`/`0x80028588`/`0x80028394`/`0x80027BBC` disassembly; `car-traj2` samples | numeric reconstruction | verified (rounding only) |
| `car+0x65C/0x660/0x664` is the **world position** in 20.12 m, stored `(x, z, y)`; over 240 frames x falls 401.44 → 295.84 m while distance rises 190.21 → 295.81 m and z stays 449.69 m. The CSV's `fx16.16` format divides by 65536, so metres = CSV × 16 | `car-traj2/samples.csv` (columns `offA/B/C` at `0x800A9D10/14/18`); `0x8003CE3C` shifts each `<<4` before the GTE | per-frame delta comparison | verified; corrects the earlier "local offset" reading (a unit error) |
| `car+0x648` is heading at 4096 units/turn and is wrapped to [-0x1000, 0x1000) by `0x80033E6C`; it is 0x400 while the car drives along -x | `0x80033E6C` disassembly; `car-traj2` samples | — | verified value/wrap; zero direction provisional |
| `0x80033E6C` accumulates a 3-component delta into `car+0x65C/0x660/0x664` and a heading delta into `car+0x648` | `psx disasm --ram … --function 0x80033E6C` (`lw 0(a1)` … `sw v1,1628(a0)` loop; `lhu 1608(s3)`) | — | verified |
| `0x80043578` writes the node index from scratchpad `0x1F800008` into `car+0x600` (`lhu 8(t0)` at `0x800435F0`, `sw v0,1536(s3)` at `0x800435F8`, with `s3 = car + 0x2C`) | disassembly; the scratch-dump value at capture did not equal the stored index | — | writer verified; the scratchpad slot's meaning is provisional |
| The car record is 0xB40 bytes and the physics sub-struct starts at `+0x2C` (`0x800A9688 + 0x2C = 0x800A96B4`), which explains the two bases in callers | `0x80043388` (stride 0xB40) and `0x80043578` (`+0x2C`) | — | verified |
| Node bodies carry two 4-corner `(x,z)` 16.16 quads. The second, at `+0x6c` (`+0x6c/+0x70`, `+0x74/+0x78`, `+0x7c/+0x80`, `+0x84/+0x88`; each corner is 8 bytes apart, not 4), is the road surface on `TC_lisence`: each end pair's midpoint is the node's centreline point (max 0.49 m near, 0.52 m far over 124 nodes), the cross-section is perpendicular to the node direction, and the width is 19.45–22.58 m (22.58 m on the start straight). The two near corners are this node's cross-section, the two far corners the next node's; adjacent nodes' quads do not share an edge exactly (up to 2.0 m corner difference), so the original stores each quad independently. The first quad at `+0x44` is 0–73 m wide and offset up to 26 m; still undecoded. Across all 126 courses the same `+0x6c` field exists, but degenerate quads (all corners equal) and far-offset quads occur, so its role outside `TC_lisence` is not established. `+0x90` is a tag, `+0x92` a count (15 in node 78) and `+0x94` that many file-relative offsets into primitive/vertex lists | `TC_lisence.tro` node 78 at `0x1ae14`; measured over all 124 nodes; all 126 files | `import tro` extracts the quad; `game/tests/course.rs` checks the ribbon; `engine::course::surface_quads` | verified on `TC_lisence`; field present 126/126, meaning elsewhere open |

Reproduce the position trajectory (CLI psx 0.3.2; the MCP server in this
project is 0.3.0 and cannot load format-9 states):

```bash
psx run --root . \
  --from-state .psx/runs/b1-state1200/state.bin \
  --frames 240 --input '[{"frame":0,"buttons":["CROSS"]}]' \
  --sample 800A9CB4:u32:seg 800A9CB8:fx16.16:dist \
  --sample 800A9D10:fx16.16:offA 800A9D14:fx16.16:offB 800A9D18:fx16.16:offC \
  --sample 800A9D04:fx16.16:ang650 800A9CFC:fx16.16:ang648 \
  --out .psx/runs/car-traj2
```

The `*.psx` format is the trap: `offA/B/C` are printed as `fx16.16`, but the
fields are 20.12, so multiply the CSV values by 16 for metres (or sample them
with `i32` and divide by 4096). Extract the loaded course and compare against a
RAM dump:

```bash
psx run --root . --from-state .psx/runs/b1-state1200/state.bin \
  --frames 1 --out .psx/runs/course-dump
cargo run -p import -- extract \
  "extracted/Gran Turismo 2 (USA) (Simulation Mode) (Rev 2).cue" \
  --id 8322 --out /tmp/TC_lisence.tro.gz
gzip -dc /tmp/TC_lisence.tro.gz > /tmp/TC_lisence.tro
# section 0 header at 0x38c; node 78 at 0x1ae14; compare with RAM 0x800B4A34+0x1ae14
```

Bake the course and validate the format across every course on the disc:

```bash
cargo run -p import -- tro /tmp/TC_lisence.tro \
  --out game/assets/gt2/course/TC_lisence.ron \
  --name /crsobj/TC_lisence.tro
for f in /tmp/tro/*.tro; do
  cargo run -q -p import -- tro "$f" --out "/tmp/tro_out/$(basename "$f" .tro).ron"
done
cargo test -p engine course
```

The B-1 baked course's `span_at(190.2086, 1)` returns node 78, matching
`car+0x600` in the race RAM, and every course's sections and records account
for every byte (126/126, `import tro` refuses a mismatch).

The replays reproduce the recordings exactly: re-running
`title-to-gtmap.json` from the same state gives the same 2 MiB RAM image
byte-for-byte as the original inline-input run (both
`8c7bcc2a…`), so the checkpoint is a usable fidelity baseline.
