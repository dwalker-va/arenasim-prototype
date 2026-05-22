# Residual Review Findings — worktree-armory

Source: `ce-code-review mode:autofix` run `20260522-014512-63eff2c7` during the LFG pipeline for the armory-screen feature.

Run artifact: `/tmp/compound-engineering/ce-code-review/20260522-014512-63eff2c7/`

## Residual Review Findings

- **[P1][maintainability] `src/states/armory_ui.rs:15` — ItemIcons / load_item_icons named after view_combatant_ui but now serve two screens.** Confidence 90. Suggested fix: move `ItemIcons`, `ItemIconHandles`, and `load_item_icons` to a shared module (e.g. `src/states/item_icons.rs`); both `view_combatant_ui` and `armory_ui` import from the shared location. Deferred from autofix because it touches two state modules and the registration audit's scope.
- **[P2][correctness] `src/states/armory_ui.rs:164` — Armory stuck on "Loading..." indefinitely if any item icon path fails to resolve.** Confidence 55. Suggested fix: either drop the `icons_ready` gate and let `tile_ui`'s placeholder cover missing icons, or add a watchdog that surfaces per-tile placeholders after ~5s. Back button still works in the meantime, so the user isn't trapped — this is defensive against a future broken `icon:` field in `items.ron`.
- **[P3][maintainability] `src/states/mod.rs:423` — BUTTON_TEXT theme constant duplicated as a literal in `main_menu_ui` ARMORY button.** Confidence 95. Suggested fix: centralize palette constants in `src/states/theme.rs` and import from both files, or document the literal convention. Current mix invites drift as new menu entries land.

## Testing Gaps (advisory)

- `ArmoryFilters::matches` has no unit tests (AND-across-axes, OR-within-axis, item_level boundaries, name search trim+case).
- iLvl min/max clamp logic has no test — only the inline UI code asserts it.
- No headless smoke test entering `GameState::Armory`.
- No fixture item with `is_weapon: true` and exactly one of {damage, speed} set.

## Advisory

- [agent-native] Consider mentioning `ArmoryFilters::matches()` in `CLAUDE.md` as a reusable filter predicate.

## Context

This file is the no-PR-yet sink for residual review findings, created per the LFG pipeline's step 5. When the PR is opened (LFG step 7), these findings should be migrated into the PR description and this file can be removed in a follow-up.
