//! The Burst play's shared half (`situation::Play::Burst`).
//!
//! While a kill window is open (`TeamSituation::kill_window`: an enemy low and
//! beyond its healers' help), a DPS on a team that runs the play —
//! `AiProfile::TeamPlan` (the play graduated at the 0.8 sweep), and not a
//! class [`BURST_EXCEPTIONS`] names — puts its BURST step at the
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

use super::plays::{own_class, PlayStage};
use super::CombatContext;
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::situation::{
    ExceptionKey, ExceptionSet, Play, PlayId, Role, TeamSituation,
};

/// A DPS class the Burst play is NOT used for, and the measurement that put it
/// here. The ruling (2026-10-12): a play that measures negative in a context
/// is not used there; each exception is named here, with its finding, and is
/// re-checked at each milestone sweep by running the head-to-head with the
/// exceptions lifted (`"team1_exceptions": {"Burst": []}` in a headless
/// config; `situation::WithheldPlays`).
#[derive(Clone, Copy, Debug)]
pub struct BurstException {
    pub class: CharacterClass,
    /// The measurement that put it here: the findings note and its row.
    pub finding: &'static str,
}

/// The classes the Burst play is not used for (see [`BurstException`]).
pub const BURST_EXCEPTIONS: &[BurstException] = &[];

/// The Burst play, as a stage of the pipeline (`plays::PLAY_ORDER`).
pub struct BurstStage;

impl PlayStage for BurstStage {
    fn id(&self) -> PlayId {
        PlayId::Burst
    }

    fn config_key(&self) -> &'static str {
        "Burst"
    }

    /// A DPS, while a kill window is open. Per role, not per class: a class
    /// [`BURST_EXCEPTIONS`] names is answered by the unit's admission.
    fn calls(&self, situation: &TeamSituation, role: Role, _me: Entity) -> Option<Play> {
        match role {
            Role::MeleeDps | Role::RangedDps if situation.kill_window.is_some() => {
                Some(Play::Burst)
            }
            Role::MeleeDps | Role::RangedDps | Role::Healer => None,
        }
    }

    /// Any unit whose class its own view knows.
    fn admits(&self, ctx: &CombatContext, _play: &Play) -> bool {
        own_class(ctx).is_some()
    }

    fn exception_key(&self, ctx: &CombatContext, _play: &Play) -> Option<ExceptionKey> {
        own_class(ctx).map(ExceptionKey::class)
    }

    fn exceptions_have_context(&self) -> bool {
        false
    }

    fn exceptions(&self) -> ExceptionSet {
        ExceptionSet::of(
            &BURST_EXCEPTIONS
                .iter()
                .map(|e| ExceptionKey::class(e.class))
                .collect::<Vec<_>>(),
        )
    }
}

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
