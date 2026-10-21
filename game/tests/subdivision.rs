//! Original recursive renderer observations, independent of the game scene.
use bevy::prelude::*;
use engine::geometry::{QuadRaster, RasterVertex};
use serde::Deserialize;

#[derive(Deserialize)]
struct Node {
    depth: u8,
    positions: [[f32; 3]; 4],
    uv: [[f32; 2]; 4],
}
#[derive(Deserialize)]
struct Root {
    source_address: String,
    nodes: Vec<Node>,
    leaves: usize,
    stitches: usize,
    width: f32,
    height: f32,
    focal_px: f32,
    max_twice_area_px2: f32,
    leaf_packets: Vec<Packet>,
    stitch_packets: Vec<Packet>,
}
#[derive(Deserialize)]
struct Packet {
    pixels: Vec<[f32; 2]>,
    uv: Vec<[f32; 2]>,
}
#[derive(Deserialize)]
struct Recording {
    roots: Vec<Root>,
}

fn check(name: &str) {
    let path = format!("{}/tests/data/original/subdivision/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let recording: Recording = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let totals = (
        recording.roots.len(),
        recording.roots.iter().map(|r| r.nodes.len()).sum::<usize>(),
        recording.roots.iter().map(|r| r.leaves).sum::<usize>(),
        recording.roots.iter().map(|r| r.stitches).sum::<usize>(),
    );
    assert_eq!(
        totals,
        match name {
            "bumper" => (16, 113, 194, 134),
            "chase_holdout" => (16, 93, 167, 151),
            "far_holdout" => (15, 124, 222, 194),
            _ => panic!("unknown recording"),
        }
    );
    let mut max_pixel_error = 0.0f32;
    for root in recording.roots {
        let first = &root.nodes[0];
        let input = std::array::from_fn(|i| RasterVertex {
            position: Vec3::from_array(first.positions[i]),
            uv: Vec2::from_array(first.uv[i]) / 256.0,
            color: Vec4::ONE,
        });
        let raster = QuadRaster {
            size: Vec2::new(root.width, root.height),
            focal_px: root.focal_px,
            position_step: 1.0,
            uv_step: 1.0 / 256.0,
            max_twice_area_px2: root.max_twice_area_px2,
            max_depth: 7,
            guard_band_px: 16.0,
        };
        let mut splits = Vec::new();
        let output = raster.refine(input, |depth, quad| splits.push((depth, *quad)));
        assert_eq!(splits.len(), root.nodes.len(), "{name} {} split count", root.source_address);
        for ((depth, quad), original) in splits.iter().zip(&root.nodes) {
            assert_eq!(*depth, original.depth, "{} split depth", root.source_address);
            for (i, vertex) in quad.iter().enumerate() {
                assert_eq!(vertex.position.to_array(), original.positions[i], "{} split position", root.source_address);
                assert_eq!((vertex.uv * 256.0).to_array(), original.uv[i], "{} split UV", root.source_address);
            }
        }
        assert_eq!(output.quads.len(), root.leaves, "{} leaf count", root.source_address);
        assert_eq!(output.stitches.len(), root.stitches, "{} stitch count", root.source_address);
        assert_eq!(root.leaf_packets.len(), root.leaves);
        assert_eq!(root.stitch_packets.len(), root.stitches);
        for (leaf, packet) in output.quads.iter().zip(&root.leaf_packets) {
            assert_eq!(packet.uv.len(), 4);
            assert_eq!(packet.pixels.len(), 4);
            for (i, corner) in [0, 1, 3, 2].into_iter().enumerate() {
                assert_eq!((leaf[corner].uv * 256.0).to_array(), packet.uv[i], "{} leaf UV", root.source_address);
                let error =
                    (raster.pixel(leaf[corner].position) - Vec2::from_array(packet.pixels[i])).abs().max_element();
                max_pixel_error = max_pixel_error.max(error);
                assert!(error <= 1.0, "{} leaf pixel error {error}", root.source_address);
            }
        }
        for (stitch, packet) in output.stitches.iter().zip(&root.stitch_packets) {
            assert_eq!(packet.uv.len(), 3);
            assert_eq!(packet.pixels.len(), 3);
            for (i, vertex) in stitch.iter().enumerate() {
                assert_eq!((vertex.uv * 256.0).to_array(), packet.uv[i], "{} stitch UV", root.source_address);
                let error = (raster.pixel(vertex.position) - Vec2::from_array(packet.pixels[i])).abs().max_element();
                max_pixel_error = max_pixel_error.max(error);
                assert!(error <= 1.0, "{} stitch pixel error {error}", root.source_address);
            }
        }
    }
    println!(
        "{name}: split topology/coordinates/UVs and packet UVs match exactly; maximum projected corner error {max_pixel_error} px"
    );
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn bumper_view_matches_original_road_refinement() {
    check("bumper");
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn independent_chase_view_matches_original_refinement() {
    check("chase_holdout");
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn far_view_holdout_matches_original_refinement_and_packets() {
    check("far_holdout");
}
