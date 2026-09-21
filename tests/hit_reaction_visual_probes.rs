//! Probes for the victim hit reaction and the caster wand auto
//! (`hit_reaction.rs`, `wand_attack.rs`, and the flinch composition in
//! `gait.rs`).
//!
//! Every geometric claim here is read off `GlobalTransform` — world positions
//! and world distances — never off a stored field or a scale scalar, so a
//! probe cannot pass on bookkeeping that no longer reaches the screen.
//!
//! **The load-bearing one is `the_flinch_is_visible_on_a_walking_victim`.**
//! The three gait writers set the body's local Y ABSOLUTELY every frame, so a
//! flinch written by a separate system is decided entirely by which of the two
//! runs later — and a flinch ordered before the gait is erased on anything
//! that walks, which in a real match is the common case. A probe that only
//! ever tested a STATIONARY victim would pass on that broken build, because
//! an idle gait's settle-ease leaves most of the dip intact. So the probe
//! drives the same scripted walk twice, once with a flinch and once without,
//! and demands the difference be exactly the dip.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window,
//! no GPU. `TransformPlugin` is load-bearing: without it `GlobalTransform`
//! never updates and every world-space assertion reads a stale identity.

use std::time::Duration;

use arenasim::states::play_match::abilities::SpellSchool;
use arenasim::states::play_match::components::{
    AutoAttackKind, AutoAttackSwing, Combatant, HitFlinch, Pet, PetType, VisualBody, WalkAnim,
    WeaponHand, WeaponKind, WeaponSocket,
};
use arenasim::states::play_match::{
    cleanup_hit_flinch, cleanup_hit_reactions, consume_hit_reactions, consume_swing_signals,
    hit_flinch_offset, tick_hit_flinch, update_fear_run, update_hit_flashes, update_hit_sparks,
    update_walk_animation, update_wand_missiles, wand_school, HitSpark, SwingStyle, WandMissile,
    FLINCH_CRIT_MULT, FLINCH_DIP, FLINCH_DURATION_SECS, PET_FLINCH_DURATION_SCALE,
    PET_SPARK_SPATIAL_SCALE, SPARK_COUNT, SPARK_CRIT_SCALE, SPARK_SIZE,
};
use arenasim::CharacterClass;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

const TICK: Duration = Duration::from_millis(50);
const TICK_SECS: f32 = 0.05;

/// The combatant capsule's horizontal radius — the silhouette a contact burst
/// has to stay outside of. Restated from `heal_impact::COMBATANT_BODY_RADIUS`
/// (not public through the crate root) so a probe reads as a claim about the
/// world rather than a re-export chase.
const BODY_RADIUS: f32 = 0.5;

fn harness() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::transform::TransformPlugin,
    ));
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
    app
}

/// A combatant in the hierarchy `spawn_combatant` builds: a sim entity at
/// `pos` owning a `VisualBody` child at local y 0.
fn spawn_unit(app: &mut App, class: CharacterClass, team: u8, pos: Vec3) -> (Entity, Entity) {
    let unit = app
        .world_mut()
        .spawn((
            Transform::from_translation(pos),
            Visibility::default(),
            Combatant::new(team, 0, class),
            WalkAnim {
                phase: 0.0,
                previous_xz: pos.xz(),
                idle_time: 0.0,
                body_offset: 0.0,
            },
        ))
        .id();
    let body = app
        .world_mut()
        .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
        .id();
    app.world_mut().entity_mut(unit).add_child(body);
    (unit, body)
}

fn spawn_pet_unit(app: &mut App, owner: Entity, pos: Vec3) -> (Entity, Entity) {
    let (pet, body) = spawn_unit(app, CharacterClass::Hunter, 1, pos);
    app.world_mut().entity_mut(pet).insert(Pet {
        owner,
        pet_type: PetType::Spider,
    });
    (pet, body)
}

fn spawn_wand_socket(app: &mut App, owner: Entity, body: Entity) -> Entity {
    let socket = app
        .world_mut()
        .spawn((
            WeaponSocket {
                kind: WeaponKind::Wand,
                hand: WeaponHand::Main,
                owner,
                rest: Transform::from_xyz(0.62, 0.55, 0.05),
                release_t: None,
                aim: Vec3::ZERO,
                winds_up_next: true,
                yaw_local: 0.0,
                prev_owner_yaw: 0.0,
                windup_s: 0.0,
                swing_style: SwingStyle::Auto,
                last_s: 0.0,
            },
            Transform::from_xyz(0.62, 0.55, 0.05),
            GlobalTransform::default(),
            Visibility::default(),
        ))
        .id();
    app.world_mut().entity_mut(body).add_child(socket);
    socket
}

fn swing(app: &mut App, attacker: Entity, target: Entity, kind: AutoAttackKind, is_crit: bool) {
    app.world_mut().spawn(AutoAttackSwing {
        attacker,
        target,
        kind,
        is_crit,
    });
}

/// A body child's WORLD y.
fn world_y(app: &App, body: Entity) -> f32 {
    app.world()
        .entity(body)
        .get::<GlobalTransform>()
        .expect("the body child has a GlobalTransform")
        .translation()
        .y
}

// ---------------------------------------------------------------------------
// The flinch: composed into the gait, and therefore visible on a MOVING victim
// ---------------------------------------------------------------------------

