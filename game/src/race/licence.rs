//! Native licence definitions and personal records. Finish conditions, award
//! progression and persistent saves are separate from these content/state types.
use bevy::prelude::*;
use engine::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const LICENCE_DEFINITION: &str = "race/licence.ron";
pub const INITIAL_RECORDS: &str = "race/records.initial.ron";

#[derive(Resource, Clone, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct LicenceDefinition {
    pub id: String,
    /// Gold, silver, bronze and the fourth original award threshold.
    /// Only the first three are shown by the records panel.
    pub time_limits_ms: [u32; 4],
}

#[derive(Clone, Debug, PartialEq, Reflect, Serialize, Deserialize)]
pub struct LicenceRecord {
    pub time_ms: u32,
    /// The record's source +0x10 halfword, displayed as speed/100 plus units.
    pub speed_readout: u16,
}

#[derive(Resource, Default, Clone, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct LicenceRecords {
    pub best: BTreeMap<String, LicenceRecord>,
}

pub struct LicencePlugin;
impl Plugin for LicencePlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<LicenceDefinition>()
            && engine::config::asset_path(LICENCE_DEFINITION).is_file()
        {
            app.add_plugins(TuningPlugin::<LicenceDefinition>::new(LICENCE_DEFINITION));
        }
        if !app.world().contains_resource::<LicenceRecords>() {
            let initial = if engine::config::asset_path(INITIAL_RECORDS).is_file() {
                load_ron(INITIAL_RECORDS).expect("initial licence records")
            } else {
                LicenceRecords::default()
            };
            app.insert_resource(initial);
        }
        app.register_type::<LicenceDefinition>().register_type::<LicenceRecords>();
    }
}

/// 80068734: below one hour use milliseconds; then show tenths and hours.
/// Absent and out-of-range records use the content's missing-time label.
pub fn format_record_time(ms: Option<u32>, missing: &str) -> String {
    match ms {
        Some(ms) if ms < 3_600_000 => format!("{}:{:02}.{:03}", ms / 60000, ms / 1000 % 60, ms % 1000),
        Some(ms) if ms < 360_000_000 => {
            format!("{}:{:02}:{:02}.{}", ms / 3_600_000, ms / 60000 % 60, ms / 1000 % 60, ms / 100 % 10)
        }
        _ => missing.to_owned(),
    }
}
