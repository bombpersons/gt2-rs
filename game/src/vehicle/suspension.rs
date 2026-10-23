//! Road-relative suspension travel, wheel loads and vertical body forces.
//!
//! `80043B54..43F18` samples travel after movement, then evaluates springs,
//! dampers, anti-roll response and body-force transfer. Loads feed next tick's
//! grip preparation. Road sampling, orientation and vertical integration are
//! separate producers/consumers; this stage does not infer a road plane.
use super::{
    VehicleSystems,
    body_dynamics::{BodyDynamics, BodyDynamicsSystems},
    forward_dynamics::{ForwardDynamics, ForwardDynamicsSystems},
    tyre_grip::TyreGripInput,
};
use bevy::{ecs::query::QueryData, prelude::*};
use engine::{prelude::TuningPlugin, shell::GameSystems};
use serde::{Deserialize, Serialize};

pub const SUSPENSION_TUNING: &str = "vehicle/suspension.ron";

#[derive(Debug, Clone, Reflect, Deserialize, Serialize)]
pub struct DamperTuning {
    /// Axle+10/+1C, /4096 m/s, subtracted from absolute sampled velocity.
    pub threshold_mps: f32,
    /// Axle+14/+20 in original force units.
    pub base_force: f32,
    /// Axle+18/+24, force per square root of m/s.
    pub square_root_response: f32,
}
#[derive(Debug, Clone, Reflect, Deserialize, Serialize)]
pub struct SuspensionAxleTuning {
    /// Axle+0/+2/+4 signed halfwords, /4096 metres. Compression is negative.
    pub bump_start_m: f32,
    pub minimum_travel_m: f32,
    pub maximum_travel_m: f32,
    /// Axle+8 minus wheel radius (+3E4/+3E6), /4096 metres.
    pub travel_offset_m: f32,
    /// Axle+C, +2C and +28 respectively. Forces retain original units;
    /// their SI interpretation has not been established.
    pub spring_force_per_m: f32,
    pub bump_force_per_m2: f32,
    pub antiroll_force_per_m2: f32,
    /// Axle+32 signed halfword, added only to tyre load.
    pub preload_force: f32,
    pub rebound: DamperTuning,
    pub compression: DamperTuning,
}
#[derive(Resource, Debug, Clone, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct SuspensionTuning {
    pub axles: [SuspensionAxleTuning; 2],
    /// Physics+1E/+20, /4096 metres, coupling body forces to wheel forces.
    pub lateral_lever_m: f32,
    pub forward_lever_m: f32,
}
#[derive(Component, Debug, Clone, Reflect, Deserialize, Serialize)]
#[reflect(Component)]
pub struct SuspensionSetup(pub SuspensionTuning);

#[derive(Component, Default, Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
#[serde(default)]
pub struct SuspensionInput {
    /// Physics+664; wheel+0, +28 and +42, /4096 metres. These are consumed
    /// road/body observations, not the rendered checkpoint attachment pose.
    pub body_height_m: f32,
    pub road_height_m: [f32; 4],
    pub attachment_height_m: [f32; 4],
    pub road_offset_m: [f32; 4],
    /// Physics+6A0 and +67C, /4096. The normal/orientation producers remain
    /// external. The source divides these before projecting travel.
    pub road_normal_up: f32,
    pub body_up: f32,
    /// Physics+704, /4096 per second, produced by 8004232C. This is an
    /// explicit reciprocal: it is not recomputed from the rounded duration.
    pub inverse_step_seconds: f32,
    /// Physics+71C/+720, original front/rear aerodynamic force units.
    pub axle_force: [f32; 2],
    /// Physics+728 /4096, lateral acceleration supplied by body dynamics.
    pub lateral_acceleration_mps2: f32,
    /// Physics+72C, reserved explicit vertical transfer channel.
    pub vertical_force: f32,
    /// Physics+734, accumulated forward force in original force units.
    pub forward_force: f32,
    /// Physics+786 ==7 skips this car; the parent countdown can also skip
    /// the entire suspension call. Both leave all persistent state alone.
    pub disabled: bool,
}

