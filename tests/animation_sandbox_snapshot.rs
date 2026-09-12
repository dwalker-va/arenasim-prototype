//! Offscreen visual snapshot of the Animation Sandbox panel.
//!
//! This is the fast visual-iteration loop for `src/states/animation_sandbox/ui.rs`:
//! it renders the real `draw_sandbox_ui` to a PNG via `egui_kittest` (wgpu, no
//! window) in a fraction of a second, instead of launching the client and
//! navigating into the state.
//!
//! ## Loop
//! ```bash
//! # Render the screen; writes tests/snapshots/animation_sandbox*.new.png
//! cargo test --release --test animation_sandbox_snapshot -- --ignored
//! # ...open / read those PNGs, tweak ui.rs, repeat.
//!
//! # Once it looks right, bless the baselines:
//! UPDATE_SNAPSHOTS=1 cargo test --release --test animation_sandbox_snapshot -- --ignored
//! ```
//!
//! `#[ignore]` keeps the two RENDERING tests out of the default `cargo test`
//! run because they need a GPU adapter (wgpu), which CI runners may lack. That
//! also means nothing catches a change to `draw_sandbox_ui` until someone runs
//! them by hand — so a commit that touches the panel must re-bless in the same
//! commit (see CLAUDE.md's snapshot-loop section).
//!
//! What the mock itself claims is guarded without a GPU:
//! `fixture_still_covers_every_row_state`, `fixture_rows_are_the_panels_own`
//! and `fixture_transport_state_is_reachable` below run in the default `cargo
//! test`, as does `entry_needs_dummy_classifies_the_sandbox_rows` in
//! `playback.rs`. Those exist because a fixture that quietly stops covering a
//! state — or draws a panel the app cannot serve — moves no pixels, so the
//! snapshot keeps passing on a lie.
//!
//! ## What is hand-set here
//!
//! Exactly one fact: `BORROWED_ROW` — WHICH ability is borrowed from a class
//! the fixture does not stage, so the panel has an `Unsupported` row to draw
//! (the Mage has none). Its label, family and `needs_dummy` are derived like
//! every other row's; only the decision to include it is a choice.
//!
//! Everything else — the row list, every row's label, family and
//! `needs_dummy`, the SELECTED readout, the transport's duration and speed —
//! comes from the code that produces it, through the same calls
//! `ui::sandbox_ui` makes. The rest of the view is scene-setting the user
//! picks at runtime (which class is staged, dummy on or off, paused or
//! playing) plus one free choice with no producing code (`PLAYHEAD_FRACTION`).
//!
//! Anything hand-written beyond that is a defect waiting to be blessed into a
//! baseline — which is exactly how the panel came to claim a 1.50s Frostbolt
//! pass (AS-62), a 30yd / 24-mana Frostbolt (AS-50), a `Cast` Frost Nova
//! (AS-43), and a Mage who knew only five of her eight spells (AS-72).
//!
//! Fidelity caveats vs the real client: kittest has no Bevy textures, so every
//! `EntryRow::icon` is `None` here and rows draw their framed empty slot rather
//! than a real spell or class icon. Fonts are egui defaults (Rajdhani is
//! installed by the app's Startup system). So this guards layout, spacing,
//! grouping, and color — not icon or font fidelity.

use arenasim::states::animation_sandbox::playback::{
    entry_duration, EntryFamily, SandboxEntry, SandboxPlayback, LOOP_TAIL_SECS,
};
use arenasim::states::animation_sandbox::ui::{
    ability_details, draw_sandbox_ui, entry_rows, CameraPreset, EntryRow, SandboxView, SPEEDS,
};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use egui_kittest::Harness;

/// The class the fixture stages. Its whole kit is drawn, exactly as the panel
/// would serve it — the fixture picks WHICH class, not which of her spells.
const FIXTURE_CLASS: CharacterClass = CharacterClass::Mage;

