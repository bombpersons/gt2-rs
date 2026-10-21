//! GT2 CDO/CNO bodies and CDP/CNP indexed paint textures.
//!
//! The storage schema and runtime readers are documented in
//! `docs/car-model-evidence.md`. No addresses or GPU packets escape this module.
use serde::Serialize;

const HEADER: usize = 0x884;
const FACE_STRIDES: [usize; 8] = [16, 16, 24, 28, 28, 28, 36, 40];
const TEXTURE_AT: usize = 0x43a0;
pub const TEXTURE_WIDTH: usize = 256;
pub const TEXTURE_HEIGHT: usize = 224;
pub const PALETTES_PER_PAINT: usize = 18;

#[derive(Debug, Serialize)]
pub struct CarModel {
    pub lods: Vec<CarLod>,
    /// The authored attachment positions in metres, in file order.
    pub wheel_centres: [[f32; 3]; 4],
    /// The body decoder does not interpret the shadow/auxiliary trailer yet.
    pub auxiliary_bytes: usize,
}

#[derive(Debug, Serialize)]
pub struct CarLod {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub faces: Vec<CarFace>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CarFace {
    /// Three triangle or four perimeter corners. A quad splits 0,1,3 / 3,1,2.
    pub vertices: Vec<u16>,
    pub normal_indices: Option<Vec<u16>>,
    /// Flat RGB, or PS1 texture modulation (128 is neutral). Raw-texture
    /// commands are resolved to neutral modulation during decoding.
    pub color: [u8; 3],
    /// Normalized image coordinates, top row first.
    pub uv: Option<Vec<[f32; 2]>>,
    pub palette: Option<usize>,
    pub semi_transparent: bool,
    /// The original optionally adds a view-dependent reflection pass.
    pub reflection: bool,
}

fn word(bytes: &[u8], at: usize) -> Result<u32, String> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| format!("car data truncated at {at:#x}"))
}
fn half(bytes: &[u8], at: usize) -> Result<u16, String> {
    bytes
        .get(at..at + 2)
        .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| format!("car data truncated at {at:#x}"))
}
fn position(bytes: &[u8], at: usize) -> Result<[f32; 3], String> {
    Ok([
        half(bytes, at)? as i16 as f32 / 4096.0,
        half(bytes, at + 2)? as i16 as f32 / 4096.0,
        half(bytes, at + 4)? as i16 as f32 / 4096.0,
    ])
}

