//! The Burst play's shared half (`situation::Play::Burst`).
//!
//! While a kill window is open (`TeamSituation::kill_window`: an enemy low and
//! beyond its healers' help), a DPS on a team that runs the play —
//! `AiProfile::TeamPlan`, unless the play is withheld for measurement, and not
//! a class `situation::BURST_EXCEPTIONS` names — puts its BURST step at the
//! head of its chain, below a peel step: its [`burst_tools`], in order, on the
//! window's subject.
//!
//! The play is a reordering plus one gate, not a scorer (the architecture's
//! §4.2):
//!
//! - **The order.** Each class tries its existing `try_*` casts for its burst
//!   tools on the subject [`burst_call`] hands it. When none fires, the
//!   class's chain runs as before. A cast the burst step chose is traced with
//!   `situation.play_step = "burst"`.
//! - **The fit.** Under the play, NO cast on the subject is started that would
//!   land after the window's `until` — the burst step's or the chain's below
//!   it. The cast gate asks it of every targeted cast
//!   (`cast_guard::fits_window`, traced `OutlastsWindow`), so a Shadow Bolt
//!   that would land after the healer's Polymorph ends, or a Rend whose first
//!   tick would, is passed over for what still lands inside the window.
//!
//! What the subject is filtered by here, once for every class: the deciding
//! unit must SEE it, and it must be alive. Everything else — range, sight,
//! immunity, a friendly crowd control its damage would break — is the cast
//! gate's, per tool.

use bevy::prelude::*;

use super::CombatContext;
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::AbilityType;

/// The burst a unit owes this tick: the kill window's subject, and when the
/// window ends (if that is known).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BurstCall {
    pub subject: Entity,
    pub until: Option<f32>,
}

/// The burst the deciding unit owes: `Some` while its chain runs under the
/// Burst play ([`CombatContext::burst_window`]) and the window's subject is
/// alive and in its view.
pub fn burst_call(ctx: &CombatContext) -> Option<BurstCall> {
    let window = ctx.burst_window()?;
    ctx.combatants
        .get(&window.subject)
        .is_some_and(|info| info.is_alive)
        .then_some(BurstCall {
            subject: window.subject,
            until: window.until,
        })
}

/// The hard hitters `class` bursts with, in the order its burst step tries
/// them. Exhaustive: a new class answers here. A healer has none — the play is
/// a DPS's — and reads its fallback chain. Read by the play's probe to tell a
/// burst from any other cast.
pub fn burst_tools(class: CharacterClass) -> &'static [AbilityType] {
    match class {
        // Mortal Strike, then a Heroic Strike that spends the rage the
        // chain otherwise holds back for the next Mortal Strike.
        CharacterClass::Warrior => &[AbilityType::MortalStrike, AbilityType::HeroicStrike],
        // Sinister Strike, ahead of the Kidney Shot planner's pooling.
        CharacterClass::Rogue => &[AbilityType::SinisterStrike],
        CharacterClass::Mage => &[AbilityType::Frostbolt],
        // Death Coil's damage and horror, then Shadow Bolt.
        CharacterClass::Warlock => &[AbilityType::DeathCoil, AbilityType::Shadowbolt],
        CharacterClass::Hunter => &[AbilityType::AimedShot, AbilityType::ArcaneShot],
        CharacterClass::Priest
        | CharacterClass::Paladin
        | CharacterClass::Shaman
        | CharacterClass::Druid => &[],
    }
}
