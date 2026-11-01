//! A talent ability is no part of the base kit: every kit list shows it apart.
//!
//! Over a fixture where Death Coil is a talent ability (`talent: true`) that
//! Fel Stamina grants — the idiom of `tests/talents.rs` and
//! `tests/talent_ai_audit.rs`, since no shipped ability is one yet:
//!
//! * View Combatant's base kit (`kit:` rows) omits it, learned or not, and it
//!   is listed under "From talents" (`kit-talent:` rows) exactly when the
//!   slot's build learns the node;
//! * the encyclopedia's class page lists it under FROM TALENTS and never under
//!   ABILITIES.
//!
//! Laid out in a bare `egui::Context` with the `--ui-script` registry armed,
//! so it needs no GPU and runs in the default `cargo test`.

use bevy_egui::egui;

use arenasim::states::configure_match_ui::ClassIcons;
use arenasim::states::encyclopedia::{
    draw_encyclopedia, EncyclopediaAction, EncyclopediaData, EncyclopediaState, Topic, View,
};
use arenasim::states::match_config::{CharacterClass, MatchConfig};
use arenasim::states::play_match::ability_config::{AbilitiesConfig, AbilityDefinitions};
use arenasim::states::play_match::equipment::{load_default_loadouts, load_item_definitions};
use arenasim::states::play_match::talents::{Build, TalentDefinitions, TalentId};
use arenasim::states::play_match::AbilityType;
use arenasim::states::view_combatant_ui::{
    draw_view_combatant, EquipmentPickerState, ViewCombatantData, ViewCombatantState,
};
use arenasim::states::GameState;
use arenasim::ui::driver::registry;
use arenasim::ui::widgets::Nav;

const GRANTED: AbilityType = AbilityType::DeathCoil;

/// The shipped ability table with Death Coil marked `talent: true`.
fn flagged() -> AbilityDefinitions {
    let text = std::fs::read_to_string(arenasim::paths::asset_path("config/abilities.ron"))
        .expect("abilities.ron reads");
    let mut config: AbilitiesConfig = ron::from_str(&text).expect("abilities.ron parses");
    config.abilities.get_mut(&GRANTED).unwrap().talent = true;
    AbilityDefinitions::new(config)
}

/// The shipped trees with Fel Stamina a one-rank node granting Death Coil.
fn granting(abilities: &AbilityDefinitions) -> TalentDefinitions {
    let text = std::fs::read_to_string(arenasim::paths::asset_path("config/talents.ron"))
        .expect("talents.ron reads");
    let rank = "max_rank: 2,\n            budget: Some(11.0),\n            // Felhunter +10%";
    let effect = "per_rank: [PetStatPercent(MaxHealth, 0.10)],";
    assert!(text.contains(rank) && text.contains(effect));
    let text = text
        .replacen(
            rank,
            "max_rank: 1,\n            budget: Some(11.0),\n            // Felhunter +10%",
            1,
        )
        .replacen(effect, "per_rank: [Grant(DeathCoil)],", 1);
    TalentDefinitions::from_ron(&text, abilities).expect("the fixture validates")
}

fn screen() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 2400.0))
}

/// Run `draw` for a few frames with the registry armed; the last frame's.
fn frames(ctx: &egui::Context, mut draw: impl FnMut(&egui::Context)) -> registry::Frame {
    let mut last = registry::Frame::default();
    for _ in 0..3 {
        registry::arm(ctx);
        let input = egui::RawInput {
            screen_rect: Some(screen()),
            ..Default::default()
        };
        let _ = ctx.run(input, &mut draw);
        last = registry::snapshot(ctx).expect("armed");
    }
    last
}

/// The `kit:` and `kit-talent:` rows the sheet draws for a Warlock whose
/// build is `build`.
fn sheet_rows(
    abilities: &AbilityDefinitions,
    talents: &TalentDefinitions,
    build: Build,
) -> Vec<String> {
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");
    let class_icons = ClassIcons::default();
    let mut config = MatchConfig::default();
    config.team1[0] = Some(CharacterClass::Warlock);
    config.team1_builds[0] = build;
    let view = ViewCombatantState {
        class: CharacterClass::Warlock,
        team: 1,
        slot: 0,
    };
    let mut picker = EquipmentPickerState::default();
    let ctx = egui::Context::default();
    arenasim::ui::install(&ctx);
    let frame = frames(&ctx, |ctx| {
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
    frame
        .widgets
        .into_iter()
        .map(|w| w.id)
        .filter(|id| id.starts_with("kit:") || id.starts_with("kit-talent:"))
        .collect()
}

#[test]
fn view_combatant_lists_a_talent_ability_only_once_learned_and_apart() {
    let abilities = flagged();
    let talents = granting(&abilities);
    let base_row = format!("kit:{GRANTED:?}");
    let talent_row = format!("kit-talent:{GRANTED:?}");

    let ungranted = sheet_rows(&abilities, &talents, Build::default());
    assert!(
        ungranted.iter().any(|id| id == "kit:Corruption"),
        "the base kit is drawn: {ungranted:?}"
    );
    assert!(
        !ungranted.contains(&base_row) && !ungranted.contains(&talent_row),
        "an ungranted talent ability is no kit row: {ungranted:?}"
    );

    let granted = sheet_rows(
        &abilities,
        &talents,
        Build::of(&[(TalentId::FelStamina, 1)]),
    );
    assert!(
        granted.contains(&talent_row),
        "a granted talent ability is listed from talents: {granted:?}"
    );
    assert!(
        !granted.contains(&base_row),
        "and never in the base kit: {granted:?}"
    );
}

#[test]
fn the_class_page_lists_a_talent_ability_from_talents_not_in_the_kit() {
    let abilities = flagged();
    let talents = granting(&abilities);
    let items = load_item_definitions().expect("items.ron must load");
    let mut state = EncyclopediaState::default();
    state.rebuild_registry(&items, &abilities, &talents);
    state.apply(EncyclopediaAction::Navigate(View::topic(Topic::Class(
        CharacterClass::Warlock,
    ))));
    let ctx = egui::Context::default();
    arenasim::ui::install(&ctx);
    let frame = frames(&ctx, |ctx| {
        let data = EncyclopediaData {
            items: &items,
            abilities: &abilities,
            item_icons: None,
            class_icons: None,
            ability_icons: None,
            talents: &talents,
        };
        let _ = draw_encyclopedia(ctx, &mut state, &data, GameState::MainMenu);
    });
    let listed = |list: &str| -> Vec<String> {
        let prefix = format!("class-kit {list} ");
        frame
            .notes
            .iter()
            .filter_map(|n| n.strip_prefix(prefix.as_str()).map(str::to_string))
            .collect()
    };
    let (base, from_talents) = (listed("base"), listed("talent"));
    assert!(base.iter().any(|a| a == "Corruption"), "{base:?}");
    assert_eq!(from_talents, vec![format!("{GRANTED:?}")]);
    assert!(!base.contains(&format!("{GRANTED:?}")), "{base:?}");
}
