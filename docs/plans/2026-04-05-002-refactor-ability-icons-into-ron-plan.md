---
title: "refactor: Move ability icon paths into abilities.ron"
type: refactor
status: completed
date: 2026-04-05
origin: docs/follow-ups/ability-icon-refactor.md
---

# refactor: Move ability icon paths into abilities.ron

## Overview

Move ability icon paths from three hardcoded locations in rendering code into the data-driven `abilities.ron` config file, mirroring the item icon refactor (commit 453360c). Adding a new ability will then only require updating `abilities.ron` instead of three separate places.

## Problem Frame

Ability icons are split across three code locations: a 55-arm match statement (`get_ability_icon_path()`), a parallel string array (`SPELL_ICON_ABILITIES`), and the ability definitions in `abilities.ron` which lack an icon field. Forgetting any one location causes silent icon loading failures. The item system already solved this exact problem.

## Requirements Trace

- R1. Add `icon` field to `AbilityConfig` struct with `#[serde(default)]`
- R2. Add icon paths to all ability entries in `abilities.ron`
- R3. Update both icon loading systems (`load_spell_icons` in rendering, `load_ability_icons` in view_combatant_ui) to read from `AbilityDefinitions`
- R4. Update `get_aura_icon_key()` to use `AbilityDefinitions` instead of `get_ability_icon_path()`
- R5. Remove `get_ability_icon_path()` and `SPELL_ICON_ABILITIES` from `rendering/mod.rs`
- R6. Add `all_abilities_have_icons` enforcement test
- R7. Update CLAUDE.md "Adding a New Ability" step 5 to reflect new single-source pattern
- R8. Remove the follow-up doc after completion

## Scope Boundaries

- Generic aura icons (`GENERIC_AURA_ICONS`) are NOT ability-specific and stay as-is
- No changes to headless mode — icon loading is graphical-only
- No changes to ability behavior or combat mechanics

## Context & Research

### Relevant Code and Patterns

- **Reference implementation**: `ItemConfig.icon` field in `equipment.rs:328-330` — `#[serde(default)]` String field
- **Item icon test**: `all_items_have_icons` in `equipment.rs:1383` — iterates definitions, asserts non-empty icon
- **Current ability icon loading (in-match)**: `load_spell_icons()` at `rendering/mod.rs:182` — iterates `SPELL_ICON_ABILITIES`, calls `get_ability_icon_path()`
- **Current ability icon loading (view screen)**: `load_ability_icons()` at `view_combatant_ui.rs:327` — dynamically collects class abilities, calls `get_ability_icon_path()`
- **Aura icon resolution**: `get_aura_icon_key()` at `rendering/mod.rs:134` — calls `get_ability_icon_path()` to check if an ability has a specific icon

### Institutional Learnings

- The Paladin implementation pattern doc explicitly calls out "Single Source of Truth" as a design principle — this refactor fulfills that
- The doc's code review checklist references `get_ability_icon_path()` — will need updating

## Key Technical Decisions

- **Use `String` not `Option<String>` for icon field**: Mirrors `ItemConfig.icon` pattern. `#[serde(default)]` gives empty string, and the enforcement test catches missing icons. Simpler than Option.
- **Add `iter()` method to `AbilityDefinitions`**: Needed for icon loaders to iterate all abilities. Mirrors `ItemDefinitions::iter()` pattern.
- **`get_aura_icon_key()` will take `&AbilityDefinitions` parameter**: Instead of calling the removed `get_ability_icon_path()`, it looks up the ability by iterating definitions to find a name match. This is called infrequently (once per aura display) so O(n) scan is fine.
- **Name aliases collapse naturally**: The current match handles `"Shadowbolt" | "Shadow Bolt"` and `"Web" | "Spider Web"`. Since the refactored loaders iterate by `AbilityType` key (not string name), these aliases are no longer needed. `get_aura_icon_key()` matches against `config.name` which is the canonical name.

## Open Questions

### Resolved During Planning

- **How to handle `get_aura_icon_key()`?** → It takes `&Aura` which has `ability_name: String`. After refactor, iterate `AbilityDefinitions` to find a matching name and check its icon. The function already falls back to generic aura icons, so an empty icon field triggers the fallback naturally.
- **Does `load_spell_icons` need `Res<AbilityDefinitions>`?** → Yes, add it as a system parameter. The resource is already loaded by `AbilityConfigPlugin` at startup.

### Deferred to Implementation

- **Exact icon path strings**: Will be transcribed from the current `get_ability_icon_path()` match arms during implementation. Mapping is 1:1.

## Implementation Units

