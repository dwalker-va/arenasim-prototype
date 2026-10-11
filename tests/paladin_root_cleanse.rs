//! AS-234 — the Paladin Cleanses a root that holds its teammate back, played.
//!
//! `Mage+Priest vs Rogue+Paladin` on BasicArena, seed 2 — the match the user
//! reviewed on AS-180. The Mage Frost Novas the Rogue at ~29.5s and steps out
//! of its swing range; the Paladin stands ~4.6yd beyond Cleanse's range. Before
//! AS-234 the Rogue stood in the Nova for its whole 6s, unable to reach the
//! Mage, while the Paladin healed: roots sat in the maintenance band, which only
//! a healthy team reaches, and nothing walked the Paladin to one. Now the root
//! holds the Rogue back, the walk is short, and the Paladin Cleanses it.
//!
//! Read two ways at once: the read-only per-frame observer, for the Rogue's
//! root window and where it and its target stood; and the decision trace, for
//! the Cleanse.

use arenasim::headless::runner::TraceConfig;
use arenasim::headless::{run_headless_match_observed, FrameObservation, HeadlessMatchConfig};
use arenasim::states::play_match::{AuraType, MELEE_RANGE};
use arenasim::CharacterClass;
use bevy::prelude::{Entity, Vec2};

/// One observed frame of the Rogue: sim time, rooted, and how far its target
/// stood (on the x/z plane).
struct RogueFrame {
    t: f64,
    rooted: bool,
    target_distance: Option<f32>,
}

#[test]
fn a_paladin_cleanses_the_frost_nova_holding_its_rogue_back() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    drop(tmp);
    let mut rogue: Option<Entity> = None;
    let mut frames = Vec::new();
    run_headless_match_observed(
        HeadlessMatchConfig {
            team1: vec!["Mage".into(), "Priest".into()],
            team2: vec!["Rogue".into(), "Paladin".into()],
            map: "BasicArena".into(),
            max_duration_secs: 40.0,
            random_seed: Some(2),
            ..Default::default()
        },
        true,
        Some(TraceConfig {
            output_path: path.clone(),
        }),
        |obs: &FrameObservation| {
            if !obs.gates_open {
                return;
            }
            let id = *rogue.get_or_insert_with(|| {
                *obs.combatants
                    .iter()
                    .find(|(_, c)| c.class == CharacterClass::Rogue && !c.is_pet)
                    .unwrap()
                    .0
            });
            let me = &obs.combatants[&id];
            let xz = |v: bevy::prelude::Vec3| Vec2::new(v.x, v.z);
            frames.push(RogueFrame {
                t: f64::from(obs.sim_time),
                rooted: me.alive && me.aura_types.contains(&AuraType::Root),
                target_distance: me
                    .target
                    .and_then(|t| obs.combatants.get(&t))
                    .map(|t| xz(t.position).distance(xz(me.position))),
            });
        },
    )
    .expect("headless match");
    let gates = frames.first().map_or(0.0, |f| f.t);
    let rogue = rogue.expect("a Rogue").index() as u64;
    let cleanses: Vec<f64> = std::fs::read_to_string(&path)
        .expect("read trace")
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|e| {
            e["actor"]["class"] == "Paladin"
                && e["outcome"]["ability"] == "PaladinCleanse"
                && e["outcome"]["target_id"].as_u64() == Some(rogue)
        })
        .map(|e| e["sim_time"].as_f64().unwrap() + gates)
        .collect();
    let _ = std::fs::remove_file(&path);

    // The Rogue's first root window.
    let start = frames
        .iter()
        .position(|f| f.rooted)
        .expect("vacuous: the Rogue was never rooted");
    let end = start
        + frames[start..]
            .iter()
            .position(|f| !f.rooted)
            .unwrap_or(frames.len() - start);
    let (t0, t1) = (frames[start].t, frames[end.min(frames.len() - 1)].t);

    // The premise: its target stepped out of its swing range while it was
    // rooted — the root held it back.
    let held = frames[start..end]
        .iter()
        .filter(|f| f.target_distance.is_some_and(|d| d > MELEE_RANGE))
        .count();
    assert!(
        held >= 20,
        "vacuous: the Rogue's target was out of its reach on only {held} frames"
    );

    assert!(
        cleanses.iter().any(|&t| (t0..=t1).contains(&t)),
        "no Cleanse on the Rogue during its root ({t0:.2}-{t1:.2}s); Cleanses at {cleanses:?}"
    );
    assert!(
        t1 - t0 <= 3.0,
        "the Rogue stood rooted {:.2}s (from {t0:.2}s)",
        t1 - t0
    );
}
