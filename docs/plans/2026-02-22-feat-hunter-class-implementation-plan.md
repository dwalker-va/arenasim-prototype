---
title: "feat: Implement Hunter Class"
type: feat
date: 2026-02-22
deepened: 2026-02-22
---

# feat: Implement Hunter Class

## Enhancement Summary

**Deepened on:** 2026-02-22
**Research agents used:** architecture-strategist, performance-oracle, code-simplicity-reviewer, spec-flow-analyzer, learnings-researcher, repo-research-analyst, Wowhead Classic MCP

### Key Improvements from Deepening
1. **Remove `AutoShot` AbilityType** — use existing ranged auto-attack system with dead zone check (3 lines)
2. **Collapse 7 trap systems into 2** — `trap_system()` + `slow_zone_system()` in `traps.rs`
3. **Add `min_range` to `AbilityConfig`** — data-driven dead zone enforcement, not Hunter-specific hack
4. **Add `AuraType::Incapacitate`** — frozen targets don't wander (unlike Polymorph)
5. **Add `GroundTargetAbility` to `AbilityDecision`** — needed for trap placement at world positions
6. **Fix system phase ordering** — trap proximity in Phase 2 (after movement), not Phase 1
7. **Remove redundant `position` fields** from Trap/SlowZone components — use `Transform::translation`
8. **Skip predictive trap placement for v1** — place at midpoint between Hunter and target
9. **Merge phases 7→5** — Foundation, Mechanics, AI+Pets, Integration, Testing
10. **Grant CC immunity during Disengage** — extend `apply_pending_auras` check

### Design Decisions (from conflicting agent advice)
- **Incapacitate vs Polymorph**: Use new `AuraType::Incapacitate` (architecture-strategist) — frozen targets must not wander
- **traps.rs vs combat_core.rs**: Keep `traps.rs` (architecture-strategist) — traps are a distinct mechanic with their own entity lifecycle, even simplified to 2 systems
- **Disengage while rooted**: Does NOT work (WoW Classic authentic). Hunter relies on Bird/Master's Call for root removal.
- **Frost Trap zone DR**: Zone-managed aura — refresh duration while inside, no DR re-application. Remove slow when enemy exits zone.
- **Trap limit**: Per-Hunter, enforced inline at spawn time (not a separate system)
- **Pets trigger traps**: Yes — pets are enemy combatants for trap purposes
- **Traps and immune targets**: Trap triggers and is consumed even if CC fails (Divine Shield, DR immune)

### WoW Classic Reference Values (from Wowhead MCP)
| Ability | WoW Classic | ArenaSim (adjusted) |
|---------|------------|-------------------|
| Aimed Shot | 3s cast, 75 mana, 35yd, 6s CD | 2.5s cast, 30 mana, 35yd, 10s CD |
| Arcane Shot | Instant, 25 mana, 35yd, 6s CD | Instant, 20 mana, 35yd, 6s CD |
| Concussive Shot | 35yd, 12s CD, 50% slow 4s | 35yd, 12s CD, 50% slow 4s |
| Freezing Trap | 50 mana, 15s CD, 10s freeze | 15 mana, 25s CD, 8s freeze |
| Frost Trap | 60 mana, 15s CD, 30s zone, 10yd, 60% slow | 15 mana, 20s CD, 10s zone, 8yd, 60% slow |

---

## Overview

Add the Hunter as ArenaSim's 7th class — the first ranged physical DPS. The Hunter introduces three new systems: **dead zone enforcement** (minimum range on ranged abilities), **ground-targeted traps** with proximity triggers, and **choosable pets** with distinct AI behaviors. This fills the "ranged physical" niche distinct from caster ranged (Mage/Warlock) and melee physical (Warrior/Rogue).

## Problem Statement / Motivation

The class roster lacks a ranged physical DPS archetype. All ranged classes are casters (Mage, Warlock, Priest). The Hunter adds positional gameplay (dead zone management), area denial (traps), and team-comp adaptability (pet choice) — mechanics that don't exist yet and create new counterplay dynamics against every existing class.

## Proposed Solution

Implement the Hunter following the established class pattern (Paladin template), introducing three new ECS systems (traps, slow zones, dead zone enforcement) while reusing existing pet, aura, and projectile infrastructure.

## Technical Approach

