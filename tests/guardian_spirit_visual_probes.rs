//! Probes for Guardian Spirit's state and the shared "Saved" cue
//! (`rendering/effects/guardian_spirit.rs`, AS-354).
//!
//! Two halves:
//!
//! - **The bench.** Every panel, the sparks and the cue pinned against the
//!   signed-off bench (AS-335 Priest Talent Visuals Bench, version
//!   1792207490-53c9, "Guardian Spirit", every layer kept and every multiplier
//!   at 1.0): colours, sprites, sizes, offsets, alphas, rates. The expected
//!   numbers are the bench's own formulas, worked here by hand, never read
//!   back from the module.
//! - **The world.** The spirit as it is drawn, run through the real systems on
//!   `MinimalPlugins` + `AssetPlugin` (no window, no GPU): raised with the
//!   aura and gone with it, sitting behind its bearer's shoulders and
//!   following it, every panel and spark facing the camera wherever it
//!   stands; and a prevented killing blow playing the heal landing and the
//!   "Saved" cue.

use std::time::Duration;

use arenasim::states::play_match::combat_core::KillingBlowPrevented;
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, CameraController, ClientParticle, Combatant, CompoundDebuff,
    FctScreenOffset, FloatingCombatText, FloatingTextState, HealImpact, HealImpactKind,
    PlayMatchEntity,
};
use arenasim::states::play_match::{
    animate_client_particles, animate_guardian_spirits, animate_saved_flashes,
    billboard_guardian_spirit_sparks, consume_killing_blow_signals, fct_placements,
    saved_flash_alpha, spawn_saved_flashes, spirit_panel_pose, update_guardian_spirit_states,
    EmitterSprite, GuardianSpiritPanel, GuardianSpiritRig, PanelPose, PanelSprite, SavedFlash,
    SpiritPanel, FCT_HEIGHT, GUARDIAN_SPIRIT_SPARK_EMITTER,
};
use arenasim::ui::theme;
use arenasim::CharacterClass;
use bevy::prelude::*;
use bevy::render::camera::CameraProjection;
use bevy::render::mesh::VertexAttributeValues;
use bevy::time::TimeUpdateStrategy;
use bevy_egui::egui;

const TICK: Duration = Duration::from_millis(50);
const TICK_SECS: f32 = 0.05;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

/// Every panel as the bench draws it: its sRGB, whether it adds light (the
/// dark circle is the one source-over layer) and its sprite.
const BENCH_LOOK: &[(SpiritPanel, [u8; 3], bool, PanelSprite)] = &[
    // The dark circle: a solid source-over disc, `rgba(60,55,40,0.25)`.
    (
        SpiritPanel::DarkCircle,
        [60, 55, 40],
        false,
        PanelSprite::Disc,
    ),
    // `ring3(..., [253,221,8], 0.9, 3)`: a level ring.
    (
        SpiritPanel::HaloRing,
        [253, 221, 8],
        true,
        PanelSprite::LevelRing,
    ),
    (
        SpiritPanel::RingGlow,
        [255, 255, 127],
        true,
        PanelSprite::Glow,
    ),
    (
        SpiritPanel::WingLeft,
        [255, 200, 70],
        true,
        PanelSprite::Glow,
    ),
    (
        SpiritPanel::WingRight,
        [255, 200, 70],
        true,
        PanelSprite::Glow,
    ),
    (SpiritPanel::Band, [249, 240, 170], true, PanelSprite::Glow),
    (
        SpiritPanel::FlareLeft,
        [255, 211, 107],
        true,
        PanelSprite::Glow,
    ),
    (
        SpiritPanel::FlareRight,
        [255, 211, 107],
        true,
        PanelSprite::Glow,
    ),
];

fn bench_look(panel: SpiritPanel) -> ([u8; 3], bool, PanelSprite) {
    let (_, rgb, additive, sprite) = BENCH_LOOK
        .iter()
        .find(|l| l.0 == panel)
        .expect("every panel is in the table");
    (*rgb, *additive, *sprite)
}

/// The bench's `bbGlow` gradient, worked independently of the module: 1 at
/// the centre, 0.45 halfway out, 0 at the rim, linear between.
fn bench_glow(r: f32) -> f32 {
    if r <= 0.5 {
        1.0 - 1.1 * r
    } else if r < 1.0 {
        0.9 * (1.0 - r)
    } else {
        0.0
    }
}

/// A drawn colour is the bench's sRGB.
fn assert_rgb(color: Color, rgb: [u8; 3], what: &str) {
    let c = color.to_srgba();
    for (got, want) in [c.red, c.green, c.blue].into_iter().zip(rgb) {
        assert!(
            (got * 255.0 - want as f32).abs() < 0.5,
            "{what}: {c:?} is not {rgb:?}"
        );
    }
}

/// An additive material: added light in the bench's colour, its emissive the
/// client emitters' 2.5x glow of it.
fn assert_additive(m: &StandardMaterial, rgb: [u8; 3], what: &str) {
    assert_eq!(m.alpha_mode, AlphaMode::Add, "{what} adds light");
    assert_rgb(m.base_color, rgb, what);
    let c = Color::srgb_u8(rgb[0], rgb[1], rgb[2]).to_linear();
    let e = m.emissive;
    for (got, want) in [(e.red, c.red), (e.green, c.green), (e.blue, c.blue)] {
        assert!((got - 2.5 * want).abs() < 1e-4, "{what} emissive {e:?}");
    }
}

