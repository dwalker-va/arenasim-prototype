---
title: "fix: Paladin auto attack should be melee, not ranged"
type: fix
date: 2026-02-17
---

# fix: Paladin auto attack should be melee, not ranged

The Paladin is incorrectly grouped with ranged classes (Mage, Priest, Warlock) for auto attack range checks, causing it to auto attack at wand range (30 yards) instead of melee range (2.5 yards). The Paladin is described as a "healer/melee" class and has melee-like attack stats (attack_power: 20.0), confirming this is a bug.

## Enhancement Summary

**Deepened on:** 2026-02-17
**Sections enhanced:** 3
**Research agents used:** pattern-recognition-specialist, code-simplicity-reviewer, architecture-strategist

### Key Improvements
1. Discovered 2 additional related Paladin misclassification bugs (CC healer heuristic + Arcane Intellect targeting)
2. Confirmed `preferred_range: 2.0` is architecturally safe — all Paladin heals are 40yd range, independent of movement targeting
3. Verified fix is minimal (net zero LOC change)

## Root Cause

Two locations classify Paladin as a ranged class:

1. **`in_attack_range()`** in `src/states/play_match/components/mod.rs:623` — Paladin is in the `WAND_RANGE` (30.0) branch alongside Mage/Priest/Warlock instead of the `MELEE_RANGE` (2.5) branch with Warrior/Rogue.

2. **`preferred_range()`** in `src/states/match_config.rs:140` — Paladin returns `28.0` (same as Priest), meaning the movement system positions the Paladin at 28 yards from its target, far too distant for melee auto attacks.

## Acceptance Criteria

- [x] Paladin uses `MELEE_RANGE` (2.5) for auto attack range check in `in_attack_range()`
- [x] Paladin `preferred_range()` returns `2.0` (same as Warrior/Rogue)
- [x] Doc comment on `in_attack_range()` no longer lists Paladin as a wand user
- [x] Comment on `preferred_range()` updated to reflect melee positioning rationale
- [x] CC healer heuristic in `combat_ai.rs` recognizes Paladin as a healer
- [x] Arcane Intellect in `mage.rs` targets Paladin as a mana user
- [x] Headless simulation confirms Paladin moves to melee range and auto attacks at close distance

## Changes Required

### 1. `src/states/play_match/components/mod.rs` (~line 619-634)

Move `CharacterClass::Paladin` from the ranged/wand branch to the melee branch in `in_attack_range()`:

```rust
// Before (bug):
/// Mages, Priests, Warlocks, and Paladins use wands (ranged), Warriors and Rogues use melee weapons.
match_config::CharacterClass::Mage
| match_config::CharacterClass::Priest
| match_config::CharacterClass::Warlock
| match_config::CharacterClass::Paladin => {
    distance <= WAND_RANGE
}

// After (fix):
/// Mages, Priests, and Warlocks use wands (ranged); Warriors, Rogues, and Paladins use melee weapons.
match_config::CharacterClass::Mage
| match_config::CharacterClass::Priest
| match_config::CharacterClass::Warlock => {
    distance <= WAND_RANGE
}
match_config::CharacterClass::Warrior
| match_config::CharacterClass::Rogue
| match_config::CharacterClass::Paladin => {
    distance <= MELEE_RANGE
}
```

### 2. `src/states/match_config.rs` (~line 137-140)

Change Paladin's `preferred_range()` from `28.0` to `2.0` and update comment:

```rust
// Before:
// Paladin: Healer that positions like Priest
// Healing range 40, but has melee utility (Hammer of Justice 10yd)
// Stay at ~28 for healing safety, move in for stuns
CharacterClass::Paladin => 28.0,

// After:
// Paladin: Holy warrior — melee positioning for auto-attacks + Hammer of Justice
// All heals are 40yd range, so melee positioning doesn't limit healing
CharacterClass::Paladin => 2.0,
```

### 3. `src/states/play_match/combat_ai.rs` (~line 234, 246) — CC healer heuristic

The CC target selection only considers `Priest` as a healer. Paladin is also a healer and should get the healer CC priority bonus.

```rust
// Before (line ~234):
.map(|(_, _, _, _, class, _, _, _)| *class == match_config::CharacterClass::Priest)

// After:
.map(|(_, _, _, _, class, _, _, _)| matches!(*class,
    match_config::CharacterClass::Priest | match_config::CharacterClass::Paladin))

// Before (line ~246):
let is_healer = *class == match_config::CharacterClass::Priest;

// After:
let is_healer = matches!(*class,
    match_config::CharacterClass::Priest | match_config::CharacterClass::Paladin);
```

### 4. `src/states/play_match/class_ai/mage.rs` (~line 233-236) — Arcane Intellect mana user check

Paladin uses Mana but is excluded from the Arcane Intellect buff target list.

```rust
// Before:
let uses_mana = matches!(
    info.class,
    CharacterClass::Mage | CharacterClass::Priest | CharacterClass::Warlock
);

// After:
let uses_mana = matches!(
    info.class,
    CharacterClass::Mage | CharacterClass::Priest | CharacterClass::Warlock | CharacterClass::Paladin
);
```

### 5. Verify via headless simulation

Run a Paladin vs Warrior match and confirm:
- Paladin moves into melee range
- Paladin auto attacks are labeled "Auto Attack" (already correct)
- Paladin deals auto attack damage at melee distance

Run a Mage+Paladin vs Warrior+Priest match and confirm:
- Mage buffs Paladin with Arcane Intellect
- Enemy AI properly prioritizes CC on Paladin healers

## Architecture Notes

**Why `preferred_range: 2.0` is safe for a healer:**
- `preferred_range()` controls distance from the **enemy** target, not from heal targets
- Healing range checks are performed independently against ally positions
- All Paladin heals have 40yd range, far exceeding arena dimensions (~20-30yd max ally distance)
- Paladin AI prioritizes healing over attacking (priorities 3/5/6 vs offensive at 8)
- Melee positioning enables Hammer of Justice (10yd) and auto attacks, which were dead code at 28yd

**Tradeoff:** Paladin at melee range is now exposed to Warrior Pummel and Rogue Kick interrupts on Holy cast-time heals. This is an intentional design tradeoff consistent with WoW Classic Paladin gameplay.

## References

- `in_attack_range()`: `src/states/play_match/components/mod.rs:619-634`
- `preferred_range()`: `src/states/match_config.rs:123-142`
- `combat_auto_attack()`: `src/states/play_match/combat_core.rs:645`
- CC healer heuristic: `src/states/play_match/combat_ai.rs:229-246`
- Arcane Intellect mana check: `src/states/play_match/class_ai/mage.rs:233-236`
- Constants: `MELEE_RANGE = 2.5`, `WAND_RANGE = 30.0` in `constants.rs`
