---
title: "fix: Resolve P2 combat bugs (duplicate buffs, CC on dead, friendly poly break, dead caster cast, fear through bubble)"
type: fix
date: 2026-02-20
deepened: 2026-02-20
---

# Fix P2 Combat Bugs (BUG-4, BUG-5, BUG-7, BUG-8, BUG-10)

## Enhancement Summary

**Deepened on:** 2026-02-20
**Research agents used:** architecture-strategist, performance-oracle, code-simplicity-reviewer, pattern-recognition-specialist, framework-docs-researcher (Bevy ECS), spec-flow-analyzer

### Key Improvements from Research
1. **BUG-4**: Verify existing dedup first before adding code; use `u8` bitmask instead of `HashSet<u8>` (zero allocation)
2. **BUG-7**: Avoid adding `caster_team` to Aura struct — look up caster's team at runtime via existing `caster: Option<Entity>` field (avoids 26-file diff); use `HashMap<Entity, bool>` instead of cloning `Vec<Aura>` (avoids 35+ String allocations/frame)
3. **BUG-8**: Drop the completed_casts re-query (YAGNI — no code path reaches it); keep only the kill-handler approach
4. **BUG-10**: Deferred commands from `process_divine_shield` won't be visible to `apply_pending_auras` in the same chain — edge case documented

### New Considerations Discovered
- `Aura` struct has 26 direct construction sites + 3 factory methods — struct field additions have large blast radius
- `ability_name: String` on Aura causes heap allocation on every `Aura::clone()` — avoid cloning Vec<Aura> in hot paths
- Bevy `.chain()` auto-inserts `ApplyDeferred` between systems, but commands from earlier systems in the chain are NOT visible to later systems unless an explicit flush occurs

## Overview

Five P2 bugs from the 2026-02-20 headless match test run need fixing. Each represents a distinct combat system flaw: duplicate buff application, CC on dead combatants, friendly fire breaking CC, dead casters completing spells, and CC landing through Divine Shield immunity.

## Bug Summaries

| Bug | Title | Root Cause | Frequency |
|-----|-------|-----------|-----------|
| BUG-4 | Duplicate group buff at match start | Stale snapshot allows multiple casters to buff same target | 7 matches |
| BUG-5 | CC applied to dead combatants | Missing `is_alive()` guard in projectile aura spawn + `apply_pending_auras` | 10+ matches |
| BUG-7 | Friendly fire breaks own Polymorph | No friendly-CC awareness in auto-attack targeting | 1 match |
| BUG-8 | Dead caster's spell completes | `process_casting` runs before death-dealing systems | 3 matches |
| BUG-10 | Fear lands through Divine Shield | `process_divine_shield` runs AFTER `apply_pending_auras` | 2 matches |

---

## BUG-4: Duplicate Group Buff Application

**Files:** `src/states/play_match/combat_ai.rs`, `src/states/play_match/class_ai/warrior.rs`, `src/states/play_match/class_ai/paladin.rs`

### Root Cause

In `decide_abilities()` (`combat_ai.rs:383`), each combatant gets a stale `CombatContext` snapshot of auras built at the top of the function (line 346-361). When two combatants on the same team both want to cast a group buff (e.g., Warrior A + Warrior B both check `ctx.active_auras` for `AttackPowerIncrease`), both see "no buff exists" and both spawn `AuraPending` entities for all allies.

The existing dedup in `apply_pending_auras` (`auras.rs:266-322`) via `applied_buffs` HashSet already catches same-frame duplicates for `AttackPowerIncrease` and `DamageTakenReduction` (both are in the `is_buff_aura` match). **The first step is to verify whether this dedup is actually failing** by running the test scenario before writing new code.

### Fix

**Step 1: Verify the existing dedup.** Run the test case and check the match log for duplicate buff entries. If the `applied_buffs` HashSet in `apply_pending_auras` catches the duplicates correctly, BUG-4 may already be fixed or require a different root cause investigation.

