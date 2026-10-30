//! AS-332 — melee avoidance (dodge and parry) and the attack-speed stat.
//!
//! A defender's dodge and parry chances are rolled against every incoming
//! melee attack: an auto-attack swing (either hand, a Hunter's melee hand, a
//! pet's bite) and every melee ability (`AbilityType::is_melee_attack`).
//! Spells and ranged attacks are never avoided. An avoided attack deals
//! nothing and applies nothing; what it cost stays spent. The roll draws
//! from `game_rng` only when the defender has avoidance, so every match
//! without it plays exactly as before.
//!
//! Three tiers here:
//! - **scenes**, one per melee ability that lands at its decision site, each
//!   paired with its control (the same scene, no avoidance): a certain dodge
//!   or parry lands nothing, and the control lands the attack;
//! - **staged matches** for the swings and the share: a defender with a
//!   dodge chance avoids a share of a Warrior's swings and Mortal Strikes,
//!   its 0-avoidance control none; a defender that avoids everything takes no
//!   melee damage at all from a Rogue's two hands, a pet's bites or a
//!   Hunter's melee hand — while its ranged attacks still land;
//! - **the zero guarantee**: matches with no avoidance draw exactly as many
//!   `game_rng` values as they did before avoidance existed (counts recorded
//!   on main at a8a30775, the commit this card branched from).

mod common;

use std::collections::BTreeSet;

use arenasim::headless::{run_headless_match_with, HeadlessMatchConfig, Stage, Unit};
use arenasim::states::match_config::{CharacterClass, RogueOpener};
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::class_ai::scene::{Scene, SceneAvoided, Who};
use arenasim::states::play_match::combat_core::{effective_attack_interval, Avoidance};
use arenasim::states::play_match::components::{AuraType, Combatant};
use arenasim::states::play_match::{
    effective_melee_interval, effective_offhand_interval, AbilityType,
};
use common::staged::{play, Played};

const ATTACKER: Who = "attacker";
const DEFENDER: Who = "defender";

/// A certain dodge, a certain parry, and no avoidance at all.
const DODGE: (f32, f32) = (1.0, 0.0);
const PARRY: (f32, f32) = (0.0, 1.0);
const NONE: (f32, f32) = (0.0, 0.0);

/// `attacker_class` in melee of an enemy Priest with `avoidance`, its target.
fn melee_scene(attacker_class: CharacterClass, avoidance: (f32, f32)) -> Scene {
    let scene = Scene::new()
        .unit(ATTACKER, 1, attacker_class)
        .targeting(DEFENDER);
    let scene = if attacker_class == CharacterClass::Warrior {
        // Shouted already, so its turn goes to the strike.
        scene
            .rage(100.0)
            .aura(ATTACKER, AuraType::AttackPowerIncrease, 120.0)
    } else {
        scene.mana(1.0)
    };
    scene
        .unit(DEFENDER, 2, CharacterClass::Priest)
        .at(1.5, 0.0)
        .avoidance(avoidance.0, avoidance.1)
}

fn avoided(attack: &str, avoidance: Avoidance) -> SceneAvoided {
    SceneAvoided {
        attacker: ATTACKER,
        defender: DEFENDER,
        attack: attack.to_string(),
        avoidance,
    }
}

