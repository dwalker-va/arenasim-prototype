//! The talent panel's rules and its click contract.
//!
//! * The panel offers exactly what the engine allows:
//!   `TalentDefinitions::learn_blocker` answers `None` for a talent exactly
//!   when the build with one more rank of it is legal (`validate_build`), and
//!   `unlearn` gives back a build exactly when the build with one rank fewer
//!   is legal — over every legal Warlock build, every node.
//! * The spec label: the tree with the most points names it, a tie reads as a
//!   hybrid, the shorthand gives every tree's points in display order.
//! * The floor (a Gauntlet run's locked-in ranks): a rank at or below it is
//!   never taken back, and Reset returns to it.
//! * The drawn panel: a click on a node learns a rank, a shift-click takes one
//!   back, a right-click asks for the talent's encyclopedia page, and a click
//!   on a locked node changes nothing. Driven as raw pointer events through a
//!   bare `egui::Context` (no GPU), aimed by the `--ui-script` registry's own
//!   rects.

use bevy_egui::egui;

use arenasim::states::encyclopedia::{EncyclopediaData, Topic};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::equipment::load_item_definitions;
use arenasim::states::play_match::talents::{
    Build, LearnBlocker, Spec, TalentDefinitions, TalentId, TreeId, MAX_TALENT_POINTS,
};
use arenasim::states::talent_panel::{apply_click, spend_panel, tree_columns, NodeClick};
use arenasim::ui::driver::registry;

const WARLOCK: CharacterClass = CharacterClass::Warlock;

fn talents() -> &'static TalentDefinitions {
    TalentDefinitions::shipped()
}

/// Every legal Warlock build, from empty to the cap.
fn every_legal_build() -> Vec<Build> {
    (0..=MAX_TALENT_POINTS)
        .flat_map(|points| talents().legal_builds(WARLOCK, points))
        .collect()
}

fn warlock_talents() -> Vec<TalentId> {
    talents()
        .talents_for_class(WARLOCK)
        .map(|(id, _)| id)
        .collect()
}

#[test]
fn a_rank_is_offered_exactly_when_the_build_with_it_is_legal() {
    let builds = every_legal_build();
    assert!(
        builds.len() > 10,
        "too few builds to mean anything: {}",
        builds.len()
    );
    let mut offered = 0;
    let mut refused = 0;
    for build in &builds {
        for id in warlock_talents() {
            let mut with = build.clone();
            *with.ranks.entry(id).or_insert(0) += 1;
            let legal = talents().validate_build(WARLOCK, &with).is_ok();
            let blocker = talents().learn_blocker(WARLOCK, build, id);
            assert_eq!(
                blocker.is_none(),
                legal,
                "{build:?} + {id:?}: blocker {blocker:?}, build with it legal: {legal}"
            );
            match talents().learn(WARLOCK, build, id) {
                Ok(next) => {
                    assert_eq!(next, with);
                    offered += 1;
                }
                Err(_) => refused += 1,
            }
        }
    }
    assert!(
        offered > 0 && refused > 0,
        "offered {offered}, refused {refused}"
    );
}

#[test]
fn a_rank_is_taken_back_exactly_when_the_build_without_it_is_legal() {
    let mut kept = 0;
    let mut given_back = 0;
    for build in every_legal_build() {
        for id in warlock_talents() {
            let rank = build.rank(id);
            let result = talents().unlearn(WARLOCK, &build, id);
            if rank == 0 {
                assert_eq!(result, None, "{build:?}: nothing of {id:?} to take back");
                continue;
            }
            let mut without = build.clone();
            if rank == 1 {
                without.ranks.remove(&id);
            } else {
                without.ranks.insert(id, rank - 1);
            }
            let legal = talents().validate_build(WARLOCK, &without).is_ok();
            assert_eq!(result.is_some(), legal, "{build:?} - {id:?}");
            if let Some(next) = result {
                assert_eq!(next, without);
                given_back += 1;
            } else {
                kept += 1;
            }
        }
    }
    // A tier-1 rank the open tier 2 leans on is kept; others come back.
    assert!(
        kept > 0 && given_back > 0,
        "kept {kept}, given back {given_back}"
    );
}

