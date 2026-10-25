//! The Peel play in a STAGED match (tier 3, `headless::stage`): a Mage whose
//! dying Priest has an enemy Warrior on it roots the Warrior with Frost Nova
//! inside one global cooldown — and, with the play withheld, does not.
//!
//! The scene tests (`tests/peel_scenes.rs`) pin the one-tick decision; this
//! pins that the decision reaches the match: the cast goes through the real
//! schedule, lands, and holds the Warrior.

mod common;

use std::collections::BTreeMap;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::class_ai::plays::every_exception_entry;
use arenasim::states::play_match::situation::PlayId;
use arenasim::states::play_match::AuraType;
use common::staged::{entity_id, of_class, play, sim_time, Played};

const MAGE: Unit = Unit::new(1, 0);
const PRIEST: Unit = Unit::new(1, 1);
const ENEMY_WARRIOR: Unit = Unit::new(2, 0);
const ENEMY_PALADIN: Unit = Unit::new(2, 1);

/// One global cooldown: the peel's deadline.
const GCD: f32 = 1.5;

fn config(seed: u64, peel: bool) -> HeadlessMatchConfig {
    // Peel graduated: its control excepts it everywhere on team 1.
    let withheld = if peel {
        BTreeMap::new()
    } else {
        BTreeMap::from([("Peel".to_string(), every_exception_entry(PlayId::Peel))])
    };
    HeadlessMatchConfig {
        team1: vec!["Mage".into(), "Priest".into()],
        team2: vec!["Warrior".into(), "Paladin".into()],
        map: "BasicArena".into(),
        ai_profile: Some("TeamPlan".into()),
        team1_exceptions: withheld,
        random_seed: Some(seed),
        max_duration_secs: 15.0,
        ..Default::default()
    }
}

/// The Priest at 30% with the enemy Warrior on it; the Mage 7yd off — inside
/// Frost Nova's 10yd, outside melee — the enemy Paladin back.
fn stage() -> Stage {
    Stage::new()
        .place(PRIEST, -20.0, 0.0)
        .health(PRIEST, 0.3)
        .place(ENEMY_WARRIOR, -18.5, 0.0)
        .target(ENEMY_WARRIOR, PRIEST)
        .place(MAGE, -13.0, 4.0)
        .place(ENEMY_PALADIN, 20.0, 0.0)
}

/// When, on the combat clock, the enemy Warrior is first rooted after the
/// stage lands.
fn warrior_rooted(played: &Played) -> Option<f32> {
    let landed = played.landed();
    played
        .staged_frames()
        .find(|f| {
            f.combatants
                .values()
                .find(|c| c.team == 2 && c.slot == 0 && !c.is_pet)
                .is_some_and(|c| c.aura_types.contains(&AuraType::Root))
        })
        .map(|f| f.sim_time - landed.app)
}

#[test]
fn the_mage_roots_the_warrior_on_its_dying_priest_within_a_gcd() {
    for seed in [1, 2] {
        let played = play(config(seed, true), stage());
        let start = played.landed().combat;
        let warrior = entity_id(&played, 2, 0);
        let peel = of_class(played.staged_trace(), "Mage")
            .filter(|e| e["kind"] == "ability_decision" && e["actor"]["team"] == 1)
            .find(|e| e["situation"]["play_step"] == "peel")
            .unwrap_or_else(|| panic!("seed {seed}: the Mage never peels"));
        assert!(
            sim_time(peel) - start <= GCD,
            "seed {seed}: the peel came {:.2}s after the stage",
            sim_time(peel) - start
        );
        assert_eq!(peel["outcome"]["ability"], "FrostNova", "seed {seed}");
        assert_eq!(
            peel["situation"]["play"]["ally"].as_u64(),
            Some(entity_id(&played, 1, 1)),
            "seed {seed}: the peel names the Priest"
        );
        let rooted = warrior_rooted(&played)
            .unwrap_or_else(|| panic!("seed {seed}: the Warrior ({warrior}) was never rooted"));
        assert!(rooted <= GCD, "seed {seed}: rooted at {rooted:.2}s");

        // Control: the play withheld. The Mage's own chain has no reason to
        // Nova a Warrior 7yd off, and the Warrior stays free.
        let played = play(config(seed, false), stage());
        assert!(
            of_class(played.staged_trace(), "Mage").all(|e| e["situation"]["play_step"].is_null()),
            "seed {seed}: a peel with the play withheld"
        );
        assert!(
            warrior_rooted(&played).is_none_or(|t| t > GCD),
            "seed {seed}: without the play the Warrior was rooted at {:?}",
            warrior_rooted(&played)
        );
    }
}
