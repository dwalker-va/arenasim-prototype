//! Master Demonologist's spell damage cut, which the talent keeps on the
//! Warlock and its Felhunter while the Felhunter lives. Listed while the
//! talent table holds a node with the mechanic.

use super::{unit_if, ArtKind, EngineKey, EngineSpec, SourceKind};
use crate::states::encyclopedia::auras::{AuraArt, AuraSource, CatalogSource, Persistence};
use crate::states::play_match::demonology;
use crate::states::play_match::talents::TalentId;

pub const SOURCE: SourceKind = SourceKind::Talent;
pub const ART: ArtKind = ArtKind::Talent;

pub fn members(source: &CatalogSource) -> Vec<EngineKey> {
    unit_if(demonology::master_demonologist_in(source.talents).is_some())
}

pub fn entry(key: EngineKey, source: &CatalogSource) -> Option<EngineSpec> {
    if key != EngineKey::Unit {
        return None;
    }
    let reduction = demonology::master_demonologist_in(source.talents)?;
    let sample =
        demonology::master_demonologist_aura(reduction, bevy::prelude::Entity::PLACEHOLDER);
    Some(EngineSpec {
        name: sample.ability_name.clone(),
        frame_name: None,
        mechanic: sample.effect_type,
        source: AuraSource::Mechanic("the Warlock's Master Demonologist talent"),
        // Wears its node's icon, keyed off the aura's `source_talent` as in
        // the buff bar.
        art: AuraArt::Talent(TalentId::MasterDemonologist),
        magnitude: sample.magnitude,
        school: None,
        break_on_damage: sample.break_on_damage_threshold,
        dispel_type: sample.dispel_type,
        persistence: Persistence::WholeMatch,
        riders: EngineSpec::no_riders(),
        landed: sample.clone(),
        provenance: format!(
            "The Warlock and its Felhunter each take {:.0}% less spell damage while the \
             Felhunter lives. The Warlock's ends when the Felhunter dies.",
            reduction * 100.0
        ),
    })
}
