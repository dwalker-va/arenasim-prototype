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
//!
//! The same rows hold every talent ability to its PRICE (AS-480): granted, the
//! class AI chooses it alone in its scene, and the caster pays exactly the
//! resource it declares (`mana_cost`: mana, energy or rage) — nothing under a
//! next-cast modifier that makes it free, such as Inner Focus — and starts the
//! global cooldown exactly when it is not declared `off_gcd`. A class AI
//! charges and sets the GCD by hand at each cast site, so a new grant whose
//! `current_mana -= cost` or `global_cooldown = GCD` line is missing (or
//! extra) fails here. An ability that begins a cast (a cast bar, or an instant
//! routed through a zero-length one) pays when it LANDS, so the decision must
//! charge nothing and the cast, landed through the real `process_casting`,
//! exactly the price.

use std::collections::BTreeSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::MinimalPlugins;

use arenasim::combat::log::CombatLog;
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::{
    AbilitiesConfig, AbilityConfig, AbilityDefinitions,
};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene, SceneCast, Who};
use arenasim::states::play_match::combat_core::process_casting;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraPending, AuraType, CastingState, Combatant, GameRng,
    MatchCountdown,
};
use arenasim::states::play_match::constants::GCD;
use arenasim::states::play_match::map_config::ActiveMapGeometry;
use arenasim::states::play_match::movement_config::load_movement_config;
use arenasim::states::play_match::next_cast;

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
    (AbilityType::Shadowburn, warlock_target_low),
    (AbilityType::Conflagrate, warlock_immolate_up),
    // Expose Armor's moment: below the Kidney Shot planner, on the kill
    // target.
    (AbilityType::Rupture, expose_armor_scene),
    (AbilityType::DivineFavor, paladin_ally_under_attack),
    (AbilityType::AuraMastery, paladin_ally_casting_by_a_rogue),
    (AbilityType::HolyNova, priest_threatened),
    (AbilityType::GuardianSpirit, priest_ally_endangered),
    (AbilityType::Repentance, paladin_enemy_healer_left_alone),
    (AbilityType::CrusaderStrike, paladin_in_melee),
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

/// [`warlock_dots_up`] with the Warrior 15 yards off and below the line the
/// team calls an enemy low: Shadowburn's finisher.
fn warlock_target_low(defs: &AbilityDefinitions) -> (Scene, Who) {
    let (scene, who) = warlock_dots_up(defs);
    (scene.place(WARRIOR, 15.0, 0.0).health_of(WARRIOR, 0.2), who)
}

