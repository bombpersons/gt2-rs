//! Material detail is selected on the source polygon, before refinement.
use super::SurfaceBlend;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SmallTexture {
    pub max_twice_area_px2: u32,
    pub texture: String,
    pub blend: SurfaceBlend,
    /// Alternate coordinates aligned with the source part's vertices.
    pub uv: Vec<[f32; 2]>,
}

/// The source compares an unsigned triangle NCLIP result, but takes the
/// absolute perimeter area for quads. In particular, a negative two-sided
/// triangle selects large detail (0x800245A8..CC, 0x80024E7C..B8).
/// Pixels are in source perimeter order, X right and Y down.
pub fn selects_small_texture(max_twice_area_px2: u32, pixels: &[Vec2]) -> bool {
    let cross = |a: Vec2, b: Vec2, c: Vec2| (b - a).perp_dot(c - a) as i32;
    let area = match *pixels {
        [a, b, c] => (-cross(a, b, c)) as u32,
        [a, b, c, d] => (cross(a, b, c) + cross(a, c, d)).unsigned_abs(),
        _ => panic!("texture detail requires a triangle or quad"),
    };
    area <= max_twice_area_px2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equality_and_triangle_winding_have_source_semantics() {
        let tri = [Vec2::ZERO, Vec2::Y * 4.0, Vec2::X * 4.0];
        assert!(selects_small_texture(16, &tri));
        assert!(!selects_small_texture(15, &tri));
        assert!(!selects_small_texture(16, &[tri[0], tri[2], tri[1]]));
        let quad = [Vec2::ZERO, Vec2::X * 4.0, Vec2::splat(4.0), Vec2::Y * 4.0];
        assert!(selects_small_texture(32, &quad));
        assert!(!selects_small_texture(31, &quad));
        assert!(selects_small_texture(32, &[quad[3], quad[2], quad[1], quad[0]]));
    }
}
