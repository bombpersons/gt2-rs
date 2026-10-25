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

fn licence_drive_case(name: &str) {
    let output = std::env::temp_dir().join(format!("gt2-licence-stop-{}", std::process::id()));
    std::fs::create_dir_all(&output).unwrap();
    println!("Licence stop evidence: {}", output.display());
    let buttons = |controls: &Value| {
        let mut buttons = Vec::new();
        for (key, button) in
            [("throttle", "cross"), ("brake", "square"), ("handbrake", "circle"), ("reverse", "triangle")]
        {
            if controls[key].as_i64().unwrap() != 0 {
                buttons.push(button);
            }
        }
        let steer = controls["steering"].as_i64().unwrap();
        if steer != 0 {
            buttons.push(if steer > 0 { "left" } else { "right" });
        }
        let shift = controls["shift"].as_i64().unwrap();
        if shift != 0 {
            buttons.push(if shift > 0 { "r2" } else { "l2" });
        }
        buttons
    };
    let expected = |outcome: &Value| match outcome["state"].as_u64().unwrap() {
        0 => json!("Active"),
        1 => json!({"Passed":{"time_ms":outcome["time_ms"]}}),
        2 => {
            json!({"Failed":{"reason":match outcome["reason"].as_u64().unwrap(){3=>"Overshot",4=>"OffRoad",5=>"Collision",x=>panic!("unknown reason {x}")}}})
        }
        x => panic!("unknown state {x}"),
    };
    {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/data/original/licence_stop/{name}.json"));
        let recording: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let rows = recording["rows"].as_array().unwrap();
        let transition = rows.iter().position(|r| r["before"]["state"] != r["after"]["state"]);
        let mut checkpoints = transition.map_or_else(Vec::new, |i| vec![i, i + 1]);
        checkpoints.push(rows.len());
        if name == "offroad" {
            checkpoints.extend((25..735).step_by(25));
        }
        checkpoints.sort_unstable();
        checkpoints.dedup();
        let mut command = Command::new(env!(concat!("CARGO_BIN_EXE_", env!("CARGO_PKG_NAME"))));
        command.env("BEVY_ASSET_ROOT", env!("CARGO_MANIFEST_DIR"));
        let mut client = Client::launch(command, &output.join(format!("{name}.log")));
        let mut cursor = 0;
        for checkpoint in checkpoints {
            while cursor < checkpoint {
                let held = buttons(&rows[cursor]["controls"]);
                let end =
                    (cursor + 1..checkpoint).find(|&i| buttons(&rows[i]["controls"]) != held).unwrap_or(checkpoint);
                client.call("game_step", json!({"frames":end-cursor,"buttons":held}));
                cursor = end;
            }
            let actual = client.call(
                "game_query",
                json!({"name":"CameraSubject","components":["LicenceRun","LicenceStopInput","VehiclePose","RoadContactState","AutomaticStop","PedalTravel"],"resources":["HudDisplay","LicenceResult"]}),
            )["structuredContent"]
                .clone();
            let run = &actual["entities"][0]["components"]["LicenceRun"];
            if name == "offroad" && checkpoint < 734 {
                let at = &actual["entities"][0]["components"]["VehiclePose"]["position_m"];
                let before = &rows[checkpoint - 1]["pose_m"];
                for axis in 0..3 {
                    assert!(
                        (at[axis].as_f64().unwrap() - before[axis].as_f64().unwrap()).abs() < 0.0001,
                        "{name} pose tick {checkpoint} expected {before}: {actual}"
                    );
                }
            }
            assert_eq!(*run, expected(&rows[checkpoint - 1]["after"]), "{name} tick {checkpoint}: {actual}");
            let result = &actual["resources"]["LicenceResult"];
            if *run == json!("Active") {
                assert_eq!(*result, json!("Driving"));
            } else {
                let timing: game::race::licence_results::LicenceResultTiming = ron::from_str(
                    &std::fs::read_to_string(
                        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/race/licence-results.ron"),
                    )
                    .unwrap(),
                )
                .unwrap();
                // These successful runs brake with Square; failed screens do
                // not accelerate. The natural source boundary starts at one
                // in the same tick that stopping latches the outcome.
                let age = checkpoint - transition.expect("completed attempt");
                let expected = if let Some(passed) = run.get("Passed") {
                    json!({"Success":{"time_ms":passed["time_ms"],"age_ticks":age.min(usize::from(timing.success_wait_ticks))}})
                } else {
                    json!({"Failure":{"reason":run["Failed"]["reason"],"age_ticks":age.min(usize::from(timing.failure_wait_ticks))}})
                };
                assert_eq!(*result, expected, "{name} result tick {checkpoint}: {actual}");
            }
            if name != "offroad" {
                let at = &actual["entities"][0]["components"]["VehiclePose"]["position_m"];
                let before = &rows[checkpoint - 1]["pose_m"];
                for axis in 0..3 {
                    assert!(
                        (at[axis].as_f64().unwrap() - before[axis].as_f64().unwrap()).abs() < 0.0001,
                        "{name} pose tick {checkpoint} expected {before}: {actual}"
                    );
                }
            }
            if *run != json!("Active") {
                assert!(actual["entities"][0]["components"]["AutomaticStop"].is_object());
            }
            if let Some(passed) = run.get("Passed") {
                assert_eq!(
                    actual["resources"]["HudDisplay"]["clock"],
                    game::race::licence::format_record_time(Some(passed["time_ms"].as_u64().unwrap() as u32), "")
                );
            } else if run.get("Failed").is_some() {
                assert_eq!(actual["resources"]["HudDisplay"]["clock"], "");
            }
        }
        client.screenshot(output.join(format!("{name}.png")));
        let actual = client.call(
            "game_query",
            json!({"name":"CameraSubject","components":["LicenceRun","LicenceStopInput","VehiclePose"]}),
        )["structuredContent"]
            .clone();
        println!("{name}: {}", actual["entities"][0]["components"]["LicenceRun"]);
        if actual["entities"][0]["components"]["LicenceRun"].get("Failed").is_some() {
            let glyphs = client.call("game_query", json!({"with":["HudGlyph"],"components":["Transform"],"limit":256}))
                ["structuredContent"]
                .clone();
            // Count the original label's actual glyph draws; its blank space
            // advances the cursor without emitting a sprite.
            let original: Value = serde_json::from_str(
                &std::fs::read_to_string(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/original/hud/initial.json"),
                )
                .unwrap(),
            )
            .unwrap();
            let label_glyphs = original["rows"][0]["draws"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|d| {
                    !d["large"].as_bool().unwrap() && d["character"].as_str().unwrap().chars().all(char::is_alphabetic)
                })
                .count();
            assert_eq!(
                glyphs["entities"].as_array().unwrap().len(),
                label_glyphs,
                "{name}: stale clock glyphs: {glyphs}"
            );
        }
        std::fs::write(output.join(format!("{name}.json")), serde_json::to_vec_pretty(&actual).unwrap()).unwrap();
        if name == "gold" || name == "overshoot" {
            let fixture = if name == "gold" {
                // The natural finish above ends at result age 33. Let the
                // prize settle without using confirm to accelerate it.
                client.call("game_step", json!({"frames":113,"buttons":["square"]}));
                "native-gold"
            } else {
                "fresh-fail"
            };
            let source: Value = serde_json::from_str(
                &std::fs::read_to_string(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/data/original/result_presentation/fresh.json"),
                )
                .unwrap(),
            )
            .unwrap();
            let row = source["rows"].as_array().unwrap().iter().find(|row| row["name"] == fixture).unwrap();
            let native = client.screenshot(output.join(format!("{name}-result.png")));
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
            // End-of-run scanout includes all of the retained GPU packet,
            // including glyphs that finish in the following video field.
            let original = image::open(
                root.join(std::path::Path::new(row["screens"][0].as_str().unwrap()).with_file_name("screen.png")),
            )
            .unwrap()
            .to_rgba8();
            let pixels: Vec<[u32; 2]> = serde_json::from_value(row["pixels"].clone()).unwrap();
            assert!(pixels.len() > 300);
            for [x, y] in pixels {
                let a = native.get_pixel(x, y);
                let b = original.get_pixel(x, y);
                for c in 0..3 {
                    assert!(a[c].abs_diff(b[c]) <= 9, "{name} result pixel {x},{y}: native {a:?} original {b:?}");
                }
            }
        }
    }
}

