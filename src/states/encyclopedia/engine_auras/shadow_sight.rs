//! Shadow Sight: granted by picking up an arena Shadow Sight orb. The one
//! aura no ability, item or talent applies, so it is listed whatever the
//! tables hold.

use super::{unit_if, EngineKey, EngineSpec};
use crate::states::encyclopedia::auras::{AuraArt, AuraSource, CatalogSource, Persistence};
use crate::states::play_match::components::{AuraType, DispelType};
use crate::states::play_match::shadow_sight::{
    SHADOW_SIGHT_BREAK_ON_DAMAGE, SHADOW_SIGHT_DURATION, SHADOW_SIGHT_SPAWN_TIME,
};

pub fn members(_source: &CatalogSource) -> Vec<EngineKey> {
    unit_if(true)
}

pub fn entry(key: EngineKey, _source: &CatalogSource) -> Option<EngineSpec> {
    (key == EngineKey::Unit).then(|| EngineSpec {
        name: "Shadow Sight".to_string(),
        frame_name: None,
        mechanic: AuraType::ShadowSight,
        source: AuraSource::Mechanic("a Shadow Sight orb"),
        // An arena orb pickup: no ability icon exists to borrow.
        art: AuraArt::Own("aura_shadow_sight"),
        magnitude: 1.0,
        school: None,
        break_on_damage: SHADOW_SIGHT_BREAK_ON_DAMAGE,
        dispel_type: DispelType::Auto,
        persistence: Persistence::Seconds(SHADOW_SIGHT_DURATION),
        riders: EngineSpec::no_riders(),
        provenance: format!(
            "Granted by picking up one of the two orbs that spawn in mid-arena {:.0} sec \
             after the gates open.",
            SHADOW_SIGHT_SPAWN_TIME
        ),
    })
}
