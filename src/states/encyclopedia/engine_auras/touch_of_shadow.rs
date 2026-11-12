//! Touch of Shadow, the buff a Demonic Sacrifice grants for the match. Listed
//! while the talent table holds a node with the mechanic.

use super::{unit_if, ArtKind, EngineKey, EngineSpec, SourceKind};
use crate::states::encyclopedia::auras::{AuraArt, AuraSource, CatalogSource, Persistence};
use crate::states::play_match::abilities::SpellSchool;
use crate::states::play_match::demonology;
use crate::states::play_match::talents::TalentId;

pub const SOURCE: SourceKind = SourceKind::Talent;
pub const ART: ArtKind = ArtKind::Talent;

pub fn members(source: &CatalogSource) -> Vec<EngineKey> {
    unit_if(demonology::demonic_sacrifice_in(source.talents).is_some())
}

pub fn entry(key: EngineKey, source: &CatalogSource) -> Option<EngineSpec> {
    if key != EngineKey::Unit {
        return None;
    }
    let bonus = demonology::demonic_sacrifice_in(source.talents)?;
    let sample = demonology::touch_of_shadow_aura(bonus, bevy::prelude::Entity::PLACEHOLDER);
    Some(EngineSpec {
        name: sample.ability_name.clone(),
        frame_name: None,
        mechanic: sample.effect_type,
        source: AuraSource::Mechanic("the Warlock's Demonic Sacrifice talent"),
        // Wears its node's icon, keyed off the aura's `source_talent` as in
        // the buff bar.
        art: AuraArt::Talent(TalentId::DemonicSacrifice),
        magnitude: sample.magnitude,
        school: Some(SpellSchool::Shadow),
        break_on_damage: sample.break_on_damage_threshold,
        dispel_type: sample.dispel_type,
        persistence: Persistence::WholeMatch,
        riders: EngineSpec::no_riders(),
        provenance: format!(
            "Granted at the gates to a Warlock that sacrifices its Felhunter: {:.0}% more \
             Shadow damage for the match.",
            bonus * 100.0
        ),
    })
}
