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

### 1. Data-Driven Ability Icons

**Problem**: Ability icons were split across three places: `get_ability_icon_path()` match, `SPELL_ICON_ABILITIES` constant, and `abilities.ron` definitions.

**Solution**: Icon paths are defined directly in `abilities.ron` alongside each ability definition:

```ron
// assets/config/abilities.ron
Frostbolt: (
    name: "Frostbolt",
    icon: "icons/abilities/spell_frost_frostbolt02.jpg",
    // ... other fields
),
```

Both icon loading systems (`load_spell_icons`, `load_ability_icons`) iterate `AbilityDefinitions` to load icons. An `all_abilities_have_icons` test enforces that every ability has an icon path.

**Pattern**: Single Source of Truth - icon paths live next to the ability definition, eliminating name aliasing issues and parallel list maintenance.

### 3. Combat Log Attribution

**Problem**: Combat log entries for Holy Shock and Cleanse showed incorrect or missing caster attribution.

**Solution**: Add caster info to pending components:

```rust
// src/states/play_match/components/combatant.rs
pub struct HolyShockHealPending {
    pub caster_spell_power: f32,
    pub caster_crit_chance: f32,
    pub caster_team: u8,
    pub caster_class: CharacterClass,
    pub target: Entity,
}
```

For dispels, add a log prefix field. `DispelPending` now lives alongside the other pending components in `components/combatant.rs` (it started life in `class_ai/priest.rs` when only the Priest dispelled, then moved out once the Paladin reused it):

```rust
// src/states/play_match/components/combatant.rs
pub struct DispelPending {
    pub target: Entity,
    pub log_prefix: &'static str,  // "[DISPEL]" or "[CLEANSE]"
}
```

**Pattern**: Context Propagation - carry caster identity through the pending component lifecycle.

### 4. Unified Dispel System

**Problem**: Separate `process_dispels` (Priest) and `process_paladin_dispels` (Paladin) with duplicate logic.

**Solution**: Merge into a single system (`src/states/play_match/effects/dispels.rs::process_dispels`) using a shared `DispelPending` component:

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

**Solution**: Pre-compute once. The original Paladin patch introduced an ad-hoc `AllyInfo` struct inside `class_ai/paladin.rs`; that lookup work has since been hoisted into the shared `CombatSnapshot` (`class_ai/combat_snapshot.rs`), which builds a single typed per-frame view consumed by every class's `decide_action` via `CombatContext`. New classes should read allies and enemies through `ctx`, not by rebuilding the HashMap-of-tuples themselves.

**Pattern**: Pre-computation - calculate expensive data once per frame, reuse across every class's AI decision.

### 6. SmallVec for Stack Allocation

**Problem**: Dispel processing allocated heap `Vec` for dispellable indices, even though typical count is 1-3.

**Solution**: Use SmallVec with appropriate inline capacity:

```rust
// src/states/play_match/effects/dispels.rs
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

**Solution**: Extract to a shared method on `CombatContext` (it started as a free function in `class_ai/mod.rs`; once `CombatContext` became the canonical per-frame view, it migrated onto the context):

```rust
// src/states/play_match/class_ai/mod.rs
impl<'a> CombatContext<'a> {
    pub fn is_team_healthy(&self, threshold: f32, my_pos: Vec3) -> bool { /* ... */ }
}

// Callers:
if ctx.is_team_healthy(0.70, my_pos) { /* idle */ }
```

**Pattern**: Shared Utility - extract common logic to the per-frame context for reuse across class AIs.

## Code Review Checklist for New Classes

### Component & Module Placement
- [ ] Gameplay pending components in `components/combatant.rs` (NOT in class-specific files)
- [ ] Aura types in `components/auras.rs`
- [ ] Visual marker components in `components/visual.rs`
- [ ] Pet components in `components/pets.rs`, resource components in `components/resources.rs`
- [ ] No class-specific component files

### Ability Implementation
- [ ] Ability added to `AbilityType` enum in `abilities.rs`
- [ ] Ability added to validation list in `ability_config.rs`
- [ ] Ability defined in `abilities.ron`
- [ ] Icon path added to ability entry in `abilities.ron`
- [ ] Icon file downloaded to `assets/icons/abilities/`

### System Registration
- [ ] Core combat systems registered in `add_core_combat_systems()` (`src/states/play_match/systems.rs`) — runs in BOTH headless and graphical modes
- [ ] Visual-only systems registered in `StatesPlugin::build()` (`src/states/mod.rs`) — graphical only
- [ ] `cargo test` passes `tests/registration_audit.rs` (this audit catches missing registrations and tells you which path to use; see `CLAUDE.md` "Adding a New Combat System")
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
| `components/combatant.rs` | `HolyShockHealPending`, `HolyShockDamagePending`, `DispelPending` |
| `class_ai/paladin.rs` | New AI logic; today, allies/enemies come from `CombatContext` / `CombatSnapshot` |
| `class_ai/mod.rs` | Import, match arm, `is_team_healthy()` method on `CombatContext` |
| `effects/dispels.rs` | Unified dispel system (`process_dispels`), SmallVec |
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
