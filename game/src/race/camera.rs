//! Ordinary driving cameras from 0x800103C0: bumper and two chase views,
//! independent of force simulation, HUD and course rendering. Runs at the
//! original race cadence (one update per two NTSC fields). Intro/replay and
//! split-screen presentation are separate work. See docs/camera-evidence.md.
use bevy::prelude::*;
use engine::{
    camera::LensData,
    controller::{Button, ControllerState},
    prelude::{EngineSet, TuningPlugin},
};
use serde::{Deserialize, Serialize};

pub const CAMERA_SETTINGS: &str = "race/camera.ron";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum CameraMode {
    #[default]
    Bumper,
    ChaseNear,
    ChaseFar,
}

impl CameraMode {
    pub fn next(self) -> Self {
        match self {
            Self::Bumper => Self::ChaseNear,
            Self::ChaseNear => Self::ChaseFar,
            Self::ChaseFar => Self::Bumper,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReferenceSubject {
    pub position_m: [f32; 3],
    pub rotation_xyzw: [f32; 4],
    #[serde(default)]
    pub stabilization: CameraSubject,
}

/// Design values from 0x80010430 and tables 0x8002F350..36C. The reference
/// subject is temporary scene setup until the vehicle supplies its body pose;
/// a mod can omit it and use the authored course's start point instead.
#[derive(Resource, Clone, Debug, Serialize, Deserialize)]
pub struct CameraSettings {
    pub bumper_height_m: f32,
    /// Height above and distance behind the subject, near/far modes.
    pub chase_offsets_m: [[f32; 2]; 2],
    pub split_chase_offsets_m: [[f32; 2]; 2],
    pub split_pitch_radians: f32,
    pub lens: LensData,
    pub initial_view: CameraMode,
    pub reference_subject: Option<ReferenceSubject>,
}

/// Attach to any subject with a Transform, independent of the vehicle plugin.
/// The original uses the rendered body pose (outer car+0x81C) and two
/// stabilizing angles (outer car+0x766/+0x768) when rebuilding a chase view.
#[derive(Component, Default, Clone, Copy, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct CameraSubject {
    pub yaw_correction_radians: f32,
    pub pitch_correction_radians: f32,
}

#[derive(Component, Clone, Copy, Debug, Reflect)]
#[reflect(Component)]
pub struct VehicleCamera {
    pub subject: Entity,
    pub mode: CameraMode,
    pub split_screen: bool,
}

/// Effective controls at the camera boundary. Replay/AI systems can supply
/// these directly; PlayerCamera opts into the logical player pad.
#[derive(Component, Default, Clone, Copy, Debug, Reflect)]
#[reflect(Component)]
pub struct CameraInput {
    pub cycle_pressed: bool,
    pub rear_held: bool,
}

#[derive(Component)]
pub struct PlayerCamera;

pub struct CameraRulesPlugin;

impl Plugin for CameraRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<CameraSettings>() {
            app.add_plugins(TuningPlugin::<CameraSettings>::new(CAMERA_SETTINGS));
        }
        app.init_resource::<ControllerState>()
            .register_type::<CameraSubject>()
            .register_type::<VehicleCamera>()
            .register_type::<CameraInput>()
            .add_systems(
                FixedUpdate,
                (read_input, follow_subject).chain().in_set(EngineSet::Camera).in_set(engine::shell::GameSystems::Game),
            );
    }
}

fn read_input(pad: Res<ControllerState>, mut cameras: Query<&mut CameraInput, With<PlayerCamera>>) {
    for mut input in &mut cameras {
        input.cycle_pressed = pad.pressed(Button::R1);
        input.rear_held = pad.held(Button::L1);
    }
}

fn follow_subject(
    settings: Res<CameraSettings>,
    subjects: Query<(&Transform, &CameraSubject), Without<VehicleCamera>>,
    mut cameras: Query<(&mut Transform, &mut VehicleCamera, &mut CameraInput)>,
) {
    for (mut transform, mut camera, mut input) in &mut cameras {
        let Ok((subject, stabilization)) = subjects.get(camera.subject) else { continue };
        // 0x80010314..33C advances once on the effective press, wrapping at 3.
        if input.cycle_pressed {
            camera.mode = camera.mode.next();
        }
        *transform = camera.pose(subject, stabilization, input.rear_held, &settings);
        input.cycle_pressed = false;
    }
}

impl VehicleCamera {
    pub fn pose(
        &self,
        subject: &Transform,
        stabilization: &CameraSubject,
        rear: bool,
        settings: &CameraSettings,
    ) -> Transform {
        if self.mode == CameraMode::Bumper {
            // 0x80010424..440 translates in body-local up, retaining body roll.
            // Looking back rotates the orientation after that translation.
            let eye = subject.translation + subject.rotation * (Vec3::Y * settings.bumper_height_m);
            let rotation =
                if rear { subject.rotation * Quat::from_rotation_y(std::f32::consts::PI) } else { subject.rotation };
            return Transform::from_translation(eye).with_rotation(rotation);
        }
        // 0x80010478..4F4 rebuilds yaw/pitch, dropping body roll. The original
        // pitch angle has the opposite sign to Bevy's local X rotation.
        let (yaw, pitch, _) = subject.rotation.to_euler(EulerRot::YXZ);
        let mut yaw = yaw - stabilization.yaw_correction_radians;
        let mut pitch = pitch - stabilization.pitch_correction_radians;
        if rear {
            yaw += std::f32::consts::PI;
            pitch = -pitch;
        }
        if self.split_screen {
            pitch -= settings.split_pitch_radians;
        }
        let rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
        let index = usize::from(self.mode == CameraMode::ChaseFar);
        let [height, back] =
            if self.split_screen { settings.split_chase_offsets_m[index] } else { settings.chase_offsets_m[index] };
        Transform::from_translation(subject.translation + rotation * Vec3::new(0.0, height, back))
            .with_rotation(rotation)
    }
}
