//! The UI input-injection driver: script parsing, and the two halves of the
//! inertness claim.
//!
//! # What the inertness tests are doing, and why they look odd
//!
//! "Compiled in normal builds but inert unless enabled" is a claim about
//! ABSENCE, and a passing test does not demonstrate absence — a test that goes
//! green with the driver disabled would go green just as happily if the driver
//! were disabled AND broken. So neither test here asserts "nothing happened".
//! Each runs the SAME sequence twice, once off and once on, and requires the
//! two runs to DIFFER. The enabled half is the non-vacuity check: if the probe
//! could not see the driver even when it is on, the test fails there first, and
//! the disabled half never gets to pass for the wrong reason.
//!
//! The two halves match the two ways the driver could leak into a normal run:
//!
//! * `registry_records_nothing_until_it_is_armed` — the per-widget `mark` /
//!   `note` calls that now sit in `src/states/`.
//! * `the_plugin_schedules_nothing_when_no_script_is_configured` — the Bevy
//!   systems and resource the plugin would otherwise add.

use bevy::prelude::*;
use bevy_egui::egui;

use arenasim::states::encyclopedia::EncyclopediaState;
use arenasim::states::GameState;
use arenasim::ui::driver::script::{Button, NamedKey, Script, Step};
use arenasim::ui::driver::{registry, runner, UiDriverConfig, UiDriverPlugin};

fn rect() -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(30.0, 40.0))
}

// ===========================================================================
// INERTNESS — the registry
// ===========================================================================

/// The opt-in calls write NOTHING until the driver arms the registry, and DO
/// write once it has. One context, one call sequence, run twice; the test is
/// the difference between the two halves, not either half alone.
#[test]
fn registry_records_nothing_until_it_is_armed() {
    let ctx = egui::Context::default();

    // --- disabled: exactly what a normal client run does ---
    //
    // The assertion reads `raw_frame`, NOT `snapshot`. `snapshot` re-checks the
    // armed flag, so it returns `None` for a leaking `mark` too — the first
    // draft of this test asserted on it and a mutation that deleted `mark`'s
    // armed check went UNDETECTED. `raw_frame` is the unguarded store.
    registry::mark_into(&ctx, rect(), true, true, format_args!("kit:MortalStrike"));
    registry::note_into(&ctx, format_args!("tooltip:Ability(MortalStrike)"));
    assert_eq!(
        registry::raw_frame(&ctx),
        None,
        "an unarmed registry must hold nothing — these calls are what every \
         normal client frame runs, and they must not RECORD. (Whether they \
         allocate is a separate claim, pinned by \
         note_does_not_format_its_arguments_when_disabled below, and it is a \
         property of the call site too — see the format_args! rule in \
         ui::driver::registry.)"
    );

    // --- enabled: the identical calls ---
    registry::arm(&ctx);
    registry::mark_into(&ctx, rect(), true, true, format_args!("kit:MortalStrike"));
    registry::note_into(&ctx, format_args!("tooltip:Ability(MortalStrike)"));

    let frame = registry::snapshot(&ctx).expect("an armed registry records a frame");
    assert_eq!(
        frame.widgets.len(),
        1,
        "the armed half must actually record, or the disabled half above proves nothing"
    );
    assert_eq!(frame.widgets[0].id, "kit:MortalStrike");
    assert_eq!(frame.widgets[0].rect, rect());
    assert!(frame.widgets[0].enabled);
    assert!(frame.widgets[0].visible);
    assert_eq!(
        frame.notes,
        vec!["tooltip:Ability(MortalStrike)".to_string()]
    );

    // --- disarmed again: back to nothing, and the old frame is dropped ---
    registry::disarm(&ctx);
    registry::mark_into(&ctx, rect(), true, true, format_args!("kit:MortalStrike"));
    assert_eq!(
        registry::raw_frame(&ctx),
        None,
        "disarming must drop the frame outright, not merely hide it behind the \
         armed flag"
    );
}

/// The text of a note is not FORMATTED while the driver is off.
///
/// This is the half of the inertness claim that `format_args!` buys, and the
/// half a call site can throw away: `format_args!` defers formatting but NOT
/// the evaluation of its argument expressions, so a `.collect().join()` in the
/// argument position runs every frame regardless of this. That is exactly what
/// `render_equipment_panel` did until review caught it; the fix was to pass a
/// `Display` wrapper, which is what this test pins the value of.
#[test]
fn note_does_not_format_its_arguments_when_disabled() {
    use std::cell::Cell;
    use std::fmt;

    struct Counting<'a>(&'a Cell<usize>);
    impl fmt::Display for Counting<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.0.set(self.0.get() + 1);
            write!(f, "payload")
        }
    }

    let formats = Cell::new(0);
    let ctx = egui::Context::default();

    registry::note_into(&ctx, format_args!("{}", Counting(&formats)));
    assert_eq!(
        formats.get(),
        0,
        "an unarmed `note` must not run its argument's Display impl — that is \
         what lets a call site hand it an expensive-to-render value for free"
    );

    registry::arm(&ctx);
    registry::note_into(&ctx, format_args!("{}", Counting(&formats)));
    assert_eq!(
        formats.get(),
        1,
        "the armed half must actually format, or the disabled half proves nothing"
    );
    assert!(registry::snapshot(&ctx).expect("armed").has_note("payload"));
}

