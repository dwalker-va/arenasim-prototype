//! Probes for Ice Barrier's client model (`rendering/effects/ice_barrier.rs`).
//!
//! World-space geometry and timing through `GlobalTransform`, the live
//! materials and the live meshes, never the rig's stored fields: the 200 ms
//! delay and the tall pop, the bright held ring whose streaks never go dark,
//! the icicles framing the body at the shoulders and knees, the glint turning
//! with the view, the frost mist falling from above the head and outliving the
//! barrier, the one swell-and-fade a break and an expiry share, the upright
//! camera-facing cards and their draw order against the blended capsule, and
//! the Mage's own Frost hands on the cast.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` with the
//! systems in production order: the lifecycle in `Update`, the placement in
//! `PostUpdate` before propagation, as `states/mod.rs` registers them.

use std::time::Duration;

use bevy::prelude::*;
use bevy::render::mesh::VertexAttributeValues;
use bevy::render::render_phase::ViewRangefinder3d;
use bevy::time::TimeUpdateStrategy;
use bevy::transform::TransformSystem;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, CastEnding, CastEndingKind, ClientParticle, Combatant,
    HealCastHand, HealCastKind, HealCastPiece, HealCastPieceRole, InstantAbilityFired, LandedCast,
    ShieldCard, ShieldKind, ShieldLayer, ShieldPhase, ShieldVisual, VisualBody,
};
use arenasim::states::play_match::{
    animate_client_particles, place_shield_cards, shield_card_extent, shield_layer_rgb,
    spawn_instant_cast_hands, update_heal_cast_glows, update_shield_visuals, IB_RIM_GAIN,
    ICE_BARRIER_AURA_NAME, PWS_CLIENT_TO_RIG, PWS_RIM_QUAD, PWS_STREAK_QUAD, PW_SHIELD_AURA_NAME,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
const DT: f32 = 0.016;
/// The capsule radius every body has, and its half-height.
const BODY_RADIUS: f32 = 0.5;
const BODY_HALF_HEIGHT: f32 = 1.25;

struct Harness {
    app: App,
}

fn base_app() -> App {
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
    app.insert_resource(AbilityDefinitions::default());
    app
}

impl Harness {
    fn new() -> Self {
        let mut app = base_app();
        app.add_systems(
            Update,
            (update_shield_visuals, animate_client_particles).chain(),
        );
        app.add_systems(
            PostUpdate,
            place_shield_cards.before(TransformSystem::TransformPropagate),
        );
        Harness { app }
    }

    fn camera(&mut self, from: Vec3, look_at: Vec3) -> Entity {
        self.app
            .world_mut()
            .spawn((
                Camera3d::default(),
                Transform::from_translation(from).looking_at(look_at, Vec3::Y),
            ))
            .id()
    }

    /// A Mage with the real rendered hierarchy: the sim entity at capsule
    /// centre height and a `VisualBody` child, as `spawn_combatant` builds it.
    fn bearer(&mut self, at: Vec3) -> (Entity, Entity) {
        let bearer = self
            .app
            .world_mut()
            .spawn((
                Combatant::new(0, 0, CharacterClass::Mage),
                Transform::from_translation(at),
                ActiveAuras::default(),
            ))
            .id();
        let body = self
            .app
            .world_mut()
            .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
            .id();
        self.app.world_mut().entity_mut(bearer).add_child(body);
        (bearer, body)
    }

    fn give(&mut self, bearer: Entity, name: &str) {
        self.app
            .world_mut()
            .get_mut::<ActiveAuras>(bearer)
            .unwrap()
            .auras
            .push(absorb(name));
    }

    /// Breaks the barrier: the absorb is used up.
    fn break_barrier(&mut self, bearer: Entity) {
        for aura in &mut self
            .app
            .world_mut()
            .get_mut::<ActiveAuras>(bearer)
            .unwrap()
            .auras
        {
            aura.magnitude = 0.0;
        }
    }

    /// The barrier runs out: the aura is gone.
    fn expire(&mut self, bearer: Entity) {
        self.app
            .world_mut()
            .get_mut::<ActiveAuras>(bearer)
            .unwrap()
            .auras
            .clear();
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    /// Every Ice Barrier rig (the shared shield rig, of this kind).
    fn rigs(&mut self) -> Vec<ShieldVisual> {
        let mut q = self.app.world_mut().query::<&ShieldVisual>();
        q.iter(self.app.world())
            .filter(|r| r.kind == ShieldKind::IceBarrier)
            .copied()
            .collect()
    }

    fn rig_of(&mut self, bearer: Entity) -> Option<ShieldVisual> {
        self.rigs().into_iter().find(|r| r.bearer == bearer)
    }

    /// Every card on screen for a bearer, as (layer, world transform, weight).
    fn cards(&mut self, bearer: Entity) -> Vec<(ShieldLayer, Transform, f32)> {
        let mut rigs = self.app.world_mut().query::<(&ShieldVisual, &Children)>();
        let kids: Vec<Entity> = rigs
            .iter(self.app.world())
            .filter(|(r, _)| r.bearer == bearer && r.kind == ShieldKind::IceBarrier)
            .flat_map(|(_, c)| c.iter().collect::<Vec<_>>())
            .collect();
        let world = self.app.world();
        let materials = world.resource::<Assets<StandardMaterial>>();
        kids.into_iter()
            .filter_map(|k| {
                let card = world.get::<ShieldCard>(k)?;
                let g = world.get::<GlobalTransform>(k)?.compute_transform();
                let m = world.get::<MeshMaterial3d<StandardMaterial>>(k)?;
                let alpha = materials.get(&m.0)?.base_color.alpha();
                Some((card.layer, g, alpha))
            })
            .collect()
    }

    /// Every card's transparent-pass sort bias, read off its live material.
    fn sort_biases(&mut self, bearer: Entity) -> Vec<(ShieldLayer, f32)> {
        let mut rigs = self.app.world_mut().query::<(&ShieldVisual, &Children)>();
        let kids: Vec<Entity> = rigs
            .iter(self.app.world())
            .filter(|(r, _)| r.bearer == bearer && r.kind == ShieldKind::IceBarrier)
            .flat_map(|(_, c)| c.iter().collect::<Vec<_>>())
            .collect();
        let world = self.app.world();
        let materials = world.resource::<Assets<StandardMaterial>>();
        kids.into_iter()
            .filter_map(|k| {
                let card = world.get::<ShieldCard>(k)?;
                let m = materials.get(&world.get::<MeshMaterial3d<StandardMaterial>>(k)?.0)?;
                Some((card.layer, m.depth_bias))
            })
            .collect()
    }

    fn card(&mut self, bearer: Entity, layer: ShieldLayer) -> (Transform, f32) {
        let c = self
            .cards(bearer)
            .into_iter()
            .find(|c| c.0 == layer)
            .unwrap_or_else(|| panic!("no {layer:?} card on the bearer"));
        (c.1, c.2)
    }

    /// The world half-size of a quad card (its mesh is 2 × 2).
    fn half(&mut self, bearer: Entity, layer: ShieldLayer) -> Vec2 {
        self.card(bearer, layer).0.scale.truncate()
    }

    /// The glint's live vertex alphas, one per vertex.
    fn glint_alphas(&mut self, bearer: Entity) -> Vec<f32> {
        let mut rigs = self.app.world_mut().query::<(&ShieldVisual, &Children)>();
        let kids: Vec<Entity> = rigs
            .iter(self.app.world())
            .filter(|(r, _)| r.bearer == bearer && r.kind == ShieldKind::IceBarrier)
            .flat_map(|(_, c)| c.iter().collect::<Vec<_>>())
            .collect();
        let world = self.app.world();
        let glint = kids
            .into_iter()
            .find(|k| world.get::<ShieldCard>(*k).map(|c| c.layer) == Some(ShieldLayer::Glint))
            .expect("no glint card");
        let mesh = world.get::<Mesh3d>(glint).unwrap();
        vertex_colours(world.resource::<Assets<Mesh>>().get(&mesh.0).unwrap())
            .iter()
            .map(|c| c[3])
            .collect()
    }

    /// Every mist puff: (entity, world position, diameter, linear colour, alpha).
    fn mist(&mut self) -> Vec<(Entity, Vec3, f32, LinearRgba, f32)> {
        let mut q = self.app.world_mut().query::<(
            Entity,
            &ClientParticle,
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
        )>();
        let world = self.app.world();
        let materials = world.resource::<Assets<StandardMaterial>>();
        q.iter(world)
            .map(|(e, _, g, m)| {
                let t = g.compute_transform();
                let c = materials.get(&m.0).unwrap().base_color.to_linear();
                (e, t.translation, t.scale.x, c, c.alpha)
            })
            .collect()
    }

    /// Ticks until the bearer's rig is `at` seconds into `phase`.
    fn run_to(&mut self, bearer: Entity, phase: ShieldPhase, at: f32) {
        for _ in 0..2000 {
            if let Some(r) = self.rig_of(bearer) {
                if r.phase == phase && r.t >= at {
                    return;
                }
            }
            self.tick(1);
        }
        panic!("the barrier never reached {phase:?} at {at}s");
    }
}

fn vertex_colours(mesh: &Mesh) -> Vec<[f32; 4]> {
    match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
        Some(VertexAttributeValues::Float32x4(c)) => c.clone(),
        other => panic!("no vertex colours: {other:?}"),
    }
}

fn absorb(name: &str) -> Aura {
    Aura {
        effect_type: AuraType::Absorb,
        duration: 60.0,
        magnitude: 60.0,
        ability_name: name.to_string(),
        ..Default::default()
    }
}

/// The client's rest half-size of a quad card on our rig.
fn rest_half(layer: ShieldLayer) -> Vec2 {
    let q = if layer == ShieldLayer::Rim {
        PWS_RIM_QUAD
    } else {
        PWS_STREAK_QUAD
    };
    Vec2::new(q[0], q[1]) / 2.0 * PWS_CLIENT_TO_RIG
}

const QUADS: [ShieldLayer; 3] = [ShieldLayer::Rim, ShieldLayer::StreakA, ShieldLayer::StreakB];

fn standard_harness() -> (Harness, Entity, Entity) {
    let mut h = Harness::new();
    h.camera(Vec3::new(0.0, 40.0, 30.0), Vec3::new(0.0, 1.0, 0.0));
    let (bearer, body) = h.bearer(Vec3::new(0.0, 1.0, 0.0));
    (h, bearer, body)
}

/// The barrier is keyed on the aura's name, which is the ability's RON name: a
/// rename there must fail here, not silently fall back to the sphere.
#[test]
fn the_aura_name_is_the_ability_name() {
    let defs = AbilityDefinitions::default();
    assert_eq!(
        defs.get_unchecked(&AbilityType::IceBarrier).name,
        ICE_BARRIER_AURA_NAME
    );
}

/// The client's birth: nothing on the cards for 200 ms, then a TALL, NARROW
/// pop at full strength at 300 ms, the icicles popping with them.
#[test]
fn the_barrier_arrives_late_tall_and_bright() {
    let (mut h, bearer, _) = standard_harness();
    h.give(bearer, ICE_BARRIER_AURA_NAME);
    h.run_to(bearer, ShieldPhase::Birth, 0.10);
    let cards = h.cards(bearer);
    assert_eq!(
        cards.len(),
        ShieldKind::IceBarrier.layers().len(),
        "rim, two streak rings, icicles, glint and the outline"
    );
    for (layer, at, _) in &cards {
        assert!(
            at.scale.truncate().max_element() < 1e-3,
            "{layer:?} drawn before 200 ms: {}",
            at.scale
        );
    }
    for layer in QUADS {
        assert!(
            h.card(bearer, layer).1 < 1e-3,
            "{layer:?} lit before 200 ms"
        );
    }

    h.run_to(bearer, ShieldPhase::Birth, 0.296);
    for layer in QUADS {
        let (_, alpha) = h.card(bearer, layer);
        let half = h.half(bearer, layer);
        let rest = rest_half(layer);
        assert!(
            alpha > 0.9,
            "{layer:?} is not at full strength at the pop: {alpha}"
        );
        assert!(
            half.x / rest.x < 0.6 && half.y / rest.y > 1.05,
            "{layer:?} does not pop tall and narrow: width ×{:.2}, height ×{:.2}",
            half.x / rest.x,
            half.y / rest.y
        );
    }
    let (icicles, weight) = h.card(bearer, ShieldLayer::Icicles);
    assert!(
        icicles.scale.x < 0.6 && icicles.scale.y > 1.05,
        "the icicles do not pop with the cards: {}",
        icicles.scale
    );
    assert!((weight - 0.6).abs() < 1e-3, "icicle weight {weight}");
}

/// The held barrier is bright, unlike Power Word: Shield's faint one: the rim
/// sits on the client's 0.12–0.30 × `IB_RIM_GAIN` (AS-401), streak A never drops below 0.6, streak B
/// flickers between dark and 0.6 against it, and the rim's inner edge clears
/// the body at every frame of the wobble.
#[test]
fn the_held_barrier_is_bright_and_shimmers() {
    let (mut h, bearer, _) = standard_harness();
    h.give(bearer, ICE_BARRIER_AURA_NAME);
    h.run_to(bearer, ShieldPhase::Loop, 0.0);
    let (mut a_range, mut b_range) = ((f32::MAX, 0.0_f32), (f32::MAX, 0.0_f32));
    for _ in 0..80 {
        h.tick(1);
        let body = Vec3::new(0.0, 1.0, 0.0);
        let (rim_at, rim) = h.card(bearer, ShieldLayer::Rim);
        let (_, a) = h.card(bearer, ShieldLayer::StreakA);
        let (_, b) = h.card(bearer, ShieldLayer::StreakB);
        assert!(
            (0.119 * IB_RIM_GAIN..=0.301 * IB_RIM_GAIN).contains(&rim),
            "rim weight {rim} outside the client's 0.12–0.30 × {IB_RIM_GAIN}"
        );
        assert!((0.599..=1.0).contains(&a), "streak A left 0.6–1.0: {a}");
        assert!(b <= 0.601, "streak B too bright: {b}");
        a_range = (a_range.0.min(a), a_range.1.max(a));
        b_range = (b_range.0.min(b), b_range.1.max(b));
        let inner = 0.5 * rim_at.scale.x;
        assert!(
            inner >= BODY_RADIUS + 0.1,
            "the rim's inner edge ({inner:.2}) does not clear the body"
        );
        assert!(
            rim_at.translation.y > body.y,
            "the rim is centred below the body"
        );
        assert!((h.card(bearer, ShieldLayer::Icicles).1 - 0.6).abs() < 1e-3);
        assert!((h.card(bearer, ShieldLayer::Glint).1 - 0.1).abs() < 1e-3);
    }
    assert!(
        a_range.1 - a_range.0 > 0.3,
        "streak A never flickered: {a_range:?}"
    );
    assert!(
        b_range.0 < 0.05 && b_range.1 > 0.35,
        "streak B never flickered: {b_range:?}"
    );
    assert_eq!(h.rig_of(bearer).unwrap().phase, ShieldPhase::Loop);
}

/// The icicles frame the body, read off the live mesh as the placed card
/// carries it: four clusters, two groups over the left shoulder (in the
/// camera's view), one over the right and one at each knee, none at the waist,
/// all of it on the ring rather than out past it, and all of it blue.
#[test]
fn the_icicles_frame_the_body_at_shoulders_and_knees() {
    let (mut h, bearer, _) = standard_harness();
    h.give(bearer, ICE_BARRIER_AURA_NAME);
    h.run_to(bearer, ShieldPhase::Loop, 0.1);
    let (card, _) = h.card(bearer, ShieldLayer::Icicles);
    let (rim, _) = h.card(bearer, ShieldLayer::Rim);
    let centre = Vec3::new(0.0, 1.0, 0.0);
    let right = card.rotation * Vec3::X;
    let world = h.app.world();
    let mut q = world.try_query::<(&ShieldCard, &Mesh3d)>().unwrap();
    let (_, mesh) = q
        .iter(world)
        .find(|(c, _)| c.layer == ShieldLayer::Icicles)
        .unwrap();
    let mesh = world.resource::<Assets<Mesh>>().get(&mesh.0).unwrap();
    let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(p)) => p.clone(),
        other => panic!("no positions: {other:?}"),
    };
    // 15 icicles of a light facet, a deep facet and a six-point cap.
    assert_eq!(positions.len(), 15 * 12);
    // Vertices per quadrant of the body: [upper left, upper right, lower
    // left, lower right].
    let mut quadrants = [0_u32; 4];
    for p in &positions {
        let at = card.transform_point(Vec3::from_array(*p));
        let across = (at - centre).dot(right);
        let up = at.y - centre.y;
        assert!(
            up > 0.4 || up < -0.4,
            "icicle ice at the waist, not the shoulders or knees: {up:.2}"
        );
        quadrants[(up < 0.0) as usize * 2 + (across > 0.0) as usize] += 1;
        // Within the rim's glow, which fades out at 0.70 of its half-size, as
        // an ellipse about the rim's own centre.
        let ex = across / (0.70 * rim.scale.x);
        let ey = (at.y - rim.translation.y) / (0.70 * rim.scale.y);
        assert!(
            ex * ex + ey * ey < 1.0,
            "icicle ice hangs out past the rim: ({across:.2}, {up:.2})"
        );
    }
    assert_eq!(
        quadrants,
        [5 * 12, 3 * 12, 3 * 12, 4 * 12],
        "the clusters are misplaced"
    );
    for c in &vertex_colours(mesh) {
        assert!(c[2] > c[0], "an icicle vertex is not blue: {c:?}");
    }
}

