//! Probes for the rogue stuns' caster-side gestures (Cheap Shot, Kidney Shot).
//!
//! The two are byte-identical on the receiver side and differ entirely on the
//! caster side, so what these pin is that each one's marker produces ITS OWN
//! stroke and ITS OWN crescent fan — the distinction is the whole feature, and
//! nothing else in the codebase would notice if they collapsed into each other.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` — no window, no GPU.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{
    Combatant, CrescentFlare, InstantAbilityFired, SwingStyle, VisualBody, WeaponHand, WeaponKind,
    WeaponSocket,
};
use arenasim::states::play_match::{
    build_crescent_ladder, cleanup_crescent_flares, consume_instant_ability_signals,
    crescent_blend_alpha, crescent_ladder_len, crescent_rung, crescent_rung_gain,
    update_crescent_flares, SAND_CRESCENT_GAIN, SAND_CRESCENT_LEVEL,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(50);

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
        // `consume_instant_ability_signals` reads Frost Nova's radius from
        // the real ability data rather than restating it as a constant.
        app.insert_resource(AbilityDefinitions::default());
        app.add_systems(
            Update,
            (
                consume_instant_ability_signals,
                update_crescent_flares,
                cleanup_crescent_flares,
            )
                .chain(),
        );
        Harness { app }
    }

    /// A Rogue with a main-hand dagger socket, as `held_weapon_models` draws it.
    fn spawn_rogue(&mut self) -> Entity {
        let unit = self
            .app
            .world_mut()
            .spawn((
                Transform::from_xyz(0.0, 1.0, 0.0),
                Combatant::new(0, 0, CharacterClass::Rogue),
            ))
            .id();
        let body = self
            .app
            .world_mut()
            .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
            .id();
        let socket = self
            .app
            .world_mut()
            .spawn((
                WeaponSocket {
                    kind: WeaponKind::Dagger,
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
            ))
            .id();
        self.app.world_mut().entity_mut(body).add_child(socket);
        self.app.world_mut().entity_mut(unit).add_child(body);
        unit
    }

    fn spawn_victim(&mut self) -> Entity {
        self.app
            .world_mut()
            .spawn((
                Transform::from_xyz(0.0, 1.0, 3.0),
                Combatant::new(1, 0, CharacterClass::Priest),
            ))
            .id()
    }

    fn fire(&mut self, caster: Entity, target: Entity, ability: AbilityType) {
        self.app.world_mut().spawn(InstantAbilityFired {
            caster,
            target: Some(target),
            ability,
            is_crit: false,
        });
    }

    fn tick(&mut self, n: u32) {
        for _ in 0..n {
            self.app.update();
        }
    }

    fn crescents(&mut self) -> usize {
        self.app
            .world_mut()
            .query::<&CrescentFlare>()
            .iter(self.app.world())
            .count()
    }

    fn style(&mut self) -> SwingStyle {
        self.app
            .world_mut()
            .query::<&WeaponSocket>()
            .iter(self.app.world())
            .next()
            .unwrap()
            .swing_style
    }

    fn markers(&mut self) -> usize {
        self.app
            .world_mut()
            .query::<&InstantAbilityFired>()
            .iter(self.app.world())
            .count()
    }
}

#[test]
fn cheap_shot_starts_its_own_stroke_and_fan() {
    let mut h = Harness::new();
    let rogue = h.spawn_rogue();
    let victim = h.spawn_victim();
    h.fire(rogue, victim, AbilityType::CheapShot);
    h.tick(1);

    assert_eq!(h.style(), SwingStyle::CheapShot, "its own stroke, not Auto");
    // CHEAP_SHOT_CRESCENTS.count — four, matching the source's four quads.
    assert_eq!(h.crescents(), 4, "four crescents");
    assert_eq!(h.markers(), 0, "the marker is consumed as it is read");
}

