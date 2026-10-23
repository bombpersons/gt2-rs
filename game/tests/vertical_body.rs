//! Original-execution comparisons for tilt, vertical force and compression
//! correction, both isolated and following the native suspension producer.
use bevy::prelude::*;
use engine::testing::{TickApp, headless_app};
use game::vehicle::{
    VehiclePose, VehicleStepDuration, VehicleVelocity,
    body_dynamics::{BodyDynamics, BodyDynamicsInput},
    road_contact::{BodyOrientation, RoadContactState},
    suspension::*,
    vertical_body::*,
};
#[path = "support/vertical_body.rs"]
mod original;
fn rows(name: &str) -> Vec<original::Row> {
    let text =
        std::fs::read_to_string(format!("{}/tests/data/original/vertical_body/{name}.csv", env!("CARGO_MANIFEST_DIR")))
            .unwrap();
    let mut lines = text.lines();
    let keys: Vec<_> = lines.next().unwrap().split(',').collect();
    lines.map(|l| keys.iter().zip(l.split(',')).map(|(k, v)| (k.to_string(), v.parse().unwrap())).collect()).collect()
}
fn check(name: &str, connected: bool, count: usize) {
    original::check_rows(rows(name), name, connected, count, false);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original body fixtures")]
fn original_tilt_vertical_impulses_reset_and_compression() {
    check("boundaries", false, 161);
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original body fixtures")]
fn original_connected_suspension_and_body() {
    check("boundaries", true, 161);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires fresh original body holdout")]
fn independent_original_body_holdout() {
    check("holdout", false, 60);
    check("holdout", true, 60);
}

#[test]
fn handmade_body_plugin_runs_alone_with_optional_force_bridge_and_per_car_setup() {
    let tuning = VerticalBodyTuning {
        pitch_response: [0.0; 2],
        roll_response: [0.0; 2],
        inverse_mass: 0.25,
        minimum_tilt_rate: -0.25,
        maximum_tilt_rate: 0.25,
        maximum_tilt_turns: 0.125,
        gravity_mps2: 2.0,
        minimum_travel_m: [-0.25; 2],
        travel_offset_m: [0.0; 2],
        wheel_levers_m: [[-0.5, 1.0], [0.5, 1.0], [-0.5, -1.0], [0.5, -1.0]],
        wheelbase_m: 2.0,
        track_m: 1.0,
        rear_height_fraction: 0.5,
        angle_threshold_ratios: vec![0.5],
        reset_height_m: 0.25,
        reset_seconds: 5.0,
        reset_ramp_seconds: 0.5,
        reset_heading_turns_per_second: 0.125,
    };
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(tuning.clone()).add_plugins(VerticalBodyRulesPlugin);
    let mut cars = Vec::new();
    for airborne in [false, true] {
        let car = app
            .world_mut()
            .spawn((
                VerticalBodyInput::default(),
                VerticalBody::default(),
                SuspensionInput { road_normal_up: 1.0, body_up: 1.0, ..default() },
                Suspension {
                    body_force: [4096.0; 4],
                    tyre_load: if airborne { [0.0; 4] } else { [1.0; 4] },
                    ..default()
                },
                VehicleStepDuration(0.25),
                VehiclePose::default(),
                VehicleVelocity::default(),
                BodyOrientation::default(),
                BodyDynamics::default(),
                RoadContactState::default(),
            ))
            .id();
        cars.push(car);
    }
    let mut variant = tuning;
    variant.inverse_mass = 0.5;
    app.world_mut().entity_mut(cars[1]).insert((VerticalBodySetup(variant), BodyDynamicsInput::default()));
    app.run_tick(&[]);
    assert_eq!(app.world().get::<VehicleVelocity>(cars[0]).unwrap().linear_mps, Vec3::NEG_Y * 0.25);
    assert_eq!(app.world().get::<VehicleVelocity>(cars[1]).unwrap().linear_mps, Vec3::ZERO);
    assert!(!app.world().get::<VerticalBody>(cars[0]).unwrap().airborne);
    assert!(app.world().get::<BodyDynamicsInput>(cars[1]).unwrap().bypass_tyre_acceleration);
    // Both caller-level countdown and mode-7 map to this skip and retain
    // motion, flags and contact history without evaluating another impulse.
    app.world_mut().get_mut::<SuspensionInput>(cars[0]).unwrap().disabled = true;
    app.run_tick(&[]);
    assert_eq!(app.world().get::<VehicleVelocity>(cars[0]).unwrap().linear_mps, Vec3::NEG_Y * 0.25);
}
