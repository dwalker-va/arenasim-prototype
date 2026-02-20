---
id: "009"
status: pending
priority: p2
title: "Refactor buff dedup to use aura refresh instead of AI-side tracking"
created: 2026-02-20
tags: [auras, buffs, wow-mechanics, refactor]
---

# Refactor Buff Dedup: Aura Refresh on Reapply

## Problem

Currently, duplicate buff prevention is handled at two layers:
1. **AI-side** (`decide_abilities`): `battle_shouted_this_frame` / `devotion_aura_this_frame` HashSets prevent multiple casters from creating duplicate AuraPendings
2. **Aura system** (`apply_pending_auras`): `applied_buffs` HashSet silently **drops** duplicate buff-type AuraPendings

Neither layer matches WoW behavior. In WoW, buffs are a property of the aura itself — most can't stack and are **refreshed** (duration reset) when reapplied. The caster doesn't need to know the buff is already there.

## Proposed Change

1. In `apply_pending_auras` (`auras.rs`), when `already_has_buff_existing` is true for a non-stacking buff, **refresh the existing aura's duration** instead of silently dropping the pending aura
2. Remove `battle_shouted_this_frame` / `devotion_aura_this_frame` tracking from `combat_ai.rs` and the class AI functions (revert the AI-side dedup added in the P2 fix commit)
3. Keep the AI-side `has_battle_shout` check in `warrior.rs:172` — the AI should still prefer not to waste a GCD on an active buff, but if two Warriors cast in the same frame, the aura system handles it gracefully via refresh

## Files to Change

- `src/states/play_match/auras.rs` — change `already_has_buff_existing` branch from skip to refresh
- `src/states/play_match/combat_ai.rs` — remove `battle_shouted_this_frame` / `devotion_aura_this_frame` declarations
- `src/states/play_match/class_ai/warrior.rs` — revert `battle_shouted_this_frame` parameter
- `src/states/play_match/class_ai/paladin.rs` — revert `devotion_aura_this_frame` parameter

## Testing Required

- Multiple Warriors on same team: Battle Shout should apply once, second cast refreshes duration
- Multiple Paladins on same team: Devotion Aura should apply once, second cast refreshes
- Buff duration should reset on refresh (not extend beyond max)
- No double stat bonuses (AttackPowerIncrease, DamageTakenReduction)
- Run 20+ headless matches across all team compositions to verify no regressions
- Verify with 3v3 compositions that had original BUG-4 (M8, M9, M12, M14, M15, M16, M17)

## Why Later

This is a behavioral change to the aura system that affects all buff types, not just Battle Shout and Devotion Aura. It needs thorough testing across all buff aura types (Absorb, MaxHealthIncrease, MaxManaIncrease, etc.) to ensure refresh semantics are correct for each.
