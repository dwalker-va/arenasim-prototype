---
title: "refactor: Codebase Quality Improvements"
type: refactor
status: completed
date: 2026-03-08
origin: docs/brainstorms/2026-03-08-codebase-refactoring-brainstorm.md
deepened: 2026-03-08
---

# Refactor: Codebase Quality Improvements

## Enhancement Summary

**Deepened on:** 2026-03-08
**Research agents used:** architecture-strategist, pattern-recognition-specialist, code-simplicity-reviewer, performance-oracle, best-practices-researcher, learnings-researcher

### Key Improvements from Research
1. **Item 6 simplified**: Drop the builder pattern — just migrate eligible sites to `from_ability()`, leave ~6 custom sites as explicit struct literals (simplicity reviewer)
2. **Item 8 expanded**: Must consolidate duplicate SystemSet enums (`PlayMatchSystems` vs `CombatSystemPhase`) and split `animate_orb_consumption` into gameplay + visual parts (architecture reviewer)
3. **Items 9-10 reduced**: Fewer, larger submodules — 5-6 files for components (not 10), 5 files for combat_core (not 8) (simplicity reviewer)
4. **Item 7 improved**: Accept `Option<(u8, CharacterClass)>` for target to also eliminate target_id boilerplate (pattern reviewer)
5. **Item 4 refined**: Keep `is_team_healthy()` as a named wrapper for readability (architecture + pattern reviewers)
6. **Item 5 expanded**: Also update `class_ai/mod.rs` module doc comment which references the deleted trait (architecture reviewer)

### New Considerations Discovered
- Existing plan for class AI parameter explosion: `docs/plans/2026-02-10-refactor-class-ai-parameter-explosion-plan.md` — related but out of scope
- Additional duplication patterns found (ability preamble/postamble, redundant `info!()` logging) — noted for future, not added to current scope
- Plugin-per-feature architecture (idiomatic Bevy) would be the ideal long-term solution for dual registration — Item 8 is a stepping stone

## Overview

A prioritized backlog of 9 refactoring items (original item 2 folded into item 7) to reduce duplication, improve file organization, and eliminate footguns. Each item is independent and can be implemented incrementally. Items are grouped into three phases to minimize rework and merge conflicts.

(See brainstorm: `docs/brainstorms/2026-03-08-codebase-refactoring-brainstorm.md`)

## Problem Statement

The codebase has grown organically over 16+ sessions. Key pain points:
- **Duplication**: AuraPending construction (17 manual sites), dispel logic (~80 lines duplicated), heal target finding (4+ copy-paste blocks), casting boilerplate (10 sites), logging boilerplate (46 call sites)
- **Dead code**: ClassAI trait with 7 stub implementations never used at runtime
- **Footguns**: Dual system registration requires updating 2 files for every new combat system; inconsistent `caster_id` construction (2 patterns for same string)
- **File size**: `combat_core.rs` (2672 lines), `components/mod.rs` (1642 lines)

## Implementation Phases

### Phase A: Safe, Independent Refactors

Items that touch few files and have no overlap with each other. Can be done in any order.

---

#### Item 5: Remove Dead ClassAI Trait

**Files touched**: `class_ai/mod.rs`, all 7 class AI files

**Problem**: `ClassAI` trait (line 256), `AbilityDecision` enum (line 224), `get_class_ai()` factory (line 264), and 7 stub `impl ClassAI for *AI` blocks are never used. All real logic lives in standalone `decide_*_action()` functions.

**Solution**: Delete the following from each file:
- `class_ai/mod.rs`: `AbilityDecision` enum, `ClassAI` trait, `get_class_ai()` factory
- `class_ai/mod.rs`: Update module doc comment (lines 1-15) which references "Each class has its own module that implements the `ClassAI` trait" — rewrite to describe the actual standalone-function architecture
- Each class AI file: The `*AI` struct (e.g., `pub struct PriestAI;`) and its `impl ClassAI` block

