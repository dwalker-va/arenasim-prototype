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
//! `fixture_still_covers_every_row_state` below runs in the default `cargo
//! test`, as does `entry_needs_dummy_pins_the_snapshot_fixture_rows` in
//! `playback.rs`. Those two exist because a fixture that quietly stops
//! covering a state — or restates a value that has since changed — moves no
//! pixels, so the snapshot keeps passing on a lie.
//!
//! Fidelity caveats vs the real client: kittest has no Bevy textures, so every
//! `EntryRow::icon` is `None` here and rows draw their framed empty slot rather
//! than a real spell or class icon. Fonts are egui defaults (Rajdhani is
//! installed by the app's Startup system). So this guards layout, spacing,
//! grouping, and color — not icon or font fidelity.

use arenasim::states::animation_sandbox::playback::{
    entries_for_class, BodyAnimation, EntryFamily, SandboxEntry,
};
use arenasim::states::animation_sandbox::ui::{
    ability_details, draw_sandbox_ui, CameraPreset, EntryRow, SandboxView,
};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use egui_kittest::Harness;

/// The real classifier's label and family for one ability, looked up through
/// the same `entries_for_class` call the live panel builds its rows from.
///
/// Searching EVERY class (not just the Mage) because the fixture deliberately
/// carries an `Unsupported` row and the Mage has none — the only two
/// `Unsupported` abilities in the game are Wind Shear and Heroic Strike.
fn classified(defs: &AbilityDefinitions, ability: AbilityType) -> (String, EntryFamily) {
    CharacterClass::all()
        .iter()
        .flat_map(|class| entries_for_class(*class, defs))
        .find(|listing| listing.entry == SandboxEntry::Ability(ability))
        .map(|listing| (listing.label, listing.family))
        .unwrap_or_else(|| panic!("{ability:?} is not a sandbox entry under any class"))
}

/// The mock entry list: a Mage's own rows plus one borrowed row, chosen so the
/// two snapshots between them cover every way the panel can draw an ability —
/// enabled, `needs dummy`, and `n/a`.
///
/// Family and label are DERIVED, never restated: `classified` asks
/// `entries_for_class` (hence `mechanism_for`) exactly as the live panel does,
/// so the mock cannot drift from the classifier the way it did when Frost Nova
/// sat here hand-labelled `Cast` while the real answer had become `Residue`.
///
/// `needs_dummy` is the one field still set by hand, because
/// `playback::entry_needs_dummy` is `pub(crate)` and out of reach of an
/// integration test. The values below are what that predicate returns today:
/// direct damage (Frostbolt, Frost Nova) or a hostile aura (Polymorph) targets
/// the dummy; a self buff does not. That is no longer an untested claim — the
/// same six pairs are pinned against the real predicate by
/// `entry_needs_dummy_pins_the_snapshot_fixture_rows` in `playback.rs`, which
/// runs in the default `cargo test`, so a predicate change fails there rather
/// than silently surviving here (a stale value moves no pixels).
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
    let ability = |ability, needs_dummy| {
        let (label, family) = classified(&defs, ability);
        EntryRow {
            entry: SandboxEntry::Ability(ability),
            family,
            label,
            icon: None,
            needs_dummy,
        }
    };
    let body = |b: BodyAnimation| EntryRow {
        entry: SandboxEntry::Body(b),
        family: EntryFamily::Body,
        label: b.label().to_string(),
        icon: None,
        needs_dummy: false,
    };

    vec![
        // Offensive — greyed with `needs dummy` whenever the dummy is off.
        ability(AbilityType::Frostbolt, true),
        ability(AbilityType::Polymorph, true),
        ability(AbilityType::FrostNova, true),
        // Self buffs — enabled in both states.
        ability(AbilityType::FrostArmor, false),
        ability(AbilityType::ArcaneIntellect, false),
        // Borrowed from the Warrior: the only coverage of the `n/a` tag and its
        // "no application code / no distinct cast visual" hover.
        ability(AbilityType::HeroicStrike, false),
        body(BodyAnimation::WalkBob),
        body(BodyAnimation::AutoAttack),
        body(BodyAnimation::DeathSink),
        body(BodyAnimation::VictoryBounce),
    ]
}

/// The fixture is only worth its render time while it still reaches the row
/// states it was built to cover. Losing one is otherwise SILENT-ish: the tag
/// changes, one snapshot fails, someone re-blesses, and the state is uncovered
/// with nothing left to say so. These run in the default `cargo test`.
#[test]
fn fixture_still_covers_every_row_state() {
    let rows = mock_rows();
    assert!(
        rows.iter().any(|r| r.family == EntryFamily::Unsupported),
        "no `n/a` row left — the fixture borrowed Heroic Strike solely to \
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

fn view(selected: Option<SandboxEntry>, paused: bool, dummy_enabled: bool) -> SandboxView {
    // DERIVED, not restated: the same call the Bevy wrapper makes, so the
    // SELECTED readout is whatever `abilities.ron` says today. Hand-written
    // numbers here are what put a 30yd / 24-mana Frostbolt — a panel the
    // shipped UI cannot produce — into the blessed baseline (AS-50).
    let (selected_label, selected_details) = selected
        .map(|entry| ability_details(entry, &AbilityDefinitions::default()))
        .unwrap_or((None, Vec::new()));
    SandboxView {
        caster_class: CharacterClass::Mage,
        class_icons: CharacterClass::all().iter().map(|c| (*c, None)).collect(),
        dummy_enabled,
        dummy_class: CharacterClass::Warrior,
        rows: mock_rows(),
        selected,
        selected_label,
        selected_details,
        applied_preset: Some(CameraPreset::ThreeQuarter),
        looping: true,
        paused,
        speed: if paused { 0.0 } else { 0.25 },
        // Nothing selected means no pass, so the track and readout must be
        // empty — mocking a live duration here would make the snapshot show a
        // state the real screen cannot reach.
        elapsed: if selected.is_some() { 0.42 } else { 0.0 },
        duration: if selected.is_some() { 1.50 } else { 0.0 },
        loop_tail: 0.6,
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
