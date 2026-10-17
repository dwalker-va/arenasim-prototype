//! The TakeCover play's stage (`situation::Play::TakeCover`).
//!
//! While its team is in its `last_stand` and the last unit is a healer, that
//! healer breaks the enemy casters' line of sight behind cover instead of
//! standing in the open. A MOVEMENT play: its movement is
//! `team_solve::last_stand_cover`, driven from the healers' posture machines
//! (`healer_postures`), and its chain is the healer's ordinary one. Inert on a
//! map with nothing to hide behind.

use bevy::prelude::*;

use super::plays::{own_class, PlayStage};
use super::CombatContext;
use crate::states::match_config::CharacterClass;
use crate::states::play_match::situation::{
    ExceptionKey, ExceptionSet, Play, PlayId, Role, TeamSituation,
};

/// A healer class the TakeCover play is NOT used for, and the measurement
/// that put it here: the same ruling, and the same re-check at each milestone
/// sweep, as `burst::BurstException`.
#[derive(Clone, Copy, Debug)]
pub struct TakeCoverException {
    pub class: CharacterClass,
    /// The measurement that put it here: the findings note and its row.
    pub finding: &'static str,
}

/// The classes the TakeCover play is not used for (see [`TakeCoverException`]).
pub const TAKE_COVER_EXCEPTIONS: &[TakeCoverException] = &[];

/// The TakeCover play, as a stage of the pipeline (`plays::PLAY_ORDER`).
pub struct TakeCoverStage;

impl PlayStage for TakeCoverStage {
    fn id(&self) -> PlayId {
        PlayId::TakeCover
    }

    fn config_key(&self) -> &'static str {
        "TakeCover"
    }

    /// A healer that is its team's [`last_stand`](TeamSituation::last_stand):
    /// alone against two or more, it hides rather than holding a global
    /// cooldown for a hand-off (a last stand has no teammate to make a lock it
    /// could extend).
    fn calls(&self, situation: &TeamSituation, role: Role, me: Entity) -> Option<Play> {
        (role == Role::Healer && situation.last_stand.is_some_and(|w| w.subject == me))
            .then_some(Play::TakeCover)
    }

    /// Every healer can take cover: the play is its movement.
    fn admits(&self, _ctx: &CombatContext, _play: &Play) -> bool {
        true
    }

    fn exception_key(&self, ctx: &CombatContext, _play: &Play) -> Option<ExceptionKey> {
        own_class(ctx).map(ExceptionKey::class)
    }

    fn exceptions_have_context(&self) -> bool {
        false
    }

    fn exceptions(&self) -> ExceptionSet {
        ExceptionSet::of(
            &TAKE_COVER_EXCEPTIONS
                .iter()
                .map(|e| ExceptionKey::class(e.class))
                .collect::<Vec<_>>(),
        )
    }
}
