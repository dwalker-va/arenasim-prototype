//! Probes for Immolate's burn STATE (`rendering/effects/immolate.rs`) — the
//! client kit-235 flame licks that climb a burning victim from the feet, and
//! the embers that stream off the crown, for as long as the DoT runs.
//!
//! These assert WORLD-SPACE GEOMETRY under propagated `GlobalTransform`s, not
//! stored fields: that licks are born at the FEET and OUTSIDE the 0.5-yd body
//! capsule (the AS-10 buried-inside-the-capsule lesson), that they climb past
//! the chest, that embers leave from above the crown, that every sprite
//! stands upright and faces the camera, and that the fire follows a running
//! victim and ends with the DoT.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window,
//! no GPU — with the exact chained contract `states/mod.rs` registers.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, Combatant, Pet, PetType,
};
use arenasim::states::play_match::{
    age_immolate_sparks, animate_immolate_burns, billboard_immolate_sparks, cleanup_immolate_burns,
    ramp_keyed, spark_alpha, spark_half_size, spawn_immolate_burns, ImmolateBurnRig, ImmolateSpark,
    ImmolateSparkKind, COMBATANT_BODY_RADIUS, CORRUPTION_AURA, CROWN_Y, EMBER_LIFE, EMBER_RATE,
    FLAME_LIFE, FLAME_RATE, FLAME_RING_INNER, FLAME_RING_OUTER, IMMOLATE_AURA,
    IMMOLATE_FLAME_RATE_SCALE, IMPACT_CHEST_Y, IMPACT_PET_STATURE,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
const TICK_SECS: f32 = 0.016;

/// The real game height: combatants stand with capsule centres at world
/// y = 1.0 over the floor plane at world y = 0.
const COMBATANT_Y: f32 = 1.0;
/// The real pet sim height (`play_match/mod.rs` spawns pets at 0.75).
const PET_SIM_Y: f32 = 0.75;
/// Slack on positions integrated over one frame.
const EPS: f32 = 0.02;
/// "Newborn": emitted within the last three frames. At 30 licks/s and 16 ms
/// frames the emitter fires at least once in any three, so the set is never
/// empty; the licks in it have risen well under a centimetre.
const NEWBORN_SECS: f32 = TICK_SECS * 3.5;

struct Harness {
    app: App,
}

impl Harness {
    fn new() -> Self {
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
        // The exact chained contract graphical mode registers.
        app.add_systems(
            Update,
            (
                spawn_immolate_burns,
                animate_immolate_burns,
                age_immolate_sparks,
                billboard_immolate_sparks,
                cleanup_immolate_burns,
            )
                .chain(),
        );
        Harness { app }
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    fn seconds(&mut self, secs: f32) {
        self.tick((secs / TICK_SECS).round() as u32);
    }

    fn aura(effect_type: AuraType, name: &str) -> Aura {
        Aura {
            effect_type,
            duration: 15.0,
            magnitude: 10.0,
            tick_interval: 3.0,
            time_until_next_tick: 3.0,
            ability_name: name.to_string(),
            ..Default::default()
        }
    }

    fn spawn_victim_with(&mut self, at: Vec3, auras: Vec<Aura>) -> Entity {
        self.app
            .world_mut()
            .spawn((
                Combatant::new(0, 0, CharacterClass::Warrior),
                Transform::from_translation(at),
                ActiveAuras { auras },
            ))
            .id()
    }

    fn spawn_burning(&mut self, at: Vec3) -> Entity {
        self.spawn_victim_with(
            at,
            vec![Self::aura(AuraType::DamageOverTime, IMMOLATE_AURA)],
        )
    }

    fn dispel(&mut self, victim: Entity) {
        self.app
            .world_mut()
            .get_mut::<ActiveAuras>(victim)
            .expect("victim has auras")
            .auras
            .retain(|a| a.ability_name != IMMOLATE_AURA);
    }

    /// (kind, age, world position, world rotation) of every live spark.
    fn sparks(&mut self) -> Vec<(ImmolateSparkKind, f32, Vec3, Quat)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&ImmolateSpark, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(s, g)| {
                let (_, rotation, translation) = g.to_scale_rotation_translation();
                (s.kind, s.age, translation, rotation)
            })
            .collect()
    }

    fn of(&mut self, kind: ImmolateSparkKind) -> Vec<(f32, Vec3, Quat)> {
        self.sparks()
            .into_iter()
            .filter(|s| s.0 == kind)
            .map(|(_, age, at, rot)| (age, at, rot))
            .collect()
    }

    fn count<C: Component>(&mut self) -> usize {
        let mut q = self.app.world_mut().query::<&C>();
        q.iter(self.app.world()).count()
    }
}

