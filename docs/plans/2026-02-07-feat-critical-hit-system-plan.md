---
title: "feat: Add Critical Hit System"
type: feat
date: 2026-02-07
---

# feat: Add Critical Hit System

## Enhancement Summary

**Deepened on:** 2026-02-07
**Sections enhanced:** 6
**Research agents used:** architecture-strategist, performance-oracle, code-simplicity-reviewer, pattern-recognition-specialist, WoW Classic mechanics researcher, institutional learnings researcher

### Key Improvements from Deepening
1. Identified 3 missed damage sites (class_ai instant attacks: Mortal Strike, Ambush, Eviscerate) — now covered
2. Clarified crit vs damage reduction ordering with explicit code placement
3. Added `GameRng` to auto-attack system signature (was missing from original plan)
4. Resolved crit helper placement: free function `roll_crit(crit_chance, rng)` for universal use
5. Confirmed WoW Classic accuracy: 2.0× melee damage, 1.5× spell/heal, DoTs never crit

### New Considerations Discovered
- Auto-attack `damage_per_target` HashMap batching needs `is_crit` propagation
- Holy Shock pending structs snapshot `caster_crit_chance` (consistent with existing `caster_spell_power` pattern)
- If crit becomes buff-modifiable later, projectile impact-time evaluation is a known limitation to revisit

## Overview

Add critical strikes to the arena combat system. Direct damage abilities, direct healing spells, and auto-attacks can critically hit for bonus damage/healing. Damage-over-time effects, heal-over-time effects, and channel ticks cannot crit. Critical hit chance is a per-class combatant statistic (`crit_chance`) that will later be modifiable by gear.

## Design Decisions

These follow WoW Classic behavior:

- **Crit chance** is stored as a flat `f32` percentage (0.0–1.0) on `Combatant`, not a rating-to-percentage conversion system. The field is named `crit_chance` for simplicity. A rating conversion can be layered on when gear is added.
- **Crit multiplier**: 2.0× for damage, 1.5× for healing (WoW Classic values). In Classic, melee crits deal 200% and spell crits deal 150% — we use 2.0× for ALL damage (both physical and spell) as a simplification that makes crit feel more impactful. Healing crits use the authentic 1.5× value.
- **Formula ordering**: Crit multiplies the **full calculated value** `(Base + Stat × Coefficient) × CritMultiplier`, then reductions (physical damage reduction, healing reduction, absorbs) apply afterward. This matches WoW Classic where crit is applied before armor/resistance reduction.
- **Projectile crit timing**: Crit is rolled at **impact time** (when the projectile hits), not at cast time. This keeps the `Projectile` component unchanged and simplifies implementation. The caster's `crit_chance` is read from the caster entity at impact (caster stats are already queried at impact for damage calculation). **Note**: If crit becomes buff-modifiable in the future, this means temporary crit buffs that expire mid-flight won't affect in-flight projectiles — an accepted simplification.
- **Heroic Strike crits**: The entire enhanced auto-attack (base + bonus damage) is multiplied by the crit multiplier. This matches WoW where "on next melee" abilities crit for the full enhanced amount.
- **Wand shots**: Use the same `crit_chance` as melee auto-attacks — no separate spell crit stat. WoW Classic has separate melee/spell crit, but for our simplified autobattler a single stat is sufficient.
- **RNG determinism**: Adding crit rolls changes the `GameRng` call sequence. This is accepted; seeds are for debugging, not saved replays.
- **No crit cap**: No artificial ceiling on crit chance. Balance is controlled through per-class base values.
- **Crit helper**: Use a free function `roll_crit(crit_chance: f32, rng: &mut GameRng) -> bool` rather than a method on `Combatant`, so it works uniformly at all damage sites including Holy Shock pending structs (which have `caster_crit_chance: f32` but no `&Combatant`).

## Base Crit Chance Per Class

