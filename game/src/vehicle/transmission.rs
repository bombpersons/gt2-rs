//! Ordinary engine feedback and automatic/manual shifting.
//!
//! Preparation 8003A060, engine feedback 8003AC24, gear choice 800449C8
//! and application 8003991C. Alternate torque-controller producers remain
//! separate. Speeds use m/s and engine rev/s; cooldown is in physics ticks.
use super::{
    DigitalGearInput, PedalTravel, VehicleStepDuration, VehicleSystems,
    body_dynamics::BodyDynamicsInput,
    braking::BrakingSystems,
    drivetrain::{
        DriveLayout, DrivetrainInput, DrivetrainOutput, DrivetrainSetup, DrivetrainSystems, DrivetrainTuning,
        EngineEngagement,
    },
    engine_torque::{ENGINE_TORQUE_TUNING, EngineFuelCut, EngineTorqueInput, EngineTorqueTuning},
    forward_dynamics::{ForwardDynamics, ForwardDynamicsInput, ForwardDynamicsSystems},
    longitudinal_slip::{LongitudinalSlip, WheelSurfaceMotion},
    tyre_forces::TyreContactState,
    wheel_rotation::{AxleDrive, WheelRotationSystems},
};
use bevy::{ecs::query::QueryData, prelude::*};
use engine::{prelude::TuningPlugin, shell::GameSystems};
use serde::{Deserialize, Serialize};

pub const TRANSMISSION_TUNING: &str = "vehicle/transmission.ron";

#[derive(Resource, Debug, Clone, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct TransmissionTuning {
    /// Physics+372 also selects the special one-/two-ratio torque paths.
    pub forward_gears: usize,
    /// Physics+394/+396 and +398 onwards, in RPM.
    pub minimum_shift_rpm: f32,
    pub upshift_rpm: f32,
    /// Gear 2's lower boundary is element 0.
    pub downshift_rpm: Vec<f32>,
    /// Physics+3C4 indexed by gear, /4096; used for body-speed gear limits.
    pub engine_rev_s_per_mps: Vec<f32>,
    pub downshift_slip_limit: [f32; 2],
    pub upshift_slip_limit: [f32; 2],
    /// Source immediates: preparation's direction/speed/throttle gates.
    pub launch_speed_mps: f32,
    pub launch_throttle: f32,
    pub forward_select_speed_mps: f32,
    pub reverse_select_speed_mps: f32,
    pub forward_return_speed_mps: f32,
    pub full_throttle_threshold: f32,
    pub throttle_downshift_margin_rpm: f32,
    pub redline_speed_margin_rpm: f32,
    pub first_downshift_rpm: f32,
    pub top_upshift_rpm: f32,
    pub feedback_redline_margin_rpm: f32,
    pub braking_idle_margin_rpm: f32,
    /// Global 801C8570 /5, narrowed by the byte store at 8003AD88.
    pub reengagement_delay_ticks: u8,
}

#[derive(Component, Debug, Clone, Reflect, Deserialize, Serialize)]
#[reflect(Component)]
pub struct TransmissionSetup(pub TransmissionTuning);

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum ShiftMode {
    #[default]
    Automatic,
    Manual,
    Disabled,
}

#[derive(Component, Default, Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
#[serde(default)]
pub struct TransmissionInput {
    /// Physics+642, unless +786 forces the automatic path.
    pub mode: ShiftMode,
    pub force_automatic: bool,
    /// Source signed request bytes at scratch+364/+365. Physical bindings
    /// are outside this rules boundary; shift direction uses its sign.
    pub reverse_requested: bool,
    pub shift_direction: i8,
    /// Physics+610, distinct from the effective engine throttle (+708).
    pub accelerator_travel: f32,
    /// Before preparation: +6A4. Selection consumes the forward phase's
    /// newer value when a ForwardDynamics component is present.
    pub forward_mps: f32,
    pub wheel_load: [f32; 4],
    pub longitudinal_slip: [f32; 4],
    pub speed_response: [f32; 4],
}

