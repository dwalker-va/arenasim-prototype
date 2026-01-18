# Session Notes

Development history of ArenaSim, preserved for context and learning.

---

## Session 16 (January 18, 2026 - Polymorph)

**New Mage Ability: Polymorph**

Implemented Polymorph as a CC spell for Mages that transforms the target into a sheep:

**Ability Definition:**
- [x] 1.5s cast time, 30 yard range, 60 mana cost
- [x] Arcane spell school
- [x] 10 second duration (same as Fear)
- [x] Breaks on ANY damage (threshold 0)
- [x] Target wanders at 50% speed (like Fear behavior)

**New AuraType:**
- [x] Added `AuraType::Polymorph` - separate from Stun for future diminishing returns
- [x] Polymorph is an "incapacitate" category, distinct from stuns/fears

**AI Behavior:**
- [x] Mage uses Polymorph on `cc_target` (not kill target)
- [x] Skips if cc_target == kill_target (any damage breaks it)
- [x] Checks if target already CC'd to prevent overlap
- [x] Checks Arcane spell school lockout
- [x] Priority: Ice Barrier → Frost Nova → Polymorph → Frostbolt

**Visual Feedback:**
- [x] "SHEEPED X.Xs" status label in hot pink above polymorphed targets
- [x] Downloaded `spell_nature_polymorph.jpg` icon from Wowhead
- [x] Icon appears in ability timeline

**Break-on-Damage Fix:**
- [x] Fixed semantics: 0.0 threshold now correctly means "break on any damage"
- [x] Changed default `break_on_damage` from 0.0 to -1.0
- [x] -1.0 means "never break on damage" (used by buffs)
- [x] Fixed WeakenedSoul in priest.rs which was incorrectly using 0.0

**Files Modified:**
- `assets/icons/abilities/spell_nature_polymorph.jpg` - New spell icon
- `src/states/play_match/components/mod.rs` - AuraType::Polymorph
- `src/states/play_match/abilities.rs` - AbilityType::Polymorph
- `assets/config/abilities.ron` - Polymorph definition
- `src/states/play_match/ability_config.rs` - Default break_on_damage changed to -1.0
- `src/states/play_match/combat_core.rs` - Polymorph wandering (50% speed)
- `src/states/play_match/auras.rs` - Break-on-damage logic fix, Polymorph CC checks
- `src/states/play_match/rendering/hud.rs` - SHEEPED status label
- `src/states/play_match/rendering/mod.rs` - Polymorph icon registration
- `src/states/play_match/class_ai/mage.rs` - try_polymorph() and AI priority
- `src/states/play_match/class_ai/mod.rs` - is_ccd() and is_incapacitated() include Polymorph
- `src/states/play_match/class_ai/priest.rs` - WeakenedSoul fix
- `src/states/play_match/combat_ai.rs` - is_entity_ccd() includes Polymorph

**Commits:**
- `4fe6133` - feat: add Polymorph ability for Mage class

**Key Learnings:**
- Break-on-damage threshold semantics matter: 0.0 = any damage, -1.0 = never
- Separate AuraType for each CC category enables future diminishing returns
- CC targeting on non-kill targets prevents wasted CC on damage focus

---

## Session 15 (January 18, 2026 - Strategic CC Targeting)

**Strategic CC Targeting System:**

Implemented a complete CC targeting system that enables AI to use crowd control on non-kill targets, creating outnumbering situations (2v1, 3v2) in arena combat.

**New Configuration Options:**
- [x] Added `team1_cc_target` and `team2_cc_target` to MatchConfig
- [x] Added same fields to HeadlessMatchConfig with serde support
- [x] Validation ensures cc_target index is within enemy team bounds

**Combatant CC Target Field:**
- [x] Added `cc_target: Option<Entity>` to Combatant struct
- [x] Separate from kill target - enables strategic CC on healers while killing DPS

