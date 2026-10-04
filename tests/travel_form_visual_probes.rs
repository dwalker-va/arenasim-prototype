//! Probes for Travel Form's body, bound and shift puff (`shapeshift.rs`,
//! `update_travel_bound` in `gait.rs`).
//!
//! Every assertion here is on what is DRAWN: the meshes under a unit that
//! render (a `Mesh3d`, not hidden on itself or any ancestor), their world-space
//! vertices, and the colour of the material each one wears. Nothing reads the
//! form's own bookkeeping, so a probe passes only if the screen would show the
//! right body.
//!
//! The form shares a body with Fear, which lands on a shifted Druid: the form
//! takes the body's mesh away and Fear swaps its material. Both start orders
//! and both end orders are run through the real fear system, as is the sheep's
//! mutual exclusion through the real polymorph system.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window,
//! no GPU.

use std::time::Duration;

use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, ClientParticle, Combatant, OriginalBodyMaterial, OriginalMesh,
    ShiftPuff, SwingStyle, VisualBody, WalkAnim, WeaponHand, WeaponKind, WeaponSocket,
};
use arenasim::states::play_match::{
    animate_client_particles, animate_weapon_swings, dress_travel_form, update_fear_run,
    update_fear_visuals, update_polymorph_visuals, update_shift_puffs, update_travel_bound,
    update_travel_form_visuals, update_walk_animation, SHIFT_PUFF_SECS, TRAVEL_BODY_HEIGHT,
    TRAVEL_BODY_LENGTH, TRAVEL_BOUND_BOB, TRAVEL_BOUND_ROCK_DEG, TRAVEL_STRIDE,
};
use arenasim::CharacterClass;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::VertexAttributeValues;
use bevy::time::TimeUpdateStrategy;

/// One 60Hz frame.
const TICK: Duration = Duration::from_micros(16_667);

/// The Druid's class colour, as the client spawns its capsule.
const DRUID_ORANGE: Color = Color::srgb(1.0, 0.49, 0.04);
/// Fear's husk tint (`fear.rs`).
const HUSK: Color = Color::srgb(0.28, 0.16, 0.40);

fn aura(effect_type: AuraType) -> Aura {
    Aura {
        effect_type,
        duration: 30.0,
        magnitude: 0.0,
        break_on_damage_threshold: -1.0,
        ..Default::default()
    }
}

struct Harness {
    app: App,
}

/// Where a unit's drawn body sits in the world.
#[derive(Debug)]
struct Extent {
    min: Vec3,
    max: Vec3,
}

