# Licence result presentation

`race::result_presentation` draws the licence result with ordinary bitmap-font
sprites and a textured 2D logo mesh. `HudPlugin` adds the optional presentation;
`LicenceResultRulesPlugin` remains independent. No saved GPU packets, VRAM,
emulator or original-format assets are used by the running game.

## Evidence and reproduction

The executing original is USA Simulation Mode Rev 2 under
`psx 0.4.2+src-4c4c84345897a7b9-release`. `8002AB60` draws Finish;
`8002B170` selects the licence result, prize and completed-time display.
`8002B600..638` darkens the driving view; `8002B488..5FC` constructs the
animated prize-logo quad. Shared text routines are `8006ADB4` (centered text)
and `8006B49C` (centered numeric cells).

`tools/record_result_presentation.py` retains original GPU primitives in the
ignored `game/tests/data/original/result_presentation/` directory. The initial
25 cases contain 602 glyph draws. Twelve later cases add 344 draws, including
unused animation ages, different medal times, the qualifying assisted attempt,
an already-passed test, failure confirmation age, a native-run comparison time
and an unmodified original run to the confirmation wait.

Controlled probes replace only the result-timer update with a successful
return and set its age, completed time and saved qualification fields. Original
drawing, font layout, medal comparisons, texture sampling and GPU execution
remain intact. The natural-wait case applies no pokes and reaches age 146 from
the successful B-1 checkpoint. Each case records its commands and checkpoint
digest; each run retains the executing build/report. GPU commands for one
complete back-buffer draw are collected across three consecutive fields:
foreground glyphs sometimes finish in the field after the world/overlay draw.
This avoids treating a partial GPU packet as a complete result layout.

```sh
python3 tools/record_result_presentation.py
python3 tools/record_result_presentation.py --fresh
cargo test -p gran_turismo_2 --all-features --test result_presentation
```

Every retained glyph's character, font, rectangle, encoded RGB and blend mode
matches exactly. Logo selection, all four animated vertices, tint and backdrop
subtraction match exactly as well. Expected values come from the original GPU
commands, not an implementation of the native layout rules.

## Rules and asset contract

Finish begins as opaque colored text. At age 80 it switches to additive text,
brightens for twelve ticks, then separates into two echoes and a centered
copy while fading to black. Its drawing stops at age 150. The background
begins subtractive darkening strictly after age 90, reaching its final level
over sixteen ticks. Failure instead shows an opaque red FAIL at its own
baseline, without that success darkening.

Strictly after age 120, success shows a prize label and the completed time.
The time uses the medium font, centered numeric cells and the original
punctuation spacing. The label uses the large font with one pixel less spacing
between characters. The engine's bitmap-font layout now exposes both centered
numeric cells and adjustable text spacing; existing font users retain their
previous behavior.

Prize text transitions from gray to its medal color over twelve ticks. The
logo rotates and shears into its final rectangle using a decoded curve,
changes tint during that transition, then uses its settled tint. The four
logos and palettes are decoded from `/arcade/license_tim.tim`; the complete
43,520-byte source upload matches captured original VRAM byte for byte.
The running game loads separate PNG images, never that TIM or VRAM data.

The prize display uses the same strict time bands as the award consumer,
with an additional assisted-pass condition from `8002B30C..328`: show that
prize only when the saved test is unpassed and this is exactly the qualifying
attempt. Later slow attempts or an already-passed test show the unqualified
label. Showing a prize does not commit a saved award.

`tools/export_assets.py` exports the medium font, four PNG logos and layered
`race/result-presentation.ron`. That settings asset contains font/image paths,
labels, five prize styles, coordinates, colors, durations and the animation
curve. Logo images carry their native dimensions and transparency. Fonts must
contain the labels and numeric time glyphs. Mods may replace these files or
disable `HudPlugin`; the handmade mod continues to drive without game assets.

## Native checks and limits

The real binary's natural gold run now continues to the settled prize wait.
Its opaque result-font pixels match a source-derived mask within the existing
nine-channel-value quantization bound. The overshoot run checks FAIL the same
way. Both native screens were rendered and inspected beside the original;
logo texture provenance and geometry are verified separately from font pixels.
The world, missing mirror and other outstanding driving presentation are not
claimed to match by these result-overlay checks.

```sh
cargo test -p gran_turismo_2 --all-features --test dev_mcp native_licence_stop_gold -- --ignored
cargo test -p gran_turismo_2 --all-features --test dev_mcp native_licence_stop_overshoot -- --ignored
```

Workspace tests, formatting and Clippy are the routine checks. The engine
sandbox also steps/renders through developer MCP; the example mod's isolated
handmade-assets driving test still passes.

Result sounds, automatic replay playback, replay exit, menu return, name entry,
record insertion, retry/reset and persistent saves remain. Confirmation still
emits the explicit replay request and leaves the parked car in the scene.
Saved achievements must commit on menu return, as established by
[award evidence](licence-award-evidence.md).
