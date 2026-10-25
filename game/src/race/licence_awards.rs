//! Licence achievement updates at the return-to-menu boundary (8004DD80).
//!
//! The race's finish and prize overlays precede this update. Front ends emit
//! one `CommitLicenceAward` after closing the completed attempt/replay; these
//! rules never mutate progression merely because a vehicle finished driving.
use super::licence::LicenceDefinition;
use bevy::prelude::*;
use engine::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const INITIAL_PROGRESS: &str = "race/licence-progress.initial.ron";

/// Original saved grades 0..4. Grade 1 passes the group-completion check but
/// requires repeated qualifying attempts; ordinary bronze starts at grade 2.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord, Reflect, Serialize, Deserialize)]
pub enum LicenceGrade {
    #[default]
    Unpassed,
    AssistedPass,
    Bronze,
    Silver,
    Gold,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub struct LicenceAchievement {
    pub grade: LicenceGrade,
    /// Saved record +2, not the number of all driving attempts. Only an
    /// unpassed test's fourth-band results increment this saturated byte.
    pub assisted_attempts: u8,
}

impl LicenceDefinition {
    /// Raw time band before assisted-pass qualification. 8004DE14..E6C uses
    /// unsigned *strict* comparisons. Equal to gold can
    /// still earn silver; equal to the fourth threshold earns no grade.
    pub fn grade_at_time(&self, time_ms: u32) -> LicenceGrade {
        use LicenceGrade::*;
        [Gold, Silver, Bronze, AssistedPass]
            .into_iter()
            .zip(self.time_limits_ms)
            .find_map(|(grade, limit)| (time_ms < limit).then_some(grade))
            .unwrap_or(Unpassed)
    }
}

impl LicenceAchievement {
    /// Apply a completed result once. Failed/aborted runs skip 8004DD80 at
    /// 8004E184..1A8. A slower success never downgrades a saved achievement.
    pub fn apply_result(&mut self, definition: &LicenceDefinition, time_ms: Option<u32>) -> bool {
        let Some(time_ms) = time_ms else { return false };
        let mut earned = definition.grade_at_time(time_ms);
        if earned == LicenceGrade::AssistedPass {
            if let Some(required) = definition.assisted_pass_attempts {
                if self.grade == LicenceGrade::Unpassed {
                    // 8004DE8C..EA0 saturates at 255; EA4..EC8 checks the
                    // incremented count against the selected GTDT byte.
                    self.assisted_attempts = self.assisted_attempts.saturating_add(1);
                    if self.assisted_attempts < required {
                        earned = LicenceGrade::Unpassed;
                    }
                }
            } else {
                earned = LicenceGrade::Unpassed;
            }
        }
        // 8004DECC..EE0 retains the greatest saved grade.
        if earned > self.grade {
            self.grade = earned;
            true
        } else {
            false
        }
    }
}

#[derive(Resource, Clone, Default, Debug, Reflect, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct LicenceProgress {
    pub achievements: BTreeMap<String, LicenceAchievement>,
}

impl LicenceProgress {
    pub fn achievement(&self, id: &str) -> LicenceAchievement {
        self.achievements.get(id).copied().unwrap_or_default()
    }

    /// 8004DCAC checks all ten saved grades for nonzero. The caller supplies
    /// group membership, so authored licences can use their own group sizes.
    pub fn group_passed<'a>(&self, tests: impl IntoIterator<Item = &'a str>) -> bool {
        tests.into_iter().all(|id| self.achievement(id).grade != LicenceGrade::Unpassed)
    }

    /// 8004DD14 checks the same group for grades at least four.
    pub fn group_gold<'a>(&self, tests: impl IntoIterator<Item = &'a str>) -> bool {
        tests.into_iter().all(|id| self.achievement(id).grade == LicenceGrade::Gold)
    }
}

/// Front-end boundary, not a race-finish event. Emit once per attempt after
/// returning from its result/replay. None represents failure or abandonment.
#[derive(Event, Clone, Debug)]
pub struct CommitLicenceAward {
    pub definition: LicenceDefinition,
    pub time_ms: Option<u32>,
}

#[derive(Event, Clone, Debug)]
pub struct LicenceAwardApplied {
    pub licence_id: String,
    pub achievement: LicenceAchievement,
    pub improved: bool,
}

pub struct LicenceAwardsPlugin;
impl Plugin for LicenceAwardsPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<LicenceProgress>() {
            let initial = if engine::config::asset_path(INITIAL_PROGRESS).is_file() {
                load_ron(INITIAL_PROGRESS).expect("initial licence achievements")
            } else {
                LicenceProgress::default()
            };
            app.insert_resource(initial);
        }
        app.register_type::<LicenceProgress>().add_observer(commit);
    }
}

fn commit(event: On<CommitLicenceAward>, mut progress: ResMut<LicenceProgress>, mut commands: Commands) {
    let id = &event.definition.id;
    let mut achievement = progress.achievement(id);
    let improved = achievement.apply_result(&event.definition, event.time_ms);
    if event.time_ms.is_some() {
        progress.achievements.insert(id.clone(), achievement);
    }
    commands.trigger(LicenceAwardApplied { licence_id: id.clone(), achievement, improved });
}