#[derive(Component, Default, Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
#[serde(default)]
pub struct TransmissionState {
    /// Halfword +78E is decremented at 8003E168 before input and physics.
    /// While nonzero it forces first gear ahead of every selection mode.
    pub first_gear_ticks: u16,
    /// Physics+60A: request coupling to the gearbox; preparation can clear
    /// it. The engine continues running on the disconnected feedback path.
    pub engagement_requested: bool,
    /// Shared shift/reengagement byte +61C, preserved through all phases.
    pub cooldown_ticks: u8,
}

#[derive(Component, Default, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct TransmissionOutput {
    /// None is the source's -1 result, including blocked shift attempts.
    pub selected_gear: Option<usize>,
}

/// Immutable inputs sampled at the post-coupling feedback boundary.
pub struct EngineFeedbackSample<'a> {
    pub drivetrain: &'a DrivetrainTuning,
    pub engine_tuning: &'a EngineTorqueTuning,
    pub axles: AxleDrive,
    pub coupling: DrivetrainOutput,
    pub seconds: f32,
}

impl TransmissionTuning {
    pub fn prepare(
        &self,
        input: TransmissionInput,
        state: &mut TransmissionState,
        engine: EngineTorqueInput,
        drive: &mut DrivetrainInput,
    ) {
        if drive.engine_controls_rotation {
            return;
        }
        match drive.engagement {
            EngineEngagement::Disconnected if state.engagement_requested => {
                let moving_in_gear_direction = input.forward_mps > self.launch_speed_mps && drive.gear != 0
                    || input.forward_mps < -self.launch_speed_mps && drive.gear == 0;
                if moving_in_gear_direction {
                    if engine.effective_throttle > self.launch_throttle {
                        drive.engagement = EngineEngagement::FixedClutch;
                    }
                } else if engine.integer_rpm > self.minimum_shift_rpm {
                    drive.engagement = EngineEngagement::Synchronizing;
                } else {
                    state.engagement_requested = false;
                }
            }
            EngineEngagement::Coupled if !state.engagement_requested => {
                drive.engagement = EngineEngagement::Disconnected
            }
            _ => {}
        }
    }

    pub fn feedback(
        &self,
        inputs: EngineFeedbackSample<'_>,
        drive: &mut DrivetrainInput,
        state: &mut TransmissionState,
        engine: &mut EngineTorqueInput,
        cut: &mut EngineFuelCut,
    ) {
        let EngineFeedbackSample { drivetrain, engine_tuning, axles, coupling, seconds } = inputs;
        if !drive.engine_controls_rotation {
            if drive.engagement == EngineEngagement::Disconnected {
                // 8003AC60: this torque call occurs after coupling only when
                // the ordinary engine was disconnected. Do not call it twice.
                let torque = cut.advance(*engine, engine_tuning);
                let acceleration = (torque.net as f64 * drivetrain.engine_rotation_response as f64).trunc() / 4096.0;
                engine.speed_rev_s += sample(acceleration * seconds as f64);
                // +8E relative to the engine curve descriptor is +10A:
                // this free-engine path raises to idle, without a redline
                // ceiling. The connected path below has a separate ceiling.
                engine.speed_rev_s = engine.speed_rev_s.max(rpm_speed(engine_tuning.idle_rpm));
            } else {
                let geared = drivetrain.geared_engine_speed(drive.gear, axles);
                if drive.engagement == EngineEngagement::Coupled {
                    engine.speed_rev_s = if self.forward_gears == 2 {
                        floor_word_product(geared, drive.engine_inertia_fraction)
                    } else {
                        geared
                    };
                } else if matches!(drive.engagement, EngineEngagement::Synchronizing | EngineEngagement::FixedClutch) {
                    let [old_engine, old_geared] = coupling.clutch_speeds_rev_s;
                    // 8003AD14..54: equality in either old pair or new pair
                    // participates in crossing; equal old speeds always join.
                    let joined = old_geared >= old_engine && engine.speed_rev_s >= geared
                        || old_engine >= old_geared && engine.speed_rev_s <= geared;
                    if joined {
                        drive.engagement = EngineEngagement::Coupled;
                        engine.speed_rev_s = geared;
                        state.cooldown_ticks = self.reengagement_delay_ticks;
                    }
                    if drive.engagement == EngineEngagement::FixedClutch
                        && old_geared <= rpm_speed(engine_tuning.idle_rpm)
                    {
                        drive.engagement = EngineEngagement::Disconnected;
                    }
                    if drive.engagement == EngineEngagement::Synchronizing
                        && drive.brake_travel == 1.0
                        && engine.speed_rev_s < rpm_speed(engine_tuning.idle_rpm + self.braking_idle_margin_rpm)
                    {
                        drive.engagement = EngineEngagement::Disconnected;
                    }
                }
                engine.speed_rev_s = engine
                    .speed_rev_s
                    .clamp(0.0, rpm_speed(engine_tuning.redline_rpm + self.feedback_redline_margin_rpm));
            }
        }
        // 80039458 stores a signed halfword before gear choice reads it.
        engine.integer_rpm = stored_rpm(engine_tuning.refresh_rpm(engine.speed_rev_s));
    }

