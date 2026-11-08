//! Proc trinkets, observed in a real headless match (AS-140).
//!
//! The unit tests in `proc_trinkets.rs` prove the roll, the internal cooldown
//! and the pricing in isolation. They cannot prove the two things that only
//! the assembled simulation can answer:
//!
//! 1. **A proc actually fires.** The hooks are in `combat_auto_attack` and
//!    `process_casting`; a wiring mistake in either would leave every unit test
//!    green and the feature inert.
//! 2. **Two different proc trinkets are live at once** — the per-trinket
//!    internal cooldown with no global lock.
//!
//! Both are read off `FrameObservation::aura_types`, which lists the effect
//! type of every live aura. The Dragonspine Trophy is a Warrior's only source
//! of `AttackSpeedIncrease` and the Whetstone of Fury its only source of
//! `CritChanceIncrease`, so each type on the wearer IS that trinket's buff.
//!
//! That a proc buff coexists with a same-type buff from another source (an
//! attack-power proc beside Battle Shout, the reason `Aura::distinct_by_source`
//! exists) is held where the dedup runs, by `tests/proc_buff_removal.rs`: no
//! shipped trinket procs a type a Warrior's own kit grants. That the haste the
//! Dragonspine Trophy grants speeds the swing timer is held by
//! `tests/melee_avoidance.rs`.
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

/// A Warrior wearing Dragonspine Trophy procs its haste: an
/// `AttackSpeedIncrease` buff, one at a time (its internal cooldown outlasts
/// the buff).
///
/// The control below is what makes the number mean something — without it, a
/// "1" could be haste from the Warrior's own kit.
#[test]
fn a_melee_proc_fires_and_hastes_the_wearer() {
    let with_trinket = peak_concurrent(
        config(
            "Warrior",
            "Priest",
            770_140,
            &[("Trinket2", "DragonspineTrophy")],
        ),
        AuraType::AttackSpeedIncrease,
    );
    assert_eq!(
        with_trinket, 1,
        "a Warrior wearing Dragonspine Trophy should carry its haste buff; saw {} \
         concurrent AttackSpeedIncrease aura(s)",
        with_trinket
    );
}

/// The control for the test above: the SAME seed and matchup with no proc
/// trinket carries no haste at all. So the buff is the proc's.
#[test]
fn without_the_trinket_the_warrior_carries_no_haste() {
    let bare = peak_concurrent(
        config("Warrior", "Priest", 770_140, &[]),
        AuraType::AttackSpeedIncrease,
    );
    assert_eq!(
        bare, 0,
        "the unequipped control should carry no haste, saw {}",
        bare
    );
}

