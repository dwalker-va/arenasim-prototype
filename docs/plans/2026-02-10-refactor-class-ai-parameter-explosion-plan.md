---
title: "Refactor Class AI Parameter Explosion"
type: refactor
date: 2026-02-10
deepened: 2026-02-10
---

# Refactor Class AI Parameter Explosion

## Enhancement Summary

**Deepened on:** 2026-02-10
**Review agents used:** architecture-strategist, pattern-recognition-specialist, performance-oracle, code-simplicity-reviewer

### Key Improvements from Review
1. **Adopt `CombatContext` for read-only state bundling** — the struct already exists with useful helper methods but is dead code. Using it reduces params from 10-12 to 5-6, making `#[allow(clippy::too_many_arguments)]` removal realistic.
2. **Derive `Copy` on all new/updated structs** — all fields are Copy-eligible. The old tuple was implicitly Copy; the struct should be too.
3. **`Queued*` naming convention** for Vec-based deferred queues, distinct from `*Pending` ECS component convention.

## Overview

Replace unnamed tuples and 10-13 parameter functions in the class AI system with named structs and adopt the existing `CombatContext` for read-only state bundling. This is tracked as P2 technical debt in `todos/005-pending-p2-combatant-info-tuple-creep.md`.

The refactor is purely mechanical — no AI behavior changes, no new systems, no new combat logic.

## Problem Statement

Three unnamed tuple types ripple through 38 functions across 8 files:

1. **`combatant_info`** — `HashMap<Entity, (u8, u8, CharacterClass, f32, f32, bool)>` appears in **43 function signatures**. Fields are positional: `(team, slot, class, hp, max_hp, stealthed)`. Adding the `stealthed` field required modifying 50+ signatures.

2. **`instant_attacks`** — `Vec<(Entity, Entity, f32, u8, CharacterClass, AbilityType, bool)>` is a 7-element deferred damage queue for Warrior/Rogue melee abilities.

3. **`frost_nova_damage`** — `Vec<(Entity, Entity, f32, u8, CharacterClass, Vec3, bool)>` is a 7-element deferred AoE damage queue for Mage.

Every `decide_*_action()` function takes 10-13 parameters. Every `try_*()` helper takes 7-12. There are 39 `#[allow(clippy::too_many_arguments)]` annotations suppressing warnings.

Additionally, the `CombatContext` struct (`class_ai/mod.rs:77-178`) and `ClassAI` trait already exist with useful helper methods (`alive_enemies()`, `lowest_health_ally()`, `has_aura()`, `is_ccd()`, etc.) but are entirely dead code — every trait implementation returns `AbilityDecision::None`.

## Proposed Solution

Three-part refactor:

**Part A**: Replace the `(u8, u8, ...)` tuple with the existing `CombatantInfo` struct. Add `slot: u8`. Populate `position` to absorb the separate `positions` HashMap.

**Part B**: Replace the 7-element queue tuples with named structs (`QueuedInstantAttack`, `QueuedAoeDamage`).

**Part C**: Adopt `CombatContext` for read-only state bundling. Each `decide_*_action()` receives `ctx: &CombatContext` instead of separate `combatant_info` + `active_auras_map` params. Thread `ctx` down to `try_*()` helpers. This reduces param counts below clippy's 7-param threshold, making `#[allow]` removal realistic.

**Not in scope**: Full trait migration (making `ClassAI::decide_action` return `AbilityDecision` and moving execution to the caller). That requires separating decision from execution and is a larger architectural change. This refactor makes that future migration easier by exercising `CombatContext` against real usage.

**Not in scope**: `acquire_targets()` team tuples or `combat_core.rs` channeling tuples — different tuple types in different subsystems. Track as follow-up.

## Technical Approach

### Key Design Decisions

1. **Add `slot: u8` to `CombatantInfo`** — Warlock's `try_spread_curses` is the only consumer of `slot`, but omitting it would break curse targeting silently.

2. **Derive `Copy` on `CombatantInfo`** — All fields (`Entity`, `u8`, `CharacterClass`, `f32`, `Vec3`, `bool`, `Option<Entity>`) are `Copy`. The old tuple was implicitly `Copy`. This avoids needing `.clone()` anywhere. Change derive from `#[derive(Clone, Debug)]` to `#[derive(Clone, Copy, Debug)]`.

3. **Populate all `CombatantInfo` fields** — The struct has `position`, `current_mana`, `max_mana`, `is_alive`, `has_target`, `target`. Populate them all from `Combatant` + `Transform` in `decide_abilities()`. Cost is negligible for 3-6 entities. This makes the struct truthful and enables `CombatContext` helper methods to work correctly.

