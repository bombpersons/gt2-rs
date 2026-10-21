//! The GT-mode menu archives: `/gtmenu/commonpic.dat` + `.idx` and
//! `/gtmenu/<lang>/gtmenudat.dat` + `.idx`.
//!
//! Both `.idx` files have one shape: `u32 count`, `u32 field` (0 for
//! commonpic, 2 for gtmenudat; meaning unknown), then `count` **end** offsets,
//! the last equal to the `.dat` byte length.
//!
//! - `commonpic.dat` is `count` chunks that each begin `GTMP`; chunk 0 starts
//!   at 0, so the ends delimit the chunks exactly and every chunk size is a
//!   multiple of 0x1000.
//! - `gtmenudat.dat` is `count` concatenated gzip members (not one), each
//!   inflating to a `GM\3` blob. Members are packed on 4-byte boundaries with
//!   0..3 zero bytes between them. The `.idx` ends track those boundaries only
//!   to within a few bytes, so this splits the `.dat` directly and uses the
//!   index only as a count/length check.
//!
//! The entry payloads are still encoded (see `docs/formats.md`); this module
//! only splits the containers and accounts for every byte.

/// A parsed `.idx`: a count, one unknown word, then one end offset per entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    pub count: usize,
    pub field: u32,
    pub ends: Vec<u32>,
}

impl Index {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 8 || !(bytes.len() - 8).is_multiple_of(4) {
            return Err(format!("index is {} bytes, not 8 + 4n", bytes.len()));
        }
        let count = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
        let field = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        if bytes.len() != 8 + 4 * count {
            return Err(format!(
                "index says {count} entries but is {} bytes (expected {})",
                bytes.len(),
                8 + 4 * count
            ));
        }
        let ends = (0..count)
            .map(|i| u32::from_le_bytes(bytes[8 + 4 * i..12 + 4 * i].try_into().unwrap()))
            .collect::<Vec<_>>();
        if ends.windows(2).any(|w| w[0] >= w[1]) {
            return Err("index ends are not strictly increasing".into());
        }
        Ok(Self { count, field, ends })
    }

    /// The length check shared by both archives: the last end is the `.dat`
    /// length.
    fn expect_len(&self, dat: &[u8]) -> Result<(), String> {
        match self.ends.last() {
            Some(&last) if last as usize == dat.len() => Ok(()),
            Some(&last) => Err(format!("index ends at {last}, .dat is {} bytes", dat.len())),
            None => Err("index has no entries".into()),
        }
    }
}

/// One `GTMP` chunk of `commonpic.dat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk {
    pub start: usize,
    pub end: usize,
}

impl Chunk {
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// Splits `commonpic.dat` at the index ends, checking that each chunk starts
/// `GTMP` and that the chunks tile the file.
pub fn commonpic_chunks(dat: &[u8], index: &Index) -> Result<Vec<Chunk>, String> {
    index.expect_len(dat)?;
    let mut chunks = Vec::with_capacity(index.count);
    let mut start = 0usize;
    for &end in &index.ends {
        let end = end as usize;
        if end <= start {
            return Err(format!("chunk end {end} does not advance past {start}"));
        }
        if !dat[start..].starts_with(b"GTMP") {
            return Err(format!("chunk at {start:#x} does not start GTMP"));
        }
        chunks.push(Chunk { start, end });
        debug_assert!(!chunks.last().unwrap().is_empty());
        start = end;
    }
    Ok(chunks)
}

/// One gzip member of `gtmenudat.dat`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub start: usize,
    /// End of the compressed stream, before padding.
    pub end: usize,
    /// Bytes the member inflates to.
    pub inflated: usize,
    /// First four inflated bytes (`GM\3` for every member on the disc).
    pub magic: [u8; 4],
}

