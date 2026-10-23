//! Suspension-driven body tilt, vertical impulses and compression-stop correction.
//!
//! Original 80043F18..44898. Movement and road sampling already happened;
//! these impulses affect the next step. Correction retains the earlier road
//! frame and travel projection, then the late contact stage rebuilds frames.
use super::{
    VehiclePose, VehicleStepDuration, VehicleSystems, VehicleVelocity,
    body_dynamics::{BodyDynamics, BodyDynamicsInput},
    direction::direction_turns,
    road_contact::{BodyOrientation, RoadContactState, vehicle_basis},
    suspension::{Suspension, SuspensionInput},
    transmission::TransmissionState,
};
use bevy::{ecs::query::QueryData, prelude::*};
use engine::{prelude::TuningPlugin, shell::GameSystems};
use serde::{Deserialize, Serialize};

pub const VERTICAL_BODY_TUNING: &str = "vehicle/vertical_body.ron";

#[derive(Resource, Clone, Debug, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct VerticalBodyTuning {
    /// Physics+6C/+70 and +74/+78, /4096 /65536: turns/s² per force.
    pub pitch_response: [f32; 2],
    pub roll_response: [f32; 2],
    /// Physics+2C /4096. This differs from the horizontal reciprocal mass.
    pub inverse_mass: f32,
    /// 8004412C..48: asymmetric signed rate cap, turns/s.
    pub minimum_tilt_rate: f32,
    pub maximum_tilt_rate: f32,
    /// 80044178/44294: ordinary integration cap; collision may exceed it.
    pub maximum_tilt_turns: f32,
    /// Shift/add constant at 800443A0..C8, /4096 metres/s².
    pub gravity_mps2: f32,
    /// Suspension axle+2 and axle+8 minus wheel radius, metres.
    pub minimum_travel_m: [f32; 2],
    pub travel_offset_m: [f32; 2],
    /// Shared car geometry (+18/0C/1A/0E, +10/+12/+14), metres.
    pub wheel_levers_m: [[f32; 2]; 4],
    pub wheelbase_m: f32,
    pub track_m: f32,
    pub rear_height_fraction: f32,
    pub angle_threshold_ratios: Vec<f32>,
    /// 80035870 height (1228/4096 m), 800357D4 reset duration (5 s),
    /// and 80043FE4..FFC heading slew (341/4096 turns/s).
    pub reset_height_m: f32,
    pub reset_seconds: f32,
    pub reset_ramp_seconds: f32,
    pub reset_heading_turns_per_second: f32,
}

#[derive(Component, Clone, Debug, Reflect, Deserialize, Serialize)]
#[reflect(Component)]
pub struct VerticalBodySetup(pub VerticalBodyTuning);

#[derive(Component, Clone, Copy, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
#[serde(default)]
pub struct VerticalBodyInput {
    /// Physics+78E. TransmissionState supplies this when present.
    pub reset_ticks: u16,
    /// 801C8570; reset phases use the original tick count, not rounded dt.
    pub ticks_per_second: u32,
    /// Event record +28 selected by physics+6B2 at 80043FA0..BC.
    /// Race event selection remains an explicit input.
    pub reset_heading_turns: f32,
}
impl Default for VerticalBodyInput {
    fn default() -> Self {
        Self { reset_ticks: 0, ticks_per_second: 30, reset_heading_turns: 0.0 }
    }
}

#[derive(Component, Default, Clone, Copy, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
#[serde(default)]
pub struct VerticalBody {
    /// Physics+6B1: zero total tyre load bypasses next tick's tyre acceleration.
    pub airborne: bool,
    pub reset_applied: bool,
    /// Diagnostics of this correction; suspension travel/rate history stays put.
    pub penetration_m: [f32; 4],
    pub lift_m: f32,
}

/// The existing shared vehicle components this feature updates. No other
/// rules plugin is required; a mod can supply their inputs directly.
pub struct BodyMotion<'a> {
    pub pose: &'a mut VehiclePose,
    pub orientation: &'a mut BodyOrientation,
    pub dynamics: &'a mut BodyDynamics,
    pub velocity: &'a mut VehicleVelocity,
    pub road: &'a mut RoadContactState,
}