#[test]
fn the_fan_is_staggered_not_simultaneous() {
    // The source pops Cheap Shot's crescents in two pairs 100ms apart. A fan
    // that appeared all at once would read as one thick smear rather than
    // successive slashes, and nothing else would catch that.
    let mut h = Harness::new();
    let rogue = h.spawn_rogue();
    let victim = h.spawn_victim();
    h.fire(rogue, victim, AbilityType::CheapShot);
    h.tick(1);

    let delays: Vec<f32> = {
        let mut q = h.app.world_mut().query::<&CrescentFlare>();
        let mut d: Vec<f32> = q.iter(h.app.world()).map(|c| c.delay).collect();
        d.sort_by(|a, b| a.partial_cmp(b).unwrap());
        d
    };
    assert_eq!(delays.len(), 4);
    assert_eq!(delays[0], 0.0, "the first crescent is immediate");
    assert!(
        delays[3] > delays[0],
        "the last must be held back, got {delays:?}"
    );
}

#[test]
fn the_fan_spreads_across_the_screen() {
    // Each crescent is rolled about the view axis so the fan reads as separate
    // strokes. All-equal rolls would stack them into one shape.
    let mut h = Harness::new();
    let rogue = h.spawn_rogue();
    let victim = h.spawn_victim();
    h.fire(rogue, victim, AbilityType::CheapShot);
    h.tick(1);

    let rolls: Vec<f32> = {
        let mut q = h.app.world_mut().query::<&CrescentFlare>();
        q.iter(h.app.world()).map(|c| c.roll).collect()
    };
    let first = rolls[0];
    assert!(
        rolls.iter().any(|r| (r - first).abs() > 0.1),
        "every crescent shares a roll: {rolls:?}"
    );
}

#[test]
fn kidney_shot_starts_its_own_stroke_and_fan() {
    let mut h = Harness::new();
    let rogue = h.spawn_rogue();
    let victim = h.spawn_victim();
    h.fire(rogue, victim, AbilityType::KidneyShot);
    h.tick(1);

    assert_eq!(h.style(), SwingStyle::KidneyShot);
    // KIDNEY_SHOT_CRESCENTS.count — three, against Cheap Shot's four.
    assert_eq!(h.crescents(), 3, "three crescents");
}

#[test]
fn the_two_rogue_stuns_are_visually_distinct() {
    // This is the whole feature. The two apply the same Stun and are
    // byte-identical on the receiver side, so if their caster-side gestures
    // ever converge, nothing else in the codebase would notice.
    let mut a = Harness::new();
    let ra = a.spawn_rogue();
    let va = a.spawn_victim();
    a.fire(ra, va, AbilityType::CheapShot);
    a.tick(1);
    let cheap_count = a.crescents();
    let cheap_style = a.style();
    let cheap: Vec<(f32, f32, f32)> = {
        let mut q = a.app.world_mut().query::<&CrescentFlare>();
        q.iter(a.app.world())
            .map(|c| {
                let s = c.color.to_srgba();
                (s.red, s.green, s.blue)
            })
            .collect()
    };

    let mut b = Harness::new();
    let rb = b.spawn_rogue();
    let vb = b.spawn_victim();
    b.fire(rb, vb, AbilityType::KidneyShot);
    b.tick(1);
    let kidney_count = b.crescents();
    let kidney_style = b.style();
    let kidney: Vec<(f32, f32, f32)> = {
        let mut q = b.app.world_mut().query::<&CrescentFlare>();
        q.iter(b.app.world())
            .map(|c| {
                let s = c.color.to_srgba();
                (s.red, s.green, s.blue)
            })
            .collect()
    };

    assert_ne!(cheap_style, kidney_style, "different strokes");
    assert_ne!(cheap_count, kidney_count, "different crescent counts");
    assert_ne!(cheap[0], kidney[0], "different tints");
    // Both are magenta-family — the reference is unambiguous on that — so the
    // hue is NOT what separates them. Kidney Shot is the hotter, more saturated
    // pink; Cheap Shot's halo is paler, closer to white with a fringe.
    let (cr, cg, cb) = cheap[0];
    let (kr, kg, kb) = kidney[0];
    assert!(
        cr > cg && cb > cg,
        "Cheap Shot's halo should be magenta-family, got {cheap:?}"
    );
    assert!(
        kr > kg + 0.4 && kb > kg + 0.3,
        "Kidney Shot's should be strongly magenta, got {kidney:?}"
    );
    assert!(
        cg > kg + 0.2,
        "Cheap Shot's halo should be visibly paler than Kidney Shot's: \
         {cheap:?} vs {kidney:?}"
    );
}

