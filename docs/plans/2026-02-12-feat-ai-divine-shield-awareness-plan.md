---
title: AI Divine Shield Awareness
type: feat
date: 2026-02-12
---

# AI Divine Shield Awareness

## Overview

When an enemy Paladin activates Divine Shield (12s `AuraType::DamageImmunity`), opposing AI currently ignores it entirely -- wasting critical cooldowns (Kidney Shot, Polymorph, Fear, Hammer of Justice) and continuing to attack for 0 damage. The AI should recognize immunity, temporarily switch targets, and avoid burning important abilities.

## Problem Statement

In the current implementation:

1. `acquire_targets()` in `combat_ai.rs` never checks for `DamageImmunity` -- the kill target and CC target remain pointed at the immune Paladin
2. Every class AI (`decide_*_action()`) fires abilities into immunity, consuming cooldowns and mana for zero effect
3. `check_interrupts()` wastes Kick/Pummel cooldowns on immune targets
4. `select_cc_target_heuristic()` can select an immune target for Polymorph/Fear
5. Warlock `try_spread_curses()` iterates all enemies without an immunity filter

The damage/aura systems correctly block the effects (`apply_damage_with_absorb` returns 0, `AuraPending` shows "Immune"), but the AI layer above doesn't know to avoid them.

## Proposed Solution

**Centralize immunity awareness in `acquire_targets()`**, treating immune targets the same way stealthed/invisible targets are handled. Add a lightweight `entity_is_immune()` helper to `CombatContext` for secondary guards in class AI.

### Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Where to check immunity | `acquire_targets()` (centralized) | All class AI benefits automatically, mirrors `can_see()` pattern |
| CC target handling | Also filter from `select_cc_target_heuristic()` | Prevents Polymorph/Fear/KS waste |
| Cancel in-progress casts | No | Too complex; one-ability waste per DS is acceptable |
| Consider DS remaining duration | No | KISS; boolean check is correct enough |
| Auto-attacks on immune target | Let deal 0 (current behavior) | Changing movement is larger scope |
| All enemies immune (1v1) | `target = None`, classes idle or self-buff | Matches WoW Classic behavior |
| Secondary guards in class AI | Yes, lightweight | Belt-and-suspenders for one-frame timing gap and AoE targeting |

## Acceptance Criteria

- [x] AI does not use Kidney Shot, Polymorph, Fear, Hammer of Justice, or Frost Nova on a Divine Shielded target
- [x] AI does not use Mortal Strike, Mind Blast, or other cooldown abilities on a Divine Shielded target
- [x] AI temporarily switches kill target to a non-immune enemy during Divine Shield
- [x] AI switches CC target away from immune enemies
- [x] AI does not attempt interrupts (Kick, Pummel) on immune targets
- [x] Warlock curse spreading skips immune enemies
- [x] AI returns to original configured kill target after Divine Shield expires
- [x] In 1v1 vs Paladin, AI idles/self-buffs during DS rather than wasting abilities
- [x] Healers (Priest, Paladin) continue healing allies normally during DS
- [x] Headless simulation confirms no wasted cooldowns in Rogue/Mage vs Paladin matchup

## Implementation

### Phase 1: CombatContext Helper

**`src/states/play_match/class_ai/mod.rs`**

Add `entity_is_immune()` to `CombatContext`:

```rust
pub fn entity_is_immune(&self, entity: Entity) -> bool {
    self.active_auras
        .get(&entity)
        .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::DamageImmunity))
        .unwrap_or(false)
}

pub fn target_is_immune(&self) -> bool {
    self.target_info()
        .map(|info| self.entity_is_immune(info.entity))
        .unwrap_or(false)
}
```

### Phase 2: Target Acquisition (`combat_ai.rs`)

**`acquire_targets()` (~line 102-138):** Add immunity filtering alongside existing visibility checks.

- When the configured `kill_target` entity has `DamageImmunity`, skip it and fall through to nearest-visible-non-immune enemy
- When no non-immune enemies exist, set `combatant.target = None`
- Natural restoration: when DS expires, next frame's `acquire_targets()` re-selects the config kill target (no stored "original target" needed)

**`select_cc_target_heuristic()` (~line 189-247):** Add immunity filter alongside existing CC-overlap filter.

- Skip enemies with `DamageImmunity` when scoring CC targets
- If all potential CC targets are immune, return `None`

**`check_interrupts()` (~line 777):** Skip interrupt attempts when the target has `DamageImmunity`.

