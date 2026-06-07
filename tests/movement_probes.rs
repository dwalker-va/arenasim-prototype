//! U2 — Behavior-probe harness: probe-support helpers + harness self-tests.
//!
//! This file is the home of the movement behavior probes (healer posture
//! work, U5–U8). What lands here in U2 is the *harness*:
//!
//! - position-timeline collection via `run_headless_match_observed`'s
//!   read-only per-frame observer,
//! - reusable KPI helpers over `(sim_time, position)` sample slices
//!   (`path_length`, `time_within_range_of`, `separation_gained_during`),
//! - a non-vacuity assertion helper (`assert_min_occurrences`) so
//!   window-conditional probes fail loudly instead of passing over an
//!   empty window set,
//! - self-tests proving the harness works, headlined by the load-bearing
//!   NON-PERTURBATION test: an observed run must return a `MatchResult`
//!   identical to an unobserved run at the same seed.
//!
//! The `current_build_exhibits_statue_pathology` test at the bottom is a
//! BASELINE DOCUMENT, not a feature test: it asserts that today's build
//! exhibits the healer-statue pathology (focused Priest barely moves),
//! proving the harness detects the problem the posture work will fix.
//! When U6 lands, that test is expected to start failing and should be
//! inverted into the real statue probe.

use std::collections::{BTreeMap, BTreeSet};

use arenasim::headless::runner::MatchResult;
use arenasim::headless::{
    run_headless_match_observed, run_headless_match_with, FrameObservation, HeadlessMatchConfig,
};
use arenasim::CharacterClass;
use bevy::prelude::{Entity, Vec3};

// ---------------------------------------------------------------------------
// Probe support: timeline collection
// ---------------------------------------------------------------------------

/// Per-entity position samples: `(sim_time, position)` in frame order.
/// Samples are recorded only on frames where the entity is ALIVE — dead
/// combatants freeze in place, and sampling a corpse would deflate every
/// rate-style KPI computed from the timeline.
pub type EntityTimeline = Vec<(f32, Vec3)>;

/// Identity of a timeline entity, captured at first sight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityInfo {
    pub team: u8,
    pub slot: u8,
    pub class: CharacterClass,
    pub is_pet: bool,
}

/// Position timeline for one observed match.
#[derive(Debug, Default)]
pub struct Timeline {
    /// Alive-only position samples per entity.
    pub samples: BTreeMap<Entity, EntityTimeline>,
    /// Identity info per entity.
    pub info: BTreeMap<Entity, EntityInfo>,
    /// `sim_time` of the first observed frame where gates were open.
    pub gates_open_time: Option<f32>,
}

impl Timeline {
    /// Record one frame observation.
    pub fn record(&mut self, frame: &FrameObservation) {
        if frame.gates_open && self.gates_open_time.is_none() {
            self.gates_open_time = Some(frame.sim_time);
        }
        for (entity, obs) in &frame.combatants {
            self.info.entry(*entity).or_insert(EntityInfo {
                team: obs.team,
                slot: obs.slot,
                class: obs.class,
                is_pet: obs.is_pet,
            });
            if obs.alive {
                self.samples
                    .entry(*entity)
                    .or_default()
                    .push((frame.sim_time, obs.position));
            }
        }
    }

    /// Find the unique entity matching (team, class, is_pet). Panics if zero
    /// or multiple match — probes should address entities unambiguously.
    pub fn find(&self, team: u8, class: CharacterClass, is_pet: bool) -> Entity {
        let matches: Vec<Entity> = self
            .info
            .iter()
            .filter(|(_, i)| i.team == team && i.class == class && i.is_pet == is_pet)
            .map(|(e, _)| *e)
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "expected exactly one team-{} {:?} (is_pet={}), found {}",
            team,
            class,
            is_pet,
            matches.len()
        );
        matches[0]
    }

    /// Samples for `entity` with `sim_time >= t0` (e.g., post-gate slices).
    pub fn samples_from(&self, entity: Entity, t0: f32) -> EntityTimeline {
        self.samples
            .get(&entity)
            .map(|s| s.iter().copied().filter(|(t, _)| *t >= t0).collect())
            .unwrap_or_default()
    }
}

