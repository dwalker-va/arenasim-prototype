---
title: "feat: Add equipment icons to view combatant UI"
type: feat
status: completed
date: 2026-04-03
origin: docs/brainstorms/2026-04-03-equipment-icons-ui-requirements.md
---

# feat: Add equipment icons to view combatant UI

## Overview

Add item icons to the equipment panel and item picker in the view combatant scene. Icons are downloaded from Wowhead via the MCP tool and loaded at runtime following the same pattern as ability icons (`AbilityIcons` / `AbilityIconHandles`).

## Problem Frame

The equipment UI is entirely text-based. WoW players identify gear by icons at a glance. The Wowhead MCP already provides `get_item_icon()` and the view combatant scene already loads ability icons with an identical pattern. (see origin: docs/brainstorms/2026-04-03-equipment-icons-ui-requirements.md)

## Requirements Trace

- R1. Each equipped item in the equipment panel shows its icon next to the slot/item name
- R2. Each selectable item in the picker window shows its icon next to the item name
- R3. Empty/unequipped slots show a placeholder or no icon rather than a broken image
- R4. Item icons are downloaded from Wowhead via `get_item_icon` and saved to `assets/icons/items/`
- R5. Item icons are loaded and registered with egui following the same pattern as ability icons
- R6. Icon-to-item mapping uses a similar approach to `get_ability_icon_path()` for items

## Scope Boundaries

- No item rarity color borders or quality indicators
- No icons outside the view combatant scene
- No dynamic icon fetching at runtime — all icons are pre-downloaded assets

## Context & Research

### Relevant Code and Patterns

- **Ability icon loading in view combatant**: `src/states/view_combatant_ui.rs:291-354` — `AbilityIcons` + `AbilityIconHandles` resources, `load_ability_icons()` system. This is the exact pattern to replicate for items.
- **Icon path mapping**: `src/states/play_match/rendering/mod.rs:30-87` — `get_ability_icon_path()` maps ability names to `icons/abilities/*.jpg` paths. Item equivalent will map `ItemId` variants to `icons/items/*.jpg`.
- **Icon rendering in UI**: Throughout view_combatant_ui.rs, icons are drawn via `ui.allocate_exact_size()` + `painter.image()` at various sizes (22px for ability rows, 42-48px for selection buttons).
- **Equipment panel**: `src/states/view_combatant_ui.rs:1157-1330` — `render_equipment_panel()` currently uses text-only `selectable_label` for slot rows and picker items.
- **System registration**: `src/states/mod.rs:77-84` — View combatant systems chained: `load_ability_icons` then `view_combatant_ui`. Item icon loader will be added to this chain.
- **Item data**: `src/states/play_match/equipment.rs:131-206` — `ItemId` enum with ~46 variants. `ItemDefinitions` resource loaded from `assets/config/items.ron`.

### Icon Sizing Decision

Based on existing patterns in the view combatant UI:
- **Equipment panel slots**: 22px — matches ability row icons at line 826
- **Item picker entries**: 22px — same row-style layout as the equipment panel
- These are compact list views, not selection buttons, so the smaller size is appropriate

## Key Technical Decisions

- **Reuse AbilityIcons pattern**: Create `ItemIcons` + `ItemIconHandles` resources and `load_item_icons()` system, mirroring the ability icon infrastructure already in view_combatant_ui.rs. This keeps all view combatant icon loading co-located.
- **Map by ItemId variant name**: Use `get_item_icon_path(item_id: &ItemId) -> Option<&'static str>` matching on enum variants rather than string names. This is type-safe and catches missing icons at compile time if new items are added.
- **No placeholder for empty slots**: Empty slots already show "— Empty —" in muted gray text. Adding a placeholder icon adds visual noise for no benefit — just skip the icon for empty slots (R3).
- **Download icons via MCP during implementation**: Use `get_item_icon` for each item name to get the Wowhead icon URL, then download and save to `assets/icons/items/`. This is a one-time manual step during implementation, not runtime behavior.

## Open Questions

### Resolved During Planning

- **Icon sizes**: 22px for both equipment panel and picker, matching ability row icon sizing already used in the same screen.
- **Empty slot handling**: No placeholder icon — just skip drawing the icon when slot is empty or item unknown.
- **Mapping approach**: Map by `ItemId` enum variant (type-safe) rather than string name.

### Deferred to Implementation

- **Exact Wowhead icon filenames**: Each item needs a `get_item_icon("Item Name")` MCP call to discover the icon filename. This is mechanical work done during Unit 1.

## Implementation Units

- [ ] **Unit 1: Download item icons from Wowhead**

**Goal:** Fetch icon URLs for all ~46 items in items.ron via the Wowhead MCP and download them to `assets/icons/items/`.

**Requirements:** R4

**Dependencies:** None

**Files:**
- Create: `assets/icons/items/` directory with ~46 .jpg icon files

**Approach:**
- For each item in items.ron, call `mcp__wowhead-classic__get_item_icon("<item name>")` to get the icon URL
- Download each icon and save with the Wowhead filename (e.g., `inv_helmet_23.jpg`)
- Track the mapping of ItemId variant → icon filename for Unit 2

