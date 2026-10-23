//! Decoded body and procedural wheels under the rendered vehicle pose.
//! Optional live wheel steering, camber, spin and suspension bindings.
//! Reflections and the auxiliary/shadow pass remain unported.
//! See `docs/car-model-evidence.md` for scale, axes and selected paint evidence.
use bevy::prelude::*;
use engine::{geometry::GeometryData, prelude::load_ron, psx::PsxMaterial};
use serde::{Deserialize, Serialize};

use crate::race::camera::{CameraMode, VehicleCamera};

pub const BODY_ASSET: &str = "gt2/vehicle/t2vzn.ron";
pub const WHEEL_LAYOUT_ASSET: &str = "gt2/vehicle/t2vzn-wheels.ron";

/// Layered, authorable geometry asset. Insert before adding the plugin to
/// replace the default body; `None` disables body presentation. The asset is
/// loaded only when an entity opts in with [`VehicleBody`].
#[derive(Resource, Clone, Debug)]
pub struct VehiclePresentation {
    pub geometry: Option<String>,
    pub wheels: Option<String>,
}

impl Default for VehiclePresentation {
    fn default() -> Self {
        Self { geometry: Some(BODY_ASSET.into()), wheels: Some(WHEEL_LAYOUT_ASSET.into()) }
    }
}

/// Opt a subject into body presentation. Its Transform is the rendered body
/// pose in metres: local X right, Y up, -Z forward. No physics, joints or
/// animation clips are required. The geometry root becomes an ordinary child.
#[derive(Component, Default)]
pub struct VehicleBody;

#[derive(Component)]
struct BodyPresented;

/// Public handle to the subject owning a spawned body. A mod may query this
/// component to attach its own presentation; it does not identify physics state.
#[derive(Component)]
pub struct VehicleBodyRoot {
    pub subject: Entity,
}

/// Authorable wheel attachment transforms under the body. Original model
/// setup/initializer data is resolved offline; runtime only loads geometry,
/// metre translations and unit-quaternion rotations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WheelLayout {
    pub body_asset: String,
    pub wheels: Vec<WheelPlacement>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WheelPlacement {
    pub name: String,
    pub geometry: String,
    pub position_m: [f32; 3],
    pub rotation_xyzw: [f32; 4],
    #[serde(default)]
    pub motion: Option<WheelMotionBinding>,
}

/// Bind a canonical Z-axle wheel to one of the simulation's four wheels.
/// A custom mesh can supply an extra local rotation after the wheel motion.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WheelMotionBinding {
    pub wheel: usize,
    pub suspension_origin_y_m: f32,
    pub camber_turns: f32,
    pub mesh_rotation_xyzw: [f32; 4],
}

impl WheelPlacement {
    pub fn transform(&self) -> Transform {
        Transform::from_translation(Vec3::from_array(self.position_m))
            .with_rotation(Quat::from_array(self.rotation_xyzw))
    }

    /// 8001356C..588: local height = axle origin - suspension travel.
    /// 80014578..5A8 / 800671AC..1CC: steering yaw minus a quarter turn,
    /// opposite camber signs, then rolling spin. Right-side geometry already
    /// includes the original axial reflection; no negative scale is needed.
    pub fn animated_transform(
        &self,
        steering: &super::WheelSteering,
        contact: &super::contact_motion::ContactMotion,
        suspension: &super::suspension::Suspension,
    ) -> Transform {
        let Some(binding) = &self.motion else { return self.transform() };
        let i = binding.wheel;
        let mut transform = self.transform();
        transform.translation.y = binding.suspension_origin_y_m - suspension.travel_m[i];
        let turn = std::f32::consts::TAU;
        transform.rotation = Quat::from_euler(
            EulerRot::YXZ,
            (steering.angle_turns[i] - 0.25) * turn,
            binding.camber_turns * (if i % 2 == 0 { turn } else { -turn }),
            contact.spin_turns[i] * turn,
        ) * Quat::from_array(binding.mesh_rotation_xyzw);
        transform
    }
}

#[derive(Component)]
struct AnimatedWheel(WheelPlacement);

