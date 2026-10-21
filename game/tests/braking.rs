//! Original per-wheel brake-preparation observations, without a tyre model.
use bevy::prelude::*;
use engine::prelude::{Region, TickRate};
use engine::testing::{TickApp, headless_app};
use game::vehicle::PedalTravel;
use game::vehicle::braking::{BrakingRulesPlugin, BrakingTuning, PedalBrakes, WheelBrakeDemand, WheelBrakeInput};
use std::collections::HashMap;

fn rows(name: &str) -> Vec<HashMap<String, i32>> {
    let path = format!("{}/tests/data/original/braking/{name}.ticks.csv", env!("CARGO_MANIFEST_DIR"));
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
    app.add_plugins(BrakingRulesPlugin);
    let car = app.world_mut().spawn((WheelBrakeInput::default(), WheelBrakeDemand::default())).id();
    let ordinary_driver = name != "wheel_holdout";
    // Check initial pedal copying separately from the allocator, which
    // consumes measured channels after the original's unported assistance.
    let pedal_copy = app.world_mut().spawn((PedalBrakes, PedalTravel::default(), WheelBrakeInput::default())).id();
    let mut fractional = false;
    let mut asymmetric = false;
    let mut negative_alias = false;
    for (tick, row) in original.iter().enumerate() {
        assert_eq!(row["run_frame"], tick as i32 * 2, "{name}: physics cadence");
        let input = WheelBrakeInput {
            travel: std::array::from_fn(|i| row[&format!("travel_{i}")] as f32 / 4096.0),
            handbrake: row["handbrake_travel"] as f32 / 4096.0,
        };
        if ordinary_driver {
            let copied = row["copied_brake_0"];
            assert!(copied >= 0);
            for wheel in 1..4 {
                assert_eq!(row[&format!("copied_brake_{wheel}")], copied, "initial pedal copy shares one channel");
            }
            *app.world_mut().get_mut::<PedalTravel>(pedal_copy).unwrap() =
                PedalTravel { throttle: 0.0, brake: copied as f32 / 4096.0, handbrake: input.handbrake };
        } else {
            assert!((0..4).all(|wheel| row[&format!("copied_brake_{wheel}")] == -1));
        }
        *app.world_mut().get_mut::<WheelBrakeInput>(car).unwrap() = input;
        {
            let mut tuning = app.world_mut().resource_mut::<BrakingTuning>();
            assert_eq!(tuning.front_capacity, row["capacity_0"] as f32);
            assert_eq!(tuning.rear_capacity, row["capacity_1"] as f32);
            if name == "handbrake_alias_holdout" {
                tuning.handbrake_capacity = row["handbrake_capacity"] as f32;
            } else {
                assert_eq!(tuning.handbrake_capacity, row["handbrake_capacity"] as f32);
            }
        }
        app.run_tick(&[]);
        if ordinary_driver {
            let copied = *app.world().get::<WheelBrakeInput>(pedal_copy).unwrap();
            assert_eq!(copied.travel, [row["copied_brake_0"] as f32 / 4096.0; 4]);
            assert_eq!(copied.handbrake, input.handbrake);
        }
        let actual = *app.world().get::<WheelBrakeDemand>(car).unwrap();
        for wheel in 0..4 {
            assert_eq!(
                actual.ordinary[wheel],
                row[&format!("ordinary_{wheel}")] as f32,
                "{name} tick {tick} wheel {wheel}: ordinary"
            );
            assert_eq!(
                actual.total[wheel],
                row[&format!("total_{wheel}")] as f32,
                "{name} tick {tick} wheel {wheel}: total"
            );
            let flags = row[&format!("initial_flags_{wheel}")];
            assert_eq!(actual.is_braking(wheel), flags & 2 != 0, "{name} tick {tick} wheel {wheel}: braking state");
            // Bounded evidence for the other initial bit, kept out of native
            // braking until the wheel-load/contact model itself is ported.
            assert_eq!(flags & 1 != 0, row[&format!("wheel_load_{wheel}")] == 0);
            assert_eq!(flags & !3, 0, "later drivetrain flags are outside this preparation phase");
        }
        fractional |= input.travel.iter().chain([&input.handbrake]).any(|&travel| travel > 0.0 && travel < 1.0);
        asymmetric |= input.travel.iter().any(|&travel| travel != input.travel[0]);
        negative_alias |= actual.total[2] < 0.0;
    }
    if name == "wheel_holdout" {
        assert!(fractional && asymmetric, "independent per-wheel and handbrake inputs must be exercised");
    }
    if name == "handbrake_alias_holdout" {
        assert!(negative_alias, "the original signed handbrake alias must be observed");
    }
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn original_service_handbrake_and_combined_demands() {
    for scenario in ["brake", "handbrake", "combined"] {
        check_original(scenario);
    }
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn independent_original_combination_and_per_wheel_holdouts() {
    check_original("mixed_holdout");
    check_original("wheel_holdout");
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn original_signed_handbrake_boundary_holdout() {
    check_original("handbrake_alias_holdout");
}
