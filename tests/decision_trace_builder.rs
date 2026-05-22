//! Unit tests for the AI decision trace builder API (U1).
//!
//! These tests exercise the builder/writer in isolation — no Bevy world, no
//! actual class AI. They prove the schema, the emission gate, and the
//! deterministic ordering before per-class instrumentation lands in U2-U9.

use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::decision_trace::{
    ActorView, CandidateStatus, DecisionTrace, NoActionReason, RejectionReason, ResourceKind, TraceWriter,
};

fn warrior_actor() -> ActorView {
    ActorView {
        entity_id: 7,
        team: 1,
        slot: 0,
        class: CharacterClass::Warrior,
        hp_pct: 1.0,
        mana_pct: 0.5,
        position: [0.0, 0.0, 0.0],
    }
}

fn assert_event_count(trace: &DecisionTrace, expected: usize) {
    assert_eq!(
        trace.pending_events.len(),
        expected,
        "expected {} pending events, got {}",
        expected,
        trace.pending_events.len()
    );
}

#[test]
fn builder_happy_path_records_three_rejects_and_one_choose() {
    let mut trace = DecisionTrace::default();

    let mut builder = trace.start_ability_decision(warrior_actor(), None);
    builder.reject(
        AbilityType::Charge,
        RejectionReason::OnCooldown { remaining: 4.2 },
    );
    builder.reject(
        AbilityType::Rend,
        RejectionReason::AlreadyApplied,
    );
    builder.reject(
        AbilityType::MortalStrike,
        RejectionReason::OutOfRange {
            distance: 18.0,
            max: 5.0,
        },
    );
    builder.choose(AbilityType::HeroicStrike, None, true);
    builder.finish();

    assert_event_count(&trace, 1);
    let json = serde_json::to_string(&trace.pending_events[0]).unwrap();
    assert!(json.contains("\"ability\":\"Charge\""), "Charge listed: {}", json);
    assert!(json.contains("\"OnCooldown\""), "OnCooldown reason emitted: {}", json);
    assert!(json.contains("\"HeroicStrike\""), "HeroicStrike chosen: {}", json);
    assert!(json.contains("\"action_taken\""), "outcome action_taken: {}", json);
}

#[test]
fn builder_emission_gate_drops_event_with_no_candidates() {
    let mut trace = DecisionTrace::default();

    // Start a decision but push no candidates — caller short-circuited before
    // evaluating any predicate. Finish should be a no-op (emission gate).
    let builder = trace.start_ability_decision(warrior_actor(), None);
    builder.finish();

    assert_event_count(&trace, 0);
}

#[test]
fn builder_rejection_with_structured_payload_serializes_with_numbers() {
    let mut trace = DecisionTrace::default();

    let mut builder = trace.start_ability_decision(warrior_actor(), None);
    builder.reject(
        AbilityType::Frostbolt,
        RejectionReason::OutOfRange {
            distance: 35.0,
            max: 12.0,
        },
    );
    builder.finish_no_action(NoActionReason::AllCandidatesRejected);

    assert_event_count(&trace, 1);
    let json = serde_json::to_string(&trace.pending_events[0]).unwrap();
    assert!(
        json.contains("\"OutOfRange\":{\"distance\":35.0,\"max\":12.0}"),
        "structured payload preserved: {}",
        json
    );
    assert!(json.contains("\"no_action\""), "no_action outcome: {}", json);
}

#[test]
fn builder_resource_variant_distinguishes_rage_from_mana() {
    let mut trace = DecisionTrace::default();

    let mut builder = trace.start_ability_decision(warrior_actor(), None);
    builder.reject(
        AbilityType::HeroicStrike,
        RejectionReason::InsufficientResource {
            resource: ResourceKind::Rage,
            have: 10.0,
            need: 65.0,
        },
    );
    builder.finish_no_action(NoActionReason::AllCandidatesRejected);

    let json = serde_json::to_string(&trace.pending_events[0]).unwrap();
    assert!(
        json.contains("\"resource\":\"Rage\""),
        "Rage discriminator preserved: {}",
        json
    );
}

#[test]
fn writer_no_op_when_writer_is_none() {
    // DecisionTrace::default() leaves writer = None. Builder calls should
    // still succeed but flush_events is never invoked (the flush system would
    // skip).
    let mut trace = DecisionTrace::default();
    assert!(trace.writer.is_none());

    let mut builder = trace.start_ability_decision(warrior_actor(), None);
    builder.reject(AbilityType::Rend, RejectionReason::AlreadyApplied);
    builder.finish_no_action(NoActionReason::AllCandidatesRejected);

    // Events are pending — they'd be drained by flush_decision_trace_system
    // and discarded since writer is None.
    assert_event_count(&trace, 1);
}

#[test]
fn writer_sorts_events_by_frame_then_entity_then_kind() {
    use arenasim::states::play_match::decision_trace::EventKind;

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    let mut trace = DecisionTrace::default();
    trace.install_writer(TraceWriter::create(path.clone(), false).unwrap());

    // Push events out of canonical order. Canonical = (frame, entity_id, kind).
    // Use distinct frame numbers to make the sort visible.
    let mut later = warrior_actor();
    later.entity_id = 9;

    trace.current_frame = 50;
    let mut b = trace.start_ability_decision(later.clone(), None);
    b.choose(AbilityType::HeroicStrike, None, true);
    b.finish();

    trace.current_frame = 10;
    let mut b = trace.start_ability_decision(warrior_actor(), None);
    b.choose(AbilityType::HeroicStrike, None, true);
    b.finish();

    trace.current_frame = 10;
    let mut b = trace.start_ability_decision(warrior_actor(), None);
    b.choose(AbilityType::Rend, Some(bevy::prelude::Entity::from_raw(4)), true);
    b.finish();

    // Drain and write.
    let events = std::mem::take(&mut trace.pending_events);
    let writer = trace.writer.as_mut().expect("writer attached");
    writer.flush_events(events).unwrap();
    let _ = EventKind::AbilityDecision; // touch import
    drop(trace); // flush via Drop

    let body = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 3, "wrote 3 lines, got {}", lines.len());

    // After sort, the order should be: frame 10/entity 7 events first, then frame 50/entity 9.
    let frames: Vec<u64> = lines
        .iter()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            v.get("frame").and_then(|f| f.as_u64()).unwrap()
        })
        .collect();
    assert_eq!(frames, vec![10, 10, 50], "frames sorted: {:?}", frames);
}

