---
status: pending
priority: p3
issue_id: "008"
tags: [code-review, documentation, paladin]
dependencies: []
---

# Excessive Docstring on Devotion Aura Function

## Problem Statement

The `try_devotion_aura()` function has a 27-line design note explaining a gameplay decision. This level of documentation belongs in design docs, not inline code comments. It creates maintenance burden and distracts from the code.

## Findings

**Location**: `src/states/play_match/class_ai/paladin.rs:652-678`

```rust
/// Try to cast Devotion Aura on an unbuffed ally.
///
/// **Design Note**: Unlike WoW Classic's toggle aura that instantly affects all party
/// members, this implementation applies the buff to one ally per GCD. This is an
/// intentional design choice for game balance - it creates a tactical window during
/// the pre-combat phase where the enemy team can engage before the Paladin's team
/// is fully buffed, adding strategic depth to match openings.
///
/// Similar to Priest's Power Word: Fortitude - buffs team pre-combat.
```

This is well-written documentation, but it's in the wrong place. The code behavior should be self-explanatory; design rationale belongs in `design-docs/`.

## Proposed Solutions

### Option A: Move to Design Doc (Recommended)
Move the design note to a design document, leave brief comment.

**Pros**: Appropriate location, code stays clean
**Cons**: Requires creating/updating design doc
**Effort**: Small (15 minutes)
**Risk**: Low

### Option B: Shorten Inline Comment
Condense to 2-3 lines.

**Pros**: Quick fix
**Cons**: Loses some context
**Effort**: Small (5 minutes)
**Risk**: Low

```rust
/// Apply Devotion Aura to allies. Buffs one ally per GCD (design choice for balance).
```

## Recommended Action

**Option A** - Move to design doc. The existing `docs/solutions/implementation-patterns/adding-new-class-paladin.md` is a natural home for this.

## Technical Details

**Affected Files**:
- `src/states/play_match/class_ai/paladin.rs` (shorten comment)
- `docs/solutions/implementation-patterns/adding-new-class-paladin.md` (add design note)

## Acceptance Criteria

- [ ] Devotion Aura function has concise docstring (3 lines max)
- [ ] Design rationale documented in appropriate design doc
- [ ] No information lost

## Work Log

| Date | Action | Notes |
|------|--------|-------|
| 2026-02-02 | Created | From simplicity review |

## Resources

- Simplicity review findings
- Target doc: `docs/solutions/implementation-patterns/adding-new-class-paladin.md`
