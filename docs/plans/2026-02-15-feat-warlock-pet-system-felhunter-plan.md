---
title: "feat: Add Warlock Pet System (Felhunter)"
type: feat
date: 2026-02-15
brainstorm: docs/brainstorms/2026-02-15-warlock-pet-system-brainstorm.md
deepened: 2026-02-15
---

# feat: Add Warlock Pet System (Felhunter)

## Enhancement Summary

**Deepened on:** 2026-02-15
**Research agents used:** architecture-strategist, pattern-recognition-specialist, performance-oracle, code-simplicity-reviewer, bevy-ecs-researcher, learnings-researcher

### Key Improvements from Research

1. **Don't pollute CharacterClass** — Felhunter is NOT a player class. Keep CharacterClass for player classes only. Use `is_pet: bool` on CombatantInfo and `PetType` enum on the Pet component.
2. **Drop PetOwner** — Single-direction link via `Pet { owner }` is sufficient. Query with `Query<&Pet>` and filter by owner. Matches existing codebase patterns (Projectile, SpeechBubble).
3. **Separate context building from class dispatch** — Don't add `Without<Pet>` to decide_abilities query. Instead, build CombatantInfo map from ALL entities, then skip pets during class dispatch with a `pet_check: Query<(), With<Pet>>` guard.
4. **Extend DispelPending** — Devour Magic uses the existing `DispelPending` pattern from `effects/dispels.rs` instead of creating a parallel DevourMagicPending system. Add `heal_caster_on_success: Option<f32>` field.
5. **Add `is_pet` to CombatantInfo early** — Phase 1, not Phase 6. Every AI function needs this from day one.
6. **Extract shared spawn function** — `spawn_pet_components()` returns a bundle, used by both graphical and headless spawning.
7. **PET_SLOT_BASE constant** — Define `pub const PET_SLOT_BASE: u8 = 10` in constants.rs.

## Overview

Add a pet system to the arena combat simulation, starting with the Warlock's Felhunter demon. The pet is a full combat participant — targetable, killable, with its own health pool, AI, and abilities. It follows the Warlock's current target but makes its own ability decisions via a dedicated `PetAI` trait.

The system is designed for reuse with Hunter pets in the future.

**Brainstorm:** [2026-02-15-warlock-pet-system-brainstorm.md](../brainstorms/2026-02-15-warlock-pet-system-brainstorm.md)

## Problem Statement / Motivation

Warlocks currently lack their signature pet mechanic, which is core to the WoW Classic Warlock identity. In arena PvP, the Felhunter provides critical anti-caster utility (interrupt + dispel) that shapes how opponents play. Adding pets also establishes a reusable entity framework for Hunter pets.

## Proposed Solution

### Architecture: Pet as Combatant with PetAI Trait

```
┌─────────────────────────────────┐
│ Warlock Entity                  │
│ ├─ Combatant (class: Warlock)   │
│ └─ ... standard components      │
└─────────────┬───────────────────┘
              │ owns (queried via Pet.owner)
              ▼
┌─────────────────────────────────┐
│ Felhunter Entity                │
│ ├─ Combatant (class: Warlock,   │
│ │    slot: PET_SLOT_BASE+N)     │
│ ├─ Pet { owner: Entity,         │
│ │        pet_type: Felhunter }  │
│ ├─ PlayMatchEntity              │
│ ├─ Transform                    │
│ └─ FloatingTextState            │
└─────────────────────────────────┘
```