/// Run one melee ability's scene three ways — a certain dodge, a certain
/// parry, and the control with no avoidance — and hold each to its outcome:
/// the attack is chosen all three times (its cost spent), the avoided ones
/// land nothing and name the avoidance, and the control lands.
///
/// `lands` reads what a LANDED attack left in the view, so the control can
/// show it and the avoided runs can show its absence.
fn assert_avoidable(
    ability: AbilityType,
    name: &str,
    build: impl Fn((f32, f32)) -> Scene,
    lands: impl Fn(&arenasim::states::play_match::class_ai::scene::DecisionView) -> bool,
) {
    let control = build(NONE).decide(ATTACKER);
    assert_eq!(
        control.chose(),
        Some((ability, Some(DEFENDER))),
        "{name}: the control scene must choose it"
    );
    assert!(lands(&control), "{name}: the control lands it");
    assert!(
        control.avoided.is_empty(),
        "{name}: the control avoids nothing"
    );

    for (avoidance, kind) in [(DODGE, Avoidance::Dodge), (PARRY, Avoidance::Parry)] {
        let view = build(avoidance).decide(ATTACKER);
        assert_eq!(
            view.chose(),
            Some((ability, Some(DEFENDER))),
            "{name} {kind:?}: still chosen, and paid for"
        );
        assert_eq!(
            view.avoided,
            vec![avoided(name, kind)],
            "{name} {kind:?}: the avoidance is recorded"
        );
        assert!(
            !lands(&view),
            "{name} {kind:?}: an avoided attack lands nothing"
        );
    }
}

#[test]
fn mortal_strike_is_avoided_and_applies_nothing() {
    assert_avoidable(
        AbilityType::MortalStrike,
        "Mortal Strike",
        // Rended already, so Mortal Strike is next.
        |a| melee_scene(CharacterClass::Warrior, a).aura(DEFENDER, AuraType::DamageOverTime, 10.0),
        |v| {
            v.landed
                .iter()
                .any(|l| l.ability == AbilityType::MortalStrike)
                && v.auras_queued
                    .contains(&(DEFENDER, AuraType::HealingReduction))
        },
    );
}

/// A defender that cannot act cannot dodge (TBC): stunned, with a certain
/// dodge, it takes the Mortal Strike; the same defender un-stunned dodges it.
#[test]
fn a_stunned_defender_cannot_dodge() {
    let scene = |stunned: bool| {
        let scene = melee_scene(CharacterClass::Warrior, DODGE).aura(
            DEFENDER,
            AuraType::DamageOverTime,
            10.0,
        );
        if stunned {
            scene.aura(DEFENDER, AuraType::Stun, 4.0)
        } else {
            scene
        }
    };
    let stunned = scene(true).decide(ATTACKER);
    assert_eq!(
        stunned.chose(),
        Some((AbilityType::MortalStrike, Some(DEFENDER)))
    );
    assert!(
        stunned.avoided.is_empty(),
        "a stunned defender cannot dodge"
    );
    assert!(
        stunned
            .landed
            .iter()
            .any(|l| l.ability == AbilityType::MortalStrike),
        "the Mortal Strike lands on the stunned defender"
    );

    let free = scene(false).decide(ATTACKER);
    assert_eq!(
        free.chose(),
        Some((AbilityType::MortalStrike, Some(DEFENDER)))
    );
    assert_eq!(
        free.avoided,
        vec![avoided("Mortal Strike", Avoidance::Dodge)]
    );
    assert!(free.landed.is_empty());
}

/// The same rule on an auto-attack swing: a Warrior beats on a dodging
/// Warrior it has stunned in place. While the stun holds no swing is dodged;
/// with the stun lifted from the same match, the swings are dodged.
#[test]
fn a_stunned_defender_cannot_dodge_a_swing() {
    use arenasim::states::play_match::components::Aura;
    const STUN_SECS: f32 = 6.0;
    let stage = |stunned: bool| {
        let stage =
            duel(1.0, 0.0)
                .place(Unit::new(1, 0), 0.0, 0.0)
                .place(Unit::new(2, 0), 1.5, 0.0);
        if !stunned {
            return stage;
        }
        stage.aura(
            Unit::new(2, 0),
            Aura {
                effect_type: AuraType::Stun,
                duration: STUN_SECS,
                break_on_damage_threshold: -1.0,
                ability_name: "Stun".into(),
                ..Default::default()
            },
        )
    };
    let config = HeadlessMatchConfig {
        max_duration_secs: 12.0,
        ..warrior_v_warrior(6)
    };
    let free = play(config.clone(), stage(false));
    let window = free.landed().app + STUN_SECS - 0.5;
    assert!(
        swings_avoided_before(&free.log, window) > 0,
        "an un-stunned defender dodges the swings in the window"
    );
    let held = play(config, stage(true));
    assert_eq!(
        swings_avoided_before(&held.log, window),
        0,
        "no swing is dodged while the defender is stunned"
    );
    assert!(
        held.log
            .lines()
            .any(|l| l.contains("Warrior #1's Auto Attack hits")
                || l.contains("Warrior #1's Auto Attack CRITS")
                || l.contains("Warrior #1's Heroic Strike hits")),
        "the swings land on the stunned defender"
    );
}

