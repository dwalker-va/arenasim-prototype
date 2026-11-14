//! Scene tests for the friendly-hold rule (`class_ai::damage_breaks_friendly_hold`):
//! a Fear or a Psychic Scream from the deciding unit's own team is a lock the
//! team waits out, like a Polymorph, so no damage and no damage over time is
//! cast on its bearer. A root is not one: a root and a nuke go together.
//!
//! Each scene test runs the real class dispatch for one tick and is paired
//! with its control, the same scene without the hold; staged matches hold
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
/// unkillable, `attacker` beside the Priest, and `holds` landed on the Priest
/// at the gates, in order. The Priest's Psychic Scream and every
/// Fear and Death Coil on `team1` are held on cooldown, so no crowd control
/// but the staged one ever touches the swings. Returns how many of
/// `attacker_log`'s swings landed while the hold lasts.
fn swings_under(team1: &[&str], attacker: Unit, attacker_log: &str, holds: &[Hold]) -> usize {
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
    for &hold in holds {
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
        let free = swings_under(team1, attacker, label, &[]);
        assert!(
            free > 0,
            "{label}: without the {}, its swings land in the window",
            hold.name
        );
        let held = swings_under(team1, attacker, label, &[hold]);
        assert_eq!(
            held, 0,
            "{label}: under its own side's {}, no swing lands",
            hold.name
        );
    }
}

/// A hold the target's OWN side placed on it is no hold the attacker's team
/// waits out, and it does not hide one that team did place: under the
/// Priest's own Fear and then its Warrior's, the Warrior still swings none.
/// The control is the Priest's own Fear alone, which the Warrior swings
/// through.
#[test]
fn a_hold_from_the_targets_own_side_hides_none_of_the_teams() {
    let warrior = Unit::new(1, 0);
    let label = "Team 1 Warrior #1";
    let own = Hold {
        effect: AuraType::Fear,
        threshold: 100.0,
        caster: Unit::new(2, 0),
        name: "Fear",
    };
    let teams = Hold {
        caster: warrior,
        ..own
    };
    assert!(
        swings_under(&["Warrior"], warrior, label, &[own]) > 0,
        "under the Priest's own Fear alone, the Warrior's swings land"
    );
    assert_eq!(
        swings_under(&["Warrior"], warrior, label, &[own, teams]),
        0,
        "the Priest's own Fear does not hide its Warrior's"
    );
}

// ---------------------------------------------------------------------------
// A swing held off a friendly hold is never made, so it spends nothing.
// ---------------------------------------------------------------------------

/// The Heroic Strike bonus staged onto the Warrior's next swing.
const QUEUED_STRIKE: f32 = 40.0;

/// One frame at the sim's fixed step.
const FRAME: f32 = 1.0 / 60.0;

/// A Warrior and its Warlock against an enemy Priest rooted in place at the
/// origin, everyone unkillable, the Warrior beside the Priest with an empty
/// rage bar and a Heroic Strike queued, and — with `feared` — the Warlock's
/// Fear landed on the Priest at the gates for [`HOLD_SECS`]. Every other
/// crowd control on either side is held on cooldown, and nothing can strike
/// the Warrior while the Priest is feared, so the Warrior's swings are its
/// only source of rage.
fn warrior_beside_its_teams_fear(feared: bool) -> common::staged::Played {
    use arenasim::states::play_match::components::{ActiveAuras, Combatant};
    let unkillable = |c: &mut Combatant| {
        c.max_health = 1.0e6;
        c.current_health = 1.0e6;
    };
    let warrior = Unit::new(1, 0);
    let warlock = Unit::new(1, 1);
    let priest = Unit::new(2, 0);
    let mut stage = Stage::new()
        .combatant(priest, unkillable)
        .place(priest, 0.0, 0.0)
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
        )
        .combatant(warrior, move |c| {
            unkillable(c);
            c.current_mana = 0.0;
            c.next_attack_bonus_damage = QUEUED_STRIKE;
        })
        .place(warrior, 1.5, 0.0)
        .place(warlock, -20.0, 0.0);
    for who in [warrior, warlock] {
        stage = stage
            .combatant(who, unkillable)
            .target(who, priest)
            .cooldown(who, AbilityType::Fear, 1000.0)
            .cooldown(who, AbilityType::DeathCoil, 1000.0);
    }
    if feared {
        stage = stage.world(move |world| {
            let caster = warlock.entity(world);
            let priest = priest.entity(world);
            world
                .get_mut::<ActiveAuras>(priest)
                .expect("the Priest's auras")
                .auras
                .push(Aura {
                    effect_type: AuraType::Fear,
                    duration: HOLD_SECS,
                    break_on_damage_threshold: 100.0,
                    caster: Some(caster),
                    ability_name: "Fear".into(),
                    ..Default::default()
                });
        });
    }
    let config = HeadlessMatchConfig {
        team1: vec!["Warrior".into(), "Warlock".into()],
        team2: vec!["Priest".into()],
        random_seed: Some(6),
        max_duration_secs: 15.0,
        ..Default::default()
    };
    play(config, stage)
}