/// The glint is the view-dependent pass: turning the camera round the Mage
/// changes its sheen, and a still camera leaves it alone.
#[test]
fn the_glint_turns_with_the_view() {
    let mut h = Harness::new();
    let target = Vec3::new(0.0, 1.0, 0.0);
    let cam = h.camera(Vec3::new(0.0, 40.0, 30.0), target);
    let (bearer, _) = h.bearer(target);
    h.give(bearer, ICE_BARRIER_AURA_NAME);
    h.run_to(bearer, ShieldPhase::Loop, 0.1);
    let before = h.glint_alphas(bearer);
    h.tick(3);
    assert_eq!(
        before,
        h.glint_alphas(bearer),
        "the sheen moved with a still camera"
    );
    *h.app.world_mut().get_mut::<Transform>(cam).unwrap() =
        Transform::from_xyz(30.0, 40.0, 0.0).looking_at(target, Vec3::Y);
    h.tick(1);
    let after = h.glint_alphas(bearer);
    let moved = before
        .iter()
        .zip(&after)
        .filter(|(a, b)| (*a - *b).abs() > 0.05)
        .count();
    assert!(
        moved > 6,
        "the sheen did not turn with the view ({moved} vertices)"
    );
}

/// What the barrier does as it goes, per frame: (rim width ×, rim weight,
/// icicle weight, glint weight).
fn ending_frames(h: &mut Harness, bearer: Entity) -> Vec<(f32, f32, f32, f32)> {
    let mut out = Vec::new();
    for _ in 0..200 {
        h.tick(1);
        if h.rig_of(bearer).is_none() {
            return out;
        }
        let (rim, rim_w) = h.card(bearer, ShieldLayer::Rim);
        let (_, ice) = h.card(bearer, ShieldLayer::Icicles);
        let (_, glint) = h.card(bearer, ShieldLayer::Glint);
        out.push((
            rim.scale.x / rest_half(ShieldLayer::Rim).x,
            rim_w,
            ice,
            glint,
        ));
    }
    panic!("the barrier never finished going");
}

