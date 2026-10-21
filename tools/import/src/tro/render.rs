//! Road-node polygon geometry -> engine::geometry's authorable mesh format.
//! Source references: node renderer 0x80020110, local transform 0x80026BB4,
//! compact polygon reader 0x80023504. Also exports placed section-2 scenery
//! through instances.rs. Procedural groups and model LOD remain open.
use super::{Course, i16_at, i32_at, section_offsets, u16_at, u32_at};
use crate::course_textures::TexturePack;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

const STRIDES: [usize; 10] = [12, 12, 20, 24, 12, 12, 20, 24, 16, 20];

#[derive(Default, Serialize)]
pub struct Geometry {
    pub parts: Vec<Part>,
}

#[derive(Default, Serialize)]
pub struct Part {
    pub name: String,
    pub texture: Option<String>,
    pub blend: Blend,
    pub double_sided: bool,
    pub positions_m: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub triangles: Vec<[u32; 3]>,
    pub quads: Vec<[u32; 4]>,
    pub subdivision: Option<Subdivision>,
    pub precision: Option<Precision>,
    pub small_texture: Option<SmallTexture>,
}

#[derive(Clone, Serialize)]
pub struct Subdivision {
    pub max_twice_area_px2: f32,
    pub max_depth: u8,
    pub uv_step: f32,
    pub guard_band_px: f32,
}

#[derive(Clone, Serialize)]
pub struct Precision {
    pub position_step_m: f32,
    pub centre_m: [f32; 3],
    pub radius_m: f32,
    pub distances_m: [f32; 2],
}

#[derive(Serialize)]
pub struct SmallTexture {
    pub max_twice_area_px2: u32,
    pub texture: String,
    pub blend: Blend,
    pub uv: Vec<[f32; 2]>,
}

