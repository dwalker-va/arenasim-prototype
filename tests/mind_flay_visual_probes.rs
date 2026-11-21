//! Probes for Mind Flay's channel visual (`rendering/effects/mind_flay.rs`,
//! AS-356): Drain Mana's flickering stars at the Priest's hand and the
//! victim's chest, the mana beam between them, and the victim star's wisps
//! and sparks. (The slow's Shadow ring is in `slow_ring_visual_probes.rs`.)
//!
//! Two halves:
//!
//! - **The bench.** Every piece pinned against the signed-off bench (AS-335
//!   Priest Talent Visuals Bench, version 1792207490-53c9, "Mind Flay", every
//!   layer kept and every multiplier at 1.0): sizes, colours, alphas, the
//!   flicker, the beam's shape and texture, the emitters. The expected numbers
//!   are the bench's own, worked here by hand, never read back from the module.
//! - **The world.** The visual as it is drawn, run through the real systems on
//!   `MinimalPlugins` + `AssetPlugin` (no window, no GPU): what each piece is
//!   drawn with (mesh, material, texture, blend), where it sits, that it faces
//!   a turned and rolled camera, how it reads through the default match
//!   camera, and that it lives exactly as long as the channel — gone on an
//!   interrupt, the channel's end, either unit's death or a new target.

use std::f32::consts::{FRAC_PI_2, PI};
use std::time::Duration;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{
    ChannelingState, ClientParticle, Combatant, Pet, PetType,
};
use arenasim::states::play_match::{
    animate_client_particles, animate_mind_flay_channels, beam_flip, beam_joints, beam_segments,
    billboard_mind_flay_particles, held_before_body, mana_funnel_flicker, sync_mind_flay_channels,
    BeamTexture, ChannelBeam, ChannelVisual, EmitterSprite, ManaFunnelChannel, ManaFunnelStar,
    ManaFunnelStarQuad, StarEnd, StarQuad, MANA_BEAM, MANA_FUNNEL_FLICKER, MIND_FLAY_SPARK_EMITTER,
    MIND_FLAY_WISP_EMITTER,
};
use arenasim::CharacterClass;
use bevy::prelude::*;
use bevy::render::mesh::VertexAttributeValues;
use bevy::time::TimeUpdateStrategy;

const TICK: Duration = Duration::from_millis(50);
const TICK_SECS: f32 = 0.05;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn near(a: Vec3, b: Vec3) -> bool {
    a.distance(b) < 1e-4
}

// =============================================================================
// The bench
// =============================================================================

/// Each star: a 2.38 yd quad in (49, 180, 255) at 0.45 under a 1.1 yd white
/// one at 0.55 (`bbGlow(p, 2.38 s fl, ..., [49,180,255], 0.45 op)` and
/// `bbGlow(p, 1.1 s fl, ..., [255,255,255], 0.55 op)`).
#[test]
fn the_stars_are_the_benchs() {
    assert_eq!(StarQuad::ALL, [StarQuad::Outer, StarQuad::Inner]);
    assert!(close(StarQuad::Outer.size(), 2.38));
    assert!(close(StarQuad::Inner.size(), 1.1));
    assert_eq!(StarQuad::Outer.rgb(), [49, 180, 255]);
    assert_eq!(StarQuad::Inner.rgb(), [255, 255, 255]);
    assert!(close(StarQuad::Outer.alpha(), 0.45));
    assert!(close(StarQuad::Inner.alpha(), 0.55));
    assert_eq!(StarEnd::ALL, [StarEnd::Hand, StarEnd::Chest]);
}

/// The flicker: `[0.99, 1.1, 1.25, 1.05, 1.2, 0.99, 1.15, 1.02][floor(ms /
/// 33.4) % 8]`, a 267 ms loop, read in the middle of each step.
#[test]
fn the_flicker_is_the_benchs() {
    let table = [0.99, 1.1, 1.25, 1.05, 1.2, 0.99, 1.15, 1.02];
    assert_eq!(MANA_FUNNEL_FLICKER, table);
    for step in 0..24 {
        let age = (step as f32 + 0.5) * 0.0334;
        assert!(
            close(mana_funnel_flicker(age), table[step % 8]),
            "step {step}: {}",
            mana_funnel_flicker(age)
        );
    }
    // The loop is 267 ms: 0.27 s is a step into the second lap.
    assert!(close(mana_funnel_flicker(0.27), 0.99));
    assert!(close(mana_funnel_flicker(0.31), 1.1));
}

