//! Authorable polygon geometry in metres, with ordinary image asset paths.
//! Importers resolve original material/texture addressing offline. This format
//! can also be authored by another game or a mod, without imported content.
use crate::psx::{PsxBlend, PsxMaterial, PsxStp};
use bevy::{
    asset::RenderAssetUsages,
    camera::{CameraUpdateSystems, visibility::VisibilitySystems},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use serde::{Deserialize, Serialize};
mod detail;
mod projection;
mod subdivision;
pub use detail::{SmallTexture, selects_small_texture};
pub use projection::{RasterPrecision, RasterProjection};
pub use subdivision::{QuadRaster, QuadRefinement, QuadSubdivision, RasterVertex};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GeometryData {
    #[serde(default)]
    pub parts: Vec<MeshPart>,
}

/// One polygon mesh with a single material. Colours are sRGB modulation
/// values: white is neutral, and values above one may brighten a texture.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MeshPart {
    pub name: String,
    #[serde(default)]
    pub texture: Option<String>,
    #[serde(default)]
    pub blend: SurfaceBlend,
    #[serde(default)]
    pub double_sided: bool,
    pub positions_m: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub triangles: Vec<[u32; 3]>,
    /// Perimeter-ordered quads. Preserve them until view-dependent refinement.
    #[serde(default)]
    pub quads: Vec<[u32; 4]>,
    #[serde(default)]
    pub subdivision: Option<QuadSubdivision>,
    #[serde(default)]
    pub precision: Option<RasterPrecision>,
    #[serde(default)]
    pub small_texture: Option<SmallTexture>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SurfaceBlend {
    #[default]
    Opaque,
    Half,
    Add,
    Subtract,
    AddQuarter,
}

impl GeometryData {
    pub fn validate(&self) -> Result<(), String> {
        for part in &self.parts {
            let n = part.positions_m.len();
            if part.uv.len() != n || part.colors.len() != n {
                return Err(format!("{}: attribute lengths differ", part.name));
            }
            if part
                .positions_m
                .iter()
                .flatten()
                .chain(part.uv.iter().flatten())
                .chain(part.colors.iter().flatten())
                .any(|v| !v.is_finite())
            {
                return Err(format!("{}: non-finite vertex attribute", part.name));
            }
            if part.triangles.iter().flatten().chain(part.quads.iter().flatten()).any(|&index| index as usize >= n) {
                return Err(format!("{}: triangle index outside its vertex array", part.name));
            }
            if let Some(settings) = &part.subdivision {
                settings.validate().map_err(|e| format!("{}: {e}", part.name))?;
            }
            if let Some(precision) = &part.precision {
                precision.validate().map_err(|e| format!("{}: {e}", part.name))?;
            } else if part.subdivision.is_some() || part.small_texture.is_some() {
                return Err(format!("{}: view-dependent geometry requires raster precision", part.name));
            }
            if let Some(small) = &part.small_texture
                && (small.texture.is_empty()
                    || small.uv.len() != n
                    || small.uv.iter().flatten().any(|v| !v.is_finite()))
            {
                return Err(format!("{}: invalid small texture attributes", part.name));
            }
        }
        Ok(())
    }
}

impl MeshPart {
    pub fn mesh(&self) -> Mesh {
        let colors: Vec<_> =
            self.colors.iter().map(|c| Color::srgba(c[0], c[1], c[2], c[3]).to_linear().to_f32_array()).collect();
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions_m.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
            .with_inserted_indices(Indices::U32(self.all_triangles().into_iter().flatten().collect()))
    }

    fn all_triangles(&self) -> Vec<[u32; 3]> {
        let mut triangles = self.triangles.clone();
        for &[a, b, c, d] in &self.quads {
            triangles.extend([[a, b, d], [d, b, c]]);
        }
        triangles
    }

    /// Build one material bucket. Small-detail UVs are chosen on source
    /// polygons before any midpoint rounding, subdivision or near clipping.
    /// Matrices are normal Bevy transforms, not original packet state.
    pub fn for_view(&self, raster_from_local: Mat4, size: Vec2, focal_px: f32, eye_local: Vec3, small: bool) -> Self {
        let Some(precision) = &self.precision else { return self.clone() };
        let local_from_raster = raster_from_local.inverse();
        let step = precision.position_step(eye_local);
        let mut out = Self {
            name: self.name.clone(),
            texture: self.texture.clone(),
            blend: self.blend,
            double_sided: self.double_sided,
            ..default()
        };
        if small && let Some(detail) = &self.small_texture {
            out.texture = Some(detail.texture.clone());
            out.blend = detail.blend;
        }
        let projection = RasterProjection { size, focal_px, position_step: step, guard_band_px: 0.0 };
        let vertex = |index: u32| {
            let index = index as usize;
            let position = raster_from_local.transform_point3(Vec3::from_array(self.positions_m[index]));
            RasterVertex {
                position: (position / step).floor() * step,
                uv: Vec2::from_array(
                    if small { self.small_texture.as_ref().map_or(&self.uv, |d| &d.uv) } else { &self.uv }[index],
                ),
                color: Vec4::from_array(self.colors[index]),
            }
        };
        let selected = |polygon: &[RasterVertex]| {
            self.small_texture.as_ref().is_none_or(|detail| {
                let pixels: Vec<_> = polygon.iter().map(|v| projection.pixel(v.position)).collect();
                selects_small_texture(detail.max_twice_area_px2, &pixels) == small
            })
        };
        for triangle in &self.triangles {
            let polygon = triangle.map(vertex);
            if !selected(&polygon) {
                continue;
            }
            let [a, b, c] = polygon.map(|v| v.position);
            if self.double_sided || (b - a).cross(c - a).dot(-a) > 0.0 {
                out.append_raster_polygon(&polygon, local_from_raster);
            }
        }
        for quad in &self.quads {
            let polygon = quad.map(vertex);
            if !selected(&polygon) {
                continue;
            }
            let Some(settings) = &self.subdivision else {
                out.append_raster_polygon(&polygon, local_from_raster);
                continue;
            };
            let mut raster = QuadRaster {
                size,
                focal_px,
                position_step: step,
                uv_step: settings.uv_step,
                max_twice_area_px2: settings.max_twice_area_px2,
                max_depth: settings.max_depth,
                guard_band_px: settings.guard_band_px,
            };
            if !raster.root_visible(&polygon, self.double_sided) {
                continue;
            }
            if !raster.needs_split(&polygon) {
                out.append_raster_polygon(&polygon, local_from_raster);
                continue;
            }
            if raster.projection_failed(&polygon) {
                // Source close roots gain coordinate precision before splitting:
                // shift all coordinates together until the largest magnitude
                // occupies 15 bits. Midpoint rounding then uses that finer grid.
                let largest = polygon.iter().map(|v| (v.position.abs() / step).max_element() as u32).max().unwrap_or(0);
                let shift = 15u32.saturating_sub(32 - largest.leading_zeros());
                raster.position_step = step / (1u32 << shift) as f32;
            }
            let refined = raster.refine(polygon, |_, _| {});
            for stitch in refined.stitches {
                out.append_raster_polygon(&stitch, local_from_raster);
            }
            for leaf in refined.quads {
                out.append_raster_polygon(&leaf, local_from_raster);
            }
        }
        out
    }

    fn append_raster_polygon(&mut self, polygon: &[RasterVertex], local_from_raster: Mat4) {
        let base = self.positions_m.len() as u32;
        for v in polygon {
            self.positions_m.push(local_from_raster.transform_point3(v.position).to_array());
            self.uv.push(v.uv.to_array());
            self.colors.push(v.color.to_array());
        }
        if polygon.len() == 3 {
            self.triangles.push([base, base + 1, base + 2]);
        } else {
            self.triangles.extend([[base, base + 1, base + 3], [base + 3, base + 1, base + 2]]);
        }
    }

    /// Clip triangles against a local-space plane before projecting them.
    /// Interpolate source UVs/colours at newly cut vertices. Affine raster
    /// attributes cannot be left to hardware clipping when an original vertex
    /// is behind the camera: they were multiplied by its negative clip w.
    pub fn clipped(&self, plane: Vec4) -> Self {
        #[derive(Clone, Copy)]
        struct Vertex {
            position: Vec3,
            uv: Vec2,
            color: Vec4,
        }
        impl Vertex {
            fn lerp(self, other: Self, t: f32) -> Self {
                Self {
                    position: self.position.lerp(other.position, t),
                    uv: self.uv.lerp(other.uv, t),
                    color: self.color.lerp(other.color, t),
                }
            }
        }
        let mut out = Self {
            name: self.name.clone(),
            texture: self.texture.clone(),
            blend: self.blend,
            double_sided: self.double_sided,
            ..default()
        };
        for triangle in &self.all_triangles() {
            let input: Vec<_> = triangle
                .iter()
                .map(|&i| {
                    let i = i as usize;
                    Vertex {
                        position: Vec3::from_array(self.positions_m[i]),
                        uv: Vec2::from_array(self.uv[i]),
                        color: Vec4::from_array(self.colors[i]),
                    }
                })
                .collect();
            let mut polygon = Vec::with_capacity(4);
            let mut previous = input[2];
            let mut previous_distance = plane.dot(previous.position.extend(1.0));
            for current in input {
                let distance = plane.dot(current.position.extend(1.0));
                if (distance >= 0.0) != (previous_distance >= 0.0) {
                    polygon.push(previous.lerp(current, previous_distance / (previous_distance - distance)));
                }
                if distance >= 0.0 {
                    polygon.push(current);
                }
                previous = current;
                previous_distance = distance;
            }
            if polygon.len() < 3 {
                continue;
            }
            let base = out.positions_m.len() as u32;
            for vertex in &polygon {
                out.positions_m.push(vertex.position.to_array());
                out.uv.push(vertex.uv.to_array());
                out.colors.push(vertex.color.to_array());
            }
            for i in 1..polygon.len() - 1 {
                out.triangles.push([base, base + i as u32, base + i as u32 + 1]);
            }
        }
        out
    }
}

/// Select the camera used to clip this scene's geometry before affine
/// projection. A scene viewed by several cameras needs a separate clipped
/// mesh set for each view; the current plugin accepts one selected camera.
#[derive(Component)]
pub struct GeometryClipCamera;

#[derive(Component)]
struct ClipSource {
    part: MeshPart,
    small: bool,
    previous_clip: Option<Mat4>,
    previous_size: Vec2,
}

/// Optional view-dependent presentation for meshes spawned through
/// spawn_geometry: refine authored quads, then clip before affine projection.
/// Rebuilds when the camera/projection, target size or object transform changes.
pub struct GeometryClippingPlugin;

impl Plugin for GeometryClippingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            clip_geometry
                .after(TransformSystems::Propagate)
                .after(CameraUpdateSystems)
                .before(VisibilitySystems::CalculateBounds),
        );
    }
}

