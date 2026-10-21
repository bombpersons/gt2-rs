//! Original tyre response/force outputs with fresh native slip inputs.
use bevy::prelude::*;
use engine::testing::{TickApp, headless_app};
use game::vehicle::WheelSteering;
use game::vehicle::longitudinal_slip::{
    LongitudinalSlip, LongitudinalSlipRulesPlugin, SlipReference, TyreForceDirection, WheelSurfaceMotion,
};
use game::vehicle::slip::{WheelLocalVelocities, WheelSlip, WheelSlipRulesPlugin};
use game::vehicle::tyre_forces::*;
use std::collections::HashMap;

type Row = HashMap<String, i32>;
fn rows(name: &str) -> Vec<Row> {
    let p = format!("{}/tests/data/original/tyre_forces/{name}.csv", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(p).unwrap();
    let mut lines = text.lines();
    let columns: Vec<_> = lines.next().unwrap().split(',').collect();
    lines
        .map(|line| columns.iter().zip(line.split(',')).map(|(k, v)| (k.to_string(), v.parse().unwrap())).collect())
        .collect()
}
fn check(name: &str, count: usize) {
    let original = rows(name);
    assert_eq!(original.len(), count);
    let mut app = headless_app(game::tick_rate(game::REGION));
    // Independent feature group, no vehicle controller, pad, renderer or
    // precomputed slip recordings supplied as native outputs.
    app.add_plugins((TyreForcesRulesPlugin, LongitudinalSlipRulesPlugin, WheelSlipRulesPlugin));
    let car = app
        .world_mut()
        .spawn((
            TyreContactState::default(),
            WheelSurfaceMotion::default(),
            WheelLocalVelocities::default(),
            WheelSteering::default(),
            WheelSlip::default(),
            LongitudinalSlip::default(),
            TyreForces::default(),
        ))
        .id();
    let mut saw_correction_without_activation = false;
    for group in original.as_chunks::<4>().0 {
        let field = |name: &str| std::array::from_fn(|i| group[i][name] as f32 / 4096.0);
        let grip = std::array::from_fn(|i| group[i]["grip"] as f32);
        assert!(group.iter().all(|r| r["body_forward"] == group[0]["body_forward"]));
        app.world_mut().entity_mut(car).insert((
            TyreContactState { grip_capacity: grip, body_forward_mps: group[0]["body_forward"] as f32 / 4096.0 },
            WheelSurfaceMotion { tread_speed_mps: field("tread"), speed_response: field("speed_response") },
            WheelLocalVelocities { forward_mps: field("forward"), lateral_mps: field("lateral") },
            WheelSteering { angle_turns: field("steering") },
        ));
        app.run_tick(&[]);
        let geometric = app.world().get::<WheelSlip>(car).unwrap();
        let longitudinal = app.world().get::<LongitudinalSlip>(car).unwrap();
        let actual = app.world().get::<TyreForces>(car).unwrap();
        for (i, row) in group.iter().enumerate() {
            let case = row["case"];
            assert_eq!(row["wheel"], i as i32);
            assert_eq!(
                geometric.angle_turns[i] * 4096.0,
                row["geometric_angle"] as f32,
                "case {case}: geometric angle"
            );
            assert_eq!(geometric.activation[i] * 4096.0, row["activation"] as f32, "case {case}: activation");
            assert_eq!(longitudinal.ratio[i] * 4096.0, row["slip"] as f32, "case {case}: longitudinal slip");
            assert_eq!(
                match longitudinal.reference[i] {
                    SlipReference::Wheel => 1,
                    SlipReference::Ground => -1,
                },
                row["reference"],
                "case {case}: reference"
            );
            assert_eq!(
                match longitudinal.force_direction[i] {
                    TyreForceDirection::Reverse => -1,
                    TyreForceDirection::None => 0,
                    TyreForceDirection::Forward => 1,
                },
                row["direction"],
                "case {case}: direction"
            );
            for (field, value, scale) in [
                ("longitudinal_response", actual.longitudinal_response[i], 4096.0),
                ("corner_limit", actual.cornering_limit[i], 4096.0),
                ("corner_response", actual.cornering_response[i], 4096.0),
                ("wheel_force", actual.wheel_reference_force[i], 1.0),
                ("ground_force", actual.ground_reference_force[i], 1.0),
                ("response_force", actual.wheel_response_force[i], 1.0),
                ("lateral_force", actual.lateral_force[i], 1.0),
            ] {
                assert_eq!(value * scale, row[field] as f32, "{name} case {case} wheel {i}: {field}");
            }
            saw_correction_without_activation |= row["activation"] == 0 && row["lateral_force"] != 0;
        }
    }
    if name == "boundaries" {
        assert!(saw_correction_without_activation, "the correction must execute independently of activation");
        // Prove coverage from consumed original inputs, rather than the
        // intended intervention: upstream stopped-wheel handling can change
        // a requested slip before it reaches the paired tyre sampler.
        let tuning = app.world().resource::<TyreTuning>();
        for axle in 0..2 {
            let axle_rows: Vec<_> = original.iter().filter(|row| row["wheel"] / 2 == axle as i32).collect();
            let slips: std::collections::HashSet<_> = axle_rows.iter().map(|row| row["slip"]).collect();
            for knot in &tuning.axles[axle].slip_curve {
                let sample = (knot.slip * 4096.0).round() as i32;
                for adjacent in [sample - 1, sample, sample + 1] {
                    if (-4096..=4096).contains(&adjacent) {
                        assert!(slips.contains(&adjacent), "axle {axle}: missing consumed slip {adjacent}");
                    }
                }
            }
            let angles: std::collections::HashSet<_> = axle_rows
                .iter()
                .filter(|row| {
                    row["activation"] > 0 && row["activation"] < 4096 && row["corner_response"] < row["corner_limit"]
                })
                .map(|row| {
                    let reverse = if row["tread"] <= 0 { 2048 } else { 0 };
                    let angle =
                        ((row["steering"] + reverse - row["geometric_angle"] + 2048).rem_euclid(4096) - 2048).abs();
                    if angle > 1024 { 2048 - angle } else { angle }
                })
                .collect();
            for knot in &tuning.axles[axle].cornering_curve {
                let sample = (knot.angle_turns * 4096.0).round() as i32;
                for adjacent in [sample - 1, sample, sample + 1] {
                    if (0..=1024).contains(&adjacent) {
                        assert!(angles.contains(&adjacent), "axle {axle}: missing unmasked angle {adjacent}");
                    }
                }
            }
        }
    }
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original tyre-force fixtures; see docs/assets.md")]
fn original_curve_knots_force_channels_cornering_and_low_speed_boundaries() {
    check("boundaries", 860);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original tyre-force fixtures; see docs/assets.md")]
fn independent_original_signed_and_mixed_control_holdouts() {
    check("holdout", 112);
}

#[test]
fn handmade_tyres_work_without_the_other_vehicle_plugins_or_disc_assets() {
    let axle = || TyreAxleTuning {
        slip_curve: vec![
            TyreSlipKnot { slip: -1.0, longitudinal: 0.5, cornering_limit: 0.75 },
            TyreSlipKnot { slip: 0.0, longitudinal: 0.0, cornering_limit: 1.0 },
            TyreSlipKnot { slip: 1.0, longitudinal: 0.5, cornering_limit: 0.75 },
        ],
        cornering_curve: vec![
            CorneringKnot { angle_turns: 0.0, response: 0.0 },
            CorneringKnot { angle_turns: 0.25, response: 1.0 },
        ],
    };
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(TyreTuning {
        axles: [axle(), axle()],
        low_speed: LowSpeedTyreCorrection {
            tread_limit_mps: 2.0,
            body_tread_difference_limit_mps: 1.0,
            saturation_speed_mps: 1.0,
            force_per_lateral_mps: 10.0,
            max_force: 12.0,
            fade_per_mps: 0.5,
        },
    })
    .add_plugins(TyreForcesRulesPlugin);
    let car = app
        .world_mut()
        .spawn((
            TyreContactState { grip_capacity: [120.0, 120.0, 0.0, 120.0], body_forward_mps: 0.0 },
            WheelSurfaceMotion { tread_speed_mps: [16.0, 16.0, 0.0, -16.0], speed_response: [1.0, 0.5, 1.0, 1.0] },
            WheelLocalVelocities { forward_mps: [0.0; 4], lateral_mps: [0.0, 0.0, 0.5, 0.0] },
            WheelSteering::default(),
            WheelSlip::default(),
            LongitudinalSlip {
                ratio: [-1.0, 1.0, 0.0, -1.0],
                reference: [SlipReference::Wheel, SlipReference::Ground, SlipReference::Wheel, SlipReference::Wheel],
                force_direction: [
                    TyreForceDirection::Forward,
                    TyreForceDirection::Reverse,
                    TyreForceDirection::None,
                    TyreForceDirection::Reverse,
                ],
                ..default()
            },
            TyreForces::default(),
        ))
        .id();
    app.run_tick(&[]);
    let f = app.world().get::<TyreForces>(car).unwrap();
    assert_eq!(f.wheel_reference_force, [60.0, 0.0, 0.0, -60.0]);
    assert_eq!(f.ground_reference_force, [0.0, -60.0, 0.0, 0.0]);
    assert_eq!(f.wheel_response_force, [60.0, 0.0, 0.0, -60.0]);
    assert_eq!(f.lateral_force, [0.0, 0.0, 5.0, 0.0]);
}
