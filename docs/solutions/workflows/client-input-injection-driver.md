---
title: "Scripted client input: verifying the running client without seeing it"
category: workflow-patterns
tags:
  - graphical-verification
  - bevy
  - bevy-egui
  - input-injection
  - ui-testing
  - agentic-development
  - macos
module: src/ui/driver/, tests/ui-scripts/
symptoms:
  - "Cannot confirm a tooltip appears without a screenshot"
  - "Cannot confirm a click handler ran in the real client"
  - "Client verification is limited to log-grepping for panics"
  - "A kittest assertion passes while the running client does nothing"
root_cause: "screencapture and osascript are permission-blocked, so the only client evidence available was stdout; egui_kittest proves a widget's contract but not the client's input routing"
date: 2026-09-18
---

# Scripted client input

`cargo run --release -- --ui-script tests/ui-scripts/<name>.script`

Drives the **running graphical client** from a text script: move the pointer to
a named widget, hover it, click it, press a key, and assert what the UI did.
One line per step in the log, non-zero exit on the first failed assertion.

## Why it exists

`screencapture` and `osascript` are permission-blocked on this machine (their
dialogs interrupt the user), so client verification meant launching the client
with a temporary state-cycler and grepping stdout. That proves *no panic*. It
cannot prove *hovering this icon shows a tooltip* or *clicking Restore defaults
empties the override map*.

The offscreen egui snapshot loop (`tests/*_snapshot.rs`) does not close that
gap either, and AS-65 round 2 is the proof: a `secondary_clicked()` assertion
passed under `egui_kittest` while the affordance did nothing in the client.
**kittest proves the widget's contract; it does not prove the client's input
routing.** Only a real run does.

Two Engineers built this tool from scratch in one afternoon (AS-65 round 3,
AS-64), used it, and reverted it before pushing. This is that tool, kept.

## Its relationship to the "Human testing" line in a PR

Every PR states what a human must check and why a machine could not
(AS-123). **This driver shrinks that set.** A sentence that used to read "a
human must confirm the tooltip appears when hovering a kit row" is now a
scripted assertion. The two are complements, not opposites: one names the gap
honestly, the other narrows it.

What stays human: anything about how it *looks* or *feels* — layout, colour,
motion, legibility — and the limits below.

## The layer, and why it matters

Events go in at **Bevy's own input events** — `CursorMoved`,
`MouseButtonInput`, `KeyboardInput`, `MouseWheel` — written in `PreUpdate`
before `bevy::input::InputSystem`. Only winit is below that. Everything above
it runs exactly as it does for a physical device:

* `InputSystem` folds the events into `ButtonInput<KeyCode>`, so
  `Keybindings::action_just_pressed` sees a scripted Escape as a real one.
* `bevy_egui`'s `EguiPreUpdateSet::ProcessInput` is ordered `.after(InputSystem)`,
  so **bevy_egui's own conversion runs** — including the
  `MouseButton::Right -> egui::PointerButton::Secondary` mapping that the AS-65
  right-click investigation turned on.

The alternative layer — pushing `egui::Event`s straight into bevy_egui's
`EguiInput` after `ProcessInput` — also works and is what AS-64 used. It
skips bevy_egui's conversion, which is exactly the part worth exercising, so
prefer the Bevy layer and say why if you ever fall back.

## The one limit that matters most

**This driver can prove a right-click handler works and still fail to
reproduce what the user saw.**

AS-65 reported that right-click did nothing on the strategic-option panels.
Injected right-clicks opened the correct page on all 14 icons. The report was
not wrong: on macOS, **Ctrl+click is delivered to a plain `NSView` as
`mouseDown` with the Control modifier** — the context-menu remap lives in
`menuForEvent:`, which winit does not use — so Bevy sees `Left` + ctrl and egui
sees a *primary* click. A two-button mouse or a two-finger trackpad tap arrives
as `rightMouseDown` and works. The user later confirmed exactly that.

So a green script says *the handler is correct*, never *the user's input
reaches it*. For an input affordance, ask what device the reporter used before
concluding anything. The same caution applies to any gesture the OS rewrites
before winit sees it.

Two smaller limits, both real:

* **It sees no pixels.** It knows a tooltip's closure ran, not that the tooltip
  is legible, correctly placed, or not drawn off-screen.
* **Do not touch the mouse during a run.** Real `CursorMoved` events land in
  the same queue and will fight the synthetic ones.

## Writing a script

