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
//!
//! The same is true of every other ordering a visual effect depends on: its
//! probe harness chains the systems itself, so removing an `.after(..)` or a
//! `.chain()` from `states/mod.rs` fails nothing there. [`ORDERINGS`] names
//! the orderings the held effects rest on, each with the reason, and
//! [`the_client_keeps_every_ordering_the_effects_rest_on`] reads each one off
//! the real registration.

use std::collections::{BTreeSet, HashMap, HashSet};

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
const PLACEMENTS: [&str; 20] = [
    "place_shield_cards",
    "place_vengeance_hands",
    "place_shadow_hands",
    "place_divine_protection_globes",
    "place_shadow_dance",
    "place_vampiric_embrace_implosions",
    "place_silence_breaths",
    "place_discipline_states",
    "place_inner_focus",
    "place_blessed_recovery_bursts",
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

    /// The node and every node under it in the set hierarchy.
    fn under(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            let n = out[i];
            for (p, c) in &self.hierarchy {
                if *p == n && !out.contains(c) {
                    out.push(*c);
                }
            }
            i += 1;
        }
        out
    }

    /// Whether the schedule runs every node named `first` before every node
    /// named `then` (a system, or a set by its `Debug` name): a path of
    /// ordering edges, where an edge between two sets orders everything
    /// under the one before everything under the other.
    fn orders(&self, first: &str, then: &str) -> bool {
        let mut after: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for (x, y) in &self.dependency {
            let ys = self.under(*y);
            for a in self.under(*x) {
                after.entry(a).or_default().extend(ys.iter().copied());
            }
        }
        let named = |name: &str| -> Vec<NodeId> {
            self.systems
                .iter()
                .map(|(id, ..)| *id)
                .chain(self.sets.iter().map(|(id, _)| *id))
                .filter(|id| self.name(*id) == name)
                .collect()
        };
        let (starts, ends) = (named(first), named(then));
        assert!(!starts.is_empty(), "no node named {first}");
        assert!(!ends.is_empty(), "no node named {then}");
        starts.into_iter().all(|start| {
            let mut seen = HashSet::from([start]);
            let mut stack = vec![start];
            while let Some(n) = stack.pop() {
                for m in after.get(&n).into_iter().flatten() {
                    if seen.insert(*m) {
                        stack.push(*m);
                    }
                }
            }
            ends.iter().all(|e| seen.contains(e))
        })
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

/// The set's members are exactly the placements, and exactly the `PostUpdate`
/// systems that read a `VisualBody` — every placement reads it through
/// `BodyHold` (the Cyclone lift itself only filters on one).
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
    assert_eq!(
        readers, expected,
        "the PostUpdate systems that read the rendered body are the set's members"
    );
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

/// The orderings the held effects rest on, as `(first, then, why)`: each
/// is a `.chain()`, `.after(..)` or `.before(..)` in `states/mod.rs` that the
/// effect's probe harness restates in its own chain, so only this sees it
/// dropped. `Update` systems all run before `PostUpdate`'s, so an ordering
/// between the two schedules is the schedule's and is not listed.
const ORDERINGS: [(&str, &str, &str); 34] = [
    (
        "handle_camera_input",
        "update_camera_position",
        "the camera moves on this frame's input",
    ),
    (
        "animate_body_lean",
        "update_finisher_hand_flashes",
        "the flash stands on the leaned hand",
    ),
    (
        "fire_bleed_sprays",
        "update_bleed_sprays",
        "a spray emits the frame it fires",
    ),
    (
        "update_bleed_sprays",
        "animate_client_particles",
        "droplets are born before the ager flies them",
    ),
    (
        "animate_client_particles",
        "billboard_hunter_shots",
        "particles are turned after they move",
    ),
    (
        "unhold_demonology_particles",
        "animate_client_particles",
        "the ager moves the true position, never the held one",
    ),
    (
        "unhold_demonology_particles",
        "spawn_demonology_landings",
        "a new spark is not unheld from a stale point",
    ),
    (
        "spawn_demonology_landings",
        "animate_soul_link_columns",
        "a column animates the frame it is built",
    ),
    (
        "animate_soul_link_columns",
        "animate_sacrifice_triads",
        "the chain the probes play",
    ),
    (
        "animate_sacrifice_triads",
        "animate_client_particles",
        "motes are born before the ager flies them",
    ),
    (
        "spawn_warlock_dot_visuals",
        "animate_siphon_life_glows",
        "the DoT detector builds the Siphon Life rig",
    ),
    (
        "animate_siphon_life_glows",
        "age_siphon_life_particles",
        "a particle ages from the frame it is born",
    ),
    (
        "age_siphon_life_particles",
        "cleanup_siphon_life_glows",
        "the glow ends after its last frame's ageing",
    ),
    (
        "spawn_warlock_dot_visuals",
        "animate_ua_states",
        "UA's rig animates the frame it is built",
    ),
    (
        "animate_ua_states",
        "billboard_warlock_dot_visuals",
        "the glow turns after it is sized",
    ),
    (
        "billboard_warlock_dot_visuals",
        "cleanup_warlock_dot_visuals",
        "a state ends after its last pose",
    ),
    (
        "fire_expose_armor_crests",
        "animate_expose_armor_crests",
        "a crest emits the frame it fires",
    ),
    (
        "animate_expose_armor_crests",
        "age_expose_armor_particles",
        "a particle ages from the frame it is born",
    ),
    (
        "spawn_conflagrate_landings",
        "animate_conflagrate_bursts",
        "a burst emits the frame it lands",
    ),
    (
        "animate_conflagrate_bursts",
        "animate_conflagrate_particles",
        "a particle flies from the frame it is born",
    ),
    (
        "spawn_aftermath_hits",
        "animate_aftermath_hits",
        "a hit emits the frame it lands",
    ),
    (
        "animate_aftermath_hits",
        "age_aftermath_hit_particles",
        "a particle ages from the frame it is born",
    ),
    (
        "spawn_aftermath_hits",
        "tick_hit_flinch",
        "the wound's dip renders on the landing frame",
    ),
    (
        "spawn_heal_impacts",
        "animate_heal_impacts",
        "a landing emits the frame it lands",
    ),
    (
        "sync_mind_flay_channels",
        "animate_mind_flay_channels",
        "a channel's stars pose the frame it starts",
    ),
    (
        "animate_mind_flay_channels",
        "animate_client_particles",
        "the star's particles are born before they age",
    ),
    (
        "animate_client_particles",
        "billboard_mind_flay_particles",
        "the star's particles turn after they move",
    ),
    (
        "spawn_vampiric_embrace_implosions",
        "animate_vampiric_embrace_implosions",
        "an implosion emits the frame it lands",
    ),
    (
        "spawn_silence_breaths",
        "animate_silence_breaths",
        "a breath emits the frame it is raised",
    ),
    (
        "spawn_immolate_burns",
        "animate_immolate_burns",
        "a burn emits the frame it is raised",
    ),
    (
        "cleanup_heal_cast_glows",
        "update_inner_focus_palms",
        "a palm yields to this frame's cast hands, so no frame shows both",
    ),
    (
        "spawn_inner_focus_flashes",
        "update_inner_focus_flashes",
        "a flash animates the frame it lands",
    ),
    (
        "update_inner_focus_flashes",
        "update_inner_focus_palms",
        "the chain the probes play",
    ),
    (
        "spawn_blessed_recovery_bursts",
        "update_blessed_recovery_bursts",
        "a burst ages from the frame it lands",
    ),
];

/// `PostUpdate` orderings beside the set's own.
const POST_UPDATE_ORDERINGS: [(&str, &str, &str); 1] = [(
    "hide_dot_states_on_sheep",
    "VisibilityPropagate",
    "a DoT state on a sheep is hidden the frame it is built",
)];

#[test]
fn the_client_keeps_every_ordering_the_effects_rest_on() {
    let mut app = client_app();
    let update = Graph::of(&mut app, Update);
    let broken: Vec<String> = ORDERINGS
        .iter()
        .filter(|(first, then, _)| !update.orders(first, then))
        .map(|(first, then, why)| format!("{first} before {then} ({why})"))
        .collect();
    assert!(broken.is_empty(), "Update no longer orders: {broken:#?}");
    let post = Graph::of(&mut app, PostUpdate);
    let broken: Vec<String> = POST_UPDATE_ORDERINGS
        .iter()
        .filter(|(first, then, _)| !post.orders(first, then))
        .map(|(first, then, why)| format!("{first} before {then} ({why})"))
        .collect();
    assert!(
        broken.is_empty(),
        "PostUpdate no longer orders: {broken:#?}"
    );
    // Non-vacuity: an ordering the schedule does not make is reported.
    assert!(!update.orders("animate_client_particles", "update_bleed_sprays"));
    assert!(!update.orders("animate_heal_impacts", "spawn_heal_impacts"));
}
