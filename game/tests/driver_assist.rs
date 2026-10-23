//! Original body-motion signals through native traction/brake preparation.
use bevy::prelude::*;
use engine::testing::{TickApp, headless_app};
use game::vehicle::{
    DigitalSteeringInput, SteeringKinematics, SteeringResponse, SteeringTuning, VehicleRulesPlugin, VehicleVelocity,
    body_dynamics::BodyDynamics,
    brake_assist::*,
    braking::*,
    driver_assist::*,
    drivetrain::{DrivetrainInput, EngineEngagement},
    engine_torque::EngineTorqueInput,
    forward_dynamics::ForwardDynamicsInput,
    longitudinal_slip::LongitudinalSlip,
    slip::{WheelLocalVelocities, WheelSlip, WheelSlipRulesPlugin},
    tyre_forces::TyreContactState,
    wheel_rotation::WheelRotationInput,
};
use std::collections::HashMap;
type Row = HashMap<String, i32>;
fn rows(name: &str) -> Vec<Row> {
    let text =
        std::fs::read_to_string(format!("{}/tests/data/original/driver_assist/{name}.csv", env!("CARGO_MANIFEST_DIR")))
            .unwrap();
    let mut lines = text.lines();
    let columns: Vec<_> = lines.next().unwrap().split(',').collect();
    lines
        .map(|line| {
            columns.iter().zip(line.split(',')).map(|(key, value)| (key.to_string(), value.parse().unwrap())).collect()
        })
        .collect()
}
fn check(name: &str, count: usize) {
    let original = rows(name);
    assert_eq!(original.len(), count);
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.add_plugins((
        VehicleRulesPlugin,
        WheelSlipRulesPlugin,
        DriverAssistRulesPlugin,
        BrakeAssistRulesPlugin,
        BrakingRulesPlugin,
    ));
    let standard = app.world().resource::<DriverAssistTuning>().clone();
    let steering: SteeringTuning = engine::prelude::load_ron("vehicle/steering.ron").unwrap();
    app.insert_resource(steering.clone());
    let car = app.world_mut().spawn_empty().id();
    let mut yaw_signals = 0;
    let mut lateral_signals = 0;
    let mut throttle_cut = 0;
    let mut throttle_partial = 0;
    let mut consumed = 0;
    let mut extensions = std::collections::HashSet::new();
    let mut normalized_full = false;
    let mut normalized_partial = false;
    let mut opposite_normalized_zero = false;
    let mut narrowed_normalized_zero = false;
    for r in original {
        let value = |phase: &str, key: &str| r[&format!("{phase}_{key}")] as f32;
        let before = |key: &str| value("before", key);
        let four =
            |phase: &str, key: &str, scale: f32| std::array::from_fn(|i| value(phase, &format!("{key}{i}")) / scale);
        let mut tuning = standard.clone();
        tuning.wheelbase_m = before("wheelbase") / 4096.0;
        tuning.traction_strength = before("gain") / 4096.0;
        tuning.feedback_fade_per_turn = before("fade") / 4096.0;
        tuning.steering_response_suppression = before("speed_gain") / 4096.0;
        tuning.traction_slip_target = [before("target0"), before("target1")].map(|v| v / 4096.0);
        tuning.brake_correction_gain = before("correction_gain") / 4096.0;
        tuning.brake_correction_threshold = before("correction_threshold") / 4096.0;
        let input = DriverAssistInput {
            forward_mps: before("forward") / 4096.0,
            lateral_mps: before("lateral") / 4096.0,
            yaw_turns_per_second: before("yaw") / 524288.0,
            steering_turns: before("steering") / 4096.0,
            lateral_feedback_turns: before("feedback") / 4096.0,
            steering_extension_response: before("speed_response") / 4096.0,
            effective_throttle: before("throttle") / 4096.0,
            available_torque: before("available"),
            engine_coupled: before("engagement") == 1.0,
            reverse_gear: before("gear") == 0.0,
            driven_axles: match r["before_layout"] {
                0 | 5 => [false, true],
                1 => [true, false],
                _ => [true, true],
            },
            wheel_load: four("before", "load", 1.0),
            longitudinal_slip: four("before", "slip", 4096.0),
        };
        // Source 1 probes skip the assistance consumer, but still run the
        // original shared signal producer. Only that producer is compared.
        if r["brake_called"] == 0 {
            tuning.feedback_fade_per_turn = 0.0;
        }
        let pure = tuning.evaluate(input);
        let mut initial_steering = SteeringResponse { angle_turns: input.steering_turns, ..default() };
        if r["control_called"] != 0 {
            let kinematics = SteeringKinematics {
                forward_velocity_mps: value("control_before", "forward") / 4096.0,
                front_slip_turns: [value("control_before", "direction0"), value("control_before", "direction1")]
                    .map(|v| v / 4096.0),
            };
            let extension = steering.traction_extension_turns(kinematics);
            extensions.insert(extension.signum() as i32 * i32::from(extension != 0.0));
            assert_eq!(extension * 4096.0, value("control_after", "speed_response"), "original signed extension");
            assert_eq!(
                steering.limits(kinematics).map(|v| v * 4096.0),
                [value("control_after", "negative_limit"), value("control_after", "positive_limit")]
            );
            assert_eq!(
                steering.traction_response(input.steering_turns, kinematics) * 4096.0,
                before("speed_response"),
                "original consumed extension response"
            );
            let normalized = before("speed_response");
            normalized_full |= normalized == 4096.0;
            normalized_partial |= normalized > 0.0 && normalized < 4096.0;
            if extension != 0.0 {
                let ratio = input.steering_turns / extension;
                opposite_normalized_zero |= ratio < 0.0 && normalized == 0.0;
                narrowed_normalized_zero |= ratio >= 8.0 && normalized == 0.0;
            }
            assert_eq!(value("control_ready", "speed_response"), before("speed_response"));
            initial_steering = SteeringResponse {
                angle_turns: value("control_before", "steering") / 4096.0,
                rate_turns_per_second: value("control_before", "steering_rate") / 4096.0,
                ..default()
            };
            app.world_mut().entity_mut(car).insert((
                DigitalSteeringInput { direction: r["control_direction"] as f32 },
                SteeringKinematics { front_slip_turns: [f32::NAN; 2], ..kinematics },
                WheelSlip::default(),
                WheelLocalVelocities {
                    forward_mps: four("control_before", "local_forward", 4096.0),
                    lateral_mps: four("control_before", "local_lateral", 4096.0),
                },
            ));
        } else {
            app.world_mut()
                .entity_mut(car)
                .remove::<(SteeringKinematics, DigitalSteeringInput, WheelSlip, WheelLocalVelocities)>();
        }
        app.insert_resource(BrakeAssistTuning {
            slip_target: [before("brake_target0"), before("brake_target1")].map(|v| v / 4096.0),
            slip_feedback: [before("slip_gain0"), before("slip_gain1")].map(|v| v / 4096.0),
            grip_feedback: [before("grip_gain0"), before("grip_gain1")].map(|v| v / 4096.0),
            yaw_target_strength: before("yaw_strength") / 100.0,
        });
        app.insert_resource(BrakingTuning {
            front_capacity: before("brake_capacity0"),
            rear_capacity: before("brake_capacity1"),
            handbrake_capacity: before("handbrake_capacity"),
        });
        let sentinel = DriverAssistInput {
            forward_mps: f32::NAN,
            lateral_mps: f32::NAN,
            yaw_turns_per_second: f32::NAN,
            steering_turns: f32::NAN,
            lateral_feedback_turns: f32::NAN,
            effective_throttle: f32::NAN,
            steering_extension_response: if r["control_called"] != 0 {
                f32::NAN
            } else {
                input.steering_extension_response
            },
            wheel_load: [f32::NAN; 4],
            longitudinal_slip: [f32::NAN; 4],
            ..input
        };
        app.world_mut().entity_mut(car).insert((
            (
                DriverAssistSetup(tuning),
                sentinel,
                DriverAssist::default(),
                TyreContactState { body_forward_mps: input.forward_mps, ..default() },
                BodyDynamics {
                    local_lateral_mps: input.lateral_mps,
                    lateral_feedback_turns: input.lateral_feedback_turns,
                    ..default()
                },
                VehicleVelocity { yaw_turns_per_second: input.yaw_turns_per_second, ..default() },
                initial_steering,
                ForwardDynamicsInput { wheel_load: input.wheel_load, ..default() },
                LongitudinalSlip { ratio: input.longitudinal_slip, ..default() },
            ),
            (
                EngineTorqueInput { effective_throttle: input.effective_throttle, ..default() },
                DrivetrainInput {
                    engagement: if input.engine_coupled {
                        EngineEngagement::Coupled
                    } else {
                        EngineEngagement::Disconnected
                    },
                    gear: usize::from(!input.reverse_gear),
                    ..default()
                },
                BrakeAssistInput {
                    speed_response: four("before", "response", 4096.0),
                    grip_response: four("before", "grip", 4096.0),
                    longitudinal_slip: input.longitudinal_slip,
                    yaw_shortfall: f32::NAN,
                    lateral_correction: f32::NAN,
                    correction_gain: f32::NAN,
                    correction_threshold: f32::NAN,
                    ..default()
                },
                WheelBrakeInput { travel: four("before", "brake", 4096.0), handbrake: before("handbrake") / 4096.0 },
                WheelBrakeDemand::default(),
                WheelRotationInput { yaw_correction: f32::NAN, ..default() },
            ),
        ));
        app.run_tick(&[]);
        let steering = app.world().get::<SteeringResponse>(car).unwrap();
        assert_eq!(steering.angle_turns, input.steering_turns, "native control steering");
        if r["control_called"] != 0 {
            assert_eq!(steering.rate_turns_per_second * 4096.0, before("steering_rate"), "native steering rate");
            let slip = app.world().get::<WheelSlip>(car).unwrap();
            assert_eq!(
                slip.angle_turns.map(|v| v * 4096.0),
                four("control_before", "direction", 1.0),
                "native precontrol slip"
            );
        }
        let state = *app.world().get::<DriverAssist>(car).unwrap();
        let label = format!("{name} case {}", r["case"]);
        for (key, actual) in [("yaw_shortfall", state.yaw_shortfall), ("lateral_correction", state.lateral_correction)]
        {
            assert_eq!(actual * 4096.0, value("signals", key), "{label}: {key}");
        }
        assert_eq!(state.yaw_shortfall, pure.yaw_shortfall);
        assert_eq!(state.lateral_correction, pure.lateral_correction);
        assert_eq!(state.effective_throttle, pure.effective_throttle);
        let rotation = app.world().get::<WheelRotationInput>(car).unwrap();
        assert_eq!(rotation.yaw_correction, state.yaw_shortfall, "{label}: wheel torque bridge");
        assert_eq!(rotation.steering_turns, input.steering_turns);
        let assist = app.world().get::<BrakeAssistInput>(car).unwrap();
        assert_eq!(assist.yaw_shortfall, state.yaw_shortfall);
        assert_eq!(assist.lateral_correction, state.lateral_correction);
        assert_eq!(assist.positive_yaw, input.yaw_turns_per_second > 0.0);
        if r["brake_called"] != 0 {
            consumed += 1;
            assert_eq!(
                state.effective_throttle * 4096.0,
                value("brake_before", "throttle"),
                "{label}: effective throttle"
            );
            assert_eq!(app.world().get::<EngineTorqueInput>(car).unwrap().effective_throttle, state.effective_throttle);
            let brakes = app.world().get::<WheelBrakeInput>(car).unwrap();
            let demand = app.world().get::<WheelBrakeDemand>(car).unwrap();
            for i in 0..4 {
                assert_eq!(brakes.travel[i] * 4096.0, value("brake_after", &format!("brake{i}")), "{label}: brake {i}");
                assert_eq!(demand.total[i], value("allocated", &format!("demand{i}")), "{label}: demand {i}");
            }
        }
        yaw_signals += usize::from(state.yaw_shortfall != 0.0);
        lateral_signals += usize::from(state.lateral_correction != 0.0);
        throttle_cut += usize::from(state.throttle_scale == 0.0);
        throttle_partial += usize::from(state.throttle_scale > 0.0 && state.throttle_scale < 1.0);
    }
    assert!(yaw_signals > 0 && lateral_signals > 0 && consumed > 40);
    assert!(throttle_cut > 0 && throttle_partial > 0);
    assert_eq!(extensions, std::collections::HashSet::from([-1, 0, 1]));
    assert!(normalized_full && normalized_partial && opposite_normalized_zero && narrowed_normalized_zero);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original assistance fixtures; see docs/assets.md")]
fn original_stability_signals_traction_layouts_and_native_brake_consumers() {
    check("boundaries", 197);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original assistance fixtures; see docs/assets.md")]
fn fresh_original_driver_assistance_holdout() {
    check("holdout", 114);
}

// Deliberately authored tuning: these tests need no original assets or fixture.
fn handmade_tuning() -> DriverAssistTuning {
    DriverAssistTuning {
        wheelbase_m: 2.0,
        radians_per_turn: std::f32::consts::TAU,
        angle_threshold_ratios: vec![],
        traction_strength: 2.0,
        feedback_fade_per_turn: 1.0,
        steering_response_suppression: 0.0,
        traction_slip_target: [0.25; 2],
        brake_correction_gain: 0.5,
        brake_correction_threshold: 0.125,
    }
}
fn handmade_input() -> DriverAssistInput {
    DriverAssistInput {
        forward_mps: 4.0,
        lateral_mps: -8.0,
        steering_turns: 0.125,
        engine_coupled: true,
        available_torque: 80.0,
        effective_throttle: 0.75,
        driven_axles: [false, true],
        wheel_load: [1.0; 4],
        ..default()
    }
}
#[test]
fn handmade_assistance_runs_alone_and_supports_per_car_setup() {
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(handmade_tuning()).add_plugins(DriverAssistRulesPlugin);
    let car = app.world_mut().spawn((handmade_input(), DriverAssist::default())).id();
    let mut other_tuning = handmade_tuning();
    other_tuning.feedback_fade_per_turn = 0.0;
    let other = app
        .world_mut()
        .spawn((
            DriverAssistSetup(other_tuning),
            handmade_input(),
            DriverAssist::default(),
            BrakeAssistInput::default(),
            WheelRotationInput::default(),
        ))
        .id();
    app.run_tick(&[]);
    let state = app.world().get::<DriverAssist>(car).unwrap();
    assert_eq!(state.yaw_shortfall, 1.0);
    assert_eq!(state.lateral_correction, 0.25);
    assert_eq!(state.throttle_scale, 0.5);
    assert_eq!(state.effective_throttle, 0.375);
    let state = app.world().get::<DriverAssist>(other).unwrap();
    assert_eq!(state.effective_throttle, 0.75);
    assert_eq!(app.world().get::<WheelRotationInput>(other).unwrap().yaw_correction, 1.0);
    let brakes = app.world().get::<BrakeAssistInput>(other).unwrap();
    assert_eq!(brakes.yaw_shortfall, 1.0);
    assert_eq!(brakes.lateral_correction, 0.25);
    assert_eq!(brakes.correction_gain, 0.5);
    assert_eq!(brakes.correction_threshold, 0.125);
}

#[test]
fn pedal_throttle_is_copied_before_assistance_and_torque_without_drivetrain() {
    use game::vehicle::{PedalTravel, engine_torque::*};
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(handmade_tuning());
    app.insert_resource(EngineTorqueTuning {
        torque_curve: vec![EngineTorqueKnot { speed_rev_s: 0.0, torque: 80.0 }],
        drag_per_rev_s: 0.0,
        redline_rpm: 6000.0,
        idle_rpm: 600.0,
        fuel_cut_hysteresis_rpm: 500.0,
    });
    // Engine registered first, with no transmission/brake/wheel plugins to
    // provide indirect ordering. The assistance plugin owns the dependency.
    app.add_plugins((EngineTorqueRulesPlugin, DriverAssistRulesPlugin));
    let car = app
        .world_mut()
        .spawn((
            PedalThrottle,
            PedalTravel { throttle: 0.75, ..default() },
            handmade_input(),
            DriverAssist::default(),
            EngineTorqueInput { effective_throttle: f32::NAN, ..default() },
            EngineFuelCut::default(),
            EngineTorque::default(),
        ))
        .id();
    let external = app
        .world_mut()
        .spawn((
            PedalTravel { throttle: 0.75, ..default() },
            handmade_input(),
            DriverAssist::default(),
            EngineTorqueInput { effective_throttle: 0.5, ..default() },
            EngineFuelCut::default(),
            EngineTorque::default(),
        ))
        .id();
    app.run_tick(&[]);
    assert_eq!(app.world().get::<EngineTorque>(car).unwrap().net, 30.0);
    assert_eq!(app.world().get::<EngineTorque>(external).unwrap().net, 20.0);
    // The opt-in copy restores this tick's request, rather than reducing
    // last tick's already-assisted throttle for a second time.
    app.world_mut().get_mut::<PedalTravel>(car).unwrap().throttle = 0.5;
    app.run_tick(&[]);
    assert_eq!(app.world().get::<EngineTorque>(car).unwrap().net, 20.0);
}

#[test]
fn signed_steering_extension_feeds_traction_with_optional_kinematics() {
    use game::vehicle::SteeringKnot;
    let mut tuning = handmade_tuning();
    tuning.steering_response_suppression = 1.0;
    let steering = SteeringTuning {
        stiffness: 1.0,
        damping: 1.0,
        centering_stiffness: 1.0,
        rate_limit_turns_per_second: 1.0,
        extension_threshold_turns: 1.0 / 64.0,
        extension_min_speed_mps: 2.0,
        extended_limit_turns: 1.0 / 8.0,
        inner_wheel_ratio: 1.0,
        outer_wheel_ratio: 1.0,
        toe_turns: [0.0; 2],
        speed_curve: vec![SteeringKnot { speed_kph: 0.0, angle_turns: 1.0 / 32.0 }],
    };
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(tuning).insert_resource(steering).add_plugins(DriverAssistRulesPlugin);
    let mut cars = Vec::new();
    for (slip, angle) in [(1.0 / 16.0, 1.0 / 32.0), (-1.0 / 16.0, -1.0 / 32.0), (1.0 / 16.0, -1.0 / 32.0)] {
        for reverse in [false, true] {
            let input = DriverAssistInput {
                steering_extension_response: f32::NAN,
                reverse_gear: reverse,
                steering_turns: angle,
                ..handmade_input()
            };
            let car = app
                .world_mut()
                .spawn((
                    input,
                    DriverAssist::default(),
                    SteeringKinematics { forward_velocity_mps: 4.0, front_slip_turns: [slip; 2] },
                ))
                .id();
            cars.push(car);
        }
    }
    let external = app
        .world_mut()
        .spawn((DriverAssistInput { steering_extension_response: 1.0, ..handmade_input() }, DriverAssist::default()))
        .id();
    app.run_tick(&[]);
    for (car, expected) in cars.into_iter().zip([0.5625, 0.375, 0.5625, 0.375, 0.375, 0.375]) {
        assert_eq!(app.world().get::<DriverAssist>(car).unwrap().effective_throttle, expected);
    }
    assert_eq!(app.world().get::<DriverAssist>(external).unwrap().effective_throttle, 0.75);
}
