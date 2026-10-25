use bevy::prelude::*;
use engine::testing::{TickApp, headless_app};
use game::race::{licence_stop::*, replay::*, replay_recording::*};
use serde::Deserialize;
#[derive(Deserialize)]
struct Row {
    action: String,
    requests: RecordingRequests,
    completed: bool,
    recorded_ticks: u32,
    storage_bytes: u32,
    closed: bool,
    pending_ticks: u32,
}
#[derive(Deserialize)]
struct Case {
    name: String,
    capacity: u32,
    seed_ticks: u32,
    natural: bool,
    discard: bool,
    rows: Vec<Row>,
}
// Compare only emitted input: the source never serializes the counted final
// input when a full flush closes storage. Both decoders discard that terminal
// sample, so its in-memory request is deliberately not an equality target.
fn emitted_runs(mut clip: ReplayClip) -> Vec<ReplayRun> {
    if let Some(last) = clip.runs.last_mut() {
        last.ticks -= 1;
        if last.ticks == 0 {
            clip.runs.pop();
        }
    }
    let mut runs: Vec<ReplayRun> = Vec::new();
    for run in clip.runs {
        if let Some(last) = runs.last_mut().filter(|r| r.controls == run.controls) {
            last.ticks += run.ticks;
        } else {
            runs.push(run);
        }
    }
    runs
}
fn check(name: &str) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/original/replay_recording");
    let base: ReplayRecordingSettings =
        ron::from_str(&std::fs::read_to_string(root.join("replay-recording.ron")).unwrap()).unwrap();
    let cases: Vec<Case> =
        serde_json::from_str(&std::fs::read_to_string(root.join(format!("{name}.json"))).unwrap()).unwrap();
    assert_eq!(
        cases.len(),
        match name {
            "initial" => 19,
            "full-capacity" => 1,
            _ => 6,
        }
    );
    for case in cases {
        let settings = ReplayRecordingSettings { capacity_bytes: case.capacity, ..base.clone() };
        let mut native = InputRecording::new(&settings);
        for _ in 0..case.seed_ticks {
            native.record(RecordingRequests::default());
        }
        let mut bridge = ReplayRecorder::new(&settings);
        for (i, row) in case.rows.iter().enumerate() {
            match row.action.as_str() {
                "record" => {
                    native.record(row.requests);
                }
                "finish" => {
                    native.finish(false);
                }
                "discard" => {
                    native.finish(true);
                }
                _ => panic!("unknown source action"),
            }
            assert_eq!(native.recorded_ticks, row.recorded_ticks, "{} call {i}", case.name);
            assert_eq!(native.storage_bytes, row.storage_bytes, "{} call {i}", case.name);
            assert_eq!(native.is_closed(), row.closed, "{} call {i}", case.name);
            if row.action == "record" {
                assert_eq!(native.pending_ticks(), row.pending_ticks, "{} call {i}", case.name);
            }
            if case.natural {
                bridge.advance(&settings, row.requests, row.completed);
                assert_eq!(bridge.recording.recorded_ticks, row.recorded_ticks, "{} bridge call {i}", case.name);
                assert_eq!(bridge.recording.storage_bytes, row.storage_bytes, "{} bridge call {i}", case.name);
                assert_eq!(bridge.recording.is_closed(), row.closed, "{} bridge call {i}", case.name);
            }
        }
        assert_eq!(native.is_closed(), case.rows.last().unwrap().closed);
        if case.discard || !native.is_closed() || native.recorded_ticks == 0 {
            assert!(native.playback_clip().is_none());
        } else {
            let clip = native.playback_clip().unwrap();
            assert_eq!(clip.ticks().unwrap(), native.recorded_ticks);
            let imported: ReplayClip =
                ron::from_str(&std::fs::read_to_string(root.join(format!("{}.ron", case.name))).unwrap()).unwrap();
            assert_eq!(emitted_runs(clip), emitted_runs(imported), "{} emitted input", case.name);
        }
    }
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original recording observations")]
fn original_recording_capacity_holds_finalization_and_result_clock() {
    check("initial");
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires fresh original recording observations")]
fn fresh_original_recording_holdout() {
    check("fresh");
}

#[test]
fn authored_recording_keeps_cancelling_requests_for_storage_and_completes_once() {
    #[derive(Resource, Default)]
    struct Finishes(usize);
    let settings = ReplayRecordingSettings { capacity_bytes: 22, closing_reserve_bytes: 17, post_finish_ticks: 5 };
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(settings.clone())
        .add_plugins(ReplayRecordingRulesPlugin)
        .init_resource::<Finishes>()
        .add_observer(|_: On<ReplayRecordingFinished>, mut f: ResMut<Finishes>| f.0 += 1);
    let car = app.world_mut().spawn((ReplayRecorder::new(&settings), LicenceRun::Active)).id();
    app.run_tick(&[]);
    app.run_tick(&[engine::controller::Button::Left, engine::controller::Button::Right]);
    let recorder = &app.world().get::<ReplayRecorder>(car).unwrap().recording;
    assert_eq!(recorder.phase, RecordingPhase::Capacity);
    assert_eq!(recorder.recorded_ticks, 2);
    assert_eq!(recorder.storage_bytes, 5);
    // Both requests cancel in the car, yet their transport change filled
    // storage. The counted unwritten sample becomes the neutral final call.
    let mut playback = ReplayPlayback::new(recorder.playback_clip().unwrap()).unwrap();
    assert_eq!(playback.advance(), ReplayTick::Input(ReplayControls::default()));
    assert_eq!(playback.advance(), ReplayTick::Finished);
    app.run_ticks(8, &[engine::controller::Button::Cross]);
    assert_eq!(app.world().resource::<Finishes>().0, 1);
    assert_eq!(app.world().get::<ReplayRecorder>(car).unwrap().recording.recorded_ticks, 2);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original full-capacity writer run")]
fn original_full_size_buffer_fills_at_the_same_input_transition() {
    check("full-capacity");
}
