//! Wheel reaction torque and differential-controlled tread-speed integration.
//!
//! Original 80045688, 800457B0 and 800465E0. The intervening axle/engine
//! coupling (80045AE8) supplies AxleDrive through the drivetrain feature;
//! this plugin can also consume an explicitly supplied axle-drive boundary.
//! Speeds are metres/second; force/torque retain the original normalized
//! units. See docs/wheel-rotation-evidence.md for the observed boundaries.
use super::{
    VehicleStepDuration, VehicleSystems,
    braking::{BrakingSystems, WheelBrakeDemand},
    forward_dynamics::{ForwardDynamics, ForwardDynamicsSystems},
    longitudinal_slip::WheelSurfaceMotion,
    tyre_forces::TyreForces,
};
use bevy::prelude::*;
use engine::{prelude::TuningPlugin, shell::GameSystems};
use serde::{Deserialize, Serialize};

pub const WHEEL_ROTATION_TUNING: &str = "vehicle/wheel_rotation.ron";

#[derive(Default, Debug, Clone, Copy, Reflect, Deserialize, Serialize)]
pub enum Differential {
    /// Source mode 0: each wheel integrates its own reaction torque.
    Independent,
    /// Source mode 1: both wheels follow the axle's angular velocity.
    Locked,
    #[default]
    /// Source mode 2: reaction-torque difference evolves the wheel split.
    Open,
    /// Source modes 3/4 share preload and drive/coast ramps with a lock latch.
    Clutch { preload_torque: f32, drive_response: f32, coast_response: f32 },
    /// Source mode 5: transfer rises with squared tread-speed difference.
    Viscous { slip_squared_limit_mps2: f32, torque_per_squared_mps: f32 },
    /// Source modes 6/7 share the speed-gated yaw correction at 800457B0.
    YawBiased { response: f32 },
}

#[derive(Resource, Debug, Clone, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct WheelRotationTuning {
    /// Physics+3E4/+3E6 /4096; converts axle angular speed to tread m/s.
    pub radius_m: [f32; 2],
    /// Physics+3F8/+3FC /512 (80075B04 with exponent -3). Independent
    /// wheels use twice this response (exponent -4).
    pub rotation_response: [f32; 2],
    pub differentials: [Differential; 2],
    /// Positive forward-speed gate at 80045854; reverse does not qualify.
    pub yaw_bias_min_forward_mps: f32,
}

/// Output of the intervening transmission/axle solver. Speeds are axle
/// angular speed (+634/+638 /4096); torque is scratch+74/+78 in source units.
#[derive(Component, Default, Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
#[serde(default)]
pub struct AxleDrive {
    pub radians_per_second: [f32; 2],
    pub torque: [f32; 2],
}

#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct WheelRotationInput {
    /// Ground-reference response at scratch wheel+8. External tyre owners
    /// supply it here; TyreWheelFeedback opts into the native tyre result.
    pub auxiliary_tread_force: [f32; 4],
    /// Signed scratch+90 /4096, shared with yaw-sensitive brake assistance.
    pub yaw_correction: f32,
    /// Physics+60C; only its sign is used by the yaw-bias preparation.
    pub steering_turns: f32,
}

/// Previous per-wheel velocity increment (+16 /4096) and each axle's
/// persistent clutch-differential latch (+61A/+61B).
/// For coupled axles only the left increment belongs to this rule. The
/// original axle solver reuses the right slot for its angular increment
/// (80046444/464E8); tests observe that boundary separately each tick.
#[derive(Component, Default, Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
#[serde(default)]
pub struct WheelRotationState {
    pub previous_delta_mps: [f32; 4],
    pub clutch_locked: [bool; 2],
}

#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct WheelReactionTorque {
    /// After tyre/brake reaction and optional yaw bias; input to axle solver.
    pub prepared: [f32; 4],
    /// After the final differential's internal torque transfer.
    pub final_torque: [f32; 4],
}

