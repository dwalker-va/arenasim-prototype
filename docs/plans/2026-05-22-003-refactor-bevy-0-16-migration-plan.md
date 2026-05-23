---
date: 2026-05-22
type: refactor
title: Migrate Bevy 0.15 → 0.16 with bevy_egui in lockstep
status: completed
origin: docs/ideation/2026-05-22-library-upgrades-ideation.md
---

# Bevy 0.15 → 0.16 with bevy_egui in lockstep

## Summary

Bump `bevy` 0.15 → 0.16 and `bevy_egui` 0.31 → 0.34.1 in lockstep. The actual migration surface is roughly 40 mechanical line edits — much smaller than the ideation's "~239 idioms" suggested, because the codebase already adopted several 0.16-friendly patterns. The headline strategic gain is **ecosystem reach**: 0.16 is the gating version for most current Bevy community crates (avian/rapier physics, hanabi, leafwing-input-manager, modern inspectors). This PR is the engine migration only — the AssetChanged hot-reload capability win that the ideation flagged is a separate feature PR (it requires a custom Bevy Asset loader, different scope).

## Problem Frame

Bevy 0.15 is now three minor versions behind (0.18 is current as of January 2026). Every quarter on 0.15 silently shrinks the addressable Bevy crate landscape — most community crates have dropped sub-0.16 support during 2025. The deliberate January 2026 `bevy_egui` investment (`design-docs/egui-migration-summary.md`) is currently being undermined by `bevy_egui = 0.31`'s lock to Bevy 0.15. The right move is one hop forward to 0.16 now: clears `ron 0.8.1` and `rand 0.8.6` transitive duplicates left over from PRs #54 and #56, unlocks the next layer of ecosystem crates, and proves the migration methodology against a meaningfully larger dep change before contemplating 0.16 → 0.17 → 0.18. The migration cost is smaller than naive estimates suggest because the codebase has already absorbed the 0.16-flavored patterns that mattered most: zero `.single()` panics (9 `get_single()` sites already handle `Result`), uniform `despawn_recursive` (one mechanical rename to `despawn()`), `try_insert` over `insert`, `Res<Time>` discipline. (see origin: `docs/ideation/2026-05-22-library-upgrades-ideation.md` survivor #1)

## Requirements

- R1. `Cargo.toml`: bump `bevy = "0.15"` → `bevy = "0.16"` (jpeg feature preserved) AND `bevy_egui = "0.31"` → `bevy_egui = "0.34.1"`. Both in the same edit — bevy_egui 0.31 won't compile against Bevy 0.16 and 0.34.1 won't compile against 0.15.
- R2. Rename 9 `get_single()` / `get_single_mut()` call sites to `single()` / `single_mut()`. The `Result` return type is unchanged in 0.16 (was already `Result` via `get_single`); call-site bodies handling `Ok(...)` / `let Ok(...) else { return; }` stay identical. (see "Already adopted" — codebase has zero `.single()` calls today; this is pure rename.)
- R3. Rename 2 `EventWriter::send(...)` calls to `EventWriter::write(...)`. Both sites use `EventWriter<AppExit>`.
- R4. Update the `EguiPlugin` instantiation at `src/main.rs:122` to use struct-literal form `EguiPlugin { enable_multipass_for_primary_context: false }` (preserves current single-pass behavior). Only 1 instantiation site — verified by grep; the `use bevy_egui::{..., EguiPlugin}` at `src/main.rs:8` is an import, not an instantiation.
- R5. Rename 28 `despawn_recursive()` calls to `despawn()` (0.16 makes recursion the default; semantics unchanged for these call sites).
- R6. The 2 `on_remove`/`on_replace`/`on_add`/`on_insert` grep hits are confirmed (at plan time) to be test function names in `src/states/play_match/equipment.rs:864` (`apply_equipment_weapon_replaces_damage_for_melee`) and `:882` (`apply_equipment_weapon_replaces_damage_for_ranged`) — both inside `#[cfg(test)]`, neither is a Bevy ComponentHook signature. **No edit needed.** R6 is closed at plan time.
- R7. `cargo build --release` compiles cleanly after the dep bump + R2-R5 edits, with no source-file changes beyond what the migration guide enumerates. **If `cargo build` surfaces any API beyond R2-R5 — including but not limited to `Parent`/`Children`, `with_children` closures, `EntityCommand` impls, `UiImage`, `Pointer<`, `#[require(...)]` syntax, `weak_from_u128`, `ComponentHook`, or any `AssetChanged` / `AssetLoader` / asset-pipeline surface — STOP and surface for review.** Do not patch inline; the U3 verification's signal depends on R2-R5 being the only source changes. AssetLoader specifically is the hot-reload feature deferred to a separate PR (see Scope Boundaries) — encountering it means the plan's boundary is being crossed.
- R8. `cargo test --release` passes. Specifically `tests/registration_audit.rs` must continue to flag unregistered systems — Bevy 0.16 changes the SystemParam derive surface in places, so the audit's signature-based matcher needs explicit re-validation rather than rubber-stamp.
- R9. Capture pre-edit matrix baseline BEFORE the Cargo.toml edit.
- R10. Run post-edit matrix and apply statistical-equivalence acceptance criteria (R11) instead of byte-identity. Byte-identity is not expected; Bevy 0.16 changes internal entity iteration order, ECS scheduler ordering between independent systems, and floating-point math (11× faster Transform propagation likely means SIMD path differences).
- R11. **Statistical equivalence acceptance criteria** (verification gate for R10):
  - Per-class-pair winrate shift ≤ ±5% (RNG-noise band at N=100 matches per cell)
  - No class-pair flipping winner direction (Mage vs Hunter stays Mage-favored; Rogue vs Paladin stays Paladin-favored; etc.)
  - Aggregate Team 1 / Team 2 / Draw totals shift ≤ ±2%
  - Mirror matchups stay within ±10% of their pre-edit baseline (mirrors are intrinsically noisier — Bevy iteration order and scheduler changes are more likely to perturb self-play than asymmetric matchups). The U0 baseline establishes the 2026-05-22 pre-edit reference; historical context from PR #56's matrix showed Mage mirror at 19% T1 / 64% draw, which is the kind of cell where modest drift is expected and large drift would be alarming.
  - **If ANY criterion is exceeded, STOP** and surface for review. Real regression vs accepted drift is a judgment call; do not silently merge "newer Bevy = newer outcomes."

