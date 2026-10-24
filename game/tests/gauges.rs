//! Expected geometry, rectangles, colour and blends come from original GPU draws.
use engine::prelude::load_ron;
use game::race::gauges::{GAUGE_SETTINGS, GaugeDisplay, GaugeDraw, GaugeSettings, layout_gauges};
use serde::Deserialize;

#[derive(Deserialize)]
struct Row {
    name: String,
    view: GaugeDisplay,
    draws: Vec<GaugeDraw>,
}
#[derive(Deserialize)]
struct Recording {
    rows: Vec<Row>,
}
fn check(name: &str) {
    let settings: GaugeSettings = load_ron(GAUGE_SETTINGS).unwrap();
    let path = format!("{}/tests/data/original/gauges/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let data: Recording = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    for row in data.rows {
        let actual = layout_gauges(&settings, &row.view);
        assert_eq!(actual.len(), row.draws.len(), "{} {:?}", row.name, row.view);
        for (i, (a, b)) in actual.iter().zip(&row.draws).enumerate() {
            assert_eq!(a, b, "{} {:?} draw {i}", row.name, row.view);
        }
    }
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires locally exported gauges and original draws")]
fn original_driving_gauges_and_controlled_edges() {
    check("initial");
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires fresh original gauge holdout")]
fn fresh_original_driving_gauges() {
    check("fresh");
}
