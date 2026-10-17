//! Proves the Animation Sandbox state can actually be ENTERED.
//!
//! ## Why this exists
//!
//! Bevy validates a system's parameters at run time, per state. A system gated
//! on `GameState::AnimationSandbox` that reads a resource only
//! `setup_play_match` inserts compiles cleanly, passes every unit test, and
//! renders perfectly in the kittest snapshot — then panics the instant the
//! state is entered. That has now happened twice on this feature:
//!
//! - `Res<SpellIcons>` was read by the panel but only inserted by
//!   `setup_play_match`, so opening the sandbox died on parameter validation.
//! - Registering the resolution systems a second time made their
//!   `SystemTypeSet`s ambiguous and Bevy rejected the `spawn_projectile_visuals`
//!   ordering, killing the app at schedule construction.
//!
//! Neither is reachable from a pure-function test. Both are caught here, because
//! this builds the real `StatesPlugin`, enters the state, and ticks it.
//!
//! ## What it does and does not cover
//!
//! It covers schedule construction and parameter validation for every system
//! that runs in this state — the "does it open" question. It says nothing about
//! whether anything LOOKS right; `tests/animation_sandbox_snapshot.rs` covers
//! layout, and only a human covers the animations themselves.

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;

use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{AbilityConfigPlugin, MapConfigPlugin, MovementConfigPlugin};
use arenasim::states::{GameState, StatesPlugin};

/// Builds the app with everything `StatesPlugin` needs, minus the window and
/// renderer.
///
/// Asset collections are registered directly rather than by pulling in
/// `RenderPlugin`/`PbrPlugin`, which would need a GPU adapter and put this test
/// out of CI's reach — the whole point is that it runs in the default
/// `cargo test`.
fn boot_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .add_plugins(BevyStatesPlugin)
        // Camera input reads keyboard/mouse state.
        .add_plugins(bevy::input::InputPlugin)
        // Window entities are queried by camera and picking systems.
        .add_plugins(bevy::window::WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        })
        // `main.rs` owns this, not `StatesPlugin`.
        .init_state::<GameState>()
        // `EguiContexts` validates against this resource. Registering it alone
        // keeps the egui-drawing systems runnable without pulling in the
        // renderer: they find no `EguiContext` component, so `try_ctx_mut()`
        // returns `None` and each returns early — AFTER its other parameters
        // have been validated, which is exactly the check this test is for.
        .init_resource::<bevy_egui::EguiUserTextures>()
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>()
        .init_asset::<Shader>()
        // `spawn_combatant` asks the asset server for weapon glTF scenes. The
        // handles are never resolved here (no loaders), but the asset types
        // must be registered or handle allocation panics.
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
            // Owns the `Keybindings` resource the camera input reads.
            arenasim::settings::SettingsPlugin,
            StatesPlugin,
        ));
    app
}

/// Enter the sandbox and tick it. Any system whose parameters cannot be
/// satisfied in this state panics here.
#[test]
fn the_animation_sandbox_state_can_be_entered() {
    let mut app = boot_app();

    // One update in the default state first, so schedule construction and the
    // state transition are exercised separately — a failure then localises to
    // one or the other.
    app.update();

    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::AnimationSandbox);

    // Several ticks: OnEnter runs on the first, and the Update systems that read
    // what OnEnter inserted run on the ones after.
    for _ in 0..4 {
        app.update();
    }

    assert_eq!(
        app.world().resource::<State<GameState>>().get(),
        &GameState::AnimationSandbox,
        "the sandbox state was left before it could be ticked"
    );
}

/// Leaving the sandbox must not strand resources that would change the next
/// match. `cleanup_sandbox` mirrors `cleanup_play_match`'s removals; this pins
/// the one with teeth.
///
/// `setup_play_match` treats a surviving seeded `GameRng` as "a replay
/// pre-seeded this match" and honours it, so a leak here silently fixes the
/// seed of every match played after visiting the sandbox.
#[test]
fn leaving_the_sandbox_does_not_strand_the_rng() {
    use arenasim::states::play_match::GameRng;

    let mut app = boot_app();
    app.update();

    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::AnimationSandbox);
    for _ in 0..4 {
        app.update();
    }
    assert!(
        app.world().get_resource::<GameRng>().is_some(),
        "the sandbox should have inserted a GameRng for the resolution systems"
    );

    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::MainMenu);
    for _ in 0..2 {
        app.update();
    }

    assert!(
        app.world().get_resource::<GameRng>().is_none(),
        "GameRng survived the sandbox; setup_play_match would honour it as a \
         replay pre-seed and the next match's seed would be fixed by having \
         opened this screen"
    );
}

