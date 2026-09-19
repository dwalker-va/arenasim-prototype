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

use arenasim::ui::driver::registry;
use arenasim::ui::driver::script::{Button, NamedKey, Script, Step};
use arenasim::ui::driver::{UiDriverConfig, UiDriverPlugin};

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
         normal client frame runs, and they must not allocate or record"
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

/// The two scripts under `tests/ui-scripts/` must parse. They are run against
/// the real client by hand (they need a window); this keeps a typo in one from
/// surviving until someone launches it.
#[test]
fn the_shipped_scripts_parse() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui-scripts");
    let mut found = 0;
    for entry in std::fs::read_dir(&dir).expect("tests/ui-scripts exists") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("script") {
            continue;
        }
        let script = Script::load(&path).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            !script.steps.is_empty(),
            "{} parsed to zero steps",
            path.display()
        );
        found += 1;
    }
    assert!(
        found >= 2,
        "expected the two shipped scripts under {}, found {found}",
        dir.display()
    );
}
