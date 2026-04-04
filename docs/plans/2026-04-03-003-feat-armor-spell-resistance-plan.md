---
title: "feat: Add armor and spell resistance damage mitigation"
type: feat
status: completed
date: 2026-04-03
origin: docs/brainstorms/2026-04-03-armor-spell-resistance-requirements.md
---

# feat: Add Armor and Spell Resistance Damage Mitigation

## Overview

Add armor and per-school spell resistance stats that reduce incoming damage based on WoW Classic formulas. Armor reduces Physical damage; each of six spell resistance stats reduces damage from its corresponding magic school. Values come from equipment, with resistance also grantable via aura buffs. The damage pipeline gains a new mitigation layer between crit and existing percentage aura reductions.

## Problem Frame

All combatants currently take identical raw damage regardless of armor type or class fantasy. A Warrior in full plate absorbs a Frostbolt the same as a Mage in cloth. Only aura-based mitigation (Devotion Aura, Curse of Weakness, absorb shields) exists. Adding stat-based mitigation creates meaningful class differentiation in survivability. (see origin: docs/brainstorms/2026-04-03-armor-spell-resistance-requirements.md)

## Requirements Trace

- R1. `armor` stat on combatants reduces Physical damage
- R2. Armor formula: `Reduction % = Armor / (Armor + 5500)`
- R3. Armor values from equipment based on `armor_type` and `item_level`
- R4. Total armor = sum of equipped items. No base class armor
- R5. Pipeline ordering: armor/resistance → DamageTakenReduction → absorb shields. Both auto-attack and ability paths
- R6. Shields contribute armor
- R7. Per-school resistance stats: Fire, Frost, Shadow, Arcane, Nature, Holy
- R8. Resistance formula: `Reduction % = Resistance / (Resistance * 5/3 + 300)` (level 60)
- R9. Deterministic average mitigation, no partial resist rolls
- R10. Resistance summed from all sources (equipment + auras) per school
- R11. Resistance applies to matching spell school including DoT ticks
- R12. `armor` field on `ItemConfig`
- R13. Optional resistance fields on `ItemConfig`
- R14. Populate armor values for all existing items
- R15. Existing aura reductions unchanged, armor/resistance applies before them
- R16. `SpellResistanceBuff` AuraType for resistance buffs
- R17. Resistance from auras stacks additively with equipment

## Scope Boundaries

- **Not in scope:** Armor penetration / spell penetration
- **Not in scope:** Partial resist RNG rolls
- **Not in scope:** Talent system
- **Not in scope:** Resistance-specific UI beyond stats panel values
- **Not in scope:** Rebalancing ability damage numbers (follow-up tuning pass)

## Context & Research

### Relevant Code and Patterns

- **Damage chokepoint:** `apply_damage_with_absorb(damage, target, active_auras)` in `combat_core/damage.rs:23` — all 8 damage paths funnel through this. Currently lacks SpellSchool parameter
- **Call sites (8):** `auto_attack.rs:234`, `casting.rs:300`, `casting.rs:907` (Drain Life), `combat_ai.rs:601` (Charge), `combat_ai.rs:754` (AoE), `holy_shock.rs:150`, `projectiles.rs:236`, `auras.rs:681` (DoT ticks)
- **Caster-side reductions:** `DamageReduction` (Curse of Weakness) applied at call sites before `apply_damage_with_absorb` — modifies outgoing damage on attacker. Armor/resistance is target-side and belongs inside the chokepoint
- **Equipment flow:** `ItemConfig` → `items.ron` → `resolve_loadout()` → `apply_equipment()` on `Combatant` → stat accumulation. `EquipmentBonuses::from_loadout()` mirrors for UI
- **SpellSchoolLockout pattern:** `AuraType::SpellSchoolLockout` uses Aura's `spell_school` field + `magnitude`. Same pattern for `SpellResistanceBuff`
- **DoT ticks:** `auras.rs:681` passes `aura.magnitude` as damage. Aura struct has `spell_school: Option<SpellSchool>` — school available at tick time
- **Stats panel:** `view_combatant_ui.rs` — `ClassStats` struct (line 148) and `EquipmentBonuses` struct (line 160) need armor/resistance fields. `render_stats_panel` (line 828) renders stat rows
- **Combat log:** `log.rs` `StructuredEventData::Damage` has no mitigation field. Human-readable message is a free-form string

