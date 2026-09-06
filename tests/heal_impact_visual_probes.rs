//! Probes for the per-spell heal landings
//! (`rendering/effects/heal_impact.rs`) — the Classic-faithful replacements
//! for the one-size-fits-all healing column.
//!
//! These assert WORLD-SPACE GEOMETRY and the routing contract, not the fields
//! the rig stores: that Holy Light's curtain falls from the head and Flash
//! Heal's motes rise from the feet, that the Heal stream stays narrow, that
//! the Healing Wave butterflies orbit the torso at the blessed radius and
//! actually ORBIT, that Flash of Light is Holy Light's borrow scaled down,
//! and that every heal in the config reaches SOME landing.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window,
//! no GPU. `TransformPlugin` is load-bearing: without it `GlobalTransform`
//! never propagates and every assertion below would read a child's LOCAL pose.
//!
//! The harness registers the full graphical chain, `spawn -> animate ->
//! billboard`, because `billboard_heal_impacts` is what actually PLACES Flash
//! Heal's ray fan (each ray's roll and its walked-out centre); a probe that
//! asserted the fan before billboarding would pass on eight rays clumped at
//! one point. Probes whose subject the billboard pass overwrites spawn a
//! `Camera3d` and assert world/screen geometry; everything else — mote
//! translations, the butterfly orbit, the rig anchor — is placed by
//! `animate_heal_impacts`, which the billboard pass never touches (it writes
//! sprite poses and mote ROTATIONS only), so those probes stay camera-free,
//! matching the graphical system's own no-camera early-out.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{
    Combatant, HealButterflyWing, HealImpact, HealImpactKind, HealMote, HealSprite,
    HealSpriteRole,
};
use arenasim::states::play_match::{
    animate_heal_impacts, billboard_heal_impacts, butterfly_center, heal_envelope, heal_style,
    spawn_heal_impacts, HealAnchor, Ramp, COMBATANT_BODY_RADIUS, FLASH_HEAL_FLASH_DURATION,
    FLASH_HEAL_RAY_COUNT, FLASH_HEAL_RAY_LENGTH, FLASH_OF_LIGHT_BORROW_INTENSITY,
    FLASH_OF_LIGHT_DURATION, HEALING_WAVE_BUTTERFLIES, HEALING_WAVE_ORBIT_RADIUS,
    HEALING_WAVE_OUTWARD_DRIFT, HEAL_BASE_Y, HEAL_STREAM_WIDTH, HOLY_LIGHT_DURATION,
    IMPACT_HEAD_Y,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);

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
        // The same chained contract graphical mode registers
        // (`states/mod.rs`): `billboard` must see the poses `animate` wrote.
        app.add_systems(
            Update,
            (spawn_heal_impacts, animate_heal_impacts, billboard_heal_impacts).chain(),
        );
        Harness { app }
    }

    /// A camera for probes whose subject `billboard_heal_impacts` places.
    /// Returns the camera's world rotation for screen-frame assertions.
    fn spawn_camera(&mut self, from: Vec3, look_at: Vec3) -> Quat {
        let transform = Transform::from_translation(from).looking_at(look_at, Vec3::Y);
        self.app.world_mut().spawn((Camera3d::default(), transform));
        transform.rotation
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    /// A recipient the landing can attach to. It needs a real `Combatant`,
    /// because the rig follows one through a `With<Combatant>` query.
    fn spawn_recipient(&mut self, at: Vec3) -> Entity {
        self.app
            .world_mut()
            .spawn((
                Combatant::new(0, 0, CharacterClass::Priest),
                Transform::from_translation(at),
            ))
            .id()
    }

    fn land(&mut self, kind: HealImpactKind, recipient: Entity) -> Entity {
        self.app
            .world_mut()
            .spawn(HealImpact {
                target: recipient,
                kind,
                age: 0.0,
            })
            .id()
    }

    fn rig_pos(&mut self, rig: Entity) -> Vec3 {
        self.app
            .world()
            .get::<GlobalTransform>(rig)
            .expect("rig has a world transform")
            .translation()
    }

    /// Every live mote with its velocity and propagated world position.
    fn motes(&mut self) -> Vec<(Vec3, f32, Vec3)> {
        let mut q = self.app.world_mut().query::<(&HealMote, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(m, g)| (m.velocity, m.age, g.translation()))
            .collect()
    }

    fn sprites(&mut self) -> Vec<(HealSpriteRole, Transform, GlobalTransform)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&HealSprite, &Transform, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(s, t, g)| (s.role, *t, *g))
            .collect()
    }

    fn wings(&mut self) -> Vec<(u32, f32, Vec3)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&HealButterflyWing, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(w, g)| (w.index, w.side, g.translation()))
            .collect()
    }
}