```text
# Comments and blank lines are ignored; '#' also ends a line.
hover <id>                  # park the cursor
click <id> [left|right]     # hover, press, release
key <Escape|Enter|Tab|Space|Backspace|ArrowLeft|ArrowRight|ArrowUp|ArrowDown>
wait <frames>               # on top of each step's own settle
assert-state <GameState>    # the Rust variant name, e.g. ViewCombatant
assert-view <Topic>         # the encyclopedia's topic, Debug form, or `none`
assert-note <substring>     # some note from the last drawn frame contains this
assert-no-note <substring>
assert-visible <id>         # drawn AND on screen
assert-absent <id>          # not drawn AT ALL
assert-enabled <id> <true|false>   # reads state, on screen or not
dump                        # log every widget and note on the last frame
```

An unknown verb is a **parse error**, not a skipped line — a mistyped
`assert-stat` must not quietly turn a check into a no-op.

**`assert-visible` and `assert-absent` are two halves of one three-way
question**, and both answer all three cases. A widget is *not drawn*, *drawn
but clipped*, or *drawn and on screen*; `assert-visible` passes only on the
third, `assert-absent` only on the first, and the middle case fails BOTH with
a message saying so. That middle case is why: `assert-absent` originally
returned "not visible", so "this row is gone" was satisfied by a row that had
merely scrolled out of sight — a negative that looked conclusive and was not.
If you want to assert about screen presence, scroll the target into view first
(`hover` and `click` scroll; an assertion does not).

`dump` is how you learn the ids: run a stub script that navigates and dumps,
read the log, then write the real assertions.

## Opting a widget in

Two calls, and nothing else in `src/states/` knows the driver exists:

```rust
use crate::ui::driver as ui_driver;

// "a script may name and click this"
ui_driver::mark(ui, rect, enabled, format_args!("kit:{ability:?}"));

// "here is something only the draw can see"
ui_driver::note(ui, format_args!("equip-row {slot:?} item={id:?} override={is_override}"));
```

`mark` takes the rect the widget already allocated and whether it is
interactive; a **disabled** widget still registers, so a script can assert that
it *is* disabled. `note` is for facts a screenshot-blind driver could not
otherwise recover: which tooltip body actually ran, what colour a row rendered
in, what the live override map holds.

**Keep the opt-in explicit.** Instrumenting every widget would make the
registry a maintenance surface and the `dump` output unreadable. What is marked
today: the main-menu buttons and Configure Match's slot and class tiles (the
route in), View Combatant's kit rows, strategic-option icons, equipment rows,
picker entries and Restore defaults, and the encyclopedia's Back/Home. The
tooltip probe is a single `note` inside `encyclopedia::widget::link_with`,
which every linked icon in the client funnels through, so one call covers them
all.

Ids are conventions, not types: `menu:MATCH`, `class:Warrior`, `slot:t1s0`,
`slot:t1s0:change`, `kit:MortalStrike`, `strategic:BattleShout`,
`strategic:CurseOfTongues:e0`, `equip:Ring1`, `equip:restore`,
`pick:SignetOfFocus`, `enc:back`, `enc:home`.

## Inertness, and how it is actually established

Without `--ui-script`, `UiDriverPlugin::build` returns before touching the app:
no systems, no resource. Nothing then calls `registry::arm`, so every `mark`
and `note` is one failed hash lookup that records nothing, and taking the text
as `fmt::Arguments` means it is never formatted either.

**That second half is a property of the call site as much as of the
primitive.** `format_args!` defers formatting but NOT the evaluation of its
arguments, so a note that assembles its text in the argument position —
`format_args!("{}", xs.iter().map(f).collect::<Vec<_>>().join(","))` — builds
that `Vec` and `String` on every frame with the driver off. The equipment
panel did exactly that until review caught it. Wrap the source in a `Display`
and let the formatter do the work (`view_combatant_ui::OverrideMap`), and
`no_driver_call_site_allocates_before_the_armed_check` keeps the next one
honest by scanning the real call sites.

That is a claim about **absence**, which a passing test cannot demonstrate: a
test that goes green with the driver off would go green just as happily if the
driver were off *and broken*. `tests/ui_driver.rs` therefore runs each claim
**twice, off and on, and requires the two to differ**, with the enabled half
asserted first so an invisible probe fails there rather than passing quietly.

Both halves were confirmed by mutation — and the second mutation is the
interesting one:

| Mutation | Result |
| --- | --- |
| `build` schedules `run_ui_script` unconditionally | `the_plugin_schedules_nothing_when_no_script_is_configured` **fails**, naming the leaked system |
| `mark_into` drops its armed check | `registry_records_nothing_until_it_is_armed` **passed** — the test was blind |
| the call site pre-joins its `String` again | `no_driver_call_site_allocates_before_the_armed_check` **fails**, naming file and line |
| `assert-absent` goes back to `!is_visible` | `presence_assertions_separate_not_drawn_from_scrolled_off` **fails** on the clipped case |
| `hover` settles for `settle` | `a_hover_settles_for_the_long_window_and_a_click_does_not` **fails** |
| a shipped script is deleted | `every_shipped_script_parses` **fails**, naming the file |

