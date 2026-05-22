# Walking Animation Requirements

**Date:** 2026-05-21
**Scope:** Lightweight
**Status:** Ready for planning

## Problem

Combatants and pets currently slide rigidly across the arena — the `Capsule3d` mesh translates frame-to-frame without any locomotion cue, producing an "ice-skating" effect that makes the simulation feel less alive than it should. The units are basic shapes with no limbs, so traditional walk cycles aren't available.

## Goal

Add a simple, cheap visual signal that a unit is *walking* rather than gliding, without changing any combat behavior or headless simulation.

## What we're building

A subtle vertical sine bob applied to the capsule meshes of combatants and pets while they're actually moving. The bob's tempo couples to distance traveled, so:

- A unit at normal move speed bobs at a normal walking cadence
- A slowed unit (e.g. under Frost Nova's slow, Concussive Shot, Hamstring) automatically bobs more slowly
- A hasted/charging unit bobs faster
- A unit standing still (idle, casting, stunned, feared, polymorphed, dead) stands perfectly still

The bob amplitude should be small enough to read as "walking," not as a cartoony hop. Target feel: the capsule's base appears to lift ~5–10% of the unit's total height.

Per-unit phase offset so a team of three doesn't bob in lockstep.

## Scope

**In scope**
- Combatant capsules (spawned at `src/states/play_match/mod.rs:608`)
- Warlock pet capsules (spawned at `src/states/play_match/mod.rs:658`)
- Per-entity walk phase so units don't sync
- Distance-coupled tempo (one bob cycle per N units of travel)

**Out of scope**
- Projectiles, traps, ice blocks, healing columns, dispel bursts — any non-combatant capsule
- Idle "breathing" bob — units must be still when not moving (rejected to keep the moving/not-moving signal clean)
- Alternative animation styles (lean, sway, squash-and-stretch) — rejected in favor of pure Y bob
- Any change to movement speed, combat logic, AI behavior, or headless simulation output
- Limb-like motion (swinging arms, leg cycles) — explicitly excluded; basic shapes only

## Success criteria

- Watching a graphical match, a viewer can tell at a glance whether each unit is moving or stationary, just from the bob
- Slowed units visually waddle slower than unslowed units — the speed difference is *visible*, not just metric
- A 3v3 team starting to advance does not bob in unison; the phase offset makes the motion feel like distinct individuals
- Stunned, feared, polymorphed, and dead units stand still — no residual bob
- `cargo test` still passes — no impact on the system registration audit or any other test
- Headless mode produces byte-identical match logs to before the change (visual-only)

## Assumptions

- Combatants and pets share a stable ground Y position throughout a match (no flying units exist today, and none are planned in the immediate roadmap)
- Pets get the same treatment as combatants — same mesh family, same locomotion concept, same visual language

## Call-outs for planning

These need answers during planning, not now:

1. **Bob driver — distance-based vs time-based phase.** Distance-based ("one cycle per N units traveled") automatically scales tempo with actual movement and degrades gracefully under slows/charges. Time-based ("one cycle per N seconds") is simpler but requires explicit speed coupling. Recommend distance-based unless it creates an unforeseen visual problem at very high speeds (Charge).
2. **Cleanup on death/despawn.** When a unit dies mid-bob or is despawned, ensure the last rendered frame doesn't leave Y at a peak — reset to ground Y before the death visual triggers.
3. **High-speed legibility.** Warrior Charge produces fast translation over a short window. Confirm the bob still reads correctly at that speed and doesn't appear stroboscopic; if it does, consider damping amplitude or capping cycle rate.
4. **Registration path.** Per the project's dual-registration rule, this is a graphical-only visual effect — register in `StatesPlugin::build()` in `src/states/mod.rs`, NOT in `add_core_combat_systems()`. The `tests/registration_audit.rs` check should be satisfied by a visual-effect signature (e.g. taking `&mut Transform` plus visual-only resources).

## References

- Combatant spawn: `src/states/play_match/mod.rs:608` (`Capsule3d::new(0.5, 1.5)`)
- Pet spawn: `src/states/play_match/mod.rs:658` (`Capsule3d::new(0.35, 0.6)`)
- Movement systems: `src/states/play_match/combat_core/movement.rs` (all `transform.translation +=` sites)
- Visual effect pattern reference: `src/states/play_match/rendering/effects.rs`
- Visual effect conventions: `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md`