/// Splits `gtmenudat.dat` into its gzip members and checks the 4-byte packing:
/// every gap between members is zero padding to the next boundary.
pub fn gtmenudat_members(dat: &[u8], index: &Index) -> Result<Vec<Member>, String> {
    index.expect_len(dat)?;
    let mut members = Vec::with_capacity(index.count);
    let mut pos = 0usize;
    while pos < dat.len() {
        if !dat[pos..].starts_with(&[0x1f, 0x8b]) {
            return Err(format!("member at {pos:#x} is not a gzip stream"));
        }
        let (blob, used) = inflate_member(&dat[pos..]).map_err(|e| format!("member at {pos:#x}: {e}"))?;
        let end = pos + used;
        let magic = blob.get(..4).map(|m| m.try_into().unwrap()).unwrap_or([0; 4]);
        members.push(Member { start: pos, end, inflated: blob.len(), magic });
        let next = (end + 3) & !3;
        if next > dat.len() {
            return Err(format!("member at {pos:#x} ends {end:#x}, past the file"));
        }
        if dat[end..next].iter().any(|&b| b != 0) {
            return Err(format!("padding after member {pos:#x} is not zero"));
        }
        pos = next;
    }
    if members.len() != index.count {
        return Err(format!("found {} gzip members, index says {}", members.len(), index.count));
    }
    Ok(members)
}

/// Inflates one gzip member and reports how many input bytes it consumed.
///
/// `flate2::read::GzDecoder` buffers ahead, so the input position after it is
/// not the member length. This walks the gzip header, inflates the raw deflate
/// stream with `Decompress` (whose `total_in` stops exactly at `StreamEnd`),
/// then adds the 8-byte CRC/length trailer, which is what the 4-byte packing is
/// checked against.
fn inflate_member(data: &[u8]) -> Result<(Vec<u8>, usize), String> {
    let header = gzip_header_len(data)?;
    let mut decoder = flate2::Decompress::new(false);
    let mut out = Vec::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let in_before = decoder.total_in();
        let out_before = decoder.total_out();
        let status = decoder
            .decompress(&data[header + in_before as usize..], &mut buf, flate2::FlushDecompress::None)
            .map_err(|e| e.to_string())?;
        let produced = (decoder.total_out() - out_before) as usize;
        out.extend_from_slice(&buf[..produced]);
        match status {
            flate2::Status::StreamEnd => {
                let used = header + decoder.total_in() as usize + 8;
                if used > data.len() {
                    return Err("gzip trailer runs past the input".into());
                }
                return Ok((out, used));
            }
            flate2::Status::Ok if decoder.total_in() > in_before || produced > 0 => {}
            flate2::Status::Ok => return Err("gzip made no progress".into()),
            flate2::Status::BufError => return Err("gzip buffer error".into()),
        }
    }
}

/// Length of a gzip member's header (RFC 1952), including optional fields.
fn gzip_header_len(data: &[u8]) -> Result<usize, String> {
    if data.len() < 10 || data[0] != 0x1f || data[1] != 0x8b || data[2] != 8 {
        return Err("not a gzip member".into());
    }
    let flags = data[3];
    let mut at = 10usize;
    if flags & 0x04 != 0 {
        let extra =
            u16::from_le_bytes(data.get(at..at + 2).ok_or("truncated gzip extra field")?.try_into().unwrap()) as usize;
        at += 2 + extra;
    }
    for bit in [0x08u8, 0x10] {
        if flags & bit != 0 {
            let rest = data.get(at..).ok_or("truncated gzip header")?;
            let nul = rest.iter().position(|&b| b == 0).ok_or("unterminated gzip header string")?;
            at += nul + 1;
        }
    }
    if flags & 0x02 != 0 {
        at += 2;
    }
    if at > data.len() {
        return Err("gzip header runs past the input".into());
    }
    Ok(at)
}

