//! `ENGN`: the original's sound samples (`/engine/**/*.es`).
//!
//! 2747 resources in the volume. Header, verified against all of them (the
//! fields are the same for every file; a `_n0.._n3`/`_t0.._t3` group and `ene`
//! included):
//!
//! ```text
//! 0x00  "ENGN"          magic
//! 0x04  u32 0
//! 0x08  u32 header      header size: 0x20 + (zones + 1) * 16
//! 0x0c  u32 0
//! 0x10  u32 payload     ADPCM payload size in bytes, a multiple of 16
//! 0x14  u32 header      the same value as 0x08
//! 0x18  u32 zones       records after the first, 0..=10 observed
//! 0x1c  u32 0x30        constant in every file
//! 0x20  record[0]       16 bytes: 0, 0, 0x7f, 0x7f (as four u32)
//! 0x30  record[1..=zones], 16 bytes each
//! ...   payload         `payload` bytes of SPU ADPCM
//! ```
//!
//! The records are 16 bytes on a fixed grid — `0x20 + (zones + 1) * 16` equals
//! the header size in all 2747 files — but their *fields* are not established.
//! They plausibly hold a per-RPM zone table (for `/engine/00001.es` the second
//! u16 of each record walks 1000, 2000, … 8000 while the payload is one loop),
//! so this decoder keeps them only as a count and reports the rest as
//! unread; naming the fields needs the loader, which lives in a `GT2.OVL`
//! overlay (the boot executable names only `/engine/ene_n.es` and
//! `/engine/ene_t.es`).
//!
//! The payload is the PS1 SPU's ADPCM: 16-byte frames, each 1 byte of shift
//! (low nibble) and filter (high nibble), 1 flags byte, then 14 bytes of 28
//! four-bit samples, high nibble first. It decodes to 28 samples per frame at
//! the SPU's native 44 100 Hz. The decode is checked by ear-shaped evidence:
//! `/engine/00000.es` decodes to a 441 Hz tone with an autocorrelation of
//! 0.995, i.e. exactly the periodic loop the format implies.

/// The SPU's native rate: it always decodes at 44100 and resamples through the
/// voice pitch. Baked WAVs are written at this rate, so a voice that plays a
/// sample untouched (pitch `0x1000`) sounds as authored.
pub const SAMPLE_RATE: u32 = 44_100;

/// Bytes in one ADPCM frame, and the samples it carries.
const FRAME_BYTES: usize = 16;
const FRAME_SAMPLES: usize = 28;

/// The ADPCM predictor pairs, indexed by a frame's filter nibble (psx-spx).
/// Filters 5..7 are undefined; they predict nothing.
const FILTERS: [(i32, i32); 8] = [(0, 0), (60, 0), (115, -52), (98, -55), (122, -60), (0, 0), (0, 0), (0, 0)];

/// A parsed `ENGN` file: its header sizes and its borrowed payload.
#[derive(Debug, PartialEq, Eq)]
pub struct Engn<'a> {
    /// Header size in bytes (`0x20 + (zones + 1) * 16`).
    pub header_size: u32,
    pub zones: u32,
    /// Payload size in bytes, a multiple of 16.
    pub payload_size: u32,
    /// The SPU ADPCM payload.
    pub payload: &'a [u8],
}

impl<'a> Engn<'a> {
    /// Parses the header and slices out the payload. Rejects a file whose
    /// declared sizes do not describe the bytes present.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, String> {
        if bytes.len() < 0x20 {
            return Err(format!("ENGN: {}-byte file is shorter than its header", bytes.len()));
        }
        if &bytes[0..4] != b"ENGN" {
            return Err(format!("ENGN: magic {:02x?}, not \"ENGN\"", &bytes[0..4]));
        }
        let at = |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let (header_size, payload_size, zones, marker) = (at(0x08), at(0x10), at(0x18), at(0x1c));
        if at(0x0c) != 0 || at(0x14) != header_size {
            return Err(format!("ENGN: header fields disagree ({:#x}/{:#x})", at(0x0c), at(0x14)));
        }
        let expected = 0x20 + (zones + 1) * FRAME_BYTES as u32;
        if header_size != expected {
            return Err(format!("ENGN: header {header_size} is not {expected} for {zones} zones"));
        }
        if marker != 0x30 {
            return Err(format!("ENGN: marker {marker:#x} is not 0x30"));
        }
        if payload_size % FRAME_BYTES as u32 != 0 {
            return Err(format!("ENGN: payload {payload_size} is not a whole number of frames"));
        }
        let end = header_size.checked_add(payload_size).ok_or("ENGN: sizes overflow")?;
        if end as usize > bytes.len() {
            return Err(format!("ENGN: header {header_size} + payload {payload_size} exceeds {}", bytes.len()));
        }
        Ok(Self { header_size, zones, payload_size, payload: &bytes[header_size as usize..end as usize] })
    }

    /// Samples in one channel per voice: 28 per 16-byte frame.
    pub fn samples(&self) -> usize {
        self.payload.len() / FRAME_BYTES * FRAME_SAMPLES
    }
}

