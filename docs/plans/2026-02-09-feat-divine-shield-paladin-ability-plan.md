---
title: "feat: Add Divine Shield ability to Paladin"
type: feat
date: 2026-02-09
---

## Enhancement Summary

**Deepened on:** 2026-02-09
**Sections enhanced:** 7
**Review agents used:** architecture-strategist, performance-oracle, code-simplicity-reviewer, pattern-recognition-specialist, Bevy ECS researcher

### Key Improvements (from deepening)
1. **Eliminated `DamageOutputReduction` AuraType** — simplicity reviewer found this was over-engineered. The 50% penalty is derived from `DamageImmunity` presence via a helper function, reducing file changes from 16 to 11.
2. **Use `DivineShieldPending` deferred pattern** — architecture reviewer identified that Paladin AI only has immutable aura access, so debuff purge must use the pending component pattern (like Holy Shock).
3. **Inline debuff purge list** — YAGNI: `is_debuff()` method has only one consumer. Use a `matches!()` expression directly. Extract to method when a second consumer appears (Mass Dispel, Purge).
4. **Reuse `ShieldBubble`** — extend existing visual system rather than creating a new 3-system component, saving ~80 lines.
5. **Skip enemy AI target switching** — low value for a 12s/300s ability. Add if simulations show problems.
6. **One-frame delay is acceptable** — pattern reviewer noted movement and auto-attack gates resolve naturally the frame after Divine Shield purges CC.

### New Edge Cases Discovered
- Drain Life channel continues but deals 0 damage and provides 0 healing during immunity
- Movement system bypass not needed — CC removal means movement resumes next frame
- `DamageImmunity` + `DamageReduction` (Curse of Weakness) can theoretically coexist — moot since DS purges CoW on activation

---

# feat: Add Divine Shield ability to Paladin

## Overview

Add Divine Shield — the Paladin's signature defensive cooldown. When activated, the Paladin becomes immune to all damage and CC for 12 seconds, all debuffs are instantly purged, but outgoing damage is reduced by 50%. Can be used while incapacitated (stunned, feared, polymorphed). 5-minute cooldown makes it a once-per-match ability.

## Problem Statement / Motivation

The Paladin currently has no panic button. Against burst compositions (Warrior + Rogue), the Paladin can be killed through stun-locks with no counterplay. Divine Shield adds a strategic decision layer: when to use the bubble for maximum value (survive burst, break CC to save a teammate, or safely cast big heals under pressure).

## Reference Values (WoW Classic)

| Stat | WoW Classic (Spell ID 642) | Our Value |
|------|---------------------------|-----------|
| Cast Time | Instant | Instant |
| Mana Cost | 75 | 0 (see rationale) |
| Cooldown | 5 min | 5 min (300s) |
| Duration | 10 sec | 12 sec (per user choice) |
| Damage Reduction | 50% outgoing | 50% outgoing |
| Spell School | N/A | None (cannot be locked out) |

**Mana cost rationale:** Set to 0 so the ability is always usable in emergencies (OOM + stunned). The 5-minute cooldown is the real cost.

## Proposed Solution

### Architecture: New `AuraType::DamageImmunity`

Add a new `AuraType::DamageImmunity` variant. This is checked:
1. At the top of `apply_damage_with_absorb()` — covers auto-attacks, casts, DoTs, projectiles (all 8 damage paths)
2. In `apply_pending_auras()` — blocks ALL hostile aura applications (CC, DoTs, debuffs)
3. In `decide_abilities()` — allows Divine Shield to bypass the incapacitation gate

**Why not a marker component?** Aura-based approach integrates with the existing aura duration/expiration system, appears in combat log buff tracking, and gets cleaned up automatically when it expires. At 6-entity scale with ~5-8 auras per entity, the aura scan is negligible (performance review confirmed).

**Why not `Absorb(999999)`?** A finite absorb can be exhausted, doesn't block CC, and doesn't conceptually represent immunity. True immunity needs a first-class type.

### Outgoing Damage Reduction (Simplified)

~~A second aura `AuraType::DamageOutputReduction`~~ **No separate aura needed.** Instead, add a helper function:

