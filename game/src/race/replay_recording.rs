//! Licence replay recording and bounded storage (800166CC / 800167D0).
//! Native input runs are retained; original bytes are only a storage budget,
//! never a runtime packet buffer or an interpreter.
use super::{
    licence_stop::LicenceRun,
    replay::{ReplayClip, ReplayControls, ReplayPlayback, ReplayRun},
};
use bevy::prelude::*;
use engine::{controller::Button, prelude::*};
use serde::{Deserialize, Serialize};

pub const RECORDING_SETTINGS: &str = "race/replay-recording.ron";
#[derive(Resource, Clone, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct ReplayRecordingSettings {
    pub capacity_bytes: u32,
    /// 800166AC reserves room before accepting another encoded transition.
    pub closing_reserve_bytes: u32,
    /// 80013D78..98: a licence records until sixty post-result fields.
    pub post_finish_ticks: u16,
}
/// Keep opposing held requests separately: although they cancel in the car,
/// their transport changes still consume recording storage in the original.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub struct RecordingRequests {
    pub throttle: bool,
    pub brake: bool,
    pub handbrake: bool,
    pub left: bool,
    pub right: bool,
    pub reverse: bool,
    pub shift_up: bool,
    pub shift_down: bool,
}
impl RecordingRequests {
    pub fn from_pad(pad: &ControllerState) -> Self {
        Self {
            throttle: pad.held(Button::Cross),
            brake: pad.held(Button::Square),
            handbrake: pad.held(Button::Circle),
            left: pad.held(Button::Left),
            right: pad.held(Button::Right),
            reverse: pad.held(Button::Triangle),
            shift_up: pad.held(Button::R2),
            shift_down: pad.held(Button::L2),
        }
    }
    pub fn controls(self) -> ReplayControls {
        ReplayControls {
            throttle: self.throttle,
            brake: self.brake,
            handbrake: self.handbrake,
            steering: i8::from(self.left) - i8::from(self.right),
            reverse: self.reverse,
            shift: i8::from(self.shift_up) - i8::from(self.shift_down),
        }
    }
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Reflect)]
pub enum RecordingPhase {
    #[default]
    Recording,
    Complete,
    Capacity,
    Discarded,
}
#[derive(Clone, Debug, Reflect)]
struct PendingRun {
    requests: RecordingRequests,
    ticks: u32,
    base_bytes: u32,
}
#[derive(Clone, Debug, Reflect)]
pub struct InputRecording {
    pub phase: RecordingPhase,
    pub recorded_ticks: u32,
    pub storage_bytes: u32,
    pub clip: ReplayClip,
    capacity_bytes: u32,
    closing_reserve_bytes: u32,
    pending: Option<PendingRun>,
}
impl InputRecording {
    pub fn new(settings: &ReplayRecordingSettings) -> Self {
        Self {
            phase: RecordingPhase::Recording,
            recorded_ticks: 0,
            storage_bytes: 0,
            clip: ReplayClip::default(),
            capacity_bytes: settings.capacity_bytes,
            closing_reserve_bytes: settings.closing_reserve_bytes,
            pending: None,
        }
    }
    pub fn is_closed(&self) -> bool {
        self.phase != RecordingPhase::Recording
    }
    pub fn pending_ticks(&self) -> u32 {
        self.pending.as_ref().map_or(0, |r| r.ticks)
    }
    /// Record one driver call. Returns true only on capacity closure.
    pub fn record(&mut self, requests: RecordingRequests) -> bool {
        if self.is_closed() {
            return false;
        }
        // 8001670C counts the new request before flushing its predecessor.
        self.recorded_ticks += 1;
        let controls = requests.controls();
        if let Some(last) = self.clip.runs.last_mut().filter(|r| r.controls == controls) {
            last.ticks += 1;
        } else {
            self.clip.runs.push(ReplayRun { ticks: 1, controls });
        }
        if let Some(pending) = &mut self.pending {
            if pending.requests == requests {
                pending.ticks += 1;
                return false;
            }
            self.flush();
            // Digital transitions change only the retained button channel.
            // The first packet explicitly stores all four channels instead.
            self.pending = Some(PendingRun { requests, ticks: 1, base_bytes: 2 });
            if self.storage_bytes >= self.capacity_bytes.saturating_sub(self.closing_reserve_bytes) {
                self.phase = RecordingPhase::Capacity;
                return true;
            }
        } else {
            self.pending = Some(PendingRun { requests, ticks: 1, base_bytes: 5 });
        }
        false
    }
    fn flush(&mut self) {
        if let Some(pending) = self.pending.take() {
            // 80016598..6A0: three hold bits in the command, then a prefix
            // length. Only this size is retained, not the original encoding.
            let repeats = pending.ticks - 1;
            let extra = match repeats {
                0..=7 => 0,
                8..=1023 => 1,
                1024..=131071 => 2,
                131072..=16777215 => 3,
                _ => 4,
            };
            self.storage_bytes += pending.base_bytes + extra;
        }
    }
    /// 800167D0: cancellation drops the pending packet; ordinary completion
    /// flushes it. A previously closed recording is immutable.
    pub fn finish(&mut self, discard: bool) -> bool {
        if self.is_closed() {
            return false;
        }
        if discard {
            self.phase = RecordingPhase::Discarded;
        } else {
            self.flush();
            self.phase = RecordingPhase::Complete;
        }
        true
    }
    pub fn playback_clip(&self) -> Option<ReplayClip> {
        (self.is_closed() && self.phase != RecordingPhase::Discarded && self.recorded_ticks != 0)
            .then(|| self.clip.clone())
    }
}
#[derive(Component, Clone, Debug, Reflect)]
#[reflect(Component)]
pub struct ReplayRecorder {
    pub recording: InputRecording,
    pub completed_ticks: u16,
}
impl ReplayRecorder {
    pub fn new(settings: &ReplayRecordingSettings) -> Self {
        Self { recording: InputRecording::new(settings), completed_ticks: 0 }
    }
    pub fn advance(
        &mut self,
        settings: &ReplayRecordingSettings,
        requests: RecordingRequests,
        completed: bool,
    ) -> bool {
        if self.recording.is_closed() {
            return false;
        }
        if completed {
            // 80029CBC initializes the elapsed fields to one native tick at
            // the outcome; 80015D10 advances it BEFORE the next driver call.
            if self.completed_ticks == 0 {
                self.completed_ticks = 1;
            }
            self.completed_ticks = self.completed_ticks.saturating_add(1);
            if self.completed_ticks >= settings.post_finish_ticks {
                return self.recording.finish(false);
            }
        }
        self.recording.record(requests)
    }
}
#[derive(Event, Clone, Copy, Debug)]
pub struct ReplayRecordingFinished {
    pub car: Entity,
    pub phase: RecordingPhase,
}
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ReplayRecordingSystems;
pub struct ReplayRecordingRulesPlugin;
impl Plugin for ReplayRecordingRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<ReplayRecordingSettings>()
            && engine::config::asset_path(RECORDING_SETTINGS).is_file()
        {
            app.add_plugins(TuningPlugin::<ReplayRecordingSettings>::new(RECORDING_SETTINGS));
        }
        app.register_type::<ReplayRecorder>()
            .register_type::<ReplayRecordingSettings>()
            .configure_sets(
                FixedUpdate,
                ReplayRecordingSystems
                    .before(crate::vehicle::VehicleSystems::ReadInput)
                    .in_set(EngineSet::Control)
                    .in_set(GameSystems::Game),
            )
            .add_systems(FixedUpdate, record.in_set(ReplayRecordingSystems));
    }
}
fn record(
    settings: Option<Res<ReplayRecordingSettings>>,
    pad: Option<Res<ControllerState>>,
    mut cars: Query<(Entity, &mut ReplayRecorder, &LicenceRun), Without<ReplayPlayback>>,
    mut commands: Commands,
) {
    let (Some(settings), Some(pad)) = (settings, pad) else { return };
    let requests = RecordingRequests::from_pad(&pad);
    for (car, mut recorder, run) in &mut cars {
        if recorder.advance(&settings, requests, *run != LicenceRun::Active) {
            commands.trigger(ReplayRecordingFinished { car, phase: recorder.recording.phase });
        }
    }
}