/// [`warlock_dots_up`] with the Warlock's curse and its own Immolate on the
/// Warrior too: Conflagrate's moment.
fn warlock_immolate_up(defs: &AbilityDefinitions) -> (Scene, Who) {
    let (mut scene, who) = warlock_dots_up(defs);
    let me = scene.entity(WARLOCK);
    for name in ["Curse of Agony", "Immolate"] {
        scene = scene.aura_full(
            WARRIOR,
            Aura {
                effect_type: AuraType::DamageOverTime,
                duration: 15.0,
                magnitude: 4.0,
                tick_interval: 3.0,
                ability_name: name.into(),
                caster: Some(me),
                break_on_damage_threshold: -1.0,
                ..Default::default()
            },
        );
    }
    (scene, who)
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

/// A healthy Paladin and Mage on an enemy Warrior, its Priest 15 yards off
/// and left alone: Repentance's moment.
fn paladin_enemy_healer_left_alone(defs: &AbilityDefinitions) -> (Scene, Who) {
    let scene = Scene::with_config(defs.clone(), load_movement_config().unwrap())
        .unit(PALADIN, 1, CharacterClass::Paladin)
        .aura_full(PALADIN, devotion())
        .targeting(WARRIOR)
        .unit(MAGE, 1, CharacterClass::Mage)
        .at(-10.0, 0.0)
        .targeting(WARRIOR)
        .unit(WARRIOR, 2, CharacterClass::Warrior)
        .at(2.0, 0.0)
        .targeting(PALADIN)
        .unit("enemy priest", 2, CharacterClass::Priest)
        .at(15.0, 0.0);
    (scene, PALADIN)
}

/// A healthy Paladin in melee of its target, a Warrior, its Hammer of
/// Justice spent: Crusader Strike's moment.
fn paladin_in_melee(defs: &AbilityDefinitions) -> (Scene, Who) {
    let scene = Scene::with_config(defs.clone(), load_movement_config().unwrap())
        .unit(PALADIN, 1, CharacterClass::Paladin)
        .aura_full(PALADIN, devotion())
        .targeting(WARRIOR)
        .cooldown(PALADIN, AbilityType::HammerOfJustice, 30.0)
        .unit(WARRIOR, 2, CharacterClass::Warrior)
        .at(2.0, 0.0)
        .targeting(PALADIN);
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

/// A Priest at 60% with an enemy Warrior beside it, Pummel ready and the
/// rage for it: the Flash Heal it owes itself would be interrupted, so Holy
/// Nova's moment. Psychic Scream cooling down and Weakened Soul on it, so
/// neither the scream nor a shield goes out first.
fn priest_threatened(defs: &AbilityDefinitions) -> (Scene, Who) {
    let scene = Scene::with_config(defs.clone(), load_movement_config().unwrap())
        .unit(PRIEST, 1, CharacterClass::Priest)
        .health(0.6)
        .aura_full(PRIEST, fortitude())
        .aura(PRIEST, AuraType::WeakenedSoul, 10.0)
        .cooldown(PRIEST, AbilityType::PsychicScream, 20.0)
        .unit(WARRIOR, 2, CharacterClass::Warrior)
        .at(1.5, 0.0)
        .rage(30.0)
        .targeting(PRIEST);
    (scene, PRIEST)
}

/// [`priest_ally_dying`] with the Warrior teammate below 25%: Guardian
/// Spirit's moment, whatever is on it.
fn priest_ally_endangered(defs: &AbilityDefinitions) -> (Scene, Who) {
    let (scene, who) = priest_ally_dying(defs);
    (scene.health_of("warrior", 0.2), who)
}

fn considered(view: &DecisionView, ability: AbilityType) -> bool {
    view.candidates.iter().any(|c| c.ability == ability)
}

/// The rows name exactly the table's talent abilities.
fn coverage(defs: &AbilityDefinitions, rows: &[(AbilityType, SceneFor)]) -> Result<(), String> {
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
    Ok(())
}

/// The audit, over a table and its rows.
fn audit(defs: &AbilityDefinitions, rows: &[(AbilityType, SceneFor)]) -> Result<(), String> {
    coverage(defs, rows)?;
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
// The charge audit (AS-480): what each talent ability costs its caster.
// ---------------------------------------------------------------------------

/// The price `ability` declares for `who` in `scene`: its `mana_cost` from the
/// caster's granted book, changed by the next-cast modifier the caster holds
/// for it (Inner Focus makes it free), and whether it starts the GCD.
fn declared(scene: &Scene, who: Who, ability: AbilityType) -> (f32, bool) {
    let def = scene.combatant(who).book().get_unchecked(&ability);
    let auras = ActiveAuras {
        auras: scene.auras_of(who).to_vec(),
    };
    let cost = next_cast::primed(Some(&auras), ability)
        .map_or(def.mana_cost, |m| next_cast::changed(def, &m).mana_cost);
    (cost, !def.off_gcd)
}

/// Land `cast`, the cast `who`'s decision began, through the real
/// `process_casting`: the caster as its decision left it (`after`), the
/// target as the scene has it, the cast completing on the first frame.
/// Returns what the landing charged.
fn land(scene: &Scene, who: Who, after: &Combatant, cast: &SceneCast) -> f32 {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .insert_resource(MatchCountdown {
            time_remaining: 0.0,
            gates_opened: true,
        })
        .insert_resource(CombatLog::default())
        .insert_resource(GameRng::from_seed(0))
        .insert_resource(AbilityDefinitions::default())
        .insert_resource(ArenaDampening::default())
        .insert_resource(ActiveMapGeometry {
            bounds: Default::default(),
            volumes: Vec::new(),
            cover_anchors: Vec::new(),
        })
        .add_systems(Update, process_casting);
    let caster = app
        .world_mut()
        .spawn((
            Transform::from_translation(scene.position(who)),
            after.clone(),
            ActiveAuras {
                auras: scene.auras_of(who).to_vec(),
            },
        ))
        .id();
    let target = match cast.target {
        Some(whom) if whom != who => app
            .world_mut()
            .spawn((
                Transform::from_translation(scene.position(whom)),
                scene.combatant(whom).clone(),
                ActiveAuras {
                    auras: scene.auras_of(whom).to_vec(),
                },
            ))
            .id(),
        _ => caster,
    };
    let mut casting = CastingState::new(cast.ability, target, 0.0);
    casting.time_remaining = 0.001;
    app.world_mut().entity_mut(caster).insert(casting);
    let before = app.world().get::<Combatant>(caster).unwrap().current_mana;
    for _ in 0..5 {
        app.update();
    }
    assert!(
        app.world().get::<CastingState>(caster).is_none(),
        "{who}'s {:?} did not resolve",
        cast.ability
    );
    before - app.world().get::<Combatant>(caster).unwrap().current_mana
}

/// The difference between what `who` paid for `ability` in `scene` (granted)
/// and what it declares, if any.
fn charge_problem(scene: Scene, who: Who, ability: AbilityType) -> Option<String> {
    let scene = scene.grant(who, ability);
    let (resource, gcd) = {
        let me = scene.combatant(who);
        (me.current_mana, me.global_cooldown)
    };
    let (cost, on_gcd) = declared(&scene, who, ability);
    let (view, after) = match catch_unwind(AssertUnwindSafe(|| scene.decide_keeping(who))) {
        Ok(decided) => decided,
        Err(_) => return Some(format!("{ability:?}: granted, {who}'s AI panics")),
    };
    let chose: Vec<AbilityType> = view.choices.iter().map(|(a, _)| *a).collect();
    if chose != [ability] {
        return Some(format!(
            "{ability:?}: not chosen alone in its scene (chose {chose:?}), so its price \
             cannot be read: give its row a scene where {who} casts it and nothing else"
        ));
    }
    let mut problems = Vec::new();
    let at_decision = resource - view.resource_after;
    match view.cast_started.filter(|c| c.ability == ability) {
        Some(cast) => {
            if at_decision != 0.0 {
                problems.push(format!(
                    "charged {at_decision} when the cast began (it pays when it lands)"
                ));
            }
            let at_landing = land(&scene, who, &after, &cast);
            if at_landing != cost {
                problems.push(format!("paid {at_landing} when it landed, declares {cost}"));
            }
        }
        None => {
            if at_decision != cost {
                problems.push(format!("paid {at_decision}, declares {cost}"));
            }
        }
    }
    let expected_gcd = if on_gcd { GCD } else { gcd };
    if view.global_cooldown_after != expected_gcd {
        problems.push(format!(
            "left the global cooldown at {}, declares {} (off_gcd: {})",
            view.global_cooldown_after, expected_gcd, !on_gcd
        ));
    }
    (!problems.is_empty()).then(|| format!("{ability:?}: {}", problems.join("; ")))
}

/// A free next-cast modifier on `who`: Inner Focus's own, as the ability
/// applies it.
fn inner_focus_up(defs: &AbilityDefinitions, scene: Scene, who: Who) -> Scene {
    let me = scene.entity(who);
    let pending = AuraPending::from_ability(me, me, defs.get_unchecked(&AbilityType::InnerFocus))
        .expect("Inner Focus applies its modifier");
    scene.aura_full(who, pending.aura)
}

/// The charge audit, over a table and its rows: each row in its scene, and
/// again under a free next-cast modifier when it has a price to waive.
fn charge_audit(defs: &AbilityDefinitions, rows: &[(AbilityType, SceneFor)]) -> Result<(), String> {
    coverage(defs, rows)?;
    let mut problems = Vec::new();
    for (ability, scene_for) in rows {
        let (scene, who) = scene_for(defs);
        problems.extend(charge_problem(scene, who, *ability));
        if defs.get_unchecked(ability).mana_cost > 0.0 {
            let (scene, who) = scene_for(defs);
            problems.extend(
                charge_problem(inner_focus_up(defs, scene, who), who, *ability)
                    .map(|p| format!("under Inner Focus, {p}")),
            );
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

#[test]
fn every_talent_ability_pays_its_declared_price() {
    let defs = AbilityDefinitions::shipped();
    if let Err(e) = charge_audit(defs, SCENES) {
        panic!("{e}");
    }
}

// ---------------------------------------------------------------------------
// The audit's own checks, on a fixture table that makes Death Coil a talent.
// The shipped Warlock AI decides Death Coil, but asks its book for it with
// `get_unchecked`: granted it passes, ungranted it panics.
// ---------------------------------------------------------------------------

fn flagged(ability: AbilityType) -> AbilityDefinitions {
    fixture(ability, |c| c.talent = true)
}

/// The shipped table with `edit` made to `ability`'s entry.
fn fixture(ability: AbilityType, edit: impl FnOnce(&mut AbilityConfig)) -> AbilityDefinitions {
    let text = std::fs::read_to_string(arenasim::paths::asset_path("config/abilities.ron"))
        .expect("abilities.ron reads");
    let mut config: AbilitiesConfig = ron::from_str(&text).expect("abilities.ron parses");
    edit(config.abilities.get_mut(&ability).unwrap());
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

// ---------------------------------------------------------------------------
// The charge audit's own checks.
// ---------------------------------------------------------------------------

#[test]
fn a_talent_ability_with_no_row_fails_the_charge_audit() {
    let defs = flagged(AbilityType::DeathCoil);
    let err = charge_audit(&defs, SCENES).unwrap_err();
    assert!(err.contains("with no row: [DeathCoil]"), "{err}");
}

/// Death Coil, made a talent, pays its mana and starts the global
/// cooldown, as declared: the audit passes it. Declared off the global
/// cooldown, the GCD its cast site starts is one it does not declare.
#[test]
fn a_talent_ability_that_starts_an_undeclared_gcd_fails() {
    let rows = and((AbilityType::DeathCoil, coil_scene));
    let defs = flagged(AbilityType::DeathCoil);
    assert_eq!(charge_audit(&defs, &rows), Ok(()));
    let defs = fixture(AbilityType::DeathCoil, |c| {
        c.talent = true;
        c.off_gcd = true;
    });
    assert_eq!(
        charge_audit(&defs, &rows).unwrap_err(),
        "DeathCoil: left the global cooldown at 1.5, declares 0 (off_gcd: true)\n\
         under Inner Focus, DeathCoil: left the global cooldown at 1.5, declares 0 \
         (off_gcd: true)"
    );
}

/// Inner Focus declared ON the global cooldown: its cast site starts none.
#[test]
fn a_talent_ability_that_skips_a_declared_gcd_fails() {
    let defs = fixture(AbilityType::InnerFocus, |c| c.off_gcd = false);
    assert_eq!(
        charge_audit(&defs, SCENES).unwrap_err(),
        "InnerFocus: left the global cooldown at 0, declares 1.5 (off_gcd: false)"
    );
}

/// A scene in which the granted ability is not chosen alone cannot price it,
/// and the audit says so rather than passing it.
#[test]
fn a_scene_that_does_not_choose_the_ability_alone_fails() {
    let defs = flagged(AbilityType::DeathCoil);
    let err = charge_audit(&defs, &and((AbilityType::DeathCoil, empty_scene))).unwrap_err();
    assert!(
        err.starts_with("DeathCoil: not chosen alone in its scene (chose [])"),
        "{err}"
    );
}

/// The audit reads both payment paths, and the free one: Shadowburn begins
/// a zero-length cast (it pays when it lands) and Power Infusion pays when
/// chosen, and each is read again under Inner Focus. A change that moved
/// either to the other path, or dropped the free pass, would leave a path
/// unread.
#[test]
fn the_charge_audit_reads_both_payment_paths() {
    let defs = AbilityDefinitions::shipped();
    let row = |ability: AbilityType| {
        let (_, scene_for) = SCENES.iter().find(|(a, _)| *a == ability).unwrap();
        let (scene, who) = scene_for(defs);
        scene.grant(who, ability).decide(who).cast_started
    };
    assert_eq!(
        row(AbilityType::Shadowburn).map(|c| (c.ability, c.cast_time)),
        Some((AbilityType::Shadowburn, 0.0))
    );
    assert_eq!(row(AbilityType::PowerInfusion), None);
}
