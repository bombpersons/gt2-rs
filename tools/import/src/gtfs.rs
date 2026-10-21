//! `GTFS`: the filesystem of `GT2.VOL`, the disc's main volume.
//!
//! Structure, from the volume's own bytes and the copy the original keeps in
//! RAM (`0x801e35f0`, verified against a runtime RAM dump; see
//! `docs/engine-model.md` and `docs/evidence.md`):
//!
//! ```text
//! 0x000  "GTFS"          magic
//! 0x004  u32             0 in every observed header
//! 0x008  u64             tag (0x0000_0000_2d64_2d3d observed)
//! 0x010  u32 entries[N]  resource descriptors, one per resource
//! 0x...  u32 sentinel    equal to the volume's size
//! 0x...  zero padding
//! 0x...  records[]       32-byte directory records (resource 1 from 0xB800)
//! ```
//!
//! The table is self-terminating: entries never decrease and the last one
//! equals the volume's byte length, so `N` (the resource count) is found by
//! reading until that sentinel. There is **11580** of them here.
//!
//! An entry packs two fields:
//!
//! ```text
//! entry >> 11     sector: resource data starts at entry & !0x7ff
//! entry & 0x7ff   pad: bytes of padding after the data in its last sector
//! ```
//!
//! so resource `id`'s data is `[entry[id] & !0x7ff, entry[id + 1] & !0x7ff)` —
//! sector-aligned, as a CD read must be — and its true length is that span
//! minus `entry[id] & 0x7ff`. This is exactly what the original's loader does
//! (`fn_8005D74C` returns `entry >> 11`, `fn_8005D79C` returns
//! `(entry[id+1] & !0x7ff) - entry[id]`). Verified on the disc: all 3576
//! `.tim` resources start with the TIM magic, all 5114 `.gz` with the gzip
//! magic and all 2747 `.es` with `ENGN` at the sector start, every one of the
//! 5114 gzip streams ends within its span, and for each of them the span minus
//! this entry's low bits is exactly the gzip stream's length.
//!
//! Every record is 32 bytes:
//!
//! ```text
//! 0x00  u32 key        per-record key; 0x385f8dcf for every ".." entry
//! 0x04  u16 ordinal    record index (from the table's start) of the node
//! 0x06  u8  flags      bitfield: 0x01 a directory node follows, 0x80 last
//! 0x07  char name[25]  NUL-padded ASCII
//! ```
//!
//! Record 0 is the root node's first entry; a directory's entries are a run of
//! records terminated by one with the 0x80 bit. This is a real directory tree:
//! the boot executable carries the paths it loads (`/arcade/arc_font.tim`,
//! `/carparam/usa_unistrdb.dat.gz`, `/sound/sys.ins`, …) and each resolves
//! through these records ([`Volume::find`]) to a resource id.
use crate::iso9660::Reader;
use std::io;

/// Bits in a record's flags byte.
pub const FLAG_DIRECTORY: u8 = 0x01;
pub const FLAG_LAST: u8 = 0x80;

/// Sector size the volume's resources are aligned to.
pub const SECTOR: u32 = 0x800;

/// A descriptor table and the resources it indexes.
#[derive(Debug)]
pub struct Volume {
    pub tag: u64,
    /// The raw table entries, plus the sentinel as the last element. Entry `id`
    /// packs resource `id`'s sector and padding (see the module docs).
    pub offsets: Vec<u32>,
    /// The directory records, in table order (a record's index in this vector
    /// is the `ordinal` a directory entry points at).
    pub records: Vec<Record>,
}

/// One 32-byte directory record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub key: u32,
    pub ordinal: u16,
    pub flags: u8,
    pub name: String,
}

impl Record {
    /// The record names a directory: a child node starts at `ordinal`.
    pub fn is_directory(&self) -> bool {
        self.flags & FLAG_DIRECTORY != 0
    }

    /// The record is the last of its directory's run.
    pub fn is_last(&self) -> bool {
        self.flags & FLAG_LAST != 0
    }
}

