//! Sampled speed-squared drag/downforce and the retained reduction response.
//!
//! Original 8003DAA8..DBE4, before tyre preparation and force evaluation.
//! The traffic/proximity producer of reduction growth remains an explicit input.
use super::{
    VehicleStepDuration, VehicleSystems, contact_motion::floor_low_product, forward_dynamics::ForwardDynamicsInput,
    suspension::SuspensionInput, tyre_forces::TyreContactState,
};
use bevy::{ecs::query::QueryData, prelude::*};
use engine::{prelude::TuningPlugin, shell::GameSystems};
use serde::{Deserialize, Serialize};

pub const AERODYNAMICS_TUNING: &str = "vehicle/aerodynamics.ron";

#[derive(Resource, Clone, Debug, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct AerodynamicsTuning {
    /// Physics+43C and +440/+444 /1024, original force per (m/s)².
    pub drag_force_per_squared_mps: f32,
    pub axle_force_per_squared_mps: [f32; 2],
    /// Global 80046EF4 /4096: multiplier at full retained reduction.
    pub minimum_response: f32,
    /// 8003DB34 shifts q16 seconds by six: a quarter fraction per second.
    pub recovery_per_second: f32,
}
#[derive(Component, Clone, Debug, Reflect, Deserialize, Serialize)]
#[reflect(Component)]
pub struct AerodynamicsSetup(pub AerodynamicsTuning);

#[derive(Component, Default, Clone, Copy, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
#[serde(default)]
pub struct AerodynamicsInput {
    /// Physics+6A4 /4096, retained contact-forward velocity. A shared
    /// TyreContactState supplies this when present.
    pub forward_mps: f32,
    /// Physics+774 /4096 per second, from the prior proximity pass. Zero
    /// recovers; nonzero grows the retained fraction. 80034480 clears and
    /// rebuilds it after movement; that upstream producer is separate.
    pub reduction_growth_per_second: f32,
}
#[derive(Component, Default, Clone, Copy, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
#[serde(default)]
pub struct Aerodynamics {
    /// Physics+776 /4096, signed-halfword retained history.
    pub reduction: f32,
    /// Diagnostics retain the quarter-(m/s)² sample and its response.
    pub squared_speed_mps2: f32,
    pub response: f32,
    pub effective_squared_speed_mps2: f32,
    /// Physics+71A signed halfword, original force units.
    pub drag_force: f32,
    /// Physics+71C/+720 signed words, original force units.
    pub axle_force: [f32; 2],
}

impl AerodynamicsTuning {
    pub fn advance(&self, input: AerodynamicsInput, seconds: f32, state: &mut Aerodynamics) {
        // 8003DAC0 samples |speed| at 1/128 m/s, then squares its signed
        // low word. Dividing that q12 square by four gives (m/s)².
        let speed = (input.forward_mps.abs() as f64 * 128.0).floor() as i64;
        state.squared_speed_mps2 = (speed.wrapping_mul(speed) as i32 >> 12) as f32 / 4.0;
        if input.reduction_growth_per_second != 0.0 {
            // Growth uses an arithmetic floor, then narrows BEFORE the
            // upper-only cap. Negative/overflowing histories are retained.
            let increment = floor_low_product(input.reduction_growth_per_second, seconds * 65536.0, 16);
            state.reduction = short(state.reduction as f64 + increment as f64 / 4096.0).min(1.0);
        } else {
            let recovery = (seconds as f64 * self.recovery_per_second as f64 * 4096.0).trunc() / 4096.0;
            state.reduction = short(state.reduction as f64 - recovery).max(0.0);
        }
        state.response = self.minimum_response
            + floor_low_product(1.0 - self.minimum_response, (1.0 - state.reduction) * 4096.0, 12) / 4096.0;
        let effective = floor_low_product(state.response, state.squared_speed_mps2 * 4.0, 12);
        state.effective_squared_speed_mps2 = effective / 4.0;
        let drag = floor_low_product(self.drag_force_per_squared_mps / 4.0, effective, 12);
        // Direction is restored before the halfword store; zero follows
        // the nonnegative-speed branch in 8003DB94 too.
        state.drag_force = stored_force(if input.forward_mps < 0.0 { drag } else { -drag });
        state.axle_force = self.axle_force_per_squared_mps.map(|c| floor_low_product(c / 4.0, effective, 12));
    }
}
fn short(value: f64) -> f32 {
    (value * 4096.0).round() as i64 as i16 as f32 / 4096.0
}
/// 8003DBA0 stores force as a signed halfword, without a unit conversion.
fn stored_force(value: f32) -> f32 {
    value as i32 as i16 as f32
}

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AerodynamicsSystems {
    Prepare,
}
pub struct AerodynamicsRulesPlugin;
impl Plugin for AerodynamicsRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<AerodynamicsTuning>() {
            app.add_plugins(TuningPlugin::<AerodynamicsTuning>::new(AERODYNAMICS_TUNING));
        }
        app.register_type::<Aerodynamics>()
            .register_type::<AerodynamicsInput>()
            .configure_sets(
                FixedUpdate,
                VehicleSystems::Prepare
                    .before(VehicleSystems::ReadInput)
                    .before(VehicleSystems::Forces)
                    .before(VehicleSystems::Suspension)
                    .in_set(GameSystems::Game),
            )
            .add_systems(FixedUpdate, prepare.in_set(AerodynamicsSystems::Prepare).in_set(VehicleSystems::Prepare));
    }
}
#[derive(QueryData)]
#[query_data(mutable)]
struct AeroCar {
    input: &'static AerodynamicsInput,
    duration: &'static VehicleStepDuration,
    state: &'static mut Aerodynamics,
    setup: Option<&'static AerodynamicsSetup>,
    contact: Option<&'static TyreContactState>,
    forward: Option<&'static mut ForwardDynamicsInput>,
    suspension: Option<&'static mut SuspensionInput>,
}
fn prepare(tuning: Res<AerodynamicsTuning>, mut cars: Query<AeroCar>) {
    for mut car in &mut cars {
        let mut input = *car.input;
        if let Some(contact) = car.contact {
            input.forward_mps = contact.body_forward_mps;
        }
        car.setup.map(|v| &v.0).unwrap_or(&tuning).advance(input, car.duration.0, &mut car.state);
        if let Some(mut forward) = car.forward {
            forward.aerodynamic_force = car.state.drag_force;
        }
        if let Some(mut suspension) = car.suspension {
            suspension.axle_force = car.state.axle_force;
        }
    }
}