/// The one ability borrowed from a class the fixture does not stage — and the
/// only fact in this file chosen by hand rather than derived.
///
/// It is here to cover the `n/a` tag and its "no application code / no distinct
/// cast visual" hover, which `FIXTURE_CLASS` cannot: the only two `Unsupported`
/// abilities in the game are Wind Shear and Heroic Strike, and the Mage owns
/// neither. `fixture_rows_are_the_panels_own` holds it to being exactly one
/// extra row; `fixture_still_covers_every_row_state` holds it to still being
/// `Unsupported`.
const BORROWED_ROW: AbilityType = AbilityType::HeroicStrike;

/// One row built exactly as the panel builds it, searching EVERY class.
///
/// `entry_rows` is per-class and the borrowed row belongs to a class the
/// fixture does not stage, so the lookup spans all of them.
fn panel_row(defs: &AbilityDefinitions, entry: SandboxEntry) -> EntryRow {
    CharacterClass::all()
        .iter()
        .flat_map(|class| entry_rows(*class, defs, |_| None))
        .find(|row| row.entry == entry)
        .unwrap_or_else(|| panic!("{entry:?} is not a sandbox entry under any class"))
}

/// The fixture's entry list: the staged class's ENTIRE list as `ui::entry_rows`
/// builds it — every ability `abilities.ron` attributes to her, in the panel's
/// own order, followed by the body rows — plus the single borrowed row.
///
/// Nothing in a row is written here. Label, family and `needs_dummy` all come
/// back from the same `entry_rows` call `sandbox_ui` makes, so the fixture is
/// not a corrected copy of the panel: it IS the panel, one class's worth, with
/// `icon_for` returning `None` because kittest has no Bevy textures. The
/// hand-assembled list this replaced had drifted three ways over three passes —
/// a `Cast` Frost Nova (AS-43), and then, quietly, a Mage missing Ice Barrier,
/// Mage Armor and Molten Armor while the module doc said "a Mage's own rows"
/// (AS-72).
///
/// One row shape has NO representation here and cannot have one: the `soon`
/// tag needs a family that is non-playable and not `Unsupported`, and
/// `EntryFamily::is_playable` is true for every family except `Unsupported`.
/// That branch is unreachable in the shipped panel too — it is the residue of
/// the staged mechanism rollout, held for the next unwired mechanism. Faking it
/// here would pin a state the real UI cannot produce, which is the exact defect
/// this fixture was rebuilt to remove.
fn mock_rows() -> Vec<EntryRow> {
    let defs = AbilityDefinitions::default();
    let mut rows = entry_rows(FIXTURE_CLASS, &defs, |_| None);

    // Ahead of the BODY rows, so the borrowed row lands in the ABILITIES
    // section in the reading order the panel would give a class ability.
    let first_body = rows
        .iter()
        .position(|row| row.family == EntryFamily::Body)
        .unwrap_or(rows.len());
    rows.insert(
        first_body,
        panel_row(&defs, SandboxEntry::Ability(BORROWED_ROW)),
    );
    rows
}

/// The borrowed row is the fixture's one departure from a panel the app could
/// serve, so it is worth proving it is still exactly that — one extra row, and
/// otherwise the staged class's own list, entire and in order.
///
/// This is what makes the row list unable to drift back into a silent subset.
/// The list it replaced showed five of the Mage's eight abilities under a doc
/// that read as all of them, for three passes, because nothing compared it to
/// what the class actually has.
#[test]
fn fixture_rows_are_the_panels_own() {
    let defs = AbilityDefinitions::default();
    let panel: Vec<SandboxEntry> = entry_rows(FIXTURE_CLASS, &defs, |_| None)
        .iter()
        .map(|row| row.entry)
        .collect();
    let fixture: Vec<SandboxEntry> = mock_rows().iter().map(|row| row.entry).collect();
    let borrowed = SandboxEntry::Ability(BORROWED_ROW);

    assert!(
        !panel.contains(&borrowed),
        "{BORROWED_ROW:?} is now a {FIXTURE_CLASS:?} ability, so it is no longer \
         borrowed — drop it from `mock_rows` and let the class list carry it"
    );
    assert_eq!(
        fixture.iter().filter(|e| **e == borrowed).count(),
        1,
        "the borrowed row must appear exactly once"
    );
    assert_eq!(
        fixture
            .iter()
            .copied()
            .filter(|e| *e != borrowed)
            .collect::<Vec<_>>(),
        panel,
        "the fixture's rows are no longer {FIXTURE_CLASS:?}'s own list plus the \
         one borrowed row — either a row was dropped (the panel would then draw \
         a class that cannot cast her own spells) or a second row was added \
         without being documented as a departure"
    );
}

