---
title: "feat: Add strategic option layers to Warrior, Mage, and Paladin"
type: feat
status: active
date: 2026-04-05
origin: docs/brainstorms/2026-04-05-class-strategic-options-requirements.md
---

# feat: Add Strategic Option Layers to Warrior, Mage, and Paladin

## Overview

Add configurable strategic choices to Warrior (Shout), Mage (Armor), and Paladin (Aura) — following the same pattern as Rogue openers and Warlock curse preferences. Each choice changes observable match behavior without encroaching on future talent tree design space.

## Problem Frame

Rogue and Warlock are the only classes with configurable strategic options. The other classes play identically every match regardless of matchup, limiting team composition depth and counter-play. (see origin: docs/brainstorms/2026-04-05-class-strategic-options-requirements.md)

## Requirements Trace

- R1-R5: Warrior Shout choice (Battle/Demoralizing/Commanding)
- R6-R11: Mage Armor choice (Frost Armor/Mage Armor/Molten Armor)
- R12-R17: Paladin Aura choice (Devotion/Shadow Resistance/Concentration)
- R18-R20: Configuration at match setup, sensible defaults, headless config support

## Scope Boundaries

- Priest excluded — no natural "choose one" mechanic
- Tactical knobs only — not spec-level changes
- No new damage/healing spells — only buff/debuff/passive utility
- Talent trees are a separate future system

## Context & Research

### Relevant Code and Patterns

**Strategic option pattern chain** (from Rogue/Warlock):
1. Enum in `match_config.rs` with `Default`, `name()`, `description()` helpers
2. `Vec<Enum>` fields on `MatchConfig` (one per team, indexed by slot)
3. Field on `Combatant` component in `components/combatant.rs`
4. Extract from config in `play_match/mod.rs` initialization loop
5. Parse from JSON strings in `headless/config.rs`
6. Match on field in class AI `decide_*_action()` function

**Existing buff patterns:**
- Battle Shout → `AttackPowerIncrease` aura, team-wide, instant
- Devotion Aura → `DamageTakenReduction` aura, team-wide, instant, 100yd range
- Ice Barrier → `Absorb` aura, self-cast, cooldown

**Existing aura infrastructure:**
- `SpellResistanceBuff` aura type exists (school + magnitude) — usable for Shadow Resistance Aura
- `MaxHealthIncrease` aura type exists — usable for Commanding Shout
- `DamageReduction` reduces outgoing physical damage — used by Curse of Weakness
- Spell resistance system is fully implemented on Combatant with `get_resistance()` and damage reduction formula

### Wowhead Classic Research

| Ability | WoW Classic Values | Sim Adaptation |
|---------|-------------------|----------------|
| Demoralizing Shout | -146 AP, 10yd, 30s | -15 AP, 30yd, 120s (scaled to sim AP range ~20-35) |
| Commanding Shout | Not in Classic (TBC) | +40 max HP (sim-original, ~20% of Warrior base 200 HP) |
| Ice Armor | +290 armor, proc: 30% move slow + 25% atk speed slow, 5s | Proc: 30% move slow + 25% atk speed slow, 5s |
| Mage Armor | +5 all resist, 30% mana regen while casting | +8 mana/s regen (flat buff, sim-adapted) |
| Molten Armor | Not in Classic (TBC) | +5% crit chance (sim-original) |
| Shadow Resistance Aura | +60 shadow resist, 30yd | +30 shadow resist (scaled for sim resistance formula) |
| Concentration Aura | 35% pushback resist, 30yd | 15% cast time reduction (sim has no pushback; adapted) |
| Devotion Aura (ref) | +735 armor, 30yd | Already: 10% damage taken reduction |

## Key Technical Decisions

- **Concentration Aura adapted to cast time reduction**: The sim does not model spell pushback. Rather than adding a new pushback system (scope creep), Concentration Aura grants a small cast time reduction to allied casters. This preserves the "helps casters" intent without requiring new core mechanics. Uses a new `CastTimeReduction` aura type.