impl VerticalBodyTuning {
    pub fn advance(
        &self,
        input: VerticalBodyInput,
        contact: SuspensionInput,
        seconds: f32,
        suspension: &mut Suspension,
        state: &mut VerticalBody,
        motion: BodyMotion<'_>,
    ) {
        if contact.disabled {
            return;
        }
        let BodyMotion { pose, orientation, dynamics, velocity, road } = motion;
        state.reset_applied = false;
        state.penetration_m = [0.0; 4];
        state.lift_m = 0.0;
        if input.reset_ticks >= 2 {
            let ticks = input.reset_ticks as f64;
            let rate = input.ticks_per_second as f64;
            let ramp = (rate * self.reset_ramp_seconds as f64).trunc();
            let duration = rate * self.reset_seconds as f64;
            let rising = ticks >= duration - ramp;
            let offset = if rising {
                sample((duration - ticks) * self.reset_height_m as f64 / ramp, 4096.0)
            } else if ticks < ramp {
                0.0
            } else {
                self.reset_height_m as f64
            };
            let target = road.road_height_m as f64 + offset;
            if pose.position_m.y as f64 <= target {
                pose.position_m.y = target as f32;
                *orientation = BodyOrientation::default();
                dynamics.tilt_turns_per_second = Vec2::ZERO;
                velocity.linear_mps.y = 0.0;
                state.reset_applied = true;
                if !rising && ticks >= ramp {
                    // 800450E0 chooses [-half turn, half turn), including
                    // the negative direction at an exact half-turn tie.
                    let difference = (input.reset_heading_turns - pose.heading_turns + 0.5).rem_euclid(1.0) - 0.5;
                    let step = sample(seconds as f64 * self.reset_heading_turns_per_second as f64, 4096.0) as f32;
                    pose.heading_turns =
                        narrow(pose.heading_turns as f64 + difference.clamp(-step, step) as f64, 4096.0);
                }
            }
        }
        if !state.reset_applied {
            let spring = suspension.body_force;
            let damper = suspension.damper_force;
            let pitch = |f: [f64; 4]| {
                low_product(f[0] + f[1], self.pitch_response[0] as f64 * 65536.0, 12)
                    - low_product(f[2] + f[3], self.pitch_response[1] as f64 * 65536.0, 12)
            };
            let roll = |f: [f64; 4], response: [f32; 2]| {
                low_product(f[0] - f[1], response[0] as f64 * 65536.0, 12)
                    + low_product(f[2] - f[3], response[1] as f64 * 65536.0, 12)
            };
            self.integrate_tilt(
                &mut orientation.pitch_turns,
                &mut dynamics.tilt_turns_per_second.x,
                pitch(spring),
                pitch(damper),
                seconds,
            );
            // 800441E8/200 deliberately reuse the *pitch* responses for
            // roll damping, while spring roll uses +74/+78.
            self.integrate_tilt(
                &mut orientation.roll_turns,
                &mut dynamics.tilt_turns_per_second.y,
                roll(spring, self.roll_response),
                roll(damper, self.pitch_response),
                seconds,
            );
            state.airborne = suspension.tyre_load.iter().sum::<f64>() == 0.0;
        }
        road.body_basis = vehicle_basis(orientation.pitch_turns, orientation.roll_turns, pose.heading_turns);
        if !state.reset_applied {
            let impulse = |forces: [f64; 4]| {
                let acceleration = (forces.iter().sum::<f64>() * self.inverse_mass as f64).trunc();
                low_product(acceleration, seconds as f64, 16)
            };
            // Project in the original axes before converting to Bevy. The
            // operand-sign bias must happen before the source-y reflection.
            let up = road.body_basis.up;
            let project = |force: f64| {
                Vec3::new(
                    low_product(force, up.x as f64, 12) as f32,
                    low_product(force, up.y as f64, 12) as f32,
                    -low_product(force, -up.z as f64, 12) as f32,
                ) / 4096.0
            };
            velocity.linear_mps += project(impulse(suspension.body_force));
            velocity.linear_mps.y -= ((seconds as f64 * self.gravity_mps2 as f64 * 4096.0).floor() / 4096.0) as f32;
            let before_damping = velocity.linear_mps.y;
            velocity.linear_mps += project(impulse(suspension.damper_force));
            if (velocity.linear_mps.y < 0.0) != (before_damping < 0.0) {
                velocity.linear_mps.y = 0.0;
            }
        }
        if suspension.compressed_mask == 0 {
            return;
        }
        let projection = (contact.road_normal_up as f64 / contact.body_up as f64 * 4096.0).trunc() / 4096.0;
        let mut mask = 0;
        for (i, &[side, ahead]) in self.wheel_levers_m.iter().enumerate() {
            let attachment = low_product(ahead as f64 * 4096.0, road.body_basis.forward.y as f64, 12)
                + low_product(side as f64 * 4096.0, road.body_basis.lateral.y as f64, 12);
            road.attachment_m[i].y = narrow(attachment / 4096.0, 4096.0);
            let height = pose.position_m.y as f64 + road.attachment_m[i].y as f64
                - contact.road_height_m[i] as f64
                - contact.road_offset_m[i] as f64;
            let travel = low_product(height * 4096.0, projection, 12) / 4096.0 + self.travel_offset_m[i / 2] as f64;
            if travel < self.minimum_travel_m[i / 2] as f64 {
                mask |= 1 << i;
            }
            // 80044628 narrows *before* testing the sign of penetration.
            state.penetration_m[i] = narrow(self.minimum_travel_m[i / 2] as f64 - travel, 4096.0).max(0.0);
        }
        suspension.compressed_mask = (suspension.compressed_mask << 4) | mask;
        if mask != 0 {
            let p = state.penetration_m.map(f64::from);
            let blend = |a: usize, b: usize| {
                narrow(
                    p[a] + low_product((p[b] - p[a]) * 4096.0, self.rear_height_fraction as f64, 12) / 4096.0,
                    4096.0,
                )
            };
            let left = blend(0, 2);
            let right = blend(1, 3);
            let front = sample((p[0] + p[1]) / 2.0, 4096.0) as f32;
            let rear = sample((p[2] + p[3]) / 2.0, 4096.0) as f32;
            let mut lift = 0.0;
            for (a, b, length, angle) in [
                (left, right, self.track_m, &mut orientation.roll_turns),
                (front, rear, self.wheelbase_m, &mut orientation.pitch_turns),
            ] {
                if a != 0.0 || b != 0.0 {
                    if a != b {
                        *angle = narrow(
                            *angle as f64 + direction_turns(&self.angle_threshold_ratios, length, a - b) as f64,
                            4096.0,
                        );
                    }
                    lift += sample(a.min(b) as f64 / 2.0, 4096.0);
                }
            }
            state.lift_m = narrow(lift, 4096.0);
            pose.position_m.y += state.lift_m;
        } else {
            state.penetration_m = [0.0; 4];
        }
        // 800447EC uses a wide floor for forward; 80044800 uses the
        // signed low word for lateral. Both use the *prior* road frame.
        let v = [velocity.linear_mps.x, -velocity.linear_mps.z, velocity.linear_mps.y].map(|v| v as f64 * 4096.0);
        let f = [road.road_basis.forward.x, -road.road_basis.forward.z, road.road_basis.forward.y];
        let s = [road.road_basis.lateral.x, -road.road_basis.lateral.z, road.road_basis.lateral.y];
        let forward = (0..3).map(|i| (v[i] * f[i] as f64).floor()).sum::<f64>();
        let side = (0..3).map(|i| low_floor(v[i], s[i] as f64)).sum::<f64>();
        let floor = (low_product(forward, f[2] as f64, 12) + low_product(side, s[2] as f64, 12)).min(0.0) / 4096.0;
        velocity.linear_mps.y = velocity.linear_mps.y.max(floor as f32);
    }

