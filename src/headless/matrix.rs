//! 7×7 class matchup matrix runner.
//!
//! Runs every (team1_class, team2_class) pair N times via the existing
//! `run_headless_match_with` entry point, accumulates per-cell win/loss/draw
//! counts and average duration, and writes both a CSV (raw cells) and a
//! Markdown heatmap (human-readable summary) to `match_logs/`.
//!
//! Determinism: each match uses `seed = seed_base + global_match_index` so
//! the same `--seed-base` reproduces identical matrix output. Combined with
//! the seeded `GameRng` and `BTreeMap`-backed `CombatSnapshot`, replays are
//! bit-for-bit reproducible.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::states::match_config::CharacterClass;

use super::config::HeadlessMatchConfig;
use super::runner::{run_headless_match_with, TraceConfig};
use crate::cli::TraceMode;

/// Per-cell stats accumulator. One cell = one (team1_class, team2_class) pair.
#[derive(Debug, Default, Clone)]
struct CellStats {
    runs: u32,
    team1_wins: u32,
    team2_wins: u32,
    draws: u32,
    sum_duration: f32,
}

impl CellStats {
    fn record(&mut self, winner: Option<u8>, duration: f32) {
        self.runs += 1;
        self.sum_duration += duration;
        match winner {
            Some(1) => self.team1_wins += 1,
            Some(2) => self.team2_wins += 1,
            _ => self.draws += 1,
        }
    }

    fn team1_winrate(&self) -> f32 {
        if self.runs == 0 { 0.0 } else { self.team1_wins as f32 / self.runs as f32 }
    }

    fn draw_rate(&self) -> f32 {
        if self.runs == 0 { 0.0 } else { self.draws as f32 / self.runs as f32 }
    }

    fn avg_duration(&self) -> f32 {
        if self.runs == 0 { 0.0 } else { self.sum_duration / self.runs as f32 }
    }
}

/// Run the 7×7 matchup matrix and write CSV + Markdown reports.
///
/// `trace_mode` defaults to `On` for matrix runs (see CLI resolution in main.rs);
/// when enabled, each match writes its own JSONL trace to
/// `match_logs/traces/match_<seed>_<class1>_v_<class2>_trace.jsonl`.
pub fn run_matrix(n: u32, seed_base: u64, save_logs: bool, trace_mode: TraceMode) -> Result<(), String> {
    if n == 0 {
        return Err("--matrix N requires N >= 1".to_string());
    }

    let classes = CharacterClass::all();
    let cell_count = classes.len() * classes.len();
    let total_matches = cell_count as u32 * n;

    println!("Running matrix: {}×{} matchups × {} runs = {} matches (seed_base={}, trace={:?})",
        classes.len(), classes.len(), n, total_matches, seed_base, trace_mode);

    if trace_mode.is_enabled() {
        fs::create_dir_all("match_logs/traces").map_err(|e| format!("create match_logs/traces/: {}", e))?;
    }

    let started = Instant::now();
    let mut stats: HashMap<(CharacterClass, CharacterClass), CellStats> = HashMap::new();
    let mut global_idx: u64 = 0;

    for &c1 in classes {
        for &c2 in classes {
            let mut cell = CellStats::default();
            for run in 0..n {
                let seed = seed_base.wrapping_add(global_idx);
                global_idx += 1;
                let config = build_config(c1, c2, seed);

                let trace_config = if trace_mode.is_enabled() {
                    Some(TraceConfig {
                        output_path: format!(
                            "match_logs/traces/match_{}_{}_v_{}_trace.jsonl",
                            seed, c1.name(), c2.name()
                        )
                        .into(),
                        verbose: trace_mode.is_verbose(),
                    })
                } else {
                    None
                };

                match run_headless_match_with(config, !save_logs, trace_config) {
                    Ok(result) => cell.record(result.winner, result.match_time),
                    Err(e) => {
                        eprintln!("  Match {} vs {} run {} failed: {}", c1.name(), c2.name(), run, e);
                    }
                }
            }
            print!("  {} vs {}: T1={} T2={} D={} (avg {:.1}s)\r",
                c1.name(), c2.name(), cell.team1_wins, cell.team2_wins, cell.draws, cell.avg_duration());
            std::io::stdout().flush().ok();
            stats.insert((c1, c2), cell);
        }
    }

    let elapsed = started.elapsed().as_secs_f32();
    println!("\nMatrix complete in {:.1}s ({:.0} matches/sec)",
        elapsed, total_matches as f32 / elapsed.max(0.001));

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    fs::create_dir_all("match_logs").map_err(|e| format!("create match_logs/: {}", e))?;

    let csv_path = format!("match_logs/matrix_{}.csv", timestamp);
    write_csv(&csv_path, classes, &stats, n, seed_base)
        .map_err(|e| format!("write {}: {}", csv_path, e))?;
    println!("Wrote {}", csv_path);

    let md_path = format!("match_logs/matrix_{}.md", timestamp);
    write_markdown(&md_path, classes, &stats, n, seed_base, elapsed)
        .map_err(|e| format!("write {}: {}", md_path, e))?;
    println!("Wrote {}", md_path);

    Ok(())
}

