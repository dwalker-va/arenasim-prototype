//! Probes for the Frostbolt and Shadow Bolt missile rigs
//! (`rendering/effects/spell_bolts.rs`).
//!
//! These assert WORLD-SPACE GEOMETRY — where each piece actually ends up and
//! which way it actually points — not the fields the rig happens to store. That
//! distinction is the whole point: the A2 defects (a decal yawed 90° off, a
//! streak reaching half its own `length`, a fan clumped at one point) all
//! shipped past probes that compared stored parameters. A shard whose `length`
//! is right and whose aim is 90° out is a shard lying across its own flight
//! path, and only a rotated basis vector catches it.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window, no
//! GPU. `TransformPlugin` is load-bearing: without it `GlobalTransform` never
//! propagates and every assertion below would read a child's LOCAL pose, which
//! is exactly the bookkeeping these probes exist not to trust.

use std::time::Duration;

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::components::{
    BoltCore, BoltImpact, BoltImpactRole, BoltImpactShard, BoltImpactSprite, BoltKind, BoltMote,
    BoltRig, BoltShard, BoltSprite, BoltTrail, Combatant, Projectile,
};
use arenasim::states::play_match::{
    animate_bolt_impacts, animate_bolts, bolt_billboard_rotation, bolt_impact_life,
    bolt_impact_origin, bolt_kind_for, bolt_ribbon_geometry, arc_roll, build_arc_band,
    frostbolt_shard_extent, shadowbolt_glow_width, spawn_bolt_impacts, spawn_bolt_visuals,
    trail_segment_rotation, update_bolt_motes, update_bolt_trails,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
/// The projectile speed both bolts ship with in `abilities.ron`.
const SPEED: f32 = 35.0;

/// Aims chosen so no single world axis can satisfy them all — a rig that
/// happens to line up with +X passes a one-direction test by coincidence.
fn aims() -> Vec<Vec3> {
    vec![
        Vec3::X,
        -Vec3::X,
        Vec3::Z,
        Vec3::new(1.0, 0.0, 1.0).normalize(),
        Vec3::new(-0.3, 0.4, 0.86).normalize(),
    ]
}

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
        app.add_systems(
            Update,
            (
                spawn_bolt_visuals,
                animate_bolts,
                update_bolt_trails,
                update_bolt_motes,
                spawn_bolt_impacts,
                animate_bolt_impacts,
            )
                .chain(),
        );
        Harness { app }
    }

    /// Spawn a projectile posed exactly as `move_projectiles` would pose it:
    /// translation on the lane, `+Z` rotated onto the direction of travel.
    fn fire(&mut self, ability: AbilityType, origin: Vec3, aim: Vec3) -> Entity {
        let dummy = self.app.world_mut().spawn_empty().id();
        self.app
            .world_mut()
            .spawn((
                Projectile {
                    caster: dummy,
                    target: dummy,
                    ability,
                    speed: SPEED,
                    caster_team: 0,
                    caster_slot: 0,
                    caster_class: CharacterClass::Mage,
                    caster_pet_type: None,
                },
                Transform::from_translation(origin)
                    .with_rotation(Quat::from_rotation_arc(Vec3::Z, aim)),
            ))
            .id()
    }

    /// Advance one frame, moving every bolt along its own aim as the sim would.
    fn fly(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
            let dt = TICK.as_secs_f32();
            let mut moved: Vec<(Entity, Vec3)> = Vec::new();
            let mut q = self.app.world_mut().query::<(Entity, &Transform, &Projectile)>();
            for (e, t, p) in q.iter(self.app.world()) {
                moved.push((e, t.translation + (t.rotation * Vec3::Z) * p.speed * dt));
            }
            for (e, to) in moved {
                if let Some(mut t) = self.app.world_mut().get_mut::<Transform>(e) {
                    t.translation = to;
                }
            }
        }
    }

    fn world(&mut self) -> &mut World {
        self.app.world_mut()
    }

    /// Every entity carrying `T`, with its propagated world transform.
    fn global<T: Component>(&mut self) -> Vec<(Entity, GlobalTransform)> {
        let mut q = self.app.world_mut().query_filtered::<(Entity, &GlobalTransform), With<T>>();
        q.iter(self.app.world()).map(|(e, g)| (e, *g)).collect()
    }

    /// A victim the burst can attach to. It needs a real `Combatant`, because
    /// the burst follows one through a `With<Combatant>` query.
    fn spawn_victim(&mut self, at: Vec3) -> Entity {
        self.app
            .world_mut()
            .spawn((
                Combatant::new(0, 0, CharacterClass::Warrior),
                Transform::from_translation(at),
            ))
            .id()
    }

    /// Land a bolt on `victim`, as `process_projectile_hits` would: `from`
    /// points back up the line the bolt arrived on.
    fn land(&mut self, ability: AbilityType, victim: Entity, from: Vec3) -> Entity {
        let kind = bolt_kind_for(ability).expect("only the two bolts raise a burst");
        self.app
            .world_mut()
            .spawn(BoltImpact {
                kind,
                target: victim,
                from: from.normalize(),
                age: 0.0,
            })
            .id()
    }

    /// The world transforms of every burst part playing a given role.
    fn impact_parts(&mut self, role: BoltImpactRole) -> Vec<GlobalTransform> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&BoltImpactSprite, &GlobalTransform)>();
        q.iter(self.app.world())
            .filter(|(s, _)| s.role == role)
            .map(|(_, g)| *g)
            .collect()
    }

    /// Just the shard's two cones.
    ///
    /// Emphatically NOT "every entity with a `Mesh3d`" — trail segments and
    /// shed motes carry one too, and once a bolt has flown a few frames those
    /// outnumber the cones by an order of magnitude. Reaching through the
    /// hierarchy is the only way to name the pieces this probe is about.
    fn shard_cones(&mut self) -> Vec<GlobalTransform> {
        let hubs: Vec<Entity> = self.global::<BoltShard>().iter().map(|(e, _)| *e).collect();
        let mut out = Vec::new();
        for hub in hubs {
            let kids: Vec<Entity> = self
                .app
                .world()
                .get::<Children>(hub)
                .map(|c| c.iter().collect())
                .unwrap_or_default();
            for kid in kids {
                if let Some(g) = self.app.world().get::<GlobalTransform>(kid) {
                    out.push(*g);
                }
            }
        }
        out
    }
}

