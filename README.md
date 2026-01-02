# ArenaSim Prototype

An arena combat autobattler prototype built with Bevy (Rust). Players configure teams of combatants and watch them battle CPU vs CPU.

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (stable)
- On Linux: `libudev-dev`, `libasound2-dev` (for Bevy dependencies)

### Build & Run

```bash
# Development build (first build will take a while to compile dependencies)
cargo run

# Faster iteration with dynamic linking (after first build)
cargo run --features dev

# Release build (optimized)
cargo run --release
```

## Project Structure

```
arenasim-prototype/
├── src/
│   ├── main.rs           # Entry point
│   ├── states/           # Game states (menu, match, results)
│   ├── combat/           # Combat system
│   │   ├── components.rs # ECS components (Health, Combatant, etc.)
│   │   ├── events.rs     # Combat events (Damage, Healing, etc.)
│   │   ├── log.rs        # Combat logging
│   │   └── systems.rs    # Combat ECS systems
│   ├── camera/           # Camera controls
│   └── ui/               # User interface
├── assets/
│   ├── config/           # Game data (RON format)
│   │   ├── characters.ron
│   │   ├── abilities.ron
│   │   └── maps.ron
│   ├── models/           # 3D models (mostly procedural)
│   ├── audio/            # Sound effects and music
│   └── fonts/            # UI fonts
└── design-docs/          # Design documentation
    ├── game-design-doc.md
    ├── tech-stack-decision.md
    └── project-todos.md
```

## Design Documents

- [Game Design Document](design-docs/game-design-doc.md) - Vision and requirements
- [Tech Stack Decision](design-docs/tech-stack-decision.md) - Why Bevy, asset strategy
- [Project TODOs](design-docs/project-todos.md) - Development progress tracking

## Tech Stack

- **Engine**: [Bevy 0.15](https://bevyengine.org/) - Data-driven game engine in Rust
- **Language**: Rust
- **Config Format**: RON (Rusty Object Notation)
- **Target Platforms**: PC, SteamDeck

## Development Approach

This project is designed for **agentic development** (AI-assisted code generation):

- Pure code, no binary editors
- Data-driven with text-based config files
- Clear module boundaries
- Comprehensive documentation

## Controls (WIP)

- **WASD** - Move camera
- **+/-** - Zoom in/out

## License

MIT
