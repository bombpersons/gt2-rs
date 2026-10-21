//! Scenery instance lists at file +0x118, consumed by 0x8002002C and
//! 0x8001F7F8. Original fields and matrix conventions stay in this importer.
//! Output is ordinary world-space geometry. See docs/course-mesh-evidence.md.
use super::{
    i16_at, i32_at, mesh,
    render::{Face, NodeMesh},
    section_offsets, u16_at, u32_at,
};

#[derive(Debug)]
pub(super) struct Placement {
    pub list: usize,
    pub index: usize,
    pub model: usize,
    rotation: [i16; 3],
    scale: [f32; 3],
    translation: [f32; 3],
}

/// All 33 lists must tile the region between their pointer table and section
/// 0 exactly. Empty lists still have a four-byte zero count. 0x8001F858..868
/// computes instance addresses as list + 4 + index*28.
pub(super) fn parse(bytes: &[u8]) -> Result<Vec<Placement>, String> {
    let sections = section_offsets(bytes)?;
    let models = u32_at(bytes, sections[1], "model count")? as usize;
    let mut at = 0x19c;
    let mut out = Vec::new();
    for list in 0..33 {
        let pointer = u32_at(bytes, 0x118 + list * 4, "instance list pointer")? as usize;
        if pointer != at || at + 4 > sections[0] {
            return Err(format!("instance list {list} does not follow the previous list"));
        }
        let count = u32_at(bytes, at, "instance count")? as usize;
        at += 4;
        if count > (sections[0] - at) / 28 {
            return Err(format!("instance list {list} exceeds the container header"));
        }
        for index in 0..count {
            let model = usize::from(u16_at(bytes, at + 6, "instance model")?);
            if model >= models {
                return Err(format!("instance list {list} instance {index}: model {model} outside 0..{models}"));
            }
            let mut rotation = [0; 3];
            let mut scale = [0.0; 3];
            let mut translation = [0.0; 3];
            for axis in 0..3 {
                rotation[axis] = i16_at(bytes, at + axis * 2, "instance angle")?;
                scale[axis] = f32::from(i16_at(bytes, at + 8 + axis * 2, "instance scale")?) / 4096.0;
                translation[axis] = i32_at(bytes, at + 16 + axis * 4, "instance translation")? as f32 / 65536.0;
            }
            // +0x0e is a separate LOD-distance divisor, not a fourth axis.
            if i16_at(bytes, at + 14, "instance LOD scale")? <= 0 {
                return Err(format!("instance list {list} instance {index}: non-positive LOD scale"));
            }
            out.push(Placement { list, index, model, rotation, scale, translation });
            at += 28;
        }
    }
    if at != sections[0] {
        return Err("instance lists do not end at section 0".into());
    }
    Ok(out)
}

impl Placement {
    /// 0x8001F88C..898 calls 0x80081374 with (-angle1, angle0, angle2).
    /// Its matrix is Ry(-angle1)*Rx(-angle0)*Rz(angle2). 0x8001F8A8..B8
    /// then postmultiplies diag(scale0, scale1, -scale2). The third-axis
    /// reflection is essential; translations are not reflected.
    fn position(&self, local: [f32; 3]) -> [f32; 3] {
        let angles = [i32::from(self.rotation[0]), -i32::from(self.rotation[1]), i32::from(self.rotation[2])];
        let [sx, sy, sz] = angles.map(sine);
        let [cx, cy, cz] = angles.map(|v| sine(v + 1024));
        // 0x80081374 truncates these two products separately, then computes
        // columns with GTE multiplication. Preserve this in the importer.
        let a = (-sy * sx) >> 12;
        let b = (-cy * sx) >> 12;
        let matrix = [
            [(cy * cz + a * sz) >> 12, (-cy * sz + a * cz) >> 12, (sy * cx) >> 12],
            [(cx * sz) >> 12, (cx * cz) >> 12, sx],
            [(-sy * cz + b * sz) >> 12, (sy * sz + b * cz) >> 12, (cy * cx) >> 12],
        ];
        let scales = [self.scale[0], self.scale[1], -self.scale[2]];
        std::array::from_fn(|row| {
            self.translation[row]
                + (0..3)
                    .map(|axis| {
                        // 0x8007B994 postmultiplies the diagonal scale in 12-bit units.
                        let coefficient = (matrix[row][axis] as f32 * scales[axis]).floor() / 4096.0;
                        coefficient * local[axis]
                    })
                    .sum::<f32>()
        })
    }
}