```rust
/// Returns the outgoing damage multiplier for the caster.
/// If caster has DamageImmunity (Divine Shield), returns 0.5.
pub fn get_divine_shield_damage_penalty(auras: Option<&ActiveAuras>) -> f32 {
    if has_damage_immunity(auras) {
        DIVINE_SHIELD_DAMAGE_PENALTY // 0.5
    } else {
        1.0
    }
}
```

This is checked at the 2 existing outgoing damage sites where `get_physical_damage_reduction()` is already called, plus the instant attack and projectile paths. The 50% penalty is an intrinsic property of having `DamageImmunity`, not a separate aura.

### Debuff Purge (Simplified)

~~Add `AuraType::is_debuff()` method~~ **Inline the purge list** at the single activation site:

```rust
// In divine shield activation:
auras.auras.retain(|a| !matches!(a.effect_type,
    AuraType::MovementSpeedSlow | AuraType::Root | AuraType::Stun |
    AuraType::DamageOverTime | AuraType::SpellSchoolLockout |
    AuraType::HealingReduction | AuraType::Fear | AuraType::Polymorph |
    AuraType::DamageReduction | AuraType::CastTimeIncrease
));
```

YAGNI: Extract to `is_debuff()` when a second consumer appears (Mass Dispel, Purge). No stat auras are debuffs, so no stat restoration is needed.

### Activation: `DivineShieldPending` Component

The Paladin AI has immutable aura access (`Option<&ActiveAuras>`), so debuff purge cannot happen inline. Use the established pending component pattern:

```rust
#[derive(Component)]
pub struct DivineShieldPending {
    pub caster: Entity,
    pub caster_team: u8,
    pub caster_class: CharacterClass,
}
```

Processed by `process_divine_shield()` in `effects/divine_shield.rs` (following `effects/holy_shock.rs` and `effects/dispels.rs` pattern). This system has mutable access to `ActiveAuras` and can purge debuffs + apply the immunity aura.

### Incapacitation Gate Bypass

In `decide_abilities()` (`combat_ai.rs`), add a Paladin-specific pre-check before the `is_incapacitated` gate:

```rust
// Before the incapacitation check:
if is_incapacitated && combatant.class == CharacterClass::Paladin {
    if try_divine_shield_while_cc(commands, combat_log, abilities, combatant, auras, allies, ...) {
        // Spawned DivineShieldPending — CC will be purged by process_divine_shield()
        // Skip normal processing this frame; next frame Paladin is free
        continue;
    }
    continue; // Still incapacitated, can't do anything else
} else if is_incapacitated {
    continue;
}
```

**One-frame delay is acceptable:** After `DivineShieldPending` is processed, CC is removed. The movement and auto-attack gates (`move_to_target()`, `combat_auto_attack()`) will resolve naturally the next frame since they check current auras.

### AI Decision Logic

Divine Shield is evaluated at **priority 0.5** (after Devotion Aura pre-buff, before everything else):

**Trigger conditions (any of these):**
1. **Survival:** Self HP < 30% AND in active combat (gates opened)
2. **CC Break for teammate:** Self is incapacitated (Stun/Fear/Poly) AND any teammate < 30% HP
3. **Heal under pressure:** Self HP < 50% AND being focused AND wants to safely cast heals

**Never use when:**
- Pre-match countdown (gates not opened)
- Already have Divine Shield active (check for `DamageImmunity` aura)
- On cooldown
- Team is healthy and no CC on self

### Visual Effect: Extend ShieldBubble

~~New `DivineShieldBubble` component~~ **Reuse `ShieldBubble`** by extending `update_shield_bubbles()` to also detect `DamageImmunity` aura. When triggered by immunity (vs `Absorb`):

- Color: `srgba(1.0, 0.85, 0.3, 0.4)` — rich gold (distinct from PW:S light blue)
- Emissive: `LinearRgba(3.0, 2.5, 0.8, 1.0)` — bright golden glow
- Scale: 1.3x combatant size (larger than PW:S 0.9x)
- Animation: Gentle pulse (scale oscillation via `sin(time)`)
- Uses `AlphaMode::Add` per visual effects pattern

### Enemy AI Target Switching — DEFERRED

~~Add target deprioritization~~ **Skip for now.** Divine Shield is active 12s out of 300s (4% of match). The Paladin is a healer so enemies are often targeting the DPS partner anyway. Add if headless sims show problematic behavior.

