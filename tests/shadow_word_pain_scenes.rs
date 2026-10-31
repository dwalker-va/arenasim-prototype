//! AS-412 — a healing Priest keeps Shadow Word: Pain on its kill target only
//! when it is idle: when no heal, dispel or crowd control is owed, the slot
//! its wand fills (user ruling, 2026-10-29). Gated under `TeamPlan`; under
//! `Legacy` the Priest never considers it.
//!
//! Scenes (tier 1), each paired with its one condition removed:
//!
//! - idle: the `TeamPlan` Priest casts it on its kill target; the `Legacy`
//!   Priest in the same scene does not consider it;
//! - a heal owed: in reach, Flash Heal goes instead; out of reach, the DoT is
//!   held for it all the same;
//! - a dispel owed: in reach, Dispel Magic goes instead; out of range or out
//!   of sight, the DoT is held for it — at the maintenance bar, not below it;
//! - crowd control owed: a melee on the Priest is screamed, and with the
//!   Scream on cooldown the DoT goes;
//! - kept up: never recast while its own is ticking on the target, though
//!   another caster's does not stop it;
//! - never onto crowd control its own team would break.

use arenasim::states::match_config::CharacterClass::{Mage, Priest, Warrior};
use arenasim::states::play_match::abilities::AbilityType::{
    self, DispelMagic, FlashHeal, MindBlast, PsychicScream, ShadowWordPain,
};
use arenasim::states::play_match::abilities::SpellSchool;
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{Aura, AuraType};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use bevy::prelude::{Entity, Vec2};

const PRIEST: &str = "priest";
const WARRIOR: &str = "warrior";
const MAGE: &str = "enemy mage";
const ENEMY_WARRIOR: &str = "enemy warrior";

const NOW: f32 = 30.0;

/// Below Flash Heal's trigger (0.9), above every urgent threshold.
const HURT: f32 = 0.8;

/// Beyond Flash Heal's 40yd and Dispel Magic's 30yd from the Priest at the
/// origin, still on the arena floor.
const FAR: (f32, f32) = (-45.0, 0.0);

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; candidates {:?}",
        view.chosen,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

fn considered(view: &DecisionView, ability: AbilityType) -> bool {
    view.candidates.iter().any(|c| c.ability == ability)
}

/// The reason `ability` was LAST rejected with: Dispel Magic is asked twice a
/// tick, at the urgent bar and then the maintenance bar.
fn last_rejection(view: &DecisionView, ability: AbilityType) -> Option<RejectionReason> {
    view.candidates
        .iter()
        .rev()
        .find(|c| c.ability == ability)
        .and_then(|c| c.reason.clone())
}

/// The `PreconditionUnmet` note `ability` was rejected with.
fn held_note(view: &DecisionView, ability: AbilityType) -> String {
    match view.rejected(ability) {
        Some(RejectionReason::PreconditionUnmet { note }) => note.clone(),
        other => panic!(
            "{ability:?} was not held by a precondition ({other:?}): {}",
            describe(view)
        ),
    }
}

/// Power Word: Fortitude and a spent Power Word: Shield (Weakened Soul) on
/// `who`, so the Priest has no buff or shield to hand out.
fn settled(scene: Scene, who: &'static str) -> Scene {
    scene
        .aura(who, AuraType::MaxHealthIncrease, 600.0)
        .aura(who, AuraType::WeakenedSoul, 10.0)
}

/// The idle scene: a topped Priest at the origin beside a topped Warrior
/// teammate, both [`settled`], the Priest's kill target an enemy Mage 20yd off
/// that is attacking the Warrior. Mind Blast is on cooldown, so the chain's
/// last damage rung above Shadow Word: Pain has nothing to do; no enemy healer,
/// so no Mana Burn.
fn idle(profile: AiProfile) -> Scene {
    let s = Scene::new()
        .now(NOW)
        .ai_profile(1, profile)
        .unit(PRIEST, 1, Priest)
        .at(0.0, 0.0)
        .targeting(MAGE)
        .cooldown(PRIEST, MindBlast, 5.0)
        .unit(WARRIOR, 1, Warrior)
        .at(3.0, 0.0)
        .targeting(MAGE)
        .unit(MAGE, 2, Mage)
        .at(20.0, 0.0)
        .targeting(WARRIOR);
    settled(settled(s, PRIEST), WARRIOR)
}

/// Idle, the `TeamPlan` Priest puts Shadow Word: Pain on its kill target; the
/// `Legacy` Priest does not consider it at all.
#[test]
fn an_idle_priest_puts_shadow_word_pain_on_its_kill_target() {
    let view = idle(AiProfile::TeamPlan).decide(PRIEST);
    assert_eq!(
        view.chose(),
        Some((ShadowWordPain, Some(MAGE))),
        "TeamPlan: {}",
        describe(&view)
    );

    let view = idle(AiProfile::Legacy).decide(PRIEST);
    assert!(
        !considered(&view, ShadowWordPain),
        "Legacy considers it: {}",
        describe(&view)
    );
}

