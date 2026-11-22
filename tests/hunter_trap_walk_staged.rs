//! A staged match (tier 3) for the Hunter's trap walk (AS-498): the walk must
//! not strobe at the throw-range edge.
//!
//! The walk and its inside-range hold meet at throw range
//! (`hunter_dip::trap_setup_range`). With the victim parked right at that
//! edge and its landing unclean — its own partner rooted beside it — a walk
//! that started and ended on one shared line traded the Hunter between the
//! walk and the kite every frame or two. The edge has a band
//! (`TRAP_WALK_EDGE`) and an ended walk re-arms only after `commit_window`,
//! so no `TrapWalk` follows a `TrapWalkEnd` sooner than that.

mod common;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::{Aura, AuraType};
use common::staged::{entity_id, of_class, play, sim_time, Played};

/// The Hunter's `commit_window` in the shipped `movement.ron`.
const COMMIT_WINDOW: f32 = 0.6;

const HUNTER: Unit = Unit::new(1, 0);
const PARTNER: Unit = Unit::new(1, 1);
const OWN_PRIEST: Unit = Unit::new(1, 2);
const ENEMY_ROGUE: Unit = Unit::new(2, 0);
const ENEMY_MAGE: Unit = Unit::new(2, 1);
const ENEMY_DRUID: Unit = Unit::new(2, 2);

fn rooted() -> Aura {
    Aura {
        effect_type: AuraType::Root,
        duration: 300.0,
        magnitude: 1.0,
        ability_name: "Staged root".to_string(),
        break_on_damage_threshold: -1.0,
        ..Default::default()
    }
}

/// Hunter + Warrior + Priest v Rogue + Mage + Druid on `map` under
/// `TeamPlan`. The enemy Druid stands rooted `edge` yards from the Hunter
/// with its Mage rooted beside it, so a trap thrown at the Druid would not
/// land clean; the enemy Rogue chases the Hunter from the side, so the kite
/// keeps moving it; the Hunter's partner attacks the Mage (the walk's
/// teammate rule).
fn edge_match(seed: u64, edge: f32) -> Played {
    let config = HeadlessMatchConfig {
        team1: vec!["Hunter".into(), "Warrior".into(), "Priest".into()],
        team2: vec!["Rogue".into(), "Mage".into(), "Druid".into()],
        map: "BasicArena".into(),
        ai_profile: Some("TeamPlan".into()),
        random_seed: Some(seed),
        max_duration_secs: 40.0,
        ..Default::default()
    };
    let stage = Stage::new()
        .place(HUNTER, 0.0, 0.0)
        .place(PARTNER, edge, 8.0)
        .place(OWN_PRIEST, -12.0, 0.0)
        .place(ENEMY_DRUID, edge, 0.0)
        .place(ENEMY_MAGE, edge, 2.5)
        .place(ENEMY_ROGUE, 0.0, 12.0)
        .clear_auras(ENEMY_ROGUE)
        .aura(ENEMY_DRUID, rooted())
        .aura(ENEMY_MAGE, rooted())
        .target(HUNTER, ENEMY_ROGUE)
        .target(PARTNER, ENEMY_MAGE)
        .target(ENEMY_ROGUE, HUNTER)
        .target(ENEMY_MAGE, PARTNER);
    play(config, stage)
}

/// The Hunter's trap walk events, `(sim time, trigger)`.
fn walk_events(played: &Played) -> Vec<(f32, String)> {
    let hunter = entity_id(played, 1, 0);
    of_class(played.staged_trace(), "Hunter")
        .filter(|e| e["kind"] == "movement_decision")
        .filter(|e| e["actor"]["entity_id"].as_u64() == Some(hunter))
        .filter_map(|e| {
            let trigger = e["trigger"].as_str()?;
            trigger
                .starts_with("TrapWalk")
                .then(|| (sim_time(e), trigger.to_string()))
        })
        .collect()
}

/// How often the Hunter's decisions weighed the trap walk: ability decisions
/// carrying a trap walk note or a pressure-trap hold on Freezing Trap.
fn trap_considered(played: &Played) -> usize {
    let hunter = entity_id(played, 1, 0);
    of_class(played.staged_trace(), "Hunter")
        .filter(|e| e["kind"] == "ability_decision")
        .filter(|e| e["actor"]["entity_id"].as_u64() == Some(hunter))
        .filter(|e| {
            e["candidates"].as_array().is_some_and(|cs| {
                cs.iter().any(|c| {
                    c["ability"] == "FreezingTrap"
                        && c["reason"]["PreconditionUnmet"]["note"]
                            .as_str()
                            .is_some_and(|n| {
                                n.starts_with("trap walk") || n.starts_with("healer trap")
                            })
                })
            })
        })
        .count()
}

#[test]
fn the_walk_does_not_restart_within_its_commit_window_at_the_edge() {
    let mut considered = 0;
    for seed in [1, 2, 3] {
        for edge in [24.5, 25.0, 25.5, 28.0] {
            let played = edge_match(seed, edge);
            considered += trap_considered(&played);
            let events = walk_events(&played);
            for pair in events.windows(2) {
                let ((t_end, end), (t_start, start)) = (&pair[0], &pair[1]);
                if end == "TrapWalkEnd" && start == "TrapWalk" {
                    assert!(
                        t_start - t_end >= COMMIT_WINDOW - 1e-3,
                        "seed {seed}, edge {edge}: a walk restarted {:.3}s after it ended: \
                         {events:?}",
                        t_start - t_end
                    );
                }
            }
        }
    }
    // Not vacuous: the Hunter weighed the trap walk at the edge, decision
    // after decision (about 50 over the bank; a walk that strobes there
    // restarts within the window on every seed).
    assert!(
        considered >= 20,
        "the stage no longer puts the trap walk's choice in front of the Hunter \
         ({considered} decisions)"
    );
}
