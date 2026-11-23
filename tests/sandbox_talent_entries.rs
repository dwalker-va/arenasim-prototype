//! Every Animation Sandbox entry plays, talent abilities included.
//!
//! ## Why this exists
//!
//! AS-509: the sandbox lists every ability of a class, talent abilities
//! among them, but staged its caster untalented. A talent ability is in a
//! book only when a build grants it, so playing one (Expose Armor,
//! Shadowform, Guardian Spirit, ...) panicked in `process_casting`: "not in
//! the ability book". The caster is now staged with the build the selected
//! entry needs (`staged_build`): the smallest legal build holding the one
//! node that grants it, with the per-match choices its `Requires` rules ask
//! for, resolved through the spawn path a match uses.
//!
//! The list is checked by set equality against the ability table, and each
//! talent entry is played through the real `StatesPlugin` schedule, start
//! to finish, on a caster that holds it unlocked.

use std::collections::BTreeSet;
use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::log::CombatLog;
use arenasim::combat::CombatPlugin;
use arenasim::states::animation_sandbox::playback::{
    entries_for_class, entry_duration, SandboxEntry, SandboxPlayback, LOOP_TAIL_SECS,
};
use arenasim::states::animation_sandbox::{staged_build, SandboxConfig, SandboxStage, StagedBuild};
use arenasim::states::match_config::{CharacterClass, MatchConfig};
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{ActiveAuras, Combatant};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::talent_choices::{ChoiceRule, SlotChoices};
use arenasim::states::play_match::talents::{Build, TalentDefinitions, TalentEffect};
use arenasim::states::play_match::{AbilityConfigPlugin, MapConfigPlugin, MovementConfigPlugin};
use arenasim::states::{GameState, StatesPlugin};

/// Every ability a class's entry list offers, across every class.
fn listed_abilities(abilities: &AbilityDefinitions) -> BTreeSet<AbilityType> {
    CharacterClass::all()
        .iter()
        .flat_map(|class| entries_for_class(*class, abilities))
        .filter_map(|listing| match listing.entry {
            SandboxEntry::Ability(ability) => Some(ability),
            SandboxEntry::Body(_) => None,
        })
        .collect()
}

#[test]
fn the_sandbox_lists_every_ability_in_the_table() {
    let abilities = AbilityDefinitions::shipped();
    let table: BTreeSet<AbilityType> = abilities.iter().map(|(a, _)| *a).collect();
    assert_eq!(listed_abilities(abilities), table);
    // Not vacuous: the talent abilities are among them.
    let talent: BTreeSet<AbilityType> = abilities.talent_abilities().into_iter().collect();
    assert!(!talent.is_empty());
    assert!(talent.is_subset(&table));
}

/// The choices a default slot of `class` makes.
fn default_choices(class: CharacterClass) -> SlotChoices {
    SlotChoices::from_config(&MatchConfig::default(), 1, 0, class)
}

/// A talent entry stages a caster whose book holds the ability, unlocked;
/// the build is the smallest legal one holding the node that grants it; and
/// only an ability a `Requires` rule names changes the slot's choices.
#[test]
fn every_talent_entry_stages_a_caster_that_can_cast_it() {
    let abilities = AbilityDefinitions::shipped();
    let talents = TalentDefinitions::shipped();
    let talent_abilities = abilities.talent_abilities();
    let mut with_a_required_choice = BTreeSet::new();
    for ability in &talent_abilities {
        let class = abilities.get(ability).unwrap().class;
        let staged = staged_build(
            talents,
            abilities,
            class,
            Some(SandboxEntry::Ability(*ability)),
        )
        .unwrap_or_else(|e| panic!("{ability:?}: {e}"));

        // The node that grants it, and only it, is the build's deepest.
        let granting: Vec<_> = talents
            .iter()
            .filter(|(_, t)| t.per_rank.contains(&TalentEffect::Grant(*ability)))
            .map(|(id, _)| id)
            .collect();
        assert_eq!(granting.len(), 1, "{ability:?} is granted by {granting:?}");
        let node = granting[0];
        assert!(
            staged.build.rank(node) >= 1,
            "{ability:?}: the staged build {:?} does not hold {node:?}",
            staged.build
        );

        // Resolved as a spawn resolves it, the book holds it unlocked.
        let resolved = talents
            .resolve_with_choices(abilities, class, &staged.build, &staged.choices)
            .unwrap_or_else(|e| panic!("{ability:?}: the staged build is illegal: {e}"));
        assert!(
            resolved.book().contains(ability),
            "{ability:?}: not in the staged caster's book"
        );
        assert!(
            !resolved.book().is_locked(*ability),
            "{ability:?}: the staged caster's choices lock it"
        );

        // Smallest: no legal build one point smaller holds the node.
        let points = staged.build.points();
        let smaller: Vec<Build> = talents
            .legal_builds(class, points - 1)
            .into_iter()
            .filter(|b| b.rank(node) >= 1)
            .collect();
        assert!(
            smaller.is_empty(),
            "{ability:?}: {:?} holds {node:?} in {} points, fewer than the staged {points}",
            smaller[0],
            points - 1
        );

        // The choices move off the default slot's only where a rule asks.
        let required = talents
            .choice_rules()
            .iter()
            .any(|r| matches!(r, ChoiceRule::Requires { ability: a, .. } if a == ability));
        if required {
            with_a_required_choice.insert(*ability);
            assert_ne!(staged.choices, default_choices(class), "{ability:?}");
            // A required option takes its menu's place: the slot still
            // makes one choice on each menu the default slot does.
            let menus = |c: &SlotChoices| c.options().map(|o| o.menu()).collect::<Vec<_>>();
            assert_eq!(
                menus(&staged.choices),
                menus(&default_choices(class)),
                "{ability:?}: {:?}",
                staged.choices
            );
        } else {
            assert_eq!(staged.choices, default_choices(class), "{ability:?}");
        }
    }
    // Not vacuous: some talent ability needs a choice made (Shadowform and
    // Silence need the Shadowform form).
    assert!(
        with_a_required_choice.contains(&AbilityType::Shadowform),
        "{with_a_required_choice:?}"
    );
}

