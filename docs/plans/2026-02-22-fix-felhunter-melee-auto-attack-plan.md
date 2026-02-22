---
title: "Fix Felhunter Melee Auto Attack"
type: fix
date: 2026-02-22
---

# Fix: Felhunter Melee Auto Attack

Felhunter pets currently auto attack at `WAND_RANGE` (30.0 units) and log attacks as "Wand Shot". In WoW Classic, Felhunter is a melee pet — it should attack at melee range and log attacks as "Auto Attack".

## Enhancement Summary

**Deepened on:** 2026-02-22
**Sections enhanced:** 3 (Implementation, Architecture, Testing)
**Review agents used:** pattern-recognition-specialist, performance-oracle, code-simplicity-reviewer, architecture-strategist

### Key Improvements
1. Fixed cross-loop variable scoping — `is_melee` must be baked into `combatant_info` snapshot since range check and attack name are in separate loops
2. Added warning doc comment on `Combatant::in_attack_range()` to prevent future misuse
3. Clarified architectural scaling path for future pet types

## Root Cause

Two functions use `CharacterClass::is_melee()` to determine attack behavior. Since the Felhunter inherits `CharacterClass::Warlock` from its owner, and Warlock returns `false` for `is_melee()`, the pet gets ranged behavior:

1. **`Combatant::in_attack_range()`** (`components/mod.rs:725`) — Uses `self.class.is_melee()` to pick between `MELEE_RANGE` (2.5) and `WAND_RANGE` (30.0). No pet awareness.
2. **Attack name in combat log** (`combat_core.rs:824`) — Uses `attacker_class.is_melee()` to label the attack. Felhunter attacks show as "Wand Shot" instead of "Auto Attack".

Note: The movement system (`combat_core.rs:548`) already handles this correctly via `pet.pet_type.preferred_range()`, returning 2.0 for Felhunter. The fix should follow this same pattern.

## Acceptance Criteria

- [x] Felhunter auto attacks use `MELEE_RANGE` (not `WAND_RANGE`)
- [x] Felhunter auto attacks are logged as "Auto Attack" (not "Wand Shot")
- [x] Future pet types can specify melee vs ranged via `PetType`
- [x] No change to player combatant auto attack behavior
- [x] Headless simulation confirms fix (grep match log for Felhunter attacks)

## Implementation

### 1. Add `is_melee()` to `PetType` (`components/mod.rs`)

```rust
// src/states/play_match/components/mod.rs — PetType impl, place after preferred_range()
pub fn is_melee(&self) -> bool {
    match self {
        PetType::Felhunter => true,
    }
}
```

This mirrors the existing `preferred_range()` pattern and `CharacterClass::is_melee()` naming. Future pet types (Imp = ranged, Succubus = melee) simply add match arms.

### 2. Bake `is_melee` into `combatant_info` snapshot (`combat_core.rs`)

**Critical detail from review:** The range check (~line 735) and attack name (~line 824) are in *separate loops*. The range check is in the mutable per-attacker loop, while the attack name is in the damage application loop. A variable from the first loop is not available in the second.

**Solution:** Extend the `combatant_info` snapshot (built at ~line 678 in an immutable iter) to include an `is_melee` boolean, computed pet-aware at snapshot time. This follows the existing pattern where `display_name` is already customized for pets in the snapshot.

```rust
// combat_core.rs — where combatant_info snapshot is built (~line 678)
// Add is_melee to the snapshot tuple
let is_melee = if let Ok(pet) = auto_attack_pet_query.get(entity) {
    pet.pet_type.is_melee()
} else {
    combatant.class.is_melee()
};
// Include is_melee in the snapshot tuple
```

### 3. Fix auto attack range check (`combat_core.rs`, ~line 735)

In the per-attacker loop, use the snapshot's `is_melee` to determine range:

```rust
// combat_core.rs — range check in the per-attacker loop
let attack_range = if *is_melee { MELEE_RANGE } else { WAND_RANGE };
let in_range = my_pos.distance(target_pos) <= attack_range;
if in_range {
```

This replaces the call to `combatant.in_attack_range(my_pos, target_pos)` for the auto-attack path, matching the `if let Ok(pet)` idiom used in `move_to_target`.

### 4. Fix attack name in combat log (`combat_core.rs`, ~line 824)

```rust
// combat_core.rs — attack name resolution in the damage loop
let attack_name = if has_bonus {
    "Heroic Strike"
} else if *is_melee {
    "Auto Attack"
} else {
    "Wand Shot"
};
```

Since `is_melee` is now in the snapshot, it's available in both loops without a second pet query.

### 5. Add warning doc comment on `in_attack_range()` (`components/mod.rs`)

```rust
/// Check if this combatant is in range to auto-attack the target.
///
/// WARNING: Uses `self.class.is_melee()` which does NOT account for pet entities.
/// For pet-aware range checks, use the `is_melee` flag from the `combatant_info`
/// snapshot in `combat_auto_attack` instead.
pub fn in_attack_range(&self, my_position: Vec3, target_position: Vec3) -> bool {
```

### 6. Test with headless simulation

```bash
echo '{"team1":["Warlock"],"team2":["Mage"]}' > /tmp/test.json
cargo run --release -- --headless /tmp/test.json
# Verify: Felhunter attacks should say "Auto Attack", not "Wand Shot"
grep -i "felhunter" match_logs/$(ls -t match_logs | head -1)
```

Also test Warlock auto-attacks are still "Wand Shot":
```bash
grep -i "warlock.*wand shot\|warlock.*auto attack" match_logs/$(ls -t match_logs | head -1)
```

## Files to Change

| File | Change |
|------|--------|
| `src/states/play_match/components/mod.rs` | Add `PetType::is_melee()`, add warning doc on `in_attack_range()` |
| `src/states/play_match/combat_core.rs` | Bake `is_melee` into snapshot, fix range check + attack name |

## Review Insights

### Pattern Consistency (pattern-recognition-specialist)
- The `if let Ok(pet) = pet_query.get(entity)` idiom appears 13+ times across `combat_core.rs` and `auras.rs` — the proposed approach is consistent
- `auto_attack_pet_query: Query<&Pet>` already exists in the function signature (line 654) — no new query needed
- `PetType::is_melee()` naming mirrors `CharacterClass::is_melee()` correctly

### Performance (performance-oracle)
- Zero concern. `query.get()` is O(1) in Bevy's archetypal storage. With 6-12 entities, this adds nanoseconds per frame.
- Baking into the snapshot avoids even the second O(1) lookup in the damage loop.

### Simplicity (code-simplicity-reviewer)
- Alternative considered: derive melee status from `preferred_range() <= MELEE_RANGE` to avoid adding `is_melee()` entirely. This is a valid single-file-change approach but `is_melee()` is more readable and mirrors the existing `CharacterClass` API. Both are acceptable.
- The fix is 2 files, ~10 net new lines. Near-minimal.

### Architecture (architecture-strategist)
- Do NOT extract a shared "check pet then class" helper yet — Rule of Three (only 2 behavioral dispatches exist after this fix)
- When 3+ pet types arrive, consider a `CombatBehavior` component computed at spawn time to eliminate the pattern entirely
- No dual-registration concern — this fix only modifies `combat_core.rs` which is already registered in both headless and graphical modes

## Context

- Brainstorm: `docs/brainstorms/2026-02-15-warlock-pet-system-brainstorm.md` (decision #8)
- Movement system already correct: `combat_core.rs:548` uses `pet.pet_type.preferred_range()`
- Constants: `MELEE_RANGE = 2.5`, `WAND_RANGE = 30.0` in `constants.rs`
- Learning: `docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md` — no new systems added, so dual-registration is not a concern here