**Key principle:** The pet IS a `Combatant` entity with `class: Warlock` (its owner's class). All existing combat systems (damage, healing, auras, projectiles, auto-attacks, movement) work automatically. The `Pet` marker component distinguishes pets from primary combatants. We use a `With<Pet>` / `pet_check` guard for dispatch, NOT `Without<Pet>` on queries that build context maps.

**Why no PetOwner:** Single-direction ownership via `Pet { owner }` is sufficient and matches existing patterns (Projectile, SpeechBubble). To find a Warlock's pet, query `Query<(Entity, &Pet)>` and filter `pet.owner == warlock_entity`.

### AI Pipeline

```
┌────────────────────┐     ┌───────────────────┐
│ acquire_targets    │────▶│ decide_abilities   │  (skip pets via pet_check guard)
│ (all Combatants)   │     │ (player classes)   │
└────────────────────┘     └───────────────────┘
         │
         ▼
┌────────────────────┐
│ pet_ai_system      │  (With<Pet> filter)
│ - Follow owner tgt │
│ - FelhunterAI      │
│   decisions        │
└────────────────────┘
```

## Technical Approach

### Phase 1: Foundation — Entity & Component Layer

Core data model changes that everything else depends on.

#### 1.1 Add `Pet` component and `PetType` enum

**File:** `src/states/play_match/components/mod.rs`

```rust
/// Pet type enum (extensible for future demons and hunter pets)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PetType {
    Felhunter,
}

impl PetType {
    pub fn name(&self) -> &'static str {
        match self {
            PetType::Felhunter => "Felhunter",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            PetType::Felhunter => Color::srgb(0.4, 0.75, 0.4), // Green (demon)
        }
    }

    pub fn preferred_range(&self) -> f32 {
        match self {
            PetType::Felhunter => 2.0, // Melee
        }
    }

    pub fn movement_speed(&self) -> f32 {
        match self {
            PetType::Felhunter => 5.5,
        }
    }
}

/// Marker component for pet entities. Links pet to its owner.
#[derive(Component, Clone)]
pub struct Pet {
    pub owner: Entity,
    pub pet_type: PetType,
}
```

**No changes to `CharacterClass` enum.** Felhunter is not a player class. The pet's `Combatant` uses `class: Warlock` (its owner's class) for combat log identification.

#### 1.2 Add `is_pet` to CombatantInfo

**File:** `src/states/play_match/class_ai/mod.rs`

Add to `CombatantInfo`:
```rust
pub struct CombatantInfo {
    // ... existing fields ...
    pub is_pet: bool,
    pub pet_type: Option<PetType>,
}
```

Add helpers to `CombatContext`:
```rust
/// Get all alive primary (non-pet) enemies
pub fn alive_primary_enemies(&self) -> Vec<&CombatantInfo> {
    let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
    self.combatants.values()
        .filter(|c| c.team != my_team && c.is_alive && !c.is_pet)
        .collect()
}

/// Get all alive primary (non-pet) allies (including self if not a pet)
pub fn alive_primary_allies(&self) -> Vec<&CombatantInfo> {
    let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
    self.combatants.values()
        .filter(|c| c.team == my_team && c.is_alive && !c.is_pet)
        .collect()
}

/// Get lowest health primary (non-pet) ally
pub fn lowest_health_primary_ally(&self) -> Option<&CombatantInfo> {
    self.alive_primary_allies()
        .into_iter()
        .min_by(|a, b| a.health_pct().partial_cmp(&b.health_pct()).unwrap())
}
```

#### 1.3 Add PET_SLOT_BASE constant

**File:** `src/states/play_match/constants.rs`

```rust
/// Pet slots start at this offset. Pet of slot 0 = 10, slot 1 = 11, etc.
pub const PET_SLOT_BASE: u8 = 10;
```

#### 1.4 Add pet ability types

**File:** `src/states/play_match/abilities.rs`

Add to `AbilityType` enum:
- `SpellLock` — Felhunter interrupt (instant, 30yd range, 30s CD, 3s silence / 8s lockout)
- `DevourMagic` — Felhunter dispel (instant, 30yd range, 8s CD, heals pet for 30 on successful devour)

**File:** `assets/config/abilities.ron`

```ron
SpellLock: (
    name: "Spell Lock",
    cast_time: 0.0,
    range: 30.0,
    mana_cost: 0.0,
    cooldown: 30.0,
    damage_base_min: 0.0,
    damage_base_max: 0.0,
    damage_coefficient: 0.0,
    damage_scales_with: None,
    spell_school: Shadow,
    applies_aura: Some((
        aura_type: SpellLockout,
        duration: 3.0,
        magnitude: 0.0,
        break_on_damage: -1.0,
    )),
),
DevourMagic: (
    name: "Devour Magic",
    cast_time: 0.0,
    range: 30.0,
    mana_cost: 0.0,
    cooldown: 8.0,
    damage_base_min: 0.0,
    damage_base_max: 0.0,
    damage_coefficient: 0.0,
    damage_scales_with: None,
    spell_school: Holy,
),
```

