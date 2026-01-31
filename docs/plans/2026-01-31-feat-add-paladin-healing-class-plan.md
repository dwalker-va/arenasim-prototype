---
title: "feat: Add Paladin Healing Class"
type: feat
date: 2026-01-31
---

# Add Paladin Healing Class

## Overview

Add Paladin as the second healing class in ArenaSim. Paladins are plate-wearing holy warriors who combine healing with melee utility, offering a distinct playstyle from the Priest's ranged healing focus.

## Problem Statement / Motivation

The game currently has only one healer (Priest). Adding Paladin provides:
- Team composition variety (melee healer vs ranged healer)
- Different tactical options (stun, cleanse, damage immunity via auras)
- More interesting 2v2 and 3v3 matchups

## Proposed Solution

Add a Paladin class with 5 abilities focused on healing and support:

| Ability | Type | Cast | Mana | Range | Effect |
|---------|------|------|------|-------|--------|
| Flash of Light | Heal | 1.5s | 20 | 40yd | Fast, efficient heal |
| Holy Light | Heal | 2.5s | 35 | 40yd | Slow, big heal |
| Holy Shock | Dual | Instant | 30 | 20/40yd | Damage enemy OR heal ally (single ability) |
| Hammer of Justice | CC | Instant | 25 | 10yd | 6s stun |
| Cleanse | Utility | Instant | 15 | 30yd | Dispel magic debuff |

Plus **Devotion Aura** - passive 10% damage reduction for entire team.

### Dual-Purpose Ability System (New Feature)

