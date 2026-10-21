//! Offline importers: files extracted from the disc with `psx` (for example
//! `psx extract`) -> engine-native assets under `game/assets/`.
//!
//! The original's raw formats (fixed-point units, packed bitfields, offsets
//! into word arrays, CLUT/VRAM layouts) live only here. Each importer converts
//! exactly into the engine's format and records where every output came from
//! with [`Provenance`], so a reader can check an asset against the disc file.
//! Validate structurally: every byte of the source accounted for, every
//! reference in range.
//!
//! Subcommands:
//!
//! ```text
//! import provenance <source> <output>      write <output>.provenance.json
//! import isofs <image>                     print the disc's file table
//! import volume <image> --out <json>       write the GTFS volume index
//! import extract <image> --path <path> --out <file>
//! import extract <image> --id <n> --out <file>
//! import tim <file> --out <png>            a TIM (raw or gzipped) -> PNG
//! import tims <image> --out <dir>          every TIM in the volume -> PNGs
//! import text <file> --out <ron>           a WSDB table (raw or gzipped) -> RON
//! import engen <file.es> --out <wav>       an ENGN sample (decoded) -> WAV
//! import engines <image> --out <dir> [--match <s>]  ENGN samples -> WAVs + table
//! import menu <dat> --idx <idx>            split a menu archive and account for every byte
//! import overlays <image> --out <dir>      GT2.OVL: the gzip overlay container -> images
//! import tro <file.tro> --out <ron> [--name <disc path>]  a course -> engine course RON
//! ```
//!
//! `<image>` is the disc: a raw `.bin`, a cooked `.iso`, or a `.cue`. `extract`
//! writes a volume resource as it is stored (compressed, if the original
//! stores it compressed): use it to study a format or feed a converter. `tim`
//! reads a file that `extract` wrote, so `import extract … && import tim …`
//! converts one of the disc's textures.
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

mod car_model;
mod car_wheels;
mod course_textures;
mod engn;
mod gtfs;
mod iso9660;
mod menu;
mod overlay;
mod tim;
mod tro;
mod wsdb;

/// Where one output came from: written as `<output>.provenance.json`.
#[derive(Serialize)]
pub struct Provenance {
    /// The source file, as named on the disc or by `psx` extraction.
    pub source: String,
    pub source_size: u64,
    /// FNV-1a 64 of the source bytes (dependency-free; enough to notice a
    /// different disc revision).
    pub source_fnv1a64: String,
    pub importer: String,
}

impl Provenance {
    pub fn of(source: &Path, importer: &str) -> std::io::Result<Self> {
        let bytes = std::fs::read(source)?;
        Ok(Self {
            source: source.display().to_string(),
            source_size: bytes.len() as u64,
            source_fnv1a64: format!("{:016x}", fnv1a64(&bytes)),
            importer: importer.to_string(),
        })
    }

    /// The same, but records `name` (the file's name on the disc) as the
    /// source instead of a local extraction path, so a baked asset's
    /// provenance stays meaningful.
    pub fn of_named(source: &Path, name: &str, importer: &str) -> std::io::Result<Self> {
        let mut provenance = Self::of(source, importer)?;
        provenance.source = name.to_string();
        Ok(provenance)
    }

