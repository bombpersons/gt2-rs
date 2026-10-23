//! The separate binary drives and renders using only handmade assets.
#![cfg(feature = "dev-mcp")]
use engine::testing::mcp::Client;
use serde_json::{Value, json};
use std::process::Command;

fn copy_assets(source: &std::path::Path, destination: &std::path::Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_assets(&entry.path(), &target);
        } else {
            assert!(entry.file_type().unwrap().is_file());
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn query(client: &mut Client, name: &str, components: &[&str]) -> Value {
    client.call("game_query", json!({"name":name,"components":components}))["structuredContent"]["entities"][0].clone()
}
fn x(entity: &Value) -> f64 {
    entity["components"]["Transform"]["translation"][0].as_f64().unwrap()
}
fn speed(entity: &Value) -> f64 {
    entity["components"]["VehicleVelocity"]["linear_mps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap().powi(2))
        .sum::<f64>()
        .sqrt()
}

#[test]
#[ignore = "requires a working offscreen GPU/software adapter; no display needed"]
fn the_mod_drives_its_handmade_car_and_circuit() {
    let output = std::env::temp_dir().join(format!("psx-example-driving-{}", std::process::id()));
    std::fs::create_dir_all(&output).unwrap();
    println!("GPU evidence: {}", output.display());
    // Run with a copy of the handmade assets in a fresh directory tree. The
    // normal ../../game/assets fallback does not exist here, even on a
    // developer machine with private original exports in the real checkout.
    let source_root = output.join("source");
    let mod_root = source_root.join("mods/example");
    copy_assets(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets"), &mod_root.join("assets"));
    assert!(!source_root.join("game/assets").exists());
    let mut command = Command::new(env!("CARGO_BIN_EXE_example-mod"));
    command.env("BEVY_ASSET_ROOT", &mod_root).current_dir(&source_root);
    let mut client = Client::launch(command, &output.join("mod.log"));
    let body_before = query(&mut client, "CameraSubject", &["Transform"]);
    let wheel_before = query(&mut client, "front left wheel", &["Transform"]);
    assert!((x(&body_before) - 50.0).abs() < 0.001);
    let image = client.screenshot(output.join("circuit.png"));
    assert_eq!(image.dimensions(), (game::SCREEN.x, game::SCREEN.y));
    let colours: std::collections::HashSet<_> = image.pixels().map(|p| p.0).collect();
    assert!(colours.len() > 32, "the authored circuit and body should render");

    client.call("game_step", json!({"frames":30,"buttons":["cross"]}));
    let body = query(&mut client, "CameraSubject", &["Transform", "VehicleVelocity"]);
    let camera = query(&mut client, "RaceCamera", &["Transform"]);
    let wheel = query(&mut client, "front left wheel", &["Transform"]);
    assert!(x(&body) < x(&body_before) - 5.0, "native controller must drive the rendered body");
    assert!(speed(&body) > 8.0, "authored engine must accelerate the car");
    assert!((x(&camera) - x(&body) - 9.0).abs() < 0.01, "camera follows the driven subject");
    assert_ne!(
        wheel_before["components"]["Transform"]["rotation"], wheel["components"]["Transform"]["rotation"],
        "wheels must roll"
    );
    client.screenshot(output.join("accelerating.png"));

    client.call("game_step", json!({"frames":1,"buttons":["cross","r1"]}));
    let far = query(&mut client, "RaceCamera", &["Transform"]);
    let subject = query(&mut client, "CameraSubject", &["Transform"]);
    assert!((x(&far) - x(&subject) - 12.0).abs() < 0.01, "R1 selects the authored far view");
    client.call("game_step", json!({"frames":30,"buttons":["cross","left"]}));
    let turning = query(
        &mut client,
        "CameraSubject",
        &["Transform", "VehiclePose", "VehicleVelocity", "WheelSteering", "ChaseStabilization", "Suspension"],
    );
    assert!(turning["components"]["VehiclePose"]["heading_turns"].as_f64().unwrap() > 0.255, "{turning}");
    assert!(turning["components"]["WheelSteering"]["angle_turns"][0].as_f64().unwrap() > 0.01);
    client.screenshot(output.join("turning.png"));
    client.call("game_step", json!({"frames":45,"buttons":["square"]}));
    let braking =
        query(&mut client, "CameraSubject", &["Transform", "VehiclePose", "VehicleVelocity", "RoadContactState"]);
    assert!(speed(&braking) < speed(&turning) - 1.0, "service brakes must slow the driven car");
    assert_eq!(braking["components"]["RoadContactState"]["missing_mask"], 0);
    client.screenshot(output.join("braking.png"));
    // Inspect the independently authored tire/hub and simulation attachment
    // from a close oblique view after the driving checks have finished.
    use bevy::prelude::{Transform, Vec3};
    let xyz = braking["components"]["Transform"]["translation"].as_array().unwrap();
    let position =
        Vec3::new(xyz[0].as_f64().unwrap() as f32, xyz[1].as_f64().unwrap() as f32, xyz[2].as_f64().unwrap() as f32);
    let detail =
        Transform::from_translation(position + Vec3::new(3.5, 1.6, 3.5)).looking_at(position + Vec3::Y * 0.4, Vec3::Y);
    client.call(
        "game_set_component",
        json!({"name":"RaceCamera","component":"Transform","fields":{
        "translation":detail.translation.to_array(),"rotation":detail.rotation.to_array()}}),
    );
    client.screenshot(output.join("wheel-detail.png"));
    std::fs::write(
        output.join("driving.json"),
        serde_json::to_vec_pretty(&json!({"turning":turning,"braking":braking})).unwrap(),
    )
    .unwrap();
}
