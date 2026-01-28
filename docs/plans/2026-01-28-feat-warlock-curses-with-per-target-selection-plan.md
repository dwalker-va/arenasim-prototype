---
title: "feat: Add Warlock Curses with Per-Target Selection"
type: feat
date: 2026-01-28
---

# feat: Add Warlock Curses with Per-Target Selection

## Overview

Add three Warlock curses (Curse of Agony, Curse of Weakness, Curse of Tongues) with a per-target configuration system in the View Combatant screen. This allows players to strategically assign different curses to different enemies - e.g., Tongues on the healer, Weakness on the melee, Agony on the kill target.

## Problem Statement / Motivation

Currently Warlocks lack their signature curse abilities which are core to WoW PvP gameplay. Curses provide strategic depth:
- **Curse of Agony**: Additional DoT pressure
- **Curse of Weakness**: Reduces melee damage output
- **Curse of Tongues**: Cripples caster effectiveness (+50% cast time)

The per-target configuration mirrors the existing Rogue opener selection pattern but extends it to support choosing abilities per enemy, enabling meaningful pre-match strategic decisions.

## Proposed Solution

1. Add `WarlockCurse` enum and per-target curse preferences to MatchConfig
2. Add three curse abilities to `abilities.ron` with accurate WoW Classic values
3. Add two new AuraTypes: `DamageReduction` and `CastTimeIncrease`
4. Create UI panel in View Combatant screen (Warlock only) for curse configuration
5. Update Warlock AI to spread curses to all enemies based on preferences
6. Add headless mode JSON support for curse configuration

## Technical Approach

### Data Model

