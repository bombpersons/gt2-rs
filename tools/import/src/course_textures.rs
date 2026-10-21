//! Offline course TIM packs and GPU material addresses -> ordinary RGBA PNGs.
//!
//! Nothing in this module belongs in the runtime: the exported material names
//! and normalized UVs replace the source's texture pages and CLUT addresses.
use crate::tim::{Mode, Tim};
use std::collections::BTreeSet;
use std::path::Path;

const WIDTH: usize = 1024;
const HEIGHT: usize = 512;

pub struct TexturePack {
    words: Vec<u16>,
    present: Vec<bool>,
    pub tim_count: usize,
}

#[derive(Debug)]
pub struct TextureAsset {
    /// Offline lookup key; do not serialize this into runtime materials.
    pub tpage: u16,
    pub clut: u16,
    /// Filename relative to the caller's output directory.
    pub texture: String,
}

impl TexturePack {
    /// Parse a decompressed `.trp`: count followed by complete, contiguous TIMs.
    /// Upload order matters: later entries can replace earlier palette words.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let count = bytes.get(..4).ok_or("texture pack has no count")?;
        let tim_count = u32::from_le_bytes(count.try_into().unwrap()) as usize;
        if tim_count == 0 || tim_count > bytes.len().saturating_sub(4) / 20 {
            return Err(format!("impossible texture pack count {tim_count}"));
        }
        let mut pack = Self { words: vec![0; WIDTH * HEIGHT], present: vec![false; WIDTH * HEIGHT], tim_count };
        let mut at = 4;
        for index in 0..tim_count {
            let tim = Tim::parse(bytes.get(at..).ok_or("texture pack is truncated")?)
                .map_err(|e| format!("texture {index} at {at:#x}: {e}"))?;
            if tim.declared_len > bytes.len() - at {
                return Err(format!("texture {index} extends past the pack"));
            }
            if tim.mode == Mode::Bpp24 {
                return Err("24-bit course TIM upload is not established".into());
            }
            if let Some(clut) = tim.clut {
                let declared = u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
                if declared != 12 + clut.entries.len() * 2 {
                    return Err(format!("texture {index} has an odd CLUT byte count"));
                }
                pack.upload(clut.x, clut.y, clut.w, clut.h, &clut.entries)?;
            }
            if tim.image.texels.len() % 2 != 0 {
                return Err(format!("texture {index} has an odd texel byte count"));
            }
            let words: Vec<_> = tim.image.texels.as_chunks::<2>().0.iter().copied().map(u16::from_le_bytes).collect();
            pack.upload(tim.image.x, tim.image.y, tim.image.w, tim.image.h, &words)?;
            at += tim.declared_len;
        }
        if at != bytes.len() {
            return Err(format!("texture pack leaves {} trailing bytes", bytes.len() - at));
        }
        Ok(pack)
    }

    fn upload(&mut self, x: u16, y: u16, w: u16, h: u16, words: &[u16]) -> Result<(), String> {
        let [x, y, w, h] = [x, y, w, h].map(usize::from);
        if w == 0 || h == 0 || x + w > WIDTH || y + h > HEIGHT || words.len() != w * h {
            return Err(format!("invalid TIM rectangle ({x}, {y}, {w}, {h}) with {} words", words.len()));
        }
        for row in 0..h {
            let start = (y + row) * WIDTH + x;
            self.words[start..start + w].copy_from_slice(&words[row * w..(row + 1) * w]);
            self.present[start..start + w].fill(true);
        }
        Ok(())
    }

    fn address(tpage: u16, u: usize, v: usize) -> Result<(usize, usize, usize), String> {
        let depth = usize::from(tpage >> 7 & 3);
        let per_word = match depth {
            0 => 4,
            1 => 2,
            2 => 1,
            _ => return Err("reserved texture page depth 3".into()),
        };
        let x = usize::from(tpage & 15) * 64 + u / per_word;
        let y = usize::from(tpage >> 4 & 1) * 256 + v;
        if x >= WIDTH || y >= HEIGHT {
            return Err("texture page extends outside VRAM".into());
        }
        Ok((y * WIDTH + x, per_word, depth))
    }

    fn palette(&self, tpage: u16, clut: u16) -> Result<usize, String> {
        let depth = tpage >> 7 & 3;
        if depth == 2 {
            return Ok(0);
        }
        let count = match depth {
            0 => 16,
            1 => 256,
            _ => return Err("reserved texture page depth 3".into()),
        };
        let x = usize::from(clut & 63) * 16;
        let y = usize::from(clut >> 6);
        if x + count > WIDTH || y >= HEIGHT {
            return Err(format!("CLUT {clut:#x} extends outside VRAM"));
        }
        let start = y * WIDTH + x;
        if !self.present[start..start + count].iter().all(|&p| p) {
            return Err(format!("CLUT {clut:#x} is absent from the texture pack"));
        }
        Ok(start)
    }

    /// Check the conservative UV bounding rectangle, including every texel
    /// the polygon can sample. Sparse padding in a page is never silently used.
    pub fn validate_uv_rect(&self, tpage: u16, clut: u16, uv: &[[u8; 2]]) -> Result<(), String> {
        self.palette(tpage, clut)?;
        if uv.is_empty() {
            return Err("a primitive has no UVs".into());
        }
        let min_u = uv.iter().map(|p| p[0]).min().unwrap();
        let max_u = uv.iter().map(|p| p[0]).max().unwrap();
        let min_v = uv.iter().map(|p| p[1]).min().unwrap();
        let max_v = uv.iter().map(|p| p[1]).max().unwrap();
        for v in min_v..=max_v {
            for u in min_u..=max_u {
                let (at, _, _) = Self::address(tpage, u.into(), v.into())?;
                if !self.present[at] {
                    return Err(format!("texture page {tpage:#x} samples missing texel ({u}, {v})"));
                }
            }
        }
        Ok(())
    }

    /// Full 256x256 page with absent words transparent. Pixel zero is fully
    /// transparent; STP is retained as alpha 128 for the engine's PS1 material.
    pub fn rgba(&self, tpage: u16, clut: u16) -> Result<Vec<u8>, String> {
        let palette = self.palette(tpage, clut)?;
        let mut rgba = vec![0; 256 * 256 * 4];
        for v in 0..256 {
            for u in 0..256 {
                let (at, per_word, depth) = Self::address(tpage, u, v)?;
                if !self.present[at] {
                    continue;
                }
                let word = self.words[at];
                let colour = match depth {
                    0 => self.words[palette + usize::from(word >> ((u % per_word) * 4) & 15)],
                    1 => self.words[palette + usize::from(word >> ((u % per_word) * 8) & 255)],
                    _ => word,
                };
                let expand = |c: u16| {
                    let c = (c & 31) as u8;
                    c << 3 | c >> 2
                };
                let alpha = if colour == 0 {
                    0
                } else if colour & 0x8000 != 0 {
                    128
                } else {
                    255
                };
                let pixel = (v * 256 + u) * 4;
                rgba[pixel..pixel + 4].copy_from_slice(&[
                    expand(colour),
                    expand(colour >> 5),
                    expand(colour >> 10),
                    alpha,
                ]);
            }
        }
        Ok(rgba)
    }

    /// Export deterministic ordinary image names for the requested materials.
    /// The caller replaces offline keys with these names and UVs / 256.
    pub fn export(&self, out_dir: &Path, materials: &[(u16, u16)]) -> Result<Vec<TextureAsset>, String> {
        std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
        let keys: BTreeSet<_> = materials.iter().copied().collect();
        let mut assets = Vec::new();
        for (index, (tpage, clut)) in keys.into_iter().enumerate() {
            let rgba = self.rgba(tpage, clut)?;
            let texture = format!("material_{index:03}.png");
            let file = std::fs::File::create(out_dir.join(&texture)).map_err(|e| e.to_string())?;
            let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 256, 256);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
            writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
            writer.finish().map_err(|e| e.to_string())?;
            assets.push(TextureAsset { tpage, clut, texture });
        }
        Ok(assets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut bytes = 1u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&0x10u32.to_le_bytes());
        bytes.extend_from_slice(&8u32.to_le_bytes());
        for (x, y, w, h, words) in [
            (640u16, 500u16, 16u16, 1u16, vec![0u16, 0x001f, 0x03e0, 0x8000, 0x7fff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            (768, 0, 1, 1, vec![0x3210]),
        ] {
            bytes.extend_from_slice(&(12u32 + words.len() as u32 * 2).to_le_bytes());
            for value in [x, y, w, h] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            for value in words {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn indexed_page_preserves_colour_zero_stp_and_nibble_order() {
        let pack = TexturePack::parse(&fixture()).unwrap();
        let clut = 500 * 64 + 640 / 16;
        let rgba = pack.rgba(12, clut).unwrap();
        assert_eq!(&rgba[..16], &[0, 0, 0, 0, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 0, 128]);
        pack.validate_uv_rect(12, clut, &[[0, 0], [3, 0]]).unwrap();
        assert!(pack.validate_uv_rect(12, clut, &[[0, 0], [4, 0]]).unwrap_err().contains("missing texel"));
        assert!(pack.rgba(12, clut + 1).unwrap_err().contains("absent"));
    }

    #[test]
    fn later_palette_uploads_replace_earlier_words() {
        let one = fixture();
        let mut bytes = 2u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&one[4..]);
        let second = bytes.len();
        bytes.extend_from_slice(&one[4..]);
        // Second TIM: 8-byte header, 12-byte CLUT header, entry 1.
        bytes[second + 22..second + 24].copy_from_slice(&0x7c00u16.to_le_bytes());
        let pack = TexturePack::parse(&bytes).unwrap();
        let rgba = pack.rgba(12, 500 * 64 + 40).unwrap();
        assert_eq!(&rgba[4..8], &[0, 0, 255, 255]);
    }

    #[test]
    fn rejects_truncation_trailing_bytes_bad_rectangles_and_counts() {
        let bytes = fixture();
        assert!(TexturePack::parse(&bytes[..bytes.len() - 1]).is_err());
        let mut bad = bytes.clone();
        bad.push(0);
        assert!(TexturePack::parse(&bad).err().unwrap().contains("trailing"));
        let mut bad = bytes.clone();
        bad[16..18].copy_from_slice(&1024u16.to_le_bytes());
        assert!(TexturePack::parse(&bad).is_err());
        let mut bad = bytes;
        bad[..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(TexturePack::parse(&bad).is_err());
    }

    /// Private source evidence remains outside the repository. This checks
    /// every B-1 material variant, then byte-compares all uploaded words with
    /// the independently captured original runtime, including overwritten CLUTs.
    #[test]
    #[ignore = "requires GT2_B1_TRP, GT2_B1_TRO and GT2_B1_VRAM private evidence paths"]
    fn b1_materials_match_the_original_texture_uploads() {
        let read = |name| std::fs::read(std::env::var(name).expect(name)).unwrap();
        let pack = TexturePack::parse(&read("GT2_B1_TRP")).unwrap();
        assert_eq!(pack.tim_count, 44);
        let vram = read("GT2_B1_VRAM");
        assert_eq!(vram.len(), WIDTH * HEIGHT * 2);
        let mut occupied = 0;
        for (index, &present) in pack.present.iter().enumerate() {
            if present {
                let observed = u16::from_le_bytes(vram[index * 2..index * 2 + 2].try_into().unwrap());
                assert_eq!(pack.words[index], observed, "VRAM word {index:#x}");
                occupied += 1;
            }
        }
        assert_eq!(occupied, 33_376);

        let bytes = read("GT2_B1_TRO");
        let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let half = |at| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
        let mut keys = Vec::new();
        let mut check = |at, quad| {
            let clut = half(at + 2);
            let tpage = half(at + 6);
            let mut uv =
                vec![[bytes[at], bytes[at + 1]], [bytes[at + 4], bytes[at + 5]], [bytes[at + 8], bytes[at + 9]]];
            if quad {
                uv.push([bytes[at + 10], bytes[at + 11]]);
            }
            pack.validate_uv_rect(tpage, clut, &uv).unwrap_or_else(|e| panic!("material at {at:#x}: {e}"));
            keys.push((tpage, clut));
        };
        let section = word(0x18);
        let mut section2_count = 0;
        for index in 0..word(section) {
            let mesh = word(section + 4 + index * 4);
            for group in 4..8 {
                let stride = [24, 24, 32, 36][group - 4];
                for primitive in 0..half(mesh + 0x30 + group * 2) as usize {
                    check(word(mesh + 4 + group * 4) + primitive * stride + 12, group % 2 != 0);
                    section2_count += 1;
                }
            }
        }
        assert_eq!(section2_count, 280);
        let section = word(0x10);
        assert_eq!(half(section + 6), 403);
        let base = word(section + 8);
        for index in 0..403 {
            check(base + index * 32, true);
            check(base + index * 32 + 16, true);
        }
        let out = std::env::temp_dir().join(format!("gt2-b1-texture-materials-{}", std::process::id()));
        let assets = pack.export(&out, &keys).unwrap();
        eprintln!(
            "validated {section2_count} mesh materials, 806 template variants and {occupied} runtime words; exported {} images to {}",
            assets.len(),
            out.display()
        );
        for asset in assets {
            assert!(keys.contains(&(asset.tpage, asset.clut)));
            assert!(out.join(asset.texture).is_file());
        }
    }

    #[test]
    #[ignore = "requires GT2_TRP_CORPUS containing extracted, decompressed course packs"]
    fn validates_extracted_texture_pack_corpus() {
        let dir = std::env::var("GT2_TRP_CORPUS").expect("GT2_TRP_CORPUS");
        let mut files = 0;
        let mut textures = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "trp") {
                let pack = TexturePack::parse(&std::fs::read(&path).unwrap())
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                files += 1;
                textures += pack.tim_count;
            }
        }
        assert_eq!(files, 126);
        assert_eq!(textures, 14_138);
        eprintln!("validated {files} complete texture packs, {textures} TIM entries");
    }

    #[test]
    #[ignore = "requires GT2_TRP_CORPUS and GT2_TRO_CORPUS paired decompressed course files"]
    fn validates_material_bindings_across_the_course_corpus() {
        let textures = std::env::var("GT2_TRP_CORPUS").expect("GT2_TRP_CORPUS");
        let courses = std::env::var("GT2_TRO_CORPUS").expect("GT2_TRO_CORPUS");
        let mut files = 0;
        let mut mesh_materials = 0;
        let mut template_variants = 0;
        let mut unique_materials = 0;
        for entry in std::fs::read_dir(textures).unwrap() {
            let path = entry.unwrap().path();
            if !path.extension().is_some_and(|e| e == "trp") {
                continue;
            }
            let pack = TexturePack::parse(&std::fs::read(&path).unwrap()).unwrap();
            let course_path = Path::new(&courses).join(path.file_name().unwrap()).with_extension("tro");
            let bytes = std::fs::read(&course_path).unwrap();
            let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            let half = |at| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
            let mut keys = BTreeSet::new();
            let mut check = |at, quad| {
                let tpage = half(at + 6);
                let clut = half(at + 2);
                let mut uv =
                    vec![[bytes[at], bytes[at + 1]], [bytes[at + 4], bytes[at + 5]], [bytes[at + 8], bytes[at + 9]]];
                if quad {
                    uv.push([bytes[at + 10], bytes[at + 11]]);
                }
                assert_eq!(tpage >> 7 & 3, 0, "{} material depth at {at:#x}", course_path.display());
                pack.validate_uv_rect(tpage, clut, &uv)
                    .unwrap_or_else(|e| panic!("{} material at {at:#x}: {e}", course_path.display()));
                keys.insert((tpage, clut));
            };
            let section = word(0x18);
            for index in 0..word(section) {
                let mesh = word(section + 4 + index * 4);
                for group in 4..8 {
                    let stride = [24, 24, 32, 36][group - 4];
                    for primitive in 0..half(mesh + 0x30 + group * 2) as usize {
                        check(word(mesh + 4 + group * 4) + primitive * stride + 12, group % 2 != 0);
                        mesh_materials += 1;
                    }
                }
            }
            let section = word(0x10);
            let base = word(section + 8);
            for index in 0..half(section + 6) as usize {
                check(base + index * 32, true);
                check(base + index * 32 + 16, true);
                template_variants += 2;
            }
            unique_materials += keys.len();
            files += 1;
        }
        assert_eq!((files, mesh_materials, template_variants, unique_materials), (126, 131_332, 255_422, 13_023));
        eprintln!(
            "validated {mesh_materials} section-2 faces and {template_variants} template variants, {unique_materials} material pairs across {files} courses; no missing palettes or texels"
        );
    }
}
