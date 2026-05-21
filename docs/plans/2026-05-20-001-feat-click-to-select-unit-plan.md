---
name: feat-click-to-select-unit
description: Click-to-select for combatants — left-click a unit to mark it as selected; selection is visually indicated by a ground ring at the unit's feet. Selection is independent of camera follow.
status: active
type: feat
created: 2026-05-20
plan_depth: standard
---

# feat: Click-to-Select Unit

## Summary

Allow the player to select a combatant by left-clicking it during a match. The selected unit is highlighted by a translucent cyan-white **selection ring on the ground at its feet**, with a gentle pulse animation. Selection is independent of the camera-follow target. Clicking on another unit moves the selection. Clicking on empty space (the floor) clears the selection. The selection survives unless the selected unit dies or the match ends.

This is a player-affordance feature only — no AI, combat, or gameplay logic is affected by the selection. Future plans will use it to drive a unit-detail panel, command queues, or other interactions.

---

## Problem Frame

The game currently has no way for the player to single out a combatant for inspection. The camera can be cycled to follow a unit, but follow-mode is awkward as a "selection" because it constantly recenters the view. The player needs a stable, lightweight way to mark a unit of interest without disturbing camera flow.

**Goal**: Let the player click any alive combatant to select it. Show selection with a visual that reads as "selected" instantly, in the WoW/RTS idiom this prototype draws from. Keep selection state cleanly separated from camera-follow state so future features can layer on top.

---

## Design Decision: The Selection Visual

The user asked: *"draws a box around the unit, indicating they have been selected. Think like a professional visual game designer to decide on a visual for this."*

**Chosen visual: a flat ring/disc on the ground at the unit's feet.**

Rationale:

| Option | Trade-off |
|---|---|
| Literal 3D wireframe box around the capsule | Fights the 3D space — reads as 2D UI awkwardly attached to a moving 3D object. Occludes the unit's mesh from many camera angles. |
| Screen-space bracket corners (Starcraft II style) | Works, but lives in the 2D HUD layer next to health bars / FCT / speech bubbles, which is already busy. Detaches from the unit on extreme camera pitch. |
| **Ground ring / selection disc (RTS + WoW convention)** | **Anchored in the game world. Follows naturally as the unit moves. Doesn't compete with the 2D HUD. Reads instantly as "selected" because of player priors from WoW, Starcraft, Age of Empires. Visible from the isometric camera angle used here.** |
| Outline shader on the capsule | Cleanest visually, but requires custom shader work in Bevy 0.15 — out of scope for a "draw a box" prototype task. |

The game's own README calls out WoW Classic as the visual reference. A ground ring is the dominant convention in that lineage and reads correctly from this prototype's isometric camera. The ring is a clean foundation that future selection-driven features (move-to commands, ability targeting, multi-select) can extend without rework.

**Visual specification:**

