//! AS-413 — the Priest's Form choice, and the Shadowform role.
//!
//! A Priest whose build takes the Shadowform talent may choose Shadowform for
//! a match. It takes the form before the gates and holds it all match; in it,
//! it heals no one, deals 15% more Shadow damage, takes 15% less Physical
//! damage, and plays a ranged damage dealer: its own rotation, and the role
//! every team-level question reads.
//!
//! Every gate is a PAIR — the situation, and its control with its one
//! condition removed — so no assertion here can pass vacuously:
//!
//! - the choice: offered only to a build with the talent; the default changes
//!   nothing;
//! - the Holy lockout: Flash Heal locked in the form, not out of it; Power
//!   Word: Shield, Dispel Magic and Fortitude still cast in it;
//! - "requires the form": the Shadowform spell (and AS-416's Silence, by the
//!   same rule) is castable only with the form chosen;
//! - the form itself: taken before anything else, never twice;
//! - the rotation: Shadow Word: Pain kept up on the kill target as a core DoT,
//!   Mind Blast, Mana Burn held until the Priest is in reach;
//! - the damage modifiers, through the real aura and its rider;
//! - the role: a damage dealer for the plays, the kill window, the target
//!   focus, the Hunt and the comp profile.

use std::collections::BTreeSet;

use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::states::match_config::CharacterClass::{self, Mage, Priest, Warrior};
use arenasim::states::match_config::PriestForm;
use arenasim::states::play_match::abilities::AbilityType::{
    self, DispelMagic, FlashHeal, ManaBurn, MindBlast, PowerWordFortitude, PowerWordShield,
    PsychicScream, ShadowWordPain, Shadowform,
};
use arenasim::states::play_match::abilities::SpellSchool;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::hunt::can_station;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::class_ai::target_focus::follows_focus;
use arenasim::states::play_match::combat_core::{
    apply_damage_with_absorb, compound_riders, damage_dealt_multiplier, DamageFollowUps, Hit,
};
use arenasim::states::play_match::components::{
    ActiveAuras, AuraPending, AuraType, Combatant, CompoundDebuff,
};
use arenasim::states::play_match::decision_trace::{PlayStep, PlayTag, RejectionReason};
use arenasim::states::play_match::situation::{PlaySet, Role, CHAIN};
use arenasim::states::play_match::talent_choices::{ChoiceMenu, ChoiceOption, SlotChoices};
use arenasim::states::play_match::talents::{Build, TalentDefinitions, TalentId};
use arenasim::states::play_match::team_plan::classify_comp;

const PRIEST: &str = "priest";
const WARRIOR: &str = "warrior";
const MAGE: &str = "enemy mage";
const ENEMY_PRIEST: &str = "enemy priest";

const SHADOWFORM: ChoiceOption = ChoiceOption::PriestForm(PriestForm::Shadowform);
const NO_FORM: ChoiceOption = ChoiceOption::PriestForm(PriestForm::NoForm);

fn defs() -> &'static AbilityDefinitions {
    AbilityDefinitions::shipped()
}

/// A Priest build that takes the Shadowform talent: tier 2 opens at two
/// points in tier 1.
fn shadow_build() -> Build {
    Build::of(&[(TalentId::ImprovedMindBlast, 2), (TalentId::Shadowform, 1)])
}

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; situation {:?}; candidates {:?}",
        view.chosen,
        view.situation,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

fn considered(view: &DecisionView, ability: AbilityType) -> bool {
    view.candidates.iter().any(|c| c.ability == ability)
}

// ---------------------------------------------------------------------------
// The choice
// ---------------------------------------------------------------------------

/// The Form menu offers Shadowform only to a build with the talent, and a
/// slot that picks it without the talent is refused. No form is always
/// offered: it is the default.
#[test]
fn shadowform_is_offered_only_by_its_talent() {
    let t = TalentDefinitions::shipped();
    let none = Build::default();
    assert_eq!(t.offered(&none, ChoiceMenu::PriestForm), vec![NO_FORM]);
    assert_eq!(
        t.offered(&shadow_build(), ChoiceMenu::PriestForm),
        vec![NO_FORM, SHADOWFORM]
    );
    let chose = |o| SlotChoices::of(&[o], &[]);
    assert!(t
        .validate_choices(Priest, &none, &chose(SHADOWFORM))
        .is_err());
    assert!(t
        .validate_choices(Priest, &shadow_build(), &chose(SHADOWFORM))
        .is_ok());
    // Another class cannot choose a form.
    assert!(t.validate_choices(Mage, &none, &chose(SHADOWFORM)).is_err());
}

