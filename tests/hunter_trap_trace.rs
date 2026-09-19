//! AS-68 — a placed trap names the enemy it was AIMED at.
//!
//! A trap is the only ability in the game whose victim is not its target. It is
//! placed at a POSITION and springs on the first enemy to reach that position,
//! so the outcome alone can never say who the Hunter meant to catch. Without the
//! intended victim on the trace, "did the trap catch who it was aimed at?" is
//! unanswerable from a sweep — which is why the AS-68 diagnosis needed it, and
//! why it must not regress to the bare `None` it used to record.
//!
//! What this pins is the INSTRUMENTATION, not the aim. Whether the Hunter aims
//! well is a balance question measured in
//! `docs/design/balance/2026-09-18-as68-freezing-trap-diagnosis.md`; whether the
//! aim is RECORDED is a property, and this is it.

use std::collections::HashMap;
use std::path::PathBuf;

use arenasim::headless::runner::TraceConfig;
use arenasim::headless::{run_headless_match_with, HeadlessMatchConfig};

fn config(team1: &[&str], team2: &[&str], seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: team1.iter().map(|s| s.to_string()).collect(),
        team2: team2.iter().map(|s| s.to_string()).collect(),
        max_duration_secs: 180.0,
        random_seed: Some(seed),
        ..Default::default()
    }
}

fn run_trace(cfg: HeadlessMatchConfig) -> String {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path: PathBuf = tmp.path().to_path_buf();
    drop(tmp);
    run_headless_match_with(
        cfg,
        true,
        Some(TraceConfig {
            output_path: path.clone(),
        }),
    )
    .expect("headless match");
    std::fs::read_to_string(&path).expect("read trace")
}

/// Every chosen `FreezingTrap` in the trace names a LIVING ENEMY as its
/// intended victim.
///
/// Both halves earn their keep. "Names something" catches the regression to
/// `None`. "An enemy" catches the subtler one: the intended victim is threaded
/// through five call sites, and passing the wrong local at any of them would
/// still produce a populated field — a teammate, or the Hunter itself.
#[test]
fn a_placed_freezing_trap_records_the_enemy_it_was_aimed_at() {
    // Three comps so the assertion spans the branches that place a Freezing
    // Trap: the opportunistic off-target drop, the dip cast, and the legacy
    // midpoint fallback. A comp is only useful here if it actually throws one,
    // which the non-vacuity floor below enforces.
    let comps: [(&[&str], &[&str], u64); 3] = [
        (&["Hunter", "Priest"], &["Rogue", "Priest"], 0),
        (&["Hunter", "Priest"], &["Paladin", "Warrior"], 0),
        (
            &["Hunter", "Priest", "Warrior"],
            &["Mage", "Priest", "Rogue"],
            0,
        ),
    ];

    let mut traps_traced = 0usize;
    for (team1, team2, seed) in comps {
        let body = run_trace(config(team1, team2, seed));

        // Build the entity -> team map from every actor view the trace carries,
        // so the enemy check reads real teams rather than an assumed slot
        // numbering.
        let mut team_of: HashMap<u64, u64> = HashMap::new();
        let mut events: Vec<serde_json::Value> = Vec::new();
        for line in body.lines() {
            let v: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if let Some(actor) = v.get("actor") {
                if let (Some(id), Some(team)) = (
                    actor.get("entity_id").and_then(|x| x.as_u64()),
                    actor.get("team").and_then(|x| x.as_u64()),
                ) {
                    team_of.insert(id, team);
                }
            }
            events.push(v);
        }

        for v in &events {
            let Some(outcome) = v.get("outcome") else {
                continue;
            };
            if outcome.get("ability").and_then(|a| a.as_str()) != Some("FreezingTrap") {
                continue;
            }
            let actor_id = v
                .get("actor")
                .and_then(|a| a.get("entity_id"))
                .and_then(|x| x.as_u64())
                .expect("an ability_decision always carries an actor");
            let actor_team = *team_of.get(&actor_id).expect("actor team");

            let intended = outcome
                .get("target_id")
                .and_then(|x| x.as_u64())
                .unwrap_or_else(|| {
                    panic!(
                        "a chosen Freezing Trap recorded no intended victim \
                         ({team1:?} vs {team2:?}, entity {actor_id}): the trace \
                         can no longer say who the trap was aimed at"
                    )
                });
            let victim_team = *team_of
                .get(&intended)
                .unwrap_or_else(|| panic!("intended victim {intended} never appears in the trace"));
            assert_ne!(
                victim_team, actor_team,
                "Freezing Trap aimed at entity {intended} on the Hunter's OWN \
                 team ({team1:?} vs {team2:?})"
            );
            traps_traced += 1;
        }
    }

    // Non-vacuity: an assertion loop over zero traps passes for the wrong
    // reason. Each comp throws at least one trap today; the floor is the pooled
    // count so a seed shift in one comp does not silently empty the whole probe.
    assert!(
        traps_traced >= 3,
        "only {traps_traced} Freezing Traps traced across three comps — the \
         probe went vacuous, so its pass says nothing about the instrumentation"
    );
}
