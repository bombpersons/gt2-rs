//! Ordinary sprite and mesh gauges driven by the native player's vehicle state.
use super::session::SessionSystems;
use crate::vehicle::{
    contact_motion::ContactMotion,
    drivetrain::{DrivetrainInput, EngineEngagement},
    engine_torque::EngineTorqueInput,
    longitudinal_slip::steering_basis,
};
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use engine::{
    prelude::*,
    psx::{PsxAnchor, PsxBlend, PsxMaterial2d, PsxSprite, PsxStp},
};
use serde::{Deserialize, Serialize};

pub const GAUGE_SETTINGS: &str = "race/gauges.ron";

/// Select one vehicle for the HUD independently from its controller or camera.
#[derive(Component, Default, Reflect)]
#[reflect(Component)]
pub struct GaugeSubject;

/// Pixel-space content. Sprite slots: dial, ten digits, units, then gears
/// (reverse followed by forward gears). No source texture/palette addresses.
#[derive(Resource, Clone, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct GaugeSettings {
    pub image: String,
    pub sprites: Vec<[u32; 4]>,
    pub center_px: Vec2,
    pub maximum_rpm: f32,
    pub redline_rpm: f32,
    pub sections_per_1000_rpm: f32,
    pub sweep_turns: f32,
    pub zero_turns: f32,
    pub outer_radius_px: f32,
    pub inner_radius_px: f32,
    pub needles: Vec<NeedleStrip>,
    pub band_rgb: [u8; 3],
    pub redline_rgb: [u8; 3],
    pub dial_rgb: [u8; 3],
    pub text_rgb: [u8; 3],
    pub coupled_gear_rgb: [u8; 3],
    pub free_gear_rgb: [u8; 3],
    pub gear_offset_px: Vec2,
    pub units_offset_px: Vec2,
    pub speed_offset_px: Vec2,
    pub digit_step_px: f32,
    pub gear_shade_offset_px: Vec2,
    pub gear_shade_size_px: Vec2,
    pub gear_shade_rgb: [u8; 3],
}