#[test]
fn rend_is_avoided_and_applies_nothing() {
    assert_avoidable(
        AbilityType::Rend,
        "Rend",
        |a| {
            melee_scene(CharacterClass::Warrior, a).cooldown(
                ATTACKER,
                AbilityType::MortalStrike,
                5.0,
            )
        },
        |v| {
            v.auras_queued
                .contains(&(DEFENDER, AuraType::DamageOverTime))
        },
    );
}

/// A Rogue in melee of the defender: stealthed with `opener`, or out of
/// stealth (`None`).
fn rogue_scene(opener: Option<RogueOpener>, avoidance: (f32, f32)) -> Scene {
    let mut scene = melee_scene(CharacterClass::Rogue, avoidance);
    let rogue = scene.combatant_mut(ATTACKER);
    match opener {
        Some(opener) => {
            rogue.stealthed = true;
            rogue.rogue_opener = opener;
        }
        None => rogue.stealthed = false,
    }
    scene
}

#[test]
fn ambush_is_avoided_and_deals_nothing() {
    assert_avoidable(
        AbilityType::Ambush,
        "Ambush",
        |a| rogue_scene(Some(RogueOpener::Ambush), a),
        |v| v.landed.iter().any(|l| l.ability == AbilityType::Ambush),
    );
}

#[test]
fn cheap_shot_is_avoided_and_stuns_nobody() {
    assert_avoidable(
        AbilityType::CheapShot,
        "Cheap Shot",
        |a| rogue_scene(Some(RogueOpener::CheapShot), a),
        |v| v.auras_queued.contains(&(DEFENDER, AuraType::Stun)),
    );
}

#[test]
fn kidney_shot_is_avoided_and_stuns_nobody() {
    assert_avoidable(
        AbilityType::KidneyShot,
        "Kidney Shot",
        |a| rogue_scene(None, a),
        |v| v.auras_queued.contains(&(DEFENDER, AuraType::Stun)),
    );
}

#[test]
fn sinister_strike_is_avoided_and_deals_nothing() {
    assert_avoidable(
        AbilityType::SinisterStrike,
        "Sinister Strike",
        |a| rogue_scene(None, a).cooldown(ATTACKER, AbilityType::KidneyShot, 10.0),
        |v| {
            v.landed
                .iter()
                .any(|l| l.ability == AbilityType::SinisterStrike)
        },
    );
}

/// An interrupter of `class` in melee of the defender, a Mage mid-Frostbolt.
fn interrupt_scene(class: CharacterClass, avoidance: (f32, f32)) -> Scene {
    let scene = Scene::new().unit(ATTACKER, 1, class).targeting(DEFENDER);
    let mut scene = if class == CharacterClass::Warrior {
        scene.rage(100.0)
    } else {
        scene.mana(1.0)
    }
    .unit(DEFENDER, 2, CharacterClass::Mage)
    .at(1.5, 0.0)
    .avoidance(avoidance.0, avoidance.1)
    .casting(DEFENDER, AbilityType::Frostbolt);
    scene.combatant_mut(ATTACKER).stealthed = false;
    scene
}

