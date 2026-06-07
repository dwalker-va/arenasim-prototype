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
//! The `priest_postures` module at the bottom is the U6 probe suite for the
//! Priest FREE/PRESSURED posture work: the inverted statue probe (the U2
//! baseline test documented the pathology; U6 fixed it), plus anchor /
//! stealth / time-in-FREE / corner / 1v1-degenerate / zigzag / wand probes
//! at fixed seeds.
//!
//! The `escape_windows` / `escape_window_math` modules are the U7 suite for
//! ESCAPE windows and cast-vs-move urgency: escape-separation, heal-defer,
//! critical-heal, multi-attacker, and wall probes at fixed seeds (see the
//! seed notes on the module), plus pure unit tests of the slow-adjusted
//! window math.

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
// U6 — Priest FREE/PRESSURED posture probes
// ---------------------------------------------------------------------------

/// Probe support for the posture suite: an observed + traced run, with the
/// trace JSONL parsed into `serde_json::Value`s and a typed view of the
/// `movement_decision` events.
mod priest_postures {
    use super::*;
    use arenasim::headless::runner::TraceConfig;
    use arenasim::states::play_match::combat_core::CORNER_PENALTY_ONSET;

    /// One parsed `movement_decision` trace event. `sim_time` is COMBAT time
    /// (the trace clock starts at gates-open); add the timeline's
    /// `gates_open_time` to compare against `FrameObservation` timestamps.
    #[derive(Debug, Clone)]
    pub(super) struct MovementEvent {
        pub(super) sim_time: f32,
        pub(super) team: u8,
        pub(super) slot: u8,
        pub(super) trigger: String,
        pub(super) goal_kind: String,
        /// Actor world position at decision time (the event's `position`).
        pub(super) position: [f32; 3],
        /// Scorer-chosen unit XZ direction, when the goal is directional.
        pub(super) chosen_direction: Option<[f32; 2]>,
    }

    /// Run an observed + traced match; returns the result, the position
    /// timeline, and every parsed trace line.
    pub(super) fn run_observed_traced(
        config: HeadlessMatchConfig,
    ) -> (MatchResult, Timeline, Vec<serde_json::Value>) {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        drop(tmp);

        let mut timeline = Timeline::default();
        let result = run_headless_match_observed(
            config,
            true,
            Some(TraceConfig {
                output_path: path.clone(),
            }),
            |frame| timeline.record(frame),
        )
        .expect("observed traced headless match failed");

        let body = std::fs::read_to_string(&path).expect("read trace file");
        let events: Vec<serde_json::Value> = body
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        let _ = std::fs::remove_file(path);
        (result, timeline, events)
    }

    pub(super) fn movement_events(trace: &[serde_json::Value]) -> Vec<MovementEvent> {
        trace
            .iter()
            .filter(|v| v["kind"] == "movement_decision")
            .map(|v| MovementEvent {
                sim_time: v["sim_time"].as_f64().unwrap() as f32,
                team: v["actor"]["team"].as_u64().unwrap() as u8,
                slot: v["actor"]["slot"].as_u64().unwrap() as u8,
                trigger: v["trigger"].as_str().unwrap_or_default().to_string(),
                goal_kind: v["goal_kind"].as_str().unwrap_or_default().to_string(),
                position: {
                    let p = &v["position"];
                    [
                        p[0].as_f64().unwrap_or_default() as f32,
                        p[1].as_f64().unwrap_or_default() as f32,
                        p[2].as_f64().unwrap_or_default() as f32,
                    ]
                },
                chosen_direction: v["chosen_direction"].as_array().map(|d| {
                    [
                        d[0].as_f64().unwrap_or_default() as f32,
                        d[1].as_f64().unwrap_or_default() as f32,
                    ]
                }),
            })
            .collect()
    }

    /// PRESSURED windows (combat-time) for one actor, from PressuredEnter /
    /// PressuredExit transitions; an unclosed window ends at `end`.
    pub(super) fn pressured_windows(
        events: &[MovementEvent],
        team: u8,
        slot: u8,
        end: f32,
    ) -> Vec<(f32, f32)> {
        let mut windows = Vec::new();
        let mut open: Option<f32> = None;
        for e in events.iter().filter(|e| e.team == team && e.slot == slot) {
            match e.trigger.as_str() {
                "PressuredEnter" if open.is_none() => open = Some(e.sim_time),
                "PressuredExit" => {
                    if let Some(start) = open.take() {
                        windows.push((start, e.sim_time));
                    }
                }
                _ => {}
            }
        }
        if let Some(start) = open {
            windows.push((start, end));
        }
        windows
    }

    /// Per-frame `(sim_time, min distance)` from `me` to the nearest of
    /// `others`, matched on identical frame stamps.
    fn min_distance_series(
        timeline: &Timeline,
        me: Entity,
        others: &[Entity],
    ) -> Vec<(f32, f32)> {
        let me_samples = timeline.samples.get(&me).cloned().unwrap_or_default();
        // Keyed by f32 bits — sim_time is positive and increasing, so bit
        // order equals numeric order.
        let mut merged: BTreeMap<u32, f32> = BTreeMap::new();
        for other in others {
            let other_samples = timeline.samples.get(other).cloned().unwrap_or_default();
            let (mut i, mut j) = (0usize, 0usize);
            while i < me_samples.len() && j < other_samples.len() {
                let (ta, pa) = me_samples[i];
                let (tb, pb) = other_samples[j];
                if ta == tb {
                    let d = pa.distance(pb);
                    merged
                        .entry(ta.to_bits())
                        .and_modify(|m| *m = m.min(d))
                        .or_insert(d);
                    i += 1;
                    j += 1;
                } else if ta < tb {
                    i += 1;
                } else {
                    j += 1;
                }
            }
        }
        merged
            .into_iter()
            .map(|(bits, d)| (f32::from_bits(bits), d))
            .collect()
    }

    /// Longest consecutive stretch (seconds) where `predicate` holds across
    /// a `(sim_time, value)` series.
    fn max_consecutive_secs(series: &[(f32, f32)], predicate: impl Fn(f32) -> bool) -> f32 {
        let mut longest = 0.0f32;
        let mut run_start: Option<f32> = None;
        let mut last_t = 0.0f32;
        for &(t, v) in series {
            if predicate(v) {
                if run_start.is_none() {
                    run_start = Some(t);
                }
                last_t = t;
            } else if let Some(start) = run_start.take() {
                longest = longest.max(last_t - start);
            }
        }
        if let Some(start) = run_start {
            longest = longest.max(last_t - start);
        }
        longest
    }

