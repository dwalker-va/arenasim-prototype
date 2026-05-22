---
date: 2026-05-22
topic: library-upgrades
focus: assess benefits of upgrading the project's major Cargo dependencies
mode: repo-grounded
---

# Ideation: Library Upgrades — Benefit Assessment

## Grounding Context

**Current versions (`Cargo.toml`):** bevy 0.15 (jpeg), bevy_egui 0.31, serde 1.0, serde_json 1.0, ron 0.8, rand 0.8, clap 4.4, smallvec 1.13.

**Codebase coupling:** ~239 Bevy idiom call sites across ~70 source files. Dual-path system registration enforced by `tests/registration_audit.rs` (headless `add_core_combat_systems()` + graphical `StatesPlugin::build()`). Seeded 7×7 matrix runner is load-bearing for replay determinism — 4900 trace JSONL files per matrix run, BTreeMap migration done. The deliberate January 2026 `bevy_egui` migration (`design-docs/egui-migration-summary.md`) eliminated 8 UI bugs and dropped 968→240 LoC in ConfigureMatch — that bet is currently held back by `bevy_egui = 0.31`'s lock-step with Bevy 0.15. No prior captured library-upgrade learnings in `docs/solutions/`. AI-native development workflow — Claude is the dominant contributor.

**External version landscape (May 2026):**
- **Bevy 0.16** (Apr 2025): `Query::single()` → `Result`; `EventWriter::send()` → `write()`; `Parent` → `ChildOf`; `despawn()` recursive by default; `AssetChanged` query filter; transform propagation 11× faster.
- **Bevy 0.17** (Sep 2025): `EventWriter` → `MessageWriter`; render module split (`bevy_camera`/`bevy_shader`/`bevy_light`/`bevy_mesh`); `RenderSet`/`UiSystem`/`TransformSystem` renamed with `Systems` suffix; `StateScoped` → `DespawnOnExit`; Reflect auto-registration.
- **Bevy 0.18** (Jan 2026): Incremental; required for `bevy_egui` 0.39.
- **`bevy_egui` lock table:** 0.31–0.32 → Bevy 0.15; 0.33+ → 0.16+; 0.39 → 0.18. Most Bevy ecosystem crates dropped sub-0.16 support during 2025.
- **`rand` 0.9** (Jan 2025): `gen()`/`gen_range()`/`thread_rng()` renamed; MSRV 1.63. **`rand` 0.10** (Feb 2026): `Rng` → `RngExt`, `RngCore` → `Rng`; `Clone` removed from `StdRng` and `ChaCha*Rng`; MSRV 1.85. ChaCha seeded output is stable across versions; `SmallRng` is documented as non-reproducible across versions.
- **`ron` 0.12** (Nov 2025): Better error spans (0.11), `serde` 1.0.220 `#[serde(untagged)]` compatibility fix.
- **`serde`, `serde_json`, `clap`, `smallvec`:** auto-update within semver; zero-risk.

## Topic Axes
1. Engine core (Bevy upgrade path)
2. Integration coupling & ecosystem reach
3. Determinism & replay correctness (rand)
4. Config & data layer (ron, serde)
5. Process & cadence

## Ranked Ideas

### 1. Bevy 0.15 → 0.16 buys `AssetChanged` hot-reload for `abilities.ron` / `items.ron` / `loadouts.ron` and cashes in idioms the codebase already wrote
**Description:** The headline gain in 0.16 isn't transform speedups or `EventWriter` renames — it's the `AssetChanged` query filter. Combined with Bevy's existing asset hot-reload, this turns the data-driven RON pipeline into a live-tuning surface: edit `abilities.ron`, see the change in a running match. Today, every balance tweak costs a `cargo build --release` cycle (≈30s + restart). With hot-reload, it's zero. The migration cost is also smaller than the headline 239 idiom-changes suggest, because the codebase has already adopted several Bevy-0.16-flavored patterns: zero `.single()` / `.single_mut()` calls (only `get_single()` at 6 sites — `selection.rs:110/113`, `camera.rs:122`, `rendering/effects.rs:48/276`, `rendering/hud.rs:205`); `Res<Time>` and `try_insert` are already encoded as institutional patterns in MEMORY.md; `despawn_recursive` is uniformly used (28 sites). 0.16 makes the safer/cleaner choice the default everywhere those patterns already live.

**Axis:** Engine core (Bevy upgrade path) + Config & data layer (ron, serde)

