//! Talent AI audit (AS-297).
//!
//! A talent ability (`talent: true` in `abilities.ron`) is in no class's base
//! book: only the talent that grants it adds it. So each needs its own
//! decision rule in its class's AI, and that rule must hold in both books:
//!
//! - **granted**, the class AI decides it: the ability is among the
//!   candidates `decide` reports, chosen or rejected with its reason;
//! - **not granted**, the class AI leaves it alone: deciding does not panic
//!   (a rule that asks the book for an ability it lacks with `get_unchecked`
//!   does) and the ability is not among the candidates.
//!
//! Each talent ability names, in [`SCENES`], the scene its class's unit
//! decides in. The rows are checked against the table by set equality, so a
//! new talent ability fails this audit until it has a row and its rule
//! passes both halves; a row for an ability that is not a talent fails too.

use std::collections::BTreeSet;
use std::panic::{catch_unwind, AssertUnwindSafe};

use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::{AbilitiesConfig, AbilityDefinitions};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene, Who};
use arenasim::states::play_match::components::{Aura, AuraType};
use arenasim::states::play_match::movement_config::load_movement_config;

/// A scene builder for one talent ability: its class's unit (the `Who`)
/// WITHOUT the ability, in a situation its rule decides. The audit grants it.
type SceneFor = fn(&AbilityDefinitions) -> (Scene, Who);

/// Every talent ability, with the scene its rule is decided in.
const SCENES: &[(AbilityType, SceneFor)] = &[
    (AbilityType::ExposeArmor, expose_armor_scene),
    (AbilityType::Shadowform, shadowform_scene),
    (AbilityType::DivineProtection, paladin_under_attack),
    (AbilityType::BlessingOfSacrifice, paladin_ally_under_attack),
    (AbilityType::SacredShield, paladin_ally_pressured),
    (AbilityType::SiphonLife, warlock_dots_up),
    (AbilityType::AmplifyCurse, warlock_dots_up),
    (AbilityType::InnerFocus, priest_ally_dying),
    (AbilityType::PowerInfusion, priest_kill_window),
    (AbilityType::PainSuppression, priest_ally_dying),
    (AbilityType::DivineFavor, paladin_ally_under_attack),
    (AbilityType::AuraMastery, paladin_ally_casting_by_a_rogue),
];

/// A Warlock 20 yards from its kill target, a Warrior, with Corruption and
/// Unstable Affliction of its own already on it and Curse of Agony its curse:
/// Siphon Life's moment, and then Amplify Curse's ahead of the curse.
fn warlock_dots_up(defs: &AbilityDefinitions) -> (Scene, Who) {
    let mut scene = Scene::with_config(defs.clone(), load_movement_config().unwrap())
        .unit(WARLOCK, 1, CharacterClass::Warlock)
        .at(0.0, 0.0)
        .targeting(WARRIOR)
        .unit(WARRIOR, 2, CharacterClass::Warrior)
        .at(20.0, 0.0);
    let me = scene.entity(WARLOCK);
    for name in ["Corruption", "Unstable Affliction"] {
        scene = scene.aura_full(
            WARRIOR,
            Aura {
                effect_type: AuraType::DamageOverTime,
                duration: 15.0,
                magnitude: 10.0,
                tick_interval: 3.0,
                ability_name: name.into(),
                caster: Some(me),
                break_on_damage_threshold: -1.0,
                ..Default::default()
            },
        );
    }
    scene.combatant_mut(WARLOCK).warlock_curse_prefs =
        vec![arenasim::states::match_config::WarlockCurse::Agony];
    (scene, WARLOCK)
}

const PALADIN: Who = "paladin";
const MAGE: Who = "mage";

/// Devotion Aura already up, so the Paladin's aura step is out of the way.
fn devotion() -> Aura {
    Aura {
        effect_type: AuraType::DamageTakenReduction,
        duration: 600.0,
        magnitude: 0.10,
        ability_name: "Devotion Aura".into(),
        ..Default::default()
    }
}