/// Build a minimal `HeadlessMatchConfig` for a 1v1 matchup with a fixed seed.
/// All per-class strategy options use defaults — the matrix is for raw class
/// matchup balance, not loadout testing.
fn build_config(team1: CharacterClass, team2: CharacterClass, seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec![team1.name().to_string()],
        team2: vec![team2.name().to_string()],
        map: "BasicArena".to_string(),
        team1_kill_target: None,
        team2_kill_target: None,
        team1_cc_target: None,
        team2_cc_target: None,
        output_path: None,
        max_duration_secs: 300.0,
        random_seed: Some(seed),
        team1_rogue_openers: vec![],
        team2_rogue_openers: vec![],
        team1_warlock_curse_prefs: vec![],
        team2_warlock_curse_prefs: vec![],
        team1_hunter_pet_types: vec![],
        team2_hunter_pet_types: vec![],
        team1_equipment: vec![],
        team2_equipment: vec![],
        team1_warrior_shouts: vec![],
        team2_warrior_shouts: vec![],
        team1_mage_armors: vec![],
        team2_mage_armors: vec![],
        team1_paladin_auras: vec![],
        team2_paladin_auras: vec![],
    }
}

fn write_csv(
    path: &str,
    classes: &[CharacterClass],
    stats: &HashMap<(CharacterClass, CharacterClass), CellStats>,
    n: u32,
    seed_base: u64,
) -> std::io::Result<()> {
    let mut f = fs::File::create(path)?;
    writeln!(f, "# Matrix run: n={} seed_base={}", n, seed_base)?;
    writeln!(f, "team1,team2,runs,team1_wins,team2_wins,draws,team1_winrate,draw_rate,avg_duration_secs")?;
    for &c1 in classes {
        for &c2 in classes {
            let cell = stats.get(&(c1, c2)).cloned().unwrap_or_default();
            writeln!(f,
                "{},{},{},{},{},{},{:.4},{:.4},{:.2}",
                c1.name(), c2.name(),
                cell.runs, cell.team1_wins, cell.team2_wins, cell.draws,
                cell.team1_winrate(), cell.draw_rate(), cell.avg_duration())?;
        }
    }
    Ok(())
}