/// Fail loudly if a collection-conditional probe went vacuous.
fn assert_min(label: &str, actual: usize, min: usize) {
    assert!(
        actual >= min,
        "{label}: expected at least {min}, saw {actual} — the probe went vacuous"
    );
}

// ── routing ────────────────────────────────────────────────────────────────

/// Every heal in the config must reach SOME landing. A future heal ability
/// with no arm in `HealImpact::kind_for` fails here instead of landing in
/// silence.
#[test]
fn every_heal_in_the_config_reaches_a_landing() {
    let defs = AbilityDefinitions::default();
    let mut silent = Vec::new();
    for (ability, config) in defs.iter() {
        if config.is_heal() && HealImpact::kind_for(*ability).is_none() {
            silent.push(*ability);
        }
    }
    assert!(
        silent.is_empty(),
        "heals with no landing at all: {silent:?} — add an arm to HealImpact::kind_for"
    );
}

/// The router names exactly the blessed per-spell mappings.
#[test]
fn the_router_names_the_blessed_mappings() {
    assert_eq!(
        HealImpact::kind_for(AbilityType::FlashHeal),
        Some(HealImpactKind::FlashHeal)
    );
    // Holy Shock's heal reuses Heal's visual verbatim in the client data.
    assert_eq!(
        HealImpact::kind_for(AbilityType::HolyShock),
        Some(HealImpactKind::HealStream)
    );
    assert_eq!(
        HealImpact::kind_for(AbilityType::HolyLight),
        Some(HealImpactKind::HolyLight)
    );
    assert_eq!(
        HealImpact::kind_for(AbilityType::FlashOfLight),
        Some(HealImpactKind::FlashOfLight)
    );
    assert_eq!(
        HealImpact::kind_for(AbilityType::LesserHealingWave),
        Some(HealImpactKind::HealingWave)
    );
    assert_eq!(HealImpact::kind_for(AbilityType::MortalStrike), None);
    assert_eq!(HealImpact::kind_for(AbilityType::Frostbolt), None);
}

// ── pure recipe checks ─────────────────────────────────────────────────────

#[test]
fn ramps_are_piecewise_linear_over_the_window() {
    let r = Ramp { start: 10.0, mid: 20.0, end: 0.0 };
    assert_eq!(r.at(0.0), 10.0);
    assert_eq!(r.at(0.25), 15.0);
    assert_eq!(r.at(0.5), 20.0);
    assert_eq!(r.at(0.75), 10.0);
    assert_eq!(r.at(1.0), 0.0);
    // Clamped outside the window.
    assert_eq!(r.at(-1.0), 10.0);
    assert_eq!(r.at(2.0), 0.0);
}

#[test]
fn the_envelope_blooms_in_and_fades_out() {
    assert_eq!(heal_envelope(0.0), 0.0);
    assert_eq!(heal_envelope(1.0), 0.0);
    assert_eq!(heal_envelope(0.3), 1.0);
    assert!(heal_envelope(0.05) > 0.0 && heal_envelope(0.05) < 1.0);
    assert!(heal_envelope(0.9) > 0.0 && heal_envelope(0.9) < 1.0);
}

