---
title: "refactor: Convert stat-modifying auras to dynamic checking with expiry reversal"
type: refactor
status: active
date: 2026-04-05
---

# refactor: Convert Stat-Modifying Auras to Dynamic Checking with Expiry Reversal

## Overview

Stat-modifying auras (AttackPowerIncrease, AttackPowerReduction, CritChanceIncrease, ManaRegenIncrease) currently mutate combatant stats directly on application and never reverse them. This causes two bugs: (1) stats double-dip when an aura expires and is re-applied, and (2) Divine Shield cannot properly purge stat-modifying debuffs since removing the aura doesn't restore the stat.

This refactor converts AP/crit/mana-regen auras to dynamic query-time checking (following the existing pattern used by DamageTakenReduction, SpellResistanceBuff, CastTimeIncrease). MaxHealth/MaxMana auras keep their apply-time mutation but gain stat reversal on expiry.

## Problem Frame

Code review identified that AttackPowerReduction (Demoralizing Shout, 120s duration) can expire mid-match and get re-applied, permanently reducing enemy AP by double the intended amount. Additionally, Divine Shield's debuff purge removes the aura entry but doesn't reverse the stat change, leaving the Paladin with permanently reduced AP after using their panic button.

## Requirements Trace

- R1. AP/crit/mana-regen bonuses from auras are calculated dynamically at query time, not mutated on combatant stats
- R2. MaxHealth/MaxMana aura bonuses are reversed when the aura expires (stat + current value clamped)
- R3. Divine Shield purge list includes AttackPowerReduction and AttackSpeedSlow
- R4. No observable behavior change for normal gameplay (buffs still appear to work identically)
- R5. Aura expiry, dispel, and Divine Shield purge all correctly restore stats

## Scope Boundaries

- MaxHealth/MaxMana auras keep apply-time mutation (NOT converted to dynamic) — they gain expiry reversal instead
- No changes to aura application timing, stacking rules, or DR system
- No changes to how auras are displayed in the UI

## Key Technical Decisions

- **AP/crit/mana-regen → dynamic**: These stats are consumed in a small number of well-defined locations (damage calc, crit roll, regen tick) that already have ActiveAuras access. Adding a dynamic bonus check at each consumption point is clean and follows the DamageTakenReduction/CastTimeIncrease pattern.

- **MaxHealth/MaxMana → expiry reversal, not dynamic**: Converting max_health to dynamic would require every HP% check in AI, every heal cap, and every health display to compute "effective max health." Instead, we reverse the stat mutation when the aura is removed (expiry, purge, or dispel). This is simpler and the stat is always accurate.

- **Helper functions in combat_core/mod.rs**: Add `get_attack_power_bonus()`, `get_crit_chance_bonus()`, `get_mana_regen_bonus()` following the exact pattern of `get_cast_time_increase()` and `get_lockout_duration_reduction()`.

## Open Questions

### Resolved During Planning

- **Where to put the helper functions?** In `combat_core/mod.rs` alongside `get_cast_time_increase()` and `get_lockout_duration_reduction()`. This is the established location for "sum aura magnitudes" helpers.

- **How to handle MaxHealth reversal on expiry?** In `update_auras()`, before removing expired auras, check each expiring aura's type. For MaxHealthIncrease, subtract magnitude from max_health and clamp current_health. Same for MaxManaIncrease. This runs every frame and catches normal expiry. Divine Shield's retain-based purge needs the same reversal logic before the retain call.

- **What about AttackPowerReduction for both AP increase and decrease?** `get_attack_power_bonus()` returns the net AP modifier: sum of AttackPowerIncrease magnitudes minus sum of AttackPowerReduction magnitudes. This handles both Battle Shout (+AP) and Demoralizing Shout (-AP) in one query.

### Deferred to Implementation

- **Exact signature of helper functions**: Whether they take `Option<&ActiveAuras>` or `&ActiveAuras` — follow the existing pattern.
- **Whether `process_completed_casts` needs query changes**: The caster's ActiveAuras may already be accessible via the existing query; verify during implementation.

## High-Level Technical Design