// ── the shard's aim ────────────────────────────────────────────────────────

/// The two cones must run ALONG the flight path, in opposite directions.
///
/// Bevy's `Cone` runs along local **+Y** while the projectile aims local **+Z**,
/// so each cone carries a baked `Y -> ±Z` rotation. Drop it and the dart lies
/// across its own flight path — the Hammer of Justice bug, in a different
/// primitive. Asserting the rotated basis vector is the only thing that sees it.
#[test]
fn the_shard_runs_along_the_flight_path() {
    for aim in aims() {
        let mut h = Harness::new();
        h.fire(AbilityType::Frostbolt, Vec3::new(0.0, 1.5, 0.0), aim);
        h.fly(1);

        let shard_children: Vec<_> = h
            .shard_cones()
            .iter()
            .map(|g| (g.rotation() * Vec3::Y).normalize())
            .collect();

        let forward = shard_children.iter().filter(|d| d.dot(aim) > 0.999).count();
        let backward = shard_children.iter().filter(|d| d.dot(aim) < -0.999).count();
        assert_eq!(
            forward, 1,
            "aim {aim:?}: exactly one cone must point down the flight path, got {shard_children:?}"
        );
        assert_eq!(
            backward, 1,
            "aim {aim:?}: exactly one cone must point back up it, got {shard_children:?}"
        );
    }
}

/// The dart must SPAN tip-to-tail about the bolt's position, with each apex
/// where the client profile puts it.
///
/// A cone anchored at its centre instead of its base puts the nose half as far
/// forward while the tail runs back through the caster — the centred-primitive
/// trap, which a length field cannot see.
#[test]
fn the_shard_spans_from_nose_to_tail() {
    let (tip_len, tail_len) = frostbolt_shard_extent();
    for aim in aims() {
        let mut h = Harness::new();
        let origin = Vec3::new(2.0, 1.5, -1.0);
        h.fire(AbilityType::Frostbolt, origin, aim);
        h.fly(1);

        // With `ConeAnchor::Base` the apex sits at local +Y * height.
        let apexes: Vec<Vec3> = h
            .shard_cones()
            .iter()
            .map(|g| {
                let height = if (g.rotation() * Vec3::Y).dot(aim) > 0.0 {
                    tip_len
                } else {
                    tail_len
                };
                g.transform_point(Vec3::Y * (height / g.scale().y))
            })
            .collect();

        let nose = origin + aim * tip_len;
        let tail = origin - aim * tail_len;
        assert!(
            apexes.iter().any(|p| p.distance(nose) < 1e-3),
            "aim {aim:?}: no cone apex at the nose {nose:?}; got {apexes:?}"
        );
        assert!(
            apexes.iter().any(|p| p.distance(tail) < 1e-3),
            "aim {aim:?}: no cone apex at the tail {tail:?}; got {apexes:?}"
        );
    }
}