    /// The statue comp at its fixed seed: Warrior+Priest vs Rogue+Priest,
    /// team 2 forced onto team 1's Priest (slot 1).
    fn statue_config() -> HeadlessMatchConfig {
        let mut cfg = create_config(
            vec!["Warrior", "Priest"],
            vec!["Rogue", "Priest"],
            Some(20260606),
        );
        cfg.team2_kill_target = Some(1);
        cfg
    }

    /// (a) STATUE PROBE — the inversion of U2's
    /// `current_build_exhibits_statue_pathology` baseline test. Pre-U6 the
    /// focused Priest's post-gate path was ~21 units (the approach walk) and
    /// it face-tanked the Rogue. With FREE/PRESSURED postures the Priest
    /// repositions: path length materially ABOVE the old statue band AND
    /// bounded time within 10yd of its attacker.
    #[test]
    fn forced_focus_priest_escapes_statue_pathology() {
        let (_result, timeline, trace) = run_observed_traced(statue_config());

        let gate_time = timeline
            .gates_open_time
            .expect("gates never opened — match misconfigured");

        let priest = timeline.find(1, CharacterClass::Priest, false);
        let rogue = timeline.find(2, CharacterClass::Rogue, false);
        let post_gate = timeline.samples_from(priest, gate_time);
        assert_min_occurrences("focused Priest post-gate samples", post_gate.len(), 60);

        let path = path_length(&post_gate);
        let alive_secs = post_gate.last().unwrap().0 - post_gate.first().unwrap().0;
        let rogue_post_gate = timeline.samples_from(rogue, gate_time);
        let t10 = time_within_range_of(&post_gate, &rogue_post_gate, 10.0);
        let frac10 = t10 / alive_secs.max(f32::EPSILON);
        let t4 = time_within_range_of(&post_gate, &rogue_post_gate, 4.0);
        let frac4 = t4 / alive_secs.max(f32::EPSILON);
        eprintln!(
            "statue probe: path={:.1} (pre-U6 baseline ~21), alive={:.1}s, \
             time-within-10yd-of-Rogue={:.1}s ({:.0}%), within-4yd={:.1}s ({:.0}%)",
            path,
            alive_secs,
            t10,
            frac10 * 100.0,
            t4,
            frac4 * 100.0
        );

        // Non-vacuity: the posture machinery actually fired.
        let events = movement_events(&trace);
        let priest_events = events.iter().filter(|e| e.team == 1 && e.slot == 1).count();
        assert_min_occurrences("focused Priest movement_decision events", priest_events, 1);

        assert!(
            path > 60.0,
            "statue pathology: focused Priest post-gate path is only {:.1} units \
             (pre-U6 baseline ~21; measured post-U6 ~89; healthy threshold 60)",
            path
        );
        // Threat-range ceiling — a REGRESSION NET, not an aspiration. The
        // Rogue moves at the same base speed as the Priest and casting locks
        // movement (R12), so a healing Priest can never shake an equal-speed
        // melee chaser: measured per-frame within-10yd is ~80% post-U6 and
        // was ~81% pre-U6 at this seed (the discriminator is path length).
        // The ceiling catches the failure mode where the Priest stops
        // repositioning between casts entirely and the Rogue parks at
        // 0.9-1.9yd for its whole life (the U2-documented pathology, ~100%
        // once engaged).
        assert!(
            frac10 < 0.85,
            "focused Priest spent {:.0}% of its post-gate life within 10yd of \
             the Rogue (ceiling 85% — see regression-net comment)",
            frac10 * 100.0
        );
    }

    /// (b) ANCHOR PROBE — while PRESSURED, the Priest never exits heal range
    /// (40) of its ally for more than a 1s grace (R6 anchor constraint).
    #[test]
    fn pressured_priest_stays_in_heal_range_of_ally() {
        let (_result, timeline, trace) = run_observed_traced(statue_config());
        let gate_time = timeline.gates_open_time.expect("gates opened");

        let priest = timeline.find(1, CharacterClass::Priest, false);
        let warrior = timeline.find(1, CharacterClass::Warrior, false);
        let events = movement_events(&trace);

        // Last priest sample (combat time) bounds any unclosed window.
        let priest_samples = timeline.samples.get(&priest).cloned().unwrap_or_default();
        let end = priest_samples.last().map(|(t, _)| t - gate_time).unwrap_or(0.0);
        let windows = pressured_windows(&events, 1, 1, end);
        assert_min_occurrences("PRESSURED windows (focused Priest)", windows.len(), 1);

        let ally_distance = min_distance_series(&timeline, priest, &[warrior]);
        for (start, stop) in &windows {
            // Convert combat-time window to timeline time.
            let (w0, w1) = (start + gate_time, stop + gate_time);
            let in_window: Vec<(f32, f32)> = ally_distance
                .iter()
                .copied()
                .filter(|(t, _)| *t >= w0 && *t <= w1)
                .collect();
            let out_of_range = max_consecutive_secs(&in_window, |d| d > 40.0);
            eprintln!(
                "anchor probe: window [{:.1},{:.1}]s, max consecutive out-of-heal-range {:.2}s",
                w0, w1, out_of_range
            );
            assert!(
                out_of_range <= 1.0,
                "PRESSURED Priest left heal range (40) of its ally for {:.2}s \
                 (grace 1.0s) during window [{:.1},{:.1}]",
                out_of_range,
                w0,
                w1
            );
        }
    }

    /// (c) STEALTH PROBE (AE2) — vs a stealth-opener Rogue forced onto the
    /// Priest, no posture transition fires before the opener lands:
    /// `enemies_targeting` is stealth-filtered, so the healer never
    /// pre-dodges an invisible Rogue.
    #[test]
    fn no_pressured_transition_before_stealth_opener_lands() {
        let mut cfg = create_config(vec!["Warrior", "Priest"], vec!["Rogue"], Some(404));
        // The lone Rogue trains the Priest (slot 1). A 2-enemy comp would
        // contaminate the probe: the Rogue's visible teammate would also be
        // forced onto the Priest and legitimately pressure it pre-opener.
        cfg.team2_kill_target = Some(1);

        let (_result, _timeline, trace) = run_observed_traced(cfg);

        // Opener = the Rogue's first non-Stealth action_taken.
        let opener_time = trace
            .iter()
            .filter(|v| {
                v["kind"] == "ability_decision"
                    && v["actor"]["team"] == 2
                    && v["actor"]["class"] == "Rogue"
                    && v["outcome"]["type"] == "action_taken"
                    && v["outcome"]["ability"] != "Stealth"
            })
            .map(|v| v["sim_time"].as_f64().unwrap() as f32)
            .next();
        let opener_time = opener_time.expect("probe went vacuous — Rogue never opened");

        let events = movement_events(&trace);
        let priest_enters: Vec<f32> = events
            .iter()
            .filter(|e| e.team == 1 && e.slot == 1 && e.trigger == "PressuredEnter")
            .map(|e| e.sim_time)
            .collect();
        assert_min_occurrences("Priest PressuredEnter events", priest_enters.len(), 1);

        eprintln!(
            "stealth probe: opener at {:.2}s (combat time), first PressuredEnter at {:.2}s",
            opener_time, priest_enters[0]
        );
        for t in &priest_enters {
            assert!(
                *t >= opener_time - 0.05,
                "PressuredEnter at {:.2}s fired BEFORE the stealth opener landed at \
                 {:.2}s — stealth filtering leaked",
                t,
                opener_time
            );
        }
    }