/// The image is the bench's glow: white, every pixel's alpha within one step
/// of the gradient at its distance from the centre.
fn assert_glow_sprite(world: &World, handle: &Handle<Image>, what: &str) {
    let image = world
        .resource::<Assets<Image>>()
        .get(handle)
        .unwrap_or_else(|| panic!("{what}: no sprite"));
    let (w, h) = (image.width(), image.height());
    assert_eq!(w, h, "{what}: a square sprite");
    let data = image.data.as_ref().expect("the sprite has pixels");
    let centre = (w as f32 - 1.0) / 2.0;
    for y in 0..h {
        for x in 0..w {
            let r = ((x as f32 - centre).hypot(y as f32 - centre)) / centre;
            let i = ((y * w + x) * 4) as usize;
            assert_eq!(&data[i..i + 3], &[255, 255, 255], "{what}: white");
            let want = (bench_glow(r) * 255.0).round();
            assert!(
                (data[i + 3] as f32 - want).abs() <= 1.0,
                "{what}: alpha {} at r {r} is not the bench's {want}",
                data[i + 3]
            );
        }
    }
}

fn mesh_positions(world: &World, handle: &Handle<Mesh>) -> Vec<Vec3> {
    let mesh = world
        .resource::<Assets<Mesh>>()
        .get(handle)
        .expect("a mesh");
    match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(v)) => v.iter().map(|p| Vec3::from(*p)).collect(),
        other => panic!("unexpected positions {other:?}"),
    }
}

/// A unit quad in its own XY plane: four corners at ±0.5.
fn assert_unit_quad(world: &World, handle: &Handle<Mesh>, what: &str) {
    let p = mesh_positions(world, handle);
    assert_eq!(p.len(), 4, "{what}: a quad");
    for v in p {
        assert!(
            close(v.x.abs(), 0.5) && close(v.y.abs(), 0.5) && close(v.z, 0.0),
            "{what}: {v}"
        );
    }
}

/// The radii a flat mesh in its XY plane spans: the nearest and farthest
/// vertex from its centre, and how many vertices it has.
fn radii(world: &World, handle: &Handle<Mesh>) -> (f32, f32, usize) {
    let p = mesh_positions(world, handle);
    assert!(p.iter().all(|v| close(v.z, 0.0)), "a flat mesh");
    let r = |v: &Vec3| v.truncate().length();
    let lo = p.iter().map(r).fold(f32::MAX, f32::min);
    let hi = p.iter().map(r).fold(0.0, f32::max);
    (lo, hi, p.len())
}

// =============================================================================
// The bench
// =============================================================================

/// The bench's loop phase, `lp = (ms % 466) / 466`.
fn lp(age: f32) -> f32 {
    ((age * 1000.0) % 466.0) / 466.0
}

fn assert_pose(panel: SpiritPanel, age: f32, want: PanelPose) {
    let got = spirit_panel_pose(panel, age)
        .unwrap_or_else(|| panic!("{panel:?} draws nothing at {age}s"));
    for (name, g, w) in [
        ("right", got.right, want.right),
        ("up", got.up, want.up),
        ("width", got.width, want.width),
        ("height", got.height, want.height),
        ("alpha", got.alpha, want.alpha),
    ] {
        assert!(
            close(g, w),
            "{panel:?} at {age}s: {name} {g} vs the bench's {w}"
        );
    }
}

fn pose(right: f32, up: f32, width: f32, height: f32, alpha: f32) -> PanelPose {
    PanelPose {
        right,
        up,
        width,
        height,
        alpha,
    }
}

/// Every panel's colour and sprite, as the bench draws it.
#[test]
fn the_panels_wear_the_benchs_colours_and_sprites() {
    use SpiritPanel::*;
    let expected: &[(SpiritPanel, [u8; 3], PanelSprite, bool)] = &[
        // The dark circle: a solid source-over disc, `rgba(60,55,40,0.25)`.
        (DarkCircle, [60, 55, 40], PanelSprite::Disc, false),
        // `ring3(..., [253,221,8], 0.9, 3)`: a level ring.
        (HaloRing, [253, 221, 8], PanelSprite::LevelRing, true),
        (RingGlow, [255, 255, 127], PanelSprite::Glow, true),
        (WingLeft, [255, 200, 70], PanelSprite::Glow, true),
        (WingRight, [255, 200, 70], PanelSprite::Glow, true),
        (Band, [249, 240, 170], PanelSprite::Glow, true),
        (FlareLeft, [255, 211, 107], PanelSprite::Glow, true),
        (FlareRight, [255, 211, 107], PanelSprite::Glow, true),
    ];
    assert_eq!(
        SpiritPanel::ALL.to_vec(),
        expected.iter().map(|e| e.0).collect::<Vec<_>>(),
        "every panel is pinned"
    );
    for &(panel, rgb, sprite, additive) in expected {
        assert_eq!(panel.rgb(), rgb, "{panel:?} colour");
        assert_eq!(panel.sprite(), sprite, "{panel:?} sprite");
        assert_eq!(panel.additive(), additive, "{panel:?} blend");
    }
}

