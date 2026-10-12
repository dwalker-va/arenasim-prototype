//! Every screen's chrome, decided in one place: which template it is, where
//! its exit goes, and which snapshot harness pins it.
//!
//! [`GameState::chrome`] and [`GameState::harness`] are exhaustive matches
//! with no wildcard, so a new screen does not compile until it answers both
//! (design doc `docs/design/proposals/2026-10-11-ui-design-system.md` §3.4b).
//!
//! # Exits are contextual
//!
//! A screen's exit returns to the screen that OPENED it. Opening is the one
//! forward move, [`ScreenNav::open`], and it records the opener in
//! [`ScreenContext`]; leaving never records anything, so walking back out
//! leaves every opener in place. The exit button names its destination from
//! that record ([`Nav::exit_label`]), and Esc reads the same record
//! ([`ScreenNav::escape`]), so the key and the button cannot disagree.
//!
//! A screen nobody opened — `--replay` boots straight into a match — falls
//! back to the destination its [`Exit`] declares.

use std::collections::HashMap;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use super::GameState;
use crate::keybindings::{GameAction, Keybindings};
use crate::ui::widgets::{Nav, NavAction};

/// The shape a screen is laid out in (design doc §2.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    /// Full-bleed scene, centred card, wordmark, a vertical menu.
    Hero,
    /// Header, optional tabs and search, a scrolling body.
    Document,
    /// Header, N columns of panels, a docked footer with one Primary.
    Setup,
    /// Header, a display banner, data panels.
    Report,
    /// Header, side panels, a bottom transport.
    Workbench,
    /// Anchored areas over the 3D scene, no header.
    Hud,
}

/// Where a screen's way out goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// No way out in the chrome. The main menu's own EXIT quits the game.
    None,
    /// One exit, to the screen that opened this one; `fallback` when nothing
    /// did.
    Return { fallback: GameState },
    /// History first (`◀ BACK`), then out to the screen that opened this one;
    /// `fallback` when nothing did.
    Stacked { fallback: GameState },
}

impl Exit {
    fn fallback(self) -> Option<GameState> {
        match self {
            Exit::None => None,
            Exit::Return { fallback } | Exit::Stacked { fallback } => Some(fallback),
        }
    }
}

/// A screen's answer to the two chrome questions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chrome {
    pub template: Template,
    pub exit: Exit,
}

