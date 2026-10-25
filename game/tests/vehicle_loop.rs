//! Complete original ticks. Only the first snapshot initializes native state.
use bevy::prelude::*;
use engine::testing::{TickApp, headless_app};
use game::vehicle::{
    aerodynamics::*, body_dynamics::*, brake_assist::*, braking::*, contact_motion::*, driver_assist::*, drivetrain::*,
    engine_torque::*, forward_dynamics::*, longitudinal_slip::*, road_contact::*, slip::*, suspension::*,
    transmission::*, tyre_forces::*, tyre_grip::*, vertical_body::*, wheel_rotation::*, *,
};
use serde::Deserialize;
use std::path::{Path, PathBuf};

struct Original(Vec<u8>);
impl Original {
    fn load(path: &Path) -> Self {
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(bytes.len(), 0x7A0);
        Self(bytes)
    }
    fn word(&self, off: usize) -> i32 {
        i32::from_le_bytes(self.0[off..off + 4].try_into().unwrap())
    }
    fn short(&self, off: usize) -> i16 {
        i16::from_le_bytes(self.0[off..off + 2].try_into().unwrap())
    }
    fn byte(&self, off: usize) -> u8 {
        self.0[off]
    }
    fn q(&self, off: usize) -> f32 {
        self.word(off) as f32 / 4096.0
    }
    fn angle(&self, off: usize) -> f32 {
        self.short(off) as f32 / 4096.0
    }
    fn vector(&self, off: usize) -> Vec3 {
        Vec3::new(self.q(off), self.q(off + 8), -self.q(off + 4))
    }
    fn axis(&self, off: usize) -> Vec3 {
        Vec3::new(self.angle(off), self.angle(off + 4), -self.angle(off + 2))
    }
    fn wheels<T>(&self, off: usize, read: impl Fn(&Self, usize) -> T) -> [T; 4] {
        std::array::from_fn(|i| read(self, 0x460 + i * 0x68 + off))
    }
    fn basis(&self, off: usize) -> VehicleBasis {
        VehicleBasis { forward: self.axis(off), lateral: self.axis(off + 8), up: self.axis(off + 16) }
    }
}
#[derive(Deserialize)]
struct Controls {
    flags: i32,
    steering: i32,
    throttle: i32,
    brake: i32,
    handbrake: i32,
    #[serde(default)]
    reverse: i32,
    #[serde(default)]
    shift: i8,
}
#[derive(Deserialize)]
struct Tick {
    run_frame: u32,
    before: String,
    forces: String,
    moved: String,
    road: String,
    end: String,
    controls: Controls,
    #[serde(default)]
    automatic_mode: u8,
    #[serde(default)]
    session: Option<SessionSample>,
}
#[derive(Deserialize)]
struct SessionSample {
    countdown_fields: u16,
    clock: u32,
}
#[derive(Deserialize)]
struct Recording {
    ticks: Vec<Tick>,
    alternate_locked_step: bool,
    #[serde(default)]
    session: Option<SessionSample>,
    #[serde(default)]
    initial_automatic_mode: u8,
}
fn engagement(raw: u8) -> EngineEngagement {
    match raw {
        0 => EngineEngagement::Disconnected,
        1 => EngineEngagement::Coupled,
        2 => EngineEngagement::Synchronizing,
        3 => EngineEngagement::FixedClutch,
        4 => EngineEngagement::Slipping,
        _ => panic!("unknown engagement {raw}"),
    }
}
fn initialize(world: &mut World, r: &Original, alternate_locked_step: bool) -> Entity {
    assert_eq!(r.byte(0x372), 5, "ordinary engine source required");
    assert_eq!(r.short(0x128), 0, "induction is a separate feature");
    let independent = std::array::from_fn(|i| r.byte(0x37A + i) == 0);
    let body = r.basis(0x668);
    let road = r.basis(0x68C);
    let pose = VehiclePose { position_m: r.vector(0x65C), heading_turns: r.angle(0x648) };
    let orientation = BodyOrientation { pitch_turns: r.angle(0x644), roll_turns: r.angle(0x646) };
    let state = game::vehicle::initial::VehicleInitialState {
        pose,
        orientation,
        velocity: VehicleVelocity {
            linear_mps: r.vector(0x628),
            yaw_turns_per_second: r.word(0x64C) as f32 / 524288.0,
        },
        step_duration: VehicleStepDuration(r.short(0x6FE) as f32 / 65536.0),
        pedals: PedalTravel { throttle: r.angle(0x610), brake: r.angle(0x612), handbrake: r.angle(0x614) },
        steering: SteeringResponse { angle_turns: r.angle(0x60C), rate_turns_per_second: r.angle(0x60E), ..default() },
        steering_kinematics: SteeringKinematics {
            forward_velocity_mps: r.q(0x6A4),
            front_slip_turns: [r.angle(0x4B0), r.angle(0x518)],
        },
        local_motion: WheelLocalVelocities {
            forward_mps: r.wheels(0x2C, Original::q),
            lateral_mps: r.wheels(0x30, Original::q),
        },
        wheel_motion: WheelSurfaceMotion {
            tread_speed_mps: r.wheels(0x18, Original::q),
            speed_response: r.wheels(0x2A, Original::angle),
        },
        tyre_contact: TyreContactState {
            body_forward_mps: r.q(0x6A4),
            grip_capacity: r.wheels(0x34, |r, o| r.word(o) as f32),
        },
        grip_input: TyreGripInput {
            load: r.wheels(8, |r, o| r.word(o) as f32),
            surface: r.wheels(0x14, |r, o| r.byte(o) as usize),
            roll_difference_turns: r.angle(0x646) - r.angle(0x6F6),
            bypass_condition: r.byte(0x78D) & 16 != 0,
        },
        tyre_condition: TyreCondition {
            accumulator: r.wheels(0x64, |r, o| r.word(o) as f32),
            load_scale: r.wheels(0x38, Original::angle),
            indicator: r.wheels(0x3F, |r, o| r.byte(o) as i8),
        },
        engine: EngineTorqueInput {
            speed_rev_s: r.q(0x624),
            integer_rpm: r.short(0x6AC) as f32,
            effective_throttle: r.angle(0x708),
        },
        fuel_cut: EngineFuelCut { active: r.byte(0x61D) != 0 },
        transmission: TransmissionInput {
            mode: match r.byte(0x642) {
                0 => ShiftMode::Automatic,
                1 => ShiftMode::Manual,
                _ => ShiftMode::Disabled,
            },
            force_automatic: r.byte(0x786) != 0,
            ..default()
        },
        transmission_state: TransmissionState {
            first_gear_ticks: r.short(0x78E) as u16,
            engagement_requested: r.short(0x60A) != 0,
            cooldown_ticks: r.byte(0x61C),
        },
        drivetrain: DrivetrainInput {
            gear: r.byte(0x618) as usize,
            engagement: engagement(r.byte(0x619)),
            adaptive_angle_turns: r.angle(0x64A),
            brake_travel: r.angle(0x612),
            alternate_locked_step,
            independent_wheels: independent,
            ..default()
        },
        drivetrain_state: DrivetrainState {
            previous_angular_delta: [r.angle(0x4DE), r.angle(0x5AE)],
            centre_locked: r.byte(0x63C) != 0,
        },
        axles: AxleDrive { radians_per_second: [r.q(0x634), r.q(0x638)], ..default() },
        wheel_state: WheelRotationState {
            previous_delta_mps: r.wheels(0x16, Original::angle),
            clutch_locked: [r.byte(0x61A) != 0, r.byte(0x61B) != 0],
        },
        body_input: BodyDynamicsInput {
            bypass_tyre_acceleration: r.byte(0x6B1) != 0,
            reset_motion: r.byte(0x718) != 0 || r.byte(0x786) == 7,
            first_gear_override: r.short(0x78E) != 0,
            braked_wheels: r.wheels(0x63, |r, o| r.byte(o) & 2 != 0),
            forward_axis: body.forward.xz(),
            lateral_axis: body.lateral.xz(),
            lateral_resistance: r.word(0x730) as f32,
            external_world_force: Vec2::new(r.word(0x778) as f32, -(r.word(0x77C) as f32)),
            ..default()
        },
        body_dynamics: BodyDynamics {
            local_lateral_mps: r.q(0x6A8),
            lateral_acceleration_mps2: r.q(0x728),
            lateral_feedback_turns: r.angle(0x64A),
            tilt_turns_per_second: Vec2::new(r.short(0x650) as f32, r.short(0x652) as f32) / 65536.0,
            ..default()
        },
        aerodynamics_input: AerodynamicsInput { forward_mps: r.q(0x6A4), reduction_growth_per_second: r.angle(0x774) },
        aerodynamics: Aerodynamics { reduction: r.angle(0x776), ..default() },
        road_input: RoadContactInput::default(),
        road: RoadContactState {
            node: r.short(0x600) as u16,
            raw_height_m: r.wheels(4, |r, o| r.word(o) as f64 / 65536.0),
            height_m: r.wheels(0, Original::q),
            surface: r.wheels(0x14, Original::byte),
            variant: r.wheels(0x15, Original::byte),
            attachment_m: std::array::from_fn(|i| r.axis(0x484 + i * 0x68)),
            road_offset_m: r.wheels(0x42, Original::angle),
            bump_response: r.wheels(0x40, Original::angle),
            body_basis: body,
            road_basis: road,
            road_height_m: r.q(0x688),
            road_pitch_turns: r.angle(0x6F4),
            road_roll_turns: r.angle(0x6F6),
            bypass_condition: r.byte(0x78D) & 16 != 0,
            ..default()
        },
        suspension_input: SuspensionInput {
            body_height_m: pose.position_m.y,
            road_height_m: r.wheels(0, Original::q),
            attachment_height_m: r.wheels(0x28, Original::angle),
            road_offset_m: r.wheels(0x42, Original::angle),
            road_normal_up: road.up.y,
            body_up: body.up.y,
            inverse_step_seconds: r.q(0x704),
            axle_force: [r.word(0x71C) as f32, r.word(0x720) as f32],
            lateral_acceleration_mps2: r.q(0x728),
            vertical_force: r.word(0x72C) as f32,
            forward_force: r.word(0x734) as f32,
            disabled: r.byte(0x786) == 7,
        },
        suspension: Suspension {
            travel_m: r.wheels(0x10, Original::angle),
            travel_rate_mps: r.wheels(0x12, Original::angle),
            tyre_load: r.wheels(8, |r, o| r.word(o) as f64),
            ..default()
        },
        vertical_input: VerticalBodyInput::default(),
        vertical: VerticalBody { airborne: r.byte(0x6B1) != 0, ..default() },
        contact_input: ContactMotionInput {
            forward_axis: road.forward,
            lateral_axis: road.lateral,
            synchronize_wheels: r.wheels(0x63, |r, o| r.byte(o) == 0),
            unloaded_wheels: r.wheels(8, |r, o| r.word(o) == 0),
            independent_axles: independent,
            wheel_heading_turns: r.wheels(0xC, Original::angle),
        },
        contact_motion: ContactMotion {
            body_forward_mps: r.q(0x6A4),
            body_lateral_mps: r.q(0x6A8),
            speed_readout: r.short(0x6AE) as u16,
            maximum_speed_readout: r.short(0x6F8) as u16,
            spin_turns: r.wheels(0x20, |r, o| r.short(o) as u16 as f32 / 4096.0),
            ..default()
        },
        chase_stabilization: Some(game::vehicle::visual_motion::ChaseStabilization {
            yaw_turns: r.angle(0x748),
            yaw_rate_turns_per_second: r.angle(0x74A),
            pitch_turns: r.angle(0x74C),
            pitch_rate_turns_per_second: r.angle(0x74E),
            previous_yaw_acceleration: r.angle(0x752),
        }),
    };
    // Exercise the public native asset format as well as the spawn path.
    let encoded = ron::to_string(&state).unwrap();
    let state: game::vehicle::initial::VehicleInitialState = ron::from_str(&encoded).unwrap();
    let car = state.spawn(world);
    world.entity_mut(car).insert((Name::new("Loop car"), Phases::default()));
    car
}