### Architecture

```
New files:
  src/states/play_match/class_ai/hunter.rs    # Hunter AI decision logic
  src/states/play_match/traps.rs              # Trap + SlowZone ECS systems (2 systems)

Modified files (16):
  src/states/match_config.rs                  # CharacterClass::Hunter, MatchConfig pet prefs
  src/states/play_match/abilities.rs          # 9 new AbilityType variants, min_range in can_cast
  src/states/play_match/ability_config.rs     # validate(), get_class_abilities(), min_range field
  assets/config/abilities.ron                 # Hunter ability definitions with min_range
  src/states/play_match/components/mod.rs     # Trap/SlowZone/Disengage/Incapacitate, PetType, stats
  src/states/play_match/class_ai/mod.rs       # pub mod hunter, dispatch, GroundTargetAbility variant
  src/states/play_match/class_ai/pet_ai.rs    # Spider/Boar/Bird AI functions
  src/states/play_match/combat_ai.rs          # Hunter dispatch block
  src/states/play_match/combat_core.rs        # Dead zone auto-attack, Disengage movement, CC immunity
  src/states/play_match/constants.rs          # HUNTER_DEAD_ZONE, TRAP_*, DISENGAGE_DISTANCE
  src/states/play_match/systems.rs            # Register trap/slowzone systems (headless)
  src/states/play_match/mod.rs                # pub mod traps, Hunter pet spawning
  src/states/mod.rs                           # Register systems (graphical)
  src/headless/config.rs                      # parse_class("Hunter"), pet pref parsing
  src/headless/runner.rs                      # Hunter pet spawning
  src/states/configure_match_ui.rs            # Hunter pet selection UI
```

### Implementation Phases

#### Phase 1: Foundation — Types, Config, Constants

Register Hunter in the type system and add data-driven ability definitions. No new gameplay yet — just the skeleton that compiles.

**Tasks:**

