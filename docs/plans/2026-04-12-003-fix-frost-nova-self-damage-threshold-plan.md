---
title: "fix: Exclude application-frame damage from aura break threshold and raise Frost Nova threshold"
type: fix
status: completed
date: 2026-04-12
origin: docs/reports/2026-04-12-bug-hunt-2v2-3v3.md
---

# fix: Exclude application-frame damage from aura break threshold and raise Frost Nova threshold

## Overview

Two issues make Frost Nova's root break too easily: (1) Frost Nova's own instant damage (~25) counts against the root's 35 break threshold, consuming 70%+ of the budget before any external damage arrives, and (2) the threshold of 35 is too low to survive incidental chip damage during the 1.5s Frostbolt cast window needed for the shatter combo pattern.

## Problem Frame

Frost Nova is a PBAoE that simultaneously deals damage and applies a root. The root has `break_on_damage_threshold: 35.0`. Due to how `DamageTakenThisFrame` carries across the frame boundary into `process_aura_breaks`, the Frost Nova's own ~25 damage gets counted against the root's threshold on the aura's very first frame. This leaves only ~10 points of headroom.

In m11 (seed 6011), a single 9-damage Priest Wand Shot pushed cumulative damage from 27 to 36, breaking the root 0.5s after application. The Mage never had a chance to cast Frostbolt into the freeze.

The shatter combo (Frost Nova → Frostbolt into frozen target) is a core Frost Mage gameplay pattern. The root needs to survive incidental damage long enough for a Frostbolt to cast and travel, but should break cleanly when the Frostbolt lands (~100 damage).

## Requirements Trace

- R1. An ability's own damage must not count toward its own aura's break threshold on the application frame
- R2. Frost Nova's break threshold must be high enough to survive incidental chip damage during a 1.5s Frostbolt cast + projectile travel time
- R3. The threshold must still be low enough that focused DPS (Frostbolt, Mortal Strike) breaks the root
- R4. The fix must be generic — any future ability that deals damage and applies a breakable aura benefits from the same protection

## Scope Boundaries

- Does NOT change which damage sources can break auras (allied damage breaking friendly CC is correct WoW behavior)
- Does NOT add the shatter talent itself — just ensures the root survives long enough that shatter gameplay is viable when the talent is added
- Does NOT change Polymorph, Fear, or other CC break thresholds
- Frost Trap's threshold (also 35.0) may warrant the same bump — deferred to implementation to verify

## Context & Research

### Relevant Code and Patterns

- **Frost Nova config**: `abilities.ron` — `break_on_damage: 35.0`, damage base 5-10, coefficient 0.2
- **Aura struct**: `components/auras.rs:109` — `break_on_damage_threshold`, `accumulated_damage` fields
- **Break processing**: `auras.rs:666` `process_aura_breaks` — accumulates `DamageTakenThisFrame.amount` against each aura's threshold
- **DamageTakenThisFrame**: `components/combatant.rs:539` — simple `{ amount: f32 }`, no source tracking
- **System ordering**: Phase 1 `apply_pending_auras` (step 8) → Phase 2 `process_aura_breaks` (step 1). The aura is created in Phase 1, and the damage from the previous frame's Frost Nova is processed against it in Phase 2 of the same frame.

### Damage Budget Analysis

With ~75 Mage spell power:
- Frost Nova self-damage: ~20-25 (base 5-10 + 0.2 * 75)
- Frostbolt: ~95-110 (base 10-15 + 0.8 * 75, after modifiers)
- Wand Shot: ~7-9
- Auto-attack: ~10-15
- DoT tick: ~10-14

Incidental damage during 1.5s cast + ~0.5s travel ≈ 15-30 from chip sources.

## Key Technical Decisions

- **Add `applied_this_frame: bool` to Aura struct**: When an aura is first created (via `apply_pending_auras` or synchronous CC application), this flag is set to true. In `process_aura_breaks`, auras with this flag skip damage accumulation and the flag is cleared. In `update_auras`, the flag is also cleared (belt-and-suspenders). This handles the frame-boundary issue generically — any damage that was tracked before the aura existed doesn't count against it. This is correct for all aura types: even Polymorph shouldn't break from damage that was dealt before the Polymorph landed.

- **Threshold of 80 for Frost Nova**: With self-damage excluded (R1), the threshold only needs to absorb incidental chip damage. 80 gives ~50-60 points of headroom after typical incidental damage (1-2 wand shots + DoT tick), while a Frostbolt (~100) cleanly breaks it. Focused DPS from two sources (e.g., Warrior Mortal Strike + Rogue Sinister Strike) would also break it, which is correct — the root should be breakable by coordinated damage.

## Open Questions

### Resolved During Planning

- **Should Frost Trap get the same threshold bump?** Yes, Frost Trap's root has the identical threshold (35.0) and the same gameplay concern. Update both in the config change.

- **Does `applied_this_frame` affect the synchronous CC path (`reflect_instant_cc_in_snapshot`)?** No. The synchronous path writes directly into the per-frame snapshot map, not into `ActiveAuras`. The actual aura is still created via `AuraPending` → `apply_pending_auras`, which will set the flag. The snapshot reflection is read-only for incapacitation checks.

## Implementation Units