**File:** `src/states/play_match/ability_config.rs` — add `SpellLock` and `DevourMagic` to `expected_abilities` validation list.

#### 1.5 Pet stat derivation from owner

**File:** `src/states/play_match/components/mod.rs`

Add a constructor for pet combatants. The pet uses its owner's class for the `Combatant` (for log formatting), but gets pet-specific stats:

```rust
impl Combatant {
    /// Create a new pet combatant with stats derived from the owner.
    pub fn new_pet(team: u8, slot: u8, pet_type: PetType, owner: &Combatant) -> Self {
        match pet_type {
            PetType::Felhunter => {
                // Start with base Warlock stats, then override for pet
                let mut pet = Self::new(team, slot, CharacterClass::Warlock);
                // Scale health to ~45% of owner's max health
                pet.max_health = owner.max_health * 0.45;
                pet.current_health = pet.max_health;
                // Pet-specific stats
                pet.max_mana = 200.0;
                pet.current_mana = 200.0;
                pet.mana_regen = 10.0;
                pet.attack_damage = 8.0;
                pet.attack_speed = 1.2;
                pet.attack_power = 20.0;
                pet.spell_power = owner.spell_power * 0.3;
                pet.crit_chance = 0.05;
                pet.base_movement_speed = pet_type.movement_speed();
                pet
            }
        }
    }
}
```

### Phase 2: Spawning & Lifecycle

#### 2.1 Extract shared spawn helper

**File:** `src/states/play_match/components/mod.rs` (or a new `pet_spawn.rs` utility)

Create a function that returns the common pet components, used by both graphical and headless:

```rust
/// Build the core components needed for a pet entity.
/// Returns (Combatant, Pet, FloatingTextState, Transform).
/// Caller adds mode-specific components (mesh/material for graphical).
pub fn build_pet_components(
    team: u8,
    owner_slot: u8,
    pet_type: PetType,
    owner_combatant: &Combatant,
    owner_position: Vec3,
) -> (Combatant, Pet, FloatingTextState, Transform) {
    let pet_slot = PET_SLOT_BASE + owner_slot;
    let pet_combatant = Combatant::new_pet(team, pet_slot, pet_type, owner_combatant);
    let pet_position = owner_position + Vec3::new(-2.0, 0.0, 1.5);
    // Pet component needs owner entity — caller must set this after spawn
    // Actually, we need the owner entity here. Caller passes it.
    // This is a data-only helper, not an ECS spawn.
    (
        pet_combatant,
        Pet { owner: Entity::PLACEHOLDER, pet_type }, // Caller sets owner
        FloatingTextState { next_pattern_index: 0 },
        Transform::from_translation(pet_position),
    )
}
```

Note: Since Pet needs the owner Entity (only available after spawning the owner), the actual spawn logic lives in the spawn sites. The helper extracts stat computation.

#### 2.2 Spawn pet at match start (graphical)

**File:** `src/states/play_match/mod.rs`

After spawning each Warlock combatant:

```rust
if class == CharacterClass::Warlock {
    let pet_slot = PET_SLOT_BASE + slot;
    let pet_combatant = Combatant::new_pet(team, pet_slot, PetType::Felhunter, &warlock_combatant);
    let pet_position = position + Vec3::new(-2.0, 0.0, 1.5);

    let pet_entity = commands.spawn((
        Mesh3d(/* smaller capsule mesh — 75% scale */),
        MeshMaterial3d(/* green material: PetType::Felhunter.color() */),
        Transform::from_translation(pet_position),
        pet_combatant,
        Pet { owner: warlock_entity, pet_type: PetType::Felhunter },
        FloatingTextState { next_pattern_index: 0 },
        OriginalMesh(/* mesh handle */),
        PlayMatchEntity,
    )).id();

    combat_log.register_combatant(format!("Team {} Felhunter", team));
}
```