**Verification**: `AbilityDecision` is only used in the dead stubs — confirmed by grep. No runtime code references it.

> **Research insight**: This is the safest, lowest-risk item. Do it first as a confidence builder. (simplicity reviewer)

---

#### Item 1: Extract Shared Dispel Logic + Relocate DispelPending

**Files touched**: `class_ai/mod.rs`, `class_ai/priest.rs`, `class_ai/paladin.rs`, `components/mod.rs`

**Problem**: `try_cleanse` (paladin.rs:821-921) and `try_dispel_magic` (priest.rs:427-562) are nearly identical. `DispelPending` is defined in `priest.rs` but used by Paladin, Felhunter, and Bird — fragile coupling.

**Solution**:
1. Move `DispelPending` from `priest.rs` to `components/mod.rs` (it's a `#[derive(Component)]` — belongs with other components). Preserve the `log_prefix` field.
2. Extract `try_dispel_ally()` in `class_ai/mod.rs` parameterized by:
   - `ability_type: AbilityType` (DispelMagic vs PaladinCleanse)
   - `log_prefix: &str` ("[DISPEL]" vs "[CLEANSE]")
   - `log_name: &str` ("Dispel Magic" vs "Cleanse")
   - `caster_class: CharacterClass`
3. Use `highest_priority = -1` (Priest's initialization — more permissive, and `min_priority` already gates dispatch)
4. Use the Paladin's cleaner implementation as the base (it spawns `DispelPending` directly without the redundant aura-indices pre-check that Priest has)
5. Update `priest.rs` and `paladin.rs` to call the shared function
6. Update imports in `paladin.rs` (currently `use super::priest::DispelPending`)

**Regression check**: The `highest_priority` initialization difference (-1 vs 0) is the main behavioral risk. Using -1 is strictly more permissive — it allows priority-0 auras to be matched. Verify with a headless match that includes dispellable debuffs.

> **Research insight** (learnings): `DispelPending` has a `log_prefix` field that must be preserved during relocation. The Paladin implementation added during session 12 was intentionally cleaner than the original Priest version. (adding-new-class-paladin.md)

---

#### Item 4: Add `CombatContext::lowest_health_ally_below()`

**Files touched**: `class_ai/mod.rs`, `class_ai/priest.rs`, `class_ai/paladin.rs`

**Problem**: 4+ copy-paste blocks for "find lowest HP ally within range below threshold X":
- `priest.rs` `try_flash_heal()` (lines 600-619)
- `paladin.rs` `try_flash_of_light()` (lines 404-417)
- `paladin.rs` `try_holy_light()` (lines 480-493)
- `paladin.rs` `try_holy_shock_heal()` (lines 550-563)

**Current `lowest_health_ally()`** (class_ai/mod.rs:197-201): Returns lowest HP ally with no filtering.

**Solution**: Add to `CombatContext`:
```rust
// class_ai/mod.rs
pub fn lowest_health_ally_below(
    &self,
    max_hp_pct: f32,
    max_range: f32,
    my_pos: Vec3,
) -> Option<(Entity, &CombatantInfo)> {
    self.alive_allies()
        .into_iter()
        .filter(|(_, info)| {
            !info.is_pet
                && (info.current_health / info.max_health) < max_hp_pct
                && my_pos.distance(info.position) <= max_range
        })
        .min_by(|(_, a), (_, b)| a.health_pct().partial_cmp(&b.health_pct()).unwrap())
}

/// Returns true if all allies are above the given HP threshold.
pub fn is_team_healthy(&self, threshold: f32, my_pos: Vec3) -> bool {
    self.lowest_health_ally_below(threshold, f32::MAX, my_pos).is_none()
}
```

Consolidate `is_team_healthy()` (class_ai/mod.rs) and `allies_are_healthy()` (paladin.rs) into the new `is_team_healthy()` wrapper above. Replace direct calls with the wrapper for readability.

> **Research insight**: Both architecture and pattern reviewers agreed: keep `is_team_healthy()` as a named wrapper rather than forcing callers to reason about `lowest_health_ally_below(...).is_none()`. A one-line wrapper preserves readability at zero cost.

---

### Phase B: Overlapping Call Sites

Items that touch the same code regions. Do them together to minimize rework.

---

#### Item 3: Add `CastingState::new()` Constructor

**Files touched**: `components/mod.rs` (struct definition), 6 class AI files, `combat_core.rs`

**Problem**: 10 identical construction blocks:
```rust
let cast_time = calculate_cast_time(def.cast_time, auras);
commands.entity(entity).insert(CastingState {
    ability,
    time_remaining: cast_time,
    target: Some(target_entity),
    interrupted: false,
    interrupted_display_time: 0.0,
});
combatant.global_cooldown = GCD;
```

**Solution**: Add constructor to `CastingState`:
```rust
// components/mod.rs
impl CastingState {
    pub fn new(ability: AbilityType, cast_time: f32, target: Option<Entity>) -> Self {
        Self {
            ability,
            time_remaining: cast_time,
            target,
            interrupted: false,
            interrupted_display_time: 0.0,
        }
    }
}
```

Construction becomes: `commands.entity(entity).insert(CastingState::new(ability, cast_time, Some(target)));`

**Note**: The `combatant.global_cooldown = GCD` line varies per site (some set GCD, some don't). Don't include it in the constructor — keep it at call sites.

> **Research insight**: Clean, textbook constructor extraction. No concerns from any reviewer. (pattern + simplicity reviewers)

---

#### Item 6: Migrate to `AuraPending::from_ability()` Everywhere

**Files touched**: `components/mod.rs`, `warrior.rs`, `mage.rs`, `rogue.rs`, `priest.rs`, `paladin.rs`, `combat_core.rs`, `projectiles.rs`, `traps.rs`

**Problem**: 3 factory methods exist (`from_ability`, `from_ability_dot`, `from_ability_with_name`) but only 1 of ~20 manual construction sites uses them.

**Eligible sites** (~12-14 of ~20): Any construction that reads from `AbilityConfig` aura fields and uses standard defaults.

**Ineligible sites** (~6-8): Custom auras not driven by ability config:
- Weakened Soul (priest.rs:386-402) — hardcoded duration, `spell_school: None`, `break_on_damage_threshold: -1.0`
- SpellSchoolLockout (combat_core.rs:1248-1264) — computed magnitude (locked school as f32)
- Freezing Trap (traps.rs:105) — hardcoded duration/magnitude from trap component
- Any aura with `caster: None` or fully custom fields

**Solution**: Migrate eligible sites to `from_ability()` / `from_ability_dot()` / `from_ability_with_name()`. Leave the ~6 ineligible sites as explicit struct literals — they are intentionally custom, and the explicitness is a feature.

> **Research insight** (simplicity reviewer): The previously proposed builder pattern (4 methods for 6 call sites) was a YAGNI violation. Six custom constructions do not justify a builder. The explicit struct literals make each custom aura's intent clear. If custom aura types grow significantly in the future, revisit then — not now.

> **Research insight** (performance oracle): The builder pattern would have had zero measurable overhead (consuming builder compiles to identical codegen), but it was unnecessary complexity regardless.

---

#### Item 7: Add Shared `log_ability_use()` Helper (subsumes Item 2)

**Files touched**: `utils.rs`, all 7 class AI files, `combat_ai.rs`, `combat_core.rs`, `pet_ai.rs`

**Problem**: 46 logging call sites repeat caster_id/target_id construction + `log_ability_cast()` call. Additionally, 15 sites use inline `format!("Team {} {}", ...)` instead of `combatant_id()`.

**Decision**: Item 2 (standardize caster_id) is folded into this item. The helper constructs both `combatant_id()` and target_id internally, eliminating both problems.

**Solution**: Add to `utils.rs`:
```rust
pub fn log_ability_use(
    combat_log: &mut CombatLog,
    caster_team: u8,
    caster_class: CharacterClass,
    ability_name: &str,
    target: Option<(u8, CharacterClass)>,
    verb: &str,  // "casts", "uses", "begins casting"
) {
    let caster_id = combatant_id(caster_team, caster_class);
    let target_id = target.map(|(team, class)| combatant_id(team, class));
    let message = match &target_id {
        Some(tid) => format!("{} {} {} on {}", caster_id, verb, ability_name, tid),
        None => format!("{} {} {}", caster_id, verb, ability_name),
    };
    combat_log.log_ability_cast(caster_id, ability_name.to_string(), target_id, message);
}
```

> **Research insight** (pattern reviewer): Accept `Option<(u8, CharacterClass)>` for target instead of `Option<String>`. This eliminates the target_id `format!()` boilerplate at call sites too — the helper constructs both IDs internally via `combatant_id()`. The `verb` parameter is acceptable given only 3 fixed values; an enum would add ceremony for little benefit.

> **Research insight** (learnings): The crit system previously broke 28 logging call sites when `is_crit` was added to `log_damage()`. The `log_ability_use()` helper should be designed so that future parameter additions are additive (with defaults), not breaking. (critical-hit-system-distributed-crit-rolls.md)

**Migration**: Touch all 46 call sites. This is the highest-churn item — do it when no feature branches are in flight.

---

### Phase C: Structural Refactors

Items that change file organization and system registration. Do item 8 before items 9-10. **Verify item 8 works correctly before starting items 9-10** — file splits make debugging system registration issues dramatically harder.

---

#### Item 8: Unify Dual System Registration

**Files touched**: `states/mod.rs`, `states/play_match/systems.rs`, `states/play_match/shadow_sight.rs`

**Problem**: `states/mod.rs` (graphical) duplicates the combat system list from `systems.rs` (headless) and interleaves visual systems. Adding a combat system to only one location causes silent bugs. `animate_orb_consumption` appears in both headless Phase 2 AND graphical resolution phase.

**Solution**:

**Step 0 — Prerequisites (must do first):**
1. **Consolidate SystemSet enums**: Delete `PlayMatchSystems` from `states/mod.rs` and use `CombatSystemPhase` (from `systems.rs`) everywhere. Without this, systems registered via `add_core_combat_systems()` use `CombatSystemPhase` sets while graphical-only systems use `PlayMatchSystems` sets — Bevy treats these as independent ordering constraints with **no cross-set ordering**.
2. **Split `animate_orb_consumption`**: The system both moves entities (Transform) and despawns entities (`commands.entity(entity).despawn_recursive()`) in `shadow_sight.rs:237-268`. Extract the timer countdown + despawn into `cleanup_consumed_orbs()` (core combat, both modes) and keep the Transform animation as `animate_orb_consumption()` (graphical only).

**Step 1 — Unify registration:**
3. Refactor `states/mod.rs` to call `add_core_combat_systems(app)` for combat systems
4. Register graphical-only systems separately in `states/mod.rs`:
   - Camera/UI systems (Phase 1 prepend): `handle_time_controls`, `handle_camera_input`, etc.
   - Visual systems (Phase 2 append): `spawn_projectile_visuals`, `animate_orb_consumption`
   - Resolution phase: all rendering/animation systems
   - Effect visual groups: shield bubbles, flames, healing lights, etc.
5. Verify system ordering is preserved — Bevy `.chain()` and `.before()/.after()` constraints must match
6. Preserve `apply_deferred` flush between phases

**Key constraint from learnings** (`docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md`):
- System ordering phases: Phase 1 (Resources/Auras) → Phase 2 (Combat Core) → Phase 3 (State Updates)
- Visual systems register ONLY in graphical mode
- Re-exports through module hierarchy: `pub use effects::*;` in `play_match/mod.rs`

> **Research insight** (architecture reviewer): The duplicate SystemSet enum is the **critical gap** in the original plan. `PlayMatchSystems` and `CombatSystemPhase` must be consolidated to a single enum or system ordering will silently break after unification. This is a prerequisite, not an afterthought.

> **Research insight** (best practices): The idiomatic Bevy approach is plugin-per-concern: `CombatCorePlugin` (both modes) + `CombatVisualsPlugin` (graphical only). Item 8 is a stepping stone toward this architecture — the immediate goal is single-source combat registration via `add_core_combat_systems()`, with a future migration to proper plugins.

> **Research insight** (performance oracle): `animate_orb_consumption` currently appears in both modes. After unification via `add_core_combat_systems()`, verify it does not run twice per frame in graphical mode (once from core registration, once from the existing resolution phase registration). Split the system first to avoid this.

**Verification**: After unification, run both modes and compare combat outcomes. The graphical mode should produce identical combat logs to headless mode.

---

#### Item 9: Split `components/mod.rs` (1642 lines)

**Files touched**: `components/mod.rs` → new submodules, all files importing `components::*`

**Target module layout** (5 content files + re-export hub):

| New file | Contents | Approx lines |
|----------|----------|-------------|
| `components/combatant.rs` | Combatant struct + impl, ResourceType, CastingState, ChannelingState, ChargingState, InterruptPending, DamageTakenThisFrame, Projectile, HolyShockHealPending, HolyShockDamagePending, DivineShieldPending, DispelPending | ~455 |
| `components/auras.rs` | AuraType, Aura (struct + impl), ActiveAuras, AuraPending (struct + factory methods), DRCategory, DRState, DRTracker | ~350 |
| `components/resources.rs` | GameRng, SimulationSpeed, DisplaySettings, CameraMode, CameraController, MatchCountdown, ShadowSightState, VictoryCelebration, MatchResults, CombatantStats, PlayMatchEntity, ArenaCamera, Celebrating, FloatingTextState, GateBar, SpeechBubble, ShadowSightOrb | ~275 |
| `components/pets.rs` | PetType, Pet, PetStats, pet stat definitions, TrapType, Trap, TrapLaunchProjectile, SlowZone, DisengagingState, TrapBurst, IceBlockVisual, DisengageTrail, ChargeTrail | ~435 |
| `components/visual.rs` | FloatingCombatText, SpellImpactEffect, DeathAnimation, ShieldBubble, OriginalMesh, PolymorphedVisual, FlameParticle, DrainLifeBeam, DrainParticle, HealingLightColumn, DispelBurst | ~75 |
| `components/mod.rs` | `pub use` re-exports only | ~15 |

> **Research insight** (simplicity reviewer): The original 10-file split was over-decomposed. Files under 100 lines (casting.rs at ~50, markers.rs at ~65, dr.rs at ~100) add navigation overhead without reducing cognitive load. Merging into 5 larger files (75-455 lines each) follows conceptual boundaries better.

> **Research insight** (architecture reviewer): Verify no circular `use` between new submodules. In Rust, sibling modules can reference each other via `super::other_module::Type`, but it's cleaner to have `mod.rs` re-export everything and have submodules import from the parent scope. Use `cargo check` — the compiler will tell you exactly what re-exports are missing.

**Import compatibility**: `mod.rs` will re-export everything via `pub use combatant::*; pub use auras::*;` etc. Existing `use components::*` imports continue to work unchanged.

**Note**: Existing `auras.rs` and `visual.rs` submodules are documentation stubs — they will be replaced with actual type definitions.

---

#### Item 10: Split `combat_core.rs` (2672 lines)

**Files touched**: `combat_core.rs` → new submodules, `systems.rs`, `states/mod.rs`

**Target module layout** (5 content files + utility hub):

| New file | Contents | Approx lines |
|----------|----------|-------------|
| `combat_core/damage.rs` | `roll_crit`, `apply_damage_with_absorb`, absorb helpers, damage formula functions, `process_interrupts`, `apply_interrupt_lockout` | ~340 |
| `combat_core/movement.rs` | `find_best_kiting_direction`, `move_to_target` (fear, poly, charge, disengage, kiting, melee follow, ranged positioning, arena bounds) | ~435 |
| `combat_core/auto_attack.rs` | `combat_auto_attack` (swing timer, wand, Heroic Strike, rage gen) | ~385 |
| `combat_core/casting.rs` | `process_casting`, `process_channeling`, `regenerate_resources`, `update_stealth_visuals` (cast completion, channeling ticks) | ~1025 |
| `combat_core/death.rs` | `trigger_death_animation`, `animate_death`, `ease_out_quad`, `despawn_pets_of_dead_owners` | ~430 |
| `combat_core/mod.rs` | Utility functions (`is_in_arena_bounds`, `clamp_to_arena`, `calculate_cast_time`, aura query helpers) + `pub use` re-exports | ~100 |

> **Research insight** (simplicity reviewer): The original 8-file split had 2 files under 50 lines (`resources.rs`, `stealth.rs`). Merging `resources` and `stealth` into `casting.rs` (single coherent "spell processing" concern) and merging `interrupts` into `damage.rs` (both are "combat resolution") yields 5 well-sized files. If `casting.rs` at ~1025 lines feels too big after extraction, split it then — not preemptively.

> **Research insight** (performance oracle): Splitting files has zero effect on inlining or codegen. Rust's compilation unit is the crate, not the file. LLVM performs cross-module inlining regardless of file boundaries. Splitting may actually *improve* incremental compile times.

**Import compatibility**: Same `pub use` re-export strategy. `systems.rs` references these functions by name — re-exports ensure no changes needed.

---

## Acceptance Criteria

### Per-item criteria
- [ ] **Item 5**: ClassAI trait, AbilityDecision, get_class_ai(), all *AI structs removed; module doc comment updated
- [ ] **Item 1**: Shared `try_dispel_ally()` in `class_ai/mod.rs`; `DispelPending` moved to `components/mod.rs`; both Priest and Paladin use shared function
- [ ] **Item 4**: `lowest_health_ally_below()` on CombatContext; `is_team_healthy()` wrapper; 4+ heal functions use it; `allies_are_healthy` consolidated
- [ ] **Item 3**: `CastingState::new()` constructor exists; all 10 construction sites migrated
- [ ] **Item 6**: 12-14 eligible AuraPending sites use `from_ability()` variants; ~6 custom sites left as explicit struct literals
- [ ] **Item 7**: `log_ability_use()` helper exists; accepts `Option<(u8, CharacterClass)>` for target; 46 call sites migrated; no remaining inline `format!("Team {} {}", ...)` for caster_id
- [ ] **Item 8**: SystemSet enums consolidated; `animate_orb_consumption` split; `states/mod.rs` calls `add_core_combat_systems()`; only visual systems registered separately
- [ ] **Item 9**: `components/mod.rs` is a thin re-export hub; types split into 5 focused files
- [ ] **Item 10**: `combat_core.rs` is a thin re-export hub; functions split into 5 focused files

### Global criteria
- [ ] `cargo build --release` succeeds with no warnings
- [ ] `cargo clippy` passes
- [ ] Headless simulation produces identical combat behavior (test with Warrior vs Mage, Priest+Warrior vs Mage+Rogue, 3v3 all classes)
- [ ] Graphical client launches and runs a match with no visual regressions
- [ ] No new `pub` items added that weren't already public

## Risks and Mitigations

| Risk | Mitigation |
|------|-----------|
| Item 1: `highest_priority` init difference changes dispel targeting | Use -1 (more permissive); verify with headless match including dispellable debuffs |
| Item 7: High churn (46 sites across 10 files) | Do when no feature branches in flight; single focused PR |
| Item 8: SystemSet enum mismatch breaks ordering | Consolidate to single `CombatSystemPhase` enum as prerequisite step |
| Item 8: `animate_orb_consumption` runs twice in graphical | Split into gameplay + visual systems before unifying registration |
| Item 8: System ordering breaks | Preserve exact `.chain()` and `.before()/.after()` constraints; preserve `apply_deferred` flushes; test both modes |
| Items 9-10: Merge conflicts with feature branches | Do last; use re-exports for import compatibility |
| Items 9-10: Circular `use` between new submodules | Use `cargo check` to catch; re-export from `mod.rs` and import from parent scope |

## Recommended Implementation Order

1. **Item 5** — Dead code removal, zero risk, immediate clarity gain
2. **Item 1** — Real duplication removal with minimal parameterization
3. **Item 4** — Consolidates copy-paste heal targeting logic
4. **Item 3** — Simple constructor, moderate churn, clear benefit
5. **Item 6** — `from_ability()` migration only (no builder)
6. **Item 7** — Do last in Phase B due to high churn; simplify verb parameter
7. **Item 8** — Eliminates the active footgun; verify before proceeding
8. **Item 9** — File split with re-exports (5 files)
9. **Item 10** — File split with re-exports (5 files)

## Future Considerations (Out of Scope)

These patterns were identified during research but are intentionally excluded from this refactoring:
- **Class AI parameter explosion**: 40+ `try_*` functions take 7-10 identical parameters. An `AbilityContext` struct would reduce every signature. See existing plan: `docs/plans/2026-02-10-refactor-class-ai-parameter-explosion-plan.md`
- **Ability preamble/postamble**: 31 sites repeat "check lockout + check mana + check cooldown" and "deduct mana + set GCD + set cooldown". Could extract `ability_available()` and `consume_ability_resources()` helpers.
- **Redundant `info!()` logging**: Many `try_*` functions log the same message to both `CombatLog` and `info!()`.
- **Plugin-per-feature architecture**: The idiomatic Bevy pattern is `CombatCorePlugin` + `CombatVisualsPlugin`. Item 8 is a stepping stone.
- **Tuple creep in instant_attacks**: Adding fields to inline tuples is getting unwieldy; named structs would be better.

## Sources & References

### Origin
- **Brainstorm document**: [docs/brainstorms/2026-03-08-codebase-refactoring-brainstorm.md](docs/brainstorms/2026-03-08-codebase-refactoring-brainstorm.md) — Key decisions: prioritized backlog approach, remove ClassAI trait, standardize on existing helpers

### Internal References
- Dual registration pattern: `docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md`
- Visual effects pattern: `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md`
- Crit system logging lessons: `docs/solutions/implementation-patterns/critical-hit-system-distributed-crit-rolls.md`
- Paladin implementation patterns: `docs/solutions/implementation-patterns/adding-new-class-paladin.md`
- Parameter explosion plan: `docs/plans/2026-02-10-refactor-class-ai-parameter-explosion-plan.md`
- `AuraPending::from_ability()`: `src/states/play_match/components/mod.rs:941`
- `combatant_id()`: `src/states/play_match/utils.rs:22`
- `CombatContext::lowest_health_ally()`: `src/states/play_match/class_ai/mod.rs:197`
- `animate_orb_consumption`: `src/states/play_match/shadow_sight.rs:237-268`
- Headless system registration: `src/states/play_match/systems.rs:126-189`
- Graphical system registration: `src/states/mod.rs:105-280`
- SystemSet enums: `PlayMatchSystems` in `states/mod.rs:40`, `CombatSystemPhase` in `systems.rs:81`

### External References
- Bevy plugin-per-feature pattern: [Tainted Coders](https://taintedcoders.com/bevy/code-organization), [NiklasEi/bevy_game_template](https://github.com/NiklasEi/bevy_game_template)
- Bevy headless pattern: [Bevy headless example](https://github.com/bevyengine/bevy/blob/main/examples/app/headless.rs)
- Rust consuming builder pattern: [Rust API Guidelines](https://doc.rust-lang.org/1.0.0/style/ownership/builders.html), [Effective Rust Item 7](https://www.lurklurk.org/effective-rust/builders.html)
- Rust file splitting: [The Rust Book Ch 7.5](https://doc.rust-lang.org/book/ch07-05-separating-modules-into-different-files.html)
