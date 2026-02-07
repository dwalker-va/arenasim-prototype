---
title: "Adding a Visual Effect: Bevy Spawn/Update/Cleanup Pattern"
tags:
  - visual-effects
  - bevy-ecs
  - rendering
  - animation
  - dispel
  - healing
  - components
  - systems
category: implementation-patterns
module: states/play_match/rendering/effects
symptoms:
  - "Need to add a new spell animation or visual effect"
  - "Visual effect entity not appearing in game"
  - "Visual effect not cleaned up after match ends"
  - "Visual effect causes crash in headless mode"
  - "Query conflict panic when adding new visual system"
severity: low
date_documented: 2026-02-07
---

# Adding a Visual Effect: Bevy Spawn/Update/Cleanup Pattern

This document captures the established pattern for adding transient visual effects (spell animations, bursts, beams, etc.) to the arena. Use this as a checklist when adding any new visual effect.

## Overview

Every visual effect in the codebase follows a **three-system lifecycle**:

1. **Spawn system** — Detects new marker components via `Added<T>`, attaches mesh/material
2. **Update system** — Animates (fade, scale, follow target) each frame
3. **Cleanup system** — Despawns when lifetime expires

This pattern has been applied to: `SpellImpactEffect`, `ShieldBubble`, `FlameParticle`, `DrainLifeBeam`, `HealingLightColumn`, and `DispelBurst`.

## The Pattern (5 Steps)

### Step 1: Define the Component

**File:** `src/states/play_match/components/mod.rs`

```rust
#[derive(Component)]
pub struct MyEffect {
    pub target: Entity,              // Entity to follow
    pub caster_class: CharacterClass, // For color differentiation (if needed)
    pub lifetime: f32,               // Decremented each frame
    pub initial_lifetime: f32,       // For fade progress calculation
}
```

Key fields:
- `target: Entity` — the entity the effect follows (position tracking)
- `lifetime` + `initial_lifetime` — used to compute fade progress as `lifetime / initial_lifetime`
- Additional fields as needed (e.g., `healer_class`, `spell_school`)

### Step 2: Spawn the Component in Combat Logic

**File:** Wherever the triggering event occurs (e.g., `combat_core.rs`, `effects/dispels.rs`)

```rust
commands.spawn((
    MyEffect {
        target: target_entity,
        caster_class: some_class,
        lifetime: 0.5,
        initial_lifetime: 0.5,
    },
    PlayMatchEntity, // CRITICAL: ensures cleanup on match exit
));
```

**Important:** Only spawn the data component here — no mesh, no material. The rendering system handles that.

### Step 3: Add Three Visual Systems

**File:** `src/states/play_match/rendering/effects.rs`

```rust
// Color helper (private)
fn my_effect_colors(class: CharacterClass) -> (Color, LinearRgba) {
    match class {
        CharacterClass::Priest => (
            Color::srgba(0.85, 0.85, 1.0, 0.5),
            LinearRgba::new(2.0, 2.0, 2.8, 1.0), // Emissive 2x+ for glow
        ),
        _ => (
            Color::srgba(0.9, 0.9, 1.0, 0.5),
            LinearRgba::new(2.0, 2.0, 2.5, 1.0),
        ),
    }
}

// SPAWN: Detect new components, attach mesh/material
pub fn spawn_my_effect_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_effects: Query<(Entity, &MyEffect), (Added<MyEffect>, Without<Mesh3d>)>,
    transforms: Query<&Transform>,
) {
    for (entity, effect) in new_effects.iter() {
        let Ok(target_transform) = transforms.get(effect.target) else {
            continue; // Target already despawned
        };
        // ... create mesh/material, use try_insert()
        commands.entity(entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position),
        ));
    }
}

// UPDATE: Animate each frame
pub fn update_my_effects(
    time: Res<Time>,  // NOT Time<Real> — match the existing convention
    mut effects: Query<(&mut MyEffect, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, Without<MyEffect>>, // Without<T> prevents query conflict!
) {
    for (mut effect, mut transform, material_handle) in effects.iter_mut() {
        effect.lifetime -= time.delta_secs();
        // Follow target, fade alpha, scale, etc.
    }
}

// CLEANUP: Remove expired effects
pub fn cleanup_expired_my_effects(
    mut commands: Commands,
    effects: Query<(Entity, &MyEffect)>,
) {
    for (entity, effect) in effects.iter() {
        if effect.lifetime <= 0.0 {
            commands.entity(entity).despawn_recursive();
        }
    }
}
```

### Step 4: Register Systems in states/mod.rs

**File:** `src/states/mod.rs`

```rust
// My effect visual systems (separate group to avoid tuple size limits)
.add_systems(
    Update,
    (
        play_match::spawn_my_effect_visuals,
        play_match::update_my_effects,
        play_match::cleanup_expired_my_effects,
    )
        .run_if(in_state(GameState::PlayMatch)),
)
```

