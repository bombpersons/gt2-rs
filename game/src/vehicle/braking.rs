//! Ordinary wheel-brake demand and the additive rear handbrake.
//!
//! This is preparation for the tyre/drivetrain solver, not braking physics:
//! no velocity or wheel rotation is inferred here. Demand uses original
//! internal brake units; an N·m conversion has not been established.
use bevy::prelude::*;
use engine::prelude::TuningPlugin;
use engine::shell::GameSystems;
use serde::{Deserialize, Serialize};

use super::{PedalTravel, VehicleSystems};

pub const BRAKING_TUNING: &str = "vehicle/braking.ron";

/// Full-travel capacities in original internal brake units. The ordinary
/// coefficients are physics+0x414/+0x418, handbrake is +0x41C. See
/// `docs/vehicle-dynamics-evidence.md` for their consumer and unit boundary.
#[derive(Resource, Debug, Clone, Copy, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct BrakingTuning {
    pub front_capacity: f32,
    pub rear_capacity: f32,
    pub handbrake_capacity: f32,
}

/// Effective brake travel for front left/right then rear left/right. Values
/// are normalized fractions, including the handbrake. Keeping four channels
/// allows later assist/AI rules to supply the original per-wheel inputs.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct WheelBrakeInput {
    pub travel: [f32; 4],
    pub handbrake: f32,
}

impl WheelBrakeInput {
    /// Normal live controls copy pedal travel to all four wheel channels at
    /// 8003E2E0..8003E2F4; effective channels may then be modified separately.
    pub fn from_pedals(pedals: PedalTravel) -> Self {
        Self { travel: [pedals.brake; 4], handbrake: pedals.handbrake }
    }
}

/// Opt into initial shared-pedal copying. Wheel assistance may then
/// modify those channels before allocation. Without this marker, a car's
/// replay/AI systems own its individual WheelBrakeInput channels.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct PedalBrakes;

/// Ordinary service-brake demand and the final demand after rear handbrake
/// addition. These are solver inputs, not forces already applied to the car.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct WheelBrakeDemand {
    pub ordinary: [f32; 4],
    pub total: [f32; 4],
}

impl WheelBrakeDemand {
    /// Initial wheel-state bit 1 (`8003A254..264`) means nonzero prepared
    /// brake demand. Later force phases add other wheel-state flags.
    pub fn is_braking(&self, wheel: usize) -> bool {
        self.total[wheel] != 0.0
    }
}

impl BrakingTuning {
    /// 8003A1B8..A22C scales each channel by its axle capacity, then adds
    /// rear handbrake demand without overriding or clamping ordinary brakes.
    pub fn allocate(&self, input: WheelBrakeInput) -> WheelBrakeDemand {
        let ordinary = std::array::from_fn(|i| {
            let capacity = if i < 2 { self.front_capacity } else { self.rear_capacity };
            (input.travel[i] as f64 * capacity as f64).floor() as f32
        });
        let handbrake = handbrake_demand(input.handbrake, self.handbrake_capacity);
        let mut total = ordinary;
        total[2] += handbrake;
        total[3] += handbrake;
        WheelBrakeDemand { ordinary, total }
    }
}

/// The original handbrake's result is a signed short, even though it is
/// added to full-width service-brake demand. 8003A204/A208 discard the upper
/// product bits before addition. Preserve this in one unit-boundary helper:
/// a capacity 32768 at full travel produces -32768, verified by a controlled
/// original run. Ordinary Vitz capacity 23520 stays in the positive range.
fn handbrake_demand(travel: f32, capacity: f32) -> f32 {
    let whole_units = (travel as f64 * capacity as f64).floor();
    ((whole_units + 32768.0).rem_euclid(65536.0) - 32768.0) as f32
}

/// Rules-only preparation, optionally reading an existing PedalTravel.
/// The feature does not require the player/controller or any rendering.
pub struct BrakingRulesPlugin;

/// The original copies the pedal, applies per-wheel assistance in
/// 8003DBE8..8003DE60, then allocates demand in 80039FC8. The optional
/// BrakeAssistRulesPlugin fills the assistance slot for opted-in cars.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrakingSystems {
    CopyPedals,
    Assistance,
    Allocate,
}

impl Plugin for BrakingRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<BrakingTuning>() {
            app.add_plugins(TuningPlugin::<BrakingTuning>::new(BRAKING_TUNING));
        }
        app.configure_sets(
            FixedUpdate,
            (BrakingSystems::CopyPedals, BrakingSystems::Assistance, BrakingSystems::Allocate)
                .chain()
                .in_set(VehicleSystems::Forces)
                .in_set(GameSystems::Game),
        )
        .add_systems(FixedUpdate, read_pedal_brakes.in_set(BrakingSystems::CopyPedals))
        .add_systems(FixedUpdate, allocate_brakes.in_set(BrakingSystems::Allocate));
    }
}

fn read_pedal_brakes(mut cars: Query<(&PedalTravel, &mut WheelBrakeInput), With<PedalBrakes>>) {
    for (&pedals, mut input) in &mut cars {
        *input = WheelBrakeInput::from_pedals(pedals);
    }
}

fn allocate_brakes(tuning: Res<BrakingTuning>, mut cars: Query<(&WheelBrakeInput, &mut WheelBrakeDemand)>) {
    for (&input, mut demand) in &mut cars {
        *demand = tuning.allocate(input);
    }
}
