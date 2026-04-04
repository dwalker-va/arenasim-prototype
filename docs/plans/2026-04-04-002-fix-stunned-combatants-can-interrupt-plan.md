---
title: "fix: Prevent stunned combatants from using interrupt abilities"
type: fix
status: completed
date: 2026-04-04
---

# fix: Prevent stunned combatants from using interrupt abilities

## Overview

Stunned combatants (e.g., Rogue hit by Hammer of Justice) can still use interrupt abilities like Kick and Pummel. The `check_interrupts` system bypasses the centralized incapacitation gate in `decide_abilities`, allowing Warriors and Rogues to interrupt while stunned, feared, or polymorphed.

## Problem Frame

In the match log (`match_logs/match_1775288262.txt`), a Rogue uses Kick at 21.63s while under a Hammer of Justice stun applied at 20.15s (expires ~26.13s). This breaks a core WoW PvP mechanic: stun windows are supposed to be safe casting windows.

The root cause is **not** missing stun checks in class AI (the bug doc's hypothesis). The centralized AI gate in `decide_abilities` correctly prevents all ability decisions while incapacitated. The actual root cause is that `check_interrupts` is a **separate Bevy system** that independently evaluates interrupt opportunities without checking the interrupter's CC state.

## Requirements Trace

- R1. Stunned/feared/polymorphed/incapacitated combatants must not use interrupt abilities (Kick, Pummel)
- R2. The fix must use the existing `is_incapacitated()` helper for consistency with other CC checks
- R3. No behavioral change for non-incapacitated combatants

## Scope Boundaries

- Only fixing `check_interrupts` — the `decide_abilities` gate and auto-attack gate are already correct
- Not changing interrupt mechanics, cooldowns, or lockout behavior
- Not adding new CC types or modifying aura processing

## Context & Research

### Relevant Code and Patterns

- `src/states/play_match/combat_ai.rs:412` — `decide_abilities` correctly calls `is_incapacitated()` before dispatching to class AI
- `src/states/play_match/combat_ai.rs:883` — `check_interrupts` lacks any incapacitation check (the bug)
- `src/states/play_match/combat_core/auto_attack.rs:84-88` — `combat_auto_attack` correctly checks `is_incapacitated` (the pattern to follow)
- `src/states/play_match/utils.rs:85` — `is_incapacitated(auras: Option<&ActiveAuras>)` checks for Stun, Fear, Polymorph, Incapacitate
- `src/states/play_match/combat_ai.rs:891` — `check_interrupts` already queries `all_auras: Query<&ActiveAuras>` but only uses it for target Divine Shield checks (line 915)

### System Ordering Context

The systems are chained in Phase 2 (CombatAndMovement) in `systems.rs`:
```
decide_abilities → pet_ai_system → apply_deferred → check_interrupts → process_interrupts
```

`check_interrupts` runs after `decide_abilities` but is a fully independent system — it does not inherit the incapacitation gate from `decide_abilities`.

## Key Technical Decisions

- **Add incapacitation check inside the combatant loop, not as a new system**: The `all_auras` query is already available in `check_interrupts`. Adding a check for the interrupter's own auras matches the pattern used in `combat_auto_attack` (line 84-88). This is a 3-line fix, not an architectural change.

## Open Questions

### Resolved During Planning

- **Is the bug in class AI decide_action methods?** No. The centralized gate in `decide_abilities` already prevents all class AI from running while incapacitated. The bug is in the separate `check_interrupts` system.
- **Does `check_interrupts` have access to the interrupter's auras?** Yes — via `all_auras.get(entity)` where `entity` is the interrupter. The query already exists (line 891).

### Deferred to Implementation

- None — this is a straightforward fix.

## Implementation Units

- [ ] **Unit 1: Add incapacitation guard to check_interrupts**

**Goal:** Prevent stunned/feared/polymorphed/incapacitated combatants from using Kick or Pummel via the `check_interrupts` system.

**Requirements:** R1, R2, R3

**Dependencies:** None

**Files:**
- Modify: `src/states/play_match/combat_ai.rs` (~line 899, inside the `check_interrupts` combatant loop)

**Approach:**
- After the `is_alive()` check (line 900-902), add an incapacitation check using `all_auras.get(entity)` and `is_incapacitated()`
- Follow the exact pattern from `combat_auto_attack` (auto_attack.rs lines 84-88)

**Patterns to follow:**
- `src/states/play_match/combat_core/auto_attack.rs:84-88` — same guard pattern with `is_incapacitated()`

**Test scenarios:**
- Happy path: Rogue uses Kick to interrupt a cast when not stunned — interrupt succeeds as before
- Happy path: Warrior uses Pummel to interrupt a cast when not stunned — interrupt succeeds as before
- Edge case: Rogue under Stun aura attempts Kick — interrupt is blocked
- Edge case: Warrior under Fear aura attempts Pummel — interrupt is blocked
- Edge case: Combatant under Polymorph attempts interrupt — interrupt is blocked
- Edge case: Stun expires, combatant can interrupt on next opportunity — no permanent suppression

**Verification:**
- Run a Rogue vs Paladin headless match; search for Kick usage during stun windows — Kick should not appear between stun application and expiration timestamps
- Run a Warrior vs Mage headless match to confirm Pummel still works when not CC'd

- [ ] **Unit 2: Update bug documentation status**

**Goal:** Mark the bug as fixed.

**Requirements:** N/A

**Dependencies:** Unit 1

**Files:**
- Modify: `docs/bugs/2026-04-04-stunned-combatants-can-use-abilities.md`

**Approach:**
- Update frontmatter `status: open` → `status: fixed`
- Add a brief resolution note identifying the actual root cause (`check_interrupts` missing incapacitation guard, not class AI)

**Test scenarios:** N/A

**Verification:**
- Bug doc reflects fixed status and accurate root cause

## System-Wide Impact

- **Interaction graph:** Only `check_interrupts` is modified. No callbacks, middleware, or other systems are affected.
- **Error propagation:** N/A — this is a guard clause addition.
- **State lifecycle risks:** None — the check is stateless (reads aura state, skips if CC'd).
- **API surface parity:** The fix brings `check_interrupts` into parity with `decide_abilities` and `combat_auto_attack`, both of which already check incapacitation.
- **Unchanged invariants:** Interrupt cooldowns, lockout durations, range checks, stealth checks, and Divine Shield immunity checks are all untouched.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Edge case: aura expires same frame as interrupt check | System ordering guarantees `apply_pending_auras` runs in Phase 1 before `check_interrupts` in Phase 2 — aura state is always current |

## Sources & References

- Bug report: `docs/bugs/2026-04-04-stunned-combatants-can-use-abilities.md`
- Match log: `match_logs/match_1775288262.txt` (Rogue vs Paladin)