/// A public wheel attachment. Optional layout bindings follow simulation;
/// unbound wheels retain their authored static pose.
#[derive(Component, Clone, Copy, Debug, Reflect)]
#[reflect(Component)]
pub struct VehicleWheel {
    pub subject: Entity,
    pub index: usize,
}

/// Optional rendering only. It does not require the course, vehicle rules or
/// camera plugin. A matching ordinary driving camera hides its own body in
/// bumper mode, as observed by the absence of body reads in that original view.
pub struct VehiclePresentationPlugin;

impl Plugin for VehiclePresentationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VehiclePresentation>()
            .add_systems(Update, (dress_bodies, animate_wheels, body_view_visibility).chain());
    }
}

fn dress_bodies(
    mut commands: Commands,
    subjects: Query<Entity, (With<VehicleBody>, Without<BodyPresented>)>,
    presentation: Res<VehiclePresentation>,
    server: Option<Res<AssetServer>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<PsxMaterial>>>,
) {
    if subjects.is_empty() {
        return;
    }
    let Some(path) = &presentation.geometry else {
        for subject in &subjects {
            commands.entity(subject).insert(BodyPresented);
        }
        return;
    };
    let (Some(server), Some(mut meshes), Some(mut materials)) = (server, meshes, materials) else { return };
    let geometry: GeometryData = load_ron(path).unwrap_or_else(|error| panic!("vehicle geometry {path}: {error}"));
    let wheels = presentation.wheels.as_ref().map(|layout_path| {
        let layout: WheelLayout =
            load_ron(layout_path).unwrap_or_else(|error| panic!("wheel layout {layout_path}: {error}"));
        assert_eq!(&layout.body_asset, path, "wheel layout must name its body asset");
        layout
            .wheels
            .into_iter()
            .map(|wheel| {
                let rotation = Quat::from_array(wheel.rotation_xyzw);
                assert!(
                    Vec3::from_array(wheel.position_m).is_finite()
                        && rotation.is_finite()
                        && (rotation.length_squared() - 1.0).abs() < 0.001,
                    "wheel {} needs a finite position and unit rotation",
                    wheel.name
                );
                if let Some(binding) = &wheel.motion {
                    let mesh_rotation = Quat::from_array(binding.mesh_rotation_xyzw);
                    assert!(
                        binding.wheel < 4
                            && binding.suspension_origin_y_m.is_finite()
                            && binding.camber_turns.is_finite()
                            && mesh_rotation.is_finite()
                            && (mesh_rotation.length_squared() - 1.0).abs() < 0.001,
                        "wheel {} has an invalid simulation binding",
                        wheel.name
                    );
                }
                let geometry: GeometryData = load_ron(&wheel.geometry)
                    .unwrap_or_else(|error| panic!("wheel geometry {}: {error}", wheel.geometry));
                geometry.validate().unwrap_or_else(|error| panic!("wheel geometry {}: {error}", wheel.geometry));
                (wheel, geometry)
            })
            .collect::<Vec<_>>()
    });
    for subject in &subjects {
        let root = engine::geometry::spawn_geometry(&mut commands, &geometry, &mut meshes, &mut materials, &server)
            .expect("valid vehicle body geometry");
        // File positions already use metres and the subject's local axes.
        // Original car-transform-reads frame 2 confirms no corrective rotation.
        commands.entity(root).insert((Name::new("Vehicle body"), VehicleBodyRoot { subject }, ChildOf(subject)));
        if let Some(wheels) = &wheels {
            for (index, (wheel, geometry)) in wheels.iter().enumerate() {
                let wheel_root =
                    engine::geometry::spawn_geometry(&mut commands, geometry, &mut meshes, &mut materials, &server)
                        .expect("validated wheel geometry");
                commands.entity(wheel_root).insert((
                    Name::new(wheel.name.clone()),
                    VehicleWheel { subject, index },
                    wheel.transform(),
                    ChildOf(root),
                ));
                if wheel.motion.is_some() {
                    commands.entity(wheel_root).insert(AnimatedWheel(wheel.clone()));
                }
            }
        }
        commands.entity(subject).insert(BodyPresented);
    }
}