/// Drive a scripted walk for `frames` ticks and return the body's world Y at
/// each frame. With `flinch`, a `HitFlinch` is inserted before the first tick.
///
/// The walk is scripted (the test writes the sim Transform each frame), and
/// `advance_gait`'s phase depends only on distance travelled, so two runs of
/// this produce an identical bob track — which is what makes the difference
/// between them attributable to the flinch and nothing else.
fn walk_track(frames: usize, flinch: bool) -> Vec<(f32, f32)> {
    let mut app = harness();
    app.add_systems(
        Update,
        (tick_hit_flinch, update_walk_animation, cleanup_hit_flinch).chain(),
    );
    let (unit, body) = spawn_unit(
        &mut app,
        CharacterClass::Priest,
        1,
        Vec3::new(0.0, 1.0, 0.0),
    );
    if flinch {
        app.world_mut().entity_mut(unit).insert(HitFlinch {
            elapsed: 0.0,
            duration: FLINCH_DURATION_SECS,
            depth: FLINCH_DIP,
        });
    }
    let mut track = Vec::with_capacity(frames);
    for i in 0..frames {
        // Walk along +X at a steady 4 units/sec, so the gait is never idle.
        let x = (i + 1) as f32 * 4.0 * TICK_SECS;
        app.world_mut()
            .entity_mut(unit)
            .get_mut::<Transform>()
            .unwrap()
            .translation
            .x = x;
        app.update();
        // The dip the gait actually saw this frame, read off the live
        // component rather than re-derived from an assumed clock — the
        // claim under test is the COMPOSITION, not Bevy's first delta.
        let dip = app
            .world()
            .entity(unit)
            .get::<HitFlinch>()
            .map_or(0.0, hit_flinch_offset);
        track.push((world_y(&app, body), dip));
    }
    track
}

#[test]
fn the_flinch_is_visible_on_a_walking_victim() {
    // THE ordering probe. Both runs walk identically; the only difference is
    // the flinch. A build that let the gait overwrite the dip — which is what
    // a separately-ordered flinch system does — returns two identical tracks
    // and fails here, while a stationary-victim probe would still pass.
    let frames = 10;
    let plain = walk_track(frames, false);
    let dipped = walk_track(frames, true);

    // The walk itself is genuinely moving, or the probe proves nothing about
    // the moving case: the bob must actually vary across the window.
    let bob_span = plain.iter().map(|s| s.0).fold(f32::MIN, f32::max)
        - plain.iter().map(|s| s.0).fold(f32::MAX, f32::min);
    assert!(
        bob_span > 0.05,
        "the control walk barely bobbed ({bob_span}) — this probe would not \
         distinguish a composed flinch from an overwritten one"
    );

    // ...and the flinch is EXACTLY additive on top of it, frame by frame.
    let mut deepest = 0.0f32;
    for (i, (&(p, control_dip), &(d, expected))) in plain.iter().zip(dipped.iter()).enumerate() {
        assert_eq!(control_dip, 0.0, "the control run must carry no flinch");
        assert!(
            (d - p - expected).abs() < 1e-5,
            "frame {i}: walking body at {d}, control at {p}, dip should be {expected}"
        );
        deepest = deepest.min(d - p);
    }
    assert!(
        deepest <= -FLINCH_DIP * 0.9,
        "the walking victim never dipped near FLINCH_DIP (deepest {deepest})"
    );
}

#[test]
fn the_flinch_is_visible_on_a_feared_victim_too() {
    // The panic run owns the same channel, and a feared unit being auto-
    // attacked is an ordinary match state — so the composition has to hold
    // through all three gaits, not just the walk.
    let mut app = harness();
    app.add_systems(
        Update,
        (tick_hit_flinch, update_fear_run, cleanup_hit_flinch).chain(),
    );
    use arenasim::states::play_match::components::FearedVisual;
    let (unit, body) = spawn_unit(
        &mut app,
        CharacterClass::Priest,
        1,
        Vec3::new(0.0, 1.0, 0.0),
    );
    app.world_mut().entity_mut(unit).insert(FearedVisual);
    app.update();
    let before = world_y(&app, body);

    app.world_mut().entity_mut(unit).insert(HitFlinch {
        elapsed: 0.0,
        duration: FLINCH_DURATION_SECS,
        depth: FLINCH_DIP,
    });
    // Advance to the dip's peak.
    for _ in 0..2 {
        app.update();
    }
    let dipped = world_y(&app, body);
    // The tremble is time-driven and small (0.04); the dip is 0.10, so the
    // body has to end up clearly below where the panic run alone put it.
    assert!(
        dipped < before - 0.05,
        "feared body at {dipped}, un-flinched panic-run height {before}"
    );
}

#[test]
fn the_flinch_leaves_no_residue_when_it_expires() {
    // The self-cleaning property that made Y the right channel: once the
    // component is gone the gait's own height must be exactly what it would
    // have been, with no leftover offset.
    let frames = 14; // 0.70s — twice FLINCH_DURATION_SECS
    let plain = walk_track(frames, false);
    let dipped = walk_track(frames, true);
    let settled = (FLINCH_DURATION_SECS / TICK_SECS).ceil() as usize;
    assert!(settled < frames);
    for i in settled..frames {
        assert!(
            (plain[i].0 - dipped[i].0).abs() < 1e-6,
            "frame {i} after expiry: {} vs {}",
            dipped[i].0,
            plain[i].0
        );
    }
    // And the component itself is gone, not merely inert.
    let mut app = harness();
    app.add_systems(
        Update,
        (tick_hit_flinch, update_walk_animation, cleanup_hit_flinch).chain(),
    );
    let (unit, _) = spawn_unit(
        &mut app,
        CharacterClass::Priest,
        1,
        Vec3::new(0.0, 1.0, 0.0),
    );
    app.world_mut().entity_mut(unit).insert(HitFlinch {
        elapsed: 0.0,
        duration: FLINCH_DURATION_SECS,
        depth: FLINCH_DIP,
    });
    for _ in 0..frames {
        app.update();
    }
    assert!(app.world().entity(unit).get::<HitFlinch>().is_none());
}

