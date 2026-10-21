# psx (native core) bug report: entering a Gran Turismo 2 driving session never leaves the load

Status: **fixed and verified** — confirmed working on `psx` 0.3.2 (CLI build
`src-659f5da213e93fb5-release`, 2026-10-19): the same `CROSS` now loads the B-1
test, the screen returns after a bounded load (~150-450 frames), and the driving
session runs (`docs/checkpoints/README.md` has the verified route). Two notes
for anyone re-running the old evidence:

- Saved states from the 0.1.0-0.3.0 era do not load on 0.3.2 (`save state
  format 8 is not supported (this core writes format 9)`); rebuild checkpoints
  from power-on.
- The MCP server in this environment still reported build 0.3.0 after the CLI
  was updated, so runs issued through it kept stalling — check the `build`
  block of a run's `report.json` when judging a fix.

The original report follows, as filed against `psx` 0.1.0.

---

Found while porting Gran Turismo 2 (USA, Simulation Mode, Rev 2). This blocks
all in-race evidence collection — no lap, no car, no course can be recorded
until it is resolved.

## Summary

Confirming the first B-License test in the game blanks the screen and it never
comes back. The run reports success (`status: "complete"`,
`termination: "frames_reached"`) even though nothing is ever drawn again.

The game is **not** crashed. The vblank counter keeps advancing once per
frame, the CPU keeps executing game code, and the CD controller keeps
receiving commands. But the game never submits another GPU draw command, so
the display stays black for thousands of frames and no input advances it.

The CPU is stuck in a CD load loop: it repeatedly re-seeks the same absolute
sector, issues a read, checks status, and tries again, forever.

## Environment

- `psx` 0.1.0, build `src-6406590b7cd127b1-release`, `x86_64-unknown-linux-gnu`.
- BIOS sha256 `11052b6499e466bbf0a709b1f9cb6834a9418e66680387912451e971cf8a1fef`.
- Disc: `Gran Turismo 2 (USA) (Simulation Mode) (Rev 2).cue`, one `mode2_raw`
  track, 294154 sectors, boot `/SCUS_944.88`
  (EXE sha256 `4dd40d01a3e83967e2d4301106890eb314d72027802bee077bbbc246f152e331`),
  volume id `GRANTURISMO2`. Single data track, no audio track.

## Reproduce (one command, from power-on)

Write `replay.json`:

```json
[
  {"frame": 2130, "buttons": ["CROSS"]},
  {"frame": 2136, "buttons": []},
  {"frame": 2710, "buttons": ["RIGHT"]},
  {"frame": 2716, "buttons": []},
  {"frame": 2800, "buttons": ["CROSS"]},
  {"frame": 2806, "buttons": []},
  {"frame": 2920, "buttons": ["CROSS"]},
  {"frame": 2926, "buttons": []},
  {"frame": 3010, "buttons": ["CROSS"]},
  {"frame": 3016, "buttons": []}
]
```

