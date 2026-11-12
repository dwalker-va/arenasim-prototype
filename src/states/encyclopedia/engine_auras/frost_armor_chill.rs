//! The chill a Frost Armor proc hangs on a melee attacker — ONE debuff doing
//! two things (a movement slow and an attack-speed slow), so ONE entry. It was
//! two entries wearing different removal badges until the engine learned to
//! bind the two effects into one debuff; see
//! [`CompoundDebuff`](crate::states::play_match::components::CompoundDebuff).

use super::{has_ability, unit_if, ArtKind, EngineKey, EngineSpec, SourceKind};
use crate::states::encyclopedia::auras::{AuraArt, AuraSource, CatalogSource, Persistence};
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::combat_core::frost_armor_chill_auras;

pub const SOURCE: SourceKind = SourceKind::Ability;
pub const ART: ArtKind = ArtKind::FromSource;

pub fn members(source: &CatalogSource) -> Vec<EngineKey> {
    unit_if(has_ability(source, AbilityType::FrostArmor))
}

pub fn entry(key: EngineKey, source: &CatalogSource) -> Option<EngineSpec> {
    if key != EngineKey::Unit || !has_ability(source, AbilityType::FrostArmor) {
        return None;
    }
    // Built from the simulation's own constructor for the whole debuff, so
    // the page cannot list an effect the proc does not apply — or miss one it
    // does.
    let [face, rider] = frost_armor_chill_auras();
    Some(EngineSpec {
        // No disambiguating qualifier on the mechanic: there is one chill now,
        // and the only other "Frost Armor" is the Mage's self-buff, which is a
        // BUFF and files on the other side of the catalog.
        name: "Frost Armor (chill)".to_string(),
        frame_name: Some(face.ability_name.clone()),
        mechanic: face.effect_type,
        source: AuraSource::Ability(AbilityType::FrostArmor),
        art: AuraArt::FromSource,
        magnitude: face.magnitude,
        school: face.spell_school,
        break_on_damage: face.break_on_damage_threshold,
        dispel_type: face.dispel_type,
        persistence: Persistence::Seconds(face.duration),
        riders: vec![rider],
        provenance: "Hung on any melee attacker who strikes a Mage wearing Frost Armor. \
                     One debuff doing two things — a dispel that lifts it takes both. It \
                     does not stack with itself: a second hit while it is up refreshes \
                     nothing."
            .to_string(),
    })
}
