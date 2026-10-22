//! Original lateral/yaw/world stages with native slip, tyre and forward rules.
use bevy::prelude::*;
use engine::testing::{TickApp, headless_app};
use game::vehicle::{
    VehicleStepDuration, VehicleSystems, VehicleVelocity, WheelSteering,
    body_dynamics::*,
    forward_dynamics::*,
    longitudinal_slip::{LongitudinalSlip, LongitudinalSlipRulesPlugin, WheelSurfaceMotion},
    slip::{WheelLocalVelocities, WheelSlip, WheelSlipRulesPlugin},
    tyre_forces::{TyreContactState, TyreForces, TyreForcesRulesPlugin},
};
use std::collections::{HashMap, HashSet};
#[path = "support/body_dynamics.rs"]
mod body_support;
use body_support::{compare, original_inputs};

type Row = HashMap<String, i32>;
fn rows(name: &str) -> Vec<Row> {
    let text =
        std::fs::read_to_string(format!("{}/tests/data/original/body_dynamics/{name}.csv", env!("CARGO_MANIFEST_DIR")))
            .unwrap();
    let mut lines = text.lines();
    let columns: Vec<_> = lines.next().unwrap().split(',').collect();
    lines
        .map(|line| columns.iter().zip(line.split(',')).map(|(k, v)| (k.to_string(), v.parse().unwrap())).collect())
        .collect()
}

#[derive(Component, Default)]
struct LateralPhase(BodyDynamics, VehicleVelocity, ForwardDynamics);
fn observe(mut cars: Query<(&BodyDynamics, &VehicleVelocity, &ForwardDynamics, &mut LateralPhase)>) {
    for (&body, &velocity, &forward, mut phase) in &mut cars {
        *phase = LateralPhase(body, velocity, forward);
    }
}

