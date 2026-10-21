//! Paired tyre responses and per-wheel longitudinal/lateral forces.
//!
//! Original 800397D0 -> 8003D940 and 8003A380..8003A6F4. Force quantities
//! retain original internal units until body mass/inertia scaling is proven.
//! Contact/grip generation and drivetrain/body integration remain separate.
use super::{
    VehicleSystems, WheelSteering,
    braking::BrakingSystems,
    longitudinal_slip::{LongitudinalSlip, SlipReference, TyreForceDirection, WheelSurfaceMotion, steering_basis},
    slip::{WheelLocalVelocities, WheelSlip},
};
use bevy::prelude::*;
use engine::{prelude::TuningPlugin, shell::GameSystems};
use serde::{Deserialize, Serialize};

pub const TYRE_TUNING: &str = "vehicle/tyres.ron";

/// Shared slip coordinates with independent longitudinal and cornering
/// ordinates, as in the paired descriptors at axle+2C/+6C.
#[derive(Debug, Clone, Reflect, Deserialize, Serialize)]
pub struct TyreSlipKnot {
    pub slip: f32,
    pub longitudinal: f32,
    pub cornering_limit: f32,
}

#[derive(Debug, Clone, Reflect, Deserialize, Serialize)]
pub struct CorneringKnot {
    pub angle_turns: f32,
    pub response: f32,
}

#[derive(Debug, Clone, Reflect, Deserialize, Serialize)]
pub struct TyreAxleTuning {
    pub slip_curve: Vec<TyreSlipKnot>,
    /// Absolute, folded slip-angle response: original axle descriptor +0.
    pub cornering_curve: Vec<CorneringKnot>,
}

#[derive(Debug, Clone, Reflect, Deserialize, Serialize)]
pub struct LowSpeedTyreCorrection {
    /// Correction requires abs(tread) strictly below this speed (8003A5DC).
    pub tread_limit_mps: f32,
    /// Inclusive abs(body forward - tread) bound (8003A5E8..5F4).
    pub body_tread_difference_limit_mps: f32,
    /// Lateral velocity magnitude at which correction saturates (8003A650).
    pub saturation_speed_mps: f32,
    /// Internal force units per m/s, immediate at 8003A684.
    pub force_per_lateral_mps: f32,
    /// Saturated internal force units, 8003A660/67C.
    pub max_force: f32,
    /// Fade slope per m/s below tread_limit, constructed at 8003A69C..6B4.
    pub fade_per_mps: f32,
}

#[derive(Resource, Debug, Clone, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct TyreTuning {
    pub axles: [TyreAxleTuning; 2],
    pub low_speed: LowSpeedTyreCorrection,
}

/// Inputs from contact/load/body processing. Grip capacity is wheel+34 in
/// original internal force units, NOT the normalized /4096 response used
/// by brake assistance. The body velocity is physics+6A4, metres/s.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct TyreContactState {
    pub grip_capacity: [f32; 4],
    pub body_forward_mps: f32,
}

#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct TyreForces {
    /// First and second paired curve outputs: scratch wheel+16 / wheel+46.
    pub longitudinal_response: [f32; 4],
    pub cornering_limit: [f32; 4],
    /// Magnitude of activated, clamped cornering response (wheel+3C).
    pub cornering_response: [f32; 4],
    /// Mutually exclusive original wheel+58/+5C force channels. These are
    /// solver inputs, not body acceleration or already-applied forces.
    pub wheel_reference_force: [f32; 4],
    pub ground_reference_force: [f32; 4],
    /// Speed-response-scaled wheel-reference channel (scratch wheel+4).
    pub wheel_response_force: [f32; 4],
    /// Wheel+54, including the independent low-speed correction.
    pub lateral_force: [f32; 4],
}

impl TyreTuning {
    pub fn evaluate(
        &self,
        contact: TyreContactState,
        motion: WheelSurfaceMotion,
        velocity: WheelLocalVelocities,
        steering: WheelSteering,
        geometric: WheelSlip,
        longitudinal: LongitudinalSlip,
    ) -> TyreForces {
        let mut forces = TyreForces::default();
        for wheel in 0..4 {
            let axle = &self.axles[wheel / 2];
            let [response, limit] = axle.slip_response(longitudinal.ratio[wheel]);
            forces.longitudinal_response[wheel] = response;
            forces.cornering_limit[wheel] = limit;
            // 8003A3C0 floors the wide response/capacity product before
            // applying the separately stored prospective force direction.
            let magnitude = (response as f64 * contact.grip_capacity[wheel] as f64).floor() as f32;
            let signed =
                if longitudinal.force_direction[wheel] == TyreForceDirection::Reverse { -magnitude } else { magnitude };
            if longitudinal.reference[wheel] == SlipReference::Wheel {
                forces.wheel_reference_force[wheel] = signed;
                forces.wheel_response_force[wheel] = scaled_force(motion.speed_response[wheel], signed);
            } else {
                forces.ground_reference_force[wheel] = signed;
            }
            if geometric.activation[wheel] != 0.0 {
                // Nonpositive tread speed uses the opposite wheel heading;
                // 800450E0 wraps the signed difference into [-half,half).
                let reverse = motion.tread_speed_mps[wheel] <= 0.0;
                let difference =
                    steering.angle_turns[wheel] + if reverse { 0.5 } else { 0.0 } - geometric.angle_turns[wheel];
                let angle = (difference + 0.5).rem_euclid(1.0) - 0.5;
                let folded = if angle.abs() > 0.25 { 0.5 - angle.abs() } else { angle.abs() };
                let curve = axle.corner_response(folded) * if angle < 0.0 { -1.0 } else { 1.0 };
                let activated = ((curve as f64 * geometric.activation[wheel] as f64 * 4096.0).trunc() / 4096.0) as f32;
                let corner = activated.clamp(-limit, limit);
                forces.cornering_response[wheel] = corner.abs();
                let lateral = scaled_force(corner, contact.grip_capacity[wheel]);
                forces.lateral_force[wheel] = if reverse { -lateral } else { lateral };
            }
            // 8003A5CC..6D0 runs even when geometric activation is zero.
            forces.lateral_force[wheel] += self.low_speed.correction(
                motion.tread_speed_mps[wheel],
                contact.body_forward_mps,
                velocity.forward_mps[wheel],
                velocity.lateral_mps[wheel],
                steering.angle_turns[wheel],
            );
        }
        forces
    }
}