impl Extent {
    fn height(&self) -> f32 {
        self.max.y
    }
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
        // The production order (`states/mod.rs`): the body treatments chained,
        // then the gaits, then the puff and its particles.
        app.add_systems(
            Update,
            (
                update_polymorph_visuals,
                update_fear_visuals,
                update_travel_form_visuals,
                dress_travel_form,
                update_walk_animation,
                update_fear_run,
                update_travel_bound,
                update_shift_puffs,
                animate_client_particles,
                animate_weapon_swings,
            )
                .chain(),
        );
        Harness { app }
    }

    /// A Druid facing +X at `x`, with its capsule `VisualBody` as the client
    /// spawns it. Returns (unit, body, class material).
    fn spawn_druid(&mut self, x: f32) -> (Entity, Entity, Handle<StandardMaterial>) {
        let world = self.app.world_mut();
        let mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(Capsule3d::new(0.5, 1.5));
        let material = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color: DRUID_ORANGE,
                ..default()
            });
        let body = world
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                OriginalMesh(mesh),
                VisualBody { rest_y: 0.0 },
                Transform::default(),
                Visibility::default(),
            ))
            .id();
        let unit = world
            .spawn((
                Transform::from_xyz(x, 1.0, 0.0)
                    .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
                Visibility::default(),
                Combatant::new(1, 0, CharacterClass::Druid),
                WalkAnim {
                    phase: 0.0,
                    previous_xz: Vec2::new(x, 0.0),
                    idle_time: 1.0,
                    body_offset: 0.0,
                },
            ))
            .id();
        world.entity_mut(unit).add_child(body);
        (unit, body, material)
    }

    /// A staff in the Druid's main hand, a socket on its body as the client
    /// mounts it.
    fn give_staff(&mut self, unit: Entity, body: Entity) -> Entity {
        let world = self.app.world_mut();
        let socket = world
            .spawn((
                WeaponSocket {
                    kind: WeaponKind::Mace,
                    hand: WeaponHand::Main,
                    owner: unit,
                    rest: Transform::IDENTITY,
                    release_t: None,
                    aim: Vec3::ZERO,
                    yaw_local: 0.0,
                    prev_owner_yaw: 0.0,
                    windup_s: 0.0,
                    swing_style: SwingStyle::Auto,
                    stroke_interval: 0.0,
                    last_s: 0.0,
                },
                Transform::default(),
                Visibility::Inherited,
            ))
            .id();
        world.entity_mut(body).add_child(socket);
        socket
    }

    fn set_auras(&mut self, unit: Entity, types: &[AuraType]) {
        let auras = types.iter().map(|&t| aura(t)).collect();
        self.app
            .world_mut()
            .entity_mut(unit)
            .insert(ActiveAuras { auras });
    }

    fn step(&mut self, frames: usize) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    /// Whether `e` renders: nothing on its path to the root is `Hidden`.
    fn shown(&self, mut e: Entity) -> bool {
        let world = self.app.world();
        loop {
            if world.get::<Visibility>(e) == Some(&Visibility::Hidden) {
                return false;
            }
            match world.get::<ChildOf>(e) {
                Some(parent) => e = parent.parent(),
                None => return true,
            }
        }
    }

    fn descendants(&self, root: Entity) -> Vec<Entity> {
        let world = self.app.world();
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(e) = stack.pop() {
            out.push(e);
            if let Some(children) = world.get::<Children>(e) {
                stack.extend(children.iter());
            }
        }
        out
    }

    /// Every mesh drawn under `unit`: (entity, its world-space vertices, the
    /// base colour of the material it wears, whether it is additive).
    fn drawn(&self, unit: Entity) -> Vec<(Entity, Vec<Vec3>, Color, bool)> {
        let world = self.app.world();
        let meshes = world.resource::<Assets<Mesh>>();
        let materials = world.resource::<Assets<StandardMaterial>>();
        self.descendants(unit)
            .into_iter()
            .filter(|&e| self.shown(e))
            .filter_map(|e| {
                let mesh = meshes.get(&world.get::<Mesh3d>(e)?.0)?;
                let material =
                    materials.get(&world.get::<MeshMaterial3d<StandardMaterial>>(e)?.0)?;
                let global = world.get::<GlobalTransform>(e)?;
                let Some(VertexAttributeValues::Float32x3(points)) =
                    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    return None;
                };
                let points = points
                    .iter()
                    .map(|p| global.transform_point(Vec3::from(*p)))
                    .collect();
                let additive = matches!(material.alpha_mode, AlphaMode::Add);
                Some((e, points, material.base_color, additive))
            })
            .collect()
    }

    /// The world box around everything drawn under `unit`.
    fn extent(&self, unit: Entity) -> Extent {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for (_, points, _, _) in self.drawn(unit) {
            for p in points {
                min = min.min(p);
                max = max.max(p);
            }
        }
        Extent { min, max }
    }

    /// The colours of the solid (non-additive) meshes drawn under `unit`.
    fn solid_colours(&self, unit: Entity) -> Vec<Color> {
        self.drawn(unit)
            .into_iter()
            .filter(|(_, _, _, additive)| !additive)
            .map(|(_, _, c, _)| c)
            .collect()
    }

    /// The lying pill — the drawn solid part spanning the most of the heading
    /// (+X) — as its world centre and the world direction of its long axis
    /// (a capsule's local Y, read off its world transform).
    fn lying_pill_axis(&self, unit: Entity) -> (Vec3, Vec3) {
        let (entity, points, _, _) = self
            .drawn(unit)
            .into_iter()
            .filter(|(_, _, _, additive)| !additive)
            .max_by(|a, b| span_x(&a.1).total_cmp(&span_x(&b.1)))
            .expect("something is drawn");
        let centre = points.iter().copied().sum::<Vec3>() / points.len() as f32;
        let world_tf = self.app.world().get::<GlobalTransform>(entity).unwrap();
        let mut axis = world_tf.affine().transform_vector3(Vec3::Y).normalize();
        // The pill's axis points nose-ward.
        if axis.x < 0.0 {
            axis = -axis;
        }
        (centre, axis)
    }
}