| Class   | Base Crit Chance | Rationale |
|---------|-----------------|-----------|
| Warrior | 8%  (0.08) | Physical DPS, benefits from crit on Mortal Strike + auto-attacks |
| Mage    | 6%  (0.06) | Spell crit on Frostbolt is impactful; lower base to balance CC utility |
| Rogue   | 10% (0.10) | Highest base crit — burst class identity, rewards landing attacks |
| Priest  | 4%  (0.04) | Healer, low offensive crit; healing crits provide emergency throughput |
| Warlock | 5%  (0.05) | DoT-focused class; crits only apply to Shadow Bolt/Immolate direct portion |
| Paladin | 6%  (0.06) | Hybrid healer/melee; crits on Holy Shock and Flash of Light add burst healing |

## What Can and Cannot Crit

| Source | Can Crit? | Notes |
|--------|-----------|-------|
| Auto-attacks (melee) | YES | Includes Heroic Strike enhanced swings |
| Auto-attacks (wand) | YES | Same crit_chance stat |
| Direct damage spells (Mortal Strike, Frostbolt, Mind Blast, etc.) | YES | Multiplied at calculation site |
| Instant damage from class_ai (Ambush, Sinister Strike, Eviscerate, Frost Nova) | YES | Via `calculate_ability_damage_config` |
| Direct healing spells (Flash Heal, Flash of Light, Holy Light) | YES | 1.5× multiplier |
| Holy Shock (damage and heal) | YES | Both portions can crit independently |
| Projectile spells (Frostbolt, Shadow Bolt) | YES | Crit rolled at impact |
| Immolate direct damage portion | YES | Initial hit at cast time |
| DoT ticks (Corruption, Rend, SW:Pain, Immolate DoT, Curse of Agony) | NO | Never — per spec |
| Channel ticks (Drain Life damage) | NO | Never — per spec |
| Channel tick healing (Drain Life self-heal) | NO | Never |
| Absorb shields (Power Word: Shield) | NO | Not a damage/heal event |

## Acceptance Criteria

- [x] `Combatant` struct has `crit_chance: f32` field, initialized per-class
- [x] Free function `roll_crit(crit_chance: f32, rng: &mut GameRng) -> bool` in `combat_core.rs`
- [x] Constants `CRIT_DAMAGE_MULTIPLIER` (2.0) and `CRIT_HEALING_MULTIPLIER` (1.5) in `constants.rs`
- [x] Auto-attacks can crit (melee and wand) — `GameRng` added to `combat_auto_attack` system signature
- [x] Direct damage abilities can crit at all pathways (cast completion, projectile impact, Holy Shock, class_ai instant attacks)
- [x] Direct healing abilities can crit at both pathways (cast completion, Holy Shock)
- [x] DoT ticks never crit (`process_dot_ticks` unchanged)
- [x] Channel ticks never crit (`process_channeling` unchanged)
- [x] `StructuredEventData::Damage` and `Healing` have `is_crit: bool` field
- [x] `log_damage()` and `log_healing()` accept and display crit status
- [x] Combat log messages show "CRITS" instead of "hits"/"heals" for critical strikes
- [x] `FloatingCombatText` has `is_crit: bool` field
- [x] Crit FCT renders at 32pt (vs normal 24pt) with "!" suffix on the number
- [x] Headless simulation works correctly with crits (no visual-only crashes)
- [x] Compiles and runs in both graphical and headless modes

## Implementation Plan

### Phase 1: Core Stat, Constants, Combat Log, and Crit Helper

**Files:** `components/mod.rs`, `constants.rs`, `combat/log.rs`, `combat_core.rs`

Do all foundational changes in one pass to avoid intermediate broken states.

1. Add crit constants to `constants.rs`:
   ```rust
   // ============================================================================
   // Critical Strike
   // ============================================================================

   /// Critical strike damage multiplier (2x in WoW Classic for melee; we use 2x for all damage)
   pub const CRIT_DAMAGE_MULTIPLIER: f32 = 2.0;

   /// Critical strike healing multiplier (1.5x in WoW Classic)
   pub const CRIT_HEALING_MULTIPLIER: f32 = 1.5;
   ```

