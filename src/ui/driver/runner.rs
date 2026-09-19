//! The frame-level executor: turns script steps into injected Bevy input
//! events and reads the results back off the registry.
//!
//! # Where the events go in
//!
//! At Bevy's own input events — `CursorMoved`, `MouseButtonInput`,
//! `KeyboardInput` — written in `PreUpdate` BEFORE [`bevy::input::InputSystem`].
//! That placement is what makes the injection real rather than a simulation of
//! itself:
//!
//! * `InputSystem` folds the events into `ButtonInput<KeyCode>`, so keybinding
//!   code (`Keybindings::action_just_pressed`) sees the key exactly as it sees
//!   a physical one.
//! * `bevy_egui`'s `EguiPreUpdateSet::ProcessInput` is configured
//!   `.after(InputSystem)`, so bevy_egui's OWN conversion runs on these events
//!   — including the `MouseButton::Right -> PointerButton::Secondary` mapping
//!   that the AS-65 right-click investigation turned on.
//!
//! Only winit sits below. Everything from Bevy's event queues upward is the
//! code the player exercises.
//!
//! # Why a click takes several frames
//!
//! egui resolves interaction against the widget layout of the PREVIOUS pass, so
//! a pointer move and a button press in the same frame do not reliably click
//! the widget that was under the cursor. Each step therefore expands into
//! frame-sized [`Micro`] actions — move, settle, press, release, settle —
//! which is also simply what a hand does.
//!
//! # Scrolling
//!
//! View Combatant is one long scroll, so a widget can be laid out and still be
//! nowhere the cursor can reach. `mark` records whether the rect was inside its
//! clip rect; a `Move` at an off-screen target parks the cursor over the middle
//! of the screen and sends wheel events toward it until it comes into view,
//! then aims. Without this, a click at a clipped widget's laid-out coordinates
//! would land on whatever happens to be drawn there — the worst possible
//! failure, because it looks like a pass.
//!
//! # Exit
//!
//! Never by writing `AppExit`: that deadlocks the macOS winit loop (see
//! `docs/solutions/implementation-patterns/bevy-macos-exit-deadlock-egui-teardown.md`).
//! The runner despawns the primary window, the documented close path, and hands
//! the verdict to `main` through the shared outcome handle so the process can
//! still exit non-zero.

use std::collections::VecDeque;
use std::io::Write;

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::{CursorMoved, PrimaryWindow};
use bevy_egui::{egui, EguiContextSettings, EguiContexts};

use crate::states::encyclopedia::EncyclopediaState;
use crate::states::GameState;

use super::registry::{self, Frame};
use super::script::{Button, NamedKey, Script, Step};
use super::{Outcome, OutcomeHandle, UiDriverConfig};

/// Wheel lines sent per frame while scrolling an off-screen target into view.
/// Small enough not to fly past a target in one step at egui's ~50pt/line.
const SCROLL_LINES: f32 = 1.0;

/// One frame's worth of work.
///
/// `pub` so `tests/ui_driver.rs` can pin what a step expands into — notably
/// that a `hover` settles for `hover_settle` and not the ordinary `settle`,
/// which is the difference between observing a tooltip and observing egui
/// still deciding whether to show one.
#[derive(Clone, Debug, PartialEq)]
pub enum Micro {
    /// Park the synthetic cursor over the named widget. Retries each frame
    /// until the widget appears in the registry, then fails on timeout.
    Move {
        id: String,
    },
    Press(Button),
    Release(Button),
    KeyDown(NamedKey),
    KeyUp(NamedKey),
    /// Burn `n` further frames.
    Idle(u32),
    /// Evaluate an assertion (or `dump`) against the last drawn frame.
    Check(Step),
}

/// stdout plus the log file, so an agent can tail the run or grep the artefact.
struct LogSink {
    file: std::fs::File,
    path: std::path::PathBuf,
}

impl LogSink {
    fn line(&mut self, text: &str) {
        println!("[ui-script] {text}");
        let _ = writeln!(self.file, "{text}");
        let _ = self.file.flush();
    }
}