/// The headless config reads the form per slot, refuses a name it does not
/// know, and refuses Shadowform without the talent; an unset form is no form.
#[test]
fn the_headless_config_takes_the_form() {
    use arenasim::headless::HeadlessMatchConfig;
    let config = |forms: &[&str], builds: Vec<Build>| HeadlessMatchConfig {
        team1: vec!["Warrior".into(), "Priest".into()],
        team2: vec!["Mage".into()],
        team1_priest_forms: forms.iter().map(|s| s.to_string()).collect(),
        team1_builds: builds,
        ..Default::default()
    };
    let ok = config(
        &["NoForm", "Shadowform"],
        vec![Build::default(), shadow_build()],
    )
    .to_match_config()
    .expect("a legal Shadowform slot");
    assert_eq!(
        ok.team1_priest_forms,
        vec![PriestForm::NoForm, PriestForm::Shadowform]
    );
    let unset = config(&[], vec![]).to_match_config().unwrap();
    assert_eq!(unset.team1_priest_forms, vec![PriestForm::NoForm; 2]);
    assert!(config(&["NoForm", "Shadowfrom"], vec![])
        .to_match_config()
        .unwrap_err()
        .contains("Unknown priest form"));
    assert!(config(&["NoForm", "Shadowform"], vec![])
        .to_match_config()
        .is_err());
}

/// A slot whose class changes loses its build, and with it its form: the
/// form is a talent's, and would be illegal with the build gone.
#[test]
fn a_class_change_drops_the_form_with_the_build() {
    use arenasim::states::match_config::MatchConfig;
    let mut config = MatchConfig::default();
    config.set_class(1, 0, Priest);
    config.team1_builds[0] = shadow_build();
    config.team1_priest_forms[0] = PriestForm::Shadowform;
    // The same class again changes nothing.
    config.set_class(1, 0, Priest);
    assert_eq!(config.team1_priest_forms[0], PriestForm::Shadowform);
    config.set_class(1, 0, Mage);
    config.set_class(1, 0, Priest);
    assert_eq!(config.team1_priest_forms[0], PriestForm::NoForm);
    assert!(config.team1_builds[0].is_empty());
}

/// Unlearning the talent in the talent panel drops the form it offered; a
/// build that still offers it keeps it.
#[test]
fn unlearning_the_talent_drops_the_form() {
    use arenasim::states::match_config::MatchConfig;
    let t = TalentDefinitions::shipped();
    let mut config = MatchConfig::default();
    config.set_class(1, 0, Priest);
    config.team1_builds[0] = shadow_build();
    config.team1_priest_forms[0] = PriestForm::Shadowform;
    config.drop_unoffered_priest_form(1, 0, t);
    assert_eq!(config.team1_priest_forms[0], PriestForm::Shadowform);
    config.team1_builds[0] = Build::of(&[(TalentId::ImprovedMindBlast, 2)]);
    config.drop_unoffered_priest_form(1, 0, t);
    assert_eq!(config.team1_priest_forms[0], PriestForm::NoForm);
}

// ---------------------------------------------------------------------------
// The Holy lockout, and what the form requires
// ---------------------------------------------------------------------------

/// The book a Priest with `build` and `form` spawns with.
fn book(
    build: &Build,
    form: ChoiceOption,
) -> arenasim::states::play_match::ability_book::AbilityBook {
    TalentDefinitions::shipped()
        .resolve_with_choices(defs(), Priest, build, &SlotChoices::of(&[form], &[]))
        .expect("a legal Priest")
        .book()
        .clone()
}

/// In the form, Flash Heal is locked and the spells TBC's client lets a
/// Shadowform Priest cast are not. Out of it, nothing is locked but the form's
/// own spell. Every lock, by set equality.
#[test]
fn shadowform_locks_the_heals_and_nothing_else() {
    let locked = |b: &arenasim::states::play_match::ability_book::AbilityBook| {
        b.iter()
            .map(|(a, _)| *a)
            .filter(|a| b.is_locked(*a))
            .collect::<BTreeSet<_>>()
    };
    let in_form = book(&shadow_build(), SHADOWFORM);
    assert_eq!(locked(&in_form), BTreeSet::from([FlashHeal]));
    for allowed in [
        PowerWordShield,
        DispelMagic,
        PowerWordFortitude,
        MindBlast,
        ManaBurn,
        PsychicScream,
        ShadowWordPain,
        Shadowform,
    ] {
        assert!(in_form.get(&allowed).is_some(), "{allowed:?} in the book");
    }
    // Control: the talent without the form.
    let healer = book(&shadow_build(), NO_FORM);
    assert_eq!(locked(&healer), BTreeSet::from([Shadowform]));
    // No talent: no Shadowform spell at all, and nothing locked.
    let plain = book(&Build::default(), NO_FORM);
    assert!(plain.get(&Shadowform).is_none());
    assert!(locked(&plain).is_empty());
}

