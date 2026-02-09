---
title: "Graphical Mode Missing System Registration"
category: implementation-patterns
tags: [bevy, system-registration, dual-mode, silent-failure, effects]
module: play_match
symptom: "AI logs ability cast but [BUFF]/[DMG] entries absent in graphical mode"
root_cause: "New systems registered only in headless add_core_combat_systems(), not in graphical states/mod.rs"
date: 2026-02-09
---

# Graphical Mode Missing System Registration

## Problem Statement

When adding new combat systems (like `process_divine_shield`, `process_holy_shock_heals`, `process_holy_shock_damage`, `process_dispels`) to the ArenaSim Bevy ECS game, the systems were registered ONLY in the headless mode's `add_core_combat_systems()` function in `src/states/play_match/systems.rs`, but NOT in the graphical mode's separate system registration in `src/states/mod.rs`.

This caused abilities to work perfectly in headless simulations but silently fail in the graphical client. The AI would log that it cast the ability and put it on cooldown, but the actual effect (damage, healing, aura application) never happened.

## Symptoms

- AI logs ability cast (e.g., "[CAST] Team 1 Paladin casts Divine Shield") but no corresponding [BUFF] or [DMG] entries appear
- Abilities work in headless mode (`cargo run --release -- --headless`) but fail in graphical mode (`cargo run --release`)
- No errors or panics — the system simply never runs because it's not registered
- Ability goes on cooldown as expected (casting system runs) but effect never applies (effect system missing)

## Root Cause

The project has **dual system registration paths**:

### 1. Headless Mode
**File**: `src/states/play_match/systems.rs`
**Function**: `add_core_combat_systems()`

```rust
pub fn add_core_combat_systems(app: &mut App) {
    app.add_systems(
        Update,
        (
            // ... other systems
            play_match::process_dispels,
            play_match::process_holy_shock_heals,
            play_match::process_holy_shock_damage,
            play_match::process_divine_shield,
        )
            .run_if(in_state(GameState::PlayMatch)),
    );
}
```

### 2. Graphical Mode
**File**: `src/states/mod.rs`
**Plugin**: `StatesPlugin::build()`

Systems are manually registered in phases:

```rust
// Phase 1: Resources and Auras
.add_systems(
    Update,
    (
        play_match::update_rage,
        play_match::update_energy,
        // ... other systems
        play_match::process_dispels,          // <-- MUST ADD HERE
        play_match::process_holy_shock_heals, // <-- MUST ADD HERE
        play_match::process_holy_shock_damage,// <-- MUST ADD HERE
        play_match::process_divine_shield,    // <-- MUST ADD HERE
    )
        .chain()
        .run_if(in_state(GameState::PlayMatch))
        .run_if(in_state(MatchFlowState::InProgress)),
)
```

When new effect processing systems were added to `systems.rs` (headless), they were never added to `states/mod.rs` (graphical).

## Why This Happens Silently

1. **Casting system runs independently**: The casting system (`process_cast_completion`) is registered in both modes, so it logs the cast and puts the ability on cooldown
2. **Effect system never runs**: The effect processing system (e.g., `process_divine_shield`) is only registered in headless mode
3. **No Bevy error**: Bevy doesn't error when a system isn't registered — it simply never runs
4. **AI continues normally**: The AI sees the cast succeeded (cooldown started) and continues making decisions

## Export Chain Requirement

For systems defined in submodules to be accessible from `states/mod.rs`, they must be re-exported through the module hierarchy:

**File**: `src/states/play_match/mod.rs`

```rust
// Re-export all effects module systems
pub use effects::*;
```

**File**: `src/states/play_match/effects/mod.rs`

```rust
pub mod dispels;
pub mod divine_shield;
pub mod holy_shock;

pub use dispels::*;
pub use divine_shield::*;
pub use holy_shock::*;
```

Without this chain, `states/mod.rs` cannot access `play_match::process_divine_shield`.

## Solution

### Step 1: Identify the Phase

Effect processing systems typically belong in **Phase 1: Resources and Auras** (before damage/healing application in Phase 2).

Search `states/mod.rs` for similar systems to find the right insertion point:

