//! Original travel/load/body-force records, including retained histories and
//! the next tick's native grip consumer. Road/orientation remain observed.
use bevy::prelude::*;
use engine::testing::{TickApp, headless_app};
use game::vehicle::{VehicleSystems, suspension::*, tyre_grip::*};
use std::collections::{HashMap, HashSet};

type Row = HashMap<String, i32>;
fn rows(name: &str) -> Vec<Row> {
    let text =
        std::fs::read_to_string(format!("{}/tests/data/original/suspension/{name}.csv", env!("CARGO_MANIFEST_DIR")))
            .unwrap();
    let mut lines = text.lines();
    let columns: Vec<_> = lines.next().unwrap().split(',').collect();
    lines
        .map(|l| columns.iter().zip(l.split(',')).map(|(k, v)| (k.to_string(), v.parse().unwrap())).collect())
        .collect()
}
fn check(name: &str, count: usize) {
    let original = rows(name);
    assert_eq!(original.len(), count);
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.add_plugins((SuspensionRulesPlugin, TyreGripRulesPlugin));
    app.configure_sets(FixedUpdate, VehicleSystems::Prepare.before(VehicleSystems::Suspension));
    let standard = app.world().resource::<SuspensionTuning>().clone();
    let car = app
        .world_mut()
        .spawn((
            SuspensionInput::default(),
            Suspension::default(),
            TyreGripInput::default(),
            TyreGrip::default(),
            TyreCondition::default(),
        ))
        .id();
    let mut previous = None;
    let mut carried = 0;
    let mut masks = HashSet::new();
    let mut average_rates = HashSet::new();
    let mut extension = false;
    let mut bump = false;
    let mut zero_load = false;
    let mut positive_damper = false;
    let mut negative_damper = false;
    let mut wrapped_rate = false;
    let mut antiroll = false;
    for r in &original {
        let label = format!("{name} call {}", r["case"]);
        let sample = |phase: &str, key: &str| r[&format!("{phase}_{key}")] as f32;
        let before = |key: &str| sample("before", key);
        let mut tuning = standard.clone();
        for i in 0..2 {
            let axle = &mut tuning.axles[i];
            let v = |key: &str| before(&format!("{key}{i}"));
            axle.bump_start_m = v("bump_start") / 4096.0;
            axle.minimum_travel_m = v("minimum") / 4096.0;
            axle.maximum_travel_m = v("maximum") / 4096.0;
            axle.travel_offset_m = (v("height_offset") - v("radius")) / 4096.0;
            axle.spring_force_per_m = v("spring");
            axle.bump_force_per_m2 = v("bump_gain");
            axle.antiroll_force_per_m2 = v("antiroll");
            axle.preload_force = v("preload");
            for (damper, key) in [(&mut axle.rebound, "rebound"), (&mut axle.compression, "compression")] {
                damper.threshold_mps = v(&format!("{key}_threshold")) / 4096.0;
                damper.base_force = v(&format!("{key}_base"));
                damper.square_root_response = v(&format!("{key}_gain"));
            }
        }
        tuning.lateral_lever_m = before("lateral_lever") / 4096.0;
        tuning.forward_lever_m = before("forward_lever") / 4096.0;
        let four =
            |phase: &str, key: &str, scale: f32| std::array::from_fn(|i| sample(phase, &format!("{key}{i}")) / scale);
        let mut state = Suspension {
            travel_m: four("before", "travel", 4096.0),
            travel_rate_mps: four("before", "rate", 4096.0),
            tyre_load: four("before", "load", 1.0).map(f64::from),
            ..default()
        };
        let mut grip = TyreGripInput {
            load: four("grip_before", "load", 1.0),
            surface: four("grip_before", "surface", 1.0).map(|v| v as usize),
            roll_difference_turns: (sample("grip_before", "roll") - sample("grip_before", "contact_roll")) / 4096.0,
            ..default()
        };
        if previous == Some(r["sequence"]) {
            let old = *app.world().get::<Suspension>(car).unwrap();
            assert_eq!(old.travel_m, state.travel_m, "{label}: retained travel");
            assert_eq!(old.travel_rate_mps, state.travel_rate_mps, "{label}: retained rate");
            assert_eq!(old.tyre_load, state.tyre_load, "{label}: retained load");
            let carried_grip = app.world().get::<TyreGripInput>(car).unwrap().load;
            assert_eq!(carried_grip, grip.load, "{label}: suspension load feeds next grip tick");
            state = old;
            grip.load = carried_grip;
            carried += 1;
        }
        let input = SuspensionInput {
            body_height_m: before("height") / 4096.0,
            road_height_m: four("before", "road_height", 4096.0),
            attachment_height_m: four("before", "attachment", 4096.0),
            road_offset_m: four("before", "road_offset", 4096.0),
            road_normal_up: before("normal_up") / 4096.0,
            body_up: before("body_up") / 4096.0,
            inverse_step_seconds: before("inverse_duration") / 4096.0,
            axle_force: [before("front_force"), before("rear_force")],
            lateral_acceleration_mps2: before("lateral_force") / 4096.0,
            vertical_force: before("vertical_force"),
            forward_force: before("forward_force"),
            disabled: before("mode") == 7.0,
        };
        for i in 0..4 {
            let travel = r[&format!("requested_travel{i}")] as f32 / 4096.0;
            let axle = &tuning.axles[i / 2];
            extension |= travel > axle.maximum_travel_m;
            bump |= travel.max(axle.minimum_travel_m) < axle.bump_start_m;
            let rate = (travel - state.travel_m[i]) * input.inverse_step_seconds;
            average_rates.insert(((rate + state.travel_rate_mps[i]) * 4096.0 / 2.0).trunc() as i32);
            wrapped_rate |= rate.abs() >= 8.0 && travel <= axle.maximum_travel_m;
        }
        app.world_mut().entity_mut(car).insert((state, input, SuspensionSetup(tuning), grip));
        app.run_tick(&[]);
        let actual = app.world().get::<Suspension>(car).unwrap();
        for (key, values, scale) in [
            ("travel", actual.travel_m.map(f64::from), 4096.0),
            ("rate", actual.travel_rate_mps.map(f64::from), 4096.0),
            ("spring_force", actual.spring_force, 1.0),
            ("damper_force", actual.damper_force, 1.0),
            ("load", actual.tyre_load, 1.0),
        ] {
            for i in 0..4 {
                assert_eq!(values[i] * scale, r[&format!("after_{key}{i}")] as f64, "{label} {key} wheel {i}");
            }
        }
        assert_eq!(actual.compressed_mask, r["after_contact_mask"] as u8, "{label}: compression mask");
        for i in 0..4 {
            assert_eq!(
                actual.requested_travel_m[i] * 4096.0,
                r[&format!("requested_travel{i}")] as f32,
                "{label}: raw travel {i}"
            );
            assert_eq!(actual.body_force[i], r[&format!("body_force{i}")] as f64, "{label}: body force {i}");
        }
        assert_eq!(app.world().get::<TyreGripInput>(car).unwrap().load, actual.tyre_load.map(|v| v as f32));
        let capacity = app.world().get::<TyreGrip>(car).unwrap().capacity;
        assert_eq!(capacity, four("grip_after", "capacity", 1.0), "{label}: next-tick native grip consumer");
        masks.insert(actual.compressed_mask);
        zero_load |= actual.tyre_load.contains(&0.0);
        positive_damper |= actual.damper_force.iter().any(|v| *v > 0.0);
        negative_damper |= actual.damper_force.iter().any(|v| *v < 0.0);
        antiroll |= actual.travel_m[0] != actual.travel_m[1] || actual.travel_m[2] != actual.travel_m[3];
        previous = Some(r["sequence"]);
    }
    assert!(carried >= 39);
    assert!(positive_damper && negative_damper && antiroll);
    if name == "boundaries" {
        assert!(extension && bump && zero_load && wrapped_rate);
        assert!(masks.contains(&0) && masks.contains(&15));
        for rate in [-4097, -4096, -4095, 0, 4095, 4096, 4097] {
            assert!(average_rates.contains(&rate), "missing unclamped mean-rate probe {rate}");
        }
    }
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original suspension fixtures; see docs/assets.md")]
fn original_suspension_travel_load_body_forces_and_next_tick_grip() {
    check("boundaries", 166);
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original suspension holdout; see docs/assets.md")]
fn independent_original_suspension_holdout() {
    check("holdout", 55);
}

fn authored_tuning() -> SuspensionTuning {
    let axle = SuspensionAxleTuning {
        bump_start_m: -0.5,
        minimum_travel_m: -1.0,
        maximum_travel_m: 0.5,
        travel_offset_m: 0.0,
        spring_force_per_m: 16.0,
        bump_force_per_m2: 8.0,
        antiroll_force_per_m2: 16.0,
        preload_force: 4.0,
        rebound: DamperTuning { threshold_mps: 0.0, base_force: 0.0, square_root_response: 0.0 },
        compression: DamperTuning { threshold_mps: 0.0, base_force: 0.0, square_root_response: 0.0 },
    };
    SuspensionTuning { axles: [axle.clone(), axle], lateral_lever_m: 0.5, forward_lever_m: 0.25 }
}

#[test]
fn handmade_suspension_runs_alone_with_per_car_setup_and_optional_grip_bridge() {
    let mut app = headless_app(game::tick_rate(game::REGION));
    let tuning = authored_tuning();
    app.insert_resource(tuning.clone()).add_plugins(SuspensionRulesPlugin);
    let input = SuspensionInput {
        body_height_m: -0.5,
        road_normal_up: 1.0,
        body_up: 1.0,
        inverse_step_seconds: 1.0,
        attachment_height_m: [-0.25, 0.25, 0.5, 1.25],
        axle_force: [2.0, -2.0],
        lateral_acceleration_mps2: 4.0 / 4096.0,
        vertical_force: 4.0,
        forward_force: 12.0,
        ..default()
    };
    let car = app.world_mut().spawn((input, Suspension::default(), TyreGripInput::default())).id();
    let mut variant = tuning.clone();
    variant.axles[0].spring_force_per_m = 32.0;
    let other = app.world_mut().spawn((input, Suspension::default(), SuspensionSetup(variant))).id();
    let transferred = app
        .world_mut()
        .spawn((
            input,
            Suspension::default(),
            SuspensionForceTransfer,
            game::vehicle::forward_dynamics::ForwardDynamics { force: 20.0, ..default() },
            game::vehicle::body_dynamics::BodyDynamics { lateral_acceleration_mps2: 0.0, ..default() },
        ))
        .id();
    app.run_tick(&[]);
    assert_eq!(app.world().get::<Suspension>(transferred).unwrap().body_force, [19.0, 3.0, 1.0, -11.0]);
    assert_eq!(app.world().get::<SuspensionInput>(transferred).unwrap().forward_force, 20.0);
    let state = *app.world().get::<Suspension>(car).unwrap();
    assert_eq!(state.travel_m, [-0.75, -0.25, 0.0, 0.5]);
    assert_eq!(state.spring_force, [12.0, 4.0, 0.0, -4.0]);
    assert_eq!(state.tyre_load, [16.0, 8.0, 4.0, 0.0]);
    assert_eq!(state.body_force, [15.0, 3.0, 1.0, -7.0]);
    assert_eq!(app.world().get::<TyreGripInput>(car).unwrap().load, [16.0, 8.0, 4.0, 0.0]);
    assert_eq!(app.world().get::<Suspension>(other).unwrap().tyre_load, [28.0, 12.0, 4.0, 0.0]);
    app.world_mut().get_mut::<SuspensionInput>(car).unwrap().disabled = true;
    app.run_tick(&[]);
    assert_eq!(app.world().get::<Suspension>(car).unwrap().body_force, state.body_force);
    assert_eq!(app.world().get::<Suspension>(car).unwrap().travel_rate_mps, state.travel_rate_mps);
}
