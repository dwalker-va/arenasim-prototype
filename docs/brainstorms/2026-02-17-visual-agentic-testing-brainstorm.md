# Visual Agentic Testing for ArenaSim

**Date**: 2026-02-17
**Status**: Deferred

## What We're Building

A visual testing harness that lets Claude Code act as a QA tester for the game's GUI and combat rendering. After making UI changes (health bars, combat log, timeline, effects), Claude can launch the game, capture screenshots at specific moments, and analyze them visually to verify correctness.

### The Workflow

1. Developer makes a UI/rendering change
2. Developer asks Claude to verify: "check that the health bars look right"
3. Claude runs `/test-visual` (or similar)
4. The app launches in visual-test mode with a predefined scenario
5. Bevy captures screenshots at specified timestamps
6. App exits automatically after captures complete
7. Claude reads the PNG screenshots and provides visual analysis
8. Developer gets feedback: "Health bars render correctly, but the mana bar text overlaps at low zoom"

## Why This Approach

### Bevy-Native Screenshot Harness (chosen over alternatives)

**Selected over:**
- **macOS `screencapture`**: OS-level capture is timing-dependent, not frame-exact, and macOS-only. Can't guarantee capturing the right frame.
- **Hybrid incremental**: While pragmatic, the user is willing to invest upfront for a robust, reproducible system.

**Why Bevy-native:**
- **Deterministic**: Fixed RNG seeds + exact frame capture = reproducible screenshots
- **Cross-platform**: No OS-specific tools needed
- **Frame-exact**: Capture at precise game timestamps, not wall-clock estimates
- **CI-ready**: The same harness works in headless CI environments (with a virtual framebuffer)
- **Clean API**: Bevy 0.15's `Screenshot::primary_window()` + observer pattern is straightforward

## Key Decisions

### 1. CLI Interface: `--visual-test <scenario>`

New CLI mode alongside existing `--headless`. Runs the full graphical client but:
- Skips the main menu (auto-starts the match)
- Uses scenario-defined teams, seed, and map
- Captures screenshots at defined timestamps
- Auto-exits after all captures complete

### 2. Scenario Definitions in RON

Follows the project's existing pattern (abilities.ron, characters.ron). Scenarios support multiple modes to test different game states.

**Match mode** (runs a full match):
```ron
(
    name: "Combat HUD Mid-Fight",
    mode: PlayMatch,
    team1: ["Paladin", "Warrior"],
    team2: ["Warlock", "Mage"],
    map: "BasicArena",
    seed: 42,
    captures: [
        // Time-based captures (during match)
        (at_seconds: 0.5, label: "pre_match_countdown"),
        (at_seconds: 15.0, label: "mid_combat"),
        (at_seconds: 15.1, action: SwitchTab("Timeline"), label: "timeline_view"),
        (at_seconds: 30.0, action: SwitchTab("Combat Log"), label: "combat_log"),
        // Event-driven captures (variable timing)
        (on_event: MatchEnd, delay: 1.0, label: "victory_celebration"),
        (on_event: StateChange(Results), delay: 0.5, label: "results_screen"),
    ],
)
```

Captures support two trigger types:
- **`at_seconds`**: Fire at a specific match timestamp. Best for combat states.
- **`on_event`**: Fire when a game event occurs, with optional `delay` for animations to settle. Best for variable-timing events (match end, state transitions).

**Menu mode** (navigates through non-match screens):
```ron
(
    name: "Menu UI Flow",
    mode: ConfigureMatch,
    team1: ["Warrior", "Priest"],
    team2: ["Mage"],
    map: "PillaredArena",
    captures: [
        (at_seconds: 1.0, label: "config_screen"),
        (at_seconds: 1.5, action: ViewCombatant(team: 1, slot: 0), label: "warrior_details"),
        (at_seconds: 3.0, action: NavigateBack, label: "config_returned"),
    ],
)
```

### 3. Screenshot Capture via Bevy Observer

Using Bevy 0.15's built-in API:
```rust
commands
    .spawn(Screenshot::primary_window())
    .observe(save_to_disk(path));
```

A system checks elapsed match time against the scenario's capture list and triggers screenshots at the right moments.

### 4. Output Directory Structure

```
visual_tests/
    scenarios/              # RON scenario definitions (checked in)
        core_menu_ui.ron
        core_combat_hud.ron
        core_combat_panels.ron
        core_match_lifecycle.ron
    captures/               # Screenshot output (gitignored)
        core_menu_ui/
            config_screen.png
            warrior_details.png
            config_returned.png
        core_combat_hud/
            pre_match_countdown.png
            mid_combat.png
            timeline_view.png
            combat_log.png
        ...
```

### 5. Slash Command: `/test-visual`

