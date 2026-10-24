//! A mod using the game's course renderer with a hand-authored circuit.
//!
//! Its asset at the game's course path wins over the base game's folder.
//! The public vehicle controller drives the handmade box car using authored
//! tuning, initial state and collision. No original assets are required.

/// Layer the base game's assets below this crate's own `assets/`.
/// Relative to Bevy's asset root (the mod crate when run through Cargo).
pub const GAME_ASSETS: &str = "../../game/assets";

/// Shared driving/session features with the game's licence HUD disabled.
pub fn plugins() -> bevy::app::PluginGroupBuilder {
    use bevy::prelude::PluginGroup;
    game::GamePlugins.build().disable::<game::race::hud::HudPlugin>().disable::<game::race::licence::LicencePlugin>()
}
