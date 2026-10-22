//! Lateral/yaw force accumulation and horizontal world-velocity feedback.
//!
//! Original 8003AEDC..8003B304. Contact orientation, resistance, external
//! forces and reset requests are explicit inputs; this is not collision.
use super::{
    VehicleStepDuration, VehicleSystems, VehicleVelocity, WheelSteering,
    drivetrain::DrivetrainInput,
    forward_dynamics::{ForwardDynamics, ForwardDynamicsSystems},
    longitudinal_slip::steering_basis,
    slip::WheelSlip,
    transmission::{TransmissionState, TransmissionSystems},
    tyre_forces::{TyreForces, scaled_force},
};
use bevy::{ecs::query::QueryData, prelude::*};
use engine::{prelude::TuningPlugin, shell::GameSystems};
use serde::{Deserialize, Serialize};

pub const BODY_DYNAMICS_TUNING: &str = "vehicle/body_dynamics.ron";

#[derive(Resource, Debug, Clone, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct BodyDynamicsTuning {
    /// Wheel lever arms (side, forward), metres: physics+18/0C/1A/0E.
    pub wheel_levers_m: [[f32; 2]; 4],
    /// Physics+68 /4096 /524288. Original torque units are not SI newtons.
    pub yaw_response_turns_per_torque_second2: f32,
    /// Physics+2A /4096; the body reciprocal, without wheel rotational mass.
    pub inverse_mass: f32,
    /// Physics+34 /4096, converting lateral acceleration to +64A feedback.
    pub lateral_feedback_response: f32,
    /// Clamp at 8003B048..6C, converted from 1/524288 turn/s.
    pub maximum_yaw_turns_per_second: f32,
    /// Inclusive symmetric speed gate at 8003B160..168, metres/s.
    pub brake_stop_speed_mps: f32,
}

#[derive(Component, Debug, Clone, Reflect, Deserialize, Serialize)]
#[reflect(Component)]
pub struct BodyDynamicsSetup(pub BodyDynamicsTuning);

#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct BodyDynamicsInput {
    /// Physics+6B1 !=0 skips tyre acceleration, but still applies external force.
    /// VerticalBody supplies the airborne gate when present.
    pub bypass_tyre_acceleration: bool,
    /// Physics+718 !=0 or +786 ==7. First-gear override is separate below.
    pub reset_motion: bool,
    /// Physics+78E !=0: reset horizontal motion while retaining vertical/tilt.
    /// A TransmissionState component supplies the current countdown if present.
    pub first_gear_override: bool,
    /// Wheel+63 bit 1, set from nonzero allocated brake demand at 8003A254.
    pub braked_wheels: [bool; 4],
    /// Wheel+52 /4096. WheelSlip supplies this when present.
    pub slip_activation: [f32; 4],
    /// Physics+668/+66A and +670/+672, projected into Bevy world (x,z).
    /// These need not be orthogonal on tilted contact surfaces.
    pub forward_axis: Vec2,
    pub lateral_axis: Vec2,
    /// Reserved opposing force at physics+730, cleared by 8003A2C8. All
    /// observed ordinary calls consume zero; nonzero authored values follow
    /// the subtraction established by the consuming binary instruction.
    pub lateral_resistance: f32,
    /// Physics+778/+77C, in Bevy world (x,z); applied after the tyre/reset gate.
    pub external_world_force: Vec2,
}

#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct BodyDynamics {
    /// Physics+6A8 /4096, supplied by local contact projection and cleared
    /// only by the brake-stop branch in this stage.
    pub local_lateral_mps: f32,
    /// Physics+728 /4096, metres/s².
    pub lateral_acceleration_mps2: f32,
    /// Physics+64A /4096; consumed by the drivetrain next tick. Its exact
    /// physical angle interpretation remains open.
    pub lateral_feedback_turns: f32,
    /// Physics+650/+652 /65536: pitch and roll turns/s. Vertical body
    /// integration updates them; a full motion reset clears them.
    pub tilt_turns_per_second: Vec2,
    pub lateral_force: f32,
    pub yaw_torque: f32,
    /// The low-speed, inactive-slip, nonzero-brake gate cleared local motion.
    pub brake_stopped: bool,
}

