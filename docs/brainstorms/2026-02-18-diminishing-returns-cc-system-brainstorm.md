# Diminishing Returns (DR) System for Crowd Control

**Date:** 2026-02-18
**Status:** Brainstorm complete, ready for planning

## What We're Building

A Classic WoW-faithful diminishing returns system for crowd control effects. Each successive CC of the same DR category applied to the same target has reduced duration, eventually granting full immunity.

**DR Curve:** 100% → 50% → 25% → Immune
**Reset Timer:** 15 seconds after last application in that category

### DR Categories

| Category | Abilities | AuraType(s) |
|---|---|---|
| Stuns | Cheap Shot, Kidney Shot, Hammer of Justice | `Stun` |
| Fears | Fear | `Fear` |
| Incapacitates | Polymorph | `Polymorph` |
| Roots | Frost Nova | `Root` |
| Slows | Frostbolt slow | `MovementSpeedSlow` |

**Excluded from DR:** SpellSchoolLockout (interrupts) — these are already gated by ability cooldowns.

### Immunity Behavior

- After the 3rd application (25% duration), the target becomes immune to that DR category
- Immunity lasts until the 15-second reset timer expires
- Immune applications show "IMMUNE" floating combat text
- Immune events are logged in the combat log
- AI can query DR state to avoid wasting CCs on immune targets

## Why This Approach

**DRTracker component on each combatant entity** (Approach A):
- Follows standard Bevy ECS patterns — state about a combatant lives on its entity
- `HashMap<DRCategory, DRState>` stores diminishment level (0-3) and time since last CC
- AI queries via `Query<&DRTracker>` — natural, no global lookups
- Automatic cleanup on entity despawn
- Minimal coupling to existing `ActiveAuras` system

**Rejected alternatives:**
- Extending `ActiveAuras` — muddies responsibility (active effects vs. historical CC counts)
- Global `Res<DRTable>` — breaks ECS locality, manual cleanup on despawn

## Key Decisions

1. **Classic-faithful model** — 100%/50%/25%/immune with 15s reset, no simplifications
2. **5 DR categories** — Stuns, Fears, Incapacitates, Roots, Slows (each independent)
3. **Interrupts excluded** — SpellSchoolLockout stays DR-free per Classic WoW
4. **Visible immunity feedback** — IMMUNE text + combat log + AI awareness
5. **DRTracker component** — per-entity, HashMap-based, clean ECS pattern
6. **DR only scales duration** — break-on-damage thresholds remain unchanged

## Refined Decisions

7. **Threshold-based AI awareness** — AI skips CC only when target is DR-immune (level 3). Still casts at 50% and 25% duration since reduced CC is still valuable. Avoids wasting abilities into immunity without over-optimizing.
8. **Combat log includes DR percentage** — Format: `[CC] Polymorph on Warrior (6.0s, DR: 50%)`. On immunity: `[CC] Polymorph RESISTED on Warrior (DR immune)`. Full info on every application for balance analysis.
9. **Graphical UI indicators deferred** — Decide during planning whether visual DR indicators are worth the effort. Core mechanic works without them.

## Open Questions

- Graphical UI: should DR state show near health bars? (deferred to planning phase)
