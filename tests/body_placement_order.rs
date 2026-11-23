//! Every system that stands on a unit's RENDERED body, or holds a piece in
//! front of it, runs where the body it reads is the one drawn this frame:
//! after every writer of the body's transform — the `Update` gaits, then
//! `apply_cyclone_lift` in `PostUpdate` — and before transform propagation.
//! In the client they are the members of one set, `BodyPlacement`
//! (`body_hold.rs`), which `StatesPlugin` orders there.
//!
//! ## Why this exists
//!
//! Each effect's probe harness registers its own placement without the
//! Cyclone lift, so no probe sees a placement moved ahead of it; the effect
//! would stand at the un-lifted body on a cycloned victim with nothing
//! failing. So this reads the client's real schedule: the set's members by
//! set equality with the list below, every `PostUpdate` system that reads a
//! `VisualBody` a member of it, no hold left in `Update`, and the set's two
//! edges.

use std::collections::BTreeSet;

use bevy::asset::AssetPlugin;
use bevy::ecs::schedule::{NodeId, ScheduleLabel, Schedules};
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;

use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::components::VisualBody;
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{AbilityConfigPlugin, MapConfigPlugin, MovementConfigPlugin};
use arenasim::states::{GameState, StatesPlugin};

/// Every `BodyPlacement` system, by name. Adding one, or taking one out, is an
/// edit here: membership is a judgement the schedule cannot make for itself.
const PLACEMENTS: [&str; 17] = [
    "place_shield_cards",
    "place_vengeance_hands",
    "place_divine_protection_globes",
    "place_shadow_dance",
    "place_vampiric_embrace_implosions",
    "place_silence_breaths",
    "place_discipline_states",
    "place_siphon_life_glows",
    "place_aftermath_hits",
    "pose_expose_armor_crests",
    "pose_conflagrate_rings",
    "place_conflagrate_particles",
    "place_bleed_droplets",
    "place_demonology_landings",
    "place_ua_states",
    "place_mana_funnel_stars",
    "billboard_heal_impacts",
];

/// The client app with everything `StatesPlugin` needs, minus the window and
/// renderer, as `tests/screen_nav_fresh_open.rs` builds it.
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

/// A system's short name: `place_ua_states`, not its module path.
fn short(name: &str) -> String {
    name.rsplit("::").next().unwrap_or(name).to_string()
}

/// One schedule of the client, initialized so every system knows its access.
struct Graph {
    /// Every system node: its short name and the components it reads.
    systems: Vec<(NodeId, String, bool, bool)>,
    /// Every set node, by its `Debug` name.
    sets: Vec<(NodeId, String)>,
    /// `parent -> child` (a set and what is in it).
    hierarchy: Vec<(NodeId, NodeId)>,
    /// `before -> after`.
    dependency: Vec<(NodeId, NodeId)>,
}

impl Graph {
    fn of(app: &mut App, label: impl ScheduleLabel + Clone) -> Self {
        let world = app.world_mut();
        let visual_body = world.register_component::<VisualBody>();
        let camera = world.register_component::<Camera3d>();
        assert!(
            world.resource::<Schedules>().contains(label.clone()),
            "the client registers the schedule"
        );
        world.schedule_scope(label, |world, schedule| {
            schedule.initialize(world).expect("the schedule builds");
            let graph = schedule.graph();
            let systems = schedule
                .systems()
                .expect("initialized")
                .map(|(id, system)| {
                    let access = system.component_access();
                    (
                        id,
                        short(&system.name()),
                        access.has_component_read(visual_body),
                        access.has_component_read(camera),
                    )
                })
                .collect();
            let sets = graph
                .system_sets()
                .map(|(id, set, _)| (id, format!("{set:?}")))
                .collect();
            let hierarchy = graph.hierarchy().graph().all_edges().collect();
            let dependency = graph.dependency().graph().all_edges().collect();
            Graph {
                systems,
                sets,
                hierarchy,
                dependency,
            }
        })
    }

    fn set(&self, name: &str) -> NodeId {
        let found: Vec<NodeId> = self
            .sets
            .iter()
            .filter(|(_, n)| n == name)
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(found.len(), 1, "one set named {name}: {found:?}");
        found[0]
    }