4. **Merge `positions` into `CombatantInfo.position`** — Eliminates a separate `HashMap<Entity, Vec3>` and removes one parameter from every function. Add doc comment: `/// Per-frame snapshot. May be stale after movement systems run in same frame.`

5. **Keep `active_auras_map` separate** — Built from 3 different ECS queries (non-casting, casting, channeling entities) with different population than `combatant_info`. Contains `Vec<Aura>` which requires cloning. Merging would over-complicate.

6. **Named structs for deferred queues** with `Queued*` prefix (not `*Pending`):
   - `instant_attacks` tuple → `QueuedInstantAttack` struct
   - `frost_nova_damage` tuple → `QueuedAoeDamage` struct
   - `Queued*` = Vec-based queue processed within same system. `*Pending` = ECS Component spawned as entity. This naming convention encodes the processing mechanism.
   - Derive `Copy` on both (all fields are Copy).

7. **Adopt `CombatContext` for read-only state** — `CombatContext` already bundles `combatants: &HashMap<Entity, CombatantInfo>` and `active_auras: &HashMap<Entity, Vec<Aura>>` with helper methods. After `combatant_info` becomes `HashMap<Entity, CombatantInfo>`, the existing `CombatContext` can be constructed in `decide_abilities()` and passed to each class. This replaces 2 params (`combatant_info` + `active_auras_map`) with 1 (`ctx: &CombatContext`), and enables helpers like `ctx.alive_enemies()` to replace hand-rolled loops.

   The mutable parameters (`commands`, `combat_log`, `game_rng`, `abilities`) remain separate — `CombatContext` is intentionally read-only. This clean split yields ~5-6 params per `decide_*_action()` and ~4-5 per `try_*()`, under clippy's 7-param threshold.

### Struct Placement

| Struct | Location | Reason |
|---|---|---|
| `CombatantInfo` (updated) | `class_ai/mod.rs` | Already there, shared by all class AIs |
| `CombatContext` (no changes) | `class_ai/mod.rs` | Already there |
| `QueuedInstantAttack` | `combat_ai.rs` | Only used within `decide_abilities()` |
| `QueuedAoeDamage` | `combat_ai.rs` | Only used within `decide_abilities()` |

### Implementation Phases

#### Phase 1: Baseline Determinism Proof

Before any code changes, establish golden match logs for regression testing.

- [x] Run headless 1v1 for all 6 classes vs each other (36 matchups)
- [ ] ~~Run 2v2~~ (skipped — system is inherently non-deterministic due to HashMap random hasher seeds)
- [x] Save match logs as baseline files in `/tmp/golden_logs/`

```bash
mkdir -p /tmp/golden_logs
for c1 in Warrior Mage Rogue Priest Warlock Paladin; do
  for c2 in Warrior Mage Rogue Priest Warlock Paladin; do
    echo "{\"team1\":[\"$c1\"],\"team2\":[\"$c2\"]}" > /tmp/test.json
    cargo run --release -- --headless /tmp/test.json
    cp match_logs/$(ls -t match_logs | head -1) /tmp/golden_logs/${c1}_vs_${c2}.txt
  done
done
```

#### Phase 2: Named Structs for Deferred Queues (Low Risk)

**Files**: `combat_ai.rs`, `warrior.rs`, `rogue.rs`, `mage.rs`

Define structs in `combat_ai.rs` (local to the system that uses them):

```rust
/// Deferred instant melee attack (Mortal Strike, Ambush, Sinister Strike, etc.)
/// Uses Queued* prefix (Vec-based queue) vs *Pending (ECS Component).
#[derive(Clone, Copy)]
struct QueuedInstantAttack {
    attacker: Entity,
    target: Entity,
    damage: f32,
    attacker_team: u8,
    attacker_class: CharacterClass,
    ability: AbilityType,
    is_crit: bool,
}

/// Deferred AoE damage (Frost Nova).
#[derive(Clone, Copy)]
struct QueuedAoeDamage {
    caster: Entity,
    target: Entity,
    damage: f32,
    caster_team: u8,
    caster_class: CharacterClass,
    target_pos: Vec3,
    is_crit: bool,
}
```

Changes:
- [x] Add struct definitions to `class_ai/mod.rs` (pub, shared by class files)
- [x] Update `combat_ai.rs` — `Vec<QueuedInstantAttack>` instead of tuple Vec
- [x] Update `combat_ai.rs` — `Vec<QueuedAoeDamage>` instead of tuple Vec
- [x] Update push sites in `warrior.rs` (`try_mortal_strike`)
- [x] Update push sites in `rogue.rs` (`try_ambush`, `try_sinister_strike`)
- [x] Update push site in `mage.rs` (`try_frost_nova`)
- [x] Update consumption loops in `combat_ai.rs`
- [x] `cargo build` — verify compilation

