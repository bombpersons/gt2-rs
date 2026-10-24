//! B-1 records/medal-target panel, using native definitions and bitmap fonts.
use super::{
    hud::HudGlyphDraw,
    licence::{LicenceDefinition, LicenceRecords, format_record_time},
    session::SessionSystems,
};
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use engine::{
    bitmap_font::{BitmapFont, NumberCells, PositionedGlyph},
    prelude::*,
    psx::{PsxAnchor, PsxMaterial2d, PsxSprite},
};
use serde::{Deserialize, Serialize};

pub const RECORDS_SETTINGS: &str = "race/records-panel.ron";
#[derive(Resource, Clone, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct RecordsSettings {
    pub font: String,
    pub label: String,
    pub missing_time: String,
    pub units: String,
    pub origin_px: Vec2,
    pub label_offset_px: Vec2,
    pub time_offset_px: Vec2,
    pub speed_offset_px: Vec2,
    pub target_offsets_px: [Vec2; 3],
    pub text_rgb: [u8; 3],
    pub underline_offset_px: Vec2,
    pub underline_size_extra_px: Vec2,
    pub underline_rgb: [[u8; 3]; 2],
    pub marker_rgb: [[u8; 3]; 3],
    pub marker_offset_px: Vec2,
    pub marker_inner_px: f32,
    pub marker_border_px: f32,
    pub cells: NumberCells,
    pub metadata_digit_px: f32,
    pub metadata_digit_inset_px: f32,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Reflect)]
#[reflect(Resource)]
pub struct RecordsDisplay {
    pub time: String,
    pub speed: String,
    pub targets: [String; 3],
    pub visible: bool,
}
pub fn display(s: &RecordsSettings, definition: &LicenceDefinition, records: &LicenceRecords) -> RecordsDisplay {
    let record = records.best.get(&definition.id);
    RecordsDisplay {
        time: format_record_time(record.map(|r| r.time_ms), &s.missing_time),
        speed: record
            .map_or_else(String::new, |r| format!("{}.{}{}", r.speed_readout / 100, r.speed_readout % 100, s.units)),
        targets: std::array::from_fn(|i| format_record_time(Some(definition.time_limits_ms[i]), &s.missing_time)),
        visible: true,
    }
}

