//! Verified 30 Hz controls, engine torque, step construction and integration.
//!
//! Run this plugin in a schedule stepping once per two NTSC video fields.
//! The application's 60 Hz presentation/menu schedule must not run these
//! systems twice per physics tick. See `docs/driving-evidence.md` for original
//! recordings, the input boundary, and the remaining tyre/drivetrain work.
//!
//! Force rules join [`VehicleSystems::Forces`]; body dynamics supplies
//! [`VehicleVelocity`] before step construction. Suspension supplies next-tick
//! loads; decoded road contact supplies surface/heights and retained bases.
//! Vertical body rules update tilt/velocity and correct compression stops.
//! Aerodynamics supplies drag and axle loads before control/force evaluation.
use bevy::prelude::*;
use engine::controller::{Button, ControllerState};
use engine::prelude::TuningPlugin;
use engine::shell::GameSystems;
use serde::{Deserialize, Serialize};

pub mod aerodynamics;
pub mod body_dynamics;
pub mod brake_assist;
pub mod braking;
pub mod contact_motion;
pub mod direction;
pub mod driver_assist;
pub mod drivetrain;
pub mod engine_torque;
pub mod forward_dynamics;
pub mod longitudinal_slip;
pub mod presentation;
pub mod road_contact;
pub mod slip;
mod steering;
pub mod suspension;
pub mod transmission;
pub mod tyre_forces;
pub mod tyre_grip;
pub mod vertical_body;
pub mod wheel_rotation;
pub use steering::{
    DigitalSteeringInput, STEERING_TUNING, SteeringKinematics, SteeringKnot, SteeringResponse, SteeringTuning,
    WheelSteering,
};

/// Moddable digital pedal rates, in fractions of full travel per 30 Hz tick.
/// The original's first two car slots share these values; additional slots
/// and analog controllers have not been established by these recordings.
#[derive(Resource, Debug, Clone, Copy, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct PedalTuning {
    pub throttle_press: f32,
    pub throttle_release: f32,
    pub brake_press: f32,
    pub brake_release: f32,
    pub handbrake_press: f32,
    pub handbrake_release: f32,
}

/// Original player pedal parameters, loaded through the normal asset layers.
pub const PEDAL_TUNING: &str = "vehicle/pedals.ron";

/// Original 30 Hz physics duration, car+0x6FE = 2184 in 16.16 seconds.
/// This differs slightly from 1/30 due to the original duration's precision.
pub const PHYSICS_STEP_SECONDS: f32 = 2184.0 / 65536.0;

/// Effective digital requests at the physics boundary, after pad sampling.
/// Multiple held pedals remain independent, as in `0x8002FB18`.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct DigitalPedalInput {
    pub throttle: bool,
    pub brake: bool,
    pub handbrake: bool,
}

/// Opt into reading the logical player pad for pedals and steering. Without
/// this marker, replay/AI systems can supply the effective requests themselves.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct PlayerPedals;

/// Pedal travel in 0..=1. Original fields are car+0x610, +0x612 and +0x614
/// (signed halfwords, 4096 = full travel). This is control state, not force.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct PedalTravel {
    pub throttle: f32,
    pub brake: f32,
    pub handbrake: f32,
}

impl PedalTravel {
    /// Move each pedal towards its held/released limit independently.
    /// Digital paths `0x80030008..44`, `0x800300A8..128` and
    /// `0x80030178..1F4` add/subtract the tuning rate, then clamp to 0..4096.
    pub fn advance(&mut self, input: DigitalPedalInput, tuning: &PedalTuning) {
        self.throttle = travel(self.throttle, input.throttle, tuning.throttle_press, tuning.throttle_release);
        self.brake = travel(self.brake, input.brake, tuning.brake_press, tuning.brake_release);
        self.handbrake = travel(self.handbrake, input.handbrake, tuning.handbrake_press, tuning.handbrake_release);
    }
}

fn travel(current: f32, held: bool, press: f32, release: f32) -> f32 {
    (current + if held { press } else { -release }).clamp(0.0, 1.0)
}

/// Physics pose in world metres and turns. Position uses Bevy's (x,y,z)
/// order; original car+0x65C/+0x660/+0x664 is (x,z,y), 4096 units/metre,
/// with original z negated at the boundary to match the course's coordinates.
/// Heading keeps the original's signed turn representative because it is
/// observable: +0x648 is wrapped at +/-4096, rather than into +/-2048.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct VehiclePose {
    pub position_m: Vec3,
    pub heading_turns: f32,
}

impl VehiclePose {
    /// Apply an already-computed step. `0x80033EAC..33F10` adds translation
    /// first, then yaw, wrapping yaw by one turn at each signed boundary.
    /// Road contact and collision correction happen later in the original
    /// and are not part of this operation.
    pub fn advance(&mut self, step: MotionStep) {
        self.position_m += step.translation_m;
        self.heading_turns += step.heading_turns;
        if self.heading_turns >= 1.0 {
            self.heading_turns -= 1.0;
        }
        if self.heading_turns <= -1.0 {
            self.heading_turns += 1.0;
        }
    }
}