- **Shape**: Thin torus, lying flat on the ground (rotated 90° on X axis). Inner radius ≈ 0.75, outer radius ≈ 0.95 (slightly larger than the capsule's 0.5 radius footprint, so the ring frames the unit cleanly).
- **Color**: Neutral cyan-white (`Color::srgba(0.6, 0.9, 1.0, 0.6)`). Distinct from team colors (red/green of health bars) and spell-target colors (yellow/red FCT). Reads as UI, not as combat effect.
- **Emissive**: Mild cyan glow (`LinearRgba::new(0.4, 0.8, 1.2, 1.0)`) so the ring stays visible against the arena floor under all lighting.
- **Blending**: `AlphaMode::Add` — matches the codebase convention (per `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md`, avoids Z-fighting flicker).
- **Position**: At the selected unit's feet (`combatant.translation - Vec3::Y * 0.95` to place at ground level — combatant transforms are centered at ~y=1.0 with capsule half-height 1.0).
- **Y-offset**: +0.05 above ground to avoid Z-fighting with the arena floor plane.
- **Animation**: Gentle pulse — scale oscillates `1.0 ± 0.05` at ~1.5 Hz via `(time.elapsed_secs() * 3.0).sin()`. Subtle, not distracting.
- **No rotation**: The ring stays world-axis-aligned; it does not spin with the unit's facing.

This single ring is sufficient as the entire selection visual. No screen-space brackets, no extra glow on the capsule itself.

---

## Scope Boundaries

### In Scope

- Left-click an alive combatant → that combatant becomes selected.
- Left-click on the floor or empty space → selection cleared.
- Left-click a different combatant → selection moves.
- A ground ring appears at the selected unit's feet for as long as selection persists.
- Ring follows the unit as it moves.
- Selection auto-clears (and ring despawns) when the selected unit dies.
- Selection auto-clears on match exit.
- Headless mode is unaffected (no input, no rendering).

### Out of Scope (Deferred to Follow-Up Work)

- Right-click as deselect, keyboard shortcuts for selection cycling.
- Multi-select (shift-click, box-drag-select).
- Showing the selected unit's stats / abilities / aura list in a side panel — the ring is the entire visual indicator for now.
- Driving the camera off the selection (the user explicitly called this out: "selected target is distinct from the target the camera is following").
- Hover highlights, click feedback flash, or selection sound effects.
- Touch / gamepad input.
- Selecting pets (Hunter pet, Warlock minion) — for now selection is restricted to entities with the `Combatant` component. Pets can be revisited once we know whether a unit-detail panel needs them.

### Not a Goal

- Click is not a gameplay action — the player does not "command" the selected unit (no move-to, no ability targeting). Selection is purely informational/visual.

---

## Key Technical Decisions

### Click vs. drag disambiguation

The camera already binds **left-click-drag** to rotate yaw/pitch (see `src/states/play_match/camera.rs:117-145`). A naive selection-on-click would steal every camera drag. The clean disambiguation pattern:

- On `MouseButton::Left::just_pressed` (and not over egui), record the cursor's press position into the existing `CameraController` state (or a new sibling resource).
- On `MouseButton::Left::just_released`, compute the cursor's travel distance from press to release. If the distance is below a threshold (≈5 px), treat the gesture as a **click** and fire selection picking. Otherwise it was a drag — do nothing.
- The existing camera-drag logic continues to update yaw/pitch during the press; it just does not interact with the selection system.

This keeps both behaviors on the same button without modal state. It mirrors the standard pattern used by RTS games on PC.

### Picking: screen-space projection, no raycasting

Bevy 0.15 has no built-in 3D picking. Adding `bevy_mod_picking` is overkill for a single feature. The codebase already projects 3D world positions to 2D screen space via `camera.world_to_viewport(...)` (see `src/states/play_match/rendering/hud.rs:230`). We reuse that approach:

1. For each alive combatant, project its capsule center to 2D viewport coordinates.
2. Compute the 2D pixel distance from the cursor to each projected position.
3. The closest combatant within a tolerance (≈40 px, generous for the small capsules) wins.
4. If no combatant is within tolerance, the click is a deselect.

This is fast (linear in combatant count, max 6) and matches the visual size of the unit on screen well enough for a click-to-select affordance. A future plan can swap in a true raycast or `bevy_mod_picking` if precision becomes important.

### State shape: a `Selection` resource, not a `Selected` marker component

A resource holding `Option<Entity>` is the right shape because:

- There is at most one selection at a time.
- The visual ring system needs to know "did the selection change?" — a `Resource` with `is_changed()` makes this trivial; a marker component requires `Added<Selected>` / `RemovedComponents<Selected>` event juggling.
- Picking logic writes the selection in one place; the visual reads it in another. A resource cleanly expresses that one-to-one relationship.

```rust
#[derive(Resource, Default)]
pub struct Selection {
    pub entity: Option<Entity>,
}
```

The visual ring is a separate entity carrying a `SelectionRing` marker component (mirroring the `ShieldBubble` follower pattern in `effects.rs:438-475`).

### Registration: graphical-only

The new systems (input handling, picking, ring spawn/follow/cleanup) all touch input, the camera, or rendering. They are graphical-only and register in `StatesPlugin::build()` in `src/states/mod.rs`, **not** in `add_core_combat_systems()`. Headless mode has no cursor and no need for selection. The `tests/registration_audit.rs` test enforces that any `pub fn` taking SystemParam types lives in one of the two registration paths; graphical-only systems go to `StatesPlugin::build()`.

### Death and cleanup

- The ring follow system also checks if the selected entity still exists and is alive. If not, it clears `Selection` and despawns the ring.
- The ring entity carries `PlayMatchEntity` so it is despawned automatically on match exit via `cleanup_play_match`.
- A small `OnExit(GameState::PlayMatch)` system resets `Selection` to `None` so a new match starts clean.

---

## Existing Patterns to Follow

| Pattern | Reference |
|---|---|
| Visual effect 3-system lifecycle (spawn / update / cleanup) | `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md` |
| Follower visual on a combatant (despawn-when-source-gone) | `ShieldBubble` and `follow_shield_bubbles` in `src/states/play_match/rendering/effects.rs:360-510` |
| Visual marker components | `src/states/play_match/components/visual.rs` |
| World-to-screen projection for picking math | `src/states/play_match/rendering/hud.rs:230` (`camera.world_to_viewport`) |
| Mouse + egui-aware input handling | `handle_camera_input` in `src/states/play_match/camera.rs:11-146` |
| Graphical-only system registration | `StatesPlugin::build()` in `src/states/mod.rs:147-262` |
| `AlphaMode::Add` + emissive for glowing translucent visuals | `ShieldBubble` material setup in `effects.rs:448-456` |
| Cleanup-on-match-exit via `PlayMatchEntity` marker | `spawn_combatant` in `src/states/play_match/mod.rs:631-637` |

---

## Implementation Units

### U1. Add `Selection` resource and `SelectionRing` marker component

**Goal**: Define the data shape — the resource that holds the current selection and the marker component that tags the ring entity.

**Dependencies**: None.

**Files**:
- `src/states/play_match/components/visual.rs` (add `SelectionRing` marker component)
- `src/states/play_match/components/mod.rs` (re-export via existing `pub use visual::*;` — likely no edit needed; verify)
- `src/states/play_match/mod.rs` (or a new small file `selection.rs`) — define the `Selection` resource and add to plugin via `.init_resource::<Selection>()`

**Approach**:

- `SelectionRing` is a unit-style marker component holding the followed entity:
  ```rust
  #[derive(Component)]
  pub struct SelectionRing {
      pub target: Entity,
  }
  ```
- `Selection` is a resource with default `None`:
  ```rust
  #[derive(Resource, Default)]
  pub struct Selection {
      pub entity: Option<Entity>,
  }
  ```
- Register the resource via `.init_resource::<Selection>()` in `StatesPlugin::build()`.

**Patterns to follow**: Visual marker component lives in `components/visual.rs` per the documented convention (`adding-visual-effect-bevy.md` Step 1). Resource registration matches existing `.init_resource::<...>()` calls in `StatesPlugin::build()`.

**Test scenarios**:
- `Selection::default()` returns a resource with `entity: None`.
- `SelectionRing { target: Entity::from_raw(1) }` can be constructed and stored.
- Test expectation: thin — these are pure data definitions. One smoke test per type is enough; broader behavior is exercised through later units.

**Verification**: `cargo check` compiles. `cargo test` continues to pass. `tests/registration_audit.rs` does not flag anything because the resource type has no SystemParam signature.

---

### U2. Click/drag disambiguation in the camera input handler

**Goal**: Distinguish a click from a drag on `MouseButton::Left`. Emit selection-pick events only on click; preserve all existing drag-to-rotate behavior.

**Dependencies**: U1 (the resource exists so the click handler can write to it on the same frame).

**Files**:
- `src/states/play_match/camera.rs` (extend `handle_camera_input` or introduce a sibling system `handle_selection_click` to track press position and emit click events)
- `src/states/play_match/components/combatant.rs` or wherever `CameraController` lives (add a `press_position: Option<Vec2>` field — discoverable via grep on `CameraController`)

**Approach**:

- On `MouseButton::Left::just_pressed` and `!egui_wants_pointer`: record the current cursor position as `press_position: Option<Vec2>` (alongside `is_dragging`).
- On `MouseButton::Left::just_released`: if `press_position` is set and the cursor's current position is within ≈5 px of the press position, write `true` to a `pending_pick: bool` flag on the controller (or fire a single-shot `ClickSelectionEvent`). Otherwise clear without firing.
- Pick threshold should be a constant alongside other camera constants. Suggested: `SELECTION_CLICK_THRESHOLD_PX: f32 = 5.0`.
- Drag-to-rotate behavior is **unchanged** — `is_dragging` still goes true on press and false on release, and yaw/pitch still updates during the press. Selection only triggers when total cursor travel was small.
- Do not fire selection if egui wanted the pointer at either press or release time.

**Patterns to follow**: The same `egui_wants_pointer` gate already used in `handle_camera_input` (line 29-31). Existing `last_mouse_pos` field in `CameraController` is a good template for adding `press_position`.

**Technical design** (directional only):
```text
press: just_pressed && !egui_pointer  → record press_position = current cursor
release: just_released                → if press_position.distance(current cursor) < THRESHOLD: pending_pick = true
                                       clear press_position
each frame: if pending_pick: U3 consumes it and clears it
```

**Test scenarios**:
- Press at (100, 100), release at (102, 99): pending_pick set to true.
- Press at (100, 100), release at (200, 100): pending_pick stays false (was a drag).
- Press while egui wants pointer (e.g., over a UI panel): pending_pick stays false.
- Release without a prior press (mouse re-entered window with button already up): pending_pick stays false.
- Test expectation: pure-input behavior is awkward to unit-test inside Bevy's `World` without rebuilding a minimal scheduler; cover via a small focused unit test on a helper that takes `(press_pos, release_pos, threshold) -> bool` (extract the disambiguation math into a free function so it is testable). Integration coverage is via U6's manual verification path.

**Verification**: Camera drag still rotates the view smoothly. Clicking without dragging now emits the pending-pick signal (logged via `info!` during development; remove the log before merge).

---

### U3. Selection-pick system: cursor → entity

**Goal**: When U2 emits a pending-pick, run screen-space picking against the live combatants and update the `Selection` resource.

**Dependencies**: U1, U2.

**Files**:
- `src/states/play_match/selection.rs` (new file; co-locate the picking system, the ring spawn system, and the ring follow/cleanup system — selection is its own concern, not really a combat system)
- `src/states/play_match/mod.rs` (declare `pub mod selection;` and re-export needed items)

**Approach**:

- New system `pick_selected_combatant` that runs only when the pending-pick flag is true.
- Inputs:
  - `Res<ButtonInput<MouseButton>>` — to confirm release semantics if needed (or simply consume the controller flag).
  - `ResMut<CameraController>` — to read & clear `pending_pick`.
  - `Query<(&Camera, &GlobalTransform), With<ArenaCamera>>` — for `world_to_viewport`.
  - `Query<&Window>` — to read cursor position (Bevy 0.15 convention).
  - `Query<(Entity, &Transform, &Combatant)>` — candidates.
  - `ResMut<Selection>` — write target.
- For each alive combatant, project `transform.translation` to viewport. Skip combatants that project off-screen (`world_to_viewport` returns Err for behind-camera or off-screen, depending on Bevy version — handle both).
- Compute `(screen_pos - cursor_pos).length()` per candidate. Track the minimum.
- If the minimum is within `SELECTION_PICK_RADIUS_PX` (suggest 40.0 — generous), set `Selection.entity = Some(winner)`. Otherwise set `Selection.entity = None` (click-on-empty-space deselects).
- Clear `pending_pick` after running.

**Patterns to follow**: `world_to_viewport` usage in `src/states/play_match/rendering/hud.rs:230`. The `ArenaCamera` marker is already in scope from `components::ArenaCamera`.

**Technical design** (directional only):
```text
For each (entity, transform) in alive combatants:
    projected = camera.world_to_viewport(camera_transform, transform.translation)
    if Ok(p): dist = (p - cursor).length(); track (entity, dist) if dist < best
If best.dist < PICK_RADIUS: Selection.entity = Some(best.entity)
Else:                       Selection.entity = None
```

**Test scenarios**:
- Cursor positioned over a combatant's screen-space center → `Selection.entity` becomes that combatant.
- Cursor 100 px from any combatant → `Selection.entity` becomes `None`.
- Two combatants nearby → the closer one wins.
- A click on a dead combatant's stale corpse position → does not pick (dead combatants filtered out — see U5 for the death-clear path too).
- Test expectation: extract a `find_closest_pick(cursor, projected_combatants, radius) -> Option<Entity>` helper and unit-test it with constructed inputs (no Bevy app needed). System-level behavior is verified manually.

**Verification**: With combatants on screen during a match, clicking a unit makes the (yet-to-be-rendered) ring appear there (after U4); clicking empty space removes it.

---

### U4. Spawn / despawn the selection ring when `Selection` changes

**Goal**: A spawn/despawn system that reacts to `Selection` resource changes and keeps exactly one `SelectionRing` entity alive matching the current selection.

**Dependencies**: U1, U3.

**Files**:
- `src/states/play_match/selection.rs` (same module as U3 — small, cohesive)

**Approach**:

- New system `sync_selection_ring`:
  - Runs every frame; cheap because it short-circuits when `selection.is_changed()` is false.
  - Inputs:
    - `Commands`
    - `ResMut<Assets<Mesh>>`, `ResMut<Assets<StandardMaterial>>`
    - `Res<Selection>`
    - `Query<Entity, With<SelectionRing>>` — existing rings.
  - Logic:
    - If `selection.is_changed()` (or first run): despawn any existing `SelectionRing` entity. Then, if `selection.entity` is `Some(target)`, spawn a new ring entity targeting it.
- Ring spawn:
  - Mesh: `Torus::new(0.75, 0.95)` (inner, outer ring radius).
  - Material: `StandardMaterial { base_color: Color::srgba(0.6, 0.9, 1.0, 0.6), emissive: LinearRgba::new(0.4, 0.8, 1.2, 1.0), alpha_mode: AlphaMode::Add, ..default() }`.
  - Transform: position set by the follow system on next frame (initial value can be zero or the target's current translation if read from the query).
  - Components: `Mesh3d(...)`, `MeshMaterial3d(...)`, `Transform::default()`, `SelectionRing { target }`, `PlayMatchEntity`.

**Patterns to follow**: `ShieldBubble` spawn block in `effects.rs:438-475` is the closest analog (follower visual carrying a target entity). Use the same `AlphaMode::Add` and emissive technique.

**Technical design** (directional only):
```text
if Selection changed:
    despawn all existing SelectionRing entities
    if Selection.entity == Some(target):
        spawn (Torus mesh, cyan-white material, SelectionRing { target }, PlayMatchEntity)
```

**Test scenarios**:
- Starting state: `Selection.entity = None`, no rings exist.
- Set `Selection.entity = Some(A)`: exactly one ring exists, targeting A.
- Change `Selection.entity = Some(B)`: exactly one ring exists, now targeting B (old ring despawned).
- Set `Selection.entity = None`: no rings exist.
- Test expectation: integration test built on a Bevy `App` with the system added, the resource present, and a stub camera/combatants — see `tests/` for any similar integration patterns; if none exist for visual systems, mark this as manually verified via U6 and note it in the unit's verification.

**Verification**: Switching the `Selection` resource value in-game (via U3 click) results in exactly one ring entity, on the correct unit. Toggling between two units alternates the ring without leaks.

---

### U5. Follow + auto-cleanup for the ring

**Goal**: Each frame, position the ring at its target's feet, apply the gentle pulse, and despawn the ring (clearing `Selection`) when the target dies or stops being a valid combatant.

**Dependencies**: U4.

**Files**:
- `src/states/play_match/selection.rs`

**Approach**:

- New system `follow_selection_ring`:
  - Inputs:
    - `Commands`
    - `Res<Time>` (real or sim time — match what other follower visuals use; `Res<Time>` is fine per the visual-effect doc)
    - `ResMut<Selection>`
    - `Query<(&Transform, &Combatant), Without<SelectionRing>>` — target lookup (note `Without` to avoid query conflicts, per the documented pattern)
    - `Query<(Entity, &SelectionRing, &mut Transform), Without<Combatant>>` — rings
  - Logic per ring:
    - Look up `ring.target` in the combatants query.
    - If the entity is gone (despawned) or the combatant is `!is_alive()`:
      - Despawn the ring.
      - Set `Selection.entity = None`.
    - Else:
      - Set `transform.translation = combatant_transform.translation + Vec3::new(0.0, -0.95 + 0.05, 0.0)` (feet, slightly above floor).
      - Rotate to lay flat: `transform.rotation = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)`.
      - Pulse: `let pulse = 1.0 + 0.05 * (time.elapsed_secs() * 3.0).sin(); transform.scale = Vec3::splat(pulse);`.

**Patterns to follow**: `follow_shield_bubbles` in `effects.rs:486-510` is the direct analog — same `Without<Combatant>` filter to avoid query conflict, same `Res<Time>` source, same pulse pattern.

**Technical design** (directional only):
```text
For each (ring_entity, SelectionRing { target }, ring_transform):
    match combatants.get(target):
        Err(_) or alive=false → despawn ring, Selection.entity = None
        Ok((t, _))            → set translation to feet, lay flat, pulse scale
```

**Test scenarios**:
- Selected unit moves across the arena → ring tracks the unit's feet each frame.
- Selected unit dies (current_health → 0) → ring despawns within one frame and `Selection.entity` is cleared.
- Selected unit's entity is despawned (e.g., cleanup) → ring despawns gracefully (no panic from missing target).
- Two consecutive selections in rapid succession (frame N: select A; frame N+1: select B): only one ring exists at any frame and is on the correct unit.
- Test expectation: helper-level test for the feet-Y math is overkill; cover via manual verification in U6. The "death clears selection" path is the high-value behavior — flag it in the U6 manual checklist.

**Verification**: Ring smoothly follows a moving unit. When the selected unit dies, the ring vanishes immediately and a new click is required to re-select.

---

### U6. Wire systems into `StatesPlugin`, register cleanup, and manual verification

**Goal**: Register the three new selection systems plus the resource in `StatesPlugin::build()`, reset the resource on match exit, and verify end-to-end behavior in the live game.

**Dependencies**: U1, U2, U3, U4, U5.

**Files**:
- `src/states/mod.rs` (register `Selection` resource, register the three systems, add `OnExit(GameState::PlayMatch)` system to reset selection)
- `src/states/play_match/mod.rs` (re-export selection module if needed for the `StatesPlugin` references)

**Approach**:

- Add `.init_resource::<Selection>()` to `StatesPlugin::build()` (alongside the other `.init_resource::<...>()` calls near the top of `build()`).
- Add the new systems with `.run_if(in_state(GameState::PlayMatch))`. Ordering:
  - `pick_selected_combatant` runs **after** `handle_camera_input` (which sets `pending_pick`).
  - `sync_selection_ring` runs after `pick_selected_combatant`.
  - `follow_selection_ring` can run anywhere after `sync_selection_ring`; place it `.after(CombatSystemPhase::CombatResolution)` alongside other follower visuals like `follow_shield_bubbles` so the ring tracks the unit's post-movement position the same frame.
- Add a small system `reset_selection_on_exit` (runs on `OnExit(GameState::PlayMatch)`) that sets `Selection.entity = None`. This complements the automatic ring despawn (rings carry `PlayMatchEntity` and are cleaned up by `cleanup_play_match`).
- Confirm `tests/registration_audit.rs` passes — any `pub fn` system added in `selection.rs` must be registered in `StatesPlugin::build()` since these are graphical-only.

**Patterns to follow**: Existing graphical-only system registration in `StatesPlugin::build()` (e.g., `update_shield_bubbles`, `follow_shield_bubbles` near `src/states/mod.rs:181-185`). The `.after(CombatSystemPhase::CombatResolution)` ordering for follower visuals is the established convention.

**Manual verification checklist** (run the graphical client and walk through these by hand — UI/input is not unit-testable in this codebase):

1. Start a match (`cargo run --release`).
2. Left-click a combatant during the countdown → ring appears at its feet within one frame.
3. Click a different combatant → ring moves to the new combatant; no orphan rings remain.
4. Click on the floor → ring disappears; selection cleared.
5. Left-click-drag to rotate the camera → no selection occurs; camera rotates normally.
6. Verify selection persists when the camera follow cycles between units (`Tab` or whatever key cycles modes) — selection ring should stay on the originally selected unit, not follow the camera.
7. Let the selected unit die in combat → ring vanishes immediately; selection cleared. Click a new unit to confirm selection still works.
8. Exit to results screen and return to a new match → no leftover ring; selection state is fresh.
9. Click on a UI element (e.g., time-controls panel) → no spurious selection.
10. Verify ring is visible against the arena floor at the default camera angle and at extreme zoom levels (in/out).

**Test scenarios**:
- `tests/registration_audit.rs` continues to pass after the new systems are added.
- `cargo build --release` succeeds.
- `cargo test` passes (no new test failures from the registration audit or any integration tests).
- Test expectation: the registration audit is the single most valuable automated check for this unit; the rest of the verification is manual per the checklist above.

**Verification**: All ten checklist items above behave as described. `cargo test` is green.

---

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| Click vs. drag threshold is wrong for trackpad / high-DPI displays | Start with 5 px and make it a `pub const` so it's trivial to tune. Document the trade-off (lower = more sensitive to drift; higher = misses small drags). |
| Screen-space pick radius (40 px) is too generous or too tight at certain zoom levels | Same: make it a `pub const`. If picking feels wrong at zoom extremes, future work can scale the radius by the camera distance. |
| Ground ring Z-fights with arena floor | Use `AlphaMode::Add` (codebase standard) plus a small +0.05 Y offset above the floor. Mirror the pattern from existing ground-plane visuals (slow zones, trap rings). |
| Selecting a unit during the countdown phase before combat starts — does the ring spawn cleanly? | The systems run in `PlayMatch` state and combatants exist throughout countdown. Manual checklist item #2 verifies this. No special handling required. |
| Visual conflict with other ground-anchored effects (slow zones, trap rings, Frost Nova radius) | The ring is small (≈0.95 outer radius) and color-coded distinctly (cyan-white). Visual designer call: this is acceptable for a prototype; revisit only if user feedback flags it. |
| Adding a new `pub fn` system without registering it in `StatesPlugin::build()` → silent failure in graphical mode | `tests/registration_audit.rs` catches this automatically and points to the right file. The unit's verification explicitly calls out running the audit. |
| User clicks rapidly between two units mid-frame → ring flickers | `Selection.is_changed()` triggers exactly one despawn+spawn cycle per frame. No flicker because Bevy renders the final state of the frame, not intermediate. |

---

## System-Wide Impact

- **No impact on headless mode**: all new systems are gated to `in_state(GameState::PlayMatch)` and registered only in `StatesPlugin::build()`. Headless runner is untouched. Determinism is preserved (selection state is not serialized into match logs or replay seeds).
- **No impact on combat AI**: selection is purely a player affordance. AI does not read `Selection`. Future plans can change this, but this plan does not.
- **No impact on save/load or match config**: selection is per-session, runtime-only, never persisted.
- **Camera behavior unchanged**: drag-to-rotate continues to work identically. The only addition is a "click was a click" branch that was previously a no-op.
- **`tests/registration_audit.rs` continues to enforce dual-mode discipline**: any future selection-related `pub fn` taking SystemParam types must be registered or allowlisted.

---

## Open Questions Deferred to Implementation

- The exact field placement of `press_position` / `pending_pick` — is it on the existing `CameraController` struct, or a new sibling resource `InputState`? Both are fine; pick whichever requires the smallest diff during U2. Document the choice in the commit message.
- Whether to use `Time<Real>` (wall clock) or `Time` (sim time) for the ring pulse. Most follower visuals use `Res<Time>`; default to that unless the visual feels wrong during pause.
- Whether `find_closest_pick` and the click-threshold helper live in `selection.rs` as `pub fn` (then auto-registered? — no, they don't take SystemParam types, so the audit ignores them) or in a small `selection/helpers.rs` submodule. Either is fine; favor whatever keeps `selection.rs` under ~250 lines.