    /// (d) TIME-IN-FREE PROBE — Warrior+Priest mirror, unforced targeting:
    /// each Priest spends substantial time in FREE. Kill-target acquisition
    /// is nearest-or-configured, so healers are rarely the formal target in
    /// team comps; the PRESSURED trigger must not over-fire.
    #[test]
    fn priests_spend_substantial_time_free_in_unforced_mirror() {
        let cfg = create_config(
            vec!["Warrior", "Priest"],
            vec!["Warrior", "Priest"],
            Some(11),
        );
        let (result, _timeline, trace) = run_observed_traced(cfg);
        assert!(
            result.match_time > 20.0,
            "probe needs a multi-phase match, got {:.1}s",
            result.match_time
        );

        let events = movement_events(&trace);
        for team in [1u8, 2u8] {
            let windows = pressured_windows(&events, team, 1, result.match_time);
            let pressured: f32 = windows.iter().map(|(a, b)| b - a).sum();
            let frac = pressured / result.match_time;
            eprintln!(
                "time-in-FREE probe: team{} Priest pressured {:.1}s of {:.1}s ({:.0}%)",
                team,
                pressured,
                result.match_time,
                frac * 100.0
            );
            assert!(
                frac < 0.5,
                "team{} Priest spent {:.0}% of the match PRESSURED in an unforced \
                 mirror (ceiling 50% — the trigger is over-firing)",
                team,
                frac * 100.0
            );
        }
    }

    /// (e) CORNER PROBE — under sustained melee pressure the Priest never
    /// sits inside the scorer's corner geometry (|x|+|z| >=
    /// CORNER_PENALTY_ONSET) for more than 5 consecutive seconds.
    #[test]
    fn pressured_priest_does_not_pin_into_corners() {
        let (_result, timeline, _trace) = run_observed_traced(statue_config());
        let gate_time = timeline.gates_open_time.expect("gates opened");

        let priest = timeline.find(1, CharacterClass::Priest, false);
        let post_gate = timeline.samples_from(priest, gate_time);
        assert_min_occurrences("focused Priest post-gate samples", post_gate.len(), 60);

        let corner_series: Vec<(f32, f32)> = post_gate
            .iter()
            .map(|(t, p)| (*t, p.x.abs() + p.z.abs()))
            .collect();
        let in_corner = max_consecutive_secs(&corner_series, |s| s >= CORNER_PENALTY_ONSET);
        eprintln!(
            "corner probe: max consecutive time at |x|+|z| >= {:.1}: {:.2}s",
            CORNER_PENALTY_ONSET, in_corner
        );
        assert!(
            in_corner < 5.0,
            "Priest sat in the corner band (|x|+|z| >= {:.1}) for {:.2}s \
             consecutively (ceiling 5s)",
            CORNER_PENALTY_ONSET,
            in_corner
        );
    }

    /// (f) 1v1 DEGENERATE PROBE (AE4) — Priest vs Warrior: with no living
    /// non-pet ally, FREE issues NO formation directive (legacy
    /// preferred_range pursuit governs); PRESSURED remains available; the
    /// match is sane (completes decisively, no crash).
    #[test]
    fn priest_1v1_issues_no_formation_directive() {
        let cfg = create_config(vec!["Priest"], vec!["Warrior"], Some(7));
        let (result, _timeline, trace) = run_observed_traced(cfg);

        assert!(
            result.winner.is_some(),
            "1v1 Priest vs Warrior at seed 7 should end decisively, got draw \
             after {:.1}s",
            result.match_time
        );

        let events = movement_events(&trace);
        let point_events = events.iter().filter(|e| e.goal_kind == "point").count();
        assert_eq!(
            point_events, 0,
            "1v1 Priest issued {} formation (Point-goal) movement decisions — \
             the degenerate case must fall through to legacy pursuit",
            point_events
        );
        // PRESSURED is still active in 1v1 (the Warrior is a melee threat
        // targeting the Priest) — non-vacuity for the degenerate branch.
        let pressured = events
            .iter()
            .filter(|e| e.team == 1 && e.trigger == "PressuredEnter")
            .count();
        assert_min_occurrences("1v1 Priest PressuredEnter", pressured, 1);
    }

    /// (g) ZIGZAG PROBE (R11) — committed direction changes per 10s of
    /// PRESSURED time stay below a ceiling: the commitment window + bonus
    /// must suppress per-tick direction thrash.
    #[test]
    fn pressured_direction_changes_are_bounded() {
        let (result, _timeline, trace) = run_observed_traced(statue_config());
        let events = movement_events(&trace);

        let mut total_pressured = 0.0f32;
        let mut total_changes = 0usize;
        for (team, slot) in [(1u8, 1u8), (2u8, 1u8)] {
            let windows = pressured_windows(&events, team, slot, result.match_time);
            total_pressured += windows.iter().map(|(a, b)| b - a).sum::<f32>();
            total_changes += events
                .iter()
                .filter(|e| e.team == team && e.slot == slot && e.trigger == "CommitExpired")
                .count();
        }
        assert!(
            total_pressured >= 5.0,
            "probe went vacuous — re-scan seeds: only {:.1}s of combined \
             PRESSURED time",
            total_pressured
        );

        let rate = total_changes as f32 / total_pressured * 10.0;
        eprintln!(
            "zigzag probe: {} committed direction changes over {:.1}s PRESSURED \
             ({:.1} per 10s)",
            total_changes, total_pressured, rate
        );
        assert!(
            rate <= 12.0,
            "{:.1} committed direction changes per 10s of PRESSURED time \
             (ceiling 12) — commitment window is not suppressing zigzag",
            rate
        );
    }