/// Resident table at 0x80093150: all 5,120 entries match this expression.
/// Negative sine values round asymmetrically: sin(-pi/2) is -4095.
/// f64 keeps generated entries on the same side of rounding boundaries.
fn sine(angle: i32) -> i32 {
    let angle = f64::from(angle.rem_euclid(4096)) * std::f64::consts::TAU / 4096.0;
    (angle.sin() * 4096.0 + 0.5) as i32
}

/// Decode the most detailed mesh. Model records contain ordered LOD pairs;
/// 0x8007AEF4 selects the first threshold greater than squared distance.
/// Dynamic LOD and list visibility are not yet exported by this static scene.
pub(super) fn decode_mesh(bytes: &[u8], placement: &Placement) -> Result<NodeMesh, String> {
    let section = section_offsets(bytes)?;
    let model = u32_at(bytes, section[1] + 4 + placement.model * 4, "model pointer")? as usize;
    let start = u32_at(bytes, model + 8, "highest LOD mesh")? as usize;
    let count = u32_at(bytes, section[2], "mesh count")? as usize;
    let offsets = (0..count)
        .map(|i| u32_at(bytes, section[2] + 4 + i * 4, "mesh offset").map(|p| p as usize))
        .collect::<Result<Vec<_>, _>>()?;
    let index = offsets.iter().position(|&p| p == start).ok_or("model does not name a mesh start")?;
    let end = offsets.get(index + 1).copied().unwrap_or(section[3]);
    let summary = mesh::validate(bytes, start, end)?;
    let exponent = i16_at(bytes, start + 0x54, "mesh scale exponent")?;
    if !(16..=26).contains(&exponent) {
        return Err(format!("unsupported scenery scale exponent {exponent}"));
    }
    // 0x8007B8A0/0x8007B778 shift 16.16 translations by exponent-12,
    // while GTE vertices remain signed integers: 2^(exponent-28) m/unit.
    let units = 2.0f32.powi(i32::from(exponent) - 28);
    let vertex_base = u32_at(bytes, start, "mesh vertices")? as usize;
    let mut positions = Vec::new();
    for i in 0..summary.vertices {
        let mut point = [0.0; 3];
        for (axis, value) in point.iter_mut().enumerate() {
            *value = f32::from(i16_at(bytes, vertex_base + i * 8 + axis * 2, "mesh vertex")?) * units;
        }
        positions.push(placement.position(point));
    }
    let mut out = NodeMesh {
        subdivision: None,
        precision: None,
        faces: Vec::new(),
        vertices: summary.vertices,
        polygons: 0,
        procedural: summary.primitives[8..].iter().sum(),
    };
    for (group, stride) in [12, 12, 20, 24, 24, 24, 32, 36].into_iter().enumerate() {
        let pointer = u32_at(bytes, start + 4 + group * 4, "primitive pointer")? as usize;
        for primitive in 0..summary.primitives[group] {
            let at = pointer + primitive * stride;
            let packed = u32_at(bytes, at, "polygon indices")?;
            let mut indices = vec![packed & 1023, packed >> 10 & 1023, packed >> 20 & 1023];
            if group % 2 == 1 {
                indices.push(u32_at(bytes, at + 4, "quad index")? & 1023);
            }
            let textured = group >= 4;
            let gouraud = matches!(group, 2 | 3 | 6 | 7);
            let mut uv = vec![[0; 2]; indices.len()];
            let texture = if textured {
                let a = u32_at(bytes, at + 12, "UV0/CLUT")?;
                let b = u32_at(bytes, at + 16, "UV1/page")?;
                let c = u32_at(bytes, at + 20, "UV2/UV3")?;
                uv = [a as u16, b as u16, c as u16, (c >> 16) as u16]
                    .iter()
                    .take(indices.len())
                    .map(|v| v.to_le_bytes())
                    .collect();
                Some(((b >> 16) as u16, (a >> 16) as u16))
            } else {
                None
            };
            let command = (u32_at(bytes, at + 8, "polygon command")? >> 24) as u8;
            let mut colors = Vec::new();
            for corner in 0..indices.len() {
                let offset = if !gouraud || corner == 0 {
                    8
                } else if textured {
                    20 + corner * 4
                } else {
                    8 + corner * 4
                };
                let rgb = u32_at(bytes, at + offset, "polygon colour")?;
                let divisor = if textured { 128.0 } else { 255.0 };
                colors.push([
                    (rgb & 255) as f32 / divisor,
                    (rgb >> 8 & 255) as f32 / divisor,
                    (rgb >> 16 & 255) as f32 / divisor,
                    1.0,
                ]);
            }
            out.faces.push(Face {
                adaptive: false,
                small_texture: None,
                positions: indices.iter().map(|&i| positions[i as usize]).collect(),
                colors,
                uv,
                texture,
                blend: if command & 2 == 0 { 0 } else { 1 + ((texture.map_or(0, |(page, _)| page) >> 5) & 3) as u8 },
                double_sided: packed & 0x80000000 == 0,
            });
            out.polygons += 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(bytes: &mut [u8], at: usize, value: usize) {
        bytes[at..at + 4].copy_from_slice(&(value as u32).to_le_bytes());
    }

    // Hand-authored storage: one placement, one model, all eight polygon
    // kinds and both procedural arrays. No disc is required by normal tests.
    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0; 0x19c];
        for list in 0..33 {
            let at = bytes.len();
            word(&mut bytes, 0x118 + list * 4, at);
            bytes.extend_from_slice(&(usize::from(list == 0) as u32).to_le_bytes());
            if list == 0 {
                bytes.extend_from_slice(&[0; 28]);
                for i in 0..4 {
                    bytes[at + 12 + i * 2..at + 14 + i * 2].copy_from_slice(&4096u16.to_le_bytes());
                }
            }
        }
        let section0 = bytes.len();
        bytes.extend_from_slice(&[0; 4]);
        let section1 = bytes.len();
        bytes.extend_from_slice(&[0; 20]);
        word(&mut bytes, section1, 1);
        word(&mut bytes, section1 + 4, section1 + 8);
        word(&mut bytes, section1 + 8, 1);
        let section2 = bytes.len();
        bytes.extend_from_slice(&[0; 8]);
        let mesh = bytes.len();
        word(&mut bytes, section1 + 16, mesh);
        word(&mut bytes, section2, 1);
        word(&mut bytes, section2 + 4, mesh);
        bytes.extend_from_slice(&[0; 0x58 + 32]);
        word(&mut bytes, mesh, mesh + 0x58);
        word(&mut bytes, mesh + 0x2c, 4);
        bytes[mesh + 0x54..mesh + 0x56].copy_from_slice(&26u16.to_le_bytes());
        let strides = [12, 12, 20, 24, 24, 24, 32, 36, 28, 20];
        for (group, stride) in strides.into_iter().enumerate() {
            let at = bytes.len();
            word(&mut bytes, mesh + 4 + group * 4, at);
            bytes[mesh + 0x30 + group * 2] = 1;
            bytes.resize(at + stride, 0);
            if group >= 8 {
                continue;
            }
            word(&mut bytes, at, 0xc0000000 | 1 << 10 | 2 << 20);
            word(&mut bytes, at + 4, 0xfffffc03);
            word(&mut bytes, at + 8, 0x2e204080);
            if group >= 4 {
                word(&mut bytes, at + 12, 0x12340201);
                word(&mut bytes, at + 16, 0x00400403);
                word(&mut bytes, at + 20, 0x08070605);
            }
            if matches!(group, 2 | 3 | 6 | 7) {
                for corner in 1..if group % 2 == 0 { 3 } else { 4 } {
                    let offset = if group >= 4 { 20 + corner * 4 } else { 8 + corner * 4 };
                    word(&mut bytes, at + offset, 0x00102030 + corner);
                }
            }
        }
        let end = bytes.len();
        bytes.extend_from_slice(&[0; 4]);
        for (i, section) in [section0, section1, section2, end, end + 4].into_iter().enumerate() {
            word(&mut bytes, 0x10 + i * 4, section);
        }
        bytes
    }

    #[test]
    fn decodes_materials_colours_and_corners_for_every_polygon_kind() {
        let bytes = fixture();
        let placements = parse(&bytes).unwrap();
        assert_eq!(placements.len(), 1);
        let mesh = decode_mesh(&bytes, &placements[0]).unwrap();
        assert_eq!((mesh.vertices, mesh.polygons, mesh.procedural), (4, 8, 2));
        for (group, face) in mesh.faces.iter().enumerate() {
            assert_eq!(face.positions.len(), if group % 2 == 0 { 3 } else { 4 });
            assert!(!face.double_sided);
            assert_eq!(face.blend, if group >= 4 { 3 } else { 1 });
            if group >= 4 {
                assert_eq!(face.texture, Some((0x40, 0x1234)));
                assert_eq!(face.uv[..3], [[1, 2], [3, 4], [5, 6]]);
                if group % 2 == 1 {
                    assert_eq!(face.uv[3], [7, 8]);
                }
                assert_eq!(face.colors[0], [1.0, 0.5, 0.25, 1.0]);
            }
            if matches!(group, 2 | 3 | 6 | 7) {
                let divisor = if group >= 4 { 128.0 } else { 255.0 };
                assert_eq!(face.colors[1], [49.0 / divisor, 32.0 / divisor, 16.0 / divisor, 1.0]);
            }
        }
    }

    #[test]
    fn rejects_bad_instance_extents_model_references_and_lod_divisors() {
        let bytes = fixture();
        for (at, value, message) in [
            (0x118, 0x1a0, "previous list"),
            (0x19c, usize::MAX, "exceeds"),
            (0x1a6, 9, "model"),
            (0x1ae, 0, "LOD scale"),
        ] {
            let mut broken = bytes.clone();
            word(&mut broken, at, value);
            assert!(parse(&broken).unwrap_err().contains(message));
        }
        assert!(parse(&bytes[..0x19f]).is_err());
    }

    #[test]
    fn instance_axes_rotate_scale_and_translate_in_the_original_order() {
        let mut p = Placement {
            list: 0,
            index: 0,
            model: 0,
            rotation: [0; 3],
            scale: [2.0, 3.0, 4.0],
            translation: [10.0, 20.0, 30.0],
        };
        assert_eq!(p.position([1.0, 2.0, 3.0]), [12.0, 26.0, 18.0]);
        p.rotation = [0, 1024, 0];
        let got = p.position([1.0, 2.0, 3.0]);
        for (a, b) in got.into_iter().zip([22.0, 26.0, 32.0]) {
            assert!((a - b).abs() < 0.004);
        }
    }

    #[test]
    #[cfg_attr(not(gt2_scenery_fixture), ignore = "requires locally recorded scenery transforms; see docs/assets.md")]
    fn recorded_transforms_including_holdout_match_the_original_exactly() {
        // 0x8007BA24..6C writes the matrix after scaling to 0x1F80009C.
        // The third case was recorded after quantization was implemented.
        // See tools/record_course_scenery.py; expected data comes from PSX.
        #[derive(serde::Deserialize)]
        struct Case {
            name: String,
            angles: [i16; 3],
            scales: [f32; 3],
            matrix: [[f32; 3]; 3],
        }
        #[derive(serde::Deserialize)]
        struct Recording {
            cases: Vec<Case>,
        }
        let recording: Recording = serde_json::from_str(
            &std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../game/tests/data/original/course/scenery_transforms.json"
            ))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(recording.cases.len(), 3);
        for case in recording.cases {
            let p = Placement {
                list: 0,
                index: 1,
                model: 4,
                rotation: case.angles,
                scale: case.scales.map(|v| v / 4096.0),
                translation: [0.0; 3],
            };
            for axis in 0..3 {
                let mut point = [0.0; 3];
                point[axis] = 1.0;
                for (row, got) in p.position(point).into_iter().enumerate() {
                    assert_eq!(got, case.matrix[row][axis] / 4096.0, "{} row {row} axis {axis}", case.name);
                }
            }
        }
    }

    #[test]
    #[ignore = "requires GT2_TRO_CORPUS: extracted 126-course corpus"]
    fn all_scenery_lists_and_placed_meshes_validate() {
        let root = std::env::var_os("GT2_TRO_CORPUS").expect("set GT2_TRO_CORPUS");
        let mut courses = 0;
        let mut instances = 0;
        for file in std::fs::read_dir(root).unwrap() {
            let path = file.unwrap().path();
            if path.extension().is_none_or(|e| e != "tro") {
                continue;
            }
            let bytes = std::fs::read(&path).unwrap();
            let placements = parse(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            for p in &placements {
                let mesh = decode_mesh(&bytes, p).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                assert!(mesh.faces.iter().flat_map(|f| &f.positions).flatten().all(|v| v.is_finite()));
            }
            courses += 1;
            instances += placements.len();
        }
        assert_eq!((courses, instances), (126, 6084));
    }
}
