//! Original wheel preparation/integration, with native slip, tyres and forward stage.
use bevy::prelude::*;
use engine::testing::{TickApp, headless_app};
use game::vehicle::braking::{BrakingRulesPlugin, WheelBrakeDemand, WheelBrakeInput};
use game::vehicle::forward_dynamics::*;
use game::vehicle::longitudinal_slip::{LongitudinalSlip, LongitudinalSlipRulesPlugin, WheelSurfaceMotion};
use game::vehicle::slip::{WheelLocalVelocities, WheelSlip, WheelSlipRulesPlugin};
use game::vehicle::tyre_forces::*;
use game::vehicle::wheel_rotation::*;
use game::vehicle::{VehicleStepDuration, WheelSteering};
use std::collections::{HashMap, HashSet};
type Row = HashMap<String, i32>;
fn rows(name: &str) -> Vec<Row> {
    let text = std::fs::read_to_string(format!(
        "{}/tests/data/original/wheel_rotation/{name}.csv",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let mut lines = text.lines();
    let columns: Vec<_> = lines.next().unwrap().split(',').collect();
    lines
        .map(|line| columns.iter().zip(line.split(',')).map(|(k, v)| (k.to_string(), v.parse().unwrap())).collect())
        .collect()
}
fn differential(row: &Row, axle: usize) -> Differential {
    let v = |key: &str| row[&format!("rot_{key}{axle}")] as f32;
    match v("mode") as i32 {
        0 => Differential::Independent,
        1 => Differential::Locked,
        2 => Differential::Open,
        3 | 4 => Differential::Clutch {
            preload_torque: v("preload"),
            drive_response: v("drive_response") / 4096.0,
            coast_response: v("coast_response") / 4096.0,
        },
        5 => Differential::Viscous {
            slip_squared_limit_mps2: v("preload") / 4096.0,
            torque_per_squared_mps: v("drive_response"),
        },
        6 | 7 => Differential::YawBiased { response: v("preload") },
        _ => panic!("unknown source differential"),
    }
}
fn check(name: &str, count: usize) {
    let original = rows(name);
    assert_eq!(original.len(), count * 4);
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.add_plugins((
        WheelRotationRulesPlugin,
        ForwardDynamicsRulesPlugin,
        TyreForcesRulesPlugin,
        LongitudinalSlipRulesPlugin,
        WheelSlipRulesPlugin,
        BrakingRulesPlugin,
    ));
    let car = app
        .world_mut()
        .spawn((
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
        ))
        .id();
    let mut modes = HashSet::new();
    let mut latch_changes = 0;
    let mut yaw_biases = 0;
    let mut clutch_torque_signs = HashSet::new();
    let mut previous: Option<(i32, [f32; 4], [bool; 2])> = None;
    let mut continued_clutch_ticks = 0;
    let mut word_clamps = 0;
    let mut gate_offsets = HashSet::new();
    let mut suppressed_yaw_inputs = 0;
    for group in original.as_chunks::<4>().0 {
        let r = &group[0];
        let case = r["case"];
        let field = |name: &str| std::array::from_fn(|i| group[i][name] as f32 / 4096.0);
        let four = |name: &str| std::array::from_fn(|i| r[&format!("rot_{name}{i}")] as f32);
        let two = |name: &str| std::array::from_fn(|i| r[&format!("rot_{name}{i}")] as f32);
        let old_latches = two("before_latch").map(|v| v != 0.0);
        let mut rotation_state = WheelRotationState {
            previous_delta_mps: four("before_delta").map(|v| v / 4096.0),
            clutch_locked: old_latches,
        };
        if let Some((sequence, deltas, latches)) = previous {
            if sequence == r["sequence"] {
                // Carry the native subsystem's history, rather than reseeding
                // it from every original row. The intervening axle solver
                // owns only the coupled axles' right-hand history slots.
                rotation_state = *app.world().get::<WheelRotationState>(car).unwrap();
                assert_eq!(old_latches, latches, "persistent clutch latch must carry between calls");
                for axle in 0..2 {
                    assert_eq!(
                        four("before_delta")[axle * 2] / 4096.0,
                        deltas[axle * 2],
                        "left differential increment must carry between calls"
                    );
                    if r[&format!("rot_mode{axle}")] != 0 {
                        rotation_state.previous_delta_mps[axle * 2 + 1] = four("before_delta")[axle * 2 + 1] / 4096.0;
                    }
                    continued_clutch_ticks += usize::from(matches!(r[&format!("rot_mode{axle}")], 3 | 4));
                }
                assert_eq!(rotation_state.previous_delta_mps, four("before_delta").map(|v| v / 4096.0));
            }
        }
        let min_forward = app.world().resource::<WheelRotationTuning>().yaw_bias_min_forward_mps;
        app.insert_resource(WheelRotationTuning {
            radius_m: two("radius").map(|v| v / 4096.0),
            rotation_response: two("rotation_response").map(|v| v / 512.0),
            differentials: std::array::from_fn(|i| differential(r, i)),
            yaw_bias_min_forward_mps: min_forward,
        });
        app.world_mut().entity_mut(car).insert((
            WheelBrakeInput {
                travel: four("brake_input").map(|v| v / 4096.0),
                handbrake: r["rot_handbrake"] as f32 / 4096.0,
            },
            WheelRotationInput {
                auxiliary_tread_force: four("auxiliary_force"),
                yaw_correction: r["rot_source_yaw_correction"] as f32 / 4096.0,
                steering_turns: r["rot_steering"] as f32 / 4096.0,
            },
            AxleDrive { radians_per_second: two("shaft_speed").map(|v| v / 4096.0), torque: two("drive_torque") },
            rotation_state,
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
            VehicleStepDuration(r["fd_duration"] as f32 / 65536.0),
        ));
        app.run_tick(&[]);
        assert_eq!(
            app.world().get::<WheelBrakeDemand>(car).unwrap().total,
            four("brake"),
            "{name} {case}: fresh brake demand"
        );
        let tyres = app.world().get::<TyreForces>(car).unwrap();
        let forward = app.world().get::<ForwardDynamics>(car).unwrap();
        assert_eq!(forward.forward_mps * 4096.0, r["fd_speed"] as f32, "{name} {case}: forward input");
        if name == "gate_holdout" {
            gate_offsets.insert((forward.forward_mps * 4096.0 - min_forward * 4096.0) as i32);
            suppressed_yaw_inputs +=
                usize::from(forward.forward_mps < min_forward && r["rot_source_yaw_correction"] != 0);
        }
        let motion = app.world().get::<WheelSurfaceMotion>(car).unwrap();
        let state = app.world().get::<WheelRotationState>(car).unwrap();
        let torque = app.world().get::<WheelReactionTorque>(car).unwrap();
        for (i, wheel) in group.iter().enumerate() {
            assert_eq!(
                tyres.wheel_response_force[i], wheel["response_force"] as f32,
                "{name} {case}: native tyre response {i}"
            );
            for (label, value, scale) in [
                ("prepared_torque", torque.prepared[i], 1.0),
                ("final_torque", torque.final_torque[i], 1.0),
                ("after_tread", motion.tread_speed_mps[i], 4096.0),
                ("after_delta", state.previous_delta_mps[i], 4096.0),
            ] {
                assert_eq!(value * scale, r[&format!("rot_{label}{i}")] as f32, "{name} {case}: {label} wheel {i}");
            }
        }
        for (axle, &old) in old_latches.iter().enumerate() {
            assert_eq!(
                state.clutch_locked[axle],
                r[&format!("rot_after_latch{axle}")] != 0,
                "{name} {case}: latch {axle}"
            );
            latch_changes += usize::from(state.clutch_locked[axle] != old);
            modes.insert(r[&format!("rot_mode{axle}")]);
            if matches!(r[&format!("rot_mode{axle}")], 3 | 4) {
                clutch_torque_signs.insert(two("drive_torque")[axle].total_cmp(&0.0));
            }
            word_clamps += r[&format!("rot_word_clamp{axle}")];
            yaw_biases += usize::from(
                r[&format!("rot_mode{axle}")] >= 6
                    && r["rot_yaw_correction"] != 0
                    && forward.forward_mps >= min_forward,
            );
        }
        previous = Some((r["sequence"], state.previous_delta_mps, state.clutch_locked));
    }
    if name == "boundaries" {
        assert_eq!(modes.len(), 8);
        assert!(latch_changes > 0, "clutch lock/release branches absent");
        assert!(yaw_biases > 0, "nonzero yaw-bias preparation absent");
        assert!(clutch_torque_signs.contains(&std::cmp::Ordering::Greater));
        assert!(clutch_torque_signs.contains(&std::cmp::Ordering::Less));
        assert!(continued_clutch_ticks >= 38, "continuous clutch differential state missing");
        assert!(word_clamps > 0, "signed product-word clamp not observed");
    }
    if name == "gate_holdout" {
        assert!(gate_offsets.is_superset(&HashSet::from([-1, 0, 1])));
        assert!(yaw_biases > 0);
        assert!(suppressed_yaw_inputs > 0, "gate must suppress an actual nonzero input");
    }
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original wheel-rotation fixtures; see docs/assets.md")]
fn original_all_differentials_reaction_torque_and_wheel_integration() {
    check("boundaries", 168);
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original wheel-rotation fixtures; see docs/assets.md")]
fn independent_original_signed_and_mixed_holdouts() {
    check("holdout", 36);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original wheel-rotation fixtures; see docs/assets.md")]
fn final_original_mixed_axles_and_small_signed_tread_holdout() {
    check("final_holdout", 16);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original wheel-rotation fixtures; see docs/assets.md")]
fn original_yaw_gate_exact_consumer_boundary() {
    check("gate_holdout", 10);
}

#[test]
fn handmade_rotation_runs_without_other_vehicle_plugins_or_disc_data() {
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(WheelRotationTuning {
        radius_m: [0.5; 2],
        rotation_response: [1.0; 2],
        differentials: [Differential::Independent, Differential::Locked],
        yaw_bias_min_forward_mps: 2.0,
    })
    .add_plugins(WheelRotationRulesPlugin)
    .add_systems(
        FixedUpdate,
        (|mut cars: Query<(&WheelReactionTorque, &mut AxleDrive)>| {
            for (torque, mut drive) in &mut cars {
                assert_eq!(torque.prepared, [-4096.0, 4096.0, -4096.0, 4096.0]);
                drive.radians_per_second = [0.0, 4.0];
            }
        })
        .in_set(WheelRotationSystems::AxleDrive),
    );
    let car = app
        .world_mut()
        .spawn((
            WheelRotationInput::default(),
            WheelSurfaceMotion { tread_speed_mps: [1.0, -1.0, 2.0, -2.0], speed_response: [1.0; 4] },
            WheelBrakeDemand { total: [4096.0; 4], ..default() },
            TyreForces::default(),
            ForwardDynamics::default(),
            AxleDrive::default(),
            VehicleStepDuration(0.25),
            WheelRotationState { previous_delta_mps: [0.0, 0.0, 1.0, 0.125], ..default() },
            WheelReactionTorque::default(),
        ))
        .id();
    app.run_tick(&[]);
    assert_eq!(app.world().get::<WheelReactionTorque>(car).unwrap().prepared, [-4096.0, 4096.0, -4096.0, 4096.0]);
    assert_eq!(app.world().get::<WheelSurfaceMotion>(car).unwrap().tread_speed_mps, [0.75, -0.75, 2.0, 2.0]);
    assert_eq!(app.world().get::<WheelRotationState>(car).unwrap().previous_delta_mps, [-0.5, 0.5, 0.0, 0.125]);
}

#[test]
fn native_ground_feedback_is_opt_in_and_preserves_explicit_reaction_inputs() {
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(WheelRotationTuning {
        radius_m: [0.5; 2],
        rotation_response: [1.0; 2],
        differentials: [Differential::Independent; 2],
        yaw_bias_min_forward_mps: 2.0,
    })
    .add_plugins(WheelRotationRulesPlugin);
    let spawn = |world: &mut World| {
        world
            .spawn((
                WheelRotationInput { auxiliary_tread_force: [4.0; 4], ..default() },
                WheelSurfaceMotion::default(),
                WheelBrakeDemand::default(),
                TyreForces { wheel_response_force: [2.0; 4], ground_response_force: [6.0; 4], ..default() },
                ForwardDynamics::default(),
                WheelReactionTorque::default(),
            ))
            .id()
    };
    let explicit = spawn(app.world_mut());
    let native = spawn(app.world_mut());
    app.world_mut().entity_mut(native).insert(TyreWheelFeedback);
    app.run_tick(&[]);
    assert_eq!(app.world().get::<WheelReactionTorque>(native).unwrap().prepared, [-4.0; 4]);
    assert_eq!(app.world().get::<WheelReactionTorque>(explicit).unwrap().prepared, [-3.0; 4]);
}