fn check(name: &str, count: usize) {
    let original = rows(name);
    assert_eq!(original.len(), count * 4);
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.add_plugins((
        BodyDynamicsRulesPlugin,
        ForwardDynamicsRulesPlugin,
        TyreForcesRulesPlugin,
        LongitudinalSlipRulesPlugin,
        WheelSlipRulesPlugin,
    ));
    app.add_systems(
        FixedUpdate,
        observe
            .in_set(VehicleSystems::Forces)
            .after(BodyDynamicsSystems::Accumulate)
            .before(BodyDynamicsSystems::ApplyWorld),
    );
    let car = app
        .world_mut()
        .spawn((
            BodyDynamicsInput::default(),
            BodyDynamics::default(),
            VehicleVelocity::default(),
            LateralPhase::default(),
            ForwardDynamicsInput::default(),
            ForwardDynamics::default(),
            TyreContactState::default(),
            TyreForces::default(),
            WheelSurfaceMotion::default(),
            WheelLocalVelocities::default(),
            WheelSteering::default(),
            WheelSlip::default(),
            LongitudinalSlip::default(),
            VehicleStepDuration::default(),
        ))
        .id();
    let mut stops = 0;
    let mut clamp_signs = HashSet::new();
    let mut reset_variants = HashSet::new();
    let mut activation_wheels = HashSet::new();
    let mut brake_wheels = HashSet::new();
    let mut stop_speeds = HashSet::new();
    let mut external_after_stop = false;
    let mut narrowed_feedback = false;
    for group in original.as_chunks::<4>().0 {
        let r = &group[0];
        let before = |key: &str| r[&format!("bd_before_{key}")] as f32;
        let field = |key: &str| std::array::from_fn(|i| group[i][key] as f32 / 4096.0);
        let (input, velocity, body) = original_inputs(r);
        let mut setup = app.world().resource::<BodyDynamicsTuning>().clone();
        setup.lateral_feedback_response = before("feedback_response") / 4096.0;
        app.world_mut().entity_mut(car).insert((
            input,
            velocity,
            body,
            BodyDynamicsSetup(setup),
            ForwardDynamicsInput {
                wheel_load: std::array::from_fn(|i| r[&format!("fd_load{i}")] as f32),
                surface: std::array::from_fn(|i| r[&format!("fd_surface{i}")] as usize),
                aerodynamic_force: r["fd_aerodynamic_force"] as f32,
            },
            TyreContactState {
                grip_capacity: std::array::from_fn(|i| group[i]["grip"] as f32),
                body_forward_mps: r["body_forward"] as f32 / 4096.0,
            },
            WheelSurfaceMotion { tread_speed_mps: field("tread"), speed_response: field("speed_response") },
            WheelLocalVelocities { forward_mps: field("forward"), lateral_mps: field("lateral") },
            WheelSteering { angle_turns: field("steering") },
            VehicleStepDuration(before("duration") / 65536.0),
        ));
        app.run_tick(&[]);
        let label = format!("{name} case {}", r["case"]);
        let tyres = app.world().get::<TyreForces>(car).unwrap();
        let slip = app.world().get::<WheelSlip>(car).unwrap();
        for (i, original_wheel) in group.iter().enumerate() {
            for (key, actual) in [
                ("wheel_force", tyres.wheel_reference_force[i]),
                ("ground_force", tyres.ground_reference_force[i]),
                ("lateral_force", tyres.lateral_force[i]),
                ("activation", slip.activation[i] * 4096.0),
            ] {
                assert_eq!(actual, original_wheel[key] as f32, "{label} wheel {i}: {key}");
            }
            if slip.activation[i] != 0.0 {
                activation_wheels.insert(i);
            }
            if input.braked_wheels[i] {
                brake_wheels.insert(i);
            }
        }
        // The accumulation stage leaves the native forward result unchanged.
        let phase = app.world().get::<LateralPhase>(car).unwrap();
        compare(r, "lateral", phase.0, phase.1, phase.2, &label);
        let body = *app.world().get::<BodyDynamics>(car).unwrap();
        let velocity = *app.world().get::<VehicleVelocity>(car).unwrap();
        let forward = *app.world().get::<ForwardDynamics>(car).unwrap();
        compare(r, "after", body, velocity, forward, &label);
        assert_eq!(body.lateral_force, r["bd_total_lateral_force"] as f32, "{label}: total side force");
        assert_eq!(body.yaw_torque, r["bd_total_yaw_torque"] as f32, "{label}: total yaw torque");
        if phase.1.yaw_turns_per_second.abs() == 1.0 {
            clamp_signs.insert(phase.1.yaw_turns_per_second.signum() as i32);
        }
        reset_variants.insert((input.reset_motion, input.first_gear_override, input.bypass_tyre_acceleration));
        if body.brake_stopped {
            stops += 1;
            stop_speeds.insert(before("forward_speed") as i32);
            external_after_stop |= input.external_world_force != Vec2::ZERO && velocity.linear_mps != Vec3::ZERO;
        }
        let response = app.world().get::<BodyDynamicsSetup>(car).unwrap().0.lateral_feedback_response;
        narrowed_feedback |= (phase.0.lateral_acceleration_mps2 * 4096.0 * response).abs() >= 32768.0;
    }
    if name == "boundaries" {
        assert_eq!(clamp_signs.len(), 2, "both signed yaw clamps must execute");
        assert_eq!(reset_variants.len(), 8, "reset/first-gear/bypass combinations missing");
        assert_eq!(activation_wheels.len(), 4);
        assert_eq!(brake_wheels.len(), 4);
        assert_eq!(stop_speeds, HashSet::from([-1137, -1, 0, 1, 1137]));
        assert!(stops >= 20);
    }
    if name == "holdout" {
        assert!(external_after_stop, "brake-stop must still apply external world forces");
        assert!(narrowed_feedback, "signed-halfword feedback narrowing must execute");
    }
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original body fixtures; see docs/assets.md")]
fn original_force_projection_yaw_clamps_reset_and_brake_stop_boundaries() {
    check("boundaries", 397);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original body fixtures; see docs/assets.md")]
fn fresh_original_signed_and_alternating_control_holdout() {
    check("holdout", 64);
}

#[test]
fn handmade_body_rules_run_alone_with_per_car_tuning() {
    let tuning = BodyDynamicsTuning {
        wheel_levers_m: [[-0.5, 1.0], [0.5, 1.0], [-0.5, -1.0], [0.5, -1.0]],
        yaw_response_turns_per_torque_second2: 1.0 / 65536.0,
        inverse_mass: 1.0,
        lateral_feedback_response: 0.5,
        maximum_yaw_turns_per_second: 1.0,
        brake_stop_speed_mps: 0.25,
    };
    let mut alternate = tuning.clone();
    alternate.inverse_mass = 0.5;
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(tuning).add_plugins(BodyDynamicsRulesPlugin);
    let spawn = |world: &mut World| {
        world
            .spawn((
                BodyDynamicsInput {
                    forward_axis: Vec2::X,
                    lateral_axis: Vec2::Y,
                    external_world_force: Vec2::new(4096.0, 8192.0),
                    lateral_resistance: 4096.0,
                    ..default()
                },
                BodyDynamics::default(),
                VehicleVelocity::default(),
                ForwardDynamics { forward_mps: 1.0, acceleration_mps2: 1.0, ..default() },
                TyreForces {
                    lateral_force: [4096.0, 0.0, 0.0, 0.0],
                    wheel_reference_force: [0.0, 8192.0, 0.0, 0.0],
                    ..default()
                },
                WheelSteering::default(),
                VehicleStepDuration(0.5),
                game::vehicle::drivetrain::DrivetrainInput::default(),
            ))
            .id()
    };
    let a = spawn(app.world_mut());
    let b = spawn(app.world_mut());
    app.world_mut().entity_mut(b).insert(BodyDynamicsSetup(alternate));
    app.run_tick(&[]);
    assert_eq!(app.world().get::<VehicleVelocity>(a).unwrap().linear_mps, Vec3::new(1.0, 0.0, 0.0));
    assert_eq!(app.world().get::<VehicleVelocity>(b).unwrap().linear_mps, Vec3::new(0.75, 0.0, 0.0));
    assert_eq!(app.world().get::<VehicleVelocity>(a).unwrap().yaw_turns_per_second, 0.0625);
    assert_eq!(app.world().get::<BodyDynamics>(a).unwrap().lateral_force, -4096.0);
    assert_eq!(app.world().get::<BodyDynamics>(a).unwrap().yaw_torque, 8192.0);
    assert_eq!(app.world().get::<game::vehicle::drivetrain::DrivetrainInput>(a).unwrap().adaptive_angle_turns, -1.0);
    // Optional transmission state overrides the fallback input without the
    // transmission plugin. This reset skips external forces and retains y/tilt.
    app.world_mut()
        .entity_mut(a)
        .insert(game::vehicle::transmission::TransmissionState { first_gear_ticks: 1, ..default() });
    app.world_mut().get_mut::<VehicleVelocity>(a).unwrap().linear_mps.y = 2.0;
    app.world_mut().get_mut::<BodyDynamics>(a).unwrap().secondary_angular_motion = Vec2::ONE;
    app.run_tick(&[]);
    assert_eq!(app.world().get::<VehicleVelocity>(a).unwrap().linear_mps, Vec3::Y * 2.0);
    assert_eq!(app.world().get::<VehicleVelocity>(a).unwrap().yaw_turns_per_second, 0.0);
    assert_eq!(app.world().get::<BodyDynamics>(a).unwrap().secondary_angular_motion, Vec2::ONE);
}
