---
title: "feat: Walking animation for combatant and pet capsules"
type: feat
status: active
depth: lightweight
created: 2026-05-21
origin: docs/brainstorms/2026-05-21-walking-animation-requirements.md
---

# feat: Walking animation for combatant and pet capsules

## Summary

Add a subtle vertical sine bob to combatant and pet `Capsule3d` meshes while they're moving. The bob's tempo couples to actual distance traveled, so slowed units waddle slowly, hasted units bob faster, and idle/stunned/dead units stand perfectly still. Visual-only, graphical mode only, no impact on combat logic or headless output.

## Problem Frame

Combatants and pets currently translate frame-to-frame without locomotion cue, producing an "ice-skating" effect. With basic shapes and no limbs, traditional walk cycles aren't available — we need a simple, cheap visual signal that distinguishes walking from gliding. (See origin: `docs/brainstorms/2026-05-21-walking-animation-requirements.md`.)

## Requirements

Carried forward from the origin requirements doc:

- **R1.** A bob signal makes it visually obvious which units are moving and which are stationary
- **R2.** Bob tempo couples to actual distance traveled — slowed units bob slowly, hasted units bob faster
- **R3.** Per-unit phase offset so a 3v3 team does not bob in lockstep
- **R4.** Idle, casting, stunned, rooted, and dead units stand perfectly still (no residual bob)
- **R5.** Feared and polymorphed units do bob — they are moving, just unpredictably (this is the natural fallout of the distance-coupled rule, not a special case; reconciles an earlier ambiguity in the origin doc where these were listed as "stand still")
- **R6.** Visual-only — no impact on combat logic, AI, movement speed, or headless simulation. Headless match logs remain byte-identical
- **R7.** Pets (Voidwalker, Hunter pets) get the same treatment as combatants
- **R8.** `cargo test` continues to pass, including the `tests/registration_audit.rs` check

## Key Technical Decisions

### Distance-driven phase, not time-driven

Drive phase advancement by horizontal distance traveled per frame (`phase += distance_xz / step_length`), not by elapsed time. Rationale:

- A slowed unit covers less horizontal distance per frame → phase advances slower → bob naturally slows. No explicit speed coupling needed.
- A charging unit covers more distance → bob naturally speeds up.
- A stationary unit covers zero distance → phase doesn't advance → no bob.
- A stunned, rooted, polymorphed-but-blocked, or dead unit also covers zero distance → no bob.
- A feared unit covers normal distance (per `combat_core/movement.rs:142`) → bobs at normal cadence.

This single rule unifies every case the origin doc enumerates. Resolves origin call-out 1.

### Bob lives on the parent Transform's Y, with a captured `ground_y`

Each combatant/pet receives a `WalkAnim { ground_y, phase, previous_xz }` component at spawn. `ground_y` is captured from the spawn position. Each frame the bob system overwrites `transform.translation.y = ground_y + sin(phase * TAU) * amplitude` when moving, or `= ground_y` when idle. This keeps the bob entirely contained in the parent transform — no child entity restructuring required. Existing movement systems do `transform.translation += direction * move_distance`, which can produce tiny Y components when the target is mid-bob; the bob system overwrites Y every frame so no drift accumulates.

### Per-entity phase offset from entity bits

Initialize `phase` at spawn from the entity's index/generation bits modulo TAU. Gives each unit a deterministic but unique starting phase, so a team of 3 advancing together does not bob in lockstep. No RNG needed (preserves headless determinism guarantees in case `WalkAnim` is ever attached in headless mode).

### Graphical-mode-only registration

Register `update_walk_animation` in `StatesPlugin::build()` in `src/states/mod.rs`, NOT in `add_core_combat_systems()`. Walk animation has zero gameplay effect — headless mode must not run it (and headless mode doesn't need it, since there's no renderer). Resolves origin call-out 4.

Per the project's `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md` pattern: visual systems live in `states/mod.rs`, the `tests/registration_audit.rs` audit allows this when the system signature is consistent with a visual-effect system (taking `&mut Transform` + a visual marker query, no combat components).

### Ordering: after CombatResolution

Order the bob system `.after(CombatSystemPhase::CombatResolution)`. All movement systems live in earlier phases, so by the time the bob system runs, `transform.translation` reflects the unit's actual post-movement position for this frame. Reading `previous_xz` (set last frame, also post-movement) gives a clean delta.

## Implementation Units

### U1. Define `WalkAnim` component and attach to combatant / pet spawns

**Goal:** Introduce the `WalkAnim` marker-with-state component and ensure every combatant and pet entity carries it from spawn.

**Requirements:** R1, R3, R7

**Dependencies:** none

**Files:**
- `src/states/play_match/components/visual.rs` — add `WalkAnim` struct
- `src/states/play_match/mod.rs` — attach `WalkAnim` to combatant spawn (~line 608) and pet spawn (~line 658)