/// The cast gate refuses a locked heal in the form: a Priest in Shadowform
/// does not heal its dying teammate (it hits its target); the healing Priest
/// in the same scene heals it.
#[test]
fn a_shadowform_priest_does_not_heal_a_dying_teammate() {
    let scene = |shadow: bool| {
        let s = Scene::new()
            .unit(PRIEST, 1, Priest)
            .targeting(MAGE)
            .aura(PRIEST, AuraType::MaxHealthIncrease, 600.0)
            .aura(PRIEST, AuraType::WeakenedSoul, 15.0)
            .unit(WARRIOR, 1, Warrior)
            .at(5.0, 0.0)
            .health(0.3)
            .aura(WARRIOR, AuraType::MaxHealthIncrease, 600.0)
            .aura(WARRIOR, AuraType::WeakenedSoul, 15.0)
            .unit(MAGE, 2, Mage)
            .at(0.0, 25.0);
        if shadow {
            s.in_shadowform(PRIEST)
        } else {
            s
        }
    };
    let healer = scene(false).decide(PRIEST);
    assert_eq!(
        healer.chose(),
        Some((FlashHeal, Some(WARRIOR))),
        "control: {}",
        describe(&healer)
    );
    let shadow = scene(true).decide(PRIEST);
    assert!(
        !shadow.choices.iter().any(|(a, _)| *a == FlashHeal),
        "{}",
        describe(&shadow)
    );
    assert_eq!(
        shadow.chose(),
        Some((ShadowWordPain, Some(MAGE))),
        "{}",
        describe(&shadow)
    );
}

/// Power Word: Shield is castable in the form: a Shadowform Priest shields a
/// hurt teammate. Control: the same teammate under Weakened Soul, which no
/// shield can take.
#[test]
fn a_shadowform_priest_still_shields() {
    let scene = |shieldable: bool| {
        let s = Scene::new()
            .unit(PRIEST, 1, Priest)
            .targeting(MAGE)
            .aura(PRIEST, AuraType::MaxHealthIncrease, 600.0)
            .aura(PRIEST, AuraType::WeakenedSoul, 15.0)
            .unit(WARRIOR, 1, Warrior)
            .at(5.0, 0.0)
            .health(0.3)
            .aura(WARRIOR, AuraType::MaxHealthIncrease, 600.0)
            .unit(MAGE, 2, Mage)
            .at(0.0, 25.0)
            .in_shadowform(PRIEST);
        if shieldable {
            s
        } else {
            s.aura(WARRIOR, AuraType::WeakenedSoul, 15.0)
        }
    };
    let hurt = scene(true).decide(PRIEST);
    assert_eq!(
        hurt.chose(),
        Some((PowerWordShield, Some(WARRIOR))),
        "{}",
        describe(&hurt)
    );
    let soul = scene(false).decide(PRIEST);
    assert_ne!(
        soul.chose().map(|(a, _)| a),
        Some(PowerWordShield),
        "control: {}",
        describe(&soul)
    );
}

/// Dispel Magic is castable in the form: a Shadowform Priest frees a
/// polymorphed teammate, and does not dispel a free one.
#[test]
fn a_shadowform_priest_still_dispels() {
    let scene = |polymorphed: bool| {
        let s = Scene::new()
            .unit(PRIEST, 1, Priest)
            .targeting(MAGE)
            .aura(PRIEST, AuraType::MaxHealthIncrease, 600.0)
            .aura(PRIEST, AuraType::WeakenedSoul, 15.0)
            .unit(WARRIOR, 1, Warrior)
            .at(5.0, 0.0)
            .aura(WARRIOR, AuraType::MaxHealthIncrease, 600.0)
            .aura(WARRIOR, AuraType::WeakenedSoul, 15.0)
            .unit(MAGE, 2, Mage)
            .at(0.0, 25.0)
            .in_shadowform(PRIEST);
        if polymorphed {
            s.aura(WARRIOR, AuraType::Polymorph, 8.0)
        } else {
            s
        }
    };
    let freed = scene(true).decide(PRIEST);
    assert_eq!(
        freed.chose(),
        Some((DispelMagic, Some(WARRIOR))),
        "{}",
        describe(&freed)
    );
    let free = scene(false).decide(PRIEST);
    assert_ne!(
        free.chose().map(|(a, _)| a),
        Some(DispelMagic),
        "control: {}",
        describe(&free)
    );
}

