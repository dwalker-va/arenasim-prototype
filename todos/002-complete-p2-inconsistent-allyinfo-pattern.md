---
status: pending
priority: p2
issue_id: "002"
tags: [code-review, architecture, paladin, consistency]
dependencies: []
---

# Inconsistent AllyInfo/EnemyInfo Pattern in Paladin AI

## Problem Statement

The Paladin AI defines custom `AllyInfo` and `EnemyInfo` structs for pre-computing combatant data, while all other class AIs (Priest, Warrior, Mage, Rogue, Warlock) iterate directly over `combatant_info`. This creates maintenance burden and codebase inconsistency.

## Findings

**Location**: `src/states/play_match/class_ai/paladin.rs:32-49`

```rust
/// Pre-computed ally information to avoid repeated iteration over combatant_info
struct AllyInfo {
    entity: Entity,
    class: CharacterClass,
    hp_percent: f32,
    pos: Vec3,
}

/// Pre-computed enemy information to avoid repeated iteration over combatant_info
struct EnemyInfo {
    entity: Entity,
    class: CharacterClass,
    pos: Vec3,
}
```

**Contrast with Priest** (`priest.rs:187`):
```rust
for (ally_entity, &(ally_team, _, _ally_class, ally_hp, _ally_max_hp, _)) in combatant_info.iter() {
```

The comment claims "to avoid repeated iteration" but:
- Pre-computing allies/enemies once is reasonable
- But the custom structs add 16 lines of boilerplate
- Different function signatures break consistency (`&[AllyInfo]` vs `&HashMap<...>`)

## Proposed Solutions

### Option A: Standardize to Direct Iteration (Recommended)
Remove AllyInfo/EnemyInfo and iterate combatant_info directly like other classes.

**Pros**: Consistency with all other class AIs, less code
**Cons**: Slightly more verbose iteration in each function
**Effort**: Medium (1-2 hours)
**Risk**: Low

### Option B: Adopt Pattern Codebase-Wide
Update all class AIs to use the pre-computed pattern.

**Pros**: Consistent optimization everywhere
**Cons**: Large refactor, may not be needed for simpler classes
**Effort**: Large (4-6 hours)
**Risk**: Medium

### Option C: Document as Intentional Deviation
Keep the pattern but document why Paladin differs.

**Pros**: No code changes
**Cons**: Perpetuates inconsistency
**Effort**: Small (15 minutes)
**Risk**: Low

## Recommended Action

**Option A** - Refactor Paladin to match established patterns. Consistency is more valuable than micro-optimization at current scale (3v3 arena).

## Technical Details

**Affected Files**:
- `src/states/play_match/class_ai/paladin.rs`

**Functions to Update**:
- `decide_paladin_action()`
- `try_flash_of_light()`
- `try_holy_light()`
- `try_holy_shock_heal()`
- `try_holy_shock_damage()`
- `try_cleanse()`
- `try_devotion_aura()`
- `try_hammer_of_justice()`

## Acceptance Criteria

- [ ] Paladin AI uses same combatant_info iteration pattern as Priest
- [ ] AllyInfo/EnemyInfo structs removed
- [ ] Function signatures match other class AIs
- [ ] All headless tests pass
- [ ] No performance regression (verify with profiling if concerned)

## Work Log

| Date | Action | Notes |
|------|--------|-------|
| 2026-02-02 | Created | From pattern-recognition-specialist review |

## Resources

- Pattern recognition review findings
- Reference: `src/states/play_match/class_ai/priest.rs` for canonical pattern