- [ ] **Unit 1: Add `applied_this_frame` flag to Aura and skip damage on new auras**

  **Goal:** Prevent damage from the application frame from counting toward a breakable aura's threshold.

  **Requirements:** R1, R4

  **Dependencies:** None

  **Files:**
  - Modify: `src/states/play_match/components/auras.rs` (Aura struct)
  - Modify: `src/states/play_match/auras.rs` (`apply_pending_auras`, `process_aura_breaks`, `update_auras`, `reflect_instant_cc_in_snapshot`)
  - Test: headless match with seed 6011 (m11 config)

  **Approach:**
  - Add `pub applied_this_frame: bool` to the `Aura` struct, defaulting to `false`
  - In `apply_pending_auras`, set `applied_this_frame: true` on newly created auras when pushing them into `ActiveAuras`
  - In `process_aura_breaks`, when iterating breakable auras: if `applied_this_frame` is true, skip damage accumulation for that aura and set the flag to false. Other auras on the same entity still accumulate damage normally.
  - In `update_auras`, clear `applied_this_frame` to false on all auras (ensures flag is cleared even if `process_aura_breaks` didn't run due to no damage)
  - Anywhere else that creates Aura instances directly (synchronous CC application helpers), ensure the default `false` value is correct — those auras go through `apply_pending_auras` anyway

  **Patterns to follow:**
  - The existing `accumulated_damage` field pattern on Aura — per-aura tracking state
  - The `process_aura_breaks` loop at `auras.rs:686` which already iterates per-aura

  **Test scenarios:**
  - Happy path: Frost Nova roots target, next frame `process_aura_breaks` runs with DamageTakenThisFrame from Frost Nova damage → root does NOT accumulate damage, `applied_this_frame` is cleared
  - Happy path: Subsequent frame, target takes Wand Shot damage → root accumulates damage normally (flag is false)
  - Edge case: Polymorph applied, damage arrives same frame → Polymorph does NOT break (correct: the damage predates the Poly)
  - Edge case: Polymorph applied, no damage same frame, damage arrives next frame → Polymorph breaks normally
  - Edge case: Aura with `break_on_damage_threshold: -1.0` (non-breakable) → unaffected by the flag, threshold check is skipped entirely
  - Edge case: Multiple auras on same target, one new and one old → only the new aura skips damage, old aura accumulates normally

  **Verification:**
  - Run m11 config (seed 6011): Frost Nova root should NOT accumulate its own damage on the first frame

- [ ] **Unit 2: Raise Frost Nova and Frost Trap break thresholds**

  **Goal:** Set thresholds high enough for the shatter combo pattern while remaining breakable by focused damage.

  **Requirements:** R2, R3

  **Dependencies:** None (independent of Unit 1, but both needed for the full fix)

  **Files:**
  - Modify: `assets/config/abilities.ron` (FrostNova and FrostTrap entries)
  - Test: headless match reproduction

  **Approach:**
  - Change Frost Nova's `break_on_damage` from 35.0 to 80.0
  - Change Frost Trap's `break_on_damage` from 35.0 to 80.0 (same rationale)
  - Update any comments referencing the old threshold value

  **Test scenarios:**
  - Happy path: Frost Nova roots target → survives 2 Wand Shots (~18 total) → Frostbolt lands (~100) → root breaks
  - Happy path: Frost Nova roots target → Mortal Strike (~80-110) → root breaks
  - Edge case: Multiple chip damage sources total ~75 → root survives (just under threshold)
  - Edge case: Two simultaneous attacks totaling >80 → root breaks

  **Verification:**
  - Run m11 config (seed 6011): Frost Nova root should survive the Priest Wand Shot that previously broke it
  - Run a Mage vs Warrior match: Frost Nova root should break from Mortal Strike (>80 damage)

- [ ] **Unit 3: Validate with match reproductions**

  **Goal:** Confirm both fixes together resolve the original bug and don't regress.

  **Requirements:** R1, R2, R3

  **Dependencies:** Unit 1, Unit 2

  **Files:**
  - Test: `/tmp/bug-hunt/m11_2v2_pillar_mp_rpal.json` (seed 6011, original bug), additional diverse matches

  **Test scenarios:**
  - Bug fix: m11 (seed 6011) — Frost Nova root survives Frost Nova self-damage AND subsequent Priest Wand Shot
  - Regression: Polymorph still breaks on damage after its first frame
  - Regression: Fear still breaks on damage exceeding its 100.0 threshold
  - Regression: Root from Frost Trap behaves consistently with Frost Nova
  - Balance: Mage vs Warrior — verify root is breakable by melee burst

  **Verification:**
  - No `[EVENT] ... Frost Nova broke from damage` within 1s of application unless a large damage source (>80) lands
  - Polymorph and Fear break behavior unchanged in non-Frost-Nova scenarios

## System-Wide Impact

- **Interaction graph:** `process_aura_breaks` gains a new check on `applied_this_frame`. `apply_pending_auras` and `update_auras` set/clear the flag. No system ordering changes.
- **Unchanged invariants:** `DamageTakenThisFrame` is unchanged. Break-on-damage mechanics for non-application-frame damage are unchanged. All existing aura types continue to work — the flag defaults to false and is only set to true during the application frame.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| `applied_this_frame` flag not cleared in an edge case | Belt-and-suspenders: both `process_aura_breaks` and `update_auras` clear it. The flag can only be true for exactly one frame. |
| Threshold of 80 too high for some matchups | 80 is below a single Mortal Strike or Frostbolt. Melee classes that land one ability on the rooted target will break it. Only chip damage (wands, minor DoTs) fails to break. |
| Frost Trap threshold change affects Hunter balance | Frost Trap root was already identical to Frost Nova. The same shatter-style gameplay logic applies to Hunter traps. |

## Sources & References

- **Origin:** [Bug Hunt Report](docs/reports/2026-04-12-bug-hunt-2v2-3v3.md) — BUG-2
- Related code: `auras.rs` process_aura_breaks, `abilities.ron` FrostNova/FrostTrap entries
- Aura break thresholds documented in CLAUDE.md memory: 0.0 = any damage, -1.0 = never, positive = cumulative threshold