    /// Writes this next to `output`.
    pub fn write_beside(&self, output: &Path) -> std::io::Result<PathBuf> {
        let mut name = output.file_name().unwrap_or_default().to_os_string();
        name.push(".provenance.json");
        let path = output.with_file_name(name);
        std::fs::write(&path, serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)?;
        Ok(path)
    }
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// The disc file table, as printed by `import isofs`.
#[derive(Serialize)]
struct DiscIndex {
    files: Vec<DiscFile>,
}

#[derive(Serialize)]
struct DiscFile {
    name: String,
    lba: u32,
    bytes: u32,
}

/// The volume index written by `import volume`.
#[derive(Serialize)]
struct VolumeIndex {
    provenance: Provenance,
    /// The 8-byte tag at header offset 8.
    tag: String,
    /// The number of resources: one less than the number of offset entries.
    resources: usize,
    /// Bytes of the volume before the directory records: the header and the
    /// whole entry table (offset `0x10` to `offset_table_bytes`).
    offset_table_bytes: u32,
    /// The entry table's sentinel, equal to the volume's size in bytes.
    volume_bytes: u32,
    /// The directory records, in table order. A record's position in this list
    /// is the `ordinal` a directory record points at.
    records: Vec<VolumeRecord>,
}

#[derive(Serialize)]
struct VolumeRecord {
    ordinal: u16,
    /// Bitfield: `0x01` a directory node follows at `ordinal`, `0x80` the last
    /// record of its directory.
    flags: u8,
    key: String,
    /// `true` for records with the directory bit.
    directory: bool,
    name: String,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["provenance", source, output] => {
            let written = Provenance::of(Path::new(source), "provenance")
                .and_then(|p| p.write_beside(Path::new(output)))
                .unwrap_or_else(|e| fail(&e.to_string()));
            println!("{}", written.display());
        }
        ["isofs", image] => {
            let mut reader = iso9660::Reader::open(Path::new(image)).unwrap_or_else(|e| fail(&e.to_string()));
            let entries = reader.files().unwrap_or_else(|e| fail(&e.to_string()));
            let index = DiscIndex {
                files: entries.into_iter().map(|e| DiscFile { name: e.name, lba: e.lba, bytes: e.size }).collect(),
            };
            println!("{}", serde_json::to_string_pretty(&index).unwrap());
        }
        ["volume", image, "--out", out] => match import_volume(Path::new(image), Path::new(out)) {
            Ok(report) => println!("{report}"),
            Err(e) => fail(&e.to_string()),
        },
        ["extract", image, rest @ ..] => {
            let id = flag(rest, "--id").map(|v| v.parse::<u32>().unwrap_or_else(|e| fail(&format!("--id: {e}"))));
            let out = flag(rest, "--out").unwrap_or_else(|| fail("extract needs --out <file>"));
            match import_extract(Path::new(image), id, flag(rest, "--path"), Path::new(out)) {
                Ok(report) => println!("{report}"),
                Err(e) => fail(&e.to_string()),
            }
        }
        ["tim", input, "--out", out] => match import_tim(Path::new(input), Path::new(out)) {
            Ok(report) => println!("{report}"),
            Err(e) => fail(&e.to_string()),
        },
        ["tims", image, "--out", dir] => match import_tims(Path::new(image), Path::new(dir)) {
            Ok(report) => println!("{report}"),
            Err(e) => fail(&e.to_string()),
        },
        ["text", input, "--out", out] => match import_text(Path::new(input), Path::new(out)) {
            Ok(report) => println!("{report}"),
            Err(e) => fail(&e.to_string()),
        },
        ["engen", input, "--out", out] => match import_engen(Path::new(input), Path::new(out)) {
            Ok(report) => println!("{report}"),
            Err(e) => fail(&e.to_string()),
        },
        ["engines", image, rest @ ..] => {
            let out = flag(rest, "--out").unwrap_or_else(|| fail("engines needs --out <dir>"));
            match import_engines(Path::new(image), Path::new(out), flag(rest, "--match")) {
                Ok(report) => println!("{report}"),
                Err(e) => fail(&e.to_string()),
            }
        }
        ["menu", dat, rest @ ..] => {
            let idx = flag(rest, "--idx").unwrap_or_else(|| fail("menu needs --idx <idx>"));
            match menu::inspect_files(Path::new(dat), Path::new(idx)) {
                Ok(report) => println!("{report}"),
                Err(e) => fail(&e.to_string()),
            }
        }
        ["overlays", image, rest @ ..] => {
            let out = flag(rest, "--out").unwrap_or_else(|| fail("overlays needs --out <dir>"));
            match import_overlays(Path::new(image), Path::new(out)) {
                Ok(report) => println!("{report}"),
                Err(e) => fail(&e.to_string()),
            }
        }
        ["tro", input, rest @ ..] => {
            let out = flag(rest, "--out").unwrap_or_else(|| fail("tro needs --out <file.ron>"));
            let default_name = Path::new(input).file_name().unwrap_or_default().to_string_lossy().into_owned();
            match tro::import(Path::new(input), Path::new(out), flag(rest, "--name").unwrap_or(&default_name)) {
                Ok(report) => println!("{report}"),
                Err(e) => fail(&e.to_string()),
            }
        }
        ["tro-scene", input, rest @ ..] => {
            let out = flag(rest, "--out").unwrap_or_else(|| fail("tro-scene needs --out <scene.ron>"));
            let textures = flag(rest, "--textures").unwrap_or_else(|| fail("tro-scene needs --textures <file.trp>"));
            let prefix = flag(rest, "--texture-prefix")
                .unwrap_or_else(|| fail("tro-scene needs --texture-prefix <asset path to scene folder>"));
            match tro::render::import(Path::new(input), Path::new(textures), Path::new(out), prefix) {
                Ok(report) => println!("{report}"),
                Err(e) => fail(&e.to_string()),
            }
        }
        ["wheels", rest @ ..] => {
            let ram = flag(rest, "--ram").unwrap_or_else(|| fail("wheels needs --ram <checkpoint ram.bin>"));
            let textures = flag(rest, "--textures").unwrap_or_else(|| fail("wheels needs --textures <file.cdp>"));
            let out = flag(rest, "--out").unwrap_or_else(|| fail("wheels needs --out <directory>"));
            let prefix =
                flag(rest, "--texture-prefix").unwrap_or_else(|| fail("wheels needs --texture-prefix <asset folder>"));
            let paint = flag(rest, "--paint").unwrap_or("5").parse().unwrap_or_else(|_| fail("invalid paint index"));
            match car_wheels::import(Path::new(ram), Path::new(textures), Path::new(out), paint, prefix) {
                Ok(report) => println!("{report}"),
                Err(error) => fail(&error),
            }
        }
        ["car", input, rest @ ..] => {
            let out = flag(rest, "--out").unwrap_or_else(|| fail("car needs --out <model.ron>"));
            let textures = flag(rest, "--textures").unwrap_or_else(|| fail("car needs --textures <file.cdp>"));
            let prefix =
                flag(rest, "--texture-prefix").unwrap_or_else(|| fail("car needs --texture-prefix <asset folder>"));
            let paint = flag(rest, "--paint").unwrap_or("0").parse().unwrap_or_else(|_| fail("invalid paint index"));
            let lod = flag(rest, "--lod").unwrap_or("0").parse().unwrap_or_else(|_| fail("invalid LOD index"));
            match car_model::import(Path::new(input), Path::new(textures), Path::new(out), paint, lod, prefix) {
                Ok(report) => println!("{report}"),
                Err(e) => fail(&e),
            }
        }
        _ => fail(
            "usage: import provenance <source> <output>\n       import isofs <image>\n       import volume <image> --out <json>\n       import extract <image> (--id <n> | --path <path>) --out <file>\n       import tim <file> --out <png>\n       import tims <image> --out <dir>\n       import text <file> --out <ron>\n       import engen <file.es> --out <wav>\n       import engines <image> --out <dir> [--match <substring>]\n       import menu <dat> --idx <idx>\n       import overlays <image> --out <dir>\n       import tro <file.tro> --out <ron> [--name <disc path>]",
        ),
    }
}

