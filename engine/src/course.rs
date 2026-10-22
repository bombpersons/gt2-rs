//! Course data in the engine's own format: the ordered road table a car's
//! course-distance solver walks, and the draw spans laid over it.
//!
//! A course is data, not code. The importer (`tools/import`) converts the
//! original's fixed-point tables once; the engine reads normal units (metres)
//! and a different game can author the same shape by hand in RON.
//!
//! What the original stores, and how it maps here:
//!
//! - **Nodes** are the road in driving order. Each node has a centreline point,
//!   a unit direction, a cumulative distance along the lap, and links to the
//!   neighbouring nodes. `next` is the next node in driving order; following it
//!   visits every node and returns to the start. Distances increase along
//!   `next` and wrap once per lap, so the last node's distance plus the last
//!   segment equals [`CourseData::lap_length_m`].
//! - **Spans** divide the drawn road into pieces. A span names the node it
//!   starts at, a distance range on the lap-length scale, and the range of
//!   detail levels it serves. The original picks one span per frame from the
//!   car's course distance and its level-of-detail setting.
//! - **Surface quads** are the road itself. Each node carries four world
//!   corners forming the asphalt from that node's cross-section to its next
//!   node's: `[near 0, near 1, far 0, far 1]`. [`CourseData::surface_quads`]
//!   yields the non-degenerate ones, which a renderer turns into a ribbon.
//!   The original stores the quad explicitly rather than sweeping a single
//!   cross-section, so two adjacent quads may not share their edge exactly;
//!   keep that (it is what the original draws).
//!
//! Collision uses node-local [`ContactMesh`] grids and ordered polygons,
//! separately from the old surface ribbon. [`CourseData::road_contact`]
//! walks the road nodes and samples a containing triangle/quad. Imported
//! contact geometry and search origins retain exact metre coordinates;
//! another game can author the same grids directly (the sandbox does).
//!
//! Span selection has one quirk worth keeping: the original's span table also
//! stores [`CourseData::level_wrap`]. A level above it wraps back into
//! `1..=level_wrap` (skipping 0), and a span whose `min_level` is below the
//! level compares the car's distance **plus one lap** rather than the distance
//! itself. Combined with overlapping level ranges, that selects exactly one
//! span group per level. [`CourseData::span_at`] does the same.
//!
//! The original keeps these in 16.16 fixed point. Importers convert to `f32`
//! metres: a 16.16 value in this range has a resolution below 0.0003 m, well
//! under the size of a tick's movement, and the format stays readable. Keep the
//! integer form in the importer (and in format notes) as the evidence.
use serde::{Deserialize, Serialize};

mod contact;
pub use contact::{ContactMesh, ContactPolygon, RoadContact};

/// A course: one lap's road table plus its draw spans.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CourseData {
    /// Length of one lap, metres. Course distances wrap at this value.
    pub lap_length_m: f32,
    /// Detail levels above this value wrap back into `1..=level_wrap`; 0
    /// disables the wrap. Comes from the original's span table.
    #[serde(default)]
    pub level_wrap: u16,
    /// Road nodes in table order; [`CourseNode::next`] is the lap order.
    #[serde(default)]
    pub nodes: Vec<CourseNode>,
    /// Draw spans over the road, in table order.
    #[serde(default)]
    pub spans: Vec<RoadSpan>,
}

/// One road node: a centreline sample and its links.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CourseNode {
    /// Previous node in driving order (`next` of the node before this one).
    pub previous: u16,
    /// Next node in driving order.
    pub next: u16,
    /// The original's per-node flags word (meaning still open; preserved so
    /// nothing is silently dropped).
    pub flags: u32,
    /// Cumulative distance from the table's origin, metres.
    pub distance_m: f32,
    /// Two per-node values, normalised to 1.0: the original stores signed
    /// 16-bit values in `-4096..=4096`, 4096 = 1.0. Their meaning is still
    /// open; preserved so nothing is silently dropped.
    pub scale: [f32; 2],
    /// Centreline point in world metres, `[x, y, z]`.
    pub position_m: [f32; 3],
    /// Unit direction along the road in world space, `[x, y, z]`.
    pub direction: [f32; 3],
    /// The road surface from this node to its next node, in world metres:
    /// `[near 0, near 1, far 0, far 1]`. The `near` corners belong to this
    /// node's cross-section and the `far` corners to [`CourseNode::next`]'s.
    /// A degenerate quad (all four corners equal, or zero area) means the
    /// original has no surface here; [`CourseData::surface_quads`] skips it.
    #[serde(default)]
    pub surface_m: [[f32; 3]; 4],
    /// Exact centreline point for contact-node boundaries. f64 preserves the
    /// imported 16.16 position even far from the course origin.
    #[serde(default)]
    pub contact_search_origin_m: Option<[f64; 3]>,
    #[serde(default)]
    pub contact: Option<ContactMesh>,
}

