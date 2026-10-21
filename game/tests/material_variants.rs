//! The original's actual material load address is the selection oracle.
use bevy::prelude::*;
use engine::geometry::{RasterProjection, selects_small_texture};
use serde::Deserialize;

#[derive(Deserialize)]
struct Capture {
    trace_lines_dropped: usize,
    records: Vec<Record>,
}

#[derive(Deserialize)]
struct Record {
    source_address: String,
    viewport: Viewport,
    pixels: Vec<[i32; 2]>,
    positions: Vec<[i32; 3]>,
    threshold: u32,
    selected_variant: u8,
    projection_failed: bool,
}

#[derive(Deserialize)]
struct Viewport {
    width: u32,
    height: u32,
    focal_px: u32,
}

#[test]
#[cfg_attr(
    not(gt2_private_data),
    ignore = "requires locally generated disc assets/reference fixtures; see docs/assets.md"
)]
fn source_parent_texture_selections_match_including_final_far_holdout() {
    for (name, json, count) in [
        (
            "bumper",
            std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/data/original/material_variants/bumper.json"
            ))
            .unwrap(),
            91,
        ),
        (
            "chase",
            std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/data/original/material_variants/chase.json"
            ))
            .unwrap(),
            84,
        ),
        (
            "far_holdout",
            std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/data/original/material_variants/far_holdout.json"
            ))
            .unwrap(),
            80,
        ),
    ] {
        let capture: Capture = serde_json::from_str(&json).unwrap();
        assert_eq!(capture.trace_lines_dropped, 0);
        assert_eq!(capture.records.len(), count);
        let mut failed_projections = 0;
        let mut small_count = 0;
        let mut max_pixel_error = 0.0_f32;
        for record in capture.records {
            let expected_small = record.selected_variant == 1;
            let pixels: Vec<_> = record.pixels.iter().map(|&p| Vec2::new(p[0] as f32, p[1] as f32)).collect();
            assert_eq!(
                selects_small_texture(record.threshold, &pixels),
                expected_small,
                "{name} {} recorded pixels",
                record.source_address
            );
            let projection = RasterProjection {
                size: Vec2::new(record.viewport.width as f32, record.viewport.height as f32),
                focal_px: record.viewport.focal_px as f32,
                position_step: 1.0,
                guard_band_px: 0.0,
            };
            let projected: Vec<_> = record
                .positions
                .iter()
                .map(|&p| projection.pixel(Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)))
                .collect();
            for (actual, expected) in projected.iter().zip(&pixels) {
                max_pixel_error = max_pixel_error.max((*actual - *expected).abs().max_element());
            }
            assert_eq!(
                selects_small_texture(record.threshold, &projected),
                expected_small,
                "{name} {} native projection",
                record.source_address
            );
            small_count += usize::from(expected_small);
            failed_projections += usize::from(record.projection_failed);
        }
        assert!(small_count > 0 && small_count < count, "both variants must execute");
        assert!(failed_projections > 0, "projection-failure selection must execute");
        assert!(max_pixel_error <= 1.0, "{name} pixel error {max_pixel_error}");
        println!(
            "{name}: {count} selections, {small_count} small, {failed_projections} projection failures, max pixel error {max_pixel_error}"
        );
    }
}