/// Flash of Light is Holy Light's effect — same head anchor, same falling
/// emitter recipe — scaled by the blessed borrow intensity and shortened.
#[test]
fn flash_of_light_is_holy_lights_borrow_scaled_down() {
    let hl = heal_style(HealImpactKind::HolyLight);
    let fol = heal_style(HealImpactKind::FlashOfLight);
    assert_eq!(hl.anchor, HealAnchor::Head);
    assert_eq!(fol.anchor, HealAnchor::Head);
    assert_eq!(hl.emit_secs, HOLY_LIGHT_DURATION);
    assert_eq!(fol.emit_secs, FLASH_OF_LIGHT_DURATION);
    assert_eq!(fol.intensity, FLASH_OF_LIGHT_BORROW_INTENSITY);
    assert_eq!(hl.emitters.len(), fol.emitters.len());
    for (a, b) in hl.emitters.iter().zip(fol.emitters.iter()) {
        assert_eq!(a, b, "the borrow must not reshape the curtain");
    }
    assert!(fol.life() < hl.life());
}

/// The blessed grammar: Holy Light is the ONLY falling heal, everything else
/// rises; the head anchor is Holy Light's alone.
#[test]
fn holy_light_is_the_only_falling_head_attached_heal() {
    for kind in [
        HealImpactKind::FlashHeal,
        HealImpactKind::HealStream,
        HealImpactKind::HealingWave,
    ] {
        let style = heal_style(kind);
        assert_eq!(style.anchor, HealAnchor::Base, "{kind:?} attaches at the feet");
        for e in &style.emitters {
            assert!(e.speed > 0.0, "{kind:?} motes must RISE (speed {})", e.speed);
        }
    }
    for kind in [HealImpactKind::HolyLight, HealImpactKind::FlashOfLight] {
        let style = heal_style(kind);
        assert_eq!(style.anchor, HealAnchor::Head);
        for e in &style.emitters {
            assert!(e.speed < 0.0, "{kind:?} motes must FALL (speed {})", e.speed);
            assert!(
                (-0.8301..=-0.5599).contains(&e.speed),
                "fall speed {} outside the measured -0.56..-0.83 band",
                e.speed
            );
        }
    }
}

// ── world-space geometry ───────────────────────────────────────────────────

/// Holy Light: the curtain spawns around the HEAD and every mote falls —
/// world heights strictly decrease frame over frame.
#[test]
fn holy_light_curtain_falls_from_the_head() {
    let mut h = Harness::new();
    let at = Vec3::new(4.0, 0.0, -3.0);
    let recipient = h.spawn_recipient(at);
    let rig = h.land(HealImpactKind::HolyLight, recipient);
    h.tick(20);

    let rig_pos = h.rig_pos(rig);
    let head = at + Vec3::Y * IMPACT_HEAD_Y;
    assert!(
        rig_pos.distance(head) < 1e-3,
        "rig sits at the head anchor: {rig_pos} vs {head}"
    );

    let before = h.motes();
    assert_min("holy light motes", before.len(), 5);
    for (velocity, _, pos) in &before {
        assert!(velocity.y < 0.0, "curtain motes fall, got {velocity}");
        // The curtain hangs off the head: origins staggered +1.05..-1.09
        // around it, nothing below the feet by more than a fall-life.
        assert!(
            (pos.y - head.y) < 1.2,
            "mote spawned above the curtain's top band: {pos}"
        );
        let horizontal = Vec2::new(pos.x - at.x, pos.z - at.z).length();
        assert!(horizontal < 1.5, "curtain mote {pos} strayed off the body");
    }

    h.tick(5);
    let after = h.motes();
    // Motes that existed before and are still alive have moved DOWN.
    let min_before = before.iter().map(|(_, _, p)| p.y).fold(f32::MAX, f32::min);
    let aged_after: Vec<f32> = after
        .iter()
        .filter(|(_, age, _)| *age > 5.0 * 0.016)
        .map(|(_, _, p)| p.y)
        .collect();
    assert_min("aged holy light motes", aged_after.len(), 3);
    let min_after = aged_after.iter().copied().fold(f32::MAX, f32::min);
    assert!(
        min_after < min_before,
        "the curtain's lowest mote must descend: {min_before} -> {min_after}"
    );
}