- **Commanding Shout and Molten Armor are sim-original**: Neither exists in WoW Classic. Values are designed to be balanced within the sim's stat ranges rather than adapted from WoW data.

- **Frost Armor proc handled in auto-attack system**: Rather than building a general proc-on-hit framework, the Frost Armor slow is applied via a targeted check in the auto-attack damage path. When a melee auto-attack hits a target with `FrostArmor` aura active, apply `MovementSpeedSlow` + `AttackSpeedSlow` to the attacker.

- **Mage Armor simplified to flat mana regen**: WoW's "30% mana regen while casting" depends on the 5-second rule, which the sim doesn't model. Simplified to a flat mana regen increase via a new `ManaRegenIncrease` aura type.

- **New AuraTypes needed**: `AttackPowerReduction`, `CritChanceIncrease`, `ManaRegenIncrease`, `AttackSpeedSlow`, `CastTimeReduction`. All follow existing patterns (magnitude-based, duration-based).

- **Constructor evolution**: Rather than adding more params to `new_with_curse_prefs()`, add new fields with defaults to `Combatant` and set them after construction in the initialization loop (same pattern that `rogue_opener`/`warlock_curse_prefs` already use).

## Open Questions

### Resolved During Planning

- **What are Demoralizing Shout / Commanding Shout values?** Demoralizing: -15 AP (scaled from Classic's -146 to sim's AP range). Commanding: +40 HP (sim-original, ~20% HP boost).
- **What are Mage Armor magnitudes?** Frost Armor: proc 30% move slow + 25% attack speed slow for 5s. Mage Armor: +8 mana/s regen. Molten Armor: +5% crit.
- **Shadow resistance value?** +30 shadow resistance. At this value, the WoW-style resistance formula (resist / (resist * 5/3 + 300)) gives ~14% damage reduction vs shadow, meaningful but not overpowering.
- **How does Concentration Aura work without pushback?** Adapted to 15% cast time reduction for allies.
- **Molten Armor: crit, reflect, or both?** Crit only (+5%). Damage reflect would require a new on-hit proc system similar to Frost Armor but offensive — adding both proc types in one feature is excessive scope. Can add reflect later.

### Deferred to Implementation

- **Frost Armor proc rate**: WoW applies it on every melee hit. If this feels too strong in testing, may need an internal cooldown.
- **Exact AttackSpeedSlow interaction with auto-attack timer**: Need to verify how slowing attack speed interacts with the `1.0 / attack_speed` interval calculation in `combat_core/auto_attack.rs`.

## High-Level Technical Design

> *This illustrates the intended approach and is directional guidance for review, not implementation specification. The implementing agent should treat it as context, not code to reproduce.*

```
Config Flow:
  HeadlessMatchConfig (JSON) 
    → parse_warrior_shout() / parse_mage_armor() / parse_paladin_aura()
    → MatchConfig { team1_warrior_shouts, team1_mage_armors, team1_paladin_auras, ... }
    → Combatant { warrior_shout, mage_armor, paladin_aura }

AI Decision Flow (pre-match):
  Warrior: match combatant.warrior_shout → cast BattleShout | DemoralizingShout | CommandingShout
  Mage: match combatant.mage_armor → cast FrostArmor | MageArmorSpell | MoltenArmor
  Paladin: match combatant.paladin_aura → cast DevotionAura | ShadowResistanceAura | ConcentrationAura

New Aura Effects (in combat systems):
  AttackPowerReduction → reduce AP in damage calc (mirror of AttackPowerIncrease)
  CritChanceIncrease → add to crit roll in damage calc
  ManaRegenIncrease → add to mana regen tick
  AttackSpeedSlow → increase auto-attack interval
  CastTimeReduction → reduce cast time on spell start
  FrostArmor (special) → on melee hit received, apply MovementSpeedSlow + AttackSpeedSlow to attacker
```

## Implementation Units