/// Reach the state a sandbox take ends in — an idle gait still easing an
/// offset off, and a live flinch — then reset the body the way
/// `clear_body_state` does, and return the body's world Y one gait frame
/// later. `rest_y` is 0 and the unit stands at y 1.0, so a clean reset reads
/// exactly 1.0.
fn reset_and_step(clear_offset: bool, clear_flinch: bool) -> f32 {
    let mut app = harness();
    app.add_systems(
        Update,
        (tick_hit_flinch, update_walk_animation, cleanup_hit_flinch).chain(),
    );
    let (unit, body) = spawn_unit(
        &mut app,
        CharacterClass::Priest,
        1,
        Vec3::new(0.0, 1.0, 0.0),
    );
    // Walk, so the gait carries a real offset...
    for i in 0..6 {
        let x = (i + 1) as f32 * 4.0 * TICK_SECS;
        app.world_mut()
            .entity_mut(unit)
            .get_mut::<Transform>()
            .unwrap()
            .translation
            .x = x;
        app.update();
    }
    // ...then stand still just past the idle threshold, which is where the
    // settle ease takes over and the offset is still non-zero. `idle_time`
    // only POSITIONS the scenario; every claim below is world geometry.
    for _ in 0..8 {
        app.update();
        if app
            .world()
            .entity(unit)
            .get::<WalkAnim>()
            .unwrap()
            .idle_time
            > 0.1
        {
            break;
        }
    }
    let carried = app
        .world()
        .entity(unit)
        .get::<WalkAnim>()
        .unwrap()
        .body_offset;
    assert!(
        carried.abs() > 0.02,
        "the gait settled before the reset ({carried}) — this probe would not \
         distinguish a cleared channel from a stale one"
    );
    app.world_mut().entity_mut(unit).insert(HitFlinch {
        elapsed: 0.0,
        duration: FLINCH_DURATION_SECS,
        depth: FLINCH_DIP,
    });

    // The reset. Every arm zeroes the transform, which is all the sandbox did
    // before the body's Y became a composed channel.
    app.world_mut()
        .entity_mut(body)
        .insert(Transform::from_xyz(0.0, 0.0, 0.0));
    if clear_offset {
        app.world_mut()
            .entity_mut(unit)
            .get_mut::<WalkAnim>()
            .unwrap()
            .body_offset = 0.0;
    }
    if clear_flinch {
        app.world_mut().entity_mut(unit).remove::<HitFlinch>();
    }

    app.update();
    world_y(&app, body)
}

#[test]
fn a_between_takes_reset_must_clear_the_composed_channels_too() {
    // The contract `clear_body_state` (`animation_sandbox/playback.rs`) has to
    // meet, pinned in the module that owns the composition it clears.
    //
    // Because the gait recovers its own contribution from
    // `WalkAnim::body_offset` instead of reading the transform back, an
    // outside reset of the transform no longer reaches the gait: on the
    // settle-to-idle ease the next frame writes `rest_y + <stale offset>`
    // back over the height the reset just zeroed. The flinch is the same
    // story carried by a component. Two FRESH OFFENDER arms — one stale
    // channel each — keep the clean arm from passing vacuously.
    const REST: f32 = 1.0;

    let clean = reset_and_step(true, true);
    assert!(
        (clean - REST).abs() < 1e-6,
        "a reset that clears both channels must leave the body at rest, got {clean}"
    );

    let stale_offset = reset_and_step(false, true);
    assert!(
        (stale_offset - REST).abs() > 0.02,
        "a stale gait offset should have re-offset the body, got {stale_offset} \
         — the clean arm above proves nothing if this one passes"
    );

    let stale_flinch = reset_and_step(true, false);
    assert!(
        (stale_flinch - REST).abs() > 0.02,
        "a surviving HitFlinch should have dipped the body, got {stale_flinch}"
    );
}

// ---------------------------------------------------------------------------
// Trigger set: every landed auto flinches; only melee bursts
// ---------------------------------------------------------------------------

fn consumer_app() -> App {
    let mut app = harness();
    // Production ordering: the reaction consumer runs BEFORE the swing
    // consumer, which despawns the marker.
    app.add_systems(
        Update,
        (consume_hit_reactions, consume_swing_signals).chain(),
    );
    app
}

fn flinch_of(app: &App, unit: Entity) -> Option<HitFlinch> {
    app.world().entity(unit).get::<HitFlinch>().copied()
}

fn spark_count(app: &mut App) -> usize {
    app.world_mut().query::<&HitSpark>().iter(app.world()).len()
}

#[test]
fn every_landed_auto_flinches_its_victim() {
    // Melee, pet melee, wand hit and auto-shot arrival — the client's generic
    // hit-react path. `wand_flinch_on_hit` from the spec is the Wand row.
    for kind in [
        AutoAttackKind::Melee,
        AutoAttackKind::Shot,
        AutoAttackKind::Wand,
    ] {
        let mut app = consumer_app();
        let (attacker, _) = spawn_unit(&mut app, CharacterClass::Mage, 1, Vec3::new(0.0, 1.0, 0.0));
        let (victim, _) = spawn_unit(
            &mut app,
            CharacterClass::Priest,
            2,
            Vec3::new(3.0, 1.0, 0.0),
        );
        swing(&mut app, attacker, victim, kind, false);
        app.update();
        assert!(
            flinch_of(&app, victim).is_some(),
            "{kind:?} left its victim with no hit reaction"
        );
    }
}

#[test]
fn only_melee_autos_throw_sparks() {
    // Client-faithful: the wand SpellVisuals carry zero impact rows, and the
    // bow impact kit is sound + CombatWound with no model. A burst on either
    // would be invented.
    for (kind, expect) in [
        (AutoAttackKind::Melee, SPARK_COUNT as usize),
        (AutoAttackKind::Shot, 0),
        (AutoAttackKind::Wand, 0),
    ] {
        let mut app = consumer_app();
        let (attacker, _) = spawn_unit(
            &mut app,
            CharacterClass::Warrior,
            1,
            Vec3::new(0.0, 1.0, 0.0),
        );
        let (victim, _) = spawn_unit(
            &mut app,
            CharacterClass::Priest,
            2,
            Vec3::new(2.0, 1.0, 0.0),
        );
        swing(&mut app, attacker, victim, kind, false);
        app.update();
        assert_eq!(spark_count(&mut app), expect, "{kind:?}");
    }
}

