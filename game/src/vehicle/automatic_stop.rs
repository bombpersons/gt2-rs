//! Shared automatic braking/parking after a licence result (80037664..37834).
//! Route-following AI can supply another steering target; ordinary B-1 result
//! stops use zero. No renderer, session or licence resources are required.
use super::{
    DigitalGearInput, PedalTravel, SteeringResponse, VehicleStepDuration, VehicleSystems, VehicleVelocity,
    body_dynamics::{BodyDynamics, BodyDynamicsInput},
    braking::WheelBrakeInput,
    drivetrain::{DrivetrainInput, EngineEngagement},
    engine_torque::EngineTorqueInput,
    suspension::SuspensionInput,
    transmission::{TransmissionInput, TransmissionState, TransmissionSystems},
};
use bevy::{ecs::query::QueryData, prelude::*};
use engine::prelude::*;
use serde::{Deserialize, Serialize};

pub const AUTOMATIC_STOP_TUNING: &str = "vehicle/automatic_stop.ron";

/// Exported from the selected car and the original shared stopping routine.
#[derive(Resource, Clone, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct AutomaticStopTuning {
    /// Car +0x62 (/4096 turns/s), applied at 80037664.
    pub steering_rate_turns_per_second: f32,
    /// Strict speed limit at 8003775C, after 8003C398's approximate norm.
    pub settled_linear_mps: f32,
    /// Strict rotation limit at 80037768, after signed axis narrowing.
    pub settled_angular_turns_per_second: f32,
    /// Three quarters of car +0x396 RPM; strict comparison at 80038664.
    pub reengage_above_rpm: f32,
    /// Idle RPM at car +0x10A; inclusive comparison at 80038684..390.
    pub disengage_below_rpm: f32,
}
#[derive(Component, Clone, Debug, Reflect)]
#[reflect(Component)]
pub struct AutomaticStopSetup(pub AutomaticStopTuning);

/// Another driver owns pedal/steering state. Native player input and its
/// spring/ramp rules leave this car alone; force and gear solvers still run.
#[derive(Component, Default, Reflect)]
#[reflect(Component)]
pub struct ExternalDriver;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum AutomaticStopPhase {
    #[default]
    Braking,
    Parked,
}
#[derive(Component, Default, Clone, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct AutomaticStop {
    pub phase: AutomaticStopPhase,
    pub steering_target_turns: f32,
}

impl AutomaticStop {
    pub fn advance(
        &mut self,
        tuning: &AutomaticStopTuning,
        linear: Vec3,
        angular: Vec3,
        seconds: f32,
        steering: &mut SteeringResponse,
        pedals: &mut PedalTravel,
    ) {
        // 80037664: a constant steering slew, independent of the player's
        // spring accumulator. Keep its positive /4096-turn step rounding.
        let step =
            ((f64::from(tuning.steering_rate_turns_per_second) * f64::from(seconds) * 4096.0).floor() / 4096.0) as f32;
        let target = if self.phase == AutomaticStopPhase::Parked { 0.0 } else { self.steering_target_turns };
        let delta = (target - steering.angle_turns).clamp(-step, step);
        steering.angle_turns += delta;
        // Yaw is narrowed from /524288, tilt from /65536, by signed truncation
        // before the shared three-axis speed estimate (80037700..758).
        let angular = angular.map(|v| ((f64::from(v) * 4096.0).trunc() / 4096.0) as f32);
        if self.phase == AutomaticStopPhase::Braking
            && super::approximate_magnitude(linear) < f64::from(tuning.settled_linear_mps)
            && super::approximate_magnitude(angular) < f64::from(tuning.settled_angular_turns_per_second)
        {
            self.phase = AutomaticStopPhase::Parked;
        }
        pedals.throttle = 0.0;
        pedals.handbrake = 0.0;
        pedals.brake = if self.phase == AutomaticStopPhase::Braking { 1.0 } else { 0.0 };
    }
}

