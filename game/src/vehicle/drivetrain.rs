//! Engine engagement, gearing and front/rear axle coupling (80045AE8).
//!
//! Torque controllers supply EngineTorque; this feature integrates engine
//! clutch slip and axle rotation, then feeds the wheel differential phase.
//! Contact/body solving and alternate torque-controller producers are separate.
use super::{
    VehicleStepDuration, VehicleSystems,
    braking::WheelBrakeDemand,
    engine_torque::{EngineTorque, EngineTorqueInput, EngineTorqueSystems},
    wheel_rotation::{AxleDrive, WheelReactionTorque, WheelRotationState, WheelRotationSystems},
};
use bevy::ecs::query::QueryData;
use bevy::prelude::*;
use engine::{prelude::TuningPlugin, shell::GameSystems};
use serde::{Deserialize, Serialize};

pub const DRIVETRAIN_TUNING: &str = "vehicle/drivetrain.ron";

#[derive(Debug, Clone, Copy, Reflect, Deserialize, Serialize)]
pub enum Axle {
    Front,
    Rear,
}
impl Axle {
    fn index(self) -> usize {
        match self {
            Self::Front => 0,
            Self::Rear => 1,
        }
    }
}

/// Source physics+370: ordinary driven axles and the shared centre modes.
#[derive(Debug, Clone, Copy, Reflect, Deserialize, Serialize)]
pub enum DriveLayout {
    Rear,
    Front,
    Viscous,
    Open,
    Clutch { front_fraction: f32 },
    AdaptiveClutch,
    Split { front_fraction: f32 },
}

/// Default setup. A car's DrivetrainSetup component can override it.
#[derive(Resource, Debug, Clone, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct DrivetrainTuning {
    pub layout: DriveLayout,
    /// Physics+373 chooses which axle wins a centre slip crossing.
    pub reference_axle: Axle,
    /// Physics+3A4 indexed by gear; reverse is slot zero, with separate sign.
    pub gear_ratios: Vec<f32>,
    /// Physics+400/+404 /16. Original internal torque divided by these
    /// inertias gives angular acceleration; not calibrated SI inertias.
    pub axle_inertia: [f32; 2],
    /// Physics+378 /16, reflected through the selected gear ratio.
    pub engine_inertia: f32,
    /// Physics+408 /256, response to the centre torque difference.
    pub centre_rotation_response: f32,
    /// Physics+374, torque per rad/s for centre viscous/adaptive response.
    pub centre_viscosity: f32,
    /// Physics+376: permit a hard centre lock in the adaptive mode.
    pub adaptive_lock: bool,
    /// Physics+376 also supplies the clutch hold/fallback torque threshold.
    pub centre_lock_torque: f32,
    /// Physics+40C /4096. Converts engine torque to a /4096 rev/s² sample.
    pub engine_rotation_response: f32,
    /// 80039400 uses a sampled inverse tau to convert angular shaft speed.
    pub revolutions_per_radian: f32,
    /// Physics+3F4/+3F6, fixed clutch and nonpositive-torque clutch capacities.
    pub fixed_clutch_torque: f32,
    pub coast_clutch_torque: f32,
    /// Speed-difference widening in 800392D8..39318.
    pub clutch_free_difference_rev_s: f32,
    pub clutch_gain_per_rev_s: f32,
    pub max_clutch_gain: f32,
    /// Adaptive-centre angle taper and brake gate, 800459BC..45A44.
    pub adaptive_full_angle_turns: f32,
    pub adaptive_min_angle_turns: f32,
    pub adaptive_min_response: f32,
    pub adaptive_taper_intercept: f32,
    pub lock_brake_travel: f32,
}

/// A car-specific authored setup, using the same tuning schema and systems.
/// Keep it distinct from the resource: Bevy resources require IsResource.
#[derive(Component, Debug, Clone, Reflect, Deserialize, Serialize)]
#[reflect(Component)]
pub struct DrivetrainSetup(pub DrivetrainTuning);