/// Every dispel entry must actually STRIP something from the staged dummy, or
/// it previews nothing at all — the ribbon and beat are spawned only on a
/// successful dispel.
///
/// This was silently broken: the sandbox staged Arcane Intellect, which
/// `Aura::can_be_dispelled` rejects (it matches magic DEBUFFS only), and fired
/// Purge with no filter where the real Shaman pins one. Every dispel entry
/// resolved, removed nothing, and drew nothing, and no test noticed because
/// the boot test only asked whether the state could be entered. This drives
/// each entry the way the panel does and asks for the ribbon.
#[test]
fn every_dispel_entry_strips_something_in_the_sandbox() {
    use arenasim::states::animation_sandbox::playback::{
        EntryFamily, SandboxEntry, SandboxPlayback,
    };
    use arenasim::states::animation_sandbox::SandboxConfig;
    use arenasim::states::match_config::CharacterClass;
    use arenasim::states::play_match::abilities::AbilityType;
    use arenasim::states::play_match::components::DispelRibbon;
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    for (ability, family, class) in [
        (
            AbilityType::DispelMagic,
            EntryFamily::Component,
            CharacterClass::Priest,
        ),
        (
            AbilityType::PaladinCleanse,
            EntryFamily::Component,
            CharacterClass::Paladin,
        ),
        (
            AbilityType::Purge,
            EntryFamily::Component,
            CharacterClass::Shaman,
        ),
        (
            AbilityType::DevourMagic,
            EntryFamily::Entity,
            CharacterClass::Warlock,
        ),
    ] {
        let mut app = boot_app();
        // The resolution systems run in FixedUpdate; a tight test loop advances
        // real time by microseconds, so drive the clock by hand.
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
        {
            let mut playback = app.world_mut().resource_mut::<SandboxPlayback>();
            playback.select(SandboxEntry::Ability(ability), family);
            playback.restart_requested = true;
        }
        for _ in 0..40 {
            app.update();
        }
        let mut ribbons = app.world_mut().query::<&DispelRibbon>();
        assert!(
            ribbons.iter(app.world()).count() >= 1,
            "{ability:?} ({class:?}) stripped nothing from the staged dummy — \
             nothing to preview"
        );
    }
}

/// Every interrupt entry must CUT a cast and land its mark on the dummy, and
/// Heroic Strike must swing its empowered blow — driven the way the panel
/// drives them.
///
/// An interrupt previews nothing against a unit that is not casting, and a
/// match queues Kick, Pummel and Wind Shear from `combat_ai.rs`, which the
/// sandbox does not run. So the entry stages a cast on the dummy and fires the
/// interrupt into it through the match's own `process_interrupts`; this asks
/// for the landing that path spawns, on the dummy, and for the cast it cut.
#[test]
fn every_interrupt_entry_cuts_a_cast_and_heroic_strike_swings_its_blow() {
    use arenasim::states::animation_sandbox::playback::{
        EntryFamily, SandboxEntry, SandboxPlayback,
    };
    use arenasim::states::animation_sandbox::{SandboxConfig, SandboxStage};
    use arenasim::states::match_config::CharacterClass;
    use arenasim::states::play_match::abilities::AbilityType;
    use arenasim::states::play_match::components::{CastingState, SchoolImpact};
    use arenasim::states::play_match::WeaponTrail;
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    for (ability, family, class) in [
        (
            AbilityType::Kick,
            EntryFamily::Residue,
            CharacterClass::Rogue,
        ),
        (
            AbilityType::Pummel,
            EntryFamily::Residue,
            CharacterClass::Warrior,
        ),
        (
            AbilityType::WindShear,
            EntryFamily::Residue,
            CharacterClass::Shaman,
        ),
        (
            AbilityType::SpellLock,
            EntryFamily::Entity,
            CharacterClass::Warlock,
        ),
        (
            AbilityType::HeroicStrike,
            EntryFamily::Component,
            CharacterClass::Warrior,
        ),
    ] {
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
        {
            let mut playback = app.world_mut().resource_mut::<SandboxPlayback>();
            playback.select(SandboxEntry::Ability(ability), family);
            playback.restart_requested = true;
        }
        let dummy = app
            .world()
            .resource::<SandboxStage>()
            .dummy
            .expect("the default staging has a dummy");

        // Before the lead the dummy is mid-cast and nothing has landed.
        for _ in 0..30 {
            app.update();
        }
        if ability != AbilityType::HeroicStrike {
            let cast = app.world().get::<CastingState>(dummy);
            assert!(
                cast.is_some_and(|c| !c.interrupted),
                "{ability:?}: the dummy should be casting before the interrupt"
            );
        }

        // Past the 1.0s lead (and, for Heroic Strike, the blade's arrival).
        let mut landed = false;
        let mut cut = false;
        let mut trail = false;
        for _ in 0..60 {
            app.update();
            let mut impacts = app.world_mut().query::<&SchoolImpact>();
            landed |= impacts
                .iter(app.world())
                .any(|i| i.ability == ability && i.target == dummy);
            cut |= app
                .world()
                .get::<CastingState>(dummy)
                .is_none_or(|c| c.interrupted);
            let mut trails = app.world_mut().query::<&WeaponTrail>();
            trail |= trails.iter(app.world()).count() > 0;
        }
        assert!(
            landed,
            "{ability:?} ({class:?}) landed nothing on the dummy"
        );
        if ability == AbilityType::HeroicStrike {
            assert!(trail, "Heroic Strike laid no trail");
        } else {
            assert!(cut, "{ability:?} did not cut the dummy's cast");
        }
    }
}

