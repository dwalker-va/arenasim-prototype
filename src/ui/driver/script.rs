//! The script language: one step per line, one verb per line.
//!
//! Parsing is deliberately dumb — split on whitespace, match the verb. The
//! whole point is that a script is readable in a diff and writable without
//! looking anything up.
//!
//! ```text
//! # Comments and blank lines are ignored.
//! click menu:MATCH            # a left click, by default
//! click equip:Ring1 right     # right click
//! hover kit:MortalStrike      # park the cursor, no button
//! key Escape
//! wait 10                     # extra frames, on top of each step's own settle
//! assert-state Encyclopedia
//! assert-view Ability(MortalStrike)
//! assert-note tooltip:ability:MortalStrike
//! assert-no-note equip-row:Ring2:override
//! assert-visible equip:restore
//! assert-absent pick:BandOfAccuria
//! assert-enabled equip:restore false
//! dump
//! ```
//!
//! An unknown verb is a PARSE error, not a skipped line: a typo in a script
//! must not quietly reduce what the script checks.

use std::fmt;
use std::path::Path;

/// Which mouse button a `click` step presses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

impl fmt::Display for Button {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Button::Left => write!(f, "left"),
            Button::Right => write!(f, "right"),
        }
    }
}

/// A named key a `key` step can press.
///
/// Deliberately a short closed list rather than a full keymap: every key here
/// is one a UI script has a reason to send, and an unrecognised name fails at
/// parse time instead of sending nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedKey {
    Escape,
    Enter,
    Tab,
    Space,
    Backspace,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
}

impl NamedKey {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "Escape" | "Esc" => NamedKey::Escape,
            "Enter" | "Return" => NamedKey::Enter,
            "Tab" => NamedKey::Tab,
            "Space" => NamedKey::Space,
            "Backspace" => NamedKey::Backspace,
            "ArrowLeft" | "Left" => NamedKey::ArrowLeft,
            "ArrowRight" | "Right" => NamedKey::ArrowRight,
            "ArrowUp" | "Up" => NamedKey::ArrowUp,
            "ArrowDown" | "Down" => NamedKey::ArrowDown,
            _ => return None,
        })
    }
}

/// One line of a script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Park the synthetic cursor over a registered widget.
    Hover { id: String },
    /// Hover, then press and release a button over a registered widget.
    Click { id: String, button: Button },
    /// Press and release a key.
    Key(NamedKey),
    /// Burn frames. Each step already settles; this is for a screen that needs
    /// longer (an asset load, a state transition that spans several frames).
    Wait { frames: u32 },
    /// The `GameState` the app must currently be in, by its Rust variant name.
    AssertState { state: String },
    /// The encyclopedia's current topic, as its `Debug` form
    /// (`Ability(MortalStrike)`, `Item(BandOfAccuria)`, `Class(Warrior)`), or
    /// `none` for a section index. Fails outside the encyclopedia.
    AssertView { topic: String },
    /// Some note from the last drawn frame contains this substring.
    AssertNote { needle: String },
    /// No note from the last drawn frame contains this substring.
    AssertNoNote { needle: String },
    /// A widget with this id was drawn on the last frame.
    AssertVisible { id: String },
    /// No widget with this id was drawn on the last frame.
    AssertAbsent { id: String },
    /// A widget with this id was drawn, with this enabled state.
    AssertEnabled { id: String, enabled: bool },
    /// Log everything the last drawn frame reported.
    Dump,
}

impl Step {
    /// A one-line rendering, used for the log's per-step line.
    pub fn describe(&self) -> String {
        match self {
            Step::Hover { id } => format!("hover {id}"),
            Step::Click { id, button } => format!("click {id} {button}"),
            Step::Key(k) => format!("key {k:?}"),
            Step::Wait { frames } => format!("wait {frames}"),
            Step::AssertState { state } => format!("assert-state {state}"),
            Step::AssertView { topic } => format!("assert-view {topic}"),
            Step::AssertNote { needle } => format!("assert-note {needle}"),
            Step::AssertNoNote { needle } => format!("assert-no-note {needle}"),
            Step::AssertVisible { id } => format!("assert-visible {id}"),
            Step::AssertAbsent { id } => format!("assert-absent {id}"),
            Step::AssertEnabled { id, enabled } => format!("assert-enabled {id} {enabled}"),
            Step::Dump => "dump".to_string(),
        }
    }
}

/// A parsed script: the steps, each tagged with the source line it came from
/// so a failure reports `script.txt:12` rather than `step 7`.
#[derive(Clone, Debug, PartialEq)]
pub struct Script {
    pub name: String,
    pub steps: Vec<(usize, Step)>,
}

