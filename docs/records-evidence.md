# Licence records panel and target definitions

The native B-1 HUD now shows its record label/underline, personal best time,
recorded speed, and gold/silver/bronze target times with coloured markers.
It reads native licence definitions and personal-record state. This does
not yet complete finish/failure detection, award progression, saving or retry.

## Original data and shared routines

The original's B-1 session selects key `LJB00`. `8007830C` looks that key up
in the relocated GTDT table, whose root pointer is stored at `80092E6C` and
whose licence descriptor is at root `+0xF8`. Its 60 rows have
156-byte strides and use a local length-prefixed key-string table. The
exporter resolves this table through the GTDT header, matches the selected
key, and reads the four time pairs at row `+0x6C`. It validates every row's
key, time fields and ordering. No runtime source pointers or packed time
fields survive conversion.

`8003D7B8` decodes the first byte as minutes×100 plus seconds, followed by
hundredths. B-1 yields 38,650 / 38,900 / 39,800 / 41,000 milliseconds.
The panel displays the first three. `race/licence.ron` holds the native
`LicenceDefinition { id, time_limits_ms }`, including the fourth original
award threshold for later rule work.

`8002D420..484` selects a personal record using the licence group/test
indices. Its time is milliseconds; `0xFFFFFFFF` means absent. The record's
`+0x10` halfword supplies the displayed speed. The exporter writes the selected
checkpoint record, if any, into `race/records.initial.ron`, a native map from
licence keys to `LicenceRecord { time_ms, speed_readout }`. B-1's supplied
checkpoint has no record. This file initializes in-memory state; it is not
a native save system, and changes during play are not persisted yet.

| Behaviour | Original evidence |
|---|---|
| Licence-mode panel, label, selected record and medal targets | `8002D308..660`, dispatch mode 3 |
| Record time and optional speed line | `8002D058..128` |
| Medal times and two nested marker rectangles | `8002D12C..208` |
| Millisecond/hour/missing-time formatting | `80068734..68AFC` |
| Speed text, without zero padding in its fractional part | `80068CA0..D08`, `%d.%d` then the units string |
| Numeric speed-character cells mixed with ordinary text spacing | `8006AF40..B0E8`, right-aligned by `8006B184` |
| Shared numeric time cells | `8006B3F4`; also used by the existing lap clock |
| Shaded label underline | `8006B77C`, descriptor at `8002F8E0` |

The time formatter keeps milliseconds below one hour, then switches to
`hours:minutes:seconds.tenths`. At 100 hours it displays the missing-time
label. The original absent-time label is `--:--:---`, including both colons.
The saved speed formatter uses `speed/100` and `speed%100`, with no fractional
zero padding: source readout 109 displays `1.9mph` on this USA setup.

`race/records-panel.ron` stores ordinary pixel offsets, colours, marker sizes,
label text, font path and numeric spacing. It is replaceable through asset
layers. The existing small font supplies all glyphs; no additional bitmap is
required. `BitmapFont::layout_number` now owns the shared time-cell layout,
with an independently authored numeric-font test. All nineteen earlier
clock/announcement draw comparisons still pass after that extraction.

## Native feature boundary

`LicencePlugin` loads the optional definition and initial records, and
registers their reflected resource types. It is separately selectable in
`GamePlugins`. `RecordsRulesPlugin` consumes those resources and optional
`RecordsSettings` without a renderer; `RecordsDisplay` remains hidden until
the first fixed tick, matching the opening upload-only field.

`HudPlugin` includes `RecordsPlugin`, which draws ordinary bitmap sprites
and shaded `Mesh2d` rectangles under the shared `Camera2d`/`PsxView`. The
panel anchors to the upper right. A standalone caller supplies a camera,
the public definition/records/settings resources and its own font. Updating
the native records map or definition updates the display on the next tick.
The handmade mod disables `LicencePlugin` and `HudPlugin`, retaining its
authored driving/session features without original content dependencies.
An authored-content test also runs the records rules with neither a session
nor a renderer, then changes the selected definition and checks record selection.

## Original comparisons

`tools/record_records.py` captures actual source glyph primitives, underline
vertices/colours and nested marker rectangles. Source GPU textures and CLUTs
provide the opaque-glyph pixel masks; the untextured opaque rectangles also
contribute their full coverage. Expected layout comes from original commands,
never a native layout function.

The initial set contains four unmodified frames and seven controlled saved
record cases. Probes change only the personal-record time and speed fields,
then let the original panel draw them. Ten fresh frames cover additional
natural boundaries, hours, the 100-hour cutoff and large unsigned values.
All **21** frames match in glyphs, pixel rectangles, colours and shading
rectangles, including the saved speed line.

An additional **60** controlled captures change only the selected licence
key and ask the original HUD to display every target-data row. The exporter
supplies native input definitions, while the original's own decoder and GPU
draws supply expected text. All 180 displayed target times match, as do the
labels/marker layouts. These are target-table/presentation probes with the
B-1 scene still loaded, not playable tests of all sixty licences. The fourth
threshold is structurally decoded but its award behaviour remains unported.

The real-binary pixel check loads each saved-record case as native RON,
steps the normal scene to the observed field boundary, and compares all
source-masked pixels. It does not supply original RAM, GPU packets or VRAM
to the native process. Captures and fixtures stay in ignored directories.
All 21 native frames pass: **42,244** source-masked pixels, with a maximum
encoded-channel difference of **9** (the existing nine-level raster tolerance).

```sh
python3 tools/record_records.py
python3 tools/record_records.py --fresh
python3 tools/record_records.py --catalogue
cargo test -p gran_turismo_2 --all-features --test records
cargo test -p gran_turismo_2 --all-features --test dev_mcp native_records \
  -- --ignored --nocapture
```

Verification also passes the full workspace suite, all eight game GPU/MCP
checks, the handmade mod's GPU driving check and the engine-only sandbox.
Clippy passes with the existing wheel-rotation test warning. A normal asset
export regenerates all 180 manifest entries, whose file hashes were checked.
The public-repository history audit passes; all reference data stays private.

## Remaining licence flow

Target decoding also located the original award/update routine at
`8004DD80..DEF4`. Its comparisons are strict, and it has an attempt-count
branch associated with the fourth threshold. Neither a display target nor
the session clock alone proves an original finish condition. Runtime finish,
stopping/failure gates, the exact award/attempt update, result presentation,
retry/reset and persistent progression still require original recordings
and native rules. Mirror and sound work also remain.
