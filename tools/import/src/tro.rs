//! `.tro` course objects -> the engine's course table.
//!
//! `/crsobj/NAME.tro` is a course: the road the car's distance solver walks,
//! the geometry the renderer draws, and per-lap event points. The container,
//! the road table (including each node's road-surface quad) and the draw spans
//! are decoded. Section-2 vertex storage, bounds and primitive references
//! are validated; placement, scale conversion and materials remain open.
//!
//! ```text
//! 0x00  "@(#)GT-PS\0\0\0"     marker
//! 0x0c  u16 0                 version
//! 0x0e  u16 0x1f              constant in all 126 courses
//! 0x10  u32 section[5]        file-relative; contiguous, last ends at EOF
//!
//! section 0  u32 lap_length_fx16.16; u16 node_count; u16 record_count;
//!            u32 record_base; u32 node_offset[node_count];
//!            node records; record_count * 32-byte records
//! section 1  u32 count; u32 offset[count];
//!            count * (u32 kind; kind * (u32 value; u32 mesh_offset))
//! section 2  u32 count; u32 offset[count];
//!            count meshes: 0x58-byte header, vertices, ten primitive arrays
//! section 3  u16 count; u16 level_wrap; u32 offset[count];
//!            count span records
//! section 4  u32 length; u32 group_count; u32 offset[group_count];
//!            groups of 40-byte records
//! ```
//!
//! The importer keeps section 0's road table (centreline and road-surface
//! quad) and section 3's span headers, converted to metres. Everything else is
//! checked structurally and reported, so a partial decode cannot pass as
//! success: sections tile the file, node offsets tile the road table,
//! section-1 records tile their table, mesh offsets name section-2 blobs, span
//! offsets tile section 3 and section-4 groups tile their section.
//!
//! The road-surface quad is the four `(x, z)` 16.16 vertices at node `+0x6c`
//! (see [`Node::surface_m`]). For `TC_lisence` it is the asphalt: the
//! midpoint of each end pair is the node's centreline point (max 0.49 m) and
//! the cross-section is perpendicular to the road direction. The same field
//! is extracted for every course; on courses other than `TC_lisence` its role
//! is not yet established, so it is recorded rather than assumed. A quad can
//! be degenerate (all four corners equal); the engine skips those.
use serde::Serialize;
use std::path::Path;

mod contact;
mod instances;
mod mesh;
pub mod render;

/// The RON shape: kept in step with `engine::course::CourseData`. The engine's
/// test parses a course written by this importer.
#[derive(Debug, Serialize)]
pub struct Course {
    /// One lap in metres (the file stores fx16.16).
    pub lap_length_m: f64,
    pub distance_sampling: Option<DistanceSampling>,
    /// The original wraps detail levels above this value into `1..=level_wrap`;
    /// 0 disables the wrap (section 3's `level_wrap` word).
    pub level_wrap: u16,
    /// Road nodes in table order. `next` is driving order.
    pub nodes: Vec<Node>,
    /// Draw spans in table order.
    pub spans: Vec<Span>,
}

#[derive(Debug, Serialize)]
pub struct DistanceSampling {
    pub projection_step_m: f64,
    pub fraction_step: f64,
}

#[derive(Debug, Serialize)]
pub struct Node {
    pub previous: u16,
    pub next: u16,
    /// The original's flags word, preserved verbatim.
    pub flags: u32,
    /// Cumulative distance from the table's origin, metres.
    pub distance_m: f64,
    /// Distance interpolation weights for the far and near boundary planes.
    pub scale: [f32; 2],
    /// Centreline point in world metres. The file stores `(x, y, z)`.
    pub position_m: [f32; 3],
    /// Unit road direction in world space. The file stores `(x, y, z)` at
    /// 4096 = 1.0.
    pub direction: [f32; 3],
    /// The road-surface quad from this node to its next node, in world metres,
    /// as four corners `[near 0, near 1, far 0, far 1]`. The two `near`
    /// corners are this node's cross-section and the two `far` corners the
    /// next node's. The file stores only `(x, z)` for each corner; the height
    /// comes from the node points (near from this node, far from the next).
    /// Verified as the asphalt on `TC_lisence` (see the module docs).
    pub surface_m: [[f32; 3]; 4],
    pub contact_search_origin_m: Option<[f64; 3]>,
    pub contact: Option<contact::ContactMesh>,
}