#[derive(Clone)]
pub(super) struct FaceSmallTexture {
    pub threshold: u16,
    pub texture: (u16, u16),
    pub uv: Vec<[u8; 2]>,
    pub blend: u8,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub enum Blend {
    #[default]
    Opaque,
    Half,
    Add,
    Subtract,
    AddQuarter,
}

#[derive(Clone)]
pub(super) struct Face {
    pub positions: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub uv: Vec<[u8; 2]>,
    pub texture: Option<(u16, u16)>,
    pub blend: u8,
    pub double_sided: bool,
    pub adaptive: bool,
    pub small_texture: Option<FaceSmallTexture>,
}

pub struct NodeMesh {
    pub(super) faces: Vec<Face>,
    pub vertices: usize,
    pub polygons: usize,
    pub procedural: usize,
    pub subdivision: Option<Subdivision>,
    pub precision: Option<Precision>,
}

/// Every node uses an embedded 68-byte mesh header at +0xa4. Its first
/// eleven words are pointers, then a u32 vertex count and ten u16 counts.
/// The earlier hypothesis that +0x92 was a pointer count was incorrect.
pub fn node_mesh(
    bytes: &[u8],
    node: usize,
    end: usize,
    record_base: usize,
    record_count: usize,
) -> Result<NodeMesh, String> {
    if node + 0xe8 > end || end > bytes.len() {
        return Err("node mesh header exceeds its node".into());
    }
    let pointers: Vec<_> = (0..11)
        .map(|i| u32_at(bytes, node + 0xa4 + i * 4, "node mesh pointer").map(|v| v as usize))
        .collect::<Result<_, _>>()?;
    let count = u32_at(bytes, node + 0xd0, "node vertex count")? as usize;
    if pointers[0] != node + 0xe8 || pointers[1] != pointers[0] + count * 8 || pointers[1] > end {
        return Err("node vertex array does not tile its extent".into());
    }
    // 0x80026D20 masks the 16.16 centre down to multiples of 64 metres;
    // 0x80026D28..40 shifts it by ten. Vertices add in 1/64 metre units,
    // stored (x, file-z, height), confirmed by the start-straight road quads.
    let origin = [0x30, 0x38, 0x34]
        .map(|offset| i32_at(bytes, node + offset, "mesh origin").map(|v| (v & !0x3fffff) as f32 / 65536.0));
    let origin = [origin[0].clone()?, origin[1].clone()?, origin[2].clone()?];
    let mut positions = Vec::with_capacity(count);
    for i in 0..count {
        let at = pointers[0] + i * 8;
        let x = origin[0] + f32::from(i16_at(bytes, at, "vertex x")?) / 64.0;
        let z = origin[1] + f32::from(i16_at(bytes, at + 2, "vertex z")?) / 64.0;
        let y = origin[2] + f32::from(i16_at(bytes, at + 4, "vertex height")?) / 64.0;
        positions.push([x, y, z]);
    }
    let mut centre = [0.0; 3];
    for (axis, value) in centre.iter_mut().enumerate() {
        *value = i32_at(bytes, node + 0x30 + axis * 4, "node bound centre")? as f32 / 65536.0;
    }
    let subdivision = Subdivision {
        // Runtime constants: 0x800201E4, 0x800255CC; six recursive stack
        // slots after the initial split. UV midpoint stores are whole bytes.
        max_twice_area_px2: 2048.0,
        max_depth: 7,
        uv_step: 1.0 / 256.0,
        guard_band_px: 16.0,
    };
    let precision = Precision {
        position_step_m: 1.0 / 64.0,
        centre_m: centre,
        radius_m: u32_at(bytes, node + 0x2c, "node bound radius")? as f32 / 65536.0,
        // 0x80026BB4 selects up to two extra precision bits from the bound.
        distances_m: [128.0, 256.0],
    };
    let mut out = NodeMesh {
        faces: Vec::new(),
        vertices: count,
        polygons: 0,
        procedural: 0,
        subdivision: Some(subdivision),
        precision: Some(precision),
    };
    for (group, stride) in STRIDES.into_iter().enumerate() {
        let n = usize::from(u16_at(bytes, node + 0xd4 + group * 2, "node primitive count")?);
        let at = pointers[group + 1];
        let limit = if group < 9 { pointers[group + 2] } else { u32_at(bytes, node + 0x94, "node mesh end")? as usize };
        if at < pointers[1] || at + n * stride != limit || limit > end {
            return Err(format!("node group {group} does not tile its extent"));
        }
        if group >= 8 {
            out.procedural += n;
            continue;
        }
        for i in 0..n {
            let at = at + i * stride;
            let a = u32_at(bytes, at, "polygon indices")?;
            let b = u32_at(bytes, at + 4, "polygon indices/texture")?;
            let mut indices = vec![a & 511, a >> 9 & 511, a >> 18 & 511];
            if group % 2 == 1 {
                indices.push(b & 511);
            }
            if indices.iter().any(|&i| i as usize >= count) {
                return Err(format!("node group {group} polygon {i} has an invalid vertex index"));
            }
            let textured = group >= 4;
            let mut uv = vec![[0; 2]; indices.len()];
            let mut texture = None;
            let mut small_texture = None;
            if textured {
                let material = (b >> 9 & 0x3fff) as usize;
                if material >= record_count {
                    return Err(format!("node polygon references material {material}, outside 0..{record_count}"));
                }
                let record = record_base + material * 32;
                let first = u32_at(bytes, record, "UV0/CLUT")?;
                let second = u32_at(bytes, record + 4, "UV1/page")?;
                let third = u32_at(bytes, record + 8, "UV2/UV3")?;
                uv = [first as u16, second as u16, third as u16, (third >> 16) as u16]
                    .iter()
                    .take(indices.len())
                    .map(|v| v.to_le_bytes())
                    .collect();
                texture = Some(((second >> 16) as u16, (first >> 16) as u16));
                let low = record + 16;
                let low_first = u32_at(bytes, low, "small UV0/CLUT")?;
                let low_second = u32_at(bytes, low + 4, "small UV1/page")?;
                let low_third = u32_at(bytes, low + 8, "small UV2/UV3")?;
                small_texture = Some(FaceSmallTexture {
                    threshold: u16_at(bytes, record + 12, "material area threshold")?,
                    texture: ((low_second >> 16) as u16, (low_first >> 16) as u16),
                    uv: [low_first as u16, low_second as u16, low_third as u16, (low_third >> 16) as u16]
                        .iter()
                        .take(indices.len())
                        .map(|v| v.to_le_bytes())
                        .collect(),
                    blend: 0,
                });
            }
            let gouraud = matches!(group, 2 | 3 | 6 | 7);
            let mut colors = Vec::new();
            let command = (u32_at(bytes, at + 8, "polygon colour")? >> 24) as u8;
            for corner in 0..indices.len() {
                let color = u32_at(bytes, at + 8 + if gouraud { corner * 4 } else { 0 }, "polygon colour")?;
                let divisor = if textured { 128.0 } else { 255.0 };
                colors.push([
                    (color & 255) as f32 / divisor,
                    (color >> 8 & 255) as f32 / divisor,
                    (color >> 16 & 255) as f32 / divisor,
                    1.0,
                ]);
            }
            let blend = if command & 2 == 0 { 0 } else { 1 + ((texture.map_or(0, |(page, _)| page) >> 5) & 3) as u8 };
            if let Some(small) = &mut small_texture {
                small.blend = if command & 2 == 0 { 0 } else { 1 + ((small.texture.0 >> 5) & 3) as u8 };
            }
            out.faces.push(Face {
                positions: indices.iter().map(|&i| positions[i as usize]).collect(),
                colors,
                uv,
                texture,
                blend,
                double_sided: a & 0x80000000 == 0,
                adaptive: group == 5,
                small_texture,
            });
            out.polygons += 1;
        }
    }
    Ok(out)
}

/// Emit real road-node polygons and both screen-area texture variants.
/// Model LOD and procedural primitives remain separate renderer work.
pub fn import(input: &Path, textures: &Path, out: &Path, texture_prefix: &str) -> std::io::Result<String> {
    let bytes = std::fs::read(input)?;
    let course = Course::parse(&bytes).map_err(std::io::Error::other)?;
    let section = section_offsets(&bytes).map_err(std::io::Error::other)?;
    let record_base = u32_at(&bytes, section[0] + 8, "material table").map_err(std::io::Error::other)? as usize;
    let record_count = usize::from(u16_at(&bytes, section[0] + 6, "material count").map_err(std::io::Error::other)?);
    let mut sources = Vec::new();
    let mut keys = BTreeSet::new();
    let pack = TexturePack::parse(&std::fs::read(textures)?).map_err(std::io::Error::other)?;
    for i in 0..course.nodes.len() {
        let at = u32_at(&bytes, section[0] + 12 + i * 4, "node offset").map_err(std::io::Error::other)? as usize;
        let end = if i + 1 < course.nodes.len() {
            u32_at(&bytes, section[0] + 16 + i * 4, "node end").map_err(std::io::Error::other)? as usize
        } else {
            record_base
        };
        let mesh = node_mesh(&bytes, at, end, record_base, record_count)
            .map_err(|e| std::io::Error::other(format!("node {i}: {e}")))?;
        sources.push((format!("Course node {i}"), mesh));
    }
    for placement in super::instances::parse(&bytes).map_err(std::io::Error::other)? {
        let mesh = super::instances::decode_mesh(&bytes, &placement).map_err(std::io::Error::other)?;
        sources.push((format!("Scenery list {} instance {}", placement.list, placement.index), mesh));
    }
    for (_, mesh) in &sources {
        for face in &mesh.faces {
            if let Some((page, clut)) = face.texture {
                pack.validate_uv_rect(page, clut, &face.uv).map_err(std::io::Error::other)?;
                keys.insert((page, clut));
            }
            if let Some(small) = &face.small_texture {
                pack.validate_uv_rect(small.texture.0, small.texture.1, &small.uv).map_err(std::io::Error::other)?;
                keys.insert(small.texture);
            }
        }
    }
    let directory = out.parent().unwrap_or(Path::new("."));
    let texture_dir = directory.join("textures");
    let assets = pack.export(&texture_dir, &keys.into_iter().collect::<Vec<_>>()).map_err(std::io::Error::other)?;
    let textures_by_key: BTreeMap<_, _> = assets
        .iter()
        .map(|a| ((a.tpage, a.clut), format!("{}/textures/{}", texture_prefix.trim_end_matches('/'), a.texture)))
        .collect();
    let mut geometry = Geometry::default();
    let mut polygons = 0;
    let mut vertices = 0;
    let mut procedural = 0;
    for (name, mesh) in &sources {
        polygons += mesh.polygons;
        vertices += mesh.vertices;
        procedural += mesh.procedural;
        let mut batches = BTreeMap::new();
        for face in &mesh.faces {
            let part = batches
                .entry((
                    face.texture,
                    face.blend,
                    face.double_sided,
                    face.adaptive,
                    face.small_texture.as_ref().map(|s| (s.texture, s.blend, s.threshold)),
                ))
                .or_insert_with(|| Part {
                    name: name.clone(),
                    texture: face.texture.map(|key| textures_by_key[&key].clone()),
                    blend: blend(face.blend),
                    double_sided: face.double_sided,
                    subdivision: if face.adaptive { mesh.subdivision.clone() } else { None },
                    precision: if face.adaptive || face.small_texture.is_some() {
                        mesh.precision.clone()
                    } else {
                        None
                    },
                    small_texture: face.small_texture.as_ref().map(|s| SmallTexture {
                        max_twice_area_px2: u32::from(s.threshold),
                        texture: textures_by_key[&s.texture].clone(),
                        blend: blend(s.blend),
                        uv: Vec::new(),
                    }),
                    ..Part::default()
                });
            let base = part.positions_m.len() as u32;
            part.positions_m.extend(&face.positions);
            part.uv.extend(face.uv.iter().map(|p| [f32::from(p[0]) / 256.0, f32::from(p[1]) / 256.0]));
            part.colors.extend(&face.colors);
            if let Some(small) = &face.small_texture {
                part.small_texture
                    .as_mut()
                    .unwrap()
                    .uv
                    .extend(small.uv.iter().map(|p| [f32::from(p[0]) / 256.0, f32::from(p[1]) / 256.0]));
            }
            if face.positions.len() == 3 {
                part.triangles.push([base, base + 1, base + 2]);
            } else if face.adaptive || face.small_texture.is_some() {
                part.quads.push([base, base + 1, base + 2, base + 3]);
            } else {
                part.triangles.extend([[base, base + 1, base + 3], [base + 3, base + 1, base + 2]]);
            }
        }
        geometry.parts.extend(batches.into_values());
    }
    std::fs::create_dir_all(directory)?;
    std::fs::write(
        out,
        ron::ser::to_string_pretty(&geometry, ron::ser::PrettyConfig::default()).map_err(std::io::Error::other)?,
    )?;
    crate::Provenance::of_named(
        input,
        &format!("/crsobj/{}", input.file_name().unwrap_or_default().to_string_lossy()),
        "import tro-scene",
    )?
    .write_beside(out)?;
    for asset in &assets {
        crate::Provenance::of_named(
            textures,
            &format!("/crsobj/{}", textures.file_name().unwrap_or_default().to_string_lossy()),
            "import tro-scene textures",
        )?
        .write_beside(&texture_dir.join(&asset.texture))?;
    }
    Ok(format!(
        "{}: {} road nodes + {} scenery instances, {vertices} vertices, {polygons} polygons, {} textures from {} TIMs, {} batches; {procedural} procedural primitives pending; screen-area texture variants; highest model LOD only",
        out.display(),
        course.nodes.len(),
        sources.len() - course.nodes.len(),
        assets.len(),
        pack.tim_count,
        geometry.parts.len()
    ))
}

fn blend(value: u8) -> Blend {
    match value {
        1 => Blend::Half,
        2 => Blend::Add,
        3 => Blend::Subtract,
        4 => Blend::AddQuarter,
        _ => Blend::Opaque,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires GT2_TRO_CORPUS: extracted 126-course corpus"]
    fn all_road_node_mesh_arrays_and_references_are_valid() {
        let root = std::env::var_os("GT2_TRO_CORPUS").expect("set GT2_TRO_CORPUS");
        let mut files: Vec<_> = std::fs::read_dir(root)
            .unwrap()
            .map(|p| p.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "tro"))
            .collect();
        files.sort();
        assert_eq!(files.len(), 126);
        let (mut nodes, mut vertices, mut polygons) = (0, 0, 0);
        for file in files {
            let bytes = std::fs::read(&file).unwrap();
            let section = section_offsets(&bytes).unwrap();
            let count = usize::from(u16_at(&bytes, section[0] + 4, "nodes").unwrap());
            let records = usize::from(u16_at(&bytes, section[0] + 6, "materials").unwrap());
            let base = u32_at(&bytes, section[0] + 8, "material base").unwrap() as usize;
            for i in 0..count {
                let at = u32_at(&bytes, section[0] + 12 + i * 4, "node").unwrap() as usize;
                let end = if i + 1 < count {
                    u32_at(&bytes, section[0] + 16 + i * 4, "next node").unwrap() as usize
                } else {
                    base
                };
                let mesh = node_mesh(&bytes, at, end, base, records)
                    .unwrap_or_else(|e| panic!("{} node {i}: {e}", file.display()));
                nodes += 1;
                vertices += mesh.vertices;
                polygons += mesh.polygons;
            }
        }
        assert_eq!((nodes, vertices, polygons), (14387, 1129468, 699211));
    }
}