pub fn parse(bytes: &[u8]) -> Result<CarModel, String> {
    if word(bytes, 0)? != 0x0002_5447 {
        return Err("not a GT2 CDO/CNO body".into());
    }
    let count = word(bytes, 0x868)? as usize;
    if count != 3 {
        return Err(format!("unsupported car LOD count {count}"));
    }
    let mut wheel_centres = [[0.0; 3]; 4];
    for (i, centre) in wheel_centres.iter_mut().enumerate() {
        let at = 0x20 + i * 8;
        *centre = [
            half(bytes, at + 6)? as i16 as f32 / 4096.0,
            half(bytes, at + 2)? as i16 as f32 / 4096.0,
            half(bytes, at + 4)? as i16 as f32 / 4096.0,
        ];
    }
    let mut at = HEADER;
    let mut lods = Vec::new();
    for lod in 0..count {
        // Optional file offsets are zero in 746 LOD entries in the CDO corpus.
        // The contiguous representation is authoritative, even for those entries.
        if lod > 0 {
            let offset = word(bytes, 0x870 + lod * 8)? as usize;
            if offset != 0 && offset != at {
                return Err(format!("LOD {lod} offset {offset:#x}, expected {at:#x}"));
            }
        }
        let vertices = half(bytes, at)? as usize;
        let normal_count = half(bytes, at + 2)? as usize;
        if vertices > 256 || normal_count > 512 {
            return Err("car vertex/normal count exceeds packed index range".into());
        }
        if word(bytes, at + 20)? != 80 {
            return Err("car vertex offset is not 80".into());
        }
        let exponent = half(bytes, at + 76)?;
        if exponent > 28 {
            return Err(format!("unsupported car scale exponent {exponent}"));
        }
        let scale = 2.0f32.powi(i32::from(exponent) - 16);
        let mut positions = Vec::with_capacity(vertices);
        let minimum = position(bytes, at + 60)?;
        let maximum = position(bytes, at + 68)?;
        for i in 0..vertices {
            let p = position(bytes, at + 80 + i * 8)?;
            if (0..3).any(|axis| p[axis] < minimum[axis] || p[axis] > maximum[axis]) {
                return Err(format!("LOD {lod} vertex {i} exceeds bounds"));
            }
            positions.push(p.map(|coordinate| coordinate * scale));
        }
        let mut cursor = at + 80 + vertices * 8;
        if word(bytes, at + 28)? as usize != cursor - at {
            return Err("car normal offset disagrees with vertex count".into());
        }
        let mut normals = Vec::with_capacity(normal_count);
        for _ in 0..normal_count {
            let packed = word(bytes, cursor)?;
            let mut normal = [0.0; 3];
            for (axis, shift) in [2, 12, 22].into_iter().enumerate() {
                normal[axis] = (((packed >> shift) as i32) << 22 >> 22) as f32;
            }
            let length = normal.iter().map(|v| v * v).sum::<f32>().sqrt();
            if length > 0.0 {
                for v in &mut normal {
                    *v /= length;
                }
            }
            normals.push(normal);
            cursor += 4;
        }
        let mut faces = Vec::new();
        for (group, stride) in FACE_STRIDES.into_iter().enumerate() {
            let face_count = half(bytes, at + 4 + group * 2)? as usize;
            // The shipped library uses only flat triangles/quads and textured
            // triangles/quads. Fail explicitly if a new resource uses another kind.
            if face_count > 0 && ![0, 1, 4, 5].contains(&group) {
                return Err(format!("unsupported car primitive group {group}"));
            }
            if group < 6 && {
                let offset = word(bytes, at + 32 + group * 4)? as usize;
                offset != cursor - at && !(face_count == 0 && offset == 0)
            } {
                return Err(format!("car group {group} offset disagrees with preceding arrays"));
            }
            for _ in 0..face_count {
                let raw = bytes.get(cursor..cursor + stride).ok_or("car primitive runs past file")?;
                let corners = if group % 2 == 0 { 3 } else { 4 };
                let ids: Vec<u16> = raw[..corners].iter().map(|&n| u16::from(n)).collect();
                if ids.iter().any(|&n| usize::from(n) >= vertices) {
                    return Err("car face vertex index out of range".into());
                }
                let flags = word(raw, 4)?;
                let references = word(raw, 8)?;
                let reflection = flags & 0x8000_0000 != 0;
                let normal_indices = if reflection {
                    let indices = [
                        (flags >> 5) & 511,
                        (references >> 1) & 511,
                        (references >> 10) & 511,
                        (references >> 19) & 511,
                    ];
                    if indices[..corners].iter().any(|&n| n as usize >= normal_count) {
                        return Err("car face normal index out of range".into());
                    }
                    Some(indices[..corners].iter().map(|&n| n as u16).collect())
                } else {
                    None
                };
                let textured = group >= 4;
                let (uv, palette) = if textured {
                    if half(raw, 22)? != 0 {
                        return Err("car texture references an unsupported page".into());
                    }
                    let clut = half(raw, 18)? as usize;
                    let palette = (clut >> 6) * 4 + (clut & 63);
                    if palette >= PALETTES_PER_PAINT {
                        return Err("car palette index out of range".into());
                    }
                    let mut uv = Vec::new();
                    for offset in [16, 20, 24, 26].into_iter().take(corners) {
                        if raw[offset + 1] as usize >= TEXTURE_HEIGHT {
                            return Err("car texture V exceeds image".into());
                        }
                        uv.push([
                            raw[offset] as f32 / TEXTURE_WIDTH as f32,
                            raw[offset + 1] as f32 / TEXTURE_HEIGHT as f32,
                        ]);
                    }
                    (Some(uv), Some(palette))
                } else {
                    (None, None)
                };
                let opcode = raw[15];
                faces.push(CarFace {
                    vertices: ids,
                    normal_indices,
                    color: if textured && opcode & 1 != 0 { [128; 3] } else { [raw[12], raw[13], raw[14]] },
                    uv,
                    palette,
                    semi_transparent: opcode & 2 != 0,
                    reflection,
                });
                cursor += stride;
            }
        }
        lods.push(CarLod { positions, normals, faces });
        at = cursor;
    }
    Ok(CarModel { lods, wheel_centres, auxiliary_bytes: bytes.len().checked_sub(at).ok_or("car mesh runs past file")? })
}