fn write_markdown(
    path: &str,
    classes: &[CharacterClass],
    stats: &HashMap<(CharacterClass, CharacterClass), CellStats>,
    n: u32,
    seed_base: u64,
    elapsed_secs: f32,
) -> std::io::Result<()> {
    let mut f = fs::File::create(path)?;

    writeln!(f, "# Matrix Run")?;
    writeln!(f)?;
    writeln!(f, "- **Runs per cell:** {}", n)?;
    writeln!(f, "- **Seed base:** {} (cell `(c1, c2)` run `i` uses seed `seed_base + (cell_idx × N + i)`)", seed_base)?;
    writeln!(f, "- **Total matches:** {}", classes.len().pow(2) as u32 * n)?;
    writeln!(f, "- **Wall time:** {:.1}s", elapsed_secs)?;
    writeln!(f)?;

    writeln!(f, "## Team 1 Winrate (rows = team 1, columns = team 2)")?;
    writeln!(f)?;
    write!(f, "| T1 \\ T2 |")?;
    for &c in classes { write!(f, " {} |", short(c))?; }
    writeln!(f)?;
    write!(f, "|---|")?;
    for _ in classes { write!(f, "---|")?; }
    writeln!(f)?;
    for &c1 in classes {
        write!(f, "| **{}** |", short(c1))?;
        for &c2 in classes {
            let cell = stats.get(&(c1, c2)).cloned().unwrap_or_default();
            write!(f, " {:>3.0}% |", cell.team1_winrate() * 100.0)?;
        }
        writeln!(f)?;
    }
    writeln!(f)?;

    writeln!(f, "## Draw Rate")?;
    writeln!(f)?;
    write!(f, "| T1 \\ T2 |")?;
    for &c in classes { write!(f, " {} |", short(c))?; }
    writeln!(f)?;
    write!(f, "|---|")?;
    for _ in classes { write!(f, "---|")?; }
    writeln!(f)?;
    for &c1 in classes {
        write!(f, "| **{}** |", short(c1))?;
        for &c2 in classes {
            let cell = stats.get(&(c1, c2)).cloned().unwrap_or_default();
            write!(f, " {:>3.0}% |", cell.draw_rate() * 100.0)?;
        }
        writeln!(f)?;
    }
    writeln!(f)?;

    writeln!(f, "## Average Match Duration (seconds)")?;
    writeln!(f)?;
    write!(f, "| T1 \\ T2 |")?;
    for &c in classes { write!(f, " {} |", short(c))?; }
    writeln!(f)?;
    write!(f, "|---|")?;
    for _ in classes { write!(f, "---|")?; }
    writeln!(f)?;
    for &c1 in classes {
        write!(f, "| **{}** |", short(c1))?;
        for &c2 in classes {
            let cell = stats.get(&(c1, c2)).cloned().unwrap_or_default();
            write!(f, " {:>4.1} |", cell.avg_duration())?;
        }
        writeln!(f)?;
    }
    writeln!(f)?;

    // Summary stats — useful for spotting outliers without re-reading the grid.
    let cells: Vec<&CellStats> = stats.values().collect();
    let totals_runs: u32 = cells.iter().map(|c| c.runs).sum();
    let totals_t1: u32 = cells.iter().map(|c| c.team1_wins).sum();
    let totals_t2: u32 = cells.iter().map(|c| c.team2_wins).sum();
    let totals_draw: u32 = cells.iter().map(|c| c.draws).sum();

    writeln!(f, "## Totals")?;
    writeln!(f)?;
    writeln!(f, "- Matches: {}", totals_runs)?;
    writeln!(f, "- Team 1 wins: {} ({:.1}%)", totals_t1, pct(totals_t1, totals_runs))?;
    writeln!(f, "- Team 2 wins: {} ({:.1}%)", totals_t2, pct(totals_t2, totals_runs))?;
    writeln!(f, "- Draws: {} ({:.1}%)", totals_draw, pct(totals_draw, totals_runs))?;
    writeln!(f)?;

    // Mirror-matchup sanity: same class on both sides should converge to ~50%
    // (modulo team-1 spawn-side bias). Big asymmetries here usually mean a
    // determinism leak or a position-dependent bug.
    writeln!(f, "## Mirror Matchups (T1 winrate; expect ~50%)")?;
    writeln!(f)?;
    for &c in classes {
        let cell = stats.get(&(c, c)).cloned().unwrap_or_default();
        writeln!(f, "- {} vs {}: **{:.0}%** T1 win, {:.0}% draw ({} matches, avg {:.1}s)",
            c.name(), c.name(),
            cell.team1_winrate() * 100.0,
            cell.draw_rate() * 100.0,
            cell.runs,
            cell.avg_duration())?;
    }

    Ok(())
}

fn pct(numerator: u32, denominator: u32) -> f32 {
    if denominator == 0 { 0.0 } else { numerator as f32 / denominator as f32 * 100.0 }
}

/// 3-letter class abbreviation for compact heatmap headers.
fn short(c: CharacterClass) -> &'static str {
    match c {
        CharacterClass::Warrior => "War",
        CharacterClass::Mage => "Mag",
        CharacterClass::Rogue => "Rog",
        CharacterClass::Priest => "Pri",
        CharacterClass::Warlock => "Wlk",
        CharacterClass::Paladin => "Pal",
        CharacterClass::Hunter => "Hun",
    }
}
