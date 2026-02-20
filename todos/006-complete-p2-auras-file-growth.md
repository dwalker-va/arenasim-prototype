---
status: pending
priority: p2
issue_id: "006"
tags: [code-review, architecture, single-responsibility]
dependencies: []
---

# auras.rs File Growth (Single Responsibility Violation)

## Problem Statement

The `auras.rs` file has grown to 934 lines (+36% from main) and now contains Holy Shock processing systems that have no conceptual relationship to "auras." The file is becoming a catch-all for "pending effect processing," violating single responsibility principle.

## Findings

**File Size Growth**:
- main branch: 688 lines
- feat/paladin-class: 934 lines (+246 lines, +36%)

**Contents of auras.rs now include**:
- Aura lifecycle (update, apply, break) - **belongs here**
- DoT tick processing - **belongs here**
- Dispel processing - **questionable**
- Holy Shock processing - **does not belong here**

**Holy Shock Systems** (`auras.rs` in diff):
- `process_holy_shock_heals()` - 73 lines
- `process_holy_shock_damage()` - 134 lines

These process instant effects that do not apply buffs/debuffs. They happen to use the "pending component" pattern, but that doesn't make them aura-related.

## Proposed Solutions

### Option A: Extract to Effects Module (Recommended)
Create a new `effects/` module for ability-specific processing.

**Pros**: Clear separation of concerns, focused files
**Cons**: More files to navigate
**Effort**: Medium (1-2 hours)
**Risk**: Low

```
src/states/play_match/
  effects/
    mod.rs
    holy_shock.rs     # process_holy_shock_heals, process_holy_shock_damage
    dispels.rs        # process_dispels (move from auras.rs)
  auras.rs            # Keep only aura-related systems
```

### Option B: Extract to Class-Specific Module
Move Holy Shock processing to `class_ai/paladin.rs`.

**Pros**: Keeps class logic together
**Cons**: class_ai is for decision logic, not effect processing
**Effort**: Small (30 minutes)
**Risk**: Low

### Option C: Accept Current Structure
Keep everything in auras.rs until it becomes more problematic.

**Pros**: No refactor needed
**Cons**: File continues to grow with each new class
**Effort**: None
**Risk**: Medium (technical debt accumulation)

## Recommended Action

**Option A** - This establishes a pattern for future classes. When Shaman is added with instant effects, there's a clear home for the processing systems.

## Technical Details

**Current File**: `src/states/play_match/auras.rs` (934 lines)

**Systems to Extract**:
- `process_holy_shock_heals` (lines 706-778)
- `process_holy_shock_damage` (lines 780-913)
- Consider: `process_dispels` (lines 335-416)

## Acceptance Criteria

- [ ] Holy Shock processing in dedicated module
- [ ] auras.rs focused on aura lifecycle only
- [ ] Systems properly registered in play_match plugin
- [ ] Build succeeds
- [ ] Headless tests pass

## Work Log

| Date | Action | Notes |
|------|--------|-------|
| 2026-02-02 | Created | From architecture-strategist review |

## Resources

- Architecture review: flagged SRP violation
- Current file: `src/states/play_match/auras.rs`
