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
            assert_eq!(
                x.survived, y.survived,
                "{}: team{} slot {} survived differs",
                context, team, slot
            );
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
    let cfg = create_config(vec!["Warrior", "Priest"], vec!["Mage", "Rogue"], Some(42));

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

    assert!(
        times.len() > 100,
        "expected a multi-second match, got {} frames",
        times.len()
    );

    // Monotonic sim time: never decreasing, and strictly increasing after the
    // first frame (Bevy's first Time update has zero delta).
    for w in times.windows(2) {
        assert!(
            w[1] >= w[0],
            "sim_time went backwards: {} -> {}",
            w[0],
            w[1]
        );
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

/// (R14 / plan U5 scenario h) The healer-posture directive machinery is wired
/// into the shared system schedule (MovementConfigPlugin, posture systems) but
/// must be a no-op for non-healer compositions: a match with no Priest/Paladin
/// touches none of the posture state. This pins that guarantee end to end —
/// the same fixed-seed Warrior v Mage match run twice yields bit-identical
/// outcomes, so the directive plumbing cannot have perturbed the sim.
#[test]
fn non_healer_outcomes_unchanged_by_directive_machinery() {
    let seed = 0x5EED_1234_u64;
    let make = || create_config(vec!["Warrior"], vec!["Mage"], Some(seed));

    let first = run_headless_match_with(make(), true, None).expect("first run");
    let second = run_headless_match_with(make(), true, None).expect("second run");

    assert_results_identical(&first, &second, "Warrior v Mage, two runs at one seed");
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
    assert!(
        (t - 3.0).abs() < 1e-6,
        "expected 3.0s within range, got {}",
        t
    );
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
    assert!(
        (t - 2.0).abs() < 1e-6,
        "expected 2.0s (b died at t=2), got {}",
        t
    );
}

#[test]
fn separation_gained_known_value() {
    // Distance grows 1.0 -> 5.0 across the window.
    let a: EntityTimeline = (0..5).map(|i| (i as f32, Vec3::ZERO)).collect();
    let b: EntityTimeline = (0..5)
        .map(|i| (i as f32, Vec3::new(1.0 + i as f32, 0.0, 0.0)))
        .collect();
    let gained = separation_gained_during(&a, &b, (0.0, 4.0)).expect("window has samples");
    assert!(
        (gained - 4.0).abs() < 1e-6,
        "expected +4.0 separation, got {}",
        gained
    );

    // Sub-window [1.0, 3.0]: distance 2.0 -> 4.0.
    let gained = separation_gained_during(&a, &b, (1.0, 3.0)).expect("sub-window has samples");
    assert!(
        (gained - 2.0).abs() < 1e-6,
        "expected +2.0 separation, got {}",
        gained
    );
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
    fn min_distance_series(timeline: &Timeline, me: Entity, others: &[Entity]) -> Vec<(f32, f32)> {
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

        // Hard-CC exclusion (added with the Rogue energy-pooling fix): the
        // Rogue now reliably lands Kidney Shot on the focused Priest, and a
        // STUNNED Priest cannot move — raw path length during stun windows
        // measures the CC, not the posture AI ("won't move" vs "can't
        // move"). Exclude [cast, cast + stun_duration] spans around every
        // enemy Rogue stun landing (the Rogue is forced onto the Priest, so
        // every stun it casts is on it) and assert the threshold over the
        // un-CC'd segments only. Trace sim_time is combat time — shift by
        // gate_time to compare against timeline timestamps.
        use arenasim::states::play_match::abilities::AbilityType;
        let ability_defs = arenasim::states::play_match::ability_config::load_ability_definitions()
            .expect("abilities.ron loads");
        let stun_spans: Vec<(f32, f32)> = trace
            .iter()
            .filter(|v| {
                v["kind"] == "ability_decision"
                    && v["actor"]["team"] == 2
                    && v["actor"]["class"] == "Rogue"
                    && v["outcome"]["type"] == "action_taken"
                    && matches!(
                        v["outcome"]["ability"].as_str(),
                        Some("KidneyShot") | Some("CheapShot")
                    )
            })
            .map(|v| {
                let t = v["sim_time"].as_f64().unwrap() as f32 + gate_time;
                let ability = match v["outcome"]["ability"].as_str().unwrap() {
                    "KidneyShot" => AbilityType::KidneyShot,
                    _ => AbilityType::CheapShot,
                };
                let dur = ability_defs
                    .get_unchecked(&ability)
                    .applies_aura
                    .as_ref()
                    .map(|a| a.duration)
                    .unwrap_or(0.0);
                (t, t + dur)
            })
            .collect();

        let in_stun = |t: f32| stun_spans.iter().any(|(a, b)| t >= *a && t <= *b);
        let mut free_path = 0.0_f32;
        let mut free_secs = 0.0_f32;
        let mut seg_start = None::<usize>;
        let close_seg = |start: usize, end: usize, fp: &mut f32, fs: &mut f32| {
            let seg = &post_gate[start..end];
            if seg.len() >= 2 {
                *fp += path_length(seg);
                *fs += seg.last().unwrap().0 - seg.first().unwrap().0;
            }
        };
        for (i, s) in post_gate.iter().enumerate() {
            if in_stun(s.0) {
                if let Some(start) = seg_start.take() {
                    close_seg(start, i, &mut free_path, &mut free_secs);
                }
            } else if seg_start.is_none() {
                seg_start = Some(i);
            }
        }
        if let Some(start) = seg_start {
            close_seg(start, post_gate.len(), &mut free_path, &mut free_secs);
        }

        // RATE, not absolute distance: a working Rogue (energy pooling lands
        // Kidney Shot now) halves the focused Priest's survival, so any
        // absolute path threshold conflates mobility with lifespan. The
        // statue band is ~0.65 units per un-CC'd second (21 units / ~32s);
        // healthy post-U6 movement measures ~2.8-3.3 u/s. Threshold 1.5 sits
        // well above statue, well below healthy.
        let rate = free_path / free_secs.max(f32::EPSILON);
        eprintln!(
            "statue probe: un-CC'd path={:.1} over {:.1}s free = {:.2} u/s \
             ({} stun span(s) excluded; raw path={:.1})",
            free_path,
            free_secs,
            rate,
            stun_spans.len(),
            path
        );

        assert!(
            rate > 1.5,
            "statue pathology: focused Priest moved {:.2} units per un-CC'd \
             second ({:.1} units / {:.1}s free; statue band ~0.65, healthy \
             ~2.8+, threshold 1.5)",
            rate,
            free_path,
            free_secs
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
    /// Grace widened 1.0s → 2.5s (2026-06-14) for Psychic Scream: when the
    /// Priest fears an attacker, the melee ally chases the feared enemy out of
    /// the Priest's 40yd heal range for a beat while the Priest re-anchors
    /// (~2s observed). The invariant the probe protects — the Priest does not
    /// ABANDON its ally — still holds; the wider grace tolerates the transient
    /// fear-scatter without masking a sustained walk-off.
    #[test]
    fn pressured_priest_stays_in_heal_range_of_ally() {
        let (_result, timeline, trace) = run_observed_traced(statue_config());
        let gate_time = timeline.gates_open_time.expect("gates opened");

        let priest = timeline.find(1, CharacterClass::Priest, false);
        let warrior = timeline.find(1, CharacterClass::Warrior, false);
        let events = movement_events(&trace);

        // Last priest sample (combat time) bounds any unclosed window.
        let priest_samples = timeline.samples.get(&priest).cloned().unwrap_or_default();
        let end = priest_samples
            .last()
            .map(|(t, _)| t - gate_time)
            .unwrap_or(0.0);
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
                out_of_range <= 2.5,
                "PRESSURED Priest left heal range (40) of its ally for {:.2}s \
                 (grace 2.5s) during window [{:.1},{:.1}]",
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
        // Pin the Ambush opener: this probe tests the Priest's stealth-opener
        // posture filtering, not the Rogue's kit. Ambush is the canonical
        // stealth-burst opener and was the project default when this probe was
        // written; the new CheapShot→Kidney default (a 10s stun lockdown that
        // suppresses the PRESSURED transition) is covered by the rogue_chain
        // probes instead.
        cfg.team2_rogue_openers = vec!["Ambush".to_string()];

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
    ///
    /// Measured SIDE-SYMMETRIZED (mean of both mirror sides) per the
    /// mirror-asymmetry protocol — same-frame action races resolve in ECS
    /// iteration order and bias one side by several points, so a per-side
    /// ceiling is fragile when the true rate sits near it. After the B8
    /// stale-directive fix (2026-06-09) the Priest anchors to its formation
    /// point on FREE entry instead of coasting on ~1s of residual PRESSURED
    /// repulsion, which raised the symmetrized rate from ~40% to ~49.5%; the
    /// 50% ceiling still guards genuine over-firing. The consolidated matrix
    /// pass is the authoritative balance check on that shift.
    /// Ceiling raised 50% → 65% (2026-06-14) for Psychic Scream. The driver is
    /// the DEFENSIVE scream, not the dip (the dip stays home in an unforced
    /// mirror — it respects the kill target, which is the enemy healer here):
    /// without the scream this seed-11 mirror resolves in <20s, but the panic
    /// button lets both healers survive into a long contested ~44s match, so
    /// the PRESSURED *fraction* rises (62% observed). The 2v2/3v3 sweep
    /// validated this as net-positive with baseline draw rates, so the mirror
    /// is more contested, not stalled. The guard still catches egregious
    /// over-firing (>65%).
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
        let mut fracs = [0.0f32; 2];
        for (i, team) in [1u8, 2u8].into_iter().enumerate() {
            let windows = pressured_windows(&events, team, 1, result.match_time);
            let pressured: f32 = windows.iter().map(|(a, b)| b - a).sum();
            fracs[i] = pressured / result.match_time;
            eprintln!(
                "time-in-FREE probe: team{} Priest pressured {:.1}s of {:.1}s ({:.0}%)",
                team,
                pressured,
                result.match_time,
                fracs[i] * 100.0
            );
        }
        let symmetrized = (fracs[0] + fracs[1]) / 2.0;
        eprintln!(
            "time-in-FREE probe: side-symmetrized {:.0}% PRESSURED",
            symmetrized * 100.0
        );
        assert!(
            symmetrized < 0.65,
            "Priest spent {:.0}% of the match PRESSURED (side-symmetrized) in an \
             unforced mirror (ceiling 65% — the trigger is over-firing)",
            symmetrized * 100.0
        );
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
    use super::priest_postures::{
        movement_events, pressured_windows, run_observed_traced, MovementEvent,
    };
    use super::*;
    use arenasim::states::play_match::constants::{ARENA_CORNER_SUM, ARENA_HALF_X, ARENA_HALF_Z};
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
                v["candidates"].as_array().is_some_and(|cands| {
                    cands.iter().any(|c| {
                        c["ability"] == "FlashHeal"
                            && c["reason"]["PreconditionUnmet"]["note"]
                                .as_str()
                                // The posture's deferral note ("dip/escape/dispel
                                // walk live: ..."); the in-window filter is what
                                // ties it to ESCAPE.
                                .is_some_and(|n| n.ends_with("non-critical heal deferred"))
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
    ///
    /// Re-pinned for AS-167, weapon speed from item data (1 -> 4). At seed 1 a
    /// Flash Heal now starts in-window at the Priest's own 48% HP — below the
    /// 0.5 urgency threshold, so a CRITICAL heal, which probe (c) requires be
    /// allowed; this seed's "no sub-threshold moment in-window" premise is
    /// what moved. The first line that differs from `main` @ 17cb9f0 is the
    /// Priest's first Wand Shot, 8 -> 10 (the Staff of Dominance fires every
    /// 1.6s for the same DPS). Seed 4 from `scan_escape_defer_seeds`: one
    /// escape window, 20 in-window deferrals, no in-window Flash Heal start.
    #[test]
    fn live_window_defers_noncritical_heals() {
        let (result, _timeline, trace) = run_observed_traced(escape_config(4));

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

    /// Exploratory scan for the defer probe's seed (re-pin
    /// `live_window_defers_noncritical_heals` when trajectories drift): a
    /// candidate has an escape window, an in-window deferral, and no Flash
    /// Heal starting in-window — i.e. no sub-threshold moment lands inside a
    /// window, which would legitimately start a critical heal there (probe c).
    /// Ignored by default.
    #[test]
    #[ignore]
    fn scan_escape_defer_seeds() {
        for seed in 0u64..60 {
            let (result, _timeline, trace) = run_observed_traced(escape_config(seed));
            let events = movement_events(&trace);
            let windows = escape_window_spans(&events, 1, 0, result.match_time);
            let defers = flash_heal_defer_times(&trace)
                .into_iter()
                .filter(|t| in_window(*t, &windows))
                .count();
            let starts = flash_heal_starts(&trace)
                .into_iter()
                .filter(|(t, _)| in_window(*t, &windows))
                .count();
            if !windows.is_empty() && defers >= 1 && starts == 0 {
                eprintln!(
                    "seed {seed:2}: windows={} in-window defers={defers} <-- CANDIDATE",
                    windows.len()
                );
            }
        }
    }

    /// (c) CRITICAL-HEAL PROBE (AE1) — an ally below the urgency threshold
    /// during a live window is healed anyway: a Flash Heal STARTS in-window.
    ///
    /// STAGED (tier 3): the Priest, the lowest ally at 20%, with the enemy
    /// Rogue on it held by the Paladin's Hammer of Justice — a threat in its
    /// face, every proximate threat impaired, so the window opens — and the
    /// enemy Mage out of the picture. The seeded match this replaced (Priest+
    /// Paladin v Rogue+Mage, pinned at 14, then 5, then 16) carried the
    /// moment on a knife-edge of timing: on `main` the window opened at 6.8s
    /// with the Priest at 66% and a Flash Heal started at 9.85s in nearly
    /// every seed, and AS-249's healer positioning shifted that by half a
    /// second in all but one of 40. The critical-heal-wins invariant itself
    /// is preserved by the scream's critical-heal-pending defer gate in
    /// `try_psychic_scream`.
    #[test]
    fn critical_heal_fires_despite_live_window() {
        use arenasim::headless::{Stage, Unit};
        use arenasim::states::play_match::abilities::AbilityType;
        const PRIEST: Unit = Unit::new(1, 0);
        const PALADIN: Unit = Unit::new(1, 1);
        const ROGUE: Unit = Unit::new(2, 0);
        const MAGE: Unit = Unit::new(2, 1);
        let threshold = load_movement_config()
            .expect("movement.ron loads")
            .shared
            .urgency_hp_threshold;

        for seed in [1u64, 2, 3] {
            let mut cfg =
                create_config(vec!["Priest", "Paladin"], vec!["Rogue", "Mage"], Some(seed));
            cfg.max_duration_secs = 15.0;
            let stage = Stage::new()
                .place(PRIEST, -10.0, 0.0)
                .health(PRIEST, 0.2)
                // Mid-GCD, so the posture opens its window before the first
                // heal can go out — the heal is then a decision made inside it.
                .combatant(PRIEST, |c| c.global_cooldown = 0.5)
                .place(ROGUE, -8.0, 0.0)
                .clear_auras(ROGUE)
                .target(ROGUE, PRIEST)
                .land(ROGUE, AbilityType::HammerOfJustice, PALADIN)
                .place(PALADIN, -6.0, 3.0)
                .place(MAGE, 35.0, 15.0);
            let tmp = tempfile::NamedTempFile::new().unwrap();
            let path = tmp.path().to_path_buf();
            drop(tmp);
            let result = arenasim::headless::run_headless_match_staged(
                cfg,
                true,
                Some(arenasim::headless::runner::TraceConfig {
                    output_path: path.clone(),
                }),
                stage,
                |_| {},
            )
            .expect("staged match");
            let trace: Vec<serde_json::Value> = std::fs::read_to_string(&path)
                .expect("read trace file")
                .lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect();
            let _ = std::fs::remove_file(path);

            let events = movement_events(&trace);
            let windows = escape_window_spans(&events, 1, 0, result.match_time);
            assert_min_occurrences(&format!("seed {seed} escape windows"), windows.len(), 1);
            let critical_starts: Vec<(f32, f32)> = flash_heal_starts(&trace)
                .into_iter()
                .filter(|(t, hp)| in_window(*t, &windows) && *hp <= threshold)
                .collect();
            eprintln!(
                "critical-heal probe seed {seed}: windows {windows:?}, in-window \
                 sub-threshold Flash Heal starts {critical_starts:?}"
            );
            assert_min_occurrences(
                &format!("seed {seed} in-window critical Flash Heal starts"),
                critical_starts.len(),
                1,
            );
        }
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
                        .min_by(|a, b| (a.0 - t).abs().partial_cmp(&(b.0 - t).abs()).unwrap())
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
    use arenasim::states::play_match::class_ai::priest::{escape_distance_gained, escape_window};

    /// (f) Window math: a 50% slow on the Priest halves the effective window
    /// distance.
    #[test]
    fn fifty_percent_slow_halves_effective_distance() {
        let full = escape_distance_gained(2.0, 5.0, 1.0);
        let slowed = escape_distance_gained(2.0, 5.0, 0.5);
        assert!(
            (full - 10.0).abs() < 1e-6,
            "2s at speed 5 = 10 units, got {}",
            full
        );
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
    use arenasim::states::play_match::map_config::ActiveMapGeometry;
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
            // move_to_target reads the active map's obstacle volumes; an empty
            // default preserves the obstacle-free behavior these probes expect.
            .insert_resource(ActiveMapGeometry::default())
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
        app.world_mut()
            .entity_mut(entity)
            .insert(MovementDirective {
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
        app.world_mut()
            .entity_mut(entity)
            .insert(MovementDirective {
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
        assert!(
            frozen.x > 0.0,
            "directive should have moved the entity before expiry"
        );
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(
            pos_of(&app, entity),
            frozen,
            "movement must stop after expiry"
        );
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
        assert_eq!(
            pos_of(&app, entity),
            start,
            "casting must block directive movement"
        );
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
        app.world_mut()
            .entity_mut(entity)
            .insert(MovementDirective {
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
        assert_eq!(
            pos_of(&app, entity),
            settled,
            "entity must hold at the point"
        );
    }

    // -----------------------------------------------------------------------
    // AS-181 — a mover flush against a Nagrand pillar FACE with its goal
    // straight behind the pillar rounds it on one side. Tangent steering once
    // compared the two tangent vertices by heading alignment, which on a flat
    // face is anti-self-reinforcing: a step toward one end tilts the goal
    // toward the OTHER end, so the choice flipped every tick and the mover
    // jittered in place against the face.
    // -----------------------------------------------------------------------

    /// The real PillaredArena geometry and its pillar at (40, 20): an octagon
    /// of circumradius 6 turned so a flat face points down each axis. Returns
    /// the geometry, the pillar centre, and the distance from that centre to a
    /// flat face's collision shell.
    fn nagrand() -> (ActiveMapGeometry, Vec2, f32) {
        use arenasim::states::play_match::map_config::load_map_geometry_config;
        use arenasim::states::play_match::map_geometry::{prism_apothem, MOVER_RADIUS};
        use arenasim::ArenaMap;
        let geometry = load_map_geometry_config()
            .expect("maps.ron loads")
            .active_for(ArenaMap::PillaredArena);
        (
            geometry,
            Vec2::new(40.0, 20.0),
            prism_apothem(6.0, 8) + MOVER_RADIUS,
        )
    }

    /// How many times the sideways component of the mover's per-tick step,
    /// relative to the line `start -> goal`, reverses sign (steps under 1mm
    /// sideways are ignored).
    fn side_reversals(track: &[Vec3], start: Vec2, goal: Vec2) -> usize {
        let line = (goal - start).normalize();
        let mut last = 0.0_f32;
        let mut reversals = 0;
        for w in track.windows(2) {
            let step = Vec2::new(w[1].x - w[0].x, w[1].z - w[0].z);
            let lateral = line.perp_dot(step);
            if lateral.abs() < 1e-3 {
                continue;
            }
            if last != 0.0 && lateral.signum() != last {
                reversals += 1;
            }
            last = lateral.signum();
        }
        reversals
    }

    /// Run `ticks` frames, recording the mover's position before and after each.
    fn track(app: &mut App, entity: Entity, ticks: usize) -> Vec<Vec3> {
        let mut out = vec![pos_of(app, entity)];
        for _ in 0..ticks {
            app.update();
            out.push(pos_of(app, entity));
        }
        out
    }

    /// A Point walk (the chase / medic / formation directive) from flush against
    /// the pillar's -x face, to a spot 3yd past its +x face on the same line.
    /// The mover commits to one side, rounds the pillar and arrives.
    #[test]
    fn point_walk_flush_against_a_nagrand_face_rounds_it_without_flipping() {
        let (geometry, center, shell) = nagrand();
        let mut app = executor_app();
        app.insert_resource(geometry);
        let start = Vec3::new(center.x - shell - 0.01, 1.0, center.y);
        let point = Vec3::new(center.x + shell + 3.0, 1.0, center.y);
        let (entity, _) = spawn_combatant(&mut app, start);
        app.world_mut()
            .entity_mut(entity)
            .insert(MovementDirective {
                goal: MovementGoal::Point(point),
                expires: 100.0,
                committed_until: 100.0,
            });

        // 5s at run speed is several times the ~20yd way round.
        let path = track(&mut app, entity, 300);
        let flips = side_reversals(
            &path,
            Vec2::new(start.x, start.z),
            Vec2::new(point.x, point.z),
        );
        let end = *path.last().unwrap();
        let off = Vec2::new(end.x - point.x, end.z - point.z).length();
        assert!(
            flips <= 1,
            "the walk reversed sideways {flips} times against the face"
        );
        assert!(
            off <= DIRECTIVE_POINT_EPSILON + 1e-3,
            "the walk ended {off:.2}yd short of its point, at {end}"
        );
    }

    /// The same setup through normal pursuit: a Warrior chasing a target that
    /// stands (stunned, so it stays put) straight behind the pillar.
    #[test]
    fn pursuit_flush_against_a_nagrand_face_rounds_it_without_flipping() {
        let (geometry, center, shell) = nagrand();
        let mut app = executor_app();
        app.insert_resource(geometry);
        let start = Vec3::new(center.x - shell - 0.01, 1.0, center.y);
        let target_pos = Vec3::new(center.x + shell + 3.0, 1.0, center.y);
        let target = app
            .world_mut()
            .spawn((
                Transform::from_translation(target_pos),
                Combatant::new(2, 0, CharacterClass::Priest),
                stun_aura(),
            ))
            .id();
        let mut chaser = Combatant::new(1, 0, CharacterClass::Warrior);
        chaser.target = Some(target);
        let reach = CharacterClass::Warrior.preferred_range();
        let entity = app
            .world_mut()
            .spawn((Transform::from_translation(start), chaser))
            .id();

        let path = track(&mut app, entity, 300);
        let flips = side_reversals(
            &path,
            Vec2::new(start.x, start.z),
            Vec2::new(target_pos.x, target_pos.z),
        );
        let end = *path.last().unwrap();
        let off = Vec2::new(end.x - target_pos.x, end.z - target_pos.z).length();
        assert!(
            flips <= 1,
            "the chase reversed sideways {flips} times against the face"
        );
        assert!(
            off <= reach + 0.1,
            "the chase ended {off:.2}yd from its target (reach {reach}), at {end}"
        );
    }

    /// A Point walk to a spot INSIDE the pillar's footprint — a point no one
    /// can stand on, which TeamPlan and Legacy formation walks do produce.
    /// The mover walks to the nearest ground it can reach and stands still
    /// there, instead of stepping back and forth across the pillar corner
    /// nearest the spot. The spot is the one measured in a TeamPlan match
    /// (0.05yd inside the shell of a diagonal face, near its corner).
    #[test]
    fn point_walk_to_a_spot_inside_a_nagrand_pillar_stands_still_beside_it() {
        use arenasim::states::play_match::map_geometry::{position_blocked, MOVER_RADIUS};
        let (geometry, center, _) = nagrand();
        let mut app = executor_app();
        let point = Vec3::new(center.x + 4.630_82, 1.0, center.y - 3.848_33);
        assert!(
            position_blocked(&geometry.volumes, point),
            "test setup: the spot is inside the pillar"
        );
        app.insert_resource(geometry);
        let start = Vec3::new(center.x + 12.0, 1.0, center.y + 3.0);
        let (entity, speed) = spawn_combatant(&mut app, start);
        app.world_mut()
            .entity_mut(entity)
            .insert(MovementDirective {
                goal: MovementGoal::Point(point),
                expires: 100.0,
                committed_until: 100.0,
            });

        let path = track(&mut app, entity, 300);
        let mut backtracks = 0;
        let mut last = Vec2::ZERO;
        for w in path.windows(2) {
            let step = Vec2::new(w[1].x - w[0].x, w[1].z - w[0].z);
            if step == Vec2::ZERO {
                continue;
            }
            if step.dot(last) < 0.0 {
                backtracks += 1;
            }
            last = step;
        }
        assert_eq!(backtracks, 0, "the walk stepped back on itself");
        // Standing still for the last second, a body radius (plus a step)
        // from the spot.
        let rest = path[path.len() - 61];
        assert!(
            path[path.len() - 60..].iter().all(|p| *p == rest),
            "still moving at the end of the walk"
        );
        let off = Vec2::new(rest.x - point.x, rest.z - point.z).length();
        assert!(
            off <= MOVER_RADIUS + speed / 60.0 + 0.1,
            "stood {off:.2}yd from the spot, at {rest}"
        );
    }
}

// ---------------------------------------------------------------------------
// U8 — Paladin postures and the HoJ DIP
// ---------------------------------------------------------------------------
//
// Seed notes (scanned seeds 1..15 per comp during development):
//
// - Dip comp (a): Paladin+Warrior vs Priest+Warrior, both kill targets on
//   index 1 (so neither healer is the formal kill target — the dip is the
//   only path to HoJ on the enemy Priest). At every scanned seed the Paladin
//   walks from spawn and lands HoJ on the enemy Priest (entity 2) at
//   ~6.7s combat time: a clean DipEnter→HoJ-on-healer→DipComplete cycle,
//   after which FREE legacy pursuit walks it back to its kill target
//   (measured min distance to the kill-target Warrior drops to ~1.9 within
//   15s of DipComplete). Seed 1 pinned.
// - Dip-abort (b): no longer a seeded comp. Both aborts — the budget and the
//   teammate-HP dive (AE3) — and the abort that casts nothing are STAGED in
//   `tests/paladin_dip_abort_band.rs`, which carries the Paladin into the
//   stage mid-dip, so neither waits on a seed to put a dip in flight.
// - Preempt comp (c): Paladin+Warrior vs Priest+Warrior, enemy forced onto
//   the Paladin (team2_kill_target 0). The enemy Warrior reaches the Paladin
//   mid-dip (~3.9s) and the dip is preempted by PressuredEnter with no
//   intervening DipComplete. Seed 1 pinned.
// - Retreat comp (d): same as (c) — the focused Paladin falls back toward
//   fallback_range (15) and keeps healing (mean distance to attacker ~9,
//   heals continue during PRESSURED).
// - Identity comp (e): Paladin+Warrior vs Warrior+Rogue (NO enemy healer),
//   unforced. Paladin melee uptime stays high (>50% of post-contact time
//   within 4yd of an enemy while the team is healthy) — the healing-heavy
//   trigger requires BOTH a hurting teammate AND a proximate melee, so a
//   healthy melee scrum never flips the posture.
// - Chip comp (f): the dip comp (a) — the enemy Warrior chips the Paladin's
//   Warrior teammate during the dip but keeps it above the urgency
//   threshold, so the dip completes anyway (cast deferral holds).
// - Self-peel comp (g): the preempt comp (c) — a focused Paladin with the
//   enemy Priest alive still lands HoJ on its own attacker (reservation
//   released under PRESSURED).

mod paladin_postures {
    use super::priest_postures::{
        movement_events, pressured_windows, run_observed_traced, MovementEvent,
    };
    use super::*;

    /// Paladin (team 1 slot 0) HoJ casts: (combat-time, target entity_id).
    fn paladin_hoj_casts(trace: &[serde_json::Value]) -> Vec<(f32, u64)> {
        trace
            .iter()
            .filter(|v| {
                v["kind"] == "ability_decision"
                    && v["actor"]["team"] == 1
                    && v["actor"]["slot"] == 0
                    && v["actor"]["class"] == "Paladin"
                    && v["outcome"]["type"] == "action_taken"
                    && v["outcome"]["ability"] == "HammerOfJustice"
            })
            .map(|v| {
                (
                    v["sim_time"].as_f64().unwrap() as f32,
                    v["outcome"]["target_id"].as_u64().unwrap_or(u64::MAX),
                )
            })
            .collect()
    }

    /// Team-2 entity_id for (class, slot) from any trace event's actor view.
    fn entity_of(trace: &[serde_json::Value], team: u8, class: &str, slot: u8) -> u64 {
        trace
            .iter()
            .find(|v| {
                v["actor"]["team"] == team
                    && v["actor"]["class"] == class
                    && v["actor"]["slot"] == slot as u64
            })
            .map(|v| v["actor"]["entity_id"].as_u64().unwrap())
            .expect("entity present in trace")
    }

    /// Paladin dip spans (combat time) from DipEnter / DipComplete / DipAbort
    /// / (preempt) PressuredEnter, with the closing trigger recorded.
    /// `("complete"|"abort"|"preempt"|"open")`.
    fn dip_spans(events: &[MovementEvent], end: f32) -> Vec<(f32, f32, &'static str)> {
        let mut spans = Vec::new();
        let mut open: Option<f32> = None;
        for e in events.iter().filter(|e| e.team == 1 && e.slot == 0) {
            match e.trigger.as_str() {
                "DipEnter" => open = Some(e.sim_time),
                "DipComplete" => {
                    if let Some(s) = open.take() {
                        spans.push((s, e.sim_time, "complete"));
                    }
                }
                "DipAbort" => {
                    if let Some(s) = open.take() {
                        spans.push((s, e.sim_time, "abort"));
                    }
                }
                "PressuredEnter" => {
                    if let Some(s) = open.take() {
                        spans.push((s, e.sim_time, "preempt"));
                    }
                }
                _ => {}
            }
        }
        if let Some(s) = open {
            spans.push((s, end, "open"));
        }
        spans
    }

    fn dip_config(seed: u64) -> HeadlessMatchConfig {
        let mut cfg = create_config(
            vec!["Paladin", "Warrior"],
            vec!["Priest", "Warrior"],
            Some(seed),
        );
        // Neither healer is the formal kill target — the dip is the only
        // path to HoJ on the enemy Priest.
        cfg.team1_kill_target = Some(1);
        cfg.team2_kill_target = Some(1);
        cfg
    }

    fn preempt_config(seed: u64) -> HeadlessMatchConfig {
        let mut cfg = create_config(
            vec!["Paladin", "Warrior"],
            vec!["Priest", "Warrior"],
            Some(seed),
        );
        cfg.team1_kill_target = Some(1);
        cfg.team2_kill_target = Some(0); // enemy trains the Paladin
        cfg
    }

    /// (a) DIP PROBE — a full DipEnter → HoJ-on-the-enemy-healer →
    /// DipComplete cycle completes, and the Paladin returns toward its kill
    /// target afterward.
    #[test]
    fn dip_cycle_stuns_enemy_healer_and_returns() {
        let (result, timeline, trace) = run_observed_traced(dip_config(1));
        let events = movement_events(&trace);
        let spans = dip_spans(&events, result.match_time);

        let completed: Vec<_> = spans.iter().filter(|(_, _, k)| *k == "complete").collect();
        assert_min_occurrences("completed Paladin dips", completed.len(), 1);

        // HoJ landed on the enemy Priest inside the completed dip span.
        let priest_id = entity_of(&trace, 2, "Priest", 0);
        let hojs = paladin_hoj_casts(&trace);
        let in_dip_on_healer = hojs.iter().any(|(t, tgt)| {
            *tgt == priest_id
                && completed
                    .iter()
                    .any(|(s, e, _)| *t >= *s - 1e-3 && *t <= *e + 1e-3)
        });
        eprintln!(
            "dip probe: spans={:?} hojs={:?} enemy_priest=e{}",
            spans, hojs, priest_id
        );
        assert!(
            in_dip_on_healer,
            "no DipEnter→HoJ-on-enemy-Priest→DipComplete cycle: hojs={:?} dips={:?}",
            hojs, completed
        );

        // Returns toward the kill target (enemy Warrior, slot 1) after the
        // first DipComplete: the min Paladin→kill-target distance drops into
        // melee range within 15s.
        let gate = timeline.gates_open_time.expect("gates opened");
        let complete_t = completed[0].1;
        let paladin = timeline.find(1, CharacterClass::Paladin, false);
        let kt = timeline.find(2, CharacterClass::Warrior, false);
        let ps = timeline.samples.get(&paladin).cloned().unwrap_or_default();
        let ks = timeline.samples.get(&kt).cloned().unwrap_or_default();
        let dist_at = |t: f32| -> Option<f32> {
            let p = ps
                .iter()
                .min_by(|a, b| (a.0 - t).abs().partial_cmp(&(b.0 - t).abs()).unwrap())?;
            let k = ks
                .iter()
                .min_by(|a, b| (a.0 - t).abs().partial_cmp(&(b.0 - t).abs()).unwrap())?;
            Some(p.1.distance(k.1))
        };
        let dmin: f32 = (0..150)
            .filter_map(|i| dist_at(complete_t + gate + i as f32 * 0.1))
            .fold(f32::MAX, f32::min);
        eprintln!(
            "dip probe: post-DipComplete min dist to kill target = {:.1}",
            dmin
        );
        assert!(
            dmin <= 5.0,
            "Paladin did not return toward its kill target after the dip \
             (min distance {:.1} > 5.0 over 15s)",
            dmin
        );
    }

    // (b) DIP ABORT PROBE — migrated off its pinned seed to staged matches:
    // `tests/paladin_dip_abort_band.rs`, the plain aborts (budget and dying
    // teammate) with no caster in the band, and `an_abort_in_hammer_reach_
    // casts_nothing`, an abort that has arrived in Hammer of Justice's reach.

    /// (c) PREEMPT PROBE — the Paladin becomes the kill target mid-dip and
    /// PressuredEnter replaces the dip with no intervening DipComplete.
    #[test]
    fn focus_mid_dip_preempts_with_pressured() {
        let (result, _timeline, trace) = run_observed_traced(preempt_config(1));
        let events = movement_events(&trace);
        let spans = dip_spans(&events, result.match_time);
        eprintln!("preempt probe: spans={:?}", spans);

        let preempts = spans.iter().filter(|(_, _, k)| *k == "preempt").count();
        assert_min_occurrences("preempted Paladin dips", preempts, 1);

        // The preempting transition is PressuredEnter (DIP→PRESSURED), never
        // a DipComplete or DipAbort, for the span that got preempted.
        // Structurally guaranteed by dip_spans' classification, so the
        // assertion above suffices; this is the readable restatement.
        for (s, e, k) in &spans {
            if *k == "preempt" {
                eprintln!(
                    "preempt probe: dip [{:.1},{:.1}] replaced by PressuredEnter",
                    s, e
                );
            }
        }
    }

    /// (d) RETREAT PROBE — a focused Paladin falls back toward fallback_range
    /// and keeps healing: mean distance to its attacker during PRESSURED
    /// sits in the fallback band (well above melee), and heals fire while
    /// PRESSURED.
    #[test]
    fn focused_paladin_retreats_and_keeps_healing() {
        use arenasim::states::play_match::movement_config::load_movement_config;
        let fallback = load_movement_config().unwrap().paladin.fallback_range;

        let (result, timeline, trace) = run_observed_traced(preempt_config(1));
        let gate = timeline.gates_open_time.expect("gates opened");
        let events = movement_events(&trace);
        let windows = pressured_windows(&events, 1, 0, result.match_time);
        assert_min_occurrences("Paladin PRESSURED windows", windows.len(), 1);

        let paladin = timeline.find(1, CharacterClass::Paladin, false);
        let atk = timeline.find(2, CharacterClass::Warrior, false);
        let ps = timeline.samples.get(&paladin).cloned().unwrap_or_default();
        let ks = timeline.samples.get(&atk).cloned().unwrap_or_default();

        let mut dist_sum = 0.0f32;
        let mut n = 0usize;
        for (a, b) in &windows {
            let (w0, w1) = (a + gate, b + gate);
            for (t, p) in ps.iter().filter(|(t, _)| *t >= w0 && *t <= w1) {
                if let Some((_, kp)) = ks
                    .iter()
                    .min_by(|x, y| (x.0 - t).abs().partial_cmp(&(y.0 - t).abs()).unwrap())
                {
                    dist_sum += p.distance(*kp);
                    n += 1;
                }
            }
        }
        let mean = dist_sum / n.max(1) as f32;

        // Heals fired while PRESSURED.
        let heals_in_pressured = trace
            .iter()
            .filter(|v| {
                v["kind"] == "ability_decision"
                    && v["actor"]["team"] == 1
                    && v["actor"]["slot"] == 0
                    && v["outcome"]["type"] == "action_taken"
                    && matches!(
                        v["outcome"]["ability"].as_str(),
                        Some("FlashOfLight") | Some("HolyLight") | Some("HolyShock")
                    )
            })
            .filter(|v| {
                let t = v["sim_time"].as_f64().unwrap() as f32;
                windows.iter().any(|(a, b)| t >= *a && t <= *b)
            })
            .count();

        eprintln!(
            "retreat probe: mean dist to attacker during PRESSURED = {:.1} (fallback {}), \
             heals in PRESSURED = {}",
            mean, fallback, heals_in_pressured
        );
        // Retreat band: the Paladin is no longer face-tanking. A melee
        // attacker chases at equal speed, so the Paladin can't sit at the
        // full fallback distance — but it should average well above melee
        // (4yd). Floor of 6yd: a clear retreat, headroom for chase dynamics.
        assert!(
            mean >= 6.0,
            "PRESSURED Paladin averaged only {:.1}yd from its attacker (floor 6.0) — \
             it is still face-tanking",
            mean
        );
        assert_min_occurrences("heals during PRESSURED", heals_in_pressured, 1);
    }

    /// (e) IDENTITY PROBE — team healthy, NO enemy healer: the Paladin keeps
    /// its melee identity. Asserts an absolute healthy floor (>50% of
    /// post-contact, team-healthy time within 4yd of an enemy) per the plan's
    /// fallback when a same-seed baseline binary isn't built here. The
    /// healing-heavy trigger needs BOTH a hurting teammate and a proximate
    /// melee, so a healthy scrum never flips the posture.
    #[test]
    fn healthy_no_healer_preserves_melee_identity() {
        let mut cfg = create_config(
            vec!["Paladin", "Warrior"],
            vec!["Warrior", "Rogue"],
            Some(1),
        );
        // This probe measures the Paladin's melee identity vs a melee comp; the
        // Rogue is incidental melee pressure. Pin its original Ambush opener (the
        // default when this probe was written) so the pressure profile is stable
        // and decoupled from the new CheapShot→Kidney stun-chain default, which
        // the rogue_chain probes cover.
        cfg.team2_rogue_openers = vec!["Ambush".to_string(), "Ambush".to_string()];
        let (result, timeline, trace) = run_observed_traced(cfg);
        let gate = timeline.gates_open_time.expect("gates opened");

        // team-1 HP series (last-known per slot) to gate "team healthy".
        let mut hp_events: Vec<(f32, u8, f32)> = trace
            .iter()
            .filter(|v| v["kind"] == "ability_decision" && v["actor"]["team"] == 1)
            .map(|v| {
                (
                    v["sim_time"].as_f64().unwrap() as f32,
                    v["actor"]["slot"].as_u64().unwrap() as u8,
                    v["actor"]["hp_pct"].as_f64().unwrap() as f32,
                )
            })
            .collect();
        hp_events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        let paladin = timeline.find(1, CharacterClass::Paladin, false);
        let enemies: Vec<Entity> = timeline
            .info
            .iter()
            .filter(|(_, i)| i.team == 2 && !i.is_pet)
            .map(|(e, _)| *e)
            .collect();
        let ps = timeline.samples.get(&paladin).cloned().unwrap_or_default();
        let at = |e: &Entity, t: f32| -> Option<Vec3> {
            timeline
                .samples
                .get(e)
                .and_then(|s| {
                    s.iter()
                        .min_by(|x, y| (x.0 - t).abs().partial_cmp(&(y.0 - t).abs()).unwrap())
                })
                .map(|(_, p)| *p)
        };

        // First contact (Paladin within 4yd of any enemy) bounds the window.
        let first_contact = ps
            .iter()
            .find(|(t, p)| {
                enemies
                    .iter()
                    .filter_map(|e| at(e, *t))
                    .any(|ep| p.distance(ep) <= 4.0)
            })
            .map(|(t, _)| *t);
        let first_contact = first_contact.expect("Paladin must reach melee at least once");

        let mut hp: BTreeMap<u8, f32> = BTreeMap::new();
        let mut hi = 0usize;
        let mut healthy_time = 0.0f32;
        let mut melee_time = 0.0f32;
        let mut prev: Option<f32> = None;
        for (t, p) in &ps {
            let ct = t - gate;
            while hi < hp_events.len() && hp_events[hi].0 <= ct {
                hp.insert(hp_events[hi].1, hp_events[hi].2);
                hi += 1;
            }
            let healthy = !hp.is_empty() && hp.values().all(|h| *h >= 0.6);
            if let Some(pt) = prev {
                if healthy && *t >= first_contact {
                    let dt = t - pt;
                    healthy_time += dt;
                    let dmin = enemies
                        .iter()
                        .filter_map(|e| at(e, *t))
                        .map(|ep| p.distance(ep))
                        .fold(f32::MAX, f32::min);
                    if dmin <= 4.0 {
                        melee_time += dt;
                    }
                }
            }
            prev = Some(*t);
        }
        let frac = melee_time / healthy_time.max(f32::EPSILON);
        eprintln!(
            "identity probe: match={:.0}s healthy-post-contact={:.1}s melee={:.1}s ({:.0}%)",
            result.match_time,
            healthy_time,
            melee_time,
            frac * 100.0
        );
        assert!(
            healthy_time >= 3.0,
            "probe went vacuous — re-scan seeds: only {:.1}s of post-contact healthy time",
            healthy_time
        );
        assert!(
            frac >= 0.5,
            "Paladin spent only {:.0}% of post-contact team-healthy time in melee \
             (floor 50%) — the healing-heavy trigger is eroding melee identity",
            frac * 100.0
        );
    }

    /// (f) CHIP-DAMAGE PROBE — a teammate takes light damage (stays above
    /// the urgency threshold) mid-dip: the dip still completes (the cast
    /// deferral held, the teammate-HP abort did not fire).
    #[test]
    fn chip_damage_mid_dip_still_completes() {
        use arenasim::states::play_match::movement_config::load_movement_config;
        let urgency = load_movement_config().unwrap().shared.urgency_hp_threshold;

        let (result, _timeline, trace) = run_observed_traced(dip_config(1));
        let events = movement_events(&trace);
        let spans = dip_spans(&events, result.match_time);
        let completed: Vec<_> = spans.iter().filter(|(_, _, k)| *k == "complete").collect();
        assert_min_occurrences("completed Paladin dips (chip)", completed.len(), 1);

        // The teammate (Warrior, slot 1) took SOME chip damage during the
        // dip but stayed above the urgency threshold (else the dip would
        // have aborted, not completed).
        let (s, e, _) = completed[0];
        let mate_hp: Vec<f32> = trace
            .iter()
            .filter(|v| {
                v["kind"] == "ability_decision"
                    && v["actor"]["team"] == 1
                    && v["actor"]["slot"] == 1
            })
            .filter(|v| {
                let t = v["sim_time"].as_f64().unwrap() as f32;
                t >= *s && t <= *e
            })
            .map(|v| v["actor"]["hp_pct"].as_f64().unwrap() as f32)
            .collect();
        eprintln!(
            "chip probe: dip [{:.1},{:.1}] completed; teammate hp during dip = {:?} (urgency {})",
            s, e, mate_hp, urgency
        );
        // Every observed teammate-HP sample during the dip stayed above the
        // urgency threshold — the chip did not trip the abort.
        for hp in &mate_hp {
            assert!(
                *hp > urgency,
                "teammate dropped to {:.2} (<= urgency {}) during a COMPLETED dip — \
                 the abort should have fired",
                hp,
                urgency
            );
        }
    }

    /// (g) SELF-PEEL PROBE — a focused Paladin with the enemy healer alive
    /// still lands HoJ on its own attacker within a bounded delay of cooldown
    /// availability: the reservation is released under PRESSURED so self-peel
    /// is never starved.
    #[test]
    fn focused_paladin_self_peels_despite_living_enemy_healer() {
        let (result, _timeline, trace) = run_observed_traced(preempt_config(1));
        let events = movement_events(&trace);
        let windows = pressured_windows(&events, 1, 0, result.match_time);
        assert_min_occurrences("Paladin PRESSURED windows", windows.len(), 1);

        // The enemy Priest must still be alive at some PRESSURED moment
        // (non-vacuity: the reservation only matters with a living healer).
        let priest_id = entity_of(&trace, 2, "Priest", 0);
        let warrior_id = entity_of(&trace, 2, "Warrior", 1);

        // A HoJ landed on the enemy Warrior (the Paladin's attacker, slot 1)
        // during a PRESSURED window — self-peel through the released
        // reservation.
        let hojs = paladin_hoj_casts(&trace);
        let self_peel = hojs.iter().any(|(t, tgt)| {
            *tgt == warrior_id && windows.iter().any(|(a, b)| *t >= *a && *t <= *b)
        });
        eprintln!(
            "self-peel probe: hojs={:?} enemy_warrior=e{} enemy_priest=e{} pressured={:?}",
            hojs, warrior_id, priest_id, windows
        );
        assert!(
            self_peel,
            "no self-peel HoJ on the attacking enemy Warrior during a PRESSURED window — \
             the reservation starved self-peel (hojs={:?})",
            hojs
        );
    }

    /// Degenerate-case identity probe (the Priest's R5 no-ally rule applied
    /// to the Paladin's retreat): a Paladin with no living non-pet teammate
    /// never enters PRESSURED — there is no team to retreat for, and falling
    /// back only deletes its melee output. The U9 validation matrix caught
    /// the failure this guards against: every Paladin 1v1 collapsed, e.g.
    /// the Paladin permanently kiting a Hunter's pet (85 PressuredEnter/Exit
    /// strobes, 300s draw; Paladin v Hunter went 100% -> 0% wins).
    ///
    /// Seed 4100 is the matrix seed of the inspected pathological trace.
    #[test]
    fn paladin_1v1_never_retreats() {
        let config = create_config(vec!["Paladin"], vec!["Hunter"], Some(4100));
        let (result, _timeline, trace) = run_observed_traced(config);

        let paladin_movement: Vec<MovementEvent> = movement_events(&trace)
            .into_iter()
            .filter(|e| e.team == 1 && e.slot == 0)
            .collect();
        assert!(
            paladin_movement.is_empty(),
            "1v1 Paladin (no teammate) must issue no posture movement; got {:?}",
            paladin_movement
                .iter()
                .map(|e| (e.sim_time, e.trigger.clone()))
                .collect::<Vec<_>>()
        );
        assert!(
            result.winner.is_some(),
            "1v1 Paladin v Hunter must be decisive (no permanent-retreat draw); \
             match ran {:.1}s",
            result.match_time
        );
    }
}

// ---------------------------------------------------------------------------
// U8 — Paladin posture unit tests (pure predicates, no Bevy world)
// ---------------------------------------------------------------------------

mod paladin_unit {
    use std::collections::BTreeMap;

    use arenasim::states::match_config::CharacterClass;
    use arenasim::states::play_match::class_ai::combat_snapshot::CombatSnapshot;
    use arenasim::states::play_match::class_ai::paladin::{
        dip_should_abort, dip_target_candidate, hoj_target_eligible, rotation_hoj_allowed,
    };
    use arenasim::states::play_match::class_ai::CombatantInfo;
    use arenasim::states::play_match::components::{Combatant, HealerPosture, Posture};
    use arenasim::states::play_match::movement_config::MovementConfig;
    use arenasim::states::play_match::{Aura, AuraType, DRCategory, DRTracker};
    use bevy::prelude::*;

    fn info(entity: Entity, team: u8, class: CharacterClass, pos: Vec3) -> CombatantInfo {
        CombatantInfo {
            entity,
            team,
            slot: 0,
            class,
            current_health: 100.0,
            max_health: 100.0,
            current_mana: 100.0,
            max_mana: 100.0,
            position: pos,
            velocity: Vec3::ZERO,
            is_alive: true,
            stealthed: false,
            target: None,
            is_pet: false,
            casting_ability: None,
            pet_type: None,
            pet: None,
            book: arenasim::states::play_match::ability_book::AbilityBook::shipped_for(class, None),
        }
    }

    fn snapshot(self_entity: Entity) -> CombatSnapshot {
        let mut combatants = BTreeMap::new();
        combatants.insert(
            self_entity,
            info(self_entity, 1, CharacterClass::Paladin, Vec3::ZERO),
        );
        CombatSnapshot {
            melee_defense: Default::default(),
            ai_profile: Default::default(),
            bounds: Default::default(),
            combatants,
            active_auras: BTreeMap::new(),
            dr_trackers: BTreeMap::new(),
            ability_cooldowns: BTreeMap::new(),
            obstacles: Vec::new(),
        }
    }

    fn dr_immune_tracker() -> DRTracker {
        // Apply Stuns until immune.
        let mut t = DRTracker::default();
        loop {
            t.apply(DRCategory::Stuns);
            if t.is_immune(DRCategory::Stuns) {
                break;
            }
        }
        t
    }

    /// (h) Reservation: suppresses rotation HoJ ONLY while a living enemy
    /// healer exists AND the Paladin is not PRESSURED/ESCAPE.
    #[test]
    fn reservation_only_when_healer_alive_and_unpressured() {
        // No enemy healer: rotation always allowed, every posture.
        for p in [
            Posture::Free,
            Posture::Pressured,
            Posture::Escape,
            Posture::Dip,
        ] {
            assert!(
                rotation_hoj_allowed(p, false),
                "no healer → rotation allowed in {:?}",
                p
            );
        }
        // Living enemy healer: suppressed in FREE/DIP, released under
        // PRESSURED/ESCAPE (self-peel never starved).
        assert!(
            !rotation_hoj_allowed(Posture::Free, true),
            "FREE + healer → reserved"
        );
        assert!(
            !rotation_hoj_allowed(Posture::Dip, true),
            "DIP + healer → reserved"
        );
        assert!(
            rotation_hoj_allowed(Posture::Pressured, true),
            "PRESSURED + healer → released"
        );
        assert!(
            rotation_hoj_allowed(Posture::Escape, true),
            "ESCAPE + healer → released"
        );
    }

    /// (h) DIP entry rejected when the HoJ eligibility predicate fails:
    /// DR-immune target is not eligible and so is not a dip candidate.
    #[test]
    fn dr_immune_target_is_not_dip_candidate() {
        let me = Entity::from_raw(1);
        let enemy_priest = Entity::from_raw(2);
        let mut snap = snapshot(me);
        snap.combatants.insert(
            enemy_priest,
            info(
                enemy_priest,
                2,
                CharacterClass::Priest,
                Vec3::new(5.0, 0.0, 0.0),
            ),
        );

        // Eligible while not DR-immune → a candidate.
        assert!(hoj_target_eligible(&snap.context_for(me), 1, enemy_priest));
        assert_eq!(
            dip_target_candidate(&snap.context_for(me), 1, Vec3::ZERO, 100.0),
            Some(enemy_priest)
        );

        // DR-immune to Stuns → not eligible, not a candidate.
        snap.dr_trackers.insert(enemy_priest, dr_immune_tracker());
        assert!(!hoj_target_eligible(&snap.context_for(me), 1, enemy_priest));
        assert_eq!(
            dip_target_candidate(&snap.context_for(me), 1, Vec3::ZERO, 100.0),
            None
        );
    }

    /// (h) Divine Shield (DamageImmunity) and stealth also fail eligibility.
    #[test]
    fn immune_and_stealthed_targets_are_not_eligible() {
        let me = Entity::from_raw(1);
        let enemy = Entity::from_raw(2);
        let mut snap = snapshot(me);
        snap.combatants.insert(
            enemy,
            info(enemy, 2, CharacterClass::Paladin, Vec3::new(5.0, 0.0, 0.0)),
        );

        // Divine Shield.
        snap.active_auras.insert(
            enemy,
            vec![Aura {
                effect_type: AuraType::DamageImmunity,
                duration: 5.0,
                magnitude: 1.0,
                ..Default::default()
            }],
        );
        assert!(
            !hoj_target_eligible(&snap.context_for(me), 1, enemy),
            "immune → ineligible"
        );

        // Stealthed.
        snap.active_auras.remove(&enemy);
        snap.combatants.get_mut(&enemy).unwrap().stealthed = true;
        assert!(
            !hoj_target_eligible(&snap.context_for(me), 1, enemy),
            "stealthed → ineligible"
        );
    }

    /// (h) Reach gate: an eligible enemy healer beyond reach is not a dip
    /// candidate; within reach, it is.
    #[test]
    fn dip_candidate_respects_reach() {
        let me = Entity::from_raw(1);
        let enemy_priest = Entity::from_raw(2);
        let mut snap = snapshot(me);
        snap.combatants.insert(
            enemy_priest,
            info(
                enemy_priest,
                2,
                CharacterClass::Priest,
                Vec3::new(20.0, 0.0, 0.0),
            ),
        );
        assert_eq!(
            dip_target_candidate(&snap.context_for(me), 1, Vec3::ZERO, 10.0),
            None,
            "healer at 20 beyond reach 10 → no candidate"
        );
        assert_eq!(
            dip_target_candidate(&snap.context_for(me), 1, Vec3::ZERO, 25.0),
            Some(enemy_priest),
            "healer at 20 within reach 25 → candidate"
        );
    }

    /// (h) Non-healer enemies are never dip candidates (the dip exists to
    /// stun the enemy HEALER).
    #[test]
    fn non_healer_is_not_dip_candidate() {
        let me = Entity::from_raw(1);
        let enemy_warrior = Entity::from_raw(2);
        let mut snap = snapshot(me);
        snap.combatants.insert(
            enemy_warrior,
            info(
                enemy_warrior,
                2,
                CharacterClass::Warrior,
                Vec3::new(3.0, 0.0, 0.0),
            ),
        );
        assert_eq!(
            dip_target_candidate(&snap.context_for(me), 1, Vec3::ZERO, 100.0),
            None
        );
    }

    /// (AE3) Teammate-HP-dive abort branch of `dip_should_abort`: a live dip
    /// (dip_target set, dip_until in the future, target still HoJ-eligible)
    /// aborts the moment a living ally (other than self) drops to/below the
    /// urgency HP threshold — the heal must un-defer immediately. This guards
    /// the most behavior-sensitive abort path with a deterministic snapshot
    /// instead of leaning on seed-dependent integration scans.
    #[test]
    fn dip_aborts_on_teammate_hp_dive() {
        let me = Entity::from_raw(1);
        let ally = Entity::from_raw(2);
        let enemy_priest = Entity::from_raw(3);

        let movement = MovementConfig::default();
        let urgency = movement.shared.urgency_hp_threshold; // 0.5

        let mut snap = snapshot(me);
        // Living, still-eligible enemy healer = the committed dip target.
        snap.combatants.insert(
            enemy_priest,
            info(
                enemy_priest,
                2,
                CharacterClass::Priest,
                Vec3::new(5.0, 0.0, 0.0),
            ),
        );
        // Wounded ally at the urgency threshold — triggers the abort.
        let mut ally_info = info(ally, 1, CharacterClass::Warrior, Vec3::new(3.0, 0.0, 0.0));
        ally_info.current_health = ally_info.max_health * urgency;
        snap.combatants.insert(ally, ally_info);

        let ctx = snap.context_for(me);
        let combatant = Combatant::new(1, 0, CharacterClass::Paladin);

        // Live dip: target set, budget deadline in the future.
        let now = 10.0;
        let mut state = HealerPosture::new(now);
        state.posture = Posture::Dip;
        state.dip_target = Some(enemy_priest);
        state.dip_until = now + movement.paladin.dip_budget;

        // Sanity: the dip target is still HoJ-eligible (so the abort is driven
        // by the teammate-HP branch, not the eligibility branch).
        assert!(hoj_target_eligible(&ctx, 1, enemy_priest));

        assert!(
            dip_should_abort(&state, &combatant, &ctx, &movement.shared, now),
            "wounded ally at/below urgency threshold must abort the dip"
        );

        // Control: lift the ally above the threshold → no abort (the live dip
        // with an eligible target and unspent budget continues).
        snap.combatants.get_mut(&ally).unwrap().current_health =
            snap.combatants[&ally].max_health * (urgency + 0.2);
        let ctx = snap.context_for(me);
        assert!(
            !dip_should_abort(&state, &combatant, &ctx, &movement.shared, now),
            "healthy ally + eligible target + unspent budget must NOT abort"
        );
    }
}

/// Bucket A unit tests (offensive-punish): the burst-during-CC predicate
/// (the `enemy_healer_locked` situation) and the pure target-swap chooser
/// (`select_softer_melee_target`). These pin the new logic deterministically;
/// the consolidated matrix pass validates the resulting balance.
mod bucket_a_unit {
    use std::collections::BTreeMap;

    use arenasim::states::match_config::CharacterClass;
    use arenasim::states::play_match::class_ai::combat_snapshot::CombatSnapshot;
    use arenasim::states::play_match::class_ai::{select_softer_melee_target, CombatantInfo};
    use arenasim::states::play_match::situation::{MatchClock, SituationConfig};
    use arenasim::states::play_match::{Aura, AuraType, DispelType};
    use bevy::prelude::*;

    fn info(entity: Entity, team: u8, class: CharacterClass, hp: f32) -> CombatantInfo {
        CombatantInfo {
            entity,
            team,
            slot: 0,
            class,
            current_health: hp,
            max_health: 100.0,
            current_mana: 100.0,
            max_mana: 100.0,
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            is_alive: hp > 0.0,
            stealthed: false,
            target: None,
            is_pet: false,
            casting_ability: None,
            pet_type: None,
            pet: None,
            book: arenasim::states::play_match::ability_book::AbilityBook::shipped_for(class, None),
        }
    }

    fn cc_aura(effect_type: AuraType) -> Aura {
        Aura {
            effect_type,
            duration: 4.0,
            magnitude: 1.0,
            break_on_damage_threshold: -1.0,
            accumulated_damage: 0.0,
            tick_interval: 0.0,
            time_until_next_tick: 0.0,
            caster: None,
            ability_name: "test".to_string(),
            fear_direction: (0.0, 0.0),
            fear_direction_timer: 0.0,
            spell_school: None,
            applied_this_frame: false,
            backlash_damage: None,
            dr_category_override: None,
            dispel_type: DispelType::Auto,
            compound: None,
            source_item: None,
            stacks: None,
            bloom: None,
            damage_mods: Default::default(),
        }
    }

    /// Warrior (team 1) vs an enemy Priest (team 2). `enemy_healer_locked`
    /// keys off the CAST-PREVENTING CC subset only.
    fn snap_with_healer_aura(aura: Option<Aura>, healer_alive: bool) -> (CombatSnapshot, Entity) {
        let me = Entity::from_raw(1);
        let healer = Entity::from_raw(2);
        let mut combatants = BTreeMap::new();
        combatants.insert(me, info(me, 1, CharacterClass::Warrior, 100.0));
        combatants.insert(
            healer,
            info(
                healer,
                2,
                CharacterClass::Priest,
                if healer_alive { 100.0 } else { 0.0 },
            ),
        );
        let mut active_auras = BTreeMap::new();
        if let Some(a) = aura {
            active_auras.insert(healer, vec![a]);
        }
        (
            CombatSnapshot {
                melee_defense: Default::default(),
                ai_profile: Default::default(),
                bounds: Default::default(),
                combatants,
                active_auras,
                dr_trackers: BTreeMap::new(),
                ability_cooldowns: BTreeMap::new(),
                obstacles: Vec::new(),
            },
            me,
        )
    }

    #[test]
    fn healer_cc_detects_cast_preventing_cc() {
        for cc in [
            AuraType::Stun,
            AuraType::Fear,
            AuraType::Polymorph,
            AuraType::Incapacitate,
        ] {
            let (snap, me) = snap_with_healer_aura(Some(cc_aura(cc)), true);
            assert!(
                snap.context_for(me)
                    .classified(&SituationConfig::default(), MatchClock::after_gates(0.0))
                    .situation()
                    .enemy_healer_locked
                    .is_some(),
                "{:?} on the enemy healer must open a burst window",
                cc
            );
        }
    }

    #[test]
    fn healer_cc_ignores_root_and_healthy_and_missing() {
        // Root does NOT stop a heal — must not open a burst window.
        let (snap, me) = snap_with_healer_aura(Some(cc_aura(AuraType::Root)), true);
        assert!(
            snap.context_for(me)
                .classified(&SituationConfig::default(), MatchClock::after_gates(0.0))
                .situation()
                .enemy_healer_locked
                .is_none(),
            "Root must not open a burst window"
        );

        // No aura at all → healer free → no window.
        let (snap, me) = snap_with_healer_aura(None, true);
        assert!(
            snap.context_for(me)
                .classified(&SituationConfig::default(), MatchClock::after_gates(0.0))
                .situation()
                .enemy_healer_locked
                .is_none(),
            "free healer → no window"
        );
        assert_eq!(
            snap.context_for(me).enemy_healer(),
            Some(Entity::from_raw(2))
        );

        // Dead healer → no living healer → no window, no healer.
        let (snap, me) = snap_with_healer_aura(Some(cc_aura(AuraType::Stun)), false);
        assert!(
            snap.context_for(me)
                .classified(&SituationConfig::default(), MatchClock::after_gates(0.0))
                .situation()
                .enemy_healer_locked
                .is_none(),
            "dead healer → no window"
        );
        assert_eq!(snap.context_for(me).enemy_healer(), None);
    }

    // --- select_softer_melee_target (pure) ---
    // kill target HP = 100; margin 0.15 → candidate must be <= 85 HP.

    #[test]
    fn swap_picks_softest_in_range_below_margin() {
        let a = Entity::from_raw(10);
        let b = Entity::from_raw(11);
        // a: 80 HP @ 3yd (qualifies), b: 50 HP @ 2yd (qualifies, softer) → b.
        let chosen =
            select_softer_melee_target(100.0, vec![(a, 3.0, 80.0), (b, 2.0, 50.0)], 4.0, 0.15);
        assert_eq!(chosen, Some(b), "lowest-HP qualifying candidate wins");
    }

    #[test]
    fn swap_respects_range_and_margin_and_emptiness() {
        let a = Entity::from_raw(10);
        // Out of range (5 > 4) → no swap.
        assert_eq!(
            select_softer_melee_target(100.0, vec![(a, 5.0, 10.0)], 4.0, 0.15),
            None
        );
        // In range but not softer enough (90 > 85 threshold) → no swap.
        assert_eq!(
            select_softer_melee_target(100.0, vec![(a, 1.0, 90.0)], 4.0, 0.15),
            None
        );
        // No candidates → None.
        assert_eq!(
            select_softer_melee_target(100.0, Vec::<(Entity, f32, f32)>::new(), 4.0, 0.15),
            None
        );
    }

    #[test]
    fn swap_tie_breaks_deterministically_by_entity() {
        let lo = Entity::from_raw(10);
        let hi = Entity::from_raw(11);
        // Equal HP + equal range: deterministic lowest-entity wins regardless of order.
        let fwd =
            select_softer_melee_target(100.0, vec![(lo, 2.0, 50.0), (hi, 2.0, 50.0)], 4.0, 0.15);
        let rev =
            select_softer_melee_target(100.0, vec![(hi, 2.0, 50.0), (lo, 2.0, 50.0)], 4.0, 0.15);
        assert_eq!(fwd, Some(lo));
        assert_eq!(rev, Some(lo), "tie-break is order-independent");
    }
}

// ---------------------------------------------------------------------------
// Mage ENGAGE/KITE posture probes (Part B pilot, U7)
// ---------------------------------------------------------------------------

mod mage_postures {
    use super::*;
    use arenasim::headless::runner::TraceConfig;
    use arenasim::states::play_match::constants::AUTO_SHOT_RANGE;

    /// Fixed seed for the Mage pilot probes (ascii "mage").
    const SEED: u64 = 0x6D61_6765;

    /// One parsed Mage movement_decision event (combat-time clock).
    struct MageEvent {
        sim_time: f32,
        trigger: String,
    }

    fn run_traced(config: HeadlessMatchConfig) -> (MatchResult, Timeline, Vec<serde_json::Value>) {
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
        .expect("observed traced match failed");
        let body = std::fs::read_to_string(&path).expect("read trace");
        let events: Vec<serde_json::Value> = body
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        let _ = std::fs::remove_file(path);
        (result, timeline, events)
    }

    /// Mage (team 1, slot 0) movement events in combat-time order.
    fn mage_events(trace: &[serde_json::Value]) -> Vec<MageEvent> {
        trace
            .iter()
            .filter(|v| v["kind"] == "movement_decision" && v["actor"]["class"] == "Mage")
            .map(|v| MageEvent {
                sim_time: v["sim_time"].as_f64().unwrap() as f32,
                trigger: v["trigger"].as_str().unwrap_or_default().to_string(),
            })
            .collect()
    }

    /// KITE windows (combat-time) from KiteEnter/KiteExit; an unclosed window
    /// ends at `end`.
    fn kite_windows(events: &[MageEvent], end: f32) -> Vec<(f32, f32)> {
        let mut windows = Vec::new();
        let mut open: Option<f32> = None;
        for e in events {
            match e.trigger.as_str() {
                "KiteEnter" => open = Some(e.sim_time),
                "KiteExit" => {
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

    /// Seed for [`mage_enters_kite_after_nova_and_exits`], split from [`SEED`]
    /// because only that probe needs a match in which a KITE window CLOSES
    /// before the match ends. See its doc for the re-pin.
    const KITE_EXIT_SEED: u64 = 0;

    /// Frost Nova roots a melee Warrior → the Mage enters KITE, and the window
    /// later closes (KITE is not a one-way trap). Honors the kite_hold floor
    /// and bounds the exit lag so a stuck-KITE regression fails loudly.
    ///
    /// Re-pinned for AS-167, weapon speed from item data. At [`SEED`] the
    /// probe went vacuous: 0 KiteExit transitions. The first line that differs
    /// from `main` @ 17cb9f0 is the Mage's first Wand Shot, 7 -> 9 (the Wand of
    /// Shadows fires every 1.8s for the same DPS, so each shot hits harder).
    /// The probe moved to its own seed, [`KITE_EXIT_SEED`], from
    /// `scan_kite_exit_seeds`: one KiteEnter, one real KiteExit, a 4.53s
    /// window. (At [`SEED`] the window ran 4.52s to match end with no exit.)
    #[test]
    fn mage_enters_kite_after_nova_and_exits() {
        let cfg = create_config(vec!["Mage"], vec!["Warrior"], Some(KITE_EXIT_SEED));
        let (result, _timeline, trace) = run_traced(cfg);
        let events = mage_events(&trace);

        let enters = events.iter().filter(|e| e.trigger == "KiteEnter").count();
        assert_min_occurrences("Mage KITE entries", enters, 1);
        // Pin that the exit TRANSITION actually fires — kite_windows() would
        // otherwise close an open window at match end (e.g. the Warrior dies
        // mid-KITE), satisfying the dwell bounds below without a real exit.
        let exits = events.iter().filter(|e| e.trigger == "KiteExit").count();
        assert_min_occurrences("Mage KITE exits", exits, 1);

        let windows = kite_windows(&events, result.match_time);
        assert_min_occurrences("Mage KITE windows", windows.len(), 1);
        for (start, end) in &windows {
            let dwell = end - start;
            // Hysteresis floor (kite_hold = 1.0) is honored, and KITE is not
            // stuck on forever (one-GCD exit lag, not minutes).
            assert!(
                dwell >= 1.0 - 1e-3,
                "KITE dwell {dwell:.2}s shorter than the kite_hold floor (1.0s)"
            );
            assert!(
                dwell <= 30.0,
                "KITE dwell {dwell:.2}s — KITE appears stuck (exit predicate not firing)"
            );
        }
    }

    /// Exploratory scan for [`KITE_EXIT_SEED`] (re-pin
    /// `mage_enters_kite_after_nova_and_exits` when trajectories drift):
    /// prints entries / exits / dwell range per seed and flags seeds where a
    /// real KiteExit fires and every window sits inside the probe's bounds.
    /// Ignored by default.
    #[test]
    #[ignore]
    fn scan_kite_exit_seeds() {
        for seed in 0u64..40 {
            let cfg = create_config(vec!["Mage"], vec!["Warrior"], Some(seed));
            let (result, _timeline, trace) = run_traced(cfg);
            let events = mage_events(&trace);
            let enters = events.iter().filter(|e| e.trigger == "KiteEnter").count();
            let exits = events.iter().filter(|e| e.trigger == "KiteExit").count();
            let windows = kite_windows(&events, result.match_time);
            let dwells: Vec<f32> = windows.iter().map(|(a, b)| b - a).collect();
            let lo = dwells.iter().cloned().fold(f32::INFINITY, f32::min);
            let hi = dwells.iter().cloned().fold(0.0f32, f32::max);
            let ok = exits >= 1 && !windows.is_empty() && lo >= 1.0 - 1e-3 && hi <= 30.0;
            eprintln!(
                "seed {seed:2}: enters={enters} exits={exits} windows={} dwell=[{lo:5.2},{hi:5.2}]{}",
                windows.len(),
                if ok { " <-- CANDIDATE" } else { "" }
            );
        }
    }

    /// KITE does not strobe: the entry count over a full 1v1 is bounded.
    #[test]
    fn mage_kite_does_not_strobe() {
        let cfg = create_config(vec!["Mage"], vec!["Warrior"], Some(SEED));
        let (_result, _timeline, trace) = run_traced(cfg);
        let enters = mage_events(&trace)
            .iter()
            .filter(|e| e.trigger == "KiteEnter")
            .count();
        assert!(
            enters <= 10,
            "Mage entered KITE {enters} times in one 1v1 — strobing (kite_hold not holding)"
        );
    }

    /// While kiting (range_band on), the Mage keeps its kill target — the
    /// Warrior — within cast range for the bulk of post-gate time, instead of
    /// fleeing it out of range (the legacy kiting bug) or face-tanking.
    #[test]
    fn mage_keeps_kill_target_in_shot_range() {
        let cfg = create_config(vec!["Mage"], vec!["Warrior"], Some(SEED));
        let (result, timeline, _trace) = run_traced(cfg);
        let gate = timeline.gates_open_time.expect("gates opened");

        let mage = timeline.find(1, CharacterClass::Mage, false);
        let warrior = timeline.find(2, CharacterClass::Warrior, false);
        let mage_s = timeline.samples.get(&mage).cloned().unwrap_or_default();
        let warrior_s = timeline.samples.get(&warrior).cloned().unwrap_or_default();

        let in_range = time_within_range_of(&mage_s, &warrior_s, AUTO_SHOT_RANGE);
        let post_gate = (result.match_time - gate).max(1e-3);
        let frac = in_range / post_gate;
        assert!(
            frac > 0.5,
            "Mage kept the Warrior in shot range only {:.0}% of the match — range_band is not \
             holding the kill target in range",
            frac * 100.0
        );
    }

    /// Non-perturbation extends to a Mage-directive match: an observed run is
    /// bit-identical to an unobserved run at the same seed, so the Mage posture
    /// machinery (which issues MovementDirectives) is observer-safe.
    #[test]
    fn mage_directive_run_does_not_perturb_outcomes() {
        let seed = SEED;
        let make = || create_config(vec!["Mage"], vec!["Warrior"], Some(seed));
        let unobserved = run_headless_match_with(make(), true, None).expect("unobserved");
        let mut frames = 0usize;
        let observed =
            run_headless_match_observed(make(), true, None, |_f| frames += 1).expect("observed");
        assert!(frames > 0, "observer never invoked");
        assert_results_identical(&observed, &unobserved, "mage observed vs unobserved");
    }
}

// ---------------------------------------------------------------------------
// Hunter ENGAGE/KITE posture probes (proximity-gated migration, H5)
// ---------------------------------------------------------------------------

mod hunter_postures {
    use super::*;
    use arenasim::headless::runner::TraceConfig;

    const SEED: u64 = 0x68_75_6e_74; // ascii "hunt"

    fn run_traced(config: HeadlessMatchConfig) -> (MatchResult, Timeline, Vec<serde_json::Value>) {
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
        .expect("observed traced match failed");
        let body = std::fs::read_to_string(&path).expect("read trace");
        let events: Vec<serde_json::Value> = body
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        let _ = std::fs::remove_file(path);
        (result, timeline, events)
    }

    /// A melee Warrior closing on the Hunter opens KITE (proximity-gated) — the
    /// Hunter is now posture-driven, not on the deleted kiting_timer branch.
    /// (No exit assertion: a Warrior that stays glued within the sustain radius
    /// keeps the Hunter in KITE for the whole match, so KiteExit is not a valid
    /// invariant for this matchup — the Mage's root-expiry exit is the case
    /// where exit is asserted.)
    #[test]
    fn hunter_enters_kite_on_proximity() {
        let cfg = create_config(vec!["Hunter"], vec!["Warrior"], Some(SEED));
        let (_result, _timeline, trace) = run_traced(cfg);
        let enters = trace
            .iter()
            .filter(|v| {
                v["kind"] == "movement_decision"
                    && v["actor"]["class"] == "Hunter"
                    && v["trigger"] == "KiteEnter"
            })
            .count();
        assert_min_occurrences("Hunter KITE entries", enters, 1);
    }

    /// The Hunter keeps its kill target within shot range for the bulk of the
    /// match (flee + gentle range_band), instead of being run down or fleeing
    /// out of range — guards the kiting effectiveness the flee term restored.
    #[test]
    fn hunter_keeps_warrior_in_shot_range() {
        use arenasim::states::play_match::constants::AUTO_SHOT_RANGE;
        let cfg = create_config(vec!["Hunter"], vec!["Warrior"], Some(SEED));
        let (result, timeline, _trace) = run_traced(cfg);
        let gate = timeline.gates_open_time.expect("gates opened");
        let hunter = timeline.find(1, CharacterClass::Hunter, false);
        let warrior = timeline.find(2, CharacterClass::Warrior, false);
        let hs = timeline.samples.get(&hunter).cloned().unwrap_or_default();
        let ws = timeline.samples.get(&warrior).cloned().unwrap_or_default();
        let in_range = time_within_range_of(&hs, &ws, AUTO_SHOT_RANGE);
        let post_gate = (result.match_time - gate).max(1e-3);
        assert!(
            in_range / post_gate > 0.5,
            "Hunter kept the Warrior in shot range only {:.0}% of the match",
            in_range / post_gate * 100.0
        );
    }

    /// Non-perturbation extends to a Hunter-directive match.
    #[test]
    fn hunter_directive_run_does_not_perturb_outcomes() {
        let make = || create_config(vec!["Hunter"], vec!["Warrior"], Some(SEED));
        let unobserved = run_headless_match_with(make(), true, None).expect("unobserved");
        let mut frames = 0usize;
        let observed =
            run_headless_match_observed(make(), true, None, |_f| frames += 1).expect("observed");
        assert!(frames > 0, "observer never invoked");
        assert_results_identical(&observed, &unobserved, "hunter observed vs unobserved");
    }

    /// Run a match with the combat log captured to a temp file and return its
    /// contents. The log carries per-attack `[DMG]` and `[CC]` lines the
    /// structured timeline/trace do not expose (e.g. pet auto-attacks).
    fn run_capturing_log(team1: Vec<&str>, team2: Vec<&str>) -> String {
        // Unique per-call path so parallel tests can't race on the same file.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let log = tmp.path().to_path_buf();
        drop(tmp);
        let mut cfg = create_config(team1, team2, Some(SEED));
        cfg.output_path = Some(log.to_string_lossy().into_owned());
        // suppress_log MUST be false: the combat log is only written to
        // output_path when logging is not suppressed (run_traced suppresses it).
        run_headless_match_with(cfg, false, None).expect("headless match for log capture");
        let body = std::fs::read_to_string(&log).expect("read captured combat log");
        let _ = std::fs::remove_file(&log);
        body
    }

    /// Parse the leading `[ T.TTs]` sim timestamp from a combat-log line.
    fn log_timestamp(line: &str) -> Option<f32> {
        let open = line.find('[')?;
        let close = line[open..].find("s]")? + open;
        line.get(open + 1..close)?.trim().parse::<f32>().ok()
    }

    /// Regression for the melee-pet dead-zone fix: a Hunter pet inherits the
    /// Hunter class and was silently cancelled by the ranged Auto-Shot dead-zone
    /// guard, dealing ZERO auto-attack damage for the entire history of the pet
    /// system. The `!attacker_is_melee` exemption restored it. The fix was a
    /// two-token change that regressed invisibly to `cargo test` — this is its
    /// guard.
    #[test]
    fn hunter_pet_deals_auto_attack_damage() {
        let log = run_capturing_log(vec!["Hunter"], vec!["Warrior"]);
        let spider_hits = log
            .lines()
            // Ids carry a "#slot" suffix now (e.g. "Team 1 Spider #1's Auto Attack ...").
            .filter(|l| l.contains("Spider #") && l.contains("Auto Attack hits"))
            .count();
        assert_min_occurrences("Spider auto-attack hits on the enemy", spider_hits, 1);
    }

    /// Regression for the friendly-CC auto-attack guard (root tier): enabling
    /// pet damage exposed that the Spider auto-attacked through its OWN Spider
    /// Web (a Root it casts to peel the target off the Hunter), shattering the
    /// peel on the first swing. The pet-only root tier makes it hold fire while
    /// its Web is up. Pre-fix the Spider attacked within ~0.8s of webbing;
    /// post-fix the next swing only lands after the ~4s root window.
    ///
    /// Against a Rogue: since the Hunter's traps share one cooldown (AS-196)
    /// Hunter v Warrior ends before the Spider's second Web, and its first
    /// always meets the Warrior's Charge immunity — no Web lands on the
    /// Warrior in seeds 0-79.
    #[test]
    fn hunter_pet_does_not_break_own_web() {
        let log = run_capturing_log(vec!["Hunter"], vec!["Rogue"]);
        // First time the Spider's Web is APPLIED (not merely cast) to the enemy.
        let web_applied = log
            .lines()
            .find(|l| l.contains("[CC] Web on Team 2"))
            .and_then(log_timestamp)
            .expect("the Spider should land a Web on the enemy at this seed");
        // The next Spider auto-attack on the enemy after the Web lands.
        let next_spider_hit = log
            .lines()
            .filter(|l| l.contains("Spider #") && l.contains("Auto Attack hits Team 2"))
            .filter_map(log_timestamp)
            .find(|&t| t >= web_applied);
        if let Some(t) = next_spider_hit {
            assert!(
                t - web_applied >= 3.0,
                "Spider auto-attacked its own Web {:.2}s after it landed — it broke its own \
                 root peel; expected it to hold fire through the ~4s window",
                t - web_applied
            );
        }
        // If the Spider never attacks the target again, it trivially never broke
        // the Web — also a pass.
    }
}

/// Psychic Scream (feat/priest-psychic-scream) behavioral probes — the
/// dual-mode AoE fear. These pin the new behavior at fixed seeds: the
/// offensive dip-to-fear-the-enemy-healer and the defensive self-peel.
mod psychic_scream {
    use super::priest_postures::{movement_events, run_observed_traced};
    use super::*;

    /// Sim-times at which `team`'s Priest CHOSE Psychic Scream (any path —
    /// defensive predicate or offensive dip cast both record a chosen
    /// candidate). Parsed from the raw ability_decision trace.
    fn scream_cast_times(trace: &[serde_json::Value], team: u8) -> Vec<f32> {
        trace
            .iter()
            .filter(|v| {
                v["kind"] == "ability_decision"
                    && v["actor"]["team"].as_u64() == Some(team as u64)
                    && v["actor"]["class"] == "Priest"
            })
            .filter(|v| {
                v["candidates"].as_array().is_some_and(|c| {
                    c.iter().any(|cand| {
                        cand["ability"] == "PsychicScream" && cand["status"] == "chosen"
                    })
                })
            })
            .map(|v| v["sim_time"].as_f64().unwrap() as f32)
            .collect()
    }

    /// AE2 / R11 — when the Priest is NOT the focus AND the team is killing
    /// someone other than the enemy healer, the Priest dips to fear the free
    /// healer (the kill-target guard keeps it from fearing an enemy the team is
    /// already breaking the fear on): DipEnter → DipComplete fire and a scream
    /// cast lands. Seed 42, 2v2. The enemy team focuses the team-1 Warrior
    /// (freeing the team-1 Priest); the team-1 team focuses the enemy Rogue
    /// (kill_target 0), leaving the enemy Paladin unfocused so the dip permits
    /// fearing it.
    ///
    /// The enemy healer is a Paladin, NOT a Priest: since Mana Burn (PR #83), a
    /// priest-mirror devolves into a mutual mana-burn war that drains both
    /// pools below Psychic Scream's 55-mana cost, so the dip's `pre_cast_ok`
    /// gate never opens and the probe goes vacuous. The enemy DPS is a Rogue
    /// (low sustained pressure), keeping the team-1 Warrior above
    /// `healing_heavy_hp` so the dip's ally-HP deferral stays open.
    #[test]
    fn offensive_dip_fears_enemy_healer() {
        let mut cfg = create_config(
            vec!["Priest", "Warrior"],
            vec!["Rogue", "Paladin"],
            Some(42),
        );
        cfg.team2_kill_target = Some(1); // focus team-1 Warrior, freeing the Priest to dip
        cfg.team1_kill_target = Some(0); // team kills the enemy Rogue, leaving the healer free
        let (_result, _timeline, trace) = run_observed_traced(cfg);

        let events = movement_events(&trace);
        let dip_enters = events
            .iter()
            .filter(|e| e.team == 1 && e.trigger == "DipEnter")
            .count();
        let dip_completes = events
            .iter()
            .filter(|e| e.team == 1 && e.trigger == "DipComplete")
            .count();
        eprintln!(
            "dip probe: team-1 Priest DipEnter={} DipComplete={}",
            dip_enters, dip_completes
        );
        assert_min_occurrences("team-1 Priest DipEnter", dip_enters, 1);
        assert_min_occurrences("team-1 Priest DipComplete", dip_completes, 1);

        // The completed dip actually cast the scream.
        let casts = scream_cast_times(&trace, 1);
        assert_min_occurrences("team-1 Priest scream cast (dip)", casts.len(), 1);
    }

    /// AE1 / R9 — when the Priest IS being focused by melee, it casts Psychic
    /// Scream as a defensive self-peel. Seed 7, 2v2 with the enemy team
    /// focusing the team-1 Priest.
    #[test]
    fn defensive_scream_fires_under_pressure() {
        let mut cfg = create_config(vec!["Priest", "Mage"], vec!["Warrior", "Rogue"], Some(7));
        cfg.team2_kill_target = Some(0); // focus the team-1 Priest
        let (_result, _timeline, trace) = run_observed_traced(cfg);

        let casts = scream_cast_times(&trace, 1);
        eprintln!(
            "defensive probe: team-1 Priest scream casts = {}",
            casts.len()
        );
        assert_min_occurrences("team-1 Priest defensive scream cast", casts.len(), 1);

        // No DipComplete: a focused Priest peels defensively, it does not dip.
        let events = movement_events(&trace);
        let dip_completes = events
            .iter()
            .filter(|e| e.team == 1 && e.trigger == "DipComplete")
            .count();
        assert_eq!(
            dip_completes, 0,
            "a focused Priest should self-peel, not complete an offensive dip"
        );
    }
}

/// Rogue Kidney Shot chain probes: the default Cheap Shot → Kidney Shot opener
/// (a ~10s undiminished lockdown on the kill target, enabled by Kidney Shot's
/// own DR category) and the no-double-spend Kick/Kidney denial chain against a
/// healer.
mod rogue_chain {
    use super::priest_postures::run_observed_traced;
    use super::*;

    /// Rogue (team 1) vs Priest (team 1 enemy). The Rogue trains the Priest.
    fn rogue_vs_priest(seed: u64) -> Vec<serde_json::Value> {
        let cfg = create_config(vec!["Rogue"], vec!["Priest"], Some(seed));
        let (_r, _t, trace) = run_observed_traced(cfg);
        trace
    }

    /// The Rogue's ordered non-Stealth ability casts: (sim_time, ability, target_id).
    /// Note: Kick is dispatched by `check_interrupts`, not the class AI, so it
    /// does NOT appear here — only `decide_abilities` casts are traced.
    fn rogue_casts(trace: &[serde_json::Value]) -> Vec<(f32, String, i64)> {
        trace
            .iter()
            .filter(|v| {
                v["kind"] == "ability_decision"
                    && v["actor"]["class"] == "Rogue"
                    && v["outcome"]["type"] == "action_taken"
                    && v["outcome"]["ability"] != "Stealth"
            })
            .map(|v| {
                (
                    v["sim_time"].as_f64().unwrap() as f32,
                    v["outcome"]["ability"].as_str().unwrap().to_string(),
                    v["outcome"]["target_id"].as_i64().unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn opener_chains_cheapshot_into_kidney_on_kill_target() {
        let trace = rogue_vs_priest(405);
        let casts = rogue_casts(&trace);
        assert!(
            casts.len() >= 2,
            "Rogue should open with at least two abilities, got {casts:?}"
        );

        let (t_cs, cs, cs_target) = &casts[0];
        let (t_ks, ks, ks_target) = &casts[1];

        assert_eq!(cs, "CheapShot", "default opener is Cheap Shot");
        assert_eq!(ks, "KidneyShot", "Cheap Shot chains into Kidney Shot");
        assert_eq!(
            cs_target, ks_target,
            "both opener stuns land on the same (kill) target"
        );

        // Hold-until-expiry: Kidney fires ~0.5s before the 4s Cheap Shot lapses,
        // not immediately — a near-seamless ~10s lockdown rather than ~8s of
        // overlap. (This also proves the opener pooled energy: a naive
        // fire-when-affordable would chain at ~2s, not ~3.5s.)
        let gap = t_ks - t_cs;
        assert!(
            (3.0..=4.0).contains(&gap),
            "Kidney Shot should chain near Cheap Shot's expiry (gap 3.0-4.0s), got {gap:.2}s"
        );
    }

    #[test]
    fn chain_holds_kidney_rather_than_double_spending() {
        // The no-double-spend behavior surfaces as the planner withholding Kidney
        // Shot with a "Kidney held: …" trace note while a Kick or school lockout
        // is already denying the healer's casts. Its presence proves the chain
        // engaged instead of blindly stacking the stun onto a cast Kick handles.
        //
        // Seed moved 404 -> 405 by AS-122, which armed the Rogue's off hand: the
        // extra swings kill the Priest sooner, and seed 404's match got short
        // enough (6 AI casts, down from 7) that the hold window never arose. The
        // BEHAVIOUR is intact — a scan of 18 seeds under the armed loadout found
        // the note in 16 of them, 404 and 1 being the exceptions — so this is a
        // seed whose window emptied, not a regression. 405 was chosen because it
        // carries the note under BOTH loadouts, so the pin is not fitted to the
        // change that moved it.
        let trace = rogue_vs_priest(405);
        let held_for_chain = trace.iter().any(|v| {
            v["kind"] == "ability_decision"
                && v["actor"]["class"] == "Rogue"
                && v["candidates"].as_array().is_some_and(|cs| {
                    cs.iter().any(|c| {
                        c["ability"] == "KidneyShot"
                            && c["reason"]["PreconditionUnmet"]["note"]
                                .as_str()
                                .is_some_and(|n| n.starts_with("Kidney held"))
                    })
                })
        });
        assert!(
            held_for_chain,
            "the chain should hold Kidney Shot at least once (trace note 'Kidney held: …')"
        );
    }
}

// ---------------------------------------------------------------------------
// U9 — Shaman totem probes
// ---------------------------------------------------------------------------
//
// These probes exercise the totem subsystem end-to-end through the observed
// headless run, using the U9 observer extensions (per-frame combatant
// health/auras + a parallel totem list). They pin fixed seeds and guard every
// windowed assertion with `assert_min_occurrences` so a seed shift can't make
// them pass vacuously.
//
// Seed notes (seed 7, BasicArena):
// - shaman_fixture (focus the Shaman, slot 0): the enemy Warrior trains the
//   Shaman, so the Shaman is injured and its Healing Stream HoT ticks for >0.
//   The Shaman drops all four totems during the 10s countdown (stationary), so
//   both it and its slot-1 Warrior ally start inside every totem's radius.
//   Post-gate the Warrior charges the enemy and leaves the ~20yd totem field —
//   the natural "outside radius" subject for the negative control.
// - placement_fixture (focus the Warrior, slot 1): enemies chase the Warrior
//   into midfield, leaving the backline Shaman unthreatened, so its totems drop
//   near its feet and far from any enemy (the "not in melee" assertion).
mod shaman_totems {
    use super::*;
    use arenasim::headless::runner::ObservedTotem;
    use arenasim::states::play_match::constants::{TOTEM_DURATION, TOTEM_SPACING_OFFSET};
    use arenasim::states::play_match::{AuraType, TotemElement};

    /// Shaman + Warrior vs Warrior + Priest, enemies forced onto the Shaman
    /// (slot 0). Pins seed 7.
    fn shaman_fixture() -> HeadlessMatchConfig {
        let mut cfg = create_config(
            vec!["Shaman", "Warrior"],
            vec!["Warrior", "Priest"],
            Some(7),
        );
        cfg.team2_kill_target = Some(0);
        cfg
    }

    /// Shaman + Warrior vs Warrior + Priest, enemies forced onto the WARRIOR
    /// (slot 1) so the backline Shaman stays unthreatened. Pins seed 7.
    fn placement_fixture() -> HeadlessMatchConfig {
        let mut cfg = create_config(
            vec!["Shaman", "Warrior"],
            vec!["Warrior", "Priest"],
            Some(7),
        );
        cfg.team2_kill_target = Some(1);
        cfg
    }

    /// Run an observed match, collecting every frame observation (combatant
    /// health/auras + totems).
    fn run_collecting_frames(config: HeadlessMatchConfig) -> (MatchResult, Vec<FrameObservation>) {
        let mut frames = Vec::new();
        let result = run_headless_match_observed(config, true, None, |frame| {
            frames.push(frame.clone());
        })
        .expect("observed shaman match failed");
        (result, frames)
    }

    /// First entity matching (team, class, non-pet) across the run.
    fn find_combatant(frames: &[FrameObservation], team: u8, class: CharacterClass) -> Entity {
        for f in frames {
            for (e, c) in &f.combatants {
                if c.team == team && c.class == class && !c.is_pet {
                    return *e;
                }
            }
        }
        panic!("no team-{} {:?} found in any frame", team, class);
    }

    /// The team's live Healing Stream (Water) totem on this frame, if any.
    fn water_totem(frame: &FrameObservation, team: u8) -> Option<&ObservedTotem> {
        frame
            .totems
            .iter()
            .find(|t| t.owner_team == team && t.element == TotemElement::Water)
    }

    /// Horizontal (x/z) distance — totems sit at y=0 and combatants at y~1, so
    /// the spacing-offset assertion compares on the plane the offset is in.
    fn horiz(a: Vec3, b: Vec3) -> f32 {
        let (dx, dz) = (a.x - b.x, a.z - b.z);
        (dx * dx + dz * dz).sqrt()
    }

    /// (1) An ally within the Healing Stream totem's radius carries the
    /// HealingOverTime buff. Asserted over the window where the geometry holds
    /// — every frame the team-1 Water totem exists AND the subject is within
    /// its radius (the same 3D distance test `totem_pulse_system` uses). Both
    /// the focused Shaman (always ~1.5yd from its own totem, injured so its
    /// ticks heal >0) and its Warrior ally (in-radius during the countdown)
    /// are checked.
    #[test]
    fn healing_stream_buffs_ally_in_radius() {
        let (_result, frames) = run_collecting_frames(shaman_fixture());
        let shaman = find_combatant(&frames, 1, CharacterClass::Shaman);
        let warrior = find_combatant(&frames, 1, CharacterClass::Warrior);

        for (label, subject) in [("Shaman", shaman), ("Warrior", warrior)] {
            let mut in_radius = 0usize;
            let mut buffed = 0usize;
            for f in &frames {
                let Some(totem) = water_totem(f, 1) else {
                    continue;
                };
                let Some(c) = f.combatants.get(&subject) else {
                    continue;
                };
                if !c.alive {
                    continue;
                }
                if totem.position.distance(c.position) > totem.radius {
                    continue;
                }
                in_radius += 1;
                if c.aura_types.contains(&AuraType::HealingOverTime) {
                    buffed += 1;
                }
            }
            assert_min_occurrences(
                &format!("{} frames in Healing Stream radius", label),
                in_radius,
                30,
            );
            let frac = buffed as f32 / in_radius as f32;
            eprintln!(
                "healing-stream probe: {} carried HoT in {}/{} in-radius frames ({:.0}%)",
                label,
                buffed,
                in_radius,
                frac * 100.0
            );
            // A handful of unbuffed frames are expected the instant a totem is
            // (re)dropped — it spawns in Phase 2, after the Phase-1 pulse, so
            // that one frame the totem exists but has not pulsed yet.
            assert!(
                frac >= 0.9,
                "{} carried the Healing Stream HoT in only {:.0}% of in-radius \
                 frames (floor 90%) — the totem buff is not landing",
                label,
                frac * 100.0
            );
        }
    }

    /// (2) NEGATIVE CONTROL — an ally kept outside the totem radius does not
    /// carry the totem buff. The Warrior charges the enemy and leaves the
    /// field; the buff lingers up to the 2s refresh window after leaving, so
    /// the assertion only fires on frames where the Warrior has been
    /// continuously outside the Water totem's radius for > 2.0s.
    #[test]
    fn ally_outside_radius_gets_no_totem_buff() {
        let (_result, frames) = run_collecting_frames(shaman_fixture());
        let warrior = find_combatant(&frames, 1, CharacterClass::Warrior);

        let mut last_in_radius_time: Option<f32> = None;
        let mut tested = 0usize;
        for f in &frames {
            let Some(c) = f.combatants.get(&warrior) else {
                continue;
            };
            if !c.alive {
                continue;
            }
            let totem = water_totem(f, 1);
            let dist = totem.map(|t| t.position.distance(c.position));
            let in_radius = matches!((totem, dist), (Some(t), Some(d)) if d <= t.radius);
            if in_radius {
                last_in_radius_time = Some(f.sim_time);
                continue;
            }
            // Outside radius (or no totem). Only assert once the residual
            // refresh window since the last in-radius frame has DURABLY
            // expired. The buff is refreshed to a 2.0s window each in-radius
            // pulse, so a subject that just stepped out keeps it for ~2s; a
            // 3.0s margin clears that boundary (the buff is genuinely gone,
            // not merely lingering — this probe surfaced the lag, which is the
            // documented behavior, not a bug).
            let residual_expired = match last_in_radius_time {
                Some(t) => f.sim_time - t > 3.0,
                None => true,
            };
            if !residual_expired {
                continue;
            }
            tested += 1;
            assert!(
                !c.aura_types.contains(&AuraType::HealingOverTime),
                "Warrior carried the Healing Stream HoT at t={:.1}s while {:.1}yd \
                 outside the totem radius (residual window already expired)",
                f.sim_time,
                dist.unwrap_or(f32::INFINITY),
            );
        }
        assert_min_occurrences(
            "Warrior outside-radius (residual-expired) frames",
            tested,
            30,
        );
    }

    /// (3) Totems spawn near the Shaman (within the spacing offset), not at the
    /// enemy. A "fresh drop" frame is one where a team-1 totem reads its full
    /// `TOTEM_DURATION` (it spawns in Phase 2, after the Phase-1 pulse that
    /// would tick it, so its first observed frame is un-ticked). At that frame
    /// the Shaman is at the drop position: horizontal distance to it must be
    /// within the spacing offset.
    ///
    /// That alone says "at the caster, not at the enemy" only for a drop made
    /// while every enemy is clear of the Shaman: with an enemy in melee, a
    /// totem at the caster's feet can sit as near that enemy as the Shaman
    /// (1.59yd against 1.6yd in this match after AS-303), and at-the-caster
    /// and at-the-enemy are the same place. So the probe also requires such a
    /// DISTINGUISHABLE drop: every enemy more than twice the tolerance from the
    /// Shaman, where a totem dropped on an enemy would fail the bound.
    #[test]
    fn totem_placement_is_near_caster_not_in_melee() {
        let (_result, frames) = run_collecting_frames(placement_fixture());
        let shaman = find_combatant(&frames, 1, CharacterClass::Shaman);

        let mut placements = 0usize;
        let mut distinguishable = 0usize;
        for f in &frames {
            let Some(sh) = f.combatants.get(&shaman) else {
                continue;
            };

            for t in f.totems.iter().filter(|t| t.owner_team == 1) {
                if t.duration_remaining < TOTEM_DURATION - 1e-3 {
                    continue; // already ticked — not a fresh drop
                }
                placements += 1;
                let d_caster = horiz(t.position, sh.position);
                // Nearest enemy to the TOTEM (not to the Shaman).
                let enemy_to_totem = f
                    .combatants
                    .values()
                    .filter(|c| c.team == 2 && !c.is_pet && c.alive)
                    .map(|c| horiz(c.position, t.position))
                    .fold(f32::INFINITY, f32::min);
                eprintln!(
                    "placement probe: {:?} totem dropped {:.2}yd from Shaman, \
                     nearest enemy {:.1}yd from the totem (t={:.1}s)",
                    t.element, d_caster, enemy_to_totem, f.sim_time
                );
                // Primary invariant: dropped at the caster's feet.
                assert!(
                    d_caster <= TOTEM_SPACING_OFFSET + 0.5,
                    "{:?} totem dropped {:.2}yd from the Shaman (spacing offset {:.1} + \
                     0.5 tolerance) — not at the caster's feet",
                    t.element,
                    d_caster,
                    TOTEM_SPACING_OFFSET
                );
                // "Not at the enemy": distinguishable only while every enemy
                // is clear of the Shaman by twice the tolerance (a totem on
                // such an enemy would then fail the bound above).
                let enemy_to_shaman = f
                    .combatants
                    .values()
                    .filter(|c| c.team == 2 && !c.is_pet && c.alive)
                    .map(|c| horiz(c.position, sh.position))
                    .fold(f32::INFINITY, f32::min);
                if enemy_to_shaman > 2.0 * (TOTEM_SPACING_OFFSET + 0.5) {
                    distinguishable += 1;
                }
            }
        }
        // At least one drop per element across the match.
        assert_min_occurrences("fresh totem placements", placements, 4);
        assert_min_occurrences(
            "fresh totem placements with every enemy clear of the Shaman",
            distinguishable,
            1,
        );
    }
}

// ===========================================================================
// Universal movement collision — pillar-interior regression guard
// ===========================================================================

/// With obstacle collision wired into every movement branch (fear/poly wander,
/// pursuit, directive, disengage, pet-follow, center-seek), no ground unit
/// should ever occupy a PillaredArena pillar's interior. Runs a fear-heavy 2v2
/// (dual Priests → Psychic Scream, plus a melee Warrior training a caster) so
/// feared wander, pursuit, and directive movement all drive units against the
/// pillars, at two fixed seeds. The pillars are the shipped PillaredArena
/// defaults: radius-2.5 cylinders mirrored at (±9, 0) (see `map_config.rs`).
///
/// `resolve_movement` keeps a mover's center at `radius + MOVER_RADIUS` (= 3.0)
/// from a pillar it would otherwise enter, so a sample whose center crosses
/// inside the solid 2.5 radius means a movement branch bypassed the resolver.
mod u6_collision_smoke {
    use super::*;
    use arenasim::states::match_config::ArenaMap;
    use arenasim::states::play_match::map_config::load_map_geometry_config;
    use arenasim::states::play_match::map_geometry::{prism_apothem, ObstacleVolume};

    /// The shipped PillaredArena cylinder footprints as (center_x, center_z,
    /// radius), loaded live so the smoke test tracks the real map geometry
    /// instead of a hardcoded copy.
    fn pillar_footprints() -> Vec<(f32, f32, f32)> {
        let geom = load_map_geometry_config()
            .expect("maps.ron loads")
            .active_for(ArenaMap::TwinPillars);
        let pillars: Vec<(f32, f32, f32)> = geom
            .volumes
            .iter()
            .filter_map(|v| match v {
                ObstacleVolume::Cylinder {
                    center_xz, radius, ..
                } => Some((center_xz.x, center_xz.y, *radius)),
                // Octagonal pillars: the closest a unit may legally come to the
                // center is the APOTHEM (flush against a flat face), which is
                // inside the circumradius — using the circumradius here would
                // false-positive on any unit standing against a face.
                ObstacleVolume::Prism {
                    center_xz,
                    circumradius,
                    sides,
                    ..
                } => Some((
                    center_xz.x,
                    center_xz.y,
                    prism_apothem(*circumradius, *sides),
                )),
                _ => None,
            })
            .collect();
        assert!(
            !pillars.is_empty(),
            "PillaredArena must carry pillar volumes"
        );
        pillars
    }

    fn pillared_fear_config(seed: u64) -> HeadlessMatchConfig {
        let mut cfg = create_config(
            vec!["Warrior", "Priest"],
            vec!["Warlock", "Priest"],
            Some(seed),
        );
        cfg.map = "TwinPillars".to_string();
        cfg
    }

    #[test]
    fn no_unit_rests_inside_a_pillar() {
        let pillars = pillar_footprints();
        for seed in [1u64, 7u64] {
            let (_result, timeline) = run_observed_collecting(pillared_fear_config(seed));
            let mut checked = 0usize;
            for (entity, samples) in &timeline.samples {
                let info = timeline.info.get(entity).expect("entity has info");
                for &(t, pos) in samples {
                    for (px, pz, radius) in pillars.iter().copied() {
                        let d = ((pos.x - px).powi(2) + (pos.z - pz).powi(2)).sqrt();
                        assert!(
                            d >= radius - 0.01,
                            "seed {}: team-{} {:?} (is_pet={}) at t={:.2} is inside pillar \
                             ({}, {}): center-dist {:.3} < {}",
                            seed,
                            info.team,
                            info.class,
                            info.is_pet,
                            t,
                            px,
                            pz,
                            d,
                            radius
                        );
                        checked += 1;
                    }
                }
            }
            assert!(
                checked > 0,
                "seed {}: no samples checked — timeline empty?",
                seed
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Healer deny posture (cover_pull) on PillaredArena
// ---------------------------------------------------------------------------
//
// A pressured healer with all teammates healthy should use the pillars to break
// its trainer's line of sight — that's the whole point of turning cover_pull on.
// We measure it directly: a Warrior trains the enemy Priest on PillaredArena;
// during the Priest's PRESSURED windows (teammates healthy → urgency suppression
// OFF → cover_pull active) the Priest spends real sim-time OCCLUDED from the
// Warrior, and its movement decisions carry the cover_pull scorer term.
mod u8_healer_cover {
    use super::priest_postures::{movement_events, pressured_windows};
    use super::*;
    use arenasim::headless::runner::TraceConfig;
    use arenasim::states::match_config::ArenaMap;
    use arenasim::states::play_match::map_config::load_map_geometry_config;
    use arenasim::states::play_match::map_geometry::{
        has_line_of_sight, ObstacleVolume, EYE_HEIGHT,
    };
    use arenasim::states::play_match::movement_config::load_movement_config;

    /// Warrior+Priest vs Priest+Mage on PillaredArena. Team-1's Warrior trains
    /// team-2's Priest (slot 0); the Priest's Mage teammate stays at range and
    /// healthy through the early pressured windows — so the deny posture is
    /// active (urgency suppression off) exactly when we measure occlusion.
    fn train_config(seed: u64) -> HeadlessMatchConfig {
        let mut cfg = create_config(
            vec!["Warrior", "Priest"],
            vec!["Priest", "Mage"],
            Some(seed),
        );
        cfg.map = "TwinPillars".to_string();
        cfg.team1_kill_target = Some(0); // team-1 focuses team-2's Priest
        cfg
    }

    /// One observed + traced PillaredArena run: full per-frame observations
    /// (positions AND health) plus the parsed trace events.
    fn run_observed_full(
        cfg: HeadlessMatchConfig,
    ) -> (Vec<FrameObservation>, Vec<serde_json::Value>) {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        drop(tmp);
        let mut frames: Vec<FrameObservation> = Vec::new();
        run_headless_match_observed(
            cfg,
            true,
            Some(TraceConfig {
                output_path: path.clone(),
            }),
            |frame| frames.push(frame.clone()),
        )
        .expect("observed traced headless match failed");
        let body = std::fs::read_to_string(&path).unwrap_or_default();
        let events: Vec<serde_json::Value> = body
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        let _ = std::fs::remove_file(path);
        (frames, events)
    }

    /// Line of sight between two ground units at the LoS eye height (the plane
    /// the cast/heal/scorer sight tests all use).
    fn sees(obstacles: &[ObstacleVolume], a: Vec3, b: Vec3) -> bool {
        has_line_of_sight(
            obstacles,
            Vec3::new(a.x, EYE_HEIGHT, a.z),
            Vec3::new(b.x, EYE_HEIGHT, b.z),
        )
    }

    /// Find the unique (team, class, non-pet) entity in a frame.
    fn find(frame: &FrameObservation, team: u8, class: CharacterClass) -> bevy::prelude::Entity {
        let m: Vec<_> = frame
            .combatants
            .iter()
            .filter(|(_, c)| c.team == team && c.class == class && !c.is_pet)
            .map(|(e, _)| *e)
            .collect();
        assert_eq!(
            m.len(),
            1,
            "expected one team-{team} {class:?}, found {}",
            m.len()
        );
        m[0]
    }

    /// Measure, over one run: total sim-seconds the trained Priest was OCCLUDED
    /// from its Warrior trainer during PRESSURED windows in which all its
    /// non-pet teammates were healthy (above the urgency threshold), plus the
    /// number of qualifying frames and whether any PRESSURED movement decision
    /// carried the cover_pull scorer term.
    struct CoverStats {
        occluded_secs: f32,
        qualifying_frames: usize,
        pressured_cover_terms: usize,
    }

    fn measure(seed: u64) -> CoverStats {
        let geom = load_map_geometry_config()
            .expect("maps.ron loads")
            .active_for(ArenaMap::TwinPillars);
        let obstacles = geom.volumes;
        assert!(
            !obstacles.is_empty(),
            "PillaredArena must carry cover volumes"
        );

        let mv = load_movement_config().expect("movement.ron loads");
        let urgency = mv.shared.urgency_hp_threshold;

        let (frames, trace) = run_observed_full(train_config(seed));
        let gate = frames
            .iter()
            .find(|f| f.gates_open)
            .map(|f| f.sim_time)
            .expect("gates opened");
        let last = frames.last().map(|f| f.sim_time).unwrap_or(gate);

        let first = frames.first().expect("frames recorded");
        let priest = find(first, 2, CharacterClass::Priest);
        let warrior = find(first, 1, CharacterClass::Warrior);

        // PRESSURED windows for the trained Priest (team 2, slot 0), in
        // combat-time; convert to frame-time with `gate`.
        let events = movement_events(&trace);
        let windows = pressured_windows(&events, 2, 0, last - gate);

        let in_window = |t: f32| {
            windows
                .iter()
                .any(|(w0, w1)| t >= *w0 + gate && t <= *w1 + gate)
        };

        let dt = 1.0_f32 / 60.0;
        let mut occluded_secs = 0.0;
        let mut qualifying_frames = 0usize;
        for f in &frames {
            if !f.gates_open || !in_window(f.sim_time) {
                continue;
            }
            let Some(p) = f.combatants.get(&priest) else {
                continue;
            };
            let Some(w) = f.combatants.get(&warrior) else {
                continue;
            };
            if !p.alive || !w.alive {
                continue;
            }
            // All non-pet team-2 teammates (excluding the Priest) healthy →
            // urgency suppression is OFF, so cover_pull is active this frame.
            let teammate_in_danger = f.combatants.iter().any(|(e, c)| {
                *e != priest
                    && c.team == 2
                    && !c.is_pet
                    && c.alive
                    && c.max_health > 0.0
                    && c.current_health / c.max_health < urgency
            });
            if teammate_in_danger {
                continue;
            }
            qualifying_frames += 1;
            if !sees(&obstacles, p.position, w.position) {
                occluded_secs += dt;
            }
        }

        // Any PRESSURED movement decision for the trained Priest carrying the
        // cover_pull scorer term (scenario 2b).
        let pressured_cover_terms = trace
            .iter()
            .filter(|v| {
                v["kind"] == "movement_decision"
                    && v["actor"]["team"].as_u64() == Some(2)
                    && v["actor"]["slot"].as_u64() == Some(0)
                    && v["posture"] == "pressured"
                    && v["scorer_terms"]["cover_pull"].is_number()
            })
            .count();

        CoverStats {
            occluded_secs,
            qualifying_frames,
            pressured_cover_terms,
        }
    }

    /// The deny posture actively engages cover and buys a real (if brief)
    /// occlusion window: at two fixed seeds, a pressured Priest (teammates
    /// healthy) runs PRESSURED cover_pull decisions and breaks its trainer's LoS
    /// for a nonzero span.
    ///
    /// Floor lowered from 2.0s to 0.5s after tangent steering. Steering applies
    /// to the Warrior trainer's normal pursuit, so it now rounds the pillar in a
    /// clean arc and RE-ACQUIRES the pillar-dancing Priest almost immediately
    /// instead of oozing around the surface and staying blind. Achievable cover
    /// occlusion collapsed from ~4.6s to a flat ~0.87s (a single structural
    /// approach-window; 52 frames, seed-invariant) — the deny posture still fires
    /// (14-19 cover_pull terms/match) and still buys a brief duck, but a competent
    /// melee no longer loses sustained sight to a pillar. This is an intended
    /// consequence of the movement fix, NOT a cover-AI regression: the Priest's
    /// cover_pull directive (a `MovementGoal::Direction` scorer output) is
    /// unsteered and unchanged; only the trainer's pursuit improved. Flagged for a
    /// possible cover/uptime rebalance follow-up. The 0.5s floor preserves the
    /// load-bearing core — the Priest achieves real, deliberate occlusion via
    /// cover — while reflecting the new ceiling.
    #[test]
    fn pressured_priest_uses_cover_against_its_trainer() {
        const OCCLUSION_FLOOR_SECS: f32 = 0.5;
        for seed in [0u64, 3u64] {
            let s = measure(seed);
            eprintln!(
                "U8 cover probe seed {seed}: occluded {:.2}s over {} qualifying pressured frames, \
                 {} PRESSURED cover_pull terms",
                s.occluded_secs, s.qualifying_frames, s.pressured_cover_terms,
            );
            // Non-vacuity: the trained Priest actually spent pressured frames
            // with healthy teammates (or the floor below proves nothing).
            assert_min_occurrences(
                &format!("seed {seed} qualifying pressured frames"),
                s.qualifying_frames,
                30,
            );
            assert!(
                s.occluded_secs >= OCCLUSION_FLOOR_SECS,
                "seed {seed}: pressured Priest was occluded from its trainer only {:.2}s \
                 (floor {OCCLUSION_FLOOR_SECS}s) — the deny posture is not using cover at all",
                s.occluded_secs,
            );
            assert_min_occurrences(
                &format!("seed {seed} PRESSURED cover_pull scorer terms"),
                s.pressured_cover_terms,
                1,
            );
        }
    }

    /// Exploratory seed scan for cover re-pinning. Ignored by default. A good pin
    /// has qualifying_frames >= 30, occluded_secs >= 2.0, cover_terms >= 1.
    #[test]
    #[ignore]
    fn scan_cover_seeds() {
        for seed in 0u64..40 {
            let s = measure(seed);
            let good =
                s.qualifying_frames >= 30 && s.occluded_secs >= 2.0 && s.pressured_cover_terms >= 1;
            eprintln!(
                "seed {seed:2}: occl={:5.2} qframes={:4} coverterms={:3}{}",
                s.occluded_secs,
                s.qualifying_frames,
                s.pressured_cover_terms,
                if good { " <-- GOOD" } else { "" },
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Attacker seek-LoS (Mage/Hunter) + melee tempo reset (Warrior)
// ---------------------------------------------------------------------------
//
// Two behaviors:
//  - Ranged seek: a kiter idle in shot range but OCCLUDED from its kill target
//    repositions (los_seek scorer term) instead of stalling behind a pillar
//    (R10). Evidenced from the trace: every Frostbolt LosBlocked is followed by
//    a successful Frostbolt cast within a bounded window.
//  - Melee reset: a CC'd Warrior with its gap closer down falls back toward its
//    healer (R12). The decision seam is pure (`melee_reset_active`) and the
//    integration is driven directly through `evaluate_warrior_reset` (World +
//    CommandQueue), asserting the emitted `MovementGoal::Point(healer)`
//    directive — the least-fragile form of the plan's acceptance bar.
mod u9_seek_reset {
    use super::*;

    use std::collections::BTreeMap;

    use bevy::ecs::world::CommandQueue;
    use bevy::prelude::*;

    use arenasim::headless::runner::TraceConfig;
    use arenasim::states::play_match::class_ai::warrior::{
        evaluate_warrior_reset, melee_reset_active, under_movement_cc,
    };
    use arenasim::states::play_match::class_ai::{CombatContext, CombatantInfo};
    use arenasim::states::play_match::components::{
        ActiveAuras, Aura, AuraType, Combatant, MeleeResetState, MovementDirective, MovementGoal,
    };
    use arenasim::states::play_match::decision_trace::{
        DecisionTrace, EventPayload, MovementTrigger,
    };
    use arenasim::states::play_match::movement_config::MeleeMovementConfig;
    use arenasim::states::play_match::situation::{MatchClock, SituationConfig};
    use arenasim::states::play_match::{AbilityType, DispelType};

    // ----- ranged seek (Mage) anti-stall, trace-derived -----

    fn run_traced_lines(
        team1: Vec<&str>,
        team2: Vec<&str>,
        seed: u64,
        map: &str,
    ) -> Vec<serde_json::Value> {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        let config = HeadlessMatchConfig {
            team1: team1.into_iter().map(String::from).collect(),
            team2: team2.into_iter().map(String::from).collect(),
            max_duration_secs: 120.0,
            random_seed: Some(seed),
            map: map.to_string(),
            ..Default::default()
        };
        run_headless_match_with(
            config,
            true,
            Some(TraceConfig {
                output_path: path.clone(),
            }),
        )
        .expect("traced headless match failed");
        std::fs::read_to_string(&path)
            .expect("read trace file")
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .collect()
    }

    /// sim_times of Mage ability decisions where a Frostbolt candidate had the
    /// given `status` (e.g. "chosen") or, when `reason` is set, that rejection
    /// reason (e.g. "LosBlocked").
    fn mage_frostbolt_times(
        lines: &[serde_json::Value],
        status: &str,
        reason: Option<&str>,
    ) -> Vec<f32> {
        lines
            .iter()
            .filter(|v| {
                v.get("kind").and_then(|k| k.as_str()) == Some("ability_decision")
                    && v.get("actor")
                        .and_then(|a| a.get("class"))
                        .and_then(|c| c.as_str())
                        == Some("Mage")
            })
            .filter_map(|v| {
                let cands = v.get("candidates")?.as_array()?;
                let hit = cands.iter().any(|c| {
                    if c.get("ability").and_then(|a| a.as_str()) != Some("Frostbolt") {
                        return false;
                    }
                    match reason {
                        Some(r) => c.get("reason").and_then(|x| x.as_str()) == Some(r),
                        None => c.get("status").and_then(|s| s.as_str()) == Some(status),
                    }
                });
                if hit {
                    v.get("sim_time").and_then(|t| t.as_f64()).map(|t| t as f32)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Count of Mage `SeekLos` movement decisions — the ENGAGE repositioning
    /// adds, fired when the Mage is occluded from its kill target in shot range.
    fn mage_seek_count(lines: &[serde_json::Value]) -> usize {
        lines
            .iter()
            .filter(|v| {
                v.get("kind").and_then(|k| k.as_str()) == Some("movement_decision")
                    && v.get("actor")
                        .and_then(|a| a.get("class"))
                        .and_then(|c| c.as_str())
                        == Some("Mage")
                    && v.get("trigger").and_then(|t| t.as_str()) == Some("SeekLos")
            })
            .count()
    }

    /// Longest CONTIGUOUS run of Frostbolt `LosBlocked` decisions (span in
    /// sim-seconds), where the run is broken by a successful cast or any other
    /// outcome. A perpetual LoS stall shows as one very long run; an enemy
    /// healer juking behind a pillar (its job) can legitimately extend a run
    /// but is NOT a Mage-side stall — so this metric is used only on the
    /// canonical no-juke seed 7.
    fn max_contiguous_block_span(lines: &[serde_json::Value]) -> f32 {
        let mut run_start: Option<f32> = None;
        let mut max_span = 0.0f32;
        for v in lines {
            if v.get("kind").and_then(|k| k.as_str()) != Some("ability_decision")
                || v.get("actor")
                    .and_then(|a| a.get("class"))
                    .and_then(|c| c.as_str())
                    != Some("Mage")
            {
                continue;
            }
            let Some(cands) = v.get("candidates").and_then(|c| c.as_array()) else {
                continue;
            };
            let Some(fb) = cands
                .iter()
                .find(|c| c.get("ability").and_then(|a| a.as_str()) == Some("Frostbolt"))
            else {
                continue;
            };
            let t = v.get("sim_time").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
            let blocked = fb.get("reason").and_then(|x| x.as_str()) == Some("LosBlocked");
            if blocked {
                let start = *run_start.get_or_insert(t);
                max_span = max_span.max(t - start);
            } else {
                run_start = None; // cast or any other outcome breaks the run
            }
        }
        max_span
    }

    /// The seeds the R10 anti-stall probe plays. It judges the matches in
    /// which the enemy healer actually denies the Mage sight — at least three
    /// Frostbolts refused `LosBlocked` at cast start — so a change that moves
    /// the RNG stream moves WHICH seeds are occluded, not whether the probe
    /// judges anything. (Two pinned seeds carried it through eight re-pins
    /// across seven cards; AS-282 migrated it here.)
    const OCCLUSION_BANK: std::ops::Range<u64> = 0..40;

    /// The fewest occluded matches the bank must hold.
    const MIN_OCCLUDED_MATCHES: usize = 4;

    /// Seconds of match a judged occlusion must leave after it begins: room
    /// for the 10s recovery bound `mage_recovers_to_cast_within_bound` holds
    /// a contiguous block to.
    const RECOVERY_ROOM_SECS: f32 = 10.0;

    /// R10 anti-stall, robust form: whenever the Mage is occluded it REPOSITIONS
    /// (emits SeekLos) and keeps landing Frostbolts — it never stands behind a
    /// pillar refusing to move. Holds regardless of how long the enemy healer
    /// jukes, so it is judged in every occluded match of the bank.
    #[test]
    fn mage_repositions_and_casts_despite_occlusion() {
        let mut occluded = 0;
        for seed in OCCLUSION_BANK {
            let lines = run_traced_lines(
                vec!["Mage", "Priest"],
                vec!["Warrior", "Priest"],
                seed,
                "TwinPillars",
            );
            let blocked = mage_frostbolt_times(&lines, "", Some("LosBlocked"));
            // The situation: the scenario actually exercises LoS denial.
            if blocked.len() < 3 {
                continue;
            }
            let casts = mage_frostbolt_times(&lines, "chosen", None);
            let seeks = mage_seek_count(&lines);
            assert!(
                seeks >= 1,
                "seed {seed}: Mage was occluded ({} blocks) but never emitted SeekLos — it stood \
                 still instead of repositioning",
                blocked.len()
            );
            // Recovery: despite heavy occlusion the Mage still lands casts, and
            // at least one lands after occlusion began (never permanently locked
            // out) — judged where the match left it room to: occlusion that
            // only begins in a match's last few seconds (seed 24 after AS-249:
            // the blocks start 3.3s before team 1's win) says nothing about a
            // stall.
            let first_block = blocked.iter().cloned().fold(f32::INFINITY, f32::min);
            let end = lines
                .iter()
                .filter_map(|v| v.get("sim_time").and_then(|t| t.as_f64()))
                .fold(0.0f64, f64::max) as f32;
            if end - first_block < RECOVERY_ROOM_SECS {
                continue;
            }
            occluded += 1;
            let casts_after = casts.iter().filter(|c| **c >= first_block).count();
            assert!(
                casts_after >= 1,
                "seed {seed}: no Frostbolt cast landed after occlusion began ({} blocks) — \
                 perpetual stall",
                blocked.len()
            );
        }
        assert_min_occurrences(
            "occluded Mage+Priest vs Warrior+Priest matches in the bank",
            occluded,
            MIN_OCCLUDED_MATCHES,
        );
    }

    /// Tight anti-stall bound, over the occlusion bank. Absent a persistent
    /// enemy-healer juke, an occluded Mage recovers to a cast quickly: the
    /// longest contiguous LosBlocked run is well under 10 sim-seconds. A juke
    /// can legitimately stretch one match's run, so the bound is held as a
    /// share: in nearly every occluded match of the bank the longest run is
    /// under it.
    ///
    /// It was a pinned seed re-derived eight times as the AI moved (77, 30,
    /// 24, 8 among them — each change took the pin's cast-start blocks to 0);
    /// AS-249's healer positioning took seed 8 to 0 again, and the probe moved
    /// to the bank `mage_repositions_and_casts_despite_occlusion` already plays.
    #[test]
    fn mage_recovers_to_cast_within_bound() {
        let mut spans = Vec::new();
        for seed in OCCLUSION_BANK {
            let lines = run_traced_lines(
                vec!["Mage", "Priest"],
                vec!["Warrior", "Priest"],
                seed,
                "TwinPillars",
            );
            if mage_frostbolt_times(&lines, "", Some("LosBlocked")).len() < 3 {
                continue;
            }
            spans.push((seed, max_contiguous_block_span(&lines)));
        }
        eprintln!("longest contiguous LosBlocked runs: {spans:?}");
        assert_min_occurrences(
            "occluded Mage+Priest vs Warrior+Priest matches in the bank",
            spans.len(),
            MIN_OCCLUDED_MATCHES,
        );
        let within = spans.iter().filter(|(_, span)| *span <= 10.0).count();
        assert!(
            within * 5 >= spans.len() * 4,
            "the Mage's longest LosBlocked run exceeded 10s in {} of {} occluded matches \
             ({spans:?}) — it stalls",
            spans.len() - within,
            spans.len()
        );
    }

    /// Scenario 2: the `los_seek` term is trace-visible in the Mage's
    /// ENGAGE seek decisions during occluded windows — over the occlusion
    /// bank. (Its pinned seed was re-pinned five times as the AI moved;
    /// AS-249 took seed 8 to 0 SeekLos decisions with the term.)
    #[test]
    fn mage_seek_emits_los_seek_scorer_term() {
        let mut seek_with_term = 0;
        for seed in OCCLUSION_BANK {
            let lines = run_traced_lines(
                vec!["Mage", "Priest"],
                vec!["Warrior", "Priest"],
                seed,
                "TwinPillars",
            );
            seek_with_term += lines
                .iter()
                .filter(|v| {
                    v.get("kind").and_then(|k| k.as_str()) == Some("movement_decision")
                        && v.get("trigger").and_then(|t| t.as_str()) == Some("SeekLos")
                })
                .filter(|v| {
                    v.get("scorer_terms")
                        .and_then(|s| s.get("los_seek"))
                        .is_some()
                })
                .count();
        }
        assert!(
            seek_with_term >= 5,
            "expected Mage SeekLos movement decisions carrying a los_seek scorer term over the \
             bank, got {seek_with_term}"
        );
    }

    // ----- melee reset (Warrior) decision seam -----

    #[test]
    fn melee_reset_active_truth_table() {
        // All conditions met (not pressing) → reset runs.
        assert!(melee_reset_active(1.0, 5.0, true, true, true, false));
        // Window lapsed (now >= armed_until) → no reset (bounded, not permanent).
        assert!(!melee_reset_active(5.0, 5.0, true, true, true, false));
        // Gap closer ready → re-engage, not reset.
        assert!(!melee_reset_active(1.0, 5.0, false, true, true, false));
        // Already in melee → keep swinging.
        assert!(!melee_reset_active(1.0, 5.0, true, false, true, false));
        // No healer to fall back toward → no reset.
        assert!(!melee_reset_active(1.0, 5.0, true, true, false, false));
        // Pressing an advantage overrides every other condition → keep
        // chasing, never reset, even with the full window/CD/range/healer set.
        assert!(!melee_reset_active(1.0, 5.0, true, true, true, true));
    }

    fn cc_aura(effect: AuraType) -> Aura {
        Aura {
            effect_type: effect,
            duration: 3.0,
            magnitude: 1.0,
            break_on_damage_threshold: -1.0,
            accumulated_damage: 0.0,
            tick_interval: 0.0,
            time_until_next_tick: 0.0,
            caster: None,
            ability_name: "test".to_string(),
            fear_direction: (0.0, 0.0),
            fear_direction_timer: 0.0,
            spell_school: None,
            applied_this_frame: false,
            backlash_damage: None,
            dr_category_override: None,
            dispel_type: DispelType::Auto,
            compound: None,
            source_item: None,
            stacks: None,
            bloom: None,
            damage_mods: Default::default(),
        }
    }

    #[test]
    fn under_movement_cc_detects_root_and_stun_not_fear() {
        let root = ActiveAuras {
            auras: vec![cc_aura(AuraType::Root)],
        };
        let stun = ActiveAuras {
            auras: vec![cc_aura(AuraType::Stun)],
        };
        let fear = ActiveAuras {
            auras: vec![cc_aura(AuraType::Fear)],
        };
        assert!(under_movement_cc(Some(&root)), "Root is a movement CC");
        assert!(under_movement_cc(Some(&stun)), "Stun is a movement CC");
        assert!(
            !under_movement_cc(Some(&fear)),
            "Fear is not (feared warriors run)"
        );
        assert!(!under_movement_cc(None), "no auras → not CC'd");
    }

    fn info(
        entity: Entity,
        team: u8,
        class: CharacterClass,
        position: Vec3,
        target: Option<Entity>,
    ) -> CombatantInfo {
        CombatantInfo {
            entity,
            team,
            slot: 0,
            class,
            current_health: 100.0,
            max_health: 100.0,
            current_mana: 100.0,
            max_mana: 100.0,
            position,
            velocity: Vec3::ZERO,
            is_alive: true,
            stealthed: false,
            target,
            is_pet: false,
            casting_ability: None,
            pet_type: None,
            pet: None,
            book: arenasim::states::play_match::ability_book::AbilityBook::shipped_for(class, None),
        }
    }

    const HEALER_POS: Vec3 = Vec3::new(5.0, 1.0, -5.0);

    /// Drive `evaluate_warrior_reset` once in a constructed scenario. Returns the
    /// movement directive goal it emitted (if any) and whether it traced a
    /// `MeleeReset` event.
    fn run_reset(
        armed_until: f32,
        now: f32,
        charge_on_cd: bool,
        target_distance: f32,
        with_healer: bool,
        press_margin: f32,
    ) -> (Option<MovementGoal>, bool) {
        let mut world = World::new();
        let warrior = world.spawn_empty().id();
        let target = Entity::from_raw(90_001);
        let healer = Entity::from_raw(90_002);

        let my_pos = Vec3::new(0.0, 1.0, 0.0);
        let target_pos = Vec3::new(0.0, 1.0, target_distance);

        let mut combatant = Combatant::new(1, 0, CharacterClass::Warrior);
        combatant.target = Some(target);
        if charge_on_cd {
            combatant.ability_cooldowns.insert(AbilityType::Charge, 8.0);
        }

        let mut combatants: BTreeMap<Entity, CombatantInfo> = BTreeMap::new();
        combatants.insert(
            warrior,
            info(warrior, 1, CharacterClass::Warrior, my_pos, Some(target)),
        );
        combatants.insert(
            target,
            info(target, 2, CharacterClass::Mage, target_pos, None),
        );
        if with_healer {
            combatants.insert(
                healer,
                info(healer, 1, CharacterClass::Priest, HEALER_POS, None),
            );
        }

        let active_auras: BTreeMap<Entity, Vec<Aura>> = BTreeMap::new();
        let dr_trackers = BTreeMap::new();
        let ability_cooldowns = BTreeMap::new();
        let obstacles = Vec::new();

        let ctx = CombatContext::new(
            warrior,
            combatants[&warrior].team,
            &combatants,
            &active_auras,
            &dr_trackers,
            &ability_cooldowns,
            &obstacles,
            Default::default(),
            Default::default(),
        )
        // The press gate reads the team situation's tempo, classified on
        // `press_margin`.
        .classified(
            &SituationConfig {
                press_advantage_margin: press_margin,
                ..SituationConfig::default()
            },
            MatchClock::after_gates(now),
        );

        let mut reset_state = MeleeResetState {
            armed_until,
            active: false,
        };
        let mut trace = DecisionTrace::default();

        let mut queue = CommandQueue::default();
        {
            let mut commands = Commands::new(&mut queue, &world);
            evaluate_warrior_reset(
                &mut commands,
                warrior,
                &combatant,
                my_pos,
                None, // CC has ended — the armed window is what keeps the reset live
                &ctx,
                Some(&mut reset_state),
                None,
                &MeleeMovementConfig::default(),
                now,
                &mut trace,
            );
        }
        queue.apply(&mut world);

        let goal = world.get::<MovementDirective>(warrior).map(|d| d.goal);
        let traced = trace.pending_events.iter().any(|e| {
            matches!(
                &e.payload,
                EventPayload::Movement { trigger, .. } if *trigger == MovementTrigger::MeleeReset
            )
        });
        (goal, traced)
    }

    #[test]
    fn warrior_reset_emits_healer_directive_when_armed_and_charge_down() {
        // Armed window open, Charge on cooldown, out of melee, healer present.
        // press_margin f32::MAX isolates the reset mechanic from the press gate
        // (the 2v1 fixture is otherwise a standing team-HP lead).
        let (goal, traced) = run_reset(100.0, 1.0, true, 20.0, true, f32::MAX);
        assert!(
            matches!(goal, Some(MovementGoal::Point(p)) if p == HEALER_POS),
            "expected a Point directive toward the healer, got {:?}",
            goal
        );
        assert!(traced, "activation should emit a MeleeReset trace event");
    }

    #[test]
    fn warrior_reset_suppressed_when_team_ahead() {
        // Same activating scenario as above, but the press gate is live at
        // the shipped 0.2 margin. The fixture team (Warrior + Priest, both full)
        // leads the lone enemy Mage by a full member (advantage 1.0 >= 0.2), so
        // the Warrior keeps chasing: no fallback directive, no MeleeReset trace.
        let (goal, traced) = run_reset(100.0, 1.0, true, 20.0, true, 0.2);
        assert!(
            goal.is_none(),
            "pressing an advantage must suppress the reset directive, got {:?}",
            goal
        );
        assert!(!traced, "no MeleeReset trace while pressing");
    }

    #[test]
    fn warrior_reset_silent_when_gap_closer_ready() {
        // Charge off cooldown → re-engage, no fallback directive.
        let (goal, traced) = run_reset(100.0, 1.0, false, 20.0, true, f32::MAX);
        assert!(
            goal.is_none(),
            "gap closer ready must not issue a reset directive"
        );
        assert!(!traced);
    }

    #[test]
    fn warrior_reset_silent_without_healer() {
        let (goal, _) = run_reset(100.0, 1.0, true, 20.0, false, f32::MAX);
        assert!(
            goal.is_none(),
            "no healer ally → nothing to fall back toward"
        );
    }

    #[test]
    fn warrior_reset_silent_in_melee() {
        // In melee range of the target → keep swinging, no reset.
        let (goal, _) = run_reset(100.0, 1.0, true, 1.0, true, f32::MAX);
        assert!(goal.is_none(), "in melee must not reset");
    }

    /// Exploratory scan for Mage-occlusion seeds (re-pin the assert_mage_*
    /// tests when trajectories drift). Prints blocked / seek / casts-after /
    /// max-span per seed. Ignored by default.
    #[test]
    #[ignore]
    fn scan_mage_occlusion_seeds() {
        for seed in 0u64..80 {
            let lines = run_traced_lines(
                vec!["Mage", "Priest"],
                vec!["Warrior", "Priest"],
                seed,
                "TwinPillars",
            );
            let blocked = mage_frostbolt_times(&lines, "", Some("LosBlocked"));
            let casts = mage_frostbolt_times(&lines, "chosen", None);
            let seeks = mage_seek_count(&lines);
            let first_block = blocked.iter().cloned().fold(f32::INFINITY, f32::min);
            let casts_after = casts.iter().filter(|c| **c >= first_block).count();
            let span = max_contiguous_block_span(&lines);
            let term = lines
                .iter()
                .filter(|v| {
                    v.get("kind").and_then(|k| k.as_str()) == Some("movement_decision")
                        && v.get("trigger").and_then(|t| t.as_str()) == Some("SeekLos")
                        && v.get("scorer_terms")
                            .and_then(|s| s.get("los_seek"))
                            .is_some()
                })
                .count();
            if blocked.len() >= 3 && seeks >= 1 && casts_after >= 1 && span <= 10.0 && term >= 1 {
                eprintln!(
                    "seed {seed:2}: blocked={:3} seeks={:3} casts_after={:2} max_span={:5.2} seek_term={} <-- CANDIDATE",
                    blocked.len(), seeks, casts_after, span, term,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Press-when-ahead (advantage signal turns denial OFF)
// ---------------------------------------------------------------------------
//
// A team clearly ahead should seek the fight, not LoS-stall into the dampening
// endgame (R13/F4). "Press" is simply "denial off": once a team's
// `team_hp_advantage` reaches `shared.press_advantage_margin`, its healers stop
// pulling into cover. We measure this directly on PillaredArena with a
// healer-heavy comp (Warrior+Priest vs Warlock+Priest): each Priest's
// PRESSURED/ESCAPE movement decisions carry the `cover_pull` scorer term, and we
// join every decision to that team's HP advantage at the moment it fired.
//
//   - the LEADING team's Priest (advantage >= margin) emits ZERO positive
//     cover_pull terms — press zeroed the weight; and
//   - the TRAILING team's Priest (advantage <= -margin) still denies
//     (cover_pull > 0 on real occlusion), proving the suppression is
//     conditional, not a global disable.
//
// The match also RESOLVES by elimination (EndReason::Kill), well under the 300s
// cap — the F4 promise that pressing closes the draw loophole. Seed 5 resolves
// at ~85s, past the 75s dampening onset, so it exercises a real attrition
// endgame that still terminates.
mod u10_press {
    use super::*;
    use arenasim::headless::runner::{EndReason, MatchResult, TraceConfig};
    use arenasim::states::play_match::movement_config::load_movement_config;
    use std::collections::BTreeMap as Btm;

    /// One observed + traced PillaredArena run of the healer-heavy comp.
    fn run(seed: u64) -> (MatchResult, Vec<FrameObservation>, Vec<serde_json::Value>) {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        drop(tmp);
        let cfg = HeadlessMatchConfig {
            team1: vec!["Warrior".into(), "Priest".into()],
            team2: vec!["Warlock".into(), "Priest".into()],
            max_duration_secs: 300.0,
            random_seed: Some(seed),
            map: "TwinPillars".to_string(),
            ..Default::default()
        };
        let mut frames: Vec<FrameObservation> = Vec::new();
        let result = run_headless_match_observed(
            cfg,
            true,
            Some(TraceConfig {
                output_path: path.clone(),
            }),
            |f| frames.push(f.clone()),
        )
        .expect("observed traced headless match failed");
        let body = std::fs::read_to_string(&path).unwrap_or_default();
        let events: Vec<serde_json::Value> = body
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        let _ = std::fs::remove_file(path);
        (result, frames, events)
    }

    /// `team`'s HP-fraction advantage at absolute frame time `abs_t` — the same
    /// alive/`!is_pet` sum the sim's `team_hp_advantage` computes, evaluated on
    /// the nearest observed frame. Mirrors the production signal so the probe
    /// classifies a decision by the very quantity the code gated on.
    fn team_adv_at(frames: &[FrameObservation], abs_t: f32, team: u8) -> Option<f32> {
        let f = frames.iter().min_by(|a, b| {
            (a.sim_time - abs_t)
                .abs()
                .partial_cmp(&(b.sim_time - abs_t).abs())
                .unwrap()
        })?;
        let mut sums: Btm<u8, f32> = Btm::new();
        for c in f.combatants.values() {
            if c.alive && !c.is_pet && c.max_health > 0.0 {
                *sums.entry(c.team).or_insert(0.0) += c.current_health / c.max_health;
            }
        }
        let own = sums.get(&team).copied().unwrap_or(0.0);
        let enemy: f32 = sums
            .iter()
            .filter(|(t, _)| **t != team)
            .map(|(_, v)| *v)
            .sum();
        Some(own - enemy)
    }

    struct PressStats {
        end_reason: EndReason,
        match_time: f32,
        /// PRESSURED/ESCAPE Priest decisions fired while that Priest's team led
        /// by >= margin (the leader, pressing).
        leader_decisions: usize,
        /// ...of those, how many carried a POSITIVE cover_pull term. Must be 0:
        /// press zeroes the weight, so a leader can never pull into cover.
        leader_cover_positive: usize,
        /// Trailing-Priest decisions (team behind by >= margin) that denied LoS
        /// (cover_pull > 0) — proof the suppression is conditional.
        trailer_denials: usize,
    }

    fn measure(seed: u64) -> PressStats {
        let margin = load_movement_config()
            .expect("movement.ron loads")
            .shared
            .press_advantage_margin;
        let (result, frames, events) = run(seed);
        let gate = frames
            .iter()
            .find(|f| f.gates_open)
            .map(|f| f.sim_time)
            .expect("gates opened");

        let (mut leader_decisions, mut leader_cover_positive, mut trailer_denials) = (0, 0, 0);
        for v in &events {
            if v["kind"] != "movement_decision" || v["actor"]["class"] != "Priest" {
                continue;
            }
            let posture = v["posture"].as_str().unwrap_or("");
            if posture != "pressured" && posture != "escape" {
                continue;
            }
            // Only decisions that ran the scorer carry the cover_pull term.
            let Some(cover) = v["scorer_terms"]["cover_pull"].as_f64() else {
                continue;
            };
            let team = v["actor"]["team"].as_u64().unwrap_or(0) as u8;
            let combat_t = v["sim_time"].as_f64().unwrap_or(0.0) as f32;
            let Some(adv) = team_adv_at(&frames, combat_t + gate, team) else {
                continue;
            };
            if adv >= margin {
                leader_decisions += 1;
                if cover > 0.0 {
                    leader_cover_positive += 1;
                }
            } else if adv <= -margin && cover > 0.0 {
                trailer_denials += 1;
            }
        }

        PressStats {
            end_reason: result.end_reason,
            match_time: result.match_time,
            leader_decisions,
            leader_cover_positive,
            trailer_denials,
        }
    }

    /// The core property, over the press bank: while ahead, a Priest never
    /// pulls into cover (press = denial off); while behind, it still does. The
    /// zero is asserted in every match; the two counts that keep it from being
    /// vacuous are pooled over the bank. (It played two pinned seeds until
    /// AS-249's healer positioning took one below its floors.)
    #[test]
    fn leading_healer_stops_denying_trailing_healer_keeps_denying() {
        let (mut leader_decisions, mut trailer_denials) = (0, 0);
        for seed in PRESS_BANK {
            let s = measure(seed);
            // Press: NONE of the leader's decisions pulled into cover.
            assert_eq!(
                s.leader_cover_positive, 0,
                "seed {seed}: a leading Priest pulled into cover {} time(s) while its team was \
                 ahead by the press margin — press should have zeroed cover_pull",
                s.leader_cover_positive,
            );
            leader_decisions += s.leader_decisions;
            trailer_denials += s.trailer_denials;
        }
        eprintln!(
            "U10 press probe over {PRESS_BANK:?}: leader_decisions={leader_decisions} \
             trailer_denials={trailer_denials}"
        );
        // Non-vacuity: the leading Priest actually took PRESSURED/ESCAPE
        // decisions while ahead (or the zero above proves nothing)...
        assert_min_occurrences(
            "leading-Priest pressured/escape decisions while ahead, over the bank",
            leader_decisions,
            20,
        );
        // ...and the trailing Priest still denied LoS (cover in use).
        assert_min_occurrences(
            "trailing-Priest cover denials while behind, over the bank",
            trailer_denials,
            20,
        );
    }

    /// The seeds the endgame guard plays: the same bank `scan_press_seeds`
    /// prints. (Pinned seeds carried it until the past-75s one had been
    /// re-picked four times; AS-293 migrated it here.) 120 seeds since AS-360:
    /// once the Warlock walked round a pillar for sight instead of standing
    /// out of it, fewer of these matches ran past 75s, and the first 40 seeds
    /// held one. 240 since AS-296: with Life Tap the Warlock's team runs dry
    /// less often, fewer matches reach 75s, and the first 120 seeds held one
    /// (4 before it; 3 of 240 after it, 8 before).
    const PRESS_BANK: std::ops::Range<u64> = 0..240;

    /// The most matches of the bank that may run to the cap (1 of 40 on
    /// `main` @ 68b9a0c, and after AS-293; 0 of 120 after AS-360; 0 of 240
    /// after AS-296).
    const MAX_CAPPED: usize = 2;

    /// The fewest kills past the 75s dampening onset the bank must hold (5 of
    /// 40 on `main` @ 68b9a0c, 4 after AS-293; 10 of 120 on `main` @
    /// 0a8f8a36, 4 after AS-360; 3 of 240 after AS-296).
    const MIN_DEEP_KILLS: usize = 2;

    /// F4 endgame guard: the healer-heavy comp RESOLVES by elimination — not
    /// the 300s cap draw — and some of those resolutions are real attrition
    /// endgames, ending past the 75s dampening onset because the leader
    /// presses instead of LoS-stalling. Judged over the bank: at most
    /// [`MAX_CAPPED`] matches run to the cap, and at least [`MIN_DEEP_KILLS`]
    /// end by a kill past 75s. The AE sweep owns the aggregate draw-rate.
    ///
    /// An endgame guard, not a guard on the press: with
    /// `press_advantage_margin` at 1.0 (no team ever presses) every match of
    /// the bank still ends by a kill, dampening deciding it (measured after
    /// AS-293). The press itself is
    /// [`leading_healer_stops_denying_trailing_healer_keeps_denying`]'s.
    #[test]
    fn press_comp_resolves_before_cap() {
        let mut capped = Vec::new();
        let mut deep_kills = Vec::new();
        for seed in PRESS_BANK {
            let s = measure(seed);
            if s.end_reason != EndReason::Kill {
                capped.push((seed, s.end_reason.as_str(), s.match_time));
            } else if s.match_time > 75.0 {
                deep_kills.push(seed);
            }
        }
        assert!(
            capped.len() <= MAX_CAPPED,
            "{} of the bank's matches ended without a kill (> {MAX_CAPPED}): {capped:?} — a \
             clearly-ahead team should press to a kill, not stall into the cap",
            capped.len(),
        );
        assert_min_occurrences(
            "kills past the 75s dampening onset in the bank",
            deep_kills.len(),
            MIN_DEEP_KILLS,
        );
    }

    /// Exploratory scan over [`PRESS_BANK`]: prints end/time and the
    /// press-property counts per seed (and lets the press-property pins be
    /// re-chosen when trajectories drift). Ignored by default. A good pin has end=kill,
    /// leader_decisions>=3, leader_cover_positive==0, trailer_denials>=3; the
    /// ">75s" pin additionally needs match_time>75.
    #[test]
    #[ignore]
    fn scan_press_seeds() {
        for seed in PRESS_BANK {
            let s = measure(seed);
            let good = s.end_reason == EndReason::Kill
                && s.leader_decisions >= 3
                && s.leader_cover_positive == 0
                && s.trailer_denials >= 3;
            eprintln!(
                "seed {seed:2}: end={:>4} t={:6.1} leader_dec={:3} leader_cover+={} trailer_deny={:3} {}",
                s.end_reason.as_str(),
                s.match_time,
                s.leader_decisions,
                s.leader_cover_positive,
                s.trailer_denials,
                if good { "<-- GOOD" } else { "" },
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Line-of-sight acceptance-evidence (AE) coverage map
// ---------------------------------------------------------------------------
//
// The LoS feature's acceptance evidence is pinned across several test files.
// This module owns the AE cells not already covered elsewhere; the comment
// below is the traceability map so a future reader knows where each AE lives:
//
//   AE1 (cast-start LoS gate emits LosBlocked) — src/.../cast_guard.rs unit
//       tests + tests/decision_trace_audit.rs's PillaredArena reference matchup
//       (Mage+Priest v Warrior+Priest, seed 7).
//   AE2 (a cast in flight fizzles at completion when the target leaves LoS)
//       and AE3 (launched projectiles still land) — `completion_fizzle_*` here.
//   AE4 (pressured healer denies LoS behind cover) — `mod u8_healer_cover`.
//   AE5 (elevation participates in sight; R15/R16) — `verticality_*` here,
//       asserted against the SHIPPED TestVerticality map data.
//   AE6 (universal collision keeps units out of pillars) — `mod u6_collision_smoke`.
//
// Kept each probe to 1-2 pinned seeds with the vacuity-guard idiom used across
// this file: a probe that measures "at least one X" first asserts the run
// actually produced the conditions X depends on.
mod los_probes {
    use super::*;
    use arenasim::states::match_config::ArenaMap;
    use arenasim::states::play_match::map_config::load_map_geometry_config;
    use arenasim::states::play_match::map_geometry::has_line_of_sight;

    // AE5 / R15 / R16 — elevation participates in line of sight.
    //
    // The AI does not navigate verticality (no pathing onto platforms), so we
    // cannot force two combatants onto a ramp/platform in a live match. Instead
    // we assert the geometry directly against the SHIPPED TestVerticality
    // volumes from assets/config/maps.ron — a stronger guarantee than synthetic
    // geometry, because it pins the map data the feature actually ships.
    //
    // TestVerticality layout (see maps.ron): a raised platform occupying
    // x∈[4,16], z∈[-6,6], y∈[0,3] (walkable top surface at y=3), a three-box
    // stepped ramp climbing west→east to it, and a full-height pillar at
    // (-12, 0). Eye height for a unit standing ON the platform top is y≈4
    // (feet at 3 + 1yd eye); a ground unit's eye is y≈1.

    /// Load the shipped TestVerticality obstacle volumes.
    fn verticality_volumes() -> Vec<arenasim::states::play_match::map_geometry::ObstacleVolume> {
        let geom = load_map_geometry_config()
            .expect("maps.ron loads")
            .active_for(ArenaMap::TestVerticality);
        assert!(
            !geom.volumes.is_empty(),
            "TestVerticality must carry obstacle volumes"
        );
        geom.volumes
    }

    /// A unit below/beside a platform edge is OCCLUDED from a unit standing on
    /// the platform top: the segment clips the platform's solid body. This is
    /// the y-axis (elevation) doing real work — a purely-XZ occlusion test
    /// could not distinguish "on top" from "on the ground beside it".
    #[test]
    fn verticality_below_edge_is_occluded_from_platform_top() {
        let obstacles = verticality_volumes();
        // Ground unit just east of the platform (x=18, past the x=16 face).
        let below = Vec3::new(18.0, 1.0, 0.0);
        // Unit standing on the platform top (feet at y=3, eye at y≈4), interior.
        let on_top = Vec3::new(10.0, 4.0, 0.0);
        assert!(
            !has_line_of_sight(&obstacles, below, on_top),
            "a ground unit beside the platform must be occluded from a unit on top \
             (the platform edge blocks the diagonal)"
        );
    }

    /// Two units both at platform-top height have a clear ramp-line sightline
    /// across the top of the ramp/platform: the segment rides at y≈4, above
    /// every obstacle's top (platform and ramp tops are y=3), so elevation
    /// GRANTS sight. Mirrors the "segment over pillar top" geometry unit test,
    /// but against the shipped map.
    #[test]
    fn verticality_ramp_line_at_height_is_clear() {
        let obstacles = verticality_volumes();
        // Atop the ramp's highest step / platform lip.
        let ramp_top = Vec3::new(5.0, 4.0, 0.0);
        // Across the platform top.
        let platform_top = Vec3::new(12.0, 4.0, 0.0);
        assert!(
            has_line_of_sight(&obstacles, ramp_top, platform_top),
            "a sightline riding along the platform top (y≈4, above the y=3 surfaces) \
             must be clear — elevation grants sight"
        );
    }

    /// AE5 headless smoke: a TestVerticality match parses and runs to
    /// completion (config accepts the test map; the sim doesn't panic on it).
    #[test]
    fn verticality_headless_match_runs_to_completion() {
        let mut cfg = create_config(vec!["Warrior", "Priest"], vec!["Mage", "Priest"], Some(1));
        cfg.map = "TestVerticality".to_string();
        cfg.max_duration_secs = 60.0;
        let result = run_headless_match_with(cfg, true, None)
            .expect("TestVerticality headless match should run to completion");
        assert!(
            result.match_time > 0.0,
            "match should have advanced some sim time"
        );
    }

    // AE2 / AE3 — completion fizzle + projectiles still land.
    //
    // On PillaredArena, a Mage's Frostbolt cast can START with LoS to its
    // target and then lose it mid-cast as the target moves behind a pillar; the
    // cast fizzles at COMPLETION ("fails to cast Frostbolt: target out of line
    // of sight"). AE3 is the complementary guarantee: this does NOT swallow
    // projectiles that were already launched — a Frostbolt that left the wand
    // before the target broke LoS still travels and lands ("Frostbolt hits").
    //
    // We assert both from the same match's combat log, in every match of a
    // seed bank of the reference matchup (Mage+Priest v Warrior+Priest on
    // TwinPillars) where a cast fizzled at completion — so a change that moves
    // the RNG stream moves WHICH seeds fizzle, not whether the probe judges
    // anything. (Two pinned seeds carried it through six re-pins; AS-282
    // migrated it here.)

    /// The seeds [`completion_fizzle_and_projectiles_still_land`] plays.
    const FIZZLE_BANK: std::ops::Range<u64> = 0..40;

    /// The fewest matches of the bank in which a cast must fizzle at completion.
    const MIN_FIZZLE_MATCHES: usize = 3;

    /// Run one PillaredArena match and return its combat-log text.
    fn pillared_log(seed: u64) -> String {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        drop(tmp);
        let mut cfg = create_config(
            vec!["Mage", "Priest"],
            vec!["Warrior", "Priest"],
            Some(seed),
        );
        cfg.map = "TwinPillars".to_string();
        cfg.max_duration_secs = 120.0;
        cfg.output_path = Some(path.to_string_lossy().into_owned());
        // suppress_log = false so the .txt log is written to output_path.
        run_headless_match_with(cfg, false, None).expect("PillaredArena match runs");
        let body = std::fs::read_to_string(&path).unwrap_or_default();
        let _ = std::fs::remove_file(&path);
        body
    }

    /// Exploratory scan of the completion-fizzle bank. Ignored by default.
    #[test]
    #[ignore]
    fn scan_fizzle_seeds() {
        for seed in FIZZLE_BANK {
            let log = pillared_log(seed);
            let fizzles = log
                .lines()
                .filter(|l| l.contains("fails to cast") && l.contains("line of sight"))
                .count();
            let impacts = log
                .lines()
                .filter(|l| l.contains("Frostbolt hits") || l.contains("Frostbolt CRITS"))
                .count();
            eprintln!(
                "seed {seed:2}: fizzles={:2} impacts={:2}{}",
                fizzles,
                impacts,
                if fizzles >= 1 && impacts >= 1 {
                    " <-- GOOD"
                } else {
                    ""
                },
            );
        }
    }

    #[test]
    fn completion_fizzle_and_projectiles_still_land() {
        let mut fizzled = 0;
        for seed in FIZZLE_BANK {
            let log = pillared_log(seed);

            let fizzles = log
                .lines()
                .filter(|l| l.contains("fails to cast") && l.contains("line of sight"))
                .count();
            // The situation: an in-flight cast fizzled at completion for LoS (AE2).
            if fizzles == 0 {
                continue;
            }
            fizzled += 1;
            let impacts = log
                .lines()
                .filter(|l| l.contains("Frostbolt hits") || l.contains("Frostbolt CRITS"))
                .count();
            // AE3: launched projectiles still landed in the same match.
            assert_min_occurrences(&format!("seed {seed} Frostbolt impacts"), impacts, 1);
        }
        assert_min_occurrences(
            "matches of the bank with a completion LoS fizzle",
            fizzled,
            MIN_FIZZLE_MATCHES,
        );
    }
}

// ---------------------------------------------------------------------------
// The kiter's chase of a lone pillar-hugging healer
// ---------------------------------------------------------------------------
//
// The two seed-bank probes that lived here — the occlusion-timeout chase
// (`chase_los`) and the leaky-bucket juke dance (`juke_chase`) — judged a lone
// Shaman hiding at a TwinPillars pillar, and that hider was the TakeCover play.
// TakeCover was deleted at the 0.8 milestone sweep and no AI hides a lone
// healer at a pillar any more, so their banks held nothing for their bounds to
// catch. The chase is now pinned where the hug holds by construction, a staged
// match (`tests/lone_healer_chase_staged.rs`). A juking hider cannot be staged,
// so the dance has no match-level probe; the bucket's arithmetic under a juke
// cadence is pinned by
// `dps_postures::tests::juke_cadence_arms_at_shipped_decay_but_never_when_flicker_empties_the_bucket`.

// ---------------------------------------------------------------------------
// Medic chase (heal-seeking movement) on PillaredArena
// ---------------------------------------------------------------------------
//
// The defect (fix 1): R5 made heals LoS-gated, but nothing moved a healer to
// REGAIN sight of a dying ally. A healer standing pillar-side from a sub-urgency
// teammate had FREE formation-follow (no sight requirement) or PRESSURED
// cover-denial pulling it AWAY — the ally died with heals silently LoS-rejected
// at cast start. The medic chase overrides FREE/PRESSURED with a direct
// `MovementGoal::Point` walk toward the dying occluded ally (SeekLos trigger);
// the chase ends when sight is regained and the heal fires.
//
// This probe drives Warrior+Priest vs Warrior+Shaman on PillaredArena — the
// task's suggested cleaner comp, where a healer routinely gets pillar-separated
// from its focused Warrior partner (the reported Mage+Paladin vs Shaman+Warlock
// comp keeps too tight a formation to produce the window reliably). For each
// (healer, ally) pair we find windows where the ally is BELOW the urgency
// threshold AND occluded from its healer, and assert the longest such window
// is bounded: the medic chase walks the healer around the pillar within a few
// seconds. The scan also prints the time to the healer's next successful heal
// (an HP increase on the ally — there is no passive HP regen, so any rise is a
// landed heal), which the window bounds but an enemy interrupt or burst can
// still delay.
mod medic_chase {
    use super::*;

    use arenasim::states::match_config::ArenaMap;
    use arenasim::states::play_match::map_config::load_map_geometry_config;
    use arenasim::states::play_match::map_geometry::{
        has_line_of_sight, ObstacleVolume, EYE_HEIGHT,
    };

    /// urgency_hp_threshold (movement.ron shipped default) — the medic-chase and
    /// deny-urgency threshold. The probe pins behavior at defaults.
    const URGENCY: f32 = 0.5;
    /// Minimum HP-fraction rise between consecutive frames counted as a landed
    /// heal (filters float noise; there is no passive HP regen in combat).
    const HEAL_EPS: f32 = 0.01;

    #[derive(Clone, Copy)]
    struct Sample {
        t: f32,
        pos: Vec3,
        hp: f32, // fraction 0..1
        alive: bool,
    }

    fn medic_config(seed: u64) -> HeadlessMatchConfig {
        HeadlessMatchConfig {
            team1: vec!["Warrior".into(), "Priest".into()],
            team2: vec!["Warrior".into(), "Shaman".into()],
            max_duration_secs: 200.0,
            random_seed: Some(seed),
            map: "TwinPillars".to_string(),
            ..Default::default()
        }
    }

    fn sees(obstacles: &[ObstacleVolume], a: Vec3, b: Vec3) -> bool {
        has_line_of_sight(
            obstacles,
            Vec3::new(a.x, EYE_HEIGHT, a.z),
            Vec3::new(b.x, EYE_HEIGHT, b.z),
        )
    }

    /// Per-entity full timelines (all frames, alive flag carried) collected via
    /// the read-only observer.
    fn collect(
        seed: u64,
    ) -> (
        Option<u8>,
        f32,
        BTreeMap<Entity, Vec<Sample>>,
        BTreeMap<Entity, EntityInfo>,
    ) {
        let mut samples: BTreeMap<Entity, Vec<Sample>> = BTreeMap::new();
        let mut info: BTreeMap<Entity, EntityInfo> = BTreeMap::new();
        let result = run_headless_match_observed(medic_config(seed), true, None, |frame| {
            if !frame.gates_open {
                return;
            }
            for (e, obs) in &frame.combatants {
                info.entry(*e).or_insert(EntityInfo {
                    team: obs.team,
                    slot: obs.slot,
                    class: obs.class,
                    is_pet: obs.is_pet,
                });
                let hp = if obs.max_health > 0.0 {
                    obs.current_health / obs.max_health
                } else {
                    0.0
                };
                samples.entry(*e).or_default().push(Sample {
                    t: frame.sim_time,
                    pos: obs.position,
                    hp,
                    alive: obs.alive,
                });
            }
        })
        .expect("observed medic match failed");
        (result.winner, result.match_time, samples, info)
    }

    fn find(info: &BTreeMap<Entity, EntityInfo>, team: u8, class: CharacterClass) -> Entity {
        info.iter()
            .find(|(_, i)| i.team == team && i.class == class && !i.is_pet)
            .map(|(e, _)| *e)
            .unwrap_or_else(|| panic!("no team-{team} {class:?}"))
    }

    struct PairStats {
        /// Number of distress-window starts (ally sub-urgency AND occluded).
        windows: usize,
        /// Total distress frames (vacuity depth).
        distress_frames: usize,
        /// Longest contiguous distress window in sim-seconds.
        max_window: f32,
        /// Worst (largest) time-to-heal across windows that resolved with a
        /// heal, in sim-seconds. `None` if no window resolved with a heal.
        worst_time_to_heal: Option<f32>,
        /// Count of windows where the ally DIED before any heal landed.
        died_before_heal: usize,
    }

    /// Measure medic behavior for healer H protecting ally A (same team).
    fn measure_pair(obstacles: &[ObstacleVolume], healer: &[Sample], ally: &[Sample]) -> PairStats {
        // Match on identical frame stamps (both entities sampled every gated
        // frame until death).
        let mut matched: Vec<(f32, Sample, Sample)> = Vec::new();
        let (mut i, mut j) = (0usize, 0usize);
        while i < healer.len() && j < ally.len() {
            if healer[i].t == ally[j].t {
                matched.push((healer[i].t, healer[i], ally[j]));
                i += 1;
                j += 1;
            } else if healer[i].t < ally[j].t {
                i += 1;
            } else {
                j += 1;
            }
        }

        let distressed = |h: &Sample, a: &Sample| -> bool {
            h.alive && a.alive && a.hp < URGENCY && !sees(obstacles, h.pos, a.pos)
        };

        let mut windows = 0usize;
        let mut distress_frames = 0usize;
        let mut max_window = 0.0f32;
        let mut worst_time_to_heal: Option<f32> = None;
        let mut died_before_heal = 0usize;

        let mut k = 0usize;
        while k < matched.len() {
            let (_, h, a) = matched[k];
            if !distressed(&h, &a) {
                k += 1;
                continue;
            }
            // Window start at k.
            windows += 1;
            let start_t = matched[k].0;
            // Walk to the end of this contiguous distress run.
            let mut end = k;
            while end + 1 < matched.len() {
                let (_, hn, an) = matched[end + 1];
                if distressed(&hn, &an) {
                    end += 1;
                } else {
                    break;
                }
            }
            distress_frames += end - k + 1;
            max_window = max_window.max(matched[end].0 - start_t);

            // From window start, find the next landed heal (ally HP rise) at any
            // later frame — the chase regains sight, then the heal fires.
            let mut resolved = false;
            let mut prev_hp = a.hp;
            let mut m = k + 1;
            while m < matched.len() {
                let (t, _, am) = matched[m];
                if !am.alive {
                    break; // ally died before a heal landed
                }
                if am.hp - prev_hp >= HEAL_EPS {
                    let ttl = t - start_t;
                    worst_time_to_heal = Some(worst_time_to_heal.map_or(ttl, |w| w.max(ttl)));
                    resolved = true;
                    break;
                }
                prev_hp = am.hp;
                m += 1;
            }
            if !resolved {
                // Reached ally death or end-of-match with no heal after start.
                let ally_died = matched[k..].iter().any(|(_, _, am)| !am.alive);
                if ally_died {
                    died_before_heal += 1;
                }
            }

            k = end + 1;
        }

        PairStats {
            windows,
            distress_frames,
            max_window,
            worst_time_to_heal,
            died_before_heal,
        }
    }

    struct SeedStats {
        winner: Option<u8>,
        duration: f32,
        priest: PairStats,
        shaman: PairStats,
    }

    fn measure(seed: u64) -> SeedStats {
        let obstacles = load_map_geometry_config()
            .expect("maps.ron loads")
            .active_for(ArenaMap::TwinPillars)
            .volumes;
        assert!(
            !obstacles.is_empty(),
            "PillaredArena must carry cover volumes"
        );

        let (winner, duration, samples, info) = collect(seed);

        let t1_priest = find(&info, 1, CharacterClass::Priest);
        let t1_warrior = find(&info, 1, CharacterClass::Warrior);
        let t2_shaman = find(&info, 2, CharacterClass::Shaman);
        let t2_warrior = find(&info, 2, CharacterClass::Warrior);

        let empty = Vec::new();
        let g = |e: &Entity| samples.get(e).unwrap_or(&empty).as_slice();

        let priest = measure_pair(&obstacles, g(&t1_priest), g(&t1_warrior));
        let shaman = measure_pair(&obstacles, g(&t2_shaman), g(&t2_warrior));

        SeedStats {
            winner,
            duration,
            priest,
            shaman,
        }
    }

    /// Exploratory seed scan — prints per-seed, per-pair medic stats over the
    /// bank [`medic_bounds_distressed_ally_over_the_seed_bank`] judges. Ignored
    /// by default.
    #[test]
    #[ignore]
    fn scan_seeds() {
        for seed in MEDIC_BANK {
            let s = measure(seed);
            let fmt = |p: &PairStats| {
                format!(
                    "win={:2} dfr={:4} maxw={:5.2} tth={:>5} died={}",
                    p.windows,
                    p.distress_frames,
                    p.max_window,
                    p.worst_time_to_heal
                        .map(|t| format!("{t:.2}"))
                        .unwrap_or_else(|| "none".into()),
                    p.died_before_heal,
                )
            };
            eprintln!(
                "seed {seed:2}: winner={:?} dur={:5.1} | Priest[{}] | Shaman[{}]",
                s.winner,
                s.duration,
                fmt(&s.priest),
                fmt(&s.shaman),
            );
        }
    }

    /// The seeds the medic probe plays. It judges the matches in which the
    /// team-1 Priest is pillar-separated from its Warrior while the Warrior is
    /// below the urgency threshold for at least a second in all, so a change
    /// that moves the RNG stream moves WHICH seeds hold the situation, not
    /// whether the probe judges anything. (Two pinned seeds carried it until
    /// they had been re-picked six times; AS-293 migrated it here.)
    const MEDIC_BANK: std::ops::Range<u64> = 0..30;

    /// Occluded-distress frames (60 = one second) a match must hold to be
    /// judged.
    const DISTRESS_FRAMES: usize = 60;

    /// The fewest judged matches the bank must hold (19 of 30 on `main` @
    /// 68b9a0c, 15 after AS-293).
    const MIN_DISTRESSED: usize = 8;

    /// A match deep enough in distress to give the bound something to catch:
    /// five seconds of it (the deepest on `main` @ 68b9a0c held 916 frames).
    const DEEP_DISTRESS_FRAMES: usize = 300;

    /// The medic chase bounds how long a healer stays occluded from a DYING
    /// ally. In every match of the bank where the team-1 Priest is
    /// pillar-separated from its Warrior while the Warrior is below the
    /// urgency threshold, the longest contiguous occluded-distress window is
    /// bounded: the chase walks the Priest around the pillar to regain sight
    /// within a few seconds, not the tens of seconds an un-chasing
    /// formation-follower would take. The bound is the walk's; whether the
    /// heal that follows lands in time is not, because an enemy interrupt,
    /// purge or burst decides that too (`scan_seeds` prints it per match).
    #[test]
    fn medic_bounds_distressed_ally_over_the_seed_bank() {
        let distressed: Vec<(u64, PairStats)> = MEDIC_BANK
            .map(|seed| (seed, measure(seed).priest))
            .filter(|(_, p)| p.distress_frames >= DISTRESS_FRAMES)
            .collect();

        // Vacuity: the situation occurs, in enough matches to be a sample,
        // and at least one of them deep enough for the bound to mean something.
        assert_min_occurrences(
            "matches with a pillar-separated, distressed Warrior in the bank",
            distressed.len(),
            MIN_DISTRESSED,
        );
        let deepest = distressed
            .iter()
            .map(|(_, p)| p.distress_frames)
            .max()
            .unwrap_or(0);
        assert!(
            deepest >= DEEP_DISTRESS_FRAMES,
            "the bank's deepest distress held {deepest} frames (< {DEEP_DISTRESS_FRAMES}) — \
             the bound has nothing to catch",
        );

        for (seed, p) in &distressed {
            // Observed longest windows across the bank are ~2-6s; 8s is a
            // comfortable ceiling that a regression removing the chase (a
            // formation-follower drifting into sight only incidentally) would
            // blow past.
            assert!(
                p.max_window <= 8.0,
                "seed {seed}: Priest's longest occluded-from-dying-Warrior window was {:.2}s \
                 (> 8s) — the medic chase is not regaining sight of the dying ally",
                p.max_window,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Mage OOM wand-pull fallback probe
//
// When the Mage runs out of mana for its primary nuke (Frostbolt), its ENGAGE
// pursuit stop distance drops to wand range so its equipped wand auto-attack
// fires — instead of parking at preferred range (38yd), outside wand range
// (30yd), and idling the mana refractory. Originally diagnosed on Mage+Priest
// vs Warrior+Shaman on PillaredArena: after the Warrior dies, the lone-Shaman
// 2v1 dragged because the Mage stood at ~36yd dealing damage once per ~20s. The
// OOM fallback closes it to 30yd and lets the wand chip fill the refractory.
//
// NOTE (2026-07-23): the "mana charged only on successful cast completion" fix
// means the Mage reaches OOM far LESS (juked Frostbolts no longer drain it), so
// the lone-Shaman endgame is now dampening-gated rather than mana-gated, and
// the probe's duration speedup proxy was retired (see the outcome assertion).
//
// NOTE (AS-138): what this probe pins is NOT the OOM fallback. That was checked
// by neutering the `KitePosture::wand_oom` arm of the pursuit stop distance and
// re-scanning: the counts are unchanged, on this tree at every seed in 0..40
// and on main @9f80a37 at 39 of 40 including its own pin. The Mage reaches wand
// range of the lone Shaman whether or not the fallback exists — the KITE orbit
// and the Shaman's own 28yd parking bring them inside 30yd anyway. Read every
// assertion below as a BEHAVIOUR PIN on the 2v1 (the Mage does close, and does
// chip through the window), never as evidence the latch fired. Isolating the
// latch needs a scenario where the stop distance is the only thing that can
// close the gap; see the block above the wand-shot floor.
// ---------------------------------------------------------------------------

mod oom_wand {
    use super::*;
    use arenasim::states::play_match::constants::WAND_RANGE;

    /// One damage event parsed from the combat log: `(wall_time, is_wand)`.
    struct MageDamage {
        wall_time: f32,
        is_wand: bool,
    }

    /// Parsed match: result, position timeline, the Warrior death time, and the
    /// Mage's combat-log damage events (wall-clock; wand auto-attacks are never
    /// traced, so they are recovered from the log).
    struct Parsed {
        result: MatchResult,
        timeline: Timeline,
        /// Warrior death time (wall clock) — the lone-Shaman 2v1 opens here.
        warrior_death_wall: Option<f32>,
        mage_damage: Vec<MageDamage>,
    }

    fn config_seed(seed: u64) -> HeadlessMatchConfig {
        HeadlessMatchConfig {
            team1: vec!["Mage".into(), "Priest".into()],
            team2: vec!["Warrior".into(), "Shaman".into()],
            map: "TwinPillars".into(),
            max_duration_secs: 300.0,
            random_seed: Some(seed),
            ..Default::default()
        }
    }

    /// Parse a `[  12.34s] ...` leading timestamp (wall clock) off a log line.
    fn log_time(line: &str) -> Option<f32> {
        let open = line.find('[')?;
        let close = line[open..].find("s]")? + open;
        line[open + 1..close].trim().parse::<f32>().ok()
    }

    fn run(config: HeadlessMatchConfig) -> Parsed {
        run_with(config, None)
    }

    fn run_with(config: HeadlessMatchConfig, stage: Option<arenasim::headless::Stage>) -> Parsed {
        // The OOM wand fallback is a movement mechanism (pursuit stop distance),
        // not a traced decision — and wand auto-attacks are never traced either.
        // So the probe recovers the Mage's damage timing from the COMBAT LOG,
        // written to a temp `output_path`.
        let log_tmp = tempfile::NamedTempFile::new().unwrap();
        let log_path = log_tmp.path().to_path_buf();
        drop(log_tmp);

        let mut cfg = config;
        cfg.output_path = Some(log_path.to_string_lossy().into_owned());

        // suppress_log MUST be false so the match-end system writes the combat
        // log to `output_path`. suppress_log gates output only, not the RNG/sim,
        // so the outcome is identical to a suppressed run.
        let mut timeline = Timeline::default();
        let result = match stage {
            Some(stage) => {
                arenasim::headless::run_headless_match_staged(cfg, false, None, stage, |frame| {
                    timeline.record(frame)
                })
            }
            None => run_headless_match_observed(cfg, false, None, |frame| timeline.record(frame)),
        }
        .expect("observed match failed");

        let log_body = std::fs::read_to_string(&log_path).expect("read combat log");
        let _ = std::fs::remove_file(&log_path);

        let mut warrior_death_wall = None;
        let mut mage_damage = Vec::new();
        for line in log_body.lines() {
            if warrior_death_wall.is_none()
                && line.contains("[DEATH]")
                && line.contains("Team 2 Warrior")
            {
                warrior_death_wall = log_time(line);
            }
            // Ids carry a "#slot" suffix now ("Team 1 Mage #1's Frostbolt ...").
            if line.contains("[DMG]") && line.contains("Team 1 Mage #") {
                if let Some(t) = log_time(line) {
                    mage_damage.push(MageDamage {
                        wall_time: t,
                        is_wand: line.contains("Wand Shot"),
                    });
                }
            }
        }

        Parsed {
            result,
            timeline,
            warrior_death_wall,
            mage_damage,
        }
    }

    fn xz_distance(a: Vec3, b: Vec3) -> f32 {
        let (dx, dz) = (a.x - b.x, a.z - b.z);
        (dx * dx + dz * dz).sqrt()
    }

    /// The lone-Shaman 2v1, STAGED (tier 3): a Mage+Priest against a lone
    /// Shaman, the Mage out of mana and 38yd off — its Frostbolt-safe parking
    /// range, outside its wand's 30yd. The Mage must close to wand range within
    /// a bounded time, land wand shots through the window its mana refractory
    /// would leave dead, and team 1 must win well under the cap.
    ///
    /// It replaced a seeded match (`config_seed`, Mage+Priest v Warrior+Shaman
    /// on TwinPillars) re-pinned six times as the AI moved — most recently
    /// AS-249, whose healer positioning took the window's wand shots at seed
    /// 26 from 10 to 1. A stage holds the 2v1 and the empty pool by
    /// construction, so this one also puts the OOM fallback under test: the
    /// seeded match reached wand range "for other reasons" (the AS-138 note
    /// above).
    #[test]
    fn mage_oom_closes_to_wand_range_and_breaks_the_dead_window() {
        use arenasim::headless::{Stage, Unit};
        const MAGE: Unit = Unit::new(1, 0);
        const PRIEST: Unit = Unit::new(1, 1);
        const SHAMAN: Unit = Unit::new(2, 0);
        for seed in [1u64, 2, 3] {
            let config = HeadlessMatchConfig {
                team1: vec!["Mage".into(), "Priest".into()],
                team2: vec!["Shaman".into()],
                map: "TwinPillars".into(),
                max_duration_secs: 300.0,
                random_seed: Some(seed),
                ..Default::default()
            };
            let stage = Stage::new()
                .place(MAGE, -20.0, 0.0)
                .mana(MAGE, 0.0)
                .place(PRIEST, -28.0, 4.0)
                .place(SHAMAN, 18.0, 0.0)
                .target(SHAMAN, PRIEST)
                .target(MAGE, SHAMAN);
            let p = run_with(config, Some(stage));
            let gates = p.timeline.gates_open_time.expect("the gates open");

            let mage = p.timeline.find(1, CharacterClass::Mage, false);
            let shaman = p.timeline.find(2, CharacterClass::Shaman, false);
            let paired: Vec<(f32, f32)> = {
                let shaman_at: BTreeMap<u32, Vec3> = p.timeline.samples[&shaman]
                    .iter()
                    .map(|(t, pos)| ((t * 60.0).round() as u32, *pos))
                    .collect();
                p.timeline.samples[&mage]
                    .iter()
                    .filter_map(|(t, pm)| {
                        let ps = shaman_at.get(&((t * 60.0).round() as u32))?;
                        Some((*t, xz_distance(*pm, *ps)))
                    })
                    .collect()
            };
            assert_min_occurrences("lone-Shaman (Mage,Shaman) samples", paired.len(), 200);

            // (a) The Mage closes to within wand range of the lone Shaman,
            // within a bounded time — the whole point is to stop parking at
            // preferred range (38yd) outside wand range (30yd).
            let reached = paired
                .iter()
                .find(|(_, d)| *d <= WAND_RANGE + 0.5)
                .map(|(t, _)| *t)
                .unwrap_or_else(|| panic!("seed {seed}: the Mage never reached wand range"));
            assert!(
                reached - gates <= 20.0,
                "seed {seed}: the Mage took {:.1}s to reach wand range of the lone Shaman \
                 (> 20s bound)",
                reached - gates
            );

            // (b) The wand chips through the refractory: the Mage starts with
            // no mana for a Frostbolt, so before the fallback it dealt nothing
            // here.
            let wand_shots = p
                .mage_damage
                .iter()
                .filter(|d| d.is_wand && d.wall_time <= gates + 60.0)
                .count();
            assert!(
                wand_shots >= 4,
                "seed {seed}: the Mage landed only {wand_shots} wand shots in its first 60s \
                 out of mana — it is not closing to wand range and chipping"
            );

            // Outcome: team 1 wins by elimination, well under the cap.
            assert_eq!(
                p.result.winner,
                Some(1),
                "seed {seed}: team 1 should win the 2v1"
            );
            assert!(
                p.result.match_time < 200.0,
                "seed {seed}: the 2v1 took {:.1}s combat",
                p.result.match_time
            );
        }
    }

    /// Exploratory seed scan for OOM re-pinning. Ignored by default. A good pin
    /// opens a lone-Shaman 2v1, has the Mage close to wand range and land wand
    /// shots (>= 4 wand / >= 9 total post-death damage events), and resolves for
    /// team 1 in < 68s.
    #[test]
    #[ignore]
    fn scan_oom_seeds() {
        for seed in 0u64..40 {
            let p = run(config_seed(seed));
            let Some(wdeath) = p.warrior_death_wall else {
                eprintln!("seed {seed:2}: no warrior death");
                continue;
            };
            let post: Vec<&MageDamage> = p
                .mage_damage
                .iter()
                .filter(|d| d.wall_time >= wdeath)
                .collect();
            let wand = post.iter().filter(|d| d.is_wand).count();
            let good = p.result.winner == Some(1)
                && p.result.match_time < 68.0
                && wand >= 4
                && post.len() >= 9;
            eprintln!(
                "seed {seed:2}: winner={:?} t={:5.1} wdeath={:5.1} wand={:2} postdmg={:2}{}",
                p.result.winner,
                p.result.match_time,
                wdeath,
                wand,
                post.len(),
                if good { " <-- GOOD" } else { "" },
            );
        }
    }
}

// ===========================================================================
// Warrior pillar pathfinding — tangent-steering effectiveness guard
// ===========================================================================
//
// A chasing melee unit must ROUND a pillar at (near) full speed, not ooze
// along its surface. Before tangent-steering (`steer_toward_goal`), a pursuer
// whose goal sat behind a pillar had only the tangential sliver left after
// `slide_against` removed the inward step component — it stuck to the surface
// and crawled. Tangent-steering aims at the obstacle's tangent point instead,
// so the mover arcs around the pillar keeping full speed.
//
// This pins that behavior directly on PillaredArena: a Warrior trains an enemy
// caster who kites and uses the pillars for cover, so the Warrior is
// repeatedly driven against a pillar with its target on the far side. We assert
// that while NEAR a pillar and STILL CHASING (target beyond melee), the
// Warrior's median speed stays near its own full-speed reference and it never
// falls into a sustained slow-crawl (ooze) episode.
//
// A companion to `u6_collision_smoke`: that guard proves the Warrior never
// enters a pillar's interior; this one proves it doesn't get *stuck to the
// outside* of one either.
mod warrior_pillar_pathing {
    use super::*;
    use arenasim::states::match_config::ArenaMap;
    use arenasim::states::play_match::map_config::load_map_geometry_config;
    use arenasim::states::play_match::map_geometry::{ObstacleVolume, MOVER_RADIUS};
    use std::collections::HashMap;

    /// "Near a pillar" = within this many yards of the collision shell
    /// (cylinder radius + `MOVER_RADIUS`). Matches the diagnosis band.
    const NEAR_BAND: f32 = 1.5;
    /// A sample counts as chasing (not parked on target) when the nearest
    /// living enemy is beyond this distance — comfortably past melee (2.5).
    const CHASING_GAP: f32 = 5.0;
    /// Below this fraction of the full-speed reference is a "slow" sample.
    const SLOW_FRAC: f32 = 0.4;
    /// Near-pillar-chasing median speed must be at least this fraction of full.
    const MIN_MEDIAN_FRAC: f32 = 0.9;
    /// A stall episode (consecutive near+chasing+slow samples) longer than this
    /// many sim-seconds is an ooze regression. A 1-2 frame dip at a genuine
    /// direction reversal is tolerated below it.
    const MAX_STALL_SECS: f32 = 0.5;

    /// PillaredArena cylinder footprints, loaded live (center_x, center_z, r).
    fn pillars() -> Vec<(f32, f32, f32)> {
        let geom = load_map_geometry_config()
            .expect("maps.ron loads")
            .active_for(ArenaMap::TwinPillars);
        let v: Vec<(f32, f32, f32)> = geom
            .volumes
            .iter()
            .filter_map(|v| match v {
                ObstacleVolume::Cylinder {
                    center_xz, radius, ..
                } => Some((center_xz.x, center_xz.y, *radius)),
                _ => None,
            })
            .collect();
        assert!(!v.is_empty(), "PillaredArena must carry cylinder pillars");
        v
    }

    fn config(seed: u64) -> HeadlessMatchConfig {
        let mut cfg = create_config(
            vec!["Warrior", "Priest"],
            vec!["Mage", "Priest"],
            Some(seed),
        );
        cfg.map = "TwinPillars".to_string();
        cfg
    }

    /// Clearance of a point from the nearest pillar's collision shell.
    /// Negative = inside the shell.
    fn shell_clearance(pos: Vec3, pillars: &[(f32, f32, f32)]) -> f32 {
        pillars
            .iter()
            .map(|&(px, pz, r)| {
                ((pos.x - px).powi(2) + (pos.z - pz).powi(2)).sqrt() - (r + MOVER_RADIUS)
            })
            .fold(f32::INFINITY, f32::min)
    }

    #[test]
    fn warrior_rounds_pillars_at_full_speed_while_chasing() {
        let pillars = pillars();
        // Aggregate across seeds so the per-seed sample count is generous and
        // the non-vacuity guard is meaningful.
        let mut near_chase_speeds: Vec<f32> = Vec::new();
        let mut worst_stall_secs = 0.0f32;
        let mut total_samples = 0usize;

        for seed in [1u64, 3u64, 7u64] {
            let (_result, timeline) = run_observed_collecting(config(seed));
            let gate = timeline.gates_open_time.unwrap_or(0.0);
            let warrior = timeline.find(1, CharacterClass::Warrior, false);

            // Enemy (team 2) living-position lookup, keyed by exact frame time.
            let enemies: Vec<Entity> = timeline
                .info
                .iter()
                .filter(|(_, i)| i.team == 2 && !i.is_pet)
                .map(|(e, _)| *e)
                .collect();
            let enemy_pos: Vec<HashMap<u32, Vec3>> = enemies
                .iter()
                .map(|e| {
                    timeline
                        .samples
                        .get(e)
                        .map(|s| s.iter().map(|&(t, p)| (t.to_bits(), p)).collect())
                        .unwrap_or_default()
                })
                .collect();
            let gap_at = |t: f32, wp: Vec3| -> Option<f32> {
                let key = t.to_bits();
                enemy_pos
                    .iter()
                    .filter_map(|m| m.get(&key).map(|p| wp.distance(*p)))
                    .fold(None, |acc: Option<f32>, d| {
                        Some(acc.map_or(d, |a| a.min(d)))
                    })
            };

            let samples = timeline.samples_from(warrior, gate);
            assert_min_occurrences(
                &format!("seed {} Warrior post-gate samples", seed),
                samples.len(),
                60,
            );
            total_samples += samples.len();

            // Full-speed reference = p90 of all post-gate per-sample speeds.
            let mut all_speeds: Vec<f32> = samples
                .windows(2)
                .filter_map(|w| {
                    let dt = w[1].0 - w[0].0;
                    (dt > 0.0).then(|| w[0].1.distance(w[1].1) / dt)
                })
                .collect();
            assert!(!all_speeds.is_empty(), "seed {}: no motion samples", seed);
            all_speeds.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let full = all_speeds[(0.9 * (all_speeds.len() as f32 - 1.0)) as usize];
            assert!(
                full > 3.0,
                "seed {}: implausible full-speed reference {:.2} — Warrior barely moved",
                seed,
                full
            );

            // Walk consecutive samples; classify each near-pillar-chasing one,
            // tracking the longest sustained slow (ooze) run.
            let mut run_start: Option<f32> = None;
            for w in samples.windows(2) {
                let (t0, p0) = w[0];
                let (t1, p1) = w[1];
                let dt = t1 - t0;
                if dt <= 0.0 {
                    continue;
                }
                let speed = p0.distance(p1) / dt;
                let near = shell_clearance(p1, &pillars) <= NEAR_BAND;
                let chasing = gap_at(t1, p1).is_some_and(|g| g > CHASING_GAP);

                if near && chasing {
                    near_chase_speeds.push(speed);
                    if speed < SLOW_FRAC * full {
                        // extend / open a stall run
                        let start = *run_start.get_or_insert(t0);
                        worst_stall_secs = worst_stall_secs.max(t1 - start);
                    } else {
                        run_start = None;
                    }
                } else {
                    run_start = None;
                }
            }
        }

        // 1. Non-vacuity: the scenario must actually drive the Warrior against
        //    pillars while chasing, or the probe proves nothing.
        assert_min_occurrences(
            "Warrior near-pillar chasing samples",
            near_chase_speeds.len(),
            30,
        );

        // 2. No oozing: median near-pillar-chasing speed stays near full speed.
        near_chase_speeds.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = near_chase_speeds[near_chase_speeds.len() / 2];
        let full_ref = {
            // Recompute a global full reference as the max median seen — the
            // per-seed full speeds are effectively identical (fixed move speed),
            // so the top near-pillar speed is a fine full-speed proxy here.
            let mut s = near_chase_speeds.clone();
            s.sort_by(|a, b| a.partial_cmp(b).unwrap());
            s[(0.9 * (s.len() as f32 - 1.0)) as usize]
        };
        assert!(
            median >= MIN_MEDIAN_FRAC * full_ref,
            "Warrior oozes near pillars: near-pillar-chasing median speed {:.2} is < {:.0}% of \
             full {:.2} ({} samples across seeds)",
            median,
            MIN_MEDIAN_FRAC * 100.0,
            full_ref,
            near_chase_speeds.len()
        );

        // 3. No sustained stall (ooze) episode.
        assert!(
            worst_stall_secs < MAX_STALL_SECS,
            "Warrior stalls against a pillar for {:.2}s (>= {:.2}s) — tangent-steering regressed",
            worst_stall_secs,
            MAX_STALL_SECS
        );

        assert!(total_samples > 0, "no samples collected");
    }
}

// ---------------------------------------------------------------------------
// Pillar self-blocking probes (Nagrand / TeamPlan)
// ---------------------------------------------------------------------------
//
// These probes pin a PATHOLOGY, in the idiom of the U2 inverted-statue probe:
// they assert what the AI does today so that a fix flips them loudly, rather
// than asserting the behaviour we want and sitting red.
//
// The finding they capture: a `TeamPlan` healer near a Nagrand pillar puts that
// same pillar between itself and the ally it is supposed to heal, and oscillates
// across the blocking axis instead of committing to a side. `Legacy` measures
// 0% on the same metric, so this is not baseline healer behaviour.
//
// CAUSE, REVISED 2026-08-01 (the probes are unchanged; only the attribution is).
// The original reading was that the healer cannot decide which way to round its
// pillar. That is the symptom. The cause was that the camp NEVER ENDED: its
// release asked "is an enemy within 15yd of me", which for a healer facing a
// ranged comp is never true, so the healer stayed welded to a ring 8.5yd around
// its pillar for the whole match with its posture AI suppressed — 71-79% of
// post-contact frames still camping, measured on these three seeds. Ending the
// camp at team contact (`teams_in_contact`) cut the blocked share from
// 40.0/51.5/55.1% to 36.7/35.5/22.5% and the longest blackout from 20.4s to
// 14.3s, and took the 12-seed paired win column from 5/12 back to 9/12 — level
// with `Legacy`, with 28.3 occlusion-seconds per match bought against its 0.0.
//
// STEP 4b LARGELY FIXED THIS, and the probes are kept because a smaller version
// survives. Routing the healer's positioning through the team solve's
// `OccupyCover` — one query for "hidden from their casters, in range of my ally,
// and able to SEE my ally" — moved every metric:
//
//                          Legacy   step 3   step 4b
//   team-1 wins (12 seeds)   9/12     9/12     10/12
//   heal line occluded post    0%      39%       14%
//   heal delivered to Warrior 481      295       347
//   Warlock denied sight     0.0s    28.3s     30.8s
//   Warrior died             3/12     4/12      2/12
//
// On these three seeds the blocked share fell 36.7/35.5/22.5% -> 13.8/18.1/12.3%
// and the longest blackout 14.25s -> 6.12s. What is left is the irreducible part:
// a healer standing near a pillar sometimes has that pillar between it and a
// MOVING ally, and one solve tick cannot preempt where the ally runs next.
// Closing it further needs the ally's predicted position, which is step 6's
// lethality/extrapolation work, not step 4's.
// Re-run `cargo test --release --test camp_sweep -- --ignored --nocapture` for
// the current numbers.
//
// Why the trace could not measure this: trace events fire on decisions, and a
// unit under sustained CC makes none. On seed 11 the Warrior stopped emitting
// at 39.0s but did not die until 50.12s, leaving the decisive window blind.
// The observer harness samples every frame regardless of decisions, so it sees
// through CC.
//
// GEOMETRY APPROXIMATION: the pillars are octagonal prisms (circumradius 6.0,
// apothem ~5.54). These helpers use the CIRCUMSCRIBED CIRCLE, which slightly
// over-counts blocking near the octagon's flats. Both headline assertions are
// robust to that choice: `self_blocked / all_blocked` is a ratio whose
// numerator and denominator use the same test, and the side-reversal rate does
// not depend on the radius at all. Only the raw blocked-fraction would move.
mod pillar_self_block {
    use super::*;

    /// Nagrand pillar centres on the x/z plane (`assets/config/maps.ron`).
    const PILLARS: [(f32, f32); 4] = [(-40.0, -20.0), (-40.0, 20.0), (40.0, -20.0), (40.0, 20.0)];
    /// Circumradius of the octagonal prisms. See the module note.
    const PILLAR_R: f32 = 6.0;

    /// Index and distance of the pillar nearest `p`.
    fn nearest_pillar(p: (f32, f32)) -> (usize, f32) {
        let mut best = (0usize, f32::MAX);
        for (i, c) in PILLARS.iter().enumerate() {
            let d = ((p.0 - c.0).powi(2) + (p.1 - c.1).powi(2)).sqrt();
            if d < best.1 {
                best = (i, d);
            }
        }
        best
    }

    /// Does segment `a..b` pass within `PILLAR_R` of pillar `c`?
    fn segment_hits(a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
        let (dx, dz) = (b.0 - a.0, b.1 - a.1);
        let (fx, fz) = (a.0 - c.0, a.1 - c.1);
        let aa = dx * dx + dz * dz;
        if aa < 1e-9 {
            return fx * fx + fz * fz <= PILLAR_R * PILLAR_R;
        }
        let t = (-(fx * dx + fz * dz) / aa).clamp(0.0, 1.0);
        let (px, pz) = (fx + t * dx, fz + t * dz);
        px * px + pz * pz <= PILLAR_R * PILLAR_R
    }

    /// Which pillar (if any) blocks `a..b`. First hit wins; pillars are far
    /// enough apart that a single segment rarely clips two.
    fn blocking_pillar(a: (f32, f32), b: (f32, f32)) -> Option<usize> {
        PILLARS.iter().position(|c| segment_hits(a, b, *c))
    }

    /// Which side of the (pillar -> target) axis `observer` stands on: the sign
    /// of the 2D cross product. `0.0` means collinear (on the axis itself).
    fn side_of(pillar: (f32, f32), target: (f32, f32), observer: (f32, f32)) -> f32 {
        let (ax, az) = (target.0 - pillar.0, target.1 - pillar.1);
        let (bx, bz) = (observer.0 - pillar.0, observer.1 - pillar.1);
        let cross = ax * bz - az * bx;
        if cross.abs() <= 1e-6 {
            0.0
        } else {
            cross.signum()
        }
    }

    fn xz(v: Vec3) -> (f32, f32) {
        (v.x, v.z)
    }

    struct SelfBlockStats {
        paired_samples: usize,
        blocked: usize,
        self_blocked: usize,
        reversals: usize,
        span_secs: f32,
        longest_self_block: f32,
    }

    /// Per-frame Priest-vs-Warrior blocking stats for one TeamPlan match.
    fn measure(seed: u64) -> SelfBlockStats {
        let mut cfg = create_config(
            vec!["Warrior", "Priest"],
            vec!["Warlock", "Priest"],
            Some(seed),
        );
        cfg.map = "PillaredArena".to_string();
        cfg.ai_profile = Some("TeamPlan".to_string());
        cfg.max_duration_secs = 300.0;

        let (_result, timeline) = run_observed_collecting(cfg);
        let priest = timeline.find(1, CharacterClass::Priest, false);
        let warrior = timeline.find(1, CharacterClass::Warrior, false);
        let gate = timeline.gates_open_time.expect("gates never opened");

        // Both timelines are alive-only and share the same frame clock, so an
        // exact sim_time join pairs them and drops frames after either death.
        let pri: BTreeMap<u32, Vec3> = timeline
            .samples_from(priest, gate)
            .into_iter()
            .map(|(t, p)| (t.to_bits(), p))
            .collect();

        let mut paired = 0usize;
        let (mut blocked, mut self_blocked, mut reversals) = (0usize, 0usize, 0usize);
        let mut prev_side: Option<f32> = None;
        let (mut first_t, mut last_t) = (f32::MAX, f32::MIN);
        let (mut longest, mut run_start): (f32, Option<f32>) = (0.0, None);

        for (t, wpos) in timeline.samples_from(warrior, gate) {
            let Some(ppos) = pri.get(&t.to_bits()) else {
                continue;
            };
            let (p, w) = (xz(*ppos), xz(wpos));
            paired += 1;
            first_t = first_t.min(t);
            last_t = last_t.max(t);

            let (near_idx, _) = nearest_pillar(p);
            let is_self_block = match blocking_pillar(p, w) {
                Some(idx) => {
                    blocked += 1;
                    if idx == near_idx {
                        self_blocked += 1;
                        true
                    } else {
                        false
                    }
                }
                None => false,
            };

            // Longest contiguous self-blocked stretch.
            match (is_self_block, run_start) {
                (true, None) => run_start = Some(t),
                (false, Some(s)) => {
                    longest = longest.max(t - s);
                    run_start = None;
                }
                _ => {}
            }

            let side = side_of(PILLARS[near_idx], w, p);
            if side != 0.0 {
                if prev_side.is_some_and(|s| s != side) {
                    reversals += 1;
                }
                prev_side = Some(side);
            }
        }
        if let Some(s) = run_start {
            longest = longest.max(last_t - s);
        }

        SelfBlockStats {
            paired_samples: paired,
            blocked,
            self_blocked,
            reversals,
            span_secs: if paired > 0 { last_t - first_t } else { 0.0 },
            longest_self_block: longest,
        }
    }

    /// PATHOLOGY PROBE (residual — see the module note for the revised cause).
    /// When a TeamPlan healer loses line of sight to the ally it is meant to
    /// heal, the obstacle is the pillar it is standing next to essentially
    /// always — not a distant pillar, not geometry it could not have avoided.
    /// Ending the camp at contact cut the blocked SHARE (40.0/51.5/55.1% ->
    /// 36.7/35.5/22.5%) but not the attribution, which stays pinned at 100%:
    /// step 4's team solve is what drives `self_blocked` toward zero and flips
    /// this probe.
    #[test]
    fn priest_blocks_its_own_line_to_the_warrior() {
        for seed in [11u64, 7, 12] {
            let s = measure(seed);
            assert!(
                s.paired_samples > 500,
                "seed {}: only {} paired samples — probe went vacuous",
                seed,
                s.paired_samples
            );
            assert_min_occurrences("frames with the heal line blocked", s.blocked, 1);

            let self_share = s.self_blocked as f32 / s.blocked as f32;
            println!(
                "seed {:2}: {} paired over {:.1}s | blocked {} ({:.1}%) | self {} ({:.0}% of blocked) | longest self-block {:.2}s",
                seed,
                s.paired_samples,
                s.span_secs,
                s.blocked,
                100.0 * s.blocked as f32 / s.paired_samples as f32,
                s.self_blocked,
                100.0 * self_share,
                s.longest_self_block,
            );
            assert!(
                self_share >= 0.9,
                "seed {}: expected the healer's own camp pillar to cause ~all blocking, got {:.0}% ({}/{})",
                seed,
                100.0 * self_share,
                s.self_blocked,
                s.blocked
            );
        }
    }

    /// PATHOLOGY PROBE (residual — see the module note for the revised cause).
    /// The healer does not commit to a side when rounding the pillar it is
    /// standing next to — it oscillates across the axis that runs through that
    /// pillar to its ally, which is what drops the heal line. Ending the camp at
    /// contact barely moved this (1.21/1.07/0.79 per second), which is itself
    /// the evidence that the thrash lives in the reactive `cover_pull` layer
    /// rather than in the camp: the camp is gone for most of these frames and
    /// the oscillation is not.
    #[test]
    fn priest_thrashes_across_the_pillar_axis() {
        for seed in [11u64, 7, 12] {
            let s = measure(seed);
            assert!(s.span_secs > 5.0, "seed {}: span too short to rate", seed);
            let rate = s.reversals as f32 / s.span_secs;
            println!(
                "seed {:2}: {} side reversals over {:.1}s = {:.2}/s",
                seed, s.reversals, s.span_secs, rate
            );
            assert!(
                rate >= 0.25,
                "seed {}: expected the healer to thrash across the pillar axis, got {:.2}/s",
                seed,
                rate
            );
        }
    }

    // --- pure-geometry unit tests (no simulation) ---

    #[test]
    fn segment_hits_detects_a_blocked_line() {
        // Straight through pillar 0's centre.
        assert!(segment_hits((-50.0, -20.0), (-30.0, -20.0), PILLARS[0]));
        // Parallel line well clear of it.
        assert!(!segment_hits((-50.0, 0.0), (-30.0, 0.0), PILLARS[0]));
        // Grazing just outside the circumradius.
        assert!(!segment_hits(
            (-50.0, -20.0 - PILLAR_R - 0.1),
            (-30.0, -20.0 - PILLAR_R - 0.1),
            PILLARS[0]
        ));
    }

    #[test]
    fn segment_hits_respects_endpoints_not_the_infinite_line() {
        // The infinite line through these points would hit pillar 0, but the
        // segment stops short of it — a mover behind its ally is not blocked.
        assert!(!segment_hits((-70.0, -20.0), (-60.0, -20.0), PILLARS[0]));
    }

    #[test]
    fn nearest_pillar_picks_the_closest() {
        assert_eq!(nearest_pillar((-39.0, -19.0)).0, 0);
        assert_eq!(nearest_pillar((41.0, 21.0)).0, 3);
        let (_, d) = nearest_pillar((-40.0, -14.0));
        assert!((d - 6.0).abs() < 1e-4, "expected 6.0, got {}", d);
    }

    #[test]
    fn side_of_is_signed_and_flips_across_the_axis() {
        let pillar = (0.0, 0.0);
        let target = (10.0, 0.0);
        let a = side_of(pillar, target, (0.0, 5.0));
        let b = side_of(pillar, target, (0.0, -5.0));
        assert_ne!(a, 0.0);
        assert_eq!(a, -b, "opposite sides must have opposite sign");
        // Collinear observer sits on the axis itself.
        assert_eq!(side_of(pillar, target, (5.0, 0.0)), 0.0);
    }
}

// ===========================================================================
// Nagrand / TeamPlan regression armor (post step-4 merge)
// ===========================================================================
//
// The design doc's step-1 item parked "the 16 failing PillaredArena probes"
// until the TeamPlan work settled. Archaeology (e673490): those probes were
// re-pointed at TwinPillars when PillaredArena became the Nagrand replica, and
// they still pass there — they stay as regression armor for the REACTIVE
// machinery, which is valid because `Legacy` has been byte-identical since
// their capture. What was actually missing is any fixed-seed armor for the
// behaviours that now exist ONLY on Nagrand under `TeamPlan`: the healer
// solve's cover play, the kiter healer-leash, and the flat-field anti-statue
// key. That is this module.
//
// Calibration data comes from the shipped measurement trail (the n=100
// head-to-head CSV in docs/design/balance/ and the pillar_self_block numbers
// above); thresholds sit at roughly half the measured healthy value so a real
// regression fails loudly while seed-level drift does not. `scan_nagrand_
// teamplan` re-prints the per-seed numbers when trajectories drift and pins
// need re-choosing.
mod nagrand_teamplan {
    use super::*;
    use arenasim::states::play_match::map_geometry::{
        has_line_of_sight, ObstacleVolume, EYE_HEIGHT,
    };
    use bevy::prelude::Vec2;

    /// Nagrand's four pillars, as LoS volumes, loaded from
    /// `assets/config/maps.ron` — the geometry the match plays on. (A hand-
    /// copied list here once lost the pillars' 22.5-degree turn, which put a
    /// vertex where the map has a face: a line grazing the pillar's top read
    /// as blocked that the match saw clear. AS-385.)
    fn volumes() -> Vec<ObstacleVolume> {
        arenasim::states::play_match::map_config::load_map_geometry_config()
            .expect("maps.ron loads")
            .active_for(arenasim::ArenaMap::PillaredArena)
            .volumes
    }

    fn eye(p: Vec2) -> Vec3 {
        Vec3::new(p.x, EYE_HEIGHT, p.y)
    }

    fn xz(v: Vec3) -> Vec2 {
        Vec2::new(v.x, v.z)
    }

    struct NagrandStats {
        /// Sim-seconds the enemy Warlock had NO line to the team-1 Priest —
        /// the cover the solve buys. Legacy measures ~0 here.
        occlusion_secs: f32,
        /// Share of paired frames with the Priest's heal line to the Warrior
        /// blocked (the step-3/4 pathology, now bounded from above).
        blocked_share: f32,
        paired_frames: usize,
    }

    fn measure_pillar(seed: u64) -> NagrandStats {
        let mut cfg = create_config(
            vec!["Warrior", "Priest"],
            vec!["Warlock", "Priest"],
            Some(seed),
        );
        cfg.map = "PillaredArena".to_string();
        cfg.ai_profile = Some("TeamPlan".to_string());
        cfg.max_duration_secs = 300.0;
        let (_result, timeline) = run_observed_collecting(cfg);
        let vols = volumes();
        let gate = timeline.gates_open_time.expect("gates never opened");
        let priest = timeline.find(1, CharacterClass::Priest, false);
        let warrior = timeline.find(1, CharacterClass::Warrior, false);
        let warlock = timeline.find(2, CharacterClass::Warlock, false);

        let by_time = |e| -> BTreeMap<u32, Vec3> {
            timeline
                .samples_from(e, gate)
                .into_iter()
                .map(|(t, p): (f32, Vec3)| (t.to_bits(), p))
                .collect()
        };
        let (wl, wr) = (by_time(warlock), by_time(warrior));

        let (mut occl, mut blocked, mut paired) = (0usize, 0usize, 0usize);
        for (t, ppos) in timeline.samples_from(priest, gate) {
            if let Some(wlpos) = wl.get(&t.to_bits()) {
                if !has_line_of_sight(&vols, eye(xz(*wlpos)), eye(xz(ppos))) {
                    occl += 1;
                }
            }
            if let Some(wrpos) = wr.get(&t.to_bits()) {
                paired += 1;
                if !has_line_of_sight(&vols, eye(xz(ppos)), eye(xz(*wrpos))) {
                    blocked += 1;
                }
            }
        }
        NagrandStats {
            occlusion_secs: occl as f32 / 60.0,
            blocked_share: blocked as f32 / paired.max(1) as f32,
            paired_frames: paired,
        }
    }

    /// THE COVER FLOOR. `Legacy` measures ~0 occlusion-seconds on Nagrand —
    /// buying cover at all is the entire point of the TeamPlan work (0.0s ->
    /// ~28s/match at n=100). Measured on these seeds: 22.3 / 22.1 / 24.0s.
    /// The floor is under half the weakest, so a mechanism regression (the
    /// silently unrouted, `OccupyCover` losing its occlusion constraint)
    /// fails loudly while trajectory drift does not.
    #[test]
    fn teamplan_healer_buys_occlusion_on_nagrand() {
        for seed in [7u64, 11, 12] {
            let s = measure_pillar(seed);
            assert!(s.paired_frames > 500, "seed {seed}: probe went vacuous");
            println!(
                "seed {seed:2}: occlusion {:.1}s | heal line blocked {:.0}%",
                s.occlusion_secs,
                100.0 * s.blocked_share
            );
            assert!(
                s.occlusion_secs >= 10.0,
                "seed {seed}: only {:.1}s of Warlock-denied sight — the solve's cover play \
                 has regressed (Legacy measures ~0 here; healthy is ~10-14s)",
                s.occlusion_secs
            );
        }
    }

    /// THE HEAL-LINE CEILING — the inverse of `pillar_self_block`'s pathology
    /// pins. The step-3 camp measured 40-55% of frames with the healer blind
    /// to its own Warrior. The probe measures what `C_SIGHT` decides: on frames
    /// the Priest is PRESSURED (the team solve's posture) and free to cast, the
    /// share whose movement goal — the spot the solve picked — has no line to
    /// the Warrior ([`solve_goal_blind_share`]). (The share of ALL frames with
    /// the line blocked stopped measuring the solve at AS-292, which moved
    /// frames the line does not matter on; see that card's note.)
    ///
    /// # A seed bank, not pinned seeds (AS-383)
    ///
    /// The probe pinned seeds 7 / 2 / 12 until AS-383 held back the
    /// `TeamPlan` Priest's scream dip, which had been ending in PRESSURED:
    /// seed 2 then had no PRESSURED frame at all. Migrated, not re-pinned
    /// (CLAUDE.md, *Test an AI decision*): it reads the PRESSURED frames over
    /// a seed bank, wherever they fall, against two ceilings, with `C_SIGHT`'s
    /// constraint removed as the mutant:
    ///
    /// ```text
    ///                  pooled over 1-24     seeds with >= 300 frames
    /// AS-383           19.2% of 4815        27 4 1 3 25 27%
    /// no C_SIGHT       44.7% of 5019        74 80 27 6 35 18%
    /// (before AS-383: AS-292 pooled 21.5%, main 13.6%, no C_SIGHT 78.6%)
    /// ```
    ///
    /// # 48 seeds, on maps.ron's geometry (AS-385)
    ///
    /// The rows above read sight through a hand-copied pillar list that had
    /// lost the pillars' 22.5-degree turn, which put a vertex where the map
    /// has a face: a goal on a line grazing a pillar top read as blocked
    /// (seed 7: 80%) that the match saw clear (2%). The probe now loads the
    /// map's own volumes, and its bank is seeds 1-48:
    ///
    /// ```text
    ///                        pooled           worst seed (>= 300 frames)
    /// main @ 43a8ee54        10.2% of 10617   15%
    /// AS-385 (yield+stance)  10.9% of 34172   28%
    /// AS-385, no C_SIGHT     49.8% of 32800   78%; 15 of 33 seeds over 50%
    /// AS-388 (dip retired)   20.7% of 13300   30%
    /// ```
    ///
    /// AS-388's rise is in the denominator, not the solve: retiring the
    /// `TeamPlan` scream dip removed the PRESSURED frames its path ended in
    /// (34172 to 13300), most of them sighted, while the blind frames fell
    /// (about 3720 to 2750).
    ///
    /// The pooled ceiling, 30%, sat about 19 points from both sides. A seed
    /// with enough frames to read on its own (300, five seconds) is held to
    /// 50%, which the mutant's worst seeds cross. The bank must hold 3000 such
    /// frames, or the probe has gone vacuous.
    #[test]
    fn teamplan_healer_keeps_its_heal_line_on_nagrand() {
        let (mut frames, mut blind) = (0usize, 0.0f32);
        for seed in HEAL_LINE_BANK {
            let (n, share) = solve_goal_blind_share(seed);
            println!(
                "seed {seed:2}: {n} pressured solve frames, goal blind {:.0}%",
                100.0 * share
            );
            frames += n;
            blind += share * n as f32;
            if n >= HEAL_LINE_SEED_FRAMES {
                assert!(
                    share <= HEAL_LINE_CEILING,
                    "seed {seed}: the PRESSURED solve's goal had no line to the Warrior on \
                     {:.0}% of {n} frames the Priest could heal — the solve has stopped \
                     asking for sight of its ally",
                    100.0 * share
                );
            }
        }
        assert_min_occurrences(
            "pressured solve frames over the bank",
            frames,
            HEAL_LINE_BANK_FRAMES,
        );
        let pooled = blind / frames as f32;
        println!("bank: {frames} frames, goal blind {:.1}%", 100.0 * pooled);
        assert!(
            pooled <= HEAL_LINE_POOLED_CEILING,
            "over the bank the PRESSURED solve's goal had no line to the Warrior on {:.0}% of \
             {frames} frames — the solve has stopped asking for sight of its ally",
            100.0 * pooled
        );
    }

    /// The heal-line probe's seed bank, its two ceilings (one seed's, the
    /// bank's), and its two frame floors (see
    /// [`teamplan_healer_keeps_its_heal_line_on_nagrand`]).
    const HEAL_LINE_BANK: std::ops::RangeInclusive<u64> = 1..=48;
    const HEAL_LINE_CEILING: f32 = 0.50;
    const HEAL_LINE_POOLED_CEILING: f32 = 0.30;
    const HEAL_LINE_SEED_FRAMES: usize = 300;
    const HEAL_LINE_BANK_FRAMES: usize = 3000;

    /// The heal-line probe's measurement at one seed (the Nagrand camp,
    /// `TeamPlan`): over the frames the team-1 Priest is PRESSURED — read off
    /// its `movement_decision` posture — with a `Point` goal, both it and its
    /// Warrior alive, and no hard CC, silence or school lockout on it (the
    /// solve asks for sight only while the healer can cast,
    /// `team_solve::can_cast_heal`), the count of such frames and the share
    /// whose goal point has no line of sight to the Warrior.
    fn solve_goal_blind_share(seed: u64) -> (usize, f32) {
        use arenasim::headless::runner::TraceConfig;
        use arenasim::states::play_match::components::AuraType;

        let mut cfg = create_config(
            vec!["Warrior", "Priest"],
            vec!["Warlock", "Priest"],
            Some(seed),
        );
        cfg.map = "PillaredArena".to_string();
        cfg.ai_profile = Some("TeamPlan".to_string());
        cfg.max_duration_secs = 300.0;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        drop(tmp);

        // (combat time, goal, Warrior position) on each frame the Priest could heal.
        let mut frames: Vec<(f32, Vec3, Vec3)> = Vec::new();
        let mut gate: Option<f32> = None;
        let mut priest_slot = None;
        run_headless_match_observed(
            cfg,
            true,
            Some(TraceConfig {
                output_path: path.clone(),
            }),
            |f| {
                if !f.gates_open {
                    return;
                }
                let gate = *gate.get_or_insert(f.sim_time);
                let find = |class| {
                    f.combatants
                        .values()
                        .find(|c| c.team == 1 && c.class == class && !c.is_pet)
                };
                let (Some(p), Some(w)) =
                    (find(CharacterClass::Priest), find(CharacterClass::Warrior))
                else {
                    return;
                };
                priest_slot = Some(p.slot);
                let held = p.aura_types.iter().any(|a| {
                    matches!(
                        a,
                        AuraType::Stun
                            | AuraType::Fear
                            | AuraType::Polymorph
                            | AuraType::Incapacitate
                            | AuraType::Silence
                            | AuraType::SpellSchoolLockout
                    )
                });
                if p.alive && w.alive && !held {
                    if let Some(goal) = p.point_goal {
                        frames.push((f.sim_time - gate, goal, w.position));
                    }
                }
            },
        )
        .expect("observed traced headless match failed");

        let slot = priest_slot.expect("no team-1 Priest");
        let postures: Vec<(f32, String)> = std::fs::read_to_string(&path)
            .expect("read trace file")
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter(|v| {
                v["kind"] == "movement_decision"
                    && v["actor"]["team"].as_u64() == Some(1)
                    && v["actor"]["slot"].as_u64() == Some(slot as u64)
            })
            .map(|v| {
                (
                    v["sim_time"].as_f64().unwrap() as f32,
                    v["posture"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        let _ = std::fs::remove_file(path);

        let vols = volumes();
        let (mut pressured, mut blind) = (0usize, 0usize);
        let mut next = 0usize;
        let mut posture = "";
        for (t, goal, warrior) in frames {
            while next < postures.len() && postures[next].0 <= t {
                posture = &postures[next].1;
                next += 1;
            }
            if posture != "pressured" {
                continue;
            }
            pressured += 1;
            if !has_line_of_sight(&vols, eye(xz(goal)), eye(xz(warrior))) {
                blind += 1;
            }
        }
        (pressured, blind as f32 / pressured.max(1) as f32)
    }

    /// THE KITER LEASH. A `TeamPlan` Hunter must stay healable: `flee` is
    /// unbounded distance-maximisation, and before the leash nothing stopped
    /// it fleeing a Rogue clean out of its own Priest's 40yd cast range —
    /// which is how the enemy deletes an unhealable kiter.
    ///
    /// Asserted as an AGGREGATE over three seeds, not per seed: the leash is a
    /// soft weight (4.0, deliberately below `flee`'s 6.0 — escaping still
    /// wins), so any single seed can spend real time out of reach. Calibrated
    /// on seeds {2, 3, 10}: with the leash the total is 18.2s; without it
    /// (Legacy) 119.2s. The 60s bound sits between the distributions with
    /// a >3x margin either way. `scan_nagrand_teamplan` re-prints both
    /// distributions when pins need re-choosing.
    #[test]
    fn teamplan_hunter_stays_within_its_priests_reach() {
        let mut total_secs = 0.0f32;
        for seed in [2u64, 3, 10] {
            let mut cfg = create_config(
                vec!["Hunter", "Priest"],
                vec!["Rogue", "Priest"],
                Some(seed),
            );
            cfg.map = "PillaredArena".to_string();
            cfg.ai_profile = Some("TeamPlan".to_string());
            cfg.max_duration_secs = 300.0;
            let (_result, timeline) = run_observed_collecting(cfg);
            let gate = timeline.gates_open_time.expect("gates never opened");
            let hunter = timeline.find(1, CharacterClass::Hunter, false);
            let priest = timeline.find(1, CharacterClass::Priest, false);
            let pri: BTreeMap<u32, Vec3> = timeline
                .samples_from(priest, gate)
                .into_iter()
                .map(|(t, p)| (t.to_bits(), p))
                .collect();
            let (mut out_of_reach, mut paired) = (0usize, 0usize);
            for (t, hpos) in timeline.samples_from(hunter, gate) {
                if let Some(ppos) = pri.get(&t.to_bits()) {
                    paired += 1;
                    if xz(hpos).distance(xz(*ppos)) > 40.0 {
                        out_of_reach += 1;
                    }
                }
            }
            assert!(paired > 500, "seed {seed}: probe went vacuous");
            let secs = out_of_reach as f32 / 60.0;
            println!("seed {seed:2}: {secs:.1}s beyond heal range");
            total_secs += secs;
        }
        println!("total: {total_secs:.1}s (leashed ~18s; unleashed ~119s)");
        assert!(
            total_secs <= 60.0,
            "Hunter spent {total_secs:.1}s beyond its Priest's 40yd reach across the \
             pinned seeds — the healer leash has regressed (it is TeamPlan-gated in \
             build_kiter_inputs; leashed baseline ~18s, unleashed ~119s)",
        );
    }

    /// THE ANTI-STATUE KEY. On an obstacle-free map with melee-only pressure,
    /// every solve candidate ties on every constraint; before the flat-field
    /// danger key, a PRESSURED TeamPlan healer froze in place where `Legacy`
    /// kites. Asserts the healer keeps moving while an enemy is on top of it —
    /// the U6 statue band idiom (statue ~0.65 u/s of pressured time; healthy
    /// movement is well above 1.5).
    ///
    /// # Why twelve seeds and two thresholds (AS-122)
    ///
    /// This sampled seeds {1, 4, 7} and required every one to clear 1.0 u/s.
    /// That threshold was never a property of the seed space — it was fitted to
    /// the three seeds it sampled. Measuring all twelve on the UNMODIFIED
    /// binary puts seed 6 at 0.78 u/s, so the old assertion would have failed on
    /// a seed it simply never looked at:
    ///
    /// ```text
    /// empty off hand  1.26 1.73 1.40 2.24 1.86 0.78 1.94 1.65 1.73 1.41 2.88 3.34
    /// armed off hand  0.90 1.49 1.23 1.17 1.80 1.57 1.57 2.51 1.44 2.29 2.61 3.57
    /// ```
    ///
    /// AS-122 armed the Rogue's off hand, which reshuffled which seed is worst
    /// (1 replaces 6) without lowering the floor — the minimum actually rose,
    /// 0.78 -> 0.90, while the median eased 1.73 -> 1.57. The healer is no more
    /// of a statue than it was; seed 1 crossed a line drawn in the wrong place.
    ///
    /// So the pin is now distributional and scans the whole set: a per-seed
    /// FLOOR that a real statue cannot clear, plus a MEDIAN over all twelve.
    /// What they divide between them is narrow, and worth stating exactly
    /// rather than generously:
    ///
    /// - the FLOOR catches any single seed dropping into the band;
    /// - the MEDIAN catches the distribution's CENTRE shifting while every
    ///   individual seed stays above the floor.
    ///
    /// The median is not a broad-sag guard, and it would be wrong to read it as
    /// one. Degrade every seed uniformly by a factor k and the floor binds
    /// first (k < 0.833, against the median's k < 0.764) — and because the
    /// per-seed `assert!` panics inside the loop, the median is never even
    /// evaluated on a run where a seed fails. A broad sag hits the floor.
    ///
    /// # How much room these have (AS-122)
    ///
    /// The median is the sturdier of the two: it sits 0.37 below the shipped
    /// value and moved only 0.16 under a change that moved individual seeds by
    /// as much as 1.07 (seed 4, 2.24 -> 1.17).
    ///
    /// The floor is BETTER-FITTED, NOT UNFITTED, and its headroom is thin:
    /// 0.75 against a shipped minimum of 0.90 is 0.15, and the mutation that
    /// proves it live trips it by 0.01. Concretely, **any change that shuffles
    /// the worst seed by about 17% turns this red** — milder than the change
    /// that prompted the re-pin. That is deliberate: widening it would buy
    /// robustness by accepting a threshold a real statue could clear. Written
    /// down so the next person to move it knows the room they have instead of
    /// rediscovering it when a green probe goes red.
    ///
    /// # Re-pinned for AS-167 (7 -> 13)
    ///
    /// Weapon speed from item data took seed 7 vacuous: 29 pressured frames
    /// against the floor of 120 (the Rogue barely reaches the healer). The
    /// first line that differs from `main` @ 17cb9f0 is the two Priests'
    /// opening Wand Shots into each other's shields, 10 -> 13 and 21 -> 26
    /// absorbed: the Staff of Dominance now fires every 1.6s for the same DPS.
    /// Seed 7 is replaced by the next seed, 13, from `scan_statue_seeds`:
    /// 1724 pressured frames at 2.97 u/s. With 13 in the set the minimum is
    /// 1.23 u/s (seed 4) and the median 2.12. The floor and the median
    /// thresholds are unchanged.
    ///
    /// # Migrated to a situation over a seed bank (AS-388)
    ///
    /// AS-388 retired the `TeamPlan` Priest's scream dip, which moved the RNG
    /// stream, and seed 10 read 0.74 u/s against the 0.75 floor. Its trace
    /// showed the pin had drifted, not the key: the Priest was FREE, healing
    /// beside a Rogue that was on its Warrior, and the probe counted any frame
    /// with the Rogue inside 12yd. Migrated, not re-pinned (CLAUDE.md, *Test
    /// an AI decision*): the probe now reads only the frames the key governs —
    /// the Priest PRESSURED (the posture the team solve runs in), free to move
    /// (no stun, fear, polymorph, incapacitate or root on it: a held Priest is
    /// not a statue), with the Rogue inside 12yd — wherever they fall over
    /// seeds 1-24, and asserts on each bar below. Measured, with two mutants:
    /// the key disarmed (`field_is_flat = false`), and the solve's answer held
    /// to where the healer stands (`solve_unit` returning `ctx.unit.pos`):
    ///
    /// ```text
    ///                   pooled     counted   lowest seed with >= 5s counted
    /// main @ 43a8ee54   2.67 u/s    94.0s    1.83 (6 such seeds)
    /// AS-388            2.34 u/s   111.2s    1.17 (11 such seeds)
    /// key disarmed      0.06 u/s   112.9s    0.00 (10 of 11 below 0.75)
    /// solve held        0.00 u/s   118.1s    0.00 (12 of 12 below 0.75)
    /// ```
    ///
    /// # The per-seed bar reads the directive, not the speed (AS-396)
    ///
    /// A speed floor asks the key's question at one remove: it is a property
    /// of the walk the executor made of the goal, so a change anywhere in
    /// that chain moves it, and AS-292's round 2 saw it stop failing the
    /// disarmed key (1.24-2.30 u/s) before AS-388's migration put it back.
    /// The per-seed bar now asks the key itself: on a counted frame, does the
    /// healer's `Point` directive aim at least 0.5yd from where it stands? A
    /// solve answering "here" is a statue whatever the healer's speed. It
    /// counts casting frames too — the frame observation does not carry the
    /// cast — and a casting healer keeps the directive it had before the cast,
    /// which for a statue is its own spot: those frames can only lower a
    /// healthy seed's share, which is why the bar sits well under it.
    ///
    /// ```text
    ///                   aim 0.5yd+ off   lowest seed with >= 5s counted
    /// AS-396            70% pooled       42% (seed 1; 11 such seeds)
    /// key disarmed       2%              14% (every seed at or under it)
    /// solve held         0%               0%
    /// ```
    ///
    /// Four bars, each failed by both mutants:
    ///
    /// - a seed with five seconds counted has its directive aim 0.5yd or more
    ///   off the healer on a quarter of them;
    /// - pooled over the bank, on three tenths;
    /// - the pooled rate is held to 1.2 u/s, the old median guard: a centre
    ///   below which the healer as a whole is sagging toward the band while
    ///   every seed still clears its floor (the branch sits at about half
    ///   again of it, main at more than twice);
    /// - the bank must hold a minute of counted pressure, or it has gone
    ///   vacuous.
    #[test]
    fn teamplan_healer_is_not_a_statue_on_basicarena() {
        let bank: Vec<(u64, usize, f32, usize)> = STATUE_SEEDS
            .map(|seed| {
                let (frames, path, aiming) = pressured_movement(seed);
                (seed, frames, path, aiming)
            })
            .collect();
        let (mut total_frames, mut total_path, mut total_aiming) = (0usize, 0.0f32, 0usize);
        for &(seed, frames, path, aiming) in &bank {
            total_frames += frames;
            total_path += path;
            total_aiming += aiming;
            if frames == 0 {
                println!("seed {seed:2}: no pressure");
                continue;
            }
            let secs = frames as f32 / 60.0;
            println!(
                "seed {seed:2}: {path:.1}u over {secs:.1}s of melee pressure = {:.2} u/s; \
                 directive aims {STATUE_AIM_YARDS}yd+ off on {:.0}% of it",
                path / secs,
                100.0 * aiming as f32 / frames as f32
            );
        }
        let secs = total_frames as f32 / 60.0;
        let pooled = total_path / secs;
        let aiming = total_aiming as f32 / total_frames.max(1) as f32;
        println!(
            "bank: {pooled:.2} u/s over {secs:.1}s of melee pressure; directive aims \
             {STATUE_AIM_YARDS}yd+ off on {:.0}% of it",
            100.0 * aiming
        );
        assert_min_occurrences(
            "pressured frames with a Rogue in 12yd over the bank",
            total_frames,
            STATUE_BANK_FRAMES,
        );
        for &(seed, frames, _, aiming) in &bank {
            if frames >= STATUE_SEED_FRAMES {
                let share = aiming as f32 / frames as f32;
                assert!(
                    share >= STATUE_SEED_AIM_FLOOR,
                    "seed {seed}: the directive aimed {STATUE_AIM_YARDS}yd+ off the PRESSURED \
                     healer on only {:.0}% of the {frames} frames a Rogue was on it — the \
                     solve is answering where it stands; the flat-field anti-statue key has \
                     regressed (main's lowest seed 42%, both statue mutants at most 14%)",
                    100.0 * share
                );
            }
        }
        assert!(
            aiming >= STATUE_POOLED_AIM_FLOOR,
            "the directive aimed {STATUE_AIM_YARDS}yd+ off the PRESSURED healer on only \
             {:.0}% of the bank's counted frames (measured 70%; key disarmed 2%, solve held 0%)",
            100.0 * aiming
        );
        assert!(
            pooled >= STATUE_POOLED_FLOOR,
            "pooled pressured movement {pooled:.2} u/s over the bank — the healer as a \
             whole is sagging toward the statue band (measured 2.34, main 2.67; with the \
             key disarmed 0.06)",
        );
    }

    /// The anti-statue probe's seed bank, floors and vacuity bound (see
    /// [`teamplan_healer_is_not_a_statue_on_basicarena`]).
    const STATUE_SEEDS: std::ops::RangeInclusive<u64> = 1..=24;
    const STATUE_POOLED_FLOOR: f32 = 1.2;
    const STATUE_SEED_AIM_FLOOR: f32 = 0.25;
    const STATUE_POOLED_AIM_FLOOR: f32 = 0.3;
    const STATUE_SEED_FRAMES: usize = 300;
    const STATUE_BANK_FRAMES: usize = 3600;
    const STATUE_AIM_YARDS: f32 = 0.5;

    /// The anti-statue probe's measurement at one seed: the frames after the
    /// gates on which the TeamPlan Priest was PRESSURED, free to move (no
    /// stun, fear, polymorph, incapacitate or root on it) and had the enemy
    /// Rogue within the 12yd danger radius, and the path it covered over them.
    fn pressured_movement(seed: u64) -> (usize, f32, usize) {
        use arenasim::headless::runner::TraceConfig;
        use arenasim::states::play_match::components::AuraType;

        let mut cfg = create_config(
            vec!["Warrior", "Priest"],
            vec!["Rogue", "Priest"],
            Some(seed),
        );
        cfg.map = "BasicArena".to_string();
        cfg.ai_profile = Some("TeamPlan".to_string());
        cfg.max_duration_secs = 300.0;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        drop(tmp);

        // (combat time, Priest position, Rogue within 12yd, Priest held, how
        // far off its directive's `Point` goal aims) per frame after the
        // gates, the Priest alive.
        let mut frames: Vec<(f32, Vec3, bool, bool, f32)> = Vec::new();
        let mut gate: Option<f32> = None;
        run_headless_match_observed(
            cfg,
            true,
            Some(TraceConfig {
                output_path: path.clone(),
            }),
            |f| {
                if !f.gates_open {
                    return;
                }
                let gate = *gate.get_or_insert(f.sim_time);
                let find = |team, class| {
                    f.combatants
                        .values()
                        .find(|c| c.team == team && c.class == class && !c.is_pet)
                };
                let Some(p) = find(1, CharacterClass::Priest).filter(|p| p.alive) else {
                    return;
                };
                let near = find(2, CharacterClass::Rogue)
                    .is_some_and(|r| r.alive && xz(r.position).distance(xz(p.position)) <= 12.0);
                let held = p.aura_types.iter().any(|a| {
                    matches!(
                        a,
                        AuraType::Stun
                            | AuraType::Fear
                            | AuraType::Polymorph
                            | AuraType::Incapacitate
                            | AuraType::Root
                    )
                });
                let aim = p.point_goal.map_or(0.0, |g| xz(g).distance(xz(p.position)));
                frames.push((f.sim_time - gate, p.position, near, held, aim));
            },
        )
        .expect("observed traced headless match failed");

        // The team-1 Priest's posture transitions, on the combat clock.
        let transitions: Vec<(f32, String)> = std::fs::read_to_string(&path)
            .expect("read trace file")
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter(|v| {
                v["kind"] == "movement_decision"
                    && v["actor"]["team"] == 1
                    && v["actor"]["class"] == "Priest"
                    && !v["previous_posture"].is_null()
            })
            .map(|v| {
                (
                    v["sim_time"].as_f64().unwrap() as f32,
                    v["posture"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        let _ = std::fs::remove_file(path);

        // Path length accumulated only while the Priest is PRESSURED — the
        // posture the team solve, and its anti-statue key, run in — free to
        // move, with the Rogue within the 12yd danger radius of it. (Until
        // AS-388 this read the Rogue's distance alone, which also counted a
        // FREE Priest healing beside a Rogue that was on its Warrior, and a
        // Priest the Rogue held in a stun: frames the key has no say over.)
        let mut pressured_path = 0.0f32;
        let mut pressured_frames = 0usize;
        let mut aiming_frames = 0usize;
        let mut prev: Option<Vec3> = None;
        let (mut next, mut posture) = (0usize, "free");
        for (t, ppos, near, held, aim) in frames {
            while next < transitions.len() && transitions[next].0 <= t {
                posture = &transitions[next].1;
                next += 1;
            }
            if posture == "pressured" && near && !held {
                pressured_frames += 1;
                if aim >= STATUE_AIM_YARDS {
                    aiming_frames += 1;
                }
                if let Some(prev) = prev {
                    pressured_path += xz(prev).distance(xz(ppos));
                }
            }
            prev = Some(ppos);
        }
        (pressured_frames, pressured_path, aiming_frames)
    }

    /// Exploratory scan for [`STATUE_SEEDS`]: pressured frames and movement
    /// rate per seed, flagging seeds under the probe's 120-frame vacuity
    /// floor. Ignored by default.
    #[test]
    #[ignore]
    fn scan_statue_seeds() {
        for seed in 1u64..=20 {
            let (frames, path, _) = pressured_movement(seed);
            let secs = frames as f32 / 60.0;
            eprintln!(
                "statue seed {seed:2}: pressured_frames={frames:4} rate={:5.2} u/s{}",
                if frames > 0 { path / secs } else { 0.0 },
                if frames < 120 { " <-- VACUOUS" } else { "" }
            );
        }
    }

    /// Exploratory seed scan — re-prints the per-seed numbers behind every pin
    /// above so thresholds can be re-chosen when trajectories drift. Ignored
    /// by default.
    #[test]
    #[ignore]
    fn scan_nagrand_teamplan() {
        for seed in 1u64..=12 {
            let s = measure_pillar(seed);
            let (solve_frames, goal_blind) = solve_goal_blind_share(seed);
            println!(
                "pillar seed {seed:2}: occl {:.1}s | blocked {:.0}% | frames {} | solve goal \
                 blind {:.0}% of {solve_frames}",
                s.occlusion_secs,
                100.0 * s.blocked_share,
                s.paired_frames,
                100.0 * goal_blind,
            );
        }
        for profile in ["TeamPlan", "Legacy"] {
            for seed in 1u64..=12 {
                let mut cfg = create_config(
                    vec!["Hunter", "Priest"],
                    vec!["Rogue", "Priest"],
                    Some(seed),
                );
                cfg.map = "PillaredArena".to_string();
                cfg.ai_profile = Some(profile.to_string());
                cfg.max_duration_secs = 300.0;
                let (_result, timeline) = run_observed_collecting(cfg);
                let gate = timeline.gates_open_time.expect("gates never opened");
                let hunter = timeline.find(1, CharacterClass::Hunter, false);
                let priest = timeline.find(1, CharacterClass::Priest, false);
                let pri: BTreeMap<u32, Vec3> = timeline
                    .samples_from(priest, gate)
                    .into_iter()
                    .map(|(t, p)| (t.to_bits(), p))
                    .collect();
                let mut out = 0usize;
                for (t, hpos) in timeline.samples_from(hunter, gate) {
                    if let Some(ppos) = pri.get(&t.to_bits()) {
                        if xz(hpos).distance(xz(*ppos)) > 40.0 {
                            out += 1;
                        }
                    }
                }
                println!(
                    "hunter {profile:8} seed {seed:2}: {:.1}s beyond heal range",
                    out as f32 / 60.0
                );
            }
        }
    }
}
