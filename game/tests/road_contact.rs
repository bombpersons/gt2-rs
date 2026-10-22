//! Decoded polygon queries through native road preparation and suspension.
//! Corrected body pose/angles between suspension and late frames are observed.
use bevy::prelude::*;
use engine::{
    course::CourseData,
    prelude::load_ron,
    testing::{TickApp, headless_app},
};
use game::vehicle::{
    VehiclePose, VehicleSystems, body_dynamics::BodyDynamicsInput, contact_motion::ContactMotionInput, road_contact::*,
    slip::WheelLocalVelocities, suspension::*, tyre_grip::TyreGripInput,
};
use std::collections::{HashMap, HashSet};
type Row = HashMap<String, i32>;
fn rows(name: &str) -> Vec<Row> {
    let text =
        std::fs::read_to_string(format!("{}/tests/data/original/road_contact/{name}.csv", env!("CARGO_MANIFEST_DIR")))
            .unwrap();
    let mut lines = text.lines();
    let columns: Vec<_> = lines.next().unwrap().split(',').collect();
    lines
        .map(|l| columns.iter().zip(l.split(',')).map(|(k, v)| (k.to_string(), v.parse().unwrap())).collect())
        .collect()
}
fn basis(r: &Row, phase: &str, kind: &str) -> VehicleBasis {
    let axis = |name: &str| {
        Vec3::new(
            r[&format!("{phase}_{kind}_{name}0")] as f32,
            r[&format!("{phase}_{kind}_{name}2")] as f32,
            -r[&format!("{phase}_{kind}_{name}1")] as f32,
        ) / 4096.0
    };
    VehicleBasis { forward: axis("forward"), lateral: axis("lateral"), up: axis("up") }
}
fn modified(course: &CourseData, r: &Row) -> CourseData {
    let mut result = course.clone();
    for (node, n) in result.nodes.iter_mut().enumerate() {
        let Some(mesh) = n.contact.as_mut() else {
            continue;
        };
        if r["empty_node"] == node as i32 {
            for cell in &mut mesh.cells {
                cell.clear();
            }
        }
        for polygon in &mut mesh.polygons {
            if r["surface_override"] >= 0 {
                polygon.surface = r["surface_override"] as u8;
                polygon.variant = r["variant_override"] as u8;
                polygon.flags = r["flags_override"] as u8;
            }
            if r["warp_node"] == node as i32 {
                for v in &mut polygon.vertices_m {
                    let x = (v[0] * 64.0).round() as i16;
                    let z = (v[2] * 64.0).round() as i16;
                    v[1] = (((x ^ z) & 127) - 63) as f64 / 64.0;
                }
            }
        }
    }
    result
}
#[derive(Component)]
struct CorrectedBody {
    pose: VehiclePose,
    orientation: BodyOrientation,
}
fn correct_body(mut cars: Query<(&CorrectedBody, &mut VehiclePose, &mut BodyOrientation)>) {
    for (boundary, mut pose, mut orientation) in &mut cars {
        *pose = boundary.pose;
        *orientation = boundary.orientation;
    }
}
fn pose(r: &Row, phase: &str) -> VehiclePose {
    let v = |key: &str| r[&format!("{phase}_{key}")] as f32 / 4096.0;
    VehiclePose { position_m: Vec3::new(v("x"), v("z"), -v("y")), heading_turns: v("heading") }
}
fn check(name: &str, count: usize) {
    let rows = rows(name);
    assert_eq!(rows.len(), count);
    let original: CourseData = load_ron(game::race::COURSE_ASSET).unwrap();
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.add_plugins((RoadContactRulesPlugin, SuspensionRulesPlugin));
    app.configure_sets(FixedUpdate, VehicleSystems::Suspension.before(VehicleSystems::RefreshContact));
    app.add_systems(
        FixedUpdate,
        correct_body.in_set(VehicleSystems::RefreshContact).before(RoadContactSystems::RefreshFrame),
    );
    let car = app
        .world_mut()
        .spawn((
            RoadContactState::default(),
            RoadContactInput::default(),
            BodyOrientation::default(),
            VehiclePose::default(),
            WheelLocalVelocities::default(),
            SuspensionInput::default(),
            Suspension::default(),
            TyreGripInput::default(),
            BodyDynamicsInput::default(),
            ContactMotionInput::default(),
        ))
        .id();
    let mut previous = None;
    let mut carried = 0;
    let mut missing = false;
    let mut neighbors = false;
    let mut surfaces = HashSet::new();
    let mut variants = HashSet::new();
    let mut offsets = HashSet::new();
    let mut slopes = HashSet::new();
    for r in &rows {
        let label = format!("{name} call {}", r["case"]);
        let v = |phase: &str, key: &str| r[&format!("{phase}_{key}")] as f32;
        let four = |phase: &str, key: &str, scale: f32| std::array::from_fn(|i| v(phase, &format!("{key}{i}")) / scale);
        let course = modified(&original, r);
        for i in 0..4 {
            let query = [
                r[&format!("query{i}_x")] as f64,
                r[&format!("query{i}_height")] as f64,
                -r[&format!("query{i}_y")] as f64,
            ]
            .map(|v| v / 65536.0);
            let result = course.road_contact(r[&format!("query{i}_seed")] as u16, query);
            let height = r[&format!("query{i}_result_height")];
            assert_eq!(
                course.node_at_position(r[&format!("query{i}_seed")] as u16, query),
                Some(r[&format!("query{i}_node")] as u16),
                "{label} node walk {i}"
            );
            if height == i32::MAX {
                assert!(result.is_none(), "{label}: expected missing polygon {i}: {result:?}");
                missing = true;
            } else {
                let result =
                    result.unwrap_or_else(|| panic!("{label}: missing native polygon wheel {i} query {query:?}"));
                assert_eq!(result.height_m * 65536.0, height as f64, "{label}: polygon height wheel {i}");
                let tags = r[&format!("query{i}_tags")];
                assert_eq!(
                    (result.surface, result.variant, result.flags),
                    (tags as u8, (tags >> 8) as u8, (tags >> 16) as u8),
                    "{label}: polygon metadata wheel {i}"
                );
                assert_eq!(
                    result.polygon_node,
                    r[&format!("query{i}_polygon_node")] as u16,
                    "{label}: selected polygon node {i}"
                );
                assert_eq!(
                    result.polygon,
                    r[&format!("query{i}_polygon")] as usize,
                    "{label}: selected polygon identity {i}"
                );
                neighbors |= result.node != result.polygon_node;
                surfaces.insert(result.surface);
                variants.insert(result.variant);
            }
        }
        let mut state = RoadContactState {
            node: v("before", "node") as u16,
            raw_height_m: std::array::from_fn(|i| r[&format!("before_raw_height{i}")] as f64 / 65536.0),
            height_m: four("before", "height", 4096.0),
            surface: four("before", "surface", 1.0).map(|v| v as u8),
            variant: four("before", "variant", 1.0).map(|v| v as u8),
            body_basis: basis(r, "before", "body"),
            road_basis: basis(r, "before", "road"),
            ..default()
        };
        let mut suspension = Suspension {
            travel_m: four("suspension_before", "travel", 4096.0),
            travel_rate_mps: four("suspension_before", "rate", 4096.0),
            ..default()
        };
        if previous == Some(r["sequence"]) {
            let old = *app.world().get::<RoadContactState>(car).unwrap();
            assert_eq!(old.node, state.node, "{label}: node seed carry");
            assert_eq!(old.raw_height_m, state.raw_height_m, "{label}: raw height carry");
            assert_eq!(old.height_m, state.height_m, "{label}: smoothed height carry");
            for (actual, expected) in [(old.body_basis, state.body_basis), (old.road_basis, state.road_basis)] {
                assert_eq!(actual.forward, expected.forward, "{label}: basis forward carry");
                assert_eq!(actual.lateral, expected.lateral, "{label}: basis lateral carry");
                assert_eq!(actual.up, expected.up, "{label}: basis up carry");
            }
            state = old;
            let old = *app.world().get::<Suspension>(car).unwrap();
            assert_eq!(old.travel_m, suspension.travel_m, "{label}: suspension travel carry");
            assert_eq!(old.travel_rate_mps, suspension.travel_rate_mps, "{label}: suspension rate carry");
            suspension = old;
            carried += 1;
        }
        app.insert_resource(RoadContactCourse(course));
        app.world_mut().entity_mut(car).insert((
            state,
            suspension,
            pose(r, "before"),
            BodyOrientation { pitch_turns: v("before", "pitch") / 4096.0, roll_turns: v("before", "roll") / 4096.0 },
            WheelLocalVelocities {
                forward_mps: four("before", "forward", 4096.0),
                lateral_mps: four("before", "lateral", 4096.0),
            },
            SuspensionInput {
                inverse_step_seconds: v("suspension_before", "inverse_duration") / 4096.0,
                axle_force: [v("suspension_before", "front_force"), v("suspension_before", "rear_force")],
                lateral_force: v("suspension_before", "lateral_force"),
                vertical_force: v("suspension_before", "vertical_force"),
                forward_force: v("suspension_before", "forward_force"),
                ..default()
            },
            CorrectedBody {
                pose: pose(r, "frame_before"),
                orientation: BodyOrientation {
                    pitch_turns: v("frame_before", "pitch") / 4096.0,
                    roll_turns: v("frame_before", "roll") / 4096.0,
                },
            },
        ));
        app.run_tick(&[]);
        let state = app.world().get::<RoadContactState>(car).unwrap();
        assert_eq!(state.node, v("after", "node") as u16, "{label} car node");
        assert_eq!(
            state.raw_height_m,
            std::array::from_fn(|i| r[&format!("after_raw_height{i}")] as f64 / 65536.0),
            "{label}: raw height"
        );
        assert_eq!(state.height_m, four("after", "height", 4096.0), "{label}: smoothed height");
        assert_eq!(state.surface, four("after", "surface", 1.0).map(|v| v as u8), "{label}: surface");
        assert_eq!(state.variant, four("after", "variant", 1.0).map(|v| v as u8), "{label}: variant");
        assert_eq!(state.bump_response, four("after", "bump_response", 4096.0), "{label}: bump response");
        assert_eq!(state.road_offset_m, four("after", "offset", 4096.0), "{label}: procedural road offset");
        assert_eq!(state.bypass_condition, (r["after_condition_flags"] & 16) != 0, "{label}: condition bypass");
        for i in 0..4 {
            let attachment = Vec3::new(
                v("queried", &format!("attachment{i}_0")),
                v("queried", &format!("attachment{i}_2")),
                -v("queried", &format!("attachment{i}_1")),
            ) / 4096.0;
            assert_eq!(state.attachment_m[i], attachment, "{label}: wheel attachment {i}");
            assert_eq!(
                state.query_m[i],
                [
                    r[&format!("query{i}_x")] as f64,
                    r[&format!("query{i}_height")] as f64,
                    -r[&format!("query{i}_y")] as f64
                ]
                .map(|v| v / 65536.0),
                "{label}: query position {i}"
            );
        }
        for (actual, key) in [
            (state.road_pitch_turns, "road_pitch"),
            (state.road_roll_turns, "road_roll"),
            (state.road_height_m, "road_height"),
        ] {
            assert_eq!(actual * 4096.0, v("frame_after", key), "{label}: {key}");
        }
        for (actual, expected) in
            [(state.body_basis, basis(r, "frame_after", "body")), (state.road_basis, basis(r, "frame_after", "road"))]
        {
            assert_eq!(actual.forward, expected.forward, "{label}: forward basis");
            assert_eq!(actual.lateral, expected.lateral, "{label}: lateral basis");
            assert_eq!(actual.up, expected.up, "{label}: up basis");
        }
        let suspension = app.world().get::<Suspension>(car).unwrap();
        for (key, values, scale) in [
            ("travel", suspension.travel_m.map(f64::from), 4096.0),
            ("rate", suspension.travel_rate_mps.map(f64::from), 4096.0),
            ("spring_force", suspension.spring_force, 1.0),
            ("damper_force", suspension.damper_force, 1.0),
            ("load", suspension.tyre_load, 1.0),
        ] {
            for i in 0..4 {
                assert_eq!(
                    values[i] * scale,
                    r[&format!("suspension_after_{key}{i}")] as f64,
                    "{label}: native road -> suspension {key} {i}"
                );
            }
        }
        assert_eq!(suspension.compressed_mask, r["suspension_after_compressed_mask"] as u8);
        let grip = app.world().get::<TyreGripInput>(car).unwrap();
        assert_eq!(grip.surface, state.surface.map(usize::from));
        assert_eq!(grip.load, suspension.tyre_load.map(|v| v as f32));
        let motion = app.world().get::<ContactMotionInput>(car).unwrap();
        assert_eq!(motion.forward_axis, state.road_basis.forward);
        assert_eq!(motion.lateral_axis, state.road_basis.lateral);
        let body = app.world().get::<BodyDynamicsInput>(car).unwrap();
        assert_eq!(body.forward_axis, state.body_basis.forward.xz());
        offsets.extend(state.road_offset_m.map(|v| (v * 4096.0) as i32));
        slopes.insert((state.road_pitch_turns * 4096.0) as i32);
        previous = Some(r["sequence"]);
    }
    assert!(carried >= 39);
    assert!(offsets.len() > 2 && slopes.len() > 2);
    if name == "boundaries" {
        assert!(missing && neighbors);
        assert_eq!(surfaces.len(), 8);
        assert_eq!(variants.len(), 4);
    }
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original road-contact fixtures; see docs/assets.md")]
fn original_polygon_sampling_road_preparation_frames_and_native_suspension() {
    check("boundaries", 204);
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires private original road-contact holdout; see docs/assets.md")]
fn independent_original_road_contact_holdout() {
    check("holdout", 53);
}

#[test]
fn handmade_road_plugin_runs_alone_with_authored_geometry_and_per_car_setup() {
    use engine::course::{ContactMesh, ContactPolygon, CourseNode};
    let polygon = ContactPolygon {
        vertices_m: vec![[-4.0, 0.25, -4.0], [-4.0, 0.25, 4.0], [4.0, 0.25, 4.0], [4.0, 0.25, -4.0]],
        surface: 6,
        variant: 3,
        flags: 2,
    };
    let mesh = ContactMesh {
        origin_m: [0.0; 3],
        grid_origin_m: [-4.0; 2],
        cell_size_m: [8.0; 2],
        dimensions: [1, 1],
        cells: vec![vec![0]],
        polygons: vec![polygon],
        horizontal_step_m: 1.0 / 64.0,
        height_step_m: 1.0 / 65536.0,
    };
    mesh.validate().unwrap();
    let course = CourseData {
        lap_length_m: 20.0,
        nodes: vec![
            CourseNode { next: 1, previous: 1, direction: [0.0, 0.0, -1.0], contact: Some(mesh), ..default() },
            CourseNode { next: 0, previous: 0, direction: [0.0, 0.0, 1.0], position_m: [0.0, 0.0, -10.0], ..default() },
        ],
        ..default()
    };
    let tuning = RoadContactTuning {
        wheel_levers_m: [[-0.5, 1.0], [0.5, 1.0], [-0.5, -1.0], [0.5, -1.0]],
        wheelbase_m: 2.0,
        track_m: 1.0,
        rear_height_fraction: 0.5,
        bumps: vec![SurfaceBump { amplitude_m: 0.0, cycles_per_m: 1.0, scale_with_speed: false }; 8],
        bump_response_per_mps: 0.25,
        angle_threshold_ratios: vec![0.5],
    };
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(RoadContactCourse(course)).insert_resource(tuning.clone()).add_plugins(RoadContactRulesPlugin);
    let car = app
        .world_mut()
        .spawn((
            VehiclePose { position_m: Vec3::new(0.0, 1.0, -2.0), heading_turns: 0.0 },
            BodyOrientation::default(),
            RoadContactInput { condition_surfaces_enabled: true },
            WheelLocalVelocities::default(),
            RoadContactState {
                body_basis: vehicle_basis(0.0, 0.0, 0.0),
                road_basis: vehicle_basis(0.0, 0.0, 0.0),
                raw_height_m: [0.25; 4],
                ..default()
            },
            SuspensionInput::default(),
            TyreGripInput::default(),
            BodyDynamicsInput::default(),
            ContactMotionInput::default(),
        ))
        .id();
    let mut variant = tuning;
    variant.wheel_levers_m[0] = [-1.0, 1.5];
    let other = app
        .world_mut()
        .spawn((
            VehiclePose { position_m: Vec3::new(0.0, 1.0, -2.0), heading_turns: 0.0 },
            BodyOrientation::default(),
            RoadContactInput::default(),
            WheelLocalVelocities::default(),
            RoadContactState {
                body_basis: vehicle_basis(0.0, 0.0, 0.0),
                road_basis: vehicle_basis(0.0, 0.0, 0.0),
                ..default()
            },
            RoadContactSetup(variant),
        ))
        .id();
    app.run_tick(&[]);
    let state = *app.world().get::<RoadContactState>(car).unwrap();
    assert_eq!(state.height_m, [0.25; 4]);
    assert_eq!(state.surface, [5; 4]);
    assert_eq!(state.variant, [3; 4]);
    assert_eq!(state.reset_surface_wheels, 4);
    assert_eq!(state.special_wheels, 4);
    assert!(state.bypass_condition);
    assert_eq!(state.road_height_m, 0.25);
    assert_eq!(state.attachment_m[0], Vec3::new(-0.5, 0.0, -1.0));
    assert_eq!(app.world().get::<RoadContactState>(other).unwrap().attachment_m[0], Vec3::new(-1.0, 0.0, -1.5));
    assert!(!app.world().get::<RoadContactState>(other).unwrap().bypass_condition);
    assert_eq!(app.world().get::<SuspensionInput>(car).unwrap().road_height_m, [0.25; 4]);
    assert_eq!(app.world().get::<TyreGripInput>(car).unwrap().surface, [5; 4]);
    assert_eq!(app.world().get::<ContactMotionInput>(car).unwrap().forward_axis, Vec3::NEG_Z);
    for polygon in
        &mut app.world_mut().resource_mut::<RoadContactCourse>().0.nodes[0].contact.as_mut().unwrap().polygons
    {
        for vertex in &mut polygon.vertices_m {
            vertex[1] = 0.5;
        }
    }
    app.run_tick(&[]);
    let state = app.world().get::<RoadContactState>(car).unwrap();
    assert_eq!(state.raw_height_m, [0.5; 4]);
    assert_eq!(state.height_m, [0.375; 4]);
    assert_eq!(state.road_height_m, 0.375);
}