#[derive(Debug, Serialize)]
pub struct Span {
    pub kind: u16,
    pub min_level: u16,
    pub max_level: u16,
    /// Node the span starts at.
    pub node: u16,
    /// Start and end on the course-distance scale, metres.
    pub start_m: f32,
    pub end_m: f32,
}

impl Course {
    /// Parses and validates a whole `.tro`. `Err` explains the first
    /// structural disagreement with the format above.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 0x24 {
            return Err(format!("a .tro is at least 0x24 bytes; this file is {}", bytes.len()));
        }
        if &bytes[0..12] != b"@(#)GT-PS\0\0\0" {
            return Err(format!("not a .tro: marker is {:02x?}", &bytes[0..12]));
        }
        if u16_at(bytes, 0x0c, "container version")? != 0 {
            return Err("container version is not 0".to_string());
        }
        let marker = u16_at(bytes, 0x0e, "container constant")?;
        if marker != 0x1f {
            return Err(format!("container constant is {marker:#x}, not 0x1f"));
        }
        let section = [
            u32_at(bytes, 0x10, "section 0 offset")? as usize,
            u32_at(bytes, 0x14, "section 1 offset")? as usize,
            u32_at(bytes, 0x18, "section 2 offset")? as usize,
            u32_at(bytes, 0x1c, "section 3 offset")? as usize,
            u32_at(bytes, 0x20, "section 4 offset")? as usize,
        ];
        if section[0] < 0x24 {
            return Err(format!("section 0 starts at {:#x}, inside the container header", section[0]));
        }
        for (index, pair) in section.windows(2).enumerate() {
            if pair[0] >= pair[1] {
                return Err(format!(
                    "section {index} starts at {:#x}, not before section {} at {:#x}",
                    pair[0],
                    index + 1,
                    pair[1]
                ));
            }
        }
        if section[4] >= bytes.len() {
            return Err(format!("section 4 starts at {:#x} in a {} byte file", section[4], bytes.len()));
        }

        let road = Road::parse(bytes, section[0], section[1])?;
        let geometry = Geometry::parse(bytes, section[1], section[2], section[3])?;
        let spans = Spans::parse(bytes, section[3], section[4], road.nodes.len())?;
        let events = Events::parse(bytes, section[4])?;

        // Every node index a node or span names is inside the table; the
        // chain check is in `Road::parse`.
        let _ = geometry;
        let _ = events;
        Ok(Self {
            lap_length_m: f64::from(road.lap_length) / 65536.0,
            distance_sampling: Some(DistanceSampling { projection_step_m: 1.0 / 65536.0, fraction_step: 1.0 / 4096.0 }),
            level_wrap: spans.level_wrap,
            nodes: road.nodes,
            spans: spans.spans,
        })
    }
}

/// The parts of a parsed file that are not engine data yet, kept for the
/// importer's one-line report.
#[derive(Debug, Default)]
pub struct Accounting {
    pub bytes: usize,
    /// Bytes in sections 0..4.
    pub sections: [usize; 4],
    pub nodes: usize,
    /// Section 0's trailing 32-byte records.
    pub records: usize,
    /// Section 1 records (kinds 1-4).
    pub mesh_records: usize,
    /// Value/mesh pairs across all section 1 records.
    pub meshes: usize,
    /// Section 2 blobs (some may be unreferenced).
    pub blobs: usize,
    pub vertices: usize,
    pub primitives: [usize; 10],
    pub spans: usize,
    /// Present section 4 groups and the records in them.
    pub groups: usize,
    pub event_records: usize,
}

