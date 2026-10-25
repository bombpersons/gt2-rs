//! Licence result timing at 30 Hz (8002A700), independent of drawing and saves.
//! A completed screen requests replay; it does not commit the licence award.
use super::licence_stop::{LicenceFailure, LicenceParticipant, LicenceRun, LicenceStopSystems};
use bevy::prelude::*;
use engine::{controller::Button, prelude::*};
use serde::{Deserialize, Serialize};

pub const RESULT_TIMING: &str = "race/licence-results.ron";

#[derive(Resource, Clone, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct LicenceResultTiming {
    /// 8002A7A0: successful prize waits for a new confirm press here.
    pub success_wait_ticks: u16,
    /// 8002A78C: failed announcement has its own shorter wait.
    pub failure_wait_ticks: u16,
    /// 8002A790: held confirm advances this many successful-screen steps.
    pub held_updates_per_tick: u16,
    /// 8002B264..268: the prize is shown strictly after this age.
    pub prize_after_ticks: u16,
}

/// Only the result screen consumes confirm; driving controls remain separate.
#[derive(Clone, Copy, Default, Debug)]
pub struct ResultInput {
    pub confirm_held: bool,
    pub confirm_pressed: bool,
}

#[derive(Resource, Clone, Default, Debug, PartialEq, Eq, Reflect)]
#[reflect(Resource)]
pub enum LicenceResult {
    #[default]
    Driving,
    Success {
        time_ms: u32,
        age_ticks: u16,
    },
    Failure {
        reason: LicenceFailure,
        age_ticks: u16,
    },
    /// Retains the attempt while a replay/front end consumes the handoff.
    ReplayRequested {
        run: LicenceRun,
    },
}

impl LicenceResult {
    pub fn begin(run: &LicenceRun) -> Self {
        match *run {
            LicenceRun::Active => Self::Driving,
            LicenceRun::Passed { time_ms } => Self::Success { time_ms, age_ticks: 0 },
            LicenceRun::Failed { reason } => Self::Failure { reason, age_ticks: 0 },
        }
    }

    pub fn age_ticks(&self) -> Option<u16> {
        match self {
            Self::Success { age_ticks, .. } | Self::Failure { age_ticks, .. } => Some(*age_ticks),
            _ => None,
        }
    }

    /// Returns true exactly once when the screen requests its replay.
    pub fn advance(&mut self, timing: &LicenceResultTiming, input: ResultInput) -> bool {
        let (age, wait, updates, run) = match self {
            Self::Success { time_ms, age_ticks } => (
                age_ticks,
                timing.success_wait_ticks,
                if input.confirm_held { timing.held_updates_per_tick.max(1) } else { 1 },
                LicenceRun::Passed { time_ms: *time_ms },
            ),
            Self::Failure { reason, age_ticks } => {
                (age_ticks, timing.failure_wait_ticks, 1, LicenceRun::Failed { reason: *reason })
            }
            _ => return false,
        };
        for _ in 0..updates {
            // Reaching the wait during accelerated updates stops this tick.
            // Confirm advances once and still leaves one tick before replay
            // (8002A7AC..7D8 and 8002A838..860).
            if *age == wait {
                if input.confirm_pressed {
                    *age = age.saturating_add(1);
                }
                break;
            }
            *age = age.saturating_add(1);
        }
        // 8002A814/86C: the final confirmed age is drawn for one tick.
        if *age > wait.saturating_add(1) {
            *self = Self::ReplayRequested { run };
            true
        } else {
            false
        }
    }
}

#[derive(Event, Clone, Debug)]
pub struct LicenceReplayRequested {
    pub run: LicenceRun,
}

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LicenceResultSystems;

/// Optional timing asset; standalone callers may insert their own tuning and
/// drive `LicenceResult::advance` without a car or any rendering plugins.
pub struct LicenceResultRulesPlugin;
impl Plugin for LicenceResultRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<LicenceResultTiming>()
            && engine::config::asset_path(RESULT_TIMING).is_file()
        {
            app.add_plugins(TuningPlugin::<LicenceResultTiming>::new(RESULT_TIMING));
        }
        app.init_resource::<LicenceResult>()
            .register_type::<LicenceResult>()
            .register_type::<LicenceResultTiming>()
            .configure_sets(
                FixedUpdate,
                LicenceResultSystems
                    .after(LicenceStopSystems::Decide)
                    .in_set(EngineSet::Objects)
                    .in_set(GameSystems::Game),
            )
            .add_systems(FixedUpdate, update.in_set(LicenceResultSystems));
    }
}

fn update(
    timing: Option<Res<LicenceResultTiming>>,
    input: Option<Res<ControllerState>>,
    mut result: ResMut<LicenceResult>,
    runs: Query<&LicenceRun, With<LicenceParticipant>>,
    mut commands: Commands,
) {
    let Some(timing) = timing else { return };
    if *result == LicenceResult::Driving {
        let Ok(run) = runs.single() else { return };
        if *run == LicenceRun::Active {
            return;
        }
        *result = LicenceResult::begin(run);
    }
    if matches!(*result, LicenceResult::ReplayRequested { .. }) {
        return;
    }
    let confirm = |pressed: bool| {
        input.as_ref().is_some_and(|pad| {
            [Button::Cross, Button::Circle]
                .into_iter()
                .any(|button| if pressed { pad.pressed(button) } else { pad.held(button) })
        })
    };
    if result.advance(&timing, ResultInput { confirm_held: confirm(false), confirm_pressed: confirm(true) })
        && let LicenceResult::ReplayRequested { run } = &*result
    {
        commands.trigger(LicenceReplayRequested { run: run.clone() });
    }
}
