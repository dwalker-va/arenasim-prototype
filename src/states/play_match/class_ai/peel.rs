//! The Peel play's shared half (`situation::Play::Peel`).
//!
//! While a teammate is in danger (`ally_in_danger`: dying AND pressured), a
//! unit on a team that runs the play — `AiProfile::TeamPlan` (the play
//! graduated at the 0.8 sweep) — puts its PEEL step at the head of its
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
//!
//! The Hunter's pet peels too: while its Hunter runs the play, a Spider webs a
//! melee or pet attacker and a Boar charges an attacker, through the pet
//! dispatch the Hunter already owns ([`pet_peel_tool`]).
//!
//! **Context gates.** The play is not run where it measured negative: a
//! peeling class does not peel for a teammate of a class it hurts
//! ([`PEEL_GATES`]). A unit whose context is gated passes to the next stage
//! of the pipeline (`plays::PLAY_ORDER`), as a Warrior (no tool) does.

use bevy::prelude::*;
use smallvec::SmallVec;

use super::plays::{own_class, own_team_size, PlayStage};
use super::{CombatContext, Incoming};
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::components::{AuraType, PetType};
use crate::states::play_match::situation::{
    ExceptionKey, ExceptionSet, Play, PlayId, Role, TeamSituation, FORMAT_SIZES,
};

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

/// The pet ability a Hunter's pet peels with while its Hunter runs the play:
/// Spider Web (a root, so a MOVEMENT peel) or Boar Charge (a stun). Exhaustive:
/// a new pet answers here. Only the Hunter dispatches pet peels; a Warlock's
/// Felhunter has none, and a Bird's Master's Call frees an ally rather than
/// holding an attacker.
pub fn pet_peel_tool(pet: PetType) -> Option<AbilityType> {
    match pet {
        PetType::Spider => Some(AbilityType::SpiderWeb),
        PetType::Boar => Some(AbilityType::BoarCharge),
        PetType::Bird | PetType::Felhunter => None,
    }
}

// ---------------------------------------------------------------------------
// Context gates
// ---------------------------------------------------------------------------

/// A context the Peel play is switched off in: a `peeler` class peeling for a
/// teammate in danger of class `ally`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeelContext {
    pub peeler: CharacterClass,
    pub ally: CharacterClass,
}

/// One named exception to the Peel play: the context, and the measurement that
/// put it there.
#[derive(Clone, Copy, Debug)]
pub struct PeelGate {
    pub context: PeelContext,
    /// One line: what the measurement read, and where it is recorded.
    pub measured: &'static str,
}

const fn gate(peeler: CharacterClass, ally: CharacterClass, measured: &'static str) -> PeelGate {
    PeelGate {
        context: PeelContext { peeler, ally },
        measured,
    }
}

/// Where every gate's measurement is recorded.
pub const PEEL_GATES_FINDINGS: &str = "docs/design/balance/2026-10-12-as290-peel-context-gates.md";

/// The contexts the Peel play is switched off in: where peeling measured
/// negative. Not every behaviour suits every composition, so the play is kept
/// where it helps and switched off, by name, where it hurts (the user's
/// ruling, 2026-10-12).
///
/// The bar, from [`PEEL_GATES_FINDINGS`]: the peeler's peel for that teammate
/// costs the peeling side win rate, significantly (z <= -1.96) over 16 seeds
/// of the head-to-head, and with the same sign on the 4 seeds that picked it
/// and the 12 that confirmed it. Two gates came later, once the Burst play
/// ran in every arm: picked on the 4 validation seeds and confirmed
/// significant on the 12 confirmation seeds alone. Two more came at the 0.8
/// milestone sweep ([`MILESTONE_08_FINDINGS`] §3.1), the AS-290 bar met on
/// the sweep's seeds and fresh ones pooled. A gate is a whole
/// peeler-for-ally context:
/// within each, no single tool measured positive. Every gate is re-checked at
/// the milestone sweep with the gates lifted (`"team1_exceptions": {"Peel":
/// []}`, and team 2's); it
/// stays only while it still measures negative, and a context that turns
/// negative joins. `tests/peel_gates.rs` names them by set equality, so adding
/// or removing one is a visible edit there too.
pub const PEEL_GATES: &[PeelGate] = &[
    gate(
        CharacterClass::Mage,
        CharacterClass::Hunter,
        "-3.3pt over 1,920 pairs (2 flips for, 66 against, z=-7.6); Nova and Polymorph each negative",
    ),
    gate(
        CharacterClass::Mage,
        CharacterClass::Warlock,
        "-2.7pt over 1,920 pairs (22 for, 74 against, z=-5.2); Nova and Polymorph each negative",
    ),
    gate(
        CharacterClass::Druid,
        CharacterClass::Hunter,
        "-1.6pt over 1,920 pairs (55 for, 86 against, z=-2.5); Cyclone and Roots each negative",
    ),
    gate(
        CharacterClass::Druid,
        CharacterClass::Warlock,
        "-1.3pt over 1,920 pairs (24 for, 49 against, z=-2.8)",
    ),
    gate(
        CharacterClass::Druid,
        CharacterClass::Warrior,
        "-1.8pt over 1,920 pairs (76 for, 110 against, z=-2.4)",
    ),
    gate(
        CharacterClass::Shaman,
        CharacterClass::Warlock,
        "-1.6pt over 1,920 pairs (43 for, 74 against, z=-2.8)",
    ),
    gate(
        CharacterClass::Rogue,
        CharacterClass::Hunter,
        "-0.5pt over 1,920 pairs (1 for, 10 against, z=-2.4)",
    ),
    gate(
        CharacterClass::Rogue,
        CharacterClass::Warrior,
        "-2.6pt over 1,440 pairs with Burst on (35 for, 72 against, z=-3.5); Kidney Shot contested with Burst",
    ),
    gate(
        CharacterClass::Rogue,
        CharacterClass::Priest,
        "-1.0pt over 1,440 pairs with Burst on (1 for, 15 against, z=-3.3); Kidney Shot contested with Burst",
    ),
    // The 0.8 milestone sweep (`MILESTONE_08_FINDINGS` §3.1): each picked on
    // the sweep's seeds and confirmed on fresh seeds 871-876, pooled.
    gate(
        CharacterClass::Paladin,
        CharacterClass::Hunter,
        "0.8 sweep: -0.8pt on its play seeds (0 for, 9 against), -0.2pt on 871-876; pooled 1 for, 12 against, z=-3.1",
    ),
    gate(
        CharacterClass::Hunter,
        CharacterClass::Priest,
        "0.8 sweep: -0.7pt on its play seeds (7 for, 15 against), -0.6pt on 871-876; pooled 16 for, 30 against, z=-2.1",
    ),
];

