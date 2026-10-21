//! `GT2.OVL`, the disc's overlay container.
//!
//! `GT2.OVL` (root directory, 289 752 bytes) holds the game's position
//! dependent code overlays. It is a `u32` table followed by one **gzip member
//! per overlay**, each member starting at a 4-byte boundary.
//!
//! The table is `0x30` bytes: `u32 header_size` (= `0x30`), then, per member,
//! the compressed size, and after it (redundantly) the next member's offset.
//! The offsets are always `align4(previous offset + previous size)`, so only
//! the sizes are load-bearing; [`Overlay::parse`] checks the two agree and
//! fails if they do not, which is how a different disc revision shows up.
//!
//! At run time the boot EXE reads the CD sectors covering a member into RAM and
//! decompresses it with its own resident DEFLATE decoder (`FUN_800847D0`,
//! entry `0x800847D0`; the inner Huffman symbol reader is `FUN_80084364`).
//! Observed at frame 2241 of the title -> license route: the CD window held the
//! gzip member beginning at file offset `0x3C784` in RAM at `0x800E54E0`, the
//! decoder wrote its output to `0x80010000`, and the first byte it produced was
//! `0xD0` (`addiu sp, sp, -0x30`) - this module's member index 4.
//!
//! Overlay images are analysis artifacts, not engine assets: they are code,
//! and the port never loads them. `import overlays` writes them so the game's
//! per-screen loaders (which live here, not in the boot EXE) can be read.

/// One overlay: a gzip member of the container and its inflated image.
#[derive(Debug)]
pub struct Member {
    /// Index in the container, in table order.
    pub index: usize,
    /// Byte offset of the gzip member in `GT2.OVL`.
    pub offset: usize,
    /// Byte length of the gzip member (its compressed size).
    pub compressed_size: usize,
    /// The inflated overlay image. Position-dependent, based at `0x80010000`.
    pub image: Vec<u8>,
    /// `true` if the stored size ran past the end of the container.
    pub truncated: bool,
}

/// The parsed container.
#[derive(Debug)]
pub struct Overlay {
    pub header_size: u32,
    pub members: Vec<Member>,
    /// Bytes that belong to no member: the table, the 0..3 alignment bytes
    /// between members, and any bytes after the last member.
    pub padding_bytes: usize,
}

/// The address every overlay image is linked at. Observed for member 4; the
/// rest are whole-program images in the same slot.
pub const LOAD_ADDRESS: u32 = 0x8001_0000;