impl BodyDynamicsTuning {
    pub fn accumulate(
        &self,
        input: BodyDynamicsInput,
        tyres: TyreForces,
        steering: WheelSteering,
        seconds: f32,
        velocity: &mut VehicleVelocity,
        body: &mut BodyDynamics,
    ) {
        body.lateral_force = 0.0;
        body.yaw_torque = 0.0;
        body.brake_stopped = false;
        if input.bypass_tyre_acceleration {
            body.lateral_acceleration_mps2 = 0.0;
            body.lateral_feedback_turns = 0.0;
            return;
        }
        for (wheel, &[side, forward]) in self.wheel_levers_m.iter().enumerate() {
            let [cosine, sine] = steering_basis(steering.angle_turns[wheel]);
            let longitudinal = tyres.wheel_reference_force[wheel] + tyres.ground_reference_force[wheel];
            let lateral = tyres.lateral_force[wheel];
            // 8003AFA4..B010 truncates each wide product before addition.
            let sideways = -force_product(cosine, lateral) - force_product(sine, longitudinal);
            let ahead = force_product(cosine, longitudinal) - force_product(sine, lateral);
            body.lateral_force += sideways;
            body.yaw_torque += force_product(side, ahead) - force_product(forward, sideways);
        }
        let yaw_acceleration =
            grid(body.yaw_torque as f64 * self.yaw_response_turns_per_torque_second2 as f64, 524288.0);
        velocity.yaw_turns_per_second = (velocity.yaw_turns_per_second
            + grid(yaw_acceleration as f64 * seconds as f64, 524288.0))
        .clamp(-self.maximum_yaw_turns_per_second, self.maximum_yaw_turns_per_second);
        body.lateral_acceleration_mps2 =
            force_product(self.inverse_mass, body.lateral_force - input.lateral_resistance) / 4096.0;
        // 8003B0A0 narrows to a signed halfword before downstream use.
        let feedback = force_product(self.lateral_feedback_response, body.lateral_acceleration_mps2 * 4096.0);
        body.lateral_feedback_turns = ((feedback as f64 + 32768.0).rem_euclid(65536.0) - 32768.0) as f32 / 4096.0;
    }

    pub fn apply_world(
        &self,
        input: BodyDynamicsInput,
        seconds: f32,
        forward: &mut ForwardDynamics,
        body: &mut BodyDynamics,
        velocity: &mut VehicleVelocity,
    ) {
        if input.reset_motion || input.first_gear_override {
            forward.force = 0.0;
            velocity.yaw_turns_per_second = 0.0;
            velocity.linear_mps.x = 0.0;
            velocity.linear_mps.z = 0.0;
            if !input.first_gear_override {
                velocity.linear_mps.y = 0.0;
                body.tilt_turns_per_second = Vec2::ZERO;
            }
            // 8003B130 jumps directly past external forces too.
            return;
        }
        if !input.bypass_tyre_acceleration {
            body.brake_stopped = forward.forward_mps.abs() <= self.brake_stop_speed_mps
                && input.slip_activation.iter().all(|&v| v == 0.0)
                && input.braked_wheels.iter().any(|&v| v);
            if body.brake_stopped {
                velocity.yaw_turns_per_second = 0.0;
                velocity.linear_mps.x = 0.0;
                velocity.linear_mps.z = 0.0;
                forward.force = 0.0;
                forward.acceleration_mps2 = 0.0;
                forward.forward_mps = 0.0;
                body.lateral_acceleration_mps2 = 0.0;
                body.lateral_feedback_turns = 0.0;
                body.local_lateral_mps = 0.0;
            } else {
                let ahead = grid(forward.acceleration_mps2 as f64 * seconds as f64, 4096.0);
                let sideways = grid(body.lateral_acceleration_mps2 as f64 * seconds as f64, 4096.0);
                // 8003B260/274 uses the signed LOW word helper, so preserve
                // its wrap/bias at the projection boundary separately.
                let project = |axis: usize| {
                    scaled_force(input.forward_axis[axis], ahead * 4096.0)
                        + scaled_force(input.lateral_axis[axis], sideways * 4096.0)
                };
                velocity.linear_mps.x += project(0) / 4096.0;
                velocity.linear_mps.z += project(1) / 4096.0;
            }
        }
        // 8003B2D4..2E8 keeps mass and duration truncation separate.
        velocity.linear_mps.x += grid(
            force_product(self.inverse_mass, input.external_world_force.x) as f64 / 4096.0 * seconds as f64,
            4096.0,
        );
        velocity.linear_mps.z += grid(
            force_product(self.inverse_mass, input.external_world_force.y) as f64 / 4096.0 * seconds as f64,
            4096.0,
        );
    }
}

