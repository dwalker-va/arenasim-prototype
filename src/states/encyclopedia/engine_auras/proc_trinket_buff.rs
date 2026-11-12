//! The stat buff a PROC item grants its wearer, named after the item. One per
//! item whose `proc:` block grants a buff, so trinket N+1 needs no code here.

use super::{EngineKey, EngineSpec};
use crate::states::encyclopedia::auras::{AuraArt, AuraSource, CatalogSource, Persistence};
use crate::states::play_match::equipment::ItemId;
use crate::states::play_match::proc_trinkets::proc_description;

pub fn members(source: &CatalogSource) -> Vec<EngineKey> {
    // Walked in `ItemId::all()` order rather than `items.iter()` order:
    // `ItemDefinitions` is backed by a `HashMap`.
    ItemId::all()
        .iter()
        .filter(|id| {
            source
                .items
                .get(id)
                .is_some_and(|item| item.proc_buff().is_some())
        })
        .map(|id| EngineKey::Item(*id))
        .collect()
}

pub fn entry(key: EngineKey, source: &CatalogSource) -> Option<EngineSpec> {
    let EngineKey::Item(item) = key else {
        return None;
    };
    let def = source.items.get(&item)?;
    let proc = def.proc_buff()?;
    Some(EngineSpec {
        name: def.name.clone(),
        frame_name: None,
        mechanic: proc.buff()?,
        // The trinket is not an ability and has no page of its own to link to
        // from here; the ITEM's page is where its numbers live, and the
        // provenance line below names it.
        source: AuraSource::Mechanic("a proc trinket you are wearing"),
        // No ability applies it; the item does, and the buff wears the item's
        // own icon — as it does in the buff bar, which reads the same key off
        // the aura's `source_item`.
        art: AuraArt::Item(item),
        magnitude: proc.magnitude,
        school: None,
        // A proc buff is never broken by damage — see `ProcConfig::aura`.
        break_on_damage: -1.0,
        dispel_type: proc.aura(item, &def.name).dispel_type,
        persistence: Persistence::Seconds(proc.duration),
        riders: EngineSpec::no_riders(),
        // Read straight off the same `ProcConfig` the simulation rolls
        // against, so the page cannot state a chance, a duration or a cooldown
        // the trinket does not have. The trinket is not named again here —
        // the page header IS its name.
        provenance: proc_description(proc),
    })
}
