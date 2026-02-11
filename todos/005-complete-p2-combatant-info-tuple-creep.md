---
status: complete
priority: p2
issue_id: "005"
tags: [code-review, architecture, technical-debt]
dependencies: []
---

# combatant_info Tuple Creep (6 Elements)

## Problem Statement

The `combatant_info` HashMap uses a 6-element tuple `(u8, u8, CharacterClass, f32, f32, bool)` that is increasingly opaque. Adding the `stealthed` field required modifying 50+ function signatures across 8 files. This pattern makes the codebase brittle and hard to extend.

## Findings

**Current State** (`combat_ai.rs:286-291`):
```rust
let combatant_info: HashMap<Entity, (u8, u8, CharacterClass, f32, f32, bool)> = ...
// Elements: team, slot, class, hp, max_hp, stealthed
```

**Problems**:
1. Positional fields are not self-documenting
2. Adding fields requires widespread signature changes
3. Most functions destructure but ignore fields: `(ally_team, _, _ally_class, ally_hp, _ally_max_hp, _)`

**Existing Better Abstraction** (`class_ai/mod.rs:31-46`):
```rust
pub struct CombatantInfo {
    pub entity: Entity,
    pub team: u8,
    pub class: CharacterClass,
    pub current_health: f32,
    pub max_health: f32,
    pub stealthed: bool,
    // ... more fields
}
```

This struct already exists but is not used for the AI HashMap.

## Proposed Solutions

### Option A: Migrate to CombatantInfo Struct (Recommended)
Replace the tuple with the existing `CombatantInfo` struct.

**Pros**: Self-documenting, extensible, struct already exists
**Cons**: Large refactor across 8 files
**Effort**: Large (4-6 hours)
**Risk**: Medium

```rust
let combatant_info: HashMap<Entity, CombatantInfo> = ...
```

### Option B: Create Dedicated CombatantSnapshot Struct
Create a new, minimal struct specifically for AI decision making.

**Pros**: Purpose-built, can omit unused fields
**Cons**: Yet another struct, potential duplication
**Effort**: Medium (2-3 hours)
**Risk**: Low

```rust
#[derive(Clone, Copy)]
pub struct CombatantSnapshot {
    pub team: u8,
    pub slot: u8,
    pub class: CharacterClass,
    pub current_health: f32,
    pub max_health: f32,
    pub stealthed: bool,
}
```

### Option C: Accept Technical Debt
Keep the tuple and document the field positions.

**Pros**: No refactor needed
**Cons**: Problem worsens with each new field
**Effort**: None
**Risk**: Low (for now)

## Recommended Action

**Option A** - Migrate to existing `CombatantInfo` struct. The struct already exists in `mod.rs` and aligns with the `CombatContext` architecture vision. This is technical debt that should be paid before it gets worse.

## Technical Details

**Affected Files** (8 total):
- `src/states/play_match/combat_ai.rs`
- `src/states/play_match/class_ai/mod.rs`
- `src/states/play_match/class_ai/paladin.rs`
- `src/states/play_match/class_ai/priest.rs`
- `src/states/play_match/class_ai/mage.rs`
- `src/states/play_match/class_ai/warrior.rs`
- `src/states/play_match/class_ai/rogue.rs`
- `src/states/play_match/class_ai/warlock.rs`

## Acceptance Criteria

- [ ] combatant_info uses a named struct instead of tuple
- [ ] All class AI files updated to use struct field access
- [ ] Build succeeds with no warnings
- [ ] Headless tests pass
- [ ] No functional changes to AI behavior

## Work Log

| Date | Action | Notes |
|------|--------|-------|
| 2026-02-02 | Created | From architecture-strategist review |

## Resources

- Architecture review: identified as "Tuple Creep" concern
- Existing struct: `src/states/play_match/class_ai/mod.rs:31-46`
