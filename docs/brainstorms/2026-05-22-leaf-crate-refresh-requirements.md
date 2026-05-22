---
date: 2026-05-22
topic: leaf-crate-refresh
---

# Leaf-Crate Refresh PR

## Summary

A small dependency-debt hygiene PR: bump `ron` 0.8 → 0.12 in `Cargo.toml` and run `cargo update` to advance `serde`, `serde_json`, `clap`, and `smallvec` within their existing version specs. Headline value is reducing the number of moving dependency variables in future PRs.

---

## Problem Frame

Every dependency in `Cargo.toml` except `bevy`/`bevy_egui` has drifted from the version installed when the project was scaffolded. `serde`, `serde_json`, `clap`, and `smallvec` are caret-spec'd and Cargo will auto-advance them on any future `cargo update`. `ron` is at `0.8` and Cargo treats `0.x` like majors, so `0.8` is pinned to `>=0.8.0, <0.9.0` — it cannot advance without an explicit `Cargo.toml` edit. As a result, the next person (or LLM agent) running `cargo update` for an unrelated reason will see `serde`/`clap`/etc. shift versions but `ron` will not — three or four moving variables to attribute behavior to instead of one. The first dependency PR of the project's life is the right time to flatten that noise floor before either the `rand` migration (idea #2) or any Bevy work (ideas #1, #3, #7) lands. Better RON parse-error spans for AI-edited config files ride along as a side benefit.

---

## Requirements

**Dependency edits**
- R1. Bump `Cargo.toml` line `ron = "0.8"` to `ron = "0.12"`. No other `Cargo.toml` lines change.
- R2. Run `cargo update` so `Cargo.lock` reflects current patch/minor versions of `serde`, `serde_json`, `clap`, and `smallvec` within their existing caret specs.

**Verification**
- R3. `cargo test` passes. The existing suite (`tests/ability_tests.rs` with 20+ `load_abilities()` test cases, `tests/registration_audit.rs`, `tests/headless_tests.rs`, `tests/combat_*.rs`, `tests/decision_trace_*.rs`) exercises the RON load path comprehensively; failure here is a hard stop.
- R4. Run `cargo run --release -- --matrix 100` and compare the resulting summary winrate table to the most recent matrix baseline in `match_logs/`. Differences within normal RNG noise are acceptable; large shifts block merge pending investigation.
- R5. The PR's commit body records the matrix winrate spot-check summary verbatim so reviewers can see what was observed.

**Scope discipline**
- R6. The PR modifies only `Cargo.toml` and `Cargo.lock`. No source-file changes ride along. If `cargo test` surfaces a compatibility break that needs a code edit, scope changes — escalate before merging.

---

## Acceptance Examples

- AE1. **Covers R1, R2.** Given the current `Cargo.toml` and `Cargo.lock`, when the PR is applied and `cargo update` runs, then only `ron = "0.8"` becomes `ron = "0.12"` in `Cargo.toml`, and `Cargo.lock` shows `ron` at `0.12.x` plus updated patch/minor for `serde`/`serde_json`/`clap`/`smallvec` within their existing caret ranges.
- AE2. **Covers R3, R6.** Given the dependency edits, when `cargo test` runs, then every existing test passes without source-file modifications. If a test fails, the PR is paused for diagnosis rather than fixed inline.
- AE3. **Covers R4, R5.** Given a green `cargo test`, when `cargo run --release -- --matrix 100` runs against the current main branch baseline and against the PR branch, then the 7×7 summary winrate table shows no class-pair winrate shift outside normal RNG variance, and the PR commit body quotes both summaries.

---

## Success Criteria

- After merge, `Cargo.toml` has one line changed (`ron` bumped to `0.12`) and `Cargo.lock` reflects refreshed leaf crates within their caret ranges.
- The next dependency PR in this codebase — whether it's the `rand` migration (idea #2) or the Bevy 0.16 hop (idea #1) — touches one moving dependency family instead of two-plus.
- `cargo run --release -- --matrix 100` continues to produce winrates consistent with the recent baseline; no class-pair regressed by more than RNG noise.

---

## Scope Boundaries

- `rand` 0.8 → 0.9 migration plus `Cargo.toml` pin comment — separate PR (ideation idea #2). Kept separate because `rand` is determinism-load-bearing and warrants its own focused review.
- `bevy` and `bevy_egui` upgrades — separate work stream (ideation ideas #1, #3, #7). This PR is explicitly the "non-Bevy" refresh.
- Byte-identity matrix oracle tooling — separate decision (ideation idea #4). This PR uses sanity spot-check only; building the oracle infrastructure is out of scope here even though it would strengthen R4.
- Per-dependency upgrade policy document — separate decision (ideation idea #6). The PR is one execution; the policy is a separate artifact.
- Building CI infrastructure to run `cargo test` / matrix runs on every push — no `.github/workflows/` directory exists today and adding one is not part of this PR.
- Tightening `Cargo.toml` version specs to exact patch versions (`"1.0.220"`, `"4.6.1"`, etc.) — the existing caret specs are the desired shape. If future work decides exact pinning is right, it's its own decision.

---

## Key Decisions

- **Hygiene framing over capability framing.** The headline value is "fewer moving variables in future PRs," not "better RON error spans" or "untagged-enum fix." Rationale: the user picked this framing in dialogue. The error-spans benefit is real but secondary, and the `serde` 1.0.220 untagged-enum fix doesn't actually apply here (the codebase's only `#[serde(untagged)]` is on `RejectionReason` in `src/states/play_match/decision_trace/events.rs`, which serializes to JSONL via `serde_json` rather than parsing through `ron`).
- **Spot-check matrix run, not byte-identity.** Rationale: matches the 1-hour PR scope. Building the byte-identity oracle from ideation idea #4 would push this work to several hours and bootstrap separate infrastructure. The oracle stays a separate decision; this PR uses summary-winrate comparison instead.
- **Do not bundle with `rand` migration.** Rationale: `rand` is the predictive-maintenance dependency (per ideation idea #6's RCM triage thinking) and warrants its own focused PR with its own pin-comment artifact. Mixing it into the hygiene refresh blurs the blast radius.
- **One-line `Cargo.toml` diff is the correct shape.** Rationale: `serde`/`serde_json`/`clap`/`smallvec` are already caret-spec'd, so `cargo update` is sufficient to advance them. Adding explicit version-pin tightening would be a different decision (more rigidity, more future-PR churn) that the user did not pick.

---

## Dependencies / Assumptions

- The `tests/ability_tests.rs` suite covers the `ron::from_str` load path for `abilities.ron` (verified: 20+ `load_abilities()` call sites). Same assumption for `equipment.rs` parsing `items.ron` and `loadouts.ron` — assumed covered transitively through equipment/loadout tests, not separately verified.
- `ron` 0.12 does not require source-code changes for this codebase's RON usage. Basis: the RON files use struct-of-arrays syntax with no byte-string literals (0.9's byte-string format change is the main format-level break in the 0.8→0.12 span), and no source code does exhaustive matches on `ron::value::Number` (0.10's `#[non_exhaustive]` addition is the main API break). Unverified beyond these two checks — if the test suite surfaces an incompatibility, R6 applies (escalate, don't fix inline).
- Recent `match_logs/` contain a matrix baseline suitable for R4's comparison. If no recent baseline exists at PR time, generating one against current `main` before applying the edits is acceptable.

---

## Outstanding Questions

### Resolve Before Planning

(none)

### Deferred to Planning

- [Affects R4][Technical] Which matrix-run baseline is canonical for the spot-check comparison — most recent in `match_logs/`, a specifically-named baseline file, or a fresh run against current `main`? Answerable by checking the `match_logs/` directory contents at execution time.
- [Affects R3][Technical] Does the `equipment.rs` test coverage actually exercise `ron::from_str` against real `items.ron` / `loadouts.ron` files at test time, or are those load paths only exercised via the runtime headless tests? Worth verifying during the PR to confirm the assumption listed under Dependencies.
