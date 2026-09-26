//! Offscreen visual snapshot of View Combatant's equipment picker.
//!
//! Renders the real `draw_item_picker_list` — the list the "Select: <socket>"
//! window shows — for a Warrior's Trinket 2 socket, over the real `items.ron`,
//! so a new item or a changed proc sentence shows up here.
//!
//! The thing this pins: a PROC trinket's row carries a second, quieter line
//! saying what the proc does (trigger, effect, duration, cooldown — the
//! `proc_description` sentence the tooltip and the encyclopedia share), and a
//! row without a proc stays one line.
//!
//! ## Loop
//! ```bash
//! cargo test --release --test item_picker_snapshot -- --ignored
//! UPDATE_SNAPSHOTS=1 cargo test --release --test item_picker_snapshot -- --ignored
//! ```
//!
//! `#[ignore]`d because it needs a GPU adapter (wgpu). Fidelity caveat: kittest
//! has no Bevy textures, so the item icons are absent and each row starts at
//! its text — layout, wrapping and colour are faithful; icon art is not.

use std::collections::BTreeMap;

use bevy_egui::egui;
use egui_kittest::Harness;

use arenasim::states::encyclopedia::widget::secondary_click_chrome_hint;
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::equipment::{load_item_definitions, ItemId, ItemSlot};
use arenasim::states::view_combatant_ui::draw_item_picker_list;
use arenasim::ui::fonts::install_game_fonts;

#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn item_picker_trinket_socket() {
    let items = load_item_definitions().expect("items.ron must load");
    let mut harness = Harness::builder().with_size([640.0, 560.0]).build(|ctx| {
        install_game_fonts(ctx);
        let valid = items.selectable_items_for_slot(
            ItemSlot::Trinket2,
            CharacterClass::Warrior,
            &BTreeMap::new(),
        );
        // The same window chrome the client opens the list in.
        egui::Window::new("Select: Trinket 2")
            .collapsible(false)
            .resizable(false)
            .min_width(300.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                secondary_click_chrome_hint(ui);
                ui.add_space(4.0);
                let _ = draw_item_picker_list(ui, &valid, Some(&ItemId::MarkOfTheChampion), None);
            });
    });
    harness.run();
    harness.snapshot("item_picker_trinket_socket");
}

/// The claim the snapshot pictures, without a GPU: every proc trinket's row
/// carries exactly `proc_description` as its second line — the tooltip's and
/// the encyclopedia's sentence, not a third copy — and every other item's row
/// is a single line.
#[test]
fn a_proc_row_carries_the_shared_proc_sentence_and_nothing_else_grows() {
    use arenasim::states::play_match::proc_trinkets::proc_description;
    use arenasim::states::view_combatant_ui::item_picker_row_text;

    let items = load_item_definitions().expect("items.ron must load");
    let mut proc_rows = 0;
    for (_, item) in items.iter() {
        let text = item_picker_row_text(item, egui::Color32::WHITE).text;
        let lines: Vec<&str> = text.lines().collect();
        match &item.proc {
            Some(proc) => {
                proc_rows += 1;
                assert_eq!(lines.len(), 2, "{}: {text:?}", item.name);
                assert_eq!(lines[1], proc_description(proc), "{}", item.name);
            }
            None => assert_eq!(lines.len(), 1, "{} grew a second line: {text:?}", item.name),
        }
    }
    assert!(proc_rows > 0, "no proc trinket ships — vacuous");
}