/// The beam: 0.25 yd wide, 2.78 yd segments, re-seeded every 66 ms, its inner
/// joints straying 6 px at the bench's framing (11 yd off, `fl` = 800 × 1.25).
#[test]
fn the_beam_is_the_benchs() {
    assert_eq!(
        ChannelVisual::for_channel(AbilityType::MindFlay).map(ChannelVisual::beam),
        Some(MANA_BEAM)
    );
    assert_eq!(MANA_BEAM.texture, BeamTexture::ManaBeam);
    assert!(close(MANA_BEAM.width, 0.25));
    assert!(close(MANA_BEAM.segment_len, 2.78));
    assert!(close(MANA_BEAM.flip_secs, 0.066));
    assert!(close(MANA_BEAM.jitter, 0.066));

    assert_eq!(beam_flip(&MANA_BEAM, 0.0), 0);
    assert_eq!(beam_flip(&MANA_BEAM, 0.065), 0);
    assert_eq!(beam_flip(&MANA_BEAM, 0.067), 1);
    assert_eq!(beam_flip(&MANA_BEAM, 1.0), 15);
    assert_eq!(beam_segments(&MANA_BEAM, 20.0), 7);
    assert_eq!(beam_segments(&MANA_BEAM, 2.0), 1);
    assert_eq!(beam_segments(&MANA_BEAM, 0.3), 1);

    // The joints: the ends exact; joint i strays sin(seed 1.7 + i 2.3) along
    // the camera's right and cos(seed 1.3 + i) down its up, 0.066 yd each.
    let (from, to) = (Vec3::new(-5.0, 1.0, 0.0), Vec3::new(9.0, 2.0, 3.0));
    let (right, up) = (Vec3::new(0.6, 0.0, -0.8), Vec3::new(0.0, 1.0, 0.0));
    let seed = 7;
    let joints = beam_joints(&MANA_BEAM, from, to, seed, right, up);
    let n = (from.distance(to) / 2.78).round() as usize;
    assert_eq!(joints.len(), n + 1);
    assert!(near(joints[0], from) && near(joints[n], to));
    for (i, joint) in joints.iter().enumerate().take(n).skip(1) {
        let s = seed as f32;
        let i_f = i as f32;
        let want = from.lerp(to, i_f / n as f32) + right * (s * 1.7 + i_f * 2.3).sin() * 0.066
            - up * (s * 1.3 + i_f).cos() * 0.066;
        assert!(near(*joint, want), "joint {i}: {joint} vs {want}");
    }
    // A new seed moves them.
    let other = beam_joints(&MANA_BEAM, from, to, seed + 1, right, up);
    assert!(joints[1].distance(other[1]) > 1e-3);
}

/// The beam's texture adds the bench's two strokes: a 0.25 yd stroke in
/// (150, 210, 255) at 0.35 and, inside 0.1 yd of it, a (240, 250, 255) core
/// at 0.9 — which saturates every channel, so the core is opaque white.
#[test]
fn the_beam_texture_adds_the_benchs_two_strokes() {
    assert_eq!(BeamTexture::ManaBeam.texel(0.0), ([255, 255, 255], 1.0));
    assert_eq!(BeamTexture::ManaBeam.texel(0.39), ([255, 255, 255], 1.0));
    let (rgb, a) = BeamTexture::ManaBeam.texel(0.7);
    assert_eq!(rgb, [150, 210, 255]);
    assert!(close(a, 0.35));
    assert_eq!(BeamTexture::ManaBeam.texel(-0.7), ([150, 210, 255], a));
    assert_eq!(BeamTexture::ManaBeam.texel(1.2).1, 0.0);

    let image = BeamTexture::ManaBeam.image();
    let (w, h) = (image.width(), image.height());
    assert_eq!((w, h), (4, 64));
    let data = image.data.as_ref().unwrap();
    let px = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [data[i], data[i + 1], data[i + 2], data[i + 3]]
    };
    // Row 32 sits at the centre line; rows 0 and 54 in the outer stroke
    // (0.70 and 0.98 of the half-width out). Nothing varies along the beam.
    for x in 0..w {
        assert_eq!(px(x, 32), [255, 255, 255, 255]);
        assert_eq!(px(x, 54), [150, 210, 255, 89]);
        assert_eq!(px(x, 0), [150, 210, 255, 89]);
        assert_eq!(px(x, 63), [150, 210, 255, 89]);
    }
}

/// P1, the wisps: 30/s, sphere, 2.2 yd/s, 0.5 s, black, alpha-blended, radii
/// 0.01 -> 0.14 -> 0.03. P0, the sparks: 20/s, sphere, 1.67 yd/s, 0.7 s,
/// (45, 20, 139) -> (0, 42, 255) -> black, radii 0.08 -> 0.06 -> 0.003, star,
/// additive. Both fade 1 -> 0 and run for the whole channel.
#[test]
fn the_emitters_are_the_benchs() {
    let w = &MIND_FLAY_WISP_EMITTER;
    assert_eq!(w.sprite, EmitterSprite::Glow);
    assert!(!w.additive, "the wisps darken");
    assert_eq!(
        (w.rate, w.life, w.speed, w.gravity, w.area),
        (30.0, 0.5, 2.2, 0.0, 0.0)
    );
    assert_eq!(w.color, [[0, 0, 0]; 3]);
    let s = &MIND_FLAY_SPARK_EMITTER;
    assert_eq!(s.sprite, EmitterSprite::Flare);
    assert!(s.additive);
    assert_eq!(
        (s.rate, s.life, s.speed, s.gravity, s.area),
        (20.0, 0.7, 1.67, 0.0, 0.0)
    );
    assert_eq!(s.color, [[45, 20, 139], [0, 42, 255], [0, 0, 0]]);
    for (e, radii) in [(w, [0.01, 0.14, 0.03]), (s, [0.08, 0.06, 0.003])] {
        assert_eq!(e.window, None, "{} runs for the whole channel", e.name);
        assert!(!e.rise && e.column.is_none() && e.track.is_none());
        assert_eq!(e.mid, 0.5);
        for (k, want) in [1.0, 0.5, 0.0].into_iter().enumerate() {
            assert!(close(e.alpha[k], want), "{} alpha {k}", e.name);
        }
        let d = e.size.diameter_keys();
        for k in 0..3 {
            assert!(close(d[k], 2.0 * radii[k]), "{} size {k}", e.name);
        }
    }
}