/// Flash Heal: the ray-fan flash opens at the feet, motes RISE, and the
/// flash is gone after its window.
///
/// The fan is PLACED by `billboard_heal_impacts` — each ray's roll about the
/// view axis and its centre walked out by half its extent — so this asserts
/// the rays' rendered WORLD geometry under a real `Camera3d`, from two
/// genuinely different bearings. The first version read only local scale
/// scalars and passed on eight rays clumped at one point (see
/// `docs/solutions/implementation-patterns/visual-probes-assert-rendered-geometry.md`).
#[test]
fn flash_heal_rays_fan_and_motes_rise() {
    let at = Vec3::new(-2.0, 0.0, 6.0);
    let base = at + Vec3::Y * HEAL_BASE_Y;

    for cam_pos in [Vec3::new(0.0, 9.0, 24.0), Vec3::new(18.0, 5.0, -12.0)] {
        let mut h = Harness::new();
        let recipient = h.spawn_recipient(at);
        let rig = h.land(HealImpactKind::FlashHeal, recipient);
        let cam_rot = h.spawn_camera(cam_pos, at);
        h.tick(12); // ~0.19s: flash fully open, motes flowing

        let rig_pos = h.rig_pos(rig);
        assert!(rig_pos.distance(base) < 1e-3, "rig sits at the feet: {rig_pos}");

        let rays: Vec<(f32, GlobalTransform)> = h
            .sprites()
            .into_iter()
            .filter_map(|(role, _, g)| match role {
                HealSpriteRole::Ray { angle } => Some((angle, g)),
                _ => None,
            })
            .collect();
        assert_eq!(rays.len(), FLASH_HEAL_RAY_COUNT as usize);

        // Each ray is a centre-origin quad: its world endpoints are the
        // quad's local ±Y/2 through the propagated transform. The inner end
        // must sit ON the flash centre and the outer end must actually reach
        // the blessed length — a translation offset of anything but half the
        // extent (the "reaching half as far as claimed" defect) fails here.
        let mut outer_ends = Vec::new();
        let mut bearings = Vec::new();
        for (angle, g) in &rays {
            let inner = g.transform_point(Vec3::new(0.0, -0.5, 0.0));
            let outer = g.transform_point(Vec3::new(0.0, 0.5, 0.0));
            assert!(
                inner.distance(rig_pos) < 0.05,
                "ray {angle:.2}'s inner end {inner} is off the flash centre {rig_pos}"
            );
            let reach = outer.distance(rig_pos);
            assert!(
                (FLASH_HEAL_RAY_LENGTH * 0.9..=FLASH_HEAL_RAY_LENGTH * 1.02)
                    .contains(&reach),
                "ray {angle:.2} reaches {reach}yd, not ~{FLASH_HEAL_RAY_LENGTH}yd"
            );
            // In the camera's own frame the ray must lie in the billboard
            // plane, at its own bearing.
            let dir = cam_rot.inverse() * (outer - inner).normalize();
            assert!(
                dir.z.abs() < 0.02,
                "ray {angle:.2} leans {} out of the billboard plane",
                dir.z
            );
            bearings.push(dir.y.atan2(dir.x));
            outer_ends.push(outer);
        }

        // The fan's world extent: opposite rays' tips span ~2x the ray
        // length. Eight distinct rolls at one point would span ~0.
        let mut span = 0.0_f32;
        for i in 0..outer_ends.len() {
            for j in (i + 1)..outer_ends.len() {
                span = span.max(outer_ends[i].distance(outer_ends[j]));
            }
        }
        assert!(
            (FLASH_HEAL_RAY_LENGTH * 1.8..=FLASH_HEAL_RAY_LENGTH * 2.1).contains(&span),
            "the open fan spans {span}yd, not ~{}yd — clumped or overreaching",
            FLASH_HEAL_RAY_LENGTH * 2.0
        );

        // Eight distinct, evenly spaced bearings around the flash centre.
        bearings.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let step = std::f32::consts::TAU / FLASH_HEAL_RAY_COUNT as f32;
        for pair in bearings.windows(2) {
            let gap = pair[1] - pair[0];
            assert!(
                (gap - step).abs() < 0.1,
                "uneven fan from {cam_pos}: gap {gap} rad, want {step}: {bearings:?}"
            );
        }
        let wrap = bearings[0] + std::f32::consts::TAU - bearings.last().unwrap();
        assert!(
            (wrap - step).abs() < 0.1,
            "uneven fan across the wrap: {wrap} rad, want {step}: {bearings:?}"
        );

        let flares = h
            .sprites()
            .into_iter()
            .filter(|(role, ..)| matches!(role, HealSpriteRole::LensFlare))
            .count();
        assert_eq!(flares, 1, "one central lens flare");

        let motes = h.motes();
        assert_min("flash heal motes", motes.len(), 4);
        for (velocity, _, pos) in &motes {
            assert!(velocity.y > 0.0, "flash heal motes rise, got {velocity}");
            assert!(pos.y >= base.y - 0.1, "motes start at/above the feet: {pos}");
        }

        // Past the flash window the rays have collapsed.
        let flash_frames = (FLASH_HEAL_FLASH_DURATION / 0.016).ceil() as u32 + 2;
        h.tick(flash_frames);
        for (role, t, _) in h.sprites() {
            if matches!(role, HealSpriteRole::Ray { .. }) {
                assert!(
                    t.scale.y <= 1e-3,
                    "a ray must be gone after the flash window, scale {}",
                    t.scale.y
                );
            }
        }
    }
}