/// The Warrior's (team 1, slot 0) observation on each staged frame, beside
/// whether the Priest still carried the Fear on it.
fn warrior_track(
    played: &common::staged::Played,
) -> Vec<(f32, arenasim::headless::ObservedCombatant, bool)> {
    played
        .staged_frames()
        .map(|frame| {
            let find = |team: u8| {
                frame
                    .combatants
                    .values()
                    .find(|c| c.team == team && c.slot == 0 && !c.is_pet)
                    .expect("the unit is in the frame")
            };
            let feared = find(2).aura_types.contains(&AuraType::Fear);
            (frame.sim_time, find(1).clone(), feared)
        })
        .collect()
}

/// The sim time of the Warrior's first swing after `from`: the first frame
/// its swing timer went back down.
fn first_swing_after(
    track: &[(f32, arenasim::headless::ObservedCombatant, bool)],
    from: f32,
) -> Option<f32> {
    track
        .windows(2)
        .find(|w| w[1].0 >= from && w[1].1.attack_timer < w[0].1.attack_timer)
        .map(|w| w[1].0)
}

/// Under its Warlock's Fear the Warrior's swing is never made, so it gains no
/// rage and keeps its queued Heroic Strike; without the Fear the first swing
/// pays rage and spends the strike, in the same window.
#[test]
fn a_swing_held_off_the_teams_fear_spends_no_rage_and_no_heroic_strike() {
    let free = warrior_track(&warrior_beside_its_teams_fear(false));
    let held_played = warrior_beside_its_teams_fear(true);
    let held = warrior_track(&held_played);
    let landed = held_played.landed().app;
    let window_end = landed + HOLD_SECS - 0.5;
    let in_window = |track: &[(f32, arenasim::headless::ObservedCombatant, bool)]| {
        track
            .iter()
            .filter(|(t, ..)| *t < window_end)
            .map(|(_, w, _)| w.clone())
            .collect::<Vec<_>>()
    };

    // The control: a swing lands in the window, pays rage and spends the
    // queued strike.
    let free_window = in_window(&free);
    assert!(
        first_swing_after(&free, landed).is_some_and(|t| t < window_end),
        "without the Fear, the Warrior swings in the window"
    );
    assert!(
        free_window.iter().any(|w| w.current_mana > 0.0),
        "without the Fear, a swing pays the Warrior rage"
    );
    assert!(
        free_window
            .iter()
            .any(|w| w.next_attack_bonus_damage == 0.0),
        "without the Fear, the first swing spends the queued Heroic Strike"
    );

    // Under the Fear: the hold covers the window, and no swing spends a thing.
    let held_window = in_window(&held);
    assert!(
        held.iter()
            .filter(|(t, ..)| *t < window_end)
            .all(|(_, _, feared)| *feared),
        "the Fear holds the Priest across the whole window"
    );
    assert!(
        held_window.iter().all(|w| w.current_mana == 0.0),
        "under its team's Fear, the Warrior gains no rage: {:?}",
        held_window
            .iter()
            .map(|w| w.current_mana)
            .fold(0.0, f32::max)
    );
    assert!(
        held_window
            .iter()
            .all(|w| w.next_attack_bonus_damage == QUEUED_STRIKE),
        "under its team's Fear, the Warrior keeps its queued Heroic Strike"
    );
}

/// Under its Warlock's Fear the Warrior's swing timer keeps building, so its
/// first swing lands on the frame the Fear ends — the queued Heroic Strike
/// with it. Without the Fear the timer swings and resets in the same window.
#[test]
fn a_swing_held_off_the_teams_fear_lands_the_moment_it_ends() {
    let free = warrior_track(&warrior_beside_its_teams_fear(false));
    let held_played = warrior_beside_its_teams_fear(true);
    let held = warrior_track(&held_played);
    let landed = held_played.landed().app;

    // The control: the timer resets inside what would be the hold.
    let free_swing = first_swing_after(&free, landed).expect("the free Warrior swings");
    assert!(
        free_swing < landed + HOLD_SECS - 0.5,
        "without the Fear, the Warrior swings at {free_swing} inside the window"
    );

    // Under the Fear: the timer never goes back down while the Fear lasts...
    let ended = held
        .iter()
        .find(|(_, _, feared)| !feared)
        .map(|(t, ..)| *t)
        .expect("the Fear ends inside the match");
    assert!(
        ended > landed + HOLD_SECS - 0.5,
        "the Fear lasted its {HOLD_SECS}s (ended at {ended}, landed at {landed})"
    );
    assert!(
        held.windows(2)
            .filter(|w| w[1].0 < ended)
            .all(|w| w[1].1.attack_timer >= w[0].1.attack_timer),
        "under its team's Fear, the Warrior's swing timer keeps building"
    );
    // ...and the first swing lands on the frame it ends, carrying the strike.
    let held_swing = first_swing_after(&held, landed).expect("the held Warrior swings");
    assert!(
        held_swing - ended <= FRAME + 1.0e-4,
        "the Warrior's first swing lands at {held_swing}, the frame the Fear ended at {ended}"
    );
    let strike = "[DMG] Team 1 Warrior #1's Heroic Strike hits Team 2 Priest #1";
    let first_hit = held_played
        .log
        .lines()
        .find(|l| l.contains("[DMG] Team 1 Warrior #1's"))
        .expect("the Warrior's first hit is logged");
    assert!(
        first_hit.contains(strike),
        "the first swing after the Fear is the queued Heroic Strike: {first_hit}"
    );
}
