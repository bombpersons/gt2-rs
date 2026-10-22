//! Refresh local contact motion after body integration/collision.
//!
//! 800304DC and 800306C0 project the corrected world velocity, add yaw at
//! wheel lever arms, optionally synchronize free wheel/axle speeds and advance
//! wheel spin. These results feed the NEXT tick's slip/control/force rules.
use super::{
    SteeringKinematics, VehicleStepDuration, VehicleSystems, VehicleVelocity,
    body_dynamics::BodyDynamics,
    longitudinal_slip::{WheelSurfaceMotion, steering_basis},
    slip::WheelLocalVelocities,
    tyre_forces::{TyreContactState, scaled_force},
    wheel_rotation::AxleDrive,
};
use bevy::{ecs::query::QueryData, prelude::*};
use engine::{prelude::TuningPlugin, shell::GameSystems};
use serde::{Deserialize, Serialize};

pub const CONTACT_MOTION_TUNING: &str = "vehicle/contact_motion.ron";

#[derive(Resource, Debug, Clone, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct ContactMotionTuning {
    /// Physics+18/0C/1A/0E: wheel (side, forward) lever arms in metres.
    pub wheel_levers_m: [[f32; 2]; 4],
    /// Physics+3E8/+3EA /4096. Converts mean tread m/s to axle rad/s.
    pub inverse_radius_per_m: [f32; 2],
    /// 8003059C immediate /4096; yaw uses a separate 1/524288 turn/s grid.
    pub radians_per_turn: f32,
    /// 80030890 immediate /4096, used after the radius multiplication.
    pub turns_per_radian: f32,
    /// 80030570/574 multiplier /4096. Readout units are left unnamed here.
    pub speed_readout_units_per_mps: f32,
    /// Strict speed gate 80030660 and the two-step gain at 8003066C..7C.
    pub full_response_speed_mps: f32,
    pub response_gain_per_mps: f32,
    pub base_speed_response: f32,
}

#[derive(Component, Debug, Clone, Reflect, Deserialize, Serialize)]
#[reflect(Component)]
pub struct ContactMotionSetup(pub ContactMotionTuning);

#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct ContactMotionInput {
    /// Physics+68C/68E/690 and +694/696/698, converted to Bevy (x,z,-y).
    /// The orientation/contact producer supplies these corrected axes.
    pub forward_axis: Vec3,
    pub lateral_axis: Vec3,
    /// Wheel+63 ==0, not just the load/contact bit. Other flags block reset.
    pub synchronize_wheels: [bool; 4],
    /// Physics+37A/+37B ==0: independent differential always refreshes shaft.
    pub independent_axles: [bool; 2],
    /// Wheel+0C steering for the synchronization projection, /4096 turns.
    /// This is sampled after orientation/contact processing.
    pub wheel_heading_turns: [f32; 4],
}

#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct ContactMotion {
    pub body_forward_mps: f32,
    pub body_lateral_mps: f32,
    /// Separate halfwords +6AE/+6F8, written by 80030594/598. These are
    /// speed-derived quantities, not engine RPM despite earlier field labels.
    pub speed_readout: u16,
    pub maximum_speed_readout: u16,
    /// Wheel+20 /4096, wrapped to [0,1) turns after each refresh.
    pub spin_turns: [f32; 4],
    pub synchronized_axles: [bool; 2],
}