/// Every other entry stages the untalented caster of a default slot.
#[test]
fn every_other_entry_stages_the_untalented_caster() {
    let abilities = AbilityDefinitions::shipped();
    let talents = TalentDefinitions::shipped();
    for class in CharacterClass::all() {
        let untalented = StagedBuild {
            build: Build::default(),
            choices: default_choices(*class),
        };
        assert_eq!(
            staged_build(talents, abilities, *class, None).unwrap(),
            untalented
        );
        for listing in entries_for_class(*class, abilities) {
            if let SandboxEntry::Ability(ability) = listing.entry {
                if abilities.get(&ability).unwrap().talent {
                    continue;
                }
            }
            assert_eq!(
                staged_build(talents, abilities, *class, Some(listing.entry)).unwrap(),
                untalented,
                "{}",
                listing.label
            );
        }
    }
}

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

/// Whether an aura `ability` applied is on the caster or the dummy.
fn aura_on_stage(app: &App, name: &str) -> bool {
    let stage = app.world().resource::<SandboxStage>();
    stage
        .caster
        .into_iter()
        .chain(stage.dummy)
        .filter_map(|unit| app.world().get::<ActiveAuras>(unit))
        .any(|active| active.auras.iter().any(|a| a.ability_name == name))
}

/// Plays every talent entry of `class` start to finish through the real
/// schedule. Each must land: the combat log names it, its cooldown starts,
/// or an aura it applied shows on a staged unit during the pass. Panics
/// (the AS-509 symptom) if a cast is not in the caster's book.
fn play_every_talent_entry(class: CharacterClass) {
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
    let abilities = app.world().resource::<AbilityDefinitions>().clone();
    let mut played = BTreeSet::new();
    for listing in entries_for_class(class, &abilities) {
        let SandboxEntry::Ability(ability) = listing.entry else {
            continue;
        };
        if !abilities.get(&ability).unwrap().talent {
            continue;
        }
        played.insert(ability);
        let log_start = app.world().resource::<CombatLog>().entries.len();
        {
            let mut playback = app.world_mut().resource_mut::<SandboxPlayback>();
            playback.select(listing.entry, listing.family);
            playback.restart_requested = true;
        }
        app.update();

        // The entry started, on a caster that holds the ability unlocked.
        let caster = app.world().resource::<SandboxStage>().caster.unwrap();
        let book = app.world().get::<Combatant>(caster).unwrap().book().clone();
        assert!(
            book.contains(&ability),
            "{}: not in the caster's book",
            listing.label
        );
        assert!(
            !book.is_locked(ability),
            "{}: locked for the caster",
            listing.label
        );
        let playback = app.world().resource::<SandboxPlayback>();
        assert!(playback.playing, "{}: did not start", listing.label);
        let pass = entry_duration(playback, &abilities) + LOOP_TAIL_SECS;

        // Play the whole pass out.
        let name = abilities.get(&ability).unwrap().name.clone();
        let frames = (pass / 0.016).ceil() as usize + 4;
        let mut aura_seen = false;
        for _ in 0..frames {
            app.update();
            aura_seen |= aura_on_stage(&app, &name);
        }
        assert!(
            !app.world().resource::<SandboxPlayback>().playing,
            "{}: the pass did not end",
            listing.label
        );
        let logged = app.world().resource::<CombatLog>().entries[log_start..]
            .iter()
            .any(|e| e.message.contains(name.as_str()));
        let on_cooldown = app
            .world()
            .get::<Combatant>(caster)
            .unwrap()
            .ability_cooldowns
            .contains_key(&ability);
        assert!(
            logged || on_cooldown || aura_seen,
            "{}: the pass ended with no trace of the cast landing",
            listing.label
        );
        for _ in 0..3 {
            app.update();
        }
    }
    // Every talent ability of the class was played.
    let expected: BTreeSet<AbilityType> = abilities
        .talent_abilities()
        .into_iter()
        .filter(|a| abilities.get(a).unwrap().class == class)
        .collect();
    assert!(!expected.is_empty(), "{class:?} has no talent ability");
    assert_eq!(played, expected);
}

macro_rules! talent_entries_play {
    ($($name:ident: $class:expr,)*) => {$(
        #[test]
        fn $name() {
            play_every_talent_entry($class);
        }
    )*};
}

talent_entries_play! {
    priest_talent_entries_play: CharacterClass::Priest,
    rogue_talent_entries_play: CharacterClass::Rogue,
    warlock_talent_entries_play: CharacterClass::Warlock,
    paladin_talent_entries_play: CharacterClass::Paladin,
}

/// The classes above are every class with a talent ability: one gaining a
/// talent ability fails here until it has a test above.
#[test]
fn every_class_with_a_talent_ability_has_a_test() {
    let abilities = AbilityDefinitions::shipped();
    let with: BTreeSet<_> = abilities
        .talent_abilities()
        .iter()
        .map(|a| format!("{:?}", abilities.get(a).unwrap().class))
        .collect();
    let tested: BTreeSet<_> = ["Priest", "Rogue", "Warlock", "Paladin"]
        .map(String::from)
        .into_iter()
        .collect();
    assert_eq!(with, tested);
}
