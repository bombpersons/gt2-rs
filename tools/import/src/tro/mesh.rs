//! Section-2 mesh storage, corroborated against the race renderer at
//! 0x80019B58 (see docs/course-mesh-evidence.md). Original coordinates and
//! GPU material words stay offline until their conversion is established.
use super::{i16_at, u16_at, u32_at};

// Eight polygon groups, then two groups the renderer expands procedurally.
const STRIDES: [usize; 10] = [12, 12, 20, 24, 24, 24, 32, 36, 28, 20];

#[derive(Debug, Default)]
pub(super) struct Summary {
    pub vertices: usize,
    pub primitives: [usize; 10],
}

/// Validate within one blob, never borrowing bytes from the following mesh.
pub(super) fn validate(bytes: &[u8], start: usize, end: usize) -> Result<Summary, String> {
    if end > bytes.len() || start > end || end - start < 0x58 {
        return Err("mesh header does not fit in its blob".into());
    }
    let mut pointers = [0usize; 11];
    for (index, pointer) in pointers.iter_mut().enumerate() {
        *pointer = u32_at(bytes, start + index * 4, "mesh pointer")? as usize;
    }
    if pointers[0] != start + 0x58 {
        return Err(format!("vertices start at {:#x}, expected {:#x}", pointers[0], start + 0x58));
    }
    let mut out = Summary { vertices: usize::from(u16_at(bytes, start + 0x2c, "vertex count")?), ..Default::default() };
    let mut at = pointers[0];
    let vertex_end = at + out.vertices * 8;
    if vertex_end > end || pointers[1] != vertex_end {
        return Err("vertex count does not fill the vertex array".into());
    }
    let mut minimum = [0i16; 3];
    let mut maximum = [0i16; 3];
    for axis in 0..3 {
        minimum[axis] = i16_at(bytes, start + 0x44 + axis * 2, "mesh minimum")?;
        maximum[axis] = i16_at(bytes, start + 0x4c + axis * 2, "mesh maximum")?;
        if minimum[axis] > maximum[axis] {
            return Err(format!("inverted bounds on axis {axis}"));
        }
    }
    for vertex in 0..out.vertices {
        for axis in 0..3 {
            let value = i16_at(bytes, at + vertex * 8 + axis * 2, "vertex coordinate")?;
            if !(minimum[axis]..=maximum[axis]).contains(&value) {
                return Err(format!("vertex {vertex} axis {axis} lies outside the mesh bounds"));
            }
        }
    }
    at = vertex_end;
    for (group, stride) in STRIDES.into_iter().enumerate() {
        let count = usize::from(u16_at(bytes, start + 0x30 + group * 2, "primitive count")?);
        out.primitives[group] = count;
        let limit = at + count * stride;
        let next = pointers.get(group + 2).copied().unwrap_or(end);
        if pointers[group + 1] != at || limit > end || limit != next {
            return Err(format!("primitive group {group} does not tile its array ({count} records of {stride} bytes)"));
        }
        // Groups 8 and 9 contain positions and sizes, not vertex references.
        if group < 8 {
            for primitive in 0..count {
                let record = at + primitive * stride;
                let packed = u32_at(bytes, record, "packed vertex indices")?;
                let mut indices = [packed & 0x3ff, (packed >> 10) & 0x3ff, (packed >> 20) & 0x3ff, 0];
                let corners = if group % 2 == 0 { 3 } else { 4 };
                if corners == 4 {
                    indices[3] = u32_at(bytes, record + 4, "fourth vertex index")? & 0x3ff;
                }
                for &index in &indices[..corners] {
                    if index as usize >= out.vertices {
                        return Err(format!(
                            "primitive group {group} record {primitive} references vertex {index}, outside 0..{}",
                            out.vertices
                        ));
                    }
                }
            }
        }
        at = limit;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hand-authored, with all ten groups populated and flags deliberately set
    // above the three packed 10-bit indices. No disc content is needed.
    fn fixture() -> (Vec<u8>, usize, usize) {
        let start = 16;
        let end = start + 0x58 + 4 * 8 + STRIDES.iter().sum::<usize>();
        let mut bytes = vec![0u8; end];
        let mut at = start + 0x58;
        let mut word = |at: usize, value: u32| bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        word(start, at as u32);
        word(start + 0x2c, 4);
        at += 4 * 8;
        for (group, stride) in STRIDES.into_iter().enumerate() {
            word(start + 4 + group * 4, at as u32);
            if group < 8 {
                word(at, 0xc000_0000 | 1 << 10 | 2 << 20);
                word(at + 4, 0xffff_fc03);
            } else {
                // These are not index words, even with an empty-looking value.
                word(at, u32::MAX);
            }
            at += stride;
        }
        for group in 0..10 {
            bytes[start + 0x30 + group * 2..start + 0x32 + group * 2].copy_from_slice(&1u16.to_le_bytes());
        }
        (bytes, start, end)
    }

    #[test]
    fn validates_every_group_and_masks_index_flags() {
        let (bytes, start, end) = fixture();
        let summary = validate(&bytes, start, end).unwrap();
        assert_eq!(summary.vertices, 4);
        assert_eq!(summary.primitives, [1; 10]);
    }

    #[test]
    fn rejects_bad_triangle_and_quad_references() {
        for (group, offset) in [(0, 0), (1, 4)] {
            let (mut bytes, start, end) = fixture();
            let at = u32_at(&bytes, start + 4 + group * 4, "test pointer").unwrap() as usize + offset;
            bytes[at..at + 4].copy_from_slice(&4u32.to_le_bytes());
            assert!(validate(&bytes, start, end).unwrap_err().contains("references vertex 4"));
        }
    }

    #[test]
    fn rejects_counts_pointers_and_cross_blob_reads() {
        let (bytes, start, end) = fixture();
        let mut wrong = bytes.clone();
        wrong[start + 0x30] = 2;
        assert!(validate(&wrong, start, end).unwrap_err().contains("group 0"));
        let mut wrong = bytes.clone();
        wrong[start + 4] += 4;
        assert!(validate(&wrong, start, end).unwrap_err().contains("vertex array"));
        assert!(validate(&bytes, start, end - 4).is_err());
        assert!(validate(&bytes, start, start + 0x54).is_err());
    }

    #[test]
    fn rejects_vertices_outside_declared_bounds() {
        let (mut bytes, start, end) = fixture();
        bytes[start + 0x58] = 1;
        assert!(validate(&bytes, start, end).unwrap_err().contains("outside the mesh bounds"));
    }
}
