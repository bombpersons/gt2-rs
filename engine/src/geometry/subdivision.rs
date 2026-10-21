//! Screen-area quad refinement. Corners are in perimeter order. Subdivision
//! keeps the original edge/centre topology, UV rounding and alternating child
//! orientation; it runs before affine rasterisation, not in the fragment shader.
use super::projection::{Projected, RasterProjection};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuadSubdivision {
    /// Twice the projected area, in target pixels squared.
    pub max_twice_area_px2: f32,
    pub max_depth: u8,
    pub uv_step: f32,
    pub guard_band_px: f32,
}

impl QuadSubdivision {
    pub fn validate(&self) -> Result<(), String> {
        if !self.max_twice_area_px2.is_finite()
            || self.max_twice_area_px2 <= 0.0
            || !(1..=12).contains(&self.max_depth)
            || !self.uv_step.is_finite()
            || self.uv_step <= 0.0
            || !self.guard_band_px.is_finite()
            || self.guard_band_px < 0.0
        {
            return Err("invalid quad subdivision parameters".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RasterVertex {
    /// Before perspective division: X right, Y down, Z forward.
    pub position: Vec3,
    pub uv: Vec2,
    pub color: Vec4,
}

/// Projection/precision for one refinement tree, also usable independently
/// of Bevy cameras (e.g. recorded renderer comparisons or an editor preview).
#[derive(Clone, Copy, Debug)]
pub struct QuadRaster {
    pub size: Vec2,
    pub focal_px: f32,
    pub position_step: f32,
    pub uv_step: f32,
    pub max_twice_area_px2: f32,
    pub max_depth: u8,
    /// Source corners use conservative visibility bounds. Newly projected
    /// midpoints use the actual viewport, retaining the inherited corner flags.
    pub guard_band_px: f32,
}

#[derive(Clone, Copy)]
struct Point {
    vertex: RasterVertex,
    projected: Projected,
}

#[derive(Default)]
pub struct QuadRefinement {
    pub quads: Vec<[RasterVertex; 4]>,
    /// Stitch triangles fill differences caused by projected midpoint rounding.
    pub stitches: Vec<[RasterVertex; 3]>,
}

fn area(a: Vec2, b: Vec2, c: Vec2) -> f32 {
    (b - a).perp_dot(c - a)
}

impl QuadRaster {
    /// Pixel position used for refinement decisions, independently of the
    /// final GPU draw. Also exposes the comparison boundary for recordings.
    pub fn pixel(&self, position: Vec3) -> Vec2 {
        self.project(position, false).pixel
    }
    fn project(&self, point: Vec3, original: bool) -> Projected {
        RasterProjection {
            size: self.size,
            focal_px: self.focal_px,
            position_step: self.position_step,
            guard_band_px: self.guard_band_px,
        }
        .project(point, original)
    }

    fn midpoint(&self, vertices: &[RasterVertex]) -> RasterVertex {
        let n = vertices.len() as f32;
        let position = vertices.iter().map(|v| v.position).sum::<Vec3>() / n;
        let uv = vertices.iter().map(|v| v.uv).sum::<Vec2>() / n;
        RasterVertex {
            position: (position / self.position_step).floor() * self.position_step,
            uv: (uv / self.uv_step).floor() * self.uv_step,
            color: vertices.iter().map(|v| v.color).sum::<Vec4>() / n,
        }
    }

    pub fn needs_split(&self, quad: &[RasterVertex; 4]) -> bool {
        let p = quad.map(|v| self.project(v.position, true));
        self.projected_needs_split(p)
    }

    pub fn root_visible(&self, quad: &[RasterVertex; 4], double_sided: bool) -> bool {
        let p = quad.map(|v| self.project(v.position, true));
        if p.iter().fold(31, |mask, v| mask & v.outside) != 0 {
            return false;
        }
        if double_sided {
            return true;
        }
        let [a, b, c, d] = quad.map(|v| v.position);
        // Equivalent to the source's extra geometric facing check for roots
        // crossing the projection plane. It also works for ordinary roots.
        (b - a).cross(c - a).dot(-a) > 0.0 || (c - a).cross(d - a).dot(-a) > 0.0
    }

    pub fn projection_failed(&self, quad: &[RasterVertex; 4]) -> bool {
        quad.iter().any(|v| self.project(v.position, true).failed)
    }

    fn projected_needs_split(&self, p: [Projected; 4]) -> bool {
        p.iter().any(|v| v.failed)
            || (area(p[0].pixel, p[1].pixel, p[2].pixel) + area(p[0].pixel, p[2].pixel, p[3].pixel)).abs()
                > self.max_twice_area_px2
    }

    /// Refine an already-selected root. A callback observes every split in
    /// traversal order, so tests can compare against original runtime records.
    pub fn refine(&self, root: [RasterVertex; 4], mut split: impl FnMut(u8, &[RasterVertex; 4])) -> QuadRefinement {
        let mut out = QuadRefinement::default();
        self.visit(
            root.map(|vertex| Point { vertex, projected: self.project(vertex.position, true) }),
            0,
            &mut out,
            &mut split,
        );
        out
    }

    fn visit(
        &self,
        quad: [Point; 4],
        depth: u8,
        out: &mut QuadRefinement,
        split: &mut impl FnMut(u8, &[RasterVertex; 4]),
    ) {
        let vertices = quad.map(|v| v.vertex);
        split(depth, &vertices);
        let edges: [_; 4] = std::array::from_fn(|i| {
            let vertex = self.midpoint(&[vertices[i], vertices[(i + 1) % 4]]);
            Point { vertex, projected: self.project(vertex.position, false) }
        });
        let vertex = self.midpoint(&vertices);
        let centre = Point { vertex, projected: self.project(vertex.position, false) };
        // Reverse perimeter traversal is significant: the source's packet
        // order and alternating recursive rotations select its diagonals.
        for i in (0..4).rev() {
            let previous = (i + 3) % 4;
            let stitch = [quad[i], edges[previous], quad[previous]];
            let p = stitch.map(|v| v.projected);
            if p.iter().all(|v| !v.failed) && area(p[0].pixel, p[1].pixel, p[2].pixel) != 0.0 {
                out.stitches.push(stitch.map(|v| v.vertex));
            }
        }
        for i in (0..4).rev() {
            let previous = (i + 3) % 4;
            let child = [quad[i], edges[i], centre, edges[previous]];
            let p = child.map(|v| v.projected);
            if p.iter().fold(31, |mask, v| mask & v.outside) != 0 {
                continue;
            }
            if self.projected_needs_split(p) {
                if depth + 1 < self.max_depth {
                    let child = if i % 2 == 0 { child } else { [child[3], child[0], child[1], child[2]] };
                    self.visit(child, depth + 1, out, split);
                }
            } else {
                out.quads.push(child.map(|v| v.vertex));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refinement_depth_limits_unprojectable_or_oversized_children() {
        let raster = QuadRaster {
            size: Vec2::new(320.0, 240.0),
            focal_px: 216.0,
            position_step: 1.0,
            uv_step: 1.0 / 256.0,
            max_twice_area_px2: 1.0,
            max_depth: 1,
            guard_band_px: 16.0,
        };
        let root = [
            [-10000.0, -10000.0, 10000.0],
            [10000.0, -10000.0, 10000.0],
            [10000.0, 10000.0, 10000.0],
            [-10000.0, 10000.0, 10000.0],
        ]
        .map(|p| RasterVertex { position: Vec3::from_array(p), uv: Vec2::ZERO, color: Vec4::ONE });
        let mut visits = 0;
        let result = raster.refine(root, |_, _| visits += 1);
        assert_eq!(visits, 1);
        assert!(result.quads.is_empty(), "unsafe leaves are dropped when the depth limit is reached");
    }
}
