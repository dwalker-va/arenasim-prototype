# Stat Scaling System - Design Document

**Created:** January 3, 2026  
**Purpose:** Gear-ready damage and healing formulas for ArenaSim

---

## Overview

The stat scaling system allows abilities to scale with character stats (`attack_power` and `spell_power`), making the game ready for a future gear system. Instead of abilities dealing fixed damage, they now use a formula that combines base values with stat-based scaling.

This matches World of Warcraft's damage formula structure and ensures that when gear is eventually added to the game, all abilities will automatically benefit from increased stats without requiring code changes.

---

## Core Formula

### Damage Abilities
```
Damage = Base Damage + (Scaling Stat × Coefficient)

Where:
- Base Damage = Random value between damage_base_min and damage_base_max
- Scaling Stat = attack_power (physical) OR spell_power (magical)
- Coefficient = How much damage per point of the stat (e.g., 0.8 = 80%)
```

### Healing Abilities
```
Healing = Base Healing + (Spell Power × Coefficient)

Where:
- Base Healing = Random value between healing_base_min and healing_base_max
- Spell Power = Always scales with spell_power (WoW standard)
- Coefficient = How much healing per point of spell power
```

---

## Character Stats (Without Gear)

These are the baseline stats for each class at level 1 (no gear equipped):

| Class   | Attack Power | Spell Power | Role                    |
|---------|--------------|-------------|-------------------------|
| Warrior | 30           | 0           | Physical DPS / Tank     |
| Rogue   | 35           | 0           | Burst Physical DPS      |
| Mage    | 0            | 50          | Magical DPS / Control   |
| Priest  | 0            | 40          | Healing / Hybrid Damage |
| Warlock | 0            | 45          | Magical DPS / DoTs      |

**Design Notes:**
- Rogues have slightly higher AP than Warriors (burst vs sustained)
- Mages have highest spell power (pure caster DPS)
- Priests have moderate spell power (healing priority, damage secondary)
- Physical classes have 0 spell power (and vice versa) for clear specialization

---

## Ability Coefficients

### Damage Abilities

| Ability          | Class  | Base Damage | Coefficient | Scales With | Example Damage (no gear) |
|------------------|--------|-------------|-------------|-------------|--------------------------|
| Auto-Attack      | All    | Class-specific | 0.0 | None | Fixed (10-12 for Rogue) |
| Frostbolt        | Mage   | 10-15       | 0.8 (80%)   | Spell Power | 50-55 (10-15 + 50×0.8) |
| Mind Blast       | Priest | 15-20       | 0.85 (85%)  | Spell Power | 49-54 (15-20 + 40×0.85) |
| Frost Nova       | Mage   | 5-10        | 0.2 (20%)   | Spell Power | 15-20 (5-10 + 50×0.2) |
| Ambush           | Rogue  | 20-30       | 1.2 (120%)  | Attack Power| 62-72 (20-30 + 35×1.2) |
| Sinister Strike  | Rogue  | 5-10        | 0.5 (50%)   | Attack Power| 22.5-27.5 (5-10 + 35×0.5) |

### Healing Abilities

| Ability    | Class  | Base Healing | Coefficient | Example Healing (no gear) |
|------------|--------|--------------|-------------|---------------------------|
| Flash Heal | Priest | 15-20        | 0.75 (75%)  | 45-50 (15-20 + 40×0.75) |

### Utility Abilities (No Damage/Healing)

| Ability       | Class   | Scales With | Notes                              |
|---------------|---------|-------------|------------------------------------|
| Heroic Strike | Warrior | None        | Adds 50% weapon damage to next AA |
| Charge        | Warrior | None        | Gap closer, no damage              |
| Kidney Shot   | Rogue   | None        | 6-second stun, no damage           |

### Buff Abilities (Stat Modifiers)

| Ability          | Class   | Effect                  | Duration | Notes                                |
|------------------|---------|-------------------------|----------|--------------------------------------|
| Arcane Intellect | Mage    | +40 Max Mana            | 600s     | Cast on allies with mana pools       |
| Battle Shout     | Warrior | +20 Attack Power        | 120s     | AOE buff, affects allies within 30yd |
| PW: Fortitude    | Priest  | +100 Max Health         | 600s     | Cast on all allies                   |