#[derive(Clone, Debug, Reflect, Serialize, Deserialize)]
pub struct NeedleStrip {
    pub distance_px: [f32; 2],
    pub half_width_px: [f32; 2],
    pub edge_rgb: [[u8; 3]; 2],
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct GaugeDisplay {
    pub rpm: i16,
    /// Hundredths of the selected display unit, already refreshed by contact motion.
    pub speed_readout: u16,
    pub gear: usize,
    pub coupled: bool,
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum GaugeBlend {
    Opaque,
    Half,
    Add,
    Subtract,
}
impl GaugeBlend {
    fn psx(self) -> PsxBlend {
        match self {
            Self::Opaque => PsxBlend::Opaque,
            Self::Half => PsxBlend::Half,
            Self::Add => PsxBlend::Add,
            Self::Subtract => PsxBlend::Subtract,
        }
    }
    fn index(self) -> usize {
        self as usize
    }
}

/// Screen pixels in draw order. Quad vertices follow the ordinary triangle-strip
/// order (top-left, top-right, bottom-left, bottom-right for rectangles).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GaugeDraw {
    pub sprite: Option<usize>,
    pub vertices: [[f32; 2]; 4],
    pub rgb: [[u8; 3]; 4],
    pub blend: GaugeBlend,
}

// 8002BCF0 rotates a 1/16-pixel design shape with the same sampled circle
// already used by steering. Each product truncates separately before the
// final asymmetric screen-pixel bias (+4 x, -8 y).
fn rotate(point: Vec2, angle: i32) -> Vec2 {
    let [c, s] = steering_basis(angle as f32 / 4096.0).map(|v| (v * 4096.0) as i32);
    let [x, y] = (point * 16.0).to_array().map(|v| v.round() as i32);
    Vec2::new(
        ((((x * c) >> 12) - ((y * s) >> 12) + 4) >> 4) as f32,
        ((((y * c) >> 12) + ((x * s) >> 12) - 8) >> 4) as f32,
    )
}

fn centered(point: Vec2, center: Vec2) -> Vec2 {
    // The original centers packed signed halfwords. Crossing from a negative
    // local x to positive screen x carries one pixel into y (8002BFC8).
    center + point + Vec2::Y * f32::from(point.x < 0.0 && center.x + point.x >= 0.0)
}

fn rectangle(sprite: Option<usize>, top: Vec2, size: Vec2, rgb: [u8; 3], blend: GaugeBlend) -> GaugeDraw {
    GaugeDraw {
        sprite,
        vertices: [
            top.to_array(),
            (top + Vec2::X * size.x).to_array(),
            (top + Vec2::Y * size.y).to_array(),
            (top + size).to_array(),
        ],
        rgb: [rgb; 4],
        blend,
    }
}

pub fn layout_gauges(settings: &GaugeSettings, view: &GaugeDisplay) -> Vec<GaugeDraw> {
    if !view.visible {
        return Vec::new();
    }
    let s = settings;
    let sweep = (s.sweep_turns * 4096.0).round() as i32;
    let zero = (s.zero_turns * 4096.0).round() as i32;
    let sections = (s.maximum_rpm * s.sections_per_1000_rpm / 1000.0) as i32;
    let redline = (s.redline_rpm * s.sections_per_1000_rpm / 1000.0) as i32;
    let point =
        |i: i32, radius| centered(rotate(Vec2::new(radius, 0.0), zero + i * sweep / sections), s.center_px).to_array();
    let mut out = Vec::new();
    for i in (0..sections).rev() {
        out.push(GaugeDraw {
            sprite: None,
            vertices: [
                point(i, s.outer_radius_px),
                point(i, s.inner_radius_px),
                point(i + 1, s.outer_radius_px),
                point(i + 1, s.inner_radius_px),
            ],
            rgb: [if i >= redline { s.redline_rgb } else { s.band_rgb }; 4],
            blend: GaugeBlend::Half,
        });
    }
    let sprite = |slot: usize, offset: Vec2, rgb, blend| {
        let rect = s.sprites[slot];
        let size = Vec2::new(rect[2] as f32, rect[3] as f32);
        rectangle(Some(slot), s.center_px + offset - (size / 2.0).floor(), size, rgb, blend)
    };
    out.push(sprite(0, Vec2::ZERO, s.dial_rgb, GaugeBlend::Add));
    let speed = view.speed_readout / 100;
    let mut digits = Vec::new();
    let mut leading = true;
    for (i, divisor) in [100, 10, 1].into_iter().enumerate() {
        let digit = usize::from(speed / divisor % 10);
        if !leading || digit > 0 || divisor == 1 {
            digits.push(sprite(
                1 + digit,
                s.speed_offset_px + Vec2::X * s.digit_step_px * i as f32,
                s.text_rgb,
                GaugeBlend::Opaque,
            ));
            leading = false;
        }
    }
    out.extend(digits.into_iter().rev());
    out.push(sprite(11, s.units_offset_px, s.text_rgb, GaugeBlend::Opaque));
    out.push(rectangle(
        None,
        s.center_px + s.gear_shade_offset_px,
        s.gear_shade_size_px,
        s.gear_shade_rgb,
        GaugeBlend::Subtract,
    ));
    if 12 + view.gear < s.sprites.len() {
        out.push(sprite(
            12 + view.gear,
            s.gear_offset_px,
            if view.coupled { s.coupled_gear_rgb } else { s.free_gear_rgb },
            GaugeBlend::Opaque,
        ));
    }
    let angle = zero + i32::from(view.rpm) * sweep / s.maximum_rpm as i32;
    for needle in &s.needles {
        let mut vertices = [[0.0; 2]; 4];
        for i in 0..2 {
            for j in 0..2 {
                vertices[i * 2 + j] = centered(
                    rotate(
                        Vec2::new(needle.distance_px[i], needle.half_width_px[i] * if j == 0 { -1.0 } else { 1.0 }),
                        angle,
                    ),
                    s.center_px,
                )
                .to_array();
            }
        }
        out.push(GaugeDraw {
            sprite: None,
            vertices,
            rgb: [needle.edge_rgb[0], needle.edge_rgb[1], needle.edge_rgb[0], needle.edge_rgb[1]],
            blend: GaugeBlend::Opaque,
        });
    }
    out
}

pub struct GaugeRulesPlugin;
impl Plugin for GaugeRulesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GaugeDisplay>()
            .register_type::<GaugeSubject>()
            .register_type::<GaugeDisplay>()
            .add_systems(
                FixedUpdate,
                update_display.after(SessionSystems::Clock).in_set(EngineSet::Objects).in_set(GameSystems::Game),
            );
    }
}
fn update_display(
    cars: Query<(&EngineTorqueInput, &ContactMotion, &DrivetrainInput), With<GaugeSubject>>,
    settings: Option<Res<GaugeSettings>>,
    mut view: ResMut<GaugeDisplay>,
) {
    if settings.is_none() {
        return;
    }
    let Ok((engine, motion, drive)) = cars.single() else {
        return;
    };
    let next = GaugeDisplay {
        rpm: engine.integer_rpm as i16,
        speed_readout: motion.speed_readout,
        gear: drive.gear,
        coupled: drive.engagement == EngineEngagement::Coupled,
        visible: true,
    };
    if *view != next {
        *view = next;
    }
}

