---
title: "Physics-Lite Debris Particles: Momentum for Dynamic Break/Impact/Death Effects"
date: 2026-08-15
category: implementation-patterns
module: states/play_match/rendering
problem_type: design_pattern
severity: medium
applies_when:
  - "A visual effect wants pieces that fly out, tumble, and fall — shatter, debris, sparks, blood, death poofs"
  - "A static flash/burst reads as 'starting an animation then abandoning it' and needs momentum instead"
  - "Extending the visual vocabulary beyond material swaps, scaled primitives, and float-fade particles"
tags:
  - visual-effects
  - particles
  - debris
  - shatter
  - gravity
  - determinism
  - bevy-ecs
  - reusable-pattern
---

# Physics-Lite Debris Particles: Momentum for Dynamic Break/Impact/Death Effects

## Context

The effect vocabulary in this game had three shapes — a **material swap** (polymorph husk,
fear husk), a **scaled primitive** (shrouds, flashes that grow and fade), and a
**spawn-float-fade particle** (fear motes, affliction drips: they drift at constant
velocity and fade). None carry *momentum*: nothing accelerates, arcs, or tumbles, so
every "break" or "impact" reads as a thing appearing and vanishing rather than coming
apart. The Fear break (PR #107) needed the shroud to look like it *shattered*, which is
the first effect here to add gravity + rotation. That fourth shape — a **physics-lite
debris particle** — is a reusable primitive: shatter is one preset of it, and Frost Nova
cracking, weapon-impact debris, and death poofs are others.

## Guidance

A debris particle is the float-fade particle plus **acceleration and spin**. The recipe,
as three systems around one component (mirrors the fear-mote / affliction-drip trio in
`src/states/play_match/rendering/effects/fear.rs` and the lifecycle in
[adding-visual-effect-bevy.md](adding-visual-effect-bevy.md)):

**1. The component** carries the integrator state (`FearShard` in
`src/states/play_match/components/visual.rs`):

```rust
#[derive(Component)]
pub struct FearShard {
    pub velocity: Vec3,          // outward + up at spawn; gravity pulls it down
    pub angular_velocity: Vec3,  // tumble about each axis (rad/sec)
    pub lifetime: f32,
    pub initial_lifetime: f32,
}
```

**2. The spawn helper**, called from a transition branch of a marker-owning system, emits
`N` fragments (a ring around the source) with an outward+up launch velocity and a jittered
tumble. It runs off a **deterministic visual-only hash**, never `GameRng`:

```rust
// per-fragment seed → hash, NOT GameRng (touching GameRng breaks headless byte-identity)
let seed = unit_pos.x.to_bits().wrapping_add(i.wrapping_mul(2_654_435_761));
let j = |k: u32| fear_mote_jitter(seed.wrapping_add(k));   // wrapping_mul/shift hash, 0..1
// ...ring position, outward+up velocity, jittered angular_velocity from j(...)
commands.spawn((FearShard { .. }, Mesh3d(pane), MeshMaterial3d(mat),
                Transform::from_translation(spawn), PlayMatchEntity));
```

**3. The update system** integrates ballistic motion, spins, and fades — **time-driven**
(`Res<Time>`), never gated on sim movement (the
[fixed-timestep-visual-strobe](fixed-timestep-visual-strobe.md) trap):

```rust
shard.lifetime -= dt;
shard.velocity.y -= GRAVITY * dt;            // acceleration
transform.translation += shard.velocity * dt; // integrate
transform.rotate(Quat::from_euler(EulerRot::XYZ, /* angular_velocity * dt */));
// fade material alpha by (lifetime / initial_lifetime)
```

**4. The cleanup system** despawns fragments whose `lifetime <= 0.0`.

Four properties make it correct in this codebase:

- **Transient, unattached, self-expiring.** Debris is a `PlayMatchEntity` world particle —
  NOT owner-scoped, NOT a child of the source. It despawns itself on lifetime, so the
  source effect can end (its marker removed) with **no debris bookkeeping** — a key
  simplification over owner-scoped attachments like the fear shroud or sheep parts.
- **Graphical-only → headless byte-identical.** Register the trio only in
  `src/states/mod.rs`, never `systems.rs`. No sim state is read or written.
- **Deterministic variation, never `GameRng`.** All per-fragment randomness comes from a
  visual-only hash seeded per index. Drawing from the seeded `GameRng` would perturb the
  sim's RNG order and break determinism/headless byte-identity — the same rule the fear
  motes and affliction drips already follow.
- **Time-driven, not sim-gated.** Integration uses `Res<Time>` so debris falls smoothly at
  render rate regardless of the fixed sim tick.

## Why This Matters

Momentum is the difference between "a thing blinked" and "a thing broke." A grow-then-fade
flash reads as an animation *starting then abandoning*; debris that arcs out and falls
reads as a consequence. Because the recipe is a self-contained primitive with no source
coupling, reusing it is a **parameter swap** — fragment count, mesh shape (thin panes for
glass, cubes for rubble, slivers for sparks), gravity, launch speed, color, lifetime — not
a re-derivation. Named reuse targets: **Frost Nova cracking / breaking on damage** and
death poofs. Weapon-impact debris has since shipped (see Examples).

## When to Apply

- A "break", "impact", or "death" beat where pieces should fly and fall.
- When a static burst/flash isn't reading as dynamic enough.
- NOT for steady ambient particles (rising motes, DoT drips) — those are the simpler
  float-fade shape with no acceleration; reach for debris only when momentum is the point.

## Examples

The Fear shroud shatter: `spawn_fear_shatter` / `update_fear_shards` / `cleanup_fear_shards`
in `src/states/play_match/rendering/effects/fear.rs`, the `FearShard` component in
`src/states/play_match/components/visual.rs`, spawned from the break branch of
`update_fear_visuals`. The probe `shatter_shards_fall_and_drain` in
`tests/fear_visual_probes.rs` asserts the burst spawns a ring of shards, they fall under
gravity (min Y drops), and they drain to zero — the testable contract for any debris preset
(count, falls, self-expires; appearance stays a sandbox-review concern).

**Weapon-impact debris** (the second preset, and the reuse target this doc named):
`MortalStrikeSpark` in `src/states/play_match/rendering/effects/mortal_strike.rs`, spawned
by `spawn_mortal_strike_flourish` at the contact point of a landed Mortal Strike. A pure
parameter swap off the shroud-shatter preset — slivers instead of panes, a tighter
up-and-outward cone instead of a ring, `AlphaMode::Add` instead of `Blend` because struck
metal glows where falling glass does not, and each spark oriented along its own velocity
rather than tumbling. Confirms the doc's claim: no new machinery, only constants and a
mesh shape.

A third variant in the same commit shows where the recipe's edge is.
`RefusedHealMote` (`src/states/play_match/rendering/effects/mortal_wounds.rs`) is debris-shaped — outward
velocity, downward acceleration, self-expiring — but deliberately opaque and unlit rather
than additive, because it represents healing being REFUSED and additive blending can only
add light. Momentum was the right primitive; the blend mode had to invert to carry the
meaning. When borrowing this preset, re-derive the material from what the effect means,
not from the preset it came from.

## Related

- [adding-visual-effect-bevy.md](adding-visual-effect-bevy.md) — the base spawn/update/cleanup three-system lifecycle every transient effect uses; debris is that lifecycle plus acceleration and spin.
- [fixed-timestep-visual-strobe.md](fixed-timestep-visual-strobe.md) — why the integrator is time-driven, never gated on sim movement.
- [signature-ability-animation-procedure.md](signature-ability-animation-procedure.md) — signature animations spawn transition effects like this from the marker-owning system's transition branches.
- [visual-probes-assert-rendered-geometry.md](visual-probes-assert-rendered-geometry.md) — this doc's shard probe is the prior art it generalises: assert the shards' world Y under gravity, never a stored velocity field.
