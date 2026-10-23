//! Surface resistance and body-forward integration before drivetrain coupling.
//!
//! Original 8003A2A0..A364 and 8003A710..A938. Both tyre channels feed this
//! stage; its result is an intermediate solver state, not world velocity.
//! Contact/load production, aerodynamic preparation and the drivetrain remain
//! separate. See docs/forward-dynamics-evidence.md for units and source quirks.
use super::{
    VehicleStepDuration, VehicleSystems, WheelSteering,
    longitudinal_slip::steering_basis,
    tyre_forces::{TyreContactState, TyreForces, scaled_force},
};
use bevy::prelude::*;
use engine::{prelude::TuningPlugin, shell::GameSystems};
use serde::{Deserialize, Serialize};

pub const FORWARD_DYNAMICS_TUNING: &str = "vehicle/forward_dynamics.ron";

#[derive(Debug, Clone, Reflect, Deserialize, Serialize)]
pub struct ResistanceKnot {
    pub speed_mps: f32,
    /// Global 801C8730 ordinate, normalized by 4096.
    pub response: f32,
}

#[derive(Resource, Debug, Clone, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct ForwardDynamicsTuning {
    /// Physics+28 /4096, in original mass units; kilograms are not established.
    pub body_mass: f32,
    /// Physics+2A /4096, precomputed reciprocal used without wheel coupling.
    pub body_inverse_mass: f32,
    /// Physics+410/+412 /4096, per wheel on the front/rear axle.
    pub wheel_rotational_mass: [f32; 2],
    /// Wheel surface byte +14 selects 80046E00; coefficients normalized /4096.
    pub surface_resistance: Vec<f32>,
    pub resistance_curve: Vec<ResistanceKnot>,
}

/// Explicit contact and aerodynamic inputs. Load is wheel+8 in its original
/// internal unit; zero load also excludes that wheel's rotational mass.
/// Surface indices refer to tuning, without assuming names for original IDs.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct ForwardDynamicsInput {
    pub wheel_load: [f32; 4],
    pub surface: [usize; 4],
    /// Signed force at physics+71A, prepared by speed-squared aerodynamic
    /// processing 8003DAAC..DBA0. Aerodynamics supplies it when present.
    pub aerodynamic_force: f32,
}

/// Solver state immediately before 80046B58. Body/contact/drivetrain code must
/// complete the remaining phases before supplying VehicleVelocity.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct ForwardDynamics {
    /// Scratch+88, signed original internal force units.
    pub surface_resistance: f32,
    /// Scratch+84 /4096, including coupled wheel rotational mass.
    pub effective_inverse_mass: f32,
    /// Physics+734, accumulated tyre/surface force. Aerodynamic force is
    /// included in acceleration only, as in the original.
    pub force: f32,
    /// Physics+724 /4096; preserves the original stopping adjustment too.
    pub acceleration_mps2: f32,
    /// Physics+6A4 /4096 after this phase, before drivetrain corrections.
    pub forward_mps: f32,
    /// The second force channel would cross the signed zero boundary.
    pub stopped: bool,
}

