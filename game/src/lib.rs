//! This game, as a library: `src/main.rs` launches it; tests in `tests/`
//! drive the same plugins headless (`engine::testing`).
//!
//! Layout: one plugin per game feature (course, car, HUD, menus...), listed
//! in [`GamePlugins`]. Per-type controllers register with the engine's object
//! type registry; rules plugins stay free of rendering so they run in tests.
//! Reusable machinery belongs in `engine/` (see its AGENTS.md).
//!
//! Mods are crates built on this library (see `mods/example`): keep what a
//! mod would reuse `pub`, and each feature plugin usable on its own.
pub mod race;
pub mod vehicle;

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;
use engine::prelude::*;

/// Window title.
pub const TITLE: &str = "Gran Turismo 2 — B-1 licence course";

/// The original game's video region. It sets the display rate and, with
/// [`FIELDS_PER_TICK`], the logic tick rate. Mega Man 8 and Croc (PAL) use
/// `Region::Pal`. `--region ntsc|pal` overrides it at run time.
pub const REGION: Region = Region::Ntsc;

/// The race body and driving-camera rules run every two video fields:
/// 30 Hz on this NTSC disc. Recorded camera writes occur at fields 0,2,4...
/// Presentation stays 60 Hz. Menus will require their own per-field update.
pub const FIELDS_PER_TICK: u32 = 2;

/// The original's display size in pixels: the PS1 screen target and, at 3x,
/// the window. Most games show 320x240; check the original's display mode.
pub const SCREEN: UVec2 = UVec2::new(320, 240);

/// The logic tick rate for a region.
pub fn tick_rate(region: Region) -> TickRate {
    TickRate::every_nth_field(region, FIELDS_PER_TICK)
}

/// The whole game, as a group so mods can disable or replace features:
/// `GamePlugins.build().disable::<race::RacePlugin>()`.
pub struct GamePlugins;

impl PluginGroup for GamePlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(EnginePlugin)
            .add(race::camera::CameraRulesPlugin)
            .add(race::RacePlugin)
            .add(vehicle::VehicleRulesPlugin)
            .add(vehicle::aerodynamics::AerodynamicsRulesPlugin)
            .add(vehicle::engine_torque::EngineTorqueRulesPlugin)
            .add(vehicle::slip::WheelSlipRulesPlugin)
            .add(vehicle::longitudinal_slip::LongitudinalSlipRulesPlugin)
            .add(vehicle::braking::BrakingRulesPlugin)
            .add(vehicle::brake_assist::BrakeAssistRulesPlugin)
            .add(vehicle::driver_assist::DriverAssistRulesPlugin)
            .add(vehicle::tyre_forces::TyreForcesRulesPlugin)
            .add(vehicle::forward_dynamics::ForwardDynamicsRulesPlugin)
            .add(vehicle::wheel_rotation::WheelRotationRulesPlugin)
            .add(vehicle::drivetrain::DrivetrainRulesPlugin)
            .add(vehicle::transmission::TransmissionRulesPlugin)
            .add(vehicle::body_dynamics::BodyDynamicsRulesPlugin)
            .add(vehicle::tyre_grip::TyreGripRulesPlugin)
            .add(vehicle::contact_motion::ContactMotionRulesPlugin)
            .add(vehicle::suspension::SuspensionRulesPlugin)
            .add(vehicle::vertical_body::VerticalBodyRulesPlugin)
            .add(vehicle::road_contact::RoadContactRulesPlugin)
            .add(vehicle::visual_motion::VehicleVisualMotionPlugin)
            .add(vehicle::presentation::VehiclePresentationPlugin)
    }
}

/// Compatibility wrapper for callers that added the game as one plugin.
/// Mods should add [`GamePlugins`] directly to select individual features.
pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(GamePlugins);
    }
}
