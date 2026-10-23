//! A mod using the game's course renderer with a hand-authored circuit.
//!
//! Its asset at the game's course path wins over the base game's folder.
//! The public vehicle controller drives the handmade box car using authored
//! tuning, initial state and collision. No original assets are required.

/// Layer the base game's assets below this crate's own `assets/`.
/// Relative to Bevy's asset root (the mod crate when run through Cargo).
pub const GAME_ASSETS: &str = "../../game/assets";

/// The same public feature group as the game, with the example's asset layer.
pub fn plugins() -> bevy::app::PluginGroupBuilder {
    use bevy::prelude::PluginGroup;
    game::GamePlugins.build()
}