fn span_x(points: &[Vec3]) -> f32 {
    let (lo, hi) = points
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p.x), hi.max(p.x))
        });
    hi - lo
}

fn close(a: Color, b: Color, tol: f32) -> bool {
    let (a, b) = (a.to_srgba(), b.to_srgba());
    (a.red - b.red).abs() < tol && (a.green - b.green).abs() < tol && (a.blue - b.blue).abs() < tol
}

fn shade(c: Color, k: f32) -> Color {
    let c = c.to_srgba();
    Color::srgb(c.red * k, c.green * k, c.blue * k)
}

/// The standing capsule: 2.5 yd tall about a centre 1 yd up, 1 yd across.
fn assert_standing(h: &Harness, unit: Entity, what: &str) {
    let e = h.extent(unit);
    assert!(
        e.height() > 2.0 && (e.max.x - e.min.x) < 1.3,
        "{what}: expected the standing capsule, drawn extent {e:?}"
    );
}

/// The form: low, and long along the heading (+X), narrow across it.
fn assert_on_all_fours(h: &Harness, unit: Entity, what: &str) {
    let e = h.extent(unit);
    let along = e.max.x - e.min.x;
    let across = e.max.z - e.min.z;
    assert!(
        e.height() < 1.4 && along > TRAVEL_BODY_LENGTH + 0.5 && across < 0.8,
        "{what}: expected the pill on all fours, drawn extent {e:?}"
    );
}

/// Every solid part of the form or the body wears `colour`, the ears and tail
/// a shade of it. `form` says which body is drawn.
fn assert_wears(h: &Harness, unit: Entity, colour: Color, form: bool, what: &str) {
    let colours = h.solid_colours(unit);
    if form {
        assert_eq!(colours.len(), 5, "{what}: pill, head, two ears, tail");
        let plain = colours.iter().filter(|&&c| close(c, colour, 1e-3)).count();
        let ears = colours
            .iter()
            .filter(|&&c| close(c, shade(colour, 0.8), 1e-3))
            .count();
        let tail = colours
            .iter()
            .filter(|&&c| close(c, shade(colour, 0.9), 1e-3))
            .count();
        assert_eq!(
            (plain, ears, tail),
            (2, 2, 1),
            "{what}: colours {colours:?}"
        );
    } else {
        assert_eq!(colours.len(), 1, "{what}: one standing capsule");
        assert!(close(colours[0], colour, 1e-3), "{what}: {colours:?}");
    }
}

#[test]
fn the_shift_lays_the_body_down_along_the_heading() {
    let mut h = Harness::new();
    let (unit, _, _) = h.spawn_druid(0.0);
    h.step(1);
    assert_standing(&h, unit, "before the shift");

    h.set_auras(unit, &[AuraType::TravelForm]);
    h.step(1);
    assert_on_all_fours(&h, unit, "shifted");
    assert_wears(&h, unit, DRUID_ORANGE, true, "shifted");

    // Its belly is off the floor and its back at the signed-off height.
    let e = h.extent(unit);
    assert!(e.min.y > 0.15 && e.min.y < TRAVEL_BODY_HEIGHT, "{e:?}");

    // The head end is the heading: the highest point (an ear tip) is forward
    // of centre, and the pill lies level at rest.
    let top = h
        .drawn(unit)
        .into_iter()
        .flat_map(|(_, p, _, _)| p)
        .max_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap();
    assert!(top.x > 0.3, "ears should be at the front, top at {top}");
    let (centre, axis) = h.lying_pill_axis(unit);
    assert!((centre.y - TRAVEL_BODY_HEIGHT).abs() < 0.02, "{centre}");
    assert!(axis.y.abs() < 0.02, "level at rest, axis {axis}");
}