// =============================================================================
// The world
// =============================================================================

struct Harness {
    app: App,
}

/// The default match camera: 60 yd out, 38.7 degrees off vertical, FOV 45,
/// looking at `at`.
fn match_camera(at: Vec3) -> Transform {
    let pitch = 38.7f32.to_radians();
    Transform::from_translation(at + Vec3::new(0.0, 60.0 * pitch.cos(), 60.0 * pitch.sin()))
        .looking_at(at, Vec3::Y)
}

impl Harness {
    fn new() -> Self {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()));
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.init_asset::<Image>();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
        // The order `states/mod.rs` runs them in.
        app.add_systems(
            Update,
            (
                sync_mind_flay_channels,
                animate_mind_flay_channels,
                animate_client_particles,
                billboard_mind_flay_particles,
            )
                .chain(),
        );
        Harness { app }
    }

    fn camera(&mut self, cam: Transform) -> Transform {
        self.app.world_mut().spawn((Camera3d::default(), cam));
        cam
    }

    fn spawn(&mut self, class: CharacterClass, team: u8, at: Vec3, facing: Quat) -> Entity {
        self.app
            .world_mut()
            .spawn((
                Transform::from_translation(at).with_rotation(facing),
                Combatant::new(team, 0, class),
            ))
            .id()
    }

    /// A Priest at (-10, 1, 0) facing +X, and a Warrior at (5, 1, 0).
    fn pair(&mut self) -> (Entity, Entity) {
        let priest = self.spawn(
            CharacterClass::Priest,
            1,
            Vec3::new(-10.0, 1.0, 0.0),
            Quat::from_rotation_y(FRAC_PI_2),
        );
        let victim = self.spawn(
            CharacterClass::Warrior,
            2,
            Vec3::new(5.0, 1.0, 0.0),
            Quat::IDENTITY,
        );
        (priest, victim)
    }

    fn channel(&mut self, caster: Entity, ability: AbilityType, target: Entity) {
        let defs = AbilityDefinitions::default();
        let def = defs
            .get(&ability)
            .expect("the ability is configured")
            .clone();
        self.app
            .world_mut()
            .entity_mut(caster)
            .insert(ChannelingState::new(ability, target, &def));
    }

    fn tick(&mut self, n: usize) {
        for _ in 0..n {
            self.app.update();
        }
    }

    fn visuals(&mut self) -> Vec<(Entity, Entity, Entity)> {
        let mut v: Vec<_> = self
            .app
            .world_mut()
            .query::<(Entity, &ManaFunnelChannel)>()
            .iter(self.app.world())
            .map(|(e, c)| (e, c.caster, c.target))
            .collect();
        v.sort();
        v
    }

    fn count<T: Component>(&mut self) -> usize {
        self.app
            .world_mut()
            .query::<&T>()
            .iter(self.app.world())
            .count()
    }

    fn children(&self, e: Entity) -> Vec<Entity> {
        self.app
            .world()
            .get::<Children>(e)
            .map(|c| c.iter().collect())
            .unwrap_or_default()
    }

    /// The visual's one star at `end`.
    fn star(&self, root: Entity, end: StarEnd) -> Entity {
        let stars: Vec<Entity> = self
            .children(root)
            .into_iter()
            .filter(|c| {
                self.app
                    .world()
                    .get::<ManaFunnelStar>(*c)
                    .is_some_and(|s| s.end == end)
            })
            .collect();
        assert_eq!(stars.len(), 1, "one {end:?} star");
        stars[0]
    }

    fn beam(&self, root: Entity) -> Entity {
        let beams: Vec<Entity> = self
            .children(root)
            .into_iter()
            .filter(|c| self.app.world().get::<ChannelBeam>(*c).is_some())
            .collect();
        assert_eq!(beams.len(), 1, "one beam");
        beams[0]
    }

    fn transform(&self, e: Entity) -> Transform {
        *self.app.world().get::<Transform>(e).unwrap()
    }

    fn material(&self, e: Entity) -> StandardMaterial {
        let handle = &self
            .app
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(e)
            .unwrap()
            .0;
        self.app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(handle)
            .unwrap()
            .clone()
    }

    fn image(&self, handle: &Handle<Image>) -> Image {
        self.app
            .world()
            .resource::<Assets<Image>>()
            .get(handle)
            .unwrap()
            .clone()
    }

    fn positions(&self, e: Entity) -> Vec<Vec3> {
        let handle = &self.app.world().get::<Mesh3d>(e).unwrap().0;
        let mesh = self
            .app
            .world()
            .resource::<Assets<Mesh>>()
            .get(handle)
            .unwrap();
        match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(VertexAttributeValues::Float32x3(p)) => p.iter().map(|v| Vec3::from(*v)).collect(),
            _ => panic!("the mesh has no positions"),
        }
    }

    fn age(&self, root: Entity) -> f32 {
        self.app.world().get::<ManaFunnelChannel>(root).unwrap().age
    }

    /// Every live particle on `star`: its velocity, its transform, its
    /// material.
    fn particles(&self, star: Entity) -> Vec<(Vec3, Transform, StandardMaterial)> {
        self.children(star)
            .into_iter()
            .filter_map(|c| {
                let p = self.app.world().get::<ClientParticle>(c)?;
                (p.age < p.life).then(|| (p.velocity, self.transform(c), self.material(c)))
            })
            .collect()
    }

    fn kill(&mut self, e: Entity) {
        self.app
            .world_mut()
            .get_mut::<Combatant>(e)
            .unwrap()
            .current_health = 0.0;
    }

    fn move_to(&mut self, e: Entity, at: Vec3) {
        self.app
            .world_mut()
            .get_mut::<Transform>(e)
            .unwrap()
            .translation = at;
    }
}