/// A break and an expiry play the client's one ending, frame for frame: the
/// outline swells past anything the held barrier does and fades, the glint is
/// out within 0.2 s and the icicles by 0.85 s, all within 1.1 s.
#[test]
fn break_and_expiry_share_one_swell_and_fade() {
    let mut h = Harness::new();
    h.camera(Vec3::new(0.0, 40.0, 30.0), Vec3::new(0.0, 1.0, 0.0));
    let (broken, _) = h.bearer(Vec3::new(-3.0, 1.0, 0.0));
    let (expired, _) = h.bearer(Vec3::new(3.0, 1.0, 0.0));
    h.give(broken, ICE_BARRIER_AURA_NAME);
    h.give(expired, ICE_BARRIER_AURA_NAME);
    h.run_to(broken, ShieldPhase::Loop, 0.2);
    h.break_barrier(broken);
    h.expire(expired);

    let mut both = (Vec::new(), Vec::new());
    for _ in 0..200 {
        h.tick(1);
        if h.rig_of(broken).is_none() && h.rig_of(expired).is_none() {
            break;
        }
        let sample = |h: &mut Harness, who| {
            let (rim, w) = h.card(who, ShieldLayer::Rim);
            let (_, ice) = h.card(who, ShieldLayer::Icicles);
            let (_, glint) = h.card(who, ShieldLayer::Glint);
            (rim.scale.x / rest_half(ShieldLayer::Rim).x, w, ice, glint)
        };
        both.0.push(sample(&mut h, broken));
        both.1.push(sample(&mut h, expired));
    }
    assert_eq!(both.0, both.1, "a break and an expiry end differently");
    let frames = both.0.len();
    assert!(
        (60..=75).contains(&frames),
        "the ending lasted {frames} frames, not the client's 1.1 s"
    );
    let widest = both.0.iter().map(|s| s.0).fold(0.0, f32::max);
    assert!(widest > 1.2, "the outline never swelled: ×{widest:.2}");
    assert!(both.0.last().unwrap().1 < 0.01, "the rim had not faded");
    let frame_at = |secs: f32| (secs / DT) as usize;
    assert!(both.0[frame_at(0.2)].3 < 1e-3, "the glint outlived 0.2 s");
    assert!(both.0[frame_at(0.1)].2 > 0.4, "the icicles faded too soon");
    assert!(
        both.0[frame_at(0.87)].2 < 1e-3,
        "the icicles outlived 0.85 s"
    );
    assert!(h.cards(broken).is_empty() && h.cards(expired).is_empty());

    // A purge (the aura simply gone) takes the same road.
    let (purged, _) = h.bearer(Vec3::new(0.0, 1.0, 6.0));
    h.give(purged, ICE_BARRIER_AURA_NAME);
    h.run_to(purged, ShieldPhase::Loop, 0.2);
    h.expire(purged);
    let seq = ending_frames(&mut h, purged);
    assert!(seq.iter().any(|s| s.0 > 1.2) && seq.last().unwrap().1 < 0.01);
}

