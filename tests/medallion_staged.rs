//! AS-305 — the Medallion in a played match: the held-unit dispatch reaches it
//! from `decide_abilities`, and `process_on_use_items` breaks the hold.
//!
//! The decision itself is pinned scene by scene in `medallion_scenes.rs`. This
//! is the played guard on the two halves a scene cannot see: the call site in
//! `decide_abilities` (with the enemy team's kill window it hands over) and
//! the resolution. A STAGED match lands a real Hammer of Justice on the
//! wearer at the gates, so no seed can stop reaching it.

mod common;

use std::collections::HashMap;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::AbilityType;
use common::staged::{entity_id, play, sim_time, Played};

const WARRIOR: Unit = Unit::new(1, 0);
const PRIEST: Unit = Unit::new(1, 1);
const PALADIN: Unit = Unit::new(2, 0);
const MAGE: Unit = Unit::new(2, 1);

fn config(team1: [&str; 2], medallion_on: usize) -> HeadlessMatchConfig {
    let mut equipment = vec![HashMap::new(), HashMap::new()];
    equipment[medallion_on].insert("Trinket2".to_string(), "MedallionOfTheAlliance".to_string());
    HeadlessMatchConfig {
        team1: team1.iter().map(|c| c.to_string()).collect(),
        team2: vec!["Paladin".into(), "Mage".into()],
        team1_equipment: equipment,
        random_seed: Some(7),
        max_duration_secs: 4.0,
        ..Default::default()
    }
}

/// The Warrior stunned by the Paladin's Hammer of Justice (6s) at the gates,
/// its Priest at `priest_hp`, everyone close.
fn stage(priest_hp: f32) -> Stage {
    Stage::new()
        .place(WARRIOR, 0.0, 0.0)
        .place(PRIEST, 0.0, 12.0)
        .health(PRIEST, priest_hp)
        .place(PALADIN, 3.0, 0.0)
        .place(MAGE, 0.0, -25.0)
        .land(WARRIOR, AbilityType::HammerOfJustice, PALADIN)
}

/// The wearer's `item_decision` events from the stage on, `(time, status)`.
fn item_decisions(played: &Played, team: u8, slot: u8) -> Vec<(f32, String)> {
    let id = entity_id(played, team, slot);
    played
        .staged_trace()
        .filter(|e| e["kind"] == "item_decision" && e["actor"]["entity_id"] == id)
        .map(|e| {
            assert_eq!(e["item"], "MedallionOfTheAlliance");
            (sim_time(e), e["status"].as_str().unwrap().to_string())
        })
        .collect()
}

#[test]
fn a_stunned_wearer_breaks_free_when_its_teammate_is_low() {
    let played = play(config(["Warrior", "Priest"], 0), stage(0.3));
    let decisions = item_decisions(&played, 1, 0);
    assert_eq!(
        decisions.first().map(|d| d.1.as_str()),
        Some("chosen"),
        "{decisions:?}"
    );
    assert_eq!(
        decisions.len(),
        1,
        "used once, never asked again: {decisions:?}"
    );
    assert!(
        played
            .log
            .contains("Team 1 Warrior #1's Medallion of the Alliance breaks Hammer of Justice"),
        "{}",
        played.log
    );
    // The Warrior is free: it decides an ability within a frame or two of the
    // break, five seconds before the Hammer would have let it go.
    let warrior = entity_id(&played, 1, 0);
    let used_at = decisions[0].0;
    let acts = played
        .staged_trace()
        .find(|e| e["kind"] == "ability_decision" && e["actor"]["entity_id"] == warrior)
        .map(sim_time)
        .expect("the Warrior acts after the break");
    assert!(acts - used_at < 0.1, "used at {used_at}, acted at {acts}");
}

#[test]
fn a_stunned_wearer_holds_it_while_its_teammate_is_healthy() {
    let played = play(config(["Warrior", "Priest"], 0), stage(1.0));
    let decisions = item_decisions(&played, 1, 0);
    assert!(!decisions.is_empty(), "the wearer was asked");
    assert!(decisions.iter().all(|d| d.1 == "rejected"), "{decisions:?}");
    assert!(!played.log.contains("[ITEM]"), "{}", played.log);
}

/// The Warrior stunned at the gates beside its Mage, at `warrior_hp`.
fn burst_stage(warrior_hp: f32) -> Stage {
    Stage::new()
        .place(WARRIOR, 0.0, 0.0)
        .health(WARRIOR, warrior_hp)
        .place(PRIEST, 0.0, 12.0)
        .place(PALADIN, 3.0, 0.0)
        .place(MAGE, 0.0, -25.0)
        .land(WARRIOR, AbilityType::HammerOfJustice, PALADIN)
}

#[test]
fn a_stunned_wearer_the_enemy_is_bursting_breaks_free() {
    // The Warrior at 20% beside a healthy Mage: no healer can save it, so the
    // enemy's kill window names it — the trigger `decide_abilities` hands the
    // held dispatch from the enemy team's situation.
    let played = play(config(["Warrior", "Mage"], 0), burst_stage(0.2));
    let decisions = item_decisions(&played, 1, 0);
    assert_eq!(
        decisions.first().map(|d| d.1.as_str()),
        Some("chosen"),
        "{decisions:?}"
    );
    // Control: the same Warrior healthy is nobody's kill target, and its Mage
    // is healthy, so it holds the Medallion.
    let played = play(config(["Warrior", "Mage"], 0), burst_stage(1.0));
    let decisions = item_decisions(&played, 1, 0);
    assert_eq!(
        decisions.first().map(|d| d.1.as_str()),
        Some("rejected"),
        "{decisions:?}"
    );
}
