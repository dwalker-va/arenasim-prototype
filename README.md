# ArenaSim Prototype

A World of Warcraft-inspired arena combat simulator built with Bevy (Rust). Configure teams of fantasy combatants with unique abilities, then watch them battle in real-time with full WoW-style combat mechanics including spells, crowd control, and resource management.

## ✨ Features

### Combat System
- **4 Character Classes**: Warrior (melee tank), Mage (ranged caster), Rogue (stealth DPS), Priest (healer)
- **15+ Unique Abilities**: Cast-time spells, instant attacks, healing, crowd control, interrupts
- **WoW-Style Mechanics**: Mana/Energy/Rage resources, cooldowns, global cooldown, spell schools
- **Status Effects**: Roots, stuns, movement slows, healing reduction, damage over time
- **Advanced AI**: Targeting priority, ability rotation, interrupt timing, kiting behavior
- **Visual Feedback**: Health/resource bars, cast bars, floating combat text, status effect labels
- **Real-time Combat Log**: Scrolling event stream with color-coded damage/healing/death messages

### Match Configuration
- **Team Sizes**: 1v1, 2v2, or 3v3 arena battles
- **Character Selection**: Build team compositions from 4 classes
- **Kill Target Strategy**: Set focus targets for coordinated team play
- **Multiple Arenas**: Choose from different battlefield layouts

### Camera & Controls
- **3 Camera Modes**: Follow Center, Follow Combatant, Manual control
- **Full Camera Controls**: Pan (WASD/middle-drag), zoom (scroll/+/-), rotate (middle-drag)
- **Time Controls**: Pause, 0.5x, 1x, 2x, 3x simulation speeds
- **Customizable Keybindings**: Remap all controls to your preference

### Quality of Life
- **Settings Persistence**: Window mode, resolution, VSync saved between sessions
- **Match Logs**: Detailed combat logs automatically saved for analysis
- **Results Screen**: Post-match statistics with damage/healing breakdown

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (stable toolchain)
- On Linux: `libudev-dev`, `libasound2-dev` (Bevy dependencies)

### Build & Run

```bash
# Development build (first build takes ~5 min to compile Bevy)
cargo run

# Faster iteration with dynamic linking (subsequent builds)
cargo run --features dev

# Release build (optimized for performance)
cargo run --release
```

## Project Structure

```
arenasim-prototype/
├── src/
│   ├── main.rs                    # Entry point, plugin registration
│   ├── states/
│   │   ├── mod.rs                 # Game state orchestration (menu, options, match, results)
│   │   ├── play_match.rs          # Combat simulation (5400+ lines - abilities, AI, rendering)
│   │   ├── configure_match_ui.rs  # Match setup UI
│   │   ├── results_ui.rs          # Post-match statistics
│   │   └── match_config.rs        # Team configuration data
│   ├── combat/
│   │   ├── mod.rs                 # Combat system exports
│   │   └── log.rs                 # Combat event logging
│   ├── camera/
│   │   └── mod.rs                 # Camera controller modes
│   ├── settings.rs                # Persistent game settings
│   ├── keybindings.rs             # Customizable input mappings
│   └── ui/
│       └── mod.rs                 # Shared UI utilities
├── assets/
│   ├── fonts/
│   │   ├── Rajdhani-Bold.ttf      # UI font (fantasy-themed)
│   │   └── Rajdhani-Regular.ttf
│   ├── config/                    # Game data (currently unused - data is code-defined)
│   ├── models/                    # 3D models (procedurally generated meshes)
│   └── audio/                     # Audio (not yet implemented)
├── match_logs/                    # Auto-saved combat logs for debugging
├── settings.ron                   # Saved player preferences
└── design-docs/                   # Design documentation
    ├── game-design-doc.md         # Vision and requirements
    ├── tech-stack-decision.md     # Architecture rationale
    ├── project-todos.md           # Development progress tracking
    ├── stat-scaling-system.md     # Combat formula documentation
    └── egui-migration-summary.md  # UI architecture decision
```

## Controls

### Main Menu & UI
- **Mouse**: Click buttons to navigate menus
- **ESC**: Return to previous screen / close modals

