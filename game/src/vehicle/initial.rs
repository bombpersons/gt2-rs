//! Native initial vehicle state, shared by the game, mods and trajectory tests.
//! All values use the components' normal units, not disc layouts or addresses.
//! Missing groups use defaults; exported moving checkpoints include retained
//! histories so restoring them does not invent a stationary first tick.
use super::{
    aerodynamics::*, body_dynamics::*, brake_assist::*, braking::*, contact_motion::*, driver_assist::*, drivetrain::*,
    engine_torque::*, forward_dynamics::*, longitudinal_slip::*, road_contact::*, slip::*, suspension::*,
    transmission::*, tyre_forces::*, tyre_grip::*, vertical_body::*, wheel_rotation::*, *,
};
use bevy::ecs::world::EntityWorldMut;
use serde::{Deserialize, Serialize};

pub const INITIAL_STATE_ASSET: &str = "vehicle/initial.ron";

/// Authorable state for the ordinary vehicle controller. This contains only
/// numerical state and independent input boundaries; geometry/tuning stay in
/// their own layered assets. Transient force outputs are rebuilt each tick.
/// See `docs/vehicle-initial-state.md` for the authoring and plugin contract.
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct VehicleInitialState {
    pub pose: VehiclePose,
    pub orientation: BodyOrientation,
    pub velocity: VehicleVelocity,
    pub step_duration: VehicleStepDuration,
    pub pedals: PedalTravel,
    pub steering: SteeringResponse,
    pub steering_kinematics: SteeringKinematics,
    pub local_motion: WheelLocalVelocities,
    pub wheel_motion: WheelSurfaceMotion,
    pub tyre_contact: TyreContactState,
    pub grip_input: TyreGripInput,
    pub tyre_condition: TyreCondition,
    pub engine: EngineTorqueInput,
    pub fuel_cut: EngineFuelCut,
    pub transmission: TransmissionInput,
    pub transmission_state: TransmissionState,
    pub drivetrain: DrivetrainInput,
    pub drivetrain_state: DrivetrainState,
    pub axles: AxleDrive,
    pub wheel_state: WheelRotationState,
    pub body_input: BodyDynamicsInput,
    pub body_dynamics: BodyDynamics,
    pub aerodynamics_input: AerodynamicsInput,
    pub aerodynamics: Aerodynamics,
    pub road_input: RoadContactInput,
    pub road: RoadContactState,
    pub suspension_input: SuspensionInput,
    pub suspension: Suspension,
    pub vertical_input: VerticalBodyInput,
    pub vertical: VerticalBody,
    pub contact_input: ContactMotionInput,
    pub contact_motion: ContactMotion,
    pub chase_stabilization: Option<visual_motion::ChaseStabilization>,
}
impl VehicleInitialState {
    /// Fill fresh authoring defaults from the pose. Exported retained bases
    /// are left intact; they are part of the original tick boundary state.
    fn prepared(&self) -> Self {
        let mut state = self.clone();
        if state.road.body_basis.up == Vec3::ZERO {
            state.road.body_basis =
                vehicle_basis(state.orientation.pitch_turns, state.orientation.roll_turns, state.pose.heading_turns);
        }
        if state.road.road_basis.up == Vec3::ZERO {
            state.road.road_basis =
                vehicle_basis(state.road.road_pitch_turns, state.road.road_roll_turns, state.pose.heading_turns);
        }
        if state.body_input.forward_axis == Vec2::ZERO {
            state.body_input.forward_axis = state.road.body_basis.forward.xz();
        }
        if state.body_input.lateral_axis == Vec2::ZERO {
            state.body_input.lateral_axis = state.road.body_basis.lateral.xz();
        }
        if state.contact_input.forward_axis == Vec3::ZERO {
            state.contact_input.forward_axis = state.road.road_basis.forward;
        }
        if state.contact_input.lateral_axis == Vec3::ZERO {
            state.contact_input.lateral_axis = state.road.road_basis.lateral;
        }
        if state.suspension_input.body_up == 0.0 {
            state.suspension_input.body_up = state.road.body_basis.up.y;
        }
        if state.suspension_input.road_normal_up == 0.0 {
            state.suspension_input.road_normal_up = state.road.road_basis.up.y;
        }
        if state.suspension_input.inverse_step_seconds == 0.0 {
            // Ordinary simulation cadence; the original 80042360 stores the
            // reciprocal separately from its rounded step duration.
            state.suspension_input.inverse_step_seconds = crate::tick_rate(crate::REGION).hz as f32;
        }
        state
    }

