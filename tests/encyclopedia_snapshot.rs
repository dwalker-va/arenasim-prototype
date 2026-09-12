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
//! filters, generated text) is covered by plain unit tests inside the module
//! itself.
//!
//! Fidelity caveat, shared with the other egui snapshot loops: kittest has no
//! Bevy textures, so every item, class and ability icon renders as the widget's
//! placeholder tile and fonts are egui defaults. Layout, spacing, colour and
//! copy iterate faithfully here; icon and font fidelity still needs the client.

use egui_kittest::Harness;

use arenasim::states::encyclopedia::{
    draw_encyclopedia, AbilityFilters, AuraId, EncyclopediaData, EncyclopediaState, ItemFilters,
    Section, Topic, View,
};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::ability_config::{load_ability_definitions, AbilityDefinitions};
use arenasim::states::play_match::equipment::{
    load_item_definitions, ArmorType, ItemDefinitions, ItemId, ItemSlotType,
};
use arenasim::states::play_match::AbilityType;

const SIZE: [f32; 2] = [1400.0, 900.0];

/// The Classes index — the landing page: eight tiles, each a link.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_classes_grid() {
    snapshot("encyclopedia_classes_grid", state_at_root());
}

/// A class page: the sim's own base stats, the derived kit, and — because the
/// Warlock is one of the two classes that field one — a labelled pet
/// subsection. Those five pet abilities appeared on no class screen at all
/// before ability attribution existed.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_class_detail() {
    snapshot(
        "encyclopedia_class_detail",
        state_at(Topic::Class(CharacterClass::Warlock)),
    );
}

/// The Abilities index: both filter chip rows over the full 70-ability grid.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_abilities_grid() {
    let mut state = state_at_root();
    state.apply(arenasim::states::encyclopedia::EncyclopediaAction::Navigate(
        View::index(Section::Abilities),
    ));
    snapshot("encyclopedia_abilities_grid", state);
}

/// The same index with both filter axes engaged, proving the chip bar and the
/// shown/total count line.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_abilities_filtered() {
    let mut state = state_at_root();
    state.apply(arenasim::states::encyclopedia::EncyclopediaAction::Navigate(
        View::index(Section::Abilities),
    ));
    state.ability_filters = AbilityFilters {
        class: Some(CharacterClass::Mage),
        school: None,
    };
    snapshot("encyclopedia_abilities_filtered", state);
}

/// An ability detail page. Frostbolt exercises every optional block at once:
/// generated mechanics text, a damage row with spell-power scaling, a
/// projectile speed, and the forward link to the aura it applies.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_ability_detail() {
    snapshot(
        "encyclopedia_ability_detail",
        state_at(Topic::Ability(AbilityType::Frostbolt)),
    );
}

/// A PET ability page: the owning class chip plus the "cast by the …" note.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_pet_ability_detail() {
    snapshot(
        "encyclopedia_pet_ability_detail",
        state_at(Topic::Ability(AbilityType::SpellLock)),
    );
}

/// The Items index: chip-bar filters over the full 136-item grid.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_items_grid() {
    let mut state = state_at_root();
    state.apply(arenasim::states::encyclopedia::EncyclopediaAction::Navigate(
        View::index(Section::Items),
    ));
    snapshot("encyclopedia_items_grid", state);
}

/// The Items index with two filter axes engaged, proving the chip bar and the
/// visible/total count line.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_items_filtered() {
    let mut state = state_at_root();
    state.apply(arenasim::states::encyclopedia::EncyclopediaAction::Navigate(
        View::index(Section::Items),
    ));
    let mut filters = ItemFilters::default();
    filters.selected_armor_types.insert(ArmorType::Plate);
    filters.item_level_min = 50;
    state.item_filters = filters;
    snapshot("encyclopedia_items_filtered", state);
}

/// The Items index filtered to the two PAIRED slot kinds. Rings and trinkets
/// are the only items a character wears two of, so they are the only ones whose
/// labels could regress to a socket name ("Ring 1"); the full grid cuts off
/// above them, so this is the view that actually shows them.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_items_paired_slots() {
    let mut state = state_at_root();
    state.apply(arenasim::states::encyclopedia::EncyclopediaAction::Navigate(
        View::index(Section::Items),
    ));
    let mut filters = ItemFilters::default();
    filters.selected_slots.insert(ItemSlotType::Ring);
    filters.selected_slots.insert(ItemSlotType::Trinket);
    state.item_filters = filters;
    snapshot("encyclopedia_items_paired_slots", state);
}

/// An item detail page: header, stat block, and the class chips that link on.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_item_detail() {
    // A weapon, so the page exercises the damage/speed/DPS rows too.
    snapshot(
        "encyclopedia_item_detail",
        state_at(Topic::Item(ItemId::EaglestrikeBow)),
    );
}

