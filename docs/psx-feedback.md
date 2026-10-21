# `psx` toolkit feedback

Issues and friction from using `psx` to port Gran Turismo 2 (USA, Simulation
Mode, Rev 2). Each entry says what I ran, what happened, what I expected, and a
suggestion. Tool vs. documentation problems are marked.

Environment: `psx` 0.1.0, build `src-6406590b7cd127b1-release`,
`x86_64-unknown-linux-gnu`; Ghidra 12.1.4 at `/home/maisy/tools/ghidra_12.1.4_PUBLIC`,
OpenJDK 21; project root is this repository.

## 1. Ghidra project directories land in the working tree (medium)

Repro:

```sh
psx ghidra functions --source .psx/exe/SCUS_944.88 --limit 1
ls ghidra-projects/            # 2 project dirs, 9.9 MB
```

What happened: `ghidra-projects/<hash>-<id>-<n>/` is created in the **project
root**, next to `engine/`, `game/` and `tools/`. The analysis manifest, by
contrast, is written under `.psx/ghidra/<hash>/`. I had to add
`ghidra-projects/` to `.gitignore` so it does not join the repository.

Expected: everything the toolkit generates under `.psx/` (as the tool
descriptions promise: "outputs go to its `.psx/`"), or at least a flag to put
the Ghidra project tree somewhere else. There is no such flag in
`psx ghidra --help` / `psx ghidra analyze --help`.

Suggestion: default to `.psx/ghidra/projects/`, and expose
`--ghidra-project-dir` for people who want to keep it or share it.

## 2. `ghidra functions --filter` also matches hex addresses (low, doc vs tool)

The help for `--filter` says "Case-insensitive substring of function name".

Repro:

```sh
psx ghidra functions --source .psx/exe/SCUS_944.88 \
    --symbols .psx/symbols/SCUS_944.88.symbols.json --filter 800100 --limit 3
# returns FUN_80010000, FUN_800100c0, FUN_800100e4
psx ghidra functions ... --filter Cd --limit 10
# returns FUN_8006cd04, FUN_8006cdcc, FUN_8008bcd8, ... — matches "cd" in the address
```

What happened: I filtered for `main`, `Cd`, `Pad`, `VSync`, `Spu`, `Init` to
find the interesting functions quickly; `Cd` returned ten functions whose
*addresses* contain "cd", and `main` returned none, which briefly convinced me
that the `--symbols` I passed were not being applied. They were: a follow-up
`--filter VSync` returned `VSyncCallback` and `_padIsVsync` with
`name_source: USER_DEFINED`.

Expected: filter matches names only, as documented.

Suggestion: match names only, and if address matching is useful, add
`--filter-addr` or document the current behaviour in the help text.

## 3. No way to read a byte window inside a disc file (medium)

I needed about 1 KB at known offsets inside `GT2.VOL` (a 488 MB file) while
reverse-engineering the `GTFS` descriptor: its first `0x800` bytes, the
descriptor's offset array, and the 32-byte name records at file offset
`0x597aa` (inside index file 1).

Repro of what I had to do:

```sh
psx extract "...cue" /GT2.VOL -o /tmp/opencode/vol     # writes all 488 MB
# then read a window from the extracted file
```

`psx extract` has no offset/length, `psx sector` addresses by LBA not by a file
offset, and `artifact-read` works on host files.

Two things made this worse:

- **No file-offset addressing.** `image.cue::/PATH` exists as a *source* for
  several tools but always means the whole file. `image.cue::/PATH@0x66b22+64`
  (or `--offset`/`--length` on `extract`, or a `psx read`) would have avoided a
  488 MB extraction.
- **No file-offset → physical-offset helper.** A raw mode-2 image has 24 bytes
  of header per 2352-byte sector, so file offset `O` is at physical
  `(O/2048)*2352 + 24 + O%2048`, not `O`. Reading the raw `.bin` at `base + O`
  gave bytes that still looked like structured data (a name table with a clean
  32-byte stride, just the wrong region), which sent me down a false path for a
  while. `psx addr` classifies CPU addresses; the disc equivalent would be
  useful. `psx sector` prints "owning file" but takes an LBA, not a file offset.

