//! Original complete pre-drivetrain forward phase with native slip/tyre inputs.
use bevy::prelude::*;
use engine::testing::{TickApp, headless_app};
use game::vehicle::forward_dynamics::*;
use game::vehicle::longitudinal_slip::{LongitudinalSlip, LongitudinalSlipRulesPlugin, WheelSurfaceMotion};
use game::vehicle::slip::{WheelLocalVelocities, WheelSlip, WheelSlipRulesPlugin};
use game::vehicle::tyre_forces::{TyreContactState, TyreForces, TyreForcesRulesPlugin};
use game::vehicle::{VehicleStepDuration, WheelSteering};
use std::collections::{HashMap, HashSet};

type Row = HashMap<String, i32>;
fn rows(name: &str) -> Vec<Row> {
    let path = format!("{}/tests/data/original/forward_dynamics/{name}.csv", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(path).unwrap();
    let mut lines = text.lines();
    let columns: Vec<_> = lines.next().unwrap().split(',').collect();
    lines
        .map(|line| columns.iter().zip(line.split(',')).map(|(k, v)| (k.to_string(), v.parse().unwrap())).collect())
        .collect()
}

fn check(name: &str) {
    let original = rows(name);
    assert_eq!(original.len(), if name == "boundaries" { 153 * 4 } else { 30 * 4 });
    let mut app = headless_app(game::tick_rate(game::REGION));
    // Reverse plugin registration order must still consume fresh native
    // geometric/longitudinal slip and forces, without a vehicle controller.
    app.add_plugins((
        ForwardDynamicsRulesPlugin,
        TyreForcesRulesPlugin,
        LongitudinalSlipRulesPlugin,
        WheelSlipRulesPlugin,
    ));
    let car = app
        .world_mut()
        .spawn((
            ForwardDynamicsInput::default(),
            TyreContactState::default(),
            WheelSurfaceMotion::default(),
            WheelLocalVelocities::default(),
            WheelSteering::default(),
            WheelSlip::default(),
            LongitudinalSlip::default(),
            TyreForces::default(),
            VehicleStepDuration::default(),
            ForwardDynamics::default(),
        ))
        .id();
    let mut stopped = 0;
    let mut inverse_masses = HashSet::new();
    let mut surfaces = HashSet::new();
    let mut body_speeds = HashSet::new();
    let mut lateral_asymmetry = false;
    let mut contact_masks = HashSet::new();
    let mut braked_coupled_wheel = false;
    let mut early_zero = false;
    let mut exact_stops = HashSet::new();
    let mut first_phase_crossing = false;
    for group in original.as_chunks::<4>().0 {
        let row = &group[0];
        let field = |name: &str| std::array::from_fn(|i| group[i][name] as f32 / 4096.0);
        let input = ForwardDynamicsInput {
            wheel_load: std::array::from_fn(|i| row[&format!("fd_load{i}")] as f32),
            surface: std::array::from_fn(|i| row[&format!("fd_surface{i}")] as usize),
            aerodynamic_force: row["fd_aerodynamic_force"] as f32,
        };
        app.world_mut().entity_mut(car).insert((
            input,
            TyreContactState {
                grip_capacity: std::array::from_fn(|i| group[i]["grip"] as f32),
                body_forward_mps: row["body_forward"] as f32 / 4096.0,
            },
            WheelSurfaceMotion { tread_speed_mps: field("tread"), speed_response: field("speed_response") },
            WheelLocalVelocities { forward_mps: field("forward"), lateral_mps: field("lateral") },
            WheelSteering { angle_turns: field("steering") },
            VehicleStepDuration(row["fd_duration"] as f32 / 65536.0),
        ));
        app.run_tick(&[]);
        let tyres = app.world().get::<TyreForces>(car).unwrap();
        for (i, r) in group.iter().enumerate() {
            for (key, value) in [
                ("wheel_force", tyres.wheel_reference_force[i]),
                ("ground_force", tyres.ground_reference_force[i]),
                ("lateral_force", tyres.lateral_force[i]),
            ] {
                assert_eq!(value, r[key] as f32, "{name} case {} wheel {i}: {key}", row["case"]);
            }
            assert_eq!(input.wheel_load[i] == 0.0, row[&format!("fd_flags{i}")] & 1 != 0);
            braked_coupled_wheel |= row[&format!("fd_flags{i}")] == 2;
        }
        contact_masks.insert((0..4).fold(0, |mask, i| mask | (usize::from(input.wheel_load[i] != 0.0) << i)));
        early_zero |= row["fd_first_speed"] == 0 && tyres.ground_reference_force.iter().any(|&f| f != 0.0);
        if row["fd_first_speed"] != 0 && row["fd_second_candidate"] == 0 {
            exact_stops.insert((row["fd_first_speed"] < 0, row["fd_stopped"] != 0));
        }
        first_phase_crossing |= row["body_forward"] > 0 && row["fd_first_speed"] < 0 && row["fd_speed"] < 0;
        let result = app.world().get::<ForwardDynamics>(car).unwrap();
        for (key, value, scale) in [
            ("fd_resistance", result.surface_resistance, 1.0),
            ("fd_inverse_mass", result.effective_inverse_mass, 4096.0),
            ("fd_force", result.force, 1.0),
            ("fd_acceleration", result.acceleration_mps2, 4096.0),
            ("fd_speed", result.forward_mps, 4096.0),
        ] {
            assert_eq!(value * scale, row[key] as f32, "{name} case {}: {key}", row["case"]);
        }
        assert_eq!(result.stopped, row["fd_stopped"] != 0, "{name} case {}: stop", row["case"]);
        stopped += usize::from(result.stopped);
        inverse_masses.insert(row["fd_inverse_mass"]);
        body_speeds.insert(row["body_forward"]);
        surfaces.extend(input.surface);
        lateral_asymmetry |= tyres.lateral_force.iter().any(|&f| f != tyres.lateral_force[0])
            && field("steering").iter().any(|&a| a != 0.0);
    }
    if name == "boundaries" {
        assert!(stopped >= 4, "forward/reverse stopping branches missing");
        assert_eq!(contact_masks.len(), 16, "every wheel-contact mask must reach the mass calculation");
        assert!(braked_coupled_wheel, "braking alone must retain rotational mass");
        assert!(early_zero, "zero after the first phase must skip nonzero ground-reference forces");
        assert!(inverse_masses.len() >= 6, "wheel coupling combinations missing");
        assert!(lateral_asymmetry, "the first-wheel lateral projection must be observable");
        let tuning = app.world().resource::<ForwardDynamicsTuning>();
        assert_eq!(surfaces.len(), tuning.surface_resistance.len());
        for knot in &tuning.resistance_curve {
            let speed = (knot.speed_mps * 4096.0).round() as i32;
            for adjacent in [speed - 1, speed, speed + 1] {
                if adjacent >= 0 {
                    assert!(body_speeds.contains(&adjacent), "missing consumed speed {adjacent}");
                    assert!(body_speeds.contains(&-adjacent), "missing consumed reverse speed {}", -adjacent);
                }
            }
        }
    } else {
        assert!(exact_stops.contains(&(true, true)), "negative arrival at zero must take the stop branch");
        assert!(exact_stops.contains(&(false, false)), "positive arrival at zero must accumulate full force");
        assert!(first_phase_crossing, "the first phase permits direction changes");
    }
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original forward-dynamics fixtures; see docs/assets.md")]
fn original_resistance_wheel_mass_force_projection_and_stopping() {
    check("boundaries");
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original forward-dynamics fixtures; see docs/assets.md")]
fn independent_original_asymmetric_and_mixed_control_holdouts() {
    check("holdout");
}

#[test]
fn handmade_forward_stage_runs_without_other_vehicle_plugins_or_disc_data() {
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(ForwardDynamicsTuning {
        body_mass: 1.0,
        body_inverse_mass: 1.0,
        wheel_rotational_mass: [0.25, 0.5],
        surface_resistance: vec![0.5],
        resistance_curve: vec![
            ResistanceKnot { speed_mps: 0.0, response: 0.0 },
            ResistanceKnot { speed_mps: 8.0, response: 0.5 },
        ],
    })
    .add_plugins(ForwardDynamicsRulesPlugin);
    let car = app
        .world_mut()
        .spawn((
            ForwardDynamicsInput { wheel_load: [8.0, 0.0, 0.0, 0.0], surface: [0; 4], aerodynamic_force: 3.0 },
            TyreContactState { body_forward_mps: 8.0, ..default() },
            WheelSteering::default(),
            TyreForces { wheel_reference_force: [9.0, 0.0, 0.0, 0.0], ..default() },
            VehicleStepDuration(0.25),
            ForwardDynamics::default(),
        ))
        .id();
    app.run_tick(&[]);
    let result = app.world().get::<ForwardDynamics>(car).unwrap();
    assert_eq!(result.surface_resistance, -2.0);
    assert_eq!(result.effective_inverse_mass, 3276.0 / 4096.0);
    assert_eq!(result.force, 7.0);
    assert_eq!(result.acceleration_mps2, 7.0 / 4096.0);
    assert_eq!(result.forward_mps, 8.0 + 1.0 / 4096.0);
    assert!(!result.stopped);
}