### Institutional Learnings

- **Signature cascade warning** (from `critical-hit-system-distributed-crit-rolls.md`): Adding `is_crit` to `log_damage()` broke 17 test calls and 11 production calls. Expect similar cascade when adding `SpellSchool` to `apply_damage_with_absorb`. Plan for updating all 8 call sites + test assertions
- **Dual registration required** (from `graphical-mode-missing-system-registration.md`): New systems must register in both `systems.rs` (headless) and `states/mod.rs` (graphical). For this feature, no new systems needed — changes are in existing damage functions
- **Existing `characters.ron`:** Already defines `armor` and `magic_resistance` per class (Warrior: 50/20, Mage: 0/30, etc.) but these are unused. Must be removed per R4 decision

## Key Technical Decisions

- **Add `SpellSchool` parameter to `apply_damage_with_absorb`:** Armor and resistance are target-side reductions that depend on knowing the spell school. Adding the parameter to the chokepoint function ensures uniform application across all 8 damage paths. Each call site already has access to the spell school via ability config or aura struct. This follows the same cascade pattern as the crit_chance addition (see institutional learning above). (see origin: R5, R11)

- **Armor values via `armor_type_multiplier * item_level` formula:** Rather than manually looking up each item, derive armor from the existing `armor_type` and `item_level` fields using per-type multipliers. Target mitigation ranges across 8 body armor slots:

  | Armor Type | Multiplier | At ilvl 58 (per piece) | 8 pieces total | Reduction % |
  |------------|-----------|----------------------|---------------|-------------|
  | Plate      | 5.0       | 290                  | 2320          | 29.7%       |
  | Mail       | 2.7       | 157                  | 1253          | 18.6%       |
  | Leather    | 1.7       | 99                   | 789           | 12.5%       |
  | Cloth      | 0.8       | 46                   | 371           | 6.3%        |
  | Shield     | 8.0       | 464 (single piece)   | +464          | +~5% on top |

  These produce WoW-authentic differentiation. Plate Warrior gets ~30% physical reduction, cloth Mage gets ~6%. Paladin with plate + shield reaches ~34%. (see origin: R3, R14)

- **Resistance formula confirmed:** `Resistance / (Resistance * 5/3 + 300)` is the standard WoW Classic level-60 average resistance formula from community research (wowwiki, classic.wowhead). At 0 resistance = 0% reduction. At 75 resistance = 13.0%. At 150 resistance = 20.0%. At 300 resistance = 28.6%. Diminishing returns built in. (see origin: R8)

- **Pipeline ordering (multiplicative stacking):** `Raw → Crit → Armor/Resistance → DamageTakenReduction auras → Absorb shields → Health`. Armor/resistance is inserted as the first step inside `apply_damage_with_absorb`, before the existing `DamageTakenReduction` loop. (see origin: pipeline ordering decision)

- **`SpellResistanceBuff` AuraType:** Single variant using existing Aura `spell_school` field for school identification and `magnitude` for resistance amount. Follows `SpellSchoolLockout` pattern. (see origin: resistance aura variant decision)

- **Remove `characters.ron` armor/magic_resistance:** These unused fields conflict with the equipment-derived-only approach (R4). Remove them during implementation. (see origin: characters.ron decision)

## Open Questions

### Resolved During Planning

- **Armor-per-slot values:** Derived via `armor_type_multiplier * item_level` formula (see Key Technical Decisions table). Produces WoW-authentic ranges without needing per-item Wowhead lookups
- **Resistance formula:** Confirmed as `R / (R * 5/3 + 300)` — standard community-researched Classic formula
- **Pipeline integration approach:** Add `SpellSchool` parameter to `apply_damage_with_absorb`, apply armor/resistance as first reduction inside the function
- **Cloth-wearer survivability:** At 6.3% physical reduction, cloth wearers are only slightly squishier than current (0% reduction). The armor system adds differentiation without making cloth classes unviable. Cloth classes retain their existing defensive tools (absorb shields, CC, kiting)
- **Accessory slots (Back, Neck, Ring, Trinket):** These items have no `armor_type` field in `items.ron` and get `armor: 0.0` by default. Only the 8 body slots + shields contribute armor — consistent with WoW Classic
- **Spell school for Charge/AoE paths:** `QueuedInstantDamage` and `QueuedAoeDamage` both have `ability: AbilityType` field — spell school looked up via `abilities.get_unchecked(&ability).spell_school` at the call site

