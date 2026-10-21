//! Original per-tick numeric observations, without fitting a force model.
//! Effective control inputs and motion deltas were read at their consuming
//! instructions. The original serial pad latency is outside this boundary.
use bevy::prelude::*;
use engine::controller::Button;
use engine::prelude::{Region, TickRate};
use engine::testing::{TickApp, headless_app};
use game::vehicle::slip::{WheelLocalVelocities, WheelSlip, WheelSlipRulesPlugin};
use game::vehicle::{
    DigitalPedalInput, DigitalSteeringInput, MotionStep, PedalTravel, PlayerPedals, SteeringKinematics,
    SteeringResponse, SteeringTuning, VehiclePose, VehicleRulesPlugin, VehicleStepDuration, VehicleVelocity,
    WheelSteering,
};
use std::collections::HashMap;

fn rows(name: &str) -> Vec<HashMap<String, i32>> {
    let path = format!("{}/tests/data/original/driving/{name}.ticks.csv", env!("CARGO_MANIFEST_DIR"));
    let csv = std::fs::read_to_string(path).unwrap();
    let mut lines = csv.lines();
    let columns: Vec<_> = lines.next().unwrap().split(',').collect();
    lines
        .map(|line| {
            columns.iter().zip(line.split(',')).map(|(key, value)| (key.to_string(), value.parse().unwrap())).collect()
        })
        .collect()
}

fn position(row: &HashMap<String, i32>, prefix: &str) -> Vec3 {
    Vec3::new(
        row[&format!("{prefix}_x")] as f32,
        row[&format!("{prefix}_y")] as f32,
        -row[&format!("{prefix}_z")] as f32,
    ) / 4096.0
}

