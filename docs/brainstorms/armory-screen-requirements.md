---
date: 2026-05-21
topic: armory-screen
---

# Armory Screen

## Summary

A player-facing armory screen, reachable from the main menu, that presents every item defined in `items.ron` as a unified scrollable grid of uniform tiles with a chip-bar of filters across the top. The vibe is "wall of trophies" — minimalist frames, icon art carrying the variety, hover tooltips for stat details.

---

## Problem Frame

ArenaSim has 128 items across 16 slots and 4 armor types, all defined in `assets/config/items.ron`. Today the only ways to see what equipment exists in the world are:

- Opening the RON file in an editor (developer-only, not aspirational)
- Starting a match and inspecting an individual combatant via the existing `ViewCombatant` screen (single character's loadout, not the full catalog)

There is no surface that lets a player browse the full set of equipment as a *catalog*. For a prototype that wants to convey a fantasy-game atmosphere, the absence of a "look at all this loot" screen is a missing feeling, not just a missing feature. The main menu currently offers MATCH / OPTIONS / EXIT, with no entry point into the game's content.

The armory exists to make the item pool feel like a real, browseable inventory the world owns — without yet committing to loadout-editing or progression mechanics.

---

## Requirements

**Entry point**
- R1. The main menu shall include an "ARMORY" button between MATCH and OPTIONS that transitions the game state to a new `Armory` state.
- R2. The armory screen shall include a back-to-menu affordance that returns to `MainMenu`.

**Wall layout**
- R3. The armory shall render every item defined in `assets/config/items.ron` as a uniform-sized tile, with no per-slot or per-type grouping in the layout itself.
- R4. Each tile shall display the item's icon as its primary visual content, with the item's `item_level` shown as a small numeric badge on the tile.
- R5. Tiles shall share identical frame styling — no border-color rarity tiers, no per-armor-type tinting.
- R6. The wall shall be vertically scrollable; the grid shall wrap based on available width.
- R7. The default sort order shall be by slot, then by `item_level` descending, so that items of the same kind cluster together and higher-iLvl items appear first within each cluster.

**Filtering**
- R8. A horizontal chip-bar shall sit above the wall, exposing filters for: Slot, Armor Type, Item Level range, and free-text Name search.
- R9. Filters shall be multi-select within an axis (e.g., Head + Chest can both be active) and combined across axes via AND.
- R10. The chip-bar shall display the current item count as `N / 128 items` (or whatever the total is, computed from `items.ron` at load).
- R11. When the active filter combination matches zero items, the wall shall render an empty-state message in place of the grid.

**Item detail on hover**
- R12. Hovering a tile shall present a tooltip containing: item name, item level, slot, armor type (if applicable), and the item's full stat block (e.g., `+15 max health`, `+8 attack power`, `+2% crit chance`).
- R13. The tooltip shall surface only stats that are non-default for the item (zero/missing values are omitted) and shall format numeric stats consistently with the existing `ViewCombatant` conventions.
- R14. There shall be no click-to-spotlight detail panel — tile interaction is hover-only.

**Visual consistency**
- R15. The armory screen shall use the same dark background palette (`rgb(20, 20, 30)`) and gold-accent typography (`rgb(230, 204, 153)` for headings, `rgb(230, 217, 191)` for buttons) as the existing main menu and configure-match screens.
- R16. Item icons shall be loaded via the same `bevy_egui` texture-registration pattern used by `configure_match_ui::load_class_icons` (handles kept alive in a resource, registered with egui once loaded).

---

## Success Criteria

- A player opening the game can reach the armory in one click from the main menu and immediately see a wall of items with recognizable WoW Classic icon art.
- A player wanting to find all plate chest pieces above iLvl 55 can select two chips and a level filter, see the wall narrow live, and identify candidates without leaving the screen.
- Hover tooltips communicate enough stat information that no other screen is needed to understand what an item does.
- Adding a new item to `items.ron` causes it to appear on the wall on next launch with no code changes (data-driven contract preserved).
- The screen never blocks: opening it, switching filters, and returning to the menu are all snappy and never gated by loading screens beyond first-frame icon registration.

---

## Scope Boundaries

- Item rarity / quality tiers — no `quality` field is added to `items.ron`; tiles are visually flat by choice.
- Class-restriction filter — items have no class binding in the data model today, and we are not adding one.
- Designer-mode toggle — no item-level-budget % readout, stat-per-point breakdown, or balance-audit affordances. Those belong in a separate tool if ever needed.
- Loadout editing or item equipping — armory is read-only; the path from "browsing" to "equipping" is explicitly out of scope for this feature.
- Click-to-spotlight detail panel — interaction is hover-only.
- Side-by-side item comparison.
- Favorites, pinning, or persisted UI state across sessions.
- Item lore text or flavor descriptions — not present in `items.ron` today, not added here.

---

## Key Decisions

- **Unified wall over slot-grouped vault layout.** Density and discoverability for 128 items beats curated shelves; filters carry the categorization role.
- **Plain uniform tile frames over rarity color tiers.** The icon art is already varied enough to give the wall texture, and avoiding a `quality` field keeps the data model untouched.
- **Default sort by slot then iLvl descending.** Compensates for the lack of visual rank on plain frames by providing implicit grouping; users scanning the wall still get an "all the helms together, best first" reading.
- **Hover-only item detail, no spotlight panel.** Keeps the screen lightweight and matches the minimalist tile choice; tooltips are the WoW-native pattern.
- **Showcase only, not a gateway to loadout editing.** Keeps tone aspirational and scope tight; any future progression / equipping work is a separate feature.

---

## Dependencies / Assumptions

- The screen will be built with `bevy_egui`, matching the existing `configure_match_ui`, `view_combatant_ui`, and `main_menu_ui` patterns.
- `items.ron` is the single source of truth for the item catalog; the armory reads whatever the equipment system already parses, with no parallel loader.
- All current items have icons at the paths declared by their `icon:` field (verified: 128 items in `items.ron`, icon files present under `assets/icons/items/`).
- A new `GameState::Armory` variant will be added; per `tests/registration_audit.rs`, any new systems will be registered in either `add_core_combat_systems` (if combat-relevant — unlikely here) or in `StatesPlugin::build()` (UI-only, the expected path for this screen).
- The existing 16 slots (`Head`, `Chest`, `Legs`, `Hands`, `Feet`, `Waist`, `Wrists`, `Shoulders`, `Back`, `Neck`, `MainHand`, `OffHand`, `Ranged`, `Ring1`, `Ring2`, `Trinket1`) and 4 armor types (`Plate`, `Mail`, `Leather`, `Cloth`) form the chip set; `Ring1`/`Ring2` and `Trinket1` may be displayed as a single `Ring` / `Trinket` filter chip for UX, but this is a planning-time choice.
