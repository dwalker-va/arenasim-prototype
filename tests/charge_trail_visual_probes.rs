//! Probes for the charge speed-streak trail (Warrior Charge + Boar Charge).
//!
//! `spawn_charge_trail` used to filter `With<Pet>`, so the Boar's charge got a
//! streak and the Warrior's byte-identical gap-closer got nothing. These pin
//! that ANY `Added<ChargingState>` — pet or not — spawns a trail, and that the
//! streak's world-space orientation actually points along the charge (geometry,
//! not bookkeeping: a trail spawned but lying flat or aimed sideways would pass
//! any existence check).
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` — no window, no GPU.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::components::{ChargeTrail, ChargingState, Pet, PetType};
use arenasim::states::play_match::{spawn_charge_trail, update_and_cleanup_charge_trails};

const TICK: Duration = Duration::from_millis(50);

fn harness() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()));
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
    app.add_systems(
        Update,
        (spawn_charge_trail, update_and_cleanup_charge_trails).chain(),
    );
    app
}

fn trails(app: &mut App) -> Vec<Transform> {
    let mut q = app.world_mut().query::<(&ChargeTrail, &Transform)>();
    q.iter(app.world()).map(|(_, t)| *t).collect()
}

/// Streak assertions shared by both charger shapes: one trail, oriented along
/// the charge direction in world space, anchored at the charger.
fn assert_streak(app: &mut App, from: Vec3, to: Vec3) {
    let found = trails(app);
    assert_eq!(found.len(), 1, "exactly one streak per charge start");
    let tf = found[0];

    // The cylinder's long axis is local Y, rotated onto the charge direction.
    let axis = tf.rotation * Vec3::Y;
    let dir = (to - from).normalize();
    let off = axis.angle_between(dir);
    assert!(
        off < 0.05,
        "the streak lies {off:.2} rad off the charge direction {dir:?}"
    );

    let anchor = tf.translation - Vec3::Y * 0.3;
    assert!(
        anchor.distance(from) < 0.05,
        "the streak sits at {:?}, not at the charger {from:?}",
        tf.translation
    );
}

#[test]
fn a_charging_warrior_gets_a_speed_streak() {
    // The card's half of the fix: the Warrior inserts the same `ChargingState`
    // the Boar does (`class_ai/warrior.rs`), and under the old `With<Pet>`
    // filter its dash rendered nothing.
    let mut app = harness();
    let from = Vec3::new(0.0, 0.0, 0.0);
    let to = Vec3::new(8.0, 0.0, 6.0);
    let target = app.world_mut().spawn(Transform::from_translation(to)).id();
    app.world_mut().spawn((
        Transform::from_translation(from),
        ChargingState { target },
    ));
    app.update();
    assert_streak(&mut app, from, to);
}

#[test]
fn a_charging_pet_still_gets_its_streak() {
    // The Boar path the old filter served must keep working now the filter is
    // gone.
    let mut app = harness();
    let from = Vec3::new(2.0, 0.0, 1.0);
    let to = Vec3::new(-6.0, 0.0, 4.0);
    let target = app.world_mut().spawn(Transform::from_translation(to)).id();
    let owner = app.world_mut().spawn(Transform::default()).id();
    app.world_mut().spawn((
        Transform::from_translation(from),
        ChargingState { target },
        Pet {
            owner,
            pet_type: PetType::Boar,
        },
    ));
    app.update();
    assert_streak(&mut app, from, to);
}

#[test]
fn the_streak_fades_out_without_leaking() {
    let mut app = harness();
    let target = app.world_mut().spawn(Transform::from_xyz(5.0, 0.0, 0.0)).id();
    app.world_mut()
        .spawn((Transform::default(), ChargingState { target }));
    app.update();
    assert_eq!(trails(&mut app).len(), 1, "guard: the probe must not go vacuous");

    // Lifetime is 0.3s; a dozen 50ms ticks is far past it.
    for _ in 0..12 {
        app.update();
    }
    assert_eq!(trails(&mut app).len(), 0, "the streak must not leak");
}
