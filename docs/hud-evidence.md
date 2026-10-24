# Countdown, start announcement and lap clock

The native B-1 scene now draws the original countdown digits, `START` banner,
lap-clock label and running clock through ordinary Bevy sprites. This is a
partial HUD: tachometer, speed/gear, records, mirror, sounds, intro cameras,
licence outcomes and retry flow remain unfinished.

## Sources and decoding

USA Simulation Mode Rev 2's `/font/racefont.dat` is a 65536-byte, 512×256
4-bit bitmap, including its palettes. Every byte matches the rectangle
uploaded at VRAM halfword x=384, y=0, width=128, height=256 in all recorded
HUD frames. The converter reads the disc resource, not a captured screen.

`8007DD3C` reads eight-byte glyph metrics, texture rectangles and bearings;
`8007DC78` reads packed pair-spacing tables. The small and large font
descriptors at `80093124` and `8009313C` point to the tables in the original
program. The exporter decodes printable ASCII, packs glyph rectangles into
native PNG atlases, and writes normal pixel bearings, advances, cell insets
and pair adjustments to the engine's `BitmapFont` RON format. It preserves
transparent pixels and the palette STP flag. Unsupported decorated glyphs
are rejected; extended character sets are separate work.

The engine format contains no original pointers, table bitfields, texture
pages, palettes or GPU packets. Games render it with ordinary sprites; the
game HUD uses `PsxSprite` for gamma-space tint and the original blend modes.
Font bitmap and metric data stay in ignored runtime assets. Design values
and labels are read from the original code/data into `race/hud.ron`; the
manifest records the font resource's hash as well as its checkpoint hashes.

| Behaviour | Original evidence |
|---|---|
| Independent announcement timer decreases by the actual field step | `8002A0D4..120`; B-1's timer at `800AF224` |
| Countdown digit and centered baseline | `8002A19C..288`; digit at x=160, baseline y=110 |
| `START` hold, brighten and darken phases | `8002A28C..378`; RGB changes and additive mode are actual GPU draws |
| Small-font label and clock placement | `8002CB10..7C`, `8002CD6C..98` |
| Right-aligned digits, punctuation widths and cell insets | `8006B218`, `8006B360`, `8006B3F4`, glyph flag at `8007DD74` |
| Integer milliseconds plus the periodic display-clock phase | `8002CCF8..CD68`; `80046F64 / 3` plus `floor((8002F864 & 15) * 1000 / 900)` |
| Display-clock phase increments after each active clock tick | `8002E5F0..60C` |

The native announcement derives the same timer from the session: the release
tick has already incremented the clock when the timer reaches its post-release
duration. The banner later changes to additive drawing; this is not a fade
implemented by reducing ordinary alpha. Clock formatting keeps integer
milliseconds and the observed periodic phase. Other lap/result clocks and
HUD configurations remain outside this comparison.

## Original draw comparisons

The initial set contains 11 GPU frames spanning both digits, release, running
clock, both fade halves and disappearance. A fresh set adds eight frames,
including longer idle-clock checks through source field 1346. It was recorded
after the initial draw comparison passed. All 19 frames match exactly in
character, rectangle, colour and additive/opaque mode.

`tools/record_hud.py` derives expected glyphs only from actual textured GPU
primitives, identifying their source rectangles through the original font
metadata. It checks complete captures and byte equality with the font upload.
It normalizes the double-buffer draw offset to screen pixels. No native
layout function or reference implementation generates the expected draws.

The GPU consumes a previously buffered HUD packet. At source field 158, the
CPU trace builds clock value 705 ms while that field's GPU capture draws
`0:00.670`, built by the preceding physics tick. The comparisons therefore
step the native session `source_field / 2` ticks. This is a measured packet
boundary; it does not alter the vehicle tick comparisons or pad transport.
The opening field contains texture uploads and no HUD draw, and is excluded.

Reproduce with the start checkpoint and font produced by asset export:

```sh
python3 tools/record_hud.py --state .psx/asset-export/start/state.bin \
  --font .psx/asset-export/racefont.dat
python3 tools/record_hud.py --state .psx/asset-export/start/state.bin \
  --font .psx/asset-export/racefont.dat --fresh
cargo test -p gran_turismo_2 --all-features --test hud
cargo test -p gran_turismo_2 --all-features --test dev_mcp \
  native_hud_opaque_glyph_pixels_match_the_original -- --ignored --nocapture
```

Recordings, source commands, state/font hashes, capture reports and images
stay in ignored local folders. The measured clock-phase trace is retained at
`.psx/runs/hud-clock-active/`; original glyph-writer trace is at
`.psx/runs/hud-glyph-fields/`.

## Rendering and extension

The real-binary pixel check replays the same HUD boundaries with normal
scene assets. It compares every nontransparent texel of the original opaque
glyphs, allowing at most one 5-bit colour step (9 encoded byte levels) for the
screen pass's quantization/dither. Scene backgrounds are different, so
transparent texels are excluded. Additive placement/colour/mode is checked
exactly in the draw comparison and its rendered fade is visually inspected;
additive pixels over different backgrounds are not claimed equal.

`HudRulesPlugin` runs headless with public `HudSettings` and `DrivingSession`
resources. `HudPlugin` adds font loading, sprites and a separate 2D `PsxView`
camera over the 3D scene. It is independently selectable in `GamePlugins`.
The session/settings must exist before startup for this presentation to
spawn. Existing content without `race/hud.ron` keeps its previous presentation.
The handmade mod disables this game's HUD plugin while retaining the same
session and vehicle rules, requiring no original font data.

Label/clock sprites anchor to the upper left; the announcement anchors to
the horizontal centre. Font metrics and text layout are reusable engine
data, independently tested with an authored RON font and pair spacing. Font
and HUD editing tools remain part of the broader editor work.

Presentation waits for the first fixed update, as the original opening field
has uploads and no HUD draw. This avoids briefly previewing a boundary digit
before the countdown has ever been decremented.

## Validation

All 19 original-frame draw comparisons passed, including the eight fresh
frames through about twenty seconds of running clock. The real-binary check
compared **11,068** pixels using masks derived from the source GPU textures
and CLUT; maximum encoded-channel error was **9**. Images at
`/tmp/gt2-hud-pixels-84/field-50.png` and `field-268.png` were inspected for the
digit and additive fade. A fresh exporter-only asset folder also rendered the
HUD correctly: 962 source-masked pixels matched within the same bound, with
its inspected image at `.psx/hud-clean-runtime/countdown.png`.

Workspace tests, all six game MCP checks, the handmade mod's GPU check, and
the rebuilt engine sandbox's movement/ramp/texture check passed. Clippy passed
with the existing wheel-rotation test warning. Formatting, Python syntax,
all 175 development-export manifest hashes, font-source identity and the
public history audit passed. New fonts, screenshots and reference fixtures
remain ignored; no runtime game data is published.