fn check_original(name: &str) {
    let original = rows(name);
    assert!(original.len() >= 30, "{name}: at least one second of physics");
    let start = &original[0];
    let mut app = headless_app(TickRate::every_nth_field(Region::Ntsc, 2));
    app.add_plugins((VehicleRulesPlugin, WheelSlipRulesPlugin));
    let car = app
        .world_mut()
        .spawn((
            Name::new("Player"),
            PlayerPedals,
            DigitalPedalInput::default(),
            DigitalSteeringInput::default(),
            SteeringKinematics::default(),
            WheelLocalVelocities::default(),
            WheelSlip::default(),
            WheelSteering::default(),
            SteeringResponse {
                angle_turns: start["before_steering"] as f32 / 4096.0,
                rate_turns_per_second: start["before_steering_rate"] as f32 / 4096.0,
                target_turns: 0.0,
            },
            PedalTravel {
                throttle: start["before_throttle"] as f32 / 4096.0,
                brake: start["before_brake"] as f32 / 4096.0,
                handbrake: start["before_handbrake"] as f32 / 4096.0,
            },
            VehiclePose {
                position_m: position(start, "before"),
                heading_turns: start["before_heading"] as f32 / 4096.0,
            },
            MotionStep::default(),
            VehicleVelocity::default(),
            VehicleStepDuration::default(),
        ))
        .id();

    for (tick, row) in original.iter().enumerate() {
        assert_eq!(row["run_frame"], tick as i32 * 2, "{name}: original physics cadence");
        assert_eq!(row["input_flags"], 0, "{name}: this test covers the digital input path");
        let mut buttons = Vec::new();
        if row["input_throttle"] != 0 {
            buttons.push(Button::Cross);
        }
        if row["input_brake"] != 0 {
            buttons.push(Button::Square);
        }
        if row["input_handbrake"] != 0 {
            buttons.push(Button::Circle);
        }
        if row["input_steering"] > 0 {
            buttons.push(Button::Left);
        } else if row["input_steering"] < 0 {
            buttons.push(Button::Right);
        }
        // The force/contact model supplies wheel-local velocities before
        // the original slip preprocessing. The native plugin computes front
        // slip and passes it to steering in this same tick.
        *app.world_mut().get_mut::<WheelLocalVelocities>(car).unwrap() = WheelLocalVelocities {
            forward_mps: std::array::from_fn(|i| row[&format!("wheel{i}_forward")] as f32 / 4096.0),
            lateral_mps: std::array::from_fn(|i| row[&format!("wheel{i}_lateral")] as f32 / 4096.0),
        };
        *app.world_mut().get_mut::<SteeringKinematics>(car).unwrap() = SteeringKinematics {
            forward_velocity_mps: row["before_forward_velocity"] as f32 / 4096.0,
            front_slip_turns: [0.0; 2],
        };
        // Feed the original velocities at the step-building boundary. Their
        // source remains the unported force model, but constructing and
        // integrating a step now both run through the native plugin.
        let velocity = VehicleVelocity {
            linear_mps: Vec3::new(row["step_vx"] as f32, row["step_vy"] as f32, -row["step_vz"] as f32) / 4096.0,
            yaw_turns_per_second: row["step_yaw_rate"] as f32 / 524288.0,
        };
        let seconds = row["step_dt"] as f32 / 65536.0;
        let step = velocity.step(seconds);
        assert_eq!(
            step.translation_m * 4096.0,
            Vec3::new(row["dx"] as f32, row["dy"] as f32, -row["dz"] as f32),
            "{name} tick {tick}: constructed displacement"
        );
        assert_eq!(step.heading_turns * 4096.0, row["dheading"] as f32, "{name} tick {tick}: constructed yaw");
        *app.world_mut().get_mut::<VehicleVelocity>(car).unwrap() = velocity;
        app.world_mut().get_mut::<VehicleStepDuration>(car).unwrap().0 = seconds;
        app.run_tick(&buttons);
        let slip = app.world().get::<WheelSlip>(car).unwrap();
        assert_eq!(
            slip.angle_turns[0] * 4096.0,
            row["input_front_left_slip"] as f32,
            "{name} tick {tick}: front left slip"
        );
        assert_eq!(
            slip.angle_turns[1] * 4096.0,
            row["input_front_right_slip"] as f32,
            "{name} tick {tick}: front right slip"
        );
        let pose = app.world().get::<VehiclePose>(car).unwrap();
        assert_eq!(pose.position_m, position(row, "after"), "{name} tick {tick}: position");
        assert_eq!(pose.heading_turns * 4096.0, row["after_heading"] as f32, "{name} tick {tick}: heading");
        let travel = app.world().get::<PedalTravel>(car).unwrap();
        for (key, actual) in [("throttle", travel.throttle), ("brake", travel.brake), ("handbrake", travel.handbrake)] {
            assert_eq!(actual * 4096.0, row[&format!("after_{key}")] as f32, "{name} tick {tick}: {key}");
        }
        let steering = app.world().get::<SteeringResponse>(car).unwrap();
        assert_eq!(steering.angle_turns * 4096.0, row["after_steering"] as f32, "{name} tick {tick}: steering");
        assert_eq!(
            steering.rate_turns_per_second * 4096.0,
            row["after_steering_rate"] as f32,
            "{name} tick {tick}: steering response rate"
        );
        let kinematics = *app.world().get::<SteeringKinematics>(car).unwrap();
        let limits = app.world().resource::<SteeringTuning>().limits(kinematics);
        assert_eq!(
            limits[0] * 4096.0,
            row["after_steering_negative_limit"] as f32,
            "{name} tick {tick}: negative target"
        );
        assert_eq!(
            limits[1] * 4096.0,
            row["after_steering_positive_limit"] as f32,
            "{name} tick {tick}: positive target"
        );
        let wheels = app.world().get::<WheelSteering>(car).unwrap();
        for (i, key) in ["front_left_steering", "front_right_steering", "rear_left_steering", "rear_right_steering"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                wheels.angle_turns[i] * 4096.0,
                row[&format!("after_{key}")] as f32,
                "{name} tick {tick}: {key}"
            );
        }
    }
    // Explicit steps are consumed; absent a force model, the plugin does not
    // silently keep applying the previous tick's displacement.
    let final_pose = *app.world().get::<VehiclePose>(car).unwrap();
    app.world_mut().entity_mut(car).remove::<VehicleVelocity>();
    app.run_tick(&[]);
    assert_eq!(app.world().get::<VehiclePose>(car).unwrap().position_m, final_pose.position_m);
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn digital_throttle_and_recorded_steps_match_original() {
    check_original("gas");
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn released_pedals_and_recorded_steps_match_original() {
    check_original("coast");
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn digital_brake_and_recorded_steps_match_original() {
    check_original("brake");
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn digital_handbrake_and_recorded_steps_match_original() {
    check_original("handbrake");
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn left_turn_step_integration_matches_original() {
    check_original("left");
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn right_turn_step_integration_matches_original() {
    check_original("right");
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn held_out_simultaneous_and_alternating_pedals_match_original() {
    check_original("mixed_holdout");
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn held_out_steering_reversal_and_centering_match_original() {
    check_original("steering_holdout");
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn held_out_velocity_step_construction_and_integration_match_original() {
    check_original("motion_holdout");
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn original_steering_curve_slip_limits_and_spring_boundary_probes() {
    let tuning: SteeringTuning = engine::config::load_ron("vehicle/steering.ron").unwrap();
    let csv = &std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/original/driving/steering_probes.csv"
    ))
    .unwrap();
    let mut lines = csv.lines();
    let columns: Vec<_> = lines.next().unwrap().split(',').collect();
    let mut count = 0;
    for line in lines {
        let row: HashMap<_, _> = columns.iter().copied().zip(line.split(',')).collect();
        let raw = |key: &str| row[key].parse::<f32>().unwrap();
        let state = SteeringKinematics {
            forward_velocity_mps: raw("speed") / 4096.0,
            front_slip_turns: [raw("left_slip") / 4096.0, raw("right_slip") / 4096.0],
        };
        let limits = tuning.limits(state);
        assert_eq!(limits[0] * 4096.0, raw("steering_negative_limit"), "{}: negative limit", row["case"]);
        assert_eq!(limits[1] * 4096.0, raw("steering_positive_limit"), "{}: positive limit", row["case"]);
        let mut response = SteeringResponse {
            angle_turns: raw("angle") / 4096.0,
            rate_turns_per_second: raw("rate") / 4096.0,
            target_turns: 0.0,
        };
        response.advance(DigitalSteeringInput { direction: raw("direction") }, &tuning, state);
        assert_eq!(response.angle_turns * 4096.0, raw("steering"), "{}: angle", row["case"]);
        assert_eq!(response.rate_turns_per_second * 4096.0, raw("steering_rate"), "{}: rate", row["case"]);
        let wheels = WheelSteering::from_response(response, &tuning);
        for (i, key) in ["front_left_steering", "front_right_steering", "rear_left_steering", "rear_right_steering"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(wheels.angle_turns[i] * 4096.0, raw(key), "{}: {key}", row["case"]);
        }
        count += 1;
    }
    assert_eq!(count, 39, "all independent original boundary probes must be present");
}
