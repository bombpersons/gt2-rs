//! Original wheel-slip preprocessing executions with explicit local velocities.
use bevy::prelude::*;
use engine::prelude::{Region, TickRate};
use engine::testing::{TickApp, headless_app};
use game::vehicle::slip::{SlipTuning, WheelLocalVelocities, WheelSlip, WheelSlipRulesPlugin};
use game::vehicle::{SteeringKinematics, VehicleRulesPlugin};
use std::collections::HashMap;

fn check_recording(csv: &str, expected_count: usize) {
    let tuning: SlipTuning = engine::config::load_ron("vehicle/slip.ron").unwrap();
    let mut lines = csv.lines();
    let columns: Vec<_> = lines.next().unwrap().split(',').collect();
    let mut count = 0;
    for line in lines {
        let row: HashMap<_, _> = columns.iter().copied().zip(line.split(',')).collect();
        let raw = |key: &str| row[key].parse::<f32>().unwrap();
        let result = tuning.evaluate(WheelLocalVelocities {
            forward_mps: [raw("forward") / 4096.0; 4],
            lateral_mps: [raw("lateral") / 4096.0; 4],
        });
        for wheel in 0..4 {
            assert_eq!(result.angle_turns[wheel] * 4096.0, raw("angle"), "case {}: direction", row["case"]);
            assert_eq!(
                result.activation[wheel] * 4096.0,
                raw("activation"),
                "case {}: low-speed activation",
                row["case"]
            );
        }
        count += 1;
    }
    assert_eq!(count, expected_count);
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn original_thresholds_octants_and_signed_normalization() {
    check_recording(
        &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/original/wheel_slip/boundaries.csv"))
            .unwrap(),
        80,
    );
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn held_out_original_unequal_vectors_and_rounding_boundaries() {
    check_recording(
        &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/original/wheel_slip/holdout.csv"))
            .unwrap(),
        20,
    );
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn wheel_local_state_updates_steering_before_controls() {
    use game::vehicle::{DigitalSteeringInput, SteeringResponse, SteeringTuning};
    let mut app = headless_app(TickRate::every_nth_field(Region::Ntsc, 2));
    app.add_plugins((VehicleRulesPlugin, WheelSlipRulesPlugin));
    let car = app
        .world_mut()
        .spawn((
            WheelLocalVelocities { forward_mps: [10.0, 10.0, 0.0, -10.0], lateral_mps: [-10.0, -10.0, 0.0, 10.0] },
            WheelSlip::default(),
            SteeringKinematics { forward_velocity_mps: 25.0, front_slip_turns: [0.0; 2] },
            DigitalSteeringInput { direction: 1.0 },
            SteeringResponse::default(),
        ))
        .id();
    app.run_tick(&[]);
    let slip = app.world().get::<WheelSlip>(car).unwrap();
    assert_eq!(slip.angle_turns, [0.125, 0.125, 0.0, -0.375]);
    assert_eq!(slip.activation, [1.0, 1.0, 0.0, 1.0]);
    let state = *app.world().get::<SteeringKinematics>(car).unwrap();
    assert_eq!(state.front_slip_turns, [0.125; 2]);
    let limits = app.world().resource::<SteeringTuning>().limits(state);
    assert_eq!(app.world().get::<SteeringResponse>(car).unwrap().target_turns, limits[1]);
    assert_eq!(limits[1] * 4096.0, 398.0, "fresh slip opens the extended steering target this tick");
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn slip_plugin_runs_without_the_other_vehicle_features() {
    let mut app = headless_app(TickRate::every_nth_field(Region::Ntsc, 2));
    app.add_plugins(WheelSlipRulesPlugin);
    let wheel = app
        .world_mut()
        .spawn((WheelLocalVelocities { forward_mps: [0.0; 4], lateral_mps: [10.0; 4] }, WheelSlip::default()))
        .id();
    app.run_tick(&[]);
    assert_eq!(app.world().get::<WheelSlip>(wheel).unwrap().angle_turns, [-0.25; 4]);
}