/// The value after `name` in `args`, if present.
fn flag<'a>(args: &[&'a str], name: &str) -> Option<&'a str> {
    args.iter().position(|a| *a == name).and_then(|at| args.get(at + 1)).copied()
}

/// Writes the volume index and its provenance. Returns a one-line summary.
fn import_volume(image: &Path, out: &Path) -> std::io::Result<String> {
    let (_, _, volume, volume_bytes) = open_volume(image)?;
    // Each directory's records form a run ending with the `0x80` bit; records
    // with the `0x01` bit name a directory node.
    let directory_entries = volume.records.iter().filter(|r| r.is_directory()).count();
    let runs = volume.records.iter().filter(|r| r.is_last()).count();
    // Structural check: the entry table tiles the volume, so every resource's
    // sector range is ordered and inside it.
    let mut empty = 0usize;
    for id in 0..volume.resources() {
        let (start, end) = volume.range(id).expect("id < resources");
        let size = volume.size(id).expect("id < resources");
        assert!(start <= end && end <= volume_bytes, "resource {id} range {start:#x}..{end:#x}");
        assert_eq!(size, end - volume.offsets[id], "resource {id} size disagrees with its entry");
        empty += usize::from(size == 0);
    }
    let index = VolumeIndex {
        provenance: Provenance::of(&disc_path(image)?, "import volume")?,
        tag: format!("{:016x}", volume.tag),
        resources: volume.resources(),
        offset_table_bytes: 0x10 + volume.resources() as u32 * 4 + 4,
        volume_bytes,
        records: volume
            .records
            .iter()
            .map(|r| VolumeRecord {
                ordinal: r.ordinal,
                flags: r.flags,
                key: format!("{:08x}", r.key),
                directory: r.is_directory(),
                name: r.name.clone(),
            })
            .collect(),
    };
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(out, serde_json::to_vec_pretty(&index).map_err(std::io::Error::other)?)?;
    index.provenance.write_beside(out)?;
    Ok(format!(
        "{}: {} resources ({} empty), {} records ({} directory entries in {} runs)",
        out.display(),
        index.resources,
        empty,
        index.records.len(),
        directory_entries,
        runs
    ))
}