pub struct AutomaticStopRulesPlugin;
impl Plugin for AutomaticStopRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<AutomaticStopTuning>()
            && engine::config::asset_path(AUTOMATIC_STOP_TUNING).is_file()
        {
            app.add_plugins(TuningPlugin::<AutomaticStopTuning>::new(AUTOMATIC_STOP_TUNING));
        }
        app.register_type::<AutomaticStop>()
            .register_type::<AutomaticStopSetup>()
            .register_type::<ExternalDriver>()
            .add_observer(acquire)
            .add_observer(release)
            .configure_sets(
                FixedUpdate,
                (VehicleSystems::ReadInput, VehicleSystems::Pedals)
                    .chain()
                    .in_set(EngineSet::Control)
                    .in_set(GameSystems::Game),
            )
            .add_systems(FixedUpdate, control.in_set(VehicleSystems::Pedals).in_set(GameSystems::Game))
            .add_systems(
                FixedUpdate,
                engagement
                    .in_set(VehicleSystems::ReadInput)
                    .before(TransmissionSystems::DriverRequests)
                    .in_set(GameSystems::Game),
            );
    }
}
fn acquire(add: On<Add<AutomaticStop>>, mut commands: Commands) {
    commands.entity(add.event_target()).try_insert(ExternalDriver);
}
fn release(
    remove: On<Remove<AutomaticStop>>,
    mut commands: Commands,
    mut q: Query<(Option<&mut BodyDynamicsInput>, Option<&mut SuspensionInput>, Option<&mut TransmissionInput>)>,
) {
    let e = remove.event_target();
    commands.entity(e).try_remove::<ExternalDriver>();
    if let Ok((body, suspension, transmission)) = q.get_mut(e) {
        if let Some(mut body) = body {
            body.reset_motion = false;
        }
        if let Some(mut suspension) = suspension {
            suspension.disabled = false;
        }
        if let Some(mut transmission) = transmission {
            transmission.force_automatic = false;
        }
    }
}
#[derive(QueryData)]
#[query_data(mutable)]
struct Car {
    stop: &'static mut AutomaticStop,
    steering: &'static mut SteeringResponse,
    pedals: &'static mut PedalTravel,
    duration: &'static VehicleStepDuration,
    velocity: Option<&'static VehicleVelocity>,
    body: Option<&'static BodyDynamics>,
    setup: Option<&'static AutomaticStopSetup>,
    body_input: Option<&'static mut BodyDynamicsInput>,
    suspension: Option<&'static mut SuspensionInput>,
    gears: Option<&'static mut DigitalGearInput>,
    transmission: Option<&'static mut TransmissionInput>,
    drive: Option<&'static mut DrivetrainInput>,
    brakes: Option<&'static mut WheelBrakeInput>,
}
fn control(tuning: Option<Res<AutomaticStopTuning>>, mut cars: Query<Car>) {
    for mut car in &mut cars {
        let tuning = car.setup.map(|s| &s.0).or(tuning.as_deref()).expect("AutomaticStop requires its tuning");
        let linear = car.velocity.map_or(Vec3::ZERO, |v| v.linear_mps);
        let angular = Vec3::new(
            car.body.map_or(0.0, |b| b.tilt_turns_per_second.x),
            car.velocity.map_or(0.0, |v| v.yaw_turns_per_second),
            car.body.map_or(0.0, |b| b.tilt_turns_per_second.y),
        );
        car.stop.advance(tuning, linear, angular, car.duration.0, &mut car.steering, &mut car.pedals);
        let parked = car.stop.phase == AutomaticStopPhase::Parked;
        if let Some(mut input) = car.body_input {
            input.reset_motion = parked;
        }
        if let Some(mut input) = car.suspension {
            input.disabled = parked;
        }
        if let Some(mut gears) = car.gears {
            *gears = DigitalGearInput::default();
        }
        if let Some(mut transmission) = car.transmission {
            transmission.force_automatic = true;
            transmission.accelerator_travel = 0.0;
            transmission.reverse_requested = false;
            transmission.shift_direction = 0;
        }
        if let Some(mut drive) = car.drive {
            drive.brake_travel = car.pedals.brake;
        }
        if let Some(mut brakes) = car.brakes {
            *brakes = WheelBrakeInput::from_pedals(*car.pedals);
        }
    }
}
fn engagement(
    tuning: Option<Res<AutomaticStopTuning>>,
    mut cars: Query<(
        &AutomaticStop,
        &EngineTorqueInput,
        &DrivetrainInput,
        &mut TransmissionState,
        Option<&AutomaticStopSetup>,
    )>,
) {
    for (stop, engine, drive, mut state, setup) in &mut cars {
        let tuning = setup.map(|s| &s.0).or(tuning.as_deref()).expect("AutomaticStop requires its tuning");
        // Ordinary multi-ratio automatic driver 80038630..390. Its clutch
        // request differs from the player's 8002F92C producer.
        state.engagement_requested = stop.phase != AutomaticStopPhase::Parked
            && if drive.engagement == EngineEngagement::Disconnected {
                engine.integer_rpm > tuning.reengage_above_rpm
            } else {
                engine.integer_rpm >= tuning.disengage_below_rpm
            };
    }
}
