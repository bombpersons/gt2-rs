//! Original whole ordinary transmission phases, with native coupling.
use bevy::prelude::*;
use engine::testing::{TickApp, headless_app};
use game::vehicle::braking::{BrakingRulesPlugin, WheelBrakeDemand, WheelBrakeInput};
use game::vehicle::drivetrain::*;
use game::vehicle::engine_torque::*;
use game::vehicle::forward_dynamics::*;
use game::vehicle::longitudinal_slip::{LongitudinalSlip, LongitudinalSlipRulesPlugin, WheelSurfaceMotion};
use game::vehicle::slip::{WheelLocalVelocities, WheelSlip, WheelSlipRulesPlugin};
use game::vehicle::transmission::*;
use game::vehicle::tyre_forces::*;
use game::vehicle::wheel_rotation::*;
use game::vehicle::{VehicleStepDuration, VehicleSystems, WheelSteering};
use std::collections::{HashMap, HashSet};

type Row = HashMap<String, i32>;
fn rows(name: &str) -> Vec<Row> {
    let text =
        std::fs::read_to_string(format!("{}/tests/data/original/transmission/{name}.csv", env!("CARGO_MANIFEST_DIR")))
            .unwrap();
    let mut lines = text.lines();
    let names: Vec<_> = lines.next().unwrap().split(',').collect();
    lines
        .map(|line| names.iter().zip(line.split(',')).map(|(k, v)| (k.to_string(), v.parse().unwrap())).collect())
        .collect()
}
fn engagement(raw: i32) -> EngineEngagement {
    match raw {
        0 => EngineEngagement::Disconnected,
        1 => EngineEngagement::Coupled,
        2 => EngineEngagement::Synchronizing,
        3 => EngineEngagement::FixedClutch,
        4 => EngineEngagement::Slipping,
        _ => panic!("unknown original engagement"),
    }
}
fn raw_engagement(v: EngineEngagement) -> i32 {
    match v {
        EngineEngagement::Disconnected => 0,
        EngineEngagement::Coupled => 1,
        EngineEngagement::Synchronizing => 2,
        EngineEngagement::FixedClutch => 3,
        EngineEngagement::Slipping => 4,
    }
}
#[derive(Default, Debug, Clone, Copy)]
struct Snapshot {
    engine: EngineTorqueInput,
    drive: DrivetrainInput,
    state: TransmissionState,
    cut: EngineFuelCut,
}
#[derive(Component, Default)]
struct Phases([Snapshot; 3]);
fn record<const N: usize>(
    mut cars: Query<(&EngineTorqueInput, &DrivetrainInput, &TransmissionState, &EngineFuelCut, &mut Phases)>,
) {
    for (&engine, &drive, &state, &cut, mut phases) in &mut cars {
        phases.0[N] = Snapshot { engine, drive, state, cut };
    }
}
fn compare(r: &Row, prefix: &str, snapshot: Snapshot, label: &str) {
    let key = |name: &str| r[&format!("tr_{prefix}_{name}")];
    for (name, value) in [
        ("engine_speed", snapshot.engine.speed_rev_s * 4096.0),
        ("rpm", snapshot.engine.integer_rpm),
        ("throttle", snapshot.engine.effective_throttle * 4096.0),
        ("gear", snapshot.drive.gear as f32),
        ("engagement", raw_engagement(snapshot.drive.engagement) as f32),
        ("cooldown", snapshot.state.cooldown_ticks as f32),
        ("force_first", snapshot.state.first_gear_ticks as f32),
    ] {
        assert_eq!(value, key(name) as f32, "{label} {prefix}: {name}");
    }
    assert_eq!(snapshot.state.engagement_requested, key("requested") != 0, "{label} {prefix}: request state");
    assert_eq!(snapshot.cut.active, key("cut") != 0, "{label} {prefix}: cut state");
}
fn current(world: &World, car: Entity) -> Snapshot {
    Snapshot {
        engine: *world.get::<EngineTorqueInput>(car).unwrap(),
        drive: *world.get::<DrivetrainInput>(car).unwrap(),
        state: *world.get::<TransmissionState>(car).unwrap(),
        cut: *world.get::<EngineFuelCut>(car).unwrap(),
    }
}
fn check(name: &str, count: usize) {
    let original = rows(name);
    assert_eq!(original.len(), count * 4);
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.add_plugins((
        TransmissionRulesPlugin,
        DrivetrainRulesPlugin,
        WheelRotationRulesPlugin,
        ForwardDynamicsRulesPlugin,
        TyreForcesRulesPlugin,
        LongitudinalSlipRulesPlugin,
        WheelSlipRulesPlugin,
        BrakingRulesPlugin,
        EngineTorqueRulesPlugin,
    ));
    app.add_systems(
        FixedUpdate,
        (
            record::<0>.after(DrivetrainSystems::Prepare).before(EngineTorqueSystems::Prepare),
            record::<1>.after(WheelRotationSystems::Integrate).before(TransmissionSystems::Feedback),
            record::<2>.after(TransmissionSystems::Feedback).before(TransmissionSystems::Select),
        )
            .in_set(VehicleSystems::Forces),
    );
    let car = app
        .world_mut()
        .spawn((
            (
                TransmissionInput::default(),
                TransmissionState::default(),
                TransmissionOutput::default(),
                Phases::default(),
                DrivetrainInput::default(),
                DrivetrainState::default(),
                DrivetrainOutput::default(),
                EngineTorqueInput::default(),
                EngineFuelCut::default(),
                EngineTorque::default(),
            ),
            (
                WheelRotationInput::default(),
                AxleDrive::default(),
                WheelRotationState::default(),
                WheelReactionTorque::default(),
                WheelBrakeDemand::default(),
                ForwardDynamicsInput::default(),
                ForwardDynamics::default(),
                TyreContactState::default(),
                WheelSurfaceMotion::default(),
                WheelLocalVelocities::default(),
                WheelSteering::default(),
                WheelSlip::default(),
                LongitudinalSlip::default(),
                TyreForces::default(),
                VehicleStepDuration::default(),
            ),
        ))
        .id();
    let mut previous = None;
    let standard_engine = app.world().resource::<EngineTorqueTuning>().clone();
    let standard_drive = app.world().resource::<DrivetrainTuning>().clone();
    let standard_wheels = app.world().resource::<WheelRotationTuning>().clone();
    let mut continued = 0;
    let mut modes = HashSet::new();
    let mut engagements = HashSet::new();
    let mut changed_gears = 0;
    let mut engaged_transitions = 0;
    let mut allowed_guard_layouts = HashSet::new();
    let mut blocked_guard_layouts = HashSet::new();
    for group in original.as_chunks::<4>().0 {
        let r = &group[0];
        let label = format!("{name} {}", r["case"]);
        let initial = |key: &str| r[&format!("tr_initial_{key}")];
        let before = |key: &str| r[&format!("dt_before_{key}")] as f32;
        let two = |key: &str| std::array::from_fn(|i| before(&format!("{key}{i}")));
        let four = |key: &str| std::array::from_fn(|i| r[&format!("rot_{key}{i}")] as f32);
        let wheel = |key: &str| std::array::from_fn(|i| group[i][key] as f32 / 4096.0);
        let mut engine_tuning = standard_engine.clone();
        if r["tr_isolated"] != 0 {
            engine_tuning.drag_per_rev_s = 0.0;
            for knot in &mut engine_tuning.torque_curve {
                knot.torque = 0.0;
            }
        }
        app.insert_resource(engine_tuning);
        let mut drivetrain = standard_drive.clone();
        let fraction = before("viscosity") / 4096.0;
        drivetrain.layout = match initial("layout") {
            0 => DriveLayout::Rear,
            1 => DriveLayout::Front,
            2 => DriveLayout::Viscous,
            3 => DriveLayout::Open,
            4 => DriveLayout::Clutch { front_fraction: fraction },
            5 => DriveLayout::AdaptiveClutch,
            6 => DriveLayout::Split { front_fraction: fraction },
            _ => panic!("unknown original layout"),
        };
        drivetrain.fixed_clutch_torque = before("fixed_clutch");
        drivetrain.coast_clutch_torque = before("coast_clutch");
        app.insert_resource(drivetrain);
        let mut wheel_tuning = standard_wheels.clone();
        wheel_tuning.differentials =
            two("wheel_kind").map(|kind| if kind == 0.0 { Differential::Independent } else { Differential::Open });
        app.insert_resource(wheel_tuning);
        assert_eq!(initial("source"), 5, "alternate engine controllers are separate work");
        assert_eq!(initial("induction"), 0, "induction controller is separate work");
        let mut state = TransmissionState {
            first_gear_ticks: r["tr_timer_first_counter"] as u16,
            engagement_requested: initial("requested") != 0,
            cooldown_ticks: initial("cooldown") as u8,
        };
        let mut engine = EngineTorqueInput {
            speed_rev_s: initial("engine_speed") as f32 / 4096.0,
            integer_rpm: initial("rpm") as f32,
            effective_throttle: initial("throttle") as f32 / 4096.0,
        };
        let mut drive = DrivetrainInput {
            gear: initial("gear") as usize,
            engagement: engagement(initial("engagement")),
            adaptive_angle_turns: before("angle") / 4096.0,
            brake_travel: initial("brake") as f32 / 4096.0,
            alternate_locked_step: before("alternate_locked_step") != 0.0,
            independent_wheels: two("wheel_kind").map(|v| v == 0.0),
            ..default()
        };
        let mut cut = EngineFuelCut { active: initial("cut") != 0 };
        let mut axles = AxleDrive { radians_per_second: two("speed").map(|v| v / 4096.0), ..default() };
        let mut dynamics = DrivetrainState {
            previous_angular_delta: two("delta").map(|v| v / 4096.0),
            centre_locked: before("locked") != 0.0,
        };
        let mut wheels = WheelRotationState {
            previous_delta_mps: four("before_delta").map(|v| v / 4096.0),
            clutch_locked: [false; 2],
        };
        if previous == Some(r["sequence"]) {
            let native = current(app.world(), car);
            // Effective throttle is a fresh control input; all actual engine,
            // gear, engagement, timer and fuel-cut state must persist natively.
            engine.speed_rev_s = native.engine.speed_rev_s;
            engine.integer_rpm = native.engine.integer_rpm;
            state = native.state;
            cut = native.cut;
            drive.gear = native.drive.gear;
            drive.engagement = native.drive.engagement;
            let mut expected = native;
            expected.engine.effective_throttle = engine.effective_throttle;
            assert_eq!(state.first_gear_ticks, r["tr_timer_first_counter"] as u16, "{label}: first-gear counter carry");
            expected.state.first_gear_ticks = expected.state.first_gear_ticks.saturating_sub(1);
            compare(r, "initial", expected, &label);
            axles = *app.world().get::<AxleDrive>(car).unwrap();
            dynamics = *app.world().get::<DrivetrainState>(car).unwrap();
            wheels = *app.world().get::<WheelRotationState>(car).unwrap();
            for i in 0..2 {
                if drive.independent_wheels[i] {
                    dynamics.previous_angular_delta[i] = two("delta")[i] / 4096.0;
                }
                if drive.independent_wheels[i] || r[&format!("dt_before_external_speed{i}")] != 0 {
                    axles.radians_per_second[i] = two("speed")[i] / 4096.0;
                }
            }
            continued += 1;
        }
        let mode = match initial("mode") {
            0 => ShiftMode::Automatic,
            1 => ShiftMode::Manual,
            _ => ShiftMode::Disabled,
        };
        app.world_mut().entity_mut(car).insert((
            (
                TransmissionInput {
                    mode,
                    force_automatic: initial("force_auto") != 0,
                    reverse_requested: initial("reverse_request") != 0,
                    shift_direction: initial("shift_request") as i8,
                    accelerator_travel: initial("accelerator") as f32 / 4096.0,
                    forward_mps: initial("forward") as f32 / 4096.0,
                    ..default()
                },
                state,
                engine,
                drive,
                cut,
                axles,
                dynamics,
            ),
            (
                WheelRotationInput {
                    auxiliary_tread_force: four("auxiliary_force"),
                    yaw_correction: r["rot_source_yaw_correction"] as f32 / 4096.0,
                    steering_turns: r["rot_steering"] as f32 / 4096.0,
                },
                wheels,
                WheelBrakeInput {
                    travel: four("brake_input").map(|v| v / 4096.0),
                    handbrake: r["rot_handbrake"] as f32 / 4096.0,
                },
                ForwardDynamicsInput {
                    wheel_load: std::array::from_fn(|i| r[&format!("fd_load{i}")] as f32),
                    surface: std::array::from_fn(|i| r[&format!("fd_surface{i}")] as usize),
                    aerodynamic_force: r["fd_aerodynamic_force"] as f32,
                },
                TyreContactState {
                    grip_capacity: std::array::from_fn(|i| group[i]["grip"] as f32),
                    body_forward_mps: initial("forward") as f32 / 4096.0,
                },
                WheelSurfaceMotion { tread_speed_mps: wheel("tread"), speed_response: wheel("speed_response") },
                WheelLocalVelocities { forward_mps: wheel("forward"), lateral_mps: wheel("lateral") },
                WheelSteering { angle_turns: wheel("steering") },
                VehicleStepDuration(r["fd_duration"] as f32 / 65536.0),
            ),
        ));
        app.run_tick(&[]);
        let phases = app.world().get::<Phases>(car).unwrap();
        for (i, prefix) in ["prepared", "rotation", "selection"].iter().enumerate() {
            compare(r, prefix, phases.0[i], &label);
        }
        compare(r, "final", current(app.world(), car), &label);
        for (i, value) in app.world().get::<WheelSurfaceMotion>(car).unwrap().tread_speed_mps.iter().enumerate() {
            assert_eq!(*value * 4096.0, r[&format!("rot_after_tread{i}")] as f32, "{label}: wheel {i} tread");
        }
        let selection = app.world().get::<TransmissionOutput>(car).unwrap().selected_gear;
        if name == "shift_holdout"
            && r["tr_selection_mode"] == 0
            && r["tr_selection_forward"] == 4096
            && r["tr_selection_engagement"] == 1
            && (0..4).any(|i| r[&format!("tr_selection_load{i}")] == 0 || r[&format!("tr_selection_slip{i}")] != 0)
        {
            if selection.is_some() {
                allowed_guard_layouts.insert(initial("layout"));
            } else {
                blocked_guard_layouts.insert(initial("layout"));
            }
        }
        let changed = initial("gear") != r["tr_final_gear"];
        assert_eq!(selection.is_some(), r["tr_selection_gear"] != r["tr_final_gear"], "{label}: gear application");
        changed_gears += usize::from(changed);
        engaged_transitions += usize::from(initial("engagement") != r["tr_final_engagement"]);
        modes.insert(initial("mode"));
        engagements.insert(initial("engagement"));
        previous = Some(r["sequence"]);
    }
    if name != "shift_holdout" {
        assert!(continued >= 39);
    } else {
        assert_eq!(allowed_guard_layouts.len(), 7, "slip/contact probes must permit shifts in every layout");
        assert_eq!(blocked_guard_layouts.len(), 7, "slip/contact probes must block shifts in every layout");
    }
    assert!(changed_gears > 0);
    assert!(engaged_transitions > 0);
    assert_eq!(modes.len(), 3);
    if name == "boundaries" {
        assert_eq!(engagements.len(), 5);
    }
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original transmission fixtures; see docs/assets.md")]
fn original_whole_ordinary_transmission_phases_and_consecutive_state() {
    check("boundaries", 254);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original transmission fixtures; see docs/assets.md")]
fn independent_original_transmission_signed_holdout() {
    check("holdout", 64);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original transmission fixtures; see docs/assets.md")]
fn final_original_exact_shift_slip_load_and_engine_boundaries() {
    check("shift_holdout", 353);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original transmission fixtures; see docs/assets.md")]
fn final_independent_original_alternating_transmission_sequence() {
    check("final_holdout", 52);
}

#[test]
fn handmade_transmission_feedback_manual_hold_and_per_car_setups_run_alone() {
    let setup = TransmissionTuning {
        forward_gears: 5,
        minimum_shift_rpm: 60.0,
        upshift_rpm: 300.0,
        downshift_rpm: vec![120.0, 180.0, 200.0, 220.0],
        engine_rev_s_per_mps: vec![1.0; 6],
        downshift_slip_limit: [0.25; 2],
        upshift_slip_limit: [-0.25; 2],
        launch_speed_mps: 0.5,
        launch_throttle: 0.25,
        forward_select_speed_mps: 0.1,
        reverse_select_speed_mps: 0.1,
        forward_return_speed_mps: 0.2,
        full_throttle_threshold: 0.75,
        throttle_downshift_margin_rpm: 10.0,
        redline_speed_margin_rpm: 60.0,
        first_downshift_rpm: -1000.0,
        top_upshift_rpm: 1000.0,
        feedback_redline_margin_rpm: 0.0,
        braking_idle_margin_rpm: 0.0,
        reengagement_delay_ticks: 3,
    };
    let mut other_setup = setup.clone();
    other_setup.upshift_rpm = 400.0;
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(setup)
        .insert_resource(EngineTorqueTuning {
            torque_curve: vec![],
            drag_per_rev_s: 0.0,
            redline_rpm: 600.0,
            idle_rpm: 60.0,
            fuel_cut_hysteresis_rpm: 60.0,
        })
        .insert_resource(DrivetrainTuning {
            layout: DriveLayout::Front,
            reference_axle: Axle::Front,
            gear_ratios: vec![1.0; 6],
            axle_inertia: [1.0; 2],
            engine_inertia: 1.0,
            centre_rotation_response: 0.0,
            centre_viscosity: 0.0,
            adaptive_lock: false,
            centre_lock_torque: 0.0,
            engine_rotation_response: 0.0,
            revolutions_per_radian: 0.125,
            fixed_clutch_torque: 0.0,
            coast_clutch_torque: 0.0,
            clutch_free_difference_rev_s: 0.0,
            clutch_gain_per_rev_s: 0.0,
            max_clutch_gain: 1.0,
            adaptive_full_angle_turns: 0.25,
            adaptive_min_angle_turns: 1.0,
            adaptive_min_response: 0.25,
            adaptive_taper_intercept: 1.25,
            lock_brake_travel: 0.75,
        })
        .add_plugins(TransmissionRulesPlugin);
    let spawn = |world: &mut World| {
        world
            .spawn((
                TransmissionInput {
                    forward_mps: 1.0,
                    accelerator_travel: 1.0,
                    wheel_load: [1.0; 4],
                    speed_response: [1.0; 4],
                    ..default()
                },
                TransmissionState { engagement_requested: true, ..default() },
                TransmissionOutput::default(),
                EngineTorqueInput { speed_rev_s: 6.0, integer_rpm: 360.0, effective_throttle: 1.0 },
                EngineFuelCut::default(),
                DrivetrainInput { gear: 1, engagement: EngineEngagement::Coupled, ..default() },
                AxleDrive { radians_per_second: [48.0; 2], ..default() },
                DrivetrainOutput { clutch_speeds_rev_s: [6.0; 2], ..default() },
                VehicleStepDuration(0.25),
            ))
            .id()
    };
    let car = spawn(app.world_mut());
    let other = spawn(app.world_mut());
    app.world_mut().entity_mut(other).insert(TransmissionSetup(other_setup));
    app.run_tick(&[]);
    assert_eq!(app.world().get::<DrivetrainInput>(car).unwrap().gear, 2);
    assert_eq!(app.world().get::<DrivetrainInput>(other).unwrap().gear, 1);
    app.world_mut().get_mut::<TransmissionInput>(car).unwrap().mode = ShiftMode::Manual;
    for _ in 0..3 {
        app.run_tick(&[]);
    }
    app.world_mut().get_mut::<TransmissionInput>(car).unwrap().shift_direction = 1;
    app.run_tick(&[]);
    assert_eq!(app.world().get::<DrivetrainInput>(car).unwrap().gear, 3);
    for _ in 0..6 {
        app.run_tick(&[]);
    }
    assert_eq!(app.world().get::<DrivetrainInput>(car).unwrap().gear, 3, "holding the request must not repeat");
    app.world_mut().get_mut::<TransmissionInput>(car).unwrap().shift_direction = 0;
    app.run_tick(&[]);
    app.world_mut().get_mut::<TransmissionInput>(car).unwrap().shift_direction = 1;
    app.run_tick(&[]);
    assert_eq!(app.world().get::<DrivetrainInput>(car).unwrap().gear, 4);
    app.world_mut().get_mut::<TransmissionInput>(other).unwrap().mode = ShiftMode::Disabled;
    app.world_mut().get_mut::<TransmissionState>(other).unwrap().first_gear_ticks = 2;
    app.world_mut().get_mut::<DrivetrainInput>(other).unwrap().gear = 4;
    app.run_tick(&[]);
    assert_eq!(app.world().get::<DrivetrainInput>(other).unwrap().gear, 1);
    assert_eq!(app.world().get::<TransmissionState>(other).unwrap().first_gear_ticks, 1);
    app.run_tick(&[]);
    assert_eq!(app.world().get::<TransmissionState>(other).unwrap().first_gear_ticks, 0);
}