    fn speed_limit(&self, rpm: f32, gear: usize) -> f32 {
        sample(rpm_speed(rpm) as f64 / self.engine_rev_s_per_mps[gear] as f64)
    }

    /// 800448C8: choose a forward gear when leaving reverse manually.
    pub fn forward_gear_for_speed(&self, speed: f32) -> usize {
        if speed < 0.0 {
            return 0;
        }
        if self.forward_gears < 3 {
            return 1;
        }
        (1..self.forward_gears)
            .find(|&gear| speed < self.speed_limit(self.upshift_rpm + self.redline_speed_margin_rpm, gear))
            .unwrap_or(self.forward_gears)
    }

    pub fn select(
        &self,
        input: TransmissionInput,
        state: &mut TransmissionState,
        drive: DrivetrainInput,
        engine: EngineTorqueInput,
        redline_rpm: f32,
        layout: DriveLayout,
    ) -> Option<usize> {
        let gear = drive.gear;
        if state.first_gear_ticks != 0 {
            return (gear != 1).then_some(1);
        }
        let mode = if input.force_automatic { ShiftMode::Automatic } else { input.mode };
        match mode {
            ShiftMode::Disabled => None,
            ShiftMode::Manual => {
                if input.reverse_requested {
                    return (gear != 0).then_some(0);
                }
                if gear == 0 && input.accelerator_travel != 0.0 {
                    let wanted = self.forward_gear_for_speed(input.forward_mps);
                    // 80044DE8 substitutes first gear when the speed helper
                    // returns zero for negative body motion.
                    return Some(wanted.max(1));
                }
                if state.cooldown_ticks != 0 {
                    state.cooldown_ticks -= 1;
                    if state.cooldown_ticks == 0 && input.shift_direction != 0 {
                        state.cooldown_ticks = 1;
                    }
                    return None;
                }
                if input.shift_direction > 0 && gear < self.forward_gears {
                    state.cooldown_ticks = 1;
                    Some(gear + 1)
                } else if input.shift_direction < 0 && gear >= 2 {
                    state.cooldown_ticks = 1;
                    Some(gear - 1)
                } else {
                    None
                }
            }
            ShiftMode::Automatic => {
                if state.cooldown_ticks != 0 {
                    state.cooldown_ticks -= 1;
                    return None;
                }
                if input.reverse_requested {
                    return (gear != 0).then_some(0);
                }
                if input.forward_mps >= self.forward_return_speed_mps && gear == 0 {
                    return Some(1);
                }
                if engine.effective_throttle != 0.0 {
                    if input.forward_mps < self.forward_select_speed_mps {
                        return (gear != 1).then_some(1);
                    }
                } else if input.forward_mps < -self.reverse_select_speed_mps && gear != 0 {
                    return Some(0);
                }
                if gear > 0
                    && gear < self.forward_gears
                    && input.forward_mps > self.speed_limit(redline_rpm + self.redline_speed_margin_rpm, gear)
                {
                    return Some(gear + 1);
                }
                if drive.engagement != EngineEngagement::Coupled || gear == 0 {
                    return None;
                }
                let mut down = if gear == 1 { self.first_downshift_rpm } else { self.downshift_rpm[gear - 2] };
                let up = if gear < self.forward_gears { self.upshift_rpm } else { self.top_upshift_rpm };
                if gear != 1 && engine.effective_throttle > self.full_throttle_threshold {
                    down = (down - self.throttle_downshift_margin_rpm)
                        .max(stored_unsigned_rpm(self.minimum_shift_rpm + self.throttle_downshift_margin_rpm));
                }
                let candidate = if engine.integer_rpm < down {
                    gear - 1
                } else if engine.integer_rpm > up {
                    gear + 1
                } else {
                    return None;
                };
                let included = match layout {
                    DriveLayout::Rear | DriveLayout::Clutch { .. } | DriveLayout::AdaptiveClutch => [false, true],
                    DriveLayout::Front | DriveLayout::Viscous => [true, false],
                    _ => [true, true],
                };
                for wheel in 0..4 {
                    let axle = wheel / 2;
                    if !included[axle] {
                        continue;
                    }
                    if input.wheel_load[wheel] == 0.0 {
                        return None;
                    }
                    if candidate < gear {
                        if input.longitudinal_slip[wheel] > self.downshift_slip_limit[axle] {
                            return None;
                        }
                    } else {
                        let limit = stored_response(-floor_word_product(
                            input.speed_response[wheel],
                            -self.upshift_slip_limit[axle],
                        ));
                        if input.longitudinal_slip[wheel] < limit {
                            return None;
                        }
                    }
                }
                Some(candidate)
            }
        }
    }
}

