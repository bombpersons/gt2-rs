//! The B-1 licence course: the first GT2 course the port renders.
//!
//! Content is the baked engine-native course (`game/assets/gt2/course/
//! TC_lisence.ron`, written by `import tro` from `/crsobj/TC_lisence.tro`):
//! the road-node table, each node's road-surface quad and the draw spans. The
//! scene uses decoded road-node polygons and placed section-2 scenery with
//! imported textures. The camera starts at the recorded course distance
//! (190.2086 m, node 78, level 1), using the recorded body pose and lens.
//!
//! The course is exposed as [`RaceCourse`] so driving rules can read the same
//! data. Ordinary driving-camera rules are ported in [`camera`]; the subject
//! uses the retained native vehicle controller. Exported scene setup selects
//! the original countdown start; the moving checkpoint remains a reference.
//!
//! A mod can choose the procedural ribbon instead by setting
//! `RacePresentation::geometry` to `None`, with no original assets required.
//! Flat-textured road quads use recorded adaptive subdivision. Other primitive
//! refinement, procedural elements, backgrounds, dynamic LOD and intro/replay
//! cameras remain open; this is not yet a complete GT2 scene.
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    mesh::{Indices, PrimitiveTopology},
    pbr::{DistanceFog, FogFalloff},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use engine::prelude::*;
use engine::psx::{PsxMaterial, PsxView};
pub mod camera;
pub mod hud;
pub mod session;
use camera::{CameraInput, CameraSettings, CameraSubject, PlayerCamera, VehicleCamera};

/// The baked course this scene runs on.
pub const COURSE_ASSET: &str = "gt2/course/TC_lisence.ron";
pub const GEOMETRY_ASSET: &str = "gt2/course/TC_lisence/scene.ron";

/// A mod may supply a different scene or use the hand-authored road ribbon.
#[derive(Resource)]
pub struct RacePresentation {
    pub geometry: Option<String>,
}

impl Default for RacePresentation {
    fn default() -> Self {
        Self { geometry: Some(GEOMETRY_ASSET.into()) }
    }
}

/// The car's course distance at the B-1 start, metres (`car+0x604` = 190.2086
/// in `.psx/runs/b1-gas-1200`). The original's node is 78 and its level 1.
pub const START_DISTANCE_M: f32 = 190.2086;
pub const START_LEVEL: u16 = 1;

/// Detail level used to pick the draw span at the start (the original's
/// `car+0x608`).
pub fn start_span(course: &CourseData) -> Option<&RoadSpan> {
    course.span_at(START_DISTANCE_M, START_LEVEL)
}

/// The course, loaded once, for rules that need its road table.
#[derive(Resource)]
pub struct RaceCourse(pub CourseData);

/// Select a native vehicle state, or keep a course-only camera subject.
/// A mod can supply its own state through the same layered format.
#[derive(Resource, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct RaceVehicle {
    pub initial_state: Option<String>,
    /// Optional start-grid hold/clock; independent of vehicle tuning.
    pub session: Option<session::DrivingSession>,
}
/// Optional layered scene setup. Existing content without it keeps free driving.
pub const RACE_VEHICLE_ASSET: &str = "race/vehicle.ron";
impl Default for RaceVehicle {
    fn default() -> Self {
        Self { initial_state: Some(crate::vehicle::initial::INITIAL_STATE_ASSET.into()), session: None }
    }
}

/// The course asset/geometry, temporary background, camera subject and view.
pub struct RacePlugin;

impl Plugin for RacePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(engine::geometry::GeometryClippingPlugin);
        if !app.world().contains_resource::<CameraSettings>() {
            app.add_plugins(TuningPlugin::<CameraSettings>::new(camera::CAMERA_SETTINGS));
        }
        app.init_resource::<RacePresentation>();
        if !app.world().contains_resource::<RaceVehicle>() {
            let setup = if engine::config::asset_path(RACE_VEHICLE_ASSET).is_file() {
                load_ron::<RaceVehicle>(RACE_VEHICLE_ASSET)
                    .unwrap_or_else(|error| panic!("race vehicle setup {RACE_VEHICLE_ASSET}: {error}"))
            } else {
                RaceVehicle::default()
            };
            app.insert_resource(setup);
        }
        if !app.world().contains_resource::<session::DrivingSession>()
            && let Some(session) = app.world().resource::<RaceVehicle>().session
        {
            app.insert_resource(session);
        }
        if app.world().resource::<RaceVehicle>().initial_state.is_some()
            && !app.world().contains_resource::<crate::vehicle::visual_motion::VehicleVisualTuning>()
        {
            app.add_plugins(TuningPlugin::<crate::vehicle::visual_motion::VehicleVisualTuning>::new(
                crate::vehicle::visual_motion::VISUAL_TUNING,
            ));
        }
        app.add_systems(Startup, load_course).add_systems(Startup, (spawn_scene, spawn_camera).after(load_course));
    }
}

