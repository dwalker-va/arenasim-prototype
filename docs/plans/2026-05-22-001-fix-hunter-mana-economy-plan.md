---
date: 2026-05-22
status: completed
type: fix
title: Hunter mana economy tuning
origin: docs/brainstorms/2026-05-22-hunter-mana-economy-requirements.md
---

# fix: Hunter mana economy tuning

## Summary

Tune Hunter's mana economy to match the other four mana classes (regen 0, fixed pool per fight). `max_mana` rises 150 → 240, `mana_regen` drops 3.0 → 0.0, and the six Hunter ability mana costs are cut ~15% uniformly. Validation is a Hunter+Priest vs each-class+Priest 2v2 matrix sweep as the primary balance signal, with a 1v1 matrix sweep as quick diagnostic and a decision-trace audit confirming the `InsufficientMana` rejection class drops ≥50%.

---

## Problem Frame

Hunter sits at ~7% winrate across the 4,900-match 1v1 baseline (see origin: `docs/brainstorms/2026-05-22-hunter-mana-economy-requirements.md`). AI decision tracing of a 27.77s Hunter v Warrior match surfaces ~1,767 `InsufficientMana` rejections — the AI wants to act and cannot afford to. Hunter is the only class on a hybrid mana model (150 pool + 3/s regen); the other four mana classes use a "one bar per fight, no regen" pattern. Bringing Hunter into structural symmetry with the other mana classes addresses the diagnosed binding constraint without changing the AI's decision tree or introducing new resource types.