/// The frost mist: from the cast, puffs appear above the head and fall at the
/// client's speed, brightening and then fading and whitening as they go, about
/// 21 alive at once.
#[test]
fn the_mist_falls_from_above_the_head() {
    let (mut h, bearer, _) = standard_harness();
    h.give(bearer, ICE_BARRIER_AURA_NAME);
    // 11.7 a second: the first puff is owed within a tenth of a second.
    let mut frames = 0;
    while h.mist().is_empty() {
        assert!(frames < 8, "no mist within 0.13 s of the cast");
        h.tick(1);
        frames += 1;
    }
    let born = h.mist();
    let head = 1.0 + BODY_HALF_HEIGHT;
    for (_, at, _, _, _) in &born {
        assert!(
            at.y > head + 0.3,
            "a puff was born at {:.2}, not above the head",
            at.y
        );
    }

    // Follow one puff through its life.
    let (id, start, _, _, _) = born[0];
    let mut track = Vec::new();
    for _ in 0..200 {
        h.tick(1);
        match h.mist().into_iter().find(|p| p.0 == id) {
            Some(p) => track.push(p),
            None => break,
        }
    }
    let life = (track.len() + 1) as f32 * DT;
    assert!(
        (life - 1.8).abs() < 0.05,
        "a puff lived {life:.2} s, not 1.8 s"
    );
    let fall = start.y - track.last().unwrap().1.y;
    let speed = fall / (track.len() as f32 * DT);
    let client = 1.667 * PWS_CLIENT_TO_RIG;
    assert!(
        speed > client * 0.75 && speed < client * 1.25,
        "the puff fell at {speed:.2} yd/s, not about {client:.2}"
    );
    let peak = track.iter().map(|p| p.4).fold(0.0, f32::max);
    assert!(
        (0.3..=0.4).contains(&peak),
        "the puff peaked at alpha {peak}"
    );
    let early = &track[track.len() / 5];
    let late = &track[track.len() * 4 / 5];
    assert!(early.2 < late.2, "the puff did not grow");
    assert!(
        late.3.red / late.3.blue > early.3.red / early.3.blue,
        "the puff did not whiten"
    );

    let alive = h.mist().len();
    assert!(
        (17..=26).contains(&alive),
        "{alive} puffs alive, not about 21"
    );
}