/// `arm` starts a fresh frame: last frame's widgets and notes are gone, so an
/// assertion always reads what the LAST draw produced and never a stale one.
#[test]
fn arming_clears_the_previous_frame() {
    let ctx = egui::Context::default();

    registry::arm(&ctx);
    registry::mark_into(&ctx, rect(), true, true, format_args!("old"));
    registry::note_into(&ctx, format_args!("old note"));

    registry::arm(&ctx);
    registry::mark_into(&ctx, rect(), true, true, format_args!("new"));

    let frame = registry::snapshot(&ctx).expect("armed");
    assert_eq!(
        frame
            .widgets
            .iter()
            .map(|w| w.id.as_str())
            .collect::<Vec<_>>(),
        vec!["new"]
    );
    assert!(frame.notes.is_empty(), "notes must be cleared too");
}

/// Lookup helpers behave the way the assertions rely on.
#[test]
fn a_frame_finds_widgets_and_notes() {
    let ctx = egui::Context::default();
    registry::arm(&ctx);
    registry::mark_into(&ctx, rect(), false, true, format_args!("equip:restore"));
    registry::note_into(
        &ctx,
        format_args!("equip-row Ring1 item=SignetOfFocus override=true"),
    );

    let frame = registry::snapshot(&ctx).expect("armed");
    let w = frame.widget("equip:restore").expect("registered widget");
    assert!(
        !w.enabled,
        "a disabled widget still registers, marked disabled"
    );
    assert!(frame.widget("equip:nope").is_none());
    assert!(frame.is_visible("equip:restore"));
    assert!(!frame.is_visible("equip:nope"));
    assert!(frame.has_note("item=SignetOfFocus"));
    assert!(!frame.has_note("item=BandOfAccuria"));
}

/// A clipped widget still registers, so the driver can scroll toward it — but
/// it does NOT count as visible, so a script can never assert its way past a
/// target that is off screen.
#[test]
fn an_off_screen_widget_registers_but_is_not_visible() {
    let ctx = egui::Context::default();
    registry::arm(&ctx);
    registry::mark_into(
        &ctx,
        rect(),
        true,
        false,
        format_args!("strategic:BattleShout"),
    );

    let frame = registry::snapshot(&ctx).expect("armed");
    assert!(
        frame.widget("strategic:BattleShout").is_some(),
        "the driver needs the rect to know which way to scroll"
    );
    assert!(
        !frame.is_visible("strategic:BattleShout"),
        "laid out is not the same as on screen"
    );
}

/// The visibility flag comes from egui's own clip rect, not from the caller.
///
/// `an_off_screen_widget_registers_but_is_not_visible` hands `mark_into` the
/// flag directly, so it pins `Frame::is_visible` and nothing else. This one
/// runs a real egui pass so the `ui.is_rect_visible` call inside `mark` is
/// what decides — the failure mutation-testing exposed above was an assertion
/// reading through a gate instead of at the thing, and this is its neighbour.
#[test]
fn mark_takes_visibility_from_eguis_clip_rect() {
    let ctx = egui::Context::default();
    registry::arm(&ctx);

    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(800.0, 600.0),
        )),
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let on_screen =
                egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(20.0, 20.0));
            // Far below the 600pt viewport — the scrolled-out case.
            let scrolled_off =
                egui::Rect::from_min_size(egui::pos2(10.0, 5_000.0), egui::vec2(20.0, 20.0));
            registry::mark(ui, on_screen, true, format_args!("on-screen"));
            registry::mark(ui, scrolled_off, true, format_args!("scrolled-off"));
        });
    });

    let frame = registry::snapshot(&ctx).expect("armed");
    assert!(
        frame.widget("scrolled-off").is_some(),
        "an off-screen widget must still REGISTER, so the driver knows which \
         way to scroll"
    );
    assert!(frame.is_visible("on-screen"));
    assert!(!frame.is_visible("scrolled-off"));
}

// ===========================================================================
// INERTNESS — the plugin
// ===========================================================================

/// Every system this plugin owns, found by name across every schedule.
fn driver_systems(app: &App) -> Vec<String> {
    let schedules = app.world().resource::<bevy::ecs::schedule::Schedules>();
    let mut found: Vec<String> = Vec::new();
    for (_label, schedule) in schedules.iter() {
        for (_id, system, _conditions) in schedule.graph().systems() {
            let name = system.name().to_string();
            if name.contains("ui::driver") {
                found.push(name);
            }
        }
    }
    found.sort();
    found
}

fn script_config(dir: &std::path::Path) -> UiDriverConfig {
    let script = dir.join("probe.script");
    std::fs::write(&script, "assert-state MainMenu\ndump\n").expect("write probe script");
    UiDriverConfig::load(&script, dir.join("probe.log")).expect("probe script parses")
}