impl Volume {
    /// Parses the descriptor at `base` (the volume's first byte in the disc's
    /// user data stream); `len` is the volume's size in bytes.
    pub fn parse(reader: &mut Reader, base: u64, len: u32) -> io::Result<Self> {
        let bad = |message: String| io::Error::new(io::ErrorKind::InvalidData, message);
        let mut head = [0u8; 16];
        reader.read_user(base, &mut head)?;
        if &head[0..4] != b"GTFS" {
            return Err(bad(format!("not a GTFS volume: magic {:02x?}", &head[0..4])));
        }
        let tag = u64::from_le_bytes(head[8..16].try_into().unwrap());

        // The table runs until an entry equals the volume's size.
        const MAX_RESOURCES: usize = 1 << 17;
        let mut offsets = Vec::new();
        let mut at = 0x10u32;
        loop {
            if offsets.len() >= MAX_RESOURCES {
                return Err(bad("the entry table has no sentinel".into()));
            }
            let mut word = [0u8; 4];
            reader.read_user(base + u64::from(at), &mut word)?;
            let value = u32::from_le_bytes(word);
            at = at.checked_add(4).ok_or_else(|| bad("the entry table runs past the volume".into()))?;
            if value == len {
                break; // sentinel: one past the last resource
            }
            if value >= len {
                return Err(bad(format!("entry {value:#x} is past the volume ({len:#x})")));
            }
            if offsets.last().is_some_and(|&previous| previous > value) {
                return Err(bad("entries are not monotonic".into()));
            }
            offsets.push(value);
        }
        if offsets.is_empty() {
            return Err(bad("empty entry table".into()));
        }
        // Keep the sentinel so lengths are a difference of adjacent entries.
        offsets.push(len);

        // A resource's length is its sector span minus its own padding; it can
        // never be negative.
        for (id, pair) in offsets.windows(2).enumerate() {
            let (this, next) = (pair[0], pair[1]);
            let size = (next & !(SECTOR - 1)) as i64 - i64::from(this);
            if size < 0 {
                return Err(bad(format!("resource {id} is {size} bytes long")));
            }
        }

        // The records follow the table's zero padding and run to the end of the
        // second resource (its data starts after them).
        let table_end = 0x10 + (offsets.len() as u32 - 1) * 4;
        if offsets.len() < 3 {
            return Err(bad("volume has no record table".into()));
        }
        let scan_end = offsets[2];
        if scan_end <= table_end || scan_end > len {
            return Err(bad("record table range is implausible".into()));
        }
        let mut bytes = vec![0u8; (scan_end - table_end) as usize];
        let mut done = 0;
        while done < bytes.len() {
            let chunk = (bytes.len() - done).min(0x8000);
            let mut piece = vec![0u8; chunk];
            reader.read_user(base + u64::from(table_end) + done as u64, &mut piece)?;
            bytes[done..done + chunk].copy_from_slice(&piece);
            done += chunk;
        }

        // The table starts at the first position with three consecutive records.
        let base_at = (0..bytes.len().saturating_sub(96))
            .find(|&at| (0..3).all(|k| record_at(&bytes, at + k * 32).is_some()))
            .ok_or_else(|| bad("no directory records in the volume".into()))?;
        let mut records = Vec::new();
        while let Some(record) = record_at(&bytes, base_at + records.len() * 32) {
            records.push(record);
        }
        if records.len() < 2 {
            return Err(bad("directory record table is too short".into()));
        }

        Ok(Self { tag, offsets, records })
    }

    /// The number of resources (the table has one more entry).
    pub fn resources(&self) -> usize {
        self.offsets.len() - 1
    }

    /// The sector-aligned `[start, end)` byte range holding resource `id`'s
    /// data, padding included.
    pub fn range(&self, id: usize) -> Option<(u32, u32)> {
        let start = *self.offsets.get(id)? & !(SECTOR - 1);
        let end = *self.offsets.get(id + 1)? & !(SECTOR - 1);
        Some((start, end))
    }

    /// Resource `id`'s true length: its sector span minus the padding the
    /// entry records for it. This is the amount the original reads.
    pub fn size(&self, id: usize) -> Option<u32> {
        let (_, end) = self.range(id)?;
        Some(end - *self.offsets.get(id)?)
    }

    /// Padding bytes after resource `id`'s data in its last sector.
    pub fn pad(&self, id: usize) -> Option<u32> {
        Some(self.offsets.get(id)? & (SECTOR - 1))
    }

    /// The records of the node starting at `start`, through its last one.
    pub fn node(&self, start: usize) -> &[Record] {
        let rest = self.records.get(start..).unwrap_or(&[]);
        let end = rest.iter().position(Record::is_last).map_or(rest.len(), |at| at + 1);
        &rest[..end]
    }

    /// Resolves a volume path (`/arcade/arc_font.tim`, `carparam/usa_unistrdb.dat.gz`)
    /// to its resource id, the way the original does: match each component in a
    /// node's records, descend through directory records by ordinal.
    pub fn find(&self, path: &str) -> Option<u16> {
        let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
        let (last, parents) = components.split_last()?;
        let mut start = 0usize;
        for component in parents {
            let record = self.node(start).iter().find(|r| r.name == *component)?;
            if !record.is_directory() {
                return None;
            }
            start = record.ordinal as usize;
        }
        self.node(start).iter().find(|r| r.name == *last && !r.is_directory()).map(|r| r.ordinal)
    }