/// The live script run. Present only when the driver is enabled — its absence
/// is what makes the whole feature inert.
#[derive(Resource)]
pub struct UiScriptRun {
    script: Script,
    outcome: OutcomeHandle,
    log: LogSink,
    /// Steps not yet started.
    pending: VecDeque<(usize, Step)>,
    /// The current step's remaining frame-level actions.
    queue: VecDeque<Micro>,
    /// Source line of the step `queue` belongs to, for failure messages.
    line: usize,
    /// Frames spent waiting for the widget a `Move` names.
    waiting: u32,
    /// Frames of settle after each injected event.
    settle: u32,
    /// Frames of settle after a `hover` — longer, because a tooltip waits on
    /// the pointer going still, not just on where it is.
    hover_settle: u32,
    /// Frames a `Move` waits for its widget before failing.
    timeout: u32,
    /// Where the synthetic cursor sits, in egui points.
    cursor: egui::Pos2,
    /// The target rect seen on the previous frame, for the stability gate in
    /// `Micro::Move`.
    last_seen: Option<(String, egui::Rect)>,
    /// Frames to burn before the first step, so the first screen has drawn and
    /// the registry has a frame to read.
    warmup: u32,
    /// Last `GameState` logged, so transitions are logged once each.
    last_state: Option<GameState>,
    /// Set once the verdict is in; every later frame is a no-op.
    done: bool,
}

impl UiScriptRun {
    pub fn new(config: UiDriverConfig) -> Self {
        if let Some(parent) = config.log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = std::fs::File::create(&config.log_path).unwrap_or_else(|e| {
            panic!(
                "--ui-script-log: could not create {}: {e}",
                config.log_path.display()
            )
        });
        let mut log = LogSink {
            file,
            path: config.log_path.clone(),
        };
        log.line(&format!(
            "script {} ({} steps), settle={} hover-settle={} timeout={} frames",
            config.script.name,
            config.script.steps.len(),
            config.settle_frames,
            config.hover_settle_frames,
            config.step_timeout_frames,
        ));
        Self {
            pending: config.script.steps.iter().cloned().collect(),
            script: config.script,
            outcome: config.outcome,
            log,
            queue: VecDeque::new(),
            line: 0,
            waiting: 0,
            settle: config.settle_frames,
            hover_settle: config.hover_settle_frames,
            timeout: config.step_timeout_frames,
            cursor: egui::pos2(0.0, 0.0),
            last_seen: None,
            warmup: config.settle_frames.max(2),
            last_state: None,
            done: false,
        }
    }

    /// The path the log is being written to, for the closing console line.
    pub fn log_path(&self) -> &std::path::Path {
        &self.log.path
    }

    fn fail(&mut self, reason: String) {
        let message = format!("{}:{}: {reason}", self.script.name, self.line);
        self.log.line(&format!("FAIL {message}"));
        self.log.line("verdict: FAILED");
        *self.outcome.lock().expect("ui script outcome mutex") = Outcome::Failed(message);
        self.done = true;
    }

    fn pass(&mut self) {
        self.log.line("verdict: PASSED");
        *self.outcome.lock().expect("ui script outcome mutex") = Outcome::Passed;
        self.done = true;
    }
}

/// Expand one script step into its frame-level actions.
pub fn expand(step: &Step, settle: u32, hover_settle: u32) -> VecDeque<Micro> {
    let mut q = VecDeque::new();
    match step {
        Step::Hover { id } => {
            q.push_back(Micro::Move { id: id.clone() });
            q.push_back(Micro::Idle(hover_settle));
        }
        Step::Click { id, button } => {
            q.push_back(Micro::Move { id: id.clone() });
            q.push_back(Micro::Idle(settle));
            q.push_back(Micro::Press(*button));
            q.push_back(Micro::Idle(1));
            q.push_back(Micro::Release(*button));
            q.push_back(Micro::Idle(settle));
        }
        Step::Key(k) => {
            q.push_back(Micro::KeyDown(*k));
            q.push_back(Micro::Idle(1));
            q.push_back(Micro::KeyUp(*k));
            q.push_back(Micro::Idle(settle));
        }
        Step::Wait { frames } => q.push_back(Micro::Idle(*frames)),
        other => q.push_back(Micro::Check(other.clone())),
    }
    q
}

/// Bevy's physical `KeyCode` and logical `Key` for a script key name.
fn key_codes(key: NamedKey) -> (KeyCode, Key) {
    match key {
        NamedKey::Escape => (KeyCode::Escape, Key::Escape),
        NamedKey::Enter => (KeyCode::Enter, Key::Enter),
        NamedKey::Tab => (KeyCode::Tab, Key::Tab),
        NamedKey::Space => (KeyCode::Space, Key::Space),
        NamedKey::Backspace => (KeyCode::Backspace, Key::Backspace),
        NamedKey::ArrowLeft => (KeyCode::ArrowLeft, Key::ArrowLeft),
        NamedKey::ArrowRight => (KeyCode::ArrowRight, Key::ArrowRight),
        NamedKey::ArrowUp => (KeyCode::ArrowUp, Key::ArrowUp),
        NamedKey::ArrowDown => (KeyCode::ArrowDown, Key::ArrowDown),
    }
}