2. Add `crit_chance: f32` field to `Combatant` struct (after `spell_power`, line ~402):
   ```rust
   /// Critical strike chance (0.0 = 0%, 1.0 = 100%). Determines probability of
   /// dealing bonus damage/healing on direct abilities and auto-attacks.
   pub crit_chance: f32,
   ```

3. Extend the `Combatant::new()` stat tuple to include `crit_chance` — add it as the 11th element:
   ```rust
   let (..., spell_power, crit_chance, movement_speed) = match class {
       Warrior => (..., 0.0, 0.08, 5.0),
       Mage    => (..., 50.0, 0.06, 4.5),
       Rogue   => (..., 0.0, 0.10, 6.0),
       Priest  => (..., 40.0, 0.04, 5.0),
       Warlock => (..., 45.0, 0.05, 4.5),
       Paladin => (..., 35.0, 0.06, 5.0),
   };
   ```

4. Initialize `crit_chance` in the `Self { ... }` constructor block.

5. Also update `new_with_curse_prefs` to pass through `crit_chance` (it delegates to `new()`).

6. Add free function in `combat_core.rs` (top of file, near `apply_damage_with_absorb`):
   ```rust
   /// Roll a critical strike check. Returns true if the roll is a crit.
   pub fn roll_crit(crit_chance: f32, rng: &mut GameRng) -> bool {
       rng.random_f32() < crit_chance
   }
   ```

7. Add `is_crit: bool` to `StructuredEventData::Damage` and `StructuredEventData::Healing` variants in `combat/log.rs`.

8. Update `log_damage()` and `log_healing()` signatures to accept `is_crit: bool`.

9. Update log message format:
   - Damage crit: `"Team 1 Warrior's Mortal Strike CRITS Team 2 Mage for 90 damage"`
   - Healing crit: `"Team 1 Priest's Flash Heal CRITICALLY heals Team 1 Warrior for 67"`
   - Non-crit messages unchanged (existing "hits"/"heals" wording)

10. Update ALL existing callers of `log_damage()` and `log_healing()` to pass `is_crit: false` temporarily. This includes callers in `combat_core.rs`, `projectiles.rs`, `effects/holy_shock.rs`, and `auras.rs`. This keeps the build compiling while individual pathways are updated in Phase 2.

11. Add `is_crit: bool` field to `FloatingCombatText` component. Update all existing FCT spawn sites to set `is_crit: false` temporarily.

### Phase 2: Damage and Healing Pathway Integration

**Files:** `combat_core.rs`, `projectiles.rs`, `effects/holy_shock.rs`

For each pathway, the pattern is:
```rust
let is_crit = roll_crit(caster.crit_chance, &mut game_rng);
let damage = if is_crit { damage * CRIT_DAMAGE_MULTIPLIER } else { damage };
// Pass is_crit to FCT and combat log
```

**Important ordering**: Crit multiplier is applied to the raw `(Base + Stat × Coefficient)` result. Damage reductions (Curse of Weakness physical reduction, Mortal Strike healing reduction) and absorb shields are applied AFTER crit. The existing code already applies reductions after the base damage calculation, so the crit multiplication slots in between.

#### 2a. Auto-attacks (`combat_core.rs`: `combat_auto_attack`)

- **Add `mut game_rng: ResMut<GameRng>` parameter** to the `combat_auto_attack` system signature. This is currently missing and is required for crit rolls.
- The auto-attack damage pipeline is: `base_damage + bonus_damage` → `physical_damage_reduction` → `total_damage`. The crit multiplier should apply to the full `base_damage + bonus_damage` BEFORE physical damage reduction:
  ```rust
  let base_damage = combatant.attack_damage + combatant.next_attack_bonus_damage;
  let is_crit = roll_crit(combatant.crit_chance, &mut game_rng);
  let crit_damage = if is_crit { base_damage * CRIT_DAMAGE_MULTIPLIER } else { base_damage };
  let damage_reduction = get_physical_damage_reduction(auras.as_deref());
  let total_damage = (crit_damage * (1.0 - damage_reduction)).max(0.0);
  ```
