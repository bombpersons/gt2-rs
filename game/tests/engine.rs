//! Compare the engine subsystem with original instruction-boundary observations.
//! Inputs are measured drivetrain speed, last integer RPM and effective throttle;
//! this deliberately does not claim to reproduce the unported drivetrain.
use bevy::prelude::*;
use engine::prelude::{Region, TickRate};
use engine::testing::{TickApp, headless_app};
use game::vehicle::engine_torque::{
    EngineFuelCut, EngineTorque, EngineTorqueInput, EngineTorqueRulesPlugin, EngineTorqueTuning,
};
use std::collections::HashMap;

fn rows(name: &str) -> Vec<HashMap<String, i32>> {
    let path = format!("{}/tests/data/original/engine/{name}.ticks.csv", env!("CARGO_MANIFEST_DIR"));
    let csv = std::fs::read_to_string(path).unwrap();
    let mut lines = csv.lines();
    let columns: Vec<_> = lines.next().unwrap().split(',').collect();
    lines
        .map(|line| {
            columns.iter().zip(line.split(',')).map(|(key, value)| (key.to_string(), value.parse().unwrap())).collect()
        })
        .collect()
}

fn check_original(name: &str) {
    let original = rows(name);
    assert!(!original.is_empty());
    let mut app = headless_app(TickRate::every_nth_field(Region::Ntsc, 2));
    app.add_plugins(EngineTorqueRulesPlugin);
    let car = app
        .world_mut()
        .spawn((
            EngineTorqueInput::default(),
            EngineFuelCut { active: original[0]["before_cut"] != 0 },
            EngineTorque::default(),
        ))
        .id();
    let mut cut_changes = 0;
    let mut positive_torque = 0;
    let mut cut_with_throttle = 0;
    for (tick, row) in original.iter().enumerate() {
        assert_eq!(row["run_frame"], tick as i32 * 2, "{name}: physics cadence");
        let before = app.world().get::<EngineFuelCut>(car).unwrap().active;
        assert_eq!(before, row["before_cut"] != 0, "{name} tick {tick}: persistent latch");
        *app.world_mut().get_mut::<EngineTorqueInput>(car).unwrap() = EngineTorqueInput {
            speed_rev_s: row["curve_speed_q12"] as f32 / 4096.0,
            integer_rpm: row["limiter_rpm"] as f32,
            effective_throttle: row["throttle_q12"] as f32 / 4096.0,
        };
        app.run_tick(&[]);
        let actual = *app.world().get::<EngineTorque>(car).unwrap();
        assert_eq!(actual.full_throttle, row["full_torque"] as f32, "{name} tick {tick}: curve");
        assert_eq!(actual.drag, row["drag"] as f32, "{name} tick {tick}: engine drag");
        assert_eq!(actual.net, row["net_torque"] as f32, "{name} tick {tick}: net torque");
        let after = app.world().get::<EngineFuelCut>(car).unwrap().active;
        assert_eq!(after, row["after_cut"] != 0, "{name} tick {tick}: fuel cut");
        let tuning = app.world().resource::<EngineTorqueTuning>();
        assert_eq!(
            tuning.refresh_rpm(row["refresh_speed_q12"] as f32 / 4096.0),
            row["refreshed_rpm"] as f32,
            "{name} tick {tick}: integer RPM refresh"
        );
        cut_changes += usize::from(before != after);
        positive_torque += usize::from(actual.net > 0.0);
        cut_with_throttle += usize::from(after && row["throttle_q12"] > 0);
    }
    if name.starts_with("limiter") {
        assert!(cut_changes >= 3, "{name}: exercise stateful cut and recovery");
        assert!(cut_with_throttle > 0, "{name}: cut must suppress positive combustion");
        assert!(positive_torque > 0, "{name}: exercise full-throttle recovery");
    }
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn original_acceleration_coasting_and_braking_engine_ticks() {
    for scenario in ["gas", "coast", "brake"] {
        check_original(scenario);
    }
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn original_stateful_redline_cut_and_strict_release_threshold() {
    check_original("limiter");
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn independent_original_holdouts_after_implementation() {
    check_original("limiter_holdout");
    check_original("mixed_holdout");
    check_original("curve_holdout");
}