    fn integrate_tilt(&self, angle: &mut f32, rate: &mut f32, spring: f64, damper: f64, seconds: f32) {
        let spring_rate = narrow(*rate as f64 + low_product(spring, seconds as f64, 16) / 65536.0, 65536.0);
        let damped = spring_rate as f64 + low_product(damper, seconds as f64, 16) / 65536.0;
        *rate = if spring_rate > 0.0 && damped < 0.0 || spring_rate < 0.0 && damped > 0.0 {
            0.0
        } else {
            (damped as f32).clamp(self.minimum_tilt_rate, self.maximum_tilt_rate)
        };
        *angle = narrow(*angle as f64 + sample(seconds as f64 * *rate as f64, 4096.0), 4096.0);
        if angle.abs() > self.maximum_tilt_turns {
            *angle = angle.clamp(-self.maximum_tilt_turns, self.maximum_tilt_turns);
            *rate = 0.0;
        }
    }
}

fn sample(value: f64, scale: f64) -> f64 {
    (value * scale).trunc() / scale
}
fn narrow(value: f64, scale: f64) -> f32 {
    (value * scale).round() as i64 as i16 as f32 / scale as f32
}
/// The source's signed-low-word multiply helpers 8007596C/98C. The
/// operand-sign bias survives overflow; a wide floating product does not.
fn low_product(value: f64, ratio: f64, bits: u32) -> f64 {
    let a = value.round() as i64;
    let b = (ratio * (1u64 << bits) as f64).round() as i64;
    let low = a.wrapping_mul(b) as i32;
    let bias = if (a < 0) != (b < 0) { (1i32 << bits) - 1 } else { 0 };
    (low.wrapping_add(bias) >> bits) as f64
}
fn low_floor(value: f64, ratio: f64) -> f64 {
    ((value.round() as i64).wrapping_mul((ratio * 4096.0).round() as i64) as i32 >> 12) as f64
}