#[test]
fn every_exit_stands_the_body_back_up() {
    type Exit = fn(&mut Harness, Entity);
    let exits: [(&str, Exit); 4] = [
        ("shift out", |h, u| h.set_auras(u, &[])),
        ("aura component removed", |h, u| {
            h.app.world_mut().entity_mut(u).remove::<ActiveAuras>();
        }),
        ("purged among other auras", |h, u| {
            h.set_auras(u, &[AuraType::MovementSpeedSlow])
        }),
        ("death with the aura still on", |h, u| {
            h.app
                .world_mut()
                .get_mut::<Combatant>(u)
                .unwrap()
                .current_health = 0.0;
        }),
    ];
    for (what, exit) in exits {
        let mut h = Harness::new();
        let (unit, _, _) = h.spawn_druid(0.0);
        h.set_auras(unit, &[AuraType::TravelForm]);
        h.step(2);
        assert_on_all_fours(&h, unit, what);

        exit(&mut h, unit);
        h.step(1);
        assert_standing(&h, unit, what);
        assert_wears(&h, unit, DRUID_ORANGE, false, what);
    }
}

#[test]
fn a_restore_is_owner_scoped() {
    let mut h = Harness::new();
    let (a, _, _) = h.spawn_druid(0.0);
    let (b, _, _) = h.spawn_druid(10.0);
    h.set_auras(a, &[AuraType::TravelForm]);
    h.set_auras(b, &[AuraType::TravelForm]);
    h.step(2);
    h.set_auras(a, &[]);
    h.step(2);
    assert_standing(&h, a, "the Druid that shifted out");
    assert_on_all_fours(&h, b, "the Druid still shifted");
}

/// The four ways a Fear and the form overlap: either may land first, and
/// either may end first. Each must leave the body the other still calls for,
/// in the colour the fear still calls for.
#[test]
fn the_form_and_a_fear_compose_in_every_order() {
    for fear_first in [false, true] {
        for form_ends_first in [false, true] {
            let what = format!(
                "{} first, {} ends first",
                if fear_first { "fear" } else { "form" },
                if form_ends_first { "form" } else { "fear" },
            );
            let mut h = Harness::new();
            let (unit, _, _) = h.spawn_druid(0.0);

            let first = if fear_first {
                AuraType::Fear
            } else {
                AuraType::TravelForm
            };
            h.set_auras(unit, &[first]);
            h.step(2);
            h.set_auras(unit, &[AuraType::TravelForm, AuraType::Fear]);
            h.step(2);
            assert_on_all_fours(&h, unit, &format!("{what}: both up"));
            assert_wears(&h, unit, HUSK, true, &format!("{what}: both up"));

            let remaining = if form_ends_first {
                AuraType::Fear
            } else {
                AuraType::TravelForm
            };
            h.set_auras(unit, &[remaining]);
            h.step(2);
            if form_ends_first {
                // Standing, still the husk, and the fear's fitted shroud is
                // drawn around it again.
                assert_standing(&h, unit, &format!("{what}: fear alone"));
                assert_wears(&h, unit, HUSK, false, &format!("{what}: fear alone"));
                assert!(
                    h.drawn(unit).iter().any(|(_, _, _, additive)| *additive),
                    "{what}: the fear shroud should be back"
                );
            } else {
                assert_on_all_fours(&h, unit, &format!("{what}: form alone"));
                assert_wears(&h, unit, DRUID_ORANGE, true, &format!("{what}: form alone"));
            }

            h.set_auras(unit, &[]);
            h.step(2);
            assert_standing(&h, unit, &format!("{what}: neither"));
            assert_wears(&h, unit, DRUID_ORANGE, false, &format!("{what}: neither"));
        }
    }
}

#[test]
fn a_sheep_keeps_the_body_until_it_lifts() {
    // The sim makes a shifted Druid immune to Polymorph, but not the other way
    // round in principle: a form that arrives under a sheep waits for it.
    let mut h = Harness::new();
    let (unit, _, _) = h.spawn_druid(0.0);
    h.set_auras(unit, &[AuraType::Polymorph]);
    h.step(2);
    h.set_auras(unit, &[AuraType::Polymorph, AuraType::TravelForm]);
    h.step(2);
    let e = h.extent(unit);
    assert!(e.height() < 1.0, "the sheep is still drawn, extent {e:?}");
    assert!(
        (e.max.x - e.min.x) < TRAVEL_BODY_LENGTH,
        "and not the form, extent {e:?}"
    );

    h.set_auras(unit, &[AuraType::TravelForm]);
    h.step(3);
    assert_on_all_fours(&h, unit, "sheep lifted, still shifted");
    assert_wears(&h, unit, DRUID_ORANGE, true, "sheep lifted, still shifted");
}

