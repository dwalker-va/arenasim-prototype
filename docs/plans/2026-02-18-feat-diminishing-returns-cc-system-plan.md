---
title: "feat: Add diminishing returns system for crowd control"
type: feat
date: 2026-02-18
brainstorm: docs/brainstorms/2026-02-18-diminishing-returns-cc-system-brainstorm.md
deepened: 2026-02-18
---

# feat: Add Diminishing Returns System for Crowd Control

## Enhancement Summary

**Deepened on:** 2026-02-18
**Review agents used:** Pattern Recognition, Performance Oracle, Code Simplicity, Architecture Strategist

### Key Improvements from Deepening
1. **Array instead of HashMap** — `[DRState; 5]` eliminates heap allocation, gives O(1) indexed access (~40 bytes inline vs ~64+ bytes HashMap overhead)
2. **Inline DR timer ticking** — Tick DR timers inside existing `update_auras()` instead of a separate system, avoiding dual-registration overhead
3. **CC replacement as prerequisite** — Separate the "new CC replaces old of same type" behavior into a small preparatory change
4. **Consolidated to 2 phases** — Down from 5, reducing integration risk
5. **swap_remove() for CC replacement** — O(1) removal since aura order doesn't matter

### Performance Assessment
Total DR system cost: ~50ns per frame (6 combatants x 5 categories = 30 Option checks). Fits entirely in L1 cache (240 bytes). **Will never be a bottleneck.**

---

## Overview

Implement a Classic WoW-faithful diminishing returns (DR) system so that successive CC of the same DR category on the same target has reduced duration, eventually granting full immunity. This prevents CC chains from permanently locking out a target and adds strategic depth to ability usage.

**DR Curve:** 100% → 50% → 25% → Immune
**Reset Timer:** 15 seconds from application time
**Categories:** Stuns, Fears, Incapacitates, Roots, Slows (each independent)

## Key Decisions (from brainstorm + SpecFlow resolution)

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | DR timer starts at **application time**, not expiration | WoW Classic-faithful. A 10s Polymorph leaves only 5s before DR resets. |
| 2 | Immune applications do **NOT** restart the 15s timer | Prevents griefing by spamming CC into immunity to extend it indefinitely. |
| 3 | New CC of same AuraType **replaces** existing CC | Without replacement, a 25% DR stun is meaningless if a full-duration stun is active. |
| 4 | DR modification happens in `apply_pending_auras()` | Single centralized interception point — all CC paths flow through it. |
| 5 | Divine Shield / Charge immunity does **NOT** advance DR | Blocked CC should not waste DR counters. Falls out automatically from existing early-return ordering in `apply_pending_auras()`. |
| 6 | Frostbolt slow immunity: damage still applies, slow blocked | Partial immunity — damage is separate from the slow aura. |
| 7 | Use "IMMUNE" terminology (not "RESISTED") | Consistent with WoW Classic and existing codebase immunity display. |
| 8 | DR timer ticking **inlined in `update_auras()`** | Avoids a separate system + dual registration overhead. Runs before `apply_pending_auras()` automatically. |
| 9 | DRTracker spawned on every combatant at match start | Simpler than lazy insertion — no `Option<&DRTracker>` queries. Include in spawn bundle to avoid archetype migration. |
| 10 | AI does NOT retarget CC based on DR level | Keeps AI simple. Reduced CC is still valuable per brainstorm decision 7. |

## Proposed Solution

### New Types