**Basis:**
- `direct:` `grep -rn "\.single()\|\.single_mut()" src` returns 0 results; `get_single()` is used 6 times specifically as a panic-avoidance idiom (sub-agent verified).
- `direct:` 28 `despawn_recursive` call sites across `projectiles.rs`, `match_flow.rs`, `rendering/effects.rs`, `auras.rs`, others.
- `direct:` `CLAUDE.md` "Adding a New Ability" Step 3 = edit `abilities.ron`; "Adding a New Item" Step 1 = edit `items.ron` — these are the most-edited files in the project.
- `external:` Bevy 0.16 release notes — `AssetChanged` query filter; `Query::single()` returns `Result`; `despawn()` recursive by default.

**Rationale:** The largest concrete *capability* gain is one feature (`AssetChanged`) that directly improves the project's hottest authoring loop — and the project has been quietly accumulating Bevy-0.16-flavored idioms as workarounds for Bevy 0.15 footguns, so the migration cost is less than naive estimates. Upgrading converts institutional knowledge into language defaults.

**Downsides:** `bevy_egui` must move in lockstep (0.31 → 0.33 minimum, 0.34 if multi-pass rendering is wanted). Still ~239 idiom call sites to mechanically migrate. Asset hot-reload may have rough edges with custom RON loaders that bypass Bevy's asset system — needs verification before betting on the feature.

**Confidence:** 70%
**Complexity:** Medium-High
**Status:** Unexplored

---

### 2. Stay on `rand` 0.9 deliberately — document the pin and the reason
**Description:** The instinct is to chase current versions across the board, but `rand` 0.10 is a *net-negative* upgrade for this project. It removes `Clone` from `StdRng` and `ChaCha*Rng` — the exact types used to implement seeded matrix-runner replay. And `SmallRng` (which a future contributor or AI agent will reach for thinking it's "the fast one") is explicitly documented as non-reproducible across `rand` versions. The right move is `rand` 0.8 → 0.9 (≈6 lines in `components/resources.rs`; `gen()` → `random()`, `from_entropy()` → `from_os_rng()`; ChaCha output stays byte-stable) — and then **pin at 0.9 with a Cargo.toml comment** explaining why. Documenting the *refusal* to upgrade is more valuable than the upgrade itself, because it turns a vague "should we?" recurring decision into an explicit, defensible non-upgrade.

**Axis:** Determinism & replay correctness (rand)

**Basis:**
- `external:` `rand` 0.10 release notes confirm `Clone` removal on `StdRng` and `ChaCha*Rng` (Feb 2026); `rand` book documents `SmallRng` as non-reproducible across versions.
- `external:` `rand` 0.9 release notes confirm seeded ChaCha output is unchanged across the 0.8 → 0.9 → 0.10 series.
- `direct:` Matrix runner's whole value proposition is byte-identical replay across 7×7 matchups (per `CLAUDE.md` and memory entries S3780, S4862).
- `reasoned:` "Latest" is a heuristic for unknown projects; for a project with a documented deterministic-replay invariant, the heuristic actively works against the invariant.

**Rationale:** The matrix runner is the project's most expensive correctness asset. A 6-line migration to 0.9 is cheap and removes deprecated method names from future LLM-generated code; staying at 0.10 would force a `Clone` workaround across replay-snapshot serialization. Naming the ceiling explicitly forecloses a class of future "we just bumped everything" PRs that would silently break it.

**Downsides:** If a new crate later requires `rand` 0.10+, Cargo will fail to unify versions and force the upgrade. The pin needs revisiting then — but you'll be making the decision with eyes open instead of by accident.

**Confidence:** 85%
**Complexity:** Low (≈6 lines edit + 1 `Cargo.toml` comment + revisit when forced)
**Status:** Unexplored

---

### 3. `bevy_egui` is the silent ecosystem gate; every quarter on Bevy 0.15 shrinks the addressable Bevy crate landscape
**Description:** The January 2026 `bevy_egui` bet (per `design-docs/egui-migration-summary.md`) was explicitly about *reaching for ecosystem leverage instead of hand-rolling UI*. That bet is currently being undermined by staying on Bevy 0.15 — most community Bevy crates (avian/rapier physics, bevy_hanabi particles, leafwing-input-manager, bevy_replicon, ECS inspectors) have dropped sub-0.16 support during 2025. The friction is invisible *today* because no one's tried to add one of these crates — but it's the silent reason any "let me grab this Bevy crate" gesture will currently fail. Adding to this: Claude's training data and Bevy community examples skew toward the latest stable, so every LLM-generated Bevy idea in this project currently pays a translation-from-current-to-0.15 tax.

**Axis:** Integration coupling & ecosystem reach

**Basis:**
- `direct:` `Cargo.toml` line 11 pins `bevy_egui = "0.31"`; `design-docs/egui-migration-summary.md` documents the deliberate January 2026 ecosystem-leverage bet.
- `external:` `bevy_egui` CHANGELOG version-to-Bevy table — 0.33+ requires Bevy 0.16+; ecosystem-wide drop of sub-0.16 support during 2025 (verified across multiple Bevy ecosystem crate release notes by Phase 1 web researcher).
- `reasoned:` Project is AI-native (per CLAUDE.md and the bug-hunt skill). LLM training cutoffs make older Bevy APIs progressively less accurate over time — the tax compounds with every prompt.

**Rationale:** The cost of staying on 0.15 isn't visible in today's code — it's visible in the absence of future code (community crates that can't be adopted). And the existing egui investment is structurally tied to the same upgrade. The "should we upgrade Bevy?" question is really "should we keep the door open to the Bevy ecosystem we already invested in reaching for?"