fn grid(value: f64, scale: f64) -> f32 {
    ((value * scale).trunc() / scale) as f32
}
fn force_product(response: f32, force: f32) -> f32 {
    (response as f64 * force as f64).trunc() as f32
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyDynamicsSystems {
    Accumulate,
    ApplyWorld,
}

pub struct BodyDynamicsRulesPlugin;
impl Plugin for BodyDynamicsRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<BodyDynamicsTuning>() {
            app.add_plugins(TuningPlugin::<BodyDynamicsTuning>::new(BODY_DYNAMICS_TUNING));
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
            (BodyDynamicsSystems::Accumulate, BodyDynamicsSystems::ApplyWorld)
                .chain()
                .in_set(VehicleSystems::Forces)
                .after(ForwardDynamicsSystems::Integrate)
                .after(TransmissionSystems::Select),
        )
        .add_systems(FixedUpdate, accumulate.in_set(BodyDynamicsSystems::Accumulate))
        .add_systems(FixedUpdate, apply_world.in_set(BodyDynamicsSystems::ApplyWorld));
    }
}

type AccumulationCar = (
    &'static BodyDynamicsInput,
    &'static TyreForces,
    &'static WheelSteering,
    &'static VehicleStepDuration,
    &'static mut VehicleVelocity,
    &'static mut BodyDynamics,
    Option<&'static BodyDynamicsSetup>,
);
fn accumulate(tuning: Res<BodyDynamicsTuning>, mut cars: Query<AccumulationCar>) {
    for (&input, &tyres, &steering, &duration, mut velocity, mut body, setup) in &mut cars {
        setup.map(|v| &v.0).unwrap_or(&tuning).accumulate(input, tyres, steering, duration.0, &mut velocity, &mut body);
    }
}

#[derive(QueryData)]
#[query_data(mutable)]
struct WorldCar {
    input: &'static BodyDynamicsInput,
    duration: &'static VehicleStepDuration,
    velocity: &'static mut VehicleVelocity,
    body: &'static mut BodyDynamics,
    forward: &'static mut ForwardDynamics,
    setup: Option<&'static BodyDynamicsSetup>,
    slip: Option<&'static WheelSlip>,
    transmission: Option<&'static TransmissionState>,
    drive: Option<&'static mut DrivetrainInput>,
}
fn apply_world(tuning: Res<BodyDynamicsTuning>, mut cars: Query<WorldCar>) {
    for mut car in &mut cars {
        let tuning = car.setup.map(|v| &v.0).unwrap_or(&tuning);
        let mut input = *car.input;
        if let Some(slip) = car.slip {
            input.slip_activation = slip.activation;
        }
        if let Some(state) = car.transmission {
            input.first_gear_override = state.first_gear_ticks != 0;
        }
        tuning.apply_world(input, car.duration.0, &mut car.forward, &mut car.body, &mut car.velocity);
        if let Some(mut drive) = car.drive {
            drive.adaptive_angle_turns = car.body.lateral_feedback_turns;
        }
    }
}