/// Heal / Holy Shock heal: a NARROW rising stream — every mote within the
/// blessed stream envelope of the recipient's axis, no flash sprites at all.
#[test]
fn heal_stream_is_narrow_quiet_and_rising() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, 0.0, 0.0);
    let recipient = h.spawn_recipient(at);
    h.land(HealImpactKind::HealStream, recipient);
    h.tick(40); // most of the emit window

    assert_eq!(h.sprites().len(), 0, "the quiet stream has no flash, no glow");

    let motes = h.motes();
    assert_min("heal stream motes", motes.len(), 8);
    // Widest emitter offset (0.28) plus half the stream envelope.
    let max_r = 0.28 + HEAL_STREAM_WIDTH / 2.0 + 1e-3;
    for (velocity, _, pos) in &motes {
        assert!(velocity.y > 0.0, "stream motes rise");
        let horizontal = Vec2::new(pos.x - at.x, pos.z - at.z).length();
        assert!(
            horizontal <= max_r + 0.01,
            "stream mote {pos} outside the narrow envelope ({horizontal} > {max_r})"
        );
    }
}

/// Healing Wave: 8 butterflies (16 wings) orbit the torso at the blessed
/// radius, drift outward, and actually ORBIT — their bearings advance.
#[test]
fn healing_wave_butterflies_orbit_the_torso() {
    let mut h = Harness::new();
    let at = Vec3::new(3.0, 0.0, 3.0);
    let recipient = h.spawn_recipient(at);
    h.land(HealImpactKind::HealingWave, recipient);
    h.tick(8);

    let wings = h.wings();
    assert_eq!(
        wings.len(),
        (HEALING_WAVE_BUTTERFLIES * 2) as usize,
        "8 butterflies, two wings each"
    );

    let base = at + Vec3::Y * HEAL_BASE_Y;
    let bearing_of = |index: u32, wings: &[(u32, f32, Vec3)]| -> f32 {
        // A butterfly's centre is the midpoint of its two wings.
        let pair: Vec<Vec3> = wings
            .iter()
            .filter(|(i, ..)| *i == index)
            .map(|(_, _, p)| *p)
            .collect();
        assert_eq!(pair.len(), 2);
        let mid = (pair[0] + pair[1]) / 2.0;
        (mid.z - base.z).atan2(mid.x - base.x)
    };

    for (index, _, pos) in &wings {
        let horizontal = Vec2::new(pos.x - base.x, pos.z - base.z).length();
        assert!(
            (HEALING_WAVE_ORBIT_RADIUS - 0.35..=HEALING_WAVE_ORBIT_RADIUS
                + HEALING_WAVE_OUTWARD_DRIFT
                + 0.35)
                .contains(&horizontal),
            "butterfly {index} at radius {horizontal}, outside the orbit band"
        );
        // The torso band: heights 0.3..1.3 above the feet (plus bob/wing span).
        let above_feet = pos.y - base.y;
        assert!(
            (0.05..=1.6).contains(&above_feet),
            "butterfly {index} at height {above_feet} is outside the torso band"
        );
    }

    // The swirl advances: every butterfly's bearing moves over time.
    let b0_before = bearing_of(0, &wings);
    let radius_before = {
        let (_, _, p) = wings.iter().find(|(i, ..)| *i == 0).unwrap();
        Vec2::new(p.x - base.x, p.z - base.z).length()
    };
    h.tick(20);
    let wings_later = h.wings();
    let b0_after = bearing_of(0, &wings_later);
    let delta = (b0_after - b0_before).rem_euclid(std::f32::consts::TAU);
    assert!(
        delta > 0.2 && delta < std::f32::consts::PI,
        "butterfly 0 must orbit counterclockwise, bearing moved {delta}"
    );
    // And drifts outward as it fades.
    let radius_after = {
        let (_, _, p) = wings_later.iter().find(|(i, ..)| *i == 0).unwrap();
        Vec2::new(p.x - base.x, p.z - base.z).length()
    };
    assert!(
        radius_after > radius_before,
        "butterflies drift outward: {radius_before} -> {radius_after}"
    );

    // The pure orbit math agrees with the rendered world positions.
    let expected = base + butterfly_center(0, 8.0 * 0.016);
    let pair: Vec<Vec3> = wings
        .iter()
        .filter(|(i, ..)| *i == 0)
        .map(|(_, _, p)| *p)
        .collect();
    let mid = (pair[0] + pair[1]) / 2.0;
    assert!(
        mid.distance(expected) < 0.25,
        "wing midpoint {mid} strayed from the orbit centre {expected}"
    );
}

