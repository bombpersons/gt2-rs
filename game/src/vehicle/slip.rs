//! Wheel-local direction and low-speed activation, evaluated before controls.
//!
//! Velocities are metres/second at each contact in vehicle-local axes;
//! direction is turns. `80039DE8` samples the prior wheel velocities before
//! steering. Contact and force calculations that produce these inputs remain
//! separate. See `docs/wheel-slip-evidence.md` for the exact rounding boundary.
use super::{SteeringKinematics, VehicleSystems};
use bevy::prelude::*;
use engine::prelude::TuningPlugin;
use engine::shell::GameSystems;
use serde::{Deserialize, Serialize};

pub const SLIP_TUNING: &str = "vehicle/slip.ron";

/// Contact velocities in vehicle-local forward/lateral axes, before steering
/// rotation, in front-left/right then rear-left/right order. Original
/// wheel+0x2C/+0x30 are signed 1/4096 m/s words.
#[derive(Component, Default, Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
#[serde(default)]
pub struct WheelLocalVelocities {
    pub forward_mps: [f32; 4],
    pub lateral_mps: [f32; 4],
}

/// Geometric wheel slip and the independent low-speed activation channel.
/// These correspond to wheel+0x50/+0x52, signed halfwords with 4096 per
/// turn and 4096 per full activation. This is not a tyre-force calculation.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct WheelSlip {
    pub angle_turns: [f32; 4],
    pub activation: [f32; 4],
}

/// Low-speed gating and exact quantized first-octant angle curve. The 512
/// thresholds in the supplied asset compress the original 4097-value table
/// without changing a value; each threshold starts another 1/4096 turn.
#[derive(Resource, Debug, Clone, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct SlipTuning {
    pub stationary_speed_mps: f32,
    pub full_speed_mps: f32,
    pub activation_per_mps: f32,
    pub angle_threshold_ratios: Vec<f32>,
}

impl SlipTuning {
    pub fn evaluate(&self, velocities: WheelLocalVelocities) -> WheelSlip {
        let mut slip = WheelSlip::default();
        for wheel in 0..4 {
            let forward = velocities.forward_mps[wheel];
            let lateral = velocities.lateral_mps[wheel];
            let activation = self.speed_activation(forward, lateral);
            slip.activation[wheel] = activation;
            // The source does not evaluate direction when the quantized
            // activation is zero (80039EE4..EF8), even above its dead zone.
            if activation != 0.0 {
                slip.angle_turns[wheel] = self.direction_turns(forward, -lateral);
            }
        }
        slip
    }

    fn speed_activation(&self, forward: f32, lateral: f32) -> f32 {
        // 80039E50..E70 bypasses the magnitude calculation when either
        // component exceeds the full-speed threshold in either direction.
        if forward.abs() > self.full_speed_mps || lateral.abs() > self.full_speed_mps {
            return 1.0;
        }
        let speed = quantized_magnitude(forward, lateral);
        if speed >= self.full_speed_mps {
            1.0
        } else {
            let above_dead_zone = (speed - self.stationary_speed_mps).max(0.0);
            ((above_dead_zone as f64 * self.activation_per_mps as f64 * 4096.0).floor() / 4096.0) as f32
        }
    }

    fn direction_turns(&self, forward: f32, sideways: f32) -> f32 {
        super::direction::direction_turns(&self.angle_threshold_ratios, forward, sideways)
    }
}

/// The original floors each squared component onto a 1/4096 (m/s)^2 grid,
/// then floors the square root onto a 1/4096 m/s grid. `80039E74..E9C`
/// invokes the integer square-root helper 80081288 with six fractional bits.
/// Preserving these grids matters: speed 1141/4096 still has zero activation.
fn quantized_magnitude(forward: f32, lateral: f32) -> f32 {
    let square = |value: f32| ((value as f64).powi(2) * 4096.0).floor() / 4096.0;
    (((square(forward) + square(lateral)).sqrt() * 4096.0).floor() / 4096.0) as f32
}

/// Opt-in rules for cars carrying wheel-local velocities. Cars that supply
/// independent steering kinematics can omit that component, as replay tests do.
pub struct WheelSlipRulesPlugin;

impl Plugin for WheelSlipRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<SlipTuning>() {
            app.add_plugins(TuningPlugin::<SlipTuning>::new(SLIP_TUNING));
        }
        app.register_type::<WheelLocalVelocities>()
            .register_type::<WheelSlip>()
            .register_type::<SlipTuning>()
            .configure_sets(FixedUpdate, VehicleSystems::Prepare.before(VehicleSystems::ReadInput))
            .add_systems(FixedUpdate, update_wheel_slip.in_set(VehicleSystems::Prepare).in_set(GameSystems::Game));
    }
}

pub fn update_wheel_slip(
    tuning: Res<SlipTuning>,
    mut vehicles: Query<(&WheelLocalVelocities, &mut WheelSlip, Option<&mut SteeringKinematics>)>,
) {
    for (&velocities, mut slip, steering) in &mut vehicles {
        *slip = tuning.evaluate(velocities);
        if let Some(mut steering) = steering {
            steering.front_slip_turns = [slip.angle_turns[0], slip.angle_turns[1]];
        }
    }
}