/// The roll must turn the shard about its own axis and nothing else.
///
/// This is the one-rotation-one-axis rule as a live assertion: if the spin ever
/// picks up a second axis the dart yaws off the flight path as it turns, which
/// reads as a tumbling shard rather than a rolling one.
#[test]
fn the_roll_never_tilts_the_dart_off_axis() {
    let aim = Vec3::new(1.0, 0.0, 1.0).normalize();
    let mut h = Harness::new();
    h.fire(AbilityType::Frostbolt, Vec3::new(0.0, 1.5, 0.0), aim);

    let mut seen_roll = false;
    let mut last: Option<Vec3> = None;
    for _ in 0..24 {
        h.fly(1);
        let mut worst = 1.0f32;
        for g in h.shard_cones() {
            let along = (g.rotation() * Vec3::Y).normalize().dot(aim).abs();
            worst = worst.min(along);
            // The facet ring turns, so local X sweeps around while Y holds.
            let facet = (g.rotation() * Vec3::X).normalize();
            if let Some(prev) = last {
                if prev.distance(facet) > 1e-3 {
                    seen_roll = true;
                }
            }
            last = Some(facet);
        }
        assert!(
            worst > 0.999,
            "the shard drifted off its flight axis while rolling (alignment {worst})"
        );
    }
    assert!(seen_roll, "the shard never actually rolled");
}

// ── the ribbons ────────────────────────────────────────────────────────────

/// Both ribbons must straddle the flight axis, and the pair must ROTATE with
/// the aim rather than spreading along a fixed world axis.
///
/// A layout built on a world axis collapses to a single line for half of all
/// bearings, and a single-direction probe never shows it — so this runs two
/// aims and checks the spread is perpendicular to each.
#[test]
fn the_ribbons_straddle_the_axis_for_every_bearing() {
    let (sep, _, _, _) = bolt_ribbon_geometry(BoltKind::Frost);
    for aim in aims() {
        let mut h = Harness::new();
        let origin = Vec3::new(0.0, 1.5, 0.0);
        h.fire(AbilityType::Frostbolt, origin, aim);
        h.fly(6);

        let trails: Vec<Vec3> = h.global::<BoltTrail>().iter().map(|(_, g)| g.translation()).collect();
        assert!(
            trails.len() >= 4,
            "aim {aim:?}: expected a laid trail, got {} segments",
            trails.len()
        );

        // Distance of each segment from the lane it was dropped along.
        let mut off_axis: Vec<f32> = Vec::new();
        for p in &trails {
            let d = *p - origin;
            let lateral = d - aim * d.dot(aim);
            off_axis.push(lateral.length());
            assert!(
                lateral.dot(aim).abs() < 1e-3,
                "aim {aim:?}: lateral offset is not perpendicular to travel"
            );
        }
        let widest = off_axis.iter().cloned().fold(0.0f32, f32::max);
        assert!(
            (widest - sep).abs() < 1e-3,
            "aim {aim:?}: ribbons sit {widest} from the axis, expected {sep}"
        );
        assert!(
            off_axis.iter().all(|d| *d > sep * 0.5),
            "aim {aim:?}: a ribbon collapsed onto the axis instead of straddling it"
        );
    }
}

/// A trail is a ribbon, not a dotted line.
///
/// The first build measured continuity against a round sprite's full QUAD
/// width and passed — while shipping a visibly dotted trail, because a radial
/// alpha falloff leaves each sprite's bright core far smaller than its quad.
/// Segments are stretched bands now, so the honest measure is whether each one
/// physically REACHES its neighbour: the span between consecutive centres must
/// not exceed a segment's own length.
#[test]
fn the_trail_is_continuous_not_beaded() {
    for kind in [BoltKind::Frost, BoltKind::Shadow] {
        let ability = match kind {
            BoltKind::Frost => AbilityType::Frostbolt,
            BoltKind::Shadow => AbilityType::Shadowbolt,
        };
        let (_, _, step, length) = bolt_ribbon_geometry(kind);
        assert!(
            length > step,
            "{kind:?}: segments are {length} yd long but dropped {step} yd apart, so they cannot overlap"
        );

        let mut h = Harness::new();
        let aim = Vec3::X;
        h.fire(ability, Vec3::new(0.0, 1.5, 0.0), aim);
        h.fly(12);

        // One strand at a time: the two are laterally separated, so pooling
        // them would hide a gap in either.
        let (sep, _, _, _) = bolt_ribbon_geometry(kind);
        let segs = h.global::<BoltTrail>();
        assert!(segs.len() >= 8, "{kind:?}: too few segments to judge");
        let lateral = |g: &GlobalTransform| {
            let d = g.translation() - Vec3::new(0.0, 1.5, 0.0);
            (d - aim * d.dot(aim)).normalize_or_zero()
        };
        let reference = lateral(&segs[0].1);
        for strand in [1.0f32, -1.0] {
            let mut along: Vec<f32> = segs
                .iter()
                .filter(|(_, g)| lateral(g).dot(reference) * strand > 0.0)
                .map(|(_, g)| g.translation().dot(aim))
                .collect();
            along.sort_by(|a, b| a.partial_cmp(b).unwrap());
            assert!(
                along.len() >= 4,
                "{kind:?}: strand {strand} has only {} segments, expected a pair of strands \
                 roughly {sep} yd apart",
                along.len()
            );
            for w in along.windows(2) {
                assert!(
                    w[1] - w[0] <= length + 1e-3,
                    "{kind:?}: a {:.3} yd span between segments only {:.3} yd long — \
                     the ribbon breaks into beads here",
                    w[1] - w[0],
                    length
                );
            }
        }
    }
}

