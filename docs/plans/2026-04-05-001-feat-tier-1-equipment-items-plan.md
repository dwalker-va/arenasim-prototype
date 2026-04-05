---
title: "feat: Add Tier 1 equipment items (ilvl 75)"
type: feat
status: completed
date: 2026-04-05
origin: docs/brainstorms/2026-04-05-tier-1-items-requirements.md
---

# feat: Add Tier 1 Equipment Items (ilvl 75)

## Overview

Add ~61 new Tier 1 equipment items at item level 69-75, creating the game's first power progression layer. Items are entirely new identities with stat profiles that lean into role specialization while keeping health/mana as PvP-essential baseline stats.

## Problem Frame

All current equipment sits at item level 54-60 (Tier 0) with no progression path. Adding Tier 1 creates a noticeable power jump (~25% budget increase) and introduces stat specialization that makes gearing decisions interesting rather than purely numerical. (see origin: docs/brainstorms/2026-04-05-tier-1-items-requirements.md)

## Requirements Trace

- R1. Tier 1 items at ilvl 75 ±3 (range 69-75, matching Tier 0's slot-based spread)
- R2. All new item names and identities
- R3. Health/mana remain present where they appear at Tier 0 (magnitude may reduce for specialization)
- R4. Lean into role specialization; a few hybrid pieces per set
- R5. All items pass budget validation (5% tolerance)
- R6. Mirror Tier 0 armor sets: plate DPS (9), plate holy (8), mail (8), leather (8), cloth (8)
- R7. Shared slots: cloaks (3), necklaces (3), rings (4), trinkets (2)
- R8. Weapons: one per weapon_type — 10 total
- R9. Add all ItemId enum variants
- R10. No default loadout changes; temporary test loadouts for validation

## Scope Boundaries

- No new armor type variants (shadow cloth, offensive leather, etc.)
- No default loadout changes
- No tier selection UI or match config options
- No item icons
- No Tier 2+ items

## Context & Research

### Relevant Code and Patterns

- `src/states/play_match/equipment.rs` — ItemId enum (line 131-220), ItemConfig struct (line 228-299), budget validation (line 379-423)
- `src/states/play_match/constants.rs` — stat weights, slot multipliers, tolerance
- `assets/config/items.ron` — 63 existing items, RON HashMap format
- `assets/config/loadouts.ron` — per-class default equipment

### Item Budget Formula

```
effective_budget = item_level × 0.75 × slot_multiplier
max_allowed = effective_budget × 1.05  (5% tolerance)
budget_usage = sum(stat_value × stat_weight)
```

**Stat weights:** max_health/max_mana = 1.0, attack_power/spell_power = 1.5, crit_chance = 300.0, movement_speed = 30.0, resistances = 0.4, mana_regen = 5.0

**Free stats (excluded):** armor, attack_damage_min/max, attack_speed

**Slot multipliers:** Head/Chest = 1.0, Legs = 0.875, Shoulders/Hands/Feet = 0.75, Waist = 0.625, Wrists = 0.5, accessories/weapons = 0.5625

## Key Technical Decisions

- **ilvl-per-slot mapping: Tier 0 + 15**: Each Tier 1 item gets ilvl = corresponding Tier 0 ilvl + 15. This produces a 69-75 range (wrists=69, waist=70, shoulders/feet=71, hands=71-73, head/chest/legs=73-75) that matches Tier 0's slot-based spread pattern exactly. Plate gets the top of the range (75 for major slots), other armor types use 73.

- **Add `item_tier` field to ItemConfig**: A `u32` field with `#[serde(default)]` (defaults to 0). Existing items need no changes. Enables future tier-based selection without touching every item later. (see origin: deferred question affecting R9)

- **Stat design direction per set**: Each set has a primary stat emphasis documented below. Within each set, most pieces follow the emphasis while 2-3 "hybrid" pieces mix stats for interesting choices. Health/mana always present but may be reduced on specialized pieces.

- **Item naming: distinct thematic sets**: Each Tier 1 armor set gets a new WoW-Classic-inspired name (e.g., plate DPS might use "Warlord's" theme, cloth might use "Netherwind" theme). Names should reference Wowhead for WoW Classic flavor. All new — no connection to Tier 0 names.

## Open Questions

### Resolved During Planning

- **ilvl spread**: Use Tier 0 ilvl + 15 for each slot. Produces 69-75 range within ±3 of baseline 72. Each slot's relative position mirrors Tier 0 exactly.
- **Budget headroom at ilvl 75**: At ilvl 75, Head budget = 56.25 (vs Tier 0's 45.0 at ilvl 60) = 25% increase. Enough for meaningful differentiation. At ilvl 69, Wrists budget = 25.875 (vs 20.25 at ilvl 54) = 27.8% increase. All slots exceed the 20% success criterion.
- **Tier field**: Yes, add `item_tier: u32` to ItemConfig. Trivial cost, avoids touching every item later.
- **Weapon roster**: Mirror Tier 0 exactly — one per weapon_type. No new weapon types at Tier 1.

### Deferred to Implementation

- **Exact stat values per item**: Implementation should compute budget per item, then allocate stats within budget using the design direction tables below. Use Wowhead MCP for name/flavor inspiration.
- **Hybrid piece selection**: Which 2-3 pieces per set get hybrid stats is an implementation choice based on what feels interesting.

## Tier 1 Stat Design Direction

> *This illustrates the intended approach and is directional guidance for review, not implementation specification. The implementing agent should treat it as context, not code to reproduce.*

### ilvl Mapping (Tier 0 → Tier 1)

| Slot | Plate ilvl | Other ilvl | Budget (Plate) | Budget (Other) |
|------|-----------|-----------|----------------|----------------|
| Head | 60→75 | 58→73 | 56.25 | 54.75 |
| Chest | 60→75 | 58→73 | 56.25 | 54.75 |
| Legs | 60→75 | 58→73 | 49.22 | 47.91 |
| Shoulders | 56→71 | 56→71 | 39.94 | 39.94 |
| Hands | 58→73 | 56→71 | 41.06 | 39.94 |
| Feet | 56→71 | 56→71 | 39.94 | 39.94 |
| Waist | 55→70 | 54→69 | 32.81 | 32.34 |
| Wrists | 54→69 | 54→69 | 25.88 | 25.88 |

### Set Design Directions

**Plate DPS (Warrior)** — Primary: attack_power + crit_chance. Hybrid pieces add movement_speed or resistances. Health always present.

**Plate Holy (Paladin)** — Primary: spell_power + max_mana. Hybrid pieces add mana_regen or resistances. Health always present.

**Mail (Hunter)** — Primary: attack_power + crit_chance. Balanced between offense and survivability. Hybrid pieces add movement_speed.

**Leather (Rogue)** — Primary: attack_power + crit_chance (aggressive). Hybrid pieces add movement_speed. Health present but can be lower on offense-heavy pieces.

**Cloth (Mage/Priest/Warlock)** — Primary: spell_power + max_mana. Hybrid pieces add mana_regen or crit_chance. Mana always present.

**Shared Slots** — Maintain role-split pattern from Tier 0: melee-focused variants (AP/crit), caster-focused variants (SP/mana), and defensive/utility variants (health/resistances).

**Weapons** — Scale damage/speed (free stats) alongside budgeted stats. Melee weapons lean AP/crit; caster weapons lean SP/mana. Staff remains two-handed with higher SP budget.

## Implementation Units

- [x] **Unit 1: Add item_tier field to ItemConfig**

**Goal:** Add a tier identifier to items so future systems can filter by tier.

**Requirements:** R9 (deferred question)

**Dependencies:** None

**Files:**
- Modify: `src/states/play_match/equipment.rs` — add `item_tier: u32` field to ItemConfig with `#[serde(default)]`

**Approach:**
- Add `#[serde(default)] pub item_tier: u32` to ItemConfig struct after `item_level`
- No changes needed to existing items.ron entries (defaults to 0)
- All new Tier 1 items will set `item_tier: 1`

**Patterns to follow:**
- Same `#[serde(default)]` pattern used by all other optional fields in ItemConfig

**Test scenarios:**
- Happy path: existing items continue to deserialize with item_tier = 0
- Happy path: new item with explicit item_tier = 1 deserializes correctly

**Verification:**
- `cargo test` passes — existing items still load and validate
- `cargo build` succeeds

- [x] **Unit 2: Add Tier 1 ItemId enum variants**

**Goal:** Register all ~61 new item identifiers in the ItemId enum.

**Requirements:** R9

**Dependencies:** Unit 1

**Files:**
- Modify: `src/states/play_match/equipment.rs` — add variants to ItemId enum (line 131-220)

**Approach:**
- Add new Tier 1 variants organized by the same section comment pattern as Tier 0
- Use comment headers like `// === Tier 1: Plate Armor — DPS (Warrior) ===`
- Variant names should be PascalCase, descriptive, and distinct from Tier 0 names
- Plan for ~61 variants: 41 armor + 10 shared + 10 weapons

**Patterns to follow:**
- Match existing ItemId organization: grouped by armor type, then shared slots, then weapons
- Same derives: `Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize`

**Test scenarios:**
- Happy path: `cargo build` compiles with all new variants
- Edge case: no name collisions with existing Tier 0 variants

**Verification:**
- `cargo build` succeeds
- Note: `cargo test` will fail until matching items.ron entries are added (expected)

- [x] **Unit 3: Add Tier 1 plate DPS armor (9 items)**

**Goal:** Create 9 plate DPS items with aggressive AP/crit stat profiles.

**Requirements:** R1, R2, R3, R4, R5, R6

**Dependencies:** Unit 2

**Files:**
- Modify: `assets/config/items.ron` — add 9 items under new Tier 1 plate DPS section

**Approach:**
- 9 items across all 8 armor slots + 1 caster/hybrid head variant (mirroring Tier 0's OnslaughtHeadGuard pattern)
- ilvl range: 69 (wrists) to 75 (head/chest/legs)
- Set `item_tier: 1` on all items
- Primary stats: max_health + attack_power + crit_chance
- 2-3 pieces should shift budget toward different stats (e.g., movement_speed on boots, resistance on a piece, hybrid SP on the caster head)
- Armor values should scale proportionally from Tier 0 (roughly +25%)
- Use Wowhead MCP for WoW Classic name inspiration

**Patterns to follow:**
- Tier 0 plate DPS section in items.ron for RON syntax and field ordering

**Test scenarios:**
- Happy path: all 9 items pass budget validation
- Happy path: each item's budget usage is at least 20% higher than its Tier 0 counterpart
- Edge case: caster/hybrid head variant has spell_power instead of attack_power (matching OnslaughtHeadGuard pattern)
- Happy path: at least 2 items have a stat not present on their Tier 0 counterpart

**Verification:**
- `cargo test` budget validation passes for all 9 items

- [x] **Unit 4: Add Tier 1 plate holy armor (8 items)**

**Goal:** Create 8 plate holy items with SP/mana stat profiles.

**Requirements:** R1, R2, R3, R4, R5, R6

**Dependencies:** Unit 2

**Files:**
- Modify: `assets/config/items.ron` — add 8 items under new Tier 1 plate holy section

**Approach:**
- 8 items across all 8 armor slots
- ilvl range: 69-75 (matching plate ilvl pattern)
- Set `item_tier: 1` on all items
- Primary stats: max_health + max_mana + spell_power
- 2-3 hybrid pieces add mana_regen or crit_chance for variety
- Armor values scale proportionally from Tier 0

**Patterns to follow:**
- Tier 0 Lawbringer set in items.ron

**Test scenarios:**
- Happy path: all 8 items pass budget validation
- Happy path: each item's budget usage ≥20% higher than Tier 0 Lawbringer counterpart
- Happy path: at least 2 items include a stat not on their Tier 0 counterpart (e.g., mana_regen, crit_chance)

**Verification:**
- `cargo test` budget validation passes for all 8 items

- [x] **Unit 5: Add Tier 1 mail armor (8 items)**

**Goal:** Create 8 mail items with balanced AP/crit profiles.

**Requirements:** R1, R2, R3, R4, R5, R6

**Dependencies:** Unit 2

**Files:**
- Modify: `assets/config/items.ron` — add 8 items under new Tier 1 mail section

**Approach:**
- 8 items across all 8 armor slots
- ilvl range: 69-73 (non-plate pattern)
- Set `item_tier: 1`
- Primary stats: max_health + attack_power + crit_chance (balanced)
- 2-3 hybrid pieces add movement_speed or resistances
- Armor values scale from Tier 0 Beaststalker set

**Patterns to follow:**
- Tier 0 Beaststalker set in items.ron

**Test scenarios:**
- Happy path: all 8 items pass budget validation
- Happy path: budget usage ≥20% higher than Beaststalker counterparts
- Happy path: at least 2 items include a new stat or significant stat shift

**Verification:**
- `cargo test` budget validation passes

- [x] **Unit 6: Add Tier 1 leather armor (8 items)**

**Goal:** Create 8 leather items with aggressive AP/crit profiles.

**Requirements:** R1, R2, R3, R4, R5, R6

**Dependencies:** Unit 2

**Files:**
- Modify: `assets/config/items.ron` — add 8 items under new Tier 1 leather section

**Approach:**
- 8 items across all 8 armor slots
- ilvl range: 69-73 (non-plate pattern)
- Set `item_tier: 1`
- Primary stats: max_health + attack_power + crit_chance (more aggressive than mail)
- Leather Tier 1 should feel like the most offense-oriented melee set
- 2-3 hybrid pieces add movement_speed (Rogues value mobility)
- Health can be lower than mail/plate on specialized pieces per R3

**Patterns to follow:**
- Tier 0 Nightstalker set in items.ron

**Test scenarios:**
- Happy path: all 8 items pass budget validation
- Happy path: budget usage ≥20% higher than Nightstalker counterparts
- Happy path: at least 2 items include a new stat or significant budget shift

**Verification:**
- `cargo test` budget validation passes

- [x] **Unit 7: Add Tier 1 cloth armor (8 items)**

**Goal:** Create 8 cloth items with SP/mana caster profiles.

**Requirements:** R1, R2, R3, R4, R5, R6

**Dependencies:** Unit 2

**Files:**
- Modify: `assets/config/items.ron` — add 8 items under new Tier 1 cloth section

**Approach:**
- 8 items across all 8 armor slots
- ilvl range: 69-73 (non-plate pattern)
- Set `item_tier: 1`
- Primary stats: max_health + max_mana + spell_power
- 2-3 hybrid pieces add crit_chance, mana_regen, or movement_speed
- Differentiate from plate holy by leaning harder into spell_power over raw mana

**Patterns to follow:**
- Tier 0 Magister's set in items.ron

**Test scenarios:**
- Happy path: all 8 items pass budget validation
- Happy path: budget usage ≥20% higher than Magister's counterparts
- Happy path: at least 2 items include a stat not on Tier 0 (e.g., crit_chance on a helm, mana_regen on bracers)

**Verification:**
- `cargo test` budget validation passes

- [x] **Unit 8: Add Tier 1 shared slot items (10 items)**

**Goal:** Create 3 cloaks, 3 necklaces, 2 rings (Ring1), and 2 trinkets (Trinket1).

**Requirements:** R1, R2, R3, R4, R5, R7

**Dependencies:** Unit 2

**Files:**
- Modify: `assets/config/items.ron` — add 10 items under new Tier 1 shared slot sections

**Approach:**
- Cloaks (3): melee-focused (AP), caster-focused (SP/mana), resistance/utility
- Necklaces (3): melee-focused (AP/crit), caster-focused (SP), resistance/utility
- Rings (2 Ring1): melee (AP/crit) and caster (SP/mana) — mirror Tier 0's Ring1 pattern. Note: not adding Ring2 items per requirements
- Trinkets (2 Trinket1): one offensive (AP/SP hybrid), one defensive (health/mana_regen) — mirror Tier 0 pattern
- All use slot multiplier 0.5625 for accessories
- ilvl 70 for all shared items (Tier 0 uses 55)
- Set `item_tier: 1`

**Patterns to follow:**
- Tier 0 shared slot items in items.ron for the role-split pattern

**Test scenarios:**
- Happy path: all 10 items pass budget validation
- Happy path: budget usage ≥20% higher than Tier 0 counterparts
- Happy path: melee/caster/utility split maintained across cloaks and necklaces

**Verification:**
- `cargo test` budget validation passes

- [x] **Unit 9: Add Tier 1 weapons (10 items)**

**Goal:** Create 10 weapons, one per weapon_type.

**Requirements:** R1, R2, R5, R8

**Dependencies:** Unit 2

**Files:**
- Modify: `assets/config/items.ron` — add 10 items under new Tier 1 weapon sections

**Approach:**
- Melee (5): Axe (2H, Warrior), Sword (1H, Warrior), Dagger (1H, Rogue), Mace (1H, Paladin), Staff (2H, casters)
- Ranged (4): Wand (casters), Bow (Hunter), Crossbow (Hunter)
  - Note: Tier 0 has 2 wands but only 1 per weapon_type needed — use Wand type once
- Off-hand (1): OffhandFrill (casters), Shield (Paladin/Warrior)
  - Need: 1 OffhandFrill + 1 Shield = 2 off-hand items
  - Wait — R8 says one per weapon_type, so that's OffhandFrill + Shield = 2, not 1. Total weapons = 5 melee + 3 ranged + 1 wand + 2 off-hand = 11? Let me recount.
  - Weapon types in R8: Axe, Sword, Dagger, Mace, Staff, Wand, Bow, Crossbow, OffhandFrill, Shield = 10
- ilvl: 73 for melee/ranged (Tier 0 uses 56-60), 70 for off-hand (Tier 0 uses 55-58)
- Set `item_tier: 1`
- Free stats (damage/speed) should scale up from Tier 0
- Budgeted stats follow role pattern: melee → AP/crit, caster → SP/mana

**Patterns to follow:**
- Tier 0 weapon entries in items.ron for is_weapon, two_handed, weapon_type usage

**Test scenarios:**
- Happy path: all 10 items pass budget validation
- Happy path: each weapon_type from Tier 0 has exactly one Tier 1 counterpart
- Happy path: two-handed weapons (Axe, Staff) set `two_handed: true`
- Happy path: all weapons set `is_weapon: true` (except off-hand items)

**Verification:**
- `cargo test` budget validation passes
- `cargo build` succeeds

- [x] **Unit 10: Validate and test with headless simulation**

**Goal:** Verify all items pass validation and run Tier 1 vs Tier 0 comparison.

**Requirements:** R5, R10, Success Criteria

**Dependencies:** Units 1-9

**Files:**
- Create: temporary test loadout configs for headless simulation

**Approach:**
- Run `cargo test` to verify all budget validations pass
- Create temporary headless configs with Tier 1 loadouts for each class
- Run headless Tier 1 vs Tier 0 matchups to verify power difference
- Verify at least 2 items per armor set have distinct stat profiles (new stats or 30%+ budget shift)
- Fix any budget violations or balance issues

**Test scenarios:**
- Happy path: `cargo test` passes (all ~124 items within budget)
- Happy path: Tier 1 loadout vs Tier 0 loadout shows Tier 1 winning majority of matchups
- Integration: headless simulation runs without crashes with new items loaded
- Edge case: items.ron parses correctly with ~124 entries (double current size)

**Verification:**
- `cargo test` green
- Headless simulation completes successfully
- Match logs show Tier 1 teams are noticeably stronger

## System-Wide Impact

- **Interaction graph:** Items are loaded at startup by `EquipmentPlugin::build()` via `load_item_definitions()`. Adding items only affects the `ItemDefinitions` resource. No callbacks, middleware, or observers are triggered.
- **Error propagation:** RON parse errors will panic at startup. Mismatched ItemId variants will cause deserialization failure. Both are caught by `cargo test`.
- **State lifecycle risks:** None — items are loaded once and immutable.
- **API surface parity:** No API changes. Headless and graphical modes both load from the same items.ron.
- **Unchanged invariants:** Tier 0 items, loadouts, budget validation formula, and class armor restrictions are not modified.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Budget math errors causing test failures | Compute budgets upfront using the formula; validate incrementally per set |
| RON syntax errors in large items.ron addition | Add items in batches per unit; run `cargo test` after each batch |
| Item names feeling generic or uninspired | Use Wowhead MCP for WoW Classic name inspiration; maintain thematic consistency per set |
| ~61 new enum variants making a large diff | Organize clearly with section comments; this is unavoidable content work |

## Sources & References

- **Origin document:** [docs/brainstorms/2026-04-05-tier-1-items-requirements.md](docs/brainstorms/2026-04-05-tier-1-items-requirements.md)
- Related code: `src/states/play_match/equipment.rs` (ItemId, ItemConfig, budget validation)
- Related code: `src/states/play_match/constants.rs` (stat weights, slot multipliers)
- Related code: `assets/config/items.ron` (existing items)
- Related plan: `docs/plans/2026-04-04-003-feat-item-level-budget-validation-plan.md` (budget system)
