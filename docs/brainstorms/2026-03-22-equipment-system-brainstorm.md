# Equipment System Brainstorm

**Date:** 2026-03-22
**Status:** Ready for planning

## What We're Building

A WoW Classic-faithful equipment system where each character has 17 gear slots. Equipment is defined as curated, named items in data files. Most items provide passive stat bonuses applied at match start, but items can also have proc effects (chance-on-hit triggering auras or unique mechanics) and on-use activated effects (treated as additional abilities).

The equipment system serves dual purpose: tactical team-building (choosing the right gear for a matchup) and future meta-progression (upgrading equipment over time).

## Why This Matters

Currently all characters of the same class have identical stats. Equipment adds a major axis of customization — two Warriors can play very differently based on gear choices. This also lays the groundwork for the planned meta-progression system around equipment upgrades and talents.

## Why This Approach

**Stats applied at spawn, not as permanent auras.** Equipment stats are static for the match duration, so modeling them as permanent auras would add per-frame overhead and 80-240 extra aura entities in a 3v3 for zero behavioral benefit. Instead, equipment stats are summed into `Combatant` fields during match setup. Procs and on-use effects are separate concerns that integrate with the existing aura and ability systems.

Other key tradeoffs: curated named items over procedural generation (keeps item pool bounded and hand-balanced), item level stat budgets over hand-tuned values (systematic balance that scales with progression), and no visual model changes for V1 (equipment UI only — avoids 3D asset pipeline complexity).

## Key Decisions

### Equipment Slots (17, WoW Classic-faithful)
Head, Neck, Shoulders, Back, Chest, Wrists, Hands, Waist, Legs, Feet, Ring x2, Trinket x2, Main Hand, Off Hand, Ranged. Off Hand covers shields, off-hand frills, and dual-wield weapons. Ranged covers bows, guns, wands, and thrown weapons.

### Named Items (WoW-style, not procedurally generated)
Each item is a specific named piece with fixed stats, defined in RON data files. Finite, curated item pool. No random affixes or quality tiers for now. Items are organized into multiple RON files split by slot type (e.g., `weapons.ron`, `armor_head.ron`, `trinkets.ron`).

### Item Level and Stat Budget
Each item has an item level that determines its total stat budget. Stats are distributed within that budget. This provides systematic balance across slots and scales naturally with future progression systems.

### Class Restrictions (WoW-style)
Items have armor type restrictions (Cloth, Leather, Mail, Plate) and weapon type restrictions. Some items are class-locked. Classes can only equip their allowed armor types and weapon types, matching WoW Classic rules:
- **Cloth**: All classes
- **Leather**: All except Mage, Priest, Warlock
- **Mail**: Warrior, Paladin, Hunter
- **Plate**: Warrior, Paladin
- **Weapons**: Class-specific per WoW Classic rules (e.g., Mages can't use swords, Paladins can't use fist weapons)

### Passive Stats
Armor and accessory items provide flat bonuses to `Combatant` stats: `max_health`, `max_mana`, `attack_power`, `spell_power`, `crit_chance`, `base_movement_speed`, `mana_regen`. May also introduce new stats like `armor` and `resistances` as needed.

### Weapon Stats (Replace, Not Add)
Weapons define base `attack_damage` (min/max) and `attack_speed` values that replace the class defaults, not add to them. This matches WoW where your weapon *is* your auto-attack damage source. Class base stats for `attack_damage` and `attack_speed` become fallback values used only when no weapon is equipped.

### Weapon Types Are Cosmetic
Weapon type (sword, mace, dagger, staff, etc.) does not affect ability eligibility or provide innate procs. Any weapon equipped in a valid slot works for all class abilities. Weapon type is for naming/flavor only.

### Proc Effects
Most procs apply temporary auras via the existing aura system (e.g., "chance on hit to gain +20 AP for 10s"). The system also supports unique mechanic procs that require custom code, but these are rare exceptions. Proc triggers include: on melee hit, on spell hit, on taking damage, on heal, on crit. **No proc from proc** — equipment procs cannot trigger other equipment procs, preventing runaway cascades.

### On-Use Effects
On-use trinkets and items register as additional abilities available to the class AI. This reuses the existing ability decision framework — the AI decides when to activate them like any other ability. On-use effects have cooldowns and are defined in the item data.

### V1 Item Pool Scope
The system and content ship together. V1 targets ~3 items per slot per armor type (~150-200 items), enough for meaningful gear choices from day one. This is a curated set — no placeholders.

### Match Config Integration
Each class has a default equipment loadout defined in a `loadouts.ron` data file (mapping class → item per slot). The headless match JSON config can override specific slots per character. This keeps testing low-friction while allowing full customization.

### Combat Logging
- **Pre-match**: Full equipment loadout summary per character in the match report header, listing all 17 equipped items.
- **During combat**: Log proc triggers (e.g., "[PROC] Warrior's Hand of Justice procs an extra attack") and on-use activations as combat events.

### No Set Bonuses (V1)
Each item is independent. No named sets with 2-piece/4-piece bonuses. Set bonuses are a natural future extension but not needed for V1.

### Visual Scope
- **Character models**: No visual changes to 3D models based on equipped gear. Characters look the same regardless of equipment.
- **Equipment UI**: An equipment panel or tooltip displays equipped items and their stats in the graphical client.
- **Proc feedback**: When a proc triggers, show floating combat text (proc name) and display the aura's existing visual effect if the aura type has one. Reuses existing floating text and aura visual systems.
- **On-use feedback**: On-use activations show in the ability timeline and floating text like any other ability cast.

## Risks and Constraints

- **Stat budget balance**: With 17 slots of stat bonuses, total character power will increase significantly. Base stats may need retuning, or items need careful stat budgeting to avoid power creep.
- **AI complexity for on-use items**: Each on-use trinket needs decision logic. Starting with simple "use on cooldown" or "use when health < X%" heuristics is reasonable before optimizing.
- **New stats**: If `armor` or `resistances` are introduced via equipment, the damage formula needs updating — this is a separate but related change.
- **Item content volume**: ~150-200 curated items is significant authoring work. Using Wowhead MCP for reference stats helps, but each item still needs naming, stat budgeting, and slot/class assignment.
- **Proc system interactions**: Procs that trigger auras need to interact correctly with existing aura stacking, diminishing returns, and dispel systems.

## Open Questions

*None — all questions resolved during brainstorm dialogue.*