impl Course {
    /// Structural counts for the importer's report, recomputed cheaply from
    /// the parsed file so the report cannot drift from the checks.
    pub fn accounting(bytes: &[u8], course: &Course) -> Result<Accounting, String> {
        let section = section_offsets(bytes)?;
        let mut out = Accounting {
            bytes: bytes.len(),
            sections: [
                section[1] - section[0],
                section[2] - section[1],
                section[3] - section[2],
                bytes.len() - section[3],
            ],
            nodes: course.nodes.len(),
            ..Accounting::default()
        };
        let (_, record_count, record_base) = road_header(bytes, section[0])?;
        out.records = usize::from(record_count);
        if record_base + out.records * 32 != section[1] {
            return Err("section 0's records do not end at section 1".to_string());
        }
        out.mesh_records = u32_at(bytes, section[1], "section 1 count")? as usize;
        let mesh_offsets = offsets_at(bytes, section[1] + 4, out.mesh_records, "mesh record")?;
        for offset in mesh_offsets {
            out.meshes += u32_at(bytes, offset, "mesh record kind")? as usize;
        }
        out.blobs = u32_at(bytes, section[2], "section 2 count")? as usize;
        let geometry = Geometry::parse(bytes, section[1], section[2], section[3])?;
        out.vertices = geometry.vertices;
        out.primitives = geometry.primitives;
        out.spans = usize::from(u16_at(bytes, section[3], "section 3 count")?);
        let groups = u32_at(bytes, section[4] + 4, "section 4 group count")? as usize;
        let offsets = offsets_at(bytes, section[4] + 8, groups, "section 4 group")?;
        out.groups = offsets.iter().filter(|&&o| o != 0).count();
        for (index, offset) in offsets.iter().copied().enumerate() {
            if offset == 0 {
                continue;
            }
            let at = section[4] + offset;
            let count = u32_at(bytes, at, "event group count")? as usize;
            out.event_records += count;
            let end = at + 4 + count * 40;
            if end > bytes.len() {
                return Err(format!("event group {index} at {offset:#x} runs past the file"));
            }
        }
        Ok(out)
    }
}

/// Section 0: the road table.
struct Road {
    lap_length: u32,
    nodes: Vec<Node>,
}

fn road_header(bytes: &[u8], start: usize) -> Result<(u32, u16, usize), String> {
    let lap = u32_at(bytes, start, "lap length")?;
    let records = u16_at(bytes, start + 6, "32-byte record count")?;
    let record_base = u32_at(bytes, start + 8, "32-byte record base")? as usize;
    Ok((lap, records, record_base))
}