- The `attacks` vector / `damage_per_target` HashMap carries damage to FCT spawning. Add `is_crit: bool` to the tuple elements so the FCT and combat log receive it. Since auto-attacks from one attacker to one target happen at most once per frame (attack timer resets), the batching HashMap does not cause crit/non-crit collision.
- Pass `is_crit` to FCT (`is_crit: is_crit` on the `FloatingCombatText` component) and `log_damage()`.

#### 2b. Cast completion — damage (`combat_core.rs`: `process_casting`)

- After `calculate_ability_damage_config()` call (line ~1159), before physical damage reduction:
  ```rust
  let mut ability_damage = caster.calculate_ability_damage_config(def, &mut game_rng);
  let is_crit = roll_crit(caster.crit_chance, &mut game_rng);
  if is_crit { ability_damage *= CRIT_DAMAGE_MULTIPLIER; }
  // Then physical damage reduction applies (existing code)
  ```
- Carry `is_crit` through the `completed_casts` vector to the FCT and log section.
- For projectile abilities, the cast completion path spawns a `Projectile` component and skips direct damage. No crit roll here for projectiles — crit is rolled at impact (2d).

#### 2c. Cast completion — healing (`combat_core.rs`: `process_casting`)

- After `calculate_ability_healing_config()` call:
  ```rust
  let ability_healing = caster.calculate_ability_healing_config(def, &mut game_rng);
  let is_crit_heal = roll_crit(caster.crit_chance, &mut game_rng);
  let ability_healing = if is_crit_heal { ability_healing * CRIT_HEALING_MULTIPLIER } else { ability_healing };
  ```
- Healing reduction (Mortal Strike) applies AFTER crit (existing code order is already correct).
- Pass `is_crit_heal` to FCT and combat log.

#### 2d. Projectile impact (`projectiles.rs`: `process_projectile_hits`)

- After `calculate_ability_damage_config()` at impact (line ~150):
  ```rust
  let mut ability_damage = caster_combatant.calculate_ability_damage_config(def, &mut game_rng);
  let is_crit = roll_crit(caster_combatant.crit_chance, &mut game_rng);
  if is_crit { ability_damage *= CRIT_DAMAGE_MULTIPLIER; }
  ```
- Pass `is_crit` to FCT and combat log.
- **Note**: If caster is dead when projectile lands, `combatants.get(projectile.caster)` returns `Err` and projectile is despawned without damage (existing behavior). Crit is irrelevant in that case.

#### 2e. Holy Shock damage (`effects/holy_shock.rs`)

- Add `caster_crit_chance: f32` to `HolyShockDamagePending` struct (follows existing `caster_spell_power` snapshot pattern).
- At the spawn site (in `paladin.rs` class_ai), set `caster_crit_chance: combatant.crit_chance`.
- In `process_holy_shock_damage()`, after `raw_damage` calculation:
  ```rust
  let is_crit = roll_crit(pending.caster_crit_chance, &mut game_rng);
  if is_crit { raw_damage *= CRIT_DAMAGE_MULTIPLIER; }
  ```
- Pass `is_crit` to FCT and combat log.

#### 2f. Holy Shock healing (`effects/holy_shock.rs`)

- Add `caster_crit_chance: f32` to `HolyShockHealPending` struct.
- At the spawn site, set `caster_crit_chance: combatant.crit_chance`.
- In `process_holy_shock_heals()`, after `heal_amount` calculation:
  ```rust
  let is_crit = roll_crit(pending.caster_crit_chance, &mut game_rng);
  if is_crit { heal_amount *= CRIT_HEALING_MULTIPLIER; }
  ```
- Pass `is_crit` to FCT and combat log.

#### 2g. Pathways that do NOT change:
- `process_dot_ticks()` in `auras.rs` — no crit logic added. FCT `is_crit: false`.
- `process_channeling()` in `combat_core.rs` — no crit logic added. FCT `is_crit: false` for both damage and healing ticks.

### Phase 3: Floating Combat Text Rendering

**Files:** `rendering/effects.rs`

