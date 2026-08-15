---
title: "Cosmetic Marker Entities: Cross-Mode Spawn Parity"
date: 2026-08-06
category: implementation-patterns
module: states/play_match/combat_core
problem_type: architecture_pattern
severity: high
applies_when:
  - "Graphical animation needs to react to a sim event (a landed hit, a cast ending) that leaves no readable component state behind"
  - "Adding any sim-to-render signal in a codebase with a headless byte-identity requirement"
tags:
  - visual-effects
  - headless
  - determinism
  - byte-identity
  - marker-entities
  - signal-pattern
  - bevy-ecs
---

# Cosmetic Marker Entities: Cross-Mode Spawn Parity

## Context

Graphical systems often need to know *how* a sim event resolved, but the sim's own state is gone by the time render code runs. Two shipped cases:

- `AutoAttackSwing` (PR dwalker-va/arenasim-prototype#94): the swing release animation must fire only for a hit that actually landed — intent-time state can't say that.
- `CastEnding` (PR dwalker-va/arenasim-prototype#96): `process_casting` (in `src/states/play_match/combat_core/casting.rs`) removes `CastingState` in pass 1 **before** pass 2 decides landed-vs-fizzled, so the two endings are indistinguishable from component state — a marker spawned at the resolution site is the only clean signal.

## Guidance

Spawn a bare, write-only marker entity in core combat code at the exact site where the event resolves, and follow all three legs:

1. **Spawn unconditionally in BOTH modes.** The spawn lives in shared core systems (`src/states/play_match/combat_core/`), never gated on graphical mode. Headless spawns the markers too and simply never reads them.
2. **Consume graphical-only.** The consumer (e.g. `consume_swing_signals` in `src/states/play_match/rendering/effects/weapon_swing.rs`, `consume_cast_ending_signals` in `rendering/effects/casting_orbs.rs`) is registered only in `src/states/mod.rs`, in `FixedUpdate` after `CombatSystemPhase::CombatResolution` — FixedUpdate can tick several times per rendered frame, and a marker consumed a tick late desyncs from its event. The consumer despawns each marker as it reads it.
3. **Tag `PlayMatchEntity`.** In headless, un-consumed markers accumulate for the match; the match-exit `PlayMatchEntity` sweep is what reclaims them. Without the tag they leak.

The marker carries only what the consumer needs to route (source entity + an outcome enum). Nothing in sim code ever queries the marker type — that is what keeps headless results byte-identical.

## Why This Matters

PR dwalker-va/arenasim-prototype#94's initial version got leg 1 wrong — spawn paths differed between modes — and produced a graphical/headless divergence that had to be fixed in review (commit message: "cross-mode spawn parity"; see also the pet-spawn-path seed divergence documented in the graphical/headless history). Gating the spawn on mode *feels* like an optimization but changes entity-allocation order between modes, which is exactly the class of divergence the byte-identity gate exists to catch. Spawning identically in both modes costs a handful of empty entities per match and buys provable parity: the fixed-seed match-log diff for PR #96 was byte-identical with all twelve `CastEnding` spawn sites active.

## When to Apply

Any time render code needs a sim event's *outcome* rather than its ongoing state — especially when the state component is removed before the outcome is decided, or when different endings (landed / fizzled / interrupted) must animate differently. Do **not** use markers when live component state already carries the signal (e.g. orb growth reads `CastingState.time_remaining` directly); markers are for edges, state is for levels.

## Examples

Spawn site (core, both modes — `src/states/play_match/combat_core/casting.rs`):

```rust
mana_charges.push((caster_entity, mana_cost));
commands.spawn((CastEnding { caster: caster_entity, kind: CastEndingKind::Landed }, PlayMatchEntity));
```

Consumer registration (graphical-only — `src/states/mod.rs`):

```rust
.add_systems(
    FixedUpdate,
    play_match::consume_cast_ending_signals
        .after(CombatSystemPhase::CombatResolution)
        .run_if(in_state(GameState::PlayMatch)),
)
```

Verification: the headless byte-identity gate — run the same fixed-seed config before and after the change and diff the match logs (see the Verification Contract in `docs/plans/2026-08-05-001-feat-casting-animations-plan.md`).

## Related Issues

- `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md` — the spawn/update/cleanup pattern the consumers follow
- `docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md` — why consumers register in `src/states/mod.rs` only
- `docs/solutions/implementation-patterns/fixed-timestep-visual-strobe.md` — the companion rule for *animating* what the markers trigger