impl Overlay {
    /// Parses the container and inflates every member.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        use std::io::Read as _;
        if bytes.len() < 8 {
            return Err(format!("GT2.OVL is {} bytes, too short for a header", bytes.len()));
        }
        let header_size = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        if !(header_size as usize).is_multiple_of(4) || header_size as usize > bytes.len() {
            return Err(format!("header size {header_size:#x} is not a multiple of 4 inside the file"));
        }
        let words = header_size as usize / 4;
        // A header is `2 * members` words: the size of each member, with the
        // next member's offset stored after all but the last.
        if words < 2 || !words.is_multiple_of(2) {
            return Err(format!("header of {words} words has no whole members"));
        }
        // Table order: header, then size/next-offset pairs, with the final
        // member's size last (no trailing offset).
        let mut sizes = Vec::new();
        let mut offsets = vec![header_size as usize];
        let mut redundancy = 0usize;
        let mut at = 1;
        while at + 1 < words {
            let size = u32::from_le_bytes(bytes[at * 4..at * 4 + 4].try_into().unwrap()) as usize;
            let next = u32::from_le_bytes(bytes[at * 4 + 4..at * 4 + 8].try_into().unwrap()) as usize;
            sizes.push(size);
            let aligned = (offsets.last().unwrap() + size + 3) & !3;
            if next != aligned {
                return Err(format!(
                    "member {}: table says it ends at {next:#x}, size {size:#x} puts it at {aligned:#x}",
                    sizes.len() - 1
                ));
            }
            offsets.push(next);
            redundancy += 1;
            at += 2;
        }
        if at < words {
            sizes.push(u32::from_le_bytes(bytes[at * 4..at * 4 + 4].try_into().unwrap()) as usize);
        }
        if sizes.len() != redundancy + 1 {
            return Err(format!("header lists {} sizes for {redundancy} offsets", sizes.len()));
        }
        let mut members = Vec::with_capacity(sizes.len());
        for (index, (&offset, &size)) in offsets.iter().zip(&sizes).enumerate() {
            let end = offset + size;
            let truncated = end > bytes.len();
            let stored = &bytes[offset..end.min(bytes.len())];
            let mut image = Vec::new();
            flate2::read::GzDecoder::new(stored)
                .read_to_end(&mut image)
                .map_err(|e| format!("member {index} at {offset:#x} did not inflate: {e}"))?;
            members.push(Member { index, offset, compressed_size: size, image, truncated });
        }
        // Structural check: the header and the members tile the file, with only
        // 4-byte alignment between members and (observed: 2 bytes) after the
        // last one, so nothing is unaccounted for.
        let end = offsets.last().unwrap() + sizes.last().unwrap();
        if end > bytes.len() {
            return Err(format!("the last member ends at {end:#x}, past the {:#x}-byte file", bytes.len()));
        }
        let alignment: usize = (1..offsets.len()).map(|i| offsets[i] - offsets[i - 1] - sizes[i - 1]).sum();
        let padding = bytes.len() - header_size as usize - sizes.iter().sum::<usize>();
        if padding != alignment + (bytes.len() - end) {
            return Err(format!(
                "container geometry: {padding} bytes unaccounted, {alignment} {:#x} alignment",
                bytes.len() - end
            ));
        }
        Ok(Self { header_size, members, padding_bytes: padding })
    }

    /// Total inflated size of every member.
    pub fn image_bytes(&self) -> usize {
        self.members.iter().map(|m| m.image.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    /// Builds a container with the same table shape as `GT2.OVL`.
    fn container(images: &[&[u8]]) -> Vec<u8> {
        let members: Vec<Vec<u8>> = images.iter().map(|i| gzip(i)).collect();
        let header_size = 4 * (2 * members.len());
        let mut out = vec![0u8; header_size];
        out[0..4].copy_from_slice(&(header_size as u32).to_le_bytes());
        let mut offsets = vec![header_size];
        for m in &members {
            offsets.push((offsets.last().unwrap() + m.len() + 3) & !3);
        }
        for (i, m) in members.iter().enumerate() {
            let at = 1 + 2 * i;
            out[at * 4..at * 4 + 4].copy_from_slice(&(m.len() as u32).to_le_bytes());
            if at + 1 < header_size / 4 {
                out[at * 4 + 4..at * 4 + 8].copy_from_slice(&(offsets[i + 1] as u32).to_le_bytes());
            }
        }
        for (i, m) in members.iter().enumerate() {
            out.resize(offsets[i], 0);
            out.extend_from_slice(m);
        }
        out
    }

    #[test]
    fn parses_a_two_member_container() {
        let bytes = container(&[b"first overlay", b"second"]);
        let parsed = Overlay::parse(&bytes).unwrap();
        assert_eq!(parsed.header_size, 0x10);
        assert_eq!(parsed.members.len(), 2);
        assert_eq!(parsed.members[0].image, b"first overlay");
        assert_eq!(parsed.members[1].image, b"second");
        assert!(!parsed.members.iter().any(|m| m.truncated));
        assert_eq!(parsed.image_bytes(), 19);
    }

    #[test]
    fn rejects_a_table_that_disagrees_with_the_sizes() {
        let mut bytes = container(&[b"aaa", b"bbb"]);
        // Member 0's next-offset is the only redundancy in the format.
        bytes[8..12].copy_from_slice(&0x40u32.to_le_bytes());
        let why = Overlay::parse(&bytes).unwrap_err();
        assert!(why.contains("table says it ends"), "{why}");
    }

    #[test]
    fn rejects_a_member_that_is_not_gzip() {
        let mut bytes = container(&[b"aaa"]);
        let at = 8usize; // member 0's offset is the header size here
        bytes[at] = 0x00;
        let why = Overlay::parse(&bytes).unwrap_err();
        assert!(why.contains("did not inflate"), "{why}");
    }
}