/// The Priest's main spell hand, for a Priest at (-10, 1, 0) turned to face
/// +X: the socket (0.72, 0.05, 0.30) turned a quarter about Y is
/// (0.30, 0.05, -0.72).
const HAND: Vec3 = Vec3::new(-9.7, 1.05, -0.72);
/// The Warrior's chest: 0.55 above its transform.
const CHEST: Vec3 = Vec3::new(5.0, 1.55, 0.0);
/// The stars' lift toward the camera: the UA glow's, 0.5 + 0.18 + 0.07 yd.
const LIFT: f32 = 0.75;
const PRIEST_AT: Vec3 = Vec3::new(-10.0, 1.0, 0.0);
const VICTIM_AT: Vec3 = Vec3::new(5.0, 1.0, 0.0);

/// Where a star on `at`, on a body standing at `body`, is drawn: on the
/// anchor's view ray, no farther than 0.75 yd in front of the body's axis.
fn lifted(at: Vec3, body: Vec3, eye: Vec3) -> Vec3 {
    let axis = Vec3::new(body.x, at.y, body.z);
    let depth = eye.distance(at).min(eye.distance(axis) - LIFT);
    eye + (at - eye).normalize() * depth
}

fn rgb_of(c: Color) -> [f32; 3] {
    let s = c.to_srgba();
    [s.red, s.green, s.blue]
}

fn same_rgb(c: Color, rgb: [u8; 3]) -> bool {
    let got = rgb_of(c);
    (0..3).all(|k| (got[k] - rgb[k] as f32 / 255.0).abs() < 1e-3)
}

/// A Mind Flay raises one visual: two stars (hand and chest) of two quads
/// each, and one beam. Every piece is drawn as the bench draws it: the quads
/// are unit squares under the soft-glow sprite, additive, in their colour and
/// alpha with an emissive glow of the same hue; the beam is white, additive,
/// under the mana-beam texture.
#[test]
fn a_mind_flay_draws_two_stars_and_a_beam_as_the_bench_does() {
    let mut h = Harness::new();
    h.camera(Transform::from_xyz(0.0, 20.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y));
    let (priest, victim) = h.pair();
    h.channel(priest, AbilityType::MindFlay, victim);
    h.tick(1);

    let visuals = h.visuals();
    assert_eq!(visuals.len(), 1);
    let (root, caster, target) = visuals[0];
    assert_eq!((caster, target), (priest, victim));

    for end in StarEnd::ALL {
        let star = h.star(root, end);
        let mut quads: Vec<StarQuad> = Vec::new();
        for piece in h.children(star) {
            let Some(q) = h.app.world().get::<ManaFunnelStarQuad>(piece) else {
                continue;
            };
            let quad = q.quad;
            quads.push(quad);
            // The mesh: a unit square, scaled to size.
            let positions = h.positions(piece);
            assert_eq!(positions.len(), 4, "{end:?} {quad:?} is one quad");
            for p in &positions {
                assert!(close(p.x.abs(), 0.5) && close(p.y.abs(), 0.5) && close(p.z, 0.0));
            }
            let m = h.material(piece);
            let (rgb, alpha) = match quad {
                StarQuad::Outer => ([49, 180, 255], 0.45),
                StarQuad::Inner => ([255, 255, 255], 0.55),
            };
            assert!(
                same_rgb(m.base_color, rgb),
                "{quad:?} colour {:?}",
                m.base_color
            );
            assert!(close(m.base_color.alpha(), alpha), "{quad:?} alpha");
            assert_eq!(m.alpha_mode, AlphaMode::Add, "{quad:?} adds light");
            let lin = Color::srgb_u8(rgb[0], rgb[1], rgb[2]).to_linear();
            assert!(close(m.emissive.red, lin.red * 2.5));
            assert!(close(m.emissive.green, lin.green * 2.5));
            assert!(close(m.emissive.blue, lin.blue * 2.5));
            assert!(!m.unlit, "unlit would drop the emissive");
            let sprite = m.base_color_texture.clone().expect("the glow sprite");
            assert_eq!(m.emissive_texture, Some(sprite.clone()));
            // The bench's gradient: 1 at the centre, 0.45 halfway, 0 at the rim.
            let image = h.image(&sprite);
            let (w, data) = (image.width(), image.data.clone().unwrap());
            let alpha_at = |x: u32, y: u32| data[((y * w + x) * 4 + 3) as usize];
            let c = w / 2;
            assert!(alpha_at(c, c) >= 245, "centre {}", alpha_at(c, c));
            let half = alpha_at(c + w / 4, c);
            assert!((105..=125).contains(&half), "halfway {half}");
            assert_eq!(alpha_at(0, c), 0, "rim");
            assert_eq!(alpha_at(0, 0), 0, "corner");
        }
        assert_eq!(quads, StarQuad::ALL.to_vec(), "{end:?}: both quads, once");
    }

    let beam = h.beam(root);
    let m = h.material(beam);
    assert!(same_rgb(m.base_color, [255, 255, 255]), "the beam is white");
    assert!(close(m.base_color.alpha(), 1.0));
    assert_eq!(m.alpha_mode, AlphaMode::Add, "the beam adds light");
    let texture = m.base_color_texture.clone().expect("the beam texture");
    assert_eq!(m.emissive_texture, Some(texture.clone()));
    assert_eq!(
        h.image(&texture).data,
        BeamTexture::ManaBeam.image().data,
        "the mana-beam texture"
    );
}