fn rpm_speed(rpm: f32) -> f32 {
    ((rpm as f64 * 4096.0 / 60.0).trunc() / 4096.0) as f32
}
fn sample(value: f64) -> f32 {
    ((value * 4096.0).trunc() / 4096.0) as f32
}
fn stored_rpm(value: f32) -> f32 {
    ((value as f64 + 32768.0).rem_euclid(65536.0) - 32768.0) as f32
}
fn stored_unsigned_rpm(value: f32) -> f32 {
    (value as f64).rem_euclid(65536.0) as f32
}
fn stored_response(value: f32) -> f32 {
    (((value as f64 * 4096.0 + 32768.0).rem_euclid(65536.0) - 32768.0) / 4096.0) as f32
}
fn floor_word_product(a: f32, b: f32) -> f32 {
    // Low multiply word followed by arithmetic shift at 80044D68/3ACDC.
    let a = (a as f64 * 4096.0).round() as i64;
    let b = (b as f64 * 4096.0).round() as i64;
    (a.wrapping_mul(b) as i32 >> 12) as f32 / 4096.0
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransmissionSystems {
    Timers,
    DriverRequests,
    DriverEngagement,
    Prepare,
    Feedback,
    Select,
}

pub struct TransmissionRulesPlugin;
impl Plugin for TransmissionRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<TransmissionTuning>() {
            app.add_plugins(TuningPlugin::<TransmissionTuning>::new(TRANSMISSION_TUNING));
        }
        if !app.world().contains_resource::<EngineTorqueTuning>() {
            app.add_plugins(TuningPlugin::<EngineTorqueTuning>::new(ENGINE_TORQUE_TUNING));
        }
        if !app.world().contains_resource::<DrivetrainTuning>() {
            app.add_plugins(TuningPlugin::<DrivetrainTuning>::new(super::drivetrain::DRIVETRAIN_TUNING));
        }
        app.configure_sets(
            FixedUpdate,
            (VehicleSystems::Prepare, VehicleSystems::Forces, VehicleSystems::BuildStep)
                .chain()
                .in_set(engine::prelude::EngineSet::Control)
                .in_set(GameSystems::Game),
        )
        .configure_sets(
            FixedUpdate,
            TransmissionSystems::DriverRequests
                .after(TransmissionSystems::Timers)
                .after(VehicleSystems::ReadInput)
                .before(VehicleSystems::Pedals)
                .in_set(engine::prelude::EngineSet::Control)
                .in_set(GameSystems::Game),
        )
        .configure_sets(
            FixedUpdate,
            TransmissionSystems::DriverEngagement
                .after(TransmissionSystems::DriverRequests)
                .after(VehicleSystems::Pedals)
                .before(VehicleSystems::SteerWheels)
                .before(TransmissionSystems::Prepare)
                .in_set(engine::prelude::EngineSet::Control)
                .in_set(GameSystems::Game),
        )
        .configure_sets(
            FixedUpdate,
            TransmissionSystems::Prepare
                .in_set(VehicleSystems::Forces)
                .before(DrivetrainSystems::Prepare)
                .before(BrakingSystems::Allocate)
                .before(ForwardDynamicsSystems::Integrate),
        )
        .configure_sets(
            FixedUpdate,
            (TransmissionSystems::Feedback, TransmissionSystems::Select)
                .chain()
                .in_set(VehicleSystems::Forces)
                .after(WheelRotationSystems::Integrate),
        )
        .add_systems(FixedUpdate, tick_timers.in_set(TransmissionSystems::Timers).in_set(VehicleSystems::Prepare))
        .add_systems(FixedUpdate, driver_requests.in_set(TransmissionSystems::DriverRequests))
        .add_systems(FixedUpdate, driver_engagement.in_set(TransmissionSystems::DriverEngagement))
        .add_systems(FixedUpdate, prepare.in_set(TransmissionSystems::Prepare))
        .add_systems(FixedUpdate, feedback.in_set(TransmissionSystems::Feedback))
        .add_systems(FixedUpdate, select.in_set(TransmissionSystems::Select));
    }
}

