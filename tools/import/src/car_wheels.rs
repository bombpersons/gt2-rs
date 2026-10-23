//! Offline wheel construction from GT2's authored profile tables and car
//! setup parameters. The native application receives ordinary metre geometry.
//! Evidence, pointer bases and remaining animation work: wheel-model-evidence.md.
use serde::Serialize;
use std::path::Path;

const OUTER_CAR: usize = 0xa9688;
const PROFILE_TABLES: [usize; 3] = [0x91670, 0x91878, 0x91990];
const PROFILE_LENGTHS: [usize; 3] = [520, 280, 220];
const GENERATED_OFFSETS: [[usize; 3]; 2] = [[0x70, 0x480, 0x6b0], [0x278, 0x598, 0x78c]];

fn half(bytes: &[u8], at: usize) -> Result<i16, String> {
    Ok(i16::from_le_bytes(bytes.get(at..at + 2).ok_or("truncated wheel halfword")?.try_into().unwrap()))
}
fn word(bytes: &[u8], at: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(bytes.get(at..at + 4).ok_or("truncated wheel word")?.try_into().unwrap()))
}

/// One axle's measured setup, in original 12-bit fractional metre storage.
/// Keeping rounding here permits byte-for-byte verification of the original
/// initializer; only ordinary floats and triangles leave the offline importer.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct WheelSetup {
    pub outer_radius: i16,
    pub width: i16,
    pub sidewall: i16,
    pub rim_inset: i16,
}

impl WheelSetup {
    fn inner_factor(self) -> Result<i32, String> {
        if self.outer_radius <= 0 || self.width <= 0 || self.sidewall < 0 || self.sidewall >= self.outer_radius {
            return Err("invalid wheel radius, width or sidewall".into());
        }
        Ok(4096 - (i32::from(self.sidewall) * 4096 / i32::from(self.outer_radius)))
    }
    fn inner_radius(self) -> Result<i16, String> {
        Ok(((i32::from(self.outer_radius) * self.inner_factor()?) >> 12) as i16)
    }
}

/// Decode the immutable profile tables into pairs of 3D vertices, then apply
/// the source initializer's radial and axial scales. Strip 0 joins the outer
/// and inner sidewall rings; remaining strips use the full radius at both ends.
/// Each strip closes with an authored pair; its quads connect adjacent pairs.
fn build_stream(template: &[u8], setup: WheelSetup) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    let mut at = 0;
    let radius = i32::from(setup.outer_radius);
    let width = i32::from(setup.width);
    let mut second_radius = i32::from(setup.inner_radius()?);
    loop {
        let count = word(template, at)? as usize;
        at += 4;
        if count > 128 {
            return Err("wheel profile strip exceeds 128 pairs".into());
        }
        result.extend_from_slice(&(count as u32).to_le_bytes());
        if count == 0 {
            if at != template.len() {
                return Err("trailing wheel profile bytes".into());
            }
            break;
        }
        for _ in 0..count {
            // Packed source pair: A.x,A.y,A.z,B.z,B.x,B.y. The renderer
            // feeds this packing to the GTE; it is not the native mesh format.
            for (corner, scale) in [radius, radius, width, width, second_radius, second_radius].into_iter().enumerate()
            {
                let value = (i32::from(half(template, at + corner * 2)?) * scale) >> 12;
                result
                    .extend_from_slice(&(i16::try_from(value).map_err(|_| "wheel coordinate overflow")?).to_le_bytes());
            }
            at += 12;
        }
        second_radius = radius;
    }
    Ok(result)
}

#[derive(Debug, Serialize)]
struct Geometry {
    parts: Vec<Part>,
}

#[derive(Debug, Serialize)]
struct WheelLayout {
    body_asset: String,
    wheels: Vec<WheelPlacement>,
}

#[derive(Debug, Serialize)]
struct WheelPlacement {
    name: String,
    geometry: String,
    position_m: [f32; 3],
    rotation_xyzw: [f32; 4],
    motion: Option<WheelMotionBinding>,
}

#[derive(Debug, Serialize)]
struct WheelMotionBinding {
    wheel: usize,
    suspension_origin_y_m: f32,
    camber_turns: f32,
    mesh_rotation_xyzw: [f32; 4],
}