/// The form's restore slot is its own: after a shift in and out, the shared
/// `OriginalMesh` / `OriginalBodyMaterial` slots still hold what the client
/// spawned, so a later Polymorph turns the Druid into a sheep and restores the
/// standing body from them.
#[test]
fn a_shift_leaves_the_shared_restore_slots_for_a_later_polymorph() {
    let mut h = Harness::new();
    let (unit, body, material) = h.spawn_druid(0.0);
    h.step(1);
    let standing = h.app.world().get::<Mesh3d>(body).unwrap().0.clone();
    let slots = |h: &Harness| {
        let world = h.app.world();
        (
            world.get::<OriginalMesh>(body).map(|m| m.0.clone()),
            world.get::<OriginalBodyMaterial>(body).map(|m| m.0.clone()),
        )
    };
    let before = slots(&h);
    assert_eq!(before, (Some(standing.clone()), None));

    h.set_auras(unit, &[AuraType::TravelForm]);
    h.step(2);
    assert_on_all_fours(&h, unit, "shifted");
    assert_eq!(
        slots(&h),
        before,
        "the form must not touch the shared slots"
    );

    h.set_auras(unit, &[]);
    h.step(2);
    assert_standing(&h, unit, "shifted out");
    assert_eq!(slots(&h), before, "nor leave anything in them");

    h.set_auras(unit, &[AuraType::Polymorph]);
    h.step(2);
    let e = h.extent(unit);
    assert!(
        e.height() < 1.0 && e.max.x - e.min.x < 1.6 && e.max.x - e.min.x > 0.5,
        "a Polymorph after the shift draws the sheep, extent {e:?}"
    );
    assert_eq!(
        slots(&h),
        (Some(standing.clone()), Some(material.clone())),
        "the sheep stored the real class material"
    );

    h.set_auras(unit, &[]);
    h.step(2);
    assert_standing(&h, unit, "sheep lifted");
    assert_wears(&h, unit, DRUID_ORANGE, false, "sheep lifted");
    assert_eq!(h.app.world().get::<Mesh3d>(body).unwrap().0, standing);
    assert_eq!(slots(&h), before);
}

#[test]
fn the_staff_is_put_away_while_shifted() {
    let mut h = Harness::new();
    let (unit, body, _) = h.spawn_druid(0.0);
    let staff = h.give_staff(unit, body);
    h.step(1);
    assert!(h.shown(staff), "standing, the Druid holds its staff");
    h.set_auras(unit, &[AuraType::TravelForm]);
    h.step(1);
    assert!(!h.shown(staff), "on all fours, no staff");
    h.set_auras(unit, &[]);
    h.step(1);
    assert!(h.shown(staff), "standing again, the staff is back");
}

/// The bound is driven by DISTANCE: a quarter of a stride into a run the nose
/// is at its full rock up, and three quarters in at its full rock down, at any
/// pace — and a run of two strides rocks nose-up exactly twice, however long
/// it takes.
#[test]
fn the_bound_keeps_its_stride_at_any_pace() {
    for speed in [3.5f32, 7.0, 10.5] {
        let mut h = Harness::new();
        let (unit, _, _) = h.spawn_druid(0.0);
        h.set_auras(unit, &[AuraType::TravelForm]);
        h.step(2);
        let dt = TICK.as_secs_f32();
        let step = speed * dt;
        let frames = (2.0 * TRAVEL_STRIDE / step).round() as usize;
        let mut pitches = Vec::with_capacity(frames);
        for _ in 0..frames {
            h.app
                .world_mut()
                .get_mut::<Transform>(unit)
                .unwrap()
                .translation
                .x += step;
            h.step(1);
            let (_, axis) = h.lying_pill_axis(unit);
            pitches.push(axis.y.asin().to_degrees());
        }
        // The frame at which the run has covered `share` of a stride.
        let at = |share: f32| pitches[(share * TRAVEL_STRIDE / step).round() as usize - 1];
        let rock = TRAVEL_BOUND_ROCK_DEG;
        for (share, want) in [(0.25, rock), (0.75, -rock), (1.25, rock), (1.75, -rock)] {
            let got = at(share);
            assert!(
                (got - want).abs() < 1.0,
                "at {speed} yd/s, {share} of a stride in: pitch {got}°, want {want}°"
            );
        }
        let nose_ups = pitches
            .windows(3)
            .filter(|w| w[1] > w[0] && w[1] >= w[2] && w[1] > rock * 0.8)
            .count();
        assert_eq!(nose_ups, 2, "at {speed} yd/s: two strides, two bounds");
    }
}

