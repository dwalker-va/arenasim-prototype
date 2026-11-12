//! A screen opened forward starts its body at the top, and a return finds it
//! where it was left. Driven through the REAL `ScreenNav` in a Bevy app, so
//! `open` and `leave` themselves decide, and the client's own bridge
//! (`chrome::open_fresh_bodies_system`) is pinned to the schedule that carries
//! an open to egui before the opened screen's first draw (AS-470).

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy_egui::egui;

use arenasim::combat::CombatPlugin;
use arenasim::keybindings::Keybindings;
use arenasim::states::chrome::{open_fresh_bodies, ScreenContext, ScreenNav};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{AbilityConfigPlugin, MapConfigPlugin, MovementConfigPlugin};
use arenasim::states::{GameState, StatesPlugin};

/// One move through the nav, as a screen's wrapper makes it.
#[derive(Clone, Copy)]
enum Move {
    Open(GameState),
    Leave(GameState),
}

#[derive(Resource, Default)]
struct Pending(Option<Move>);

fn drive_nav(mut pending: ResMut<Pending>, mut nav: ScreenNav) {
    match pending.0.take() {
        Some(Move::Open(to)) => nav.open(to),
        Some(Move::Leave(to)) => nav.leave(to),
        None => {}
    }
}

/// The nav and nothing else: states, input, keybindings, the screen context.
fn nav_app(start: GameState) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(BevyStatesPlugin)
        .add_plugins(bevy::input::InputPlugin)
        .insert_state(start)
        .init_resource::<ScreenContext>()
        .insert_resource(Keybindings::default())
        .init_resource::<Pending>()
        .add_systems(Update, drive_nav);
    app.update();
    app
}

/// Make `mv` through the real nav, let the state transition apply, then hand
/// the nav's record to egui as the client's bridge does.
fn step(app: &mut App, ctx: &egui::Context, mv: Move) {
    app.world_mut().resource_mut::<Pending>().0 = Some(mv);
    app.update();
    app.update();
    let to = match mv {
        Move::Open(to) | Move::Leave(to) => to,
    };
    assert_eq!(app.world().resource::<State<GameState>>().get(), &to);
    open_fresh_bodies(&mut app.world_mut().resource_mut::<ScreenContext>(), ctx);
}

/// Draw `screen`'s body over tall content and return its scroll offset;
/// with `scroll_to`, leave it scrolled there for the next visit.
fn draw(ctx: &egui::Context, screen: GameState, scroll_to: Option<f32>) -> f32 {
    let mut offset = f32::NAN;
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let body = screen.show_body(ui, |ui| {
                ui.allocate_space(egui::vec2(100.0, 5000.0));
            });
            offset = body.state.offset.y;
            if let Some(y) = scroll_to {
                let mut state = body.state;
                state.offset.y = y;
                state.store(ui.ctx(), body.id);
            }
        });
    });
    offset
}

#[test]
fn an_open_starts_at_the_top_and_a_leave_comes_back_where_it_was() {
    let ctx = egui::Context::default();
    let mut app = nav_app(GameState::ConfigureMatch);

    // Open the sheet, and scroll it down.
    step(&mut app, &ctx, Move::Open(GameState::ViewCombatant));
    assert_eq!(draw(&ctx, GameState::ViewCombatant, Some(900.0)), 0.0);
    assert_eq!(
        draw(&ctx, GameState::ViewCombatant, None),
        900.0,
        "setup: the sheet is scrolled"
    );

    // Out to the encyclopedia, which opens at its top, and back with a leave:
    // the sheet is where it was left.
    step(&mut app, &ctx, Move::Open(GameState::Encyclopedia));
    assert_eq!(draw(&ctx, GameState::Encyclopedia, Some(300.0)), 0.0);
    step(&mut app, &ctx, Move::Leave(GameState::ViewCombatant));
    assert_eq!(
        draw(&ctx, GameState::ViewCombatant, None),
        900.0,
        "a leave back to the sheet must restore its offset"
    );

    // Leave for Match Setup, and open the sheet fresh: at the top again.
    step(&mut app, &ctx, Move::Leave(GameState::ConfigureMatch));
    step(&mut app, &ctx, Move::Open(GameState::ViewCombatant));
    assert_eq!(
        draw(&ctx, GameState::ViewCombatant, None),
        0.0,
        "a fresh open must start at the top, not where the last visit left it"
    );

    // And the encyclopedia, opened again, is fresh too.
    step(&mut app, &ctx, Move::Open(GameState::Encyclopedia));
    assert_eq!(draw(&ctx, GameState::Encyclopedia, None), 0.0);
}

/// The client app with everything `StatesPlugin` needs, minus the window and
/// renderer, as `tests/encyclopedia_boot.rs` builds it.
fn client_app() -> App {
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
        ));
    app
}

/// The client runs the bridge in `PostUpdate`: after every screen's `Update`,
/// where an open is made, and before the next frame's `Update`, where the
/// opened screen first draws. Anywhere else, or nowhere, and a fresh sheet
/// could draw a frame at the old offset or keep it.
#[test]
fn the_client_hands_opens_to_egui_in_post_update() {
    // Read before the first update: a schedule that has run has moved its
    // systems out of the graph this reads.
    let app = client_app();

    let bridge = "open_fresh_bodies_system";
    let schedules = app.world().resource::<bevy::ecs::schedule::Schedules>();
    let mut found: Vec<String> = Vec::new();
    for (label, schedule) in schedules.iter() {
        for (_id, system, _conditions) in schedule.graph().systems() {
            if system.name().ends_with(bridge) {
                found.push(format!("{label:?}"));
            }
        }
    }
    assert_eq!(
        found,
        vec![format!("{PostUpdate:?}")],
        "the bridge must be registered once, in PostUpdate"
    );
}
