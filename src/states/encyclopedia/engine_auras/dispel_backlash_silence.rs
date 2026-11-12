//! The silence Unstable Affliction's dispel backlash puts on the dispeller,
//! which shares the damage-over-time's name.

use super::{has_ability, unit_if, EngineKey, EngineSpec};
use crate::states::encyclopedia::auras::{AuraArt, AuraSource, CatalogSource, Persistence};
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::effects::backlash::{
    dispel_backlash_silence_aura, DISPEL_BACKLASH_SILENCE_DURATION,
};

pub fn members(source: &CatalogSource) -> Vec<EngineKey> {
    unit_if(has_ability(source, AbilityType::UnstableAffliction))
}

pub fn entry(key: EngineKey, source: &CatalogSource) -> Option<EngineSpec> {
    if key != EngineKey::Unit || !has_ability(source, AbilityType::UnstableAffliction) {
        return None;
    }
    let sample = dispel_backlash_silence_aura(None, DISPEL_BACKLASH_SILENCE_DURATION);
    Some(EngineSpec {
        name: "Unstable Affliction (dispel backlash)".to_string(),
        frame_name: Some(sample.ability_name.clone()),
        mechanic: sample.effect_type,
        source: AuraSource::Ability(AbilityType::UnstableAffliction),
        art: AuraArt::FromSource,
        magnitude: sample.magnitude,
        school: sample.spell_school,
        break_on_damage: sample.break_on_damage_threshold,
        dispel_type: sample.dispel_type,
        persistence: Persistence::Seconds(sample.duration),
        riders: EngineSpec::no_riders(),
        provenance: "Punishes whoever lifts an Unstable Affliction off an ally, \
                     alongside the backlash damage. Dispelling it on your own team \
                     costs nothing."
            .to_string(),
    })
}
