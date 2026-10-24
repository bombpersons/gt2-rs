use bevy::prelude::*;
use engine::testing::{TickApp, headless_app};
use engine::{course::CourseData, prelude::load_ron};
use game::race::{
    licence_stop::*,
    session::{DrivingPhase, DrivingSession},
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Row {
    frame: u32,
    seed: u16,
    points_m: [[f64; 3]; 4],
    distances_m: Vec<f64>,
    velocity_mps: [f32; 3],
    surface: [u8; 4],
    faults: u8,
    clock: u32,
    time_origin_ms: u32,
    before: Outcome,
    after: Outcome,
}
#[derive(Deserialize)]
struct Outcome {
    state: u8,
    reason: u8,
    time_ms: u32,
}
impl Outcome {
    fn native(&self) -> LicenceRun {
        match self.state {
            0 => LicenceRun::Active,
            1 => LicenceRun::Passed { time_ms: self.time_ms },
            2 => LicenceRun::Failed {
                reason: match self.reason {
                    3 => LicenceFailure::Overshot,
                    4 => LicenceFailure::OffRoad,
                    5 => LicenceFailure::Collision,
                    x => panic!("unknown original failure {x}"),
                },
            },
            x => panic!("unknown original run state {x}"),
        }
    }
}
#[derive(Deserialize)]
struct Recording {
    rows: Vec<Row>,
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original licence stopping queries")]
fn original_wheel_course_distances() {
    let course: CourseData = load_ron("gt2/course/TC_lisence.ron").unwrap();
    for name in ["early", "gold", "overshoot", "offroad", "holdout"] {
        let path = format!("{}/tests/data/original/licence_stop/{name}.json", env!("CARGO_MANIFEST_DIR"));
        let data: Recording = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for row in data.rows {
            for (point, expected) in row.points_m.into_iter().zip(row.distances_m) {
                let actual = course.distance_at_position(row.seed, point).unwrap();
                assert_eq!(
                    (actual * 65536.0).round() as i64,
                    (expected * 65536.0).round() as i64,
                    "{name} field {} at {point:?}: {actual} vs {expected}",
                    row.frame
                );
            }
        }
    }
}

#[test]
#[cfg_attr(not(gt2_private_data), ignore = "requires original licence stopping decisions")]
fn original_stopping_decisions_and_latched_time() {
    let course: CourseData = load_ron("gt2/course/TC_lisence.ron").unwrap();
    let settings: LicenceStopSettings = load_ron(STOP_SETTINGS).unwrap();
    for name in ["early", "gold", "overshoot", "offroad", "holdout"] {
        let path = format!("{}/tests/data/original/licence_stop/{name}.json", env!("CARGO_MANIFEST_DIR"));
        let data: Recording = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut app = headless_app(game::tick_rate(game::REGION));
        app.insert_resource(settings.clone()).add_plugins(LicenceStopPlugin);
        let car = app.world_mut().spawn((data.rows[0].before.native(), LicenceStopInput::default())).id();
        for row in data.rows {
            assert_eq!(
                *app.world().get::<LicenceRun>(car).unwrap(),
                row.before.native(),
                "{name} before {}",
                row.frame
            );
            let wheel_distance_m = row.points_m.map(|point| {
                let d = course.distance_at_position(row.seed, point).unwrap();
                let wrap = ((course.lap_length_m * 65536.0 / 8.0).trunc() * 7.0) / 65536.0;
                if d > wrap { d - course.lap_length_m } else { d }
            });
            app.world_mut().entity_mut(car).insert(LicenceStopInput {
                wheel_distance_m,
                velocity_mps: Vec3::from_array(row.velocity_mps),
                surface: row.surface,
                offroad_fault: row.faults & 2 != 0,
                collision_fault: row.faults & 1 != 0,
                time_origin_ms: row.time_origin_ms,
            });
            assert_eq!(row.clock % 100, 0);
            app.insert_resource(DrivingSession { phase: DrivingPhase::Driving, elapsed_ticks: row.clock / 100 });
            app.run_tick(&[]);
            assert_eq!(*app.world().get::<LicenceRun>(car).unwrap(), row.after.native(), "{name} after {}", row.frame);
        }
    }
}

#[test]
fn authored_stopping_rules_run_without_vehicle_or_renderer_and_latch_results() {
    let mut app = headless_app(game::tick_rate(game::REGION));
    app.insert_resource(LicenceStopSettings { start_m: 10.0, end_m: 20.0, stop_speed_mps: 1.0 })
        .add_plugins(LicenceStopPlugin);
    let car = app
        .world_mut()
        .spawn((
            LicenceRun::Active,
            LicenceStopInput { wheel_distance_m: [10.0, 20.0, 15.0, 15.0], velocity_mps: Vec3::X * 0.125, ..default() },
        ))
        .id();
    app.run_tick(&[]);
    assert_eq!(*app.world().get::<LicenceRun>(car).unwrap(), LicenceRun::Active);
    app.insert_resource(DrivingSession { phase: DrivingPhase::Driving, elapsed_ticks: 10 });
    app.run_tick(&[]);
    assert_eq!(*app.world().get::<LicenceRun>(car).unwrap(), LicenceRun::Passed { time_ms: 337 });
    app.world_mut().get_mut::<LicenceStopInput>(car).unwrap().wheel_distance_m[0] = 20.0 + 1.0 / 65536.0;
    app.run_tick(&[]);
    assert_eq!(*app.world().get::<LicenceRun>(car).unwrap(), LicenceRun::Passed { time_ms: 337 });
    *app.world_mut().get_mut::<LicenceRun>(car).unwrap() = LicenceRun::Active;
    app.run_tick(&[]);
    assert_eq!(*app.world().get::<LicenceRun>(car).unwrap(), LicenceRun::Failed { reason: LicenceFailure::Overshot });
}
