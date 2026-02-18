---
status: pending
priority: p2
issue_id: "010"
tags: [code-review, bug, cosmetic]
dependencies: []
---

# Warlock auto attack labeled "Auto Attack" instead of "Wand Shot"

## Problem Statement

Warlock uses `WAND_RANGE` (30yd) in `in_attack_range()` but its auto attack log label falls through to `_ => "Auto Attack"` instead of "Wand Shot". Pre-existing bug, not introduced by the Paladin fix.

## Findings

- `combat_core.rs:832-835`: Only Mage and Priest are in the "Wand Shot" branch
- Warlock should be there too since it uses wand range

## Proposed Solutions

### Option A: Add Warlock to Wand Shot match arm

- **Effort:** Small (1 line)
- **Risk:** Low

## Technical Details

- **Affected files:** `src/states/play_match/combat_core.rs`

## Acceptance Criteria

- [ ] Warlock auto attacks labeled "Wand Shot" in combat log
