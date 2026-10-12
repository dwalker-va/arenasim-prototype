//! AS-210 — the Druid heals a dying teammate before it casts damage or
//! utility, and holds its damage and utility while the medic walk has not
//! reached that teammate.
//!
//! The defect was found in a played match: a crit dropped the Druid's Mage
//! below the urgency line behind a pillar edge, the medic walk started, and
//! the Druid spent the global cooldown on a Moonfire in the frames before
//! sight returned — so the Mage's heal waited 1.5s, and it was never healed.
//! Every Druid spell is an instant on the global cooldown, so the walk's cast
//! deferral, which holds movement-locking casts, held nothing for it.
//!
//! The rule is pinned case by case in `class_ai/druid.rs` (`reach_tests`).
//! This is the played guard: a STAGED match (tier 3, `headless::stage`) puts
//! the scene on the field by construction, so no seed can stop reaching it.

mod common;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::AbilityType;
use common::staged::{entity_id, of_class, play, sim_time, Played};

/// Below `urgency_hp_threshold` (0.5), above `DRUID_EMERGENCY_HP` (0.45): the
/// Mage is dying, but not so low that Swiftmend's steps take the turn first.
const MAGE_HP: f32 = 0.47;

const MAGE: Unit = Unit::new(2, 0);
const DRUID: Unit = Unit::new(2, 1);
const WARRIOR: Unit = Unit::new(1, 0);
const PALADIN: Unit = Unit::new(1, 1);

/// The Druid's heals.
const HEALS: [&str; 3] = ["Rejuvenation", "Lifebloom", "Swiftmend"];

/// One frame of the fixed 60Hz clock.
const FRAME: f32 = 1.0 / 60.0;

fn config(seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Warrior".into(), "Paladin".into()],
        team2: vec!["Mage".into(), "Druid".into()],
        map: "PillaredArena".into(),
        // The Druid is the enemy's kill target, the Paladin the Druid's.
        team1_kill_target: Some(1),
        team2_kill_target: Some(1),
        random_seed: Some(seed),
        max_duration_secs: 10.0,
        ..Default::default()
    }
}

/// The stage, round the pillar at (40, 20) (apothem 5.5yd): the Druid 12yd
/// west of its centre, the dying Mage 12yd east — 24yd apart, in heal range,
/// out of sight. The enemy Paladin stands 25yd from the Druid, in sight and in
/// Moonfire's range; the Warrior is across the bowl, 90yd off.
///
/// The enemies are after the Druid, so the Mage is nobody's target and no
/// one's heal FOCUS: the Druid is, and it carries its own Rejuvenation and a
/// full Lifebloom stack, so the focus steps (5-6) have nothing to add. The
/// Mage's heal can then only come from step 7 (the dying teammate's) — which
/// is what puts it ahead of Moonfire (step 10).
fn stage() -> Stage {
    let stage = Stage::new()
        .place(DRUID, 28.0, 23.0)
        .place(MAGE, 52.0, 23.0)
        .health(MAGE, MAGE_HP)
        .place(PALADIN, 25.0, 45.0)
        .place(WARRIOR, -45.0, -35.0)
        .land(DRUID, AbilityType::Rejuvenation, DRUID);
    (0..3).fold(stage, |s, _| s.land(DRUID, AbilityType::Lifebloom, DRUID))
}

/// What the Druid did from the gates until its first heal on the Mage.
struct Walk {
    /// When the medic walk to the Mage started (combat clock).
    started: f32,
    /// The Druid's casts from the gates, `(combat time, ability, target)`, up
    /// to and including its first heal on the Mage.
    casts: Vec<(f32, String, u64)>,
    /// The Druid's turns on which Moonfire was held for the walk.
    held: Vec<f32>,
}

fn walk(played: &Played) -> Walk {
    let mage = entity_id(played, 2, 0);
    let druid: Vec<&serde_json::Value> = of_class(played.staged_trace(), "Druid").collect();
    let started = druid
        .iter()
        .find(|e| {
            e["kind"] == "movement_decision"
                && e["trigger"] == "SeekLos"
                && e["target"]["entity_id"] == mage
        })
        .map(|e| sim_time(e))
        .expect("the Druid walks to its dying Mage");
    let mut casts = Vec::new();
    let mut held = Vec::new();
    for e in druid.iter().filter(|e| e["kind"] == "ability_decision") {
        let moonfire_held = e["candidates"].as_array().into_iter().flatten().any(|c| {
            c["ability"] == "Moonfire"
                && c["reason"]["PreconditionUnmet"]["note"]
                    .as_str()
                    .is_some_and(|n| n.contains("medic walk"))
        });
        if moonfire_held {
            held.push(sim_time(e));
        }
        if let Some(ability) = e["outcome"]["ability"].as_str() {
            let target = e["outcome"]["target_id"].as_u64().unwrap_or(u64::MAX);
            casts.push((sim_time(e), ability.to_string(), target));
            if HEALS.contains(&ability) && target == mage {
                break;
            }
        }
    }
    Walk {
        started,
        casts,
        held,
    }
}

/// From the gates the medic walk runs to the dying Mage round the pillar. On
/// every turn of it the Druid holds Moonfire — castable at the Paladin in
/// sight but for the hold — and casts nothing; on the first turn the hold lets
/// go, the Mage is in reach and the Druid heals it. So the global cooldown the
/// hold kept free goes to the heal, and no damage comes first. Over three
/// seeds: the stage holds the scene by construction, and the seed only moves
/// the rolls.
#[test]
fn the_druid_holds_its_damage_for_the_walk_and_heals_its_dying_mage_first() {
    for seed in [1, 2, 3] {
        let played = play(config(seed), stage());
        let mage = entity_id(&played, 2, 0);
        let walk = walk(&played);

        assert!(
            walk.started < 0.1,
            "seed {seed}: the walk starts at the gates, not at {:.2}s",
            walk.started
        );
        let first = walk.casts.first().expect("the Druid casts after the gates");
        assert!(
            HEALS.contains(&first.1.as_str()) && first.2 == mage,
            "seed {seed}: the Druid's first cast after the gates was {} on {} at {:.2}s, not \
             a heal on its dying Mage ({mage}): {:?}",
            first.1,
            first.2,
            first.0,
            walk.casts
        );
        // Not vacuous: a walk of a second or more, every turn of it holding a
        // castable Moonfire.
        assert!(
            walk.held.len() >= 60,
            "seed {seed}: Moonfire was held for the walk on {} turns — this stage no longer \
             walks the Druid round the pillar with a Moonfire to hold",
            walk.held.len()
        );
        let released = walk.held.last().copied().unwrap_or_default() + FRAME;
        assert!(
            (first.0 - released).abs() < FRAME / 2.0,
            "seed {seed}: the hold let go at {released:.2}s but the Mage's heal came at \
             {:.2}s",
            first.0
        );
    }
}