/// A Paladin at 40% health with an enemy Warrior on it: Divine Protection's
/// moment.
fn paladin_under_attack(defs: &AbilityDefinitions) -> (Scene, Who) {
    let scene = Scene::with_config(defs.clone(), load_movement_config().unwrap())
        .unit(PALADIN, 1, CharacterClass::Paladin)
        .health(0.4)
        .aura_full(PALADIN, devotion())
        .unit(WARRIOR, 2, CharacterClass::Warrior)
        .at(2.0, 0.0)
        .targeting(PALADIN);
    (scene, PALADIN)
}

/// A healthy Paladin whose Mage, at `health`, has an enemy Warrior on it.
fn paladin_ally_at(defs: &AbilityDefinitions, health: f32) -> (Scene, Who) {
    let scene = Scene::with_config(defs.clone(), load_movement_config().unwrap())
        .unit(PALADIN, 1, CharacterClass::Paladin)
        .aura_full(PALADIN, devotion())
        .unit(MAGE, 1, CharacterClass::Mage)
        .at(15.0, 0.0)
        .health(health)
        .unit(WARRIOR, 2, CharacterClass::Warrior)
        .at(17.0, 0.0)
        .targeting(MAGE);
    (scene, PALADIN)
}

/// The Mage dying under the Warrior: Blessing of Sacrifice's moment.
fn paladin_ally_under_attack(defs: &AbilityDefinitions) -> (Scene, Who) {
    paladin_ally_at(defs, 0.3)
}

/// The Mage mid-Frostbolt with an enemy Rogue beside it, its Kick ready:
/// Aura Mastery's moment.
fn paladin_ally_casting_by_a_rogue(defs: &AbilityDefinitions) -> (Scene, Who) {
    let mut scene = Scene::with_config(defs.clone(), load_movement_config().unwrap())
        .unit(PALADIN, 1, CharacterClass::Paladin)
        .aura_full(PALADIN, devotion())
        .unit(MAGE, 1, CharacterClass::Mage)
        .at(10.0, 0.0)
        .casting(MAGE, AbilityType::Frostbolt)
        .unit("enemy rogue", 2, CharacterClass::Rogue)
        .at(12.0, 0.0)
        .targeting(MAGE);
    scene.combatant_mut("enemy rogue").stealthed = false;
    (scene, PALADIN)
}

/// The Mage pressured but healthy: Sacred Shield's moment.
fn paladin_ally_pressured(defs: &AbilityDefinitions) -> (Scene, Who) {
    paladin_ally_at(defs, 0.8)
}

/// A Rogue in melee of its kill target, a plate-armored Warrior: Expose
/// Armor's moment (`class_ai::rogue::try_expose_armor`).
fn expose_armor_scene(defs: &AbilityDefinitions) -> (Scene, Who) {
    const ROGUE: Who = "rogue";
    let mut scene = Scene::with_config(defs.clone(), load_movement_config().unwrap())
        .unit(ROGUE, 1, CharacterClass::Rogue)
        .at(0.0, 0.0)
        .targeting("plate warrior")
        .unit("plate warrior", 2, CharacterClass::Warrior)
        .at(1.5, 0.0)
        .armor(2300.0)
        // Below the Kidney Shot planner: a ready stun would take the turn.
        .cooldown(ROGUE, AbilityType::KidneyShot, 10.0);
    scene.combatant_mut(ROGUE).stealthed = false;
    (scene, ROGUE)
}

const PRIEST: Who = "priest";

/// A Priest whose Form choice is Shadowform, before the gates and not yet in
/// the form: the moment it takes it.
fn shadowform_scene(defs: &AbilityDefinitions) -> (Scene, Who) {
    let mut scene = Scene::with_config(defs.clone(), load_movement_config().unwrap())
        .before_gates()
        .unit(PRIEST, 1, CharacterClass::Priest)
        .at(0.0, 0.0);
    scene.combatant_mut(PRIEST).priest_form =
        arenasim::states::match_config::PriestForm::Shadowform;
    (scene, PRIEST)
}

/// Power Word: Fortitude, already up, so the Priest's first priority is out
/// of the way.
fn fortitude() -> Aura {
    Aura {
        effect_type: AuraType::MaxHealthIncrease,
        duration: 600.0,
        ability_name: "Power Word: Fortitude".into(),
        ..Default::default()
    }
}

