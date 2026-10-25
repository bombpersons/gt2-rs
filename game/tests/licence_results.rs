use bevy::prelude::*;
use engine::controller::Button;
use engine::testing::{TickApp, headless_app};
use game::race::{licence_results::*, licence_stop::*};
use serde::Deserialize;

#[derive(Deserialize)]
struct Row {
    before: i16,
    after: i16,
    held: bool,
    pressed: bool,
    running: bool,
}
#[derive(Deserialize)]
struct Recording {
    passed: bool,
    rows: Vec<Row>,
}

fn compare_original(name: &str) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/original/licence_results");
    let tuning: LicenceResultTiming =
        ron::from_str(&std::fs::read_to_string(root.join("licence-results.ron")).unwrap()).unwrap();
    let recordings: Vec<Recording> =
        serde_json::from_str(&std::fs::read_to_string(root.join(format!("{name}.json"))).unwrap()).unwrap();
    let expected = if name == "initial" { (72, 407) } else { (2, 136) };
    assert_eq!((recordings.len(), recordings.iter().map(|r| r.rows.len()).sum::<usize>()), expected);
    for (case, recording) in recordings.iter().enumerate() {
        let mut retained = None;
        for (tick, row) in recording.rows.iter().enumerate() {
            let run = if recording.passed {
                LicenceRun::Passed { time_ms: 38_675 }
            } else {
                LicenceRun::Failed { reason: LicenceFailure::Overshot }
            };
            let initial = if row.before < 0 {
                LicenceResult::Driving
            } else if recording.passed {
                LicenceResult::Success { time_ms: 38_675, age_ticks: row.before as u16 }
            } else {
                LicenceResult::Failure { reason: LicenceFailure::Overshot, age_ticks: row.before as u16 }
            };
            // Seed only the first row. Later ages come from the native state,
            // including the fresh holdout's complete held/released schedule.
            let native = retained.get_or_insert(initial);
            if row.before >= 0 {
                assert_eq!(native.age_ticks(), Some(row.before as u16), "{name} case {case} tick {tick}");
            } else {
                assert!(matches!(native, LicenceResult::Driving | LicenceResult::ReplayRequested { .. }));
            }
            let requested =
                native.advance(&tuning, ResultInput { confirm_held: row.held, confirm_pressed: row.pressed });
            assert_eq!(requested, !row.running, "{name} case {case} tick {tick}");
            let age = match native {
                LicenceResult::ReplayRequested { run: actual } => {
                    assert_eq!(*actual, run);
                    if requested {
                        (if recording.passed { tuning.success_wait_ticks + 1 } else { tuning.failure_wait_ticks + 1 })
                            as i16
                    } else {
                        // The original deactivates its timer after the handoff.
                        -1
                    }
                }
                _ => native.age_ticks().map_or(-1, |age| age as i16),
            };
            assert_eq!(age, row.after, "{name} case {case} tick {tick}");
        }
    }
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original licence result recordings")]
fn original_result_timer_waits_and_consumed_confirm_edges() {
    compare_original("initial");
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires fresh original result recordings")]
fn fresh_original_result_holdout() {
    compare_original("fresh");
}

#[test]
fn authored_result_requests_replay_once_and_never_commits_an_award() {
    #[derive(Resource, Default)]
    struct Handoffs(usize);
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(LicenceResultTiming {
        success_wait_ticks: 5,
        failure_wait_ticks: 2,
        held_updates_per_tick: 3,
        prize_after_ticks: 3,
    })
    .init_resource::<Handoffs>()
    .insert_resource(game::race::licence_awards::LicenceProgress::default())
    .insert_resource(game::race::licence::LicenceDefinition {
        id: "hill".into(),
        time_limits_ms: [500, 600, 700, 800],
        assisted_pass_attempts: Some(2),
    })
    .add_plugins((LicenceResultRulesPlugin, game::race::licence_awards::LicenceAwardsPlugin))
    .add_observer(|_: On<LicenceReplayRequested>, mut handoffs: ResMut<Handoffs>| handoffs.0 += 1);
    let entity = app.world_mut().spawn((LicenceParticipant, LicenceRun::Active)).id();
    app.run_tick(&[]);
    assert_eq!(*app.world().resource::<LicenceResult>(), LicenceResult::Driving);
    app.world_mut().entity_mut(entity).insert(LicenceRun::Passed { time_ms: 400 });
    app.run_tick(&[Button::Cross]);
    assert_eq!(app.world().resource::<LicenceResult>().age_ticks(), Some(3));
    app.run_tick(&[Button::Cross]);
    assert_eq!(app.world().resource::<LicenceResult>().age_ticks(), Some(5));
    app.run_tick(&[Button::Cross]);
    assert_eq!(app.world().resource::<LicenceResult>().age_ticks(), Some(5));
    app.run_tick(&[]);
    app.run_tick(&[Button::Circle]);
    assert_eq!(app.world().resource::<LicenceResult>().age_ticks(), Some(6));
    assert_eq!(app.world().resource::<Handoffs>().0, 0);
    app.run_tick(&[]);
    assert_eq!(
        *app.world().resource::<LicenceResult>(),
        LicenceResult::ReplayRequested { run: LicenceRun::Passed { time_ms: 400 } }
    );
    app.run_ticks(8, &[]);
    assert_eq!(app.world().resource::<Handoffs>().0, 1);
    assert!(app.world().resource::<game::race::licence_awards::LicenceProgress>().achievements.is_empty());
}