## Technical Approach

### Implementation Phases

#### Phase 1: Foundation — AuraType + Ability Registration

- [x] Add `AuraType::DamageImmunity` variant to `components/mod.rs`
- [x] Add `DamageImmunity` to buff stacking prevention in `apply_pending_auras()`
- [x] Add `DivineShield` variant to `AbilityType` enum in `abilities.rs`
- [x] Add `DivineShield` to validation in `ability_config.rs`
- [x] Add `DivineShield` definition to `abilities.ron` (instant, 0 mana, 300s CD, spell_school: None, applies_aura: DamageImmunity duration 12.0)
- [x] Add constants to `constants.rs`: `DIVINE_SHIELD_DAMAGE_PENALTY: f32 = 0.5`, `DIVINE_SHIELD_HP_THRESHOLD: f32 = 0.3`
- [x] Add `DivineShieldPending` component to `components/mod.rs`
- [x] Add helper: `pub fn has_damage_immunity(auras: Option<&ActiveAuras>) -> bool` in `combat_core.rs`
- [x] Add helper: `pub fn get_divine_shield_damage_penalty(auras: Option<&ActiveAuras>) -> f32` in `combat_core.rs`

#### Phase 2: Immunity Mechanics — Damage + Aura Blocking

- [x] Add immunity check at top of `apply_damage_with_absorb()` in `combat_core.rs` — return `(0.0, 0.0)` immediately (before `DamageTakenReduction` and `Absorb` checks)
- [x] Each call site: check `has_damage_immunity()` on target before spawning damage FCT; show "Immune" text instead of "0"
- [x] Add `get_divine_shield_damage_penalty()` check in `combat_auto_attack()` — multiply raw damage
- [x] Add `get_divine_shield_damage_penalty()` check in `process_casting()` for cast-completion damage
- [x] Add `get_divine_shield_damage_penalty()` check in instant attack processing in `combat_ai.rs`
- [x] Add `get_divine_shield_damage_penalty()` check in projectile damage at impact time (query caster's auras)
- [x] Block ALL hostile aura applications during `DamageImmunity` in `apply_pending_auras()` — show "Immune" FCT (follow `ChargingState` immunity pattern at auras.rs:116-170)
- [x] Handle Drain Life edge case: if damage is 0 due to immunity, healing is also 0

#### Phase 3: Activation — `effects/divine_shield.rs`

- [x] Create `effects/divine_shield.rs` module
- [x] Add `process_divine_shield()` system: query `DivineShieldPending`, get mutable `ActiveAuras` on caster
- [x] Purge debuffs: `auras.retain(|a| !matches!(a.effect_type, Stun | Fear | Polymorph | Root | ...))` (inline list)
- [x] Apply `DamageImmunity` aura (duration 12.0, magnitude 1.0)
- [x] Log: "[Paladin] uses Divine Shield" + "[Paladin]'s Divine Shield removes N debuffs"
- [x] Spawn golden "Divine Shield" FCT on the Paladin
- [x] Despawn the pending entity
- [x] Register in `effects/mod.rs` and `systems.rs`

#### Phase 4: AI Integration

- [x] Add `try_divine_shield()` function to `class_ai/paladin.rs` (follows existing `try_*` pattern)
- [x] Add incapacitation gate bypass in `decide_abilities()` in `combat_ai.rs` (Paladin-specific pre-check)
- [x] Implement trigger conditions: self HP < 30%, CC break for teammate, heal-under-pressure
- [x] Add guards: gates opened, not already active, not on cooldown
- [x] Insert Divine Shield at priority 0.5 in `decide_paladin_action()` priority list

#### Phase 5: Visual Effect + UI

- [x] Extend `update_shield_bubbles()` in `rendering/effects.rs` to detect `DamageImmunity` aura
- [x] When `DamageImmunity` detected: spawn bubble with golden color, larger scale (1.3x), pulse animation
- [x] Download spell icon: `spell_holy_divineintervention.jpg` to `assets/icons/abilities/`
- [x] Add "Divine Shield" to `get_ability_icon_path()` and `SPELL_ICON_ABILITIES` in `rendering/mod.rs`

#### Phase 6: Testing

- [x] Run headless simulations: Paladin vs Warrior (test immunity to melee)
- [x] Run headless simulations: Paladin vs Mage (test immunity to spells + Polymorph)
- [x] Run headless simulations: Paladin vs Rogue (test CC break from Kidney Shot)
- [x] Run headless simulations: Paladin vs Warlock (test DoT purge + Fear break)
- [x] Run headless simulations: Priest+Paladin vs Warrior+Rogue (test AI uses bubble to survive burst)
- [x] Verify combat log shows "Immune" for blocked damage (0 damage auto-attacks during DS)
- [x] Verify combat log shows debuff removal count
- [x] Verify 50% damage reduction applies to auto-attacks and Holy Shock damage
- [x] Verify Divine Shield expires after 12 seconds (activates ~22.96s, expires ~34.96s)
- [x] Verify healing still works during Divine Shield (38, 37, 21 heals during DS)
- [x] `cargo test` — all existing tests pass

## Key Design Decisions

### 1. AuraType::DamageImmunity vs Absorb(999999)

**Chosen: New AuraType.** A finite absorb can be exhausted by enough damage, doesn't block CC, and doesn't conceptually represent immunity. True immunity needs a first-class type.

### 2. Single Aura vs Dual Aura (DamageImmunity + DamageOutputReduction)

**Chosen: Single aura with helper function.** The outgoing 50% penalty is derived from the presence of `DamageImmunity` via `get_divine_shield_damage_penalty()`. Simplicity reviewer confirmed this eliminates an unnecessary AuraType variant and reduces file changes.

### 3. Inline Debuff List vs `is_debuff()` Method

**Chosen: Inline `matches!()` expression.** Only one consumer exists (Divine Shield). Extract to method when Mass Dispel or Purge is added. YAGNI.

### 4. Reuse ShieldBubble vs New DivineShieldBubble

**Chosen: Reuse ShieldBubble.** Extend existing `update_shield_bubbles()` with a different trigger (DamageImmunity vs Absorb), different color/scale. Saves ~80 lines and 3 new system registrations.

### 5. `DivineShieldPending` vs Inline Activation

**Chosen: Pending component.** Architecture reviewer identified that Paladin AI only has immutable `&ActiveAuras`. The pending component pattern (matching Holy Shock, Dispels) provides mutable access in a separate system.

### 6. Mana Cost: 0 vs 75

**Chosen: 0.** Must be usable in worst case (OOM + stunned). 5-minute cooldown is the real cost.

### 7. Duration: 10s (WoW) vs 12s (user choice)

**Chosen: 12s.** User explicitly chose this. Slightly longer than WoW for game pace.

### 8. SpellSchool: Holy vs None

**Chosen: None.** Cannot be locked out by interrupts. A Pummel on Flash of Light must not prevent the panic button.

### 9. Not Dispellable (future Mass Dispel)

**Chosen: Not dispellable now.** `is_magic_dispellable()` returns `false` for `DamageImmunity` by default.

### 10. Enemy AI Target Switching — Deferred

**Chosen: Skip for now.** 4% uptime, healer target priority already favors DPS. Add if sims show problems.

## Edge Cases

| Edge Case | Resolution |
|-----------|------------|
| Projectile in flight when DS activates | Damage blocked at impact by immunity check in `apply_damage_with_absorb()` |
| DoT applied before DS, tick happens during DS | DoT removed on DS activation (debuff purge) |
| Enemy applies Mortal Strike during DS | Blocked — hostile aura application blocked during DamageImmunity |
| PW:S absorb active + Divine Shield | Both coexist; absorb shield not consumed (no damage reaches it) |
| DS used during pre-match countdown | AI guard: never use when gates not opened |
| Paladin stunned + 0 mana | Usable — 0 mana cost |
| DS expires mid-combat | Paladin immediately vulnerable; no DoTs (they were purged). Enemy must reapply |
| Drain Life on DS'd Paladin | Channel continues; damage = 0, therefore healing = 0 for Warlock |
| Auto-attack timer built up during stun | Timer continues; first attack after DS at 50% damage |
| Movement after CC break via DS | One-frame delay: CC purged by `process_divine_shield()`, movement resumes next frame |
| Auto-attack gate after CC break | Same one-frame delay; resolves naturally |
| `DamageReduction` (CoW) + `DamageImmunity` | Moot — DS purges CoW on activation. If somehow reapplied during DS, aura application is blocked |
| Frostbolt slow applied during DS | Blocked by hostile aura application check in `apply_pending_auras()` |
| `WeakenedSoul` during DS | NOT purged (classified as buff/neutral); Paladin keeps PW:S cooldown restriction |
| `ShadowSight` during DS | NOT purged (classified as neutral); Paladin keeps visibility debuff |
| Holy school lockout + Divine Shield | DS uses `SpellSchool::None` — cannot be locked out |

## Files to Modify

| File | Change |
|------|--------|
| `src/states/play_match/components/mod.rs` | Add `DamageImmunity` to `AuraType`; add `DivineShieldPending` component |
| `src/states/play_match/abilities.rs` | Add `DivineShield` variant |
| `src/states/play_match/ability_config.rs` | Add to validation |
| `assets/config/abilities.ron` | Add `DivineShield` definition |
| `src/states/play_match/constants.rs` | Add DS constants |
| `src/states/play_match/combat_core.rs` | Immunity check in `apply_damage_with_absorb()`, `has_damage_immunity()` + `get_divine_shield_damage_penalty()` helpers, outgoing penalty in `combat_auto_attack()` and `process_casting()` |
| `src/states/play_match/combat_ai.rs` | Incapacitation gate bypass; outgoing penalty in instant attacks |
| `src/states/play_match/auras.rs` | Block hostile aura application during immunity; buff stacking prevention |
| `src/states/play_match/class_ai/paladin.rs` | `try_divine_shield()` AI logic |
| `src/states/play_match/effects/divine_shield.rs` | **NEW:** Process `DivineShieldPending` — debuff purge + apply immunity aura |
| `src/states/play_match/effects/mod.rs` | Add `divine_shield` submodule |
| `src/states/play_match/projectiles.rs` | Outgoing damage penalty check at impact |
| `src/states/play_match/rendering/effects.rs` | Extend `update_shield_bubbles()` for golden DS bubble |
| `src/states/play_match/rendering/mod.rs` | Icon mapping |
| `src/states/play_match/systems.rs` | Re-export `process_divine_shield` system |

## References & Research

### Internal References
- Crit system distributed damage sites: `docs/solutions/implementation-patterns/critical-hit-system-distributed-crit-rolls.md`
- Visual effects 3-system pattern: `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md`
- Paladin class implementation: `docs/solutions/implementation-patterns/adding-new-class-paladin.md`
- Existing shield bubble visual: `src/states/play_match/rendering/effects.rs:363-476`
- Aura system: `src/states/play_match/auras.rs`
- Damage application: `src/states/play_match/combat_core.rs:40-105`
- Paladin AI: `src/states/play_match/class_ai/paladin.rs`
- Incapacitation gate: `src/states/play_match/combat_ai.rs:337-344`
- `has_absorb_shield()` helper pattern: `src/states/play_match/combat_core.rs:108`
- `get_physical_damage_reduction()` helper: `src/states/play_match/combat_core.rs:121-129`
- ChargingState CC immunity pattern: `src/states/play_match/auras.rs:116-170`

### External References
- WoW Classic Divine Shield: Spell ID 642 (Wowhead)
- Icon: `spell_holy_divineintervention.jpg`
- Bevy ECS marker component pattern: `With<T>`/`Without<T>` query filters (confirmed viable but unnecessary at 6-entity scale)

### Review Agent Findings
- **Architecture:** Use `DivineShieldPending` deferred pattern; `has_damage_immunity()` helper; single chokepoint at `apply_damage_with_absorb()`
- **Performance:** All changes performance-safe at 6-entity scale; aura scan is ~5-8 comparisons; marker component would save 0 measurable time
- **Simplicity:** Eliminated `DamageOutputReduction` AuraType, `is_debuff()` method, `DivineShieldBubble` component, enemy AI target switching — reduced from 16 to ~15 file changes (but simpler changes in each)
- **Pattern consistency:** All patterns consistent; incapacitation bypass is a clean extension (precedent: `ChargingState` bypass); `try_divine_shield()` follows `try_*` pattern; `effects/divine_shield.rs` follows `effects/holy_shock.rs` pattern
