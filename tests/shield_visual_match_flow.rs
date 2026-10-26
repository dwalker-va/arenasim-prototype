//! Divine Shield's bubble in a REAL match: from the frame the Paladin holds
//! the immunity to the frame it ends, the bubble's rig is up (birth or loop)
//! with every card it wears visible, placed on the Paladin and lit; and when
//! the immunity ends the rig plays its death.
//!
//! ## Why this exists
//!
//! The 0.8 review reported the bubble "invisible when cast" (AS-401). The
//! bare-app probes in `divine_shield_visual_probes.rs` drive the shield
//! systems alone, so they cannot say whether the match path draws it. This
//! boots `PlayMatch` through the real `StatesPlugin` schedule the way
//! `--replay` does (the `shield_cast_hands_match_flow.rs` shape), with Bevy's
//! own visibility propagation, on the review's replay. It found the rig drawn
//! for the whole immunity: the faintness is the client weights over the
//! bright sand, a tuning matter, not a missing bubble.
//!
//! Runs with no window or GPU. Visual-only systems never touch the sim, so the
//! match is the seeded one.

use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::components::{
    ActiveAuras, Combatant, ShieldCard, ShieldKind, ShieldPhase, ShieldVisual,
};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{
    is_divine_shield, AbilityConfigPlugin, GameRng, MapConfigPlugin, MovementConfigPlugin,
};
use arenasim::states::{GameState, StatesPlugin};
use arenasim::CharacterClass;
use arenasim::HeadlessMatchConfig;

/// One rendered frame at 60 fps.
const FRAME: Duration = Duration::from_micros(16_667);
/// Long enough for the review's Divine Shield (25.7 s to 37.7 s, log clock).
const MAX_FRAMES: usize = 60 * 45;
/// The review's replay (AS-401).
const REPLAY: &str = r#"{"team1":["Paladin"],"team2":["Rogue"],"map":"BasicArena","random_seed":1,"ai_profile":"Legacy"}"#;

/// One frame's view of the Paladin's bubble.
#[derive(Clone, Debug)]
struct Frame {
    immune: bool,
    /// The Divine Shield rig's phase, if there is one.
    phase: Option<ShieldPhase>,
    /// Its cards that are visible, stand within 3 yd of the Paladin and carry
    /// a weight above zero.
    lit_cards: usize,
    /// Its cards at all.
    cards: usize,
}

#[derive(Resource, Default)]
struct Seen(Vec<Frame>);

fn boot() -> App {
    let cfg: HeadlessMatchConfig = serde_json::from_str(REPLAY).unwrap();
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
        .add_plugins(bevy::transform::TransformPlugin)
        // The client's own visibility propagation, so a card hidden by its
        // rig or its own `Visibility` would read as hidden here.
        .add_plugins(bevy::render::view::VisibilityPlugin)
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
    app.insert_resource(cfg.to_match_config().unwrap())
        .insert_resource(cfg.ai_profiles().unwrap())
        .insert_resource(GameRng::from_seed(cfg.random_seed.unwrap()))
        .insert_state(GameState::PlayMatch)
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .init_resource::<Seen>()
        .add_systems(Last, observe);
    app
}

#[allow(clippy::type_complexity)]
fn observe(
    mut seen: ResMut<Seen>,
    paladins: Query<(Entity, &Combatant, &ActiveAuras, &GlobalTransform)>,
    rigs: Query<(&ShieldVisual, &Children)>,
    cards: Query<
        (
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
            &InheritedVisibility,
        ),
        With<ShieldCard>,
    >,
    materials: Res<Assets<StandardMaterial>>,
) {
    let Some((paladin, _, auras, at)) = paladins
        .iter()
        .find(|(_, c, _, _)| c.class == CharacterClass::Paladin)
    else {
        return;
    };
    let immune = auras.auras.iter().any(is_divine_shield);
    let rig = rigs
        .iter()
        .find(|(r, _)| r.bearer == paladin && r.kind == ShieldKind::DivineShield);
    let (phase, cards_n, lit) = match rig {
        None => (None, 0, 0),
        Some((r, kids)) => {
            let mut n = 0;
            let mut lit = 0;
            for k in kids.iter() {
                let Ok((tf, mat, vis)) = cards.get(k) else {
                    continue;
                };
                n += 1;
                let weight = materials.get(&mat.0).map_or(0.0, |m| m.base_color.alpha());
                let near = tf.translation().distance(at.translation()) < 3.0;
                if vis.get() && near && weight > 0.0 {
                    lit += 1;
                }
            }
            (Some(r.phase), n, lit)
        }
    };
    seen.0.push(Frame {
        immune,
        phase,
        lit_cards: lit,
        cards: cards_n,
    });
}

#[test]
fn the_bubble_is_drawn_for_the_whole_immunity_in_a_real_match() {
    let mut app = boot();
    for _ in 0..MAX_FRAMES {
        app.update();
        if app.world().resource::<State<GameState>>().get() != &GameState::PlayMatch {
            break;
        }
    }
    let seen = std::mem::take(&mut app.world_mut().resource_mut::<Seen>().0);

    let immune: Vec<usize> = (0..seen.len()).filter(|&i| seen[i].immune).collect();
    // Not vacuous: the review's bubble, about 12 s of it, is in the window.
    assert!(
        immune.len() > 60 * 10,
        "the replay held Divine Shield for only {} frames",
        immune.len()
    );
    let (first, last) = (immune[0], *immune.last().unwrap());
    assert_eq!(
        immune.len(),
        last - first + 1,
        "one unbroken immunity expected in this replay"
    );

    // The rig is spawned by the next frame's Update, so the first immune frame
    // may precede it; from the second, it must be up and drawn. Its cards are
    // zero-weight only in the birth's first 200 ms, the client's delay.
    let wears = ShieldKind::DivineShield.layers().len();
    for (i, f) in seen.iter().enumerate().take(last + 1).skip(first + 1) {
        assert!(
            matches!(f.phase, Some(ShieldPhase::Birth | ShieldPhase::Loop)),
            "frame {i}: the Paladin is immune but its bubble is {:?}",
            f.phase
        );
        assert_eq!(
            f.cards, wears,
            "frame {i}: the bubble wears {} cards",
            f.cards
        );
        if i > first + 1 + 15 {
            assert_eq!(
                f.lit_cards, wears,
                "frame {i}: only {} of the bubble's cards are visible, placed and lit",
                f.lit_cards
            );
        }
    }
    // ...and it goes with the immunity, through its death.
    let after = &seen[last + 2];
    assert_eq!(
        after.phase,
        Some(ShieldPhase::Death),
        "the immunity ended and the bubble is {:?}",
        after.phase
    );
}