/// The halo: the dark circle (radius 0.8, alpha 0.25), the gold ring (radius
/// 0.35, 0.15 up, alpha 0.9, 3 px at the bench's 7 yd framing) and the ring
/// glow, which sweeps 0.28 -> 1.04 yd through 533-1100 ms of the onset and
/// once a loop after, at 0.6 alpha in the onset and 0.35 after.
#[test]
fn the_halo_is_the_benchs() {
    for age in [0.0, 0.7, 4.0, 9.9] {
        assert_pose(SpiritPanel::DarkCircle, age, pose(0.0, 0.0, 1.6, 1.6, 0.25));
        assert_pose(
            SpiritPanel::HaloRing,
            age,
            pose(0.0, 0.15, 0.7, 3.0 / (1000.0 / 7.0), 0.9),
        );
    }
    // Onset, before the flash window: the loop's ring.
    let rr = 0.28 + 0.76 * lp(0.2);
    assert_pose(SpiritPanel::RingGlow, 0.2, pose(0.0, 0.0, rr, rr, 0.6));
    // Halfway through the window: halfway from 0.28 to 1.04.
    let mid = (533.0 + 1100.0) / 2000.0;
    assert_pose(SpiritPanel::RingGlow, mid, pose(0.0, 0.0, 0.66, 0.66, 0.6));
    // The loop.
    let rr = 0.28 + 0.76 * lp(2.0);
    assert_pose(SpiritPanel::RingGlow, 2.0, pose(0.0, 0.0, rr, rr, 0.35));
}

/// The wings: each lobe `w × 0.32` out along the camera's right, 0.05 up,
/// `w × 0.6` by 0.5, at 0.55 alpha, fading in over the first 400 ms; the span
/// `w` grows 2.2 -> 2.8 yd across the onset, then breathes on `sin(ms / 466 π)`.
#[test]
fn the_wings_are_the_benchs() {
    // 200 ms: w = 2.2 + 0.6 × (200 / 1800), half faded in.
    let w = 2.2 + 0.6 * (0.2 / 1.8);
    assert_pose(
        SpiritPanel::WingLeft,
        0.2,
        pose(-w * 0.32, 0.05, w * 0.6, 0.5, 0.55 * 0.5),
    );
    assert_pose(
        SpiritPanel::WingRight,
        0.2,
        pose(w * 0.32, 0.05, w * 0.6, 0.5, 0.55 * 0.5),
    );
    // 900 ms: halfway through the onset, fully in.
    assert_pose(SpiritPanel::WingRight, 0.9, pose(0.8, 0.05, 1.5, 0.5, 0.55));
    // The loop's widest: ms / 466 × π = 4.5 π.
    let widest = 0.466 * 4.5;
    assert_pose(
        SpiritPanel::WingLeft,
        widest,
        pose(-2.8 * 0.32, 0.05, 2.8 * 0.6, 0.5, 0.55),
    );
    // The loop's narrowest: 5.5 π.
    let narrowest = 0.466 * 5.5;
    assert_pose(
        SpiritPanel::WingRight,
        narrowest,
        pose(2.2 * 0.32, 0.05, 2.2 * 0.6, 0.5, 0.55),
    );
}

/// The band: 0.1 below the centre, widening 0.96 -> 3.9 yd each loop, 0.8
/// tall, at 0.45 alpha in the onset and 0.25 after. The long flares: ±0.6 yd
/// out, 1.4 × 0.14, flashing on `sin((ms - 540) / 560 π)` through 540-1100 ms
/// of the onset, and nothing outside it.
#[test]
fn the_band_and_the_flares_are_the_benchs() {
    let bw = 0.96 + (3.9 - 0.96) * lp(0.2);
    assert_pose(SpiritPanel::Band, 0.2, pose(0.0, -0.1, bw, 0.8, 0.45));
    let bw = 0.96 + (3.9 - 0.96) * lp(5.0);
    assert_pose(SpiritPanel::Band, 5.0, pose(0.0, -0.1, bw, 0.8, 0.25));

    // 820 ms is the flash's peak.
    assert_pose(
        SpiritPanel::FlareLeft,
        0.82,
        pose(-0.6, 0.0, 1.4, 0.14, 1.0),
    );
    assert_pose(
        SpiritPanel::FlareRight,
        0.82,
        pose(0.6, 0.0, 1.4, 0.14, 1.0),
    );
    let a = ((680.0f32 - 540.0) / 560.0 * std::f32::consts::PI).sin();
    assert_pose(SpiritPanel::FlareRight, 0.68, pose(0.6, 0.0, 1.4, 0.14, a));
    for age in [0.0, 0.5, 1.15, 1.79, 2.6, 9.0] {
        for panel in [SpiritPanel::FlareLeft, SpiritPanel::FlareRight] {
            assert_eq!(spirit_panel_pose(panel, age), None, "{panel:?} at {age}s");
        }
    }
}

/// The sparks (P0): 100 a second for the whole spirit, in every direction at
/// 2.8 yd/s from a 0.8 × 0.3 × 0.8 yd box, living 0.5 s, white -> gold ->
/// olive gold and fading out, radii 0.06 -> 0.03 (the bench draws a key as a
/// radius), a star glint, added light.
#[test]
fn the_sparks_are_the_benchs() {
    let e = &GUARDIAN_SPIRIT_SPARK_EMITTER;
    assert_eq!(e.sprite, EmitterSprite::Flare);
    assert!(e.additive);
    assert_eq!(e.rate, 100.0);
    assert_eq!(e.window, None, "the sparks run for the whole spirit");
    assert_eq!(e.life, 0.5);
    assert_eq!(e.speed, 2.8);
    assert_eq!(e.gravity, 0.0);
    assert!(!e.rise, "every direction, not a rise");
    assert_eq!(e.column, Some((0.8, 0.3)));
    assert_eq!(e.color, [[255, 255, 255], [235, 210, 0], [183, 183, 0]]);
    assert_eq!(e.mid, 0.5);
    let alpha = e.alpha;
    for (k, want) in [1.0, 0.5, 0.0].into_iter().enumerate() {
        assert!(close(alpha[k], want), "alpha key {k}");
    }
    let diameters = e.size.diameter_keys();
    for (k, radius) in [0.06, 0.045, 0.03].into_iter().enumerate() {
        assert!(close(diameters[k], 2.0 * radius), "size key {k}");
    }
}

