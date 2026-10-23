//! Body-motion stability signals and ordinary traction throttle reduction.
//!
//! 8003E4FC..E688 prepares the signals shared by brake assistance and wheel
//! torque. 8003DFDC/DE68 then reduces effective throttle on driven-wheel slip.
//! Motion, contact and logical requests remain independent producers.
use super::{
    PedalTravel, SteeringResponse, VehicleSystems, VehicleVelocity,
    body_dynamics::BodyDynamics,
    brake_assist::BrakeAssistInput,
    braking::BrakingSystems,
    contact_motion::floor_low_product,
    direction::direction_turns,
    drivetrain::{DriveLayout, DrivetrainInput, DrivetrainSetup, DrivetrainTuning, EngineEngagement},
    engine_torque::{EngineTorqueInput, EngineTorqueSystems, EngineTorqueTuning},
    forward_dynamics::ForwardDynamicsInput,
    longitudinal_slip::{LongitudinalSlip, steering_basis},
    transmission::TransmissionSystems,
    tyre_forces::TyreContactState,
    wheel_rotation::WheelRotationInput,
};
use bevy::{ecs::query::QueryData, prelude::*};
use engine::{prelude::TuningPlugin, shell::GameSystems};
use serde::{Deserialize, Serialize};

pub const DRIVER_ASSIST_TUNING: &str = "vehicle/driver_assist.ron";

#[derive(Resource, Clone, Debug, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct DriverAssistTuning {
    /// Physics+10 /4096; 8003E560 uses the wheelbase for desired yaw.
    pub wheelbase_m: f32,
    /// 8003E574..598 builds 25736/4096 radians per turn.
    pub radians_per_turn: f32,
    pub angle_threshold_ratios: Vec<f32>,
    /// Physics+450 /4096, +454 /4096 and +456 /4096. A zero fade disables
    /// ordinary traction assistance at the caller, regardless of strength.
    pub traction_strength: f32,
    pub feedback_fade_per_turn: f32,
    pub speed_suppression: f32,
    /// Physics+1FC/+2D4, /4096; target follows the lowest loaded driven slip.
    pub traction_slip_target: [f32; 2],
    /// Physics+458/+45A /4096, passed to the ordinary brake consumer.
    pub brake_correction_gain: f32,
    pub brake_correction_threshold: f32,
}
#[derive(Component, Clone, Debug, Reflect, Deserialize, Serialize)]
#[reflect(Component)]
pub struct DriverAssistSetup(pub DriverAssistTuning);

#[derive(Component, Default, Clone, Copy, Debug, Reflect)]
#[reflect(Component)]
pub struct DriverAssistInput {
    /// Retained contact motion, +6A4/+6A8, metres/s.
    pub forward_mps: f32,
    pub lateral_mps: f32,
    pub yaw_turns_per_second: f32,
    pub steering_turns: f32,
    /// Lateral acceleration feedback (+64A), sampled before this tick's forces.
    pub lateral_feedback_turns: f32,
    /// Speed-dependent response (+700 /4096); its upstream producer is separate.
    pub speed_response: f32,
    pub effective_throttle: f32,
    pub available_torque: f32,
    pub engine_coupled: bool,
    pub reverse_gear: bool,
    /// Ordinary layouts 0/5 use rear, 1 uses front, the others inspect both.
    pub driven_axles: [bool; 2],
    pub wheel_load: [f32; 4],
    pub longitudinal_slip: [f32; 4],
}

#[derive(Component, Default, Clone, Copy, Debug, Reflect)]
#[reflect(Component)]
pub struct DriverAssist {
    /// Scratch+90/+92, normalized /4096; both reset every call.
    pub yaw_shortfall: f32,
    pub lateral_correction: f32,
    pub effective_throttle: f32,
    pub throttle_scale: f32,
}

