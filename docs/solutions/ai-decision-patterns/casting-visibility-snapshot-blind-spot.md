---
title: "CombatSnapshot excludes mid-cast entities, so AI skips decision ticks against casting targets"
category: ai-decision-patterns
tags:
  - combat-snapshot
  - class-ai
  - casting
  - channeling
  - combat-context
  - balance
  - decision-tick
module: src/states/play_match/class_ai
symptom: "A class AI whose target is mid-cast produces no decision that tick; matchups vs casters/healers swing wildly when the blind spot is fixed"
root_cause: "CombatSnapshot::build filtered casting/channeling entities out of the combatants map, so target lookups returned None and the AI fell through"
date: 2026-06-07
---

# CombatSnapshot excludes mid-cast entities → AI skips decision ticks against casting targets

## Problem

`CombatSnapshot::build` populated the `combatants` map only from queries filtered
`Without<CastingState>` / `Without<ChannelingState>`. Any combatant that was
mid-cast or channeling was therefore **absent from the snapshot every other AI
reads**. A class AI whose current target was casting got `None` from
`ctx.combatants.get(&target)` / `ctx.target_info()` and fell straight through its
decision ladder — emitting **no action that tick**.

Casters cast most of the time, so against a Mage/Priest/Warlock the attacker was
silently idle for a large fraction of the match.

## Symptoms

- A matchup's winrate is decided "in the last second" and feels swingy run-to-run.
- An attacker visibly does nothing while its target is casting (no trace
  `ability_decision` for that actor on those ticks).
- A balance change with an apparently unrelated mechanism produces enormous
  matrix swings.

## Why it matters

This single blind spot was worth **±50 winrate points** once fixed (measured,
side-symmetrized N=100): Rogue v Priest 100% → 48%, Mage v Rogue 13.5% → 61%,
Warrior v Paladin 50.5% → 15.5%. The magnitude reflects how many matchups were
decided on the margin while the attacker idled.

It also caused a **second-order regression**: the Rogue's accidental idle ticks
had been pooling energy for free. Removing them (the fix below) collapsed Kidney
Shot usage 86/100 → 0/100 games, because Sinister Strike now drained energy
every tick and the 60-energy stun was never affordable. See
[[rogue-energy-pooling]] — a fix that only became necessary because this blind
spot was masking the AI's real (broken) energy economy. The lesson: an AI that
silently skips ticks can hide other AI bugs; fixing visibility surfaces them.

## Solution

Extend the `casting_auras` / `channeling_auras` queries in `decide_abilities`
(`src/states/play_match/combat_ai.rs`) with the components needed to build a full
`CombatantInfo` — `&Combatant`, `&Transform` — and have `CombatSnapshot::build`
insert casting/channeling entities into the `combatants` map alongside the
non-casting ones. The three source queries stay disjoint via their
`With`/`Without<CastingState/ChannelingState>` filters, so there is no borrow
conflict and no double-insert.

```rust
// Before: casting entities harvested for auras only — invisible as targets.
casting_auras: Query<(Entity, &ActiveAuras), With<CastingState>>,
// After: carry the components build() needs to construct a full CombatantInfo.
casting_auras: Query<(Entity, &Combatant, &Transform, Option<&ActiveAuras>), With<CastingState>>,
```

Files: `src/states/play_match/class_ai/combat_snapshot.rs`,
`src/states/play_match/combat_ai.rs`. Commit `b14b577`.

## Prevention

- **Treat "target missing from snapshot" as a red flag, not a no-op.** A class
  AI hitting `None` on its own target lookup should be rare; if it is the steady
  state, the snapshot is lying about the world.
- **This is a deliberate, balance-shifting change — measure it in isolation.**
  It alters what *every* class AI can see. Land it as its own commit and run the
  matrix before/after so the delta is attributable to nothing else (this is what
  the U4 unit in the healer-movement plan did). Use the side-symmetrized protocol
  ([[mirror-asymmetry-side-symmetrized-measurement]]) for the deltas.
- **Pets are separate.** `pet_ai_system` builds its own local snapshot; it did
  not share this blind spot and was not changed. Don't assume one snapshot fix
  covers pet AI.
- Provenance / full mechanism dive: `docs/reports/2026-06-healer-movement.md`
  (U4 mechanism section).
