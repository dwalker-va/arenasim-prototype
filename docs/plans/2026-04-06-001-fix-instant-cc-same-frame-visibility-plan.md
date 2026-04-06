---
title: "fix: Apply instant CC auras synchronously to close same-frame visibility gap"
type: fix
status: active
date: 2026-04-06
---

# fix: Apply Instant CC Auras Synchronously to Close Same-Frame Visibility Gap

## Overview

Instant crowd-control openers like Cheap Shot, Hammer of Justice, and Kidney Shot currently queue their stun aura via `commands.spawn(AuraPending { ... })`, which is a deferred ECS spawn. The aura does not attach to the target until `apply_pending_auras` runs later in the schedule. As a result, other class AI systems that run in the same frame as the CC opener observe the target as un-stunned and may start a cast that should have been prevented. The CC eventually lands and `process_casting` cancels the in-progress cast, but the wasted decision burns the attacker's interrupt (e.g., a Rogue ends up using Kick on the same target it just Cheap Shot stunned).

This fix changes instant CC openers to write the aura directly into the target's `ActiveAuras` component during the same frame, so any AI deciding later in the same frame already sees the target as crowd-controlled.

## Problem Frame

Observed in a recent match: Rogue Cheap Shotted a Priest from stealth at frame T. The Priest's class AI (running later in the same system chain) decided to start casting Flash Heal because it did not yet see the stun aura. The Rogue's interrupt branch then queued a Kick to cancel the Flash Heal — the Rogue's "free" stun was effectively wasted because it had to spend its Kick on the same target. The behavior is correct in WoW: an instant CC should prevent the target from acting at all in the same tick.

The deferred-spawn pattern is appropriate for casted spells (where the cast can be interrupted before completion and the aura must wait for cast resolution) but is wrong for instant CC openers, which should land the moment the ability is used.

## Requirements Trace

- R1. After an instant CC ability is executed, the target's `ActiveAuras` reflects the new aura before any other class AI runs in the same frame
- R2. Other AI systems observing the target in the same frame see the target as crowd-controlled (e.g., `is_incapacitated` returns true)
- R3. Diminishing returns, CC immunity (charging/disengaging), Divine Shield purge interaction, and CC replacement semantics continue to behave identically to the existing `apply_pending_auras` path
- R4. Combat log, floating combat text, and DR tracker updates continue to fire as before — no observable change to logging or visuals
- R5. No regression in casted-spell aura application — those still flow through `AuraPending` and `apply_pending_auras`

## Scope Boundaries

- Only **instant** CC openers used by class AI are converted. Casted-spell aura application stays on the deferred path.
- Buff auras applied via class AI (Battle Shout, Power Word: Fortitude, etc.) are out of scope — same-frame visibility for those is not load-bearing.
- DoT application (Rend, Corruption) is out of scope — DoTs do not gate other AI decisions.
- No changes to the `AuraPending` struct or to `apply_pending_auras` itself.
- No new ability behavior, balance changes, or new CC types.

## Context & Research

### Relevant Code and Patterns

- `src/states/play_match/class_ai/rogue.rs` `try_cheap_shot` — concrete site of the observed bug, currently uses `commands.spawn(aura_pending)` for the stun
- `src/states/play_match/class_ai/rogue.rs` `try_kidney_shot` — same pattern, instant stun
- `src/states/play_match/class_ai/paladin.rs` Hammer of Justice site (`commands.spawn(AuraPending { ... })` around line 652) — same pattern, instant stun
- `src/states/play_match/auras.rs` `apply_pending_auras` — the canonical aura-application logic the new helper must mirror: charging/disengaging immunity check, Divine Shield (`DamageImmunity`) hostile-aura block, DR category lookup + immunity + multiplier, CC replacement semantics, "Immune" floating text, combat log and DR-aware logging
- `src/states/play_match/components/auras.rs` `ActiveAuras`, `Aura`, `DRTracker`, `DRCategory` — the data structures to operate on
- `src/states/play_match/class_ai/mod.rs` `CombatContext` — the per-frame snapshot is read by AIs that decide later in the same frame; the fix must update the *live* `ActiveAuras` component, not the snapshot (the snapshot is computed once per frame upstream and is intentionally read-only)

### Institutional Learnings

- Memory record 1261: a prior fix addressed "stunned combatants using abilities" by adding `is_incapacitated` checks in casting and class AI. That fix correctly handles stuns that landed in *prior* frames but does not address same-frame visibility because the aura is still spawned via `AuraPending` and not yet attached when the next AI runs.
- The recently merged dynamic stat auras refactor (`docs/plans/2026-04-05-004-...`) demonstrated that several class AI sites already mutate the combatant directly (mana, GCD, stealth) — the codebase is comfortable with synchronous mutation in instant-attack paths. A synchronous CC application is a small extension of that pattern.