### Deferred to Implementation

- **Exact resistance values on items:** Most current items won't have resistance stats initially. Resistance will primarily come from aura buffs (R16). Resistance values can be added to specific items in a follow-up pass
- **Combat log format for mitigation:** The human-readable log message can include mitigated amounts (e.g., "[DMG] Warrior hits Mage for 45 (12 mitigated)") but exact formatting is an implementation-time choice

## High-Level Technical Design

> *This illustrates the intended approach and is directional guidance for review, not implementation specification. The implementing agent should treat it as context, not code to reproduce.*

```
Damage Pipeline (updated):

  Raw Damage (from ability/auto-attack)
    ↓
  Crit Roll (2x multiplier)
    ↓
  Caster-side reductions [at call site, unchanged]
    - DamageReduction (Curse of Weakness, outgoing)
    - Divine Shield penalty (outgoing)
    ↓
  ══════════════════════════════════════════════
  apply_damage_with_absorb(damage, target, auras, spell_school)  ← NEW parameter
  ══════════════════════════════════════════════
    ↓
  DamageImmunity check (Divine Shield on target) [unchanged]
    ↓
  NEW → Armor/Resistance reduction:
    if spell_school == Physical:
      reduction = target.armor / (target.armor + 5500)
    else if spell_school is magical:
      resistance = target.{school}_resistance + sum(SpellResistanceBuff auras for school)
      reduction = resistance / (resistance * 5/3 + 300)
    damage *= (1.0 - reduction)
    ↓
  DamageTakenReduction auras (Devotion Aura) [unchanged]
    ↓
  Absorb shields [unchanged]
    ↓
  Apply to health
```

## Implementation Units

- [ ] **Unit 1: Add armor and resistance fields to Combatant and ItemConfig**

**Goal:** Establish the data model for armor and resistance stats on both items and combatants

**Requirements:** R1, R7, R12, R13

**Dependencies:** None

**Files:**
- Modify: `src/states/play_match/components/combatant.rs` — add `armor: f32`, `fire_resistance: f32`, `frost_resistance: f32`, `shadow_resistance: f32`, `arcane_resistance: f32`, `nature_resistance: f32`, `holy_resistance: f32` fields to `Combatant` struct. Initialize all to `0.0` in `new()`. Add accumulation in `apply_equipment()`
- Modify: `src/states/play_match/equipment.rs` — add `armor: f32` and six resistance fields to `ItemConfig`. Default to `0.0`
- Modify: `assets/config/characters.ron` — remove unused `armor` and `magic_resistance` fields from base_stats
- Test: `src/states/play_match/combat_core/damage_tests.rs` (or existing test file)

**Approach:**
- Add fields to both structs with `0.0` defaults so existing RON config files parse without changes
- In `apply_equipment()`, accumulate armor and resistance the same way other stats are accumulated (additive)
- Remove `armor`/`magic_resistance` from `characters.ron` per the key decision
- Follow the pattern used when `crit_chance` was added (field + helper + constants)

**Patterns to follow:**
- `apply_equipment()` in `combatant.rs:362` — existing stat accumulation pattern
- `Combatant::new()` class-specific stat initialization

**Test scenarios:**
- Happy path: Combatant with plate equipment has armor > 0 after `apply_equipment()`
- Happy path: Combatant with no equipment has armor == 0 (R4)
- Happy path: Multiple items' armor values sum correctly
- Edge case: Item with no armor field defaults to 0.0

**Verification:**
- `cargo build --release` succeeds
- Existing tests pass unchanged (new fields default to 0.0)

---

- [ ] **Unit 2: Populate armor values for all items in items.ron**

**Goal:** Add numeric armor values to all existing equipment items using the type-multiplier formula

**Requirements:** R3, R6, R14

**Dependencies:** Unit 1

**Files:**
- Modify: `assets/config/items.ron` — add `armor:` field to every item that has an `armor_type`

**Approach:**
- Apply formula: `armor = armor_type_multiplier * item_level`
- Multipliers: Plate=5.0, Mail=2.7, Leather=1.7, Cloth=0.8
- Shields (OffHand with `weapon_type: Shield`): multiplier 8.0
- Items without `armor_type` (cloaks, necks, rings, trinkets, weapons, offhand frills): no armor field needed (defaults to 0.0)
- Round to nearest integer for cleanliness

