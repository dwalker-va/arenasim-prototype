---
title: "AI breaks friendly breakable CC by applying damage/DoTs to CC'd targets"
category: ai-decision-patterns
tags:
  - crowd-control
  - dot-management
  - class-ai
  - combat-context
  - polymorph
  - freezing-trap
module: src/states/play_match/class_ai
symptom: "All DPS classes apply DoTs or direct damage to targets under friendly breakable CC, immediately breaking the CC"
root_cause: "Class AI try_*() functions lacked awareness of friendly breakable CC on targets; no guard checks before applying damage abilities to CC'd enemies"
---

# Friendly CC Break Prevention

## Problem

AI classes (Warlock, Warrior, Mage) applied DoTs or direct damage to targets that had friendly breakable CC (Polymorph, Freezing Trap), immediately breaking their own team's CC. This is an AI decision bug — the break-on-damage combat system correctly follows WoW Classic behavior where ANY damage breaks Polymorph.

**Observed in bug hunt (2026-03-16):**
- Warlock's Curse of Agony tick breaks own Mage's Polymorph
- Warrior's Rend tick breaks own Mage's Polymorph
- Mage Frostbolts own team's polymorphed target

## Root Cause

No check existed to determine whether a target was under a break-on-damage CC cast by a friendly player. The AI blindly applied Corruption, Immolate, Rend, Frostbolt, and curses to targets with friendly Polymorph or Freezing Trap active.

## Solution

### 1. Helper Methods on `CombatContext` (class_ai/mod.rs)

```rust
/// Check if target has a break-on-any-damage CC from a friendly caster.
/// Uses threshold-based detection: any aura with break_on_damage_threshold == 0.0
/// from a same-team caster is protected.
pub fn has_friendly_breakable_cc(&self, target: Entity) -> bool {
    let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
    self.active_auras
        .get(&target)
        .map(|auras| {
            auras.iter().any(|a| {
                a.break_on_damage_threshold == 0.0
                    && a.caster
                        .and_then(|c| self.combatants.get(&c).map(|info| info.team))
                        == Some(my_team)
            })
        })
        .unwrap_or(false)
}

/// Check if target has DoTs from a friendly caster that would break CC.
pub fn has_friendly_dots_on_target(&self, target: Entity) -> bool {
    let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
    self.active_auras
        .get(&target)
        .map(|auras| {
            auras.iter().any(|a| {
                a.effect_type == AuraType::DamageOverTime
                    && a.caster
                        .and_then(|c| self.combatants.get(&c).map(|info| info.team))
                        == Some(my_team)
            })
        })
        .unwrap_or(false)
}
```

**Key pattern — team lookup via `Aura.caster`:** The `Aura` struct stores `caster: Option<Entity>`. To check the caster's team:
```rust
a.caster.and_then(|c| self.combatants.get(&c).map(|info| info.team)) == Some(my_team)
```

### 2. Guard in Each Damage/DoT Function

Add at the top of every `try_*()` function that applies damage or DoTs:

```rust
// Don't apply damage/DoT to a target under friendly breakable CC
if ctx.has_friendly_breakable_cc(target_entity) {
    return false;
}
```

**Applied to:**
- **Warrior:** `try_mortal_strike()`, `try_charge()`, `try_rend()`
- **Rogue:** `try_ambush()`, `try_sinister_strike()`
- **Mage:** `try_frostbolt()`
- **Warlock:** `try_corruption()`, `try_immolate()`, `try_cast_curse()`, `try_shadowbolt()`, `try_drain_life()`
- **Priest:** `try_mind_blast()`
- **Paladin:** `try_holy_shock_damage()`
- **Hunter:** `try_aimed_shot()`, `try_arcane_shot()`, `try_concussive_shot()`

### 3. Reverse Guard on CC Application

Mage's `try_polymorph()` checks the reverse — don't Poly a target with friendly DoTs already ticking:

```rust
if ctx.has_friendly_dots_on_target(cc_target) {
    return false;
}
```

## Prevention: Adding New CC Abilities

**When adding a CC with `break_on_damage_threshold: 0.0`** (Blind, Sap, Gouge, Intimidating Shout, Repentance, Hibernate, Scatter Shot, etc.):

- [ ] No code changes needed — `has_friendly_breakable_cc()` is threshold-based and automatically detects any aura with `break_on_damage_threshold == 0.0` from a friendly caster
- [ ] Verify all existing damage/DoT `try_*()` functions already call `has_friendly_breakable_cc()` (see Applied To list above)
- [ ] Test with headless matches using a team comp that has both the new CC class and a DoT class

**When adding a new damage or DoT ability:**

- [ ] Add `ctx.has_friendly_breakable_cc(target)` guard at the top of the `try_*()` function
- [ ] For AoE abilities, filter CC'd targets from the target list
- [ ] For pet abilities, ensure pet AI also respects the check

**Rule of thumb:** Every `try_<damage_ability>()` function should ask "Is a teammate's CC on this target?" before doing anything. The helper is threshold-based — any new CC with `break_on_damage_threshold: 0.0` in `abilities.ron` is automatically protected.

## Currently Covered CC Types

| AuraType | Ability | break_on_damage | In `has_friendly_breakable_cc`? |
|----------|---------|-----------------|-------------------------------|
| Polymorph | Polymorph | 0.0 (any) | Yes |
| Incapacitate | Freezing Trap | 0.0 (any) | Yes |
| Root | Frost Nova, Frost Trap | 35.0 | No (threshold-based, OK to DoT) |
| Fear | Fear | 100.0 | No (threshold-based, OK to DoT) |

## Related Documentation

- Bug report: `docs/reports/2026-03-16-headless-match-bug-report.md` (BUG-5)
- CC type taxonomy: `design-docs/wow-mechanics.md`
- Known issues: `docs/known-issues.md` (KI-8: Divine Shield while CC'd is intentional)
- Dual system registration: `docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md`
