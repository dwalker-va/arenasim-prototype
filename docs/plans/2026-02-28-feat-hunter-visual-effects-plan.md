---
title: "feat: Add Hunter Visual Effects"
type: feat
date: 2026-02-28
brainstorm: docs/brainstorms/2026-02-24-hunter-visual-effects-brainstorm.md
deepened: 2026-02-28
---

# feat: Add Hunter Visual Effects

## Enhancement Summary

**Deepened on:** 2026-02-28
**Agents used:** code-simplicity-reviewer, architecture-strategist, performance-oracle, best-practices-researcher

### Key Simplifications (from code-simplicity-reviewer)
1. **Cut CosmeticArrow** — Auto Shot damage is instant; a visual arrow arriving after HP drops is confusing. Not worth the complexity.
2. **Cut ConcussiveImpact** — Reuse `DispelBurst` with Hunter gold color instead of a new component.
3. **Drop SlowZoneVisual marker** — Trigger visual off `Added<SlowZone>` directly (insert mesh onto SlowZone entity).
4. **Drop TrapGroundCircle marker** — Trigger visual off `Added<Trap>` directly (insert mesh onto Trap entity).
5. **Merge update+cleanup** for fire-and-forget effects (TrapBurst, DisengageTrail, ChargeTrail).
6. **Consolidate from 6 phases to 3.**

**Result:** Components: 8 → 4. Systems: ~21 → ~12.

### Performance Recommendations (from performance-oracle)
- Cache mesh handles via `Local<Option<Handle<Mesh>>>` in spawn systems
- Use `despawn()` not `despawn_recursive()` for childless entities
- Max ~34 simultaneous visual entities — well within Bevy comfort zone

### Architecture Notes (from architecture-strategist)
- Spider Web `projectile_speed` should be 50+ (not 35) given its defensive root role
- Document that instant→projectile conversion delays damage/effects by ~0.5-0.8s
- AlphaMode mixed state is acceptable (older effects use Blend too)

## Overview

Add visual effects for all Hunter and Hunter pet abilities. The Hunter class currently has fully functional combat mechanics but zero visual feedback beyond floating combat text and speech bubbles. This plan covers traps, projectiles, movement abilities, and pet abilities.

## Problem Statement

When watching a Hunter fight, there's no visual distinction between abilities. Traps are invisible on the ground, the Disengage leap has no trail, ranged shots look identical to caster projectiles (spheres), and pet abilities have no visual feedback beyond speech bubbles. This makes Hunter matches hard to follow and visually unpolished compared to other classes.

## Proposed Solution

Implement visual effects following the established spawn/update/cleanup 3-system pattern. Each effect gets a marker component, systems in `rendering/effects.rs`, and registration in `states/mod.rs`.

## Key Architectural Decisions

These decisions resolve ambiguities identified during spec analysis:

1. **Aimed Shot, Arcane Shot, Concussive Shot, Spider Web → real projectiles.** Add `projectile_speed` to abilities.ron. Convert AI functions from instant damage/effects to spawning `Projectile` entities. The existing `process_projectile_hits` handles damage on arrival. This delays effects by travel time (~0.5-0.8s at 35-yard range) but is architecturally clean and reuses the existing projectile pipeline. All arrow visuals come for free from `spawn_projectile_visuals`.

2. **Auto Shot → NO cosmetic arrow.** Damage is instant via `combat_auto_attack`. A visual arrow arriving after HP already dropped would be confusing and misleading. Auto Shots simply have no projectile visual — this matches the understated treatment in the brainstorm.

3. **Trap visual → insert mesh directly onto Trap entity via `Added<Trap>`.** No separate marker component needed. Auto-despawns when trap triggers.

4. **TrapBurst → separate entity.** Spawned in `trap_system()` at the trap's position right before the trap entity despawns.

5. **IceBlockVisual break detection → poll for aura absence.** Each frame, check if target still has Incapacitate aura. When it disappears, despawn the ice block. Consistent with existing ShieldBubble pattern.

