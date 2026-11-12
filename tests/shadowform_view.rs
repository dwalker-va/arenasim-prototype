//! AS-413 — View Combatant's Form row, drawn the way the client draws it.
//!
//! The Priest's Form row appears only for a build the Shadowform talent
//! offers it to, and a slot holding Shadowform without that talent (it was
//! unlearned in the talent panel) is put back to no form as the sheet draws,
//! so the gates never see a choice its build cannot make.
//!
//! Laid out in a bare `egui::Context` with the `--ui-script` registry armed
//! (`tests/talent_kit_split.rs`'s harness), so it needs no GPU and runs in the
//! default `cargo test`.

use bevy_egui::egui;

use arenasim::states::configure_match_ui::ClassIcons;
use arenasim::states::match_config::{CharacterClass, MatchConfig, PriestForm};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::equipment::{load_default_loadouts, load_item_definitions};
use arenasim::states::play_match::talents::{Build, TalentDefinitions, TalentId};
use arenasim::states::view_combatant_ui::{
    draw_view_combatant, EquipmentPickerState, ViewCombatantData, ViewCombatantState,
};
use arenasim::states::GameState;
use arenasim::ui::driver::registry;
use arenasim::ui::widgets::Nav;

fn shadow_build() -> Build {
    Build::of(&[(TalentId::ImprovedMindBlast, 2), (TalentId::Shadowform, 1)])
}

/// Draw the sheet of team 1's slot 0, a Priest with `build` holding `form`,
/// for a few frames. The slot's form afterwards, and the ids of the widgets
/// the last frame drew.
fn draw(build: Build, form: PriestForm) -> (PriestForm, Vec<String>) {
    let abilities = AbilityDefinitions::shipped();
    let talents = TalentDefinitions::shipped();
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");
    let class_icons = ClassIcons::default();
    let mut config = MatchConfig::default();
    config.team1[0] = Some(CharacterClass::Priest);
    config.team1_builds[0] = build;
    config.team1_priest_forms[0] = form;
    let view = ViewCombatantState {
        class: CharacterClass::Priest,
        team: 1,
        slot: 0,
    };
    let mut picker = EquipmentPickerState::default();
    let ctx = egui::Context::default();
    arenasim::ui::install(&ctx);
    let mut last = registry::Frame::default();
    for _ in 0..3 {
        registry::arm(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 2400.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            let data = ViewCombatantData {
                abilities,
                items: &items,
                default_loadouts: &defaults,
                class_icons: &class_icons,
                ability_icons: None,
                item_icons: None,
                pet_icons: None,
                talents,
            };
            let _ = draw_view_combatant(
                ctx,
                Some(&view),
                &mut config,
                &mut picker,
                &data,
                Nav::Return(GameState::ConfigureMatch),
            );
        });
        last = registry::snapshot(&ctx).expect("armed");
    }
    let ids = last.widgets.into_iter().map(|w| w.id).collect();
    (config.team1_priest_forms[0], ids)
}

/// Shadowform held without the talent goes back to no form as the sheet
/// draws, and no Form row is drawn. Control: with the talent the slot keeps
/// Shadowform, and the row offers it.
#[test]
fn the_sheet_drops_a_form_its_build_no_longer_offers() {
    let (form, ids) = draw(Build::default(), PriestForm::Shadowform);
    assert_eq!(form, PriestForm::NoForm);
    assert!(
        !ids.iter().any(|id| id == "strategic:Shadowform"),
        "no Form row without the talent: {ids:?}"
    );

    let (form, ids) = draw(shadow_build(), PriestForm::Shadowform);
    assert_eq!(form, PriestForm::Shadowform);
    assert!(
        ids.iter().any(|id| id == "strategic:Shadowform"),
        "the Form row offers Shadowform: {ids:?}"
    );
}