#[test]
fn each_blocker_names_its_reason() {
    let empty = Build::default();
    assert_eq!(
        talents().learn_blocker(WARLOCK, &empty, TalentId::UnholyPower),
        Some(LearnBlocker::TierClosed {
            tree: TreeId::Demonology,
            needed: 2,
            has: 0
        })
    );
    let maxed = Build::of(&[(TalentId::DemonicEmbrace, 2)]);
    assert_eq!(
        talents().learn_blocker(WARLOCK, &maxed, TalentId::DemonicEmbrace),
        Some(LearnBlocker::MaxRank)
    );
    assert_eq!(
        talents().learn_blocker(CharacterClass::Mage, &empty, TalentId::DemonicEmbrace),
        Some(LearnBlocker::NotThisClass)
    );
    assert_eq!(
        talents().learn_blocker(WARLOCK, &empty, TalentId::DemonicEmbrace),
        None
    );
}

#[test]
fn the_tree_with_the_most_points_names_the_spec() {
    assert_eq!(talents().spec(WARLOCK, &Build::default()), None);
    let spec = talents()
        .spec(
            WARLOCK,
            &Build::of(&[(TalentId::DemonicEmbrace, 2), (TalentId::FelStamina, 1)]),
        )
        .expect("a spent build has a spec");
    assert_eq!(spec.label, "Demonology Warlock");
    assert_eq!(spec.short, "3");
    assert_eq!(spec.line(), "Demonology Warlock 3");
    // A class without trees has no spec, whatever it is handed.
    assert_eq!(
        talents().spec(
            CharacterClass::Mage,
            &Build::of(&[(TalentId::DemonicEmbrace, 1)])
        ),
        None
    );
}

/// The spec over three trees, the shape every class's trees will have: the
/// tree with the most points names it, a tie at the top is a hybrid named in
/// display order, and the shorthand gives every tree in display order.
#[test]
fn the_spec_reads_three_trees_and_names_a_hybrid() {
    let named = |points: [u32; 3]| {
        Spec::from_tree_points(
            WARLOCK,
            &[
                ("Affliction", points[0]),
                ("Demonology", points[1]),
                ("Destruction", points[2]),
            ],
        )
    };
    assert_eq!(named([0, 0, 0]), None);
    let deep = named([2, 6, 0]).expect("spent");
    assert_eq!(deep.label, "Demonology Warlock");
    assert_eq!(deep.short, "2/6/0");
    let late = named([0, 2, 6]).expect("spent");
    assert_eq!(late.label, "Destruction Warlock");
    let hybrid = named([4, 0, 4]).expect("spent");
    assert_eq!(hybrid.label, "Affliction-Destruction hybrid");
    assert_eq!(hybrid.short, "4/0/4");
    // A tie BELOW the top is no hybrid.
    assert_eq!(named([1, 6, 1]).expect("spent").label, "Demonology Warlock");
}

/// The shipped trees with Unholy Power needing Fel Stamina 2/2 — an arrow the
/// shipped data does not draw yet, so the prerequisite rule is held here.
fn requiring_fixture() -> TalentDefinitions {
    let text = std::fs::read_to_string(arenasim::paths::asset_path("config/talents.ron"))
        .expect("talents.ron reads");
    let from = "per_rank: [PetStatPercent(WeaponDamage, 0.10)],";
    assert!(text.contains(from));
    let text = text.replacen(from, &format!("requires: Some((FelStamina, 2)), {from}"), 1);
    TalentDefinitions::from_ron(&text, AbilityDefinitions::shipped()).expect("fixture validates")
}

#[test]
fn a_prerequisite_blocks_until_it_is_met_and_holds_its_rank() {
    let talents = requiring_fixture();
    let open_gate = Build::of(&[(TalentId::DemonicEmbrace, 2)]);
    assert_eq!(
        talents.learn_blocker(WARLOCK, &open_gate, TalentId::UnholyPower),
        Some(LearnBlocker::Requires {
            talent: TalentId::FelStamina,
            rank: 2
        })
    );
    let met = Build::of(&[(TalentId::FelStamina, 2)]);
    assert_eq!(
        talents.learn_blocker(WARLOCK, &met, TalentId::UnholyPower),
        None
    );
    // ...and once taken, the rank it leans on stays.
    let leaning = Build::of(&[(TalentId::FelStamina, 2), (TalentId::UnholyPower, 1)]);
    assert_eq!(
        talents.unlearn(WARLOCK, &leaning, TalentId::FelStamina),
        None
    );
    // The rule still answers exactly as the build's legality does.
    for points in 0..=MAX_TALENT_POINTS {
        for build in talents.legal_builds(WARLOCK, points) {
            for id in warlock_talents() {
                let mut with = build.clone();
                *with.ranks.entry(id).or_insert(0) += 1;
                assert_eq!(
                    talents.learn_blocker(WARLOCK, &build, id).is_none(),
                    talents.validate_build(WARLOCK, &with).is_ok(),
                    "{build:?} + {id:?}"
                );
            }
        }
    }
}

