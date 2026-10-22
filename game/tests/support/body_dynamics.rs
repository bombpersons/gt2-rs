//! Shared private-recording boundary conversion and assertions.
use bevy::prelude::*;
use game::vehicle::{VehicleVelocity, body_dynamics::*, forward_dynamics::ForwardDynamics};
use std::collections::HashMap;
type Row = HashMap<String, i32>;
pub fn compare(
    r: &Row,
    phase: &str,
    body: BodyDynamics,
    velocity: VehicleVelocity,
    forward: ForwardDynamics,
    label: &str,
) {
    for (key, actual) in [
        ("yaw", velocity.yaw_turns_per_second * 524288.0),
        ("vx", velocity.linear_mps.x * 4096.0),
        ("vy", -velocity.linear_mps.z * 4096.0),
        ("vz", velocity.linear_mps.y * 4096.0),
        ("secondary0", body.tilt_turns_per_second.x * 65536.0),
        ("secondary1", body.tilt_turns_per_second.y * 65536.0),
        ("feedback", body.lateral_feedback_turns * 4096.0),
        ("lateral_acceleration", body.lateral_acceleration_mps2 * 4096.0),
        ("lateral_speed", body.local_lateral_mps * 4096.0),
        ("forward_acceleration", forward.acceleration_mps2 * 4096.0),
        ("forward_speed", forward.forward_mps * 4096.0),
        ("force", forward.force),
    ] {
        assert_eq!(actual, r[&format!("bd_{phase}_{key}")] as f32, "{label} {phase}: {key}");
    }
}

pub fn original_inputs(r: &Row) -> (BodyDynamicsInput, VehicleVelocity, BodyDynamics) {
    let before = |key: &str| r[&format!("bd_before_{key}")] as f32;
    let input = BodyDynamicsInput {
        bypass_tyre_acceleration: before("bypass") != 0.0,
        reset_motion: before("reset") != 0.0 || before("forced_mode") == 7.0,
        first_gear_override: before("first") != 0.0,
        braked_wheels: std::array::from_fn(|i| r[&format!("bd_before_flags{i}")] & 2 != 0),
        // Native WheelSlip must override this fallback in the connected test.
        slip_activation: [-1.0; 4],
        forward_axis: Vec2::new(before("forward_axis0"), -before("forward_axis1")) / 4096.0,
        lateral_axis: Vec2::new(before("lateral_axis0"), -before("lateral_axis1")) / 4096.0,
        lateral_resistance: before("lateral_resistance"),
        external_world_force: Vec2::new(before("external0"), -before("external1")),
    };
    let velocity = VehicleVelocity {
        linear_mps: Vec3::new(before("vx"), before("vz"), -before("vy")) / 4096.0,
        yaw_turns_per_second: before("yaw") / 524288.0,
    };
    let body = BodyDynamics {
        local_lateral_mps: before("lateral_speed") / 4096.0,
        lateral_acceleration_mps2: before("lateral_acceleration") / 4096.0,
        lateral_feedback_turns: before("feedback") / 4096.0,
        tilt_turns_per_second: Vec2::new(before("secondary0"), before("secondary1")) / 65536.0,
        ..default()
    };
    (input, velocity, body)
}
