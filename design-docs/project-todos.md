# ArenaSim Project TODOs

This document tracks the development progress of the ArenaSim prototype. Reference this document at the start of each agentic session to maintain continuity.

**Last Updated:** January 16, 2026 (Session 13 - Headless Mode Fixes & Absorb Shield Refinements)

---

## Project Status: 🟢 Codebase Refactored & Organized

### Recent Major Changes 🔄

**Session 6 - Major Code Refactoring (87.7% Reduction in mod.rs):**
- ✅ **Complete `play_match` Module Reorganization**
  - Refactored monolithic `src/states/play_match.rs` (2640 lines) into organized submodules
  - Final `play_match/mod.rs`: 325 lines (87.7% reduction!)
  - Improved maintainability, readability, and navigation
  
- ✅ **10 Focused Submodules Created**
  1. **`abilities.rs`** (435 lines) - All ability definitions and spell schools
  2. **`components.rs`** (467 lines) - Component & resource data structures
  3. **`camera.rs`** (291 lines) - Camera control systems (Follow, Manual, Zoom)
  4. **`projectiles.rs`** (270 lines) - Spell projectile systems
  5. **`rendering.rs`** (834 lines) - All UI rendering (cast bars, FCT, status effects, combat log)
  6. **`auras.rs`** (329 lines) - Status effect & aura systems (Root, Stun, DoT, etc.)
  7. **`match_flow.rs`** (288 lines) - Match countdown, victory celebration, time controls
  8. **`combat_ai.rs`** (1169 lines) - All AI decision-making (target acquisition, ability priorities)
  9. **`combat_core.rs`** (1144 lines) - Core combat mechanics (movement, auto-attack, casting, interrupts)
  10. **`mod.rs`** (325 lines) - Setup, cleanup, and module coordination
  
- ✅ **Clean Module Boundaries**
  - Each module has a clear, single responsibility
  - Proper public/private visibility
  - Re-exports in `mod.rs` for clean external API
  - No circular dependencies
  
- ✅ **Compilation & Testing**
  - All modules compile successfully
  - No regressions in functionality
  - Game runs as expected post-refactoring
  
- **Benefits of Refactoring:**
  - **Easier Navigation:** Find systems quickly by domain (AI, rendering, auras, etc.)
  - **Better Collaboration:** Multiple systems can be worked on simultaneously
  - **Reduced Cognitive Load:** Each file is now <1200 lines with focused purpose
  - **Scalability:** Adding new abilities, classes, or systems is now straightforward
  - **Agent-Friendly:** AI can reason about and modify specific subsystems without context overload

**Session 4 - Stat Scaling System (Gear-Ready Architecture):**
- ✅ **Attack Power & Spell Power Stats**
  - Added `attack_power` and `spell_power` fields to `Combatant` struct
  - Class-specific stat allocation:
    - Warriors: 30 Attack Power (physical DPS)
    - Rogues: 35 Attack Power (burst physical DPS)
    - Mages: 50 Spell Power (magical DPS)
    - Priests: 40 Spell Power (healing & damage hybrid)
  - Foundation for gear system - when gear is added, it will increase these stats
  
- ✅ **Coefficient-Based Damage Formula**
  - Replaced fixed damage ranges with scalable formula: **Damage = Base + (Stat × Coefficient)**
  - `AbilityDefinition` refactored:
    - Old: `damage_min`, `damage_max` (fixed values)
    - New: `damage_base_min`, `damage_base_max`, `damage_coefficient`, `damage_scales_with`
  - Each ability now has a coefficient (e.g., Frostbolt: 80% of Spell Power)
  - `ScalingStat` enum: AttackPower, SpellPower, or None (for utility/CC)
  
- ✅ **Coefficient-Based Healing Formula**
  - Same system for healing: **Healing = Base + (Spell Power × Coefficient)**
  - Flash Heal: 15-20 base + 75% of Spell Power
  - All healing scales with Spell Power (WoW standard)
  
- ✅ **Ability Coefficients Balanced**
  - Frostbolt: 10-15 base + 80% SP (strong scaling, reliable DPS)
  - Flash Heal: 15-20 base + 75% SP (efficient healing)
  - Mind Blast: 15-20 base + 85% SP (burst damage on cooldown)
  - Ambush: 20-30 base + 120% AP (high burst from stealth)
  - Sinister Strike: 5-10 base + 50% AP (energy spender)
  - Frost Nova: 5-10 base + 20% SP (utility with minor damage)
  - Heroic Strike, Charge, Kidney Shot: Utility (no damage scaling)
  
- ✅ **Helper Methods for Calculations**
  - `Combatant::calculate_ability_damage(&def)` - applies stat scaling for damage
  - `Combatant::calculate_ability_healing(&def)` - applies stat scaling for healing
  - Centralized calculation logic prevents formula inconsistencies
  - Random variance between min/max + stat scaling for interesting variation
  
- ✅ **Future-Proof for Gear**
  - When gear is implemented, just increase `attack_power` or `spell_power`
  - All abilities automatically scale stronger
  - No need to modify individual ability definitions
  - Easy to test balance by adjusting coefficients

**Session 4 - Crowd Control & Advanced Combat Mechanics:**
- ✅ **Floating Combat Text (FCT)**
  - Damage numbers float above combatants when hit
  - WoW-style: white for auto-attacks, yellow for abilities
  - Batching: Multiple hits on same target in one frame combine into single number
  - Smooth animation: floats upward, fades out over 1.5 seconds
  - Black outline for readability against any background

- ✅ **Combat Log Stream**
  - Real-time scrolling combat log in bottom-right corner
  - Color-coded by event type: red (damage), green (healing), grey (death), yellow (match events)
  - Auto-scrolls to bottom as new events occur
  - Timestamps show match time
  - Similar to WoW combat log

- ✅ **Mana/Resource System**
  - Added mana tracking to all combatants (max_mana, current_mana, mana_regen)
  - Class-specific resource pools:
    - Mage: 200 mana, 10/sec regen
    - Priest: 150 mana, 8/sec regen
    - Rogue: 100 energy, 5/sec regen
    - Warrior: 100 rage (no regen yet)
  - Mana bars rendered below health bars (blue)
  - Resource regeneration system runs each frame