/// Before the gates a Shadowform Priest takes the form first, then buffs its
/// team with Fortitude — castable in the form. Once in the form it never
/// takes it again.
#[test]
fn the_form_comes_first_and_fortitude_still_casts() {
    let scene = || {
        Scene::new()
            .before_gates()
            .unit(PRIEST, 1, Priest)
            .unit(WARRIOR, 1, Warrior)
            .at(3.0, 0.0)
            .unit(MAGE, 2, Mage)
            .at(60.0, 0.0)
    };
    let taking = scene()
        .priest_form(PRIEST, PriestForm::Shadowform)
        .decide(PRIEST);
    assert_eq!(
        taking.chose(),
        Some((Shadowform, Some(PRIEST))),
        "{}",
        describe(&taking)
    );
    let held = scene().in_shadowform(PRIEST).decide(PRIEST);
    assert!(
        matches!(
            held.rejected(Shadowform),
            Some(RejectionReason::AlreadyApplied)
        ),
        "{}",
        describe(&held)
    );
    assert_eq!(
        held.chose().map(|(a, _)| a),
        Some(PowerWordFortitude),
        "{}",
        describe(&held)
    );
    // Control: a healing Priest never considers the form.
    let healer = scene().decide(PRIEST);
    assert!(!considered(&healer, Shadowform), "{}", describe(&healer));
}

/// The form's spell requires the form: granted by the talent but with no form
/// chosen, it is locked, and the cast gate refuses it. With the form chosen
/// it is not. This is the rule AS-416's Silence takes.
#[test]
fn the_form_spell_requires_the_form() {
    use arenasim::states::play_match::class_ai::cast_guard::{own_state_ok, PreCastOpts};
    let def = defs().get_unchecked(&Shadowform);
    let gate = |form| {
        let c = Combatant::new(1, 0, Priest).with_book(book(&shadow_build(), form));
        own_state_ok(Shadowform, def, &c, None, PreCastOpts::default())
    };
    assert!(!gate(NO_FORM));
    assert!(gate(SHADOWFORM));
}

/// "An ability requires an option", on a Priest menu: the hook AS-416's
/// Silence uses, here on a stand-in ability. Locked unless the form is chosen.
#[test]
fn a_priest_ability_can_require_the_form() {
    let text = std::fs::read_to_string(arenasim::paths::asset_path("config/talents.ron"))
        .expect("talents.ron reads");
    let with_rule = text.replacen(
        "\n    choice_rules: [\n",
        "\n    choice_rules: [\n        Requires(ability: PsychicScream, option: PriestForm(Shadowform)),\n",
        1,
    );
    assert_ne!(with_rule, text);
    let t = TalentDefinitions::from_ron(&with_rule, defs()).expect("the rule loads");
    let locked = |form| {
        t.resolve_with_choices(
            defs(),
            Priest,
            &shadow_build(),
            &SlotChoices::of(&[form], &[]),
        )
        .unwrap()
        .book()
        .is_locked(PsychicScream)
    };
    assert!(locked(NO_FORM));
    assert!(!locked(SHADOWFORM));
}

// ---------------------------------------------------------------------------
// The rotation
// ---------------------------------------------------------------------------

/// A Shadowform Priest, topped and settled, its kill target an enemy Mage
/// 20yd off.
fn rotation(profile: AiProfile) -> Scene {
    Scene::new()
        .ai_profile(1, profile)
        .unit(PRIEST, 1, Priest)
        .targeting(MAGE)
        .aura(PRIEST, AuraType::MaxHealthIncrease, 600.0)
        .aura(PRIEST, AuraType::WeakenedSoul, 15.0)
        .unit(WARRIOR, 1, Warrior)
        .at(3.0, 0.0)
        .aura(WARRIOR, AuraType::MaxHealthIncrease, 600.0)
        .aura(WARRIOR, AuraType::WeakenedSoul, 15.0)
        .unit(MAGE, 2, Mage)
        .at(20.0, 0.0)
        .targeting(WARRIOR)
        .in_shadowform(PRIEST)
}