#[derive(Clone)]
struct Sample {
    name: String,
    off: usize,
    kind: char,
    value: f64,
}
#[derive(Component, Default)]
struct Phases([Vec<Sample>; 4]);
#[derive(Resource)]
struct CarEntity(Entity);
fn capture<const N: usize>(world: &mut World) {
    let car = world.resource::<CarEntity>().0;
    let mut out = Vec::new();
    let mut add =
        |name: &str, off: usize, kind: char, value: f64| out.push(Sample { name: name.into(), off, kind, value });
    let velocity = world.get::<VehicleVelocity>(car).unwrap();
    for (i, v) in [velocity.linear_mps.x, -velocity.linear_mps.z, velocity.linear_mps.y].into_iter().enumerate() {
        add("world velocity", 0x628 + i * 4, 'i', v as f64 * 4096.0);
    }
    add("yaw", 0x64C, 'i', velocity.yaw_turns_per_second as f64 * 524288.0);
    let pose = world.get::<VehiclePose>(car).unwrap();
    for (i, v) in [pose.position_m.x, -pose.position_m.z, pose.position_m.y].into_iter().enumerate() {
        add("position", 0x65C + i * 4, 'i', v as f64 * 4096.0);
    }
    add("heading", 0x648, 'h', pose.heading_turns as f64 * 4096.0);
    if N == 3 {
        let state = world.get::<game::vehicle::visual_motion::ChaseStabilization>(car).unwrap();
        for (offset, value) in [
            (0x748, state.yaw_turns),
            (0x74A, state.yaw_rate_turns_per_second),
            (0x74C, state.pitch_turns),
            (0x74E, state.pitch_rate_turns_per_second),
            (0x752, state.previous_yaw_acceleration),
        ] {
            add("chase stabilization", offset, 'h', value as f64 * 4096.0);
        }
    }
    let steering = world.get::<SteeringResponse>(car).unwrap();
    add("steering", 0x60C, 'h', steering.angle_turns as f64 * 4096.0);
    add("steering rate", 0x60E, 'h', steering.rate_turns_per_second as f64 * 4096.0);
    let pedals = world.get::<PedalTravel>(car).unwrap();
    for (i, v) in [pedals.throttle, pedals.brake, pedals.handbrake].into_iter().enumerate() {
        add("pedals", 0x610 + i * 2, 'h', v as f64 * 4096.0);
    }
    let engine = world.get::<EngineTorqueInput>(car).unwrap();
    add("engine speed", 0x624, 'i', engine.speed_rev_s as f64 * 4096.0);
    add("rpm", 0x6AC, 'h', engine.integer_rpm as f64);
    add("effective throttle", 0x708, 'h', engine.effective_throttle as f64 * 4096.0);
    let drive = world.get::<DrivetrainInput>(car).unwrap();
    add("gear", 0x618, 'B', drive.gear as f64);
    let raw_engagement = match drive.engagement {
        EngineEngagement::Disconnected => 0,
        EngineEngagement::Coupled => 1,
        EngineEngagement::Synchronizing => 2,
        EngineEngagement::FixedClutch => 3,
        EngineEngagement::Slipping => 4,
    };
    add("engagement", 0x619, 'B', raw_engagement as f64);
    add("fuel cut", 0x61D, 'B', f64::from(world.get::<EngineFuelCut>(car).unwrap().active));
    let state = world.get::<TransmissionState>(car).unwrap();
    add("coupling request", 0x60A, 'h', f64::from(state.engagement_requested) * 4096.0);
    add("cooldown", 0x61C, 'B', state.cooldown_ticks as f64);
    add("first gear timer", 0x78E, 'H', state.first_gear_ticks as f64);
    let torque = world.get::<EngineTorque>(car).unwrap();
    add("curve torque", 0x710, 'i', torque.full_throttle as f64);
    add("engine drag", 0x714, 'i', torque.drag as f64);
    let rotation = world.get::<WheelRotationState>(car).unwrap();
    for (i, v) in rotation.previous_delta_mps.into_iter().enumerate() {
        add("wheel increment", 0x476 + i * 0x68, 'h', v as f64 * 4096.0);
    }
    let drivetrain = world.get::<DrivetrainState>(car).unwrap();
    for i in 0..2 {
        if !drive.independent_wheels[i] {
            add("axle increment", 0x4DE + i * 0xD0, 'h', drivetrain.previous_angular_delta[i] as f64 * 4096.0);
        }
    }
    let forward = world.get::<ForwardDynamics>(car).unwrap();
    add("forward force", 0x734, 'i', forward.force as f64);
    add("forward acceleration", 0x724, 'i', forward.acceleration_mps2 as f64 * 4096.0);
    let contact = world.get::<ContactMotion>(car).unwrap();
    add(
        "forward motion",
        0x6A4,
        'i',
        if N == 3 { contact.body_forward_mps } else { forward.forward_mps } as f64 * 4096.0,
    );
    let body = world.get::<BodyDynamics>(car).unwrap();
    add("lateral motion", 0x6A8, 'i', body.local_lateral_mps as f64 * 4096.0);
    add("lateral feedback", 0x64A, 'h', body.lateral_feedback_turns as f64 * 4096.0);
    for (i, v) in body.tilt_turns_per_second.to_array().into_iter().enumerate() {
        add("tilt rate", 0x650 + i * 2, 'h', v as f64 * 65536.0);
    }
    let orientation = world.get::<BodyOrientation>(car).unwrap();
    add("pitch", 0x644, 'h', orientation.pitch_turns as f64 * 4096.0);
    add("roll", 0x646, 'h', orientation.roll_turns as f64 * 4096.0);
    let axles = world.get::<AxleDrive>(car).unwrap();
    for (i, v) in axles.radians_per_second.into_iter().enumerate() {
        add("shaft speed", 0x634 + i * 4, 'i', v as f64 * 4096.0);
    }
    let wheels = world.get::<WheelSurfaceMotion>(car).unwrap();
    let local = world.get::<WheelLocalVelocities>(car).unwrap();
    let suspension = world.get::<Suspension>(car).unwrap();
    let road = world.get::<RoadContactState>(car).unwrap();
    add("node", 0x600, 'H', road.node as f64);
    add("road pitch", 0x6F4, 'h', road.road_pitch_turns as f64 * 4096.0);
    add("road roll", 0x6F6, 'h', road.road_roll_turns as f64 * 4096.0);
    add("road centre height", 0x688, 'i', road.road_height_m as f64 * 4096.0);
    add("compression mask", 0x719, 'B', suspension.compressed_mask as f64);
    add("airborne", 0x6B1, 'B', f64::from(world.get::<VerticalBody>(car).unwrap().airborne));
    add("aero reduction", 0x776, 'h', world.get::<Aerodynamics>(car).unwrap().reduction as f64 * 4096.0);
    if N == 3 {
        add("speed readout", 0x6AE, 'H', contact.speed_readout as f64);
        add("maximum speed", 0x6F8, 'H', contact.maximum_speed_readout as f64);
    }
    for (offset, basis) in [(0x668, road.body_basis), (0x68C, road.road_basis)] {
        for (axis, v) in [basis.forward, basis.lateral, basis.up].into_iter().enumerate() {
            for (c, v) in [v.x, -v.z, v.y].into_iter().enumerate() {
                add("basis", offset + axis * 8 + c * 2, 'h', v as f64 * 4096.0);
            }
        }
    }
    let geometric = world.get::<WheelSlip>(car).unwrap();
    let longitudinal = world.get::<LongitudinalSlip>(car).unwrap();
    let tyres = world.get::<TyreForces>(car).unwrap();
    let grip = world.get::<TyreGrip>(car).unwrap();
    let steering = world.get::<WheelSteering>(car).unwrap();
    let brakes = world.get::<WheelBrakeInput>(car).unwrap();
    for i in 0..4 {
        let b = 0x460 + i * 0x68;
        for (name, offset, kind, v) in [
            ("steering", 0xC, 'h', steering.angle_turns[i] as f64 * 4096.0),
            ("response", 0x2A, 'h', wheels.speed_response[i] as f64 * 4096.0),
            ("spin", 0x20, 'H', contact.spin_turns[i] as f64 * 4096.0),
            ("grip", 0x34, 'i', grip.capacity[i] as f64),
            ("geometric slip", 0x50, 'h', geometric.angle_turns[i] as f64 * 4096.0),
            ("activation", 0x52, 'h', geometric.activation[i] as f64 * 4096.0),
            ("longitudinal slip", 0x44, 'h', longitudinal.ratio[i] as f64 * 4096.0),
            ("brake travel", 0x60, 'h', brakes.travel[i] as f64 * 4096.0),
            ("wheel force", 0x58, 'i', tyres.wheel_reference_force[i] as f64),
            ("ground force", 0x5C, 'i', tyres.ground_reference_force[i] as f64),
            ("lateral force", 0x54, 'i', tyres.lateral_force[i] as f64),
            ("surface", 0x14, 'B', road.surface[i] as f64),
            ("variant", 0x15, 'B', road.variant[i] as f64),
            ("road offset", 0x42, 'h', road.road_offset_m[i] as f64 * 4096.0),
            ("tread", 0x18, 'i', wheels.tread_speed_mps[i] as f64 * 4096.0),
            ("local forward", 0x2C, 'i', local.forward_mps[i] as f64 * 4096.0),
            ("local lateral", 0x30, 'i', local.lateral_mps[i] as f64 * 4096.0),
            ("wheel load", 8, 'i', suspension.tyre_load[i]),
            ("travel", 0x10, 'h', suspension.travel_m[i] as f64 * 4096.0),
            ("travel rate", 0x12, 'h', suspension.travel_rate_mps[i] as f64 * 4096.0),
            ("road height", 0, 'i', road.height_m[i] as f64 * 4096.0),
            ("raw road height", 4, 'i', road.raw_height_m[i] * 65536.0),
        ] {
            add(name, b + offset, kind, v);
        }
    }
    world.get_mut::<Phases>(car).unwrap().0[N] = out;
}
fn check(scenario: &str) {
    check_from(scenario, false);
}
fn check_from(scenario: &str, exported: bool) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/original/vehicle_loop").join(scenario);
    let recording: Recording =
        serde_json::from_str(&std::fs::read_to_string(root.join("recording.json")).unwrap()).unwrap();
    let mut app = headless_app(game::tick_rate(game::REGION));
    if recording.initial_automatic_mode != 0 {
        app.add_plugins(game::vehicle::automatic_stop::AutomaticStopRulesPlugin);
    }
    app.add_plugins((
        VehicleRulesPlugin,
        WheelSlipRulesPlugin,
        TyreGripRulesPlugin,
        LongitudinalSlipRulesPlugin,
        TyreForcesRulesPlugin,
        DriverAssistRulesPlugin,
        BrakeAssistRulesPlugin,
        BrakingRulesPlugin,
        EngineTorqueRulesPlugin,
        DrivetrainRulesPlugin,
        TransmissionRulesPlugin,
        WheelRotationRulesPlugin,
        ForwardDynamicsRulesPlugin,
        BodyDynamicsRulesPlugin,
    ));
    app.add_plugins((
        AerodynamicsRulesPlugin,
        RoadContactRulesPlugin,
        SuspensionRulesPlugin,
        VerticalBodyRulesPlugin,
        ContactMotionRulesPlugin,
        game::vehicle::visual_motion::VehicleVisualMotionPlugin,
    ));
    app.add_systems(
        FixedUpdate,
        (
            capture::<0>.after(VehicleSystems::Forces).before(VehicleSystems::BuildStep),
            capture::<1>.after(VehicleSystems::Integrate).before(VehicleSystems::SampleRoad),
            capture::<2>.after(VehicleSystems::SampleRoad).before(VehicleSystems::Suspension),
            capture::<3>.after(game::vehicle::visual_motion::VehicleVisualSystems::Stabilize),
        ),
    );
    let car = initialize(
        app.world_mut(),
        &Original::load(&root.join(&recording.ticks[0].before)),
        recording.alternate_locked_step,
    );
    let car = if exported {
        app.world_mut().despawn(car);
        let mut path = game::vehicle::initial::INITIAL_STATE_ASSET.to_owned();
        if recording.session.is_some() {
            let setup: game::race::RaceVehicle = engine::prelude::load_ron(game::race::RACE_VEHICLE_ASSET).unwrap();
            path = setup.initial_state.expect("default scene must select a native vehicle");
            app.insert_resource(setup.session.expect("default scene must select the countdown session"));
        }
        let state: game::vehicle::initial::VehicleInitialState = engine::prelude::load_ron(&path).unwrap();
        let car = state.spawn(app.world_mut());
        app.world_mut().entity_mut(car).insert(Phases::default());
        car
    } else {
        car
    };
    app.insert_resource(CarEntity(car));
    if recording.initial_automatic_mode != 0 {
        use game::vehicle::automatic_stop::*;
        app.world_mut().entity_mut(car).insert(AutomaticStop {
            phase: if recording.initial_automatic_mode == 7 {
                AutomaticStopPhase::Parked
            } else {
                AutomaticStopPhase::Braking
            },
            ..default()
        });
    }
    if let Some(session) = &recording.session {
        assert_eq!(session.clock % 100, 0, "clock must start at a complete physics tick");
        let mut native = game::race::session::DrivingSession::countdown(session.countdown_fields);
        native.elapsed_ticks = session.clock / 100;
        if !app.world().contains_resource::<game::race::session::DrivingSession>() {
            app.insert_resource(native);
        }
        app.add_plugins(game::race::session::SessionRulesPlugin);
        app.world_mut().entity_mut(car).insert(game::race::session::SessionParticipant);
    }
    for (index, tick) in recording.ticks.iter().enumerate() {
        assert_eq!(tick.run_frame, index as u32 * 2);
        let before = Original::load(&root.join(&tick.before));
        assert_eq!(
            app.world().get::<VehicleStepDuration>(car).unwrap().0,
            f32::from(before.short(0x6FE)) / 65536.0,
            "fixed physics duration at tick {index}"
        );
        assert_eq!(tick.controls.flags, 0);
        *app.world_mut().get_mut::<DigitalPedalInput>(car).unwrap() = DigitalPedalInput {
            throttle: tick.controls.throttle != 0,
            brake: tick.controls.brake != 0,
            handbrake: tick.controls.handbrake != 0,
        };
        *app.world_mut().get_mut::<DigitalGearInput>(car).unwrap() =
            DigitalGearInput { reverse: tick.controls.reverse != 0, shift_direction: tick.controls.shift };
        app.world_mut().get_mut::<DigitalSteeringInput>(car).unwrap().direction = tick.controls.steering as f32;
        app.run_tick(&[]);
        if recording.initial_automatic_mode != 0 {
            let parked = app.world().get::<game::vehicle::automatic_stop::AutomaticStop>(car).unwrap().phase
                == game::vehicle::automatic_stop::AutomaticStopPhase::Parked;
            assert_eq!(parked, tick.automatic_mode == 7, "automatic phase tick {index}");
        }
        if recording.session.is_some() {
            let session = app.world().resource::<game::race::session::DrivingSession>();
            let expected = tick.session.as_ref().expect("session boundary");
            let countdown = match session.phase {
                game::race::session::DrivingPhase::Countdown { fields_left } => fields_left,
                game::race::session::DrivingPhase::Driving => 0,
            };
            assert_eq!(countdown, expected.countdown_fields, "countdown tick {index}");
            assert_eq!(session.elapsed_ticks * 100, expected.clock, "clock tick {index}");
            assert_eq!(session.elapsed_millis(), u64::from(expected.clock) / 3, "display time tick {index}");
        }
        let phases = &app.world().get::<Phases>(car).unwrap().0;
        let mut differences = Vec::new();
        for (phase, (samples, file)) in
            phases.iter().zip([&tick.forces, &tick.moved, &tick.road, &tick.end]).enumerate()
        {
            assert!(!samples.is_empty(), "native phase {phase} did not run");
            let expected = Original::load(&root.join(file));
            for s in samples {
                let value = match s.kind {
                    'i' => expected.word(s.off) as f64,
                    'h' => expected.short(s.off) as f64,
                    'B' => expected.byte(s.off) as f64,
                    'H' => expected.short(s.off) as u16 as f64,
                    _ => unreachable!(),
                };
                if s.value != value {
                    differences
                        .push(format!("phase {phase}: {} +{:X}: native {} original {value}", s.name, s.off, s.value));
                }
            }
        }
        assert!(differences.is_empty(), "{scenario} tick {index}:\n{}", differences.join("\n"));
    }
    assert!(recording.ticks.len() >= 30);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original countdown recordings")]