/// When the barrier ends the mist stops spawning within the client's 767 ms,
/// and the puffs already falling outlive the cards, then clear.
#[test]
fn the_mist_outlives_the_barrier_then_clears() {
    let (mut h, bearer, _) = standard_harness();
    h.give(bearer, ICE_BARRIER_AURA_NAME);
    h.run_to(bearer, ShieldPhase::Loop, 0.5);
    h.expire(bearer);
    let mut newest_birth = 0.0;
    let mut seen: Vec<Entity> = h.mist().iter().map(|p| p.0).collect();
    let mut t = 0.0;
    let mut gone_at = None;
    for _ in 0..400 {
        h.tick(1);
        t += DT;
        for p in h.mist() {
            if !seen.contains(&p.0) {
                seen.push(p.0);
                newest_birth = t;
            }
        }
        if h.rig_of(bearer).is_none() && gone_at.is_none() {
            gone_at = Some(t);
            assert!(!h.mist().is_empty(), "the mist went with the cards");
        }
        if h.mist().is_empty() {
            break;
        }
    }
    assert!(
        (0.6..=0.8).contains(&newest_birth),
        "the last puff was born {newest_birth:.2} s into the death, not by 0.767 s"
    );
    assert!(gone_at.is_some(), "the cards never went");
    assert!(h.mist().is_empty(), "the mist never cleared");
    assert!(t < 0.8 + 1.8 + 0.1, "the mist lingered {t:.2} s");
}

/// Each card is upright and faces the camera across the floor: its normal is
/// horizontal and points at the camera's ground position, its up is world up.
#[test]
fn the_cards_stand_upright_facing_the_camera() {
    let mut h = Harness::new();
    let eye = Vec3::new(17.0, 30.0, -22.0);
    h.camera(eye, Vec3::new(2.0, 1.0, 3.0));
    let (bearer, _) = h.bearer(Vec3::new(2.0, 1.0, 3.0));
    h.give(bearer, ICE_BARRIER_AURA_NAME);
    h.run_to(bearer, ShieldPhase::Loop, 0.1);
    let cards = h.cards(bearer);
    assert_eq!(cards.len(), ShieldKind::IceBarrier.layers().len());
    for (layer, at, _) in cards {
        let normal = at.rotation * Vec3::Z;
        let want = (eye - at.translation).with_y(0.0).normalize();
        assert!(normal.y.abs() < 1e-4, "{layer:?} tips toward the camera");
        assert!(
            normal.dot(want) > 0.9999,
            "{layer:?} faces {normal}, not {want}"
        );
        assert!(
            (at.rotation * Vec3::Y - Vec3::Y).length() < 1e-4,
            "{layer:?} is not upright"
        );
    }
}