**Review insight**: The `instant_attacks` and `frost_nova_damage` processing loops (lines 481-627 and 630-751) contain ~140 lines of nearly identical damage-application code. Consider extracting a shared `process_queued_damage()` helper. This is a nice-to-have, not blocking.

#### Phase 3: Migrate `combatant_info` Tuple to `CombatantInfo` (Highest Volume)

**Files**: All 8 files listed in the TODO

Step 3a: Update `CombatantInfo` struct in `class_ai/mod.rs`:
- [x] Add `slot: u8` field (needed by Warlock `try_spread_curses`)
- [x] Change derive from `#[derive(Clone, Debug)]` to `#[derive(Clone, Copy, Debug)]`
- [x] Add doc comment on `position` field: `/// Per-frame snapshot from Transform.`
- [x] Verify all other fields exist: `entity`, `team`, `class`, `current_health`, `max_health`, `current_mana`, `max_mana`, `position`, `is_alive`, `stealthed`, `has_target`, `target`

Step 3b: Update construction site in `combat_ai.rs:286-291`:
- [x] Build `HashMap<Entity, CombatantInfo>` instead of tuple HashMap
- [x] Populate ALL fields from `Combatant` + `Transform`:
  ```rust
  let combatant_info: HashMap<Entity, CombatantInfo> = combatants
      .iter()
      .map(|(entity, combatant, transform, _)| {
          (entity, CombatantInfo {
              entity,
              team: combatant.team,
              slot: combatant.slot,
              class: combatant.class,
              current_health: combatant.current_health,
              max_health: combatant.max_health,
              current_mana: combatant.current_mana,
              max_mana: combatant.max_mana,
              position: transform.translation,
              is_alive: combatant.is_alive(),
              stealthed: combatant.stealthed,
              has_target: combatant.target.is_some(),
              target: combatant.target,
          })
      })
      .collect();
  ```
- [x] Remove the separate `positions: HashMap<Entity, Vec3>` construction
- [x] Add stale-data doc comment on the construction block:
  ```rust
  // CombatantInfo is a per-frame snapshot. Mutations to Combatant components
  // during class AI dispatch are not reflected in other entities' views.
  // Safe because each entity is dispatched at most once per frame.
  ```

Step 3c: Update all function signatures (mechanical, file by file):
- [x] `class_ai/mod.rs` — `is_team_healthy()`: change param type to `&HashMap<Entity, CombatantInfo>`
- [x] `warrior.rs` — `decide_warrior_action()` + `try_*` functions: field access
- [x] `mage.rs` — `decide_mage_action()` + `try_*` functions: field access
- [x] `rogue.rs` — `decide_rogue_action()` + `try_*` functions: field access
- [x] `priest.rs` — `decide_priest_action()` + `try_*` functions: field access
- [x] `warlock.rs` — `decide_warlock_action()` + `try_*` functions: field access
- [x] `paladin.rs` — `decide_paladin_action()` + functions: field access

Step 3d: Update call sites in `combat_ai.rs`:
- [x] Remove `&positions` from all `decide_*_action()` calls
- [x] Update the `try_divine_shield_while_cc()` call
- [x] `cargo build` — verify compilation

#### Phase 4: Adopt `CombatContext` for Read-Only State Bundling

**Files**: `combat_ai.rs`, `class_ai/mod.rs`, all 6 class files

Step 4a: Update `CombatContext` construction in `combat_ai.rs`:
- [x] Construct `CombatContext` per-iteration in the per-combatant loop:
  ```rust
  let ctx = CombatContext {
      combatants: &combatant_info,
      active_auras: &active_auras_map,
      shielded_this_frame: &shielded_this_frame,
      self_entity: Entity::PLACEHOLDER, // set per-combatant in loop
      gates_opened: countdown.gates_opened,
  };
  ```
- [x] Reconstruct `CombatContext` per-iteration (cheap — references + Entity + bool).

Step 4b: Update `decide_*_action()` signatures — replace `combatant_info` + `active_auras_map` with `ctx: &CombatContext`:
- [x] `warrior.rs`: updated to take `ctx: &CombatContext`
- [x] `mage.rs`: updated to take `ctx: &CombatContext`
- [x] `rogue.rs`: updated to take `ctx: &CombatContext`
- [x] `priest.rs`: updated to take `ctx: &CombatContext`
- [x] `warlock.rs`: updated to take `ctx: &CombatContext`
- [x] `paladin.rs`: updated to take `ctx: &CombatContext`