/// Ice Barrier previews through the real registration: the entry lands the
/// absorb on the Mage itself, which wears the client's five cards (placed, not
/// just spawned) and sheds its frost mist, and the Mage's two hands flash the
/// Frost launch exactly once (the zero-length cast and the marker both land,
/// and only the marker may draw the hands).
#[test]
fn ice_barrier_previews_its_cards_mist_and_both_hands() {
    use arenasim::states::animation_sandbox::playback::{
        EntryFamily, SandboxEntry, SandboxPlayback,
    };
    use arenasim::states::animation_sandbox::SandboxConfig;
    use arenasim::states::match_config::CharacterClass;
    use arenasim::states::play_match::abilities::AbilityType;
    use arenasim::states::play_match::components::{
        ClientParticle, HealCastHand, HealCastKind, PwShieldCard, PwShieldVisual, ShieldKind,
    };
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    let mut app = boot_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        16,
    )));
    app.update();
    app.insert_resource(SandboxConfig {
        caster_class: CharacterClass::Mage,
        ..Default::default()
    });
    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::AnimationSandbox);
    for _ in 0..4 {
        app.update();
    }
    {
        let mut playback = app.world_mut().resource_mut::<SandboxPlayback>();
        playback.select(
            SandboxEntry::Ability(AbilityType::IceBarrier),
            EntryFamily::Residue,
        );
        playback.restart_requested = true;
    }
    let (mut most_hands, mut cards_seen, mut placed, mut mist) = (0, 0, false, 0);
    let mut caster = None;
    for _ in 0..60 {
        app.update();
        let mut hands = app.world_mut().query::<&HealCastHand>();
        let now: Vec<(HealCastKind, Entity)> = hands
            .iter(app.world())
            .map(|h| (h.kind, h.caster))
            .collect();
        assert!(
            now.iter().all(|(k, _)| *k == HealCastKind::Frost),
            "{now:?}"
        );
        if let Some((_, c)) = now.first() {
            caster = Some(*c);
        }
        most_hands = most_hands.max(now.len());
        let mut cards = app.world_mut().query::<(&PwShieldCard, &Transform)>();
        let scales: Vec<f32> = cards.iter(app.world()).map(|(_, t)| t.scale.y).collect();
        cards_seen = cards_seen.max(scales.len());
        placed |= scales.iter().any(|s| *s > 1.0);
        let mut puffs = app.world_mut().query::<&ClientParticle>();
        mist = mist.max(puffs.iter(app.world()).count());
    }
    assert_eq!(
        most_hands, 2,
        "the Frost launch must flash both hands, once"
    );
    assert_eq!(cards_seen, 5, "the Mage must wear the five cards");
    assert!(placed, "the cards were spawned but never placed");
    assert!(mist > 5, "the barrier shed no frost mist ({mist} puffs)");
    let mut rigs = app.world_mut().query::<&PwShieldVisual>();
    let rig = *rigs.iter(app.world()).next().expect("no barrier");
    assert_eq!(rig.kind, ShieldKind::IceBarrier);
    let bearer = rig.bearer;
    assert_eq!(
        Some(bearer),
        caster,
        "the barrier is not on the casting Mage"
    );
}