**Step 2 (if dedup fails):** Follow the established `shielded_this_frame` / `fortified_this_frame` pattern (`combat_ai.rs:372-378`), but use a `u8` bitmask instead of a `HashSet<u8>` to avoid per-frame heap allocation:

```rust
// combat_ai.rs — in decide_abilities(), near line 378
// Bitmask: bit 0 = team 1, bit 1 = team 2
let mut battle_shout_teams_buffed: u8 = 0;
let mut devotion_aura_teams_buffed: u8 = 0;

// warrior.rs — in try_battle_shout()
// Add parameter: battle_shout_teams_buffed: &mut u8
if *battle_shout_teams_buffed & (1 << (combatant.team - 1)) != 0 {
    return false;
}
// ... after successful cast:
*battle_shout_teams_buffed |= 1 << (combatant.team - 1);

// paladin.rs — in try_devotion_aura()
// Add parameter: devotion_aura_teams_buffed: &mut u8
if *devotion_aura_teams_buffed & (1 << (combatant.team - 1)) != 0 {
    return false;
}
// ... after successful cast:
*devotion_aura_teams_buffed |= 1 << (combatant.team - 1);
```

### Research Insights

**Performance:** `HashSet<u8>` allocates on first insert (Rust starts with 0 capacity). With only 2 teams, a `u8` bitmask eliminates all allocation — two stack bytes, O(1) check-and-set. Saves 2 heap alloc/free cycles per frame.

**Pattern consistency:** The existing `shielded_this_frame: HashSet<Entity>` and `fortified_this_frame: HashSet<Entity>` key by entity (target-level dedup). Group buffs need team-level dedup. The key type difference (`u8` bitmask vs `HashSet<Entity>`) is intentional.

**Defense-in-depth:** The `applied_buffs` HashSet in `apply_pending_auras` (auras.rs:282) remains as a second layer of dedup. Both guards are retained — the upstream one prevents unnecessary `AuraPending` entity spawning, the downstream one prevents double-application.

### Testing

```bash
# Step 1: Verify existing dedup first
echo '{"team1":["Warrior","Warrior","Priest"],"team2":["Mage","Warlock","Paladin"]}' > /tmp/bug4.json
cargo run --release -- --headless /tmp/bug4.json
# Check: grep for duplicate Battle Shout entries at same timestamp

# Step 2: After fix, verify no duplicates
echo '{"team1":["Warrior","Paladin"],"team2":["Mage","Priest"]}' > /tmp/bug4b.json
cargo run --release -- --headless /tmp/bug4b.json
# Verify: one Battle Shout + one Devotion Aura per ally, no duplicates
```

---

## BUG-5: CC Applied to Dead Combatants

**Files:** `src/states/play_match/projectiles.rs`, `src/states/play_match/auras.rs`

### Root Cause

In `process_projectile_hits` (`projectiles.rs:186-337`), after a projectile kills its target (`is_killing_blow = true` at line 211), the code continues to line 314 and unconditionally spawns `AuraPending` for the CC component of the spell. The `is_killing_blow` flag is available in scope but not used to guard the spawn.

Additionally, `apply_pending_auras` (`auras.rs:78`) has NO `is_alive()` check on the target before processing the aura.

### Fix (Two-pronged)

1. **Guard AuraPending spawn in `projectiles.rs`** — wrap the CC spawn at line 314 with `if !is_killing_blow`:

```rust
// projectiles.rs, around line 314
if !is_killing_blow {
    if let Some(aura) = def.applies_aura.as_ref() {
        // ... existing AuraPending spawn code
    }
}
```

2. **Add `is_alive()` guard in `apply_pending_auras`** — defense-in-depth at `auras.rs` after getting the target combatant (line 116). This catches CC from ANY source targeting dead combatants (instant spells, DoT-applied CC, etc.):

