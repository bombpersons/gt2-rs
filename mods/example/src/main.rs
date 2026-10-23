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
            // Its own layered tuning, initial state and collision feed the
            // same numerical controller as the base game.
            app.add_plugins(example_mod::plugins());
        },
    });
}
