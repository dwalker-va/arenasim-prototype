---
title: "feat: Add dispel spell animation for Priest and Paladin"
type: feat
date: 2026-02-07
---

# feat: Add Dispel Spell Animation

## Overview

Add a visual animation for dispel spells (Priest's Dispel Magic, Paladin's Cleanse) inspired by WoW's dispel effect. When a dispel successfully removes an aura, a burst of light expands outward from the target — white/silver for Priest, golden for Paladin.

**Affected spells:**
- Dispel Magic (Priest) — white/silver burst
- Cleanse (Paladin) — golden burst

## Problem Statement / Motivation

Dispels are match-defining abilities — removing a Polymorph or Fear can swing the outcome. Currently, dispels are visually silent. The only feedback is the aura icon disappearing from the health bar and a combat log entry. This makes it difficult for spectators to:
- Notice when a dispel happens
- Appreciate the impact of Priest/Paladin utility
- Distinguish Priest dispels from Paladin cleanses

Healing spells already have a light column animation (commit 56860b1). Dispels are the last major instant-cast ability type without visual feedback.

## Proposed Solution

Add a `DispelBurst` component and three visual systems following the established pattern from `HealingLightColumn`:

1. **Component** — `DispelBurst` tracks target, caster class, and lifetime
2. **Spawn system** — Detects new bursts, creates expanding sphere mesh with emissive material
3. **Update system** — Expands sphere outward, fades over lifetime, follows target
4. **Cleanup system** — Despawns when lifetime expires

The WoW dispel animation is a burst of sparkles/light radiating outward from the target. We'll implement this as an **expanding sphere** (matching the `SpellImpactEffect` pattern) with additive blending for a clean glow effect.

## Technical Considerations

### Architecture

Follow existing visual effect patterns from `rendering/effects.rs`:
- Use `Added<DispelBurst>` query pattern for spawning visuals
- Use `AlphaMode::Add` for additive blending (prevents Z-fighting, stacks well)
- Use `Res<Time>` for animation timing (matches all other visual systems in effects.rs)
- Use `try_insert()` for safe entity modification

### Data Model Change: Extend DispelPending

`DispelPending` (at `class_ai/priest.rs:602`) currently lacks caster class information. To color the dispel animation by class, add a `caster_class` field:

```rust
pub struct DispelPending {
    pub target: Entity,
    pub log_prefix: &'static str,
    pub caster_class: CharacterClass,  // NEW: for visual effect coloring
}
```

Update both spawn sites:
- `class_ai/priest.rs` (~line 560) — pass `CharacterClass::Priest`
- `class_ai/paladin.rs` (~line 756) — pass `CharacterClass::Paladin`

### Visual Specifications

| Property | Value | Rationale |
|----------|-------|-----------|
| Mesh | `Sphere::new(0.3)` (initial) | Small starting size, expands outward |
| Final scale | ~3.0x | Expands to ~0.9 radius over lifetime |
| Alpha Mode | `AlphaMode::Add` | Additive blending for glow, stacks cleanly |
| Lifetime | 0.5 seconds | Snappy burst feel — shorter than healing column (0.8s) |
| Position | Target position + `Vec3::Y * 1.0` | Chest height, same as healing column |
| Animation | Scale up + fade out | Sphere grows while becoming transparent |

#### Class-Specific Colors

| Class | Base Color | Emissive | Description |
|-------|------------|----------|-------------|
| **Priest** | `Color::srgba(0.85, 0.85, 1.0, 0.5)` | `LinearRgba::new(2.0, 2.0, 2.8, 1.0)` | White/silver with slight blue tint (cleansing/purifying feel) |
| **Paladin** | `Color::srgba(1.0, 0.9, 0.6, 0.5)` | `LinearRgba::new(2.5, 2.0, 1.0, 1.0)` | Golden (reuses Paladin healing color for consistency) |

### Spawn Location

Spawn the `DispelBurst` inside `process_dispels()` (`effects/dispels.rs:36-53`) after the aura is successfully removed. This ensures the visual only appears when a dispel actually removes an aura.

### Edge Cases