**Usage:**
- `/test-visual` — Runs all core scenarios (default)
- `/test-visual core_combat_hud` — Runs one specific scenario
- `/test-visual --no-build` — Skip the cargo build step
- `/test-visual core_combat_hud --no-build` — Combine both

**Workflow:**
1. Build (unless `--no-build`): `cargo build --release`
2. For each scenario:
   a. Run: `cargo run --release -- --visual-test visual_tests/scenarios/<name>.ron`
   b. Wait for app to auto-exit after all captures taken
3. Read each captured PNG via Claude's vision capabilities
4. Report detailed analysis for every screenshot

**Output format:**
For each screenshot, Claude provides a detailed description of what's visible, then flags any issues. Example:

```
📸 core_combat_hud / mid_combat.png
Visible: 4 combatants with health bars — Paladin (85% HP, full mana),
Warrior (70% HP, 45 rage), Warlock (60% HP, casting Shadow Bolt),
Mage (90% HP, Frost Nova aura icon visible). Felhunter pet visible
with nameplate. Cast bar showing on Warlock. Time controls in
top-right showing 1x speed.
⚠ Issue: Warrior's aura icon overlaps the rage bar text.

📸 core_combat_hud / timeline_view.png
Visible: Timeline panel with 5 columns (4 combatants + pet). Ability
icons spaced correctly, time axis visible. Multiple spell casts
recorded in first 15 seconds.
✓ No issues detected.
```

After all screenshots: brief summary with total issues found.

### 6. Claude's Analysis Role

Claude reads the screenshots and checks for:
- **Layout**: Elements positioned correctly, no overlaps, proper alignment
- **Readability**: Text legible, contrast sufficient, font sizes appropriate
- **Completeness**: Expected UI elements present (health bars, mana bars, cast bars, aura icons)
- **Visual effects**: Spell effects rendering (correct colors, positions, no Z-fighting)
- **Regressions**: Comparison against previous captures or baselines if available

This is qualitative analysis, not pixel-diff. Claude describes what it sees and flags anything that looks wrong.

## Scenario Library

Four core scenarios provide broad coverage across all game screens. No granular per-effect scenarios — Claude's qualitative analysis handles specifics.

### 1. `core_menu_ui.ron` — Non-match screens (ConfigureMatch + ViewCombatant)
- **Mode**: ConfigureMatch
- **Pre-filled**: Warrior + Priest vs Mage, PillaredArena
- **Captures**: Config screen layout, navigate to Warrior detail view, navigate back
- **Covers**: Three-column layout, class icons, character picker, stat display, ability list

### 2. `core_combat_hud.ron` — Maximum in-match UI coverage
- **Mode**: PlayMatch
- **Teams**: Paladin + Warrior vs Warlock + Mage (4 combatants + Felhunter pet)
- **Why**: Different resource types (mana, rage), healing + damage, DoTs, CC effects, aura icons, pet nameplate
- **Captures**: Pre-match buffing (~0.5s), mid-combat with active auras (~15s), low-HP moment (~45s)

### 3. `core_combat_panels.ron` — Combat log and timeline tabs
- **Mode**: PlayMatch
- **Teams**: Same 2v2 composition (lots of events = rich log data)
- **Captures with actions**: Combat log tab (~20s), switch to Timeline tab (~20.1s), late-match timeline (~40s)

### 4. `core_match_lifecycle.ron` — Full match arc including results
- **Mode**: PlayMatch
- **Teams**: Warrior vs Mage (1v1, faster match)
- **Captures**: Countdown overlay (at_seconds: 0.5), gates-open (at_seconds: 10.5), mid-fight (at_seconds: 20), victory celebration (on_event: MatchEnd), results screen (on_event: StateChange(Results))

Additional scenarios can be created ad hoc, but these four cover all game screens and the majority of visual verification needs.

## Resolved Design Questions

1. **Window size**: Match current display resolution. No fixed resolution enforcement — this is a local dev tool, not a CI artifact generator.
2. **Camera control**: Default camera only. Scenarios do not override camera position or zoom. Tests verify what players actually see.
3. **Baseline comparison**: Claude qualitative analysis only. No pixel-diff tooling or baseline images. Claude reads each screenshot and describes what it sees, flagging anything that looks wrong.
4. **UI state switching**: Yes — scenarios can include actions between captures (e.g., "switch to timeline tab", "open combat log"). This enables testing multiple UI states in a single match run.
5. **CI integration**: Not now, maybe later. Focus on the local developer workflow first. Architectural choices should not preclude CI, but don't add complexity for it.

## Technical Notes

- **Bevy version**: 0.15 with `Screenshot::primary_window()` observer API
- **Existing patterns**: RON config files, clap CLI args, deterministic RNG seeds
- **Image reading**: Claude Code can read PNG files natively via the Read tool
- **egui UI**: The HUD uses `bevy_egui` 0.31 — UI state may need explicit setup for tab selection in scenarios
