---
title: "feat: Add healing spell light column animation"
type: feat
date: 2026-02-03
status: completed
---

# feat: Add Healing Spell Light Column Animation

## Overview

Add a translucent golden column of light animation that appears when healing spells land on a target, providing clear visual feedback for healing events. The column causes the heal target to appear to glow.

**Affected spells:**
- Flash of Light (Paladin) - golden
- Holy Light (Paladin) - golden
- Flash Heal (Priest) - white-gold

*Note: Holy Shock will receive a unique animation in a future update.*

## Problem Statement / Motivation

Currently, healing spells have no visual feedback beyond floating combat text. Damage spells have projectiles and impact effects, but heals are visually silent. This makes it harder to:
- Recognize when a heal lands
- Distinguish healing from other combat events
- Create a satisfying visual experience for healing classes

## Proposed Solution

Add a `HealingLightColumn` component and three visual systems following the existing pattern established by `SpellImpactEffect` and `ShieldBubble`:

1. **Component** - `HealingLightColumn` tracks target, lifetime, and spell school
2. **Spawn system** - Detects new columns, creates cylinder mesh with golden emissive material
3. **Update system** - Tracks target position, animates fade
4. **Cleanup system** - Despawns when lifetime expires

## Technical Considerations

### Architecture

Follow existing visual effect patterns from `rendering/effects.rs`:
- Use `Added<HealingLightColumn>` query pattern for spawning visuals
- Use `AlphaMode::Add` (not Blend) to prevent Z-fighting flicker
- Use `Time<Real>` for animation timing (works during pause)
- Use `try_insert()` for safe entity modification

### Visual Specifications

| Property | Value | Rationale |
|----------|-------|-----------|
| Mesh | `Cylinder::new(1.0, 3.5)` | Radius 1.0, height 3.5 (combatant fits inside with clearance) |
| Alpha Mode | `AlphaMode::Add` | Additive blending for glow effect |
| Lifetime | 0.8 seconds | Quick but visible |
| Position | Target position + `Vec3::Y * 1.0` | Centered on combatant |

#### Class-Specific Colors

| Class | Base Color | Emissive | Description |
|-------|------------|----------|-------------|
| **Priest** | `Color::srgba(1.0, 1.0, 0.9, 0.35)` | `LinearRgba::new(2.8, 2.8, 2.4, 1.0)` | White-gold (brighter, less yellow) |
| **Paladin** | `Color::srgba(1.0, 0.9, 0.6, 0.35)` | `LinearRgba::new(2.5, 2.0, 1.0, 1.0)` | Golden (warmer, more yellow) |

### Spawn Location

Cast-time heals are processed in `combat_core.rs:1388-1450` in the `def.is_heal()` branch. This is where the `HealingLightColumn` component should be spawned.

*Note: Holy Shock uses a separate system and will get its own unique animation later.*

### Edge Cases

| Case | Behavior |
|------|----------|
| Target dies during animation | Column continues fade at last position, despawns normally |
| Overheal (target at full HP) | Still shows column (heal was cast) |
| Self-heal | Column appears at healer's own position |
| Multiple simultaneous heals | Multiple columns stack (additive blending handles this well) |
| Headless mode | Component spawns but visual systems don't run (no mesh/material access) |
| Drain Life | No column - it has its own beam visual already |

## Acceptance Criteria

### Functional Requirements

- [x] Golden light column appears when Flash of Light (Paladin) cast completes
- [x] Golden light column appears when Holy Light (Paladin) cast completes
- [x] White-gold light column appears when Flash Heal (Priest) cast completes
- [x] Paladin heals are visibly more golden/yellow than Priest heals
- [x] Priest heals are visibly whiter than Paladin heals
- [x] Column appears at heal target's position (not caster's)
- [x] Column fades out over 0.8 second lifetime
- [x] Column is despawned after lifetime expires
- [x] Self-heals show column at healer's own position

### Non-Functional Requirements

- [x] Headless simulation runs without errors
- [x] No orphaned entities after match ends
- [x] Column visually distinct from shield bubble effect

## Implementation

### Phase 1: Component Definition

Add to `src/states/play_match/components/mod.rs`:

```rust
// components/mod.rs (near line 943, after ShieldBubble)

/// Visual effect for healing spells - a column of light at the target
#[derive(Component)]
pub struct HealingLightColumn {
    pub target: Entity,
    pub healer_class: CharacterClass,
    pub lifetime: f32,
    pub initial_lifetime: f32,
}
```

### Phase 2: Spawn Logic in Combat Core

Add to `src/states/play_match/combat_core.rs` in the `def.is_heal()` block (~line 1430):

```rust
// combat_core.rs (after FloatingCombatText spawn, inside def.is_heal() block)

// Spawn healing light column visual
commands.spawn((
    HealingLightColumn {
        target: target_entity,
        healer_class: caster.class,
        lifetime: 0.8,
        initial_lifetime: 0.8,
    },
    PlayMatchEntity,
));
```

### Phase 3: Visual Systems

Add to `src/states/play_match/rendering/effects.rs`:

```rust
// rendering/effects.rs

/// Returns (base_color, emissive) for healing light based on healer class
fn healing_light_colors(class: CharacterClass) -> (Color, LinearRgba) {
    match class {
        CharacterClass::Priest => (
            // White-gold: brighter, less yellow
            Color::srgba(1.0, 1.0, 0.9, 0.35),
            LinearRgba::new(2.8, 2.8, 2.4, 1.0),
        ),
        CharacterClass::Paladin => (
            // Golden: warmer, more yellow
            Color::srgba(1.0, 0.9, 0.6, 0.35),
            LinearRgba::new(2.5, 2.0, 1.0, 1.0),
        ),
        _ => (
            // Fallback golden
            Color::srgba(1.0, 0.95, 0.7, 0.35),
            LinearRgba::new(2.5, 2.2, 1.2, 1.0),
        ),
    }
}

/// Spawns visual mesh for healing light columns
pub fn spawn_healing_light_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_columns: Query<(Entity, &HealingLightColumn), (Added<HealingLightColumn>, Without<Mesh3d>)>,
    transforms: Query<&Transform>,
) {
    for (column_entity, column) in new_columns.iter() {
        let Ok(target_transform) = transforms.get(column.target) else {
            continue;
        };

        let (base_color, emissive) = healing_light_colors(column.healer_class);

        let mesh = meshes.add(Cylinder::new(0.4, 3.5));
        let material = materials.add(StandardMaterial {
            base_color,
            emissive,
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        let position = target_transform.translation + Vec3::Y * 1.0;

        commands.entity(column_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position),
        ));
    }
}

/// Updates healing light column position and fade
pub fn update_healing_light_columns(
    time: Res<Time<Real>>,
    mut columns: Query<(&mut HealingLightColumn, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, Without<HealingLightColumn>>,
) {
    for (mut column, mut column_transform, material_handle) in columns.iter_mut() {
        column.lifetime -= time.delta_secs();

        // Update position to follow target
        if let Ok(target_transform) = transforms.get(column.target) {
            column_transform.translation = target_transform.translation + Vec3::Y * 1.0;
        }

        // Fade based on remaining lifetime
        let progress = (column.lifetime / column.initial_lifetime).max(0.0);
        let (base_color, emissive) = healing_light_colors(column.healer_class);

        if let Some(material) = materials.get_mut(&material_handle.0) {
            // Scale alpha by progress for fade
            let faded_base = base_color.with_alpha(base_color.alpha() * progress);
            material.base_color = faded_base;
            material.emissive = LinearRgba::new(
                emissive.red * progress,
                emissive.green * progress,
                emissive.blue * progress,
                1.0,
            );
        }
    }
}

/// Removes expired healing light columns
pub fn cleanup_expired_healing_lights(
    mut commands: Commands,
    columns: Query<(Entity, &HealingLightColumn)>,
) {
    for (entity, column) in columns.iter() {
        if column.lifetime <= 0.0 {
            commands.entity(entity).despawn_recursive();
        }
    }
}
```

### Phase 4: System Registration

Add to `src/states/mod.rs` in the visual effects systems group (~line 176):

```rust
// states/mod.rs (in the Update systems group)

play_match::spawn_healing_light_visuals,
play_match::update_healing_light_columns,
play_match::cleanup_expired_healing_lights,
```

### Phase 5: Re-exports

Add to `src/states/play_match/mod.rs`:

```rust
pub use rendering::effects::{
    spawn_healing_light_visuals,
    update_healing_light_columns,
    cleanup_expired_healing_lights,
};
```

## Test Plan

1. **Run headless simulation** with Priest vs enemy to verify no crashes:
   ```bash
   echo '{"team1":["Priest"],"team2":["Warrior"]}' > /tmp/test.json
   cargo run --release -- --headless /tmp/test.json
   ```

2. **Run graphical client** and observe Priest heals:
   - Priest Flash Heal shows white-gold column (brighter, less yellow)
   - Column fades smoothly over ~0.8 seconds
   - Column follows target if they move during animation

3. **Run graphical client** and observe Paladin heals:
   - Paladin Flash of Light shows golden column (warmer, more yellow)
   - Paladin Holy Light shows golden column
   - Color is noticeably different from Priest heals

4. **Edge case verification**:
   - Kill heal target during animation - column should complete fade without crash
   - Full HP target receives heal - column still appears
   - Self-heals show column at healer's own position

## References & Research

### Internal References

- Shield bubble pattern: `rendering/effects.rs:356-456`
- Spell impact effect pattern: `rendering/effects.rs:185-256`
- Drain Life beam pattern: `rendering/effects.rs:568-614`
- Healing processing: `combat_core.rs:1388-1450`
- Visual component definitions: `components/mod.rs:893-943`

### Key Learnings Applied

- Use `AlphaMode::Add` instead of `Blend` to prevent Z-fighting flicker
- Use `try_insert()` for safe entity modification
- Use `Time<Real>` for visual animations (works during pause)
- Emissive values should be 2x+ scaled for visible glow
- Position effects at chest height (Y offset ~1.0)