**Approach:**
- Add `WalkAnim` as a `#[derive(Component)]` struct holding `ground_y: f32`, `phase: f32`, `previous_xz: Vec2`
- At combatant spawn (in `spawn_combatant`): capture `position.y` as `ground_y`, derive `phase` deterministically from the entity bits or a per-spawn counter modulo `std::f32::consts::TAU`, set `previous_xz` from `position.xz()`. Spawn the component alongside existing combatant components in the tuple.
- At pet spawn (in `spawn_pet`): same logic, capture `position.y` as `ground_y`. Pets use a smaller capsule but the animation parameters can be identical — distance coupling auto-scales.
- The phase offset source must be deterministic (entity index, spawn order counter, or position hash) — not RNG — to avoid touching headless determinism guarantees.

**Patterns to follow:**
- Visual marker components in `src/states/play_match/components/visual.rs` (see `PlayMatchEntity`, `SpeechBubble`)
- Per-entity state attached at spawn (see `FloatingTextState`, `DRTracker` at `mod.rs:594-597`)

**Test scenarios:**
- Build verification: `cargo build --release` succeeds
- Component is present on every combatant after spawn — a unit test or assertion-in-test that spawns a combatant via the existing test helpers and queries for `WalkAnim`
- Pets receive `WalkAnim` with `ground_y` matching their spawn Y (Warlock pet)
- Two combatants spawned at the same position receive **different** initial phases (verifies per-entity offset is working)
- Headless match log byte-identical to pre-change for a seeded match (verifies R6 — no determinism impact)

**Verification:** A graphical match starts, no panics, both teams render at their normal positions. (Bob behavior verified in U2.)

---

### U2. Implement `update_walk_animation` system and register in graphical mode

**Goal:** Drive the actual bob — read post-movement XZ, advance phase by distance, overwrite Y. Register in `StatesPlugin::build()` so it runs only in graphical mode.

**Requirements:** R1, R2, R3, R4, R5, R6, R7, R8

**Dependencies:** U1

**Files:**
- `src/states/play_match/rendering/effects.rs` — add `update_walk_animation` (or a sibling file `walk_animation.rs` if effects.rs is getting crowded; implementer decides)
- `src/states/mod.rs` — register the system in `StatesPlugin::build()` after CombatResolution

**Approach:**
- System signature: `Res<Time>` (NOT `Time<Real>` — match the codebase convention), `Query<(&mut Transform, &mut WalkAnim, &Combatant)>`
- Per-frame loop:
  1. `let current_xz = transform.translation.xz()`
  2. `let distance = (current_xz - walk_anim.previous_xz).length()`
  3. If `!combatant.is_alive()` OR `distance < WALK_IDLE_EPSILON`: set `transform.translation.y = walk_anim.ground_y`, update `previous_xz = current_xz`, continue
  4. Otherwise: `walk_anim.phase = (walk_anim.phase + distance / WALK_STEP_LENGTH * TAU) % TAU`
  5. `transform.translation.y = walk_anim.ground_y + walk_anim.phase.sin() * WALK_BOB_AMPLITUDE`
  6. `walk_anim.previous_xz = current_xz`
- Constants (suggested starting values, tune in playtest):
  - `WALK_BOB_AMPLITUDE: f32 = 0.10` (capsule full height is ~2.5, so ~4% of height — subtle, reads as "walk")
  - `WALK_STEP_LENGTH: f32 = 1.5` (one full bob cycle per ~1.5 arena units traveled — at base movement speed this should feel like a comfortable walk cadence)
  - `WALK_IDLE_EPSILON: f32 = 0.001` (per-frame XZ delta below this counts as "not moving")
- Pet query: the system needs to cover both `Combatant` and pet entities. Pets in this codebase are also entities with `Combatant` components (per the spawn code), so a single query over `Combatant` covers both. Verify this against the actual pet spawn shape during implementation.
- Registration in `src/states/mod.rs`:
  ```
  .add_systems(
      Update,
      play_match::update_walk_animation
          .after(CombatSystemPhase::CombatResolution)
          .run_if(in_state(GameState::PlayMatch)),
  )
  ```
  (Pseudocode — directional only. Match the existing block style.)
- Re-export: if a new file is created, ensure `pub use rendering::*` chain in `src/states/play_match/mod.rs` picks it up. If added to `effects.rs`, no new export needed.