**New enum in `match_config.rs`:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum WarlockCurse {
    #[default]
    Agony,      // DoT: 84 damage over 24s
    Weakness,   // -3 damage dealt for 2 min
    Tongues,    // +50% cast time for 30s
}
```

**MatchConfig additions:**
```rust
/// Per-warlock curse preferences: [warlock_slot][enemy_target_index] -> curse
pub team1_warlock_curse_prefs: Vec<Vec<WarlockCurse>>,
pub team2_warlock_curse_prefs: Vec<Vec<WarlockCurse>>,
```

### New AuraTypes

Add to `AuraType` enum in `components/mod.rs`:
- `DamageReduction` - magnitude = flat damage reduction (3 for Curse of Weakness)
- `CastTimeIncrease` - magnitude = percentage increase (0.5 for 50% slower)

### Curse Abilities (WoW Classic Values)

| Curse | Mana | Range | Duration | Effect | Icon |
|-------|------|-------|----------|--------|------|
| Agony | 25 | 30yd | 24s | 84 Shadow DoT (14 damage per tick, 6 ticks) | spell_shadow_curseofsargeras |
| Weakness | 20 | 30yd | 2 min | -3 damage dealt | spell_shadow_curseofmannoroth |
| Tongues | 80 | 30yd | 30s | +50% cast time | spell_shadow_curseoftounges |

### WoW Mechanic: One Curse Per Warlock Per Target

Track curses by checking aura `ability_name` for "Curse of" prefix from the same caster. When applying a new curse, remove any existing curse from that Warlock on that target.

### UI Layout (View Combatant Screen)

```
┌─────────────────────────────────────────────────────────────────┐
│ CURSE PREFERENCES                                                │
│                                                                  │
│ Enemy 1 (Warrior):  [Agony] [Weakness*] [Tongues]               │
│ Enemy 2 (Priest):   [Agony] [Weakness]  [Tongues*]              │
│ Enemy 3 (Mage):     [Agony*] [Weakness] [Tongues]               │
│                                                                  │
│ * = selected (gold border like Rogue opener)                     │
└─────────────────────────────────────────────────────────────────┘
```

- Only shown for Warlock class
- Shows enemy class names if configured, else "Enemy 1/2/3"
- Clickable curse icons with tooltips
- Gold border on selected curse (matches Rogue opener pattern)

### Warlock AI Changes

**Curse spreading behavior:**
1. In each decision cycle, check each enemy for existing curse from this Warlock
2. For uncursed enemies, apply the configured curse (or Agony if no preference)
3. Priority: Apply curses early (after initial DoTs, before Fear)

**Updated AI priority:**
```
1. Corruption (if target missing)
2. Immolate (if target missing)
3. Apply curse to any uncursed enemy (spread behavior)
4. Fear (on CC target if not CC'd)
5. Drain Life (if HP < 80% and target has DoTs)
6. Shadow Bolt (filler)
```

### Headless Mode JSON

```json
{
  "team1": ["Warlock", "Priest"],
  "team2": ["Warrior", "Mage", "Rogue"],
  "team1_warlock_curse_prefs": [
    ["Weakness", "Tongues", "Agony"],
    null
  ]
}
```

- Array indexed by team slot
- `null` for non-Warlock slots
- Inner array indexed by enemy slot (0, 1, 2)
- Defaults to Agony if not specified or shorter than enemy count

## Acceptance Criteria

### Functional Requirements
- [ ] Three curse abilities added: Curse of Agony, Curse of Weakness, Curse of Tongues
- [ ] Each curse has correct WoW Classic values (mana cost, duration, effect)
- [ ] Curse of Weakness reduces target's damage dealt by 3
- [ ] Curse of Tongues increases target's cast time by 50%
- [ ] Curse of Agony deals 84 Shadow damage over 24 seconds
- [ ] Only one curse per Warlock per target (WoW mechanic)
- [ ] Applying a new curse replaces the old curse from same Warlock
- [ ] Curses are dispellable by Priest's Dispel Magic

### UI Requirements
- [ ] "CURSE PREFERENCES" panel visible only for Warlocks
- [ ] Shows row per enemy with 3 curse icon options
- [ ] Enemy class shown if configured, else "Enemy 1/2/3"
- [ ] Selected curse has gold border (matches Rogue opener style)
- [ ] Clicking curse icon updates MatchConfig
- [ ] Tooltips show curse name and effect on hover

### AI Requirements
- [ ] Warlock AI spreads curses to all enemies
- [ ] Applies configured curse per target (default: Agony)
- [ ] Curse application happens after DoTs, before Fear
- [ ] AI doesn't waste curse if target already has this Warlock's curse

### Headless Mode Requirements
- [ ] `team1_warlock_curse_prefs` and `team2_warlock_curse_prefs` accepted in JSON
- [ ] Validates curse names and target indices
- [ ] Defaults to Agony for missing preferences

### Testing
- [ ] Headless simulation with curse configuration works
- [ ] Match logs show curse applications
- [ ] Curse effects (damage reduction, cast time slow) function correctly

## Implementation Phases

### Phase 1: Data Model & Abilities
Files: `match_config.rs`, `abilities.rs`, `ability_config.rs`, `abilities.ron`, `components/mod.rs`

1. Add `WarlockCurse` enum to `match_config.rs`
2. Add curse preference fields to `MatchConfig`
3. Add `DamageReduction` and `CastTimeIncrease` to `AuraType` enum
4. Add `CurseOfAgony`, `CurseOfWeakness`, `CurseOfTongues` to `AbilityType`
5. Add curse definitions to `abilities.ron`
6. Add to validation list in `ability_config.rs`

### Phase 2: Combat Mechanics
Files: `combat_core.rs`, `auras.rs`, `warlock.rs`

1. Implement `DamageReduction` aura effect (reduce outgoing damage)
2. Implement `CastTimeIncrease` aura effect (multiply cast times)
3. Add curse replacement logic (one curse per Warlock per target)
4. Update Warlock AI with curse spreading behavior
5. Ensure curses are magic-dispellable

### Phase 3: UI
Files: `view_combatant_ui.rs`, `rendering/mod.rs`

1. Download curse icons from Wowhead
2. Add icon mappings in `get_ability_icon_path()`
3. Create `render_curse_preferences_panel()` function
4. Integrate into View Combatant screen (Warlock only)
5. Handle click events to update MatchConfig

### Phase 4: Headless Mode & Testing
Files: `config.rs`, `runner.rs`

1. Add curse preference parsing to headless config
2. Add validation for curse names and indices
3. Run headless tests with various curse configurations
4. Verify match logs capture curse applications

## Files to Modify

| File | Changes |
|------|---------|
| `src/states/match_config.rs` | Add `WarlockCurse` enum, preference fields, `name()`/`description()` methods |
| `src/states/play_match/abilities.rs` | Add 3 curse variants to `AbilityType` |
| `src/states/play_match/ability_config.rs` | Add curses to validation list |
| `assets/config/abilities.ron` | Add 3 curse definitions |
| `src/states/play_match/components/mod.rs` | Add `DamageReduction`, `CastTimeIncrease` to `AuraType` |
| `src/states/play_match/combat_core.rs` | Implement damage reduction effect, cast time modification |
| `src/states/play_match/auras.rs` | Handle curse aura tracking |
| `src/states/play_match/class_ai/warlock.rs` | Add curse spreading AI logic |
| `src/states/view_combatant_ui.rs` | Add curse preferences panel for Warlocks |
| `src/states/play_match/mod.rs` | Pass curse preferences during combatant spawn |
| `src/headless/config.rs` | Parse curse preferences from JSON |
| `src/states/play_match/rendering/mod.rs` | Add curse icon mappings |
| `assets/icons/abilities/` | Add 3 curse icon files |

## Dependencies & Risks

**Dependencies:**
- Existing aura system (well-established)
- Rogue opener pattern (proven template)
- View Combatant UI infrastructure

**Risks:**
- Cast time modification needs testing with existing spells
- Damage reduction interacts with attack power scaling
- UI layout may need adjustment for smaller screens

## References

### WoW Classic Spell Data (from Wowhead MCP)
- Curse of Agony (ID: 980): 25 mana, 30yd, instant, 84 Shadow over 24s
- Curse of Weakness (ID: 702): 20 mana, 30yd, instant, -3 damage for 2 min
- Curse of Tongues (ID: 1714): 80 mana, 30yd, instant, +50% cast time for 30s

### Internal References
- Rogue opener pattern: `match_config.rs:9-35`, `view_combatant_ui.rs:899-1025`
- Warlock AI: `class_ai/warlock.rs`
- Ability config: `ability_config.rs`, `abilities.ron`
- Aura system: `components/mod.rs:324-357`, `auras.rs`

### Icons
- Curse of Agony: `spell_shadow_curseofsargeras.jpg`
- Curse of Weakness: `spell_shadow_curseofmannoroth.jpg`
- Curse of Tongues: `spell_shadow_curseoftounges.jpg`