/// The snapshot harness that pins a screen, or why it has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenHarness {
    /// The test file, relative to the repo root.
    Snapshot(&'static str),
    /// No harness yet, and the reason.
    None(&'static str),
}

impl GameState {
    /// Which template this screen is, and where its exit goes.
    ///
    /// Exhaustive, no wildcard: a new screen does not compile until it
    /// answers.
    pub fn chrome(self) -> Chrome {
        let chrome = |template, exit| Chrome { template, exit };
        let back_to = |fallback| Exit::Return { fallback };
        match self {
            GameState::MainMenu => chrome(Template::Hero, Exit::None),
            GameState::Options => chrome(Template::Document, back_to(GameState::MainMenu)),
            GameState::Keybindings => chrome(Template::Document, back_to(GameState::Options)),
            GameState::ConfigureMatch => chrome(Template::Setup, back_to(GameState::MainMenu)),
            GameState::ViewCombatant => {
                chrome(Template::Document, back_to(GameState::ConfigureMatch))
            }
            // A match has no visible exit; Esc abandons it to whatever
            // started it.
            GameState::PlayMatch => chrome(Template::Hud, back_to(GameState::MainMenu)),
            GameState::Results => chrome(Template::Report, back_to(GameState::MainMenu)),
            GameState::Encyclopedia => chrome(
                Template::Document,
                Exit::Stacked {
                    fallback: GameState::MainMenu,
                },
            ),
            GameState::AnimationSandbox => {
                chrome(Template::Workbench, back_to(GameState::MainMenu))
            }
        }
    }

    /// The snapshot harness that pins this screen, or why it has none.
    ///
    /// A decision, not a list: a new screen names its harness file or states
    /// why it has none. `chrome::tests` checks that every named file exists.
    pub fn harness(self) -> ScreenHarness {
        match self {
            GameState::MainMenu => ScreenHarness::Snapshot("tests/main_menu_snapshot.rs"),
            GameState::Options | GameState::Keybindings => ScreenHarness::None(
                "no pure draw function to drive yet; the split and harness are UI 11/14",
            ),
            GameState::ConfigureMatch => {
                ScreenHarness::Snapshot("tests/configure_match_snapshot.rs")
            }
            GameState::ViewCombatant => ScreenHarness::Snapshot("tests/view_combatant_snapshot.rs"),
            GameState::PlayMatch => ScreenHarness::Snapshot("tests/team_frames_snapshot.rs"),
            GameState::Results => ScreenHarness::Snapshot("tests/results_screen_snapshot.rs"),
            GameState::Encyclopedia => ScreenHarness::Snapshot("tests/encyclopedia_snapshot.rs"),
            GameState::AnimationSandbox => {
                ScreenHarness::Snapshot("tests/animation_sandbox_snapshot.rs")
            }
        }
    }
}

/// Who opened each screen: the shared per-screen context behind every
/// contextual exit.
///
/// Written only by [`ScreenNav::open`] and [`ScreenNav::open_match`],
/// read by every exit label and by Esc.
#[derive(Resource, Debug, Default, Clone)]
pub struct ScreenContext {
    openers: HashMap<GameState, GameState>,
}

impl ScreenContext {
    /// Record that `from` opened `screen`: its exit returns there.
    pub fn open_from(&mut self, screen: GameState, from: GameState) {
        self.openers.insert(screen, from);
    }

    /// Where `screen`'s exit goes: its recorded opener, else the fallback its
    /// chrome declares. `None` for a screen with no exit.
    pub fn return_to(&self, screen: GameState) -> Option<GameState> {
        let fallback = screen.chrome().exit.fallback()?;
        Some(self.openers.get(&screen).copied().unwrap_or(fallback))
    }

    /// `screen`'s header navigation, resolved against who opened it.
    /// `can_back` is whether a stacked screen has history; flat screens
    /// ignore it.
    pub fn nav(&self, screen: GameState, can_back: bool) -> Option<Nav> {
        let return_to = self.return_to(screen)?;
        Some(match screen.chrome().exit {
            Exit::None => return None,
            Exit::Return { .. } => Nav::Return(return_to),
            Exit::Stacked { .. } => Nav::Stacked {
                can_back,
                return_to,
            },
        })
    }
}

/// The one Esc rule, given that the key went down. A screen's own rungs come
/// first — close a modal, clear a search — and `rung` runs them, returning
/// true when one took the key. Once they are spent, Esc does exactly what the
/// header does: [`Nav::on_escape`], Back while there is history, then out. A
/// screen with no exit ignores it.
pub fn escape_ladder(nav: Option<Nav>, rung: impl FnOnce() -> bool) -> Option<NavAction> {
    if rung() {
        return None;
    }
    nav.map(Nav::on_escape)
}

/// Everything a screen's Bevy wrapper needs to move between screens: the
/// current screen, who opened it, the Back key, and the transition.
///
/// Every forward move goes through [`Self::open`], so an opener is recorded by
/// construction; every way out goes through [`Self::leave`].
#[derive(SystemParam)]
pub struct ScreenNav<'w> {
    state: Res<'w, State<GameState>>,
    next: ResMut<'w, NextState<GameState>>,
    context: ResMut<'w, ScreenContext>,
    keybindings: Res<'w, Keybindings>,
    keyboard: Res<'w, ButtonInput<KeyCode>>,
}

