//! Coupled original engine/axle/wheel phases; contact inputs remain observed.
use bevy::prelude::*;
use engine::testing::{TickApp, headless_app};
use game::vehicle::braking::{BrakingRulesPlugin, WheelBrakeDemand, WheelBrakeInput};
use game::vehicle::drivetrain::*;
use game::vehicle::engine_torque::*;
use game::vehicle::forward_dynamics::*;
use game::vehicle::longitudinal_slip::{LongitudinalSlip, LongitudinalSlipRulesPlugin, WheelSurfaceMotion};
use game::vehicle::slip::{WheelLocalVelocities, WheelSlip, WheelSlipRulesPlugin};
use game::vehicle::tyre_forces::*;
use game::vehicle::wheel_rotation::*;
use game::vehicle::{VehicleStepDuration, WheelSteering};
use std::collections::{HashMap, HashSet};

type Row = HashMap<String, i32>;
fn rows(name: &str) -> Vec<Row> {
    let data =
        std::fs::read_to_string(format!("{}/tests/data/original/drivetrain/{name}.csv", env!("CARGO_MANIFEST_DIR")))
            .unwrap();
    let mut lines = data.lines();
    let keys: Vec<_> = lines.next().unwrap().split(',').collect();
    lines
        .map(|line| keys.iter().zip(line.split(',')).map(|(k, v)| (k.to_string(), v.parse().unwrap())).collect())
        .collect()
}
fn engagement(value: i32) -> EngineEngagement {
    match value {
        0 => EngineEngagement::Disconnected,
        1 => EngineEngagement::Coupled,
        2 => EngineEngagement::Synchronizing,
        3 => EngineEngagement::FixedClutch,
        4 => EngineEngagement::Slipping,
        _ => panic!("unknown original engagement"),
    }
}
fn layout(r: &Row) -> DriveLayout {
    let fraction = r["dt_before_viscosity"] as f32 / 4096.0;
    match r["dt_before_layout"] {
        0 => DriveLayout::Rear,
        1 => DriveLayout::Front,
        2 => DriveLayout::Viscous,
        3 => DriveLayout::Open,
        4 => DriveLayout::Clutch { front_fraction: fraction },
        5 => DriveLayout::AdaptiveClutch,
        6 => DriveLayout::Split { front_fraction: fraction },
        _ => panic!("unknown original layout"),
    }
}
fn check(name: &str, count: usize) {
    let original = rows(name);
    assert_eq!(original.len(), count * 4);
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.add_plugins((
        DrivetrainRulesPlugin,
        WheelRotationRulesPlugin,
        ForwardDynamicsRulesPlugin,
        TyreForcesRulesPlugin,
        LongitudinalSlipRulesPlugin,
        WheelSlipRulesPlugin,
        BrakingRulesPlugin,
        EngineTorqueRulesPlugin,
    ));
    let entity = app
        .world_mut()
        .spawn((
            (
                DrivetrainInput::default(),
                DrivetrainState::default(),
                DrivetrainOutput::default(),
                EngineTorqueInput::default(),
                EngineFuelCut::default(),
                EngineTorque::default(),
            ),
            (
                WheelRotationInput::default(),
                AxleDrive::default(),
                WheelRotationState::default(),
                WheelReactionTorque::default(),
                WheelBrakeDemand::default(),
                ForwardDynamicsInput::default(),
                ForwardDynamics::default(),
                TyreContactState::default(),
                WheelSurfaceMotion::default(),
                WheelLocalVelocities::default(),
                WheelSteering::default(),
                WheelSlip::default(),
                LongitudinalSlip::default(),
                TyreForces::default(),
                VehicleStepDuration::default(),
            ),
        ))
        .id();
    let mut previous_sequence = None;
    let mut modes = HashSet::new();
    let mut engagements = HashSet::new();
    let mut transitions = HashSet::new();
    let mut native_carry = 0;
    let mut observed_stops = 0;
    let mut observed_direct_steps = 0;
    let mut word_alias_stops = 0;
    let mut angles = HashSet::new();
    let mut brake_travel = HashSet::new();
    for group in original.as_chunks::<4>().0 {
        let r = &group[0];
        let case = r["case"];
        if name == "coupling_holdout" {
            observed_stops += r["dt_stop"];
            observed_direct_steps += r["dt_locked_direct"];
            word_alias_stops += usize::from(r["dt_stop"] > 0 && r["dt_before_speed0"].abs() > 900 * 4096);
            angles.insert(r["dt_before_angle"]);
            brake_travel.insert(r["dt_before_brake"]);
        }
        let before = |key: &str| r[&format!("dt_before_{key}")] as f32;
        let before_two = |key: &str| std::array::from_fn(|i| before(&format!("{key}{i}")));
        let four = |key: &str| std::array::from_fn(|i| r[&format!("rot_{key}{i}")] as f32);
        let wheel_field = |key: &str| std::array::from_fn(|i| group[i][key] as f32 / 4096.0);
        assert_eq!(r["dt_before_source"], 5, "alternate torque controllers need separate producer evidence");
        assert_eq!(r["dt_before_induction"], 0, "induction producer is a separate boundary");
        let mut tuning = app.world().resource::<DrivetrainTuning>().clone();
        tuning.layout = layout(r);
        tuning.reference_axle = if r["dt_before_reference"] == 0 { Axle::Front } else { Axle::Rear };
        tuning.gear_ratios = (0..8).map(|i| before(&format!("gear_ratio{i}")) / 4096.0).collect();
        tuning.axle_inertia = before_two("axle_inertia").map(|v| v / 16.0);
        tuning.engine_inertia = before("engine_inertia") / 16.0;
        tuning.centre_rotation_response = before("centre_response") / 256.0;
        tuning.centre_viscosity = before("viscosity");
        tuning.adaptive_lock = before("lock_torque") != 0.0;
        tuning.centre_lock_torque = before("lock_torque");
        tuning.engine_rotation_response = before("engine_response") / 4096.0;
        tuning.fixed_clutch_torque = before("fixed_clutch");
        tuning.coast_clutch_torque = before("coast_clutch");
        app.insert_resource(tuning);
        let old_locked = before("locked") != 0.0;
        let old_delta = before_two("delta").map(|v| v / 4096.0);
        let old_speeds = before_two("speed").map(|v| v / 4096.0);
        let independent = before_two("wheel_kind").map(|v| v == 0.0);
        let recorded_wheel_history = four("before_delta").map(|v| v / 4096.0);
        let mut wheel_state =
            WheelRotationState { previous_delta_mps: recorded_wheel_history, clutch_locked: [false; 2] };
        let mut state = DrivetrainState { previous_angular_delta: old_delta, centre_locked: old_locked };
        let mut drive = AxleDrive { radians_per_second: old_speeds, ..default() };
        if previous_sequence == Some(r["sequence"]) {
            wheel_state = *app.world().get::<WheelRotationState>(entity).unwrap();
            state = *app.world().get::<DrivetrainState>(entity).unwrap();
            drive = *app.world().get::<AxleDrive>(entity).unwrap();
            assert_eq!(state.centre_locked, old_locked, "{name} {case}: persistent centre latch");
            for axle in 0..2 {
                assert_eq!(
                    wheel_state.previous_delta_mps[axle * 2],
                    recorded_wheel_history[axle * 2],
                    "{name} {case}: native wheel history"
                );
                if independent[axle] {
                    assert_eq!(
                        wheel_state.previous_delta_mps[axle * 2 + 1],
                        recorded_wheel_history[axle * 2 + 1],
                        "{name} {case}: independent right wheel history"
                    );
                    state.previous_angular_delta[axle] = old_delta[axle];
                } else {
                    assert_eq!(state.previous_angular_delta[axle], old_delta[axle], "{name} {case}: angular history");
                }
                if independent[axle] || r[&format!("dt_before_external_speed{axle}")] != 0 {
                    // Contact correction 80030830 can reset a shaft between
                    // these phases. Apply only explicitly observed writes;
                    // the native angular history and centre latch still carry.
                    drive.radians_per_second[axle] = old_speeds[axle];
                } else {
                    assert_eq!(drive.radians_per_second[axle], old_speeds[axle], "{name} {case}: axle speed carry");
                    native_carry += 1;
                }
            }
        }
        let mut rotation = app.world().resource::<WheelRotationTuning>().clone();
        rotation.radius_m = std::array::from_fn(|i| r[&format!("rot_radius{i}")] as f32 / 4096.0);
        rotation.rotation_response = std::array::from_fn(|i| r[&format!("rot_rotation_response{i}")] as f32 / 512.0);
        rotation.differentials =
            independent.map(|value| if value { Differential::Independent } else { Differential::Open });
        app.insert_resource(rotation);
        app.world_mut().entity_mut(entity).insert((
            (
                DrivetrainInput {
                    gear: r["dt_before_gear"] as usize,
                    engagement: engagement(r["dt_before_engagement"]),
                    adaptive_angle_turns: before("angle") / 4096.0,
                    brake_travel: before("brake") / 4096.0,
                    alternate_locked_step: before("alternate_locked_step") != 0.0,
                    independent_wheels: independent,
                    ..default()
                },
                state,
                drive,
                EngineTorqueInput {
                    speed_rev_s: before("engine_speed") / 4096.0,
                    integer_rpm: before("rpm"),
                    effective_throttle: if r["dt_before_engagement"] == 3 {
                        -17.0
                    } else {
                        before("throttle") / 4096.0
                    },
                },
                EngineFuelCut { active: before("cut") != 0.0 },
            ),
            (
                WheelRotationInput {
                    auxiliary_tread_force: four("auxiliary_force"),
                    yaw_correction: r["rot_source_yaw_correction"] as f32 / 4096.0,
                    steering_turns: r["rot_steering"] as f32 / 4096.0,
                },
                wheel_state,
                WheelBrakeInput {
                    travel: four("brake_input").map(|v| v / 4096.0),
                    handbrake: r["rot_handbrake"] as f32 / 4096.0,
                },
                ForwardDynamicsInput {
                    wheel_load: std::array::from_fn(|i| r[&format!("fd_load{i}")] as f32),
                    surface: std::array::from_fn(|i| r[&format!("fd_surface{i}")] as usize),
                    aerodynamic_force: r["fd_aerodynamic_force"] as f32,
                },
                TyreContactState {
                    grip_capacity: std::array::from_fn(|i| group[i]["grip"] as f32),
                    body_forward_mps: r["body_forward"] as f32 / 4096.0,
                },
                WheelSurfaceMotion {
                    tread_speed_mps: wheel_field("tread"),
                    speed_response: wheel_field("speed_response"),
                },
                WheelLocalVelocities { forward_mps: wheel_field("forward"), lateral_mps: wheel_field("lateral") },
                WheelSteering { angle_turns: wheel_field("steering") },
                VehicleStepDuration(r["fd_duration"] as f32 / 65536.0),
            ),
        ));
        app.run_tick(&[]);
        let engine = app.world().get::<EngineTorqueInput>(entity).unwrap();
        let torque = app.world().get::<EngineTorque>(entity).unwrap();
        if r["dt_before_engagement"] != 0 {
            assert_eq!(torque.full_throttle, before("full_torque"), "{name} {case}: engine curve");
            assert_eq!(torque.drag, before("drag"), "{name} {case}: engine drag");
        }
        assert_eq!(torque.net, r["dt_source_torque"] as f32, "{name} {case}: source torque");
        assert_eq!(engine.speed_rev_s * 4096.0, r["dt_after_engine_speed"] as f32, "{name} {case}: engine rotation");
        assert_eq!(app.world().get::<EngineFuelCut>(entity).unwrap().active, r["dt_after_cut"] != 0);
        let output = app.world().get::<DrivetrainOutput>(entity).unwrap();
        assert_eq!(output.gearbox_torque, r["dt_gearbox"] as f32, "{name} {case}: gearbox torque");
        assert_eq!(
            output.clutch_speeds_rev_s,
            before_two("clutch_speed").map(|v| v / 4096.0),
            "{name} {case}: native clutch speeds"
        );
        let drive = app.world().get::<AxleDrive>(entity).unwrap();
        let state = app.world().get::<DrivetrainState>(entity).unwrap();
        for axle in 0..2 {
            assert_eq!(
                drive.radians_per_second[axle] * 4096.0,
                r[&format!("dt_after_speed{axle}")] as f32,
                "{name} {case}: axle {axle} speed"
            );
            assert_eq!(
                drive.torque[axle],
                r[&format!("dt_after_torque{axle}")] as f32,
                "{name} {case}: axle {axle} torque"
            );
            assert_eq!(
                state.previous_angular_delta[axle] * 4096.0,
                r[&format!("dt_after_delta{axle}")] as f32,
                "{name} {case}: axle {axle} history"
            );
        }
        assert_eq!(state.centre_locked, r["dt_after_locked"] != 0, "{name} {case}: centre latch");
        let motion = app.world().get::<WheelSurfaceMotion>(entity).unwrap();
        let wheels = app.world().get::<WheelRotationState>(entity).unwrap();
        let reaction = app.world().get::<WheelReactionTorque>(entity).unwrap();
        for i in 0..4 {
            assert_eq!(
                r[&format!("dt_before_flags{i}")] | output.wheel_flags[i] as i32,
                r[&format!("dt_after_flags{i}")],
                "{name} {case}: wheel {i} flags"
            );
            assert_eq!(
                motion.tread_speed_mps[i] * 4096.0,
                r[&format!("rot_after_tread{i}")] as f32,
                "{name} {case}: wheel {i} tread"
            );
            assert_eq!(
                wheels.previous_delta_mps[i] * 4096.0,
                r[&format!("rot_after_delta{i}")] as f32,
                "{name} {case}: wheel {i} history"
            );
            assert_eq!(
                reaction.prepared[i],
                r[&format!("rot_prepared_torque{i}")] as f32,
                "{name} {case}: wheel {i} reaction"
            );
        }
        modes.insert(r["dt_before_layout"]);
        engagements.insert(r["dt_before_engagement"]);
        if old_locked != state.centre_locked {
            transitions.insert((old_locked, state.centre_locked));
        }
        previous_sequence = Some(r["sequence"]);
    }
    if name == "coupling_holdout" {
        assert!(observed_stops > 0, "original stopping branch not observed");
        assert!(observed_direct_steps > 0, "original locked integration exception not observed");
        assert!(word_alias_stops > 0, "high-speed product-word stopping cases not observed");
        assert!(angles.is_superset(&HashSet::from([1024, 1025, 4095, 4096, 4097, -1025, -4097])));
        assert!(brake_travel.is_superset(&HashSet::from([3071, 3072, 3073])));
    } else {
        assert!(native_carry >= 19);
    }
    assert_eq!(modes.len(), 7);
    if name == "boundaries" {
        assert_eq!(engagements.len(), 5);
        assert_eq!(transitions.len(), 2);
    }
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private drivetrain originals; see docs/assets.md")]
fn original_engine_clutch_gear_centre_and_axle_boundaries() {
    check("boundaries", 222);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private drivetrain originals; see docs/assets.md")]
fn independent_original_signed_mixed_holdout() {
    check("holdout", 41);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private drivetrain originals; see docs/assets.md")]
fn final_original_adaptive_angles_brake_gate_gears_and_stopping() {
    check("coupling_holdout", 75);
}

#[test]
fn handmade_drivetrain_runs_alone_without_original_assets() {
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(DrivetrainTuning {
        layout: DriveLayout::Front,
        reference_axle: Axle::Front,
        gear_ratios: vec![1.0, 2.0],
        axle_inertia: [1.0; 2],
        engine_inertia: 0.5,
        centre_rotation_response: 0.0,
        centre_viscosity: 0.0,
        adaptive_lock: false,
        centre_lock_torque: 1.0,
        engine_rotation_response: 1.0,
        revolutions_per_radian: 0.125,
        fixed_clutch_torque: 1.0,
        coast_clutch_torque: 1.0,
        clutch_free_difference_rev_s: 1.0,
        clutch_gain_per_rev_s: 0.25,
        max_clutch_gain: 2.0,
        adaptive_full_angle_turns: 0.25,
        adaptive_min_angle_turns: 1.0,
        adaptive_min_response: 0.25,
        adaptive_taper_intercept: 1.25,
        lock_brake_travel: 0.5,
    })
    .add_plugins(DrivetrainRulesPlugin);
    let spawn = |world: &mut World| {
        world
            .spawn((
                DrivetrainInput { engagement: EngineEngagement::Coupled, ..default() },
                DrivetrainState::default(),
                DrivetrainOutput::default(),
                AxleDrive { radians_per_second: [1.0, 2.0], ..default() },
                EngineTorqueInput { speed_rev_s: 11.0, ..default() },
                EngineTorque { net: 2.0, ..default() },
                VehicleStepDuration(0.5),
                WheelBrakeDemand::default(),
                WheelReactionTorque::default(),
            ))
            .id()
    };
    let car = spawn(app.world_mut());
    let mut other_setup = app.world().resource::<DrivetrainTuning>().clone();
    other_setup.layout = DriveLayout::Rear;
    let other = spawn(app.world_mut());
    app.world_mut().entity_mut(other).insert(DrivetrainSetup(other_setup));
    app.run_tick(&[]);
    let drive = app.world().get::<AxleDrive>(car).unwrap();
    assert_eq!(drive.radians_per_second, [1.5, 2.0]);
    assert_eq!(drive.torque, [4.0, 0.0]);
    assert_eq!(app.world().get::<EngineTorqueInput>(car).unwrap().speed_rev_s, 11.0);
    assert_eq!(app.world().get::<DrivetrainOutput>(car).unwrap().wheel_flags, [8, 8, 0, 0]);
    let other_drive = app.world().get::<AxleDrive>(other).unwrap();
    assert_eq!(other_drive.radians_per_second, [1.0, 2.5]);
    assert_eq!(other_drive.torque, [0.0, 4.0]);
    assert_eq!(app.world().get::<DrivetrainOutput>(other).unwrap().wheel_flags, [0, 0, 8, 8]);
}
