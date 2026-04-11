---
title: "docs: Update documentation after adding Paladin class"
type: docs
date: 2026-02-02
status: completed
---

# 📚 Update Documentation After Adding Paladin Class

## Overview

The Paladin class was just merged to main with 6 abilities (Devotion Aura, Flash of Light, Holy Light, Holy Shock, Cleanse, Hammer of Justice). Several documentation files reference class lists that are now out of date.

## Files to Update

### 1. CLAUDE.md

**Line 22** - Config options list:
```diff
- - `team1`, `team2`: Arrays of class names (Warrior, Mage, Rogue, Priest, Warlock)
+ - `team1`, `team2`: Arrays of class names (Warrior, Mage, Rogue, Priest, Warlock, Paladin)
```

**Lines 69-73** - class_ai file list:
```diff
        warrior.rs        # Warrior ability priorities
        mage.rs           # Mage kiting, control logic
        rogue.rs          # Rogue stealth, burst logic
        priest.rs         # Priest healing priorities
        warlock.rs        # Warlock DoT management
+       paladin.rs        # Paladin healing and utility
```

**Lines 176-180** - Class Design section:
```diff
- **Warrior**: Rage (generates on damage), melee, Charge/Mortal Strike/Pummel
- **Mage**: Mana, ranged, Frostbolt/Frost Nova/Polymorph
- **Rogue**: Energy, melee, Stealth/Ambush/Kick/Eviscerate
- **Priest**: Mana, healer, Flash Heal/Mind Blast/Power Word: Fortitude
- **Warlock**: Mana, DoT caster, Corruption/Shadow Bolt/Fear
+ - **Paladin**: Mana, healer/melee, Holy Shock/Flash of Light/Hammer of Justice
```

### 2. design-docs/roadmap.md

**Line 6** - Classes count:
```diff
- - **Classes**: Warrior, Mage, Rogue, Priest, Warlock (5)
+ - **Classes**: Warrior, Mage, Rogue, Priest, Warlock, Paladin (6)
```

### 3. design-docs/wow-mechanics.md

**Mana section (around line 37)** - Add Paladin stats:
```diff
| Mage    | 200      | 10        |
| Priest  | 150      | 8         |
| Warlock | 200      | 8         |
+ | Paladin | 160      | 8         |
```

**Spell Schools section (around line 132)** - Update Holy school:
```diff
- | Holy     | Gold        | Priest (Flash Heal, PW:S, Fort)  |
+ | Holy     | Gold        | Priest, Paladin (heals, Holy Shock, HoJ) |
```

**Base Stats section (around line 153)** - Add Paladin:
```diff
| Warrior | 30           | 0           |
| Rogue   | 35           | 0           |
| Mage    | 0            | 50          |
| Priest  | 0            | 40          |
| Warlock | 0            | 45          |
+ | Paladin | 20           | 35          |
```

**Add new section** - Paladin abilities:
```markdown
### Paladin Abilities

- **Devotion Aura**: AoE buff, 10% damage reduction to all allies
- **Flash of Light**: 1.5s cast, fast efficient heal
- **Holy Light**: 2.5s cast, large heal for safe situations
- **Holy Shock**: Instant, heals allies OR damages enemies (20 yard range for damage)
- **Cleanse**: Instant, removes 1 dispellable debuff (Polymorph/Fear/Root/DoT)
- **Hammer of Justice**: 10 yard range, 6s stun, prioritizes healers
```

## Acceptance Criteria

- [x] CLAUDE.md lists all 6 classes in config options (line 22)
- [x] CLAUDE.md class_ai section includes paladin.rs (lines 69-73)
- [x] CLAUDE.md Class Design includes Paladin description (lines 176-180)
- [x] roadmap.md shows 6 classes
- [x] wow-mechanics.md has Paladin in mana table
- [x] wow-mechanics.md has Paladin in spell schools (Holy)
- [x] wow-mechanics.md has Paladin in base stats table
- [x] wow-mechanics.md has Paladin abilities section

## References

- Paladin implementation: `src/states/play_match/class_ai/paladin.rs`
- Implementation pattern doc: `docs/solutions/implementation-patterns/adding-new-class-paladin.md`
- Recent commits: `d460162`, `d03c64f`