/// Run an observed headless match, collecting the full position timeline.
pub fn run_observed_collecting(config: HeadlessMatchConfig) -> (MatchResult, Timeline) {
    let mut timeline = Timeline::default();
    let result = run_headless_match_observed(config, true, None, |frame| {
        timeline.record(frame);
    })
    .expect("observed headless match failed");
    (result, timeline)
}

// ---------------------------------------------------------------------------
// Probe support: KPI helpers (pure functions over sample slices)
// ---------------------------------------------------------------------------

/// Total distance traveled along the sampled path. Zero for empty or
/// single-sample timelines.
pub fn path_length(samples: &[(f32, Vec3)]) -> f32 {
    samples.windows(2).map(|w| w[0].1.distance(w[1].1)).sum()
}

/// Match two timelines on identical `sim_time` stamps (both sides of an
/// observed run record the same frame clock, so equality is exact). Returns
/// `(sim_time, distance)` per matched frame. A mid-timeline death simply
/// truncates the matched set — no special casing needed downstream.
fn matched_distances(a: &[(f32, Vec3)], b: &[(f32, Vec3)]) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        let (ta, pa) = a[i];
        let (tb, pb) = b[j];
        if ta == tb {
            out.push((ta, pa.distance(pb)));
            i += 1;
            j += 1;
        } else if ta < tb {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// Simulated seconds during which entities `a` and `b` were within `range`
/// of each other. Each inter-sample interval is attributed to the distance
/// at its starting sample. Zero for fewer than two matched samples.
pub fn time_within_range_of(a: &[(f32, Vec3)], b: &[(f32, Vec3)], range: f32) -> f32 {
    matched_distances(a, b)
        .windows(2)
        .filter(|w| w[0].1 <= range)
        .map(|w| w[1].0 - w[0].0)
        .sum()
}

/// Separation gained between `a` and `b` over the window `[start, end]`:
/// distance at the last matched sample in-window minus distance at the first.
/// Positive = they moved apart. `None` if fewer than two matched samples
/// fall inside the window (the window is vacuous — see
/// `assert_min_occurrences`).
pub fn separation_gained_during(
    a: &[(f32, Vec3)],
    b: &[(f32, Vec3)],
    window: (f32, f32),
) -> Option<f32> {
    let in_window: Vec<(f32, f32)> = matched_distances(a, b)
        .into_iter()
        .filter(|(t, _)| *t >= window.0 && *t <= window.1)
        .collect();
    if in_window.len() < 2 {
        return None;
    }
    Some(in_window.last().unwrap().1 - in_window.first().unwrap().1)
}

/// Non-vacuity guard for window-conditional probes. A probe that asserts
/// "in every window where X held, Y happened" passes trivially if no window
/// occurred — e.g., after a seed-shifting change empties the window set.
/// Call this with the observed occurrence count so the probe fails loudly
/// ("probe went vacuous — re-scan seeds") instead.
#[track_caller]
pub fn assert_min_occurrences(label: &str, actual: usize, min: usize) {
    assert!(
        actual >= min,
        "probe went vacuous — re-scan seeds: '{}' occurred {} time(s), expected at least {}",
        label,
        actual,
        min
    );
}

// ---------------------------------------------------------------------------
// Shared config helper
// ---------------------------------------------------------------------------

fn create_config(team1: Vec<&str>, team2: Vec<&str>, seed: Option<u64>) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: team1.into_iter().map(String::from).collect(),
        team2: team2.into_iter().map(String::from).collect(),
        max_duration_secs: 120.0,
        random_seed: seed,
        ..Default::default()
    }
}

