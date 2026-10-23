# README screenshots

The README images are unedited PNGs captured from the native Bevy binaries
through their developer MCP's `game_screenshot` tool, using a software GPU.
They are not emulator screenshots or extracted textures. The B-1 images use
locally exported course/car assets, at the same recorded reference pose.
The example mod uses only handmade assets.

| Image | Capture settings |
|---|---|
| `docs/images/b1-ps1.png` | B-1 near chase, 320×240; vertex snapping, dithering and 15-bit colour on |
| `docs/images/b1-widescreen.png` | Same pose and near chase, 640×360; all three look switches off |
| `docs/images/example-mod.png` | Handmade example circuit/car, near chase, 320×240; PS1 look on |

These depict the current renderer. B-1's ground and sky are provisional,
the car and wheels are static, and the driving session is unfinished. The
widescreen image changes the actual render target and camera aspect; it is
not a resized or cropped 4:3 image. Turning off the three global look switches
does not change the imported assets or replace the renderer's affine textures.

Reproduce from the repository root after [exporting local assets](assets.md):

```sh
cargo build --workspace --all-features
python3 tools/capture_readme.py
```

The script isolates display settings from player preferences, waits for the
initial GPU frame, selects near chase through the public R1 camera control,
then captures the screen target. It records the source commit, image digests,
commands, display settings, game status and logs under ignored
`.psx/readme-capture/`. Inspect all three images before publishing an update.

At the user's request, these three named presentation images are the only
documentation-image exceptions in `tools/check_public_repo.py`. No generated
runtime assets, tuning tables, numerical fixtures, original screenshots,
disc/BIOS data or analysis captures may be added with them. The regular
current-tree and full-history publication audits still apply.