Holy Shock (and future abilities like Priest's Penance) can target either enemies or allies with different effects. This requires extending `AbilityConfig` to support:

```rust
// New fields in AbilityConfig
pub struct DualTargetConfig {
    /// Range when targeting enemies
    pub enemy_range: f32,
    /// Range when targeting allies
    pub ally_range: f32,
    /// Damage values (used when targeting enemy)
    pub damage_base_min: f32,
    pub damage_base_max: f32,
    pub damage_coefficient: f32,
    /// Healing values (used when targeting ally)
    pub healing_base_min: f32,
    pub healing_base_max: f32,
    pub healing_coefficient: f32,
}
```

The AI decides target type based on combat context, and `combat_core.rs` applies the appropriate effect based on whether the target is friendly or hostile.

## Technical Approach

### Class Stats

```
Paladin: (ResourceType::Mana, 175.0, 160.0, 8.0, 160.0, 8.0, 0.9, 20.0, 35.0, 5.0)
         (resource_type, max_health, max_mana, mana_regen, starting_mana, attack_dmg, attack_spd, AP, SP, move_spd)
```

Rationale:
- **175 HP** - Tankier than Priest (150) due to plate armor fantasy
- **160 Mana** - Slightly more than Priest (150), less than Warlock (180)
- **35 Spell Power** - Lower than Priest (40) to offset tankiness and utility
- **20 Attack Power** - Some melee capability for Hammer of Justice flavor

### Ability Definitions (abilities.ron)

Values scaled for our stat system (WoW Classic values are much higher due to level 60 stats):

```ron
// PALADIN ABILITIES

FlashOfLight: (
    name: "Flash of Light",
    cast_time: 1.5,
    range: 40.0,
    mana_cost: 20.0,
    cooldown: 0.0,
    healing_base_min: 12.0,
    healing_base_max: 16.0,
    healing_coefficient: 0.65,
    spell_school: Holy,
),

HolyLight: (
    name: "Holy Light",
    cast_time: 2.5,
    range: 40.0,
    mana_cost: 35.0,
    cooldown: 0.0,
    healing_base_min: 25.0,
    healing_base_max: 32.0,
    healing_coefficient: 0.90,
    spell_school: Holy,
),

// Dual-purpose ability - damages enemies OR heals allies
HolyShock: (
    name: "Holy Shock",
    cast_time: 0.0,
    mana_cost: 30.0,
    cooldown: 15.0,
    spell_school: Holy,
    // NEW: Dual-target configuration
    dual_target: Some((
        enemy_range: 20.0,
        ally_range: 40.0,
        // Damage (when targeting enemy)
        damage_base_min: 18.0,
        damage_base_max: 24.0,
        damage_coefficient: 0.45,
        // Healing (when targeting ally)
        healing_base_min: 20.0,
        healing_base_max: 26.0,
        healing_coefficient: 0.50,
    )),
),

HammerOfJustice: (
    name: "Hammer of Justice",
    cast_time: 0.0,
    range: 10.0,
    mana_cost: 25.0,
    cooldown: 60.0,
    applies_aura: Some((
        aura_type: Stun,
        duration: 6.0,
        magnitude: 1.0,
        break_on_damage: -1.0,
    )),
    spell_school: Holy,
),

PaladinCleanse: (
    name: "Cleanse",
    cast_time: 0.0,
    range: 30.0,
    mana_cost: 15.0,
    cooldown: 0.0,
    is_dispel: true,
    spell_school: Holy,
),
```

### Devotion Aura System

Implement as a permanent team buff applied at match start:

1. Add `PaladinAura` enum to `match_config.rs`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum PaladinAura {
    #[default]
    Devotion,  // 10% damage reduction
    // Future: Concentration, Retribution
}
```

2. Add `paladin_aura_prefs` to `MatchConfig` (like `warlock_curse_prefs`)

3. Apply aura during pre-combat phase to all team members

4. Implement as `AuraType::DamageReduction` with magnitude 0.10 (10%)

### AI Priority System

Based on Priest patterns, Paladin priorities:

```
Priority 1: Apply Devotion Aura (pre-combat, once)
Priority 2: Cleanse - Urgent (Polymorph, Fear on allies)
Priority 3: Emergency healing (ally < 40% HP) - Holy Shock (target=ally)
Priority 4: Hammer of Justice (interrupt enemy healer cast)
Priority 5: Standard healing (ally < 80% HP) - Flash of Light
Priority 6: Holy Light (ally damaged, no burst threat, time to cast)
Priority 7: Cleanse - Maintenance (roots, DoTs when team stable)
Priority 8: Holy Shock (target=enemy) - when team healthy (>80%)
```

**Key AI decisions:**
- Use Holy Light when target >50% HP and no enemies in melee (safe to cast)
- Use Flash of Light when target <50% HP or under pressure
- Save Hammer of Justice for: interrupt healer > peel for ally > setup kill
- Holy Shock targets ally when emergency heal needed, enemy when team healthy

**Holy Shock Target Selection Logic:**
```rust
fn should_holy_shock_heal(ctx: &CombatContext) -> bool {
    // Heal if any ally is below 50% HP
    ctx.allies().any(|ally| ally.health_pct() < 0.5)
}
```

## Acceptance Criteria

### Functional Requirements

- [ ] Paladin appears in class selection UI with correct color (pink #F58CBA)
- [ ] All 5 abilities function correctly in combat
- [ ] Cleanse removes magic debuffs (uses existing dispel system)
- [ ] Hammer of Justice stuns for 6 seconds
- [ ] Holy Shock works as dual-purpose (damage enemy OR heal ally, same cooldown)
- [ ] Devotion Aura applies 10% damage reduction to all team members
- [ ] AI prioritizes healing over damage appropriately
- [ ] Paladin works in headless simulation mode

### Files to Create

- [ ] `src/states/play_match/class_ai/paladin.rs` - AI decision logic

### Files to Modify

| File | Changes |
|------|---------|
| `src/states/match_config.rs` | Add `Paladin` to `CharacterClass`, add `PaladinAura` enum, add config fields |
| `src/states/play_match/components/mod.rs` | Add Paladin stats to `Combatant::new()`, add `paladin_aura` field |
| `src/states/play_match/abilities.rs` | Add 5 ability variants to `AbilityType` |
| `src/states/play_match/ability_config.rs` | Add abilities to validation, add `DualTargetConfig` support |
| `src/states/play_match/combat_core.rs` | Handle dual-target ability resolution |
| `assets/config/abilities.ron` | Add Paladin ability definitions |
| `src/states/play_match/class_ai/mod.rs` | Add `pub mod paladin`, update `get_class_ai()` |
| `src/states/play_match/combat_ai.rs` | Integrate `decide_paladin_action()` |
| `src/headless/config.rs` | Add "Paladin" to `parse_class()` |
| `src/states/configure_match_ui.rs` | Add Paladin to class selection |
| `src/states/view_combatant_ui.rs` | Add Paladin stats/abilities display |
| `src/states/play_match/rendering/mod.rs` | Add ability icon mappings |

### Assets to Add

- [ ] `assets/icons/classes/paladin.png` - Class icon
- [ ] `assets/icons/abilities/spell_holy_flashheal.jpg` - Flash of Light
- [ ] `assets/icons/abilities/spell_holy_holybolt.jpg` - Holy Light
- [ ] `assets/icons/abilities/spell_holy_searinglight.jpg` - Holy Shock
- [ ] `assets/icons/abilities/spell_holy_sealofmight.jpg` - Hammer of Justice
- [ ] `assets/icons/abilities/spell_holy_renew.jpg` - Cleanse
- [ ] `assets/icons/abilities/spell_holy_devotionaura.jpg` - Devotion Aura

## Implementation Phases

### Phase 1: Core Class Setup
1. Add `CharacterClass::Paladin` to enum
2. Add Paladin stats to `Combatant::new()`
3. Add ability variants to `AbilityType`
4. Add ability definitions to `abilities.ron`
5. Add headless config parsing

### Phase 2: Basic AI
1. Create `paladin.rs` with basic healing logic
2. Implement Flash of Light / Holy Light decisions
3. Test with headless simulation

### Phase 3: Dual-Target Ability System
1. Add `DualTargetConfig` struct to `ability_config.rs`
2. Update `combat_core.rs` to check target team and apply appropriate effect
3. Implement Holy Shock using the new system
4. Test dual-purpose targeting works correctly

### Phase 4: Utility Abilities
1. Implement Hammer of Justice (stun)
2. Implement Cleanse (copy Priest dispel pattern)

### Phase 5: Devotion Aura
1. Add `PaladinAura` enum and config
2. Add `AuraType::DamageReduction` if not exists
3. Apply aura to team at match start
4. Add UI for aura selection

### Phase 6: Polish
1. Download and add spell icons
2. Add to class selection UI
3. Add to view combatant stats UI
4. Final balance testing

## Testing Plan

### Headless Test Configs

```json
// 1v1 vs each class
{"team1": ["Paladin"], "team2": ["Warrior"]}
{"team1": ["Paladin"], "team2": ["Mage"]}
{"team1": ["Paladin"], "team2": ["Rogue"]}
{"team1": ["Paladin"], "team2": ["Priest"]}
{"team1": ["Paladin"], "team2": ["Warlock"]}

// Healer comparison
{"team1": ["Warrior", "Paladin"], "team2": ["Warrior", "Priest"]}

// 2v2 with DPS
{"team1": ["Rogue", "Paladin"], "team2": ["Mage", "Priest"]}
```

### Validation Checks
- [ ] Flash of Light heals for expected amount (12-16 + 35*0.65 = ~35 avg)
- [ ] Holy Light heals for expected amount (25-32 + 35*0.90 = ~60 avg)
- [ ] Hammer of Justice applies 6s stun
- [ ] Cleanse removes Polymorph/Fear from allies
- [ ] Holy Shock damages enemies (18-24 + 35*0.45 = ~37 avg)
- [ ] Holy Shock heals allies (20-26 + 35*0.50 = ~40 avg)
- [ ] Holy Shock uses correct range based on target type (20yd enemy, 40yd ally)
- [ ] Devotion Aura reduces incoming damage by 10%

## Open Questions (Resolved)

| Question | Resolution |
|----------|------------|
| Holy Shock cooldown? | 15 seconds (reduced from WoW's 30s for faster gameplay) |
| Devotion Aura effect? | 10% damage reduction (armor doesn't exist in our system) |
| Paladin preferred range? | 28.0 (ranged healer positioning, like Priest) |
| Multiple auras? | Start with Devotion only, add more later |

## References

### WoW Classic Spell Data (via Wowhead MCP)
- Flash of Light: 1.5s cast, 35 mana, 67-77 healing
- Holy Light: 2.5s cast, 35 mana, 42-51 healing (rank 1)
- Holy Shock: Instant, 225 mana, 204-220 damage/healing, 30s CD
- Hammer of Justice: Instant, 30 mana, 3s stun (rank 1), 1min CD
- Cleanse: Instant, removes poison/disease/magic
- Devotion Aura: Instant, +55 armor to party within 30yd

### Internal References
- Priest AI pattern: `src/states/play_match/class_ai/priest.rs`
- Dispel system: `src/states/play_match/auras.rs:333` (`process_dispels`)
- Class stats: `src/states/play_match/components/mod.rs:434`
- Ability config: `assets/config/abilities.ron`