**Patterns to follow:**
- Existing icons in `assets/icons/abilities/` use Wowhead's original filenames

**Test scenarios:**
- Happy path: All ~46 items have downloadable icons in `assets/icons/items/`
- Edge case: Any items not found on Wowhead are noted for manual resolution

**Verification:**
- `assets/icons/items/` contains an icon file for every item in items.ron

- [ ] **Unit 2: Add item icon infrastructure to view combatant UI**

**Goal:** Create `ItemIcons`/`ItemIconHandles` resources, `get_item_icon_path()` mapping function, and `load_item_icons()` system.

**Requirements:** R5, R6

**Dependencies:** Unit 1 (need icon filenames for the mapping)

**Files:**
- Modify: `src/states/view_combatant_ui.rs` — add `ItemIcons`, `ItemIconHandles` resources, `get_item_icon_path()` function, `load_item_icons()` system
- Modify: `src/states/mod.rs` — add `load_item_icons` to the ViewCombatant system chain

**Approach:**
- Add `ItemIcons` and `ItemIconHandles` resources mirroring `AbilityIcons`/`AbilityIconHandles`
- Add `get_item_icon_path(item_id: &ItemId) -> Option<&'static str>` that matches on ItemId enum variants and returns icon paths under `icons/items/`
- Add `load_item_icons()` system following the same 3-phase pattern as `load_ability_icons()`: collect all ItemId variants → load handles → wait for images → register with egui
- Register `load_item_icons` in the ViewCombatant update system chain in `states/mod.rs`
- Init `ItemIcons` and `ItemIconHandles` as resources (same as AbilityIcons init pattern)

**Patterns to follow:**
- `AbilityIcons` / `AbilityIconHandles` / `load_ability_icons()` in view_combatant_ui.rs:40-354
- `get_ability_icon_path()` in rendering/mod.rs:30-87

**Test scenarios:**
- Happy path: `load_item_icons` loads all item icon textures and registers them with egui on the first frame of ViewCombatant state
- Happy path: `get_item_icon_path` returns a valid path for every ItemId variant that has an icon
- Edge case: `get_item_icon_path` returns `None` for any ItemId without a downloaded icon (should not happen if Unit 1 is complete, but handled gracefully)

**Verification:**
- Project compiles with no warnings
- Entering the view combatant screen logs "Item icons loaded" similar to the ability icons log

- [ ] **Unit 3: Display icons in equipment panel and item picker**

**Goal:** Render item icons next to slot entries in the equipment panel and next to items in the picker window.

**Requirements:** R1, R2, R3

**Dependencies:** Unit 2

**Files:**
- Modify: `src/states/view_combatant_ui.rs` — update `render_equipment_panel()` to accept and use `ItemIcons`, render icons in slot rows and picker entries

**Approach:**
- Pass `&ItemIcons` into `render_equipment_panel()` 
- In the equipment panel slot loop (lines 1207-1242): before the selectable label, draw the item icon at 22px if available using the allocate_exact_size + painter.image pattern used elsewhere in this file
- In the picker window item loop (lines 1283-1304): similarly draw the item icon at 22px before the item name/stats text
- For empty slots or missing icons: skip drawing the icon, keeping current text-only behavior (R3)
- Wire the `ItemIcons` resource through the `view_combatant_ui` system to `render_equipment_panel()`

**Patterns to follow:**
- Ability icon rendering at view_combatant_ui.rs:824-855 (22px icon + text row pattern)
- The `allocate_exact_size` + `painter.image` pattern used throughout view_combatant_ui.rs

**Test scenarios:**
- Happy path: Equipment panel shows 22px icon next to each equipped item name
- Happy path: Item picker shows 22px icon next to each selectable item
- Edge case: Empty slots ("— Empty —") render without an icon and without visual glitches
- Edge case: If an item has no icon (get_item_icon_path returns None), the row renders text-only without spacing artifacts

**Verification:**
- Project compiles
- Visual inspection: equipment panel shows icons next to item names
- Visual inspection: item picker shows icons next to selectable items
- Visual inspection: empty slots look clean without broken icons

## System-Wide Impact

- **Interaction graph:** Only the ViewCombatant state is affected. No impact on PlayMatch, headless mode, or other UI screens.
- **State lifecycle risks:** None — item icons load once per ViewCombatant entry, same as ability icons.
- **Unchanged invariants:** Equipment override system, item filtering, 2H conflict logic, tooltip rendering — all unchanged.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Some items may not exist on Wowhead Classic (custom/invented names) | Use a generic placeholder icon or find a close match manually |
| Adding ~46 icon assets increases binary size | Each icon is ~5-10KB; total ~300-500KB is negligible |

## Sources & References

- **Origin document:** [docs/brainstorms/2026-04-03-equipment-icons-ui-requirements.md](docs/brainstorms/2026-04-03-equipment-icons-ui-requirements.md)
- Related code: `src/states/view_combatant_ui.rs` (AbilityIcons pattern, equipment panel)
- Related code: `src/states/play_match/rendering/mod.rs` (get_ability_icon_path pattern)
- Related code: `src/states/play_match/equipment.rs` (ItemId enum, ItemDefinitions)
