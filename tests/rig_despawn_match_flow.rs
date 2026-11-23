//! A rig that ends on a unit's death, in a REAL match: the frame a unit dies,
//! no visual system may parent a new child onto a rig another system is
//! despawning for that death.
//!
//! ## Why this exists
//!
//! AS-507: when a Shadowform Priest died, `update_shadowform_visuals`
//! despawned its feet shroud while `update_shadowform_shrouds` had already
//! queued an `add_child` of a fresh particle onto it in the same command
//! flush, and Bevy's default error handler panicked the client. Only the real
//! `StatesPlugin` schedule decides which of two unordered systems' commands
//! flush first, so a bare app over the two systems cannot show it. This boots
//! `PlayMatch` the way `--replay` does (the `shield_cast_hands_match_flow.rs`
//! shape) and plays a death through it.
//!
//! The death is staged so it happens by construction, not by seed: some
//! seconds after the gates open the unit is set to 1 health, and the match's
//! own next hit kills it through the real kill site. Visual-only systems never
//! touch the sim, so everything else is the seeded match.

use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::components::{
    ClientParticle, Combatant, MatchCountdown, PlayMatchEntity, ShadowformShroud, TransformPuff,
};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{
    AbilityConfigPlugin, GameRng, MapConfigPlugin, MovementConfigPlugin,
};
use arenasim::states::{GameState, StatesPlugin};
use arenasim::{CharacterClass, HeadlessMatchConfig};

/// One rendered frame at 60 fps — one sim tick, the client's own cadence.
const FRAME: Duration = Duration::from_micros(16_667);
/// The longest a match is played looking for the staged death.
const MAX_FRAMES: usize = 60 * 120;
/// Frames played on past the death: the despawn and anything queued against
/// it have flushed by the first.
const AFTER_FRAMES: usize = 10;

/// Which unit the stage brings to 1 health, and when.
#[derive(Resource, Clone, Copy)]
struct Doomed {
    team: u8,
    class: CharacterClass,
    /// Seconds after the gates open.
    after_secs: f32,
}

/// What the observers saw.
#[derive(Resource, Default)]
struct Seen {
    /// The doomed unit, once found.
    unit: Option<Entity>,
    /// The frame it was first seen dead.
    died_at: Option<usize>,
    frame: usize,
}

/// Boot `cfg` as `--replay` does, staging `doomed`'s near-death if given.
fn boot(cfg: &str, doomed: Option<Doomed>) -> App {
    let cfg: HeadlessMatchConfig = serde_json::from_str(cfg).unwrap();
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
    // The `--replay` entrance: straight into the match with the seed set.
    app.insert_resource(cfg.to_match_config().unwrap())
        .insert_resource(cfg.ai_profiles().unwrap())
        .insert_resource(GameRng::from_seed(cfg.random_seed.unwrap()))
        .insert_state(GameState::PlayMatch)
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .init_resource::<Seen>()
        .add_systems(Last, observe_death);
    if let Some(doomed) = doomed {
        app.insert_resource(doomed)
            .add_systems(PreUpdate, stage_near_death);
    }
    app
}

/// The staged fact: `after_secs` after the gates open, the doomed unit stands
/// at 1 health. Applied once, before that frame's fixed ticks.
fn stage_near_death(
    mut done: Local<bool>,
    mut since_gates: Local<f32>,
    time: Res<Time>,
    doomed: Res<Doomed>,
    countdown: Option<Res<MatchCountdown>>,
    mut seen: ResMut<Seen>,
    mut combatants: Query<(Entity, &mut Combatant)>,
) {
    if *done || !countdown.is_some_and(|c| c.gates_opened) {
        return;
    }
    *since_gates += time.delta_secs();
    if *since_gates < doomed.after_secs {
        return;
    }
    for (entity, mut c) in combatants.iter_mut() {
        if c.team == doomed.team && c.class == doomed.class && c.is_alive() {
            c.current_health = 1.0;
            seen.unit = Some(entity);
            *done = true;
            return;
        }
    }
    panic!(
        "the staged match has no living team {} {:?}",
        doomed.team, doomed.class
    );
}

fn observe_death(mut seen: ResMut<Seen>, combatants: Query<&Combatant>) {
    seen.frame += 1;
    if seen.died_at.is_some() {
        return;
    }
    if let Some(unit) = seen.unit {
        if combatants.get(unit).is_ok_and(|c| !c.is_alive()) {
            seen.died_at = Some(seen.frame);
        }
    }
}