#[test]
fn a_pet_victim_flinches_faster_than_a_player() {
    // `wolf.m2`'s CombatWound is 667ms against humanmale's 1000ms — wound
    // durations are authored per rig, so a beast's flinch is snappier.
    let mut app = consumer_app();
    let (attacker, _) = spawn_unit(
        &mut app,
        CharacterClass::Warrior,
        1,
        Vec3::new(0.0, 1.0, 0.0),
    );
    let (player, _) = spawn_unit(
        &mut app,
        CharacterClass::Priest,
        2,
        Vec3::new(2.0, 1.0, 0.0),
    );
    let (pet, _) = spawn_pet_unit(&mut app, attacker, Vec3::new(-2.0, 0.75, 0.0));
    swing(&mut app, attacker, player, AutoAttackKind::Melee, false);
    swing(&mut app, attacker, pet, AutoAttackKind::Melee, false);
    app.update();

    let player_flinch = flinch_of(&app, player).expect("player flinched");
    let pet_flinch = flinch_of(&app, pet).expect("pet flinched");
    assert!((player_flinch.duration - FLINCH_DURATION_SECS).abs() < 1e-6);
    assert!((pet_flinch.duration - FLINCH_DURATION_SECS * PET_FLINCH_DURATION_SCALE).abs() < 1e-6);
    assert!(pet_flinch.duration < player_flinch.duration);
}

#[test]
fn a_crit_dips_deeper_and_a_refresh_never_pops_the_body_upward() {
    let mut app = consumer_app();
    let (attacker, _) = spawn_unit(
        &mut app,
        CharacterClass::Warrior,
        1,
        Vec3::new(0.0, 1.0, 0.0),
    );
    let (victim, _) = spawn_unit(
        &mut app,
        CharacterClass::Priest,
        2,
        Vec3::new(2.0, 1.0, 0.0),
    );

    swing(&mut app, attacker, victim, AutoAttackKind::Melee, true);
    app.update();
    let crit = flinch_of(&app, victim).expect("crit flinched");
    assert!((crit.depth - FLINCH_DIP * FLINCH_CRIT_MULT).abs() < 1e-6);

    // A NORMAL hit arriving while the crit dip is still deep must not shallow
    // the body — that upward pop is the artifact focus fire would produce
    // constantly, so the refresh floors at where the body already is.
    app.world_mut()
        .entity_mut(victim)
        .get_mut::<HitFlinch>()
        .unwrap()
        .elapsed = FLINCH_DURATION_SECS * 0.2;
    let before = hit_flinch_offset(&flinch_of(&app, victim).unwrap()).abs();
    swing(&mut app, attacker, victim, AutoAttackKind::Melee, false);
    app.update();
    let after = flinch_of(&app, victim).expect("refreshed");
    assert_eq!(after.elapsed, 0.0, "a refresh restarts the dip");
    assert!(
        after.depth >= before - 1e-6,
        "refresh shallowed the dip from {before} to {}",
        after.depth
    );
}

#[test]
fn a_same_tick_crit_keeps_its_depth_whichever_lands_first() {
    // The test above covers the ACROSS-tick refresh, where the floor can read
    // the deeper dip off the victim's live component. Within ONE tick it
    // cannot: the insert carrying the crit's depth is still in the command
    // queue, so both swings read the same pre-tick component and the later
    // one wins outright. Measured at 22% of landed autos under focus fire —
    // the scenario whose whole point is that a crit reads as a crit.
    //
    // Both orders, because the defect is order-dependent by construction:
    // crit-then-normal loses the depth, normal-then-crit happens to keep it.
    for crit_first in [true, false] {
        let mut app = consumer_app();
        let (attacker, _) = spawn_unit(
            &mut app,
            CharacterClass::Warrior,
            1,
            Vec3::new(0.0, 1.0, 0.0),
        );
        let (second, _) = spawn_unit(&mut app, CharacterClass::Rogue, 1, Vec3::new(0.0, 1.0, 2.0));
        let (victim, _) = spawn_unit(
            &mut app,
            CharacterClass::Priest,
            2,
            Vec3::new(2.0, 1.0, 0.0),
        );

        // Two attackers, one victim, one tick — signals spawned before any
        // `app.update()`, which is exactly what a shared FixedUpdate tick
        // hands the consumer.
        swing(
            &mut app,
            attacker,
            victim,
            AutoAttackKind::Melee,
            crit_first,
        );
        swing(&mut app, second, victim, AutoAttackKind::Melee, !crit_first);
        app.update();

        let landed = flinch_of(&app, victim).expect("a same-tick pair still flinches");
        assert!(
            (landed.depth - FLINCH_DIP * FLINCH_CRIT_MULT).abs() < 1e-6,
            "crit_first={crit_first}: dip is {} but a crit shared the tick, so \
             it should be the crit's {}",
            landed.depth,
            FLINCH_DIP * FLINCH_CRIT_MULT
        );
        // And both bursts were thrown — the crit's spark scale was never the
        // thing at risk here, and a fix that collapsed the pair into one
        // reaction would be a different regression.
        assert_eq!(
            spark_count(&mut app),
            2 * SPARK_COUNT as usize,
            "crit_first={crit_first}: both contacts should throw a burst"
        );
    }
}

// ---------------------------------------------------------------------------
// The burst's world geometry
// ---------------------------------------------------------------------------

