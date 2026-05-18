---
date: 2026-05-18
topic: open-ideation
focus: refreshed improvement set after matrix runner ships
supersedes: 2026-04-26-open-ideation.md
---

# Ideation: ArenaSim — Open Improvement Set (May 2026 refresh)

Refresh of the [2026-04-26 ideation](2026-04-26-open-ideation.md). Same project shape; what changed is that the matrix runner shipped (PR #48), so balance work now has data behind it.

## What changed since April

- **Done:** `--matrix N` runner + deterministic seeded replays (PR #48, merged). Closed the last non-determinism gap (`CombatSnapshot` `HashMap` → `BTreeMap`). `run_headless_match` returns `Result<MatchResult, String>`; seed embedded in log header.
- **New artifact:** baseline matrix at `design-docs/balance/matrix_baseline_2026-05-16.{csv,md}` (4,900 matches, reproducible via `--matrix 100 --seed-base 0`).
- **Bonus side effect:** headless integration tests now run ~30× faster (manual `app.update()` loop + `TimeUpdateStrategy::ManualDuration`).

## Carry-over from April (still open)

### Refactors

1. **Single-source system registration** — collapse the parallel registrations in `states/mod.rs` (graphical) and `systems.rs::add_core_combat_systems` (headless). Still the #1 historical bug class per MEMORY.md (Divine Shield, Holy Shock, Dispels all hit it). The CombatSnapshot work in PR #48 tripped the audit again, reminder that the abstraction leaks.
   - Confidence: 90% • Complexity: Medium

2. **Split `equipment.rs` (1,400 LOC) and `auras.rs` (1,135 LOC)** — the two largest files. Likely lift item-budget validation out of equipment, and tick/expiration vs type-definitions out of auras.
   - Confidence: 80% • Complexity: Medium

3. **Unified `apply_damage(DamageEvent)` funnel** — collapse the ~7 distributed damage sites. Pet attribution and UA-backlash bugs were both "wrong Entity passed in." Adding a future stat (haste, hit, parry) currently threads through 7 sites.
   - Confidence: 85% • Complexity: High

4. **Trim large class_ai files** — `paladin.rs` (844) and `warlock.rs` (829) are 2× the median. Now safer with the predicate unit tests from the May 2 work.
   - Confidence: 75% • Complexity: Medium

### Features

5. **Diminishing returns for CC** — top of `roadmap.md`. Open known-issue `kidney-shot-shares-dr-with-other-stuns`. Probably the single most impactful gameplay change still pending.
   - Confidence: 90% • Complexity: Medium

6. **Silence CC type + HP-only combat-log filter** — both on the roadmap. Small, contained.
   - Confidence: 90% • Complexity: Low

7. **AI decision trace (JSONL) + F-key inspector overlay** — capture every AI decision (target, ability chosen, rejected candidates with reasons) into structured logs; toggleable egui overlay during graphical matches. Solves "why didn't X cast Y?" without rerunning. Pairs directly with the matrix work: when a cell looks off, traces make root-cause cheap.
   - Confidence: 80% • Complexity: Medium

### Quick wins

8. **Invert `match_logs/` retention** — only save on assertion failure or `--save-log`. Folder is back to a few entries but will refill. PR #48 added `--save-logs` for matrix mode; this would extend the same logic to the default `--headless` path.
   - Confidence: 95% • Complexity: Low

9. **Codegen `AbilityType` + `expected_abilities` from `abilities.ron`** — build script reads RON at compile time. Eliminates 2 of 7 manual steps for adding an ability.
   - Confidence: 85% • Complexity: Low

## New work surfaced by the baseline matrix

The 4,900-match baseline made these concrete with hard numbers. All are now measurable: tune → `--matrix 100` → diff cells.

10. **Hunter rebuild — single largest balance outlier.** True winrate ~7%. Loses 0% across 6 of 7 row matchups. Goes 0% as T1 vs Warriors and Rogues — a ranged class with a pet should not be losing to melee at 0%. Bounded fix surface (one class). With the matrix runner this is a tight iteration loop.
    - Confidence: 95% • Complexity: Medium

11. **Investigate Paladin > Rogue 100%/100%.** Counter to expected design — Rogue's burst should pierce Paladin defensives. Likely Divine Shield + Holy Shock burst countering stealth + Cheap Shot, but worth tracing one match to confirm. Pairs with #7 (decision trace) for diagnosis.
    - Confidence: 80% • Complexity: Small (investigation), Medium (fix)

12. **Tune Mage vs Warrior.** Mage wins 100% in both seat orderings — consistent class advantage, not positional. Frost Nova + range is decisive. If a flatter distribution is wanted, lever is Frost Nova duration or Polymorph cooldown.
    - Confidence: 70% (whether to tune is a design call) • Complexity: Low (one stat tweak + re-run matrix)

13. **Healer mirror stalls.** Paladin v Paladin avg 207s (vs 300s timeout), Priest v Priest 90s, Mage v Mage 64% draws. Either accept and reduce default timeout, or add fatigue mechanic to break stalemates.
    - Confidence: 75% • Complexity: Medium

14. **`--matrix --diff <baseline.csv>`.** Small add-on now that a baseline exists. Highlights cells that moved >X% after a balance change. ~50 LOC in `matrix.rs`; makes #10–13 dramatically faster to iterate.
    - Confidence: 95% • Complexity: Low

## Recommended sequencing

Two viable paths:

**Balance-first** (use the tool you just built):
1. #14 (matrix diff) — 1 hour, makes everything else faster
2. #11 (Paladin > Rogue investigation) — gather evidence first
3. #10 (Hunter rebuild) — biggest user-visible impact
4. #5 (DR for CC) — interacts with all classes; consider after Hunter so Hunter changes settle first

**Infra-first** (compound returns):
1. #1 (single-source registration) — removes a recurring bug class permanently
2. #14 (matrix diff)
3. #10 (Hunter rebuild)

Personal recommendation: balance-first. The matrix runner is fresh, the data is fresh, and Hunter at 7% is a clear "ship something visible" win. Infra refactors are always there.

## Baseline reference (from PR #48)

Average T1 row winrate per class:

| Class | T1 winrate | Notes |
|---|---|---|
| Mage | 77% | Frost Nova + kite |
| Paladin | 73% | Heals + Divine Shield |
| Rogue | 71% | Stealth opener, except vs Paladin |
| Warrior | 53% | OK except vs Mage/Rogue |
| Warlock | 39% | DoTs not enough |
| Priest | 21% | Healer, loses to all DPS |
| Hunter | 7% | **Broken** |

Full matrix: `design-docs/balance/matrix_baseline_2026-05-16.md`.
