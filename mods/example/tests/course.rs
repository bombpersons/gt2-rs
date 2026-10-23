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
    let camera_x = camera.translation.x;
    let car = app.world_mut().query::<&game::vehicle::VehiclePose>().single(app.world()).unwrap();
    assert!((camera_x - car.position_m.x - 9.0).abs() < 0.01);
    assert!(car.position_m.x < 50.0, "the native vehicle already advances on the first tick");
}

fn driving_app() -> (App, Entity) {
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(game::race::RacePresentation { geometry: None });
    app.init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<PsxMaterial>>()
        .init_resource::<Assets<Image>>()
        .add_plugins(example_mod::plugins());
    app.run_tick(&[]);
    let car = app.world_mut().query_filtered::<Entity, With<game::vehicle::VehiclePose>>().single(app.world()).unwrap();
    (app, car)
}

#[test]
fn handmade_car_uses_native_throttle_brakes_steering_and_contact() {
    use engine::controller::Button;
    use game::vehicle::{VehiclePose, VehicleVelocity, road_contact::RoadContactState, suspension::Suspension};
    let mut speeds = Vec::new();
    for buttons in [vec![Button::Cross], vec![], vec![Button::Square], vec![Button::Circle]] {
        let (mut app, car) = driving_app();
        for _ in 0..60 {
            app.run_tick(&buttons);
        }
        let velocity = app.world().get::<VehicleVelocity>(car).unwrap();
        let pose = app.world().get::<VehiclePose>(car).unwrap();
        let road = app.world().get::<RoadContactState>(car).unwrap();
        let suspension = app.world().get::<Suspension>(car).unwrap();
        println!("{buttons:?}: position {:?}, speed {:.3} m/s", pose.position_m, velocity.linear_mps.length());
        assert!(pose.position_m.is_finite() && velocity.linear_mps.is_finite());
        assert!(pose.position_m.y.abs() < 0.15, "car must stay on the authored floor");
        assert_eq!(road.missing_mask, 0, "all four wheels sample authored collision");
        assert!(suspension.tyre_load.iter().all(|v| *v > 0.0));
        speeds.push(velocity.linear_mps.xz().length());
    }
    assert!(speeds[0] > 8.0, "throttle must accelerate beyond the initial 7 m/s: {speeds:?}");
    assert!(speeds[0] > speeds[1] + 1.0, "throttle vs coast: {speeds:?}");
    assert!(speeds[2] < speeds[1] - 1.0, "braking vs coast: {speeds:?}");
    assert!(speeds[3] < speeds[1] - 0.5, "handbrake vs coast: {speeds:?}");
    let mut headings = Vec::new();
    for direction in [Button::Left, Button::Right] {
        let (mut app, car) = driving_app();
        for _ in 0..30 {
            app.run_tick(&[Button::Cross, direction]);
        }
        let pose = app.world().get::<VehiclePose>(car).unwrap();
        println!("{direction:?}: {pose:?}");
        headings.push(pose.heading_turns);
    }
    assert!(
        headings[0] > 0.26 && headings[1] < 0.24,
        "both steering directions must turn the native body: {headings:?}"
    );
}

#[test]
fn all_vehicle_data_resolves_to_handmade_assets() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    for name in [
        "pedals",
        "steering",
        "slip",
        "longitudinal_slip",
        "braking",
        "brake_assist",
        "driver_assist",
        "engine",
        "tyres",
        "tyre_grip",
        "forward_dynamics",
        "wheel_rotation",
        "drivetrain",
        "transmission",
        "body_dynamics",
        "contact_motion",
        "aerodynamics",
        "road_contact",
        "suspension",
        "vertical_body",
        "visual",
        "initial",
    ] {
        let path = format!("vehicle/{name}.ron");
        assert_eq!(engine::config::asset_path(&path), root.join(path));
    }
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