6. **Arrow cuboid orientation → long axis on Z.** Use `Cuboid::new(0.08, 0.08, 0.6)` so the existing `Quat::from_rotation_arc(Vec3::Z, direction)` rotates correctly.

7. **Boar Charge trail → filter with `With<Pet>`.** Distinguishes Boar charges from Warrior charges in the spawn system.

8. **Master's Call burst → on actual cleanse target.** Not always on the Hunter — the Bird can cleanse any ally.

9. **Concussive Shot impact → reuse DispelBurst.** No new ConcussiveImpact component. Spawn `DispelBurst` with Hunter gold color when Concussive Shot projectile hits.

10. **SlowZone visual → insert mesh directly onto SlowZone entity via `Added<SlowZone>`.** No separate marker component.

## Technical Approach

### Architecture

All effects follow the existing pattern documented in `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md`:

1. **Define component** in `components/visual.rs` (only for effects that need dedicated tracking)
2. **Spawn system** — `Added<Component>` filter creates mesh + material
3. **Update system** — per-frame animation (pulse, fade, position tracking)
4. **Cleanup system** — despawn when lifetime expires or source removed (merge with update for fire-and-forget effects)
5. **Register** in `states/mod.rs` in separate `.add_systems()` groups

**Critical rules:**
- `AlphaMode::Add` for all glow effects (except Ice Block which uses `AlphaMode::Blend`)
- `Without<T>` filter on second Transform queries to avoid runtime panics
- `try_insert()` not `insert()` for safe entity modification
- `Res<Time>` not `Res<Time<Real>>`
- `PlayMatchEntity` marker on all spawned entities
- Visual systems in `states/mod.rs` only — never in `systems.rs`
- Cache mesh handles via `Local<Option<Handle<Mesh>>>` to avoid per-entity GPU allocation
- Use `despawn()` not `despawn_recursive()` for childless visual entities

### New Components (4 total)

```rust
/// Expanding burst sphere when a trap triggers.
#[derive(Component)]
pub struct TrapBurst {
    pub trap_type: TrapType,
    pub lifetime: f32,
    pub initial_lifetime: f32,
}

/// Translucent ice cuboid around a Freezing Trap target.
#[derive(Component)]
pub struct IceBlockVisual {
    pub target: Entity,
}

/// Wind streak trail left behind during Disengage leap.
#[derive(Component)]
pub struct DisengageTrail {
    pub lifetime: f32,
    pub initial_lifetime: f32,
}

/// Speed streak trail behind Boar during charge.
#[derive(Component)]
pub struct ChargeTrail {
    pub lifetime: f32,
    pub initial_lifetime: f32,
}
```

**Not new components** (reuse existing or trigger off existing):
- Trap ground circle: mesh inserted directly onto `Trap` entity via `Added<Trap>`
- SlowZone visual: mesh inserted directly onto `SlowZone` entity via `Added<SlowZone>`
- Concussive impact: reuse `DispelBurst` with Hunter gold color
- Master's Call: reuse `DispelBurst` with Hunter gold color
- Arrow projectiles: handled by existing `spawn_projectile_visuals` with cuboid mesh branch

### Implementation Phases

#### Phase 1: Trap Visuals + Ice Block + SlowZone

**Trap ground circle** (trigger off `Added<Trap>`):

- `spawn_trap_visuals()` — detect `Added<Trap>`, insert flat cylinder mesh (r=1.5, h=0.05) at y=0.02 onto the Trap entity. Color based on trap_type: Frost=cyan `(0.3, 0.8, 1.0)`, Freezing=ice-white `(0.8, 0.9, 1.0)`. Cache mesh handle via `Local<Option<Handle<Mesh>>>`.
- `update_trap_visuals()` — read the `Trap` component's `arm_timer`. While arming: low alpha (0.15) with slow sine pulse. When armed (arm_timer <= 0): full alpha (0.4) with subtle shimmer.
- Cleanup: automatic — mesh is on the Trap entity which self-despawns on trigger.

