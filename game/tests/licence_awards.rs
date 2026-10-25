use bevy::prelude::*;
use engine::testing::{TickApp, headless_app};
use game::race::{licence::LicenceDefinition, licence_awards::*};
use serde::Deserialize;

#[derive(Deserialize)]
struct State {
    grade: u8,
    assisted_attempts: u8,
    group_grades: Vec<u8>,
}
#[derive(Deserialize)]
struct Check {
    all_gold: bool,
    value: u8,
}
#[derive(Deserialize)]
struct Row {
    definition: LicenceDefinition,
    time_ms: Option<u32>,
    before: State,
    after: State,
    checks: Vec<Check>,
    test_index: usize,
}
#[derive(Deserialize)]
struct Recording {
    rows: Vec<Row>,
}
fn grade(raw: u8) -> LicenceGrade {
    match raw {
        0 => LicenceGrade::Unpassed,
        1 => LicenceGrade::AssistedPass,
        2 => LicenceGrade::Bronze,
        3 => LicenceGrade::Silver,
        4 => LicenceGrade::Gold,
        _ => panic!("unsupported original grade {raw}"),
    }
}

fn check(name: &str, count: usize) {
    let path = format!("{}/tests/data/original/licence_awards/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let recording: Recording = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(recording.rows.len(), count);
    for (index, row) in recording.rows.into_iter().enumerate() {
        let keys: Vec<_> = (0..10)
            .map(|i| if i == row.test_index { row.definition.id.clone() } else { format!("other-{i}") })
            .collect();
        let mut progress = LicenceProgress::default();
        for (id, &raw) in keys.iter().zip(&row.before.group_grades) {
            progress.achievements.insert(id.clone(), LicenceAchievement { grade: grade(raw), ..default() });
        }
        progress.achievements.get_mut(&row.definition.id).unwrap().assisted_attempts = row.before.assisted_attempts;
        assert_eq!(progress.achievement(&row.definition.id).grade, grade(row.before.grade));
        let verify_groups = |progress: &LicenceProgress, checks: &[Check]| {
            for check in checks {
                let actual = if check.all_gold {
                    progress.group_gold(keys.iter().map(String::as_str))
                } else {
                    progress.group_passed(keys.iter().map(String::as_str))
                };
                assert_eq!(actual, check.value != 0, "{name} row {index} group");
            }
        };
        let mut app = headless_app(game::tick_rate(game::REGION));
        app.insert_resource(progress).add_plugins(LicenceAwardsPlugin);
        verify_groups(app.world().resource::<LicenceProgress>(), &row.checks[..row.checks.len() / 2]);
        app.world_mut().trigger(CommitLicenceAward { definition: row.definition.clone(), time_ms: row.time_ms });
        app.run_tick(&[]);
        let progress = app.world().resource::<LicenceProgress>();
        assert_eq!(
            progress.achievement(&row.definition.id),
            LicenceAchievement { grade: grade(row.after.grade), assisted_attempts: row.after.assisted_attempts },
            "{name} row {index}"
        );
        verify_groups(progress, &row.checks[row.checks.len() / 2..]);
        for (id, raw) in keys.iter().zip(row.after.group_grades) {
            assert_eq!(progress.achievement(id).grade, grade(raw), "{name} row {index} neighbor {id}");
        }
    }
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original menu award probes")]
fn original_strict_medal_attempt_and_group_boundaries() {
    check("initial", 50);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires fresh original all-licence award probes")]
fn fresh_original_all_licence_award_holdout() {
    check("fresh", 60);
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original completed licence progress export")]
fn native_progress_format_preserves_original_saved_achievements() {
    #[derive(Deserialize)]
    struct Saved {
        id: String,
        grade: u8,
        assisted_attempts: u8,
    }
    #[derive(Deserialize)]
    struct Export {
        rows: Vec<Saved>,
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/original/licence_awards");
    let progress: LicenceProgress =
        ron::from_str(&std::fs::read_to_string(root.join("licence-progress.initial.ron")).unwrap()).unwrap();
    let data: Export = serde_json::from_str(&std::fs::read_to_string(root.join("exported.json")).unwrap()).unwrap();
    assert_eq!(data.rows.len(), 60);
    assert!(data.rows.iter().any(|row| row.grade > 0), "export must exercise a completed achievement");
    for row in data.rows {
        assert_eq!(
            progress.achievement(&row.id),
            LicenceAchievement { grade: grade(row.grade), assisted_attempts: row.assisted_attempts },
            "{}",
            row.id
        );
    }
    let definition: LicenceDefinition =
        ron::from_str(&std::fs::read_to_string(root.join("licence.ron")).unwrap()).unwrap();
    assert!(definition.assisted_pass_attempts.is_some());
    assert!(progress.achievements.contains_key(&definition.id));
}

#[test]
fn authored_achievements_wait_for_commit_and_survive_failures_and_slower_results() {
    #[derive(Resource, Default)]
    struct Applied(Vec<bool>);
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(LicenceProgress::default())
        .init_resource::<Applied>()
        .add_plugins(LicenceAwardsPlugin)
        .add_observer(|event: On<LicenceAwardApplied>, mut applied: ResMut<Applied>| {
            applied.0.push(event.improved);
        });
    let definition = LicenceDefinition {
        id: "hill".into(),
        time_limits_ms: [1000, 2000, 3000, 4000],
        assisted_pass_attempts: Some(2),
    };
    app.run_tick(&[]);
    assert!(app.world().resource::<LicenceProgress>().achievements.is_empty());
    for expected in [LicenceGrade::Unpassed, LicenceGrade::AssistedPass] {
        app.world_mut().trigger(CommitLicenceAward { definition: definition.clone(), time_ms: Some(3500) });
        assert_eq!(app.world().resource::<LicenceProgress>().achievement("hill").grade, expected);
    }
    app.world_mut().trigger(CommitLicenceAward { definition: definition.clone(), time_ms: Some(999) });
    app.world_mut().trigger(CommitLicenceAward { definition: definition.clone(), time_ms: None });
    app.world_mut().trigger(CommitLicenceAward { definition, time_ms: Some(3500) });
    app.world_mut().flush();
    let progress = app.world().resource::<LicenceProgress>();
    assert_eq!(progress.achievement("hill"), LicenceAchievement { grade: LicenceGrade::Gold, assisted_attempts: 2 });
    assert!(progress.group_passed(["hill"]));
    assert!(progress.group_gold(["hill"]));
    assert!(!progress.group_passed(["hill", "circuit"]));
    assert_eq!(progress.achievements.len(), 1);
    assert_eq!(app.world().resource::<Applied>().0, [false, true, true, false, false]);
}