```rust
// auras.rs, after line 119 (after successful combatants.get_mut)
if !target_combatant.is_alive() {
    commands.entity(pending_entity).despawn();
    continue;
}
```

### Research Insights

**Pattern consistency:** The `is_alive()` guard in `apply_pending_auras` matches existing Pattern B (early-continue after `get_mut`) used at `auras.rs:605` (dot ticks) and `combat_core.rs:1225` (process_casting). 38 existing `is_alive()` call sites use this exact idiom.

**`is_killing_blow` scope:** The variable is destructured into the outer scope at `projectiles.rs:187` (`let (actual_damage, absorbed, target_team, target_class, is_killing_blow) = { ... }`), so it IS accessible at line 314 where `AuraPending` is spawned. No restructuring needed.

**Multiple projectiles same frame:** If two projectiles hit the same target, the first kills it and the second hits a dead target. The existing early guard at `projectiles.rs:122` (`!target.is_alive()`) despawns the second projectile before damage. The `is_killing_blow` guard on the first projectile prevents its CC spawn. Both cases are covered.

### Testing

```bash
# Frostbolt kill scenario — CC should NOT apply after kill
echo '{"team1":["Mage"],"team2":["Warrior"],"seed":202}' > /tmp/bug5.json
cargo run --release -- --headless /tmp/bug5.json
# Verify: no [CC] entries after a [DEATH] entry for the same target
```

---

## BUG-7: Friendly Fire Breaks Own Team's Polymorph

**Files:** `src/states/play_match/combat_core.rs` (combat_auto_attack only)

### Root Cause

`process_aura_breaks` (`auras.rs:489-551`) tracks damage via `DamageTakenThisFrame` which has no source team info. ANY damage breaks breakable auras. And `acquire_targets` (`combat_ai.rs:21`) does not exclude targets with friendly breakable CC, so pets and allies freely auto-attack polymorphed enemies, breaking their own team's CC.

### Fix (Single-file approach)

**Suppress auto-attacks on targets with friendly breakable CC in `combat_auto_attack`.**

Do NOT add `caster_team` to the `Aura` struct (avoids touching 26 Aura construction sites across 11 files). Instead, look up the caster's team at runtime via the existing `caster: Option<Entity>` field on `Aura`. Dead caster entities still have their `Combatant` component (they're not despawned), so the lookup always works.

**Step 1: Build a lightweight `entity_teams` map** at the top of `combat_auto_attack` (same pattern as existing `positions` and `combatant_info` maps):

```rust
// combat_core.rs — in combat_auto_attack, near line 671
let entity_teams: HashMap<Entity, u8> = combatants.iter()
    .map(|(entity, _, combatant, _, _, _, _)| (entity, combatant.team))
    .collect();
```

This is 7 entries max (6 combatants + 1 pet), each `(Entity, u8)`. No String cloning, no Vec cloning.

**Step 2: Pre-compute a `has_friendly_cc` boolean map** to avoid cloning `Vec<Aura>`:

```rust
// Build a set of entities that have friendly breakable CC
// (Polymorph or Fear with break_on_damage >= 0.0, cast by a specific team)
let friendly_cc_targets: HashMap<Entity, u8> = combatants.iter()
    .filter_map(|(entity, _, _, _, _, auras_opt, _)| {
        auras_opt.as_ref().and_then(|auras| {
            auras.auras.iter()
                .find(|a|
                    a.break_on_damage_threshold >= 0.0
                    && matches!(a.effect_type, AuraType::Polymorph | AuraType::Fear)
                )
                .and_then(|a| a.caster.and_then(|c| entity_teams.get(&c).copied()))
                .map(|caster_team| (entity, caster_team))
        })
    })
    .collect();
```

**Step 3: Check before queueing attack** in the attack collection loop (~line 739):

