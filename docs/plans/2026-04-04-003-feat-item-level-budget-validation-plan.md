---
title: "feat: Add item level budget validation system"
type: feat
status: completed
date: 2026-04-04
origin: docs/brainstorms/2026-04-04-item-level-budget-validation-requirements.md
---

# feat: Add Item Level Budget Validation System

## Overview

Add a test-only validation layer that ensures hand-authored items in `items.ron` don't exceed their stat budget based on item level and slot. Budget is computed from a base formula scaled by WoW Classic-accurate slot multipliers, with weighted stat costs per stat type.

## Problem Frame

Items are hand-authored with `item_level` as informational-only metadata. There is no enforcement that stats are proportional to item level and slot. Over-tuned or under-tuned items slip in silently. A validation test catches authoring mistakes and keeps the item pool internally consistent. (see origin: `docs/brainstorms/2026-04-04-item-level-budget-validation-requirements.md`)

## Requirements Trace

- R1. Base stat budget as a function of item level
- R2. Per-slot multipliers using WoW Classic-accurate values
- R3. Effective budget = base_budget(item_level) * slot_multiplier(slot)
- R4. Weighted stat costs per stat type
- R5. Budget usage = sum of (stat_value * stat_weight)
- R6. Armor on armor pieces is free (excluded from budget sum)
- R7. Weapon DPS (attack_damage_min/max, attack_speed) is free (excluded from budget sum)
- R8. All other stats consume budget per their weights
- R9. Dedicated Rust test validates all items against budget
- R10. Test-only — no startup enforcement
- R11. Test reports all over-budget items with details

## Scope Boundaries

- No auto-generation of item stats — items remain hand-authored
- No runtime enforcement or startup panics
- No UI changes
- No separate armor or weapon DPS scaling formulas — free stats are simply excluded from the budget sum (see Key Technical Decisions)
- No changes to how stats are applied to combatants at runtime

## Context & Research

### Relevant Code and Patterns

- `ItemConfig` struct with all stat fields: `equipment.rs:228-299`
- `ItemSlot` enum (17 variants): `equipment.rs:26-44`
- `load_item_definitions()` loads from disk, usable in tests: `equipment.rs:481-495`
- `validate_class_restrictions()` existing validation function: `equipment.rs:341-365`
- `constants.rs` grouped constants with comment headers and doc comments: `constants.rs:1-210`
- Existing test helpers in `equipment.rs` mod tests: `equipment.rs:590+`
- Items range ilvl 54-60 with ~60 items across Plate, Mail, Leather, Cloth, Cloaks, Neck, Rings, Trinkets, Weapons, Shields, Off-hands

### Item Stat Patterns Observed

| Stat | Scale | Example Values | Notes |
|------|-------|---------------|-------|
| max_health | integer-scale | 3.0 - 20.0 | Ubiquitous across items |
| max_mana | integer-scale | 6.0 - 20.0 | Caster items |
| attack_power | integer-scale | 2.0 - 8.0 | Melee/hunter items |
| spell_power | integer-scale | 3.0 - 10.0 | Caster items |
| crit_chance | fraction | 0.01 - 0.02 | 0.01 = 1% |
| mana_regen | integer-scale | 1.0 | Rare, only on trinket |
| movement_speed | fraction | 0.1 - 0.15 | Boot items only |
| resistances | integer-scale | 15.0 - 25.0 | Resist-focused items |
| armor | integer-scale | 43.0 - 464.0 | Varies by armor type, FREE |
| attack_damage_min/max | integer-scale | 6.0 - 28.0 | Weapons only, FREE |
| attack_speed | fraction | 0.35 - 1.5 | Weapons only, FREE |

## Key Technical Decisions