/// One pending physics step, consumed by the integration system. Original
/// `0x80030330` fills scratchpad 0x1F8000B4..0x1F8000C0 before calling
/// `0x80033E6C`. The native step builder derives it from velocity; force/tyre
/// rules that produce that velocity remain unfinished.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct MotionStep {
    pub translation_m: Vec3,
    pub heading_turns: f32,
}

/// World-space velocity in Bevy coordinates and yaw rate in turns/second.
/// The source stores linear velocity at car+0x628/62C/630 in 1/4096 m/s,
/// and yaw rate at +0x64C in 1/524288 turn/s. The yaw scale follows from
/// `800303D8..800303F0`: product with duration / 2^23 becomes 1/4096 turn.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct VehicleVelocity {
    pub linear_mps: Vec3,
    pub yaw_turns_per_second: f32,
}

impl VehicleVelocity {
    /// `80030330` constructs a step using the velocities *after* forces,
    /// which may differ from either frame-boundary velocity sample. Keep
    /// signed truncation to the original 1/4096 metre/turn step grid, as
    /// these steps accumulate into the exact observable body trajectory.
    pub fn step(self, seconds: f32) -> MotionStep {
        let step = |velocity: f32| ((velocity as f64 * seconds as f64 * 4096.0).trunc() / 4096.0) as f32;
        MotionStep { translation_m: self.linear_mps.map(step), heading_turns: step(self.yaw_turns_per_second) }
    }
}

/// Per-car integration duration, car+0x6FE in 1/65536 seconds. It is
/// distinct from the host's scheduling interval (30 Hz for this race).
#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct VehicleStepDuration(pub f32);

impl Default for VehicleStepDuration {
    fn default() -> Self {
        Self(PHYSICS_STEP_SECONDS)
    }
}

/// Slots for a 30 Hz vehicle schedule. The original first processes input in
/// `0x8002FB18`, calculates forces/velocity in `0x80039FC8`, constructs the
/// step in `0x80030330`, and integrates in `0x80034320` -> `0x80033E6C`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VehicleSystems {
    Prepare,
    ReadInput,
    Pedals,
    SteerWheels,
    ContactSlip,
    Forces,
    BuildStep,
    Integrate,
    SampleRoad,
    Suspension,
    CorrectBody,
    RefreshContact,
}

/// Rules-only verified subset. Requires the tuning file unless the caller
/// inserts [`PedalTuning`] first. Does not spawn a car, render, or infer forces.
pub struct VehicleRulesPlugin;

impl Plugin for VehicleRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<PedalTuning>() {
            app.add_plugins(TuningPlugin::<PedalTuning>::new(PEDAL_TUNING));
        }
        if !app.world().contains_resource::<SteeringTuning>() {
            app.add_plugins(TuningPlugin::<SteeringTuning>::new(STEERING_TUNING));
        }
        app.init_resource::<ControllerState>()
            .configure_sets(
                FixedUpdate,
                (
                    VehicleSystems::Prepare,
                    VehicleSystems::ReadInput,
                    VehicleSystems::Pedals,
                    VehicleSystems::SteerWheels,
                    VehicleSystems::ContactSlip,
                    VehicleSystems::Forces,
                    VehicleSystems::BuildStep,
                    VehicleSystems::Integrate,
                    VehicleSystems::SampleRoad,
                    VehicleSystems::Suspension,
                    VehicleSystems::CorrectBody,
                    VehicleSystems::RefreshContact,
                )
                    .chain()
                    .in_set(engine::prelude::EngineSet::Control)
                    .in_set(GameSystems::Game),
            )
            .add_systems(
                FixedUpdate,
                (read_player_pedals, steering::read_player_steering).in_set(VehicleSystems::ReadInput),
            )
            .add_systems(FixedUpdate, (advance_pedals, steering::advance_steering).in_set(VehicleSystems::Pedals))
            .add_systems(FixedUpdate, steering::steer_wheels.in_set(VehicleSystems::SteerWheels))
            .add_systems(FixedUpdate, build_motion_steps.in_set(VehicleSystems::BuildStep))
            .add_systems(FixedUpdate, integrate_motion.in_set(VehicleSystems::Integrate));
    }
}

fn read_player_pedals(pad: Res<ControllerState>, mut players: Query<&mut DigitalPedalInput, With<PlayerPedals>>) {
    // Default pad mapping, verified by original CROSS / SQUARE / CIRCLE runs.
    // Physical key/gamepad rebinding remains at ControllerState's boundary.
    for mut input in &mut players {
        *input = DigitalPedalInput {
            throttle: pad.held(Button::Cross),
            brake: pad.held(Button::Square),
            handbrake: pad.held(Button::Circle),
        };
    }
}

fn advance_pedals(tuning: Res<PedalTuning>, mut pedals: Query<(&DigitalPedalInput, &mut PedalTravel)>) {
    for (&input, mut travel) in &mut pedals {
        travel.advance(input, &tuning);
    }
}

fn integrate_motion(mut vehicles: Query<(&mut VehiclePose, &mut MotionStep)>) {
    for (mut pose, mut step) in &mut vehicles {
        pose.advance(std::mem::take(&mut *step));
    }
}

fn build_motion_steps(mut vehicles: Query<(&VehicleVelocity, &VehicleStepDuration, &mut MotionStep)>) {
    for (&velocity, &duration, mut step) in &mut vehicles {
        *step = velocity.step(duration.0);
    }
}