> *This illustrates the intended approach and is directional guidance for review, not implementation specification.*

```
BEFORE (current):
  apply_pending_auras:
    AttackPowerIncrease → combatant.attack_power += magnitude
    AttackPowerReduction → combatant.attack_power -= magnitude
    CritChanceIncrease → combatant.crit_chance += magnitude
    ManaRegenIncrease → combatant.mana_regen += magnitude
  
  update_auras:
    expired auras → just removed, stats never reversed

  damage calc:
    uses combatant.attack_power directly (already includes aura mutation)

AFTER (refactored):
  apply_pending_auras:
    AttackPowerIncrease → log only, no stat mutation
    AttackPowerReduction → log only, no stat mutation
    CritChanceIncrease → log only, no stat mutation
    ManaRegenIncrease → log only, no stat mutation
    MaxHealthIncrease → still mutates (apply-time), but...
    MaxManaIncrease → still mutates (apply-time), but...

  update_auras:
    MaxHealthIncrease expiring → max_health -= magnitude, clamp current_health
    MaxManaIncrease expiring → max_mana -= magnitude, clamp current_mana

  damage calc / ability damage:
    effective_ap = combatant.attack_power + get_attack_power_bonus(active_auras)
  
  crit roll:
    effective_crit = combatant.crit_chance + get_crit_chance_bonus(active_auras)
  
  regen tick:
    effective_regen = combatant.mana_regen + get_mana_regen_bonus(active_auras)
  
  divine_shield purge:
    add AttackPowerReduction + AttackSpeedSlow to purge list
    before retain: reverse MaxHealthIncrease/MaxManaIncrease stats for purged auras
```

## Implementation Units

- [ ] **Unit 1: Add dynamic stat bonus helper functions**

  **Goal:** Create helper functions for querying AP, crit, and mana regen bonuses from active auras, following the established pattern.

  **Requirements:** R1

  **Dependencies:** None

  **Files:**
  - Modify: `src/states/play_match/combat_core/mod.rs`
  - Test: `src/states/play_match/combat_core/mod.rs` (existing test module)

  **Approach:**
  - Add `get_attack_power_bonus(auras: Option<&ActiveAuras>) -> f32` — sums `AttackPowerIncrease` magnitudes and subtracts `AttackPowerReduction` magnitudes. Can return negative.
  - Add `get_crit_chance_bonus(auras: Option<&ActiveAuras>) -> f32` — sums `CritChanceIncrease` magnitudes
  - Add `get_mana_regen_bonus(auras: Option<&ActiveAuras>) -> f32` — sums `ManaRegenIncrease` magnitudes
  - Make all three `pub` so they're accessible from other combat modules

  **Patterns to follow:**
  - `get_cast_time_increase()` and `get_lockout_duration_reduction()` in the same file — identical structure

  **Test scenarios:**
  - Happy path: `get_attack_power_bonus` with one AttackPowerIncrease aura (magnitude 20) → returns 20.0
  - Happy path: `get_attack_power_bonus` with one AttackPowerReduction aura (magnitude 15) → returns -15.0
  - Happy path: `get_attack_power_bonus` with both increase (20) and reduction (15) → returns 5.0
  - Happy path: `get_crit_chance_bonus` with CritChanceIncrease (0.05) → returns 0.05
  - Happy path: `get_mana_regen_bonus` with ManaRegenIncrease (8.0) → returns 8.0
  - Edge case: All three functions with no auras (None) → returns 0.0
  - Edge case: All three with empty ActiveAuras → returns 0.0

  **Verification:**
  - Unit tests pass for all helpers
  - `cargo test` passes

