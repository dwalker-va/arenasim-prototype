# Designing a Game for AI-Assisted Development

ArenaSim is a WoW-inspired arena combat simulator built almost entirely through AI-assisted development with Claude. This post documents the architectural decisions that made that practical and the problems we encountered along the way.

## Code-First Architecture

We chose Bevy (Rust) over Unity or Godot because it's purely code-driven. There are no binary scene files, no visual editors, no GUI workflows. Every game element—character stats, ability definitions, arena layouts—lives in version-controlled text files that an AI can read and modify directly.

The visual style reinforces this: low-poly meshes with vertex colors, no textures, no external 3D assets. The AI generates geometric primitives in code. No modeling tools required.

## Immediate-Mode UI

We initially used Bevy's built-in retained-mode UI system. After two screens and 8 bugs related to state synchronization and change detection, we switched to egui (immediate-mode). The same screen went from 968 lines to 240 lines, and the bugs disappeared.

Immediate-mode UI follows a simple pattern: "given this state, render this UI." Retained-mode requires spawning entities, tracking changes, synchronizing state, and despawning on cleanup. The former is straightforward for an AI to reason about. The latter has many failure modes.

## Headless Simulation

The most useful feature for AI-assisted development is headless mode. A complete arena match runs without graphics:

```bash
echo '{"team1":["Warrior"],"team2":["Mage"]}' > /tmp/test.json
cargo run --release -- --headless /tmp/test.json
```

The simulation runs at maximum speed and saves a detailed combat log. The AI can make a code change, run a test match, and verify the results without any manual intervention. When implementing absorb shields, the AI ran headless tests after each change to confirm the mechanic worked before I launched the graphical client.

## Structured Combat Logging

The combat log records every damage event, heal, crowd control effect, and death with structured data. The results screen queries this log to generate statistics. This makes features testable: implement Ice Barrier, run a Mage vs Warrior match, check if the log shows absorbed damage.

## Domain Knowledge via MCP

We integrated a Wowhead MCP tool that returns WoW Classic spell data—cast times, mana costs, damage values, spell schools. When implementing abilities, the AI pulls reference data instead of guessing values.

## Problems

**Visual positioning.** The AI cannot see the rendered output. Implementing shield bubble visuals required 6 iterations because assumptions about coordinate systems were wrong. Screenshots help but add friction.

**Multi-file debugging.** When systems interact unexpectedly, the AI needs to trace through multiple files. Focused modules and documentation help but don't eliminate the problem.

**Type system iteration.** Rust's compiler catches bugs early, which is valuable. But the AI sometimes writes logically correct code that doesn't satisfy the borrow checker, requiring back-and-forth.

## Potential Improvements

- Automated visual regression testing with screenshot comparisons
- Real-time combat state dumps for AI analysis
- Faster incremental compilation (full builds take ~2 minutes)

## Summary

The architecture choices that matter most:

1. Everything in code, no binary assets or visual editors
2. Immediate-mode UI over retained-mode
3. Headless execution for automated testing
4. Structured logging as a verification mechanism
5. Domain-specific tools for reference data

The current codebase has 5,500+ lines of combat logic, 22 abilities across 5 classes, and full visual feedback. The architecture made AI-assisted development practical rather than theoretical.