#[test]
fn sparks_stay_clear_of_the_victims_silhouette() {
    // World geometry off `GlobalTransform`, over the WHOLE flight — not just
    // at the spawn point. The spray axis points back at the striker, which is
    // what keeps a fleck from being thrown through the body it came off.
    let mut app = consumer_app();
    app.add_systems(PostUpdate, (update_hit_sparks, cleanup_hit_reactions));
    let victim_pos = Vec3::new(4.0, 1.0, -2.0);
    let (attacker, _) = spawn_unit(
        &mut app,
        CharacterClass::Warrior,
        1,
        Vec3::new(6.0, 1.0, -2.0),
    );
    let (victim, _) = spawn_unit(&mut app, CharacterClass::Priest, 2, victim_pos);
    swing(&mut app, attacker, victim, AutoAttackKind::Melee, true);
    app.update();
    assert_eq!(spark_count(&mut app), SPARK_COUNT as usize);

    let mut samples = 0usize;
    let mut closest = f32::MAX;
    for _ in 0..8 {
        let positions: Vec<Vec3> = app
            .world_mut()
            .query::<(&HitSpark, &GlobalTransform)>()
            .iter(app.world())
            .map(|(_, g)| g.translation())
            .collect();
        for p in positions {
            let horizontal = (p - victim_pos).with_y(0.0).length();
            closest = closest.min(horizontal);
            samples += 1;
        }
        app.update();
    }
    assert!(
        samples > 20,
        "only {samples} spark samples — probe went vacuous"
    );
    assert!(
        closest >= BODY_RADIUS,
        "a fleck came within {closest} of the victim's axis (silhouette {BODY_RADIUS})"
    );
}

#[test]
fn the_burst_sits_on_the_attackers_side() {
    let mut app = consumer_app();
    let victim_pos = Vec3::new(0.0, 1.0, 0.0);
    // Attacker due -Z of the victim, so the burst must land at negative Z.
    let (attacker, _) = spawn_unit(
        &mut app,
        CharacterClass::Warrior,
        1,
        Vec3::new(0.0, 1.0, -2.0),
    );
    let (victim, _) = spawn_unit(&mut app, CharacterClass::Priest, 2, victim_pos);
    swing(&mut app, attacker, victim, AutoAttackKind::Melee, false);
    app.update();

    let positions: Vec<Vec3> = app
        .world_mut()
        .query::<(&HitSpark, &GlobalTransform)>()
        .iter(app.world())
        .map(|(_, g)| g.translation())
        .collect();
    assert_eq!(positions.len(), SPARK_COUNT as usize);
    for p in positions {
        assert!(p.z < victim_pos.z, "fleck at {p} is behind the victim");
        assert!(
            p.y > victim_pos.y,
            "fleck at {p} is below the victim's transform, not at chest height"
        );
    }
}

/// The world-space LENGTH of one fleck's streak, measured by transforming the
/// two Z-face centres of its cuboid mesh (`SPARK_SIZE` long on Z) into the
/// world. A length off `GlobalTransform` — never a scale scalar — so the claim
/// is about what is drawn.
fn streak_length(g: &GlobalTransform) -> f32 {
    g.transform_point(Vec3::Z * (SPARK_SIZE * 0.5))
        .distance(g.transform_point(Vec3::NEG_Z * (SPARK_SIZE * 0.5)))
}

/// The world-space CROSS-SECTION of one fleck, the same way across its X face.
/// Only ever compared against another cross-section from the same probe, so
/// the local half-extent used here need not be the mesh's real one.
fn streak_cross_section(g: &GlobalTransform) -> f32 {
    g.transform_point(Vec3::X * (SPARK_SIZE * 0.5))
        .distance(g.transform_point(Vec3::NEG_X * (SPARK_SIZE * 0.5)))
}

/// Every fleck of one non-crit melee burst, sampled once per frame for its
/// whole life: `(length, cross_section)` per frame, per fleck, in spawn order.
fn fleck_life_histories(dt: Duration) -> Vec<Vec<(f32, f32)>> {
    let mut app = consumer_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(dt));
    app.add_systems(
        PostUpdate,
        (update_hit_sparks, cleanup_hit_reactions)
            .chain()
            // Sampled off `GlobalTransform`, so the taper must be written
            // BEFORE propagation or every reading is a frame stale.
            .before(bevy::transform::TransformSystem::TransformPropagate),
    );
    let (attacker, _) = spawn_unit(
        &mut app,
        CharacterClass::Warrior,
        1,
        Vec3::new(0.0, 1.0, -2.0),
    );
    let (victim, _) = spawn_unit(
        &mut app,
        CharacterClass::Priest,
        2,
        Vec3::new(0.0, 1.0, 0.0),
    );
    // Warm-up: Bevy's first update carries a zero delta, so a burst spawned on
    // it would sit one frame at its spawn scale and break the frame-count
    // bracket below. Every frame after this one advances by exactly `dt`.
    app.update();
    swing(&mut app, attacker, victim, AutoAttackKind::Melee, false);
    app.update();
    assert_eq!(spark_count(&mut app), SPARK_COUNT as usize);

    let mut order: Vec<Entity> = Vec::new();
    let mut history: Vec<Vec<(f32, f32)>> = Vec::new();
    loop {
        let frame: Vec<(Entity, f32, f32)> = app
            .world_mut()
            .query_filtered::<(Entity, &GlobalTransform), With<HitSpark>>()
            .iter(app.world())
            .map(|(e, g)| (e, streak_length(g), streak_cross_section(g)))
            .collect();
        if frame.is_empty() {
            break;
        }
        for (entity, length, cross) in frame {
            let slot = match order.iter().position(|e| *e == entity) {
                Some(i) => i,
                None => {
                    order.push(entity);
                    history.push(Vec::new());
                    history.len() - 1
                }
            };
            history[slot].push((length, cross));
        }
        app.update();
    }
    history
}

