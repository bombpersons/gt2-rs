//! Engine torque curve, drag and hysteretic fuel cut, independent of gears.
//!
//! Engine speed and effective throttle come from the drivetrain boundary;
//! this module does not infer them from pedal travel or integrate wheel speed.
//! Torque uses the original internal unit: a conversion to N·m is unproven.
//! See `docs/vehicle-dynamics-evidence.md` for instructions and recordings.
use bevy::prelude::*;
use engine::prelude::TuningPlugin;
use engine::shell::GameSystems;
use serde::{Deserialize, Serialize};

use super::VehicleSystems;

pub const ENGINE_TORQUE_TUNING: &str = "vehicle/engine.ron";

/// A point on the original clamped engine curve (`car+0x7C`). Speed is in
/// revolutions per second; torque is in original internal torque units.
#[derive(Debug, Clone, Copy, Reflect, Deserialize, Serialize)]
pub struct EngineTorqueKnot {
    pub speed_rev_s: f32,
    pub torque: f32,
}

#[derive(Resource, Debug, Clone, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct EngineTorqueTuning {
    pub torque_curve: Vec<EngineTorqueKnot>,
    /// Engine drag per revolution/second: car+0x10C, raw 9 for the Vitz.
    pub drag_per_rev_s: f32,
    /// Integer RPM thresholds at car+0x108/+0x10A.
    pub redline_rpm: f32,
    pub idle_rpm: f32,
    /// 80035388 subtracts 500 RPM; release is strictly below this band.
    pub fuel_cut_hysteresis_rpm: f32,
}

/// Explicit inputs to torque production. `integer_rpm` is the last refreshed
/// RPM (`car+0x6AC`), not recomputed from speed during this call: curve sampling
/// precedes this tick's drivetrain update, while RPM refresh follows it.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct EngineTorqueInput {
    pub speed_rev_s: f32,
    pub integer_rpm: f32,
    /// Normalized effective throttle after shifts/induction, not pedal travel.
    pub effective_throttle: f32,
}

/// Persistent fuel-cut latch (`car+0x61D`), separate from control/gear state.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct EngineFuelCut {
    pub active: bool,
}

#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct EngineTorque {
    pub full_throttle: f32,
    pub drag: f32,
    pub net: f32,
}

impl EngineTorqueTuning {
    /// Sample the clamped linear curve. 80075D2C truncates the interpolated
    /// torque delta towards zero, including on the descending curve branch.
    /// Keep its one-unit torque resolution because the original consumes
    /// these quantized outputs in the drivetrain.
    pub fn available_torque(&self, speed_rev_s: f32) -> f32 {
        let Some(first) = self.torque_curve.first() else {
            return 0.0;
        };
        if speed_rev_s <= first.speed_rev_s {
            return first.torque;
        }
        for pair in self.torque_curve.windows(2) {
            let [a, b] = [pair[0], pair[1]];
            if speed_rev_s < b.speed_rev_s {
                let fraction =
                    (speed_rev_s as f64 - a.speed_rev_s as f64) / (b.speed_rev_s as f64 - a.speed_rev_s as f64);
                return a.torque + (fraction * (b.torque as f64 - a.torque as f64)).trunc() as f32;
            }
        }
        self.torque_curve.last().unwrap().torque
    }

    /// 8003941C refreshes integer RPM after drivetrain integration. The
    /// minimum is idle RPM minus one; it does not cap RPM at idle.
    pub fn refresh_rpm(&self, speed_rev_s: f32) -> f32 {
        (speed_rev_s as f64 * 60.0).floor().max(self.idle_rpm as f64 - 1.0) as f32
    }
}

impl EngineFuelCut {
    /// Apply the latch decision before producing torque (`8003533C`).
    /// Cut combustion and closed throttle both leave negative engine drag.
    pub fn advance(&mut self, input: EngineTorqueInput, tuning: &EngineTorqueTuning) -> EngineTorque {
        if self.active {
            if input.integer_rpm < tuning.redline_rpm - tuning.fuel_cut_hysteresis_rpm {
                self.active = false;
            }
        } else if input.integer_rpm >= tuning.redline_rpm {
            self.active = true;
        }
        let full_throttle = tuning.available_torque(input.speed_rev_s);
        // 8003E214 calls product >>12 without a rounding bias, hence floor.
        let drag = (input.speed_rev_s as f64 * tuning.drag_per_rev_s as f64).floor() as f32;
        let combustion = if self.active {
            0.0
        } else {
            (input.effective_throttle as f64 * (full_throttle as f64 + drag as f64)).floor() as f32
        };
        EngineTorque { full_throttle, drag, net: combustion - drag }
    }
}

/// Rules-only torque feature. Requires explicit inputs, fuel-cut state and
/// output components on a car; missing drivetrain inputs do not move a car.
pub struct EngineTorqueRulesPlugin;

impl Plugin for EngineTorqueRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<EngineTorqueTuning>() {
            app.add_plugins(TuningPlugin::<EngineTorqueTuning>::new(ENGINE_TORQUE_TUNING));
        }
        app.add_systems(FixedUpdate, advance_engine_torque.in_set(VehicleSystems::Forces).in_set(GameSystems::Game));
    }
}

fn advance_engine_torque(
    tuning: Res<EngineTorqueTuning>,
    mut cars: Query<(&EngineTorqueInput, &mut EngineFuelCut, &mut EngineTorque)>,
) {
    for (input, mut cut, mut torque) in &mut cars {
        *torque = cut.advance(*input, &tuning);
    }
}