/// Where the two gates the 0.8 milestone sweep added are recorded.
pub const MILESTONE_08_FINDINGS: &str = "docs/design/balance/2026-10-25-0.8-milestone-sweep.md";

/// One FORMAT gate: a peeling class that does not peel at all on a team of
/// `team_size` (in any context), and the measurement that put it there.
#[derive(Clone, Copy, Debug)]
pub struct PeelFormatGate {
    pub peeler: CharacterClass,
    pub team_size: u8,
    /// One line: what the measurement read, and where it is recorded.
    pub measured: &'static str,
}

/// Where the format gates are recorded.
pub const PEEL_FORMAT_GATES_FINDINGS: &str =
    "docs/design/balance/2026-10-25-as398-play-graduation.md";

/// The formats a peeling class does not peel in at all. AS-290 picked the
/// context gates in 2v2 only; the 0.8 sweep found the Mage's and the Druid's
/// 3v3 teams losing to the play, and AS-398 confirmed each class's own peel
/// as the cost, by the AS-290 bar: picked on the 0.8 sweep's 3v3 seeds,
/// confirmed on fresh ones (871-876), every block negative. The full 3v3
/// context selection, healer allies included, is AS-327's.
pub const PEEL_FORMAT_GATES: &[PeelFormatGate] = &[
    PeelFormatGate {
        peeler: CharacterClass::Mage,
        team_size: 3,
        measured: "AS-398: -1.0pt on the 0.8 sweep's 3v3 seeds (31 for, 70 against), -0.6pt on 871-876 (98 for, 141 against); pooled 129 for, 211 against, z=-4.4",
    },
    PeelFormatGate {
        peeler: CharacterClass::Druid,
        team_size: 3,
        measured: "AS-398: -1.0pt on the 0.8 sweep's 3v3 seeds (92 for, 131 against), -1.3pt on 871-876 (186 for, 286 against); pooled 278 for, 417 against, z=-5.3",
    },
];

/// Whether [`PEEL_GATES`] switches the play off for a `peeler` peeling for an
/// `ally`.
pub fn gated(peeler: CharacterClass, ally: CharacterClass) -> bool {
    PEEL_GATES
        .iter()
        .any(|g| g.context == PeelContext { peeler, ally })
}

/// The Peel play, as a stage of the pipeline (`plays::PLAY_ORDER`).
pub struct PeelStage;

impl PlayStage for PeelStage {
    fn id(&self) -> PlayId {
        PlayId::Peel
    }

    fn config_key(&self) -> &'static str {
        "Peel"
    }

    /// Every role, for a teammate in danger with an attacker to peel — never
    /// the teammate in danger itself: its own defence is its chain's.
    fn calls(&self, situation: &TeamSituation, _role: Role, me: Entity) -> Option<Play> {
        situation
            .ally_in_danger
            .filter(|w| w.subject != me && !situation.danger_attackers.is_empty())
            .map(|w| Play::Peel { ally: w.subject })
    }

    /// A unit with a peel tool ([`peel_tools`]): a Warrior has none.
    fn admits(&self, ctx: &CombatContext, _play: &Play) -> bool {
        own_class(ctx).is_some_and(|class| !peel_tools(class).is_empty())
    }

    /// The peeler's class, for the class of the teammate it peels for: the
    /// context [`PEEL_GATES`] switches off. A teammate whose class its view
    /// does not know is gated by nothing.
    fn exception_key(&self, ctx: &CombatContext, play: &Play) -> Option<ExceptionKey> {
        let Play::Peel { ally } = *play else {
            return None;
        };
        let class = |e: Entity| ctx.combatants.get(&e).map(|info| info.class);
        Some(ExceptionKey::in_context(
            class(ctx.self_entity)?,
            class(ally)?,
        ))
    }

    fn exceptions_have_context(&self) -> bool {
        true
    }

    /// The peeler's class on its team's size: the format
    /// [`PEEL_FORMAT_GATES`] switches off.
    fn format_exception_key(&self, ctx: &CombatContext, _play: &Play) -> Option<ExceptionKey> {
        let size = own_team_size(ctx).filter(|s| FORMAT_SIZES.contains(s))?;
        Some(ExceptionKey::in_format(own_class(ctx)?, size))
    }

    fn exceptions_by_format(&self) -> bool {
        true
    }

    fn exceptions(&self) -> ExceptionSet {
        ExceptionSet::of(
            &PEEL_GATES
                .iter()
                .map(|g| ExceptionKey::in_context(g.context.peeler, g.context.ally))
                .chain(
                    PEEL_FORMAT_GATES
                        .iter()
                        .map(|g| ExceptionKey::in_format(g.peeler, g.team_size)),
                )
                .collect::<Vec<_>>(),
        )
    }
}
