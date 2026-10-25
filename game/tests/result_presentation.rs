//! Layout expectations are actual original GPU draws, never a reference model.
use engine::{bitmap_font::BitmapFont, prelude::load_ron};
use game::race::{
    licence::LicenceDefinition,
    licence_awards::{LicenceAchievement, LicenceGrade},
    licence_results::{LicenceResult, LicenceResultTiming, RESULT_TIMING},
    licence_stop::LicenceFailure,
    result_presentation::*,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Row {
    name: String,
    passed: bool,
    age: i16,
    time_ms: u32,
    grade: usize,
    attempts: u8,
    definition: LicenceDefinition,
    layout: ResultLayout,
}
#[derive(Deserialize)]
struct Recording {
    rows: Vec<Row>,
}
fn check(name: &str) {
    let s: ResultPresentationSettings = load_ron(RESULT_PRESENTATION).unwrap();
    let timing: LicenceResultTiming = load_ron(RESULT_TIMING).unwrap();
    let large: BitmapFont = load_ron(&s.large_font).unwrap();
    let time: BitmapFont = load_ron(&s.time_font).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/data/original/result_presentation/{name}.json"));
    let r: Recording = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(r.rows.len(), if name == "initial" { 25 } else { 12 });
    let grades = [
        LicenceGrade::Unpassed,
        LicenceGrade::AssistedPass,
        LicenceGrade::Bronze,
        LicenceGrade::Silver,
        LicenceGrade::Gold,
    ];
    let key = |g: &ResultGlyphDraw| (g.time_font, g.area[0] as i32, g.area[1] as i32, g.character, g.rgb, g.additive);
    for row in r.rows {
        let result = if row.age < 0 {
            LicenceResult::Driving
        } else if row.passed {
            LicenceResult::Success { time_ms: row.time_ms, age_ticks: row.age as u16 }
        } else {
            LicenceResult::Failure { reason: LicenceFailure::Overshot, age_ticks: row.age as u16 }
        };
        let achievement = LicenceAchievement { grade: grades[row.grade], assisted_attempts: row.attempts };
        let mut actual = layout_result(&large, &time, &s, &timing, &result, Some(&row.definition), achievement);
        let mut expected = row.layout;
        actual.glyphs.sort_by_key(key);
        expected.glyphs.sort_by_key(key);
        assert_eq!(actual.glyphs.len(), expected.glyphs.len(), "{} glyph count", row.name);
        for (a, b) in actual.glyphs.iter().zip(&expected.glyphs) {
            assert_eq!(a, b, "{}", row.name);
        }
        assert_eq!(actual, expected, "{}", row.name);
    }
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original result overlay draws")]
fn original_finish_failure_prizes_and_animation_boundaries() {
    check("initial");
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires fresh original result overlay draws")]
fn fresh_original_result_presentation_holdout() {
    check("fresh");
}
