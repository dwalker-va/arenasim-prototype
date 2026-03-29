---
title: "feat: Equipment system first slice — data model, loading, and stat application"
type: feat
status: completed
date: 2026-03-28
origin: docs/brainstorms/2026-03-22-equipment-system-brainstorm.md
---

# Equipment System First Slice

## Overview

Add the foundation of the equipment system: item data model, RON-based item definitions, default class loadouts, stat application at spawn time, headless config overrides, and combat log equipment summary. This slice establishes the data pipeline from item definitions through to combat-ready combatants with equipment-modified stats, without procs, on-use effects, or visual UI.

## Problem Frame

All characters of the same class currently have identical stats. Equipment adds a major customization axis — two Warriors can play very differently based on gear choices. This first slice establishes the structural foundation that all future equipment features (procs, on-use, UI, progression) will build on. (see origin: docs/brainstorms/2026-03-22-equipment-system-brainstorm.md)

## Requirements Trace

- R1. Item data model with 17 WoW Classic-faithful equipment slots
- R2. Named items defined in RON data files with stat bonuses and class/armor restrictions
- R3. Item level and stat budget metadata per item
- R4. Default equipment loadout per class, defined in data
- R5. Equipment stats applied to Combatant at spawn time (additive for armor/accessories, replacement for weapons)
- R6. Both graphical and headless spawn paths apply equipment identically
- R7. Headless JSON config supports per-character equipment overrides, backward-compatible
- R8. Class restriction validation at config parse time
- R9. Pre-match combat log lists equipped items per character
- R10. Pet stats correctly reflect owner's equipment-boosted stats

## Scope Boundaries

- No proc effects (chance-on-hit triggers) — future slice
- No on-use activated effects — future slice
- No equipment UI panel in graphical client — future slice
- No armor or resistance stats (no damage formula changes) — future slice
- No set bonuses — explicitly V1 non-goal per origin doc
- No 3D model changes — explicitly V1 non-goal per origin doc
- Item level is informational metadata only — no enforcement of stat budgets against ilvl
- Starter item pool of ~2-3 items per slot per armor type (~30-40 items total), not the full ~150-200 target

## Context & Research

### Relevant Code and Patterns

- **RON loading pattern**: `ability_config.rs` — serde structs, `HashMap<AbilityType, AbilityConfig>`, `AbilityDefinitions` Resource with validation, `AbilityConfigPlugin` that loads at startup. Equipment follows this exact pattern.
- **Combatant stats**: `components/combatant.rs` — `Combatant::new()` sets base stats from hardcoded class match (line 164). Equipment modifies these fields: `max_health`, `max_mana`, `mana_regen`, `attack_damage`, `attack_speed`, `attack_power`, `spell_power`, `crit_chance`, `base_movement_speed`.
- **Dual spawn paths**: Graphical in `play_match/mod.rs:spawn_combatant()` (line 525) and headless in `headless/runner.rs:headless_setup_match()` (line 116). Both independently construct Combatant and spawn entities.
- **MatchConfig bridge**: `match_config.rs` — per-team `Vec` fields for class preferences (rogue openers, warlock curses, hunter pets). Equipment overrides follow the same pattern.
- **Headless JSON**: `headless/config.rs` — `HeadlessMatchConfig` with `#[serde(default)]` for backward compatibility, `validate()` method, `to_match_config()` conversion.
- **Pet spawning**: Pets are spawned immediately after owner using `Combatant::new_pet()` which reads `owner.max_health` and `owner.spell_power`. Equipment must be applied before pet spawn.

### Institutional Learnings

- **Crit system pattern** (`docs/solutions/implementation-patterns/critical-hit-system-distributed-crit-rolls.md`): Equipment bonuses baked into Combatant fields automatically propagate through cast-time snapshots (`caster_spell_power`, `caster_crit_chance`) — no additional work needed.
- **Dual registration** (`docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md`): Equipment loading is a Resource (via Plugin), not a per-frame system, so dual registration risk is limited to the plugin addition in both paths.
- **Signature cascades**: Adding new Combatant fields causes ripple effects across test and production call sites. Equipment avoids this by modifying existing fields, not adding new ones.

## Key Technical Decisions