Suggestion: accept `@OFFSET[:LENGTH]` on `image.cue::/PATH` wherever a source is
taken (at least `artifact-read`, `extract`, `assets_scan`), and add
`psx sector --image cue --file-offset O` (or a small `psx read` verb) that
prints the physical offset, sector and header bytes.

## 4. No signal for an unrecognized container (low)

`psx assets scan "...cue::/GT2.OVL"` reports `1 files inspected` and
`total: 0`, which is correct — no TIM/TMD/VAB/VAG — but it says nothing about
the file being a container at all.

`GT2.OVL` is 289 752 bytes: a `u32 0x30` table of per-member compressed sizes
with each next member's offset (always the 4-byte-aligned end of the previous
one), then **six gzip members**, every one inflating to a MIPS overlay linked
at `0x80010000`. I found this by hand, and (with a first, wrong table reading)
spent a while on "gzip-shaped header that will not inflate" before re-reading
the table as sizes rather than offsets.

Suggestion: when a file's leading words form a monotonic
`[size, next-offset, ...]` table that tiles it (`last offset + last size ==
file size`, modulo a few alignment bytes), say so in the scan output —
"possible container: 6 sections" — and when a section starts with the gzip
magic, say "gzip member (inflates to N bytes)" rather than leaving it as
`total: 0`. Trying to inflate each section and reporting the result would have
settled it immediately.

## 5. `disc` output has two different meanings of "files" (low)

`psx disc` returns both `files` and `media_files` (identical: the `.bin` on
disk) *and* `entries` / `filesystem` (the ISO9660 entries). Reading
`files: [ {name: "...bin"} ]` next to `entries: { files: 6 }` is confusing; I
initially missed that the disc holds six files.

Suggestion: drop `files` (it duplicates `media_files`), or rename the two
clearly: `media_files` vs `iso_entries`.

## 6. Follow the crt0 stub to the game's main (low, feature)

`psx exe` / `psx disc` report the entry point (`0x8005D600`), which is a crt0
stub: it saves registers, calls libc init and then the real main at
`0x8005D6E0`. I only found main by disassembling 0x40 instructions and
following the `jal`.

Suggestion: for a PS-X EXE, note the first `jal` target after the register-save
prologue as "main candidate: 0x8005D6E0", or add `psx exe --follow-entry`.

## 7. License test entry stalls (high, possible CD sector-size/interrupt gap)

Repro, from the recorded title state (see `docs/checkpoints/README.md`):

```sh
psx run --media <cue> --frames 2100 --save-state-at 2100     # title state
psx run --from-state <state_f002100.bin> \
        --replay docs/checkpoints/title-to-license-list.json --frames 900 --save-state
# then press CROSS at the B-License list:
psx run --from-state <that state.bin> --input '[{"frame":10,"buttons":["CROSS"],"until":16}]' \
        --frames 600 --save-state-at 600 --trace-cd
```

What happened: the confirming `CROSS` blanks the screen and it stays black for
thousands of frames (button presses do not advance it). `--trace-cd` shows the
game issuing the same transfer every ~17 frames — 12 bytes from LBA 129726,
`sector_size 2340`, `pc 0x8008A534` (inside the CD/DMA handshake
`FUN_8008a488`). LBA 129726 is inside volume resource 11357,
`/gtmenu/commonpic.dat` (the 44.7 MB menu image archive), and `psx native`'s
CD history shows the driver looping
Pause→Setloc(LBA 129726)→Setmode(**0xA0**, double speed + 2340-byte sectors)
→ReadN→Getstat×2→GetlocL every ~8 frames, re-seeking the same MSF — `ReadN` is
acknowledged (INT3) but no data-ready interrupt is seen. The game never
submits another GPU draw command: `--gpu-next-draw` finds nothing in 120
frames **even with `--gpu-draw-filter any`**, so the framebuffer stays black
while the vblank counter `0x801F0680` keeps advancing +1/frame and the game's
own counter `0x800B1228` stays 0. `pc`/`cop0_epc` sit in ordinary PsyQ
`syscall` stubs (`cause 0x20`), i.e. the CPU is running, not halted.