/// Shadow Word: Pain is a core DoT in the form, under both profiles: put on
/// the kill target first, then kept up — with its own ticking, Mind Blast
/// goes.
#[test]
fn shadow_word_pain_is_kept_up_then_mind_blast() {
    for profile in [AiProfile::Legacy, AiProfile::TeamPlan] {
        let first = rotation(profile).decide(PRIEST);
        assert_eq!(
            first.chose(),
            Some((ShadowWordPain, Some(MAGE))),
            "{profile:?}: {}",
            describe(&first)
        );
        let mut ticking = rotation(profile);
        let priest = ticking.entity(PRIEST);
        let dot = AuraPending::from_ability(
            ticking.entity(MAGE),
            priest,
            defs().get_unchecked(&ShadowWordPain),
        )
        .unwrap()
        .aura;
        ticking = ticking.aura_full(MAGE, dot);
        let next = ticking.decide(PRIEST);
        assert!(
            matches!(
                next.rejected(ShadowWordPain),
                Some(RejectionReason::AlreadyApplied)
            ),
            "{profile:?}: {}",
            describe(&next)
        );
        assert_eq!(
            next.chose(),
            Some((MindBlast, Some(MAGE))),
            "{profile:?}: {}",
            describe(&next)
        );
    }
}

/// A teammate below Flash Heal's trigger holds a HEALING Priest's Shadow Word:
/// Pain (it owes the heal); a Shadowform Priest owes no heal and casts it.
#[test]
fn a_hurt_teammate_does_not_hold_the_core_dot() {
    let scene = |shadow: bool| {
        let s = Scene::new()
            .ai_profile(1, AiProfile::TeamPlan)
            .unit(PRIEST, 1, Priest)
            .targeting(MAGE)
            .cooldown(PRIEST, MindBlast, 5.0)
            .aura(PRIEST, AuraType::MaxHealthIncrease, 600.0)
            .aura(PRIEST, AuraType::WeakenedSoul, 15.0)
            .unit(WARRIOR, 1, Warrior)
            .at(-45.0, 0.0)
            .health(0.8)
            .aura(WARRIOR, AuraType::MaxHealthIncrease, 600.0)
            .aura(WARRIOR, AuraType::WeakenedSoul, 15.0)
            .unit(MAGE, 2, Mage)
            .at(20.0, 0.0);
        if shadow {
            s.in_shadowform(PRIEST)
        } else {
            s
        }
    };
    let healer = scene(false).decide(PRIEST);
    assert!(
        matches!(
            healer.rejected(ShadowWordPain),
            Some(RejectionReason::PreconditionUnmet { note }) if note.starts_with("a heal is owed")
        ),
        "control: {}",
        describe(&healer)
    );
    let shadow = scene(true).decide(PRIEST);
    assert_eq!(
        shadow.chose(),
        Some((ShadowWordPain, Some(MAGE))),
        "{}",
        describe(&shadow)
    );
}

/// The kill target beyond Mind Blast's 30 yards: the Shadowform Priest holds
/// its 40-yard Mana Burn and closes. Inside 30, with its DoT up and Mind
/// Blast cooling, it burns the enemy healer.
#[test]
fn mana_burn_waits_until_the_priest_is_in_reach() {
    let scene = |dist: f32| {
        let mut s = Scene::new()
            .unit(PRIEST, 1, Priest)
            .targeting(MAGE)
            .cooldown(PRIEST, MindBlast, 5.0)
            .aura(PRIEST, AuraType::MaxHealthIncrease, 600.0)
            .aura(PRIEST, AuraType::WeakenedSoul, 15.0)
            .unit(MAGE, 2, Mage)
            .at(dist, 0.0)
            .unit(ENEMY_PRIEST, 2, Priest)
            .at(dist, 5.0)
            .in_shadowform(PRIEST);
        let dot = AuraPending::from_ability(
            s.entity(MAGE),
            s.entity(PRIEST),
            defs().get_unchecked(&ShadowWordPain),
        )
        .unwrap()
        .aura;
        s = s.aura_full(MAGE, dot);
        s
    };
    let near = scene(25.0).decide(PRIEST);
    assert_eq!(
        near.chose(),
        Some((ManaBurn, Some(ENEMY_PRIEST))),
        "{}",
        describe(&near)
    );
    let far = scene(35.0).decide(PRIEST);
    assert!(
        matches!(
            far.rejected(ManaBurn),
            Some(RejectionReason::PreconditionUnmet { note }) if note.starts_with("closing")
        ),
        "{}",
        describe(&far)
    );
    assert_eq!(far.chose(), None, "{}", describe(&far));
}

