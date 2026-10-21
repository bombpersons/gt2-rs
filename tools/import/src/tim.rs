//! PS1 `TIM` textures -> RGBA8.
//!
//! `TIM` is the original's texture container: a `u32` magic `0x00000010`, a
//! `u32` flags word, then an optional CLUT block and an image block. Each block
//! is `u32 size` followed by `u16 x, y, w, h` and `size - 12` bytes of data.
//! Flags bits 0..2 are the pixel mode (0 = 4bpp, 1 = 8bpp, 2 = 16bpp,
//! 3 = 24bpp) and bit 3 says a CLUT block is present.
//!
//! The header's `w` counts **16-bit VRAM words**, so the pixel width is
//! `w * 16 / bits-per-pixel`: four times `w` at 4bpp, twice at 8bpp, once at
//! 16bpp. Verified on the disc, where this makes the declared block size exact
//! every time. 4bpp: `arc_font.tim` `w = 128, h = 256` = 512 x 256 = 65536
//! texel bytes, `carwheel/bb001--s.tim` `12 x 48` = 48 x 48 = 1152,
//! `crsmap/2p_autumn.tim` `24 x 96` = 96 x 96 = 4608. 8bpp: `l-sel.tim`
//! `176 x 480` = 352 x 480 = 168960, `topmenu_panels.tim` `320 x 256` =
//! 640 x 256. Reading `w` as pixels leaves three quarters of a 4bpp block (or
//! half of an 8bpp block) unaccounted for, and renders the image at half width.
//!
//! 39 of the disc's 3707 TIMs — every `/arcade` screen panel, `arc_panels.tim`,
//! `topmenu_panels.tim`, `gtmode_font.tim` and friends — are indexed images
//! with **no CLUT block**: the game uploads their palette separately, so the
//! file holds indices only. They decode structurally (the sizes above are
//! exact) and `to_rgba` shows the indices as a grey ramp; `has_palette` says
//! which kind of image this is.
//!
//! Colour is the PS1 5-bit-per-channel form, bit 15 the semi-transparency flag.
//! The PNG keeps the distinction the flag carries: opaque pixels are alpha 255,
//! semi-transparent ones alpha 128. Colour and blend mode are separate in the
//! original, so a later exporter that feeds the engine must keep the flag, not
//! the alpha.

/// Pixels per texel in a TIM image block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 4 bits: two pixels per byte, palette index = nibble.
    Bpp4,
    /// 8 bits: one byte per pixel, palette index = byte.
    Bpp8,
    /// 16 bits: direct 5-5-5 colour.
    Bpp16,
    /// 24 bits: three bytes per pixel, direct colour.
    Bpp24,
}

/// The palette block: VRAM position, size, and the 16-bit entries in order.
#[derive(Debug)]
pub struct Clut {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub entries: Vec<u16>,
}

/// The image block: VRAM position, size, and the texel bytes as stored.
#[derive(Debug)]
pub struct Image {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub texels: Vec<u8>,
}

/// One parsed TIM.
#[derive(Debug)]
pub struct Tim {
    pub mode: Mode,
    pub clut: Option<Clut>,
    pub image: Image,
    /// Bytes the declared block sizes account for: the file's length when the
    /// TIM is exactly one image (what the disc's `.tim` resources usually are).
    pub declared_len: usize,
}

impl Tim {
    /// Parses the first TIM in `bytes`. Extra bytes after the image block are
    /// left alone: some of the disc's `.tim` resources are packs of several.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let word = |at: usize| -> Result<u32, String> {
            bytes
                .get(at..at + 4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .ok_or_else(|| format!("TIM is truncated at {at:#x}"))
        };
        let half = |at: usize| -> Result<u16, String> {
            bytes
                .get(at..at + 2)
                .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
                .ok_or_else(|| format!("TIM is truncated at {at:#x}"))
        };
        if word(0)? != 0x10 {
            return Err(format!("not a TIM: magic is {:#x}", word(0)?));
        }
        let flags = word(4)?;
        let mode = match flags & 7 {
            0 => Mode::Bpp4,
            1 => Mode::Bpp8,
            2 => Mode::Bpp16,
            3 => Mode::Bpp24,
            other => return Err(format!("unknown TIM pixel mode {other}")),
        };

        let mut at = 8;
        let mut clut = None;
        if flags & 8 != 0 {
            let size = word(at)? as usize;
            if size < 12 {
                return Err(format!("CLUT block declares only {size} bytes"));
            }
            let raw = bytes
                .get(at + 12..at + size)
                .ok_or_else(|| format!("CLUT data runs past the file ({size} at {at:#x})"))?;
            let entries = raw.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
            clut = Some(Clut { x: half(at + 4)?, y: half(at + 6)?, w: half(at + 8)?, h: half(at + 10)?, entries });
            at += size;
        }

