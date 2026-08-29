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
fn kidney_shot_starts_its_own_stroke_and_fan() {
    let mut h = Harness::new();
    let rogue = h.spawn_rogue();
    let victim = h.spawn_victim();
    h.fire(rogue, victim, AbilityType::KidneyShot);
    h.tick(1);

    assert_eq!(h.style(), SwingStyle::KidneyShot);
    // KIDNEY_SHOT_CRESCENTS.count — three, against Cheap Shot's four.
    assert_eq!(h.crescents(), 3, "three crescents");
}

#[test]
fn the_two_rogue_stuns_are_visually_distinct() {
    // This is the whole feature. The two apply the same Stun and are
    // byte-identical on the receiver side, so if their caster-side gestures
    // ever converge, nothing else in the codebase would notice.
    let mut a = Harness::new();
    let ra = a.spawn_rogue();
    let va = a.spawn_victim();
    a.fire(ra, va, AbilityType::CheapShot);
    a.tick(1);
    let cheap_count = a.crescents();
    let cheap_style = a.style();
    let cheap: Vec<(f32, f32, f32)> = {
        let mut q = a.app.world_mut().query::<&CrescentFlare>();
        q.iter(a.app.world())
            .map(|c| {
                let s = c.color.to_srgba();
                (s.red, s.green, s.blue)
            })
            .collect()
    };

    let mut b = Harness::new();
    let rb = b.spawn_rogue();
    let vb = b.spawn_victim();
    b.fire(rb, vb, AbilityType::KidneyShot);
    b.tick(1);
    let kidney_count = b.crescents();
    let kidney_style = b.style();
    let kidney: Vec<(f32, f32, f32)> = {
        let mut q = b.app.world_mut().query::<&CrescentFlare>();
        q.iter(b.app.world())
            .map(|c| {
                let s = c.color.to_srgba();
                (s.red, s.green, s.blue)
            })
            .collect()
    };

    assert_ne!(cheap_style, kidney_style, "different strokes");
    assert_ne!(cheap_count, kidney_count, "different crescent counts");
    assert_ne!(cheap[0], kidney[0], "different tints");
    // Cheap Shot is white (the source has no colour track); Kidney Shot is
    // magenta. Prove the difference is a real hue, not a rounding wobble.
    let (cr, cg, cb) = cheap[0];
    assert!(
        (cr - cg).abs() < 0.1 && (cg - cb).abs() < 0.1,
        "Cheap Shot's crescents should be untinted, got {cheap:?}"
    );
    let (kr, kg, kb) = kidney[0];
    assert!(
        kr > kg + 0.4 && kb > kg + 0.3,
        "Kidney Shot's should be magenta, got {kidney:?}"
    );
}

#[test]
fn kidney_shot_is_the_longer_stroke() {
    // The source is 1233ms against Cheap Shot's 634ms. The finisher must stay
    // the heavier of the two; swapping them would invert the reading.
    let cheap = SwingStyle::CheapShot.stroke_secs();
    let kidney = SwingStyle::KidneyShot.stroke_secs();
    assert!(
        kidney > cheap * 1.5,
        "kidney {kidney}s should be well past cheap {cheap}s"
    );
}

#[test]
fn the_fan_sweeps_across_the_casters_breadth() {
    // The first version spawned every crescent at ONE point and fanned only
    // their roll, so the flare read as a rosette clumped beside the body rather
    // than a slash through it. The old probe never caught that because it only
    // compared rolls — it never looked at where the crescents actually were.
    //
    // A combatant capsule is 1.0yd across, so the fan has to span at least that
    // to read as body-wide.
    for ability in [AbilityType::CheapShot, AbilityType::KidneyShot] {
        let mut h = Harness::new();
        let rogue = h.spawn_rogue();
        // Victim straight ahead on +Z, so "across" is the world X axis.
        let victim = h.spawn_victim();
        h.fire(rogue, victim, ability);
        h.tick(1);

        let xs: Vec<f32> = {
            let mut q = h.app.world_mut().query::<(&CrescentFlare, &Transform)>();
            q.iter(h.app.world()).map(|(_, t)| t.translation.x).collect()
        };
        let lo = xs.iter().cloned().fold(f32::INFINITY, f32::min);
        let hi = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let span = hi - lo;
        assert!(
            span > 1.0,
            "{ability:?}'s fan spans only {span}yd across a 1.0yd body — it will \
             read as a clump beside the caster, not a slash through it"
        );
        // And it must stay a slash rather than becoming two separate effects
        // either side of the unit.
        assert!(span < 3.0, "{ability:?}'s fan spans {span}yd — too scattered");
    }
}

