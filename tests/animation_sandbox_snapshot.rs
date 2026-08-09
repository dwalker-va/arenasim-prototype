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
//! `#[ignore]` keeps it out of the default `cargo test` run because it needs a
//! GPU adapter (wgpu), which CI runners may lack.
//!
//! Fidelity caveats vs the real client: kittest has no Bevy textures, so every
//! `EntryRow::icon` is `None` here and rows draw their framed empty slot rather
//! than a real spell or class icon. Fonts are egui defaults (Rajdhani is
//! installed by the app's Startup system). So this guards layout, spacing,
//! grouping, and color — not icon or font fidelity.

use arenasim::states::animation_sandbox::playback::{BodyAnimation, EntryFamily, SandboxEntry};
use arenasim::states::animation_sandbox::ui::{draw_sandbox_ui, EntryRow, SandboxView};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::abilities::AbilityType;
use egui_kittest::Harness;

/// Mock rows resembling a Mage's list: a couple of playable hard casts, several
/// greyed instants, and the body entries.
fn mage_rows() -> Vec<EntryRow> {
    let ability = |ability, label: &str, family| EntryRow {
        entry: SandboxEntry::Ability(ability),
        family,
        label: label.to_string(),
        icon: None,
    };
    let body = |b: BodyAnimation| EntryRow {
        entry: SandboxEntry::Body(b),
        family: EntryFamily::Body,
        label: b.label().to_string(),
        icon: None,
    };

    vec![
        ability(AbilityType::Frostbolt, "Frostbolt", EntryFamily::HardCast),
        ability(AbilityType::Polymorph, "Polymorph", EntryFamily::HardCast),
        ability(AbilityType::FrostNova, "Frost Nova", EntryFamily::Instant),
        ability(AbilityType::FrostArmor, "Frost Armor", EntryFamily::Instant),
        ability(
            AbilityType::ArcaneIntellect,
            "Arcane Intellect",
            EntryFamily::Instant,
        ),
        body(BodyAnimation::WalkBob),
        body(BodyAnimation::AutoAttack),
        body(BodyAnimation::DeathSink),
        body(BodyAnimation::VictoryBounce),
    ]
}

fn view(selected: Option<SandboxEntry>, paused: bool, dummy_enabled: bool) -> SandboxView {
    SandboxView {
        caster_class: CharacterClass::Mage,
        class_icons: CharacterClass::all().iter().map(|c| (*c, None)).collect(),
        dummy_enabled,
        dummy_class: CharacterClass::Warrior,
        rows: mage_rows(),
        selected,
        looping: true,
        paused,
        speed: if paused { 0.0 } else { 0.25 },
        elapsed: 0.42,
        duration: 1.50,
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
/// the enabled `Step`, and the no-dummy warning that only appears in this state.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn animation_sandbox_paused_no_selection() {
    render(
        "animation_sandbox_paused_no_selection",
        view(None, true, false),
    );
}
