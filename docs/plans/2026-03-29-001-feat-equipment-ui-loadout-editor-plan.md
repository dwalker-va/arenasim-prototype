---
title: "feat: Add equipment UI loadout editor to ViewCombatant screen"
type: feat
status: completed
date: 2026-03-29
origin: docs/brainstorms/2026-03-29-equipment-ui-loadout-editor-requirements.md
---

# feat: Add Equipment UI Loadout Editor to ViewCombatant Screen

## Overview

Add a pre-match equipment loadout editor to the ViewCombatant screen, replacing the "GEAR" and "TALENTS" Coming Soon placeholders. Players can view all 17 equipment slots, change items via a filtered picker, and see aggregate stat totals — making the equipment system a visible, interactive feature in the graphical client.

## Problem Frame

The equipment system (first slice, PR #23) is functional but invisible in the graphical client. Equipment can only be configured via headless JSON overrides. Adding a loadout editor to ViewCombatant makes equipment a real player-facing feature and validates the data foundation before building procs, on-use effects, or expanding the item pool. (see origin: docs/brainstorms/2026-03-29-equipment-ui-loadout-editor-requirements.md)

## Requirements Trace

- R1. Display all 17 gear slots with current item names; visual indicator for overrides
- R2. Empty slots show "— Empty —" placeholder
- R3. Always-visible stat totals summary from resolved loadout (HP, Mana, Mana Regen, AP, SP, Crit, Move Speed); omit weapon stats; update immediately
- R4. Click slot → picker filtered by slot type, armor type, class restrictions; Ring1/Ring2 share pool, Trinket1/Trinket2 share pool
- R5. Picker shows item name + stat bonuses; weapons show absolute damage/speed
- R6. Selecting item equips immediately and closes picker
- R6b. Dismiss picker via click-outside or Escape
- R7. "Reset to Default" option in picker when slot has active override
- R8. (Nice-to-have) Hover tooltip with item stat breakdown
- R9. Selections persist in MatchConfig team equipment overrides, keyed by team/slot
- R10. Selections survive navigation (persisted in MatchConfig resource)
- R11. Equipment overrides applied at spawn via existing resolve_loadout → apply_equipment (no new work)
- R12. Follow ViewCombatant dark theme styling with gold highlights
- R13. Vertical list grouped: Armor (8), Accessories (6), Weapons (3) with section headers
- R14. Replace entire bottom row (Gear + Talents placeholders) with full-width equipment section

## Scope Boundaries

- No equipment display during active match
- No item icons — text-based names only
- No drag-and-drop — click-to-select only
- No preset loadout system
- No item comparison overlay
- No changes to items.ron, loadouts.ron, or spawn-time stat application
- Minor accessor additions to ItemDefinitions (items_for_slot method) are in scope

## Context & Research

### Relevant Code and Patterns

- **ViewCombatant UI**: `src/states/view_combatant_ui.rs` — main UI file, egui immediate-mode. System `view_combatant_ui()` at line 311 already takes `ResMut<MatchConfig>`
- **Panel rendering pattern**: `render_*_panel()` functions use `ui.group()` with gold section titles, `set_min_width/height`, consistent color constants
- **Selection picker pattern**: Rogue opener (line 511), Hunter pet (line 532), Warlock curse (line 552) — all follow: read current value → declare `clicked_*: Option<T>` → render clickable options → apply mutation after UI loop
- **Bottom row placeholder**: Lines 576-601 — two-column layout with `render_coming_soon_panel("GEAR")` and `render_coming_soon_panel("TALENTS")`, using `panel_width` and `bottom_panel_height = 100.0`
- **Equipment data model**: `src/states/play_match/equipment.rs` — `ItemSlot` enum (17 variants with `all()` and `name()`), `ItemConfig` struct (stat fields), `ItemDefinitions` resource, `DefaultLoadouts` resource
- **ItemDefinitions API**: Currently only `get()`, `get_unchecked()`, `item_count()` — needs `items_for_slot()` iterator
- **can_equip()**: Free function at equipment.rs:273 — checks class restrictions and armor type
- **resolve_loadout()**: equipment.rs:321 — merges defaults with overrides
- **MatchConfig equipment**: `team1_equipment: Vec<HashMap<ItemSlot, ItemId>>` and `team2_equipment` at match_config.rs:275-277
- **ViewCombatantState**: team (u8) + slot (usize) + class — identifies which combatant is being viewed
- **Color constants**: Gold `(255, 215, 0)`, title `(230, 204, 153)`, subtitle `(170, 170, 170)`, dark bg `(20, 20, 30)`, group bg `(35, 35, 45)`, gray border `(80, 80, 90)`
- **Layout dimensions**: `content_width = (screen_width * 0.7).min(700.0).max(500.0)`, `panel_width = (content_width - 20.0) / 2.0`

### Institutional Learnings

- **Dual system registration** (docs/solutions/): New ECS systems must register in both `systems.rs` (headless) and `states/mod.rs` (graphical). Not applicable here since this is purely UI — no new ECS systems needed
- **Single source of truth**: Derive UI data dynamically from equipment definitions rather than hardcoding item lists (from Paladin implementation doc)
- **Component placement**: Shared components belong in `components/mod.rs` (from Paladin doc)

## Key Technical Decisions

- **egui Window for item picker**: The picker should use `egui::Window` (floating popup) rather than `ComboBox` or inline panel. Rationale: ComboBox cannot show multi-line stat info per item; an inline panel would require complex layout reflow. An egui Window can overlay content, show detailed stats, and be dismissed with click-outside — matching R5, R6, R6b naturally. The existing codebase doesn't use egui Windows yet, but the API is straightforward and consistent with the immediate-mode pattern. (Resolves deferred question from origin doc)
- **Stat totals below equipment list**: Place the aggregate stat summary directly below the slot list within the equipment panel. Rationale: it's contextually tied to the equipment and avoids header clutter. The full-width panel (R14) provides enough vertical space. (Resolves deferred question from origin doc)
- **Ring/Trinket slot matching via category helper**: Add an `ItemSlot::is_same_slot_type()` method that treats Ring1/Ring2 as equivalent and Trinket1/Trinket2 as equivalent. Use this in the picker filter instead of exact slot equality. This keeps the matching logic in the data layer rather than scattered through UI code
- **Equipment panel state as local struct**: Track picker state (which slot is open, if any) in a `Local<EquipmentPickerState>` Bevy system parameter rather than a global resource. This keeps the state scoped to the UI system and avoids polluting the resource namespace
- **Full-width panel replaces both placeholders**: Replace the entire two-column bottom section (lines 576-601) with a single full-width equipment panel. Height will be dynamic based on content rather than the fixed 100px `bottom_panel_height`

## Open Questions

### Resolved During Planning

- **Picker widget type** (from origin): Use egui::Window — supports multi-line stat display, overlay, click-outside dismiss, and Escape key handling
- **Stat totals placement** (from origin): Below the equipment slot list within the same panel
- **Ring/Trinket interchangeability mechanism**: Add `is_same_slot_type()` to ItemSlot rather than modifying `validate_class_restrictions` — the picker is a UI-only concern and validation happens separately at spawn time

### Deferred to Implementation

- Exact scroll behavior if 17 slots + stat totals exceed available vertical space — egui's ScrollArea should handle this naturally but may need tuning
- Precise egui Window sizing for the picker — will depend on how stat text renders at runtime

## High-Level Technical Design

> *This illustrates the intended approach and is directional guidance for review, not implementation specification. The implementing agent should treat it as context, not code to reproduce.*

```
ViewCombatant Screen Layout (after change):
┌──────────────────────────────────────┐
│  ← Back        VIEW COMBATANT       │
├──────────────────────────────────────┤
│  [Class Icon]  Class Name            │
│  Class description text              │
├──────────────┬───────────────────────┤
│  STATS       │  ABILITIES            │
│  HP: 4000    │  [Icon] Mortal Strike │
│  Mana: 0     │  [Icon] Charge        │
│  AP: 140     │  ...                  │
├──────────────┴───────────────────────┤
│  [Class-specific panel if any]       │
├──────────────────────────────────────┤
│  EQUIPMENT (full width)              │
│  ┌─ Armor ─────────────────────────┐ │
│  │ Head      Lionheart Helm        │ │
│  │ Shoulders Shoulderplates of V.  │ │
│  │ ...                             │ │
│  ├─ Accessories ───────────────────┤ │
│  │ Neck      Amulet of Power       │ │
│  │ Ring 1    Band of Accuria  [*]  │ │  [*] = override indicator
│  │ ...                             │ │
│  ├─ Weapons ───────────────────────┤ │
│  │ Main Hand Arcanite Reaper       │ │
│  │ ...                             │ │
│  ├──────────────────────────────────┤ │
│  │ +119 HP  +42 AP  +3% Crit      │ │  ← stat totals
│  └──────────────────────────────────┘ │
└──────────────────────────────────────┘

Item Picker (egui::Window, floating):
┌─ Select: Head ──────────────────────┐
│  ▸ Reset to Default                 │  ← only if override active
│  ───────────────────────────────────│
│  Lionheart Helm                     │
│    +40 HP, +28 AP, +1% Crit        │
│  ─────────────────────────────────  │
│  Onslaught Head Guard               │
│    +52 HP, +18 AP                   │
│  ─────────────────────────────────  │
│  Magisters Crown                    │  ← grayed out / not shown
│  ...                                │     (wrong armor type)
└─────────────────────────────────────┘
```

**Data flow per frame (immediate-mode):**
1. Read ViewCombatantState → get team, slot, class
2. Read MatchConfig → get current overrides for this combatant
3. Call resolve_loadout(class, defaults, overrides) → resolved loadout
4. Render slot list from resolved loadout + ItemDefinitions lookups
5. If picker open: render egui::Window with items_for_slot() filtered by can_equip()
6. On selection: write override to MatchConfig (or remove on Reset to Default)
7. Calculate stat totals from resolved loadout → render summary

## Implementation Units

- [ ] **Unit 1: ItemDefinitions API — items_for_slot() method**

  **Goal:** Add an iterator method to ItemDefinitions that returns all items valid for a given slot and class, enabling the picker to enumerate choices.

  **Requirements:** R4 (filtered picker)

  **Dependencies:** None

  **Files:**
  - Modify: `src/states/play_match/equipment.rs`
  - Test: `src/states/play_match/equipment.rs` (add test in existing #[cfg(test)] module, or create one)

  **Approach:**
  - Add `ItemSlot::is_same_slot_type(&self, other: &ItemSlot) -> bool` — returns true for exact match, Ring1↔Ring2, Trinket1↔Trinket2
  - Add `ItemDefinitions::items_for_slot(&self, slot: ItemSlot, class: CharacterClass) -> Vec<(ItemId, &ItemConfig)>` — iterates all items, filters by `is_same_slot_type` and `can_equip(class, item)`, returns sorted by name for stable picker ordering

  **Patterns to follow:**
  - Existing `ItemDefinitions` methods at equipment.rs:349-368
  - `can_equip()` function at equipment.rs:273

  **Test scenarios:**
  - Happy path: items_for_slot(Head, Warrior) returns plate and mail head items but not cloth
  - Happy path: items_for_slot(Ring2, Mage) returns all ring items (Ring1-slotted and Ring2-slotted)
  - Happy path: items_for_slot(Trinket1, Priest) returns all trinket items
  - Edge case: is_same_slot_type(Ring1, Ring2) returns true; is_same_slot_type(Ring1, Neck) returns false
  - Edge case: items_for_slot for a slot with class-restricted items only shows items for that class

  **Verification:**
  - All ring items appear in both Ring1 and Ring2 picker queries
  - Class armor type restrictions properly filter items

- [ ] **Unit 2: Equipment panel — slot list display**

  **Goal:** Replace the bottom row Coming Soon placeholders with a full-width equipment panel showing all 17 slots grouped by category with current item names.

  **Requirements:** R1, R2, R12, R13, R14

  **Dependencies:** Unit 1 (for item name lookups, though can use `ItemDefinitions::get()` directly)

  **Files:**
  - Modify: `src/states/view_combatant_ui.rs`
  - Modify: `src/states/play_match/equipment.rs` (pub use if needed)

  **Approach:**
  - Add `Res<ItemDefinitions>` and `Res<DefaultLoadouts>` to the `view_combatant_ui` system parameters
  - Replace lines 576-601 (the two-column Coming Soon layout) with a single full-width `render_equipment_panel()` call
  - `render_equipment_panel()` takes ui, content_width, view_state, match_config, items, defaults, and a mutable picker state reference
  - Use `resolve_loadout()` to get the currently resolved loadout for this combatant
  - Render three sections (Armor, Accessories, Weapons) with gold section sub-headers
  - Each slot is a clickable row: slot name (left-aligned) + item name (right-aligned)
  - Override indicator: items from overrides shown in a distinct color (e.g., brighter gold or green) vs default items in standard text color
  - Empty slots: "— Empty —" in muted gray
  - Use `egui::Sense::click()` on each row to detect clicks for opening the picker (Unit 3)

  **Patterns to follow:**
  - `render_stats_panel()` at view_combatant_ui.rs:607 — group + section title pattern
  - Rogue opener panel — clickable item pattern with response.clicked() tracking
  - Color constants used throughout ViewCombatant panels

  **Test scenarios:**
  - Happy path: panel renders 17 slots in three groups with section headers
  - Happy path: default items show item names from resolved loadout
  - Happy path: overridden slots show visual indicator distinguishing them from defaults
  - Edge case: empty slot (no default and no override) shows "— Empty —" placeholder
  - Integration: panel reads from MatchConfig overrides and DefaultLoadouts to build resolved view

  **Verification:**
  - The Coming Soon placeholders are gone
  - All 17 slots visible in correct category grouping
  - Equipment panel takes full width of the bottom area
  - Items from overrides are visually distinct from default items

- [ ] **Unit 3: Item picker — selection and persistence**

  **Goal:** Implement the floating item picker window that opens when clicking a slot, showing filtered items with stats, and persisting selections to MatchConfig.

  **Requirements:** R4, R5, R6, R6b, R7, R9, R10

  **Dependencies:** Unit 1, Unit 2

  **Files:**
  - Modify: `src/states/view_combatant_ui.rs`

  **Approach:**
  - Add a `EquipmentPickerState` struct (tracks `open_slot: Option<ItemSlot>`) as a `Local<>` system parameter
  - When a slot row is clicked (from Unit 2), set `open_slot = Some(slot)`
  - Render an `egui::Window` titled "Select: {slot_name}" when open_slot is Some
  - Window contents: optional "Reset to Default" button (only when slot has active override) + list of valid items from `items_for_slot()`
  - Each item row shows: item name + stat bonuses formatted per R5 (additive "+X" for armor stats, absolute values for weapon damage/speed)
  - Currently equipped item highlighted with gold border/background
  - On item click: insert override into the appropriate MatchConfig team equipment map, close picker
  - On "Reset to Default" click: remove the override for that slot from MatchConfig, close picker
  - Dismiss: egui::Window with `.collapsible(false).resizable(false)` — detect close via the window's `show()` return (open state becomes false on click-outside or X button). Also check for Escape key press

  **Patterns to follow:**
  - Warlock curse picker — mutation pattern (declare clicked option, apply after loop)
  - MatchConfig access via `view_state.team` and `view_state.slot` to index into team equipment vectors
  - egui::Window API for floating popups

  **Test scenarios:**
  - Happy path: clicking Head slot opens picker showing only head-slot items equippable by the viewed class
  - Happy path: selecting an item writes override to MatchConfig and closes picker
  - Happy path: Ring2 slot picker shows all ring items (Ring1 and Ring2 defined items)
  - Happy path: "Reset to Default" appears only when slot has override, removes override on click
  - Happy path: picker shows stat bonuses per item; weapon items show damage range and attack speed
  - Edge case: Escape key dismisses picker without changing equipment
  - Edge case: clicking outside picker dismisses it
  - Edge case: currently equipped item is visually indicated in the picker
  - Integration: selections persist in MatchConfig across ViewCombatant navigations (leave and return)

  **Verification:**
  - Can change equipment for any slot on any combatant
  - Picker only shows valid items per class and slot
  - Selections survive navigating away and back to ViewCombatant
  - Picker dismissable via Escape and click-outside

- [ ] **Unit 4: Stat totals summary**

  **Goal:** Add an always-visible aggregate stat summary below the equipment slot list, calculated from the resolved loadout.

  **Requirements:** R3

  **Dependencies:** Unit 2 (equipment panel exists)

  **Files:**
  - Modify: `src/states/view_combatant_ui.rs`

  **Approach:**
  - After rendering the slot list in `render_equipment_panel()`, add a stat totals section
  - Iterate all items in the resolved loadout, sum each additive stat (max_health, max_mana, mana_regen, attack_power, spell_power, crit_chance, movement_speed)
  - Omit weapon replacement stats (attack_damage, attack_speed) per R3
  - Display order: HP, Mana, Mana Regen, AP, SP, Crit, Move Speed
  - Format: "+119 HP, +42 AP, +3% Crit" — crit and movement speed as percentages, others as flat values
  - Only show non-zero stats
  - Use a horizontal layout with separator styling; muted gold or white text
  - Updates immediately since the entire panel re-renders each frame in immediate-mode

  **Patterns to follow:**
  - `render_stats_panel()` — stat display formatting
  - ItemConfig stat fields at equipment.rs:222-247

  **Test scenarios:**
  - Happy path: stat totals reflect sum of all equipped items' additive stats
  - Happy path: weapon stats (attack_damage, attack_speed) are omitted from totals
  - Happy path: crit_chance and movement_speed displayed as percentages
  - Edge case: zero-value stats are omitted from display
  - Integration: stat totals update immediately when an item is changed via picker

  **Verification:**
  - Stat summary visible below equipment list
  - Values match manual sum of all equipped items
  - Changing an item immediately updates the totals

- [ ] **Unit 5: (Nice-to-have) Hover tooltips for equipped items**

  **Goal:** Show a tooltip on hover over equipment slots with the item's full stat breakdown.

  **Requirements:** R8

  **Dependencies:** Unit 2

  **Files:**
  - Modify: `src/states/view_combatant_ui.rs`

  **Approach:**
  - On each slot row, check `response.hovered()` in addition to clicked
  - Use `response.on_hover_ui()` or `egui::show_tooltip_at_pointer()` to render a tooltip
  - Tooltip contents: item name, item level, armor type, and each non-zero stat bonus
  - For weapons: show absolute damage range and attack speed
  - Keep tooltip compact — no interaction needed

  **Patterns to follow:**
  - egui tooltip API (response.on_hover_ui)
  - Stat display format from Unit 4

  **Test scenarios:**
  - Happy path: hovering a filled slot shows tooltip with item name, level, armor type, stats
  - Happy path: weapon tooltip shows absolute damage range and attack speed
  - Edge case: hovering an empty slot shows no tooltip (or minimal "Empty" tooltip)
  - Edge case: tooltip disappears when mouse moves away

  **Verification:**
  - Hovering equipment slots shows contextual item information
  - Tooltip displays correct stats matching the item definition

## System-Wide Impact

- **Interaction graph:** The equipment panel reads `ItemDefinitions`, `DefaultLoadouts` (immutable resources) and reads/writes `MatchConfig` (already mutably accessed by the system). No new resources, events, or callbacks introduced
- **Error propagation:** No error paths — all items exist in loaded definitions; the picker filters prevent invalid selections. If a previously overridden item were removed from items.ron, `ItemDefinitions::get()` returns None and the slot would show as empty
- **State lifecycle risks:** MatchConfig equipment overrides must be initialized with enough Vec entries for each team slot. Check that `team1_equipment` and `team2_equipment` Vecs are properly sized when combatants are added/removed in ConfigureMatch
- **API surface parity:** Headless mode already supports equipment overrides via JSON config — the UI adds graphical parity
- **Unchanged invariants:** `resolve_loadout()`, `apply_equipment()`, spawn-time stat application, `items.ron`, `loadouts.ron`, and the headless config path are all unchanged

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| MatchConfig equipment Vec not properly sized for team slot count | Check Vec initialization in ConfigureMatch when adding/removing combatants; ensure equipment Vec resizes accordingly |
| egui::Window layering/focus issues with the main panel | egui Windows render on top by default; test that click-outside properly closes the picker |
| 17 slots + stat totals may exceed available vertical space | Wrap the equipment panel content in `egui::ScrollArea::vertical()` as a fallback |
| Ring/Trinket slot type matching could confuse validation at spawn time | The `is_same_slot_type` helper is only used in the UI picker filter; spawn-time `validate_class_restrictions` is unchanged and items have correct specific slots in loadouts.ron |

## Sources & References

- **Origin document:** [docs/brainstorms/2026-03-29-equipment-ui-loadout-editor-requirements.md](docs/brainstorms/2026-03-29-equipment-ui-loadout-editor-requirements.md)
- Related code: `src/states/view_combatant_ui.rs`, `src/states/play_match/equipment.rs`, `src/states/match_config.rs`
- Related PR: #23 (Equipment system first slice)
- Institutional learnings: `docs/solutions/implementation-patterns/adding-new-class-paladin.md`, `docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md`