    /// (h) WAND PROBE — an unthreatened Priest (its teammate soaks the
    /// focus) drifts into wand range (30) of its kill target. The U2
    /// `FrameObservation` does not expose wand hits, so this asserts
    /// POSITIONAL CONVERGENCE into wand range (per the probe spec's
    /// fallback), not landed-hit counts.
    #[test]
    fn unthreatened_priest_drifts_into_wand_range() {
        let mut cfg = create_config(
            vec!["Warrior", "Priest"],
            vec!["Warrior", "Priest"],
            Some(11),
        );
        // Team 2 trains team 1's WARRIOR — team 1's Priest stays unthreatened.
        cfg.team2_kill_target = Some(0);

        let (_result, timeline, _trace) = run_observed_traced(cfg);
        let gate_time = timeline.gates_open_time.expect("gates opened");

        let priest = timeline.find(1, CharacterClass::Priest, false);
        let enemies: Vec<Entity> = timeline
            .info
            .iter()
            .filter(|(_, i)| i.team == 2 && !i.is_pet)
            .map(|(e, _)| *e)
            .collect();

        // Allow the formation to settle before measuring convergence.
        let settle = gate_time + 8.0;
        let series: Vec<(f32, f32)> = min_distance_series(&timeline, priest, &enemies)
            .into_iter()
            .filter(|(t, _)| *t >= settle)
            .collect();
        assert_min_occurrences("post-settle samples", series.len(), 60);

        let total = series.last().unwrap().0 - series.first().unwrap().0;
        let mut in_range = 0.0f32;
        for w in series.windows(2) {
            if w[0].1 <= 30.0 {
                in_range += w[1].0 - w[0].0;
            }
        }
        let frac = in_range / total.max(f32::EPSILON);
        eprintln!(
            "wand probe: {:.1}s of {:.1}s ({:.0}%) within wand range (30) of the \
             nearest enemy after settling",
            in_range,
            total,
            frac * 100.0
        );
        assert!(
            frac >= 0.5,
            "unthreatened Priest spent only {:.0}% of post-settle time within \
             wand range (30) of an enemy (floor 50%) — the wand pull is not \
             working",
            frac * 100.0
        );
    }
}

// ---------------------------------------------------------------------------
// U7 — ESCAPE windows and cast-vs-move urgency
// ---------------------------------------------------------------------------
//
// Seed notes (scanned seeds 1..20 per comp during development; the forced-
// target openings are essentially seed-invariant for the first ~15s, so the
// pinned seeds are robust):
//
// - Escape/defer comp: Priest+Paladin vs Warrior+Mage, both kill targets on
//   index 0 (team1 → enemy Warrior, team2 → our Priest). The Paladin melees
//   the Warrior that is chasing the Priest and rotation-HoJs it at first
//   contact (~6.4s combat time) — a 6s stun right next to the Priest, the
//   canonical escape window. The enemy Mage stays at caster range (beyond
//   the danger radius), so the Warrior is the only proximate threat.
//   NOTE: the plan suggested Mage Frost Nova as the window source, but the
//   Mage AI only Novas with an enemy inside MELEE_RANGE of the MAGE — a
//   Warrior forced onto the Priest never gets that close to the Mage in any
//   scanned seed (0 windows over 8 seeds × 2 comps). A teammate stun (HoJ /
//   Kidney Shot) is the reliable natural source. Also scanned and rejected:
//   enemy comps with pet owners (Warlock/Hunter) — the pet inherits the
//   Priest as target and parks in melee as a permanently-unimpaired
//   proximate threat, correctly voiding every window (multi-attacker rule).
// - Critical-heal comp: Priest+Paladin vs Rogue+Mage at seed 14 — the only
//   scanned seed (1..20) where the Priest's HP sits below the urgency
//   threshold mid-window with Holy school unlocked and PW:Shield spent, so
//   a Flash Heal STARTS inside the live window (measured: t=12.37, hp=0.36).
// - Multi-attacker comp: Priest+Paladin vs Warrior+Warrior — both Warriors
//   reach the Priest together, HoJ stuns exactly one, the other stays free:
//   0 windows across all scanned seeds.
// - Wall comp: Priest+Rogue vs lone Warrior (forced onto the Priest). The
//   Rogue's Kidney Shot lands at ~10.8s combat time, by which point the
//   Priest has been chased onto the west wall (x = -36.5): the window OPENS
//   in the wall band and the scored direction bends back into the arena
//   (measured chosen_direction ≈ (0.92, 0.38), not the straight-away (-1,0)
//   that would pin into the wall).

mod escape_windows {
    use super::priest_postures::{movement_events, pressured_windows, run_observed_traced, MovementEvent};
    use super::*;
    use arenasim::states::play_match::constants::{
        ARENA_CORNER_SUM, ARENA_HALF_X, ARENA_HALF_Z,
    };
    use arenasim::states::play_match::movement_config::load_movement_config;

    /// Separation floor asserted per window (units of XZ distance gained
    /// from the impaired attacker). There is no movement.ron knob for this —
    /// it is the probe's regression bound, set at a quarter of the measured
    /// value (~20 units over the canonical ~5.9s HoJ window) so weight
    /// tuning has headroom without letting the behavior regress to "stood
    /// still through the window".
    const MIN_WINDOW_SEPARATION: f32 = 5.0;

    /// ESCAPE windows (combat time) for one actor, from EscapeWindowOpen /
    /// EscapeWindowClosed transitions; an unclosed window (match ended
    /// mid-escape) ends at `end`.
    fn escape_window_spans(
        events: &[MovementEvent],
        team: u8,
        slot: u8,
        end: f32,
    ) -> Vec<(f32, f32)> {
        let mut windows = Vec::new();
        let mut open: Option<f32> = None;
        for e in events.iter().filter(|e| e.team == team && e.slot == slot) {
            match e.trigger.as_str() {
                "EscapeWindowOpen" if open.is_none() => open = Some(e.sim_time),
                "EscapeWindowClosed" => {
                    if let Some(start) = open.take() {
                        windows.push((start, e.sim_time));
                    }
                }
                _ => {}
            }
        }
        if let Some(start) = open {
            windows.push((start, end));
        }
        windows
    }

    /// Flash Heal deferral rejects (combat time) emitted by the team-1
    /// Priest — the cast-vs-move urgency rule's trace fingerprint.
    fn flash_heal_defer_times(trace: &[serde_json::Value]) -> Vec<f32> {
        trace
            .iter()
            .filter(|v| {
                v["kind"] == "ability_decision"
                    && v["actor"]["team"] == 1
                    && v["actor"]["class"] == "Priest"
            })
            .filter(|v| {
                v["candidates"].as_array().map_or(false, |cands| {
                    cands.iter().any(|c| {
                        c["ability"] == "FlashHeal"
                            && c["reason"]["PreconditionUnmet"]["note"]
                                .as_str()
                                .map_or(false, |n| n.starts_with("escape window"))
                    })
                })
            })
            .map(|v| v["sim_time"].as_f64().unwrap() as f32)
            .collect()
    }

