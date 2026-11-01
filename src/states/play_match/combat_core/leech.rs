//! Leech: a share of the damage a caster deals heals the caster, or the
//! caster's party (AS-331).
//!
//! The `leech` field of an ability's `applies_aura` block ([`Leech`]). Two
//! sources ([`LeechSource`]):
//!
//! - **`Ability`** — the ability's OWN damage: the hit that applies the aura
//!   and the aura's periodic ticks. Death Coil (its direct hit, every point
//!   back to the Warlock) and Siphon Life (its ticks).
//! - **`CasterSchool(school)`** — while the aura is on its bearer, every
//!   damage of that school ITS CASTER deals the bearer. Vampiric Embrace: 20%
//!   of the Priest's Shadow damage to the target heals the Priest's party.
//!
//! Every leech heal lands through `apply_healing` ([`land`]), so healing
//! reduction, a Cyclone and arena dampening cut it like any other heal. Its
//! amount is the damage that reached health: an absorbed hit feeds nothing.
//!
//! **Where it is read.** The sites a caster's spell damage lands at: a cast
//! completing (`process_casting`), a projectile arriving
//! (`process_projectile_hits`), a DoT tick (`process_dot_ticks`) and a channel
//! tick (`process_channeling`). A melee ability or an auto-attack reads no
//! leech: no ability leeches physical damage.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::super::abilities::SpellSchool;
use super::super::components::{ActiveAuras, ArenaDampening, Combatant};
use super::healing::apply_healing;
use crate::combat::log::{CombatLog, CombatantId};

/// An ability's leech, as its `applies_aura` block declares it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Leech {
    /// The share of the damage that heals, in `(0, 1]`.
    pub fraction: f32,
    /// Who it heals.
    #[serde(default)]
    pub to: LeechRecipient,
    /// Which damage feeds it.
    #[serde(default)]
    pub of: LeechSource,
}

/// Who a leech heals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LeechRecipient {
    /// The caster alone.
    #[default]
    Caster,
    /// Every living teammate of the caster, the caster included (pets are not
    /// party members).
    CasterParty,
}

/// Which damage feeds a leech.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LeechSource {
    /// The ability's own damage: its direct hit and its aura's ticks.
    #[default]
    Ability,
    /// While its aura is on the bearer, every damage of this school the aura's
    /// caster deals the bearer.
    CasterSchool(SpellSchool),
}

impl Leech {
    /// Reject a leech whose share is not in `(0, 1]`.
    pub fn validate(&self) -> Result<(), String> {
        if !(self.fraction > 0.0 && self.fraction <= 1.0) {
            return Err(format!(
                "a leech fraction of {} is outside (0, 1]",
                self.fraction
            ));
        }
        Ok(())
    }
}

/// One heal a leech owes: its source's name, who it goes to, and how much.
#[derive(Clone, Debug, PartialEq)]
pub struct LeechHeal {
    pub source: String,
    pub to: LeechRecipient,
    pub amount: f32,
}

/// The heals one landed damage event owes, from `attacker` to a bearer
/// carrying `target_auras`, of `school`, `damage` of it reaching health:
/// - `own`, the damaging ability's own leech with its name, when its source
///   is [`LeechSource::Ability`];
/// - every [`LeechSource::CasterSchool`] leech on the bearer that `attacker`
///   placed and whose school is `school`.
///
/// Empty when nothing leeches, which is every event of a match without a
/// leech (Death Coil's included only when Death Coil hit).
pub fn owed(
    attacker: Entity,
    own: Option<(&Leech, &str)>,
    target_auras: Option<&ActiveAuras>,
    school: SpellSchool,
    damage: f32,
) -> Vec<LeechHeal> {
    if damage <= 0.0 {
        return Vec::new();
    }
    let mut heals = Vec::new();
    if let Some((leech, name)) = own {
        if leech.of == LeechSource::Ability {
            heals.push(LeechHeal {
                source: name.to_string(),
                to: leech.to,
                amount: damage * leech.fraction,
            });
        }
    }
    for aura in target_auras.into_iter().flat_map(|a| a.auras.iter()) {
        let Some(leech) = aura.leech else { continue };
        if aura.caster == Some(attacker) && leech.of == LeechSource::CasterSchool(school) {
            heals.push(LeechHeal {
                source: aura.ability_name.clone(),
                to: leech.to,
                amount: damage * leech.fraction,
            });
        }
    }
    heals
}

