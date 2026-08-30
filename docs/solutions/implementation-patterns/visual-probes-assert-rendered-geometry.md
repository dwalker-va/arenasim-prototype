---
title: "Visual Probes Assert Rendered Geometry, Not Stored Parameters"
date: 2026-08-29
category: implementation-patterns
module: states/play_match/rendering/effects
problem_type: convention
severity: high
applies_when:
  - "Writing or reviewing a probe for a visual effect that is aimed, spans two points, or spawns as a group"
  - "A visible defect shipped past a fully green test suite"
  - "Sampling a procedurally generated texture in a test"
symptoms:
  - "The probe asserts a length or scale field and the effect still points the wrong way"
  - "An effect reads correctly from one camera bearing and wrongly from another"
  - "A test keeps passing after a proportion change that moved the feature it samples"
tags:
  - visual-effects
  - testing
  - probes
  - world-geometry
  - animation
  - bevy-ecs
---

# Visual Probes Assert Rendered Geometry, Not Stored Parameters

## Context

The A2 animation work (PR #115, open as of this writing) produced four separate
visible defects that a fully green suite waved through. Every one had a probe pointed at it. Every probe passed,
because each asserted a value the code had *written down* rather than where the
thing actually *ended up*:

| Defect | What the probe asserted | What it missed |
|---|---|---|
| A ground streak yawed 90° off, lying across the line of fire | `HolyStreak.length == 8.0` | orientation |
| The same streak reaching half as far as it claimed, its tail through the caster | `Transform.scale.x` grew over time | world position |
| A crescent fan clumped at one point beside the caster | the members' `roll` values differed | world extent |
| Slashes running head-to-toe down the victim | the streak was near the axis it had been given | the on-screen result |

The last is the most instructive. The probe asked *"is the streak aligned to the
aim?"* — and it was, exactly as implemented. But the aim projects to almost
precisely screen-vertical for the usual over-the-shoulder camera, so "aligned to
the aim" and "running head to toe" were the same thing. The test confirmed the
implementation's intent instead of the requirement, and passed through three
rounds of a human reporting the identical problem.

This is the third documented class of animation defect that survives every
mechanical check, alongside
[the wandering swing axis](swing-is-one-rotation-one-axis.md) and
[the fixed-timestep strobe](fixed-timestep-visual-strobe.md). They share a
shape: nothing panics, headless stays byte-identical, and the only thing that
notices is a human looking at the screen.

## Guidance

**Assert the rendered result. A stored parameter is the input to the thing you
care about, not the thing itself.**

Concretely, by effect shape:

- **Anything aimed** — assert the rotated basis vector against the target
  direction, not that some field holds the direction:
  `(rotation * Vec3::X).dot(aim) > 0.99`. Use whichever local axis actually
  carries the feature (see the axis rule below). Test **several bearings**; one
  direction can pass by coincidence.

- **Anything spanning two points** — compute the world head and tail and assert
  them against the endpoints. A `length` field says what the code believed, not
  where the mesh reached. Remember that Bevy's `Rectangle` and `Cuboid` are
  **centred on their origin**, so scaling alone spans `±extent/2` about the
  anchor; growing from a fixed end means walking the centre out by half the
  current extent.

- **Anything spawned as a group** (a fan, a burst, a ring of parts) — assert the
  group's **world extent**, not that its members' parameters differ. Members can
  hold four distinct roll values and still sit at one point.

- **Anything with lateral extent that is also aimed** — place the target on at
  least two different axes and assert the layout *rotates* with it. A spread
  along a fixed world axis collapses to nothing for half of all bearings.

- **Procedural textures** — sample relative to the feature's **actual** radius or
  span, never a hardcoded coordinate. A test sampling at `0.86` keeps passing
  while measuring empty sprite once the proportions move.

**Know which local axis carries the feature, and pick the matching yaw.** Both
conventions live in this repo and both are correct for their own mesh:

| Mesh feature runs along | Yaw | Example |
|---|---|---|
| local **+X** (length of a `Rectangle`/`Cuboid`) | `atan2(-dz, dx)` | `spawn_arena_walls`, `src/states/play_match/mod.rs:487` |
| local **+Z** (a unit's facing) | `atan2(dx, dz)` | `src/states/play_match/combat_core/movement.rs:158` |

Using the facing convention on a length-along-+X mesh is a silent 90° error —
the mesh renders, it just lies across the direction it should follow.

**When no world axis works, assert the screen-space property instead.** A
horizontal world vector projected to the screen degenerates whenever it points
near the view axis. For an effect that must read the same from every bearing —
and for a **billboarded** sprite, which is camera-facing by construction anyway —
the honest requirement is the on-screen result, and the probe should say so.

**Prove the probe fail-first.** Run it against the broken version before
committing the fix. A probe written to pass will restate the implementation, and
the four defects above are what that looks like in practice.

## Why This Matters

A visual defect that ships is not caught by the type system, the borrow checker,
the determinism pin, or code review — reviewers read the same intent the author
wrote. It is caught by a human looking at the screen, days later, describing it
in words that have to be translated back into geometry. The head-to-toe slashes
took three rounds of that translation before the requirement was stated in terms
a probe could hold.

The cost asymmetry is stark: `(rotation * Vec3::X).dot(aim) > 0.99` is one line
and would have failed instantly on the 90° yaw error. The alternative was four
round trips through the graphical client.

There is a second-order cost too. A suite full of parameter assertions gives
false confidence — it is green, so attention goes elsewhere, and the defect
survives longer than it would have with no test at all.

## When to Apply

- Writing any probe for an effect with a direction, a span, or multiple parts
- Reviewing a visual probe that only reads component fields
- After any change to a sprite's or mesh's proportions — re-check every test that
  samples it by coordinate
- When a human reports a visual problem that the suite does not reproduce: the
  probe is very likely asserting the implementation rather than the requirement

Not needed for effects with no spatial claim — a colour, a lifetime, a spawn
count. Those *are* the stored parameter.

## How to Check It

```rust
// Aimed: assert the rotated basis against the aim, across several bearings.
for (dx, dz) in [(0.0, 8.0), (8.0, 0.0), (0.0, -8.0), (-5.0, 5.0)] {
    let aim = Vec3::new(dx, 0.0, dz).normalize();
    let length_axis = rot * Vec3::X;       // whichever axis carries the feature
    assert!(length_axis.dot(aim) > 0.99, "the effect does not follow its aim");
    let normal = rot * Vec3::Z;
    assert!(normal.y.abs() > 0.99, "the ground decal is not lying flat");
}

// Group: assert world EXTENT, not that parameters differ.
let lo = xs.iter().cloned().fold(f32::INFINITY, f32::min);
let hi = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
assert!(hi - lo > 1.0, "the fan spans only {}yd across a 1.0yd body", hi - lo);

// Screen-space: sweep the caster through a full circle so a fix that works
// from one angle cannot pass.
for i in 0..8 {
    let a = i as f32 / 8.0 * TAU;
    // ...place the target at that bearing, then:
    let axis = cam_rot.inverse() * (rot * Vec3::X);
    let mut off = axis.y.atan2(axis.x).abs();
    off = off.min(std::f32::consts::PI - off);   // a streak and its twin read alike
    assert!(off < 1.0, "not horizontal on screen at bearing {a}");
}
```

Two further habits worth the keystrokes:

- **Guard against vacuous drains.** `assert!(count > 0)` before asserting a
  cleanup reaches zero, or the drain check passes when nothing ever spawned.
- **Fold mirrored angles.** A streak and its 180° twin render identically, so
  compare on `[0, π/2]` rather than failing a correct effect for pointing the
  other way along its own axis.

## Examples

The probes that resulted, all in this repo:

- `the_streak_reads_horizontally_from_every_bearing`
  (`tests/rogue_stun_visual_probes.rs`) — eight bearings, screen-space assertion.
  Both earlier world-axis attempts fail it.
- `the_fan_sweeps_across_the_casters_breadth` (same file) — world extent against
  the body's own width.
- `the_fan_follows_the_aim_not_the_world_axes` (same file) — the target on two
  different axes, asserting the layout rotates.
- `the_caster_ring_stays_centred_on_the_paladin`
  (`tests/holy_justice_visual_probes.rs`) — world position across three bearings.
- `the_starburst_reaches_past_the_ring`
  (`src/states/play_match/rendering/effects/holy_justice.rs`) — catches a
  starburst whose rays were clipped *inside* their own ring's footprint, so they
  were drawn every frame and never visible.

Prior art that already followed the rule: `shatter_shards_fall_and_drain` in
`tests/fear_visual_probes.rs` asserts shards' **world Y** drops under gravity
rather than reading a stored velocity field — see
[physics-lite-debris-particles.md](physics-lite-debris-particles.md).

Historical note: the aimed-streak defects above were on a Hammer of Justice
ground streak that **no longer exists** — it was replaced by a caster-centred
expanding wave once reference imagery showed the source has no projectile. The
fixed aimed-effect assertions now live on Kidney Shot's slash.

## Related

- [swing-is-one-rotation-one-axis.md](swing-is-one-rotation-one-axis.md) — its
  `How to Check It` is this same world-geometry assertion applied to a rotation
  axis, and it is where the fail-first rule was first written down
- [signature-ability-animation-procedure.md](signature-ability-animation-procedure.md)
  — the procedure this extends; its step 7 is the probe checklist
- [aura-driven-visual-exit-paths.md](aura-driven-visual-exit-paths.md) — the
  **lifecycle** half of visual-probe coverage; this is the **geometry** half
- [fixed-timestep-visual-strobe.md](fixed-timestep-visual-strobe.md) — the third
  defect class that passes every mechanical check and is visible only to a human
- [adding-visual-effect-bevy.md](adding-visual-effect-bevy.md) — the three-system
  lifecycle every effect under test uses
- [animation-sandbox-cc-entry-gotchas.md](animation-sandbox-cc-entry-gotchas.md)
  — the sandbox is where all four defects were actually caught; better probes
  narrow what reaches it, they do not replace it
