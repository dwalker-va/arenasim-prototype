---
status: pending
priority: p3
issue_id: "007"
tags: [code-review, quality, paladin, constants]
dependencies: []
---

# Magic Numbers for HP Thresholds

## Problem Statement

The Paladin AI uses hardcoded HP threshold values (0.40, 0.50, 0.70, 0.85) scattered throughout the code. These should be named constants in `constants.rs` for consistency with how other thresholds are handled.

## Findings

**Locations in paladin.rs**:
- Line 47: `ally.hp_percent < 0.40` (emergency threshold)
- Line 54: `ally.hp_percent >= 0.70` (healthy threshold)
- Line 329-332: `hp_percent >= 0.50 && hp_percent < 0.85` (heal range)
- Line 404-407: `hp_percent < 0.50` (shield threshold)

**Existing Constants** (`constants.rs`):
```rust
pub const DEFENSIVE_HP_THRESHOLD: f32 = 0.8;
pub const SAFE_KITING_DISTANCE: f32 = 20.0;
// etc.
```

The codebase already has a pattern for these - Paladin should follow it.

## Proposed Solutions

### Option A: Add Constants to constants.rs (Recommended)
Define named constants for all HP thresholds.

**Pros**: Self-documenting, easy to tune
**Cons**: Minor code change
**Effort**: Small (20 minutes)
**Risk**: Low

```rust
// In constants.rs
pub const EMERGENCY_HP_THRESHOLD: f32 = 0.40;
pub const HEALTHY_HP_THRESHOLD: f32 = 0.70;
pub const HEAL_MIN_HP_THRESHOLD: f32 = 0.50;
pub const HEAL_MAX_HP_THRESHOLD: f32 = 0.85;
```

### Option B: Paladin-Specific Constants
Define constants at the top of paladin.rs.

**Pros**: Keeps class-specific values together
**Cons**: May duplicate across healer classes
**Effort**: Small (15 minutes)
**Risk**: Low

## Recommended Action

**Option A** - Use shared constants. These thresholds likely apply to other healers (Priest) as well.

## Technical Details

**Affected Files**:
- `src/states/play_match/constants.rs` (add constants)
- `src/states/play_match/class_ai/paladin.rs` (use constants)

## Acceptance Criteria

- [ ] No magic numbers for HP thresholds in paladin.rs
- [ ] Constants defined in constants.rs
- [ ] Build succeeds
- [ ] No functional change

## Work Log

| Date | Action | Notes |
|------|--------|-------|
| 2026-02-02 | Created | From pattern-recognition-specialist review |

## Resources

- Pattern recognition review: identified as anti-pattern
- Existing constants: `src/states/play_match/constants.rs`