- **Free stats = simple exclusion, not separate formulas**: The brainstorm review identified tension between "free stats need their own formulas" and "just exclude them from the budget sum." Since auto-generation is out of scope, simply excluding `armor`, `attack_damage_min`, `attack_damage_max`, and `attack_speed` from the budget calculation achieves the validation goal without researching WoW Classic armor/DPS tables. (Resolves review findings #2 and #7/#8)

- **Weapon bonus stats ARE budgeted**: Only the DPS triplet (attack_damage_min/max, attack_speed) is free on weapons. Other stats on weapons (attack_power, spell_power, crit_chance, max_mana) consume budget per R8. This resolves the ambiguity in R7.

- **Fractional stats get high weights**: `crit_chance` (0.01 = 1%) and `movement_speed` (0.1 = 10%) are stored as fractions. Their stat weights will be proportionally large to produce meaningful budget values (e.g., crit_chance weight ~500 so 0.02 crit = 10 budget points). This avoids a normalization layer.

- **Calibrate weights from the current item pool**: Derive stat weights by analyzing the existing ~60 items, ensuring existing items pass validation. Anchor relative weights to WoW Classic equivalency ratios (spell_power costs more than max_health), but calibrate the absolute scale to the current pool. (Addresses review finding #1 — the formula is principled, not purely circular)

- **10% tolerance band**: Hand-authored items naturally have slight budget variance. The test allows items to be up to 10% over their computed budget before flagging. This prevents brittle failures during tuning.

- **Collect-all-then-fail test pattern**: The test collects all violations across all items, then asserts with a single failure message listing every over-budget item and its overage. This serves R11's diagnostic goal.

- **Ring1/Ring2 and Trinket1/Trinket2 each independently use their slot-type multiplier**: The multiplier is per-slot, not per-slot-category.

- **Constants in constants.rs**: Budget formula, slot multipliers, and stat weights live as `pub const` values in `constants.rs`, following the existing grouping pattern. This keeps balance-tunable numbers centralized.

## Open Questions

### Resolved During Planning

- **Where should budget constants live?** In `constants.rs` following existing grouped-constant pattern.
- **Do weapons need slot multipliers?** Yes — weapons carry budgeted bonus stats (AP, SP, crit) that need a slot-relative budget cap.
- **How to handle movement_speed and crit_chance scale?** Use proportionally large stat weights rather than a normalization step.
- **Should free-stat formulas (armor, weapon DPS) be implemented?** No — just exclude those fields from the budget sum. Formulas are only needed for auto-generation, which is out of scope.

### Deferred to Implementation

- **Exact stat weight values**: Must be derived by analyzing the full item pool during implementation. The plan provides the framework; the implementer calibrates by computing budget usage across all items and adjusting weights until existing items pass within the tolerance band.
- **Exact base budget formula coefficient**: Linear scaling `item_level * K` where K is calibrated to the item pool.
- **Exact WoW Classic slot multiplier values**: Well-documented but should be verified against WoW Classic community references during implementation.

## High-Level Technical Design

> *This illustrates the intended approach and is directional guidance for review, not implementation specification. The implementing agent should treat it as context, not code to reproduce.*

```
// Budget computation flow:
//
// For each item in items.ron:
//   1. effective_budget = item_level * BUDGET_PER_ILVL * slot_multiplier(slot)
//   2. budget_usage = sum over budgeted stats of (stat_value * stat_weight)
//      - Budgeted stats: max_health, max_mana, mana_regen, attack_power,
//        spell_power, crit_chance, movement_speed, + all 6 resistances
//      - Excluded (free): armor, attack_damage_min, attack_damage_max, attack_speed
//   3. if budget_usage > effective_budget * (1.0 + BUDGET_TOLERANCE):
//        flag as over-budget
//
// Slot multipliers (WoW Classic-accurate):
//   Head=1.0, Chest=1.0, Legs=0.875, Shoulders=0.75, Hands=0.75,
//   Feet=0.75, Waist=0.625, Neck=0.5625, Back=0.5625, Ring=0.5625,
//   Trinket=0.5625, Ranged=0.5625, Wrists=0.5, MainHand=varies, OffHand=0.5
```

## Implementation Units

- [x] **Unit 1: Define budget constants in constants.rs**

  **Goal:** Add all budget-related constants — base budget formula coefficient, slot multipliers, stat weights, and tolerance — to the centralized constants file.

  **Requirements:** R1, R2, R4

  **Dependencies:** None

  **Files:**
  - Modify: `src/states/play_match/constants.rs`

  **Approach:**
  - Add a new `// Item Budget` section following the existing comment-header grouping style
  - Define `BUDGET_PER_ILVL: f32` — the linear scaling coefficient
  - Define `BUDGET_TOLERANCE: f32 = 0.10` — 10% over-budget tolerance
  - Define a `pub fn slot_budget_multiplier(slot: ItemSlot) -> f32` function with a match on all 17 ItemSlot variants, returning WoW Classic-accurate multipliers
  - Define stat weight constants: `WEIGHT_MAX_HEALTH`, `WEIGHT_MAX_MANA`, `WEIGHT_MANA_REGEN`, `WEIGHT_ATTACK_POWER`, `WEIGHT_SPELL_POWER`, `WEIGHT_CRIT_CHANCE`, `WEIGHT_MOVEMENT_SPEED`, `WEIGHT_RESISTANCE` (single weight for all 6 resistance types)
  - Import `ItemSlot` from `super::equipment::ItemSlot` (or wherever the mod path resolves)
  - Calibrate weights by analyzing the item pool: compute budget usage for each item with trial weights, adjust until all current items pass within tolerance

  **Patterns to follow:**
  - `constants.rs` existing section headers (e.g., `// ============================================================================`)
  - `constants.rs` doc comment style on each constant
  - `ItemSlot::all()` for canonical slot enumeration

  **Test scenarios:**
  - Happy path: `slot_budget_multiplier` returns correct value for Head (1.0), Wrists (0.5), Ring1 (0.5625)
  - Edge case: Ring1 and Ring2 return the same multiplier; Trinket1 and Trinket2 return the same multiplier
  - Happy path: All stat weight constants are positive

  **Verification:**
  - All budget constants compile and are accessible from the equipment module's test block

- [x] **Unit 2: Add budget calculation and validation function in equipment.rs**

  **Goal:** Add a function that computes an item's budget usage and checks it against the item's effective budget, returning a detailed result.

  **Requirements:** R3, R5, R6, R7, R8

  **Dependencies:** Unit 1

  **Files:**
  - Modify: `src/states/play_match/equipment.rs`

  **Approach:**
  - Add `pub fn calculate_budget_usage(item: &ItemConfig) -> f32` that sums `stat_value * weight` across all budgeted stat fields. Explicitly skip `armor`, `attack_damage_min`, `attack_damage_max`, `attack_speed`.
  - Add `pub fn calculate_effective_budget(item: &ItemConfig) -> f32` that returns `item.item_level as f32 * BUDGET_PER_ILVL * slot_budget_multiplier(item.slot)`.
  - Add `pub fn validate_item_budget(name: &str, item: &ItemConfig) -> Result<(), String>` that calls both, compares with tolerance, returns descriptive error on over-budget.
  - Place these alongside `validate_class_restrictions` in the validation section of the file

  **Patterns to follow:**
  - `validate_class_restrictions()` at `equipment.rs:341-365` for function signature and error message style
  - Import budget constants from `super::constants`

  **Test scenarios:**
  - Happy path: Item within budget returns Ok(())
  - Happy path: Item exactly at budget returns Ok(())
  - Edge case: Item at budget * 1.09 (within 10% tolerance) returns Ok(())
  - Error path: Item at budget * 1.15 (exceeds tolerance) returns Err with item name and overage details
  - Happy path: Armor field is excluded from budget — an item with high armor but low stats passes
  - Happy path: Weapon DPS fields are excluded — a weapon with high damage but low bonus stats passes
  - Edge case: Item with zero budgeted stats (hypothetical) returns Ok (0 <= any positive budget)
  - Edge case: Item with item_level 0 has effective budget 0, any stats trigger over-budget

  **Verification:**
  - Unit tests pass for the validation function with hand-constructed items
  - Function correctly excludes free stats and includes all budgeted stats

- [x] **Unit 3: Add full item pool validation test**

  **Goal:** Add a test that loads all items from `items.ron` and validates each against its budget, collecting and reporting all violations.

  **Requirements:** R9, R10, R11

  **Dependencies:** Unit 1, Unit 2

  **Files:**
  - Modify: `src/states/play_match/equipment.rs` (mod tests block)

  **Approach:**
  - Add `#[test] fn all_items_within_budget()` to the existing mod tests block
  - Call `load_item_definitions().expect("items.ron must load")` to get real item data
  - Iterate all items, calling `validate_item_budget` on each
  - Collect all Err results into a Vec
  - Assert the Vec is empty, printing all violations in the failure message with item name, budget usage, effective budget, and overage percentage
  - This is test-only — no startup validation, no Bevy system registration needed

  **Patterns to follow:**
  - Existing tests in `equipment.rs:590+` for mod tests structure
  - `load_item_definitions()` direct call pattern (same as ability config tests)

  **Test scenarios:**
  - Integration: All ~60 items in items.ron pass budget validation (this IS the main test)
  - The test output on failure lists every over-budget item with its name, budget usage, effective budget, and overage %

  **Verification:**
  - `cargo test all_items_within_budget` passes with the current item pool
  - Temporarily making an item wildly over-budget in items.ron causes the test to fail with a clear diagnostic message naming that item

## System-Wide Impact

- **Interaction graph:** No runtime impact. Budget validation is purely a test-time check. No Bevy systems, no graphical/headless registration needed.
- **Error propagation:** Test failures surface through `cargo test` only.
- **State lifecycle risks:** None — no runtime state is modified.
- **API surface parity:** No API changes. The validation functions are `pub` for testability but have no runtime callers.
- **Unchanged invariants:** `load_item_definitions()`, `validate_class_restrictions()`, item stat application via `apply_equipment()`, and all runtime combat behavior remain unchanged.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Stat weight calibration is iterative and may take multiple passes | Start with WoW Classic equivalency ratios, adjust until all items pass. The 10% tolerance provides buffer. |
| Resistance-heavy items (CloakOfFrostWarding, BandOfElementalResistance) may be outliers | If resist items consistently exceed budget, adjust resistance weight downward or increase tolerance for resist-focused items. |
| Future items may be harder to balance within the budget | The budget system catches mistakes, not edge cases. Tolerance band and adjustable weights provide flexibility. |

## Sources & References

- **Origin document:** [docs/brainstorms/2026-04-04-item-level-budget-validation-requirements.md](docs/brainstorms/2026-04-04-item-level-budget-validation-requirements.md)
- Related code: `src/states/play_match/equipment.rs` (ItemConfig, ItemSlot, validation functions, tests)
- Related code: `src/states/play_match/constants.rs` (centralized constants pattern)
- Related data: `assets/config/items.ron` (all item definitions, ~60 items, ilvl 54-60)