/// A piece of drawn road. The original selects one from the car's position and
/// detail level, then draws that piece's geometry.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RoadSpan {
    /// Span kind (the original's first word; meaning still open).
    pub kind: u16,
    /// Lowest detail level this span serves.
    pub min_level: u16,
    /// Highest detail level this span serves.
    pub max_level: u16,
    /// Node the span starts at.
    pub node: u16,
    /// Start of the span on the lap-length scale, metres.
    pub start_m: f32,
    /// End of the span on the lap-length scale, metres.
    pub end_m: f32,
}

/// A piece of the road surface, ready to become two triangles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceQuad {
    /// Node the quad starts at.
    pub node: u16,
    /// Four world-space corners: `[near 0, near 1, far 0, far 1]`. Triangles
    /// `[0, 2, 1]` and `[1, 2, 3]` face up for a course in the original's
    /// corner order.
    pub corners: [[f32; 3]; 4],
    /// Cumulative course distance at the near and far edges, metres. The far
    /// value is at least the near one (one lap is added across the wrap), so
    /// renderers can use the difference as a texture coordinate.
    pub distance_m: [f32; 2],
}

/// A point on the road centreline at a course distance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoursePoint {
    pub position_m: [f32; 3],
    /// Unit direction of the segment the point lies on.
    pub direction: [f32; 3],
}

impl CourseData {
    /// The node with this index, if it exists.
    pub fn node(&self, index: u16) -> Option<&CourseNode> {
        self.nodes.get(index as usize)
    }

    /// Follows `next` from node 0 and returns the nodes visited before
    /// returning to the start. A well-formed lap visits every node once.
    pub fn lap(&self) -> Vec<u16> {
        let mut visited = vec![false; self.nodes.len()];
        let mut order = Vec::with_capacity(self.nodes.len());
        let mut at = 0u16;
        while let Some(node) = self.node(at) {
            let index = at as usize;
            if visited[index] {
                break;
            }
            visited[index] = true;
            order.push(at);
            at = node.next;
        }
        order
    }

    /// The point on the road centreline at a course distance, interpolated
    /// along the segment in driving order that contains it. Returns `None`
    /// for an empty course or a non-positive lap length.
    pub fn point_at(&self, distance_m: f32) -> Option<CoursePoint> {
        if self.nodes.len() < 2 || self.lap_length_m <= 0.0 {
            return None;
        }
        let distance_m = distance_m.rem_euclid(self.lap_length_m);
        let order = self.lap();
        for (position, &from) in order.iter().enumerate() {
            let to = order[(position + 1) % order.len()];
            let a = self.node(from)?;
            let b = self.node(to)?;
            let start = a.distance_m;
            let mut end = b.distance_m;
            if end <= start {
                end += self.lap_length_m;
            }
            if distance_m < start || distance_m >= end {
                continue;
            }
            let t = (distance_m - start) / (end - start);
            let position_m =
                std::array::from_fn(|axis| a.position_m[axis] + (b.position_m[axis] - a.position_m[axis]) * t);
            let delta: [f32; 3] = std::array::from_fn(|axis| b.position_m[axis] - a.position_m[axis]);
            let length = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
            let direction = if length > 0.0 { delta.map(|v| v / length) } else { a.direction };
            return Some(CoursePoint { position_m, direction });
        }
        None
    }

    /// The road's surface quads, in node-table order, skipping degenerate
    /// ones. Each quad's `distance_m` is the near and far edge's cumulative
    /// course distance; the far value is unwrapped across the lap boundary.
    pub fn surface_quads(&self) -> Vec<SurfaceQuad> {
        let mut quads = Vec::new();
        for (index, node) in self.nodes.iter().enumerate() {
            let Some(next) = self.node(node.next) else { continue };
            let [a, b, c, d] = node.surface_m;
            // Twice the triangle area; also the up component's sign, which
            // tells a renderer whether to reverse the winding.
            let ab = sub(b, a);
            let ac = sub(c, a);
            let normal = cross(ac, ab);
            if normal.iter().map(|v| v * v).sum::<f32>() < 1e-12 {
                continue;
            }
            let mut far = next.distance_m;
            if far < node.distance_m {
                far += self.lap_length_m;
            }
            quads.push(SurfaceQuad { node: index as u16, corners: [a, b, c, d], distance_m: [node.distance_m, far] });
        }
        quads
    }