impl Geometry {
    fn mirror_axle(&mut self) {
        for part in &mut self.parts {
            for position in &mut part.positions_m {
                position[2] = -position[2];
            }
            // The original toggles the tire's culling direction after its
            // odd-wheel axial reflection. Reflecting geometry reverses its
            // winding; reversing indices keeps native front-face culling.
            for triangle in &mut part.triangles {
                triangle.swap(1, 2);
            }
        }
    }
}
#[derive(Debug, Serialize)]
struct Part {
    name: String,
    texture: Option<String>,
    double_sided: bool,
    positions_m: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    triangles: Vec<[u32; 3]>,
}

fn geometry(stream: &[u8], setup: WheelSetup, texture: String) -> Result<Geometry, String> {
    let mut tire = Part {
        name: "Authored tire profile".into(),
        texture: None,
        double_sided: false,
        positions_m: vec![],
        uv: vec![],
        colors: vec![],
        triangles: vec![],
    };
    let mut at = 0;
    loop {
        let count = word(stream, at)? as usize;
        at += 4;
        if count == 0 {
            break;
        }
        let start = tire.positions_m.len() as u32;
        for _ in 0..count {
            let p: Vec<_> =
                (0..6).map(|i| half(stream, at + i * 2).map(|v| f32::from(v) / 4096.0)).collect::<Result<_, _>>()?;
            tire.positions_m.extend([[p[0], p[1], p[2]], [p[4], p[5], p[3]]]);
            tire.uv.extend([[0.0; 2]; 2]);
            // B-1 header +8's tire colour word is zero, loaded through
            // 0x80067738..750 into scratch+3B0 and read at 0x80066F90.
            tire.colors.extend([[0.0, 0.0, 0.0, 1.0]; 2]);
            at += 12;
        }
        for pair in 0..count - 1 {
            let a = start + pair as u32 * 2;
            tire.triangles.extend([[a, a + 1, a + 2], [a + 2, a + 1, a + 3]]);
        }
    }
    let r = f32::from(setup.inner_radius()?) / 4096.0;
    let z = f32::from((setup.width >> 1) - setup.rim_inset) / 4096.0;
    let mut rim = Part {
        name: "Default wheel image".into(),
        texture: Some(texture),
        double_sided: true,
        positions_m: vec![],
        uv: vec![],
        colors: vec![[1.0; 4]; 8],
        triangles: vec![],
    };
    // This rim path emits two textured GPU quads without NCLIP rejection
    // once packet capacity and projection checks allow the wheel draw.
    // Their opposing strip winding is preserved with a double-sided material.
    let corners = [[-r, r, z], [r, r, z], [r, -r, z], [-r, -r, z], [0.0, 0.0, z]];
    for (quad, uv) in [
        ([1, 0, 4, 3], [[47., 0.], [0., 0.], [23., 23.], [0., 47.]]),
        ([1, 2, 4, 3], [[47., 0.], [47., 47.], [23., 23.], [0., 47.]]),
    ]
    .into_iter()
    .enumerate()
    {
        let (ids, uv) = uv;
        rim.positions_m.extend(ids.map(|i| corners[i]));
        rim.uv.extend(uv.map(|p| [p[0] / 256.0, p[1] / 224.0]));
        let a = quad as u32 * 4;
        rim.triangles.extend([[a, a + 1, a + 2], [a + 2, a + 1, a + 3]]);
    }
    Ok(Geometry { parts: vec![tire, rim] })
}

