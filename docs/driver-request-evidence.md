# Ordinary driver requests and idle launches

The ordinary digital controller now produces gearbox-engagement requests,
reverse and manual-shift requests, reverse throttle, current accelerator
travel and current brake travel for the existing transmission/drivetrain.
The complete vehicle loop matches **440 original physics ticks**: the earlier
120 moving ticks plus 320 new idle-start ticks. All sampled quantities still
compare at four stage boundaries with zero tolerance and one initialization
per recording. No later engine, wheel, pose or contact state is injected.

This verifies ready-to-drive idle launches, brake-release launches,
reverse/forward transitions and manual shifting. It does not complete B-1's
countdown, licence clock, success/failure, retry/reset selection, collision,
HUD or audio. The normal scene still selects its moving checkpoint asset;
the real-binary idle test supplies a separately converted native initial state.

## Source rules

USA Simulation Mode Rev 2, Vitz F '99, physics base `800A96B4`.

| Rule | Original code and state |
|---|---|
| Triangle requests reverse and raises digital throttle, unless the first-gear countdown is active | `8002FFC8..80030004`; logical input+9, physics+78E and scratch+364 |
| L2/R2 supply signed down/up requests; simultaneous holds cancel | Observed logical input+8 from real pad replays, read at `80030168` |
| Suppress manual-shift requests during the first-gear countdown | `80030158..174`; scratch+365 |
| Reset, first-gear override, or full handbrake on the rear reference axle clears engagement request | `8002F92C..978`; physics+718/+78E/+786, +373, +614 |
| A disconnected ordinary engine requests coupling unless the service brake is fully down | `8002F97C..9A0`; +619, +612, +60A |
| An already engaged engine requests coupling while last refreshed RPM is at least idle | `8002F9A4..9C0`; +6AC compared with unsigned idle parameter +10A |
| Prepare curve torque/drag even while disconnected; defer combustion/fuel-cut evaluation to free-engine feedback | `8003E1FC..21C`, then `8003AC60` |

Pedal ramping precedes the engagement producer. The first-gear countdown is
already decremented when reverse/shift gates read it. The producer runs
before transmission preparation, which may clear an engagement request
again while waiting for sufficient launch RPM. Native systems preserve those
boundaries; a launch is not implemented as an immediate velocity assignment.

The new `DigitalGearInput` opts an entity into ordinary driver-request
production. `PlayerPedals` reads the logical pad into it alongside pedals and
steering. Without `DigitalGearInput`, replay/AI/subsystem callers retain
ownership of their explicit `TransmissionInput` and engagement request.
Raw throttle input remains caller-owned: reverse is combined with a copy
while advancing pedals, rather than latching a synthetic press into input.

The engagement producer consumes `BodyDynamicsInput::reset_motion` as an
explicit parent/session boundary, and the per-car or default drivetrain's
reference axle. The parent supplies reset/controller-mode gates; this feature
does not invent a licence/reset state machine. The special one-/two-ratio
control producers and analog controls remain separate work.

## Reproduction

The exporter's saved transmission-dialog checkpoint can be reused to make
stationary original references. These are local emulator outputs, not assets
distributed by the repository:

```sh
psx run --from-state .psx/asset-export-full/transmission/state.bin --frames 750 \
  --input '[{"frame":10,"buttons":["CROSS"],"until":16}]' \
  --save-state --out .psx/runs/b1-stationary-checkpoint --summary
psx run --from-state .psx/asset-export-full/transmission/state.bin --frames 750 \
  --input '[{"frame":4,"buttons":["RIGHT"],"until":12},{"frame":22,"buttons":["CROSS"],"until":28}]' \
  --save-state --out .psx/runs/b1-manual-stationary-checkpoint --summary

python3 tools/record_vehicle_loop.py --state .psx/runs/b1-stationary-checkpoint/state.bin \
  --frames 120 --prefix standing- --scenario gas --scenario brake-launch --scenario reverse-forward
python3 tools/record_vehicle_loop.py --state .psx/runs/b1-manual-stationary-checkpoint/state.bin \
  --frames 120 --prefix manual- --scenario shifts
# Recorded after those complete native trajectories passed:
python3 tools/record_vehicle_loop.py --state .psx/runs/b1-stationary-checkpoint/state.bin \
  --frames 160 --prefix standing- --scenario launch-holdout
python3 tools/record_driver_requests.py
cargo test -p gran_turismo_2 --all-features --test vehicle_loop --test driver_requests
```