#[derive(Debug)]
pub struct CarTexture {
    pub paint_names: Vec<u8>,
    /// Palette RGBA values, paint-major then palette-major.
    pub colors: Vec<[[u8; 4]; 16]>,
    pub indices: Vec<u8>,
}

pub fn parse_texture(bytes: &[u8]) -> Result<CarTexture, String> {
    if bytes.len() != TEXTURE_AT + TEXTURE_WIDTH * TEXTURE_HEIGHT / 2 {
        return Err(format!("car paint texture has {} bytes, expected 45984", bytes.len()));
    }
    let paints = half(bytes, 0)? as usize;
    if !(1..=30).contains(&paints) {
        return Err(format!("car texture paint count {paints} exceeds slots"));
    }
    let mut colors = Vec::new();
    for paint in 0..paints {
        for palette in 0..PALETTES_PER_PAINT {
            let mut row = [[0; 4]; 16];
            for (i, color) in row.iter_mut().enumerate() {
                let word = half(bytes, 32 + ((paint * PALETTES_PER_PAINT + palette) * 16 + i) * 2)?;
                let expand = |v: u16| {
                    let v = (v & 31) as u8;
                    (v << 3) | (v >> 2)
                };
                *color = [
                    expand(word),
                    expand(word >> 5),
                    expand(word >> 10),
                    if word == 0 {
                        0
                    } else if word & 0x8000 != 0 {
                        128
                    } else {
                        255
                    },
                ];
            }
            colors.push(row);
        }
    }
    let indices = bytes[TEXTURE_AT..].iter().flat_map(|&b| [b & 15, b >> 4]).collect();
    Ok(CarTexture { paint_names: bytes[2..2 + paints].to_vec(), colors, indices })
}