/// Power Word: Shield previews through the real registration: the entry lands
/// the absorb on the dummy, which wears the client's three cards (placed, not
/// just spawned), and the Priest's two hands flash the Holy launch exactly
/// once — the entry lands through a zero-length cast AND spawns the instant
/// marker, and only the marker may draw the hands.
#[test]
fn power_word_shield_previews_its_cards_and_both_hands() {
    use arenasim::states::animation_sandbox::playback::{
        EntryFamily, SandboxEntry, SandboxPlayback,
    };
    use arenasim::states::animation_sandbox::SandboxConfig;
    use arenasim::states::match_config::CharacterClass;
    use arenasim::states::play_match::abilities::AbilityType;
    use arenasim::states::play_match::components::{HealCastHand, HealCastKind, PwShieldCard};
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    let mut app = boot_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        16,
    )));
    app.update();
    app.insert_resource(SandboxConfig {
        caster_class: CharacterClass::Priest,
        ..Default::default()
    });
    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::AnimationSandbox);
    for _ in 0..4 {
        app.update();
    }
    {
        let mut playback = app.world_mut().resource_mut::<SandboxPlayback>();
        playback.select(
            SandboxEntry::Ability(AbilityType::PowerWordShield),
            EntryFamily::Residue,
        );
        playback.restart_requested = true;
    }
    let (mut most_hands, mut cards_seen, mut placed) = (0, 0, false);
    for _ in 0..60 {
        app.update();
        let mut hands = app.world_mut().query::<&HealCastHand>();
        let kinds: Vec<HealCastKind> = hands.iter(app.world()).map(|h| h.kind).collect();
        assert!(kinds.iter().all(|k| *k == HealCastKind::Holy), "{kinds:?}");
        most_hands = most_hands.max(kinds.len());
        let mut cards = app.world_mut().query::<(&PwShieldCard, &Transform)>();
        let now: Vec<f32> = cards.iter(app.world()).map(|(_, t)| t.scale.y).collect();
        cards_seen = cards_seen.max(now.len());
        placed |= now.iter().any(|s| *s > 1.0);
    }
    assert_eq!(most_hands, 2, "the Holy launch must flash both hands, once");
    assert_eq!(
        cards_seen, 3,
        "the shielded dummy must wear the three cards"
    );
    assert!(
        placed,
        "the cards were spawned but never placed on the dummy"
    );
}

/// The shield's cards stand on the RENDERED body after every writer of it,
/// the Cyclone lift (`PostUpdate`) included: a cycloned, shielded dummy rises
/// and its rim rises with it, centred just above the body centre on every
/// frame. Placed before the lift, the rim would sit at the unlifted height and
/// fall below the body centre — the order the leg-crossing arc's draw order
/// rests on (`pw_shield.rs`).
#[test]
fn the_shield_rides_a_cyclone_lift() {
    use arenasim::states::animation_sandbox::playback::{
        EntryFamily, SandboxEntry, SandboxPlayback,
    };
    use arenasim::states::animation_sandbox::SandboxConfig;
    use arenasim::states::match_config::CharacterClass;
    use arenasim::states::play_match::abilities::AbilityType;
    use arenasim::states::play_match::components::{
        ActiveAuras, Aura, AuraType, PwShieldCard, PwShieldVisual, PwsLayer, VisualBody,
    };
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    let mut app = boot_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        16,
    )));
    app.update();
    app.insert_resource(SandboxConfig {
        caster_class: CharacterClass::Priest,
        ..Default::default()
    });
    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::AnimationSandbox);
    for _ in 0..4 {
        app.update();
    }
    {
        let mut playback = app.world_mut().resource_mut::<SandboxPlayback>();
        playback.select(
            SandboxEntry::Ability(AbilityType::PowerWordShield),
            EntryFamily::Residue,
        );
        playback.restart_requested = true;
    }
    for _ in 0..50 {
        app.update();
    }
    let mut rigs = app.world_mut().query::<&PwShieldVisual>();
    let bearer = rigs
        .iter(app.world())
        .next()
        .expect("the preview shielded no one")
        .bearer;
    app.world_mut()
        .get_mut::<ActiveAuras>(bearer)
        .expect("the shielded unit has auras")
        .auras
        .push(Aura {
            effect_type: AuraType::Cyclone,
            duration: 6.0,
            ability_name: "Cyclone".to_string(),
            ..Default::default()
        });
    let body = app
        .world()
        .get::<Children>(bearer)
        .unwrap()
        .iter()
        .find(|c| app.world().get::<VisualBody>(*c).is_some())
        .unwrap();

    // This app has no `TransformPlugin`, so read local transforms: the rig
    // sits at the origin, so a card's local pose is its world pose, and the
    // body's world centre is its parent's transform applied to its own.
    let (mut frames, mut highest) = (0, 0.0_f32);
    for _ in 0..40 {
        app.update();
        let root = *app.world().get::<Transform>(bearer).unwrap();
        let lifted = app.world().get::<Transform>(body).unwrap().translation;
        let body_at = root.transform_point(lifted);
        highest = highest.max(body_at.y - root.translation.y);
        let mut cards = app.world_mut().query::<(&PwShieldCard, &Transform)>();
        for (card, t) in cards.iter(app.world()) {
            if card.layer != PwsLayer::Rim {
                continue;
            }
            let rise = t.translation.y - body_at.y;
            assert!(
                (0.1..0.2).contains(&rise),
                "the rim stands {rise:.3} above a body lifted {:.3}",
                body_at.y - root.translation.y
            );
            frames += 1;
        }
    }
    assert!(
        highest > 0.3,
        "the Cyclone never lifted the dummy ({highest:.3})"
    );
    assert_eq!(
        frames, 40,
        "the rim was not on screen for every frame of the lift"
    );
}