fn animate_wheels(
    subjects: Query<(&super::WheelSteering, &super::contact_motion::ContactMotion, &super::suspension::Suspension)>,
    mut wheels: Query<(&VehicleWheel, &AnimatedWheel, &mut Transform)>,
) {
    for (wheel, placement, mut transform) in &mut wheels {
        if let Ok((steering, contact, suspension)) = subjects.get(wheel.subject) {
            *transform = placement.0.animated_transform(steering, contact, suspension);
        }
    }
}

fn body_view_visibility(cameras: Query<&VehicleCamera>, mut bodies: Query<(&VehicleBodyRoot, &mut Visibility)>) {
    for (body, mut visibility) in &mut bodies {
        let bumper = cameras.iter().any(|camera| camera.subject == body.subject && camera.mode == CameraMode::Bumper);
        *visibility = if bumper { Visibility::Hidden } else { Visibility::Inherited };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(not(gt2_private_assets), ignore = "requires locally exported car body; see docs/assets.md")]
    fn baked_body_retains_high_lod_topology_and_metres() {
        let body: GeometryData = load_ron(BODY_ASSET).unwrap();
        body.validate().unwrap();
        assert_eq!(body.parts.len(), 13);
        assert_eq!(body.parts.iter().map(|part| part.triangles.len()).sum::<usize>(), 333);
        let mut minimum = Vec3::splat(f32::INFINITY);
        let mut maximum = Vec3::splat(f32::NEG_INFINITY);
        for part in &body.parts {
            assert!(!part.double_sided, "retain authored body face culling");
            if let Some(texture) = &part.texture {
                assert!(texture.contains("paint5"), "checkpoint paint slot 5");
                assert!(part.colors.iter().all(|color| color == &[1.0; 4]), "B-1 uses raw-texture commands");
            }
            for &position in &part.positions_m {
                minimum = minimum.min(Vec3::from_array(position));
                maximum = maximum.max(Vec3::from_array(position));
            }
        }
        let extent = maximum - minimum;
        assert!((extent - Vec3::new(1.910, 1.541, 3.611)).abs().max_element() < 0.001);
    }

    #[test]
    fn disabled_presentation_needs_no_original_assets_or_renderer() {
        let mut app = App::new();
        app.insert_resource(VehiclePresentation { geometry: None, wheels: None })
            .add_plugins(VehiclePresentationPlugin);
        let subject = app.world_mut().spawn((VehicleBody, Transform::default())).id();
        app.update();
        assert!(app.world().get::<BodyPresented>(subject).is_some());
    }

    #[test]
    #[cfg_attr(not(gt2_private_assets), ignore = "requires locally exported camera; see docs/assets.md")]
    fn file_local_axes_project_as_the_original_chase_body() {
        use crate::race::camera::{CAMERA_SETTINGS, CameraSettings};
        let settings: CameraSettings = load_ron(CAMERA_SETTINGS).unwrap();
        let reference = settings.reference_subject.as_ref().unwrap();
        let subject = Transform::from_translation(Vec3::from_array(reference.position_m))
            .with_rotation(Quat::from_array(reference.rotation_xyzw).normalize());
        let rig = VehicleCamera { subject: Entity::PLACEHOLDER, mode: CameraMode::ChaseNear, split_screen: false };
        let camera = rig.pose(&subject, &reference.stabilization, false, &settings);
        let view_from_body = camera.to_matrix().inverse() * subject.to_matrix();
        // Original body renderer's actual GTE loads, run frame 2 of
        // car-transform-reads, 0x800617EC..0x80061808. Its Y row contains
        // viewport inversion and 3723/4096 scale; its depth faces +Z.
        let recorded = Mat3::from_cols_array(&[4096., 0., 0., 0., -3723., -13., 0., 11., -4096.]) / 4096.0;
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            let native = view_from_body.transform_vector3(axis);
            let original_axes = Vec3::new(native.x, -native.y * 3723.0 / 4096.0, -native.z);
            assert!((original_axes - recorded * axis).abs().max_element() < 0.002);
        }
    }
}