impl TyreAxleTuning {
    pub fn slip_response(&self, slip: f32) -> [f32; 2] {
        let curve = &self.slip_curve;
        assert!(curve.len() >= 2, "tyre slip curve needs at least two ordered points");
        let index = curve.partition_point(|k| k.slip <= slip);
        if index == 0 || index == curve.len() {
            let point = &curve[if index == 0 { 0 } else { curve.len() - 1 }];
            return [point.longitudinal, point.cornering_limit];
        }
        let (a, b) = (&curve[index - 1], &curve[index]);
        [
            interpolate(slip, a.slip, b.slip, a.longitudinal, b.longitudinal),
            interpolate(slip, a.slip, b.slip, a.cornering_limit, b.cornering_limit),
        ]
    }

    pub fn corner_response(&self, angle_turns: f32) -> f32 {
        let curve = &self.cornering_curve;
        assert!(curve.len() >= 2, "cornering curve needs at least two ordered points");
        let index = curve.partition_point(|k| k.angle_turns <= angle_turns);
        if index == 0 {
            curve[0].response
        } else if index == curve.len() {
            curve[index - 1].response
        } else {
            let (a, b) = (&curve[index - 1], &curve[index]);
            interpolate(angle_turns, a.angle_turns, b.angle_turns, a.response, b.response)
        }
    }
}

/// Shared signed-short interpolation in 8003D848 and 8003D940. Truncate the
/// ordinate DELTA towards zero before adding its lower knot; flooring the
/// complete interpolated ordinate changes descending segments.
fn interpolate(x: f32, x0: f32, x1: f32, y0: f32, y1: f32) -> f32 {
    let delta = ((x as f64 - x0 as f64) * (y1 as f64 - y0 as f64) * 4096.0 / (x1 as f64 - x0 as f64)).trunc();
    (y0 as f64 + delta / 4096.0) as f32
}

/// Signed low-word multiplication with the source's sign-dependent bias
/// before division by 4096 (8007596C), retained at the force-unit boundary.
pub(super) fn scaled_force(response: f32, force: f32) -> f32 {
    let product = (response as f64 * 4096.0).round() * (force as f64).round();
    let bias = if response.is_sign_negative() != force.is_sign_negative() { 4095.0 } else { 0.0 };
    let signed = (product + bias + 2147483648.0).rem_euclid(4294967296.0) - 2147483648.0;
    (signed / 4096.0).floor() as f32
}

impl LowSpeedTyreCorrection {
    pub fn correction(&self, tread: f32, body_forward: f32, forward: f32, lateral: f32, steering: f32) -> f32 {
        if tread.abs() >= self.tread_limit_mps || (body_forward - tread).abs() > self.body_tread_difference_limit_mps {
            return 0.0;
        }
        let [cosine, sine] = steering_basis(steering);
        let project = |a: f32, b: f32| ((a as f64 * b as f64 * 4096.0).trunc() / 4096.0) as f32;
        let sideways = project(cosine, lateral) + project(sine, forward);
        let force = if sideways >= self.saturation_speed_mps {
            self.max_force
        } else if sideways <= -self.saturation_speed_mps {
            -self.max_force
        } else {
            (sideways as f64 * self.force_per_lateral_mps as f64).trunc() as f32
        };
        let fade = (((self.tread_limit_mps as f64 - tread.abs() as f64) * self.fade_per_mps as f64 * 4096.0).floor()
            / 4096.0) as f32;
        scaled_force(fade, force)
    }
}

pub struct TyreForcesRulesPlugin;

impl Plugin for TyreForcesRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<TyreTuning>() {
            app.add_plugins(TuningPlugin::<TyreTuning>::new(TYRE_TUNING));
        }
        app.configure_sets(
            FixedUpdate,
            (
                VehicleSystems::Prepare,
                VehicleSystems::SteerWheels,
                VehicleSystems::ContactSlip,
                VehicleSystems::Forces,
                VehicleSystems::BuildStep,
            )
                .chain()
                .in_set(engine::prelude::EngineSet::Control)
                .in_set(GameSystems::Game),
        )
        .add_systems(
            FixedUpdate,
            calculate
                .in_set(VehicleSystems::Forces)
                .after(BrakingSystems::Allocate)
                .before(super::forward_dynamics::ForwardDynamicsSystems::Integrate),
        );
    }
}

fn calculate(
    tuning: Res<TyreTuning>,
    mut cars: Query<(
        &TyreContactState,
        &WheelSurfaceMotion,
        &WheelLocalVelocities,
        &WheelSteering,
        &WheelSlip,
        &LongitudinalSlip,
        &mut TyreForces,
    )>,
) {
    for (&contact, &motion, &velocity, &steering, &geometric, &longitudinal, mut forces) in &mut cars {
        *forces = tuning.evaluate(contact, motion, velocity, steering, geometric, longitudinal);
    }
}