/// Play `cfg` until the doomed unit has been dead [`AFTER_FRAMES`] frames,
/// calling `each` with the world after every frame. Panics (the AS-507
/// symptom) if a command flush meets a rig despawned under it.
fn play_through_death(cfg: &str, doomed: Doomed, mut each: impl FnMut(&mut World)) -> Seen {
    let mut app = boot(cfg, Some(doomed));
    for _ in 0..MAX_FRAMES {
        app.update();
        each(app.world_mut());
        let seen = app.world().resource::<Seen>();
        if seen.died_at.is_some_and(|d| seen.frame >= d + AFTER_FRAMES) {
            break;
        }
        if app.world().resource::<State<GameState>>().get() != &GameState::PlayMatch {
            break;
        }
    }
    std::mem::take(&mut *app.world_mut().resource_mut::<Seen>())
}

/// The card's repro: a Shadowform Priest beside a Warrior, `TeamPlan`, on
/// TwinPillars.
const SHADOWFORM_REPRO: &str = r#"{"team1":["Priest","Warrior"],"team2":["Priest","Rogue"],
    "team1_builds":[{"ImprovedMindBlast":2,"ImprovedPsychicScream":1,"Shadowform":1,
                     "VampiricEmbrace":1,"Silence":1},{}],
    "team1_priest_forms":["Shadowform","NoForm"],"ai_profile":"TeamPlan",
    "map":"TwinPillars","random_seed":1}"#;

/// The Shadowform Priest dies with its shroud emitting, and the shroud goes
/// with it — without the death's despawn meeting a particle queued onto the
/// rig in the same flush.
#[test]
fn a_shadowform_priest_dies_with_its_shroud_emitting() {
    // Not vacuous: the shroud is up and has live particles under it on the
    // frame before the death is seen.
    let mut shroud_particles_before = Vec::new();
    let seen = play_through_death(
        SHADOWFORM_REPRO,
        Doomed {
            team: 1,
            class: CharacterClass::Priest,
            after_secs: 8.0,
        },
        |world| {
            let mut rigs = world.query::<(&ShadowformShroud, Option<&Children>)>();
            let mut parts = world.query_filtered::<(), With<ClientParticle>>();
            let n: usize = rigs
                .iter(world)
                .flat_map(|(_, kids)| kids.into_iter().flat_map(|k| k.iter()))
                .filter(|&k| parts.get(world, k).is_ok())
                .count();
            shroud_particles_before.push(n);
        },
    );
    let died = seen
        .died_at
        .expect("the staged Shadowform Priest never died");
    assert!(
        shroud_particles_before[died - 2] > 0,
        "no shroud particles were alive on the frame before the death"
    );
    assert_eq!(
        shroud_particles_before.last().copied(),
        Some(0),
        "the dead Priest's shroud outlived it"
    );
}

/// A transform puff that ends on the frame its cloud is attached: the lobes
/// are parented onto the puff by `spawn_transform_puff_visuals` while
/// `cleanup_expired_transform_puffs` despawns it.
///
/// No puff reaches this today. `spawn_transform_puff` always gives the full
/// lifetime, and only `update_transform_puffs` lowers it, once the attach
/// has given the puff its material, so a hitch spends a puff only after its
/// lobes are on. A born-spent puff is staged here to pin the group's order
/// against a future path that lowers the lifetime before the attach.
#[test]
fn a_transform_puff_spent_on_its_first_frame_goes_cleanly() {
    #[derive(Resource, Default)]
    struct Puffed(bool);
    fn stage_spent_puff(
        mut commands: Commands,
        mut puffed: ResMut<Puffed>,
        countdown: Option<Res<MatchCountdown>>,
    ) {
        if puffed.0 || !countdown.is_some_and(|c| c.gates_opened) {
            return;
        }
        commands.spawn((
            TransformPuff {
                position: Vec3::Y,
                lifetime: 0.0,
                initial_lifetime: 0.45,
            },
            PlayMatchEntity,
        ));
        puffed.0 = true;
    }
    let mut app = boot(
        r#"{"team1":["Mage"],"team2":["Warrior"],"map":"BasicArena","random_seed":1}"#,
        None,
    );
    app.init_resource::<Puffed>()
        .add_systems(PreUpdate, stage_spent_puff);
    let mut gone_after = None;
    for frame in 0..60 * 20 {
        app.update();
        let puffs = app
            .world_mut()
            .query::<&TransformPuff>()
            .iter(app.world())
            .count();
        if app.world().resource::<Puffed>().0 && puffs == 0 {
            gone_after = Some(frame);
            break;
        }
    }
    assert!(
        gone_after.is_some(),
        "the staged puff never appeared and went"
    );
}
