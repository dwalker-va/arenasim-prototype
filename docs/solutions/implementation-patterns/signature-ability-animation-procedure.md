---
title: "Signature-Ability Animation: The Repeatable Procedure"
date: 2026-08-12
category: implementation-patterns
module: states/play_match/rendering
problem_type: design_pattern
severity: medium
applies_when:
  - "Giving an ability a bespoke, glance-recognizable animation (the tiered animation walk: signatures bespoke, lower tiers recolors)"
  - "Adding any visual keyed on an aura or combatant state that must transform and restore a unit's body"
tags:
  - visual-effects
  - animation
  - polymorph
  - body-swap
  - gait
  - bevy-ecs
  - signature-abilities
---

# Signature-Ability Animation: The Repeatable Procedure

## Context

The tiered per-ability animation plan gives a couple of signature abilities per class a bespoke, recognizable-at-a-glance animation, with lower tiers sharing a cheaper vocabulary. The pilot was Polymorph (PR dwalker-va/arenasim-prototype#103, plan at `docs/plans/2026-08-10-001-feat-polymorph-signature-animation-plan.md`): sheep body swap, hop gait, transform puffs. It shipped in roughly one session, which is the cost signal the pilot existed to measure. Fear and Mortal Strike are the named next candidates.

## Guidance

The procedure that worked, in dependency order:

1. **Consider the receiver side first.** WoW-style readability comes as much from the *target's* reaction as the caster's motion — and the caster side already has generic coverage (casting orb, windup/release, projectiles). Polymorph's entire animation is receiver-side.
2. **One marker component is the single source of truth for every visual keyed on the same aura.** `PolymorphedVisual` gates the body swap, the weapon-socket hiding, and the gait selection. Visuals that each re-derive state from `ActiveAuras` drift out of sync (the socket-hide did exactly that until review finding: a corpse showed no weapons until the aura ticked out). The system that owns the swap inserts/removes the marker; everything else reads it.
3. **Body transformation rides the existing rails.** The `VisualBody` child's own `Mesh3d` swaps (restored from `OriginalMesh`); the displaced material is stored in a dedicated component on the body child (`OriginalBodyMaterial`, mirroring `OriginalWeaponMaterial`'s insert-at-swap / remove-at-restore lifecycle). Extra body parts spawn as children of the `VisualBody` tagged with an **owner-carrying marker** (`SheepPart { owner: Entity }`) so the restore despawn is scoped — two simultaneously transformed units must not strip each other's parts.
4. **Bake static pose offsets into mesh data when animation systems own the transform.** The walk bob, hop, and death sink all write the body child's transform, so it is unavailable for posing. `Sphere::new(1.0).mesh().scaled_by(...).translated_by(...)` bakes the squash and offset into the vertices instead (see the torso build in `update_polymorph_visuals`). Attached parts must reach *into* the volume, not stop at its bounding plane — an ellipsoid's underside curves away at the corners, which left the sheep's legs visibly detached until they were lengthened into the torso interior.
5. **Gait variants are separate systems arbitrated by marker filters.** `update_sheep_hop` runs `With<PolymorphedVisual>`; `update_walk_animation` gained `Without<PolymorphedVisual>` — mirroring the existing `Without<DeathAnimation>, Without<Celebrating>` exclusion idiom so exactly one system writes the body Y per frame. Both share `WalkAnim` state (via the extracted `advance_gait`/`apply_gait_offset` helpers) so the bob resumes on a live baseline at restore. `Without<VisualBody>` on the mover query is load-bearing — omitting it is a B0001 access-conflict panic at schedule init.
6. **One-shot transition effects follow the `DispelBurst` three-system template** (spawn on `Added<T>`/update/cleanup, `AlphaMode::Add`, `try_insert`, `PlayMatchEntity` tag). Spawn them from the marker-owning system's transition branches — if the state is readable there, no core-combat marker entity is needed and byte-identity holds by construction. Keep transition effects short (the puff is 0.45s) so DR-shortened or instantly-broken windows still read as distinct events.
7. **Pin the restore contract with headless probes** (`tests/polymorph_visual_probes.rs` is the template: `MinimalPlugins` + `AssetPlugin`, manual clock, systems added directly). Appearance is untestable, but transform-in, non-accumulation across repeats, every exit path, and owner scoping are all cheap assertions. See `aura-driven-visual-exit-paths.md` for the exit paths that MUST be covered.
8. **Review in the animation sandbox** — with the CC-entry staging gotchas in `animation-sandbox-cc-entry-gotchas.md` handled, it is a sub-minute loop.

## Why This Matters

The pilot's verified defects were all in steps 2-5 territory: exit paths that silently never restored, visuals drifting from the body state, floating attachments, and query-conflict panics. Following the procedure means the next signature (Fear's terror treatment is machinery-heavier — an aura-driven shake/tint/wisp vocabulary intended for reuse across the CC tier) inherits the solved shape instead of rediscovering it.

The standing prerequisite has shipped: `effects.rs` (~4.6k lines after the pilot) was split into per-effect submodules under `rendering/effects/` (PR dwalker-va/arenasim-prototype#106), so the next signature (Fear) lands in the cleanly-isolated `rendering/effects/{polymorph,gait,transform_puffs}.rs` neighborhood rather than a 4k-line file. See `byte-identical-module-split.md` for the split's byte-identity verification technique and the two traps it surfaced.

## When to Apply

- Any new signature-ability animation (Fear, Mortal Strike, and onward through the tier walk)
- Any aura-driven body treatment (CC states: stun slump, root ice, frozen tint)
- Instant-ability flourishes additionally need core-side cosmetic markers (`CastEnding` pattern) and sandbox Phase B — neither existed as of the pilot

## Examples

The Polymorph pilot itself: `update_polymorph_visuals` + `spawn_sheep_parts` (`src/states/play_match/rendering/effects/polymorph.rs`), `update_sheep_hop` (`rendering/effects/gait.rs`), and the transform-puff trio (`rendering/effects/transform_puffs.rs`); probes in `tests/polymorph_visual_probes.rs`, sandbox staging in `src/states/animation_sandbox/playback.rs`.

## Related

- `adding-visual-effect-bevy.md` — the underlying three-system lifecycle every transient effect uses
- `fixed-timestep-visual-strobe.md` — why gaits are distance-driven and never gated on "sim moved this frame"
- `aura-driven-visual-exit-paths.md` — the two exit-path traps (component removal, death) any aura-keyed visual must handle
- `animation-sandbox-cc-entry-gotchas.md` — staging requirements for reviewing CC entries in the sandbox
- `cosmetic-marker-cross-mode-spawn-parity.md` — when a core-side marker IS needed (state not readable at render time)
- `byte-identical-module-split.md` — how the `effects.rs` split (this procedure's prerequisite) was done and verified, and the traps a large-module split reliably hits
