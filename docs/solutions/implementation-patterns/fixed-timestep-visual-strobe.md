---
title: "Fixed-Timestep Visual Strobe: Never Gate Visuals on Per-Frame Sim Movement"
date: 2026-08-06
category: implementation-patterns
module: states/play_match/rendering
problem_type: convention
severity: high
applies_when:
  - "Writing any per-frame visual system (Update schedule) in a fixed-timestep sim"
  - "Animating something attached to a moving combatant (weapons, orbs, particles)"
  - "Diagnosing visual jitter, vibration, or strobing that the sim math says should not exist"
tags:
  - visual-effects
  - fixed-timestep
  - strobe
  - animation
  - walk-bob
  - bevy-ecs
  - rendering
---

# Fixed-Timestep Visual Strobe: Never Gate Visuals on Per-Frame Sim Movement

## Context

The sim runs in `FixedUpdate` (fixed tick rate); visuals run in `Update` (render frame rate, often 2x+ faster). On frames where no sim tick fired, every "did the sim move since last frame?" check reads *zero movement* — so any visual gated on it flickers between its moving and idle states at render frequency. The walk-bob animation vibrated bodies +/-0.1yd for months this way before the cause was found; the diagnosis required a temporary per-frame `GlobalTransform` probe and a frame-to-frame delta script, because by-eye theories kept fingering the wrong code.

## Guidance

- **Never gate a per-frame visual on "sim moved since last frame".** A `delta < epsilon` idle check strobes at render fps. If an idle state is needed, accumulate it over time (`idle_time += dt; if idle_time > threshold`), never per-frame.
- **Drive animation from `Res<Time>` delta accumulation.** Growth curves, easing, mote travel, and ending timers all advance by `time.delta_secs()` in `Update` — smooth at any render rate, and consistent with every other visual system (never `Time<Real>`).
- **Compose attachment motion in the parent's local frame.** Bob, sway, or orbit relative to a moving body is calculated in the parent's local space and left to transform propagation. World-space stabilization of an attached visual against a per-tick-snapping parent re-introduces the strobe.
- **Diagnose numerically, not by theory.** For any jitter report: add a temporary per-frame `GlobalTransform` probe, log positions, script the frame deltas, and look for sign reversals. Delete the probe after.

## Why This Matters

The failure is invisible in code review and in headless tests (headless has no render frames), ships silently, and reads as "the game feels off" rather than as a bug — the walk-bob strobe survived months and multiple by-eye "fixes" that touched the wrong system. The rule is cheap to follow at write time and expensive to rediscover: the casting-orb systems in PR dwalker-va/arenasim-prototype#96 applied it deliberately (`update_casting_orbs` advances growth/sputter/flash purely from `Res<Time>` and recomputes position every frame with no movement gate) and shipped strobe-free on the first graphical smoke.

## When to Apply

Every new system registered in `Update` that positions, scales, fades, or toggles anything visible — especially anything attached to or following a combatant. Not applicable to `FixedUpdate` sim code, which by definition runs once per tick.

## Examples

Wrong — strobes at render fps (the historical walk-bob shape):

```rust
// On non-tick frames delta is zero -> pose snaps to rest -> vibrates.
if (transform.translation - anim.last_pos).length() < 0.001 {
    reset_to_rest(&mut body);
}
```

Right — time-accumulated animation, no movement gate (casting orb, `src/states/play_match/rendering/effects/casting_orbs.rs`):

```rust
let dt = time.delta_secs();
orb.ending_remaining -= dt;                       // timers accumulate real frame time
orb_transform.translation = casting_orb_anchor(   // position recomputed every frame,
    caster_transform.translation, target_pos);    // never gated on "did it move"
```

## Related Issues

- `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md` — the base pattern (`Res<Time>`, spawn/update/cleanup) this rule extends
- `docs/solutions/implementation-patterns/cosmetic-marker-cross-mode-spawn-parity.md` — the companion pattern for *triggering* visuals from sim events
- `docs/solutions/implementation-patterns/visual-probes-assert-rendered-geometry.md` — a sibling defect class: like the strobe, it survives every mechanical check and is visible only to a human watching