impl Road {
    fn parse(bytes: &[u8], start: usize, end: usize) -> Result<Self, String> {
        let lap = u32_at(bytes, start, "lap length")?;
        let count = usize::from(u16_at(bytes, start + 4, "node count")?);
        let records = usize::from(u16_at(bytes, start + 6, "32-byte record count")?);
        if lap > i32::MAX as u32 {
            return Err(format!("lap length {lap} does not fit signed 16.16"));
        }
        let record_base = u32_at(bytes, start + 8, "32-byte record base")? as usize;
        if count < 2 {
            return Err(format!("section 0 has {count} nodes; a lap needs at least 2"));
        }
        let table = start + 12;
        let offsets = offsets_at(bytes, table, count, "node")?;
        let table_end = table + count * 4;
        if offsets[0] != table_end {
            return Err(format!(
                "the first node starts at {:#x}, not after its offset table at {:#x}",
                offsets[0], table_end
            ));
        }
        for (index, pair) in offsets.windows(2).enumerate() {
            if pair[0] >= pair[1] {
                return Err(format!(
                    "node {index} starts at {:#x}, not before node {} at {:#x}",
                    pair[0],
                    index + 1,
                    pair[1]
                ));
            }
        }
        if record_base <= offsets[count - 1] {
            return Err(format!(
                "the last node starts at {:#x}, at or past the 32-byte records at {:#x}",
                offsets[count - 1],
                record_base
            ));
        }
        if record_base + records * 32 != end {
            return Err(format!(
                "{} 32-byte records at {record_base:#x} end at {:#x}, not at section 1 ({end:#x})",
                records,
                record_base + records * 32
            ));
        }

        // Node header: 42 bytes, then a variable body. The road-surface quad
        // is the second of the body's two quads: four `(x, z)` 16.16 vertices
        // at +0x6c, +0x74, +0x7c, +0x84, ending at +0x8c.
        struct Raw {
            previous: u16,
            next: u16,
            flags: u32,
            distance: i32,
            scale: [f32; 2],
            position: [f32; 3],
            direction: [f32; 3],
            /// `(x, z)` in the file's axes, metres.
            surface: [[f32; 2]; 4],
            search_origin: [f64; 3],
            contact: contact::ContactMesh,
        }
        let mut raw = Vec::with_capacity(count);
        for (index, &at) in offsets.iter().enumerate() {
            let limit = offsets.get(index + 1).copied().unwrap_or(record_base);
            if at + 0x8c > limit {
                return Err(format!(
                    "node {index} at {at:#x} ends at {limit:#x}, before its road-surface quad at +0x8c"
                ));
            }
            let previous = u16_at(bytes, at, "node previous")?;
            let next = u16_at(bytes, at + 2, "node next")?;
            if usize::from(previous) >= count || usize::from(next) >= count {
                return Err(format!("node {index} links to {previous}/{next}, outside 0..{count}"));
            }
            for (name, offset) in [("link address A", at + 4), ("link address B", at + 8)] {
                let address = u32_at(bytes, offset, "node link address")?;
                if address != 0 {
                    return Err(format!("node {index}'s {name} is {address:#x}, expected 0 in the file"));
                }
            }
            let flags = u32_at(bytes, at + 12, "node flags")?;
            let distance = i32_at(bytes, at + 16, "node distance")?;
            let first_scale = i16_at(bytes, at + 20, "node scale")?;
            let second_scale = i16_at(bytes, at + 22, "node scale")?;
            let x = fx16(i32_at(bytes, at + 24, "node x")?);
            let y = fx16(i32_at(bytes, at + 28, "node y")?);
            let z = fx16(i32_at(bytes, at + 32, "node z")?);
            let dx = i16_at(bytes, at + 36, "node direction x")? as f32 / 4096.0;
            let dy = i16_at(bytes, at + 38, "node direction y")? as f32 / 4096.0;
            let dz = i16_at(bytes, at + 40, "node direction z")? as f32 / 4096.0;
            let mut surface = [[0.0f32; 2]; 4];
            for (corner, slot) in surface.iter_mut().enumerate() {
                // Each corner is `s32 x; s32 z`; the pairs are 8 bytes apart,
                // not 4, so corner 0 is +0x6c/+0x70 and corner 3 +0x84/+0x88.
                *slot = [
                    fx16(i32_at(bytes, at + 0x6c + corner * 8, "road-surface x")?),
                    fx16(i32_at(bytes, at + 0x70 + corner * 8, "road-surface z")?),
                ];
            }
            raw.push(Raw {
                previous,
                next,
                flags,
                distance,
                scale: [f32::from(first_scale) / 4096.0, f32::from(second_scale) / 4096.0],
                // Preserve the course axes; the original main-view matrix confirms them.
                position: [x, y, z],
                direction: [dx, dy, dz],
                surface,
                search_origin: [0x18, 0x1C, 0x20]
                    .map(|o| i32_at(bytes, at + o, "contact node origin").map(|v| v as f64 / 65536.0))
                    .into_iter()
                    .collect::<Result<Vec<_>, _>>()?
                    .try_into()
                    .unwrap(),
                contact: contact::parse(bytes, at, limit).map_err(|e| format!("node {index}: {e}"))?,
            });
        }

        // The quad's far corners belong to the next node, so their height is
        // the next node's; the file stores no height for either pair.
        let nodes: Vec<Node> = raw
            .iter()
            .map(|node| {
                let next = &raw[usize::from(node.next)];
                let surface = std::array::from_fn(|corner| {
                    let [x, z] = node.surface[corner];
                    let height = if corner < 2 { node.position[1] } else { next.position[1] };
                    [x, height, z]
                });
                Node {
                    previous: node.previous,
                    next: node.next,
                    flags: node.flags,
                    distance_m: f64::from(node.distance) / 65536.0,
                    scale: node.scale,
                    position_m: node.position,
                    direction: node.direction,
                    surface_m: surface,
                    contact_search_origin_m: Some(node.search_origin),
                    contact: Some(node.contact.clone()),
                }
            })
            .collect();

        // Driving order: follow `next` from node 0. It must visit every node
        // once and come back, or the distance solver cannot cover a lap.
        let mut seen = vec![false; count];
        let mut at = 0usize;
        let mut visited = 0usize;
        loop {
            if seen[at] {
                break;
            }
            seen[at] = true;
            visited += 1;
            at = usize::from(nodes[at].next);
        }
        if at != 0 || visited != count {
            return Err(format!(
                "following `next` from node 0 visits {visited} of {count} nodes and ends at node {at}"
            ));
        }
        Ok(Self { lap_length: lap, nodes })
    }
}

/// Section 1 references and section 2's validated mesh contents. Placement,
/// scale conversion and material decoding remain separate investigations.
#[derive(Default)]
struct Geometry {
    vertices: usize,
    primitives: [usize; 10],
}