- [ ] **Unit 1: Define strategic option enums and config plumbing**

  **Goal:** Add `WarriorShout`, `MageArmor`, `PaladinAura` enums and wire them through MatchConfig, Combatant, headless config, and combatant initialization.

  **Requirements:** R1, R6, R12, R18, R19, R20

  **Dependencies:** None

  **Files:**
  - Modify: `src/states/match_config.rs` — add enums + MatchConfig fields
  - Modify: `src/states/play_match/components/combatant.rs` — add fields to Combatant
  - Modify: `src/states/play_match/mod.rs` — extract from config in init loop
  - Modify: `src/headless/config.rs` — add JSON fields and parse functions
  - Test: `src/headless/config.rs` (existing test module)

  **Approach:**
  - Follow the exact pattern of `RogueOpener` and `WarlockCurse` enums in match_config.rs
  - Each enum: `Default` trait (Battle Shout, Frost Armor, Devotion Aura), `Clone`, `Copy`, `PartialEq`, `Serialize`, `Deserialize`
  - Add `name()` and `description()` helper methods for UI
  - MatchConfig gets `team1_warrior_shouts: Vec<WarriorShout>`, `team2_warrior_shouts`, same for mage/paladin
  - Combatant gets `warrior_shout: WarriorShout`, `mage_armor: MageArmor`, `paladin_aura: PaladinAura` with defaults
  - Set fields in `play_match/mod.rs` init loop after `Combatant::new_with_curse_prefs()` call
  - Headless config: `team1_warrior_shouts: Vec<String>`, parse functions match string to enum

  **Patterns to follow:**
  - `RogueOpener` enum definition in `match_config.rs` (~line 12-38)
  - `parse_rogue_opener()` in `headless/config.rs` (~line 188-192)
  - Init loop in `play_match/mod.rs` that extracts from config by slot index

  **Test scenarios:**
  - Happy path: Parse valid JSON with `"warrior_shouts": ["Demoralizing"]` → correct enum value
  - Happy path: Parse valid JSON with `"mage_armors": ["MageArmor"]` → correct enum value
  - Happy path: Parse valid JSON with `"paladin_auras": ["ShadowResistance"]` → correct enum value
  - Edge case: Missing/empty config fields → defaults (BattleShout, FrostArmor, DevotionAura)
  - Edge case: Unrecognized string → falls back to default

  **Verification:**
  - `cargo test` passes
  - Headless config with new fields parses without error

- [ ] **Unit 2: Add new AuraTypes and AbilityType variants**

  **Goal:** Add the new aura types and ability variants needed by all three class strategic options, plus ability definitions in abilities.ron.

  **Requirements:** R2-R4, R7-R9, R13-R15

  **Dependencies:** None (can be done in parallel with Unit 1)

  **Files:**
  - Modify: `src/states/play_match/components/auras.rs` — add 5 new AuraType variants
  - Modify: `src/states/play_match/abilities.rs` — add ability variants to AbilityType enum
  - Modify: `src/states/play_match/ability_config.rs` — add to validation list
  - Modify: `assets/config/abilities.ron` — add 6 new ability definitions
  - Test: ability config validation test (existing)

  **Approach:**
  - New AuraType variants: `AttackPowerReduction`, `CritChanceIncrease`, `ManaRegenIncrease`, `AttackSpeedSlow`, `CastTimeReduction`
  - Document each with comments following existing pattern (magnitude meaning)
  - New AbilityType variants: `DemoralizingShout`, `CommandingShout`, `FrostArmor`, `MageArmorSpell`, `MoltenArmor`, `ShadowResistanceAura`, `ConcentrationAura`
  - Note: `MageArmorSpell` avoids collision with the `MageArmor` config enum name
  - abilities.ron entries — all instant cast (0.0 cast time), team-wide range for shouts/auras (30.0), self-range (0.0) for armors:
    - DemoralizingShout: instant, 30yd, 0 mana (rage-free), `AttackPowerReduction` magnitude 15.0, duration 120s
    - CommandingShout: instant, 30yd, 0 mana, `MaxHealthIncrease` magnitude 40.0, duration 120s
    - FrostArmor: instant, self, 0 mana, special handling (self-buff marker, proc handled separately)
    - MageArmorSpell: instant, self, 0 mana, `ManaRegenIncrease` magnitude 8.0, duration 600s
    - MoltenArmor: instant, self, 0 mana, `CritChanceIncrease` magnitude 0.05, duration 600s
    - ShadowResistanceAura: instant, 100yd, 0 mana, `SpellResistanceBuff` magnitude 30.0, duration 600s, spell_school Shadow
    - ConcentrationAura: instant, 100yd, 0 mana, `CastTimeReduction` magnitude 0.15, duration 600s

  **Patterns to follow:**
  - Existing AuraType variant documentation style in `components/auras.rs`
  - BattleShout and DevotionAura entries in `abilities.ron` for team buffs
  - IceBarrier entry for self-buffs

  **Test scenarios:**
  - Happy path: Ability config validation passes with all new abilities in expected list
  - Happy path: All new abilities load from abilities.ron without parse errors

  **Verification:**
  - `cargo test` passes, especially ability config validation
  - `cargo build` compiles with new variants