impl WheelRotationTuning {
    pub fn prepare(
        &self,
        input: WheelRotationInput,
        motion: WheelSurfaceMotion,
        brakes: WheelBrakeDemand,
        tyres: TyreForces,
        forward: ForwardDynamics,
    ) -> WheelReactionTorque {
        let mut torque = std::array::from_fn(|i| {
            // 80045718: wide signed product, truncate before negating.
            let reaction = -((tyres.wheel_response_force[i] as f64 + input.auxiliary_tread_force[i] as f64)
                * self.radius_m[i / 2] as f64)
                .trunc() as f32;
            if motion.tread_speed_mps[i] > 0.0 {
                reaction - brakes.total[i]
            } else if motion.tread_speed_mps[i] < 0.0 {
                reaction + brakes.total[i]
            } else {
                reaction
            }
        });
        for axle in 0..2 {
            let Differential::YawBiased { response } = self.differentials[axle] else { continue };
            if forward.forward_mps < self.yaw_bias_min_forward_mps {
                continue;
            }
            let bias = (input.yaw_correction as f64 * response as f64).floor() as f32;
            if bias == 0.0 {
                continue;
            }
            let left = axle * 2;
            let right = left + 1;
            let mut target = sample((motion.tread_speed_mps[left] as f64 + motion.tread_speed_mps[right] as f64) / 2.0);
            let eighth = sample(target as f64 / 8.0);
            if input.steering_turns > 0.0 {
                target += eighth;
            } else if input.steering_turns < 0.0 {
                target -= eighth;
            }
            let smaller = (bias as f64 * 7.0 / 16.0).trunc() as f32;
            let larger = (bias as f64 * 9.0 / 16.0).trunc() as f32;
            if motion.tread_speed_mps[right] < target {
                torque[right] += smaller;
                torque[left] -= larger;
            } else if motion.tread_speed_mps[right] > target {
                torque[right] -= larger;
                torque[left] += smaller;
            }
        }
        WheelReactionTorque { prepared: torque, final_torque: torque }
    }

    pub fn integrate(
        &self,
        drive: AxleDrive,
        brakes: WheelBrakeDemand,
        seconds: f32,
        motion: &mut WheelSurfaceMotion,
        state: &mut WheelRotationState,
        reaction: &mut WheelReactionTorque,
    ) {
        reaction.final_torque = reaction.prepared;
        for axle in 0..2 {
            let left = axle * 2;
            let right = left + 1;
            let difference = motion.tread_speed_mps[left] - motion.tread_speed_mps[right];
            let kind = self.differentials[axle];
            let mut locked = matches!(kind, Differential::Locked);
            let transfer = match kind {
                Differential::Clutch { preload_torque, drive_response, coast_response } => {
                    let response = if drive.torque[axle] >= 0.0 { drive_response } else { coast_response };
                    let capacity = (drive.torque[axle].abs() as f64 * response as f64).floor() as f32;
                    let capacity = capacity.max(preload_torque);
                    if state.clutch_locked[axle] {
                        if (reaction.prepared[left] - reaction.prepared[right]).abs() > capacity * 2.0 {
                            state.clutch_locked[axle] = false;
                        } else {
                            locked = true;
                        }
                    }
                    if locked { 0.0 } else { capacity }
                }
                Differential::Viscous { slip_squared_limit_mps2, torque_per_squared_mps } => {
                    let squared = (difference as f64 * difference as f64 * 4096.0).floor() / 4096.0;
                    (squared.min(slip_squared_limit_mps2 as f64) * torque_per_squared_mps as f64).floor() as f32
                }
                _ => 0.0,
            };
            if difference > 0.0 {
                reaction.final_torque[left] -= transfer;
                reaction.final_torque[right] += transfer;
            } else if difference < 0.0 {
                reaction.final_torque[left] += transfer;
                reaction.final_torque[right] -= transfer;
            }
            if locked {
                let tread = sample(self.radius_m[axle] as f64 * drive.radians_per_second[axle] as f64);
                motion.tread_speed_mps[left] = tread;
                motion.tread_speed_mps[right] = tread;
                // 800469D8 clears ONLY the left wheel's previous increment.
                state.previous_delta_mps[left] = 0.0;
            } else if matches!(kind, Differential::Independent) {
                for i in [left, right] {
                    let acceleration =
                        (reaction.final_torque[i] as f64 * self.rotation_response[axle] as f64 * 2.0).trunc() / 4096.0;
                    let mut delta = sample(acceleration * seconds as f64);
                    let old = motion.tread_speed_mps[i];
                    let mut speed = old + sample((state.previous_delta_mps[i] as f64 + delta as f64) / 2.0);
                    if brakes.total[i] != 0.0 && (old == 0.0 || old > 0.0 && speed < 0.0 || old < 0.0 && speed > 0.0) {
                        speed = 0.0;
                        delta = 0.0;
                    }
                    state.previous_delta_mps[i] = stored_delta(delta);
                    motion.tread_speed_mps[i] = speed;
                }
            } else {
                let acceleration = ((reaction.final_torque[left] as f64 - reaction.final_torque[right] as f64)
                    * self.rotation_response[axle] as f64)
                    .trunc()
                    / 4096.0;
                let mut delta = sample(acceleration * seconds as f64);
                let mut half_difference = sample(difference as f64 / 2.0)
                    + sample((state.previous_delta_mps[left] as f64 + delta as f64) / 2.0);
                // 80046A30 uses the signed low product word, even when its
                // sign differs from the mathematical product at large slip.
                if product_word_negative(difference, half_difference) {
                    half_difference = 0.0;
                    // 80046A4C is the branch delay slot: the stored delta
                    // is cleared for every differential kind, not only clutch.
                    delta = 0.0;
                    if matches!(kind, Differential::Clutch { .. }) {
                        state.clutch_locked[axle] = true;
                    }
                }
                state.previous_delta_mps[left] = stored_delta(delta);
                let base = sample(self.radius_m[axle] as f64 * drive.radians_per_second[axle] as f64);
                motion.tread_speed_mps[left] = base + half_difference;
                motion.tread_speed_mps[right] = base - half_difference;
            }
        }
    }
}