/// Decodes an SPU ADPCM payload to signed 16-bit PCM.
pub fn adpcm_to_pcm(payload: &[u8]) -> Vec<i16> {
    let mut out = Vec::with_capacity(payload.len() / FRAME_BYTES * FRAME_SAMPLES);
    let (mut previous, mut older) = (0i32, 0i32);
    for frame in payload.as_chunks::<FRAME_BYTES>().0 {
        let shift = frame[0] & 0x0f;
        let (first, second) = FILTERS[usize::from(frame[0] >> 4) & 0x07];
        for &pair in &frame[2..] {
            for nibble in [pair >> 4, pair & 0x0f] {
                // The four bits are a signed value scaled into the top of the
                // word, then shifted down by the frame's exponent and added to
                // the filters' prediction of this sample.
                let value = if nibble < 8 { i32::from(nibble) } else { i32::from(nibble) - 16 };
                let sample = ((value << 12) >> shift) + ((previous * first + older * second + 32) >> 6);
                let sample = sample.clamp(i32::from(i16::MIN), i32::from(i16::MAX));
                out.push(sample as i16);
                older = previous;
                previous = sample;
            }
        }
    }
    out
}

/// Wraps PCM as a mono 16-bit RIFF/WAVE file.
pub fn wav(pcm: &[i16], sample_rate: u32) -> Vec<u8> {
    let data = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // bytes per second
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for sample in pcm {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

/// The engine's sound table, as `import engines` writes it (RON). `engine`'s
/// `sound::SoundTable` mirrors this shape; a test there loads a baked file so
/// the two cannot drift.
#[derive(Debug, serde::Serialize)]
pub struct SoundTable {
    /// Every sample was decoded at the SPU's native rate.
    pub sample_rate: u32,
    /// The distinct decoded samples, in first-seen order.
    pub samples: Vec<Sample>,
    /// Every `ENGN` resource, in volume path order.
    pub files: Vec<SoundFile>,
}

/// One decoded sample file.
#[derive(Debug, serde::Serialize)]
pub struct Sample {
    /// Path under the asset root, e.g. `gt2/sound/sample_0000.wav`.
    pub file: String,
    /// ADPCM frames in the source payload (28 samples each).
    pub frames: u32,
    /// FNV-1a 64 of the source payload, as the dedup key.
    pub source_fnv1a64: String,
}

/// One of the original's sound files.
#[derive(Debug, serde::Serialize)]
pub struct SoundFile {
    /// The volume path, e.g. `/engine/ene_n.es`.
    pub name: String,
    /// Index into `samples`.
    pub sample: usize,
    /// Header records after the first; the zone table's fields are not yet
    /// interpreted (see the module docs).
    pub zones: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An `ENGN` file with `zones` zones and `frames` all-zero frames.
    fn engn_file(zones: u32, frames: u32, payload: &[u8]) -> Vec<u8> {
        let header = 0x20 + (zones + 1) * 16;
        let mut v = vec![0u8; header as usize + payload.len()];
        v[0..4].copy_from_slice(b"ENGN");
        v[0x08..0x0c].copy_from_slice(&header.to_le_bytes());
        v[0x10..0x14].copy_from_slice(&(frames * 16).to_le_bytes());
        v[0x14..0x18].copy_from_slice(&header.to_le_bytes());
        v[0x18..0x1c].copy_from_slice(&zones.to_le_bytes());
        v[0x1c..0x20].copy_from_slice(&0x30u32.to_le_bytes());
        v[0x28..0x2c].copy_from_slice(&0x7fu32.to_le_bytes());
        v[0x2c..0x30].copy_from_slice(&0x7fu32.to_le_bytes());
        v[header as usize..].copy_from_slice(payload);
        v
    }

    #[test]
    fn parses_the_header_and_slices_the_payload() {
        let file = engn_file(2, 1, &[0u8; 16]);
        let engn = Engn::parse(&file).unwrap();
        assert_eq!(engn.header_size, 0x20 + 3 * 16);
        assert_eq!(engn.zones, 2);
        assert_eq!(engn.payload_size, 16);
        assert_eq!(engn.payload, &[0u8; 16]);
        assert_eq!(engn.samples(), 28);
    }

    #[test]
    fn rejects_files_whose_sizes_do_not_describe_the_bytes() {
        let good = engn_file(0, 1, &[0u8; 16]);
        assert!(Engn::parse(&good).is_ok());

        let mut bad = good.clone();
        bad[0] = b'X';
        assert!(Engn::parse(&bad).is_err(), "magic");

        let mut bad = good.clone();
        bad[0x18..0x1c].copy_from_slice(&1u32.to_le_bytes());
        assert!(Engn::parse(&bad).is_err(), "zone count disagrees with the header size");

        let mut bad = good.clone();
        bad[0x10..0x14].copy_from_slice(&17u32.to_le_bytes());
        assert!(Engn::parse(&bad).is_err(), "payload is not a whole number of frames");

        let bad = engn_file(0, 2, &[0u8; 16]); // declares 32 payload bytes, has 16
        assert!(Engn::parse(&bad).is_err(), "payload past the end of the file");
    }

    #[test]
    fn decodes_a_frame_of_bare_nibbles() {
        // shift 0, filter 0: each nibble is its own sample, value << 12.
        let mut frame = [0u8; 16];
        frame[2] = 0x12;
        frame[3] = 0x3f; // -1 in four-bit two's complement
        let pcm = adpcm_to_pcm(&frame);
        assert_eq!(pcm.len(), 28);
        assert_eq!(&pcm[..4], &[0x1000, 0x2000, 0x3000, -0x1000]);
        assert!(pcm[4..].iter().all(|&s| s == 0));
    }

    #[test]
    fn shift_scales_down_and_the_filter_decays_the_previous_sample() {
        // Frame 0: filter 0, so all nibbles pass through; the last one is 1 at
        // shift 0, leaving `previous` at 0x1000 for the next frame.
        let mut first = [0u8; 16];
        first[15] = 0x01;
        // Frame 1: all-zero nibbles, filter 1 (0*old + 60*previous) at shift 0.
        let mut second = [0u8; 16];
        second[0] = 0x10;
        let pcm = adpcm_to_pcm(&[first, second].concat());
        assert!(pcm[..27].iter().all(|&s| s == 0));
        assert_eq!(pcm[27], 0x1000);
        // (0x1000 * 60 + 32) >> 6 = 3840, then a tail of zeros predicts this
        // sample: (3840 * 60 + 32) >> 6 = 3600.
        assert_eq!(pcm[28], 3840);
        assert_eq!(pcm[29], 3600);

        // A shift of 1 halves the nibble's contribution.
        let mut frame = [0u8; 16];
        frame[0] = 0x01;
        frame[2] = 0x20;
        assert_eq!(adpcm_to_pcm(&frame)[0], 0x1000);
    }

    #[test]
    fn writes_a_mono_wav() {
        let wav = wav(&[0, 1, -1], 44_100);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(u32::from_le_bytes(wav[16..20].try_into().unwrap()), 16);
        assert_eq!(u16::from_le_bytes(wav[20..22].try_into().unwrap()), 1, "PCM");
        assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 1, "mono");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 44_100);
        assert_eq!(u16::from_le_bytes(wav[34..36].try_into().unwrap()), 16, "16-bit");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 6);
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()), 36 + 6);
        assert_eq!(wav.len(), 44 + 6);
        assert_eq!(i16::from_le_bytes(wav[46..48].try_into().unwrap()), 1);
    }
}
