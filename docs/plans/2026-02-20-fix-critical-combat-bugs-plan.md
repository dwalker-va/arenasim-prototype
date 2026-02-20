---
title: "fix: Critical Combat Bugs — CC Bypass, 3v3 Spawn Drop, Double Kill"
type: fix
date: 2026-02-20
deepened: 2026-02-20
source: docs/reports/2026-02-20-headless-match-bug-report.md
---

# fix: Critical Combat Bugs — CC Bypass, 3v3 Spawn Drop, Double Kill

## Enhancement Summary

**Deepened on:** 2026-02-20
**Research agents used:** Bevy ECS death patterns, WoW CC mechanics, documented learnings, code state audit

### Key Improvements from Research

1. **BUG-3 architecture change**: Switched from `Dead` marker component to `is_dead: bool` field on `Combatant`. Marker components use deferred `Commands` — invisible within the same system and across unordered system groups. A bool field is immediately visible via `&mut Combatant` with zero deferred-command complexity.
2. **BUG-1 combat logging**: CC interruptions should log via `CombatLogEventType::CrowdControl` and call `mark_cast_interrupted()` for timeline UI. No school lockout (only dedicated interrupt abilities cause lockouts).
3. **BUG-1 Root exclusion confirmed**: Root does NOT interrupt casting in WoW Classic. Only `Stun | Fear | Polymorph` — matches the existing `is_incapacitated()` helper exactly.

---

## Overview

Three critical bugs identified in the 20-match headless test run need fixing. These break core combat mechanics: stunned combatants can cast, 3v3 matches can silently lose combatants, and dead combatants can die twice.

---

## Bug 1: Actions While Stunned / CC Bypass (P0)

### Problem

Stunned/feared/polymorphed combatants can continue casting abilities. `process_casting()` and `process_channeling()` in `combat_core.rs` never check for incapacitating auras before advancing cast timers.

**Affected in 6/20 matches** (M2, M3, M8, M13, M15, M17, M18).

### Root Cause

- `process_casting()` (line ~1199) checks `is_alive()` and `interrupted` but never checks for Stun/Fear/Polymorph auras before ticking `casting.time_remaining -= dt` at line 1245
- `process_channeling()` (line ~1780) has the same gap — its `_caster_auras` parameter is unused (underscore prefix)
- `decide_abilities()` in `combat_ai.rs` correctly blocks **starting** new casts while incapacitated, but its `Without<CastingState>` filter excludes entities already casting, so mid-cast stuns are never caught

### Fix

In both `process_casting` and `process_channeling`, after the `is_alive()` check (line ~1225) and before the `interrupted` check (line ~1232), add an incapacitation check. Follow the existing pattern from `combat_auto_attack()` (line ~703):

```rust
// combat_core.rs — process_casting, after is_alive() check (line ~1229)
// WoW Mechanic: Stun, Fear, and Polymorph cancel casts in progress
// (Root does NOT interrupt casting — only movement)
let is_incapacitated = if let Some(ref auras) = caster_auras {
    auras.auras.iter().any(|a| matches!(
        a.effect_type,
        AuraType::Stun | AuraType::Fear | AuraType::Polymorph
    ))
} else {
    false
};
if is_incapacitated {
    // Log CC interruption (no school lockout — only dedicated interrupts cause lockouts)
    let ability_name = &abilities.get_unchecked(&casting.ability).name;
    let caster_id = format!("Team {} {}", caster.team, caster.class.name());
    combat_log.mark_cast_interrupted(&caster_id, ability_name);
    combat_log.log(
        CombatLogEventType::CrowdControl,
        format!("{}'s {} interrupted by crowd control", caster_id, ability_name),
    );
    commands.entity(caster_entity).remove::<CastingState>();
    continue;
}
```

Same pattern for `process_channeling`, removing `ChannelingState` instead. Rename `_caster_auras` to `caster_auras`.

### Research Insights

**WoW Mechanic Accuracy:**
- **Cancel, not pause** — stuns/fears/poly remove the cast entirely, mana is NOT consumed (mana consumed at completion line ~1255, which never executes)
- **No school lockout** — only dedicated interrupt abilities (Kick, Pummel, Counterspell) apply SpellSchoolLockout. CC-based interrupts do not.
- **Root excluded** — Root prevents movement, not casting. Matches `is_incapacitated()` helper in `class_ai/mod.rs:155` exactly (Stun | Fear | Polymorph, no Root).

**System Ordering (no race condition):**
- Phase 1: `apply_pending_auras` (CC auras land here)
- `apply_deferred` barrier between Phase 1 and Phase 2
- Phase 2 (chained): `decide_abilities` → `process_casting` → `process_channeling`
- CC aura applied in Phase 1 is visible to `process_casting` in Phase 2 on the same frame. No timing issue.

**Same-frame new cast:** Already handled — `decide_abilities` runs before `process_casting` in the chain and blocks new casts while incapacitated.

### Files to Modify

- `src/states/play_match/combat_core.rs` — `process_casting()` (after line ~1229) and `process_channeling()` (after line ~1838)

