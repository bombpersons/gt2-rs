//! Licence Finish/FAIL and prize overlay (8002AB60 / 8002B170).
//! All fonts, labels, logos and animation tuning are ordinary layered assets.
use super::{
    licence::{LicenceDefinition, format_record_time},
    licence_awards::{LicenceAchievement, LicenceGrade, LicenceProgress},
    licence_results::{LicenceResult, LicenceResultTiming},
};
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use engine::{
    bitmap_font::{BitmapFont, NumberCells},
    prelude::*,
    psx::{PsxAnchor, PsxBlend, PsxMaterial2d, PsxSprite},
};
use serde::{Deserialize, Serialize};

pub const RESULT_PRESENTATION: &str = "race/result-presentation.ron";
#[derive(Clone, Debug, Reflect, Serialize, Deserialize)]
pub struct PrizeStyle {
    pub text: String,
    pub rgb: [u8; 3],
    pub logo: Option<usize>,
}
#[derive(Clone, Debug, Reflect, Serialize, Deserialize)]
pub struct PrizeLogo {
    pub image: String,
    pub size_px: Vec2,
}
#[derive(Resource, Clone, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct ResultPresentationSettings {
    pub large_font: String,
    pub time_font: String,
    pub finish_text: String,
    pub failure_text: String,
    pub finish_baseline: Vec2,
    pub failure_baseline: Vec2,
    pub failure_rgb: [u8; 3],
    pub finish_rgb: [u8; 3],
    pub finish_peak: u8,
    pub finish_fade_after_ticks: u16,
    pub finish_fade_ticks: u16,
    pub finish_echo_px: f32,
    pub finish_until_ticks: u16,
    pub shade_after_ticks: u16,
    pub shade_ticks: u16,
    pub shade_peak: u8,
    pub prize_spacing_px: f32,
    pub prize_fade_ticks: u16,
    pub prize_initial_rgb: [u8; 3],
    pub prize_baseline: Vec2,
    pub time_baseline: Vec2,
    pub time_cells: NumberCells,
    pub logo_center_px: Vec2,
    pub logo_curve: Vec<u8>,
    pub logo_initial_rgb: [u8; 3],
    pub logo_final_rgb: [u8; 3],
    pub logo_settled_rgb: [u8; 3],
    /// Unqualified, assisted pass, bronze, silver, gold.
    pub prizes: [PrizeStyle; 5],
    pub logos: Vec<PrizeLogo>,
}

