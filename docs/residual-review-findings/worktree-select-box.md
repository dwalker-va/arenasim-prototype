# Residual Review Findings — `worktree-select-box`

Source run: `/tmp/compound-engineering/ce-code-review/20260520-180554-42a855a3/synthesis.md`
Mode: `ce-code-review mode:autofix`
Plan: `docs/plans/2026-05-20-001-feat-click-to-select-unit-plan.md`
Reviewers: correctness, testing, maintainability, project-standards, agent-native, learnings, adversarial (7 of 7 returned)

Two `safe_auto` findings were applied in-place during the autofix pass and shipped with the feature commit:

- **#1 [P1][safe_auto -> review-fixer] `src/states/play_match/selection.rs` — Torus ring rendered vertical instead of flat.** Bevy 0.15's `Torus` is already flat in the XZ plane; the redundant `Quat::from_rotation_x(FRAC_PI_2)` was making the ring stand upright. Removed.
- **#2 [P2][safe_auto -> review-fixer (widened from manual via 3-reviewer corroboration)] `src/states/play_match/camera.rs` — First click of every match silently dropped.** `last_mouse_pos` is `None` at match enter (CameraController resets); pressing before any cursor movement silently skipped picking. Replaced with live `window.cursor_position()` reads.

## Residual Review Findings

### P2 — Should fix

- **[P2][gated_auto -> downstream-resolver] `src/states/play_match/selection.rs:21` — 5 px click threshold may be too tight for HiDPI / trackpads.** The plan flagged this. `SELECTION_CLICK_THRESHOLD_PX` is a `pub const` so it can be tuned. Adversarial reviewer (confidence 75). Suggested fix: scale the threshold by `Window::scale_factor()` or playtest and tune the constant.
- **[P2][manual -> downstream-resolver] `src/states/play_match/components/resources.rs` — `CameraController` owns selection-input state.** `press_position` and `pending_pick` are logically input-dispatch fields for the selection system, not camera positioning state. `pick_selected_combatant` takes `ResMut<CameraController>` despite not touching the camera. Multi-select / keyboard shortcuts will compound this. Maintainability reviewer. Suggested fix: split into a two-field `SelectionInput` resource in `selection.rs`.
- **[P2][safe_auto -> downstream-resolver] `src/states/play_match/selection.rs:21,28` — `SELECTION_CLICK_THRESHOLD_PX` and `SELECTION_PICK_RADIUS_PX` belong in `src/states/play_match/constants.rs`.** Every other tunable in the project lives there. Mechanical refactor; deferred from the autofix pass to keep changes focused.
- **[P2][manual -> downstream-resolver] `src/states/play_match/camera.rs` — Cross-module call to `super::selection::is_click_gesture` and `super::selection::SELECTION_CLICK_THRESHOLD_PX`.** Bypasses the `play_match` re-export surface and creates a round-trip dependency. Maintainability reviewer. Suggested fix: inline the two-line helper into `camera.rs` (the test can stay in `selection.rs`), or move the click-gesture concept to `camera.rs` as its owner.

### P3 — Discretionary

- **[P3][advisory -> human] Off-screen click deselects (Starcraft semantics, not WoW).** Clicking floor / empty space clears Selection. The plan explicitly allowed this; WoW Classic actually keeps selection until you target another unit. Adversarial reviewer (confidence 100). Design call — decide whether to match the game's stated WoW Classic inspiration.
- **[P3][advisory -> human] `src/states/play_match/selection.rs` — One-frame ring at world origin on new selection.** `sync_selection_ring` spawns the ring with `Transform::default()`; `follow_selection_ring` repositions on the next frame. Mirrors the existing `ShieldBubble` pattern. May or may not be visible at 60 fps — verify in playtest.
- **[P3][advisory -> human] `src/states/play_match/selection.rs` — Ring briefly follows dead combatant's falling capsule before despawn.** When a selected unit dies, the death animation (~0.6 s) starts before the next `follow_selection_ring` tick despawns the ring. Sub-frame artifact.
- **[P3][advisory -> human] `src/states/play_match/selection.rs` — `find_closest_pick` tie-break depends on ECS iteration order when two candidates are exactly equidistant.** Practically impossible at floating-point pixel distances; undocumented.
- **[P3][advisory -> human] `src/states/play_match/selection.rs` — Magic literals in the pulse animation `1.0 + 0.05 * (time.elapsed_secs() * 3.0).sin()`.** Three numbers each carry distinct semantic roles (base scale, amplitude, frequency). Named constants would help designer tuning.

## Soft-bucket items (not findings, captured for context)

### Residual risks

- `pick_selected_combatant` clears `pending_pick` even when guards fail (no camera, no window). A click while the window briefly loses focus is silently dropped.
- 5 px click threshold has no HiDPI scaling.

### Testing gaps

- No automated tests for `pick_selected_combatant` (screen-space picking pipeline) — manual checklist only.
- No automated tests for `sync_selection_ring` (spawn/despawn on `Selection` change) — manual checklist only.
- No automated tests for `follow_selection_ring` (target-follows + death-clears-selection) — manual checklist only.
- No automated tests for the `press_position` / `pending_pick` state machine in `handle_camera_input` — manual checklist only.
- No test for `reset_selection_on_exit`.

## Verdict from review

Ready with fixes applied. No P0, no merge blockers. The 9 residual items above are tracked for follow-up.
