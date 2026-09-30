//! Every combatant enters the gates at its Classic starting resource, observed
//! in the real headless match flow: mana and energy full, rage EMPTY.
//!
//! Rage lives in `current_mana`, so any path that "refills mana" hands a
//! Warrior a full rage bar. Two did — the countdown's every-frame refill
//! (`match_flow::update_countdown`) and `Combatant::apply_equipment` — and every
//! Warrior opened the match at 100 rage. Both now go through
//! `Combatant::pre_combat_resource`, keyed on `ResourceType`. This pins the
//! outcome at the frame the gates open, where the unit tests of either seam
//! alone cannot see the other.

use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};
use arenasim::states::play_match::components::class_base_stats;
use arenasim::states::play_match::components::ResourceType;
use arenasim::CharacterClass;

/// `(class, current, max)` for every non-pet combatant on one frame.
type Resources = Vec<(CharacterClass, f32, f32)>;

struct Watch {
    /// The last countdown frame: the countdown's hold has run, the AI has not.
    last_countdown: Option<Resources>,
    /// The first gates-open frame, observed AFTER that frame's AI — a caster
    /// may already have spent mana, but no Warrior can have earned rage.
    gates_open: Option<Resources>,
    /// Highest rage any Warrior reached after the gates — proves the
    /// observation reads the live bar, not a constant.
    peak_rage: f32,
}

fn watch(team1: &[&str], team2: &[&str], seed: u64) -> Watch {
    let config = HeadlessMatchConfig {
        team1: team1.iter().map(|c| c.to_string()).collect(),
        team2: team2.iter().map(|c| c.to_string()).collect(),
        random_seed: Some(seed),
        max_duration_secs: 40.0,
        ..Default::default()
    };
    let mut w = Watch {
        last_countdown: None,
        gates_open: None,
        peak_rage: 0.0,
    };
    run_headless_match_observed(config, true, None, |frame| {
        let resources: Resources = frame
            .combatants
            .values()
            .filter(|c| !c.is_pet)
            .map(|c| (c.class, c.current_mana, c.max_mana))
            .collect();
        if !frame.gates_open {
            w.last_countdown = Some(resources);
        } else {
            if w.gates_open.is_none() {
                w.gates_open = Some(resources.clone());
            }
            for (class, current, _) in resources {
                if class == CharacterClass::Warrior {
                    w.peak_rage = w.peak_rage.max(current);
                }
            }
        }
    })
    .expect("match runs");
    w
}

fn assert_pre_combat_levels(label: &str, resources: &Resources) {
    for &(class, current, max) in resources {
        match class_base_stats(class).resource_type {
            ResourceType::Rage => assert_eq!(current, 0.0, "{label}: {class:?} rage"),
            ResourceType::Mana | ResourceType::Energy => {
                assert_eq!(current, max, "{label}: {class:?} resource")
            }
        }
    }
}

/// All nine classes across two matches, a Warrior on each side of both.
#[test]
fn gates_open_with_rage_empty_and_mana_and_energy_full() {
    for (team1, team2) in [
        (
            &["Warrior", "Rogue", "Mage"][..],
            &["Warrior", "Priest", "Warlock"][..],
        ),
        (
            &["Warrior", "Hunter", "Paladin"][..],
            &["Warrior", "Shaman", "Druid"][..],
        ),
    ] {
        let w = watch(team1, team2, 1);
        let countdown = w.last_countdown.expect("match has a countdown");
        let open = w.gates_open.expect("gates open");
        assert_eq!(
            open.len(),
            6,
            "{team1:?} v {team2:?}: all six combatants seen"
        );
        assert_pre_combat_levels("last countdown frame", &countdown);
        for &(class, rage, _) in &open {
            if class == CharacterClass::Warrior {
                assert_eq!(rage, 0.0, "first gates-open frame: Warrior rage");
            }
        }
        assert!(
            w.peak_rage > 0.0,
            "{team1:?} v {team2:?}: a Warrior never earned rage after the gates"
        );
    }
}