### Acceptance Criteria

- [ ] Stunned combatant's in-progress cast is cancelled immediately
- [ ] Feared combatant's in-progress cast is cancelled immediately
- [ ] Polymorphed combatant's in-progress cast is cancelled immediately
- [ ] Channeled spells are also interrupted by CC
- [ ] Root does NOT interrupt casting (only movement)
- [ ] Combat log shows `[CC]` entry when CC interrupts a cast
- [ ] No school lockout applied from CC interruption
- [ ] No mana consumed for interrupted cast
- [ ] Headless test with seed 303 (M17) no longer shows Warlock casting during HoJ stun

---

## Bug 2: 3v3 Config Silently Drops Combatants (P0)

### Problem

Match 18 configured 3v3 (6 combatants) but only spawned 4. Three combatants never appeared with no error.

**Affected in 1/20 matches** (M18), but 100% severity when it occurs.

### Root Cause

Two issues contribute:

1. **Spawn loops silently skip `None` entries**: Both `setup_play_match` (mod.rs line ~329) and `headless_setup_match` (runner.rs line ~150) use `if let Some(character) = character_opt` with no `else` branch — `None` slots are silently ignored.

2. **Headless config uses `.ok()` for silent conversion**: `to_match_config()` in `config.rs` (line ~220) maps parse failures to `None` via `.ok()`, which passes validation (validation runs first with `?`) but could theoretically produce `None` entries if state changes between validation and conversion.

### Fix

**A. Make headless config parsing fail loudly** — replace `.ok()` with proper error propagation at lines 220 and 226:

```rust
// config.rs — to_match_config()
let team1: Vec<Option<CharacterClass>> = self
    .team1
    .iter()
    .map(|s| Self::parse_class(s).map(Some))
    .collect::<Result<Vec<_>, _>>()?;
```

**B. Add warning in spawn loops** for `None` entries (both graphical and headless):

```rust
for (i, character_opt) in config.team1.iter().enumerate() {
    if let Some(character) = character_opt {
        // spawn...
    } else {
        warn!("Team 1 slot {} is empty — skipping spawn", i);
    }
}
```

**C. Add spawn count validation** after both team spawn loops complete:

```rust
let spawned_team1 = config.team1.iter().filter(|c| c.is_some()).count();
let spawned_team2 = config.team2.iter().filter(|c| c.is_some()).count();
if spawned_team1 != config.team1_size || spawned_team2 != config.team2_size {
    error!(
        "Spawn count mismatch! Expected {}v{}, got {}v{}",
        config.team1_size, config.team2_size, spawned_team1, spawned_team2
    );
}
```

### Files to Modify

- `src/headless/config.rs` — `to_match_config()` (lines ~220, ~226)
- `src/headless/runner.rs` — `headless_setup_match()` (lines ~150, ~186)
- `src/states/play_match/mod.rs` — `setup_play_match()` (lines ~329, ~375)

### Acceptance Criteria

- [ ] Invalid class names in headless config produce an error, not silent `None`
- [ ] Spawn loops warn when encountering `None` slots
- [ ] Post-spawn validation logs error if spawned count != expected team size
- [ ] Seed 304 3v3 config spawns all 6 combatants or fails loudly
- [ ] All other 3v3 seeds continue working correctly

---

## Bug 3: Double Kill — Dead Combatant Receives Damage (P1)

### Problem

A dead combatant (HP = 0) continues receiving damage and triggers a second death. The `is_alive()` check exists in each damage system, but within the same frame, a combatant killed by one system can be hit by another before the frame ends.

**Affected in 1/20 matches** (M8).

### Root Cause

Damage is applied at 7 different sites (per documented learnings). Each checks `is_alive()` independently, but within a single frame:
1. System A kills the target (HP → 0)
2. System B runs later and its `is_alive()` check sees `current_health <= 0.0` — but the kill was already processed and logged by System A
3. System B applies damage to the already-dead target, logging a second death

The existing `died_this_frame` HashSet in `combat_auto_attack` only tracks dead **attackers** within that single system, not dead **targets** across systems.

### Fix — `is_dead` bool field (NOT marker component)

**Why not a `Dead` marker component:** Research confirmed that `commands.entity(e).try_insert(Dead)` is **deferred** — the component is NOT added until the next `apply_deferred` sync point. Within the same system (e.g., `process_projectile_hits` processing multiple projectiles), a `Without<Dead>` query filter would NOT see the marker inserted earlier in the same loop iteration. There is also no `apply_deferred` between the Phase 2 chain and the Phase 3 unordered group where `combat_auto_attack` runs.

**The correct fix**: Add an `is_dead: bool` field to `Combatant`, set it `true` immediately when HP reaches 0. This is visible instantly via `&mut Combatant` — no deferred commands, no sync points needed.

```rust
// components/mod.rs — add to Combatant struct
pub is_dead: bool,  // Set on first killing blow, prevents duplicate death processing

// Update is_alive() to also check is_dead
pub fn is_alive(&self) -> bool {
    self.current_health > 0.0 && !self.is_dead
}

// Initialize in Combatant::new() or Default
is_dead: false,
```