Expected: the license test loads a short course and a car and shows a "Start"
screen, as it does on hardware.

Full standalone report to pass on: `docs/psx-bug-license-test-stall.md`.

Suggestion: I suspect the CD core does not deliver sectors (or raise the
data-ready interrupt) for `Setmode 0xA0` / 2340-byte sectors, so the driver
retries forever. See that document for the diagnostics I would want. The extra
tooling grief: the run reported `complete` / `frames_reached` while presenting
no frame at all — one line in the report ("no GPU draws in N frames; CPU in a
CD wait loop"), or surfacing the repeated CD command without `--trace-cd`,
would have saved hours of manual RAM/register digging.

## 8. No way to analyse a raw code image at a base address (medium, feature)

The game's overlays are not files on the disc: `GT2.OVL` holds six gzip
members that inflate to MIPS images linked at `0x80010000`, with no header.
Every analysis entry point wants a PS-X EXE or `disc.cue::/PATH`:

```sh
psx ghidra analyze --source .psx/exe/SCUS_944.88          # ok
psx disasm --source docs/overlays/overlay_04.bin          # no base, no header
```

I got there by hand-writing a 0x800-byte PS-X EXE header
(`"PS-X EXE"`, `pc0 = t_addr = 0x80010000`, `t_size = image length`, code at
0x800) and passing that, which is a thing every PS1 reverser ends up doing.

Suggestion: let `--source` name a raw image plus a base —
`psx ghidra analyze --source foo.bin --load 0x80010000` (or
`foo.bin@80010000`) — and accept the same spelling in `disasm`. A one-line
"this is not an executable, but if you tell me where it is linked I will treat
it as code" would cover overlays, BIOS modules and homebrew images.

## 9. Notes (no action needed)

- `psx symbols` writes the names to `.sym` / `.ghidra.txt` and its JSON has no
  symbol array; the JSON's `files` field points at them, so this is just worth
  saying explicitly in the tool description.
- `psx run --replay` reproduced an earlier inline-input run's 2 MiB RAM image
  byte-for-byte (same SHA-256), which is exactly the fidelity baseline the port
  needs.
- `psx ram search --pred increased --width 4` over per-frame snapshots found
  per-frame counters in one call; `--save`/`--candidates` for narrowing is
  well designed.
- The `guide` topics (`port`, `run`, `input`) answered nearly every "how do I"
  question I had without a round trip.

## 10. Follow-up: issue 7 fixed in 0.3.2 (verified)

The license-test stall is gone on `psx` 0.3.2, CLI build
`src-659f5da213e93fb5-release` (binary dated 2026-10-19). The same route that
stalled now loads the B-1 test: the screen returns after a bounded load and the
session runs; the verified route is in `docs/checkpoints/README.md`. The
suggested "no GPU draws in N frames" warning was not exercised this time
(fix first), so that suggestion stands on its own.

One migration trap worth a line in the changelog: saved states are not
portable across this fix — `psx run --from-state` on a 0.1.0-era state fails
with `save state format 8 is not supported (this core writes format 9)`. The
error message is clear; rebuilding the chain from power-on took one command.

Also observed while verifying: the `psx` MCP server in this environment kept
reporting build 0.3.0 (`src-a3beb4f8`) and kept stalling after the CLI on
`PATH` was 0.3.2 — a stale server process serving an updated CLI. Worth either
bumping the server with the CLI or making `context`'s build block visibly
differ when the server binary and the `psx` on PATH disagree.
