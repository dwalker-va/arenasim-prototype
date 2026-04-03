---
title: "feat: Add equipment lookup and icon tools to Wowhead MCP"
type: feat
status: completed
date: 2026-04-03
---

# feat: Add equipment lookup and icon tools to Wowhead MCP

## Overview

Extend the Wowhead Classic MCP server with tools for looking up equipment (items) by name or ID, fetching item icons, and listing known items. This mirrors the existing spell lookup tools and enables pulling accurate item stats and icons from Wowhead rather than relying on training data.

## Problem Frame

The equipment system now has a full loadout editor UI, but item data (stats, icons) is currently hand-authored in `items.ron`. When adding new items or verifying stats, there's no way to quickly look up accurate WoW Classic item data. The spell side already has this workflow via `lookup_spell`, `get_spell_icon`, and `list_known_spells`. We need equivalent tools for items.

## Requirements Trace

- R1. Look up item details by Wowhead item ID — returns name, icon URL, stats (armor, stamina, strength, intellect, etc.), quality, item level, slot, and tooltip text
- R2. Look up item details by name — resolves name to ID via known items database, then fetches details
- R3. Get item icon URL by name or ID — for downloading equipment icons to `assets/icons/`
- R4. List known items, optionally filtered by slot or armor type
- R5. Parse item stats from Wowhead tooltip HTML (since the item API only returns structured name/icon/quality — stats are in HTML)

## Scope Boundaries

- **In scope:** New MCP tools for item lookup, icon retrieval, and listing; known items database; HTML tooltip stat parsing
- **Out of scope:** Automatically updating `items.ron` from Wowhead data; downloading icons automatically; item search by arbitrary text against Wowhead search API (known-items lookup is sufficient for now); Rust-side changes

## Context & Research

### Relevant Code and Patterns

- Existing MCP server: `tools/wowhead-mcp/src/index.ts` — 4 spell tools using `McpServer` from `@modelcontextprotocol/sdk`, Zod schemas, tooltip API at `nether.wowhead.com/classic/tooltip/spell/{id}`
- Spell tools pattern: `lookup_spell_by_id`, `lookup_spell`, `get_spell_icon`, `list_known_spells` — each registered via `server.tool()` with Zod schema and async handler
- Known spells database: `KNOWN_SPELLS` record mapping lowercase name → spell ID
- Item tooltip API: `https://nether.wowhead.com/classic/tooltip/item/{id}` returns `{ name, quality, icon, tooltip (HTML), spells[] }`
- Wowhead search API: `https://www.wowhead.com/classic/search/suggestions-template?q=...` returns `{ results: [{ type, id, name, icon, quality }] }` where type=3 is Item
- Current items: `assets/config/items.ron` — 40+ items across plate, mail, leather, cloth armor sets plus weapons, cloaks, jewelry, trinkets
- Icon infrastructure: `assets/icons/abilities/` holds `.jpg` files named by Wowhead icon name (e.g., `spell_frost_frostbolt02.jpg`)

### Key API Difference: Items vs Spells

The spell tooltip API returns some structured fields that can be parsed from clean text. The item tooltip API returns stats **only inside HTML markup** in the `tooltip` field. The structured response only includes `name`, `quality`, `icon`, and `spells[]`. This means item stat extraction requires HTML parsing — specifically looking for patterns like `<span><!--stat5-->+12 Stamina</span>` and damage/speed values in the tooltip HTML.

## Key Technical Decisions

- **Parse stats from HTML tooltip rather than requiring a scraping library:** The tooltip HTML follows consistent Wowhead patterns. Regex-based extraction (matching the existing spell tooltip approach) is sufficient and avoids adding heavy dependencies like cheerio. The existing `stripHtml()` and regex parsing functions establish this pattern.
- **Use a `KNOWN_ITEMS` database mirroring `KNOWN_SPELLS`:** Populate with items matching those in `items.ron` so the MCP is immediately useful for our existing equipment set. This mirrors the spell lookup pattern exactly.
- **Icon URL follows same base URL pattern:** Item icons use the same `wow.zamimg.com/images/wow/icons/large/{icon}.jpg` URL pattern as spell icons. No new URL infrastructure needed.
- **Wowhead item page URL pattern:** `https://www.wowhead.com/classic/item={id}` (differs from spell URL pattern `spell={id}`)

## Open Questions

### Resolved During Planning