/// The fixture is only worth its render time while it still reaches the row
/// states it was built to cover. Losing one is otherwise SILENT-ish: the tag
/// changes, one snapshot fails, someone re-blesses, and the state is uncovered
/// with nothing left to say so. These run in the default `cargo test`.
///
/// Now that the rows are DERIVED, this reads as an assertion about the real
/// code rather than about a copy of it: the `needs dummy` check, for instance,
/// fails the moment `entry_needs_dummy` stops marking any of the staged class's
/// abilities — without a GPU, and without waiting for whoever next renders the
/// `#[ignore]`d snapshots. That is the coverage the fixture's old hand-set
/// booleans bought by restating the predicate, bought instead by asking it.
#[test]
fn fixture_still_covers_every_row_state() {
    let rows = mock_rows();
    assert!(
        rows.iter().any(|r| r.family == EntryFamily::Unsupported),
        "no `n/a` row left — the fixture borrows {BORROWED_ROW:?} solely to \
         cover the Unsupported tag and its hover; if it gained a visual, \
         borrow the other Unsupported ability (Wind Shear) instead"
    );
    assert!(
        rows.iter().any(|r| r.needs_dummy),
        "no `needs dummy` row left — the paused/no-dummy snapshot exists to \
         show AE3's greying and would now show nothing"
    );
    assert!(
        rows.iter()
            .any(|r| !r.needs_dummy && r.family.is_playable() && r.family != EntryFamily::Body),
        "no self-buff row left — the fixture needs one ability that stays \
         enabled with the dummy off"
    );
    assert!(
        rows.iter().any(|r| r.family == EntryFamily::Body),
        "no Body row left — the BODY section would render empty"
    );
}

/// Where the playhead sits, as a fraction of the pass it is inside.
///
/// The one transport quantity with no producing code to derive from — any
/// position in `0..=duration + LOOP_TAIL_SECS` is reachable, so the fixture
/// picks one. Expressed as a FRACTION rather than an absolute second count so
/// it stays inside the pass whatever `entry_duration` returns: an absolute
/// 0.42s was a 6% sliver of Frostbolt's real 6.76s window, and against a short
/// entry it could sit past the end of the track entirely.
const PLAYHEAD_FRACTION: f32 = 0.35;

/// The transport's speed rung, taken FROM the offered set rather than named.
///
/// `SandboxAction::SetSpeed` only ever carries a `SPEEDS` member, so a speed
/// outside the array highlights no chip at all — a transport the running panel
/// cannot produce. The slowest rung is the one the control exists for (see
/// `SPEEDS`' own doc comment), and `min` finds it without assuming the array
/// stays sorted.
fn watching_speed() -> f32 {
    SPEEDS.iter().copied().fold(f32::INFINITY, f32::min)
}