## Key Technical Decisions

- **Introduce a single shared helper, not per-call inline mutation.** `apply_pending_auras` performs ~10 distinct steps (immunity checks, DR, CC replacement, logging, FCT spawning). Inlining all of that into every CC opener would duplicate logic and drift over time. A shared helper called from each CC opener keeps behavior parity with the existing path.
  - **Rejected alternative:** keep `AuraPending` and reorder systems so `apply_pending_auras` runs before all class AI. Risky — class AI for the *acting* combatant runs before `apply_pending_auras` regardless of ordering, so this only fixes downstream observers. It also cascades into the whole schedule and could break other invariants.
  - **Rejected alternative:** make every class AI scan `Query<&AuraPending>` for incoming CC. Easy to forget for new abilities, and a hot-path query that wouldn't normally exist.

- **The helper lives in `auras.rs` next to `apply_pending_auras`** so the two paths stay close and any future change to CC application is visible to both.

- **The helper takes the same data the deferred path receives**, plus mutable references to the target's `ActiveAuras` and `DRTracker`. The class AI sites already have or can obtain these via existing queries — they currently spawn `AuraPending` because it was the path of least resistance, not because direct access is impossible.

- **The helper handles the "no `ActiveAuras` component yet" case** by inserting one via `commands.entity(target).insert(...)`. This still has a one-frame visibility gap for that specific edge case (the inserted component isn't visible until the next system runs `ApplyDeferred`), but in practice every combatant receives at least one buff aura during the pre-match countdown, so `ActiveAuras` is virtually always present by the time combat starts. Document this caveat explicitly.

- **CC replacement semantics must match `apply_pending_auras`.** When a CC of the same DR category already exists on the target, the existing path removes it via `swap_remove` before adding the new one. The synchronous helper must do the same — otherwise instant CC openers could double-stack.

## Open Questions

### Resolved During Planning

- **Should buffs and DoTs also go synchronous?** No. Same-frame visibility only matters for auras that gate AI decisions. Buffs and DoTs do not, and converting them would expand scope without benefit.
- **Should `apply_pending_auras` be deleted?** No. Casted spells must apply auras only after the cast completes — the deferred path is correct for them. The fix is additive: instant CC openers gain a synchronous path; casted spells keep the deferred path.
- **Does this affect channeled abilities?** No. Channels do not currently use `AuraPending` for instant CC.

### Deferred to Implementation

- **Exact helper signature.** Whether the helper takes `&mut Commands` (for the no-`ActiveAuras` insert path and FCT spawning) versus a result struct the caller acts on. Both work; pick whichever produces the smallest diff at the call sites.
- **Whether to extract `apply_pending_auras`'s body into the helper and have `apply_pending_auras` call into it for each `AuraPending`, or keep them as parallel implementations sharing only sub-helpers.** Extraction is cleaner long-term but a bigger diff. Pick based on what the implementer finds when touching the code — extraction is preferred if it's not painful.
- **Whether `try_cheap_shot` already has access to the target's `ActiveAuras` via an existing query.** The class AI dispatch system reads many components per combatant; verify whether `ActiveAuras` is already in the query or needs to be added.

## High-Level Technical Design

> *This illustrates the intended approach and is directional guidance for review, not implementation specification. The implementing agent should treat it as context, not code to reproduce.*

```
BEFORE (instant CC opener):
  try_cheap_shot:
    consume mana, set GCD, break stealth
    log ability use
    commands.spawn(AuraPending { stun })   // deferred — not visible same frame
    return true

  apply_pending_auras (later in frame):
    check charging immunity
    check Divine Shield immunity
    check DR (immunity, multiplier)
    handle CC replacement
    push aura into ActiveAuras
    log + FCT

AFTER (instant CC opener):
  try_cheap_shot:
    consume mana, set GCD, break stealth
    log ability use
    apply_cc_aura_now(target, stun, ...)   // synchronous — visible immediately
    return true

  apply_cc_aura_now (new helper in auras.rs):
    same logic as apply_pending_auras inner loop:
      charging immunity → log + return
      Divine Shield immunity → log + return
      DR check → immune (log + return) or multiplier
      CC replacement (swap_remove existing same-category)
      push aura into target's ActiveAuras (insert component if missing)
      log + FCT spawn

  apply_pending_auras (unchanged):
    still drains AuraPending entities for casted-spell auras, buffs, DoTs
```

## Implementation Units

- [ ] **Unit 1: Extract synchronous CC application helper**

  **Goal:** Add a single helper function that applies a CC aura to a target's live `ActiveAuras` component in the current frame, with full parity to `apply_pending_auras`'s CC handling (immunity, DR, CC replacement, logging, FCT).

  **Requirements:** R1, R2, R3, R4

  **Dependencies:** None

  **Files:**
  - Modify: `src/states/play_match/auras.rs` (add the helper alongside `apply_pending_auras`)

  **Approach:**
  - Identify the CC-specific portion of `apply_pending_auras`'s loop body: charging immunity, Divine Shield hostile-aura block, DR category lookup, DR immunity check, DR multiplier application, CC replacement, aura push, combat log, DR-aware log, FCT spawn
  - Extract that into a helper that operates on a target entity, an `Aura` value, and the references it needs (target combatant, transform, optional `ActiveAuras`, optional `DRTracker`, fct state, pet query, combat log, commands)
  - Decide between (a) extracting fully and having `apply_pending_auras` call the helper per pending, or (b) duplicating the CC subset and keeping `apply_pending_auras` unchanged. Prefer (a) if the diff stays manageable.
  - The helper must handle the "no `ActiveAuras` component yet" case via `commands.entity(target).insert(ActiveAuras { ... })`, matching the existing `new_auras_map` flow's intent
  - The helper must NOT handle buff/DoT-only behaviors that don't apply to instant CC (e.g., MaxHealth mutation, AttackPowerIncrease logging) — those stay exclusive to `apply_pending_auras`

  **Patterns to follow:**
  - `apply_pending_auras` in the same file — every step the helper performs must have an existing equivalent there
  - The recently merged dynamic stat auras refactor's helper extraction style (`get_attack_power_bonus_from_slice`, etc.) for naming and module organization

  **Test scenarios:**
  - Happy path: helper called with a Stun aura on a target with no existing CC → aura is pushed, DR level advances, combat log entry written
  - Edge case: helper called on a target who is `charging` → aura is rejected, "Immune" FCT spawned, combat log notes immunity, target's `ActiveAuras` unchanged
  - Edge case: helper called on a target with `DamageImmunity` (Divine Shield) → aura is rejected, "Immune" FCT spawned, target's `ActiveAuras` unchanged
  - Edge case: helper called on a target who is already DR-immune in the relevant category → aura is rejected, "IMMUNE" FCT spawned, DR-immune log entry written
  - Edge case: helper called with a Stun on a target who already has a Stun aura → existing Stun is removed (CC replacement), new Stun is pushed with DR multiplier applied
  - Edge case: helper called on a target with no `ActiveAuras` component → component is inserted with the new aura
  - Integration: helper output must produce identical combat log and FCT entries to the equivalent flow through `apply_pending_auras` for the same input

  **Verification:**
  - All existing tests pass
  - A new direct-call test (or characterization test) demonstrates parity with the deferred path for at least one CC type per DR category (Stun, Polymorph/Incapacitate, Root, Fear, Slow)

- [ ] **Unit 2: Convert instant CC openers to call the synchronous helper**

  **Goal:** Switch every class AI site that applies an instant CC aura via `commands.spawn(AuraPending { ... })` to call the new synchronous helper instead, so the aura is visible to other AI in the same frame.

  **Requirements:** R1, R2, R5

  **Dependencies:** Unit 1

  **Files:**
  - Modify: `src/states/play_match/class_ai/rogue.rs` (`try_cheap_shot`, `try_kidney_shot`)
  - Modify: `src/states/play_match/class_ai/paladin.rs` (Hammer of Justice site near line 652)
  - Modify: any other class AI file the implementer discovers using `commands.spawn(AuraPending { ... })` for an **instant** stun, root, fear, polymorph, or incapacitate
  - Possibly modify: `src/states/play_match/class_ai/mod.rs` if the per-combatant query needs an extra `ActiveAuras` or `DRTracker` borrow

  **Approach:**
  - Audit every `commands.spawn(...AuraPending...)` call site listed in repo grep results
  - For each site, classify the spawned aura as **instant CC** (applied at the moment of ability use) versus **deferred** (applied after a cast completes, or applied as a buff/DoT). Only instant CC sites are converted
  - Convert each instant CC site to call the new helper with the same aura data
  - Where the class AI site lacks live `ActiveAuras` / `DRTracker` access, add the necessary query parameters or borrow plumbing — mirror how the dynamic stat auras refactor threaded `ctx.active_auras` into instant attack functions
  - **Do NOT touch sites that spawn `AuraPending` for casted-spell aura application or buffs.** Those must stay on the deferred path
  - The implementer should verify each converted site by reading the surrounding code to confirm the aura is "instant" semantics, not a delayed payload

  **Patterns to follow:**
  - The same call shape used today for `commands.spawn(aura_pending)`, just routed through the new helper instead
  - Existing class AI synchronous mutations (mana decrement, GCD set, stealth break) for plumbing reference — those already mutate the acting combatant directly

  **Test scenarios:**
  - Integration: Rogue uses Cheap Shot from stealth on a Priest who would otherwise start Flash Heal → after the rogue's `try_cheap_shot` returns, the Priest's class AI runs and observes the stun, does not start the cast, and the Rogue does not need to use Kick. This is the original observed bug.
  - Integration: Paladin uses Hammer of Justice on a casting Mage → cast cancels in the same frame the Hammer lands, no separate interrupt needed
  - Integration: Rogue uses Kidney Shot on an enemy → enemy's class AI observes the stun in the next AI tick within the same frame
  - Edge case: Rogue uses Cheap Shot on a `charging` enemy → still gets the "Immune" treatment (charging immunity preserved)
  - Edge case: Rogue uses Cheap Shot on an enemy under Divine Shield → still gets the immunity treatment
  - Edge case: Rogue uses Cheap Shot on an enemy already at DR-immune Stun level → CC blocked, DR-immune log written
  - Regression: a casted CC (Polymorph from Mage) still applies via the deferred path and behaves identically to current
  - Regression: Battle Shout, Power Word: Fortitude, and Commanding Shout still flow through `AuraPending` (no behavioral change)

  **Verification:**
  - Headless match reproducing the original bug scenario shows the Priest does NOT start Flash Heal after a Cheap Shot from stealth
  - Headless match logs show no "Cheap Shot then Kick on same target same frame" sequence
  - All existing tests pass

## System-Wide Impact

- **Interaction graph:** Affects the class AI decision loop for any combatant deciding *after* an instant CC opener has fired in the same frame. Also affects `process_casting`'s incapacitation check (which already cancels casts on stunned targets) — that path becomes unreachable in the same-frame case because the cast is never started in the first place. Both behaviors converge on the same correct outcome.
- **State lifecycle risks:** Direct mutation of `ActiveAuras` mid-frame must respect the same invariants `apply_pending_auras` enforces (CC replacement, DR application, immunity blocks). Drift between the two paths is the largest risk; mitigated by extracting a shared helper rather than reimplementing per call site.
- **API surface parity:** Both headless and graphical modes use the same combat systems, so the fix applies to both automatically. No system registration changes needed.
- **Unchanged invariants:** `AuraPending`, `apply_pending_auras`, and the deferred path for casted spells / buffs / DoTs all remain. DR system semantics unchanged. CC immunity rules unchanged. Combat log format unchanged. Floating combat text behavior unchanged.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Synchronous helper drifts from `apply_pending_auras` over time, causing instant CC and casted CC to behave differently | Extract a shared inner helper so both paths share the same code. If full extraction is impractical, add a comment block in both paths that names the other as a parity target. |
| A buff or DoT site is mis-classified as "instant CC" and converted, breaking its delayed-application semantics | Implementer must verify each call site by reading surrounding code; the aura type alone is not sufficient (e.g., a Fear from a casted spell still goes deferred). The conversion list should be reviewed at PR time. |
| Adding `&mut ActiveAuras` and `&mut DRTracker` to class AI queries causes Bevy query conflicts with other systems running in the same set | The class AI dispatch already mutably borrows the acting combatant's components; adding aura/DR borrows on the *target* requires care. If a conflict arises, fall back to a per-call `Query` parameter or restructure the helper to take `Commands` + entity and do the borrow internally. |
| The "no `ActiveAuras` component yet" insert path leaves a one-frame visibility gap for that specific case | In practice every combatant has `ActiveAuras` by the time combat starts (pre-match buffs ensure this). Document the caveat in the helper's doc comment. |

## Sources & References

- Related PR: #36 (dynamic stat auras refactor — established the pattern of synchronous mutation in class AI instant attacks)
- Related observation: same-frame Cheap Shot → Flash Heal → Kick sequence in match log on 2026-04-05
- Related code: `apply_pending_auras` in `src/states/play_match/auras.rs` (the parity target for the new helper)
- Related code: `process_casting` incapacitation check in `src/states/play_match/combat_core/casting.rs` (the existing safety net that the fix renders unreachable in the same-frame case)
- Memory record 1261: prior "stunned combatants using abilities" fix that handled the prior-frame case but not same-frame