    /// Flash Heal cast STARTS (combat time, actor hp_pct) by the team-1
    /// Priest.
    fn flash_heal_starts(trace: &[serde_json::Value]) -> Vec<(f32, f32)> {
        trace
            .iter()
            .filter(|v| {
                v["kind"] == "ability_decision"
                    && v["actor"]["team"] == 1
                    && v["actor"]["class"] == "Priest"
                    && v["outcome"]["type"] == "action_taken"
                    && v["outcome"]["ability"] == "FlashHeal"
            })
            .map(|v| {
                (
                    v["sim_time"].as_f64().unwrap() as f32,
                    v["actor"]["hp_pct"].as_f64().unwrap() as f32,
                )
            })
            .collect()
    }

    /// Half-open in-window test, `[open, close)`: the close tick itself is
    /// post-window (the EscapeWindowClosed transition and the first
    /// post-window decision share a tick — `evaluate_priest_posture` runs
    /// before the ability pass).
    fn in_window(t: f32, windows: &[(f32, f32)]) -> bool {
        windows.iter().any(|(open, close)| t >= *open && t < *close)
    }

    /// Distance from `pos` to the nearest arena boundary (rect edges + the
    /// |x|+|z| corner walls). Small values = pressed against a wall.
    fn boundary_proximity(pos: [f32; 3]) -> f32 {
        let (x, z) = (pos[0], pos[2]);
        (ARENA_HALF_X - x.abs())
            .min(ARENA_HALF_Z - z.abs())
            .min(ARENA_CORNER_SUM - (x.abs() + z.abs()))
    }

    /// The canonical escape comp: Priest+Paladin vs Warrior+Mage, Warrior
    /// forced onto the Priest, team 1 forced onto the Warrior.
    fn escape_config(seed: u64) -> HeadlessMatchConfig {
        let mut cfg = create_config(
            vec!["Priest", "Paladin"],
            vec!["Warrior", "Mage"],
            Some(seed),
        );
        cfg.team1_kill_target = Some(0);
        cfg.team2_kill_target = Some(0);
        cfg
    }

    /// (a) ESCAPE PROBE — a teammate's stun on the Priest's attacker
    /// converts into separation: ≥1 window occurred (non-vacuity), and in
    /// EVERY window the Priest gained at least the configured separation
    /// from the impaired attacker.
    #[test]
    fn escape_window_converts_cc_into_separation() {
        let (result, timeline, trace) = run_observed_traced(escape_config(1));
        let gate = timeline.gates_open_time.expect("gates opened");

        let events = movement_events(&trace);
        let windows = escape_window_spans(&events, 1, 0, result.match_time);
        assert_min_occurrences("escape windows", windows.len(), 1);

        let priest = timeline.find(1, CharacterClass::Priest, false);
        let warrior = timeline.find(2, CharacterClass::Warrior, false);
        let priest_samples = timeline.samples.get(&priest).cloned().unwrap_or_default();
        let warrior_samples = timeline.samples.get(&warrior).cloned().unwrap_or_default();

        for (open, close) in &windows {
            let gained = separation_gained_during(
                &priest_samples,
                &warrior_samples,
                (open + gate, close + gate),
            )
            .expect("window must contain matched samples");
            eprintln!(
                "escape probe: window [{:.1},{:.1}]s (combat time) separation gained {:.1} \
                 (floor {})",
                open, close, gained, MIN_WINDOW_SEPARATION
            );
            assert!(
                gained >= MIN_WINDOW_SEPARATION,
                "escape window [{:.1},{:.1}] gained only {:.1} separation from the \
                 impaired attacker (floor {})",
                open,
                close,
                gained,
                MIN_WINDOW_SEPARATION
            );
        }
    }

    /// (b) DEFER PROBE — while a window is live and the would-be heal target
    /// is above the urgency threshold, the Priest does NOT start a Flash
    /// Heal: the deferral reject fires in-window (non-vacuity), and no Flash
    /// Heal cast starts inside any live window at this seed (the only
    /// sub-threshold moment gets an instant PW:Shield — instants are not
    /// deferred — whose GCD outlasts the window).
    #[test]
    fn live_window_defers_noncritical_heals() {
        let (result, _timeline, trace) = run_observed_traced(escape_config(1));

        let events = movement_events(&trace);
        let windows = escape_window_spans(&events, 1, 0, result.match_time);
        assert_min_occurrences("escape windows", windows.len(), 1);

        let defers_in_window = flash_heal_defer_times(&trace)
            .into_iter()
            .filter(|t| in_window(*t, &windows))
            .count();
        assert_min_occurrences("in-window Flash Heal deferrals", defers_in_window, 1);

        let starts_in_window: Vec<(f32, f32)> = flash_heal_starts(&trace)
            .into_iter()
            .filter(|(t, _)| in_window(*t, &windows))
            .collect();
        eprintln!(
            "defer probe: {} in-window deferral rejects, {} in-window Flash Heal starts",
            defers_in_window,
            starts_in_window.len()
        );
        assert!(
            starts_in_window.is_empty(),
            "Flash Heal started inside a live escape window at {:?} — the \
             cast-vs-move deferral did not hold",
            starts_in_window
        );
    }

    /// (c) CRITICAL-HEAL PROBE (AE1) — an ally below the urgency threshold
    /// during a live window is healed anyway: a Flash Heal STARTS in-window.
    /// Seed 14 is the scanned seed where the Priest's own HP (it is the
    /// lowest ally — the whole enemy team is on it) is sub-threshold
    /// mid-window with Holy unlocked: measured start t=12.37, hp=0.36.
    #[test]
    fn critical_heal_fires_despite_live_window() {
        let threshold = load_movement_config()
            .expect("movement.ron loads")
            .shared
            .urgency_hp_threshold;

        let mut cfg = create_config(
            vec!["Priest", "Paladin"],
            vec!["Rogue", "Mage"],
            Some(14),
        );
        cfg.team1_kill_target = Some(0);
        cfg.team2_kill_target = Some(0);
        let (result, _timeline, trace) = run_observed_traced(cfg);

        let events = movement_events(&trace);
        let windows = escape_window_spans(&events, 1, 0, result.match_time);
        assert_min_occurrences("escape windows", windows.len(), 1);

        let critical_starts: Vec<(f32, f32)> = flash_heal_starts(&trace)
            .into_iter()
            .filter(|(t, hp)| in_window(*t, &windows) && *hp <= threshold)
            .collect();
        eprintln!(
            "critical-heal probe: in-window sub-threshold Flash Heal starts: {:?}",
            critical_starts
        );
        assert_min_occurrences(
            "in-window critical Flash Heal starts",
            critical_starts.len(),
            1,
        );
    }