/// Power Word: Shield's draw-order proof, for all five cards: they ride the
/// RENDERED body, so at every pitch and yaw the camera allows and under every
/// body offset the animation layers write, each card sorts in front of the
/// blended capsule (`ViewRangefinder3d`, as Bevy sorts `Transparent3d`).
#[test]
fn the_cards_sort_in_front_of_the_body_at_every_pitch() {
    let offsets = [
        Vec3::new(0.0, 0.22, 0.0),
        Vec3::new(0.0, 0.28, 0.0),
        Vec3::new(0.18, 0.10, 0.0),
        Vec3::new(-0.18, 0.10, 0.0),
        Vec3::new(0.0, 0.10, 0.18),
        Vec3::new(0.0, 0.10, -0.18),
        Vec3::new(0.0, 1.2, 0.0),
        Vec3::new(0.0, -0.10, 0.0),
    ];
    let target = Vec3::new(0.0, 1.0, 0.0);
    let mut checked = 0;
    for offset in offsets {
        let mut h = Harness::new();
        let cam = h.camera(Vec3::new(0.0, 40.0, 30.0), target);
        let (bearer, body) = h.bearer(target);
        h.app
            .world_mut()
            .get_mut::<Transform>(body)
            .unwrap()
            .translation = offset;
        h.give(bearer, ICE_BARRIER_AURA_NAME);
        h.run_to(bearer, ShieldPhase::Loop, 0.0);
        for pitch in [0.1_f32, 0.4, 0.675, 1.0, 1.3, 1.5] {
            for yaw in [0.0_f32, 1.3, 2.9, 4.4] {
                let d = 40.0;
                let eye = target
                    + Vec3::new(
                        d * pitch.sin() * yaw.sin(),
                        d * pitch.cos(),
                        d * pitch.sin() * yaw.cos(),
                    );
                *h.app.world_mut().get_mut::<Transform>(cam).unwrap() =
                    Transform::from_translation(eye).looking_at(target, Vec3::Y);
                h.tick(1);
                let view = h
                    .app
                    .world()
                    .get::<GlobalTransform>(cam)
                    .unwrap()
                    .compute_matrix();
                let range = ViewRangefinder3d::from_world_from_view(&view);
                let body_at = h
                    .app
                    .world()
                    .get::<GlobalTransform>(body)
                    .unwrap()
                    .translation();
                let body_depth = range.distance_translation(&body_at);
                // Bevy's transparent sort key: view depth plus the material's
                // depth bias (larger draws later, in front).
                let biases = h.sort_biases(bearer);
                let key = |layer: ShieldLayer, at: Vec3| {
                    range.distance_translation(&at)
                        + biases.iter().find(|b| b.0 == layer).unwrap().1
                };
                let cards = h.cards(bearer);
                for (layer, at, _) in &cards {
                    assert!(
                        (at.translation.xz() - body_at.xz()).length() < 1e-4,
                        "{layer:?} does not stand on the body (offset {offset})"
                    );
                    assert!(
                        key(*layer, at.translation) > body_depth,
                        "{layer:?} sorts behind the body at pitch {pitch}, yaw {yaw}, offset {offset}"
                    );
                    checked += 1;
                }
                // The blended outline draws before (behind) every additive
                // card, so it cannot dim their light (AS-401).
                let outline = cards
                    .iter()
                    .find(|c| c.0 == ShieldLayer::Outline)
                    .unwrap()
                    .1;
                for (layer, at, _) in &cards {
                    if *layer != ShieldLayer::Outline {
                        assert!(
                            key(ShieldLayer::Outline, outline.translation)
                                < key(*layer, at.translation),
                            "the outline sorts over {layer:?} at pitch {pitch}, yaw {yaw}"
                        );
                    }
                }
            }
        }
    }
    assert_eq!(checked, 8 * 6 * 4 * ShieldKind::IceBarrier.layers().len());
}

/// The anchor, read off the same function the system calls: every card
/// centre sits above the body centre at every frame of the barrier's life.
#[test]
fn every_card_centre_sits_above_the_body_centre() {
    for layer in ShieldKind::IceBarrier.layers().iter().copied() {
        for (w, hgt) in [
            (0.0, 0.0),
            (0.522, 1.11),
            (0.901, 1.123),
            (1.04, 1.0),
            (1.271, 1.158),
        ] {
            let (centre, _) = shield_card_extent(ShieldKind::IceBarrier, layer, Vec2::new(w, hgt));
            assert!(
                centre - BODY_HALF_HEIGHT > 0.1,
                "{layer:?} centre {centre:.3} is not above the body centre at ({w}, {hgt})"
            );
        }
    }
}

/// Ice Barrier wears its own model, in frost blue, never Power Word: Shield's
/// gold; and a Mage wearing both absorbs wears both models at once.
#[test]
fn ice_barrier_wears_its_own_model_beside_power_word_shield() {
    let (mut h, mage, _) = standard_harness();
    h.give(mage, ICE_BARRIER_AURA_NAME);
    h.tick(3);
    let kinds = |h: &mut Harness| {
        let mut q = h.app.world_mut().query::<&ShieldVisual>();
        let mut k: Vec<ShieldKind> = q.iter(h.app.world()).map(|r| r.kind).collect();
        k.sort_by_key(|k| *k as u8);
        k
    };
    assert_eq!(kinds(&mut h), vec![ShieldKind::IceBarrier]);
    let rim = shield_layer_rgb(ShieldKind::IceBarrier, ShieldLayer::Rim);
    assert!(
        rim[2] > rim[0] && rim[2] > rim[1],
        "the barrier's rim is not frost blue: {rim:?}"
    );
    let world = h.app.world();
    let mut q = world
        .try_query::<(&ShieldCard, &MeshMaterial3d<StandardMaterial>)>()
        .unwrap();
    let materials = world.resource::<Assets<StandardMaterial>>();
    let drawn = q
        .iter(world)
        .find(|(c, _)| c.layer == ShieldLayer::Rim)
        .map(|(_, m)| materials.get(&m.0).unwrap().base_color.to_srgba())
        .unwrap();
    assert!(
        (drawn.red - rim[0]).abs() < 1e-3 && (drawn.blue - rim[2]).abs() < 1e-3,
        "the rim drew {drawn:?}, not the client's {rim:?}"
    );

    h.give(mage, PW_SHIELD_AURA_NAME);
    h.tick(3);
    assert_eq!(
        kinds(&mut h),
        vec![ShieldKind::PowerWordShield, ShieldKind::IceBarrier],
        "the barrier did not stand beside PW:S"
    );
}

