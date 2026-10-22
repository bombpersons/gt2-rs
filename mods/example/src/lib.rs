//! A mod using the game's course renderer with a hand-authored circuit.
//!
//! Its asset at the game's course path wins over the base game's folder.
//! There is no driving controller in the port yet; grow this example with
//! the real controller once its recorded trajectories pass.

/// Layer the base game's assets below this crate's own `assets/`.
/// Relative to Bevy's asset root (the mod crate when run through Cargo).
pub const GAME_ASSETS: &str = "../../game/assets";

/// This authored scene uses camera and geometry, with no private dynamics
/// tuning. Keep the binary and its headless checks on the same feature set.
pub fn plugins() -> bevy::app::PluginGroupBuilder {
    use bevy::prelude::PluginGroup;
    game::GamePlugins
        .build()
        .disable::<game::vehicle::VehicleRulesPlugin>()
        .disable::<game::vehicle::engine_torque::EngineTorqueRulesPlugin>()
        .disable::<game::vehicle::slip::WheelSlipRulesPlugin>()
        .disable::<game::vehicle::longitudinal_slip::LongitudinalSlipRulesPlugin>()
        .disable::<game::vehicle::braking::BrakingRulesPlugin>()
        .disable::<game::vehicle::brake_assist::BrakeAssistRulesPlugin>()
        .disable::<game::vehicle::tyre_forces::TyreForcesRulesPlugin>()
        .disable::<game::vehicle::forward_dynamics::ForwardDynamicsRulesPlugin>()
        .disable::<game::vehicle::wheel_rotation::WheelRotationRulesPlugin>()
        .disable::<game::vehicle::drivetrain::DrivetrainRulesPlugin>()
}
