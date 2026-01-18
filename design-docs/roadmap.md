# Project Roadmap

## Current Status

- **Core gameplay loop**: COMPLETE
- **Classes**: Warrior, Mage, Rogue, Priest, Warlock (5)
- **Abilities**: 22 across all classes
- **Headless testing**: COMPLETE
- **Results screen**: Enhanced with WoW Details-style breakdown

## Active TODOs

### High Priority

- [ ] Combat log filter (HP changes only)

### Medium Priority

- [ ] Diminishing returns for CC
- [ ] Team summary totals on results screen
- [ ] Silence CC type (prevent casting)

---

## Milestone 2: Visual Polish

- [ ] Procedural character meshes (distinct silhouettes per class)
- [ ] Ability visual effects (AoE indicators, ground effects)
- [ ] Death animations
- [ ] Arena environment details (pillars, decorations)
- [x] ~~Victory celebration animations~~ (basic version done)
- [x] ~~Spell projectile visuals~~ (Frostbolt, etc. done)

## Milestone 3: Depth

- [ ] Full ability roster per class (currently ~4 per class)
- [ ] Talent system (simplified)
- [ ] Additional maps (only Basic Arena functional)
- [ ] Imbalanced matchups (1v2, 2v3, etc.)
- [x] ~~Detailed results breakdown (WoW Details-style)~~ DONE

## Milestone 4: Polish

- [ ] Audio implementation
- [ ] Font styling (fantasy theme)
- [ ] Gamepad support
- [ ] SteamDeck testing and optimization
- [x] ~~Options menu expansion (keybinds)~~ (Keybindings menu done)
- [x] ~~Settings persistence~~ (settings.ron saves/loads)

---

## Technical Debt

### Aura System Architecture
Currently auras are separate entities. May need to reconsider as child entities or components on the combatant for better performance and simpler queries.

### Combat Log Performance
If matches get long, may need to limit log size or virtualize the display to prevent memory growth and UI slowdown.

---

## Completed Features

### Core Gameplay Loop (Milestone 1)

- [x] Tech stack decision (Bevy/Rust)
- [x] Project structure scaffolded
- [x] Data schemas (RON config files)
- [x] UI system (bevy_egui for menus)
- [x] Main Menu Scene
- [x] Options Menu Scene
- [x] Configure Match Scene
- [x] Play Match Scene
- [x] Results Scene
- [x] Camera system (zoom, pan, follow)

### Combat System

- [x] Auto-attack combat with attack speed
- [x] Health/Mana/Resource bars
- [x] Cast bars during spell casting
- [x] Win/lose detection with victory celebration
- [x] Pre-match countdown (10s) with gates
- [x] Mana restoration during countdown (pre-buffing phase)
- [x] 22 abilities across 5 classes
- [x] Ability cooldowns
- [x] Cast time handling (interruptible)
- [x] Resource cost/generation (Mana, Rage, Energy)
- [x] Spell school lockouts on interrupt
- [x] Killing blow tracking

### AI System

- [x] Target selection (nearest enemy, lowest HP ally)
- [x] Ability usage logic with priorities
- [x] Movement towards targets
- [x] Kiting behavior (Mages)
- [x] Interrupt logic (Warriors)
- [x] Defensive cooldown usage
- [x] Strategic CC targeting (separate from kill target)
- [x] CC target heuristics (healer priority, context-aware inversion)

### Simulation Controls

- [x] Pause/Play toggle (Space)
- [x] Speed buttons (0.5x, 1x, 2x, 3x)
- [x] Keyboard shortcuts (1-4)

### Auras and Buffs

- [x] Aura system (Root, Stun, Slow, DoTs, buffs)
- [x] Duration tracking
- [x] Visual labels with duration countdown (ROOT 5.2s, STUN 3.1s, etc.)
- [x] Pre-match buff phase (Fortitude)
- [x] Absorb shields (Ice Barrier, Power Word: Shield)

### Crowd Control

- [x] Root (Frost Nova) - prevents movement
- [x] Stun (Kidney Shot, Charge) - prevents all actions
- [x] Fear (Warlock) - target runs randomly, breaks on damage (100 threshold)
- [x] Polymorph (Mage) - target wanders slowly, breaks on ANY damage
- [x] CC indicators on combatants
- [x] CC breaks (Fear breaks on damage threshold, Polymorph on any damage)
- [x] Strategic CC targeting (separate cc_target from kill_target)
- [x] Heuristic CC target selection (healer priority, inverted when killing healer)

### Data-Driven Configuration

- [x] abilities.ron - All 22 ability definitions
- [x] AbilityDefinitions Bevy resource
- [x] Runtime balance changes without recompilation
