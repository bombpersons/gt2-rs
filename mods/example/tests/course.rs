//! The mod's override is read by the game's real loader and renderer.
use bevy::prelude::*;
use engine::prelude::*;
use engine::psx::PsxMaterial;
use engine::testing::{TickApp, headless_app};
use game::race::{COURSE_ASSET, RaceCourse, RacePlugin};

#[test]
fn the_game_runs_with_only_the_mods_hand_made_course() {
    // No base asset root: this must work without the game's extracted files.
    let mut app = headless_app(game::tick_rate(game::REGION));
    // Match the mod binary's explicit choice of hand-authored ribbon geometry.
    app.insert_resource(game::race::RacePresentation { geometry: None });
    app.init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<PsxMaterial>>()
        .init_resource::<Assets<Image>>()
        .add_plugins(example_mod::plugins());
    app.run_tick(&[]);
    let course = &app.world().resource::<RaceCourse>().0;
    assert_eq!(course.lap_length_m, 320.0);
    assert_eq!(course.lap(), vec![0, 1, 2, 3]);
    assert_eq!(course.surface_quads().len(), 4);
    assert_eq!(game::race::start_span(course).unwrap().node, 2);
    let start = course.point_at(game::race::START_DISTANCE_M).unwrap();
    assert!((start.position_m[0] - 49.7914).abs() < 0.001);
    assert_eq!(start.position_m[2], 80.0);
    assert!(game::race::road_mesh(course).count_vertices() > 0);
    let camera = app
        .world_mut()
        .query::<(&Name, &Transform)>()
        .iter(app.world())
        .find(|(name, _)| name.as_str() == "RaceCamera")
        .unwrap()
        .1;
    assert!((camera.translation.x - 58.7914).abs() < 0.001);
}

#[test]
fn layering_the_base_game_keeps_the_mods_override() {
    engine::assets::add_root(example_mod::GAME_ASSETS);
    let path = engine::config::asset_path(COURSE_ASSET);
    assert_eq!(path, std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join(COURSE_ASSET));
    let course: CourseData = load_ron(COURSE_ASSET).unwrap();
    assert_eq!(course.nodes.len(), 4);
}

#[test]
fn a_mod_can_disable_the_games_scene() {
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.add_plugins(example_mod::plugins().disable::<RacePlugin>());
    app.run_tick(&[]);
    assert!(!app.world().contains_resource::<RaceCourse>());
}
