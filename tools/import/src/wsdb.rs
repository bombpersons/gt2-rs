//! `WSDB` text tables -> an engine-native string table.
//!
//! `/carparam/*_unistrdb.dat` (gzipped in the volume, decompressing to `WSDB`)
//! is the game's name database: car and part names, in release order. The
//! layout, verified on all seven language files (`eng`, `usa`, `jpn`, `fra`,
//! `ger`, `ita`, `spa`):
//!
//! ```text
//! 0x00  u32             file length in bytes (equals the real length)
//! 0x04  "WSDB"
//! 0x08  u16             entry count (846; usa 851, jpn 764)
//! 0x0a  entry[count]    [u16 chars][chars * UTF-16LE][u16 0]
//! ```
//!
//! The strings are **indexed by position**: the original refers to a name by
//! its slot, so the engine asset preserves the order and the gaps a language
//! leaves rather than sorting or deduplicating. Every byte is accounted for:
//! parsing all `count` entries ends exactly at the declared length (checked
//! for every entry of every file).
//!
//! The `u16` before each string is its length in UTF-16 code units, not bytes,
//! and the string is followed by a `u16` zero. Japanese names are BMP text
//! (the disc's Japanese names decode as ordinary UTF-16LE); a surrogate pair
//! would count as two units and still parse, because the length is in units.
use serde::Serialize;

/// The RON shape the importer writes: the strings in their original slots.
///
/// Kept in step with `engine::text::TextTable`; the engine's test loads a
/// table written by this importer.
#[derive(Debug, Serialize)]
pub struct TextTable {
    pub strings: Vec<String>,
}

impl TextTable {
    /// Parses a whole `WSDB` file. Rejects anything that does not account for
    /// every byte, so a partial decode cannot pass as success.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let word = |at: usize| -> Result<u16, String> {
            bytes
                .get(at..at + 2)
                .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
                .ok_or_else(|| format!("WSDB is truncated at {at:#x}"))
        };
        let declared = bytes
            .get(0..4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
            .ok_or_else(|| "WSDB is shorter than its header".to_string())?;
        if bytes.get(4..8) != Some(b"WSDB") {
            return Err(format!("not a WSDB table: magic is {:02x?}", bytes.get(4..8).unwrap_or_default()));
        }
        if declared != bytes.len() {
            return Err(format!("WSDB declares {declared} bytes but the file is {} bytes", bytes.len()));
        }
        let count = usize::from(word(8)?);
        let mut strings = Vec::with_capacity(count);
        let mut at = 10;
        for index in 0..count {
            let units = usize::from(word(at)?);
            at += 2;
            let end = at + units * 2;
            let raw = bytes.get(at..end).ok_or_else(|| format!("string {index} runs past the file at {at:#x}"))?;
            let text = decode_utf16(raw).map_err(|e| format!("string {index} at {at:#x}: {e}"))?;
            if word(end)? != 0 {
                return Err(format!("string {index} at {at:#x} has no NUL terminator"));
            }
            strings.push(text);
            at = end + 2;
        }
        if at != bytes.len() {
            return Err(format!("{} trailing bytes after {count} strings", bytes.len() - at));
        }
        Ok(Self { strings })
    }
}

/// UTF-16LE bytes to a `String`, rejecting unpaired surrogates.
fn decode_utf16(bytes: &[u8]) -> Result<String, String> {
    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    String::from_utf16(&units).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(entries: &[&str]) -> Vec<u8> {
        let mut body = Vec::new();
        for entry in entries {
            let units: Vec<u16> = entry.encode_utf16().collect();
            body.extend_from_slice(&(units.len() as u16).to_le_bytes());
            for unit in units {
                body.extend_from_slice(&unit.to_le_bytes());
            }
            body.extend_from_slice(&0u16.to_le_bytes());
        }
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&((10 + body.len()) as u32).to_le_bytes());
        bytes.extend_from_slice(b"WSDB");
        bytes.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&body);
        bytes
    }

    #[test]
    fn parses_slots_in_release_order() {
        let parsed = TextTable::parse(&table(&["Sports Brake Kit", "ワゴンR エアロRS", ""])).unwrap();
        assert_eq!(parsed.strings, vec!["Sports Brake Kit", "ワゴンR エアロRS", ""]);
    }

    #[test]
    fn rejects_trailing_bytes_bytes_and_a_wrong_declared_length() {
        let mut bytes = table(&["a"]);
        bytes.push(0);
        assert!(TextTable::parse(&bytes).is_err(), "trailing byte");

        let mut bytes = table(&["a"]);
        bytes[0] = bytes[0].wrapping_sub(1);
        assert!(TextTable::parse(&bytes).is_err(), "declared length disagrees with the file");
    }

    #[test]
    fn rejects_a_string_without_its_terminator() {
        let mut bytes = table(&["a"]);
        *bytes.last_mut().unwrap() = 1;
        assert!(TextTable::parse(&bytes).is_err());
    }
}