/// The cue's flash: up to 0.7 at 80 ms, down to nothing at 0.6 s.
#[test]
fn the_saved_flash_is_the_benchs() {
    for (age, want) in [
        (0.0, 0.0),
        (0.02, 0.175),
        (0.08, 0.7),
        (0.21, 0.7 * (0.6 - 0.21) / 0.52),
        (0.599, 0.7 * 0.001 / 0.52),
        (0.6, 0.0),
        (1.0, 0.0),
    ] {
        assert!(
            close(saved_flash_alpha(age), want),
            "{age}s: {} vs {want}",
            saved_flash_alpha(age)
        );
    }
}

// =============================================================================
// The world
// =============================================================================

/// Guardian Spirit's face, as `apply_pending_auras` lands it.
fn spirit_aura() -> Aura {
    Aura {
        effect_type: AuraType::HealingReceivedIncrease,
        duration: 10.0,
        magnitude: 0.4,
        ability_name: "Guardian Spirit".into(),
        compound: Some(CompoundDebuff::GuardianSpirit),
        ..Default::default()
    }
}

/// The same aura type, not the spirit.
fn other_boost() -> Aura {
    Aura {
        effect_type: AuraType::HealingReceivedIncrease,
        duration: 10.0,
        magnitude: 0.2,
        ability_name: "Some Other Boost".into(),
        ..Default::default()
    }
}

struct Text {
    text: String,
    color: egui::Color32,
    world_position: Vec3,
    is_crit: bool,
    offset: Option<FctScreenOffset>,
}

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
        // The order `states/mod.rs` registers them in.
        app.add_systems(
            Update,
            (
                consume_killing_blow_signals,
                update_guardian_spirit_states,
                animate_guardian_spirits,
                animate_client_particles,
                billboard_guardian_spirit_sparks,
                spawn_saved_flashes,
                animate_saved_flashes,
            )
                .chain(),
        );
        Harness { app }
    }

    fn camera(&mut self, at: Vec3) -> Transform {
        let cam = Transform::from_translation(at).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y);
        self.app.world_mut().spawn((Camera3d::default(), cam));
        cam
    }

    fn spawn_unit(&mut self, at: Vec3) -> Entity {
        self.app
            .world_mut()
            .spawn((
                Transform::from_translation(at),
                Combatant::new(1, 0, CharacterClass::Warrior),
                FloatingTextState {
                    next_pattern_index: 0,
                },
            ))
            .id()
    }

    fn set_auras(&mut self, unit: Entity, auras: Vec<Aura>) {
        self.app
            .world_mut()
            .entity_mut(unit)
            .insert(ActiveAuras { auras });
    }

    fn tick(&mut self, n: usize) {
        for _ in 0..n {
            self.app.update();
        }
    }

    fn rigs_of(&mut self, bearer: Entity) -> Vec<Entity> {
        self.app
            .world_mut()
            .query::<(Entity, &GuardianSpiritRig)>()
            .iter(self.app.world())
            .filter(|(_, r)| r.bearer == bearer)
            .map(|(e, _)| e)
            .collect()
    }

    fn rig_of(&mut self, bearer: Entity) -> Entity {
        let rigs = self.rigs_of(bearer);
        assert_eq!(rigs.len(), 1, "one spirit on the bearer");
        rigs[0]
    }

    fn children(&self, rig: Entity) -> Vec<Entity> {
        self.app
            .world()
            .get::<Children>(rig)
            .map(|c| c.iter().collect())
            .unwrap_or_default()
    }

    fn panels(&self, rig: Entity) -> Vec<(SpiritPanel, Transform, StandardMaterial, Handle<Mesh>)> {
        let world = self.app.world();
        self.children(rig)
            .into_iter()
            .filter_map(|c| {
                let panel = world.get::<GuardianSpiritPanel>(c)?;
                let material = world
                    .resource::<Assets<StandardMaterial>>()
                    .get(&panel.material)?
                    .clone();
                let mesh = world.get::<Mesh3d>(c)?.0.clone();
                Some((panel.panel, *world.get::<Transform>(c)?, material, mesh))
            })
            .collect()
    }

    /// Every live spark: its age, its velocity and its transform.
    fn live_sparks(&self, rig: Entity) -> Vec<(f32, Vec3, Transform)> {
        let world = self.app.world();
        self.children(rig)
            .into_iter()
            .filter_map(|c| {
                let p = world.get::<ClientParticle>(c)?;
                (p.age < p.life).then(|| (p.age, p.velocity, *world.get::<Transform>(c).unwrap()))
            })
            .collect()
    }

    /// Every floating text: its words, colour, position and crit flag.
    fn texts(&mut self) -> Vec<Text> {
        self.app
            .world_mut()
            .query::<(&FloatingCombatText, Option<&FctScreenOffset>)>()
            .iter(self.app.world())
            .map(|(t, offset)| Text {
                text: t.text.clone(),
                color: t.color,
                world_position: t.world_position,
                is_crit: t.is_crit,
                offset: offset.copied(),
            })
            .collect()
    }

    fn count<T: Component>(&mut self) -> usize {
        self.app
            .world_mut()
            .query::<&T>()
            .iter(self.app.world())
            .count()
    }

    fn save(&mut self, holder: Entity, by: &str, heal: Option<f32>) {
        self.app.world_mut().spawn((
            KillingBlowPrevented {
                holder,
                by: by.into(),
                heal,
            },
            PlayMatchEntity,
        ));
    }
}

