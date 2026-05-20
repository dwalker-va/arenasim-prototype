//! JSONL Trace Writer
//!
//! Buffered writer that serializes `DecisionEvent`s to a JSONL file. Canonicalizes
//! event order by `(frame, actor.entity_id, kind)` immediately before flush so
//! trace files are byte-identical across runs at the same seed even if intermediate
//! query iteration order varies.
//!
//! The writer is opened by the headless runner at match start (when trace mode is
//! enabled) and flushed by `flush_decision_trace_system` each frame; a `Drop` impl
//! provides defense-in-depth in case explicit flush is missed.

use bevy::prelude::*;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use super::events::{DecisionEvent, EventKind};
use super::DecisionTrace;

/// Wraps a `BufWriter<File>` and owns the output path. Dropping the writer
/// flushes the buffer.
pub struct TraceWriter {
    inner: BufWriter<File>,
    path: PathBuf,
    /// True when verbose mode is enabled — extra state may be attached to
    /// events before serialization. Plumbed but not yet consumed in U1.
    pub verbose: bool,
}

impl TraceWriter {
    pub fn create(path: PathBuf, verbose: bool) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = File::create(&path)?;
        Ok(Self {
            inner: BufWriter::new(file),
            path,
            verbose,
        })
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Sort events by `(frame, actor.entity_id, kind)` and write each as a JSONL
    /// line. Returns the number of events written.
    pub fn flush_events(&mut self, mut events: Vec<DecisionEvent>) -> std::io::Result<usize> {
        events.sort_by(|a, b| {
            (a.frame, a.actor.entity_id, kind_order(a.kind)).cmp(&(
                b.frame,
                b.actor.entity_id,
                kind_order(b.kind),
            ))
        });
        let count = events.len();
        for event in events {
            serde_json::to_writer(&mut self.inner, &event)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            self.inner.write_all(b"\n")?;
        }
        self.inner.flush()?;
        Ok(count)
    }
}

impl Drop for TraceWriter {
    fn drop(&mut self) {
        // Best-effort flush on drop. Errors are intentionally swallowed —
        // the explicit flush via `flush_decision_trace_system` is the primary
        // success path; Drop is just a safety net.
        let _ = self.inner.flush();
    }
}

fn kind_order(kind: EventKind) -> u8 {
    match kind {
        EventKind::TargetAcquisition => 0,
        EventKind::AbilityDecision => 1,
        EventKind::PetDecision => 2,
    }
}

/// System that updates `DecisionTrace.current_frame` and `current_sim_time` each
/// frame, then drains any pending events into the writer (if a writer is
/// attached). Registered in both `add_core_combat_systems` (headless) and
/// `StatesPlugin::build` (graphical) per the dual-registration discipline.
pub fn flush_decision_trace_system(
    time: Res<Time>,
    countdown: Res<crate::states::play_match::components::MatchCountdown>,
    mut trace: ResMut<DecisionTrace>,
) {
    trace.current_frame = trace.current_frame.wrapping_add(1);
    if countdown.gates_opened {
        trace.current_sim_time += time.delta_secs();
    }
    if trace.writer.is_none() || trace.pending_events.is_empty() {
        trace.pending_events.clear();
        return;
    }
    let events = std::mem::take(&mut trace.pending_events);
    if let Some(writer) = trace.writer.as_mut() {
        if let Err(e) = writer.flush_events(events) {
            warn!("decision_trace: flush failed: {}", e);
        }
    }
}
