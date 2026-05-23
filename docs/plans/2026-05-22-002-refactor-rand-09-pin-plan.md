---
date: 2026-05-22
type: refactor
title: Migrate rand to 0.9 and pin at 0.9
status: completed
origin: docs/ideation/2026-05-22-library-upgrades-ideation.md
---

# Migrate `rand` to 0.9 and pin at 0.9

## Summary

Bump `rand` from 0.8 to 0.9 in `Cargo.toml`, replace two deprecated API calls in `src/states/play_match/components/resources.rs`, and add a comment above the `rand` line documenting why we deliberately stop at 0.9 (do not chase 0.10). The headline value is the *pinning rationale* — turning a recurring "should we bump rand?" decision into a written, defensible non-upgrade.

## Problem Frame

`rand` 0.10 is a determinism downgrade for ArenaSim. It removes `Clone` from `StdRng` and `ChaCha*Rng` — the exact types the 4900-match seeded matrix runner depends on for byte-identical replay. `SmallRng` is explicitly documented as non-reproducible across rand versions, so a future contributor (or LLM agent) reaching for it thinking "fast RNG" would silently break replay determinism. Meanwhile rand 0.9 deprecates `gen()` / `from_entropy()` in favor of `random()` / `from_os_rng()`, so staying on 0.8 means future LLM-generated code keeps picking up the deprecated names. The right move is to migrate forward to 0.9 (removes deprecated API surface) AND pin there with a `Cargo.toml` comment that names the pin and the reason — so the next dependency-refresh PR doesn't accidentally walk into the 0.10 cliff. ChaCha's seeded output is byte-stable across the 0.8 → 0.9 → 0.10 span (per the rand book), so the matrix runner will produce identical match outcomes pre- and post-edit; that's the verification gate. (see origin: `docs/ideation/2026-05-22-library-upgrades-ideation.md` survivor #2)

## Requirements