/// The spirit rises with Guardian Spirit and with nothing else of its type,
/// carries all eight panels, and comes down when the aura ends, when its
/// bearer dies and when its bearer is gone.
#[test]
fn the_spirit_rises_with_the_aura_and_comes_down_with_it() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(Vec3::new(0.0, 1.0, 0.0));
    h.set_auras(unit, vec![other_boost()]);
    h.tick(2);
    assert!(
        h.rigs_of(unit).is_empty(),
        "another healing boost raises none"
    );

    h.set_auras(unit, vec![other_boost(), spirit_aura()]);
    h.tick(1);
    let rig = h.rig_of(unit);
    let mut panels: Vec<SpiritPanel> = h.panels(rig).into_iter().map(|p| p.0).collect();
    panels.sort_by_key(|p| SpiritPanel::ALL.iter().position(|q| q == p));
    assert_eq!(panels, SpiritPanel::ALL.to_vec(), "every panel, once");
    h.tick(5);
    assert_eq!(h.rigs_of(unit).len(), 1, "one spirit, held");

    // Spent on a save (or run out, or purged): the aura goes, so does it.
    h.set_auras(unit, vec![other_boost()]);
    h.tick(1);
    assert!(h.rigs_of(unit).is_empty(), "down with the aura");

    // A killing blow leaves the aura on the corpse.
    h.set_auras(unit, vec![spirit_aura()]);
    h.tick(1);
    assert_eq!(h.rigs_of(unit).len(), 1);
    h.app
        .world_mut()
        .get_mut::<Combatant>(unit)
        .unwrap()
        .current_health = 0.0;
    h.tick(1);
    assert!(h.rigs_of(unit).is_empty(), "down with its bearer's death");

    let other = h.spawn_unit(Vec3::new(4.0, 1.0, 0.0));
    h.set_auras(other, vec![spirit_aura()]);
    h.tick(1);
    assert_eq!(h.rigs_of(other).len(), 1);
    h.app.world_mut().entity_mut(other).despawn();
    h.tick(1);
    assert_eq!(
        h.count::<GuardianSpiritRig>(),
        0,
        "a spirit without its bearer goes"
    );
}

/// The spirit sits 1.9 yd above its bearer's feet and 0.25 yd behind its
/// shoulders along its facing, and follows it.
#[test]
fn the_spirit_sits_behind_the_shoulders_and_follows() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(Vec3::new(5.0, 1.0, -3.0));
    h.set_auras(unit, vec![spirit_aura()]);
    h.tick(2);
    let rig = h.rig_of(unit);
    let at = h.app.world().get::<Transform>(rig).unwrap().translation;
    // Facing +Z (unrotated): behind is -Z. The transform stands 1.0 above
    // the floor.
    assert!(at.distance(Vec3::new(5.0, 1.9, -3.25)) < 1e-4, "{at}");

    // Turned to face +X, and moved.
    {
        let mut t = h.app.world_mut().get_mut::<Transform>(unit).unwrap();
        t.translation = Vec3::new(-2.0, 1.0, 6.0);
        t.rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    }
    h.tick(1);
    let at = h.app.world().get::<Transform>(rig).unwrap().translation;
    assert!(at.distance(Vec3::new(-2.25, 1.9, 6.0)) < 1e-4, "{at}");
}