**Patterns to follow:**
- Existing `items.ron` field ordering and formatting

**Test scenarios:**
- Happy path: Plate item at ilvl 60 gets `armor: 300.0` (5.0 × 60)
- Happy path: Cloth item at ilvl 56 gets `armor: 45.0` (0.8 × 56, rounded)
- Happy path: Shield at ilvl 58 gets `armor: 464.0` (8.0 × 58)
- Happy path: Accessory items (rings, necks, trinkets, cloaks) have no armor field

**Verification:**
- `cargo build --release` succeeds (RON parses)
- Spot-check a few items from each armor type to validate formula application

---

- [ ] **Unit 3: Add SpellSchool to apply_damage_with_absorb and implement armor reduction**

**Goal:** Thread spell school through the damage chokepoint and apply armor-based Physical damage reduction

**Requirements:** R1, R2, R5, R15

**Dependencies:** Unit 1

**Files:**
- Modify: `src/states/play_match/combat_core/damage.rs` — add `spell_school: SpellSchool` parameter to `apply_damage_with_absorb()`. Add armor reduction logic for `SpellSchool::Physical` between DamageImmunity check and DamageTakenReduction loop
- Modify: `src/states/play_match/combat_core/auto_attack.rs` — pass `SpellSchool::Physical` at call site (line 234)
- Modify: `src/states/play_match/combat_core/casting.rs` — pass ability's `spell_school` at call sites (lines 300, 907)
- Modify: `src/states/play_match/combat_ai.rs` — pass ability spell school at Charge (line 601) and AoE (line 754) call sites. Look up via `abilities.get_unchecked(&ability).spell_school`
- Modify: `src/states/play_match/effects/holy_shock.rs` — pass `SpellSchool::Holy` at call site (line 150)
- Modify: `src/states/play_match/projectiles.rs` — pass ability spell school at call site (line 236). Look up via `abilities.get_unchecked(&ability).spell_school`
- Modify: `src/states/play_match/auras.rs` — pass `aura.spell_school.unwrap_or(SpellSchool::None)` at DoT tick call site (line 681)
- Test: existing test file for `apply_damage_with_absorb` tests

**Approach:**
- Insert armor reduction as the first operation inside `apply_damage_with_absorb`, after `DamageImmunity` check, before `DamageTakenReduction` loop
- For `SpellSchool::Physical`: `let reduction = target.armor / (target.armor + 5500.0); remaining_damage *= 1.0 - reduction;`
- For `SpellSchool::None` or non-Physical schools: skip armor reduction (spell resistance handled in Unit 4)
- Update all 8 call sites to pass the appropriate `SpellSchool`
- Expect test compilation failures — update all existing test calls to pass a `SpellSchool` parameter

**Patterns to follow:**
- Existing `DamageTakenReduction` loop inside `apply_damage_with_absorb` for reduction pattern
- Crit system institutional learning for managing the signature cascade across call sites and tests

**Test scenarios:**
- Happy path: Physical damage to target with 2000 armor is reduced by ~26.7%
- Happy path: Physical damage to target with 0 armor is not reduced
- Happy path: Magical damage (e.g., Frost) is NOT reduced by armor
- Edge case: DamageImmunity still blocks all damage before armor applies
- Integration: Armor reduction stacks multiplicatively with DamageTakenReduction — 30% armor + 10% DamageTakenReduction = ~37% total reduction, not 40%
- Integration: Absorb shields absorb post-armor damage, not pre-armor

**Verification:**
- `cargo build --release` succeeds
- All existing tests pass (with updated SpellSchool parameter)
- New armor reduction tests pass
- Headless simulation: Warrior vs Warrior shows reduced damage compared to pre-armor baseline

---

- [ ] **Unit 4: Add spell resistance reduction to damage pipeline**

**Goal:** Apply per-school spell resistance reduction for magical damage inside the damage chokepoint

**Requirements:** R7, R8, R9, R10, R11

**Dependencies:** Unit 3 (SpellSchool is already threaded through)

**Files:**
- Modify: `src/states/play_match/combat_core/damage.rs` — add resistance reduction logic for non-Physical spell schools inside `apply_damage_with_absorb()`, after DamageImmunity check, alongside armor reduction
- Test: existing test file for `apply_damage_with_absorb`

