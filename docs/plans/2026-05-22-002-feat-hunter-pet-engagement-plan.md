---
date: 2026-05-22
status: active
type: feat
title: Hunter pet engagement (Send/Heel + Hunter dispatch)
origin: docs/brainstorms/2026-05-22-hunter-pet-engagement-requirements.md
shipped_units: [U1, U2, U3, U5, U6, U7, U8]
remaining_units: [U4]
---

## Handoff Status

**Open PR:** https://github.com/dwalker-va/arenasim-prototype/pull/59 (branch `worktree-hunter-rebalance`).

**Shipped on the PR so far:** U1, U2, U3, U5, U6, U7, U8. Validation in `docs/reports/2026-05-22-hunter-pet-engagement.md` shows Hunter v Warrior 0→20%, Hunter v Paladin 0→5% at N=20.

**Remaining: U4 — Hunter `try_dispatch_*` helpers.** Will bundle into the same PR before merge (decision: avoid landing ~75 LOC of dormant infrastructure that the code review flagged as 90% dead until U4 ships).

**Next-session invocation:** run `/ce-work` against this plan path. ce-work will see `status: active` + the U1-U3,5-8 commits already in git history, identify U4 as the only remaining unit, and pick up there. Do **not** re-run `/ce-plan` (the plan is correct as written) or `/lfg` (would restart the whole pipeline).

**Pre-merge checklist after U4 lands:**

- [ ] Confirm `pet_decision` trace events include `dispatched_by: Some(hunter_id)` for Hunter-dispatched casts (recipe in CLAUDE.md decision-trace section)
- [ ] Re-run trace audit on Hunter v Warlock — Spider Web fires via PetCommand path, not via the autonomous `spider_ai` fallback
- [ ] Decide whether to keep the autonomous `spider_ai` Spider Web path as a fallback (the U5 Outstanding Question — default delete per the plan)
- [ ] Re-run 1v1 matrix at N=20 to confirm Hunter v Warrior 20% holds with Hunter-dispatched Web instead of autonomous Web
- [ ] N=100 rerun (1v1) before tagging the PR ready

**Why bundle on the same PR (vs split):** the code review flagged ~75 LOC of inert surface (PetCommand component, `ability_cooldowns` snapshot field, `start_pet_dispatch_decision` builder, AbilityType Ord derive) at 90% confidence dead until U4. Landing U4 on the same PR makes the maintainability finding moot — the infrastructure ships with its consumer. Reverting U3 to split would also work but loses the Hunter v Warrior 0→20% signal that U3+U5+U8 produced.

---

# feat: Hunter pet engagement (AI-only Send/Heel)

## Summary