    /// Attach the complete ordinary controller to an existing entity. Input
    /// remains externally owned until a caller adds `PlayerPedals`.
    pub fn insert(&self, entity: &mut EntityWorldMut) {
        let state = self.prepared();
        entity.insert((
            state.pose,
            state.orientation,
            state.velocity,
            state.step_duration,
            state.pedals,
            state.steering,
            state.steering_kinematics,
            state.local_motion,
            state.wheel_motion,
            state.tyre_contact,
        ));
        entity.insert((
            state.grip_input,
            state.tyre_condition,
            state.engine,
            state.fuel_cut,
            state.transmission,
            state.transmission_state,
            state.drivetrain,
            state.drivetrain_state,
            state.axles,
            state.wheel_state,
        ));
        entity.insert((
            state.body_input,
            state.body_dynamics,
            state.aerodynamics_input,
            state.aerodynamics,
            state.road_input,
            state.road,
            state.suspension_input,
            state.suspension,
            state.vertical_input,
            state.vertical,
        ));
        entity.insert((state.contact_input, state.contact_motion));
        entity.insert(
            state
                .chase_stabilization
                .unwrap_or_else(|| visual_motion::ChaseStabilization::at(state.pose, state.orientation)),
        );
        entity.insert((
            DigitalPedalInput::default(),
            DigitalSteeringInput::default(),
            MotionStep::default(),
            WheelSteering::default(),
            WheelSlip::default(),
            LongitudinalSlip::default(),
            TyreForces::default(),
            TyreGrip::default(),
            BrakeAssistInput::default(),
            DriverAssistInput::default(),
        ));
        entity.insert((
            DriverAssist::default(),
            WheelBrakeInput::default(),
            WheelBrakeDemand::default(),
            EngineTorque::default(),
            TransmissionOutput::default(),
            DrivetrainOutput::default(),
            WheelRotationInput::default(),
            WheelReactionTorque::default(),
            ForwardDynamics::default(),
            ForwardDynamicsInput::default(),
        ));
        entity.insert((PedalThrottle, PedalBrakes, TyreWheelFeedback, SuspensionForceTransfer));
    }

    pub fn spawn(&self, world: &mut World) -> Entity {
        let mut entity = world.spawn_empty();
        self.insert(&mut entity);
        entity.id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_state_spawns_without_assets_and_preserves_retained_history() {
        let state: VehicleInitialState = ron::from_str(
            r#"(
                pose: (position_m: (12.0, 0.5, -8.0), heading_turns: 0.25),
                wheel_state: (previous_delta_mps: (0.5, -0.25, 0.0, 1.0)),
                transmission_state: (first_gear_ticks: 7),
                suspension: (travel_m: (0.1, 0.2, 0.3, 0.4)),
            )"#,
        )
        .unwrap();
        let mut world = World::new();
        let car = state.spawn(&mut world);
        assert_eq!(world.get::<VehiclePose>(car).unwrap().position_m, Vec3::new(12.0, 0.5, -8.0));
        let basis = vehicle_basis(0.0, 0.0, 0.25);
        assert_eq!(world.get::<BodyDynamicsInput>(car).unwrap().forward_axis, basis.forward.xz());
        assert_eq!(world.get::<ContactMotionInput>(car).unwrap().lateral_axis, basis.lateral);
        assert_eq!(world.get::<SuspensionInput>(car).unwrap().body_up, 1.0);
        assert_eq!(world.get::<WheelRotationState>(car).unwrap().previous_delta_mps, [0.5, -0.25, 0.0, 1.0]);
        assert_eq!(world.get::<TransmissionState>(car).unwrap().first_gear_ticks, 7);
        assert_eq!(world.get::<Suspension>(car).unwrap().travel_m, [0.1, 0.2, 0.3, 0.4]);
        assert!(world.get::<PlayerPedals>(car).is_none(), "logical control remains caller-owned");
        assert!(world.get::<TyreWheelFeedback>(car).is_some());
        assert!(world.get::<SuspensionForceTransfer>(car).is_some());
        assert!(world.get::<TyreForces>(car).is_some());
    }
}