- **Single `items.ron` file for V1**: The brainstorm suggests splitting by slot type, but with ~30-40 starter items, a single file is simpler and follows the `abilities.ron` precedent. Split when the item count warrants it.
- **Shared `apply_equipment()` method on Combatant**: Extracts equipment stat application into `Combatant::apply_equipment()` callable from both spawn paths, eliminating the divergence risk between graphical and headless modes. This is the single most important architectural choice in this slice.
- **Weapon stat resolution by class melee/ranged**: Melee classes (Warrior, Rogue, Paladin) use Main Hand weapon stats. Ranged classes (Mage, Priest, Warlock, Hunter) use Ranged slot weapon stats. This maps to the existing `CharacterClass::is_melee()` method and avoids complex multi-weapon resolution.
- **Equipment overrides in headless JSON as slot→item maps**: `"team1_equipment": [{"MainHand": "ArcaniteReaper", "Head": "LionheartHelm"}]` — one map per team member, referencing items by string name (parsed to `ItemId`). Follows the established per-slot preference pattern.
- **Class restriction validation at parse time**: Invalid equipment (wrong armor type, wrong weapon type for class) fails loudly during `HeadlessMatchConfig::validate()`, matching the existing pattern for invalid class names.
- **Equipment applied between `Combatant::new()` and `current_health/mana` sync**: Create combatant with base stats → apply equipment (modify `max_health`, `max_mana`, etc.) → set `current_health = max_health` and `current_mana = max_mana` → spawn pet from boosted owner.

## Open Questions

### Resolved During Planning

- **Multiple weapon slots**: Resolved by class-based resolution — melee uses Main Hand, ranged uses Ranged slot. Off Hand weapons contribute only their non-weapon stats (attack_power, crit, etc.) but don't replace attack_damage/attack_speed.
- **Empty slots**: Represented as `Option<ItemId>` in loadout maps. Empty slots contribute no stats. Some slots may be legitimately empty in default loadouts.
- **RON file organization**: Single `items.ron` for V1, split later when item count grows.
- **Item level enforcement**: Informational only for V1. No validation that item stats match ilvl budget.

### Deferred to Implementation

- **Exact starter item names and stats**: Item content authoring happens during implementation, using Wowhead MCP for reference values. The plan defines the data structure, not the item database.
- **Whether `Combatant::new()` internals should be refactored**: The current constructor has a large match statement for base stats. Equipment application is a separate step after construction, so refactoring the constructor is not required but may be convenient.

## High-Level Technical Design

> *This illustrates the intended approach and is directional guidance for review, not implementation specification. The implementing agent should treat it as context, not code to reproduce.*

```
Data Flow:
  items.ron ──parse──> ItemDefinitions (Resource)
  loadouts.ron ──parse──> DefaultLoadouts (Resource)

Spawn-Time Flow:
  MatchConfig.equipment[team][slot] ──resolve──> Loadout (default + overrides)
    │
    ▼
  Combatant::new(class)          ← base stats from class
  combatant.apply_equipment(items) ← modify stats from resolved loadout
  combatant.current_health = max  ← sync after equipment
  combatant.current_mana = max_mana
    │
    ▼
  Spawn entity
  If has_pet: Combatant::new_pet(owner) ← reads equipment-boosted owner stats

Headless JSON Override:
  "team1_equipment": [
    {"MainHand": "ArcaniteReaper", "Head": "LionheartHelm"},  // slot 0
    {}  // slot 1: all defaults
  ]
```

## Implementation Units

- [x] **Unit 1: Item data model and RON definitions**

**Goal:** Define the item data structures (enums, config structs) and create the starter item set in RON format.

**Requirements:** R1, R2, R3

**Dependencies:** None

**Files:**
- Create: `src/states/play_match/equipment.rs` — `ItemSlot`, `ItemId`, `ArmorType`, `WeaponType`, `ItemConfig`, `ItemsConfig`, `ItemDefinitions` Resource, `DefaultLoadouts` Resource, `EquipmentPlugin`
- Create: `assets/config/items.ron` — starter item definitions (~30-40 items)
- Create: `assets/config/loadouts.ron` — default loadout per class
- Modify: `src/states/play_match/mod.rs` — add `pub mod equipment;` and re-export
- Test: headless match simulation with equipment-equipped combatants (integration test via cargo run)