```rust
// Don't auto-attack targets with friendly breakable CC
if let Some(&cc_caster_team) = friendly_cc_targets.get(&target_entity) {
    if cc_caster_team == combatant.team {
        continue; // Skip — don't break our own Poly/Fear
    }
}
```

### Research Insights

**Why NOT add `caster_team` to `Aura` struct:** The pattern-recognition analysis found 26 direct `Aura { ... }` construction sites + 3 factory methods across 11 files. Adding a field touches all of them (compiler-enforced). Looking up the team at runtime from `caster: Option<Entity>` requires changes in only `combat_core.rs`. Dead entities retain their `Combatant` component, so the lookup is reliable.

**Why suppress in `combat_auto_attack` not `acquire_targets`:** Suppressing in `acquire_targets` would remove the polymorphed enemy from `combatant.target`, causing the entire team to retarget. The kill target must remain assigned; only auto-attack execution should be suppressed. The existing auto-attack system already has attacker-side guards (dead, incapacitated, casting, stealthed at lines 699-726) — this adds the first target-side suppression guard.

**Performance:** The `friendly_cc_targets: HashMap<Entity, u8>` stores at most 7 booleans. No `Vec<Aura>` cloning, no `String` allocation. The performance oracle flagged that building a `HashMap<Entity, Vec<Aura>>` would cause 35+ String heap allocations per frame — this approach avoids that entirely.

**Scope:** This fix covers auto-attacks and pet attacks (both use `combat_auto_attack`). It does NOT cover AoE damage (Frost Nova) or DoT ticks breaking friendly CC. Those are separate considerations — enemy DoTs breaking friendly Poly IS correct WoW behavior, and AoE-breaks-friendly-CC is an acceptable simplification for now.

### Testing

```bash
# Mage + Felhunter vs Paladin — Mage polys, pet should NOT break it
echo '{"team1":["Warlock","Mage"],"team2":["Warrior","Paladin"]}' > /tmp/bug7.json
cargo run --release -- --headless /tmp/bug7.json
# Verify: no "Polymorph broke from damage" entries from friendly auto-attacks
```

---

## BUG-8: Dead Combatant's Spell Completes After Death

**Files:** `src/states/play_match/combat_core.rs`, `src/states/play_match/projectiles.rs`

### Root Cause

System ordering: `process_casting` (Phase 2) runs BEFORE `process_projectile_hits` (Phase 2, later) and `combat_auto_attack` (Phase 3). When `process_casting` runs, the caster is alive and the cast completes. Then a projectile or auto-attack kills the caster later in the same frame. The spell was already completed and effects applied.

The existing `is_alive()` check at `combat_core.rs:1225` only catches deaths from EARLIER systems (DoT ticks in Phase 1).

### Fix

**Cancel CastingState and ChannelingState on death** — In `process_projectile_hits` and `combat_auto_attack`, when a killing blow occurs, remove `CastingState` and `ChannelingState` from the dead entity:

```rust
// projectiles.rs — after is_killing_blow is determined (line 211), in killing blow handling
if is_killing_blow {
    commands.entity(target_entity).remove::<CastingState>();
    commands.entity(target_entity).remove::<ChannelingState>();
}

// combat_core.rs — in combat_auto_attack, after killing blow detection
if is_killing_blow {
    commands.entity(target_entity).remove::<CastingState>();
    commands.entity(target_entity).remove::<ChannelingState>();
}
```

This prevents the dead caster from having a cast complete on the NEXT frame (where `process_casting` would otherwise advance/complete it). The deferred `remove::<CastingState>()` is flushed by `apply_deferred` between phases, so by Phase 1 of the next frame, the CastingState is gone.

**Note on projectiles in flight:** A projectile already spawned before caster death SHOULD still land (correct WoW behavior). This fix only prevents NEW spell completions, not in-flight projectiles.

### Research Insights

