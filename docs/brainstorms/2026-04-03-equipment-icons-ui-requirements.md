---
date: 2026-04-03
topic: equipment-icons-ui
status: implemented
---

# Equipment Icons in View Combatant UI

## Problem Frame

The equipment UI in the view combatant scene is entirely text-based. Players see slot names and item names but no visual representation of equipment. WoW players expect item icons as a primary visual identifier — they're how you recognize gear at a glance. The Wowhead MCP already provides `get_item_icon()` for fetching icon URLs, and the spell icon pipeline is a proven pattern to extend.

## Requirements

**Icon Display**
- R1. Each equipped item in the equipment panel shows its icon next to the slot/item name
- R2. Each selectable item in the picker window shows its icon next to the item name
- R3. Empty/unequipped slots show a placeholder or no icon rather than a broken image

**Icon Pipeline**
- R4. Item icons are downloaded from Wowhead via the MCP's `get_item_icon` tool and saved to `assets/icons/items/`
- R5. Item icons are loaded and registered with egui following the same pattern as spell icons (`SpellIcons` / `SpellIconHandles`)
- R6. Icon-to-item mapping uses a similar approach to `get_ability_icon_path()` for items

## Success Criteria
- All items in `items.ron` have visible icons in both the equipment panel and item picker
- Icons load without blocking or visual glitches
- The pattern is consistent with existing spell icon loading

## Scope Boundaries
- No item rarity color borders or quality indicators (future enhancement)
- No icons outside the view combatant scene (e.g., no icons in combat HUD or match results)
- No dynamic icon fetching at runtime — all icons are pre-downloaded assets

## Key Decisions
- **Extend spell icon pattern**: Reuse the proven `load_*_icons()` + egui texture registration approach rather than inventing a new system
- **Both locations**: Icons appear in equipment panel slots and in the item picker for maximum visual impact

## Outstanding Questions

### Deferred to Planning
- [Affects R6][Needs research] What are the actual Wowhead icon filenames for all items in `items.ron`? Batch lookup needed during implementation.
- [Affects R1, R2][Technical] What icon sizes work best in the equipment panel vs picker? Likely similar to ability icon sizes used elsewhere (28-48px).

## Next Steps
→ `/ce:plan` for structured implementation planning