        let size = word(at)? as usize;
        if size < 12 {
            return Err(format!("image block declares only {size} bytes"));
        }
        let image =
            Image { x: half(at + 4)?, y: half(at + 6)?, w: half(at + 8)?, h: half(at + 10)?, texels: Vec::new() };
        // 24-bit images keep a u32 after the header, before the texels.
        let texels_at = at + 12 + if mode == Mode::Bpp24 { 4 } else { 0 };
        let end = (at + size).min(bytes.len());
        let texels = bytes
            .get(texels_at..end)
            .ok_or_else(|| format!("image data starts past the file ({texels_at:#x})"))?
            .to_vec();
        Ok(Self { mode, clut, image: Image { texels, ..image }, declared_len: at + size })
    }

    /// The image's size in pixels. The header's `w` counts 16-bit words, so the
    /// pixel width is `w * 16 / bits-per-pixel`.
    pub fn pixel_size(&self) -> (u32, u32) {
        let pixels_per_word = match self.mode {
            Mode::Bpp4 => 4,
            Mode::Bpp8 => 2,
            Mode::Bpp16 | Mode::Bpp24 => 1,
        };
        (u32::from(self.image.w) * pixels_per_word, u32::from(self.image.h))
    }

    /// Whether the colour of every pixel is in the file. False for the indexed
    /// images whose palette the game uploads separately.
    pub fn has_palette(&self) -> bool {
        self.clut.is_some() || matches!(self.mode, Mode::Bpp16 | Mode::Bpp24)
    }

    /// The image as 8-bit RGBA, row-major, top row first.
    pub fn to_rgba(&self) -> Result<(u32, u32, Vec<u8>), String> {
        let (width, height) = self.pixel_size();
        let (w, h) = (width as usize, height as usize);
        if w == 0 || h == 0 {
            return Err(format!("image is {w}x{h} pixels"));
        }
        let pixels = w * h;
        let texels = &self.image.texels;
        let mut rgba = vec![0u8; pixels * 4];

        let put = |index: usize, colour: u16, rgba: &mut [u8]| {
            let (r, g, b, a) = split555(colour);
            rgba[index * 4..index * 4 + 4].copy_from_slice(&[r, g, b, a]);
        };

        match self.mode {
            Mode::Bpp16 => {
                if texels.len() < pixels * 2 {
                    return Err(format!("16-bit image has {} of {} texel bytes", texels.len(), pixels * 2));
                }
                for index in 0..pixels {
                    let colour = u16::from_le_bytes([texels[index * 2], texels[index * 2 + 1]]);
                    put(index, colour, &mut rgba);
                }
            }
            Mode::Bpp24 => {
                if texels.len() < pixels * 3 {
                    return Err(format!("24-bit image has {} of {} texel bytes", texels.len(), pixels * 3));
                }
                for index in 0..pixels {
                    let t = &texels[index * 3..index * 3 + 3];
                    rgba[index * 4..index * 4 + 4].copy_from_slice(&[t[0], t[1], t[2], 255]);
                }
            }
            Mode::Bpp4 | Mode::Bpp8 => {
                // Without a CLUT the indices are all we have; a grey ramp shows
                // the image's shape and keeps the indices readable.
                let fallback: Vec<u16> = (0..256u16)
                    .map(|i| {
                        let level = i >> 3;
                        level | (level << 5) | (level << 10)
                    })
                    .collect();
                let entries: &[u16] = match &self.clut {
                    Some(clut) => &clut.entries,
                    None => &fallback,
                };
                let per_byte = if self.mode == Mode::Bpp4 { 2 } else { 1 };
                if texels.len() < pixels.div_ceil(per_byte) {
                    return Err(format!(
                        "indexed image has {} of {} texel bytes",
                        texels.len(),
                        pixels.div_ceil(per_byte)
                    ));
                }
                let palette = |index: usize| entries.get(index).copied().unwrap_or(0);
                for index in 0..pixels {
                    let entry = if self.mode == Mode::Bpp4 {
                        let byte = texels[index / 2];
                        if index % 2 == 0 { byte & 0x0f } else { byte >> 4 }
                    } else {
                        texels[index]
                    };
                    put(index, palette(entry as usize), &mut rgba);
                }
            }
        }
        Ok((w as u32, h as u32, rgba))
    }
}