/// The plugin adds its systems and its resource ONLY when a script is
/// configured. Both halves run; the assertion is that they differ.
#[test]
fn the_plugin_schedules_nothing_when_no_script_is_configured() {
    let dir = tempfile::tempdir().expect("tempdir");

    let mut enabled = App::new();
    enabled.add_plugins(UiDriverPlugin::enabled(script_config(dir.path())));
    let enabled_systems = driver_systems(&enabled);

    let mut disabled = App::new();
    disabled.add_plugins(UiDriverPlugin::disabled());
    let disabled_systems = driver_systems(&disabled);

    // Non-vacuity first: if the probe cannot see the driver when it IS on,
    // the empty result below would mean nothing.
    assert!(
        !enabled_systems.is_empty(),
        "an enabled driver must schedule at least one system, else this test \
         cannot tell inert from invisible"
    );
    assert!(
        enabled_systems.iter().any(|n| n.contains("run_ui_script")),
        "expected the runner among {enabled_systems:?}"
    );
    assert!(
        enabled
            .world()
            .get_resource::<arenasim::ui::driver::runner::UiScriptRun>()
            .is_some(),
        "an enabled driver owns its run resource"
    );

    // The claim.
    assert_eq!(
        disabled_systems,
        Vec::<String>::new(),
        "a disabled driver must schedule NO systems; found {disabled_systems:?}"
    );
    assert!(
        disabled
            .world()
            .get_resource::<arenasim::ui::driver::runner::UiScriptRun>()
            .is_none(),
        "a disabled driver must not insert its run resource"
    );
}

// ===========================================================================
// SCRIPT PARSING
// ===========================================================================

fn steps(text: &str) -> Vec<Step> {
    Script::parse("probe", text)
        .expect("parses")
        .steps
        .into_iter()
        .map(|(_line, step)| step)
        .collect()
}

#[test]
fn every_verb_parses() {
    let parsed = steps(
        "\
hover kit:MortalStrike
click menu:MATCH
click equip:Ring1 left
click equip:Ring1 right
key Escape
wait 12
assert-state ViewCombatant
assert-view Ability(MortalStrike)
assert-note equip-row Ring1 item=SignetOfFocus override=true
assert-no-note tooltip:Item(BandOfAccuria)
assert-visible equip:restore
assert-absent pick:BandOfAccuria
assert-enabled equip:restore false
dump
",
    );

    assert_eq!(
        parsed,
        vec![
            Step::Hover {
                id: "kit:MortalStrike".into()
            },
            Step::Click {
                id: "menu:MATCH".into(),
                button: Button::Left
            },
            Step::Click {
                id: "equip:Ring1".into(),
                button: Button::Left
            },
            Step::Click {
                id: "equip:Ring1".into(),
                button: Button::Right
            },
            Step::Key(NamedKey::Escape),
            Step::Wait { frames: 12 },
            Step::AssertState {
                state: "ViewCombatant".into()
            },
            Step::AssertView {
                topic: "Ability(MortalStrike)".into()
            },
            Step::AssertNote {
                needle: "equip-row Ring1 item=SignetOfFocus override=true".into()
            },
            Step::AssertNoNote {
                needle: "tooltip:Item(BandOfAccuria)".into()
            },
            Step::AssertVisible {
                id: "equip:restore".into()
            },
            Step::AssertAbsent {
                id: "pick:BandOfAccuria".into()
            },
            Step::AssertEnabled {
                id: "equip:restore".into(),
                enabled: false
            },
            Step::Dump,
        ]
    );
}

#[test]
fn comments_and_blank_lines_are_skipped_but_line_numbers_are_not() {
    let script = Script::parse(
        "probe",
        "\
# a leading comment

hover kit:Ambush   # trailing comment

dump
",
    )
    .expect("parses");

    assert_eq!(
        script.steps,
        vec![
            (
                3,
                Step::Hover {
                    id: "kit:Ambush".into()
                }
            ),
            (5, Step::Dump),
        ],
        "a failure must be able to cite the SOURCE line, so blank and comment \
         lines still count toward the numbering"
    );
}

/// A typo must fail the script, not silently reduce what it checks. This is
/// the property that stops a mis-typed `assert-stat` from turning a real check
/// into a no-op line.
#[test]
fn an_unknown_verb_is_a_parse_error() {
    let err = Script::parse("probe", "hover a\nassert-stat MainMenu\n").expect_err("rejected");
    assert_eq!(err.line, 2);
    assert!(
        err.message.contains("unknown step `assert-stat`"),
        "{}",
        err.message
    );
}

#[test]
fn every_malformed_step_reports_its_own_line_and_reason() {
    for (text, line, needle) in [
        ("key Meta\n", 1usize, "unknown key `Meta`"),
        ("wait soon\n", 1, "needs a frame count"),
        ("click a middle\n", 1, "`left` or `right`"),
        ("hover\n", 1, "needs a widget id"),
        ("hover a b\n", 1, "got a second token"),
        (
            "assert-enabled equip:restore maybe\n",
            1,
            "`true` or `false`",
        ),
        ("assert-note\n", 1, "needs a substring"),
        ("dump now\n", 1, "takes no arguments"),
        ("hover a\n\nkey Nope\n", 3, "unknown key `Nope`"),
    ] {
        let err = Script::parse("probe", text).expect_err(&format!("{text:?} should not parse"));
        assert_eq!(err.line, line, "wrong line for {text:?}: {err}");
        assert!(
            err.message.contains(needle),
            "{text:?}: expected {needle:?} in {:?}",
            err.message
        );
    }
}

