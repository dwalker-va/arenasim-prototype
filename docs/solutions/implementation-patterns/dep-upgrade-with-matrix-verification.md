---
title: "Dependency Upgrades: Byte-Identity Matrix Verification"
category: implementation-patterns
tags: [dependencies, cargo, determinism, matrix-runner, verification, ron, rand, bevy]
module: project-wide
symptom: "Need to upgrade a determinism-sensitive Rust dependency without silently breaking seeded combat behavior"
root_cause: "cargo test alone cannot catch RNG-output drift or floating-point divergence across dependency versions"
date: 2026-05-22
---

# Dependency Upgrades: Byte-Identity Matrix Verification

When upgrading a dependency that *could* influence combat output — RNG crates, math libraries, serialization libraries that touch determinism-sensitive paths — the 4900-match seeded matrix runner is a stronger behavioral oracle than `cargo test` alone. This doc captures the U0→U1→U2→U3 pattern proven across PR #54 (`ron` 0.8 → 0.12) and PR #56 (`rand` 0.8 → 0.9 + pin at 0.9).

## When to use this pattern

Use byte-identity matrix verification when **any** of these hold:

- The dep touches `rand` (RNG output), `rand_chacha`, or any other crate in the determinism path
- The dep crosses a `0.x` major boundary (Cargo treats 0.x like majors; multiple boundaries means accumulated breaking surface)
- The dep is widely used in transitive places (touches floating-point math, hash iteration order, etc.)
- The dep is documented as "byte-stable across versions" — verifying the documentation against reality is exactly the point
- Anything else where `cargo test` passing would leave you uncertain whether combat output drifted

## When to skip it

`cargo test` alone is sufficient when:

