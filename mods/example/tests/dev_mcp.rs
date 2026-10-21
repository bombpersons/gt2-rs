//! The separate mod binary must load and render its own course.
#![cfg(feature = "dev-mcp")]
use engine::testing::mcp::Client;
use serde_json::json;
use std::process::Command;

#[test]
#[ignore = "requires a working offscreen GPU/software adapter; no display needed"]
fn the_mod_renders_its_own_circuit() {
    let output = std::env::temp_dir().join(format!("psx-example-mod-{}", std::process::id()));
    std::fs::create_dir_all(&output).unwrap();
    println!("GPU evidence: {}", output.display());
    let mut command = Command::new(env!("CARGO_BIN_EXE_example-mod"));
    command.env("BEVY_ASSET_ROOT", env!("CARGO_MANIFEST_DIR"));
    let mut client = Client::launch(command, &output.join("mod.log"));
    client.call("game_step", json!({"frames":2}));
    let image = client.screenshot(output.join("circuit.png"));
    assert_eq!(image.dimensions(), (game::SCREEN.x, game::SCREEN.y));
    let camera = client.call("game_query", json!({"name":"RaceCamera","components":["Transform"]}));
    let at = &camera["structuredContent"]["entities"][0]["components"]["Transform"]["translation"];
    assert!((at[0].as_f64().unwrap() - 58.7914).abs() < 0.001);
    assert_eq!(at[2], 80.0);
    let colours: std::collections::HashSet<_> = image.pixels().map(|p| p.0).collect();
    assert!(colours.len() > 32, "the circuit should render");
    // Same public controller, hand-authored mod offsets: R1 chooses the
    // 12 m far view. It follows an ordinary subject Transform without cars,
    // imported models or vehicle physics.
    client.call("game_step", json!({"frames":1,"buttons":["r1"]}));
    let far = client.call("game_query", json!({"name":"RaceCamera","components":["Transform"]}));
    let at = &far["structuredContent"]["entities"][0]["components"]["Transform"]["translation"];
    assert!((at[0].as_f64().unwrap() - 61.7914).abs() < 0.001);
    client.call(
        "game_set_component",
        json!({"name":"CameraSubject","component":"Transform","fields":{"translation":[10.0,2.0,30.0]}}),
    );
    client.call("game_step", json!({"frames":1,"buttons":[]}));
    let moved = client.call("game_query", json!({"name":"RaceCamera","components":["Transform"]}));
    let at = &moved["structuredContent"]["entities"][0]["components"]["Transform"]["translation"];
    for (i, expected) in [22.0, 5.4, 30.0].into_iter().enumerate() {
        assert!((at[i].as_f64().unwrap() - expected).abs() < 0.001);
    }
    client.screenshot(output.join("camera-follow.png"));
}