/// Writes one volume resource exactly as stored. Returns a one-line summary.
fn import_extract(image: &Path, id: Option<u32>, path: Option<&str>, out: &Path) -> std::io::Result<String> {
    let (mut reader, base, volume, _) = open_volume(image)?;
    let (id, what) = match (id, path) {
        (Some(id), _) => (id as usize, format!("id {id}")),
        (None, Some(path)) => {
            let found =
                volume.find(path).ok_or_else(|| std::io::Error::other(format!("{path} is not in the volume")))?;
            (found as usize, format!("/{} (id {found})", path.trim_matches('/')))
        }
        (None, None) => return Err(std::io::Error::other("extract needs --id or --path")),
    };
    let (start, end) = volume.range(id).ok_or_else(|| std::io::Error::other(format!("id {id} is out of range")))?;
    let size = volume.size(id).expect("id < resources");
    let mut bytes = vec![0u8; size as usize];
    reader.read_user(base + u64::from(start), &mut bytes)?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(out, &bytes)?;
    Provenance::of(&disc_path(image)?, "import extract")?.write_beside(out)?;
    Ok(format!(
        "{}: {size} bytes from {what}; {:#x}..{:#x} ({} pad)",
        out.display(),
        start,
        end,
        volume.pad(id).unwrap()
    ))
}

/// Converts one TIM (raw or gzipped, as `import extract` writes it) to a PNG.
/// Returns a one-line summary.
fn import_tim(input: &Path, out: &Path) -> std::io::Result<String> {
    use std::io::Read as _;
    let stored = std::fs::read(input)?;
    let (bytes, gzipped) = if stored.starts_with(&[0x1f, 0x8b]) {
        let mut inflated = Vec::new();
        flate2::read::GzDecoder::new(&stored[..]).read_to_end(&mut inflated)?;
        (inflated, true)
    } else {
        (stored, false)
    };
    let tim = tim::Tim::parse(&bytes).map_err(std::io::Error::other)?;
    let (width, height, rgba) = tim.to_rgba().map_err(std::io::Error::other)?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = std::fs::File::create(out)?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
    writer.write_image_data(&rgba).map_err(std::io::Error::other)?;
    writer.finish().map_err(std::io::Error::other)?;
    let name = input.file_name().unwrap_or_default().to_string_lossy();
    Provenance::of_named(input, &name, "import tim")?.write_beside(out)?;
    let palette =
        tim.clut.as_ref().map(|c| format!(", CLUT {}x{} at VRAM ({}, {})", c.w, c.h, c.x, c.y)).unwrap_or_default();
    let missing = if tim.has_palette() { "" } else { ", no CLUT in the file: indices shown as grey" };
    Ok(format!(
        "{}: {}x{} {:?}, image at VRAM ({}, {}){}{} from {}{}; the TIM header accounts for {} of its {} bytes",
        out.display(),
        width,
        height,
        tim.mode,
        tim.image.x,
        tim.image.y,
        palette,
        missing,
        input.display(),
        if gzipped { " (gzip)" } else { "" },
        tim.declared_len,
        bytes.len()
    ))
}

