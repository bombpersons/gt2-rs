//! Speed-dependent digital steering targets and their damped spring response.
//! Retained contact motion and the shared wheel-slip feature supply inputs.
//! `docs/driving-evidence.md` records the control and traction boundaries.
use super::{PHYSICS_STEP_SECONDS, PlayerPedals};
use bevy::prelude::*;
use engine::controller::{Button, ControllerState};
use serde::{Deserialize, Serialize};

pub const STEERING_TUNING: &str = "vehicle/steering.ron";

#[derive(Debug, Clone, Copy, Reflect, Deserialize, Serialize)]
pub struct SteeringKnot {
    pub speed_kph: f32,
    pub angle_turns: f32,
}

/// Digital spring and per-car steering limits. Knots are in increasing speed
/// order; parameters for the recorded Vitz are in `vehicle/steering.ron`.
#[derive(Resource, Debug, Clone, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct SteeringTuning {
    pub stiffness: f32,
    pub damping: f32,
    pub centering_stiffness: f32,
    pub rate_limit_turns_per_second: f32,
    pub extension_threshold_turns: f32,
    pub extension_min_speed_mps: f32,
    pub extended_limit_turns: f32,
    /// Inner/outer wheel multipliers, car+0x64/+0x66 (4096 = 1).
    pub inner_wheel_ratio: f32,
    pub outer_wheel_ratio: f32,
    /// Front/rear axle alignment, car+0x36C/+0x36E, in turns.
    pub toe_turns: [f32; 2],
    pub speed_curve: Vec<SteeringKnot>,
}

/// Input at the digital physics boundary. Left is positive, right negative;
/// only the sign matters, as in `8002FCC4..8002FCEC`.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct DigitalSteeringInput {
    pub direction: f32,
}

/// Retained contact state sampled before control processing. These are car+0x6A4 and front wheels +0x4B0/+0x518. Slip is
/// recomputed by `80039DE8` -> atan2(-wheel_vz, wheel_vx) before controls.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct SteeringKinematics {
    pub forward_velocity_mps: f32,
    pub front_slip_turns: [f32; 2],
}

/// Commanded wheel angle and response accumulator. Rate remains unclamped
/// in state; only the rate applied to angle is limited (`8002FD4C..8002FD98`).
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct SteeringResponse {
    pub angle_turns: f32,
    pub rate_turns_per_second: f32,
    pub target_turns: f32,
}

/// Wheel steering commands in original wheel order: front left/right, rear
/// left/right. The ordinary controller writes these before force evaluation
/// (`8003E340..8003E408`). Assistance corrections are a later, unported phase.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct WheelSteering {
    pub angle_turns: [f32; 4],
}

impl WheelSteering {
    /// Apply the inner/outer ratio on the magnitude, then restore the sign.
    /// This preserves symmetric truncation for negative turns; multiplying a
    /// signed angle and flooring would introduce a one-unit right-turn bias.
    pub fn from_response(response: SteeringResponse, tuning: &SteeringTuning) -> Self {
        let angle = response.angle_turns;
        let inner = angle_grid(angle.abs() as f64 * tuning.inner_wheel_ratio as f64);
        let outer = angle_grid(angle.abs() as f64 * tuning.outer_wheel_ratio as f64);
        let [left, right] = if angle >= 0.0 { [inner, outer] } else { [-outer, -inner] };
        let [front_toe, rear_toe] = tuning.toe_turns;
        Self { angle_turns: [left - front_toe, right + front_toe, -rear_toe, rear_toe] }
    }
}

impl SteeringTuning {
    /// Negative/positive digital targets. `8002FB54..8002FCAC` uses a speed
    /// curve unless front-wheel slip has already passed the extension
    /// threshold while moving forwards above 0x855B/4096 m/s. In that case
    /// it preserves a larger target in the current turn direction.
    pub fn limits(&self, state: SteeringKinematics) -> [f32; 2] {
        self.limits_with_extension(state).0
    }

    /// Intermediate signed limit at physics+700 (8002FC70/8002FCAC),
    /// before 8002FF30..8002FF78 replaces it with the traction response fraction.
    pub fn traction_extension_turns(&self, state: SteeringKinematics) -> f32 {
        self.limits_with_extension(state).1
    }

    /// Applied steering as a fraction of the signed extended limit, or zero
    /// when extension is inactive. 8002FF4C truncates the ratio, 8002FF54
    /// narrows it to a signed halfword, then 8002FF60..8002FF78 clamps to [0,1].
    pub fn traction_response(&self, angle_turns: f32, state: SteeringKinematics) -> f32 {
        let extension = self.traction_extension_turns(state);
        if extension == 0.0 {
            return 0.0;
        }
        let ratio = (angle_turns as f64 / extension as f64 * 4096.0).trunc() as i64 as i16;
        (ratio as f32 / 4096.0).clamp(0.0, 1.0)
    }