**Approach:**
- `ItemSlot` enum with 17 variants matching WoW Classic slots (Head, Neck, Shoulders, Back, Chest, Wrists, Hands, Waist, Legs, Feet, Ring1, Ring2, Trinket1, Trinket2, MainHand, OffHand, Ranged)
- `ItemId` enum with named variants for each item (e.g., `ArcaniteReaper`, `LionheartHelm`)
- `ArmorType` enum: Cloth, Leather, Mail, Plate, None (for accessories/weapons)
- `WeaponType` enum: Sword, Mace, Axe, Dagger, Staff, Polearm, Fist, Bow, Gun, Crossbow, Wand, Thrown, Shield, OffhandFrill, None
- `ItemConfig` struct with: name, item_level, slot, armor_type, weapon_type, allowed_classes (Option<Vec<CharacterClass>>), stat bonuses (all f32 with serde default 0.0), is_weapon flag
- Follow `ability_config.rs` pattern: `ItemsConfig` wraps `HashMap<ItemId, ItemConfig>`, `ItemDefinitions` Resource with `get()`, `validate()`, `Default` impl that loads from file
- `DefaultLoadouts` as `HashMap<CharacterClass, HashMap<ItemSlot, ItemId>>` loaded from `loadouts.ron`
- `EquipmentPlugin` loads both resources at startup, validates all items exist and loadout references resolve
- Starter items: ~2-3 per slot per relevant armor type. Use Wowhead MCP for reference stats. Focus on one complete loadout per class.

**Patterns to follow:**
- `ability_config.rs` for RON loading, Resource wrapper, validation, Plugin pattern
- `match_config.rs` for enum definitions with `name()`, `description()` methods

**Test scenarios:**
- Happy path: All 7 classes have valid default loadouts that reference existing items
- Happy path: Item validation passes when all expected items are defined
- Edge case: Empty optional fields (allowed_classes = None means all classes can equip)
- Error path: Missing item in loadout references → startup panic with descriptive message
- Error path: Duplicate ItemId in RON → parse error

**Verification:**
- `cargo build --release` succeeds with new module
- Equipment plugin loads without panic in both graphical and headless modes

---

- [x] **Unit 2: Equipment stat application on Combatant**

**Goal:** Add `Combatant::apply_equipment()` that modifies combatant stats from a resolved equipment loadout. Ensure health/mana sync after application.

**Requirements:** R5, R6, R10

**Dependencies:** Unit 1

**Files:**
- Modify: `src/states/play_match/components/combatant.rs` — add `apply_equipment()` method
- Modify: `src/states/play_match/equipment.rs` — add `resolve_loadout()` helper and class restriction checking functions

**Approach:**
- `apply_equipment(&mut self, loadout: &HashMap<ItemSlot, ItemId>, items: &ItemDefinitions)` method on Combatant
- Iterate all equipped items. For non-weapon items: add stats to combatant fields. For weapon items in the primary weapon slot (Main Hand for melee, Ranged for ranged per `class.is_melee()`): replace `attack_damage` and `attack_speed`, add other stats
- After all items applied: `self.current_health = self.max_health` and `self.current_mana = self.max_mana`
- `resolve_loadout(class, defaults, overrides) -> HashMap<ItemSlot, ItemId>` merges default loadout with optional per-slot overrides
- `validate_class_restrictions(class, loadout, items) -> Result<(), String>` checks armor type and weapon type eligibility

**Patterns to follow:**
- `Combatant::new_with_curse_prefs()` pattern of creating base combatant then mutating it
- `CharacterClass::is_melee()` for weapon resolution

**Test scenarios:**
- Happy path: Warrior with full plate loadout has higher stats than base Warrior
- Happy path: Weapon in Main Hand replaces attack_damage and attack_speed for melee class
- Happy path: Weapon in Ranged slot replaces attack_damage and attack_speed for ranged class (Mage wand)
- Happy path: current_health equals max_health after equipment application (not base max_health)
- Edge case: Empty loadout (no items) → combatant has base class stats unchanged
- Edge case: Slot with no item (Option::None) → that slot contributes nothing
- Edge case: Off Hand weapon adds non-weapon stats but does not replace attack_damage/attack_speed
- Integration: Pet spawned after equipment application inherits boosted owner max_health and spell_power

