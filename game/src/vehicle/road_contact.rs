//! Sample decoded course polygons and prepare smoothed wheel/road frames.
//!
//! Road geometry is engine data. Height history, surface aliasing, procedural
//! bumps and vehicle projection are this game's rules (80043388..43780 and
//! 8003E7EC). Vertical collision/body-angle integration remain separate.
use super::{
    VehiclePose, VehicleSystems,
    body_dynamics::BodyDynamicsInput,
    contact_motion::{ContactMotionInput, ContactMotionSystems},
    direction::direction_turns,
    longitudinal_slip::steering_basis,
    slip::WheelLocalVelocities,
    suspension::SuspensionInput,
    tyre_grip::TyreGripInput,
};
use bevy::{ecs::query::QueryData, prelude::*};
use engine::{
    course::CourseData,
    prelude::{TuningPlugin, load_ron},
    shell::GameSystems,
};
use serde::{Deserialize, Serialize};

pub const ROAD_CONTACT_TUNING: &str = "vehicle/road_contact.ron";
#[derive(Resource, Clone)]
pub struct RoadContactCourse(pub CourseData);
#[derive(Clone, Debug, Reflect, Deserialize, Serialize)]
pub struct SurfaceBump {
    /// Globals 80046F88/6F98, converted to metres and cycles/metre.
    pub amplitude_m: f32,
    pub cycles_per_m: f32,
    /// 80046FA8 nonzero scales the waveform by wheel motion response.
    pub scale_with_speed: bool,
}
#[derive(Resource, Clone, Debug, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct RoadContactTuning {
    /// Physics+18/0C/1A/0E: side/forward lever arms in metres.
    pub wheel_levers_m: [[f32; 2]; 4],
    /// Physics+10/+12/+14, wheelbase, track and fore/aft interpolation.
    pub wheelbase_m: f32,
    pub track_m: f32,
    pub rear_height_fraction: f32,
    pub bumps: Vec<SurfaceBump>,
    /// 80043428 immediate /4096. Approximate speed reaches full bump at its reciprocal.
    pub bump_response_per_mps: f32,
    pub angle_threshold_ratios: Vec<f32>,
}
#[derive(Component, Clone, Debug, Reflect, Deserialize, Serialize)]
#[reflect(Component)]
pub struct RoadContactSetup(pub RoadContactTuning);
#[derive(Component, Default, Clone, Copy, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
#[serde(default)]
pub struct BodyOrientation {
    /// Physics+644/+646 /4096 turns, updated by vertical body integration;
    /// yaw comes from VehiclePose.
    pub pitch_turns: f32,
    pub roll_turns: f32,
}
#[derive(Component, Default, Clone, Copy, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
#[serde(default)]
pub struct RoadContactInput {
    /// Race-mode gate returned by 80041AB8; licence/replay mode supplies it.
    pub condition_surfaces_enabled: bool,
}
#[derive(Default, Clone, Copy, Debug, Reflect, Serialize, Deserialize)]
#[serde(default)]
pub struct VehicleBasis {
    pub forward: Vec3,
    pub lateral: Vec3,
    pub up: Vec3,
}
#[derive(Component, Default, Clone, Copy, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
#[serde(default)]
pub struct RoadContactState {
    /// Source car+600. The first wheel's longitudinal result becomes the car seed.
    pub node: u16,
    /// Wheel+4 /65536 metres; retained when no polygon was found.
    pub raw_height_m: [f64; 4],
    /// Wheel+0 /4096 metres: sampled mean of current and prior raw height.
    pub height_m: [f32; 4],
    /// Wheel+14/+15, retained on a missing surface; source surface 6 aliases 5.
    pub surface: [u8; 4],
    pub variant: [u8; 4],
    pub road_offset_m: [f32; 4],
    pub attachment_m: [Vec3; 4],
    pub query_m: [[f64; 3]; 4],
    pub bump_response: [f32; 4],
    pub missing_mask: u8,
    pub offroad_wheels: u8,
    pub special_wheels: u8,
    pub reset_surface_wheels: u8,
    pub bypass_condition: bool,
    /// Physics+688, +6F4/+6F6 and +68C..6A0 in normal world units.
    pub road_height_m: f32,
    pub road_pitch_turns: f32,
    pub road_roll_turns: f32,
    pub body_basis: VehicleBasis,
    pub road_basis: VehicleBasis,
}