**Trigger burst** (new `TrapBurst` component):

- `spawn_trap_burst_visuals()` — detect `Added<TrapBurst>`, create sphere mesh.
- `update_and_cleanup_trap_bursts()` — expand scale 1.0 → 3.0, fade alpha to 0. Duration: 0.3s. Despawn when lifetime <= 0. (Merged update+cleanup for fire-and-forget.)

**Ice Block** (new `IceBlockVisual` component):

- `spawn_ice_block_visuals()` — detect `Added<IceBlockVisual>`, create cuboid mesh (1.2 x 2.0 x 1.2). Material: ice-blue `(0.6, 0.85, 1.0)`, alpha 0.4, `AlphaMode::Blend`, emissive `(0.8, 1.2, 2.0)`.
- `update_ice_blocks()` — follow target position each frame. Uses `Without<IceBlockVisual>` on the target transform query.
- `cleanup_ice_blocks()` — despawn when the target no longer has the `Incapacitate` aura (poll `ActiveAuras`). Also handle target death.

**SlowZone visual** (trigger off `Added<SlowZone>`):

- `spawn_slow_zone_visuals()` — detect `Added<SlowZone>`, insert flat cylinder mesh (r=`FROST_TRAP_ZONE_RADIUS` = 8.0, h=0.03) at y=0.02 onto the SlowZone entity. Cyan `(0.3, 0.8, 1.0)`, alpha 0.2, `AlphaMode::Add`.
- `update_slow_zone_visuals()` — gentle alpha sine pulse (period ~2s). In last 2 seconds of `duration_remaining`, lerp alpha toward 0.
- Cleanup: automatic — mesh is on the SlowZone entity which self-despawns when duration expires.

**Spawning in `traps.rs`:**
- `TrapBurst` spawned as new entity at trap position right before `commands.entity(trap_entity).despawn()`
- `IceBlockVisual` spawned as new entity targeting the trapped combatant (Freezing Trap only)

**Files to modify:**
- `src/states/play_match/components/visual.rs` — add `TrapBurst`, `IceBlockVisual`
- `src/states/play_match/rendering/effects.rs` — add ~8 systems (2 trap circle + 2 trap burst + 3 ice block + 2 slow zone, with some merged)
- `src/states/play_match/traps.rs` — spawn `TrapBurst` + `IceBlockVisual` on trigger
- `src/states/mod.rs` — register new `.add_systems()` groups

#### Phase 2: Arrow Projectiles + Ability Conversions

**Convert Hunter instant abilities to real projectiles.** This is the biggest mechanical change — Arcane Shot, Concussive Shot, and Spider Web currently apply effects instantly. Converting them means effects are delayed by travel time but the architecture is clean.

**Important behavioral note:** This conversion delays damage/effects by ~0.5-0.8s (at 35-yard range with 45 speed). This is an intentional tradeoff for architectural cleanliness.

**abilities.ron changes:**
- `AimedShot`: Add `projectile_speed: Some(45.0)`, `projectile_visuals: Some((color: (1.0, 0.85, 0.4), emissive: (1.5, 1.3, 0.6)))`
- `ArcaneShot`: Add `projectile_speed: Some(45.0)`, `projectile_visuals: Some((color: (1.0, 0.8, 0.3), emissive: (1.5, 1.2, 0.5)))`
- `ConcussiveShot`: Add `projectile_speed: Some(40.0)`, `projectile_visuals: Some((color: (0.8, 0.6, 0.3), emissive: (0.0, 0.0, 0.0)))`
- `SpiderWeb`: Add `projectile_speed: Some(50.0)`, `projectile_visuals: Some((color: (0.95, 0.95, 0.9), emissive: (1.5, 1.5, 1.4)))` (50+ speed for defensive root role)

