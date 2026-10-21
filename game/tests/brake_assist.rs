//! Exact original consumer recordings, including pedal -> assist -> demand.
use bevy::prelude::*;
use engine::prelude::{Region, TickRate};
use engine::testing::{TickApp, headless_app};
use game::vehicle::PedalTravel;
use game::vehicle::brake_assist::{BrakeAssistInput, BrakeAssistRulesPlugin, BrakeAssistTuning};
use game::vehicle::braking::{BrakingRulesPlugin, BrakingTuning, PedalBrakes, WheelBrakeDemand, WheelBrakeInput};
use std::collections::HashMap;

type Row = HashMap<String, i32>;

fn rows(name: &str) -> Vec<Row> {
    let path = format!("{}/tests/data/original/brake_assist/{name}.csv", env!("CARGO_MANIFEST_DIR"));
    let csv = std::fs::read_to_string(path).unwrap();
    let mut lines = csv.lines();
    let columns: Vec<_> = lines.next().unwrap().split(',').collect();
    lines
        .map(|line| columns.iter().zip(line.split(',')).map(|(k, v)| (k.to_string(), v.parse().unwrap())).collect())
        .collect()
}

fn tuning(row: &Row) -> BrakeAssistTuning {
    let fraction = |field: &str| row[field] as f32 / 4096.0;
    BrakeAssistTuning {
        slip_target: [fraction("target_front"), fraction("target_rear")],
        slip_feedback: [fraction("slip_feedback_0"), fraction("slip_feedback_1")],
        grip_feedback: [fraction("grip_feedback_0"), fraction("grip_feedback_1")],
        yaw_target_strength: row["yaw_strength"] as f32 / 100.0,
    }
}

fn state(row: &Row) -> BrakeAssistInput {
    let fraction = |field: &str| row[field] as f32 / 4096.0;
    BrakeAssistInput {
        speed_response: std::array::from_fn(|i| fraction(&format!("speed_{i}"))),
        grip_response: std::array::from_fn(|i| fraction(&format!("grip_{i}"))),
        longitudinal_slip: std::array::from_fn(|i| fraction(&format!("slip_{i}"))),
        yaw_shortfall: fraction("yaw_shortfall"),
        lateral_correction: fraction("lateral_correction"),
        positive_yaw: row["yaw"] > 0,
        correction_gain: fraction("correction_gain"),
        correction_threshold: fraction("correction_threshold"),
    }
}

