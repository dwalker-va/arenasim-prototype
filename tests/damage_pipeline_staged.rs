//! The damage pipeline's mechanisms reached through the real damage sites
//! (AS-408), in staged matches: each case is the same seed played twice, the
//! staged aura or talent mechanic its one difference.
//!
//! A unit's FIRST hit of an ability comes before anything the difference can
//! change (no damage-dealt multiplier draws a roll), so the two arms' first
//! hits differ by exactly the multiplier.

mod common;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::match_config::RoguePoison;
use arenasim::states::play_match::abilities::SpellSchool;
use arenasim::states::play_match::components::{Aura, AuraDamageMods, AuraType, DamageScope};
use arenasim::states::play_match::talent_mechanics::{TalentMechanic, TalentMechanics};
use arenasim::states::play_match::AbilityType;
use common::staged::{play, Played};

const T1: Unit = Unit::new(1, 0);
const T2: Unit = Unit::new(2, 0);

fn config(team1: &str, team2: &str, seed: u64, secs: f32) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec![team1.into()],
        team2: vec![team2.into()],
        random_seed: Some(seed),
        max_duration_secs: secs,
        ..Default::default()
    }
}

/// A long aura of `kind` with `magnitude`, in `scope`.
fn aura(kind: AuraType, magnitude: f32, scope: Option<DamageScope>) -> Aura {
    Aura {
        effect_type: kind,
        duration: 600.0,
        magnitude,
        break_on_damage_threshold: -1.0,
        ability_name: "Staged".into(),
        damage_mods: AuraDamageMods {
            scope,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Every damage number (to health and absorbed) the log gives `actor`'s `ability` ("X's Frostbolt hits
/// Y for 89 damage", "X's Corruption ticks for 12 damage on Y"), in order.
fn hits(played: &Played, actor: &str, ability: &str) -> Vec<f32> {
    let hit = format!("{actor}'s {ability} hits ");
    let tick = format!("{actor}'s {ability} ticks for ");
    played
        .log
        .lines()
        .filter_map(|line| {
            let rest = if let Some(i) = line.find(&hit) {
                let rest = &line[i + hit.len()..];
                &rest[rest.find(" for ")? + 5..]
            } else {
                &line[line.find(&tick)? + tick.len()..]
            };
            // "89 damage" or "1 damage (60 absorbed)": the hit is the two.
            let dealt: f32 = rest.split(' ').next()?.parse().ok()?;
            let absorbed = rest
                .split('(')
                .nth(1)
                .and_then(|a| a.split(' ').next()?.parse::<f32>().ok())
                .unwrap_or(0.0);
            Some(dealt + absorbed)
        })
        .collect()
}

fn first(played: &Played, actor: &str, ability: &str) -> f32 {
    *hits(played, actor, ability)
        .first()
        .unwrap_or_else(|| panic!("{actor} landed no {ability}:\n{}", played.log))
}

/// `with` is within a rounding of twice `without`: the log prints whole
/// numbers.
fn doubled(with: f32, without: f32) {
    assert!(
        (with - 2.0 * without).abs() <= 2.0 && without > 0.0,
        "{with} is not twice {without}"
    );
}

const WARRIOR: &str = "Team 1 Warrior #1";
const MAGE: &str = "Team 2 Mage #1";
const WARLOCK: &str = "Team 1 Warlock #1";

/// The Warrior swings first at the Mage it starts beside; the Mage's first
/// Frostbolt flies at the Warrior later.
fn melee_stage() -> Stage {
    Stage::new().place(T1, 0.0, 0.0).place(T2, 2.0, 0.0)
}

#[test]
fn a_damage_dealt_increase_reaches_melee_swings_abilities_and_projectiles() {
    let doubled_all = || aura(AuraType::DamageDealtIncrease, 1.0, None);
    let control = play(config("Warrior", "Mage", 3, 20.0), melee_stage());
    let warrior = play(
        config("Warrior", "Mage", 3, 20.0),
        melee_stage().aura(T1, doubled_all()),
    );
    // The auto-attack site.
    doubled(
        first(&warrior, WARRIOR, "Auto Attack"),
        first(&control, WARRIOR, "Auto Attack"),
    );
    // The instant-attack site.
    doubled(
        first(&warrior, WARRIOR, "Mortal Strike"),
        first(&control, WARRIOR, "Mortal Strike"),
    );
    // The projectile site: the Mage's first Frostbolt, with the Mage doubled.
    let mage = play(
        config("Warrior", "Mage", 3, 20.0),
        melee_stage().aura(T2, doubled_all()),
    );
    doubled(
        first(&mage, MAGE, "Frostbolt"),
        first(&control, MAGE, "Frostbolt"),
    );
    // Scope: a Shadow-only increase leaves the Warrior's Physical alone.
    let shadow = play(
        config("Warrior", "Mage", 3, 20.0),
        melee_stage().aura(
            T1,
            aura(
                AuraType::DamageDealtIncrease,
                1.0,
                Some(DamageScope::School(SpellSchool::Shadow)),
            ),
        ),
    );
    assert_eq!(
        hits(&shadow, WARRIOR, "Auto Attack"),
        hits(&control, WARRIOR, "Auto Attack")
    );
}

/// The DoT-tick site: a Corruption landed at the gates ticks for twice as much
/// from a doubled Warlock.
#[test]
fn a_damage_dealt_increase_reaches_damage_over_time_ticks() {
    let stage = || {
        Stage::new()
            .place(T1, 0.0, 0.0)
            .place(T2, 30.0, 0.0)
            .land(T2, AbilityType::Corruption, T1)
    };
    let control = play(config("Warlock", "Warrior", 5, 8.0), stage());
    let doubled_lock = play(
        config("Warlock", "Warrior", 5, 8.0),
        stage().aura(T1, aura(AuraType::DamageDealtIncrease, 1.0, None)),
    );
    doubled(
        first(&doubled_lock, WARLOCK, "Corruption"),
        first(&control, WARLOCK, "Corruption"),
    );
}

/// A charged vulnerability raises the direct hit and not the tick: the same
/// staged Corruption ticks as in the control under a 4-charge Shadow
/// vulnerability on its victim.
#[test]
fn a_charged_vulnerability_does_not_raise_dot_ticks() {
    let stage = || {
        Stage::new()
            .place(T1, 0.0, 0.0)
            .place(T2, 30.0, 0.0)
            .land(T2, AbilityType::Corruption, T1)
    };
    let mut vuln = aura(
        AuraType::DamageTakenIncrease,
        1.0,
        Some(DamageScope::School(SpellSchool::Shadow)),
    );
    vuln.damage_mods.charges = Some(4);
    let uncharged = aura(
        AuraType::DamageTakenIncrease,
        1.0,
        Some(DamageScope::School(SpellSchool::Shadow)),
    );
    let control = play(config("Warlock", "Warrior", 5, 8.0), stage());
    let charged = play(config("Warlock", "Warrior", 5, 8.0), stage().aura(T2, vuln));
    let plain = play(
        config("Warlock", "Warrior", 5, 8.0),
        stage().aura(T2, uncharged),
    );
    assert_eq!(
        first(&charged, WARLOCK, "Corruption"),
        first(&control, WARLOCK, "Corruption"),
        "a tick is not a hit"
    );
    // Control of the control: an uncharged vulnerability does raise the tick,
    // so the tick reaches the increase at all.
    doubled(
        first(&plain, WARLOCK, "Corruption"),
        first(&control, WARLOCK, "Corruption"),
    );
}

/// Retribution Aura's shape through the auto-attack site: a melee swing on a
/// holder is answered, and the follow-up lands in the frame's
/// `process_damage_follow_ups`.
#[test]
fn a_melee_swing_on_a_reflect_holder_is_answered() {
    let mut ret = aura(AuraType::DamageReflect, 7.0, None);
    ret.spell_school = Some(SpellSchool::Holy);
    ret.ability_name = "Retribution Aura".into();
    let with = play(
        config("Warrior", "Mage", 3, 6.0),
        melee_stage().aura(T2, ret),
    );
    let line = format!("[REFLECT] {WARRIOR} takes");
    assert!(
        with.log
            .lines()
            .any(|l| l.contains(&line) && l.contains("Retribution Aura")),
        "{}",
        with.log
    );
    let control = play(config("Warrior", "Mage", 3, 6.0), melee_stage());
    assert!(!control.log.contains("[REFLECT]"));
}

/// Soul Link end to end: the Warlock's share lands on its Felhunter.
#[test]
fn soul_link_moves_a_share_of_the_warlocks_damage_to_its_pet() {
    let stage = |link: bool| {
        let s = Stage::new().place(T1, 0.0, 0.0).place(T2, 2.0, 0.0);
        if link {
            s.combatant(T1, |c| {
                c.talent_mechanics = TalentMechanics::of(&[TalentMechanic::SoulLink { share: 0.5 }])
            })
        } else {
            s
        }
    };
    let with = play(config("Warlock", "Warrior", 7, 8.0), stage(true));
    assert!(
        with.log
            .lines()
            .any(|l| l.contains("[TRANSFER]") && l.contains("(Soul Link)")),
        "{}",
        with.log
    );
    let control = play(config("Warlock", "Warrior", 7, 8.0), stage(false));
    assert!(!control.log.contains("[TRANSFER]"));
}

/// Reflective Shield's one site: a shield cast by a Priest with the mechanic
/// lands reflecting (`apply_pending_auras`), and the shield's soak is
/// answered.
#[test]
fn a_shield_from_a_reflective_shield_priest_reflects() {
    let stage = |reflect: bool| {
        let s = Stage::new()
            .place(T1, 0.0, 0.0)
            .place(T2, 2.0, 0.0)
            .clear_auras(T2)
            .land(T2, AbilityType::PowerWordShield, T2);
        if reflect {
            s.combatant(T2, |c| {
                c.talent_mechanics =
                    TalentMechanics::of(&[TalentMechanic::ReflectiveShield { share: 0.5 }])
            })
        } else {
            s
        }
    };
    let with = play(config("Warrior", "Priest", 3, 6.0), stage(true));
    assert!(
        with.log
            .lines()
            .any(|l| l.contains(&format!("[REFLECT] {WARRIOR} takes"))
                && l.contains("Power Word: Shield")),
        "{}",
        with.log
    );
    let control = play(config("Warrior", "Priest", 3, 6.0), stage(false));
    assert!(!control.log.contains("[REFLECT]"));
}

/// Master Poisoner's slot through the auto-attack site: with an off-hand
/// poison, the off hand's landed swings roll it — Wound Poison, which deals
/// damage the log names — while the main hand keeps Crippling.
#[test]
fn an_off_hand_poison_rides_the_off_hand_swings() {
    let stage = |off: Option<RoguePoison>| {
        Stage::new()
            .place(T1, 0.0, 0.0)
            .place(T2, 2.0, 0.0)
            .combatant(T1, move |c| {
                c.stealthed = false;
                c.rogue_poison = RoguePoison::Crippling;
                c.rogue_offhand_poison = off;
            })
    };
    let rogue = "Team 1 Rogue #1";
    let with = play(
        config("Rogue", "Warrior", 11, 15.0),
        stage(Some(RoguePoison::Wound)),
    );
    assert!(
        !hits(&with, rogue, "Wound Poison").is_empty(),
        "{}",
        with.log
    );
    let control = play(config("Rogue", "Warrior", 11, 15.0), stage(None));
    assert!(hits(&control, rogue, "Wound Poison").is_empty());
}
