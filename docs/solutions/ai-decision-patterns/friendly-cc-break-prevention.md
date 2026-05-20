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

### 2. Centralized Guard via `cast_guard::pre_cast_ok`

The original BUG-1 fix added an inline `ctx.has_friendly_breakable_cc(target_entity)` guard at the top of every damage/DoT `try_*()` function. That worked, but it meant the same preamble (friendly-CC check, spell-school lockout, silence, cooldown, range/mana) was repeated in ~15 places — easy to forget when adding a new ability.

The current pattern collapses the preamble into a single helper in `src/states/play_match/class_ai/cast_guard.rs`:

```rust
pub fn pre_cast_ok(
    ability: AbilityType,
    def: &AbilityConfig,
    caster: &Combatant,
    caster_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target: Option<(Entity, Vec3)>,
    ctx: &CombatContext,
    opts: PreCastOpts,
) -> bool { /* ... */ }

#[derive(Debug, Clone, Copy, Default)]
pub struct PreCastOpts {
    pub check_friendly_cc: bool,    // BUG-1: skip if target is under our team's break-on-damage CC
    pub check_friendly_dots: bool,  // BUG-2: skip if target carries our team's DoTs (e.g. before applying Polymorph)
    pub check_target_immune: bool,  // skip if target is damage-immune (Divine Shield)
    pub bypass_silence: bool,       // allow even when silenced (Divine Shield etc.)
}
```

Each damage/DoT `try_*()` function now reduces its preamble to one call. Example from `class_ai/mage.rs::try_frostbolt`:

```rust
if !pre_cast_ok(
    AbilityType::Frostbolt,
    frostbolt_def,
    combatant,
    my_pos,
    Some(auras),
    Some((target_entity, target_pos)),
    ctx,
    PreCastOpts { check_friendly_cc: true, ..Default::default() },
) {
    return false;
}
```

**Applied (via `PreCastOpts { check_friendly_cc: true }`) to:** every damage/DoT `try_*()` in Warrior, Rogue, Mage, Warlock, Priest, Paladin, and Hunter. The `has_friendly_breakable_cc` helper is still queryable directly from `CombatContext` for the rare case that needs a non-cast pathway (e.g. Warrior charge target filtering in `class_ai/warrior.rs`).

### 3. Reverse Guard on CC Application

Polymorph and analogous incapacitates check the reverse — don't Poly a target with friendly DoTs already ticking. This is the same `pre_cast_ok` helper with a different opt:

```rust
PreCastOpts { check_friendly_cc: true, check_friendly_dots: true, ..Default::default() }
```

Look for `check_friendly_dots: true` (e.g. `class_ai/mage.rs::try_polymorph`, `class_ai/paladin.rs::try_hammer_of_justice`) to find current callers.

## Prevention: Adding New CC Abilities

**When adding a CC with `break_on_damage_threshold: 0.0`** (Blind, Sap, Gouge, Intimidating Shout, Repentance, Hibernate, Scatter Shot, etc.):

- [ ] No code changes needed — `has_friendly_breakable_cc()` is threshold-based and automatically detects any aura with `break_on_damage_threshold == 0.0` from a friendly caster
- [ ] Verify all existing damage/DoT `try_*()` functions already call `pre_cast_ok(..., PreCastOpts { check_friendly_cc: true, ... })`
- [ ] Test with headless matches using a team comp that has both the new CC class and a DoT class

**When adding a new damage or DoT ability:**

- [ ] Wire the cast through `pre_cast_ok` with `PreCastOpts { check_friendly_cc: true, ..Default::default() }` — that single opt covers BUG-1 protection
- [ ] If the new ability also applies CC (e.g. a stun that follows the damage), set `check_friendly_dots: true` as well so it won't break a teammate's existing DoT-incompatible CC
- [ ] For AoE abilities, filter CC'd targets from the target list (the helper guards a single `target` entity; AoE selection happens before `pre_cast_ok`)
- [ ] For pet abilities, ensure pet AI also respects the check (see `class_ai/pet_ai.rs`)

**Rule of thumb:** Every damage/DoT/CC cast should route through `pre_cast_ok` with the appropriate `PreCastOpts`. Any new CC with `break_on_damage_threshold: 0.0` in `abilities.ron` is automatically protected because the underlying `has_friendly_breakable_cc` check is threshold-based.

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