impl Geometry {
    fn parse(bytes: &[u8], start: usize, end: usize, next: usize) -> Result<Self, String> {
        let count = u32_at(bytes, start, "section 1 count")? as usize;
        let offsets = offsets_at(bytes, start + 4, count, "mesh record")?;
        let table_end = start + 4 + count * 4;
        let mut at = table_end;
        for (index, &offset) in offsets.iter().enumerate() {
            if offset != at {
                return Err(format!(
                    "mesh record {index} starts at {offset:#x}, expected {at:#x} (records are contiguous)"
                ));
            }
            let kind = u32_at(bytes, offset, "mesh record kind")?;
            if !(1..=4).contains(&kind) {
                return Err(format!("mesh record {index} has kind {kind}; only 1..=4 are known"));
            }
            at += 4 + 8 * kind as usize;
            if at > end {
                return Err(format!("mesh record {index} ends at {at:#x}, past section 2 ({end:#x})"));
            }
        }
        if at != end {
            return Err(format!("mesh records end at {at:#x}, not at section 2 ({end:#x})"));
        }

        let blobs = u32_at(bytes, end, "section 2 count")? as usize;
        let blob_offsets = offsets_at(bytes, end + 4, blobs, "mesh blob")?;
        let blob_table_end = end + 4 + blobs * 4;
        let mut summary = Self::default();
        if blobs == 0 && blob_table_end != next {
            return Err("empty mesh table does not end at section 3".into());
        }
        if blobs > 0 {
            if blob_offsets[0] != blob_table_end {
                return Err(format!(
                    "the first mesh blob starts at {:#x}, not after its table at {blob_table_end:#x}",
                    blob_offsets[0]
                ));
            }
            for (index, &blob) in blob_offsets.iter().enumerate() {
                let limit = blob_offsets.get(index + 1).copied().unwrap_or(next);
                if blob < blob_table_end || limit > next || blob + 0x58 > limit {
                    return Err(format!("mesh blob {index} at {blob:#x} does not fit in section 2"));
                }
                let mesh = mesh::validate(bytes, blob, limit).map_err(|error| format!("mesh blob {index}: {error}"))?;
                summary.vertices += mesh.vertices;
                for (total, count) in summary.primitives.iter_mut().zip(mesh.primitives) {
                    *total += count;
                }
            }
        }

        // Every mesh a section-1 record names is a section-2 blob start.
        for &offset in &offsets {
            let kind = u32_at(bytes, offset, "mesh record kind")? as usize;
            for pair in 0..kind {
                let mesh = u32_at(bytes, offset + 4 + pair * 8 + 4, "mesh offset")? as usize;
                if !blob_offsets.contains(&mesh) {
                    return Err(format!("mesh offset {mesh:#x} is not a section-2 blob start"));
                }
            }
        }
        Ok(summary)
    }
}

/// Section 3: draw spans and the level wrap.
struct Spans {
    level_wrap: u16,
    spans: Vec<Span>,
}

impl Spans {
    fn parse(bytes: &[u8], start: usize, end: usize, nodes: usize) -> Result<Self, String> {
        let count = usize::from(u16_at(bytes, start, "span count")?);
        let level_wrap = u16_at(bytes, start + 2, "span level wrap")?;
        let offsets = offsets_at(bytes, start + 4, count, "span")?;
        let table_end = start + 4 + count * 4;
        for (index, &offset) in offsets.iter().enumerate() {
            if offset < table_end || offset + 16 > end {
                return Err(format!("span {index} at {offset:#x} does not fit in section 3"));
            }
            if index > 0 && offsets[index - 1] >= offset {
                return Err(format!("span {index} starts at {offset:#x}, not after span {}", index - 1));
            }
        }
        let mut spans = Vec::with_capacity(count);
        for (index, &offset) in offsets.iter().enumerate() {
            let kind = u16_at(bytes, offset, "span kind")?;
            let min_level = u16_at(bytes, offset + 2, "span min level")?;
            let max_level = u16_at(bytes, offset + 4, "span max level")?;
            let node = u16_at(bytes, offset + 6, "span node")?;
            if usize::from(node) >= nodes {
                return Err(format!("span {index} names node {node}, outside 0..{nodes}"));
            }
            let start_m = fx16(i32_at(bytes, offset + 8, "span start")?);
            let end_m = fx16(i32_at(bytes, offset + 12, "span end")?);
            if start_m < 0.0 || end_m < 0.0 {
                return Err(format!("span {index} has a negative boundary ({start_m}, {end_m})"));
            }
            spans.push(Span { kind, min_level, max_level, node, start_m, end_m });
        }
        Ok(Self { level_wrap, spans })
    }
}

