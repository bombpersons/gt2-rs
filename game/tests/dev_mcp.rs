//! The B-1 course through the real binary: offscreen GPU and the developer
//! MCP, with a software adapter. `#[ignore]`d: it needs a working adapter.
#![cfg(feature = "dev-mcp")]

use engine::testing::mcp::Client;
use serde_json::{Value, json};
use std::process::Command;

fn query(client: &mut Client, name: &str) -> Value {
    client.call("game_query", json!({"name":name,"components":["Transform"]}))["structuredContent"]["entities"][0]
        .clone()
}

#[test]
#[ignore = "requires a working offscreen GPU/software adapter; no display needed"]
fn headless_render_edit_step_and_query() {
    let output = std::env::temp_dir().join(format!("psx-template-headless-{}", std::process::id()));
    std::fs::create_dir_all(&output).unwrap();
    println!("GPU evidence: {}", output.display());
    let mut command = Command::new(env!(concat!("CARGO_BIN_EXE_", env!("CARGO_PKG_NAME"))));
    command.env("BEVY_ASSET_ROOT", env!("CARGO_MANIFEST_DIR"));
    let mut client = Client::launch(command, &output.join("game.log"));
    let tools = client.request("tools/list", json!({}));
    assert!(tools["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "game_set_component"));
    let status = client.call("game_status", json!({}))["structuredContent"].clone();
    assert_eq!(status["paused"], true);
    assert_eq!(status["headless"], true);
    assert_eq!(status["frame"], 0);
    let window = client.request("tools/call", json!({"name":"game_screenshot","arguments":{"target":"window"}}));
    assert!(window["error"]["message"].as_str().unwrap().contains("No primary window"), "{window}");

    // The scene renders: a sky, a green ground plane and the grey road.
    let before = client.screenshot(output.join("before.png"));
    assert_eq!(before.dimensions(), (game::SCREEN.x, game::SCREEN.y));
    let colors: std::collections::HashSet<_> = before.pixels().map(|p| p.0).collect();
    assert!(colors.len() > 32, "offscreen frame should render the course, not an empty target");
    // The source bumper view sees asphalt across this near-road region.
    // Without clipping before affine projection, triangles crossing behind
    // the camera sampled transparent atlas padding and exposed green ground.
    let ground_holes = before
        .enumerate_pixels()
        .filter(|(x, y, p)| {
            (60..260).contains(x)
                && *y >= 180
                && f32::from(p[1]) > f32::from(p[0]) * 1.4
                && f32::from(p[1]) > f32::from(p[2]) * 1.4
        })
        .count();
    assert!(ground_holes < 100, "near road has {ground_holes} exposed ground pixels");

    // Original low bumper view, body pose from the B-1 checkpoint plus its
    // 0xCCCC/65536 metre local up offset. Projection is checked numerically
    // against the original in camera.rs.
    let camera = query(&mut client, "RaceCamera");
    let at = &camera["components"]["Transform"]["translation"];
    assert!((at[0].as_f64().unwrap() - 401.77837).abs() < 0.001, "camera x: {camera}");
    assert!((at[1].as_f64().unwrap() - 0.962997).abs() < 0.001, "camera y: {camera}");
    assert!((at[2].as_f64().unwrap() + 449.69).abs() < 2.0, "camera z: {camera}");

    // Editing the road's transform moves it, and the frame changes with it.
    let road = query(&mut client, "Road");
    assert_eq!(road["components"]["Transform"]["translation"], json!([0.0, 0.0, 0.0]));
    let edit = client.call(
        "game_set_component",
        json!({"entity":road["entity"],"component":"Transform","fields":{"translation":[0.0,1.0,0.0]}}),
    )["structuredContent"]
        .clone();
    assert_eq!(edit["frame"], 0);
    assert_eq!(edit["after"]["translation"], json!([0.0, 1.0, 0.0]));
    assert_eq!(query(&mut client, "Road")["components"]["Transform"]["translation"], json!([0.0, 1.0, 0.0]));
    let invalid = client.request("tools/call",json!({"name":"game_set_component","arguments":{"name":"Road","component":"Transform","fields":{"translation":[99,1,0],"zzz":1}}}));
    assert!(invalid.get("error").is_some(), "{invalid}");
    let after_edit = client.screenshot(output.join("after-edit.png"));
    assert!(
        before.pixels().zip(after_edit.pixels()).filter(|(a, b)| a != b).count() > 100,
        "moving the road should affect the rendered scene"
    );

    let initial_subject = query(&mut client, "CameraSubject");
    let initial_x = initial_subject["components"]["Transform"]["translation"][0].as_f64().unwrap();
    // The retained native controller drives the same subject the camera sees.
    let stepped = client.call("game_step", json!({"frames":12,"buttons":[]}))["structuredContent"].clone();
    assert_eq!(stepped["frame"], 12);
    assert!(query(&mut client, "Road")["components"]["Transform"]["translation"][1].as_f64().unwrap() == 1.0);
    client.screenshot(output.join("after-step.png"));
    let moved_subject = query(&mut client, "CameraSubject");
    assert!(moved_subject["components"]["Transform"]["translation"][0].as_f64().unwrap() < initial_x - 5.0);

    client.call("game_pause", json!({"paused":false}));
    let running = client.request("tools/call",json!({"name":"game_set_component","arguments":{"name":"Road","component":"Transform","fields":{"translation":[99,1,0]}}}));
    assert!(running["error"]["message"].as_str().unwrap().contains("pause"), "{running}");
    client.call("game_pause", json!({"paused":true}));
    // Restore the road before viewing the body: leaving it raised would
    // submerge most of the car and invalidate presentation screenshots.
    client.call(
        "game_set_component",
        json!({"name":"Road","component":"Transform","fields":{"translation":[0.0,0.0,0.0]}}),
    );
    // The live binary uses the public driving-camera rules. R1 cycles on a
    // press and stays in that view while held; L1 supplies rear look.
    client.call("game_step", json!({"frames":1,"buttons":["r1"]}));
    let chase = query(&mut client, "RaceCamera");
    let chase_x = chase["components"]["Transform"]["translation"][0].as_f64().unwrap();
    let subject_x = query(&mut client, "CameraSubject")["components"]["Transform"]["translation"][0].as_f64().unwrap();
    assert!((chase_x - subject_x - 5.4).abs() < 0.02, "near chase: {chase}");
    client.screenshot(output.join("chase.png"));
    client.call("game_step", json!({"frames":2,"buttons":["r1"]}));
    let held = query(&mut client, "RaceCamera");
    let subject_x = query(&mut client, "CameraSubject")["components"]["Transform"]["translation"][0].as_f64().unwrap();
    assert!(
        (held["components"]["Transform"]["translation"][0].as_f64().unwrap() - subject_x - 5.4).abs() < 0.02,
        "held button must not cycle again"
    );
    client.call("game_step", json!({"frames":1,"buttons":["l1"]}));
    let rear = query(&mut client, "RaceCamera");
    let subject_x = query(&mut client, "CameraSubject")["components"]["Transform"]["translation"][0].as_f64().unwrap();
    assert!(
        (rear["components"]["Transform"]["translation"][0].as_f64().unwrap() - subject_x + 5.4).abs() < 0.02,
        "rear chase: {rear}"
    );
    client.screenshot(output.join("rear.png"));
    client.call("game_step", json!({"frames":1,"buttons":["r1"]}));
    let far = query(&mut client, "RaceCamera");
    let subject_x = query(&mut client, "CameraSubject")["components"]["Transform"]["translation"][0].as_f64().unwrap();
    assert!(
        (far["components"]["Transform"]["translation"][0].as_f64().unwrap() - subject_x - 6.8).abs() < 0.02,
        "far chase: {far}"
    );
    client.screenshot(output.join("far.png"));
    let wheel_before = query(&mut client, "front left wheel");
    client.call("game_step", json!({"frames":12,"buttons":["cross","left"]}));
    let wheel_after = query(&mut client, "front left wheel");
    assert_ne!(
        wheel_before["components"]["Transform"]["rotation"], wheel_after["components"]["Transform"]["rotation"],
        "wheel presentation must follow live steering/spin"
    );
    let physics = client.call(
        "game_query",
        json!({"name":"CameraSubject","components":["VehiclePose","WheelSteering","ChaseStabilization"]}),
    );
    std::fs::write(output.join("physics.json"), serde_json::to_vec_pretty(&physics).unwrap()).unwrap();
    client.screenshot(output.join("driving.png"));
    std::fs::write(output.join("status.json"), serde_json::to_vec_pretty(&status).unwrap()).unwrap();
}

#[test]
#[ignore = "requires local original fixtures and a working offscreen GPU/software adapter"]
fn native_driving_frame_matches_the_recorded_body_origin() {
    let output = std::env::temp_dir().join(format!("gt2-live-reference-{}", std::process::id()));
    std::fs::create_dir_all(&output).unwrap();
    println!("Driving comparison: {}", output.display());
    let mut command = Command::new(env!(concat!("CARGO_BIN_EXE_", env!("CARGO_PKG_NAME"))));
    command.env("BEVY_ASSET_ROOT", env!("CARGO_MANIFEST_DIR"));
    let mut client = Client::launch(command, &output.join("game.log"));
    client.call("game_step", json!({"frames":30,"buttons":["cross","r1"]}));
    let expected: Value = serde_json::from_str(
        &std::fs::read_to_string(format!(
            "{}/tests/data/original/vehicle_presentation/gas.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap();
    let body = query(&mut client, "CameraSubject");
    for i in 0..3 {
        let actual = body["components"]["Transform"]["translation"][i].as_f64().unwrap();
        let original = expected["rows"][29]["presented"]["body_position"][i].as_f64().unwrap() / 65536.0;
        assert!((actual - original).abs() < 0.001, "body axis {i}: {actual} vs original {original}");
    }
    client.screenshot(output.join("driving-30.png"));
    std::fs::write(output.join("body.json"), serde_json::to_vec_pretty(&body).unwrap()).unwrap();
}