- [ ] **Unit 1: Add icon field to AbilityConfig and abilities.ron**

  **Goal:** Make ability definitions carry their own icon paths

  **Requirements:** R1, R2

  **Dependencies:** None

  **Files:**
  - Modify: `src/states/play_match/ability_config.rs`
  - Modify: `assets/config/abilities.ron`

  **Approach:**
  - Add `#[serde(default)] pub icon: String` field to `AbilityConfig` after `name`
  - Add `icon` field initialization to test helper `AbilityConfig` structs
  - Add `pub fn iter(&self) -> impl Iterator<Item = (&AbilityType, &AbilityConfig)>` to `AbilityDefinitions`
  - Transcribe icon paths from `get_ability_icon_path()` match arms into each ability entry in `abilities.ron`
  - For abilities sharing icons (e.g., HolyShock variants mapped through HolyShock's icon), use the same path

  **Patterns to follow:**
  - `ItemConfig.icon` field at `equipment.rs:328-330`
  - `ItemDefinitions::iter()` at `equipment.rs:595`

  **Test scenarios:**
  - Happy path: abilities.ron parses successfully with icon fields
  - Happy path: `AbilityDefinitions::iter()` yields all ability entries

  **Verification:**
  - `cargo test` passes (existing ability config tests still work)
  - `cargo build --release` compiles

- [ ] **Unit 2: Update icon loading systems and remove hardcoded constants**

  **Goal:** Both icon loaders read from `AbilityDefinitions`; remove `get_ability_icon_path()` and `SPELL_ICON_ABILITIES`

  **Requirements:** R3, R4, R5

  **Dependencies:** Unit 1

  **Files:**
  - Modify: `src/states/play_match/rendering/mod.rs`
  - Modify: `src/states/view_combatant_ui.rs`

  **Approach:**
  - **`load_spell_icons()`**: Add `ability_definitions: Res<AbilityDefinitions>` parameter. Replace `SPELL_ICON_ABILITIES` iteration with `ability_definitions.iter()`, reading `config.icon` for non-empty paths. Keep generic aura icon loading unchanged.
  - **`load_ability_icons()`**: Add `ability_definitions: Res<AbilityDefinitions>` parameter. Replace dynamic class ability collection + `get_ability_icon_path()` with iteration over `ability_definitions.iter()`.
  - **`get_aura_icon_key()`**: Add `ability_definitions: &AbilityDefinitions` parameter. Replace `get_ability_icon_path()` call with a scan of definitions for matching `config.name` with non-empty icon. Update all callers.
  - **Remove**: `get_ability_icon_path()` function and `SPELL_ICON_ABILITIES` constant
  - **Remove**: `get_ability_icon_path` import from `view_combatant_ui.rs`

  **Patterns to follow:**
  - Item icon loading in `view_combatant_ui.rs` (already refactored to read from `ItemDefinitions`)

  **Test scenarios:**
  - Happy path: project compiles with no references to removed functions
  - Edge case: abilities with empty icon field are skipped (no panic, just no icon loaded)
  - Integration: `get_aura_icon_key()` returns ability name for abilities with icons, generic key for those without

  **Verification:**
  - `cargo build --release` compiles with zero warnings about removed items
  - No remaining references to `get_ability_icon_path` or `SPELL_ICON_ABILITIES` in codebase

- [ ] **Unit 3: Add enforcement test and update documentation**

  **Goal:** Prevent future regressions; update docs to reflect new pattern

  **Requirements:** R6, R7, R8

  **Dependencies:** Unit 2

  **Files:**
  - Modify: `src/states/play_match/ability_config.rs` (add test)
  - Modify: `CLAUDE.md` (update "Adding a New Ability" step 5)
  - Delete: `docs/follow-ups/ability-icon-refactor.md`

  **Approach:**
  - Add `all_abilities_have_icons` test in `ability_config.rs` test module — load definitions, iterate, assert all have non-empty icon field
  - Update CLAUDE.md step 5 to say: add `icon:` field to the ability entry in `abilities.ron` (remove references to `rendering/mod.rs` match and `SPELL_ICON_ABILITIES`)
  - Update the Paladin implementation pattern doc checklist if it references `get_ability_icon_path()`
  - Delete the follow-up doc

  **Patterns to follow:**
  - `all_items_have_icons` test at `equipment.rs:1383-1400`

  **Test scenarios:**
  - Happy path: `all_abilities_have_icons` passes when all abilities have icons
  - Error path: test would fail if an ability has an empty icon field (verified by temporarily clearing one in a local test run)

  **Verification:**
  - `cargo test` passes including new `all_abilities_have_icons` test
  - `grep -r "get_ability_icon_path\|SPELL_ICON_ABILITIES" src/` returns zero results
  - Follow-up doc is deleted

## System-Wide Impact

- **Interaction graph:** `load_spell_icons` and `load_ability_icons` are registered in `states/mod.rs` — no registration changes needed, only function signatures change
- **Error propagation:** Empty icon fields result in skipped icon loading (no crash), caught by enforcement test
- **State lifecycle risks:** None — icon loading is idempotent (loads once, checks `loaded` flag)
- **API surface parity:** Both icon loaders (in-match and view-combatant) must be updated together
- **Unchanged invariants:** Generic aura icons (`GENERIC_AURA_ICONS`) remain untouched; aura fallback behavior preserved

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Missing icon path transcription | Enforcement test catches any ability without an icon |
| `get_aura_icon_key()` callers miss parameter update | Compiler error — Rust enforces signature changes |
| Typo in icon path in abilities.ron | Icon silently won't load; but same risk existed before. Could add asset-exists test later |

## Sources & References

- **Origin document:** [docs/follow-ups/ability-icon-refactor.md](docs/follow-ups/ability-icon-refactor.md)
- Reference commit: 453360c (item icon refactor)
- Item icon pattern: `src/states/play_match/equipment.rs:328-330, 595, 1383`
- Paladin pattern doc: `docs/solutions/implementation-patterns/adding-new-class-paladin.md`