```rust
// src/states/play_match/components/mod.rs

/// DR categories — fixed enum with known size for array indexing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DRCategory {
    Stuns = 0,
    Fears = 1,
    Incapacitates = 2,
    Roots = 3,
    Slows = 4,
}

impl DRCategory {
    pub const COUNT: usize = 5;

    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_aura_type(aura_type: &AuraType) -> Option<DRCategory> {
        match aura_type {
            AuraType::Stun => Some(DRCategory::Stuns),
            AuraType::Fear => Some(DRCategory::Fears),
            AuraType::Polymorph => Some(DRCategory::Incapacitates),
            AuraType::Root => Some(DRCategory::Roots),
            AuraType::MovementSpeedSlow => Some(DRCategory::Slows),
            _ => None, // Non-CC auras (DoTs, buffs, lockouts) have no DR
        }
    }
}

/// Per-category DR state. 5 bytes per category, 25 bytes total for all 5.
#[derive(Debug, Clone, Copy, Default)]
pub struct DRState {
    pub level: u8,    // 0 = fresh, 1 = 50%, 2 = 25%, 3 = immune
    pub timer: f32,   // seconds remaining until reset (counts down from 15.0)
}

/// Fixed-size DR tracker. No heap allocation, fully inline in archetype table.
/// Uses [DRState; 5] indexed by DRCategory discriminant — O(1) access.
#[derive(Component, Debug, Clone)]
pub struct DRTracker {
    states: [DRState; DRCategory::COUNT],
}

impl Default for DRTracker {
    fn default() -> Self {
        Self {
            states: [DRState::default(); DRCategory::COUNT],
        }
    }
}

impl DRTracker {
    /// Apply a CC of the given category. Returns the duration multiplier.
    /// Advances DR level and resets the 15s timer (unless already immune).
    #[inline]
    pub fn apply(&mut self, category: DRCategory) -> f32 {
        let state = &mut self.states[category.index()];
        let multiplier = DR_MULTIPLIERS[state.level.min(3) as usize];
        if state.level < DR_IMMUNE_LEVEL {
            state.level += 1;
            state.timer = DR_RESET_TIMER;
        }
        // Immune applications do NOT restart the timer (decision #2)
        multiplier
    }

    #[inline]
    pub fn is_immune(&self, category: DRCategory) -> bool {
        self.states[category.index()].level >= DR_IMMUNE_LEVEL
    }

    /// Tick all DR timers. Called from update_auras() each frame.
    pub fn tick_timers(&mut self, dt: f32) {
        for state in &mut self.states {
            if state.timer > 0.0 {
                state.timer -= dt;
                if state.timer <= 0.0 {
                    state.level = 0;
                    state.timer = 0.0;
                }
            }
        }
    }

    /// Get current DR level for a category (for combat log display).
    #[inline]
    pub fn level(&self, category: DRCategory) -> u8 {
        self.states[category.index()].level
    }
}
```

### DR Constants

```rust
// src/states/play_match/constants.rs

pub const DR_RESET_TIMER: f32 = 15.0;
pub const DR_IMMUNE_LEVEL: u8 = 3;
pub const DR_MULTIPLIERS: [f32; 4] = [1.0, 0.5, 0.25, 0.0];
```

### Research Insights — Data Structure Choice

**Why array over HashMap (unanimous across 4 reviewers):**
- `[DRState; 5]` is ~40 bytes inline in the ECS archetype table
- `HashMap<DRCategory, DRState>` is ~64+ bytes of overhead before storing any data, plus heap pointer chase
- Array gives direct index lookup (`states[category as usize]`) — single instruction
- Mirrors the `ability_cooldowns: HashMap<AbilityType, f32>` pattern but optimized for the known-size enum
- Zero heap allocations, zero hashing, zero tombstones from insert/remove cycles

## Implementation Phases

### Phase 1: Core DR Mechanics (single commit)

**Files:**
- `src/states/play_match/components/mod.rs` — Add `DRCategory`, `DRState`, `DRTracker`
- `src/states/play_match/constants.rs` — Add `DR_RESET_TIMER`, `DR_IMMUNE_LEVEL`, `DR_MULTIPLIERS`
- `src/states/play_match/mod.rs` — Spawn DRTracker on combatants at match start
- `src/states/play_match/auras.rs` — DR timer ticking in `update_auras()`, DR application in `apply_pending_auras()`, CC replacement, IMMUNE FCT
- `src/combat/log.rs` — DR info in CC log messages

**Tasks:**

**Components & Constants:**
- [x] Define `DRCategory`, `DRState`, `DRTracker` in `components/mod.rs`
- [x] Add `DR_RESET_TIMER`, `DR_IMMUNE_LEVEL`, `DR_MULTIPLIERS` to `constants.rs`

**Spawning:**
- [x] Spawn `DRTracker::default()` on each combatant entity during match setup in `setup_play_match()` (both graphical in `mod.rs` and headless in `runner.rs`)
- [x] Include DRTracker in the initial spawn bundle (not inserted later) to avoid archetype migration
- [x] Also spawn on pets (Felhunter) — they are CC targets too

**DR Timer Ticking (inline in `update_auras()`):**
- [x] In `update_auras()` (`auras.rs:26`), add DR timer ticking for each combatant's DRTracker
- [x] Add `&mut DRTracker` to the `update_auras` query
- [x] Call `tracker.tick_timers(dt)` for each combatant in the existing loop
- [x] **No separate system needed** — avoids dual-registration pitfall from MEMORY.md

