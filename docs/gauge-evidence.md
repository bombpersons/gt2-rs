# Driving gauges

The B-1 HUD now renders speed, selected gear, units, tachometer dial, redline
band and shaded needle from the native vehicle state. These use ordinary
Bevy sprites and `Mesh2d` polygons. Mirror, boost gauges, sounds,
licence result/award presentation and retry flow remain unfinished.
Personal records and medal targets are covered by [records evidence](records-evidence.md).

## Disc conversion

USA Simulation Mode Rev 2's `/arcade/game_status_files.gz` decompresses into
an eleven-entry TIM pack. Entry zero supplies common gauge artwork and its
palettes; ten variants supply the tachometer dial. `8002E390..4E8` loads the
common image and selects the per-car dial using `8002BD84..BE78` and the
threshold table at `8002F868`. The Vitz's maximum of 6,800 RPM rounds up to
a 7,000 RPM scale and selects pack entry two. Its redline is 6,300 RPM.

The exporter parses the pack's offsets and TIM blocks, applies the selected
80×80 dial, and resolves the original sprite rectangles/palettes offline.
It writes an RGBA PNG atlas and `race/gauges.ron`: normal pixel rectangles,
layout offsets, colours, needle shapes and RPM/radius settings. Transparent
colour zero and STP flags are preserved. The native assets contain no texture
pages, CLUT addresses, source pointers or GPU packets. The exporter records
the decompressed pack's SHA-256 in its manifest.

Every recorded frame independently verifies that these source TIM images
reproduce the entire uploaded 64×234-halfword gauge rectangle byte for byte.
The runtime PNG is converted from the disc resource, never from VRAM or a
screen capture. All converted content and reference captures remain ignored.

## Rules and rendering

| Behaviour | Original evidence |
|---|---|
| Scale, redline divisions and dial variant | `8002BD84..BE78`; maximum RPM rounded up to thousands, four sections per thousand; redline truncated to sections |
| Arc geometry | `8002BE7C..BED4`; two radii sampled at each division |
| Screen-space rotation | `8002BCF0..BD80`; separate product shifts and asymmetric pixel biases |
| RPM angle and two shaded needle strips | `8002C1FC..274`, templates at `8002F880/898` |
| Gear, engagement tint and subtractive background | `8002C278..3AC`; gear map at `8002F874` |
| Units and speed digits | `8002C3B0..550`; readout divided by 100, three fixed cells, leading zero suppression |
| Dial and arc drawing | `8002C00C..19C`; actual captured packets establish consumed blend state and draw order |

Rotation reuses the vehicle's already verified 4,096-sample steering circle.
Each rotated product is shifted separately before conversion from sixteenth
pixels. Centering also preserves the source's one-pixel y carry for points
left of centre, caused by adding packed signed coordinates. The runtime
expresses that explicitly as a screen-coordinate correction.

Actual GPU draws establish half blending for the arc, additive drawing for
the dial, and subtractive drawing for the gear background. The band draws
in reverse section order, followed by dial, speed digits, units, gear shade,
gear and the two opaque Gouraud needle strips. Source dithering is disabled
for these polygons. The renderer retains those layers and uses the palette
STP flags for dial texels.

`GaugeRulesPlugin` reads `EngineTorqueInput.integer_rpm`,
`ContactMotion.speed_readout` and `DrivetrainInput` after vehicle/session
updates. A public `GaugeSubject` marker selects the vehicle independently
from its input controller or camera. `GaugeDisplay` is reflected for the
developer MCP. It first becomes visible after a simulation tick, matching
the opening upload-only field.

`HudPlugin` includes `GaugePlugin` and supplies a 2D camera even when the
scene has no driving-session clock. Gauges anchor to the lower right.
`GaugePlugin` can also be used with a caller's own `Camera2d`/`PsxView`,
authored settings and atlas; `GaugeRulesPlugin` requires no renderer. The
handmade mod continues to disable the game's HUD and needs no original data.
The current export selects one Vitz/USA mph style. Other vehicle styles,
unit-selection menus and boost gauges remain separate work.

## Original comparisons

`tools/record_gauges.py` records consumed source loads and actual GPU
primitives. It extracts expected screen rectangles, polygon vertices,
vertex colours, blend modes and order directly from complete captures.
No native layout function produces expected geometry.

The initial set has eight unmodified idle/acceleration frames and nine
controlled probes. The probes replace only HUD load instructions with
explicit values; original drawing code and assets remain responsible for
every expected primitive. They cover reverse and six forward gears, both
engagement tints, signed RPM, scale/redline/overscale angles, zero and leading
digits, and speed boundaries through the unsigned 16-bit maximum.
Seven fresh unmodified driving frames were recorded after the initial layout
comparison passed. All **24** frames match exactly, including draw order.

The real-binary check replays continuous idle and acceleration sequences,
queries the resulting `GaugeDisplay`, and compares source-masked opaque
sprite pixels to the original. It delivers CROSS after the first native tick:
the original pad transport delivers it to physics tick two, as independently
observed in the countdown-gas vehicle recording. It does not reseed vehicle
state between observations. Arc, dial blending and needle rendering are also
visually inspected; complete pixel parity over different scene backgrounds
is not claimed.

```sh
python3 tools/record_gauges.py --pack .psx/asset-export/game_status_files
python3 tools/record_gauges.py --pack .psx/asset-export/game_status_files --fresh
cargo test -p gran_turismo_2 --all-features --test gauges
cargo test -p gran_turismo_2 --all-features --test dev_mcp native_gauges \
  -- --ignored --nocapture
```

The final native check covers all fifteen unmodified initial/fresh frames:
every queried gauge state matches, and **9,801** source-masked sprite pixels
match with maximum encoded-channel error **9** (one 5-bit colour step).
Idle/accelerating images at `/tmp/gt2-gauges-404/idle-50.png`, `gas-260.png`
and `gas-446.png` were inspected. The free-driving chase image at
`/tmp/psx-template-headless-404/driving.png` confirms gauges also render without
a session resource. All seven game MCP checks pass, including the unchanged
11,068-pixel clock/announcement comparison and exact launch/reverse replays.
The handmade mod's GPU test and rebuilt engine sandbox movement/ramp/texture
check pass. The export manifest verifies all 177 generated files.
Workspace tests and doctests, formatting, Python syntax and the public
repository history audit pass. Workspace Clippy passes with the existing
wheel-rotation test's `collapsible_if` warning. No template files, generated
assets, original fixtures or runtime screenshots are included in the commit.