**AI function changes:**
- `try_arcane_shot()` in `hunter.rs`: Remove `QueuedInstantAttack` push. Instead, spawn `Projectile` entity.
- `try_concussive_shot()` in `hunter.rs`: Remove direct `AuraPending` spawn. Instead, spawn `Projectile` entity. The aura application happens in `process_projectile_hits`.
- `spider_ai()` in `pet_ai.rs`: Same conversion — Projectile instead of direct AuraPending.

**Arrow mesh** — modify `spawn_projectile_visuals()` in `projectiles.rs`:

Currently spawns `Sphere::new(0.3)` for all projectiles. Add match branches for Hunter abilities: use `Cuboid::new(0.08, 0.08, 0.6)` (long axis on Z). For `SpiderWeb`: use `Cuboid::new(0.06, 0.06, 0.4)`. Cache cuboid mesh handles via `Local`.

**Per-ability arrow colors** (via `projectile_visuals` in abilities.ron):
- `AimedShot` → gold `(1.0, 0.85, 0.4)`, emissive `(1.5, 1.3, 0.6)`
- `ArcaneShot` → golden-arcane `(1.0, 0.8, 0.3)`, emissive `(1.5, 1.2, 0.5)`
- `ConcussiveShot` → brown/gold `(0.8, 0.6, 0.3)`, no emissive
- `SpiderWeb` → white `(0.95, 0.95, 0.9)`, emissive `(1.5, 1.5, 1.4)`

**Concussive Shot impact** — when ConcussiveShot projectile hits, spawn `DispelBurst` with Hunter gold color `(1.0, 0.85, 0.3)`, emissive `(2.0, 1.7, 0.6)`. Add Hunter color branch in `spawn_dispel_visuals()`.

**Files to modify:**
- `assets/config/abilities.ron` — add `projectile_speed` + `projectile_visuals` to 4 abilities
- `src/states/play_match/class_ai/hunter.rs` — convert Arcane Shot + Concussive Shot to Projectile spawns
- `src/states/play_match/class_ai/pet_ai.rs` — convert Spider Web to Projectile spawn
- `src/states/play_match/projectiles.rs` — add cuboid mesh for Hunter abilities
- `src/states/play_match/rendering/effects.rs` — add Hunter color branch in `spawn_dispel_visuals()`
- `src/states/play_match/combat_core.rs` — spawn `DispelBurst` on Concussive Shot projectile hit

#### Phase 3: Movement Trails + Pet Abilities

**Disengage trail** (new `DisengageTrail` component):

- `spawn_disengage_trail()` — detect `Added<DisengagingState>` on combatants, spawn elongated cylinder at the Hunter's current position. Orient along `disengage.direction`. White/light-blue `(0.85, 0.9, 1.0)`, emissive `(1.5, 1.7, 2.0)`, y=0.5.
- `update_and_cleanup_disengage_trails()` — fade alpha over 0.5s. Despawn when lifetime <= 0. (Merged update+cleanup.)

**Boar Charge trail** (new `ChargeTrail` component):

- `spawn_charge_trail()` — detect `Added<ChargingState>` with `With<Pet>` filter (distinguishes from Warrior charges). Spawn elongated cylinder at start position. Brown/earthy `(0.6, 0.5, 0.3)`, emissive `(1.0, 0.8, 0.4)`.
- `update_and_cleanup_charge_trails()` — fade over 0.3s. Despawn when lifetime <= 0. (Merged update+cleanup.)

**Master's Call:**

- Reuse existing `DispelBurst` component. When Master's Call triggers in `pet_ai.rs`, spawn `DispelBurst` targeting the actual `cleanse_target` with `caster_class: CharacterClass::Hunter`. The Hunter color branch in `spawn_dispel_visuals()` (added in Phase 2) handles the gold color.

**Files to modify:**
- `src/states/play_match/components/visual.rs` — add `DisengageTrail`, `ChargeTrail`
- `src/states/play_match/rendering/effects.rs` — add 4 systems (2 disengage + 2 charge)
- `src/states/play_match/class_ai/pet_ai.rs` — spawn `DispelBurst` on Master's Call
- `src/states/mod.rs` — register new `.add_systems()` groups