**DR Application in `apply_pending_auras()`:**
- [x] Add `&mut DRTracker` to the combatants query in `apply_pending_auras()`
- [x] After existing Divine Shield / Charge immunity checks (which `continue` early — DR naturally not advanced):
  1. Call `DRCategory::from_aura_type(&pending.aura.effect_type)` — if `None`, skip DR (non-CC aura)
  2. If `tracker.is_immune(category)`: despawn pending entity, spawn "IMMUNE" FCT (match existing Divine Shield pattern), log immune event, `continue`
  3. If not immune: `let multiplier = tracker.apply(category)` — modifies duration: `pending.aura.duration *= multiplier`
- [x] Extract a helper function `check_and_apply_dr()` to keep `apply_pending_auras()` manageable (~320 lines currently)

**CC Replacement (in `apply_pending_auras()`):**
- [x] When applying a CC aura, first remove any existing aura of the same `AuraType` from target's `ActiveAuras`
- [x] Use `swap_remove()` (not `retain()`) — O(1) removal, aura order doesn't matter:
  ```rust
  if let Some(pos) = active_auras.auras.iter().position(|a| {
      DRCategory::from_aura_type(&a.effect_type) == Some(category)
  }) {
      active_auras.auras.swap_remove(pos);
  }
  ```

**Combat Log:**
- [x] Include DR info in the existing `log_crowd_control()` message string parameter
- [x] Normal CC format: `[CC] Polymorph on Warrior (5.0s, DR: 50%)`
- [x] Immune CC format: `[CC] Polymorph IMMUNE on Warrior (DR immune)`
- [x] No new structured fields needed — DR info goes in the message string (no programmatic consumer exists today)

**IMMUNE Floating Combat Text:**
- [x] When DR immunity blocks CC, spawn FCT "IMMUNE" on target
- [x] Follow existing pattern from Divine Shield immunity in `apply_pending_auras()` (same code path, same FCT type)

**Unit Tests:**
- [x] Test `DRCategory::from_aura_type()` — all 5 CC types map correctly, non-CC types return `None`
- [x] Test `DRTracker::apply()` — returns 1.0, 0.5, 0.25, 0.0 for levels 0-3
- [x] Test `DRTracker::tick_timers()` — timer decrements, resets to level 0 when expired
- [x] Test `DRTracker::is_immune()` — true at level 3, false below

### Phase 2: AI DR Awareness (separate commit)

**Files:**
- `src/states/play_match/class_ai/mod.rs` — Add DR query helper to `CombatContext`
- `src/states/play_match/combat_ai.rs` — Build DR data into CombatContext
- `src/states/play_match/class_ai/mage.rs` — Polymorph, Frost Nova
- `src/states/play_match/class_ai/warlock.rs` — Fear
- `src/states/play_match/class_ai/rogue.rs` — Cheap Shot, Kidney Shot
- `src/states/play_match/class_ai/paladin.rs` — Hammer of Justice

**Tasks:**
- [x] Add `dr_trackers: &'a HashMap<Entity, &'a DRTracker>` to `CombatContext` (follows existing `active_auras` pattern)
- [x] Build `dr_trackers` map in `decide_abilities()` (`combat_ai.rs`) from `Query<&DRTracker>`
- [x] Add `is_dr_immune(&self, entity: Entity, category: DRCategory) -> bool` helper on `CombatContext`
- [x] In each class AI, add DR immunity check before CC abilities (~1 line per check):
  - Mage: skip Polymorph if `is_dr_immune(target, Incapacitates)`, skip Frost Nova if `is_dr_immune(target, Roots)`
  - Warlock: skip Fear if `is_dr_immune(target, Fears)`
  - Rogue: skip Cheap Shot if `is_dr_immune(target, Stuns)`, skip Kidney Shot if `is_dr_immune(target, Stuns)`
  - Paladin: skip Hammer of Justice if `is_dr_immune(target, Stuns)`

### Research Insight — Phase Ordering Note

Phase 1 is self-contained and fully testable via headless simulation before Phase 2. DR works correctly without AI awareness — AI will just occasionally waste CC into immunity, which Phase 2 fixes. This means Phase 1 can be merged and tested independently.

## Edge Cases (from SpecFlow analysis)