- ✅ **Ability System with Cast Times**
  - Implemented Frostbolt (Mage's first spell):
    - 2.5 second cast time
    - 30 unit range (ranged spell)
    - 25-30 damage (random)
    - 20 mana cost
    - Applies "Chilled" debuff (30% movement speed slow for 5 seconds)
  - CastingState component tracks active casts
  - AI decision-making: Mages cast Frostbolt when target in range and mana available
  - Ability damage shows in yellow FCT (vs white for auto-attacks)

- ✅ **Aura/Debuff System**
  - ActiveAuras component tracks status effects on combatants
  - Aura types: MovementSpeedSlow (more to come)
  - Aura duration ticks down each frame, removes when expired
  - Movement system applies aura modifiers to calculate effective speed
  - Pending aura system to handle borrow checker issues

- ✅ **Match Log Saving (Debug Feature)**
  - Combat logs automatically save to `match_logs/match_<timestamp>.txt` after each match
  - Comprehensive match report includes:
    - Match metadata (arena, duration, winner)
    - Team compositions with final stats (HP, mana, damage dealt/taken)
    - Final positions of all combatants
    - Complete timestamped combat log with position data
    - Distance calculations between entities for each event
  - Position data included for AI debugging (since AI can't watch the visualization)
  - Enables post-match analysis and balance debugging

- ✅ **Casting Movement Restriction (WoW Mechanic)**
  - Combatants now cannot move while casting non-instant spells
  - Movement system checks for `CastingState` component and blocks movement if present
  - Casters face their target when beginning a cast
  - Prevents the visual bug where Mages appeared to cast while running

- ✅ **Cast Bars (WoW-style Visual Feedback)**
  - Orange cast bars appear below health/mana bars when a combatant is casting
  - Shows spell name centered on the bar
  - Fills from left to right as cast progresses (visual progress indicator)
  - Only visible during active casts
  - Slightly wider and taller than health/mana bars for better visibility
  - Distinctive orange color with yellow border (classic WoW style)

- ✅ **Priest Healing Spell (Flash Heal)**
  - Priests now have Flash Heal ability (1.5s cast, 25 mana)
  - Heals 30-40 HP on friendly targets (including self)
  - AI targets lowest HP ally below 90% health
  - Green floating combat text for healing (vs yellow for damage)
  - Cast bar shows "Flash Heal" during cast
  - Healing logged to combat log with position data
  - Adds healer/support role to combat dynamics

- ✅ **Rogue Ambush Combat Logging**
  - Added proper combat log tracking for Ambush attacks
  - Ambush now appears in match reports with position data
  - Yellow FCT spawned for Ambush damage (consistent with ability damage)
  - Fixed combatant_info structure to include class for logging
  - Allows debugging of Rogue stealth mechanics and damage timing

- ✅ **Code Cleanup - Session 4**
  - **UI Module**: Removed unused UI marker components (UiRoot), colors, and fonts modules
    - All UI now uses bevy_egui's own styling, eliminated 60+ lines of unused code
  - **Combat Module**: Removed unused scaffolding from initial design
    - Deleted `components.rs` (180 lines of unused combat components)
    - Deleted `events.rs` (127 lines of unused event definitions)
    - Deleted `systems.rs` (171 lines of unused system implementations)
    - Simplified `mod.rs` to only export CombatLog (all combat logic is in play_match.rs)
  - **Camera Module**: Removed unused advanced camera features
    - Deleted CameraSettings resource and CameraMode enum (60+ lines)
    - Simplified to essential keyboard controls (zoom, pan) and ESC handling
    - Added comprehensive documentation for current and future features
  - **Result**: Removed ~600+ lines of unused/scaffolding code, improved clarity

- ✅ **Time Controls for Match Simulation**
  - Implemented full speed control UI per design doc requirements
  - **UI Panel**: Compact time control panel in top-left corner
    - Current speed indicator (color-coded: red for paused, green for active)
    - Play/Pause button (⏸/▶)
    - Speed buttons: 0.5x, 1x, 2x, 3x (active speed highlighted)
    - Keyboard shortcut hints displayed
  - **Keyboard Shortcuts**:
    - `Space`: Pause/Unpause toggle
    - `1-4`: Set specific speeds (1=0.5x, 2=1x, 3=2x, 4=3x)
  - **Implementation**: Uses Bevy's `Time<Virtual>` for smooth time scaling
  - **Benefits**: Essential for analyzing combat, testing abilities, and debugging

- ✅ **Stealth Visual Indicators**
  - Added clear visual feedback for Rogues in stealth
  - **Transparency**: Stealthed Rogues appear at 40% opacity with darkened tint
  - **Label**: Purple "STEALTH" text above health bar when in stealth
  - **Dynamic**: Instantly transitions to full opacity when stealth breaks (e.g., Ambush)
  - **Implementation**: Material alpha blending with change detection for performance
  - **Benefits**: Clear at-a-glance indication of stealth status

- ✅ **Ambush Combat Logging & Bug Fixes**
  - Fixed Ambush attacks not appearing in match reports
  - Added full position data logging for Ambush (like other abilities)
  - Yellow FCT spawned for Ambush damage (consistent with ability damage)
  - Fixed Rogue color restoration bug after exiting stealth
    - Problem: RGB values stayed darkened (60%) after stealth ended
    - Solution: Mathematically reverse darkening operation (divide by 0.6)

- ✅ **Frost Nova - First Crowd Control Ability**
  - **Mage's Defensive Tool**: AOE instant-cast ability
  - **Root Effect**: New CC type that prevents movement (can still attack/cast)
  - **Ability Parameters**:
    - 10 unit radius AOE centered on caster
    - 30 mana cost, 25 second cooldown
    - 6 second root duration on all enemies hit
  - **Visual Feedback**: "ROOTED" label in ice blue above affected combatants
  - **WoW-like Mechanics**: Roots prevent movement but not actions
  - **Strategic Use**: Mages use to escape melee range, then kite

- ✅ **Cooldown System**
  - Added `ability_cooldowns` HashMap to Combatant component
  - Tracks remaining cooldown per ability (ticked down in `regenerate_resources`)
  - AI checks cooldowns before attempting to cast
  - Cooldowns set when ability is used, cleared when timer reaches 0
  - Extensible for all future abilities with cooldowns

- ✅ **Enhanced Mage AI - Defensive Kiting**
  - **Priority 1**: Check if enemies in melee range → Cast Frost Nova if off cooldown
  - **Priority 2**: Cast Frostbolt on target (standard ranged damage)
  - AI now makes survival decisions (defensive ability usage)
  - **Kiting Behavior Implemented**: After casting Frost Nova, Mage enters kiting mode
    - Kiting timer set to root duration (6 seconds)
    - While kiting, Mage moves away from nearest enemy
    - Allows Mage to create distance while enemies are rooted
    - Timer ticks down over time, returning to normal movement when expired
  - Demonstrates layered decision-making for combat AI

- ✅ **Root CC Implementation**
  - New `AuraType::Root` status effect
  - Modified `move_to_target` system to check for Root aura
  - Rooted combatants cannot move but can still attack and cast
  - Distinct from stuns (which would prevent all actions)
  - Foundation for more CC types (Stun, Silence, etc.)

- ✅ **UI Improvements for Overlapping Elements**
  - **Time Controls**: Moved to top-right corner (was conflicting with combat log)
  - **Semi-transparent Backgrounds**: Both time controls and combat log now use alpha 180-200
  - **Narrower Combat Log**: Default width reduced from 350px to 320px
  - **Smaller Fonts**: Reduced text sizes for more compact UI
  - **Result**: Better visibility of battlefield, less obstruction

- ✅ **Code Refactoring**
  - Extracted Play Match combat logic to dedicated `play_match.rs` module (885 lines)
  - Extracted Configure Match UI to `configure_match_ui.rs` module
  - Extracted Results UI to `results_ui.rs` module
  - `src/states/mod.rs` reduced from 889 to 452 lines
  - Improved modularity and maintainability

**Session 3 - Architecture Refactor & UI Polish:**
- ✅ **Migrated UI from Bevy UI to bevy_egui**
  - Replaced 968-line retained-mode Bevy UI with ~300 lines of immediate-mode egui
  - MainMenu: 180 lines → 60 lines (67% reduction)
  - ConfigureMatch: 968 lines → 240 lines (75% reduction)
  - Eliminated all UI marker components and state synchronization logic
  - Removed complex change detection and system ordering issues
  - Fixed 8 previous bugs by switching to declarative UI model
  - **Rationale**: Immediate mode UI is dramatically simpler for agentic development
  - **Benefits**: Fewer bugs, easier to reason about, more maintainable
  - **Trade-off**: Less ECS-idiomatic, but better suited for menu screens

- ✅ **Iterative UI Polish (Multiple rounds)**
  - Fixed viewport margins (content was touching edges)
  - Implemented responsive panel sizing with ScrollArea
  - Fixed map selector centering (multiple attempts with different egui APIs)
  - Fixed oversized buttons in map selector (was using centered_and_justified layout)
  - All panels now properly sized and centered

- ✅ **Options Menu Implementation**
  - Created `GameSettings` resource with window/graphics options
  - Implemented Options UI with 3 essential settings categories:
    - **Window Mode**: Windowed / Borderless Fullscreen
    - **Resolution**: 1280×720, 1920×1080, 2560×1440 (windowed only)
    - **VSync**: On / Off toggle
  - Added `SettingsPlugin` with reactive settings application
  - Settings apply instantly when changed (no "Apply" button needed)
  - Clean, consistent styling matching Main Menu and Configure Match

- ⏸️ **WASM Support Attempted (Deferred)**
  - Ran into `getrandom 0.3` compatibility issue with Bevy 0.15 + WASM
  - Tried multiple approaches (rustflags, cargo config, dependency patching)
  - Issue: Bevy 0.15 uses getrandom 0.3 which requires special WASM config
  - **Decision**: Defer WASM until Bevy 0.16 or use manual testing for now
  - **Learning**: WASM would enable browser-based testing with Playwright for faster iteration

---

## Project Status Summary

### Completed ✅

- [x] **Tech Stack Decision** - Bevy (Rust) selected and documented
- [x] **Project Structure** - Basic Bevy project scaffolded
  - `Cargo.toml` with Bevy 0.15
  - Module structure: `states`, `camera`, `combat`, `ui`
  - Asset directory structure with config files
- [x] **Data Schemas** - RON configuration files created
  - `characters.ron` - 4 base characters (Warrior, Mage, Rogue, Priest)
  - `abilities.ron` - Initial ability definitions
  - `maps.ron` - 2 arena maps defined
- [x] **Combat Foundation** - Components, events, and logging defined
  - Combatant, Health, Resource, CombatStats components
  - Damage, Healing, Aura, CC event types
  - Combat log system
- [x] **UI System** - bevy_egui (immediate mode)
  - Added bevy_egui 0.31 dependency
  - All menu screens use egui instead of Bevy UI
  - Dramatic code simplification and bug reduction
- [x] **Main Menu Scene** (egui)
  - [x] Basic menu layout (Match, Options, Exit)
  - [x] Button interactions and state transitions
  - [x] Dark fantasy styling with gold accents
  - [x] Centered vertical layout
- [x] **Options Menu Scene** (egui)
  - [x] GameSettings resource for persistent configuration
  - [x] Window Mode selection (Windowed / Borderless Fullscreen)
  - [x] Resolution presets (720p, 1080p, 1440p)
  - [x] VSync toggle with explanatory text
  - [x] Instant application of settings changes
  - [x] Responsive layout with grouped settings panels
- [x] **Configure Match Scene** (egui)
  - [x] Team size selection (1v1, 2v2, 3v3) with +/- buttons
  - [x] Character selection with modal picker
  - [x] 4 character classes: Warrior, Mage, Rogue, Priest
  - [x] Map selection (Basic Arena, Pillared Arena)
  - [x] Start Match button with validation
  - [x] MatchConfig resource persists between states
  - [x] ESC key closes modal first, then returns to menu

---

## Current Sprint: Core Gameplay Loop ✅ COMPLETE

All core gameplay loop features have been implemented. The game loop of "Configure Match → Play Match → Results → Repeat" is fully functional.

### Play Match Scene ✅
- [x] Spawn arena (octagonal with walls and gates)
- [x] Spawn combatants at team positions (starting pens)
- [x] Auto-attack combat loop with attack speed
- [x] Health/Mana/Resource bars above combatants
- [x] Cast bars during spell casting
- [x] Win/lose detection with victory celebration
- [x] Pre-match countdown (10s) with gates that open
- [x] Mana restoration during countdown (pre-buffing phase)

### Results Scene ✅ (Basic)
- [x] Display winning team
- [x] Show per-combatant damage done/taken
- [x] "Done" button to return to menu

### Camera System ✅
- [x] Mouse scroll zoom
- [x] Left-click drag camera rotation/pitch
- [x] WASD camera panning
- [x] Follow combatant mode (Tab to cycle)
- [x] Follow center mode
- [x] Manual mode
- [x] Camera works while paused (uses real time)

### Combat System - Abilities ✅
- [x] 15+ abilities across 4 classes
- [x] Ability cooldowns
- [x] Cast time handling (interruptible)
- [x] Resource cost/generation (Mana, Rage, Energy)
- [x] Spell school lockouts on interrupt

### Combat System - AI ✅
- [x] Target selection (nearest enemy, lowest HP ally)
- [x] Ability usage logic with priorities
- [x] Movement towards targets
- [x] Kiting behavior (Mages)
- [x] Interrupt logic (Warriors)
- [x] Defensive cooldown usage

### Simulation Speed Controls ✅
- [x] Pause/Play toggle (Space)
- [x] Speed buttons (0.5x, 1x, 2x, 3x)
- [x] Keyboard shortcuts (1-4)

### Combat Log Display ✅
- [x] Scrollable log panel
- [x] Color-coded entries (damage, healing, death, events)
- [x] Auto-scroll to latest
- [ ] **TODO**: Filter to HP changes only

### Auras and Buffs ✅
- [x] Aura system (Root, Stun, Slow, DoTs, buffs)
- [x] Duration tracking
- [x] Visual labels (ROOTED, STUNNED, STEALTH)
- [x] Pre-match buff phase (Fortitude)
- [ ] **TODO**: Show remaining duration on auras

### Crowd Control System ✅ (Partial)
- [x] Root (Frost Nova) - prevents movement
- [x] Stun (Kidney Shot, Charge) - prevents all actions
- [x] Fear (Warlock) - target runs randomly, breaks on damage
- [x] CC indicators on combatants
- [x] CC breaks (Fear breaks on damage threshold)
- [ ] **TODO**: Polymorph

### Pre-Match Countdown ✅
- [x] 10-second countdown with visual display
- [x] Gates that lower when countdown ends
- [x] Mana restoration during countdown

---

## Current Sprint: Results Scene Enhancement ✅ COMPLETE

### High Priority 🔴

- [x] **Enhanced Results Scene (WoW Details-style)**
  - [x] Killing blows tracking per combatant
  - [x] CC done/received (measured in seconds)
  - [ ] Team summary totals
  - [x] Damage/healing breakdown by ability (% contribution with bar charts)
  - [x] Use CombatLog as the definitive data source/API
  - [x] Card-based UI with class colors and status badges

### Medium Priority 🟡

- [x] **Combat Log Improvements**
  - [ ] Filter toggle for HP changes only
  - [x] Structured event data for querying (ability breakdown)

- [ ] **Aura Duration Display**
  - [ ] Show remaining time on buff/debuff labels

- [x] **Visual Indicators**
  - [x] Low HP highlighting (below threshold) - Pulsing red glow when HP < 35%
  - [x] Match preview overlay during countdown - Class icons with team-colored borders

### Lower Priority 🟢

- [ ] **Additional CC Types**
  - [x] Fear (run in random direction) - Warlock class added
  - [ ] Polymorph (transform, breaks on damage)
  - [ ] Silence (prevent casting)

---

## Future Milestones

### Milestone 2: Visual Polish
- [ ] Procedural character meshes (distinct silhouettes per class)
- [ ] Ability visual effects (AoE indicators, ground effects)
- [ ] Death animations
- [ ] Arena environment details (pillars, decorations)
- [x] ~~Victory celebration animations~~ (basic version done)
- [x] ~~Spell projectile visuals~~ (Frostbolt, etc. done)

### Milestone 3: Depth
- [ ] Full ability roster per class (currently ~4 per class)
- [ ] Talent system (simplified)
- [ ] Additional maps (only Basic Arena functional)
- [x] ~~Detailed results breakdown (WoW Details-style)~~ → **Moved to Current Sprint**
- [ ] Imbalanced matchups (1v2, 2v3, etc.)

### Milestone 4: Polish
- [ ] Audio implementation
- [ ] Font styling (fantasy theme)
- [x] ~~Options menu expansion (keybinds)~~ (Keybindings menu done)
- [x] ~~Settings persistence~~ (settings.ron saves/loads)
- [ ] Gamepad support
- [ ] SteamDeck testing and optimization

---

## Technical Debt / Notes

- **Aura system architecture**: Currently auras are separate entities. May need to reconsider as child entities or components on the combatant.
- **Killer tracking**: Death events don't properly track who dealt the killing blow. Need to add last-damage-source tracking.
- **Combat log performance**: If matches get long, may need to limit log size or virtualize display.

## Bug Detection Checklist (Enhanced)

For every implementation, verify:
1. **State Lifecycle**: Resources created are cleaned up, entities despawned properly
2. **UI Reactivity**: ALL UI elements update when underlying data changes (not just labels)
3. **Entity Ordering**: Flexbox/UI children are in correct order after rebuilds
4. **Component Attachment**: Interactive elements (Button, etc.) are properly attached
5. **Event Propagation**: Click/input events reach the intended targets
6. **Edge Cases**: Test with max/min values, empty states, rapid changes
7. **Rebuild Granularity**: Only rebuild UI elements that actually changed, not the entire tree
8. **Visual Feedback**: Check for unnecessary flashing/rebuilding (user-visible performance)
9. **Query Filters**: Avoid overly restrictive filters (like `With<Children>`) that can fail after despawning
10. **Idempotency**: Rebuilds should work multiple times in a row without query failures
11. **System Ordering**: Use `.chain()` when one system must see changes from another in the same frame
12. **Change Detection**: Bevy's change detection is frame-based - changes consumed in frame N won't show as changed in frame N+1
13. **Visual Stability**: Use fixed/min heights for panels to prevent disorienting resizing as content changes
14. **Cognitive Load**: Unexpected layout shifts force users to re-scan the entire screen

## Bugs Fixed

### Session 2 Fixes (Part 2 - Post-User Testing)
- **Character slots unresponsive after selection**: Fixed child ordering bug in `update_config_ui`. When rebuilding panels, they were added AFTER the MapPanel, corrupting the UI tree structure (Map→Team1→Team2 instead of Team1→Map→Team2). Now rebuilds entire content area to maintain correct ordering.
- **Character buttons flash when map changes**: Was rebuilding entire UI on ANY config change. Now tracks previous config state with `PreviousMatchConfig` resource and only rebuilds when team compositions actually change, not when just the map changes.
- **REGRESSION - Last character slot unresponsive after team size change (attempt 1)**: The `With<Children>` filter in the content_area query prevented subsequent rebuilds. After first despawn_descendants(), the Children component state was invalid, causing query to fail silently. Fixed by removing the `With<Children>` requirement - only `With<MainContentArea>` is needed.
- **REGRESSION - Character slot unresponsive after shrink→grow sequence**: Systems were not chained, causing race condition. `handle_configure_buttons` modifies config, but `update_config_ui` might run in parallel/before, missing the change. Next frame, `config.is_changed()` returns false (already consumed), so no rebuild. Fixed by adding `.chain()` to guarantee execution order: button handler → update UI → ESC handler.

### Session 2 Fixes (Part 1 - Self-Review)
- **Character slots update on team size change**: Team panels now properly rebuild when +/- buttons change team size
- **Resource cleanup**: CharacterPickerState resource is now properly removed on scene exit
- **ESC key handling**: ESC now closes modal first before returning to main menu (ConfigureMatch has dedicated ESC handler)

---

## Session Notes

### Session 13 (January 16, 2026 - Headless Mode Fixes & Absorb Shield Refinements)

**Headless Mode Bug Fixes:**
- [x] Fixed timestamp regression bug in combat logs
  - Cause: `headless_track_time` was overwriting `combat_log.match_time` with post-gates-only elapsed time
  - Symptom: Timestamps jumped backwards after gates opened (9.99s → 3.16s)
  - Fix: Removed the overwrite line in `runner.rs` - combat_core already handles match time correctly
- [x] Fixed projectiles not working in headless mode (Frostbolts never landing)
  - Cause: Projectiles spawned without `Transform` component; `spawn_projectile_visuals` (graphical only) was adding it
  - Symptom: `move_projectiles` query never matched projectiles, so they never moved or hit
  - Fix: Added `Transform` to projectile spawn in `process_casting` (headless-compatible)

**Absorb Shield Stacking Fix (WoW-accurate):**
- [x] Fixed Ice Barrier incorrectly blocking Power Word: Shield
  - Cause: PW:S AI checked for any `AuraType::Absorb` on target
  - WoW Behavior: Weakened Soul only prevents PW:S, not other absorb effects
  - Fix: AI now checks specifically for existing PW:S by `ability_name`, not any Absorb aura
- [x] Fixed different absorb shields not coexisting on same target
  - Cause: Aura stacking used `(entity, effect_type)` as key - all Absorbs conflicted
  - Fix: Absorb shields now use `ability_name` as stacking key: `format!("absorb:{}", pending.aura.ability_name)`
  - Result: Ice Barrier + PW:S can now coexist on the same target

**Absorbed Damage Tracking Fixes:**
- [x] Fixed absorbed damage not appearing in results screen statistics
  - Cause: `damage_dealt` only tracked actual health damage, not absorbed
  - Fix: Auto-attacks and projectiles now add `actual_damage + absorbed` to `damage_dealt`
  - Combat logs now show total damage including absorbed amounts
- [x] Fixed Mind Blast (and other instant abilities) bypassing absorb shields entirely
  - Cause: `process_casting` applied damage directly without calling `apply_damage_with_absorb()`
  - Fix: Instant damage abilities now use the absorb helper function
  - Added absorbed damage floating text for instant abilities

**Files Modified:**
- `src/headless/runner.rs` - Removed timestamp overwrite
- `src/states/play_match/combat_core.rs` - Added Transform to projectile spawn, integrated absorb handling for instant abilities, fixed damage tracking
- `src/states/play_match/projectiles.rs` - Fixed damage tracking to include absorbed
- `src/states/play_match/auras.rs` - Changed absorb stacking to use ability_name as key
- `src/states/play_match/combat_ai.rs` - PW:S check now only looks for existing PW:S specifically

**Commits:**
- `f8e549f` - fix: headless mode timestamp and projectile bugs
- `a6b4b2c` - fix: allow different absorb shields to coexist (Ice Barrier + PW:S)
- `e7bce00` - fix: track absorbed damage in damage_dealt stats and fix Mind Blast shield bypass

**Key Learnings:**
- Headless mode requires all gameplay-critical components (like Transform) to be added in core systems, not visual-only systems
- Combat log timestamp should be managed in one place (combat_core), not overwritten by multiple systems
- Aura stacking keys should be specific enough to allow intentional coexistence (same effect type from different abilities)
- All damage application sites need to consistently use the absorb helper function

**Next:** Consider balance testing with headless simulations, or expanding other class rosters.

### Session 12 (January 16, 2026 - Absorb Shields: Ice Barrier & Power Word: Shield)

**New Absorb Shield Mechanic:**
- [x] Added `AuraType::Absorb` - damage absorption shields that deplete before health
- [x] Added `AuraType::WeakenedSoul` - debuff preventing re-shielding (15s duration)
- [x] Created `apply_damage_with_absorb()` helper in combat_core.rs
- [x] Updated all damage application sites (auto-attacks, abilities, DoTs, projectiles)

**Ice Barrier (Mage):**
- [x] Self-only instant cast absorb shield (60 damage absorption)
- [x] 30 mana cost, 30 second cooldown, 60 second duration
- [x] Frost spell school
- [x] AI casts pre-combat and re-applies when HP < 80% and shield broken

**Power Word: Shield (Priest):**
- [x] Can shield self or allies (50 damage absorption)
- [x] 25 mana cost, no cooldown (limited by Weakened Soul on target)
- [x] 30 second duration, applies 15s Weakened Soul debuff
- [x] Holy spell school
- [x] AI shields allies below 70% HP who lack shield/Weakened Soul

**Visual Feedback:**
- [x] Shield bubble visual effect around shielded combatants
  - Tall narrow ellipsoid shape (WoW-style)
  - Light blue for Ice Barrier (Frost), golden for PW:S (Holy)
  - Uses additive blending to prevent flickering
  - Bubbles follow combatants and despawn when shield breaks
- [x] Absorbed damage shown in combat logs as "(X absorbed)"
- [x] Light blue floating combat text for absorbed amounts
- [x] Split font rendering: 24pt damage number, 14pt "absorbed" label
- [x] Shield bar extension on health bar (light blue)

**Spell Icons:**
- [x] Downloaded Ice Barrier icon from Wowhead (spell_ice_lament.jpg)
- [x] Downloaded Power Word: Shield icon (spell_holy_powerwordshield.jpg)
- [x] Downloaded Weakened Soul icon for auras

**Files Modified:**
- `src/states/play_match/components.rs` - AuraType::Absorb, WeakenedSoul, ShieldBubble component
- `src/states/play_match/abilities.rs` - IceBarrier, PowerWordShield ability definitions
- `src/states/play_match/combat_core.rs` - apply_damage_with_absorb() helper
- `src/states/play_match/combat_ai.rs` - Shield usage AI for Mage and Priest
- `src/states/play_match/auras.rs` - DoT damage with absorb handling
- `src/states/play_match/projectiles.rs` - Projectile damage with absorb handling
- `src/states/play_match/rendering.rs` - Shield bubble visuals, absorbed FCT, health bar extension
- `src/states/mod.rs` - Shield bubble system registration
- `assets/icons/abilities/` - New spell icons

**Key Learnings:**
- Additive blending (AlphaMode::Add) prevents Z-fighting flicker on translucent meshes
- Combatant transform is at capsule center, not feet - affects bubble positioning
- Split font rendering in egui requires manual layout with strip_suffix detection

**Next:** Consider adding more defensive abilities, or expanding other class rosters.

### Session 11 (January 15, 2026 - Shadow Sight Orbs & Headless Testing)

**Shadow Sight Orb System (Stealth Stalemate Fix):**
- [x] Implemented Shadow Sight orb spawning after 90 seconds of combat
- [x] Two orbs spawn at symmetric positions (north/south of arena center)
- [x] Pickup detection - combatants within 2.5 units collect orb
- [x] Shadow Sight buff (15s duration) - allows seeing stealthed enemies
- [x] Modified target acquisition to check for Shadow Sight buff
- [x] Combatants seek nearest orb when they have no visible targets
- [x] Created `shadow_sight.rs` module with full system implementation

**Shadow Sight Visual Polish:**
- [x] Purple core orb with outer transparent glowing aura
- [x] Bobbing animation (gentle vertical float)
- [x] Rotation animation (continuous spin)
- [x] Pulsing animation (breathing scale effect)
- [x] Pickup animation - orb shrinks and moves toward collector before despawning

**Headless Mode Support:**
- [x] Shadow Sight systems work in both graphical and headless mode
- [x] Optional mesh/material resources for headless compatibility
- [x] Pickup animation system runs in headless mode (for proper despawn timing)

**New Components & Resources:**
- `ShadowSightOrb` - marks orb entities
- `ShadowSightOrbConsuming` - marks orbs being picked up (with animation timer)
- `ShadowSightState` - tracks combat time and orb spawn state
- `AuraType::ShadowSight` - new aura type for the buff

**Files Created/Modified:**
- `src/states/play_match/shadow_sight.rs` (NEW) - Complete Shadow Sight implementation
- `src/states/play_match/components.rs` - New components and AuraType variant
- `src/states/play_match/combat_ai.rs` - Target acquisition with Shadow Sight visibility
- `src/states/play_match/combat_core.rs` - Orb-seeking movement behavior
- `src/states/play_match/auras.rs` - AuraType::ShadowSight match arm
- `src/states/mod.rs` - System registration for graphical mode
- `src/headless/runner.rs` - System registration for headless mode
- `design-docs/game-design-doc.md` - Shadow Sight mechanic documentation

**Key Learnings:**
- Bevy query conflicts require explicit `Without<T>` filters when multiple queries access same component
- Headless mode needs careful handling of optional resources (Assets<Mesh>, Assets<StandardMaterial>)
- Parent-child entity relationships enable layered visual effects (core sphere + outer aura)

**Next:** Consider adding more visual effects or expanding class ability rosters.

### Session 10 (January 12, 2026 - Timeline, Low HP Highlighting & Match Preview)

**Ability Timeline Feature:**
- [x] Added visual ability timeline to combat panel (tabbed view with Combat Log)
- [x] Downloaded spell icons from Wowhead MCP (17 icons)
- [x] Timeline shows columnar layout with one column per combatant
- [x] Spell icons displayed at timestamp positions
- [x] Hover tooltips show "{time}s {ability}" with gold timestamp
- [x] Interrupted abilities shown with red tint
- [x] Match timer now starts from beginning (prep phase visible)
- [x] Dynamic column widths to fill available panel space
- [x] Always-visible scrollbar for navigation
- [x] Timeline is now the default view
- [x] Fixed: All combatant columns show from match start (not just after ability use)

**Low HP Highlighting:**
- [x] Pulsing red glow effect when combatant HP drops below 35%
- [x] Thicker red border on health bar
- [x] Outer glow halo that expands/contracts
- [x] Uses real time so animation works when simulation is paused
- [x] Configurable constants (threshold, pulse speed, intensity)

**Match Preview Overlay:**
- [x] Team composition display during countdown phase
- [x] Shows class icons with team-colored borders (blue T1, red T2)
- [x] Class names displayed alongside icons
- [x] Mirrored layout (T1: icon-name, T2: name-icon) for visual symmetry
- [x] Reuses ClassIcons resource from configure match screen
- [x] Displayed in foreground layer during countdown

**Files Modified:**
- `src/states/play_match/rendering.rs` - Timeline rendering, spell icon loading, low HP highlighting, match preview overlay
- `src/states/play_match/components.rs` - CombatPanelView enum (default changed to Timeline), SpellIcons resource
- `src/states/play_match/combat_core.rs` - Match timer starts from beginning
- `src/states/play_match/camera.rs` - Camera controls moved to bottom-right
- `src/states/play_match/mod.rs` - Register combatants with combat log at spawn
- `src/combat/log.rs` - AbilityCast struct, interrupt tracking, registered_combatants field
- `Cargo.toml` - Added jpeg feature for spell icon loading
- `assets/icons/abilities/` - 17 spell icon JPGs from Wowhead

**Key Learnings:**
- egui's Painter API allows custom drawing with precise positioning
- Foreground layers (`Order::Foreground`) allow tooltips to overflow panel bounds
- Real time (`Time<Real>`) enables animations during simulation pause

### Session 9 (January 10, 2026 - Warlock Class & Fear CC)

**New Warlock Class:**
- [x] Added 5th playable class: Warlock (Shadow/DoT specialist)
- [x] Warlock abilities:
  - **Corruption** - Instant cast DoT (10 damage per 3s for 18s, 6 ticks total)
  - **Shadowbolt** - 2.5s cast, 25-35 shadow damage, purple projectile
  - **Fear** - 1.5s cast, 8s duration, target runs randomly, breaks on 40 damage
- [x] Warlock AI priorities: Fear → Corruption → Shadowbolt
- [x] Purple projectile visuals for Shadowbolt (distinct from blue Frostbolt)

**Fear CC Implementation (WoW-style):**
- [x] `AuraType::Fear` - prevents intentional movement, attacking, and casting
- [x] Random movement behavior - feared targets run in random directions
- [x] Direction changes every 1-2 seconds (randomized timer)
- [x] Added `fear_direction` and `fear_direction_timer` fields to Aura struct
- [x] Fear breaks on damage (40 damage threshold)
- [x] "FEARED" status label displayed above affected combatants
- [x] Speech bubble appears when Fear lands (not when cast starts)

**Visual Polish:**
- [x] Faster spell projectiles (Frostbolt/Shadowbolt: 20→35 speed)
- [x] Ability-based projectile colors (purple Shadowbolt, blue Frostbolt, golden default)

**Bug Fixes:**
- [x] Priest AI no longer tries to heal dead allies
- [x] Fear speech bubble timing (shows on successful application, not cast start)

**Files Modified:**
- `src/states/play_match/abilities.rs` - Warlock abilities (Corruption, Shadowbolt, Fear)
- `src/states/play_match/components.rs` - AuraType::Fear, fear_direction fields in Aura
- `src/states/play_match/combat_ai.rs` - Warlock AI decision logic
- `src/states/play_match/combat_core.rs` - Fear movement behavior, speech bubble timing
- `src/states/play_match/auras.rs` - Fear direction timer ticking
- `src/states/play_match/projectiles.rs` - Ability-based projectile colors
- `src/states/play_match/rendering.rs` - FEARED status label
- `src/states/play_match/match_config.rs` - Warlock class definition

**Key Learnings:**
- Fear movement requires both direction tracking (in Aura) and movement handling (in combat_core)
- Speech bubbles for interruptible abilities should trigger on successful application, not cast start
- Projectile colors based on ability type improve visual clarity

**Next:** Consider adding Polymorph, Silence, or expanding other class ability rosters.

### Session 8 (January 8, 2026 - Results Scene Enhancement)

**Enhanced Results Scene (WoW Details-style):**
- [x] Implemented structured combat logging with `StructuredEventData` enum variants
- [x] Added aggregation methods: `damage_by_ability()`, `healing_by_ability()`, `killing_blows()`, `cc_done_seconds()`, `cc_received_seconds()`
- [x] Updated all combat systems to use structured logging (`log_damage`, `log_healing`, `log_crowd_control`, `log_death`)
- [x] Redesigned Results UI with polished card-based layout:
  - Dark rounded cards for each combatant
  - Class-colored name with ALIVE/DEAD status badge
  - Stat pills showing DMG, TAKEN, HEAL, KILLS, CC
  - Expandable "Ability Details" section with horizontal bar charts
  - Bars show ability name, damage/healing amount, and percentage contribution
- [x] Fixed egui widget ID clash with `.id_salt()` for unique IDs
- [x] Fixed egui ctx panic with `try_ctx_mut()` graceful handling

**Files Modified:**
- `src/combat/log.rs` - Structured event data and aggregation methods
- `src/states/results_ui.rs` - Card-based UI with ability breakdown charts
- `src/states/play_match/combat_core.rs` - Updated to use structured logging
- `src/states/play_match/combat_ai.rs` - Updated to use structured logging
- `src/states/play_match/auras.rs` - Updated DoT damage logging
- `src/states/play_match/projectiles.rs` - Updated projectile damage logging

**Key Learnings:**
- egui CollapsingHeaders need unique IDs via `.id_salt()` when multiple exist
- `try_ctx_mut()` is safer than `ctx_mut()` for systems that may run before egui is ready
- Card-based layouts with stat pills provide cleaner UX than grid tables

**Next:** Choose from remaining tasks - aura duration display, low HP highlighting, additional CC types, or team summary totals.

### Session 5 (January 4, 2026 - UX Polish & Deterministic FCT)

**Floating Combat Text Improvements:**
- [x] Implemented deterministic alternating pattern for FCT positioning
- [x] Replaced random offsets with predictable center → right → left cycle
- [x] Added `FloatingTextState` component to track pattern index per combatant
- [x] Created `get_next_fct_offset()` helper function for consistent offset calculation
- [x] Updated all 8 FCT spawn points (auto-attacks, abilities, DoTs, healing, projectiles)
- [x] Made offsets adjustable via `FCT_HORIZONTAL_SPREAD` and `FCT_VERTICAL_SPREAD` constants
- [x] **Benefits**: Guaranteed separation, predictable patterns, improved readability

**Polish & Bug Fixes:**
- [x] Fixed spell school lockouts not enforced for all abilities (Frost Nova after Frostbolt interrupt)
- [x] Added `is_spell_school_locked()` helper function for cleaner code
- [x] Prevented combat actions during victory celebration (casting, damage, DoTs)
- [x] Despawn active spell casts, projectiles, and impact effects when match ends
- [x] Implemented Mortal Strike ability for Warriors (healing reduction debuff)
- [x] Made Mage/Priest auto-attacks ranged (Wand Shots) to fix positioning issues
- [x] Enhanced status effect labels (STEALTH, STUN, ROOT) with outlines and brighter colors
- [x] Fixed overlapping FCT issue that inspired the deterministic pattern implementation

**Documentation:**
- [x] Updated README.md to reflect current project state
  - Added feature showcase (15+ abilities, 4 classes, WoW-style mechanics)
  - Comprehensive controls section with camera and time controls
  - Character class breakdown with abilities and playstyles
  - Accurate project structure
  - Roadmap section
- [x] Updated project-todos.md with Session 5 notes

**Key Learnings:**
- User-driven iterative refinement: The FCT overlap issue led to a discussion about deterministic vs random positioning, resulting in a better UX design
- Constant-based tunables: Extracting magic numbers into named constants makes experimentation easier
- Helper functions for repeated logic: Created `is_spell_school_locked()` and `get_next_fct_offset()` to centralize logic

**Next:** Consider adding more class abilities, additional arenas, or sound effects.

### Session 4 (January 3, 2026 - Combat System Implementation)

**Core Combat Features:**
- [x] Play Match Scene with 3D arena and combatants
- [x] Results scene with team statistics
- [x] Basic auto-attack combat
- [x] Movement system (combatants move to melee range)
- [x] Real-time combat log stream (WoW-style)
- [x] Floating combat text with batching
- [x] Health bars and cast bars

**Advanced Combat Systems:**
- [x] Ability system (cast times, mana costs, range, cooldowns)
- [x] Mana/resource system with regeneration
- [x] Aura/debuff system (e.g., movement speed slow)
- [x] AI decision-making for spell usage
- [x] Mage: Frostbolt (damage spell with slow debuff)
- [x] Priest: Flash Heal (healing spell with cast time)

**Debugging Tools:**
- [x] Match log saving to file (combat events + metadata + positions)
- [x] Detailed position tracking for debugging AI movement

**WoW-Style Combat Mechanics:**
- [x] Movement prevented while casting
- [x] Casters face target when starting cast
- [x] **Auto-attack disabled while casting** (this fix)

**Code Refactoring:**
- [x] Extracted Play Match logic to `src/states/play_match.rs`
- [x] Extracted Configure Match UI to `src/states/configure_match_ui.rs`
- [x] Extracted Results UI to `src/states/results_ui.rs`
- [x] Comprehensive documentation in combat module

**Bug Fixes:**
- Fixed Priest healing not applying (query filter issue with self-targeting)
- Fixed caster damage_dealt not updating (borrow checker conflict)
- Fixed auto-attacking while casting (missing casting state check)

**Next:** Continue adding more class abilities and refining AI behavior.

### Session 3 (January 2, 2026 - UI Architecture Refactor & Polish)

**Major Decision:** Switched from Bevy UI to bevy_egui for all menu screens after experiencing 8 UI-related bugs.

**Problems with Bevy UI (retained mode):**
- Manual state synchronization (PreviousMatchConfig resource)
- Complex change detection (is_changed() consumed per-frame)
- System ordering dependencies (needed `.chain()`)
- Verbose rebuilds (despawn_descendants + respawn)
- 9 different marker components just for queries
- Subtle bugs with query filters, flexbox, entity ordering

**Migration Results:**
- Added bevy_egui 0.31 dependency
- MainMenu: 180 lines → 60 lines (67% reduction)
- ConfigureMatch: 968 lines → 240 lines (75% reduction)
- Deleted src/states/configure_match.rs
- All 8 previous bugs eliminated by design
- Code is now declarative and easier to reason about

**Iterative UI Fixes (Multiple rounds of user feedback):**
1. Character slot interaction fixed (proper click sensing)
2. Viewport margins added (20px padding on CentralPanel)
3. ScrollArea wrapper for responsive behavior
4. Map selector centering attempts:
   - Tried `horizontal_centered()` - doesn't exist in egui
   - Tried `with_layout + top_down Center` - didn't work
   - Tried `allocate_ui_with_layout + centered_and_justified` - made buttons giant
   - **Final solution**: Manual centering with calculated padding
5. Button sizing fixed (removed min_size constraints, used default button size)

**WASM Attempt (Deferred):**
- Installed wasm32-unknown-unknown target
- Installed wasm-bindgen-cli
- Hit getrandom 0.3 incompatibility with WASM (Bevy 0.15 issue)
- Tried 8+ different approaches (rustflags, cargo config, patches, env vars)
- **Decision**: Skip WASM for now, revisit with Bevy 0.16 or manual testing
- **Rationale**: Would enable Playwright browser testing for faster iteration

**Key Learning:** Immediate-mode UI (egui) is dramatically better than retained-mode UI (Bevy UI) for agentic development of menu screens. The declarative "show current state" model is much easier for AI to work with. However, even with better tools, UI iteration still requires multiple rounds of user feedback - visual refinement is challenging without direct visual testing.

**Next:** Continue with Play Match Scene implementation using Bevy ECS for gameplay (egui only for menus/HUD).

### Session 2 (January 2, 2026)
- Implemented Main Menu Scene with full UI
  - Title, subtitle, version footer
  - Three buttons: Match, Options, Exit
  - Button hover/press visual feedback
  - State transitions working
- Added ESC key handling for navigation
- Implemented Configure Match Scene
  - Team size selection with +/- controls (1-3 per team)
  - Character slot system with modal picker
  - 4 character classes with WoW-inspired colors
  - Map selection with prev/next controls
  - Start Match button validates configuration
  - MatchConfig resource for state persistence
- New files: `states/match_config.rs`, `states/configure_match.rs`
- **Bug fixes** (discovered via systematic review):
  - Team panels now rebuild when size changes (added markers: TeamPanel, TeamSlot, MapPanel)
  - CharacterPickerState resource cleanup on scene exit
  - ESC handler in ConfigureMatch closes modal before exiting scene
- **Established bug detection workflow**: State lifecycle analysis, edge case testing, code path verification
- **User testing revealed multiple issues**:
  - Child ordering bug in rebuild logic causing unresponsive buttons
  - System ordering race condition for shrink→grow sequence
  - Panels resizing as content changes (disorienting UX)
- **UX improvement**: Fixed panel dimensions to prevent disorienting resize on content changes
  - Fixed height at 400px with `align_self: AlignSelf::Start`
  - Fixed width distribution with `flex_basis: Val::Px(0.0)` + `flex_grow: 1.0` + `flex_shrink: 1.0`
  - This forces equal width distribution regardless of content (standard CSS flexbox pattern)
  - Team panels no longer resize when team size changes (1→2→3)
  - Map panel has consistent dimensions
  - Reduces cognitive load - no need to re-scan entire screen
- Next session should focus on: **Play Match Scene** for core gameplay

### Session 1 (January 2, 2026)
- Created tech stack decision document
- Scaffolded Bevy project with module structure
- Defined combat components, events, and logging
- Created RON config files for characters, abilities, and maps
- Next session should focus on: **Main Menu Scene** to establish UI patterns, then **Play Match Scene** for core gameplay

---

## Quick Reference

### Build Commands
```bash
# Development build (fast compile, slower runtime)
cargo run

# Development build with dynamic linking (fastest compile)
cargo run --features dev

# Release build (slow compile, optimized)
cargo run --release

# Check for errors without building
cargo check
```

### Key Files
- `src/main.rs` - Entry point, plugin registration
- `src/states/mod.rs` - Game state definitions
- `src/combat/` - Combat system modules
- `assets/config/` - Game data in RON format

### Bevy Patterns Used
- **States** for scene management
- **Events** for combat actions
- **Resources** for global data (CombatLog, SimulationSpeed)
- **Marker Components** for entity queries (MainMenuEntity, etc.)

