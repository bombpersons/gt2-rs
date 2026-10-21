//! Temporary probe.
#![cfg(feature = "dev-mcp")]

use engine::testing::mcp::Client;
use serde_json::json;
use std::process::Command;

#[test]
#[ignore = "temporary probe"]
fn probe_ground() {
    let output = std::env::temp_dir().join(format!("psx-probe-{}", std::process::id()));
    std::fs::create_dir_all(&output).unwrap();
    println!("probe evidence: {}", output.display());
    let mut command = Command::new(env!(concat!("CARGO_BIN_EXE_", env!("CARGO_PKG_NAME"))));
    command.env("BEVY_ASSET_ROOT", env!("CARGO_MANIFEST_DIR"));
    let mut client = Client::launch(command, &output.join("game.log"));
    client.call("game_status", json!({}));
    let image = client.screenshot(output.join("shot.png"));
    let grey = image.pixels().filter(|p| p.0[0] > 80 && p.0[0] < 130 && p.0[1] > 80 && p.0[2] < 130).count();
    println!("grey {grey}");
}
