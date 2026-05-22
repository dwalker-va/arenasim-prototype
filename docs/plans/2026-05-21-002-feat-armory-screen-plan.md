---
title: "feat: Add Armory Screen"
type: feat
status: active
date: 2026-05-21
origin: docs/brainstorms/armory-screen-requirements.md
---

# feat: Add Armory Screen

## Summary

Add a new player-facing `GameState::Armory` reachable from the main menu. The screen reads the existing `ItemDefinitions` resource and renders every item as a uniform-frame tile in a wrapping grid, with a horizontal chip-bar of filters (Slot, Armor Type, Item Level range, Name search) and hover-tooltips for stat details. The implementation mirrors the established `bevy_egui` pattern in `src/states/configure_match_ui.rs` and reuses the `ItemIcons` resource already loaded by `src/states/view_combatant_ui.rs`.

---

## Problem Frame

ArenaSim currently has no surface that presents the full item catalog to a player — equipment can only be inspected through `GameState::ViewCombatant` (single character's loadout) or by reading `assets/config/items.ron`. The main menu offers MATCH / OPTIONS / EXIT with no entry point into the game's content. See origin: `docs/brainstorms/armory-screen-requirements.md`.

---

## Assumptions

*This plan was authored in LFG pipeline mode without synchronous user confirmation. The items below are agent inferences that fill gaps in the input — un-validated bets that should be reviewed before implementation proceeds.*

- **File layout:** the new screen lives as a single sibling file at `src/states/armory_ui.rs`, matching `configure_match_ui.rs` and `view_combatant_ui.rs`. The alternative — a `src/states/armory/` submodule directory — was rejected as premature for a single-screen feature.
- **Icon resource reuse:** the armory reads `view_combatant_ui::ItemIcons` directly rather than introducing a parallel resource. The existing `load_item_icons` system is added to the Armory state's system chain; the resource's internal `loaded: bool` guard makes re-running it from a second state safe and idempotent.
- **Filter state persistence:** an `ArmoryFilters` resource persists across `MainMenu ↔ Armory` transitions within a session. It is initialized at app startup and never reset on enter; the user can leave and come back without losing their filter selection. Reset across game launches happens naturally because it's a non-serialized resource.
- **Sort is render-time, not pre-computed:** the default sort (slot ascending, then `item_level` descending) is applied every frame by collecting filter-matched items into a `Vec` and sorting. With 128 items this is trivial; no need for a cached sorted index.
- **Filter combinator:** within an axis (e.g., multi-selected slots) the filter is OR; across axes the combinator is AND. This matches the brainstorm's R9.
- **`Ring1`/`Ring2` and `Trinket1`/`Trinket2` are presented as single `Ring` and `Trinket` chips** to avoid duplicate-looking filters; the underlying filter logic matches against both variants.
- **Keyboard navigation is out of scope.** The screen is mouse-driven, matching the WoW Classic UI tradition the visual style references. The search `TextEdit` accepts keyboard focus naturally; chips and tiles do not need explicit tab order.
- **No item-name label on tiles.** Tooltip surfaces the name on hover (R12). Tiles render only icon + iLvl badge, keeping the wall visually uniform; name-text labels would force truncation logic and break the trophy-wall texture goal.

---

## Requirements

Traced to origin requirements doc (`R<N>` IDs preserved):

- R1. Main menu shows an "ARMORY" button between MATCH and OPTIONS; clicking transitions to `GameState::Armory`.
- R2. Armory screen has a back-to-menu affordance returning to `MainMenu`.
- R3. The grid renders every item from `ItemDefinitions` as uniform-size tiles, no per-slot or per-type grouping in the layout.
- R4. Each tile shows the item's icon and the `item_level` as a small numeric badge.
- R5. Tile frames are visually identical — no rarity-color borders, no armor-type tinting.
- R6. The grid is vertically scrollable and wraps based on available width.
- R7. Default sort: by slot, then `item_level` descending within slot.
- R8. A horizontal chip-bar above the grid exposes filters for Slot, Armor Type, Item Level range, and free-text Name search.
- R9. Filters multi-select within an axis (OR) and combine across axes (AND).
- R10. Live count displayed as `N / <total> items`.
- R11. When zero items match, an empty-state message replaces the grid.
- R12. Hover tooltip shows: item name, item level, slot, armor type (if not `None`), and full stat block.
- R13. Tooltip omits zero/missing stats; formatting matches `view_combatant_ui` conventions.
- R14. No click-to-spotlight detail panel — hover only.
- R15. Visual styling matches existing menus (dark `rgb(20,20,30)` background; gold-accent typography `rgb(230,204,153)` / `rgb(230,217,191)`).
- R16. Item icons load via the same `bevy_egui` texture-registration pattern as `configure_match_ui::load_class_icons` / `view_combatant_ui::load_item_icons`.

---

## Scope Boundaries

Carried verbatim from origin (Scope Boundaries):

- Item rarity/quality tiers — no `quality` field is added to `items.ron`; tiles stay visually flat.
- Class-restriction filter — the data model has `allowed_classes: Option<Vec<CharacterClass>>` on `ItemConfig`, but no current item uses it, and the armory deliberately does not filter on it.
- Designer-mode toggle — no item-level-budget %, stat-per-point, or balance-audit affordances.
- Loadout editing or item equipping — armory is read-only.
- Click-to-spotlight detail panel.
- Side-by-side item comparison.
- Favorites / pinning / persisted UI state across sessions.
- Item lore / flavor text — not in `items.ron`, not added here.

---

## Context & Research

### Relevant Code and Patterns

- `src/states/mod.rs`:
  - `GameState` enum (lines 17–34) — new `Armory` variant inserts here.
  - `StatesPlugin::build()` (lines 42–96) — registration site for the armory's `Update` systems and resources.
  - `main_menu_ui` (line 309) — three-button vertical stack with `button_size = vec2(280.0, 60.0)`, gold-on-dark egui styling. New ARMORY button inserts between MATCH and OPTIONS.
- `src/states/configure_match_ui.rs`:
  - `ClassIcons` / `ClassIconHandles` resources (around line 29) — exact pattern for icon loading.
  - `load_class_icons` system — three-phase init (queue handles → wait for `Assets<Image>` to contain them → register with `EguiContexts::add_image`).
  - `configure_match_ui` UI function — egui dark-theme setup, `CentralPanel`, `vertical_centered`.
- `src/states/view_combatant_ui.rs`:
  - `ItemIcons` / `ItemIconHandles` (around line 56) — `HashMap<ItemId, egui::TextureId>` keyed by `ItemId`. Already loads every item icon listed in `ItemDefinitions`. **Direct reuse target.**
  - `load_item_icons` (line 402) — pulls from `Res<ItemDefinitions>` and iterates `item.icon` paths.
  - Stat formatting helpers within the view fn — reference for tooltip stat block.
- `src/states/play_match/equipment.rs`:
  - `ItemSlot` enum (line 27) — 17 slots including `Trinket2`.
  - `ArmorType` enum (line 100) — `Cloth`, `Leather`, `Mail`, `Plate`, `None`.
  - `ItemConfig` struct (line ~160) — stat fields (`max_health`, `max_mana`, `mana_regen`, `attack_power`, `spell_power`, `crit_chance`, `movement_speed`, `armor`, `*_resistance`, `attack_damage_min/max`, `attack_speed`).
  - `ItemDefinitions::iter()` — iterates `(ItemId, &ItemConfig)`.
  - `EquipmentPlugin::build()` (line ~696) — inserts `ItemDefinitions` as a resource at startup; no extra wiring needed.
- `tests/registration_audit.rs` — enforces system registration; UI-only systems for a new state register via `StatesPlugin::build()` (path 2 of 3).

### Institutional Learnings

- `docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md` — historical bug from registering systems in only one of `add_core_combat_systems` vs `StatesPlugin::build()`. Not relevant here (armory is UI-only, graphical-only) but the audit catches the failure mode regardless.

### External References

- None — local pattern coverage is strong (three sibling egui menu screens), no external research warranted.

---

## Key Technical Decisions

- **Reuse `view_combatant_ui::ItemIcons` rather than introducing `ArmoryIcons`.** The resource is already keyed by `ItemId` and already loads every item with a non-empty `icon` field. Duplicating it would mean double the GPU textures and double the load wait. The system's `if item_icons.loaded { return; }` short-circuit makes it safe to register in two states.
- **Single-file module, not a submodule directory.** The armory is one screen with one Update system, one filter resource, and ~3 helper fns (tile render, tooltip, chip-bar). A directory would be premature structure; `configure_match_ui.rs` (which is more complex) is also a single file.
- **`Ring1`/`Ring2` and `Trinket1`/`Trinket2` collapsed in the UI.** The slot-chip set in the filter bar shows `Ring` and `Trinket` as single chips. The filter predicate matches both numeric variants. This avoids the UX surprise of seeing two identical-looking chips for what users perceive as one slot kind.
- **Default sort key: `(slot_order, -item_level, name)`.** Slot order matches the canonical `ItemSlot::all()` ordering from `equipment.rs`. Tie-breaker by name keeps ordering stable across runs.
- **Wrapping grid via egui's `ui.horizontal_wrapped`.** Each row is filled left-to-right with tiles until the next tile would overflow; then a new row starts. No manual column math; egui handles width-responsive wrapping.
- **Empty-state copy** ("No items match these filters.") rendered centered in the grid area, in the same low-contrast gray (`rgb(102,102,102)`) used for the menu version label.

---

## Open Questions

### Resolved During Planning

- *Should the armory have its own icon resource?* — No, reuse `ItemIcons`. (See Key Technical Decisions.)
- *Should `Ring1`/`Ring2` show as separate chips?* — No, collapse. (See Key Technical Decisions.)
- *Where does the `Armory` system register?* — `StatesPlugin::build()`, alongside other UI-only state systems. The registration audit treats this as the correct path for a non-combat UI system.
- *Item Level filter widget choice?* — Two `egui::DragValue` widgets (min and max), bounds 0..=100. `egui` has no built-in range slider; dual `DragValue` is the standard idiom and uses minimal horizontal real estate.
- *Keyboard navigation?* — Out of scope. The screen is mouse-driven, matching the WoW Classic UI tradition. `TextEdit` accepts keyboard focus naturally.
- *Item name on tile?* — No name label on the tile itself; tooltip carries the name on hover.
- *Clear-filters affordance?* — One inline button at the end of chip-bar row 2, plus a secondary button beneath the empty-state copy. Both reset `ArmoryFilters` to `Default::default()`.

### Deferred to Implementation

- *Exact tile size in pixels.* The brainstorm calls for "uniform tiles" but doesn't fix the size. Start with 64×64 icon + iLvl badge → roughly 76×76 tile (no name label per Resolved above), adjust by feel during the UI pass.
- *Active vs inactive chip visual treatment.* egui's `SelectableLabel` carries native selected styling; if that reads clearly against the dark theme, use it. If not, swap to manual `Frame` with `fill: Color32::from_rgb(230, 204, 153)` + dark text when selected, transparent + gold text when unselected. Decide at U4 implementation.
- *Tile hover treatment.* egui's default frame brightening on hover may be sufficient. If tiles look "dead" at rest, add an explicit hover treatment in U5 (border-color shift, or 8% lightness bump on the frame fill).
- *Back button placement vs. centered title.* Header layout has two reasonable options: (a) three-column row with back button left, spacer, title centered, spacer; (b) back button absolute top-left, title `vertical_centered`. Pick whichever lays out cleaner on first build; both are visually acceptable.

---

## Implementation Units

### U1. Add `GameState::Armory` variant and wire main menu navigation

**Goal:** Establish the new state, register a placeholder Update system, and surface an "ARMORY" button on the main menu that transitions into it.

**Requirements:** R1, R2

**Dependencies:** None

**Files:**
- Modify: `src/states/mod.rs`
- (No test file — covered by `tests/registration_audit.rs` plus manual smoke.)

**Approach:**
- Add `Armory` variant to the `GameState` enum, after `Results` (preserves source order; no behavioral significance).
- Insert ARMORY button in `main_menu_ui` between the MATCH and OPTIONS buttons, mirroring the existing button styling (`button_size = vec2(280.0, 60.0)`, gold-on-dark `RichText`, `info!` log on click).
- Add a new module declaration `pub mod armory_ui;` near the top of `src/states/mod.rs`.
- In `StatesPlugin::build()`, add an `.init_resource::<armory_ui::ArmoryFilters>()` call alongside the other resource initializations, and add an `.add_systems(Update, (...).chain().run_if(in_state(GameState::Armory)))` block for the armory's system chain. The chain at this U-ID is `(view_combatant_ui::load_item_icons, armory_ui::armory_ui)` — the loader is reused, not duplicated.

**Patterns to follow:**
- Existing ConfigureMatch / Options system registration blocks in `StatesPlugin::build()`.
- Existing button styling in `main_menu_ui`.

**Test scenarios:**
- Integration: `cargo test` — the `registration_audit` test must still pass after introducing the new `pub fn armory_ui` system (it should, because the system is registered in `StatesPlugin::build()`).
- Manual: launching the graphical client shows three menu buttons in order MATCH / ARMORY / OPTIONS; clicking ARMORY logs the transition and switches state; back button returns to menu.

**Verification:**
- `cargo build --release` succeeds.
- `cargo test` passes (in particular `registration_audit`).
- Manual launch shows the new button between MATCH and OPTIONS.

---

### U2. Create `armory_ui.rs` with module skeleton, filter resource, header, and back button

**Goal:** Stand up the armory screen file with: module imports, `ArmoryFilters` resource definition, the `armory_ui` Update system rendering a header (title + back button) and an empty content area. The screen loads without panicking; back button returns to MainMenu.

**Requirements:** R2, R15

**Dependencies:** U1

**Files:**
- Create: `src/states/armory_ui.rs`

**Approach:**
- Define `#[derive(Resource, Default)] pub struct ArmoryFilters { selected_slots: HashSet<ItemSlot>, selected_armor_types: HashSet<ArmorType>, item_level_min: u32, item_level_max: u32, name_search: String, }`. Initial defaults: empty sets, `item_level_min: 0`, `item_level_max: u32::MAX`, empty string.
- Provide a helper `ArmoryFilters::matches(&self, item: &ItemConfig) -> bool` for use in the filter logic later (returns `true` until U4 implements the predicate; U2 stubs it as `true`).
- `pub fn armory_ui(...)` system signature: `EguiContexts`, `ResMut<NextState<GameState>>`, `ResMut<ArmoryFilters>`, `Res<ItemDefinitions>`, `Option<Res<view_combatant_ui::ItemIcons>>`.
- Apply the same dark-theme egui styling as `main_menu_ui` (`window_fill`, `panel_fill` = `Color32::from_rgb(20, 20, 30)`).
- Render header: title "ARMORY" in gold (`rgb(230, 204, 153)`, size ~48), a "← Back" button on the left that calls `next_state.set(GameState::MainMenu)`.
- Below the header, render a placeholder `ui.label("Loading...")` if `item_icons` is None or `!item_icons.loaded`; otherwise render the empty content area.

**Patterns to follow:**
- `main_menu_ui` — for theme setup, RichText sizing, button styling, `CentralPanel` usage.
- `configure_match_ui::configure_match_ui` — for `Res<ItemDefinitions>` access patterns and Option-based icon-resource handling.

**Test scenarios:**
- Manual: clicking ARMORY from the main menu opens a dark screen with a gold "ARMORY" title and a "← Back" button. Clicking back returns to MainMenu. No panic, no warning logs.

**Verification:**
- The screen renders with title and back button.
- Back button works and state transitions cleanly.
- `cargo build --release` and `cargo test` both pass.

---

### U3. Render the item tile grid

**Goal:** Render every item from `ItemDefinitions` as a uniform-size tile in a width-responsive wrapping grid. Each tile shows the item icon (when loaded) and an item-level badge. Tiles are visually identical regardless of armor type or item level.

**Requirements:** R3, R4, R5, R6, R7, R16

**Dependencies:** U2

**Files:**
- Modify: `src/states/armory_ui.rs`

**Approach:**
- Collect items: `let mut items: Vec<(&ItemId, &ItemConfig)> = item_defs.iter().collect();`.
- Sort with key `(slot_order(item.slot), Reverse(item.item_level), item.name.as_str())`. `slot_order` is a small helper mapping `ItemSlot` to its position in `ItemSlot::all()`.
- Wrap in `egui::ScrollArea::vertical()` followed by `ui.horizontal_wrapped(|ui| { for (id, item) in items { tile_ui(ui, id, item, &item_icons); } })`.
- `tile_ui` helper: fixed allocate (e.g., `ui.allocate_ui(vec2(76.0, 76.0), |ui| {...})`), render icon as `egui::Image::new((texture_id, vec2(64.0, 64.0)))` when available, with a small text badge overlay showing `item.item_level`. Fall back to a colored rectangle placeholder if the icon isn't loaded yet. No item-name label on the tile (see Key Technical Decisions).
- Frame styling: uniform `egui::Frame` with thin border, identical for all tiles. No conditional color from armor type or item level. Hover treatment: rely on egui's default frame response brightening; if tiles read as static in playtest, add an explicit hover lightening of the frame fill in U5.

**Patterns to follow:**
- `view_combatant_ui` for `egui::Image::new((texture_id, size))` usage.
- `egui::ScrollArea` + `ui.horizontal_wrapped` pairing — standard egui idiom for responsive grids.

**Test scenarios:**
- Manual: all 128+ items render as tiles. Tiles wrap to multiple rows. Vertical scrolling works. Item-level badge visible. Default sort: all Head items appear first, sorted by iLvl desc within slot.
- Manual: launching the armory immediately after game start (before any combatant view) shows tiles populating once `load_item_icons` finishes — placeholder shape visible briefly, real icons replace them.
- Edge case: if any item has empty `icon: ""`, tile renders the placeholder rectangle without crashing.

**Verification:**
- Wall scrolls; all items visible.
- Default sort produces a "all helms, then all chests, ..." reading.
- Resizing the window re-wraps the grid.

---

### U4. Implement the filter chip-bar, live count, and empty state

**Goal:** Add the filter UI above the grid (Slot chips, Armor Type chips, Item Level range, Name search) and live filtering of the rendered tiles. Show `N / <total> items` count. Show empty state when the filtered set is zero.

**Requirements:** R8, R9, R10, R11

**Dependencies:** U3

**Files:**
- Modify: `src/states/armory_ui.rs`

**Approach:**
- Replace the U2 stub `ArmoryFilters::matches` with the real predicate:
  - Slot match: `selected_slots.is_empty() || selected_slots.contains(&item.slot)`. For UI presentation, `Ring1`/`Ring2` are collapsed to a "Ring" chip in the chip-bar (toggling the chip toggles both variants in the set); same for `Trinket1`/`Trinket2`.
  - Armor type match: `selected_armor_types.is_empty() || selected_armor_types.contains(&item.armor_type)`.
  - Item level: `item.item_level >= item_level_min && item.item_level <= item_level_max`.
  - Name: case-insensitive `item.name.to_lowercase().contains(&search.to_lowercase())` (skip if empty).
- Render chip-bar between header and grid as **two stacked rows** (one `ui.horizontal_wrapped` block won't reliably preserve groupings, and a single horizontal row overflows):
  - **Row 1 — Slots:** label `"SLOT:"` followed by toggle-button chips for each logical slot kind (Head, Neck, Shoulders, Back, Chest, Wrists, Hands, Waist, Legs, Feet, Ring, Trinket, MainHand, OffHand, Ranged).
  - **Row 2 — Type / Range / Search / Count / Clear:** label `"TYPE:"` + Armor Type chips (Cloth, Leather, Mail, Plate, None), then a separator, then iLvl range as two `egui::DragValue` widgets labelled `"iLvl"` with min/max bounds 0..=100 (egui has no built-in `RangeSlider`; dual `DragValue` is the chosen widget — see Resolved During Planning), then `egui::TextEdit::singleline().hint_text("Search...")`, then the live count `format!("{} / {} items", filtered.len(), item_defs.item_count())` right-aligned, then a `"Clear filters"` button that resets `ArmoryFilters` to `Default::default()`.
- Empty state: when `filtered.is_empty()`, render `ui.colored_label(Color32::from_rgb(102, 102, 102), "No items match these filters.")` centered, **with a secondary `"Clear filters"` button below it** so the user has an in-place escape from the empty state.

**Patterns to follow:**
- `configure_match_ui` for egui button/text-edit usage and modal layout.

**Test scenarios:**
- Happy path: selecting "Plate" + "Chest" narrows to only plate chest pieces; count updates live.
- Happy path: typing "Lion" in the search filters the grid to only items whose name contains "lion" (case-insensitive).
- Happy path: setting iLvl range to 55..60 hides items outside that band.
- Edge case: selecting one Slot chip and one Armor Type chip applies AND across axes; selecting two Slot chips applies OR within the axis.
- Edge case: filter combination that matches zero items shows the empty-state message; clicking either the chip-bar's "Clear filters" button or the empty-state's "Clear filters" button restores the full grid.
- Edge case: search string with leading/trailing whitespace is trimmed before matching (or at least doesn't break matching).
- Edge case: the "Ring" chip filters in items with `slot: Ring1` OR `slot: Ring2`.

**Verification:**
- Each filter axis individually narrows results.
- Combined filters compose with AND across axes, OR within.
- Live count matches the rendered tile count.
- Empty state renders when no items match.

---

### U5. Hover tooltip with item stats

**Goal:** Hovering a tile presents a tooltip with item name, item level, slot, armor type (when not `None`), and the item's non-zero stats.

**Requirements:** R12, R13, R14

**Dependencies:** U3

**Files:**
- Modify: `src/states/armory_ui.rs`

**Approach:**
- In `tile_ui`, capture the `Response` from `ui.allocate_ui(...)` (or from the `egui::Image`'s response if interactive) and call `.on_hover_ui(|ui| { render_tooltip(ui, item); })`.
- `render_tooltip` formats:
  - Item name as heading (gold accent).
  - One line: `iLvl {item_level}  ·  {slot_name}  ·  {armor_type_name}` (omit armor type when `ArmorType::None`).
  - Stats block, one line per non-zero stat. Format examples: `+15 Stamina (Max Health)`, `+8 Attack Power`, `+2% Crit Chance`, `+10% Movement Speed`, `300 Armor`. For weapons: `Damage: {min}-{max}, Speed: {attack_speed:.1}`.
  - Resistance stats grouped on one line when multiple are non-zero (`+10 Fire / +5 Frost Resist`); single resistance gets its own line.
- All formatting helpers private to `armory_ui.rs`. **Do not** reach into `view_combatant_ui` for them — that file is a megafunction; pull the formatting logic into a small local helper instead. If duplication grows, extract to a shared `stat_format.rs` module in a follow-up.

**Patterns to follow:**
- `egui::Response::on_hover_ui` — standard egui tooltip pattern.

**Test scenarios:**
- Happy path: hovering an armor tile shows name, iLvl, slot, armor type, and stat lines for every non-zero stat.
- Happy path: hovering a weapon tile shows damage range and attack speed.
- Happy path: hovering an accessory (ring/neck/trinket) omits the armor type line.
- Edge case: hovering an item with zero stats (hypothetical) renders the header lines only, no stat block — does not crash.
- Edge case: tooltip doesn't appear when filters hide the item.

**Verification:**
- Tooltip appears on hover, disappears on leave.
- Only non-zero stats appear.
- Formatting matches the rest of the game's UI tone.

---

## System-Wide Impact

- **Interaction graph:** new `Update` chain `(load_item_icons, armory_ui).chain().run_if(in_state(GameState::Armory))` registered in `StatesPlugin::build()`. `load_item_icons` is now reached from two states; its existing `loaded` short-circuit keeps the second registration safe.
- **Error propagation:** none new. `ItemDefinitions` loading is `panic!` at startup (existing behavior in `EquipmentPlugin::build`); the armory inherits this. If `items.ron` is broken, the game fails to launch — same as today.
- **State lifecycle risks:** `ArmoryFilters` resource persists across state changes by design (see Assumptions). No partial-write concerns; the resource only contains UI state.
- **API surface parity:** none — this is graphical-only, headless mode is unaffected.
- **Integration coverage:** `tests/registration_audit.rs` covers the system-registration invariant. Filter and tile logic are pure UI; manual exercise during U3–U5 is the integration test.
- **Unchanged invariants:** `items.ron` schema unchanged. `ItemConfig` / `ItemSlot` / `ArmorType` enums unchanged. `view_combatant_ui::ItemIcons` resource definition and `load_item_icons` system signature unchanged — only its registration set expands.

---

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| `load_item_icons` registered in two states could attempt double-registration of egui textures if the `loaded` flag failed to short-circuit. | The existing `if item_icons.loaded { return; }` guard makes the second registration a no-op after first load. Confirmed by reading the system implementation. |
| Egui tooltip rendering inside a `horizontal_wrapped` grid may have placement quirks. | Use `on_hover_ui_at_pointer` if `on_hover_ui` placement is poor; fallback is a fixed-position panel near the grid. Resolved at implementation time. |
| Item icons not loaded on first armory open (race between state-enter and asset load). | U2's "Loading..." placeholder + U3's tile placeholder rectangle handle this without crashing. Subsequent frames swap in real icons. |
| New `pub fn armory_ui` system might trip `registration_audit.rs` if registration is forgotten. | Registration is part of U1 by construction; audit failure on `cargo test` is the safety net. |
| The collapsed `Ring`/`Trinket` chips could confuse a user who expects two chips to match two ring slots. | The chip label is "Ring" (singular), and the filter is documented in code. If user feedback flags it, splitting later is one-line change. |

---

## Documentation / Operational Notes

- No external documentation impact.
- `CLAUDE.md` and `design-docs/session-notes.md` may want a session note after merge, but that's a post-merge concern outside this plan.

---

## Sources & References

- **Origin document:** `docs/brainstorms/armory-screen-requirements.md`
- Related code:
  - `src/states/mod.rs` — GameState enum and StatesPlugin::build()
  - `src/states/configure_match_ui.rs` — egui menu pattern
  - `src/states/view_combatant_ui.rs` — ItemIcons resource and load_item_icons system
  - `src/states/play_match/equipment.rs` — ItemDefinitions, ItemConfig, ItemSlot, ArmorType
  - `tests/registration_audit.rs` — system-registration enforcement
- Related learnings: `docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md`