/// A 5-5-5 colour to 8-bit RGBA: bit 15 is the semi-transparency flag, kept as
/// alpha 128 so the distinction survives into the PNG.
fn split555(colour: u16) -> (u8, u8, u8, u8) {
    let expand = |channel: u16| {
        let value = (channel & 0x1f) as u8;
        (value << 3) | (value >> 2)
    };
    let alpha = if colour & 0x8000 != 0 { 128 } else { 255 };
    (expand(colour), expand(colour >> 5), expand(colour >> 10), alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-image TIM: `mode`, `width` x `height`, `clut` optional.
    fn tim(mode: Mode, width: u16, height: u16, clut: Option<&[u16]>, texels: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&0x10u32.to_le_bytes());
        let flag: u32 = match (mode, clut.is_some()) {
            (Mode::Bpp4, false) => 0,
            (Mode::Bpp4, true) => 8,
            (Mode::Bpp8, false) => 1,
            (Mode::Bpp8, true) => 9,
            (Mode::Bpp16, _) => 2,
            (Mode::Bpp24, _) => 3,
        };
        v.extend_from_slice(&flag.to_le_bytes());
        if let Some(clut) = clut {
            v.extend_from_slice(&((12 + clut.len() * 2) as u32).to_le_bytes());
            for _ in 0..4 {
                v.extend_from_slice(&0u16.to_le_bytes()); // x, y, then w, h below
            }
            v[12..14].copy_from_slice(&0u16.to_le_bytes()); // x
            v[16..18].copy_from_slice(&(clut.len() as u16).to_le_bytes()); // w
            v[18..20].copy_from_slice(&1u16.to_le_bytes()); // h
            for entry in clut {
                v.extend_from_slice(&entry.to_le_bytes());
            }
        }
        let header = if mode == Mode::Bpp24 { 16 } else { 12 };
        v.extend_from_slice(&((header + texels.len()) as u32).to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(&width.to_le_bytes());
        v.extend_from_slice(&height.to_le_bytes());
        if mode == Mode::Bpp24 {
            v.extend_from_slice(&0u32.to_le_bytes());
        }
        v.extend_from_slice(texels);
        v
    }

    #[test]
    fn parses_the_blocks_and_accounts_for_every_byte() {
        // 4bpp: the header's w is in 16-bit words, so w = 1 is four pixels.
        let bytes = tim(Mode::Bpp4, 1, 1, Some(&[0x001f, 0x03e0]), &[0x10, 0x32]);
        let parsed = Tim::parse(&bytes).unwrap();
        assert_eq!(parsed.mode, Mode::Bpp4);
        assert_eq!(parsed.pixel_size(), (4, 1));
        assert_eq!(parsed.image.texels, vec![0x10, 0x32]);
        assert_eq!(parsed.clut.as_ref().unwrap().entries, vec![0x001f, 0x03e0]);
        assert_eq!(parsed.declared_len, bytes.len(), "the header accounts for the whole file");
    }

    #[test]
    fn four_bit_pixels_use_the_low_nibble_first() {
        let palette = [0x001fu16, 0x03e0, 0x7c00, 0x7fff];
        let bytes = tim(Mode::Bpp4, 1, 1, Some(&palette), &[0x10, 0x32]);
        let (w, h, rgba) = Tim::parse(&bytes).unwrap().to_rgba().unwrap();
        assert_eq!((w, h), (4, 1));
        // Nibbles in order: 0, 1 then 2, 3 -> red, green, blue, white.
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
        assert_eq!(&rgba[4..8], &[0, 255, 0, 255]);
        assert_eq!(&rgba[8..12], &[0, 0, 255, 255]);
        assert_eq!(&rgba[12..16], &[255, 255, 255, 255]);
    }

    #[test]
    fn sixteen_bit_pixels_expand_and_keep_the_semi_transparency_flag() {
        let bytes = tim(
            Mode::Bpp16,
            3,
            1,
            None,
            &[0x001fu16, 0x03e0, 0x8000].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>(),
        );
        let (_, _, rgba) = Tim::parse(&bytes).unwrap().to_rgba().unwrap();
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
        assert_eq!(&rgba[4..8], &[0, 255, 0, 255]);
        assert_eq!(&rgba[8..12], &[0, 0, 0, 128], "bit 15 is the semi-transparency flag");
    }

    #[test]
    fn eight_bit_pixels_are_two_per_word() {
        // w = 2 words -> 4 pixels of 8 bits each.
        let palette = [0x001fu16, 0x03e0, 0x7c00, 0x7fff];
        let bytes = tim(Mode::Bpp8, 2, 1, Some(&palette), &[0, 1, 2, 3]);
        let parsed = Tim::parse(&bytes).unwrap();
        assert_eq!(parsed.pixel_size(), (4, 1));
        assert!(parsed.has_palette());
        let (w, h, rgba) = parsed.to_rgba().unwrap();
        assert_eq!((w, h), (4, 1));
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
        assert_eq!(&rgba[12..16], &[255, 255, 255, 255]);
    }

    #[test]
    fn an_indexed_image_without_a_palette_shows_a_grey_ramp() {
        let bytes = tim(Mode::Bpp8, 1, 1, None, &[0x00, 0xff]);
        let parsed = Tim::parse(&bytes).unwrap();
        assert_eq!(parsed.pixel_size(), (2, 1));
        assert!(!parsed.has_palette(), "the palette lives outside the file");
        let (_, _, rgba) = parsed.to_rgba().unwrap();
        assert_eq!(&rgba[0..4], &[0, 0, 0, 255]);
        assert_eq!(&rgba[4..8], &[255, 255, 255, 255]);
    }

    #[test]
    fn rejects_a_non_tim_and_a_truncated_block() {
        assert!(Tim::parse(&[0u8; 16]).is_err());
        let mut bytes = tim(Mode::Bpp16, 4, 1, None, &[0u8; 8]);
        assert!(Tim::parse(&bytes).is_ok());
        bytes.truncate(bytes.len() - 4); // drop half the image
        assert!(Tim::parse(&bytes).unwrap().to_rgba().is_err());
    }
}