#### 2.3 Spawn pet at match start (headless)

**File:** `src/headless/runner.rs`

Same logic without mesh/material:

```rust
if *character == CharacterClass::Warlock {
    let pet_slot = PET_SLOT_BASE + i as u8;
    let pet_combatant = Combatant::new_pet(team_num, pet_slot, PetType::Felhunter, &warlock_combatant);
    let pet_pos = /* offset from warlock spawn */;

    let pet_entity = commands.spawn((
        Transform::from_translation(pet_pos),
        pet_combatant,
        Pet { owner: warlock_entity, pet_type: PetType::Felhunter },
        FloatingTextState { next_pattern_index: 0 },
    )).id();

    combat_log.register_combatant(format!("Team {} Felhunter", team_num));
}
```

#### 2.4 Pet visual appearance (graphical mode only)

- **Mesh:** Smaller capsule (75% scale of player combatants)
- **Color:** Green (`PetType::Felhunter.color()`) to distinguish from Warlock's purple
- **Health bar:** Standard health bar system works automatically since pet is a `Combatant`

### Phase 3: Victory Conditions & Filtering

#### 3.1 Filter pets from victory checks

**File:** `src/states/play_match/match_flow.rs`

Change the `check_match_end` query to add `Without<Pet>`:
```rust
Query<(Entity, &Combatant, &Transform), Without<Pet>>
```

Only primary combatants determine victory.

**File:** `src/headless/runner.rs`

Same change to `headless_check_match_end` query.

### Phase 4: Pet AI System

#### 4.1 PetAI trait and FelhunterAI

**New file:** `src/states/play_match/pet_ai/mod.rs`

```rust
pub mod felhunter;

/// Context for pet AI decisions, extends CombatContext with owner info.
pub struct PetContext<'a> {
    pub ctx: &'a CombatContext<'a>,
    pub owner_entity: Entity,
    pub owner_target: Option<Entity>,
    pub pet_entity: Entity,
}

/// Trait for pet-specific AI logic.
pub trait PetAI {
    fn decide_action(&self, pet_ctx: &PetContext, combatant: &Combatant) -> AbilityDecision;
}
```

**New file:** `src/states/play_match/pet_ai/felhunter.rs`

FelhunterAI priority logic:

1. **If incapacitated (stunned/feared/polymorphed):** do nothing
2. **Devour Magic — CC removal (highest priority):**
   - Check owner and all non-pet allies for Polymorph, Fear, Root auras (magic school only)
   - If found and DevourMagic off cooldown + in range -> dispel highest-priority CC'd ally
3. **Spell Lock — interrupt:**
   - Check if any visible enemy is casting/channeling
   - If owner's target is casting: interrupt that (prefer owner's target)
   - If any other enemy casting: interrupt them
   - Must be in range (30yd) and Spell Lock off cooldown
   - Uses existing `InterruptPending` pattern (same as Pummel/Kick)
4. **Devour Magic — eat enemy defensive buffs:**
   - Check enemies for key beneficial auras: Absorb (PW:Shield, Ice Barrier), DamageImmunity (Divine Shield)
   - Prioritize: DamageImmunity > Absorb > other beneficial magic auras
   - Target must be in range (30yd)
5. **Devour Magic — clean ally debuffs:**
   - If team HP is stable (all primary allies > 70% HP), check owner/allies for any dispellable magic debuffs
   - Only if no higher-priority actions available
6. **Otherwise:** Melee auto-attack (handled by existing `combat_auto_attack` system)

#### 4.2 Pet AI system function

**File:** `src/states/play_match/pet_ai/mod.rs`