/// A Priest whose Warrior teammate, at 30%, has an enemy Warrior on it and
/// Weakened Soul (so no shield goes out first): Pain Suppression's moment,
/// and Inner Focus's ahead of the Flash Heal. The Priest has Weakened Soul
/// too: it would shield itself first.
fn priest_ally_dying(defs: &AbilityDefinitions) -> (Scene, Who) {
    const ALLY: Who = "warrior";
    let scene = Scene::with_config(defs.clone(), load_movement_config().unwrap())
        .unit(PRIEST, 1, CharacterClass::Priest)
        .aura_full(PRIEST, fortitude())
        .aura(PRIEST, AuraType::WeakenedSoul, 10.0)
        .unit(ALLY, 1, CharacterClass::Warrior)
        .at(10.0, 0.0)
        .health(0.3)
        .aura_full(ALLY, fortitude())
        .aura(ALLY, AuraType::WeakenedSoul, 10.0)
        .unit(WARRIOR, 2, CharacterClass::Warrior)
        .at(11.5, 0.0)
        .targeting(ALLY);
    (scene, PRIEST)
}

/// A Priest whose Mage teammate's enemy, a lone Warrior, is at 20% with no
/// healer to save it: a kill window, Power Infusion's moment.
fn priest_kill_window(defs: &AbilityDefinitions) -> (Scene, Who) {
    const ALLY: Who = "mage";
    let scene = Scene::with_config(defs.clone(), load_movement_config().unwrap())
        .unit(PRIEST, 1, CharacterClass::Priest)
        .aura_full(PRIEST, fortitude())
        .unit(ALLY, 1, CharacterClass::Mage)
        .at(10.0, 0.0)
        .aura_full(ALLY, fortitude())
        .targeting(WARRIOR)
        .unit(WARRIOR, 2, CharacterClass::Warrior)
        .at(30.0, 0.0)
        .health(0.2);
    (scene, PRIEST)
}

fn considered(view: &DecisionView, ability: AbilityType) -> bool {
    view.candidates.iter().any(|c| c.ability == ability)
}

