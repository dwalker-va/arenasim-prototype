---
title: "feat: Track damage mitigated by armor and spell resistance in combat logs"
type: feat
status: pending
date: 2026-04-04
---

# feat: Track Damage Mitigated by Armor and Spell Resistance

## Problem

The armor/spell resistance system reduces incoming damage but the mitigated amounts are invisible in combat logs and match reports. Post-match analysis cannot determine how much damage each combatant's armor or resistances prevented, or which spell schools were mitigated most.

## Approach

Add per-school mitigation tracking to the Combatant struct rather than expanding the `apply_damage_with_absorb` return type (which would require another signature cascade across 8+ call sites).

## Requirements

- R1. Track total physical damage mitigated by armor per combatant
- R2. Track total damage mitigated by spell resistance per school per combatant
- R3. Include mitigation totals in the match report (END OF REPORT section)
- R4. Optionally include per-hit mitigation in combat log lines (e.g., "Frostbolt hits Warrior for 75 damage (32 resisted)")

## Implementation Notes

- Add `damage_mitigated_by_armor: f32` and `damage_mitigated_by_resistance: [f32; 6]` (indexed by school) to Combatant
- Accumulate inside `apply_damage_with_absorb` where reduction is already calculated — no signature change needed
- Add a summary section to the match report in `combat/log.rs`
- R4 (per-hit log lines) would require passing the pre-mitigation damage to the log call sites, which is a larger change — defer or skip
