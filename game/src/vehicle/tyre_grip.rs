//! Load/camber/surface grip and the optional tyre-condition multiplier.
//!
//! 80039A4C..39DE4 runs before controls and uses current contact load, roll
//! difference and surface. Suspension/contact and the condition accumulator's
//! later producer remain explicit inputs, rather than fitted grip constants.
use super::{
    VehicleSystems, brake_assist::BrakeAssistInput, contact_motion::floor_low_product,
    forward_dynamics::ForwardDynamicsInput, tyre_forces::TyreContactState,
};
use bevy::{ecs::query::QueryData, prelude::*};
use engine::{prelude::TuningPlugin, shell::GameSystems};
use serde::{Deserialize, Serialize};

pub const TYRE_GRIP_TUNING: &str = "vehicle/tyre_grip.ron";

#[derive(Debug, Clone, Reflect, Deserialize, Serialize)]
pub struct GripLoadKnot {
    pub load: f32,
    pub capacity: f32,
}
#[derive(Debug, Clone, Reflect, Deserialize, Serialize)]
pub struct GripCamberKnot {
    pub angle_turns: f32,
    pub response: f32,
}
#[derive(Debug, Clone, Reflect, Deserialize, Serialize)]
pub struct GripAxleTuning {
    pub load_curve: Vec<GripLoadKnot>,
    pub camber_curve: Vec<GripCamberKnot>,
    /// Physics+368/+36A /4096, added after the wheel's signed roll difference.
    pub camber_turns: f32,
}
#[derive(Debug, Clone, Reflect, Deserialize, Serialize)]
pub struct TyreConditionTuning {
    /// Globals 80046F48/5C/54 in original accumulator units. The physical
    /// interpretation of the condition accumulator remains unproven.
    pub maximum: f32,
    pub warm_start: f32,
    pub cold_limit: f32,
    /// Globals 80046F4C/60/58, normalized /4096.
    pub maximum_load_loss: f32,
    pub warm_load_loss: f32,
    pub cold_load_loss: f32,
}
#[derive(Resource, Debug, Clone, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct TyreGripTuning {
    pub axles: [GripAxleTuning; 2],
    /// Physics+348..366 signed halfwords, /256 (not /4096).
    pub surface_response: Vec<f32>,
    /// None corresponds to global 80046F48 ==0, as in B-1.
    pub condition: Option<TyreConditionTuning>,
}
#[derive(Component, Debug, Clone, Reflect, Deserialize, Serialize)]
#[reflect(Component)]
pub struct TyreGripSetup(pub TyreGripTuning);

#[derive(Component, Default, Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
#[serde(default)]
pub struct TyreGripInput {
    /// Wheel+8, in original load units; suspension/contact supplies it.
    pub load: [f32; 4],
    pub surface: [usize; 4],
    /// Physics+646 minus +6F6, /4096 turns, before per-side sign/camber.
    pub roll_difference_turns: f32,
    /// Physics+78D bit 0x10 skips condition updates and load scaling.
    pub bypass_condition: bool,
}
#[derive(Component, Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
#[serde(default)]
pub struct TyreCondition {
    /// Wheel+64, modified later in 8003B310..444; that producer is separate.
    pub accumulator: [f32; 4],
    /// Wheel+38 /4096. Retained unchanged when condition processing is off.
    pub load_scale: [f32; 4],
    /// Wheel+3F, signed indicator. Its presentation meaning is unported.
    pub indicator: [i8; 4],
}
impl Default for TyreCondition {
    fn default() -> Self {
        Self { accumulator: [0.0; 4], load_scale: [1.0; 4], indicator: [0; 4] }
    }
}
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct TyreGrip {
    pub effective_load: [f32; 4],
    pub load_capacity: [f32; 4],
    pub camber_response: [f32; 4],
    /// Wheel+34, original force units, consumed by tyre forces/assistance.
    pub capacity: [f32; 4],
}

impl TyreConditionTuning {
    pub fn advance(&self, state: &mut TyreCondition) {
        for i in 0..4 {
            let value = state.accumulator[i];
            let (scale, indicator) = if value >= self.maximum {
                state.accumulator[i] = self.maximum;
                (1.0 - self.maximum_load_loss, 127)
            } else if value >= self.warm_start {
                let fraction = ratio(value - self.warm_start, self.maximum - self.warm_start);
                let loss = floor_grid(fraction as f64 * (self.maximum_load_loss - self.warm_load_loss) as f64);
                let indicator = if fraction >= 1.0 { 127 } else { (fraction * 4096.0) as i32 / 64 + 64 };
                (1.0 - loss - self.warm_load_loss, indicator)
            } else if value >= 0.0 {
                let fraction = ratio(value, self.warm_start);
                let loss = floor_grid(fraction as f64 * self.warm_load_loss as f64);
                (1.0 - loss, if fraction >= 1.0 { 63 } else { (fraction * 4096.0) as i32 / 64 })
            } else if value < self.cold_limit {
                // 80039C8C deliberately restores full scale below the cold
                // limit; do not extrapolate the adjacent reduction curve.
                (1.0, -128)
            } else {
                let fraction = ratio(-value, -self.cold_limit);
                let loss = floor_grid(fraction as f64 * self.cold_load_loss as f64);
                (1.0 - loss, if fraction >= 1.0 { -63 } else { -((fraction * 4096.0) as i32 / 64) })
            };
            state.load_scale[i] = signed_half_grid(scale);
            state.indicator[i] = indicator as i8;
        }
    }
}

