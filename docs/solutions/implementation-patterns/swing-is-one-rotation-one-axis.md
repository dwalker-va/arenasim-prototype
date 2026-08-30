---
title: "A Swing Is One Rotation About One Axis"
date: 2026-08-23
category: implementation-patterns
module: states/play_match/rendering/effects
problem_type: design_pattern
severity: medium
applies_when:
  - "Giving an ability a bespoke weapon stroke that must differ in SHAPE from the auto-attack"
  - "Any animation that composes rotations to build a diagonal, arcing, or off-plane motion"
symptoms:
  - "The weapon reads as being TURNED or rotated rather than swung"
  - "A strike lands visually off-target even though the sim says it hit"
  - "An arc looks 'wrong' in a way that resists tuning the angles"
tags:
  - visual-effects
  - animation
  - weapon-swing
  - quaternions
  - signature-abilities
  - mortal-strike
---

# A Swing Is One Rotation About One Axis

## Context

The tiered animation walk gives signature abilities strokes that must be
recognisable against the auto-attack. The auto-attack (`swing_pose` in
`src/states/play_match/rendering/effects/weapon_swing.rs`) is a single pitch
rotation about the socket's X axis — a chop in the sagittal plane.

Mortal Strike's signature is a rising diagonal. The obvious way to build one is
to add the axes the auto-attack lacks: some yaw to carry the blade across the
body, some roll to turn the edge into the cut. That is what shipped first, and
it read as **the axe being turned rather than swung** — the one thing an
animation cannot afford to look like.

## Guidance

**A swing is one rotation about one axis. A diagonal swing is that same
rotation about a TILTED axis.** Not a chop plus a turn.

```rust
// WRONG — three axes composed. Reads as turning, not swinging.
Transform::from_rotation(
    Quat::from_rotation_y(yaw)
        * Quat::from_rotation_x(pitch)
        * Quat::from_rotation_z(roll),
)

// RIGHT — one rotation, one axis, leaned off vertical.
let axis = Quat::from_rotation_z(tilt) * Vec3::X;
Transform::from_rotation(Quat::from_axis_angle(axis, angle))
```

The composed version fails for a reason that is easy to state and hard to see
by inspection: **when each component angle scales independently with the swing
parameter, the composite rotation's axis MOVES through the stroke.** Measured on
the shipped-then-reverted Mortal Strike values, the axis drifted 37° between
windup and impact (axis alignment fell to 0.80). A rigid body rotating about a
wandering axis is tumbling. That is precisely what "turning rather than
swinging" describes, and no amount of retuning the three angles fixes it,
because the defect is structural.

**Two specific traps inside the general one:**

1. **A socket-frame Z rotation is a cartwheel, not a blade roll.** In the socket
   frame X is lateral, Y is up, Z is forward, so `from_rotation_z` swings the
   whole weapon sideways like a clock hand. To roll a blade about its own haft
   you would have to rotate about the weapon's local axis, which means composing
   on the RIGHT of the mount (`rest`) — and the swing pose is applied on its
   left. From inside the pose function, a genuine blade roll is not expressible.
2. **Never add yaw to a stroke.** `animate_weapon_swings` already composes
   `Quat::from_rotation_y(socket.yaw_local)` to aim the weapon at its target. A
   pose that adds its own Y rotation stacks on that and points the blade away
   from the victim *at the exact frame of impact* — the strike visibly misses
   what it hit.

**Corollary — the roll bought nothing anyway.** Its stated purpose was to orient
the weapon-trail ribbon. But the trail samples both its tip and inner edge along
the blade's own axis (`Vec3::Y * TRAIL_TIP_LOCAL` and the same axis a span
shorter), so a rotation about that axis provably cannot move either point.
Before adding a degree of freedom to fix a downstream visual, check whether the
downstream visual can observe it at all.

## Why This Matters

The failure is aesthetic, so it survives every mechanical check — the tests
passed, headless stayed byte-identical, the client did not panic, and reviewers
found nothing. It is only visible to a human watching the animation, and it
resists tuning, because turning the three knobs just moves a wandering axis
somewhere else. Reaching for a second rotation axis is the natural instinct when
an arc needs to leave a plane, which makes this worth knowing before the next
nine signatures rather than after.

## When to Apply

- Any signature-ability stroke that must differ in shape from the auto-attack
  (the remaining tier walk: Ambush, Kidney Shot, Execute, Hammer of Justice…)
- Any time an animation composes more than one rotation and the result "looks
  wrong" in a way that resists tuning — measure the axis before touching angles

## How to Check It

The invariant is cheap to assert and does not need a GPU or a human eye:

```rust
// The swing axis must not move through the stroke.
let reference = pose_at(1.0).rotation.to_axis_angle().0;
for s in [-1.0, -0.6, 0.25, 0.7, 1.0] {
    let (axis, angle) = pose_at(s).rotation.to_axis_angle();
    if angle.abs() < 1e-4 { continue; }         // axis is arbitrary at rest
    assert!(axis.dot(reference).abs() > 0.999); // parallel OR antiparallel
}
```

`.abs()` on the dot product is load-bearing: `(axis, angle)` and
`(-axis, -angle)` are the same rotation, and the sign flips when the stroke
crosses rest.

`the_signature_never_stacks_a_second_rotation_axis` in `weapon_swing.rs` is this
test, and it was proven fail-first against the reverted composition rather than
merely written to pass — worth doing, since a test that only restates the
current implementation would not have caught the bug it exists for.

## Examples

`SwingArc` in `src/states/play_match/rendering/effects/weapon_swing.rs`:
`Sagittal` (the auto-attack) and `TiltedPlane` (Mortal Strike) are the same
motion, and `Sagittal` is `TiltedPlane` at `tilt == 0` — pinned by
`the_sagittal_chop_is_the_zero_tilt_case`. The tilt alone produces the lateral
travel that the yaw was wrongly added for.

## Related

- `signature-ability-animation-procedure.md` — the procedure this belongs to; its
  Mortal Strike amendment records the same lesson from the design side
- `fixed-timestep-visual-strobe.md` — the other class of animation defect that
  passes every mechanical check and is visible only to a human watching
- `visual-probes-assert-rendered-geometry.md` — the general statement of this doc's `How to Check It`: assert the rendered world geometry, not the parameters the code stored
