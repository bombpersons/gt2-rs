//! Raster-space coordinate precision and rounded projection shared by
//! material detail selection and adaptive geometry.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RasterPrecision {
    pub position_step_m: f32,
    pub centre_m: [f32; 3],
    pub radius_m: f32,
    /// Distances for two and one extra bits of coordinate precision.
    pub distances_m: [f32; 2],
}

impl RasterPrecision {
    pub fn validate(&self) -> Result<(), String> {
        let [near, far] = self.distances_m;
        if !self.position_step_m.is_finite()
            || self.position_step_m <= 0.0
            || self.centre_m.iter().any(|v| !v.is_finite())
            || !self.radius_m.is_finite()
            || self.radius_m < 0.0
            || !near.is_finite()
            || !far.is_finite()
            || near <= 0.0
            || far <= near
        {
            return Err("invalid raster precision parameters".into());
        }
        Ok(())
    }

    pub fn position_step(&self, eye_m: Vec3) -> f32 {
        let distance_squared = eye_m.distance_squared(Vec3::from_array(self.centre_m)) + self.radius_m.powi(2);
        let extra_bits = if distance_squared < self.distances_m[0].powi(2) {
            2
        } else if distance_squared < self.distances_m[1].powi(2) {
            1
        } else {
            0
        };
        self.position_step_m / (1u32 << extra_bits) as f32
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Projected {
    pub pixel: Vec2,
    pub outside: u8,
    pub failed: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct RasterProjection {
    pub size: Vec2,
    pub focal_px: f32,
    pub position_step: f32,
    pub guard_band_px: f32,
}
impl RasterProjection {
    pub fn pixel(&self, point: Vec3) -> Vec2 {
        self.project(point, false).pixel
    }
    pub(crate) fn project(&self, point: Vec3, original: bool) -> Projected {
        let near = self.focal_px * self.position_step * 0.5;
        // The source projection rounds its perspective quotient to 16
        // fractional bits before multiplying coordinates. This affects
        // single-pixel stitch decisions even when ordinary division has an
        // error below one pixel (see the recorded packet comparisons).
        let ratio = if point.z <= near {
            131071.0
        } else {
            (f64::from(self.focal_px) * f64::from(self.position_step) / f64::from(point.z) * 65536.0)
                .round()
                .min(131071.0)
        };
        let factor = (ratio / (65536.0 * f64::from(self.position_step))) as f32;
        let pixel = (point.truncate() * factor + self.size * 0.5).floor();
        let margin = if original { self.guard_band_px } else { 0.0 };
        let mut outside = u8::from(point.z < if original { self.position_step } else { near });
        if pixel.x < -margin {
            outside |= 2;
        }
        if pixel.x > self.size.x - 1.0 + margin {
            outside |= 4;
        }
        if pixel.y < -margin {
            outside |= 8;
        }
        if pixel.y > self.size.y - 1.0 + margin {
            outside |= 16;
        }
        Projected {
            pixel: pixel.clamp(Vec2::splat(-1024.0), Vec2::splat(1023.0)),
            outside,
            failed: point.z <= near || pixel.min_element() < -1024.0 || pixel.max_element() > 1023.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn precision_changes_at_bound_distance_thresholds() {
        let settings = RasterPrecision {
            position_step_m: 1.0 / 64.0,
            centre_m: [0.0; 3],
            radius_m: 0.0,
            distances_m: [128.0, 256.0],
        };
        settings.validate().unwrap();
        for (x, step) in [(127.0, 1.0 / 256.0), (128.0, 1.0 / 128.0), (255.0, 1.0 / 128.0), (256.0, 1.0 / 64.0)] {
            assert_eq!(settings.position_step(Vec3::X * x), step);
        }
    }
}