### Phase 3: Class AI Secondary Guards

Add `target_is_immune()` early-return in offensive ability functions across all 6 class AI files. This is defense-in-depth for the one-frame timing gap between `DivineShieldPending` → aura application.

**Priority abilities to guard (by cooldown impact):**

| File | Abilities | Pattern |
|------|-----------|---------|
| `rogue.rs` | `try_kidney_shot()`, `try_cheap_shot()`, `try_sinister_strike()`, `try_eviscerate()` | Check `entity_is_immune(target)` before attempting |
| `mage.rs` | `try_polymorph()`, `try_frostbolt()`, `try_frost_nova()`, `try_fire_blast()` | Check cc_target immunity for poly, kill target for damage |
| `warrior.rs` | `try_mortal_strike()`, `try_charge()`, `try_rend()`, `try_heroic_strike()` | Check target immunity |
| `warlock.rs` | `try_fear()`, `try_corruption()`, `try_shadowbolt()`, `try_immolate()`, `try_drain_life()`, `try_spread_curses()` | Check target immunity; spread_curses filters per-enemy |
| `priest.rs` | `try_mind_blast()` | Check target immunity |
| `paladin.rs` | `try_hammer_of_justice()`, `try_holy_shock_damage()` | Check target immunity |

**Self-buffs and ally heals need NO changes** -- they don't target enemies.

### Phase 4: Verify with Headless Simulation

Test configs:

```json
// 2v2: Rogue+Mage vs Paladin+Priest (Paladin is kill target)
{"team1":["Rogue","Mage"],"team2":["Paladin","Priest"],"team1_kill_target":0}

// 1v1: Warrior vs Paladin (no alternate target)
{"team1":["Warrior"],"team2":["Paladin"]}

// 3v3: Warrior+Mage+Priest vs Paladin+Warlock+Rogue
{"team1":["Warrior","Mage","Priest"],"team2":["Paladin","Warlock","Rogue"],"team1_kill_target":0}
```

Verify in match logs:
- No Kidney Shot / Polymorph / Fear during DS window
- Kill target switches to non-Paladin during DS
- Kill target returns to Paladin after DS expires

## Edge Cases

| Scenario | Expected Behavior |
|----------|-------------------|
| 1v1 vs Paladin with DS | `target = None`, AI idles for 12s, re-acquires when DS expires |
| All enemies immune (multi-Paladin) | `target = None`, healers continue healing, DPS idle |
| DS pops mid-cast (Frostbolt in progress) | Cast completes, deals 0 -- acceptable one-time waste |
| DS pops mid-projectile-flight | Projectile lands, deals 0 -- already handled by combat_core |
| Frost Nova with immune + non-immune in range | Still cast (roots non-immune), immune target unaffected (handled by aura system) |
| Warlock DoTs purged by DS, tries to reapply | Secondary guard prevents reapplication on immune target |

## Files Modified

1. `src/states/play_match/class_ai/mod.rs` -- Add `entity_is_immune()`, `target_is_immune()` helpers
2. `src/states/play_match/combat_ai.rs` -- Immunity filter in `acquire_targets()`, `select_cc_target_heuristic()`, `check_interrupts()`
3. `src/states/play_match/class_ai/warrior.rs` -- Secondary guards on offensive abilities
4. `src/states/play_match/class_ai/mage.rs` -- Secondary guards on offensive abilities + poly
5. `src/states/play_match/class_ai/rogue.rs` -- Secondary guards on KS, SS, Evisc
6. `src/states/play_match/class_ai/priest.rs` -- Secondary guard on Mind Blast
7. `src/states/play_match/class_ai/warlock.rs` -- Secondary guards + spread_curses filter
8. `src/states/play_match/class_ai/paladin.rs` -- Secondary guards on HoJ, Holy Shock damage

## References

- Divine Shield implementation: `src/states/play_match/effects/divine_shield.rs`
- Immunity damage block: `src/states/play_match/combat_core.rs:59-63`
- Immunity aura block: `src/states/play_match/auras.rs:172-207`
- Existing CC-overlap pattern (model for immunity checks): `src/states/play_match/class_ai/rogue.rs:111-115`
- Target acquisition: `src/states/play_match/combat_ai.rs:21-178`
- CC target heuristic: `src/states/play_match/combat_ai.rs:189-247`
- DS config: `assets/config/abilities.ron:571-583` (12s duration, 300s cooldown)