    /// (d) MULTI-ATTACKER PROBE — two melee on the Priest, only one stunned:
    /// no EscapeWindowOpen ever fires. Non-vacuity is established
    /// structurally: the Paladin's HoJ landed while the Priest was PRESSURED
    /// with BOTH Warriors inside the danger radius — exactly one of them
    /// impaired — so a window WOULD have opened but for the unimpaired
    /// second attacker.
    #[test]
    fn unimpaired_second_attacker_voids_window() {
        let danger_radius = load_movement_config()
            .expect("movement.ron loads")
            .shared
            .danger_radius;

        let mut cfg = create_config(
            vec!["Priest", "Paladin"],
            vec!["Warrior", "Warrior"],
            Some(1),
        );
        cfg.team1_kill_target = Some(0);
        cfg.team2_kill_target = Some(0);
        let (result, timeline, trace) = run_observed_traced(cfg);
        let gate = timeline.gates_open_time.expect("gates opened");

        // Non-vacuity 1: the stun actually landed.
        let hoj_times: Vec<f32> = trace
            .iter()
            .filter(|v| {
                v["kind"] == "ability_decision"
                    && v["actor"]["team"] == 1
                    && v["actor"]["class"] == "Paladin"
                    && v["outcome"]["type"] == "action_taken"
                    && v["outcome"]["ability"] == "HammerOfJustice"
            })
            .map(|v| v["sim_time"].as_f64().unwrap() as f32)
            .collect();
        assert_min_occurrences("Paladin HoJ casts", hoj_times.len(), 1);

        // Non-vacuity 2: the Priest was PRESSURED at the stun moment.
        let events = movement_events(&trace);
        let pressured = pressured_windows(&events, 1, 0, result.match_time);
        let hoj = hoj_times[0];
        assert!(
            pressured.iter().any(|(a, b)| hoj >= *a && hoj <= *b),
            "probe went vacuous — re-scan seeds: HoJ at {:.1}s fell outside every \
             PRESSURED window {:?}",
            hoj,
            pressured
        );

        // Non-vacuity 3: both Warriors were proximate threats at that moment.
        let priest = timeline.find(1, CharacterClass::Priest, false);
        let warriors: Vec<bevy::prelude::Entity> = timeline
            .info
            .iter()
            .filter(|(_, i)| i.team == 2 && i.class == CharacterClass::Warrior && !i.is_pet)
            .map(|(e, _)| *e)
            .collect();
        assert_eq!(warriors.len(), 2, "comp must field two enemy Warriors");
        let at = |entity: bevy::prelude::Entity, t: f32| -> Vec3 {
            timeline
                .samples
                .get(&entity)
                .and_then(|s| {
                    s.iter()
                        .min_by(|a, b| {
                            (a.0 - t).abs().partial_cmp(&(b.0 - t).abs()).unwrap()
                        })
                        .map(|(_, p)| *p)
                })
                .expect("entity has samples")
        };
        let priest_pos = at(priest, hoj + gate);
        for w in &warriors {
            let d = priest_pos.distance(at(*w, hoj + gate));
            eprintln!(
                "multi-attacker probe: warrior at {:.1} units from the Priest at the \
                 HoJ moment (danger radius {})",
                d, danger_radius
            );
            assert!(
                d <= danger_radius,
                "probe went vacuous — re-scan seeds: a Warrior was {:.1} units away \
                 at the HoJ moment (outside danger radius {})",
                d,
                danger_radius
            );
        }

        // The actual rule: one free proximate attacker voids every window.
        let opens = events
            .iter()
            .filter(|e| e.team == 1 && e.slot == 0 && e.trigger == "EscapeWindowOpen")
            .count();
        assert_eq!(
            opens, 0,
            "EscapeWindowOpen fired with an unimpaired second melee on the Priest — \
             the multi-attacker rule leaked"
        );
    }

    /// (e) WALL PROBE — a window that OPENS with the Priest pressed against
    /// a boundary still produces separation: the scored direction bends back
    /// into the arena (boundary penalty active) instead of pinning into the
    /// wall. Priest+Rogue vs a lone forced Warrior: Kidney Shot lands at
    /// ~10.8s, by which point the Priest sits on the west wall (x=-36.5).
    #[test]
    fn wall_adjacent_window_still_gains_separation() {
        let mut cfg = create_config(vec!["Priest", "Rogue"], vec!["Warrior"], Some(1));
        cfg.team2_kill_target = Some(0);
        let (result, timeline, trace) = run_observed_traced(cfg);
        let gate = timeline.gates_open_time.expect("gates opened");

        let events = movement_events(&trace);
        let opens: Vec<&MovementEvent> = events
            .iter()
            .filter(|e| e.team == 1 && e.slot == 0 && e.trigger == "EscapeWindowOpen")
            .collect();
        let wall_opens: Vec<&&MovementEvent> = opens
            .iter()
            .filter(|e| boundary_proximity(e.position) <= 1.0)
            .collect();
        assert_min_occurrences("wall-adjacent escape windows", wall_opens.len(), 1);

        let windows = escape_window_spans(&events, 1, 0, result.match_time);
        let priest = timeline.find(1, CharacterClass::Priest, false);
        let warrior = timeline.find(2, CharacterClass::Warrior, false);
        let priest_samples = timeline.samples.get(&priest).cloned().unwrap_or_default();
        let warrior_samples = timeline.samples.get(&warrior).cloned().unwrap_or_default();

        for open_event in &wall_opens {
            // The scored direction must not push out of bounds: one
            // scorer-lookahead step along it stays inside the arena.
            let dir = open_event
                .chosen_direction
                .expect("escape windows carry a chosen direction");
            let next = [
                open_event.position[0] + dir[0] * 2.0,
                open_event.position[2] + dir[1] * 2.0,
            ];
            assert!(
                next[0].abs() <= ARENA_HALF_X
                    && next[1].abs() <= ARENA_HALF_Z
                    && next[0].abs() + next[1].abs() <= ARENA_CORNER_SUM,
                "wall-adjacent escape direction {:?} from {:?} pushes out of bounds — \
                 the boundary penalty is not bending the escape",
                dir,
                open_event.position
            );

            // And the window still buys separation (measured to the last
            // matched sample — the attacker may die mid-window in this comp).
            let (open, close) = windows
                .iter()
                .find(|(a, _)| (*a - open_event.sim_time).abs() < 1e-3)
                .copied()
                .expect("open event has a matching window span");
            let gained = separation_gained_during(
                &priest_samples,
                &warrior_samples,
                (open + gate, close + gate),
            )
            .expect("wall window must contain matched samples");
            eprintln!(
                "wall probe: window [{:.1},{:.1}]s opened at {:?} (boundary {:.2} away), \
                 dir {:?}, separation gained {:.1}",
                open,
                close,
                open_event.position,
                boundary_proximity(open_event.position),
                dir,
                gained
            );
            assert!(
                gained >= 3.0,
                "wall-adjacent window gained only {:.1} separation (floor 3.0)",
                gained
            );
        }
    }
}