    /// The span the original would pick for a course distance and detail
    /// level. This is the original's selection rule, quirks included:
    ///
    /// 1. A level above [`CourseData::level_wrap`] wraps into
    ///    `1..=level_wrap`; 0 means no wrap.
    /// 2. A span is in range when `min_level <= level <= max_level`.
    /// 3. If `min_level < level`, the distance is compared after adding one
    ///    lap; otherwise it is compared as given. The interval wraps once when
    ///    `end_m < start_m`.
    ///
    /// Step 3 is what makes overlapping level ranges select one group per
    /// level; see the module docs.
    pub fn span_at(&self, distance_m: f32, level: u16) -> Option<&RoadSpan> {
        let level =
            if self.level_wrap != 0 && self.level_wrap < level { (level - 1) % self.level_wrap + 1 } else { level };
        self.spans.iter().find(|span| {
            if level < span.min_level || level > span.max_level {
                return false;
            }
            let mut end = span.end_m;
            if end < span.start_m {
                end += self.lap_length_m;
            }
            let mut at = distance_m;
            if span.min_level < level {
                at += self.lap_length_m;
            }
            span.start_m <= at && at < end
        })
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|axis| a[axis] - b[axis])
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line() -> CourseData {
        CourseData {
            lap_length_m: 30.0,
            level_wrap: 2,
            nodes: vec![
                CourseNode { previous: 2, next: 1, distance_m: 0.0, ..default_node([0.0, 0.0, 0.0]) },
                CourseNode { previous: 0, next: 2, distance_m: 10.0, ..default_node([10.0, 0.0, 0.0]) },
                CourseNode { previous: 1, next: 0, distance_m: 20.0, ..default_node([20.0, 0.0, 0.0]) },
            ],
            spans: vec![
                // Level group 0: the wrap run, 24..6.
                RoadSpan { kind: 3, min_level: 0, max_level: 1, node: 2, start_m: 24.0, end_m: 6.0 },
                // Level group 1: 6..24.
                RoadSpan { kind: 3, min_level: 1, max_level: 2, node: 0, start_m: 6.0, end_m: 24.0 },
                // Level group 2: 6..24 again, different node.
                RoadSpan { kind: 3, min_level: 2, max_level: 3, node: 1, start_m: 6.0, end_m: 24.0 },
            ],
        }
    }

    fn default_node(position_m: [f32; 3]) -> CourseNode {
        CourseNode {
            previous: 0,
            next: 0,
            flags: 0,
            distance_m: 0.0,
            scale: [1.0, 1.0],
            position_m,
            direction: [1.0, 0.0, 0.0],
            surface_m: [[0.0; 3]; 4],
            contact_search_origin_m: None,
            contact: None,
        }
    }

    #[test]
    fn following_next_visits_every_node_once() {
        assert_eq!(line().lap(), vec![0, 1, 2]);
    }

    #[test]
    fn point_at_interpolates_along_the_lap() {
        let course = line();
        let point = course.point_at(5.0).unwrap();
        assert_eq!(point.position_m, [5.0, 0.0, 0.0]);
        assert_eq!(point.direction, [1.0, 0.0, 0.0]);
        // 25 lies on the wrap segment, node 2 (20 m) back to node 0 (0 m).
        let point = course.point_at(25.0).unwrap();
        assert_eq!(point.position_m, [10.0, 0.0, 0.0]);
        assert_eq!(point.direction, [-1.0, 0.0, 0.0]);
        // A distance one lap on is the same point as zero.
        assert_eq!(course.point_at(30.0), course.point_at(0.0));
    }

    #[test]
    fn surface_quads_skip_degenerate_nodes() {
        let mut course = line();
        course.nodes[0].surface_m = [[0.0, 0.0, -1.0], [0.0, 0.0, 1.0], [10.0, 0.0, -1.0], [10.0, 0.0, 1.0]];
        course.nodes[1].surface_m = [[10.0, 0.0, -1.0], [10.0, 0.0, 1.0], [20.0, 0.0, -1.0], [20.0, 0.0, 1.0]];
        // Node 2 keeps the default all-zero quad and is skipped.
        let quads = course.surface_quads();
        assert_eq!(quads.len(), 2);
        assert_eq!(quads[0].node, 0);
        assert_eq!(quads[0].corners[0], [0.0, 0.0, -1.0]);
        assert_eq!(quads[0].distance_m, [0.0, 10.0]);
        assert_eq!(quads[1].node, 1);
        assert_eq!(quads[1].distance_m, [10.0, 20.0]);
    }