impl ContactMotionTuning {
    pub fn refresh(
        &self,
        input: ContactMotionInput,
        velocity: VehicleVelocity,
        seconds: f32,
        state: &mut ContactMotion,
        local: &mut WheelLocalVelocities,
        wheels: &mut WheelSurfaceMotion,
    ) -> [f32; 2] {
        // 8003052C floors each WIDE product; 8003053C/554 floors the LOW
        // word of each lateral product. Keep the original boundaries.
        state.body_forward_mps = velocity
            .linear_mps
            .to_array()
            .into_iter()
            .zip(input.forward_axis.to_array())
            .map(|(speed, response)| floor_grid(speed as f64 * response as f64, 4096.0))
            .sum();
        state.body_lateral_mps = velocity
            .linear_mps
            .to_array()
            .into_iter()
            .zip(input.lateral_axis.to_array())
            .map(|(speed, response)| floor_low_product(response, speed * 4096.0, 12) / 4096.0)
            .sum();
        state.speed_readout =
            ((state.body_forward_mps.abs() as f64 * self.speed_readout_units_per_mps as f64).floor() as u32) as u16;
        state.maximum_speed_readout = state.maximum_speed_readout.max(state.speed_readout);
        let omega = trunc_grid(velocity.yaw_turns_per_second as f64 * self.radians_per_turn as f64, 4096.0);
        state.synchronized_axles = [false; 2];
        for (i, &[side, forward]) in self.wheel_levers_m.iter().enumerate() {
            local.forward_mps[i] = state.body_forward_mps + scaled_force(side, omega * 4096.0) / 4096.0;
            local.lateral_mps[i] = state.body_lateral_mps - scaled_force(forward, omega * 4096.0) / 4096.0;
            let speed = local.forward_mps[i].abs();
            wheels.speed_response[i] = if speed > self.full_response_speed_mps {
                1.0
            } else {
                let first = trunc_grid(speed as f64 * self.response_gain_per_mps as f64, 4096.0);
                self.base_speed_response + floor_grid(first as f64 / 2.0, 4096.0)
            };
            if input.synchronize_wheels[i] {
                let [cosine, sine] = steering_basis(input.wheel_heading_turns[i]);
                wheels.tread_speed_mps[i] = trunc_grid(cosine as f64 * local.forward_mps[i] as f64, 4096.0)
                    - trunc_grid(sine as f64 * local.lateral_mps[i] as f64, 4096.0);
                state.synchronized_axles[i / 2] = true;
            }
        }
        let shafts = std::array::from_fn(|axle| {
            state.synchronized_axles[axle] |= input.independent_axles[axle];
            // 80030820..2C truncates the signed pair mean before radius.
            let mean = trunc_grid(
                (wheels.tread_speed_mps[axle * 2] as f64 + wheels.tread_speed_mps[axle * 2 + 1] as f64) / 2.0,
                4096.0,
            );
            trunc_grid(mean as f64 * self.inverse_radius_per_m[axle] as f64, 4096.0)
        });
        for i in 0..4 {
            let distance = floor_grid(wheels.tread_speed_mps[i] as f64 * seconds as f64, 4096.0);
            let radians = floor_low_product(self.inverse_radius_per_m[i / 2], distance * 4096.0, 12);
            let delta = scaled_force(self.turns_per_radian, radians) / 4096.0;
            state.spin_turns[i] = (state.spin_turns[i] + delta).rem_euclid(1.0);
        }
        shafts
    }
}

pub(super) fn floor_low_product(response: f32, value: f32, shift: u32) -> f32 {
    let response = (response as f64 * 4096.0).round() as i64;
    let value = (value as f64).round() as i64;
    (response.wrapping_mul(value) as i32 >> shift) as f32
}
fn floor_grid(value: f64, scale: f64) -> f32 {
    ((value * scale).floor() / scale) as f32
}
fn trunc_grid(value: f64, scale: f64) -> f32 {
    ((value * scale).trunc() / scale) as f32
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContactMotionSystems {
    Refresh,
}
pub struct ContactMotionRulesPlugin;
impl Plugin for ContactMotionRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<ContactMotionTuning>() {
            app.add_plugins(TuningPlugin::<ContactMotionTuning>::new(CONTACT_MOTION_TUNING));
        }
        app.configure_sets(
            FixedUpdate,
            (
                VehicleSystems::Forces,
                VehicleSystems::BuildStep,
                VehicleSystems::Integrate,
                VehicleSystems::RefreshContact,
            )
                .chain()
                .in_set(engine::prelude::EngineSet::Control)
                .in_set(GameSystems::Game),
        )
        .add_systems(FixedUpdate, refresh.in_set(ContactMotionSystems::Refresh).in_set(VehicleSystems::RefreshContact));
    }
}
#[derive(QueryData)]
#[query_data(mutable)]
struct ContactCar {
    input: &'static ContactMotionInput,
    velocity: &'static VehicleVelocity,
    duration: &'static VehicleStepDuration,
    state: &'static mut ContactMotion,
    local: &'static mut WheelLocalVelocities,
    wheels: &'static mut WheelSurfaceMotion,
    setup: Option<&'static ContactMotionSetup>,
    axles: Option<&'static mut AxleDrive>,
    contact: Option<&'static mut TyreContactState>,
    steering: Option<&'static mut SteeringKinematics>,
    body: Option<&'static mut BodyDynamics>,
}
fn refresh(tuning: Res<ContactMotionTuning>, mut cars: Query<ContactCar>) {
    for mut car in &mut cars {
        let setup = car.setup.map(|v| &v.0).unwrap_or(&tuning);
        let shafts =
            setup.refresh(*car.input, *car.velocity, car.duration.0, &mut car.state, &mut car.local, &mut car.wheels);
        if let Some(mut axles) = car.axles {
            for (i, shaft) in shafts.into_iter().enumerate() {
                if car.state.synchronized_axles[i] {
                    axles.radians_per_second[i] = shaft;
                }
            }
        }
        if let Some(mut contact) = car.contact {
            contact.body_forward_mps = car.state.body_forward_mps;
        }
        if let Some(mut steering) = car.steering {
            steering.forward_velocity_mps = car.state.body_forward_mps;
        }
        if let Some(mut body) = car.body {
            body.local_lateral_mps = car.state.body_lateral_mps;
        }
    }
}
