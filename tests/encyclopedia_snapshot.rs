//! Offscreen visual snapshots of the in-game encyclopedia.
//!
//! This is the fast visual-iteration loop for `src/states/encyclopedia/`:
//! it renders the real `draw_encyclopedia` to a PNG via `egui_kittest`
//! (wgpu, no window, no Bevy app) in a fraction of a second.
//!
//! ## Loop
//! ```bash
//! # Render the screens; writes tests/snapshots/encyclopedia_*.new.png
//! cargo test --release --test encyclopedia_snapshot -- --ignored
//! # ...then open / read those PNGs, tweak the encyclopedia module, repeat.
//!
//! # Once it looks right, bless the baselines (the tests then pass as
//! # regression guards; a future pixel change writes .new.png + .diff.png):
//! UPDATE_SNAPSHOTS=1 cargo test --release --test encyclopedia_snapshot -- --ignored
//! ```
//!
//! `#[ignore]` keeps these out of the default `cargo test` run because they
//! need a GPU adapter (wgpu), which CI runners may lack. The non-visual
//! behaviour they would otherwise guard (navigation stack, search registry,
//! filters) is covered by plain unit tests inside the module itself.
//!
//! Fidelity caveat, shared with the other egui snapshot loops: kittest has no
//! Bevy textures, so every item and class icon renders as the widget's
//! placeholder tile and fonts are egui defaults. Layout, spacing, colour and
//! copy iterate faithfully here; icon and font fidelity still needs the client.

use egui_kittest::Harness;

use arenasim::states::encyclopedia::{
    draw_encyclopedia, EncyclopediaData, EncyclopediaState, ItemFilters, Section, Topic, View,
};
use arenasim::states::play_match::equipment::{
    load_item_definitions, ArmorType, ItemDefinitions, ItemId,
};

const SIZE: [f32; 2] = [1400.0, 900.0];

/// The Items index: chip-bar filters over the full 136-item grid.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_items_grid() {
    snapshot("encyclopedia_items_grid", state_at_root(), &items());
}

/// The Items index with two filter axes engaged, proving the chip bar and the
/// visible/total count line.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_items_filtered() {
    let mut state = state_at_root();
    let mut filters = ItemFilters::default();
    filters.selected_armor_types.insert(ArmorType::Plate);
    filters.item_level_min = 50;
    state.item_filters = filters;
    snapshot("encyclopedia_items_filtered", state, &items());
}

/// An item detail page: header, stat block, and the class chips that link on.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_item_detail() {
    let mut state = state_at_root();
    // A weapon, so the page exercises the damage/speed/DPS rows too.
    state.apply(arenasim::states::encyclopedia::EncyclopediaAction::Navigate(
        View::topic(Topic::Item(ItemId::EaglestrikeBow)),
    ));
    snapshot("encyclopedia_item_detail", state, &items());
}

/// An active search: grouped, linked result rows over the whole registry.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_search() {
    let mut state = state_at_root();
    state.search = "gu".to_string();
    snapshot("encyclopedia_search", state, &items());
}

/// A section whose content card has not landed: tab, breadcrumb and placeholder.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_pending_section() {
    let mut state = state_at_root();
    state.apply(arenasim::states::encyclopedia::EncyclopediaAction::Navigate(
        View::index(Section::Abilities),
    ));
    snapshot("encyclopedia_pending_section", state, &items());
}

// ============================================================================
// Harness
// ============================================================================

fn items() -> ItemDefinitions {
    load_item_definitions().expect("items.ron must load")
}

/// A fresh state with its registry built — the same thing the Bevy wrapper does
/// on its first frame.
fn state_at_root() -> EncyclopediaState {
    let mut state = EncyclopediaState::default();
    state.rebuild_registry(&items());
    state
}

fn snapshot(name: &'static str, mut state: EncyclopediaState, items: &ItemDefinitions) {
    // `Harness::build` takes a 'static closure, so the definitions are moved in
    // rather than borrowed from the caller's frame.
    let items = items.clone();
    let mut harness = Harness::builder().with_size(SIZE).build(move |ctx| {
        let data = EncyclopediaData {
            items: &items,
            // No egui textures in kittest — icons render as placeholder tiles.
            item_icons: None,
            class_icons: None,
        };
        let _ = draw_encyclopedia(ctx, &mut state, &data);
    });

    harness.run();
    harness.snapshot(name);
}