/// Who `to` heals, among `members` — `(entity, team, is_pet, alive)` for
/// every combatant — for a caster `caster` of `team`. In `members` order.
pub fn recipients(
    to: LeechRecipient,
    caster: Entity,
    team: u8,
    members: impl IntoIterator<Item = (Entity, u8, bool, bool)>,
) -> Vec<Entity> {
    match to {
        LeechRecipient::Caster => vec![caster],
        LeechRecipient::CasterParty => members
            .into_iter()
            .filter(|(_, t, is_pet, alive)| *t == team && !is_pet && *alive)
            .map(|(e, ..)| e)
            .collect(),
    }
}

/// Land one leech heal on `recipient`, through `apply_healing`, and log it.
/// Returns the effective healing, for the caster's `healing_done`.
///
/// A heal to the caster logs `"<caster>'s <source> heals for N"` (Death Coil's
/// line since it shipped); a heal to anyone else names them.
#[allow(clippy::too_many_arguments)]
pub fn land(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    dampening: &ArenaDampening,
    caster_id: &CombatantId,
    heal: &LeechHeal,
    recipient_entity: Entity,
    recipient: &mut Combatant,
    recipient_auras: Option<&ActiveAuras>,
    recipient_id: &CombatantId,
) -> f32 {
    if !recipient.is_alive() {
        return 0.0;
    }
    let effective = apply_healing(
        commands,
        recipient_entity,
        recipient,
        recipient_auras,
        dampening,
        heal.amount,
    );
    if effective > 0.0 {
        let message = if recipient_id == caster_id {
            format!("{}'s {} heals for {:.0}", caster_id, heal.source, effective)
        } else {
            format!(
                "{}'s {} heals {} for {:.0}",
                caster_id, heal.source, recipient_id, effective
            )
        };
        combat_log.log_healing(
            caster_id.clone(),
            recipient_id.clone(),
            heal.source.clone(),
            effective,
            false,
            message,
        );
    }
    effective
}

/// One damage event's leech heals, still to land: what [`owed`] returned,
/// with the caster it came from.
#[derive(Clone, Debug)]
pub struct OwedLeech {
    pub caster: Entity,
    pub caster_team: u8,
    pub caster_id: CombatantId,
    pub heals: Vec<LeechHeal>,
}

impl OwedLeech {
    /// Whether a heal goes to the caster's party, so its site must list the
    /// party's members.
    pub fn needs_party(&self) -> bool {
        self.heals
            .iter()
            .any(|h| h.to == LeechRecipient::CasterParty)
    }
}

