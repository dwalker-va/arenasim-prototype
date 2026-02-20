---
status: pending
priority: p2
issue_id: "003"
tags: [code-review, duplication, paladin, priest]
dependencies: []
---

# Duplicate Dispel Priority Logic Between Paladin and Priest

## Problem Statement

The Paladin's `try_cleanse()` function duplicates the dispel priority logic from Priest's `try_dispel_magic()`. Both have identical priority scoring for aura types (Polymorph=100, Fear=90, Root=80, DoT=50, Slow=20). This violates DRY and creates maintenance burden.

## Findings

**Paladin** (`paladin.rs:669-681`):
```rust
let priority = match aura.effect_type {
    AuraType::Polymorph => 100,
    AuraType::Fear => 90,
    AuraType::Root => 80,
    AuraType::DamageOverTime => 50,
    AuraType::MovementSpeedSlow => 20,
    _ => 0,
};
```

**Priest** (`priest.rs:503-517`):
```rust
let priority = match aura.effect_type {
    AuraType::Polymorph => 100,  // Highest - complete incapacitate
    AuraType::Fear => 90,         // Very high - loss of control
    AuraType::Root => 80,         // High - can't move
    AuraType::DamageOverTime => 50,  // Medium - taking damage
    AuraType::MovementSpeedSlow => 20, // Too low to dispel (threshold is 50)
    _ => 0, // Other types
};
```

The code is **identical** except for comments.

## Proposed Solutions

### Option A: Extract to Shared Function in mod.rs (Recommended)
Create `fn dispel_priority(aura_type: AuraType) -> i32` in `class_ai/mod.rs`.

**Pros**: Single source of truth, easy to update priorities
**Cons**: Minor refactor
**Effort**: Small (30 minutes)
**Risk**: Low

```rust
// In class_ai/mod.rs
pub fn dispel_priority(aura_type: AuraType) -> i32 {
    match aura_type {
        AuraType::Polymorph => 100,
        AuraType::Fear => 90,
        AuraType::Root => 80,
        AuraType::DamageOverTime => 50,
        AuraType::MovementSpeedSlow => 20,
        _ => 0,
    }
}
```

### Option B: Extract Constants
Define priority constants and use in both files.

**Pros**: Named constants are self-documenting
**Cons**: Still duplicates the match logic
**Effort**: Small (20 minutes)
**Risk**: Low

### Option C: Shared try_friendly_dispel Function
Extract the entire dispel targeting logic into a shared helper.

**Pros**: Maximum code reuse
**Cons**: Larger refactor, may over-abstract
**Effort**: Medium (1-2 hours)
**Risk**: Medium

## Recommended Action

**Option A** - Extract just the priority function. This is the minimum viable DRY fix.

## Technical Details

**Affected Files**:
- `src/states/play_match/class_ai/mod.rs` (add shared function)
- `src/states/play_match/class_ai/paladin.rs` (use shared function)
- `src/states/play_match/class_ai/priest.rs` (use shared function)

## Acceptance Criteria

- [ ] Dispel priority logic exists in exactly one place
- [ ] Both Paladin and Priest use the shared function
- [ ] No functional change to dispel behavior
- [ ] Headless tests pass

## Work Log

| Date | Action | Notes |
|------|--------|-------|
| 2026-02-02 | Created | From pattern-recognition-specialist review |

## Resources

- Pattern recognition review: identified duplication
- Simplicity review: flagged as YAGNI violation