**Approach:**
- For magical schools (Fire, Frost, Shadow, Arcane, Nature, Holy):
  - Look up target's base resistance for that school from `Combatant` fields
  - Sum any `SpellResistanceBuff` auras from `active_auras` matching the school (Unit 5 adds the aura type, but the lookup code can be written now to handle the case where no such auras exist)
  - Apply formula: `reduction = total_resistance / (total_resistance * 5.0/3.0 + 300.0)`
  - `remaining_damage *= 1.0 - reduction`
- For `SpellSchool::None`: no reduction (abilities like Divine Shield)
- Need a helper method on `Combatant` to get resistance by `SpellSchool` — a match on the six resistance fields

**Patterns to follow:**
- Armor reduction logic from Unit 3 (same position in pipeline, same multiplicative pattern)

**Test scenarios:**
- Happy path: Frost damage to target with 75 Frost resistance is reduced by ~13%
- Happy path: Frost damage to target with 0 resistance is not reduced
- Happy path: Shadow damage is only reduced by Shadow resistance, not Frost resistance
- Happy path: Resistance of 150 produces ~20% reduction (formula validation)
- Edge case: Very high resistance (300) produces ~28.6% reduction (diminishing returns work)
- Edge case: SpellSchool::None bypasses resistance entirely

**Verification:**
- New resistance reduction tests pass
- Headless simulation: Mage Frostbolt damage to a target with Frost resistance gear is visibly reduced

---

- [ ] **Unit 5: Add SpellResistanceBuff AuraType**

**Goal:** Enable aura-based resistance buffs for future abilities (R16, R17)

**Requirements:** R16, R17

**Dependencies:** Unit 4

**Files:**
- Modify: `src/states/play_match/components/auras.rs` — add `SpellResistanceBuff` variant to `AuraType` enum
- Modify: `src/states/play_match/combat_core/damage.rs` — ensure resistance lookup in Unit 4 sums `SpellResistanceBuff` aura magnitudes matching the damage's spell school
- Test: existing test file for `apply_damage_with_absorb`

**Approach:**
- Add `SpellResistanceBuff` to the `AuraType` enum
- In the resistance reduction code (Unit 4), iterate `active_auras` for `SpellResistanceBuff` entries where `aura.spell_school == Some(damage_school)`, sum their `magnitude` values, add to equipment-based resistance
- No abilities grant this aura yet — this unit establishes the mechanism. Future abilities (Paladin resistance aura, Mark of the Wild) will use it
- Follow `SpellSchoolLockout` pattern for school identification via Aura's `spell_school` field

**Patterns to follow:**
- `SpellSchoolLockout` in `auras.rs` and its handling in `damage.rs`

**Test scenarios:**
- Happy path: Target with 0 equipment Frost resistance but a `SpellResistanceBuff` aura (spell_school: Frost, magnitude: 50) gets 50 effective Frost resistance
- Happy path: Equipment resistance (30) + aura resistance (20) = 50 total (additive stacking per R17)
- Edge case: Multiple `SpellResistanceBuff` auras for same school stack additively

**Verification:**
- Tests pass demonstrating aura-based resistance works
- `cargo build --release` succeeds

---

- [ ] **Unit 6: Update stats panel and combat log**

**Goal:** Make armor and resistance values visible in the UI and combat log

**Requirements:** Success criteria — "Armor/resistance values appear in the combat log or stats panel"

**Dependencies:** Units 1-4

**Files:**
- Modify: `src/states/view_combatant_ui.rs` — add `armor: f32` and resistance fields to `ClassStats` and `EquipmentBonuses` structs. Add armor row to `render_stats_panel`. Show non-zero resistance values
- Modify: `src/combat/log.rs` — include mitigated amount in damage log messages (human-readable string)

**Approach:**
- Stats panel: Add "Armor" row after existing stats, showing total armor with equipment contribution in green (same pattern as other stats). Show resistance values only when non-zero to avoid cluttering the panel
- `EquipmentBonuses::from_loadout()`: accumulate armor and resistance from items
- Combat log: Update the human-readable damage message to include mitigated amount when > 0, e.g., "Warrior hits Mage for 45 damage (12 mitigated by armor)"
- `apply_damage_with_absorb` may need to return the mitigated amount as a third value, or the mitigation can be computed at log time from the damage delta

