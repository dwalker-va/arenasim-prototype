//! The plays as a pipeline: each gated play is a [`PlayStage`], and
//! [`PLAY_ORDER`] is the one table that orders them.
//!
//! A unit's play is the first stage in [`PLAY_ORDER`] that its team RUNS
//! (`AiProfile::TeamPlan`, less the plays withheld from it for measurement),
//! that the team situation CALLS for this unit ([`PlayStage::calls`]), and
//! that the unit itself ADMITS ([`PlayStage::admits`], then the stage's own
//! exceptions). A stage that does not apply passes the unit to the NEXT stage,
//! never straight to the fallback: a peeler its context gates off still
//! chains, and a chain the unit cannot hand off still bursts. With no stage
//! left the unit runs [`Play::Default`], its class's ordinary priority order.
//!
//! [`walk`] is that rule, once. Both readers take it:
//! `TeamSituation::play_for` with every stage admitted (the team's view, which
//! cannot see the unit's class), and `CombatContext::play` with the unit's own
//! admission.
//!
//! **Adding a play.** A new play is a new module holding its stage — its
//! `calls`, its `admits`, its own exceptions table with the measurement for
//! each entry, its config key — its variant in `Play` and `PlayId` (the
//! compiler then asks for its trace tag), and ONE line in [`PLAY_ORDER`]. The
//! walk, the withhold and exception controls (`situation::WithheldPlays`), the
//! headless config and the scene builder are keyed by the play's id, so none
//! of them changes.

use bevy::prelude::*;

use super::CombatContext;
use crate::states::match_config::CharacterClass;
use crate::states::play_match::situation::{
    ExceptionKey, ExceptionSet, Play, PlayId, Role, TeamSituation,
};

/// One gated play, as a stage of the pipeline.
pub trait PlayStage: Sync {
    /// Which play this stage is.
    fn id(&self) -> PlayId;

    /// The play's name in a headless config (`team1_withheld_plays`,
    /// `team1_exceptions`): its `Play` variant's name. The snake-case
    /// spelling is accepted too.
    fn config_key(&self) -> &'static str;

    /// The team half: whether the situation calls this play for the unit
    /// `me`, playing `role`, and with what payload. Reads the team situation
    /// only — never the unit's class — and is asked only of a team that runs
    /// the play.
    fn calls(&self, situation: &TeamSituation, role: Role, me: Entity) -> Option<Play>;

    /// The unit half: whether the deciding unit can run `play` at all (it has
    /// a tool for it). Its exceptions are applied after, from
    /// [`Self::exception_key`].
    fn admits(&self, ctx: &CombatContext, play: &Play) -> bool;

    /// The exceptions-table key the deciding unit running `play` is looked up
    /// under: its class, and for a play that is switched off by context, the
    /// class of that context (a peel's teammate). `None` when it cannot be
    /// said, and then no exception applies.
    fn exception_key(&self, ctx: &CombatContext, play: &Play) -> Option<ExceptionKey>;

    /// Whether this play's exceptions are keyed by a context class as well as
    /// the deciding class ([`ExceptionKey::context`]). A headless config's
    /// exception entry must have the same shape.
    fn exceptions_have_context(&self) -> bool;

    /// The shipped exceptions: where the play measured negative and is not
    /// run (the stage's own named table, each entry with its measurement).
    fn exceptions(&self) -> ExceptionSet;
}

/// The order the plays are tried in. A teammate in danger is answered before
/// anything else; a last-stand healer hides rather than holding a global
/// cooldown for a hand-off; a team that takes a pillar together takes it
/// before a hand-off (AS-385 §3.0); a lock about to end is extended before a
/// burst is spent into it.
pub static PLAY_ORDER: &[&dyn PlayStage] = &[
    &super::peel::PeelStage,
    &super::take_cover::TakeCoverStage,
    &super::pillar_stance::PillarStage,
    &super::chain::ChainStage,
    &super::burst::BurstStage,
];

/// The stage for `id`. Every [`PlayId`] has exactly one
/// (`every_play_id_has_one_stage`).
pub fn stage(id: PlayId) -> &'static dyn PlayStage {
    *PLAY_ORDER
        .iter()
        .find(|s| s.id() == id)
        .expect("every PlayId has a stage in PLAY_ORDER")
}

/// The play id a config names: a stage's [`PlayStage::config_key`], or its
/// snake-case spelling.
pub fn parse_play(name: &str) -> Result<PlayId, String> {
    PLAY_ORDER
        .iter()
        .find(|s| name == s.config_key() || name == snake_case(s.config_key()))
        .map(|s| s.id())
        .ok_or_else(|| {
            let keys: Vec<&str> = PLAY_ORDER.iter().map(|s| s.config_key()).collect();
            format!(
                "Unknown gated play: '{name}'. Gated plays: {}",
                keys.join(", ")
            )
        })
}