#[test]
fn the_bound_bobs_and_rocks_once_per_stride_and_settles_level() {
    let mut h = Harness::new();
    let (unit, _, _) = h.spawn_druid(0.0);
    h.set_auras(unit, &[AuraType::TravelForm]);
    h.step(2);

    // Run along the heading at a Travel Form pace for two strides.
    let speed = 7.0;
    let dt = TICK.as_secs_f32();
    let frames = (2.0 * TRAVEL_STRIDE / (speed * dt)) as usize;
    let (mut lo_y, mut hi_y) = (f32::INFINITY, f32::NEG_INFINITY);
    let (mut up, mut down) = (0.0f32, 0.0f32);
    let mut nose_up_at = None;
    for i in 0..frames {
        h.app
            .world_mut()
            .get_mut::<Transform>(unit)
            .unwrap()
            .translation
            .x += speed * dt;
        h.step(1);
        let (centre, axis) = h.lying_pill_axis(unit);
        lo_y = lo_y.min(centre.y);
        hi_y = hi_y.max(centre.y);
        let pitch = axis.y.asin().to_degrees();
        if pitch > up {
            up = pitch;
            nose_up_at.get_or_insert(i);
        }
        down = down.min(pitch);
    }
    let rock = TRAVEL_BOUND_ROCK_DEG;
    assert!((up - rock).abs() < 0.5, "nose-up peak {up}°, want {rock}°");
    assert!(
        (down + rock).abs() < 0.5,
        "nose-down peak {down}°, want -{rock}°"
    );
    assert!(
        (lo_y - TRAVEL_BODY_HEIGHT).abs() < 0.01
            && (hi_y - TRAVEL_BODY_HEIGHT - TRAVEL_BOUND_BOB).abs() < 0.01,
        "bob between {lo_y} and {hi_y}, want {TRAVEL_BODY_HEIGHT} .. +{TRAVEL_BOUND_BOB}"
    );

    // Stop: the bound settles level and down within a second.
    h.step(60);
    let (centre, axis) = h.lying_pill_axis(unit);
    assert!(axis.y.abs() < 1e-3, "settled level, axis {axis}");
    assert!((centre.y - TRAVEL_BODY_HEIGHT).abs() < 1e-3, "{centre}");
}

#[test]
fn the_puff_plays_at_the_feet_on_the_shift_in_and_out() {
    let mut h = Harness::new();
    let (unit, _, _) = h.spawn_druid(3.0);
    let particles = |h: &mut Harness| -> Vec<(Vec3, bool)> {
        h.app
            .world_mut()
            .query::<(&ClientParticle, &GlobalTransform, Has<NotShadowCaster>)>()
            .iter(h.app.world())
            .map(|(_, t, no_shadow)| (t.translation(), no_shadow))
            .collect()
    };
    let puffs = |h: &mut Harness| {
        h.app
            .world_mut()
            .query::<&ShiftPuff>()
            .iter(h.app.world())
            .count()
    };
    let puff_frames = (SHIFT_PUFF_SECS / TICK.as_secs_f32()).ceil() as usize + 2;

    for (what, auras) in [
        ("shift in", &[AuraType::TravelForm][..]),
        ("shift out", &[][..]),
    ] {
        h.set_auras(unit, auras);
        h.step(20);
        let live = particles(&mut h);
        assert!(
            live.len() > 20,
            "{what}: a puff of particles, got {}",
            live.len()
        );
        for (p, no_shadow) in &live {
            assert!(*no_shadow, "{what}: particles cast no shadow");
            let flat = Vec2::new(p.x - 3.0, p.z).length();
            assert!(
                flat < 3.0 && p.y > -0.5 && p.y < 2.5,
                "{what}: a particle at {p}, away from the Druid's feet"
            );
        }
        h.step(puff_frames);
        assert_eq!(
            puffs(&mut h),
            0,
            "{what}: the puff retires after its windows"
        );
        // Its particles finish their own short lives too.
        h.step(90);
        assert!(particles(&mut h).is_empty(), "{what}: particles expire");
    }
}
