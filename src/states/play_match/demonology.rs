//! The Demonology tree's sites: Master Demonologist and Demonic Sacrifice.
//!
//! Both are `Mechanic` talents (`talent_mechanics`) whose one site is
//! [`demonic_bonds`], a Phase 1 system that runs right after the countdown:
//!
//! - **Master Demonologist** keeps a Spells-scoped `DamageTakenReduction` aura
//!   on its holder and on the holder's pet for as long as the pet lives (from
//!   the first frame, the countdown included), and lifts the holder's the
//!   frame after the pet dies.
//! - **Demonic Sacrifice** lands at the gates, on a holder whose demon choice
//!   is `Sacrifice`: its Felhunter, which spawned as usual, dies (it plays the
//!   ordinary death animation and is never despawned), and the holder gains
//!   Touch of Shadow, a Shadow-scoped `DamageDealtIncrease` for the match.
//!
//! **The sacrifice is not a kill.** The Felhunter's health goes to zero
//! outside the damage pipeline, so no damage event, killing blow or killer is
//! recorded, and its death line is logged without the structured death record
//! (`CombatLog::log_death`) the Results screen and `combatant_survived` read
//! — the shape `despawn_pets_of_dead_owners` gives a pet whose owner died.
//! `check_match_end` counts only non-pet combatants, so a 1v1 goes on; and
//! kill-call banter follows the player's call on non-pet combatants, which a
//! pet's death does not touch.
//!
//! Soul Link, the tree's capstone, needs no site here: its transfer is a step
//! of `combat_core::apply_damage_with_absorb`, and the `Sacrifice` choice
//! switches it off for the match (`talents.ron`'s `choice_rules`).
//!
//! Both auras are TALENT auras ([`talent_aura`]): source-keyed by their
//! talent, so each sits
//! beside a same-type aura from anywhere else (Devotion Aura beside Master
//! Demonologist) and the damage pipeline takes the strongest; undispellable;
//! and outlasting the match. A build without these talents meets none of
//! this: the system finds nothing to do.

use std::collections::BTreeMap;

use bevy::prelude::*;

use super::abilities::SpellSchool;
use super::components::{
    ActiveAuras, Aura, AuraDamageMods, AuraPending, AuraType, CastingState, ChannelingState,
    Combatant, DamageScope, DispelType, MatchCountdown, Pet,
};
use super::talent_mechanics::TalentMechanic;
use super::talents::{TalentDefinitions, TalentEffect, TalentId};
use super::utils::{combatant_id, pet_combatant_id};
use crate::combat::log::{CombatLog, CombatLogEventType};

/// The name Master Demonologist's aura carries, on the Warlock and its pet.
pub const MASTER_DEMONOLOGIST_NAME: &str = "Master Demonologist";

/// The name of the buff Demonic Sacrifice grants.
pub const TOUCH_OF_SHADOW_NAME: &str = "Touch of Shadow";

/// A talent aura's duration: past any match (the 300s cap and the 10s
/// countdown), as Devotion Aura's 600s is.
pub const TALENT_AURA_DURATION: f32 = 600.0;

/// `talent`'s passive aura: `kind` at `magnitude`, reaching the hits in
/// `scope`, for the match.
///
/// It carries its talent ([`Aura::source_talent`]), which keys it by its
/// SOURCE rather than its type, so it neither refuses nor is refused by a
/// same-type aura from elsewhere (a Paladin's Devotion Aura lands beside
/// Master Demonologist, and the pipeline applies the stronger), and wears the
/// talent's icon. Untyped, so no dispel or purge takes it.
pub fn talent_aura(
    talent: TalentId,
    name: &str,
    kind: AuraType,
    magnitude: f32,
    scope: DamageScope,
    caster: Entity,
) -> Aura {
    Aura {
        effect_type: kind,
        duration: TALENT_AURA_DURATION,
        magnitude,
        break_on_damage_threshold: -1.0,
        caster: Some(caster),
        damage_mods: AuraDamageMods {
            scope: Some(scope),
            ..Default::default()
        },
        ability_name: name.to_string(),
        dispel_type: DispelType::Untyped,
        source_talent: Some(talent),
        ..Default::default()
    }
}

/// Master Demonologist's aura: `reduction` less spell damage taken.
pub fn master_demonologist_aura(reduction: f32, caster: Entity) -> Aura {
    talent_aura(
        TalentId::MasterDemonologist,
        MASTER_DEMONOLOGIST_NAME,
        AuraType::DamageTakenReduction,
        reduction,
        DamageScope::Spells,
        caster,
    )
}

/// Touch of Shadow: `bonus` more Shadow damage dealt.
pub fn touch_of_shadow_aura(bonus: f32, caster: Entity) -> Aura {
    talent_aura(
        TalentId::DemonicSacrifice,
        TOUCH_OF_SHADOW_NAME,
        AuraType::DamageDealtIncrease,
        bonus,
        DamageScope::School(SpellSchool::Shadow),
        caster,
    )
}

/// The reduction `talents`' Master Demonologist node grants at its full rank
/// (what the encyclopedia's page prints), or `None` for a table without one.
pub fn master_demonologist_in(talents: &TalentDefinitions) -> Option<f32> {
    mechanic_in(talents, |m| match m {
        TalentMechanic::MasterDemonologist { reduction } => Some(reduction),
        _ => None,
    })
}

/// The Shadow damage bonus `talents`' Demonic Sacrifice node grants, or
/// `None` for a table without one.
pub fn demonic_sacrifice_in(talents: &TalentDefinitions) -> Option<f32> {
    mechanic_in(talents, |m| match m {
        TalentMechanic::DemonicSacrifice { shadow_damage } => Some(shadow_damage),
        _ => None,
    })
}