/// The prize drawing has a narrower assisted-pass gate than saved awards:
/// 8002B30C..328 requires an unpassed test and exactly the qualifying attempt.
pub fn prize_style(definition: &LicenceDefinition, achievement: LicenceAchievement, time_ms: u32) -> usize {
    match definition.grade_at_time(time_ms) {
        LicenceGrade::Gold => 4,
        LicenceGrade::Silver => 3,
        LicenceGrade::Bronze => 2,
        LicenceGrade::AssistedPass
            if achievement.grade == LicenceGrade::Unpassed
                && definition
                    .assisted_pass_attempts
                    .is_some_and(|n| u16::from(achievement.assisted_attempts) + 1 == u16::from(n)) =>
        {
            1
        }
        _ => 0,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResultGlyphDraw {
    pub character: char,
    pub time_font: bool,
    pub area: [f32; 4],
    pub rgb: [u8; 3],
    pub additive: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResultLogoDraw {
    pub logo: usize,
    pub vertices: [[f32; 2]; 4],
    pub rgb: [u8; 3],
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ResultLayout {
    pub glyphs: Vec<ResultGlyphDraw>,
    pub logo: Option<ResultLogoDraw>,
    pub shade: Option<u8>,
}

// 8006B548: interpolate each encoded channel, truncating the signed delta
// towards zero. This changes visible fade colours by a channel at boundaries.
fn fade(a: [u8; 3], b: [u8; 3], age: u16, duration: u16) -> [u8; 3] {
    let duration = i32::from(duration.max(1));
    let age = i32::from(age).min(duration);
    std::array::from_fn(|i| (i32::from(a[i]) + (i32::from(b[i]) - i32::from(a[i])) * age / duration) as u8)
}
fn text(
    out: &mut ResultLayout,
    font: &BitmapFont,
    label: &str,
    at: Vec2,
    rgb: [u8; 3],
    additive: bool,
    spacing_px: f32,
) {
    let (glyphs, width) = font.layout_with_spacing(label, spacing_px).expect("result glyphs");
    for g in glyphs {
        out.glyphs.push(ResultGlyphDraw {
            character: g.character,
            time_font: false,
            area: [
                at.x - (width / 2.0).floor() + g.top_left_px[0],
                at.y + g.top_left_px[1],
                g.rect[2] as f32,
                g.rect[3] as f32,
            ],
            rgb,
            additive,
        });
    }
}

pub fn layout_result(
    large: &BitmapFont,
    time: &BitmapFont,
    s: &ResultPresentationSettings,
    timing: &LicenceResultTiming,
    result: &LicenceResult,
    definition: Option<&LicenceDefinition>,
    achievement: LicenceAchievement,
) -> ResultLayout {
    let mut out = ResultLayout::default();
    let (age, time_ms) = match result {
        LicenceResult::Success { age_ticks, time_ms } => (*age_ticks, *time_ms),
        LicenceResult::Failure { .. } => {
            text(&mut out, large, &s.failure_text, s.failure_baseline, s.failure_rgb, false, 0.0);
            return out;
        }
        _ => return out,
    };
    if age < s.finish_until_ticks {
        let start = s.finish_fade_after_ticks;
        let duration = s.finish_fade_ticks.max(1);
        if age < start {
            text(&mut out, large, &s.finish_text, s.finish_baseline, s.finish_rgb, false, 0.0);
        } else if age < start.saturating_add(duration) {
            let rgb = fade(s.finish_rgb, [s.finish_peak; 3], age - start, duration);
            text(&mut out, large, &s.finish_text, s.finish_baseline, rgb, true, 0.0);
        } else {
            // 8002AC08..CA4: two separating echoes and the centered banner.
            let phase = (age - start - duration).min(duration);
            let rgb = fade([s.finish_peak; 3], [0; 3], phase, duration);
            let offset = (s.finish_echo_px * f32::from(phase) / f32::from(duration)).floor();
            for x in [-offset, offset, 0.0] {
                text(&mut out, large, &s.finish_text, s.finish_baseline + Vec2::X * x, rgb, true, 0.0);
            }
        }
    }
    if age > s.shade_after_ticks {
        out.shade = Some(
            (u32::from(s.shade_peak) * u32::from((age - s.shade_after_ticks).min(s.shade_ticks))
                / u32::from(s.shade_ticks.max(1))) as u8,
        );
    }
    if age <= timing.prize_after_ticks {
        return out;
    }
    let Some(definition) = definition else { return out };
    let style = &s.prizes[prize_style(definition, achievement, time_ms)];
    let phase = (age - timing.prize_after_ticks).min(s.prize_fade_ticks);
    let rgb = fade(s.prize_initial_rgb, style.rgb, phase, s.prize_fade_ticks);
    text(&mut out, large, &style.text, s.prize_baseline, rgb, false, s.prize_spacing_px);
    // Shared numeric layout 8006B49C, centered instead of the HUD's right edge.
    for g in
        time.layout_number_centered(&format_record_time(Some(time_ms), ""), s.time_cells).expect("result time glyphs")
    {
        out.glyphs.push(ResultGlyphDraw {
            character: g.character,
            time_font: true,
            area: [
                s.time_baseline.x + g.top_left_px[0],
                s.time_baseline.y + g.top_left_px[1],
                g.rect[2] as f32,
                g.rect[3] as f32,
            ],
            rgb,
            additive: false,
        });
    }
    if let Some(logo) = style.logo {
        let size = s.logos[logo].size_px;
        let steps = s.logo_curve.len().saturating_sub(1);
        if steps != 0 {
            // 8002B488..588: the lookup curve rotates and shears the logo
            // into its final rectangle. Floor positions are visible pixels.
            let step = (usize::from(phase) * steps / usize::from(s.prize_fade_ticks.max(1))).min(steps);
            let half = (size / 2.0).floor();
            let dx = (f32::from(s.logo_curve[step]) * half.y / 256.0).floor();
            let dy = (f32::from(s.logo_curve[steps - step]) * half.y / 128.0).floor();
            let c = s.logo_center_px;
            out.logo = Some(ResultLogoDraw {
                logo,
                vertices: [
                    [c.x - half.x + dx, c.y - dy],
                    [c.x + half.x - dx, c.y - dy],
                    [c.x - half.x - dx, c.y + dy],
                    [c.x + half.x + dx, c.y + dy],
                ],
                rgb: if step == steps {
                    s.logo_settled_rgb
                } else {
                    fade(s.logo_initial_rgb, s.logo_final_rgb, step as u16, steps as u16)
                },
            });
        }
    }
    out
}

pub struct ResultPresentationPlugin;
impl Plugin for ResultPresentationPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<ResultPresentationSettings>()
            && engine::config::asset_path(RESULT_PRESENTATION).is_file()
        {
            app.add_plugins(TuningPlugin::<ResultPresentationSettings>::new(RESULT_PRESENTATION));
        }
        app.register_type::<ResultPresentationSettings>()
            .register_type::<ResultGlyph>()
            .add_systems(Startup, load_assets)
            .add_systems(Update, render);
    }
}
#[derive(Resource)]
struct ResultAssets {
    large: BitmapFont,
    time: BitmapFont,
    large_image: Handle<Image>,
    time_image: Handle<Image>,
    opaque: Handle<PsxSprite>,
    additive: Handle<PsxSprite>,
    shade: Handle<PsxMaterial2d>,
    logos: Vec<Handle<PsxMaterial2d>>,
}
#[derive(Component)]
struct ResultVisual;
#[derive(Component, Reflect)]
#[reflect(Component)]
pub struct ResultGlyph;
fn load_assets(
    mut commands: Commands,
    s: Option<Res<ResultPresentationSettings>>,
    server: Option<Res<AssetServer>>,
    sprites: Option<ResMut<Assets<PsxSprite>>>,
    materials: Option<ResMut<Assets<PsxMaterial2d>>>,
) {
    let (Some(s), Some(server), Some(mut sprites), Some(mut materials)) = (s, server, sprites, materials) else {
        return;
    };
    let large: BitmapFont = load_ron(&s.large_font).expect("result large font");
    let time: BitmapFont = load_ron(&s.time_font).expect("result time font");
    let logos = s
        .logos
        .iter()
        .map(|logo| {
            materials.add(PsxMaterial2d { texture: Some(server.load(logo.image.clone())), dither: false, ..default() })
        })
        .collect();
    commands.insert_resource(ResultAssets {
        large_image: server.load(large.image.clone()),
        time_image: server.load(time.image.clone()),
        large,
        time,
        opaque: sprites.add(PsxSprite::default()),
        additive: sprites.add(PsxSprite::blend(PsxBlend::Add)),
        shade: materials.add(PsxMaterial2d { blend: PsxBlend::Subtract, dither: false, ..default() }),
        logos,
    });
}
fn quad(points: [[f32; 2]; 4], rgb: [u8; 3], modulation: f32) -> Mesh {
    let color = Color::srgb(rgb[0] as f32 / modulation, rgb[1] as f32 / modulation, rgb[2] as f32 / modulation)
        .to_linear()
        .to_f32_array();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, points.map(|p| [p[0], -p[1], 0.0]).to_vec())
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![color; 4])
        .with_inserted_indices(Indices::U32(vec![0, 2, 1, 1, 2, 3]))
}
#[allow(clippy::too_many_arguments)]
fn render(
    mut commands: Commands,
    s: Option<Res<ResultPresentationSettings>>,
    a: Option<Res<ResultAssets>>,
    timing: Option<Res<LicenceResultTiming>>,
    result: Option<Res<LicenceResult>>,
    definition: Option<Res<LicenceDefinition>>,
    progress: Option<Res<LicenceProgress>>,
    visuals: Query<Entity, With<ResultVisual>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    screen: Res<engine::psx::PsxScreen>,
) {
    let (Some(s), Some(a), Some(timing), Some(result), Some(mut meshes)) = (s, a, timing, result, meshes) else {
        return;
    };
    if !result.is_changed()
        && !s.is_changed()
        && !screen.is_changed()
        && !definition.as_ref().is_some_and(|r| r.is_changed())
        && !progress.as_ref().is_some_and(|r| r.is_changed())
    {
        return;
    }
    for entity in &visuals {
        commands.entity(entity).despawn();
    }
    let achievement =
        definition.as_ref().and_then(|d| progress.as_ref().map(|p| p.achievement(&d.id))).unwrap_or_default();
    let layout = layout_result(&a.large, &a.time, &s, &timing, &result, definition.as_deref(), achievement);
    if let Some(shade) = layout.shade {
        let half = Vec2::new(screen.width as f32, screen.height as f32) / 2.0;
        let points = [[-half.x, -half.y], [half.x, -half.y], [-half.x, half.y], [half.x, half.y]];
        commands.spawn((
            ResultVisual,
            Mesh2d(meshes.add(quad(points, [shade; 3], 255.0))),
            MeshMaterial2d(a.shade.clone()),
            Transform::from_xyz(0.0, 0.0, 20.0),
        ));
    }
    let anchor = Vec2::new(0.0, 1.0);
    let offset = Vec2::new(-(crate::SCREEN.x as f32) / 2.0, 0.0);
    if let Some(logo) = layout.logo {
        let size = s.logos[logo.logo].size_px;
        let uv = (size - Vec2::ONE) / size;
        let mesh = quad(logo.vertices, logo.rgb, 128.0)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0], [uv.x, 0.0], [0.0, uv.y], [uv.x, uv.y]]);
        commands.spawn((
            ResultVisual,
            Mesh2d(meshes.add(mesh)),
            MeshMaterial2d(a.logos[logo.logo].clone()),
            Transform::from_xyz(0.0, 0.0, 22.0),
            PsxAnchor::new(anchor, offset),
        ));
    }
    for (layer, draw) in layout.glyphs.into_iter().enumerate() {
        let font = if draw.time_font { &a.time } else { &a.large };
        let image = if draw.time_font { &a.time_image } else { &a.large_image };
        let [x, y, w, h] = font.glyphs[&draw.character].rect;
        let rgb = draw.rgb;
        commands.spawn((
            ResultVisual,
            ResultGlyph,
            Sprite {
                image: image.clone(),
                rect: Some(Rect::new(x as f32, y as f32, (x + w) as f32, (y + h) as f32)),
                color: Color::srgb(rgb[0] as f32 / 128.0, rgb[1] as f32 / 128.0, rgb[2] as f32 / 128.0),
                ..default()
            },
            SpriteMaterial(if draw.additive { a.additive.clone() } else { a.opaque.clone() }),
            Transform::from_xyz(0.0, 0.0, 23.0 + layer as f32 * 0.001),
            PsxAnchor::new(anchor, offset + Vec2::new(draw.area[0] + w as f32 / 2.0, -draw.area[1] - h as f32 / 2.0)),
        ));
    }
}
