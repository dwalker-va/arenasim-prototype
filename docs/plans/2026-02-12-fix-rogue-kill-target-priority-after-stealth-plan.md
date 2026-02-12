---
title: Fix Rogue Kill Target Priority After Stealth Break
type: fix
date: 2026-02-12
---

# Fix Rogue Kill Target Priority After Stealth Break

## Problem Statement

When a Rogue is configured as the kill target but starts in stealth, the opposing team correctly falls back to the nearest visible enemy. However, when the Rogue breaks stealth (e.g., Ambush opener), the team never switches back to the Rogue as their kill target. They remain locked onto whatever fallback target they acquired.

**Root cause:** In `acquire_targets()` (`combat_ai.rs:107-143`), the `target_valid` check only verifies the current target is alive, visible, and not immune. If it passes, the entire re-acquisition block is skipped — there is no check for whether a higher-priority configured kill target has become available.

```
Frame 1 (Rogue stealthed):
  target_valid = false → try kill target (Rogue) → can't see → fallback to Mage
  combatant.target = Mage

Frame N (Rogue breaks stealth):
  target_valid = true (Mage is alive/visible) → SKIP re-acquisition
  combatant.target = Mage  ← BUG: should switch to Rogue
```

## Proposed Solution

After the `target_valid` check passes, add a **kill target priority override**: if the configured kill target is now visible, not immune, and is NOT the current target, switch to it.

### `src/states/play_match/combat_ai.rs` (~line 113)

```rust
// If no valid target, acquire a new one
if !target_valid {
    // ... existing acquisition logic ...
} else if let Some(index) = kill_target_index {
    // Current target is valid, but check if configured kill target
    // has become available (e.g., Rogue broke stealth) and should take priority
    if let Some((kt_entity, _, stealthed, enemy_ss, _, _, immune)) = enemy_combatants.get(index) {
        if can_see(*stealthed, *enemy_ss) && !immune && combatant.target != Some(*kt_entity) {
            combatant.target = Some(*kt_entity);
        }
    }
}
```

This is 7 lines of code. The same pattern should be applied to CC target acquisition (~line 156).

## Acceptance Criteria

- [x] When Rogue is kill target and breaks stealth, team switches to Rogue
- [x] When Rogue is stealthed, team correctly targets visible enemies
- [x] When kill target becomes immune (Divine Shield), team still switches away (existing behavior preserved)
- [x] When kill target becomes visible again after immunity, team switches back
- [x] CC target follows same priority override pattern
- [x] Headless simulation confirms target switch on Rogue stealth break

## Test Configs

```json
// Rogue as kill target, starts stealthed
{"team1":["Warrior","Priest"],"team2":["Rogue","Mage"],"team1_kill_target":0}

// Rogue NOT as kill target (should NOT switch to Rogue on stealth break)
{"team1":["Warrior","Priest"],"team2":["Rogue","Mage"],"team1_kill_target":1}
```

## References

- Target acquisition: `src/states/play_match/combat_ai.rs:80-185`
- Stealth/visibility: `src/states/play_match/combat_ai.rs:98-104`
- Immunity filtering (recent PR #4): `src/states/play_match/combat_ai.rs:107-112`
- Rogue stealth break on opener: `src/states/play_match/class_ai/rogue.rs:65-75`
