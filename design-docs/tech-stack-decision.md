# Tech Stack Decision Document

**Decision Date:** January 2, 2026  
**Status:** Approved

## Summary

This document captures the technology choices for the ArenaSim prototype, optimized for **agentic development** (AI-assisted code generation and modification).

---

## Game Engine: Bevy (Rust)

### Decision

We will use **Bevy** as our game engine.

### Rationale

| Criterion | Bevy | Why It Matters |
|-----------|------|----------------|
| **Code-first architecture** | ✅ Pure code, no binary editor | Agents excel at code generation; no manual editor work needed |
| **ECS structure** | ✅ Entity Component System | Logical, structured patterns that agents can reason about |
| **Type safety** | ✅ Rust's borrow checker | Compiler catches errors that agents might introduce |
| **Configuration format** | ✅ RON (Rusty Object Notation) | Human-readable text files, easy for agents to generate/modify |
| **Documentation** | ✅ Excellent | Agents can reference for accurate implementations |
| **Platform support** | ✅ PC + SteamDeck native | No wrapper or emulation needed |
| **Asset format** | ✅ GLTF/GLB | Text-based 3D format, agent-friendly |
| **License** | ✅ MIT/Apache 2.0 | No licensing complexity |

### Alternatives Considered

- **Godot 4**: Good option but still editor-oriented; scene files require visual editing for best results
- **Three.js/Babylon.js**: Pure code but web-first; SteamDeck deployment requires wrappers
- **Unity**: Binary scene format, heavy editor dependency, not suitable for agentic workflow

---

## Visual Style: Procedural Primitives + Minimal External Assets

### Decision

We will use a **procedural-first** approach for visuals, with minimal external asset dependencies.

### Rationale

Our design doc specifies:
- Low-poly, flat-shaded, primitive meshes
- Grid-aligned geometry
- Flat/vertex colors instead of textures
- Limited color palette

This visual style is **ideal for agentic development** because:

1. **Geometric primitives** can be generated in code - no 3D modeling tools needed
2. **Vertex colors** are just hex values - agents can easily manipulate them
3. **No UV mapping** complexity - no image editing tools required
4. **Grid alignment** is mathematical - precise, deterministic

### Asset Sources (When External Assets Are Needed)

| Source | Type | License | Notes |
|--------|------|---------|-------|
| **Kenney.nl** | 3D Models, Audio, UI | CC0 | Massive library, consistent low-poly style |
| **Quaternius** | 3D Characters | CC0 | Game-ready, low-poly |
| **Poly Pizza** | 3D Models | CC0 | Various low-poly assets |
| **Freesound.org** | Audio | Various CC | Sound effects and ambient |

### Asset Format Standards

- **3D Models**: `.glb` (binary GLTF) preferred for performance, `.gltf` for debugging
- **Audio**: `.ogg` for music, `.wav` for short sound effects
- **Configuration**: `.ron` (Rusty Object Notation)
- **Fonts**: `.ttf` or `.otf`

---

## Project Structure

```
arenasim-prototype/
├── Cargo.toml                 # Rust/Bevy dependencies
├── src/
│   ├── main.rs               # Entry point
│   ├── lib.rs                # Library root (optional)
│   ├── states/               # Game states (menu, match, results)
│   ├── combat/               # Combat system, abilities, buffs
│   ├── characters/           # Character definitions, stats
│   ├── ui/                   # UI components
│   ├── camera/               # Camera controls
│   └── utils/                # Utilities, logging
├── assets/
│   ├── models/               # 3D models (mostly procedural)
│   ├── audio/                # Sound effects, music
│   ├── fonts/                # UI fonts
│   └── config/               # RON configuration files
│       ├── characters.ron    # Character definitions
│       ├── abilities.ron     # Ability data
│       └── maps.ron          # Map configurations
└── design-docs/              # Design documentation
```

---

## Development Workflow

### Agentic Workflow Principles

1. **Everything as code**: Avoid manual editor work; all game logic and data in version-controlled files
2. **Data-driven design**: Game balance and configuration in RON files, not hardcoded
3. **Clear module boundaries**: Each system (combat, UI, camera) in its own module
4. **Comprehensive logging**: Combat log as a first-class feature, useful for debugging
5. **Incremental builds**: Bevy's fast compile times with dynamic linking during development

### Bevy-Specific Patterns

- Use **States** for scene management (Menu, ConfigureMatch, PlayMatch, Results)
- Use **Events** for combat actions (damage dealt, ability used, buff applied)
- Use **Resources** for global data (match configuration, combat log)
- Use **Queries** for entity iteration (all combatants, all buffs on a combatant)

---

## Dependencies (Initial)

```toml
[dependencies]
bevy = "0.15"

[dev-dependencies]
# For faster compile times during development
# Consider bevy's dynamic_linking feature
```

### Recommended Plugins (Add As Needed)

- `bevy_egui` - Immediate-mode UI, great for debug tools and complex menus
- `bevy_asset_loader` - Structured asset loading
- `bevy_kira_audio` - Advanced audio (if needed beyond Bevy's built-in)

---

## Validation Checklist

| Requirement | Solution | Status |
|-------------|----------|--------|
| PC + SteamDeck | Bevy native Linux/Windows | ✅ Ready |
| Keyboard/Mouse + Gamepad | Bevy input system | ✅ Ready |
| Low-poly 3D | Procedural meshes + vertex colors | ✅ Ready |
| Combat system (buffs, abilities) | ECS with components | 🔲 To Build |
| Camera controls | Bevy camera + custom systems | 🔲 To Build |
| Combat log | Event system + UI | 🔲 To Build |
| Statistics/Results | Query + aggregation | 🔲 To Build |
| UI (menus, HUD) | bevy_ui or bevy_egui | 🔲 To Build |

