---
status: pending
priority: p2
issue_id: "013"
tags: [code-review, refactor, duplication]
dependencies: []
---

# Extract Incapacitation Check Into Helper Function

## Problem Statement

The CC incapacitation check (`Stun | Fear | Polymorph`) is duplicated 5 times across the codebase. If a new CC type is added, all 5 sites must be updated.

## Findings

Sites:
1. `combat_core.rs:705` — combat_auto_attack (pre-existing)
2. `combat_core.rs:1239` — process_casting (new)
3. `combat_core.rs:1866` — process_channeling (new)
4. `combat_ai.rs:395` — decide_abilities (pre-existing)
5. `class_ai/pet_ai.rs:85` — pet_ai_system (pre-existing)

Plus `CombatContext::is_incapacitated()` in `class_ai/mod.rs:155` which operates on a different data shape.

## Proposed Solution

Extract a standalone utility:

```rust
// In utils.rs
pub fn is_incapacitated_by_auras(auras: Option<&ActiveAuras>) -> bool {
    auras.map_or(false, |a| {
        a.auras.iter().any(|aura| matches!(
            aura.effect_type,
            AuraType::Stun | AuraType::Fear | AuraType::Polymorph
        ))
    })
}
```

## Acceptance Criteria

- [ ] Single source of truth for incapacitation CC types
- [ ] All 5 inline sites use the helper
- [ ] CombatContext::is_incapacitated delegates to the same list

## Work Log

- 2026-02-20: Found during code review
