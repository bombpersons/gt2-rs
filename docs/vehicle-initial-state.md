# Native vehicle initial state

`game::vehicle::initial::VehicleInitialState` is the public, authorable state
for the ordinary vehicle controller. Load it with `engine::prelude::load_ron`,
then call `state.spawn(world)` or `state.insert(&mut entity)`. The latter
attaches the controller to an existing entity. Both attach the same components
and feedback markers used by the continuous original-trajectory comparisons.
Neither loads assets, installs plugins nor requires a renderer.

The format uses the public components' documented units: Bevy world metres
and m/s, turns and turns/s, seconds, normalized pedal travel, and explicitly
documented internal force units. It contains no original addresses, pointers,
packed records or executable state. All groups and their omitted fields use
defaults. A minimal handmade spawn description is:

```ron
(
    pose: (position_m: (0.0, 0.5, 0.0), heading_turns: 0.0),
    drivetrain: (gear: 1, engagement: Coupled),
)
```

For fresh authoring, zero body/road bases and zero projected axes are filled
from the supplied orientation and heading. Zero suspension normal components
use those bases; a zero reciprocal duration uses the game's ordinary cadence.
These defaults are conveniences for new states, not a standing-start rule.
A moving checkpoint supplies its retained bases, reciprocal duration, wheel
history, suspension travel/rates, clutch history, engine/fuel-cut state and
transmission timers explicitly. Spawning preserves them. Transient force and
command outputs are rebuilt by the systems each tick.

The caller selects plugins and tuning. The full ordinary loop uses the vehicle
rules plus aerodynamics, engine torque, wheel slip, longitudinal slip, tyre
grip/forces, driver/brake assistance, braking, forward/body dynamics,
wheel rotation, drivetrain/transmission, road contact, suspension, vertical
body and contact-motion plugins, as listed in `GamePlugins`. Supply their
layered tuning files or insert their public tuning resources before adding
the plugins. Road contact uses `RaceCourse`; a mod can provide handmade
`CourseData` through that resource. Per-car setup components remain available.
Installing a subset of rules is supported, but only that subset will advance.

Spawning leaves logical input caller-owned through `DigitalPedalInput` and
`DigitalSteeringInput`. Add `PlayerPedals` to read the player's controller.
The spawn hook also attaches `DigitalGearInput`: ordinary driver systems
consume its reverse/shift controls, force reverse throttle when permitted,
and produce engagement requests from current pedals/RPM/reset inputs.
Removing that component leaves explicit transmission requests caller-owned.
The optional `chase_stabilization` group preserves the camera follower's
angles, rates and previous yaw acceleration; when omitted it starts at the
supplied heading/pitch.
Presentation is separate: this hook does not attach `VehicleBody`, a camera
subject, a `Transform` or wheel geometry. The
[live presentation plugin](vehicle-presentation-evidence.md) now consumes
this state; the [handmade driving mod](../mods/example/README.md) supplies
its own initial state and tuning through these same public pieces.

## Checkpoint export and verification

`tools/export_assets.py` writes `vehicle/initial.ron` alongside the current
B-1 tuning, from the same local checkpoint. It supports the established
ordinary non-induction engine boundary and rejects unknown engagement/shift
modes. The generated state remains ignored in `game/assets/`, like all
disc-derived outputs. The game never loads an emulator snapshot.

`game/tests/vehicle_loop.rs` now restores every scenario through the public
state format, including an RON serialize/deserialize round trip. All four
retained 30-tick trajectories still compare at zero tolerance, including
the previously held-out steering/mixed-pedal recording. An additional check
loads the checkpoint export and reproduces all four original trajectories.
The exporter and fixture initializer independently convert the original
fields. A handmade unit test also checks partial RON defaults, derived axes,
retained histories and caller-owned input without private assets.

This verifies an initialization boundary for the moving B-1 sequences in
`vehicle-loop-evidence.md`. Standing-start behaviour, collision, licence flow,
and broader content parity remain unfinished.
Idle starts are now verified separately in
[driver-request evidence](driver-request-evidence.md), including real-binary
replay from an offline-converted idle initial asset.

## Validation

`cargo test --workspace --all-features` passed with the private fixtures
present. The regenerated checkpoint asset passed all four 30-tick sequences
at zero tolerance. Workspace Clippy completed with the existing
`wheel_rotation` test's `collapsible_if` suggestion. Game formatting,
`git diff --check`, Python compilation and the public-history audit passed.
No rendering behaviour changed in this step.
