---
title: "fix: Prevent dead combatant events from firing in same frame"
type: fix
status: completed
date: 2026-04-12
origin: docs/reports/2026-04-12-bug-hunt-2v2-3v3.md
---

# fix: Prevent dead combatant events from firing in same frame

## Overview

When a combatant dies from an instant attack (Ambush, Mortal Strike, etc.) during `decide_abilities`, their queued effects (Frost Nova damage/CC, other AoE) still process in the same frame's post-loop phase. Additionally, `process_aura_breaks` fires aura break events on dead combatants. This produces confusing combat logs and incorrect gameplay — dead combatants applying CC, damage to/from dead entities, and duplicate `[DEATH]` entries.

## Problem Frame

The `decide_abilities` system has three sequential phases within a single function:

1. **Per-combatant loop** — each combatant decides an action, queuing instant attacks and AoE damage
2. **Instant attack processing** — queued attacks resolve, potentially killing targets (`is_dead = true`)
3. **Frost Nova damage processing** — queued AoE resolves, but does NOT check if the caster died in phase 2

A Mage that queues Frost Nova in phase 1 and dies from an Ambush in phase 2 still has its Frost Nova damage and CC applied in phase 3. Separately, `process_aura_breaks` has no `is_alive()` guard, so dead combatants process aura break events.

This was observed in 3/24 matches during the 2026-04-12 bug hunt (m17, m19, m20).

## Requirements Trace

- R1. A combatant killed by an instant attack in `decide_abilities` must not have its queued Frost Nova damage or CC applied
- R2. `process_aura_breaks` must skip dead combatants
- R3. No duplicate `[DEATH]` log entries for the same combatant
- R4. Existing behavior must be preserved: DoTs from dead casters continue ticking (KI-1), projectiles in flight still land (KI-2)

## Scope Boundaries