/// Pummel and Kick are melee attacks: a dodged or parried one interrupts
/// nothing, though its cooldown is spent. Wind Shear is a spell, and a
/// dodging target is interrupted by it all the same.
#[test]
fn a_melee_interrupt_is_avoided_and_a_spell_interrupt_is_not() {
    for (class, ability, name) in [
        (CharacterClass::Warrior, AbilityType::Pummel, "Pummel"),
        (CharacterClass::Rogue, AbilityType::Kick, "Kick"),
    ] {
        let control = interrupt_scene(class, NONE).interrupt(ATTACKER);
        assert_eq!(control.chose(), Some((ability, Some(DEFENDER))), "{name}");
        assert_eq!(
            control.interrupts_queued,
            vec![(ability, DEFENDER)],
            "{name}"
        );
        assert!(control.avoided.is_empty(), "{name}");
        for (avoidance, kind) in [(DODGE, Avoidance::Dodge), (PARRY, Avoidance::Parry)] {
            let view = interrupt_scene(class, avoidance).interrupt(ATTACKER);
            assert_eq!(
                view.chose(),
                Some((ability, Some(DEFENDER))),
                "{name} {kind:?}"
            );
            assert_eq!(view.avoided, vec![avoided(name, kind)], "{name} {kind:?}");
            assert!(
                view.interrupts_queued.is_empty(),
                "{name} {kind:?}: an avoided interrupt interrupts nothing"
            );
        }
    }

    let shear = interrupt_scene(CharacterClass::Shaman, DODGE).interrupt(ATTACKER);
    assert_eq!(
        shear.chose(),
        Some((AbilityType::WindShear, Some(DEFENDER)))
    );
    assert_eq!(
        shear.interrupts_queued,
        vec![(AbilityType::WindShear, DEFENDER)],
        "Wind Shear is a spell: a dodging target is interrupted"
    );
    assert!(shear.avoided.is_empty());
}

// ---------------------------------------------------------------------------
// Staged matches: the swings, and the share.
// ---------------------------------------------------------------------------

/// The defender's health in the staged matches: enough that it never dies,
/// so the match runs to its cap and every arm sees the same span of swings.
const UNKILLABLE: f32 = 1_000_000.0;

/// `who` with `dodge` and `parry`, and too much health to die.
fn defender(who: Unit, dodge: f32, parry: f32) -> Stage {
    Stage::new().combatant(who, move |c: &mut Combatant| {
        c.dodge_chance = dodge;
        c.parry_chance = parry;
        c.max_health = UNKILLABLE;
        c.current_health = UNKILLABLE;
    })
}

/// One `[AVOID]` line of the combat log, parsed: the defender, the verb, the
/// attacker and the attack.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct AvoidLine {
    defender: String,
    verb: String,
    attacker: String,
    attack: String,
}

fn avoid_lines(log: &str) -> Vec<AvoidLine> {
    log.lines()
        .filter_map(|line| line.split_once("[AVOID] ").map(|(_, rest)| rest))
        .map(|rest| {
            let (defender, rest) = rest
                .split_once(" dodges ")
                .map(|(d, r)| (d, ("dodges", r)))
                .or_else(|| {
                    rest.split_once(" parries ")
                        .map(|(d, r)| (d, ("parries", r)))
                })
                .expect("an [AVOID] line names a dodge or a parry");
            let (verb, rest) = rest;
            let (attacker, attack) = rest.split_once("'s ").expect("attacker's attack");
            AvoidLine {
                defender: defender.to_string(),
                verb: verb.to_string(),
                attacker: attacker.to_string(),
                attack: attack.to_string(),
            }
        })
        .collect()
}

/// How many times `attacker`'s `attack` landed (hit or crit) on anyone.
fn landed(log: &str, attacker: &str, attack: &str) -> usize {
    let hits = format!("{attacker}'s {attack} hits ");
    let crits = format!("{attacker}'s {attack} CRITS ");
    log.lines()
        .filter(|l| l.contains(&hits) || l.contains(&crits))
        .count()
}

/// The `melee_avoided` events of the trace.
fn traced_avoidances(played: &Played) -> Vec<&serde_json::Value> {
    played
        .trace
        .iter()
        .filter(|e| e["kind"] == "melee_avoided")
        .collect()
}

const WARRIOR_ID: &str = "Team 1 Warrior #1";
const DEFENDER_WARRIOR_ID: &str = "Team 2 Warrior #1";