**Why no completed_casts re-query:** The simplicity review identified that the second guard (re-querying caster alive in the `completed_casts` loop) is YAGNI — within `process_casting`, both passes run in the same function. No system runs between them that could kill the caster. The plan's own comment acknowledged "no system runs between passes." Adding a guard for a code path that cannot be reached adds complexity without value. The existing `is_alive()` check at line 1225 catches Phase 1 deaths; the kill-handler approach catches Phase 2/3 deaths.

**Pattern consistency:** `process_channeling` (`combat_core.rs:1835`) already has an equivalent dead-caster check. The kill-handler approach (remove CastingState/ChannelingState on death) makes `process_casting` consistent with `process_channeling`.

### Testing

```bash
# Reproduce M17 scenario
echo '{"team1":["Warrior","Mage","Priest"],"team2":["Rogue","Mage","Paladin"],"seed":303}' > /tmp/bug8.json
cargo run --release -- --headless /tmp/bug8.json
# Verify: no spell completions from dead casters (except projectiles already in flight)
```

---

## BUG-10: Fear Landed Through Divine Shield

**Files:** `src/states/mod.rs`, `src/states/play_match/systems.rs`, `src/states/play_match/effects/divine_shield.rs`

### Root Cause

Phase 1 system ordering places `process_divine_shield` AFTER `apply_pending_auras`:

```
apply_pending_auras → process_dispels → ... → process_divine_shield
```

When both `AuraPending(Fear)` and `DivineShieldPending` exist in the same frame, `apply_pending_auras` runs first, checks for `DamageImmunity` (not yet applied), and applies Fear. Then `process_divine_shield` runs, adds `DamageImmunity`, and purges Fear. Result: Fear was briefly applied (advances DR counter, logged to combat log) even though the Paladin had Divine Shield.

### Fix (Two parts)

**Part 1: Reorder Phase 1 chain** — Move `process_divine_shield` before `apply_pending_auras`. Update BOTH registration sites:

**`src/states/mod.rs` (graphical mode), lines 105-126:**

```rust
// Phase 1: Resources and Auras
(
    play_match::handle_time_controls,
    play_match::handle_camera_input,
    play_match::update_camera_position,
    play_match::update_countdown,
    play_match::update_play_match,
    play_match::regenerate_resources,
    play_match::track_shadow_sight_timer,
    play_match::process_dot_ticks,
    play_match::update_auras,
    play_match::process_divine_shield,    // MOVED: before apply_pending_auras
    play_match::apply_pending_auras,
    play_match::process_dispels,
    play_match::process_holy_shock_heals,
    play_match::process_holy_shock_damage,
)
```

**`src/states/play_match/systems.rs` (headless mode), lines 121-138:**

```rust
// Phase 1: Resources and Auras
(
    update_countdown,
    regenerate_resources,
    track_shadow_sight_timer,
    process_dot_ticks,
    update_auras,
    process_divine_shield,    // MOVED: before apply_pending_auras
    apply_pending_auras,
    process_dispels,
    process_holy_shock_heals,
    process_holy_shock_damage,
)
```

**Part 2: Fix `ActiveAuras` query in `process_divine_shield`** — Change from required `&mut ActiveAuras` to `Option<&mut ActiveAuras>`:

```rust
// divine_shield.rs — change query at line 21
mut combatants: Query<(&Combatant, &Transform, Option<&mut ActiveAuras>)>,

// In the processing loop, handle the None case:
if let Ok((combatant, transform, active_auras_opt)) = combatants.get_mut(pending.caster) {
    if !combatant.is_alive() { ... }

    if let Some(mut active_auras) = active_auras_opt {
        // Existing purge + push logic (unchanged)
    } else {
        // No auras yet — insert new ActiveAuras with DamageImmunity
        commands.entity(pending.caster).insert(ActiveAuras {
            auras: vec![Aura { /* DamageImmunity aura */ }],
        });
    }
}
```

### Research Insights