pub struct VerticalBodyRulesPlugin;
impl Plugin for VerticalBodyRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<VerticalBodyTuning>() {
            app.add_plugins(TuningPlugin::<VerticalBodyTuning>::new(VERTICAL_BODY_TUNING));
        }
        app.register_type::<VerticalBody>()
            .register_type::<VerticalBodyInput>()
            .configure_sets(
                FixedUpdate,
                VehicleSystems::CorrectBody
                    .after(VehicleSystems::Suspension)
                    .before(VehicleSystems::RefreshContact)
                    .in_set(GameSystems::Game),
            )
            .add_systems(FixedUpdate, advance.in_set(VehicleSystems::CorrectBody));
    }
}
#[derive(QueryData)]
#[query_data(mutable)]
struct BodyCar {
    input: &'static VerticalBodyInput,
    contact: &'static SuspensionInput,
    duration: &'static VehicleStepDuration,
    suspension: &'static mut Suspension,
    state: &'static mut VerticalBody,
    pose: &'static mut VehiclePose,
    orientation: &'static mut BodyOrientation,
    dynamics: &'static mut BodyDynamics,
    velocity: &'static mut VehicleVelocity,
    road: &'static mut RoadContactState,
    setup: Option<&'static VerticalBodySetup>,
    transmission: Option<&'static TransmissionState>,
    forces: Option<&'static mut BodyDynamicsInput>,
}
fn advance(tuning: Res<VerticalBodyTuning>, mut cars: Query<BodyCar>) {
    for mut car in &mut cars {
        let mut input = *car.input;
        if let Some(transmission) = car.transmission {
            input.reset_ticks = transmission.first_gear_ticks;
        }
        car.setup.map(|v| &v.0).unwrap_or(&tuning).advance(
            input,
            *car.contact,
            car.duration.0,
            &mut car.suspension,
            &mut car.state,
            BodyMotion {
                pose: &mut car.pose,
                orientation: &mut car.orientation,
                dynamics: &mut car.dynamics,
                velocity: &mut car.velocity,
                road: &mut car.road,
            },
        );
        if !car.contact.disabled
            && let Some(mut forces) = car.forces
        {
            forces.bypass_tyre_acceleration = car.state.airborne;
        }
    }
}
