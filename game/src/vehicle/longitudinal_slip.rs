//! Longitudinal tyre slip from tread speed and steered contact velocity.
//!
//! Ports 80039490..80039774, including stopped-wheel handling, reverse,
//! signed ratio sampling and low-speed fade. Contact/rotation dynamics still
//! supply the input velocities. See docs/longitudinal-slip-evidence.md.
use super::{VehicleSystems, WheelSteering, brake_assist::BrakeAssistInput, slip::WheelLocalVelocities};
use bevy::prelude::*;
use engine::{prelude::TuningPlugin, shell::GameSystems};
use serde::{Deserialize, Serialize};

pub const LONGITUDINAL_SLIP_TUNING: &str = "vehicle/longitudinal_slip.ron";

#[derive(Resource, Debug, Clone, Copy, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct LongitudinalSlipTuning {
    /// Inclusive tread-speed dead zone, immediate at 80039528, metres/s.
    pub stationary_wheel_speed_mps: f32,
    /// At this speed the final fade is bypassed (800396F8), metres/s.
    pub full_response_speed_mps: f32,
    /// Low-speed response slope constructed at 80039708..3971C, per m/s.
    pub response_per_mps: f32,
}

/// Wheel rotation/contact solver inputs in front-left/right, rear-left/right
/// order. The tread velocity is wheel+18 (/4096 m/s), not angular velocity.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct WheelSurfaceMotion {
    pub tread_speed_mps: [f32; 4],
    /// Wheel+2A (/4096), speed-dependent response written by 80030680.
    pub speed_response: [f32; 4],
}

/// Which velocity normalizes the slip and controls the final speed fade.
/// Corresponds to scratch wheel+10: +1 wheel, -1 ground. This is not a
/// grounded/airborne flag; a stopped wheel uses Ground even at zero speed.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum SlipReference {
    Wheel,
    #[default]
    Ground,
}

/// The source's prospective longitudinal tyre-force direction (scratch+12).
/// A zero slip ratio can retain a nonzero direction; no force is applied here.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum TyreForceDirection {
    Reverse,
    #[default]
    None,
    Forward,
}

#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct LongitudinalSlip {
    /// Signed wheel+44 response, /4096. Braking normally positive, driving
    /// normally negative; direction is stored separately by the original.
    pub ratio: [f32; 4],
    pub reference: [SlipReference; 4],
    pub force_direction: [TyreForceDirection; 4],
    /// Contact velocity projected onto the wheel's steering axis, metres/s.
    pub rolling_speed_mps: [f32; 4],
}

/// Original steering phase sampling: /4096 turn, then the signed sine curve.
/// All 4096 sine/cosine pairs match the original tables at 80093150/80093950.
/// Negative entries match a +0.5 then truncation quantizer;
/// ordinary symmetric rounding would change 2047 sine values.
pub fn steering_basis(angle_turns: f32) -> [f32; 2] {
    let phase = (angle_turns as f64 * 4096.0).round().rem_euclid(4096.0);
    let sine = |phase: f64| {
        ((phase.rem_euclid(4096.0) * std::f64::consts::TAU / 4096.0).sin() * 4096.0 + 0.5).trunc() as f32 / 4096.0
    };
    [sine(phase + 1024.0), sine(phase)]
}