The reference states reach vblank 7050, after the licence clock has started.
The car is idling near the line with only its observed sub-speedometer motion;
this is not the earlier race-loading/countdown phase. Commands, state/RAM
hashes and toolkit version accompany the ignored fixtures. Both states select
the same Vitz; the second uses the original manual-transmission menu choice.

The whole-loop recorder now observes all twelve effective input bytes after
the original pad producer and before `8002FB18`. It requires every byte to
have been written, every phase every two fields, complete watches and no
dropped trace lines. This covers reverse's throttle branch even though that
branch skips the ordinary throttle load. Native tests replay consumed logical
requests; physical serial-pad latency is outside that boundary.

| Recording | Original ticks | Coverage |
|---|---:|---|
| Idle throttle | 60 | Free engine, repeated coupling requests, synchronization and motion |
| Brake hold then launch | 60 | Full-brake suppression, release, engine run-up and coupling |
| Reverse then forward | 60 | Reverse throttle/selection and forward return |
| Manual requests | 60 | Up/down holds, release and simultaneous cancellation |
| Fresh launch holdout | 80 | Brake, throttle against brake, release, handbrake, reverse and both steering directions |

The fresh holdout passed without another rule change. It is additional
original execution, not a sequence generated by a reference model.

## Focused gates

Fourteen controlled original executions observe the engagement producer before
transmission preparation. They cover service brake at full/almost full,
engaged RPM below/at idle, all ordinary engagement modes, rear-reference
handbrake at full/almost full, front-reference handbrake, countdown active/
expiring and reset. The expected request is the last actual CPU store by
`8002F92C`; the disconnected-release path briefly clears it before setting it.
Native tests observe the equivalent intermediate boundary, not a later result
that preparation could have changed.

The probe zeros pedal ramp rates so chosen fractions reach the producer
unchanged, and verifies intended values and reads inside the unmodified
producer. For reset, the licence loop overwrites the global input gate, so
that one probe overrides the parent's reset assignment with an authored
`li s3,1`. It leaves the tested request routine unchanged and requires its
actual reset read to be one. These probes are controlled consumer evidence,
not claims that the original licence naturally entered every configuration.
Reverse/shift suppression during a countdown is established by the binary;
the natural launch recordings have no active first-gear override.

## Runtime and extension checks

The real-binary MCP idle test copies the current native assets into a temporary
folder, replaces only `vehicle/initial.ron` through the offline converter,
and replays the original reverse/forward logical requests through normal
buttons. All three final world-position axes match exactly. The native
executable reads ordinary RON/PNG assets; it never loads an emulator state.
`/tmp/gt2-idle-driving-289/after-launch.png` was inspected. Its near-line chase
view shows the body and course; licence HUD, mirror and full presentation
parity remain unfinished.

The handmade mod additionally stops, re-engages and reverses through the
same public controls. Its independently authored free-engine response and
first/top shift thresholds now suit idle re-engagement and its finite ratio
list. No original tuning was copied into those assets.

Validation passed: workspace tests with all features, the focused original
comparisons, the native idle MCP test, the handmade mod's rules/GPU checks,
and the engine sandbox checks. Workspace Clippy passed with the existing
`wheel_rotation` test's `collapsible_if` warning. Generated original fixtures
and screenshots remain ignored and outside the public repository.
