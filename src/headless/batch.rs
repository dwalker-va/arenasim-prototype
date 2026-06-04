//! Parallel in-process batch match runner.
//!
//! Reads a JSONL file of [`HeadlessMatchConfig`] (one config per line), runs
//! every match across all cores, and writes one CSV row per match. This is the
//! fast path for balance sweeps: 1v1, 2v2, 3v3, and strategy-var sweeps are all
//! just different sets of lines in the input.
//!
//! Three things make it fast vs. spawning one process per match (the old shell
//! approach):
//!   1. **In-process** — no per-match process spawn / dynamic-link cost.
//!   2. **Parse-once** — the three RON configs are parsed a single time and
//!      cloned into each match (see [`PreloadedConfigs`]).
//!   3. **Match-level parallelism** — Bevy's global task pools are pinned to one
//!      thread so each match runs single-threaded (deterministic, no nested
//!      contention), and we parallelize across matches with OS threads.
//!
//! Output order matches input order, so a downstream `awk`/`jq` step can join
//! against the generated input if needed. Determinism is preserved: each match
//! is an independent seeded `World`, so parallel results are byte-identical to
//! sequential ones (validated in `tests`/by the balance harness).

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::headless::config::HeadlessMatchConfig;
use crate::headless::runner::{run_headless_match_prepared, MatchResult, PreloadedConfigs};

/// Pin Bevy's global task pools to a single thread each. Each match's internal
/// schedule then runs single-threaded — deterministic and free of nested
/// parallelism — while THIS module parallelizes at the match level via OS
/// threads. Idempotent: `get_or_init` is a no-op once a pool exists, and the
/// later `MinimalPlugins` `TaskPoolPlugin` sees the pools already initialized.
fn pin_task_pools_single_threaded() {
    use bevy::tasks::{AsyncComputeTaskPool, ComputeTaskPool, IoTaskPool, TaskPoolBuilder};
    ComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());
    AsyncComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());
    IoTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());
}

/// Default worker count: leave a couple of cores for the OS / aggregation.
fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(2).max(1))
        .unwrap_or(4)
}

/// Run a batch of matches from a JSONL config file and write a per-match CSV.
pub fn run_batch(input: PathBuf, output: PathBuf, jobs: Option<usize>) -> Result<(), String> {
    // 1. Read & parse all match configs (one JSON object per non-blank line).
    let file = std::fs::File::open(&input)
        .map_err(|e| format!("open batch input {}: {}", input.display(), e))?;
    let reader = BufReader::new(file);
    let mut configs: Vec<HeadlessMatchConfig> = Vec::new();
    for (i, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| format!("read line {}: {}", i + 1, e))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let cfg: HeadlessMatchConfig = serde_json::from_str(trimmed)
            .map_err(|e| format!("parse batch line {}: {}", i + 1, e))?;
        configs.push(cfg);
    }
    let total = configs.len();
    if total == 0 {
        return Err("batch input contained no match configs".to_string());
    }

    // 2. Parse the three game-config RON files once for the whole run.
    let preloaded = Arc::new(PreloadedConfigs::load()?);

    // 3. Pin task pools so each match is internally single-threaded.
    pin_task_pools_single_threaded();

    let n_jobs = jobs.unwrap_or_else(default_jobs).max(1);
    let started = std::time::Instant::now();
    eprintln!("Batch: {} matches across {} workers", total, n_jobs);

    // 4. Run in parallel. A shared atomic cursor hands out indices (load-balances
    //    naturally across uneven match durations); each worker returns its own
    //    (index, result) pairs which we merge back into input order.
    let configs = Arc::new(configs);
    let cursor = Arc::new(AtomicUsize::new(0));

    let mut slots: Vec<Option<MatchResult>> = (0..total).map(|_| None).collect();

    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(n_jobs);
        for _ in 0..n_jobs {
            let configs = Arc::clone(&configs);
            let cursor = Arc::clone(&cursor);
            let preloaded = Arc::clone(&preloaded);
            handles.push(scope.spawn(move || {
                let mut local: Vec<(usize, MatchResult)> = Vec::new();
                loop {
                    let idx = cursor.fetch_add(1, Ordering::Relaxed);
                    if idx >= total {
                        break;
                    }
                    match run_headless_match_prepared(configs[idx].clone(), &preloaded, true, None) {
                        Ok(r) => local.push((idx, r)),
                        Err(e) => eprintln!(
                            "batch match {} ({} v {}) failed: {}",
                            idx,
                            configs[idx].team1.join("+"),
                            configs[idx].team2.join("+"),
                            e
                        ),
                    }
                }
                local
            }));
        }
        for h in handles {
            // A worker panic is a bug (e.g. a non-deterministic global); surface it.
            for (idx, r) in h.join().expect("batch worker thread panicked") {
                slots[idx] = Some(r);
            }
        }
    });

    // 5. Write per-match CSV in input order.
    write_results_csv(&output, &configs, &slots)?;

    let elapsed = started.elapsed().as_secs_f32();
    let completed = slots.iter().filter(|s| s.is_some()).count();
    eprintln!(
        "Batch complete: {}/{} matches in {:.1}s ({:.0}/s) -> {}",
        completed,
        total,
        elapsed,
        completed as f32 / elapsed.max(0.001),
        output.display()
    );
    Ok(())
}

/// Write one CSV row per match: the matchup identity, seed, outcome, and why it
/// ended. Aggregation (winrates per matchup) is left to cheap downstream tools.
fn write_results_csv(
    output: &PathBuf,
    configs: &[HeadlessMatchConfig],
    slots: &[Option<MatchResult>],
) -> Result<(), String> {
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {}", parent.display(), e))?;
        }
    }
    let file = std::fs::File::create(output)
        .map_err(|e| format!("create {}: {}", output.display(), e))?;
    let mut w = BufWriter::new(file);

    writeln!(w, "label,team1,team2,seed,winner,end_reason,duration_secs")
        .map_err(|e| e.to_string())?;
    for (cfg, slot) in configs.iter().zip(slots.iter()) {
        let team1 = cfg.team1.join("+");
        let team2 = cfg.team2.join("+");
        let label = cfg.label.clone().unwrap_or_default();
        let seed = cfg.random_seed.map(|s| s.to_string()).unwrap_or_default();
        match slot {
            Some(r) => {
                let winner = match r.winner {
                    Some(1) => "team1",
                    Some(2) => "team2",
                    _ => "draw",
                };
                writeln!(
                    w,
                    "{},{},{},{},{},{},{:.2}",
                    label, team1, team2, seed, winner, r.end_reason.as_str(), r.match_time
                )
                .map_err(|e| e.to_string())?;
            }
            None => {
                // Match errored out (logged to stderr above); record it so row
                // counts stay aligned with the input.
                writeln!(
                    w,
                    "{},{},{},{},error,error,0.00",
                    label, team1, team2, seed
                )
                .map_err(|e| e.to_string())?;
            }
        }
    }
    w.flush().map_err(|e| e.to_string())?;
    Ok(())
}