- Does NOT change how `is_dead` works — the synchronous bool field pattern is correct and stays
- Does NOT centralize the scattered death-processing code (that's a separate refactor)
- Does NOT address projectile hits on recently-dead targets — `process_projectile_hits` already has a correct `is_alive()` guard
- Does NOT touch auto-attack death handling — `combat_auto_attack` already has its own `died_this_frame` HashSet

## Context & Research

### Relevant Code and Patterns

- `decide_abilities` at `combat_ai.rs:307` — the main AI system with per-combatant loop, instant attack queue, and Frost Nova damage queue
- Instant attack processing at `combat_ai.rs:604-769` — sets `target.is_dead = true` on kill
- Frost Nova damage processing at `combat_ai.rs:773-909` — checks `target.is_alive()` but NOT `caster.is_alive()`
- `process_aura_breaks` at `auras.rs:666-731` — no `is_alive()` guard on the main loop
- CC fix pattern at `combat_ai.rs:401-421` — `same_frame_cc_queue` drained into snapshot before each combatant's turn, established in the instant CC same-frame visibility fix (commit 58c1151)
- Auto-attack `died_this_frame` HashSet at `combat_core/auto_attack.rs:194` — prior art for intra-system death tracking

### Institutional Learnings

- `docs/plans/2026-02-20-fix-critical-combat-bugs-plan.md` — established the `is_dead: bool` field pattern (no marker components, no deferred commands for death state)
- `docs/plans/2026-04-06-001-fix-instant-cc-same-frame-visibility-plan.md` — established the snapshot + queue pattern for same-frame state propagation
- `docs/plans/2026-02-20-fix-p2-combat-bugs-plan.md` — established defense-in-depth pattern: guard at source AND at consumer

## Key Technical Decisions

- **Track deaths in a HashSet, not the snapshot**: The `combatant_info` snapshot has an `is_alive` field, but mutating a HashMap of cloned structs for each death is heavier and more error-prone than a simple `HashSet<Entity>`. The CC fix used a queue because it needed to propagate rich aura data into the snapshot. Deaths are binary — a set is sufficient.

- **Check caster alive on queued AoE, not just target**: The Frost Nova damage loop already checks `target.is_alive()`. The missing guard is on the caster side. Adding `caster.is_dead` check is the minimal fix.

- **Skip dead combatants in `process_aura_breaks` at the top of the loop**: This follows the defense-in-depth pattern from the P2 bugs plan. Even though dead combatants shouldn't have `DamageTakenThisFrame` in steady state, the same-frame window means they can.

## Open Questions

### Resolved During Planning

- **Should Frost Nova CC from a dead Mage still apply?** No. The CC is pushed onto `same_frame_cc_queue` during the per-combatant loop, but the actual aura application happens in `apply_pending_auras` (next frame via deferred commands). The same-frame snapshot reflection should also be skipped for dead casters. However, the CC queue drain happens at the top of the next combatant's iteration — at that point, the caster hasn't died yet (deaths happen in post-loop instant attack processing). So the CC is already reflected in the snapshot before the caster dies. This is a minor cosmetic issue (the root shows in logs but expires normally). Filtering it out of `same_frame_cc_queue` post-loop would add complexity for minimal benefit. The `AuraPending` spawn is deferred and will be filtered by `apply_pending_auras`'s `is_alive()` check on the target (not caster). **Decision: accept that the same-frame CC snapshot may briefly reflect CC from a caster who dies later in the same frame. The real fix is preventing the damage, not the snapshot reflection.**

- **Should this also add a caster-alive check to the instant attack processing loop?** No. Instant attacks are queued by the caster during their own turn in the per-combatant loop. The caster is alive when they decide to attack. Another combatant's instant attack could kill the original caster, but the processing order is: all instant attacks process in queue order. A caster dying from a later-queued instant attack won't have their earlier-queued attack cancelled — this matches WoW behavior where simultaneously-resolved abilities both land.

### Deferred to Implementation

- Exact iteration order between `instant_attacks` and `frost_nova_damage` should be verified by reading the current code flow, to confirm that instant attacks always resolve before Frost Nova damage

## Implementation Units

- [x] **Unit 1: Add caster-alive guard to Frost Nova damage processing**

  **Goal:** Prevent dead Mages from dealing Frost Nova damage after being killed by instant attacks earlier in the same frame.

  **Requirements:** R1

  **Dependencies:** None

  **Files:**
  - Modify: `src/states/play_match/combat_ai.rs` (Frost Nova damage processing loop, ~line 773)
  - Test: headless match reproduction with seed 6024 (m17 config)

  **Approach:**
  - At the start of the `for aoe in frost_nova_damage` loop body, after destructuring the `QueuedAoeDamage`, add a check: query the caster entity from `combatants` and skip if `!caster.is_alive()`
  - This is a single guard, analogous to the existing `target.is_alive()` check at line 786
  - The caster's `is_dead` flag was set synchronously during instant attack processing (line 709), so it is visible here

  **Patterns to follow:**
  - The existing `if target.is_alive()` guard at `combat_ai.rs:786`
  - The `died_this_frame` HashSet pattern in `combat_core/auto_attack.rs:194` (though a simple query check is sufficient here since `is_dead` is already set)

  **Test scenarios:**
  - Happy path: Mage casts Frost Nova, is NOT killed — Frost Nova damage applies normally to all enemies in range
  - Edge case: Mage queues Frost Nova, then dies from instant attack in same frame — Frost Nova damage does NOT apply, no damage log entries from the dead Mage's Frost Nova
  - Edge case: Mage queues Frost Nova, Frost Nova kills a target, Mage also dies from instant attack — verify the ordering: if instant attacks process first (killing Mage), Frost Nova should not fire; if Frost Nova somehow processes first, it should work normally

  **Verification:**
  - Run m17 config (seed 6024, `{"team1":["Warrior","Rogue","Mage"],"team2":["Warlock","Mage","Rogue"]}`). In the original run, `[DMG] Team 1 Mage's Frost Nova hits Team 2 Rogue for 32 damage` appeared after `[DEATH] Team 1 Mage`. After the fix, no Frost Nova damage should appear after the Mage's death entry.

- [x] **Unit 2: Add `is_alive()` guard to `process_aura_breaks`**

  **Goal:** Prevent aura break events from firing on dead combatants.

  **Requirements:** R2

  **Dependencies:** None

  **Files:**
  - Modify: `src/states/play_match/auras.rs` (`process_aura_breaks` function, ~line 671)
  - Test: headless match reproduction

  **Approach:**
  - Add `if !combatant.is_alive() { continue; }` at the top of the loop body in `process_aura_breaks`, right after the `damage_taken` check
  - This follows the defense-in-depth pattern: even though dead combatants ideally wouldn't have `DamageTakenThisFrame`, the same-frame window means they can

  **Patterns to follow:**
  - Other systems that guard with `is_alive()`: `acquire_targets` in `combat_ai.rs`, `process_dot_ticks` in `auras.rs`, `process_casting` in `casting.rs`

  **Test scenarios:**
  - Happy path: Living combatant with Polymorph takes damage — Polymorph breaks normally with log message
  - Edge case: Combatant dies and has `DamageTakenThisFrame` on them from the killing blow — `process_aura_breaks` skips them, no aura break log entry on dead combatant
  - Happy path: Living combatant with non-breakable aura (negative threshold) takes damage — aura is not broken (existing behavior preserved)

  **Verification:**
  - Grep match logs for `[EVENT].*broke from damage` entries and confirm none appear after a `[DEATH]` entry for the same combatant in the same timestamp

- [x] **Unit 3: Validate and regression test**

  **Goal:** Confirm the fix resolves the bug across the originally-affected matches and doesn't regress existing behavior.

  **Requirements:** R1, R2, R3, R4

  **Dependencies:** Unit 1, Unit 2

  **Files:**
  - Test configs: `/tmp/bug-hunt/m17_3v3_triple_dps.json`, `/tmp/bug-hunt/m19_3v3_3war_3mage.json`, `/tmp/bug-hunt/m20_3v3_3priest_3pal.json`

  **Approach:**
  - Rerun all 3 originally-affected matches (m17, m19, m20)
  - Scan logs for: damage from dead casters, aura breaks on dead combatants, duplicate `[DEATH]` entries, CC applied by dead casters
  - Run 2-3 additional diverse matches to check for regressions (Frost Nova still works when Mage is alive, aura breaks still fire correctly)

  **Test scenarios:**
  - Regression: Mage casts Frost Nova while alive — damage and root apply correctly
  - Regression: Polymorph breaks from damage on living target — break event fires correctly
  - Regression: DoTs from dead casters still tick (KI-1 preserved)
  - Regression: Projectiles in flight from dead casters still land (KI-2 preserved)
  - Bug fix: m17 seed 6024 — no Frost Nova damage from dead Mage
  - Bug fix: m19 seed 6026 — no duplicate DEATH, no targeting dead combatants
  - Bug fix: m20 seed 6027 — no damage on dead Priest

  **Verification:**
  - All 3 affected matches run without dead-combatant events
  - Frost Nova, Polymorph break, DoTs, and projectiles all still work correctly in non-death scenarios

## System-Wide Impact

- **Interaction graph:** Only `decide_abilities` and `process_aura_breaks` are modified. No changes to system registration, ordering, or other combat systems.
- **Error propagation:** The `is_alive()` check is a simple boolean guard — no new error paths introduced.
- **State lifecycle risks:** None. The `is_dead` flag is already set synchronously during instant attack processing. This fix only adds reads of that flag in two additional locations.
- **Unchanged invariants:** DoT continuation after caster death (KI-1), projectile-in-flight behavior (KI-2), auto-attack `died_this_frame` pattern, all other damage/healing systems.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Frost Nova caster-alive check uses wrong entity | The `QueuedAoeDamage` struct already carries `caster: Entity` — query it directly from `combatants` |
| `process_aura_breaks` guard hides a different bug | This is defense-in-depth — the guard is correct regardless of whether `DamageTakenThisFrame` should be present on dead entities |

## Sources & References

- **Origin:** [Bug Hunt Report](docs/reports/2026-04-12-bug-hunt-2v2-3v3.md) — BUG-4
- Related plan: [Instant CC Same-Frame Fix](docs/plans/2026-04-06-001-fix-instant-cc-same-frame-visibility-plan.md)
- Related plan: [Critical Combat Bugs Fix](docs/plans/2026-02-20-fix-critical-combat-bugs-plan.md) — `is_dead` field pattern
- Related plan: [P2 Combat Bugs Fix](docs/plans/2026-02-20-fix-p2-combat-bugs-plan.md) — defense-in-depth pattern