#### Integration Testing

- Run headless matches: verify no panics from visual component spawning in headless mode
- Run graphical matches: verify all effects render correctly
- Test edge cases: match end during active effects, target dies with ice block, double trap trigger
- Verify `PlayMatchEntity` cleanup removes all visual entities on match end
- Check for query conflicts in new systems

**Test configurations:**
```json
{"team1":["Hunter"],"team2":["Warrior"]}
{"team1":["Hunter"],"team2":["Paladin"]}
{"team1":["Hunter","Hunter"],"team2":["Warrior","Priest"]}
```

## Acceptance Criteria

### Functional Requirements

- [ ] Trap ground circles visible with dim-to-bright arming transition
- [ ] Trap trigger produces expanding burst at trap location (0.3s duration)
- [ ] Freezing Trap shows translucent ice cuboid on trapped target
- [ ] Ice block tracks target position and despawns when Incapacitate breaks
- [ ] Frost Trap SlowZone shows large cyan ground disc with alpha pulse
- [ ] SlowZone disc fades out in last 2 seconds of duration
- [ ] Disengage leaves wind streak trail that fades over 0.5s
- [ ] Aimed Shot, Arcane Shot, Concussive Shot, Spider Web fire as real projectiles
- [ ] Hunter projectiles use elongated cuboid "arrow" mesh (long axis on Z)
- [ ] Arrow colors differ per ability (Aimed=gold, Arcane=golden, Concussive=brown)
- [ ] Concussive Shot produces golden DispelBurst on projectile hit
- [ ] Boar Charge leaves brown speed streak trail (Pet filter, not Warrior)
- [ ] Master's Call produces golden DispelBurst on actual cleanse target

### Non-Functional Requirements

- [ ] All glow effects use `AlphaMode::Add` (except Ice Block = `AlphaMode::Blend`)
- [ ] No query conflicts (all secondary Transform queries use `Without<T>`)
- [ ] All spawned entities have `PlayMatchEntity` marker for cleanup
- [ ] Headless mode runs without panics (88 tests pass)
- [ ] No visual systems registered in `systems.rs` (graphical-only in `states/mod.rs`)
- [ ] Match end during active effects cleans up all visual entities
- [ ] Mesh handles cached via `Local<Option<Handle<Mesh>>>` in spawn systems
- [ ] Use `despawn()` not `despawn_recursive()` for childless entities

## Dependencies & Risks

- **Bevy tuple size limit**: Each `.add_systems()` group has ~12 system maximum. New effects need separate groups.
- **Ice Block AlphaMode::Blend**: Only exception to the `Add` rule. May show Z-fighting if overlapping with other Blend effects — test carefully.
- **Arrow orientation**: Cuboid is oriented along Z-axis by default. Existing `Quat::from_rotation_arc(Vec3::Z, direction)` should rotate correctly, but needs verification.
- **Headless component spawning**: Visual marker components added in `traps.rs` and `pet_ai.rs` will spawn in headless mode. This is fine — the spawn systems won't run (not registered), and entities self-clean via `PlayMatchEntity`.
- **Instant→projectile timing change**: Converting Arcane Shot, Concussive Shot, Spider Web to real projectiles delays damage/effects by ~0.5-0.8s. This is an intentional tradeoff.

## References

### Internal References

- Visual effect pattern guide: `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md`
- Existing visual effects: `src/states/play_match/rendering/effects.rs`
- Visual components: `src/states/play_match/components/visual.rs`
- Projectile system: `src/states/play_match/projectiles.rs`
- System registration: `src/states/mod.rs` (line 181+)
- Trap system: `src/states/play_match/traps.rs`
- Hunter AI: `src/states/play_match/class_ai/hunter.rs`
- Pet AI: `src/states/play_match/class_ai/pet_ai.rs`
- Brainstorm: `docs/brainstorms/2026-02-24-hunter-visual-effects-brainstorm.md`
