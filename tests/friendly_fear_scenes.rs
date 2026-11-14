//! Scene tests for the friendly-hold rule (`class_ai::damage_breaks_friendly_hold`):
//! a Fear or a Psychic Scream from the deciding unit's own team is a lock the
//! team waits out, like a Polymorph, so no damage and no damage over time is
//! cast on its bearer. A root is not one: a root and a nuke go together.
//!
//! Each scene test runs the real class dispatch for one tick and is paired
//! with its control, the same scene without the hold; one staged match holds
//! the auto-attack, which lands outside the dispatch, to the same rule.
//! `tests/decision_properties.rs`'s `never_breaks_a_friendly_breakable_cc`
//! guards the rule over the whole grid.

mod common;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::match_config::CharacterClass::{Mage, Priest, Warlock, Warrior};
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::class_ai::damage_breaks_friendly_hold;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::{AbilityType, Aura, AuraType};
use common::staged::play;
use std::collections::BTreeSet;

const ME: &str = "me";
const ALLY: &str = "ally";
const ENEMY: &str = "enemy warrior";
const HEALER: &str = "enemy priest";

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

/// `ability`'s own aura as the engine lands it on `who`, cast by `caster`:
/// its type, duration and damage threshold straight from `abilities.ron`.
fn landed(scene: Scene, who: &'static str, caster: &'static str, ability: AbilityType) -> Scene {
    let defs = load_ability_definitions().expect("abilities.ron");
    let aura = defs
        .get(&ability)
        .and_then(|d| d.applies_aura.clone())
        .expect("the ability applies an aura");
    let caster = scene.entity(caster);
    scene.aura_full(
        who,
        Aura {
            effect_type: aura.aura_type,
            duration: aura.duration,
            magnitude: aura.magnitude,
            break_on_damage_threshold: aura.break_on_damage,
            caster: Some(caster),
            ability_name: defs.get(&ability).unwrap().name.clone(),
            ..Default::default()
        },
    )
}

fn assert_refused(view: &DecisionView, ability: AbilityType, why: &str) {
    assert!(
        matches!(
            view.rejected(ability),
            Some(RejectionReason::FriendlyBreakableCC)
        ),
        "{why}: {}",
        describe(view)
    );
}

// ---------------------------------------------------------------------------
// The set the rule covers, named.
// ---------------------------------------------------------------------------

/// The abilities whose landed aura the team waits out, by name: every one
/// that breaks on any damage, and every hold that breaks once enough damage
/// accumulates. The roots that break on damage (Frost Nova, Entangling
/// Roots, Spider Web) are not in it, nor is Death Coil's horror (it never
/// breaks).
#[test]
fn the_holds_the_team_waits_out_are_named() {
    let defs = load_ability_definitions().expect("abilities.ron");
    let waited: BTreeSet<AbilityType> = defs
        .iter()
        .filter(|(_, def)| {
            def.applies_aura.as_ref().is_some_and(|a| {
                damage_breaks_friendly_hold(&Aura {
                    effect_type: a.aura_type,
                    break_on_damage_threshold: a.break_on_damage,
                    ..Default::default()
                })
            })
        })
        .map(|(ability, _)| *ability)
        .collect();
    assert_eq!(
        waited,
        BTreeSet::from([
            AbilityType::Polymorph,
            AbilityType::Fear,
            AbilityType::PsychicScream,
            AbilityType::FreezingTrap,
            AbilityType::Repentance,
        ]),
    );
    // ...and the damage-breakable roots are deliberately outside it.
    for root in [
        AbilityType::FrostNova,
        AbilityType::EntanglingRoots,
        AbilityType::SpiderWeb,
    ] {
        let a = defs.get(&root).unwrap().applies_aura.as_ref().unwrap();
        assert_eq!(a.aura_type, AuraType::Root);
        assert!(a.break_on_damage > 0.0, "{root:?} breaks on damage");
    }
}

// ---------------------------------------------------------------------------
// Mortal Strike waits out a friendly Fear (the card's repro).
// ---------------------------------------------------------------------------

/// The Warrior in melee of its target, with the rage for Mortal Strike,
/// shouted already and its Rend on the target, so its turn goes to the
/// strike; its Warlock and Priest teammates stand back.
fn warrior_scene() -> Scene {
    let scene = Scene::new()
        .unit(ME, 1, Warrior)
        .rage(100.0)
        .aura(ME, AuraType::AttackPowerIncrease, 120.0)
        .targeting(ENEMY)
        .unit("warlock", 1, Warlock)
        .at(-20.0, 0.0)
        .aura("warlock", AuraType::AttackPowerIncrease, 120.0)
        .unit("priest", 1, Priest)
        .at(-20.0, 6.0)
        .aura("priest", AuraType::AttackPowerIncrease, 120.0)
        .unit(ENEMY, 2, Warrior)
        .at(1.5, 0.0)
        .targeting(ME);
    let rend = Aura {
        effect_type: AuraType::DamageOverTime,
        duration: 10.0,
        // A bleed never breaks on damage (an aura's default threshold, 0.0,
        // would read as the team's own breakable crowd control).
        break_on_damage_threshold: -1.0,
        caster: Some(scene.entity(ME)),
        ability_name: "Rend".to_string(),
        ..Default::default()
    };
    scene.aura_full(ENEMY, rend)
}