fn load_course(mut commands: Commands) {
    let course: CourseData = load_ron(COURSE_ASSET).unwrap_or_else(|error| panic!("course {COURSE_ASSET}: {error}"));
    let span = start_span(&course).expect("the B-1 start is inside a draw span");
    info!(
        "course: {:.1} m lap, {} nodes, {} spans; start node {}",
        course.lap_length_m,
        course.nodes.len(),
        course.spans.len(),
        span.node
    );
    commands.insert_resource(RaceCourse(course));
}

const SKY: Color = Color::srgb(0.55, 0.63, 0.74);
/// Metres per repeat of the asphalt texture along the road.
const TEXTURE_REPEAT_M: f32 = 8.0;

fn spawn_scene(
    mut commands: Commands,
    course: Res<RaceCourse>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PsxMaterial>>,
    mut images: ResMut<Assets<Image>>,
    server: Option<Res<AssetServer>>,
    presentation: Res<RacePresentation>,
) {
    if let Some(path) = &presentation.geometry {
        let geometry: engine::geometry::GeometryData =
            load_ron(path).unwrap_or_else(|error| panic!("course geometry {path}: {error}"));
        let root = engine::geometry::spawn_geometry(
            &mut commands,
            &geometry,
            &mut meshes,
            &mut materials,
            server.as_deref().expect("imported geometry requires AssetPlugin"),
        )
        .expect("validated course geometry");
        commands.entity(root).insert(Name::new("Road"));
    } else {
        let texture = images.add(asphalt());
        commands.spawn((
            Name::new("Road"),
            Mesh3d(meshes.add(road_mesh(&course.0))),
            MeshMaterial3d(materials.add(PsxMaterial { lit: false, ..PsxMaterial::textured(texture) })),
        ));
    }

    // A flat ground plane stands in for the not-yet-imported scenery. The
    // B-1 course is flat (node heights stay within 1.2 cm of zero).
    commands.spawn((
        Name::new("Ground"),
        Mesh3d(meshes.add(Plane3d::default().mesh().size(2000.0, 2000.0).subdivisions(64).build())),
        MeshMaterial3d(materials.add(PsxMaterial { lit: false, ..PsxMaterial::color(Color::srgb(0.16, 0.28, 0.12)) })),
        Transform::from_xyz(0.0, -0.05, 0.0),
    ));

    commands.spawn((
        DirectionalLight { illuminance: 1200.0, ..default() },
        Transform::from_xyz(1.0, 3.0, 2.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight { color: Color::WHITE, brightness: 400.0, ..default() });
}

fn spawn_camera(world: &mut World) {
    let course = world.resource::<RaceCourse>();
    let camera_settings = world.resource::<CameraSettings>().clone();
    let start = course.0.point_at(START_DISTANCE_M).expect("the course has a start point");
    let forward = Vec3::from_array(start.direction).normalize_or_zero();
    let mut subject_pose = camera_settings.reference_subject.as_ref().map_or_else(
        || Transform::from_translation(Vec3::from_array(start.position_m)).looking_to(forward, Vec3::Y),
        |reference| {
            Transform::from_translation(Vec3::from_array(reference.position_m))
                .with_rotation(Quat::from_array(reference.rotation_xyzw).normalize())
        },
    );
    let mut subject_state = camera_settings
        .reference_subject
        .as_ref()
        .map_or_else(CameraSubject::default, |reference| reference.stabilization);
    let state = world.resource::<RaceVehicle>().initial_state.as_ref().map(|path| {
        load_ron::<crate::vehicle::initial::VehicleInitialState>(path)
            .unwrap_or_else(|error| panic!("vehicle initial state {path}: {error}"))
    });
    let subject = if let Some(state) = state {
        let subject = state.spawn(world);
        if world.contains_resource::<session::DrivingSession>() {
            world.entity_mut(subject).insert(session::SessionParticipant);
        }
        subject_pose = crate::vehicle::visual_motion::body_transform(
            *world.get::<crate::vehicle::VehiclePose>(subject).unwrap(),
            world.get::<crate::vehicle::road_contact::RoadContactState>(subject).unwrap(),
            world.resource::<crate::vehicle::visual_motion::VehicleVisualTuning>(),
        );
        subject_state = world
            .get::<crate::vehicle::visual_motion::ChaseStabilization>(subject)
            .unwrap()
            .correction(state.pose, state.orientation);
        world.entity_mut(subject).insert(crate::vehicle::PlayerPedals);
        subject
    } else {
        world.spawn_empty().id()
    };
    world.entity_mut(subject).insert((
        Name::new("CameraSubject"),
        subject_pose,
        subject_state,
        crate::vehicle::presentation::VehicleBody,
    ));
    let rig = VehicleCamera { subject, mode: camera_settings.initial_view, split_screen: false };
    let camera_pose = rig.pose(&subject_pose, &subject_state, false, &camera_settings);
    world.spawn((
        Name::new("RaceCamera"),
        Camera3d::default(),
        Camera { order: 0, clear_color: ClearColorConfig::Custom(SKY), ..default() },
        PsxView,
        engine::geometry::GeometryClipCamera,
        camera_settings.lens.projection().expect("valid camera lens"),
        DistanceFog { color: SKY, falloff: FogFalloff::Linear { start: 60.0, end: 320.0 }, ..default() },
        camera_pose,
        rig,
        CameraInput::default(),
        PlayerCamera,
    ));
}

/// How long each rendered road piece is, metres. The original stores one quad
/// per node (about 50 m here); drawing each as a single polygon would run the
/// PS1's screen-space UV interpolation across tens of metres and smear the
/// edge lines near the camera. Subdividing is a renderer choice, not content:
/// positions and distances are interpolated linearly along the quad.
const SEGMENT_LENGTH_M: f32 = 2.0;

/// Builds the road ribbon from the course's surface quads, subdividing along
/// the course so texture coordinates interpolate over short spans. Four
/// vertices and six indices per piece, with the texture running across the
/// width and along the course distance.
pub fn road_mesh(course: &CourseData) -> Mesh {
    let quads = course.surface_quads();
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    let lerp = |a: [f32; 3], b: [f32; 3], t: f32| -> [f32; 3] {
        std::array::from_fn(|axis| a[axis] + (b[axis] - a[axis]) * t)
    };
    for quad in &quads {
        let [near, far] = quad.distance_m;
        let length = (far - near).max(0.0);
        let segments = ((length / SEGMENT_LENGTH_M).ceil() as usize).clamp(1, 64);
        let [a, b, c, d] = quad.corners;
        // `[0, 2, 1]` / `[1, 2, 3]` face up in the original's corner order;
        // reverse the winding if a hand-authored course orders them the other
        // way.
        let ab = Vec3::from_array(b) - Vec3::from_array(a);
        let ac = Vec3::from_array(c) - Vec3::from_array(a);
        let front = ac.cross(ab).y >= 0.0;
        for segment in 0..segments {
            let t0 = segment as f32 / segments as f32;
            let t1 = (segment + 1) as f32 / segments as f32;
            let v0 = (near + (far - near) * t0) / TEXTURE_REPEAT_M;
            let v1 = (near + (far - near) * t1) / TEXTURE_REPEAT_M;
            let base = positions.len() as u32;
            positions.extend([lerp(a, c, t0), lerp(b, d, t0), lerp(a, c, t1), lerp(b, d, t1)]);
            uvs.extend([[0.0, v0], [1.0, v0], [0.0, v1], [1.0, v1]]);
            if front {
                indices.extend([base, base + 2, base + 1, base + 1, base + 2, base + 3]);
            } else {
                indices.extend([base, base + 1, base + 2, base + 1, base + 3, base + 2]);
            }
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// Grey asphalt with white edge lines and a dashed centre, repeating every
/// [`TEXTURE_REPEAT_M`] metres along the road.
fn asphalt() -> Image {
    const SIZE: u32 = 128;
    let mut data = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let u = x as f32 / SIZE as f32;
            let v = y as f32 / SIZE as f32;
            // A little fixed variation so the flat colour does not band.
            let grain = ((x * 7 + y * 13) % 7) as i16 - 3;
            let mut colour = [100i16 + grain, 100 + grain, 96 + grain];
            if u < 0.02 || u > 0.98 {
                colour = [225, 225, 220];
            } else if (0.485..=0.515).contains(&u) && v < 0.45 {
                colour = [230, 230, 225];
            }
            data.extend_from_slice(&[
                colour[0].clamp(0, 255) as u8,
                colour[1].clamp(0, 255) as u8,
                colour[2].clamp(0, 255) as u8,
                255,
            ]);
        }
    }
    let mut image = Image::new(
        Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::nearest()
    });
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    fn course() -> CourseData {
        load_ron(COURSE_ASSET).unwrap()
    }

    #[test]
    #[cfg_attr(not(gt2_private_assets), ignore = "requires locally exported course; see docs/assets.md")]
    fn the_start_uses_the_recorded_span_and_node() {
        let course = course();
        assert_eq!(start_span(&course).map(|span| span.node), Some(78));
    }

    #[test]
    #[cfg_attr(not(gt2_private_assets), ignore = "requires locally exported course; see docs/assets.md")]
    fn the_ribbon_covers_every_surface_node() {
        let course = course();
        let mesh = road_mesh(&course);
        let vertices = mesh.count_vertices();
        assert!(vertices > 0);
        assert_eq!(vertices % 4, 0, "four vertices per road piece");
        assert_eq!(mesh.indices().unwrap().len(), vertices / 4 * 6);
    }
}