/// Two Warriors, toe to toe for 90s: a defender that wants melee as much as
/// its attacker, so the swings keep coming.
fn warrior_v_warrior(seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Warrior".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(seed),
        max_duration_secs: 90.0,
        ..Default::default()
    }
}

/// [`defender`], with an attacker that cannot die either, so the match runs
/// its whole span.
fn duel(dodge: f32, parry: f32) -> Stage {
    defender(Unit::new(2, 0), dodge, parry).combatant(Unit::new(1, 0), |c: &mut Combatant| {
        c.max_health = UNKILLABLE;
        c.current_health = UNKILLABLE;
    })
}

/// A Warrior on a Warrior with a 30% chance to dodge (or to parry): the
/// defender avoids a share of the Warrior's swings and Mortal Strikes — about 30%,
/// within four standard deviations of it — and its control, with neither,
/// avoids none. Every avoidance is logged once and traced once, as the
/// defender's dodge or parry.
#[test]
fn a_defender_avoids_a_share_of_swings_and_mortal_strikes() {
    const CHANCE: f32 = 0.3;
    let control = play(warrior_v_warrior(1), duel(0.0, 0.0));
    assert!(
        avoid_lines(&control.log).is_empty(),
        "the control avoids nothing"
    );
    assert!(traced_avoidances(&control).is_empty());
    assert!(landed(&control.log, WARRIOR_ID, "Auto Attack") > 0);
    assert!(landed(&control.log, WARRIOR_ID, "Mortal Strike") > 0);

    for (dodge, parry, verb) in [(CHANCE, 0.0, "dodges"), (0.0, CHANCE, "parries")] {
        let played = play(warrior_v_warrior(1), duel(dodge, parry));
        let lines = avoid_lines(&played.log);
        for line in &lines {
            assert_eq!(line.defender, DEFENDER_WARRIOR_ID);
            assert_eq!(line.attacker, WARRIOR_ID);
            assert_eq!(line.verb, verb, "only the chance it has: {line:?}");
        }
        let attacks: BTreeSet<&str> = lines.iter().map(|l| l.attack.as_str()).collect();
        assert!(
            attacks.contains("Auto Attack") && attacks.contains("Mortal Strike"),
            "{verb}: both a swing and a Mortal Strike are avoided ({attacks:?})"
        );
        // Every attack of the Warrior's that was rolled: landed or avoided.
        let rolled_names = ["Auto Attack", "Heroic Strike", "Mortal Strike", "Rend"];
        let landed_n: usize = rolled_names
            .iter()
            .map(|a| landed(&played.log, WARRIOR_ID, a))
            .sum::<usize>()
            + played
                .log
                .lines()
                .filter(|l| l.contains("applies Rend"))
                .count();
        let avoided_n = lines.len();
        let n = (landed_n + avoided_n) as f64;
        assert!(n >= 40.0, "{verb}: enough attacks to measure a share ({n})");
        let share = avoided_n as f64 / n;
        let sd = (CHANCE as f64 * (1.0 - CHANCE as f64) / n).sqrt();
        assert!(
            (share - CHANCE as f64).abs() < 4.0 * sd,
            "{verb}: {avoided_n} of {n} attacks avoided ({share:.3}), expected {CHANCE} +/- 4 x {sd:.3}"
        );

        // One trace event per logged avoidance, naming the same attacks.
        let traced = traced_avoidances(&played);
        assert_eq!(traced.len(), lines.len(), "{verb}: one trace event each");
        let expected = if dodge > 0.0 { "dodge" } else { "parry" };
        let mut traced_attacks: Vec<String> = traced
            .iter()
            .map(|e| {
                assert_eq!(e["avoided"], expected);
                e["attack"].as_str().expect("attack").to_string()
            })
            .collect();
        let mut logged_attacks: Vec<String> = lines.iter().map(|l| l.attack.clone()).collect();
        traced_attacks.sort();
        logged_attacks.sort();
        assert_eq!(traced_attacks, logged_attacks);
    }
}