#[test]
fn builder_finish_no_action_emits_even_with_zero_candidates() {
    // Hunter/Rogue top-level TargetImmune short-circuits call finish_no_action
    // immediately after start_ability_decision without pushing any candidates.
    // The original implementation gated this case on candidates.is_empty() and
    // silently dropped the event — defeating the diagnostic use case. Verify
    // finish_no_action always emits.
    let mut trace = DecisionTrace::default();
    let builder = trace.start_ability_decision(warrior_actor(), None);
    builder.finish_no_action(NoActionReason::TargetImmune);
    assert_event_count(&trace, 1);
    let json = serde_json::to_string(&trace.pending_events[0]).unwrap();
    assert!(
        json.contains("\"no_action\""),
        "no_action outcome present: {}",
        json
    );
    assert!(
        json.contains("\"TargetImmune\""),
        "TargetImmune reason present: {}",
        json
    );
}

#[test]
fn start_pet_decision_event_carries_owner_and_pet_type() {
    use bevy::prelude::Entity;
    let mut trace = DecisionTrace::default();
    let owner = Entity::from_raw(42);
    let actor = ActorView {
        entity_id: 100,
        team: 2,
        slot: 0,
        class: CharacterClass::Hunter,
        hp_pct: 1.0,
        mana_pct: 1.0,
        position: [0.0, 0.0, 0.0],
    };
    let mut builder = trace.start_pet_decision(actor, None, owner, "Spider");
    builder.reject(AbilityType::SpiderWeb, RejectionReason::AlreadyApplied);
    builder.finish();
    assert_event_count(&trace, 1);
    let json = serde_json::to_string(&trace.pending_events[0]).unwrap();
    assert!(json.contains("\"kind\":\"pet_decision\""), "kind=pet_decision: {}", json);
    assert!(json.contains("\"owner\":42"), "owner=42: {}", json);
    assert!(json.contains("\"pet_type\":\"Spider\""), "pet_type=Spider: {}", json);
}

#[test]
fn writer_sorts_target_acquisition_before_ability_decision_at_same_frame_and_entity() {
    // Canonical order is (frame, entity_id, kind). For two events at the same
    // frame and entity_id, kind tie-break should put TargetAcquisition (0)
    // before AbilityDecision (1). The earlier test only covered AbilityDecision
    // → AbilityDecision which never reaches the kind comparator.
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    let mut trace = DecisionTrace::default();
    trace.install_writer(TraceWriter::create(path.clone(), false).unwrap());
    trace.current_frame = 10;

    // Push ability_decision first
    let mut b = trace.start_ability_decision(warrior_actor(), None);
    b.choose(AbilityType::HeroicStrike, None, true);
    b.finish();

    // Push target_acquisition second (same frame, same entity)
    let mut t = trace.start_target_acquisition(warrior_actor(), None, None);
    t.score(
        bevy::prelude::Entity::from_raw(99),
        CharacterClass::Mage,
        -10,
        CandidateStatus::Chosen,
        None,
    );
    t.finish(Some(bevy::prelude::Entity::from_raw(99)), None);

    let events = std::mem::take(&mut trace.pending_events);
    let writer = trace.writer.as_mut().unwrap();
    writer.flush_events(events).unwrap();
    drop(trace);

    let body = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 2);
    let kinds: Vec<String> = lines
        .iter()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            v.get("kind").and_then(|k| k.as_str()).unwrap().to_string()
        })
        .collect();
    assert_eq!(
        kinds,
        vec!["target_acquisition".to_string(), "ability_decision".to_string()],
        "TargetAcquisition (kind_order=0) sorted before AbilityDecision (kind_order=1): {:?}",
        kinds
    );
}

#[test]
fn writer_creates_parent_directory_on_create() {
    let temp = tempfile::tempdir().unwrap();
    let nested = temp.path().join("traces").join("subdir").join("trace.jsonl");
    assert!(!nested.parent().unwrap().exists());

    let writer = TraceWriter::create(nested.clone(), false).unwrap();
    assert!(nested.parent().unwrap().exists());
    drop(writer);
}

#[test]
fn close_writer_drains_pending_and_resets_clock() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    let mut trace = DecisionTrace::default();
    trace.install_writer(TraceWriter::create(path.clone(), false).unwrap());
    trace.current_frame = 100;
    trace.current_sim_time = 42.5;

    let mut b = trace.start_ability_decision(warrior_actor(), None);
    b.choose(AbilityType::HeroicStrike, None, true);
    b.finish();

    assert_eq!(trace.pending_events.len(), 1);
    trace.close_writer().expect("flush on close");

    assert!(trace.writer.is_none(), "writer detached");
    assert_eq!(trace.current_frame, 0, "frame reset");
    assert_eq!(trace.current_sim_time, 0.0, "sim_time reset");
    assert!(trace.pending_events.is_empty(), "pending drained");

    let body = std::fs::read_to_string(&path).unwrap();
    let line_count = body.lines().count();
    assert_eq!(line_count, 1, "event flushed to disk: {} lines", line_count);
}