fn view(selected: Option<SandboxEntry>, paused: bool, dummy_enabled: bool) -> SandboxView {
    let defs = AbilityDefinitions::default();
    // DERIVED, not restated: the same call the Bevy wrapper makes, so the
    // SELECTED readout is whatever `abilities.ron` says today. Hand-written
    // numbers here are what put a 30yd / 24-mana Frostbolt — a panel the
    // shipped UI cannot produce — into the blessed baseline (AS-50).
    let (selected_label, selected_details) = selected
        .map(|entry| ability_details(entry, &defs))
        .unwrap_or((None, Vec::new()));

    // The transport readout, derived the same way — and for the same reason,
    // one panel lower. `duration: 1.50` was not merely at risk of drifting: it
    // was already 5.26s short of the Frostbolt pass the screen plays, so the
    // blessed baseline showed a loop-tail boundary at 71% of a track where the
    // real one sits at 92%. This is the driver's own chain: `select` stamps the
    // entry and its family exactly as `SandboxAction::Select` does, and
    // `entry_duration` is what `drive_playback` assigns to `playback.duration`.
    // The empty case needs no special-casing either — an unselected `playback`
    // reaches that function's own `None => 0.0` arm.
    let mut playback = SandboxPlayback::default();
    if let Some(entry) = selected {
        playback.select(entry, panel_row(&defs, entry).family);
    }
    let duration = entry_duration(&playback, &defs);

    // `paused` is not an independent input in the real panel: the wrapper reads
    // ONE quantity — the virtual clock's relative speed — and derives both
    // fields from it, pausing BEING a zeroed clock. Modelling the same single
    // quantity here keeps the pair from disagreeing in a way the app cannot.
    let relative_speed = if paused { 0.0 } else { watching_speed() };

    SandboxView {
        caster_class: FIXTURE_CLASS,
        class_icons: CharacterClass::all().iter().map(|c| (*c, None)).collect(),
        dummy_enabled,
        dummy_class: CharacterClass::Warrior,
        rows: mock_rows(),
        selected,
        selected_label,
        selected_details,
        applied_preset: Some(CameraPreset::ThreeQuarter),
        looping: true,
        paused: relative_speed == 0.0,
        speed: relative_speed,
        elapsed: duration * PLAYHEAD_FRACTION,
        duration,
        loop_tail: LOOP_TAIL_SECS,
    }
}

/// The transport half of the same claim `fixture_still_covers_every_row_state`
/// makes about the rows: that every state this fixture pins is one the running
/// panel can actually reach. Runs in the default `cargo test`, because a
/// transport that drifts back into an impossible state moves pixels only in a
/// snapshot nobody runs without a GPU.
#[test]
fn fixture_transport_state_is_reachable() {
    let playing = view(
        Some(SandboxEntry::Ability(AbilityType::Frostbolt)),
        false,
        true,
    );
    let idle = view(None, true, false);

    assert!(
        playing.duration > 0.0,
        "a selected entry must have a pass; a zero duration would draw the \
         empty track the no-selection snapshot already covers"
    );
    assert_eq!(
        idle.duration, 0.0,
        "nothing selected means no pass, so the track and readout must be empty"
    );

    for v in [&playing, &idle] {
        assert!(
            v.elapsed <= v.duration + v.loop_tail,
            "playhead ({:.2}s) is past the end of the pass plus its loop tail \
             ({:.2}s) — a position the transport never reaches, since \
             `drive_playback` restarts or stops there",
            v.elapsed,
            v.duration + v.loop_tail,
        );
        assert!(
            v.speed == 0.0 || SPEEDS.contains(&v.speed),
            "speed {} is not a rung the panel offers, so the chip row would \
             highlight nothing — `SetSpeed` only ever carries a `SPEEDS` member",
            v.speed,
        );
        assert_eq!(
            v.paused,
            v.speed == 0.0,
            "`paused` and `speed` disagree; the wrapper derives BOTH from the \
             virtual clock, so they cannot come apart in the running panel"
        );
    }
}

fn render(name: &str, view: SandboxView) {
    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .build(move |ctx| {
            let _ = draw_sandbox_ui(ctx, &view);
        });
    harness.run();
    harness.snapshot(name);
}

/// The ordinary working state: an entry selected and playing, dummy staged.
/// With the dummy on, only the `n/a` row is greyed.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn animation_sandbox() {
    render(
        "animation_sandbox",
        view(
            Some(SandboxEntry::Ability(AbilityType::Frostbolt)),
            false,
            true,
        ),
    );
}

/// Paused with nothing selected and no dummy — exercises the disabled `Play`,
/// the enabled `Step`, the no-dummy warning that only appears in this state,
/// and AE3's `needs dummy` greying of every offensive row.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn animation_sandbox_paused_no_selection() {
    render(
        "animation_sandbox_paused_no_selection",
        view(None, true, false),
    );
}