/// The stars sit on the Priest's main spell hand and on the victim's chest,
/// each held 0.75 yd toward the camera; they follow both units, and so does
/// the beam, whose ends stay on the hand and the chest themselves.
#[test]
fn the_stars_sit_on_the_hand_and_the_chest_and_follow() {
    let mut h = Harness::new();
    let cam = h.camera(Transform::from_xyz(-3.0, 18.0, 25.0).looking_at(Vec3::ZERO, Vec3::Y));
    let (priest, victim) = h.pair();
    h.channel(priest, AbilityType::MindFlay, victim);
    h.tick(2);
    let root = h.visuals()[0].0;
    let eye = cam.translation;
    let hand_star = h.transform(h.star(root, StarEnd::Hand)).translation;
    let chest_star = h.transform(h.star(root, StarEnd::Chest)).translation;
    assert!(near(hand_star, lifted(HAND, PRIEST_AT, eye)), "{hand_star}");
    assert!(
        near(chest_star, lifted(CHEST, VICTIM_AT, eye)),
        "{chest_star}"
    );
    let p = h.positions(h.beam(root));
    let (first, last) = ((p[0] + p[1]) / 2.0, (p[p.len() - 2] + p[p.len() - 1]) / 2.0);
    assert!(near(first, HAND) && near(last, CHEST), "{first} .. {last}");

    // The victim runs; the Priest turns to face -Z.
    h.move_to(victim, Vec3::new(2.0, 1.0, 8.0));
    h.app
        .world_mut()
        .get_mut::<Transform>(priest)
        .unwrap()
        .rotation = Quat::from_rotation_y(PI);
    h.tick(1);
    // (0.72, 0.05, 0.30) turned half about Y: (-0.72, 0.05, -0.30).
    let hand = Vec3::new(-10.72, 1.05, -0.3);
    let chest = Vec3::new(2.0, 1.55, 8.0);
    assert!(near(
        h.transform(h.star(root, StarEnd::Hand)).translation,
        lifted(hand, PRIEST_AT, eye)
    ));
    assert!(near(
        h.transform(h.star(root, StarEnd::Chest)).translation,
        lifted(chest, Vec3::new(2.0, 1.0, 8.0), eye)
    ));
    let p = h.positions(h.beam(root));
    let (first, last) = ((p[0] + p[1]) / 2.0, (p[p.len() - 2] + p[p.len() - 1]) / 2.0);
    assert!(near(first, hand) && near(last, chest), "{first} .. {last}");
}

/// A pet victim's star sits at the pet's chest: its body hangs 0.45 below its
/// transform at 0.55 of a combatant's stature.
#[test]
fn a_pet_victims_star_sits_at_the_pets_chest() {
    let mut h = Harness::new();
    let cam = h.camera(Transform::from_xyz(0.0, 30.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y));
    let (priest, _) = h.pair();
    let pet = h.spawn(
        CharacterClass::Hunter,
        2,
        Vec3::new(3.0, 0.75, 2.0),
        Quat::IDENTITY,
    );
    h.app.world_mut().entity_mut(pet).insert(Pet {
        owner: priest,
        pet_type: PetType::Spider,
    });
    h.channel(priest, AbilityType::MindFlay, pet);
    h.tick(1);
    let root = h.visuals()[0].0;
    let chest = Vec3::new(3.0, 0.75 - 0.45 + 0.55 * 0.55, 2.0);
    let got = h.transform(h.star(root, StarEnd::Chest)).translation;
    assert!(
        near(
            got,
            lifted(chest, Vec3::new(3.0, 0.75, 2.0), cam.translation)
        ),
        "{got}"
    );
}

/// A star is drawn on its anchor's view ray, no farther from the camera than
/// 0.75 yd in front of its body's axis: an anchor behind or beside the axis
/// comes forward to that depth; one already nearer stays put; with no camera,
/// nothing moves.
#[test]
fn a_star_is_held_in_front_of_its_body_on_its_own_ray() {
    let eye = Vec3::new(0.0, 0.0, 20.0);
    let body = Vec3::new(0.0, -1.0, 0.0);
    // Behind the axis (0.7 yd off it, away from the camera): pulled to 19.25.
    let behind = Vec3::new(0.0, 0.0, -0.7);
    assert!(near(
        held_before_body(behind, body, Some(eye)),
        Vec3::new(0.0, 0.0, 0.75)
    ));
    // Beside it, off the view axis: on its own ray, 0.75 yd nearer than the axis.
    let beside = Vec3::new(0.7, 0.0, 0.0);
    let got = held_before_body(beside, body, Some(eye));
    assert!(
        close(got.distance(eye), 20.0 - 0.75),
        "{}",
        got.distance(eye)
    );
    assert!((got - eye).normalize().dot((beside - eye).normalize()) > 0.99999);
    // Already a yard in front: stays.
    let ahead = Vec3::new(0.2, 0.0, 1.0);
    assert!(near(held_before_body(ahead, body, Some(eye)), ahead));
    assert!(near(held_before_body(behind, body, None), behind));
}