## Requirements Traceability

| Plan U-ID | Requirements | What it covers |
|---|---|---|
| U0 | R9 | Capture pre-edit matrix baseline (4900 matches against current Cargo.lock) |
| U1 | R1, R2, R3, R4, R5, R7 | Cargo.toml edit + cargo update + mechanical renames + cargo build clean (R6 closed at plan time — no implementation work) |
| U2 | R8 | `cargo test --release` passes; registration audit re-validated |
| U3 | R10, R11 | Post-edit matrix run + statistical-equivalence diff against U0 baseline |

## High-Level Technical Design

This illustrates the intended approach and is directional guidance for review, not implementation specification. The implementing agent should treat it as context, not code to reproduce.

```
Migration sequence (single PR, four implementation units):

  U0 ────────► U1 ────────► U2 ────────► U3
  (~55 min)   (~30 min)    (~5 min)    (~55 min)
  baseline     edit +       test       post-edit
  matrix       renames       suite      matrix +
                                         stat-eq diff

Per-unit verification gate (must pass before next unit):
  U0 → matrix_<timestamp>.{csv,md} exists, 4900 matches complete
  U1 → cargo build --release passes
  U2 → cargo test --release passes (240+ green); audit honored
  U3 → all four R11 criteria met (or surfaced)

Wall-clock budget: ~3 hours (mostly U0 + U3 matrix runs in background)
```

## Implementation Units

### U0. Capture pre-edit matrix baseline

**Goal:** Produce the canonical pre-edit matrix-run output so U3 has a concrete reference point for statistical-equivalence comparison. Without this, U3 has nothing to diff against.

