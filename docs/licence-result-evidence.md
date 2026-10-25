# Licence result timing

`race::licence_results` ports the licence branch of `8002A700` as an
independent rules plugin. A successful or failed attempt starts a result
state; a completed screen emits one `LicenceReplayRequested` event, retaining
the attempt. This event does not apply achievements or insert best records.

## Original execution

The installed `psx` build is `0.4.2+src-4c4c84345897a7b9-release`.
`tools/record_licence_results.py` records instruction-boundary observations of
`8002A700` and its return at `8002AAD4`, with stores to the signed result timer
`800AF226` and consumed input words `800A9590` (held) / `800A9594` (pressed).
Each private fixture retains the command, checkpoint digest and executing
build. Expected ages and return flags come from execution, not a reference
implementation of the new rules.

The retained corpus contains 72 initial recordings (407 result ticks) and two
later recordings (136 ticks). Controlled probes include inactive age -1,
start age zero, both wait boundaries, the prize boundary and confirmed final
ages, with Cross, Circle, Square, Start and no input. The later recordings use
different held/released schedules and were captured after implementation.

Checkpoints are local and deliberately absent from public Git:

- Success: `.psx/awards/gold-session/state.bin`, a natural B-1 stop at
  38.675 seconds, result age 33. From the current countdown checkpoint it uses
  2,500 fields, Cross through field 2,210 and then Square.
- Failure: `.psx/new-core/failure-checkpoint/state.bin`, a natural high-speed
  overshoot. It is a current-core checkpoint; old state format 9 is incompatible.

Reproduce the retained probes and the later holdout with:

```sh
python3 tools/record_licence_results.py
python3 tools/record_licence_results.py --fresh
cargo test -p gran_turismo_2 --all-features --test licence_results
```

An additional natural run in `.psx/results/natural-boundary` traces the stop
return and timer entry together. At absolute field 10952 / run field 2435,
`80015714` writes the successful outcome; `8002A6FC` initializes the timer to
zero; the stopping routine returns at `8003D7A0`; `8002A700` advances it to one.
The native result system therefore runs after stopping decisions in the same
fixed tick.

## Verified rules and native contract

Success advances once per 30 Hz tick, or three times while Cross or Circle is
held (`8002A790`, `8002A7F4`). At age 146 it waits for a **new press**, including
when accelerated updates reach that age midway through a tick. Confirm moves
to 147 and stops that tick's updates. The next tick requests replay; the
source clamps its completed age to 147 (`8002A814..824`).

Failure advances once regardless of held input. Age 29 waits for a new Cross
or Circle press; age 30 remains visible for one tick before replay is
requested (`8002A828..87C`). Square and Start do not confirm either screen.
Negative source ages are inactive. The native enum models that as `Driving`,
and preserves completed attempts in `ReplayRequested` instead of retaining an
inactive sentinel or a raw dispatch byte.

Timing is exported from the original instruction immediates into the layered
`race/licence-results.ron` asset. Mods may supply their own tuning and use the
public state/input API without a vehicle or renderer. The optional car bridge
reads one `LicenceParticipant` after `LicenceStopSystems::Decide`. Consumers
of the replay request are responsible for the next phase and resetting the
result to `Driving` when a new attempt begins.

## Native verification

The trajectory checks initialize native state once per recording and retain
it for every subsequent input, including the fresh held/released schedules.
They compare both the age and replay request against original execution;
after the handoff, the native state retains the attempt and emits no second
request when the original timer becomes inactive. An authored headless
integration check also verifies the Bevy event and that confirmation does
not commit an achievement.

The developer-MCP licence driving checks query `LicenceResult` alongside the
car's outcome immediately before and after the natural finish/failure and
at the final recorded tick. They check that the result starts in that same
tick and respects its distinct wait. Run them with:

```sh
cargo test -p gran_turismo_2 --all-features --test dev_mcp native_licence_stop -- --ignored --test-threads=1
```

The timing exporter reproduces the runtime asset and reference fixture byte
for byte from the original RAM checkpoint. Routine validation is
`cargo test --workspace --all-features`, `cargo fmt --all --check` and
`cargo clippy --workspace --all-targets --all-features`.

## Remaining scope

Finish/FAIL and prize drawing are now covered by
[result presentation](result-presentation-evidence.md). Result sounds and
replay playback remain unfinished. In the current binary, confirmation reaches the
explicit replay request and the car remains parked. Automatic replay,
replay exit, menu return, name entry, record insertion, retry and persistent
saves still need their own original-execution checks. Award commitment must
remain at menu return, after replay, as documented in
[licence award evidence](licence-award-evidence.md).
