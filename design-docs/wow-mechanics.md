# WoW Mechanics Reference

Implemented WoW Classic mechanics adapted for our autobattler. Reference this document when implementing new abilities or debugging combat behavior.

---

## Combat System

### Global Cooldown (GCD)
- 1.5 seconds between most abilities
- Prevents ability spam, creates tactical decisions

### Casting
- Movement stops while casting non-instant spells
- Caster faces target when beginning a cast
- Cast bars show spell name and progress
- Cast can be interrupted by enemy abilities

### Interrupts & Spell Lockout
- Interrupting a cast locks that spell school for X seconds
- Only the interrupted school is locked (e.g., interrupting Frostbolt locks Frost, not Arcane)
- Spell schools: Physical, Fire, Frost, Shadow, Arcane, Holy, Nature

### Auto-Attacks
- Disabled while casting
- Melee: Within MELEE_RANGE (2.5 units)
- Ranged: Mage/Priest use "Wand Shots" at 40 unit range
- Attack speed varies by class

---

## Resource Systems

### Mana
| Class   | Max Mana | Regen/sec |
|---------|----------|-----------|
| Mage    | 200      | 10        |
| Priest  | 150      | 8         |
| Warlock | 200      | 8         |

### Rage (Warrior)
- Max: 100
- Generates on damage dealt and received
- Decays over time out of combat
- No passive regeneration

### Energy (Rogue)
- Max: 100
- Regenerates: 5/sec (constant rate)
- Instant regeneration tick model

---

## Crowd Control

### Root
- Prevents movement only
- Target can still attack and cast spells
- Example: Frost Nova (6s duration)

### Stun
- Prevents all actions (movement, attacking, casting)
- Examples: Kidney Shot, Charge stun component

### Fear
- Target runs in random directions
- Direction changes every 1-2 seconds
- Breaks on damage (threshold: 100 damage)
- Prevents intentional movement, attacking, casting
- Example: Warlock Fear (8-10s duration)

### Polymorph (Incapacitate)
- Target wanders at 50% speed
- Breaks on ANY damage (threshold: 0)
- Separate category from stuns for future diminishing returns
- Example: Mage Polymorph (10s duration)

### Future: Diminishing Returns
- Not yet implemented
- Same CC type on same target has reduced duration
- Categories: Stun, Fear, Incapacitate, Root

---

## Defensive Mechanics

### Absorb Shields
- Damage absorbed before health is reduced
- Multiple shields can coexist (Ice Barrier + Power Word: Shield)
- Each shield tracked by ability_name, not just AuraType
- Shield value depletes as damage is absorbed
- Visual: Bubble around shielded combatant

### Weakened Soul
- Applied by Power Word: Shield
- Prevents re-application of PW:S for 15 seconds
- Does NOT prevent other absorb effects (Ice Barrier)

### Stealth (Rogue)
- Invisible to enemies
- Breaks on damage or ability use
- Visual: 40% opacity, purple "STEALTH" label
- Shadow Sight orbs spawn after 90s to counter stealth stalemates

---

## Buffs

### Pre-Match Buffing Phase
- 10 second countdown before gates open
- Mana restored each frame during countdown
- Combatants can cast buffs on allies
- Examples: Power Word: Fortitude, Arcane Intellect

### Stat Buffs
| Buff               | Effect        | Duration |
|--------------------|---------------|----------|
| Fortitude          | +100 Max HP   | 300s     |
| Arcane Intellect   | +40 Max Mana  | 300s     |

---

## Spell Schools

| School   | Color       | Classes Using                    |
|----------|-------------|----------------------------------|
| Physical | White       | Warrior, Rogue                   |
| Fire     | Orange      | (Future: Mage Fire spec)         |
| Frost    | Blue        | Mage (Frostbolt, Frost Nova, Ice Barrier) |
| Shadow   | Purple      | Warlock, Priest (Mind Blast)     |
| Arcane   | Pink/Purple | Mage (Polymorph)                 |
| Holy     | Gold        | Priest (Flash Heal, PW:S, Fort)  |
| Nature   | Green       | (Future: Druid, Shaman)          |

---

## Damage & Healing Formulas

### Stat Scaling
```
Damage = Base + (Stat × Coefficient)
Healing = Base + (Spell Power × Coefficient)
```

### Scaling Stats
- `AttackPower`: Warrior, Rogue physical abilities
- `SpellPower`: Mage, Priest, Warlock magical abilities
- `None`: Utility/CC abilities with no scaling

### Class Base Stats
| Class   | Attack Power | Spell Power |
|---------|--------------|-------------|
| Warrior | 30           | 0           |
| Rogue   | 35           | 0           |
| Mage    | 0            | 50          |
| Priest  | 0            | 40          |
| Warlock | 0            | 45          |

### Example Coefficients
| Ability       | Base Damage | Coefficient | Scales With |
|---------------|-------------|-------------|-------------|
| Frostbolt     | 10-15       | 80%         | SpellPower  |
| Ambush        | 20-30       | 120%        | AttackPower |
| Flash Heal    | 15-20       | 75%         | SpellPower  |
| Mind Blast    | 15-20       | 85%         | SpellPower  |

---

## Break-on-Damage Semantics

For auras that can break on damage:

| Threshold Value | Behavior                          |
|-----------------|-----------------------------------|
| `-1.0`          | Never breaks on damage (buffs)    |
| `0.0`           | Breaks on ANY damage (Polymorph)  |
| `100.0`         | Breaks after 100 cumulative damage (Fear) |

---

## Aura Types Reference

| AuraType            | Effect                                    |
|---------------------|-------------------------------------------|
| `Absorb`            | Damage shield that depletes              |
| `Root`              | Prevents movement                        |
| `Stun`              | Prevents all actions                     |
| `Fear`              | Random movement, breaks on damage        |
| `Polymorph`         | Slow wander, breaks on any damage        |
| `MovementSpeedSlow` | Reduces movement speed by magnitude %    |
| `HealingReduction`  | Reduces healing received (Mortal Strike) |
| `DamageOverTime`    | Periodic damage ticks                    |
| `MaxHealthIncrease` | Temporary max HP buff                    |
| `MaxManaIncrease`   | Temporary max mana buff                  |
| `SpellLockout`      | Prevents casting school for duration     |
| `WeakenedSoul`      | Prevents PW:S reapplication              |
| `ShadowSight`       | Can see stealthed enemies                |