- **Q: Does the item tooltip API exist?** Yes — `nether.wowhead.com/classic/tooltip/item/{id}` confirmed working. Returns name, quality, icon, and HTML tooltip.
- **Q: Are item icons at the same CDN path as spell icons?** Yes — same `wow.zamimg.com/images/wow/icons/large/{icon}.jpg` pattern.
- **Q: How are stats encoded in item tooltips?** In HTML spans with stat IDs (e.g., `<!--stat5-->` for stamina). Damage/speed are in separate table cells.

### Deferred to Implementation

- **Exact regex patterns for all stat types:** Will be refined by testing against real Wowhead tooltip HTML during implementation. The broad patterns are known but edge cases (set bonuses, on-use effects, resistance stats) will emerge from real data.

## High-Level Technical Design

> *This illustrates the intended approach and is directional guidance for review, not implementation specification. The implementing agent should treat it as context, not code to reproduce.*

```
Wowhead Item Tooltip API Response:
  { name, quality, icon, tooltip: "<table>...<span>+12 Stamina</span>..." }
                    │
                    ▼
  ┌─────────────────────────────┐
  │  parseItemStats(tooltip)    │ ← New function, mirrors spell parsing approach
  │  - parseArmor()             │
  │  - parseItemDamage()        │
  │  - parseItemSpeed()         │
  │  - parseBonusStats()        │   (stamina, intellect, strength, agility, spirit,
  │  - parseItemLevel()         │    spell power, attack power, crit, hit)
  │  - parseItemSlot()          │
  │  - parseArmorType()         │
  │  - parseRequiredLevel()     │
  └─────────────────────────────┘
                    │
                    ▼
  Tool outputs: lookup_item_by_id, lookup_item, get_item_icon, list_known_items
  (same registration pattern as spell tools)
```

## Implementation Units

- [x] **Unit 1: Known items database and item fetch function**

**Goal:** Add `KNOWN_ITEMS` record and `fetchItemById()` function with HTML stat parsing

**Requirements:** R1, R2, R5

**Dependencies:** None

**Files:**
- Modify: `tools/wowhead-mcp/src/index.ts`

**Approach:**
- Add `KNOWN_ITEMS` record mapping lowercase item names to Wowhead item IDs. Populate with items from `items.ron` — look up each item's actual Wowhead ID. Organize by category (plate, mail, leather, cloth, weapons, accessories) mirroring the spell database's class groupings.
- Add `ITEM_TOOLTIP_API` constant: `https://nether.wowhead.com/classic/tooltip/item`
- Add `WOWHEAD_ITEM_URL` constant: `https://www.wowhead.com/classic/item`
- Add `fetchItemById()` async function following `fetchSpellById()` pattern — fetch from tooltip API, parse HTML tooltip for stats
- Add stat parsing functions: `parseArmor()`, `parseItemDamage()`, `parseItemSpeed()`, `parseBonusStats()` (stamina, intellect, strength, agility, spirit, spell power, attack power, crit, hit), `parseItemLevel()`, `parseItemSlot()`, `parseArmorType()`, `parseRequiredLevel()`
- Add `findItemId()` function mirroring `findSpellId()` for name lookups
- Quality number mapping: 0=Poor(grey), 1=Common(white), 2=Uncommon(green), 3=Rare(blue), 4=Epic(purple), 5=Legendary(orange)

**Patterns to follow:**
- `fetchSpellById()` function structure and return type pattern
- `KNOWN_SPELLS` record organization
- Existing regex parsing functions (`parseCastTime`, `parseDamage`, etc.)

**Test scenarios:**
- Happy path: Fetch a known item by ID, verify all stat fields are populated from tooltip HTML
- Happy path: Name lookup resolves "arcanite reaper" → correct item ID
- Edge case: Partial name match finds the right item
- Edge case: Item with no bonus stats (plain white item) returns empty stats object
- Error path: Invalid item ID returns null
- Error path: Unknown item name returns null from `findItemId()`

**Verification:**
- `fetchItemById()` returns structured data for known Classic items
- `findItemId()` resolves all items in `KNOWN_ITEMS`

- [x] **Unit 2: Register item lookup tools**

**Goal:** Add `lookup_item_by_id`, `lookup_item`, `get_item_icon`, and `list_known_items` MCP tools

**Requirements:** R1, R2, R3, R4

**Dependencies:** Unit 1

**Files:**
- Modify: `tools/wowhead-mcp/src/index.ts`