#[test]
fn mortal_strike_waits_out_its_teams_fear_and_psychic_scream() {
    let free = warrior_scene().decide(ME);
    assert_eq!(
        free.chose(),
        Some((AbilityType::MortalStrike, Some(ENEMY))),
        "the control strikes the free Warrior: {}",
        describe(&free)
    );

    for (caster, fear) in [
        ("warlock", AbilityType::Fear),
        ("priest", AbilityType::PsychicScream),
    ] {
        let feared = landed(warrior_scene(), ENEMY, caster, fear).decide(ME);
        assert_refused(
            &feared,
            AbilityType::MortalStrike,
            &format!("Mortal Strike waits out the team's {fear:?}"),
        );
        assert!(
            feared.chose().is_none_or(|(_, t)| t != Some(ENEMY)),
            "nothing is cast on the feared Warrior: {}",
            describe(&feared)
        );
    }
}

// ---------------------------------------------------------------------------
// The Warlock's damage over time waits out a friendly Fear on the healer.
// ---------------------------------------------------------------------------

/// The Warlock on the enemy healer, nothing of its own on it yet, with the
/// Priest and a Warrior teammate standing back.
fn warlock_scene() -> Scene {
    Scene::new()
        .unit(ME, 1, Warlock)
        .targeting(HEALER)
        .unit("priest", 1, Priest)
        .at(-10.0, 0.0)
        .unit(ALLY, 1, Warrior)
        .at(-10.0, 6.0)
        .unit(HEALER, 2, Priest)
        .at(20.0, 0.0)
        .unit(ENEMY, 2, Warrior)
        .at(25.0, 10.0)
}

#[test]
fn the_warlocks_dots_wait_out_a_friendly_fear_on_the_healer() {
    let free = warlock_scene().decide(ME);
    let (opened, on) = free.chose().expect("the control casts");
    assert_eq!(
        on,
        Some(HEALER),
        "the control casts on the free healer: {}",
        describe(&free)
    );
    let dot_or_curse = [
        AbilityType::Corruption,
        AbilityType::CurseOfAgony,
        AbilityType::Immolate,
        AbilityType::UnstableAffliction,
    ];
    assert!(
        dot_or_curse.contains(&opened),
        "the control opens with damage over time: {}",
        describe(&free)
    );

    let feared = landed(
        warlock_scene(),
        HEALER,
        "priest",
        AbilityType::PsychicScream,
    )
    .decide(ME);
    assert_refused(&feared, opened, "the opener waits out the Scream");
    assert_refused(
        &feared,
        AbilityType::Corruption,
        "Corruption waits out the Scream",
    );
    assert!(
        feared.chose().is_none_or(|(_, t)| t != Some(HEALER)),
        "nothing is cast on the feared healer: {}",
        describe(&feared)
    );
}

// ---------------------------------------------------------------------------
// A root is no hold: the Mage still nukes a target its team has rooted.
// ---------------------------------------------------------------------------

/// The Mage with its armor and intellect up and Ice Barrier cooling down, so
/// its turn goes to its nuke, on an enemy Warrior out at range.
fn mage_scene() -> Scene {
    Scene::new()
        .unit(ME, 1, Mage)
        .aura(ME, AuraType::FrostArmorBuff, 600.0)
        .aura(ME, AuraType::MaxManaIncrease, 600.0)
        .cooldown(ME, AbilityType::IceBarrier, 20.0)
        .targeting(ENEMY)
        .unit("priest", 1, Priest)
        .at(-10.0, 0.0)
        .aura("priest", AuraType::MaxManaIncrease, 600.0)
        .unit(ENEMY, 2, Warrior)
        .at(25.0, 0.0)
        .targeting("priest")
}

#[test]
fn a_root_from_the_team_does_not_stop_a_nuke_but_a_fear_does() {
    let rooted = landed(mage_scene(), ENEMY, ME, AbilityType::FrostNova).decide(ME);
    assert_eq!(
        rooted.chose(),
        Some((AbilityType::Frostbolt, Some(ENEMY))),
        "a root and a nuke go together: {}",
        describe(&rooted)
    );

    let feared = landed(mage_scene(), ENEMY, "priest", AbilityType::PsychicScream).decide(ME);
    assert_refused(
        &feared,
        AbilityType::Frostbolt,
        "Frostbolt waits out the Scream",
    );
}

// ---------------------------------------------------------------------------
// The auto-attack waits out the team's breakable holds too.
// ---------------------------------------------------------------------------

/// How long the staged hold lasts.
const HOLD_SECS: f32 = 6.0;