/// Healing Wave's glow must SURROUND the body, never hide inside it. A glow
/// quad billboarded on the spine axis renders at or behind the capsule's
/// front surface out to `COMBATANT_BODY_RADIUS`, so a layer at or under that
/// radius is fully depth-occluded by the opaque body — the third build
/// shipped exactly that (0.50/0.38/0.28 radii) and the green glow vanished
/// from the screen while every stored field looked healthy. This pins the
/// RENDERED geometry: each wrap layer's drawn edge reaches outside the
/// capsule silhouette at the blessed spine heights, and the green pool lies
/// flat ON the ground at the feet, un-billboarded.
#[test]
fn healing_wave_glow_wraps_outside_the_body() {
    let mut h = Harness::new();
    let at = Vec3::new(2.0, 0.0, -1.0);
    let recipient = h.spawn_recipient(at);
    h.land(HealImpactKind::HealingWave, recipient);
    // The billboard pass poses the wrap quads, so render under a real camera.
    h.spawn_camera(Vec3::new(0.0, 9.0, 24.0), at);
    h.tick(27); // ~0.43s: k ≈ 0.31, envelope at its full-bloom plateau

    let base = at + Vec3::Y * HEAL_BASE_Y;

    // The recipe itself: blessed spine heights, and every wrap radius clears
    // the body with margin.
    let style = heal_style(HealImpactKind::HealingWave);
    let heights: Vec<f32> = style.torso_glows.iter().map(|g| g.height).collect();
    assert_eq!(heights, vec![0.49, 0.70, 0.83], "the blessed spine heights");
    for layer in &style.torso_glows {
        assert!(
            layer.radius > COMBATANT_BODY_RADIUS + 0.15,
            "wrap layer at height {} has radius {} — at or inside the body \
             (radius {COMBATANT_BODY_RADIUS}), it renders behind the capsule \
             and is invisible",
            layer.height,
            layer.radius
        );
    }

    // The rendered wrap: centred on the spine at the blessed heights, drawn
    // edge OUTSIDE the capsule silhouette.
    let glows: Vec<GlobalTransform> = h
        .sprites()
        .into_iter()
        .filter_map(|(role, _, g)| matches!(role, HealSpriteRole::TorsoGlow).then_some(g))
        .collect();
    assert_eq!(glows.len(), 3, "three wrap layers");
    let mut seen_heights: Vec<f32> = Vec::new();
    for g in &glows {
        let centre = g.translation();
        let spine_r = Vec2::new(centre.x - at.x, centre.z - at.z).length();
        assert!(spine_r < 1e-3, "wrap layer centred on the spine, off by {spine_r}");
        seen_heights.push(centre.y - base.y);
        // The world edge of the unit quad, through scale and billboard pose.
        let edge = g.transform_point(Vec3::new(0.5, 0.0, 0.0));
        let reach = Vec2::new(edge.x - at.x, edge.z - at.z).length();
        assert!(
            reach > COMBATANT_BODY_RADIUS + 0.1,
            "wrap layer's rendered edge reaches only {reach}yd from the spine \
             — buried inside the {COMBATANT_BODY_RADIUS}yd body"
        );
    }
    seen_heights.sort_by(|a, b| a.partial_cmp(b).unwrap());
    for (seen, blessed) in seen_heights.iter().zip([0.49, 0.70, 0.83]) {
        assert!(
            (seen - blessed).abs() < 1e-3,
            "wrap layer rendered at height {seen}, blessed {blessed}"
        );
    }

    // The green pool at the feet: one flat quad on the ground, in the blessed
    // radius band, and NOT billboarded — its normal stays vertical under the
    // camera the billboard pass is using.
    let pools: Vec<GlobalTransform> = h
        .sprites()
        .into_iter()
        .filter_map(|(role, _, g)| matches!(role, HealSpriteRole::UnderGlow).then_some(g))
        .collect();
    assert_eq!(pools.len(), 1, "one feet under-glow");
    let pool = pools[0];
    let pos = pool.translation();
    let spine_r = Vec2::new(pos.x - at.x, pos.z - at.z).length();
    assert!(spine_r < 1e-3, "the pool sits under the feet, off by {spine_r}");
    assert!(
        (base.y - 0.12..=base.y + 0.01).contains(&pos.y),
        "the pool lies at ground level, not at {} (feet at {})",
        pos.y,
        base.y
    );
    let normal = (pool.transform_point(Vec3::Z) - pos).normalize();
    assert!(
        normal.y.abs() > 0.99,
        "the pool must lie FLAT on the ground (billboarding it tips it \
         toward the camera); normal {normal}"
    );
    let edge = pool.transform_point(Vec3::new(0.5, 0.0, 0.0));
    let reach = Vec2::new(edge.x - at.x, edge.z - at.z).length();
    assert!(
        (0.6..=1.3).contains(&reach),
        "the pool's rendered radius {reach} is outside the blessed 0.8–1.2 \
         band (with bloom slack)"
    );
}

