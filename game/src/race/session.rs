//! Start-grid hold and driving clock, independently usable without presentation.
//!
//! The original decrements its field countdown before physics (80015CE0..CFC)
//! and advances a 3000 Hz clock after physics (8003D168). A native tick covers
//! two fields, so keep whole ticks and convert to milliseconds only for display.
use bevy::prelude::*;
use engine::{prelude::EngineSet, shell::GameSystems};
use serde::{Deserialize, Serialize};

use crate::vehicle::{
    VehicleMotionGate, VehicleSystems, body_dynamics::BodyDynamicsInput, suspension::SuspensionInput,
    visual_motion::VehicleVisualSystems,
};

/// An optional session. Inserting this resource enables its clock; ordinary
/// subsystem tests and mods can run vehicles without a session controller.
#[derive(Resource, Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct DrivingSession {
    pub phase: DrivingPhase,
    pub elapsed_ticks: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum DrivingPhase {
    Countdown { fields_left: u16 },
    Driving,
}

impl DrivingSession {
    /// The duration is content/session setup; B-1's observed initial value is
    /// 120 fields (800159A4), rather than a vehicle-controller constant.
    pub fn countdown(fields: u16) -> Self {
        Self { phase: DrivingPhase::Countdown { fields_left: fields }, elapsed_ticks: 0 }
    }

    pub fn held_at_start(&self) -> bool {
        matches!(self.phase, DrivingPhase::Countdown { fields_left } if fields_left != 0)
    }

    pub fn elapsed_millis(&self) -> u64 {
        u64::from(self.elapsed_ticks) * 1000 / 30
    }
}

/// Let this session supply the vehicle's reset and suspension gates. Vehicles
/// without this marker retain ownership of those explicit subsystem inputs.
#[derive(Component, Default, Reflect)]
#[reflect(Component)]
pub struct SessionParticipant;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionSystems {
    Begin,
    Clock,
}

pub struct SessionRulesPlugin;
impl Plugin for SessionRulesPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            FixedUpdate,
            SessionSystems::Begin.before(VehicleSystems::Prepare).in_set(EngineSet::Control).in_set(GameSystems::Game),
        )
        .configure_sets(
            FixedUpdate,
            SessionSystems::Clock
                .after(VehicleSystems::RefreshContact)
                .after(VehicleVisualSystems::Stabilize)
                .in_set(EngineSet::Objects)
                .in_set(GameSystems::Game),
        )
        .add_systems(FixedUpdate, begin.in_set(SessionSystems::Begin))
        .add_systems(FixedUpdate, advance_clock.in_set(SessionSystems::Clock));
    }
}

fn begin(
    session: Option<ResMut<DrivingSession>>,
    mut cars: Query<
        (&mut BodyDynamicsInput, &mut SuspensionInput, Option<&mut VehicleMotionGate>),
        With<SessionParticipant>,
    >,
) {
    let Some(mut session) = session else { return };
    if let DrivingPhase::Countdown { fields_left } = session.phase {
        let fields_left = fields_left.saturating_sub(crate::FIELDS_PER_TICK as u16);
        session.phase = if fields_left == 0 { DrivingPhase::Driving } else { DrivingPhase::Countdown { fields_left } };
    }
    let held = session.held_at_start();
    for (mut body, mut suspension, motion) in &mut cars {
        // 8003E120 resets world motion; 8003E91C skips suspension and its
        // vertical correction. Session participants use the ordinary mode.
        body.reset_motion = held;
        suspension.disabled = held;
        if let Some(mut motion) = motion {
            motion.blocked = held;
        }
    }
}

fn advance_clock(session: Option<ResMut<DrivingSession>>) {
    let Some(mut session) = session else { return };
    if !session.held_at_start() {
        session.elapsed_ticks += 1;
    }
}
