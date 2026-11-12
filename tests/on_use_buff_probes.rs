//! Buff on-use items, observed in real headless matches (AS-430).
//!
//! `tests/on_use_buff_scenes.rs` proves WHEN the AI uses one, one tick at a
//! time. These prove what only the assembled simulation can: that a use the
//! AI decides reaches `process_on_use_items`, lands its buff through
//! `apply_pending_auras`, and — for the Diamond Flask — that its heal over
//! time ticks through `process_hot_ticks`, the HoT site that heals through
//! `combat_core::apply_healing` (which `tests/heal_site_audit.rs` holds to
//! it). Each runs over a small seed bank and first asserts the event it reads
//! happened, so no claim passes on zero events.

use std::collections::HashMap;

use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};
use arenasim::states::play_match::components::AuraType;

/// A match with team 1's first member wearing `gear`.
fn config(team1: &[&str], team2: &[&str], seed: u64, gear: &[(&str, &str)]) -> HeadlessMatchConfig {
    let equipment: HashMap<String, String> = gear
        .iter()
        .map(|(socket, item)| (socket.to_string(), item.to_string()))
        .collect();
    let mut team1_equipment = vec![equipment];
    team1_equipment.resize(team1.len(), HashMap::new());
    HeadlessMatchConfig {
        team1: team1.iter().map(|c| c.to_string()).collect(),
        team2: team2.iter().map(|c| c.to_string()).collect(),
        random_seed: Some(seed),
        team1_equipment,
        ..Default::default()
    }
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

const SEEDS: std::ops::RangeInclusive<u64> = 1..=8;

/// The Diamond Flask is used once a match, lands its attack power, and heals
/// its wearer over time: 4 a tick at most (healing reduction and dampening
/// only cut it), at most the 6 ticks 30s holds at one every 5s.
#[test]
fn the_diamond_flask_is_used_once_and_heals_over_time() {
    let mut ticks = 0;
    for seed in SEEDS {
        let log = logged_match(config(
            &["Warrior", "Priest"],
            &["Rogue", "Mage"],
            seed,
            &[("Trinket2", "DiamondFlask")],
        ));
        assert_eq!(
            log.matches("[ITEM] Team 1 Warrior #1 uses Diamond Flask")
                .count(),
            1,
            "seed {seed}: the Flask is used once a match"
        );
        assert!(
            log.contains("Team 1 Warrior #1 gains Diamond Flask (+20 attack power)"),
            "seed {seed}: the Flask's attack power never landed"
        );
        // A use is logged as a use, never as a proc a proc-rate count reads.
        assert!(
            !log.contains("procs Diamond Flask"),
            "seed {seed}: the Flask's use was logged as a proc"
        );
        let heals: Vec<f32> = log
            .lines()
            .filter_map(|l| l.split("Diamond Flask heals Team 1 Warrior #1 for ").nth(1))
            .map(|n| n.trim().parse::<f32>().expect("a heal amount"))
            .collect();
        assert!(heals.len() <= 6, "seed {seed}: {} ticks", heals.len());
        assert!(
            heals.iter().all(|h| *h <= 4.0),
            "seed {seed}: a tick over 4: {heals:?}"
        );
        ticks += heals.len();
    }
    assert!(ticks >= 8, "the bank saw only {ticks} heal ticks");
}

/// The control: the same bank with no Flask has no Flask in it at all.
#[test]
fn without_the_flask_nothing_is_used_or_healed() {
    for seed in SEEDS {
        let log = logged_match(config(
            &["Warrior", "Priest"],
            &["Rogue", "Mage"],
            seed,
            &[],
        ));
        assert!(!log.contains("Diamond Flask"), "seed {seed}");
        assert!(!log.contains("[ITEM]"), "seed {seed}");
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
            peak = peak.max(c.aura_types.iter().filter(|a| **a == effect).count());
        }
    })
    .expect("match runs");
    peak
}

/// Kiss of the Spider hastes a Hunter — a ranged attacker, whose kit grants
/// no haste of its own — in the kill window of a 1v1 it wins.
#[test]
fn kiss_of_the_spider_hastes_a_ranged_wearer() {
    let mut hasted = 0;
    for seed in SEEDS {
        let with = peak_concurrent(
            config(
                &["Hunter"],
                &["Warlock"],
                seed,
                &[("Trinket2", "KissOfTheSpider")],
            ),
            AuraType::AttackSpeedIncrease,
        );
        assert!(with <= 1, "seed {seed}: {with} haste auras at once");
        hasted += with;
        let bare = peak_concurrent(
            config(&["Hunter"], &["Warlock"], seed, &[]),
            AuraType::AttackSpeedIncrease,
        );
        assert_eq!(bare, 0, "seed {seed}: the control carries haste");
    }
    assert!(hasted >= 4, "only {hasted} of the bank's matches hasted");
}
