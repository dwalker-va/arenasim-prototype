---
title: "Aura-Driven Visuals: The Two Exit-Path Traps"
date: 2026-08-12
category: implementation-patterns
module: states/play_match/rendering
problem_type: architecture_pattern
severity: high
applies_when:
  - "Any graphical system that applies a visual while an aura is active and restores it when the aura ends"
  - "Writing probes for an aura-keyed visual's restore contract"
symptoms:
  - "Visual applies correctly but never restores when the aura expires naturally"
  - "A unit killed while the aura is active keeps the visual on its corpse, then it pops off seconds later"
tags:
  - visual-effects
  - auras
  - polymorph
  - restore
  - bevy-ecs
  - exit-paths
  - death
---

# Aura-Driven Visuals: The Two Exit-Path Traps

## Context

An aura-driven visual (body swap, tint, particle attachment) has more exit paths than the obvious "aura expired": damage break, dispel, natural expiry, and death. Two of these have non-obvious mechanics that make a naive implementation silently wrong. Both were latent in the original Polymorph cuboid swap and were found and fixed in PR dwalker-va/arenasim-prototype#103; Fear's terror treatment (the next CC visual) hits both again.

## Guidance

**Trap 1 — `ActiveAuras` is removed, not emptied.** `update_auras` removes the whole component once the last aura expires (`commands.entity(entity).remove::<ActiveAuras>()`, `src/states/play_match/auras.rs:85-87`). A visual system whose query *requires* `&ActiveAuras` silently drops the entity the moment that happens — its restore branch never observes the aura's absence, so natural expiry never restores. Take the component optionally:

```rust
// required &ActiveAuras never sees natural expiry — the component is gone
Option<&ActiveAuras>          // in the query
auras.is_some_and(|a| a.auras.iter().any(|au| au.effect_type == AuraType::Polymorph))
```

This matches the weapon-socket idiom that was already correct.

**Trap 2 — death preserves the aura.** `process_aura_breaks` skips dead combatants (`src/states/play_match/auras.rs:737-741`), so a killing blow never triggers break-on-damage: the aura stays on the corpse and ticks out over its remaining duration. Any visual keyed purely on aura presence therefore persists on the corpse (a sheep sinking instead of the restored body; weapons staying hidden until the aura expires and then popping back). Death must count as an exit path:

```rust
let is_active = combatant.is_alive() && auras.is_some_and(/* ... */);
```

and the restore branch must run while `DeathAnimation` is present (no `Without<DeathAnimation>` filter on the swap/restore system — the death sink and the restore must compose in the same frame window).

**Corollary — one predicate, one owner.** Every visual keyed on the same aura (body, weapon sockets, gait) must read the *same* state, ideally a marker the swap system owns (`PolymorphedVisual`), or the predicates drift: the pilot's socket-hiding stayed aura-only after the body swap gained the `is_alive` conjunct, producing the weaponless-corpse-then-pop artifact until review caught it.

## Why This Matters

Both traps fail silently and permanently — no panic, no log line, just a unit stuck in the wrong body forever (trap 1 hits any target whose only aura was the visual's aura, a common case) or a corpse wearing the visual for seconds (trap 2 hits every lethal break). Probes catch both cheaply once you know to write them.

## When to Apply

- Every new aura-driven visual, before it ships: cover *component removed*, *vec emptied*, and *death with aura still present* in its probes (`tests/polymorph_visual_probes.rs` shows all three)
- When an aura visual "sometimes doesn't reset" — check trap 1 first

## Examples

`update_polymorph_visuals` in `src/states/play_match/rendering/effects/polymorph.rs` implements both fixes; `restores_on_death_with_aura_still_present` and the component-removal branch of `transforms_and_restores` in `tests/polymorph_visual_probes.rs` pin them.

## Related

- `signature-ability-animation-procedure.md` — the full procedure this trap list belongs to
- `adding-visual-effect-bevy.md` — the general visual-effect lifecycle