**Requirements:** R9 (prerequisite for U3's comparison)

**Dependencies:** none — runs against current (unedited) `Cargo.lock`

**Files:** No source edits. Generates `match_logs/matrix_<U0-timestamp>.csv` and `.md` per `src/cli.rs:53`. Files are gitignored (per project convention); the PR commit body records the U0 timestamp and the summary content verbatim.

**Approach:** Run `cargo run --release -- --matrix 100`. **Record the exact `<U0-timestamp>` explicitly** in a scratch note — `match_logs/` already contains stale files from PRs #54 (`matrix_1779473428`, `matrix_1779476958`), #56 (`matrix_1779482011`, `matrix_1779485752`), and possibly others. Do not rely on "most recent file" heuristics in U3; reference the U0 timestamp specifically.

**Patterns to follow:** PR #54 / PR #56 U0 step; methodology in `docs/solutions/implementation-patterns/dep-upgrade-with-matrix-verification.md`.

**Test scenarios:** Not feature-bearing. `Test expectation: none -- baseline-capture step, no behavior to test.`

**Verification:** `match_logs/matrix_<U0-timestamp>.{csv,md}` exists with a complete 7×7 summary table and 4900 trace coverage. U0 timestamp captured for U3 reference.

---

### U1. Apply dep bump + mechanical renames + verify compile

**Goal:** Bump `bevy` and `bevy_egui` together; rename API surfaces enumerated by R2-R6; verify `cargo build --release` passes. This is the largest unit (~40 line edits across multiple files) but every edit is mechanical or compile-driven.

**Requirements:** R1, R2, R3, R4, R5, R6, R7

**Dependencies:** U0 (baseline must exist before edits)

**Files:**
- `Cargo.toml` — bump `bevy = "0.15"` to `bevy = "0.16"` (preserve `features = ["jpeg"]`) and `bevy_egui = "0.31"` to `bevy_egui = "0.34.1"`
- `Cargo.lock` — regenerated by `cargo update` (gitignored)
- `get_single()` / `get_single_mut()` renames (9 sites — pure rename, callers handle `Result` unchanged):
  - `src/settings.rs:236` — `windows.get_single_mut()`
  - `src/camera/mod.rs:96` — `camera_query.get_single_mut()`
  - `src/states/play_match/selection.rs:110, 113` — `cameras.get_single()`, `windows.get_single()`
  - `src/states/play_match/camera.rs:122, 182` — `windows.get_single()`, `camera_query.get_single_mut()`
  - `src/states/play_match/rendering/effects.rs:48, 276` — `camera_query.get_single()` (twice)
  - `src/states/play_match/rendering/hud.rs:205` — `camera_query.get_single()`
- `EventWriter::send()` → `write()` renames (2 sites — call locations, not declarations):
  - `src/states/mod.rs:429` — `.send(AppExit::Success)` call inside `main_menu_ui` (declaration at line 329)
  - `src/headless/runner.rs:499` — `.send(AppExit::Success)` call inside `headless_exit_on_complete` (declaration at line 497)
- `EguiPlugin` instantiation update (1 site only):
  - `src/main.rs:122` — currently `EguiPlugin` (unit-struct form, inside `.add_plugins((...))` tuple); change to struct-literal `EguiPlugin { enable_multipass_for_primary_context: false }` preserving the trailing comma.
- `despawn_recursive()` → `despawn()` renames (28 sites across `match_flow.rs`, `projectiles.rs`, `rendering/effects.rs`, `auras.rs`, `selection.rs`, `shadow_sight.rs`, `mod.rs` etc. — full list via `grep -rn 'despawn_recursive' src/`)
- R6's 2 `on_*` grep hits are confirmed test function names in `equipment.rs:864, :882` (no edit needed; see R6).

**Approach:**
1. **Edit `Cargo.toml`** first: bump both `bevy` and `bevy_egui` lines.
2. **Run `cargo update`**: this refreshes `Cargo.lock` for Bevy 0.16's transitive graph. Inspect the diff — `ron 0.8.1` and `rand 0.8.6` may drop out (they were pinned by Bevy 0.15's ecosystem). Capture the resolved versions for the commit body.
3. **Run `cargo build --release` once**: collect the first wave of compile errors. The errors guide the rename work — each one points at a specific call site.
4. **Apply mechanical renames in order**: `get_single` → `single`, `.send(` → `.write(` on EventWriters, `despawn_recursive(` → `despawn(`. After each batch, re-run `cargo build --release` to confirm the wave shrinks.
5. **Add `EguiPlugin` field**: locate the 2 sites; add `enable_multipass_for_primary_context: false`. Choose `false` rather than `true` to preserve current single-pass behavior — opting into multi-pass is its own decision.
6. **Build must end clean**. Any error beyond R2-R5's enumerated renames is an unexpected surface — surface to the user before patching inline (R7 escalation). **Specifically: if any `AssetChanged`, `AssetLoader`, or asset-pipeline API appears in compiler output, STOP — that is the hot-reload feature PR boundary, not this migration.**

