---
title: "fix: Despawn traps and ice blocks on match end"
type: fix
status: completed
date: 2026-03-07
deepened: 2026-03-07
---

# fix: Despawn traps and ice blocks on match end

## Enhancement Summary

**Deepened on:** 2026-03-07
**Review agents used:** pattern-recognition, architecture-strategist, code-simplicity, performance-oracle, entity-audit

### Key Improvements
1. Clarified guard style — use multi-line with comment (dominant codebase pattern)
2. Confirmed belt-and-suspenders approach is necessary due to Bevy command deferral timing
3. IceBlockVisual despawn confirmed critical for pre-existing freeze case (not just new triggers)
4. Identified additional entities with same problem as out-of-scope follow-ups

## Problem Statement

When a combatant walks into a Freezing Trap during the victory celebration, the `trap_system()` triggers and spawns an `IceBlockVisual` on the celebrating combatant. The celebration bounce animation then makes the combatant jump up and down inside a translucent ice cube — a jarring visual bug.

Additionally, if a combatant is *already* frozen when the match ends (teammate died), the `IceBlockVisual` persists for the entire 5-second celebration because `process_aura_ticks` is guarded by `VictoryCelebration` and never expires the Incapacitate aura, so `cleanup_ice_blocks()` never fires.

**Root cause**: `check_match_end()` despawns `Projectile` and `SpellImpactEffect` entities but does not despawn `Trap`, `TrapLaunchProjectile`, `SlowZone`, or `IceBlockVisual` entities. The `trap_system()` and `slow_zone_system()` also lack `VictoryCelebration` guards.

## Proposed Solution

Follow the established patterns from projectile cleanup (`match_flow.rs:236-244`) and the pet despawn guard (`combat_core.rs:2341`):

1. **Add `VictoryCelebration` guards** to `trap_system()` and `slow_zone_system()` — prevents new traps from triggering and new slow auras from applying during celebration
2. **Despawn trap-related entities in `check_match_end()`** — cleans up entities that already exist when the match ends (armed traps, active slow zones, pre-existing ice blocks)

### Why Both Are Needed

The guards alone prevent NEW trap triggers, but don't handle pre-existing entities:
- An `IceBlockVisual` already on a frozen combatant when their teammate dies
- A `SlowZone` already active from an earlier Frost Trap trigger
- Armed traps sitting on the ground (visually harmless but unclean)

The despawns alone don't prevent the one-frame race where `trap_system` runs in Phase 2 (CombatAndMovement) **before** `check_match_end` in the resolution group on the match-ending frame. Bevy's deferred commands mean the despawn hasn't applied yet.

## Acceptance Criteria

- [x] `trap_system()` has `Option<Res<VictoryCelebration>>` early-return guard
- [x] `slow_zone_system()` has `Option<Res<VictoryCelebration>>` early-return guard
- [x] `move_trap_launch_projectiles()` already has guard (verify, no change needed)
- [x] `Trap` entities despawned in `check_match_end()`
- [x] `TrapLaunchProjectile` entities despawned in `check_match_end()`
- [x] `SlowZone` entities despawned in `check_match_end()`
- [x] `IceBlockVisual` entities despawned in `check_match_end()`
- [x] Headless test: `Hunter vs Warrior` completes without panics

## MVP

### `src/states/play_match/traps.rs` — trap_system()

Add VictoryCelebration guard at the top of the function. Use multi-line style with comment (dominant codebase pattern):

```rust
pub fn trap_system(
    // ... existing params ...
    celebration: Option<Res<VictoryCelebration>>,
) {
    // Don't trigger traps during victory celebration
    if celebration.is_some() {
        return;
    }
    // ... rest of function unchanged ...
}
```

### `src/states/play_match/traps.rs` — slow_zone_system()

Same guard pattern:

```rust
pub fn slow_zone_system(
    // ... existing params ...
    celebration: Option<Res<VictoryCelebration>>,
) {
    // Don't apply slow zone auras during victory celebration
    if celebration.is_some() {
        return;
    }
    // ... rest of function unchanged ...
}
```

### Verify: `move_trap_launch_projectiles()` already has guard

Confirm line ~268 of traps.rs already has `Option<Res<VictoryCelebration>>` guard — no changes needed.

### `src/states/play_match/match_flow.rs` — check_match_end()

Add queries and despawn loops for trap entities alongside existing projectile/effect cleanup. Use `despawn_recursive()` for consistency with existing pattern:

```rust
// Add to check_match_end() function signature:
traps: Query<Entity, With<Trap>>,
trap_projectiles: Query<Entity, With<TrapLaunchProjectile>>,
slow_zones: Query<Entity, With<SlowZone>>,
ice_blocks: Query<Entity, With<IceBlockVisual>>,

// Add after existing projectile/spell effect despawn block:

// Despawn all active traps to prevent triggering during celebration
for trap_entity in traps.iter() {
    commands.entity(trap_entity).despawn_recursive();
}

// Despawn all in-flight trap projectiles
for trap_proj_entity in trap_projectiles.iter() {
    commands.entity(trap_proj_entity).despawn_recursive();
}

// Despawn all active slow zones
for zone_entity in slow_zones.iter() {
    commands.entity(zone_entity).despawn_recursive();
}

// Despawn all ice block visuals (aura system frozen during celebration prevents self-cleanup)
for ice_entity in ice_blocks.iter() {
    commands.entity(ice_entity).despawn_recursive();
}
```

## Technical Notes

- **Must use `Option<Res<VictoryCelebration>>`** (not `Res<VictoryCelebration>`) — the resource doesn't exist until match ends, and headless mode may never insert it
- **Use multi-line guard style** with descriptive comment — this is the dominant pattern (~10 systems) vs single-line (2 systems)
- **Residual auras on combatants** (e.g., MovementSpeedSlow from Frost Trap) are harmless — aura ticking is frozen during celebration, and all entities are cleaned up on state exit via `PlayMatchEntity` marker
- **TrapBurst** (0.3s lifetime) is harmless and self-cleans — no explicit despawn needed
- **No system registration changes needed** — changes are to function bodies only, both graphical and headless paths pick them up automatically
- **System parameter count**: `check_match_end()` goes from ~7 to ~11 parameters, well within Bevy's 16-parameter limit

### Command Deferral Timing

On the match-ending frame:
1. Phase 2 (CombatAndMovement): `trap_system` runs — guard not yet active (VictoryCelebration not inserted yet)
2. Resolution group: `check_match_end` issues despawn commands + inserts VictoryCelebration
3. Commands flush between frames
4. Next frame Phase 2: `trap_system` sees VictoryCelebration and returns early

The guards close the one-frame window. The despawns clean up pre-existing entities. Both are necessary.

## Out of Scope (Follow-up)

Entity audit found other entity types with the same class of bug during victory celebration:
- `ShadowSightOrb` — continues animating (pulse, bob, rotate) during celebration
- `DrainLifeBeam` + `DrainParticle` — frozen mid-channel if Warlock was channeling at match end
- `ShieldBubble` — follows celebrating/dead combatants
- `PolymorphedVisual` — cuboid persists on polymorphed combatant

These should be addressed in a follow-up cleanup pass.

## Sources

- Existing cleanup pattern: `src/states/play_match/match_flow.rs:236-244` (projectile/effect despawn)
- Pet guard pattern: `src/states/play_match/combat_core.rs:2341` (`despawn_pets_of_dead_owners`)
- Trap components: `src/states/play_match/components/mod.rs:1258-1352`
- Trap systems: `src/states/play_match/traps.rs`
- Ice block visuals: `src/states/play_match/rendering/effects.rs:1253-1327`
- Documented pattern: `docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md`