| Edge Case | Expected Behavior | Why |
|---|---|---|
| Two CCs of same category in same frame | First gets current DR, second sees incremented DR | `apply_pending_auras()` processes sequentially with mutable DRTracker |
| DR reset on same frame as new CC | CC gets full duration (level 0) | `update_auras()` (which ticks DR timers) runs before `apply_pending_auras()` in the Phase 1 chain |
| Dispel removes active CC | DR level unchanged, timer keeps counting from application time | DR state is on DRTracker, not on the aura itself |
| CC breaks on damage early | DR level unchanged, timer keeps counting from application time | Same as dispel — DR was already advanced at application time |
| Frostbolt into slow-immune target | Damage applies normally, slow aura blocked with IMMUNE FCT | DR check only blocks the aura; projectile damage is a separate system |
| Divine Shield blocks CC | DR NOT advanced | Divine Shield immunity check `continue`s before DR check runs |
| Charge immunity blocks CC | DR NOT advanced | Charge immunity check `continue`s before DR check runs |
| Pet (Felhunter) receives CC | Pet has own DRTracker, DR tracked independently | Pets spawned with DRTracker like any combatant |

## Acceptance Criteria

- [x] First CC application: full duration, combat log shows `DR: 100%`
- [x] Second CC (same category, <15s): 50% duration, log shows `DR: 50%`
- [x] Third CC (same category, <15s): 25% duration, log shows `DR: 25%`
- [x] Fourth CC (same category, <15s): blocked, "IMMUNE" FCT, log shows `DR immune`
- [x] After 15s from last application: DR resets, next CC gets full duration
- [x] Different DR categories are independent (Stun doesn't affect Fear DR)
- [x] Immune applications do NOT restart the 15s reset timer
- [x] DR timer starts at application time, not expiration
- [x] New CC of same AuraType replaces existing active CC of that type
- [x] Divine Shield / Charge immunity does NOT advance DR
- [x] AI skips CC on DR-immune targets but still casts at 50%/25%
- [x] Frostbolt into slow-immune target: damage applies, slow blocked with IMMUNE
- [x] Headless simulation produces correct DR behavior (test via match logs)
- [x] Graphical mode produces correct DR behavior (DR timer ticking is inline in `update_auras()` which is already dual-registered)
- [x] Unit tests pass for DRCategory, DRTracker, and timer logic
- [x] `cargo build --release` compiles without warnings

## Test Plan

```bash
# Test 1: Basic DR escalation — Warlock vs Warrior (Fear has 0 cooldown)
echo '{"team1":["Warlock"],"team2":["Warrior"]}' > /tmp/dr-test.json
cargo run --release -- --headless /tmp/dr-test.json
# Verify: Fear durations decrease (8.0s → 4.0s → 2.0s → IMMUNE)

# Test 2: Cross-category independence — Mage vs Warrior
echo '{"team1":["Mage"],"team2":["Warrior"]}' > /tmp/dr-test2.json
cargo run --release -- --headless /tmp/dr-test2.json
# Verify: Polymorph and Frost Nova root have independent DR

# Test 3: Multi-class stun stacking — Rogue+Paladin vs Mage
echo '{"team1":["Rogue","Paladin"],"team2":["Mage"]}' > /tmp/dr-test3.json
cargo run --release -- --headless /tmp/dr-test3.json
# Verify: Cheap Shot + Hammer of Justice share Stuns DR category

# Test 4: DR reset after 15s — long match with varied CC timing
echo '{"team1":["Warlock"],"team2":["Priest"],"max_duration_secs":120}' > /tmp/dr-test4.json
cargo run --release -- --headless /tmp/dr-test4.json
# Verify: DR resets visible in combat log when 15s elapses between Fears
```

## References

- **Brainstorm:** `docs/brainstorms/2026-02-18-diminishing-returns-cc-system-brainstorm.md`
- **Aura application system:** `src/states/play_match/auras.rs:74` (`apply_pending_auras`)
- **Aura update system:** `src/states/play_match/auras.rs:26` (`update_auras`)
- **AuraType enum:** `src/states/play_match/components/mod.rs:366`
- **ActiveAuras component:** `src/states/play_match/components/mod.rs:708`
- **CombatContext (AI):** `src/states/play_match/class_ai/mod.rs:105`
- **Combat log:** `src/combat/log.rs:57` (`CrowdControl` event)
- **System registration chain:** `src/states/play_match/systems.rs:121`
- **Dual registration pattern:** `docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md`
- **Critical hit distributed pattern:** `docs/solutions/implementation-patterns/critical-hit-system-distributed-crit-rolls.md`
- **Constants:** `src/states/play_match/constants.rs`
- **Combatant spawn (graphical):** `src/states/play_match/mod.rs:523`
- **Combatant spawn (headless):** `src/headless/runner.rs:157`
