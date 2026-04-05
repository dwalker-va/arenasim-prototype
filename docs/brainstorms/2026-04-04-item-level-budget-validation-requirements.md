---
date: 2026-04-04
topic: item-level-budget-validation
---

# Item Level Budget Validation

## Problem Frame

Items in `items.ron` are hand-authored with `item_level` as informational-only metadata. There is no enforcement that an item's stats are proportional to its item level and slot. This means over-tuned or under-tuned items can slip in silently, making balance unpredictable. A validation layer would catch authoring mistakes and keep the item pool internally consistent.

## Requirements

**Budget Formula**
- R1. Define a base stat budget as a function of item level (e.g., a linear or piecewise formula that converts ilvl to total budget points).
- R2. Define per-slot multipliers using WoW Classic-accurate values (e.g., chest/head = 1.0, legs = 0.875, ring/neck = 0.5625, wrist = 0.5, etc.) that scale the base budget for each slot.
- R3. The effective budget for an item = base_budget(item_level) * slot_multiplier(slot).

**Stat Cost Weights**
- R4. Each budgeted stat type has a defined cost-per-point weight (e.g., 1 point of crit_chance costs more budget than 1 point of attack_power). These weights determine how much budget each point of a stat consumes.
- R5. An item's total budget usage = sum of (stat_value * stat_weight) across all budgeted stats.

**Free Stats (Not Budgeted)**
- R6. Armor on armor pieces is free — it does not consume item level budget. Armor scales with item level and slot via its own formula (ilvl, slot, armor type).
- R7. Weapon DPS (attack_damage_min, attack_damage_max, attack_speed) on weapons is free — it does not consume item level budget. Weapon DPS scales with item level via its own formula.
- R8. All other stats (max_health, max_mana, mana_regen, attack_power, spell_power, crit_chance, movement_speed, and all elemental resistances) consume budget per their stat weights.

**Validation**
- R9. A dedicated Rust test validates every item in `items.ron` against its computed budget. The test loads all item configs, computes each item's budget usage, compares it to the item's effective budget, and fails if any item exceeds its budget.
- R10. The validation is test-only — it does not run at startup and does not prevent the game from launching.
- R11. The test should report which items are over-budget and by how much, for easy diagnosis.

## Success Criteria

- Running `cargo test` catches any item whose stats exceed its item level budget for its slot.
- The budget formula, slot multipliers, stat weights, and free-stat formulas are defined as data (constants or a config), not scattered through logic.
- Existing items in `items.ron` pass validation (weights and formulas are calibrated to the current item pool).

## Scope Boundaries

- No auto-generation of item stats from budgets — items remain hand-authored.
- No runtime enforcement or startup panics — validation is test-only.
- No UI changes — budget data is internal only.
- No changes to how stats are applied to combatants at runtime.

## Key Decisions

- **Weighted stat costs over equal costs**: Different stats have different power levels; crit_chance should cost more budget than max_health per point.
- **WoW-accurate slot multipliers**: Use Classic-era slot multipliers as baseline for authenticity and proven balance.
- **Armor + Weapon DPS are free**: These scale with ilvl/slot via their own formulas, separate from the stat budget. Resistances consume budget.
- **Test-only enforcement**: Keeps the feedback loop in CI/dev without blocking game launches during iteration.

## Outstanding Questions

### Deferred to Planning
- [Affects R1][Needs research] What specific base budget formula fits the current item pool? Linear, piecewise, or exponential scaling with ilvl? Should be calibrated against existing items.
- [Affects R4][Needs research] What are the appropriate stat cost weights? These should be derived by analyzing the current item pool and WoW Classic itemization references.
- [Affects R2][Needs research] Exact WoW Classic slot multiplier values for all 17 slots (including weapon slots — MainHand, OffHand, Ranged, Shield).
- [Affects R6][Needs research] What formula should armor use to scale with ilvl, slot, and armor type? Reference WoW Classic armor tables.
- [Affects R7][Needs research] What formula should weapon DPS use to scale with ilvl? Reference WoW Classic weapon DPS tables.
- [Affects R9][Technical] Where should the budget constants/config live — inline in the test module, a separate constants file, or a RON config?

## Next Steps

-> `/ce:plan` for structured implementation planning