**Verification:**
- Headless match with equipped combatants shows higher stats in combat log than unequipped base stats
- Pet health scales with owner's equipment-boosted max_health

---

- [x] **Unit 3: MatchConfig and headless JSON equipment integration**

**Goal:** Add equipment fields to MatchConfig and HeadlessMatchConfig so equipment can be specified per-character in headless JSON configs.

**Requirements:** R7, R8

**Dependencies:** Unit 1

**Files:**
- Modify: `src/states/match_config.rs` — add `team1_equipment` and `team2_equipment` fields to `MatchConfig`
- Modify: `src/headless/config.rs` — add equipment fields to `HeadlessMatchConfig`, parse/validate in `to_match_config()`

**Approach:**
- `MatchConfig` gets `team1_equipment: Vec<HashMap<ItemSlot, ItemId>>` and `team2_equipment: Vec<HashMap<ItemSlot, ItemId>>` — one map per team slot, containing only overrides (not the full loadout)
- `HeadlessMatchConfig` gets `team1_equipment: Vec<HashMap<String, String>>` with `#[serde(default)]` — string-keyed for JSON ergonomics
- Add `parse_item_slot()` and `parse_item_id()` methods to HeadlessMatchConfig
- In `validate()`: check that referenced items exist in ItemDefinitions and pass class restrictions
- In `to_match_config()`: parse string maps into typed `HashMap<ItemSlot, ItemId>` maps, resize to team size with empty defaults
- `MatchConfig::Default` initializes equipment vecs as empty (all defaults)
- `set_team1_size()` / `set_team2_size()` resize equipment vecs

**Patterns to follow:**
- `team1_rogue_openers` / `team1_warlock_curse_prefs` pattern for per-slot config fields
- `HeadlessMatchConfig::validate()` for parse-time validation
- `#[serde(default)]` for backward compatibility

**Test scenarios:**
- Happy path: JSON with no equipment fields → parses successfully, empty overrides (backward compatible)
- Happy path: JSON with equipment overrides → correct items in MatchConfig after conversion
- Happy path: Partial overrides (only MainHand specified) → only that slot overridden, rest use defaults
- Error path: Unknown item name in JSON → descriptive validation error
- Error path: Unknown slot name in JSON → descriptive validation error
- Error path: Class restriction violation (Plate on Mage) → descriptive validation error at parse time
- Edge case: Equipment array shorter than team size → missing entries get empty overrides
- Edge case: Equipment array longer than team size → extra entries ignored

**Verification:**
- Existing headless JSON configs (without equipment fields) continue to work unchanged
- New configs with equipment overrides produce correct MatchConfig

---

- [x] **Unit 4: Wire equipment into both spawn paths**

**Goal:** Integrate equipment loading and stat application into both graphical and headless spawn paths so combatants spawn with equipment-modified stats.

**Requirements:** R5, R6, R10

**Dependencies:** Units 1, 2, 3

**Files:**
- Modify: `src/states/play_match/mod.rs` — update `setup_play_match()` and `spawn_combatant()` to resolve loadout and apply equipment
- Modify: `src/headless/runner.rs` — update `headless_setup_match()` to resolve loadout and apply equipment
- Modify: `src/states/play_match/mod.rs` — add `EquipmentPlugin` to graphical plugin chain
- Modify: `src/headless/runner.rs` — add `EquipmentPlugin` to headless plugin chain

**Approach:**
- Both spawn paths: after `Combatant::new_with_curse_prefs()`, call `resolve_loadout()` with class defaults + config overrides, then call `combatant.apply_equipment()`
- Equipment must be applied BEFORE pet spawning (both paths already spawn pets after owner — just insert equipment application between combatant creation and pet spawn)
- `EquipmentPlugin` added alongside `AbilityConfigPlugin` in both paths
- `spawn_combatant()` signature gains `equipment_overrides: &HashMap<ItemSlot, ItemId>` and `item_defs: &ItemDefinitions` and `default_loadouts: &DefaultLoadouts` parameters