- [ ] **Unit 3: Implement new aura effects in combat systems**

  **Goal:** Make the 5 new AuraType variants actually affect combat — damage reduction from AP debuff, crit increase, mana regen, attack speed slow, and cast time reduction. Plus the Frost Armor on-hit proc.

  **Requirements:** R3, R4, R7, R8, R9, R14, R15

  **Dependencies:** Unit 2 (AuraTypes must exist)

  **Files:**
  - Modify: `src/states/play_match/combat_core/damage.rs` — handle `AttackPowerReduction` and `CritChanceIncrease` in damage calc
  - Modify: `src/states/play_match/combat_core/auto_attack.rs` — handle `AttackSpeedSlow` on attack interval, add Frost Armor proc
  - Modify: `src/states/play_match/combat_core/casting.rs` or wherever cast time is calculated — handle `CastTimeReduction`
  - Modify: `src/states/play_match/auras.rs` — handle `ManaRegenIncrease` in mana regen tick (if mana regen is processed here)
  - Test: `src/states/play_match/combat_core/damage.rs` or new test file

  **Approach:**
  - **AttackPowerReduction**: In damage calc, check attacker's auras for this type, subtract magnitude from effective AP. Mirror of how `AttackPowerIncrease` is handled but as a debuff on the attacker.
  - **CritChanceIncrease**: In crit roll, check attacker's auras for this type, add magnitude to base crit chance.
  - **ManaRegenIncrease**: In mana regen tick, check entity's auras for this type, add magnitude to regen rate.
  - **AttackSpeedSlow**: In auto-attack interval calculation, check attacker's auras for this type, multiply interval by `1.0 / (1.0 - magnitude)` (e.g., 25% slow → 1.33x interval).
  - **CastTimeReduction**: When calculating cast time for a spell, check caster's auras for this type, multiply cast time by `(1.0 - magnitude)` (e.g., 15% → 0.85x).
  - **Frost Armor proc**: In the auto-attack hit path, after damage is applied, check if the target has a `FrostArmor` aura. If so, apply `MovementSpeedSlow` (magnitude 0.7 = 30% slow) and `AttackSpeedSlow` (magnitude 0.25) to the attacker for 5s. Use `AuraPending` pattern.

  **Patterns to follow:**
  - How `SpellResistanceBuff` is checked in `damage.rs` (iterating active auras)
  - How `DamageReduction` and `CastTimeIncrease` modify combat values
  - `AuraPending` spawn pattern for applying proc effects

  **Test scenarios:**
  - Happy path: Combatant with AttackPowerReduction aura deals less damage
  - Happy path: Combatant with CritChanceIncrease aura has higher effective crit
  - Happy path: Combatant with ManaRegenIncrease aura regenerates mana faster
  - Happy path: Combatant with AttackSpeedSlow attacks less frequently
  - Happy path: Combatant with CastTimeReduction casts spells faster
  - Integration: Melee hit on target with FrostArmor → attacker gets slowed
  - Edge case: Multiple AttackPowerReduction auras stack additively
  - Edge case: CritChanceIncrease doesn't push effective crit above 100%

  **Verification:**
  - `cargo test` passes
  - Headless match with Warrior (Demoralizing Shout) vs melee shows reduced damage in logs
  - Headless match with Mage (Frost Armor) vs Warrior shows slow applied in logs

