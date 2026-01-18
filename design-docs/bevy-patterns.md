# Bevy/Rust Patterns

Learnings from ArenaSim development. Reference when debugging issues or implementing new systems.

---

## ECS Patterns

### States for Scene Management
```rust
#[derive(States, Default, Clone, Eq, PartialEq, Debug, Hash)]
pub enum GameState {
    #[default]
    MainMenu,
    ConfigureMatch,
    PlayMatch,
    Results,
    Options,
}
```
- Use `OnEnter(state)` and `OnExit(state)` for setup/cleanup
- Systems run only in their associated state via `in_state(GameState::PlayMatch)`

### Events for Combat Actions
- Damage, healing, CC should flow through events
- Allows multiple systems to react to same event
- Decouples combat logic from effects

### Resources for Global Data
- `CombatLog` - match history and statistics
- `MatchConfig` - team composition
- `Time<Virtual>` - simulation speed control
- `AbilityDefinitions` - loaded ability configs

### Marker Components for Entity Queries
```rust
#[derive(Component)]
pub struct Combatant { /* ... */ }

#[derive(Component)]
pub struct MainMenuEntity;  // For cleanup on scene exit
```
- Marker components enable precise queries
- Cleanup: `despawn_recursive` all entities with marker on `OnExit`

---

## Query Patterns

### Without<T> Filters
Prevent query conflicts when multiple queries access same component:
```rust
fn system_a(query: Query<&mut Transform, With<Combatant>>) { }
fn system_b(query: Query<&mut Transform, (With<Projectile>, Without<Combatant>)>) { }
```

### Option<&Component> for Optional Components
Useful for headless mode where visual components may not exist:
```rust
fn system(query: Query<(&Combatant, Option<&Transform>)>) {
    for (combatant, maybe_transform) in &query {
        if let Some(transform) = maybe_transform {
            // Use transform
        }
    }
}
```

### Avoid Overly Restrictive Filters
```rust
// BAD: Fails after despawn_descendants() removes Children
Query<Entity, (With<MainContentArea>, With<Children>)>

// GOOD: Only filter by marker component
Query<Entity, With<MainContentArea>>
```

---

## UI Patterns

### Immediate Mode (egui) for Menus
- Declarative: "show current state"
- No sync bugs - UI always matches data
- Much less code than retained mode
- Use for: MainMenu, ConfigureMatch, Options, Results

### Bevy UI for In-Game HUD
- World-space integration (health bars above 3D entities)
- Use for: floating combat text, cast bars, status labels

### egui Best Practices
```rust
// Unique IDs for multiple similar widgets
CollapsingHeader::new("Details")
    .id_salt(combatant_entity)  // Prevent ID clashes
    .show(ui, |ui| { /* ... */ });

// Safe context access
if let Some(ctx) = egui_ctx.try_ctx_mut() {
    // Use ctx
}
```

---

## Time Handling

### Virtual Time for Simulation
```rust
// Pause/slow/speed up combat
time.set_relative_speed(0.0);  // Paused
time.set_relative_speed(2.0);  // 2x speed

// Systems use virtual time
fn combat_system(time: Res<Time<Virtual>>) {
    let delta = time.delta_seconds();
}
```

### Real Time for Animations During Pause
```rust
fn animation_system(time: Res<Time<Real>>) {
    // Runs at real speed even when simulation paused
    // Use for: camera controls, UI animations, pulse effects
}
```

---

## Common Pitfalls & Solutions

### Change Detection

**Problem**: `is_changed()` consumed per-frame
```rust
// Frame N: config changes, is_changed() = true
// Frame N+1: is_changed() = false (already consumed)
```

**Solution**: Use `.chain()` for system ordering dependencies
```rust
app.add_systems(Update, (
    handle_buttons,
    update_ui,
    handle_esc,
).chain());  // Guarantees order: buttons → UI → ESC
```

### Query Filter Failures

**Problem**: `With<Children>` filter fails after `despawn_descendants()`

**Solution**: Only filter by marker component, not structural components

### Headless Mode Compatibility

**Problem**: Projectiles need `Transform` even without graphics

**Solution**: Add gameplay-critical components in core systems, not visual systems:
```rust
// In process_casting (runs in both modes)
commands.spawn((
    Projectile { /* ... */ },
    Transform::from_translation(start_pos),  // Required for movement!
));

// In spawn_projectile_visuals (graphical only)
// Add mesh, materials, etc.
```

### Entity Despawn Safety

**Problem**: Entity may be despawned before system runs

**Solution**: Use `try_insert()` instead of `insert()`:
```rust
// BAD: Panics if entity despawned
commands.entity(entity).insert(Component);

// GOOD: Silent no-op if entity despawned
commands.entity(entity).try_insert(Component);
```

### Absorb Shield Stacking

**Problem**: Different shields conflicting (Ice Barrier blocking PW:S)

**Solution**: Use `ability_name` as stacking key, not just `AuraType`:
```rust
// Each absorb shield is tracked separately
let key = format!("absorb:{}", pending.aura.ability_name);
```

---

## Bug Detection Checklist

Run through this checklist after every implementation:

### 1. State Lifecycle
- [ ] Resources created are cleaned up on scene exit
- [ ] Entities despawned properly (marker components for bulk cleanup)

### 2. UI Reactivity
- [ ] ALL UI elements update when underlying data changes
- [ ] Not just labels - buttons, panels, everything

### 3. System Ordering
- [ ] Use `.chain()` when one system must see changes from another
- [ ] Change detection is frame-based

### 4. Query Filters
- [ ] Avoid overly restrictive filters like `With<Children>`
- [ ] Use `Without<T>` to prevent query conflicts

### 5. Edge Cases
- [ ] Test max/min values
- [ ] Test empty states
- [ ] Test rapid changes

### 6. Visual Stability
- [ ] Fixed/min heights for panels
- [ ] No disorienting resizing as content changes

### 7. Idempotency
- [ ] Rebuilds work multiple times without query failures

### 8. Headless Compatibility
- [ ] Gameplay-critical components added in core systems
- [ ] Optional resources for graphics (Assets<Mesh>, etc.)

---

## Build Commands

```bash
# Development build (fast compile, slower runtime)
cargo run

# Development with dynamic linking (fastest compile)
cargo run --features dev

# Release build (slow compile, optimized)
cargo run --release

# Check for errors without building
cargo check

# Run headless simulation
cargo run --release -- --headless /tmp/test.json
```

---

## Module Organization

After Session 6 refactoring, the `play_match` module follows single-responsibility:

| Module          | Responsibility                        | Lines |
|-----------------|---------------------------------------|-------|
| `abilities.rs`  | Ability definitions and spell schools | ~435  |
| `components/`   | Component & resource data structures  | ~467  |
| `camera.rs`     | Camera control systems                | ~291  |
| `projectiles.rs`| Spell projectile systems              | ~270  |
| `rendering/`    | UI rendering (cast bars, FCT, etc.)   | ~834  |
| `auras.rs`      | Status effect & aura systems          | ~329  |
| `match_flow.rs` | Countdown, victory, time controls     | ~288  |
| `combat_ai.rs`  | AI decision-making                    | ~1169 |
| `combat_core.rs`| Core combat mechanics                 | ~1144 |
| `mod.rs`        | Setup, cleanup, module coordination   | ~325  |

**Benefit**: AI can reason about and modify specific subsystems without context overload.