fn mouse_button_of(button: Button) -> MouseButton {
    match button {
        Button::Left => MouseButton::Left,
        Button::Right => MouseButton::Right,
    }
}

/// The encyclopedia's current topic, in the form `assert-view` compares to.
fn current_topic(encyclopedia: &EncyclopediaState) -> String {
    match encyclopedia.current().topic {
        Some(topic) => format!("{topic:?}"),
        None => "none".to_string(),
    }
}

fn describe_widgets(frame: &Frame) -> String {
    if frame.widgets.is_empty() {
        return "(none)".to_string();
    }
    let mut ids: Vec<String> = frame
        .widgets
        .iter()
        .map(|w| {
            if w.visible {
                w.id.clone()
            } else {
                format!("{} (off-screen)", w.id)
            }
        })
        .collect();
    ids.sort_unstable();
    ids.join(", ")
}

/// One frame of the script. See the module docs for the scheduling contract.
pub fn run_ui_script(
    mut run: ResMut<UiScriptRun>,
    mut contexts: EguiContexts,
    mut commands: Commands,
    windows: Query<(Entity, &EguiContextSettings), With<PrimaryWindow>>,
    state: Res<State<GameState>>,
    encyclopedia: Res<EncyclopediaState>,
    mut cursor_moved: EventWriter<CursorMoved>,
    mut mouse_buttons: EventWriter<MouseButtonInput>,
    mut mouse_wheel: EventWriter<MouseWheel>,
    mut keyboard: EventWriter<KeyboardInput>,
) {
    if run.done {
        return;
    }

    // The egui context dies with the window; on the teardown frame there is
    // nothing to read and nothing to arm.
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };
    let ctx = ctx.clone();
    let Ok((window, egui_settings)) = windows.single() else {
        return;
    };
    let scale = egui_settings.scale_factor;

    // What the UI drew since we last ran. Empty on the very first frames,
    // before `arm` has ever been called.
    let frame = registry::snapshot(&ctx).unwrap_or_default();

    let now = *state.get();
    if run.last_state != Some(now) {
        run.last_state = Some(now);
        run.log.line(&format!("state {now:?}"));
    }

    if run.warmup > 0 {
        run.warmup -= 1;
        registry::arm(&ctx);
        return;
    }

    if run.queue.is_empty() {
        let Some((line, step)) = run.pending.pop_front() else {
            run.pass();
            commands.entity(window).despawn();
            return;
        };
        run.log.line(&format!("step {line}: {}", step.describe()));
        run.line = line;
        run.waiting = 0;
        run.queue = expand(&step, run.settle, run.hover_settle);
    }

    // Exactly one frame-level action per frame, so every injected event gets
    // its own pass through bevy_egui and egui.
    let action = run.queue.pop_front().expect("queue is non-empty here");

    match action {
        Micro::Idle(0) => {}
        Micro::Idle(n) => run.queue.push_front(Micro::Idle(n - 1)),
        Micro::Move { id } => match frame.widget(&id) {
            Some(widget) if widget.visible => {
                // AIM ONLY AT A RECT THAT HAS STOPPED MOVING. The snapshot is
                // one frame old, and a scroll (egui's is smooth, so it keeps
                // running for a frame or two after the last wheel event) moves
                // the row out from under the coordinate we read. The first run
                // of the equipment script clicked Trinket1 because Ring1 had
                // slid exactly one wheel line — the worst failure mode, since
                // the click lands on SOMETHING and looks like it worked.
                let stable = run.last_seen.as_ref() == Some(&(id.clone(), widget.rect));
                if !stable {
                    run.last_seen = Some((id.clone(), widget.rect));
                    run.waiting += 1;
                    if run.waiting > run.timeout {
                        let timeout = run.timeout;
                        run.fail(format!(
                            "widget `{id}` never stopped moving ({timeout} frames)"
                        ));
                        commands.entity(window).despawn();
                        return;
                    }
                    run.queue.push_front(Micro::Move { id });
                } else {
                    let target = widget.rect.center();
                    let delta = target - run.cursor;
                    run.cursor = target;
                    cursor_moved.write(CursorMoved {
                        window,
                        position: Vec2::new(target.x * scale, target.y * scale),
                        delta: Some(Vec2::new(delta.x * scale, delta.y * scale)),
                    });
                    run.last_seen = None;
                    run.waiting = 0;
                }
            }
            // Laid out but clipped: scroll toward it instead of clicking a
            // coordinate that is showing something else.
            Some(widget) => {
                let screen = ctx.screen_rect();
                let below = widget.rect.center().y > screen.center().y;
                // The wheel goes to whatever the pointer is over, so park it
                // mid-screen — over the scroll area, not over the chrome.
                let park = screen.center();
                if run.cursor != park {
                    let delta = park - run.cursor;
                    run.cursor = park;
                    cursor_moved.write(CursorMoved {
                        window,
                        position: Vec2::new(park.x * scale, park.y * scale),
                        delta: Some(Vec2::new(delta.x * scale, delta.y * scale)),
                    });
                }
                mouse_wheel.write(MouseWheel {
                    unit: MouseScrollUnit::Line,
                    x: 0.0,
                    // egui's convention matches a real wheel: positive y moves
                    // the content down, so revealing something BELOW the
                    // viewport needs a negative delta.
                    y: if below { -SCROLL_LINES } else { SCROLL_LINES },
                    window,
                });
                run.waiting += 1;
                if run.waiting > run.timeout {
                    let timeout = run.timeout;
                    run.fail(format!(
                        "widget `{id}` is laid out but never scrolled into view \
                         ({timeout} frames)"
                    ));
                    commands.entity(window).despawn();
                    return;
                }
                run.queue.push_front(Micro::Move { id });
            }
            None => {
                // Not drawn yet. Put the move back and try again next frame.
                run.waiting += 1;
                if run.waiting > run.timeout {
                    let known = describe_widgets(&frame);
                    let timeout = run.timeout;
                    run.fail(format!(
                        "widget `{id}` never appeared ({timeout} frames). \
                         Registered on the last frame: {known}"
                    ));
                    commands.entity(window).despawn();
                    return;
                }
                run.queue.push_front(Micro::Move { id });
            }
        },
        Micro::Press(button) => {
            mouse_buttons.write(MouseButtonInput {
                button: mouse_button_of(button),
                state: ButtonState::Pressed,
                window,
            });
        }
        Micro::Release(button) => {
            mouse_buttons.write(MouseButtonInput {
                button: mouse_button_of(button),
                state: ButtonState::Released,
                window,
            });
        }
        Micro::KeyDown(key) => {
            let (key_code, logical_key) = key_codes(key);
            keyboard.write(KeyboardInput {
                key_code,
                logical_key,
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window,
            });
        }
        Micro::KeyUp(key) => {
            let (key_code, logical_key) = key_codes(key);
            keyboard.write(KeyboardInput {
                key_code,
                logical_key,
                state: ButtonState::Released,
                text: None,
                repeat: false,
                window,
            });
        }
        Micro::Check(step) => match evaluate(&step, &frame, now, &encyclopedia) {
            Ok(lines) => {
                for line in lines {
                    run.log.line(&line);
                }
            }
            Err(reason) => {
                run.fail(reason);
                commands.entity(window).despawn();
                return;
            }
        },
    }

    registry::arm(&ctx);
}