#[derive(Component, Default, Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
#[serde(default)]
pub struct Suspension {
    /// Wheel+10/+12 signed halfwords, /4096 metres and m/s.
    pub travel_m: [f32; 4],
    pub travel_rate_mps: [f32; 4],
    pub requested_travel_m: [f32; 4],
    /// Physics+719: each bit tests raw travel strictly below the axle minimum.
    pub compressed_mask: u8,
    /// Wheel+48/+4C and +8, in original force units. f64 preserves the
    /// integer force results even for large authored progressive responses.
    pub spring_force: [f64; 4],
    pub damper_force: [f64; 4],
    pub tyre_load: [f64; 4],
    /// Body-facing vertical forces (scratch +18..24). Anti-roll and transfer
    /// affect these, not the wheel+8 loads used by next tick's grip.
    pub body_force: [f64; 4],
}

impl SuspensionTuning {
    pub fn advance(&self, input: SuspensionInput, state: &mut Suspension) {
        if input.disabled {
            return;
        }
        let projection = (input.road_normal_up as f64 / input.body_up as f64 * 4096.0).trunc() / 4096.0;
        state.compressed_mask = 0;
        for i in 0..4 {
            let axle = &self.axles[i / 2];
            let height = input.body_height_m as f64 + input.attachment_height_m[i] as f64
                - input.road_height_m[i] as f64
                - input.road_offset_m[i] as f64;
            // 80043BE8: low-word signed product, then add the axle/radius offset.
            let travel = low_product(height, projection * 4096.0) / 4096.0 + axle.travel_offset_m as f64;
            state.requested_travel_m[i] = travel as f32;
            if travel < axle.minimum_travel_m as f64 {
                state.compressed_mask |= 1 << i;
            }
            if travel > axle.maximum_travel_m as f64 {
                // 80043930..50: reset extension and spring/load, but retain
                // the previous rate. The damper does not execute here.
                state.travel_m[i] = axle.maximum_travel_m;
                state.spring_force[i] = -(axle.preload_force as f64);
                state.damper_force[i] = 0.0;
                state.tyre_load[i] = 0.0;
                continue;
            }
            let limited = travel.max(axle.minimum_travel_m as f64);
            let mut spring = -low_product(limited, axle.spring_force_per_m as f64);
            if limited < axle.bump_start_m as f64 {
                // 80043990..A8 narrows the gap before squaring and samples
                // its square before applying the progressive coefficient.
                let gap = signed_short(axle.bump_start_m as f64 - limited);
                let squared = sampled_square(gap);
                spring += (squared * axle.bump_force_per_m2 as f64).floor();
            }
            // Use the requested travel for velocity, even beyond the lower
            // stop, and the *wide* rate before its signed-halfword store.
            let rate =
                ((travel - state.travel_m[i] as f64) * input.inverse_step_seconds as f64 * 4096.0).trunc() / 4096.0;
            let average = (((rate + state.travel_rate_mps[i] as f64) * 4096.0 / 2.0).trunc() / 4096.0).clamp(-1.0, 1.0);
            let damper = if average < 0.0 { axle.rebound.force(-average) } else { -axle.compression.force(average) };
            state.travel_rate_mps[i] = signed_short(rate) as f32;
            state.travel_m[i] = signed_short(limited) as f32;
            state.spring_force[i] = spring;
            state.damper_force[i] = damper;
            state.tyre_load[i] = (spring + damper + axle.preload_force as f64).max(0.0);
        }
        let antiroll: [f64; 2] = std::array::from_fn(|axle| {
            let difference = state.travel_m[axle * 2] as f64 - state.travel_m[axle * 2 + 1] as f64;
            let square = sampled_square(difference);
            let force = (square * self.axles[axle].antiroll_force_per_m2 as f64).floor();
            if difference < 0.0 { -force } else { force }
        });
        let lateral = low_product(self.lateral_lever_m as f64, -(input.lateral_acceleration_mps2 as f64 * 4096.0));
        let forward =
            low_product(self.forward_lever_m as f64, input.forward_force as f64 - input.vertical_force as f64);
        for i in 0..4 {
            let axle = i / 2;
            let side = if i % 2 == 0 { 1.0 } else { -1.0 };
            let end = if axle == 0 { 1.0 } else { -1.0 };
            state.body_force[i] =
                state.spring_force[i] - side * antiroll[axle] - (input.axle_force[axle] as f64 / 2.0).trunc()
                    + end * forward
                    + side * lateral;
        }
    }
}
impl DamperTuning {
    fn force(&self, speed: f64) -> f64 {
        // 80081288(a0,6) takes an unsigned 32-bit radicand and returns
        // floor(sqrt(raw * 4096)). Preserve its wrap for authored thresholds
        // above the sampled speed; this is not a zero-clamped square root.
        let raw = ((speed - self.threshold_mps as f64) * 4096.0).round() as i64 as u32;
        let root = ((raw as f64 * 4096.0).sqrt().floor()) / 4096.0;
        self.base_force as f64 + (root * self.square_root_response as f64).trunc()
    }
}
/// 8007596C truncates the signed low word with the operand-sign bias.
/// Keep that sampling at the dimension/force boundary, including overflow.
fn low_product(distance: f64, response: f64) -> f64 {
    let a = (distance * 4096.0).round() as i64;
    let b = response.round() as i64;
    let word = a.wrapping_mul(b) as i32;
    let biased = word.wrapping_add(if (a < 0) != (b < 0) { 4095 } else { 0 });
    (biased >> 12) as f64
}
/// 80043998/43D8C/43DA8 sample the signed low square before the next product.
fn sampled_square(distance: f64) -> f64 {
    let raw = (distance * 4096.0).round() as i64;
    (raw.wrapping_mul(raw) as i32 >> 12) as f64 / 4096.0
}
fn signed_short(value: f64) -> f64 {
    ((value * 4096.0 + 32768.0).rem_euclid(65536.0) - 32768.0) / 4096.0
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SuspensionSystems {
    Evaluate,
}
/// Opt into transferring the current forward/body dynamics outputs to
/// suspension after horizontal integration. Without it, explicit inputs are
/// retained for independently driven suspension, recorded scenes and mods.
#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct SuspensionForceTransfer;

pub struct SuspensionRulesPlugin;
impl Plugin for SuspensionRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<SuspensionTuning>() {
            app.add_plugins(TuningPlugin::<SuspensionTuning>::new(SUSPENSION_TUNING));
        }
        app.configure_sets(
            FixedUpdate,
            VehicleSystems::Suspension
                .after(VehicleSystems::Integrate)
                .before(VehicleSystems::RefreshContact)
                .in_set(GameSystems::Game),
        )
        .add_systems(
            FixedUpdate,
            transfer_forces
                .after(BodyDynamicsSystems::ApplyWorld)
                .after(ForwardDynamicsSystems::Integrate)
                .before(SuspensionSystems::Evaluate)
                .in_set(VehicleSystems::Suspension),
        )
        .add_systems(FixedUpdate, evaluate.in_set(SuspensionSystems::Evaluate).in_set(VehicleSystems::Suspension));
    }
}
type ForceTransferCar = (&'static mut SuspensionInput, Option<&'static ForwardDynamics>, Option<&'static BodyDynamics>);
fn transfer_forces(mut cars: Query<ForceTransferCar, With<SuspensionForceTransfer>>) {
    for (mut input, forward, body) in &mut cars {
        // 80043AE0 reads +734 and +728 after 8003B304 has completed.
        // The vertical +72C channel has its own producer and stays explicit.
        if let Some(forward) = forward {
            input.forward_force = forward.force;
        }
        if let Some(body) = body {
            input.lateral_acceleration_mps2 = body.lateral_acceleration_mps2;
        }
    }
}

#[derive(QueryData)]
#[query_data(mutable)]
struct SuspensionCar {
    input: &'static SuspensionInput,
    state: &'static mut Suspension,
    setup: Option<&'static SuspensionSetup>,
    grip: Option<&'static mut TyreGripInput>,
}
fn evaluate(tuning: Res<SuspensionTuning>, mut cars: Query<SuspensionCar>) {
    for mut car in &mut cars {
        car.setup.map(|v| &v.0).unwrap_or(&tuning).advance(*car.input, &mut car.state);
        if !car.input.disabled
            && let Some(mut grip) = car.grip
        {
            grip.load = car.state.tyre_load.map(|v| v as f32);
        }
    }
}
