# Residual Review Findings — Iteration 2a (Hunter Pet Engagement)

Source: `ce-code-review mode:autofix` run `20260522-204339-a6ad2fa1` against plan `docs/plans/2026-05-22-002-feat-hunter-pet-engagement-plan.md`.

Branch: `worktree-hunter-rebalance` (HEAD `370525e`)
Run artifact: `/tmp/compound-engineering/ce-code-review/20260522-204339-a6ad2fa1/`
Reviewers: correctness, testing, maintainability, project-standards, agent-native, learnings. **Adversarial reviewer timed out** at ~63 min — noted in Coverage.

The autofix loop applied 2 safe_auto fixes (CLAUDE.md doc updates: `dispatched_by` jq recipe + LowHealthHeel recipe + pet target_acquisition exclusion note + Hunter class engagement-model description). These residuals were left for downstream resolution.

## Residual Review Findings

- **[P1][manual]** Heel predicate has no unit test. `pet_ai_system`'s HP<25% branch clears target, despawns PetCommand, and emits LowHealthHeel — three coupled effects with zero unit coverage. Plan U6 listed test scenarios that weren't implemented. *Suggested fix:* focused unit test for the Heel branch covering (a) target cleared, (b) PetCommand removed, (c) single LowHealthHeel reject in trace.

- **[P2][manual]** `apply_deferred` between `decide_abilities` and `pet_ai_system` (`src/states/play_match/systems.rs:176`) makes Felhunter see same-tick CastingState. Could be intended interrupt responsiveness or unintended early-interrupt window. Correctness reviewer flagged at 70% confidence. *Suggested fix:* decide intent, add a comment noting the behavior change either way.

- **[P2][manual]** **`ability_cooldowns` snapshot field has zero consumers (90% confidence dead code until U4)** at `src/states/play_match/class_ai/mod.rs` + `combat_snapshot.rs`. Maintainability reviewer's strongest finding. *Suggested fix:* land alongside U4 (consumer arrives same iteration), OR revert the field + population sites + AbilityType Ord derive until U4 is imminent.

- **[P2][manual]** **PetCommand 50 LOC inert surface** — component, three despawn handlers, apply_deferred sync point, `dispatched_by` event field, `start_pet_dispatch_decision` builder all dormant today. *Suggested fix:* same as above — land with U4 or revert until U4 is imminent.

- **[P2][manual]** Two snapshot-build sites with 17-field `CombatantInfo` literals (`combat_snapshot.rs` and `pet_ai.rs:50-`) — drift risk. They already silently diverge on `is_pet`/`pet_type` (benign today because pet_ai filters `Without<Pet>`, but undocumented). *Suggested fix:* extract `fn build_combatant_info(entity, combatant, transform, pet_lookup, …) -> CombatantInfo` helper used by both sites.

- **[P2][manual]** `start_pet_dispatch_decision` duplicates `start_pet_decision` (20 LOC clone, one field differs, zero callers). *Suggested fix:* fold into `start_pet_decision` via `dispatched_by: Option<Entity>` parameter, OR defer until U4 actually calls it.

- **[P2][manual]** Test gaps (4): `headline_ability_for` table test, PetCommand lifecycle test (3 despawn paths), snapshot field population assertions, `spider_ai` tie-break semantics (changed from dist_to_owner to dist_to_spider). *Suggested fix:* add focused tests for each.

- **[P3][gated_auto]** `AbilityType` `PartialOrd, Ord` derive (`src/states/play_match/abilities.rs:44`) load-bearing only for the unused `ability_cooldowns` map (domain-meaningless enum-declaration order). *Suggested fix:* keep with a doc-comment guardrail noting load-bearing-for-BTreeMap-determinism, or revert until U4.

## Coverage notes

- **Adversarial reviewer timed out** after ~63 minutes. Adversarial findings (race conditions in PetCommand spawn/read across apply_deferred, multiple-pet handling, Heel-while-casting interaction, Spider Web reach-through-obstacles, Ord-derive sorting impact, pet target staleness) are not in this residual list. Re-run adversarial review separately if time allows before iteration 2b.
- **project-standards** reported zero violations — CLAUDE.md / AGENTS.md compliance is clean.
- **Learnings researcher** surfaced 5 relevant institutional patterns (friendly-CC threshold semantics, BTreeMap determinism discipline, dual system-registration audit, one-shot component placement convention, snapshot-at-cast pattern). All currently compliant; documented as advisory.

## Honest framing of the dead-code findings

The single load-bearing critique across maintainability (#5 and #6 in the synthesis) is that this iteration lands ~75 LOC of inert infrastructure (PetCommand component, apply_deferred wiring, `ability_cooldowns` snapshot field, `CombatantInfo.pet` field, `start_pet_dispatch_decision` builder, AbilityType Ord derive) whose only consumer is U4 (Hunter `try_dispatch_*` helpers).

**Resolution (post-review decision):** U4 will land as additional commits on the same PR (#59) before merge — not as a separate "iteration 2b" PR. This makes the dead-code findings moot at merge time because the infrastructure ships with its consumer. The plan was updated (`status: active`, U4 added back to `remaining_units`) so a fresh `/ce-work` invocation picks up U4 directly.

If U4's implementation reveals a reason to defer further, the alternative is to revert the U3 commit (PetCommand + EventPayload extension + apply_deferred) and re-land it with U4. The Heel predicate + `LowHealthHeel` rejection variant are live today and must stay — the partial revert would only touch the dispatch-side scaffolding.
