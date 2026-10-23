//! Original-execution comparisons for tilt, vertical force and compression
//! correction, both isolated and following the native suspension producer.
use bevy::prelude::*;
use engine::testing::{TickApp, headless_app};
use game::vehicle::{
    VehiclePose, VehicleStepDuration, VehicleVelocity, WheelSteering,
    aerodynamics::*,
    body_dynamics::{BodyDynamics, BodyDynamicsInput},
    forward_dynamics::*,
    road_contact::{BodyOrientation, RoadContactState, VehicleBasis},
    suspension::*,
    tyre_forces::{TyreContactState, TyreForces},
    vertical_body::*,
};
use std::collections::{HashMap, HashSet};
pub type Row = HashMap<String, i32>;
pub fn check_rows(original: Vec<Row>, name: &str, connected: bool, count: usize, aero: bool) {
    assert_eq!(original.len(), count);
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.add_plugins(VerticalBodyRulesPlugin);
    if aero {
        app.add_plugins((ForwardDynamicsRulesPlugin, AerodynamicsRulesPlugin));
    }
    if connected {
        app.add_plugins(SuspensionRulesPlugin);
    }
    let standard = app.world().resource::<VerticalBodyTuning>().clone();
    let spring_standard = if connected { Some(app.world().resource::<SuspensionTuning>().clone()) } else { None };
    let car = app.world_mut().spawn_empty().id();
    let mut previous = None;
    let mut carried = 0;
    let mut masks = HashSet::new();
    let mut airborne = false;
    let mut resets = false;
    let mut lift = false;
    let mut capped = false;
    for r in original {
        let value = |phase: &str, key: &str| r[&format!("{phase}_{key}")] as f32;
        let before = |key: &str| value("before", key);
        let four =
            |phase: &str, key: &str, scale: f32| std::array::from_fn(|i| value(phase, &format!("{key}{i}")) / scale);
        let vector = |phase: &str, key: &str| {
            Vec3::new(
                value(phase, &format!("{key}0")),
                value(phase, &format!("{key}2")),
                -value(phase, &format!("{key}1")),
            ) / 4096.0
        };
        let mut tuning = standard.clone();
        tuning.inverse_mass = before("inverse_mass") / 4096.0;
        tuning.pitch_response = [before("pitch_response0"), before("pitch_response1")].map(|v| v / 4096.0 / 65536.0);
        tuning.roll_response = [before("roll_response0"), before("roll_response1")].map(|v| v / 4096.0 / 65536.0);
        tuning.minimum_travel_m = [before("axle_minimum0"), before("axle_minimum1")].map(|v| v / 4096.0);
        tuning.travel_offset_m = std::array::from_fn(|i| {
            (before(&format!("axle_height_offset{i}")) - before(&format!("radius{i}"))) / 4096.0
        });
        let phase = if connected { "suspension" } else { "before" };
        let contact = SuspensionInput {
            body_height_m: value(phase, "height") / 4096.0,
            road_height_m: four(phase, "road", 4096.0),
            attachment_height_m: four(phase, "attachment", 4096.0),
            road_offset_m: four(phase, "offset", 4096.0),
            road_normal_up: value(phase, "normal_up") / 4096.0,
            body_up: value(phase, "body_up") / 4096.0,
            inverse_step_seconds: value(phase, "inverse_duration") / 4096.0,
            axle_force: if aero { [-999.0; 2] } else { [value(phase, "front_force"), value(phase, "rear_force")] },
            lateral_force: value(phase, "lateral_force"),
            vertical_force: value(phase, "vertical_force"),
            forward_force: value(phase, "forward_force"),
            disabled: value(phase, "mode") == 7.0,
        };
        let mut suspension = Suspension {
            travel_m: four(phase, "travel", 4096.0),
            travel_rate_mps: four(phase, "rate", 4096.0),
            spring_force: four(phase, "spring", 1.0).map(f64::from),
            damper_force: four(phase, "damper", 1.0).map(f64::from),
            tyre_load: four(phase, "load", 1.0).map(f64::from),
            body_force: std::array::from_fn(|i| r[&format!("body_force{i}")] as f64),
            compressed_mask: value(phase, "mask") as u8,
            ..default()
        };
        let mut orientation =
            BodyOrientation { pitch_turns: before("pitch") / 4096.0, roll_turns: before("roll") / 4096.0 };
        let mut dynamics = BodyDynamics {
            tilt_turns_per_second: Vec2::new(before("pitch_rate"), before("roll_rate")) / 65536.0,
            ..default()
        };
        let mut state = VerticalBody { airborne: before("airborne") != 0.0, ..default() };
        if previous == Some(r["sequence"]) {
            let old_angles = *app.world().get::<BodyOrientation>(car).unwrap();
            let old_rates = app.world().get::<BodyDynamics>(car).unwrap().tilt_turns_per_second;
            assert_eq!(old_angles.pitch_turns, orientation.pitch_turns, "retained pitch");
            assert_eq!(old_angles.roll_turns, orientation.roll_turns, "retained roll");
            assert_eq!(old_rates, dynamics.tilt_turns_per_second, "retained tilt velocity");
            assert_eq!(app.world().get::<VerticalBody>(car).unwrap().airborne, state.airborne);
            orientation = old_angles;
            dynamics.tilt_turns_per_second = old_rates;
            state = *app.world().get::<VerticalBody>(car).unwrap();
            if connected {
                let old = *app.world().get::<Suspension>(car).unwrap();
                assert_eq!(old.travel_m, suspension.travel_m);
                assert_eq!(old.travel_rate_mps, suspension.travel_rate_mps);
                suspension = old;
            }
            carried += 1;
        }
        let road = RoadContactState {
            height_m: four("before", "road", 4096.0),
            road_offset_m: four("before", "offset", 4096.0),
            attachment_m: four("before", "attachment", 4096.0).map(|v| Vec3::Y * v),
            road_height_m: before("road_height") / 4096.0,
            road_basis: VehicleBasis {
                forward: vector("before", "road_forward"),
                lateral: vector("before", "road_side"),
                up: Vec3::ZERO,
            },
            body_basis: VehicleBasis {
                forward: vector("before", "body_forward"),
                lateral: vector("before", "body_side"),
                up: vector("before", "body_normal"),
            },
            ..default()
        };
        app.world_mut().entity_mut(car).insert((
            VerticalBodySetup(tuning),
            contact,
            suspension,
            orientation,
            dynamics,
            state,
            VerticalBodyInput {
                reset_ticks: before("reset_ticks") as u16,
                ticks_per_second: before("tick_rate") as u32,
                reset_heading_turns: before("reset_heading") / 4096.0,
            },
            VehiclePose {
                position_m: Vec3::Y * (before("height") / 4096.0),
                heading_turns: before("heading") / 4096.0,
            },
            VehicleVelocity { linear_mps: vector("before", "velocity"), ..default() },
            VehicleStepDuration(before("duration") / 65536.0),
            road,
            BodyDynamicsInput::default(),
        ));
        if let Some(mut t) = spring_standard.clone() {
            for i in 0..2 {
                let v = |k: &str| value("suspension", &format!("axle_{k}{i}"));
                let a = &mut t.axles[i];
                a.bump_start_m = v("bump_start") / 4096.0;
                a.minimum_travel_m = v("minimum") / 4096.0;
                a.maximum_travel_m = v("maximum") / 4096.0;
                a.travel_offset_m = (v("height_offset") - before(&format!("radius{i}"))) / 4096.0;
                a.spring_force_per_m = v("spring");
                a.bump_force_per_m2 = v("bump_gain");
                a.antiroll_force_per_m2 = v("antiroll");
                a.preload_force = v("preload");
                for (d, k) in [(&mut a.rebound, "rebound"), (&mut a.compression, "compression")] {
                    d.threshold_mps = v(&format!("{k}_threshold")) / 4096.0;
                    d.base_force = v(&format!("{k}_base"));
                    d.square_root_response = v(&format!("{k}_gain"));
                }
            }
            app.world_mut().entity_mut(car).insert(SuspensionSetup(t));
        }
        if aero {
            let mut t = app.world().resource::<AerodynamicsTuning>().clone();
            t.drag_force_per_squared_mps = value("aero_before", "drag_coefficient") / 1024.0;
            t.axle_force_per_squared_mps =
                [value("aero_before", "front_coefficient"), value("aero_before", "rear_coefficient")]
                    .map(|v| v / 1024.0);
            t.minimum_response = value("aero_before", "minimum_response") / 4096.0;
            let mut state = Aerodynamics { reduction: value("aero_before", "reduction") / 4096.0, ..default() };
            if previous == Some(r["sequence"]) {
                let old = *app.world().get::<Aerodynamics>(car).unwrap();
                assert_eq!(old.reduction, state.reduction, "retained aerodynamic reduction");
                state = old;
            }
            assert_eq!(
                value("aero_before", "speed"),
                value("forward_before", "speed"),
                "shared retained contact speed"
            );
            app.world_mut().entity_mut(car).insert((
                AerodynamicsSetup(t),
                state,
                AerodynamicsInput {
                    forward_mps: f32::NAN,
                    reduction_growth_per_second: value("aero_before", "growth") / 4096.0,
                },
                TyreContactState { body_forward_mps: value("aero_before", "speed") / 4096.0, ..default() },
                ForwardDynamicsInput {
                    wheel_load: four("forward_before", "load", 1.0),
                    surface: four("forward_before", "surface", 1.0).map(|v| v as usize),
                    aerodynamic_force: -999.0,
                },
                ForwardDynamics::default(),
                WheelSteering { angle_turns: four("forward_before", "steering", 4096.0) },
                TyreForces {
                    wheel_reference_force: four("forward_before", "wheel_force", 1.0),
                    ground_reference_force: four("forward_before", "ground_force", 1.0),
                    lateral_force: four("forward_before", "lateral_force", 1.0),
                    ..default()
                },
            ));
        }
        app.run_tick(&[]);
        let pose = app.world().get::<VehiclePose>(car).unwrap();
        let angles = app.world().get::<BodyOrientation>(car).unwrap();
        let rates = app.world().get::<BodyDynamics>(car).unwrap().tilt_turns_per_second;
        let road = app.world().get::<RoadContactState>(car).unwrap();
        let velocity = app.world().get::<VehicleVelocity>(car).unwrap().linear_mps;
        let state = app.world().get::<VerticalBody>(car).unwrap();
        let suspension = app.world().get::<Suspension>(car).unwrap();
        let label = format!("{name} case {} connected {connected}", r["case"]);
        if aero {
            let state = app.world().get::<Aerodynamics>(car).unwrap();
            for (key, actual) in [
                ("reduction", state.reduction * 4096.0),
                ("drag", state.drag_force),
                ("axle0", state.axle_force[0]),
                ("axle1", state.axle_force[1]),
            ] {
                assert_eq!(actual, value("aero_after", key), "{label}: aero {key}");
            }
            for (key, actual) in [
                ("square", state.squared_speed_mps2 * 4.0),
                ("response", state.response * 4096.0),
                ("effective", state.effective_squared_speed_mps2 * 4.0),
            ] {
                assert_eq!(actual, r[&format!("aero_{key}")] as f32, "{label}: aero intermediate {key}");
            }
            let forward = app.world().get::<ForwardDynamics>(car).unwrap();
            for (key, actual) in [
                ("speed", forward.forward_mps * 4096.0),
                ("force", forward.force),
                ("acceleration", forward.acceleration_mps2 * 4096.0),
            ] {
                assert_eq!(actual, value("forward_after", key), "{label}: native aero -> forward {key}");
            }
            for i in 0..4 {
                assert_eq!(
                    suspension.body_force[i],
                    r[&format!("body_force{i}")] as f64,
                    "{label}: native aero -> suspension force {i}"
                );
            }
        }
        for (key, actual, scale) in [
            ("height", pose.position_m.y, 4096.0),
            ("pitch", angles.pitch_turns, 4096.0),
            ("roll", angles.roll_turns, 4096.0),
            ("heading", pose.heading_turns, 4096.0),
            ("pitch_rate", rates.x, 65536.0),
            ("roll_rate", rates.y, 65536.0),
            ("velocity0", velocity.x, 4096.0),
            ("velocity1", -velocity.z, 4096.0),
            ("velocity2", velocity.y, 4096.0),
        ] {
            assert_eq!(actual * scale, value("after", key), "{label}: {key}");
        }
        assert_eq!(state.airborne, value("after", "airborne") != 0.0, "{label}: airborne");
        assert_eq!(suspension.compressed_mask, value("after", "mask") as u8, "{label}: mask");
        assert_eq!(app.world().get::<BodyDynamicsInput>(car).unwrap().bypass_tyre_acceleration, state.airborne);
        if before("mode") != 7.0 {
            for (key, v) in [
                ("body_forward", road.body_basis.forward),
                ("body_side", road.body_basis.lateral),
                ("body_normal", road.body_basis.up),
            ] {
                assert_eq!(v, vector("integrated", key), "{label}: integrated {key}");
            }
        }
        for i in 0..4 {
            assert_eq!(
                road.attachment_m[i].y * 4096.0,
                value("after", &format!("attachment{i}")),
                "{label}: attachment {i}"
            );
            assert_eq!(
                suspension.travel_m[i] * 4096.0,
                value("after", &format!("travel{i}")),
                "{label}: travel history {i}"
            );
        }
        airborne |= state.airborne;
        resets |= state.reset_applied;
        lift |= state.lift_m != 0.0;
        capped |= angles.pitch_turns.abs() == standard.maximum_tilt_turns;
        masks.insert(suspension.compressed_mask);
        previous = Some(r["sequence"]);
    }
    assert!(carried >= 40);
    if !aero {
        assert!(airborne && lift && masks.len() > 3);
    }
    if name == "boundaries" {
        assert!(resets && capped);
    }
}
