---
title: "Adding a New Class: Paladin Implementation Patterns"
tags:
  - paladin
  - healer
  - class-ai
  - ability-icons
  - dispel-system
  - code-review
  - refactoring
  - performance-optimization
  - pending-components
category: implementation-patterns
module: states/play_match
symptoms:
  - "Missing spell icons on view combatant screen for new abilities"
  - "Duplicate system logic between classes"
  - "Combat log showing incorrect caster attribution"
  - "Pending components lacking caster information"
  - "Hardcoded ability lists requiring manual updates"
  - "Inefficient ally/enemy list computation in AI logic"
severity: medium
date_documented: 2026-01-31
---

# Adding a New Class: Paladin Implementation Patterns

This document captures lessons learned from implementing the Paladin healing class, including code quality improvements from a comprehensive review. Use this as a reference when adding future classes.

## Overview

The Paladin implementation involved:
1. Adding a full healer class with 6 abilities
2. Fixing UI icon display issues
3. Addressing P2 (Important) code review findings
4. Addressing P3 (Nice-to-Have) code quality improvements

**Net result**: 132 lines removed through refactoring while adding new functionality.

## Key Solutions

### 1. Dynamic Ability Icon Loading

**Problem**: The `load_ability_icons` function used a hardcoded `SPELL_ICON_ABILITIES` list that required manual updates for every new ability.

**Solution**: Dynamically collect abilities from all classes:

```rust
// src/states/view_combatant_ui.rs
let all_classes = [
    CharacterClass::Warrior, CharacterClass::Mage, CharacterClass::Rogue,
    CharacterClass::Priest, CharacterClass::Warlock, CharacterClass::Paladin,
];
let mut ability_names: Vec<&'static str> = Vec::new();
for class in &all_classes {
    for ability in get_class_abilities(*class) {
        let name = get_ability_name(ability);
        if !ability_names.contains(&name) {
            ability_names.push(name);
        }
    }
}
```

**Pattern**: Single Source of Truth - derive data from canonical definitions rather than maintaining parallel lists.

### 2. Icon Path Aliases

**Problem**: "Shadow Bolt" icon not loading due to naming inconsistency ("Shadowbolt" vs "Shadow Bolt").

**Solution**: Add alias mapping in `get_ability_icon_path()`:

```rust
// src/states/play_match/rendering/mod.rs
"Shadowbolt" | "Shadow Bolt" => Some("icons/abilities/spell_shadow_shadowbolt.jpg"),
```

**Pattern**: Use `|` in match arms to handle multiple name variants gracefully.

### 3. Combat Log Attribution

**Problem**: Combat log entries for Holy Shock and Cleanse showed incorrect or missing caster attribution.

**Solution**: Add caster info to pending components:

```rust
// src/states/play_match/components/mod.rs
pub struct HolyShockHealPending {
    pub caster_spell_power: f32,
    pub caster_team: u8,
    pub caster_class: CharacterClass,
    pub target: Entity,
}
```

For dispels, add a log prefix field:

```rust
// src/states/play_match/class_ai/priest.rs
pub struct DispelPending {
    pub target: Entity,
    pub log_prefix: &'static str,  // "[DISPEL]" or "[CLEANSE]"
}
```

**Pattern**: Context Propagation - carry caster identity through the pending component lifecycle.

### 4. Unified Dispel System

**Problem**: Separate `process_dispels` (Priest) and `process_paladin_dispels` (Paladin) with duplicate logic.

**Solution**: Merge into single system using shared `DispelPending` component:

```rust
// Both classes spawn the same component with different log_prefix
commands.spawn(DispelPending {
    target: target.entity,
    log_prefix: "[CLEANSE]",  // or "[DISPEL]" for Priest
});
```

**Pattern**: Unified System - single processing system handles multiple ability sources.

### 5. Pre-computed Ally/Enemy Lists

**Problem**: Paladin AI repeatedly iterated over `combatant_info` HashMap to find allies and enemies.

**Solution**: Define lightweight info structs and pre-compute once:

```rust
// src/states/play_match/class_ai/paladin.rs
struct AllyInfo {
    entity: Entity,
    class: CharacterClass,
    hp_percent: f32,
    pos: Vec3,
}

// Pre-compute at start of AI decision
let allies: Vec<AllyInfo> = combatant_info
    .iter()
    .filter(|(_, (team, _, _, hp, _))| *team == combatant.team && *hp > 0.0)
    .filter_map(|(e, (_, _, class, hp, max_hp))| {
        positions.get(e).map(|pos| AllyInfo {
            entity: *e, class: *class, hp_percent: *hp / *max_hp, pos: *pos,
        })
    })
    .collect();
```

**Pattern**: Pre-computation - calculate expensive data once, reuse multiple times.

### 6. SmallVec for Stack Allocation