#[test]
#[ignore = "requires offscreen GPU and original licence fixtures"]
fn native_licence_stop_gold() {
    licence_drive_case("gold");
}
#[test]
#[ignore = "requires offscreen GPU and original licence fixtures"]
fn native_licence_stop_early() {
    licence_drive_case("early");
}
#[test]
#[ignore = "requires offscreen GPU and original licence fixtures"]
fn native_licence_stop_overshoot() {
    licence_drive_case("overshoot");
}
#[test]
#[ignore = "requires offscreen GPU and original licence fixtures"]
fn native_licence_stop_offroad() {
    licence_drive_case("offroad");
}
#[test]
#[ignore = "requires offscreen GPU and fresh original licence fixture"]
fn native_licence_stop_holdout() {
    licence_drive_case("holdout");
}

fn copy_assets(source: &std::path::Path, destination: &std::path::Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_assets(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn moving_reference_assets(output: &std::path::Path) -> std::path::PathBuf {
    let root = output.join("moving-reference");
    copy_assets(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets"), &root.join("assets"));
    std::fs::write(
        root.join("assets").join(game::race::RACE_VEHICLE_ASSET),
        ron::ser::to_string(&game::race::RaceVehicle::default()).unwrap(),
    )
    .unwrap();
    root
}

#[test]
#[ignore = "requires a working offscreen GPU/software adapter; no display needed"]
fn headless_render_edit_step_and_query() {
    let output = std::env::temp_dir().join(format!("psx-template-headless-{}", std::process::id()));
    std::fs::create_dir_all(&output).unwrap();
    println!("GPU evidence: {}", output.display());
    let mut command = Command::new(env!(concat!("CARGO_BIN_EXE_", env!("CARGO_PKG_NAME"))));
    command.env("BEVY_ASSET_ROOT", moving_reference_assets(&output));
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
    command.env("BEVY_ASSET_ROOT", moving_reference_assets(&output));
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

#[test]
#[ignore = "requires stationary original checkpoint/fixtures, generated assets and an offscreen GPU"]
fn native_idle_reverse_and_forward_replays_original_requests() {
    native_start_replay("standing-reverse-forward", "b1-stationary-checkpoint", None);
}

#[test]
#[ignore = "requires original countdown checkpoint/fixtures, generated assets and an offscreen GPU"]
fn native_countdown_to_launch_replays_original_requests() {
    native_start_replay("countdown-gas", "b1-countdown-checkpoint", Some(120));
}

#[test]
#[ignore = "requires exported default standing-start assets, original fixtures and an offscreen GPU"]
fn native_default_scene_starts_from_the_original_countdown() {
    let output = std::env::temp_dir().join(format!("gt2-default-start-{}", std::process::id()));
    std::fs::create_dir_all(&output).unwrap();
    println!("Default standing start: {}", output.display());
    let mut command = Command::new(env!(concat!("CARGO_BIN_EXE_", env!("CARGO_PKG_NAME"))));
    command.env("BEVY_ASSET_ROOT", env!("CARGO_MANIFEST_DIR"));
    let mut client = Client::launch(command, &output.join("game.log"));
    client.screenshot(output.join("before-start.png"));
    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/original/vehicle_loop/countdown-gas");
    replay_start(&mut client, &fixtures, &output);
}

#[test]
#[ignore = "requires original HUD frames, exported bitmap fonts and an offscreen GPU"]
fn native_hud_opaque_glyph_pixels_match_the_original() {
    let output = std::env::temp_dir().join(format!("gt2-hud-pixels-{}", std::process::id()));
    std::fs::create_dir_all(&output).unwrap();
    println!("HUD pixel evidence: {}", output.display());
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let data: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("game/tests/data/original/hud/initial.json")).unwrap())
            .unwrap();
    let mut command = Command::new(env!(concat!("CARGO_BIN_EXE_", env!("CARGO_PKG_NAME"))));
    command.env("BEVY_ASSET_ROOT", env!("CARGO_MANIFEST_DIR"));
    let mut client = Client::launch(command, &output.join("game.log"));
    let mut ticks = 0;
    let mut total_pixels = 0;
    let mut maximum_error = 0;
    for row in data["rows"].as_array().unwrap() {
        let frame = row["frame"].as_u64().unwrap() as u32;
        client.call("game_step", json!({"frames":frame/2-ticks,"buttons":[]}));
        ticks = frame / 2;
        let native = client.screenshot(output.join(format!("field-{frame}.png")));
        let original = image::open(root.join(row["screen"].as_str().unwrap())).unwrap().to_rgba8();
        // This mask comes from the original GPU texture/palette, not the
        // native atlas: missing native pixels must still fail comparison.
        let pixels: Vec<[u32; 2]> = serde_json::from_value(row["opaque_pixels"].clone()).unwrap();
        assert!(pixels.len() > 150, "clock/label pixels must be checked");
        for [x, y] in pixels {
            let a = native.get_pixel(x, y);
            let b = original.get_pixel(x, y);
            for c in 0..3 {
                let error = a[c].abs_diff(b[c]);
                maximum_error = maximum_error.max(error);
                assert!(error <= 9, "field {frame} pixel {x},{y}: native {a:?} original {b:?}");
            }
            total_pixels += 1;
        }
    }
    println!("{total_pixels} source-masked pixels; maximum encoded-channel error {maximum_error}");
}

#[test]
#[ignore = "requires original gauge frames, exported gauge atlas and an offscreen GPU"]
fn native_gauges_follow_vehicle_and_match_original_sprite_pixels() {
    let output = std::env::temp_dir().join(format!("gt2-gauges-{}", std::process::id()));
    std::fs::create_dir_all(&output).unwrap();
    println!("Gauge GPU evidence: {}", output.display());
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut rows = Vec::new();
    for set in ["initial", "fresh"] {
        let data: Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(format!("game/tests/data/original/gauges/{set}.json"))).unwrap(),
        )
        .unwrap();
        rows.extend(data["rows"].as_array().unwrap().iter().cloned());
    }
    rows.sort_by_key(|row| row["frame"].as_u64().unwrap());
    let mut pixels = 0;
    let mut maximum_error = 0;
    for scenario in ["idle", "gas"] {
        let mut command = Command::new(env!(concat!("CARGO_BIN_EXE_", env!("CARGO_PKG_NAME"))));
        command.env("BEVY_ASSET_ROOT", env!("CARGO_MANIFEST_DIR"));
        let mut client = Client::launch(command, &output.join(format!("{scenario}.log")));
        let mut ticks = 0;
        // Source pad transport delivers CROSS to the second physics tick.
        // The native controller consumes effective logical inputs directly,
        // as in the full countdown-gas trajectory recording.
        if scenario == "gas" {
            client.call("game_step", json!({"frames":1,"buttons":[]}));
            ticks = 1;
        }
        for row in rows.iter().filter(|r| r["name"].as_str().unwrap().starts_with(scenario)) {
            let frame = row["frame"].as_u64().unwrap() as u32;
            let buttons = if scenario == "gas" { json!(["cross"]) } else { json!([]) };
            client.call("game_step", json!({"frames":frame/2-ticks,"buttons":buttons}));
            ticks = frame / 2;
            let actual=client.call("game_query",json!({"resources":["GaugeDisplay"]}))["structuredContent"]["resources"]["GaugeDisplay"].clone();
            assert_eq!(actual, row["view"], "{} native gauge state", row["name"]);
            let native = client.screenshot(output.join(format!("{}.png", row["name"].as_str().unwrap())));
            let original = image::open(root.join(row["screen"].as_str().unwrap())).unwrap().to_rgba8();
            let mask: Vec<[u32; 2]> = serde_json::from_value(row["opaque_pixels"].clone()).unwrap();
            assert!(mask.len() > 50);
            for [x, y] in mask {
                let a = native.get_pixel(x, y);
                let b = original.get_pixel(x, y);
                for c in 0..3 {
                    let error = a[c].abs_diff(b[c]);
                    maximum_error = maximum_error.max(error);
                    assert!(error <= 9, "{} at {x},{y}: native {a:?} original {b:?}", row["name"]);
                }
                pixels += 1;
            }
        }
    }
    println!("{pixels} source-masked gauge pixels; maximum encoded-channel error {maximum_error}");
}

