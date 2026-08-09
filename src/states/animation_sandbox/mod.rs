//! Animation Sandbox — play combat animations on demand on an inert caster.
//!
//! Seeing a combat animation used to mean engineering a match that produced it:
//! the class had to be in the comp, the AI had to choose the ability, and the
//! game state had to satisfy its preconditions. `--replay` removed the
//! menu-hopping half of that cost; this state removes the rest.
//!
//! ## What makes a preview faithful
//!
//! The sandbox substitutes for the AI DECISION layer only. Everything from cast
//! resolution rightward — [`add_sandbox_combat_systems`] plus the shared
//! visual-effect layer gated by `in_combat_scene` in `src/states/mod.rs` — is
//! the same code a match runs. A hand-authored preview that could drift from
//! real behavior would be worse than no preview, because it would produce
//! confident wrong conclusions.
//!
//! ## What is deliberately absent
//!
//! No AI, no match clock, no arena bounds, no obstacles, and none of the match
//! chrome (HUD, team frames, combat log, speech bubbles, banter, gate bars,
//! selection). A match running underneath is itself a reason animations are
//! hard to judge, so the sandbox runs none of it.
//!
//! [`add_sandbox_combat_systems`]: crate::states::play_match::systems::add_sandbox_combat_systems

pub mod playback;
pub mod ui;

use bevy::prelude::*;
use std::collections::HashMap;

use super::match_config::{
    ArenaMap, CharacterClass, MageArmor, PaladinAura, RogueOpener, RoguePoison, WarriorShout,
};
use super::play_match::ai_profile::AiProfiles;
use super::play_match::components::{
    ArenaDampening, GameRng, MatchCountdown, ShadowSightState, SimulationSpeed,
};
use super::play_match::map_config::MapGeometryConfig;
use super::play_match::team_plan::TeamPlans;
use super::play_match::equipment::{
    enforce_two_hand_conflicts, resolve_loadout, DefaultLoadouts, ItemDefinitions,
};
use super::play_match::spawn_combatant;

/// Marks every entity the sandbox spawns, so teardown despawns exactly its own
/// scene. The sandbox does NOT reuse `PlayMatchEntity` for this: that marker is
/// cleared by `cleanup_play_match` on `OnExit(PlayMatch)`, which never fires
/// here.
#[derive(Component)]
pub struct SandboxEntity;

/// Distance from the stage centre to each combatant, in yards. Wide enough that
/// a projectile visibly travels, close enough that both units stay framed.
pub(crate) const STAGE_SEPARATION: f32 = 8.0;

/// Radius of the staging floor. Large enough that no ability's travel or AoE
/// visual runs off the edge at the framing distances the presets use.
const FLOOR_RADIUS: f32 = 60.0;

/// What the sandbox currently has staged.
///
/// Changing any field restages the scene ([`restage_on_config_change`]), so the
/// selection UI mutates this resource and nothing else.
#[derive(Resource)]
pub struct SandboxConfig {
    /// Class of the unit that performs the animation.
    pub caster_class: CharacterClass,
    /// Whether a target dummy is staged. Relational visuals (projectiles,
    /// beams, launch arcs, a swing landing) need one.
    pub dummy_enabled: bool,
    /// Class of the target dummy, when enabled.
    pub dummy_class: CharacterClass,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            // A Mage casts Frostbolt: a hard cast with a projectile, so the
            // default staging exercises the cast orb, the projectile and the
            // impact without the user changing anything.
            caster_class: CharacterClass::Mage,
            dummy_enabled: true,
            dummy_class: CharacterClass::Warrior,
        }
    }
}

/// Handles to the staged units, so playback and camera framing can find them
/// without a query that might also match leftover entities.
#[derive(Resource, Default)]
pub struct SandboxStage {
    pub caster: Option<Entity>,
    pub dummy: Option<Entity>,
    /// Where the caster was staged. The walk-bob entry drives the caster around
    /// the floor (the bob keys off real movement), so its position has to be
    /// restorable — otherwise the next entry plays off-centre and the camera
    /// presets frame empty floor.
    pub caster_home: Vec3,
}

/// Builds the staging scene: floor, light, camera, caster, optional dummy.
///
/// No `ArenaBounds` and no `ObstacleVolume` are inserted. Every entry in
/// `assets/config/maps.ron` carries both, and the sandbox wants neither — a
/// bare plane satisfies "no obstacles" by construction rather than by
/// configuring them away.
pub fn setup_sandbox(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    config: Res<SandboxConfig>,
    item_defs: Res<ItemDefinitions>,
    default_loadouts: Res<DefaultLoadouts>,
    map_geometry: Res<MapGeometryConfig>,
    mut stage: ResMut<SandboxStage>,
) {
    // The resolution systems this state shares with a match read these. They are
    // normally inserted by `setup_play_match`, which never runs here.
    //
    // `MatchCountdown` is inserted with the gates ALREADY OPEN: much of the
    // combat layer no-ops before the gates open, and the sandbox has no
    // countdown to wait through.
    commands.insert_resource(MatchCountdown {
        time_remaining: 0.0,
        gates_opened: true,
    });
    commands.insert_resource(ArenaDampening::default());
    commands.insert_resource(ShadowSightState::default());
    commands.insert_resource(SimulationSpeed { multiplier: 1.0 });
    commands.insert_resource(GameRng::default());
    commands.insert_resource(TeamPlans::default());
    commands.insert_resource(AiProfiles::default());
    commands.insert_resource(map_geometry.active_for(ArenaMap::BasicArena));

    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 220.0,
        ..default()
    });

    commands.spawn((
        DirectionalLight {
            illuminance: 9_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(20.0, 40.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
        SandboxEntity,
    ));

    commands.spawn((
        Mesh3d(meshes.add(Circle::new(FLOOR_RADIUS))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.20, 0.21, 0.24),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        SandboxEntity,
    ));

    stage_units(
        &mut commands,
        &mut meshes,
        &mut materials,
        &asset_server,
        &config,
        &item_defs,
        &default_loadouts,
        &mut stage,
    );

    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 6.0, 18.0).looking_at(Vec3::new(0.0, 1.2, 0.0), Vec3::Y),
        SandboxEntity,
    ));
}