- [ ] **Unit 4: Update Warrior AI for shout choice**

  **Goal:** Warrior AI reads `combatant.warrior_shout` preference and casts the chosen shout during pre-match instead of always casting Battle Shout.

  **Requirements:** R1, R2, R3, R4, R5

  **Dependencies:** Units 1, 2, 3

  **Files:**
  - Modify: `src/states/play_match/class_ai/warrior.rs` — update `try_battle_shout()` or add shout selection logic
  - Test: headless match test or dedicated unit test

  **Approach:**
  - Rename `try_battle_shout()` to `try_shout()` or add a dispatch function
  - Match on `combatant.warrior_shout` to determine which AbilityType to use
  - Battle Shout: existing team buff logic (apply to allies)
  - Demoralizing Shout: apply debuff to nearby enemies instead of buffing allies. Check for existing debuff to avoid reapplication (similar dedup pattern as Battle Shout)
  - Commanding Shout: apply MaxHealthIncrease buff to allies (same pattern as Battle Shout but different aura)
  - All three share the same priority position in decide_warrior_action() — first thing attempted

  **Patterns to follow:**
  - Existing `try_battle_shout()` function for team buff application
  - `battle_shouted_this_frame` HashSet deduplication pattern
  - Warlock's `try_spread_curses()` for matching on a preference field

  **Test scenarios:**
  - Happy path: Warrior with default shout casts Battle Shout (backwards compatible)
  - Happy path: Warrior with Demoralizing shout applies AP reduction to enemies
  - Happy path: Warrior with Commanding shout applies HP increase to allies
  - Integration: Demoralizing Shout visible in combat log as debuff application
  - Edge case: Multiple warriors with different shouts — each applies their own

  **Verification:**
  - Headless match: Warrior with each shout choice produces different combat log entries
  - Default config produces identical behavior to current

- [ ] **Unit 5: Update Mage AI for armor choice**

  **Goal:** Mage AI reads `combatant.mage_armor` preference and self-casts the chosen armor during pre-match, in addition to Ice Barrier.

  **Requirements:** R6, R7, R8, R9, R10, R11

  **Dependencies:** Units 1, 2, 3

  **Files:**
  - Modify: `src/states/play_match/class_ai/mage.rs` — add armor cast logic in pre-match priority
  - Test: headless match test

  **Approach:**
  - Add `try_mage_armor()` function that matches on `combatant.mage_armor` preference
  - Cast the corresponding ability (FrostArmor / MageArmorSpell / MoltenArmor) on self
  - Insert this in the pre-match priority after Arcane Intellect buff but before combat abilities
  - Only cast once — check if the armor aura is already active before recasting
  - Uses a GCD (self-cast buff like Ice Barrier) per R10
  - Armor auras should have long duration (600s) so they don't expire during a match

  **Patterns to follow:**
  - `try_ice_barrier()` for self-buff casting pattern
  - Aura presence check before recasting

  **Test scenarios:**
  - Happy path: Mage with default (Frost Armor) casts Frost Armor in pre-match
  - Happy path: Mage with Mage Armor gets mana regen buff
  - Happy path: Mage with Molten Armor gets crit chance buff
  - Integration: Frost Armor proc applies slow when Warrior hits Mage
  - Edge case: Armor doesn't recast if already active

  **Verification:**
  - Headless match: Each armor choice visible in combat log
  - Mage with Molten Armor has visibly higher crit rate in extended matches

