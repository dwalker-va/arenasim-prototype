---
status: pending
priority: p3
issue_id: "011"
tags: [code-review, documentation]
dependencies: []
---

# Stale doc comments listing incomplete class sets

## Problem Statement

Several doc comments enumerate class names but are incomplete or outdated.

## Findings

- `constants.rs:21`: WAND_RANGE comment says "(Mage, Priest)" — missing Warlock
- `components/mod.rs:425`: `class` field comment says "(Warrior, Mage, Rogue, Priest)" — missing Warlock, Paladin
- `combat_ai.rs:215`: CC heuristic doc says "(Priest)" — missing Paladin (code was fixed but comment not)

## Proposed Solutions

### Option A: Update all stale comments

- **Effort:** Small (3 lines)
- **Risk:** Low

## Acceptance Criteria

- [ ] All class-enumerating comments are complete or removed
