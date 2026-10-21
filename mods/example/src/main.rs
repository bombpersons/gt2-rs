//! A separate executable built on the game's public feature plugins.
use engine::launcher::{self, Game};

fn main() {
    engine::assets::add_root(example_mod::GAME_ASSETS);
    launcher::run(Game {
        name: env!("CARGO_PKG_NAME"),
        title: "Example mod — hand-made circuit",
        region: game::REGION,
        fields_per_tick: game::FIELDS_PER_TICK,
        screen: game::SCREEN,
        add: |app| {
            app.insert_resource(game::race::RacePresentation { geometry: None });
            // This authored scene uses the camera/geometry features only.
            // Disabled dynamics must not load the base game's private tuning.
            app.add_plugins(example_mod::plugins());
        },
    });
}