#[test]
fn a_floor_keeps_its_ranks_and_reset_returns_to_it() {
    let floor = Build::of(&[(TalentId::DemonicEmbrace, 1)]);
    let mut build = Build::of(&[(TalentId::DemonicEmbrace, 2)]);
    // The pending rank comes back...
    assert!(apply_click(
        talents(),
        WARLOCK,
        &mut build,
        Some(&floor),
        TalentId::DemonicEmbrace,
        NodeClick::Unlearn
    ));
    assert_eq!(build, floor);
    // ...the locked-in one does not.
    assert!(!apply_click(
        talents(),
        WARLOCK,
        &mut build,
        Some(&floor),
        TalentId::DemonicEmbrace,
        NodeClick::Unlearn
    ));
    assert_eq!(build, floor);
    // With no floor, the same rank comes back.
    assert!(apply_click(
        talents(),
        WARLOCK,
        &mut build,
        None,
        TalentId::DemonicEmbrace,
        NodeClick::Unlearn
    ));
    assert!(build.is_empty());
}

#[test]
fn the_drawn_tree_marks_open_and_locked_nodes_and_pending_ranks() {
    let items = load_item_definitions().expect("items.ron must load");
    let data = data(&items);
    let build = Build::of(&[(TalentId::DemonicEmbrace, 2)]);
    let floor = Build::of(&[(TalentId::DemonicEmbrace, 1)]);
    let (columns, ids) = tree_columns(talents(), WARLOCK, &build, Some(&floor), &data, true);
    assert_eq!(columns.len(), talents().trees(WARLOCK).len());
    let node = |id: TalentId| {
        let (t, n) = ids
            .iter()
            .enumerate()
            .find_map(|(t, tree)| tree.iter().position(|x| *x == id).map(|n| (t, n)))
            .expect("drawn");
        &columns[t].nodes[n]
    };
    let embrace = node(TalentId::DemonicEmbrace);
    assert_eq!((embrace.rank, embrace.pending, embrace.open), (2, 1, false));
    assert!(node(TalentId::UnholyPower).open, "two points open tier 2");
    assert!(node(TalentId::FelStamina).open);
    assert_eq!(columns[0].points, 2);
    assert!(columns[0].leading);
}

// ===========================================================================
// THE CLICK CONTRACT, ON THE DRAWN PANEL
// ===========================================================================

fn data(items: &arenasim::states::play_match::equipment::ItemDefinitions) -> EncyclopediaData<'_> {
    EncyclopediaData {
        items,
        abilities: AbilityDefinitions::shipped(),
        item_icons: None,
        class_icons: None,
        ability_icons: None,
        talents: talents(),
    }
}

/// The panel in a bare egui context, its build spent by raw pointer events.
struct Panel {
    ctx: egui::Context,
    build: Build,
    last: registry::Frame,
    opened: Option<Topic>,
    items: arenasim::states::play_match::equipment::ItemDefinitions,
}

impl Panel {
    fn new(build: Build) -> Self {
        let ctx = egui::Context::default();
        arenasim::ui::install(&ctx);
        let mut panel = Panel {
            ctx,
            build,
            last: registry::Frame::default(),
            opened: None,
            items: load_item_definitions().expect("items.ron must load"),
        };
        panel.frame(Vec::new(), egui::Modifiers::NONE);
        panel.frame(Vec::new(), egui::Modifiers::NONE);
        panel
    }