impl DriverAssistTuning {
    pub fn evaluate(&self, input: DriverAssistInput) -> DriverAssist {
        let mut result =
            DriverAssist { effective_throttle: input.effective_throttle, throttle_scale: 1.0, ..default() };
        if input.forward_mps > 0.0 {
            if aligned_turn_product(input.steering_turns, input.yaw_turns_per_second) {
                let sign = if input.steering_turns < 0.0 { -1.0 } else { 1.0 };
                let sine = steering_basis(input.steering_turns.abs())[1];
                let sideways = floor_grid(input.forward_mps as f64 * sine as f64);
                let circumference = floor_low_product(self.wheelbase_m, self.radians_per_turn * 4096.0, 12) / 4096.0;
                // 8003E5A8 tests the truncated /4096-turn/s result before
                // shifting to the yaw-rate grid. Preserve the low-word shift.
                let desired = stored_yaw_rate(sideways as f64 / circumference as f64);
                let actual = sign * input.yaw_turns_per_second;
                if desired != 0.0 && actual < desired {
                    result.yaw_shortfall = stored_response(1.0 - trunc_grid(actual as f64 / desired as f64));
                }
            }
            let angle = direction_turns(&self.angle_threshold_ratios, input.forward_mps, -input.lateral_mps);
            if angle > 0.0 && input.yaw_turns_per_second <= 0.0 || angle < 0.0 && input.yaw_turns_per_second >= 0.0 {
                // 8003E658 calls 80075C54 with shift=2: angle * forward/4,
                // floored on the response grid, then upper-clamped to one.
                result.lateral_correction = floor_grid(angle.abs() as f64 * input.forward_mps as f64 / 4.0).min(1.0);
            }
        }
        if self.feedback_fade_per_turn != 0.0
            && input.engine_coupled
            && input.available_torque > 0.0
            && input.effective_throttle != 0.0
        {
            // 8003DFDC: fade only beyond a quarter turn of lateral feedback.
            // Negative authored fade may exceed one; there is no upper cap.
            let above = input.lateral_feedback_turns.abs() - 0.25;
            let feedback = if above < 0.0 { 1.0 } else { (1.0 - product(self.feedback_fade_per_turn, above)).max(0.0) };
            let mut lowest = 1.0;
            let mut target = 1.0;
            for axle in 0..2 {
                if !input.driven_axles[axle] {
                    continue;
                }
                for wheel in axle * 2..axle * 2 + 2 {
                    if input.wheel_load[wheel] != 0.0 && input.longitudinal_slip[wheel] < lowest {
                        lowest = input.longitudinal_slip[wheel];
                        target = self.traction_slip_target[axle];
                    }
                }
            }
            let suppression =
                if input.reverse_gear { 0.0 } else { product(input.speed_response, self.speed_suppression) };
            let strength = product(self.traction_strength, 1.0 - suppression);
            // The final error/strength product truncates (8007596C), unlike
            // the preceding floor products. This affects throttle cut ticks.
            result.throttle_scale =
                (1.0 + trunc_grid((lowest - product(target, feedback)) as f64 * strength as f64)).clamp(0.0, 1.0);
            result.effective_throttle = stored_response(product(result.throttle_scale, input.effective_throttle));
        }
        result
    }
}

/// 8003E520 tests the sign of the LOW product of two distinct angle/rate
/// grids. An overflowing product can disagree with an ordinary sign test.
fn aligned_turn_product(steering: f32, yaw: f32) -> bool {
    let angle = (steering as f64 * 4096.0).round() as i64;
    let rate = (yaw as f64 * 524288.0).round() as i64;
    angle.wrapping_mul(rate) as i32 >= 0
}
fn stored_yaw_rate(value: f64) -> f32 {
    ((value * 4096.0).trunc() as i32).wrapping_shl(7) as f32 / 524288.0
}
fn stored_response(value: f32) -> f32 {
    (value as f64 * 4096.0).round() as i64 as i16 as f32 / 4096.0
}
fn product(a: f32, b: f32) -> f32 {
    floor_low_product(a, b * 4096.0, 12) / 4096.0
}
fn floor_grid(value: f64) -> f32 {
    ((value * 4096.0).floor() / 4096.0) as f32
}
fn trunc_grid(value: f64) -> f32 {
    ((value * 4096.0).trunc() / 4096.0) as f32
}