#[test]
#[ignore = "requires original records-panel frames, exported assets and an offscreen GPU"]
fn native_records_panel_pixels_match_original_saved_time_cases() {
    let output = std::env::temp_dir().join(format!("gt2-records-{}", std::process::id()));
    std::fs::create_dir_all(&output).unwrap();
    println!("Records GPU evidence: {}", output.display());
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let native_root = output.join("native");
    copy_assets(&root.join("game/assets"), &native_root.join("assets"));
    let definition: game::race::licence::LicenceDefinition = ron::from_str(
        &std::fs::read_to_string(native_root.join("assets").join(game::race::licence::LICENCE_DEFINITION)).unwrap(),
    )
    .unwrap();
    let mut total_pixels = 0;
    let mut maximum_error = 0;
    let mut frames = 0;
    for set in ["initial", "fresh"] {
        let data: Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(format!("game/tests/data/original/records/{set}.json"))).unwrap(),
        )
        .unwrap();
        for row in data["rows"].as_array().unwrap() {
            let name = format!("{set}-{}", row["name"].as_str().unwrap());
            let mut records = game::race::licence::LicenceRecords::default();
            if !row["record"].is_null() {
                records.best.insert(definition.id.clone(), serde_json::from_value(row["record"].clone()).unwrap());
            }
            std::fs::write(
                native_root.join("assets").join(game::race::licence::INITIAL_RECORDS),
                ron::ser::to_string(&records).unwrap(),
            )
            .unwrap();
            let mut command = Command::new(env!(concat!("CARGO_BIN_EXE_", env!("CARGO_PKG_NAME"))));
            command.env("BEVY_ASSET_ROOT", &native_root);
            let mut client = Client::launch(command, &output.join(format!("{name}.log")));
            client.call("game_step", json!({"frames":row["frame"].as_u64().unwrap()/2,"buttons":[]}));
            let native = client.screenshot(output.join(format!("{name}.png")));
            let original = image::open(root.join(row["screen"].as_str().unwrap())).unwrap().to_rgba8();
            let mask: Vec<[u32; 2]> = serde_json::from_value(row["pixels"].clone()).unwrap();
            assert!(mask.len() > 800);
            for [x, y] in mask {
                let a = native.get_pixel(x, y);
                let b = original.get_pixel(x, y);
                for c in 0..3 {
                    let error = a[c].abs_diff(b[c]);
                    maximum_error = maximum_error.max(error);
                    assert!(error <= 9, "{name} pixel {x},{y}: native {a:?} original {b:?}");
                }
                total_pixels += 1;
            }
            frames += 1;
        }
    }
    println!(
        "{frames} records frames; {total_pixels} source-masked pixels; maximum encoded-channel error {maximum_error}"
    );
}