    /// Every file in the tree as `(path, id)`, in record order.
    pub fn paths(&self) -> Vec<(String, u16)> {
        let mut out = Vec::new();
        self.walk_paths(0, &mut String::new(), &mut out);
        out
    }

    fn walk_paths(&self, start: usize, prefix: &mut String, out: &mut Vec<(String, u16)>) {
        for record in self.node(start) {
            if record.name == ".." {
                continue;
            }
            let at = prefix.len();
            prefix.push('/');
            prefix.push_str(&record.name);
            if record.is_directory() {
                self.walk_paths(record.ordinal as usize, prefix, out);
            } else {
                out.push((prefix.clone(), record.ordinal));
            }
            prefix.truncate(at);
        }
    }
}

/// A 32-byte record at `at`, if the bytes there form one.
fn record_at(bytes: &[u8], at: usize) -> Option<Record> {
    let record = bytes.get(at..at + 32)?;
    let flags = record[6];
    if flags & !(FLAG_DIRECTORY | FLAG_LAST) != 0 {
        return None;
    }
    let name_bytes = &record[7..32];
    let end = name_bytes.iter().position(|&b| b == 0).unwrap_or(name_bytes.len());
    let name = &name_bytes[..end];
    if name.is_empty() || !name.iter().all(|&b| (0x20..0x7f).contains(&b)) {
        return None;
    }
    Some(Record {
        key: u32::from_le_bytes(record[0..4].try_into().unwrap()),
        ordinal: u16::from_le_bytes(record[4..6].try_into().unwrap()),
        flags,
        name: String::from_utf8_lossy(name).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic volume: the header, the record table, then two resources.
    ///
    /// ```text
    /// 0x0000 header + entry table {0x010, 0x800, 0x1008, 0x1800 (=len)}
    /// 0x0800 records (resource 1's sectors)
    /// ```
    fn volume_bytes() -> Vec<u8> {
        let len = 0x1800u32;
        let mut v = vec![0u8; len as usize];
        v[0..4].copy_from_slice(b"GTFS");
        v[8..16].copy_from_slice(&0x0000_0000_2d64_2d3du64.to_le_bytes());
        for (i, entry) in [0x010u32, 0x800, 0x1008, len].iter().enumerate() {
            v[0x10 + i * 4..0x14 + i * 4].copy_from_slice(&entry.to_le_bytes());
        }
        let mut record = |index: usize, key: u32, ordinal: u16, flags: u8, name: &str| {
            let at = 0x800 + index * 32;
            v[at..at + 4].copy_from_slice(&key.to_le_bytes());
            v[at + 4..at + 6].copy_from_slice(&ordinal.to_le_bytes());
            v[at + 6] = flags;
            v[at + 7..at + 7 + name.len()].copy_from_slice(name.as_bytes());
        };
        record(0, 0x385f_8dcf, 0, FLAG_DIRECTORY, "..");
        record(1, 0x385f_fab2, 2, 0, "license_info_uk");
        record(2, 0x384d_360e, 3, FLAG_DIRECTORY | FLAG_LAST, "arcade");
        // arcade's child node.
        record(3, 0x385f_8dcf, 3, FLAG_DIRECTORY, "..");
        record(4, 0x385f_fab2, 4, 0, "arc_font.tim");
        record(5, 0x384d_360f, 5, FLAG_LAST, "topmenu_panels_us.tim");
        v
    }

    struct Fake(Vec<u8>);

    impl Fake {
        /// Writes a cooked ISO holding these bytes and opens it. `label` keeps
        /// concurrently running tests off each other's file.
        fn reader(self, label: &str) -> Reader {
            // A cooked image with user data == the volume bytes.
            let path = std::env::temp_dir().join(format!("gtfs-test-{}-{label}.iso", std::process::id()));
            let mut padded = self.0;
            padded.resize(32 * 2048, 0);
            padded[16 * 2048] = 1;
            padded[16 * 2048 + 1..16 * 2048 + 6].copy_from_slice(b"CD001");
            padded[16 * 2048 + 156] = 34;
            padded[16 * 2048 + 158..16 * 2048 + 162].copy_from_slice(&17u32.to_le_bytes());
            padded[16 * 2048 + 166..16 * 2048 + 170].copy_from_slice(&2048u32.to_le_bytes());
            std::fs::write(&path, &padded).unwrap();
            let r = Reader::open(&path).unwrap();
            std::fs::remove_file(&path).ok();
            r
        }
    }

    #[test]
    fn parses_entries_terminated_by_the_volume_size() {
        let bytes = volume_bytes();
        let len = bytes.len() as u32;
        let mut reader = Fake(bytes).reader("ok");
        let volume = Volume::parse(&mut reader, 0, len).unwrap();
        assert_eq!(volume.tag, 0x2d64_2d3d);
        assert_eq!(volume.resources(), 3);
        assert_eq!(volume.offsets, vec![0x010, 0x800, 0x1008, len]);
    }

    #[test]
    fn resource_ranges_are_sectors_and_sizes_drop_the_padding() {
        let bytes = volume_bytes();
        let len = bytes.len() as u32;
        let mut reader = Fake(bytes).reader("sizes");
        let volume = Volume::parse(&mut reader, 0, len).unwrap();
        assert_eq!(volume.range(0), Some((0x000, 0x800)));
        assert_eq!(volume.size(0), Some(0x7f0));
        assert_eq!(volume.pad(0), Some(0x10));
        assert_eq!(volume.range(1), Some((0x800, 0x1000)));
        assert_eq!(volume.size(1), Some(0x800));
        assert_eq!(volume.pad(1), Some(0x00));
        assert_eq!(volume.range(2), Some((0x1000, 0x1800)));
        assert_eq!(volume.size(2), Some(0x7f8));
        assert_eq!(volume.pad(2), Some(0x08));
        assert_eq!(volume.range(3), None);
    }

    #[test]
    fn reads_records_and_their_flags() {
        let bytes = volume_bytes();
        let len = bytes.len() as u32;
        let mut reader = Fake(bytes).reader("records");
        let volume = Volume::parse(&mut reader, 0, len).unwrap();
        assert_eq!(volume.records.len(), 6);
        assert_eq!(volume.records[1].name, "license_info_uk");
        assert_eq!(volume.records[1].key, 0x385f_fab2);
        assert!(!volume.records[1].is_directory());
        assert!(!volume.records[1].is_last());
        let arcade = &volume.records[2];
        assert_eq!(arcade.ordinal, 3);
        assert!(arcade.is_directory() && arcade.is_last());
        // The root node is records 0..=2, arcade's node is 3..=5.
        assert_eq!(volume.node(0).len(), 3);
        assert_eq!(volume.node(3).len(), 3);
    }

    #[test]
    fn resolves_paths_through_the_tree() {
        let bytes = volume_bytes();
        let len = bytes.len() as u32;
        let mut reader = Fake(bytes).reader("find");
        let volume = Volume::parse(&mut reader, 0, len).unwrap();
        assert_eq!(volume.find("/license_info_uk"), Some(2));
        assert_eq!(volume.find("license_info_uk"), Some(2));
        assert_eq!(volume.find("/arcade/arc_font.tim"), Some(4));
        assert_eq!(volume.find("/arcade/topmenu_panels_us.tim"), Some(5));
        assert_eq!(volume.find("/arcade"), None, "a directory is not a file");
        assert_eq!(volume.find("/arcade/nothing.tim"), None);
        assert_eq!(volume.find("/"), None);
    }

    #[test]
    fn rejects_a_bad_magic_and_a_broken_table() {
        let mut bytes = volume_bytes();
        bytes[0] = b'X';
        let len = bytes.len() as u32;
        assert!(Volume::parse(&mut Fake(bytes).reader("magic"), 0, len).is_err());

        // A decrease is not an entry table; equal neighbours are fine.
        let mut bytes = volume_bytes();
        bytes[0x18..0x1C].copy_from_slice(&0x400u32.to_le_bytes());
        let len = bytes.len() as u32;
        assert!(Volume::parse(&mut Fake(bytes).reader("order"), 0, len).is_err());

        // An entry past the volume is rejected rather than read.
        let mut bytes = volume_bytes();
        bytes[0x14..0x18].copy_from_slice(&0x4000u32.to_le_bytes());
        let len = bytes.len() as u32;
        assert!(Volume::parse(&mut Fake(bytes).reader("range"), 0, len).is_err());

        // Padding larger than the span would make a negative length.
        let mut bytes = volume_bytes();
        bytes[0x14..0x18].copy_from_slice(&0x840u32.to_le_bytes());
        bytes[0x18..0x1C].copy_from_slice(&0x840u32.to_le_bytes());
        let len = bytes.len() as u32;
        assert!(Volume::parse(&mut Fake(bytes).reader("negative"), 0, len).is_err());
    }
}