- [ ] **Unit 2: Wire dynamic AP/crit/mana-regen checks into consumption points**

  **Goal:** Replace direct stat reads with base stat + dynamic bonus at every point where AP, crit, and mana regen are consumed.

  **Requirements:** R1, R4

  **Dependencies:** Unit 1

  **Files:**
  - Modify: `src/states/play_match/components/combatant.rs` — update `calculate_ability_damage_config()` to accept aura bonus or take ActiveAuras
  - Modify: `src/states/play_match/combat_core/casting.rs` — pass aura bonus to damage calc, add crit bonus to crit rolls, add mana regen bonus to regen tick
  - Modify: `src/states/play_match/combat_core/auto_attack.rs` — add crit bonus to auto-attack crit roll
  - Modify: `src/states/play_match/projectiles.rs` — add crit bonus to projectile crit roll

  **Approach:**
  - At each `combatant.attack_power` read in damage calc: add `get_attack_power_bonus(active_auras)`
  - At each `roll_crit(combatant.crit_chance, ...)` call: replace with `roll_crit(combatant.crit_chance + get_crit_chance_bonus(active_auras), ...)`
  - At the mana regen tick: replace `combatant.mana_regen` with `combatant.mana_regen + get_mana_regen_bonus(active_auras)`
  - The `regenerate_resources` system currently queries only `&mut Combatant` — it needs `Option<&ActiveAuras>` added to its query

  **Patterns to follow:**
  - How `apply_damage_with_absorb` receives and uses `active_auras` parameter
  - How `calculate_cast_time` takes `auras: Option<&ActiveAuras>` and applies the modifier

  **Test scenarios:**
  - Integration: Warrior with Battle Shout (+20 AP) deals more damage than without → damage calc uses dynamic bonus
  - Integration: Mage with Molten Armor (+5% crit) has higher crit rate → crit roll uses dynamic bonus
  - Integration: Mage with Mage Armor (+8 mana/s) regenerates faster → regen tick uses dynamic bonus
  - Happy path: Default config (no auras) produces identical damage/crit/regen to current behavior

  **Verification:**
  - Headless match with default config produces identical combat log output
  - `cargo test` passes

- [ ] **Unit 3: Remove stat mutations from apply_pending_auras**

  **Goal:** Stop mutating combatant.attack_power, crit_chance, and mana_regen when AP/crit/mana-regen auras are applied. Keep the combat log messages.

  **Requirements:** R1, R4

  **Dependencies:** Unit 2 (dynamic checks must be wired before removing mutations)

  **Files:**
  - Modify: `src/states/play_match/auras.rs` — remove stat mutation lines, keep log messages

  **Approach:**
  - In `apply_pending_auras`, for `AttackPowerIncrease`: remove `target_combatant.attack_power += ap_bonus`, keep the combat log entry
  - For `AttackPowerReduction`: remove `target_combatant.attack_power = (... - ap_reduction).max(0.0)`, keep the log
  - For `CritChanceIncrease`: remove `target_combatant.crit_chance += magnitude`, keep the log
  - For `ManaRegenIncrease`: remove `target_combatant.mana_regen += magnitude`, keep the log
  - MaxHealthIncrease and MaxManaIncrease: keep stat mutations as-is

  **Test scenarios:**
  - Integration: Battle Shout aura applied → combat log shows "+20 attack power" but `combatant.attack_power` unchanged
  - Integration: Demoralizing Shout expires at 120s, re-applied → AP reduction is still exactly -15 (not -30 double-dip)
  - Edge case: Multiple Demoralizing Shouts from different warriors stack correctly via dynamic bonus sum

  **Verification:**
  - Headless match: Warrior with Battle Shout deals same damage as before (dynamic bonus compensates)
  - Headless match: Demoralizing Shout after 120s+ re-applies without double-dip