pub struct RecordsRulesPlugin;
impl Plugin for RecordsRulesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RecordsDisplay>().register_type::<RecordsDisplay>().add_systems(
            FixedUpdate,
            update_display.after(SessionSystems::Clock).in_set(EngineSet::Objects).in_set(GameSystems::Game),
        );
    }
}
fn update_display(
    settings: Option<Res<RecordsSettings>>,
    definition: Option<Res<LicenceDefinition>>,
    records: Option<Res<LicenceRecords>>,
    mut current: ResMut<RecordsDisplay>,
) {
    let (Some(s), Some(definition), Some(records)) = (settings, definition, records) else {
        return;
    };
    let next = display(&s, &definition, &records);
    if *current != next {
        *current = next;
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecordQuad {
    pub area: [f32; 4],
    pub rgb: [[u8; 3]; 2],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecordsLayout {
    pub glyphs: Vec<HudGlyphDraw>,
    pub quads: Vec<RecordQuad>,
}

fn speed_layout(font: &BitmapFont, text: &str, s: &RecordsSettings) -> Vec<PositionedGlyph> {
    let chars: Vec<_> = text.chars().collect();
    let steps: Vec<_> = chars
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            if c.is_ascii_digit() {
                s.metadata_digit_px
            } else {
                let adjustment = chars
                    .get(i + 1)
                    .and_then(|&n| font.kerning.iter().find(|k| k.pair == (c, n)))
                    .map_or(0.0, |k| k.adjustment_px);
                font.glyphs[&c].advance_px + adjustment
            }
        })
        .collect();
    let mut x = -steps.iter().sum::<f32>();
    chars
        .into_iter()
        .zip(steps)
        .map(|(character, step)| {
            let g = font.glyphs[&character];
            let placed = PositionedGlyph {
                character,
                rect: g.rect,
                top_left_px: [
                    x + g.bearing_px[0]
                        + if character.is_ascii_digit() { s.metadata_digit_inset_px + g.cell_inset_px } else { 0.0 },
                    g.bearing_px[1],
                ],
            };
            x += step;
            placed
        })
        .collect()
}

pub fn layout_records(font: &BitmapFont, s: &RecordsSettings, view: &RecordsDisplay) -> RecordsLayout {
    let mut out = RecordsLayout { glyphs: Vec::new(), quads: Vec::new() };
    if !view.visible {
        return out;
    }
    let (mut label, width) = font.layout(&s.label).expect("records label glyphs");
    for g in &mut label {
        g.top_left_px[0] -= width;
    }
    let underline = s.origin_px + s.label_offset_px - Vec2::X * width + s.underline_offset_px;
    out.quads.push(RecordQuad {
        area: [underline.x, underline.y, width + s.underline_size_extra_px.x, s.underline_size_extra_px.y],
        rgb: s.underline_rgb,
    });
    let mut add = |glyphs: Vec<PositionedGlyph>, offset: Vec2| {
        let origin = s.origin_px + offset;
        for g in glyphs {
            out.glyphs.push(HudGlyphDraw {
                character: g.character,
                large: false,
                area: [origin.x + g.top_left_px[0], origin.y + g.top_left_px[1], g.rect[2] as f32, g.rect[3] as f32],
                rgb: s.text_rgb,
                additive: false,
            });
        }
    };
    add(label, s.label_offset_px);
    for i in (0..3).rev() {
        let at = s.origin_px + s.target_offsets_px[i] + s.marker_offset_px;
        let border = s.marker_border_px;
        out.quads.push(RecordQuad {
            area: [at.x - border, at.y - border, s.marker_inner_px + border * 2.0, s.marker_inner_px + border * 2.0],
            rgb: [[0; 3]; 2],
        });
        out.quads
            .push(RecordQuad { area: [at.x, at.y, s.marker_inner_px, s.marker_inner_px], rgb: [s.marker_rgb[i]; 2] });
        add(
            font.layout_number(&view.targets[i], s.cells).expect("medal target glyphs"),
            s.target_offsets_px[i] + s.time_offset_px * Vec2::X,
        );
    }
    if !view.speed.is_empty() {
        add(speed_layout(font, &view.speed, s), s.speed_offset_px);
    }
    add(font.layout_number(&view.time, s.cells).expect("record time glyphs"), s.time_offset_px);
    out
}

pub struct RecordsPlugin;
impl Plugin for RecordsPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<RecordsSettings>() && engine::config::asset_path(RECORDS_SETTINGS).is_file()
        {
            app.add_plugins(TuningPlugin::<RecordsSettings>::new(RECORDS_SETTINGS));
        }
        app.add_plugins(RecordsRulesPlugin).add_systems(Startup, load_records).add_systems(Update, render_records);
    }
}
#[derive(Resource)]
struct RecordsAssets {
    font: BitmapFont,
    image: Handle<Image>,
    sprite: Handle<PsxSprite>,
    polygon: Handle<PsxMaterial2d>,
}
#[derive(Component)]
struct RecordVisual;
fn load_records(
    mut commands: Commands,
    s: Option<Res<RecordsSettings>>,
    server: Option<Res<AssetServer>>,
    sprites: Option<ResMut<Assets<PsxSprite>>>,
    polygons: Option<ResMut<Assets<PsxMaterial2d>>>,
) {
    let (Some(s), Some(server), Some(mut sprites), Some(mut polygons)) = (s, server, sprites, polygons) else {
        return;
    };
    let font: BitmapFont = load_ron(&s.font).expect("record font");
    commands.insert_resource(RecordsAssets {
        image: server.load(font.image.clone()),
        font,
        sprite: sprites.add(PsxSprite::default()),
        polygon: polygons.add(PsxMaterial2d { dither: false, ..default() }),
    });
}
fn render_records(
    mut commands: Commands,
    s: Option<Res<RecordsSettings>>,
    assets: Option<Res<RecordsAssets>>,
    view: Res<RecordsDisplay>,
    visuals: Query<Entity, With<RecordVisual>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
) {
    let (Some(s), Some(a), Some(mut meshes)) = (s, assets, meshes) else {
        return;
    };
    if !view.is_changed() && !s.is_changed() {
        return;
    }
    for e in &visuals {
        commands.entity(e).despawn();
    }
    let layout = layout_records(&a.font, &s, &view);
    let anchor = Vec2::new(1.0, 1.0);
    let offset = Vec2::new(-(crate::SCREEN.x as f32), 0.0);
    for (layer, q) in layout.quads.into_iter().enumerate() {
        let [x, y, w, h] = q.area;
        let positions = vec![[x, -y, 0.0], [x + w, -y, 0.0], [x, -y - h, 0.0], [x + w, -y - h, 0.0]];
        let colors: Vec<_> = [q.rgb[0], q.rgb[1], q.rgb[0], q.rgb[1]]
            .map(|c| {
                Color::srgb(c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0).to_linear().to_f32_array()
            })
            .into();
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
            .with_inserted_indices(Indices::U32(vec![0, 1, 2, 2, 1, 3]));
        // Nested marker rectangles need increasing depth; the underline stays
        // below its transparent glyph pixels too.
        let depth = 10.0 + layer as f32 * 0.01;
        commands.spawn((
            RecordVisual,
            Mesh2d(meshes.add(mesh)),
            MeshMaterial2d(a.polygon.clone()),
            Transform::from_xyz(0.0, 0.0, depth),
            PsxAnchor::new(anchor, offset),
        ));
    }
    for draw in layout.glyphs {
        let g = a.font.glyphs[&draw.character];
        let [x, y, w, h] = g.rect;
        let at = Vec2::new(draw.area[0] + w as f32 / 2.0, -draw.area[1] - h as f32 / 2.0);
        let rgb = draw.rgb;
        commands.spawn((
            RecordVisual,
            Sprite {
                image: a.image.clone(),
                rect: Some(Rect::new(x as f32, y as f32, (x + w) as f32, (y + h) as f32)),
                color: Color::srgb(rgb[0] as f32 / 128.0, rgb[1] as f32 / 128.0, rgb[2] as f32 / 128.0),
                ..default()
            },
            SpriteMaterial(a.sprite.clone()),
            Transform::from_xyz(0.0, 0.0, 11.0),
            PsxAnchor::new(anchor, offset + at),
        ));
    }
}
