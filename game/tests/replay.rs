use bevy::prelude::*;
use engine::testing::{TickApp, headless_app};
use game::race::replay::*;
use game::vehicle::{DigitalGearInput, DigitalPedalInput, DigitalSteeringInput, PlayerPedals, VehicleRulesPlugin};
use serde::Deserialize;

#[derive(Deserialize)]
struct Row {
    controls: ReplayControls,
    finished: bool,
    remaining: u32,
    repeat: i32,
}
#[derive(Deserialize)]
struct Case {
    name: String,
    clip: ReplayClip,
    rows: Vec<Row>,
}
fn original(name: &str) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/original/replay_inputs");
    let cases: Vec<Case> =
        serde_json::from_str(&std::fs::read_to_string(root.join(format!("{name}.json"))).unwrap()).unwrap();
    assert_eq!(cases.len(), if name == "initial" { 10 } else { 2 });
    for case in cases {
        let imported: ReplayClip =
            ron::from_str(&std::fs::read_to_string(root.join(format!("{}.ron", case.name))).unwrap()).unwrap();
        assert_eq!(imported, case.clip);
        let mut native = ReplayPlayback::new(imported).unwrap();
        for (i, row) in case.rows.iter().enumerate() {
            let input = match native.advance() {
                ReplayTick::Input(c) => c,
                ReplayTick::Finished | ReplayTick::Inactive => ReplayControls::default(),
            };
            assert_eq!(input, row.controls, "{} call {i}", case.name);
            assert_eq!(native.held_ticks_remaining(), row.repeat as u32, "{} hold at call {i}", case.name);
            assert_eq!(native.is_finished(), row.finished, "{} call {i}", case.name);
            assert_eq!(native.total_ticks - native.elapsed_ticks, row.remaining, "{} call {i}", case.name);
        }
    }
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original replay decoder recordings")]
fn original_replay_input_runs_hold_boundaries_and_terminal_tick() {
    original("initial");
}
#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires fresh original replay decoder recordings")]
fn fresh_original_replay_input_holdout() {
    original("fresh");
}

#[test]
fn authored_replay_overrides_live_pad_then_emits_one_neutral_handoff() {
    #[derive(Resource, Default)]
    struct Finished(usize);
    let mut clip = ReplayClip::default();
    let idle = ReplayControls::from_driver(
        DigitalPedalInput::default(),
        DigitalSteeringInput::default(),
        DigitalGearInput::default(),
    );
    assert_eq!(idle, ReplayControls::default());
    for _ in 0..3 {
        clip.record(ReplayControls { throttle: true, steering: -1, shift: 1, ..default() }).unwrap();
    }
    clip.record(ReplayControls { brake: true, reverse: true, ..default() }).unwrap();
    clip.record(idle).unwrap();
    assert_eq!(clip.runs.len(), 3);
    let clip: ReplayClip = ron::from_str(&ron::to_string(&clip).unwrap()).unwrap();
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.add_plugins((VehicleRulesPlugin, ReplayInputRulesPlugin))
        .init_resource::<Finished>()
        .add_observer(|_: On<ReplayInputsFinished>, mut f: ResMut<Finished>| f.0 += 1);
    let car = app
        .world_mut()
        .spawn((
            PlayerPedals,
            ReplayPlayback::new(clip).unwrap(),
            DigitalPedalInput::default(),
            DigitalSteeringInput::default(),
            DigitalGearInput::default(),
        ))
        .id();
    for _ in 0..3 {
        app.run_tick(&[engine::controller::Button::Square, engine::controller::Button::Left]);
        assert!(app.world().get::<DigitalPedalInput>(car).unwrap().throttle);
        assert_eq!(app.world().get::<DigitalSteeringInput>(car).unwrap().direction, -1.0);
        assert_eq!(app.world().get::<DigitalGearInput>(car).unwrap().shift_direction, 1);
    }
    app.run_tick(&[]);
    assert!(app.world().get::<DigitalGearInput>(car).unwrap().reverse);
    app.run_tick(&[]);
    assert_eq!(app.world().resource::<Finished>().0, 1);
    app.run_tick(&[engine::controller::Button::Cross]);
    assert!(!app.world().get::<DigitalPedalInput>(car).unwrap().throttle);
    assert_eq!(app.world().resource::<Finished>().0, 1);
}
