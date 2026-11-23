//! Restarting an Animation Sandbox entry mid-effect sweeps the last pass's
//! rigs without meeting a particle an effect system parents onto one of them
//! in the same command flush.
//!
//! ## Why this exists
//!
//! AS-507: a restart (a loop's, or the panel's) clears the last pass's
//! leftovers, every `PlayMatchEntity` the staged units do not carry. That
//! includes the effect rigs a pass leaves: a DoT's state rig, a burst still
//! emitting. The sweep ran in `drive_playback`, unordered against the effect
//! systems, so its despawn could flush ahead of an emitter's `add_child` onto
//! the same rig, and the client panicked (a Warlock DoT entry did, through
//! `animate_dot_apply_bursts`). The sweep now runs in `Last`
//! (`sweep_sandbox_leftovers`). Only the real `StatesPlugin` schedule decides
//! which flush comes first, so this boots it, as `animation_sandbox_boot.rs`
//! does, and restarts every entry of a class mid-effect.
//!
//! Talent abilities are restarted too (AS-509): each is staged on a caster
//! with the build that grants it, and leaving one restages the caster
//! through the same sweep.

use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::CombatPlugin;
use arenasim::states::animation_sandbox::playback::{
    entries_for_class, SandboxEntry, SandboxPlayback,
};
use arenasim::states::animation_sandbox::{SandboxConfig, SandboxEntity, SandboxStage};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{Combatant, PlayMatchEntity};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{AbilityConfigPlugin, MapConfigPlugin, MovementConfigPlugin};
use arenasim::states::{GameState, StatesPlugin};

/// Frames played into a pass before it is restarted: once early, while a cast
/// or its landing is in flight, and once later, while an applied state emits.
const RESTART_AT: [usize; 2] = [25, 70];

/// The `animation_sandbox_boot.rs` app: everything `StatesPlugin` needs,
/// minus the window and renderer.
fn boot_app() -> App {
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

/// What a restart is about to sweep: every leftover of the last pass, and
/// how many of them are rigs with children, which an emitter may still be
/// parenting onto this frame.
fn leftovers(app: &mut App) -> (Vec<Entity>, usize) {
    let mut q = app
        .world_mut()
        .query_filtered::<(Entity, Option<&Children>), (With<PlayMatchEntity>, Without<SandboxEntity>)>();
    let all: Vec<(Entity, bool)> = q
        .iter(app.world())
        .map(|(e, kids)| (e, kids.is_some_and(|k| !k.is_empty())))
        .collect();
    let rigs = all.iter().filter(|(_, has)| *has).count();
    (all.into_iter().map(|(e, _)| e).collect(), rigs)
}

/// Restart every previewable entry of `class` mid-effect, twice. Panics (the
/// AS-507 symptom) if a sweep meets a particle queued onto a swept rig.
/// Returns how many restarts swept a rig with children.
fn restart_every_entry(class: CharacterClass) -> usize {
    let mut app = boot_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        16,
    )));
    app.update();
    app.insert_resource(SandboxConfig {
        caster_class: class,
        ..Default::default()
    });
    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::AnimationSandbox);
    for _ in 0..4 {
        app.update();
    }
    let entries = entries_for_class(class, app.world().resource::<AbilityDefinitions>());
    let mut swept_rigs = 0;
    for listing in entries {
        if !listing.family.is_playable() {
            continue;
        }
        app.world_mut()
            .resource_mut::<SandboxPlayback>()
            .select(listing.entry, listing.family);
        for frames in RESTART_AT {
            let (before, rigs) = leftovers(&mut app);
            if rigs > 0 {
                swept_rigs += 1;
            }
            app.world_mut()
                .resource_mut::<SandboxPlayback>()
                .restart_requested = true;
            app.update();
            // ...and the restart's frame swept every one of them.
            let left: Vec<Entity> = before
                .into_iter()
                .filter(|&e| app.world().get_entity(e).is_ok())
                .collect();
            assert!(
                left.is_empty(),
                "{}: the restart left {} of the last pass's entities",
                listing.label,
                left.len()
            );
            // The entry plays on a caster whose book holds it, unlocked (a
            // pet's ability is in its pet's book).
            let caster_ability = match listing.entry {
                SandboxEntry::Ability(ability) => {
                    let defs = app.world().resource::<AbilityDefinitions>();
                    defs.get(&ability).unwrap().pet.is_none().then_some(ability)
                }
                SandboxEntry::Body(_) => None,
            };
            if let Some(ability) = caster_ability {
                let caster = app.world().resource::<SandboxStage>().caster.unwrap();
                let book = app.world().get::<Combatant>(caster).unwrap().book();
                assert!(
                    book.contains(&ability) && !book.is_locked(ability),
                    "{}: the caster cannot cast it",
                    listing.label
                );
            }
            for _ in 1..frames {
                app.update();
            }
        }
        app.world_mut().resource_mut::<SandboxPlayback>().stop();
        for _ in 0..3 {
            app.update();
        }
    }
    swept_rigs
}

/// The old caster a switch restages, and whether it was still in the world
/// in `PostUpdate`: after every `Update` system had run.
#[derive(Resource, Default)]
struct OldCaster {
    entity: Option<Entity>,
    alive_after_update: bool,
}

