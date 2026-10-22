//! Ordered surface polygons indexed by an authorable uniform grid.
//!
//! Quantized horizontal containment and centre-fan height interpolation follow
//! 800279E8/80027C70. Keep the supplied polygon and cell order: overlapping
//! surfaces choose the first polygon, including points exactly on an edge.
use super::CourseData;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContactPolygon {
    /// Local metres, clockwise when seen in the horizontal (x,z) plane.
    /// Triangles use one plane; quads use four triangles about their mean.
    pub vertices_m: Vec<[f64; 3]>,
    pub surface: u8,
    pub variant: u8,
    pub flags: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContactMesh {
    pub origin_m: [f64; 3],
    /// Local horizontal (x,z) origin and cell sizes in metres.
    pub grid_origin_m: [f64; 2],
    pub cell_size_m: [f64; 2],
    pub dimensions: [usize; 2],
    /// Row-major cells contain indices into polygons, in selection order.
    pub cells: Vec<Vec<usize>>,
    pub polygons: Vec<ContactPolygon>,
    /// Quantization for containment/quad centre and returned height. The
    /// importer supplies these from the original geometry arithmetic.
    pub horizontal_step_m: f64,
    pub height_step_m: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoadContact {
    /// Node selected by the longitudinal search, even when another node
    /// supplies the first containing surface polygon.
    pub node: u16,
    pub polygon_node: u16,
    pub polygon: usize,
    pub height_m: f64,
    pub surface: u8,
    pub variant: u8,
    pub flags: u8,
}

impl ContactMesh {
    pub fn validate(&self) -> Result<(), String> {
        if self.dimensions.contains(&0) || self.dimensions[0].checked_mul(self.dimensions[1]) != Some(self.cells.len())
        {
            return Err("contact grid dimensions do not match its cells".into());
        }
        if self.cell_size_m.iter().any(|v| !v.is_finite() || *v <= 0.0)
            || !self.horizontal_step_m.is_finite()
            || self.horizontal_step_m <= 0.0
            || !self.height_step_m.is_finite()
            || self.height_step_m <= 0.0
            || self.origin_m.iter().chain(self.grid_origin_m.iter()).any(|v| !v.is_finite())
        {
            return Err("contact grid has invalid coordinates or sampling steps".into());
        }
        for cell in &self.cells {
            if cell.iter().any(|i| *i >= self.polygons.len()) {
                return Err("contact cell references a missing polygon".into());
            }
        }
        for polygon in &self.polygons {
            if !(3..=4).contains(&polygon.vertices_m.len())
                || polygon.vertices_m.iter().flatten().any(|v| !v.is_finite())
            {
                return Err("contact polygon needs three or four finite vertices".into());
            }
        }
        Ok(())
    }

    pub fn sample(&self, world_m: [f64; 3]) -> Option<(usize, f64)> {
        let local = std::array::from_fn::<_, 3, _>(|i| world_m[i] - self.origin_m[i]);
        let horizontal = [local[0], local[2]].map(|v| (v / self.horizontal_step_m).floor() * self.horizontal_step_m);
        let cell: [isize; 2] =
            std::array::from_fn(|i| ((horizontal[i] - self.grid_origin_m[i]) / self.cell_size_m[i]).floor() as isize);
        if cell.iter().zip(self.dimensions).any(|(i, size)| *i < 0 || *i as usize >= size) {
            return None;
        }
        let indices = self.cells.get(cell[1] as usize * self.dimensions[0] + cell[0] as usize)?;
        for &index in indices {
            let polygon = self.polygons.get(index)?;
            if polygon.contains(horizontal, self.horizontal_step_m) {
                return Some((
                    index,
                    self.origin_m[1] + polygon.height(local, self.horizontal_step_m, self.height_step_m),
                ));
            }
        }
        None
    }
}

impl ContactPolygon {
    fn contains(&self, point: [f64; 2], step: f64) -> bool {
        // The source packs the horizontal query into signed halfwords before
        // edge tests. Area keeps the signed low word (GTE MAC0), 80027A78..B90.
        let q = point.map(|v| (v / step).round() as i64 as i16 as i64);
        (0..self.vertices_m.len()).all(|i| {
            let a = self.vertices_m[i];
            let b = self.vertices_m[(i + 1) % self.vertices_m.len()];
            let a = [(a[0] / step).round() as i64 - q[0], (a[2] / step).round() as i64 - q[1]];
            let b = [(b[0] / step).round() as i64 - q[0], (b[2] / step).round() as i64 - q[1]];
            (a[0] * b[1] - a[1] * b[0]) as i32 <= 0
        })
    }

    fn height(&self, point: [f64; 3], step: f64, height_step: f64) -> f64 {
        let mut a = self.vertices_m[0];
        let mut b = self.vertices_m[1];
        let mut c = self.vertices_m[2];
        if self.vertices_m.len() == 4 {
            // 80027D08..78 floors each coordinate of the four-vertex mean.
            c = std::array::from_fn(|i| {
                (self.vertices_m.iter().map(|v| v[i]).sum::<f64>() / 4.0 / step).floor() * step
            });
            let q = [(point[0] / step).floor() * step - c[0], (point[2] / step).floor() * step - c[2]];
            let mut mask = 0u8;
            for (i, v) in self.vertices_m.iter().enumerate() {
                if q[0] * (v[2] - c[2]) - q[1] * (v[0] - c[0]) < 0.0 {
                    mask |= 1 << i;
                }
            }
            // Sector/tie choices from 80027E58..EAC. This preserves the
            // original fan when the quad is nonplanar or at a fan boundary.
            let edge = match mask {
                1 | 3 | 7 => [3, 0],
                4 | 12 | 13 => [1, 2],
                8 | 9 | 11 => [2, 3],
                _ => [0, 1],
            };
            a = self.vertices_m[edge[0]];
            b = self.vertices_m[edge[1]];
        }
        let [ax, az, ay] = [a[0] - c[0], a[2] - c[2], a[1] - c[1]];
        let [bx, bz, by] = [b[0] - c[0], b[2] - c[2], b[1] - c[1]];
        let area = ax * bz - az * bx;
        if area == 0.0 {
            return c[1];
        }
        let delta = ((point[0] - c[0]) * (ay * bz - az * by) + (point[2] - c[2]) * (ax * by - ay * bx)) / area;
        // Signed wide numerator/division (80027F58..9C), followed by adding
        // the third vertex/centre height. Keep the truncation on the delta.
        c[1] + (delta / height_step).trunc() * height_step
    }
}

impl CourseData {
    /// Longitudinal node walk (80028394): equality advances into the next
    /// node, and a full cycle restores the starting candidate.
    pub fn node_at_position(&self, mut node: u16, point_m: [f64; 3]) -> Option<u16> {
        let ahead = |index: u16| -> Option<bool> {
            let node = self.node(index)?;
            let origin = node.contact_search_origin_m.unwrap_or(node.position_m.map(f64::from));
            Some(
                point_m.into_iter().zip(origin).zip(node.direction).map(|((p, o), d)| (p - o) * d as f64).sum::<f64>()
                    >= 0.0,
            )
        };
        for _ in 0..self.nodes.len() {
            if ahead(node)? {
                break;
            }
            node = self.node(node)?.previous;
        }
        let start = node;
        for _ in 0..self.nodes.len() {
            let next = self.node(node)?.next;
            if !ahead(next)? {
                return Some(node);
            }
            if next == start {
                return Some(start);
            }
            node = next;
        }
        Some(start)
    }

    /// Search the chosen node, then alternating previous/next neighbors.
    /// 80028470 bounds the fallback to eight nodes in each direction.
    pub fn road_contact(&self, start: u16, point_m: [f64; 3]) -> Option<RoadContact> {
        let node = self.node_at_position(start, point_m)?;
        let sample = |candidate| {
            let mesh = self.node(candidate)?.contact.as_ref()?;
            let (polygon, height_m) = mesh.sample(point_m)?;
            let p = &mesh.polygons[polygon];
            Some(RoadContact {
                node,
                polygon_node: candidate,
                polygon,
                height_m,
                surface: p.surface,
                variant: p.variant,
                flags: p.flags,
            })
        };
        if let Some(result) = sample(node) {
            return Some(result);
        }
        let mut previous = self.node(node)?.previous;
        let mut next = self.node(node)?.next;
        for _ in 0..8 {
            if let Some(result) = sample(previous) {
                return Some(result);
            }
            if let Some(result) = sample(next) {
                return Some(result);
            }
            previous = self.node(previous)?.previous;
            next = self.node(next)?.next;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn authored() -> ContactMesh {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/sandbox/contact.ron");
        ron::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }
    #[test]
    fn authored_grid_samples_ramp_edges_and_cell_bounds() {
        let mesh = authored();
        mesh.validate().unwrap();
        assert_eq!(mesh.sample([-1.0, 17.0, 0.0]), Some((0, 0.0)));
        assert_eq!(mesh.sample([0.0, -17.0, 0.0]), Some((0, 0.0)), "first polygon wins a shared edge");
        assert_eq!(mesh.sample([1.5, 17.0, 0.0]), Some((1, 0.375)));
        assert_eq!(mesh.sample([3.0, 17.0, 2.0]), Some((1, 0.75)));
        assert_eq!(mesh.sample([-4.0, 0.0, -4.0]), Some((0, 0.0)));
        assert_eq!(mesh.sample([4.0, 0.0, 0.0]), None, "grid maximum is exclusive");
        assert_eq!(mesh.sample([-4.0 - 1.0 / 65536.0, 0.0, 0.0]), None, "negative horizontal sampling floors");
    }
    #[test]
    fn nonplanar_quad_uses_sampled_centre_fan_and_signed_height_division() {
        let mut mesh = authored();
        mesh.cells[0] = vec![1];
        mesh.polygons[1].vertices_m[2][1] = 2.0;
        // Centre height is the floored mean, not either fixed diagonal.
        assert_eq!(mesh.sample([2.0, 0.0, 0.0]), Some((1, 0.75)));
        mesh.polygons[1].vertices_m[0][1] = -1.0 / 64.0;
        mesh.polygons[1].vertices_m[1][1] = 0.0;
        mesh.polygons[1].vertices_m[2][1] = 0.0;
        mesh.polygons[1].vertices_m[3][1] = 0.0;
        assert_eq!(mesh.sample([2.0, 0.0, 0.0]), Some((1, -1.0 / 64.0)), "centre itself floors before interpolation");
    }
    #[test]
    fn validation_rejects_missing_polygons_and_invalid_sampling_contracts() {
        let mut mesh = authored();
        mesh.cells[0].push(2);
        assert!(mesh.validate().is_err());
        let mut mesh = authored();
        mesh.polygons[0].vertices_m.pop();
        mesh.polygons[0].vertices_m.pop();
        assert!(mesh.validate().is_err());
        let mut mesh = authored();
        mesh.horizontal_step_m = 0.0;
        assert!(mesh.validate().is_err());
        let mut mesh = authored();
        mesh.dimensions = [2, 2];
        assert!(mesh.validate().is_err());
    }
}