/// Reads an archive pair and returns a one-line structural summary. Chooses
/// the splitter from the `.dat`'s first bytes, not its name.
pub fn inspect(dat: &[u8], index_bytes: &[u8]) -> Result<String, String> {
    let index = Index::parse(index_bytes)?;
    if dat.starts_with(b"GTMP") {
        let chunks = commonpic_chunks(dat, &index)?;
        let sizes = chunks.iter().map(Chunk::len);
        let (min, max) = (sizes.clone().min().unwrap_or(0), sizes.clone().max().unwrap_or(0));
        let aligned = chunks.iter().all(|c| c.len() % 0x1000 == 0);
        return Ok(format!(
            "commonpic: {} GTMP chunks, {:.1} MB, sizes {min:#x}..{max:#x}{}, {} bytes unaccounted",
            chunks.len(),
            dat.len() as f64 / (1024.0 * 1024.0),
            if aligned { ", all multiples of 0x1000" } else { "" },
            dat.len() - chunks.iter().map(|c| c.len()).sum::<usize>()
        ));
    }
    if dat.starts_with(&[0x1f, 0x8b]) {
        let members = gtmenudat_members(dat, &index)?;
        let blobs = members.iter().filter(|m| &m.magic[..3] == b"GM\x03").count();
        let inflated: usize = members.iter().map(|m| m.inflated).sum();
        let packed: usize = members.iter().map(|m| m.end - m.start).sum();
        return Ok(format!(
            "gtmenudat: {} gzip members ({blobs} are GM\\3), {:.1} MB packed, {} bytes of zero padding, {:.1} MB inflated, 0 bytes unaccounted",
            members.len(),
            packed as f64 / (1024.0 * 1024.0),
            dat.len() - packed,
            inflated as f64 / (1024.0 * 1024.0)
        ));
    }
    Err("not a menu archive (neither GTMP nor gzip)".into())
}

/// Reads a `.dat` and its `.idx` from disk. Present so the CLI path and the
/// tests share one code path.
pub fn inspect_files(dat: &std::path::Path, index: &std::path::Path) -> std::io::Result<String> {
    let dat_bytes = std::fs::read(dat)?;
    let index_bytes = std::fs::read(index)?;
    inspect(&dat_bytes, &index_bytes).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(entries: &[u32], field: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        out.extend_from_slice(&field.to_le_bytes());
        for &e in entries {
            out.extend_from_slice(&e.to_le_bytes());
        }
        out
    }

    #[test]
    fn index_rejects_a_count_that_does_not_match() {
        let bytes = index(&[0x1000, 0x2000], 0);
        assert!(Index::parse(&bytes).is_ok());
        assert!(Index::parse(&bytes[..bytes.len() - 4]).is_err());
    }

    #[test]
    fn commonpic_chunks_tile_the_file() {
        let chunk = |n: u8| {
            let mut c = b"GTMP".to_vec();
            c.resize(0x1000, n);
            c
        };
        let mut dat = Vec::new();
        for n in 0..3 {
            dat.extend_from_slice(&chunk(n));
        }
        let idx = index(&[0x1000, 0x2000, 0x3000], 0);
        let chunks = commonpic_chunks(&dat, &Index::parse(&idx).unwrap()).unwrap();
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[2].start, 0x2000);
        assert!(commonpic_chunks(&dat[..0x2fff], &Index::parse(&idx).unwrap()).is_err());
    }

    fn gzip(blob: &[u8]) -> Vec<u8> {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(blob).unwrap();
        enc.finish().unwrap()
    }

    #[test]
    fn gtmenudat_members_are_gzip_blobs_on_4_byte_boundaries() {
        let mut dat = Vec::new();
        let mut ends = Vec::new();
        for n in 0..3u8 {
            dat.extend_from_slice(&gzip(&[b'G', b'M', 3, n, n, n]));
            while !dat.len().is_multiple_of(4) {
                dat.push(0);
            }
            ends.push(dat.len() as u32);
        }
        let idx = index(&ends, 2);
        let members = gtmenudat_members(&dat, &Index::parse(&idx).unwrap()).unwrap();
        assert_eq!(members.len(), 3);
        assert!(members.iter().all(|m| &m.magic[..3] == b"GM\x03"));
        assert!(members.iter().all(|m| m.start % 4 == 0));
    }

    #[test]
    fn inspect_picks_the_splitter_from_the_magic() {
        let dat = gzip(b"GM\x03payload");
        let mut padded = dat.clone();
        while !padded.len().is_multiple_of(4) {
            padded.push(0);
        }
        let idx = index(&[padded.len() as u32], 2);
        let report = inspect(&padded, &idx).unwrap();
        assert!(report.starts_with("gtmenudat:"), "{report}");

        let mut pic = b"GTMP".to_vec();
        pic.resize(0x1000, 0);
        let report = inspect(&pic, &index(&[0x1000], 0)).unwrap();
        assert!(report.starts_with("commonpic:"), "{report}");
    }
}