impl ScreenNav<'_> {
    /// The screen on show.
    pub fn screen(&self) -> GameState {
        *self.state.get()
    }

    /// This screen's header navigation. See [`ScreenContext::nav`].
    pub fn nav(&self, can_back: bool) -> Option<Nav> {
        self.context.nav(self.screen(), can_back)
    }

    /// Where `screen`'s exit goes. See [`ScreenContext::return_to`].
    pub fn return_to(&self, screen: GameState) -> Option<GameState> {
        self.context.return_to(screen)
    }

    /// The Back key (Esc by default) went down this frame.
    pub fn back_pressed(&self) -> bool {
        self.keybindings
            .action_just_pressed(GameAction::Back, &self.keyboard)
    }

    /// What the Back key asks of this screen this frame, if it went down:
    /// [`escape_ladder`] over the screen's own `rung` and its nav.
    pub fn escape(&self, can_back: bool, rung: impl FnOnce() -> bool) -> Option<NavAction> {
        if !self.back_pressed() {
            return None;
        }
        escape_ladder(self.nav(can_back), rung)
    }

    /// Open `to` from this screen: its exit will return here.
    pub fn open(&mut self, to: GameState) {
        let from = self.screen();
        self.context.open_from(to, from);
        self.next.set(to);
    }

    /// Start the match. It runs in the context its setup was opened from, so
    /// Esc out of the match and the Results screen's exit both return there —
    /// the main menu today, a run hub for a future Gauntlet — rather than to
    /// the setup screen itself.
    pub fn open_match(&mut self) {
        let origin = self.return_to(self.screen()).unwrap_or(GameState::MainMenu);
        self.context.open_from(GameState::Results, origin);
        self.context.open_from(GameState::PlayMatch, origin);
        self.next.set(GameState::PlayMatch);
    }

    /// Leave for `to`, recording nothing: the way back out.
    pub fn leave(&mut self, to: GameState) {
        self.next.set(to);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [GameState; 9] = [
        GameState::MainMenu,
        GameState::Options,
        GameState::Keybindings,
        GameState::ConfigureMatch,
        GameState::ViewCombatant,
        GameState::PlayMatch,
        GameState::Results,
        GameState::Encyclopedia,
        GameState::AnimationSandbox,
    ];

    /// A guard on [`ALL`] itself: the match has no wildcard, so adding a
    /// state breaks this until the list names it.
    #[test]
    fn all_names_every_state() {
        for state in ALL {
            match state {
                GameState::MainMenu
                | GameState::Options
                | GameState::Keybindings
                | GameState::ConfigureMatch
                | GameState::ViewCombatant
                | GameState::PlayMatch
                | GameState::Results
                | GameState::Encyclopedia
                | GameState::AnimationSandbox => {}
            }
        }
    }

    #[test]
    fn every_named_harness_exists() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        for state in ALL {
            match state.harness() {
                ScreenHarness::Snapshot(path) => assert!(
                    root.join(path).is_file(),
                    "{state:?} names {path} as its harness, and there is no such file"
                ),
                ScreenHarness::None(reason) => {
                    assert!(!reason.trim().is_empty(), "{state:?} gives no reason")
                }
            }
        }
    }

    /// Only the main menu has no way out; no screen falls back to itself.
    #[test]
    fn every_screen_but_the_main_menu_has_an_exit_elsewhere() {
        let context = ScreenContext::default();
        for state in ALL {
            match context.return_to(state) {
                None => assert_eq!(state, GameState::MainMenu),
                Some(to) => assert_ne!(to, state, "{state:?} exits to itself"),
            }
        }
    }

    /// The exit follows whoever opened the screen, and names it.
    #[test]
    fn the_exit_returns_to_the_opener_and_names_it() {
        let mut context = ScreenContext::default();
        assert_eq!(
            context.nav(GameState::Results, false),
            Some(Nav::Return(GameState::MainMenu)),
            "nothing opened it: the declared fallback"
        );

        context.open_from(GameState::Encyclopedia, GameState::ViewCombatant);
        let nav = context.nav(GameState::Encyclopedia, true).unwrap();
        assert_eq!(nav.destination(), GameState::ViewCombatant);
        assert_eq!(nav.exit_label(), "\u{1f3e0} COMBATANT");
        assert_eq!(nav.on_escape(), NavAction::Back, "history first");
        assert_eq!(
            context
                .nav(GameState::Encyclopedia, false)
                .unwrap()
                .on_escape(),
            NavAction::Exit(GameState::ViewCombatant)
        );

        context.open_from(GameState::Results, GameState::Encyclopedia);
        assert_eq!(
            context.nav(GameState::Results, false).unwrap().exit_label(),
            "\u{25c0} ENCYCLOPEDIA"
        );
        assert_eq!(context.nav(GameState::MainMenu, false), None);
    }
}