/// Strict `MatchResult` equality — exact float bits, not tolerance bands.
/// The non-perturbation guarantee is "identical", so the comparison is too.
fn assert_results_identical(a: &MatchResult, b: &MatchResult, context: &str) {
    assert_eq!(a.winner, b.winner, "{}: winner differs", context);
    assert_eq!(
        a.match_time.to_bits(),
        b.match_time.to_bits(),
        "{}: match_time differs: {} vs {}",
        context,
        a.match_time,
        b.match_time
    );
    assert_eq!(a.random_seed, b.random_seed, "{}: seed differs", context);

    for (team, ca, cb) in [
        (1u8, &a.team1_combatants, &b.team1_combatants),
        (2u8, &a.team2_combatants, &b.team2_combatants),
    ] {
        assert_eq!(ca.len(), cb.len(), "{}: team{} size differs", context, team);
        for (slot, (x, y)) in ca.iter().zip(cb.iter()).enumerate() {
            assert_eq!(
                x.class_name, y.class_name,
                "{}: team{} slot {} class differs",
                context, team, slot
            );
            assert_eq!(x.survived, y.survived, "{}: team{} slot {} survived differs", context, team, slot);
            for (field, fa, fb) in [
                ("max_health", x.max_health, y.max_health),
                ("final_health", x.final_health, y.final_health),
                ("damage_dealt", x.damage_dealt, y.damage_dealt),
                ("damage_taken", x.damage_taken, y.damage_taken),
            ] {
                assert_eq!(
                    fa.to_bits(),
                    fb.to_bits(),
                    "{}: team{} slot {} {} differs: {} vs {}",
                    context,
                    team,
                    slot,
                    field,
                    fa,
                    fb
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Harness self-tests
// ---------------------------------------------------------------------------

/// (a) NON-PERTURBATION — the load-bearing test. The per-frame observer is
/// read-only by construction (`observe_frame` uses `&World` access only);
/// this proves it end-to-end: an observed run returns a `MatchResult`
/// identical (exact float bits) to an unobserved run at the same seed.
#[test]
fn observed_run_does_not_perturb_outcomes() {
    let seed = 0xB0BA_F377_u64;
    let make = || {
        let mut cfg = create_config(
            vec!["Warrior", "Priest"],
            vec!["Rogue", "Priest"],
            Some(seed),
        );
        // Forced focus (Rogue trains the Priest) — same shape as the statue
        // probe below, so the non-perturbation guarantee covers the exact
        // scenario the probes lean on.
        cfg.team2_kill_target = Some(1);
        cfg
    };

    let unobserved = run_headless_match_with(make(), true, None).expect("unobserved run");

    let mut frames_observed = 0usize;
    let observed = run_headless_match_observed(make(), true, None, |_frame| {
        frames_observed += 1;
    })
    .expect("observed run");

    assert!(frames_observed > 0, "observer was never invoked");
    assert_results_identical(&observed, &unobserved, "observed vs unobserved");
}

/// (b) The observer receives monotonically increasing sim_time and sees the
/// same full combatant set every frame (dead combatants stay present with
/// `alive: false`; the alive count never increases).
#[test]
fn observer_sees_monotonic_time_and_all_combatants() {
    let cfg = create_config(
        vec!["Warrior", "Priest"],
        vec!["Mage", "Rogue"],
        Some(42),
    );

    let mut times: Vec<f32> = Vec::new();
    let mut entity_sets: Vec<BTreeSet<Entity>> = Vec::new();
    let mut alive_counts: Vec<usize> = Vec::new();
    let mut first_frame_alive_non_pets: Option<usize> = None;

    run_headless_match_observed(cfg, true, None, |frame| {
        times.push(frame.sim_time);
        entity_sets.push(frame.combatants.keys().copied().collect());
        alive_counts.push(frame.combatants.values().filter(|c| c.alive).count());
        if first_frame_alive_non_pets.is_none() {
            first_frame_alive_non_pets = Some(
                frame
                    .combatants
                    .values()
                    .filter(|c| c.alive && !c.is_pet)
                    .count(),
            );
        }
    })
    .expect("observed run");

    assert!(times.len() > 100, "expected a multi-second match, got {} frames", times.len());

    // Monotonic sim time: never decreasing, and strictly increasing after the
    // first frame (Bevy's first Time update has zero delta).
    for w in times.windows(2) {
        assert!(w[1] >= w[0], "sim_time went backwards: {} -> {}", w[0], w[1]);
    }
    let strict_increases = times.windows(2).filter(|w| w[1] > w[0]).count();
    assert!(
        strict_increases >= times.len().saturating_sub(2),
        "sim_time stalled: only {} strict increases over {} frames",
        strict_increases,
        times.len()
    );

    // All four combatants spawn alive and visible on the first frame.
    assert_eq!(
        first_frame_alive_non_pets,
        Some(4),
        "first frame should show all 4 living non-pet combatants"
    );

    // The entity set is identical every frame — dead combatants are not
    // despawned, so every living combatant is necessarily visible each frame.
    let first_set = &entity_sets[0];
    for (i, set) in entity_sets.iter().enumerate() {
        assert_eq!(
            set, first_set,
            "frame {}: combatant entity set changed mid-match",
            i
        );
    }

    // Alive count never increases (no resurrection mechanic).
    for (i, w) in alive_counts.windows(2).enumerate() {
        assert!(
            w[1] <= w[0],
            "frame {}: alive count increased {} -> {}",
            i + 1,
            w[0],
            w[1]
        );
    }
}

// ---------------------------------------------------------------------------
// (c) KPI unit tests on hand-built timelines
// ---------------------------------------------------------------------------

#[test]
fn path_length_of_known_path() {
    // Two unit steps: (0,0,0) -> (1,0,0) -> (1,0,1)
    let samples = vec![
        (0.0, Vec3::new(0.0, 0.0, 0.0)),
        (1.0, Vec3::new(1.0, 0.0, 0.0)),
        (2.0, Vec3::new(1.0, 0.0, 1.0)),
    ];
    assert!((path_length(&samples) - 2.0).abs() < 1e-6);
}

#[test]
fn path_length_edge_cases() {
    assert_eq!(path_length(&[]), 0.0, "empty timeline");
    assert_eq!(
        path_length(&[(0.0, Vec3::splat(3.0))]),
        0.0,
        "single-sample timeline"
    );
}

#[test]
fn time_within_range_known_value() {
    // a static at origin; b walks away 1 unit per second: distances 0,1,2,3,4.
    let a: EntityTimeline = (0..5).map(|i| (i as f32, Vec3::ZERO)).collect();
    let b: EntityTimeline = (0..5)
        .map(|i| (i as f32, Vec3::new(i as f32, 0.0, 0.0)))
        .collect();
    // Intervals starting at distance <= 2.0: [0,1), [1,2), [2,3) => 3 seconds.
    let t = time_within_range_of(&a, &b, 2.0);
    assert!((t - 3.0).abs() < 1e-6, "expected 3.0s within range, got {}", t);
}

#[test]
fn time_within_range_single_sample_is_zero() {
    let a = vec![(0.0, Vec3::ZERO)];
    let b = vec![(0.0, Vec3::new(1.0, 0.0, 0.0))];
    assert_eq!(time_within_range_of(&a, &b, 5.0), 0.0);
}

#[test]
fn time_within_range_entity_death_mid_timeline() {
    // a lives 0..=4s; b "dies" after t=2 (alive-only sampling truncates its
    // timeline). Matched samples stop at t=2 — only [0,1) and [1,2) count.
    let a: EntityTimeline = (0..5).map(|i| (i as f32, Vec3::ZERO)).collect();
    let b: EntityTimeline = (0..3)
        .map(|i| (i as f32, Vec3::new(1.0, 0.0, 0.0)))
        .collect();
    let t = time_within_range_of(&a, &b, 5.0);
    assert!((t - 2.0).abs() < 1e-6, "expected 2.0s (b died at t=2), got {}", t);
}

#[test]
fn separation_gained_known_value() {
    // Distance grows 1.0 -> 5.0 across the window.
    let a: EntityTimeline = (0..5).map(|i| (i as f32, Vec3::ZERO)).collect();
    let b: EntityTimeline = (0..5)
        .map(|i| (i as f32, Vec3::new(1.0 + i as f32, 0.0, 0.0)))
        .collect();
    let gained = separation_gained_during(&a, &b, (0.0, 4.0)).expect("window has samples");
    assert!((gained - 4.0).abs() < 1e-6, "expected +4.0 separation, got {}", gained);

    // Sub-window [1.0, 3.0]: distance 2.0 -> 4.0.
    let gained = separation_gained_during(&a, &b, (1.0, 3.0)).expect("sub-window has samples");
    assert!((gained - 2.0).abs() < 1e-6, "expected +2.0 separation, got {}", gained);
}

#[test]
fn separation_gained_vacuous_window_is_none() {
    let a: EntityTimeline = (0..5).map(|i| (i as f32, Vec3::ZERO)).collect();
    let b: EntityTimeline = (0..5)
        .map(|i| (i as f32, Vec3::new(1.0, 0.0, 0.0)))
        .collect();
    // Window after all samples.
    assert_eq!(separation_gained_during(&a, &b, (10.0, 20.0)), None);
    // Window containing exactly one sample.
    assert_eq!(separation_gained_during(&a, &b, (1.9, 2.1)), None);
}

#[test]
fn assert_min_occurrences_passes_at_threshold() {
    assert_min_occurrences("test windows", 3, 3);
    assert_min_occurrences("test windows", 5, 3);
}

#[test]
#[should_panic(expected = "probe went vacuous")]
fn assert_min_occurrences_fails_loudly_below_threshold() {
    assert_min_occurrences("escape windows", 0, 1);
}

// ---------------------------------------------------------------------------
// (d) Demo statue probe — documents the CURRENT pathology
// ---------------------------------------------------------------------------

/// BASELINE DOCUMENT, not a feature test. Today's Priest is positionally
/// inert: in a forced-focus match (Rogue training the Priest), its entire
/// post-gate path is roughly the initial approach walk (~21 units measured),
/// even while being attacked in melee. This test asserts the pathology IS
/// present — proving the probe harness detects the problem the healer
/// posture work (U6+) will fix. When that work lands, this test should
/// FAIL; invert it into the real statue probe (path length materially
/// ABOVE the baseline) at that point.
#[test]
fn current_build_exhibits_statue_pathology() {
    let mut cfg = create_config(
        vec!["Warrior", "Priest"],
        vec!["Rogue", "Priest"],
        Some(20260606),
    );
    // Rogue (team2) trains team1's Priest (slot 1).
    cfg.team2_kill_target = Some(1);

    let (_result, timeline) = run_observed_collecting(cfg);

    let gate_time = timeline
        .gates_open_time
        .expect("gates never opened — match misconfigured");

    let focused_priest = timeline.find(1, CharacterClass::Priest, false);
    let post_gate = timeline.samples_from(focused_priest, gate_time);

    // Non-vacuity: the Priest must survive at least ~1s of post-gate combat
    // for the path measurement to mean anything (60 frames at 60Hz).
    assert_min_occurrences("focused Priest post-gate samples", post_gate.len(), 60);

    let path = path_length(&post_gate);
    assert!(
        path < 30.0,
        "statue pathology no longer present: focused Priest post-gate path \
         length is {:.1} units (baseline ~21, threshold 30). If healer \
         movement AI has landed, invert this probe into the real statue test.",
        path
    );
}