/// Sampled Euler basis from 80044EA4; normal Bevy (x,up,-source_y) axes.
/// Products are rounded at the same intermediate boundaries as the source.
pub fn vehicle_basis(pitch: f32, roll: f32, heading: f32) -> VehicleBasis {
    let [cp, sp] = steering_basis(pitch).map(f64::from);
    let [cr, sr] = steering_basis(roll).map(f64::from);
    let [ch, sh] = steering_basis(heading).map(f64::from);
    let q = |v: f64| (v * 4096.0).trunc() / 4096.0;
    let x = q(ch * sr);
    let y = q(sh * sr);
    VehicleBasis {
        forward: Vec3::new(q(x * sp - sh * cp) as f32, q(cr * sp) as f32, -q(y * sp + ch * cp) as f32),
        lateral: Vec3::new(q(ch * cr) as f32, -sr as f32, -q(sh * cr) as f32),
        up: Vec3::new(q(x * cp + sh * sp) as f32, q(cr * cp) as f32, -q(y * cp - ch * sp) as f32),
    }
}
fn short_m(value: f64) -> f32 {
    ((value * 4096.0).round() as i64 as i16) as f32 / 4096.0
}
fn sample_m(value: f64) -> f32 {
    ((value * 4096.0).trunc() / 4096.0) as f32
}
fn low_product(a: f32, b: f32) -> f32 {
    let a = (a as f64 * 4096.0).round() as i64;
    let b = (b as f64 * 4096.0).round() as i64;
    let product = a.wrapping_mul(b) as i32;
    let product = product.wrapping_add(if (a < 0) != (b < 0) { 4095 } else { 0 });
    (product >> 12) as f32 / 4096.0
}

impl RoadContactTuning {
    /// 8004335C refreshes all lever attachments using the retained body
    /// basis before the later angle-to-basis rebuild at 8003E7EC.
    pub fn refresh_attachments(&self, state: &mut RoadContactState) {
        for i in 0..4 {
            let [side, forward] = self.wheel_levers_m[i];
            // 800431A0/4323C project each lever separately, then narrow the
            // combined attachment to a signed halfword. Rear levers are
            // negated after multiplication, preserving signed sine bias.
            let forward_length = forward.abs();
            let side_length = side.abs();
            let project = |axis: usize| {
                let f = low_product(state.body_basis.forward[axis], forward_length);
                let s = low_product(state.body_basis.lateral[axis], side_length);
                short_m((if forward < 0.0 { -f } else { f }) as f64 + (if side < 0.0 { -s } else { s }) as f64)
            };
            state.attachment_m[i] = Vec3::new(project(0), project(1), project(2));
        }
    }