`frame 3010` is the confirming CROSS on B-License test 1; the earlier presses
are just the route into the License Test menu. (The same file is committed in
the port's repo as `docs/checkpoints/poweron-to-license-test-stall.json`.)

```sh
psx native \
    --disc "Gran Turismo 2 (USA) (Simulation Mode) (Rev 2).cue" \
    --frames 3600 --replay replay.json \
    --dump /tmp/gt2-stall
```

`/tmp/gt2-stall.screen.png` is black. With `--snapshot-every 600`, the frame
3000 snapshot is the B-License list and the last one is black.

## Reproduce (from a saved state, faster for iteration)

```sh
# 1. title state
psx run --media "<cue>" --frames 2100 --save-state-at 2100

# 2. recorded route: Start Game -> RIGHT to LICENSE -> CROSS -> CROSS,
#    leaving the cursor on B-License test 1
psx run --from-state <title state_f002100.bin> \
        --replay title-to-license-list.json --frames 900 --save-state

# 3. confirming CROSS on test 1
psx run --from-state <list state_f000900.bin> \
        --input '[{"frame":10,"buttons":["CROSS"],"until":16}]' \
        --frames 600 --save-state-at 600 --trace-cd
```

The step-2 replay is four events: `CROSS` at frame 30 (Start Game), `RIGHT`
at 610, `CROSS` at 700, `CROSS` at 820 (into License Test → B-License); it
leaves the cursor on test 1.

## Observed

1. Screen before the confirming CROSS: the B-License list. After it: black,
   permanently. Button presses do not change it.

2. No GPU work at all. From a state captured 600 frames after the CROSS,
   `--gpu-next-draw` over a 120-frame window returns `status: "not_found"`
   with `frames_inspected: 120` — **even with `--gpu-draw-filter any`**, so
   there is not a single fill, clear or primitive. The GPU is idle.

3. The machine is still running. Sampling from the same state:

   ```text
   run_frame,frame,vblank,gameframe
   0,3600,2517,0        <- 0x801F0680 increments +1 per frame
   1,3601,2518,0
   ...
   20,3620,2537,0       <- 0x800B1228 (game's own frame counter) stays 0
   ```

4. `--trace-cd` from the same state logs an identical transfer roughly every
   17 frames:

   ```json
   {"bytes":12,"frame":3603,"lba":129726,"pc":2148050228,"ram":2148174628,"run_frame":3,"sector_size":2340}
   {"bytes":12,"frame":3620,"lba":129726,"pc":2148050228,"ram":2148174628,"run_frame":20,"sector_size":2340}
   {"bytes":12,"frame":3637,"lba":129726,"pc":2148050228,"ram":2148174628,"run_frame":37,"sector_size":2340}
   ```

   `pc 2148050228 = 0x8008A534`, inside `FUN_8008a488`, a CD handshake routine
   (the decompile shows it polling a status word at `0x800A77E4` for bit
   `0x40`, then a completion bit).

5. `psx sector --lba 129726`: owner `/GT2.VOL`, `form1`, user data begins
   `47 54 4d 50` = `"GTMP"`. LBA 129726 is inside volume resource 11357,
   `/gtmenu/commonpic.dat` (span 220657664..265381888, 44724224 bytes).

## CD controller history (`psx native`, frames ~3570-3595)

The loop, every ~8 frames:

```text
frame 3578  09 []          -> INT[3,2]   Pause
frame 3578  02 [28,51,51]  -> INT[3]     Setloc (BCD 28:51:51 -> LBA 129726)
frame 3578  0E [A0]        -> INT[3]     Setmode 0xA0
frame 3578  06 []          -> INT[3]     ReadN
frame 3578  01 []          -> INT[3] x2  Getstat
frame 3578  10 []          -> INT[3]     GetlocL
frame 3586  09 []          -> INT[3,2]   Pause
frame 3586  02 [28,51,51]  -> INT[3]     Setloc -> LBA 129726 again
frame 3586  0E [A0]        -> INT[3]
frame 3586  06 []          -> INT[3]     ReadN
frame 3586  02 [08,40,59]  -> INT[3]     Setloc -> LBA 38909
frame 3586  0E [A0]        -> INT[3]
frame 3586  06 []          -> INT[3]
...
```

- `Setmode 0xA0` = bit 0x80 (double speed) + bit 0x20 (sector size 2340). The
  trace's `sector_size: 2340` lines up with that.
- `Setloc 28:51:51` is LBA 129726 (`/gtmenu/commonpic.dat`); the other Setloc,
  `08:40:59`, is LBA 38909 = resource 7961 `/gtmenu/usa/usa_gtmode_race.dat.gz`
  (a gzip stream, `FNAME usa_gtmode_race.dat`).
- Every command is acknowledged, but the `ReadN` never shows a data-ready
  interrupt in the history — the driver re-seeks the same MSF instead of
  reading on. `unknown_commands: []`, `xa_sectors: 0`,
  `sectors_delivered: 1801` (accumulated since power-on, not during the loop).

## What it is not

- **Not a disc-swap prompt.** The disc is a single `mode2_raw` track with no
  second disc or audio track to switch to, and the loop reads the game's own
  volume files, not a "insert disc" retry.
- **Not a crash or a bad jump.** The CPU keeps running game code (last PC
  inside the game, `0x80081D0C`); `cop0` shows it taking ordinary PsyQ
  `syscall` exceptions (cause 0x20) in the critical-section helper
  `FUN_80081cf8`, which is normal, not a fault. No unmapped access or
  unhandled DMA is reported (`unmodelled_dma: 0`; one benign unhandled read of
  `0x1F801130`).
- **Not a renderer/VRAM problem.** `vram.png` is full of normal game textures;
  only the framebuffer is black, because no draws are submitted.

## Where the bug most likely is

The sequence is a textbook "pause, setloc, set mode, read, check, retry"
driver. The fact that it re-issues the *same* Setloc every cycle means the
read is not producing acceptable data, so the load routine never returns and
its frame's draw is never reached.

Two details differ from an ordinary load and are the natural suspects:

- the read is issued with `Setmode 0xA0`, i.e. 2340-byte sectors at double
  speed, not the default 2048;
- no data-ready interrupt is observed for that `ReadN`.

I cannot see inside the core from here, but the most likely failure is in CD
sector delivery and/or the interrupt raised for the 2340-byte sector-size
mode, which makes the driver retry forever. If the core delivers the sector
but the game still retries, then it is the interrupt timing or the sector
header/subheader the core synthesises.

## Suggested diagnostics and fixes

1. For each CD command, log whether the data-ready interrupt is raised, how
   many bytes the DMA actually moves, and (when it differs from 2048) the
   sector-size bit. A "Setmode changed sector size to 2340" line plus a
   "ReadN acked, no data-ready in N frames" line would have pointed straight
   at this.
2. Compare the same read at `Setmode 0x80` / `0x00`.
3. Do not report such a run as `complete`. When the game presents no frame for
   the whole window, surface it: add a `warnings`/`status` line like
   "no GPU draws in the last N frames; CPU in a CD wait loop", and include the
   repeated CD command + LBA + retry period even without `--trace-cd`. Right
   now a run that renders nothing looks exactly like a successful one.

## Notes for reproducing my reading of the disc

- `GT2.VOL` starts at LBA 473 (`GTFS`); resource 11357 `/gtmenu/commonpic.dat`
  spans volume bytes 220657664..265381888 (`GTMP` magic), so LBA 129726 is
  43.0 MB into that 44.7 MB file.
- The `GTFS` entry table starts at `GT2.VOL + 0x10`, is monotonic, and ends at
  a sentinel equal to the volume size (`0x1d19f800`), giving 11580 resources.
  Absolute LBA = 473 + (resource byte offset / 2048).

## Attachments

The run directories referenced above (not part of any repository) hold:
`screen.png` (black) and `vram.png` before/after, `cd_native.jsonl` (the
18-second trace), the saved `state_f000600.bin`, and `report.json`. The
`psx native` output above carries the full CD command history and hot PCs.
