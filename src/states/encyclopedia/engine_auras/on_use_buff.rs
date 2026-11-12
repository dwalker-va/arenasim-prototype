//! The buff an item's ON-USE grants its wearer (a stat burst or haste, with
//! the heal over time one may carry as a rider: the Diamond Flask), named
//! after the item. One per item whose `on_use:` block grants a buff.

use super::{ArtKind, EngineKey, EngineSpec, SourceKind};
use crate::states::encyclopedia::auras::{AuraArt, AuraSource, CatalogSource, Persistence};
use crate::states::play_match::equipment::ItemId;
use crate::states::play_match::on_use_items::on_use_description;

pub const SOURCE: SourceKind = SourceKind::Item;
pub const ART: ArtKind = ArtKind::Item;

pub fn members(source: &CatalogSource) -> Vec<EngineKey> {
    ItemId::all()
        .iter()
        .filter(|id| {
            source
                .items
                .get(id)
                .is_some_and(|item| item.on_use_buff().is_some())
        })
        .map(|id| EngineKey::Item(*id))
        .collect()
}

pub fn entry(key: EngineKey, source: &CatalogSource) -> Option<EngineSpec> {
    let EngineKey::Item(item) = key else {
        return None;
    };
    let def = source.items.get(&item)?;
    let on_use = def.on_use_buff()?;
    // The auras the simulation lands, through its own constructors: the buff
    // is the face, and a heal over time beside it a rider, so the page states
    // both and the catalog keys both.
    let face = on_use.aura(item, &def.name);
    let riders = on_use
        .heal_aura(item, &def.name, bevy::prelude::Entity::PLACEHOLDER)
        .into_iter()
        .collect();
    Some(EngineSpec {
        name: def.name.clone(),
        frame_name: None,
        mechanic: face.effect_type,
        source: AuraSource::Mechanic("a trinket you are wearing, when you use it"),
        art: AuraArt::Item(item),
        magnitude: face.magnitude,
        school: None,
        break_on_damage: face.break_on_damage_threshold,
        dispel_type: face.dispel_type,
        persistence: Persistence::Seconds(face.duration),
        riders,
        landed: face.clone(),
        provenance: on_use_description(on_use),
    })
}