impl CarTexture {
    pub fn rgba(&self, paint: usize, palette: usize) -> Result<Vec<u8>, String> {
        if paint >= self.paint_names.len() || palette >= PALETTES_PER_PAINT {
            return Err("car paint/palette selection out of range".into());
        }
        let colors = &self.colors[paint * PALETTES_PER_PAINT + palette];
        Ok(self.indices.iter().flat_map(|&index| colors[index as usize]).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn texture_rejects_invalid_paints_and_length() {
        let mut bytes = vec![0; 45984];
        assert!(parse_texture(&bytes).is_err());
        bytes[0] = 31;
        assert!(parse_texture(&bytes).is_err());
        bytes[0] = 1;
        assert_eq!(parse_texture(&bytes).unwrap().indices.len(), 256 * 224);
        bytes.pop();
        assert!(parse_texture(&bytes).is_err());
    }

    #[test]
    fn native_colours_keep_flat_rgb_and_ps1_texture_modulation_ranges() {
        let face = CarFace {
            vertices: vec![0, 0, 0],
            normal_indices: None,
            color: [128; 3],
            uv: Some(vec![[0.0; 2]; 3]),
            palette: Some(0),
            semi_transparent: false,
            reflection: false,
        };
        let flat = CarFace { palette: None, uv: None, color: [255; 3], ..face.clone() };
        let model = CarModel {
            lods: vec![CarLod { positions: vec![[0.0; 3]], normals: vec![], faces: vec![face, flat] }],
            wheel_centres: [[0.0; 3]; 4],
            auxiliary_bytes: 0,
        };
        let paths = [(0, "handmade.png".to_owned())].into_iter().collect();
        let native = geometry(&model, 0, &paths).unwrap();
        assert!(native.parts.iter().all(|part| part.colors == vec![[1.0; 4]; 3]));
    }
    fn tiny_body() -> Vec<u8> {
        let mut bytes = vec![0; HEADER + 3 * 108];
        bytes[..4].copy_from_slice(&0x0002_5447u32.to_le_bytes());
        bytes[0x868..0x86c].copy_from_slice(&3u32.to_le_bytes());
        for lod in 0..3 {
            let at = HEADER + lod * 108;
            bytes[at..at + 2].copy_from_slice(&1u16.to_le_bytes());
            bytes[at + 2..at + 4].copy_from_slice(&1u16.to_le_bytes());
            bytes[at + 4..at + 6].copy_from_slice(&1u16.to_le_bytes());
            for (offset, value) in
                [(20, 80u32), (28, 88), (32, 92), (36, 108), (40, 108), (44, 108), (48, 108), (52, 108)]
            {
                bytes[at + offset..at + offset + 4].copy_from_slice(&value.to_le_bytes());
            }
            bytes[at + 76..at + 78].copy_from_slice(&16u16.to_le_bytes());
            bytes[at + 88..at + 92].copy_from_slice(&(500u32 << 22).to_le_bytes());
            bytes[at + 96..at + 100].copy_from_slice(&0x8000_0000u32.to_le_bytes());
            bytes[at + 104..at + 108].copy_from_slice(&0x20ff_ffffu32.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn zero_lod_offsets_follow_contiguous_storage() {
        let model = parse(&tiny_body()).unwrap();
        assert_eq!(model.lods.len(), 3);
        assert_eq!(model.lods[0].normals[0], [0.0, 0.0, 1.0]);
        assert_eq!(model.lods[0].faces[0].vertices, vec![0, 0, 0]);
        assert_eq!(model.auxiliary_bytes, 0);
    }

    #[test]
    fn rejects_bad_geometry_references_and_offsets() {
        let mut bytes = tiny_body();
        bytes[HEADER + 92] = 1;
        assert!(parse(&bytes).unwrap_err().contains("vertex index"));
        let mut bytes = tiny_body();
        bytes[HEADER + 96] = 32;
        assert!(parse(&bytes).unwrap_err().contains("normal index"));
        let mut bytes = tiny_body();
        bytes[HEADER + 80] = 1;
        assert!(parse(&bytes).unwrap_err().contains("exceeds bounds"));
        let mut bytes = tiny_body();
        bytes[HEADER + 32] = 91;
        assert!(parse(&bytes).unwrap_err().contains("offset"));
    }

    #[test]
    fn rejects_truncated_model() {
        assert!(parse(b"GT\x02\0").is_err());
    }

    #[test]
    #[ignore = "requires an independently extracted local car library"]
    fn car_corpus() {
        let root = std::env::var("GT2_CAR_CORPUS").expect("GT2_CAR_CORPUS");
        let mut counts = [0; 4];
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            let bytes = std::fs::read(&path).unwrap();
            match path.extension().and_then(|s| s.to_str()) {
                Some("cdo" | "cno") => {
                    let model = parse(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                    counts[0] += 1;
                    counts[1] += model.lods.len();
                    counts[2] += model.lods.iter().map(|m| m.faces.len()).sum::<usize>();
                }
                Some("cdp" | "cnp") => {
                    let texture = parse_texture(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                    assert_eq!(texture.rgba(0, 0).unwrap().len(), 256 * 224 * 4);
                    counts[3] += 1;
                }
                _ => (),
            }
        }
        assert!(counts[0] > 0 && counts[3] > 0);
        eprintln!("body files {}, LODs {}, faces {}, texture files {}", counts[0], counts[1], counts[2], counts[3]);
    }
}

// Kept in sync with engine::geometry's authorable asset schema. The offline
// importer deliberately does not depend on the game or Bevy renderer.
#[derive(Debug, Serialize)]
struct GeometryData {
    parts: Vec<MeshPart>,
}
#[derive(Debug, Serialize)]
struct MeshPart {
    name: String,
    texture: Option<String>,
    blend: SurfaceBlend,
    double_sided: bool,
    positions_m: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    triangles: Vec<[u32; 3]>,
}
#[derive(Debug, Serialize)]
enum SurfaceBlend {
    Opaque,
    Half,
}

fn geometry(
    model: &CarModel,
    lod: usize,
    image_paths: &std::collections::BTreeMap<usize, String>,
) -> Result<GeometryData, String> {
    let source = model.lods.get(lod).ok_or_else(|| format!("car LOD {lod} does not exist"))?;
    let mut groups = std::collections::BTreeMap::<(Option<usize>, bool), MeshPart>::new();
    for face in &source.faces {
        let part = groups.entry((face.palette, face.semi_transparent)).or_insert_with(|| MeshPart {
            name: format!(
                "Body LOD {lod} palette {:?}{}",
                face.palette,
                if face.semi_transparent { " blended" } else { "" }
            ),
            texture: face.palette.and_then(|n| image_paths.get(&n).cloned()),
            blend: if face.semi_transparent { SurfaceBlend::Half } else { SurfaceBlend::Opaque },
            double_sided: false,
            positions_m: Vec::new(),
            uv: Vec::new(),
            colors: Vec::new(),
            triangles: Vec::new(),
        });
        let start = part.positions_m.len() as u32;
        for (corner, &index) in face.vertices.iter().enumerate() {
            part.positions_m.push(source.positions[index as usize]);
            part.uv.push(face.uv.as_ref().map_or([0.0; 2], |uv| uv[corner]));
            // PS1 textured colour 0x80 is neutral; flat colour uses the
            // ordinary byte range. The native shader multiplies encoded RGB.
            let divisor = if face.palette.is_some() { 128.0 } else { 255.0 };
            part.colors.push([
                face.color[0] as f32 / divisor,
                face.color[1] as f32 / divisor,
                face.color[2] as f32 / divisor,
                1.0,
            ]);
        }
        if face.vertices.len() == 3 {
            part.triangles.push([start, start + 1, start + 2]);
        } else {
            // The original emits GPU strip corners 0,1,3,2.
            part.triangles.extend([[start, start + 1, start + 3], [start + 3, start + 1, start + 2]]);
        }
    }
    Ok(GeometryData { parts: groups.into_values().collect() })
}

/// Converts one body LOD and one selected paint to engine-native geometry RON
/// and ordinary PNGs beside it. `texture_prefix` is the engine asset-relative
/// directory containing the PNGs, e.g. "gt2/cars". Source files may be gzipped.
pub fn import(
    model_path: &std::path::Path,
    texture_path: &std::path::Path,
    output: &std::path::Path,
    paint: usize,
    lod: usize,
    texture_prefix: &str,
) -> Result<String, String> {
    use std::io::Read;
    let read = |path: &std::path::Path| -> Result<Vec<u8>, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if bytes.starts_with(&[0x1f, 0x8b]) {
            let mut raw = Vec::new();
            flate2::read::GzDecoder::new(bytes.as_slice()).read_to_end(&mut raw).map_err(|e| e.to_string())?;
            Ok(raw)
        } else {
            Ok(bytes)
        }
    };
    let model = parse(&read(model_path)?)?;
    let texture = parse_texture(&read(texture_path)?)?;
    if paint >= texture.paint_names.len() {
        return Err(format!("paint {paint} does not exist"));
    }
    let selected = model.lods.get(lod).ok_or_else(|| format!("car LOD {lod} does not exist"))?;
    let palettes: std::collections::BTreeSet<_> = selected.faces.iter().filter_map(|face| face.palette).collect();
    let directory = output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let stem = output.file_stem().ok_or("car output has no filename")?.to_string_lossy();
    let mut paths = std::collections::BTreeMap::new();
    for palette in palettes {
        let filename = format!("{stem}-paint{paint}-palette{palette}.png");
        let image_path = directory.join(&filename);
        let writer = std::io::BufWriter::new(std::fs::File::create(&image_path).map_err(|e| e.to_string())?);
        let mut encoder = png::Encoder::new(writer, TEXTURE_WIDTH as u32, TEXTURE_HEIGHT as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&texture.rgba(paint, palette)?).map_err(|e| e.to_string())?;
        crate::Provenance::of(texture_path, "import car paint")
            .and_then(|p| p.write_beside(&image_path))
            .map_err(|e| e.to_string())?;
        let prefix = texture_prefix.trim_matches('/');
        paths.insert(palette, if prefix.is_empty() { filename } else { format!("{prefix}/{filename}") });
    }
    let data = geometry(&model, lod, &paths)?;
    let text = ron::ser::to_string_pretty(&data, ron::ser::PrettyConfig::default()).map_err(|e| e.to_string())?;
    std::fs::write(output, text).map_err(|e| e.to_string())?;
    crate::Provenance::of(model_path, "import car body")
        .and_then(|p| p.write_beside(output))
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "{}: LOD {lod}, paint {paint} (id {}), {} vertices, {} faces, {} material parts; {} auxiliary bytes and reflection/wheel rendering remain unconverted",
        output.display(),
        texture.paint_names[paint],
        selected.positions.len(),
        selected.faces.len(),
        data.parts.len(),
        model.auxiliary_bytes
    ))
}