#[test]
fn key_aliases_resolve() {
    assert_eq!(NamedKey::parse("Esc"), Some(NamedKey::Escape));
    assert_eq!(NamedKey::parse("Escape"), Some(NamedKey::Escape));
    assert_eq!(NamedKey::parse("Left"), Some(NamedKey::ArrowLeft));
    assert_eq!(NamedKey::parse("escape"), None, "names are case-sensitive");
}

// ===========================================================================
// THE SHIPPED SCRIPTS
// ===========================================================================

/// Every shipped script parses, BY NAME.
///
/// The first version of this counted (`found >= 2`) and shipped alongside
/// three scripts, so deleting one left it green — a floor assertion whose
/// expectation does not move with the thing it guards. Naming them means a
/// rename or a lost file fails and says which; the second half means a NEW
/// script cannot be added without being parse-checked.
#[test]
fn every_shipped_script_parses() {
    const SHIPPED: &[&str] = &[
        "view-combatant-tooltips.script",
        "equipment-restore-defaults.script",
        "replay-boots-into-the-match.script",
    ];

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui-scripts");

    for name in SHIPPED {
        let path = dir.join(name);
        assert!(
            path.is_file(),
            "{name} is listed as shipped but is missing from {}",
            dir.display()
        );
        let script = Script::load(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!script.steps.is_empty(), "{name} parsed to zero steps");
    }

    let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
        .expect("tests/ui-scripts exists")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("script"))
        .map(|p| {
            p.file_name()
                .expect("script has a file name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    on_disk.sort();
    let mut expected: Vec<String> = SHIPPED.iter().map(|s| s.to_string()).collect();
    expected.sort();
    assert_eq!(
        on_disk, expected,
        "the scripts on disk and the SHIPPED list have diverged — add a new \
         script to the list so it gets parse-checked, or delete the stale entry"
    );
}

// ===========================================================================
// THE PURE SEAMS OF THE RUNNER
//
// The runner's frame loop needs a window and cannot be unit-tested; it is
// covered by running the scripts. Its two pure decisions can be, and both
// carry a fix a later "simplification" could quietly undo.
// ===========================================================================

fn frame_with(widgets: Vec<(&str, bool, bool)>, notes: Vec<&str>) -> registry::Frame {
    registry::Frame {
        widgets: widgets
            .into_iter()
            .map(|(id, enabled, visible)| registry::Widget {
                id: id.to_string(),
                rect: rect(),
                enabled,
                visible,
            })
            .collect(),
        notes: notes.into_iter().map(|n| n.to_string()).collect(),
    }
}

fn eval(step: Step, frame: &registry::Frame) -> Result<Vec<String>, String> {
    runner::evaluate(
        &step,
        frame,
        GameState::ViewCombatant,
        &EncyclopediaState::default(),
    )
}

/// `assert-visible` and `assert-absent` must BOTH answer all three cases.
///
/// The middle case is the finding this test exists for: `assert-absent` used
/// to return `!is_visible`, so a row that was merely scrolled out of view
/// satisfied "this is gone". A negative that looks conclusive and is not is
/// the worst kind, because nobody re-checks a passing one.
#[test]
fn presence_assertions_separate_not_drawn_from_scrolled_off() {
    let visible = frame_with(vec![("equip:Ring1", true, true)], vec![]);
    let clipped = frame_with(vec![("equip:Ring1", true, false)], vec![]);
    let missing = frame_with(vec![("equip:Ring2", true, true)], vec![]);

    let id = || "equip:Ring1".to_string();

    // assert-visible
    assert!(eval(Step::AssertVisible { id: id() }, &visible).is_ok());
    let e = eval(Step::AssertVisible { id: id() }, &clipped).expect_err("clipped is not visible");
    assert!(e.contains("scrolled out of view"), "{e}");
    let e = eval(Step::AssertVisible { id: id() }, &missing).expect_err("missing is not visible");
    assert!(e.contains("was not drawn"), "{e}");

    // assert-absent — the other half of the same three-way question
    assert!(
        eval(Step::AssertAbsent { id: id() }, &missing).is_ok(),
        "not drawn at all is the only case `assert-absent` may pass on"
    );
    let e = eval(Step::AssertAbsent { id: id() }, &visible).expect_err("drawn on screen");
    assert!(e.contains("was drawn, on screen"), "{e}");
    let e = eval(Step::AssertAbsent { id: id() }, &clipped)
        .expect_err("a clipped widget EXISTS, so assert-absent must not pass");
    assert!(e.contains("WAS drawn"), "{e}");
    assert!(e.contains("scrolled out of view"), "{e}");
}

#[test]
fn note_assertions_match_on_substrings_of_the_last_frame() {
    let frame = frame_with(
        vec![("equip:restore", false, true)],
        vec!["equip-row Ring1 item=SignetOfFocus override=true"],
    );

    assert!(eval(
        Step::AssertNote {
            needle: "item=SignetOfFocus".into()
        },
        &frame
    )
    .is_ok());
    assert!(eval(
        Step::AssertNote {
            needle: "item=BandOfAccuria".into()
        },
        &frame
    )
    .is_err());
    assert!(eval(
        Step::AssertNoNote {
            needle: "item=BandOfAccuria".into()
        },
        &frame
    )
    .is_ok());
    assert!(eval(
        Step::AssertNoNote {
            needle: "override=true".into()
        },
        &frame
    )
    .is_err());
}

/// A disabled widget still registers, and `assert-enabled` reads its state
/// whether or not it is on screen — deliberate, and what lets a script pin
/// "Restore defaults is dead with no overrides" before scrolling to it.
#[test]
fn assert_enabled_reads_state_not_visibility() {
    let offscreen_disabled = frame_with(vec![("equip:restore", false, false)], vec![]);
    assert!(eval(
        Step::AssertEnabled {
            id: "equip:restore".into(),
            enabled: false
        },
        &offscreen_disabled
    )
    .is_ok());
    let e = eval(
        Step::AssertEnabled {
            id: "equip:restore".into(),
            enabled: true,
        },
        &offscreen_disabled,
    )
    .expect_err("state mismatch");
    assert!(e.contains("enabled == false, expected true"), "{e}");
}

#[test]
fn assert_view_refuses_to_answer_outside_the_encyclopedia() {
    let frame = registry::Frame::default();
    let e = eval(
        Step::AssertView {
            topic: "Ability(MortalStrike)".into(),
        },
        &frame,
    )
    .expect_err("assert-view outside the encyclopedia is not a pass");
    assert!(e.contains("needs the Encyclopedia on screen"), "{e}");
}

/// A `hover` waits on egui's own gates; it does NOT count frames.
///
/// This replaced a 30-frame settle, and the replacement is the point. Every
/// gate egui applies to a tooltip is measured in SECONDS — the 0.1s velocity
/// window behind `is_still()`, the smooth-scroll animation, the
/// click-then-move rule — so a frame count is a wall-clock claim in the wrong
/// unit, and its correctness is a property of the machine that ran it.
/// Measured: 33 frames spanned 0.175-0.242s on a contended machine against a
/// 0.1s requirement, a margin under 2x, and the client runs uncapped when
/// vsync is off. It passed for two agents on a loaded machine and failed for
/// the user on an idle one.
///
/// If someone "simplifies" `SettleForTooltip` back into an `Idle`, this fails.
#[test]
fn a_hover_waits_on_eguis_gates_rather_than_counting_frames() {
    use arenasim::ui::driver::runner::Micro;

    let hover = runner::expand(
        &Step::Hover {
            id: "kit:Ambush".into(),
        },
        3,
    );
    assert_eq!(
        hover
            .iter()
            .filter(|m| matches!(m, Micro::SettleForTooltip))
            .count(),
        1,
        "a hover must wait on the tooltip gates: {hover:?}"
    );
    assert!(
        matches!(hover.front(), Some(Micro::Move { .. })),
        "aim first, then wait: {hover:?}"
    );
    // The trailing Idle is legitimately frame-shaped: it lets the pass draw
    // and the registry snapshot catch up. It is NOT the wait.
    assert!(
        hover.iter().any(|m| matches!(m, Micro::Idle(3))),
        "a hover still needs a frame or two for the draw to be observed: {hover:?}"
    );

    let click = runner::expand(
        &Step::Click {
            id: "kit:Ambush".into(),
            button: Button::Left,
        },
        3,
    );
    assert!(
        !click.iter().any(|m| matches!(m, Micro::SettleForTooltip)),
        "a click has no tooltip to wait for: {click:?}"
    );
    assert_eq!(
        click
            .iter()
            .filter(|m| matches!(m, Micro::Press(_) | Micro::Release(_)))
            .count(),
        2,
        "press and release are separate frames: {click:?}"
    );
}

/// Every assertion verb routes to `Check`, and only those. A new verb added
/// and forgotten here would reach `evaluate`'s catch-all and fail with
/// "not an assertion" at runtime instead of at review.
#[test]
fn only_assertions_expand_to_a_check() {
    use arenasim::ui::driver::runner::Micro;

    for step in [
        Step::AssertState {
            state: "MainMenu".into(),
        },
        Step::AssertView {
            topic: "none".into(),
        },
        Step::AssertNote { needle: "x".into() },
        Step::AssertNoNote { needle: "x".into() },
        Step::AssertVisible { id: "x".into() },
        Step::AssertAbsent { id: "x".into() },
        Step::AssertEnabled {
            id: "x".into(),
            enabled: true,
        },
        Step::Dump,
    ] {
        let q = runner::expand(&step, 3);
        assert!(
            matches!(q.front(), Some(Micro::Check(_))) && q.len() == 1,
            "{step:?} should expand to exactly one Check, got {q:?}"
        );
    }

    for step in [
        Step::Hover { id: "x".into() },
        Step::Click {
            id: "x".into(),
            button: Button::Right,
        },
        Step::Key(NamedKey::Escape),
        Step::Wait { frames: 5 },
    ] {
        let q = runner::expand(&step, 3);
        assert!(
            !q.iter().any(|m| matches!(m, Micro::Check(_))),
            "{step:?} is an action, not an assertion, got {q:?}"
        );
    }
}

/// The assertions that wait for something to ARRIVE retry; the ones satisfied
/// by ABSENCE never do.
///
/// Retrying a negative would mean "pass on the first frame before the thing
/// shows up" — a vacuous pass, and the shape this card has spent its life
/// removing. Retrying `assert-note` / `assert-visible` would quietly change
/// what they mean: they read the LAST DRAWN FRAME, the one the preceding step
/// set up.
#[test]
fn only_arrival_assertions_retry() {
    use arenasim::ui::driver::runner::is_retryable;

    for step in [
        Step::AssertState {
            state: "ViewCombatant".into(),
        },
        Step::AssertView {
            topic: "none".into(),
        },
    ] {
        assert!(
            is_retryable(&step),
            "{step:?} waits for something to arrive and must poll"
        );
    }

    for step in [
        Step::AssertAbsent { id: "x".into() },
        Step::AssertNoNote { needle: "x".into() },
        Step::AssertNote { needle: "x".into() },
        Step::AssertVisible { id: "x".into() },
        Step::AssertEnabled {
            id: "x".into(),
            enabled: true,
        },
        Step::Dump,
    ] {
        assert!(
            !is_retryable(&step),
            "{step:?} must NOT poll — a retried negative passes on the frame \
             before the thing appears, and a retried frame-read stops meaning \
             `the frame the last step produced`"
        );
    }
}

// ===========================================================================
// THE `format_args!` RULE, AS AN AUDIT
//
// `note_does_not_format_its_arguments_when_disabled` pins the PRIMITIVE's
// half of "formats nothing when disabled". The other half belongs to every
// call site, and a call site can throw it away without touching the driver at
// all: `format_args!` defers formatting but not the evaluation of its
// argument expressions, so `.collect::<Vec<_>>().join(",")` in the argument
// position runs on every frame with the driver off. That is exactly what
// `render_equipment_panel` did until review caught it.
//
// One bug is a fix; the class needs a guard. This walks the real call sites
// the way `registration_audit` walks systems.
//
// WHAT THIS AUDIT CANNOT SEE — read before trusting a green run:
//
//   * It is a SUBSTRING SCAN over source text, not a parse. It recognises
//     work by spelling (`EAGER_MARKERS`), so a helper function that allocates
//     internally — `ui_driver::note(ui, format_args!("{}", summarise(xs)))` —
//     is invisible to it. Only shapes written inline are caught.
//   * It finds calls by walking back from `mark(` / `note(` over a path of
//     `[A-Za-z0-9_:]` and keeping those whose path ends in `driver::`. So it
//     sees QUALIFIED calls in any spelling — `ui_driver::note(`,
//     `crate::ui::driver::note(`, `myui::driver::note(` — and nothing else.
//     In particular it does NOT see a bare call behind a direct import:
//
//         use crate::ui::driver::note;
//         note(ui, format_args!("{}", xs.join(",")));
//
//     That is a shape rustfmt produces happily. No call site uses it today
//     (every import is the module, not the function), and
//     `the_call_site_scanner_sees_every_spelling` pins that the scanner skips
//     it, so this comment and the behaviour cannot drift apart.
//   * A call split across a line break mid-path, or written with spaces
//     around `::`, is also invisible. rustfmt produces neither.
//   * It says nothing about cost that is not allocation.
//
// THE CENSUS IS NOT A BACKSTOP FOR THE ABOVE. `EXPECTED_CALL_SITES` catches a
// file whose count CHANGES, which is how a partially-seen file surfaces. But
// a file whose calls are ALL invisible contributes no entry at all, so the
// map comparison still balances and the file passes unseen. The census
// guards files the scanner can already see into; only the spelling test
// guards the scanner itself. That asymmetry is the whole reason the two
// claims are separate tests.
//
// The first version of this audit had a far worse blind spot and shipped
// green: it matched four hardcoded opener strings and skipped any match whose
// preceding character was `:`, so EVERY fully-qualified call was invisible —
// including `encyclopedia::widget`'s, the one note that every linked icon in
// the client funnels through. Its own non-vacuity plant used the spelling
// that worked, so the self-check could not reveal it either, and a
// `sites.len() >= 10` floor could not notice 20 sites found where 21 exist.
// Hence `EXPECTED_CALL_SITES` below: an exact per-file census, not a bound.
// ===========================================================================

/// Work that must not appear inside a `mark` / `note` argument expression,
/// because it happens before the call and therefore before the armed check.
const EAGER_MARKERS: &[&str] = &[
    "format!(",
    ".to_string()",
    ".to_owned()",
    ".join(",
    ".collect::<",
    "String::from",
];

/// Every instrumented file and how many driver calls it contains.
///
/// An exact census, not a floor. Instrumenting a new widget means adding it
/// here, which is the point: the audit's whole value is that it sees EVERY
/// call site, and the only way to keep that checkable is to state the number
/// and let it fail when it moves. (A `>=` bound here previously hid a scanner
/// that found 20 of 21.)
const EXPECTED_CALL_SITES: &[(&str, usize)] = &[
    ("src/states/configure_match_ui.rs", 3),
    ("src/states/encyclopedia/mod.rs", 2),
    ("src/states/encyclopedia/widget.rs", 1),
    ("src/states/main_menu.rs", 1),
    ("src/states/view_combatant_ui.rs", 14),
];

/// Every `mark` / `note` call under `src/`, as `(file, line, full call text)`.
fn driver_call_sites() -> Vec<(String, usize, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("readable src dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }

    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    walk(&manifest.join("src"), &mut files);
    files.sort();

    let mut sites = Vec::new();
    for file in files {
        // The driver's own module DEFINES these; it is not a call site.
        if file.components().any(|c| c.as_os_str() == "driver") {
            continue;
        }
        let text = std::fs::read_to_string(&file).expect("readable source");
        let rel = file
            .strip_prefix(manifest)
            .unwrap_or(&file)
            .display()
            .to_string();
        sites.extend(
            extract_calls(&text)
                .into_iter()
                .map(|(line, call)| (rel.clone(), line, call)),
        );
    }
    sites
}

/// Pull out each driver `mark` / `note` call, balanced to its closing paren.
///
/// Works backwards from the function name rather than forwards from a list of
/// spellings: find `mark(` / `note(`, walk back over the path characters, and
/// accept when the path ends in `driver::`. That covers `ui_driver::note(`,
/// `driver::note(` and `crate::ui::driver::note(` alike — the last of which
/// the opener-list version could not see at all.
fn extract_calls(text: &str) -> Vec<(usize, String)> {
    fn is_path_char(c: char) -> bool {
        c.is_alphanumeric() || c == '_' || c == ':'
    }

    let mut calls = Vec::new();
    for name in ["mark(", "note("] {
        let mut from = 0;
        while let Some(rel) = text[from..].find(name) {
            let start = from + rel;
            from = start + name.len();

            // The qualified path immediately before the function name.
            let prefix_end = start;
            let prefix_start = text[..prefix_end]
                .char_indices()
                .rev()
                .take_while(|(_, c)| is_path_char(*c))
                .last()
                .map(|(i, _)| i)
                .unwrap_or(prefix_end);
            let prefix = &text[prefix_start..prefix_end];

            // `ui_driver::`, `driver::`, `crate::ui::driver::`, … — but not
            // `registry::mark(`, not a bare `fn mark(`, and not `remark(`.
            if !prefix.ends_with("driver::") {
                continue;
            }

            let mut depth = 0usize;
            let mut end = from;
            for (i, c) in text[start..].char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = start + i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let line = text[..prefix_start].matches('\n').count() + 1;
            calls.push((line, text[prefix_start..end].to_string()));
        }
    }
    calls.sort();
    calls
}

/// The scanner sees every spelling of a driver call that the tree contains.
///
/// Separated from the allocation check because it is a different claim, and
/// because the allocation check is worthless without it: the first version of
/// this audit was green precisely because it could not see the call site that
/// mattered. Both spellings are planted, including the fully-qualified one
/// that was invisible.
#[test]
fn the_call_site_scanner_sees_every_spelling() {
    let planted = r#"
        ui_driver::note(ui, format_args!("a {}", names.join("/")));
        crate::ui::driver::note(ui, format_args!("b {}", names.join("/")));
        driver::mark(ui, rect, true, format_args!("c"));
        super::super::ui::driver::mark(ui, rect, true, format_args!("d"));
        // Not driver calls, and must not be collected:
        registry::mark(ctx, rect, true, false, format_args!("e"));
        thing.remark(format_args!("f"));
        fn note(ui: &Ui) {}
        // A DOCUMENTED BLIND SPOT, pinned so it cannot drift: a bare call
        // behind `use crate::ui::driver::note;` is not seen. Nothing in the
        // tree writes this, and the audit's doc comment says so. If you make
        // the scanner handle it, delete this and update that comment — the
        // two must not disagree.
        note(ui, format_args!("g {}", names.join("/")));
    "#;

    let found = extract_calls(planted);
    let texts: Vec<&str> = found.iter().map(|(_, c)| c.as_str()).collect();
    assert_eq!(
        found.len(),
        4,
        "expected the four QUALIFIED driver calls and nothing else, got {texts:#?}"
    );
    assert!(
        !texts.iter().any(|c| c.starts_with("note(")),
        "the bare-import form is a documented blind spot; if it is now seen, \
         say so in the audit's doc comment instead of leaving the two \
         disagreeing: {texts:#?}"
    );
    assert!(
        texts
            .iter()
            .any(|c| c.starts_with("crate::ui::driver::note(")),
        "the FULLY-QUALIFIED spelling is the one the first version missed: {texts:#?}"
    );
    assert!(
        texts
            .iter()
            .any(|c| c.starts_with("super::super::ui::driver::mark(")),
        "a longer qualified path must work too: {texts:#?}"
    );
    for (_, call) in &found {
        assert!(
            EAGER_MARKERS.iter().any(|m| call.contains(m))
                || call.contains("format_args!(\"c\")")
                || call.contains("format_args!(\"d\")"),
            "each planted call should be captured whole: {call}"
        );
    }

    // And the markers flag the known-bad shape in BOTH spellings — the plant
    // that the first version's self-check was missing.
    for spelling in ["ui_driver::note(", "crate::ui::driver::note("] {
        let call = found
            .iter()
            .find(|(_, c)| c.starts_with(spelling))
            .unwrap_or_else(|| panic!("{spelling} not captured"));
        assert!(
            EAGER_MARKERS.iter().any(|m| call.1.contains(m)),
            "{spelling}: markers must flag `.join(` in {}",
            call.1
        );
    }
}

/// No `mark` / `note` call site builds a `String` in its argument position.
///
/// Fails with the file, line and the offending construct, and says what to do
/// instead (`view_combatant_ui::OverrideMap` is the worked example).
#[test]
fn no_driver_call_site_allocates_before_the_armed_check() {
    let sites = driver_call_sites();

    // The census, first. A scan that silently under-collects would make the
    // allocation check below vacuous, which is exactly how the first version
    // of this audit shipped green while missing a call site.
    let mut per_file: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for (file, _, _) in &sites {
        *per_file.entry(file.as_str()).or_default() += 1;
    }
    let expected: std::collections::BTreeMap<&str, usize> =
        EXPECTED_CALL_SITES.iter().copied().collect();
    assert_eq!(
        per_file, expected,
        "the driver call sites found do not match the expected census. If you \
         instrumented a new widget, add it to EXPECTED_CALL_SITES; if this \
         dropped without you removing a call, the scanner has stopped seeing \
         a spelling and the allocation check below is not looking at \
         everything."
    );

    let mut offenders = Vec::new();
    for (file, line, call) in &sites {
        for marker in EAGER_MARKERS {
            if call.contains(marker) {
                offenders.push(format!(
                    "{file}:{line}: `{marker}` in a driver call argument"
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "these driver call sites do work BEFORE the armed check, so they cost \
         something on every frame with the driver off:\n  {}\n\n\
         `format_args!` defers formatting, not the evaluation of its \
         arguments. Wrap the source in a `Display` and let the formatter do \
         the work — see `view_combatant_ui::OverrideMap`.",
        offenders.join("\n  ")
    );
}
// ===========================================================================
// THE FLAG ITSELF
// ===========================================================================

/// `--ui-script` names the mode it clashes with, and only when it clashes.
///
/// The headless arms are dispatched before the graphical one, so a script
/// passed alongside them would be accepted and never run — silence, which is
/// the failure this whole driver exists to stop shipping. The named mode is
/// the one `main`'s dispatch would actually have taken, so the message cannot
/// point at the wrong arm.
#[test]
fn ui_script_conflicts_with_every_windowless_mode() {
    use arenasim::cli::Args;
    use clap::Parser;

    let args = |argv: &[&str]| Args::try_parse_from(argv).expect("valid argv");

    // Each windowless mode, named.
    assert_eq!(
        args(&[
            "arenasim",
            "--headless",
            "m.json",
            "--ui-script",
            "s.script"
        ])
        .ui_script_conflict(),
        Some("--headless")
    );
    assert_eq!(
        args(&["arenasim", "--matrix", "10", "--ui-script", "s.script"]).ui_script_conflict(),
        Some("--matrix")
    );
    assert_eq!(
        args(&["arenasim", "--batch", "b.jsonl", "--ui-script", "s.script"]).ui_script_conflict(),
        Some("--batch")
    );

    // Dispatch order decides which is reported when several are given, so the
    // message always names the arm that would have won.
    assert_eq!(
        args(&[
            "arenasim",
            "--headless",
            "m.json",
            "--matrix",
            "10",
            "--batch",
            "b.jsonl",
            "--ui-script",
            "s.script",
        ])
        .ui_script_conflict(),
        Some("--batch"),
        "main dispatches --batch first, so that is the mode that eats the run"
    );

    // The legitimate combinations stay legitimate.
    assert_eq!(
        args(&["arenasim", "--ui-script", "s.script"]).ui_script_conflict(),
        None
    );
    assert_eq!(
        args(&["arenasim", "--replay", "r.json", "--ui-script", "s.script"]).ui_script_conflict(),
        None,
        "--replay IS graphical; the shipped replay script depends on this"
    );

    // And a windowless run without a script is not a conflict.
    assert_eq!(
        args(&["arenasim", "--headless", "m.json"]).ui_script_conflict(),
        None
    );
}