**Downsides:** This is observational, not a single action — it's the strategic frame behind ideas #1 and #7. Without those, idea #3 doesn't convert into anything shippable.

**Confidence:** 80%
**Complexity:** N/A (observation that sharpens the cost-of-inaction side of the assessment)
**Status:** Unexplored

---

### 4. CI-gate every dependency bump on matrix-runner byte-identity
**Description:** ArenaSim has something most projects upgrading Rust deps don't: a deterministic, seeded 7×7 matrix runner that produces ~4900 reproducible trace JSONL files per run. That's a behavioral regression test stronger than any combination of unit tests. The methodology that makes every future dep bump cheap: snapshot the current trace corpus as a baseline (committed to a sidecar location or git-LFS), add a CI job that re-runs the matrix at the same seeds post-bump and diffs `match_logs/traces/*.jsonl` byte-for-byte. The upgrade isn't done until the diff is empty, or every diff is explained and accepted. This turns "did this upgrade preserve behavior?" from a code-read into a `git diff` question. Pairs naturally with idea #2 — when `rand` *is* eventually upgraded, the baseline rotates and the new baseline carries a `rand_version` stamp in trace metadata so old corpora remain comparable evidence rather than orphaned files.

**Axis:** Process & cadence

**Basis:**
- `direct:` `CLAUDE.md` describes the matrix runner producing 4900 trace JSONL files at predictable paths (`match_logs/traces/match_<seed>_<c1>_v_<c2>_trace.jsonl`); BTreeMap migration is already done so iteration order is stable.
- `direct:` Memory entries S3780 ("Determinism integration tests added for seeded match replay verification") + S4862 confirm seeded replay infrastructure is already in place.
- `reasoned:` A deterministic oracle + a batch runner + a stable iteration order is exactly the trio needed for behavioral equivalence as a CI check.

**Rationale:** Most dependency-upgrade anxiety dissolves once you have a behavioral oracle this strong. Without this gate, a Bevy or `rand` bump silently changes RNG output or system ordering and the breakage surfaces months later. With it, the upgrade either passes or produces a precise, auditable list of what changed.

**Downsides:** Trace files are large (≈12MB per matrix run per memory entry S3741). Storage cost across baselines needs a strategy (LFS, sidecar branch, or "latest only"). The "diff is non-empty but acceptable" case requires human judgment — easy to rubber-stamp.

**Confidence:** 90%
**Complexity:** Low-Medium (one-time tooling: a `scripts/diff-matrix.sh` and a CI hook)
**Status:** Unexplored

---

### 5. One-hour leaf-crate refresh PR — decouple the cheap half from the Bevy half
**Description:** Most of the perceived "upgrade everything" effort is actually Bevy. Decoupling: `ron` 0.8 → 0.12 (better error spans for `abilities.ron`/`items.ron` parse failures — matters disproportionately for AI agents authoring those files), and let `serde`, `serde_json`, `clap`, `smallvec` float to current within their respective `1.x`/`4.x` ranges (auto-resolved by Cargo within the existing version specs). The migration risk is near-zero (`ron` 0.12 doesn't break our use of the format because we don't use byte-string literals in `.ron` config; `ron` 0.12 also closes the `serde` 1.0.220 untagged-enum compatibility hole). The signal value is high: it closes ~half of the dependency-freshness debt in a single PR and validates the CI gate from idea #4 on a low-stakes change before betting on it for Bevy.