- [ ] Add `Hunter` to `CharacterClass` enum in `match_config.rs:62`
  - Add arms to: `all()`, `name()`, `description()`, `color()` (#ABD473), `is_melee()` (false), `is_healer()` (false), `uses_mana()` (true), `preferred_range()` (25.0)
- [ ] Add `Spider`, `Boar`, `Bird` to existing `PetType` enum in `components/mod.rs:36` (no separate `HunterPet` enum — extend `PetType` directly, like Felhunter)
  - Add `name()`, `color()`, `preferred_range()` (MELEE_RANGE for all — melee pets), `movement_speed()`, `is_melee()` (true) for each
  - Add `team1_hunter_pet_type: Vec<PetType>` and `team2_hunter_pet_type: Vec<PetType>` to `MatchConfig`
- [ ] Add `AuraType::Incapacitate` to components/mod.rs
  - Prevents all actions (like Stun) but uses `DRCategory::Incapacitates` (shares DR with Polymorph)
  - Update `DRCategory::from_aura_type()` to map `Incapacitate -> Incapacitates`
  - Update `is_incapacitated()` util to include `Incapacitate`
  - Update `apply_pending_auras` CC immunity list
  - Update `move_to_target` movement prevention to include `Incapacitate`
  - `is_magic_dispellable()` = true (Freezing Trap is a Frost school effect)
  - **No wandering behavior** (unlike Polymorph) — frozen in place
- [ ] Add 9 `AbilityType` variants in `abilities.rs:42` (NO `AutoShot` — use existing ranged auto-attack):
  - `AimedShot`, `ArcaneShot`, `ConcussiveShot`, `Disengage`
  - `FreezingTrap`, `FrostTrap`
  - `SpiderWeb`, `BoarCharge`, `MastersCall`
- [ ] Add `min_range: Option<f32>` field to `AbilityConfig` struct in `ability_config.rs` (default `None`)
  - Add `min_range` enforcement to `can_cast_config()` in `abilities.rs:93`: `if distance < ability_def.min_range.unwrap_or(0.0) { return false; }`
- [ ] Add all 9 to `validate()` expected_abilities in `ability_config.rs:242`
- [ ] Add `get_class_abilities()` arm for Hunter returning 7 abilities (AimedShot, ArcaneShot, ConcussiveShot, Disengage, FreezingTrap, FrostTrap — no pet abilities)
- [ ] Add `GroundTargetAbility { ability: AbilityType, position: Vec3 }` variant to `AbilityDecision` in `class_ai/mod.rs`
  - Add handling in `combat_ai.rs` `decide_abilities()` to spawn trap entity when this decision is returned
- [ ] Add ability definitions to `abilities.ron` (with `min_range` for dead zone abilities):

  ```ron
  AimedShot: (
      name: "Aimed Shot",
      cast_time: 2.5,
      range: 35.0,
      min_range: Some(8.0),
      mana_cost: 30.0,
      cooldown: 10.0,
      damage_base_min: 25.0,
      damage_base_max: 35.0,
      damage_coefficient: 0.6,
      damage_scales_with: AttackPower,
      spell_school: Physical,
      applies_aura: Some((
          aura_type: HealingReduction,
          duration: 10.0,
          magnitude: 0.5,
          break_on_damage: -1.0,
      )),
  ),
  ArcaneShot: (
      name: "Arcane Shot",
      cast_time: 0.0,
      range: 35.0,
      min_range: Some(8.0),
      mana_cost: 20.0,
      cooldown: 6.0,
      damage_base_min: 12.0,
      damage_base_max: 18.0,
      damage_coefficient: 0.4,
      damage_scales_with: AttackPower,
      spell_school: Arcane,
  ),
  ConcussiveShot: (
      name: "Concussive Shot",
      cast_time: 0.0,
      range: 35.0,
      min_range: Some(8.0),
      mana_cost: 15.0,
      cooldown: 12.0,
      damage_base_min: 0.0,
      damage_base_max: 0.0,
      damage_coefficient: 0.0,
      damage_scales_with: AttackPower,
      spell_school: Physical,
      applies_aura: Some((
          aura_type: MovementSpeedSlow,
          duration: 4.0,
          magnitude: 0.5,
          break_on_damage: -1.0,
      )),
  ),
  Disengage: (
      name: "Disengage",
      cast_time: 0.0,
      range: 0.0,
      mana_cost: 10.0,
      cooldown: 25.0,
      damage_base_min: 0.0,
      damage_base_max: 0.0,
      damage_coefficient: 0.0,
      damage_scales_with: AttackPower,
      spell_school: Physical,
  ),
  FreezingTrap: (
      name: "Freezing Trap",
      cast_time: 0.0,
      range: 25.0,
      mana_cost: 15.0,
      cooldown: 25.0,
      damage_base_min: 0.0,
      damage_base_max: 0.0,
      damage_coefficient: 0.0,
      damage_scales_with: AttackPower,
      spell_school: Frost,
      applies_aura: Some((
          aura_type: Incapacitate,
          duration: 8.0,
          magnitude: 1.0,
          break_on_damage: 0.0,
      )),
  ),
  FrostTrap: (
      name: "Frost Trap",
      cast_time: 0.0,
      range: 25.0,
      mana_cost: 15.0,
      cooldown: 20.0,
      damage_base_min: 0.0,
      damage_base_max: 0.0,
      damage_coefficient: 0.0,
      damage_scales_with: AttackPower,
      spell_school: Frost,
  ),
  SpiderWeb: (
      name: "Spider Web",
      cast_time: 0.0,
      range: 20.0,
      mana_cost: 0.0,
      cooldown: 45.0,
      damage_base_min: 0.0,
      damage_base_max: 0.0,
      damage_coefficient: 0.0,
      damage_scales_with: AttackPower,
      spell_school: Physical,
      applies_aura: Some((
          aura_type: Root,
          duration: 4.0,
          magnitude: 1.0,
          break_on_damage: 35.0,
      )),
  ),
  BoarCharge: (
      name: "Boar Charge",
      cast_time: 0.0,
      range: 25.0,
      mana_cost: 0.0,
      cooldown: 25.0,
      damage_base_min: 5.0,
      damage_base_max: 8.0,
      damage_coefficient: 0.2,
      damage_scales_with: AttackPower,
      spell_school: Physical,
      is_charge: true,
      applies_aura: Some((
          aura_type: Stun,
          duration: 1.0,
          magnitude: 1.0,
          break_on_damage: -1.0,
      )),
  ),
  MastersCall: (
      name: "Master's Call",
      cast_time: 0.0,
      range: 40.0,
      mana_cost: 0.0,
      cooldown: 45.0,
      damage_base_min: 0.0,
      damage_base_max: 0.0,
      damage_coefficient: 0.0,
      damage_scales_with: AttackPower,
      spell_school: Physical,
      is_dispel: true,
  ),
  ```

- [ ] Add Hunter stats to `Combatant::new()` in `components/mod.rs:590`:
  ```rust
  CharacterClass::Hunter => (ResourceType::Mana, 165.0, 160.0, 0.0, 160.0,
      11.0, 0.8, 30.0, 0.0, 0.07, 5.0),
  ```
- [ ] Add pet stats arms in `Combatant::new_pet()` in `components/mod.rs:664` (all use 45% owner HP, same as Felhunter)
- [ ] Add constants to `constants.rs`:
  ```rust
  pub const HUNTER_DEAD_ZONE: f32 = 8.0;
  pub const AUTO_SHOT_RANGE: f32 = 35.0;
  pub const TRAP_ARM_DELAY: f32 = 1.5;
  pub const TRAP_TRIGGER_RADIUS: f32 = 5.0;
  pub const FROST_TRAP_ZONE_RADIUS: f32 = 8.0;
  pub const FROST_TRAP_ZONE_DURATION: f32 = 10.0;
  pub const DISENGAGE_DISTANCE: f32 = 15.0;
  ```
- [ ] Add `"Hunter"` to `parse_class()` in `headless/config.rs:145`
- [ ] Add Hunter pet preference parsing in `headless/config.rs` (map `"Spider"`, `"Boar"`, `"Bird"` to `PetType`)
- [ ] Download spell icons for all Hunter abilities via Wowhead MCP
- [ ] Add icon paths to `get_ability_icon_path()` in `rendering/mod.rs`

**Success criteria:** `cargo build --release` compiles. Hunter appears in class selection. No new systems running yet.

---

#### Phase 2: New Mechanics — Traps, Dead Zone, Disengage

Build the trap system, slow zones, dead zone enforcement, and Disengage. All the novel Hunter-specific mechanics.

**Tasks:**

**ECS Components** (in `components/mod.rs`):
- [ ] Add `Trap` component:
  ```rust
  #[derive(Component)]
  pub struct Trap {
      pub owner: Entity,
      pub owner_team: u8,
      pub trap_type: TrapType,
      pub arm_timer: f32,
      pub armed: bool,
      pub trigger_radius: f32,
      // Position comes from Transform::translation — no redundant position field
  }

  #[derive(Clone, Copy, Debug, PartialEq)]
  pub enum TrapType { Freezing, Frost }
  ```
- [ ] Add `SlowZone` component:
  ```rust
  #[derive(Component)]
  pub struct SlowZone {
      pub owner: Entity,
      pub owner_team: u8,
      pub radius: f32,
      pub duration_remaining: f32,
      pub slow_magnitude: f32,
      // Position comes from Transform::translation
  }
  ```
- [ ] Add `DisengagingState` component:
  ```rust
  #[derive(Component)]
  pub struct DisengagingState {
      pub direction: Vec3,  // Must be normalized (use normalize_or_zero())
      pub distance_remaining: f32,
      pub speed: f32,
  }
  ```
- [ ] **CRITICAL**: Add `PlayMatchEntity` marker to ALL trap/slowzone spawns from day one

**Trap Systems** (create `src/states/play_match/traps.rs` — 2 systems, not 7):
- [ ] `trap_system()` — single system handling the full trap lifecycle:
  - Decrement `arm_timer`, set `armed = true` when timer hits 0
  - For armed traps: check proximity against all enemy combatants (AND enemy pets)
  - On trigger for Freezing: apply `Incapacitate` aura to triggering enemy via `AuraPending`, despawn trap
  - On trigger for Frost: spawn `SlowZone` entity at trap `Transform::translation`, despawn trap
  - Skip unarmed traps (enemy walks through before armed = no trigger)
  - Skip friendly combatants (check `owner_team != target_team`)
  - Trap triggers and is consumed even if target is immune (Divine Shield, DR immune)
  - Include combat log entries for trap placement and trigger
- [ ] `slow_zone_system()` — single system for zone lifecycle:
  - Decrement `duration_remaining`, despawn when expired
  - For enemies inside radius: refresh `MovementSpeedSlow` aura duration (no new DR application)
  - When enemy exits zone: aura naturally expires (don't forcibly remove — let duration handle it)
  - Zone persists after owner dies
- [ ] Add `pub mod traps;` to `play_match/mod.rs`
- [ ] **CRITICAL: Dual registration** — register BOTH systems in:
  - `systems.rs::add_core_combat_systems()` (headless)
  - `states/mod.rs` (graphical)
  - **System phase assignment (corrected from original plan):**
    - `trap_system` → **Phase 2** (CombatAndMovement), after `move_to_target` — needs current positions
    - `slow_zone_system` → **Phase 1** (ResourcesAndAuras), before `apply_pending_auras` — zone ticks are aura-adjacent

**Dead Zone Enforcement** (in `combat_core.rs`):
- [ ] Add dead zone check to `combat_auto_attack()` at `combat_core.rs:645`:
  ```rust
  // After existing range check:
  if combatant.class == CharacterClass::Hunter && distance < HUNTER_DEAD_ZONE {
      continue; // In dead zone, can't auto-attack
  }
  ```
- [ ] Add third auto-attack name branch: Hunter uses "Auto Shot" (not "Wand Shot") in combat log
  - Add `AUTO_SHOT_RANGE` as Hunter's ranged auto-attack range (35.0 instead of WAND_RANGE 30.0)
- [ ] `min_range` enforcement already handled via `AbilityConfig` field added in Phase 1
  - Aimed Shot, Arcane Shot, Concussive Shot all have `min_range: Some(8.0)`
  - Traps and Disengage have no min_range (work in dead zone)

**Disengage Mechanic** (in `combat_core.rs`):
- [ ] Add `DisengagingState` handling in `move_to_target()`, after `ChargingState` block (~line 351):
  ```rust
  if let Ok(disengaging) = disengaging_query.get(entity) {
      let new_pos = transform.translation + disengaging.direction * disengaging.speed * dt;
      // Clamp to arena bounds
      new_pos.x = new_pos.x.clamp(-ARENA_HALF_X, ARENA_HALF_X);
      new_pos.z = new_pos.z.clamp(-ARENA_HALF_Z, ARENA_HALF_Z);
      transform.translation = new_pos;
      // ... decrement distance_remaining, remove component when done
      continue; // Skip normal movement
  }
  ```
- [ ] Use `normalize_or_zero()` when creating DisengagingState direction (prevent NaN if positions equal)
- [ ] Add CC immunity during Disengage — extend `apply_pending_auras` in `auras.rs:128`:
  ```rust
  let is_unstoppable = charging_query.get(pending.target).is_ok()
      || disengaging_query.get(pending.target).is_ok();
  ```
- [ ] Disengage does NOT work while rooted (movement system already prevents rooted movement — `DisengagingState` insertion should check root status)

**Trap limit enforcement** (inline, not a separate system):
- [ ] When Hunter AI triggers `GroundTargetAbility` for a trap, check for existing traps of same type from same owner
- [ ] If found, despawn the old trap before spawning the new one
- [ ] This is a 3-line check at spawn time, not a per-frame system

**Success criteria:** Traps arm, trigger on proximity, apply correct effects. Slow zones persist and slow enemies. Dead zone prevents auto-attack and ranged abilities within 8 yards. Disengage leaps backward, clamped to bounds.

---

#### Phase 3: AI — Hunter Decision Logic, Kiting, Pet AI

Build the Hunter's brain and all three pet AIs. Combined since they're interdependent.

**Tasks:**

**Hunter AI** (create `src/states/play_match/class_ai/hunter.rs`):
- [ ] `pub struct HunterAI;` implementing `ClassAI` trait
- [ ] `pub fn decide_hunter_action()` following `decide_warrior_action()` signature pattern
- [ ] Range zone detection: distance to nearest enemy → categorize:
  - Safe (20-40 yards): full rotation available
  - Closing (8-20 yards): kite + instants
  - Dead zone (<8 yards): escape only
- [ ] **Dead zone priority (<8 yards):**
  1. Disengage (if off cooldown AND not rooted)
  2. Frost Trap at current position (if off cooldown)
  3. Set `kiting_timer` (trigger flee behavior)
- [ ] **Closing range priority (8-20 yards):**
  1. Concussive Shot on nearest enemy (if not already slowed, in min_range)
  2. Frost Trap between self and nearest enemy (midpoint)
  3. Set `kiting_timer` (kite away)
  4. Arcane Shot on target while kiting (instant, if in min_range)
- [ ] **Safe range priority (20-40 yards):**
  1. Concussive Shot (if target not slowed)
  2. Freezing Trap on healer/CC target (midpoint between self and target)
  3. Frost Trap between self and approaching enemy (midpoint)
  4. Aimed Shot (if target slowed AND not closing fast — safe to hardcast 2.5s)
  5. Arcane Shot (instant filler)
  6. Auto Shot runs automatically via auto-attack system
- [ ] **V1 trap placement** (skip predictive pathing):
  - Frost Trap: midpoint between Hunter and nearest approaching enemy
  - Freezing Trap: at CC target's current position (healer priority)
  - Can refine with velocity prediction post-v1 if traps are too easy to avoid
- [ ] Kiting: set `kiting_timer` when nearest enemy enters closing range
  - Hunter kites more aggressively than Mage (kiting_timer = 3.0 whenever enemy < 20 yards)
  - `find_best_kiting_direction()` already exists in `combat_core.rs`
- [ ] Add `pub mod hunter;` and `HunterAI` to `class_ai/mod.rs`
- [ ] Add `get_class_ai()` dispatch: `CharacterClass::Hunter => Box::new(hunter::HunterAI)`
- [ ] Add Hunter dispatch block in `combat_ai.rs::decide_abilities()`

**Pet AI** (extend `pet_ai.rs`):
- [ ] Add dispatch arms in `pet_ai_system()`:
  ```rust
  PetType::Spider => spider_ai(/* ... */),
  PetType::Boar => boar_ai(/* ... */),
  PetType::Bird => bird_ai(/* ... */),
  ```
- [ ] `spider_ai()`:
  - Melee auto-attack (existing)
  - **Web** (45s CD): Use when enemy is within 15 yards of **owner** (not spider) AND approaching
  - Don't use on already-rooted/heavily-slowed targets
  - Target: enemy closest to owner (defensive bodyguard)
  - Pet must look up owner via `Pet.owner` field for position/aura checks
- [ ] `boar_ai()`:
  - Melee auto-attack (existing)
  - **Charge** (25s CD): Use `is_charge: true` pattern (existing Warrior Charge)
  - Primary: Charge enemy mid-cast (especially healers — check `CastingState`)
  - Secondary: Charge primary kill target (owner's target)
  - Min range: CHARGE_MIN_RANGE (8.0) — same as Warrior
- [ ] `bird_ai()`:
  - Melee auto-attack (existing)
  - **Master's Call** (45s CD): Remove `MovementSpeedSlow` and `Root` auras from target
  - Does NOT remove `Stun` or `Fear` (movement impairments only)
  - Does NOT grant immunity after cleanse (simplest v1)
  - Primary: Use when owner has Root or MovementSpeedSlow aura
  - Secondary: Use on teammate with movement impairments (if owner is clean)
- [ ] Add pet spawning for Hunter in `play_match/mod.rs::setup_play_match()` (graphical)
- [ ] Add pet spawning for Hunter in `headless/runner.rs::headless_setup_match()` (headless)
- [ ] Default pet type: `PetType::Boar` when not specified in config

**Success criteria:** Hunter maintains 20-35 yard range, uses abilities in correct priority per range zone, places traps at midpoints, kites when enemies approach. All three pet types auto-attack and use their special abilities correctly.

---

#### Phase 4: Config UI & Integration

Wire up UI, headless config, icons, and polish.

**Tasks:**

- [x] Add Hunter pet selection to `configure_match_ui.rs` (follow Rogue opener / Warlock curse UI pattern)
- [x] Add Hunter color (#ABD473) to `spawn_combatant` in `play_match/mod.rs` (mesh color)
- [x] Verify combat log entries:
  - "Auto Shot" name for Hunter ranged auto-attacks
  - Trap placement logged: "[TRAP] Hunter places Freezing Trap at (x, z)"
  - Trap trigger logged: "[TRAP] Freezing Trap triggered by Warrior"
  - Slow zone logged: "[ZONE] Frost Trap slow zone applied to Warrior"
  - Pet ability usage logged normally through existing combat log
  - Disengage logged: "[MOVE] Hunter disengages 15.0 yards"
- [x] Add Hunter to UI class descriptions
- [x] Verify `PlayMatchEntity` marker on ALL trap/slowzone entities
- [x] Icon downloads and path mappings for:
  - Aimed Shot: `inv_spear_07.jpg`
  - Arcane Shot: `ability_impalingbolt.jpg`
  - Concussive Shot: `spell_frost_stun.jpg`
  - Disengage: (generic hunter icon)
  - Freezing Trap: `spell_frost_chainsofice.jpg`
  - Frost Trap: `spell_frost_freezingbreath.jpg`

---

#### Phase 5: Headless Testing at Scale

Systematic testing across all matchups. **Use agent teams to parallelize bug detection and fixing.**

**Tasks:**

- [ ] Run Hunter vs every class (1v1): Warrior, Mage, Rogue, Priest, Warlock, Paladin, Hunter
- [ ] Run Hunter in 2v2 comps: Hunter+Priest, Hunter+Paladin, Hunter+Warrior (vs various)
- [ ] Run Hunter in 3v3 comps (mixed, including multiple Hunters)
- [ ] Test on both maps: BasicArena and PillaredArena
- [ ] Test all three pet types (Spider, Boar, Bird) in representative matchups
- [ ] Verify no panics, no infinite loops, matches complete within timeout
- [ ] Verify combat log correctness (see Phase 4 log entries)
- [ ] **Spawn parallel agent team for bug fixing:**
  - Agent 1: Fix AI/behavior bugs (hunter.rs, pet_ai.rs)
  - Agent 2: Fix systems/ECS bugs (traps.rs, combat_core.rs)
  - Agent 3: Fix config/balance issues (abilities.ron, constants.rs)

**Critical edge cases to verify:**
- [ ] Hunter vs Hunter mirror match — 4 traps on field, trap ownership correct
- [ ] Freezing Trap + ally damage → breaks CC (break_on_damage: 0.0)
- [ ] Disengage at arena edge → position clamped to bounds
- [ ] Frost Trap zone + Frost Nova slows → multiply correctly (0.5 * 0.4 = 0.2 speed)
- [ ] Pet dying mid-fight → no crash, Hunter loses that pet's toolkit
- [ ] Hunter OOM → Auto Shot still fires (free), no mana abilities used
- [ ] Boar Charge on moving target → charge tracking works
- [ ] Master's Call on stunned Hunter → does NOT cleanse stun (only roots/slows)
- [ ] Enemy walks through unarmed trap → no trigger (arm_timer > 0)
- [ ] Pets trigger enemy traps → correct (pets are enemy combatants)
- [ ] Freezing Trap on Divine Shield target → trap consumed, CC fails
- [ ] Warrior Charge into dead zone → Hunter AI immediately Disengages
- [ ] Rogue Cheap Shot opener → Hunter stunned in dead zone, pet can still act
- [ ] Mage Frost Nova roots Hunter at 10 yards → Hunter can still Arcane Shot (>= 8yd)
- [ ] Mage Frost Nova roots Hunter at 5 yards → Hunter cannot use ranged abilities, Disengage blocked by root, needs Bird pet or wait
- [ ] Aimed Shot cast interrupted by closing enemy entering dead zone → cast continues (in-progress casts not interrupted by dead zone)
- [ ] Frost Trap zone DR → no re-application spam (zone-managed refresh, not new applications)

**Success criteria:** All matchups complete without crashes. Combat logs show expected ability usage patterns. Win rates are reasonable (no class is dominant >70%).

## Acceptance Criteria

### Functional Requirements

- [ ] Hunter appears as 7th class in class selection (UI + headless config)
- [ ] All 6 Hunter abilities function correctly (Aimed Shot, Arcane Shot, Concussive Shot, Disengage, Freezing Trap, Frost Trap)
- [ ] Auto Shot works as ranged auto-attack with dead zone (8-35 yard range)
- [ ] Dead zone prevents ranged abilities within 8 yards (via `min_range` in AbilityConfig)
- [ ] Disengage leaps Hunter backward ~15 yards, clamped to arena, CC immune during leap
- [ ] Freezing Trap incapacitates first enemy to enter trigger radius (breaks on damage)
- [ ] Frost Trap creates persistent slow zone on trigger (~10s duration, 8yd radius, 60% slow)
- [ ] One trap of each type max per Hunter (second despawns first)
- [ ] All three pet types (Spider, Boar, Bird) function with correct special abilities
- [ ] Pet choice configurable via match config JSON and UI
- [ ] Hunter AI maintains 20-35 yard range through kiting
- [ ] `AuraType::Incapacitate` works correctly with DR system (shares `Incapacitates` category)

### Non-Functional Requirements

- [ ] No performance regression (max ~30 entities in worst case — negligible overhead)
- [ ] All systems registered in BOTH `systems.rs` (headless) and `states/mod.rs` (graphical)
- [ ] Combat logs include correct caster attribution for all trap/pet effects
- [ ] Compiles on `cargo build --release` with no warnings

### Quality Gates

- [ ] All existing tests pass (`cargo test`)
- [ ] Headless simulations complete for Hunter vs all 7 classes (1v1)
- [ ] No panics in any test configuration
- [ ] Match durations are reasonable (not stalemates)

## Dependencies & Prerequisites

- Existing pet system (Warlock Felhunter) — reused for all Hunter pets
- Existing aura system — provides Root, Stun, MovementSpeedSlow, HealingReduction (add Incapacitate)
- Existing kiting logic (Mage `kiting_timer`) — extended for Hunter
- Existing charge mechanic (Warrior Charge) — reused for Boar Charge

## Risk Analysis & Mitigation

| Risk | Impact | Mitigation |
|------|--------|------------|
| Trap proximity checks per frame | Low | Max ~6 traps, ~6 enemies = 36 distance checks. Negligible at 60fps. |
| Dead zone makes Hunter too weak vs melee | Medium | Disengage (25s CD), pet CC, traps help escape. Tune dead zone range if needed. |
| Hunter AI kiting too effective | Medium | Tune Concussive Shot CD (12s), Disengage CD (25s). Warriors have Charge. |
| Freezing Trap too strong CC | Medium | Breaks on ANY damage (0.0), 1.5s arm delay, 5yd trigger, shares Incapacitate DR. |
| Frost Trap zone DR spam | Medium | Zone-managed aura refresh (no new DR applications while inside). |
| Dual system registration forgotten | High | Explicit checklist in Phase 2. Test BOTH headless and graphical. |
| Disengage NaN direction | Low | Use `normalize_or_zero()` with team-side fallback direction. |
| Incapacitate aura missing integration points | Medium | Checklist: `from_aura_type`, `is_incapacitated`, `apply_pending_auras`, `move_to_target`. |

## References & Research

### Internal References
- Paladin class implementation pattern: `docs/solutions/implementation-patterns/adding-new-class-paladin.md`
- Visual effects pattern: `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md`
- Dual system registration: `docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md`
- Critical hit distributed damage: `docs/solutions/implementation-patterns/critical-hit-system-distributed-crit-rolls.md`
- Two-agent bug hunting: `docs/solutions/workflows/two-agent-bug-hunting.md`
- Mage kiting AI: `src/states/play_match/class_ai/mage.rs:445,597`
- Pet AI system: `src/states/play_match/class_ai/pet_ai.rs`
- Auto-attack system: `src/states/play_match/combat_core.rs:645`
- Charge mechanic: `src/states/play_match/combat_core.rs:351`
- CharacterClass enum: `src/states/match_config.rs:62`
- AbilityType enum: `src/states/play_match/abilities.rs:42`
- AbilityDecision enum: `src/states/play_match/class_ai/mod.rs:223`

### WoW Classic Spell Data
- Aimed Shot (ID 19434): 3s cast, 75 mana, 35yd range, 6s CD
- Arcane Shot (ID 3044): Instant, 25 mana, 35yd, 6s CD, Arcane school
- Concussive Shot (ID 5116): 35yd, 12s CD, 50% slow 4s
- Freezing Trap (ID 1499): 50 mana, 15s CD, 10s freeze, breaks on damage
- Frost Trap (ID 13809): 60 mana, 15s CD, 30s zone, 10yd radius, 60% slow

### Brainstorm
- Hunter class brainstorm: `docs/brainstorms/2026-02-22-hunter-class-brainstorm.md`
