# Example course mod

Run `cargo run -p example-mod` from the workspace root. F1 opens controls;
F2 opens display settings. The mod is a separate executable depending on
`game/` and using its public `GamePlugins` group, course renderer and driving
camera controller. R1 cycles views; L1 looks back.

The hand-authored 320 m circuit in `assets/gt2/course/TC_lisence.ron`
replaces the game's course at that path. Bevy and synchronous RON loads
search this mod's `assets/` first, then `../../game/assets`. Its course has
four nodes, four surface quads and four level-1 draw spans; all geometry is
written by hand and no extracted content is required. Run the binary outside
Cargo with `BEVY_ASSET_ROOT` set to this crate's absolute directory.

Its handmade `assets/race/camera.ron` replaces the camera's lens, offsets and
starting view. The public `CameraRulesPlugin` follows an ordinary subject
Transform; no imported vehicle/body or physics data is needed. The MCP check
cycles to the mod's 12 m far view, moves its subject and verifies the camera
follows it. Race rules run at 30 Hz, with 60 Hz presentation.

Its `assets/gt2/vehicle/t2vzn.ron` replaces the game's body with two handmade
boxes: a coral deck and a blue cabin. It contains no extracted vertices or
textures. The public `vehicle::presentation::VehiclePresentationPlugin`
dresses subjects marked `VehicleBody` with engine-native geometry under their
ordinary Transform (metres, X right, Y up, -Z forward). The asset override
exercises body replacement through the same asset layers as the course.
Alternatively, insert `VehiclePresentation { geometry: Some(path), wheels: None }`
before adding the feature to select another body without inherited wheels,
set both fields to `None` to disable dressing, or disable the presentation
plugin through `GamePlugins`.
No wheels, reflections or physics are implied by this authored body.

The separate `assets/gt2/vehicle/t2vzn-wheels.ron` layout supplies four
handmade charcoal cuboid wheels using `wheel.ron`. Their dimensions and
poses are authored for this box car. The base port's independently decoded
procedural tire profiles and wheel image are not used by the mod. Replace
or disable the optional wheel layout separately from the body, and keep its
`body_asset` path equal to the selected body to avoid applying unrelated
attachments. These static wheels do not add steering, suspension or rolling
physics.

The port still lacks the vehicle force/body simulation and a playable driving
session. This example proves course and body replacement, camera tuning and controller
reuse. As driving
rules land, add their trajectory tests and a new vehicle using the real
controller's documented asset contract. It does not yet demonstrate driving parity.

A code mod can select features with
`game::GamePlugins.build().disable::<game::race::RacePlugin>()` and add its
own plugins. Object and action overrides use `replace_object_type` and
`replace_action` after the original registration; accidental duplicate
registrations still panic.

Checks:

```sh
cargo test -p example-mod --all-features
cargo test -p example-mod --all-features --test dev_mcp -- --ignored --nocapture
```

The first checks the actual game loader with the handmade course, asset
priority, and disabling the scene without requiring rendering resources.
The second renders the separate binary offscreen and checks its camera is
on the mod's circuit, then exercises its camera controls and subject following.
It needs a software GPU adapter.
