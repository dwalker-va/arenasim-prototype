---
title: "fix: Frost Nova CC applied through Divine Shield"
type: fix
status: completed
date: 2026-03-21
---

# Fix Frost Nova CC Applied Through Divine Shield (BUG-7)

## Overview

Frost Nova's root CC is logged as applied to a Paladin with active Divine Shield. The `apply_pending_auras()` system in `auras.rs:186-221` correctly blocks the root via its DamageImmunity check, but the Frost Nova code in `mage.rs` logs the CC to the combat log **before** the aura system runs the immunity check. This creates a false positive in the combat log.

## Root Cause

In `mage.rs:312-351`, for each Frost Nova target:
1. Line 317: Damage is queued via `QueuedAoeDamage` (damage immunity checked later → correctly returns 0)
2. Line 329: `AuraPending` is spawned for root aura (immunity checked later in `apply_pending_auras`)
3. Lines 333-349: **CC is logged to combat log immediately** — before any immunity check runs

When `apply_pending_auras()` runs next frame, it blocks the root and despawns the pending entity, but the log entry is already written. No counter-log entry ("Immune") is added to the combat log.

## Fix

Add a `ctx.entity_is_immune()` check before spawning `AuraPending` and logging CC. This method already exists on `CombatContext` and checks for `DamageImmunity` aura.

**File:** `src/states/play_match/class_ai/mage.rs` (~line 327)

Wrap the root aura spawn + CC logging in an immunity check:
```rust
// Skip root on immune targets (Divine Shield)
if !ctx.entity_is_immune(*target_entity) {
    if let Some(aura_pending) = AuraPending::from_ability(*target_entity, entity, nova_def) {
        commands.spawn(aura_pending);
    }
    // Log CC...
}
```

The `apply_pending_auras()` immunity check in `auras.rs:186-221` remains as a belt-and-suspenders safety net.

## Acceptance Criteria

- [x] Frost Nova does NOT root a Paladin with active Divine Shield
- [x] No CC log entry for immune targets
- [x] Frost Nova still roots non-immune targets normally
- [x] Frost Nova damage still shows 0 against Divine Shield (existing behavior)

## Verification

```bash
echo '{"team1":["Mage","Priest"],"team2":["Warlock","Paladin"],"random_seed":6002}' > /tmp/bug7.json
cargo run --release -- --headless /tmp/bug7.json
```

## Sources

- Bug report: `docs/reports/2026-03-16-headless-match-bug-report.md` (BUG-7)
- Immunity check pattern: `auras.rs:186-221`
- `entity_is_immune()`: `class_ai/mod.rs:231-236`