/// Physics+619. Values 2 and 4 share the slipping-clutch path.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum EngineEngagement {
    #[default]
    Disconnected,
    Coupled,
    Synchronizing,
    Slipping,
    FixedClutch,
}

#[derive(Component, Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
#[serde(default)]
pub struct DrivetrainInput {
    pub gear: usize,
    pub engagement: EngineEngagement,
    /// Source controller 8004530C owns its engine rotation and returned
    /// torque. Other sources leave clutch-slip engine integration here.
    pub engine_controls_rotation: bool,
    /// Alternate torque controllers/induction can supply EngineTorque
    /// themselves instead of the basic combustion plugin.
    pub externally_produced_torque: bool,
    /// Scratch+8C /4096 from the variable-ratio controller; other sources 1.
    pub engine_inertia_fraction: f32,
    /// Physics+64A /4096. Its physical angle meaning is not yet established.
    pub adaptive_angle_turns: f32,
    pub brake_travel: f32,
    /// Global 801D5866 == 3 enables the adaptive locked-step exception.
    pub alternate_locked_step: bool,
    /// Axles whose wheel differential integrates independent wheels (mode 0)
    /// skip the ordinary per-axle shaft update, as at 800463AC.
    pub independent_wheels: [bool; 2],
}
impl Default for DrivetrainInput {
    fn default() -> Self {
        Self {
            gear: 1,
            engagement: EngineEngagement::Disconnected,
            engine_controls_rotation: false,
            externally_produced_torque: false,
            engine_inertia_fraction: 1.0,
            adaptive_angle_turns: 0.0,
            brake_travel: 0.0,
            alternate_locked_step: false,
            independent_wheels: [false; 2],
        }
    }
}

#[derive(Component, Default, Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
#[serde(default)]
pub struct DrivetrainState {
    /// Right-wheel history slots +4DE/+5AE /4096. In combined centre modes
    /// they hold common and split increments; otherwise one per axle.
    pub previous_angular_delta: [f32; 2],
    pub centre_locked: bool,
}

#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct DrivetrainOutput {
    pub gearbox_torque: f32,
    /// Scratch+7C/+80, rebuilt from current engine/axle rotation by 3932C.
    pub clutch_speeds_rev_s: [f32; 2],
    /// Additions to wheel flags: bit 2 (coupled), bit 3 (driven).
    pub wheel_flags: [u8; 4],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CentreMotion {
    Separate,
    Transferring,
    Open,
    Locked,
}

/// Force producers feeding the coupled rotation phase in this tick.
#[derive(Default, Debug, Clone, Copy)]
pub struct DrivetrainForces {
    pub engine: EngineTorque,
    pub brakes: WheelBrakeDemand,
    pub wheels: WheelReactionTorque,
}

impl DrivetrainTuning {
    /// 8003932C: choose the layout's driven shaft, apply reverse and gear
    /// ratio, then convert positive angular speed to engine rev/s. The last
    /// two products floor separately; the split weighted speed truncates.
    pub fn geared_engine_speed(&self, gear: usize, drive: AxleDrive) -> f32 {
        let [front, rear] = drive.radians_per_second;
        let shaft = match self.layout {
            DriveLayout::Rear | DriveLayout::Clutch { .. } | DriveLayout::AdaptiveClutch => rear,
            DriveLayout::Front | DriveLayout::Viscous => front,
            DriveLayout::Open => mean(front, rear),
            DriveLayout::Split { front_fraction } => {
                rear + sample((front as f64 - rear as f64) * front_fraction as f64)
            }
        };
        let shaft = if gear == 0 { -shaft } else { shaft };
        if shaft <= 0.0 {
            return 0.0;
        }
        let geared = (shaft as f64 * self.gear_ratios[gear] as f64 * 4096.0).floor() / 4096.0;
        ((geared * self.revolutions_per_radian as f64 * 4096.0).floor() / 4096.0) as f32
    }

