//! Does `RecentDamage` actually observe damage?
//!
//! It underpins the CC value model's break term and the denial rate `D`, and it
//! has already been wrong once in a way that silently zeroed everything
//! downstream: the first version measured a HEALTH delta, so a unit healed as
//! fast as it was damaged reported zero incoming damage — and healed units are
//! precisely the ones the model cares about.
//!
//! This asserts the tracker is wired and non-trivial, so a future regression
//! fails loudly here instead of quietly turning every denial rate into zero.

use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};

#[test]
fn recent_damage_is_nonzero_while_units_are_being_hit() {
    // Everything needed to answer "is the tracker alive" is already exposed on
    // the observation surface: the enemy's health falls, so damage is landing,
    // and the tracker must see it.
    let mut saw_damage_land = false;
    let mut saw_tracker_report = false;
    let mut max_rate = 0.0f32;
    let mut prev: Option<f32> = None;

    run_headless_match_observed(
        HeadlessMatchConfig {
            team1: vec!["Warlock".into(), "Priest".into()],
            team2: vec!["Priest".into(), "Rogue".into()],
            map: "BasicArena".to_string(),
            random_seed: Some(1),
            max_duration_secs: 60.0,
            ..Default::default()
        },
        true,
        None,
        |frame| {
            let hp: f32 = frame
                .combatants
                .values()
                .filter(|c| c.team == 1 && !c.is_pet)
                .map(|c| c.current_health)
                .sum();
            if let Some(p) = prev {
                if hp < p - 0.5 {
                    saw_damage_land = true;
                }
            }
            prev = Some(hp);

            for c in frame.combatants.values() {
                if c.recent_damage_rate < 0.0 {
                    panic!("RecentDamage component is MISSING from a spawned combatant");
                }
                if c.recent_damage_rate > 0.0 {
                    saw_tracker_report = true;
                    max_rate = max_rate.max(c.recent_damage_rate);
                }
            }
        },
    )
    .expect("match should run");

    assert!(
        saw_damage_land,
        "no damage landed on team 1 at all — the fixture is wrong, not the tracker"
    );
    assert!(
        saw_tracker_report,
        "damage landed but RecentDamage reported a rate of zero for every unit on \
         every frame — the tracker is not observing it, which silently zeroes the \
         CC break term and every denial rate"
    );
    // A real fight sustains meaningful damage; a near-zero peak would mean the
    // tracker is technically alive but reporting noise.
    assert!(
        max_rate > 5.0,
        "peak observed damage rate was {max_rate:.2}/s, which is too low to be a \
         real fight — the tracker is under-counting"
    );
}
