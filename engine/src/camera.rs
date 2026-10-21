//! Authorable camera lenses, including independent horizontal/vertical focal
//! lengths. Ordinary Bevy cameras and transforms; no original GPU state.
use bevy::{
    camera::{CameraProjection, SubCameraView},
    math::Vec3A,
    prelude::*,
};
use serde::{Deserialize, Serialize};

/// Focal lengths and optical centre in reference image pixels. Resizing
/// preserves vertical framing and expands the horizontal view. This is also
/// useful for cameras with non-square source pixels or an off-centre lens.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LensData {
    pub reference_size: [f32; 2],
    pub focal_px: [f32; 2],
    pub centre_px: [f32; 2],
    pub near_m: f32,
    pub far_m: f32,
}

impl LensData {
    pub fn projection(&self) -> Result<Projection, String> {
        if self.reference_size.iter().chain(&self.focal_px).any(|v| !v.is_finite() || *v <= 0.0)
            || self.centre_px.iter().any(|v| !v.is_finite())
            || !self.near_m.is_finite()
            || !self.far_m.is_finite()
            || self.near_m <= 0.0
            || self.far_m <= self.near_m
        {
            return Err("invalid camera lens dimensions, focal length or clipping range".into());
        }
        Ok(Projection::custom(LensProjection {
            perspective: PerspectiveProjection {
                fov: 2.0 * (self.reference_size[1] / (2.0 * self.focal_px[1])).atan(),
                aspect_ratio: self.reference_size[0] / self.reference_size[1],
                near: self.near_m,
                far: self.far_m,
                ..default()
            },
            horizontal_gain: self.focal_px[0] / self.focal_px[1],
            centre: Vec2::from_array(self.centre_px) / Vec2::from_array(self.reference_size),
        }))
    }
}

#[derive(Clone, Debug)]
struct LensProjection {
    perspective: PerspectiveProjection,
    horizontal_gain: f32,
    centre: Vec2,
}

impl LensProjection {
    fn adjust(&self, mut matrix: Mat4, full_over_sub: Vec2) -> Mat4 {
        matrix.x_axis.x *= self.horizontal_gain;
        matrix.z_axis.x += (1.0 - 2.0 * self.centre.x) * full_over_sub.x;
        matrix.z_axis.y += (2.0 * self.centre.y - 1.0) * full_over_sub.y;
        matrix
    }
}

impl CameraProjection for LensProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        self.adjust(self.perspective.get_clip_from_view(), Vec2::ONE)
    }

    fn get_clip_from_view_for_sub(&self, sub: &SubCameraView) -> Mat4 {
        self.adjust(self.perspective.get_clip_from_view_for_sub(sub), sub.full_size.as_vec2() / sub.size.as_vec2())
    }

    fn update(&mut self, width: f32, height: f32) {
        self.perspective.update(width, height);
    }

    fn far(&self) -> f32 {
        self.perspective.far
    }

    fn get_frustum_corners(&self, near: f32, far: f32) -> [Vec3A; 8] {
        let mut corners = self.perspective.get_frustum_corners(near, far);
        let tangent = (self.perspective.fov / 2.0).tan();
        for corner in &mut corners {
            let half_height = corner.z.abs() * tangent;
            corner.x = (corner.x + (1.0 - 2.0 * self.centre.x) * half_height * self.perspective.aspect_ratio)
                / self.horizontal_gain;
            corner.y += (2.0 * self.centre.y - 1.0) * half_height;
        }
        corners
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_focal_lengths_and_optical_centre_project_in_pixels() {
        let data = LensData {
            reference_size: [320.0, 240.0],
            focal_px: [200.0, 180.0],
            centre_px: [148.0, 132.0],
            near_m: 0.1,
            far_m: 1000.0,
        };
        let projection = data.projection().unwrap();
        let clip = projection.get_clip_from_view() * Vec4::new(2.0, 3.0, -10.0, 1.0);
        let pixel = Vec2::new((clip.x / clip.w + 1.0) * 160.0, (1.0 - clip.y / clip.w) * 120.0);
        assert!((pixel - Vec2::new(188.0, 78.0)).length() < 0.0001);
        for (i, point) in projection.get_frustum_corners(-1.0, -10.0).into_iter().enumerate() {
            let clip = projection.get_clip_from_view() * point.extend(1.0);
            let expected = [Vec2::new(1.0, -1.0), Vec2::ONE, Vec2::new(-1.0, 1.0), -Vec2::ONE][i % 4];
            assert!((clip.truncate().truncate() / clip.w - expected).length() < 0.0001);
        }

        let sub = SubCameraView {
            full_size: UVec2::new(320, 240),
            offset: Vec2::new(80.0, 30.0),
            size: UVec2::new(160, 120),
        };
        let clip = projection.get_clip_from_view_for_sub(&sub) * Vec4::new(2.0, 3.0, -10.0, 1.0);
        let pixel = Vec2::new((clip.x / clip.w + 1.0) * 80.0, (1.0 - clip.y / clip.w) * 60.0);
        assert!((pixel - Vec2::new(108.0, 48.0)).length() < 0.0001);

        let mut wide = projection;
        wide.update(640.0, 240.0);
        let clip = wide.get_clip_from_view() * Vec4::new(2.0, 3.0, -10.0, 1.0);
        let pixel = Vec2::new((clip.x / clip.w + 1.0) * 320.0, (1.0 - clip.y / clip.w) * 120.0);
        assert!((pixel - Vec2::new(336.0, 78.0)).length() < 0.0001);
    }
}
