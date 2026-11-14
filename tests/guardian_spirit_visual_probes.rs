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
    ActiveAuras, Aura, AuraType, ClientParticle, Combatant, CompoundDebuff, FloatingCombatText,
    FloatingTextState, HealImpact, HealImpactKind, PlayMatchEntity,
};
use arenasim::states::play_match::{
    animate_client_particles, animate_guardian_spirits, animate_saved_flashes,
    billboard_guardian_spirit_sparks, consume_killing_blow_signals, saved_flash_alpha,
    spawn_saved_flashes, spirit_panel_pose, update_guardian_spirit_states, EmitterSprite,
    GuardianSpiritPanel, GuardianSpiritRig, PanelPose, PanelSprite, SavedFlash, SpiritPanel,
    FCT_HEIGHT, GUARDIAN_SPIRIT_SPARK_EMITTER,
};
use arenasim::ui::theme;
use arenasim::CharacterClass;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

const TICK: Duration = Duration::from_millis(50);
const TICK_SECS: f32 = 0.05;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
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
    color: bevy_egui::egui::Color32,
    world_position: Vec3,
    is_crit: bool,
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

    fn panels(&self, rig: Entity) -> Vec<(SpiritPanel, Transform, StandardMaterial)> {
        let world = self.app.world();
        self.children(rig)
            .into_iter()
            .filter_map(|c| {
                let panel = world.get::<GuardianSpiritPanel>(c)?;
                let material = world
                    .resource::<Assets<StandardMaterial>>()
                    .get(&panel.material)?
                    .clone();
                Some((panel.panel, *world.get::<Transform>(c)?, material))
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
            .query::<&FloatingCombatText>()
            .iter(self.app.world())
            .map(|t| Text {
                text: t.text.clone(),
                color: t.color,
                world_position: t.world_position,
                is_crit: t.is_crit,
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
        for (panel, t, material) in &panels {
            let want = spirit_panel_pose(*panel, age).expect("every panel draws at 0.85 s");
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
/// "Saved" cue: gold "Saved" in `fct::INVULNERABLE`, at number size, 0.5 yd
/// left of and 0.4 yd above the number, and the gold flash. The marker is
/// consumed.
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
    assert!(
        saved
            .world_position
            .distance(anchor + Vec3::new(-0.5, 0.4, 0.0))
            < 1e-4
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

/// The flash rides its holder's body (1.3 yd above the feet), faces the
/// camera at 2.2 × 3.0 yd, keys its alpha to the bench's ramp, and is gone
/// once 0.6 s have passed.
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