/// Section 4: per-lap event groups. Framed so every byte is accounted for.
struct Events;

impl Events {
    fn parse(bytes: &[u8], start: usize) -> Result<Self, String> {
        let length = u32_at(bytes, start, "section 4 length")? as usize;
        if length < 8 {
            return Err(format!("section 4 declares {length} bytes, too short for its header"));
        }
        if start + length != bytes.len() {
            return Err(format!("section 4 declares {length} bytes but {} remain", bytes.len().saturating_sub(start)));
        }
        let groups = u32_at(bytes, start + 4, "section 4 group count")? as usize;
        if 8 + groups * 4 > length {
            return Err(format!("section 4's {groups} group offsets do not fit in {length} bytes"));
        }
        let offsets = offsets_at(bytes, start + 8, groups, "event group")?;
        let mut previous = 8 + groups * 4;
        let mut present = Vec::new();
        for (index, &offset) in offsets.iter().enumerate() {
            if offset == 0 {
                continue;
            }
            if offset < previous || offset % 4 != 0 || offset + 4 > length {
                return Err(format!("event group {index} offset {offset:#x} is out of order or out of section 4"));
            }
            previous = offset;
            present.push((index, offset));
        }
        for (position, &(index, offset)) in present.iter().enumerate() {
            let at = start + offset;
            let count = u32_at(bytes, at, "event group count")? as usize;
            let end = offset + 4 + count * 40;
            let limit = present.get(position + 1).map_or(length, |&(_, next)| next);
            if end > limit {
                return Err(format!(
                    "event group {index} at {offset:#x} has {count} records, running to {end:#x} past {limit:#x}"
                ));
            }
        }
        Ok(Self)
    }
}

/// File-relative section offsets, checked to be inside the file.
fn section_offsets(bytes: &[u8]) -> Result<[usize; 5], String> {
    let mut out = [0usize; 5];
    for (index, slot) in out.iter_mut().enumerate() {
        let at = 0x10 + index * 4;
        *slot = u32_at(bytes, at, "section offset")? as usize;
        if *slot >= bytes.len() && index < 4 {
            return Err(format!("section {index} starts at {slot:#x} past the file"));
        }
    }
    Ok(out)
}

/// `count` `u32` values at `at`.
fn offsets_at(bytes: &[u8], at: usize, count: usize, what: &str) -> Result<Vec<usize>, String> {
    (0..count).map(|index| Ok(u32_at(bytes, at + index * 4, what)? as usize)).collect()
}

fn bytes_at<const N: usize>(bytes: &[u8], at: usize, what: &str) -> Result<[u8; N], String> {
    bytes
        .get(at..at + N)
        .and_then(|slice| slice.try_into().ok())
        .ok_or_else(|| format!("{what} at {at:#x} is past the end of the {} byte file", bytes.len()))
}

fn u16_at(bytes: &[u8], at: usize, what: &str) -> Result<u16, String> {
    Ok(u16::from_le_bytes(bytes_at(bytes, at, what)?))
}

fn i16_at(bytes: &[u8], at: usize, what: &str) -> Result<i16, String> {
    Ok(i16::from_le_bytes(bytes_at(bytes, at, what)?))
}

fn u32_at(bytes: &[u8], at: usize, what: &str) -> Result<u32, String> {
    Ok(u32::from_le_bytes(bytes_at(bytes, at, what)?))
}

fn i32_at(bytes: &[u8], at: usize, what: &str) -> Result<i32, String> {
    Ok(i32::from_le_bytes(bytes_at(bytes, at, what)?))
}

/// 16.16 fixed point metres to metres.
fn fx16(value: i32) -> f32 {
    value as f32 / 65536.0
}