Update `render_floating_combat_text()` to use `is_crit` for visual differentiation:

```rust
let font_size = if fct.is_crit { 32.0 } else { 24.0 };
let display_text = if fct.is_crit {
    format!("{}!", fct.text)
} else {
    fct.text.clone()
};
```

Replace the hardcoded `24.0` font size with the `font_size` variable. The "!" suffix and larger size apply to both damage and healing crits (matching WoW behavior).

### Phase 4: Compile and Test

1. `cargo build --release` — fix all compilation errors from signature changes.
2. Run headless test:
   ```bash
   echo '{"team1":["Warrior","Priest"],"team2":["Rogue","Paladin"]}' > /tmp/crit_test.json
   cargo run --release -- --headless /tmp/crit_test.json
   ```
3. Verify in match log:
   - Some hits show "CRITS" in the log text
   - Crit damage values are approximately 2× normal damage for that ability
   - Crit healing values are approximately 1.5× normal healing
   - DoT tick lines never show "CRITS"
   - Channel tick lines never show "CRITS"
4. Run graphical client to verify crit FCT renders larger with "!" suffix.
5. Run multiple matches to spot-check crit frequency is reasonable (Rogue should crit ~10% of the time, Priest ~4%).

## Complete File Change List

| File | Changes |
|------|---------|
| `src/states/play_match/constants.rs` | Add `CRIT_DAMAGE_MULTIPLIER`, `CRIT_HEALING_MULTIPLIER` |
| `src/states/play_match/components/mod.rs` | Add `crit_chance` to `Combatant`, `is_crit` to `FloatingCombatText` |
| `src/combat/log.rs` | Add `is_crit` to `Damage`/`Healing` variants, update `log_damage`/`log_healing` |
| `src/states/play_match/combat_core.rs` | Add `roll_crit()` free function, add `GameRng` to auto-attack system, crit in `combat_auto_attack` + `process_casting` |
| `src/states/play_match/projectiles.rs` | Crit roll in `process_projectile_hits` |
| `src/states/play_match/effects/holy_shock.rs` | Add `crit_chance` to pending structs, crit rolls in both processors |
| `src/states/play_match/class_ai/paladin.rs` | Set `caster_crit_chance` when spawning Holy Shock pending components |
| `src/states/play_match/rendering/effects.rs` | Crit-aware FCT rendering (font size 32pt, "!" suffix) |

## Architectural Notes

### Why distributed crit, not centralized

Pattern analysis shows only 2 of 6 damage sites use `calculate_ability_damage_config()`. Auto-attacks, channel ticks, DoT ticks, and Holy Shock all bypass it with different calculation paths. Centralizing crit in the calculate methods would only cover a fraction of sites and still require per-site handling for FCT and logging. The distributed approach matches the existing codebase pattern where each damage site is a self-contained unit.

### Holy Shock crit_chance snapshotting

Holy Shock pending structs snapshot `caster_crit_chance` at spawn time (matching `caster_spell_power`), while projectiles read crit_chance from the live caster entity at impact. Since Holy Shock is instant (no travel time), this has no practical impact. The asymmetry is intentional and documented.

### Performance impact

Zero measurable impact. 1-6 entities, damage events at sub-Hz frequency per combatant, one extra RNG call (~5ns) per event. The `Combatant` struct is already ~200+ bytes with heap-allocated fields; 4 more bytes changes nothing. No new ECS systems, no new queries.

## References

- **Stat scaling system**: `design-docs/stat-scaling-system.md` — damage formula, crit listed as planned enhancement (#1)
- **Bevy patterns**: `design-docs/bevy-patterns.md` — ECS query patterns, `Without<T>` filters
- **Visual effect pattern**: `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md`
- **Paladin implementation**: `docs/solutions/implementation-patterns/adding-new-class-paladin.md` — pending component context propagation pattern
- **WoW Classic crit behavior**: Damage crits = 200%, spell crits = 150% (we use 200% for all damage), healing crits = 150%, DoTs cannot crit, crit applies before armor/resistance reduction