/// The streak rings swap textures against Power Word: Shield's, as the client
/// does: Ice Barrier's bright ring A wears the `shockwave10b` ring PW:S wears
/// as B, and its B wears PW:S's A. The rim wears the same rim.
#[test]
fn the_streak_rings_wear_the_clients_textures() {
    let (mut h, mage, _) = standard_harness();
    let (priest_target, _) = h.bearer(Vec3::new(4.0, 1.0, 0.0));
    h.give(mage, ICE_BARRIER_AURA_NAME);
    h.give(priest_target, PW_SHIELD_AURA_NAME);
    h.tick(3);
    let world = h.app.world();
    let materials = world.resource::<Assets<StandardMaterial>>();
    let mut q = world.try_query::<(&ShieldVisual, &Children)>().unwrap();
    let rigs: Vec<(ShieldKind, Vec<Entity>)> = q
        .iter(world)
        .map(|(r, kids)| (r.kind, kids.iter().collect()))
        .collect();
    let texture = |kind: ShieldKind, layer: ShieldLayer| {
        let (_, kids) = rigs
            .iter()
            .find(|(k, _)| *k == kind)
            .expect("no rig of that kind");
        kids.iter()
            .copied()
            .find_map(|k| {
                let card = world.get::<ShieldCard>(k)?;
                (card.layer == layer).then(|| {
                    let m = world.get::<MeshMaterial3d<StandardMaterial>>(k).unwrap();
                    materials.get(&m.0).unwrap().base_color_texture.clone()
                })
            })
            .expect("no card of that layer")
    };
    let ib = |l| texture(ShieldKind::IceBarrier, l);
    let pws = |l| texture(ShieldKind::PowerWordShield, l);
    assert!(ib(ShieldLayer::StreakA).is_some());
    assert_eq!(ib(ShieldLayer::StreakA), pws(ShieldLayer::StreakB));
    assert_eq!(ib(ShieldLayer::StreakB), pws(ShieldLayer::StreakA));
    assert_eq!(ib(ShieldLayer::Rim), pws(ShieldLayer::Rim));
    assert_ne!(ib(ShieldLayer::StreakA), ib(ShieldLayer::StreakB));
}

// --- the bearer going away ---------------------------------------------------

/// A bearer removed from the world takes its barrier with it on the next
/// frame: the rig and all its cards.
#[test]
fn a_despawned_bearer_takes_its_barrier_within_a_frame() {
    let (mut h, bearer, _) = standard_harness();
    h.give(bearer, ICE_BARRIER_AURA_NAME);
    h.run_to(bearer, ShieldPhase::Loop, 0.1);
    assert_eq!(h.cards(bearer).len(), ShieldKind::IceBarrier.layers().len());
    h.app.world_mut().entity_mut(bearer).despawn();
    h.tick(1);
    assert!(h.rigs().is_empty(), "the rig outlived its bearer");
    let mut cards = h.app.world_mut().query::<&ShieldCard>();
    assert_eq!(
        cards.iter(h.app.world()).count(),
        0,
        "a card outlived its bearer"
    );
}

/// A Mage that dies keeps its auras on the corpse, but its barrier goes: it
/// plays the one ending and is gone by the end of it.
#[test]
fn a_dying_bearer_lets_its_barrier_go() {
    let (mut h, bearer, _) = standard_harness();
    h.give(bearer, ICE_BARRIER_AURA_NAME);
    h.run_to(bearer, ShieldPhase::Loop, 0.1);
    h.app
        .world_mut()
        .get_mut::<Combatant>(bearer)
        .unwrap()
        .current_health = 0.0;
    let seq = ending_frames(&mut h, bearer);
    assert!(
        (60..=75).contains(&seq.len()),
        "the barrier lasted {} frames past its bearer's death",
        seq.len()
    );
    assert!(seq.iter().any(|s| s.0 > 1.2), "it left without the swell");
    assert!(h
        .app
        .world()
        .get::<ActiveAuras>(bearer)
        .unwrap()
        .auras
        .iter()
        .any(|a| a.ability_name == ICE_BARRIER_AURA_NAME));
}

// --- the caster's hands -------------------------------------------------------

fn hands_app() -> (App, Entity) {
    let mut app = base_app();
    app.add_systems(
        Update,
        (spawn_instant_cast_hands, update_heal_cast_glows).chain(),
    );
    let mage = app
        .world_mut()
        .spawn((
            Combatant::new(0, 0, CharacterClass::Mage),
            Transform::from_xyz(0.0, 1.0, 0.0),
        ))
        .id();
    let body = app
        .world_mut()
        .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
        .id();
    app.world_mut().entity_mut(mage).add_child(body);
    (app, mage)
}

fn hand_kinds(app: &mut App) -> Vec<HealCastKind> {
    let mut q = app.world_mut().query::<&HealCastHand>();
    q.iter(app.world()).map(|h| h.kind).collect()
}

fn fire(app: &mut App, mage: Entity) {
    app.world_mut().spawn(InstantAbilityFired {
        caster: mage,
        target: Some(mage),
        ability: AbilityType::IceBarrier,
        is_crit: false,
    });
}

/// Every Frost hand piece now: (role, world diameter, alpha).
fn frost_pieces(app: &mut App) -> Vec<(HealCastPieceRole, f32, f32)> {
    let mut q = app.world_mut().query::<(
        &HealCastPiece,
        &GlobalTransform,
        &MeshMaterial3d<StandardMaterial>,
    )>();
    let world = app.world();
    let materials = world.resource::<Assets<StandardMaterial>>();
    q.iter(world)
        .map(|(p, g, m)| {
            (
                p.role,
                g.compute_transform().scale.x,
                materials.get(&m.0).unwrap().base_color.alpha(),
            )
        })
        .collect()
}