/// The first of `talents`' nodes whose mechanic `pick` reads, at the node's
/// full rank.
fn mechanic_in(
    talents: &TalentDefinitions,
    pick: impl Fn(TalentMechanic) -> Option<f32>,
) -> Option<f32> {
    talents.iter().find_map(|(_, node)| {
        node.per_rank.iter().find_map(|effect| match effect {
            TalentEffect::Mechanic(m) => pick(m.at_rank(node.max_rank)),
            _ => None,
        })
    })
}

fn has_aura(auras: Option<&ActiveAuras>, name: &str) -> bool {
    auras.is_some_and(|a| a.auras.iter().any(|aura| aura.ability_name == name))
}

/// Master Demonologist and Demonic Sacrifice, each frame from the first:
/// keep each Master Demonologist's aura on its holder and its pet while the
/// pet lives (and lift the holder's once it has died), and land each pending
/// sacrifice on the frame the gates open. A no-op for every combatant whose
/// build carries neither.
#[allow(clippy::type_complexity)]
pub fn demonic_bonds(
    mut commands: Commands,
    countdown: Res<MatchCountdown>,
    mut combat_log: ResMut<CombatLog>,
    mut owners: Query<(Entity, &mut Combatant, Option<&mut ActiveAuras>), Without<Pet>>,
    mut pets: Query<(Entity, &Pet, &mut Combatant, Option<&ActiveAuras>)>,
) {
    // Owners in (team, slot) order, so the log reads the same whatever the
    // ECS iteration order.
    let mut holders: Vec<(u8, u8, Entity)> = owners
        .iter()
        .filter(|(_, c, _)| {
            c.sacrifice_pending || c.talent_mechanics.master_demonologist().is_some()
        })
        .map(|(e, c, _)| (c.team, c.slot, e))
        .collect();
    if holders.is_empty() {
        return;
    }
    holders.sort();
    let pet_of: BTreeMap<Entity, Entity> = pets
        .iter()
        .map(|(entity, pet, _, _)| (pet.owner, entity))
        .collect();

    for (_, _, owner_entity) in holders {
        let Ok((_, mut owner, mut owner_auras)) = owners.get_mut(owner_entity) else {
            continue;
        };
        if !owner.is_alive() {
            continue;
        }
        let owner_id = combatant_id(owner.team, owner.slot, owner.class);
        let pet_entity = pet_of.get(&owner_entity).copied();

        if owner.sacrifice_pending && countdown.gates_opened {
            owner.sacrifice_pending = false;
            let mut sacrificed = "its demon".to_string();
            if let Some(Ok((entity, pet, mut pet_combatant, _))) =
                pet_entity.map(|e| pets.get_mut(e))
            {
                if pet_combatant.is_alive() {
                    let pet_id = pet_combatant_id(
                        pet_combatant.team,
                        pet_combatant.owner_relative_slot(),
                        pet.pet_type,
                    );
                    pet_combatant.current_health = 0.0;
                    pet_combatant.is_dead = true;
                    commands.entity(entity).remove::<CastingState>();
                    commands.entity(entity).remove::<ChannelingState>();
                    // A plain line, not `log_death`: the sacrifice is no kill.
                    combat_log.log(
                        CombatLogEventType::Death,
                        format!("{pet_id} is sacrificed by {owner_id}"),
                    );
                    sacrificed = format!("its {}", pet.pet_type.name());
                }
            }
            let bonus = owner.talent_mechanics.demonic_sacrifice().unwrap_or(0.0);
            if bonus > 0.0 {
                commands.spawn(AuraPending {
                    target: owner_entity,
                    aura: touch_of_shadow_aura(bonus, owner_entity),
                });
            }
            combat_log.log(
                CombatLogEventType::MatchEvent,
                format!(
                    "[TALENTS] {owner_id} sacrifices {sacrificed} (Demonic Sacrifice): \
                     {TOUCH_OF_SHADOW_NAME}, +{:.0}% Shadow damage",
                    bonus * 100.0
                ),
            );
        }

        let Some(reduction) = owner.talent_mechanics.master_demonologist() else {
            continue;
        };
        let living_pet = pet_entity.and_then(|e| {
            pets.get(e)
                .ok()
                .filter(|(_, _, c, _)| c.is_alive())
                .map(|(entity, _, _, auras)| (entity, has_aura(auras, MASTER_DEMONOLOGIST_NAME)))
        });
        let owner_has = has_aura(owner_auras.as_deref(), MASTER_DEMONOLOGIST_NAME);
        match living_pet {
            Some((pet, pet_has)) => {
                if !owner_has {
                    commands.spawn(AuraPending {
                        target: owner_entity,
                        aura: master_demonologist_aura(reduction, owner_entity),
                    });
                }
                if !pet_has {
                    commands.spawn(AuraPending {
                        target: pet,
                        aura: master_demonologist_aura(reduction, owner_entity),
                    });
                }
            }
            None if owner_has => {
                if let Some(auras) = owner_auras.as_deref_mut() {
                    auras
                        .auras
                        .retain(|a| a.ability_name != MASTER_DEMONOLOGIST_NAME);
                }
                combat_log.log(
                    CombatLogEventType::MatchEvent,
                    format!(
                        "[TALENTS] {owner_id}'s {MASTER_DEMONOLOGIST_NAME} ends: its demon is dead"
                    ),
                );
            }
            None => {}
        }
    }
}
