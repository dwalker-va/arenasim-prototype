//! The buff one element's totem pulses onto nearby allies, one per element
//! whose totem the ability table holds — so a fifth element needs no code
//! here.

use super::{has_ability, EngineKey, EngineSpec};
use crate::states::encyclopedia::auras::{AuraArt, AuraSource, CatalogSource, Persistence};
use crate::states::play_match::class_ai::shaman::totem_spec;
use crate::states::play_match::components::TotemElement;
use crate::states::play_match::constants::TOTEM_DURATION;
use crate::states::play_match::totems::make_totem_aura;

pub fn members(source: &CatalogSource) -> Vec<EngineKey> {
    TotemElement::ALL
        .iter()
        .filter(|element| has_ability(source, totem_spec(**element).0))
        .map(|element| EngineKey::Totem(*element))
        .collect()
}

pub fn entry(key: EngineKey, source: &CatalogSource) -> Option<EngineSpec> {
    let EngineKey::Totem(element) = key else {
        return None;
    };
    let (ability, aura_type, magnitude, school) = totem_spec(element);
    has_ability(source, ability).then(|| EngineSpec {
        name: element.buff_name().to_string(),
        frame_name: None,
        mechanic: aura_type,
        source: AuraSource::Ability(ability),
        art: AuraArt::FromSource,
        magnitude,
        school: Some(school),
        break_on_damage: -1.0,
        // The engine's own constructor decides the class, so the page cannot
        // disagree with the buff a totem actually pulses.
        dispel_type: make_totem_aura(
            aura_type,
            magnitude,
            bevy::prelude::Entity::PLACEHOLDER,
            school,
            element.buff_name(),
        )
        .dispel_type,
        persistence: Persistence::WhileSourceActive("while you stand near the totem"),
        riders: EngineSpec::no_riders(),
        provenance: format!(
            "Pulsed onto nearby allies by a dropped totem, which itself lasts {:.0} sec.",
            TOTEM_DURATION
        ),
    })
}