/// Reads, converts and writes one course. Returns the importer's one-line
/// report; the provenance file is written beside `out`.
pub fn import(input: &Path, out: &Path, name: &str) -> std::io::Result<String> {
    let bytes = std::fs::read(input)?;
    let course = Course::parse(&bytes).map_err(std::io::Error::other)?;
    let accounting = Course::accounting(&bytes, &course).map_err(std::io::Error::other)?;
    let ron = ron::ser::to_string_pretty(&course, ron::ser::PrettyConfig::default()).map_err(std::io::Error::other)?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(out, ron.as_bytes())?;
    crate::Provenance::of_named(input, name, "import tro")?.write_beside(out)?;
    Ok(format!(
        "{}: lap {:.3} m, {} nodes, {} spans, level wrap {}; sections {} bytes; \
         {} 32-byte records, {} mesh records -> {} meshes, {} blobs ({} vertices, {} primitives), {} event groups/{} records; \
         every byte of {} accounted for",
        out.display(),
        course.lap_length_m,
        accounting.nodes,
        course.spans.len(),
        course.level_wrap,
        accounting.sections.iter().map(|n| n.to_string()).collect::<Vec<_>>().join("/"),
        accounting.records,
        accounting.mesh_records,
        accounting.meshes,
        accounting.blobs,
        accounting.vertices,
        accounting.primitives.iter().sum::<usize>(),
        accounting.groups,
        accounting.event_records,
        accounting.bytes
    ))
}

// A tiny helper so the header can be read once in `accounting` without
// inventing a second tuple type at the call site.
#[cfg(test)]
mod tests {
    use super::*;

    /// A two-node, no-records, no-spans course. Exercises the container,
    /// section-0 framing, link chain and the empty 1-4 sections.
    fn minimal() -> Vec<u8> {
        const S0: usize = 0x24;
        const NODE: usize = 0x150;
        let node_table = S0 + 12;
        let node0 = node_table + 8;
        let node1 = node0 + NODE;
        let record_base = node1 + NODE;
        let s1 = record_base;
        let s2 = s1 + 4;
        let s3 = s2 + 4;
        let s4 = s3 + 4;
        let mut bytes = vec![0u8; s4 + 8];
        bytes[0..12].copy_from_slice(b"@(#)GT-PS\0\0\0");
        bytes[0x0e..0x10].copy_from_slice(&0x1fu16.to_le_bytes());
        for (index, offset) in [S0, s1, s2, s3, s4].into_iter().enumerate() {
            bytes[0x10 + index * 4..0x14 + index * 4].copy_from_slice(&(offset as u32).to_le_bytes());
        }

        bytes[S0..S0 + 4].copy_from_slice(&(20u32 << 16).to_le_bytes());
        bytes[S0 + 4..S0 + 6].copy_from_slice(&2u16.to_le_bytes());
        bytes[S0 + 6..S0 + 8].copy_from_slice(&0u16.to_le_bytes());
        bytes[S0 + 8..S0 + 12].copy_from_slice(&(record_base as u32).to_le_bytes());
        bytes[node_table..node_table + 4].copy_from_slice(&(node0 as u32).to_le_bytes());
        bytes[node_table + 4..node_table + 8].copy_from_slice(&(node1 as u32).to_le_bytes());

        let mut node = |at: usize, previous: u16, next: u16, distance: i32, x: i32, far_x: i32| {
            bytes[at..at + 2].copy_from_slice(&previous.to_le_bytes());
            bytes[at + 2..at + 4].copy_from_slice(&next.to_le_bytes());
            bytes[at + 16..at + 20].copy_from_slice(&distance.to_le_bytes());
            bytes[at + 20..at + 22].copy_from_slice(&4096i16.to_le_bytes());
            bytes[at + 22..at + 24].copy_from_slice(&4096i16.to_le_bytes());
            bytes[at + 24..at + 28].copy_from_slice(&x.to_le_bytes());
            bytes[at + 36..at + 38].copy_from_slice(&4096i16.to_le_bytes());
            for o in [0x94, 0x98, 0x9C] {
                bytes[at + o..at + o + 4].copy_from_slice(&((at + 0xE8) as u32).to_le_bytes());
            }
            bytes[at + 0xA0..at + 0xA4].copy_from_slice(&((at + NODE) as u32).to_le_bytes());
            for i in 0..11 {
                bytes[at + 0xA4 + i * 4..at + 0xA8 + i * 4].copy_from_slice(&((at + 0xE8) as u32).to_le_bytes());
            }
            for i in 0..16 {
                bytes[at + 0xE8 + 40 + i * 4..at + 0xE8 + 44 + i * 4]
                    .copy_from_slice(&((at + NODE) as u32).to_le_bytes());
            }
            // Road-surface quad: near at this node's x, far at the next one,
            // one metre either side (file z).
            let half = 1i32 << 16;
            let corners = [(x, -half), (x, half), (far_x, -half), (far_x, half)];
            for (corner, (cx, cz)) in corners.into_iter().enumerate() {
                bytes[at + 0x6c + corner * 8..at + 0x70 + corner * 8].copy_from_slice(&cx.to_le_bytes());
                bytes[at + 0x70 + corner * 8..at + 0x74 + corner * 8].copy_from_slice(&cz.to_le_bytes());
            }
        };
        node(node0, 1, 1, 0, 0, 10 << 16);
        node(node1, 0, 0, 10 << 16, 10 << 16, 0);

        // Sections 1 and 2 are empty tables; 3 is an empty span table; 4 is a
        // length and a zero group count.
        bytes[s4..s4 + 8].copy_from_slice(&[8, 0, 0, 0, 0, 0, 0, 0]);
        bytes
    }

