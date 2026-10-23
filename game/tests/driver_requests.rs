//! Requests observed at the original ordinary driver producer, before the
//! already-ported transmission preparation can consume or clear them.
use bevy::prelude::*;
use engine::testing::{TickApp, headless_app};
use game::vehicle::{
    DigitalGearInput, PedalTravel,
    body_dynamics::BodyDynamicsInput,
    drivetrain::{Axle, DrivetrainInput, DrivetrainSetup, DrivetrainTuning, EngineEngagement},
    engine_torque::{EngineTorqueInput, EngineTorqueTuning},
    transmission::{TransmissionInput, TransmissionRulesPlugin, TransmissionState, TransmissionSystems},
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Before {
    reset: u8,
    counter: u16,
    mode: u8,
    rear_reference: u8,
    handbrake: i16,
    brake: i16,
    engagement: u8,
    rpm: i16,
    idle: u16,
}
#[derive(Deserialize)]
struct Probe {
    name: String,
    before: Before,
    requested: i32,
    starting_counter: u16,
    consumed: Vec<String>,
}
#[derive(Deserialize)]
struct Recording {
    cases: Vec<Probe>,
}
#[derive(Component, Default)]
struct Observed(Option<(bool, u16)>);
fn observe(mut cars: Query<(&TransmissionState, &mut Observed)>) {
    for (state, mut observation) in &mut cars {
        observation.0 = Some((state.engagement_requested, state.first_gear_ticks));
    }
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires local original driver-request probes")]
fn original_engagement_gates_and_countdown_boundaries() {
    let recording: Recording = serde_json::from_str(
        &std::fs::read_to_string(format!(
            "{}/tests/data/original/driver_requests/probes.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(recording.cases.len(), 14);
    let mut consumed = std::collections::HashSet::new();
    for probe in recording.cases {
        consumed.extend(probe.consumed);
        let mut app = headless_app(game::tick_rate(game::REGION));
        app.add_plugins(TransmissionRulesPlugin);
        app.add_systems(
            FixedUpdate,
            observe.after(TransmissionSystems::DriverEngagement).before(TransmissionSystems::Prepare),
        );
        let b = probe.before;
        assert_eq!(app.world().resource::<EngineTorqueTuning>().idle_rpm, b.idle as f32);
        let mut tuning = app.world().resource::<DrivetrainTuning>().clone();
        tuning.reference_axle = if b.rear_reference == 0 { Axle::Front } else { Axle::Rear };
        let car = app
            .world_mut()
            .spawn((
                DigitalGearInput::default(),
                Observed::default(),
                PedalTravel { brake: b.brake as f32 / 4096.0, handbrake: b.handbrake as f32 / 4096.0, ..default() },
                TransmissionState { first_gear_ticks: probe.starting_counter, ..default() },
                TransmissionInput::default(),
                DrivetrainInput {
                    engagement: match b.engagement {
                        0 => EngineEngagement::Disconnected,
                        1 => EngineEngagement::Coupled,
                        2 => EngineEngagement::Synchronizing,
                        3 => EngineEngagement::FixedClutch,
                        4 => EngineEngagement::Slipping,
                        _ => panic!("unknown engagement"),
                    },
                    ..default()
                },
                DrivetrainSetup(tuning),
                EngineTorqueInput { integer_rpm: b.rpm as f32, ..default() },
                BodyDynamicsInput { reset_motion: b.reset != 0 || b.mode == 7, ..default() },
            ))
            .id();
        app.run_tick(&[]);
        assert_eq!(
            app.world().get::<Observed>(car).unwrap().0,
            Some((probe.requested != 0, b.counter)),
            "{}",
            probe.name
        );
    }
    for field in ["reset", "counter", "rear_reference", "handbrake", "brake", "engagement", "rpm", "idle"] {
        assert!(consumed.contains(field), "missing original consumption of {field}");
    }
}
