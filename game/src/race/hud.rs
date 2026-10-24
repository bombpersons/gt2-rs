//! Bitmap countdown/start announcement and B-1 lap clock. The drawing data is
//! native font/layout content; no original draw packets or VRAM exist here.
use super::session::{DrivingPhase, DrivingSession, SessionSystems};
use bevy::prelude::*;
use engine::{
    bitmap_font::{BitmapFont, PositionedGlyph},
    prelude::*,
    psx::{PsxAnchor, PsxBlend, PsxSprite, PsxView},
};
use serde::{Deserialize, Serialize};

pub const HUD_SETTINGS: &str = "race/hud.ron";

#[derive(Resource, Clone, Debug, Reflect, Deserialize, Serialize)]
#[reflect(Resource)]
pub struct HudSettings {
    pub small_font: String,
    pub large_font: String,
    pub label: String,
    pub start_text: String,
    pub label_baseline: Vec2,
    pub clock_baseline: Vec2,
    pub announcement_baseline: Vec2,
    pub text_rgb: [u8; 3],
    pub digit_rgb: [u8; 3],
    pub start_rgb: [u8; 3],
    pub announcement_after_release_fields: u16,
    pub full_banner_until_fields: u16,
    pub fade_fields: u16,
    pub digit_cell_px: f32,
    pub punctuation_cell_px: f32,
    pub decimal_inset_px: f32,
    pub number_origin_inset_px: f32,
    pub fade_peak: u8,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Reflect)]
#[reflect(Resource)]
pub struct HudDisplay {
    pub clock: String,
    pub announcement: String,
    pub announcement_rgb: [u8; 3],
    pub additive: bool,
}

pub fn display(session: &DrivingSession, settings: &HudSettings) -> HudDisplay {
    // 8002CCF8..CD68 adds the low-four-bit clock phase, converted
    // by the signed reciprocal at CD14..60: floor(phase * 1000 / 900).
    // In a fresh B-1 session 8002F864 starts at zero and increments once
    // after each active clock tick (8002E5F0..60C).
    let correction = u64::from(session.elapsed_ticks & 15) * 1000 / 900;
    let ms = session.elapsed_millis() + correction;
    let clock = format!("{}:{:02}.{:03}", ms / 60000, ms / 1000 % 60, ms % 1000);
    // The independent announcement timer continues after the start hold.
    // 8002A19C consumes it after its two-field decrement at 8002A0D4.
    // The release tick is already clock tick one when the announcement
    // counter first reaches its post-release duration.
    let fields = match session.phase {
        DrivingPhase::Countdown { fields_left } => {
            i64::from(fields_left) + i64::from(settings.announcement_after_release_fields)
        }
        DrivingPhase::Driving => {
            i64::from(settings.announcement_after_release_fields) + i64::from(crate::FIELDS_PER_TICK)
                - i64::from(session.elapsed_ticks) * i64::from(crate::FIELDS_PER_TICK)
        }
    };
    let mut out = HudDisplay { clock, ..default() };
    if fields > i64::from(settings.announcement_after_release_fields) {
        // 8002A234..288: integer countdown digit, centered on its baseline.
        out.announcement = ((fields - i64::from(settings.announcement_after_release_fields)) / 60 + 1).to_string();
        out.announcement_rgb = settings.digit_rgb;
    } else if fields > i64::from(settings.full_banner_until_fields) {
        out.announcement.clone_from(&settings.start_text);
        out.announcement_rgb = settings.start_rgb;
    } else {
        let age = i64::from(settings.full_banner_until_fields) - fields;
        if age < i64::from(settings.fade_fields) {
            // 8002A2A4..35C: brighten to grey during the first half,
            // then darken; both halves use additive sprites, not alpha.
            let half = i64::from(settings.fade_fields) / 2;
            out.announcement.clone_from(&settings.start_text);
            out.additive = true;
            out.announcement_rgb = std::array::from_fn(|i| {
                if age < half {
                    (i64::from(settings.start_rgb[i])
                        + (i64::from(settings.fade_peak) - i64::from(settings.start_rgb[i])) * age / half)
                        as u8
                } else {
                    (i64::from(settings.fade_peak) * (i64::from(settings.fade_fields) - age) / half) as u8
                }
            });
        }
    }
    out
}

pub struct HudRulesPlugin;
impl Plugin for HudRulesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudDisplay>().add_systems(
            FixedUpdate,
            update_display.after(SessionSystems::Clock).in_set(EngineSet::Objects).in_set(GameSystems::Game),
        );
    }
}
fn update_display(
    session: Option<Res<DrivingSession>>,
    settings: Option<Res<HudSettings>>,
    mut current: ResMut<HudDisplay>,
) {
    if let (Some(session), Some(settings)) = (session, settings) {
        let next = display(&session, &settings);
        if *current != next {
            *current = next;
        }
    }
}

/// Presentation is optional: a mod can disable this plugin while retaining
/// the session/vehicle rules, or supply its own layout/font files.
pub struct HudPlugin;
impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<HudSettings>() && engine::config::asset_path(HUD_SETTINGS).is_file() {
            app.add_plugins(TuningPlugin::<HudSettings>::new(HUD_SETTINGS));
        }
        app.add_plugins((HudRulesPlugin, super::gauges::GaugePlugin, super::records::RecordsPlugin))
            .add_systems(Startup, (spawn_hud_camera, spawn_hud))
            .add_systems(Update, render_hud);
    }
}