/// The game's HUD includes these gauges, but a mod can also select this alone
/// with its own Camera2d/PsxView and authored GaugeSettings.
pub struct GaugePlugin;
impl Plugin for GaugePlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<GaugeSettings>() && engine::config::asset_path(GAUGE_SETTINGS).is_file() {
            app.add_plugins(TuningPlugin::<GaugeSettings>::new(GAUGE_SETTINGS));
        }
        app.add_plugins(GaugeRulesPlugin).add_systems(Startup, spawn_gauges).add_systems(Update, render_gauges);
    }
}
#[derive(Resource)]
struct GaugeAssets {
    image: Handle<Image>,
    sprites: [Handle<PsxSprite>; 4],
    polygons: [Handle<PsxMaterial2d>; 4],
}
#[derive(Component)]
struct GaugeVisual;
fn spawn_gauges(
    mut commands: Commands,
    settings: Option<Res<GaugeSettings>>,
    server: Option<Res<AssetServer>>,
    sprites: Option<ResMut<Assets<PsxSprite>>>,
    polygons: Option<ResMut<Assets<PsxMaterial2d>>>,
) {
    let (Some(s), Some(server), Some(mut sprites), Some(mut polygons)) = (settings, server, sprites, polygons) else {
        return;
    };
    let blends = [GaugeBlend::Opaque, GaugeBlend::Half, GaugeBlend::Add, GaugeBlend::Subtract];
    commands.insert_resource(GaugeAssets {
        image: server.load(s.image.clone()),
        sprites: blends.map(|b| sprites.add(PsxSprite { blend: b.psx(), stp: PsxStp::Texel, ..default() })),
        polygons: blends.map(|b| polygons.add(PsxMaterial2d { blend: b.psx(), dither: false, ..default() })),
    });
}

fn render_gauges(
    mut commands: Commands,
    settings: Option<Res<GaugeSettings>>,
    assets: Option<Res<GaugeAssets>>,
    view: Res<GaugeDisplay>,
    visuals: Query<Entity, With<GaugeVisual>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
) {
    let (Some(s), Some(a), Some(mut meshes)) = (settings, assets, meshes) else {
        return;
    };
    if !view.is_changed() && !s.is_changed() {
        return;
    }
    for entity in &visuals {
        commands.entity(entity).despawn();
    }
    for (layer, draw) in layout_gauges(&s, &view).into_iter().enumerate() {
        let anchor = PsxAnchor::new(Vec2::new(1.0, -1.0), Vec2::new(-(crate::SCREEN.x as f32), crate::SCREEN.y as f32));
        let z = 20.0 + layer as f32 * 0.01;
        if let Some(slot) = draw.sprite {
            let [x, y, w, h] = s.sprites[slot];
            let rgb = draw.rgb[0];
            let at = (Vec2::from(draw.vertices[0]) + Vec2::from(draw.vertices[3])) / 2.0;
            commands.spawn((
                GaugeVisual,
                Sprite {
                    image: a.image.clone(),
                    rect: Some(Rect::new(x as f32, y as f32, (x + w) as f32, (y + h) as f32)),
                    color: Color::srgb(rgb[0] as f32 / 128.0, rgb[1] as f32 / 128.0, rgb[2] as f32 / 128.0),
                    ..default()
                },
                SpriteMaterial(a.sprites[draw.blend.index()].clone()),
                Transform::from_xyz(0.0, 0.0, z),
                PsxAnchor::new(anchor.at, anchor.offset + Vec2::new(at.x, -at.y)),
            ));
        } else {
            let positions: Vec<_> = draw.vertices.map(|[x, y]| [x, -y, 0.0]).into();
            let colors: Vec<_> = draw
                .rgb
                .map(|c| {
                    Color::srgb(c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0)
                        .to_linear()
                        .to_f32_array()
                })
                .into();
            let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
                .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
                .with_inserted_indices(Indices::U32(vec![0, 1, 2, 2, 1, 3]));
            commands.spawn((
                GaugeVisual,
                Mesh2d(meshes.add(mesh)),
                MeshMaterial2d(a.polygons[draw.blend.index()].clone()),
                Transform::from_xyz(0.0, 0.0, z),
                anchor,
            ));
        }
    }
}