    pub fn sample(
        &self,
        course: &CourseData,
        pose: VehiclePose,
        local: WheelLocalVelocities,
        input: RoadContactInput,
        state: &mut RoadContactState,
    ) {
        state.missing_mask = 0;
        state.offroad_wheels = 0;
        state.special_wheels = 0;
        state.reset_surface_wheels = 0;
        self.refresh_attachments(state);
        let seed = state.node;
        let mut surface_flags = 0;
        for i in 0..4 {
            state.query_m[i] =
                std::array::from_fn(|axis| pose.position_m[axis] as f64 + state.attachment_m[i][axis] as f64);
            let a = local.forward_mps[i].abs();
            let b = local.lateral_mps[i].abs();
            // 8003C360 is max + ceil(min/2), not a Euclidean norm.
            let speed = a + b - ((a.min(b) as f64 * 4096.0 / 2.0).floor() / 4096.0) as f32;
            state.bump_response[i] =
                ((speed as f64 * self.bump_response_per_mps as f64 * 4096.0).floor() / 4096.0).min(1.0) as f32;
            if i == 0
                && let Some(node) = course.node_at_position(seed, state.query_m[i])
            {
                state.node = node;
            }
            let Some(contact) = course.road_contact(seed, state.query_m[i]) else {
                state.missing_mask |= 1 << i;
                state.reset_surface_wheels += 1;
                state.road_offset_m[i] = 0.0;
                continue;
            };
            if i == 0 {
                state.node = contact.node;
            }
            // 8004362C..34: a floored mean at the /4096 output boundary.
            state.height_m[i] = (((contact.height_m + state.raw_height_m[i]) * 2048.0).floor() / 4096.0) as f32;
            state.raw_height_m[i] = contact.height_m;
            state.surface[i] = contact.surface;
            state.variant[i] = contact.variant;
            surface_flags |= contact.flags;
            if contact.surface >= 2 {
                state.offroad_wheels += 1;
            }
            if contact.flags & 2 != 0 {
                state.special_wheels += 1;
            }
            if contact.surface == 6 {
                state.surface[i] = 5;
                state.reset_surface_wheels += 1;
            }
            let bump = &self.bumps[state.surface[i] as usize];
            let mut offset = 0.0;
            if bump.amplitude_m != 0.0 {
                let phase =
                    |position: f64| (position * bump.cycles_per_m as f64 * 4096.0).floor().rem_euclid(4096.0) / 4096.0;
                let sine = steering_basis(phase(state.query_m[i][0]) as f32)[1];
                let cosine = steering_basis(phase(-state.query_m[i][2]) as f32)[0];
                let wave = if bump.scale_with_speed {
                    low_product(sine + cosine, state.bump_response[i])
                } else {
                    sine + cosine
                };
                offset = short_m(((wave as f64 * bump.amplitude_m as f64 * 4096.0).floor()) / 4096.0);
            }
            state.road_offset_m[i] = offset;
        }
        state.bypass_condition = surface_flags & 2 != 0 && input.condition_surfaces_enabled;
    }
    pub fn refresh_frame(&self, heading: f32, state: &mut RoadContactState) {
        // 8003E804..898: fore/aft interpolation on each side, then pitch,
        // roll and centre height. Interpolation uses the signed low word.
        let left = state.height_m[0] + low_product(state.height_m[2] - state.height_m[0], self.rear_height_fraction);
        let right = state.height_m[1] + low_product(state.height_m[3] - state.height_m[1], self.rear_height_fraction);
        state.road_height_m = sample_m((left as f64 + right as f64) / 2.0);
        let tilt = sample_m(
            (state.height_m[0] as f64 + state.height_m[1] as f64 - state.height_m[2] as f64 - state.height_m[3] as f64)
                / 2.0,
        );
        state.road_pitch_turns = direction_turns(&self.angle_threshold_ratios, self.wheelbase_m, tilt);
        state.road_roll_turns = direction_turns(&self.angle_threshold_ratios, self.track_m, left - right);
        state.road_basis = vehicle_basis(state.road_pitch_turns, state.road_roll_turns, heading);
    }
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoadContactSystems {
    Sample,
    RefreshFrame,
}
pub struct RoadContactRulesPlugin;
impl Plugin for RoadContactRulesPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<RoadContactTuning>() {
            app.add_plugins(TuningPlugin::<RoadContactTuning>::new(ROAD_CONTACT_TUNING));
        }
        if !app.world().contains_resource::<RoadContactCourse>() {
            app.insert_resource(RoadContactCourse(load_ron(crate::race::COURSE_ASSET).expect("road contact course")));
        }
        for (node, data) in app.world().resource::<RoadContactCourse>().0.nodes.iter().enumerate() {
            if let Some(mesh) = &data.contact {
                mesh.validate().unwrap_or_else(|error| panic!("road contact node {node}: {error}"));
            }
        }
        app.register_type::<RoadContactState>()
            .register_type::<BodyOrientation>()
            .register_type::<RoadContactInput>()
            .configure_sets(
                FixedUpdate,
                VehicleSystems::SampleRoad
                    .after(VehicleSystems::Prepare)
                    .after(VehicleSystems::Integrate)
                    .before(VehicleSystems::Suspension)
                    .before(VehicleSystems::RefreshContact)
                    .in_set(GameSystems::Game),
            )
            .add_systems(FixedUpdate, sample.in_set(RoadContactSystems::Sample).in_set(VehicleSystems::SampleRoad))
            .add_systems(
                FixedUpdate,
                refresh_frame
                    .in_set(RoadContactSystems::RefreshFrame)
                    .in_set(VehicleSystems::RefreshContact)
                    .before(ContactMotionSystems::Refresh),
            );
    }
}
#[derive(QueryData)]
#[query_data(mutable)]
struct RoadCar {
    pose: &'static VehiclePose,
    local: &'static WheelLocalVelocities,
    input: &'static RoadContactInput,
    state: &'static mut RoadContactState,
    setup: Option<&'static RoadContactSetup>,
    suspension: Option<&'static mut SuspensionInput>,
    grip: Option<&'static mut TyreGripInput>,
}
fn sample(tuning: Res<RoadContactTuning>, course: Res<RoadContactCourse>, mut cars: Query<RoadCar>) {
    for mut car in &mut cars {
        car.setup.map(|v| &v.0).unwrap_or(&tuning).sample(&course.0, *car.pose, *car.local, *car.input, &mut car.state);
        if let Some(mut input) = car.suspension {
            input.body_height_m = car.pose.position_m.y;
            input.road_height_m = car.state.height_m;
            input.attachment_height_m = car.state.attachment_m.map(|v| v.y);
            input.road_offset_m = car.state.road_offset_m;
            input.body_up = car.state.body_basis.up.y;
            input.road_normal_up = car.state.road_basis.up.y;
        }
        if let Some(mut grip) = car.grip {
            grip.surface = car.state.surface.map(usize::from);
            grip.bypass_condition = car.state.bypass_condition;
        }
    }
}
#[derive(QueryData)]
#[query_data(mutable)]
struct FrameCar {
    pose: &'static VehiclePose,
    orientation: &'static BodyOrientation,
    state: &'static mut RoadContactState,
    setup: Option<&'static RoadContactSetup>,
    body: Option<&'static mut BodyDynamicsInput>,
    motion: Option<&'static mut ContactMotionInput>,
    grip: Option<&'static mut TyreGripInput>,
}
fn refresh_frame(tuning: Res<RoadContactTuning>, mut cars: Query<FrameCar>) {
    for mut car in &mut cars {
        car.setup.map(|v| &v.0).unwrap_or(&tuning).refresh_attachments(&mut car.state);
        car.state.body_basis =
            vehicle_basis(car.orientation.pitch_turns, car.orientation.roll_turns, car.pose.heading_turns);
        car.setup.map(|v| &v.0).unwrap_or(&tuning).refresh_frame(car.pose.heading_turns, &mut car.state);
        if let Some(mut body) = car.body {
            body.forward_axis = car.state.body_basis.forward.xz();
            body.lateral_axis = car.state.body_basis.lateral.xz();
        }
        if let Some(mut grip) = car.grip {
            grip.roll_difference_turns = car.orientation.roll_turns - car.state.road_roll_turns;
        }
        if let Some(mut motion) = car.motion {
            motion.forward_axis = car.state.road_basis.forward;
            motion.lateral_axis = car.state.road_basis.lateral;
        }
    }
}