**Approach:**
- Register `lookup_item_by_id` tool — takes `itemId: number`, returns formatted item details with stats, icon URL, Wowhead URL, quality, slot, and tooltip. Output format mirrors `lookup_spell_by_id`.
- Register `lookup_item` tool — takes `itemName: string`, resolves via `findItemId()`, fetches details. Includes "did you mean" suggestions on miss.
- Register `get_item_icon` tool — takes `itemIdOrName: string`, returns icon name, icon URL, and Wowhead URL. Mirrors `get_spell_icon`.
- Register `list_known_items` tool — takes optional `slotFilter` and `typeFilter` strings, lists items grouped by category. Mirrors `list_known_spells`.
- Item groups for listing: organize by armor type (Plate, Mail, Leather, Cloth) and category (Weapons, Cloaks, Jewelry, Trinkets)

**Patterns to follow:**
- Exact tool registration pattern from `lookup_spell_by_id`, `lookup_spell`, `get_spell_icon`, `list_known_spells`
- Zod schema definitions matching spell tool patterns
- Output markdown formatting matching spell tool output

**Test scenarios:**
- Happy path: `lookup_item_by_id` with valid ID returns formatted stats and icon URL
- Happy path: `lookup_item` with "Arcanite Reaper" returns full item details
- Happy path: `get_item_icon` returns icon name and downloadable URL
- Happy path: `list_known_items` with no filter returns all items grouped by category
- Happy path: `list_known_items` with `slotFilter: "Head"` returns only head slot items
- Happy path: `list_known_items` with `typeFilter: "Plate"` returns only plate items
- Edge case: `lookup_item` with unknown name shows suggestions
- Error path: `lookup_item_by_id` with invalid ID returns "not found" message
- Error path: `get_item_icon` with unknown item returns "not found" message

**Verification:**
- All four tools are registered and callable via MCP
- Output format is consistent with existing spell tools
- Build succeeds: `cd tools/wowhead-mcp && npm run build`

- [x] **Unit 3: Build, test, and update server description**

**Goal:** Verify the MCP server builds, update package description, and validate tools work end-to-end

**Requirements:** R1, R2, R3, R4

**Dependencies:** Unit 2

**Files:**
- Modify: `tools/wowhead-mcp/package.json` (update description)
- Modify: `tools/wowhead-mcp/src/index.ts` (update server description comment)

**Approach:**
- Update package.json description to mention items alongside spells
- Update the file-level JSDoc comment to mention equipment/item lookup
- Build the TypeScript: `npm run build`
- Verify by checking the compiled output exists

**Test scenarios:**
- Happy path: `npm run build` succeeds with no TypeScript errors
- Integration: Start the server and verify all 8 tools (4 spell + 4 item) are registered

**Verification:**
- `npm run build` succeeds cleanly
- No TypeScript compilation errors
- Server starts without errors

## System-Wide Impact

- **Interaction graph:** The MCP server is a standalone process — changes don't affect Rust code. Claude Code connects to it via stdio transport. Adding tools extends the MCP tool list visible to Claude.
- **Error propagation:** Wowhead API failures return user-friendly "not found" messages, matching spell tool behavior. No crashes on bad data.
- **State lifecycle risks:** None — the MCP server is stateless.
- **API surface parity:** After this change, items and spells have symmetric tool sets: `lookup_*_by_id`, `lookup_*`, `get_*_icon`, `list_known_*`.
- **Unchanged invariants:** All existing spell tools remain identical. No changes to Rust code, items.ron, or the equipment system.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Wowhead tooltip HTML format changes break stat parsing | Regex patterns are resilient to minor HTML changes; return "Unknown" for unparseable fields rather than crashing |
| Some items may not exist on Wowhead Classic (custom items in items.ron) | Known items database only includes real Classic items; custom items gracefully return "not found" |
| Large KNOWN_ITEMS database bloats the file | Items organized by category with clear sections; same pattern as KNOWN_SPELLS which works well at ~100 entries |

## Sources & References

- Wowhead item tooltip API: `https://nether.wowhead.com/classic/tooltip/item/{id}`
- Wowhead search API: `https://www.wowhead.com/classic/search/suggestions-template?q=...`
- Existing MCP server: `tools/wowhead-mcp/src/index.ts`
- Item definitions: `assets/config/items.ron`
- Icon CDN: `https://wow.zamimg.com/images/wow/icons/large/{icon}.jpg`