#[test]
fn kidney_shot_is_the_longer_stroke() {
    // The source is 1233ms against Cheap Shot's 634ms. The finisher must stay
    // the heavier of the two; swapping them would invert the reading.
    let cheap = SwingStyle::CheapShot.stroke_secs();
    let kidney = SwingStyle::KidneyShot.stroke_secs();
    assert!(
        kidney > cheap * 1.5,
        "kidney {kidney}s should be well past cheap {cheap}s"
    );
}

#[test]
fn the_fan_sweeps_across_the_casters_breadth() {
    // The first version spawned every crescent at ONE point and fanned only
    // their roll, so the flare read as a rosette clumped beside the body rather
    // than a slash through it. The old probe never caught that because it only
    // compared rolls — it never looked at where the crescents actually were.
    //
    // A combatant capsule is 1.0yd across, so the fan has to span at least that
    // to read as body-wide.
    // Kidney Shot only — Cheap Shot is a halo on the VICTIM, not a sweep across
    // the caster, and has its own probe below.
    {
        let ability = AbilityType::KidneyShot;
        let mut h = Harness::new();
        let rogue = h.spawn_rogue();
        // Victim straight ahead on +Z, so "across" is the world X axis.
        let victim = h.spawn_victim();
        h.fire(rogue, victim, ability);
        h.tick(1);

        let xs: Vec<f32> = {
            let mut q = h.app.world_mut().query::<(&CrescentFlare, &Transform)>();
            q.iter(h.app.world())
                .map(|(_, t)| t.translation.x)
                .collect()
        };
        let lo = xs.iter().cloned().fold(f32::INFINITY, f32::min);
        let hi = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let span = hi - lo;
        assert!(
            span > 1.0,
            "{ability:?}'s fan spans only {span}yd across a 1.0yd body — it will \
             read as a clump beside the caster, not a slash through it"
        );
        // And it must stay a slash rather than becoming two separate effects
        // either side of the unit.
        assert!(
            span < 3.0,
            "{ability:?}'s fan spans {span}yd — too scattered"
        );
    }
}

#[test]
fn the_fan_follows_the_aim_not_the_world_axes() {
    // The sweep is across the caster's OWN facing, so a target off to the side
    // must rotate the whole fan with it. Spreading along a fixed world axis
    // would collapse the fan to a point for half the possible target bearings.
    let mut h = Harness::new();
    let rogue = h.spawn_rogue();
    // Victim on +X this time, so "across" becomes the world Z axis.
    let victim = h
        .app
        .world_mut()
        .spawn((
            Transform::from_xyz(6.0, 1.0, 0.0),
            Combatant::new(1, 0, CharacterClass::Priest),
        ))
        .id();
    h.fire(rogue, victim, AbilityType::KidneyShot);
    h.tick(1);

    let zs: Vec<f32> = {
        let mut q = h.app.world_mut().query::<(&CrescentFlare, &Transform)>();
        q.iter(h.app.world())
            .map(|(_, t)| t.translation.z)
            .collect()
    };
    let span = zs.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
        - zs.iter().cloned().fold(f32::INFINITY, f32::min);
    assert!(
        span > 1.0,
        "with the target on +X the fan should spread along Z, got {span}yd"
    );
}