/// Land every heal of `owed` on its recipients ([`recipients`]), each through
/// [`land`], and return what landed, for the caster's `healing_done`.
///
/// A site hands each recipient over through `visit`: called with the
/// recipient's entity and a lander, it calls the lander with that unit's
/// combatant, auras and combat-log id, and returns what it returned (0 for a
/// unit it cannot find). `members` lists every combatant as `(entity, team,
/// is_pet, alive)` when [`OwedLeech::needs_party`], and may be empty when not.
pub fn land_owed(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    dampening: &ArenaDampening,
    owed: &OwedLeech,
    members: &[(Entity, u8, bool, bool)],
    mut visit: impl FnMut(
        Entity,
        &mut dyn FnMut(&mut Combatant, Option<&ActiveAuras>, &CombatantId) -> f32,
    ) -> f32,
) -> f32 {
    let mut credited = 0.0;
    for heal in &owed.heals {
        for recipient in recipients(
            heal.to,
            owed.caster,
            owed.caster_team,
            members.iter().copied(),
        ) {
            credited += visit(recipient, &mut |combatant, auras, id| {
                land(
                    commands,
                    combat_log,
                    dampening,
                    &owed.caster_id,
                    heal,
                    recipient,
                    combatant,
                    auras,
                    id,
                )
            });
        }
    }
    credited
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::components::{Aura, AuraType};

    fn e(n: u32) -> Entity {
        Entity::from_raw(n)
    }

    #[test]
    fn an_ability_leech_heals_its_share_of_the_damage_that_landed() {
        let coil = Leech {
            fraction: 1.0,
            to: LeechRecipient::Caster,
            of: LeechSource::Ability,
        };
        let heals = owed(
            e(1),
            Some((&coil, "Death Coil")),
            None,
            SpellSchool::Shadow,
            37.0,
        );
        assert_eq!(
            heals,
            vec![LeechHeal {
                source: "Death Coil".into(),
                to: LeechRecipient::Caster,
                amount: 37.0
            }]
        );
        assert!(owed(
            e(1),
            Some((&coil, "Death Coil")),
            None,
            SpellSchool::Shadow,
            0.0
        )
        .is_empty());
    }

    /// Vampiric Embrace: only its own caster's damage, only of its school,
    /// feeds it.
    #[test]
    fn a_caster_school_leech_is_fed_by_its_casters_damage_of_its_school() {
        let embrace = Aura {
            effect_type: AuraType::DamageOverTime,
            caster: Some(e(1)),
            ability_name: "Test".into(),
            leech: Some(Leech {
                fraction: 0.2,
                to: LeechRecipient::CasterParty,
                of: LeechSource::CasterSchool(SpellSchool::Shadow),
            }),
            ..Default::default()
        };
        let auras = ActiveAuras {
            auras: vec![embrace],
        };
        let heals = owed(e(1), None, Some(&auras), SpellSchool::Shadow, 50.0);
        assert_eq!(heals.len(), 1);
        assert_eq!(heals[0].source, "Test", "named after the aura");
        assert!((heals[0].amount - 10.0).abs() < 1e-6);
        assert_eq!(heals[0].to, LeechRecipient::CasterParty);
        assert!(owed(e(2), None, Some(&auras), SpellSchool::Shadow, 50.0).is_empty());
        assert!(owed(e(1), None, Some(&auras), SpellSchool::Fire, 50.0).is_empty());
        // An Ability-source leech on the aura is not fed by other damage.
        let mut own = ActiveAuras {
            auras: auras.auras.clone(),
        };
        own.auras[0].leech = Some(Leech {
            fraction: 1.0,
            to: LeechRecipient::Caster,
            of: LeechSource::Ability,
        });
        assert!(owed(e(1), None, Some(&own), SpellSchool::Shadow, 50.0).is_empty());
    }

    #[test]
    fn a_party_is_the_casters_living_teammates_without_pets() {
        let members = [
            (e(1), 1, false, true),
            (e(2), 1, false, true),
            (e(3), 1, true, true),
            (e(4), 1, false, false),
            (e(5), 2, false, true),
        ];
        assert_eq!(
            recipients(LeechRecipient::CasterParty, e(1), 1, members),
            vec![e(1), e(2)]
        );
        assert_eq!(
            recipients(LeechRecipient::Caster, e(1), 1, members),
            vec![e(1)]
        );
    }

    #[test]
    fn validate_holds_the_fraction_to_a_share() {
        let mut l = Leech {
            fraction: 0.2,
            to: LeechRecipient::Caster,
            of: LeechSource::Ability,
        };
        assert!(l.validate().is_ok());
        l.fraction = 0.0;
        assert!(l.validate().is_err());
        l.fraction = 1.5;
        assert!(l.validate().is_err());
    }
}