    fn clutch_speeds(&self, input: DrivetrainInput, engine: EngineTorqueInput, drive: AxleDrive) -> [f32; 2] {
        if matches!(input.engagement, EngineEngagement::Synchronizing | EngineEngagement::FixedClutch) {
            [engine.speed_rev_s, self.geared_engine_speed(input.gear, drive)]
        } else {
            [0.0; 2]
        }
    }

    pub fn advance(
        &self,
        input: DrivetrainInput,
        forces: DrivetrainForces,
        seconds: f32,
        engine: &mut EngineTorqueInput,
        drive: &mut AxleDrive,
        state: &mut DrivetrainState,
    ) -> DrivetrainOutput {
        let DrivetrainForces { engine: torque, brakes, wheels: reaction } = forces;
        let engaged = input.engagement != EngineEngagement::Disconnected;
        let clutch_speeds = self.clutch_speeds(input, *engine, *drive);
        let transmitted = if input.engine_controls_rotation {
            if engaged { torque.net } else { 0.0 }
        } else if !engaged {
            0.0
        } else if input.engagement == EngineEngagement::Coupled {
            torque.net
        } else {
            let mut clutch = if input.engagement == EngineEngagement::FixedClutch {
                self.fixed_clutch_torque
            } else if torque.net <= 0.0 {
                self.coast_clutch_torque
            } else {
                // 800392D8: widen positive-torque clutch capacity above
                // 34133/4096 rev/s speed difference, capped at 1.125.
                let difference = (clutch_speeds[0] - clutch_speeds[1]).abs();
                let gain = (1.0
                    + (difference as f64 - self.clutch_free_difference_rev_s as f64).max(0.0)
                        * self.clutch_gain_per_rev_s as f64)
                    .min(self.max_clutch_gain as f64);
                let gain = (gain * 4096.0).floor() / 4096.0;
                (gain * torque.net as f64).floor() as f32
            };
            if clutch_speeds[0] < clutch_speeds[1] {
                clutch = -clutch;
            }
            // 80045CFC..D24: engine rotates from the untransmitted torque.
            let acceleration =
                ((torque.net as f64 - clutch as f64) * self.engine_rotation_response as f64).trunc() / 4096.0;
            engine.speed_rev_s += sample(acceleration * seconds as f64);
            clutch
        };
        let ratio = self.gear_ratios[input.gear];
        let gearbox = (transmitted as f64 * if input.gear == 0 { -ratio as f64 } else { ratio as f64 }).trunc() as f32;
        let reaction = [reaction.prepared[0] + reaction.prepared[1], reaction.prepared[2] + reaction.prepared[3]];
        let mut output = DrivetrainOutput { gearbox_torque: gearbox, clutch_speeds_rev_s: clutch_speeds, ..default() };
        if engaged {
            let axle_flags = match self.layout {
                DriveLayout::Front => [8, 0],
                DriveLayout::Viscous => [8, 4],
                DriveLayout::Open | DriveLayout::Split { .. } => [4, 8],
                _ => [0, 8],
            };
            output.wheel_flags = [axle_flags[0], axle_flags[0], axle_flags[1], axle_flags[1]];
        }
        let mut motion = CentreMotion::Separate;
        let mut front_torque = 0.0;
        match self.layout {
            DriveLayout::Viscous => motion = CentreMotion::Transferring,
            DriveLayout::Open => motion = CentreMotion::Open,
            DriveLayout::Clutch { front_fraction } | DriveLayout::Split { front_fraction } => {
                let capacity = if matches!(self.layout, DriveLayout::Split { .. }) {
                    Some((front_fraction as f64 * gearbox as f64).trunc() as f32)
                } else if front_fraction == 1.0
                    || gearbox <= 0.0
                    || input.gear == 0 && input.engagement == EngineEngagement::Coupled
                {
                    None
                } else {
                    Some((front_fraction as f64 * gearbox as f64).floor() as f32)
                };
                (motion, front_torque) = self.clutch_transfer(capacity, gearbox, reaction, drive, state);
            }
            DriveLayout::AdaptiveClutch => {
                let ratio = self.adaptive_transfer(input, gearbox, reaction, drive);
                let capacity = ratio.map(|ratio| ((ratio as f64 * gearbox as f64).floor() / 2.0).trunc() as f32);
                (motion, front_torque) = self.clutch_transfer(capacity, gearbox, reaction, drive, state);
            }
            _ => {}
        }
        if matches!(self.layout, DriveLayout::Clutch { .. } | DriveLayout::AdaptiveClutch | DriveLayout::Split { .. })
            && motion != CentreMotion::Separate
        {
            output.wheel_flags[0] |= 4;
            output.wheel_flags[1] |= 4;
        }
        let reflected = if !input.engine_controls_rotation && input.engagement == EngineEngagement::Coupled {
            // 80045F88/FAC use floor, preserving the source inertia grid.
            let inertia = (self.engine_inertia as f64 * 16.0 * ratio as f64).floor();
            (inertia * input.engine_inertia_fraction as f64).floor() as f32 / 16.0
        } else {
            0.0
        };
        if matches!(motion, CentreMotion::Locked | CentreMotion::Open) {
            let old_common = if motion == CentreMotion::Locked {
                drive.radians_per_second[0]
            } else {
                mean(drive.radians_per_second[0], drive.radians_per_second[1])
            };
            let mut delta = angular_delta(
                gearbox + reaction[0] + reaction[1],
                self.axle_inertia[0] + self.axle_inertia[1] + reflected,
                seconds,
            );
            let direct_step = motion == CentreMotion::Locked
                && input.alternate_locked_step
                && matches!(self.layout, DriveLayout::AdaptiveClutch)
                && input.brake_travel > self.lock_brake_travel;
            let mut common =
                old_common + if direct_step { delta } else { mean(state.previous_angular_delta[0], delta) };
            if stops_with_brakes(old_common, common, brakes.total.iter().sum()) {
                common = 0.0;
                delta = 0.0;
            }
            state.previous_angular_delta[0] = store_increment(delta);
            if motion == CentreMotion::Locked {
                state.previous_angular_delta[1] = store_increment(delta);
                drive.radians_per_second = [common; 2];
            } else {
                let difference = drive.radians_per_second[0] - drive.radians_per_second[1];
                let resistance = (difference as f64 * self.centre_viscosity as f64).trunc() as f32;
                let acceleration = ((reaction[0] as f64 - reaction[1] as f64 - resistance as f64)
                    * self.centre_rotation_response as f64)
                    .trunc()
                    / 4096.0;
                let mut delta = sample(acceleration * seconds as f64);
                let old_half = sample(difference as f64 / 2.0);
                let mut split = old_half + mean(state.previous_angular_delta[1], delta);
                // 80046254 tests the signed low word, not the wide sign.
                if product_word_negative(old_half, split) {
                    split = 0.0;
                    delta = -old_half;
                }
                state.previous_angular_delta[1] = store_increment(delta);
                drive.radians_per_second = [common + split, common - split];
            }
            drive.torque = [(gearbox as f64 / 2.0).trunc() as f32; 2];
            return output;
        }
        let mut inertia = self.axle_inertia;
        match self.layout {
            DriveLayout::Rear => {
                inertia[0] = 0.0;
                inertia[1] += reflected;
                drive.torque = [0.0, gearbox];
            }
            DriveLayout::Front => {
                inertia[0] += reflected;
                inertia[1] = 0.0;
                drive.torque = [gearbox, 0.0];
            }
            DriveLayout::Viscous => {
                inertia[0] += reflected;
                let transfer = ((drive.radians_per_second[0] as f64 - drive.radians_per_second[1] as f64)
                    * self.centre_viscosity as f64)
                    .trunc() as f32;
                drive.torque = [gearbox - transfer, transfer];
            }
            DriveLayout::Clutch { .. } | DriveLayout::AdaptiveClutch | DriveLayout::Split { .. } => {
                inertia[1] += reflected;
                drive.torque = [front_torque, gearbox - front_torque];
            }
            DriveLayout::Open => unreachable!(),
        }
        let old_difference = drive.radians_per_second[0] - drive.radians_per_second[1];
        for axle in 0..2 {
            if input.independent_wheels[axle] {
                continue;
            }
            let mut delta = angular_delta(drive.torque[axle] + reaction[axle], inertia[axle], seconds);
            let old = drive.radians_per_second[axle];
            let mut speed = old + mean(state.previous_angular_delta[axle], delta);
            if stops_with_brakes(old, speed, brakes.total[axle * 2] + brakes.total[axle * 2 + 1]) {
                speed = 0.0;
                delta = 0.0;
            }
            drive.radians_per_second[axle] = speed;
            state.previous_angular_delta[axle] = store_increment(delta);
        }
        let difference = drive.radians_per_second[0] - drive.radians_per_second[1];
        if motion == CentreMotion::Transferring
            && (old_difference > 0.0 && difference <= 0.0 || old_difference < 0.0 && difference >= 0.0)
        {
            let reference = self.reference_axle.index();
            drive.radians_per_second[1 - reference] = drive.radians_per_second[reference];
            if matches!(self.layout, DriveLayout::Clutch { .. } | DriveLayout::AdaptiveClutch) {
                state.centre_locked = true;
            }
        }
        output
    }

