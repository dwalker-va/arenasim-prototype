//! A dying teammate comes before the dispel walk (AS-271's dying-first rule,
//! `class_ai::another_teammate_dying`), in a played match.
//!
//! The healer walks to a teammate held in urgent crowd control that its
//! dispel cannot reach — but not while ANOTHER teammate is dying: the heal is
//! the higher-value global cooldown, and the walk can carry the healer off the
//! dying teammate's heal range and sight. The rule is pinned by its scene
//! tests; this STAGED match (tier 3, `headless::stage`) shows it holds in play,
//! where no seed bank had put the two on the field at once.

mod common;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::AbilityType;
use common::staged::{entity_id, of_class, play, sim_time, Played};

/// Below `urgency_hp_threshold` (0.5).
const DYING: f32 = 0.3;

const WARRIOR: Unit = Unit::new(1, 0);
const MAGE: Unit = Unit::new(1, 1);
const PRIEST: Unit = Unit::new(1, 2);
const ENEMY_MAGE: Unit = Unit::new(2, 0);
const ENEMY_WARRIOR: Unit = Unit::new(2, 1);

/// Whether the enemy Warrior is on the Priest, putting it in PRESSURED — the
/// posture in which `TeamPlan` hands the walk to its team solve's dispel goal.
#[derive(Clone, Copy, Debug)]
enum Priest {
    Free,
    Pressured,
}

fn config(seed: u64, profile: &str, priest: Priest) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Warrior".into(), "Mage".into(), "Priest".into()],
        team2: vec!["Mage".into(), "Warrior".into()],
        map: "BasicArena".into(),
        ai_profile: Some(profile.into()),
        // The enemy is after the Priest when it presses it, otherwise after
        // the polymorphed Mage, far from the Priest.
        team2_kill_target: Some(match priest {
            Priest::Free => 1,
            Priest::Pressured => 2,
        }),
        random_seed: Some(seed),
        max_duration_secs: 10.0,
        ..Default::default()
    }
}

/// The Priest with its Warrior dying beside it, in sight and in heal range;
/// its Mage polymorphed 45yd away, beyond Dispel Magic's 30yd. The enemy Mage
/// stands beyond our Mage; the enemy Warrior is on the Priest, or out past the
/// enemy Mage.
fn stage(priest: Priest) -> Stage {
    let stage = Stage::new()
        .place(PRIEST, -20.0, 0.0)
        .place(WARRIOR, -15.0, 2.0)
        .health(WARRIOR, DYING)
        .place(MAGE, 25.0, 0.0)
        .land(MAGE, AbilityType::Polymorph, ENEMY_MAGE)
        .cooldown(ENEMY_MAGE, AbilityType::Polymorph, 30.0)
        .place(ENEMY_MAGE, 32.0, 10.0);
    match priest {
        Priest::Free => stage.place(ENEMY_WARRIOR, 34.0, -10.0),
        Priest::Pressured => stage
            .place(ENEMY_WARRIOR, -22.0, -2.0)
            .target(ENEMY_WARRIOR, PRIEST),
    }
}

/// When the Warrior is first out of danger, on the combat clock.
fn warrior_recovers(played: &Played) -> f32 {
    let landed = played.landed();
    played
        .staged_frames()
        .find(|f| {
            f.combatants
                .values()
                .find(|c| c.team == 1 && c.slot == 0 && !c.is_pet)
                .is_some_and(|c| c.current_health / c.max_health >= 0.5)
        })
        .map_or(f32::INFINITY, |f| f.sim_time - landed.app)
}

/// While the Warrior is dying the Priest does not walk to its polymorphed
/// Mage — no `DispelChase` — and heals the Warrior first. Once the Warrior is
/// out of danger the walk goes, so it was owed all along. Under both AI
/// profiles, with the Priest free and pressed: the `Legacy` dispel walk runs
/// in both postures under `Legacy` and in FREE under `TeamPlan`, and the team
/// solve's dispel goal in PRESSURED under `TeamPlan`.
#[test]
fn the_priest_heals_its_dying_warrior_before_it_walks_to_dispel() {
    for profile in ["Legacy", "TeamPlan"] {
        for posture in [Priest::Free, Priest::Pressured] {
            for seed in [1, 2] {
                let case = format!("{profile} {posture:?} seed {seed}");
                let played = play(config(seed, profile, posture), stage(posture));
                let warrior = entity_id(&played, 1, 0);
                let mage = entity_id(&played, 1, 1);
                let priest: Vec<&serde_json::Value> = of_class(played.staged_trace(), "Priest")
                    .filter(|e| e["actor"]["team"] == 1)
                    .collect();
                let recovered = warrior_recovers(&played);

                let early = priest
                    .iter()
                    .filter(|e| e["kind"] == "movement_decision" && e["trigger"] == "DispelChase")
                    .map(|e| sim_time(e))
                    .find(|&t| t < recovered);
                assert!(
                    early.is_none(),
                    "{case}: the Priest walked to dispel its Mage at {early:?}s while its \
                     Warrior was still dying (until {recovered:.2}s)"
                );

                let first = priest
                    .iter()
                    .find_map(|e| {
                        let ability = e["outcome"]["ability"].as_str()?;
                        Some((ability, e["outcome"]["target_id"].as_u64()))
                    })
                    .expect("the Priest casts");
                assert_eq!(
                    first.1,
                    Some(warrior),
                    "{case}: the Priest's first cast was {} on {:?}, not a heal on its dying \
                     Warrior",
                    first.0,
                    first.1
                );

                // Not vacuous: the Warrior recovers inside the match, and then
                // the walk to the Mage — owed all along — goes.
                assert!(
                    recovered < 8.0,
                    "{case}: the Warrior never recovered ({recovered})"
                );
                let walked = priest
                    .iter()
                    .find(|e| {
                        e["kind"] == "movement_decision"
                            && e["trigger"] == "DispelChase"
                            && e["target"]["entity_id"] == mage
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "{case}: the Priest never walked to its polymorphed Mage, so the \
                             dispel walk was never owed on this stage"
                        )
                    });
                // ...in the posture the case is about: under `TeamPlan`, a
                // PRESSURED walk is the team solve's dispel goal.
                let expected = match posture {
                    Priest::Free => "free",
                    Priest::Pressured => "pressured",
                };
                assert_eq!(walked["posture"], expected, "{case}");
            }
        }
    }
}
