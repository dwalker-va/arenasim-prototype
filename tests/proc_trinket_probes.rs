//! Proc trinkets, observed in a real headless match (AS-140).
//!
//! The unit tests in `proc_trinkets.rs` prove the roll, the internal cooldown
//! and the pricing in isolation. They cannot prove the two things that only
//! the assembled simulation can answer:
//!
//! 1. **A proc actually fires.** The hooks are in `combat_auto_attack` and
//!    `process_casting`; a wiring mistake in either would leave every unit test
//!    green and the feature inert.
//! 2. **A proc buff COEXISTS with a same-stat buff from another source.** This
//!    is the whole reason `Aura::distinct_by_source` exists. A Warrior carries
//!    Battle Shout, which is an `AttackPowerIncrease`; before the flag, the
//!    buff dedup in `apply_pending_auras` would have swallowed an attack-power
//!    proc for the entire match, silently.
//!
//! Both are read off `FrameObservation::aura_types`, which lists the effect
//! type of every live aura — so "two AttackPowerIncrease auras at once" IS the
//! coexistence claim, stated in the only vocabulary the observer has.
//!
//! The observer is read-only by construction (`run_headless_match_observed`),
//! and `tests/movement_probes.rs` pins that an observed run returns a result
//! bit-identical to an unobserved one.

use std::collections::HashMap;

use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};
use arenasim::states::play_match::components::AuraType;

/// A 1v1 at a fixed seed, optionally with equipment overrides on team 1's
/// single member.
fn config(team1: &str, team2: &str, seed: u64, t1_gear: &[(&str, &str)]) -> HeadlessMatchConfig {
    let mut equipment: HashMap<String, String> = HashMap::new();
    for (socket, item) in t1_gear {
        equipment.insert(socket.to_string(), item.to_string());
    }
    HeadlessMatchConfig {
        team1: vec![team1.to_string()],
        team2: vec![team2.to_string()],
        random_seed: Some(seed),
        team1_equipment: vec![equipment],
        ..Default::default()
    }
}

/// The largest number of `effect` auras team 1 slot 0 carried on any one frame.
fn peak_concurrent(cfg: HeadlessMatchConfig, effect: AuraType) -> usize {
    let mut peak = 0;
    run_headless_match_observed(cfg, true, None, |frame| {
        for c in frame.combatants.values() {
            if c.team != 1 || c.slot != 0 || c.is_pet {
                continue;
            }
            let n = c.aura_types.iter().filter(|a| **a == effect).count();
            peak = peak.max(n);
        }
    })
    .expect("match runs");
    peak
}

/// A Warrior wearing Dragonspine Trophy ends up with TWO attack-power buffs at
/// once: Battle Shout's and the trinket's.
///
/// The control below is what makes the number mean something — without it, a
/// "2" could be two Battle Shouts, or an observer counting the same aura twice.
#[test]
fn a_melee_proc_fires_and_coexists_with_battle_shout() {
    let with_trinket = peak_concurrent(
        config(
            "Warrior",
            "Priest",
            770_140,
            &[("Trinket2", "DragonspineTrophy")],
        ),
        AuraType::AttackPowerIncrease,
    );
    assert_eq!(
        with_trinket, 2,
        "a Warrior wearing Dragonspine Trophy should carry Battle Shout's attack power AND \
         the trinket's at the same time; saw {} concurrent AttackPowerIncrease aura(s)",
        with_trinket
    );
}

/// The control for the test above: the SAME seed and matchup with no proc
/// trinket never reaches two. So the second buff is the proc, not the Warrior's
/// own kit.
#[test]
fn without_the_trinket_the_warrior_carries_only_battle_shout() {
    let bare = peak_concurrent(
        config("Warrior", "Priest", 770_140, &[]),
        AuraType::AttackPowerIncrease,
    );
    assert_eq!(
        bare, 1,
        "the unequipped control should see only Battle Shout, saw {}",
        bare
    );
}

