//! Sound samples: the original's engine sounds, in the importer's baked form.
//!
//! `import engines` decodes the original's `ENGN` files — SPU ADPCM — to mono
//! 16-bit WAV at the SPU's native 44 100 Hz and writes a [`SoundTable`] beside
//! them saying which sample each original file uses. The table is data: a
//! different game can bake its own and read it with this type.
//!
//! This is the data half of the engine's audio. The workspace does not enable
//! Bevy's audio backend, so nothing here plays a sample: the game's presentation
//! layer loads the WAV (`AssetServer`) and plays it (Bevy's `AudioPlayer`),
//! reading the sample's identity and rate here. Pitch is the original's per-RPM
//! choice and belongs to the controller that drives the voice.
use serde::{Deserialize, Serialize};

/// The rate every baked sample is written at: the SPU's native rate. A voice
/// playing a sample untouched (pitch `0x1000`, "1.0") sounds as authored.
pub const SAMPLE_RATE: u32 = 44_100;

/// The baked sounds: the sample files and the table that names them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SoundTable {
    /// The rate the samples were decoded at (see [`SAMPLE_RATE`]).
    #[serde(default)]
    pub sample_rate: u32,
    /// The distinct decoded samples, each a WAV beside this table.
    #[serde(default)]
    pub samples: Vec<Sample>,
    /// Every original sound file, in the order the importer baked them.
    #[serde(default)]
    pub files: Vec<SoundFile>,
}

/// One decoded sample: a WAV beside the table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// File name relative to the table, e.g. `sample_0000.wav`.
    pub file: String,
    /// ADPCM frames in the source payload; each frame is 28 samples.
    pub frames: u32,
    /// FNV-1a 64 of the source payload, as the importer's dedup key.
    pub source_fnv1a64: String,
}

impl Sample {
    /// Mono samples in the file (28 per ADPCM frame).
    pub fn samples(&self) -> u32 {
        self.frames * 28
    }

    /// Playback length at `rate` Hz.
    pub fn seconds(&self, rate: u32) -> f32 {
        if rate == 0 { 0.0 } else { self.samples() as f32 / rate as f32 }
    }
}

/// One of the original's sound files.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SoundFile {
    /// The original's path for it, e.g. `/engine/ene_n.es`.
    pub name: String,
    /// Index into [`SoundTable::samples`].
    pub sample: usize,
    /// Header records after the first. The zone table's fields are not yet
    /// interpreted (the loader lives in a `GT2.OVL` overlay); this only counts
    /// them, so nothing is silently dropped.
    pub zones: u32,
}

impl SoundTable {
    /// The sound file with this original path.
    pub fn find(&self, name: &str) -> Option<&SoundFile> {
        self.files.iter().find(|file| file.name == name)
    }

    /// The sample a sound file uses.
    pub fn sample_of(&self, file: &SoundFile) -> Option<&Sample> {
        self.samples.get(file.sample)
    }

    /// The sample's asset path, resolved relative to the table's own asset path
    /// (`gt2/sound/table.ron` -> `gt2/sound/sample_0000.wav`). Sample files sit
    /// beside the table, so the table can be moved with its folder.
    pub fn sample_asset(&self, index: usize, table_asset: &str) -> Option<String> {
        let file = &self.samples.get(index)?.file;
        Some(match table_asset.rsplit_once('/') {
            Some((dir, _)) => format!("{dir}/{file}"),
            None => file.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_length_is_frames_of_twenty_eight() {
        let sample = Sample { file: "x.wav".into(), frames: 250, source_fnv1a64: String::new() };
        assert_eq!(sample.samples(), 7000);
        assert!((sample.seconds(44_100) - 0.158_73).abs() < 1e-4);
    }

    #[test]
    fn sample_paths_resolve_beside_the_table() {
        let table: SoundTable = ron::from_str(
            r#"(sample_rate: 44100, samples: [(file: "sample_0000.wav", frames: 1, source_fnv1a64: "0")], files: [])"#,
        )
        .unwrap();
        assert_eq!(table.sample_asset(0, "gt2/sound/table.ron").as_deref(), Some("gt2/sound/sample_0000.wav"));
        assert_eq!(table.sample_asset(0, "table.ron").as_deref(), Some("sample_0000.wav"));
        assert_eq!(table.sample_asset(1, "table.ron"), None);
    }

    #[test]
    fn a_baked_table_loads() {
        // Written by `import engines`, kept in the game's assets: the importer
        // and this type must agree on the shape.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../game/assets/gt2/sound/table.ron");
        let Ok(text) = std::fs::read_to_string(&path) else {
            // Allow a checkout without imported assets yet.
            return;
        };
        let table: SoundTable = ron::from_str(&text).expect("baked sound table parses");
        assert_eq!(table.sample_rate, SAMPLE_RATE);
        assert!(!table.samples.is_empty(), "the baked table has samples");
        for file in &table.files {
            assert!(table.sample_of(file).is_some(), "{} names a sample that exists", file.name);
        }
        // The boot executable itself names these two; they are what the bake
        // covers until the overlay's per-car loader is ported.
        let ene_n = table.find("/engine/ene_n.es").expect("/engine/ene_n.es is baked");
        assert_eq!(ene_n.zones, 8);
        assert!(table.find("/engine/ene_t.es").is_some());
    }
}