fn check(name: &str) -> usize {
    let rows = rows(name);
    assert!(!rows.is_empty());
    let mut app = headless_app(TickRate::every_nth_field(Region::Ntsc, 2));
    // Add in reverse order: set ordering, rather than plugin registration
    // order, must enforce pedal copy -> assistance -> demand allocation.
    app.add_plugins((BrakeAssistRulesPlugin, BrakingRulesPlugin));
    let car = app
        .world_mut()
        .spawn((
            PedalBrakes,
            PedalTravel::default(),
            BrakeAssistInput::default(),
            WheelBrakeInput::default(),
            WheelBrakeDemand::default(),
        ))
        .id();
    // The existing replay/AI channel API stays usable without assistance.
    let independent = app
        .world_mut()
        .spawn((WheelBrakeInput { travel: [1.0, 0.5, 0.25, 0.0], handbrake: 0.0 }, WheelBrakeDemand::default()))
        .id();
    let mut minimum = false;
    let mut shortfall = false;
    let mut correction = false;
    let mut amplified = false;
    for (tick, row) in rows.iter().enumerate() {
        assert_eq!(row["run_frame"], tick as i32 * 2);
        for i in 0..4 {
            assert_eq!(row[&format!("pedal_{i}")], row["pedal_0"]);
            assert_eq!(row[&format!("input_{i}")], row["pedal_0"]);
        }
        *app.world_mut().get_mut::<PedalTravel>(car).unwrap() = PedalTravel {
            brake: row["pedal_0"] as f32 / 4096.0,
            handbrake: row["handbrake"] as f32 / 4096.0,
            throttle: 0.0,
        };
        *app.world_mut().get_mut::<BrakeAssistInput>(car).unwrap() = state(row);
        *app.world_mut().resource_mut::<BrakeAssistTuning>() = tuning(row);
        let allocation = BrakingTuning {
            front_capacity: row["capacity_0"] as f32,
            rear_capacity: row["capacity_1"] as f32,
            handbrake_capacity: row["handbrake_capacity"] as f32,
        };
        *app.world_mut().resource_mut::<BrakingTuning>() = allocation;
        app.run_tick(&[]);
        let actual = app.world().get::<WheelBrakeInput>(car).unwrap();
        let demand = app.world().get::<WheelBrakeDemand>(car).unwrap();
        for i in 0..4 {
            assert_eq!(actual.travel[i] * 4096.0, row[&format!("output_{i}")] as f32, "{name} tick {tick} wheel {i}");
            assert_eq!(
                demand.ordinary[i],
                row[&format!("ordinary_{i}")] as f32,
                "{name} tick {tick} wheel {i} ordinary"
            );
            assert_eq!(demand.total[i], row[&format!("total_{i}")] as f32, "{name} tick {tick} wheel {i} total");
            minimum |= row[&format!("output_{i}")] == 1 && row[&format!("before_attenuation_{i}")] != 0;
            amplified |= row[&format!("before_attenuation_{i}")] > row[&format!("input_{i}")];
        }
        assert_eq!(actual.handbrake * 4096.0, row["handbrake"] as f32);
        shortfall |= row["yaw_shortfall"] > 0;
        correction |= row["lateral_correction"] > 0;
        let independent_input = *app.world().get::<WheelBrakeInput>(independent).unwrap();
        assert_eq!(independent_input.travel, [1.0, 0.5, 0.25, 0.0]);
        assert_eq!(
            app.world().get::<WheelBrakeDemand>(independent).unwrap().total,
            allocation.allocate(independent_input).total
        );
    }
    if name == "zero_factor" {
        assert!(minimum);
    }
    if matches!(name, "left_yaw" | "right_yaw" | "yaw_holdout") {
        assert!(shortfall && amplified);
    }
    if name == "zero_strength_holdout" {
        assert!(correction);
    }
    rows.len()
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn original_braking_and_controlled_assistance_branches() {
    let count: usize =
        ["brake", "combined", "left_yaw", "right_yaw", "upper_targets", "zero_factor"].into_iter().map(check).sum();
    assert_eq!(count, 78);
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn independent_original_assistance_holdouts() {
    let count: usize =
        ["mixed_holdout", "yaw_holdout", "zero_strength_holdout", "feedback_holdout"].into_iter().map(check).sum();
    assert_eq!(count, 52);
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn assistance_feature_runs_without_the_other_vehicle_features() {
    let original = rows("left_yaw");
    let mut app = headless_app(TickRate::every_nth_field(Region::Ntsc, 2));
    app.add_plugins(BrakeAssistRulesPlugin);
    let car = app.world_mut().spawn((WheelBrakeInput::default(), BrakeAssistInput::default())).id();
    // This also verifies the shipped tuning against every original parameter
    // in a recording without parameter interventions (yaw control aside).
    let asset = app.world().resource::<BrakeAssistTuning>().clone();
    for row in &original {
        assert_eq!(asset.slip_target, tuning(row).slip_target);
        assert_eq!(asset.slip_feedback, tuning(row).slip_feedback);
        assert_eq!(asset.grip_feedback, tuning(row).grip_feedback);
        assert_eq!(asset.yaw_target_strength, tuning(row).yaw_target_strength);
        *app.world_mut().get_mut::<WheelBrakeInput>(car).unwrap() = WheelBrakeInput {
            travel: std::array::from_fn(|i| row[&format!("input_{i}")] as f32 / 4096.0),
            handbrake: row["handbrake"] as f32 / 4096.0,
        };
        *app.world_mut().get_mut::<BrakeAssistInput>(car).unwrap() = state(row);
        app.run_tick(&[]);
        let actual = app.world().get::<WheelBrakeInput>(car).unwrap();
        for i in 0..4 {
            assert_eq!(actual.travel[i] * 4096.0, row[&format!("output_{i}")] as f32);
        }
    }
}
