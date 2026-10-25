# Reference checkpoints

Replayable input for the original, recorded with `psx run`. Replays are the
`--replay` format: a JSON array of complete held pad states, frame numbers
relative to the run's start.

Regenerate a run from the project root (the disc is not committed):

```sh
psx run --media "extracted/Gran Turismo 2 (USA) (Simulation Mode) (Rev 2).cue" \
        --frames 2100 --snapshot-every 300 --save-state-at 2100
psx run --from-state .psx/runs/<id>/state_f002100.bin \
        --replay docs/checkpoints/title-to-gtmap.json --frames 900 --snapshot-every 100
```

| File | Starts from | Frames | Observed |
|---|---|---|---|
| `title-to-gtmap.json` | the frame-2100 title state | 900 | `CROSS` at frame 30 selects "Start Game"; the GT Mode map settles by ~600 |
| `title-to-license-list.json` | the frame-2100 title state | 900 | `CROSS` starts the game, `RIGHT` moves the map cursor from HOME to LICENSE, two `CROSS`es enter License Test → B-License, leaving the cursor on test 1 |
| `poweron-to-b1-list.json` | power-on | 3000 | the title → B-License list route as one run from power-on; save a state at 3000 (`--save-state-at 3000`). Same presses as `title-to-license-list.json`, with absolute frames |
| `poweron-to-b1-list-current.json` | power-on, 0.4 core | 4800 | longer loading/menu timings verified on 0.4.2; the earlier replay enters Home on this core |
| `b1-list-to-b1-menu.json` | the frame-3000 list state | 600 | `CROSS` selects B-1; the test's own menu (`Replay / Start / Records / Save Replay / Demonstration / Exit`, car: Toyota Vitz F '99) is up by ~450 |
| `b1-menu-to-transmission.json` | the B-1 menu state | 60 | `CROSS` on `Start` opens the `TRANSMISSION` dialog, cursor on `AT` |
| `transmission-to-race.json` | the transmission-dialog state | 900 | `CROSS` confirms `AT`; load (black) ends by ~300, the test scene is up with the timer running from ~370; with no further input the car stays on the line at 0 mph while the clock runs |
| `poweron-to-license-test-stall.json` | power-on | 3016 | the old psx 0.1.0 stall repro (same route plus the confirming `CROSS`); kept for history — on psx ≥ 0.3.2 the same press opens the B-1 menu (see `docs/psx-bug-license-test-stall.md`) |

The driving chain is `poweron-to-b1-list` → `b1-list-to-b1-menu` →
`b1-menu-to-transmission` → `transmission-to-race`, each hop from the previous
hop's saved state; it was verified end to end on psx 0.3.2 and ends in the
live B-1 session with the lap clock running. To drive, hold `CROSS`
(accelerator) from ~frame 400 of the last hop: the car launches at the timer's
start and the AT shifts 1→2 around ~35 mph.

The table's original short route describes the 0.3 core. With 0.4, use the
current power-on replay, then allow 1,800 fields after selecting B-1 and 1,500
after selecting Start. Confirm each at relative fields 10..22. The asset
exporter observes initialized vehicle state and countdown samples after
confirming AT, avoiding a fixed capture inside the longer load. See
[automatic stopping evidence](../automatic-stop-evidence.md) for the fresh
post-result checkpoint recipe. Old format-9 states cannot be loaded by the
current core; their RAM/trajectory fixtures remain valid native references.

For current idle AT/MT checkpoint recipes and complete retained launch
recordings, see [driver-request evidence](../driver-request-evidence.md).
Those checkpoints reach vblank 7050 with the licence clock already running;
they exercise ready-to-drive idle launches, not the earlier countdown.

[B-1 countdown evidence](../session-start-evidence.md) adds the vblank-6550
checkpoint recipe and natural countdown-to-launch recordings. It also explains
the optional native session content setup and the remaining HUD/session gaps.

A non-driving submenu is reachable and does not stall: the GT map's city
icons open 2D dealership maps. West City is HOME → LEFT (GO RACE) → DOWN
(MACHINE TEST) → LEFT (WEST CITY) → CROSS; its Alfa Romeo dealer (`LINE UP`,
`TUNE`, `SPECIAL`, `EVENT`) lists new cars from 11 860 credits up, all above
the starting 10 000, so a car cannot be bought from the recorded state.

`docs/evidence.md` lists the addresses and counters these runs support.
