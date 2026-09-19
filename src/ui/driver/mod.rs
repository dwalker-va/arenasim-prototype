//! Scripted client input — the loop that lets an agent verify the RUNNING
//! client without being able to see it.
//!
//! `screencapture` and `osascript` are permission-blocked on the development
//! machine, so client verification used to mean log-grepping a temporary
//! state-cycler: enough to prove "no panic", not enough to prove "hovering
//! this icon shows a tooltip" or "Restore defaults empties the override map".
//! Two Engineers built the same throwaway injector in one afternoon and both
//! reverted it before pushing. This is that tool, kept.
//!
//! # Shape
//!
//! * [`registry`] — one call, [`mark`], at the widgets that matter, so a
//!   script can name them; one call, [`note`], where only the draw can see
//!   what happened (a tooltip closure running, a row's rendered colour).
//! * [`script`] — a line-per-step text format.
//! * [`runner`] — injects `CursorMoved` / `MouseButtonInput` / `KeyboardInput`
//!   at Bevy's own input events, below `bevy_egui`'s conversion and above
//!   winit, and checks the assertions against what the last frame drew.
//!
//! # Enabling it
//!
//! `cargo run --release -- --ui-script tests/ui-scripts/<name>.script`.
//! It composes with `--replay`, and is an ERROR alongside `--headless`,
//! `--matrix` or `--batch` — those have no window, dispatch first, and would
//! swallow the script silently (see [`crate::cli::Args::ui_script_conflict`]).
//! Without that flag [`UiDriverPlugin::build`] returns immediately: no
//! resource, no systems, and — because nothing ever calls [`registry::arm`] —
//! every [`mark`] and [`note`] in `src/states/` is a single failed hash lookup
//! that records nothing and formats nothing.
//!
//! "Formats nothing" is a property of the CALL SITE as much as of the
//! primitive: `format_args!` defers formatting but not the evaluation of its
//! arguments, so a note that assembles a string in the argument position pays
//! for it every frame regardless. See the rule in [`registry`].
//!
//! That claim is about ABSENCE, which a passing test does not demonstrate, so
//! `tests/ui_driver.rs` proves it by mutation instead: it runs the identical
//! calls with the driver off and on and requires the results to DIFFER, at
//! both layers (the registry's writes, and the systems the plugin schedules).
//!
//! # What this cannot tell you
//!
//! It proves a handler runs when the event reaches it. It cannot prove the
//! player's input reaches the handler — see the Ctrl+click limit in
//! `docs/solutions/workflows/client-input-injection-driver.md`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bevy::prelude::*;

pub mod registry;
pub mod runner;
pub mod script;

pub use registry::{mark, note};
pub use script::Script;

/// Frames of settle after each injected event. Three is comfortably more than
/// the two egui needs to turn a pointer move into a `hovered()` response.
pub const DEFAULT_SETTLE_FRAMES: u32 = 3;

/// Frames a `hover` settles for, on top of the move itself.
///
/// Much longer than [`DEFAULT_SETTLE_FRAMES`] because a tooltip is not a
/// function of position alone: egui's `show_tooltips_only_when_still` gates it
/// on the pointer's VELOCITY falling to zero, measured over a ~0.1s history
/// window, and only then does `tooltip_delay` start. Three frames put the
/// assertion inside that window and the first run of the five-class script
/// read an empty tooltip on a hover that was in fact working. Half a second at
/// 60fps clears it with room for a frame-rate dip.
pub const DEFAULT_HOVER_SETTLE_FRAMES: u32 = 30;

/// Frames a step waits for the widget it names before failing. Ten seconds at
/// 60fps — long enough for an icon-loading frame hitch, short enough that a
/// misspelled id fails while you are still watching.
pub const DEFAULT_STEP_TIMEOUT_FRAMES: u32 = 600;

/// How a script run ended.
///
/// `Incomplete` is the initial value and survives if the window is closed
/// before the last step — which is a failure, not a pass: the script did not
/// finish, so nothing it was going to check was checked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Outcome {
    #[default]
    Incomplete,
    Passed,
    Failed(String),
}

/// The verdict, shared with `main` so the PROCESS can exit non-zero.
///
/// It cannot travel out through `AppExit`: writing that from a system
/// deadlocks the macOS winit loop, so the runner closes the window instead and
/// leaves the verdict here for `main` to read after `App::run` returns.
pub type OutcomeHandle = Arc<Mutex<Outcome>>;

/// Everything a run needs. Cloneable so the plugin can hold it by value and
/// hand it to the resource at `build` time.
#[derive(Clone)]
pub struct UiDriverConfig {
    pub script: Script,
    pub log_path: PathBuf,
    pub outcome: OutcomeHandle,
    pub settle_frames: u32,
    pub hover_settle_frames: u32,
    pub step_timeout_frames: u32,
}

impl UiDriverConfig {
    /// Load a script and prepare a run against `log_path`.
    pub fn load(script_path: &std::path::Path, log_path: PathBuf) -> Result<Self, String> {
        Ok(Self {
            script: Script::load(script_path)?,
            log_path,
            outcome: Arc::new(Mutex::new(Outcome::Incomplete)),
            settle_frames: DEFAULT_SETTLE_FRAMES,
            hover_settle_frames: DEFAULT_HOVER_SETTLE_FRAMES,
            step_timeout_frames: DEFAULT_STEP_TIMEOUT_FRAMES,
        })
    }
}

/// The driver's Bevy wiring.
///
/// Constructed [`UiDriverPlugin::disabled`] on every normal run. `build` then
/// returns before touching the app at all, which is the whole inertness
/// guarantee in one line — there is no "off" code path to get wrong, because
/// there is no code.
pub struct UiDriverPlugin {
    config: Option<UiDriverConfig>,
}

impl UiDriverPlugin {
    /// The normal client: no resource, no systems, no registry.
    pub fn disabled() -> Self {
        Self { config: None }
    }

    /// A scripted run.
    pub fn enabled(config: UiDriverConfig) -> Self {
        Self {
            config: Some(config),
        }
    }
}

impl Plugin for UiDriverPlugin {
    fn build(&self, app: &mut App) {
        let Some(config) = self.config.clone() else {
            return;
        };
        app.insert_resource(runner::UiScriptRun::new(config))
            // BEFORE `InputSystem`, so the injected events are folded into
            // `ButtonInput<KeyCode>` and then converted by bevy_egui's own
            // `ProcessInput` (which is configured `.after(InputSystem)`)
            // exactly as a physical event would be.
            .add_systems(
                PreUpdate,
                runner::run_ui_script.before(bevy::input::InputSystem),
            );
    }
}