/// The defensive Psychic Scream holds for a critical heal a healing Priest
/// could cast; a Shadowform Priest has none to cast, and screams.
#[test]
fn a_shadowform_priest_screams_without_waiting_for_a_heal() {
    const BRUTE: &str = "enemy warrior";
    let scene = |shadow: bool| {
        let s = Scene::new()
            .unit(PRIEST, 1, Priest)
            .targeting(BRUTE)
            .aura(PRIEST, AuraType::MaxHealthIncrease, 600.0)
            .aura(PRIEST, AuraType::WeakenedSoul, 15.0)
            .unit(WARRIOR, 1, Warrior)
            .at(10.0, 0.0)
            .health(0.2)
            .aura(WARRIOR, AuraType::MaxHealthIncrease, 600.0)
            .aura(WARRIOR, AuraType::WeakenedSoul, 15.0)
            .unit(BRUTE, 2, Warrior)
            .at(1.5, 0.0)
            .targeting(PRIEST);
        if shadow {
            s.in_shadowform(PRIEST)
        } else {
            s
        }
    };
    let healer = scene(false).decide(PRIEST);
    assert_ne!(
        healer.chose().map(|(a, _)| a),
        Some(PsychicScream),
        "control: {}",
        describe(&healer)
    );
    let shadow = scene(true).decide(PRIEST);
    assert_eq!(
        shadow.chose().map(|(a, _)| a),
        Some(PsychicScream),
        "{}",
        describe(&shadow)
    );
}

// ---------------------------------------------------------------------------
// The damage modifiers
// ---------------------------------------------------------------------------

/// The form as its own cast lands it: the aura `abilities.ron` describes, and
/// its rider.
fn the_form() -> ActiveAuras {
    let mut world = World::new();
    let me = world.spawn_empty().id();
    let face = AuraPending::from_ability(me, me, defs().get_unchecked(&Shadowform))
        .unwrap()
        .aura;
    assert_eq!(face.compound, Some(CompoundDebuff::Shadowform));
    let riders = compound_riders(CompoundDebuff::Shadowform);
    ActiveAuras {
        auras: std::iter::once(face).chain(riders).collect(),
    }
}

/// +15% Shadow damage dealt, nothing else dealt; -15% Physical damage taken,
/// nothing else taken. Control: the same hits with no form.
#[test]
fn shadowform_raises_shadow_dealt_and_cuts_physical_taken() {
    let form = the_form();
    assert!((damage_dealt_multiplier(Some(&form), SpellSchool::Shadow) - 1.15).abs() < 1e-6);
    assert_eq!(damage_dealt_multiplier(Some(&form), SpellSchool::Fire), 1.0);
    assert_eq!(damage_dealt_multiplier(None, SpellSchool::Shadow), 1.0);

    let mut world = World::new();
    let victim = world.spawn_empty().id();
    let taken = |school: SpellSchool, auras: Option<&mut ActiveAuras>| {
        let mut c = Combatant::new(1, 0, Priest);
        c.max_health = 500.0;
        c.current_health = 500.0;
        c.armor = 0.0;
        c.shadow_resistance = 0.0;
        let hit = Hit::new(100.0, school, "Probe", None, None, victim);
        apply_damage_with_absorb(
            hit,
            &mut c,
            auras,
            &mut DamageFollowUps::default(),
            &mut CombatLog::default(),
        )
        .0
    };
    let mut on = the_form();
    assert!((taken(SpellSchool::Physical, Some(&mut on)) - 85.0).abs() < 1e-3);
    assert_eq!(taken(SpellSchool::Shadow, Some(&mut the_form())), 100.0);
    assert_eq!(taken(SpellSchool::Physical, None), 100.0);
}

// ---------------------------------------------------------------------------
// The role
// ---------------------------------------------------------------------------

/// A Shadowform Priest is a ranged damage dealer; a Priest without a form is a
/// healer.
#[test]
fn the_form_decides_the_role() {
    let mut c = Combatant::new(1, 0, Priest);
    assert_eq!(c.role(), Role::Healer);
    assert!(c.is_healer());
    c.priest_form = PriestForm::Shadowform;
    assert_eq!(c.role(), Role::RangedDps);
    assert!(!c.is_healer());
    // The class-keyed role questions, answered for the role.
    assert!(follows_focus(Role::RangedDps) && !follows_focus(Role::Healer));
    assert!(can_station(Role::RangedDps) && can_station(Role::Healer));
    assert!(!can_station(Role::MeleeDps));
    // The comp profile reads it: Warrior + Shadowform Priest has no healer.
    let comp = |role| classify_comp(&[(Warrior, Role::MeleeDps), (Priest, role)]);
    assert!(comp(Role::Healer).has_healer && !comp(Role::Healer).has_ranged);
    assert!(!comp(Role::RangedDps).has_healer && comp(Role::RangedDps).has_ranged);
}