**Axis:** Config & data layer (ron, serde) + Process & cadence

**Basis:**
- `direct:` Three RON load paths (`ability_config.rs:371`, `equipment.rs:646`, `equipment.rs:663`) with two error-handling sites (`ability_config.rs:398`, `equipment.rs:704/709`) — parse failures happen in real workflows.
- `direct:` CLAUDE.md's "Adding a New Ability" and "Adding a New Item" workflows make `.ron` files the most-edited assets in the project.
- `external:` `ron` 0.11/0.12 release notes — better spans + `serde` 1.0.220 untagged enum fix.
- `external:` `serde`, `clap`, `smallvec` Cargo.toml entries use floating specs (`"1.0"`, `"4.4"`, `"1.13"`); Cargo will resolve to latest compatible automatically on `cargo update`.

**Rationale:** Most projects conflate "upgrade dependencies" into a single decision. Splitting the cheap half off lets it ship this week, exercises the CI matrix-diff oracle from #4 on a low-stakes target, and stops the bigger Bevy decision from blocking small wins.

**Downsides:** Mostly cosmetic gain — better error messages for human/agent debugging, not new capability. The pair-wise interaction with `serde` 1.0.220 + older `ron` could silently corrupt nested ability configs (no observed defect today, but the untagged-enum hole is a real future-bug magnet).

**Confidence:** 95%
**Complexity:** Low (≈1-hour PR, run `cargo update` + bump `ron` to `"0.12"` + `cargo test`)
**Status:** Explored

---

### 6. Adopt per-dependency upgrade tracks (RCM triage)
**Description:** Factories sort equipment into three maintenance strategies based on failure cost: run-to-failure (cheap to replace), scheduled (predictable wear), predictive (catastrophic if missed). Apply the same to this dependency graph and write the result into `CLAUDE.md` or a new `design-docs/dependency-policy.md`:
- **Run-to-failure** (just bump when something breaks): `serde`, `serde_json`, `clap`, `smallvec`.
- **Scheduled** (planned upgrade windows, never in the same PR as a feature): `bevy`, `bevy_egui`, `ron`.
- **Predictive** (instrument before touching, requires explicit category boundary): `rand`.

This is the meta-framework that absorbs the cadence cluster of ideas (Debian tracks, console seasons, blue/green, five-year lens, quarterly cadence, defer-until-1.0). Different policy per class means individual upgrade PRs reference the document instead of re-deriving "should we?" each time. Paired with idea #4 (the CI oracle), it tells reviewers what's acceptable diff for each class.

**Axis:** Process & cadence

**Basis:**
- `external:` Reliability-centered maintenance (RCM) classifies assets by consequence-of-failure, not by age — a 50-year-old industrial framework that maps cleanly onto Cargo dependencies.
- `reasoned:` The three dep classes in this project have measurably different failure shapes: leaf crates churn API never (run-to-failure works), Bevy churns visibly on a 3-4 month cadence (scheduled works), rand churns determinism (predictive is the only safe choice given the matrix runner).
- `direct:` No prior captured upgrade learnings — the absence is the gap this policy fills (per Phase 1 learnings-researcher scan).

**Rationale:** Closes the recurring decision loop. Every future dep-upgrade PR has a written policy to reference, and the absence of one was specifically flagged by the Phase 1 learnings researcher as the highest-value documentation gap.

