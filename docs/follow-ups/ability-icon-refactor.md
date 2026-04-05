# Refactor: Move Ability Icons into abilities.ron

## Problem

Ability icons are split across three places in code, identical to the item icon problem we just fixed:

1. **`get_ability_icon_path()`** in `src/states/play_match/rendering/mod.rs` (line 30-87) — a 55+ arm match statement mapping ability name strings to icon paths
2. **`SPELL_ICON_ABILITIES`** in `src/states/play_match/rendering/mod.rs` (line 90+) — a separate list of ability names that have icons, used by the icon loading system
3. **`abilities.ron`** in `assets/config/abilities.ron` — the actual ability definitions, which have no icon field

Adding a new ability requires updating all three places. If you forget `SPELL_ICON_ABILITIES` or the match arm, the icon silently doesn't load.

## Solution

Mirror the item icon refactor:

1. Add an `icon: String` field to the ability config struct in `ability_config.rs` (with `#[serde(default)]`)
2. Add `icon: "icons/abilities/spell_frost_frostbolt02.jpg"` to every ability entry in `abilities.ron`
3. Update `load_ability_icons()` in `view_combatant_ui.rs` to read icon paths from `AbilityDefinitions` instead of the hardcoded constant
4. Remove `get_ability_icon_path()` and `SPELL_ICON_ABILITIES` from `rendering/mod.rs`
5. Add an `all_abilities_have_icons` test to enforce every ability has an icon

## Files to Change

- `src/states/play_match/ability_config.rs` — add `icon` field to ability config struct
- `assets/config/abilities.ron` — add icon paths to all 47 ability entries
- `src/states/view_combatant_ui.rs` — update `load_ability_icons()` to read from ability definitions
- `src/states/play_match/rendering/mod.rs` — remove `get_ability_icon_path()` and `SPELL_ICON_ABILITIES`

## Reference

See the item icon refactor in commit `453360c` for the exact pattern to follow.