#[test]
fn the_streak_reads_horizontally_from_every_bearing() {
    // The slash must cut ACROSS the victim — right to left on screen — whichever
    // way the caster happens to be facing.
    //
    // Two world axes were tried and both fail somewhere, because projecting a
    // horizontal world vector to the screen degenerates when it points near the
    // view axis: the AIM goes vertical for the usual over-the-shoulder camera,
    // and the across-body SWEEP goes vertical when the caster faces sideways.
    // This asserts the property that actually matters — the on-screen result —
    // rather than any particular means of getting there, and it sweeps the
    // caster through a full circle of bearings so a fix that only works from one
    // angle cannot pass.
    for i in 0..8 {
        let a = i as f32 / 8.0 * std::f32::consts::TAU;
        let (dx, dz) = (a.cos() * 6.0, a.sin() * 6.0);

        let mut h = Harness::new();
        let rogue = h.spawn_rogue();
        let victim = h
            .app
            .world_mut()
            .spawn((
                Transform::from_xyz(dx, 1.0, dz),
                Combatant::new(1, 0, CharacterClass::Priest),
            ))
            .id();
        let cam = h
            .app
            .world_mut()
            .spawn((
                Camera3d::default(),
                Transform::from_xyz(0.0, 9.0, 24.0).looking_at(Vec3::ZERO, Vec3::Y),
            ))
            .id();
        h.fire(rogue, victim, AbilityType::KidneyShot);
        h.tick(8);

        let cam_rot = h.app.world().get::<Transform>(cam).unwrap().rotation;
        let rots: Vec<Quat> = {
            let mut q = h.app.world_mut().query::<(&CrescentFlare, &Transform)>();
            q.iter(h.app.world()).map(|(_, t)| t.rotation).collect()
        };
        assert!(!rots.is_empty(), "no crescents to check");

        for rot in &rots {
            // The streak's long axis, in the camera's own frame.
            let axis = cam_rot.inverse() * (*rot * Vec3::X);
            let mut off = axis.y.atan2(axis.x).abs();
            // A streak and its 180-degree twin read the same.
            off = off.min(std::f32::consts::PI - off);
            assert!(
                off < 1.0,
                "at bearing {a:.2} a streak sits {off} rad off horizontal — \
                 that is the head-to-toe orientation, not a cut across the body"
            );
        }
    }
}

#[test]
fn kidney_shots_streak_is_a_slash_not_a_pill() {
    // Shape stated as geometry rather than left to the eye. The circular-arc
    // parametrisation this replaced could not exceed about 2:1 however it was
    // tuned, and rendered as a fat pill.
    let aspect = arenasim::states::play_match::crescent_aspect();
    assert!(aspect > 6.0, "the stroke is {aspect}:1 — that is a blob");
    let bow = arenasim::states::play_match::crescent_bow();
    assert!(bow > 0.05 && bow < 0.45, "bow {bow} is not a cut");
}

#[test]
fn cheap_shots_halo_sits_above_the_victim_not_the_caster() {
    // The whole placement finding. Cheap Shot shares Sap's visual kit, and the
    // reference for that kit is a ring floating over the TARGET's head — the
    // rogue's own side of it is just the weapon flash. The first version put
    // four slashes in front of the caster, which is a different ability's
    // grammar entirely.
    let mut h = Harness::new();
    let rogue = h.spawn_rogue();
    let victim = h.spawn_victim(); // 3yd along +Z
    h.fire(rogue, victim, AbilityType::CheapShot);
    h.tick(1);

    let ps: Vec<Vec3> = {
        let mut q = h.app.world_mut().query::<(&CrescentFlare, &Transform)>();
        q.iter(h.app.world()).map(|(_, t)| t.translation).collect()
    };
    let centre = ps.iter().fold(Vec3::ZERO, |a, b| a + *b) / ps.len() as f32;

    assert!(
        centre.z > 2.0,
        "the halo centred at {centre:?} — it belongs over the victim at z=3, \
         not beside the rogue at z=0"
    );
    // And above the victim's crown (sim y 1.0, crown 2.25).
    assert!(
        centre.y > 2.3,
        "the halo should float clear of the victim's head, got y={}",
        centre.y
    );
}

#[test]
fn cheap_shots_halo_is_a_ring_not_a_line() {
    // Four crescents arranged around an ellipse, so the fan must have real
    // extent on BOTH ground axes. A row of slashes would collapse to one.
    let mut h = Harness::new();
    let rogue = h.spawn_rogue();
    let victim = h.spawn_victim();
    h.fire(rogue, victim, AbilityType::CheapShot);
    h.tick(1);

    let ps: Vec<Vec3> = {
        let mut q = h.app.world_mut().query::<(&CrescentFlare, &Transform)>();
        q.iter(h.app.world()).map(|(_, t)| t.translation).collect()
    };
    let span = |f: fn(&Vec3) -> f32| {
        ps.iter().map(f).fold(f32::NEG_INFINITY, f32::max)
            - ps.iter().map(f).fold(f32::INFINITY, f32::min)
    };
    assert!(span(|p| p.x) > 1.0, "no width: {ps:?}");
    assert!(
        span(|p| p.z) > 1.0,
        "no depth — the halo is a flat line: {ps:?}"
    );
    // Tilted, so it reads as a ring rather than a flat disc from the camera.
    assert!(span(|p| p.y) > 0.3, "the halo is not tilted: {ps:?}");
}