```rust
pub fn pet_ai_system(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    ability_defs: Res<AbilityDefinitions>,
    time: Res<Time>,
    mut pet_query: Query<(Entity, &mut Combatant, &Transform, &Pet)>,
    combatant_query: Query<(Entity, &Combatant, &Transform)>,
    aura_query: Query<&ActiveAuras>,
    casting_query: Query<&CastingState>,
    channeling_query: Query<&ChannelingState>,
) {
    // Build CombatContext from ALL combatants (same HashMap as decide_abilities)
    // For each pet entity:
    //   1. Set pet.target = owner.target (follow owner's target)
    //   2. Build PetContext with owner info
    //   3. Dispatch to FelhunterAI::decide_action()
    //   4. Execute the returned AbilityDecision
}
```

#### 4.3 Pet target following

Inside `pet_ai_system`, before ability decisions:
- Read the owner's `target` field from the combatant query
- Set the pet's `target` to match the owner's target
- If owner has no target or is dead, pet acquires its own target (nearest alive enemy)

#### 4.4 Devour Magic processing — extend DispelPending

**File:** `src/states/play_match/effects/dispels.rs`

Extend the existing `DispelPending` component with an optional self-heal field:

```rust
#[derive(Component)]
pub struct DispelPending {
    pub caster: Entity,
    pub target: Entity,
    pub is_offensive: bool,
    pub log_prefix: String,
    pub heal_caster_on_success: Option<f32>,  // NEW: Devour Magic heals pet for 30
}
```

The existing `process_dispels` system already handles:
- Removing auras from the target
- Logging with `log_prefix`
- Spawning `DispelBurst` visual

Add to `process_dispels`: after a successful dispel, if `heal_caster_on_success` is Some, heal the caster entity.

This avoids creating a parallel system and reuses the battle-tested dispel pipeline.

### Phase 5: Movement

#### 5.1 Pet movement behavior

**File:** `src/states/play_match/combat_core.rs` — `move_to_target` system

The pet already participates in `move_to_target` since it has `Combatant` + `Transform`. The existing logic handles:
- Moving toward target if out of preferred range (Felhunter preferred_range = 2.0 = melee)
- Fear/Polymorph wandering
- Root/stun stopping movement
- Speed slow debuffs

**Additional behavior needed:** When pet has NO target, follow the owner. Add a pet follow check in `move_to_target`:

```rust
// If this is a pet with no target, follow the owner
if let Ok(pet) = pet_query.get(entity) {
    if combatant.target.is_none() {
        // Move toward owner if > 5 units away
        if let Ok(owner_transform) = transform_query.get(pet.owner) {
            let dist = transform.translation.distance(owner_transform.translation);
            if dist > 5.0 {
                let direction = (owner_transform.translation - transform.translation).normalize();
                transform.translation += direction * combatant.base_movement_speed * dt;
            }
        }
    }
}
```

**Note on preferred_range:** The existing `CharacterClass::preferred_range()` is used for movement. Since the pet's Combatant has `class: Warlock`, we need to check if the entity is a pet and use `PetType::preferred_range()` instead. Add a `Pet` query to the movement system and override range for pet entities.

### Phase 6: Existing System Integration

Critical changes to prevent undesirable interactions.

#### 6.1 decide_abilities — skip pets without hiding from context

**File:** `src/states/play_match/combat_ai.rs`

**Do NOT** add `Without<Pet>` to the decide_abilities query. This would hide pets from the CombatantInfo HashMap, making them invisible to all AI.

Instead, add a separate check query:
```rust
pet_check: Query<(), With<Pet>>,
```

In the dispatch loop, skip pet entities:
```rust
for (entity, mut combatant, transform, ...) in &mut query {
    if pet_check.get(entity).is_ok() {
        continue; // Pets use pet_ai_system
    }
    // ... existing class dispatch ...
}
```

When building the CombatantInfo HashMap, populate `is_pet` and `pet_type` by checking the Pet component:
```rust
let is_pet = pet_query.get(entity).is_ok();
let pet_type = pet_query.get(entity).ok().map(|p| p.pet_type);
```

#### 6.2 Healer AI — deprioritize pet healing

**File:** `src/states/play_match/class_ai/priest.rs` and `paladin.rs`

Use `ctx.lowest_health_primary_ally()` for healing decisions. Healers should only heal pets if ALL primary allies are above 90% HP and the pet is below 50%.

