//! Text tables: the original's display names, addressed by slot.
//!
//! A table is an ordered list of strings. The order is the identity — the
//! original refers to a name by its index, and languages differ in length, so
//! nothing here sorts, deduplicates or fills gaps. Importers write the shape
//! from the original's format (`tools/import`'s `WSDB` decoder does); people
//! can write one by hand.
use serde::{Deserialize, Serialize};

/// An indexed list of display strings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TextTable {
    #[serde(default)]
    pub strings: Vec<String>,
}

impl TextTable {
    /// The string in slot `index`, if the table has it.
    pub fn get(&self, index: usize) -> Option<&str> {
        self.strings.get(index).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_the_identity() {
        let table: TextTable =
            ron::from_str(r#"(strings: ["Active Stability Controller", "Sports Brake Kit", ""])"#).unwrap();
        assert_eq!(table.get(0), Some("Active Stability Controller"));
        assert_eq!(table.get(2), Some(""));
        assert_eq!(table.get(3), None);
        assert_eq!(table.len(), 3);
    }

    #[test]
    fn a_baked_table_loads() {
        // Written by `import text`, kept in the game's assets: the importer and
        // this type must agree on the shape.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../game/assets/gt2/text/eng_unistrdb.ron");
        let Ok(text) = std::fs::read_to_string(&path) else {
            // Allow a checkout without imported assets yet.
            return;
        };
        let table: TextTable = ron::from_str(&text).expect("baked text table parses");
        assert_eq!(table.get(0), Some("Active Stability Controller"));
        assert_eq!(table.len(), 846);
    }
}