    fn clutch_transfer(
        &self,
        capacity: Option<f32>,
        gearbox: f32,
        reaction: [f32; 2],
        drive: &AxleDrive,
        state: &mut DrivetrainState,
    ) -> (CentreMotion, f32) {
        if state.centre_locked {
            if capacity.is_none() || ((reaction[0] - reaction[1]) - gearbox).abs() <= self.centre_lock_torque {
                return (CentreMotion::Locked, 0.0);
            }
            state.centre_locked = false;
        }
        let transfer = capacity.unwrap_or_else(|| {
            if drive.radians_per_second[1] < drive.radians_per_second[0] {
                -self.centre_lock_torque
            } else {
                self.centre_lock_torque
            }
        });
        (if transfer == 0.0 { CentreMotion::Separate } else { CentreMotion::Transferring }, transfer)
    }

    fn adaptive_transfer(
        &self,
        input: DrivetrainInput,
        torque: f32,
        reaction: [f32; 2],
        drive: &AxleDrive,
    ) -> Option<f32> {
        // 800459BC..9F4: full response through 1/4 turn, taper to 1/4
        // response at one turn. Physical interpretation of +64A is open.
        let angle = input.adaptive_angle_turns.abs();
        let fade = if angle <= self.adaptive_full_angle_turns {
            1.0
        } else if angle >= self.adaptive_min_angle_turns {
            self.adaptive_min_response
        } else {
            self.adaptive_taper_intercept - angle
        };
        if self.adaptive_lock
            && (input.engagement == EngineEngagement::Disconnected
                || input.gear == 0 && input.engagement == EngineEngagement::Coupled
                || fade == 1.0
                    && (input.engagement == EngineEngagement::Synchronizing
                        || input.brake_travel > self.lock_brake_travel
                        || torque > 0.0 && torque + reaction[1] > 0.0
                        || torque < 0.0 && torque + reaction[1] < 0.0))
        {
            return None;
        }
        let difference = (drive.radians_per_second[1] - drive.radians_per_second[0]).abs();
        let response = (self.centre_viscosity as f64 * difference as f64).floor();
        let response = (fade as f64 * response).floor() as f32 / 4096.0;
        if response >= 1.0 { if self.adaptive_lock { None } else { Some(1.0) } } else { Some(response) }
    }
}