#[test]
fn crescents_expire_without_leaking() {
    let mut h = Harness::new();
    let rogue = h.spawn_rogue();
    let victim = h.spawn_victim();
    h.fire(rogue, victim, AbilityType::CheapShot);
    h.tick(1);
    assert!(
        h.crescents() > 0,
        "guard: the drain must not pass vacuously"
    );

    // Past the last crescent's delay plus its lifetime.
    h.tick(30);
    assert_eq!(h.crescents(), 0, "the fan must not leak");
}

#[test]
fn a_caster_with_no_socket_still_gets_its_flourish() {
    // This Mage has no `WeaponSocket` at all (none is spawned here), so the
    // stroke half of the router is unreachable for it. That must not suppress
    // the flourish — the bug the router restructure fixed.
    let mut h = Harness::new();
    let mage = h
        .app
        .world_mut()
        .spawn((
            Transform::from_xyz(0.0, 1.0, 0.0),
            Combatant::new(0, 0, CharacterClass::Mage),
        ))
        .id();
    let victim = h.spawn_victim();
    // Cheap Shot on a socketless caster is not a real pairing; it is the
    // cheapest way to prove the flourish path does not depend on a socket.
    h.fire(mage, victim, AbilityType::CheapShot);
    h.tick(1);

    assert_eq!(h.crescents(), 4, "the fan does not require a weapon");
    assert_eq!(h.markers(), 0);
}

// ==============================================================================
// Sand legibility (AS-435)
// ==============================================================================

/// The bench's crescent colour at life fraction `k`: the three-stop track,
/// lerped in sRGB, at level 1.0 in linear light.
fn bench_track(f: &CrescentFlare, k: f32) -> LinearRgba {
    const FLASH_AT: f32 = 0.22;
    let (a, b, u) = if k < FLASH_AT {
        (f.color, f.color_mid, k / FLASH_AT)
    } else {
        (f.color_mid, f.color_end, (k - FLASH_AT) / (1.0 - FLASH_AT))
    };
    let (a, b) = (a.to_srgba(), b.to_srgba());
    Color::srgb(
        a.red + (b.red - a.red) * u,
        a.green + (b.green - a.green) * u,
        a.blue + (b.blue - a.blue) * u,
    )
    .to_linear()
}

