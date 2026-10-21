//! Original longitudinal-slip executions plus the coupled braking consumer.
use bevy::prelude::*;
use engine::testing::{TickApp, headless_app};
use game::vehicle::brake_assist::{BrakeAssistInput, BrakeAssistRulesPlugin, BrakeAssistTuning};
use game::vehicle::braking::{BrakingRulesPlugin, BrakingTuning, PedalBrakes, WheelBrakeDemand, WheelBrakeInput};
use game::vehicle::longitudinal_slip::{
    LongitudinalSlip, LongitudinalSlipRulesPlugin, LongitudinalSlipTuning, SlipReference, TyreForceDirection,
    WheelSurfaceMotion, steering_basis,
};
use game::vehicle::slip::WheelLocalVelocities;
use game::vehicle::{PedalTravel, VehicleSystems, WheelSteering};
use std::collections::HashMap;

type Row = HashMap<String, i32>;
fn rows(name: &str) -> Vec<Row> {
    let path = format!("{}/tests/data/original/longitudinal_slip/{name}.csv", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(path).unwrap();
    let mut lines = text.lines();
    let columns: Vec<_> = lines.next().unwrap().split(',').collect();
    lines
        .map(|line| columns.iter().zip(line.split(',')).map(|(k, v)| (k.to_string(), v.parse().unwrap())).collect())
        .collect()
}
fn inputs(group: &[Row]) -> (WheelSurfaceMotion, WheelLocalVelocities, WheelSteering) {
    let field = |name: &str| std::array::from_fn(|i| group[i][name] as f32 / 4096.0);
    (
        WheelSurfaceMotion { tread_speed_mps: field("tread"), speed_response: field("speed_response") },
        WheelLocalVelocities { forward_mps: field("forward"), lateral_mps: field("lateral") },
        WheelSteering { angle_turns: field("steering") },
    )
}
fn compare(slip: &LongitudinalSlip, group: &[Row]) {
    for (i, row) in group.iter().enumerate() {
        assert_eq!(row["wheel"], i as i32);
        let case = row["case"];
        assert_eq!(slip.ratio[i] * 4096.0, row["slip"] as f32, "case {case}: slip");
        assert_eq!(slip.rolling_speed_mps[i] * 4096.0, row["projected"] as f32, "case {case}: projected speed");
        assert_eq!(
            match slip.reference[i] {
                SlipReference::Wheel => 1,
                SlipReference::Ground => -1,
            },
            row["regime"],
            "case {case}: denominator"
        );
        assert_eq!(
            match slip.force_direction[i] {
                TyreForceDirection::Reverse => -1,
                TyreForceDirection::None => 0,
                TyreForceDirection::Forward => 1,
            },
            row["direction"],
            "case {case}: force direction"
        );
        let [cos, sin] = steering_basis(row["steering"] as f32 / 4096.0);
        assert_eq!(cos * 4096.0, row["cosine"] as f32, "case {case}: cosine");
        assert_eq!(sin * 4096.0, row["sine"] as f32, "case {case}: sine");
    }
}
fn check(name: &str, expected: usize) {
    let original = rows(name);
    assert_eq!(original.len(), expected);
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.add_plugins(LongitudinalSlipRulesPlugin);
    let car = app
        .world_mut()
        .spawn((
            WheelSurfaceMotion::default(),
            WheelLocalVelocities::default(),
            WheelSteering::default(),
            LongitudinalSlip::default(),
        ))
        .id();
    for group in original.as_chunks::<4>().0.iter() {
        let (motion, velocity, steering) = inputs(group);
        app.world_mut().entity_mut(car).insert((motion, velocity, steering));
        app.run_tick(&[]);
        compare(app.world().get::<LongitudinalSlip>(car).unwrap(), group);
    }
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original fixtures; see docs/assets.md")]
fn original_stopped_rolling_driving_braking_and_reverse_cases() {
    check("boundaries", 204);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original fixtures; see docs/assets.md")]
fn independent_original_fractional_and_signed_holdouts() {
    check("holdout", 64);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original direction table; see docs/assets.md")]
fn every_original_steering_sine_and_cosine_pair_matches() {
    let original = rows("steering_basis");
    assert_eq!(original.len(), 4096);
    for row in original {
        let [c, s] = steering_basis(row["phase"] as f32 / 4096.0);
        assert_eq!([c * 4096.0, s * 4096.0], [row["cosine"] as f32, row["sine"] as f32], "phase {}", row["phase"]);
    }
}

fn check_braking(name: &str, scenario: &str, sequence: i32) {
    let original: Vec<_> = rows(name).into_iter().filter(|r| r["sequence"] == sequence).collect();
    let assisted = rows(&format!("{name}_{scenario}_assistance"));
    assert_eq!(original.len(), 48);
    assert_eq!(assisted.len(), 12);
    let mut app = headless_app(game::tick_rate(game::REGION));
    // Deliberately reversed plugin order proves scheduling is explicit.
    app.add_plugins((BrakeAssistRulesPlugin, BrakingRulesPlugin, LongitudinalSlipRulesPlugin));
    let car = app
        .world_mut()
        .spawn((
            PedalBrakes,
            PedalTravel::default(),
            WheelBrakeInput::default(),
            WheelBrakeDemand::default(),
            BrakeAssistInput::default(),
            WheelSurfaceMotion::default(),
            WheelLocalVelocities::default(),
            WheelSteering::default(),
            LongitudinalSlip::default(),
        ))
        .id();
    for (group, a) in original.as_chunks::<4>().0.iter().zip(assisted) {
        assert_eq!(group[0]["run_frame"], a["run_frame"]);
        let fraction = |key: &str| a[key] as f32 / 4096.0;
        let (motion, velocity, steering) = inputs(group);
        *app.world_mut().resource_mut::<BrakeAssistTuning>() = BrakeAssistTuning {
            slip_target: [fraction("target_front"), fraction("target_rear")],
            slip_feedback: [fraction("slip_feedback_0"), fraction("slip_feedback_1")],
            grip_feedback: [fraction("grip_feedback_0"), fraction("grip_feedback_1")],
            yaw_target_strength: a["yaw_strength"] as f32 / 100.0,
        };
        *app.world_mut().resource_mut::<BrakingTuning>() = BrakingTuning {
            front_capacity: a["capacity_0"] as f32,
            rear_capacity: a["capacity_1"] as f32,
            handbrake_capacity: a["handbrake_capacity"] as f32,
        };
        app.world_mut().entity_mut(car).insert((
            motion,
            velocity,
            steering,
            PedalTravel { throttle: 0.0, brake: fraction("pedal_0"), handbrake: fraction("handbrake") },
            BrakeAssistInput {
                speed_response: [99.0; 4],
                longitudinal_slip: [99.0; 4], // must be overwritten in this tick
                grip_response: std::array::from_fn(|i| fraction(&format!("grip_{i}"))),
                yaw_shortfall: fraction("yaw_shortfall"),
                lateral_correction: fraction("lateral_correction"),
                positive_yaw: a["yaw"] > 0,
                correction_gain: fraction("correction_gain"),
                correction_threshold: fraction("correction_threshold"),
            },
        ));
        app.run_tick(&[]);
        compare(app.world().get::<LongitudinalSlip>(car).unwrap(), group);
        let brakes = app.world().get::<WheelBrakeInput>(car).unwrap();
        let demand = app.world().get::<WheelBrakeDemand>(car).unwrap();
        let state = app.world().get::<BrakeAssistInput>(car).unwrap();
        for i in 0..4 {
            assert_eq!(state.longitudinal_slip[i] * 4096.0, group[i]["slip"] as f32);
            assert_eq!(
                brakes.travel[i] * 4096.0,
                a[&format!("output_{i}")] as f32,
                "{scenario} frame {} wheel {i}: assisted travel",
                a["run_frame"]
            );
            assert_eq!(demand.ordinary[i], a[&format!("ordinary_{i}")] as f32);
            assert_eq!(demand.total[i], a[&format!("total_{i}")] as f32);
        }
    }
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original fixtures; see docs/assets.md")]
fn fresh_longitudinal_slip_feeds_original_brake_assistance_and_allocation() {
    check_braking("boundaries", "brake", 2);
    check_braking("boundaries", "turn", 3);
    check_braking("holdout", "mixed", 4);
}

#[test]
fn authored_tuning_runs_standalone_and_reads_same_tick_steering() {
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(LongitudinalSlipTuning {
        stationary_wheel_speed_mps: 0.01,
        full_response_speed_mps: 10.0,
        response_per_mps: 0.1,
    });
    app.add_plugins(LongitudinalSlipRulesPlugin);
    app.add_systems(
        FixedUpdate,
        (|mut wheels: Query<&mut WheelSteering>| {
            for mut w in &mut wheels {
                w.angle_turns = [0.0; 4];
            }
        })
        .in_set(VehicleSystems::SteerWheels),
    );
    let car = app
        .world_mut()
        .spawn((
            WheelSurfaceMotion { tread_speed_mps: [12.0, -12.0, 0.0, 12.0], speed_response: [1.0; 4] },
            WheelLocalVelocities { forward_mps: [12.0, -12.0, 12.0, 6.0], lateral_mps: [0.0; 4] },
            WheelSteering { angle_turns: [0.25; 4] },
            LongitudinalSlip::default(),
        ))
        .id();
    app.run_tick(&[]);
    let actual = app.world().get::<LongitudinalSlip>(car).unwrap();
    assert_eq!(actual.rolling_speed_mps, [12.0, -12.0, 12.0, 6.0]);
    assert_eq!(actual.ratio, [0.0, 0.0, 1.0, -0.5]);
    assert_eq!(
        actual.reference,
        [SlipReference::Wheel, SlipReference::Wheel, SlipReference::Ground, SlipReference::Wheel]
    );
}
