---
date: 2026-03-29
topic: equipment-ui-loadout-editor
---

# Equipment UI: Pre-Match Loadout Editor

## Problem Frame

The equipment system (first slice) is functional but invisible in the graphical client. Equipment can only be configured via headless JSON overrides — there's no way for a player using the graphical client to see or change equipment. Adding a loadout editor to the existing ViewCombatant screen makes equipment a real player-facing feature and validates the data foundation before building procs, on-use effects, or expanding the item pool.

## Requirements

**Loadout Display**

- R1. The ViewCombatant screen shows an equipment section listing all 17 gear slots with the currently equipped item name for each slot. Slots with the class default item show the item name normally. Slots with a player override show the item name with a visual indicator (e.g., different text color) to distinguish from defaults
- R2. Empty slots (no item equipped in default loadout and no override) display a placeholder like "— Empty —" in a muted style
- R3. A stat totals summary is always visible, dynamically calculated from the currently resolved loadout (defaults + overrides), showing the aggregate stat bonuses from all equipped items (e.g., "+119 HP, +42 AP, +3% Crit"). Show only non-zero stats. Display crit chance and movement speed as percentages. Omit weapon replacement stats (attack_damage, attack_speed) from the additive totals. Order: HP, Mana, Mana Regen, AP, SP, Crit, Move Speed. Updates immediately when items are changed in the UI

**Item Selection**

- R4. Clicking an equipment slot opens a picker showing all valid items for that slot and class (filtered by slot type, armor type, and class restrictions). Ring1 and Ring2 share the same item pool (all ring items). Trinket1 and Trinket2 share the same item pool (all trinket items)
- R5. The picker shows each item's name and its stat bonuses so the player can compare before selecting. For weapons, show attack damage range and attack speed as absolute values (not "+X" deltas)
- R6. Selecting an item in the picker immediately equips it and closes the picker
- R6b. The picker can be dismissed without changing equipment by clicking outside the picker area or pressing Escape
- R7. A "Reset to Default" option in the picker removes the player's override for that slot, restoring the class default item from loadouts.ron. Only shown when the slot has an active override

**Stat Tooltips**

- R8. *(Nice-to-have)* Hovering over a filled equipment slot shows a tooltip with that item's individual stat breakdown (name, item level, armor type, and each non-zero stat bonus). For weapons, show absolute damage range and attack speed. This is a polish item — the picker (R5) already shows stats for comparison

**Data Flow**

- R9. Equipment selections persist in `MatchConfig.team1_equipment` / `team2_equipment` as overrides on top of the class default loadout. Edits apply to the specific combatant identified by `ViewCombatantState.team` and `ViewCombatantState.slot`
- R10. Equipment selections survive navigating away from ViewCombatant and back (persisted in MatchConfig resource)
- R11. Equipment overrides are applied at spawn time using the existing `resolve_loadout` → `apply_equipment` pipeline (no new spawn-time work needed)

**Visual Design**

- R12. The equipment section follows existing ViewCombatant styling — dark theme, gold highlights for selected/hovered elements, class-colored accents
- R13. Slot layout is a vertical list grouped by category: Armor (Head, Shoulders, Chest, Wrists, Hands, Waist, Legs, Feet = 8 slots), Accessories (Neck, Back, Ring1, Ring2, Trinket1, Trinket2 = 6 slots), Weapons (MainHand, OffHand, Ranged = 3 slots) — with section headers, not a paper-doll diagram
- R14. The equipment section replaces the entire bottom row (Gear + Talents "Coming Soon" placeholders), taking full width for more space

## Success Criteria

- A player using the graphical client can view and change equipment for any combatant before starting a match
- Equipment changes are reflected in the combat log and combatant stats during the match
- The UI correctly filters items by slot and class restrictions — invalid items are never shown in the picker

## Scope Boundaries

- No equipment display during active match (inspect panel is a future slice)
- No item icons or artwork — text-based item names only for V1
- No drag-and-drop — click-to-select only
- No preset loadout system — per-slot selection only
- No item comparison overlay (side-by-side current vs. candidate) — the picker shows candidate stats, the tooltip shows current stats
- No changes to items.ron, loadouts.ron, or spawn-time stat application. Minor accessor additions to `ItemDefinitions` (e.g., an iterator or `items_for_slot()` method) are in scope as they are API additions, not data model changes

## Key Decisions

- **Location: ViewCombatant screen** — Equipment section added to the existing detail screen where rogue openers, warlock curses, and hunter pet preferences already live. Follows the established "click slot → detail screen" pattern.
- **Click slot → filtered picker** — Simple interaction model matching the existing icon-button pattern in the UI. Each slot is a clickable row; clicking opens a picker filtered to valid items.
- **Item names + stat totals + optional hover tooltips** — Compact by default (slot name + item name), with always-visible aggregate stat summary. Hover tooltips (R8) are a nice-to-have polish layer.
- **Vertical list, not paper-doll** — The existing ViewCombatant screen uses vertical layouts. A 17-slot vertical list grouped by category is simpler to implement and consistent with the UI's style.
- **"Reset to Default" not "Clear to Empty"** — Removing an override restores the class default item rather than emptying the slot. This avoids needing to represent "explicitly empty" in the override data model and prevents players from accidentally weakening characters.
- **Ring/Trinket slots are interchangeable** — Ring1 and Ring2 share the same item pool in the picker. Same for Trinket1 and Trinket2. This prevents empty pickers and matches player expectations from WoW.
- **Replace bottom row entirely** — The equipment section takes the full width of the bottom row, replacing both the Gear and Talents "Coming Soon" placeholders. This provides enough space for 17 slots without scrolling.

## Dependencies / Assumptions

- First slice equipment system is merged (PR #23) — data model, RON loading, `ItemDefinitions` and `DefaultLoadouts` resources, class restriction logic, and `resolve_loadout` all available
- `ItemDefinitions` and `DefaultLoadouts` resources are accessible from egui UI systems via Bevy `Res<>`
- `ItemDefinitions` needs an iteration method (e.g., `iter()` or `items_for_slot(slot, class)`) to enumerate items for the picker — this is a minor API addition
- Ring/Trinket interchangeability requires the picker to match items by slot category (Ring, Trinket) rather than exact slot (Ring1 vs Ring2). The existing `validate_class_restrictions` check on `item.slot == equipped_slot` will need a small adjustment for these paired slots

## Outstanding Questions

### Deferred to Planning

- [Affects R4][Technical] Should the item picker be an egui `Window` (floating popup), a `ComboBox`, or a custom painted panel? Planner should check which egui pattern best fits the existing ViewCombatant screen layout and can display multi-line item stats
- [Affects R3][Technical] Where exactly should the stat totals summary go — below the equipment list, or in a header area? Depends on available screen real estate after replacing the bottom row

## Next Steps

→ `/ce:plan` for structured implementation planning