#### 6.3 Warlock curse spreading — exclude pets

**File:** `src/states/play_match/class_ai/warlock.rs`

In curse target selection, use `ctx.alive_primary_enemies()` instead of `ctx.alive_enemies()`. Warlocks should not waste curses on enemy pets.

#### 6.4 CC target heuristics — deprioritize pets

**File:** `src/states/play_match/combat_ai.rs`

In `select_cc_target_heuristic()`, give pets a very low CC priority score. Mages should not Polymorph the Felhunter when the Warlock is available.

#### 6.5 Fallback targeting — deprioritize pets

In `acquire_targets`, when no kill_target is specified and fallback targeting selects by distance/priority, deprioritize pet entities. Add a distance penalty (e.g., +20 virtual yards) for pets so players are preferred targets.

### Phase 7: Configuration

#### 7.1 Headless config — pet type selection

**File:** `src/headless/config.rs`

Add optional pet config to `HeadlessMatchConfig`:

```rust
pub team1_warlock_pets: Option<Vec<String>>,  // ["Felhunter"]
pub team2_warlock_pets: Option<Vec<String>>,
```

Default: `Felhunter` for each Warlock (if not specified).

#### 7.2 Kill target — pet targeting

Extend `kill_target` to support targeting pets. Convention: indices 0-2 are primary combatants, indices 10+ are pets (slot 10 = pet of slot 0, 11 = pet of slot 1, etc.).

In `acquire_targets`, when resolving `kill_target_index`:
- Indices 0-2: target primary combatants (existing behavior)
- Indices 10+: target the pet owned by the combatant at slot (index - 10)

**Note:** Use `PET_SLOT_BASE` constant for this resolution, not hardcoded 10.

### Phase 8: Combat Logging

#### 8.1 Pet combatant ID format

**File:** `src/states/play_match/utils.rs`

The `combatant_id()` function currently uses `class.name()`. For pets, check the `Pet` component and use `pet_type.name()` instead. Format: `"Team 1 Felhunter"`.

For multi-Warlock teams with multiple Felhunters, append a suffix: `"Team 1 Felhunter (2)"`.

#### 8.2 Match results

**File:** `src/headless/runner.rs` — `build_match_result()`

Include pet stats in match results. Mark pet entries so the results display can distinguish them.

### Phase 9: System Registration (CRITICAL)

Per project memory: **BOTH** graphical and headless modes must register all new systems.

#### Systems to register:

| System | Phase | `states/mod.rs` | `systems.rs` |
|--------|-------|-----------------|--------------|
| `pet_ai_system` | CombatAndMovement (after acquire_targets, before decide_abilities) | Yes | Yes |
| Extended `process_dispels` | ResourcesAndAuras (already registered — just extend it) | N/A | N/A |

**File:** `src/states/mod.rs` — Add to graphical system registration
**File:** `src/states/play_match/systems.rs` — Add to `add_core_combat_systems()`
**File:** `src/states/play_match/mod.rs` — Add `pub mod pet_ai;` and re-export

## Acceptance Criteria

### Functional Requirements

- [ ] Warlock spawns with a Felhunter pet at match start (both graphical and headless)
- [ ] Felhunter follows Warlock's target and engages in melee
- [ ] Felhunter uses Spell Lock to interrupt enemy casters (30yd range, 30s CD)
- [ ] Felhunter uses Devour Magic offensively (eat enemy buffs) and defensively (remove ally CC/debuffs)
- [ ] Devour Magic priority: CC removal > enemy defensives > ally debuff cleanup
- [ ] Devour Magic heals Felhunter for 30 HP on successful devour
- [ ] Felhunter auto-attacks in melee range
- [ ] Felhunter is targetable and killable by enemies
- [ ] Killing the Felhunter does NOT end the match (WoW-style victory)
- [ ] Felhunter health scales with Warlock stats (~45% of Warlock HP)
- [ ] When Warlock has no target, Felhunter follows the Warlock
- [ ] When Warlock dies, Felhunter acquires own targets and continues fighting
- [ ] Healer AI does not waste heals on the Felhunter over primary combatants
- [ ] Warlocks do not waste curses on enemy pets
- [ ] CC heuristics deprioritize pets as CC targets
- [ ] Headless config supports `team1_warlock_pets` / `team2_warlock_pets` fields
- [ ] `kill_target` config supports targeting pets (index 10+)
- [ ] CharacterClass enum is NOT modified — pets use Pet component for identification

