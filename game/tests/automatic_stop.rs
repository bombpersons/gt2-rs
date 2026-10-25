use bevy::prelude::*;
use engine::{
    prelude::load_ron,
    testing::{TickApp, headless_app},
};
use game::vehicle::{
    automatic_stop::*,
    body_dynamics::*,
    braking::WheelBrakeInput,
    drivetrain::{DrivetrainInput, EngineEngagement},
    engine_torque::EngineTorqueInput,
    transmission::{TransmissionInput, TransmissionState},
    *,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct View {
    mode: u8,
    angle: i16,
    rate: i16,
    pedals: [i16; 3],
    brakes: [i16; 4],
    coupling: bool,
}
#[derive(Deserialize)]
struct Row {
    before: View,
    after: View,
    linear: [f32; 3],
    angular: [f32; 3],
    duration: f32,
    rpm: f32,
    engaged: u8,
}
#[derive(Deserialize)]
struct Recording {
    rows: Vec<Row>,
}

#[test]
fn authored_per_car_stopping_parks_and_releases_control_without_a_session() {
    let tuning = AutomaticStopTuning {
        steering_rate_turns_per_second: 1.0,
        settled_linear_mps: 0.5,
        settled_angular_turns_per_second: 0.125,
        reengage_above_rpm: 2000.0,
        disengage_below_rpm: 500.0,
    };
    let mut app = headless_app(game::tick_rate(game::REGION));
    // A deliberately different default proves that the car owns its setup.
    app.insert_resource(AutomaticStopTuning { settled_linear_mps: 10.0, ..tuning.clone() })
        .add_plugins(AutomaticStopRulesPlugin);
    let car = app
        .world_mut()
        .spawn((
            AutomaticStop::default(),
            AutomaticStopSetup(tuning),
            PedalTravel { throttle: 1.0, handbrake: 1.0, ..default() },
            SteeringResponse { angle_turns: 0.25, rate_turns_per_second: -0.5, ..default() },
            VehicleStepDuration(0.125),
            VehicleVelocity { linear_mps: Vec3::X, ..default() },
            BodyDynamicsInput::default(),
            game::vehicle::suspension::SuspensionInput::default(),
            TransmissionInput::default(),
            DigitalGearInput { reverse: true, shift_direction: 1 },
            WheelBrakeInput::default(),
        ))
        .id();
    app.run_tick(&[]);
    assert_eq!(app.world().get::<AutomaticStop>(car).unwrap().phase, AutomaticStopPhase::Braking);
    let pedals = app.world().get::<PedalTravel>(car).unwrap();
    assert_eq!([pedals.throttle, pedals.brake, pedals.handbrake], [0.0, 1.0, 0.0]);
    assert_eq!(app.world().get::<SteeringResponse>(car).unwrap().angle_turns, 0.125);
    assert_eq!(app.world().get::<WheelBrakeInput>(car).unwrap().travel, [1.0; 4]);
    assert!(app.world().get::<ExternalDriver>(car).is_some());
    assert!(!app.world().get::<DigitalGearInput>(car).unwrap().reverse);
    assert_eq!(app.world().get::<DigitalGearInput>(car).unwrap().shift_direction, 0);

    app.world_mut().get_mut::<VehicleVelocity>(car).unwrap().linear_mps = Vec3::ZERO;
    app.run_tick(&[]);
    assert_eq!(app.world().get::<AutomaticStop>(car).unwrap().phase, AutomaticStopPhase::Parked);
    assert_eq!(app.world().get::<PedalTravel>(car).unwrap().brake, 0.0);
    assert_eq!(app.world().get::<SteeringResponse>(car).unwrap().rate_turns_per_second, -0.5);
    assert!(app.world().get::<BodyDynamicsInput>(car).unwrap().reset_motion);
    assert!(app.world().get::<game::vehicle::suspension::SuspensionInput>(car).unwrap().disabled);
    assert!(app.world().get::<TransmissionInput>(car).unwrap().force_automatic);

    app.world_mut().entity_mut(car).remove::<AutomaticStop>();
    app.run_tick(&[]);
    assert!(app.world().get::<ExternalDriver>(car).is_none());
    assert!(!app.world().get::<BodyDynamicsInput>(car).unwrap().reset_motion);
    assert!(!app.world().get::<game::vehicle::suspension::SuspensionInput>(car).unwrap().disabled);
    assert!(!app.world().get::<TransmissionInput>(car).unwrap().force_automatic);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original automatic stopping boundary probes")]
fn original_speed_rotation_slew_and_coupling_boundaries() {
    let tuning: AutomaticStopTuning = load_ron(AUTOMATIC_STOP_TUNING).unwrap();
    let data: Recording = serde_json::from_str(
        &std::fs::read_to_string(format!(
            "{}/tests/data/original/automatic_stop/boundaries.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(data.rows.len(), 26);
    for (i, r) in data.rows.into_iter().enumerate() {
        let mut app = headless_app(game::tick_rate(game::REGION));
        app.insert_resource(tuning.clone()).add_plugins(AutomaticStopRulesPlugin);
        let car = app
            .world_mut()
            .spawn((
                AutomaticStop {
                    phase: if r.before.mode == 7 { AutomaticStopPhase::Parked } else { AutomaticStopPhase::Braking },
                    ..default()
                },
                SteeringResponse {
                    angle_turns: f32::from(r.before.angle) / 4096.0,
                    rate_turns_per_second: f32::from(r.before.rate) / 4096.0,
                    ..default()
                },
                PedalTravel {
                    throttle: f32::from(r.before.pedals[0]) / 4096.0,
                    brake: f32::from(r.before.pedals[1]) / 4096.0,
                    handbrake: f32::from(r.before.pedals[2]) / 4096.0,
                },
                VehicleStepDuration(r.duration),
                VehicleVelocity { linear_mps: Vec3::from_array(r.linear), yaw_turns_per_second: r.angular[1] },
                BodyDynamics { tilt_turns_per_second: Vec2::new(r.angular[0], r.angular[2]), ..default() },
                WheelBrakeInput::default(),
                TransmissionInput::default(),
                TransmissionState { engagement_requested: r.before.coupling, ..default() },
                DrivetrainInput {
                    engagement: match r.engaged {
                        0 => EngineEngagement::Disconnected,
                        1 => EngineEngagement::Coupled,
                        _ => panic!("unsupported probe clutch"),
                    },
                    ..default()
                },
                EngineTorqueInput { integer_rpm: r.rpm, ..default() },
            ))
            .id();
        app.run_tick(&[]);
        assert!(app.world().get::<ExternalDriver>(car).is_some());
        let stop = app.world().get::<AutomaticStop>(car).unwrap();
        assert_eq!(stop.phase == AutomaticStopPhase::Parked, r.after.mode == 7, "case {i} phase");
        let steering = app.world().get::<SteeringResponse>(car).unwrap();
        assert_eq!(steering.angle_turns * 4096.0, f32::from(r.after.angle), "case {i} angle");
        assert_eq!(steering.rate_turns_per_second * 4096.0, f32::from(r.after.rate), "case {i} spring history");
        let p = app.world().get::<PedalTravel>(car).unwrap();
        assert_eq!(
            [p.throttle, p.brake, p.handbrake].map(|v| v * 4096.0),
            r.after.pedals.map(f32::from),
            "case {i} pedals"
        );
        assert_eq!(
            app.world().get::<WheelBrakeInput>(car).unwrap().travel.map(|v| v * 4096.0),
            r.after.brakes.map(f32::from),
            "case {i} wheel brakes"
        );
        assert_eq!(
            app.world().get::<TransmissionState>(car).unwrap().engagement_requested,
            r.after.coupling,
            "case {i} clutch"
        );
    }
}