/// Horizontal distance from the victim's vertical axis.
fn radial(at: Vec3, victim: Vec3) -> f32 {
    Vec2::new(at.x - victim.x, at.z - victim.z).length()
}

fn assert_nonempty<T>(label: &str, v: &[T]) {
    assert!(
        !v.is_empty(),
        "{label}: nothing to check — the probe went vacuous"
    );
}

// ── pure tracks ─────────────────────────────────────────────────────────────

/// The client tracks put their middle key off-centre (55 % for the licks):
/// the ramp must hit it exactly there, and the endpoints at 0 and 1.
#[test]
fn keyed_ramps_hit_their_off_centre_middle_key() {
    let t = [0.2, 1.0, 0.4];
    let at = |k: f32| ramp_keyed(t, 0.55, k);
    let near = |a: f32, b: f32| (a - b).abs() < 1e-6;
    assert!(near(at(0.0), 0.2));
    assert!(near(at(0.55), 1.0));
    assert!(near(at(1.0), 0.4));
    assert!(
        near(at(0.275), 0.6),
        "halfway to the key is halfway up the ramp"
    );
    // Clamped outside the life.
    assert!(near(at(-1.0), 0.2));
    assert!(near(at(2.0), 0.4));
}

/// A lick swells to its peak at 55 % of life and fades out at the top; an
/// ember flares at 30 % and is gone at the end of its life.
#[test]
fn licks_swell_mid_life_and_embers_burn_out() {
    let peak = spark_half_size(ImmolateSparkKind::Lick, 0.55);
    assert!(peak > spark_half_size(ImmolateSparkKind::Lick, 0.0));
    assert!(peak > spark_half_size(ImmolateSparkKind::Lick, 1.0));
    assert_eq!(spark_alpha(ImmolateSparkKind::Lick, 0.55), 1.0);
    assert_eq!(spark_alpha(ImmolateSparkKind::Ember, 0.3), 1.0);
    assert_eq!(spark_alpha(ImmolateSparkKind::Ember, 1.0), 0.0);
}

// ── geometry ────────────────────────────────────────────────────────────────

/// Freshly emitted licks sit at the FEET (the floor, attachment 19) on a ring
/// that clears the body capsule — a lick born inside the capsule would be
/// swallowed by the opaque body the moment it rose.
#[test]
fn licks_are_born_at_the_feet_outside_the_body() {
    let mut h = Harness::new();
    let victim = Vec3::new(3.0, COMBATANT_Y, -2.0);
    h.spawn_burning(victim);
    h.seconds(0.5);

    let fresh: Vec<_> = h
        .of(ImmolateSparkKind::Lick)
        .into_iter()
        .filter(|(age, _, _)| *age <= NEWBORN_SECS)
        .collect();
    assert_nonempty("newborn licks", &fresh);
    let all = h.of(ImmolateSparkKind::Lick);
    // Every lick alive after half a second, however old, is still outside the
    // body (their rise is vertical plus an OUTWARD lean).
    for (_, at, _) in &all {
        let r = radial(*at, victim);
        assert!(
            r > COMBATANT_BODY_RADIUS,
            "a lick at radius {r} is inside the body capsule"
        );
    }
    for (_, at, _) in &fresh {
        assert!(
            (0.0..0.1).contains(&at.y),
            "a newborn lick must start at the floor under the victim, got y = {}",
            at.y
        );
        let r = radial(*at, victim);
        assert!(
            (FLAME_RING_INNER - EPS..=FLAME_RING_OUTER + EPS).contains(&r),
            "a newborn lick must start on the emission ring, got radius {r}"
        );
    }
}