### Quality Gates

- [ ] `cargo build --release` compiles cleanly
- [ ] Headless simulation: Warlock+Priest vs Mage+Priest — Felhunter interrupts Frostbolt
- [ ] Headless simulation: Warlock+Priest vs Priest+Warrior — Felhunter devours PW:Shield
- [ ] Headless simulation: Match ends correctly when Warlock dies but Felhunter survives
- [ ] Headless simulation: Match does NOT end when only the Felhunter dies
- [ ] Combat log shows Felhunter actions with proper formatting
- [ ] Systems registered in BOTH `states/mod.rs` AND `systems.rs`

## Dependencies & Risks

**Dependencies:**
- Existing `DispelPending` / `process_dispels` must be extended with `heal_caster_on_success` field and offensive dispel support.
- `SpellLockout` aura type must work when applied by a pet entity.
- `InterruptPending` pattern from existing interrupt system (Pummel/Kick) reused for Spell Lock.

**Risks:**
- **CombatantInfo context visibility:** Mitigated by using `pet_check` guard instead of `Without<Pet>` on context-building queries.
- **Widespread AI behavior change:** Every class AI that iterates `alive_enemies()` or `alive_allies()` now sees pets. Use `alive_primary_enemies()` / `alive_primary_allies()` where pets should be excluded.
- **System ordering:** `pet_ai_system` must run after `acquire_targets` but before `decide_abilities`. Register in the correct phase.
- **Preferred range override:** Pet uses owner's class for Combatant, so `CharacterClass::preferred_range()` returns Warlock range (28.0). Must override with `PetType::preferred_range()` in movement system.

## Implementation Order

1. Phase 1: Foundation (Pet component, PetType, is_pet on CombatantInfo, abilities, constants)
2. Phase 2: Spawning — pet appears in arena, has health bar, can be attacked
3. Phase 3: Victory filtering — match end works correctly with pets
4. Phase 6: Existing system integration — prevent bad interactions (context visibility, healer priorities, curse filtering, CC deprioritization)
5. Phase 4: Pet AI — Felhunter makes decisions and uses abilities
6. Phase 5: Movement — follow-owner behavior when idle, preferred_range override
7. Phase 7: Configuration — headless config pet selection
8. Phase 8: Logging — proper combat log formatting
9. Phase 9: System registration — verify both modes work

## References

### Internal References
- Brainstorm: `docs/brainstorms/2026-02-15-warlock-pet-system-brainstorm.md`
- Dual system registration pattern: `docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md`
- Visual effects pattern: `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md`
- Adding new class pattern: `docs/solutions/implementation-patterns/adding-new-class-paladin.md`
- Entity spawning: `src/states/play_match/mod.rs:448-500`
- Victory conditions: `src/states/play_match/match_flow.rs:155-276`
- Target selection: `src/states/play_match/combat_ai.rs:21-198`
- Class AI trait: `src/states/play_match/class_ai/mod.rs:239-256`
- Combatant struct: `src/states/play_match/components/mod.rs:376-437`
- Headless config: `src/headless/config.rs:11-55`
- Dispel processing: `src/states/play_match/effects/dispels.rs`
- Interrupt pending pattern: `src/states/play_match/effects/interrupts.rs`

### WoW Classic Spell Data
- **Spell Lock (Rank 2):** Instant, 30yd, 30s CD. 3s silence, 8s school lockout on interrupt. [Wowhead #19647](https://www.wowhead.com/classic/spell=19647)
- **Devour Magic (Rank 4):** Instant, 30yd, 8s CD. Purge 1 magic effect. Heals pet for 619 on success. [Wowhead #19736](https://www.wowhead.com/classic/spell=19736)