/// Converts every TIM in the volume to PNG, reporting what did not convert.
/// Returns a one-line-per-group summary.
fn import_tims(image: &Path, out_dir: &Path) -> std::io::Result<String> {
    use std::io::Read as _;
    let (mut reader, base, volume, _) = open_volume(image)?;
    std::fs::create_dir_all(out_dir)?;
    let mut seen = 0usize;
    let mut compressed = 0usize;
    let mut converted = 0usize;
    let mut without_palette = 0usize;
    let mut failures: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for (path, id) in volume.paths() {
        let id = id as usize;
        let (start, _) = volume.range(id).expect("id < resources");
        let size = volume.size(id).expect("id < resources");
        if size < 4 {
            continue;
        }
        let mut bytes = vec![0u8; size as usize];
        reader.read_user(base + u64::from(start), &mut bytes)?;
        if bytes.starts_with(&[0x1f, 0x8b]) {
            compressed += 1;
            let mut inflated = Vec::new();
            flate2::read::GzDecoder::new(&bytes[..]).read_to_end(&mut inflated)?;
            bytes = inflated;
        }
        if !bytes.starts_with(&[0x10, 0x00, 0x00, 0x00]) {
            continue;
        }
        seen += 1;
        let parsed = tim::Tim::parse(&bytes).and_then(|tim| {
            let has_palette = tim.has_palette();
            let (w, h, rgba) = tim.to_rgba()?;
            Ok((w, h, rgba, has_palette))
        });
        match parsed {
            Ok((w, h, rgba, has_palette)) => {
                let name = path.trim_start_matches('/').replace('/', "_");
                let file = std::fs::File::create(out_dir.join(format!("{name}.png")))?;
                let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
                writer.write_image_data(&rgba).map_err(std::io::Error::other)?;
                writer.finish().map_err(std::io::Error::other)?;
                converted += 1;
                if !has_palette {
                    without_palette += 1;
                }
            }
            Err(why) => {
                // Group by the message with its numbers blanked out.
                let shape: String = why.chars().map(|c| if c.is_ascii_digit() { '#' } else { c }).collect();
                *failures.entry(shape).or_default() += 1;
            }
        }
    }
    let mut report = format!(
        "{}: {seen} TIMs ({compressed} gzipped), {converted} converted, {} failed",
        out_dir.display(),
        seen - converted
    );
    if without_palette > 0 {
        report.push_str(&format!(
            "\n  {without_palette} of them carry no CLUT: the game uploads the palette, so the PNG is a grey ramp of the indices"
        ));
    }
    for (shape, count) in failures {
        report.push_str(&format!("\n  {count:5}  {shape}"));
    }
    Ok(report)
}

/// Converts one `WSDB` table (raw or gzipped, as `import extract` writes it) to
/// the engine's RON string table. Returns a one-line summary.
fn import_text(input: &Path, out: &Path) -> std::io::Result<String> {
    use std::io::Read as _;
    let stored = std::fs::read(input)?;
    let (bytes, gzipped) = if stored.starts_with(&[0x1f, 0x8b]) {
        let mut inflated = Vec::new();
        flate2::read::GzDecoder::new(&stored[..]).read_to_end(&mut inflated)?;
        (inflated, true)
    } else {
        (stored, false)
    };
    let table = wsdb::TextTable::parse(&bytes).map_err(std::io::Error::other)?;
    let ron = ron::ser::to_string_pretty(&table, ron::ser::PrettyConfig::default()).map_err(std::io::Error::other)?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(out, ron.as_bytes())?;
    let name = input.file_name().unwrap_or_default().to_string_lossy();
    Provenance::of_named(input, &name, "import text")?.write_beside(out)?;
    let longest = table.strings.iter().map(|s| s.chars().count()).max().unwrap_or(0);
    Ok(format!(
        "{}: {} strings, longest {longest} chars, from {}{}; every byte of the {} accounted for",
        out.display(),
        table.strings.len(),
        input.display(),
        if gzipped { " (gzip)" } else { "" },
        bytes.len()
    ))
}

/// Decodes one `ENGN` sample (raw, as `import extract` writes it) to a WAV.
/// Returns a one-line summary.
fn import_engen(input: &Path, out: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(input)?;
    let engn = engn::Engn::parse(&bytes).map_err(std::io::Error::other)?;
    let pcm = engn::adpcm_to_pcm(engn.payload);
    let wav = engn::wav(&pcm, engn::SAMPLE_RATE);
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(out, &wav)?;
    let name = input.file_name().unwrap_or_default().to_string_lossy();
    Provenance::of_named(input, &name, "import engen")?.write_beside(out)?;
    Ok(format!(
        "{}: {} frames, {} samples at {} Hz ({:.3} s), {} zones, header {} bytes; {} of {} source bytes are padding",
        out.display(),
        engn.payload_size / 16,
        engn.samples(),
        engn::SAMPLE_RATE,
        engn.samples() as f64 / f64::from(engn::SAMPLE_RATE),
        engn.zones,
        engn.header_size,
        bytes.len() as u64 - u64::from(engn.header_size) - u64::from(engn.payload_size),
        bytes.len()
    ))
}