/// `attacker`'s Auto Attacks that hit the enemy Priest before `until` on the
/// app clock (the combat log's stamps).
fn swings_landed_before(log: &str, attacker: &str, until: f32) -> usize {
    let hit = format!("[DMG] {attacker}'s Auto Attack hits Team 2 Priest #1");
    log.lines()
        .filter(|l| l.contains(&hit))
        .filter_map(|l| {
            l.trim_start_matches('[')
                .split('s')
                .next()?
                .trim()
                .parse::<f32>()
                .ok()
        })
        .filter(|t| *t < until)
        .count()
}

/// A hold of `effect` from `caster` on the enemy Priest, breaking as
/// `threshold` says.
#[derive(Clone, Copy)]
struct Hold {
    effect: AuraType,
    threshold: f32,
    caster: Unit,
    name: &'static str,
}

/// `team1` against an enemy Priest rooted in place at the origin, everyone
/// unkillable, `attacker` beside the Priest, and — with `hold` — the hold
/// landed on the Priest at the gates. The Priest's Psychic Scream and every
/// Fear and Death Coil on `team1` are held on cooldown, so no crowd control
/// but the staged one ever touches the swings. Returns how many of
/// `attacker_log`'s swings landed while the hold lasts.
fn swings_under(team1: &[&str], attacker: Unit, attacker_log: &str, hold: Option<Hold>) -> usize {
    use arenasim::states::play_match::components::{ActiveAuras, Combatant};
    let unkillable = |c: &mut Combatant| {
        c.max_health = 1.0e6;
        c.current_health = 1.0e6;
    };
    let priest = Unit::new(2, 0);
    let mut stage = Stage::new()
        .combatant(priest, unkillable)
        .place(priest, 0.0, 0.0)
        .place(attacker, 1.5, 0.0)
        .cooldown(priest, AbilityType::PsychicScream, 1000.0)
        .aura(
            priest,
            Aura {
                effect_type: AuraType::Root,
                duration: 30.0,
                break_on_damage_threshold: -1.0,
                ability_name: "Root".into(),
                ..Default::default()
            },
        );
    for slot in 0..team1.len() as u8 {
        let who = Unit::new(1, slot);
        stage = stage
            .combatant(who, unkillable)
            .target(who, priest)
            .cooldown(who, AbilityType::Fear, 1000.0)
            .cooldown(who, AbilityType::DeathCoil, 1000.0);
    }
    if let Some(hold) = hold {
        stage = stage.world(move |world| {
            let caster = hold.caster.entity(world);
            let priest = priest.entity(world);
            world
                .get_mut::<ActiveAuras>(priest)
                .expect("the Priest's auras")
                .auras
                .push(Aura {
                    effect_type: hold.effect,
                    duration: HOLD_SECS,
                    break_on_damage_threshold: hold.threshold,
                    caster: Some(caster),
                    ability_name: hold.name.into(),
                    ..Default::default()
                });
        });
    }
    let config = HeadlessMatchConfig {
        team1: team1.iter().map(|c| c.to_string()).collect(),
        team2: vec!["Priest".into()],
        random_seed: Some(6),
        max_duration_secs: 15.0,
        ..Default::default()
    };
    let played = play(config, stage);
    let window = played.landed().app + HOLD_SECS - 0.5;
    swings_landed_before(&played.log, attacker_log, window)
}

/// Each attacker's swings land on the free Priest, and none land while a
/// hold its own team placed lasts: a Fear (the attacker's own), a teammate's
/// Freezing Trap, and its owner's Fear for a pet.
#[test]
fn the_teams_breakable_holds_hold_off_the_auto_attack() {
    let warrior = Unit::new(1, 0);
    let cases: [(&[&str], Unit, &str, Hold); 3] = [
        (
            &["Warrior"],
            warrior,
            "Team 1 Warrior #1",
            Hold {
                effect: AuraType::Fear,
                threshold: 100.0,
                caster: warrior,
                name: "Fear",
            },
        ),
        (
            &["Warrior", "Hunter"],
            warrior,
            "Team 1 Warrior #1",
            Hold {
                effect: AuraType::Incapacitate,
                threshold: 0.0,
                caster: Unit::new(1, 1),
                name: "Freezing Trap",
            },
        ),
        (
            &["Warlock"],
            Unit::pet_of(1, 0),
            "Team 1 Felhunter #1",
            Hold {
                effect: AuraType::Fear,
                threshold: 100.0,
                caster: Unit::new(1, 0),
                name: "Fear",
            },
        ),
    ];
    for (team1, attacker, label, hold) in cases {
        let free = swings_under(team1, attacker, label, None);
        assert!(
            free > 0,
            "{label}: without the {}, its swings land in the window",
            hold.name
        );
        let held = swings_under(team1, attacker, label, Some(hold));
        assert_eq!(
            held, 0,
            "{label}: under its own side's {}, no swing lands",
            hold.name
        );
    }
}