#[derive(Resource)]
struct HudFonts {
    small: BitmapFont,
    large: BitmapFont,
    small_image: Handle<Image>,
    large_image: Handle<Image>,
    opaque: Handle<PsxSprite>,
    additive: Handle<PsxSprite>,
}
#[derive(Component)]
struct HudGlyph;
fn spawn_hud_camera(
    mut commands: Commands,
    settings: Option<Res<HudSettings>>,
    gauges: Option<Res<super::gauges::GaugeSettings>>,
    records: Option<Res<super::records::RecordsSettings>>,
) {
    if settings.is_none() && gauges.is_none() && records.is_none() {
        return;
    }
    commands.spawn((
        Name::new("Licence HUD camera"),
        Camera2d,
        Camera { order: 1, clear_color: ClearColorConfig::None, ..default() },
        PsxView,
    ));
}
fn spawn_hud(
    mut commands: Commands,
    settings: Option<Res<HudSettings>>,
    server: Option<Res<AssetServer>>,
    materials: Option<ResMut<Assets<PsxSprite>>>,
) {
    let (Some(settings), Some(server), Some(mut materials)) = (settings, server, materials) else {
        return;
    };
    let small: BitmapFont = load_ron(&settings.small_font).expect("small HUD font");
    let large: BitmapFont = load_ron(&settings.large_font).expect("large HUD font");
    commands.insert_resource(HudFonts {
        small_image: server.load(small.image.clone()),
        large_image: server.load(large.image.clone()),
        small,
        large,
        opaque: materials.add(PsxSprite::default()),
        additive: materials.add(PsxSprite::blend(PsxBlend::Add)),
    });
}

fn number_layout(font: &BitmapFont, text: &str, settings: &HudSettings) -> Vec<PositionedGlyph> {
    // Shared source routine 8006B3F4, also used by licence records.
    font.layout_number(
        text,
        engine::bitmap_font::NumberCells {
            digit_px: settings.digit_cell_px,
            punctuation_px: settings.punctuation_cell_px,
            decimal_inset_px: settings.decimal_inset_px,
            origin_inset_px: settings.number_origin_inset_px,
        },
    )
    .expect("clock glyphs")
}

/// Ordinary screen-pixel glyph placement, shared with the original-draw checks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HudGlyphDraw {
    pub character: char,
    pub large: bool,
    pub area: [f32; 4],
    pub rgb: [u8; 3],
    pub additive: bool,
}

pub fn layout_display(
    small: &BitmapFont,
    large: &BitmapFont,
    settings: &HudSettings,
    view: &HudDisplay,
) -> Vec<HudGlyphDraw> {
    let label = small.layout(&settings.label).expect("label glyphs").0;
    let clock = number_layout(small, &view.clock, settings);
    let (mut announcement, width) = large.layout(&view.announcement).expect("announcement glyphs");
    for g in &mut announcement {
        g.top_left_px[0] -= (width / 2.0).floor();
    }
    let mut out = Vec::new();
    for (glyphs, origin, rgb, is_large, additive) in [
        (label, settings.label_baseline, settings.text_rgb, false, false),
        (clock, settings.clock_baseline, settings.text_rgb, false, false),
        (announcement, settings.announcement_baseline, view.announcement_rgb, true, view.additive),
    ] {
        for g in glyphs {
            out.push(HudGlyphDraw {
                character: g.character,
                large: is_large,
                area: [origin.x + g.top_left_px[0], origin.y + g.top_left_px[1], g.rect[2] as f32, g.rect[3] as f32],
                rgb,
                additive,
            });
        }
    }
    out
}

fn render_hud(
    mut commands: Commands,
    fonts: Option<Res<HudFonts>>,
    settings: Option<Res<HudSettings>>,
    view: Res<HudDisplay>,
    glyphs: Query<Entity, With<HudGlyph>>,
) {
    let (Some(fonts), Some(settings)) = (fonts, settings) else { return };
    // The original opening field uploads textures but has no HUD packet.
    // Wait for the first simulation update instead of previewing a spurious
    // boundary digit before the countdown has ever been decremented.
    if !view.is_changed() || view.clock.is_empty() {
        return;
    }
    for e in &glyphs {
        commands.entity(e).despawn();
    }
    for draw in layout_display(&fonts.small, &fonts.large, &settings, &view) {
        let font = if draw.large { &fonts.large } else { &fonts.small };
        let image = if draw.large { &fonts.large_image } else { &fonts.small_image };
        let material = if draw.additive { &fonts.additive } else { &fonts.opaque };
        let anchor = Vec2::new(if draw.large { 0.0 } else { -1.0 }, 1.0);
        let rgb = draw.rgb;
        let g = font.glyphs.get(&draw.character).expect("laid-out glyph");
        let [x, y, w, h] = g.rect;
        let size = Vec2::new(w as f32, h as f32);
        let offset = Vec2::new(draw.area[0], draw.area[1]) + size / 2.0;
        let color = Color::srgb(rgb[0] as f32 / 128.0, rgb[1] as f32 / 128.0, rgb[2] as f32 / 128.0);
        commands.spawn((
            HudGlyph,
            Sprite {
                image: image.clone(),
                rect: Some(Rect::from_corners(
                    Vec2::new(x as f32, y as f32),
                    Vec2::new((x + w) as f32, (y + h) as f32),
                )),
                color,
                ..default()
            },
            SpriteMaterial(material.clone()),
            Transform::from_xyz(0.0, 0.0, 10.0),
            PsxAnchor::new(
                anchor,
                Vec2::new(offset.x - if anchor.x == 0.0 { crate::SCREEN.x as f32 / 2.0 } else { 0.0 }, -offset.y),
            ),
        ));
    }
}