fn native_start_replay(scenario: &str, checkpoint: &str, countdown_fields: Option<u16>) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let output = std::env::temp_dir().join(format!("gt2-start-{scenario}-{}", std::process::id()));
    let asset_root = output.join("native");
    std::fs::create_dir_all(&asset_root).unwrap();
    println!("Start replay evidence: {}", output.display());
    // Offline conversion only. The native process receives ordinary assets,
    // never an emulator checkpoint. Keep the normal moving-reference assets
    // untouched while giving this test the recorded idle initial condition.
    let script = r#"
import importlib.util, pathlib, shutil, sys
root, checkpoint, assets = map(pathlib.Path, sys.argv[1:])
shutil.copytree(root / 'game/assets', assets, dirs_exist_ok=True)
spec=importlib.util.spec_from_file_location('export_assets', root / 'tools/export_assets.py')
module=importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.export_initial_vehicle(checkpoint.read_bytes(), assets)
"#;
    assert!(
        Command::new("python3")
            .arg("-c")
            .arg(script)
            .arg(root)
            .arg(root.join(format!(".psx/runs/{checkpoint}/ram.bin")))
            .arg(asset_root.join("assets"))
            .status()
            .unwrap()
            .success()
    );
    let setup = game::race::RaceVehicle {
        session: countdown_fields.map(game::race::session::DrivingSession::countdown),
        ..Default::default()
    };
    std::fs::write(
        asset_root.join("assets").join(game::race::RACE_VEHICLE_ASSET),
        ron::ser::to_string(&setup).unwrap(),
    )
    .unwrap();
    let fixtures = root.join(format!("game/tests/data/original/vehicle_loop/{scenario}"));
    let mut command = Command::new(env!(concat!("CARGO_BIN_EXE_", env!("CARGO_PKG_NAME"))));
    command.env("BEVY_ASSET_ROOT", &asset_root);
    let mut client = Client::launch(command, &output.join("game.log"));
    replay_start(&mut client, &fixtures, &output);
}