/// Licks accelerate upward and climb the whole body — past the chest — the
/// client's "burning victim". None may overshoot far above the head.
#[test]
fn licks_climb_past_the_chest() {
    let mut h = Harness::new();
    let victim = Vec3::new(0.0, COMBATANT_Y, 0.0);
    h.spawn_burning(victim);
    h.seconds(3.0);

    let licks = h.of(ImmolateSparkKind::Lick);
    assert_nonempty("licks", &licks);
    let top = licks.iter().map(|(_, at, _)| at.y).fold(f32::MIN, f32::max);
    assert!(
        top > victim.y + IMPACT_CHEST_Y,
        "the highest lick ({top}) never reached the chest ({})",
        victim.y + IMPACT_CHEST_Y
    );
    assert!(
        top < victim.y + CROWN_Y + 1.0,
        "a lick flew {top} — far above the head"
    );
    // Height increases with age: the oldest third sits above the youngest
    // third on average (they rise; they do not hover at the feet).
    let mut by_age = licks.clone();
    by_age.sort_by(|a, b| a.0.total_cmp(&b.0));
    let third = by_age.len() / 3;
    let mean = |s: &[(f32, Vec3, Quat)]| s.iter().map(|x| x.1.y).sum::<f32>() / s.len() as f32;
    assert!(mean(&by_age[by_age.len() - third..]) > mean(&by_age[..third]) + 0.5);
}

/// Embers stream off the CROWN, straight up, within the client's small plane.
#[test]
fn embers_rise_off_the_crown() {
    let mut h = Harness::new();
    let victim = Vec3::new(-4.0, COMBATANT_Y, 1.0);
    h.spawn_burning(victim);
    h.seconds(2.0);

    let embers = h.of(ImmolateSparkKind::Ember);
    assert_nonempty("embers", &embers);
    for (_, at, _) in &embers {
        assert!(
            at.y >= victim.y + CROWN_Y - EPS,
            "an ember at y {} is below the crown {}",
            at.y,
            victim.y + CROWN_Y
        );
        assert!(radial(*at, victim) < 0.3, "an ember strayed off the crown");
    }
}

/// Steady state: the rig carries rate × life of each stream at once (within
/// one sprite of rounding) — so it neither leaks sprites nor starves.
#[test]
fn the_burn_holds_its_steady_state_population() {
    let mut h = Harness::new();
    h.spawn_burning(Vec3::new(0.0, COMBATANT_Y, 0.0));
    h.seconds(4.0);
    let licks = h.of(ImmolateSparkKind::Lick).len() as f32;
    let embers = h.of(ImmolateSparkKind::Ember).len() as f32;
    let want_licks = FLAME_RATE * IMMOLATE_FLAME_RATE_SCALE * FLAME_LIFE;
    let want_embers = EMBER_RATE * IMMOLATE_FLAME_RATE_SCALE * EMBER_LIFE;
    assert!(
        (licks - want_licks).abs() <= 1.0,
        "{licks} licks, want {want_licks}"
    );
    assert!(
        (embers - want_embers).abs() <= 1.0,
        "{embers} embers, want {want_embers}"
    );
}

/// Every sprite stands UPRIGHT (its local Y stays world Y, so the upward
/// stretch reads as a tongue from any angle) and faces the camera — checked
/// from two camera bearings, since one can pass by coincidence.
#[test]
fn sprites_stand_upright_and_face_the_camera() {
    for cam_at in [Vec3::new(12.0, 8.0, 0.0), Vec3::new(-3.0, 6.0, 14.0)] {
        let mut h = Harness::new();
        h.app.world_mut().spawn((
            Camera3d::default(),
            Transform::from_translation(cam_at).looking_at(Vec3::ZERO, Vec3::Y),
        ));
        h.spawn_burning(Vec3::new(0.0, COMBATANT_Y, 0.0));
        h.seconds(1.0);
        let sparks = h.sparks();
        assert_nonempty("sparks", &sparks);
        for (_, _, at, rot) in sparks {
            assert!((rot * Vec3::Y).dot(Vec3::Y) > 0.999, "a sprite tipped over");
            let to_cam = (cam_at - at).with_y(0.0).normalize();
            assert!(
                (rot * Vec3::Z).dot(to_cam) > 0.99,
                "a sprite at {at} does not face the camera at {cam_at}"
            );
        }
    }
}

/// The fire follows a running victim: the rig re-anchors under the victim's
/// new position and its sparks come along.
#[test]
fn the_fire_follows_a_moving_victim() {
    let mut h = Harness::new();
    let victim = h.spawn_burning(Vec3::new(0.0, COMBATANT_Y, 0.0));
    h.seconds(0.5);
    let moved = Vec3::new(7.0, COMBATANT_Y, 4.0);
    h.app
        .world_mut()
        .get_mut::<Transform>(victim)
        .unwrap()
        .translation = moved;
    h.tick(2);
    let licks = h.of(ImmolateSparkKind::Lick);
    assert_nonempty("licks", &licks);
    for (_, at, _) in licks {
        assert!(
            radial(at, moved) < FLAME_RING_OUTER + 0.3,
            "a lick stayed behind at {at}"
        );
    }
}

