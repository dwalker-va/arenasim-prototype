---
date: 2026-04-03
topic: armor-spell-resistance
status: implemented
---

# Armor & Spell Resistance System

## Problem Frame

All combatants currently take the same raw damage regardless of their armor type or class fantasy. A Warrior in full plate absorbs a Frostbolt identically to a Mage in cloth. The only passive mitigation comes from auras (Devotion Aura, Curse of Weakness) and absorb shields. Adding armor and spell resistances creates meaningful class differentiation in survivability, rewards strategic gear choices, and brings the sim closer to WoW Classic's combat feel.

## Requirements

**Armor (Physical Damage Mitigation)**

- R1. Add an `armor` stat to combatants that reduces incoming Physical spell school damage
- R2. Use WoW Classic's level-60 armor formula: `Reduction % = Armor / (Armor + 5500)` — this provides natural diminishing returns
- R3. Each equipment piece contributes an armor value derived from its `armor_type` and `item_level` — Plate > Mail > Leather > Cloth > None
- R4. Total armor is the sum of all equipped item armor values. No base class armor — unequipped combatants have 0 armor
- R5. Armor reduction applies in the damage pipeline after crit calculation, before percentage aura reductions (DamageTakenReduction) and absorb shields. Applies to both auto-attack and ability damage paths for Physical school damage
- R6. Shields (off-hand) contribute armor like other equipment pieces

**Spell Resistance (Magical Damage Mitigation)**

- R7. Add per-school resistance stats: Fire, Frost, Shadow, Arcane, Nature, Holy resistance
- R8. Use WoW Classic's average resistance formula: `Average Reduction % = Resistance / (Resistance * 5/3 + AttackerLevel * 5)` (at level 60, approximately `Resistance / (Resistance * 5/3 + 300)`)
- R9. Apply average mitigation deterministically (no partial resist rolls) — can add RNG partial resists in a future iteration
- R10. Resistances are summed from all sources (equipment stats, active auras) into a single value per school before applying the formula
- R11. Resistance reduction applies in the damage pipeline for the matching spell school, after crit calculation, before percentage aura reductions and absorb shields. Must also apply to DoT tick damage matching the spell school

**Equipment Integration**

- R12. Add an `armor` field to `ItemConfig` for numeric armor values on equipment
- R13. Add optional resistance fields to `ItemConfig` (e.g., `fire_resistance`, `frost_resistance`) so gear can grant spell resistance
- R14. Populate armor values for all existing items based on their armor_type and item_level, using WoW-authentic scaling curves per armor type
- R15. Existing aura types (`DamageReduction`, `DamageTakenReduction`) continue to work — armor/resistance applies before them in the pipeline (see R5, R11)

**Aura/Buff Resistance**

- R16. Support resistance buffs via the existing aura system (e.g., a future Paladin resistance aura or Mark of the Wild could grant flat resistance to a school)
- R17. Resistance from auras stacks additively with resistance from equipment

## Success Criteria

- A Warrior in plate armor takes noticeably less physical damage than a Mage in cloth
- A combatant with Frost Resistance gear takes reduced damage from Frostbolt/Frost Nova
- Armor/resistance values appear in the combat log or stats panel so the mitigation is visible
- Existing aura-based mitigation (Devotion Aura, absorb shields) still functions correctly alongside the new system
- Headless match simulations produce different outcomes reflecting the new mitigation
- No class achieves >85% win rate across all 1v1 matchups (basic balance sanity check via headless simulations)

## Scope Boundaries

- **Not in scope:** Armor penetration / spell penetration stats (future feature)
- **Not in scope:** Partial resist RNG rolls — using average mitigation only for now
- **Not in scope:** Talent system — resistance stats are simple f32 sums, so talents can add to them later with no special design
- **Not in scope:** Resistance-specific UI beyond showing values in stats panel
- **Not in scope:** Rebalancing ability damage numbers to account for new mitigation (follow-up tuning pass)

## Key Decisions

- **WoW-authentic formulas:** Armor uses `Armor / (Armor + 5500)` diminishing returns curve; resistance uses the Classic average-resist formula. These are well-tested and create natural balance.
- **Per-school resistances:** Six separate resistance stats (Fire, Frost, Shadow, Arcane, Nature, Holy) rather than a single unified stat. Enables strategic counter-play through gear choices.
- **Item-level armor values:** Armor comes from equipment, not class base stats. Total armor = sum of equipped pieces. This makes equipment choices matter and avoids redundant stat layers.
- **Average mitigation (no RNG):** Resistance applies as deterministic average reduction. Simpler to balance and reason about. Partial resist rolls can be layered on later.
- **Pipeline ordering (multiplicative stacking):** Damage flows: Raw → Crit → Armor/Resistance reduction → DamageTakenReduction auras (Devotion Aura) → Absorb shields → Health. Each layer stacks multiplicatively, matching WoW Classic's ordering.
- **Resistance aura variant:** A single `SpellResistanceBuff` AuraType variant following the existing `SpellSchoolLockout` pattern — the Aura struct's `spell_school` field identifies which school, `magnitude` holds the resistance amount.
- **Existing characters.ron:** `assets/config/characters.ron` already defines `armor` and `magic_resistance` per class (currently unused by combat code). These should be removed or repurposed during implementation to avoid conflicting stat sources, since R4 makes armor purely equipment-derived.

## Outstanding Questions

### Deferred to Planning

- [Affects R3, R14][Needs research] What are appropriate armor-per-slot values for each armor type at the item levels used in the sim? May need to reference WoW Classic item data via Wowhead MCP.
- [Affects R8][Technical] Confirm the exact WoW Classic resistance formula at level 60 — several community sources differ slightly on the constant terms.
- [Affects R5, R11][Technical] `apply_damage_with_absorb` currently has no SpellSchool parameter and is called from 7+ sites. Determine the cleanest integration approach for adding spell-school-aware mitigation.
- [Affects R5][Technical] Validate expected mitigation ranges per armor type (cloth ~7-8%, plate ~39-42%) and confirm cloth-wearer survivability remains viable against physical damage.

## Next Steps

-> `/ce:plan` for structured implementation planning