fn sample(value: f64) -> f32 {
    ((value * 4096.0).trunc() / 4096.0) as f32
}

/// Halfword stores narrow the increment after it has affected this tick.
fn stored_delta(delta: f32) -> f32 {
    (((delta as f64 * 4096.0 + 32768.0).rem_euclid(65536.0) - 32768.0) / 4096.0) as f32
}

fn product_word_negative(a: f32, b: f32) -> bool {
    // Preserve low bits even when the wide product exceeds f64's exact
    // integer range. Source operands are signed words on the /4096 grid.
    let a = (a as f64 * 4096.0).round() as i64;
    let b = (b as f64 * 4096.0).round() as i64;
    (a.wrapping_mul(b) as i32) < 0
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WheelRotationSystems {
    Prepare,
    AxleDrive,
    Integrate,
}

/// Use the tyre feature's ground-reference response in reaction torque.
/// Omit this marker to supply `auxiliary_tread_force` explicitly.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct TyreWheelFeedback;

pub struct WheelRotationRulesPlugin;
impl Plugin for WheelRotationRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<WheelRotationTuning>() {
            app.add_plugins(TuningPlugin::<WheelRotationTuning>::new(WHEEL_ROTATION_TUNING));
        }
        app.configure_sets(
            FixedUpdate,
            (VehicleSystems::Forces, VehicleSystems::BuildStep)
                .chain()
                .in_set(engine::prelude::EngineSet::Control)
                .in_set(GameSystems::Game),
        )
        .configure_sets(
            FixedUpdate,
            (WheelRotationSystems::Prepare, WheelRotationSystems::AxleDrive, WheelRotationSystems::Integrate)
                .chain()
                .in_set(VehicleSystems::Forces)
                .after(ForwardDynamicsSystems::Integrate)
                .after(BrakingSystems::Allocate),
        )
        .add_systems(FixedUpdate, prepare.in_set(WheelRotationSystems::Prepare))
        .add_systems(FixedUpdate, integrate.in_set(WheelRotationSystems::Integrate));
    }
}
type ReactionCar = (
    &'static WheelRotationInput,
    &'static WheelSurfaceMotion,
    &'static WheelBrakeDemand,
    &'static TyreForces,
    &'static ForwardDynamics,
    &'static mut WheelReactionTorque,
    Option<&'static TyreWheelFeedback>,
);
fn prepare(tuning: Res<WheelRotationTuning>, mut cars: Query<ReactionCar>) {
    for (&input, &motion, &brakes, &tyres, &forward, mut torque, feedback) in &mut cars {
        let mut input = input;
        if feedback.is_some() {
            input.auxiliary_tread_force = tyres.ground_response_force;
        }
        *torque = tuning.prepare(input, motion, brakes, tyres, forward);
    }
}
fn integrate(
    tuning: Res<WheelRotationTuning>,
    mut cars: Query<(
        &AxleDrive,
        &WheelBrakeDemand,
        &VehicleStepDuration,
        &mut WheelSurfaceMotion,
        &mut WheelRotationState,
        &mut WheelReactionTorque,
    )>,
) {
    for (&drive, &brakes, &duration, mut motion, mut state, mut torque) in &mut cars {
        tuning.integrate(drive, brakes, duration.0, &mut motion, &mut state, &mut torque);
    }
}