fn replay_start(client: &mut Client, fixtures: &std::path::Path, output: &std::path::Path) {
    let recording: Value =
        serde_json::from_str(&std::fs::read_to_string(fixtures.join("recording.json")).unwrap()).unwrap();
    let mut groups: Vec<(Vec<&str>, u32)> = Vec::new();
    let ticks = recording["ticks"].as_array().unwrap();
    for tick in ticks {
        let c = &tick["controls"];
        let mut buttons = Vec::new();
        for (key, button) in
            [("throttle", "cross"), ("brake", "square"), ("handbrake", "circle"), ("reverse", "triangle")]
        {
            if c[key].as_i64().unwrap() != 0 {
                buttons.push(button);
            }
        }
        let steering = c["steering"].as_i64().unwrap();
        if steering != 0 {
            buttons.push(if steering > 0 { "left" } else { "right" });
        }
        let shift = c["shift"].as_i64().unwrap();
        if shift != 0 {
            buttons.push(if shift > 0 { "r2" } else { "l2" });
        }
        if let Some((previous, count)) = groups.last_mut()
            && *previous == buttons
        {
            *count += 1;
        } else {
            groups.push((buttons, 1));
        }
    }
    for (buttons, count) in groups {
        client.call("game_step", json!({"frames":count,"buttons":buttons}));
    }
    let car=client.call("game_query",json!({"name":"CameraSubject","components":["VehiclePose","VehicleVelocity","DrivetrainInput","EngineTorqueInput"]}))["structuredContent"]["entities"][0].clone();
    let expected = std::fs::read(fixtures.join(ticks.last().unwrap()["end"].as_str().unwrap())).unwrap();
    let word = |offset: usize| i32::from_le_bytes(expected[offset..offset + 4].try_into().unwrap()) as f64 / 4096.0;
    for (i, value) in [word(0x65C), word(0x664), -word(0x660)].into_iter().enumerate() {
        assert_eq!(
            car["components"]["VehiclePose"]["position_m"][i].as_f64().unwrap(),
            value,
            "world position axis {i}"
        );
    }
    assert_eq!(car["components"]["DrivetrainInput"]["gear"], expected[0x618]);
    std::fs::write(output.join("state.json"), serde_json::to_vec_pretty(&car).unwrap()).unwrap();
    client.call("game_step", json!({"frames":1,"buttons":["r1"]}));
    client.screenshot(output.join("after-launch.png"));
}