| Case | Behavior |
|------|----------|
| Failed dispel (no auras to remove) | No visual effect — animation only spawns on successful removal |
| Headless mode | Component spawns in `process_dispels` but visual systems never run (they're registered only in `states/mod.rs`, not `systems.rs`) |

## Acceptance Criteria

### Functional Requirements

- [x] White/silver expanding sphere appears when Priest's Dispel Magic successfully removes an aura
- [x] Golden expanding sphere appears when Paladin's Cleanse successfully removes an aura
- [x] Priest and Paladin dispel colors are visually distinct from each other
- [x] Sphere expands outward and fades over 0.5 second lifetime
- [x] Sphere follows the dispel target's position
- [x] No visual effect appears when a dispel fails to remove an aura
- [x] `DispelPending` struct extended with `caster_class: CharacterClass`
- [x] Component named `DispelBurst` (describes shape, consistent with `HealingLightColumn`, `ShieldBubble`)

### Non-Functional Requirements

- [x] Headless simulation runs without errors
- [x] No orphaned entities after match ends (`PlayMatchEntity` marker)
- [x] Dispel animation visually distinct from healing light column and shield bubble
- [x] `cargo build --release` compiles cleanly

## Implementation

### Phase 1: Extend DispelPending with Caster Class

**File: `src/states/play_match/class_ai/priest.rs`** (~line 602)

```rust
#[derive(bevy::prelude::Component)]
pub struct DispelPending {
    pub target: Entity,
    pub log_prefix: &'static str,
    pub caster_class: match_config::CharacterClass,  // NEW
}
```

**File: `src/states/play_match/class_ai/priest.rs`** (DispelPending spawn site, ~line 560)

Add `caster_class: match_config::CharacterClass::Priest` to the struct literal.

**File: `src/states/play_match/class_ai/paladin.rs`** (DispelPending spawn site, ~line 756)

Add `caster_class: match_config::CharacterClass::Paladin` to the struct literal.

### Phase 2: Component Definition

**File: `src/states/play_match/components/mod.rs`** (after `HealingLightColumn`, ~line 1002)

```rust
/// Visual effect for dispel spells - an expanding sphere burst at the target
#[derive(Component)]
pub struct DispelBurst {
    pub target: Entity,
    pub caster_class: match_config::CharacterClass,
    pub lifetime: f32,
    pub initial_lifetime: f32,
}
```

### Phase 2b: Spawn DispelBurst in process_dispels

**File: `src/states/play_match/effects/dispels.rs`** (inside the `if !dispellable_indices.is_empty()` block, after the log entry at ~line 53)

```rust
// Spawn dispel visual effect
commands.spawn((
    DispelBurst {
        target: pending.target,
        caster_class: pending.caster_class,
        lifetime: 0.5,
        initial_lifetime: 0.5,
    },
    PlayMatchEntity,
));
```

### Phase 3: Visual Systems

**File: `src/states/play_match/rendering/effects.rs`** (after healing light column systems)

```rust
/// Returns (base_color, emissive) for dispel burst based on caster class
fn dispel_burst_colors(class: CharacterClass) -> (Color, LinearRgba) {
    match class {
        CharacterClass::Priest => (
            // White/silver with slight blue tint
            Color::srgba(0.85, 0.85, 1.0, 0.5),
            LinearRgba::new(2.0, 2.0, 2.8, 1.0),
        ),
        CharacterClass::Paladin => (
            // Golden (matches Paladin healing color)
            Color::srgba(1.0, 0.9, 0.6, 0.5),
            LinearRgba::new(2.5, 2.0, 1.0, 1.0),
        ),
        _ => (
            Color::srgba(0.9, 0.9, 1.0, 0.5),
            LinearRgba::new(2.0, 2.0, 2.5, 1.0),
        ),
    }
}

/// Spawns visual mesh for dispel effects
pub fn spawn_dispel_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_effects: Query<(Entity, &DispelBurst), (Added<DispelBurst>, Without<Mesh3d>)>,
    transforms: Query<&Transform>,
) {
    for (effect_entity, effect) in new_effects.iter() {
        let Ok(target_transform) = transforms.get(effect.target) else {
            continue;
        };

        let (base_color, emissive) = dispel_burst_colors(effect.caster_class);

        let mesh = meshes.add(Sphere::new(0.3));
        let material = materials.add(StandardMaterial {
            base_color,
            emissive,
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        let position = target_transform.translation + Vec3::Y * 1.0;

        commands.entity(effect_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position),
        ));
    }
}

/// Updates dispel bursts: expand sphere and fade
pub fn update_dispel_bursts(
    time: Res<Time>,
    mut effects: Query<(&mut DispelBurst, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, Without<DispelBurst>>,
) {
    for (mut effect, mut effect_transform, material_handle) in effects.iter_mut() {
        effect.lifetime -= time.delta_secs();

        // Follow target position
        if let Ok(target_transform) = transforms.get(effect.target) {
            effect_transform.translation = target_transform.translation + Vec3::Y * 1.0;
        }

        // Progress: 1.0 (just spawned) → 0.0 (expired)
        let progress = (effect.lifetime / effect.initial_lifetime).max(0.0);

        // Scale up as it expands (1.0 → 3.0)
        let scale = 1.0 + (1.0 - progress) * 2.0;
        effect_transform.scale = Vec3::splat(scale);

        // Fade out
        let (base_color, emissive) = dispel_burst_colors(effect.caster_class);
        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.base_color = base_color.with_alpha(base_color.alpha() * progress);
            material.emissive = LinearRgba::new(
                emissive.red * progress,
                emissive.green * progress,
                emissive.blue * progress,
                1.0,
            );
        }
    }
}

/// Removes expired dispel effects
pub fn cleanup_expired_dispel_bursts(
    mut commands: Commands,
    effects: Query<(Entity, &DispelBurst)>,
) {
    for (entity, effect) in effects.iter() {
        if effect.lifetime <= 0.0 {
            commands.entity(entity).despawn_recursive();
        }
    }
}
```

### Phase 4: System Registration

**File: `src/states/mod.rs`** (add a new `.add_systems()` group after the healing light column group, ~line 190)

```rust
// Dispel visual effects
.add_systems(
    Update,
    (
        play_match::spawn_dispel_visuals,
        play_match::update_dispel_bursts,
        play_match::cleanup_expired_dispel_bursts,
    )
        .run_if(in_state(GameState::PlayMatch)),
)
```

*Note: Functions are already `pub`, and `play_match/mod.rs` uses `pub use rendering::*`, so no additional re-exports should be needed.*

## Test Plan

1. **Headless smoke test** — verify no crashes:
   ```bash
   echo '{"team1":["Priest","Warrior"],"team2":["Mage","Warlock"]}' > /tmp/test.json
   cargo run --release -- --headless /tmp/test.json
   ```
   Mage has Polymorph and Warlock has Fear/Corruption — gives the Priest targets to dispel.

2. **Graphical test — Priest Dispel Magic**:
   - Run `Priest + Warrior` vs `Mage + Warlock`
   - Observe white/silver expanding sphere when Priest dispels Polymorph, Fear, or DoTs
   - Sphere should expand outward and fade over ~0.5s
   - Sphere should appear at the dispel target's position

3. **Graphical test — Paladin Cleanse**:
   - Run `Paladin + Warrior` vs `Mage + Warlock`
   - Observe golden expanding sphere when Paladin cleanses
   - Color should be visibly different from Priest's white/silver

4. **Visual distinction test**:
   - Run `Priest + Paladin` vs `Mage + Warlock`
   - Both healers should dispel with distinct colors
   - Dispel effect should be visually distinct from healing light columns

5. **Edge case verification**:
   - Target dies during dispel animation — sphere completes without crash
   - Multiple dispels on same target — spheres stack cleanly (additive blending)

## References & Research

### Internal References

- HealingLightColumn pattern: `rendering/effects.rs:811-888` (direct precedent)
- SpellImpactEffect expanding sphere: `rendering/effects.rs:185-256`
- Dispel processing: `effects/dispels.rs:17-68`
- DispelPending struct: `class_ai/priest.rs:601-607`
- Priest dispel AI: `class_ai/priest.rs:456-560`
- Paladin cleanse AI: `class_ai/paladin.rs:660-756`
- System registration pattern: `states/mod.rs:181-189`
- Visual component definitions: `components/mod.rs:992-1002`

### External References

- WoW Dispel Magic: [Wowhead Classic](https://www.wowhead.com/classic/spell=527/dispel-magic)
- WoW dispel animation: white/golden holy light burst on target, pink/purple spiral on icon

### Key Learnings Applied

- Use `AlphaMode::Add` instead of `Blend` to prevent Z-fighting flicker
- Use `try_insert()` for safe entity modification
- Use `Res<Time>` for visual animations (matches existing effects convention)
- Emissive values should be 2x+ scaled for visible glow
- Position effects at chest height (Y offset ~1.0)
- Visual systems registered only in `states/mod.rs` (not `systems.rs`) for headless compatibility
- Follow spawn → update → cleanup 3-system pattern