/// TWO DIFFERENT proc trinkets are both live at once — the per-trinket internal
/// cooldown with no global lock, which is what makes the second trinket socket
/// worth filling.
///
/// Asserted as an OVERLAP: a frame on which both buffs are up, not merely that
/// each fired at some point in the match. `Whetstone of Fury` is the only
/// source of `CritChanceIncrease` a Warrior has, so its presence is
/// unambiguous.
#[test]
fn two_different_proc_trinkets_are_live_at_the_same_time() {
    let cfg = config(
        "Warrior",
        "Priest",
        770_141,
        &[
            ("Trinket1", "DragonspineTrophy"),
            ("Trinket2", "WhetstoneOfFury"),
        ],
    );
    let mut overlap_frames = 0;
    let mut saw_ap = false;
    let mut saw_crit = false;
    run_headless_match_observed(cfg, true, None, |frame| {
        for c in frame.combatants.values() {
            if c.team != 1 || c.slot != 0 || c.is_pet {
                continue;
            }
            // Battle Shout is one of the AttackPowerIncrease auras, so the
            // TRINKET's is the second one.
            let ap = c
                .aura_types
                .iter()
                .filter(|a| **a == AuraType::AttackPowerIncrease)
                .count()
                >= 2;
            let crit = c.aura_types.contains(&AuraType::CritChanceIncrease);
            saw_ap |= ap;
            saw_crit |= crit;
            if ap && crit {
                overlap_frames += 1;
            }
        }
    })
    .expect("match runs");

    // Named separately so a failure says WHICH trinket never fired, rather
    // than only that the overlap was zero.
    assert!(saw_ap, "Dragonspine Trophy never procced");
    assert!(saw_crit, "Whetstone of Fury never procced");
    assert!(
        overlap_frames > 0,
        "both trinkets procced but never overlapped — with per-trinket cooldowns and no \
         global lock, two buffs of different lengths should share frames"
    );
}

/// A caster proc fires off a completed cast, and a healer proc off a completed
/// heal — the two trigger kinds the melee tests above cannot reach.
///
/// One test over both because they share the claim: `process_casting`'s two
/// resolution points are wired. Each half names itself on failure.
///
/// The Mage half deliberately faces a PRIEST rather than a Warrior. Against a
/// Warrior the 1v1 is over in about twenty seconds, which is few enough casts
/// that a 12% proc genuinely misses on some seeds — the probe would then be
/// pinning the seed rather than the wiring.
#[test]
fn the_cast_and_heal_triggers_both_fire() {
    let caster = peak_concurrent(
        config(
            "Mage",
            "Priest",
            770_142,
            &[("Trinket2", "SigilOfArcaneSurge")],
        ),
        AuraType::SpellPowerIncrease,
    );
    assert_eq!(
        caster, 1,
        "Sigil of Arcane Surge should proc off the Mage's Frostbolts; saw {}",
        caster
    );

    let healer = peak_concurrent(
        config(
            "Priest",
            "Warrior",
            770_143,
            &[("Trinket2", "ReliquaryOfRenewal")],
        ),
        AuraType::ManaRegenIncrease,
    );
    assert_eq!(
        healer, 1,
        "Reliquary of Renewal should proc off the Priest's heals; saw {}",
        healer
    );
}

/// The healer trinket's trigger is a HEAL, not any cast — so a Mage wearing it
/// never procs it. This is what keeps `Heal` a real axis rather than a second
/// spelling of `SpellCast`.
///
/// The SAME matchup and seed as the Mage half above, where a `SpellCast`
/// trinket demonstrably fires. So this zero is the TRIGGER declining, not a
/// match too short for anything to happen in.
#[test]
fn the_heal_trigger_does_not_fire_on_a_damage_cast() {
    let mage = peak_concurrent(
        config(
            "Mage",
            "Priest",
            770_142,
            &[("Trinket2", "ReliquaryOfRenewal")],
        ),
        AuraType::ManaRegenIncrease,
    );
    assert_eq!(
        mage, 0,
        "a Mage casts constantly and heals never — a Heal-triggered proc must stay silent, \
         saw {}",
        mage
    );
}
