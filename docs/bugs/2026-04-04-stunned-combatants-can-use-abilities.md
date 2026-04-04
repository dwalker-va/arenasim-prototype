---
date: 2026-04-04
type: bug
severity: high
status: fixed
---

# Bug: Stunned Combatants Can Use Abilities

## Summary

Combatants under Stun CC effects (e.g., Hammer of Justice) can still use instant-cast abilities like Kick. Stun should prevent all actions: movement, casting, auto-attacks, and abilities.

## Resolution

**Root cause:** The `check_interrupts` system in `combat_ai.rs` was a separate Bevy system from `decide_abilities` and did not check for incapacitation before allowing Warriors and Rogues to use interrupt abilities (Kick, Pummel). The centralized AI gate in `decide_abilities` correctly blocked all other abilities during stun, but `check_interrupts` bypassed it entirely.

**Fix:** Added an `is_incapacitated()` guard at the top of the `check_interrupts` combatant loop, matching the existing pattern used in `combat_auto_attack` and `decide_abilities`.

## Reproduction

**Match log:** `match_logs/match_1775288262.txt` (Rogue vs Paladin)

**Timeline:**
```
[20.13s] Paladin casts Hammer of Justice on Rogue
[20.15s] Hammer of Justice stun on Rogue (6.0s, DR: 100%)
[21.63s] Rogue uses Kick — interrupts Paladin's Flash of Light  ← BUG: Rogue is stunned
```

The stun starts at 20.13s and lasts 6.0s (expires ~26.13s). The Rogue uses Kick at 21.63s — 1.5 seconds into the stun — which should be impossible.

## Expected Behavior

A stunned combatant cannot use any abilities (instant or cast-time) until the stun expires. Kick at 21.63s should not happen; the Rogue should remain unable to act until ~26.13s.

## Likely Cause

The class AI `decide_action()` methods for each class may not be checking for active Stun auras before queuing instant-cast abilities like Kick. The casting system likely blocks cast-time spells during stun, but instant abilities queued through the AI decision layer may bypass that check.

## Where to Look

- `src/states/play_match/class_ai/rogue.rs` — `decide_rogue_action()` for Kick usage
- `src/states/play_match/class_ai/mod.rs` — check if there's a shared stun guard before AI decisions
- `src/states/play_match/combat_ai.rs` — the main AI loop that calls class-specific decision functions
- Check whether other classes have the same issue (Warrior's Pummel, Warlock's Spell Lock via pet)

## Impact

High — stun is a core CC mechanic. If stunned combatants can still interrupt, it undermines the counter-play of casting during stun windows, which is a fundamental WoW PvP strategy (stun the Rogue, then heal).
