//! Probes for Hammer of Justice's caster-side gesture.
//!
//! The load-bearing fact here is a NEGATIVE one: Hammer of Justice must get its
//! flourish while getting NO weapon stroke. The source spawns no hammer and no
//! projectile, so swinging the Paladin's mace would invent a weapon attack the
//! spell does not have — and until the router's swing and flourish dispatches
//! were separated, "no stroke" also meant "no flourish".
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` — no window, no GPU.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::components::{
    Combatant, HolyStreak, InstantAbilityFired, JusticeRune, SwingStyle, VisualBody, WeaponHand,
    WeaponKind, WeaponSocket,
};
use arenasim::states::play_match::{
    cleanup_holy_justice, consume_instant_ability_signals, swing_style_for_ability,
    update_holy_streaks, update_justice_runes,
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
        // `consume_instant_ability_signals` reads Frost Nova's radius from
        // the real ability data rather than restating it as a constant.
        app.insert_resource(AbilityDefinitions::default());
        app.add_systems(
            Update,
            (
                consume_instant_ability_signals,
                update_holy_streaks,
                update_justice_runes,
                cleanup_holy_justice,
            )
                .chain(),
        );
        Harness { app }
    }

    /// A Paladin WITH a main-hand mace socket, exactly as `class_weapon_loadout`
    /// gives it — so "no stroke" is proven to be a choice, not an accident of
    /// the Paladin having nothing to swing.
    fn spawn_paladin(&mut self) -> Entity {
        let unit = self
            .app
            .world_mut()
            .spawn((
                Transform::from_xyz(0.0, 1.0, 0.0),
                Combatant::new(0, 0, CharacterClass::Paladin),
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
                    kind: WeaponKind::Mace,
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

    fn spawn_victim(&mut self, distance: f32) -> Entity {
        self.app
            .world_mut()
            .spawn((
                Transform::from_xyz(0.0, 1.0, distance),
                Combatant::new(1, 0, CharacterClass::Priest),
            ))
            .id()
    }

    fn fire(&mut self, caster: Entity, target: Entity) {
        self.app.world_mut().spawn(InstantAbilityFired {
            caster,
            target: Some(target),
            ability: AbilityType::HammerOfJustice,
            is_crit: false,
        });
    }

    fn tick(&mut self, n: u32) {
        for _ in 0..n {
            self.app.update();
        }
    }

    fn streaks(&mut self) -> usize {
        self.app
            .world_mut()
            .query::<&HolyStreak>()
            .iter(self.app.world())
            .count()
    }

    fn runes(&mut self) -> usize {
        self.app
            .world_mut()
            .query::<&JusticeRune>()
            .iter(self.app.world())
            .count()
    }

    fn socket_style(&mut self) -> SwingStyle {
        self.app
            .world_mut()
            .query::<&WeaponSocket>()
            .iter(self.app.world())
            .next()
            .unwrap()
            .swing_style
    }

    fn socket_released(&mut self) -> bool {
        self.app
            .world_mut()
            .query::<&WeaponSocket>()
            .iter(self.app.world())
            .next()
            .unwrap()
            .release_t
            .is_some()
    }
}

#[test]
fn hammer_of_justice_swings_an_uppercut() {
    // This ability shipped for a while with NO stroke, on the reasoning that
    // `HasMissile = 0` and a `SpecialUnarmed` animation name meant no weapon
    // motion. Those rule out a projectile -- nothing is thrown -- and say
    // nothing about the arm; the reference shows a raised-weapon pose.
    assert_eq!(
        swing_style_for_ability(AbilityType::HammerOfJustice),
        Some(SwingStyle::HammerOfJustice),
    );

    let mut h = Harness::new();
    let paladin = h.spawn_paladin();
    let victim = h.spawn_victim(6.0);
    h.fire(paladin, victim);
    h.tick(1);

    assert_eq!(h.streaks(), 1, "the ground wave must still fire");
    assert_eq!(h.runes(), 1, "and so must the victim's rune");
    assert_eq!(
        h.socket_style(),
        SwingStyle::HammerOfJustice,
        "the mace should be swinging its own stroke"
    );
    assert!(h.socket_released(), "the stroke should have started");
}

#[test]
fn the_uppercut_is_not_mortal_strikes_diagonal() {
    // The only two big two-beat strokes in the game. An uppercut is
    // near-vertical and Mortal Strike is a 49-degree diagonal; if they converge
    // the Paladin looks like it is casting the Warrior's ability.
    use arenasim::states::play_match::swing_plane_tilt;
    let hoj = swing_plane_tilt(SwingStyle::HammerOfJustice).expect("HoJ is a plane");
    let ms = swing_plane_tilt(SwingStyle::MortalStrike).expect("MS is a plane");
    assert!(hoj < 0.35, "the uppercut leans {hoj} rad -- that is a slash");
    assert!(
        ms - hoj > 0.5,
        "the uppercut {hoj} and Mortal Strike {ms} are too close to tell apart"
    );
}

#[test]
fn the_caster_ring_stays_centred_on_the_paladin() {
    // The reference sweeps a flat ring out around the paladin's OWN feet. An
    // earlier version aimed a streak at the victim, which implies a projectile
    // this spell does not have (`HasMissile = 0`) — and that streak had to be
    // yawed and re-anchored every frame, both of which it got wrong.
    //
    // A ring needs neither: it is radially symmetric and it does not move.
    for (dx, dz) in [(0.0, 8.0), (7.0, 0.0), (-5.0, 5.0)] {
        let mut h = Harness::new();
        let paladin = h.spawn_paladin();
        let victim = h
            .app
            .world_mut()
            .spawn((
                Transform::from_xyz(dx, 1.0, dz),
                Combatant::new(1, 0, CharacterClass::Priest),
            ))
            .id();
        h.fire(paladin, victim);
        h.tick(8);

        let (pos, scale) = {
            let mut q = h.app.world_mut().query::<(&HolyStreak, &Transform)>();
            let (_, t) = q.iter(h.app.world()).next().unwrap();
            (t.translation, t.scale)
        };
        assert!(
            pos.x.abs() < 0.01 && pos.z.abs() < 0.01,
            "toward ({dx}, {dz}) the ring drifted to {pos:?} — it should stay on \
             the caster wherever the victim is"
        );
        assert!(
            (scale.x - scale.y).abs() < 0.01,
            "the ring is being stretched rather than scaled: {scale:?}"
        );
    }
}

#[test]
fn the_caster_ring_sweeps_outward() {
    // It expands rather than appearing at full size — that sweep is the whole
    // of the caster-side tell.
    let mut h = Harness::new();
    let paladin = h.spawn_paladin();
    let victim = h.spawn_victim(6.0);
    h.fire(paladin, victim);
    h.tick(2);

    let early = {
        let mut q = h.app.world_mut().query::<(&HolyStreak, &Transform)>();
        q.iter(h.app.world()).next().unwrap().1.scale.x
    };
    h.tick(6);
    let later = {
        let mut q = h.app.world_mut().query::<(&HolyStreak, &Transform)>();
        q.iter(h.app.world()).next().unwrap().1.scale.x
    };
    assert!(
        later > early + 0.4,
        "the ring should be sweeping out: {early} -> {later}"
    );
}

#[test]
fn both_halves_expire_without_leaking() {
    let mut h = Harness::new();
    let paladin = h.spawn_paladin();
    let victim = h.spawn_victim(6.0);
    h.fire(paladin, victim);
    h.tick(1);
    assert!(h.streaks() > 0 && h.runes() > 0, "guard against a vacuous drain");

    // Past the rune's life, which is the longer of the two.
    h.tick(40);
    assert_eq!(h.streaks(), 0, "streak leaked");
    assert_eq!(h.runes(), 0, "rune leaked");
}

#[test]
fn the_rune_outlives_the_streak_in_practice() {
    // Cause then consequence: the streak arrives and goes, the rune holds on
    // the victim after it. Asserted on real timings, not just the constants.
    let mut h = Harness::new();
    let paladin = h.spawn_paladin();
    let victim = h.spawn_victim(6.0);
    h.fire(paladin, victim);
    h.tick(1);

    // 15 ticks = 0.75s: past the streak's 0.667s, inside the rune's 1.05s.
    h.tick(15);
    assert_eq!(h.streaks(), 0, "the streak should have landed and gone");
    assert_eq!(h.runes(), 1, "the rune should still be marking the victim");
}