/// An active search: grouped, linked result rows over the whole registry —
/// classes, abilities and items in one list.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_search() {
    let mut state = state_at_root();
    state.search = "sha".to_string();
    snapshot("encyclopedia_search", state);
}

/// The Buffs & Debuffs index: every named aura, split buff vs debuff, each row
/// tagged with the mechanic it shares with its siblings.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_auras_index() {
    let mut state = state_at_root();
    state.apply(arenasim::states::encyclopedia::EncyclopediaAction::Navigate(
        View::index(Section::Auras),
    ));
    snapshot("encyclopedia_auras_index", state);
}

/// A named aura's page. Corruption is the entry that makes the card's point:
/// it is its own debuff, badged with the Damage over Time mechanic and
/// cross-linked to the OTHER damage-over-time effects rather than being
/// collapsed into them. It is also dispellable where its sibling Rend is not,
/// so the removal badge shows the per-aura rule at work.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_aura_detail() {
    let mut state = state_at_root();
    state.apply(arenasim::states::encyclopedia::EncyclopediaAction::Navigate(
        View::topic(Topic::Aura(AuraId::Ability(AbilityType::Corruption))),
    ));
    snapshot("encyclopedia_aura_detail", state);
}

/// A CURSE's page, and the reason the stat block carries a removal line of its
/// own. Curse of Agony is a Shadow damage-over-time effect — the same school and
/// the same mechanic as Corruption above — and the two pages have to read
/// differently, because a dispel takes Corruption and nothing in the arena takes
/// a curse. School and removal class are separate lines precisely so this page
/// can say "Shadow" and "Curse" at once.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_aura_detail_curse() {
    let mut state = state_at_root();
    state.apply(arenasim::states::encyclopedia::EncyclopediaAction::Navigate(
        View::topic(Topic::Aura(AuraId::Ability(AbilityType::CurseOfAgony))),
    ));
    snapshot("encyclopedia_aura_detail_curse", state);
}

/// An aura page with a diminishing-returns section and an engine origin: the
/// Frost Trap slow is re-applied by a zone, so it has no fixed duration and no
/// `applies_aura` block behind it.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_aura_detail_engine() {
    let mut state = state_at_root();
    state.apply(arenasim::states::encyclopedia::EncyclopediaAction::Navigate(
        View::topic(Topic::Aura(AuraId::Engine(
            arenasim::states::encyclopedia::EngineAura::FrostTrapSlow,
        ))),
    ));
    snapshot("encyclopedia_aura_detail_engine", state);
}

/// A DISAMBIGUATED aura page. The Rogue's weapon coating and the slow that
/// coating applies are both called "Crippling Poison" on the actor frames, and
/// a catalog keyed on name alone gave the player only the debuff's page — wrong
/// polarity, wrong mechanic, wrong duration, wrong removal rule. This is the
/// page they should reach from their own gold-bordered buff: a Buff badge, the
/// Weapon Poison mechanic, no expiry, and a provenance line naming what the
/// frames call it.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn encyclopedia_aura_detail_name_collision() {
    let mut state = state_at_root();
    state.apply(arenasim::states::encyclopedia::EncyclopediaAction::Navigate(
        View::topic(Topic::Aura(AuraId::Engine(
            arenasim::states::encyclopedia::EngineAura::WeaponPoisonCoating(
                arenasim::states::match_config::RoguePoison::Crippling,
            ),
        ))),
    ));
    snapshot("encyclopedia_aura_detail_name_collision", state);
}

// ============================================================================
// Harness
// ============================================================================

fn items() -> ItemDefinitions {
    load_item_definitions().expect("items.ron must load")
}

fn abilities() -> AbilityDefinitions {
    load_ability_definitions().expect("abilities.ron must load")
}

/// A fresh state with its registry built — the same thing the Bevy wrapper does
/// on its first frame.
fn state_at_root() -> EncyclopediaState {
    let mut state = EncyclopediaState::default();
    state.rebuild_registry(&items(), &abilities());
    state
}

/// A fresh state navigated to one topic's detail page.
fn state_at(topic: Topic) -> EncyclopediaState {
    let mut state = state_at_root();
    state.apply(arenasim::states::encyclopedia::EncyclopediaAction::Navigate(View::topic(topic)));
    state
}

fn snapshot(name: &'static str, mut state: EncyclopediaState) {
    // `Harness::build` takes a 'static closure, so the definitions are moved in
    // rather than borrowed from the caller's frame.
    let items = items();
    let abilities = abilities();
    let mut harness = Harness::builder().with_size(SIZE).build(move |ctx| {
        let data = EncyclopediaData {
            items: &items,
            abilities: &abilities,
            // No egui textures in kittest — icons render as placeholder tiles.
            item_icons: None,
            class_icons: None,
            ability_icons: None,
        };
        let _ = draw_encyclopedia(ctx, &mut state, &data);
    });

    harness.run();
    harness.snapshot(name);
}