/// Evaluate one assertion against the last drawn frame.
///
/// Pure, and `pub` on purpose: this is where every `assert-*` verb decides,
/// and `tests/ui_driver.rs` drives it directly. `Ok` carries the lines to log
/// on success, so the decision holds no reference to the run.
pub fn evaluate(
    step: &Step,
    frame: &Frame,
    state: GameState,
    encyclopedia: &EncyclopediaState,
) -> Result<Vec<String>, String> {
    let ok = |line: String| Ok(vec![line]);
    match step {
        Step::AssertState { state: want } => {
            let got = format!("{state:?}");
            if &got == want {
                ok(format!("  ok  state == {got}"))
            } else {
                Err(format!("expected state {want}, got {got}"))
            }
        }
        Step::AssertView { topic: want } => {
            if state != GameState::Encyclopedia {
                return Err(format!(
                    "assert-view needs the Encyclopedia on screen, got {state:?}"
                ));
            }
            let got = current_topic(encyclopedia);
            if &got == want {
                ok(format!("  ok  view == {got}"))
            } else {
                Err(format!("expected view {want}, got {got}"))
            }
        }
        Step::AssertNote { needle } => {
            if frame.has_note(needle) {
                ok(format!("  ok  note ~ {needle}"))
            } else {
                Err(format!(
                    "no note matched `{needle}`. Notes on the last frame: {:?}",
                    frame.notes
                ))
            }
        }
        Step::AssertNoNote { needle } => {
            if frame.has_note(needle) {
                Err(format!(
                    "a note matched `{needle}` and should not have. Notes: {:?}",
                    frame.notes
                ))
            } else {
                ok(format!("  ok  no note ~ {needle}"))
            }
        }
        // `assert-visible` and `assert-absent` are the two halves of the SAME
        // three-way question, and both have to answer all three cases or the
        // pair stops being a pair. A widget is: not drawn, drawn but clipped,
        // or drawn and on screen. The middle case is the dangerous one —
        // `assert-absent` used to pass on it, so "this row is gone" was
        // satisfied by a row that was merely scrolled out of sight. That is
        // the Trinket1 trap again (an assertion succeeding for a reason other
        // than the one it names), and it is worst here, because a negative is
        // exactly what nobody re-checks by hand.
        Step::AssertVisible { id } => match presence(frame, id) {
            Presence::Visible => ok(format!("  ok  visible {id}")),
            Presence::Clipped => Err(format!(
                "widget `{id}` is laid out but scrolled out of view. `hover` or \
                 `click` it first — those scroll a target into view; an assertion \
                 does not."
            )),
            Presence::NotDrawn => Err(format!(
                "widget `{id}` was not drawn. Registered: {}",
                describe_widgets(frame)
            )),
        },
        Step::AssertAbsent { id } => match presence(frame, id) {
            Presence::NotDrawn => ok(format!("  ok  absent {id}")),
            Presence::Visible => {
                let rect = frame.widget(id).map(|w| w.rect);
                Err(format!("widget `{id}` was drawn, on screen, at {rect:?}"))
            }
            // NOT a pass. `assert-absent` asks whether the widget EXISTS, and
            // a clipped one does.
            Presence::Clipped => {
                let rect = frame.widget(id).map(|w| w.rect);
                Err(format!(
                    "widget `{id}` WAS drawn, at {rect:?}, but is scrolled out of \
                     view — so `assert-absent` cannot answer. It asks whether the \
                     widget exists at all, not whether it is on screen. Scroll it \
                     into view (`hover` it) and assert what you actually expect."
                ))
            }
        },
        Step::AssertEnabled { id, enabled } => match frame.widget(id) {
            Some(w) if w.enabled == *enabled => ok(format!("  ok  {id} enabled == {enabled}")),
            Some(w) => Err(format!(
                "widget `{id}` enabled == {}, expected {enabled}",
                w.enabled
            )),
            None => Err(format!(
                "widget `{id}` was not drawn. Registered: {}",
                describe_widgets(frame)
            )),
        },
        Step::Dump => {
            let mut lines = vec![format!("  dump state={state:?}")];
            if state == GameState::Encyclopedia {
                lines.push(format!("  dump view={}", current_topic(encyclopedia)));
            }
            lines.extend(frame.widgets.iter().map(|w| {
                format!(
                    "  dump widget {} rect=({:.0},{:.0})-({:.0},{:.0}) enabled={} visible={}",
                    w.id,
                    w.rect.min.x,
                    w.rect.min.y,
                    w.rect.max.x,
                    w.rect.max.y,
                    w.enabled,
                    w.visible
                )
            }));
            lines.extend(frame.notes.iter().map(|n| format!("  dump note {n}")));
            Ok(lines)
        }
        // `expand` never routes a non-assertion step here.
        other => Err(format!("internal: {other:?} is not an assertion")),
    }
}

/// How a widget stood on the last drawn frame. The three cases `assert-visible`
/// and `assert-absent` must both distinguish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    /// Nothing registered under this id.
    NotDrawn,
    /// Registered, but outside its clip rect — laid out, unreachable.
    Clipped,
    /// Registered and on screen.
    Visible,
}

/// Classify a widget on the last drawn frame.
pub fn presence(frame: &Frame, id: &str) -> Presence {
    match frame.widget(id) {
        None => Presence::NotDrawn,
        Some(w) if w.visible => Presence::Visible,
        Some(_) => Presence::Clipped,
    }
}