- [ ] **Unit 4: Add MaxHealth/MaxMana reversal on aura expiry and purge**

  **Goal:** When MaxHealthIncrease or MaxManaIncrease auras expire or are purged, reverse the stat mutation and clamp current health/mana.

  **Requirements:** R2, R5

  **Dependencies:** None (independent of Units 1-3)

  **Files:**
  - Modify: `src/states/play_match/auras.rs` — add reversal logic in `update_auras` before expired aura removal
  - Modify: `src/states/play_match/effects/divine_shield.rs` — add reversal before retain-based purge

  **Approach:**
  - In `update_auras`, the system queries `(Entity, &mut ActiveAuras, ...)` but NOT `&mut Combatant`. Need to add `&mut Combatant` to the query or handle reversal separately.
  - Before `auras.auras.retain(|aura| aura.duration > 0.0)`, iterate expiring auras and for MaxHealthIncrease: record the magnitude. After the retain, subtract from combatant's max_health and clamp current_health.
  - Same logic in `process_divine_shield` before the retain call.
  - Clamp: `combatant.current_health = combatant.current_health.min(combatant.max_health)`

  **Patterns to follow:**
  - The existing `update_auras` system structure for iterating auras before removal

  **Test scenarios:**
  - Happy path: Commanding Shout (MaxHealthIncrease +40) expires → max_health decreases by 40, current_health clamped
  - Happy path: Divine Shield purges MaxHealthIncrease → same reversal occurs
  - Edge case: Current health below new max after reversal → current_health unchanged (only clamped if above)
  - Edge case: Multiple MaxHealthIncrease auras expire same frame → both reversed correctly

  **Verification:**
  - Headless match 300s+: Commanding Shout expires at 120s, max_health returns to base value
  - `cargo test` passes

- [ ] **Unit 5: Update Divine Shield purge list**

  **Goal:** Add new debuff types to Divine Shield's purge list so they are properly removed.

  **Requirements:** R3, R5

  **Dependencies:** Unit 3 (AP/crit auras no longer need stat reversal on purge since they're dynamic), Unit 4 (MaxHealth reversal logic exists)

  **Files:**
  - Modify: `src/states/play_match/effects/divine_shield.rs`

  **Approach:**
  - Add `AttackPowerReduction` and `AttackSpeedSlow` to the retain filter's matches list
  - Since AttackPowerReduction is now dynamic (from Unit 3), removing the aura is sufficient — no stat reversal needed
  - AttackSpeedSlow is already dynamic (checked in auto-attack interval), so removing the aura is sufficient

  **Test scenarios:**
  - Happy path: Paladin with AttackPowerReduction debuff uses Divine Shield → debuff is purged, AP fully restored (dynamic check returns 0 with no aura)
  - Happy path: Paladin with AttackSpeedSlow uses Divine Shield → attack speed returns to normal
  - Integration: Warrior Demoralizing Shouts Paladin, Paladin Divine Shields → AP penalty gone in combat log damage

  **Verification:**
  - Headless match: Paladin under Demoralizing Shout uses Divine Shield → subsequent damage output matches unbuffed baseline

## System-Wide Impact

- **Interaction graph:** `get_attack_power_bonus()` is called from `calculate_ability_damage_config()` in combatant.rs, which feeds into `process_completed_casts`. `get_crit_chance_bonus()` is called from 4 crit roll sites. `get_mana_regen_bonus()` is called from `regenerate_resources`. All existing aura tick/expiration/dispel systems work unchanged for the dynamic auras.
- **State lifecycle risks:** MaxHealth/MaxMana reversal in `update_auras` must run before any healing calculations in the same frame to prevent "heal based on inflated max_health" exploits. The current system ordering (update_auras runs early) handles this.
- **Unchanged invariants:** DamageTakenReduction, SpellResistanceBuff, CastTimeIncrease, LockoutDurationReduction — all remain unchanged. MaxHealthIncrease and MaxManaIncrease keep their apply-time mutation pattern but gain expiry reversal.
- **API surface parity:** Both headless and graphical modes use the same combat systems, so changes apply to both automatically.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| `regenerate_resources` query change may conflict with other systems | Check Bevy system ordering; the query adds read-only `Option<&ActiveAuras>` which shouldn't conflict |
| `update_auras` adding `&mut Combatant` to query may cause Bevy query conflicts | Verify no other concurrent query borrows both `ActiveAuras` and `Combatant` mutably in the same system set |
| MaxHealth reversal on same frame as healing could cause HP to spike then drop | System ordering ensures `update_auras` runs before healing; verify during implementation |

## Sources & References

- Related PR: #35 (class strategic options — introduced the new stat-modifying auras)
- Existing dynamic pattern: `get_cast_time_increase()` in `combat_core/mod.rs`
- Existing dynamic pattern: `DamageTakenReduction` check in `damage.rs`
- Code review findings: AttackPowerReduction double-dip bug, Divine Shield purge gap