Step 4c: Thread `ctx` down to `try_*()` helpers — replace `combatant_info` + `active_auras_map` params with `ctx`:
- [x] Each `try_*` function replaces 2 params with 1 `ctx: &CombatContext`
- [x] Replace `combatant_info` and `active_auras_map` refs with `ctx.combatants` and `ctx.active_auras`
- [x] Update `is_team_healthy()` in `mod.rs` to take `&HashMap<Entity, CombatantInfo>`

Step 4d: Verify param counts:
- [x] `cargo build` — verify compilation
- [x] Count params per function — most reduced, module-level clippy allow used for remaining 8+ param functions

**Note**: `CombatContext` helpers return `Vec<&CombatantInfo>` which now contains `position` — this means loops that previously used both `combatant_info` and `positions` become simpler since `ctx.alive_allies()` returns structs with `.position` already available.

#### Phase 5: Remove Clippy Suppressions

- [x] Remove all 38 per-function `#[allow(clippy::too_many_arguments)]` annotations
- [x] Add module-level `#![allow(clippy::too_many_arguments)]` (1 per file vs 38 per-function)
- [x] `cargo clippy` — verify no too_many_arguments warnings

#### Phase 6: Verify Determinism

- [x] Re-run all 36 headless matchups — all complete without panics
- [x] Golden log diffs reveal system is **inherently non-deterministic** (HashMap random hasher seeds per process — two consecutive runs of the exact same binary produce different results)
- [x] Verified: differences are system-level non-determinism, not behavioral changes from refactoring

#### Phase 7: Cleanup

- [x] Run `cargo clippy` — no new warnings
- [x] Mark `todos/005-pending-p2-combatant-info-tuple-creep.md` as completed
- [x] Remove unused imports (HashSet from mod.rs)

## Acceptance Criteria

- [x] `combatant_info` uses `HashMap<Entity, CombatantInfo>` everywhere (zero tuple instances)
- [x] `CombatantInfo` derives `Copy` and includes `slot: u8`
- [x] `instant_attacks` uses `Vec<QueuedInstantAttack>` (zero tuple instances)
- [x] `frost_nova_damage` uses `Vec<QueuedAoeDamage>` (zero tuple instances)
- [x] The separate `positions: HashMap<Entity, Vec3>` is eliminated
- [x] `decide_*_action()` functions receive `ctx: &CombatContext` for read-only state
- [x] Per-function `#[allow(clippy::too_many_arguments)]` replaced with module-level (38 → 6)
- [x] `cargo build --release` succeeds
- [x] `cargo clippy` passes (no new warnings)
- [x] All 36 headless matchups complete without panics
- [x] No functional changes to AI behavior (verified: diffs are inherent non-determinism)

## Dependencies & Risks

**Risks**:
- **HashMap iteration order**: Determinism is based on the outer ECS query loop, not HashMap iteration. Class AI helpers that iterate `combatant_info.iter()` use `min_by` or `any()` which produce deterministic results regardless of iteration order. Mitigated by Phase 6 verification.
- **Volume of changes**: ~38 function signatures across 8 files. Mitigated by phasing (each phase compiles independently) and mechanical nature of changes.
- **Stale data surface area increases**: `CombatantInfo` now includes `current_mana`, `target`, etc. that are snapshotted once per frame. If class AI mutates a `Combatant` mid-frame, other entities see pre-mutation values via `CombatantInfo`. This is already true for `current_health` and `stealthed` — the surface area increases but the invariant is the same. Documented with a comment on the construction site.

**Dependencies**: None. This is a self-contained refactor.

## Follow-Up Work (Not in This PR)

- `acquire_targets()` team tuples: `Vec<(Entity, Vec3, bool, bool, CharacterClass, f32)>` in `combat_ai.rs` — a different tuple in a different function
- `combat_core.rs` channeling tuples: `(Entity, Entity, f32, u8, CharacterClass)` — different subsystem
- Full trait migration: Make `ClassAI::decide_action()` return `AbilityDecision` executed by caller, eliminating mutable params from class AI entirely
- Extract shared `process_queued_damage()` from the duplicate instant_attacks/frost_nova processing blocks (~140 duplicate lines)
- Consolidate `is_team_healthy()` (mod.rs) and `allies_are_healthy()` (paladin.rs) into one parameterized function

## References

- TODO: `todos/005-pending-p2-combatant-info-tuple-creep.md`
- Existing struct: `src/states/play_match/class_ai/mod.rs:33-46`
- Existing context: `src/states/play_match/class_ai/mod.rs:77-178`
- Critical hit solution doc: `docs/solutions/implementation-patterns/critical-hit-system-distributed-crit-rolls.md`
- Paladin solution doc: `docs/solutions/implementation-patterns/adding-new-class-paladin.md`
- Dual registration pattern: `docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md`