/// The Burst play calls a Shadowform Priest as a DPS: in a kill window it
/// bursts with Mind Blast on the low enemy. The healing Priest in the same
/// scene runs no play.
#[test]
fn a_shadowform_priest_bursts_in_a_kill_window() {
    const LOW: &str = "low enemy";
    let scene = |shadow: bool| {
        let s = Scene::new()
            .ai_profile(1, AiProfile::TeamPlan)
            .except_everywhere(1, PlaySet::of(&[CHAIN]))
            .unit(PRIEST, 1, Priest)
            .targeting(LOW)
            .aura(PRIEST, AuraType::MaxHealthIncrease, 600.0)
            .aura(PRIEST, AuraType::WeakenedSoul, 15.0)
            .unit(WARRIOR, 1, Warrior)
            .at(-5.0, 0.0)
            .aura(WARRIOR, AuraType::MaxHealthIncrease, 600.0)
            .aura(WARRIOR, AuraType::WeakenedSoul, 15.0)
            .unit(LOW, 2, Warrior)
            .at(20.0, 0.0)
            .health(0.2)
            .unit(ENEMY_PRIEST, 2, Priest)
            .at(20.0, 30.0)
            .aura(ENEMY_PRIEST, AuraType::Stun, 4.0);
        if shadow {
            s.in_shadowform(PRIEST)
        } else {
            s
        }
    };
    let shadow = scene(true).decide(PRIEST);
    let situation = shadow.situation.expect("a situation");
    assert!(situation.kill_window.is_some(), "{}", describe(&shadow));
    assert_eq!(
        situation.play,
        Some(PlayTag::Burst),
        "{}",
        describe(&shadow)
    );
    assert_eq!(situation.play_step, Some(PlayStep::Burst));
    assert_eq!(shadow.chose(), Some((MindBlast, Some(LOW))));

    let healer = scene(false).decide(PRIEST);
    let situation = healer.situation.expect("a situation");
    assert!(situation.kill_window.is_some());
    assert_eq!(situation.play, None, "control: {}", describe(&healer));
}

/// An ENEMY Shadowform Priest is no enemy healer: the kill window ("beyond
/// every enemy healer's help") opens on a low enemy beside it, where a healing
/// Priest in its place keeps it shut.
#[test]
fn an_enemy_shadowform_priest_is_no_enemy_healer() {
    const ME: &str = "me";
    const LOW: &str = "low enemy";
    let scene = |shadow: bool| {
        let s = Scene::new()
            .ai_profile(1, AiProfile::TeamPlan)
            .unit(ME, 1, Mage)
            .targeting(LOW)
            .unit(LOW, 2, Warrior)
            .at(20.0, 0.0)
            .health(0.2)
            .unit(ENEMY_PRIEST, 2, Priest)
            .at(20.0, 10.0);
        if shadow {
            s.in_shadowform(ENEMY_PRIEST)
        } else {
            s
        }
    };
    let healer = scene(false).decide(ME);
    assert!(
        healer.situation.unwrap().kill_window.is_none(),
        "control: {}",
        describe(&healer)
    );
    let shadow = scene(true).decide(ME);
    assert!(
        shadow.situation.unwrap().kill_window.is_some(),
        "{}",
        describe(&shadow)
    );
}

/// Every Priest ability a Shadowform Priest decides in its rotation is one
/// TBC's client lets it cast in the form, and no heal is among them.
#[test]
fn the_rotation_casts_only_what_the_form_allows() {
    let allowed: BTreeSet<AbilityType> = [
        Shadowform,
        PowerWordFortitude,
        PowerWordShield,
        DispelMagic,
        PsychicScream,
        ShadowWordPain,
        MindBlast,
        ManaBurn,
    ]
    .into();
    for profile in [AiProfile::Legacy, AiProfile::TeamPlan] {
        let view = rotation(profile).decide(PRIEST);
        let decided: BTreeSet<AbilityType> = view.candidates.iter().map(|c| c.ability).collect();
        assert!(!decided.is_empty());
        assert!(
            decided.is_subset(&allowed),
            "{profile:?}: {:?}",
            decided.difference(&allowed).collect::<Vec<_>>()
        );
    }
}