**Patterns to follow:** Bevy 0.16 migration guide (https://bevy.org/learn/migration-guides/0-15-to-0-16/); bevy_egui CHANGELOG 0.32, 0.33, 0.34 entries (https://docs.rs/crate/bevy_egui/latest/source/CHANGELOG.md).

**Test scenarios:**
- After all edits, `cargo build --release` exits 0. No source-file changes beyond R2-R6.
- `Cargo.lock` diff shows `bevy 0.15 → 0.16` and `bevy_egui 0.31 → 0.34.1` direct upgrades, plus expected transitive shifts. `ron 0.8.1` and `rand 0.8.6` transitive duplicates from Bevy 0.15 may or may not still appear (depends on which dep pulled them); record the result for the commit body.
- Only the enumerated files (Cargo.toml + the ~7-10 source files with renames) are modified per `git status`. Any other modified file is a sign of unexpected API drift — escalate before patching.

**Verification:** `cargo build --release` succeeds. `git diff --name-only` lists only Cargo.toml + the source files explicitly named in the Files section above (no Cargo.lock since gitignored).

---

### U2. `cargo test --release` passes; registration audit re-validated

**Goal:** Confirm the test suite passes under Bevy 0.16 + bevy_egui 0.34.1, and explicitly verify that `tests/registration_audit.rs` still catches unregistered systems (the audit's signature-based matcher could silently relax if Bevy 0.16 shifts the SystemParam derive surface).

**Requirements:** R8

**Dependencies:** U1

**Files:**
- No source edits expected. This unit invokes:
  - `tests/registration_audit.rs` (the dual-registration audit — most-likely-to-flex test)
  - `tests/headless_tests.rs` (seeded-determinism + match-result tests)
  - `tests/decision_trace_audit.rs`, `tests/decision_trace_builder.rs` (serde + Bevy Resource serialization)
  - `tests/ability_tests.rs`, `tests/combat_*.rs`, `tests/cast_guard_tests.rs`, `tests/class_ai_decisions.rs` (full coverage of combat logic + RON config load + serde derive)

**Approach:**
1. Run `cargo test --release`. Expect 240+ passed, 0 failed (PR #54 / #56 baseline shape).
2. **Registration audit re-validation step (~5 minutes, 3 commands)**: after `cargo test` passes, add a one-line `pub fn audit_canary(_cmds: Commands) {}` to `src/states/play_match/match_flow.rs` (or any other file under `play_match/` not yet registered — `match_flow.rs` is a good pick because it already imports `Commands`). Run `cargo test --release registration_audit`. **Success criterion: the audit fails and its failure message names `audit_canary` as a SystemParam-taking pub fn not in the registration allowlist.** If the audit passes — meaning the matcher no longer detects the unregistered function — Bevy 0.16's SystemParam derive surface has silently relaxed the matcher; STOP and investigate before proceeding. If the audit fails as expected, revert via `git checkout -- src/states/play_match/match_flow.rs`. The specific error wording may differ from pre-0.16 versions; what must be present is the function-name identification.
3. If `cargo test --release` fails on its own (not the deliberate audit test), the failure is a real Bevy 0.16 semantic change beyond the mechanical renames in U1. STOP and investigate — do not modify tests to accept new output. Likely categories:
   - `tests/headless_tests::seeded_matches_are_deterministic` fails → ECS iteration order change or RNG plumbing shifted
   - `tests/decision_trace_audit` fails → trace JSONL format shifted (Bevy Resource serialization changed)
   - `tests/registration_audit` fails on existing functions → Bevy 0.16's SystemParam trait surface changed; matcher needs an update (deliberate fix, not a relaxation)
   - Other test failures → real regression; surface for review

**Patterns to follow:** Existing `cargo test --release` workflow per `CLAUDE.md`. PR #54 / #56 baseline.

**Test scenarios:**
- Given U1's edits, `cargo test --release` exits 0 with all suites green (target: 240+ passed, 0 failed, ~3 ignored matching PR #56's baseline).
- Registration audit re-validation: a deliberately-added unregistered `pub fn` with `Commands` parameter causes `tests/registration_audit.rs` to fail. After reverting the deliberate test function, `cargo test --release` returns to 0.
- Specifically: `tests/headless_tests::seeded_matches_are_deterministic` and `trace_file_is_deterministic_at_same_seed` pass — seeded RNG plumbing through Bevy Resources still produces deterministic per-seed output.

**Verification:** `cargo test --release` exits 0. Audit re-validation step proves the audit still catches unregistered systems. No test modifications.

---

### U3. Post-edit matrix run + statistical-equivalence diff against U0

**Goal:** Prove via 4900-match matrix that Bevy 0.16's combat behavior stays within acceptable statistical bounds of 0.15. Byte-identity is not expected (per the methodology doc's caveat for this case); statistical equivalence per R11 is the merge gate.

**Requirements:** R10, R11

**Dependencies:** U2 (and U0 for the baseline)

**Files:** No source edits. Generates `match_logs/matrix_<U3-timestamp>.{csv,md}` (gitignored). PR commit body records both U0 and U3 summaries verbatim plus the diff analysis.

**Approach:**
1. Run `cargo run --release -- --matrix 100` against the post-edit `Cargo.lock`. Capture the `<U3-timestamp>`.
2. **First check: `diff -q match_logs/matrix_<U0>.csv match_logs/matrix_<U3>.csv`**. If byte-identical (empty output), we got lucky — Bevy 0.16 didn't perturb anything determinism-relevant in this codebase. Note in the PR body, treat as a positive surprise, proceed to merge. (Probability: low but not zero — codebase already uses BTreeMap for combat state and ChaCha for RNG.)
3. **If diff produces output, apply R11 statistical-equivalence criteria**:
   - Open both `.md` files side by side. Compare each cell of the 7×7 winrate table.
   - For each class-pair cell: compute `|post - pre|`. Must be ≤ 5% (5 percentage points at N=100).
   - For each class-pair cell: verify winner direction unchanged. If pre-edit was Mage 100% / Hunter 0%, post-edit may shift to Mage 95% / Hunter 5% (within band) but NOT to Mage 60% / Hunter 40% (direction preserved but >5%) or Hunter winning (direction flipped — STOP).
   - Compute aggregate Team 1 / Team 2 / Draw totals from both files. Must each shift ≤ 2%.
   - Mirror matchups: ±10% band (mirrors are intrinsically noisier).
4. **Document the diff in the PR body**. Even if all R11 criteria pass, the diff data is the strongest evidence of "what changed and what didn't." Quote both `.md` summary tables verbatim and call out specific cells that moved more than 1% (informational, not blocking).
5. **If R11 criteria are exceeded, STOP**. Do not silently accept "newer Bevy = newer outcomes." Surface to the user with: which cells failed, by how much, possible root causes (likely: ECS iteration order, system execution order between independent systems, floating-point math drift). The user decides whether the drift is acceptable, needs investigation, or needs the migration backed out.

**Patterns to follow:** Methodology in `docs/solutions/implementation-patterns/dep-upgrade-with-matrix-verification.md` — specifically the "If diff -q produces output, stop" section, adapted here for statistical equivalence rather than byte-identity.

**Test scenarios:**
- Given green U2, when `cargo run --release -- --matrix 100` runs, then a complete 7×7 summary is generated.
- When diff'd against U0: ideally byte-identical (lucky case, document as positive surprise). Otherwise: all R11 criteria pass (every per-pair shift ≤5%, no direction flips, aggregate shifts ≤2%, mirror shifts ≤10%).
- If R11 exceeded on any criterion: STOP, do not merge, surface specifics to user.

**Verification:** `match_logs/matrix_<U3-timestamp>.csv` exists. Either `diff -q` produces no output (byte-identical), OR all R11 statistical-equivalence criteria pass with the diff documented in the PR commit body. No silent acceptance of drift.

---

## Key Technical Decisions

- **bevy_egui 0.34.1, not 0.33 and not 0.35+.** 0.34.1 is the highest 0.3x release that pairs with Bevy 0.16 AND stays pre-camera-attachment refactor (0.35 attaches `EguiContext` to cameras instead of windows — that's its own breaking change). Picking 0.34.1 keeps this PR focused on Bevy core; the 0.35 egui refactor is a separate decision later. (External research: bevy_egui CHANGELOG, 0.34.0 entry says "Update Bevy to 0.16".)
- **One PR, both deps together.** bevy_egui 0.31 won't compile against Bevy 0.16; bevy_egui 0.34.1 won't compile against Bevy 0.15. They must move together. No way to stage as separate PRs.
- **Choose `enable_multipass_for_primary_context: false`** for the new `EguiPlugin` field. Preserves current single-pass behavior. Opting into multi-pass is a separate decision; this PR is about engine migration, not about exploiting new egui features.
- **Keep `despawn_recursive()` rename in scope.** It's a 28-site mechanical rename matching the codebase's existing convention (uniform usage). The old API may still exist as a deprecated alias in 0.16, but consistency with the new default is cleaner; leaving 28 deprecation warnings would be its own cleanup burden later.
- **Statistical equivalence as verification, not byte-identity.** Bevy 0.16's documented internal changes (Transform propagation 11× faster, ECS iteration order, observer-before-hook ordering) make byte-identity unlikely. R11's specific thresholds (≤5% per-pair, no direction flips, ≤2% aggregate) are calibrated for "real regression vs ordinary RNG/scheduler-ordering drift at N=100 matches." Tighter thresholds would fail on noise; looser would miss real regressions.
- **Audit re-validation step is mandatory.** `tests/registration_audit.rs` detects unregistered systems by `pub fn` signature matching SystemParam types. Bevy 0.16 changes some SystemParam derive surface. If the matcher silently relaxes, the bug class the audit prevents (PR-era `process_dispels` etc.) returns. Deliberate test of "does the audit still catch a planted unregistered function?" is the only way to verify.

## System-Wide Impact

- **Combat determinism:** Possibly perturbed. Bevy 0.16 changes Transform propagation (11× faster — likely SIMD), entity iteration order, and observer-before-hook ordering. The codebase has already hardened against HashMap iteration (BTreeMap migration done) and seeded RNG (ChaCha via rand 0.9), but Transform propagation hits every entity every frame. U3 statistical-equivalence check is the verification gate. If matrix winrates shift outside R11 thresholds, the migration backs out.
- **Decision-trace JSONL output:** Possibly format-shifted. `tests/decision_trace_audit.rs` and `tests/decision_trace_builder.rs` exercise the `#[serde(untagged)]` `RejectionReason` serialization via Bevy `Resource`. Bevy 0.16 may change Resource implementation in subtle ways that surface as JSONL format drift. `cargo test` is the detection surface.
- **bevy_egui UI:** Configuration screen + decision-trace viewer + any debug overlays. Going from bevy_egui 0.31 → 0.34.1 spans three minor versions. Most changes are additive (multi-pass support is opt-in). The `EguiSettings` → `EguiContextSettings` rename does not apply (codebase has zero `EguiSettings` references).
- **Registration audit:** Will be re-validated in U2 step 2. If the audit's signature matcher needs updating for 0.16's `SystemParam` derive surface, that's a deliberate audit-test fix (not a relaxation), and counts as in-scope work because R8 explicitly requires audit re-validation.
- **Ecosystem reach:** Positive. Post-merge, the project can adopt Bevy 0.16-compatible community crates (avian/rapier physics, hanabi particles, leafwing-input-manager, modern inspectors). None planned for this PR but unlocked.
- **Transitive deduplication:** Likely positive. Bevy 0.15 pinned `ron 0.8.1` and `rand 0.8.6` transitively via `bevy_animation` + `bevy_asset`. Bevy 0.16 may advance those pins or release them entirely. Cargo.lock diff in U1 step 2 captures the actual outcome.

## Scope Boundaries

- **Out of scope: AssetChanged hot-reload feature.** This is the ideation's "headline capability win" — but it requires implementing a custom Bevy `AssetLoader` for RON files, registering the loader, and migrating `abilities.ron` / `items.ron` / `loadouts.ron` from `ron::from_str` parsing to Bevy's asset pipeline. That's its own feature with its own design surface (Asset vs Resource lifecycle, hot-reload event handling, validation timing) and merits a separate PR after this migration lands.
- **Out of scope: Bevy 0.16 → 0.17 → 0.18 further upgrades.** Ideation idea #7 (dual-registration audit revisit) depends on Bevy 0.17's SystemSet renames and Reflect auto-registration. Defer until this PR is stable on main.
- **Out of scope: bevy_egui 0.35+ camera-attachment refactor.** The 0.35 release attaches `EguiContext` to cameras instead of windows — that's its own breaking change that needs adapter code on the project side. Defer to a follow-up.
- **Out of scope: per-dependency upgrade policy doc** (ideation idea #6).
- **Out of scope: M1-style follow-up renames if any surface during migration.** If U1 reveals a `GameRng::from_os_rng()`-style naming-mismatch issue, capture it for a separate PR.
- **Out of scope: opting into Bevy 0.16's new opt-in features** (Required Components syntax, multi-pass egui, Reflect auto-registration if exposed in 0.16). Each is a separate decision.

### Deferred to Follow-Up Work

- AssetChanged hot-reload feature PR (post-merge follow-up)
- bevy_egui 0.35+ migration PR
- Bevy 0.17+ migration PRs
- New ecosystem crate adoption PRs (each crate is its own decision)

## Dependencies / Assumptions

- bevy_egui 0.34.1 is the right pairing for Bevy 0.16. Verified via the bevy_egui CHANGELOG: "Update Bevy to 0.16" appears explicitly in the 0.34.0 entry. 0.34.1 is the bugfix release that also targets Bevy 0.16.
- The codebase has zero `Parent` / `Children` / `with_children` / `EntityCommand` / `Pointer<` / `UiImage` / `TargetCamera` / `Volume()` / `#[require]` / `weak_from_u128` / `ComponentHook` / `.many()` / `.to_readonly()` / `trigger.entity` references (verified by grep at plan-write time). If any of those hits appear during `cargo build` they're an unexpected surface — R7 escalation applies.
- The 2 `on_remove/on_replace/on_add/on_insert` grep hits are likely unrelated method names (combat callbacks like `on_remove_aura`). U1 step 6 confirms during implementation.
- The Bevy 0.16 transitive graph drops `ron 0.8.1` and `rand 0.8.6` (or at least one of them) — Bevy 0.15 was the primary source of those duplications. Verifiable via `Cargo.lock` diff in U1.
- The U0 baseline is the canonical reference for U3's diff. `match_logs/` contains stale files from prior PRs; explicit timestamp tracking is mandatory.
- Wall-clock cost: ~3 hours total (U0 ~55 min + U1 ~30 min + U2 ~5 min + U3 ~55 min). U0 and U3 can run in the background.

## Risks

- **Matrix winrate drift exceeds R11 thresholds (medium probability).** Bevy 0.16's documented changes (Transform propagation rewrite, ECS iteration order, observer-before-hook ordering) could plausibly shift combat outcomes outside the ±5% / no-direction-flip / ±2% aggregate band. Detection: U3 diff analysis. Mitigation: STOP, surface to user with specifics. The methodology doc covers root-cause descent. Rollback: revert the migration commit and re-plan with tighter understanding of which Bevy change is causing the shift.
- **Registration audit silently relaxes (low probability, high impact).** If Bevy 0.16 changes how `SystemParam` is declared/derived and the audit's pattern matcher stops matching, the dual-mode silent-failure bug class (PR-era `process_dispels` etc.) returns invisibly. Detection: U2 step 2 deliberate-unregistered-function test. Mitigation: update the audit matcher for the new SystemParam surface (not a relaxation — a deliberate fix that re-establishes the bug-prevention contract).
- **Unexpected API drift surfaces in U1 (medium probability).** The grep at plan time was thorough but not exhaustive. If `cargo build` reveals a Bevy API I didn't enumerate (e.g., a less-common API surface), R7 escalation applies. Most likely candidates: `bevy_math` reflect feature (now non-default), `Mesh::merge()` Result return, hierarchy command struct removals. Mitigation: surface for review rather than fix inline — scope discipline preserves the U3 verification's signal.
- **`bevy_egui` 0.34.1 has unexpected behavior diff from 0.31** (low probability). The CHANGELOG calls out multi-pass support (opt-in via the new field, defaulted off) and `EguiSet` split (only matters if `.before(EguiSet::...)` is used anywhere — grep returns zero hits in the codebase). Detection: ConfigureMatch screen still works after migration; decision-trace viewer still works. Mitigation: visual spot-check during U2.
- **Transitive crates that ARE on the determinism path advance during cargo update** (low probability). Possible candidates: `rand_chacha`, `glam` (math). Detection: U3 matrix diff. Mitigation: methodology doc's root-cause-descent procedure.

## Outstanding Questions

### Resolve Before Implementation
(None.)

### Deferred to Implementation
- [Affects U3][Technical] Will Bevy 0.16's Transform propagation rewrite (11× faster, likely SIMD) shift seeded matrix output? No way to predict; U3 IS the answer. If it shifts but stays within R11 thresholds, accept. If it exceeds, STOP and investigate.