// ── pets ────────────────────────────────────────────────────────────────────

/// A burning pet: the flames start on the floor (a pet's stature-corrected
/// feet anchor lands below it and is clamped) and the ring shrinks with the
/// pet's stature.
#[test]
fn a_burning_pet_burns_from_the_floor_at_pet_scale() {
    let mut h = Harness::new();
    let owner = h.spawn_victim_with(Vec3::new(9.0, COMBATANT_Y, 0.0), vec![]);
    let at = Vec3::new(4.0, PET_SIM_Y, 4.0);
    h.app.world_mut().spawn((
        Combatant::new(0, 0, CharacterClass::Hunter),
        Pet {
            owner,
            pet_type: PetType::Felhunter,
        },
        Transform::from_translation(at),
        ActiveAuras {
            auras: vec![Harness::aura(AuraType::DamageOverTime, IMMOLATE_AURA)],
        },
    ));
    h.seconds(0.5);
    let fresh: Vec<_> = h
        .of(ImmolateSparkKind::Lick)
        .into_iter()
        .filter(|(age, _, _)| *age <= NEWBORN_SECS)
        .collect();
    assert_nonempty("newborn pet licks", &fresh);
    for (_, p, _) in fresh {
        assert!((0.0..0.1).contains(&p.y), "pet lick born at y {}", p.y);
        let r = radial(p, at);
        let ring = FLAME_RING_INNER * IMPACT_PET_STATURE - EPS
            ..=FLAME_RING_OUTER * IMPACT_PET_STATURE + EPS;
        assert!(ring.contains(&r), "pet lick radius {r} is not at pet scale");
    }
}

// ── lifecycle ───────────────────────────────────────────────────────────────

/// Dispelling Immolate ends the fire at once — rig and every spark.
#[test]
fn dispel_puts_the_fire_out() {
    let mut h = Harness::new();
    let victim = h.spawn_burning(Vec3::new(0.0, COMBATANT_Y, 0.0));
    h.seconds(1.0);
    assert_eq!(h.count::<ImmolateBurnRig>(), 1);
    assert!(h.count::<ImmolateSpark>() > 0);
    h.dispel(victim);
    h.tick(2);
    assert_eq!(h.count::<ImmolateBurnRig>(), 0, "the rig outlived the DoT");
    assert_eq!(h.count::<ImmolateSpark>(), 0, "sparks outlived the DoT");
}

/// The fire dies with the victim (auras linger on corpses).
#[test]
fn death_puts_the_fire_out() {
    let mut h = Harness::new();
    let victim = h.spawn_burning(Vec3::new(0.0, COMBATANT_Y, 0.0));
    h.seconds(1.0);
    assert_eq!(h.count::<ImmolateBurnRig>(), 1, "the fire never started");
    assert!(h.count::<ImmolateSpark>() > 0, "the fire never emitted");
    h.app
        .world_mut()
        .get_mut::<Combatant>(victim)
        .unwrap()
        .current_health = 0.0;
    h.tick(2);
    assert_eq!(h.count::<ImmolateBurnRig>(), 0);
    assert_eq!(h.count::<ImmolateSpark>(), 0);
}

/// Only an Immolate DAMAGE-OVER-TIME aura lights the fire: another DoT does
/// not, and neither does a non-DoT aura that merely carries the name.
#[test]
fn only_the_immolate_dot_lights_the_fire() {
    let mut h = Harness::new();
    h.spawn_victim_with(
        Vec3::new(0.0, COMBATANT_Y, 0.0),
        vec![Harness::aura(AuraType::DamageOverTime, CORRUPTION_AURA)],
    );
    h.spawn_victim_with(
        Vec3::new(5.0, COMBATANT_Y, 0.0),
        vec![Harness::aura(AuraType::MovementSpeedSlow, IMMOLATE_AURA)],
    );
    h.seconds(0.5);
    assert_eq!(h.count::<ImmolateBurnRig>(), 0);

    // ...and the positive control in the same world.
    h.spawn_burning(Vec3::new(-5.0, COMBATANT_Y, 0.0));
    h.tick(1);
    assert_eq!(h.count::<ImmolateBurnRig>(), 1);
}