/// A trail segment's LENGTH must lie along the flight path, with only its roll
/// spent on facing the camera.
///
/// Handing a stretched band the camera's rotation wholesale — which is right
/// for a round sprite and is what the motes do — would swing its long axis off
/// the lane and splay the ribbon sideways across the arena.
#[test]
fn trail_bands_keep_their_length_on_the_flight_path() {
    let to_camera = Vec3::new(0.4, 3.0, -8.0);
    for aim in aims() {
        let q = trail_segment_rotation(aim, to_camera);
        assert!(
            (q * Vec3::X).dot(aim) > 0.999,
            "aim {aim:?}: the band's length axis left the flight path"
        );
        // Its face should be as square to the viewer as that constraint allows,
        // i.e. the normal is the component of `to_camera` perpendicular to aim.
        let want = (to_camera - aim * to_camera.dot(aim)).normalize();
        assert!(
            (q * Vec3::Z).dot(want).abs() > 0.999,
            "aim {aim:?}: the band is not turned face-on to the camera"
        );
    }
    // Looking straight down the barrel is degenerate; it must still produce a
    // usable rotation rather than a NaN one.
    let q = trail_segment_rotation(Vec3::X, Vec3::X);
    assert!(q.is_finite() && (q * Vec3::X).dot(Vec3::X) > 0.999);
}

/// No part of the trail may reach AHEAD of the bolt that is laying it.
///
/// A `Rectangle` is centred on its origin, so a band placed at the emission
/// point reaches half its length forward and pokes out through the nose — which
/// on Shadow Bolt showed as the ribbon spearing its own core. This is the
/// centred-primitive trap that the Hammer of Justice streak hit, in a different
/// mesh: an anchor bug is invisible to any check that only reads `length`.
#[test]
fn the_trail_never_reaches_past_the_bolt() {
    for (ability, kind) in [
        (AbilityType::Frostbolt, BoltKind::Frost),
        (AbilityType::Shadowbolt, BoltKind::Shadow),
    ] {
        let (_, _, _, length) = bolt_ribbon_geometry(kind);
        for aim in aims() {
            let mut h = Harness::new();
            let origin = Vec3::new(0.0, 1.5, 0.0);
            h.fire(ability, origin, aim);
            h.fly(8);

            let head = h
                .global::<BoltRig>()
                .first()
                .map(|(_, g)| g.translation().dot(aim))
                .expect("the bolt should still be in flight");

            for (_, g) in h.global::<BoltTrail>() {
                // The band's leading edge, walked out from its centre.
                let nose = (g.translation() + aim * (length * 0.5)).dot(aim);
                assert!(
                    nose <= head + 1e-3,
                    "aim {aim:?}: a {kind:?} band reaches {:.3} yd past the bolt's own head",
                    nose - head
                );
            }
        }
    }
}

/// Nothing in a bolt may cast a shadow.
///
/// The first build let every trail segment cast one, which painted a dotted
/// black line across the arena floor alongside the trail — instantly visible on
/// screen, and invisible to every other check here.
#[test]
fn no_part_of_a_bolt_casts_a_shadow() {
    for ability in [AbilityType::Frostbolt, AbilityType::Shadowbolt] {
        let mut h = Harness::new();
        h.fire(ability, Vec3::new(0.0, 1.5, 0.0), Vec3::X);
        h.fly(8);

        let mut q = h
            .app
            .world_mut()
            .query_filtered::<Entity, (With<Mesh3d>, Without<NotShadowCaster>)>();
        let casters: Vec<Entity> = q.iter(h.app.world()).collect();
        assert!(
            casters.is_empty(),
            "{ability:?}: {} bolt meshes still cast shadows",
            casters.len()
        );
    }
}