impl ForwardDynamicsTuning {
    pub fn evaluate(
        &self,
        input: ForwardDynamicsInput,
        contact: TyreContactState,
        steering: WheelSteering,
        tyres: TyreForces,
        duration: VehicleStepDuration,
    ) -> ForwardDynamics {
        let load_response: f64 = input
            .wheel_load
            .iter()
            .zip(input.surface)
            .map(|(&load, surface)| (load as f64 * self.surface_resistance[surface] as f64).floor())
            .sum();
        let resistance = -(load_response * self.resistance_response(contact.body_forward_mps.abs()) as f64).floor()
            * contact.body_forward_mps.signum() as f64;
        // f32::signum returns +1 for positive zero: the original explicitly
        // clears resistance at zero speed, even with a nonzero modded curve.
        let resistance = if contact.body_forward_mps == 0.0 { 0.0 } else { resistance as f32 };
        let rotational_mass: f32 = input
            .wheel_load
            .iter()
            .enumerate()
            .filter(|(_, load)| **load != 0.0)
            .map(|(i, _)| self.wheel_rotational_mass[i / 2])
            .sum();
        // 8003A7D8..804 narrows the sum to a signed short, then divides
        // 2^24 by the combined original masses. Ordinary car masses fit;
        // arbitrary overflowing or nonpositive modded masses are unsupported.
        let inverse_mass = if rotational_mass == 0.0 {
            self.body_inverse_mass
        } else {
            ((4096.0 / (self.body_mass as f64 + rotational_mass as f64)).trunc() / 4096.0) as f32
        };
        let basis = steering.angle_turns.map(steering_basis);
        let force: f32 =
            (0..4).map(|i| scaled_force(basis[i][0], tyres.wheel_reference_force[i])).sum::<f32>() + resistance;
        // First phase uses the wide multiply 80075A5C; the second uses the
        // signed low-word helper 8007596C. Keep their boundaries separate.
        let acceleration =
            ((force as f64 + input.aerodynamic_force as f64) * inverse_mass as f64).trunc() as f32 / 4096.0;
        let mut result = ForwardDynamics {
            surface_resistance: resistance,
            effective_inverse_mass: inverse_mass,
            force,
            acceleration_mps2: acceleration,
            forward_mps: contact.body_forward_mps + velocity_change(acceleration, duration.0),
            stopped: false,
        };
        if result.forward_mps == 0.0 {
            return result;
        }
        // 8003A898 reads wheel 0's lateral force on EVERY iteration. This is
        // established by original consuming loads, including asymmetric cases.
        let second_force: f32 = (0..4)
            .map(|i| {
                scaled_force(basis[i][0], tyres.ground_reference_force[i])
                    - scaled_force(basis[i][1], tyres.lateral_force[0])
            })
            .sum();
        let second_acceleration = scaled_force(inverse_mass, second_force) / 4096.0;
        let candidate = result.forward_mps + velocity_change(second_acceleration, duration.0);
        // 8003A8D4 compares sign BITS, so negative -> zero takes the stopping
        // branch while positive -> zero accumulates the full second force.
        if (result.forward_mps < 0.0) != (candidate < 0.0) {
            // 8003A8F8..90C multiplies duration_q16 by -speed_q12 /4096;
            // it does not divide by duration. Preserve this observed source
            // correction rather than substituting a physically ideal stop.
            result.acceleration_mps2 += velocity_change(-result.forward_mps, duration.0 * 16.0);
            result.forward_mps = 0.0;
            result.stopped = true;
        } else {
            result.force += second_force;
            result.acceleration_mps2 += second_acceleration;
            result.forward_mps = candidate;
        }
        result
    }

    /// 80075D2C clamps endpoints and truncates only the interpolated delta.
    pub fn resistance_response(&self, speed_mps: f32) -> f32 {
        let knots = &self.resistance_curve;
        if speed_mps <= knots[0].speed_mps {
            return knots[0].response;
        }
        for pair in knots.windows(2) {
            let [a, b] = pair else { unreachable!() };
            if speed_mps <= b.speed_mps {
                let fraction = (speed_mps as f64 - a.speed_mps as f64) / (b.speed_mps as f64 - a.speed_mps as f64);
                return a.response
                    + (((b.response as f64 - a.response as f64) * fraction * 4096.0).trunc() / 4096.0) as f32;
            }
        }
        knots.last().expect("resistance curve requires knots").response
    }
}

fn velocity_change(acceleration: f32, seconds: f32) -> f32 {
    ((acceleration as f64 * seconds as f64 * 4096.0).trunc() / 4096.0) as f32
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ForwardDynamicsSystems {
    Integrate,
}

pub struct ForwardDynamicsRulesPlugin;

impl Plugin for ForwardDynamicsRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<ForwardDynamicsTuning>() {
            app.add_plugins(TuningPlugin::<ForwardDynamicsTuning>::new(FORWARD_DYNAMICS_TUNING));
        }
        app.configure_sets(
            FixedUpdate,
            (VehicleSystems::Forces, VehicleSystems::BuildStep)
                .chain()
                .in_set(engine::prelude::EngineSet::Control)
                .in_set(GameSystems::Game),
        )
        .configure_sets(FixedUpdate, ForwardDynamicsSystems::Integrate.in_set(VehicleSystems::Forces))
        .add_systems(FixedUpdate, integrate.in_set(ForwardDynamicsSystems::Integrate));
    }
}

fn integrate(
    tuning: Res<ForwardDynamicsTuning>,
    mut cars: Query<(
        &ForwardDynamicsInput,
        &TyreContactState,
        &WheelSteering,
        &TyreForces,
        &VehicleStepDuration,
        &mut ForwardDynamics,
    )>,
) {
    for (&input, &contact, &steering, &tyres, &duration, mut result) in &mut cars {
        *result = tuning.evaluate(input, contact, steering, tyres, duration);
    }
}