impl TyreGripTuning {
    pub fn evaluate(&self, input: TyreGripInput, state: &mut TyreCondition) -> TyreGrip {
        let condition = self.condition.as_ref().filter(|_| !input.bypass_condition);
        if let Some(tuning) = condition {
            tuning.advance(state);
        }
        let mut result = TyreGrip::default();
        for i in 0..4 {
            let axle = &self.axles[i / 2];
            result.effective_load[i] = if condition.is_some() {
                floor_low_product(state.load_scale[i], input.load[i], 12)
            } else {
                input.load[i]
            };
            result.load_capacity[i] = axle.load_response(result.effective_load[i]);
            // 80039D3C..54 adds, takes abs, THEN narrows to signed short.
            let roll = if i % 2 == 0 { -input.roll_difference_turns } else { input.roll_difference_turns };
            let camber = signed_half_grid((roll + axle.camber_turns).abs());
            result.camber_response[i] = axle.camber_response(camber);
            let camber_capacity = floor_low_product(result.camber_response[i], result.load_capacity[i], 12);
            // Surface response is q8. Passing /16 preserves its actual
            // multiply word before the source's shift by 8 at 80039D90.
            result.capacity[i] = floor_low_product(self.surface_response[input.surface[i]] / 16.0, camber_capacity, 8);
        }
        result
    }
}
impl GripAxleTuning {
    pub fn load_response(&self, load: f32) -> f32 {
        curve(load, self.load_curve.iter().map(|k| (k.load, k.capacity)), 1.0)
    }
    pub fn camber_response(&self, angle: f32) -> f32 {
        curve(angle, self.camber_curve.iter().map(|k| (k.angle_turns, k.response)), 4096.0)
    }
}
fn curve(x: f32, points: impl Iterator<Item = (f32, f32)>, scale: f64) -> f32 {
    let mut points = points;
    let (mut x0, mut y0) = points.next().expect("grip curve requires knots");
    if x <= x0 {
        return y0;
    }
    for (x1, y1) in points {
        if x <= x1 {
            return y0
                + (((y1 as f64 - y0 as f64) * (x as f64 - x0 as f64) / (x1 as f64 - x0 as f64) * scale).trunc() / scale)
                    as f32;
        }
        (x0, y0) = (x1, y1);
    }
    y0
}
fn ratio(a: f32, b: f32) -> f32 {
    ((a as f64 * 4096.0 / b as f64).trunc() / 4096.0) as f32
}
fn floor_grid(value: f64) -> f32 {
    ((value * 4096.0).floor() / 4096.0) as f32
}
fn signed_half_grid(value: f32) -> f32 {
    (((value as f64 * 4096.0 + 32768.0).rem_euclid(65536.0) - 32768.0) / 4096.0) as f32
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TyreGripSystems {
    Prepare,
}
pub struct TyreGripRulesPlugin;
impl Plugin for TyreGripRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<TyreGripTuning>() {
            app.add_plugins(TuningPlugin::<TyreGripTuning>::new(TYRE_GRIP_TUNING));
        }
        app.configure_sets(
            FixedUpdate,
            VehicleSystems::Prepare.before(VehicleSystems::ReadInput).in_set(GameSystems::Game),
        )
        .add_systems(
            FixedUpdate,
            prepare
                .in_set(TyreGripSystems::Prepare)
                .in_set(VehicleSystems::Prepare)
                .before(super::slip::update_wheel_slip),
        );
    }
}
#[derive(QueryData)]
#[query_data(mutable)]
struct GripCar {
    input: &'static TyreGripInput,
    condition: &'static mut TyreCondition,
    grip: &'static mut TyreGrip,
    setup: Option<&'static TyreGripSetup>,
    contact: Option<&'static mut TyreContactState>,
    assist: Option<&'static mut BrakeAssistInput>,
    forward: Option<&'static mut ForwardDynamicsInput>,
}
fn prepare(tuning: Res<TyreGripTuning>, mut cars: Query<GripCar>) {
    for mut car in &mut cars {
        *car.grip = car.setup.map(|v| &v.0).unwrap_or(&tuning).evaluate(*car.input, &mut car.condition);
        if let Some(mut contact) = car.contact {
            contact.grip_capacity = car.grip.capacity;
        }
        if let Some(mut assist) = car.assist {
            assist.grip_response = car.grip.capacity.map(|v| v / 4096.0);
        }
        if let Some(mut forward) = car.forward {
            forward.wheel_load = car.input.load;
            forward.surface = car.input.surface;
        }
    }
}