The second test asserted on `registry::snapshot`, which re-checks the armed
flag itself, so a leaking `mark` still read back as `None`. **The assertion was
reading through a gate instead of at the thing.** The fix was
`registry::raw_frame`, the unguarded view of the store; the mutation fails
against it. If you add an inertness check anywhere, ask what the reader is
gated on before trusting a negative — and, having found one such assertion,
re-read its neighbours: that is how
`mark_takes_visibility_from_eguis_clip_rect` came to exist, replacing a test
that handed the visibility flag in by hand and so pinned nothing about `mark`.

## Two timing traps, both of which produced a wrong-looking pass

**1. Aim only at a rect that has stopped moving.** The registry snapshot is one
frame old. View Combatant is a long scroll, so the driver scrolls an
off-screen target into view — and egui's scrolling is smooth, running on for a
frame or two after the last wheel event. The first run of the equipment script
clicked **Trinket1** while aiming at Ring1, because the row had slid exactly
one wheel line. The click landed on *something*, which is the worst failure
mode available. `Micro::Move` now requires the target rect to be identical on
two consecutive frames before it aims.

**2. A tooltip is not a function of position.** egui's
`show_tooltips_only_when_still` gates the tooltip on pointer **velocity**
reaching zero, measured over a ~0.1s history window, and only then does
`tooltip_delay` start counting. Three settle frames put the assertion inside
that window, and the five-class script read an empty tooltip on a hover that
was working. `hover` therefore settles for `DEFAULT_HOVER_SETTLE_FRAMES` (30,
about half a second) rather than the ordinary three.

Both traps share a shape: the driver observed a state that was still changing.
When a new assertion is flaky, suspect that before suspecting the UI.

## Exit, and why not `AppExit`

Writing `AppExit` from a system deadlocks the macOS winit event loop (see
`../implementation-patterns/bevy-macos-exit-deadlock-egui-teardown.md`). The
runner despawns the primary window instead — the documented close path — and
leaves its verdict in an `Arc<Mutex<Outcome>>` that `main` reads after
`App::run` returns, then `std::process::exit(1)`.

A run that ends without finishing (the window closed early, a panic) leaves
`Outcome::Incomplete`, which is also a non-zero exit: an unfinished script
checked nothing, and must not read as a pass.

## The shipped scripts

`tests/ui-scripts/view-combatant-tooltips.script` — the five-class walk AS-65
round 3 did by hand. Per class: the kit-row tooltip body ran, the
strategic-option tooltip body ran (hover-only, after round 3 removed the
right-click), clicking the kit row opens that ability's encyclopedia page, and
Esc returns to the **same** combatant. It finishes on the encyclopedia's nav
cluster: Back is disabled at the root of a deep link, Home returns to View
Combatant. 86 steps, 44 of them assertions.

`tests/ui-scripts/equipment-restore-defaults.script` — the AS-64 ring repro.
Warrior defaults, Ring 1 → Signet of Focus, Ring 2 → Band of Accuria (offered
once Ring 1 lets go of it), then Restore defaults, and the property the old
per-slot reset broke: after a restore **no row still renders as overridden**.
Then the right-click reference path to the item's page. 36 steps, 24 of them
assertions.

`tests/ui-scripts/replay-boots-into-the-match.script` — four lines, paired with
a `.json` config, pinning that `--replay` still boots straight into `PlayMatch`
and that `--ui-script` composes with it. The in-match HUD is deliberately not
instrumented, so there is nothing to click; this exists because the graphical
dispatch in `main.rs` is the one thing this card restructured that could have
broken silently.

All three are checked for parse errors by `cargo test`
(`the_shipped_scripts_parse`), but running them needs a window, so they are not
in the suite. Run them by hand after touching View Combatant, the equipment
panel, the encyclopedia's navigation, `main.rs`'s graphical dispatch, or
anything under `src/ui/driver/`.

## What is covered by a test, and what only by running

`tests/ui_driver.rs` covers the parser, the registry (including both halves of
inertness), and the runner's two PURE seams: `expand`, which decides what a
step costs in frames, and `evaluate`, which decides every `assert-*` verb.

**The runner's frame loop is not covered by any test.** It needs a window, so
the scroll-to-reveal arm, the rect-stability gate and the event injection
itself are established only by running the scripts. Treat a change in
`run_ui_script` as unverified until all three scripts pass against the client.

## When a script fails

The failure line names the source line and says what it saw — for a missing
widget it lists every id that *was* registered, marking the off-screen ones.
Most failures are one of: a stale id after a rename, a widget that needs
scrolling in a container the driver cannot reach, or a settle too short for
what you are asserting. Add a `dump` next to the failing step first.
