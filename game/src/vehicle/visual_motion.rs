//! Body-origin conversion and retained chase-camera stabilization.
//! Numerical presentation rules only: no meshes, images or renderer required.
//! Source: 8001336C/800133F0 and 8003E978..8003EB1C; see
//! `docs/vehicle-presentation-evidence.md` for observed boundaries and limits.
use super::{
    VehiclePose, VehicleStepDuration, VehicleSystems, VehicleVelocity,
    direction::direction_turns,
    road_contact::{BodyOrientation, RoadContactState},
    slip::SlipTuning,
};
use crate::race::camera::CameraSubject;
use bevy::prelude::*;
use engine::{
    prelude::{EngineSet, TuningPlugin},
    shell::GameSystems,
};
use serde::{Deserialize, Serialize};

pub const VISUAL_TUNING: &str = "vehicle/visual.ron";

#[derive(Clone, Copy, Debug, Reflect, Serialize, Deserialize)]
pub enum ChaseHeading {
    Body,
    Velocity,
}

#[derive(Resource, Clone, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct VehicleVisualTuning {
    /// Model origin offset in body-local metres (outer+5A is local Z /4096).
    pub body_offset_m: Vec3,
    pub chase_heading: ChaseHeading,
    /// 8003C360: |x|+|z|-floor(min(|x|,|z|)/2), compared at 8003E9C8.
    pub velocity_heading_min_mps: f32,
    /// Table pair 80046C94/69C indexed by chase tracking option.
    pub yaw_stiffness: f32,
    pub yaw_damping: f32,
    /// 8003EAA4/EAB4, ordinary pitch follower gains.
    pub pitch_stiffness: f32,
    pub pitch_damping: f32,
}

#[derive(Component, Clone, Debug, Reflect)]
#[reflect(Component)]
pub struct VehicleVisualSetup(pub VehicleVisualTuning);

/// Retained angles/rates in turns, turns/s and turns/s². The original uses
/// /4096 signed halfwords at physics+748/74A/74C/74E/752, respectively.
#[derive(Component, Default, Clone, Copy, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
#[serde(default)]
pub struct ChaseStabilization {
    pub yaw_turns: f32,
    pub yaw_rate_turns_per_second: f32,
    pub pitch_turns: f32,
    pub pitch_rate_turns_per_second: f32,
    pub previous_yaw_acceleration: f32,
}

impl ChaseStabilization {
    pub fn at(pose: VehiclePose, orientation: BodyOrientation) -> Self {
        Self { yaw_turns: pose.heading_turns, pitch_turns: orientation.pitch_turns, ..default() }
    }

    pub fn advance(&mut self, target_yaw: f32, target_pitch: f32, seconds: f32, tuning: &VehicleVisualTuning) {
        // Yaw uses the mean of the old/new accelerations; pitch uses the
        // current spring force. Do not replace these with one generic lerp.
        let acceleration = grid(
            difference(target_yaw, self.yaw_turns) as f64 * tuning.yaw_stiffness as f64
                - self.yaw_rate_turns_per_second as f64 * tuning.yaw_damping as f64,
        );
        let mean = grid((self.previous_yaw_acceleration as f64 + acceleration as f64) / 2.0);
        self.yaw_rate_turns_per_second = halfword(self.yaw_rate_turns_per_second + grid(seconds as f64 * mean as f64));
        self.previous_yaw_acceleration = halfword(acceleration);
        self.yaw_turns =
            difference(halfword(self.yaw_turns + grid(seconds as f64 * self.yaw_rate_turns_per_second as f64)), 0.0);
        let pitch_force = grid(difference(target_pitch, self.pitch_turns) as f64 * tuning.pitch_stiffness as f64)
            - grid(self.pitch_rate_turns_per_second as f64 * tuning.pitch_damping as f64);
        self.pitch_rate_turns_per_second =
            halfword(self.pitch_rate_turns_per_second + grid(seconds as f64 * pitch_force as f64));
        self.pitch_turns = difference(
            halfword(self.pitch_turns + grid(seconds as f64 * self.pitch_rate_turns_per_second as f64)),
            0.0,
        );
    }

