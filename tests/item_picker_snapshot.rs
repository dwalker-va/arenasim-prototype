//! Offscreen visual snapshot of View Combatant's equipment picker.
//!
//! Renders the real `draw_item_picker` — the kit modal View Combatant opens
//! as "Select: <socket>" — for a Warrior's Trinket 2 socket, over the real
//! `items.ron`, so a new item or a changed proc sentence shows up here.
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
//! has no Bevy textures, so every item icon draws the kit's fallback tile —
//! layout, wrapping and colour are faithful; icon art is not.

use std::collections::BTreeMap;

use egui_kittest::Harness;

use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::equipment::{load_item_definitions, ItemId, ItemSlot};
use arenasim::states::view_combatant_ui::draw_item_picker;

#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn item_picker_trinket_socket() {
    let items = load_item_definitions().expect("items.ron must load");
    let mut harness = Harness::builder().with_size([640.0, 900.0]).build(|ctx| {
        arenasim::ui::install(ctx);
        let valid = items.selectable_items_for_slot(
            ItemSlot::Trinket2,
            CharacterClass::Warrior,
            &BTreeMap::new(),
        );
        // The real picker, modal and all, as View Combatant opens it.
        let _ = draw_item_picker(
            ctx,
            "item-picker",
            ItemSlot::Trinket2,
            &valid,
            Some(&ItemId::MarkOfTheChampion),
            None,
        );
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
        let text = item_picker_row_text(item);
        assert!(!text.name.contains('\n'), "{}: {:?}", item.name, text.name);
        match &item.proc {
            Some(proc) => {
                proc_rows += 1;
                assert_eq!(
                    text.proc_line.as_deref(),
                    Some(proc_description(proc).as_str()),
                    "{}",
                    item.name
                );
            }
            None => assert_eq!(text.proc_line, None, "{} grew a second line", item.name),
        }
    }
    assert!(proc_rows > 0, "no proc trinket ships — vacuous");
}