/// Bake all three front/rear wheel detail levels from the verified B-1
/// checkpoint. It rejects any generated coordinate that disagrees with the
/// runtime arrays. This entry is checkpoint-specific; broader upgrade/paint
/// selection and arbitrary vehicle-parameter extraction remain separate work.
pub fn import(
    ram_path: &Path,
    paint_path: &Path,
    output: &Path,
    paint: usize,
    texture_prefix: &str,
) -> Result<String, String> {
    let ram = std::fs::read(ram_path).map_err(|e| e.to_string())?;
    if ram.len() != 2 * 1024 * 1024 {
        return Err("wheel evidence requires a 2 MiB RAM dump".into());
    }
    let body = word(&ram, OUTER_CAR + 0x878)?.checked_sub(0x80000000).ok_or("invalid body pointer")? as usize;
    if body != 0x15f894 {
        return Err("wheel importer currently supports the identified B-1 checkpoint".into());
    }
    let mut paints = std::fs::read(paint_path).map_err(|e| e.to_string())?;
    if paints.starts_with(&[0x1f, 0x8b]) {
        use std::io::Read;
        let mut decoded = Vec::new();
        flate2::read::GzDecoder::new(paints.as_slice()).read_to_end(&mut decoded).map_err(|e| e.to_string())?;
        paints = decoded;
    }
    let paint_data = super::car_model::parse_texture(&paints)?;
    // Observed default wheel: page1A at (640,256), CLUT7828 at (640,480).
    // This is the first active body palette, proven paint slot5 in B-1.
    if word(&ram, body + 0x40)? != 0x7828001a || paint != 5 {
        return Err("unsupported wheel texture/paint selection; B-1 requires observed slot5 palette0".into());
    }
    std::fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let texture_file = output.join("t2vzn-paint5-palette0.png");
    let file = std::fs::File::create(&texture_file).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 256, 224);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .map_err(|e| e.to_string())?
        .write_image_data(&paint_data.rgba(paint, 0)?)
        .map_err(|e| e.to_string())?;
    crate::Provenance::of(paint_path, "import default wheel paint")
        .and_then(|p| p.write_beside(&texture_file))
        .map_err(|e| e.to_string())?;
    let texture = format!("{}/t2vzn-paint5-palette0.png", texture_prefix.trim_matches('/'));
    let car_index = half(&ram, OUTER_CAR + 0xc)?;
    if car_index < 0 {
        return Err("invalid wheel car index".into());
    }
    // 0x80018074..0x800180B8: setup-table record stride 208, flags+0x64
    // selects one of the four rim inset values at 0x80091A70.
    let wheel_flags = word(&ram, 0x1d585c + car_index as usize * 208 + 0x64)?;
    let rim_inset = half(&ram, 0x91a70 + ((wheel_flags >> 7) & 6) as usize)?;
    for (axle, lod_offsets) in GENERATED_OFFSETS.iter().enumerate() {
        let setup = WheelSetup {
            outer_radius: half(&ram, OUTER_CAR + 0x410 + axle * 2)?,
            width: half(&ram, OUTER_CAR + 0x418 + axle * 2)?,
            sidewall: half(&ram, OUTER_CAR + 0x41c + axle * 2)?,
            rim_inset,
        };
        for (lod, &runtime_offset) in lod_offsets.iter().enumerate() {
            let start = PROFILE_TABLES[lod];
            let stream = build_stream(&ram[start..start + PROFILE_LENGTHS[lod]], setup)?;
            let runtime = body + runtime_offset;
            if ram.get(runtime..runtime + stream.len()) != Some(stream.as_slice()) {
                return Err(format!("generated axle{axle} LOD{lod} does not match observed runtime geometry"));
            }
            let file = output.join(format!("{}-lod{lod}.ron", if axle == 0 { "front" } else { "rear" }));
            let mut native = geometry(&stream, setup, texture.clone())?;
            std::fs::write(
                &file,
                ron::ser::to_string_pretty(&native, ron::ser::PrettyConfig::default()).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            crate::Provenance::of(ram_path, "import procedural wheel profiles and setup")
                .and_then(|p| p.write_beside(&file))
                .map_err(|e| e.to_string())?;
            native.mirror_axle();
            let right_file = output.join(format!("{}-right-lod{lod}.ron", if axle == 0 { "front" } else { "rear" }));
            std::fs::write(
                &right_file,
                ron::ser::to_string_pretty(&native, ron::ser::PrettyConfig::default()).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            crate::Provenance::of(ram_path, "import reflected wheel profiles with native culling")
                .and_then(|p| p.write_beside(&right_file))
                .map_err(|e| e.to_string())?;
        }
    }
    let mut placements = Vec::new();
    for wheel in 0..4 {
        let steering = half(&ram, OUTER_CAR + 0x394 + (wheel / 2) * 2)?;
        let camber = half(&ram, OUTER_CAR + 0x498 + wheel * 104)?;
        if steering != 0 || camber != 0 {
            return Err("checkpoint layout currently requires observed zero steering/camber".into());
        }
        let spin = f32::from(half(&ram, OUTER_CAR + 0x4ac + wheel * 104)?) * std::f32::consts::TAU / 4096.0;
        let (sin, cos) = (spin * 0.5).sin_cos();
        let quarter = std::f32::consts::FRAC_1_SQRT_2;
        let mut position_m = [0.0; 3];
        for (axis, value) in position_m.iter_mut().enumerate() {
            *value = f32::from(half(&ram, OUTER_CAR + 0x7c4 + wheel * 16 + axis * 2)?) / 4096.0;
        }
        let axle = if wheel < 2 { "front" } else { "rear" };
        let side = if wheel % 2 == 0 { "" } else { "-right" };
        placements.push(WheelPlacement {
            name: format!("{axle} {} wheel", if wheel % 2 == 0 { "left" } else { "right" }),
            geometry: format!("{}/{axle}{side}-lod0.ron", texture_prefix.trim_matches('/')),
            position_m,
            rotation_xyzw: [-quarter * sin, -quarter * cos, quarter * sin, quarter * cos],
            motion: Some(WheelMotionBinding {
                wheel,
                suspension_origin_y_m: f32::from(half(&ram, OUTER_CAR + 0x160 + (wheel / 2) * 52)?) / 4096.0,
                camber_turns: f32::from(half(&ram, OUTER_CAR + 0x394 + (wheel / 2) * 2)?) / 4096.0,
                mesh_rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
            }),
        });
    }
    let layout = WheelLayout { body_asset: "gt2/vehicle/t2vzn.ron".into(), wheels: placements };
    let layout_file = output.parent().ok_or("wheel output needs a parent directory")?.join("t2vzn-wheels.ron");
    std::fs::write(
        &layout_file,
        ron::ser::to_string_pretty(&layout, ron::ser::PrettyConfig::default()).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    crate::Provenance::of(ram_path, "import checkpoint wheel placement and spin")
        .and_then(|p| p.write_beside(&layout_file))
        .map_err(|e| e.to_string())?;
    Ok("six byte-verified axle/LOD constructions, twelve meshes including reflected right wheels, checkpoint layout with simulation bindings and default paint".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_wheel_dimensions() {
        let setup = WheelSetup { outer_radius: 0, width: 634, sidewall: 507, rim_inset: 81 };
        assert!(build_stream(&[], setup).is_err());
    }
    #[test]
    fn constructs_the_authored_sidewall_tread_and_inner_surface_scales() {
        let setup = WheelSetup { outer_radius: 1183, width: 634, sidewall: 507, rim_inset: 81 };
        assert_eq!(setup.inner_radius().unwrap(), 676);
        let mut source = Vec::new();
        for _ in 0..2 {
            source.extend_from_slice(&1u32.to_le_bytes());
            for v in [4096i16, 814, 2048, 2048, 4096, 814] {
                source.extend_from_slice(&v.to_le_bytes());
            }
        }
        source.extend_from_slice(&0u32.to_le_bytes());
        let built = build_stream(&source, setup).unwrap();
        assert_eq!(
            (0..6).map(|i| half(&built, 4 + i * 2).unwrap()).collect::<Vec<_>>(),
            [1183, 235, 317, 317, 676, 134]
        );
        assert_eq!(
            (0..6).map(|i| half(&built, 20 + i * 2).unwrap()).collect::<Vec<_>>(),
            [1183, 235, 317, 317, 1183, 235]
        );
    }

    #[test]
    fn axial_reflection_keeps_native_tire_culling_and_original_rim_uvs() {
        let setup = WheelSetup { outer_radius: 1183, width: 634, sidewall: 507, rim_inset: 81 };
        let mut source = 2u32.to_le_bytes().to_vec();
        for pair in [[4096i16, 814, 2048, 2048, 4096, 814], [4095, -814, 2048, 2048, 4095, -814]] {
            for value in pair {
                source.extend_from_slice(&value.to_le_bytes());
            }
        }
        source.extend_from_slice(&0u32.to_le_bytes());
        let mut native = geometry(&build_stream(&source, setup).unwrap(), setup, "handmade.png".into()).unwrap();
        let normal_z = |part: &Part| {
            let [a, b, c] = part.triangles[0].map(|i| part.positions_m[i as usize]);
            (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
        };
        assert!(normal_z(&native.parts[0]) > 0.0, "canonical sidewall faces its positive-Z exterior");
        assert!(!native.parts[0].double_sided);
        assert!(native.parts[1].double_sided, "source rim has no NCLIP rejection in this path");
        let uv = native.parts[1].uv.clone();
        assert_eq!(uv[0], [47.0 / 256.0, 0.0]);
        assert_eq!(uv[2], [23.0 / 256.0, 23.0 / 224.0]);
        native.mirror_axle();
        assert!(normal_z(&native.parts[0]) < 0.0, "reflected tire exterior faces negative Z");
        assert_eq!(native.parts[1].uv, uv, "reflection changes geometry, not authored image coordinates");
    }
}