/// Wherever the camera stands, every glow and the disc face it, unrolled,
/// each laid out along the camera's right at its pose and drawn at its pose's
/// size and alpha; the gold ring lies level; and every live spark faces it.
/// Each panel is drawn as the bench draws it: its colour, its blend, and its
/// sprite (the glow gradient on a quad, a hard disc, a ring of the bench's
/// 3 px stroke).
#[test]
fn the_spirit_faces_the_camera_wherever_it_stands() {
    for cam_at in [Vec3::new(12.0, 8.0, 0.0), Vec3::new(-3.0, 16.0, 14.0)] {
        let mut h = Harness::new();
        let cam = h.camera(cam_at);
        let unit = h.spawn_unit(Vec3::new(0.0, 1.0, 0.0));
        h.set_auras(unit, vec![spirit_aura()]);
        // 0.85 s: inside the onset's flare window, the sparks steady.
        h.tick(18);
        let rig = h.rig_of(unit);
        let rig_rotation = h.app.world().get::<Transform>(rig).unwrap().rotation;
        assert_eq!(rig_rotation, Quat::IDENTITY, "the rig is unrotated");
        let age = h.app.world().get::<GuardianSpiritRig>(rig).unwrap().age - TICK_SECS;
        let right = cam.rotation * Vec3::X;

        let panels = h.panels(rig);
        assert_eq!(panels.len(), SpiritPanel::ALL.len());
        let mut faced = 0;
        for (panel, t, material, mesh) in &panels {
            let want = spirit_panel_pose(*panel, age).expect("every panel draws at 0.85 s");
            let world = h.app.world();
            let what = format!("{panel:?}");
            let (rgb, additive, sprite) = bench_look(*panel);
            assert_eq!(panel.sprite(), sprite, "{what} sprite");
            if additive {
                assert_additive(material, rgb, &what);
            } else {
                assert_eq!(material.alpha_mode, AlphaMode::Blend, "{what} darkens");
                assert!(material.unlit, "{what} is a flat colour");
                assert_rgb(material.base_color, rgb, &what);
            }
            match sprite {
                PanelSprite::Glow => {
                    let tex = material.base_color_texture.as_ref().expect("a glow sprite");
                    assert_eq!(material.emissive_texture.as_ref(), Some(tex), "{what}");
                    assert_glow_sprite(world, tex, &what);
                    assert_unit_quad(world, mesh, &what);
                }
                PanelSprite::Disc => {
                    assert!(material.base_color_texture.is_none(), "{what}: hard-edged");
                    let (_, hi, n) = radii(world, mesh);
                    assert!(
                        close(hi, 0.5) && n > 16,
                        "{what}: a unit disc, {n} vertices"
                    );
                }
                PanelSprite::LevelRing => {
                    assert!(
                        material.base_color_texture.is_none(),
                        "{what}: a solid line"
                    );
                    // The ring as drawn: its mesh's radii at its scale.
                    let (lo, hi, _) = radii(world, mesh);
                    let (inner, outer) = (lo * t.scale.x, hi * t.scale.x);
                    assert!(
                        close(outer - inner, 3.0 / (1000.0 / 7.0)),
                        "{what}: drawn {} yd wide, not the bench's 3 px",
                        outer - inner
                    );
                    assert!(
                        close(outer - inner, want.height),
                        "{what}: the pose's width"
                    );
                    assert!(close((outer + inner) / 2.0, 0.35), "{what}: radius 0.35");
                }
            }
            assert!(
                close(material.base_color.alpha(), want.alpha),
                "{panel:?} alpha {} vs {}",
                material.base_color.alpha(),
                want.alpha
            );
            match panel.sprite() {
                PanelSprite::LevelRing => {
                    let normal = t.rotation * Vec3::Z;
                    assert!(normal.abs().dot(Vec3::Y) > 0.9999, "{panel:?} lies level");
                    assert!(close(t.scale.x, want.width) && close(t.scale.y, want.width));
                    assert!(t.translation.distance(Vec3::Y * want.up) < 1e-4);
                }
                PanelSprite::Glow | PanelSprite::Disc => {
                    assert!(
                        (t.rotation * Vec3::Z).dot(cam.rotation * Vec3::Z) > 0.9999,
                        "{panel:?} does not face the camera at {cam_at}"
                    );
                    assert!(
                        (t.rotation * Vec3::Y).dot(cam.rotation * Vec3::Y) > 0.9999,
                        "{panel:?} is rolled against the camera at {cam_at}"
                    );
                    assert!(close(t.scale.x, want.width) && close(t.scale.y, want.height));
                    let laid = right * want.right + Vec3::Y * want.up;
                    assert!(
                        t.translation.distance(laid) < 1e-4,
                        "{panel:?} at {} not {laid}",
                        t.translation
                    );
                    faced += 1;
                }
            }
        }
        assert_eq!(faced, 7, "seven camera-facing panels checked");
        // The wings spread along the camera's right, either side.
        let wing = |p: SpiritPanel| panels.iter().find(|q| q.0 == p).unwrap().1.translation;
        assert!(wing(SpiritPanel::WingLeft).dot(right) < -0.5);
        assert!(wing(SpiritPanel::WingRight).dot(right) > 0.5);

        let sparks = h.live_sparks(rig);
        assert!(sparks.len() > 30, "too few live sparks: {}", sparks.len());
        for (_, _, t) in &sparks {
            assert!(
                (t.rotation * Vec3::Z).dot(cam.rotation * Vec3::Z) > 0.999,
                "a spark does not face the camera at {cam_at}"
            );
            assert!(
                (t.rotation * Vec3::Y).dot(cam.rotation * Vec3::Y) > 0.999,
                "a spark is rolled against the camera at {cam_at}"
            );
        }
    }
}

/// The sparks fly at 2.8 yd/s from 0.05 below the spirit's centre, and a held
/// spirit keeps a steady ~50 of them (100 a second, living 0.5 s) rather than
/// a pile.
#[test]
fn the_sparks_fly_at_the_benchs_speed_and_keep_a_steady_count() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(Vec3::new(0.0, 1.0, 0.0));
    h.set_auras(unit, vec![spirit_aura()]);
    h.tick(40);
    let rig = h.rig_of(unit);
    let sparks = h.live_sparks(rig);
    assert!(
        (40..=60).contains(&sparks.len()),
        "a steady ~50 sparks, got {}",
        sparks.len()
    );
    for (age, velocity, t) in &sparks {
        assert!(close(velocity.length(), 2.8), "{}", velocity.length());
        // Born in the box about (0, -0.05, 0), flown for its age.
        let born = t.translation - *velocity * *age;
        assert!(
            born.x.abs() <= 0.4 + 1e-3 && born.z.abs() <= 0.4 + 1e-3,
            "{born}"
        );
        assert!((born.y + 0.05).abs() <= 0.15 + 1e-3, "{born}");
    }
    let total = h.children(rig).len();
    assert!(total < 70, "spent sparks are retired: {total} children");
}

