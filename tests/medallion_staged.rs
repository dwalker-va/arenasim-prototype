//! AS-305 — the Medallion in a played match: the held-unit dispatch reaches it
//! from `decide_abilities`, and `process_on_use_items` breaks the hold.
//!
//! The decision itself is pinned scene by scene in `medallion_scenes.rs`. This
//! is the played guard on what a scene cannot see: the call site in
//! `decide_abilities`, the resolution, the wait's state carried from one frame
//! to the next, and a teammate's dispel landing while the wearer waits. A
//! STAGED match lands the crowd control on the wearer at the gates, so no
//! seed can stop reaching it.

mod common;

use std::collections::HashMap;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::{AbilityType, Aura, AuraType};
use common::staged::{entity_id, play, sim_time, Played};

const WEARER: Unit = Unit::new(1, 0);
const MATE: Unit = Unit::new(1, 1);
const PALADIN: Unit = Unit::new(2, 0);
const MAGE: Unit = Unit::new(2, 1);

/// Team 1 is a Warrior wearing the Medallion and `mate`, against a Paladin
/// and a Mage.
fn config(mate: &str) -> HeadlessMatchConfig {
    let mut equipment = vec![HashMap::new(), HashMap::new()];
    equipment[0].insert("Trinket2".to_string(), "MedallionOfTheAlliance".to_string());
    HeadlessMatchConfig {
        team1: vec!["Warrior".into(), mate.into()],
        team2: vec!["Paladin".into(), "Mage".into()],
        team1_equipment: equipment,
        random_seed: Some(7),
        max_duration_secs: 4.0,
        ..Default::default()
    }
}

/// Everyone close: the mate 12yd from the wearer, the Paladin beside it, the
/// Mage 25yd off.
fn placed() -> Stage {
    Stage::new()
        .place(WEARER, 0.0, 0.0)
        .place(MATE, 0.0, 12.0)
        .place(PALADIN, 3.0, 0.0)
        .place(MAGE, 0.0, -25.0)
}

/// The wearer's `item_decision` events from the stage on, `(time, status,
/// note)`.
fn item_decisions(played: &Played) -> Vec<(f32, String, String)> {
    let id = entity_id(played, 1, 0);
    played
        .staged_trace()
        .filter(|e| e["kind"] == "item_decision" && e["actor"]["entity_id"] == id)
        .map(|e| {
            assert_eq!(e["item"], "MedallionOfTheAlliance");
            (
                sim_time(e),
                e["status"].as_str().unwrap().to_string(),
                e["reason"]["PreconditionUnmet"]["note"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            )
        })
        .collect()
}

#[test]
fn a_long_stun_is_broken_at_once() {
    let stage = placed().land(WEARER, AbilityType::HammerOfJustice, PALADIN);
    let played = play(config("Priest"), stage);
    let decisions = item_decisions(&played);
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
fn a_short_stun_runs_out_with_the_medallion_kept() {
    let stun = Aura {
        effect_type: AuraType::Stun,
        duration: 1.5,
        break_on_damage_threshold: -1.0,
        ability_name: "Hammer of Justice".to_string(),
        ..Default::default()
    };
    let played = play(config("Priest"), placed().aura(WEARER, stun));
    let decisions = item_decisions(&played);
    assert!(!decisions.is_empty(), "the wearer was asked");
    assert!(
        decisions
            .iter()
            .all(|d| d.1 == "rejected" && d.2.contains("short hold")),
        "{decisions:?}"
    );
    assert!(!played.log.contains("[ITEM]"), "{}", played.log);
}

#[test]
fn a_polymorph_its_priest_dispels_keeps_the_medallion() {
    let stage = placed().land(WEARER, AbilityType::Polymorph, MAGE);
    let played = play(config("Priest"), stage);
    // While polymorphed the wearer only waited: its first ask, on the frame the
    // Polymorph landed, holds the Medallion for the Priest...
    let decisions = item_decisions(&played);
    let first = decisions.first().expect("the wearer was asked");
    assert!(
        first.1 == "rejected" && first.2.contains("waiting for"),
        "{decisions:?}"
    );
    // ...whose Dispel Magic took the Polymorph off inside the grace, so the
    // Medallion never broke it. (It stays ready for the next long hold: the
    // Paladin's Hammer of Justice, which it breaks.)
    assert!(
        played
            .log
            .contains("[DISPEL] Polymorph removed from Team 1 Warrior #1"),
        "{}",
        played.log
    );
    assert!(!played.log.contains("breaks Polymorph"), "{}", played.log);
    // Control: beside a Mage, which frees nobody, the same Polymorph is
    // broken at once.
    let stage = placed().land(WEARER, AbilityType::Polymorph, MAGE);
    let played = play(config("Mage"), stage);
    let decisions = item_decisions(&played);
    assert_eq!(
        decisions.first().map(|d| d.1.as_str()),
        Some("chosen"),
        "{decisions:?}"
    );
    assert!(
        played
            .log
            .contains("Team 1 Warrior #1's Medallion of the Alliance breaks Polymorph"),
        "{}",
        played.log
    );
}
