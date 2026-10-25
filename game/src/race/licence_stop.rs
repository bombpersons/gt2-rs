//! Licence stopping-zone decisions, separate from awards and result flow.
//! Source: 8003D5F8, called after physics and the session clock at 8003ECB8.
use super::session::{DrivingSession, SessionSystems};
use crate::vehicle::{
    VehiclePose, VehicleVelocity,
    road_contact::{RoadContactCourse, RoadContactState},
};
use bevy::prelude::*;
use engine::prelude::*;
use serde::{Deserialize, Serialize};

pub const STOP_SETTINGS: &str = "race/licence-stop.ron";

#[derive(Resource, Clone, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct LicenceStopSettings {
    pub start_m: f64,
    pub end_m: f64,
    /// Strictly below this approximate world speed counts as stopped.
    pub stop_speed_mps: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum LicenceFailure {
    Overshot,
    OffRoad,
    Collision,
}

#[derive(Component, Default, Clone, Debug, PartialEq, Eq, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub enum LicenceRun {
    #[default]
    Active,
    Passed {
        time_ms: u32,
    },
    Failed {
        reason: LicenceFailure,
    },
}

/// Supply these ordinary inputs directly when running the rules alone.
#[derive(Component, Default, Clone, Debug, Reflect)]
#[reflect(Component)]
pub struct LicenceStopInput {
    /// Wheel projections, unwrapped near the lap origin (8003D498).
    pub wheel_distance_m: [f64; 4],
    pub velocity_mps: Vec3,
    pub surface: [u8; 4],
    pub offroad_fault: bool,
    /// Collision systems may supply this; barrier collision is not ported yet.
    pub collision_fault: bool,
    /// Milliseconds subtracted from the global clock (source physics +0x780).
    pub time_origin_ms: u32,
}

/// Opt into deriving stopping inputs from the native car and course.
#[derive(Component, Default, Reflect)]
#[reflect(Component)]
pub struct LicenceParticipant;

impl LicenceStopSettings {
    pub fn decide(&self, input: &LicenceStopInput, elapsed_ticks: u32, run: &mut LicenceRun) {
        if *run != LicenceRun::Active {
            return;
        }
        // Fault precedence and inclusive wheel-zone bounds: 8003D628..6AC.
        let failure = if input.offroad_fault {
            Some(LicenceFailure::OffRoad)
        } else if input.collision_fault {
            Some(LicenceFailure::Collision)
        } else if input.wheel_distance_m.into_iter().any(|d| d > self.end_m) {
            Some(LicenceFailure::Overshot)
        } else {
            None
        };
        if let Some(reason) = failure {
            *run = LicenceRun::Failed { reason };
            return;
        }
        if input.wheel_distance_m.into_iter().any(|d| d < self.start_m) {
            return;
        }
        // 8003C398: largest absolute axis plus a floored quarter of the other
        // two. This is an approximate speed, rather than a Euclidean norm.
        let speed = (crate::vehicle::approximate_magnitude(input.velocity_mps) * 4096.0).round() as u32;
        let threshold = (f64::from(self.stop_speed_mps) * 4096.0).round() as u32;
        if speed >= threshold {
            return;
        }
        if input.surface.into_iter().any(|s| s >= 2) {
            *run = LicenceRun::Failed { reason: LicenceFailure::OffRoad };
            return;
        }
        // 8003D718..794 interpolates within the 3 kHz clock tick, then floors
        // to milliseconds. Its division is by the same stop-speed threshold.
        let correction = u64::from(speed) * 100 / u64::from(threshold);
        let ms = (u64::from(elapsed_ticks) * 100 + correction) / 3;
        *run = LicenceRun::Passed { time_ms: (ms as u32).wrapping_sub(input.time_origin_ms) };
    }
}

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LicenceStopSystems {
    Decide,
}

pub struct LicenceStopPlugin;
impl Plugin for LicenceStopPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<LicenceStopSettings>()
            && engine::config::asset_path(STOP_SETTINGS).is_file()
        {
            app.add_plugins(TuningPlugin::<LicenceStopSettings>::new(STOP_SETTINGS));
        }
        app.register_type::<LicenceRun>()
            .register_type::<LicenceStopInput>()
            .register_type::<LicenceParticipant>()
            .configure_sets(
                FixedUpdate,
                LicenceStopSystems::Decide
                    .after(SessionSystems::Clock)
                    .in_set(EngineSet::Objects)
                    .in_set(GameSystems::Game),
            )
            .add_systems(FixedUpdate, (derive_vehicle_inputs, decide).chain().in_set(LicenceStopSystems::Decide));
    }
}

fn derive_vehicle_inputs(
    course: Option<Res<RoadContactCourse>>,
    mut cars: Query<
        (&VehiclePose, &VehicleVelocity, &RoadContactState, &mut LicenceStopInput),
        With<LicenceParticipant>,
    >,
) {
    let Some(course) = course else {
        return;
    };
    for (pose, velocity, road, mut input) in &mut cars {
        let mut distances = [0.0; 4];
        let mut complete = true;
        for (i, distance) in distances.iter_mut().enumerate() {
            let point =
                std::array::from_fn(|axis| f64::from(pose.position_m[axis]) + f64::from(road.attachment_m[i][axis]));
            if let Some(d) = course.0.distance_at_position(road.node, point) {
                // 8003D4F4..58C unwraps the final eighth of a lap behind zero.
                let wrap = ((course.0.lap_length_m * 65536.0 / 8.0).trunc() * 7.0) / 65536.0;
                *distance = if d > wrap { d - course.0.lap_length_m } else { d };
            } else {
                complete = false;
            }
        }
        if !complete {
            continue;
        }
        input.wheel_distance_m = distances;
        input.velocity_mps = velocity.linear_mps;
        input.surface = road.surface;
        // 800437E0..43888: all wheels off the road or missing disqualify.
        input.offroad_fault = road.offroad_wheels == 4 || road.missing_mask == 15;
    }
}

fn decide(
    settings: Option<Res<LicenceStopSettings>>,
    session: Option<Res<DrivingSession>>,
    mut cars: Query<(
        Entity,
        &LicenceStopInput,
        &mut LicenceRun,
        Option<&crate::vehicle::automatic_stop::AutomaticStopSetup>,
    )>,
    stop_tuning: Option<Res<crate::vehicle::automatic_stop::AutomaticStopTuning>>,
    mut commands: Commands,
) {
    let (Some(settings), Some(session)) = (settings, session) else {
        return;
    };
    for (entity, input, mut run, stop_setup) in &mut cars {
        let active = *run == LicenceRun::Active;
        settings.decide(input, session.elapsed_ticks, &mut run);
        if active && *run != LicenceRun::Active && (stop_tuning.is_some() || stop_setup.is_some()) {
            // Failed runs use the zero target directly (80037878). A valid
            // ordinary stop is below the AI's +/-2276 speed band, which also
            // selects zero (80037AB4..7B94), then shares the braking routine.
            commands.entity(entity).insert(crate::vehicle::automatic_stop::AutomaticStop::default());
        }
    }
}