fn snake_case(key: &str) -> String {
    let mut out = String::with_capacity(key.len() + 4);
    for (i, c) in key.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// The pipeline: the first stage after `after` (from the head when `None`)
/// that `situation`'s team runs, that it calls for `me`, and that `admit`
/// lets the unit run; else [`Play::Default`].
pub fn walk(
    situation: &TeamSituation,
    role: Role,
    me: Entity,
    after: Option<PlayId>,
    admit: impl Fn(&dyn PlayStage, &Play) -> bool,
) -> Play {
    let start = after.map_or(0, |id| {
        PLAY_ORDER
            .iter()
            .position(|s| s.id() == id)
            .expect("every PlayId has a stage in PLAY_ORDER")
            + 1
    });
    PLAY_ORDER[start..]
        .iter()
        .find_map(|stage| {
            let play = called(*stage, situation, role, me)?;
            admit(*stage, &play).then_some(play)
        })
        .unwrap_or(Play::Default)
}

/// `stage`'s play for `me`, when its team runs it and the situation calls it.
pub fn called(
    stage: &dyn PlayStage,
    situation: &TeamSituation,
    role: Role,
    me: Entity,
) -> Option<Play> {
    if !situation.runs.contains(stage.id()) {
        return None;
    }
    stage.calls(situation, role, me)
}

/// Whether the deciding unit runs `play` as `stage`: its team runs the play,
/// the stage admits it, and the stage's exceptions do not name it.
pub fn admitted(ctx: &CombatContext, stage: &dyn PlayStage, play: &Play) -> bool {
    let situation = ctx.situation();
    situation.runs.contains(stage.id())
        && stage.admits(ctx, play)
        && stage
            .exception_key(ctx, play)
            .is_none_or(|key| !situation.exceptions.of(stage.id()).contains(key))
}

/// The deciding unit's class, as its own view has it.
pub(crate) fn own_class(ctx: &CombatContext) -> Option<CharacterClass> {
    ctx.self_info().map(|info| info.class)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pipeline's order, pinned: Peel, TakeCover, Pillar, Chain, Burst.
    /// A change to it is a change to every unit's play, and must be made here
    /// too.
    #[test]
    fn the_order_table_is_peel_take_cover_pillar_chain_burst() {
        let order: Vec<PlayId> = PLAY_ORDER.iter().map(|s| s.id()).collect();
        assert_eq!(
            order,
            vec![
                PlayId::Peel,
                PlayId::TakeCover,
                PlayId::Pillar,
                PlayId::Chain,
                PlayId::Burst
            ]
        );
    }

    /// Every play id is one stage, once; and its config key names it.
    #[test]
    fn every_play_id_has_one_stage() {
        for id in PlayId::ALL {
            assert_eq!(
                PLAY_ORDER.iter().filter(|s| s.id() == id).count(),
                1,
                "{id:?}"
            );
            assert_eq!(stage(id).id(), id);
            assert_eq!(parse_play(stage(id).config_key()), Ok(id));
            assert_eq!(parse_play(&snake_case(stage(id).config_key())), Ok(id));
        }
        assert_eq!(PLAY_ORDER.len(), PlayId::ALL.len());
        assert_eq!(parse_play("TakeCover"), Ok(PlayId::TakeCover));
        assert_eq!(parse_play("take_cover"), Ok(PlayId::TakeCover));
        assert!(parse_play("Default").is_err());
    }

    /// The walk's composition, with every play called at once: the first
    /// stage in table order that the team runs wins, and a stage the unit
    /// does not admit passes it to the NEXT stage — never straight to the
    /// fallback — until none is left. A team that does not run a play skips
    /// its stage the same way.
    #[test]
    fn the_walk_takes_the_first_stage_and_passes_a_refused_one_to_the_next() {
        use crate::states::play_match::situation::{PlaySet, Window};
        let me = Entity::from_raw(1);
        let ally = Entity::from_raw(2);
        let subject = Entity::from_raw(3);
        let window = |subject| Window {
            subject,
            since: 0.0,
            until: Some(31.0),
        };
        let every = TeamSituation {
            ally_in_danger: Some(window(ally)),
            danger_attackers: [subject].into_iter().collect(),
            last_stand: Some(window(me)),
            lock_ending: Some(window(subject)),
            kill_window: Some(window(subject)),
            pillar_stance: Some(super::super::pillar_stance::StanceWindow {
                healer: me,
                pillar: 0,
                side: 0.0,
                reason: super::super::pillar_stance::StanceReason::AvoidCc,
                since: 0.0,
            }),
            now: 30.0,
            runs: PlaySet::all(),
            ..Default::default()
        };
        let peel = Play::Peel { ally };
        let chain = Play::Chain { subject };
        // Refuse each stage in turn, cumulatively, in table order.
        let refusing = |s: &TeamSituation, role: Role, refused: &[PlayId]| {
            walk(s, role, me, None, |stage, _| !refused.contains(&stage.id()))
        };
        let healer: Vec<Play> = (0..=PlayId::COUNT)
            .map(|n| refusing(&every, Role::Healer, &PLAY_ORDER_IDS[..n]))
            .collect();
        assert_eq!(
            healer,
            vec![
                peel,
                Play::TakeCover,
                Play::Pillar,
                chain,
                Play::Default,
                Play::Default
            ]
        );
        // A melee is never called to the Pillar play (its half of the stance
        // is the sight yield); a ranged DPS is.
        let dps: Vec<Play> = (0..=PlayId::COUNT)
            .map(|n| refusing(&every, Role::MeleeDps, &PLAY_ORDER_IDS[..n]))
            .collect();
        assert_eq!(
            dps,
            vec![peel, chain, chain, chain, Play::Burst, Play::Default]
        );
        let ranged: Vec<Play> = (0..=PlayId::COUNT)
            .map(|n| refusing(&every, Role::RangedDps, &PLAY_ORDER_IDS[..n]))
            .collect();
        assert_eq!(
            ranged,
            vec![
                peel,
                Play::Pillar,
                Play::Pillar,
                chain,
                Play::Burst,
                Play::Default
            ]
        );
        // A refused stage alone passes to the one after it, not the fallback.
        assert_eq!(
            refusing(&every, Role::Healer, &[PlayId::Peel]),
            Play::TakeCover
        );
        assert_eq!(refusing(&every, Role::MeleeDps, &[PlayId::Chain]), peel);
        assert_eq!(
            refusing(&every, Role::MeleeDps, &[PlayId::Peel, PlayId::Chain]),
            Play::Burst
        );
        // Not running a play is skipping its stage; `after` starts the walk
        // past a stage.
        let no_peel = TeamSituation {
            runs: PlaySet::of(&[Play::TakeCover, chain, Play::Burst]),
            ..every.clone()
        };
        assert_eq!(no_peel.play_for(Role::Healer, me), Play::TakeCover);
        assert_eq!(
            every.play_after(PlayId::Peel, Role::Healer, me),
            Play::TakeCover
        );
        assert_eq!(
            every.play_after(PlayId::TakeCover, Role::Healer, me),
            Play::Pillar
        );
        assert_eq!(every.play_after(PlayId::Pillar, Role::Healer, me), chain);
        assert_eq!(
            every.play_after(PlayId::Chain, Role::MeleeDps, me),
            Play::Burst
        );
        assert_eq!(every.play_for(Role::MeleeDps, me), peel);
    }

    /// The lock-window rule runs for a unit the Burst stage does NOT admit:
    /// under `Legacy`, and under `TeamPlan` for a class the Burst play's
    /// exceptions name — not for one they do not. With the enemy Priest
    /// stunned, a Warrior named a Burst exception by override runs the rule;
    /// one with a Mage named instead, or the shipped table, does not.
    #[test]
    fn a_burst_exception_hands_its_class_back_the_lock_window() {
        use super::super::reach_fixture::{ctx, roster, unit};
        use crate::states::match_config::CharacterClass::{Mage, Priest, Warrior};
        use crate::states::play_match::ai_profile::AiProfile;
        use crate::states::play_match::components::{Aura, AuraType};
        use crate::states::play_match::situation::{
            ClassSet, MatchClock, SituationConfig, WithheldPlays,
        };
        use std::collections::BTreeMap;

        let mut world = World::new();
        let me = world.spawn_empty().id();
        let healer = world.spawn_empty().id();
        let mage = world.spawn_empty().id();
        let roster = roster(vec![
            unit(me, 1, Warrior, Vec3::ZERO, 1.0),
            unit(healer, 2, Priest, Vec3::new(0.0, 0.0, 10.0), 1.0),
            unit(mage, 2, Mage, Vec3::new(5.0, 0.0, 0.0), 1.0),
        ]);
        let auras: BTreeMap<Entity, Vec<Aura>> = [(
            healer,
            vec![Aura {
                effect_type: AuraType::Stun,
                duration: 3.0,
                ..Default::default()
            }],
        )]
        .into_iter()
        .collect();
        let lock_window = |profile: AiProfile, excepted: Option<CharacterClass>| {
            let mut withheld = WithheldPlays::default();
            if let Some(class) = excepted {
                withheld
                    .exceptions_mut(1)
                    .set(PlayId::Burst, ClassSet::of(&[class]).into());
            }
            let cfg = SituationConfig {
                withheld,
                ..SituationConfig::default()
            }
            .for_team(1, profile);
            let c = ctx(me, &roster, &auras, &[]).classified(&cfg, MatchClock::after_gates(30.0));
            assert!(c.situation().enemy_healer_locked.is_some());
            c.lock_window()
        };
        assert!(lock_window(AiProfile::Legacy, None), "Legacy");
        assert!(!lock_window(AiProfile::TeamPlan, None), "TeamPlan bursts");
        assert!(
            lock_window(AiProfile::TeamPlan, Some(Warrior)),
            "an excepted Warrior"
        );
        assert!(
            !lock_window(AiProfile::TeamPlan, Some(Mage)),
            "another class excepted"
        );
    }

    /// [`PLAY_ORDER`]'s ids, for the walk test.
    const PLAY_ORDER_IDS: [PlayId; 5] = [
        PlayId::Peel,
        PlayId::TakeCover,
        PlayId::Pillar,
        PlayId::Chain,
        PlayId::Burst,
    ];
}