    fn name(&self, id: NodeId) -> String {
        self.systems
            .iter()
            .find(|(s, ..)| *s == id)
            .map(|(_, n, ..)| n.clone())
            .or_else(|| {
                self.sets
                    .iter()
                    .find(|(s, _)| *s == id)
                    .map(|(_, n)| n.clone())
            })
            .unwrap_or_else(|| format!("{id:?}"))
    }

    /// The node and everything under it in the set hierarchy.
    fn closure(&self, id: NodeId) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            if out.insert(self.name(n)) {
                stack.extend(
                    self.hierarchy
                        .iter()
                        .filter(|(p, _)| *p == n)
                        .map(|(_, c)| *c),
                );
            }
        }
        out
    }

    /// The systems directly in `set`.
    fn members(&self, set: NodeId) -> BTreeSet<String> {
        self.hierarchy
            .iter()
            .filter(|(p, _)| *p == set)
            .filter_map(|(_, c)| {
                self.systems
                    .iter()
                    .find(|(s, ..)| s == c)
                    .map(|(_, n, ..)| n.clone())
            })
            .collect()
    }
}

/// The set's members are exactly the placements, and so is every
/// `PostUpdate` system that reads a `VisualBody` (the Cyclone lift itself
/// only filters on one).
#[test]
fn every_rendered_body_placement_is_in_the_set() {
    let mut app = client_app();
    let graph = Graph::of(&mut app, PostUpdate);
    let expected: BTreeSet<String> = PLACEMENTS.iter().map(|s| s.to_string()).collect();
    assert_eq!(expected.len(), PLACEMENTS.len(), "no name twice");
    let set = graph.set("BodyPlacement");
    assert_eq!(graph.members(set), expected);
    let readers: BTreeSet<String> = graph
        .systems
        .iter()
        .filter(|(_, _, body, _)| *body)
        .map(|(_, n, ..)| n.clone())
        .collect();
    assert!(
        readers.is_subset(&expected),
        "reads the rendered body outside BodyPlacement: {:?}",
        readers.difference(&expected).collect::<Vec<_>>()
    );
    // Non-vacuity: the hold's readers are found by their access.
    assert!(readers.contains("place_siphon_life_glows") && readers.len() >= 12);
}

/// The set runs after the Cyclone lift and before transform propagation.
#[test]
fn the_set_runs_after_the_cyclone_lift_and_before_propagation() {
    let mut app = client_app();
    let graph = Graph::of(&mut app, PostUpdate);
    let set = graph.set("BodyPlacement");
    let after: Vec<BTreeSet<String>> = graph
        .dependency
        .iter()
        .filter(|(_, b)| *b == set)
        .map(|(a, _)| graph.closure(*a))
        .collect();
    assert!(
        after.iter().any(|c| c.contains("apply_cyclone_lift")),
        "BodyPlacement is not ordered after apply_cyclone_lift: {after:?}"
    );
    let before: Vec<BTreeSet<String>> = graph
        .dependency
        .iter()
        .filter(|(a, _)| *a == set)
        .map(|(_, b)| graph.closure(*b))
        .collect();
    assert!(
        before.iter().any(|c| c.contains("TransformPropagate")),
        "BodyPlacement is not ordered before TransformPropagate: {before:?}"
    );
}

/// No hold is left in `Update`, where the Cyclone lift has not run and the
/// camera may not have moved yet: no `Update` system reads both a body and the
/// camera, as every `BodyHold` does.
#[test]
fn no_hold_runs_in_update() {
    let mut app = client_app();
    let graph = Graph::of(&mut app, Update);
    let holds: BTreeSet<String> = graph
        .systems
        .iter()
        .filter(|(_, _, body, camera)| *body && *camera)
        .map(|(_, n, ..)| n.clone())
        .collect();
    assert!(holds.is_empty(), "holds in Update: {holds:?}");
    // Non-vacuity: Update does read bodies (the gaits), and the same test
    // finds every hold in PostUpdate.
    assert!(graph.systems.iter().any(|(_, _, body, _)| *body));
    let post = Graph::of(&mut app, PostUpdate);
    let holds: BTreeSet<String> = post
        .systems
        .iter()
        .filter(|(_, _, body, camera)| *body && *camera)
        .map(|(_, n, ..)| n.clone())
        .collect();
    assert!(
        holds.contains("place_siphon_life_glows") && holds.contains("place_ua_states"),
        "{holds:?}"
    );
}