#[test]
fn the_fan_follows_the_aim_not_the_world_axes() {
    // The sweep is across the caster's OWN facing, so a target off to the side
    // must rotate the whole fan with it. Spreading along a fixed world axis
    // would collapse the fan to a point for half the possible target bearings.
    let mut h = Harness::new();
    let rogue = h.spawn_rogue();
    // Victim on +X this time, so "across" becomes the world Z axis.
    let victim = h
        .app
        .world_mut()
        .spawn((
            Transform::from_xyz(6.0, 1.0, 0.0),
            Combatant::new(1, 0, CharacterClass::Priest),
        ))
        .id();
    h.fire(rogue, victim, AbilityType::KidneyShot);
    h.tick(1);

    let zs: Vec<f32> = {
        let mut q = h.app.world_mut().query::<(&CrescentFlare, &Transform)>();
        q.iter(h.app.world()).map(|(_, t)| t.translation.z).collect()
    };
    let span = zs.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
        - zs.iter().cloned().fold(f32::INFINITY, f32::min);
    assert!(
        span > 1.0,
        "with the target on +X the fan should spread along Z, got {span}yd"
    );
}

#[test]
fn the_streak_runs_along_the_attack_line() {
    // The WoW reference is a burst of slashes running ALONG the strike, toward
    // the victim. The first three versions had the right shape pointing 90 deg
    // the wrong way, and no probe noticed because none of them asked which way
    // the streak lay — only how big or how bright it was.
    //
    // The streak's long axis is its local +X. Project both it and the aim into
    // the camera's plane and require them to line up.
    for (dx, dz) in [(0.0, 6.0), (6.0, 0.0), (-4.0, 4.0), (3.0, -5.0)] {
        let mut h = Harness::new();
        let rogue = h.spawn_rogue();
        let victim = h
            .app
            .world_mut()
            .spawn((
                Transform::from_xyz(dx, 1.0, dz),
                Combatant::new(1, 0, CharacterClass::Priest),
            ))
            .id();
        // A camera looking down the arena from above and behind, as the sandbox
        // and match cameras both do.
        let cam = h
            .app
            .world_mut()
            .spawn((
                Camera3d::default(),
                Transform::from_xyz(0.0, 9.0, 24.0).looking_at(Vec3::ZERO, Vec3::Y),
            ))
            .id();
        h.fire(rogue, victim, AbilityType::KidneyShot);
        // Far enough in that every crescent has passed its stagger delay. A
        // crescent still waiting its turn holds `scale = ZERO` and returns
        // early, so its billboard rotation has not been written yet — sampling
        // at tick 2 sees only the first of the fan.
        h.tick(8);

        let cam_rot = h.app.world().get::<Transform>(cam).unwrap().rotation;
        let aim = Vec3::new(dx, 0.0, dz).normalize();
        let aim_screen = cam_rot.inverse() * aim;
        let aim_angle = aim_screen.y.atan2(aim_screen.x);

        let rots: Vec<Quat> = {
            let mut q = h.app.world_mut().query::<(&CrescentFlare, &Transform)>();
            q.iter(h.app.world()).map(|(_, t)| t.rotation).collect()
        };
        assert!(!rots.is_empty(), "no crescents to check");

        // The middle crescent of the fan carries no roll offset, so it should
        // sit closest to the aim; check the whole fan brackets it.
        let mut best = f32::MAX;
        for rot in &rots {
            let long_axis = cam_rot.inverse() * (*rot * Vec3::X);
            let angle = long_axis.y.atan2(long_axis.x);
            let mut delta = (angle - aim_angle).abs();
            while delta > std::f32::consts::PI {
                delta = (delta - std::f32::consts::TAU).abs();
            }
            // A streak and its mirror read the same, so fold onto [0, PI/2].
            let folded = delta.min((std::f32::consts::PI - delta).abs());
            best = best.min(folded);
        }
        assert!(
            best < 0.35,
            "toward ({dx}, {dz}) the closest streak is {best} rad off the attack \
             line — the burst is standing across the strike, not along it"
        );
    }
}

#[test]
fn the_streak_is_a_slash_not_a_crescent_moon() {
    // Shape, stated as geometry rather than by eye: the arc's chord must be
    // several times its sag, or it is a deep "C" standing beside the caster
    // rather than a cut running through the strike. The reference is
    // near-straight with a slight bow.
    let span = arenasim::states::play_match::crescent_span();
    let radius = arenasim::states::play_match::crescent_arc_radius();
    let chord = 2.0 * radius * span.sin();
    let sag = radius * (1.0 - span.cos());
    assert!(
        chord > sag * 5.0,
        "chord {chord} against sag {sag} — that is a crescent moon, not a slash"
    );
    assert!(sag > 0.02, "a slash wants some bow, got {sag}");
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