/// The Peel play calls a Shadowform Priest as it calls the healing Priest: its
/// teammate dying with an enemy Warrior on it, inside the scream, it screams.
/// Control: the play excepted everywhere on its team.
#[test]
fn a_shadowform_priest_peels_with_its_scream() {
    use arenasim::states::play_match::situation::PEEL;
    const ME: &str = "me";
    const ALLY: &str = "ally";
    const ATTACKER: &str = "enemy warrior";
    let scene = || {
        Scene::new()
            .ai_profile(1, AiProfile::TeamPlan)
            .unit(ME, 1, Priest)
            .targeting(ENEMY_PRIEST)
            .unit(ALLY, 1, Mage)
            .at(3.0, 0.0)
            .health(0.3)
            .targeting(ENEMY_PRIEST)
            .unit(ATTACKER, 2, Warrior)
            .at(4.5, 0.0)
            .targeting(ALLY)
            .unit(ENEMY_PRIEST, 2, Priest)
            .at(0.0, 30.0)
            .in_shadowform(ME)
    };
    let with = scene().decide(ME);
    let situation = with.situation.expect("a situation");
    assert!(
        matches!(situation.play, Some(PlayTag::Peel { .. })),
        "{}",
        describe(&with)
    );
    assert_eq!(
        with.chose(),
        Some((PsychicScream, None)),
        "{}",
        describe(&with)
    );
    assert_eq!(situation.play_step, Some(PlayStep::Peel));

    let without = scene()
        .except_everywhere(1, PlaySet::of(&[PEEL]))
        .decide(ME);
    assert_eq!(without.situation.and_then(|s| s.play_step), None);
    assert_ne!(
        without.chose(),
        Some((PsychicScream, None)),
        "control: {}",
        describe(&without)
    );
}

/// Mana Burn is aimed at the enemy healer (`CombatContext::enemy_healer`, a
/// role): an enemy Priest in Shadowform is none, so there is nothing to burn;
/// the same enemy Priest without the form is burned.
#[test]
fn an_enemy_shadowform_priest_is_not_mana_burned() {
    let scene = |enemy_shadow: bool| {
        let mut s = Scene::new()
            .unit(PRIEST, 1, Priest)
            .targeting(MAGE)
            .cooldown(PRIEST, MindBlast, 5.0)
            .aura(PRIEST, AuraType::MaxHealthIncrease, 600.0)
            .aura(PRIEST, AuraType::WeakenedSoul, 15.0)
            .unit(MAGE, 2, Mage)
            .at(25.0, 0.0)
            .unit(ENEMY_PRIEST, 2, Priest)
            .at(25.0, 5.0)
            .in_shadowform(PRIEST);
        let dot = AuraPending::from_ability(
            s.entity(MAGE),
            s.entity(PRIEST),
            defs().get_unchecked(&ShadowWordPain),
        )
        .unwrap()
        .aura;
        s = s.aura_full(MAGE, dot);
        if enemy_shadow {
            s.in_shadowform(ENEMY_PRIEST)
        } else {
            s
        }
    };
    let healer = scene(false).decide(PRIEST);
    assert_eq!(
        healer.chose(),
        Some((ManaBurn, Some(ENEMY_PRIEST))),
        "control: {}",
        describe(&healer)
    );
    let shadow = scene(true).decide(PRIEST);
    assert!(
        matches!(
            shadow.rejected(ManaBurn),
            Some(RejectionReason::NoValidTarget)
        ),
        "{}",
        describe(&shadow)
    );
}

/// The Shaman's Wind Shear takes an enemy HEALER's cast first: a casting
/// Priest beyond a nearer casting Mage is the one sheared. A Shadowform
/// Priest is no healer, so the nearer Mage is.
#[test]
fn wind_shear_takes_no_shadowform_priest_for_a_healer() {
    const SHAMAN: &str = "shaman";
    let scene = |shadow: bool| {
        let s = Scene::new()
            .unit(SHAMAN, 1, CharacterClass::Shaman)
            .targeting(MAGE)
            .unit(MAGE, 2, Mage)
            .at(10.0, 0.0)
            .casting(MAGE, AbilityType::Frostbolt)
            .unit(ENEMY_PRIEST, 2, Priest)
            .at(18.0, 0.0)
            .casting(ENEMY_PRIEST, MindBlast);
        if shadow {
            s.in_shadowform(ENEMY_PRIEST)
        } else {
            s
        }
    };
    let healer = scene(false).interrupt(SHAMAN);
    assert_eq!(
        healer.chose(),
        Some((AbilityType::WindShear, Some(ENEMY_PRIEST))),
        "control: {}",
        describe(&healer)
    );
    let shadow = scene(true).interrupt(SHAMAN);
    assert_eq!(
        shadow.chose(),
        Some((AbilityType::WindShear, Some(MAGE))),
        "{}",
        describe(&shadow)
    );
}
