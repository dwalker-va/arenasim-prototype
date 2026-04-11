---
title: "Hunter Trap Launch Arc"
type: feat
date: 2026-03-04
deepened: 2026-03-04
status: completed
---

# Hunter Trap Launch Arc

## Enhancement Summary

**Deepened on:** 2026-03-04
**Agents used:** performance-oracle, architecture-strategist, code-simplicity-reviewer, pattern-recognition-specialist, visual-effect-learnings, system-registration-learnings

### Key Improvements
1. Explicit system registration order with exact insertion points
2. Confirmed `origin` field is required (Transform is mutated each frame, so origin can't be derived)
3. Added edge case handling: match-end guard, frame ordering between landing and trap_system
4. Verified arc math: `sin(t * PI)` peaks at t=0.5, correctly produces parabola from 0→peak→0

### Review Verdicts
- **Performance**: Approved — max 6 concurrent projectiles, sin() cost negligible (~360 calls/sec)
- **Architecture**: Clean ECS design — separate component from `Projectile` is correct (Vec3 target vs Entity target)
- **Pattern Compliance**: All 10 codebase patterns followed (AlphaMode::Add, try_insert, PlayMatchEntity, dual registration, etc.)
- **Simplicity**: `origin` and `total_distance` fields kept — origin needed because Transform is mutated per frame; distance threshold kept per user requirement

## Overview

Rather than traps appearing instantly at their target location, hunters "launch" traps in an arc. The trap travels through the air to the target position before beginning its arming timer. If the hunter places a trap within 10 yards of himself, it drops immediately with no travel time (current behavior).

## Problem Statement

Currently, `try_place_trap_at()` in `class_ai/hunter.rs` spawns the `Trap` entity instantly at the target world position. This looks jarring — especially at match start when traps pop into existence 15-20 yards away with no visual connection to the Hunter. The mechanic also has zero travel-time cost for distant placements, which is a slight tactical freebie.

## Proposed Solution

Add a `TrapLaunchProjectile` component that arcs from the Hunter to the landing position. On arrival, it despawns and spawns the regular `Trap` entity (which then arms via the existing 1.5s timer). Short-range placements (distance <= 10.0 units) skip the launch phase entirely.

### Distance Threshold

- `<= 10.0` units from Hunter to landing position → **drop** (current instant behavior)
- `> 10.0` units → **launch** (new arc projectile with travel time)

Use post-`clamp_to_arena` position for the distance measurement (consistent with where the trap actually lands).

### Travel Speed & Arc

- `TRAP_LAUNCH_SPEED: f32 = 20.0` — horizontal speed in units/sec
- `TRAP_LAUNCH_ARC_HEIGHT: f32 = 6.0` — peak Y offset at midpoint of travel
- Arc is a simple `sin(progress * PI) * arc_height` curve on Y axis
- Headless mode uses the same travel time but ignores Y (straight-line timer countdown)
- Travel time example: 20 units distance = 1.0s travel, 30 units = 1.5s travel

### Hunter Death / Match End During Travel

- **Hunter dies**: projectile continues, trap spawns and functions (consistent with existing trap behavior where `trap_system` never checks owner liveness)
- **Match ends (VictoryCelebration)**: landing system returns early, projectile effectively vanishes (consistent with `process_projectile_hits` guard)

### Combat Log

- On launch: `[TRAP] Team X Hunter launches Freezing Trap toward (x, z)`
- On landing: `[TRAP] Freezing Trap lands at (x, z)`
- Short-range drop: existing log unchanged (`[TRAP] Team X Hunter places Freezing Trap at (x, z)`)

## Changes

### 1. New constants — `constants.rs`

| Constant | Value | Purpose |
|----------|-------|---------|
| `TRAP_LAUNCH_MIN_RANGE` | `10.0` | Distance threshold for launch vs drop |
| `TRAP_LAUNCH_SPEED` | `20.0` | Horizontal travel speed (units/sec) |
| `TRAP_LAUNCH_ARC_HEIGHT` | `6.0` | Peak Y offset of parabolic arc |

### 2. New component — `components/mod.rs`

Add `TrapLaunchProjectile` near the existing `Trap`/`TrapBurst` structs (~line 1249):

```rust
/// A trap that has been lobbed and is traveling through the air to its landing position.
/// On arrival, despawns and spawns a regular Trap entity.
#[derive(Component)]
pub struct TrapLaunchProjectile {
    pub trap_type: TrapType,
    pub owner_team: u8,
    pub owner: Entity,
    pub origin: Vec3,          // Hunter's position at launch
    pub landing_position: Vec3, // World-space target (post-clamp)
    pub total_distance: f32,    // Precomputed horizontal distance
    pub distance_traveled: f32, // Accumulated horizontal travel
}
```

Track `origin` + `total_distance` + `distance_traveled` to compute progress `t = distance_traveled / total_distance` for the arc height calculation.

### 3. Modify trap spawning — `class_ai/hunter.rs`

In `try_place_trap_at()` (~line 257), after the existing `clamp_to_arena` call:

```rust
let distance = Vec3::new(my_pos.x, 0.0, my_pos.z)
    .distance(Vec3::new(position.x, 0.0, position.z));

if distance > TRAP_LAUNCH_MIN_RANGE {
    // Launch: spawn arc projectile from Hunter position
    commands.spawn((
        Transform::from_translation(Vec3::new(my_pos.x, 1.5, my_pos.z)),
        TrapLaunchProjectile {
            trap_type,
            owner_team: combatant.team,
            owner: entity,
            origin: Vec3::new(my_pos.x, 1.5, my_pos.z),
            landing_position: Vec3::new(position.x, 0.0, position.z),
            total_distance: distance,
            distance_traveled: 0.0,
        },
        PlayMatchEntity,
    ));
    combat_log.log(&format!("[TRAP] Team {} Hunter launches {} at ({:.1}, {:.1})",
        combatant.team, trap_type_name, position.x, position.z));
} else {
    // Drop: existing instant spawn (current behavior)
    commands.spawn((
        Transform::from_translation(Vec3::new(position.x, 0.0, position.z)),
        Trap { /* existing fields */ },
        PlayMatchEntity,
    ));
    combat_log.log(&format!("[TRAP] Team {} Hunter places {} at ({:.1}, {:.1})",
        combatant.team, trap_type_name, position.x, position.z));
}
```

This requires passing `my_pos: Vec3` into `try_place_trap_at` (currently not passed — add it as a parameter).

### 4. New movement system — `traps.rs`

Add `move_trap_launch_projectiles` system:

```rust
pub fn move_trap_launch_projectiles(
    mut commands: Commands,
    time: Res<Time>,
    mut projectiles: Query<(Entity, &mut Transform, &mut TrapLaunchProjectile)>,
    mut combat_log: ResMut<CombatLog>,
    celebration: Option<Res<VictoryCelebration>>,
) {
    if celebration.is_some() { return; }
    let dt = time.delta_secs();

    for (entity, mut transform, mut proj) in projectiles.iter_mut() {
        // Advance horizontal distance
        proj.distance_traveled += TRAP_LAUNCH_SPEED * dt;

        if proj.distance_traveled >= proj.total_distance {
            // Arrived — spawn Trap, despawn projectile
            commands.spawn((
                Transform::from_translation(proj.landing_position),
                Trap {
                    trap_type: proj.trap_type,
                    owner_team: proj.owner_team,
                    owner: proj.owner,
                    arm_timer: TRAP_ARM_DELAY,
                    trigger_radius: TRAP_TRIGGER_RADIUS,
                    triggered: false,
                },
                PlayMatchEntity,
            ));
            let name = match proj.trap_type {
                TrapType::Freezing => "Freezing Trap",
                TrapType::Frost => "Frost Trap",
            };
            combat_log.log(&format!("[TRAP] {} lands at ({:.1}, {:.1})",
                name, proj.landing_position.x, proj.landing_position.z));
            commands.entity(entity).despawn();
            continue;
        }

        // Interpolate position along arc
        let t = proj.distance_traveled / proj.total_distance;
        let horizontal = proj.origin.lerp(
            Vec3::new(proj.landing_position.x, proj.origin.y, proj.landing_position.z), t);
        let arc_y = (t * std::f32::consts::PI).sin() * TRAP_LAUNCH_ARC_HEIGHT;
        transform.translation = Vec3::new(horizontal.x, arc_y, horizontal.z);

        // Rotate to face travel direction
        let direction = (proj.landing_position - proj.origin).normalize_or_zero();
        if direction != Vec3::ZERO {
            transform.rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
        }
    }
}
```

### 5. Visual effects — `rendering/effects.rs`

Add a spawn/update pair for the in-flight trap projectile (following three-system pattern from MEMORY.md):

**`spawn_trap_launch_visuals`**: Detects `Added<TrapLaunchProjectile>` without `Mesh3d`. Inserts a small sphere mesh (radius 0.3) with trap-type color via `trap_type_rgb`/`trap_type_emissive` helpers. Uses `AlphaMode::Add`, `try_insert()`.

**`update_trap_launch_visuals`**: Each frame, apply a gentle rotation spin for visual interest. No cleanup system needed — the projectile entity is despawned by the movement system on arrival.

### 6. System registration — `states/mod.rs` + `systems.rs`

**Headless + graphical** (both registration points):
- `move_trap_launch_projectiles` — runs in `CombatAndMovement` phase, **after** `move_projectiles` and **before** `trap_system`

In `systems.rs::add_core_combat_systems()`, insert into the Phase 2 chain:
```rust
move_projectiles,
move_trap_launch_projectiles,  // NEW — after regular projectiles, before trap_system
process_projectile_hits,
// ...
trap_system,
```

In `states/mod.rs`, add to the same CombatAndMovement phase chain.

**Graphical only** (`states/mod.rs`):
- `spawn_trap_launch_visuals`
- `update_trap_launch_visuals`

Add as a new `.add_systems()` group (Bevy tuple size limits) after existing trap visual systems.

### 7. Module exports

- Add `TrapLaunchProjectile` to `pub use` in `play_match/mod.rs` components re-export
- New systems in `traps.rs` are already covered by `pub use super::traps::*` in `systems.rs`

## Files Modified

| File | Change |
|------|--------|
| `src/states/play_match/constants.rs` | Add `TRAP_LAUNCH_MIN_RANGE`, `TRAP_LAUNCH_SPEED`, `TRAP_LAUNCH_ARC_HEIGHT` |
| `src/states/play_match/components/mod.rs` | Add `TrapLaunchProjectile` component |
| `src/states/play_match/class_ai/hunter.rs` | Branch `try_place_trap_at` on distance: launch vs drop |
| `src/states/play_match/traps.rs` | Add `move_trap_launch_projectiles` system |
| `src/states/play_match/rendering/effects.rs` | Add spawn/update visuals for in-flight trap |
| `src/states/mod.rs` | Register all new systems (headless + graphical) |
| `src/states/play_match/systems.rs` | Register `move_trap_launch_projectiles` in headless path |

## Edge Cases

- **Hunter dies mid-flight**: Projectile continues and spawns functional trap (consistent with existing trap behavior — `trap_system` never checks owner liveness)
- **Match ends mid-flight**: `celebration.is_some()` guard causes early return, projectile effectively vanishes
- **Trap at exactly 10.0 units**: Uses `> TRAP_LAUNCH_MIN_RANGE` check, so exactly 10.0 = drop (short-range path)
- **Landing position outside arena**: Already handled — `clamp_to_arena()` runs before distance check
- **Enemy moves after launch**: Trap lands at original target position (fixed Vec3, not entity-tracking) — correct WoW behavior
- **Multiple traps in flight**: Max 2 per hunter (Freezing + Frost on separate cooldowns), max 6 total in 3v3 — no contention

## Acceptance Criteria

- [x] Traps placed > 10 yards from Hunter arc through the air before landing
- [x] Traps placed <= 10 yards appear instantly (existing behavior unchanged)
- [x] Arm timer (1.5s) starts only after landing, not during travel
- [x] In-flight trap has visible colored sphere matching trap type
- [x] Headless mode has matching travel delay (verifiable via combat log timestamps)
- [x] Match-end guard prevents trap landing during VictoryCelebration
- [x] `cargo build --release` compiles clean with no warnings

## Verification

```bash
# Headless: verify travel time gap in combat log
echo '{"team1":["Hunter"],"team2":["Warrior"]}' > /tmp/test.json
cargo run --release -- --headless /tmp/test.json
# Check log for "launches" → "lands" entries with time gap

# Graphical: watch for arc animation
cargo run --release
# Configure Hunter vs Warrior, observe trap launch arc
```