fn sample(value: f64) -> f32 {
    ((value * 4096.0).trunc() / 4096.0) as f32
}
fn mean(a: f32, b: f32) -> f32 {
    sample((a as f64 + b as f64) / 2.0)
}
fn store_increment(value: f32) -> f32 {
    (((value as f64 * 4096.0 + 32768.0).rem_euclid(65536.0) - 32768.0) / 4096.0) as f32
}
fn angular_delta(torque: f32, inertia: f32, seconds: f32) -> f32 {
    if inertia == 0.0 {
        return 0.0;
    }
    sample(sample(torque as f64 / inertia as f64) as f64 * seconds as f64)
}
fn stops_with_brakes(old: f32, new: f32, brake_sum: f32) -> bool {
    // 80075BF4's floored wide product precedes the sign test. It can
    // return zero for tiny opposite signs; retain that boundary.
    let product = (old as f64 * new as f64 * 4096.0).floor() as i64;
    brake_sum != 0.0 && (old == 0.0 || (product as i32) < 0)
}
fn product_word_negative(a: f32, b: f32) -> bool {
    (((a as f64 * 4096.0).round() as i64).wrapping_mul((b as f64 * 4096.0).round() as i64) as i32) < 0
}

pub struct DrivetrainRulesPlugin;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DrivetrainSystems {
    Prepare,
}
impl Plugin for DrivetrainRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<DrivetrainTuning>() {
            app.add_plugins(TuningPlugin::<DrivetrainTuning>::new(DRIVETRAIN_TUNING));
        }
        app.configure_sets(
            FixedUpdate,
            (VehicleSystems::Forces, VehicleSystems::BuildStep)
                .chain()
                .in_set(engine::prelude::EngineSet::Control)
                .in_set(GameSystems::Game),
        )
        .add_systems(
            FixedUpdate,
            prepare
                .in_set(DrivetrainSystems::Prepare)
                .in_set(VehicleSystems::Forces)
                .before(EngineTorqueSystems::Prepare),
        )
        .add_systems(
            FixedUpdate,
            advance
                .in_set(WheelRotationSystems::AxleDrive)
                .in_set(VehicleSystems::Forces)
                .after(EngineTorqueSystems::Prepare),
        );
    }
}
fn prepare(
    default_tuning: Res<DrivetrainTuning>,
    mut cars: Query<(&DrivetrainInput, &AxleDrive, &mut EngineTorqueInput, Option<&DrivetrainSetup>)>,
) {
    for (&input, &drive, mut engine, tuning) in &mut cars {
        let tuning = tuning.map(|setup| &setup.0).unwrap_or(&default_tuning);
        if input.externally_produced_torque {
            continue;
        }
        // 8003A178..194: the fixed-clutch launch phase chooses binary
        // throttle by the two clutch speeds before requesting engine torque.
        if input.engagement == EngineEngagement::FixedClutch {
            let [current, geared] = tuning.clutch_speeds(input, *engine, drive);
            engine.effective_throttle = if geared < current { 0.0 } else { 1.0 };
        }
    }
}
#[derive(QueryData)]
#[query_data(mutable)]
struct DrivetrainCar {
    tuning: Option<&'static DrivetrainSetup>,
    input: &'static DrivetrainInput,
    torque: &'static EngineTorque,
    duration: &'static VehicleStepDuration,
    brakes: &'static WheelBrakeDemand,
    reaction: &'static WheelReactionTorque,
    engine: &'static mut EngineTorqueInput,
    drive: &'static mut AxleDrive,
    state: &'static mut DrivetrainState,
    output: &'static mut DrivetrainOutput,
    wheels: Option<&'static mut WheelRotationState>,
}
fn advance(default_tuning: Res<DrivetrainTuning>, mut cars: Query<DrivetrainCar>) {
    for mut car in &mut cars {
        let tuning = car.tuning.map(|setup| &setup.0).unwrap_or(&default_tuning);
        let forces = DrivetrainForces { engine: *car.torque, brakes: *car.brakes, wheels: *car.reaction };
        *car.output =
            tuning.advance(*car.input, forces, car.duration.0, &mut car.engine, &mut car.drive, &mut car.state);
        if let Some(mut wheels) = car.wheels {
            for axle in 0..2 {
                if !car.input.independent_wheels[axle] {
                    wheels.previous_delta_mps[axle * 2 + 1] = car.state.previous_angular_delta[axle];
                }
            }
        }
    }
}