/// The trail is left behind in world space, so it must outlive the bolt that
/// laid it — and then clear itself up.
///
/// Parenting the segments to the projectile would have been the obvious build,
/// and it would delete the whole trail at the instant of impact, which is the
/// one moment it should be most visible.
#[test]
fn the_trail_outlives_the_bolt_and_then_expires() {
    let mut h = Harness::new();
    let bolt = h.fire(AbilityType::Frostbolt, Vec3::new(0.0, 1.5, 0.0), Vec3::X);
    h.fly(8);
    assert!(!h.global::<BoltTrail>().is_empty());

    h.world().entity_mut(bolt).despawn();
    h.fly(1);
    assert!(
        !h.global::<BoltTrail>().is_empty(),
        "the trail vanished with the bolt"
    );
    assert!(
        h.global::<BoltShard>().is_empty() && h.global::<BoltSprite>().is_empty(),
        "the rig's own children should die with the projectile"
    );

    // Longest ribbon life is well under half a second.
    h.fly(60);
    assert!(
        h.global::<BoltTrail>().is_empty(),
        "trail segments never expired"
    );
    assert!(
        h.global::<BoltMote>().is_empty(),
        "shed sprites never expired"
    );
}

// ── shed sprites ───────────────────────────────────────────────────────────

/// Shed sprites must actually leave the head, and scatter — a group that all
/// takes the same velocity is one sprite drawn many times.
#[test]
fn shed_sprites_scatter_off_the_head() {
    let mut h = Harness::new();
    h.fire(AbilityType::Frostbolt, Vec3::new(0.0, 1.5, 0.0), Vec3::X);
    // 26 flakes/sec against a 16ms tick, so a handful of frames is one flake.
    h.fly(10);

    let early = h.global::<BoltMote>();
    assert!(early.len() >= 2, "expected shed flakes, got {}", early.len());

    let spread_of = |v: &Vec<(Entity, GlobalTransform)>| {
        let pts: Vec<Vec3> = v.iter().map(|(_, g)| g.translation()).collect();
        let mut worst = 0.0f32;
        for a in &pts {
            for b in &pts {
                worst = worst.max(a.distance(*b));
            }
        }
        worst
    };
    let before = spread_of(&early);
    h.fly(6);
    let after = spread_of(&h.global::<BoltMote>());
    assert!(
        after > before,
        "shed sprites never spread apart ({before} -> {after})"
    );
}

// ── routing and silhouette ─────────────────────────────────────────────────

/// Only the two bolts get a rig; everything else keeps the generic sphere.
#[test]
fn only_frostbolt_and_shadowbolt_are_rigged() {
    assert_eq!(bolt_kind_for(AbilityType::Frostbolt), Some(BoltKind::Frost));
    assert_eq!(
        bolt_kind_for(AbilityType::Shadowbolt),
        Some(BoltKind::Shadow)
    );
    assert_eq!(bolt_kind_for(AbilityType::DeathCoil), None);

    let mut h = Harness::new();
    h.fire(AbilityType::DeathCoil, Vec3::new(0.0, 1.5, 0.0), Vec3::X);
    h.fly(3);
    assert!(
        h.global::<BoltRig>().is_empty(),
        "an unrelated projectile was given a bolt rig"
    );
    assert!(h.global::<BoltTrail>().is_empty());
}

/// Shadow Bolt gets the opaque core and the churn quads; Frostbolt gets the
/// shard. Neither may pick up the other's parts.
#[test]
fn each_bolt_builds_only_its_own_parts() {
    let mut frost = Harness::new();
    frost.fire(AbilityType::Frostbolt, Vec3::Y * 1.5, Vec3::X);
    frost.fly(1);
    assert_eq!(frost.global::<BoltShard>().len(), 1);
    assert!(frost.global::<BoltCore>().is_empty());
    assert_eq!(frost.global::<BoltSprite>().len(), 2, "flare + tip glow");

    let mut shade = Harness::new();
    shade.fire(AbilityType::Shadowbolt, Vec3::Y * 1.5, Vec3::X);
    shade.fly(1);
    assert_eq!(shade.global::<BoltCore>().len(), 1);
    assert!(shade.global::<BoltShard>().is_empty());
    assert_eq!(
        shade.global::<BoltSprite>().len(),
        3,
        "halo + two churn quads"
    );
}

/// The design's load-bearing invariant: the two bolts must stay apart by
/// SILHOUETTE, which is how the Classic models differ (bounding radius 2.054
/// against 0.658). Tuning them back to a similar size would undo the whole
/// point of the change even with the colours left alone.
#[test]
fn the_two_bolts_keep_distinct_silhouettes() {
    let (tip, tail) = frostbolt_shard_extent();
    let frost_len = tip + tail;
    let shadow_width = shadowbolt_glow_width();
    assert!(
        frost_len > shadow_width * 1.8,
        "Frostbolt is {frost_len:.2} yd long against Shadow Bolt's {shadow_width:.2} yd — \
         too close to read apart at a glance"
    );
    // ...and Frostbolt still fits the arena: a combatant capsule is 2.5yd tall.
    assert!(
        frost_len <= 2.5,
        "Frostbolt ({frost_len:.2} yd) is longer than a combatant is tall"
    );
}

