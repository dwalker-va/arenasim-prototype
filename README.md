# ArenaSim

A WoW Classic-inspired arena combat simulator built with Bevy (Rust). Configure teams of 1-3 combatants, then watch them battle with full combat mechanics including spells, crowd control, and resource management.

## Quick Start

**Prerequisites:** [Rust](https://rustup.rs/) (stable toolchain)

```bash
# Development build (first build takes ~5 min to compile Bevy)
cargo run

# Release build (optimized)
cargo run --release

# Headless simulation (no graphics)
cargo run --release -- --headless /tmp/match.json
```

## Features

- **5 Classes**: Warrior, Mage, Rogue, Priest, Warlock
- **22 Abilities**: Spells, heals, interrupts, crowd control, absorb shields
- **WoW Mechanics**: Mana/Energy/Rage, cooldowns, spell schools, cast times
- **Smart AI**: Target priority, ability rotation, interrupts, kiting
- **Match Config**: 1v1, 2v2, 3v3 with kill target strategy
- **Time Controls**: Pause, 0.5x-3x speed
- **Combat Log**: Real-time events + ability timeline

## Documentation

- **[CLAUDE.md](CLAUDE.md)** - Developer guide: project structure, adding abilities, running tests
- **[design-docs/roadmap.md](design-docs/roadmap.md)** - TODOs and milestones
- **[design-docs/game-design-doc.md](design-docs/game-design-doc.md)** - Game vision
- **[design-docs/wow-mechanics.md](design-docs/wow-mechanics.md)** - Implemented WoW mechanics
- **[design-docs/bevy-patterns.md](design-docs/bevy-patterns.md)** - Bevy/Rust patterns

## Tech Stack

- **[Bevy 0.15](https://bevyengine.org/)** - ECS game engine
- **[bevy_egui](https://github.com/mvlabat/bevy_egui)** - Immediate-mode UI
- **Rust** (stable)

## License

MIT

---

**Built with** [Bevy](https://bevyengine.org/) | **Inspired by** World of Warcraft Arena