    pub fn correction(&self, pose: VehiclePose, orientation: BodyOrientation) -> CameraSubject {
        CameraSubject {
            yaw_correction_radians: difference(pose.heading_turns, self.yaw_turns) * std::f32::consts::TAU,
            pitch_correction_radians: difference(orientation.pitch_turns, self.pitch_turns) * std::f32::consts::TAU,
        }
    }
}

fn grid(value: f64) -> f32 {
    ((value * 4096.0).trunc() / 4096.0) as f32
}
fn halfword(value: f32) -> f32 {
    (value as f64 * 4096.0).round() as i64 as i16 as f32 / 4096.0
}
fn difference(target: f32, current: f32) -> f32 {
    (target - current + 0.5).rem_euclid(1.0) - 0.5
}

/// 8001336C converts body basis/position to rendering axes; 80013514..51C
/// adds the rotated model-origin offset. Quantized matrices become ordinary
/// unit rotations; physical state is never changed by presentation.
pub fn body_transform(pose: VehiclePose, road: &RoadContactState, tuning: &VehicleVisualTuning) -> Transform {
    let basis = &road.body_basis;
    let matrix = Mat3::from_cols(basis.lateral, basis.up, -basis.forward);
    Transform::from_translation(pose.position_m + matrix * tuning.body_offset_m)
        .with_rotation(Quat::from_mat3(&matrix).normalize())
}

pub struct VehicleVisualMotionPlugin;
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VehicleVisualSystems {
    Stabilize,
    BodyPose,
}
impl Plugin for VehicleVisualMotionPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<VehicleVisualTuning>() {
            app.add_plugins(TuningPlugin::<VehicleVisualTuning>::new(VISUAL_TUNING));
        }
        app.configure_sets(
            FixedUpdate,
            (VehicleVisualSystems::Stabilize, VehicleVisualSystems::BodyPose)
                .chain()
                .after(VehicleSystems::RefreshContact)
                .in_set(EngineSet::Objects)
                .in_set(GameSystems::Game),
        );
        app.add_systems(
            FixedUpdate,
            (stabilize.in_set(VehicleVisualSystems::Stabilize), sync_bodies.in_set(VehicleVisualSystems::BodyPose)),
        );
    }
}

type StabilizedCar = (
    &'static VehiclePose,
    &'static BodyOrientation,
    &'static VehicleStepDuration,
    &'static mut ChaseStabilization,
    Option<&'static VehicleVelocity>,
    Option<&'static VehicleVisualSetup>,
    Option<&'static mut CameraSubject>,
);
fn stabilize(tuning: Res<VehicleVisualTuning>, slip: Option<Res<SlipTuning>>, mut cars: Query<StabilizedCar>) {
    for (pose, orientation, seconds, mut state, velocity, setup, subject) in &mut cars {
        let tuning = setup.map_or(&*tuning, |s| &s.0);
        let mut target = pose.heading_turns;
        if let (ChaseHeading::Velocity, Some(velocity)) = (tuning.chase_heading, velocity) {
            let v = velocity.linear_mps.xz().abs();
            let speed = v.x + v.y - (v.min_element() * 4096.0 / 2.0).floor() / 4096.0;
            if speed >= tuning.velocity_heading_min_mps {
                let slip = slip.as_ref().expect("velocity chase tracking requires SlipTuning's shared direction curve");
                target = direction_turns(&slip.angle_threshold_ratios, -velocity.linear_mps.z, -velocity.linear_mps.x);
            }
        }
        state.advance(target, orientation.pitch_turns, seconds.0, tuning);
        if let Some(mut subject) = subject {
            *subject = state.correction(*pose, *orientation);
        }
    }
}
fn sync_bodies(
    tuning: Res<VehicleVisualTuning>,
    mut cars: Query<
        (&VehiclePose, &RoadContactState, &mut Transform, Option<&VehicleVisualSetup>),
        With<super::presentation::VehicleBody>,
    >,
) {
    for (pose, road, mut transform, setup) in &mut cars {
        *transform = body_transform(*pose, road, setup.map_or(&*tuning, |s| &s.0));
    }
}