/// TWO DIFFERENT proc trinkets are both live at once — the per-trinket internal
/// cooldown with no global lock, which is what makes the second trinket socket
/// worth filling.
///
/// Asserted as an OVERLAP: a frame on which both buffs are up, not merely that
/// each fired at some point in the match. Each buff's type has no other source
/// on a Warrior (module docs), so its presence is unambiguous.
///
/// Seed re-pinned for AS-167 (weapon speed from item data), from 770_141 to
/// 770_142. The Warrior's Arcanite Reaper now swings every 3.8s, not every
/// 1.11s, so it lands about a third as many swings. Each swing still rolls a
/// flat 15% / 10%, so there are fewer proc chances per fight, and at 770_141
/// Dragonspine never fired. The first line that differs from `main` @ 17cb9f0
/// is the Priest's opening Wand Shot, 8 -> 10 (the Staff of Dominance fires
/// every 1.6s for the same DPS). Of 770_141..=770_160, 770_142 is the first
/// where both trinkets fire with overlapping buffs (Dragonspine at 23.8s,
/// Whetstone at 31.5s).
#[test]
fn two_different_proc_trinkets_are_live_at_the_same_time() {
    let cfg = config(
        "Warrior",
        "Priest",
        770_142,
        &[
            ("Trinket1", "DragonspineTrophy"),
            ("Trinket2", "WhetstoneOfFury"),
        ],
    );
    let mut overlap_frames = 0;
    let mut saw_haste = false;
    let mut saw_crit = false;
    run_headless_match_observed(cfg, true, None, |frame| {
        for c in frame.combatants.values() {
            if c.team != 1 || c.slot != 0 || c.is_pet {
                continue;
            }
            let haste = c.aura_types.contains(&AuraType::AttackSpeedIncrease);
            let crit = c.aura_types.contains(&AuraType::CritChanceIncrease);
            saw_haste |= haste;
            saw_crit |= crit;
            if haste && crit {
                overlap_frames += 1;
            }
        }
    })
    .expect("match runs");

    // Named separately so a failure says WHICH trinket never fired, rather
    // than only that the overlap was zero.
    assert!(saw_haste, "Dragonspine Trophy never procced");
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
///
/// The Priest half's seed was re-pinned for AS-167 (weapon speed from item
/// data), from 770_143 to 770_144. At 770_143 the Warrior's 3.8s Arcanite
/// Reaper now kills the Priest in 17.6s of combat, after ONE landed heal, so
/// the 20% Reliquary proc had one chance; the first line that differs from
/// `main` @ 17cb9f0 is the Priest's opening Wand Shot, 8 -> 10 (the Staff of
/// Dominance fires every 1.6s for the same DPS). A scan of 770_143..=770_162
/// picked 770_144: eight landed heals over a 36s fight, one proc.
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
            770_144,
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

/// Run a match with its combat log written, and return the log's text.
fn logged_match(cfg: HeadlessMatchConfig) -> String {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    drop(tmp);
    let cfg = HeadlessMatchConfig {
        output_path: Some(path.to_string_lossy().into_owned()),
        ..cfg
    };
    run_headless_match_observed(cfg, false, None, |_| {}).expect("match runs");
    let text = std::fs::read_to_string(&path).expect("the match log was written");
    let _ = std::fs::remove_file(&path);
    text
}

/// A proc is logged twice, under the TRINKET's name: the `procs` line a sweep
/// counts proc rates by, and the buff line with what it grants.
///
/// Same seed as `a_melee_proc_fires_and_hastes_the_wearer`, where the proc
/// demonstrably fires.
#[test]
fn a_proc_buff_is_logged_under_the_trinket_s_name() {
    let log = logged_match(config(
        "Warrior",
        "Priest",
        770_140,
        &[("Trinket2", "DragonspineTrophy")],
    ));
    assert!(
        log.contains("procs Dragonspine Trophy"),
        "the proc never fired, so this test proves nothing"
    );
    assert!(
        log.contains("gains Dragonspine Trophy (+20% attack speed)"),
        "the proc's [BUFF] line does not name the trinket and its haste"
    );
}

/// `MeleeHit` is a MELEE swing. A Hunter's Auto Shot is an auto-attack too, but
/// it is loosed from the ranged socket (`AutoAttackKind::Shot`), and must not
/// fire a melee proc.
///
/// Counted, not assumed: the logs are required to hold at least 50 of the
/// Hunter's Auto Shots, and at Dragonspine's 15% a trigger that DID fire on
/// shots would stay silent across 50 of them with probability 0.85^50 < 0.1%.
///
/// Six seeds, not five, since AS-167 (weapon speed from item data): the five
/// seeds `770_150..770_155` fell from 57 Auto Shots to 48, because two of
/// those fights got shorter — the first line that differs from `main` @
/// 17cb9f0 is the Hunter's opening Auto Shot, 65 -> 62 absorbed, the Ashwood
/// Bow's 2.4s shot at the bow's unchanged DPS. The sixth seed brings the count
/// back to 62.
#[test]
fn a_ranged_auto_attack_does_not_fire_a_melee_proc() {
    let mut shots = 0;
    for seed in 770_150..770_156 {
        let log = logged_match(config(
            "Hunter",
            "Priest",
            seed,
            &[("Trinket2", "DragonspineTrophy")],
        ));
        shots += log.matches("Team 1 Hunter #1's Auto Shot").count();
        assert!(
            !log.contains("procs Dragonspine Trophy"),
            "seed {seed}: a Hunter's Auto Shot procced a melee trinket"
        );
    }
    assert!(
        shots >= 50,
        "only {shots} Auto Shots across the seeds — too few for the silence to mean anything"
    );
}

/// Team 1's Rogue's landed auto-attacks and Hand of Justice procs across the
/// seeds, each match played with `gear` on the Rogue.
fn rogue_swings_and_procs(seeds: std::ops::Range<u64>, gear: &[(&str, &str)]) -> (usize, usize) {
    let (mut hits, mut procs) = (0, 0);
    for seed in seeds {
        let log = logged_match(config("Rogue", "Paladin", seed, gear));
        hits += log.matches("Team 1 Rogue #1's Auto Attack hits").count()
            + log.matches("Team 1 Rogue #1's Auto Attack CRITS").count();
        procs += log
            .matches("Team 1 Rogue #1 procs Hand of Justice (extra attack)")
            .count();
    }
    (hits, procs)
}

/// Hand of Justice procs its extra swing in real matches, at its declared
/// chance per landed melee hit (AS-427). A Rogue, because it rolls the proc
/// off both hands, against a Paladin, the longest-lived melee fight.
///
/// The landed hits include the extra swings themselves, which roll nothing,
/// so the rolled hits lie between `hits - procs` and `hits`: the rate is
/// bracketed, not estimated, and both ends must sit near 5%. Thirty seeds give
/// on the order of a thousand rolls, where a binomial 5% has a standard
/// error under 0.7 points, so the [3%, 7.5%] window is several of them wide
/// and holds for any bank, not a fitted one.
#[test]
fn hand_of_justice_procs_at_its_chance_in_real_matches() {
    let (hits, procs) = rogue_swings_and_procs(770_427..770_457, &[("Trinket2", "HandOfJustice")]);
    assert!(
        hits >= 500,
        "only {hits} landed swings — too few to judge a rate"
    );
    assert!(
        procs > 0,
        "Hand of Justice never procced in {hits} landed swings"
    );
    let low = procs as f64 / hits as f64;
    let high = procs as f64 / (hits - procs) as f64;
    assert!(
        low >= 0.03 && high <= 0.075,
        "Hand of Justice procced on {procs} of {hits} landed swings: rate in [{low:.3}, {high:.3}], \
         not near its 5%"
    );
}

/// Gressil, Dawn of Ruin procs the same extra swing from the main hand
/// (AS-427), and a Rogue holding its default main hand logs none.
#[test]
fn gressil_procs_its_extra_swing_in_real_matches() {
    let count = |gear: &[(&str, &str)]| {
        (770_427..770_437)
            .map(|seed| {
                logged_match(config("Rogue", "Paladin", seed, gear))
                    .matches("Team 1 Rogue #1 procs Gressil, Dawn of Ruin (extra attack)")
                    .count()
            })
            .sum::<usize>()
    };
    assert!(
        count(&[("MainHand", "Gressil")]) > 0,
        "Gressil never procced across the seeds"
    );
    assert_eq!(count(&[]), 0);
}

/// The control: the same Rogue and seeds without the trinket log no proc.
#[test]
fn without_hand_of_justice_nothing_procs_an_extra_swing() {
    let (hits, procs) = rogue_swings_and_procs(770_427..770_457, &[]);
    assert!(hits >= 500, "only {hits} landed swings");
    assert_eq!(procs, 0);
}

/// An extra swing is a MELEE proc: a Priest's wand shots never roll it, however
/// many it fires. Counted, not assumed — at 5% a trigger that DID fire on wand
/// shots would stay silent across 100 of them with probability 0.95^100 < 1%.
#[test]
fn a_wand_shot_does_not_proc_an_extra_swing() {
    let mut shots = 0;
    for seed in 770_427..770_431 {
        let log = logged_match(config(
            "Priest",
            "Priest",
            seed,
            &[("Trinket2", "HandOfJustice")],
        ));
        shots += log.matches("Team 1 Priest #1's Wand Shot").count();
        assert!(
            !log.contains("procs Hand of Justice"),
            "seed {seed}: a Priest's wand shot procced Hand of Justice"
        );
    }
    assert!(shots >= 100, "only {shots} wand shots across the seeds");
}

/// The REALISED uptime of each melee proc against the long-run bound it is
/// priced at, over 60 Warrior+Priest mirrors — the measurement the
/// `proc_trinkets` module docs quote. A report, not an assertion: `--ignored`,
/// and read with `--nocapture`.
///
/// ```text
/// cargo test --release --test proc_trinket_probes -- --ignored --nocapture
/// ```
///
/// The wearer is team 1's Warrior, carrying both melee trinkets and set to
/// Demoralizing Shout. Neither buff's type has another source on its side, so
/// any `AttackSpeedIncrease` on it IS the Dragonspine buff and any
/// `CritChanceIncrease` the Whetstone's. Uptime is measured per frame over the
/// frames the wearer is alive after the gates, pooled across matches; the mean
/// of those lives is printed too.
#[test]
#[ignore]
fn realised_uptime_against_the_long_run_bound() {
    let mut alive = 0u64;
    let mut dragonspine = 0u64;
    let mut whetstone = 0u64;
    let mut both = 0u64;
    for seed in 900_000..900_060 {
        let mut gear: HashMap<String, String> = HashMap::new();
        gear.insert("Trinket1".into(), "DragonspineTrophy".into());
        gear.insert("Trinket2".into(), "WhetstoneOfFury".into());
        let cfg = HeadlessMatchConfig {
            team1: vec!["Warrior".into(), "Priest".into()],
            team2: vec!["Warrior".into(), "Priest".into()],
            random_seed: Some(seed),
            team1_equipment: vec![gear],
            team1_warrior_shouts: vec!["demoralizing".into()],
            ..Default::default()
        };
        run_headless_match_observed(cfg, true, None, |frame| {
            if !frame.gates_open {
                return;
            }
            for c in frame.combatants.values() {
                if c.team != 1 || c.slot != 0 || c.is_pet || !c.alive {
                    continue;
                }
                alive += 1;
                let haste = c.aura_types.contains(&AuraType::AttackSpeedIncrease);
                let crit = c.aura_types.contains(&AuraType::CritChanceIncrease);
                dragonspine += haste as u64;
                whetstone += crit as u64;
                both += (haste && crit) as u64;
            }
        })
        .expect("match runs");
    }
    let pct = |n: u64| 100.0 * n as f64 / alive as f64;
    println!("wearer alive post-gate frames: {alive}");
    // A frame is exactly 1/60s (`FrameObservation::sim_time`).
    println!(
        "wearer's mean post-gate life: {:.1}s",
        alive as f64 / 60.0 / 60.0
    );
    println!(
        "Dragonspine Trophy: realised {:.1}% vs long-run bound {:.1}%",
        pct(dragonspine),
        100.0 * 10.0 / 55.0
    );
    println!(
        "Whetstone of Fury:  realised {:.1}% vs long-run bound {:.1}%",
        pct(whetstone),
        100.0 * 8.0 / 68.0
    );
    println!(
        "both up: {:.1}% vs {:.1}% if independent",
        pct(both),
        pct(dragonspine) * pct(whetstone) / 100.0
    );
    assert!(alive > 0 && dragonspine > 0 && whetstone > 0, "vacuous run");
}
