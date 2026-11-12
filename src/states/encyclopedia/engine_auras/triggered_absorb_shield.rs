//! The absorb shield an item's TRIGGERED ABSORB raises (Aegis of the Last
//! Stand), named after the item. One per item carrying a `triggered_absorb:`
//! block.

use super::{ArtKind, EngineKey, EngineSpec, SourceKind};
use crate::states::encyclopedia::auras::{AuraArt, AuraSource, CatalogSource, Persistence};
use crate::states::play_match::equipment::ItemId;
use crate::states::play_match::triggered_absorbs::{triggered_absorb_description, ShieldFrom};

pub const SOURCE: SourceKind = SourceKind::Item;
pub const ART: ArtKind = ArtKind::Item;

pub fn members(source: &CatalogSource) -> Vec<EngineKey> {
    ItemId::all()
        .iter()
        .filter(|id| {
            source
                .items
                .get(id)
                .is_some_and(|item| item.triggered_absorb.is_some())
        })
        .map(|id| EngineKey::Item(*id))
        .collect()
}

pub fn entry(key: EngineKey, source: &CatalogSource) -> Option<EngineSpec> {
    let EngineKey::Item(item) = key else {
        return None;
    };
    let def = source.items.get(&item)?;
    let absorb = &def.triggered_absorb.as_ref()?.absorb;
    // The aura the simulation raises, before dampening: its size on a
    // reference combatant is the item's to state, so the page reads its
    // mechanic, removal and duration off the real constructor.
    let sample = absorb.shield_aura(&def.name, ShieldFrom::Item(item), 0.0);
    Some(EngineSpec {
        name: def.name.clone(),
        frame_name: None,
        mechanic: sample.effect_type,
        source: AuraSource::Mechanic("a trinket you are wearing"),
        art: AuraArt::Item(item),
        magnitude: 0.0,
        school: None,
        break_on_damage: sample.break_on_damage_threshold,
        dispel_type: sample.dispel_type,
        persistence: Persistence::Seconds(absorb.duration),
        riders: EngineSpec::no_riders(),
        provenance: triggered_absorb_description(absorb),
    })
}