**Downsides:** Risk of over-formalizing for a solo/prototype project. The categorization itself is a judgment call (e.g., is `bevy_egui` scheduled like Bevy, or run-to-failure like leaf crates? — it's actually scheduled-coupled-to-Bevy, which is a slightly different fourth class).

**Confidence:** 65%
**Complexity:** Low (write the policy; ≈1 page in `design-docs/`)
**Status:** Unexplored

---

### 7. Bevy 0.17's SystemSet rename is the moment to revisit the dual-registration audit
**Description:** `tests/registration_audit.rs` exists because `process_dispels`, `process_holy_shock_heals`, `process_holy_shock_damage`, and `process_divine_shield` each silently failed in one of the two registration paths historically (per `docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md`). The audit is institutional pain made manifest. Bevy 0.17 renames `RenderSet` → `RenderSystems`, `UiSystem` → `UiSystems`, `TransformSystem` → `TransformSystems`, and ships Reflect auto-registration — exactly the surface the audit walks. The upgrade is a natural forcing function to ask: *could the dual-registration architecture be replaced by a single SystemSet-based composition*? If yes, the headline benefit of Bevy 0.17 isn't engine features — it's deleting `tests/registration_audit.rs` and the three-path decision flow that takes up half a section of `CLAUDE.md`. If no, extending the audit to be Bevy-version-aware (emit a schedule-set delta report) makes it the regression detector for the upgrade itself.

**Axis:** Engine core (Bevy upgrade path) + Process & cadence

**Basis:**
- `direct:` `tests/registration_audit.rs` exists; `CLAUDE.md` "Adding a New Combat System" devotes a whole section to picking between three registration paths; `docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md` names four historical silent-failure bugs.
- `external:` Bevy 0.17 `SystemSet` renames with `Systems` suffix and Reflect auto-registration (per migration guide).
- `reasoned:` Audit-style guard tests are signals of architectural pain the codebase couldn't resolve at the type level; a major refactor in the surface they guard is the natural time to ask whether the pain is still necessary.

**Rationale:** This is the highest-leverage *architectural* opportunity in the assessment — every other survivor is a tactical or process win. Deleting an institutional guard test (or hardening it as a Bevy-version-aware regression detector) is the move that has the biggest long-term contributor-friction impact.

**Downsides:** Speculative — depends on whether 0.17's new SystemSet composition actually obviates the dual-registration shape. May turn out the audit is still necessary for headless/graphical mode discrimination unrelated to SystemSets. Only worth surfacing after the Bevy 0.16 hop in idea #1 has landed.

**Confidence:** 60%
**Complexity:** Medium (the *investigation* is small; the *refactor* depends on the answer)
**Status:** Unexplored

## Rejection Summary

| # | Idea | Reason Rejected |
|---|------|-----------------|
| F2.2 | Remove `bevy_egui` entirely; render debug UI as Bevy entities | Basis wrong — `design-docs/egui-migration-summary.md` shows `bevy_egui` is user-facing (ConfigureMatch, 968→240 LoC win), not debug-only. Removal would re-undo the deliberate January 2026 ecosystem-leverage bet |
| F5.8 | Publish ArenaSim's trace-versioning pattern as community write-up to attract Bevy upgrade PRs | Scope overrun; below meeting-test for this assessment. Could be a follow-up after idea #4 ships |
| F6.3 | Abandon byte-identical replay determinism (free `rand` 0.10 + `SmallRng`) | Useful diagnostic question ("is our determinism need byte-identical or statistical?") but as a design it's a subject-shifting move, not a library-upgrade conclusion |
| F6.7 | Snapshot demolition — split human-edited RON (stable) from machine-generated traces (per-version) | Too speculative; current format works and no observed defect motivates the split |
| F6.5 | Parallel-agents codemod pipeline for Bevy migration | Duplicates the ast-grep codemod tactic; folded as a Bevy-upgrade tactic under idea #1 |
| (cluster G) | Cadence variants — Debian/Arch tracks, console seasons, blue/green `bevy-next` branch, five-year lens N=2, quarterly cadence, defer-until-1.0 | All merged into survivor #6 (RCM triage) — they're each *one* policy answer that #6 generalizes |
| F4.2 | Document first Bevy upgrade as playbook for future ones | Merged into survivor #6 — the policy doc subsumes the playbook |
| F1.2 | Time<Real> trap — Bevy 0.16 sharpens Time story | Folded into survivor #1 as part of "already-adopted future-proof idioms" |
| F1.4 | `despawn_recursive` cliff — 28 sites that 0.16 makes obsolete-but-still-working | Folded into survivor #1 as part of "already-adopted future-proof idioms" |
| F4.5 | Reflect auto-registration (0.17) compounds across components | Folded into survivor #7 (depends on the SystemSet/audit revisit) |
| F2.5 / F6.2 | Vendor `rand_chacha`; Cargo.lock as versioned asset | Folded into survivor #2 — pinning at 0.9 with a written rationale achieves the same insulation without the vendoring overhead |
| F6.8 | MSRV ratchet — bump Rust before Bevy hop | Tactical detail under survivor #1; not standalone meeting-worthy |