fn clip_geometry(
    cameras: Query<(&GlobalTransform, &Projection, &Camera), With<GeometryClipCamera>>,
    mut parts: Query<(&GlobalTransform, &Mesh3d, &mut ClipSource)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Ok((camera, projection, viewport)) = cameras.single() else { return };
    let Some(size) =
        viewport.physical_viewport_size().map(|size| size.as_vec2()).filter(|size| size.min_element() > 0.0)
    else {
        return;
    };
    let clip_from_view = projection.get_clip_from_view();
    let clip_from_world = clip_from_view * camera.to_matrix().inverse();
    for (transform, mesh, mut source) in &mut parts {
        let clip_from_local = clip_from_world * transform.to_matrix();
        if source.previous_clip == Some(clip_from_local) && source.previous_size == size {
            continue;
        }
        source.previous_clip = Some(clip_from_local);
        source.previous_size = size;
        // WebGPU reverse depth: the near plane is clip.w - clip.z >= 0.
        let transpose = clip_from_local.transpose();
        let plane = transpose.w_axis - transpose.z_axis;
        if let Some(mut mesh) = meshes.get_mut(&mesh.0) {
            let x_scale = clip_from_view.x_axis.x;
            let raster_from_local = Mat4::from_cols(
                clip_from_local.row(0) / x_scale,
                -clip_from_local.row(1) * (size.y / (size.x * x_scale)),
                clip_from_local.row(3),
                Vec4::W,
            )
            .transpose();
            let eye_local = transform.to_matrix().inverse().transform_point3(camera.translation());
            *mesh = source
                .part
                .for_view(raster_from_local, size, x_scale * size.x * 0.5, eye_local, source.small)
                .clipped(plane)
                .mesh();
        }
    }
}

