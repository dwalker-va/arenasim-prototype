# Codebase Refactoring Brainstorm

**Date:** 2026-03-08
**Status:** Implemented
**Approach:** Prioritized backlog — one document covering all items, cherry-pick individually to plan/implement

## What We're Building

A series of targeted refactors to reduce duplication, improve file organization, and eliminate footguns across the codebase. Each item is independent and can be tackled incrementally.

## Why This Matters

As we add more classes and abilities, duplicated patterns compound maintenance burden. The codebase has grown organically through 16+ development sessions, and several copy-paste patterns have emerged that make adding new abilities error-prone. Oversized files slow navigation, and the dual system registration pattern has already caused silent bugs.

## Refactoring Backlog

### Tier 1: Low Effort, High Impact

#### 1. Extract Shared Dispel Logic (Priest/Paladin)
**Problem:** `try_cleanse` (paladin.rs) and `try_dispel_magic` (priest.rs) are ~80 lines of nearly identical code. Only differences: AbilityType variant, log prefix, caster class.
**Solution:** Extract `try_dispel_ally()` in `class_ai/mod.rs` parameterized by ability type, log prefix, and caster class.
**Estimated savings:** ~80 lines eliminated.

#### 2. Standardize `caster_id` Construction
**Problem:** Two patterns coexist for the same string — `combatant_id()` (27 uses) and inline `format!()` (15 uses). Warrior and Warlock files use `format!()` exclusively.
**Solution:** Find-and-replace all inline `format!("Team {} {}", ...)` with `combatant_id()`.
**Estimated savings:** Consistency, not line count.

#### 3. Add `CastingState::new()` Constructor
**Problem:** 11 identical CastingState construction blocks with `interrupted: false, interrupted_display_time: 0.0` boilerplate.
**Solution:** `CastingState::new(ability, cast_time, target)` constructor with sensible defaults.
**Estimated savings:** ~55 lines of boilerplate.

#### 4. Add `CombatContext::lowest_health_ally_below()`
**Problem:** 4+ copy-paste blocks for "find lowest HP ally within range below threshold X" across Priest and Paladin healing functions.
**Solution:** Extend `CombatContext` with `lowest_health_ally_below(threshold, max_range, pos)` that handles filtering, pet exclusion, and min_by.
**Estimated savings:** ~40 lines across heal functions.

#### 5. Remove Dead ClassAI Trait
**Problem:** `ClassAI` trait, 7 stub implementations, `AbilityDecision` enum, and `get_class_ai()` factory are never used at runtime. All real logic lives in standalone functions.
**Decision:** Remove it. The standalone-function approach works and is battle-tested.
**Estimated savings:** ~100 lines of dead code removed, eliminates confusion for new readers.

### Tier 2: Medium Effort, High Impact

#### 6. Migrate to `AuraPending::from_ability()` Everywhere
**Problem:** 18 manual `AuraPending` constructions, 17 of which could use the existing `from_ability()` helper (currently used in only 1 place). Each repeats ~12 fields with identical defaults like `fear_direction: (0.0, 0.0)`.
**Solution:** Migrate all 17 manual constructions to `from_ability()`. Add builder methods like `.with_spell_school(None)` for special cases (physical debuffs).
**Estimated savings:** ~200 lines of boilerplate.

#### 7. Add Shared `log_ability_use()` Helper
**Problem:** 45 nearly identical combat logging call sites constructing caster_id and target_id before calling `log_ability_cast()`.
**Solution:** `log_ability_use(combat_log, combatant, ability_name, target_entity, ctx)` helper.
**Estimated savings:** ~90 lines across 9 files.

#### 8. Unify Dual System Registration
**Problem:** Combat systems must be registered in both `states/mod.rs` (graphical) and `systems.rs` (headless). The graphical mode does NOT call `add_core_combat_systems()` — it duplicates the list and interleaves visual systems. Adding a combat system to only one location causes silent bugs.
**Solution:** Refactor `states/mod.rs` to call `add_core_combat_systems()` for combat, then layer graphical-only systems separately.
**Estimated savings:** Eliminates the #1 source of "works in headless, broken in graphical" bugs.

### Tier 3: Higher Effort, Organizational

#### 9. Split `components/mod.rs` (1642 lines)
**Problem:** Single file contains resources, combatant struct, aura types, casting state, pet components, visual components, and more.
**Solution:** Split into focused files: `resources.rs`, `combatant.rs`, `auras.rs`, `casting.rs`, `pets.rs`, `markers.rs`. Keep `mod.rs` as a thin re-export hub.
**Note:** Existing `auras.rs` and `visual.rs` submodules are documentation-only stubs, not actual type definitions.

#### 10. Split `combat_core.rs` (2672 lines)
**Problem:** Largest file in the codebase handles movement, auto-attacks, resource regen, casting, interrupts, stealth visuals, and damage application.
**Solution:** Split into `movement.rs`, `auto_attack.rs`, `casting.rs`, `damage.rs`, with `combat_core.rs` as glue/re-exports.

## Key Decisions

- **Prioritized backlog, not big-bang:** Each item is independent and can be planned/implemented individually
- **Remove ClassAI trait:** Delete dead code rather than completing an unused migration
- **Standardize on existing helpers:** `combatant_id()` and `AuraPending::from_ability()` already exist but are underused

## Risks and Constraints

- **Merge conflicts:** Items that touch many files (6, 7, 9, 10) should be done when no feature branches are in flight, or landed quickly to minimize conflict windows.
- **Item 6 scope:** If `AuraPending::from_ability()` doesn't cleanly handle all 17 cases, the builder method additions could expand. Cap it — if more than 2-3 builder methods are needed, the abstraction isn't right.
- **Item 8 ordering dependency:** Unifying system registration (item 8) should be done before file splits (9-10), since splitting files while also restructuring registration doubles the coordination cost.
- **Testing:** Every item should be validated with a headless match run to confirm no behavioral regressions.

## Suggested Implementation Order

Start with Tier 1 items (1-5) as they're low-risk and deliver immediate wins. Item 8 (dual registration) is medium effort but prevents the most impactful class of bugs. File splits (9-10) are lower priority since they're organizational, not behavioral.

A reasonable first batch: items 2, 3, 4, 5 (all low effort, independent). Then 1 and 6 together (both touch class_ai files). Then 8. Then 7, 9, 10 as time allows.