```bash
# Find where other aura/effect systems are registered
grep -n "process_" src/states/mod.rs
```

### Step 2: Add Systems to Graphical Mode

**File**: `src/states/mod.rs` (around line 181+)

```rust
// Phase 1: Resources and Auras
.add_systems(
    Update,
    (
        play_match::update_rage,
        play_match::update_energy,
        play_match::update_mana,
        play_match::update_diminishing_returns,
        play_match::update_spell_lockout,
        play_match::tick_auras,
        play_match::expire_auras,
        play_match::apply_healing_reduction,
        play_match::update_max_health_auras,
        play_match::update_max_mana_auras,
        play_match::apply_movement_speed_modifiers,
        play_match::process_dispels,          // <-- ADD NEW SYSTEMS HERE
        play_match::process_holy_shock_heals,
        play_match::process_holy_shock_damage,
        play_match::process_divine_shield,
    )
        .chain()
        .run_if(in_state(GameState::PlayMatch))
        .run_if(in_state(MatchFlowState::InProgress)),
)
```

### Step 3: Verify Re-exports

Ensure the systems are exported through the module chain:

**File**: `src/states/play_match/mod.rs`

```rust
pub use effects::*;  // <-- MUST EXIST
```

### Step 4: Test Both Modes

```bash
# Test headless
cargo run --release -- --headless /tmp/test.json
cat match_logs/$(ls -t match_logs | head -1)

# Test graphical
cargo run --release
# Manually trigger ability and check combat log
```

Look for both cast logs AND effect logs:
- `[CAST] Team 1 Paladin casts Divine Shield` ✓
- `[BUFF] Team 1 Paladin gains Divine Shield (absorbs 500 damage)` ✓

## Secondary Bug: Break-on-Damage Convention

While fixing Divine Shield, discovered the `break_on_damage_threshold` convention:

### Incorrect (breaks on any damage, like Polymorph):
```rust
absorb_amount: 500.0,
break_on_damage_threshold: 0.0,  // ❌ WRONG for Divine Shield
```

### Correct (never breaks on damage):
```rust
absorb_amount: 500.0,
break_on_damage_threshold: -1.0,  // ✅ CORRECT for Divine Shield
```

### Convention:
- `>= 0.0`: Breakable (0.0 = any damage breaks it, 35.0 = breaks after 35 cumulative damage)
- `< 0.0` (negative): Never breaks from damage

**File**: `src/states/play_match/effects/divine_shield.rs`

## Prevention Checklist

When adding ANY new combat system:

- [ ] Add system to `src/states/play_match/systems.rs` → `add_core_combat_systems()`
- [ ] Add system to `src/states/mod.rs` → `StatesPlugin` in the correct phase
- [ ] Verify `pub use effects::*;` exists in `play_match/mod.rs`
- [ ] Test in **headless mode**: `cargo run --release -- --headless /tmp/test.json`
- [ ] Test in **graphical mode**: `cargo run --release`
- [ ] Verify BOTH cast logs AND effect logs appear in combat log
- [ ] Check `break_on_damage_threshold` convention for absorb/immunity auras

## System Ordering Phases

For reference, the graphical mode uses this phase structure in `states/mod.rs`:

### Phase 1: Resources and Auras
- Energy/Rage/Mana updates
- Aura ticks and expiration
- Stat modifiers (max health, movement speed)
- **Effect processing systems** (dispels, Divine Shield, Holy Shock)

### Phase 2: Combat Core
- Melee damage
- Spell damage/healing
- Projectile impacts
- DoT ticks

### Phase 3: State Updates
- Death detection
- Combat state changes
- Match end conditions

Place new effect processing systems in **Phase 1** unless they depend on damage/healing results (then Phase 2).

## Related Files

- **Headless registration**: `src/states/play_match/systems.rs`
- **Graphical registration**: `src/states/mod.rs`
- **Module re-exports**: `src/states/play_match/mod.rs`
- **Effect systems**: `src/states/play_match/effects/`

## Related Patterns

- [Adding Visual Effect (Bevy)](./adding-visual-effect-bevy.md) - Visual systems registration (graphical-only)
- [Dual Mode Architecture](../../design-docs/bevy-patterns.md) - Headless vs graphical separation
