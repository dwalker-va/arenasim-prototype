//! The slow a triggered Frost Trap's zone re-applies each tick. Frost Trap
//! places a zone rather than carrying an `applies_aura` block, so the slow is
//! the engine's.

use super::{has_ability, unit_if, ArtKind, EngineKey, EngineSpec, SourceKind};
use crate::states::encyclopedia::auras::{AuraArt, AuraSource, CatalogSource, Persistence};
use crate::states::play_match::abilities::{AbilityType, SpellSchool};
use crate::states::play_match::components::AuraType;
use crate::states::play_match::constants::{FROST_TRAP_SLOW_MAGNITUDE, FROST_TRAP_ZONE_DURATION};
use crate::states::play_match::traps::frost_trap_slow_aura;

pub const SOURCE: SourceKind = SourceKind::Ability;
pub const ART: ArtKind = ArtKind::FromSource;

pub fn members(source: &CatalogSource) -> Vec<EngineKey> {
    unit_if(has_ability(source, AbilityType::FrostTrap))
}

pub fn entry(key: EngineKey, source: &CatalogSource) -> Option<EngineSpec> {
    (key == EngineKey::Unit && has_ability(source, AbilityType::FrostTrap)).then(|| EngineSpec {
        name: "Frost Trap".to_string(),
        frame_name: None,
        mechanic: AuraType::MovementSpeedSlow,
        source: AuraSource::Ability(AbilityType::FrostTrap),
        art: AuraArt::FromSource,
        magnitude: FROST_TRAP_SLOW_MAGNITUDE,
        school: Some(SpellSchool::Frost),
        break_on_damage: -1.0,
        // The engine's own constructor decides the class, as for the totem
        // buffs.
        dispel_type: frost_trap_slow_aura(
            bevy::prelude::Entity::PLACEHOLDER,
            FROST_TRAP_SLOW_MAGNITUDE,
        )
        .dispel_type,
        persistence: Persistence::WhileSourceActive("while you stand in the zone"),
        riders: EngineSpec::no_riders(),
        provenance: format!(
            "Re-applied every tick by a triggered Frost Trap's slow zone, which itself \
             lasts {:.0} sec.",
            FROST_TRAP_ZONE_DURATION
        ),
    })
}