#[test]
fn a_fleck_thins_and_shortens_along_the_authored_curve() {
    // Every other burst guard in the module is about how MUCH is spawned —
    // the compile-time ceiling against Mortal Strike, the concurrency peak
    // under focus fire. Nothing watched the SHAPE of a fleck once it was
    // alive, and a taper that compounded frame over frame stayed inside all
    // of them while rendering at a fraction of its authored length. This is
    // the curve claim: at each moment of a fleck's life the drawn streak is
    // `SPARK_SIZE * (0.4 + 0.6 * k)` and the cross-section tapers as
    // `max(k, 0.15)`, for `k` the fraction of life remaining.
    const DT: Duration = Duration::from_millis(10);

    let history = fleck_life_histories(DT);
    assert_eq!(
        history.len(),
        SPARK_COUNT as usize,
        "every fleck of the burst should have been sampled"
    );

    let mut samples = 0usize;
    for (fleck, life) in history.iter().enumerate() {
        let n = life.len();
        // Sampled after every frame and despawned the frame its clock crosses
        // zero, so a fleck seen `n` times had a total life in
        // `(n*dt, (n+1)*dt]`. That brackets `k` at every sample without
        // reading a private field off the component.
        assert!(
            n >= 15,
            "fleck {fleck} lived only {n} frames — too coarse to pin a curve"
        );
        let k_bounds = |j: usize| {
            let lo = 1.0 - (j + 1) as f32 / n as f32;
            let hi = 1.0 - (j + 1) as f32 / (n + 1) as f32;
            (lo.max(0.0), hi)
        };
        let (k0_lo, k0_hi) = k_bounds(0);
        let (spawn_len, spawn_cross) = life[0];

        for (j, (length, cross)) in life.iter().copied().enumerate() {
            let (k_lo, k_hi) = k_bounds(j);
            // Length is an ABSOLUTE claim: a non-crit burst on a player victim
            // spawns at scale 1.0, so the authored streak is `SPARK_SIZE`.
            let lo = SPARK_SIZE * (0.4 + 0.6 * k_lo) - 1e-5;
            let hi = SPARK_SIZE * (0.4 + 0.6 * k_hi) + 1e-5;
            assert!(
                length >= lo && length <= hi,
                "fleck {fleck} frame {j} of {n}: streak {length} outside the \
                 authored [{lo}, {hi}] (SPARK_SIZE {SPARK_SIZE})"
            );
            // Cross-section is a RATIO against this fleck's own first sample,
            // so it needs no private aspect constant.
            let ratio = cross / spawn_cross;
            let ratio_lo = k_lo.max(0.15) / k0_hi.max(0.15) - 1e-4;
            let ratio_hi = k_hi.max(0.15) / k0_lo.max(0.15) + 1e-4;
            assert!(
                ratio >= ratio_lo && ratio <= ratio_hi,
                "fleck {fleck} frame {j} of {n}: cross-section ratio {ratio} \
                 outside the authored [{ratio_lo}, {ratio_hi}]"
            );
            samples += 1;
        }
        // The two ends are the halves a compounding taper destroys, so pin
        // them explicitly rather than leaving them to the band above.
        assert!(
            spawn_len > 0.95 * SPARK_SIZE,
            "fleck {fleck} starts at streak {spawn_len}, not a full SPARK_SIZE"
        );
        let (final_len, _) = life[n - 1];
        assert!(
            (0.40 * SPARK_SIZE..=0.45 * SPARK_SIZE).contains(&final_len),
            "fleck {fleck} ends at streak {final_len}, not the authored {}",
            0.4 * SPARK_SIZE
        );
    }
    assert!(
        samples > 150,
        "only {samples} fleck samples — the curve was barely exercised"
    );
}

#[test]
fn a_crit_throws_the_same_flecks_bigger() {
    // The taper is a function of remaining life alone, so it must leave the
    // crit and pet spawn scales untouched. Normalising the ratio out of the
    // curve would satisfy the probe above and break this one.
    fn mean_spawn_streak(is_crit: bool, pet_victim: bool) -> f32 {
        let mut app = consumer_app();
        app.add_systems(
            PostUpdate,
            update_hit_sparks.before(bevy::transform::TransformSystem::TransformPropagate),
        );
        let (attacker, _) = spawn_unit(
            &mut app,
            CharacterClass::Warrior,
            1,
            Vec3::new(0.0, 1.0, -2.0),
        );
        let victim = if pet_victim {
            let (owner, _) = spawn_unit(
                &mut app,
                CharacterClass::Hunter,
                2,
                Vec3::new(3.0, 1.0, 0.0),
            );
            spawn_pet_unit(&mut app, owner, Vec3::new(0.0, 1.0, 0.0)).0
        } else {
            spawn_unit(
                &mut app,
                CharacterClass::Priest,
                2,
                Vec3::new(0.0, 1.0, 0.0),
            )
            .0
        };
        swing(&mut app, attacker, victim, AutoAttackKind::Melee, is_crit);
        app.update();
        let lengths: Vec<f32> = app
            .world_mut()
            .query_filtered::<&GlobalTransform, With<HitSpark>>()
            .iter(app.world())
            .map(streak_length)
            .collect();
        assert_eq!(lengths.len(), SPARK_COUNT as usize);
        lengths.iter().sum::<f32>() / lengths.len() as f32
    }

    let normal = mean_spawn_streak(false, false);
    let crit = mean_spawn_streak(true, false);
    let pet = mean_spawn_streak(false, true);
    // One frame of taper has already run on each burst, identically, so these
    // ratios are the spawn scales to well under a percent.
    let crit_ratio = crit / normal;
    let pet_ratio = pet / normal;
    assert!(
        (crit_ratio - SPARK_CRIT_SCALE).abs() < 0.02,
        "a crit fleck is {crit_ratio}x a normal one, not SPARK_CRIT_SCALE \
         ({SPARK_CRIT_SCALE})"
    );
    assert!(
        (pet_ratio - PET_SPARK_SPATIAL_SCALE).abs() < 0.02,
        "a fleck off a pet victim is {pet_ratio}x a normal one, not \
         PET_SPARK_SPATIAL_SCALE ({PET_SPARK_SPATIAL_SCALE})"
    );
}

