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
    /// Consecutive flush failures. After CIRCUIT_BREAKER_THRESHOLD, the
    /// `flush_decision_trace_system` detaches the writer to stop the
    /// per-frame warn spam loop.
    consecutive_failures: u32,
}

const CIRCUIT_BREAKER_THRESHOLD: u32 = 3;

impl TraceWriter {
    pub fn create(path: PathBuf) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = File::create(&path)?;
        Ok(Self {
            inner: BufWriter::new(file),
            path,
            consecutive_failures: 0,
        })
    }

    /// Number of consecutive `flush_events` failures since the last success.
    /// Used by `flush_decision_trace_system` to drive the circuit breaker.
    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }

    /// True if the writer has accumulated enough consecutive failures to be
    /// detached. Once true, the next flush_decision_trace_system tick should
    /// drop the writer from DecisionTrace.
    pub fn should_detach(&self) -> bool {
        self.consecutive_failures >= CIRCUIT_BREAKER_THRESHOLD
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Sort events by `(frame, actor.entity_id, kind)` and write each as a JSONL
    /// line. Returns the number of events written.
    ///
    /// Increments `consecutive_failures` on Err and resets it to 0 on Ok.
    /// The flush system reads this counter to detach the writer after
    /// `CIRCUIT_BREAKER_THRESHOLD` consecutive failures, stopping the
    /// per-frame warn spam loop in a broken-writer state (ENOSPC, etc.).
    pub fn flush_events(&mut self, mut events: Vec<DecisionEvent>) -> std::io::Result<usize> {
        events.sort_by(|a, b| {
            (a.frame, a.actor.entity_id, kind_order(a.kind)).cmp(&(
                b.frame,
                b.actor.entity_id,
                kind_order(b.kind),
            ))
        });
        let count = events.len();
        let result = (|| -> std::io::Result<()> {
            for event in events {
                serde_json::to_writer(&mut self.inner, &event)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                self.inner.write_all(b"\n")?;
            }
            self.inner.flush()?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.consecutive_failures = 0;
                Ok(count)
            }
            Err(e) => {
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                Err(e)
            }
        }
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

/// System that drains pending decision events into the writer and advances
/// the frame/sim_time clock for the NEXT frame's events.
///
/// Registered in `add_core_combat_systems` (which is called from both
/// `HeadlessPlugin::build` for headless mode and `StatesPlugin::build`
/// for graphical mode) — a single registration reaches both modes through
/// that helper.
///
/// Frame ordering: events emitted by class AI / target acquisition / pet AI
/// in Phase 2 (CombatAndMovement) carry the frame number and sim_time set
/// BEFORE this system runs. We bump the counters AFTER drain so the next
/// frame's events get fresh values, and frame 0 events carry the initial
/// `current_frame == 0` / `current_sim_time == 0.0` set at writer install.
pub fn flush_decision_trace_system(
    time: Res<Time>,
    countdown: Res<crate::states::play_match::components::MatchCountdown>,
    mut trace: ResMut<DecisionTrace>,
) {
    // Drain pending events FIRST so they carry the frame/sim_time values
    // active when they were emitted.
    let mut detach_writer = false;
    if trace.writer.is_some() && !trace.pending_events.is_empty() {
        let events = std::mem::take(&mut trace.pending_events);
        if let Some(writer) = trace.writer.as_mut() {
            if let Err(e) = writer.flush_events(events) {
                warn!("decision_trace: flush failed: {}", e);
            }
            if writer.should_detach() {
                detach_writer = true;
            }
        }
    } else {
        // No writer or no events — clear unconditionally so pending_events
        // doesn't accumulate forever when tracing is disabled.
        trace.pending_events.clear();
    }

    // Circuit breaker: after N consecutive flush failures, detach the writer
    // so we stop the per-frame warn-and-discard cycle and stop the in-memory
    // events from getting written into a broken file.
    if detach_writer {
        warn!("decision_trace: detaching writer after repeated flush failures");
        trace.writer = None;
    }

    // Advance the clock for the NEXT frame's events.
    trace.current_frame = trace.current_frame.wrapping_add(1);
    if countdown.gates_opened {
        trace.current_sim_time += time.delta_secs();
    }
}
