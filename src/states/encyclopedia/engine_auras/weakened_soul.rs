//! Weakened Soul: the Power Word: Shield cooldown marker, stamped on the ally
//! the moment the shield lands.

use super::{has_ability, unit_if, EngineKey, EngineSpec};
use crate::states::encyclopedia::auras::{AuraArt, AuraSource, CatalogSource, Persistence};
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::components::{AuraType, DispelType};
use crate::states::play_match::constants::WEAKENED_SOUL_DURATION;

pub fn members(source: &CatalogSource) -> Vec<EngineKey> {
    unit_if(has_ability(source, AbilityType::PowerWordShield))
}

pub fn entry(key: EngineKey, source: &CatalogSource) -> Option<EngineSpec> {
    (key == EngineKey::Unit && has_ability(source, AbilityType::PowerWordShield)).then(|| {
        EngineSpec {
            name: "Weakened Soul".to_string(),
            frame_name: None,
            mechanic: AuraType::WeakenedSoul,
            source: AuraSource::Ability(AbilityType::PowerWordShield),
            // Applied by Power Word: Shield, but it is the debuff that BLOCKS
            // the next shield — borrowing the shield's icon would say the
            // opposite.
            art: AuraArt::Own("aura_weakened_soul"),
            magnitude: 0.0,
            school: None,
            break_on_damage: -1.0,
            dispel_type: DispelType::Auto,
            persistence: Persistence::Seconds(WEAKENED_SOUL_DURATION),
            riders: EngineSpec::no_riders(),
            provenance: "Placed on the ally the moment the shield lands: one cast applies both."
                .to_string(),
        }
    })
}