fn watch_old_caster(mut watch: ResMut<OldCaster>, entities: Query<()>) {
    if let Some(old) = watch.entity {
        watch.alive_after_update = entities.get(old).is_ok();
    }
}

/// Switch from each entry of `class` to the next mid-effect, with no stop
/// between: a switch into or out of a talent entry restages the caster
/// (`restage_caster_for_entry`), and the caster it leaves, with every rig
/// it carries, goes through the sweep: still there for every `Update`
/// system, which may emit onto it that frame, and gone after `Last`.
/// Returns how many switches restaged the caster, and how many of those
/// left one with children.
fn switch_every_entry_mid_effect(class: CharacterClass) -> (usize, usize) {
    let mut app = boot_app();
    app.init_resource::<OldCaster>()
        .add_systems(PostUpdate, watch_old_caster);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        16,
    )));
    app.update();
    app.insert_resource(SandboxConfig {
        caster_class: class,
        ..Default::default()
    });
    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::AnimationSandbox);
    for _ in 0..4 {
        app.update();
    }
    let entries = entries_for_class(class, app.world().resource::<AbilityDefinitions>());
    let (mut restaged, mut with_children) = (0, 0);
    for listing in entries {
        let old = app.world().resource::<SandboxStage>().caster.unwrap();
        app.world_mut().resource_mut::<OldCaster>().entity = Some(old);
        let old_tree: Vec<Entity> = app
            .world()
            .get::<Children>(old)
            .map(|kids| kids.iter().collect())
            .unwrap_or_default();
        {
            let mut playback = app.world_mut().resource_mut::<SandboxPlayback>();
            playback.select(listing.entry, listing.family);
            playback.restart_requested = true;
        }
        app.update();
        if app.world().resource::<SandboxStage>().caster != Some(old) {
            restaged += 1;
            if !old_tree.is_empty() {
                with_children += 1;
            }
            assert!(
                app.world().resource::<OldCaster>().alive_after_update,
                "{}: the old caster was despawned before the sweep",
                listing.label
            );
            // The caster it left is gone, children and all.
            for e in std::iter::once(old).chain(old_tree) {
                assert!(
                    app.world().get_entity(e).is_err(),
                    "{}: the restage left the old caster's {e}",
                    listing.label
                );
            }
        }
        for _ in 0..40 {
            app.update();
        }
    }
    (restaged, with_children)
}

macro_rules! switch_mid_effect {
    ($($name:ident: $class:expr,)*) => {$(
        #[test]
        fn $name() {
            let (restaged, with_children) = switch_every_entry_mid_effect($class);
            // Not vacuous: the caster was restaged, children and all.
            assert!(restaged > 0 && with_children > 0, "{:?}: {restaged} restages, {with_children} with children", $class);
        }
    )*};
}

switch_mid_effect! {
    priest_entries_switch_mid_effect: CharacterClass::Priest,
    rogue_entries_switch_mid_effect: CharacterClass::Rogue,
    warlock_entries_switch_mid_effect: CharacterClass::Warlock,
    paladin_entries_switch_mid_effect: CharacterClass::Paladin,
}

/// The classes above are every class with a talent ability, the only ones
/// whose caster a switch restages.
#[test]
fn every_class_with_a_talent_ability_switches_mid_effect() {
    let defs = AbilityDefinitions::shipped();
    let with: std::collections::BTreeSet<String> = defs
        .talent_abilities()
        .iter()
        .map(|a| format!("{:?}", defs.get(a).unwrap().class))
        .collect();
    let tested: std::collections::BTreeSet<String> = ["Priest", "Rogue", "Warlock", "Paladin"]
        .map(String::from)
        .into_iter()
        .collect();
    assert_eq!(with, tested);
}

macro_rules! restart_mid_effect {
    ($($name:ident: $class:expr,)*) => {$(
        #[test]
        fn $name() {
            // Not vacuous: some restart swept a rig that had children.
            let swept = restart_every_entry($class);
            assert!(swept > 0, "no restart of a {:?} entry swept a rig with children", $class);
        }
    )*};
}

restart_mid_effect! {
    warrior_entries_restart_mid_effect: CharacterClass::Warrior,
    mage_entries_restart_mid_effect: CharacterClass::Mage,
    rogue_entries_restart_mid_effect: CharacterClass::Rogue,
    priest_entries_restart_mid_effect: CharacterClass::Priest,
    warlock_entries_restart_mid_effect: CharacterClass::Warlock,
    paladin_entries_restart_mid_effect: CharacterClass::Paladin,
    hunter_entries_restart_mid_effect: CharacterClass::Hunter,
    shaman_entries_restart_mid_effect: CharacterClass::Shaman,
    druid_entries_restart_mid_effect: CharacterClass::Druid,
}

/// The classes above are every class: a new one fails to compile here until
/// it has a test above.
#[allow(dead_code)]
fn every_class_has_a_test(class: CharacterClass) {
    match class {
        CharacterClass::Warrior
        | CharacterClass::Mage
        | CharacterClass::Rogue
        | CharacterClass::Priest
        | CharacterClass::Warlock
        | CharacterClass::Paladin
        | CharacterClass::Hunter
        | CharacterClass::Shaman
        | CharacterClass::Druid => {}
    }
}
