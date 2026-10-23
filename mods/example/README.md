# Example driving mod

Run `cargo run -p example-mod` from the workspace root. Cross accelerates,
Square brakes, Circle applies the handbrake, and left/right steer. R1 cycles
views; L1 looks back. F1 opens control bindings; F2 opens display settings.
Triangle requests reverse; R2/L2 shift up/down when initial state selects MT.
The box car starts moving at 7 m/s on a handmade 320 m square circuit.

The mod depends on `game/` and uses its unchanged `GamePlugins` group: the
same force, engine, drivetrain, tyre, contact, suspension, body, wheel and
camera features as the Vitz. It supplies its own data through the ordinary
asset layer. No edit to `game/` or `engine/` and no disc, BIOS, exported
parameters or reference fixtures are required.

The values are independently authored for this example, rather than a copy
of the original car's setup. They demonstrate controller reuse and authoring;
they are not a claim about an original GT2 car's handling. Race physics runs
at 30 Hz, with 60 Hz presentation.

## What to change

| Asset | Purpose |
|---|---|
| `assets/gt2/course/TC_lisence.ron` | Four road nodes, surface quads, draw spans and flat collision grids |
| `assets/gt2/vehicle/t2vzn.ron` | Handmade coral deck and blue cabin, in engine `GeometryData` |
| `assets/gt2/vehicle/wheel.ron` | Handmade octagonal tyre and two hubs, with a bright sector to show spin |
| `assets/gt2/vehicle/t2vzn-wheels.ron` | Four metre attachments and live simulation bindings |
| `assets/vehicle/initial.ron` | Native moving initial state: position, velocity and retained controller state |
| Other `assets/vehicle/*.ron` | Pedals, steering, forces, engine/gears, assistance, contact, suspension and visual tuning |
| `assets/race/camera.ron` | Lens, bumper/chase offsets and initial view |

RON and Bevy asset loading search this crate's `assets/` first, then the
base game's `../../game/assets`. Every file used by this example is present
in its own layer. Outside Cargo, set `BEVY_ASSET_ROOT` to this crate's absolute
directory. The executable selects `RacePresentation { geometry: None }` to
render the authored road ribbon; physics reads the collision polygons, not
that visual mesh.

## Controller and model contract

The [public initial-state format](../../docs/vehicle-initial-state.md) and
[live presentation contract](../../docs/vehicle-presentation-evidence.md)
describe the shared pieces. `RaceVehicle` selects `vehicle/initial.ron` by
default. Numerical rules do not require a model, joints or animation clips.

The handmade body uses local X right, Y up, -Z forward and metre vertices.
`VehicleBody` attaches ordinary engine geometry beneath its driven Transform.
The optional wheel layout names the matching body path. Its bindings select
simulation wheels 0..3, suspension-origin height, axle camber and an extra
mesh-local unit rotation. Canonical wheel geometry has Z as axle and XY as
rolling plane. This example's wheels follow steering, suspension and spin
through that contract; the tyre/hub geometry is constructed independently
from an octagonal prism and discs. No original wheel profiles or textures
are used.

Replace the body/layout through assets, or insert `VehiclePresentation` to
select other paths or disable them. A code mod can disable a feature with
`GamePlugins.build().disable::<FeaturePlugin>()`, add its own plugins or
supply public tuning resources/per-car setup components. This example needs
no custom movement or force systems.

The numerical tuning retains the controller's documented units, including
normalized internal mass/force units rather than an asserted SI calibration.
The initial ride state balances four equal spring loads against authored
gravity; rendering puts the quarter-metre tyres just above the floor.
Several authoring details matter:

- Tyre slip curves cover **negative and positive** coordinates. Force direction
  is a separate solver output, so both sides of the curve have positive grip.
- This rear-drive car uses independently rotating front wheels and an open
  rear differential. The initial `independent_wheels`/`independent_axles`
  flags agree with that choice.
- The four direction curves are independent mathematical samples of
  `tan((i - 0.5) * tau / 4096)` for `i = 1..512`, rounded to `f32`. Their
  provenance is written in each file; they are not the original lookup table.
- Collision has local metre polygons and a 1 cm horizontal sampling grid.
  Coordinates and areas fit the sampler's signed query/edge arithmetic.
  The flat floor covers the circuit and its runoff, from -40 to 120 m on
  each horizontal axis. There are no barriers, lap rules or recovery outside
  that floor yet.

## Checks

```sh
cargo test -p example-mod --all-features
cargo test -p example-mod --all-features --test dev_mcp -- --ignored --nocapture
```

Headless checks drive identical initial states with throttle, coast, service
brake and handbrake, verify acceleration/deceleration and retained floor
contact/load, then turn both directions through the public controller. They
also check asset priority, that every numerical asset resolves to this mod,
and that callers can disable the scene.
An additional check stops the car, releases the brake, launches again and
then selects reverse through the new ordinary driver-request producer. The
authored free-engine response and first/top shift thresholds support those
idle transitions and the three available forward ratios.

A timed-start check inserts `DrivingSession` and marks the same handmade car
with `SessionParticipant`: it holds despite throttle, releases on the countdown
boundary, advances the clock and drives away. The controller needs no edits.
See [the session API](../../docs/session-start-evidence.md) for optional content
setup through `race/vehicle.ron`.

The opt-in GPU/MCP check copies only the handmade assets into a fresh tree
with **no base-game assets folder**, then runs the real executable there.
It verifies acceleration, body/camera following, wheel rolling, view cycling,
steering and braking, and captures normal and close oblique views for visual
inspection. It requires an offscreen software GPU adapter.

The base port still needs default start selection, collision, licence conditions,
HUD, sound and complete pass/fail/retry flow. This authored example proves
reuse of the current controller; it does not finish those original-game tasks.