/// A parse failure, located.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl Script {
    /// Parse a script from a file, naming it by its path.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read UI script {}: {e}", path.display()))?;
        Self::parse(&path.display().to_string(), &text)
            .map_err(|e| format!("{}:{}", path.display(), e))
    }

    /// Parse a script from text.
    pub fn parse(name: &str, text: &str) -> Result<Self, ParseError> {
        let mut steps = Vec::new();
        for (idx, raw) in text.lines().enumerate() {
            let line_no = idx + 1;
            // A '#' starts a comment anywhere, so a step can carry a trailing
            // note. Ids and note needles never contain '#'.
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            steps.push((line_no, parse_step(line_no, line)?));
        }
        Ok(Script {
            name: name.to_string(),
            steps,
        })
    }
}

fn err(line: usize, message: impl Into<String>) -> ParseError {
    ParseError {
        line,
        message: message.into(),
    }
}

fn parse_step(line_no: usize, line: &str) -> Result<Step, ParseError> {
    let mut parts = line.split_whitespace();
    let verb = parts.next().expect("non-empty line has a first token");
    // The remainder of the line, verbatim after the verb — what the
    // substring-matching verbs want, since a note may contain spaces.
    let rest = line[verb.len()..].trim().to_string();

    let one_word = |what: &str| -> Result<String, ParseError> {
        let mut it = rest.split_whitespace();
        let first = it
            .next()
            .ok_or_else(|| err(line_no, format!("`{verb}` needs {what}")))?;
        match it.next() {
            None => Ok(first.to_string()),
            Some(extra) => Err(err(
                line_no,
                format!("`{verb}` takes one {what}, got a second token `{extra}`"),
            )),
        }
    };

    Ok(match verb {
        "hover" => Step::Hover {
            id: one_word("a widget id")?,
        },
        "click" => {
            let mut it = rest.split_whitespace();
            let id = it
                .next()
                .ok_or_else(|| err(line_no, "`click` needs a widget id"))?
                .to_string();
            let button = match it.next() {
                None | Some("left") => Button::Left,
                Some("right") => Button::Right,
                Some(other) => {
                    return Err(err(
                        line_no,
                        format!("`click` takes `left` or `right`, got `{other}`"),
                    ))
                }
            };
            if let Some(extra) = it.next() {
                return Err(err(line_no, format!("`click` got a stray token `{extra}`")));
            }
            Step::Click { id, button }
        }
        "key" => {
            let name = one_word("a key name")?;
            let key = NamedKey::parse(&name).ok_or_else(|| {
                err(
                    line_no,
                    format!(
                        "unknown key `{name}` (Escape, Enter, Tab, Space, Backspace, \
                         ArrowLeft/Right/Up/Down)"
                    ),
                )
            })?;
            Step::Key(key)
        }
        "wait" => {
            let n = one_word("a frame count")?;
            let frames = n
                .parse::<u32>()
                .map_err(|_| err(line_no, format!("`wait` needs a frame count, got `{n}`")))?;
            Step::Wait { frames }
        }
        "assert-state" => Step::AssertState {
            state: one_word("a GameState name")?,
        },
        "assert-view" => Step::AssertView {
            topic: one_word("a Topic (or `none`)")?,
        },
        "assert-note" => {
            if rest.is_empty() {
                return Err(err(line_no, "`assert-note` needs a substring"));
            }
            Step::AssertNote { needle: rest }
        }
        "assert-no-note" => {
            if rest.is_empty() {
                return Err(err(line_no, "`assert-no-note` needs a substring"));
            }
            Step::AssertNoNote { needle: rest }
        }
        "assert-visible" => Step::AssertVisible {
            id: one_word("a widget id")?,
        },
        "assert-absent" => Step::AssertAbsent {
            id: one_word("a widget id")?,
        },
        "assert-enabled" => {
            let mut it = rest.split_whitespace();
            let id = it
                .next()
                .ok_or_else(|| err(line_no, "`assert-enabled` needs a widget id"))?
                .to_string();
            let enabled = match it.next() {
                Some("true") => true,
                Some("false") => false,
                Some(other) => {
                    return Err(err(
                        line_no,
                        format!("`assert-enabled` takes `true` or `false`, got `{other}`"),
                    ))
                }
                None => return Err(err(line_no, "`assert-enabled` needs `true` or `false`")),
            };
            Step::AssertEnabled { id, enabled }
        }
        "dump" => {
            if !rest.is_empty() {
                return Err(err(
                    line_no,
                    format!("`dump` takes no arguments, got `{rest}`"),
                ));
            }
            Step::Dump
        }
        other => {
            return Err(err(
                line_no,
                format!(
                    "unknown step `{other}` (hover, click, key, wait, assert-state, \
                     assert-view, assert-note, assert-no-note, assert-visible, \
                     assert-absent, assert-enabled, dump)"
                ),
            ))
        }
    })
}