fn driver_requests(mut cars: Query<(&DigitalGearInput, &TransmissionState, &mut TransmissionInput)>) {
    for (controls, state, mut input) in &mut cars {
        // 8002FFD8..FFEC and 80030158..174 use the already-decremented
        // first-gear counter. Suppression does not mutate caller-owned input.
        let enabled = state.first_gear_ticks == 0;
        input.reverse_requested = enabled && controls.reverse;
        input.shift_direction = if enabled { controls.shift_direction } else { 0 };
    }
}

#[derive(QueryData)]
#[query_data(mutable)]
struct DriverEngagementCar {
    pedals: &'static PedalTravel,
    state: &'static mut TransmissionState,
    input: &'static mut TransmissionInput,
    drive: &'static mut DrivetrainInput,
    engine: &'static EngineTorqueInput,
    body: Option<&'static BodyDynamicsInput>,
    setup: Option<&'static DrivetrainSetup>,
}
fn driver_engagement(
    engine: Res<EngineTorqueTuning>,
    drive: Res<DrivetrainTuning>,
    mut cars: Query<DriverEngagementCar, (With<DigitalGearInput>, Without<super::automatic_stop::ExternalDriver>)>,
) {
    for mut car in &mut cars {
        let setup = car.setup.map_or(&*drive, |s| &s.0);
        // Ordinary multi-ratio producer 8002F92C, after pedal ramping.
        // A reset/countdown or a full handbrake on the rear reference axle
        // disconnects. Otherwise a disconnected engine requests coupling
        // unless the service brake is fully down; an engaged engine stays
        // requested only while its last refreshed RPM is at least idle.
        let blocked = car.body.is_some_and(|b| b.reset_motion)
            || car.state.first_gear_ticks != 0
            || matches!(setup.reference_axle, super::drivetrain::Axle::Rear) && car.pedals.handbrake == 1.0;
        car.state.engagement_requested = !blocked
            && if car.drive.engagement == EngineEngagement::Disconnected {
                car.pedals.brake != 1.0
            } else {
                car.engine.integer_rpm >= engine.idle_rpm
            };
        car.input.accelerator_travel = car.pedals.throttle;
        car.drive.brake_travel = car.pedals.brake;
    }
}

fn tick_timers(mut cars: Query<&mut TransmissionState>) {
    for mut state in &mut cars {
        state.first_gear_ticks = state.first_gear_ticks.saturating_sub(1);
    }
}

