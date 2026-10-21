//! ISO9660 reader for the original disc image.
//!
//! The disc is one data track in `mode2_raw` form (2352-byte sectors, 24-byte
//! subheader, 2048 bytes of user data). [`Reader`] hides that: every read is
//! addressed in the *user data* stream, so the same code reads a raw `.bin` or
//! a cooked `.iso`.
//!
//! Only what the importers need is implemented: the primary volume descriptor
//! and the root directory. The file table this produces is the same one the
//! original builds in RAM, so it doubles as a structural check
//! (`docs/evidence.md`).
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// User data bytes per sector.
pub const SECTOR_DATA: u64 = 2048;
/// Bytes of a raw mode-2 sector, including headers.
const SECTOR_RAW: u64 = 2352;
/// Offset of user data inside a raw mode-2 sector.
const RAW_DATA_OFFSET: u64 = 24;
/// Sector holding the volume descriptors.
const PVD_LBA: u64 = 16;

/// One file: the name without its `;1` version suffix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub lba: u32,
    pub size: u32,
}

pub struct Reader {
    file: File,
    /// Bytes per physical sector in the image (2352 raw, 2048 cooked).
    sector: u64,
    /// Offset of user data inside a sector (24 raw, 0 cooked).
    user: u64,
    /// Total bytes of user data addressable.
    pub user_len: u64,
}

impl Reader {
    /// Opens a `.iso`, raw `.bin` or `.cue` (whose `FILE` line names the
    /// image). The layout is detected from the "CD001" identifier, so a
    /// mislabelled extension still works.
    pub fn open(path: &Path) -> io::Result<Self> {
        let path = resolve_cue(path)?;
        let mut file = File::open(&path)?;
        let len = file.metadata()?.len();
        for &(sector, user) in &[(SECTOR_RAW, RAW_DATA_OFFSET), (SECTOR_DATA, 0)] {
            let probe = sector * PVD_LBA + user + 1;
            if len >= probe + 5 {
                let mut id = [0u8; 5];
                file.seek(SeekFrom::Start(probe))?;
                file.read_exact(&mut id)?;
                if &id == b"CD001" {
                    let user_len = if sector == SECTOR_DATA { len } else { (len / sector) * SECTOR_DATA };
                    return Ok(Self { file, sector, user, user_len });
                }
            }
        }
        Err(io::Error::new(io::ErrorKind::InvalidData, format!("{}: no ISO9660 volume descriptor", path.display())))
    }

    /// Reads `buf.len()` bytes starting at user-data offset `offset`, across
    /// sector boundaries.
    pub fn read_user(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let end = offset + buf.len() as u64;
        if end > self.user_len {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "read past the image"));
        }
        let mut done = 0usize;
        while done < buf.len() {
            let at = offset + done as u64;
            let sector_index = at / SECTOR_DATA;
            let within = at % SECTOR_DATA;
            let take = ((SECTOR_DATA - within) as usize).min(buf.len() - done);
            let physical = sector_index * self.sector + self.user + within;
            self.file.seek(SeekFrom::Start(physical))?;
            self.file.read_exact(&mut buf[done..done + take])?;
            done += take;
        }
        Ok(())
    }

    fn read_sector(&mut self, lba: u32) -> io::Result<[u8; SECTOR_DATA as usize]> {
        let mut buf = [0u8; SECTOR_DATA as usize];
        self.read_user(u64::from(lba) * SECTOR_DATA, &mut buf)?;
        Ok(buf)
    }

    /// The primary volume descriptor's root directory: its extent and length.
    fn root_directory(&mut self) -> io::Result<(u32, u32)> {
        let sector = self.read_sector(PVD_LBA as u32)?;
        if sector[0] != 1 || &sector[1..6] != b"CD001" {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "sector 16 is not a primary volume descriptor"));
        }
        let root = &sector[156..190];
        let lba = le32(&root[2..6]);
        let size = le32(&root[10..14]);
        Ok((lba, size))
    }

    /// Every file in the root directory, in disc order.
    pub fn files(&mut self) -> io::Result<Vec<Entry>> {
        let (root_lba, root_size) = self.root_directory()?;
        let mut dir = vec![0u8; root_size as usize];
        self.read_user(u64::from(root_lba) * SECTOR_DATA, &mut dir)?;
        let mut entries = Vec::new();
        let mut at = 0usize;
        while at < dir.len() {
            let record_len = dir[at] as usize;
            if record_len == 0 {
                // Zero length jumps to the next sector in the directory.
                at = (at / SECTOR_DATA as usize + 1) * SECTOR_DATA as usize;
                continue;
            }
            let record = &dir[at..at + record_len];
            let flags = record[25];
            let name_len = record[32] as usize;
            let raw_name = &record[33..33 + name_len];
            let is_dir = flags & 0x02 != 0;
            if !is_dir && raw_name != [0] && raw_name != [1] {
                let text = String::from_utf8_lossy(raw_name).into_owned();
                entries.push(Entry {
                    name: text.split(';').next().unwrap_or("").to_string(),
                    lba: le32(&record[2..6]),
                    size: le32(&record[10..14]),
                });
            }
            at += record_len;
        }
        Ok(entries)
    }
}