/// The auto-attack sources whose swings an `attack` names: a log id and the
/// attack. The Rogue swings two hands, both named "Auto Attack".
fn auto_attack_sources(log: &str, defender_id: &str) -> BTreeSet<String> {
    log.lines()
        .filter(|l| {
            l.contains("'s Auto Attack ")
                && (l.contains(&format!("hits {defender_id} "))
                    || l.contains(&format!("CRITS {defender_id} ")))
        })
        .filter_map(|l| l.split("[DMG] ").nth(1))
        .filter_map(|l| l.split_once("'s Auto Attack").map(|(a, _)| a.to_string()))
        .collect()
}

fn rogue_hunter_v_warrior(seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Rogue".into(), "Hunter".into()],
        team2: vec!["Warrior".into()],
        // The Warrior chases the Hunter, which brings it into the Hunter's
        // melee reach as well as its Spider's.
        team2_kill_target: Some(1),
        random_seed: Some(seed),
        max_duration_secs: 60.0,
        ..Default::default()
    }
}

/// A defender that avoids every melee attack takes no melee damage at all:
/// not from a Rogue's main or off hand, a Spider's bite, or a Hunter's melee
/// hand, and not from a melee ability — while the Hunter's ranged attacks
/// still land. The control shows each of those swings landing without
/// avoidance, so every source is one the dodging arm actually met.
#[test]
fn a_defender_that_avoids_everything_takes_no_melee_damage() {
    const DEFENDER_ID: &str = "Team 2 Warrior #1";
    let control = play(
        rogue_hunter_v_warrior(2),
        defender(Unit::new(2, 0), 0.0, 0.0),
    );
    let sources = auto_attack_sources(&control.log, DEFENDER_ID);
    let expected: BTreeSet<String> = ["Team 1 Rogue #1", "Team 1 Hunter #2", "Team 1 Spider #2"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(sources, expected, "the control: every swing source lands");

    let played = play(
        rogue_hunter_v_warrior(2),
        defender(Unit::new(2, 0), 1.0, 0.0),
    );
    assert!(
        auto_attack_sources(&played.log, DEFENDER_ID).is_empty(),
        "no swing lands on a defender that dodges everything"
    );
    let melee_landed = played
        .log
        .lines()
        .filter(|l| {
            l.contains(&format!(" {DEFENDER_ID} for "))
                && [
                    "Sinister Strike",
                    "Ambush",
                    "Kidney Shot",
                    "Cheap Shot",
                    "Heroic Strike",
                ]
                .iter()
                .any(|a| l.contains(&format!("'s {a} ")))
        })
        .count();
    assert_eq!(melee_landed, 0, "no melee ability lands either");
    let dodged_swings: BTreeSet<String> = avoid_lines(&played.log)
        .into_iter()
        .filter(|l| l.attack == "Auto Attack")
        .map(|l| l.attacker)
        .collect();
    assert_eq!(dodged_swings, expected, "every swing source was dodged");
    assert!(
        landed(&played.log, "Team 1 Hunter #2", "Auto Shot") > 0,
        "a ranged attack is never avoided"
    );
}

/// The avoided SWINGS (auto-attacks, Heroic Strike included) stamped before
/// `until` on the log's clock.
fn swings_avoided_before(log: &str, until: f32) -> usize {
    log.lines()
        .filter(|l| {
            l.contains("[AVOID] ")
                && (l.ends_with("'s Auto Attack") || l.ends_with("'s Heroic Strike"))
        })
        .filter_map(|l| {
            let stamp = l.trim_start().strip_prefix('[')?.split('s').next()?;
            stamp.trim().parse::<f32>().ok()
        })
        .filter(|t| *t < until)
        .count()
}

/// A swing the attacker's team would break its own Fear with is dropped
/// unlanded, so it is not avoided either: no "Dodge" for an attack that never
/// happens. The Priest stands rooted beside the Warrior, dodging everything;
/// with a Fear from the Warrior's own side on it, no swing is avoided while
/// the Fear holds, and without one the same Warrior's swings are dodged.
#[test]
fn a_swing_held_off_a_friendly_fear_is_not_avoided() {
    use arenasim::states::play_match::components::Aura;
    const FEAR_SECS: f32 = 8.0;
    let stage = |feared: bool| {
        let stage = defender(Unit::new(2, 0), 1.0, 0.0)
            .place(Unit::new(1, 0), 0.0, 0.0)
            .place(Unit::new(2, 0), 1.5, 0.0)
            .aura(
                Unit::new(2, 0),
                Aura {
                    effect_type: AuraType::Root,
                    duration: 30.0,
                    break_on_damage_threshold: -1.0,
                    ability_name: "Root".into(),
                    ..Default::default()
                },
            );
        if !feared {
            return stage;
        }
        stage.world(|world| {
            let warrior = Unit::new(1, 0).entity(world);
            let priest = Unit::new(2, 0).entity(world);
            let fear = Aura {
                effect_type: AuraType::Fear,
                duration: FEAR_SECS,
                break_on_damage_threshold: 100.0,
                caster: Some(warrior),
                ability_name: "Fear".into(),
                ..Default::default()
            };
            world
                .get_mut::<arenasim::states::play_match::components::ActiveAuras>(priest)
                .expect("the Priest's auras")
                .auras
                .push(fear);
        })
    };
    let config = HeadlessMatchConfig {
        max_duration_secs: 15.0,
        ..warrior_v_warrior(6)
    };
    let config = HeadlessMatchConfig {
        team2: vec!["Priest".into()],
        ..config
    };
    let open = play(config.clone(), stage(false));
    let window = open.landed().app + FEAR_SECS - 0.5;
    assert!(
        swings_avoided_before(&open.log, window) > 0,
        "without the Fear, the Warrior's swings are dodged in the window"
    );
    let feared = play(config, stage(true));
    assert_eq!(
        swings_avoided_before(&feared.log, window),
        0,
        "under its own side's Fear, no swing is rolled, so none is avoided"
    );
}

// ---------------------------------------------------------------------------
// The zero guarantee.
// ---------------------------------------------------------------------------

/// Matches with no avoidance draw exactly as many `game_rng` values as they
/// did before avoidance existed — recorded on main at a8a30775 (the counter
/// itself draws nothing). Melee-heavy on purpose: dual wield, pets, Heroic
/// Strike, a Hunter's melee hand, Kick and Pummel, on an open map and in the
/// bowl.
#[test]
fn a_match_without_avoidance_draws_what_it_drew_before() {
    for (team1, team2, map, profile, seed, recorded) in ZERO_AVOIDANCE_DRAWS {
        let config = HeadlessMatchConfig {
            team1: team1.iter().map(|s| s.to_string()).collect(),
            team2: team2.iter().map(|s| s.to_string()).collect(),
            map: map.to_string(),
            ai_profile: Some(profile.to_string()),
            random_seed: Some(*seed),
            ..Default::default()
        };
        let result = run_headless_match_with(config, true, None).expect("the match plays");
        assert_eq!(
            result.rng_draws, *recorded,
            "{team1:?} v {team2:?} on {map} ({profile}), seed {seed}"
        );
    }
}

type DrawPin = (
    &'static [&'static str],
    &'static [&'static str],
    &'static str,
    &'static str,
    u64,
    u64,
);