**Buff Design Philosophy:**
- **Mage (Arcane Intellect)**: Benefits casters (Mage, Priest, Warlock) by increasing their mana pool
- **Warrior (Battle Shout)**: Benefits physical classes and Rogue/Warrior themselves with attack power
- **Priest (PW: Fortitude)**: Universal benefit, increases survivability for all classes
- **Rogue/Warlock**: No party buffs (matches WoW Classic design - Rogues are selfish, Warlocks have curses/debuffs instead)

**Coefficient Design Philosophy:**
- **High Damage Spells (80-85%)**: Frostbolt, Mind Blast - reliable DPS, cast time balances power
- **Burst Abilities (120%)**: Ambush - high coefficient justifies high energy cost and stealth requirement
- **Moderate Spenders (50%)**: Sinister Strike - spammable, moderate coefficient for sustained DPS
- **Utility Damage (20%)**: Frost Nova - primarily for CC, damage is bonus
- **Healing (75%)**: Flash Heal - strong scaling to keep healing relevant as damage increases

---

## Implementation Details

### Combatant Struct Fields
```rust
pub struct Combatant {
    // ... other fields ...
    
    /// Attack Power - scales physical damage abilities and auto-attacks
    pub attack_power: f32,
    
    /// Spell Power - scales magical damage and healing abilities
    pub spell_power: f32,
    
    // ... other fields ...
}
```

### AbilityDefinition Struct Fields
```rust
pub struct AbilityDefinition {
    // ... other fields ...
    
    /// Base minimum damage (before stat scaling)
    pub damage_base_min: f32,
    /// Base maximum damage (before stat scaling)
    pub damage_base_max: f32,
    /// Coefficient: how much damage per point of Attack Power or Spell Power
    pub damage_coefficient: f32,
    /// What stat this ability's damage scales with
    pub damage_scales_with: ScalingStat,
    
    /// Base minimum healing (before stat scaling)
    pub healing_base_min: f32,
    /// Base maximum healing (before stat scaling)
    pub healing_base_max: f32,
    /// Coefficient: how much healing per point of Spell Power
    pub healing_coefficient: f32,
    
    // ... other fields ...
}
```

### ScalingStat Enum
```rust
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ScalingStat {
    /// Scales with Attack Power (physical abilities and auto-attacks)
    AttackPower,
    /// Scales with Spell Power (magical abilities and healing)
    SpellPower,
    /// Doesn't scale with any stat (CC abilities, utility)
    None,
}
```

### Helper Methods
```rust
impl Combatant {
    /// Calculate damage for an ability based on character stats.
    /// Formula: Base Damage + (Scaling Stat × Coefficient)
    pub fn calculate_ability_damage(&self, ability_def: &AbilityDefinition) -> f32 {
        // Random base damage
        let damage_range = ability_def.damage_base_max - ability_def.damage_base_min;
        let base_damage = ability_def.damage_base_min + (rand::random::<f32>() * damage_range);
        
        // Add stat scaling
        let stat_value = match ability_def.damage_scales_with {
            ScalingStat::AttackPower => self.attack_power,
            ScalingStat::SpellPower => self.spell_power,
            ScalingStat::None => 0.0,
        };
        
        base_damage + (stat_value * ability_def.damage_coefficient)
    }
    
    /// Calculate healing for an ability based on character stats.
    /// Formula: Base Healing + (Spell Power × Coefficient)
    pub fn calculate_ability_healing(&self, ability_def: &AbilityDefinition) -> f32 {
        // Random base healing
        let healing_range = ability_def.healing_base_max - ability_def.healing_base_min;
        let base_healing = ability_def.healing_base_min + (rand::random::<f32>() * healing_range);
        
        // Add spell power scaling (healing always scales with spell power)
        base_healing + (self.spell_power * ability_def.healing_coefficient)
    }
}
```

---

## Adding Gear (Future)

When the gear system is implemented, here's how it will work:

### 1. Create Gear Items
```rust
pub struct GearItem {
    pub name: String,
    pub slot: GearSlot, // Weapon, Chest, etc.
    pub attack_power_bonus: f32,
    pub spell_power_bonus: f32,
    // ... other stats ...
}
```

### 2. Equip Gear to Combatants
```rust
// When equipping gear, add its stats to the combatant
combatant.attack_power += gear_item.attack_power_bonus;
combatant.spell_power += gear_item.spell_power_bonus;
```