/// The audit, over a table and its rows.
fn audit(defs: &AbilityDefinitions, rows: &[(AbilityType, SceneFor)]) -> Result<(), String> {
    let talents: BTreeSet<AbilityType> = defs.talent_abilities().into_iter().collect();
    let named: BTreeSet<AbilityType> = rows.iter().map(|(a, _)| *a).collect();
    if talents != named {
        return Err(format!(
            "the talent abilities and the audit's rows differ.\n  talent abilities with no \
             row: {:?}\n  rows that are not talent abilities: {:?}",
            talents.difference(&named).collect::<Vec<_>>(),
            named.difference(&talents).collect::<Vec<_>>(),
        ));
    }
    let mut problems = Vec::new();
    for (ability, scene_for) in rows {
        let (scene, who) = scene_for(defs);
        let granted = scene.grant(who, *ability);
        match catch_unwind(AssertUnwindSafe(|| granted.decide(who))) {
            Ok(view) if considered(&view, *ability) => {}
            Ok(_) => problems.push(format!(
                "{ability:?}: granted, {who}'s AI never decides it in its scene"
            )),
            Err(_) => problems.push(format!("{ability:?}: granted, {who}'s AI panics")),
        }
        let (scene, who) = scene_for(defs);
        match catch_unwind(AssertUnwindSafe(|| scene.decide(who))) {
            Ok(view) if !considered(&view, *ability) => {}
            Ok(_) => problems.push(format!(
                "{ability:?}: not granted, {who}'s AI still decides it"
            )),
            Err(_) => problems.push(format!(
                "{ability:?}: not granted, {who}'s AI panics (it asks the book for an \
                 ability it lacks)"
            )),
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

#[test]
fn every_talent_ability_is_decided_by_its_class_ai() {
    let defs = AbilityDefinitions::shipped();
    if let Err(e) = audit(defs, SCENES) {
        panic!("{e}");
    }
    // The inventory the rows were checked against is the shipped table's.
    assert_eq!(
        defs.talent_abilities(),
        SCENES.iter().map(|(a, _)| *a).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// The audit's own checks, on a fixture table that makes Death Coil a talent.
// The shipped Warlock AI decides Death Coil, but asks its book for it with
// `get_unchecked`: granted it passes, ungranted it panics.
// ---------------------------------------------------------------------------

fn flagged(ability: AbilityType) -> AbilityDefinitions {
    let text = std::fs::read_to_string(arenasim::paths::asset_path("config/abilities.ron"))
        .expect("abilities.ron reads");
    let mut config: AbilitiesConfig = ron::from_str(&text).expect("abilities.ron parses");
    config.abilities.get_mut(&ability).unwrap().talent = true;
    AbilityDefinitions::new(config)
}

const WARLOCK: Who = "warlock";
const WARRIOR: Who = "enemy warrior";

/// A Warlock whose kill target, a Warrior, is on it: Death Coil's moment.
fn coil_scene(defs: &AbilityDefinitions) -> (Scene, Who) {
    let scene = Scene::with_config(defs.clone(), load_movement_config().unwrap())
        .unit(WARLOCK, 1, CharacterClass::Warlock)
        .at(0.0, 0.0)
        .health(0.4)
        .targeting(WARRIOR)
        .unit(WARRIOR, 2, CharacterClass::Warrior)
        .at(2.0, 0.0)
        .targeting(WARLOCK);
    (scene, WARLOCK)
}

/// The same Warlock, with no enemy at all: nothing to Death Coil.
fn empty_scene(defs: &AbilityDefinitions) -> (Scene, Who) {
    let scene = Scene::with_config(defs.clone(), load_movement_config().unwrap())
        .unit(WARLOCK, 1, CharacterClass::Warlock)
        .at(0.0, 0.0);
    (scene, WARLOCK)
}

/// The shipped rows with `row` added: the fixture's Death Coil beside the
/// talent abilities the shipped table already has.
fn and(row: (AbilityType, SceneFor)) -> Vec<(AbilityType, SceneFor)> {
    SCENES.iter().copied().chain([row]).collect()
}

#[test]
fn a_talent_ability_with_no_row_fails() {
    let defs = flagged(AbilityType::DeathCoil);
    let err = audit(&defs, SCENES).unwrap_err();
    assert!(err.contains("with no row: [DeathCoil]"), "{err}");
}

#[test]
fn a_row_for_an_ability_that_is_not_a_talent_fails() {
    let defs = AbilityDefinitions::shipped();
    let err = audit(defs, &and((AbilityType::DeathCoil, coil_scene))).unwrap_err();
    assert!(err.contains("not talent abilities: [DeathCoil]"), "{err}");
}

/// Granted, the Warlock decides Death Coil in its scene; ungranted, its
/// `get_unchecked` panics, and the audit names that half.
#[test]
fn an_ai_that_asks_for_an_ungranted_ability_fails() {
    let defs = flagged(AbilityType::DeathCoil);
    let err = audit(&defs, &and((AbilityType::DeathCoil, coil_scene))).unwrap_err();
    assert_eq!(
        err,
        "DeathCoil: not granted, warlock's AI panics (it asks the book for an ability it lacks)"
    );
}

/// A scene in which the granted ability is never decided fails the granted
/// half.
#[test]
fn a_scene_that_never_reaches_the_rule_fails() {
    let defs = flagged(AbilityType::DeathCoil);
    let err = audit(&defs, &and((AbilityType::DeathCoil, empty_scene))).unwrap_err();
    assert!(
        err.contains("DeathCoil: granted, warlock's AI never decides it"),
        "{err}"
    );
}

/// A rule that decides the ability without the grant fails the ungranted
/// half: here the scene's own unit already holds it.
#[test]
fn an_ai_that_decides_an_ungranted_ability_fails() {
    fn holding(defs: &AbilityDefinitions) -> (Scene, Who) {
        let (scene, who) = coil_scene(defs);
        (scene.grant(who, AbilityType::DeathCoil), who)
    }
    let defs = flagged(AbilityType::DeathCoil);
    let err = audit(&defs, &and((AbilityType::DeathCoil, holding))).unwrap_err();
    assert_eq!(err, "DeathCoil: not granted, warlock's AI still decides it");
}