const ZERO_AVOIDANCE_DRAWS: &[DrawPin] = &[
    (
        &["Warrior", "Rogue"],
        &["Hunter", "Priest"],
        "BasicArena",
        "Legacy",
        11,
        113,
    ),
    (
        &["Rogue", "Warrior", "Paladin"],
        &["Warrior", "Hunter", "Shaman"],
        "PillaredArena",
        "TeamPlan",
        5,
        276,
    ),
    (&["Rogue"], &["Warrior"], "BasicArena", "Legacy", 3, 53),
];

// ---------------------------------------------------------------------------
// The attack-speed stat.
// ---------------------------------------------------------------------------

/// A +25% attack-speed bonus divides every swing interval — main hand, off
/// hand, a Hunter's melee hand — by 1.25, before any slow stretches it; with
/// no bonus the interval is the weapon's speed to the bit.
#[test]
fn an_attack_speed_bonus_shortens_the_swing_timer() {
    let mut c = Combatant::new(1, 0, CharacterClass::Rogue);
    c.weapon_speed = 2.5;
    c.offhand_weapon_speed = 1.5;
    c.offhand_damage = 10.0;
    c.melee_weapon_speed = 3.0;
    c.melee_damage = 10.0;

    assert_eq!(
        effective_attack_interval(&c, None).to_bits(),
        2.5f32.to_bits()
    );
    assert_eq!(
        effective_offhand_interval(&c, None).to_bits(),
        1.5f32.to_bits()
    );
    assert_eq!(
        effective_melee_interval(&c, None).to_bits(),
        3.0f32.to_bits()
    );

    c.attack_speed_bonus = 0.25;
    assert_eq!(effective_attack_interval(&c, None), 2.5 / 1.25);
    assert_eq!(effective_offhand_interval(&c, None), 1.5 / 1.25);
    assert_eq!(effective_melee_interval(&c, None), 3.0 / 1.25);

    // A slow still stretches the hastened interval.
    let slowed = arenasim::states::play_match::components::ActiveAuras {
        auras: vec![arenasim::states::play_match::components::Aura {
            effect_type: AuraType::AttackSpeedSlow,
            magnitude: 0.5,
            ..Default::default()
        }],
    };
    assert_eq!(
        effective_attack_interval(&c, Some(&slowed)),
        2.5 / 1.25 * (1.0 / (1.0 - 0.5))
    );
}

