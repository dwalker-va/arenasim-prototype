//! The one sheep rule for DoT states (`dot_state.rs`): a polymorphed victim
//! wears none of them — every piece a DoT state draws on it is hidden while
//! it is a sheep, and shows again when the sheep ends, its clock having run
//! on. The rule is one system, `hide_dot_states_on_sheep`, over every entity
//! a state draws (`DotStateOn`), so this drives every state's own spawner on
//! one victim carrying every state at once and reads what they built.

use std::collections::BTreeSet;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, Combatant, DripKind, PolymorphedVisual,
};
use arenasim::states::play_match::{
    animate_siphon_life_glows, hide_dot_states_on_sheep, spawn_drip_emitters_for_afflicted,
    spawn_immolate_burns, spawn_warlock_dot_visuals, update_drip_emitters, DotStateOn,
    DotStateVisual, SiphonLifeRig, CORRUPTION_AURA, IMMOLATE_AURA, SIPHON_LIFE_AURA, UA_AURA,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);

/// Whether a DoT routed to `kind` draws a STATE on its victim for as long as
/// it runs. Exhaustive: a new kind of DoT visual has to answer it.
fn draws_a_state(kind: DotStateVisual) -> bool {
    match kind {
        DotStateVisual::Drip(_)
        | DotStateVisual::CorruptionShroud
        | DotStateVisual::UnstableAfflictionState
        | DotStateVisual::ImmolateBurn
        | DotStateVisual::SiphonLifeGlow => true,
        // The apparition is the curse's apply moment, not a state.
        DotStateVisual::ApplyApparitionOnly(_) => false,
    }
}

/// Every state kind, one DoT routed to each.
fn every_state() -> Vec<(&'static str, DotStateVisual)> {
    let all = vec![
        ("Serpent Sting", DotStateVisual::Drip(DripKind::Poison)),
        ("Rend", DotStateVisual::Drip(DripKind::Bleed)),
        (CORRUPTION_AURA, DotStateVisual::CorruptionShroud),
        (UA_AURA, DotStateVisual::UnstableAfflictionState),
        (IMMOLATE_AURA, DotStateVisual::ImmolateBurn),
        (SIPHON_LIFE_AURA, DotStateVisual::SiphonLifeGlow),
    ];
    for (name, kind) in &all {
        assert_eq!(
            DotStateVisual::for_dot(name),
            Some(*kind),
            "{name} routes there"
        );
        assert!(draws_a_state(*kind));
    }
    all
}

fn dot(name: &str) -> Aura {
    Aura {
        effect_type: AuraType::DamageOverTime,
        duration: 30.0,
        magnitude: 4.0,
        tick_interval: 3.0,
        time_until_next_tick: 3.0,
        ability_name: name.to_string(),
        ..Default::default()
    }
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::transform::TransformPlugin,
    ));
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.init_asset::<Image>();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
    app.add_systems(
        Update,
        (
            spawn_warlock_dot_visuals,
            animate_siphon_life_glows,
            spawn_immolate_burns,
            spawn_drip_emitters_for_afflicted,
            update_drip_emitters,
            hide_dot_states_on_sheep,
        )
            .chain(),
    );
    app.update();
    app
}

/// What every state draws on `victim`: each entity's kind and visibility.
fn drawn(app: &mut App, victim: Entity) -> Vec<(DotStateVisual, Visibility)> {
    let world = app.world_mut();
    world
        .query::<(&DotStateOn, &Visibility)>()
        .iter(world)
        .filter(|(s, _)| s.victim == victim)
        .map(|(s, v)| (s.kind, *v))
        .collect()
}

fn kinds(drawn: &[(DotStateVisual, Visibility)]) -> BTreeSet<String> {
    drawn.iter().map(|(k, _)| format!("{k:?}")).collect()
}

/// Every state on a sheep is hidden, whichever DoT draws it — and only the
/// sheep's: a victim beside it, carrying the same DoTs, shows all of them.
#[test]
fn a_sheep_wears_no_dot_state() {
    let mut app = app();
    let states = every_state();
    let auras = || ActiveAuras {
        auras: states.iter().map(|(n, _)| dot(n)).collect(),
    };
    let sheep = app
        .world_mut()
        .spawn((
            Combatant::new(2, 0, CharacterClass::Warrior),
            Transform::from_xyz(0.0, 1.0, 0.0),
            auras(),
            PolymorphedVisual,
        ))
        .id();
    let beside = app
        .world_mut()
        .spawn((
            Combatant::new(2, 1, CharacterClass::Mage),
            Transform::from_xyz(4.0, 1.0, 0.0),
            auras(),
        ))
        .id();
    // Long enough for both drip emitters to let a drop fall.
    for _ in 0..40 {
        app.update();
    }
    let all: BTreeSet<String> = states.iter().map(|(_, k)| format!("{k:?}")).collect();
    let on_sheep = drawn(&mut app, sheep);
    assert_eq!(kinds(&on_sheep), all, "every state drew on the sheep");
    for (kind, visibility) in &on_sheep {
        assert_eq!(
            *visibility,
            Visibility::Hidden,
            "{kind:?} shows on the sheep"
        );
    }
    let on_beside = drawn(&mut app, beside);
    assert_eq!(kinds(&on_beside), all);
    for (kind, visibility) in &on_beside {
        assert_eq!(
            *visibility,
            Visibility::Inherited,
            "{kind:?} hidden off a sheep"
        );
    }

    // The sheep ends: every state shows again, its clock having run on.
    let age = |app: &mut App| {
        let world = app.world_mut();
        world
            .query::<&SiphonLifeRig>()
            .iter(world)
            .find(|r| r.target == sheep)
            .map(|r| r.age)
            .expect("the sheep's Siphon Life glow")
    };
    let before = age(&mut app);
    app.world_mut()
        .entity_mut(sheep)
        .remove::<PolymorphedVisual>();
    app.update();
    let after = drawn(&mut app, sheep);
    assert_eq!(kinds(&after), all);
    for (kind, visibility) in &after {
        assert_eq!(*visibility, Visibility::Inherited, "{kind:?} still hidden");
    }
    assert!(age(&mut app) > before, "the clock ran on under the sheep");
}