/// Decodes every `ENGN` sound in the volume to WAV, deduplicating identical
/// payloads, and writes the engine's sound table beside them. `filter` keeps
/// only paths containing it. Returns a one-line summary.
fn import_engines(image: &Path, out_dir: &Path, filter: Option<&str>) -> std::io::Result<String> {
    let (mut reader, base, volume, _) = open_volume(image)?;
    std::fs::create_dir_all(out_dir)?;
    let mut table = engn::SoundTable { sample_rate: engn::SAMPLE_RATE, samples: Vec::new(), files: Vec::new() };
    // Payload FNV-1a 64 -> index into `samples`: many resources share one loop
    // (most `_n*`/`_t*` slots hold the same 441 Hz placeholder), so the WAVs
    // are written once.
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut scanned = 0usize;
    let mut rejected = 0usize;
    let mut padding = 0u64;
    for (path, id) in volume.paths() {
        if !path.starts_with("/engine/") || !path.ends_with(".es") {
            continue;
        }
        if filter.is_some_and(|needle| !path.contains(needle)) {
            continue;
        }
        let id = id as usize;
        let (start, _) = volume.range(id).expect("id < resources");
        let size = volume.size(id).expect("id < resources");
        let mut bytes = vec![0u8; size as usize];
        reader.read_user(base + u64::from(start), &mut bytes)?;
        let Ok(engn) = engn::Engn::parse(&bytes) else {
            rejected += 1;
            continue;
        };
        // Structural check: the header, payload and the rest of the resource
        // are the whole file, so nothing is left unaccounted for.
        padding += size as u64 - u64::from(engn.header_size) - u64::from(engn.payload_size);
        scanned += 1;
        let key = format!("{:016x}", fnv1a64(engn.payload));
        let sample = match seen.get(&key) {
            Some(&existing) => existing,
            None => {
                let index = table.samples.len();
                let file = format!("sample_{index:04}.wav");
                let pcm = engn::adpcm_to_pcm(engn.payload);
                std::fs::write(out_dir.join(&file), engn::wav(&pcm, engn::SAMPLE_RATE))?;
                table.samples.push(engn::Sample { file, frames: engn.payload_size / 16, source_fnv1a64: key.clone() });
                seen.insert(key, index);
                index
            }
        };
        table.files.push(engn::SoundFile { name: path, sample, zones: engn.zones });
    }
    let ron = ron::ser::to_string_pretty(&table, ron::ser::PrettyConfig::default()).map_err(std::io::Error::other)?;
    std::fs::write(out_dir.join("table.ron"), ron.as_bytes())?;
    Provenance::of(&disc_path(image)?, "import engines")?.write_beside(&out_dir.join("table.ron"))?;
    let seconds: u64 =
        table.samples.iter().map(|s| u64::from(s.frames) * 28).sum::<u64>() / u64::from(engn::SAMPLE_RATE);
    let mut report = format!(
        "{}: {} sounds ({} distinct samples, {seconds} s) at {} Hz, {padding} bytes of resource padding",
        out_dir.display(),
        table.files.len(),
        table.samples.len(),
        engn::SAMPLE_RATE
    );
    if rejected > 0 {
        report.push_str(&format!(
            "\n  {rejected} of {} resources did not parse as ENGN and were skipped",
            scanned + rejected
        ));
    }
    Ok(report)
}

/// The overlay container index written by `import overlays`.
#[derive(Serialize)]
struct OverlayIndex {
    provenance: Provenance,
    /// The disc file the container came from.
    source: String,
    header_size: u32,
    /// Every image is position-dependent code linked at this address.
    load_address: u32,
    padding_bytes: usize,
    image_bytes: usize,
    members: Vec<OverlayEntry>,
}

#[derive(Serialize)]
struct OverlayEntry {
    index: usize,
    /// Byte offset of the gzip member in the container.
    offset: usize,
    compressed_size: usize,
    image_size: usize,
    image_fnv1a64: String,
    /// The first word of the image, to tell whole-program overlays apart.
    first_word: String,
    /// `true` if the declared member ran past the end of the container.
    truncated: bool,
}