/// Spawns the caster and, when enabled, the dummy. Split out of
/// [`setup_sandbox`] so a config change can restage without rebuilding the
/// floor, light and camera.
#[allow(clippy::too_many_arguments)]
fn stage_units(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    asset_server: &AssetServer,
    config: &SandboxConfig,
    item_defs: &ItemDefinitions,
    default_loadouts: &DefaultLoadouts,
    stage: &mut SandboxStage,
) {
    let caster_home = Vec3::new(-STAGE_SEPARATION, 1.0, 0.0);
    let caster = spawn_staged_unit(
        commands,
        meshes,
        materials,
        asset_server,
        1,
        config.caster_class,
        caster_home,
        item_defs,
        default_loadouts,
    );
    stage.caster = Some(caster);
    stage.caster_home = caster_home;

    stage.dummy = config.dummy_enabled.then(|| {
        spawn_staged_unit(
            commands,
            meshes,
            materials,
            asset_server,
            2,
            config.dummy_class,
            Vec3::new(STAGE_SEPARATION, 1.0, 0.0),
            item_defs,
            default_loadouts,
        )
    });
}

/// Spawns one combatant through the match's own spawn path, then tags it as
/// sandbox-owned.
#[allow(clippy::too_many_arguments)]
fn spawn_staged_unit(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    asset_server: &AssetServer,
    team: u8,
    class: CharacterClass,
    position: Vec3,
    item_defs: &ItemDefinitions,
    default_loadouts: &DefaultLoadouts,
) -> Entity {
    let mut loadout: HashMap<_, _> = resolve_loadout(class, default_loadouts, &HashMap::new());
    enforce_two_hand_conflicts(&mut loadout, item_defs);

    let (entity, _combatant) = spawn_combatant(
        commands,
        meshes,
        materials,
        asset_server,
        team,
        0,
        class,
        position,
        0,
        RogueOpener::default(),
        RoguePoison::default(),
        Vec::new(),
        WarriorShout::default(),
        MageArmor::default(),
        PaladinAura::default(),
        &loadout,
        item_defs,
    );
    commands.entity(entity).insert(SandboxEntity);
    entity
}

/// Restages the units when the selection UI changes caster or dummy.
///
/// Bevy's `is_changed()` fires on the first run after insert, which would
/// restage a scene `setup_sandbox` just built. The guard is the stage handles:
/// a freshly-built stage already matches the config, so restaging only runs
/// when the resource changes AFTER setup.
pub fn restage_on_config_change(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    config: Res<SandboxConfig>,
    item_defs: Res<ItemDefinitions>,
    default_loadouts: Res<DefaultLoadouts>,
    mut stage: ResMut<SandboxStage>,
) {
    if !config.is_changed() || config.is_added() {
        return;
    }

    for entity in stage.caster.take().into_iter().chain(stage.dummy.take()) {
        if let Ok(mut e) = commands.get_entity(entity) {
            e.despawn();
        }
    }

    stage_units(
        &mut commands,
        &mut meshes,
        &mut materials,
        &asset_server,
        &config,
        &item_defs,
        &default_loadouts,
        &mut stage,
    );
}

/// Tears down everything the sandbox spawned.
pub fn cleanup_sandbox(
    mut commands: Commands,
    entities: Query<Entity, With<SandboxEntity>>,
    mut stage: ResMut<SandboxStage>,
) {
    for entity in entities.iter() {
        commands.entity(entity).despawn();
    }
    *stage = SandboxStage::default();
    commands.remove_resource::<AmbientLight>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_staging_is_a_hard_caster_against_a_dummy() {
        // The default must exercise a cast orb, a projectile and an impact
        // without the user touching the selection UI. A Mage with a dummy does;
        // a Warrior (no hard casts) or a dummy-less stage would not.
        let config = SandboxConfig::default();
        assert_eq!(config.caster_class, CharacterClass::Mage);
        assert!(config.dummy_enabled);
    }

    #[test]
    fn units_are_separated_enough_for_travel_to_read() {
        // A projectile that spawns already touching its target shows no travel,
        // which is most of what a projectile animation is.
        assert!(STAGE_SEPARATION >= 4.0);
        assert!(STAGE_SEPARATION * 2.0 < FLOOR_RADIUS);
    }
}
