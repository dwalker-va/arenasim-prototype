//! The Peel play's shared half (`situation::Play::Peel`).
//!
//! While a teammate is in danger (`ally_in_danger`: dying AND pressured), a
//! unit on a team that runs the play — `AiProfile::TeamPlan`, unless the
//! play is withheld for measurement — puts its PEEL step at the head of its
//! own chain: crowd control on one of the teammate's attackers
//! (`TeamSituation::danger_attackers`). The teammate itself does not peel;
//! its own defence is its chain's.
//!
//! The play is a reordering, not a scorer (the architecture's §4.2): each
//! class tries its existing `try_*` casts for its [`peel_tools`], in its own
//! order, on the attackers [`peel_call`] hands it, and every cast goes through
//! the cast gate — range, sight, cooldown, resource, and the hold rule (no
//! hard CC on a unit the team already holds). When nothing fires, the class's
//! chain runs exactly as its fallback play orders it. A cast the peel step
//! chose is traced with `situation.play_step = "peel"`.
//!
//! What the attackers are filtered by here, once for every class:
//! - the deciding unit must SEE the attacker (its own view);
//! - an attacker its team already HOLDS (a stun, a fear, a polymorph, a
//!   cyclone, a horror, a trap) is not attacking, so it needs no peel.
//!
//! What each tool asks of an attacker itself:
//! - a MOVEMENT peel (a root or a snare) answers only an attacker that has to
//!   move to keep hitting — a melee or a pet — and one not already rooted
//!   ([`movement_peel_target`]);
//! - a CC that breaks on damage is not cast on the unit's own target, which
//!   its own damage would break at once (each class's tool says so where it
//!   applies).

use bevy::prelude::*;
use smallvec::SmallVec;

use super::{CombatContext, Incoming};
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::components::AuraType;
use crate::states::play_match::situation::{Play, Role};

/// The peel a unit owes this tick: the teammate in danger, and the attackers
/// it can peel, nearest that teammate first.
#[derive(Clone, Debug, PartialEq)]
pub struct PeelCall {
    pub ally: Entity,
    pub attackers: SmallVec<[Entity; 4]>,
}

/// The peel the deciding unit owes as `role`: `Some` when its play is
/// [`Play::Peel`] and at least one of the teammate's attackers is in its view,
/// alive, and not already held by its team (see the module docs).
pub fn peel_call(ctx: &CombatContext, role: Role) -> Option<PeelCall> {
    let Play::Peel { ally } = ctx.play(role) else {
        return None;
    };
    let attackers: SmallVec<[Entity; 4]> = ctx
        .situation()
        .danger_attackers
        .iter()
        .copied()
        .filter(|e| ctx.combatants.get(e).is_some_and(|info| info.is_alive))
        // `Breaks`: every hold counts, whatever the team is doing to it.
        .filter(|e| ctx.held_in_hard_cc(*e, Incoming::Breaks).is_none())
        .collect();
    (!attackers.is_empty()).then_some(PeelCall { ally, attackers })
}

/// Whether a MOVEMENT peel — a root, a snare — answers `attacker`: it is a
/// melee or a pet, which has to move to keep hitting, and it is not already
/// rooted or held.
pub fn movement_peel_target(ctx: &CombatContext, attacker: Entity) -> bool {
    ctx.combatants
        .get(&attacker)
        .is_some_and(|info| (info.is_pet || info.class.is_melee()) && !ctx.is_ccd(attacker))
}

/// Whether `attacker` is already slowed — a snare peel on it would only
/// refresh one.
pub fn slowed(ctx: &CombatContext, attacker: Entity) -> bool {
    ctx.active_auras.get(&attacker).is_some_and(|auras| {
        auras
            .iter()
            .any(|a| a.effect_type == AuraType::MovementSpeedSlow)
    })
}

/// The crowd control `class` peels with, in the order its peel step tries
/// them. Exhaustive: a new class answers here. Read by the play's probe to
/// tell a peel from any other cast.
pub fn peel_tools(class: CharacterClass) -> &'static [AbilityType] {
    match class {
        CharacterClass::Mage => &[AbilityType::FrostNova, AbilityType::Polymorph],
        CharacterClass::Warlock => &[AbilityType::DeathCoil, AbilityType::Fear],
        CharacterClass::Paladin => &[AbilityType::HammerOfJustice],
        CharacterClass::Hunter => &[AbilityType::ConcussiveShot, AbilityType::FrostTrap],
        CharacterClass::Rogue => &[AbilityType::KidneyShot],
        CharacterClass::Druid => &[AbilityType::Cyclone, AbilityType::EntanglingRoots],
        CharacterClass::Priest => &[AbilityType::PsychicScream],
        CharacterClass::Shaman => &[AbilityType::FrostShock],
        // No Hamstring or Intimidating Shout in the kit: the Warrior has no
        // peel, and runs its fallback chain.
        CharacterClass::Warrior => &[],
    }
}