/// Wherever the camera stands, rolled or not: every star quad faces it,
/// unrolled against it, at its size times the flicker; the beam is a ribbon
/// 0.25 yd across at every joint, square to the view; every live particle
/// faces it.
#[test]
fn every_piece_faces_the_camera_wherever_it_stands() {
    let rolled_up = Vec3::new(0.4, 1.0, 0.2).normalize();
    for cam in [
        Transform::from_xyz(12.0, 8.0, 0.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
        Transform::from_xyz(-3.0, 16.0, 14.0).looking_at(Vec3::new(0.0, 1.0, 0.0), rolled_up),
        match_camera(Vec3::new(-2.5, 0.0, 0.0)),
    ] {
        let mut h = Harness::new();
        h.camera(cam);
        let (priest, victim) = h.pair();
        h.channel(priest, AbilityType::MindFlay, victim);
        h.tick(13);
        let root = h.visuals()[0].0;
        // The age the pose was taken at: the system ages after posing.
        let age = h.age(root) - TICK_SECS;
        let flicker = mana_funnel_flicker(age);
        let mut faced = 0;
        for end in StarEnd::ALL {
            let star = h.star(root, end);
            assert_eq!(
                h.transform(star).rotation,
                Quat::IDENTITY,
                "the rig is unrotated"
            );
            for piece in h.children(star) {
                let Some(q) = h.app.world().get::<ManaFunnelStarQuad>(piece) else {
                    continue;
                };
                let size = match q.quad {
                    StarQuad::Outer => 2.38,
                    StarQuad::Inner => 1.1,
                };
                let t = h.transform(piece);
                assert!((t.rotation * Vec3::Z).dot(cam.rotation * Vec3::Z) > 0.9999);
                assert!(
                    (t.rotation * Vec3::Y).dot(cam.rotation * Vec3::Y) > 0.9999,
                    "{end:?} {:?} is rolled against the camera",
                    q.quad
                );
                assert!(close(t.scale.x, size * flicker) && close(t.scale.y, size * flicker));
                faced += 1;
            }
        }
        assert_eq!(faced, 4, "four quads checked");

        let p = h.positions(h.beam(root));
        let joints = p.len() / 2;
        assert_eq!(joints, (HAND.distance(CHEST) / 2.78).round() as usize + 1);
        for k in 0..joints {
            let (a, b) = (p[2 * k], p[2 * k + 1]);
            assert!(
                close(a.distance(b), 0.25),
                "joint {k} is {} across",
                a.distance(b)
            );
            let view = (cam.translation - (a + b) / 2.0).normalize();
            assert!(
                (b - a).normalize().dot(view).abs() < 1e-3,
                "joint {k} is not square to the view"
            );
        }

        let particles = h.particles(h.star(root, StarEnd::Chest));
        assert!(particles.len() > 10, "{} live particles", particles.len());
        for (_, t, _) in &particles {
            assert!((t.rotation * Vec3::Z).dot(cam.rotation * Vec3::Z) > 0.999);
            assert!((t.rotation * Vec3::Y).dot(cam.rotation * Vec3::Y) > 0.999);
        }
    }
}

/// The drawn beam zig-zags and re-seeds as the bench's does, at its call site:
/// each inner joint strays the bench's `sin(seed 1.7 + i 2.3)` along the
/// camera's right and `cos(seed 1.3 + i)` down its up, 0.066 yd each, off the
/// hand-to-chest line and in the camera's plane, with `seed` the 66 ms flip
/// the channel is in; the joints hold within a flip and move across one; the
/// ends stay on the hand and the chest.
#[test]
fn the_drawn_beam_zig_zags_and_reseeds_every_flip() {
    let mut h = Harness::new();
    let cam = h.camera(
        Transform::from_xyz(-3.0, 16.0, 14.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
    );
    let (right, up, forward) = (
        cam.rotation * Vec3::X,
        cam.rotation * Vec3::Y,
        cam.rotation * Vec3::NEG_Z,
    );
    let (priest, victim) = h.pair();
    h.channel(priest, AbilityType::MindFlay, victim);
    let n = (HAND.distance(CHEST) / 2.78).round() as usize;
    assert!(n >= 3, "a beam of {n} segments has inner joints");

    let mut frames: Vec<(u32, Vec<Vec3>)> = Vec::new();
    for _ in 0..6 {
        h.tick(1);
        let root = h.visuals()[0].0;
        // The age the pose was taken at (the system ages after posing), and
        // the 66 ms flip that age is in.
        let age = h.age(root) - TICK_SECS;
        let flip = (age / 0.066).floor() as u32;
        let p = h.positions(h.beam(root));
        let centres: Vec<Vec3> = (0..p.len() / 2)
            .map(|k| (p[2 * k] + p[2 * k + 1]) / 2.0)
            .collect();
        assert_eq!(centres.len(), n + 1);
        assert!(
            near(centres[0], HAND) && near(centres[n], CHEST),
            "the ends"
        );
        let seed = flip as f32;
        for (i, c) in centres.iter().enumerate().take(n).skip(1) {
            let i_f = i as f32;
            let line = HAND.lerp(CHEST, i_f / n as f32);
            let along = (seed * 1.7 + i_f * 2.3).sin();
            let down = (seed * 1.3 + i_f).cos();
            let want = line + right * along * 0.066 - up * down * 0.066;
            assert!(near(*c, want), "flip {flip} joint {i}: {c} vs {want}");
            let stray = *c - line;
            assert!(
                close(stray.length(), 0.066 * (along * along + down * down).sqrt()),
                "joint {i} strays {}",
                stray.length()
            );
            assert!(
                stray.dot(forward).abs() < 1e-4,
                "joint {i} leaves the camera's plane"
            );
        }
        frames.push((flip, centres));
    }
    // Within a flip the zig-zag holds; across a flip boundary it moves.
    let (mut held, mut moved) = (0, 0);
    for pair in frames.windows(2) {
        let ((fa, a), (fb, b)) = (&pair[0], &pair[1]);
        let shift = (1..n).map(|i| a[i].distance(b[i])).fold(0.0f32, f32::max);
        if fa == fb {
            assert!(shift < 1e-5, "flip {fa}: joints moved {shift} within it");
            held += 1;
        } else {
            assert!(shift > 0.01, "flip {fa} -> {fb}: joints moved only {shift}");
            moved += 1;
        }
    }
    assert!(
        held >= 1 && moved >= 2,
        "{held} held, {moved} moved: {:?}",
        frames.iter().map(|f| f.0).collect::<Vec<_>>()
    );
}

/// Through the default match camera (60 yd, 38.7 degrees, FOV 45, a 1080-line
/// screen): each star stands in front of its body's near skin and of a
/// stacked Corruption shroud (0.68 yd), yet projects onto its anchor; the
/// beam reads at about 5 px across and the stars at about 50 and 24.
#[test]
fn the_match_camera_sees_the_stars_over_the_bodies_and_the_beam() {
    let mut h = Harness::new();
    let cam = h.camera(match_camera(Vec3::new(-2.5, 0.0, 0.0)));
    let (priest, victim) = h.pair();
    h.channel(priest, AbilityType::MindFlay, victim);
    h.tick(2);
    let root = h.visuals()[0].0;
    let eye = cam.translation;
    let focal = 540.0 / (22.5f32.to_radians()).tan();
    let project = |p: Vec3| {
        let local = cam.rotation.inverse() * (p - eye);
        Vec2::new(local.x, local.y) * (focal / -local.z)
    };

    for (end, anchor, body) in [
        (StarEnd::Hand, HAND, Vec3::new(-10.0, 1.0, 0.0)),
        (StarEnd::Chest, CHEST, Vec3::new(5.0, 1.0, 0.0)),
    ] {
        let star = h.transform(h.star(root, end)).translation;
        // In front of the body's skin and the shroud, nearest the camera.
        let axis = Vec3::new(body.x, anchor.y, body.z);
        let skin = axis.distance(eye) - 0.68;
        assert!(
            star.distance(eye) < skin - 0.05,
            "{end:?} star at {} behind the skin at {skin}",
            star.distance(eye)
        );
        // On the anchor, on screen.
        assert!(
            project(star).distance(project(anchor)) < 0.5,
            "{end:?} moved on screen"
        );
        // The quads' size on screen: 2.38 and 1.1 yd at ~60 yd.
        let px = |yd: f32| yd * focal / star.distance(eye);
        assert!((45.0..60.0).contains(&px(2.38)), "{}", px(2.38));
        assert!((20.0..28.0).contains(&px(1.1)), "{}", px(1.1));
    }

    let p = h.positions(h.beam(root));
    for k in 0..p.len() / 2 {
        let across = project(p[2 * k]).distance(project(p[2 * k + 1]));
        assert!((4.0..7.0).contains(&across), "joint {k}: {across} px");
    }
}

/// The victim's star throws the wisps (black, alpha-blended, 2.2 yd/s) and the
/// sparks (added light, 1.67 yd/s), about 15 and 14 live at a time; the
/// hand's throws nothing. The particles ride the victim's star.
#[test]
fn the_victims_star_throws_the_wisps_and_the_sparks() {
    let mut h = Harness::new();
    h.camera(Transform::from_xyz(0.0, 20.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y));
    let (priest, victim) = h.pair();
    h.channel(priest, AbilityType::MindFlay, victim);
    h.tick(30);
    let root = h.visuals()[0].0;
    assert!(
        h.particles(h.star(root, StarEnd::Hand)).is_empty(),
        "the hand's star throws nothing"
    );
    let chest = h.star(root, StarEnd::Chest);
    let particles = h.particles(chest);
    let wisps: Vec<_> = particles
        .iter()
        .filter(|(v, _, _)| close(v.length(), 2.2))
        .collect();
    let sparks: Vec<_> = particles
        .iter()
        .filter(|(v, _, _)| close(v.length(), 1.67))
        .collect();
    assert_eq!(wisps.len() + sparks.len(), particles.len(), "nothing else");
    assert!((12..=17).contains(&wisps.len()), "{} wisps", wisps.len());
    assert!((11..=16).contains(&sparks.len()), "{} sparks", sparks.len());
    for (_, _, m) in &wisps {
        assert_eq!(m.alpha_mode, AlphaMode::Blend, "a wisp darkens");
        assert!(same_rgb(m.base_color, [0, 0, 0]), "a wisp is black");
        assert!(m.unlit);
    }
    for (_, _, m) in &sparks {
        assert_eq!(m.alpha_mode, AlphaMode::Add, "a spark adds light");
    }
    // Born at the star's centre and flown for their age: within reach of it.
    for (v, t, _) in &particles {
        assert!(t.translation.length() <= v.length() * 0.7 + 1e-3);
    }
    // Spent particles are retired: no more than about a second's worth.
    assert!(h.children(chest).len() < 60, "{}", h.children(chest).len());

    // The victim moves; its star, and every particle on it, moves with it.
    let before = h.transform(chest).translation;
    h.move_to(victim, Vec3::new(5.0, 1.0, 6.0));
    h.tick(1);
    let after = h.transform(chest).translation;
    assert!((after - before).z > 5.5, "{before} -> {after}");
}

/// The visual lives exactly as long as its channel: gone on an interrupt, on
/// the channel's end, on the target's death mid-channel, on the caster's
/// death, on the target's despawn, and swapped on a new target; never raised
/// for another channel or on a corpse. Everything it spawned goes with it.
#[test]
fn the_visual_lives_exactly_as_long_as_the_channel() {
    let mut h = Harness::new();
    h.camera(Transform::from_xyz(0.0, 20.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y));
    let (priest, victim) = h.pair();
    let gone = |h: &mut Harness| {
        assert_eq!(h.visuals(), vec![]);
        assert_eq!(h.count::<ManaFunnelStar>(), 0, "stars left");
        assert_eq!(h.count::<ManaFunnelStarQuad>(), 0, "quads left");
        assert_eq!(h.count::<ChannelBeam>(), 0, "beam left");
        assert_eq!(h.count::<ClientParticle>(), 0, "particles left");
    };

    // Drain Life draws its own beam, not this.
    h.channel(priest, AbilityType::DrainLife, victim);
    h.tick(2);
    gone(&mut h);

    // An interrupt.
    h.channel(priest, AbilityType::MindFlay, victim);
    h.tick(10);
    assert_eq!(h.visuals().len(), 1);
    h.app
        .world_mut()
        .get_mut::<ChannelingState>(priest)
        .unwrap()
        .interrupted = true;
    h.tick(1);
    gone(&mut h);

    // The channel's end.
    h.app
        .world_mut()
        .entity_mut(priest)
        .remove::<ChannelingState>();
    h.channel(priest, AbilityType::MindFlay, victim);
    h.tick(10);
    assert_eq!(h.visuals().len(), 1);
    h.app
        .world_mut()
        .entity_mut(priest)
        .remove::<ChannelingState>();
    h.tick(1);
    gone(&mut h);

    // A new target mid-channel: the old visual goes, one on the new target.
    let other = h.spawn(
        CharacterClass::Mage,
        2,
        Vec3::new(0.0, 1.0, 9.0),
        Quat::IDENTITY,
    );
    h.channel(priest, AbilityType::MindFlay, victim);
    h.tick(3);
    h.channel(priest, AbilityType::MindFlay, other);
    h.tick(1);
    let v = h.visuals();
    assert_eq!(
        v.iter().map(|x| (x.1, x.2)).collect::<Vec<_>>(),
        vec![(priest, other)]
    );
    assert_eq!(h.count::<ChannelBeam>(), 1);

    // The target dies mid-channel (the channel itself is still on the Priest).
    h.kill(other);
    h.tick(1);
    gone(&mut h);

    // A channel on a corpse draws nothing.
    h.channel(priest, AbilityType::MindFlay, other);
    h.tick(2);
    gone(&mut h);

    // The caster dies.
    h.channel(priest, AbilityType::MindFlay, victim);
    h.tick(3);
    assert_eq!(h.visuals().len(), 1);
    h.kill(priest);
    h.tick(1);
    gone(&mut h);

    // The target is despawned.
    let priest2 = h.spawn(
        CharacterClass::Priest,
        1,
        Vec3::new(-6.0, 1.0, 4.0),
        Quat::IDENTITY,
    );
    h.channel(priest2, AbilityType::MindFlay, victim);
    h.tick(3);
    assert_eq!(h.visuals().len(), 1);
    h.app.world_mut().entity_mut(victim).despawn();
    h.tick(1);
    gone(&mut h);
}

/// Two Priests flaying one victim draw two visuals, one each.
#[test]
fn two_priests_draw_two_channels() {
    let mut h = Harness::new();
    h.camera(Transform::from_xyz(0.0, 20.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y));
    let (priest, victim) = h.pair();
    let second = h.spawn(
        CharacterClass::Priest,
        1,
        Vec3::new(-4.0, 1.0, 7.0),
        Quat::IDENTITY,
    );
    h.channel(priest, AbilityType::MindFlay, victim);
    h.channel(second, AbilityType::MindFlay, victim);
    h.tick(4);
    let mut pairs: Vec<_> = h.visuals().into_iter().map(|v| (v.1, v.2)).collect();
    pairs.sort();
    let mut want = vec![(priest, victim), (second, victim)];
    want.sort();
    assert_eq!(pairs, want);
    assert_eq!(h.count::<ChannelBeam>(), 2);
    assert_eq!(h.count::<ManaFunnelStar>(), 4);
}