/// A Guardian Spirit save plays the client's heal landing (Heal's rising
/// stream) on its holder with the heal's number in the heal green, and the
/// "Saved" cue: gold "Saved" in `fct::INVULNERABLE`, at number size, at the
/// number's anchor and one floating line above it on screen, and the gold
/// flash. The marker is consumed.
#[test]
fn a_spirit_save_lands_the_heal_and_the_saved_cue() {
    let mut h = Harness::new();
    let holder = h.spawn_unit(Vec3::new(3.0, 1.0, 2.0));
    h.save(holder, "Guardian Spirit", Some(126.4));
    h.tick(1);
    assert_eq!(
        h.count::<KillingBlowPrevented>(),
        0,
        "the marker is consumed"
    );

    let texts = h.texts();
    assert_eq!(
        texts.len(),
        2,
        "{:?}",
        texts.iter().map(|t| &t.text).collect::<Vec<_>>()
    );
    let heal = texts
        .iter()
        .find(|t| t.text == "+126")
        .expect("the heal's number");
    let saved = texts.iter().find(|t| t.text == "Saved").expect("Saved");
    assert_eq!(heal.color, theme::fct::HEAL);
    assert_eq!(saved.color, theme::fct::INVULNERABLE);
    assert!(!saved.is_crit, "number size, not crit size");
    // The FCT anchor above the holder (the first of the spread pattern, the
    // centre).
    let anchor = Vec3::new(3.0, 1.0 + FCT_HEIGHT, 2.0);
    assert!(heal.world_position.distance(anchor) < 1e-4);
    assert!(saved.world_position.distance(anchor) < 1e-4);
    assert_eq!(heal.offset, None, "the number stands at its anchor");
    assert_eq!(
        saved.offset,
        Some(FctScreenOffset(egui::vec2(0.0, -theme::fct::LINE_STEP))),
        "one line up"
    );

    let impacts: Vec<(Entity, HealImpactKind)> = h
        .app
        .world_mut()
        .query::<&HealImpact>()
        .iter(h.app.world())
        .map(|i| (i.target, i.kind))
        .collect();
    assert_eq!(impacts, vec![(holder, HealImpactKind::HealStream)]);
    let flashes: Vec<Entity> = h
        .app
        .world_mut()
        .query::<&SavedFlash>()
        .iter(h.app.world())
        .map(|f| f.target)
        .collect();
    assert_eq!(flashes, vec![holder]);
}

/// A save with no heal (the Rogue's Cheat Death) lands the same "Saved" cue,
/// and no heal: no number, no heal landing.
#[test]
fn a_save_without_a_heal_lands_only_the_saved_cue() {
    let mut h = Harness::new();
    let holder = h.spawn_unit(Vec3::new(0.0, 1.0, 0.0));
    h.save(holder, "Cheat Death", None);
    h.tick(1);
    assert_eq!(
        h.count::<KillingBlowPrevented>(),
        0,
        "the marker is consumed"
    );
    let texts: Vec<String> = h.texts().into_iter().map(|t| t.text).collect();
    assert_eq!(texts, vec!["Saved".to_string()]);
    assert_eq!(h.count::<HealImpact>(), 0, "no heal, no heal landing");
    assert_eq!(h.count::<SavedFlash>(), 1);
}

/// The flash is the bench's gold glow, added light; it rides its holder's
/// body (1.3 yd above the feet), faces the camera at 2.2 × 3.0 yd, keys its
/// alpha to the bench's ramp, and is gone once 0.6 s have passed.
#[test]
fn the_saved_flash_rides_the_body_faces_the_camera_and_goes() {
    let mut h = Harness::new();
    let cam = h.camera(Vec3::new(-6.0, 9.0, 11.0));
    let holder = h.spawn_unit(Vec3::new(1.0, 1.0, 1.0));
    h.save(holder, "Guardian Spirit", Some(100.0));
    h.tick(1);
    let flash = h
        .app
        .world_mut()
        .query_filtered::<Entity, With<SavedFlash>>()
        .single(h.app.world())
        .expect("one flash");
    {
        // Drawn as the bench draws it: its gold, added light, the glow
        // gradient on a quad.
        let world = h.app.world();
        let material = world
            .resource::<Assets<StandardMaterial>>()
            .get(
                &world
                    .get::<MeshMaterial3d<StandardMaterial>>(flash)
                    .unwrap()
                    .0,
            )
            .unwrap();
        assert_additive(material, [255, 215, 90], "the flash");
        let tex = material.base_color_texture.as_ref().expect("a glow sprite");
        assert_eq!(material.emissive_texture.as_ref(), Some(tex));
        assert_glow_sprite(world, tex, "the flash");
        assert_unit_quad(world, &world.get::<Mesh3d>(flash).unwrap().0, "the flash");
    }
    h.app
        .world_mut()
        .get_mut::<Transform>(holder)
        .unwrap()
        .translation = Vec3::new(4.0, 1.0, -2.0);
    let mut seen = 0;
    for _ in 0..14 {
        h.tick(1);
        let world = h.app.world();
        let Some(f) = world.get::<SavedFlash>(flash) else {
            break;
        };
        let t = world.get::<Transform>(flash).unwrap();
        assert!(t.translation.distance(Vec3::new(4.0, 1.3, -2.0)) < 1e-4);
        assert!((t.rotation * Vec3::Z).dot(cam.rotation * Vec3::Z) > 0.9999);
        assert!((t.rotation * Vec3::Y).dot(cam.rotation * Vec3::Y) > 0.9999);
        assert_eq!(t.scale, Vec3::new(2.2, 3.0, 1.0));
        let handle = &world
            .get::<MeshMaterial3d<StandardMaterial>>(flash)
            .unwrap()
            .0;
        let alpha = world
            .resource::<Assets<StandardMaterial>>()
            .get(handle)
            .unwrap()
            .base_color
            .alpha();
        assert!(
            close(alpha, saved_flash_alpha(f.age - TICK_SECS)),
            "{alpha}"
        );
        seen += 1;
    }
    assert!(seen >= 10, "the flash lived {seen} ticks");
    assert!(
        h.app.world().get_entity(flash).is_err(),
        "the flash is gone after 0.6 s"
    );
}

