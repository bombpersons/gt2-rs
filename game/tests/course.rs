//! The B-1 course, headless: the baked engine-native data, the road ribbon
//! built from its surface quads, and the recorded start.
use bevy::prelude::*;
use engine::prelude::*;
use game::race::{COURSE_ASSET, START_DISTANCE_M, START_LEVEL, road_mesh, start_span};

#[test]
#[cfg_attr(
    not(gt2_private_assets),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn baked_scene_contains_the_original_road_and_all_thirteen_scenery_instances() {
    let geometry: engine::geometry::GeometryData = load_ron(game::race::GEOMETRY_ASSET).unwrap();
    geometry.validate().unwrap();
    assert_eq!(
        geometry.parts.iter().map(|p| p.quads.len()).sum::<usize>(),
        2907,
        "all source flat-textured road quads retain their topology"
    );
    assert!(geometry.parts.iter().any(|p| p.subdivision.is_some()), "the running asset enables refinement");
    assert_eq!(
        geometry
            .parts
            .iter()
            .filter(|p| p.small_texture.is_some())
            .map(|p| p.triangles.len() + p.quads.len())
            .sum::<usize>(),
        3295,
        "every textured road parent retains both material variants"
    );
    let names: std::collections::HashSet<_> = geometry.parts.iter().map(|p| p.name.as_str()).collect();
    // The other 72 source nodes have no ordinary polygons. The static scene
    // emits exactly these 52 nonempty node meshes; no empty placeholders.
    let source_nodes: Vec<_> = [0, 1].into_iter().chain(4..16).chain(60..98).collect();
    assert_eq!(names.iter().filter(|name| name.starts_with("Course node ")).count(), source_nodes.len());
    for node in source_nodes {
        assert!(names.contains(format!("Course node {node}").as_str()));
    }
    for instance in 0..13 {
        assert!(names.contains(format!("Scenery list 0 instance {instance}").as_str()));
    }
    let textures: std::collections::HashSet<_> = geometry
        .parts
        .iter()
        .flat_map(|p| p.texture.iter().chain(p.small_texture.iter().map(|s| &s.texture)))
        .collect();
    assert_eq!(textures.len(), 32);
    for texture in textures {
        assert!(engine::config::asset_path(texture).is_file(), "missing {texture}");
    }
}

fn course() -> CourseData {
    load_ron(COURSE_ASSET).unwrap_or_else(|error| panic!("course {COURSE_ASSET}: {error}"))
}

#[test]
#[cfg_attr(
    not(gt2_private_assets),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn the_baked_course_is_the_b1_course() {
    let course = course();
    assert!((course.lap_length_m - 5241.5273).abs() < 0.01);
    assert_eq!(course.nodes.len(), 124);
    assert_eq!(course.spans.len(), 37);
    assert_eq!(course.level_wrap, 3);
    assert_eq!(start_span(&course).map(|span| span.node), Some(78), "level {START_LEVEL}");
}

#[test]
#[cfg_attr(
    not(gt2_private_assets),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn the_start_point_matches_the_recorded_car() {
    // `.psx/runs/b1-gas-1200`: car+0x604 = 190.2086 m, car position
    // (x, z, y) = (401.4431, 449.69, 0) in 20.12 m.
    let start = course().point_at(START_DISTANCE_M).expect("start point");
    assert!((start.position_m[0] - 401.4431).abs() < 1.0, "x: {}", start.position_m[0]);
    assert!((start.position_m[2] + 449.69).abs() < 1.0, "z: {}", start.position_m[2]);
    assert!(start.direction[0] < -0.9, "heading: {:?}", start.direction);
}

#[test]
#[cfg_attr(
    not(gt2_private_assets),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn the_road_ribbon_covers_every_surface_node() {
    let course = course();
    let quads = course.surface_quads();
    assert_eq!(quads.len(), course.nodes.len(), "every B-1 node has a surface quad");
    let mesh = road_mesh(&course);
    let vertices = mesh.count_vertices();
    assert!(vertices >= quads.len() * 4, "at least four vertices per surface node");
    assert_eq!(vertices % 4, 0, "four vertices per road piece");
    assert_eq!(mesh.indices().unwrap().len(), vertices / 4 * 6);
}

#[test]
#[cfg_attr(
    not(gt2_private_assets),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn the_surface_runs_from_each_node_to_its_next() {
    let course = course();
    for quad in course.surface_quads() {
        let node = course.node(quad.node).unwrap();
        let next = course.node(node.next).unwrap();
        // The near edge's midpoint is the node's cross-section, the far
        // edge's the next node's, within the original's measured error
        // (max 0.49 m near, 0.52 m far over the whole course).
        let near = midpoint(quad.corners[0], quad.corners[1]);
        let far = midpoint(quad.corners[2], quad.corners[3]);
        assert!(near.distance(Vec3::from_array(node.position_m)) < 0.75, "node {} near edge", quad.node);
        assert!(far.distance(Vec3::from_array(next.position_m)) < 0.75, "node {} far edge", quad.node);
    }
}

fn midpoint(a: [f32; 3], b: [f32; 3]) -> Vec3 {
    (Vec3::from_array(a) + Vec3::from_array(b)) / 2.0
}
