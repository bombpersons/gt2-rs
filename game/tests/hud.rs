//! Expected characters, positions, colours and blending are actual GPU draws.
use bevy::prelude::*;
use engine::{
    bitmap_font::BitmapFont,
    prelude::load_ron,
    testing::{TickApp, headless_app},
};
use game::race::{
    hud::{HUD_SETTINGS, HudDisplay, HudGlyphDraw, HudRulesPlugin, HudSettings, layout_display},
    session::{DrivingSession, SessionRulesPlugin},
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Frame {
    frame: u32,
    draws: Vec<HudGlyphDraw>,
}
#[derive(Deserialize)]
struct Recording {
    rows: Vec<Frame>,
}
fn check(name: &str) {
    let settings: HudSettings = load_ron(HUD_SETTINGS).unwrap();
    let small: BitmapFont = load_ron(&settings.small_font).unwrap();
    let large: BitmapFont = load_ron(&settings.large_font).unwrap();
    let recording: Recording = serde_json::from_str(
        &std::fs::read_to_string(format!("{}/tests/data/original/hud/{name}.json", env!("CARGO_MANIFEST_DIR")))
            .unwrap(),
    )
    .unwrap();
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(settings.clone())
        .insert_resource(DrivingSession::countdown(120))
        .add_plugins((SessionRulesPlugin, HudRulesPlugin));
    let mut ticks = 0;
    let key = |d: &HudGlyphDraw| (d.large, d.area[0] as i32, d.area[1] as i32, d.character);
    for frame in recording.rows {
        // The GPU consumes the previous physics tick's buffered HUD packet.
        // Active clock trace at source field 158 builds 705 ms while its
        // captured draw is 670 ms, built at field 156. See HUD evidence.
        while ticks < frame.frame / 2 {
            app.run_tick(&[]);
            ticks += 1;
        }
        let view = app.world().resource::<HudDisplay>();
        let mut actual = layout_display(&small, &large, &settings, view);
        let mut expected = frame.draws;
        actual.sort_by_key(key);
        expected.sort_by_key(key);
        assert_eq!(actual, expected, "{name} source field {}: {view:?}", frame.frame);
    }
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires local original HUD draws")]
fn original_countdown_clock_and_fading_start_draws() {
    check("initial");
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires fresh original HUD holdout")]
fn fresh_original_hud_holdout() {
    check("fresh");
}