/// A sprite hanging off an aimed parent must end up facing the CAMERA, not the
/// camera composed with the parent's aim.
#[test]
fn billboards_cancel_the_parent_aim() {
    let camera = Quat::from_euler(EulerRot::YXZ, 0.7, -0.4, 0.0);
    for aim in aims() {
        let parent = Quat::from_rotation_arc(Vec3::Z, aim);
        let world = parent * bolt_billboard_rotation(parent, camera);
        assert!(
            (world * Vec3::Z).distance(camera * Vec3::Z) < 1e-5,
            "aim {aim:?}: billboard ended up facing {:?}, wanted {:?}",
            world * Vec3::Z,
            camera * Vec3::Z
        );
    }
}

// ── impact ─────────────────────────────────────────────────────────────────

/// Shadow Bolt's arcs are BILATERAL, and the pair must straddle the line the
/// bolt came in on — rotating with it for every bearing.
///
/// This is the trap the rogue crescent fan hit from the other side: a layout
/// built on a fixed world axis collapses to a single point for half of all
/// bearings, and a probe that only fires one direction never sees it. The whole
/// distinction between this burst and Frostbolt's is that it has two sides, so
/// if they ever merge the shape is gone.
#[test]
fn the_shadow_arcs_straddle_the_incoming_line() {
    for from in aims() {
        let mut h = Harness::new();
        let victim = h.spawn_victim(Vec3::new(3.0, 0.0, -2.0));
        h.land(AbilityType::Shadowbolt, victim, from);
        h.fly(1);

        let arcs: Vec<Vec3> = h
            .impact_parts(BoltImpactRole::Arc)
            .iter()
            .map(|g| g.translation())
            .collect();
        assert_eq!(arcs.len(), 2, "from {from:?}: expected exactly two arcs");

        let chest = bolt_impact_origin(Vec3::new(3.0, 0.0, -2.0));
        let a = arcs[0] - chest;
        let b = arcs[1] - chest;

        // Both are pushed FORWARD, toward the caster...
        assert!(
            a.dot(from) > 0.0 && b.dot(from) > 0.0,
            "from {from:?}: the arcs sit behind the chest instead of in front of it"
        );
        // ...and their lateral parts are equal and opposite, perpendicular to
        // the incoming line.
        let lat_a = a - from * a.dot(from);
        let lat_b = b - from * b.dot(from);
        assert!(
            lat_a.length() > 0.1 && lat_b.length() > 0.1,
            "from {from:?}: an arc collapsed onto the incoming axis"
        );
        assert!(
            lat_a.normalize().dot(lat_b.normalize()) < -0.99,
            "from {from:?}: the arcs are on the SAME side — the burst is no longer bilateral"
        );
        assert!(
            (lat_a.length() - lat_b.length()).abs() < 1e-3,
            "from {from:?}: the arcs are not symmetric about the incoming line"
        );
    }
}

/// Frostbolt's burst is RADIAL, so its chips must actually spread over a
/// sphere — not clump, and not flatten into a plane.
///
/// The source is unambiguous on this: all 33 of its bone pivots sit at the
/// origin, so every emitter fires from one point outward.
#[test]
fn the_frost_chips_spread_over_a_sphere() {
    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::ZERO);
    h.land(AbilityType::Frostbolt, victim, Vec3::X);
    h.fly(6);

    let chips: Vec<Vec3> = h
        .global::<BoltImpactShard>()
        .iter()
        .map(|(_, g)| g.translation())
        .collect();
    assert!(chips.len() >= 8, "expected a fan of chips, got {}", chips.len());

    let chest = bolt_impact_origin(Vec3::ZERO);
    // Extent on all three axes: a fan that clumps has none, and one that is
    // secretly planar has none on its missing axis.
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        let spread: Vec<f32> = chips.iter().map(|p| (*p - chest).dot(axis)).collect();
        let lo = spread.iter().cloned().fold(f32::MAX, f32::min);
        let hi = spread.iter().cloned().fold(f32::MIN, f32::max);
        assert!(
            hi - lo > 0.35,
            "the chips span only {:.3} yd on {axis:?} — the burst is not radial",
            hi - lo
        );
    }
    // And they must have LEFT the chest, not sat on it.
    assert!(
        chips.iter().all(|p| p.distance(chest) > 0.05),
        "a chip never left the impact point"
    );
}