**Patterns to follow:**
- Existing stat row rendering in `render_stats_panel` (line 828+)
- `EquipmentBonuses::from_loadout()` accumulation pattern
- `format_item_stats` tooltip pattern for showing armor on items

**Test scenarios:**
- Happy path: Stats panel shows Armor value for equipped combatant
- Happy path: Resistance values appear when > 0
- Happy path: Combat log includes mitigation amount for physical damage against armored target

**Verification:**
- Visual confirmation: stats panel shows armor/resistance values
- Combat log entries include mitigation info
- `cargo build --release` succeeds

---

- [ ] **Unit 7: Balance validation via headless simulations**

**Goal:** Verify the system produces reasonable outcomes and no class is broken

**Requirements:** Success criteria — "No class achieves >85% win rate across all 1v1 matchups"

**Dependencies:** Units 1-6

**Files:**
- No code changes — validation only

**Approach:**
- Run headless 1v1 simulations for all class matchups (Warrior, Mage, Rogue, Priest, Warlock, Paladin, Hunter = 21 unique matchups)
- Run each matchup multiple times (5-10) to account for RNG variance
- Check win rates: no class should exceed 85% aggregate win rate
- Compare before/after: run a baseline without armor/resistance to quantify the impact
- If balance is severely broken, note specific matchups for the follow-up tuning pass

**Test scenarios:**
- Happy path: All classes have win rates between 15-85% across matchups
- Integration: Warrior vs Mage still produces competitive matches (Warrior gains armor but Mage has ranged advantage)
- Integration: Rogue vs Warrior is still playable despite armor disadvantage (Rogue has burst + CC)

**Verification:**
- Headless simulation results show no degenerate matchups
- If any class exceeds 85% win rate, document which matchups are problematic for the follow-up tuning pass

## System-Wide Impact

- **Interaction graph:** `apply_damage_with_absorb` is the central damage chokepoint — 8 call sites across auto_attack, casting, combat_ai, holy_shock, projectiles, and auras must all pass SpellSchool. No new systems or registrations needed
- **Error propagation:** Division by zero impossible in armor formula (denominator is `armor + 5500`, minimum 5500). Resistance formula similarly safe (denominator minimum 300). No new error paths introduced
- **State lifecycle risks:** Armor/resistance are computed once at spawn via `apply_equipment()` and remain constant for the match (like attack_power, spell_power). No mid-match state changes to manage
- **API surface parity:** `EquipmentBonuses::from_loadout()` must mirror the accumulation logic in `apply_equipment()` — same pattern already used for all other equipment stats
- **Unchanged invariants:** Caster-side reductions (DamageReduction, Divine Shield penalty) remain at call sites. DamageTakenReduction and Absorb shields remain inside `apply_damage_with_absorb`, just after the new armor/resistance step. Healing is unaffected — armor/resistance only apply to damage

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Signature cascade on `apply_damage_with_absorb` breaks many tests | Follow crit_chance addition pattern (documented institutional learning). Budget time for updating all test call sites |
| Plate armor makes Warriors/Paladins too tanky without damage rebalancing | Balance validation in Unit 7 catches this. Scope explicitly defers rebalancing but validates viability |
| Rogue physical-only damage disadvantaged against plate wearers | Rogue has burst + CC tools. Armor penetration is planned as future feature. Unit 7 validates matchup remains competitive |
| `characters.ron` removal breaks something | Verify no code reads these fields before removing. Research shows they are currently unused |

## Sources & References

- **Origin document:** [docs/brainstorms/2026-04-03-armor-spell-resistance-requirements.md](docs/brainstorms/2026-04-03-armor-spell-resistance-requirements.md)
- Damage pipeline: `src/states/play_match/combat_core/damage.rs:23`
- Equipment system: `src/states/play_match/equipment.rs`, `src/states/play_match/components/combatant.rs:362`
- Aura types: `src/states/play_match/components/auras.rs`
- Stats panel: `src/states/view_combatant_ui.rs:148-828`
- Combat log: `src/combat/log.rs`
- Institutional learning: `docs/solutions/implementation-patterns/critical-hit-system-distributed-crit-rolls.md`
- WoW Classic armor formula: wowwiki community research, `Armor / (Armor + 5500)` at level 60
- WoW Classic resistance formula: wowwiki community research, `R / (R * 5/3 + 300)` average mitigation at level 60
