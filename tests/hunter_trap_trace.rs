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
    // Trap: the dip cast and opportunistic off-target drop on a healer (both
    // double-healer and single-healer), and the peel on a melee. A comp is only
    // useful here if it actually throws one, which the non-vacuity floor below
    // enforces. None fields a Rogue: a lane trap thrown for a stealthed enemy
    // cannot name it, and records no aim by design (pinned separately below).
    let comps: [(&[&str], &[&str], u64); 3] = [
        (&["Hunter", "Priest"], &["Priest", "Paladin"], 0),
        (&["Hunter", "Priest"], &["Warrior", "Priest"], 4),
        (&["Hunter"], &["Warrior"], 0),
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

/// One traced match that also keeps its `.txt` log, for probes that pair an
/// aim (trace) with an outcome (the `[TRAP] ... triggers on ...` log line).
fn run_trace_and_log(mut cfg: HeadlessMatchConfig) -> (Vec<serde_json::Value>, String) {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("match.txt");
    let trace = dir.path().join("trace.jsonl");
    cfg.output_path = Some(log.to_string_lossy().into_owned());
    run_headless_match_with(
        cfg,
        false,
        Some(TraceConfig {
            output_path: trace.clone(),
        }),
    )
    .expect("headless match");
    let events = std::fs::read_to_string(&trace)
        .expect("read trace")
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    (events, std::fs::read_to_string(&log).expect("read log"))
}

fn hunter_trap_events(events: &[serde_json::Value]) -> impl Iterator<Item = &serde_json::Value> {
    events.iter().filter(|v| {
        v.get("kind").and_then(|k| k.as_str()) == Some("ability_decision")
            && v.pointer("/actor/class").and_then(|c| c.as_str()) == Some("Hunter")
    })
}

/// AS-125 — the opener no longer throws the trap into the melee's lane.
///
/// These are AS-68's three miss comps. In each, the Hunter's team is killing
/// the enemy healer and the enemy Rogue opens from stealth, so at gates-open
/// there was no off-target and the legacy fallback lobbed the trap at the
/// midpoint between the Hunter and the healer 70 yards away — arena centre,
/// where the Rogue ran over it and the healer dispelled it 0.3s later, every
/// seed. Now the trap is HELD: no Freezing Trap is chosen in the opening, and
/// the trace names why — an enemy the Hunter cannot see could spring it, and
/// the healer in view would free it.
#[test]
fn the_opener_holds_the_trap_instead_of_throwing_it_into_the_melee_lane() {
    use arenasim::states::play_match::class_ai::hunter::TRAP_HELD_UNSEEN;

    let comps: [(&[&str], &[&str]); 3] = [
        (&["Hunter", "Priest"], &["Rogue", "Priest"]),
        (&["Hunter", "Warrior"], &["Rogue", "Priest"]),
        (
            &["Hunter", "Priest", "Warrior"],
            &["Mage", "Priest", "Rogue"],
        ),
    ];
    const OPENING_SECS: f64 = 15.0;

    let mut held_unseen = 0usize;
    for (team1, team2) in comps {
        let mut cfg = config(team1, team2, 0);
        cfg.max_duration_secs = 30.0;
        let (events, _) = run_trace_and_log(cfg);
        for v in hunter_trap_events(&events) {
            let t = v.get("sim_time").and_then(|x| x.as_f64()).unwrap();
            if v.pointer("/outcome/ability").and_then(|a| a.as_str()) == Some("FreezingTrap") {
                assert!(
                    t > OPENING_SECS,
                    "{team1:?} vs {team2:?}: Freezing Trap thrown {t:.1}s after the \
                     gates — the opening throw into the melee lane is back"
                );
            }
            held_unseen += v
                .get("candidates")
                .and_then(|c| c.as_array())
                .into_iter()
                .flatten()
                .filter(|c| {
                    c.get("ability").and_then(|a| a.as_str()) == Some("FreezingTrap")
                        && c.pointer("/reason/PreconditionUnmet/note")
                            .and_then(|n| n.as_str())
                            == Some(TRAP_HELD_UNSEEN)
                })
                .count();
        }
    }
    // Non-vacuity: a match in which the trap was never even considered would
    // pass the no-throw assertion for the wrong reason.
    assert!(
        held_unseen > 0,
        "no Freezing Trap was held for an unseen victim across the three comps — \
         the probe no longer exercises the hold"
    );
}

/// AS-125 — the held trap goes to the dispeller, cleanly.
///
/// Hunter+Priest vs Warrior+Priest: the Hunter's team kills the Warrior, so the
/// enemy Priest is the off-target and the one enemy able to free anyone. The
/// Hunter holds the trap until it can land it with the Priest the only enemy
/// near the landing, and then it springs on the Priest. Seeds 3 and 4 each
/// throw one; every trap thrown is asserted aimed at the Priest AND sprung on
/// it.
#[test]
fn the_held_trap_springs_on_the_dispeller_it_was_aimed_at() {
    let mut thrown = 0usize;
    for seed in [3u64, 4] {
        let mut cfg = config(&["Hunter", "Priest"], &["Warrior", "Priest"], seed);
        cfg.max_duration_secs = 60.0;
        let (events, log) = run_trace_and_log(cfg);

        let mut class_of: HashMap<u64, String> = HashMap::new();
        for v in &events {
            if let (Some(id), Some(class)) = (
                v.pointer("/actor/entity_id").and_then(|x| x.as_u64()),
                v.pointer("/actor/class").and_then(|x| x.as_str()),
            ) {
                class_of.insert(id, class.to_string());
            }
        }
        let aims: Vec<String> = hunter_trap_events(&events)
            .filter(|v| {
                v.pointer("/outcome/ability").and_then(|a| a.as_str()) == Some("FreezingTrap")
            })
            .map(|v| {
                let id = v
                    .pointer("/outcome/target_id")
                    .and_then(|x| x.as_u64())
                    .unwrap();
                class_of.get(&id).cloned().unwrap_or_default()
            })
            .collect();
        let sprung: Vec<&str> = log
            .lines()
            .filter(|l| l.contains("Freezing Trap triggers on"))
            .collect();

        assert!(
            aims.iter().all(|c| c == "Priest"),
            "seed {seed}: Freezing Trap aimed at {aims:?}, not the enemy Priest"
        );
        assert_eq!(
            sprung.len(),
            aims.len(),
            "seed {seed}: {} thrown, {} sprung: {sprung:?}",
            aims.len(),
            sprung.len()
        );
        assert!(
            sprung
                .iter()
                .all(|l| l.contains("triggers on Team 2 Priest")),
            "seed {seed}: a trap aimed at the Priest sprang on someone else: {sprung:?}"
        );
        thrown += aims.len();
    }
    assert!(
        thrown >= 2,
        "only {thrown} Freezing Traps thrown across the pinned seeds — the probe went vacuous"
    );
}

/// AS-125 — a lone enemy nobody can free is trapped, and the throw is decided on
/// whoever would actually spring it.
///
/// 1v1, so there is no off-target and every trap is the fallback's lane throw.
/// Against a Priest the only enemy is the Priest, whom nobody can free. Against
/// a Warlock the Felhunter could free its Warlock but nobody frees the
/// Felhunter, and the Felhunter is the one that runs into the lane. Pinned: a
/// trap is thrown in every seed, the trace records it aimed at the enemy it
/// then springs on, and against the Warlock that enemy is the Felhunter and
/// nobody lifts it early.
#[test]
fn a_one_v_one_trap_is_thrown_at_whoever_nobody_can_free() {
    for (enemy, expected) in [("Priest", "Priest"), ("Warlock", "Felhunter")] {
        let mut thrown = 0usize;
        for seed in 0u64..3 {
            let mut cfg = config(&["Hunter"], &[enemy], seed);
            cfg.max_duration_secs = 60.0;
            let (events, log) = run_trace_and_log(cfg);

            // A pet's actor view carries its OWNER's class; its own kind is the
            // pet decision's top-level `pet_type`.
            let mut class_of: HashMap<u64, String> = HashMap::new();
            for v in &events {
                if let Some(id) = v.pointer("/actor/entity_id").and_then(|x| x.as_u64()) {
                    let name = v
                        .get("pet_type")
                        .or_else(|| v.pointer("/actor/class"))
                        .and_then(|x| x.as_str())
                        .unwrap_or_default();
                    class_of.insert(id, name.to_string());
                }
            }
            let aims: Vec<String> = hunter_trap_events(&events)
                .filter(|v| {
                    v.pointer("/outcome/ability").and_then(|a| a.as_str()) == Some("FreezingTrap")
                })
                .map(|v| {
                    let id = v
                        .pointer("/outcome/target_id")
                        .and_then(|x| x.as_u64())
                        .unwrap();
                    class_of.get(&id).cloned().unwrap_or_default()
                })
                .collect();
            assert!(
                !aims.is_empty(),
                "Hunter vs {enemy}, seed {seed}: no Freezing Trap thrown in 60s — \
                 the trap is held against an enemy nobody can free"
            );
            assert!(
                aims.iter().all(|a| a == expected),
                "Hunter vs {enemy}, seed {seed}: trap aimed at {aims:?}, not the {expected}"
            );
            let sprung: Vec<&str> = log
                .lines()
                .filter(|l| l.contains("Freezing Trap triggers on"))
                .collect();
            assert!(
                sprung.iter().all(|l| l.contains(&format!(" {expected} #"))),
                "Hunter vs {enemy}, seed {seed}: a trap sprang on someone other than \
                 the {expected}: {sprung:?}"
            );
            assert!(
                !log.contains("Freezing Trap removed from"),
                "Hunter vs {enemy}, seed {seed}: a trap was freed, but nobody can \
                 free its victim"
            );
            thrown += aims.len();
        }
        assert!(
            thrown >= 3,
            "Hunter vs {enemy}: only {thrown} traps across 3 seeds"
        );
    }
}

/// AS-125 — no trap is ever thrown farther than its configured range.
///
/// Traps are placed at range (the Trap Launcher model), up to the ability's
/// `range` in `abilities.ron`, and never beyond it: the fallback's lane throw
/// used to land ~35yd out against a 30yd range. Asserted on GEOMETRY, per
/// throw: every trap in flight, from where the Hunter stood to where it lands,
/// read off the running match. A trap closer than `TRAP_LAUNCH_MIN_RANGE` is
/// dropped without flying, so the static check below covers those.
#[test]
fn no_trap_lands_beyond_its_range() {
    use arenasim::headless::run_headless_match_observed;
    use arenasim::states::play_match::abilities::AbilityType;
    use arenasim::states::play_match::ability_config::AbilityDefinitions;
    use arenasim::states::play_match::components::TrapType;
    use arenasim::states::play_match::constants::TRAP_LAUNCH_MIN_RANGE;
    use std::collections::BTreeSet;

    let defs = AbilityDefinitions::default();
    let range = |t: TrapType| {
        defs.get_unchecked(&match t {
            TrapType::Freezing => AbilityType::FreezingTrap,
            TrapType::Frost => AbilityType::FrostTrap,
        })
        .range
    };
    for t in [TrapType::Freezing, TrapType::Frost] {
        assert!(
            TRAP_LAUNCH_MIN_RANGE <= range(t),
            "a {t:?} dropped without flying could land beyond its range"
        );
    }

    // Comps whose lane throw opens the match (it landed ~35yd out before the
    // range was enforced), plus the aimed healer trap.
    let comps: [(&[&str], &[&str]); 5] = [
        (&["Hunter"], &["Warrior"]),
        (&["Hunter"], &["Warlock"]),
        (&["Hunter", "Priest"], &["Rogue", "Warrior"]),
        (&["Hunter", "Priest"], &["Shaman", "Rogue"]),
        (&["Hunter", "Priest"], &["Priest", "Paladin"]),
    ];
    let mut seen: BTreeSet<(usize, u64, bevy::prelude::Entity)> = BTreeSet::new();
    let mut kinds: BTreeSet<&str> = BTreeSet::new();
    for (i, (team1, team2)) in comps.iter().enumerate() {
        for seed in 0u64..2 {
            let mut cfg = config(team1, team2, seed);
            cfg.max_duration_secs = 90.0;
            run_headless_match_observed(cfg, true, None, |frame| {
                for launch in &frame.trap_launches {
                    if !seen.insert((i, seed, launch.entity)) {
                        continue;
                    }
                    let flat = |v: bevy::prelude::Vec3| bevy::prelude::Vec3::new(v.x, 0.0, v.z);
                    let thrown = flat(launch.origin).distance(flat(launch.landing));
                    assert!(
                        thrown <= range(launch.trap_type) + 1e-3,
                        "{team1:?} vs {team2:?} seed {seed}: {:?} thrown {thrown:.2}yd \
                         against a {:.0}yd range",
                        launch.trap_type,
                        range(launch.trap_type)
                    );
                    kinds.insert(match launch.trap_type {
                        TrapType::Freezing => "Freezing",
                        TrapType::Frost => "Frost",
                    });
                }
            })
            .expect("headless match");
        }
    }
    // Non-vacuity: a probe that saw no throw proves nothing about range. Only
    // Freezing Trap flies in practice — the Frost Trap peel lands at the
    // Hunter's feet or between it and a melee inside 20yd, so under
    // `TRAP_LAUNCH_MIN_RANGE` — and the static check above covers that drop.
    assert!(
        seen.len() >= 8 && kinds.contains("Freezing"),
        "only {} thrown traps observed ({kinds:?}) — the range probe went vacuous",
        seen.len()
    );
}

/// AS-125 — a lane trap springs on the enemy it was decided on.
///
/// Hunter+Priest vs Warlock+Rogue: the lane throws come late, once the Priest
/// and the Warlock are dead, and each is decided on the Rogue running the lane
/// at the Hunter — never on the Felhunter nearest the landing, which then
/// despawned with its Warlock while the Rogue sprang the trap. Pinned: every
/// throw names the Rogue and springs on it.
///
/// Hunter+Priest vs Rogue+Warrior: at gates-open the Rogue is in stealth, the
/// lane throw is decided on it without seeing it (nobody on its team could free
/// it), so the trace records no aim — and it springs on the Rogue.
#[test]
fn a_lane_trap_springs_on_the_enemy_it_was_decided_on() {
    let class_of = |events: &[serde_json::Value]| {
        let mut class_of: HashMap<u64, String> = HashMap::new();
        for v in events {
            if let Some(id) = v.pointer("/actor/entity_id").and_then(|x| x.as_u64()) {
                let name = v
                    .get("pet_type")
                    .or_else(|| v.pointer("/actor/class"))
                    .and_then(|x| x.as_str())
                    .unwrap_or_default();
                class_of.insert(id, name.to_string());
            }
        }
        class_of
    };
    let throws = |events: &[serde_json::Value]| -> Vec<Option<u64>> {
        hunter_trap_events(events)
            .filter(|v| {
                v.pointer("/outcome/ability").and_then(|a| a.as_str()) == Some("FreezingTrap")
            })
            .map(|v| v.pointer("/outcome/target_id").and_then(|x| x.as_u64()))
            .collect()
    };
    let sprung_on = |log: &str| -> Vec<String> {
        log.lines()
            .filter_map(|l| l.split("Freezing Trap triggers on ").nth(1))
            .map(|rest| rest.split(" —").next().unwrap_or_default().to_string())
            .collect()
    };

    let mut named = 0usize;
    for seed in [4u64, 8, 11] {
        let mut cfg = config(&["Hunter", "Priest"], &["Warlock", "Rogue"], seed);
        cfg.max_duration_secs = 60.0;
        let (events, log) = run_trace_and_log(cfg);
        let classes = class_of(&events);
        let aims: Vec<String> = throws(&events)
            .into_iter()
            .map(|id| {
                id.and_then(|id| classes.get(&id).cloned())
                    .unwrap_or_default()
            })
            .collect();
        let sprung = sprung_on(&log);
        assert!(
            aims.iter().all(|a| a == "Rogue"),
            "seed {seed}: lane trap decided on {aims:?}, not the Rogue"
        );
        assert_eq!(
            sprung.len(),
            aims.len(),
            "seed {seed}: {} thrown, {} sprung: {sprung:?}",
            aims.len(),
            sprung.len()
        );
        assert!(
            sprung.iter().all(|v| v.contains(" Rogue #")),
            "seed {seed}: a trap decided on the Rogue sprang on {sprung:?}"
        );
        named += aims.len();
    }
    assert!(
        named >= 2,
        "only {named} lane traps across the pinned seeds"
    );

    let mut cfg = config(&["Hunter", "Priest"], &["Rogue", "Warrior"], 0);
    cfg.max_duration_secs = 30.0;
    let (events, log) = run_trace_and_log(cfg);
    let aims = throws(&events);
    assert_eq!(
        aims,
        vec![None],
        "the opening lane trap against a stealthed Rogue records no aim"
    );
    let sprung = sprung_on(&log);
    assert!(
        sprung.len() == 1 && sprung[0].contains(" Rogue #"),
        "the opening lane trap sprang on {sprung:?}, not the Rogue"
    );
}

/// AS-179 — the Hunter's own shot does not break its own trap.
///
/// Hunter+Priest vs Warlock+Rogue: once the Priest and the Warlock are dead the
/// lane trap goes on the Rogue, the Hunter's own target — allowed, because a
/// Hunter alone holds fire on a trapped enemy. But an Aimed Shot begun as the
/// Rogue ran the lane used to land just after the trap sprang and break it
/// (seeds 3, 8 and 11 before AS-166's Flare changed how those fights run: the
/// trap broke from the Hunter's own Aimed Shot 1.6-2.5s in). The Hunter now
/// holds a shot that would land after its own trap catches the target. Pinned
/// seeds that reach that ending: in each the trap springs on the Rogue, is
/// never broken, and the trace shows the hold firing.
#[test]
fn the_hunters_own_shot_does_not_break_its_trap() {
    use arenasim::states::play_match::class_ai::hunter_dip::OWN_TRAP_WOULD_BREAK;

    for seed in [17u64, 58, 61] {
        let mut cfg = config(&["Hunter", "Priest"], &["Warlock", "Rogue"], seed);
        cfg.max_duration_secs = 60.0;
        let (events, log) = run_trace_and_log(cfg);
        let sprung = log
            .lines()
            .filter(|l| l.contains("Freezing Trap triggers on Team 2 Rogue"))
            .count();
        assert!(sprung >= 1, "seed {seed}: no trap sprang on the Rogue");
        let broke: Vec<&str> = log
            .lines()
            .filter(|l| l.contains("Freezing Trap broke from damage"))
            .collect();
        assert!(broke.is_empty(), "seed {seed}: the trap broke: {broke:?}");
        let held = hunter_trap_events(&events)
            .flat_map(|v| v["candidates"].as_array().cloned().unwrap_or_default())
            .filter(|c| {
                c.pointer("/reason/PreconditionUnmet/note")
                    .and_then(|n| n.as_str())
                    == Some(OWN_TRAP_WOULD_BREAK)
            })
            .count();
        assert!(
            held > 0,
            "seed {seed}: the own-trap hold never fired, so the pass says nothing"
        );
    }
}

/// AS-179 — against a lone Priest the opener is Serpent Sting, and the Hunter
/// wins.
///
/// Nothing a Priest does can stop an Aimed Shot, so there is time for both
/// and the sting goes first: it ticks a GCD longer, and the Priest spends its
/// early GCDs dispelling it rather than Mind Blasting the Hunter.
/// With Aimed Shot first the Hunter lost these seeds (3, 7 and 13 of the 7 in
/// 50 it lost).
#[test]
fn against_a_lone_priest_the_opener_is_serpent_sting() {
    for seed in [3u64, 7, 13] {
        let (events, log) = run_trace_and_log(config(&["Hunter"], &["Priest"], seed));
        let opener = hunter_trap_events(&events)
            .filter_map(|v| v.pointer("/outcome/ability").and_then(|a| a.as_str()))
            .find(|a| *a == "AimedShot" || *a == "SerpentSting");
        assert_eq!(opener, Some("SerpentSting"), "seed {seed}");
        assert!(
            log.lines()
                .any(|l| l.contains("Team 2 Priest #1 has been eliminated")),
            "seed {seed}: the Hunter did not kill the Priest"
        );
    }
}

/// AS-125 — with the Rogue on the Hunter, the trap goes on the enemy Priest.
///
/// Hunter+Priest vs Rogue+Priest: the Hunter is killing the Priest when the
/// Rogue closes on it. Freezing Trap is thrown at the Priest (the trace names
/// it), springs on the Priest, and the Hunter moves its target to the Rogue —
/// every shot holds fire on a trapped enemy, so staying on the Priest would
/// idle it. Pinned seeds where the Priest reaches the landing as it arms.
#[test]
fn a_pressured_hunter_traps_the_enemy_healer_and_turns_on_the_melee() {
    let mut caught = 0usize;
    for seed in [1u64, 3, 4] {
        let mut cfg = config(&["Hunter", "Priest"], &["Rogue", "Priest"], seed);
        cfg.max_duration_secs = 60.0;
        let (events, log) = run_trace_and_log(cfg);
        let class_of: HashMap<u64, String> = events
            .iter()
            .filter_map(|v| {
                Some((
                    v.pointer("/actor/entity_id")?.as_u64()?,
                    v.pointer("/actor/class")?.as_str()?.to_string(),
                ))
            })
            .collect();
        let decisions: Vec<&serde_json::Value> = hunter_trap_events(&events).collect();
        let throw = decisions
            .iter()
            .position(|v| {
                v.pointer("/outcome/ability").and_then(|a| a.as_str()) == Some("FreezingTrap")
            })
            .unwrap_or_else(|| panic!("seed {seed}: no Freezing Trap thrown"));
        let aimed = decisions[throw]
            .pointer("/outcome/target_id")
            .and_then(|x| x.as_u64())
            .and_then(|id| class_of.get(&id).cloned());
        assert_eq!(aimed.as_deref(), Some("Priest"), "seed {seed}: aimed at");
        let next_target = decisions[throw + 1..]
            .iter()
            .find_map(|v| v.pointer("/target/class").and_then(|c| c.as_str()));
        assert_eq!(
            next_target,
            Some("Rogue"),
            "seed {seed}: the Hunter's next target"
        );
        let sprung = log
            .lines()
            .find_map(|l| l.split("Freezing Trap triggers on ").nth(1))
            .unwrap_or_default();
        assert!(
            sprung.contains(" Priest #"),
            "seed {seed}: the healer trap sprang on {sprung:?}"
        );
        caught += 1;
    }
    assert_eq!(caught, 3);
}

/// AS-125 — the healer trap survives a configured kill target.
///
/// The graphical client configures a kill target by default, and target
/// acquisition re-forces a Hunter onto it every tick. Here the Hunter's team is
/// told to kill the enemy Priest (`team1_kill_target: 1`) while the Rogue runs
/// the Hunter down, so the pressure trap goes on the Hunter's own kill target.
/// The Hunter must leave it for the Rogue at the throw — traced as a target
/// acquisition, so the `target switches` recipe in CLAUDE.md shows it — and
/// stay off it while it is frozen, deciding on the Rogue instead of holding
/// fire on its own trap. Whether a shot then lands is the dead zone's call:
/// since Flare reveals the Rogue before it opens, it reaches the Hunter
/// unstunned and in nearly every seed pins it inside 8 yards for the whole
/// freeze (1 seed of 320 scanned has the Hunter shooting it). Pinned seeds
/// where the trap springs on the Priest.
#[test]
fn a_healer_trap_on_the_kill_target_turns_the_hunter_onto_the_melee() {
    const GATES_OPEN: f64 = 10.0;
    for seed in [6u64, 8, 9] {
        let mut cfg = config(&["Hunter", "Priest"], &["Rogue", "Priest"], seed);
        cfg.team1_kill_target = Some(1);
        cfg.max_duration_secs = 60.0;
        let (events, log) = run_trace_and_log(cfg);
        // Target acquisition events come from primaries only: a pet's actor
        // view carries its owner's class.
        let id_of = |team: u64, class: &str| {
            events
                .iter()
                .filter(|v| v.get("kind").and_then(|k| k.as_str()) == Some("target_acquisition"))
                .find_map(|v| {
                    let a = v.get("actor")?;
                    (a.get("team")?.as_u64()? == team && a.get("class")?.as_str()? == class)
                        .then(|| a.get("entity_id")?.as_u64())?
                })
                .unwrap_or_else(|| panic!("seed {seed}: no team {team} {class} in the trace"))
        };
        let (hunter, priest, rogue) = (id_of(1, "Hunter"), id_of(2, "Priest"), id_of(2, "Rogue"));

        let throw = hunter_trap_events(&events)
            .find(|v| {
                v.pointer("/outcome/ability").and_then(|a| a.as_str()) == Some("FreezingTrap")
            })
            .unwrap_or_else(|| panic!("seed {seed}: no Freezing Trap thrown"));
        assert_eq!(
            throw.pointer("/outcome/target_id").and_then(|x| x.as_u64()),
            Some(priest),
            "seed {seed}: the trap was not aimed at the Priest"
        );
        let thrown_at = throw["sim_time"].as_f64().unwrap();
        let sprung_at = log
            .lines()
            .find(|l| l.contains("Freezing Trap triggers on Team 2 Priest"))
            .and_then(|l| {
                l.trim_start_matches('[')
                    .split('s')
                    .next()?
                    .trim()
                    .parse::<f64>()
                    .ok()
            })
            .unwrap_or_else(|| panic!("seed {seed}: the trap never sprang on the Priest"))
            - GATES_OPEN;

        // The switch, traced as a target acquisition between throw and spring.
        let switched = events.iter().any(|v| {
            v.get("kind").and_then(|k| k.as_str()) == Some("target_acquisition")
                && v.pointer("/actor/entity_id").and_then(|x| x.as_u64()) == Some(hunter)
                && v.get("changed").and_then(|c| c.as_bool()) == Some(true)
                && v.get("previous_target").and_then(|x| x.as_u64()) == Some(priest)
                && v.get("new_target").and_then(|x| x.as_u64()) == Some(rogue)
                && (thrown_at..=sprung_at).contains(&v["sim_time"].as_f64().unwrap())
        });
        assert!(
            switched,
            "seed {seed}: no traced Priest -> Rogue switch between the throw at \
             {thrown_at:.2}s and the spring at {sprung_at:.2}s"
        );

        // While the Priest is frozen the Hunter neither decides on it (the
        // kill-target re-force is what used to pin it there) nor idles.
        let frozen = (sprung_at + 0.1)..(sprung_at + 7.9);
        let on_priest = hunter_trap_events(&events)
            .filter(|v| frozen.contains(&v["sim_time"].as_f64().unwrap()))
            .filter(|v| v.pointer("/target/entity_id").and_then(|x| x.as_u64()) == Some(priest))
            .count();
        assert_eq!(
            on_priest, 0,
            "seed {seed}: the Hunter decided on its frozen kill target {on_priest} times"
        );
        let on_rogue = hunter_trap_events(&events)
            .filter(|v| frozen.contains(&v["sim_time"].as_f64().unwrap()))
            .filter(|v| v.pointer("/target/entity_id").and_then(|x| x.as_u64()) == Some(rogue))
            .count();
        assert!(
            on_rogue > 0,
            "seed {seed}: the Hunter never decided on the Rogue while the Priest was frozen"
        );
    }
}

/// AS-125 — no Aimed Shot is begun while the enemy Rogue is in stealth.
///
/// A stealthed Rogue opens in melee range and Kicks a 2.5s cast. The opener
/// already put Serpent Sting first while an enemy is hidden; the sting ends
/// the opener, so the rotation's own Aimed Shot must ask the same question or
/// it is begun into the still-hidden Rogue and Kicked. Every Aimed Shot the
/// Hunter begins comes after the Rogue's first action in the log, and the
/// trace shows the hidden-enemy hold actually firing.
#[test]
fn no_aimed_shot_is_begun_while_a_rogue_is_hidden() {
    use arenasim::states::play_match::class_ai::hunter::AIMED_SHOT_HELD_HIDDEN;

    let log_time = |l: &str| {
        l.trim_start_matches('[')
            .split('s')
            .next()
            .and_then(|t| t.trim().parse::<f64>().ok())
    };
    let mut held = 0usize;
    for (team2, seed) in [(["Rogue", "Priest"], 0u64), (["Rogue", "Paladin"], 0)] {
        let mut cfg = config(&["Hunter", "Priest"], &team2, seed);
        cfg.max_duration_secs = 40.0;
        let (events, log) = run_trace_and_log(cfg);
        let rogue_acts = log
            .lines()
            .find(|l| {
                l.contains("] [CAST] Team 2 Rogue") || l.contains("Team 2 Rogue #1 is revealed")
            })
            .and_then(log_time)
            .unwrap_or_else(|| panic!("{team2:?}: the Rogue never acts"));
        for l in log
            .lines()
            .filter(|l| l.contains("Team 1 Hunter #1 begins casting Aimed Shot"))
        {
            let t = log_time(l).unwrap();
            assert!(
                t >= rogue_acts,
                "{team2:?}: Aimed Shot begun at {t:.2}s, before the hidden Rogue acted at {rogue_acts:.2}s"
            );
        }
        held += hunter_trap_events(&events)
            .flat_map(|v| {
                v.get("candidates")
                    .and_then(|c| c.as_array())
                    .into_iter()
                    .flatten()
            })
            .filter(|c| {
                c.get("ability").and_then(|a| a.as_str()) == Some("AimedShot")
                    && c.pointer("/reason/PreconditionUnmet/note")
                        .and_then(|n| n.as_str())
                        == Some(AIMED_SHOT_HELD_HIDDEN)
            })
            .count();
    }
    assert!(
        held > 0,
        "the hidden-enemy hold never fired — the probe went vacuous"
    );
}

/// AS-166 — with the Rogue the kill target, Flare finds it and the trap takes
/// its partner.
///
/// The case from the user's review of AS-125: Hunter+Warrior vs
/// Rogue+Shaman, both kill targets at slot 0 (the graphical client's default),
/// so the Hunter's team is told to kill the Rogue. Nobody on the Rogue's team
/// could free a trap on it, so before Flare the opening lane trap caught and
/// revealed it every seed, and the team broke the trap as it converged. Now
/// the lane trap is held for an unseen kill target (traced), a Flare lights the
/// Rogue before it can open, and the trap springs on the Shaman instead.
#[test]
fn flare_reveals_the_kill_target_rogue_and_the_trap_takes_its_partner() {
    use arenasim::states::play_match::class_ai::hunter::TRAP_HELD_UNSEEN_KILL_TARGET;

    let mut held = 0usize;
    for seed in [0u64, 1, 2] {
        let mut cfg = config(&["Hunter", "Warrior"], &["Rogue", "Shaman"], seed);
        cfg.team1_kill_target = Some(0);
        cfg.team2_kill_target = Some(0);
        cfg.max_duration_secs = 60.0;
        let (events, log) = run_trace_and_log(cfg);
        assert!(
            log.lines()
                .any(|l| l.contains("[STEALTH] Team 2 Rogue #1 is revealed by Flare")),
            "seed {seed}: the Flare did not find the Rogue"
        );
        let opened = log.lines().any(|l| {
            l.contains("[CAST] Team 2 Rogue #1 uses Cheap Shot on")
                || l.contains("[CAST] Team 2 Rogue #1 uses Ambush on")
        });
        assert!(!opened, "seed {seed}: the Rogue opened from stealth");
        let sprung: Vec<&str> = log
            .lines()
            .filter_map(|l| l.split("Freezing Trap triggers on ").nth(1))
            .collect();
        assert!(
            !sprung.iter().any(|v| v.starts_with("Team 2 Rogue")),
            "seed {seed}: a trap sprang on the kill-target Rogue: {sprung:?}"
        );
        assert!(
            sprung.iter().any(|v| v.starts_with("Team 2 Shaman")),
            "seed {seed}: no trap sprang on the Shaman: {sprung:?}"
        );
        held += hunter_trap_events(&events)
            .flat_map(|v| v["candidates"].as_array().cloned().unwrap_or_default())
            .filter(|c| {
                c.pointer("/reason/PreconditionUnmet/note")
                    .and_then(|n| n.as_str())
                    == Some(TRAP_HELD_UNSEEN_KILL_TARGET)
            })
            .count();
    }
    assert!(
        held > 0,
        "the lane trap was never held for an unseen kill target — the probe went vacuous"
    );
}