/// The landing follows a recipient that keeps moving — a healed runner
/// carries their sparkle.
#[test]
fn the_landing_follows_a_moving_recipient() {
    let mut h = Harness::new();
    let recipient = h.spawn_recipient(Vec3::ZERO);
    let rig = h.land(HealImpactKind::HealStream, recipient);
    h.tick(3);
    let before = h.rig_pos(rig);

    let moved = Vec3::new(5.0, 0.0, 2.0);
    h.app
        .world_mut()
        .get_mut::<Transform>(recipient)
        .unwrap()
        .translation = moved;
    h.tick(2);
    let after = h.rig_pos(rig);
    assert!(
        (after - before).length() > 4.0,
        "the rig must follow: {before} -> {after}"
    );
    assert!(after.distance(moved + Vec3::Y * HEAL_BASE_Y) < 1e-3);
}

/// A landing retires itself — rig and every child gone once it is spent.
#[test]
fn a_spent_landing_despawns_with_all_its_pieces() {
    let mut h = Harness::new();
    let recipient = h.spawn_recipient(Vec3::ZERO);
    let rig = h.land(HealImpactKind::HealingWave, recipient);
    h.tick(10);
    assert!(h.app.world().get_entity(rig).is_ok());
    assert_min("healing wave pieces mid-flight", h.wings().len() + h.motes().len(), 16);

    let life = heal_style(HealImpactKind::HealingWave).life();
    h.tick((life / 0.016).ceil() as u32 + 4);
    assert!(
        h.app.world().get_entity(rig).is_err(),
        "the rig must despawn when spent"
    );
    assert_eq!(h.wings().len(), 0, "wings die with the rig");
    assert_eq!(h.motes().len(), 0, "motes die with the rig");
}