/// A heal owed in reach goes first: a teammate below Flash Heal's trigger
/// beside the Priest is healed, not the DoT.
#[test]
fn a_heal_in_reach_goes_before_shadow_word_pain() {
    let view = idle(AiProfile::TeamPlan)
        .health_of(WARRIOR, HURT)
        .decide(PRIEST);
    assert_eq!(
        view.chose(),
        Some((FlashHeal, Some(WARRIOR))),
        "{}",
        describe(&view)
    );
}

/// A heal owed out of reach still holds the DoT: the hurt teammate stands
/// beyond Flash Heal's range, so Flash Heal cannot go, and Shadow Word: Pain
/// waits for it. With the same teammate topped, the DoT goes.
#[test]
fn a_heal_owed_out_of_reach_holds_shadow_word_pain() {
    let (x, z) = FAR;
    let held = idle(AiProfile::TeamPlan)
        .place(WARRIOR, x, z)
        .health_of(WARRIOR, HURT)
        .decide(PRIEST);
    assert!(
        held.rejected(FlashHeal).is_some(),
        "the heal was in reach: {}",
        describe(&held)
    );
    assert!(
        held_note(&held, ShadowWordPain).starts_with("a heal is owed"),
        "{}",
        describe(&held)
    );

    let control = idle(AiProfile::TeamPlan)
        .place(WARRIOR, x, z)
        .decide(PRIEST);
    assert_eq!(
        control.chose(),
        Some((ShadowWordPain, Some(MAGE))),
        "{}",
        describe(&control)
    );
}

/// The Priest's own health counts: a hurt Priest owes itself the heal.
#[test]
fn the_priests_own_heal_holds_shadow_word_pain() {
    let view = idle(AiProfile::TeamPlan)
        .health_of(PRIEST, HURT)
        .decide(PRIEST);
    assert_eq!(
        view.chose(),
        Some((FlashHeal, Some(PRIEST))),
        "{}",
        describe(&view)
    );
}

/// A dispel owed in reach goes first: a polymorphed teammate beside the
/// Priest is freed, not the DoT.
#[test]
fn a_dispel_in_reach_goes_before_shadow_word_pain() {
    let view = idle(AiProfile::TeamPlan)
        .aura(WARRIOR, AuraType::Polymorph, 6.0)
        .decide(PRIEST);
    assert_eq!(
        view.chose(),
        Some((DispelMagic, Some(WARRIOR))),
        "{}",
        describe(&view)
    );
}

/// An enemy's magic debuff of `effect` on a teammate.
fn magic_debuff(effect: AuraType) -> Aura {
    Aura {
        effect_type: effect,
        duration: 12.0,
        magnitude: 0.5,
        break_on_damage_threshold: -1.0,
        spell_school: Some(SpellSchool::Shadow),
        ability_name: "an enemy's debuff".to_string(),
        ..Default::default()
    }
}

/// A dispel owed out of reach still holds the DoT: the teammate stands beyond
/// Dispel Magic's range carrying a magic damage over time — the maintenance
/// bar, the lowest a dispel is owed at — and Shadow Word: Pain waits for it.
/// With a snare there instead (below every dispel bar), or with nothing,
/// nothing is owed and the DoT goes.
#[test]
fn a_dispel_owed_out_of_reach_holds_shadow_word_pain() {
    let (x, z) = FAR;
    let far = || idle(AiProfile::TeamPlan).place(WARRIOR, x, z);

    let held = far()
        .aura_full(WARRIOR, magic_debuff(AuraType::DamageOverTime))
        .decide(PRIEST);
    assert!(
        matches!(
            last_rejection(&held, DispelMagic),
            Some(RejectionReason::OutOfRange { .. })
        ),
        "the dispel was in reach: {}",
        describe(&held)
    );
    assert!(
        held_note(&held, ShadowWordPain).starts_with("a dispel is owed"),
        "{}",
        describe(&held)
    );

    for control in [
        far().aura_full(WARRIOR, magic_debuff(AuraType::MovementSpeedSlow)),
        far(),
    ] {
        let view = control.decide(PRIEST);
        assert_eq!(
            view.chose(),
            Some((ShadowWordPain, Some(MAGE))),
            "{}",
            describe(&view)
        );
    }
}

/// A 1.5yd pillar at (-5, 0): between the Priest at the origin and a teammate
/// at (-10, 0), inside Dispel Magic's range; the enemy Mage east of the Priest
/// stays in sight.
fn pillar() -> Vec<ObstacleVolume> {
    vec![ObstacleVolume::Cylinder {
        center_xz: Vec2::new(-5.0, 0.0),
        radius: 1.5,
        base_y: 0.0,
        height: 10.0,
    }]
}