#[test]
#[ignore = "requires exported assets, original input recording and an offscreen GPU"]
fn native_original_input_clip_drives_the_complete_licence_attempt() {
    let output = std::env::temp_dir().join(format!("gt2-replay-inputs-{}", std::process::id()));
    std::fs::create_dir_all(&output).unwrap();
    println!("Replay input evidence: {}", output.display());
    let game_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let asset_root = output.join("source");
    copy_assets(&game_root.join("assets"), &asset_root.join("assets"));
    let mut setup: game::race::RaceVehicle =
        ron::from_str(&std::fs::read_to_string(game_root.join("assets").join(game::race::RACE_VEHICLE_ASSET)).unwrap())
            .unwrap();
    setup.replay_inputs = Some("race/replay-inputs.ron".into());
    std::fs::write(asset_root.join("assets").join(game::race::RACE_VEHICLE_ASSET), ron::to_string(&setup).unwrap())
        .unwrap();
    std::fs::copy(
        game_root.join("tests/data/original/replay_inputs/natural-recording.ron"),
        asset_root.join("assets/race/replay-inputs.ron"),
    )
    .unwrap();
    let mut command = Command::new(env!(concat!("CARGO_BIN_EXE_", env!("CARGO_PKG_NAME"))));
    command.env("BEVY_ASSET_ROOT", &asset_root);
    let mut client = Client::launch(command, &output.join("game.log"));
    client.call("game_step", json!({"frames":1250,"buttons":["square","left","triangle","l2"]}));
    let actual = client
        .call("game_query", json!({"name":"CameraSubject","components":["VehiclePose","LicenceRun","ReplayPlayback"]}))
        ["structuredContent"]
        .clone();
    let source: Value = serde_json::from_str(
        &std::fs::read_to_string(game_root.join("tests/data/original/replay_inputs/initial.json")).unwrap(),
    )
    .unwrap();
    let row =
        &source.as_array().unwrap().iter().find(|case| case["name"] == "natural-recording").unwrap()["source_outcome"];
    let components = &actual["entities"][0]["components"];
    assert_eq!(components["LicenceRun"]["Passed"]["time_ms"], row["time_ms"]);
    for axis in 0..3 {
        assert!(
            (components["VehiclePose"]["position_m"][axis].as_f64().unwrap()
                - row["position_m"][axis].as_f64().unwrap())
            .abs()
                < 0.0001,
            "{actual}"
        );
    }
    assert_eq!(components["ReplayPlayback"]["elapsed_ticks"], 1247);
    assert_eq!(components["ReplayPlayback"]["finished"], true);
    client.screenshot(output.join("finished.png"));
    std::fs::write(output.join("result.json"), serde_json::to_vec_pretty(&actual).unwrap()).unwrap();
}
