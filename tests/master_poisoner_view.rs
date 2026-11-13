//! AS-417 — View Combatant's Off-Hand Poison row, drawn the way the client
//! draws it.
//!
//! The row appears only for a build Master Poisoner opens the slot for, and
//! offers every poison but the main hand's. A slot holding an off-hand poison
//! without the talent (it was unlearned in the talent panel) is emptied as the
//! sheet draws, so the gates never see a choice its build cannot make.
//!
//! Laid out in a bare `egui::Context` with the `--ui-script` registry armed
//! (`tests/shadowform_view.rs`'s harness), so it needs no GPU and runs in the
//! default `cargo test`.

use bevy_egui::egui;

use arenasim::states::configure_match_ui::ClassIcons;
use arenasim::states::match_config::{CharacterClass, MatchConfig, RoguePoison};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::equipment::{load_default_loadouts, load_item_definitions};
use arenasim::states::play_match::talents::{Build, TalentDefinitions, TalentId};
use arenasim::states::view_combatant_ui::{
    draw_view_combatant, EquipmentPickerState, ViewCombatantData, ViewCombatantState,
};
use arenasim::states::GameState;
use arenasim::ui::driver::registry;
use arenasim::ui::widgets::Nav;

fn master_poisoner() -> Build {
    Build::of(&[(TalentId::VilePoisons, 2), (TalentId::MasterPoisoner, 1)])
}

/// What the sheet left: the slot's main-hand and off-hand poisons, and the
/// ids of the off-hand tiles the last frame drew.
struct Drawn {
    main: RoguePoison,
    off: Option<RoguePoison>,
    tiles: Vec<String>,
}

/// Draw the sheet of team 1's slot 0, a Rogue with `build` wearing `main`
/// and `off`, for a few frames; then click each widget of `clicks` in turn
/// (pressed one frame, released the next, then settled).
fn draw_clicking(
    build: Build,
    main: RoguePoison,
    off: Option<RoguePoison>,
    clicks: &[&str],
) -> Drawn {
    let abilities = AbilityDefinitions::shipped();
    let talents = TalentDefinitions::shipped();
    let items = load_item_definitions().expect("items.ron must load");
    let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");
    let class_icons = ClassIcons::default();
    let mut config = MatchConfig::default();
    config.team1[0] = Some(CharacterClass::Rogue);
    config.team1_builds[0] = build;
    config.team1_rogue_poisons[0] = main;
    config.team1_rogue_offhand_poisons[0] = off;
    let view = ViewCombatantState {
        class: CharacterClass::Rogue,
        team: 1,
        slot: 0,
    };
    let mut picker = EquipmentPickerState::default();
    let ctx = egui::Context::default();
    arenasim::ui::install(&ctx);
    let mut frame = |config: &mut MatchConfig, events: Vec<egui::Event>| {
        registry::arm(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 2400.0),
            )),
            events,
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
                config,
                &mut picker,
                &data,
                Nav::Return(GameState::ConfigureMatch),
            );
        });
        registry::snapshot(&ctx).expect("armed")
    };
    let mut last = registry::Frame::default();
    for _ in 0..3 {
        last = frame(&mut config, Vec::new());
    }
    for id in clicks {
        let at = last
            .widgets
            .iter()
            .find(|w| w.id == *id)
            .unwrap_or_else(|| panic!("no widget {id} to click"))
            .rect
            .center();
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            &mut config,
            vec![egui::Event::PointerMoved(at), button(true)],
        );
        frame(&mut config, vec![button(false)]);
        for _ in 0..2 {
            last = frame(&mut config, Vec::new());
        }
    }
    let mut tiles: Vec<String> = last
        .widgets
        .into_iter()
        .map(|w| w.id)
        .filter(|id| id.ends_with(":offhand"))
        .collect();
    tiles.sort();
    Drawn {
        main: config.team1_rogue_poisons[0],
        off: config.team1_rogue_offhand_poisons[0],
        tiles,
    }
}

/// [`draw_clicking`] with no clicks: the slot's off-hand poison, and the
/// off-hand tiles drawn.
fn draw(
    build: Build,
    main: RoguePoison,
    off: Option<RoguePoison>,
) -> (Option<RoguePoison>, Vec<String>) {
    let drawn = draw_clicking(build, main, off, &[]);
    (drawn.off, drawn.tiles)
}

fn tiles(poisons: &[&str]) -> Vec<String> {
    let mut ids: Vec<String> = poisons
        .iter()
        .map(|p| format!("strategic:{p}:offhand"))
        .collect();
    ids.sort();
    ids
}

/// With Master Poisoner the row offers every poison but the main hand's,
/// and an empty slot is filled with the first of them. Without it no row is
/// drawn, and an off-hand poison left from an unlearned talent is dropped.
#[test]
fn the_off_hand_row_follows_master_poisoner() {
    let (off, ids) = draw(master_poisoner(), RoguePoison::Wound, None);
    assert_eq!(off, Some(RoguePoison::Crippling));
    assert_eq!(
        ids,
        tiles(&["CripplingPoison", "DeadlyPoison", "MindNumbingPoison"])
    );

    let (off, ids) = draw(
        master_poisoner(),
        RoguePoison::Crippling,
        Some(RoguePoison::Wound),
    );
    assert_eq!(off, Some(RoguePoison::Wound), "a legal choice is kept");
    assert_eq!(
        ids,
        tiles(&["DeadlyPoison", "MindNumbingPoison", "WoundPoison"])
    );

    let (off, ids) = draw(
        Build::of(&[(TalentId::VilePoisons, 2)]),
        RoguePoison::Crippling,
        Some(RoguePoison::Wound),
    );
    assert_eq!(off, None);
    assert!(ids.is_empty(), "no row without the talent: {ids:?}");
}

/// The rows take clicks: an off-hand tile sets the off hand's poison, and
/// choosing the off hand's poison for the main hand swaps the two hands.
#[test]
fn the_poison_rows_take_clicks() {
    let drawn = draw_clicking(
        master_poisoner(),
        RoguePoison::Crippling,
        Some(RoguePoison::Deadly),
        &["strategic:WoundPoison:offhand"],
    );
    assert_eq!(
        (drawn.main, drawn.off),
        (RoguePoison::Crippling, Some(RoguePoison::Wound))
    );
    // Main Wound, off Deadly: Deadly for the main hand puts Wound on the off
    // hand (where the legality repair alone would put Crippling, the first
    // poison offered).
    let drawn = draw_clicking(
        master_poisoner(),
        RoguePoison::Wound,
        Some(RoguePoison::Deadly),
        &["strategic:DeadlyPoison"],
    );
    assert_eq!(
        (drawn.main, drawn.off),
        (RoguePoison::Deadly, Some(RoguePoison::Wound))
    );
    assert_eq!(
        drawn.tiles,
        tiles(&["CripplingPoison", "MindNumbingPoison", "WoundPoison"])
    );
}