    fn frame(&mut self, events: Vec<egui::Event>, modifiers: egui::Modifiers) {
        registry::arm(&self.ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 900.0),
            )),
            modifiers,
            events,
            ..Default::default()
        };
        let data = data(&self.items);
        let build = &mut self.build;
        let mut opened = None;
        let _ = self.ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                opened = spend_panel(ui, WARLOCK, build, None, &data, "test");
            });
        });
        if opened.is_some() {
            self.opened = opened;
        }
        self.last = registry::snapshot(&self.ctx).expect("armed");
    }

    /// Press and release `button` on `talent`'s node, holding `modifiers`.
    fn click(&mut self, talent: TalentId, button: egui::PointerButton, modifiers: egui::Modifiers) {
        let id = format!("talent:{}", talent.as_str());
        let pos = self
            .last
            .widget(&id)
            .unwrap_or_else(|| panic!("{id} is not drawn: {:?}", self.last.widgets))
            .rect
            .center();
        self.frame(vec![egui::Event::PointerMoved(pos)], modifiers);
        for pressed in [true, false] {
            self.frame(
                vec![egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                }],
                modifiers,
            );
        }
        self.frame(Vec::new(), egui::Modifiers::NONE);
    }

    fn left(&mut self, talent: TalentId) {
        self.click(talent, egui::PointerButton::Primary, egui::Modifiers::NONE);
    }
}

#[test]
fn a_click_learns_a_rank_and_a_shift_click_takes_it_back() {
    let mut panel = Panel::new(Build::default());
    panel.left(TalentId::DemonicEmbrace);
    assert_eq!(panel.build, Build::of(&[(TalentId::DemonicEmbrace, 1)]));
    panel.left(TalentId::DemonicEmbrace);
    assert_eq!(panel.build, Build::of(&[(TalentId::DemonicEmbrace, 2)]));
    assert!(
        panel
            .last
            .has_note("talents:test points=2 spec=Demonology Warlock 2"),
        "{:?}",
        panel.last.notes
    );

    panel.click(
        TalentId::DemonicEmbrace,
        egui::PointerButton::Primary,
        egui::Modifiers::SHIFT,
    );
    assert_eq!(panel.build, Build::of(&[(TalentId::DemonicEmbrace, 1)]));
    assert_eq!(panel.opened, None, "no click here opens a page");
}

#[test]
fn a_click_on_a_locked_node_changes_nothing() {
    let mut panel = Panel::new(Build::default());
    let locked = panel
        .last
        .widget("talent:UnholyPower")
        .expect("drawn")
        .enabled;
    assert!(!locked, "tier 2 is locked with no points spent");
    panel.left(TalentId::UnholyPower);
    assert!(panel.build.is_empty());

    // Two points open it.
    panel.left(TalentId::DemonicEmbrace);
    panel.left(TalentId::FelStamina);
    assert!(
        panel
            .last
            .widget("talent:UnholyPower")
            .expect("drawn")
            .enabled
    );
    panel.left(TalentId::UnholyPower);
    assert_eq!(panel.build.rank(TalentId::UnholyPower), 1);
}

#[test]
fn a_right_click_asks_for_the_talents_page_and_spends_nothing() {
    let mut panel = Panel::new(Build::default());
    panel.click(
        TalentId::FelStamina,
        egui::PointerButton::Secondary,
        egui::Modifiers::NONE,
    );
    assert_eq!(panel.opened, Some(Topic::Talent(TalentId::FelStamina)));
    assert!(panel.build.is_empty());
}

#[test]
fn reset_clears_the_build() {
    let mut panel = Panel::new(Build::of(&[(TalentId::DemonicEmbrace, 2)]));
    let pos = panel
        .last
        .widget("talents:reset")
        .expect("Reset is drawn")
        .rect
        .center();
    panel.frame(vec![egui::Event::PointerMoved(pos)], egui::Modifiers::NONE);
    for pressed in [true, false] {
        panel.frame(
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
            egui::Modifiers::NONE,
        );
    }
    assert!(panel.build.is_empty());
}

// ===========================================================================
// A PLAYED MATCH CARRIES EACH BUILD TO RESULTS
// ===========================================================================