### 3. Abilities Automatically Scale
No changes needed! All abilities will automatically:
- Deal more damage (damage formulas use `combatant.attack_power` / `spell_power`)
- Heal for more (healing formulas use `combatant.spell_power`)

### Example: Adding a "Rusty Sword"
```rust
let rusty_sword = GearItem {
    name: "Rusty Sword".to_string(),
    slot: GearSlot::Weapon,
    attack_power_bonus: 10.0,  // +10 Attack Power
    spell_power_bonus: 0.0,
};

// Equip to Rogue (35 base AP)
rogue.attack_power += rusty_sword.attack_power_bonus; // Now 45 AP

// Sinister Strike damage BEFORE: 5-10 + (35 × 0.5) = 22.5-27.5
// Sinister Strike damage AFTER:  5-10 + (45 × 0.5) = 27.5-32.5
// +5 damage from gear, automatically!
```

---

## Balance Tuning

### Adjusting Ability Power
To make an ability stronger/weaker, adjust its coefficient:

```rust
// Frostbolt too weak? Increase coefficient
damage_coefficient: 0.8,  // Was: 80% of Spell Power
damage_coefficient: 0.9,  // Now: 90% of Spell Power (+10% more damage)

// Ambush too strong? Decrease coefficient
damage_coefficient: 1.2,  // Was: 120% of Attack Power
damage_coefficient: 1.0,  // Now: 100% of Attack Power (-20% less damage)
```

### Adjusting Class Power
To make a class stronger/weaker overall, adjust their base stats:

```rust
// Mages underperforming? Increase spell power
match_config::CharacterClass::Mage => (..., 50.0, 0.0, ...),  // Old
match_config::CharacterClass::Mage => (..., 55.0, 0.0, ...),  // New (+5 SP)
// All Mage spells now deal +10% more damage!

// Warriors overperforming? Decrease attack power
match_config::CharacterClass::Warrior => (..., 30.0, 0.0, ...),  // Old
match_config::CharacterClass::Warrior => (..., 25.0, 0.0, ...),  // New (-5 AP)
```

### Testing Balance
Use the match log files in `match_logs/` to analyze:
- Average damage per ability
- Time to kill
- Healing throughput
- Resource efficiency

Adjust coefficients and base stats to achieve desired balance.

---

## Why This System?

### ✅ Advantages
1. **Gear-Ready**: When gear is added, all abilities automatically scale
2. **Easy Balancing**: Adjust coefficients without touching combat logic
3. **WoW-Like Feel**: Familiar formula for players of similar games
4. **Clear Specialization**: Physical vs magical classes have distinct scaling
5. **Interesting Itemization**: Gear choices matter (AP vs SP)
6. **Consistent Formula**: All abilities use the same calculation method

### ❌ Previous System (Fixed Damage)
- Abilities dealt fixed damage (e.g., Frostbolt always 25-30)
- Gear would require modifying every ability definition
- No sense of character progression
- Less interesting build diversity

### 🎮 WoW Inspiration
World of Warcraft uses a similar formula:
```
Damage = (Base + Spell Power × Coefficient) × Crit × Other Modifiers
```

Our system is a simplified version:
```
Damage = Base + Stat × Coefficient
```

This captures the essential scaling behavior while keeping implementation simple for a prototype.

---

## Future Enhancements

Possible additions to the stat system:

1. **Critical Strike**: Chance to deal 2x damage
2. **Haste**: Reduce cast times and increase attack speed
3. **Armor**: Reduce physical damage taken
4. **Resistance**: Reduce magical damage taken
5. **Weapon Damage**: Physical abilities scale with weapon's base damage
6. **Armor Penetration**: Bypass target's armor
7. **Spell Penetration**: Bypass target's resistance

Each of these can be added as new stats on `Combatant` and integrated into the existing formula system.

---

## Summary

The stat scaling system transforms ArenaSim from a prototype with fixed damage values into a game ready for progression systems. By separating base values from stat scaling, we've created a flexible foundation that:

- Makes abilities scale naturally with character power
- Requires no code changes when gear is added
- Provides clear class specialization (physical vs magical)
- Enables interesting build choices through gear itemization
- Simplifies balance tuning through coefficient adjustments

When gear is implemented, players will immediately feel their character growing stronger as their stats increase, and all existing abilities will benefit without any rework.

