# Automatic braking after a licence result

B-1 now transfers control from the player to a shared braking controller when
the licence result latches. It centers the steering, applies full service
brakes, releases throttle/handbrake, and parks only after both linear and
angular motion settle. Parking releases the brakes and uses the original
motion-reset and suspension gates. Results, awards, retry and saves remain
separate unfinished features.

## Original rules and boundaries

The single-car control dispatcher at `8003E2A0..310` chooses player controls
or the automatic driver `80038540`. The latter requests engine engagement
before updating steering/pedals. For the ordinary multi-ratio car, a
disconnected engine requests engagement strictly above three quarters of
car `+0x396` RPM; an engaged engine keeps requesting it at or above the idle
RPM at `+0x10A` (`80038630..390`). Parking clears the request. Automatic gear
selection remains the existing transmission solver.

`80037664` slews steering towards its target using car `+0x62` times the
car's physics duration, flooring the positive step on the /4096-turn grid.
It leaves the player's steering spring history untouched. Failed B-1 runs
select target zero at `80037878`. Successful B-1 stops enter mode 5; their
speed is inside the route driver's neutral band (`80037AB4..7B94`), which
also selects zero before the shared stopping routine. General route-following
AI and high-speed route steering are not implemented by this slice.

`800376D8..7E8` releases throttle and tests two approximate three-axis
magnitudes. `8003C398` uses the largest absolute axis plus a floored quarter
of the sum of the other two. Linear axes use /4096 metres/second. Yaw
(/524288 turns/second) and pitch/roll (/65536) are first narrowed with signed
truncation to /4096 turns/second. Both comparisons are strict: linear below
the immediate at `8003775C`, angular below the immediate at `80037768`.
Otherwise the pedal and all four wheel brake channels are full travel.
Below both thresholds the routine clears those channels and selects parked
mode 7; `800377E8..834` continues centering with released pedals.

The exported `vehicle/automatic_stop.ron` contains the selected car's slew
rate, settling thresholds and engagement bands. No original parameter table
or emulator state is compiled into the native game.

## Native integration and extension

`AutomaticStopRulesPlugin` runs without a renderer or licence session. Attach
`AutomaticStop` to an existing car to acquire `ExternalDriver` ownership;
player pedal, steering and engagement producers then leave its controls
alone. Existing force, transmission, wheel, road and body rules continue.
`AutomaticStopSetup` overrides tuning per car. Another driver can supply a
steering target while braking. Removing the component releases ownership and
the stop-specific body, suspension and transmission gates.

`LicenceStopPlugin` attaches the controller after its post-physics result
decision, so automatic control first runs on the next physics tick. It
supports either shared or per-car stop tuning. The existing independent
licence-only tests still work without a vehicle controller. `GamePlugins`
lists the feature separately so mods can replace or disable it.

## Verification

The complete-loop tests initialize once, then compare four physics boundaries
with zero tolerance. Later snapshots are assertions, never state injections.

| Recording | Complete native/original ticks | Coverage |
|---|---:|---|
| `post-gold-gas` | 80 | Successful stop, centering, immediate settling and parked feedback |
| `post-overshoot-gas` | 250 | High-speed failure, retained full braking, engine/gear transitions, parking |
| `post-fresh-stop-holdout` | 180 | Independent 0.4.2 core run with conflicting steering, pedals, reverse and shifts |

The fresh holdout was captured after implementation from a new power-on
route, not a converted old save state. It reaches parked state at zero-based tick 118.
Cross remains continuously held so no new confirm edge selects Retry in the
original result menu. Its other player requests change at fields 40, 100 and
160; native tests supply those requests even though the automatic driver
ignores them. All 510 complete ticks pass, bringing the retained vehicle
corpus to 1,330 original ticks. Physics duration is also checked at every
entry boundary.

Twenty-six original executed probes check both signs of the strict linear
and angular boundaries, fractional negative angular narrowing, the combined
axis estimate, steering slew/clamp edges, already parked control, and both
engagement RPM boundaries. These were also regenerated on the new core.
An authored rules-only test checks per-car override, parking, wheel-brake
copying and control release without a session or renderer.

The real native binary additionally replays gold, early, overshoot, off-road
and held-out licence attempts through the existing developer MCP checks.
Gold/early/overshoot/holdout checks now compare world position before and
after the result and at the end, alongside the latched result/clock. Off-road
position remains checked only before its existing unported collision boundary.
No result banner, award animation or retry parity is claimed.

Workspace tests and all five real-binary licence checks pass. Gold and
overshoot screenshots were inspected. The full importer/exporter was also
run into an isolated ignored asset folder with the regenerated countdown;
the current core's power-on capture produced an initialized 120-field
countdown car identical to the earlier reference. The source and history
audits pass. No disc-derived output is included in the public changes.

```sh
python3 tools/record_automatic_stop.py --psx /path/to/updated/psx \
  --state .psx/new-core/failure-checkpoint/state.bin
python3 tools/record_vehicle_loop.py --psx /path/to/updated/psx \
  --state .psx/new-core/failure-checkpoint/state.bin \
  --scenario stop-holdout --frames 360 --prefix post-fresh-
cargo test -p gran_turismo_2 --all-features --test automatic_stop --test vehicle_loop
cargo test -p gran_turismo_2 --all-features --test dev_mcp native_licence_stop \
  -- --ignored --nocapture
```

To rebuild the fresh checkpoint: use `poweron-to-b1-list-current.json` for
4,800 fields; confirm B-1 for 1,800 fields, Start for 1,500, then AT for 400.
Each confirmation holds Cross at fields 10..22 of its hop. Continue idle for
600 fields, then hold Cross for 2,312 fields and save. The capture directories,
commands, build identity and state/RAM hashes stay in ignored `.psx/` and
`game/tests/data/original/`.
