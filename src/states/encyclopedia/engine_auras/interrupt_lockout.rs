//! The school lockout a successful interrupt leaves behind, named after the
//! interrupting ability (`combat_core::damage` builds it that way). One per
//! ability carrying the interrupt flags, so interrupt N+1 gets a lockout entry
//! with no code here.

use super::{EngineKey, EngineSpec};
use crate::states::encyclopedia::auras::{AuraArt, AuraSource, CatalogSource, Persistence};
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::components::{AuraType, DispelType};

pub fn members(source: &CatalogSource) -> Vec<EngineKey> {
    let mut interrupts: Vec<AbilityType> = source
        .abilities
        .iter()
        .filter(|(_, def)| def.is_interrupt && def.lockout_duration > 0.0)
        .map(|(ability, _)| *ability)
        .collect();
    interrupts.sort_unstable();
    interrupts.into_iter().map(EngineKey::Ability).collect()
}

pub fn entry(key: EngineKey, source: &CatalogSource) -> Option<EngineSpec> {
    let EngineKey::Ability(ability) = key else {
        return None;
    };
    let def = source.abilities.get(&ability)?;
    Some(EngineSpec {
        name: def.name.clone(),
        frame_name: None,
        mechanic: AuraType::SpellSchoolLockout,
        source: AuraSource::Ability(ability),
        art: AuraArt::FromSource,
        // The magnitude encodes WHICH school was locked and depends on what
        // the interrupt caught, so it is not a fact about this aura;
        // `magnitude_row` prints nothing for this mechanic.
        magnitude: 0.0,
        school: None,
        break_on_damage: -1.0,
        dispel_type: DispelType::Auto,
        persistence: Persistence::Seconds(def.lockout_duration),
        riders: EngineSpec::no_riders(),
        provenance: "Left behind when this interrupt lands. Only the school of the \
                     interrupted spell is locked — the target's other schools keep \
                     working."
            .to_string(),
    })
}