/// Spawn a validated asset under one ordinary Bevy parent. Geometry uses the
/// authored vertex colours; lighting can be added by a separate feature.
pub fn spawn_geometry(
    commands: &mut Commands,
    geometry: &GeometryData,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<PsxMaterial>,
    server: &AssetServer,
) -> Result<Entity, String> {
    geometry.validate()?;
    let root = commands.spawn((Name::new("Geometry"), Transform::default(), Visibility::default())).id();
    for part in &geometry.parts {
        for small in [false, true] {
            if small && part.small_texture.is_none() {
                continue;
            }
            let (texture, surface_blend) = if small {
                let detail = part.small_texture.as_ref().unwrap();
                (Some(&detail.texture), detail.blend)
            } else {
                (part.texture.as_ref(), part.blend)
            };
            let blend = match surface_blend {
                SurfaceBlend::Opaque => PsxBlend::Opaque,
                SurfaceBlend::Half => PsxBlend::Half,
                SurfaceBlend::Add => PsxBlend::Add,
                SurfaceBlend::Subtract => PsxBlend::Subtract,
                SurfaceBlend::AddQuarter => PsxBlend::AddQuarter,
            };
            let base = match texture {
                Some(path) => PsxMaterial::textured(server.load(path.clone())),
                None => PsxMaterial::color(Color::WHITE),
            };
            // Refined roots are culled geometrically. Stitch triangles can
            // have either screen winding after midpoint/pixel rounding.
            let material = PsxMaterial {
                lit: false,
                blend,
                double_sided: part.double_sided || part.subdivision.is_some(),
                stp: PsxStp::Texel,
                ..base
            };
            let mesh = meshes.add(part.mesh());
            let name = if small { format!("{} small detail", part.name) } else { part.name.clone() };
            commands.spawn((
                Name::new(name.clone()),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(materials.add(material.clone())),
                ChildOf(root),
                ClipSource { part: part.clone(), small, previous_clip: None, previous_size: Vec2::ZERO },
            ));
            if blend == PsxBlend::Subtract && texture.is_some() {
                commands.spawn((
                    Name::new(format!("{name} opaque texels")),
                    Mesh3d(mesh),
                    MeshMaterial3d(materials.add(PsxMaterial { stp: PsxStp::OpaqueOnly, ..material })),
                    ChildOf(root),
                    ClipSource { part: part.clone(), small, previous_clip: None, previous_size: Vec2::ZERO },
                ));
            }
        }
    }
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_detail_uses_whole_parents_before_refining_and_keeps_alternate_uvs() {
        let source = MeshPart {
            name: "handmade detail panels".into(),
            double_sided: true,
            positions_m: [8.0, 32.0]
                .into_iter()
                .flat_map(|z| [[-2.0, -2.0, z], [2.0, -2.0, z], [2.0, 2.0, z], [-2.0, 2.0, z]])
                .collect(),
            uv: [[0.0, 0.0], [0.25, 0.0], [0.25, 0.25], [0.0, 0.25]].repeat(2),
            colors: vec![[1.0; 4]; 8],
            quads: vec![[0, 1, 2, 3], [4, 5, 6, 7]],
            small_texture: Some(SmallTexture {
                max_twice_area_px2: 512,
                texture: "handmade.png".into(),
                blend: SurfaceBlend::Opaque,
                uv: [[0.5, 0.5], [0.75, 0.5], [0.75, 0.75], [0.5, 0.75]].repeat(2),
            }),
            precision: Some(RasterPrecision {
                position_step_m: 1.0 / 64.0,
                centre_m: [0.0; 3],
                radius_m: 0.0,
                distances_m: [128.0, 256.0],
            }),
            subdivision: Some(QuadSubdivision {
                max_twice_area_px2: 256.0,
                max_depth: 4,
                uv_step: 1.0 / 256.0,
                guard_band_px: 0.0,
            }),
            ..default()
        };
        GeometryData { parts: vec![source.clone()] }.validate().unwrap();
        let large = source.for_view(Mat4::IDENTITY, Vec2::splat(128.0), 64.0, Vec3::ZERO, false);
        let small = source.for_view(Mat4::IDENTITY, Vec2::splat(128.0), 64.0, Vec3::ZERO, true);
        assert!(large.triangles.len() > 2, "large source parent refines into many children");
        assert_eq!(small.triangles.len(), 2, "distant parent stays a single quad");
        assert!(large.positions_m.iter().all(|p| p[2] == 8.0));
        assert!(small.positions_m.iter().all(|p| p[2] == 32.0));
        assert!(large.uv.iter().flatten().all(|&v| v <= 0.25));
        assert!(small.uv.iter().flatten().all(|&v| (0.5..=0.75).contains(&v)));
        assert_eq!(small.texture.as_deref(), Some("handmade.png"));
    }

    #[test]
    fn near_clipping_cuts_behind_camera_vertices_and_interpolates_attributes() {
        let source = MeshPart {
            name: "crossing triangle".into(),
            positions_m: vec![[-1.0, 0.0, -1.0], [1.0, 0.0, -1.0], [0.0, 2.0, 1.0]],
            uv: vec![[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]],
            colors: vec![[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0], [0.5, 1.0, 0.0, 1.0]],
            triangles: vec![[0, 1, 2]],
            ..default()
        };
        let clipped = source.clipped(Vec4::new(0.0, 0.0, -1.0, -0.1));
        assert_eq!((clipped.positions_m.len(), clipped.triangles.len()), (4, 2));
        assert!(clipped.positions_m.iter().all(|p| p[2] <= -0.099999));
        for (point, uv) in clipped.positions_m.iter().zip(&clipped.uv) {
            if (point[2] + 0.1).abs() < 0.00001 {
                assert!((point[1] - 0.9).abs() < 0.00001);
                assert!((uv[1] - 0.45).abs() < 0.00001);
                assert!((uv[0] - if point[0] < 0.0 { 0.225 } else { 0.775 }).abs() < 0.00001);
            }
        }
        GeometryData { parts: vec![clipped] }.validate().unwrap();
        assert!(source.clipped(Vec4::new(0.0, 0.0, -1.0, -2.0)).triangles.is_empty());
        assert_eq!(source.positions_m[2], [0.0, 2.0, 1.0], "source geometry must stay intact for the next view");
    }

    #[test]
    fn rejects_mismatched_and_invalid_mesh_attributes() {
        let mut data = GeometryData {
            parts: vec![MeshPart {
                name: "triangle".into(),
                positions_m: vec![[0.0; 3]; 3],
                uv: vec![[0.0; 2]; 3],
                colors: vec![[1.0; 4]; 3],
                triangles: vec![[0, 1, 2]],
                ..default()
            }],
        };
        assert!(data.validate().is_ok());
        let ron = ron::ser::to_string(&data).unwrap();
        assert_eq!(ron::from_str::<GeometryData>(&ron).unwrap(), data);
        data.parts[0].triangles[0][2] = 3;
        assert!(data.validate().unwrap_err().contains("triangle index"));
        data.parts[0].triangles.clear();
        data.parts[0].colors.pop();
        assert!(data.validate().unwrap_err().contains("attribute lengths"));
    }
}
