//! The aura a TALENT's proc applies (Shadow Trance, Blackout's stun), named
//! after its effect: the talent and the proc's index among its `Proc` effects.
//! Derived from the talent table, so talent N+1's proc gets an entry with no
//! code here.

use super::{ArtKind, EngineKey, EngineSpec, SourceKind};
use crate::states::encyclopedia::auras::{AuraArt, AuraSource, CatalogSource, Persistence};
use crate::states::play_match::talent_procs::{effect_aura, ProcEvent, TalentProc, TalentProcSlot};
use crate::states::play_match::talents::{TalentConfig, TalentEffect};
use crate::states::talent_text::talent_procs_description;

pub const SOURCE: SourceKind = SourceKind::Talent;
pub const ART: ArtKind = ArtKind::Talent;

/// A talent's procs, in its `per_rank` order.
fn procs_of(config: &TalentConfig) -> Vec<&TalentProc> {
    config
        .per_rank
        .iter()
        .filter_map(|e| match e {
            TalentEffect::Proc(p) => Some(p),
            _ => None,
        })
        .collect()
}

pub fn members(source: &CatalogSource) -> Vec<EngineKey> {
    // Every aura a talent proc lands, in `TalentId` order. Procs of one talent
    // that land the same aura on different events (Vengeance) are one entry,
    // keyed by the first of them.
    let mut keys = Vec::new();
    for (talent, config) in source.talents.iter() {
        let procs = procs_of(config);
        keys.extend(
            procs
                .iter()
                .enumerate()
                .filter(|(i, p)| !procs[..*i].iter().any(|q| q.effect == p.effect))
                .map(|(i, _)| EngineKey::TalentProc(talent, i as u8)),
        );
    }
    keys
}

pub fn entry(key: EngineKey, source: &CatalogSource) -> Option<EngineSpec> {
    let EngineKey::TalentProc(talent, index) = key else {
        return None;
    };
    let config = source.talents.get(talent)?;
    let procs = procs_of(config);
    let proc = *procs.get(usize::from(index))?;
    // Every proc of the talent landing this aura, so the page names each
    // event that lands it.
    let landing: Vec<&TalentProc> = procs
        .iter()
        .copied()
        .filter(|p| p.effect == proc.effect)
        .collect();
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
        landed: landed.clone(),
        provenance: talent_procs_description(&config.name, &landing, source.abilities),
    })
}