Wire up the Send/Heel command model for Hunter pets via a `PetCommand` one-shot component, lift headline pet-ability dispatch (Spider Web / Boar Charge / Master's Call) from pet AI to Hunter AI, remove Spider Web's owner-distance filter, add low-HP Heel predicate in pet AI, and apply a minimal Spider Web cooldown carveout (45s → 20s) so the architecture has a chance to fire 2-3 times per match. Validation runs the same shape as the mana iteration: pre/post trace audit + 1v1 + 2v2 matrix sweeps. **Approach A−** — AI architecture changes plus a single ability cooldown tune; no other CD or range tuning.

---

## Problem Frame

The post-mana trace (Hunter v Warlock, post PR #55) shows pet AI is the new dominant rejection class — 1,000+ `SpiderWeb:NoValidTarget` rejections in a single 16s match, and zero pet auto-attack damage entries despite the Spider being on the field. The brainstorm (see origin: `docs/brainstorms/2026-05-22-hunter-pet-engagement-requirements.md`) attributed this to pets never receiving a `combatant.target`. Repo research corrected that premise — `acquire_targets` (combat_ai.rs:103) **does** iterate pets and assign targets via the team-wide kill-priority logic. The actual cause is that `spider_ai` (pet_ai.rs:362) uses its own target-search loop with a `dist_to_owner ≤ 15.0 AND dist_to_spider ≤ 20.0` filter independent of `combatant.target`, and that filter is incompatible with Hunter's 35yd range identity. Spider Web's target IS rejected; Spider doesn't pursue because spider_ai's own target search never finds a valid one and the pet's pursued target (set by acquire_targets) may not align with what the Hunter is doing.

The plan reframes "pet engagement" as three coupled changes: (a) move pet target ownership out of acquire_targets and into pet AI + Hunter dispatch, (b) lift the headline-ability decision out of per-pet `try_*` helpers and into Hunter AI via a `PetCommand` one-shot component, (c) remove the spider_ai owner-distance filter that prevents Web from ever firing in 1v1.

---

## Requirements Traceability

| Plan unit | Origin R-IDs covered |
|---|---|
| U1 | R1, R2 (pet target ownership) |
| U2 | R3 (snapshot extension: ability_cooldowns + pet) |
| U3 | R3, R5 (PetCommand dispatch mechanism, trace attribution) |
| U4 | R3, R5 (Hunter AI dispatch helpers; optimistic dispatch) |
| U5 | R6 (Spider Web filter removal) |
| U6 | R7, R8 (Heel predicate in pet AI) |
| U8 | Supports R9-R11 (Spider Web CD tune so validation is falsifiable) |
| U7 | R9, R10, R11 (validation) |

R4 (pet auto-attack execution and pursuit movement) is preserved by existing code — no unit modifies the `combat_auto_attack` system or `move_to_target`'s target-pursuit branch. U1 explicitly does not break auto-attack.

Origin flows F1 (engage), F2 (headline ability), F3 (Heel) are covered by U1+U2+U3+U4 (engage + dispatch), U3+U4 (ability), U6 (Heel) respectively. AEs are addressed in per-unit test scenarios — except **AE3 is dropped** (see Key Technical Decisions).

---

## Key Technical Decisions

- **`PetCommand` one-shot component is the dispatch mechanism.** Rationale: matches the existing one-shot-component idiom already used by `AuraPending`, `DispelPending`, `InterruptPending` in `pet_ai.rs:248,286,597`. Avoids needing to restructure pet `Combatant` borrows in `combat_ai.rs::decide_abilities`. Hunter AI spawns `PetCommand { pet: Entity, ability: AbilityType, target: Entity }`; `pet_ai_system` reads at the top of its tick, executes the headline ability, despawns the component.
- **`apply_deferred` is inserted between `decide_abilities` and `pet_ai_system`** in the system chain at `src/states/play_match/systems.rs:170-184`. Required because Bevy's `Commands` queue is only flushed at `apply_deferred` boundaries; without it, a PetCommand spawned by Hunter in `decide_abilities` would not be visible to `pet_ai_system` in the same tick (one-tick lag). With the explicit `apply_deferred`, same-tick consumption works as the U3/U4 narratives describe. Cost: one extra sync point per tick (negligible at 60 FPS).
- **`CombatantInfo` gets two new snapshot fields:** `ability_cooldowns: BTreeMap<AbilityType, f32>` and `pet: Option<Entity>` (the owner→pet reverse lookup). Both populated by `CombatSnapshot::build` in `class_ai/combat_snapshot.rs` AND by pet_ai's local snapshot build at `class_ai/pet_ai.rs:50-70` — same fields must appear at both sites or pet AI's reads will return stale/missing data. `BTreeMap` (not `HashMap`) for determinism (per `docs/solutions/implementation-patterns/ai-decision-trace.md` rules).
- **Optimistic dispatch: Hunter uses snapshot heuristics, pet AI runs authoritative `pre_cast_ok` at execution time.** Rationale: avoids `pre_cast_ok` signature changes (which would touch ~30 existing class-AI call sites). Hunter AI gates dispatch on the snapshot's cooldown + Heel + range checks; pet AI runs full `pre_cast_ok` with live `&Combatant` when executing the PetCommand and rejects (despawn command, emit `reject(OnCooldown)` or equivalent) if conditions changed. The "race condition" the brainstorm worried about becomes the design — both layers do their job; misalignment is recoverable.
- **Pet AI owns the Heel state machine; Hunter AI respects it.** Rationale: keeps `pet_decision` traces clean — the pet's own decide function emits a single rejection with reason `LowHealthHeel` when HP < 25%. Hunter AI just reads `ctx.combatants.get(&pet_entity).map(|info| info.health_pct() >= 0.25)` as a precondition. Single source of truth for "pet too hurt to engage."
- **Pets are excluded from `acquire_targets`.** Rationale: pet `combatant.target` becomes solely owned by pet AI + Hunter AI dispatch. Avoids the "kill_target index resolves to a different enemy than the Hunter is shooting" inconsistency. One-conditional change at `combat_ai.rs:103-104` mirroring the existing dispatch-loop skip at combat_ai.rs:488.
- **`EventPayload::Pet` gets `dispatched_by: Option<u32>` field.** Rationale: preserves the existing `jq -c 'select(.kind == "pet_decision") | ...'` audit recipes (per project `CLAUDE.md`); surfaces hybrid-model attribution for the trace audit (R11). Optional so existing event consumers see `null` for autonomous pet decisions.
- **Minimum Spider Web cooldown tune (45s → 20s) is in scope; other pet CDs unchanged.** Rationale: at 45s CDs in 30s matches, Spider Web fires 0-1 times per match — the architecture changes (PetCommand, lifted dispatch) won't produce a measurable signal because they barely fire. Reducing only Spider Web (not Boar Charge or Master's Call) preserves "isolate variables" intent while ensuring the Hunter AI dispatch path fires 2-3 times per match for real validation. The brainstorm's "Approach A vs Approach B" choice is refined to "A− with Spider Web carveout."
- **AE3 from the origin is dropped as a scope/cost decision, with a detection trace recipe added to U7.** Rationale: AE3 (Hunter Aimed Shot crit breaking Spider Web's 80-damage threshold root) is a real failure mode — `has_friendly_breakable_cc` only fires on `threshold == 0.0` auras (matches Frost Nova's existing semantics). Implementing AE3 would require expanding the guard to predict crit damage against numeric thresholds, affecting Frost Nova too. Out of scope for this iteration, BUT U7's trace audit now includes a recipe that detects friendly-broken Roots so silent debt becomes visible: "Spider Web aura applied at tick N → Root removed by friendly damage within K seconds." If matrix data shows this case biting, future iteration adds Hunter target deprioritization while pet root is active (sidesteps the crit prediction problem).
- **R8's recovery exit path is forward-looking infrastructure.** Rationale: research confirmed pets cannot recover HP mid-match today — no AoE heals exist, all single-target heals filter `is_pet`. The HP-above-25% recovery check is implemented for correctness but won't fire until a Mend Pet ability or party heal exists. Acceptance criteria adjust accordingly.

---

## High-Level Technical Design

This illustrates the intended approach and is directional guidance for review, not implementation specification. The implementing agent should treat it as context, not code to reproduce.

```text
Per-tick sequence (Hunter player has a Spider pet, enemy at 28yd):

[acquire_targets system]
  Hunter:   target = enemy (kill-priority resolves to enemy)
  Spider:   SKIPPED (new conditional at combat_ai.rs:103)

[Hunter AI dispatch — class_ai/hunter.rs]
  decide_hunter_action:
    1. Existing Hunter ability dispatch (Aimed Shot / Concussive / etc.) runs
    2. NEW: try_dispatch_spider_web(commands, abilities, hunter_entity, spider_entity, ctx, builder)
       - Reads ctx.combatants[spider].ability_cooldowns[SpiderWeb] (from new snapshot field)
       - Reads ctx.combatants[spider].position
       - Calls pre_cast_ok(SpiderWeb, def, /* pet's combatant proxy via snapshot */, ...) with friendly_cc opts
       - If ok: spawns PetCommand { pet: spider_entity, ability: SpiderWeb, target: enemy }
       - Emits pet_decision trace event with dispatched_by: Some(hunter_entity)

[pet_ai_system — class_ai/pet_ai.rs]
  For each pet:
    1. Check Heel state: if combatant.health < 0.25 * max_health,
       clear combatant.target, emit pet_decision with reject(SelfHeeling), return.
    2. NEW: check for PetCommand on this entity. If present:
       - Execute the commanded ability (Spider Web projectile spawn at pet position)
       - Emit pet_decision with chose(SpiderWeb, target, dispatched_by: Some(owner))
       - Despawn PetCommand
       - Return (don't run autonomous decide path this tick)
    3. Autonomous path: set combatant.target = owner.target (if owner has one)
       so existing target-pursuit movement closes on enemies.
    4. (No autonomous Spider Web call this iteration — Hunter owns headline ability)

[movement.rs::move_to_target]
  Spider has combatant.target = enemy (from step 3 above).
  Existing target-pursuit branch (movement.rs:391) runs, Spider closes.
  Once within 2.5yd, combat_auto_attack starts landing pet swings.
```

---

## Implementation Units

### U1. Skip pets in `acquire_targets`; assign pet target from owner in pet AI

- **Goal:** Pet `combatant.target` becomes pet-AI-owned. `acquire_targets` no longer writes to pet combatants. Pet AI's per-tick decide assigns `pet.target = owner.target` when the owner has a valid target.
- **Requirements:** R1, R2, F1.
- **Dependencies:** None.
- **Files:**
  - `src/states/play_match/combat_ai.rs` (add pet skip near line 103 — `if pet_query.get(entity).is_ok() { continue; }`)
  - `src/states/play_match/class_ai/pet_ai.rs` (pet target assignment at the top of `pet_ai_system`'s per-pet loop after Heel check)
- **Approach:**
  - In `acquire_targets`, add the pet-skip conditional immediately after the dead-target clear logic at combat_ai.rs:103. Pattern mirrors the existing skip at combat_ai.rs:488-490 used in the dispatch loop.
  - In `pet_ai_system`, before the per-pet `match pet.pet_type { ... }` block, assign `combatant.target = ctx.combatants.get(&pet.owner).and_then(|o| o.target)` when the pet is not in Heel mode.
  - When the owner has no target (e.g., owner died, owner facing all-stealth), pet target falls back to `None`, and existing movement code routes the pet to the "follow owner at 3yd" branch (movement.rs:309-340).
- **Patterns to follow:**
  - `combat_ai.rs:488-490` — existing pet-skip pattern in the dispatch loop
  - `pet_ai.rs:30-167` — `pet_ai_system` structure
- **Test scenarios:**
  - Happy path: Hunter targets enemy Warlock at 28yd. Pet (Spider) gets `combatant.target == Warlock_entity` on the next tick. (Covers F1.)
  - Edge case: Hunter target is dead. Pet target clears to None on the next tick.
  - Edge case: Hunter has no target (facing all-stealth Rogue team). Pet target stays None; movement routes pet to follow-owner branch.
  - Determinism: Same seed runs produce identical pet `combatant.target` sequences in the trace.
- **Verification:** `cargo test --release` passes. A headless Hunter v Warlock match shows the Spider's position changing in the match log (closing on the Warlock) and at least one pet auto-attack damage entry. AI decision trace shows `target_acquisition` events for the Hunter but **not** for the Spider (pets skipped).

### U2. Extend `CombatantInfo` with `ability_cooldowns` + `pet` snapshot fields

- **Goal:** Hunter AI can read the pet's per-ability cooldown state AND find its pet entity from the per-frame snapshot, enabling `try_dispatch_*` helpers without holding mutable handles to pet `Combatant`.
- **Requirements:** R3 (precondition for Hunter-side cooldown gating + pet lookup).
- **Dependencies:** None.
- **Files:**
  - `src/states/play_match/class_ai/mod.rs` (`CombatantInfo` struct definition — add fields)
  - `src/states/play_match/class_ai/combat_snapshot.rs` (`CombatSnapshot::build` — populate fields)
  - `src/states/play_match/class_ai/pet_ai.rs` (lines ~50-70 — pet AI's local snapshot build must populate same fields)
- **Approach:**
  - Add `pub ability_cooldowns: BTreeMap<AbilityType, f32>` to `CombatantInfo`. `BTreeMap` not `HashMap` for determinism (per `docs/solutions/implementation-patterns/ai-decision-trace.md` rules).
  - Add `pub pet: Option<Entity>` to `CombatantInfo`. For non-pet combatants, this is `Some(pet_entity)` when the combatant owns a pet (lookup via `Query<&Pet>` filtered by `pet.owner == combatant_entity`). For pet combatants and non-owners, this is `None`.
  - In `CombatSnapshot::build`, clone the combatant's `ability_cooldowns` and populate the owner→pet reverse lookup. Cheap (~5-10 cooldown entries per combatant; pet lookup is a single iterate over Query<&Pet> per snapshot build).
  - In `pet_ai_system`'s local snapshot build at `pet_ai.rs:50-70`, populate the same fields. **Critical:** missing this site causes pet AI's `ctx.combatants` reads to return stale/empty data — both build sites must stay in sync.
  - No other AI changes needed for this unit; downstream Hunter dispatch in U4 reads from these fields.
- **Patterns to follow:**
  - `class_ai/mod.rs` `CombatantInfo` existing fields (team, slot, class, current_health, position, target, is_pet, pet_type)
  - `class_ai/combat_snapshot.rs:62-89` existing field population
- **Test scenarios:**
  - Happy path: A combatant with `combatant.ability_cooldowns.insert(AimedShot, 8.5)` produces a `CombatantInfo` with `ability_cooldowns.get(&AimedShot) == Some(&8.5)` after `CombatSnapshot::build`.
  - Edge case: Combatant with no recorded cooldowns produces a `CombatantInfo` with an empty `ability_cooldowns` map.
  - Determinism: `tests/headless_tests.rs::trace_on_matches_trace_off_outcomes` continues to pass — the new field doesn't introduce iteration order non-determinism (BTreeMap guarantees order).
- **Verification:** `cargo test --release` passes. Existing class AI continues to work (none of the current call sites read this field yet — U4 is the first consumer).

### U3. `PetCommand` one-shot component + integration with `pet_ai_system`

- **Goal:** A new component carrying `{ pet: Entity, ability: AbilityType, target: Entity }` and integration logic in `pet_ai_system` that reads the component, executes the commanded ability, despawns the component, and emits the appropriate `pet_decision` trace event.
- **Requirements:** R3, R5 (dispatch mechanism + trace attribution).
- **Dependencies:** None code-wise, but doesn't deliver value until U4 lands.
- **Files:**
  - `src/states/play_match/components/pets.rs` (add `PetCommand` struct + derive Component)
  - `src/states/play_match/class_ai/pet_ai.rs` (read `PetCommand` at top of per-pet loop, execute, despawn)
  - `src/states/play_match/decision_trace/events.rs` (extend `EventPayload::Pet` with `dispatched_by: Option<u32>`)
  - `src/states/play_match/decision_trace/builder.rs` (helper to attach `dispatched_by` when building pet_decision events)
  - `src/states/play_match/decision_trace/mod.rs` (extend `start_pet_decision` or add `start_pet_dispatch_decision` helper)
- **Approach:**
  - `PetCommand` is a one-shot ECS component spawned by Hunter AI via `commands.spawn(PetCommand { ... })` or `commands.entity(pet).try_insert(PetCommand { ... })`. Pattern matches `AuraPending` (a global entity) vs `DispelPending` (attached to caster); choose attached-to-pet to make the pet AI query straightforward.
  - In `pet_ai_system`, query `Query<&PetCommand, With<Pet>>` (or attach to the existing pets query). If a `PetCommand` exists for the pet on this tick:
    1. Build a `pet_decision` trace event with `dispatched_by: Some(command.owner_entity_id)` (where owner_entity_id comes from the pet's `Pet.owner`).
    2. Execute the headline ability based on `command.ability`. For Spider Web: spawn the projectile as the existing `try_spider_web` does today. For Boar Charge: apply the existing charge logic. For Master's Call: apply the dispel.
    3. `commands.entity(pet).remove::<PetCommand>()` to despawn after execution.
    4. Return early from the per-pet loop — autonomous decide path does not run this tick.
  - `EventPayload::Pet` gains the optional field. Existing emissions pass `dispatched_by: None`. Hunter AI's U4 dispatch passes `Some(hunter_entity_id)`.
  - Update `tests/decision_trace_audit.rs::expected_reasons` (or wherever the trace event-shape audit lives) to accept the new field as optional.
- **Patterns to follow:**
  - `AuraPending` / `DispelPending` / `InterruptPending` in `components/` — existing one-shot-component idiom
  - `pet_ai.rs:248, 286, 597` — existing one-shot-component despawn patterns
  - `EventPayload::Pet` in `events.rs:105-114` — current shape
- **Test scenarios:**
  - Happy path: A `PetCommand { ability: SpiderWeb, target: Warlock_entity }` is spawned on the Spider entity. On the next `pet_ai_system` tick, a SpiderWeb projectile is spawned, the `PetCommand` is removed, and the trace records a `pet_decision` with `outcome: chose(SpiderWeb, Some(Warlock), was_instant=true)` and `dispatched_by: Some(hunter_entity_id)`.
  - Edge case: Pet has a `PetCommand` but the commanded ability's cooldown is non-zero (race between Hunter dispatch and pet execution). Pet AI rejects the command, emits a `reject(OnCooldown)` trace event, and despawns the `PetCommand` to prevent re-execution next frame.
  - Edge case: Pet is in Heel mode (HP < 25%) AND has a `PetCommand`. Heel takes precedence: command is despawned without execution; trace emits `reject(LowHealthHeel)` (new rejection variant — see U6).
  - Determinism: `PetCommand` is a single-component-per-pet contract; if Hunter AI dispatches the same pet twice in one tick, the second `try_insert` is a no-op (`try_insert` overwrites in place per Bevy semantics — verify at implementation time).
  - Trace audit: `jq -c 'select(.kind == "pet_decision" and .dispatched_by != null)' trace.jsonl` returns the Hunter-dispatched events; pre-existing recipes that don't filter on `dispatched_by` continue to work because the field is optional.
- **Verification:** `cargo test --release` passes including `tests/decision_trace_audit.rs`. A headless test that manually spawns a `PetCommand` (via test harness) produces the expected pet_decision trace event.

### U4. Hunter AI `try_dispatch_*` helpers for pet headline abilities

- **Goal:** Three new helpers in `class_ai/hunter.rs` that decide when Hunter AI dispatches Spider Web, Boar Charge, or Master's Call to the pet (via `PetCommand`). Integration into the per-tick `decide_hunter_action` decision loop.
- **Requirements:** R3, R5 (headline ability dispatch, friendly-CC respect via pre_cast_ok).
- **Dependencies:** U2 (cooldown snapshot read), U3 (`PetCommand` exists).
- **Files:**
  - `src/states/play_match/class_ai/hunter.rs` (add `try_dispatch_spider_web`, `try_dispatch_boar_charge`, `try_dispatch_masters_call`; integrate into `decide_hunter_action`)
  - `src/states/play_match/class_ai/cast_guard.rs` (verify `pre_cast_ok` works for pet-as-caster; no changes expected per research)
- **Approach:**
  - Each new helper follows the pattern of existing `try_*` helpers in `hunter.rs` (e.g., `try_concussive_shot` at hunter.rs:371-432). Optimistic dispatch model: Hunter uses snapshot heuristics to decide whether to spawn the PetCommand; pet AI runs authoritative `pre_cast_ok` at execution time. Shape:
    1. Look up the pet entity via `ctx.combatants.get(&hunter_entity).and_then(|info| info.pet)` (new U2 field). If `None`, return false (Hunter has no pet — Felhunter doesn't reach this path because Hunter doesn't summon one).
    2. Read pet's `ability_cooldowns` from `ctx.combatants[&pet_entity].ability_cooldowns` (new U2 field). Check the relevant ability's cooldown is 0. If non-zero, `builder.reject(ability, RejectionReason::OnCooldown { remaining })`; return false.
    3. Read pet's position and the target's position from the snapshot. Check pet→target distance against the ability's `range` field. If out of range, `reject(OutOfRange)`; return false.
    4. Heel gate: `if ctx.combatants.get(&pet_entity).map(|info| info.health_pct() < 0.25).unwrap_or(true) { reject(LowHealthHeel); return false; }`.
    5. Friendly-CC heuristic: check the snapshot's `active_auras` for the target. If the target has a threshold-0 friendly CC (Polymorph, Freezing Trap stun, Sap), `reject(FriendlyBreakableCC)`; return false. This is a snapshot-based approximation of `pre_cast_ok`'s friendly-CC check — pet AI runs the authoritative check at execution time.
    6. If all snapshot checks pass: `commands.entity(pet_entity).try_insert(PetCommand { ability, target: target_entity })`. Emit a pet_decision event via a new `start_pet_dispatch_decision(decision_trace, pet_actor_view, target_view, hunter_entity, pet_type)` helper that sets `dispatched_by: Some(hunter_entity_id)` on the resulting `EventPayload::Pet`.
    7. Return true. Hunter's own ability dispatch this tick uses Hunter's GCD; pet's GCD is consumed by the pet's execution NEXT tick (after `apply_deferred` flushes the PetCommand spawn).
  - **Pet AI authoritative check at execution time** (in U3): when `pet_ai_system` reads the PetCommand, it runs full `pre_cast_ok` with the pet's live `&Combatant`. If conditions changed (e.g., cooldown advanced, pet's mana drained, target died), pet AI emits `reject(OnCooldown / InsufficientMana / NoValidTarget)`, despawns the command, and returns. This is the design — snapshot heuristics + authoritative execution check is the dispatch contract.
  - Integration into `decide_hunter_action`: try_dispatch helpers slot into the existing per-tick evaluation order. Reasonable ordering: try Hunter's own high-priority abilities first (Disengage on dead-zone, Aimed Shot on safe range), then evaluate pet ability dispatch in parallel. Pet ability dispatch is non-exclusive with Hunter's own cast (different GCD pools), so a single tick can fire both.
- **Patterns to follow:**
  - `try_concussive_shot` at hunter.rs:371 — `try_*` helper canonical shape
  - `try_aimed_shot` at hunter.rs:434 — example with friendly-CC `pre_cast_ok` integration
  - `pre_cast_ok` invocation pattern at hunter.rs:455
- **Test scenarios:**
  - Happy path (Spider): Hunter has a Spider pet at 28yd from an enemy. Spider Web is off cooldown. Hunter AI dispatches Spider Web via PetCommand; next tick the Spider executes it; the target gets the Root aura. (Covers AE2 from origin.)
  - Happy path (Boar): Hunter has a Boar pet. Enemy at 22yd is within Boar Charge range (25yd). Hunter dispatches BoarCharge; Boar gains ChargingState; target gets Stun aura on impact.
  - Happy path (Bird): Hunter has a Bird pet. Hunter has a MovementSpeedSlow aura. Hunter dispatches MastersCall on self; the slow aura is removed.
  - Edge case: Pet is in Heel (HP < 25%). `try_dispatch_spider_web` rejects with `LowHealthHeel`; no PetCommand spawned.
  - Edge case: Pet's Spider Web is on cooldown. `try_dispatch_spider_web` rejects with `OnCooldown`.
  - Edge case: Target is out of range of Spider Web from the pet's current position. `pre_cast_ok` returns false; helper emits `reject(OutOfRange)`.
  - Friendly-CC: Target is currently Polymorphed by an allied Mage. `pre_cast_ok`'s `check_friendly_cc` rejects Spider Web (Polymorph has threshold 0, fires the guard).
  - Note on AE3 (dropped per Key Technical Decisions): Spider Web's 80-damage break threshold is NOT covered by `has_friendly_breakable_cc`. No test scenario for "Hunter Aimed Shot crit breaks pet's Spider Web."
- **Verification:** `cargo test --release` passes. A headless Hunter v Warlock match (with the same seed used in pre-change trace) shows Spider Web firing in the match log within the first 15s. Decision trace shows pet_decision events with `dispatched_by: Some(hunter_id)` for each pet ability dispatch.

### U5. Remove Spider Web's owner-distance filter

- **Goal:** Spider Web's per-tick target search (in `spider_ai`) no longer applies the `dist_to_owner ≤ 15.0` filter. Spider Web's only spatial constraint becomes its own `range` field (currently 20yd, unchanged in this iteration).
- **Requirements:** R6.
- **Dependencies:** None (independent of U1-U4 but only meaningful once U1 has the Spider engaging targets). Could land in any order with U1-U4, but recommended last so trace audit can isolate its impact.
- **Files:**
  - `src/states/play_match/class_ai/pet_ai.rs` (delete the `dist_to_owner > 15.0` filter in `spider_ai` near line 412)
- **Approach:**
  - In the hybrid model, autonomous Spider Web casting from pet_ai's `spider_ai` becomes vestigial — Hunter AI owns the dispatch decision via U4. **However**, the filter removal still has scope:
    - If `spider_ai` retains a fallback autonomous decide path (e.g., for matches with no Hunter AI active, or as a safety net when no PetCommand is queued), the filter affects whether Spider Web fires autonomously at all.
    - **Recommended scope:** delete the autonomous Spider Web decide logic in `spider_ai` entirely (it's redundant with Hunter dispatch). What remains in `spider_ai` is just the Heel check + PetCommand reader + autonomous target assignment (per U1). The filter goes with the deleted code path.
  - If we choose to retain the autonomous Spider Web path as a fallback (in case Hunter is not dispatching), then the filter is removed per R6 while keeping the rest of the autonomous decide. Decision deferred to implementation (see Outstanding Questions).
- **Patterns to follow:**
  - `spider_ai` at pet_ai.rs:362 — current implementation
- **Test scenarios:**
  - With U4 not yet implemented (or PetCommand unavailable): Spider does not autonomously fire Spider Web at a target 30yd from Hunter (or no fallback path exists, in which case Spider Web simply doesn't fire from the autonomous path). Trace shows no SpiderWeb events from spider_ai.
  - With U4 implemented: Hunter dispatch flows through PetCommand and Spider Web fires correctly. Filter is no longer reachable.
- **Verification:** Reading `pet_ai.rs` confirms the `dist_to_owner > 15.0` check is gone. `cargo test --release` passes.

### U6. Heel predicate in pet AI (low-HP retreat)

- **Goal:** When a pet's `current_health / max_health < 0.25`, the pet enters Heel mode: target is cleared, ability execution is suppressed, pet returns to owner's flank via existing follow-owner movement.
- **Note on naming:** This is a stateless per-tick predicate, not a state machine with persisted entry/exit transitions. If pets later gain mid-match HP recovery (Mend Pet, party heals), the threshold check will thrash around 25% as HP oscillates. Adding hysteresis (e.g., retreat below 25%, re-engage above 40%) becomes appropriate at that point — out of scope today since no heal mechanism exists.
- **Requirements:** R7, R8, F3.
- **Dependencies:** U1 (pet target ownership), U3 (PetCommand handling — Heel suppresses execution).
- **Files:**
  - `src/states/play_match/class_ai/pet_ai.rs` (Heel check at the top of `pet_ai_system`'s per-pet loop)
  - `src/states/play_match/decision_trace/events.rs` (new `RejectionReason::LowHealthHeel` variant)
  - `tests/decision_trace_audit.rs` (extend `EXPECTED_REJECTION_REASONS` constant — note actual identifier is uppercase per audit-test source — for the new variant)
- **Approach:**
  - At the top of `pet_ai_system`'s per-pet loop (before PetCommand check and autonomous decide):
    ```text
    if combatant.current_health / combatant.max_health < 0.25 {
      combatant.target = None;
      // If a PetCommand is queued, despawn it.
      // Emit reject with the pet's headline ability per PetType:
      //   Spider    -> AbilityType::SpiderWeb
      //   Boar      -> AbilityType::BoarCharge
      //   Bird      -> AbilityType::MastersCall
      //   Felhunter -> AbilityType::SpellLock
      builder.reject(headline_ability_for(pet.pet_type), RejectionReason::LowHealthHeel);
      builder.finish();
      continue;
    }
    ```
  - Recovery: when HP rises above 25% (currently unreachable — no party heals exist, no Mend Pet), the predicate check fails on next tick and the pet resumes engagement. Forward-looking; not exercised today.
- **Patterns to follow:**
  - `pet_ai.rs:84-95` — existing top-of-loop guards (alive check, incapacitated check)
  - `decision_trace/events.rs:191-210` — `RejectionReason` variants
- **Test scenarios:**
  - Happy path (Heel entry): Spider takes damage that brings it below 25% HP. On the next pet_ai tick, the Spider's `combatant.target` is None, trace records `reject(LowHealthHeel)`, no PetCommand executes (if any was queued). Movement system routes Spider to "follow owner at 3yd" branch. (Covers AE5 from origin.)
  - Edge case: Hunter dispatches Spider Web on the same frame Spider drops below 25% HP. PetCommand is spawned by Hunter AI; pet AI's Heel check fires before PetCommand read; PetCommand is despawned without execution. (Covers AE6 from origin.)
  - Edge case (forward-looking, dead-code today): Spider drops to 24%, then a hypothetical heal raises it to 26% on a later tick. Pet target re-acquires from owner.target; ability dispatch becomes eligible again. **Skip this scenario for now** — no heal mechanism exists.
  - Determinism: Trace events for Heel entry are deterministic at the same seed.
- **Verification:** `cargo test --release` passes including the extended `expected_reasons` audit. A headless test that scripts pet HP to drop below 25% (via debug damage application) produces the expected Heel trace events.

### U8. Spider Web cooldown tune (45s → 20s)

- **Goal:** Reduce Spider Web's cooldown from 45s to 20s in `assets/config/abilities.ron` so the Hunter AI dispatch path can fire 2-3 times in an average 30s match. Boar Charge and Master's Call cooldowns stay at 45s in this iteration.
- **Requirements:** Supports R9 / R10 / R11 (validation actually exercises the architecture). Not a brainstorm requirement on its own — added per the doc-review decision to make Approach A falsifiable.
- **Dependencies:** None (RON data tweak); should land alongside U1-U6 before U7 validation.
- **Files:**
  - `assets/config/abilities.ron` (Spider Web `cooldown: 45.0` → `20.0`, near the SpiderWeb entry)
- **Approach:**
  - Single line change in the SpiderWeb ability definition. No code changes.
  - Document rationale in the commit message: "Approach A− carveout — only Spider Web CD tuned so validation can attribute matrix movement to AI architecture changes (U1-U6) rather than a broader cadence rebalance."
- **Patterns to follow:**
  - Prior balance commit pattern from PR #55 (`balance(hunter):`) — single-line tuning commits in `abilities.ron`
- **Test scenarios:**
  - Test expectation: existing `cargo test --release` continues to pass. `tests/ability_tests.rs` validates `cooldown >= 0` — 20.0 passes.
- **Verification:** `cargo build --release` succeeds. A headless Hunter v Warlock match shows Spider Web firing 2-3 times in a 30s match (vs 0-1 with 45s CD).

### U7. Validation: trace audit + 1v1/2v2 matrix sweeps + report

- **Goal:** Run the validation suite mirroring the mana iteration (PR #55). Confirm pet engagement moves the matrix needles and reduces SpiderWeb:NoValidTarget rejections.
- **Requirements:** R9, R10, R11.
- **Dependencies:** U1-U6 all merged.
- **Files:**
  - `design-docs/balance/matrix_baseline_2026-05-22_pet_engage_1v1_post.csv` and `.md` (new — post-change 1v1 matrix)
  - `design-docs/balance/matrix_baseline_2026-05-22_pet_engage_2v2_post.csv` and `.md` (new — post-change 2v2 matrix)
  - `docs/reports/2026-05-22-hunter-pet-engagement.md` (new — tuning report, mirrors `docs/reports/2026-05-22-hunter-mana-tuning.md` shape)
- **Approach:**
  - **Pre-change baseline reuse:** post-mana 1v1 baseline is `design-docs/balance/matrix_baseline_2026-05-22.csv` (N=20). Post-mana 2v2 baseline is `design-docs/balance/matrix_baseline_2026-05-22_2v2_post.csv` (N=10). Both serve as pre-change comparison for this iteration.
  - **Post-change 1v1 matrix:** `target/release/arenasim --matrix 20 --seed-base 0`. Move output to `design-docs/balance/matrix_baseline_2026-05-22_pet_engage_1v1_post.{csv,md}`.
  - **Post-change 2v2 matrix:** `scripts/hunter_2v2_matrix.sh 10 --seed-base 0 --out design-docs/balance/matrix_baseline_2026-05-22_pet_engage_2v2_post.csv`. Write markdown summary.
  - **Trace audit:** Hunter v Warlock with `--trace-mode on`. jq recipes:
    ```bash
    # Confirm SpiderWeb:NoValidTarget drops ≥75% from ~1,000 baseline
    jq -r 'select(.kind == "pet_decision") | .candidates[]? | select(.ability == "SpiderWeb" and .status == "rejected") | .reason | if type == "object" then keys[0] else . end' $T | sort | uniq -c
    # Confirm pet auto-attack damage events appear
    grep -c "Spider's Auto Attack hits" match_logs/match_*.txt
    # Confirm pet_decision events with dispatched_by attribution
    jq -c 'select(.kind == "pet_decision" and .dispatched_by != null) | {ability: .outcome.ability, dispatched_by}' $T | head -20
    # AE3 silent-debt detection: count Root auras applied to enemies and removed within K=2 seconds with a friendly Hunter damage event between
    # (correlation-based; relies on the trace including aura_applied and aura_removed events with cause attribution)
    jq -c 'select(.kind == "aura_applied" and .aura_type == "Root" and .source_class == "Hunter")' $T | head -20
    jq -c 'select(.kind == "aura_removed" and .aura_type == "Root" and .removal_cause == "FriendlyDamage")' $T | wc -l
    ```
  - **Tuning report:** Follow the shape of `docs/reports/2026-05-22-hunter-mana-tuning.md`. Compare pre/post 1v1 and 2v2 winrates per matchup; note the trace audit numbers; honest reading section flagging which matchups moved vs which didn't (likely Hunter v Mage still 0% due to 10s defeats, Hunter v Paladin still 0% due to no healer-CC).
- **Patterns to follow:**
  - `docs/reports/2026-05-22-hunter-mana-tuning.md` — report shape
  - `design-docs/balance/matrix_baseline_2026-05-22*.md` — baseline markdown shape
  - `CLAUDE.md` jq recipes for decision-trace audit
- **Test scenarios:**
  - Test expectation: none — produces measurement artifacts. Validation is whether the data matches success criteria:
    - Pet auto-attack damage events appear in ≥80% of Hunter matches (currently 0%)
    - SpiderWeb:NoValidTarget rejections drop by ≥75% from ~1,000 baseline
    - 1v1 Hunter aggregate non-mirror winrate moves from ~1.4% to ≥5% as a soft signal
    - At least 2 of 6 paired 2v2 matchups move from 0% to ≥10%
    - Non-Hunter matchups stay within ±5pp of post-mana baseline
- **Verification:** All three new artifact files committed. Report explicitly states whether each Success Criterion is met. If any criterion fails materially, surface as Residual Actionable Work — most likely candidate: pet ability cooldowns are still 45s (Approach B follow-up).

---

## System-Wide Impact

- **AI behavior:** Hunter pets now engage and Hunter dispatches headline pet abilities. Enemy AI behavior unchanged, but enemy winrates may shift because Hunter now applies more pressure. The U7 validation regression check (±5pp on non-Hunter matchups) is the safety net.
- **`acquire_targets` semantics:** pets are excluded; pet target ownership moves entirely to pet AI + Hunter dispatch. This is a behavioral change visible in decision traces (no `target_acquisition` events for pets anymore).
- **`CombatantInfo` field set:** the snapshot grows by `ability_cooldowns: BTreeMap`. Cheap clone; deterministic; no other AI currently reads it, but future Hunter / class AI may.
- **`EventPayload::Pet` shape:** gains optional `dispatched_by: Option<u32>`. Existing event consumers see `null`; existing jq recipes continue to work.
- **Determinism:** All new collections (BTreeMap snapshot, PetCommand reads ordered by query iteration) preserve determinism. `tests/headless_tests.rs::trace_on_matches_trace_off_outcomes` and `tests/headless_tests.rs::trace_file_is_deterministic_at_same_seed` continue to gate.
- **Test surface:**
  - New tests for `PetCommand` execution and Heel state (U3, U6) live in `tests/` adjacent to existing pet/AI tests
  - `tests/decision_trace_audit.rs::expected_reasons` updated for `LowHealthHeel` rejection variant (U6) and `dispatched_by` field (U3)
  - Existing tests untouched in spirit; some may require minor updates for the `CombatantInfo` field extension (U2)
- **Documentation:**
  - `CLAUDE.md` may benefit from a subsection under "Diagnose AI behaviour with the decision trace" describing the new `dispatched_by` field and how to filter pet decisions by source. Optional polish in U7's report commit.
  - New tuning report in `docs/reports/`. New balance baselines in `design-docs/balance/`.

---

## Scope Boundaries

- **Pet ability cooldown tuning beyond Spider Web** (Boar Charge 45s → ~18s, Master's Call 45s → ~25s) — out of scope. Only Spider Web's CD is tuned this iteration (45s → 20s, U8) to make Approach A's validation falsifiable.
- **Spider Web range tuning** (20yd → 30yd) — out of scope per origin.
- **Minion framework generalization** (lift to a shared subsystem usable by Warlock Felhunter and future pets) — out of scope per origin.
- **Pet HP / damage stat tuning** — out of scope.
- **Pet AI changes for Warlock Felhunter** — out of scope. Felhunter doesn't pursue today; the U1 acquire_targets skip applies to ALL pets, but Felhunter's existing ranged-utility decisions (Spell Lock, Devour Magic) work without `combatant.target`. Behavior should remain identical.
- **Other Hunter rebalance survivors** — Devour Magic counter, predictive trap placement, Disengage follow-through, team-comp awareness — all separately tracked in `docs/ideation/2026-05-22-hunter-rebalance-ideation.md`.
- **AE3 implementation** (Hunter Aimed Shot crit breaking Spider Web at 80-damage threshold) — dropped per Key Technical Decisions. Would require expanding `has_friendly_breakable_cc` to predict crit damage and check against numeric thresholds; affects Frost Nova too; out of scope for this iteration.
- **Forward-looking: Heel recovery exit** — when pets gain mid-match HP recovery (e.g., Mend Pet, party heals), the existing `< 0.25` predicate in U6 will naturally exit Heel when HP rises above the threshold. The HP check is implemented for this future case but cannot fire today (no party heals exist). Note: AE6 itself (dispatch suppression while in Heel) is fully implemented in U6 — see test scenario at U6.
- **Mend Pet ability** or any pet-heal mechanism — out of scope. Would make Heel recovery testable but is its own feature.

### Deferred to Follow-Up Work

- If U7 validation signal is muted because pet abilities still fire 0-1 times per match (45s CDs in 30s matches), the immediate follow-up is Approach B from the brainstorm: pet ability CD tuning + Spider Web range alignment. Runs through brainstorm → plan → work cycle.
- If pet survivability is poor (pets die before reaching the 25% threshold in most matches), revisit Heel threshold and/or pet base HP scaling.
- The autonomous Spider Web fallback path in `spider_ai` (whether to keep or delete) is a U5 implementation-time decision but could become a follow-up if there's value in autonomous pet ability use when Hunter is incapacitated.

---

## Dependencies / Assumptions

- **`pre_cast_ok` signature is unchanged.** Per Key Technical Decisions, Hunter AI does NOT call `pre_cast_ok` for pet ability dispatch — it uses snapshot heuristics instead. Pet AI runs the authoritative `pre_cast_ok` at execution time with live `&Combatant` (its existing signature). No call-site refactor required.
- **Assumes the `tests/decision_trace_audit.rs::expected_reasons` test gates new rejection variants and event-shape changes.** Per the research, the test file enumerates expected values; new `LowHealthHeel` variant and new `dispatched_by` field both need extensions.
- **Assumes existing target-pursuit movement at `movement.rs:391+` is sufficient for pets once they have a non-None target.** Confirmed by research — no special-case guards reject pets in the pursuit branch.
- **Hunter→pet entity lookup resolved via U2 snapshot field.** `CombatantInfo.pet: Option<Entity>` populated by both `CombatSnapshot::build` (combat_snapshot.rs) and `pet_ai_system`'s local build (pet_ai.rs:50-70). Hunter AI reads `ctx.combatants[&hunter_entity].pet` — O(1) lookup with no query plumbing changes.
- **Assumes pet HP recovery is dead code today.** Confirmed by research (no party heals, no Mend Pet, all single-target heals filter `is_pet`).
- **Assumes Hunter v Mage 10.5s defeats won't move from this change.** Pet doesn't have time to close on the Mage before Hunter dies; matchup gated by Hunter survivability, not pet engagement.

---

## Outstanding Questions (Deferred to Implementation)

- [Affects U5][Technical] **Whether to keep an autonomous Spider Web fallback in `spider_ai`** (firing when no PetCommand is queued) or delete the autonomous decide path entirely. Default to delete (Hunter owns dispatch); revisit if Hunter incapacitation scenarios need pet autonomy.
- [Affects U3][Technical] **`PetCommand` attached to pet entity vs spawned as global one-shot.** Both patterns exist in the codebase. Attached is slightly cleaner for the query side; global one-shot matches `AuraPending` precedent. Planner picks.
- [Affects U1][Needs verification] **Whether Felhunter's autonomous Spell Lock / Devour Magic logic continues to work correctly when pets are excluded from `acquire_targets`.** Research found Felhunter's decide functions use their own target search (not `combatant.target`), so the skip should be benign. Verify with a headless Warlock v Hunter match after U1 lands.
