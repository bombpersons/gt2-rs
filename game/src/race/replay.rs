//! Native recorded driver requests, replayed through the ordinary car rules.
//! Source playback boundary: 80016428 -> 80013C90 -> 8003E2C8.
//! Disc/RAM packet compression is confined to the offline replay importer.
use crate::vehicle::{DigitalGearInput, DigitalPedalInput, DigitalSteeringInput, VehicleSystems};
use bevy::prelude::*;
use engine::prelude::*;
use serde::{Deserialize, Serialize};

/// Effective digital requests; physical pad bindings and result-menu input
/// remain independent of the driver that supplies these components.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub struct ReplayControls {
    pub throttle: bool,
    pub brake: bool,
    pub handbrake: bool,
    pub steering: i8,
    pub reverse: bool,
    pub shift: i8,
}
impl ReplayControls {
    pub fn from_driver(pedals: DigitalPedalInput, steering: DigitalSteeringInput, gears: DigitalGearInput) -> Self {
        Self {
            throttle: pedals.throttle,
            brake: pedals.brake,
            handbrake: pedals.handbrake,
            steering: i8::from(steering.direction > 0.0) - i8::from(steering.direction < 0.0),
            reverse: gears.reverse,
            shift: gears.shift_direction.signum(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub struct ReplayRun {
    pub ticks: u32,
    pub controls: ReplayControls,
}
/// Run-length input data, in ticks at this game's fixed cadence. Ordinary RON
/// content can be recorded or authored without the original packet format.
#[derive(Clone, Default, Debug, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub struct ReplayClip {
    pub runs: Vec<ReplayRun>,
}
impl ReplayClip {
    pub fn ticks(&self) -> Result<u32, String> {
        self.runs.iter().try_fold(0u32, |total, run| {
            if run.ticks == 0 || !(-1..=1).contains(&run.controls.steering) || !(-1..=1).contains(&run.controls.shift) {
                return Err("replay runs require positive ticks and signed unit driver directions".into());
            }
            total.checked_add(run.ticks).ok_or_else(|| "replay duration exceeds u32 ticks".into())
        })
    }
    /// Coalesce identical requests. This is native content recording; original
    /// bounded-buffer recording/overflow policy is a separate session rule.
    pub fn record(&mut self, controls: ReplayControls) -> Result<(), String> {
        if self.ticks()? == u32::MAX {
            return Err("replay duration exceeds u32 ticks".into());
        }
        if !(-1..=1).contains(&controls.steering) || !(-1..=1).contains(&controls.shift) {
            return Err("replay driver directions must be -1, 0 or 1".into());
        }
        if let Some(last) = self.runs.last_mut().filter(|r| r.controls == controls) {
            last.ticks = last.ticks.checked_add(1).ok_or("replay run exceeds u32 ticks")?;
        } else {
            self.runs.push(ReplayRun { ticks: 1, controls });
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayTick {
    Input(ReplayControls),
    Finished,
    Inactive,
}
#[derive(Component, Clone, Debug, Reflect)]
#[reflect(Component)]
pub struct ReplayPlayback {
    pub clip: ReplayClip,
    /// Number of playback calls already consumed, including the terminal tick.
    pub elapsed_ticks: u32,
    pub total_ticks: u32,
    run: usize,
    run_ticks_left: u32,
    finished: bool,
}
impl ReplayPlayback {
    pub fn new(clip: ReplayClip) -> Result<Self, String> {
        let total_ticks = clip.ticks()?;
        Ok(Self { clip, total_ticks, elapsed_ticks: 0, run: 0, run_ticks_left: 0, finished: false })
    }
    pub fn advance(&mut self) -> ReplayTick {
        if self.finished {
            return ReplayTick::Inactive;
        }
        // 80016430..458 decrements the saved duration BEFORE reading input.
        // Its last recorded tick is a neutral terminal tick, not another run
        // sample. 80013CC0 clears all five output channels before this call.
        if self.total_ticks.saturating_sub(self.elapsed_ticks) <= 1 {
            self.elapsed_ticks = self.total_ticks;
            self.finished = true;
            return ReplayTick::Finished;
        }
        if self.run_ticks_left == 0 {
            self.run_ticks_left = self.clip.runs[self.run].ticks;
        }
        let controls = self.clip.runs[self.run].controls;
        self.run_ticks_left -= 1;
        if self.run_ticks_left == 0 {
            self.run += 1;
        }
        self.elapsed_ticks += 1;
        ReplayTick::Input(controls)
    }
    pub fn held_ticks_remaining(&self) -> u32 {
        if self.finished { 0 } else { self.run_ticks_left }
    }
    pub fn is_finished(&self) -> bool {
        self.finished
    }
}
#[derive(Event, Clone, Copy, Debug)]
pub struct ReplayInputsFinished {
    pub car: Entity,
}
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ReplayInputSystems;
/// Optional input source. Does not reset the car, session, results or camera;
/// the session controller owns those transitions and retains the completed run.
pub struct ReplayInputRulesPlugin;
impl Plugin for ReplayInputRulesPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<ReplayPlayback>()
            .configure_sets(
                FixedUpdate,
                ReplayInputSystems
                    .after(VehicleSystems::ReadInput)
                    .before(crate::vehicle::transmission::TransmissionSystems::DriverRequests)
                    .before(VehicleSystems::Pedals)
                    .in_set(EngineSet::Control)
                    .in_set(GameSystems::Game),
            )
            .add_systems(FixedUpdate, play.in_set(ReplayInputSystems));
    }
}
fn play(
    mut cars: Query<(
        Entity,
        &mut ReplayPlayback,
        &mut DigitalPedalInput,
        &mut DigitalSteeringInput,
        &mut DigitalGearInput,
    )>,
    mut commands: Commands,
) {
    for (entity, mut playback, mut pedals, mut steering, mut gears) in &mut cars {
        let controls = match playback.advance() {
            ReplayTick::Input(input) => input,
            ReplayTick::Finished => {
                commands.trigger(ReplayInputsFinished { car: entity });
                ReplayControls::default()
            }
            // The caller's output is cleared on EVERY inactive source call.
            ReplayTick::Inactive => ReplayControls::default(),
        };
        *pedals =
            DigitalPedalInput { throttle: controls.throttle, brake: controls.brake, handbrake: controls.handbrake };
        steering.direction = f32::from(controls.steering);
        *gears = DigitalGearInput { reverse: controls.reverse, shift_direction: controls.shift };
    }
}