/// The burst is chest-attached, so it must FOLLOW a victim that keeps moving.
///
/// The client attaches both impacts to attachment 34. A burst pinned to the
/// world position of the hit slides off a running target within a frame or two,
/// which on a kiting Mage is most of the time.
#[test]
fn the_burst_rides_a_moving_victim() {
    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::ZERO);
    h.land(AbilityType::Frostbolt, victim, Vec3::X);
    h.fly(1);

    let first = h.global::<BoltImpact>()[0].1.translation();
    assert!((first - bolt_impact_origin(Vec3::ZERO)).length() < 1e-3);

    let moved = Vec3::new(2.5, 0.0, 1.5);
    if let Some(mut t) = h.world().get_mut::<Transform>(victim) {
        t.translation = moved;
    }
    h.fly(1);

    let after = h.global::<BoltImpact>()[0].1.translation();
    assert!(
        (after - bolt_impact_origin(moved)).length() < 1e-3,
        "the burst stayed at {after:?} while its victim moved to {moved:?}"
    );
}

/// A billboarded arc must bulge along the direction it is ANCHORED on.
///
/// The anchors are offset along a world axis while the crescent curves in the
/// camera plane; if the roll does not reconcile those, the pair stops
/// straddling the incoming line and closes into a ring around the victim —
/// which is both the wrong shape and a near-copy of Frostbolt's shockwave.
#[test]
fn the_arcs_bulge_the_way_they_are_anchored() {
    let cameras = [
        Quat::IDENTITY,
        Quat::from_rotation_y(0.9),
        Quat::from_euler(EulerRot::YXZ, -1.4, -0.35, 0.0),
    ];
    for cam in cameras {
        for from in aims() {
            let rig = Quat::from_rotation_arc(Vec3::Z, from);
            let lateral = rig * Vec3::X;
            let right = cam * Vec3::X;
            let up = cam * Vec3::Y;
            // Skip a bearing looking straight down the anchor axis, where the
            // projection is degenerate and any roll is as good as any other.
            if lateral.dot(right).hypot(lateral.dot(up)) < 0.15 {
                continue;
            }
            for side in [1.0f32, -1.0] {
                let roll = arc_roll(lateral, cam, side);
                // Where the crescent actually bulges, in world space...
                let bulge = cam * (Quat::from_rotation_z(roll) * Vec3::X);
                // ...against where its anchor sits, flattened into the same plane.
                let anchor = lateral * side;
                let normal = cam * Vec3::Z;
                let flat = (anchor - normal * anchor.dot(normal)).normalize();
                assert!(
                    bulge.dot(flat) > 0.99,
                    "cam {cam:?} from {from:?} side {side}: arc bulges {bulge:?} \
                     but is anchored toward {flat:?}"
                );
            }
            let a = cam * (Quat::from_rotation_z(arc_roll(lateral, cam, 1.0)) * Vec3::X);
            let b = cam * (Quat::from_rotation_z(arc_roll(lateral, cam, -1.0)) * Vec3::X);
            assert!(
                a.dot(b) < -0.99,
                "the two arcs stopped opposing each other ({a:?} vs {b:?})"
            );
        }
    }
}

/// Frostbolt's ring must be a CLOSED band with soft rims.
///
/// A hard-edged `Annulus` renders as a drawn hoop — a geometric shape sitting
/// on the victim rather than a wavefront leaving them. The softness is carried
/// entirely by vertex alpha, and the closure by not repeating the seam vertex.
#[test]
fn the_shockwave_ring_is_closed_and_soft_rimmed() {
    use bevy::render::mesh::VertexAttributeValues;
    let segments = 72u32;
    let mesh = build_arc_band(segments, std::f32::consts::TAU, 0.24, false);

    let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
    else {
        panic!("the ring must carry Float32x4 vertex colours");
    };
    assert_eq!(colors.len(), (segments * 3) as usize, "the ring repeats its seam");
    for (i, c) in colors.iter().enumerate() {
        if i % 3 == 1 {
            assert_eq!(c[3], 1.0, "the ring's spine must hold full strength");
        } else {
            assert_eq!(c[3], 0.0, "a ring rim is not transparent");
        }
    }

    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("positions");
    };
    let first = Vec3::from(pos[1]);
    let last = Vec3::from(pos[pos.len() - 2]);
    assert!(
        first.distance(last) > 1e-3,
        "the ring repeats its seam vertex, which shows as a hairline"
    );
    for i in (1..pos.len()).step_by(3) {
        let p = Vec3::from(pos[i]);
        assert!(
            (p.length() - 1.0).abs() < 1e-4,
            "a spine vertex left the unit circle"
        );
    }
}

