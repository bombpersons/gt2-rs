# Licence stopping-zone decisions

The native B-1 car now latches a completed stopping test or an overshoot /
off-road failure. Its lap clock retains the completed time or clears on
failure. Awards, result artwork, automatic post-result driving and retry are
still separate work; this does not yet complete the licence route.

## Original decision boundary

`8003ECB8` calls `8003D5F8` after physics/contact updates and the 3 kHz
session clock. It ignores a previously completed/failed attempt. Source
contact fault bit 2 takes precedence over bit 1, then it checks the selected
stopping test. B-1's configuration has kind 2 at `801C98A2`, start distance
1000 m at `801C98C4` (stored in tens of metres), and length 26 m at the next
byte. The exporter writes these as ordinary metre bounds in
`race/licence-stop.ron`, along with the source's strict stopping-speed limit.

The bounds apply to the wheels, rather than the centre of the car:

| Rule | Original evidence |
|---|---|
| Project each wheel position onto the course | `8003D498..5C4`, using `80028C6C` / `80028588` |
| Unwrap the final eighth of the lap behind its origin | `8003D4F4..590` |
| Fail if the front-most wheel is strictly beyond the zone end | `8003D66C..698` |
| Continue if the rear-most wheel is below the zone start | `8003D69C..6AC` |
| Require approximate world speed strictly below 1138/4096 m/s | `8003D6B0..6CC` |
| Reject a stopped car if any wheel's surface id is at least 2 | `8003D458..494`, called at `8003D6D4` |
| Latch a completed time after sub-tick correction | `8003D6F8..79C` |
| Store the resulting outcome/time for the session | `8003D244`, `8003D2A0`, then `800156EC` |

`8003C398` estimates speed as the largest absolute velocity axis plus a
floored quarter of the other two axes. At this boundary, speed is sampled
in 1/4096 m/s. It is not a Euclidean norm. The time rule adds
`floor(speed * 100 / 1138)` to the 3 kHz clock, divides by three to whole
milliseconds, then subtracts the run's time origin. This correction changes
the visible result and its medal classification, so its rounding is retained.

The source fault flag at physics `+0x6FA` is produced outside this routine.
All four wheels off-road or missing can set bit 2 at `80043888`. Collision
response can set bit 1 at `80034604`; the native collision producer is still
unported. The native API accepts an explicit collision fault for that future
system and derives the ordinary all-wheel off-road fault from road contact.
The additional surface check at a stop remains independent of those faults.

## Course projection

The engine now provides `CourseData::distance_at_position`. It finds the
longitudinal node, projects onto its near and far boundary planes, weights
the two distances, and interpolates the cumulative course distance. The
previously unnamed node `scale` values are the far and near weights,
respectively (`80028614..710`).

Imported cumulative node/lap distances use `f64` metres. This retains the
source's sampling precision for a wheel exactly on a test boundary; render
positions and surface ribbons still use `f32`. Optional `DistanceSampling`
stores the projection and interpolation grids as ordinary metre/fraction
steps. Authored courses can omit it for continuous projection. An authored
engine test checks both paths and unequal plane weights without game data.

## Recordings and native boundary

`tools/record_licence_stop.py` records each call's inputs, the original's four
wheel projection returns, and its outcome/time writes. Expected decisions
come from executed original code. It also observes effective digital controls
after their producer, separately from raw controller events. The original
serial-pad transport applies these requests one physics tick later; native
replay consumes the observed logical controls, as the existing vehicle-loop
checks do.

The initial cases are an early stop, a successful stop, an overshoot and an
off-road run. A further braking run was captured after the initial rules
passed. Together they cover **5,800 original decisions** and **22,616 wheel
projection returns**. All projection results match at the original 1/65536 m
output grid, and all decisions/latched times match. Notable natural results:

| Case | Original transition (fields from checkpoint) | Result |
|---|---:|---|
| Brake at field 2200 | No completion through 2600 | Stops short of the zone |
| Brake at field 2210 | 2434 | Completed, 38.641 seconds |
| Continue accelerating | 2356 | Overshoot failure |
| Accelerate with right steering | 1468 | Off-road failure |
| Fresh brake at field 2220 | 2444 | Completed, 38.811 seconds |

The real-binary MCP checks replay all five routes from the normal native
standing-start assets, supplying only the observed logical requests. They
query immediately before/after the original transition and at the end.
All five pass with exact outcome timing and completed milliseconds; the
successful clock is retained and the failed clock is blank. The steering
route also checks native positions before the result. No original snapshots
are injected during these native drives.

An additional unmodified idle survey continued to field 3000 with no failure.
The fourth medal/award target is therefore not by itself a timeout condition.

`LicenceStopPlugin` runs without a renderer or vehicle plugin. Public
`LicenceStopSettings`, `LicenceStopInput` and the typed `LicenceRun` component
are its contract. `LicenceParticipant` opts a native car into the course/road
input bridge. The normal B-1 scene adds it when stopping-test settings exist.
The rules run after the session clock and before HUD presentation. An authored
test checks optional session ownership, inclusive bounds and latched results.
The HUD's initial visibility is independent of its clock string: failure
removes the clock sprites while keeping the label. The native checks count
actual remaining HUD glyph entities after rendering, catching stale sprites
as well as an incorrect display value.

The long steering replay also exposed an earlier contact-motion integration
gap: an unloaded wheel was being synchronized to rolling speed. Retaining the
pre-force load gate restores all 734 pre-result physics ticks; see the
[contact-input evidence](contact-input-evidence.md#current-drivebrake-synchronization).

```sh
python3 tools/record_licence_stop.py
# Fresh recording after the first comparisons pass:
python3 tools/record_licence_stop.py --case holdout
cargo test -p gran_turismo_2 --all-features --test licence_stop
cargo test -p gran_turismo_2 --all-features --test dev_mcp native_licence_stop \
  -- --ignored --nocapture --test-threads=1
```

All RAM, traces, samples, fixtures and screenshots remain ignored. Runtime
code loads native RON and ordinary Bevy state, with no emulator data structures.

Verification passes the full workspace suite and all-targets/all-features
Clippy (with the existing wheel-rotation test warning), the 126-course importer
corpus check, all thirteen game GPU/MCP scenarios, the handmade mod's GPU drive
and the engine sandbox. Existing HUD/gauge/record pixel regressions pass; the
final HUD check covers 11,068 source-masked pixels within the established
nine-level channel tolerance. Normal export regenerates 181 files whose
manifest hashes were verified. The public-repository history audit passes.

## Remaining flow

The original changes driving mode after a result, presents its medal/failure
artwork, then applies awards/attempt updates and personal records through the
return flow. Those transitions, automatic braking/input ownership, retry/reset
and persistent progression remain unported. Collision faults have an input
boundary but require a real collision implementation. Other licence kinds and
the complete set of off-road event-zone exceptions still need their own
recordings; the current vehicle bridge covers the ordinary B-1 course.