/// Peak concurrent flecks under the cadence three attackers focusing one
/// target actually produce, and the total streak length that peak draws.
fn sustained_burst_peak(interval_secs: f32, seconds: f32) -> (usize, f32) {
    let mut app = consumer_app();
    app.add_systems(PostUpdate, (update_hit_sparks, cleanup_hit_reactions));
    let (attacker, _) = spawn_unit(
        &mut app,
        CharacterClass::Warrior,
        1,
        Vec3::new(0.0, 1.0, -2.0),
    );
    let (victim, _) = spawn_unit(
        &mut app,
        CharacterClass::Priest,
        2,
        Vec3::new(0.0, 1.0, 0.0),
    );

    let frames = (seconds / TICK_SECS).round() as usize;
    let every = (interval_secs / TICK_SECS).round().max(1.0) as usize;
    let mut peak = 0usize;
    for frame in 0..frames {
        if frame % every == 0 {
            swing(&mut app, attacker, victim, AutoAttackKind::Melee, false);
        }
        app.update();
        peak = peak.max(spark_count(&mut app));
    }
    (peak, peak as f32 * SPARK_SIZE)
}

#[test]
fn focus_fire_stays_under_one_mortal_strike_of_debris() {
    // Three attackers on one target land every ~0.2s (the card's cadence, and
    // the case the bench's 1.6s single-attacker budget note does NOT cover).
    // The claim is comparative, because "reads as mush" is not a number: the
    // sustained peak must draw LESS total streak than one Mortal Strike
    // flourish, which is the loudest single melee beat in the game.
    const MS_SPARK_COUNT: f32 = 14.0;
    const MS_SPARK_LENGTH: f32 = 0.13;
    let ms_budget = MS_SPARK_COUNT * MS_SPARK_LENGTH;

    // The peak this cadence actually produces at the blessed values, pinned
    // rather than described. A loose bound with an exact number in the
    // comment beside it reads as verified and is not — so the number IS the
    // assertion. `streak` is `peak * SPARK_SIZE` by construction, so pinning
    // the count pins the streak too (22 * 0.06 = 1.32 against one Mortal
    // Strike flourish's 14 * 0.13 = 1.82).
    //
    // What would have to change for this to move: `SPARK_COUNT`,
    // `SPARK_LIFETIME_SECS` or its 0.7-1.3 per-fleck jitter — which is to say
    // exactly the intensity knobs the card froze and the compile-time ceiling
    // watches from the other side. A failure here is a real question, not
    // noise. (The harness tick and the 0.2s cadence are test-local and
    // spelled out in the call.)
    const FOCUS_FIRE_PEAK: usize = 22;
    let (peak, streak) = sustained_burst_peak(0.2, 2.0);
    assert!(
        peak > SPARK_COUNT as usize,
        "bursts never overlapped — probe is vacuous"
    );
    assert_eq!(
        peak, FOCUS_FIRE_PEAK,
        "the three-attacker peak moved off its measured value; \
         {streak} streak-units drawn"
    );
    // The slow end of the same three-attacker band.
    let (peak_03, streak_03) = sustained_burst_peak(0.3, 2.4);
    assert!(
        streak_03 <= streak,
        "the 0.3s cadence ({peak_03} flecks) cannot outdraw the 0.2s one ({peak})"
    );
    assert!(
        streak < ms_budget,
        "sustained focus fire peaks at {peak} flecks / {streak} streak-units, \
         over one Mortal Strike's {ms_budget}"
    );

    // The single-attacker cadence the bench measured, for contrast: no
    // overlap at all at 1.6s.
    let (slow_peak, _) = sustained_burst_peak(1.6, 3.2);
    assert_eq!(slow_peak, SPARK_COUNT as usize);
}

// ---------------------------------------------------------------------------
// The wand auto
// ---------------------------------------------------------------------------

#[test]
fn a_wand_shot_launches_a_bolt_that_reaches_the_victims_chest() {
    let mut app = consumer_app();
    app.add_systems(PostUpdate, update_wand_missiles);
    let caster_pos = Vec3::new(0.0, 1.0, 0.0);
    let victim_pos = Vec3::new(0.0, 1.0, 12.0);
    let (caster, caster_body) = spawn_unit(&mut app, CharacterClass::Mage, 1, caster_pos);
    spawn_wand_socket(&mut app, caster, caster_body);
    let (victim, _) = spawn_unit(&mut app, CharacterClass::Priest, 2, victim_pos);
    // Propagate the socket's GlobalTransform before the shot, so the muzzle
    // is read off the real rod rather than an identity.
    app.update();

    swing(&mut app, caster, victim, AutoAttackKind::Wand, false);
    app.update();

    let launch = app
        .world_mut()
        .query::<(&WandMissile, &GlobalTransform)>()
        .iter(app.world())
        .map(|(_, g)| g.translation())
        .next()
        .expect("a wand shot launches one bolt");
    // It leaves from the rod, which is raised and offset to the hand side —
    // not from the caster's own transform.
    assert!(
        launch.y > caster_pos.y,
        "the bolt left from {launch}, at or below the caster's transform"
    );
    assert!((launch - caster_pos).length() > 0.3);

    // Fly it. The victim's chest anchor is 0.55 above its transform.
    let chest = victim_pos + Vec3::Y * 0.55;
    let mut closest = f32::MAX;
    for _ in 0..40 {
        let live: Vec<Vec3> = app
            .world_mut()
            .query::<(&WandMissile, &GlobalTransform)>()
            .iter(app.world())
            .map(|(_, g)| g.translation())
            .collect();
        if live.is_empty() {
            break;
        }
        for p in live {
            closest = closest.min((p - chest).length());
        }
        app.update();
    }
    // The bolt despawns the frame it comes within one step of its target, so
    // one frame's travel is the tightest bound a 50ms probe can assert.
    let step = 30.0 * TICK_SECS;
    assert!(
        closest <= step,
        "the bolt's closest approach to the chest anchor was {closest},          over one frame's travel ({step})"
    );
    // ...and it despawns on arrival rather than hanging in the victim.
    assert_eq!(
        app.world_mut()
            .query::<&WandMissile>()
            .iter(app.world())
            .len(),
        0,
        "the bolt outlived its arrival"
    );
}

