//! Adjust individual brake channels for wheel slip and yaw correction.
//!
//! Ports 8003DBE8..8003DE64. Wheel grip, longitudinal slip and the two yaw
//! correction signals are inputs from the still-unfinished contact/body
//! model. See docs/brake-assist-evidence.md for their exact evidence boundary.
use super::VehicleSystems;
use super::braking::{BrakingSystems, WheelBrakeInput};
use bevy::prelude::*;
use engine::prelude::TuningPlugin;
use engine::shell::GameSystems;
use serde::{Deserialize, Serialize};

pub const BRAKE_ASSIST_TUNING: &str = "vehicle/brake_assist.ron";

/// Axle parameters, front then rear. All responses use dimensionless
/// fractions; the physical calibration of grip_response is still open.
#[derive(Resource, Debug, Clone, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct BrakeAssistTuning {
    /// Physics +1FE/+2D6: longitudinal-slip targets, /4096.
    pub slip_target: [f32; 2],
    /// Physics +424/+428: multiplier on each wheel's speed response, /4096.
    pub slip_feedback: [f32; 2],
    /// Physics +42C/+42E: multiplier on the load/camber/surface response, /4096.
    pub grip_feedback: [f32; 2],
    /// Global byte 80046EE8, expressed as a fraction of 100 percent.
    pub yaw_target_strength: f32,
}

/// Inputs at the assistance consumer, front-left/right then rear-left/right.
/// Supplying this component opts into assistance. Cars without it retain
/// ownership of their effective channels (e.g. AI and subsystem replays).
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct BrakeAssistInput {
    /// Wheel +2A: speed-dependent response, /4096 (writer 80030680).
    pub speed_response: [f32; 4],
    /// Wheel +34: load/camber/surface response, /4096 (writer 80039D98).
    pub grip_response: [f32; 4],
    /// Wheel +44: signed longitudinal slip, /4096 (writer 80039744).
    pub longitudinal_slip: [f32; 4],
    /// Scratch +90: yaw shortfall response, /4096 (writer 8003E5E4).
    pub yaw_shortfall: f32,
    /// Scratch +92: lateral correction response, /4096 (writer 8003E678).
    pub lateral_correction: f32,
    /// The sign of physics +64C chooses the side; magnitude is unused here.
    pub positive_yaw: bool,
    /// Caller a1 (normal controls +458), /4096. Zero disables added braking.
    pub correction_gain: f32,
    /// Caller a2 (normal controls +45A), /4096. Applied before added braking.
    pub correction_threshold: f32,
}

impl BrakeAssistTuning {
    /// Reduce slip targets on the yaw-selected side, add correction braking
    /// on the yaw-selected front wheel, then attenuate each nonzero channel.
    pub fn apply(&self, state: BrakeAssistInput, brakes: &mut WheelBrakeInput) {
        let mut targets = [self.slip_target[0], self.slip_target[0], self.slip_target[1], self.slip_target[1]];
        if self.yaw_target_strength != 0.0 && state.yaw_shortfall != 0.0 {
            // 8003DC44..DC78 divides the integer percentage product by 100
            // towards zero, before subtracting from full response.
            let percent = (self.yaw_target_strength as f64 * 100.0).round();
            let reduction = ((state.yaw_shortfall as f64 * 4096.0).round() * percent / 100.0).trunc() / 4096.0;
            let response = (1.0 - reduction as f32).max(0.0);
            let side = usize::from(state.positive_yaw);
            for wheel in [side, side + 2] {
                targets[wheel] = response_product(response, targets[wheel]);
            }
            // The upper clamp runs only on this branch (8003DCE4..DD08).
            targets = targets.map(|target| target.min(1.0));
        }
        let excess = state.lateral_correction - state.correction_threshold;
        if state.correction_gain != 0.0 && excess > 0.0 {
            let wheel = usize::from(state.positive_yaw);
            // 8003DD60 shifts by 9 rather than 12: eight times the ordinary
            // response product, quantized before addition and upper clamp.
            let addition = response_product_at(state.correction_gain, excess, 9);
            brakes.travel[wheel] = (brakes.travel[wheel] + addition).min(1.0);
        }
        for (wheel, target) in targets.into_iter().enumerate() {
            if brakes.travel[wheel] == 0.0 {
                continue;
            }
            let axle = wheel / 2;
            let slip_gain = response_product(state.speed_response[wheel], self.slip_feedback[axle]);
            let grip_gain = response_product(state.grip_response[wheel], self.grip_feedback[axle]);
            let slip_error = response_product(state.longitudinal_slip[wheel] - target, slip_gain);
            let factor = response_product(grip_gain, 1.0 - slip_error).clamp(0.0, 1.0);
            // 8003DE40..DE48 keeps one raw travel unit if attenuation would
            // extinguish a nonzero command. A genuinely zero input stays zero.
            let adjusted = response_product(factor, brakes.travel[wheel]);
            brakes.travel[wheel] = if adjusted == 0.0 { 1.0 / 4096.0 } else { adjusted };
        }
    }
}

/// One original response multiplication boundary: retain the signed low
/// word before flooring onto a /4096 grid (8003DDC0..DE3C). Wrapping can
/// matter for large gains; keep it here instead of in the Bevy systems.
fn response_product(a: f32, b: f32) -> f32 {
    response_product_at(a, b, 12)
}

fn response_product_at(a: f32, b: f32, fractional_bits: u32) -> f32 {
    let product = (a as f64 * 4096.0).round() * (b as f64 * 4096.0).round();
    let signed = (product + 2147483648.0).rem_euclid(4294967296.0) - 2147483648.0;
    ((signed / (1u32 << fractional_bits) as f64).floor() / 4096.0) as f32
}

/// Rules-only, usable without controls, rendering, or a completed tyre model.
pub struct BrakeAssistRulesPlugin;

impl Plugin for BrakeAssistRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<BrakeAssistTuning>() {
            app.add_plugins(TuningPlugin::<BrakeAssistTuning>::new(BRAKE_ASSIST_TUNING));
        }
        app.configure_sets(
            FixedUpdate,
            (BrakingSystems::CopyPedals, BrakingSystems::Assistance, BrakingSystems::Allocate)
                .chain()
                .in_set(VehicleSystems::Forces)
                .in_set(GameSystems::Game),
        )
        .add_systems(FixedUpdate, assist_brakes.in_set(BrakingSystems::Assistance));
    }
}

fn assist_brakes(tuning: Res<BrakeAssistTuning>, mut cars: Query<(&BrakeAssistInput, &mut WheelBrakeInput)>) {
    for (&state, mut brakes) in &mut cars {
        tuning.apply(state, &mut brakes);
    }
}