/// A dispel owed behind cover holds the DoT too: the teammate is in range but
/// out of sight. Without the pillar the dispel goes instead.
#[test]
fn a_dispel_owed_out_of_sight_holds_shadow_word_pain() {
    let behind = |obstacles: Vec<ObstacleVolume>| {
        idle(AiProfile::TeamPlan)
            .obstacles(obstacles)
            .place(WARRIOR, -10.0, 0.0)
            .aura_full(WARRIOR, magic_debuff(AuraType::DamageOverTime))
            .decide(PRIEST)
    };

    let held = behind(pillar());
    assert!(
        matches!(
            last_rejection(&held, DispelMagic),
            Some(RejectionReason::LosBlocked)
        ),
        "the dispel was in sight: {}",
        describe(&held)
    );
    assert!(
        held_note(&held, ShadowWordPain).starts_with("a dispel is owed"),
        "{}",
        describe(&held)
    );

    let control = behind(Vec::new());
    assert_eq!(
        control.chose(),
        Some((DispelMagic, Some(WARRIOR))),
        "{}",
        describe(&control)
    );
}

/// The idle scene with an enemy Warrior in melee on the Priest.
fn pressed(scream_cooldown: Option<f32>) -> Scene {
    let mut s = idle(AiProfile::TeamPlan)
        .unit(ENEMY_WARRIOR, 2, Warrior)
        .at(1.5, 0.0)
        .targeting(PRIEST);
    if let Some(secs) = scream_cooldown {
        s = s.cooldown(PRIEST, PsychicScream, secs);
    }
    s
}

/// Crowd control owed goes first: a melee on the Priest is screamed. With the
/// Scream on cooldown nothing is owed that the Priest can cast, and the DoT
/// goes.
#[test]
fn crowd_control_goes_before_shadow_word_pain() {
    let view = pressed(None).decide(PRIEST);
    assert_eq!(
        view.chose().map(|(a, _)| a),
        Some(PsychicScream),
        "{}",
        describe(&view)
    );

    let control = pressed(Some(20.0)).decide(PRIEST);
    assert_eq!(
        control.chose(),
        Some((ShadowWordPain, Some(MAGE))),
        "{}",
        describe(&control)
    );
}

/// A Shadow Word: Pain on the kill target, cast by `caster`.
fn pain_from(caster: Entity) -> Aura {
    Aura {
        effect_type: AuraType::DamageOverTime,
        duration: 12.0,
        magnitude: 10.0,
        tick_interval: 3.0,
        time_until_next_tick: 3.0,
        // As the RON aura carries it: a DoT does not break on damage.
        break_on_damage_threshold: -1.0,
        caster: Some(caster),
        ability_name: "Shadow Word: Pain".to_string(),
        ..Default::default()
    }
}

/// Kept up, not recast: with its own Shadow Word: Pain ticking on the kill
/// target the Priest does not cast it again; another caster's on the same
/// target (the Warrior's, standing in for a second Priest) does not stop it.
#[test]
fn shadow_word_pain_is_not_recast_while_its_own_ticks() {
    let s = idle(AiProfile::TeamPlan);
    let own = pain_from(s.entity(PRIEST));
    let view = s.aura_full(MAGE, own).decide(PRIEST);
    assert!(
        matches!(
            view.rejected(ShadowWordPain),
            Some(RejectionReason::AlreadyApplied)
        ),
        "{}",
        describe(&view)
    );

    let s = idle(AiProfile::TeamPlan);
    let others = pain_from(s.entity(WARRIOR));
    let view = s.aura_full(MAGE, others).decide(PRIEST);
    assert_eq!(
        view.chose(),
        Some((ShadowWordPain, Some(MAGE))),
        "{}",
        describe(&view)
    );
}

/// Never onto crowd control its team would break: with the kill target in
/// the Warrior's Polymorph (breaks on any damage) the DoT is refused; the idle
/// scene without it is [`an_idle_priest_puts_shadow_word_pain_on_its_kill_target`].
#[test]
fn shadow_word_pain_never_breaks_its_teams_crowd_control() {
    let s = idle(AiProfile::TeamPlan);
    let sheep = Aura {
        effect_type: AuraType::Polymorph,
        duration: 8.0,
        break_on_damage_threshold: 0.0,
        caster: Some(s.entity(WARRIOR)),
        ability_name: "Polymorph".to_string(),
        ..Default::default()
    };
    let view = s.aura_full(MAGE, sheep).decide(PRIEST);
    assert!(
        matches!(
            view.rejected(ShadowWordPain),
            Some(RejectionReason::FriendlyBreakableCC)
        ),
        "{}",
        describe(&view)
    );
}