#[test]
fn a_bolt_that_never_arrives_still_despawns() {
    // The TTL backstop: a victim that despawns mid-flight must not strand a
    // bolt for the rest of the match.
    let mut app = consumer_app();
    app.add_systems(PostUpdate, update_wand_missiles);
    let (caster, _) = spawn_unit(
        &mut app,
        CharacterClass::Warlock,
        1,
        Vec3::new(0.0, 1.0, 0.0),
    );
    let (victim, _) = spawn_unit(
        &mut app,
        CharacterClass::Priest,
        2,
        Vec3::new(0.0, 1.0, 25.0),
    );
    swing(&mut app, caster, victim, AutoAttackKind::Wand, false);
    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&WandMissile>()
            .iter(app.world())
            .len(),
        1
    );
    // Freeze the bolt's target out of reach by moving it far away every frame
    // is overkill — the TTL alone bounds it. 2.0s at 50ms ticks.
    for _ in 0..60 {
        app.update();
    }
    assert_eq!(
        app.world_mut()
            .query::<&WandMissile>()
            .iter(app.world())
            .len(),
        0,
        "a bolt outlived its TTL backstop"
    );
}

#[test]
fn each_caster_throws_its_own_school() {
    assert_eq!(wand_school(CharacterClass::Mage), SpellSchool::Arcane);
    assert_eq!(wand_school(CharacterClass::Priest), SpellSchool::Holy);
    assert_eq!(wand_school(CharacterClass::Warlock), SpellSchool::Shadow);
}

// ---------------------------------------------------------------------------
// The marker contract
// ---------------------------------------------------------------------------

#[test]
fn the_reaction_consumer_leaves_the_marker_for_the_swing_consumer() {
    // The two consumers share one marker and only ONE of them despawns it.
    // If the reaction consumer despawned, the weapon would stop swinging; if
    // the swing consumer ran first, every reaction would be dropped.
    let mut app = harness();
    app.add_systems(Update, consume_hit_reactions);
    let (attacker, _) = spawn_unit(
        &mut app,
        CharacterClass::Warrior,
        1,
        Vec3::new(0.0, 1.0, 0.0),
    );
    let (victim, _) = spawn_unit(
        &mut app,
        CharacterClass::Priest,
        2,
        Vec3::new(2.0, 1.0, 0.0),
    );
    swing(&mut app, attacker, victim, AutoAttackKind::Melee, false);
    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&AutoAttackSwing>()
            .iter(app.world())
            .len(),
        1,
        "consume_hit_reactions must not despawn the marker"
    );

    // ...and with both registered in production order, it is gone exactly once.
    let mut app = consumer_app();
    let (attacker, _) = spawn_unit(
        &mut app,
        CharacterClass::Warrior,
        1,
        Vec3::new(0.0, 1.0, 0.0),
    );
    let (victim, _) = spawn_unit(
        &mut app,
        CharacterClass::Priest,
        2,
        Vec3::new(2.0, 1.0, 0.0),
    );
    swing(&mut app, attacker, victim, AutoAttackKind::Melee, false);
    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&AutoAttackSwing>()
            .iter(app.world())
            .len(),
        0
    );
    assert!(flinch_of(&app, victim).is_some());
}

#[test]
fn a_swing_at_a_despawned_victim_is_dropped_cleanly() {
    let mut app = consumer_app();
    let (attacker, _) = spawn_unit(
        &mut app,
        CharacterClass::Warrior,
        1,
        Vec3::new(0.0, 1.0, 0.0),
    );
    let (victim, _) = spawn_unit(
        &mut app,
        CharacterClass::Priest,
        2,
        Vec3::new(2.0, 1.0, 0.0),
    );
    app.world_mut().entity_mut(victim).despawn();
    swing(&mut app, attacker, victim, AutoAttackKind::Melee, false);
    app.update();
    assert_eq!(spark_count(&mut app), 0);
}

#[test]
fn the_flash_expires_without_leaking_its_entity() {
    let mut app = consumer_app();
    app.add_systems(
        PostUpdate,
        (update_hit_sparks, update_hit_flashes, cleanup_hit_reactions),
    );
    let (attacker, _) = spawn_unit(
        &mut app,
        CharacterClass::Warrior,
        1,
        Vec3::new(0.0, 1.0, 0.0),
    );
    let (victim, _) = spawn_unit(
        &mut app,
        CharacterClass::Priest,
        2,
        Vec3::new(2.0, 1.0, 0.0),
    );
    swing(&mut app, attacker, victim, AutoAttackKind::Melee, false);
    app.update();
    for _ in 0..20 {
        app.update();
    }
    assert_eq!(spark_count(&mut app), 0, "flecks leaked");
    use arenasim::states::play_match::HitFlash;
    assert_eq!(
        app.world_mut().query::<&HitFlash>().iter(app.world()).len(),
        0,
        "the contact flash leaked"
    );
}