**Heuristic CC Target Selection:**
- [x] Implemented `select_cc_target_heuristic()` function with scoring system:
  - Healer (Priest): +100 points when killing DPS
  - DPS: +100 points when killing healer (inverted priority)
  - Non-kill-target: +50 points (enables outnumbering)
  - Higher HP: +20 points (don't waste CC on dying targets)
  - Skip already-CC'd targets (prevent CC overlap)

**Class AI Updates:**
- [x] Warlock Fear now uses `cc_target.or(target)` for strategic CC
- [x] Rogue Kidney Shot uses cc_target with melee range fallback
  - Added `select_melee_cc_target()` helper that falls back to kill target if CC target out of range

**Fear Balance Adjustments:**
- [x] Increased Fear `break_on_damage` threshold from 30 to 100
- [x] Fear now survives DoT ticks and requires ~2 Frostbolts to break

**Bug Fixes:**
- [x] Fixed entity despawn panic in `spawn_spell_impact_visuals`
  - Changed `insert()` to `try_insert()` to handle despawned entities gracefully

**Helper Functions:**
- [x] Added `is_ccd()` method to CombatContext for CC overlap prevention
- [x] Added `is_entity_ccd()` helper in combat_ai.rs

**Files Modified:**
- `src/states/match_config.rs` - cc_target config fields
- `src/headless/config.rs` - HeadlessMatchConfig cc_target support
- `src/states/play_match/components/mod.rs` - Combatant.cc_target field
- `src/states/play_match/combat_ai.rs` - CC target acquisition and heuristics
- `src/states/play_match/class_ai/mod.rs` - is_ccd() helper
- `src/states/play_match/class_ai/warlock.rs` - Fear uses cc_target
- `src/states/play_match/class_ai/rogue.rs` - Kidney Shot with melee fallback
- `src/states/play_match/rendering/effects.rs` - try_insert fix
- `assets/config/abilities.ron` - Fear break threshold
- `tests/headless_tests.rs` - cc_target test fields

**Commits:**
- `45e970c` - feat: add strategic CC targeting system

**Testing Results:**
- Fear break threshold: Verified Fear breaks at 144/100 damage (working)
- Inverted heuristic: Warlock correctly Fears Warrior (DPS) when killing Priest (healer)
- Melee fallback: Rogue uses Kidney Shot on kill target when CC target out of range

**Key Learnings:**
- CC targeting separate from damage targeting enables realistic arena tactics
- Heuristic inversion (CC DPS when killing healer) prevents wasted CC
- Melee abilities need range-aware target selection with fallback logic

---

## Session 14 (January 17, 2026 - Data-Driven Ability Definitions)

**Major Refactoring: Data-Driven Abilities (Plan Item 3.3)**

Migrated all 21 ability definitions from hardcoded Rust to RON configuration file, enabling runtime modification without recompilation.

**New Architecture:**
- [x] Created `ability_config.rs` with new config structs:
  - `AuraEffect` - named fields for aura effects (replaces tuple)
  - `ProjectileVisuals` - color configuration for spell projectiles
  - `AbilityConfig` - full ability definition with all parameters
  - `AbilityDefinitions` - Bevy Resource loaded at startup
- [x] Created `assets/config/abilities.ron` with all 21 abilities in RON format
- [x] Added `AbilityConfigPlugin` for loading and validation at startup
- [x] Validation ensures all `AbilityType` variants have definitions

**Migration Completed:**
- [x] Added serde derives to `SpellSchool`, `ScalingStat`, `AbilityType`, `AuraType` enums
- [x] Migrated `mage.rs` - all 5 mage abilities use config system
- [x] Migrated `priest.rs` - all 5 priest abilities use config system
- [x] Migrated `warrior.rs` - all 5 warrior abilities use config system
- [x] Migrated `rogue.rs` - all 3 rogue abilities use config system
- [x] Migrated `warlock.rs` - all 3 warlock abilities use config system
- [x] Migrated `projectiles.rs` - projectile hit processing uses config
- [x] Updated `combat_ai.rs` - passes `AbilityDefinitions` to all class AI

**Key Changes:**
- `ability.definition()` → `abilities.get_unchecked(&ability)`
- `calculate_ability_damage(&def)` → `calculate_ability_damage_config(def)`
- Aura tuple `(aura_type, duration, magnitude, break_threshold)` → Named fields `aura.aura_type`, `aura.duration`, etc.
- Tick interval now stored in config instead of hardcoded

**Files Created:**
- `src/states/play_match/ability_config.rs` - Config structs and loading plugin

**Files Modified:**
- `assets/config/abilities.ron` - Complete rewrite with new schema
- `src/states/play_match/abilities.rs` - Added serde derives
- `src/states/play_match/components/mod.rs` - Added serde derives to AuraType, dual damage calculation methods
- `src/states/play_match/class_ai/*.rs` - All 5 class AI modules updated
- `src/states/play_match/combat_ai.rs` - Pass abilities to all class AI
- `src/states/play_match/projectiles.rs` - Use config for hit processing
- `src/main.rs` - Register AbilityConfigPlugin
- `src/headless/runner.rs` - Register AbilityConfigPlugin

**Commits:**
- `afadf65` - feat: add data-driven ability config system (partial migration)
- `fdbaf02` - feat: complete migration of class AI to data-driven abilities

**Benefits:**
- Balance changes via config file without recompilation
- Cleaner ability definitions with named fields
- Foundation for modding support
- Easier testing of ability variations
- All 82 unit tests pass, all 18 regression tests pass

**Legacy Code Retained:**
- `AbilityType::definition()` method still exists for backward compatibility
- Can be removed in future cleanup pass once all callers migrated

---

## Session 13 (January 16, 2026 - Headless Mode Fixes & Absorb Shield Refinements)

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

---

## Session 12 (January 16, 2026 - Absorb Shields: Ice Barrier & Power Word: Shield)

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

---

## Session 11 (January 15, 2026 - Shadow Sight Orbs & Headless Testing)

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

---

## Session 10 (January 12, 2026 - Timeline, Low HP Highlighting & Match Preview)

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

---

## Session 9 (January 10, 2026 - Warlock Class & Fear CC)

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

---

## Session 8 (January 8, 2026 - Results Scene Enhancement)

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

---

## Session 6 (January 5, 2026 - Major Code Refactoring)

**Complete `play_match` Module Reorganization:**
- Refactored monolithic `src/states/play_match.rs` (2640 lines) into organized submodules
- Final `play_match/mod.rs`: 325 lines (87.7% reduction!)
- Improved maintainability, readability, and navigation

**10 Focused Submodules Created:**
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

**Clean Module Boundaries:**
- Each module has a clear, single responsibility
- Proper public/private visibility
- Re-exports in `mod.rs` for clean external API
- No circular dependencies

**Benefits of Refactoring:**
- **Easier Navigation:** Find systems quickly by domain (AI, rendering, auras, etc.)
- **Better Collaboration:** Multiple systems can be worked on simultaneously
- **Reduced Cognitive Load:** Each file is now <1200 lines with focused purpose
- **Scalability:** Adding new abilities, classes, or systems is now straightforward
- **Agent-Friendly:** AI can reason about and modify specific subsystems without context overload

---

## Session 5 (January 4, 2026 - UX Polish & Deterministic FCT)

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

---

## Session 4 (January 3, 2026 - Combat System Implementation)

**Stat Scaling System (Gear-Ready Architecture):**
- Added `attack_power` and `spell_power` fields to `Combatant` struct
- Class-specific stat allocation:
  - Warriors: 30 Attack Power (physical DPS)
  - Rogues: 35 Attack Power (burst physical DPS)
  - Mages: 50 Spell Power (magical DPS)
  - Priests: 40 Spell Power (healing & damage hybrid)
- Foundation for gear system - when gear is added, it will increase these stats

**Coefficient-Based Damage Formula:**
- Replaced fixed damage ranges with scalable formula: **Damage = Base + (Stat × Coefficient)**
- `AbilityDefinition` refactored:
  - Old: `damage_min`, `damage_max` (fixed values)
  - New: `damage_base_min`, `damage_base_max`, `damage_coefficient`, `damage_scales_with`
- Each ability now has a coefficient (e.g., Frostbolt: 80% of Spell Power)
- `ScalingStat` enum: AttackPower, SpellPower, or None (for utility/CC)

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

**Crowd Control:**
- Frost Nova - First Crowd Control Ability
  - Mage's Defensive Tool: AOE instant-cast ability
  - Root Effect: New CC type that prevents movement (can still attack/cast)
  - 10 unit radius AOE centered on caster
  - 30 mana cost, 25 second cooldown
  - 6 second root duration on all enemies hit
  - "ROOTED" label in ice blue above affected combatants

**WoW-Style Combat Mechanics:**
- [x] Movement prevented while casting
- [x] Casters face target when starting cast
- [x] Auto-attack disabled while casting

**Code Refactoring:**
- [x] Extracted Play Match logic to `src/states/play_match.rs`
- [x] Extracted Configure Match UI to `src/states/configure_match_ui.rs`
- [x] Extracted Results UI to `src/states/results_ui.rs`
- [x] Comprehensive documentation in combat module

**Bug Fixes:**
- Fixed Priest healing not applying (query filter issue with self-targeting)
- Fixed caster damage_dealt not updating (borrow checker conflict)
- Fixed auto-attacking while casting (missing casting state check)

---

## Session 3 (January 2, 2026 - UI Architecture Refactor & Polish)

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

---

## Session 2 (January 2, 2026 - Configure Match & Bug Hunting)

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

**Bug fixes** (discovered via systematic review):
- Team panels now rebuild when size changes (added markers: TeamPanel, TeamSlot, MapPanel)
- CharacterPickerState resource cleanup on scene exit
- ESC handler in ConfigureMatch closes modal before exiting scene

**Established bug detection workflow**: State lifecycle analysis, edge case testing, code path verification

**User testing revealed multiple issues:**
- Child ordering bug in rebuild logic causing unresponsive buttons
- System ordering race condition for shrink→grow sequence
- Panels resizing as content changes (disorienting UX)

**UX improvement**: Fixed panel dimensions to prevent disorienting resize on content changes
- Fixed height at 400px with `align_self: AlignSelf::Start`
- Fixed width distribution with `flex_basis: Val::Px(0.0)` + `flex_grow: 1.0` + `flex_shrink: 1.0`
- This forces equal width distribution regardless of content (standard CSS flexbox pattern)
- Team panels no longer resize when team size changes (1→2→3)
- Map panel has consistent dimensions
- Reduces cognitive load - no need to re-scan entire screen

**Bugs Fixed (Post-User Testing):**
- **Character slots unresponsive after selection**: Fixed child ordering bug in `update_config_ui`. When rebuilding panels, they were added AFTER the MapPanel, corrupting the UI tree structure (Map→Team1→Team2 instead of Team1→Map→Team2). Now rebuilds entire content area to maintain correct ordering.
- **Character buttons flash when map changes**: Was rebuilding entire UI on ANY config change. Now tracks previous config state with `PreviousMatchConfig` resource and only rebuilds when team compositions actually change, not when just the map changes.
- **REGRESSION - Last character slot unresponsive after team size change (attempt 1)**: The `With<Children>` filter in the content_area query prevented subsequent rebuilds. After first despawn_descendants(), the Children component state was invalid, causing query to fail silently. Fixed by removing the `With<Children>` requirement - only `With<MainContentArea>` is needed.
- **REGRESSION - Character slot unresponsive after shrink→grow sequence**: Systems were not chained, causing race condition. `handle_configure_buttons` modifies config, but `update_config_ui` might run in parallel/before, missing the change. Next frame, `config.is_changed()` returns false (already consumed), so no rebuild. Fixed by adding `.chain()` to guarantee execution order: button handler → update UI → ESC handler.

---

## Session 1 (January 2, 2026 - Project Setup)

- Created tech stack decision document
- Scaffolded Bevy project with module structure
- Defined combat components, events, and logging
- Created RON config files for characters, abilities, and maps
- Next session should focus on: **Main Menu Scene** to establish UI patterns, then **Play Match Scene** for core gameplay