**Problem**: Dispel processing allocated heap `Vec` for dispellable indices, even though typical count is 1-3.

**Solution**: Use SmallVec with appropriate inline capacity:

```rust
// src/states/play_match/auras.rs
use smallvec::SmallVec;

let dispellable_indices: SmallVec<[usize; 8]> = active_auras
    .auras.iter().enumerate()
    .filter(|(_, a)| a.can_be_dispelled())
    .map(|(i, _)| i)
    .collect();
```

**Pattern**: SmallVec - stack-allocate up to N elements, only heap-allocate if exceeded.

### 7. Shared Utility Functions

**Problem**: Both Priest and Paladin needed identical "is team healthy" check for maintenance tasks.

**Solution**: Extract to shared function in parent module:

```rust
// src/states/play_match/class_ai/mod.rs
pub fn is_team_healthy(
    team: u8,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32)>,
) -> bool {
    for &(ally_team, _, _, ally_hp, ally_max_hp) in combatant_info.values() {
        if ally_team != team || ally_hp <= 0.0 { continue; }
        if ally_hp / ally_max_hp < 0.70 { return false; }
    }
    true
}
```

**Pattern**: Shared Utility - extract common logic to parent module for reuse.

## Code Review Checklist for New Classes

### Component & Module Placement
- [ ] Pending components in `components/mod.rs` (NOT in class-specific files)
- [ ] Aura types in `components/auras.rs`
- [ ] No class-specific component files

### Ability Implementation
- [ ] Ability added to `AbilityType` enum in `abilities.rs`
- [ ] Ability added to validation list in `ability_config.rs`
- [ ] Ability defined in `abilities.ron`
- [ ] Icon path added to `get_ability_icon_path()` in `rendering/mod.rs`
- [ ] Icon file downloaded to `assets/icons/abilities/`

### System Registration
- [ ] Systems registered in `play_match/mod.rs`
- [ ] No duplicate system logic (search codebase first!)
- [ ] Proper Bevy scheduling

### Combat Log Integration
- [ ] All damage/healing events include caster information
- [ ] Aura applications logged with source and target
- [ ] Dispels logged with proper prefix

### AI Logic
- [ ] Use `CombatContext` helpers
- [ ] Ability priorities documented in comments
- [ ] Pre-compute iteration-heavy data

## Testing Recommendations

### Headless Match Test Suite

```bash
# Solo survivability
echo '{"team1":["Paladin"],"team2":[]}' > /tmp/solo.json

# Mirror match
echo '{"team1":["Paladin"],"team2":["Paladin"]}' > /tmp/mirror.json

# vs Each class
for class in Warrior Mage Rogue Priest Warlock; do
  echo "{\"team1\":[\"Paladin\"],\"team2\":[\"$class\"]}" > /tmp/vs_$class.json
  cargo run --release -- --headless /tmp/vs_$class.json
done

# Team compositions
echo '{"team1":["Warrior","Paladin"],"team2":["Warrior","Priest"]}' > /tmp/2v2.json
```

### Log Validation Checklist
- [ ] All abilities show correct caster names (no "Unknown")
- [ ] Damage/healing amounts are reasonable
- [ ] Auras applying and expiring correctly
- [ ] No duplicate event entries
- [ ] Match ends properly

## Files Modified

| File | Changes |
|------|---------|
| `states/match_config.rs` | `CharacterClass::Paladin` variant |
| `abilities.rs` | 6 new `AbilityType` variants |
| `ability_config.rs` | Validation entries, `get_class_abilities()` |
| `abilities.ron` | All Paladin ability definitions |
| `components/mod.rs` | `HolyShockHealPending`, `HolyShockDamagePending` |
| `class_ai/paladin.rs` | New AI logic with pre-computed lists |
| `class_ai/mod.rs` | Import, match arm, `is_team_healthy()` |
| `class_ai/priest.rs` | Added `log_prefix` to `DispelPending` |
| `auras.rs` | Unified dispel system, SmallVec |
| `rendering/mod.rs` | Icon paths |
| `view_combatant_ui.rs` | Dynamic icon loading |

## Related Documentation

- **[WoW Mechanics](../../../design-docs/wow-mechanics.md)** - Implemented mechanics reference
- **[Bevy Patterns](../../../design-docs/bevy-patterns.md)** - ECS patterns and bug detection checklist
- **[Stat Scaling](../../../design-docs/stat-scaling-system.md)** - Damage/healing formulas
- **[Two-Agent Bug Hunting](../workflows/two-agent-bug-hunting.md)** - Testing methodology

## Commits

```
d23fcfe feat(paladin): add Paladin healing class with full ability kit
a4f5a7f fix(ui): display Shadow Bolt icon on view combatant screen
98dbf64 fix(paladin): address P2 code review findings
053f8e2 chore(paladin): address easy P3 code review findings
52cc317 refactor(paladin): address remaining P3 code review findings
```