// ---------------------------------------------------------------------------
// U7 — ESCAPE window math unit tests (pure, no Bevy world)
// ---------------------------------------------------------------------------

mod escape_window_math {
    use arenasim::states::play_match::class_ai::priest::{
        escape_distance_gained, escape_window,
    };

    /// (f) Window math: a 50% slow on the Priest halves the effective window
    /// distance.
    #[test]
    fn fifty_percent_slow_halves_effective_distance() {
        let full = escape_distance_gained(2.0, 5.0, 1.0);
        let slowed = escape_distance_gained(2.0, 5.0, 0.5);
        assert!((full - 10.0).abs() < 1e-6, "2s at speed 5 = 10 units, got {}", full);
        assert!(
            (slowed - full / 2.0).abs() < 1e-6,
            "50% slow must halve the distance: {} vs {}",
            slowed,
            full
        );
    }

    /// (g) Sub-cutoff windows do not trigger ESCAPE — including windows that
    /// are only sub-cutoff AFTER the slow adjustment.
    #[test]
    fn sub_cutoff_window_is_rejected() {
        // Raw window below the cutoff: rejected.
        assert_eq!(escape_window(&[Some(0.3)], 1.0, 0.5), None);
        // At/above the cutoff: accepted, raw duration returned.
        assert_eq!(escape_window(&[Some(0.6)], 1.0, 0.5), Some(0.6));
        // A 50% slow halves the effective window: 0.8s raw → 0.4s effective,
        // below the 0.5 cutoff → rejected.
        assert_eq!(escape_window(&[Some(0.8)], 0.5, 0.5), None);
        // 1.2s raw → 0.6s effective → accepted (and the RAW window is
        // returned: the slowed Priest still escapes for the full CC time).
        assert_eq!(escape_window(&[Some(1.2)], 0.5, 0.5), Some(1.2));
    }

    /// Multi-attacker rule and min-over-threats window duration.
    #[test]
    fn multi_attacker_rule_and_min_window() {
        // One unimpaired proximate threat voids the window.
        assert_eq!(escape_window(&[Some(4.0), None], 1.0, 0.5), None);
        // No proximate threat → nothing to escape from.
        assert_eq!(escape_window(&[], 1.0, 0.5), None);
        // Window = min over impaired threats (first to break free ends it).
        assert_eq!(escape_window(&[Some(4.0), Some(1.5)], 1.0, 0.5), Some(1.5));
    }
}

// ---------------------------------------------------------------------------
// U5 — movement config registration probe
// ---------------------------------------------------------------------------

/// (j) Headless mode loads `assets/config/movement.ron`. Two mechanisms make
/// a successful run the proof: `MovementConfigPlugin` panics if the file is
/// missing/malformed/invalid, and `run_headless_match_impl` carries a
/// `debug_assert!` that the `MovementConfig` resource exists (so deleting the
/// plugin registration fails this test under `cargo test`, where
/// debug_assertions are on).
#[test]
fn headless_runner_registers_movement_config() {
    let cfg = create_config(vec!["Warrior"], vec!["Mage"], Some(7));
    run_headless_match_with(cfg, true, None)
        .expect("headless run must succeed with MovementConfigPlugin registered");
}

// ---------------------------------------------------------------------------
// U5 — MovementDirective executor tests (World-level, minimal App/schedule)
// ---------------------------------------------------------------------------
//
// These drive `move_to_target` directly in a minimal Bevy App: MinimalPlugins
// for the clock (manual 1/60s steps, same strategy as the headless runner),
// gates forced open, and only the system under test registered. No class AI
// runs, so the directives injected here are the ONLY movement source — which
// is exactly the isolation the executor contract needs.

mod directive_executor {
    use std::time::Duration;

    use arenasim::states::play_match::abilities::AbilityType;
    use arenasim::states::play_match::combat_core::{move_to_target, DIRECTIVE_POINT_EPSILON};
    use arenasim::states::play_match::components::{
        ActiveAuras, Aura, AuraType, CastingState, Combatant, MatchCountdown, MovementDirective,
        MovementGoal,
    };
    use arenasim::CharacterClass;
    use bevy::prelude::*;
    use bevy::time::TimeUpdateStrategy;
    use bevy::MinimalPlugins;