**Patterns to follow:**
- Existing `spawn_combatant()` parameter passing pattern
- `AbilityConfigPlugin` registration in both graphical `mod.rs` and headless `runner.rs`

**Test scenarios:**
- Happy path: Headless match with default equipment → combatants have equipment-boosted stats in log
- Happy path: Headless match with equipment overrides → overridden slots reflect specified items
- Happy path: Graphical mode loads equipment without panic (manual verification)
- Integration: Both modes produce identical combatant stats for the same config
- Integration: Pet stats reflect owner's equipment-boosted values

**Verification:**
- `cargo run --release -- --headless /tmp/test.json` succeeds and shows equipment-modified stats
- `cargo run --release` launches graphical client without equipment-related panics

---

- [x] **Unit 5: Combat log equipment summary**

**Goal:** Add pre-match equipment listing to combat log so match reports show each character's equipped items.

**Requirements:** R9

**Dependencies:** Unit 4

**Files:**
- Modify: `src/combat/log.rs` — add equipment logging helper or event type
- Modify: `src/states/play_match/mod.rs` — log equipment after spawning in graphical path
- Modify: `src/headless/runner.rs` — log equipment after spawning in headless path

**Approach:**
- After all combatants are spawned (with equipment applied), log a summary line per combatant listing their equipped items by name
- Format: `[EQUIPMENT] Team 1 Warrior: MainHand=Arcanite Reaper, Head=Lionheart Helm, ...` (listing only filled slots)
- Use existing `CombatLogEventType::MatchEvent` — no new event type needed for V1
- Log after spawn, before combat begins (during countdown phase setup)

**Patterns to follow:**
- Existing `combat_log.log()` calls in `headless_setup_match()` and `setup_play_match()`
- `combatant_id()` format for identifying characters

**Test scenarios:**
- Happy path: Match log contains equipment summary for each combatant with item names
- Happy path: Empty slots are omitted from the log line (not listed as "None")
- Edge case: Character with no equipment (all empty) → log line shows "No equipment"

**Verification:**
- `cat match_logs/<latest>.txt` shows equipment listing in pre-match section
- Equipment stats visible in match report stat summaries (higher HP/damage than base)

## System-Wide Impact

- **Interaction graph:** Equipment stat application happens at spawn time only — no new per-frame systems. The `EquipmentPlugin` loads resources at startup. No callbacks, middleware, or observers affected.
- **Error propagation:** Invalid equipment config fails at startup (RON loading) or config parse time (headless JSON). No runtime failures from equipment.
- **State lifecycle risks:** Equipment is immutable after spawn — no mid-match equipment changes, no cache invalidation concerns. The stat modifications are one-shot mutations during entity construction.
- **API surface parity:** Both graphical and headless spawn paths must apply equipment identically. The shared `Combatant::apply_equipment()` method ensures this.
- **Integration coverage:** Equipment-modified stats flow through all existing combat systems automatically via the Combatant fields. Cast-time snapshots (`caster_spell_power`, etc.) will reflect equipment bonuses without changes.
- **Unchanged invariants:** All existing ability definitions, aura mechanics, AI decision logic, and combat formulas remain unchanged. Equipment only modifies the input stats that feed into these systems.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Graphical/headless stat divergence | Shared `apply_equipment()` method on Combatant, called from both paths |
| Pet stats not reflecting owner equipment | Spawn ordering enforced: equipment applied before pet spawn in both paths |
| Existing headless configs break | All new fields use `#[serde(default)]`, empty defaults mean no equipment overrides |
| Balance impact of equipment stats | Starter item pool is small and conservative; base stats remain as-is for no-equipment play |
| RON file authoring errors | Startup validation catches missing items, invalid loadout references, and class restriction violations |

## Sources & References

- **Origin document:** [docs/brainstorms/2026-03-22-equipment-system-brainstorm.md](docs/brainstorms/2026-03-22-equipment-system-brainstorm.md)
- Related code: `ability_config.rs` (RON loading pattern), `combatant.rs` (stat fields), `match_config.rs` (config bridge)
- Design reference: `design-docs/stat-scaling-system.md` (damage/healing formulas that equipment stats feed into)
