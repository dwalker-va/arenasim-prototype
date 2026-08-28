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

The tiered per-ability animation plan gives a couple of signature abilities per class a bespoke, recognizable-at-a-glance animation, with lower tiers sharing a cheaper vocabulary. The pilot was Polymorph (PR dwalker-va/arenasim-prototype#103, plan at `docs/plans/2026-08-10-001-feat-polymorph-signature-animation-plan.md`): sheep body swap, hop gait, transform puffs. It shipped in roughly one session, which is the cost signal the pilot existed to measure. Fear (#107), Lightning Bolt (#111) and Mortal Strike have since shipped on this procedure, then the Root/Stun receiver treatment (A) and the four caster-side gestures of A2. **The next candidate list lives in `design-docs/roadmap.md` under *Animation tier walk: the next candidates*** — it came out of removing the in-combat ability speech bubbles, which had been standing in as the only cue for several abilities, and it is ordered by a structural finding rather than by ability: an ability that is instant AND aura-only has neither generic caster-side hook (`CastingState` → casting orb, `QueuedInstantAttack` → `InstantAbilityFired`), and the receiver side has no treatment for `Root`, `Stun` or `MovementSpeedSlow`, so one receiver-side piece of work covers four abilities at once.

**Amendment, Mortal Strike (2026-08-22).** The first signature for a TRUE INSTANT, and it broke two of the procedure's assumptions:

- **Step 1's "the caster side already has generic coverage" is false for instants.** Mortal Strike is applied inline in `class_ai/warrior.rs` and resolved in `combat_ai.rs`'s `QueuedInstantAttack` drain — it never enters `CastingState`, so neither the casting orb nor the `CastEnding` marker exists for it, and the weapon socket keeps running its auto-attack cycle straight through the ability. An instant's caster side has NO coverage; the stroke is the work.
- **A signature stroke needs a different arc PLANE, not a bigger one.** The first attempt scaled the auto-attack's pitch by a depth multiplier, which produces the same chop, louder. `SwingProfile` now carries a `SwingArc`, and `SwingStyle::Auto` reproduces the shipped constants and the sagittal pose exactly (pinned by `the_auto_style_reproduces_the_shipped_constants` and `the_auto_arc_is_the_untouched_sagittal_pose`), so auto-attacks are provably unchanged.
- **But build that plane by TILTING one rotation axis, never by composing several.** The second attempt added yaw and roll on top of pitch and read as the axe being turned rather than swung — see [swing-is-one-rotation-one-axis.md](swing-is-one-rotation-one-axis.md) for why, and for the cheap assertion that catches it. Both wrong turns cost a round trip through the graphical client, which is the only place either was visible.

Two further lessons worth carrying forward:

- **Ground the look in the real ability before designing.** Mortal Strike's WoW animation is a rising diagonal ("bottom left to top right") and Classic-era warrior ability visuals are carried by a weapon trail, not by target-side gore. Both facts came from a fifteen-minute look at source material and both reversed a design that had already been agreed.
- **A pre-implementation visual bench pays for itself.** The arc, timing and colour were settled in an interactive HTML bench that ported `swing_param`/`swing_pose` verbatim, before any Rust existed. Every tuning value shipped as a named const straight out of it.

**The body-animation ceiling, and how it lifted (2026-08-24).** For a melee signature the weapon was originally the only signal there was, which capped how much any stroke could read regardless of shape. Mortal Strike's arc was pushed to ~2.8 rad on a plane leaned 49° — larger than the auto-attack's and unmistakably diagonal — and the remaining gap was not reachable by exaggerating further.

`animate_body_lean` (`rendering/effects/weapon_swing.rs`) lifted it for the whole melee tier at once. The body turns about the swing's **own** axis ([`arc_rotation`]) by `SwingProfile::lean` of its angle, driven by the same `s` the weapon uses — so wind-back and drive-through fall out of one input, with no second curve to keep in sync. Three things make it work:

- **The hierarchy does half the job.** A `WeaponSocket` is a CHILD of the `VisualBody`, so the lean composes onto the weapon's arc instead of sitting beside it: the blade's world sweep grows as well as the body moving. Most of the improvement comes from this, not from the torso being separately visible.
- **Per-style amplitude is what preserves the distinction.** An auto-attack leans ~8°, a signature ~22°. A routine swing gains weight without gaining ceremony, and every future signature inherits body motion for free rather than being another stroke against the same ceiling.
- **Channel ownership decided the design.** `translation.y` belongs to the gaits and the victory bounce, so the weight shift is HORIZONTAL only, where nothing was writing; rotation was unclaimed except by the death fall, which the lean cedes to — clearing the horizontal step on the way out, since nothing else writes it and a unit killed mid-swing would keep the offset on its corpse. Before adding a body treatment, work out which Transform channels are already spoken for; the free one shapes what the treatment can be.

**Amendment, A2 (2026-08-28) — research the source for EVERY ability, not just the one you feel unsure about.** Four caster-side gestures shipped together, and reading the actual Classic client data (DB2 tables, then the parsed M2 models and BLP textures) reversed the design for three of them. Kidney Shot is a lunging `Attack1HPierce`, **not** the kick everyone remembers — the kick is the Rogue's *Kick*. Cheap Shot is so generic it shares its visual with Sap and has zero colour tracks. Hammer of Justice has **no hammer at all**: `HasMissile = 0`, and it is internally named *FistOfJustice* — a flat gold ground streak, not a thrown weapon. Every one of those was something the model had asserted confidently from memory first.

Three durable lessons:

- **Prose sources are nearly useless for this; client data is excellent.** Wowhead comments are JS-loaded and unfetchable, and the one promising prose hit turned out to be an April Fools joke. The DB2 → `SpellVisualKit` → M2 chain gives exact animation names, geometry bounds, colour keyframes and timings.
- **Ask what the ability is, not how to draw what you already pictured.** The two rogue stuns are byte-identical on the receiver side and differ ENTIRELY on the caster side (pierce vs swing, magenta vs untinted white, torso vs head, 1233ms vs 634ms). Collapsing them into one shared stroke — which was the cheaper design, and the one initially recommended — would have discarded the only thing separating them.
- **A negative finding is a design constraint.** "No hammer" meant `swing_style_for_ability` must return `None` for Hammer of Justice, which in turn exposed that the router could not give a flourish to an ability with no stroke. The research found a code bug.

**The receiver side does not have to be a body treatment.** Mortal Wounds — a 10s `HealingReduction` debuff — gets no visual on the victim at all. It states itself by breaking incoming heals: a heal landing on an afflicted target sheds the refused share as ash (`rendering/effects/mortal_wounds.rs`), keyed at the three sites that already apply the reduction. This is cheaper than a body treatment AND more legible, because it fires exactly when the debuff costs someone something — and it sidesteps the whole `OriginalBodyMaterial` contention family (see `shared-restore-slot-mutual-exclusion.md`) by never touching the body. Prefer it whenever a debuff's meaning is "some other mechanic is now worse."

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

- Any new signature-ability animation, onward through the tier walk
- Any aura-driven body treatment (CC states: stun slump, root ice, frozen tint)
- Instant-ability flourishes hang off `InstantAbilityFired` (renamed from
  `InstantAttackLanded` in A2), spawned by combat code at each ability's own
  resolution site and dispatched by `rendering/effects/instant_ability.rs`. The
  marker carries `caster` and `target: Option<Entity>` — `None` for a
  caster-centred effect — and `InstantAbilityFired::is_spawned_for` is the single
  list both combat code and the animation sandbox derive from, so they cannot
  drift.
- **Swing dispatch and flourish dispatch are INDEPENDENT, and must stay so.** They
  were nested once, with the flourish reachable only through a `Some(style)` and a
  `Some(target_pos)`. That silently excluded two whole shapes of ability: one with
  a flourish and no weapon stroke (Hammer of Justice — the source spawns no
  hammer), and one that is caster-centred with no target at all (Frost Nova).
  Both would have spawned a marker, been consumed, and drawn nothing.
- Adding a signature to an ability that ALREADY reaches the drain (Mortal Strike,
  Ambush, Sinister Strike) costs one arm in `swing_style_for_ability` and one in
  the flourish match. An ability that does NOT — anything instant AND aura-only —
  needs a marker spawned at its own application site first, which is a
  combat-file edit (see `cosmetic-marker-cross-mode-spawn-parity.md`), plus an
  arm in the sandbox's `Residue` family or it will never preview.
- **`SwingArc` is not only for swings.** The "build a signature plane by tilting
  ONE rotation axis, never composing several" rule governs how to shape a SWING.
  A thrust traces no plane at all, so it needs its own arc kind — `SwingArc::Lunge`
  is translation-dominant along the aim axis, added for Kidney Shot's
  `Attack1HPierce`. Expressing a pierce as a rotation reads as a slash however it
  is tuned.

## Examples

The Polymorph pilot itself: `update_polymorph_visuals` + `spawn_sheep_parts` (`src/states/play_match/rendering/effects/polymorph.rs`), `update_sheep_hop` (`rendering/effects/gait.rs`), and the transform-puff trio (`rendering/effects/transform_puffs.rs`); probes in `tests/polymorph_visual_probes.rs`, sandbox staging in `src/states/animation_sandbox/playback.rs`.

## Related

- `adding-visual-effect-bevy.md` — the underlying three-system lifecycle every transient effect uses
- `fixed-timestep-visual-strobe.md` — why gaits are distance-driven and never gated on "sim moved this frame"
- `aura-driven-visual-exit-paths.md` — the two exit-path traps (component removal, death) any aura-keyed visual must handle
- `animation-sandbox-cc-entry-gotchas.md` — staging requirements for reviewing CC entries in the sandbox
- `cosmetic-marker-cross-mode-spawn-parity.md` — when a core-side marker IS needed (state not readable at render time)
- `byte-identical-module-split.md` — how the `effects.rs` split (this procedure's prerequisite) was done and verified, and the traps a large-module split reliably hits