- R1. `Cargo.toml`: `rand = "0.8"` → `rand = "0.9"`, with a 1-2 line comment immediately above naming the pin ceiling and the reason (rand 0.10 removes `Clone` from `StdRng`/`ChaCha*Rng`; `SmallRng` is non-reproducible across versions; both would break the matrix runner's seeded-replay invariant).
- R2. `src/states/play_match/components/resources.rs:34`: replace `StdRng::from_entropy()` with `StdRng::from_os_rng()`.
- R3. `src/states/play_match/components/resources.rs:41`: replace `self.rng.gen()` with `self.rng.random()`.
- R4. Capture a pre-edit matrix baseline (`cargo run --release -- --matrix 100`) BEFORE the Cargo.toml edit so U3 has a known reference point to diff against. `match_logs/` is empty in this worktree — no usable baseline exists today.
- R5. `cargo test --release` passes after the edits. Includes `tests/headless_tests.rs` seeded-determinism tests, `tests/decision_trace_audit.rs`, `tests/decision_trace_builder.rs`, and `tests/ability_tests.rs` (the latter doesn't touch RNG but exercises full config-load + serde paths).
- R6. Post-edit matrix run produces a CSV byte-identical to the U0 baseline. Any class-pair winrate shift outside RNG noise (none expected, since rand 0.9 preserves ChaCha output for the same seed) blocks merge pending investigation.
- R7. Only `Cargo.toml` and `src/states/play_match/components/resources.rs` are modified — escalate if any other source file needs an edit (no other call sites touch `rand` directly).

## Requirements Traceability

| Plan U-ID | Requirement | What it covers |
|---|---|---|
| U0 | R4 | Pre-edit matrix baseline (4900-match seeded run on current `Cargo.lock`) |
| U1 | R1, R2, R3, R7 | Apply the dependency bump + the 2 line edits in `resources.rs` + the `Cargo.toml` pin comment |
| U2 | R5 | `cargo test --release` passes |
| U3 | R6 | Post-edit matrix run + byte-identity diff vs U0 baseline |

## Implementation Units

### U0. Capture pre-edit matrix baseline

**Goal:** Produce the canonical pre-edit matrix-run output so U3 has a concrete reference point to diff against. Without this step, U3's "compare against the latest baseline" reduces to "run a matrix and observe it produced numbers," which cannot detect a winrate shift.

**Requirements:** R4 (prerequisite for U3's comparison)

**Dependencies:** none — runs against current `Cargo.lock` before any edits

**Files:** No source edits. Generates `match_logs/matrix_<timestamp>.csv` and `match_logs/matrix_<timestamp>.md` (per `src/cli.rs:53` matrix runner output). These are NOT committed (`match_logs/` is gitignored). The PR commit body records the file path + summary content verbatim.

**Approach:** Run `cargo run --release -- --matrix 100` against the current `Cargo.lock` (before U1 touches anything). Save the generated matrix summary file path; copy its contents into a scratch note for the PR commit body. This is the U3-comparison baseline.

**Why this is U0 and not part of U3:** Generating the baseline *after* U1 would require reverting the Cargo.toml edit to recover the pre-edit `Cargo.lock` state — a stash dance that's strictly worse than capturing the baseline first. Proven shape from PR #54.

**Caveat — stale matrix files exist from PR #54.** `match_logs/` already contains `matrix_1779473428.{csv,md}` (PR #54 pre-edit baseline) and `matrix_1779476958.{csv,md}` (PR #54 post-edit run). Those are PR #54's audit trail and should not be confused with U0's output. Record the U0 timestamp explicitly in a scratch note and reference *that exact timestamp* in U3 — do not rely on "most recent file" heuristics.

**Test scenarios:** Not feature-bearing. `Test expectation: none -- baseline-capture step, no behavior to test.`

**Verification:** A new `matrix_<timestamp>.csv` and `.md` pair exists in `match_logs/` with a complete 7×7 summary table. **Record the exact U0 timestamp** in a scratch note — U3 references this specific file, not "latest in match_logs/".

---

### U1. Apply dependency bump + API renames + pin comment

**Goal:** Bump `rand` to 0.9 in `Cargo.toml` with a documented pin ceiling, replace the two deprecated API calls in `resources.rs`, and verify `cargo build --release` passes.

**Requirements:** R1, R2, R3, R7

**Dependencies:** U0 (baseline must exist before edits)

**Files:**
- `Cargo.toml` — change `rand = "0.8"` to `rand = "0.9"`, add a 1-2 line comment immediately above the line naming the pin and its reason
- `src/states/play_match/components/resources.rs` — two line edits at `:34` (`from_entropy()` → `from_os_rng()`) and `:41` (`.gen()` → `.random()`)
- `Cargo.lock` — regenerated by `cargo update` (gitignored per project convention)

**Approach:**
1. Add the Cargo.toml comment first (so the rationale lands above the bump). Suggested comment shape: `# Pinned at 0.9 — rand 0.10 removes Clone from StdRng/ChaCha*Rng (breaks seeded matrix replay)` plus a second line if needed for `SmallRng` non-reproducibility. Implementer chooses exact wording within those constraints.
2. Bump `rand = "0.8"` to `rand = "0.9"` on the next line.
3. Apply the two API renames in `resources.rs`. `use rand::prelude::*` at line 3 stays — both `Rng` and `SeedableRng` are still re-exported in 0.9.
4. Run `cargo update` to refresh `Cargo.lock`. Inspect the diff to confirm `rand` advanced to 0.9.x and the other leaf crates (which already auto-resolved during PR #54) didn't shift further than expected.
5. Run `cargo build --release` to verify compile. If it fails on anything other than the two enumerated lines in `resources.rs`, R7 escalation applies — stop and surface the unexpected call site rather than fixing inline.

**Caveat — `rand::prelude` re-exports.** The two call sites in `resources.rs` both rely on the `Rng` trait (for `.random()`) and `SeedableRng` (for `from_os_rng()` / `seed_from_u64`). Both stay in `rand::prelude` in 0.9. If `cargo build` surfaces a "method not found" error on either trait, **escalate per R7** — the prelude re-export surface is part of the assumed encapsulation; a shift there means the migration map is incomplete and the PR scope changes. Do not patch trait imports locally.

**Test scenarios:**
- `cargo build --release` succeeds after the edits. Confirms `from_os_rng()` and `.random()` are valid API in rand 0.9, and that no other source file references a deprecated API that would force an additional edit.
- `Cargo.lock` shows `rand` at 0.9.x (typically 0.9.x.y) and `rand_chacha` advanced in lockstep (rand 0.9 may pull a newer rand_chacha — both stay byte-stable for ChaCha output).
- Only `Cargo.toml`, `src/states/play_match/components/resources.rs`, and `Cargo.lock` appear in `git status`. Any other modified file is a sign of a hidden rand API surface that wasn't enumerated — investigate.

**Verification:** `cargo build --release` exits 0. `git diff --name-only` shows exactly two tracked files modified (`Cargo.toml` + `resources.rs`); `Cargo.lock` is gitignored so doesn't appear.

---

### U2. Verify via `cargo test --release`

**Goal:** Confirm the existing test suite passes against the new dependency version and the migrated API calls.

**Requirements:** R5, R7

**Dependencies:** U1

**Files:**
- No source edits. This unit invokes existing tests. The seeded-determinism surface (most version-sensitive) lives in:
  - `tests/headless_tests.rs` — seeded match replay tests (`seeded_matches_are_deterministic`, `trace_file_is_deterministic_at_same_seed`, `different_seeds_produce_different_matches`)
  - `tests/decision_trace_audit.rs` — exercises full match execution including RNG-driven combat
  - `tests/decision_trace_builder.rs` — builder-level coverage of the `#[serde(untagged)]` `RejectionReason` (untouched by this PR, but the suite is fast and would catch surprises)
  - `tests/ability_tests.rs`, `tests/combat_*.rs`, `tests/cast_guard_tests.rs`, `tests/class_ai_decisions.rs`, `tests/registration_audit.rs` — full coverage of the rest of the system

**Approach:** Run `cargo test --release`. Reasoning: rand 0.9 preserves seeded ChaCha output across the 0.8 → 0.9 span (per the rand book and the verifiable U3 diff). The seeded-determinism tests in `tests/headless_tests.rs` are the strongest signal — they assert byte-identical trace JSONL output for the same seed. If those pass, R5 is met.

A failure surfaces a real compatibility break — escalate per R7 rather than fixing inline.

**Test scenarios:**
- Covers AE2-equivalent. Given the U1 edits, when `cargo test --release` runs, then every existing test passes. Specifically: `tests/headless_tests::seeded_matches_are_deterministic` passes (ChaCha output stable), `trace_file_is_deterministic_at_same_seed` passes (full trace JSONL byte-identical for same seed). No existing test needs modification.
- If any seeded-determinism test fails, that's a real ChaCha output divergence — STOP, do not modify the test to match the new output. Investigate and surface for review.

**Verification:** `cargo test --release` exits 0 with all suites green (target: 240+ passed, 0 failed, ~3 ignored matching PR #54's baseline). No source-file modifications beyond U1's two lines in `resources.rs`.

---

### U3. Post-edit matrix run and byte-identity diff against U0 baseline

**Goal:** Prove via 4900-match matrix that the rand 0.9 migration preserves combat behavior byte-for-byte. This is the strongest behavioral oracle the project has.

**Requirements:** R6

**Dependencies:** U2 (and U0 for the baseline)

**Files:** No source edits. Generates `match_logs/matrix_<timestamp>.csv` and `.md` (gitignored). PR commit body records the diff result.

**Approach:** Run `cargo run --release -- --matrix 100` against the post-edit `Cargo.lock`. Compare against the U0 baseline file via `diff -q match_logs/matrix_<U0-timestamp>.csv match_logs/matrix_<U3-timestamp>.csv`. Expected: byte-identical CSV (every class-pair winrate, every draw rate, every match-duration averaged across 100 matches matches exactly). The MD file is expected to differ only on the wall-clock timestamp line (PR #54 saw `3260.3s` vs `3248.7s`).

If the CSV differs by even one match outcome, that's a real regression — the rand 0.9 ChaCha output has somehow diverged from 0.8 for the same seed (would contradict the rand book's stability guarantee). STOP and surface for investigation before merging.

**Test scenarios:**
- Covers AE3-equivalent. Given a green U2, when `cargo run --release -- --matrix 100` runs, then `diff -q baseline.csv post-edit.csv` reports no differences. The MD file diff shows only the wall-clock timestamp line as different.
- If CSV differs: STOP. Do not investigate further inside U3 — surface to the user. Likely root causes (in descending probability): (a) a transitive crate also advanced and that crate is on the determinism path, (b) a hidden code path I didn't enumerate uses a now-deprecated API and the migration changed semantics, (c) the rand book's stability guarantee doesn't hold for the exact crate combination in use. All three need real investigation, not a workaround.

**Verification:** `match_logs/matrix_<U3-timestamp>.csv` exists. `diff -q match_logs/matrix_<U0-timestamp>.csv match_logs/matrix_<U3-timestamp>.csv` produces no output (files identical). The PR commit body quotes both baseline and post-edit summaries verbatim plus the diff command result.

---

## Key Technical Decisions

- **Migrate to 0.9 and pin there.** Not 0.10. The rand 0.10 changelog removes `Clone` from `StdRng`/`ChaCha*Rng`, which are the seeded types underpinning the matrix runner's replay invariant (4900 reproducible match outcomes per run). `SmallRng` is explicitly documented as non-reproducible across rand versions, so a future contributor reaching for it would silently corrupt determinism. Documenting this ceiling in `Cargo.toml` is the headline product of this PR — the migration is the supporting action. (see origin: ideation idea #2 rationale)
- **Comment lives above the `rand` line in `Cargo.toml`, not in a separate doc.** Rationale: a `Cargo.toml` comment is the highest-visibility location for future contributors and LLM agents — anyone touching dependencies reads `Cargo.toml`. A separate `docs/solutions/` entry could be missed; a comment in the file being edited cannot. The plan's per-dependency upgrade tracks artifact (ideation idea #6) is the right place for a longer policy doc; this PR delivers the most-likely-to-be-read minimum.
- **Verification mirrors PR #54's proven shape.** U0 → U1 → U2 → U3 with byte-identity CSV diff as the merge gate. Rationale: PR #54 demonstrated that the seeded matrix runner is a stronger behavioral oracle than `cargo test` alone (4900 matches vs ~240 tests, exercises full combat AI + RNG path). Reusing the shape means the verification methodology is already trusted and the implementer doesn't need to invent a new approach.
- **One PR, one dependency.** Do not bundle other dep work (bevy_egui patch bumps, etc.) into this PR. Rationale: per ideation idea #6 (RCM triage), `rand` is the predictive-maintenance class — touched only with explicit, isolated, well-instrumented changes. Bundling blurs blast radius and undermines the audit value of the byte-identity diff.

---

## System-Wide Impact

- **Combat determinism:** Zero expected impact. ChaCha output is documented byte-stable across 0.8 → 0.9 in the rand book; the matrix runner will produce identical match outcomes pre- and post-edit. U3's byte-identity diff is the proof.
- **AI / LLM-generated code:** Positive impact. Removing the deprecated `.gen()` and `from_entropy()` from the project's call sites means future LLM-generated code referencing this codebase as context will pick up the current API (`.random()` / `from_os_rng()`) rather than reintroducing deprecated names.
- **Future dependency-refresh PRs:** Positive impact. The `Cargo.toml` pin comment forecloses a class of "just bump everything" PR that would otherwise silently take rand to 0.10 and break replay determinism. The comment is the durable artifact.
- **Other code paths:** None. The `GameRng` wrapper at `src/states/play_match/components/resources.rs` encapsulates all `rand` API usage. Callers (`mod.rs:72`, `runner.rs:162`, the in-file `from_entropy()` wrapper at `:32`, the `Default` impl at `:52`) go through `GameRng` and are untouched.

---

## Scope Boundaries

- Bevy / `bevy_egui` upgrades — separate work stream (ideation ideas #1, #3, #7).
- Per-dependency upgrade policy document — separate decision (ideation idea #6). This PR delivers the `Cargo.toml` comment as the minimum-viable artifact for the rand ceiling; the broader RCM triage doc is its own follow-up.
- Byte-identity matrix oracle tooling formalization — separate decision (ideation idea #4). This PR uses the U0/U3 diff manually; building it into a `scripts/diff-matrix.sh` is a follow-up that benefits the whole project, not just this PR.
- CI workflow infrastructure — no `.github/workflows/` exists today; adding one is out of scope.
- Migrating to `rand_chacha` directly (skipping the `rand` crate facade) — out of scope. The `GameRng` wrapper is the project's own encapsulation; deeper insulation is a separate decision.

### Deferred to Follow-Up Work

- (None — all scope deferrals point to separate planned PRs above, not implementation sequencing of this work.)

---

## Dependencies / Assumptions

- The rand book's documented stability of seeded ChaCha output across 0.8 → 0.9 holds. This is the load-bearing assumption — if it doesn't hold, U3 fails the byte-identity diff and the PR doesn't merge. Verifiable: U3 IS the verification.
- `use rand::prelude::*` continues to re-export both `Rng` and `SeedableRng` traits in 0.9. Verifiable: U1's `cargo build --release` either succeeds or surfaces "trait not found" errors that point at the gap.
- The two enumerated call sites at `resources.rs:34` and `:41` are the *only* direct `rand` API usages in `src/`. Verified by grep at plan time (`grep -rn "thread_rng\|gen_range\|\.gen(\|gen_bool\|StdRng\|SmallRng\|from_entropy\|rand::distributions\|use rand" src/`). If `cargo build` surfaces a third site, R7 escalation applies.
- The matrix runner output format (`match_logs/matrix_<timestamp>.{csv,md}`) is unchanged since PR #54. Verifiable at runtime.

---

## Risks

- **rand 0.9 ChaCha output diverges from 0.8 for the same seed (very low probability).** The rand book documents byte-stability across this span; this would contradict that. Detection: U3's CSV diff is non-empty. Mitigation: STOP, do not merge. Likely root cause investigation paths in U3's test scenarios. Rollback: revert the two-line Cargo.toml diff.
- **A transitive crate advances during `cargo update` and is on the determinism path (low probability).** The leaf crates already auto-advanced in PR #54; further `cargo update` in U1 may shift `rand_chacha` or other transitive deps. Detection: U3 CSV diff. The `rand_chacha` algorithm is documented stable; only `SmallRng` is the version-unstable concern, and we don't use `SmallRng`.
- **Hidden rand API surface I didn't enumerate (low probability).** Grep at plan time found only the two enumerated sites. If `cargo build` surfaces another, the PR scope changes — R7 escalation applies; do not silently fix.
- **Cargo.toml comment wording doesn't survive future copy-paste edits (medium probability over the long term).** A future contributor or LLM agent doing wholesale `Cargo.toml` reformat could lose the comment. Mitigation: place the comment immediately above the `rand` line on its own (so a line-by-line edit preserves the relationship). Future hardening (out of scope): a `Cargo.toml` linter or `tests/` check that asserts the comment exists. Per the policy doc deferral (ideation idea #6), the policy doc would be the second line of defense.

---

## Outstanding Questions

### Resolve Before Implementation
(None.)

### Deferred to Implementation
- [Affects U1][Editorial] Exact wording of the `Cargo.toml` pin comment. Implementer chooses within the constraints in U1's Approach (1-2 lines; names the ceiling at 0.9; names the reason as rand 0.10's Clone removal on StdRng/ChaCha*Rng breaking matrix replay; mentions SmallRng non-reproducibility if it fits without bloating the comment).
