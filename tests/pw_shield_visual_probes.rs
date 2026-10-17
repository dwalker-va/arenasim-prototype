//! Probes for Power Word: Shield's client cards (`rendering/effects/pw_shield.rs`).
//!
//! World-space geometry and timing through `GlobalTransform` and the live
//! material, never the rig's stored fields: the 200 ms delay and the tall pop,
//! the faint hollow shimmer that clears the body, the swell-and-fade that a
//! break and an expiry share, the upright camera-facing card, and the draw
//! order against the blended capsule at every camera pitch.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` with the two
//! systems in production order: the lifecycle in `Update`, the placement in
//! `PostUpdate` before propagation, as `states/mod.rs` registers them.

use std::time::Duration;

use bevy::prelude::*;
use bevy::render::render_phase::ViewRangefinder3d;
use bevy::time::TimeUpdateStrategy;
use bevy::transform::TransformSystem;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, CastEnding, CastEndingKind, Combatant, HealCastHand, HealCastKind,
    InstantAbilityFired, LandedCast, PwShieldCard, PwShieldVisual, PwsLayer, PwsPhase,
    ShieldBubble, VisualBody,
};
use arenasim::states::play_match::{
    place_pw_shield_cards, pws_card_extent, spawn_instant_cast_hands, update_pw_shield_visuals,
    update_shield_bubbles, PWS_CLIENT_TO_RIG, PWS_RIM_QUAD, PWS_STREAK_QUAD, PW_SHIELD_AURA_NAME,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
/// The capsule radius every body has.
const BODY_RADIUS: f32 = 0.5;

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
        app.insert_resource(AbilityDefinitions::default());
        app.add_systems(Update, (update_shield_bubbles, update_pw_shield_visuals));
        app.add_systems(
            PostUpdate,
            place_pw_shield_cards.before(TransformSystem::TransformPropagate),
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

    /// A bearer with the real rendered hierarchy: the sim entity at capsule
    /// centre height and a `VisualBody` child, as `spawn_combatant` builds it.
    fn bearer(&mut self, at: Vec3) -> (Entity, Entity) {
        let bearer = self
            .app
            .world_mut()
            .spawn((
                Combatant::new(0, 0, CharacterClass::Warrior),
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

    /// Breaks the shield: the absorb is used up.
    fn break_shield(&mut self, bearer: Entity) {
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

    /// The shield runs out: the aura is gone.
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

    fn rigs(&mut self) -> Vec<PwShieldVisual> {
        let mut q = self.app.world_mut().query::<&PwShieldVisual>();
        q.iter(self.app.world()).copied().collect()
    }

    fn rig_of(&mut self, bearer: Entity) -> Option<PwShieldVisual> {
        self.rigs().into_iter().find(|r| r.bearer == bearer)
    }

    /// Every card on screen for a bearer, as (layer, world centre, world
    /// half-size, world rotation, weight).
    fn cards(&mut self, bearer: Entity) -> Vec<(PwsLayer, Vec3, Vec2, Quat, f32)> {
        let mut rigs = self.app.world_mut().query::<(&PwShieldVisual, &Children)>();
        let kids: Vec<Entity> = rigs
            .iter(self.app.world())
            .filter(|(r, _)| r.bearer == bearer)
            .flat_map(|(_, c)| c.iter().collect::<Vec<_>>())
            .collect();
        let world = self.app.world();
        let materials = world.resource::<Assets<StandardMaterial>>();
        kids.into_iter()
            .filter_map(|k| {
                let card = world.get::<PwShieldCard>(k)?;
                let g = world.get::<GlobalTransform>(k)?.compute_transform();
                let m = world.get::<MeshMaterial3d<StandardMaterial>>(k)?;
                let alpha = materials.get(&m.0)?.base_color.alpha();
                Some((
                    card.layer,
                    g.translation,
                    g.scale.truncate(),
                    g.rotation,
                    alpha,
                ))
            })
            .collect()
    }

    fn card(&mut self, bearer: Entity, layer: PwsLayer) -> (Vec3, Vec2, Quat, f32) {
        let c = self
            .cards(bearer)
            .into_iter()
            .find(|c| c.0 == layer)
            .unwrap_or_else(|| panic!("no {layer:?} card on the bearer"));
        (c.1, c.2, c.3, c.4)
    }

    /// Ticks until the bearer's rig is `at` seconds into `phase`.
    fn run_to(&mut self, bearer: Entity, phase: PwsPhase, at: f32) {
        for _ in 0..2000 {
            if let Some(r) = self.rig_of(bearer) {
                if r.phase == phase && r.t >= at {
                    return;
                }
            }
            self.tick(1);
        }
        panic!("the shield never reached {phase:?} at {at}s");
    }
}

fn absorb(name: &str) -> Aura {
    Aura {
        effect_type: AuraType::Absorb,
        duration: 30.0,
        magnitude: 70.0,
        ability_name: name.to_string(),
        ..Default::default()
    }
}

/// The client's rest half-size of a card on our rig.
fn rest_half(layer: PwsLayer) -> Vec2 {
    let q = if layer == PwsLayer::Rim {
        PWS_RIM_QUAD
    } else {
        PWS_STREAK_QUAD
    };
    Vec2::new(q[0], q[1]) / 2.0 * PWS_CLIENT_TO_RIG
}

fn standard_harness() -> (Harness, Entity, Entity) {
    let mut h = Harness::new();
    h.camera(Vec3::new(0.0, 40.0, 30.0), Vec3::new(0.0, 1.0, 0.0));
    let (bearer, body) = h.bearer(Vec3::new(0.0, 1.0, 0.0));
    (h, bearer, body)
}

/// The shield is keyed on the aura's name, which is the ability's RON name: a
/// rename there must fail here, not silently fall back to the sphere.
#[test]
fn the_aura_name_is_the_ability_name() {
    let defs = AbilityDefinitions::default();
    assert_eq!(
        defs.get_unchecked(&AbilityType::PowerWordShield).name,
        PW_SHIELD_AURA_NAME
    );
}

/// The client's birth: nothing for 200 ms, then a TALL, NARROW pop at full
/// strength at 300 ms, three cards together.
#[test]
fn the_shield_arrives_late_tall_and_bright() {
    let (mut h, bearer, _) = standard_harness();
    h.give(bearer, PW_SHIELD_AURA_NAME);
    h.run_to(bearer, PwsPhase::Birth, 0.10);
    let cards = h.cards(bearer);
    assert_eq!(cards.len(), 3, "rim, streak A and streak B");
    for (layer, _, half, _, alpha) in &cards {
        assert!(
            half.max_element() < 1e-3,
            "{layer:?} drawn before 200 ms: {half}"
        );
        assert!(*alpha < 1e-3, "{layer:?} lit before 200 ms: {alpha}");
    }

    h.run_to(bearer, PwsPhase::Birth, 0.296);
    for layer in PwsLayer::ALL {
        let (_, half, _, alpha) = h.card(bearer, layer);
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
}

/// The sustained state is quiet and hollow: the rim stays at or under the
/// client's 0.15, the streaks under 0.2 and never both lit at once (they
/// flicker against each other), and the rim's inner edge clears the body at
/// every frame of the wobble.
#[test]
fn the_held_shield_is_faint_hollow_and_shimmers() {
    let (mut h, bearer, _) = standard_harness();
    h.give(bearer, PW_SHIELD_AURA_NAME);
    h.run_to(bearer, PwsPhase::Loop, 0.0);
    let (mut a_lit, mut b_lit, mut frames) = (0, 0, 0);
    for _ in 0..80 {
        h.tick(1);
        let body = Vec3::new(0.0, 1.0, 0.0);
        let (rim_at, rim_half, _, rim) = h.card(bearer, PwsLayer::Rim);
        let (_, _, _, a) = h.card(bearer, PwsLayer::StreakA);
        let (_, _, _, b) = h.card(bearer, PwsLayer::StreakB);
        assert!(
            rim <= 0.151 && rim >= 0.059,
            "rim weight {rim} outside the client's 0.06–0.15"
        );
        assert!(a <= 0.201 && b <= 0.201, "a streak is too bright: {a} {b}");
        assert!(!(a > 0.1 && b > 0.1), "both streaks lit at once: {a} {b}");
        a_lit += (a > 0.1) as u32;
        b_lit += (b > 0.1) as u32;
        // The texture's inner edge is at half the card's half-size.
        let inner = 0.5 * rim_half.x;
        assert!(
            inner >= BODY_RADIUS + 0.1,
            "the rim's inner edge ({inner:.2}) does not clear the body"
        );
        assert!(
            rim_at.y > body.y,
            "the rim is centred below the body centre: {}",
            rim_at.y
        );
        frames += 1;
    }
    assert!(
        a_lit > 0 && b_lit > 0,
        "the shimmer never flickered: A {a_lit}, B {b_lit}"
    );
    assert_eq!(frames, 80);
    assert_eq!(h.rig_of(bearer).unwrap().phase, PwsPhase::Loop);
}

/// What the shield does as it goes, sampled per frame: (rim width ×, rim weight).
fn ending(h: &mut Harness, bearer: Entity) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    for _ in 0..200 {
        h.tick(1);
        if h.rig_of(bearer).is_none() {
            return out;
        }
        let (_, half, _, alpha) = h.card(bearer, PwsLayer::Rim);
        out.push((half.x / rest_half(PwsLayer::Rim).x, alpha));
    }
    panic!("the shield never finished going");
}

/// A break and an expiry play the client's one ending, frame for frame, and it
/// reads as an ending: the outline swells past anything the held shield does
/// and fades out, in 1.1 s, and then nothing is left.
#[test]
fn break_and_expiry_share_one_swell_and_fade() {
    let mut h = Harness::new();
    h.camera(Vec3::new(0.0, 40.0, 30.0), Vec3::new(0.0, 1.0, 0.0));
    let (broken, _) = h.bearer(Vec3::new(-3.0, 1.0, 0.0));
    let (expired, _) = h.bearer(Vec3::new(3.0, 1.0, 0.0));
    h.give(broken, PW_SHIELD_AURA_NAME);
    h.give(expired, PW_SHIELD_AURA_NAME);
    h.run_to(broken, PwsPhase::Loop, 0.2);
    h.break_shield(broken);
    h.expire(expired);

    let mut both = (Vec::new(), Vec::new());
    for _ in 0..200 {
        h.tick(1);
        let (b, e) = (h.rig_of(broken), h.rig_of(expired));
        if b.is_none() && e.is_none() {
            break;
        }
        let sample = |h: &mut Harness, who| {
            let (_, half, _, alpha) = h.card(who, PwsLayer::Rim);
            (half.x / rest_half(PwsLayer::Rim).x, alpha)
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
    assert!(
        widest > 1.2,
        "the outline never swelled past the held wobble: ×{widest:.2}"
    );
    assert!(
        both.0.last().unwrap().1 < 0.01,
        "the rim had not faded at the end"
    );
    assert!(h.cards(broken).is_empty() && h.cards(expired).is_empty());
    assert!(h.rigs().is_empty(), "a rig outlived its ending");

    // A dispel (the aura simply gone) takes the same road as an expiry.
    let (dispelled, _) = h.bearer(Vec3::new(0.0, 1.0, 6.0));
    h.give(dispelled, PW_SHIELD_AURA_NAME);
    h.run_to(dispelled, PwsPhase::Loop, 0.2);
    h.expire(dispelled);
    let seq = ending(&mut h, dispelled);
    assert!(seq.iter().any(|s| s.0 > 1.2) && seq.last().unwrap().1 < 0.01);
}

/// Each card is upright and faces the camera across the floor: its normal is
/// horizontal and points at the camera's ground position, its up is world up.
#[test]
fn the_cards_stand_upright_facing_the_camera() {
    let mut h = Harness::new();
    let eye = Vec3::new(17.0, 30.0, -22.0);
    h.camera(eye, Vec3::new(2.0, 1.0, 3.0));
    let (bearer, _) = h.bearer(Vec3::new(2.0, 1.0, 3.0));
    h.give(bearer, PW_SHIELD_AURA_NAME);
    h.run_to(bearer, PwsPhase::Loop, 0.1);
    for (layer, at, _, rot, _) in h.cards(bearer) {
        let normal = rot * Vec3::Z;
        let want = (eye - at).with_y(0.0).normalize();
        assert!(normal.y.abs() < 1e-4, "{layer:?} tips toward the camera");
        assert!(
            normal.dot(want) > 0.9999,
            "{layer:?} faces {normal}, not {want}"
        );
        assert!(
            (rot * Vec3::Y - Vec3::Y).length() < 1e-4,
            "{layer:?} is not upright"
        );
    }
}

/// The capsule is `AlphaMode::Blend` with no depth write, so the card and the
/// body are ordered by the view depth of their translations
/// (`ViewRangefinder3d`, as Bevy sorts `Transparent3d`). The cards ride the
/// RENDERED body — bob, fear bob, swing step, a Cyclone lift — so at every
/// pitch the camera allows (0.1–1.5 rad from vertical, `camera.rs`) and every
/// yaw, each card sorts in front of the body and the leg-crossing arc cannot
/// flicker. A card anchored on the sim transform fails this under a fear bob.
#[test]
fn the_cards_sort_in_front_of_the_body_at_every_pitch() {
    // Body offsets the animation layers write: a fear bob at its crest
    // (0.18 + 0.04 tremble), a sheep hop, a swing step each way, a cyclone lift,
    // and the walk bob's trough.
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
        h.give(bearer, PW_SHIELD_AURA_NAME);
        h.run_to(bearer, PwsPhase::Loop, 0.0);
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
                for (layer, at, _, _, _) in h.cards(bearer) {
                    assert!(
                        (at.xz() - body_at.xz()).length() < 1e-4,
                        "{layer:?} does not stand on the body (offset {offset})"
                    );
                    assert!(
                        range.distance_translation(&at) > body_depth,
                        "{layer:?} sorts behind the body at pitch {pitch}, yaw {yaw}, offset {offset}"
                    );
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 8 * 6 * 4 * 3);
}

/// The anchor, read off the same function the system calls: the card centres
/// sit above the body centre at every frame of the shield's life, which is
/// what the sort above rests on.
#[test]
fn every_card_centre_sits_above_the_body_centre() {
    let feet_to_centre = 1.25;
    for layer in PwsLayer::ALL {
        for (w, hgt) in [
            (0.0, 0.0),
            (0.522, 1.11),
            (0.901, 1.123),
            (1.04, 1.0),
            (1.271, 1.158),
        ] {
            let (centre, _) = pws_card_extent(layer, Vec2::new(w, hgt));
            assert!(
                centre - feet_to_centre > 0.1,
                "{layer:?} centre {centre:.3} is not above the body centre at scale ({w}, {hgt})"
            );
        }
    }
}

/// Ice Barrier keeps the sphere; Power Word: Shield never draws one.
#[test]
fn only_power_word_shield_leaves_the_sphere() {
    let (mut h, pws, _) = standard_harness();
    let (ice, _) = h.bearer(Vec3::new(4.0, 1.0, 0.0));
    h.give(pws, PW_SHIELD_AURA_NAME);
    h.give(ice, "Ice Barrier");
    h.tick(3);
    let mut q = h.app.world_mut().query::<&ShieldBubble>();
    let owners: Vec<Entity> = q.iter(h.app.world()).map(|b| b.combatant).collect();
    assert_eq!(owners, vec![ice], "the sphere is Ice Barrier's alone");
    assert_eq!(h.rigs().len(), 1);
    assert_eq!(
        h.rigs()[0].bearer,
        pws,
        "Ice Barrier must not get the cards"
    );
}

// --- the caster's hands -------------------------------------------------------

fn hands_app() -> (App, Entity) {
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
    app.add_systems(Update, spawn_instant_cast_hands);
    let priest = app
        .world_mut()
        .spawn((
            Combatant::new(0, 0, CharacterClass::Priest),
            Transform::from_xyz(0.0, 1.0, 0.0),
        ))
        .id();
    let body = app
        .world_mut()
        .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
        .id();
    app.world_mut().entity_mut(priest).add_child(body);
    (app, priest)
}

fn hand_kinds(app: &mut App) -> Vec<HealCastKind> {
    let mut q = app.world_mut().query::<&HealCastHand>();
    q.iter(app.world()).map(|h| h.kind).collect()
}

fn fire(app: &mut App, priest: Entity) {
    app.world_mut().spawn(InstantAbilityFired {
        caster: priest,
        target: None,
        ability: AbilityType::PowerWordShield,
        is_crit: false,
    });
}

/// The cast flashes both of the Priest's hands with the Holy launch (kit 270),
/// off the instant marker — PW:S never has a cast state.
#[test]
fn a_shield_cast_flashes_both_holy_hands() {
    let (mut app, priest) = hands_app();
    fire(&mut app, priest);
    app.update();
    assert_eq!(
        hand_kinds(&mut app),
        vec![HealCastKind::Holy, HealCastKind::Holy]
    );
}

/// Where a zero-length cast lands it too (the animation sandbox lands the entry
/// through one AND spawns the marker), the hands still flash once: the marker
/// owns the gesture of every ability it is spawned for.
#[test]
fn a_landed_shield_with_its_marker_flashes_the_hands_once() {
    let (mut app, priest) = hands_app();
    app.world_mut().spawn((
        CastEnding {
            caster: priest,
            kind: CastEndingKind::Landed,
        },
        LandedCast {
            ability: AbilityType::PowerWordShield,
        },
    ));
    app.update();
    assert!(
        hand_kinds(&mut app).is_empty(),
        "the Landed path must leave PW:S to its marker"
    );
    fire(&mut app, priest);
    app.update();
    assert_eq!(hand_kinds(&mut app).len(), 2);
}