/// A graphical match hands the Results screen every combatant's build, which
/// is what Results names a spec from: a talented Warlock's build, and an empty
/// one for its untalented opponent. Played on the real `StatesPlugin`
/// schedule (the shape of `tests/client_ai_profile.rs`).
#[test]
fn a_played_match_hands_results_each_combatants_build() {
    use arenasim::combat::CombatPlugin;
    use arenasim::headless::config::HeadlessMatchConfig;
    use arenasim::states::play_match::components::VictoryCelebration;
    use arenasim::states::play_match::equipment::EquipmentPlugin;
    use arenasim::states::play_match::{
        AbilityConfigPlugin, MapConfigPlugin, MovementConfigPlugin,
    };
    use arenasim::states::{GameState, StatesPlugin};
    use bevy::asset::AssetPlugin;
    use bevy::prelude::*;
    use bevy::state::app::StatesPlugin as BevyStatesPlugin;
    use bevy::time::TimeUpdateStrategy;

    const FRAME: std::time::Duration = std::time::Duration::from_micros(16_667);
    let build = Build::of(&[(TalentId::DemonicEmbrace, 2), (TalentId::FelStamina, 1)]);
    let cfg: HeadlessMatchConfig = serde_json::from_str(
        r#"{"team1":["Warlock"],"team2":["Warrior"],"random_seed":7,
            "team1_builds":[{"DemonicEmbrace":2,"FelStamina":1}]}"#,
    )
    .expect("config parses");

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .add_plugins(BevyStatesPlugin)
        .add_plugins(bevy::input::InputPlugin)
        .add_plugins(bevy::window::WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        })
        .init_state::<GameState>()
        .init_resource::<bevy_egui::EguiUserTextures>()
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>()
        .init_asset::<Shader>()
        .init_asset::<bevy::scene::Scene>()
        .init_asset::<bevy::gltf::Gltf>()
        .init_asset::<bevy::gltf::GltfNode>()
        .init_asset::<bevy::gltf::GltfMesh>()
        .init_asset::<bevy::gltf::GltfPrimitive>()
        .add_plugins((
            AbilityConfigPlugin,
            MovementConfigPlugin,
            MapConfigPlugin,
            EquipmentPlugin,
            CombatPlugin,
            arenasim::settings::SettingsPlugin,
            StatesPlugin,
        ))
        .add_plugins(bevy::transform::TransformPlugin)
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .insert_resource(cfg.to_match_config().expect("config resolves"));
    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::PlayMatch);
    let mut results = None;
    for _ in 0..(330.0 / FRAME.as_secs_f32()) as usize {
        app.update();
        if let Some(c) = app.world().get_resource::<VictoryCelebration>() {
            results = Some(c.match_results.clone());
            break;
        }
    }
    let results = results.expect("the match ended");
    assert_eq!(results.team1_combatants.len(), 1);
    assert_eq!(results.team1_combatants[0].build, build);
    assert!(results.team2_combatants[0].build.is_empty());
    assert_eq!(
        talents()
            .spec(WARLOCK, &results.team1_combatants[0].build)
            .map(|s| s.line()),
        Some("Demonology Warlock 3".to_string())
    );
}

/// Results names the spec of a talented combatant's row, and of no other.
#[test]
fn results_names_a_talented_combatants_spec() {
    use arenasim::combat::log::CombatLog;
    use arenasim::states::play_match::ai_profile::AiProfiles;
    use arenasim::states::play_match::{CombatantStats, MatchResults};
    use arenasim::states::results_ui::draw_results_screen;
    use arenasim::states::GameState;
    use arenasim::ui::widgets::Nav;

    let stats = |class, build: Build| CombatantStats {
        class,
        slot: 0,
        damage_dealt: 100.0,
        damage_taken: 50.0,
        healing_done: 0.0,
        survived: true,
        build,
    };
    let results = MatchResults {
        winner: Some(1),
        duration_secs: 30.0,
        team1_combatants: vec![stats(
            WARLOCK,
            Build::of(&[(TalentId::DemonicEmbrace, 2), (TalentId::FelStamina, 1)]),
        )],
        team2_combatants: vec![stats(CharacterClass::Mage, Build::default())],
        pet_damage_links: Default::default(),
        ai_profiles: AiProfiles::client_default(),
    };
    let items = load_item_definitions().expect("items.ron must load");
    let data = data(&items);
    let log = CombatLog::default();
    let ctx = egui::Context::default();
    arenasim::ui::install(&ctx);
    let mut last = registry::Frame::default();
    for _ in 0..2 {
        registry::arm(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1500.0, 820.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            let _ = draw_results_screen(
                ctx,
                Some(&results),
                &log,
                &data,
                Nav::Return(GameState::MainMenu),
            );
        });
        last = registry::snapshot(&ctx).expect("armed");
    }
    let specs: Vec<&String> = last
        .notes
        .iter()
        .filter(|n| n.starts_with("results:spec"))
        .collect();
    assert_eq!(specs, vec!["results:spec t1s0 Demonology Warlock 3"]);
}