**At each damage application site**, after the killing blow:
```rust
let is_killing_blow = !target.is_alive();
if is_killing_blow && !target.is_dead {
    target.is_dead = true;
    // Log death, trigger animations, etc. — happens exactly once
}
```

**Existing `is_alive()` checks across all 7 sites will automatically work** because `is_alive()` now returns `false` when `is_dead` is `true`, even if `current_health` is somehow still checked separately.

### The 7 damage sites to guard

1. Auto-attacks — `combat_core.rs:combat_auto_attack()` (line ~864) — already has `died_this_frame` for attackers, add `is_dead` target check
2. Cast completion damage — `combat_core.rs:process_casting()` (line ~1280) — add `is_dead` check before damage
3. Cast completion healing — `combat_core.rs:process_casting()` (healing path) — skip healing dead targets
4. Projectile impact — `projectiles.rs:process_projectile_hits()` (line ~188) — add `is_dead` check
5. Holy Shock damage — `effects/holy_shock.rs` — add `is_dead` check
6. Holy Shock healing — `effects/holy_shock.rs` — skip healing dead targets
7. Class AI instant attacks — `combat_ai.rs:decide_abilities()` (line ~567) — add `is_dead` check

Also update:
- `process_dot_ticks` in `auras.rs` (line ~663) — DoTs should not tick on dead targets
- `apply_pending_auras` in `auras.rs` — auras should not apply to dead targets (fixes BUG-5 too)

### Research Insights

**Why `is_dead: bool` beats `Dead` marker component:**

| Approach | Immediate visibility | Within same system | Across unordered groups | Complexity |
|---|---|---|---|---|
| `is_dead: bool` on Combatant | Yes (`&mut`) | Yes | Yes | Low |
| `Dead` marker + `Without<Dead>` | No (deferred) | No | No (no sync point) | Medium |

**Single source of truth**: The `is_dead` field and `current_health` are both on `Combatant`. Updating `is_alive()` to check both means all 50+ existing `is_alive()` callsites automatically benefit. No risk of desync between two separate truth sources.

**No archetype move cost**: Changing a bool doesn't move the entity to a new archetype table (a `Dead` component insertion would).

### Files to Modify

- `src/states/play_match/components/mod.rs` — add `is_dead: bool` to `Combatant`, update `is_alive()`, update constructors
- `src/states/play_match/combat_core.rs` — set `is_dead = true` on killing blows in `combat_auto_attack` and `process_casting`
- `src/states/play_match/projectiles.rs` — set `is_dead = true` on killing blows in `process_projectile_hits`
- `src/states/play_match/auras.rs` — add `is_alive()` guard in `process_dot_ticks`, `apply_pending_auras`
- `src/states/play_match/combat_ai.rs` — set `is_dead = true` on killing blows from instant attacks
- `src/states/play_match/effects/holy_shock.rs` — set `is_dead = true` on killing blows

### Acceptance Criteria

- [ ] `is_dead` field added to `Combatant` struct, initialized `false`
- [ ] `is_alive()` returns `false` when `is_dead` is `true`
- [ ] All 7 damage sites set `is_dead = true` on killing blow
- [ ] Kill processing (death log, animation) gated on `!target.is_dead` — happens exactly once
- [ ] DoT ticks stop on dead combatants
- [ ] Auras are not applied to dead combatants
- [ ] Seed 202 match no longer shows Warlock dying twice
- [ ] No regression in normal kill scenarios

---

## Implementation Order

1. **BUG-1 (CC bypass)** — Smallest change, biggest impact. Two functions, same pattern.
2. **BUG-3 (Double kill)** — Add `is_dead` bool, update `is_alive()`, guard all 7 damage sites.
3. **BUG-2 (3v3 spawn)** — Defensive logging + config hardening. Needs seed 304 to reproduce.

## Test Plan

- [ ] Run headless match with seed 303 (BUG-1 reproduction case)
- [ ] Run headless match with seed 202 (BUG-3 reproduction case)
- [ ] Run headless match with seed 304 and 3v3 config (BUG-2 reproduction case)
- [ ] Run 10 random 3v3 matches to verify no spawn regressions
- [ ] Run `cargo test` for unit test suite
- [ ] Run `cargo clippy` for lint check
- [ ] Verify both headless AND graphical modes work (dual registration)

## References

- Bug report: `docs/reports/2026-02-20-headless-match-bug-report.md`
- Existing CC check pattern: `combat_core.rs:703-711` (`combat_auto_attack`)
- Existing `is_incapacitated()` helper: `class_ai/mod.rs:155-157`
- Dual system registration: `docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md`
- Damage site inventory: `docs/solutions/implementation-patterns/critical-hit-system-distributed-crit-rolls.md`
- Bug hunting workflow: `docs/solutions/workflows/two-agent-bug-hunting.md`
- Bevy command deferral: Commands are deferred until `apply_deferred` sync points — [Bevy Cheat Book](https://bevy-cheatbook.github.io/programming/commands.html)
