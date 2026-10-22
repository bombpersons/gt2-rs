//! Shared sampled direction used by wheel slip and road orientation.

/// Direction of (forward, sideways), with the original /4096 scalar sampling
/// and a table-defined first-octant curve. Both inputs use the same units.
pub fn direction_turns(thresholds: &[f32], forward: f32, sideways: f32) -> f32 {
    let [forward, sideways] = direction_sample(forward, sideways);
    let major = forward.abs().max(sideways.abs());
    if major == 0 {
        return 0.0;
    }
    // 80082E54..E6C quantizes the minor/major ratio to 1/4096 before
    // consulting 800A4AC8. A standard atan differs at 133 table entries.
    let ratio = (forward.abs().min(sideways.abs()) * 4096 / major) as f32 / 4096.0;
    let angle = thresholds.partition_point(|&threshold| threshold <= ratio) as f32 / 4096.0;
    let direction = if sideways.abs() > forward.abs() { 0.25 - angle } else { angle };
    // 800A2A8C/800A2A94 reconstruct the other octants. Exactly backward
    // is +half a turn; negative sideways keeps the negative representative.
    let direction = if forward < 0 { 0.5 - direction } else { direction };
    if sideways < 0 { -direction } else { direction }
}

/// Sample a direction on the original signed velocity grid, reducing large
/// vectors to 14 magnitude bits while preserving their ratio. The shared
/// scale uses leading sign bits, so a negative exact power of two fits one
/// more bit than its positive counterpart. Discarded negative fractions floor
/// rather than truncate (`80081AF8..80081B44`); this affects slip by one angle
/// unit around curve boundaries. Integer semantics stay at this input boundary.
fn direction_sample(forward: f32, sideways: f32) -> [i32; 2] {
    let samples = [forward, sideways].map(|value| (value as f64 * 4096.0).trunc() as i32);
    let magnitude_bits = samples
        .iter()
        .map(|&value| {
            let sign_magnitude = if value < 0 { !value } else { value } as u32;
            32 - sign_magnitude.leading_zeros()
        })
        .max()
        .unwrap_or(0);
    samples.map(|value| value >> magnitude_bits.saturating_sub(14))
}
