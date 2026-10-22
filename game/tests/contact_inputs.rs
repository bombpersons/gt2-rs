//! Original grip producer/force consumers and late contact-motion refresh.
use bevy::prelude::*;
use engine::testing::{TickApp, headless_app};
use game::vehicle::{
    VehicleStepDuration, VehicleSystems, VehicleVelocity, WheelSteering,
    body_dynamics::BodyDynamics,
    brake_assist::BrakeAssistInput,
    contact_motion::*,
    forward_dynamics::ForwardDynamicsInput,
    longitudinal_slip::{LongitudinalSlip, LongitudinalSlipRulesPlugin, WheelSurfaceMotion},
    slip::{WheelLocalVelocities, WheelSlip, WheelSlipRulesPlugin},
    tyre_forces::{TyreContactState, TyreForces, TyreForcesRulesPlugin},
    tyre_grip::*,
    wheel_rotation::AxleDrive,
};
use std::collections::{HashMap, HashSet};

type Row = HashMap<String, i32>;
fn rows(name: &str) -> Vec<Row> {
    let text = std::fs::read_to_string(format!(
        "{}/tests/data/original/contact_inputs/{name}.csv",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let mut lines = text.lines();
    let columns: Vec<_> = lines.next().unwrap().split(',').collect();
    lines
        .map(|l| columns.iter().zip(l.split(',')).map(|(k, v)| (k.to_string(), v.parse().unwrap())).collect())
        .collect()
}

/// Explicit gap: collision/orientation and the intervening drivetrain still
/// supply these values at the original late refresh boundary. Force input
/// velocities and grip remain native and observable earlier in the tick.
#[derive(Component)]
struct MotionBoundary {
    velocity: VehicleVelocity,
    treads: [f32; 4],
    shafts: [f32; 2],
}
fn enter_motion(mut cars: Query<(&MotionBoundary, &mut VehicleVelocity, &mut WheelSurfaceMotion, &mut AxleDrive)>) {
    for (boundary, mut velocity, mut wheels, mut axles) in &mut cars {
        *velocity = boundary.velocity;
        wheels.tread_speed_mps = boundary.treads;
        axles.radians_per_second = boundary.shafts;
    }
}

fn check(name: &str, count: usize) {
    let original = rows(name);
    assert_eq!(original.len(), count * 4);
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.add_plugins((
        ContactMotionRulesPlugin,
        TyreForcesRulesPlugin,
        LongitudinalSlipRulesPlugin,
        WheelSlipRulesPlugin,
        TyreGripRulesPlugin,
    ));
    app.add_systems(
        FixedUpdate,
        enter_motion.in_set(VehicleSystems::RefreshContact).before(ContactMotionSystems::Refresh),
    );
    let standard = app.world().resource::<TyreGripTuning>().clone();
    let car = app
        .world_mut()
        .spawn((
            (
                TyreGripInput::default(),
                TyreCondition::default(),
                TyreGrip::default(),
                TyreContactState::default(),
                WheelSurfaceMotion::default(),
                WheelLocalVelocities::default(),
                WheelSteering::default(),
                WheelSlip::default(),
                LongitudinalSlip::default(),
                TyreForces::default(),
            ),
            (
                ContactMotionInput::default(),
                ContactMotion::default(),
                VehicleVelocity::default(),
                VehicleStepDuration::default(),
                AxleDrive::default(),
                BrakeAssistInput::default(),
                BodyDynamics::default(),
            ),
        ))
        .id();
    let mut previous = None;
    let mut carried = 0;
    let mut surfaces = HashSet::new();
    let mut synchronization = HashSet::new();
    let mut condition_indicators = HashSet::new();
    let mut response_speeds = HashSet::new();
    let mut negative_projection = false;
    let mut skipped_scale = false;
    for group in original.as_chunks::<4>().0 {
        let r = &group[0];
        let label = format!("{name} case {}", r["case"]);
        let sample = |phase: &str, key: &str| r[&format!("ci_{phase}_{key}")] as f32;
        let gb = |key: &str| sample("grip_before", key);
        let mb = |key: &str| sample("motion_before", key);
        let four = |key: &str| std::array::from_fn(|i| mb(&format!("{key}{i}")));
        let two = |key: &str| std::array::from_fn(|i| mb(&format!("{key}{i}")));
        let wheel = |key: &str| std::array::from_fn(|i| group[i][key] as f32 / 4096.0);
        let mut tuning = standard.clone();
        for i in 0..2 {
            tuning.axles[i].camber_turns = gb(&format!("camber{i}")) / 4096.0;
        }
        tuning.condition = (gb("condition_maximum") != 0.0).then(|| TyreConditionTuning {
            maximum: gb("condition_maximum"),
            warm_start: gb("condition_warm"),
            cold_limit: gb("condition_cold"),
            maximum_load_loss: gb("condition_max_loss") / 4096.0,
            warm_load_loss: gb("condition_warm_loss") / 4096.0,
            cold_load_loss: gb("condition_cold_loss") / 4096.0,
        });
        let input = TyreGripInput {
            load: std::array::from_fn(|i| gb(&format!("load{i}"))),
            surface: std::array::from_fn(|i| gb(&format!("surface{i}")) as usize),
            roll_difference_turns: (gb("roll") - gb("contact_roll")) / 4096.0,
            bypass_condition: gb("condition_flags") as u32 & 16 != 0,
        };
        surfaces.extend(input.surface);
        let mut condition = TyreCondition {
            accumulator: std::array::from_fn(|i| gb(&format!("accumulator{i}"))),
            load_scale: std::array::from_fn(|i| gb(&format!("condition_scale{i}")) / 4096.0),
            indicator: std::array::from_fn(|i| gb(&format!("indicator{i}")) as i8),
        };
        let mut contact = TyreContactState { body_forward_mps: r["body_forward"] as f32 / 4096.0, ..default() };
        let mut local = WheelLocalVelocities { forward_mps: wheel("forward"), lateral_mps: wheel("lateral") };
        let mut state = ContactMotion {
            spin_turns: four("spin").map(|v| v / 4096.0),
            maximum_speed_readout: mb("maximum_speed") as u16,
            ..default()
        };
        if previous == Some(r["sequence"]) {
            local = *app.world().get::<WheelLocalVelocities>(car).unwrap();
            assert_eq!(
                local.forward_mps,
                wheel("forward"),
                "{label}: contact forward velocities carry to next slip/force tick"
            );
            assert_eq!(
                local.lateral_mps,
                wheel("lateral"),
                "{label}: contact lateral velocities carry to next slip/force tick"
            );
            contact.body_forward_mps = app.world().get::<TyreContactState>(car).unwrap().body_forward_mps;
            assert_eq!(contact.body_forward_mps * 4096.0, r["body_forward"] as f32, "{label}: local body speed carry");
            state = *app.world().get::<ContactMotion>(car).unwrap();
            assert_eq!(state.spin_turns, four("spin").map(|v| v / 4096.0), "{label}: spin carry");
            assert_eq!(state.maximum_speed_readout, mb("maximum_speed") as u16, "{label}: maximum speed carry");
            let old = *app.world().get::<TyreCondition>(car).unwrap();
            assert_eq!(old.load_scale, condition.load_scale, "{label}: condition scale carry");
            assert_eq!(old.indicator, condition.indicator, "{label}: condition indicator carry");
            condition.load_scale = old.load_scale;
            condition.indicator = old.indicator;
            carried += 1;
        }
        let vector =
            |axis: &str| Vec3::new(mb(&format!("{axis}0")), mb(&format!("{axis}2")), -mb(&format!("{axis}1"))) / 4096.0;
        app.world_mut().entity_mut(car).insert((
            (
                input,
                condition,
                TyreGripSetup(tuning),
                contact,
                local,
                state,
                WheelSurfaceMotion { tread_speed_mps: wheel("tread"), speed_response: wheel("speed_response") },
                WheelSteering { angle_turns: wheel("steering") },
            ),
            (
                ContactMotionInput {
                    forward_axis: vector("forward_axis"),
                    lateral_axis: vector("lateral_axis"),
                    synchronize_wheels: std::array::from_fn(|i| mb(&format!("flags{i}")) == 0.0),
                    independent_axles: two("independent").map(|v| v != 0.0),
                    wheel_heading_turns: four("heading").map(|v| v / 4096.0),
                },
                VehicleStepDuration(mb("duration") / 65536.0),
                MotionBoundary {
                    velocity: VehicleVelocity {
                        linear_mps: Vec3::new(mb("vx"), mb("vz"), -mb("vy")) / 4096.0,
                        yaw_turns_per_second: mb("yaw") / 524288.0,
                    },
                    treads: four("tread").map(|v| v / 4096.0),
                    shafts: two("shaft").map(|v| v / 4096.0),
                },
            ),
        ));
        app.run_tick(&[]);
        let grip = app.world().get::<TyreGrip>(car).unwrap();
        let condition = app.world().get::<TyreCondition>(car).unwrap();
        let tyres = app.world().get::<TyreForces>(car).unwrap();
        let assist = app.world().get::<BrakeAssistInput>(car).unwrap();
        for (i, original_wheel) in group.iter().enumerate() {
            for (key, value) in [
                ("effective_load", grip.effective_load[i]),
                ("load_capacity", grip.load_capacity[i]),
                ("camber_response", grip.camber_response[i] * 4096.0),
            ] {
                assert_eq!(value, r[&format!("ci_{key}{i}")] as f32, "{label} wheel {i}: {key}");
            }
            assert_eq!(grip.capacity[i], sample("grip_after", &format!("capacity{i}")), "{label} wheel {i}: capacity");
            assert_eq!(assist.grip_response[i] * 4096.0, grip.capacity[i], "{label}: assistance bridge");
            assert_eq!(
                condition.accumulator[i],
                sample("grip_after", &format!("accumulator{i}")),
                "{label}: accumulator"
            );
            assert_eq!(
                condition.load_scale[i] * 4096.0,
                sample("grip_after", &format!("condition_scale{i}")),
                "{label}: scale"
            );
            assert_eq!(
                condition.indicator[i] as f32,
                sample("grip_after", &format!("indicator{i}")),
                "{label}: indicator"
            );
            condition_indicators.insert(condition.indicator[i]);
            skipped_scale |=
                condition.load_scale[i] != 1.0 && grip.effective_load[i] == input.load[i] && input.load[i] != 0.0;
            for (key, value) in [
                ("wheel_force", tyres.wheel_reference_force[i]),
                ("ground_force", tyres.ground_reference_force[i]),
                ("lateral_force", tyres.lateral_force[i]),
            ] {
                assert_eq!(value, original_wheel[key] as f32, "{label} wheel {i}: native {key}");
            }
        }
        let state = app.world().get::<ContactMotion>(car).unwrap();
        let local = app.world().get::<WheelLocalVelocities>(car).unwrap();
        let wheels = app.world().get::<WheelSurfaceMotion>(car).unwrap();
        let axles = app.world().get::<AxleDrive>(car).unwrap();
        for (key, value) in [
            ("body_forward", state.body_forward_mps * 4096.0),
            ("body_lateral", state.body_lateral_mps * 4096.0),
            ("speed", state.speed_readout as f32),
            ("maximum_speed", state.maximum_speed_readout as f32),
        ] {
            assert_eq!(value, sample("motion_after", key), "{label}: {key}");
        }
        for i in 0..4 {
            for (key, value) in [
                ("forward", local.forward_mps[i] * 4096.0),
                ("lateral", local.lateral_mps[i] * 4096.0),
                ("response", wheels.speed_response[i] * 4096.0),
                ("spin", state.spin_turns[i] * 4096.0),
                ("tread", wheels.tread_speed_mps[i] * 4096.0),
            ] {
                assert_eq!(value, sample("motion_after", &format!("{key}{i}")), "{label} wheel {i}: {key}");
            }
            response_speeds.insert((local.forward_mps[i].abs() * 4096.0) as i32);
            negative_projection |= local.lateral_mps[i] < 0.0;
        }
        for i in 0..2 {
            assert_eq!(
                axles.radians_per_second[i] * 4096.0,
                sample("motion_after", &format!("shaft{i}")),
                "{label}: shaft {i}"
            );
            synchronization.insert((i, state.synchronized_axles[i]));
        }
        previous = Some(r["sequence"]);
    }
    assert!(carried >= 39);
    assert!(negative_projection);
    if name == "boundaries" {
        assert_eq!(surfaces.len(), 16);
        assert_eq!(synchronization.len(), 4, "both axles must synchronize and preserve");
        assert!(
            condition_indicators.contains(&-128)
                && condition_indicators.contains(&-63)
                && condition_indicators.contains(&127)
        );
        assert!(skipped_scale, "bypassed/disabled condition must ignore retained scale");
        for speed in [113798, 113799, 113800] {
            assert!(response_speeds.contains(&speed), "missing exact response speed {speed}");
        }
    }
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original contact-input fixtures; see docs/assets.md")]
fn original_grip_conditions_force_consumers_and_late_contact_motion() {
    check("boundaries", 326);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original contact-input fixtures; see docs/assets.md")]
fn independent_original_contact_input_holdout() {
    check("holdout", 54);
}

#[test]
fn handmade_contact_refresh_runs_without_force_control_or_render_plugins() {
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(ContactMotionTuning {
        wheel_levers_m: [[-0.5, 1.0], [0.5, 1.0], [-0.5, -1.0], [0.5, -1.0]],
        inverse_radius_per_m: [1.0; 2],
        radians_per_turn: 8.0,
        turns_per_radian: 0.125,
        speed_readout_units_per_mps: 16.0,
        full_response_speed_mps: 10.0,
        response_gain_per_mps: 0.25,
        base_speed_response: 0.5,
    })
    .add_plugins(ContactMotionRulesPlugin);
    let car = app
        .world_mut()
        .spawn((
            ContactMotionInput {
                forward_axis: Vec3::X,
                lateral_axis: Vec3::Z,
                synchronize_wheels: [true, false, false, true],
                ..default()
            },
            ContactMotion::default(),
            WheelSurfaceMotion::default(),
            WheelLocalVelocities::default(),
            VehicleVelocity { linear_mps: Vec3::new(2.0, 0.0, 3.0), yaw_turns_per_second: 0.125 },
            VehicleStepDuration(1.0),
            AxleDrive::default(),
            TyreContactState::default(),
            game::vehicle::SteeringKinematics::default(),
            BodyDynamics::default(),
        ))
        .id();
    app.run_tick(&[]);
    let state = app.world().get::<ContactMotion>(car).unwrap();
    assert_eq!(state.spin_turns, [0.1875, 0.0, 0.0, 0.3125]);
    assert_eq!(state.speed_readout, 32);
    assert_eq!(app.world().get::<WheelLocalVelocities>(car).unwrap().forward_mps, [1.5, 2.5, 1.5, 2.5]);
    assert_eq!(app.world().get::<WheelLocalVelocities>(car).unwrap().lateral_mps, [2.0, 2.0, 4.0, 4.0]);
    assert_eq!(app.world().get::<AxleDrive>(car).unwrap().radians_per_second, [0.75, 1.25]);
    assert_eq!(app.world().get::<TyreContactState>(car).unwrap().body_forward_mps, 2.0);
    assert_eq!(app.world().get::<BodyDynamics>(car).unwrap().local_lateral_mps, 3.0);
}

#[test]
fn handmade_grip_runs_alone_and_updates_optional_force_assistance_inputs() {
    let axle = GripAxleTuning {
        load_curve: vec![GripLoadKnot { load: 0.0, capacity: 0.0 }, GripLoadKnot { load: 8.0, capacity: 16.0 }],
        camber_curve: vec![
            GripCamberKnot { angle_turns: 0.0, response: 1.0 },
            GripCamberKnot { angle_turns: 0.5, response: 0.5 },
        ],
        camber_turns: 0.0,
    };
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(TyreGripTuning {
        axles: [axle.clone(), axle],
        surface_response: vec![1.0, 0.5],
        condition: None,
    })
    .add_plugins(TyreGripRulesPlugin);
    let car = app
        .world_mut()
        .spawn((
            TyreGripInput { load: [8.0; 4], surface: [0, 1, 0, 1], roll_difference_turns: 0.125, ..default() },
            TyreGrip::default(),
            TyreCondition::default(),
            TyreContactState::default(),
            BrakeAssistInput::default(),
            ForwardDynamicsInput::default(),
        ))
        .id();
    app.run_tick(&[]);
    assert_eq!(app.world().get::<TyreGrip>(car).unwrap().capacity, [14.0, 7.0, 14.0, 7.0]);
    assert_eq!(app.world().get::<TyreContactState>(car).unwrap().grip_capacity, [14.0, 7.0, 14.0, 7.0]);
    assert_eq!(app.world().get::<ForwardDynamicsInput>(car).unwrap().wheel_load, [8.0; 4]);
    assert_eq!(app.world().get::<ForwardDynamicsInput>(car).unwrap().surface, [0, 1, 0, 1]);
    assert_eq!(
        app.world().get::<BrakeAssistInput>(car).unwrap().grip_response,
        [14.0 / 4096.0, 7.0 / 4096.0, 14.0 / 4096.0, 7.0 / 4096.0]
    );
}
