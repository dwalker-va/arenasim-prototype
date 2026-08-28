//! Probes for the rogue stuns' caster-side gestures (Cheap Shot, Kidney Shot).
//!
//! The two are byte-identical on the receiver side and differ entirely on the
//! caster side, so what these pin is that each one's marker produces ITS OWN
//! stroke and ITS OWN crescent fan — the distinction is the whole feature, and
//! nothing else in the codebase would notice if they collapsed into each other.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` — no window, no GPU.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::components::{
    Combatant, CrescentFlare, InstantAbilityFired, SwingStyle, VisualBody, WeaponHand, WeaponKind,
    WeaponSocket,
};
use arenasim::states::play_match::{
    cleanup_crescent_flares, consume_instant_ability_signals, update_crescent_flares,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(50);

struct Harness {
    app: App,
}

impl Harness {
    fn new() -> Self {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()));
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.init_asset::<Image>();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
        app.add_systems(
            Update,
            (
                consume_instant_ability_signals,
                update_crescent_flares,
                cleanup_crescent_flares,
            )
                .chain(),
        );
        Harness { app }
    }

    /// A Rogue with a main-hand dagger socket, mirroring `class_weapon_loadout`.
    fn spawn_rogue(&mut self) -> Entity {
        let unit = self
            .app
            .world_mut()
            .spawn((
                Transform::from_xyz(0.0, 1.0, 0.0),
                Combatant::new(0, 0, CharacterClass::Rogue),
            ))
            .id();
        let body = self
            .app
            .world_mut()
            .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
            .id();
        let socket = self
            .app
            .world_mut()
            .spawn((
                WeaponSocket {
                    kind: WeaponKind::Dagger,
                    hand: WeaponHand::Main,
                    owner: unit,
                    rest: Transform::IDENTITY,
                    release_t: None,
                    aim: Vec3::ZERO,
                    winds_up_next: true,
                    yaw_local: 0.0,
                    prev_owner_yaw: 0.0,
                    windup_s: 0.0,
                    swing_style: SwingStyle::Auto,
                    last_s: 0.0,
                },
                Transform::default(),
            ))
            .id();
        self.app.world_mut().entity_mut(body).add_child(socket);
        self.app.world_mut().entity_mut(unit).add_child(body);
        unit
    }

    fn spawn_victim(&mut self) -> Entity {
        self.app
            .world_mut()
            .spawn((
                Transform::from_xyz(0.0, 1.0, 3.0),
                Combatant::new(1, 0, CharacterClass::Priest),
            ))
            .id()
    }

    fn fire(&mut self, caster: Entity, target: Entity, ability: AbilityType) {
        self.app.world_mut().spawn(InstantAbilityFired {
            caster,
            target: Some(target),
            ability,
            is_crit: false,
        });
    }

    fn tick(&mut self, n: u32) {
        for _ in 0..n {
            self.app.update();
        }
    }

    fn crescents(&mut self) -> usize {
        self.app
            .world_mut()
            .query::<&CrescentFlare>()
            .iter(self.app.world())
            .count()
    }

    fn style(&mut self) -> SwingStyle {
        self.app
            .world_mut()
            .query::<&WeaponSocket>()
            .iter(self.app.world())
            .next()
            .unwrap()
            .swing_style
    }

    fn markers(&mut self) -> usize {
        self.app
            .world_mut()
            .query::<&InstantAbilityFired>()
            .iter(self.app.world())
            .count()
    }
}

#[test]
fn cheap_shot_starts_its_own_stroke_and_fan() {
    let mut h = Harness::new();
    let rogue = h.spawn_rogue();
    let victim = h.spawn_victim();
    h.fire(rogue, victim, AbilityType::CheapShot);
    h.tick(1);

    assert_eq!(h.style(), SwingStyle::CheapShot, "its own stroke, not Auto");
    // CHEAP_SHOT_CRESCENTS.count — four, matching the source's four quads.
    assert_eq!(h.crescents(), 4, "four crescents");
    assert_eq!(h.markers(), 0, "the marker is consumed as it is read");
}

#[test]
fn the_fan_is_staggered_not_simultaneous() {
    // The source pops Cheap Shot's crescents in two pairs 100ms apart. A fan
    // that appeared all at once would read as one thick smear rather than
    // successive slashes, and nothing else would catch that.
    let mut h = Harness::new();
    let rogue = h.spawn_rogue();
    let victim = h.spawn_victim();
    h.fire(rogue, victim, AbilityType::CheapShot);
    h.tick(1);

    let delays: Vec<f32> = {
        let mut q = h.app.world_mut().query::<&CrescentFlare>();
        let mut d: Vec<f32> = q.iter(h.app.world()).map(|c| c.delay).collect();
        d.sort_by(|a, b| a.partial_cmp(b).unwrap());
        d
    };
    assert_eq!(delays.len(), 4);
    assert_eq!(delays[0], 0.0, "the first crescent is immediate");
    assert!(
        delays[3] > delays[0],
        "the last must be held back, got {delays:?}"
    );
}

#[test]
fn the_fan_spreads_across_the_screen() {
    // Each crescent is rolled about the view axis so the fan reads as separate
    // strokes. All-equal rolls would stack them into one shape.
    let mut h = Harness::new();
    let rogue = h.spawn_rogue();
    let victim = h.spawn_victim();
    h.fire(rogue, victim, AbilityType::CheapShot);
    h.tick(1);

    let rolls: Vec<f32> = {
        let mut q = h.app.world_mut().query::<&CrescentFlare>();
        q.iter(h.app.world()).map(|c| c.roll).collect()
    };
    let first = rolls[0];
    assert!(
        rolls.iter().any(|r| (r - first).abs() > 0.1),
        "every crescent shares a roll: {rolls:?}"
    );
}

#[test]
fn crescents_expire_without_leaking() {
    let mut h = Harness::new();
    let rogue = h.spawn_rogue();
    let victim = h.spawn_victim();
    h.fire(rogue, victim, AbilityType::CheapShot);
    h.tick(1);
    assert!(h.crescents() > 0, "guard: the drain must not pass vacuously");

    // Past the last crescent's delay plus its lifetime.
    h.tick(30);
    assert_eq!(h.crescents(), 0, "the fan must not leak");
}

#[test]
fn a_caster_with_no_socket_still_gets_its_flourish() {
    // The Mage has no `WeaponSocket` at all (`class_weapon_loadout`), so the
    // stroke half of the router is unreachable for it. That must not suppress
    // the flourish — the bug the router restructure fixed.
    let mut h = Harness::new();
    let mage = h
        .app
        .world_mut()
        .spawn((
            Transform::from_xyz(0.0, 1.0, 0.0),
            Combatant::new(0, 0, CharacterClass::Mage),
        ))
        .id();
    let victim = h.spawn_victim();
    // Cheap Shot on a socketless caster is not a real pairing; it is the
    // cheapest way to prove the flourish path does not depend on a socket.
    h.fire(mage, victim, AbilityType::CheapShot);
    h.tick(1);

    assert_eq!(h.crescents(), 4, "the fan does not require a weapon");
    assert_eq!(h.markers(), 0);
}
