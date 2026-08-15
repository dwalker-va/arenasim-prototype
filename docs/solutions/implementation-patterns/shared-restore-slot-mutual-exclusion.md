---
title: "Two Body Treatments, One Restore Slot: Guard Them Mutually Exclusive"
date: 2026-08-15
category: implementation-patterns
module: states/play_match/rendering
problem_type: design_pattern
severity: high
applies_when:
  - "Adding a second aura-driven body treatment that swaps the same VisualBody material another treatment already swaps"
  - "Two visual effects share one 'original value' restore component (OriginalBodyMaterial, OriginalMesh, ...)"
  - "Any CC/status body treatment beyond the first in the tier (fear, stun, root, ...)"
symptoms:
  - "A unit stays stuck on the wrong tint/mesh for the rest of the match after two effects that both touched its body end"
  - "The 'original' handle a restore reads back is the OTHER effect's displaced value, not the true one"
tags:
  - visual-effects
  - auras
  - body-treatment
  - restore
  - mutual-exclusion
  - crowd-control
  - bevy-ecs
  - polymorph
  - fear
---

# Two Body Treatments, One Restore Slot: Guard Them Mutually Exclusive

## Context

The tiered animation walk gives crowd-control abilities bespoke body treatments that
**swap the `VisualBody`'s material** and stash the displaced handle in a single shared
component — `OriginalBodyMaterial` — to restore from when the effect ends. Polymorph
(sheep) shipped first as the only material-swapper. Fear (shadow husk, PR #107) was the
second. The moment a *second* treatment reuses that one shared restore slot, a
co-application bug appears that the first treatment never could: two displacers, one
slot. This is the third trap in the aura-driven-visual family, after the two in
[aura-driven-visual-exit-paths.md](aura-driven-visual-exit-paths.md) — and unlike those
(which are about *one* treatment's exit paths), this one is about *two* treatments
colliding.

## Guidance

**The trap.** Fear and Polymorph can be active on the same unit at the same time — this
is not a corner case:

- They are **different DR categories** (`AuraType::Fear => DRCategory::Fears`,
  `AuraType::Polymorph => DRCategory::Incapacitates`, `src/states/play_match/components/auras.rs:542-543`), so
  applying one does **not** replace the other (CC replacement only removes a same-category
  aura).
- Fear deals **no damage**, so applying Fear to a polymorphed target does not break the
  sheep (Polymorph breaks on any damage).
- `src/states/play_match/combat_core/movement.rs` already reads **both** `fear_direction` and
  `polymorph_direction` in one frame — the sim treats co-held Fear+Polymorph as a real,
  handled state.

With both treatments live and each guarding only on **its own** marker, whichever applies
**second** reads the *already-displaced* material (the husk, or the wool) and stores
**that** into `OriginalBodyMaterial`, overwriting the true handle the first treatment
saved. When both effects end, restore writes the wrong material back and removes the slot,
so the true material handle is gone — the unit is stuck on the wrong tint permanently.
The single slot cannot represent two stacked displacers.

**The fix — make the two treatments mutually exclusive at the query.** Each treatment's
combatant query excludes the *other's* marker, so exactly one treatment ever holds the
body (and the slot) at a time. Whichever grabs it first holds until it lifts; then the
other applies:

```rust
// update_fear_visuals
combatants: Query<(/* ... */), Without<PolymorphedVisual>>,   // sheep wins if already polymorphed
// update_polymorph_visuals
combatants: Query<(/* ... */), Without<FearedVisual>>,        // husk wins if already feared
```

Both directions are required. A guard on only one side (the original plan guarded only
`Without<PolymorphedVisual>`) fixes Polymorph-first but leaves Fear-then-Polymorph
corrupting the slot — which is exactly what code review caught.

**Two adjacent gotchas the wrong mental model produced:**

1. **The transition-in guard is the treatment's own MARKER absence, not
   `OriginalBodyMaterial.is_none()`.** Polymorph gates on `polymorphed_marker.is_none()`
   (`src/states/play_match/rendering/effects/polymorph.rs`), never on the shared component's absence. "Guard the
   `try_insert` on `OriginalBodyMaterial.is_none()`" is a tempting but wrong fix — it
   would let the second displacer *skip* storing (good) but still leave both effects
   fighting over one restore value on the way out. Mutual exclusion removes the conflict
   at the source instead.
2. **Marker removal is deferred (a `Command`).** Even with the guards, remember the
   arbitration is total only because the marker filter is evaluated per-frame; a treatment
   whose aura just ended still carries its marker for the frame the removal is queued, so
   the *other* treatment stays excluded until the next frame — which is the desired
   "hand off cleanly" behavior, not a bug.

## Why This Matters

The corruption is permanent, silent, and hits an ordinary ability pairing (a Mage sheeps a
target a Warlock already feared, or vice versa) — no panic, no log, just a combatant glued
to the wrong material for the rest of the match. It is invisible to a single-treatment test
suite: the bug only exists when two real systems run together, so a probe that fakes the
other treatment's marker (rather than running its system) cannot catch it. And it recurs by
construction — **every** new CC body treatment in the tier (stun slump, root ice, …) adds
another displacer to the same slot and must carry the mutual-exclusion guard against every
sibling, or pick a per-treatment restore component instead of sharing one.

## When to Apply

- Before adding any second-or-later aura-driven treatment that swaps the same `VisualBody`
  material/mesh another treatment swaps — add the mutual-exclusion guards up front.
- When two visual effects share any single "original value" restore component.
- Reviewing a CC-visual diff: check that the guard is **symmetric** (both queries exclude
  the other's marker), not one-directional.

## Examples

Fear + Polymorph, `src/states/play_match/rendering/effects/fear.rs` and `src/states/play_match/rendering/effects/polymorph.rs`: `update_fear_visuals`
carries `Without<PolymorphedVisual>`, `update_polymorph_visuals` carries the mirror
`Without<FearedVisual>`; both store into the shared `OriginalBodyMaterial`
(`src/states/play_match/components/visual.rs`). The regression probe
`fear_then_polymorph_does_not_clobber_material` in `tests/fear_visual_probes.rs` runs
**both** real systems and is fail-first-proven against the missing mirror guard (remove
`Without<FearedVisual>` and it fails at the "sheep deferred while feared" assertion).

## Related

- [aura-driven-visual-exit-paths.md](aura-driven-visual-exit-paths.md) — the two single-treatment exit-path traps (component removal, death-preserves-aura); this doc is the third trap, for *co-held* treatments.
- [signature-ability-animation-procedure.md](signature-ability-animation-procedure.md) — the signature-animation procedure whose "one marker owns the treatment" step this trap extends to the multi-treatment case.