    /// Minimal App that runs only `move_to_target` with gates open and a
    /// manual 1/60s clock (mirrors the headless runner's time strategy).
    fn executor_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 60.0,
            )))
            .insert_resource(MatchCountdown {
                time_remaining: 0.0,
                gates_opened: true,
            })
            .add_systems(Update, move_to_target);
        app
    }

    /// Spawn a combatant at `pos` with NO target (so the legacy ladder's
    /// no-target branch holds still while within 5 units of arena center —
    /// keeping the directive the only movement source in these tests).
    fn spawn_combatant(app: &mut App, pos: Vec3) -> (Entity, f32) {
        let combatant = Combatant::new(1, 0, CharacterClass::Priest);
        let speed = combatant.base_movement_speed;
        let entity = app
            .world_mut()
            .spawn((Transform::from_translation(pos), combatant))
            .id();
        (entity, speed)
    }

    fn now(app: &App) -> f32 {
        app.world().resource::<Time>().elapsed_secs()
    }

    fn pos_of(app: &App, entity: Entity) -> Vec3 {
        app.world().get::<Transform>(entity).unwrap().translation
    }

    fn slow_aura(magnitude: f32) -> ActiveAuras {
        ActiveAuras {
            auras: vec![Aura {
                effect_type: AuraType::MovementSpeedSlow,
                duration: 60.0,
                magnitude,
                ..Default::default()
            }],
        }
    }

    fn stun_aura() -> ActiveAuras {
        ActiveAuras {
            auras: vec![Aura {
                effect_type: AuraType::Stun,
                duration: 60.0,
                magnitude: 1.0,
                ..Default::default()
            }],
        }
    }

    /// (e) A Direction directive moves the entity at base speed: distance
    /// traveled equals base_movement_speed × elapsed sim time.
    #[test]
    fn direction_directive_moves_at_base_speed() {
        let mut app = executor_app();
        let start = Vec3::new(0.0, 1.0, 0.0);
        let (entity, speed) = spawn_combatant(&mut app, start);
        app.world_mut().entity_mut(entity).insert(MovementDirective {
            goal: MovementGoal::Direction(Vec2::new(1.0, 0.0)),
            expires: 100.0,
            committed_until: 100.0,
        });

        for _ in 0..30 {
            app.update();
        }

        let elapsed = now(&app);
        let pos = pos_of(&app, entity);
        let expected_x = speed * elapsed;
        assert!(
            (pos.x - expected_x).abs() < 1e-3,
            "expected x ≈ {} (speed {} × elapsed {}), got {}",
            expected_x,
            speed,
            elapsed,
            pos.x
        );
        assert!(pos.x > 1.0, "entity should have moved meaningfully");
        assert_eq!(pos.z, start.z, "Direction(+X) must not move on Z");
    }

    /// (e) Slow-adjusted speed: a 50% MovementSpeedSlow halves directive
    /// movement (mirrors the kiting branch's slow handling).
    #[test]
    fn direction_directive_respects_movement_slows() {
        let mut app = executor_app();
        let (entity, speed) = spawn_combatant(&mut app, Vec3::new(0.0, 1.0, 0.0));
        app.world_mut().entity_mut(entity).insert((
            MovementDirective {
                goal: MovementGoal::Direction(Vec2::new(1.0, 0.0)),
                expires: 100.0,
                committed_until: 100.0,
            },
            slow_aura(0.5),
        ));

        for _ in 0..30 {
            app.update();
        }

        let elapsed = now(&app);
        let pos = pos_of(&app, entity);
        let expected_x = speed * 0.5 * elapsed;
        assert!(
            (pos.x - expected_x).abs() < 1e-3,
            "expected slow-adjusted x ≈ {}, got {}",
            expected_x,
            pos.x
        );
    }

    /// (f) Expiry removes the directive WITHOUT executing it — including the
    /// stunned-past-deadline case: a directive issued pre-stun must be gone
    /// on the first post-stun frame, with zero movement along the stale
    /// vector. The expiry check sits ABOVE the root/stun early-continue.
    #[test]
    fn expired_directive_removed_while_stunned_no_stale_movement() {
        let mut app = executor_app();
        app.update(); // prime the clock so `now` is meaningful
        let start = Vec3::new(0.0, 1.0, 0.0);
        let (entity, _) = spawn_combatant(&mut app, start);
        let deadline = now(&app) + 0.1;
        app.world_mut().entity_mut(entity).insert((
            MovementDirective {
                goal: MovementGoal::Direction(Vec2::new(1.0, 0.0)),
                expires: deadline,
                committed_until: deadline,
            },
            stun_aura(),
        ));

        // Stunned across the deadline: no movement, and once sim time passes
        // `expires` the directive must be gone (removed, never executed).
        for _ in 0..30 {
            app.update();
        }
        assert!(now(&app) > deadline, "test must run past the deadline");
        assert_eq!(pos_of(&app, entity), start, "stunned entity must not move");
        assert!(
            app.world().get::<MovementDirective>(entity).is_none(),
            "expired directive must be removed even while the owner is stunned"
        );

        // First post-stun frame: still no movement along the stale vector
        // (the no-target legacy branch holds still this close to center).
        app.world_mut().entity_mut(entity).remove::<ActiveAuras>();
        app.update();
        assert_eq!(
            pos_of(&app, entity),
            start,
            "no movement along stale directive vector on first post-stun frame"
        );
    }

    /// (f) Plain expiry without CC: directive executes until the deadline,
    /// then is removed and movement stops.
    #[test]
    fn directive_expires_and_movement_stops() {
        let mut app = executor_app();
        app.update();
        let (entity, _) = spawn_combatant(&mut app, Vec3::new(0.0, 1.0, 0.0));
        let deadline = now(&app) + 0.2;
        app.world_mut().entity_mut(entity).insert(MovementDirective {
            goal: MovementGoal::Direction(Vec2::new(1.0, 0.0)),
            expires: deadline,
            committed_until: deadline,
        });

        for _ in 0..30 {
            app.update();
        }
        assert!(now(&app) > deadline);
        assert!(
            app.world().get::<MovementDirective>(entity).is_none(),
            "directive must be removed after expiry"
        );

        let frozen = pos_of(&app, entity);
        assert!(frozen.x > 0.0, "directive should have moved the entity before expiry");
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(pos_of(&app, entity), frozen, "movement must stop after expiry");
    }

    /// (g) Casting blocks directive execution (R12: the casting-locks-movement
    /// rule is preserved — the cast early-continue sits above the directive
    /// branch). The unexpired directive itself survives the cast.
    #[test]
    fn casting_blocks_directive_execution() {
        let mut app = executor_app();
        let start = Vec3::new(0.0, 1.0, 0.0);
        let (entity, _) = spawn_combatant(&mut app, start);
        app.world_mut().entity_mut(entity).insert((
            MovementDirective {
                goal: MovementGoal::Direction(Vec2::new(1.0, 0.0)),
                expires: 100.0,
                committed_until: 100.0,
            },
            CastingState::new(AbilityType::FlashHeal, entity, 2.0),
        ));

        for _ in 0..30 {
            app.update();
        }
        assert_eq!(pos_of(&app, entity), start, "casting must block directive movement");
        assert!(
            app.world().get::<MovementDirective>(entity).is_some(),
            "unexpired directive must survive the cast"
        );

        // Cast gap: once the cast ends, the directive executes.
        app.world_mut().entity_mut(entity).remove::<CastingState>();
        for _ in 0..10 {
            app.update();
        }
        assert!(
            pos_of(&app, entity).x > 0.0,
            "directive must execute in the cast gap"
        );
    }

    /// (h) A Point goal walks to the point and stops within
    /// DIRECTIVE_POINT_EPSILON, holding position afterwards (no oscillation).
    #[test]
    fn point_goal_stops_at_epsilon() {
        let mut app = executor_app();
        let (entity, speed) = spawn_combatant(&mut app, Vec3::new(0.0, 1.0, 0.0));
        let point = Vec3::new(3.0, 1.0, 1.0);
        app.world_mut().entity_mut(entity).insert(MovementDirective {
            goal: MovementGoal::Point(point),
            expires: 100.0,
            committed_until: 100.0,
        });

        // More than enough frames to cover the ~3.2-unit walk.
        let frames = ((4.0 / speed) * 60.0) as usize + 30;
        for _ in 0..frames {
            app.update();
        }

        let pos = pos_of(&app, entity);
        let xz_dist = ((pos.x - point.x).powi(2) + (pos.z - point.z).powi(2)).sqrt();
        assert!(
            xz_dist <= DIRECTIVE_POINT_EPSILON + 1e-4,
            "entity must stop within epsilon of the point, ended {} away",
            xz_dist
        );

        // Stable: holding at the point, no oscillation.
        let settled = pos_of(&app, entity);
        for _ in 0..20 {
            app.update();
        }
        assert_eq!(pos_of(&app, entity), settled, "entity must hold at the point");
    }
}