/// Splits `GT2.OVL` into its gzip members and writes one image per overlay,
/// with an index. Returns a one-line summary.
fn import_overlays(image: &Path, out_dir: &Path) -> std::io::Result<String> {
    let mut reader = iso9660::Reader::open(image)?;
    let entry = reader
        .files()?
        .into_iter()
        .find(|e| e.name == "GT2.OVL")
        .ok_or_else(|| std::io::Error::other("GT2.OVL not on this disc"))?;
    let mut bytes = vec![0u8; entry.size as usize];
    reader.read_user(u64::from(entry.lba) * iso9660::SECTOR_DATA, &mut bytes)?;
    let container = overlay::Overlay::parse(&bytes).map_err(std::io::Error::other)?;
    std::fs::create_dir_all(out_dir)?;
    let mut index = OverlayIndex {
        // Provenance is of the container itself, not the 691 MB disc image.
        provenance: Provenance {
            source: entry.name.clone(),
            source_size: bytes.len() as u64,
            source_fnv1a64: format!("{:016x}", fnv1a64(&bytes)),
            importer: "import overlays".to_string(),
        },
        source: entry.name.clone(),
        header_size: container.header_size,
        load_address: overlay::LOAD_ADDRESS,
        padding_bytes: container.padding_bytes,
        image_bytes: container.image_bytes(),
        members: Vec::with_capacity(container.members.len()),
    };
    for member in &container.members {
        let file = format!("overlay_{:02}.bin", member.index);
        std::fs::write(out_dir.join(&file), &member.image)?;
        let mut word = [0u8; 4];
        let n = member.image.len().min(4);
        word[..n].copy_from_slice(&member.image[..n]);
        index.members.push(OverlayEntry {
            index: member.index,
            offset: member.offset,
            compressed_size: member.compressed_size,
            image_size: member.image.len(),
            image_fnv1a64: format!("{:016x}", fnv1a64(&member.image)),
            first_word: format!("{:08x}", u32::from_le_bytes(word)),
            truncated: member.truncated,
        });
    }
    std::fs::write(out_dir.join("manifest.json"), serde_json::to_vec_pretty(&index).map_err(std::io::Error::other)?)?;
    index.provenance.write_beside(&out_dir.join("manifest.json"))?;
    Ok(format!(
        "{}: {} overlays, {} bytes of image ({} of table and alignment padding), linked at {:#x}",
        out_dir.display(),
        index.members.len(),
        index.image_bytes,
        index.padding_bytes,
        index.load_address
    ))
}

/// Opens the disc and parses `GT2.VOL`, returning the reader, the volume's byte
/// offset in the user data stream, the parsed volume and its size.
fn open_volume(image: &Path) -> std::io::Result<(iso9660::Reader, u64, gtfs::Volume, u32)> {
    let mut reader = iso9660::Reader::open(image)?;
    let entry = reader
        .files()?
        .into_iter()
        .find(|e| e.name == "GT2.VOL")
        .ok_or_else(|| std::io::Error::other("GT2.VOL not on this disc"))?;
    let base = u64::from(entry.lba) * iso9660::SECTOR_DATA;
    let volume = gtfs::Volume::parse(&mut reader, base, entry.size)?;
    Ok((reader, base, volume, entry.size))
}

/// The disc image the cue points at, for provenance; the image itself when it
/// is not a cue.
fn disc_path(image: &Path) -> std::io::Result<PathBuf> {
    if !image.extension().is_some_and(|e| e.eq_ignore_ascii_case("cue")) {
        return Ok(image.to_path_buf());
    }
    let text = std::fs::read_to_string(image)?;
    Ok(text
        .lines()
        .find_map(|l| l.trim().strip_prefix("FILE").and_then(|r| r.find('"').map(|s| &r[s + 1..])))
        .and_then(|r| r.find('"').map(|e| image.parent().unwrap_or(Path::new(".")).join(&r[..e])))
        .unwrap_or_else(|| image.to_path_buf()))
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2)
}

#[cfg(test)]
mod tests {
    #[test]
    fn fnv1a64_matches_reference_values() {
        assert_eq!(super::fnv1a64(b""), 0xcbf29ce484222325);
        assert_eq!(super::fnv1a64(b"a"), 0xaf63dc4c8601ec8c);
    }
}
