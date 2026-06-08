---
title: "Bevy 0.16 macOS exit hang: AppExit deadlocks winit; egui ctx_mut panics on the teardown frame"
category: implementation-patterns
tags:
  - bevy
  - winit
  - macos
  - egui
  - app-exit
  - shutdown
  - bevy-0-16
module: src/states/mod.rs
symptom: "Clicking EXIT (or the macOS red-X) freezes the app, or exits then panics in a UI system"
root_cause: "Programmatic AppExit from a system deadlocks the macOS winit loop; and the schedule ticks one more frame after the window closes, where EguiContexts::ctx_mut panics on the dead context"
date: 2026-06-07
---

# Bevy 0.16 macOS exit: AppExit deadlock + egui teardown-frame panic

Two distinct bugs on the same shutdown path, both surfaced after the Bevy 0.16 /
winit-0.30 migration. They must be fixed together — fixing the first exposes the
second.

## Bug 1 — programmatic `AppExit` deadlocks the macOS event loop

Writing `AppExit::Success` from a system (e.g. an EXIT button handler) hard-locks
the macOS winit event loop: the app freezes instead of quitting and must be
SIGKILLed. Upstream: [bevyengine/bevy#23313](https://github.com/bevyengine/bevy/issues/23313).

**Symptom:** EXIT button does nothing; window unresponsive; process alive at 0% CPU.

**Fix:** don't write `AppExit` from a system. Despawn the primary window instead —
this re-enters winit's native close path, and the default
`ExitCondition::OnAllClosed` turns it into a clean exit.

```rust
fn main_menu_ui(
    mut commands: Commands,
    primary_window: Query<Entity, With<bevy::window::PrimaryWindow>>,
    // ...
) {
    // EXIT button:
    for window in primary_window.iter() {
        commands.entity(window).despawn();
    }
}
```

## Bug 2 — `EguiContexts::ctx_mut()` panics on the teardown frame

After the window despawns, the schedule runs **one more frame** before the event
loop winds down. Any per-frame UI system that calls `EguiContexts::ctx_mut()`
panics there — the egui context died with the primary window
(`bevy_egui-0.34.1/src/lib.rs:626`, "called for an uninitialized context").

**Symptom:** the app exits but the log ends with a panic in a `*_ui` system, then
`Encountered a panic in system Main::run_main`. Same panic fires on the macOS
red-X close button (whatever state's UI is active that frame).

**Fix:** use the fallible `try_ctx_mut()` with an early return in every per-frame
UI system. This repo already had the convention ("gracefully handle window
close") — four systems had simply missed it: `main_menu_ui`, `options_ui`,
`keybindings_ui`, `armory_ui`.

```rust
// Per-frame UI system: the context can vanish on the teardown frame.
let Some(ctx) = contexts.try_ctx_mut() else { return; };
```

**Exception:** run-once `Startup` systems keep the loud `ctx_mut()` deliberately —
a silent skip there would permanently lose whatever it sets up (e.g.
`setup_custom_font` would drop the custom font), and there is no teardown frame at
startup.

Files: `src/states/mod.rs`, `src/main.rs`, `src/states/armory_ui.rs`. Commit `cc98fc5`.

## Why this works

`AppExit`-from-a-system and window-despawn both *intend* to exit, but only the
despawn goes through winit's own close handling — the path macOS expects. And the
teardown frame is real: any code that assumes the egui context outlives the window
will fault on it. `try_ctx_mut()` makes "no context this frame" a normal,
skippable condition instead of a panic.

## Prevention

- **Never write `AppExit` from a system on a windowed macOS app.** Despawn the
  primary window. (Headless/`ScheduleRunner` apps are a different path and are
  unaffected.)
- **Every per-frame egui UI system uses `try_ctx_mut()` with early return.** Audit
  with `grep -rn "ctx_mut()" src/ | grep -v try_ctx` — the only legitimate bare
  `ctx_mut()` is in run-once Startup systems.
- This is the kind of bug that recurs on every engine/winit bump — when upgrading
  Bevy, re-verify the exit path (launch → EXIT click → clean log, no panic) by
  hand, since it has no automated coverage.