**Patterns to follow:**
- Visual-effect system pattern: `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md` (spawn/update/cleanup — though this case is update-only, since the lifecycle matches the parent entity's lifecycle)
- Update systems in `effects.rs`: `update_drain_life_beams`, `update_healing_light_columns`, `update_dispel_bursts` — all use `Res<Time>`, plain `Query<&mut T>`, no `Without<>` needed here since we only touch the unit's own Transform
- System registration: existing `.after(CombatSystemPhase::CombatResolution)` blocks in `src/states/mod.rs:150-250`

**Test scenarios:**
- **Happy path — moving unit bobs.** In a graphical match (or via a headless probe wrapping the system), a unit walking toward a target has `transform.translation.y` oscillating between `ground_y` and `ground_y + WALK_BOB_AMPLITUDE` over time. Covers R1, R2.
- **Slow vs normal speed visual difference.** A unit under Frost Nova's slow aura visibly bobs at a slower cadence than an unslowed unit. Verify in a 1v1 graphical match by eye, or by sampling Y values across frames and comparing peak-to-peak period. Covers R2.
- **Idle unit stands still.** A unit standing on its spawn position (no target, or stunned) has `transform.translation.y == ground_y` exactly, for at least several frames. Covers R4.
- **Stunned mid-walk → freezes mid-air-impossible.** A unit walking, then hit with Hammer of Justice mid-bob, snaps to `ground_y` on the next frame (distance becomes 0). Covers R4.
- **Dead unit stands still.** A unit whose `combatant.is_alive() == false` has `translation.y == ground_y` regardless of any previous phase. Covers R4 / origin call-out 2.
- **Feared unit bobs (not stand-still).** Documents the R5 reconciliation: feared units DO bob because they're moving via fear locomotion. Covers R5.
- **Per-entity phase offset.** Two combatants spawned at the same X/Z, walking in lockstep toward the same target, do NOT have identical Y values in the same frame. Covers R3.
- **Charging Warrior bobs visibly without strobing.** Watch a Warrior Charge in graphical mode and confirm the bob reads as a fast walk, not a flicker. If strobing occurs, dampen amplitude at high speed or cap phase advance per frame. Covers origin call-out 3.
- **Registration audit passes.** `cargo test` runs the `tests/registration_audit.rs` check and passes. If it flags `update_walk_animation`, add it to the audit's `ALLOWLIST` with a "visual-only animation system, registered in StatesPlugin::build()" justification, OR confirm the audit's signature heuristic already classifies it as visual-only. Covers R8.
- **Headless determinism.** Run a seeded headless match before and after this change. Match logs are byte-identical. Covers R6.

**Verification:**
- `cargo build --release` succeeds
- `cargo test` passes, including the registration audit
- `cargo run --release` launches a graphical match, both teams visibly bob while moving and stand still when idle/stunned/dead
- Seeded headless replay (`cargo run --release -- --headless /tmp/test.json`) produces byte-identical output to a pre-change baseline run

## Scope Boundaries

### In scope
- Combatant capsules (`src/states/play_match/mod.rs:608`)
- Warlock and Hunter pet capsules (`src/states/play_match/mod.rs:658`)
- Per-unit phase offset
- Distance-coupled tempo

### Outside this product's identity
- Limb animation, swinging arms, leg cycles — units are basic shapes by design
- Lean, sway, squash-and-stretch, or any animation style other than vertical bob (rejected during brainstorm)

### Deferred for later
- Idle "breathing" bob (rejected during brainstorm — keeps the moving/not-moving signal clean)

### Deferred to Follow-Up Work
- None — work is small enough to land in a single PR

### Out of scope
- Projectiles, traps, ice blocks, healing columns, dispel bursts — non-combatant capsules
- Any change to movement speed, AI behavior, or combat logic
- Any change to headless simulation output

## Assumptions

- Combatants and pets share a stable ground Y throughout a match (verified by reading `combat_core/movement.rs` — all movement is XZ-only; no current ability flies a unit). If a future ability introduces vertical motion (e.g., a leap), `ground_y` will need to update — flagged for future awareness, not for current scope.
- Pets in this codebase carry a `Combatant` component (verified at `mod.rs:658` via `Combatant::new_pet`). The bob system's `Query<&Combatant>` covers them automatically. Implementer should confirm during U2.

## Risks

- **Visual strobing during Warrior Charge.** Charge moves the warrior fast; at small `WALK_STEP_LENGTH`, phase could advance more than TAU per frame, producing aliasing. **Mitigation:** cap per-frame phase advance to `TAU * 0.5` (half a cycle), which produces a smooth fast bob instead of strobing. Address in U2 if observed.
- **Registration audit flags the new system.** The audit at `tests/registration_audit.rs` enforces that `pub fn` taking SystemParam types are registered in `add_core_combat_systems`, `StatesPlugin::build()`, or `ALLOWLIST`. Since this system is registered in `StatesPlugin::build()`, it should pass — but if the audit's heuristic misclassifies (e.g., because it takes `&mut Transform` which is also used in core combat), add an `ALLOWLIST` entry. **Mitigation:** explicit allowlist entry with justification if needed.
- **Determinism regression.** Any code path that affects gameplay logic in headless mode would violate R6. The bob system never runs in headless mode (registered in `StatesPlugin::build()`, not `add_core_combat_systems`), and the `WalkAnim` component carries no gameplay data, so this risk is low. **Mitigation:** run a seeded headless match before/after as part of U2's verification.

## References

- Origin: `docs/brainstorms/2026-05-21-walking-animation-requirements.md`
- Visual effect pattern: `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md`
- Combatant spawn: `src/states/play_match/mod.rs:608`
- Pet spawn: `src/states/play_match/mod.rs:658`
- Movement systems: `src/states/play_match/combat_core/movement.rs`
- System phase ordering: `src/states/play_match/systems.rs` (`CombatSystemPhase`)
- Registration audit: `tests/registration_audit.rs`
- Visual marker components: `src/states/play_match/components/visual.rs`
- Combatant alive check: `Combatant::is_alive()` at `src/states/play_match/components/combatant.rs:325`