### Match Simulation
#### Time Controls
- **Space**: Pause / Resume
- **1**: 0.5x speed (slow motion)
- **2**: 1x speed (normal)
- **3**: 2x speed (fast forward)
- **4**: 3x speed (very fast)

#### Camera Controls
- **TAB**: Cycle camera modes (Follow Center → Follow Combatant → Manual)
- **C**: Reset camera to default position
- **Mouse Wheel** / **+/-**: Zoom in/out
- **WASD**: Pan camera (in Manual mode)
- **Middle Mouse Drag**: Rotate camera or pan (context-dependent)

### Keybinding Customization
All controls can be remapped via **Options → Keybindings**

## Tech Stack

- **Engine**: [Bevy 0.15](https://bevyengine.org/) - ECS game engine in Rust
- **UI**: [bevy_egui 0.31](https://github.com/mvlabat/bevy_egui) - Immediate-mode GUI
- **Language**: Rust (stable)
- **Graphics**: Low-poly, flat-shaded, vertex-colored meshes (no textures)
- **Target Platforms**: PC (Windows, macOS, Linux), Steam Deck

## Design Documents

- [Game Design Document](design-docs/game-design-doc.md) - Core vision and feature requirements
- [Tech Stack Decision](design-docs/tech-stack-decision.md) - Why Bevy and code-first approach
- [Project TODOs](design-docs/project-todos.md) - Development progress and session notes
- [Stat Scaling System](design-docs/stat-scaling-system.md) - Combat formula documentation
- [egui Migration Summary](design-docs/egui-migration-summary.md) - UI architecture decision

## Development Approach

This project is designed for **agentic development** (AI-assisted code generation):

- **Pure code**: No visual editors or binary assets - everything is code or text files
- **Modular architecture**: Clear separation of concerns (combat, UI, camera, state management)
- **Comprehensive documentation**: Inline comments, design docs, and session notes
- **Deterministic**: Procedural generation and simple assets make behavior predictable
- **Debuggable**: Match logs saved automatically, extensive console logging

## Character Classes & Abilities

### Warrior (Melee Tank/DPS)
- **Resource**: Rage (gained from dealing/taking damage)
- **Abilities**: Auto-attack (melee), Charge, Heroic Strike, Mortal Strike, Rend (DoT), Pummel (interrupt)
- **Playstyle**: Gap closer, sustained pressure, interrupt enemy casters

### Mage (Ranged Caster/Control)
- **Resource**: Mana
- **Abilities**: Wand Shot (ranged), Frostbolt (slow), Frost Nova (AoE root)
- **Playstyle**: Kite melee, control positioning, burst damage from range

### Rogue (Stealth Melee DPS/Control)
- **Resource**: Energy
- **Abilities**: Auto-attack (melee), Ambush (from stealth), Sinister Strike, Kidney Shot (stun), Kick (interrupt)
- **Playstyle**: Stealth opener, high burst, lockdown key targets

### Priest (Healer/Support)
- **Resource**: Mana
- **Abilities**: Wand Shot (ranged), Flash Heal, Mind Blast, Power Word: Fortitude (HP buff)
- **Playstyle**: Keep allies alive, buff before combat, contribute damage when safe

## Roadmap

### Near-Term
- [ ] Additional abilities per class (6-8 abilities each)
- [ ] More arenas with environmental obstacles
- [ ] Sound effects and music
- [ ] Gear system (stat scaling foundation is ready)

### Mid-Term
- [ ] Talent system (simplified WoW-style talent trees)
- [ ] Advanced camera cinematics
- [ ] Detailed post-match analysis (damage/healing breakdowns)
- [ ] Replay system

### Long-Term
- [ ] AI opponent strategies (configurable team behaviors)
- [ ] Tournament mode (bracket progression)
- [ ] Steam Deck optimization
- [ ] Modding support (expose ability/character configs)

## Contributing

This project is primarily developed via agentic workflows (AI-assisted coding). However, feedback, bug reports, and design suggestions are welcome via GitHub Issues.

## License

MIT - See LICENSE file for details

---

**Built with** [Bevy](https://bevyengine.org/) • **Inspired by** World of Warcraft Arena