/// The cast flashes both of the Mage's hands with the Frost launch (kit 207)
/// off the instant marker: a deep-blue glow card about 0.93 yd across, a ring
/// at 0.75 and four sparks each, for one second, and then nothing.
#[test]
fn a_barrier_cast_flashes_both_frost_hands_for_a_second() {
    let (mut app, mage) = hands_app();
    fire(&mut app, mage);
    app.update();
    assert_eq!(
        hand_kinds(&mut app),
        vec![HealCastKind::Frost, HealCastKind::Frost]
    );
    for _ in 0..30 {
        app.update();
    }
    let pieces = frost_pieces(&mut app);
    assert_eq!(pieces.len(), 2 * 6, "glow, ring and four sparks a hand");
    let glow = 0.76 * PWS_CLIENT_TO_RIG;
    for (role, size, alpha) in &pieces {
        match role {
            HealCastPieceRole::FrostGlow => {
                assert!((size / glow - 1.0).abs() < 0.15, "glow {size:.2} yd");
                assert!(*alpha > 0.99, "glow alpha {alpha}");
            }
            HealCastPieceRole::FrostRing => {
                assert!((alpha - 0.75).abs() < 1e-3, "ring alpha {alpha}");
            }
            HealCastPieceRole::FrostSpark { .. } => {
                assert!((alpha - 0.9).abs() < 1e-3, "spark alpha {alpha}");
            }
            other => panic!("a non-Frost piece on a Frost hand: {other:?}"),
        }
    }
    for _ in 0..40 {
        app.update();
    }
    assert!(
        hand_kinds(&mut app).is_empty(),
        "the flash outlived its second"
    );
}

/// Where a zero-length cast lands it too (the animation sandbox lands the
/// entry through one AND spawns the marker), the hands still flash once.
#[test]
fn a_landed_barrier_with_its_marker_flashes_the_hands_once() {
    let (mut app, mage) = hands_app();
    app.world_mut().spawn((
        CastEnding {
            caster: mage,
            kind: CastEndingKind::Landed,
        },
        LandedCast {
            ability: AbilityType::IceBarrier,
        },
    ));
    app.update();
    assert!(
        hand_kinds(&mut app).is_empty(),
        "the Landed path must leave Ice Barrier to its marker"
    );
    fire(&mut app, mage);
    app.update();
    assert_eq!(hand_kinds(&mut app).len(), 2);
}

// --- the real Mage decision ----------------------------------------------------

const MAGE: &str = "mage";
const FOE: &str = "warrior";
const MAGE_XZ: (f32, f32) = (0.0, 0.0);

/// A Mage at full health with a Warrior across the arena.
fn barrier_scene() -> Scene {
    Scene::new()
        .unit(MAGE, 1, CharacterClass::Mage)
        .at(MAGE_XZ.0, MAGE_XZ.1)
        .unit(FOE, 2, CharacterClass::Warrior)
        .at(25.0, 0.0)
}

/// Replays what a scene decision spawned through the real hand system against
/// a rendered stand-in for the Mage, and returns every hand's world position.
fn hands_from_decision(view: &DecisionView) -> Vec<Vec3> {
    let mut app = base_app();
    app.add_systems(Update, spawn_instant_cast_hands);
    let mage = app
        .world_mut()
        .spawn((
            Combatant::new(1, 0, CharacterClass::Mage),
            Transform::from_xyz(MAGE_XZ.0, 1.0, MAGE_XZ.1),
        ))
        .id();
    let body = app
        .world_mut()
        .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
        .id();
    app.world_mut().entity_mut(mage).add_child(body);
    for marker in &view.instant_markers {
        assert_eq!(marker.caster, MAGE, "a marker named another caster");
        app.world_mut().spawn(InstantAbilityFired {
            caster: mage,
            target: marker.target.map(|_| mage),
            ability: marker.ability,
            is_crit: false,
        });
    }
    app.update();
    app.update();
    let mut hands = app.world_mut().query::<(&HealCastHand, &GlobalTransform)>();
    hands
        .iter(app.world())
        .map(|(hand, at)| {
            assert_eq!(
                hand.kind,
                HealCastKind::Frost,
                "Ice Barrier plays the Frost hands"
            );
            at.translation()
        })
        .collect()
}

/// The real Mage AI raises its barrier, and the Frost launch flashes on its
/// own two spell hands. The control: a Mage already wearing its barrier
/// raises none, and no hand lights.
#[test]
fn the_mages_own_hands_flash_when_it_raises_its_barrier() {
    let view = barrier_scene().decide(MAGE);
    assert!(
        view.choices
            .contains(&(AbilityType::IceBarrier, Some(MAGE))),
        "the Mage did not raise its barrier: {:?}",
        view.choices
    );
    let hands = hands_from_decision(&view);
    assert_eq!(hands.len(), 2, "two spell hands: {hands:?}");
    let mage = Vec3::new(MAGE_XZ.0, 1.0, MAGE_XZ.1);
    for at in &hands {
        assert!(
            at.distance(mage) < 1.2,
            "a hand lit {:.2} yd off",
            at.distance(mage)
        );
    }
    assert!((hands[0] - hands[1]).length() > 1.0, "{hands:?}");

    let view = barrier_scene()
        .aura_full(MAGE, absorb(ICE_BARRIER_AURA_NAME))
        .decide(MAGE);
    assert!(!view
        .choices
        .iter()
        .any(|(a, _)| *a == AbilityType::IceBarrier));
    assert!(
        hands_from_decision(&view).is_empty(),
        "no barrier, no hands"
    );
}
