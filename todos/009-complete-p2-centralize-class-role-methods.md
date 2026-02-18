---
status: pending
priority: p2
issue_id: "009"
tags: [code-review, architecture, refactor]
dependencies: []
---

# Centralize class role predicates on CharacterClass

## Problem Statement

Class role categorizations (melee vs ranged, healer vs DPS, mana-user) are scattered across 6+ inline `matches!()` calls in 4 files. The Paladin melee bug was caused by this pattern — Paladin was independently categorized in each location and was wrong in 4 of them.

## Findings

- `is_melee` concept appears in 2 sites: `in_attack_range()`, `preferred_range()`
- `is_healer` concept appears in 3 sites: `combat_ai.rs` (×2), `paladin.rs`
- `uses_mana` concept appears in 1 site: `mage.rs`
- All three reviewers independently recommended centralizing these

## Proposed Solutions

### Option A: Add methods to CharacterClass (Recommended)

Add `is_melee()`, `is_healer()`, `uses_mana()` to `impl CharacterClass` in `match_config.rs`.

- **Pros:** Single source of truth, self-documenting, follows existing pattern
- **Cons:** None significant
- **Effort:** Small (under 20 lines)
- **Risk:** Low

## Technical Details

- **Affected files:** `src/states/match_config.rs`, `src/states/play_match/components/mod.rs`, `src/states/play_match/combat_ai.rs`, `src/states/play_match/class_ai/mage.rs`

## Acceptance Criteria

- [ ] `CharacterClass::is_melee()` method exists and is used by `in_attack_range()`
- [ ] `CharacterClass::is_healer()` method exists and is used by CC heuristic
- [ ] `CharacterClass::uses_mana()` method exists and is used by Arcane Intellect
- [ ] All inline `matches!` on class roles replaced with method calls
