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
fn hammer_of_justice_has_a_flourish_but_no_stroke() {
    // The whole shape of this ability. Until the router separated its two
    // dispatches, an ability with no stroke could not reach a flourish at all.
    assert_eq!(
        swing_style_for_ability(AbilityType::HammerOfJustice),
        None,
        "the source spawns no hammer — the mace must not swing"
    );

    let mut h = Harness::new();
    let paladin = h.spawn_paladin();
    let victim = h.spawn_victim(6.0);
    h.fire(paladin, victim);
    h.tick(1);

    assert_eq!(h.streaks(), 1, "the ground streak must still fire");
    assert_eq!(h.runes(), 1, "and so must the victim's rune");
    assert_eq!(
        h.socket_style(),
        SwingStyle::Auto,
        "the Paladin's mace must be left alone"
    );
    assert!(
        !h.socket_released(),
        "no stroke may be started on the socket"
    );
}

#[test]
fn the_streak_reaches_the_victim() {
    // Our deliberate divergence from the source, which uses a fixed ~4 units.
    // At a 10yd range a streak that stops short reads as a misfire, and it is
    // the only thing connecting the two units.
    let mut h = Harness::new();
    let paladin = h.spawn_paladin();
    let victim = h.spawn_victim(8.0);
    h.fire(paladin, victim);
    h.tick(1);

    let length = {
        let mut q = h.app.world_mut().query::<&HolyStreak>();
        q.iter(h.app.world()).next().unwrap().length
    };
    assert!(
        (length - 8.0).abs() < 0.1,
        "streak should span the real 8yd gap, got {length}"
    );
}

#[test]
fn the_streak_actually_points_at_the_victim() {
    // Both of this module's real bugs slipped past the original probes because
    // they asserted on the `length` FIELD and on `scale.x` — bookkeeping, not
    // geometry. The quad's length axis is its local +X, so the only honest test
    // is where that axis ends up in the world.
    for (dx, dz) in [(0.0, 8.0), (8.0, 0.0), (0.0, -8.0), (-5.0, 5.0), (4.0, -6.0)] {
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
        h.tick(1);

        let (rot, _) = {
            let mut q = h.app.world_mut().query::<(&HolyStreak, &Transform)>();
            let (_, t) = q.iter(h.app.world()).next().unwrap();
            (t.rotation, t.translation)
        };
        let aim = Vec3::new(dx, 0.0, dz).normalize();
        let length_axis = rot * Vec3::X;
        let dot = length_axis.dot(aim);
        assert!(
            dot > 0.99,
            "streak's length axis {length_axis:?} should follow the aim {aim:?} \
             toward ({dx}, {dz}); dot = {dot}"
        );
        // The flat rotation must still leave the decal lying on the ground.
        let normal = rot * Vec3::Z;
        assert!(
            normal.y.abs() > 0.99,
            "the streak should lie flat, normal {normal:?}"
        );
    }
}

#[test]
fn the_streak_grows_forward_from_the_caster() {
    // A Bevy Rectangle is centred on its origin, so scaling alone would put half
    // the streak BEHIND the Paladin and stop its head halfway to the victim.
    let mut h = Harness::new();
    let paladin = h.spawn_paladin();
    let victim = h.spawn_victim(8.0);
    h.fire(paladin, victim);
    // Far enough in for the streak to have reached full extension.
    h.tick(14);

    let (rot, pos, len) = {
        let mut q = h.app.world_mut().query::<(&HolyStreak, &Transform)>();
        let (s, t) = q.iter(h.app.world()).next().unwrap();
        (t.rotation, t.translation, t.scale.x)
    };
    let axis = rot * Vec3::X;
    let head = pos + axis * (len * 0.5);
    let tail = pos - axis * (len * 0.5);

    assert!(
        tail.distance(Vec3::new(0.0, tail.y, 0.0)) < 0.3,
        "the tail should stay at the Paladin's feet, got {tail:?}"
    );
    assert!(
        head.z > 7.0,
        "the head should arrive at the victim ~8yd out, got {head:?}"
    );
}

#[test]
fn the_streak_extends_over_time() {
    // It races outward rather than appearing at full length — that travel is
    // how a projectile-less 10yd ability covers its range.
    let mut h = Harness::new();
    let paladin = h.spawn_paladin();
    let victim = h.spawn_victim(8.0);
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
        later > early + 0.5,
        "streak should be racing outward: {early} -> {later}"
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
