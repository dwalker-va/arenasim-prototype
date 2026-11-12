//! The Rogue's weapon-coating marker — a permanent SELF-BUFF named after the
//! poison, distinct from the debuff that poison applies on hit. One per poison
//! whose ability the table holds, so poison N+1 needs no code here.

use super::{has_ability, EngineKey, EngineSpec};
use crate::states::encyclopedia::auras::{AuraArt, AuraSource, CatalogSource, Persistence};
use crate::states::match_config::RoguePoison;
use crate::states::play_match::components::weapon_poison_marker_aura;

pub fn members(source: &CatalogSource) -> Vec<EngineKey> {
    RoguePoison::ALL
        .iter()
        .filter(|poison| has_ability(source, poison.ability()))
        .map(|poison| EngineKey::Poison(*poison))
        .collect()
}

pub fn entry(key: EngineKey, source: &CatalogSource) -> Option<EngineSpec> {
    let EngineKey::Poison(poison) = key else {
        return None;
    };
    if !has_ability(source, poison.ability()) {
        return None;
    }
    let sample = weapon_poison_marker_aura(poison);
    Some(EngineSpec {
        name: format!("{} (weapon coating)", poison.name()),
        frame_name: Some(sample.ability_name.clone()),
        mechanic: sample.effect_type,
        source: AuraSource::Ability(poison.ability()),
        art: AuraArt::FromSource,
        magnitude: sample.magnitude,
        school: sample.spell_school,
        break_on_damage: sample.break_on_damage_threshold,
        dispel_type: sample.dispel_type,
        persistence: Persistence::WholeMatch,
        riders: EngineSpec::no_riders(),
        provenance: "A Rogue carries this from the opening bell — it marks the coated \
                     weapon. The poison's effect comes from the coating's on-hit proc, \
                     not from this mark."
            .to_string(),
    })
}