fn original_countdown_to_throttle_launch() {
    check("countdown-gas");
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires exported default scene and original countdown recordings")]
fn exported_default_scene_reproduces_original_countdown_trajectories() {
    for scenario in ["countdown-gas", "countdown-coast", "countdown-fresh-launch-holdout"] {
        check_from(scenario, true);
    }
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original countdown recordings")]
fn original_countdown_to_idle() {
    check("countdown-coast");
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original long steering countdown recording")]
fn original_standing_start_with_continuous_steering() {
    check_from("countdown-right-start", true);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original automatic result braking recordings")]
fn original_automatic_braking_after_success_and_failure() {
    check("post-gold-gas");
    check("post-overshoot-gas");
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires fresh original automatic braking holdout")]
fn original_fresh_automatic_braking_ignores_player_controls() {
    check("post-fresh-stop-holdout");
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original countdown recordings")]
fn original_fresh_countdown_launch_holdout() {
    check("countdown-fresh-launch-holdout");
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original whole-vehicle recordings")]
fn original_continuous_b1_gas() {
    check("gas");
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original whole-vehicle recordings")]
fn original_continuous_b1_braking() {
    check("brake");
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original whole-vehicle recordings")]
fn original_continuous_b1_coast() {
    check("coast");
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original whole-vehicle holdout")]
fn fresh_original_continuous_b1_turning_and_mixed_pedals() {
    check("holdout");
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires exported initial state and original trajectory")]
fn native_exported_initial_asset_reproduces_original_trajectory() {
    for scenario in ["gas", "coast", "brake", "holdout"] {
        check_from(scenario, true);
    }
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original stationary B-1 recordings")]
fn original_standing_start_and_brake_release_launch() {
    check("standing-gas");
    check("standing-brake-launch");
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original B-1 reverse/forward recording")]
fn original_reverse_then_forward_from_idle() {
    check("standing-reverse-forward");
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original manual-transmission B-1 recording")]
fn original_manual_shift_requests_from_idle() {
    check("manual-shifts");
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires fresh original B-1 launch holdout")]
fn fresh_original_brake_rev_reverse_and_turn_launch_holdout() {
    check("standing-launch-holdout");
}