/// Bursts must clear themselves, and only the two bolts may raise one.
#[test]
fn bursts_expire_and_only_the_two_bolts_raise_them() {
    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::ZERO);
    h.land(AbilityType::Frostbolt, victim, Vec3::X);
    h.fly(2);
    assert_eq!(h.global::<BoltImpact>().len(), 1);

    // Longest burst is well under a second.
    h.fly(80);
    assert!(
        h.global::<BoltImpact>().is_empty(),
        "a burst never expired"
    );
    assert!(
        h.global::<BoltImpactSprite>().is_empty() && h.global::<BoltImpactShard>().is_empty(),
        "burst parts outlived their rig"
    );
}

/// Shadow Bolt's burst must contain something that DARKENS.
///
/// Every other piece of both impacts is additive, and additive can only
/// brighten. On the arena's pale sand a desaturated `SpellSchool::Shadow` glow
/// barely moves the pixels, which is why the hit first shipped reading as
/// blended and slight beside Frostbolt's near-white ring. The source's Opaque
/// batch is the answer, and it only works while it stays non-additive.
#[test]
fn the_shadow_burst_darkens_as_well_as_glows() {
    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::ZERO);
    h.land(AbilityType::Shadowbolt, victim, Vec3::X);
    h.fly(1);

    let blots = h.impact_parts(BoltImpactRole::Blot);
    assert_eq!(blots.len(), 1, "the shadow burst has no darkening piece");

    // Its material must not be additive, or it cannot darken anything.
    let mut q = h
        .app
        .world_mut()
        .query::<(&BoltImpactSprite, &MeshMaterial3d<StandardMaterial>)>();
    let handle = q
        .iter(h.app.world())
        .find(|(s, _)| s.role == BoltImpactRole::Blot)
        .map(|(_, m)| m.0.clone())
        .expect("blot material");
    let materials = h.app.world().resource::<Assets<StandardMaterial>>();
    let material = materials.get(&handle).expect("blot material present");
    assert!(
        matches!(material.alpha_mode, AlphaMode::Blend),
        "the blot is {:?}, which cannot darken — it must be Blend",
        material.alpha_mode
    );
    let lum = material.base_color.to_linear();
    assert!(
        lum.red + lum.green + lum.blue < 0.3,
        "the blot is not dark enough to read against the arena floor"
    );
    // Frostbolt has no blot: its burst is bright against a light ground and
    // needs no help.
    let mut f = Harness::new();
    let v2 = f.spawn_victim(Vec3::ZERO);
    f.land(AbilityType::Frostbolt, v2, Vec3::X);
    f.fly(1);
    assert!(f.impact_parts(BoltImpactRole::Blot).is_empty());
}

/// Shadow's burst must stay strictly snappier than frost's.
///
/// The source is emphatic — shadow is over at 667ms while frost runs to
/// 1234ms — and that tempo gap is half of what tells the two hits apart, the
/// other half being radial-versus-bilateral. Both were compressed to fit the
/// cast cadence; the ORDER is what must survive the compression.
#[test]
fn the_two_bursts_keep_their_tempo_apart() {
    let frost = bolt_impact_life(BoltKind::Frost);
    let shadow = bolt_impact_life(BoltKind::Shadow);
    assert!(
        shadow < frost,
        "shadow's burst ({shadow:.2}s) is not snappier than frost's ({frost:.2}s)"
    );
    // Frostbolt recasts about every 1.5s; a burst that outlives the gap would
    // still be on screen when the next one lands.
    assert!(
        frost < 0.75,
        "frost's burst ({frost:.2}s) is long enough to overlap the next cast"
    );
}

/// The crescent must taper to nothing at both tips and both rims.
///
/// Mirrors `frost_nova`'s ring assertion, and for the same reason: the soft
/// edge is carried entirely by vertex alpha, so a build that forgets it ends
/// the stroke on a hard cut.
#[test]
fn the_crescent_tapers_to_nothing_at_its_tips() {
    use bevy::render::mesh::VertexAttributeValues;
    let mesh = build_arc_band(24, 2.1, 0.19, true);
    let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
    else {
        panic!("the crescent must carry Float32x4 vertex colours");
    };
    // Vertices come in rim/spine/rim triples along the sweep.
    assert_eq!(colors.len() % 3, 0);
    for (i, c) in colors.iter().enumerate() {
        if i % 3 != 1 {
            assert_eq!(c[3], 0.0, "a rim vertex is not transparent");
        }
    }
    let first_spine = colors[1][3];
    let last_spine = colors[colors.len() - 2][3];
    assert!(
        first_spine < 1e-3 && last_spine < 1e-3,
        "the crescent does not fade out at its tips ({first_spine}, {last_spine})"
    );
    let mid = colors[(colors.len() / 2 / 3) * 3 + 1][3];
    assert!(mid > 0.9, "the crescent has no solid middle ({mid})");
}