impl LongitudinalSlipTuning {
    pub fn evaluate(
        &self,
        motion: WheelSurfaceMotion,
        velocity: WheelLocalVelocities,
        steering: WheelSteering,
    ) -> LongitudinalSlip {
        let mut slip = LongitudinalSlip::default();
        for wheel in 0..4 {
            let [cosine, sine] = steering_basis(steering.angle_turns[wheel]);
            // 80039508/39518 call 80075A5C separately: discard each signed
            // product's fraction towards zero before subtracting, not after.
            let ground = product_towards_zero(cosine, velocity.forward_mps[wheel])
                - product_towards_zero(sine, velocity.lateral_mps[wheel]);
            let tread = motion.tread_speed_mps[wheel];
            slip.rolling_speed_mps[wheel] = ground;
            let difference = ground - tread;
            let (ratio, reference, direction, speed) = if tread.abs() <= self.stationary_wheel_speed_mps {
                // 80039528..39568: a stopped wheel uses full braking slip
                // before fading by ground speed, even with zero response gain.
                let direction = if ground > tread {
                    TyreForceDirection::Reverse
                } else if ground < tread {
                    TyreForceDirection::Forward
                } else {
                    TyreForceDirection::None
                };
                (if difference == 0.0 { 0.0 } else { 1.0 }, SlipReference::Ground, direction, ground)
            } else {
                // Signed comparisons preserve the opposite-rotation cases;
                // choosing the larger *absolute* velocity changes their rule.
                let wheel_reference = if tread > 0.0 { tread >= ground } else { tread <= ground };
                let (reference, speed) =
                    if wheel_reference { (SlipReference::Wheel, tread) } else { (SlipReference::Ground, ground) };
                let forward = (tread > 0.0) == wheel_reference;
                let numerator = product_towards_zero(motion.speed_response[wheel], difference);
                (
                    sampled_ratio(numerator, speed).clamp(-1.0, 1.0),
                    reference,
                    if forward { TyreForceDirection::Forward } else { TyreForceDirection::Reverse },
                    speed,
                )
            };
            // The source floors the *positive* fade coefficient, then uses
            // signed truncation for its product with the sampled slip.
            slip.ratio[wheel] = if speed.abs() < self.full_response_speed_mps {
                let fade = ((speed.abs() as f64 * self.response_per_mps as f64 * 4096.0).floor() / 4096.0) as f32;
                product_towards_zero(fade, ratio)
            } else {
                ratio
            };
            slip.reference[wheel] = reference;
            slip.force_direction[wheel] = direction;
        }
        slip
    }
}

/// Signed 1/4096 response boundary in 80075A5C / 8007596C. Speeds and
/// responses in the verified vehicle range fit the helpers' result words.
fn product_towards_zero(a: f32, b: f32) -> f32 {
    ((a as f64 * b as f64 * 4096.0).trunc() / 4096.0) as f32
}

/// 800395AC/39600/39648/3969C store the quotient to a signed halfword
/// *before* clamping. Opposite wheel/contact velocities can cross that
/// boundary; saturating the full quotient first does not reproduce them.
fn sampled_ratio(numerator: f32, denominator: f32) -> f32 {
    let sample = (numerator as f64 / denominator as f64 * 4096.0).trunc();
    ((sample + 32768.0).rem_euclid(65536.0) - 32768.0) as f32 / 4096.0
}

pub struct LongitudinalSlipRulesPlugin;

impl Plugin for LongitudinalSlipRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<LongitudinalSlipTuning>() {
            app.add_plugins(TuningPlugin::<LongitudinalSlipTuning>::new(LONGITUDINAL_SLIP_TUNING));
        }
        app.configure_sets(
            FixedUpdate,
            (VehicleSystems::SteerWheels, VehicleSystems::ContactSlip, VehicleSystems::Forces)
                .chain()
                .in_set(engine::prelude::EngineSet::Control)
                .in_set(GameSystems::Game),
        )
        .add_systems(FixedUpdate, update_longitudinal_slip.in_set(VehicleSystems::ContactSlip));
    }
}

fn update_longitudinal_slip(
    tuning: Res<LongitudinalSlipTuning>,
    mut cars: Query<(
        &WheelSurfaceMotion,
        &WheelLocalVelocities,
        &WheelSteering,
        &mut LongitudinalSlip,
        Option<&mut BrakeAssistInput>,
    )>,
) {
    for (&motion, &velocity, &steering, mut slip, assist) in &mut cars {
        *slip = tuning.evaluate(motion, velocity, steering);
        if let Some(mut assist) = assist {
            assist.longitudinal_slip = slip.ratio;
            assist.speed_response = motion.speed_response;
        }
    }
}