    fn limits_with_extension(&self, state: SteeringKinematics) -> ([f32; 2], f32) {
        let limit = self.speed_limit(state.forward_velocity_mps);
        let mean = angle_grid((state.front_slip_turns[0] as f64 + state.front_slip_turns[1] as f64) / 2.0);
        let threshold = limit.min(self.extension_threshold_turns);
        if state.forward_velocity_mps > self.extension_min_speed_mps && (mean < -threshold || mean > threshold) {
            if mean > 0.0 {
                let extended = mean.min(self.extended_limit_turns);
                ([-limit, extended], extended)
            } else {
                let extended = mean.max(-self.extended_limit_turns);
                ([extended, limit], extended)
            }
        } else {
            ([-limit, limit], 0.0)
        }
    }

    fn speed_limit(&self, speed_mps: f32) -> f32 {
        // 8002FB7C..8002FBB0 converts |velocity| to the curve's 1/128 km/h
        // grid with *461/4096. Its approximate m/s -> km/h ratio is 461/128.
        let speed = (speed_mps.abs() as f64 * 461.0).trunc().min(32767.0) / 128.0;
        let Some(first) = self.speed_curve.first() else {
            return 0.0;
        };
        if speed <= first.speed_kph as f64 {
            return first.angle_turns;
        }
        for pair in self.speed_curve.windows(2) {
            let [lower, upper] = [&pair[0], &pair[1]];
            if speed < upper.speed_kph as f64 {
                let fraction = (speed - lower.speed_kph as f64) / (upper.speed_kph - lower.speed_kph) as f64;
                // Interpolation in 8003D848 truncates the signed increment,
                // then adds the lower knot (not a truncation of the sum).
                return lower.angle_turns + angle_grid((upper.angle_turns - lower.angle_turns) as f64 * fraction);
            }
        }
        self.speed_curve.last().unwrap().angle_turns
    }
}

impl SteeringResponse {
    /// Follow a speed-dependent digital target with a damped spring, clamp
    /// the applied rate, and snap at the target. `8002FCF0..8002FE20`.
    pub fn advance(&mut self, input: DigitalSteeringInput, tuning: &SteeringTuning, state: SteeringKinematics) {
        let limits = tuning.limits(state);
        let target = if input.direction > 0.0 {
            limits[1]
        } else if input.direction < 0.0 {
            limits[0]
        } else {
            0.0
        };
        self.target_turns = target;
        let mut acceleration = (target - self.angle_turns) as f64 * tuning.stiffness as f64
            - self.rate_turns_per_second as f64 * tuning.damping as f64;
        if target == 0.0 {
            acceleration -= self.angle_turns as f64 * tuning.centering_stiffness as f64;
        }
        self.rate_turns_per_second += angle_grid(acceleration * PHYSICS_STEP_SECONDS as f64);
        let applied_rate =
            self.rate_turns_per_second.clamp(-tuning.rate_limit_turns_per_second, tuning.rate_limit_turns_per_second);
        let previous = self.angle_turns;
        self.angle_turns += angle_grid(applied_rate as f64 * PHYSICS_STEP_SECONDS as f64);
        if (target > 0.0 && self.angle_turns > target)
            || (target < 0.0 && self.angle_turns < target)
            || (previous < target && self.angle_turns >= target)
            || (previous > target && self.angle_turns <= target)
        {
            self.angle_turns = target;
            self.rate_turns_per_second = 0.0;
        }
    }
}

/// Keep the original 1/4096-turn grid, with signed truncation towards zero.
/// `8007596C`, `8007598C` and `8003D848` truncate intermediate steering
/// updates. Here that rounding affects endpoint snap ticks and target changes.
fn angle_grid(value: f64) -> f32 {
    ((value * 4096.0).trunc() / 4096.0) as f32
}

pub(super) fn read_player_steering(
    pad: Res<ControllerState>,
    mut players: Query<&mut DigitalSteeringInput, With<PlayerPedals>>,
) {
    for mut input in &mut players {
        input.direction = f32::from(u8::from(pad.held(Button::Left))) - f32::from(u8::from(pad.held(Button::Right)));
    }
}

pub(super) fn advance_steering(
    tuning: Res<SteeringTuning>,
    mut steering: Query<(&DigitalSteeringInput, &SteeringKinematics, &mut SteeringResponse)>,
) {
    for (&input, &state, mut response) in &mut steering {
        response.advance(input, &tuning, state);
    }
}

pub(super) fn steer_wheels(tuning: Res<SteeringTuning>, mut vehicles: Query<(&SteeringResponse, &mut WheelSteering)>) {
    for (&response, mut wheels) in &mut vehicles {
        *wheels = WheelSteering::from_response(response, &tuning);
    }
}