This is one slice of a multi-phase Hunter rebalance. Pet engagement, Felhunter Devour Magic counter, predictive trap placement, and Disengage follow-through ship separately (see origin's Scope Boundaries).

---

## Requirements Traceability

| Plan unit | Origin R-IDs covered |
|---|---|
| U1 | R10 (2v2 wrapper tooling support) |
| U2 | R10, R11 (pre-change baseline capture) |
| U3 | R1, R2, R3 (base stat changes) |
| U4 | R4-R9 (ability mana cost cuts) |
| U5 | R10, R11, R12 (post-change validation + trace audit + report) |

Success criteria from the origin's `## Success Criteria` are addressed in U5's validation outputs and the balance report.

---

## Key Technical Decisions

- **`starting_resource` (5th tuple value at `src/states/play_match/components/combatant.rs:215`) is raised 150 → 240 alongside `max_mana`.** Rationale: `apply_equipment` (`combatant.rs:463`) overwrites `current_mana` to the new `max_mana` for any combatant equipped via loadout, but `tests/cast_guard_tests.rs:110` constructs combatants via `Combatant::new` directly without equipment. Updating both values keeps the direct-construction path consistent with R3 ("full pool at match start").
- **2v2 validation uses a shell-script wrapper around `--headless`, not a matrix-runner extension.** Rationale: `HeadlessMatchConfig.team1: Vec<String>` (`src/headless/config.rs:16-19`) already accepts multi-class teams, so no schema work is needed. The wrapper mirrors `scripts/run_combat_tests.sh` and emits one JSON config per matchup × seed. Extending the matrix runner is a separate decision that should not gate this work (see origin's Dependencies / Assumptions).
- **Priest is the 2v2 healer partner.** Rationale: Paladin is also a healer but is currently dominant (Paladin v Rogue 100% in the baseline) — pairing Hunter with Paladin would conflate Hunter improvements with Paladin's existing advantage. Priest is the cleaner partner. Paladin re-runs are deferred as optional follow-up.
- **N=100 matches per matchup for both 1v1 and 2v2 sweeps**, matching the cadence of `design-docs/balance/matrix_baseline_2026-05-21.md`. Deterministic seeds via `--seed-base 0`.
- **Pre-change comparison uses `design-docs/balance/matrix_baseline_2026-05-21.md` for the 1v1 axis** (the latest pre-change baseline already exists). A new pre-change 2v2 baseline must be captured in U2 because no 2v2 data exists today.
- **Cost cuts are uniform ~15% across all six Hunter abilities** (per origin's Key Decisions). Selective cuts (e.g., keep Freezing Trap expensive) deferred as a possible follow-up.

---

## Implementation Units

### U1. 2v2 matrix sweep wrapper script

- **Goal:** A new shell script at `scripts/hunter_2v2_matrix.sh` that loops the 6 Hunter+Priest vs class+Priest matchups for N seeds, runs `target/release/arenasim --headless <config>`, and aggregates winrates into a CSV.
- **Requirements:** R10.
- **Dependencies:** None.
- **Files:**
  - `scripts/hunter_2v2_matrix.sh` (new)
- **Approach:**
  - Mirror the structure of `scripts/run_combat_tests.sh`.
  - Accept `N` (seed count) and optionally `--seed-base` as CLI arguments; default `N=100`, `--seed-base 0`.
  - Loop over the 6 opposing classes: Warrior, Mage, Rogue, Priest, Warlock, Paladin. (Hunter mirror is excluded — it's a self-match and not the validation target.)
  - For each opposing class × seed, emit a JSON config to `/tmp/hunter_2v2_<class>_<seed>.json` of shape `{"team1":["Hunter","Priest"],"team2":["<class>","Priest"],"max_duration_secs":120,"random_seed":<seed>}`.
  - Run `target/release/arenasim --headless <config>` for each. Parse the `Result: Team N (<duration>s)` line from stdout (precedent: `scripts/run_combat_tests.sh` parses match results similarly).
  - Aggregate per-matchup winrates: count Team 1 wins, Team 2 wins, draws. Write a CSV to a path passed via argument (e.g., `match_logs/2v2_hunter_<timestamp>.csv`) with columns matching the existing matrix format: `team1,team2,matches,team1_wins,team2_wins,draws,team1_winrate,draw_rate,avg_duration`.
- **Patterns to follow:**
  - `scripts/run_combat_tests.sh` — shell-script structure, JSON config generation, parsing pattern.
  - `src/headless/matrix.rs:97-188` — CSV output format and per-cell aggregation conventions.
- **Test scenarios:**
  - Test expectation: none — shell script behavior is verified by running it once with N=1 and confirming a single-row CSV is produced. The script itself is plumbing, not feature-bearing logic.
- **Verification:** Running `scripts/hunter_2v2_matrix.sh 1` produces a CSV with 6 rows (one per matchup) and the expected column shape. Script exits 0.

### U2. Capture pre-change 2v2 baseline

- **Goal:** Run U1's wrapper against the current Hunter values (pre-change) and commit the resulting baseline as durable comparison data for U5.
- **Requirements:** R10, R11.
- **Dependencies:** U1.
- **Files:**
  - `design-docs/balance/matrix_baseline_2026-05-22_2v2_pre.csv` (new)
  - `design-docs/balance/matrix_baseline_2026-05-22_2v2_pre.md` (new — short human-readable summary in the shape of `matrix_baseline_2026-05-21.md`)
- **Approach:**
  - Run `scripts/hunter_2v2_matrix.sh 100 --seed-base 0 > /tmp/2v2_pre.csv` (or equivalent invocation).
  - Move the CSV to `design-docs/balance/matrix_baseline_2026-05-22_2v2_pre.csv`.
  - Write the accompanying `.md` summary by hand following `matrix_baseline_2026-05-21.md` shape: matchup table with winrate, avg duration, and a 1-2 line interpretation note.
  - Commit under `docs(balance):` prefix.
- **Patterns to follow:**
  - `design-docs/balance/matrix_baseline_2026-05-21.md` — markdown summary shape.
  - Git history: `abbf3b6 docs(balance): capture initial 4900-match class matchup baseline` — commit convention.
- **Test scenarios:**
  - Test expectation: none — produces measurement artifacts, not behavior.
- **Verification:** CSV has 6 rows × 100 matches each = 600 simulated matches recorded. The `.md` file summarizes the same data in human-readable form. Both files committed.

### U3. Hunter base stat changes (combatant.rs)

- **Goal:** Update the Hunter row in `src/states/play_match/components/combatant.rs:215` so that `max_mana` is 240, `mana_regen` is 0.0, and `starting_resource` (5th tuple value) is 240.
- **Requirements:** R1, R2, R3.
- **Dependencies:** U2 (capture pre-change measurements before applying the change).
- **Files:**
  - `src/states/play_match/components/combatant.rs` (line 215 — the Hunter tuple)
- **Approach:**
  - Current: `match_config::CharacterClass::Hunter => (ResourceType::Mana, 265.0, 150.0, 3.0, 150.0, 18.0, 0.4, 30.0, 0.0, 0.07, 5.0),`
  - Target: `match_config::CharacterClass::Hunter => (ResourceType::Mana, 265.0, 240.0, 0.0, 240.0, 18.0, 0.4, 30.0, 0.0, 0.07, 5.0),`
  - Update the inline comment on the line above to reflect the new resource model (no regen, larger pool — match the pattern of Mage/Warlock comments).
- **Patterns to follow:**
  - Mage's row two above (`combatant.rs:201`) — single-line tuple with inline comment, no regen.
  - Prior balance commit: `641e325 balance(warlock): bump base HP 160 -> 180 for survivability` — shape of a single-file stat change commit.
- **Test scenarios:**
  - Test expectation: existing `cargo test` continues to pass. `tests/ability_tests.rs:71` validates `mana_cost >= 0` (passes — values are positive). `tests/cast_guard_tests.rs:110-180` uses `Combatant::new` directly and asserts on mana behavior — should pass with new values because we raised both `max_resource` and `starting_resource` consistently. `tests/headless_tests.rs::trace_on_matches_trace_off_outcomes` (Hunter pairings) — byte-equality between trace-on and trace-off runs unaffected.
- **Verification:** `cargo build --release` succeeds. `cargo test --release` passes. Running a single `target/release/arenasim --headless /tmp/test.json` where team1=["Hunter"], team2=["Warrior"] shows Hunter starting with 240 mana in the match log.

### U4. Hunter ability mana cost cuts (abilities.ron)

- **Goal:** Reduce the six Hunter ability `mana_cost` values by ~15% in `assets/config/abilities.ron`.
- **Requirements:** R4, R5, R6, R7, R8, R9.
- **Dependencies:** U2 (pre-change baseline must be captured first).
- **Files:**
  - `assets/config/abilities.ron`
- **Approach:**
  - Line 690 (AimedShot): `mana_cost: 40.0` → `34.0`
  - Line 717 (ArcaneShot): `mana_cost: 25.0` → `21.0`
  - Line 739 (ConcussiveShot): `mana_cost: 15.0` → `13.0`
  - Line 761 (Disengage): `mana_cost: 20.0` → `17.0`
  - Line 773 (FreezingTrap): `mana_cost: 50.0` → `43.0`
  - Line 792 (FrostTrap): `mana_cost: 30.0` → `26.0`
- **Patterns to follow:**
  - Prior balance commit: `7630fac balance(ua): iter 2 buff — UA tick 8->16, backlash 40->80 base + 0.5 SP` — config-only `abilities.ron` tuning commit shape.
- **Test scenarios:**
  - Test expectation: existing `cargo test` continues to pass. `tests/ability_tests.rs:71` validates `mana_cost >= 0` (passes — all new values positive). No ability-cost budget validator exists. No Hunter-specific stat tests pin existing cost values.
- **Verification:** `cargo build --release` succeeds. `cargo test --release` passes. Reading `assets/config/abilities.ron` confirms the six values are at the new numbers. A headless Hunter match log shows Hunter casting more abilities than before (qualitative — final validation in U5).

### U5. Post-change validation + balance report

- **Goal:** Run post-change 1v1 matrix, post-change 2v2 matrix, and a Hunter v Warrior decision-trace audit. Commit the post-change baseline artifacts and write a tuning report comparing pre/post.
- **Requirements:** R10, R11, R12.
- **Dependencies:** U3, U4.
- **Files:**
  - `design-docs/balance/matrix_baseline_2026-05-22.csv` (new — full 1v1 post-change)
  - `design-docs/balance/matrix_baseline_2026-05-22.md` (new — markdown summary)
  - `design-docs/balance/matrix_baseline_2026-05-22_2v2_post.csv` (new — 2v2 post-change)
  - `design-docs/balance/matrix_baseline_2026-05-22_2v2_post.md` (new — 2v2 markdown summary)
  - `docs/reports/2026-05-22-hunter-mana-tuning.md` (new — tuning report)
- **Approach:**
  - **1v1 sweep:** Run `target/release/arenasim --matrix 100 --seed-base 0`. The runner emits `match_logs/matrix_<timestamp>.{csv,md}` automatically. Move to `design-docs/balance/matrix_baseline_2026-05-22.{csv,md}`.
  - **2v2 sweep:** Run `scripts/hunter_2v2_matrix.sh 100 --seed-base 0`. Move output to `design-docs/balance/matrix_baseline_2026-05-22_2v2_post.csv`. Write a markdown summary alongside it.
  - **Trace audit:** Run `target/release/arenasim --headless /tmp/hunter_warrior.json --trace-mode on` with `team1=["Hunter"], team2=["Warrior"], max_duration_secs=60, random_seed=0`. Run the documented `jq` recipe on the resulting trace file:
    ```bash
    jq -r 'select(.actor.class == "Hunter") | .candidates[] | select(.status == "rejected") | .reason | if type == "object" then keys[0] else . end' $T | sort | uniq -c
    ```
    Record the `InsufficientMana` count and confirm it is ≥50% lower than the pre-change ~1,767 baseline.
  - **Report:** Write `docs/reports/2026-05-22-hunter-mana-tuning.md` following the shape of `docs/reports/2026-04-18-ua-simulation-tuning.md`. Include: pre/post Hunter 1v1 winrates per matchup, pre/post 2v2 Hunter+Priest team winrates, the InsufficientMana count delta, and a 2-3 paragraph interpretation noting which matchups moved as expected (Warrior, Rogue) vs which didn't (Mage, Paladin — different binding constraints per the origin doc's Success Criteria).
  - Commit baseline artifacts under `docs(balance):` and the report under `report(hunter):` (mirroring the UA precedent).
- **Patterns to follow:**
  - `docs/reports/2026-04-18-ua-simulation-tuning.md` — report shape (problem, measurement, recommendation, before/after table).
  - Git history: `abbf3b6 docs(balance): capture initial 4900-match class matchup baseline` and `report(ua):` commits.
  - `docs/solutions/implementation-patterns/ai-decision-trace.md:107-148` — determinism rules and jq recipes for the trace audit.
- **Test scenarios:**
  - Test expectation: none — produces measurement artifacts. Validation is whether the data matches the origin's Success Criteria:
    - 1v1 Hunter aggregate winrate moves from ~7% toward ~15-20%.
    - Hunter v Warrior and Hunter v Rogue move from 0% to ≥10%.
    - Hunter v Mage and Hunter v Paladin do **not** materially move (correctly — they're gated by different binding constraints).
    - 2v2 Hunter+Priest team reaches ≥30% winrate in at least 3 of 6 paired matchups.
    - `InsufficientMana` rejection count in the audit trace drops by ≥50%.
    - Non-Hunter matchups stay within ±5 percentage points of baseline (no collateral regressions).
- **Verification:** All five files committed. Report explicitly states whether each Success Criterion is met. If any criterion fails materially, surface as Residual Actionable Work for follow-up (e.g., revisit cost cut percentages, consider Approach C from the origin's deferred items).

---

## System-Wide Impact

- **Combat AI behavior in real matches:** Hunter will cast abilities more frequently. Other classes' AI is unchanged — but their winrates against Hunter may move because the Hunter is now applying more pressure. The U5 validation regression check (non-Hunter matchups within ±5pp of baseline) is the safety net.
- **Determinism:** Pure data-tuple edits don't introduce new collections or alter iteration order. Determinism rules (BTreeMap discipline, deterministic seeds) are unaffected. `tests/headless_tests.rs::trace_on_matches_trace_off_outcomes` continues to gate trace byte-equality.
- **Test surface:** No new tests required. Existing tests remain green:
  - `tests/ability_tests.rs` — generic ability invariants, no Hunter-specific assertions
  - `tests/cast_guard_tests.rs` — uses `Combatant::new` directly, sees the updated `starting_resource`
  - `tests/headless_tests.rs` — Hunter pairings included, validates determinism not winrate
  - `tests/registration_audit.rs` — no system registration changes
- **Equipment:** No Hunter equipment in `assets/config/loadouts.ron` carries `max_mana` or `mana_regen`, so the base-stat change is the entire effective change. No equipment retuning needed.
- **Documentation surface:** Three new files in `design-docs/balance/` (post-change CSV+MD, plus 2v2 pre+post). One new file in `docs/reports/`. The brainstorm doc (`docs/brainstorms/2026-05-22-hunter-mana-economy-requirements.md`) and the ideation doc (`docs/ideation/2026-05-22-hunter-rebalance-ideation.md`) remain unchanged.

---

## Scope Boundaries

- Pet engagement, Felhunter Devour Magic counter, predictive trap placement, team-comp-aware target selection, and Disengage follow-through — separately tracked Hunter rebalance survivors in `docs/ideation/2026-05-22-hunter-rebalance-ideation.md`. Out of scope.
- Approach C (Auto Shot returns mana) — deferred per origin's Key Decisions.
- Selective cost cuts (e.g., keep Freezing Trap expensive) — deferred per origin's Key Decisions.
- Extending `--matrix` to natively support team-comp templates — out of scope. Shell wrapper at `scripts/hunter_2v2_matrix.sh` is the chosen path.
- Paladin 2v2 partner — optional follow-up. This plan validates with Priest only.
- Mana stat changes to other classes — out of scope.
- Resource model changes (Energy / Focus for Hunter) — out of scope.
- Loadout / equipment changes — out of scope.
- Hunter AI logic changes — out of scope (the existing decision tree is expected to suffice once mana is no longer the binding constraint).
- Auto Shot tuning — out of scope.

### Deferred to Follow-Up Work

- If 1v1 Hunter aggregate winrate fails to move from 7% toward ~15-20% target after this change, revisit cost cuts (selective vs uniform) or consider Approach C from the origin.
- If trace audit shows `InsufficientMana` is no longer the dominant rejection but another rejection class (e.g., `OutOfRange`) dominates, that becomes a new diagnostic feeding the next survivor's brainstorm.
- Paladin 2v2 partner re-run if Priest results are inconclusive.

---

## Dependencies / Assumptions

- **Assumes `target/release/arenasim` builds cleanly with the stat changes.** No tests pin existing Hunter mana values (verified by research).
- **Assumes `HeadlessMatchConfig` already accepts `team1: ["Hunter","Priest"]`** per `src/headless/config.rs:16-19`. Confirmed by research.
- **Assumes the existing 1v1 pre-change baseline at `design-docs/balance/matrix_baseline_2026-05-21.md` is the comparison target** for the 1v1 axis. If a fresher baseline is committed before this plan ships, retarget U5's comparison.
- **Assumes the AI's existing decision tree functions correctly once mana isn't the binding constraint** (origin's stated bet). If post-change matrix shows surprising AI behavior (e.g., burns full pool in first 5 seconds then idles), AI changes become an in-scope follow-up — surface as Residual Actionable Work.
- **Assumes the matrix runner's determinism (BTreeMap discipline, fixed-step time) is unaffected** by stat tuple changes. Verified by research — no new collections introduced.

---

## Outstanding Questions (Deferred to Implementation)

- [Affects U1][Technical] Should the wrapper script emit per-match `.txt` logs (`--save-logs`-style) or aggregate stdout parsing only? Default to aggregate parsing for speed; revisit if debugging individual matches is needed.
- [Affects U5][Technical] If the trace audit shows `InsufficientMana` drops <50%, is the right move (a) more aggressive cost cuts in a follow-up, (b) deeper investigation of the AI's per-ability priority, or (c) accept and ship as a partial improvement? Decided at validation time based on actual numbers.
- [Affects U5][Technical] Exact matchup definitions for the 2v2 sweep: should both teams use identical Priest loadouts (same partner), or should team composition variants be explored? Default to identical Priest+Hunter mirror partner; consider variants only if winrate signal is unclear.
