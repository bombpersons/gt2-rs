//! Original aerodynamic preparation feeding native forward/suspension/body rules.
use bevy::prelude::*;
use engine::testing::{TickApp, headless_app};
use game::vehicle::{
    VehicleStepDuration, aerodynamics::*, forward_dynamics::ForwardDynamicsInput, suspension::SuspensionInput,
    tyre_forces::TyreContactState,
};

#[path = "support/vertical_body.rs"]
mod original;

fn check(name: &str, count: usize) {
    let text =
        std::fs::read_to_string(format!("{}/tests/data/original/aerodynamics/{name}.csv", env!("CARGO_MANIFEST_DIR")))
            .unwrap();
    let mut lines = text.lines();
    let keys: Vec<_> = lines.next().unwrap().split(',').collect();
    let rows: Vec<original::Row> = lines
        .map(|l| keys.iter().zip(l.split(',')).map(|(k, v)| (k.to_string(), v.parse().unwrap())).collect())
        .collect();
    assert!(rows.iter().any(|r| r["aero_before_growth"] > 0));
    assert!(rows.iter().any(|r| r["aero_before_growth"] < 0));
    assert!(rows.iter().any(|r| r["aero_before_growth"] == 0));
    assert!(rows.iter().any(|r| r["aero_before_speed"] > 0));
    assert!(rows.iter().any(|r| r["aero_before_speed"] < 0));
    if name == "boundaries" {
        assert!(rows.iter().any(|r| r["aero_square"] < 0));
        assert!(rows.iter().any(|r| r["aero_after_reduction"] == 4096));
        assert!(rows.iter().any(|r| r["aero_after_reduction"] == 0));
        assert!(
            rows.iter().any(|r| r["aero_before_reduction"] > 0
                && r["aero_before_growth"] > 0
                && r["aero_after_reduction"] < 0)
        );
    }
    original::check_rows(rows, &format!("aerodynamics {name}"), true, count, true);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original aerodynamic fixtures")]
fn original_speed_reduction_drag_and_native_force_consumers() {
    check("boundaries", 192);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original aerodynamic holdout")]
fn independent_original_aerodynamics_holdout() {
    check("holdout", 74);
}

#[test]
fn handmade_aerodynamics_runs_alone_with_per_car_setup_and_optional_bridges() {
    let tuning = AerodynamicsTuning {
        drag_force_per_squared_mps: 4.0,
        axle_force_per_squared_mps: [2.0, 3.0],
        minimum_response: 0.5,
        recovery_per_second: 0.25,
    };
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(tuning.clone()).add_plugins(AerodynamicsRulesPlugin);
    let car = app
        .world_mut()
        .spawn((
            AerodynamicsInput { forward_mps: 4.0, reduction_growth_per_second: 0.0 },
            Aerodynamics { reduction: 1.0 / 64.0, ..default() },
            VehicleStepDuration(1.0 / 16.0),
            ForwardDynamicsInput::default(),
            SuspensionInput::default(),
        ))
        .id();
    let mut variant = tuning;
    variant.drag_force_per_squared_mps = 8.0;
    let other = app
        .world_mut()
        .spawn((
            AerodynamicsSetup(variant),
            AerodynamicsInput { forward_mps: f32::NAN, reduction_growth_per_second: 1.0 },
            TyreContactState { body_forward_mps: -4.0, ..default() },
            Aerodynamics { reduction: 1.0, ..default() },
            VehicleStepDuration(1.0 / 16.0),
        ))
        .id();
    app.run_tick(&[]);
    assert_eq!(app.world().get::<ForwardDynamicsInput>(car).unwrap().aerodynamic_force, -64.0);
    assert_eq!(app.world().get::<SuspensionInput>(car).unwrap().axle_force, [32.0, 48.0]);
    assert_eq!(app.world().get::<Aerodynamics>(car).unwrap().reduction, 0.0);
    assert_eq!(app.world().get::<Aerodynamics>(other).unwrap().drag_force, 64.0);
    assert_eq!(app.world().get::<Aerodynamics>(other).unwrap().axle_force, [16.0, 24.0]);
}
