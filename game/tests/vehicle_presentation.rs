//! Observed render outputs and retained chase springs, independent of a GPU.
use bevy::prelude::*;
use engine::{
    prelude::load_ron,
    testing::{TickApp, headless_app},
};
use game::vehicle::{
    VehiclePose, VehicleStepDuration, VehicleVelocity, WheelSteering,
    contact_motion::ContactMotion,
    presentation::{WHEEL_LAYOUT_ASSET, WheelLayout},
    road_contact::{BodyOrientation, RoadContactState, VehicleBasis},
    suspension::Suspension,
    visual_motion::{ChaseStabilization, VehicleVisualMotionPlugin},
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Snapshot {
    position: [i32; 3],
    angles: [i16; 3],
    velocity: [i32; 3],
    duration: i16,
    tracking: [i16; 5],
    correction: [i16; 2],
    body_basis: [i16; 12],
    wheel_travel: [i16; 4],
    wheel_heading: [i16; 4],
    wheel_spin: [u16; 4],
    body_matrix: [i16; 9],
    body_position: [i32; 3],
    wheel_positions: [[i16; 3]; 4],
    #[serde(default)]
    camber: [i16; 2],
}
#[derive(Deserialize)]
struct Row {
    run_frame: usize,
    before: Snapshot,
    stabilized: Snapshot,
    presented: Snapshot,
    wheel_matrices: Vec<[i16; 9]>,
}
#[derive(Deserialize)]
struct Recording {
    rows: Vec<Row>,
    #[serde(default)]
    velocity_tracking: bool,
    yaw_coefficients: Option<[i32; 2]>,
}

fn vector(raw: [i32; 3]) -> Vec3 {
    Vec3::new(raw[0] as f32, raw[2] as f32, -raw[1] as f32) / 4096.0
}
fn matrix(raw: [i16; 9]) -> Mat3 {
    Mat3::from_cols_array(&raw.map(|v| v as f32 / 4096.0)).transpose()
}
fn tracking(raw: [i16; 5]) -> ChaseStabilization {
    let [yaw_turns, yaw_rate_turns_per_second, pitch_turns, pitch_rate_turns_per_second, previous_yaw_acceleration] =
        raw.map(|v| v as f32 / 4096.0);
    ChaseStabilization {
        yaw_turns,
        yaw_rate_turns_per_second,
        pitch_turns,
        pitch_rate_turns_per_second,
        previous_yaw_acceleration,
    }
}
fn check(name: &str) {
    let recording: Recording = serde_json::from_str(
        &std::fs::read_to_string(format!(
            "{}/tests/data/original/vehicle_presentation/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap();
    let layout: WheelLayout = load_ron(WHEEL_LAYOUT_ASSET).unwrap();
    assert_eq!(layout.wheels.len(), 4);
    assert!(layout.wheels.iter().all(|w| w.motion.is_some()), "re-export wheel bindings");
    let mut app = headless_app(game::tick_rate(game::REGION));
    if recording.velocity_tracking {
        use game::vehicle::{
            slip::{SLIP_TUNING, SlipTuning},
            visual_motion::{ChaseHeading, VISUAL_TUNING, VehicleVisualTuning},
        };
        let mut tuning: VehicleVisualTuning = load_ron(VISUAL_TUNING).unwrap();
        tuning.chase_heading = ChaseHeading::Velocity;
        let coefficients = recording.yaw_coefficients.unwrap();
        tuning.yaw_stiffness = coefficients[0] as f32 / 4096.0;
        tuning.yaw_damping = coefficients[1] as f32 / 4096.0;
        app.insert_resource(tuning).insert_resource(load_ron::<SlipTuning>(SLIP_TUNING).unwrap());
    }
    app.add_plugins(VehicleVisualMotionPlugin);
    let car = app
        .world_mut()
        .spawn((
            tracking(recording.rows[0].before.tracking),
            Transform::default(),
            game::vehicle::presentation::VehicleBody,
            game::race::camera::CameraSubject::default(),
        ))
        .id();
    for (tick, row) in recording.rows.iter().enumerate() {
        assert_eq!(row.run_frame, tick * 2);
        let r = &row.before;
        let axis = |off: usize| {
            Vec3::new(
                row.presented.body_basis[off] as f32,
                row.presented.body_basis[off + 2] as f32,
                -row.presented.body_basis[off + 1] as f32,
            ) / 4096.0
        };
        app.world_mut().entity_mut(car).insert((
            VehiclePose { position_m: vector(r.position), heading_turns: r.angles[2] as f32 / 4096.0 },
            BodyOrientation { pitch_turns: r.angles[0] as f32 / 4096.0, roll_turns: r.angles[1] as f32 / 4096.0 },
            VehicleVelocity { linear_mps: vector(r.velocity), ..default() },
            VehicleStepDuration(r.duration as f32 / 65536.0),
            RoadContactState {
                body_basis: VehicleBasis { forward: axis(0), lateral: axis(4), up: axis(8) },
                ..default()
            },
        ));
        app.run_tick(&[]);
        let actual = app.world().get::<ChaseStabilization>(car).unwrap();
        let values = [
            actual.yaw_turns,
            actual.yaw_rate_turns_per_second,
            actual.pitch_turns,
            actual.pitch_rate_turns_per_second,
            actual.previous_yaw_acceleration,
        ];
        assert_eq!(values.map(|v| (v * 4096.0).round() as i16), row.stabilized.tracking, "{name} tick {tick} tracking");
        let correction = app.world().get::<game::race::camera::CameraSubject>(car).unwrap();
        assert_eq!(
            [correction.yaw_correction_radians, correction.pitch_correction_radians]
                .map(|v| (v * 4096.0 / std::f32::consts::TAU).round() as i16),
            row.stabilized.correction
        );
        let body = app.world().get::<Transform>(car).unwrap();
        let original_position = Vec3::from_array(row.presented.body_position.map(|v| v as f32 / 65536.0));
        assert!(
            body.translation.distance(original_position) < 0.001,
            "{name} tick {tick}: {:?} vs {original_position:?}",
            body.translation
        );
        let original_rotation = matrix(row.presented.body_matrix);
        for (a, b) in Mat3::from_quat(body.rotation).to_cols_array().into_iter().zip(original_rotation.to_cols_array())
        {
            assert!((a - b).abs() < 0.002, "{name} tick {tick}: body matrix {a} vs {b}");
        }
        let steering = WheelSteering { angle_turns: row.presented.wheel_heading.map(|v| v as f32 / 4096.0) };
        let contact = ContactMotion { spin_turns: row.presented.wheel_spin.map(|v| v as f32 / 4096.0), ..default() };
        let suspension = Suspension { travel_m: row.presented.wheel_travel.map(|v| v as f32 / 4096.0), ..default() };
        for (i, wheel) in layout.wheels.iter().enumerate() {
            let mut wheel = wheel.clone();
            wheel.motion.as_mut().unwrap().camber_turns = row.presented.camber[i / 2] as f32 / 4096.0;
            let pose = wheel.animated_transform(&steering, &contact, &suspension);
            assert_eq!(
                (pose.translation * 4096.0).round().as_ivec3().to_array(),
                row.presented.wheel_positions[i].map(i32::from),
                "{name} tick {tick} wheel {i}"
            );
            if let Some(original) = row.wheel_matrices.get(i) {
                for (a, b) in
                    Mat3::from_quat(pose.rotation).to_cols_array().into_iter().zip(matrix(*original).to_cols_array())
                {
                    assert!((a - b).abs() < 0.002, "{name} tick {tick} wheel {i}: matrix {a} vs {b}");
                }
            }
        }
    }
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires locally exported assets and original presentation recordings")]
fn original_body_wheels_and_retained_chase_tracking() {
    for name in ["gas", "coast", "brake"] {
        check(name);
    }
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires locally exported assets and original presentation holdout")]
fn fresh_original_turning_presentation_holdout() {
    check("holdout");
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original velocity-tracking and asymmetric-camber probes")]
fn original_velocity_tracking_and_nonzero_camber() {
    check("holdout-velocity");
    check("holdout-camber");
}