fn le32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[..4].try_into().unwrap())
}

/// A `.cue` names its image on a `FILE "..."` line, relative to the cue.
fn resolve_cue(path: &Path) -> io::Result<PathBuf> {
    let is_cue = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("cue"));
    if !is_cue {
        return Ok(path.to_path_buf());
    }
    let text = std::fs::read_to_string(path)?;
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("FILE") else { continue };
        let Some(start) = rest.find('"') else { continue };
        let Some(end) = rest[start + 1..].find('"') else { continue };
        let name = &rest[start + 1..start + 1 + end];
        let image = path.parent().unwrap_or(Path::new(".")).join(name);
        return Ok(image);
    }
    Err(io::Error::new(io::ErrorKind::InvalidData, format!("{}: no FILE line", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The disc's own file table, as the original stores it in RAM
    /// (`0x80009224`, see `docs/evidence.md`): LBA and size come from runtime
    /// evidence, not from this parser.
    pub const RAM_FILE_TABLE: &[(&str, u32, u32)] = &[
        ("FAULTY.PSX", 280504, 27_648_000),
        ("GT2.OVL", 331, 289_752),
        ("GT2.VOL", 473, 488_241_152),
        ("MUSIC.DAT", 238_872, 85_262_336),
        ("SCUS_944.88", 24, 628_736),
        ("SYSTEM.CNF", 23, 68),
    ];

    /// Reads the file table from a real image when one is present; the disc is
    /// an input, not part of the build.
    fn real_image() -> Option<PathBuf> {
        let candidates = ["extracted/Gran Turismo 2 (USA) (Simulation Mode) (Rev 2).cue"];
        candidates.iter().map(PathBuf::from).find(|p| p.exists())
    }

    #[test]
    fn file_table_matches_the_runtime_evidence() {
        let Some(image) = real_image() else {
            eprintln!("skipped: disc image not present");
            return;
        };
        let mut reader = Reader::open(&image).unwrap();
        let mut found: Vec<(String, u32, u32)> =
            reader.files().unwrap().into_iter().map(|e| (e.name, e.lba, e.size)).collect();
        assert_eq!(found.len(), RAM_FILE_TABLE.len(), "entry count");
        for (name, lba, size) in RAM_FILE_TABLE {
            let hit = found.iter().position(|f| f.0 == *name).expect(name);
            let (_, got_lba, got_size) = found.remove(hit);
            assert_eq!((got_lba, got_size), (*lba, *size), "{name}");
        }
    }

    #[test]
    fn cue_files_resolve_against_the_cue_directory() {
        let dir = std::env::temp_dir().join("import-cue-test");
        std::fs::create_dir_all(&dir).unwrap();
        let cue = dir.join("game.cue");
        std::fs::write(&cue, "FILE \"image.bin\" BINARY\n  TRACK 01 MODE2/2352\n").unwrap();
        assert_eq!(resolve_cue(&cue).unwrap(), dir.join("image.bin"));
        assert_eq!(resolve_cue(Path::new("x.iso")).unwrap(), PathBuf::from("x.iso"));
    }
}