- The change is a pure lexical rename (e.g., `from_entropy()` → `from_os_rng()`). The compiler is the strongest oracle; matrix would add no information. PR #57 (`GameRng` method rename) is the canonical "skip" case — 4 call sites changed, no behavioral surface touched.
- The change is in code that has no determinism implications (UI, logging, non-RNG render paths)
- The dep is auto-resolved within an existing caret range (`cargo update` for caret-spec'd leaf crates) — but consider matrix anyway if RNG-adjacent

## The pattern

Structure the plan as four implementation units. **U0 must run before U1 — capturing the pre-edit baseline AFTER the Cargo.toml edit is impossible without reverting changes, which defeats the purpose.**

### U0. Capture pre-edit matrix baseline

Run against the **unedited** `Cargo.lock`:

```bash
cargo run --release -- --matrix 100
```

Output lands in `match_logs/matrix_<timestamp>.{csv,md}`. **Record the exact `<timestamp>`** in a scratch note — do not rely on "latest in match_logs/" heuristics. Stale matrix files from prior PRs will be present in the worktree.

Each run takes ~50-55 minutes wall-clock at 4900 matches (per PR #54: 3260s; PR #56: 3525s pre-edit, 3282s post-edit). Run in the background; do other work while it executes.

### U1. Apply the dependency edit

Make the Cargo.toml change. Run `cargo update`. Verify `cargo build --release` passes. **Do not touch source files outside the bare minimum the API rename requires** — scope discipline keeps the byte-identity check meaningful (otherwise an unrelated change could mask a determinism regression).

If the build surfaces an unexpected API drift in an unenumerated call site, **escalate** rather than fix inline. The plan's scope contract (R7 in PR #54 and #56) is load-bearing.

### U2. Run `cargo test --release`

The seeded-determinism tests in `tests/headless_tests.rs` (`seeded_matches_are_deterministic`, `trace_file_is_deterministic_at_same_seed`, `different_seeds_produce_different_matches`) catch single-match drift. A green `cargo test` is a *necessary* signal but not *sufficient* — they exercise the determinism contract on a small sample of seeds and class pairs. The matrix runner exercises 49 pairs × 100 seeds.

### U3. Post-edit matrix run + byte-identity diff

Run `cargo run --release -- --matrix 100` again. Then:

```bash
diff -q match_logs/matrix_<U0-timestamp>.csv match_logs/matrix_<U3-timestamp>.csv
```

**Expected output: empty (files byte-identical).** The MD files will differ only on the wall-clock timestamp line — that's measurement noise.

If `diff -q` produces output, **stop**. Likely root causes (descending probability):

1. A transitive crate also advanced and is on the determinism path. Inspect the `Cargo.lock` diff for non-target version moves.
2. A hidden code path uses a now-changed API and the rename/migration changed semantics in a way the compiler didn't catch.
3. The crate's "byte-stable" guarantee doesn't actually hold for the exact version combination in use.

Investigate each before assuming the upgrade is OK. Do not modify tests to accept the new output.

## Caveats

### Stale match_logs/ files

`match_logs/` is gitignored, so prior PRs' matrix files accumulate locally. PR #56's review caught this — the plan claimed "match_logs is empty" but PR #54 had left two pairs behind. Mitigation: always record the U0 timestamp explicitly; reference it specifically in U3's `diff -q` command rather than "most recent" heuristics.

### Dual-version transitive crates

Each leaf-crate upgrade may add a duplicated transitive version when Bevy 0.15 pins the old version. PR #54 left `ron` 0.8.1 (via `bevy_animation` + `bevy_asset`) coexisting with `ron` 0.12.1 (direct). PR #56 did the same with `rand` 0.8.6 / `rand` 0.9.4. This is correctness-safe (Cargo's version namespacing prevents collision) but produces a slight binary-size cost. The duplications self-resolve when Bevy itself upgrades.

### The matrix runner finds drift that `cargo test` misses

`cargo test` exercises ~240 tests across the whole codebase; seeded-determinism tests are a small subset. The matrix runner exercises the full combat AI + RNG + serialization path 4900 times. Both proven runs (PR #54 and #56) produced byte-identical CSVs against their pre-edit baselines — strong evidence the upgrade was truly behavior-preserving. The matrix is the merge gate; tests are an early-fail signal.

### Wall-clock cost

~50-55 min per run, twice per PR (U0 baseline + U3 verification). Total ~110 min per dep upgrade. Run the bigger ones in the background while doing other work. The cost is real but the alternative — shipping a silent determinism regression that only surfaces months later as a "why don't seeded matches replay anymore?" bug report — is much worse.

## Anti-pattern: skipping U0 because match_logs/ "has files"

PR #54 hit this trap: prior matrix files exist, but they were generated against a *different* `Cargo.lock` (an earlier point in the upgrade sequence). Diffing against an older baseline conflates the current upgrade's effects with earlier upgrade batches'. **Always generate a fresh U0 baseline against the exact pre-edit `Cargo.lock` of the current PR.**

## Related

- `tests/headless_tests.rs` — seeded-determinism tests that fire on every `cargo test`
- `src/states/play_match/components/resources.rs` — `GameRng` wrapper encapsulating all `rand` API surface
- `src/headless/matrix.rs` — the matrix runner implementation
- PR #54 — `ron` 0.8 → 0.12 leaf-crate refresh (proved the pattern with byte-identical CSV)
- PR #56 — `rand` 0.8 → 0.9 + pin at 0.9 (proved ChaCha output stability across rand version boundary)
- PR #57 — `GameRng::from_entropy()` → `from_os_rng()` rename (canonical "skip the matrix" case)

## Modern prevention

There's no `tests/`-level enforcement of this pattern yet. The discipline lives in `ce-plan`'s habit of writing U0→U1→U2→U3 plans for determinism-sensitive dep work. A future hardening would be a `scripts/diff-matrix.sh` helper that takes two timestamp arguments and produces the diff with explanatory output, or a CI workflow that captures baseline + post-edit matrix runs on dep-bump PRs automatically. Both are ideation backlog (idea #4 from `docs/ideation/2026-05-22-library-upgrades-ideation.md` covers the CI angle).
