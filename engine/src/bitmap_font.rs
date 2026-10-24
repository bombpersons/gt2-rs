//! Authorable bitmap-font metrics. Coordinates are pixels, with y down from
//! a text baseline; renderers can use ordinary sprites and texture rectangles.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BitmapFont {
    pub image: String,
    pub glyphs: BTreeMap<char, BitmapGlyph>,
    #[serde(default)]
    pub kerning: Vec<Kerning>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BitmapGlyph {
    pub rect: [u32; 4],
    pub bearing_px: [f32; 2],
    pub advance_px: f32,
    /// Additional inset when placing the glyph in a fixed-width number cell.
    #[serde(default)]
    pub cell_inset_px: f32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Kerning {
    pub pair: (char, char),
    pub adjustment_px: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct PositionedGlyph {
    pub character: char,
    pub top_left_px: [f32; 2],
    pub rect: [u32; 4],
}

impl BitmapFont {
    /// Missing characters are errors so content authors can supply the glyph.
    pub fn layout(&self, text: &str) -> Result<(Vec<PositionedGlyph>, f32), char> {
        let chars: Vec<_> = text.chars().collect();
        let mut x = 0.0;
        let mut placed = Vec::new();
        for (index, &character) in chars.iter().enumerate() {
            let glyph = self.glyphs.get(&character).ok_or(character)?;
            if glyph.rect[2] != 0 && glyph.rect[3] != 0 {
                placed.push(PositionedGlyph {
                    character,
                    top_left_px: [x + glyph.bearing_px[0], glyph.bearing_px[1]],
                    rect: glyph.rect,
                });
            }
            x += glyph.advance_px;
            if let Some(&next) = chars.get(index + 1)
                && let Some(pair) = self.kerning.iter().find(|k| k.pair == (character, next))
            {
                x += pair.adjustment_px;
            }
        }
        Ok((placed, x))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_bearings_spaces_and_pair_spacing_survive_ron() {
        let font: BitmapFont = ron::from_str(
            r#"(
            image: "handmade.png",
            glyphs: {
                'A': (rect: (0,0,5,7), bearing_px: (-1.0,-7.0), advance_px: 6.0),
                'V': (rect: (5,0,5,7), bearing_px: (0.0,-7.0), advance_px: 6.0),
                ' ': (rect: (0,0,0,0), bearing_px: (0.0,0.0), advance_px: 3.0),
            },
            kerning: [(pair: ('A','V'), adjustment_px: -2.0)],
        )"#,
        )
        .unwrap();
        let (glyphs, width) = font.layout("AV A").unwrap();
        assert_eq!(width, 19.0);
        assert_eq!(glyphs.iter().map(|g| g.top_left_px[0]).collect::<Vec<_>>(), [-1.0, 4.0, 12.0]);
        assert_eq!(font.layout("?").unwrap_err(), '?');
    }
}
