//! The aura a TALENT's proc applies (Shadow Trance, Blackout's stun), named
//! after its effect: the talent and the proc's index among its `Proc` effects.
//! Derived from the talent table, so talent N+1's proc gets an entry with no
//! code here.

use super::{EngineKey, EngineSpec};
use crate::states::encyclopedia::auras::{AuraArt, AuraSource, CatalogSource, Persistence};
use crate::states::play_match::talent_procs::{effect_aura, ProcEvent, TalentProcSlot};
use crate::states::play_match::talents::TalentEffect;
use crate::states::talent_text::talent_proc_description;

pub fn members(source: &CatalogSource) -> Vec<EngineKey> {
    // Every talent proc that lands an aura, in `TalentId` order.
    let mut keys = Vec::new();
    for (talent, config) in source.talents.iter() {
        let procs = config
            .per_rank
            .iter()
            .filter(|e| matches!(e, TalentEffect::Proc(_)))
            .count();
        keys.extend((0..procs as u8).map(|i| EngineKey::TalentProc(talent, i)));
    }
    keys
}

pub fn entry(key: EngineKey, source: &CatalogSource) -> Option<EngineSpec> {
    let EngineKey::TalentProc(talent, index) = key else {
        return None;
    };
    let config = source.talents.get(talent)?;
    let proc = config
        .per_rank
        .iter()
        .filter_map(|e| match e {
            TalentEffect::Proc(p) => Some(p),
            _ => None,
        })
        .nth(usize::from(index))?;
    // Built by the constructor the resolver lands it with, so the page states
    // the aura that lands.
    let slot = TalentProcSlot::new(talent, &config.name, proc.clone());
    let event = ProcEvent::new(bevy::prelude::Entity::PLACEHOLDER, proc.trigger)
        .other(bevy::prelude::Entity::PLACEHOLDER)
        .amount(1.0);
    let landed = effect_aura(&slot, bevy::prelude::Entity::PLACEHOLDER, &event)?.aura;
    Some(EngineSpec {
        name: landed.ability_name.clone(),
        frame_name: None,
        mechanic: landed.effect_type,
        source: AuraSource::Mechanic("a talent"),
        // A talent's proc wears its talent's icon, as in the buff bar, which
        // reads the same key off the aura's `source_talent`.
        art: AuraArt::Talent(talent),
        magnitude: landed.magnitude,
        school: None,
        break_on_damage: landed.break_on_damage_threshold,
        dispel_type: landed.dispel_type,
        persistence: Persistence::Seconds(landed.duration),
        riders: EngineSpec::no_riders(),
        provenance: talent_proc_description(&config.name, proc, source.abilities),
    })
}