/// The interval is the swing timer: over the same staged span, a Warrior with
/// +50% attack speed swings about half again as often as the same Warrior
/// without it.
#[test]
fn an_attack_speed_bonus_swings_more_often_in_a_match() {
    let swings = |bonus: f32| {
        let stage = duel(0.0, 0.0).combatant(Unit::new(1, 0), move |c: &mut Combatant| {
            c.attack_speed_bonus = bonus
        });
        let played = play(warrior_v_warrior(4), stage);
        landed(&played.log, WARRIOR_ID, "Auto Attack")
            + landed(&played.log, WARRIOR_ID, "Heroic Strike")
    };
    let (plain, hasted) = (swings(0.0), swings(0.5));
    assert!(plain > 10, "the control swings ({plain})");
    let ratio = hasted as f64 / plain as f64;
    assert!(
        (1.3..1.7).contains(&ratio),
        "+50% attack speed: {hasted} swings against {plain} ({ratio:.2}x)"
    );
}

/// `is_melee_attack` agrees with the data it judges: every melee attack is a
/// physical attack (`prevention: Pacify`) used at melee range.
#[test]
fn every_melee_attack_is_a_pacify_ability_at_melee_range() {
    let defs = load_ability_definitions().expect("abilities.ron loads");
    let melee: BTreeSet<AbilityType> = defs
        .ability_types()
        .copied()
        .filter(|a| a.is_melee_attack())
        .collect();
    let expected: BTreeSet<AbilityType> = [
        AbilityType::HeroicStrike,
        AbilityType::MortalStrike,
        AbilityType::Rend,
        AbilityType::Pummel,
        AbilityType::Ambush,
        AbilityType::CheapShot,
        AbilityType::SinisterStrike,
        AbilityType::KidneyShot,
        AbilityType::Kick,
    ]
    .into_iter()
    .collect();
    assert_eq!(melee, expected);
    for ability in melee {
        let def = defs.get_unchecked(&ability);
        assert_eq!(
            format!("{:?}", def.prevention),
            "Pacify",
            "{ability:?} is a physical attack"
        );
        assert!(
            def.range <= arenasim::states::play_match::MELEE_RANGE,
            "{ability:?} is used at melee range"
        );
    }
}