/// The match camera (`CameraController`'s placement) at `zoom` and `yaw`,
/// looking at `target`.
fn match_camera(target: Vec3, zoom: f32, yaw: f32) -> Transform {
    let pitch = CameraController::default().pitch;
    let at = target
        + Vec3::new(
            zoom * pitch.sin() * yaw.sin(),
            zoom * pitch.cos(),
            zoom * pitch.sin() * yaw.cos(),
        );
    Transform::from_translation(at).looking_at(target, Vec3::Y)
}

/// Where `p` lands on a `size` viewport (points) through `camera` and Bevy's
/// default perspective (45° field of view).
fn project(camera: &Transform, p: Vec3, size: egui::Vec2) -> egui::Pos2 {
    let projection = PerspectiveProjection {
        aspect_ratio: size.x / size.y,
        ..default()
    };
    let clip = projection.get_clip_from_view() * camera.compute_matrix().inverse() * p.extend(1.0);
    assert!(clip.w > 0.0, "in front of the camera");
    let ndc = clip.truncate() / clip.w;
    egui::pos2((ndc.x + 1.0) / 2.0 * size.x, (1.0 - ndc.y) / 2.0 * size.y)
}

/// At the match camera, "Saved" never lies on top of the heal's number: both
/// labels as the consumer spawns them, projected and placed by the
/// renderer's own placement rule (`fct_placements`), measured in the client's own
/// fonts (`ui::install`) with their 2 pt outline, are apart at every yaw from
/// the closest zoom to the farthest.
#[test]
fn saved_and_the_heal_number_never_overlap_at_the_match_camera() {
    let mut h = Harness::new();
    let holder = h.spawn_unit(Vec3::new(3.0, 1.0, 2.0));
    h.save(holder, "Guardian Spirit", Some(126.0));
    h.tick(1);
    let world = h.app.world_mut();
    let mut texts = world.query::<(&FloatingCombatText, Option<&FctScreenOffset>)>();

    let ctx = egui::Context::default();
    arenasim::ui::install(&ctx);
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    let size_of = |text: &str| {
        ctx.fonts(|f| {
            f.layout_no_wrap(
                text.to_string(),
                egui::FontId::proportional(theme::fct::NUMBER_SIZE),
                egui::Color32::WHITE,
            )
            .size()
        })
    };
    let outline = egui::vec2(4.0, 4.0);
    let viewport = egui::vec2(1280.0, 720.0);
    let default_zoom = CameraController::default().zoom_distance;
    let mut checked = 0;
    for zoom in [20.0, default_zoom, 220.0] {
        for step in 0..8 {
            let yaw = step as f32 * std::f32::consts::FRAC_PI_4;
            let cam = match_camera(Vec3::new(3.0, 1.0, 2.0), zoom, yaw);
            // Placed by the renderer's own rule, through this camera.
            let placed = fct_placements(texts.iter(world), |p| Some(project(&cam, p, viewport)));
            assert_eq!(placed.len(), 2, "both labels on screen");
            let rect = |text: &str| {
                let (_, at) = placed
                    .iter()
                    .find(|(t, _)| t.text == text)
                    .unwrap_or_else(|| panic!("{text} placed"));
                egui::Rect::from_center_size(*at, size_of(text) + outline)
            };
            let (a, b) = (rect("+126"), rect("Saved"));
            assert!(
                !a.intersects(b),
                "zoom {zoom}, yaw {yaw}: the number {a:?} and Saved {b:?} overlap"
            );
            assert!(b.max.y < a.min.y, "Saved stands above the number");
            checked += 1;
        }
    }
    assert_eq!(checked, 24);
}

/// A save whose holder another blow of the same frame killed plays nothing on
/// the corpse: no "Saved", no flash, no heal. A living holder saved in the
/// same frame still gets its cue.
#[test]
fn a_save_on_a_corpse_plays_nothing() {
    let mut h = Harness::new();
    let corpse = h.spawn_unit(Vec3::new(0.0, 1.0, 0.0));
    h.app
        .world_mut()
        .get_mut::<Combatant>(corpse)
        .unwrap()
        .current_health = 0.0;
    let living = h.spawn_unit(Vec3::new(5.0, 1.0, 0.0));
    h.save(corpse, "Guardian Spirit", None);
    h.save(living, "Cheat Death", None);
    h.tick(1);
    assert_eq!(
        h.count::<KillingBlowPrevented>(),
        0,
        "both markers consumed"
    );
    let texts: Vec<String> = h.texts().into_iter().map(|t| t.text).collect();
    assert_eq!(texts, vec!["Saved".to_string()], "only the living holder's");
    let flashes: Vec<Entity> = h
        .app
        .world_mut()
        .query::<&SavedFlash>()
        .iter(h.app.world())
        .map(|f| f.target)
        .collect();
    assert_eq!(flashes, vec![living]);
    assert_eq!(h.count::<HealImpact>(), 0);
}