/// AS-435: each stroke is alpha-blended in its own colour track — no added
/// light — at level 1.0, its opacity `min(1, 2.0 × fade × texture)` (the Sand
/// Legibility Bench's signed-off gain), carried as a ladder rung's baked gain
/// times a material alpha that never exceeds 1. Every live crescent of both
/// rogue stuns, every frame of its life.
#[test]
fn the_strokes_are_blended_in_their_own_colour_at_gain_two() {
    assert_eq!(SAND_CRESCENT_GAIN, 2.0, "the signed-off gain");
    assert_eq!(SAND_CRESCENT_LEVEL, 1.0, "the signed-off level");
    for ability in [AbilityType::CheapShot, AbilityType::KidneyShot] {
        let mut h = Harness::new();
        let rogue = h.spawn_rogue();
        let victim = h.spawn_victim();
        h.fire(rogue, victim, ability);
        let (mut saturated, mut unsaturated) = (0, 0);
        for _ in 0..24 {
            h.tick(1);
            let live: Vec<(CrescentFlare, Handle<StandardMaterial>)> = {
                let mut q = h
                    .app
                    .world_mut()
                    .query::<(&CrescentFlare, &MeshMaterial3d<StandardMaterial>)>();
                q.iter(h.app.world())
                    .map(|(f, m)| (f.clone(), m.0.clone()))
                    .collect()
            };
            for (flare, handle) in live {
                if flare.age < flare.delay {
                    continue;
                }
                let k = ((flare.age - flare.delay) / flare.lifetime).clamp(0.0, 1.0);
                let fade = (1.0 - k) * (1.0 - k);
                let materials = h.app.world().resource::<Assets<StandardMaterial>>();
                let m = materials.get(&handle).unwrap();
                assert_eq!(m.alpha_mode, AlphaMode::Blend, "{ability:?}");
                assert!(m.unlit, "{ability:?}: an unlit layer");
                assert_eq!(m.emissive, LinearRgba::BLACK, "{ability:?}: no added light");

                let c = m.base_color.to_linear();
                let want = bench_track(&flare, k);
                for (got, want, ch) in [
                    (c.red, want.red, "red"),
                    (c.green, want.green, "green"),
                    (c.blue, want.blue, "blue"),
                ] {
                    assert!(
                        (got - want).abs() < 1e-4,
                        "{ability:?} {ch} {got} vs {want}"
                    );
                }

                // The rung's baked gain times the material alpha is the bench's
                // gain × fade, and the material alpha stays a real opacity.
                let tex = m.base_color_texture.clone().expect("a rung");
                let rung = flare
                    .ladder
                    .0
                    .iter()
                    .position(|r| *r == tex)
                    .expect("the texture is a rung of the ladder");
                let gain = crescent_rung_gain(rung);
                assert!(c.alpha <= 1.0 && c.alpha >= 0.0, "alpha {}", c.alpha);
                let want = 2.0 * fade;
                assert!(
                    (gain * c.alpha - want).abs() < 1e-4,
                    "{ability:?}: rung gain {gain} × alpha {} vs {want}",
                    c.alpha
                );
                assert!(gain >= want.max(1.0) - 1e-6, "the rung reaches the gain");
                assert!(
                    gain - want.max(1.0) <= 1.0 / 32.0 + 1e-6,
                    "the nearest rung: {gain} for {want}"
                );
                if want > 1.0 {
                    saturated += 1;
                } else {
                    unsaturated += 1;
                }
            }
        }
        assert!(
            saturated > 0 && unsaturated > 0,
            "{ability:?}: both halves of the fade seen ({saturated}, {unsaturated})"
        );
    }
}

/// Each rung bakes `min(1, gain × stroke)`: the top rung is the bench's gain,
/// and drawn through `crescent_rung` the composite opacity is the bench's
/// `min(1, 2 × fade × texel)` — exact where the rung is unsaturated, within one
/// rung step where it saturates.
#[test]
fn the_ladder_draws_the_benchs_opacity() {
    let mut images = Assets::<Image>::default();
    let ladder = build_crescent_ladder(&mut images);
    assert_eq!(ladder.0.len(), crescent_ladder_len());
    assert_eq!(crescent_rung_gain(0), 1.0);
    assert_eq!(crescent_rung_gain(ladder.0.len() - 1), 2.0, "the top rung");

    let texels = |i: usize| -> Vec<u8> {
        let img = images.get(&ladder.0[i]).unwrap();
        img.data.as_ref().unwrap().chunks(4).map(|p| p[3]).collect()
    };
    let base = texels(0);
    assert!(base.iter().any(|&a| a > 200) && base.iter().any(|&a| a > 0 && a < 100));
    for i in [1, ladder.0.len() / 2, ladder.0.len() - 1] {
        let g = crescent_rung_gain(i);
        for (b, r) in base.iter().zip(texels(i)) {
            let want = (g * *b as f32).min(255.0);
            assert!(
                (r as f32 - want).abs() <= g + 1.0,
                "rung {i}: {r} vs min(255, {g} × {b})"
            );
        }
    }

    for f in 0..=40 {
        let fade = f as f32 / 40.0;
        let (rung, alpha) = crescent_rung(fade);
        assert!((0.0..=1.0).contains(&alpha));
        let g = crescent_rung_gain(rung);
        for t in 0..=50 {
            let texel = t as f32 / 50.0;
            let drawn = alpha * (g * texel).min(1.0);
            let bench = crescent_blend_alpha(texel, fade);
            assert_eq!(bench, (2.0 * fade * texel).min(1.0));
            if g * texel < 1.0 {
                assert!((drawn - bench).abs() < 1e-5, "fade {fade} texel {texel}");
            } else {
                assert!(
                    drawn <= bench + 1e-6 && bench - drawn <= 1.0 / 32.0 + 1e-6,
                    "fade {fade} texel {texel}: {drawn} vs {bench}"
                );
            }
        }
    }
}