- [ ] **Unit 6: Update Paladin AI for aura choice**

  **Goal:** Paladin AI reads `combatant.paladin_aura` preference and applies the chosen aura instead of always applying Devotion Aura.

  **Requirements:** R12, R13, R14, R15, R16, R17

  **Dependencies:** Units 1, 2, 3

  **Files:**
  - Modify: `src/states/play_match/class_ai/paladin.rs` — update `try_devotion_aura()` or add aura selection logic
  - Test: headless match test

  **Approach:**
  - Rename `try_devotion_aura()` to `try_aura()` or add dispatch logic
  - Match on `combatant.paladin_aura` to determine which AbilityType to use
  - All three auras use the same team-wide application pattern (100yd range, apply to all allies)
  - Devotion Aura: existing behavior (DamageTakenReduction)
  - Shadow Resistance Aura: apply SpellResistanceBuff with Shadow school to allies
  - Concentration Aura: apply CastTimeReduction to allies
  - Auras are passive — no GCD, no mana cost (per R16)
  - Same deduplication pattern as current Devotion Aura

  **Patterns to follow:**
  - Existing `try_devotion_aura()` function and `devotion_aura_this_frame` HashSet
  - Same team-wide buff application pattern

  **Test scenarios:**
  - Happy path: Paladin with default aura applies Devotion Aura (backwards compatible)
  - Happy path: Paladin with Shadow Resistance applies shadow resist to team
  - Happy path: Paladin with Concentration applies cast time reduction to team
  - Integration: Shadow Resistance Aura reduces Shadow Bolt damage in combat log
  - Integration: Concentration Aura makes Mage Frostbolt cast faster
  - Edge case: Aura doesn't stack if already applied

  **Verification:**
  - Headless match: Each aura choice produces different combat log entries
  - Shadow Resistance Aura vs Warlock team shows measurable damage reduction

## System-Wide Impact

- **Interaction graph**: New aura effects interact with damage calculation (AP reduction, crit), auto-attack system (attack speed slow, Frost Armor proc), casting system (cast time reduction), and mana regen. All existing aura tick/expiration systems handle new types automatically via the AuraType enum.
- **Error propagation**: No new error paths — aura application follows existing `AuraPending` pattern with graceful fallback.
- **State lifecycle risks**: Frost Armor proc creates secondary auras — need to ensure these expire correctly and don't leak between matches.
- **API surface parity**: Headless and graphical modes both need the new abilities registered. New combat systems must be registered in both `systems.rs` (headless) and `states/mod.rs` (graphical) per the dual registration pattern.
- **Unchanged invariants**: Rogue opener and Warlock curse preference systems are not touched. Existing ability priorities for all classes remain unchanged for their default configurations.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Frost Armor proc could be too strong in Mage vs melee matchups | Tune magnitude (30% slow) and consider adding internal cooldown if testing shows it's oppressive |
| New AuraTypes may need handling in aura display/rendering | Rendering impact is cosmetic-only; can be added in a follow-up |
| Concentration Aura cast time reduction may make casters too strong | 15% is modest; comparable to Curse of Tongues' 50% cast time increase in reverse but weaker |
| AttackSpeedSlow interaction with auto-attack timer math | Verify in implementation that the interval multiplication produces correct behavior |

## Sources & References

- **Origin document:** [docs/brainstorms/2026-04-05-class-strategic-options-requirements.md](docs/brainstorms/2026-04-05-class-strategic-options-requirements.md)
- Related patterns: `RogueOpener` enum in match_config.rs, `WarlockCurse` enum, `try_battle_shout()` in warrior.rs, `try_devotion_aura()` in paladin.rs
- Wowhead Classic: Demoralizing Shout (Rank 5), Ice Armor, Mage Armor, Shadow Resistance Aura (Rank 3), Concentration Aura, Devotion Aura (Rank 7)
