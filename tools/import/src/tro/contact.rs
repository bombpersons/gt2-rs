//! Node-local collision grids referenced by 80027C1C/800279E8.
//! Decode file pointers offline; exported cells contain ordinary polygon ids.
use super::{i16_at, i32_at, u16_at, u32_at};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize)]
pub struct ContactPolygon {
    pub vertices_m: Vec<[f64; 3]>,
    pub surface: u8,
    pub variant: u8,
    pub flags: u8,
}
#[derive(Clone, Debug, Serialize)]
pub struct ContactMesh {
    pub origin_m: [f64; 3],
    pub grid_origin_m: [f64; 2],
    pub cell_size_m: [f64; 2],
    pub dimensions: [usize; 2],
    pub cells: Vec<Vec<usize>>,
    pub polygons: Vec<ContactPolygon>,
    pub horizontal_step_m: f64,
    pub height_step_m: f64,
}

pub fn parse(bytes: &[u8], node: usize, end: usize) -> Result<ContactMesh, String> {
    if node + 0xE8 > end || end > bytes.len() {
        return Err("contact node header exceeds its extent".into());
    }
    let grid = u32_at(bytes, node + 0x9C, "contact grid")? as usize;
    let limit = u32_at(bytes, node + 0xA0, "contact grid end")? as usize;
    let vertices = u32_at(bytes, node + 0xA4, "contact vertices")? as usize;
    let vertex_count = u32_at(bytes, node + 0xD0, "contact vertex count")? as usize;
    if grid < node + 0xE8
        || grid + 104 > limit
        || limit > end
        || vertices < node + 0xE8
        || vertices + vertex_count * 8 > grid
    {
        return Err("contact grid/vertices exceed their node".into());
    }
    // References must name a real ordinary primitive record, never an opaque
    // address into a neighboring node, vertex array or procedural primitive.
    let mut ordinary = BTreeMap::new();
    for (group, stride) in [12, 12, 20, 24, 12, 12, 20, 24].into_iter().enumerate() {
        let start = u32_at(bytes, node + 0xA8 + group * 4, "contact primitive array")? as usize;
        let count = u16_at(bytes, node + 0xD4 + group * 2, "contact primitive count")? as usize;
        if start < vertices + vertex_count * 8 || start + count * stride > grid {
            return Err("contact primitive array exceeds node geometry".into());
        }
        for i in 0..count {
            ordinary.insert(start + i * stride, if group % 2 == 0 { 3 } else { 4 });
        }
    }
    let mut result = ContactMesh {
        origin_m: [0x30, 0x34, 0x38]
            .map(|o| i32_at(bytes, node + o, "contact origin").map(|v| (v & !0x3FFFFF) as f64 / 65536.0))
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?
            .try_into()
            .unwrap(),
        grid_origin_m: [
            i16_at(bytes, grid, "contact grid x")? as f64 / 64.0,
            i16_at(bytes, grid + 2, "contact grid z")? as f64 / 64.0,
        ],
        cell_size_m: [1.0; 2],
        dimensions: [4, 4],
        cells: Vec::with_capacity(16),
        polygons: Vec::new(),
        horizontal_step_m: 1.0 / 64.0,
        height_step_m: 1.0 / 65536.0,
    };
    for i in 0..2 {
        let shift = i16_at(bytes, grid + 4 + i * 2, "contact grid shift")?;
        if !(0..=30).contains(&shift) {
            return Err("invalid contact grid cell shift".into());
        }
        result.cell_size_m[i] = (1u64 << shift) as f64 / 64.0;
    }
    let mut known = BTreeMap::new();
    let mut at = grid + 104;
    for i in 0..16 {
        let count = u16_at(bytes, grid + 8 + i * 2, "contact cell count")? as usize;
        let pointer = u32_at(bytes, grid + 40 + i * 4, "contact cell list")? as usize;
        if pointer != at || at + count * 4 > limit {
            return Err("contact cell lists do not tile their extent".into());
        }
        let mut cell = Vec::with_capacity(count);
        for j in 0..count {
            let polygon = u32_at(bytes, at + j * 4, "contact polygon reference")? as usize;
            let corners = ordinary.get(&polygon).ok_or("contact cell references a non-polygon record")?;
            let id = if let Some(id) = known.get(&polygon) {
                *id
            } else {
                let a = u32_at(bytes, polygon, "contact polygon indices")?;
                let b = u32_at(bytes, polygon + 4, "contact polygon metadata")?;
                if (a & 0x40000000 != 0) != (*corners == 4) {
                    return Err("contact polygon kind disagrees with its primitive array".into());
                }
                let indices = [a & 511, a >> 9 & 511, a >> 18 & 511, b & 511];
                let mut points = Vec::with_capacity(*corners);
                for &index in &indices[..*corners] {
                    if index as usize >= vertex_count {
                        return Err("contact polygon references missing vertex".into());
                    }
                    let v = vertices + index as usize * 8;
                    points.push([
                        i16_at(bytes, v, "contact vertex x")? as f64 / 64.0,
                        i16_at(bytes, v + 4, "contact vertex height")? as f64 / 64.0,
                        i16_at(bytes, v + 2, "contact vertex z")? as f64 / 64.0,
                    ]);
                }
                let id = result.polygons.len();
                result.polygons.push(ContactPolygon {
                    vertices_m: points,
                    surface: (b >> 28) as u8,
                    variant: (b >> 23 & 3) as u8,
                    flags: (b >> 25 & 3) as u8,
                });
                known.insert(polygon, id);
                id
            };
            cell.push(id);
        }
        at += count * 4;
        result.cells.push(cell);
    }
    if at != limit {
        return Err("contact cell lists leave unaccounted bytes".into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Vec<u8>, usize, usize, usize) {
        let node = 16;
        let vertices = node + 0xE8;
        let polygon = vertices + 32;
        let grid = polygon + 12;
        let end = grid + 108;
        let mut bytes = vec![0; end];
        let mut word = |at: usize, v: usize| bytes[at..at + 4].copy_from_slice(&(v as u32).to_le_bytes());
        word(node + 0x9C, grid);
        word(node + 0xA0, end);
        word(node + 0xA4, vertices);
        word(node + 0xD0, 4);
        word(node + 0xA8, polygon);
        word(node + 0xAC, polygon);
        for i in 2..10 {
            word(node + 0xA8 + i * 4, grid);
        }
        word(polygon, 0x40000000 | 1 << 9 | 2 << 18);
        word(polygon + 4, 3 | 2 << 28);
        word(grid + 40, grid + 104);
        for i in 1..16 {
            word(grid + 40 + i * 4, end);
        }
        word(grid + 104, polygon);
        bytes[node + 0xD6..node + 0xD8].copy_from_slice(&1u16.to_le_bytes());
        bytes[grid + 8..grid + 10].copy_from_slice(&1u16.to_le_bytes());
        for i in 0..2 {
            bytes[grid + 4 + i * 2..grid + 6 + i * 2].copy_from_slice(&6i16.to_le_bytes());
        }
        for (i, point) in [[0i16, 0, 0], [0, 64, 0], [64, 64, 16], [64, 0, 16]].into_iter().enumerate() {
            for (axis, v) in point.into_iter().enumerate() {
                bytes[vertices + i * 8 + axis * 2..vertices + i * 8 + axis * 2 + 2].copy_from_slice(&v.to_le_bytes());
            }
        }
        (bytes, node, grid, end)
    }
    #[test]
    fn converts_authored_polygon_grid_and_removes_file_addresses() {
        let (bytes, node, _, end) = fixture();
        let mesh = parse(&bytes, node, end).unwrap();
        assert_eq!(mesh.cells[0], vec![0]);
        assert!(mesh.cells[1..].iter().all(Vec::is_empty));
        assert_eq!(mesh.polygons.len(), 1);
        assert_eq!(mesh.polygons[0].surface, 2);
        assert_eq!(mesh.polygons[0].vertices_m[2], [1.0, 0.25, 1.0]);
        assert_eq!(mesh.cell_size_m, [1.0, 1.0]);
    }
    #[test]
    fn rejects_bad_grid_extents_polygon_kinds_and_references() {
        for corrupt in 0..4 {
            let (mut bytes, node, grid, end) = fixture();
            let (at, value) = match corrupt {
                0 => (grid + 40, (grid + 100) as u32),
                1 => (grid + 104, (node + 0xE8) as u32),
                2 => (grid - 12, 0x40000004 | 1 << 9 | 2 << 18),
                _ => (grid - 12, 1 << 9 | 2 << 18),
            };
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            assert!(parse(&bytes, node, end).is_err(), "corruption {corrupt}");
        }
    }
}