    #[test]
    fn span_selection_shifts_the_distance_by_level() {
        let course = line();
        // Level 0 compares the distance as given; 10 is in no span.
        assert_eq!(course.span_at(10.0, 0).map(|s| s.node), None);
        // Level 1 belongs to the group whose min_level is 1; the group at 0
        // gets a lap added and no longer covers this distance.
        assert_eq!(course.span_at(10.0, 1).map(|s| s.node), Some(0));
        assert_eq!(course.span_at(10.0, 2).map(|s| s.node), Some(1));
        // The wrap run at level 0: 28 lies in 24..36 after wrapping.
        assert_eq!(course.span_at(28.0, 0).map(|s| s.node), Some(2));
    }

    #[test]
    fn levels_above_the_wrap_roll_over() {
        let course = line();
        // level_wrap 2: level 3 wraps to 1, level 4 to 2.
        assert_eq!(course.span_at(10.0, 3).map(|s| s.node), Some(0));
        assert_eq!(course.span_at(10.0, 4).map(|s| s.node), Some(1));
        assert_eq!(course.span_at(10.0, 5).map(|s| s.node), Some(0));
    }

    #[test]
    fn a_baked_course_loads() {
        // Written by `import tro`, kept in the game's assets: the importer and
        // this type must agree on the shape.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../game/assets/gt2/course/TC_lisence.ron");
        let Ok(text) = std::fs::read_to_string(&path) else {
            // Allow a checkout without imported assets yet.
            return;
        };
        let course: CourseData = ron::from_str(&text).expect("baked course parses");
        assert!(course.nodes.len() > 1);
        assert!(course.lap_length_m > 0.0);
        assert_eq!(course.lap().len(), course.nodes.len(), "the road table is one lap");
        for (index, node) in course.nodes.iter().enumerate() {
            assert!(node.next < course.nodes.len() as u16, "node {index} links inside the table");
            assert!(node.previous < course.nodes.len() as u16, "node {index} links inside the table");
        }
        let order = course.lap();
        let distance = |index: &u16| course.nodes[*index as usize].distance_m;
        let descents = order.windows(2).filter(|pair| distance(&pair[1]) < distance(&pair[0])).count();
        assert_eq!(descents, 1, "distances rise along the lap and wrap once");
        for node in &course.nodes {
            let length = node.direction.iter().map(|v| v * v).sum::<f32>().sqrt();
            assert!((length - 1.0).abs() < 0.01, "directions are unit length");
        }
        // The B-1 start: the car's course distance is 190.2086 m and the
        // original's node index is 78 (car+0x600). The level-1 and level-2
        // span groups covering that distance name node 78.
        for level in 1..=2 {
            assert_eq!(
                course.span_at(190.2086, level).map(|span| span.node),
                Some(78),
                "level {level} at the B-1 start"
            );
        }

        // Source course axes are retained (negative z on this straight),
        // so the source corner order winds downward. Renderers choose their
        // upward triangle winding from that order. Node 78's near edge is
        // the node's own cross-section (midpoint within half a metre of the
        // centreline point, as measured from the original).
        let quads = course.surface_quads();
        assert!(!quads.is_empty(), "the baked course has a road surface");
        for quad in &quads {
            let normal = cross(sub(quad.corners[2], quad.corners[0]), sub(quad.corners[1], quad.corners[0]));
            assert!(normal[1] < 0.0, "surface quad {} retains the source winding", quad.node);
        }
        let node = course.node(78).unwrap();
        let near =
            [(node.surface_m[0][0] + node.surface_m[1][0]) / 2.0, (node.surface_m[0][2] + node.surface_m[1][2]) / 2.0];
        let offset = ((near[0] - node.position_m[0]).powi(2) + (near[1] - node.position_m[2]).powi(2)).sqrt();
        assert!(offset < 0.5, "node 78's near edge is centred on the node: {offset} m");

        // The interpolated start point lands where the original's car was:
        // x = 401.44 m, course z = -449.69 m, heading -x.
        let start = course.point_at(190.2086).unwrap();
        assert!((start.position_m[0] - 401.4431).abs() < 1.0, "start x: {}", start.position_m[0]);
        assert!((start.position_m[2] + 449.69).abs() < 1.0, "start z: {}", start.position_m[2]);
        assert!(start.direction[0] < -0.9, "start heading: {:?}", start.direction);
    }
}