/// Opt into the ordinary pedal-to-effective-throttle copy (8003E2DC).
/// Replay/AI owners can omit it and supply their own effective request.
#[derive(Component, Default, Clone, Copy, Debug, Reflect)]
#[reflect(Component)]
pub struct PedalThrottle;

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DriverAssistSystems {
    Prepare,
}
pub struct DriverAssistRulesPlugin;
impl Plugin for DriverAssistRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<DriverAssistTuning>() {
            app.add_plugins(TuningPlugin::<DriverAssistTuning>::new(DRIVER_ASSIST_TUNING));
        }
        app.register_type::<DriverAssist>()
            .register_type::<DriverAssistInput>()
            .register_type::<PedalThrottle>()
            .configure_sets(
                FixedUpdate,
                DriverAssistSystems::Prepare
                    .in_set(VehicleSystems::Forces)
                    .in_set(GameSystems::Game)
                    .after(BrakingSystems::CopyPedals)
                    .before(BrakingSystems::Assistance)
                    .before(BrakingSystems::Allocate)
                    .before(EngineTorqueSystems::Prepare)
                    .before(TransmissionSystems::Prepare),
            )
            .add_systems(FixedUpdate, copy_throttle.in_set(BrakingSystems::CopyPedals).in_set(GameSystems::Game))
            .add_systems(FixedUpdate, prepare.in_set(DriverAssistSystems::Prepare));
    }
}
fn copy_throttle(mut cars: Query<(&PedalTravel, &mut EngineTorqueInput), With<PedalThrottle>>) {
    for (pedals, mut engine) in &mut cars {
        engine.effective_throttle = pedals.throttle;
    }
}
#[derive(QueryData)]
#[query_data(mutable)]
struct AssistCar {
    input: &'static DriverAssistInput,
    state: &'static mut DriverAssist,
    setup: Option<&'static DriverAssistSetup>,
    contact: Option<&'static TyreContactState>,
    body: Option<&'static BodyDynamics>,
    velocity: Option<&'static VehicleVelocity>,
    steering: Option<&'static SteeringResponse>,
    forward: Option<&'static ForwardDynamicsInput>,
    slip: Option<&'static LongitudinalSlip>,
    drive: Option<&'static DrivetrainInput>,
    drivetrain: Option<&'static DrivetrainSetup>,
    engine: Option<&'static mut EngineTorqueInput>,
    brakes: Option<&'static mut BrakeAssistInput>,
    rotation: Option<&'static mut WheelRotationInput>,
}
fn prepare(
    tuning: Res<DriverAssistTuning>,
    engine_tuning: Option<Res<EngineTorqueTuning>>,
    drive_tuning: Option<Res<DrivetrainTuning>>,
    mut cars: Query<AssistCar>,
) {
    for mut car in &mut cars {
        let tuning = car.setup.map(|v| &v.0).unwrap_or(&tuning);
        let mut input = *car.input;
        if let Some(contact) = car.contact {
            input.forward_mps = contact.body_forward_mps;
        }
        if let Some(body) = car.body {
            input.lateral_mps = body.local_lateral_mps;
            input.lateral_feedback_turns = body.lateral_feedback_turns;
        }
        if let Some(velocity) = car.velocity {
            input.yaw_turns_per_second = velocity.yaw_turns_per_second;
        }
        if let Some(steering) = car.steering {
            input.steering_turns = steering.angle_turns;
        }
        if let Some(forward) = car.forward {
            input.wheel_load = forward.wheel_load;
        }
        if let Some(slip) = car.slip {
            input.longitudinal_slip = slip.ratio;
        }
        if let Some(drive) = car.drive {
            input.engine_coupled = drive.engagement == EngineEngagement::Coupled;
            input.reverse_gear = drive.gear == 0;
            let setup = car.drivetrain.map(|v| &v.0).or(drive_tuning.as_deref());
            if let Some(setup) = setup {
                input.driven_axles = match setup.layout {
                    DriveLayout::Rear | DriveLayout::AdaptiveClutch => [false, true],
                    DriveLayout::Front => [true, false],
                    _ => [true, true],
                };
            }
        }
        if let Some(engine) = car.engine.as_ref() {
            input.effective_throttle = engine.effective_throttle;
            if let Some(tuning) = &engine_tuning {
                input.available_torque = tuning.available_torque(engine.speed_rev_s);
            }
        }
        *car.state = tuning.evaluate(input);
        if let Some(mut engine) = car.engine {
            engine.effective_throttle = car.state.effective_throttle;
        }
        if let Some(mut brakes) = car.brakes {
            brakes.yaw_shortfall = car.state.yaw_shortfall;
            brakes.lateral_correction = car.state.lateral_correction;
            brakes.positive_yaw = input.yaw_turns_per_second > 0.0;
            brakes.correction_gain = tuning.brake_correction_gain;
            brakes.correction_threshold = tuning.brake_correction_threshold;
        }
        if let Some(mut rotation) = car.rotation {
            rotation.yaw_correction = car.state.yaw_shortfall;
            rotation.steering_turns = input.steering_turns;
        }
    }
}