**Deferred command visibility caveat:** When `process_divine_shield` runs before `apply_pending_auras` in the same `.chain()`, the `commands.entity().insert(ActiveAuras {...})` from the `None` branch is deferred and NOT visible to `apply_pending_auras` in the same frame. This means there's a one-frame edge case where a Paladin with zero prior auras activates DS and receives CC in the same frame — the CC will land and be purged next frame. In practice this edge case is extremely narrow (Paladins almost always have Devotion Aura by combat start), but should be documented with a comment.

**Bevy `.chain()` behavior:** Confirmed via framework docs research — `.chain()` auto-inserts `ApplyDeferred` between systems where the preceding system has `Commands`, but the deferred commands are flushed BETWEEN chained systems, not within them. Since both `process_divine_shield` and `apply_pending_auras` are in the same chain, the flush happens between them automatically, so the `ActiveAuras` insert IS visible to `apply_pending_auras`. The edge case above only applies to the `None` path where a brand-new `ActiveAuras` component is inserted.

**Why the `Option<&mut ActiveAuras>` fix is needed:** The simplicity reviewer questioned this, but the architecture review confirmed: with `process_divine_shield` running earlier in the chain (before any buffs are applied), the chance of a Paladin having zero `ActiveAuras` is higher than before. Without this fix, Divine Shield would silently fail in that scenario.

### Testing

```bash
# Paladin bubbles while Fear is cast
echo '{"team1":["Warlock"],"team2":["Paladin"],"seed":303}' > /tmp/bug10.json
cargo run --release -- --headless /tmp/bug10.json
# Verify: no Fear entries on Paladin while Divine Shield is active
```

---

## Acceptance Criteria

- [x] BUG-4: Group buffs (Battle Shout, Devotion Aura) applied exactly ONCE per ally per cast, even with multiple casters on same team
- [x] BUG-5: No CC auras applied to dead combatants (no [CC] log entries after [DEATH] for same target)
- [x] BUG-7: Auto-attacks suppressed on targets with friendly breakable CC (Polymorph/Fear from same team)
- [x] BUG-8: Casts cancelled on caster death — no spell completions from dead casters
- [x] BUG-10: No hostile auras land through Divine Shield immunity
- [x] All changes work in BOTH graphical AND headless modes
- [x] `cargo build --release` compiles without errors or warnings
- [x] Headless match simulations pass without panics across 1v1, 2v2, and 3v3 formats

## Implementation Order

1. **BUG-10** (system reorder + Optional query) — simplest change, high impact
2. **BUG-5** (is_alive guards) — two small guard additions
3. **BUG-8** (death cancels cast) — targeted changes in kill handlers
4. **BUG-7** (friendly CC auto-attack suppression) — single file change in `combat_core.rs`
5. **BUG-4** (duplicate buff dedup) — verify existing dedup first, then add bitmask if needed

## Verification Plan

After all fixes:

```bash
# Run the specific seeds from the bug report
echo '{"team1":["Rogue","Priest"],"team2":["Warlock","Paladin"],"seed":202}' > /tmp/verify1.json
echo '{"team1":["Warrior","Mage","Priest"],"team2":["Rogue","Mage","Paladin"],"seed":303}' > /tmp/verify2.json
echo '{"team1":["Rogue","Mage","Priest"],"team2":["Warrior","Warlock","Paladin"],"seed":304}' > /tmp/verify3.json

cargo run --release -- --headless /tmp/verify1.json
cargo run --release -- --headless /tmp/verify2.json
cargo run --release -- --headless /tmp/verify3.json

# Broad smoke test — multiple formats
echo '{"team1":["Warrior","Warrior","Priest"],"team2":["Mage","Warlock","Paladin"]}' > /tmp/smoke1.json
echo '{"team1":["Warlock","Mage"],"team2":["Warrior","Paladin"]}' > /tmp/smoke2.json
cargo run --release -- --headless /tmp/smoke1.json
cargo run --release -- --headless /tmp/smoke2.json
```
