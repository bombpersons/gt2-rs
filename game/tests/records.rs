use bevy::prelude::*;
use engine::{
    bitmap_font::BitmapFont,
    prelude::load_ron,
    testing::{TickApp, headless_app},
};
use game::race::{
    hud::HudGlyphDraw,
    licence::{LICENCE_DEFINITION, LicenceDefinition, LicenceRecord, LicenceRecords},
    records::{RECORDS_SETTINGS, RecordsDisplay, RecordsLayout, RecordsRulesPlugin, RecordsSettings, layout_records},
};
use serde::Deserialize;
#[derive(Deserialize)]
struct Row {
    name: String,
    record: Option<LicenceRecord>,
    layout: RecordsLayout,
    definition: Option<LicenceDefinition>,
}
#[derive(Deserialize)]
struct Recording {
    rows: Vec<Row>,
}
fn check(name: &str) {
    let s: RecordsSettings = load_ron(RECORDS_SETTINGS).unwrap();
    let definition: LicenceDefinition = load_ron(LICENCE_DEFINITION).unwrap();
    let font: BitmapFont = load_ron(&s.font).unwrap();
    let path = format!("{}/tests/data/original/records/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let data: Recording = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(s.clone())
        .insert_resource(definition.clone())
        .insert_resource(LicenceRecords::default())
        .add_plugins(RecordsRulesPlugin);
    assert!(!app.world().resource::<RecordsDisplay>().visible);
    let key = |d: &HudGlyphDraw| (d.area[1] as i32, d.area[0] as i32, d.character);
    for row in data.rows {
        let definition = row.definition.unwrap_or_else(|| definition.clone());
        app.insert_resource(definition.clone());
        app.world_mut().resource_mut::<LicenceRecords>().best.clear();
        if let Some(record) = row.record {
            app.world_mut().resource_mut::<LicenceRecords>().best.insert(definition.id.clone(), record);
        }
        app.run_tick(&[]);
        let mut actual = layout_records(&font, &s, app.world().resource::<RecordsDisplay>());
        let mut expected = row.layout;
        actual.glyphs.sort_by_key(key);
        expected.glyphs.sort_by_key(key);
        assert_eq!(actual.quads, expected.quads, "{} quads", row.name);
        assert_eq!(actual.glyphs.len(), expected.glyphs.len(), "{} glyph count", row.name);
        for (a, b) in actual.glyphs.iter().zip(expected.glyphs) {
            assert_eq!(*a, b, "{}", row.name);
        }
    }
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires local original records-panel draws")]
fn original_record_panel_and_saved_record_boundaries() {
    check("initial");
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires fresh original records-panel draws")]
fn fresh_record_panel_and_time_format_holdout() {
    check("fresh");
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original target-panel probes for all licence rows")]
fn all_licence_medal_targets_match_original_hud() {
    check("catalogue");
}

#[test]
fn authored_records_rules_run_alone_and_follow_selected_definition() {
    use game::race::licence::LicenceRecord;
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.add_plugins(RecordsRulesPlugin);
    app.run_tick(&[]);
    assert!(!app.world().resource::<RecordsDisplay>().visible);
    // An authored panel needs no game asset directory, renderer or session.
    let settings: RecordsSettings = ron::from_str(
        r#"(font:"handmade.ron", label:"Best", missing_time:"?", units:"km/h",
        origin_px:(0.0,0.0), label_offset_px:(0.0,0.0), time_offset_px:(0.0,10.0),
        speed_offset_px:(0.0,20.0), target_offsets_px:((0.0,30.0),(0.0,40.0),(0.0,50.0)),
        text_rgb:(128,128,128), underline_offset_px:(0.0,0.0),
        underline_size_extra_px:(2.0,1.0), underline_rgb:((10,10,10),(20,20,20)),
        marker_rgb:((200,100,0),(100,100,100),(150,50,0)), marker_offset_px:(-50.0,0.0),
        marker_inner_px:3.0, marker_border_px:1.0,
        cells:(digit_px:6.0,punctuation_px:3.0,decimal_inset_px:0.0,origin_inset_px:0.0),
        metadata_digit_px:6.0,metadata_digit_inset_px:0.0)"#,
    )
    .unwrap();
    app.insert_resource(settings)
        .insert_resource(LicenceDefinition {
            id: "hill".into(),
            time_limits_ms: [10000, 20000, 30000, 40000],
            assisted_pass_attempts: None,
        })
        .insert_resource(LicenceRecords {
            best: [("circuit".into(), LicenceRecord { time_ms: 12345, speed_readout: 12678 })].into(),
        });
    app.run_tick(&[]);
    let display = app.world().resource::<RecordsDisplay>();
    assert!(display.visible);
    assert_eq!(display.time, "?");
    assert!(display.speed.is_empty());
    app.world_mut().resource_mut::<LicenceDefinition>().id = "circuit".into();
    app.run_tick(&[]);
    let display = app.world().resource::<RecordsDisplay>();
    assert_eq!(display.time, "0:12.345");
    assert_eq!(display.speed, "126.78km/h");
}
