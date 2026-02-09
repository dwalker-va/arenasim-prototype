---
title: "Implementing a Critical Hit System with Distributed Damage Sites"
category: implementation-patterns
tags: [combat-mechanics, damage-application, crit-chance, stat-system, logging, visual-effects, deferred-processing, projectiles]
module: combat_core
symptom: "Need to apply critical hit chance consistently across 7 different damage/healing sites while maintaining per-class balance and correct formula ordering"
root_cause: "Critical hit calculations must be distributed across multiple damage/healing application points rather than centralized, requiring consistent patterns at each site"
date: 2026-02-08
---

# Critical Hit System: Distributed Crit Rolls Across Damage Sites

## Problem

Adding critical strikes to a combat system with multiple damage/healing pathways requires consistent integration at every site where damage or healing is applied. The challenge is that damage happens in 7 different places (auto-attacks, cast completion, projectile impact, Holy Shock pending components, class AI instant attacks), each with different data shapes and timing.

## Solution

### Core Pattern: Roll → Multiply → Propagate

Every damage/healing site follows the same 3-step pattern:

```rust
// 1. Roll crit (before any reductions)
let is_crit = roll_crit(caster.crit_chance, &mut game_rng);

// 2. Apply multiplier to raw value
if is_crit {
    damage *= CRIT_DAMAGE_MULTIPLIER;  // 2.0 for damage
    // or: healing *= CRIT_HEALING_MULTIPLIER;  // 1.5 for healing
}

// 3. Propagate is_crit to FCT and combat log
commands.spawn(FloatingCombatText { is_crit, .. });
combat_log.log_damage(..., is_crit, message);
```

### Foundation (3 pieces)

**1. Stat on Combatant** (`components/mod.rs`):
```rust
pub struct Combatant {
    pub crit_chance: f32,  // 0.0 to 1.0
    // ...
}
```

**2. Free function** (`combat_core.rs`):
```rust
pub fn roll_crit(crit_chance: f32, rng: &mut GameRng) -> bool {
    rng.random_f32() < crit_chance
}
```

**3. Constants** (`constants.rs`):
```rust
pub const CRIT_DAMAGE_MULTIPLIER: f32 = 2.0;
pub const CRIT_HEALING_MULTIPLIER: f32 = 1.5;
```

### The 7 Sites

| # | Site | File | Key Detail |
|---|------|------|------------|
| 1 | Auto-attacks | `combat_core.rs` | Crit before physical damage reduction |
| 2 | Cast completion (damage) | `combat_core.rs` | Crit before absorbs |
| 3 | Cast completion (healing) | `combat_core.rs` | Crit before healing reduction (Mortal Strike) |
| 4 | Projectile impact | `projectiles.rs` | Crit rolled at **impact time**, not cast time |
| 5 | Holy Shock damage | `effects/holy_shock.rs` | Uses snapshotted `caster_crit_chance` from pending struct |
| 6 | Holy Shock healing | `effects/holy_shock.rs` | Same snapshot pattern |
| 7 | Class AI instant attacks | `warrior.rs`, `rogue.rs`, `mage.rs` | Crit rolled in class AI, passed via tuple |

### Explicitly Excluded Sites

| Site | Reason |
|------|--------|
| DoT ticks (`auras.rs`) | WoW Classic rules: DoTs never crit |
| Channel ticks (`combat_core.rs`) | Channels never crit |
| Absorb shield FCTs | Informational text, not damage/heal |

Each excluded site passes `is_crit: false` with a comment explaining why.

## Key Decisions

### 1. Crit BEFORE reductions

Formula: `(Base + Stat × Coefficient) × CritMultiplier`, then damage reductions and absorbs apply after. This matches WoW Classic behavior.

### 2. Deferred abilities snapshot crit_chance

Holy Shock uses a pending-component pattern. The `caster_crit_chance` is captured at cast time alongside `caster_spell_power`:

```rust
pub struct HolyShockHealPending {
    pub caster_spell_power: f32,
    pub caster_crit_chance: f32,  // Snapshotted at cast time
    // ...
}
```

The actual crit roll happens when the pending component is processed, but uses the snapshotted value.

### 3. Projectiles roll at impact

Unlike Holy Shock, projectile spells (Frostbolt, Shadow Bolt) roll crit at impact time using the caster's current `crit_chance` (queried from the caster entity at impact). This means if a buff changes crit_chance mid-flight, the impact uses the current value.

### 4. Per-class base crit rates

| Class | Crit Chance | Rationale |
|-------|------------|-----------|
| Rogue | 10% | Highest — burst DPS class |
| Warrior | 8% | Melee sustained DPS |
| Mage | 6% | Balanced caster |
| Paladin | 6% | Hybrid class |
| Warlock | 5% | DoT-focused (DoTs can't crit) |
| Priest | 4% | Healer (healing crits at 1.5×) |

## Gotchas and Lessons Learned

### 1. Signature changes cascade

Adding `is_crit: bool` to `log_damage()` and `log_healing()` broke every call site (17 test calls, 11 production calls). Plan for this and fix tests systematically.

### 2. Use proper imports, not super::super

Inline `super::super::combat_core::roll_crit` paths are fragile. Always add a `use` import at the top of the file. This was caught in code review and fixed.

### 3. Tuple creep is real

Adding `is_crit` as a 7th element to `instant_attacks` tuples made destructuring harder to read. Named structs would be better but following the existing pattern was the right call for consistency.

### 4. FCT rendering: avoid per-frame allocations

The initial implementation used `format!("{}!", fct.text)` every render frame for crit FCTs. While negligible at current scale (~6 entities), appending "!" at spawn time would be cleaner. Left as-is since the performance impact is immeasurable.

### 5. Combat log verb patterns

Damage crits: replace `"hits"` with `"CRITS"` — e.g., `"Mortal Strike CRITS Team 2 Warrior for 60 damage"`
Healing crits: replace `"heals"` with `"CRITICALLY heals"` — e.g., `"Flash Heal CRITICALLY heals Team 1 Warrior for 71"`

## Cross-References

- [Adding Visual Effects: Bevy Pattern](adding-visual-effect-bevy.md) — FCT rendering pattern used for crit text
- [Adding New Class: Paladin](adding-new-class-paladin.md) — Pending component snapshot pattern (caster_crit_chance follows caster_spell_power pattern)
- [Two-Agent Bug Hunting](../workflows/two-agent-bug-hunting.md) — Testing methodology for validating crit behavior across matchups
- PR: https://github.com/dwalker-va/arenasim-prototype/pull/1

## Prevention / Future Work

- When adding new damage/healing sites, always check the 3-step pattern: roll → multiply → propagate
- When adding new stats that need snapshotting in pending components, follow the `caster_crit_chance` / `caster_spell_power` pattern
- Consider migrating the large tuples (`instant_attacks`, `frost_nova_damage`, `hits_to_process`) to named structs to make adding future fields easier (tracked in `todos/005-pending-p2-combatant-info-tuple-creep.md`)
