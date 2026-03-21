---
title: "fix: P0 pet/owner entity confusion and target index corruption"
type: fix
status: completed
date: 2026-03-20
---

# Fix P0 Pet/Owner Entity Confusion and Target Index Corruption

## Enhancement Summary

**Deepened on:** 2026-03-20
**Research agents used:** pattern-recognition, architecture-strategist, performance-oracle, code-simplicity, bevy-codebase-explorer, learnings-researcher (x2)

### Key Improvements
1. Confirmed point-fix approach over CombatContext pre-filter (pre-filter breaks `target_info()` when combatant's target is a pet)
2. `lowest_health_ally()` removed from explicit change list — inherits fix from `alive_allies()`
3. Added doc comment requirement on `CombatContext` to prevent future regressions
4. Clarified Part A filtering must preserve entity-index sort order

### Architectural Decision: Why Point-Fix Over Pre-Filter
A CombatContext pre-filter (splitting into `combatants` and `all_combatants`) was evaluated and rejected. If `ctx.combatants` excluded pets, then `target_info()` would return `None` when a combatant's auto-attack target is a pet — causing the AI to think it has no target. The single-map approach with opt-in filtering at the helper method level is correct for this codebase.

---

## Overview

Two P0 bugs from the 2026-03-16 bug hunt share a root cause: pet entities (Felhunter, spider, boar, bird) are included in combatant queries where only primary combatants should appear. This causes kill/CC target indices to misalign (BUG-1) and buffs/stuns to land on pets instead of their owners (BUG-2/3).

## Problem Statement

### BUG-1: Target Index Corruption in 3v3 with Pets

`acquire_targets()` in `combat_ai.rs` (lines 58-80) builds `team1_combatants`/`team2_combatants` vectors that include pet entities, sorted by entity index. Kill/CC target indices from `MatchConfig` are 0-based slot indices (0, 1, 2 for 3v3), but pets inserted between owners shift the indices:

```
Spawn order: Warrior (idx 0), Warlock (idx 1), Felhunter (idx 2), Mage (idx 3)
Config kill_target=2 → hits Felhunter instead of Mage
```

This explains the "team composition corruption" observed in matches m17-m19 — classes aren't actually swapped, the wrong entities are being targeted and logged.

### BUG-2+3: Pet/Owner Entity Confusion in AI Targeting

Multiple class AI functions iterate `ctx.combatants` without `!info.is_pet` filtering:
- **HoJ stun** targets Felhunter instead of Warlock (paladin.rs:598)
- **PW:Fortitude** buffs Felhunter instead of Warlock (priest.rs:158)
- **Devotion Aura** buffs pets and uses pet for "already buffed" check (paladin.rs:742)
- **Emergency heal trigger** fires on low-HP pets (paladin.rs:327)
- **Dispel ally** wastes GCDs dispelling pet debuffs (class_ai/mod.rs:278)
- **lowest_health_ally()** returns pets (pets have 45% owner HP, so always lowest)

The codebase already has correct `!info.is_pet` filtering in several places (`lowest_health_ally_below()`, `select_cc_target_heuristic()`, `try_shield()`, hunter/warlock targeting) — it's just applied inconsistently.

## Proposed Solution

### Part A: Fix `acquire_targets()` Index Lookups (BUG-1)

Build a separate pet-filtered list for config index lookups while keeping the full list for nearest-enemy fallback targeting. This preserves the ability to auto-attack pets when all primary enemies are dead/stealthed.

**File:** `src/states/play_match/combat_ai.rs`

1. After building `team1_combatants`/`team2_combatants` (line ~80), create pet-filtered versions for index lookups. The filter is applied **after** the entity-index sort, which is correct because primary combatants are spawned in slot order and pets are spawned after their owners — filtering preserves the relative order of primary combatants:
   ```rust
   let team1_primary: Vec<_> = team1_combatants.iter()
       .filter(|(_, info)| !info.is_pet).collect();
   let team2_primary: Vec<_> = team2_combatants.iter()
       .filter(|(_, info)| !info.is_pet).collect();
   ```
2. Use `*_primary` lists for `kill_target_index` lookups (lines ~119-126, ~159-163)
3. Use `*_primary` lists for `cc_target_index` lookups (lines ~179-186, ~201-208)
4. Keep the original lists for nearest-enemy fallback logic (lines ~140-154)

### Part B: Add `!info.is_pet` Filters to AI Functions (BUG-2/3)

Apply the existing pattern from `lowest_health_ally_below()` to all affected locations:

| Location | File | What to filter |
|---|---|---|
| `alive_enemies()` | `class_ai/mod.rs:184` | Exclude pets from enemy list |
| `alive_allies()` | `class_ai/mod.rs:193` | Exclude pets from ally list |
| `try_fortitude()` | `priest.rs:158` | Skip pet allies |
| `try_devotion_aura()` | `paladin.rs:742` | Skip pets in both "already buffed" check AND buff application |
| `has_emergency_target()` | `paladin.rs:327` | Don't trigger emergency mode for low-HP pets |
| `try_hammer_of_justice()` | `paladin.rs:598` | Don't waste 60s CD stun on pets |
| `try_dispel_ally()` | `class_ai/mod.rs:278` | Don't waste dispels on pets (Felhunter handles its own) |

**Note:** `lowest_health_ally()` does not need an explicit change — it calls `alive_allies()` which will now exclude pets. Similarly, any other function that goes through `alive_enemies()`/`alive_allies()` inherits the fix automatically.

### Part C: Add Doc Comment to CombatContext (regression prevention)

Add a documentation comment to the `CombatContext` struct (class_ai/mod.rs:111) to make the pet-filtering convention visible at the point of use:

```rust
/// Shared context for AI decision making.
///
/// The `combatants` map contains ALL entities including pets.
/// Use `alive_enemies()` / `alive_allies()` for primary-combatant-only queries.
/// When iterating `combatants` directly, filter with `!info.is_pet`
/// unless the ability should affect pets (e.g., AoE damage, auto-attacks).
```

### What NOT to Filter

Pets should remain valid targets for:
- **Auto-attacks** — players can attack pets in WoW
- **AoE damage** (Frost Nova, etc.) — AoE hits everything in range
- **Trap triggers** — proximity-based, hits pets correctly
- **Pet AI queries** — pet AI uses its own `Without<Pet>` query (pet_ai.rs:33), completely separate
- **Offensive Holy Shock** (`try_holy_shock_damage()`, paladin.rs:525) — damaging pets is valid gameplay
- **`target_info()` / `self_info()` entity lookups** — these must see all entities including pets, since a combatant's auto-attack target may be a pet

## Acceptance Criteria

- [x] **BUG-1**: Config `kill_target=2` in a 3v3 with Warlock targets the 3rd player, not the pet
- [x] **BUG-2**: HoJ targets the Warlock, not the Felhunter
- [x] **BUG-3**: PW:Fortitude buffs the Warlock, not the Felhunter
- [x] Pet AI still functions correctly (Felhunter interrupts/dispels, spider roots, boar charges)
- [x] Auto-attacks can still target pets as fallback when all primary enemies are dead/stealthed
- [x] No regressions in 2v2 matches (no pets = no behavioral change)
- [x] Doc comment added to `CombatContext` struct

## Verification

Run headless matches with the reproduction configs from the bug report:

```bash
# BUG-1: Team composition corruption
echo '{"team1":["Priest","Paladin","Mage"],"team2":["Warrior","Rogue","Warlock"],"random_seed":6018}' > /tmp/bug1.json
cargo run --release -- --headless /tmp/bug1.json

# BUG-2+3: Pet/owner confusion
echo '{"team1":["Warlock","Priest"],"team2":["Mage","Paladin"],"random_seed":6004}' > /tmp/bug2.json
cargo run --release -- --headless /tmp/bug2.json

# Pet AI regression check
echo '{"team1":["Warlock","Priest","Paladin"],"team2":["Warrior","Mage","Rogue"],"random_seed":6017}' > /tmp/pet_check.json
cargo run --release -- --headless /tmp/pet_check.json

# Hunter pet regression check
echo '{"team1":["Hunter","Priest"],"team2":["Warrior","Mage"],"random_seed":6020}' > /tmp/hunter_check.json
cargo run --release -- --headless /tmp/hunter_check.json
```

Verify:
- No "Felhunter" in stun/buff log lines (BUG-2/3)
- Correct class names in kill target selection (BUG-1)
- Felhunter still appears in Spell Lock/Devour Magic log lines (pet AI working)
- Hunter pets still function (charge, root, cleanse depending on pet type)

## Files to Modify

1. `src/states/play_match/combat_ai.rs` — `acquire_targets()` index lookups
2. `src/states/play_match/class_ai/mod.rs` — `alive_enemies()`, `alive_allies()`, `try_dispel_ally()`, `CombatContext` doc comment
3. `src/states/play_match/class_ai/paladin.rs` — `has_emergency_target()`, `try_hammer_of_justice()`, `try_devotion_aura()`
4. `src/states/play_match/class_ai/priest.rs` — `try_fortitude()`

## Sources

- Bug report: `docs/reports/2026-03-16-headless-match-bug-report.md`
- Correct filtering pattern: `lowest_health_ally_below()` in `class_ai/mod.rs:209-223`
- Pet AI isolation: `pet_ai.rs:33` (`Without<Pet>` query)
- Match end pet exclusion: `match_flow.rs:156` (`Without<Pet>` in `check_match_end`)
