//! Recorded camera inputs/outputs at the original driving-camera boundary.
//! Pad transport latency, tyre simulation and vehicle body animation are
//! outside this test: each tick supplies the observed body pose and controls.
use bevy::prelude::*;
use engine::{
    camera::LensData,
    prelude::load_ron,
    testing::{TickApp, headless_app},
};
use game::race::camera::{CameraInput, CameraMode, CameraRulesPlugin, CameraSettings, CameraSubject, VehicleCamera};
use serde::Deserialize;

#[derive(Deserialize)]
struct Row {
    run_frame: usize,
    mode: usize,
    cycle: bool,
    rear: bool,
    body_matrix: [i16; 9],
    body_position: [i32; 3],
    yaw_correction: i16,
    pitch_correction: i16,
    matrix: [i16; 9],
    position: [i32; 3],
}

#[derive(Deserialize)]
struct Recording {
    rows: Vec<Row>,
}

fn rotation(matrix: [i16; 9]) -> Mat3 {
    Mat3::from_cols_array(&matrix.map(|v| f32::from(v) / 4096.0)).transpose()
}

fn check(name: &str) {
    let recording: Recording = serde_json::from_str(
        &std::fs::read_to_string(format!("{}/tests/data/original/camera/{name}.json", env!("CARGO_MANIFEST_DIR")))
            .unwrap(),
    )
    .unwrap();
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.add_plugins(CameraRulesPlugin);
    let subject = app.world_mut().spawn((Transform::default(), CameraSubject::default())).id();
    let camera = app
        .world_mut()
        .spawn((
            Transform::default(),
            CameraInput::default(),
            VehicleCamera { subject, mode: CameraMode::Bumper, split_screen: false },
        ))
        .id();
    assert_eq!(recording.rows.len(), 48);
    let (mut max_position, mut max_rotation) = (0.0f32, 0.0f32);
    for (tick, row) in recording.rows.iter().enumerate() {
        assert_eq!(row.run_frame, tick * 2, "original camera updates every two fields");
        app.world_mut().entity_mut(subject).insert((
            Transform::from_translation(Vec3::from_array(row.body_position.map(|v| v as f32 / 65536.0)))
                .with_rotation(Quat::from_mat3(&rotation(row.body_matrix)).normalize()),
            CameraSubject {
                yaw_correction_radians: f32::from(row.yaw_correction) * std::f32::consts::TAU / 4096.0,
                pitch_correction_radians: f32::from(row.pitch_correction) * std::f32::consts::TAU / 4096.0,
            },
        ));
        app.world_mut().entity_mut(camera).insert(CameraInput { cycle_pressed: row.cycle, rear_held: row.rear });
        app.run_tick(&[]);
        let actual_mode = app.world().get::<VehicleCamera>(camera).unwrap().mode;
        assert_eq!(
            actual_mode,
            [CameraMode::Bumper, CameraMode::ChaseNear, CameraMode::ChaseFar][row.mode],
            "{name} tick {tick}"
        );
        let actual = app.world().get::<Transform>(camera).unwrap();
        let original = Vec3::from_array(row.position.map(|v| v as f32 / 65536.0));
        max_position = max_position.max(actual.translation.distance(original));
        // Modern unit rotations replace the source's quantized Euler lookup
        // and GTE matrices. Bound the resulting pose error, not fitted offsets.
        assert!(
            actual.translation.distance(original) < 0.025,
            "{name} tick {tick} position {:?}, expected {original:?}",
            actual.translation
        );
        let matrix = Mat3::from_quat(actual.rotation);
        for (got, expected) in matrix.to_cols_array().into_iter().zip(rotation(row.matrix).to_cols_array()) {
            max_rotation = max_rotation.max((got - expected).abs());
            assert!((got - expected).abs() < 0.002, "{name} tick {tick} orientation {got} != {expected}");
        }
    }
    println!(
        "{name}: {} camera ticks; max position error {max_position:.6} m, matrix coefficient error {max_rotation:.6}",
        recording.rows.len()
    );
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn bumper_camera_matches_original_coasting_body_motion() {
    check("coast");
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn all_three_views_and_rear_look_match_the_original() {
    check("views");
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn chase_camera_matches_turning_and_stabilization() {
    check("turn");
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn independent_camera_holdout_matches_original_view_changes_and_turns() {
    check("holdout");
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn the_reference_lens_matches_the_original_projection_scaling() {
    let settings: CameraSettings = load_ron(game::race::camera::CAMERA_SETTINGS).unwrap();
    let lens: LensData = settings.lens;
    let projection = lens.projection().unwrap();
    // 0x8007B374: 320/320 X scale, floor(240*4096/264) Y scale;
    // H=216, centre=(160,120). Check a source-independent view-space point.
    let clip = projection.get_clip_from_view() * Vec4::new(2.0, 3.0, -10.0, 1.0);
    let pixel = Vec2::new((clip.x / clip.w + 1.0) * 160.0, (1.0 - clip.y / clip.w) * 120.0);
    let original = Vec2::new(160.0 + 216.0 * 2.0 / 10.0, 120.0 - 216.0 * 3723.0 / 4096.0 * 3.0 / 10.0);
    assert!(pixel.distance(original) < 0.0001);
}