#[derive(QueryData)]
#[query_data(mutable)]
struct PreparationCar {
    input: &'static TransmissionInput,
    state: &'static mut TransmissionState,
    engine: &'static EngineTorqueInput,
    drive: &'static mut DrivetrainInput,
    tuning: Option<&'static TransmissionSetup>,
    contact: Option<&'static TyreContactState>,
}
fn prepare(default_tuning: Res<TransmissionTuning>, mut cars: Query<PreparationCar>) {
    for mut car in &mut cars {
        let tuning = car.tuning.map(|v| &v.0).unwrap_or(&default_tuning);
        let mut input = *car.input;
        if let Some(contact) = car.contact {
            input.forward_mps = contact.body_forward_mps;
        }
        tuning.prepare(input, &mut car.state, *car.engine, &mut car.drive);
    }
}

#[derive(QueryData)]
#[query_data(mutable)]
struct FeedbackCar {
    tuning: Option<&'static TransmissionSetup>,
    drivetrain: Option<&'static DrivetrainSetup>,
    drive: &'static mut DrivetrainInput,
    state: &'static mut TransmissionState,
    engine: &'static mut EngineTorqueInput,
    cut: &'static mut EngineFuelCut,
    axles: &'static AxleDrive,
    coupling: &'static DrivetrainOutput,
    duration: &'static VehicleStepDuration,
}
fn feedback(
    default_tuning: Res<TransmissionTuning>,
    default_drive: Res<DrivetrainTuning>,
    engine_tuning: Res<EngineTorqueTuning>,
    mut cars: Query<FeedbackCar>,
) {
    for mut car in &mut cars {
        let tuning = car.tuning.map(|v| &v.0).unwrap_or(&default_tuning);
        let drivetrain = car.drivetrain.map(|v| &v.0).unwrap_or(&default_drive);
        tuning.feedback(
            EngineFeedbackSample {
                drivetrain,
                engine_tuning: &engine_tuning,
                axles: *car.axles,
                coupling: *car.coupling,
                seconds: car.duration.0,
            },
            &mut car.drive,
            &mut car.state,
            &mut car.engine,
            &mut car.cut,
        );
    }
}

#[derive(QueryData)]
#[query_data(mutable)]
struct SelectionCar {
    tuning: Option<&'static TransmissionSetup>,
    drivetrain: Option<&'static DrivetrainSetup>,
    input: &'static TransmissionInput,
    state: &'static mut TransmissionState,
    drive: &'static mut DrivetrainInput,
    engine: &'static EngineTorqueInput,
    output: &'static mut TransmissionOutput,
    forward: Option<&'static ForwardDynamics>,
    load: Option<&'static ForwardDynamicsInput>,
    slip: Option<&'static LongitudinalSlip>,
    motion: Option<&'static WheelSurfaceMotion>,
}
fn select(
    default_tuning: Res<TransmissionTuning>,
    default_drive: Res<DrivetrainTuning>,
    engine_tuning: Res<EngineTorqueTuning>,
    mut cars: Query<SelectionCar>,
) {
    for mut car in &mut cars {
        let tuning = car.tuning.map(|v| &v.0).unwrap_or(&default_tuning);
        let drivetrain = car.drivetrain.map(|v| &v.0).unwrap_or(&default_drive);
        let mut input = *car.input;
        if let Some(v) = car.forward {
            input.forward_mps = v.forward_mps;
        }
        if let Some(v) = car.load {
            input.wheel_load = v.wheel_load;
        }
        if let Some(v) = car.slip {
            input.longitudinal_slip = v.ratio;
        }
        if let Some(v) = car.motion {
            input.speed_response = v.speed_response;
        }
        let selected =
            tuning.select(input, &mut car.state, *car.drive, *car.engine, engine_tuning.redline_rpm, drivetrain.layout);
        car.output.selected_gear = selected;
        if let Some(gear) = selected {
            if car.drive.engagement != EngineEngagement::Disconnected {
                car.drive.engagement = if gear == 0 || car.drive.gear == 0 {
                    EngineEngagement::Synchronizing
                } else {
                    EngineEngagement::FixedClutch
                };
            }
            car.drive.gear = gear;
        }
    }
}
