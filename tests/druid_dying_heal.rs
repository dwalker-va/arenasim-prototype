//! AS-210 — the Druid heals a dying teammate before it casts damage or
//! utility.
//!
//! The played match the defect was found in: Hunter+Paladin vs Mage+Druid on
//! Nagrand (`TeamPlan`), seed 2. At about 94s of combat a Paladin crit drops
//! the Mage from 57% to 40%, behind a pillar edge from the Druid. The medic
//! walk starts at once, and sight returns within a fraction of a second — but
//! the Druid had spent the global cooldown on a Moonfire at the Paladin in the
//! frames between, so the Mage's heal waited 1.5s and it was never healed
//! before the match ended. Every Druid spell is an instant on the global
//! cooldown, so the medic walk's cast deferral, which holds movement-locking
//! casts, held nothing for it; the Druid now holds its damage and utility
//! itself while the walk's teammate is out of reach.
//!
//! The rule itself is pinned case by case in `class_ai/druid.rs`
//! (`reach_tests`); this is the played match.

use arenasim::headless::runner::TraceConfig;
use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};
use arenasim::states::match_config::CharacterClass;

/// The Druid's heals.
const HEALS: [&str; 3] = ["Rejuvenation", "Lifebloom", "Swiftmend"];

fn repro_config() -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Hunter".into(), "Paladin".into()],
        team2: vec!["Mage".into(), "Druid".into()],
        map: "PillaredArena".to_string(),
        ai_profile: Some("TeamPlan".to_string()),
        random_seed: Some(2),
        ..Default::default()
    }
}

/// The played match: the Mage's `(sim time, HP fraction)` on every frame after
/// the gates, its trace entity id, and the decision trace's events.
struct Played {
    mage_hp: Vec<(f32, f32)>,
    mage_id: u64,
    trace: Vec<serde_json::Value>,
}

fn play() -> Played {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    drop(tmp);
    let mut mage_hp = Vec::new();
    let mut mage_id = None;
    run_headless_match_observed(
        repro_config(),
        true,
        Some(TraceConfig {
            output_path: path.clone(),
        }),
        |frame| {
            if !frame.gates_open {
                return;
            }
            let (entity, mage) = frame
                .combatants
                .iter()
                .find(|(_, c)| c.team == 2 && !c.is_pet && c.class == CharacterClass::Mage)
                .expect("the Mage");
            mage_id = Some(entity.index() as u64);
            if mage.alive {
                mage_hp.push((frame.sim_time, mage.current_health / mage.max_health));
            }
        },
    )
    .expect("headless match");
    let body = std::fs::read_to_string(&path).expect("read trace");
    let _ = std::fs::remove_file(&path);
    Played {
        mage_hp,
        mage_id: mage_id.expect("the gates opened"),
        trace: body
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect(),
    }
}

/// From the moment the medic walk starts toward the 40% Mage, the Druid's
/// first cast is a heal on the Mage — not a Moonfire — and the Mage is healed
/// within two global cooldowns of dropping below the urgency threshold.
#[test]
fn the_repro_druid_heals_its_dying_mage_first() {
    let played = play();
    let druid = |e: &&serde_json::Value| e["actor"]["class"] == "Druid";

    // The scenario still happens: the medic walk to the Mage at/after 94s.
    let walk = played
        .trace
        .iter()
        .filter(druid)
        .find(|e| {
            e["kind"] == "movement_decision"
                && e["trigger"] == "SeekLos"
                && e["target"]["entity_id"] == played.mage_id
                && e["sim_time"].as_f64().unwrap() >= 94.0
        })
        .expect(
            "the repro no longer walks the Druid to its dying Mage at 94s — re-seed this probe",
        );
    let walk_at = walk["sim_time"].as_f64().unwrap();

    let first_cast = played
        .trace
        .iter()
        .filter(druid)
        .find(|e| {
            e["kind"] == "ability_decision"
                && e["sim_time"].as_f64().unwrap() >= walk_at
                && e["outcome"]["ability"].is_string()
        })
        .expect("the Druid casts after the walk starts");
    let (ability, target) = (
        first_cast["outcome"]["ability"].as_str().unwrap(),
        &first_cast["outcome"]["target_id"],
    );
    assert!(
        HEALS.contains(&ability) && *target == played.mage_id,
        "the Druid's first cast after the medic walk started at {walk_at:.2}s was {ability} \
         on {target}, not a heal on the dying Mage ({})",
        played.mage_id
    );

    // The Mage below the threshold at the walk's start, and healed soon after.
    let dropped = played
        .mage_hp
        .iter()
        .position(|&(_, hp)| hp < 0.45)
        .expect("the Mage drops below 45%");
    let (dropped_at, low) = played.mage_hp[dropped];
    let healed = played.mage_hp[dropped..]
        .iter()
        .find(|&&(_, hp)| hp > low)
        .map(|&(t, _)| t - dropped_at);
    assert!(
        healed.is_some_and(|after| after <= 2.0 * 1.5),
        "the Mage at {:.0}% was healed {healed:?}s after it dropped, not within two global \
         cooldowns",
        low * 100.0
    );
}
