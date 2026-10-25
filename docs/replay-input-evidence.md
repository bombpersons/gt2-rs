# Recorded replay inputs

`race::replay` feeds native recorded driver requests into the unchanged vehicle
rules. `ReplayClip` is ordinary RON containing runs of named digital controls;
`ReplayPlayback` owns the per-car cursor. The game does not read original packet
bytes, memory layouts or emulator states. The automatic result-to-replay reset,
replay cameras/menu and return controller remain separate work.

## Original execution and data boundary

The original records through `800166CC`, flushes a changed/final run through
`80016598`, and finalizes at `800167D0`. Playback uses `80016428`, followed by
`80013C90` to normalize the requests. `80013DAC` records the player's requests;
`80013DC0` plays them. The licence recording closes after sixty post-result
fields (`80013D78..98`), independently of the accelerated result-screen timer.

The finalized header contains recorded duration and encoded payload length.
Each packet changes a subset of four retained channels and holds them for a
variable number of ticks. Its long duration uses up to four prefix-coded
bytes, plus the command's low three bits. `tools/import_replay.py` confines
that original-format interpretation to offline conversion. It checks the
finalized flag, payload bounds, truncation and total duration; analog/special
selectors are rejected explicitly because the native car's analog request
producer is not ported. Digital output uses named pedal, steering, reverse and
shift requests. Native clips can be authored or coalesced with `ReplayClip::record`.
The separate [automatic recorder](replay-recording-evidence.md) implements the
original bounded storage and finalization policy for live digital attempts.

The original decrements its saved duration **before** applying input. Its
terminal call clears remaining hold state, marks completion and leaves the
caller's neutral output intact (`80016430..458`). The last recorded sample is
therefore not applied as another driving tick. Later inactive calls remain
neutral. The native cursor preserves that boundary and emits one
`ReplayInputsFinished` event. Removing its component returns input ownership
to the ordinary player reader.

## Executed checks

`tools/record_replay_inputs.py` executes the original decoder and normalizer
under `psx 0.4.2+src-4c4c84345897a7b9-release`. Write watches and instruction
boundaries retain the actual effective driver outputs, remaining duration and
hold counter; expected steering/pedal/gear requests do not come from another
implementation of the importer's packet mapping.

Ten initial cases cover 1,359 calls: a complete 1,247-call natural B-1 recording,
short holds, every duration-prefix width, retained channels, simultaneous
buttons and the terminal/inactive boundary. Two fresh packet schedules add
sixteen calls after implementation, including different steering, pedal,
reverse and shift combinations. All output requests, remaining duration,
completion state and held-tick counts match exactly. The importer reproduces
native RON runs whose recorded duration matches the source header. Deliberately
truncated, unsupported-selector, duration-mismatched and over-capacity inputs
are rejected.

The decoder checkpoint is in active replay, created from the licence result
wait (the preceding checkpoint recipe is in [award evidence](licence-award-evidence.md)):

```sh
psx run --from-state .psx/awards/result-menu/state.bin --frames 40 \
  --input '[{"frame":0,"buttons":["CROSS"],"until":6}]' \
  --save-state --out .psx/results/replay-start --summary
python3 tools/record_replay_inputs.py
python3 tools/record_replay_inputs.py --fresh
python3 tools/import_replay.py --ram .psx/awards/gold-session/ram.bin \
  --out game/assets/race/replay-inputs.ron
cargo test -p gran_turismo_2 --all-features --test replay
```

Recordings, original packet bytes, RON clips and emulator outputs remain in
ignored asset/reference/analysis directories. The scripts retain commands,
executing builds, reports and checkpoint identities.

Four existing retained vehicle trajectories now also run through
`ReplayInputRulesPlugin`, with conflicting physical pad input held. Moving
mixed pedals/steering, an idle mixed-control holdout, manual shifts and a
countdown launch still match every sampled state at four physics boundaries
with zero tolerance. Native state is initialized only once; later snapshots
are assertions. A handmade rules-only case checks RON recording/coalescing,
physical-input override and one neutral completion event.

The actual binary also drives the complete imported natural B-1 input track
through native countdown/vehicle/stopping rules with conflicting live input.
After 1,250 native ticks it matches the recording's original 38.675-second
result and final body position (within 0.0001 metres per axis), and the input
cursor is completed at 1,247 calls. Its rendered Finish screen was inspected.
This uses the source recording's own final RAM/report, not a different driving
fixture with a different initial input schedule.

```sh
cargo test -p gran_turismo_2 --all-features --test vehicle_loop recorded_native
cargo test -p gran_turismo_2 --all-features --test dev_mcp native_original_input_clip -- --ignored
```

## Native scene and extension contract

Optional `RaceVehicle.replay_inputs` names a layered native clip asset. Its
vehicle initial state and countdown must describe the same starting boundary
as the clip. An absent path selects ordinary live input. The replay plugin is also a
separate `GamePlugins` entry, usable on an existing car with the three digital
input components. It applies recorded requests after physical input sampling
and before transmission requests/pedal/steering updates; camera and front-end
pad input remain separately owned. Mods can supply native recordings without
using anything from the disc.

This establishes input-track playback and its vehicle integration. Normal
result confirmation still reaches `LicenceReplayRequested`; it does not yet
reset the car or attach a recording from the just-completed attempt. Analog
input support, restart/reset, replay camera/HUD, pause/exit and the return
event that commits awards remain. Live digital recording and the original
capacity/overflow behavior are covered by the automatic recorder.
Persistent replay files and saves are also unfinished. Nothing here applies
awards at race finish or bypasses the original replay/return boundary.
