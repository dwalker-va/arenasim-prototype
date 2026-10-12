//! Offscreen visual snapshots of the View Combatant screen.
//!
//! This is the fast visual-iteration loop for `src/states/view_combatant_ui.rs`:
//! it renders the real `draw_view_combatant` to a PNG via `egui_kittest`
//! (wgpu, no window, no Bevy app) in a fraction of a second, over the real
//! `abilities.ron`, `items.ron` and `loadouts.ron`, so a kit or loadout change
//! shows up here.
//!
//! ## Loop
//! ```bash
//! # Render the screens; writes tests/snapshots/view_combatant_*.new.png
//! cargo test --release --test view_combatant_snapshot -- --ignored
//! # ...then open / read those PNGs, tweak view_combatant_ui.rs, repeat.
//!
//! # Once it looks right, bless the baselines:
//! UPDATE_SNAPSHOTS=1 cargo test --release --test view_combatant_snapshot -- --ignored
//! ```
//!
//! `#[ignore]` keeps these out of the default `cargo test` run because they
//! need a GPU adapter (wgpu), which CI runners may lack.
//!
//! Fidelity caveat, shared with the other egui snapshot loops: kittest has no
//! Bevy textures, so the class icon is absent and every ability, item and pet
//! icon renders as its placeholder tile (or, for equipment rows, no icon at
//! all). Fonts are the client's own stack (`arenasim::ui::install`), so layout,
//! spacing, colour, type and copy iterate faithfully; icon art still needs the
//! client.

use egui_kittest::Harness;

use arenasim::states::configure_match_ui::ClassIcons;
use arenasim::states::match_config::{CharacterClass, MatchConfig};
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::equipment::{load_default_loadouts, load_item_definitions};
use arenasim::states::view_combatant_ui::{
    draw_view_combatant, EquipmentPickerState, ViewCombatantData, ViewCombatantState,
};

/// Tall enough that the whole sheet — header, stats and kit, the class's
/// strategic panel and every equipment row — fits without scrolling.
const SIZE: [f32; 2] = [1280.0, 1720.0];

/// A 2v2: Warrior + Priest against Mage + Warlock, so both views below have a
/// filled enemy team (the Warlock's curse panel draws one row per enemy).
fn mock_config() -> MatchConfig {
    let mut config = MatchConfig::default();
    config.set_team1_size(2);
    config.set_team2_size(2);
    config.team1[0] = Some(CharacterClass::Warrior);
    config.team1[1] = Some(CharacterClass::Priest);
    config.team2[0] = Some(CharacterClass::Mage);
    config.team2[1] = Some(CharacterClass::Warlock);
    config
}

/// The Warrior: the generic strategic-option panel (Battle Shout) and a
/// two-hander's equipment set.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn view_combatant_warrior() {
    snapshot(
        "view_combatant_warrior",
        ViewCombatantState {
            class: CharacterClass::Warrior,
            team: 1,
            slot: 0,
        },
    );
}

/// The Warlock: the curse-preferences panel, one row per enemy, and the
/// Felhunter's pet subsection in the kit.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn view_combatant_warlock() {
    snapshot(
        "view_combatant_warlock",
        ViewCombatantState {
            class: CharacterClass::Warlock,
            team: 2,
            slot: 1,
        },
    );
}

fn snapshot(name: &str, view: ViewCombatantState) {
    let abilities = load_ability_definitions().expect("abilities.ron must load");
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");
    let class_icons = ClassIcons::default();
    let mut config = mock_config();
    let mut picker = EquipmentPickerState::default();

    let mut harness = Harness::builder().with_size(SIZE).build(move |ctx| {
        arenasim::ui::install(ctx);
        let data = ViewCombatantData {
            abilities: &abilities,
            items: &items,
            default_loadouts: &defaults,
            class_icons: &class_icons,
            ability_icons: None,
            item_icons: None,
            pet_icons: None,
        };
        let _ = draw_view_combatant(ctx, Some(&view), &mut config, &mut picker, &data);
    });

    harness.run();
    harness.snapshot(name);
}