**Critical:** Register in `states/mod.rs` (graphical only), NOT in `systems.rs` (shared with headless).

### Step 5: Verify Re-exports

The `pub use rendering::*` chain in `play_match/mod.rs` should automatically re-export new `pub fn` systems. Verify by building. If it fails, add explicit re-exports.

## Gotchas and Lessons Learned

### 1. Use `AlphaMode::Add`, Not `Blend`

`AlphaMode::Blend` causes Z-fighting flicker on overlapping translucent meshes. `AlphaMode::Add` produces clean additive blending where effects stack visually.

### 2. Use `Without<T>` on the Second Transform Query

The update system needs two `Transform` queries — one mutable (for the effect entity) and one read-only (for the target). Without a `Without<T>` filter, Bevy panics at runtime due to query conflict:

```rust
// CORRECT:
mut effects: Query<(&mut MyEffect, &mut Transform, ...)>,
transforms: Query<&Transform, Without<MyEffect>>,

// WRONG (runtime panic):
mut effects: Query<(&mut MyEffect, &mut Transform, ...)>,
transforms: Query<&Transform>,  // Overlaps with first query!
```

### 3. Use `try_insert()`, Not `insert()`

In the spawn system, the entity might be despawned between the query iteration and command application. `try_insert()` handles this gracefully; `insert()` would panic.

### 4. Use `Res<Time>`, Not `Res<Time<Real>>`

All visual systems in `effects.rs` use `Res<Time>`. Using `Time<Real>` would cause the animation to ignore simulation speed changes, creating visual inconsistency.

### 5. Always Include `PlayMatchEntity` Marker

Without it, the effect entity persists after the match ends. The `cleanup_play_match` system despawns all entities with this marker on state exit.

### 6. Headless Mode: Components Spawn, Visuals Don't Run

Combat systems (in `systems.rs`) run in both modes. Visual systems (in `states/mod.rs`) only run in graphical mode. This means marker component entities spawn in headless mode but never get meshes/materials attached and never get cleaned up (they leak until the process exits). This is a known, accepted trade-off — the entity count is bounded by match duration.

### 7. Name Components After Their Visual Shape

Convention: `HealingLightColumn`, `ShieldBubble`, `DrainLifeBeam`, `DispelBurst` — not `HealingEffect` or `DispelAnimation`. The name should describe what the spectator sees.

### 8. Emissive Values Need 2x+ Scaling

`LinearRgba::new(1.0, 1.0, 1.0, 1.0)` produces barely visible glow. Use 2x-4x scaled values for noticeable emissive effects.

### 9. Position at Chest Height

Use `target_transform.translation + Vec3::Y * 1.0` for effects centered on combatants. This places them at approximately chest height rather than at ground level.

### 10. Separate `.add_systems()` Groups

Bevy has a compile-time tuple size limit for system groups. Each visual effect type gets its own `.add_systems()` block with a comment explaining why.

## Color Reference

| Class/School | Base Color | Emissive | Used By |
|-------------|-----------|----------|---------|
| Priest (healing) | `srgba(1.0, 1.0, 0.9, 0.35)` | `LinearRgba(2.8, 2.8, 2.4, 1.0)` | HealingLightColumn |
| Paladin (healing) | `srgba(1.0, 0.9, 0.6, 0.35)` | `LinearRgba(2.5, 2.0, 1.0, 1.0)` | HealingLightColumn |
| Priest (dispel) | `srgba(0.85, 0.85, 1.0, 0.5)` | `LinearRgba(2.0, 2.0, 2.8, 1.0)` | DispelBurst |
| Paladin (dispel) | `srgba(1.0, 0.9, 0.6, 0.5)` | `LinearRgba(2.5, 2.0, 1.0, 1.0)` | DispelBurst |
| Frost (shield) | `srgba(0.4, 0.7, 1.0, 0.25)` | varies | ShieldBubble |
| Shadow (impact) | `srgba(0.5, 0.2, 0.8, 0.8)` | purple | SpellImpactEffect |

## File Checklist

When adding a new visual effect, touch these files:

- [ ] `components/mod.rs` — New component struct
- [ ] Combat system file — Spawn the component (e.g., `combat_core.rs`, `effects/dispels.rs`)
- [ ] `rendering/effects.rs` — Three systems + color helper
- [ ] `states/mod.rs` — System registration (new `.add_systems()` group)
- [ ] Verify build: `cargo build --release`
- [ ] Headless test: `cargo run --release -- --headless /tmp/test.json`

## Cross-References

- [Adding a New Class: Paladin](adding-new-class-paladin.md) — Documents the dispel system and pending component pattern
- [CLAUDE.md: Adding a New Ability](../../CLAUDE.md) — Full ability addition checklist
- Commit `56860b1` — HealingLightColumn implementation (the original pattern)
- Commit `070891c` — DispelBurst implementation (second application of the pattern)