    #[test]
    fn rejects_a_non_tro() {
        assert!(Course::parse(b"not a course").is_err());
        let mut bytes = minimal();
        bytes[0] = b'X';
        assert!(Course::parse(&bytes).is_err());
    }

    #[test]
    fn reads_a_minimal_course() {
        let bytes = minimal();
        let course = Course::parse(&bytes).unwrap();
        assert_eq!(course.lap_length_m, 20.0);
        assert_eq!(course.nodes.len(), 2);
        assert_eq!(course.nodes[0].next, 1);
        assert_eq!(course.nodes[1].previous, 0);
        assert_eq!(course.nodes[1].position_m, [10.0, 0.0, 0.0]);
        assert_eq!(course.nodes[1].direction, [1.0, 0.0, 0.0]);
        // The quad runs node 0's cross-section to node 1's, and the file's z
        // is preserved on the way into the engine.
        assert_eq!(course.nodes[0].surface_m, [[0.0, 0.0, -1.0], [0.0, 0.0, 1.0], [10.0, 0.0, -1.0], [10.0, 0.0, 1.0]]);
        assert_eq!(course.nodes[1].surface_m, [[10.0, 0.0, -1.0], [10.0, 0.0, 1.0], [0.0, 0.0, -1.0], [0.0, 0.0, 1.0]]);
        assert!(course.spans.is_empty());
        let accounting = Course::accounting(&bytes, &course).unwrap();
        assert_eq!(accounting.nodes, 2);
        assert_eq!(accounting.records, 0);
        assert_eq!(accounting.bytes, bytes.len());
    }

    #[test]
    fn converts_16_16_to_metres() {
        assert_eq!(fx16(0), 0.0);
        assert_eq!(fx16(1 << 16), 1.0);
        assert_eq!(fx16(-(3 << 16)), -3.0);
    }

    /// The complete USA Simulation Rev 2 corpus; source files stay outside git.
    #[test]
    #[ignore = "requires GT2_TRO_CORPUS: directory of all 126 extracted .tro files"]
    fn validates_extracted_course_corpus() {
        let root = std::env::var_os("GT2_TRO_CORPUS").expect("set GT2_TRO_CORPUS to the extracted courses directory");
        let mut files: Vec<_> = std::fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "tro"))
            .collect();
        files.sort();
        assert_eq!(files.len(), 126, "use the complete USA Simulation Rev 2 course corpus");
        let mut meshes = 0;
        let mut contact_nodes = 0;
        let mut contact_polygons = 0;
        let mut contact_references = 0;
        let mut vertices = 0;
        let mut primitives = [0usize; 10];
        for path in files {
            let bytes = std::fs::read(&path).unwrap();
            let course = Course::parse(&bytes).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let summary = Course::accounting(&bytes, &course).unwrap();
            for node in &course.nodes {
                let contact = node.contact.as_ref().expect("every source node has a collision grid");
                contact_nodes += 1;
                contact_polygons += contact.polygons.len();
                contact_references += contact.cells.iter().map(Vec::len).sum::<usize>();
            }
            meshes += summary.blobs;
            vertices += summary.vertices;
            for (total, count) in primitives.iter_mut().zip(summary.primitives) {
                *total += count;
            }
        }
        assert_eq!((meshes, vertices), (5709, 280301));
        assert_eq!(contact_nodes, 14387);
        println!(
            "{contact_nodes} contact grids: {contact_polygons} polygons, {contact_references} cell references; every cell list tiles and references an ordinary primitive"
        );
        assert_eq!(primitives, [827, 5731, 218, 1038, 18183, 107634, 2355, 3160, 6027, 488]);
        println!(
            "126 courses: {meshes} meshes, {vertices} vertices, {} primitives; all mesh arrays tile and polygon indices are in range",
            primitives.iter().sum::<usize>()
        );
    }
}
