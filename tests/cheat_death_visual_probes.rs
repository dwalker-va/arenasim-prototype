//! Probes for Cheat Death's skull and ghost-fire
//! (`rendering/effects/cheat_death.rs`, AS-369).
//!
//! Two halves:
//!
//! - **The bench.** Every piece, both emitters and every sprite pinned against
//!   the signed-off bench (AS-336 Rogue Talent Visuals Bench, version
//!   1792302646-d16f, "Cheat Death", every layer kept and every multiplier at
//!   1.0): colours, sprites, blends, sizes, heights, alphas, windows, rates.
//!   The expected numbers are the bench's own formulas and literals, worked
//!   here by hand, never read back from the module.
//! - **The world.** The rig as it is drawn, run through the real systems on
//!   `MinimalPlugins` + `AssetPlugin` (no window, no GPU): raised by a Cheat
//!   Death save and by nothing else, every piece drawn with the bench's
//!   colour, blend, sprite and mesh, every quad facing the camera wherever it
//!   stands, the particles flying as the bench flies them, the ghost-fire
//!   holding with the buff and going with it, nothing left on a corpse, and
//!   the skull clear of the head at the match camera.

use std::f32::consts::{PI, TAU};
use std::time::Duration;

use arenasim::states::play_match::combat_core::{
    cheat_death_aura, KillingBlowPrevented, CHEAT_DEATH_NAME,
};
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, CameraController, ClientParticle, Combatant, FloatingCombatText,
    FloatingTextState, ParticleDrag, ParticleFacing, PlayMatchEntity,
};
use arenasim::states::play_match::{
    animate_client_particles, animate_saved_flashes, billboard_cheat_death_particles,
    build_cheat_death_rigs, cheat_death_pose, cheat_death_sprite_image,
    consume_killing_blow_signals, fire_alpha, spawn_saved_flashes, update_cheat_death,
    CheatDeathLaunch, CheatDeathPart, CheatDeathPiece, CheatDeathPose, CheatDeathRig,
    CheatDeathSprite, EmitterSprite, SavedFlash, CHEAT_DEATH_LAYERS, CHEAT_DEATH_SPRITE_PX,
};
use arenasim::CharacterClass;
use bevy::prelude::*;
use bevy::render::camera::CameraProjection;
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

use CheatDeathPiece as P;
use CheatDeathSprite as S;

/// Every piece as the bench draws it: its sRGB, its sprite, and whether it
/// darkens (the sockets, painted source-over) rather than adds light.
const BENCH_LOOK: &[(CheatDeathPiece, [u8; 3], CheatDeathSprite, bool)] = &[
    // `skull(..., [149,120,255], a)`: the body at a × 0.75, `lighter`.
    (P::SkullBody, [149, 120, 255], S::SkullBody, false),
    // `rgba(20,10,40, a × 0.85)`, source-over.
    (P::SkullSockets, [20, 10, 40], S::SkullSockets, true),
    (P::EyeLeft, [255, 255, 255], S::EyeGlow, false),
    (P::EyeRight, [255, 255, 255], S::EyeGlow, false),
    // `ring3(..., [255,250,240], a1, 3)` and `ring3(..., [255,255,214], a2, 4)`.
    (P::InnerRing, [255, 250, 240], S::LevelRing, false),
    (P::OuterRing, [255, 255, 214], S::LevelRing, false),
    // `bbGlow(hp, r3, r3, keys([[700,[91,117,150]], ...]))`.
    (P::Shockwave, [91, 117, 150], S::Glow, false),
    (P::Flare, [255, 255, 255], S::Glow, false),
    (P::LightColumn, [216, 241, 251], S::Glow, false),
];
const FLAME_RGB: [u8; 3] = [181, 134, 244];

fn bench_look(piece: CheatDeathPiece) -> ([u8; 3], CheatDeathSprite, bool) {
    if let P::Flame(_) = piece {
        return (FLAME_RGB, S::Flame, false);
    }
    let (_, rgb, sprite, darkens) = BENCH_LOOK
        .iter()
        .find(|l| l.0 == piece)
        .expect("every piece is in the table");
    (*rgb, *sprite, *darkens)
}

/// Every piece, once: the nine onset pieces and eighteen flames.
#[test]
fn the_pieces_wear_the_benchs_colours_sprites_and_blends() {
    let mut expected: Vec<CheatDeathPiece> = BENCH_LOOK.iter().map(|l| l.0).collect();
    expected.extend((0..18).map(P::Flame));
    assert_eq!(CheatDeathPiece::all(), expected, "every piece, once");
    assert_eq!(expected.len(), 27);
    for piece in expected {
        let (rgb, sprite, darkens) = bench_look(piece);
        assert_eq!(piece.rgb(), rgb, "{piece:?} colour");
        assert_eq!(piece.sprite(), sprite, "{piece:?} sprite");
        assert_eq!(piece.darkens(), darkens, "{piece:?} blend");
    }
}

fn pose_of(piece: CheatDeathPiece, age: f32, ended: Option<f32>) -> CheatDeathPose {
    cheat_death_pose(piece, age, ended)
        .unwrap_or_else(|| panic!("{piece:?} draws nothing at {age}s"))
}

fn assert_pose(piece: CheatDeathPiece, age: f32, world: Vec3, view: Vec3, size: Vec2, alpha: f32) {
    let p = pose_of(piece, age, None);
    assert!(
        near(p.world, world),
        "{piece:?} at {age}s: world {}",
        p.world
    );
    assert!(near(p.view, view), "{piece:?} at {age}s: view {}", p.view);
    assert!(
        close(p.size.x, size.x) && close(p.size.y, size.y),
        "{piece:?} at {age}s: size {}",
        p.size
    );
    assert!(
        close(p.alpha, alpha),
        "{piece:?} at {age}s: alpha {} vs {alpha}",
        p.alpha
    );
}

/// The skull: 1.3 yd, 2.9 yd above the feet; in over 120 ms, full to 567 ms,
/// out by 867 ms and drawn only before 900 ms. The body at 0.75 of it, the
/// sockets at 0.85, and the eyes (`R × 0.2` glows at `(±0.24 R, 0.12 R)`,
/// R = 0.65) at all of it, each layer 0.01 yd nearer the camera.
#[test]
fn the_skull_is_the_benchs() {
    let at = Vec3::new(0.0, 2.9, 0.0);
    let skull = Vec2::splat(1.3);
    let eye = Vec2::splat(0.26);
    for (age, a) in [(0.06, 0.5), (0.3, 1.0), (0.567, 1.0), (0.717, 0.5)] {
        assert_pose(P::SkullBody, age, at, Vec3::ZERO, skull, 0.75 * a);
        assert_pose(
            P::SkullSockets,
            age,
            at,
            Vec3::new(0.0, 0.0, 0.01),
            skull,
            0.85 * a,
        );
        assert_pose(P::EyeLeft, age, at, Vec3::new(-0.156, 0.078, 0.02), eye, a);
        assert_pose(P::EyeRight, age, at, Vec3::new(0.156, 0.078, 0.02), eye, a);
    }
    for age in [0.0, 0.87, 0.9, 1.5] {
        for piece in [P::SkullBody, P::SkullSockets, P::EyeLeft, P::EyeRight] {
            assert_eq!(cheat_death_pose(piece, age, None), None, "{piece:?} {age}");
        }
    }
}

/// Over the head (3.04 yd), 0.54-1.4 s: the inner ring, radius
/// `keys([[567,0.56],[1067,0.97]]) × 0.5` at a 3 px stroke; the outer ring,
/// `keys([[700,0.56],[1200,0.83]]) × 0.55` at 4 px (the bench's framing, 8 yd
/// at 1000 px focal); the shockwave glow `keys([[700,0.56],[1300,1.67]])`
/// across at half alpha, slate -> white -> mauve; the 1.3 yd white flare.
#[test]
fn the_rings_shockwave_and_flare_are_the_benchs() {
    let hp = Vec3::new(0.0, 3.04, 0.0);
    // 600 ms.
    let r1 = 0.56 + 0.41 * (33.0 / 500.0);
    assert_pose(
        P::InnerRing,
        0.6,
        hp,
        Vec3::ZERO,
        Vec2::new(r1 * 0.5, 0.024),
        60.0 / 160.0,
    );
    assert_eq!(
        cheat_death_pose(P::OuterRing, 0.6, None),
        None,
        "not yet in"
    );
    // 817 ms.
    let r1 = 0.56 + 0.41 * (250.0 / 500.0);
    assert_pose(
        P::InnerRing,
        0.817,
        hp,
        Vec3::ZERO,
        Vec2::new(r1 * 0.5, 0.024),
        1.0 - 117.0 / 400.0,
    );
    let r2 = 0.56 + 0.27 * (117.0 / 500.0);
    assert_pose(
        P::OuterRing,
        0.817,
        hp,
        Vec3::ZERO,
        Vec2::new(r2 * 0.55, 0.032),
        1.0 - 57.0 / 440.0,
    );
    let r3 = 0.56 + 1.11 * (117.0 / 600.0);
    assert_pose(
        P::Shockwave,
        0.817,
        hp,
        Vec3::ZERO,
        Vec2::splat(r3),
        0.5 * (1.0 - 57.0 / 540.0),
    );
    let u = 117.0 / 180.0;
    let want = [
        91.0 + (255.0 - 91.0) * u,
        117.0 + (255.0 - 117.0) * u,
        150.0 + (255.0 - 150.0) * u,
    ];
    let got = pose_of(P::Shockwave, 0.817, None).rgb;
    for (g, w) in got.into_iter().zip(want) {
        assert!((g - w).abs() < 0.5, "shockwave {got:?} vs {want:?}");
    }
    // At 1.1 s the shockwave is halfway from white to mauve.
    let got = pose_of(P::Shockwave, 1.09, None).rgb;
    let u = (1090.0 - 880.0) / 420.0;
    let want = [
        255.0 + (152.0 - 255.0) * u,
        255.0 + (99.0 - 255.0) * u,
        255.0 + (157.0 - 255.0) * u,
    ];
    for (g, w) in got.into_iter().zip(want) {
        assert!((g - w).abs() < 0.5, "shockwave {got:?} vs {want:?}");
    }
    assert_pose(
        P::Flare,
        0.817,
        hp,
        Vec3::ZERO,
        Vec2::splat(1.3),
        0.39 * (1.0 - 167.0 / 650.0),
    );
    for age in [0.5, 0.54, 1.4, 1.6] {
        for piece in [P::InnerRing, P::OuterRing, P::Shockwave, P::Flare] {
            assert_eq!(cheat_death_pose(piece, age, None), None, "{piece:?} {age}");
        }
    }
}

/// The light column: `t_vfx_flare05` 3.2 yd × 0.35 wide and 16.8 × 0.6 tall,
/// 3.8 yd above the feet, peaking at 0.6 alpha at 767 ms, drawn 0.7-1.15 s.
#[test]
fn the_light_column_is_the_benchs() {
    let at = Vec3::new(0.0, 3.8, 0.0);
    let size = Vec2::new(1.12, 10.08);
    assert_pose(P::LightColumn, 0.767, at, Vec3::ZERO, size, 0.6);
    assert_pose(
        P::LightColumn,
        0.72,
        at,
        Vec3::ZERO,
        size,
        0.6 * 20.0 / 67.0,
    );
    assert_pose(
        P::LightColumn,
        0.9,
        at,
        Vec3::ZERO,
        size,
        0.6 * (1.0 - 133.0 / 333.0),
    );
    for age in [0.69, 0.7, 1.1, 1.12, 1.2] {
        assert_eq!(cheat_death_pose(P::LightColumn, age, None), None, "{age}");
    }
}

/// The bench's `ghostFire`, worked here: flame `k` at `ms` stands at angle
/// `k / 18 τ + ms / 900` and radius `0.4 (1 − 0.35 ph)`, `−0.1 + 2.3 ph` up,
/// `0.42 (1 − 0.5 ph)` across, at `a sin(ph π) 0.8`; `ph` its place in a
/// `520 + (k % 5) 60` ms rise, offset `97 k` ms.
fn bench_flame(k: u8, ms: f32, a: f32) -> (Vec3, f32, f32) {
    let per = 520.0 + (k % 5) as f32 * 60.0;
    let ph = ((ms + k as f32 * 97.0) % per) / per;
    let angle = k as f32 / 18.0 * TAU + ms / 900.0;
    let r = 0.4 * (1.0 - ph * 0.35);
    (
        Vec3::new(r * angle.cos(), -0.1 + ph * 2.3, r * angle.sin()),
        0.42 * (1.0 - ph * 0.5),
        a * (ph * PI).sin() * 0.8,
    )
}

/// The ghost-fire's alpha: nothing to 0.8 s, full at 1.367 s, held while the
/// buff is, then out over 0.6 s.
#[test]
fn the_ghost_fire_comes_in_holds_and_goes_as_the_bench_has_it() {
    for (age, ended, want) in [
        (0.0, None, 0.0),
        (0.79, None, 0.0),
        (1.0835, None, 0.5),
        (1.367, None, 1.0),
        (2.9, None, 1.0),
        (40.0, None, 1.0),
        (3.0, Some(3.0), 1.0),
        (3.3, Some(3.0), 0.5),
        (3.6, Some(3.0), 0.0),
        (4.0, Some(3.0), 0.0),
    ] {
        assert!(
            close(fire_alpha(age, ended), want),
            "{age}s ended {ended:?}: {} vs {want}",
            fire_alpha(age, ended)
        );
    }
    // Flame 7 at 2 s, by hand: ph = (2679 mod 640) / 640.
    let p = pose_of(P::Flame(7), 2.0, None);
    assert!(close(p.world.y, 0.327_656), "{}", p.world.y);
    assert!(close(p.world.xz().length(), 0.373_969));
    assert!(close(p.size.x, 0.380_953) && close(p.size.y, 0.380_953));
    assert!(close(p.alpha, 0.441_186), "{}", p.alpha);
    // Every flame, through the fire's whole life.
    for (age, ended) in [
        (0.95, None),
        (1.5, None),
        (2.2, None),
        (2.95, None),
        (3.2, Some(3.0)),
    ] {
        for k in 0..18 {
            let (world, size, alpha) = bench_flame(k, age * 1000.0, fire_alpha(age, ended));
            match cheat_death_pose(P::Flame(k), age, ended) {
                Some(p) => {
                    assert!(near(p.world, world), "flame {k} at {age}s: {}", p.world);
                    assert!(close(p.size.x, size) && close(p.size.y, size));
                    assert!(close(p.alpha, alpha), "flame {k} at {age}s alpha");
                    assert_eq!(p.view, Vec3::ZERO);
                    assert_eq!(p.rgb, [181.0, 134.0, 244.0]);
                }
                None => assert!(alpha <= 1e-6, "flame {k} at {age}s draws nothing"),
            }
        }
    }
    for k in 0..18 {
        assert_eq!(cheat_death_pose(P::Flame(k), 0.5, None), None);
        assert_eq!(cheat_death_pose(P::Flame(k), 3.7, Some(3.0)), None);
    }
}

/// P6, the streaks: `t_vfx_flar_blur`, 300/s at 0.567-0.833 s, from 3.17 yd up
/// and 0.2 to the side, every direction at 8.3 yd/s under drag 0.6, living
/// 0.9 s, white -> lavender (183,183,255), alpha 0.2 -> 1 -> 0 and radius 0.4
/// -> 0.056 keyed at 5% of life. P0, the smoke: `toonsmoke16` at 3.3 yd, 50 ->
/// 100 -> 0/s across 0-567-1100 ms, emitted 167-1100 ms, sinking at 0.56 yd/s
/// within 0.23 rad of straight down, living 2.5 s, indigo, radius 0.83 -> 0.26
/// -> 0.42. Both add light.
#[test]
fn the_streaks_and_smoke_are_the_benchs() {
    let [streaks, smoke] = CHEAT_DEATH_LAYERS;

    let e = &streaks.emitter;
    assert!(streaks.on);
    assert_eq!(e.sprite, EmitterSprite::Streak);
    assert!(e.additive);
    assert_eq!(e.rate, 300.0);
    assert_eq!(e.track, None);
    assert_eq!(e.window, Some((0.567, 0.833)));
    assert_eq!(e.life, 0.9);
    assert_eq!(e.speed, 8.3);
    assert_eq!(e.gravity, 0.0);
    assert_eq!(e.color, [[255, 255, 255], [235, 210, 255], [183, 183, 255]]);
    assert_eq!(e.alpha, [0.2, 1.0, 0.0]);
    assert_eq!(e.mid, 0.05);
    let d = e.size.diameter_keys();
    for (k, radius) in [0.4, 0.056, 0.056].into_iter().enumerate() {
        assert!(close(d[k], 2.0 * radius), "streak size key {k}");
    }
    assert!(close(streaks.height, 3.17));
    assert_eq!(streaks.offset, Vec2::new(-0.2, 0.0));
    assert_eq!(streaks.launch, CheatDeathLaunch::Sphere);
    assert!(close(streaks.drag, 1.8), "3 × the bench's 0.6");
    assert!((streaks.count_between(0.0, 3.0) - 300.0 * 0.266).abs() < 1e-3);

    let e = &smoke.emitter;
    assert!(smoke.on);
    assert_eq!(e.sprite, EmitterSprite::Glow);
    assert!(e.additive);
    assert_eq!(e.window, Some((0.167, 1.1)));
    assert_eq!(e.life, 2.5);
    assert_eq!(e.speed, 0.56);
    assert_eq!(e.color, [[42, 12, 157], [60, 15, 203], [97, 110, 203]]);
    assert_eq!(e.alpha, [0.31, 0.7, 0.0]);
    assert_eq!(e.mid, 0.5);
    let d = e.size.diameter_keys();
    for (k, radius) in [0.83, 0.26, 0.42].into_iter().enumerate() {
        assert!(close(d[k], 2.0 * radius), "smoke size key {k}");
    }
    assert!(close(smoke.height, 3.3));
    assert_eq!(smoke.offset, Vec2::ZERO);
    assert_eq!(smoke.launch, CheatDeathLaunch::Sinking { cone: 0.23 });
    assert_eq!(smoke.drag, 0.0);
    // The rate track, integrated by hand: 64.73 -> 100 over 167-567 ms, then
    // 100 -> 0 over 567-1100 ms.
    let at_167 = 50.0 + 50.0 * 167.0 / 567.0;
    let total = 0.4 * (at_167 + 100.0) / 2.0 + 0.533 * 100.0 / 2.0;
    assert!((smoke.count_between(0.0, 3.0) - total).abs() < 1e-2);
    assert!(
        (smoke.count_between(0.0, 0.367) - 0.2 * (at_167 + (at_167 + 100.0) / 2.0) / 2.0).abs()
            < 1e-2
    );

    // Launches: every streak at 8.3 yd/s, both up and down; every smoke
    // particle down, within 0.23 rad of straight down, at 0.56.
    let (mut up, mut down) = (0, 0);
    for seed in 0..400u32 {
        let (at, v) = streaks.launch(seed.wrapping_mul(2_654_435_761));
        assert!(near(at, Vec3::new(-0.2, 3.17, 0.0)));
        assert!(close(v.length(), 8.3));
        if v.y > 0.0 {
            up += 1
        } else {
            down += 1
        }
        let (at, v) = smoke.launch(seed.wrapping_mul(2_654_435_761));
        assert!(near(at, Vec3::new(0.0, 3.3, 0.0)));
        assert!(close(v.length(), 0.56));
        assert!(v.normalize().angle_between(Vec3::NEG_Y) <= 0.23 + 1e-4);
    }
    assert!(up > 150 && down > 150, "a sphere: {up} up, {down} down");
}

/// A sprite's alpha at `(x, up)` in its quad's `[-1, 1]²`, read off the image
/// as the quad draws it: through the `Rectangle`'s own UVs (V = 0 on its top
/// edge, `+Y`).
fn drawn_alpha(image: &Image, x: f32, up: f32) -> f32 {
    let px = image.width();
    let u = (x + 1.0) / 2.0;
    let v = (1.0 - up) / 2.0;
    let col = ((u * px as f32) as u32).min(px - 1);
    let row = ((v * px as f32) as u32).min(px - 1);
    let data = image.data.as_ref().expect("pixels");
    let i = ((row * px + col) * 4) as usize;
    assert_eq!(&data[i..i + 3], &[255, 255, 255], "a white sprite");
    data[i + 3] as f32 / 255.0
}

fn sprite(s: CheatDeathSprite) -> Image {
    let image = cheat_death_sprite_image(s).expect("a textured sprite");
    assert_eq!(image.width(), CHEAT_DEATH_SPRITE_PX);
    assert_eq!(image.height(), CHEAT_DEATH_SPRITE_PX);
    image
}

/// The skull, the bench's `skull()` in its `R`: a cranium arc (centre 0.15 up,
/// radius 0.62) over a jaw narrowing to 0.42 either side at 0.42 down, open at
/// two 0.14 × 0.17 sockets at (±0.24, 0.12) and a nose from 0.05 to 0.2 down;
/// the sockets and nose its own sprite. Read off each image the right way up.
#[test]
fn the_skull_sprites_are_the_benchs_shapes() {
    let body = sprite(S::SkullBody);
    let sockets = sprite(S::SkullSockets);
    // (x, up, in the skull's body, in a socket or the nose)
    for (x, up, solid, hole) in [
        (0.0, 0.6, true, false),   // the dome
        (0.0, 0.73, true, false),  // just under its crown (0.77)
        (0.0, 0.82, false, false), // above it
        (0.5, 0.3, true, false),   // the temple
        (0.24, 0.12, false, true), // a socket
        (-0.24, 0.12, false, true),
        (-0.24, 0.25, false, true),  // its top (0.12 + 0.17)
        (0.0, -0.12, false, true),   // the nose
        (0.0, -0.03, true, false),   // between the nose and the sockets
        (0.3, -0.3, true, false),    // the jaw
        (0.45, -0.38, false, false), // outside the narrowing jaw
        (0.0, -0.5, false, false),   // below the chin
        (0.66, 0.0, false, false),   // beside the jaw
        (0.9, 0.9, false, false),
    ] {
        let (b, s) = (drawn_alpha(&body, x, up), drawn_alpha(&sockets, x, up));
        assert!(
            if solid { b > 0.99 } else { b < 0.01 },
            "body at ({x}, {up}): {b}"
        );
        assert!(
            if hole { s > 0.99 } else { s < 0.01 },
            "sockets at ({x}, {up}): {s}"
        );
    }
}

/// The eye's glow is a two-stop gradient, full at its centre, nothing at the
/// rim; the flame is the bench's `flame` sprite (0.8, 0.55 at 0.45, 0 at
/// the rim, filling an ellipse 0.6 wide and 1 tall standing 0.25 up); the
/// glow is the bench's `bbGlow` (1, 0.45 halfway out, 0 at the rim).
#[test]
fn the_glow_eye_and_flame_sprites_are_the_benchs() {
    let step = 1.5 / 255.0 + 2.0 / CHEAT_DEATH_SPRITE_PX as f32;
    let eye = sprite(S::EyeGlow);
    for (r, want) in [(0.0, 1.0), (0.5, 0.5), (0.8, 0.2), (1.0, 0.0)] {
        let got = drawn_alpha(&eye, r * 0.6, r * 0.8);
        assert!((got - want).abs() < step, "eye at {r}: {got} vs {want}");
    }
    let flame = sprite(S::Flame);
    for (x, up, want) in [
        (0.0, 0.0, 0.8),
        (0.0, 0.45, 0.55),
        (0.0, 0.9, 0.55 * (0.1 / 0.55)),
        (0.0, -0.7, 0.55 * (0.3 / 0.55)),
        (0.0, -0.8, 0.0),  // below the ellipse
        (0.65, 0.25, 0.0), // beside it
        (0.3, 0.1, 0.8 - 0.25 * (0.316_23 / 0.45)),
    ] {
        let got = drawn_alpha(&flame, x, up);
        assert!(
            (got - want).abs() < step * 2.0,
            "flame at ({x}, {up}): {got} vs {want}"
        );
    }
    let glow = sprite(S::Glow);
    for (r, want) in [
        (0.0, 1.0),
        (0.25, 0.725),
        (0.5, 0.45),
        (0.75, 0.225),
        (1.0, 0.0),
    ] {
        let got = drawn_alpha(&glow, 0.0, r);
        assert!(
            (got - want).abs() < step * 2.0,
            "glow at {r}: {got} vs {want}"
        );
    }
    assert!(
        cheat_death_sprite_image(S::LevelRing).is_none(),
        "a solid ring"
    );
}

// =============================================================================
// The world
// =============================================================================

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
        // The order `states/mod.rs` runs them in: the marker's consumer
        // (FixedUpdate there, ahead of the visual groups), the rig, the
        // particles' ageing, the billboard, and the shared flash.
        app.add_systems(
            Update,
            (
                consume_killing_blow_signals,
                build_cheat_death_rigs,
                update_cheat_death,
                animate_client_particles,
                billboard_cheat_death_particles,
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

    fn spawn_rogue(&mut self, at: Vec3) -> Entity {
        self.app
            .world_mut()
            .spawn((
                Transform::from_translation(at),
                Combatant::new(1, 0, CharacterClass::Rogue),
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

    fn save(&mut self, holder: Entity, by: &str) {
        self.app.world_mut().spawn((
            KillingBlowPrevented {
                holder,
                by: by.into(),
                heal: None,
            },
            PlayMatchEntity,
        ));
    }

    /// A Cheat Death save, its buff landed with it as the funnel lands it.
    fn cheat_death(&mut self, rogue: Entity) {
        self.set_auras(rogue, vec![cheat_death_aura(0.9, 3.0)]);
        self.save(rogue, CHEAT_DEATH_NAME);
    }

    fn kill(&mut self, unit: Entity) {
        self.app
            .world_mut()
            .get_mut::<Combatant>(unit)
            .unwrap()
            .current_health = 0.0;
    }

    fn tick(&mut self, n: usize) {
        for _ in 0..n {
            self.app.update();
        }
    }

    fn rigs_of(&mut self, bearer: Entity) -> Vec<Entity> {
        self.app
            .world_mut()
            .query::<(Entity, &CheatDeathRig)>()
            .iter(self.app.world())
            .filter(|(_, r)| r.bearer == bearer)
            .map(|(e, _)| e)
            .collect()
    }

    fn rig_of(&mut self, bearer: Entity) -> Entity {
        let rigs = self.rigs_of(bearer);
        assert_eq!(rigs.len(), 1, "one rig on the Rogue");
        rigs[0]
    }

    fn rig(&self, rig: Entity) -> &CheatDeathRig {
        self.app.world().get::<CheatDeathRig>(rig).unwrap()
    }

    /// The age the rig's pieces were last posed at.
    fn posed_age(&self, rig: Entity) -> f32 {
        self.rig(rig).age - TICK_SECS
    }

    fn children(&self, rig: Entity) -> Vec<Entity> {
        self.app
            .world()
            .get::<Children>(rig)
            .map(|c| c.iter().collect())
            .unwrap_or_default()
    }

    fn parts(&self, rig: Entity) -> Vec<Part> {
        let world = self.app.world();
        self.children(rig)
            .into_iter()
            .filter_map(|c| {
                let part = world.get::<CheatDeathPart>(c)?;
                let material = world
                    .resource::<Assets<StandardMaterial>>()
                    .get(&world.get::<MeshMaterial3d<StandardMaterial>>(c)?.0)?
                    .clone();
                let mesh = world.get::<Mesh3d>(c)?.0.clone();
                assert_eq!(mesh, part.mesh, "the part draws its own mesh");
                Some(Part {
                    piece: part.piece,
                    transform: *world.get::<Transform>(c)?,
                    material,
                    mesh,
                })
            })
            .collect()
    }

    /// Every live particle under a rig: its particle, drag and transform.
    fn particles(&self, rig: Entity) -> Vec<(ClientParticle, Option<f32>, Transform, Entity)> {
        let world = self.app.world();
        self.children(rig)
            .into_iter()
            .filter_map(|c| {
                let p = world.get::<ClientParticle>(c)?;
                (p.age < p.life).then(|| {
                    (
                        ClientParticle {
                            palette: p.palette.clone(),
                            size: p.size,
                            facing: p.facing,
                            ..*p
                        },
                        world.get::<ParticleDrag>(c).map(|d| d.per_sec),
                        *world.get::<Transform>(c).unwrap(),
                        c,
                    )
                })
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
}

struct Part {
    piece: CheatDeathPiece,
    transform: Transform,
    material: StandardMaterial,
    mesh: Handle<Mesh>,
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

/// A unit quad in its own XY plane, its top edge (+Y) at V = 0: four corners
/// at ±0.5, so a sprite stands the right way up.
fn assert_unit_quad(world: &World, handle: &Handle<Mesh>, what: &str) {
    let p = mesh_positions(world, handle);
    assert_eq!(p.len(), 4, "{what}: a quad");
    let mesh = world.resource::<Assets<Mesh>>().get(handle).unwrap();
    let Some(VertexAttributeValues::Float32x2(uv)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else {
        panic!("{what}: no UVs");
    };
    for (v, uv) in p.iter().zip(uv) {
        assert!(
            close(v.x.abs(), 0.5) && close(v.y.abs(), 0.5) && close(v.z, 0.0),
            "{what}: {v}"
        );
        assert!(close(uv[0], v.x + 0.5), "{what}: U runs left to right");
        assert!(close(uv[1], 0.5 - v.y), "{what}: V runs down from the top");
    }
}

/// The radii a flat mesh in its XY plane spans.
fn radii(world: &World, handle: &Handle<Mesh>) -> (f32, f32) {
    let p = mesh_positions(world, handle);
    assert!(p.iter().all(|v| close(v.z, 0.0)), "a flat mesh");
    let r = |v: &Vec3| v.truncate().length();
    (
        p.iter().map(r).fold(f32::MAX, f32::min),
        p.iter().map(r).fold(0.0, f32::max),
    )
}

fn assert_rgb(color: Color, rgb: [f32; 3], what: &str) {
    let c = color.to_srgba();
    for (got, want) in [c.red, c.green, c.blue].into_iter().zip(rgb) {
        assert!(
            (got * 255.0 - want).abs() < 0.5,
            "{what}: {c:?} is not {rgb:?}"
        );
    }
}

fn rgb_f(c: [u8; 3]) -> [f32; 3] {
    [c[0] as f32, c[1] as f32, c[2] as f32]
}

/// A Cheat Death save raises one rig on its Rogue, carrying every piece, and
/// lands the shared "Saved" cue; a Guardian Spirit save lands the cue and
/// raises no rig.
#[test]
fn a_cheat_death_save_raises_the_rig_and_the_saved_cue() {
    let mut h = Harness::new();
    let rogue = h.spawn_rogue(Vec3::new(2.0, 1.0, -1.0));
    let other = h.spawn_rogue(Vec3::new(-4.0, 1.0, 3.0));
    h.cheat_death(rogue);
    h.save(other, "Guardian Spirit");
    h.tick(1);
    assert_eq!(
        h.count::<KillingBlowPrevented>(),
        0,
        "the markers are consumed"
    );
    let rig = h.rig_of(rogue);
    assert!(
        h.rigs_of(other).is_empty(),
        "Guardian Spirit raises no skull"
    );
    let mut pieces: Vec<CheatDeathPiece> = h.parts(rig).into_iter().map(|p| p.piece).collect();
    let all = CheatDeathPiece::all();
    pieces.sort_by_key(|p| all.iter().position(|q| q == p));
    assert_eq!(pieces, all, "every piece, once");
    assert!(h.app.world().get::<PlayMatchEntity>(rig).is_some());

    let saved = h
        .app
        .world_mut()
        .query::<&FloatingCombatText>()
        .iter(h.app.world())
        .filter(|t| t.text == "Saved")
        .count();
    assert_eq!(saved, 2, "both saves land the shared cue");
    let flashes: Vec<Entity> = h
        .app
        .world_mut()
        .query::<&SavedFlash>()
        .iter(h.app.world())
        .map(|f| f.target)
        .collect();
    assert!(flashes.contains(&rogue) && flashes.contains(&other));
}

/// The rig stands at the Rogue's feet and follows it: the skull 2.9 yd above
/// the floor wherever the Rogue goes.
#[test]
fn the_rig_stands_at_the_feet_and_follows() {
    let mut h = Harness::new();
    let rogue = h.spawn_rogue(Vec3::new(5.0, 1.0, -3.0));
    h.cheat_death(rogue);
    h.tick(2);
    let rig = h.rig_of(rogue);
    let at = h.app.world().get::<Transform>(rig).unwrap().translation;
    assert!(near(at, Vec3::new(5.0, 0.0, -3.0)), "{at}");
    {
        let mut t = h.app.world_mut().get_mut::<Transform>(rogue).unwrap();
        t.translation = Vec3::new(-2.0, 1.0, 6.0);
        t.rotation = Quat::from_rotation_y(1.2);
    }
    h.tick(8);
    let rig_t = *h.app.world().get::<Transform>(rig).unwrap();
    assert!(near(rig_t.translation, Vec3::new(-2.0, 0.0, 6.0)));
    assert_eq!(rig_t.rotation, Quat::IDENTITY, "the rig is unrotated");
    let body = h
        .parts(rig)
        .into_iter()
        .find(|p| p.piece == P::SkullBody)
        .unwrap();
    assert!(close(
        rig_t.translation.y + body.transform.translation.y,
        2.9
    ));
}

/// Wherever the camera stands, every piece is drawn as the bench draws it:
/// its colour and alpha, its blend (added light, the sockets darkening), its
/// sprite, its mesh (a unit quad the right way up, or a ring of the bench's
/// radius and stroke); every quad faces the camera unrolled, at its pose's
/// size and place in the camera's frame; the rings lie level; the sockets
/// and the eyes stand nearer the camera than the skull they lie on.
#[test]
fn every_piece_is_drawn_as_the_bench_draws_it_wherever_the_camera_stands() {
    for cam_at in [Vec3::new(12.0, 8.0, 0.0), Vec3::new(-3.0, 16.0, 14.0)] {
        let mut h = Harness::new();
        let cam = h.camera(cam_at);
        let rogue = h.spawn_rogue(Vec3::new(0.0, 1.0, 0.0));
        h.cheat_death(rogue);
        // 0.85 s: the skull still in, the rings, column and fire all drawn.
        // (The first tick's clock does not advance.)
        h.tick(19);
        let rig = h.rig_of(rogue);
        let age = h.posed_age(rig);
        assert!(close(age, 0.85), "{age}");
        let (right, up, back) = (
            cam.rotation * Vec3::X,
            cam.rotation * Vec3::Y,
            cam.rotation * Vec3::Z,
        );
        let parts = h.parts(rig);
        assert_eq!(parts.len(), 27);
        let world = h.app.world();
        let mut drawn = 0;
        for part in &parts {
            let what = format!("{:?}", part.piece);
            let pose = cheat_death_pose(part.piece, age, None)
                .unwrap_or_else(|| panic!("{what} draws at 0.85 s"));
            let (rgb, sprite, darkens) = bench_look(part.piece);
            let m = &part.material;
            // The colour: the bench's (the shockwave's keyed with its pose).
            let want = if part.piece == P::Shockwave {
                pose.rgb
            } else {
                rgb_f(rgb)
            };
            assert_rgb(m.base_color, want, &what);
            assert!(close(m.base_color.alpha(), pose.alpha), "{what} alpha");
            assert!(m.unlit, "{what}: colour × sprite × alpha, unlit");
            assert_eq!(
                m.alpha_mode,
                if darkens {
                    AlphaMode::Blend
                } else {
                    AlphaMode::Add
                },
                "{what} blend"
            );
            match sprite {
                S::LevelRing => {
                    assert!(m.base_color_texture.is_none(), "{what}: a solid line");
                    let (lo, hi) = radii(world, &part.mesh);
                    assert!(close(hi - lo, pose.size.y), "{what}: stroke {}", hi - lo);
                    assert!(close((hi + lo) / 2.0, pose.size.x), "{what}: radius");
                    let stroke = if part.piece == P::InnerRing { 3.0 } else { 4.0 };
                    assert!(close(hi - lo, stroke * 8.0 / 1000.0), "{what}: px at 8 yd");
                    assert_eq!(part.transform.scale, Vec3::ONE, "{what}: built to size");
                    let normal = part.transform.rotation * Vec3::Z;
                    assert!(normal.abs().dot(Vec3::Y) > 0.9999, "{what} lies level");
                    assert!(near(part.transform.translation, Vec3::Y * 3.04));
                }
                _ => {
                    let tex = m.base_color_texture.as_ref().expect("a sprite");
                    let image = world.resource::<Assets<Image>>().get(tex).unwrap();
                    let bench = cheat_death_sprite_image(sprite).unwrap();
                    assert_eq!(image.data, bench.data, "{what} draws the {sprite:?} sprite");
                    assert_unit_quad(world, &part.mesh, &what);
                    let t = &part.transform;
                    assert!(
                        (t.rotation * Vec3::Z).dot(back) > 0.9999,
                        "{what} does not face the camera at {cam_at}"
                    );
                    assert!(
                        (t.rotation * Vec3::Y).dot(up) > 0.9999,
                        "{what} is rolled against the camera at {cam_at}"
                    );
                    assert!(close(t.scale.x, pose.size.x) && close(t.scale.y, pose.size.y));
                    let laid =
                        pose.world + right * pose.view.x + up * pose.view.y + back * pose.view.z;
                    assert!(near(t.translation, laid), "{what} at {}", t.translation);
                }
            }
            drawn += 1;
        }
        assert_eq!(drawn, 27);
        // Layered toward the camera: the sockets over the skull, the eyes
        // over the sockets; each eye to its own side.
        let at = |p: CheatDeathPiece| {
            parts
                .iter()
                .find(|q| q.piece == p)
                .unwrap()
                .transform
                .translation
        };
        let skull = at(P::SkullBody);
        assert!(close((at(P::SkullSockets) - skull).dot(back), 0.01));
        for eye in [P::EyeLeft, P::EyeRight] {
            assert!(
                (at(eye) - at(P::SkullSockets)).dot(back) > 0.005,
                "{eye:?} over the socket"
            );
        }
        assert!((at(P::EyeLeft) - skull).dot(right) < -0.15);
        assert!((at(P::EyeRight) - skull).dot(right) > 0.15);
        assert!((at(P::EyeLeft) - skull).dot(up) > 0.07, "the eyes sit high");
    }
}

/// The streaks fly from 3.17 yd above the feet and 0.2 yd aside, at 8.3 yd/s
/// slowed by the drag, facing the camera along their flight; the smoke
/// sinks from 3.3 yd at 0.56 yd/s. Each emitter emits its bench count, every
/// particle under the rig, drawn in its emitter's added light.
#[test]
fn the_streaks_and_smoke_fly_as_the_bench_flies_them() {
    let mut h = Harness::new();
    let cam = h.camera(Vec3::new(-6.0, 9.0, 11.0));
    let rogue = h.spawn_rogue(Vec3::new(1.0, 1.0, 1.0));
    h.cheat_death(rogue);
    h.tick(19);
    let rig = h.rig_of(rogue);
    let age = h.rig(rig).age;
    let particles = h.particles(rig);
    let streaks: Vec<_> = particles.iter().filter(|p| p.0.life == 0.9).collect();
    let smoke: Vec<_> = particles.iter().filter(|p| p.0.life == 2.5).collect();
    assert_eq!(streaks.len() + smoke.len(), particles.len(), "nothing else");
    // 300/s across 0.567-0.833 s: 79.8, all still alive.
    assert!(
        (79..=80).contains(&streaks.len()),
        "{} streaks",
        streaks.len()
    );
    // The smoke's track to 0.9 s.
    let at_167 = 50.0 + 50.0 * 167.0 / 567.0;
    let to = 0.9 - 0.567;
    let want = 0.4 * (at_167 + 100.0) / 2.0 + 100.0 * to - 100.0 * to * to / (2.0 * 0.533);
    assert!(
        (smoke.len() as f32 - want).abs() <= 1.0,
        "{} smoke vs {want}",
        smoke.len()
    );
    let world = h.app.world();
    for (p, drag, t, _) in &streaks {
        assert!(drag.is_some_and(|d| close(d, 1.8)), "{drag:?}");
        assert_eq!(p.facing, ParticleFacing::Streak);
        assert!(p.owned, "under the rig, retired with it");
        // Born at the emitter, flown under exp(-1.8 t) decay, a frame's drag
        // applied before each frame's step.
        let speed = p.velocity.length();
        let frames = (p.age / TICK_SECS).round() as i32;
        let flown: f32 = (1..=frames)
            .map(|i| 8.3 * (-1.8 * i as f32 * TICK_SECS).exp() * TICK_SECS)
            .sum();
        let born = t.translation - p.velocity.normalize() * flown;
        assert!(
            born.distance(Vec3::new(-0.2, 3.17, 0.0)) < 0.05,
            "born at {born}"
        );
        assert!((speed - 8.3 * (-1.8 * p.age).exp()).abs() < 0.05, "{speed}");
        // Facing the camera, its long axis along its flight on screen.
        assert!((t.rotation * Vec3::Z).dot(cam.rotation * Vec3::Z) > 0.999);
        let along = t.rotation * Vec3::Y;
        let v = p.velocity;
        let screen = v - (cam.rotation * Vec3::Z) * v.dot(cam.rotation * Vec3::Z);
        if screen.length() > 0.1 {
            assert!(
                along.dot(screen.normalize()) > 0.999,
                "a streak along its flight"
            );
        }
    }
    for (p, drag, t, _) in &smoke {
        assert_eq!(*drag, None);
        assert_eq!(p.facing, ParticleFacing::Camera);
        assert!(close(p.velocity.length(), 0.56));
        assert!(p.velocity.y < 0.0, "the smoke sinks");
        let born = t.translation - p.velocity * p.age;
        assert!(
            born.distance(Vec3::new(0.0, 3.3, 0.0)) < 0.05,
            "born at {born}"
        );
        assert!((t.rotation * Vec3::Z).dot(cam.rotation * Vec3::Z) > 0.9999);
        assert!((t.rotation * Vec3::Y).dot(cam.rotation * Vec3::Y) > 0.9999);
    }
    // Their palettes: each step the bench's keyed colour and alpha at its
    // middle, added light, on a sprite.
    let ramp = |keys: [f32; 3], mid: f32, t: f32| {
        if t < mid {
            keys[0] + (keys[1] - keys[0]) * t / mid
        } else {
            keys[1] + (keys[2] - keys[1]) * (t - mid) / (1.0 - mid)
        }
    };
    for (group, colour, alpha, mid) in [
        (
            &streaks,
            [
                [255.0, 255.0, 255.0],
                [235.0, 210.0, 255.0],
                [183.0, 183.0, 255.0],
            ],
            [0.2, 1.0, 0.0],
            0.05,
        ),
        (
            &smoke,
            [
                [42.0, 12.0, 157.0],
                [60.0, 15.0, 203.0],
                [97.0, 110.0, 203.0],
            ],
            [0.31, 0.7, 0.0],
            0.5,
        ),
    ] {
        let palette = &group[0].0.palette;
        assert!(palette.len() >= 8);
        for (i, handle) in palette.iter().enumerate() {
            let t = (i as f32 + 0.5) / palette.len() as f32;
            let m = world
                .resource::<Assets<StandardMaterial>>()
                .get(handle)
                .unwrap();
            assert_eq!(m.alpha_mode, AlphaMode::Add);
            assert!(m.base_color_texture.is_some());
            let c = m.base_color.to_srgba();
            for (ch, got) in [c.red, c.green, c.blue].into_iter().enumerate() {
                let want = ramp([colour[0][ch], colour[1][ch], colour[2][ch]], mid, t);
                assert!((got * 255.0 - want).abs() < 0.6, "step {i}: {c:?}");
            }
            assert!(
                (c.alpha - ramp(alpha, mid, t)).abs() < 1e-4,
                "step {i} alpha"
            );
        }
    }
    assert!(age > 0.85);
}

/// The ghost-fire holds while the buff does and goes with it: full at 2.5 s
/// with the buff up; out over 0.6 s from the frame the buff ends; and the
/// rig gone once the fire is out and the last smoke spent (3.6 s). A buff
/// that lands a tick after the save (a pending aura) is still held.
#[test]
fn the_ghost_fire_holds_with_the_buff_and_goes_with_it() {
    let mut h = Harness::new();
    let rogue = h.spawn_rogue(Vec3::new(0.0, 1.0, 0.0));
    // The save's buff lands two ticks late.
    h.save(rogue, CHEAT_DEATH_NAME);
    h.tick(2);
    h.set_auras(rogue, vec![cheat_death_aura(0.9, 3.0)]);
    h.tick(50);
    let rig = h.rig_of(rogue);
    let age = h.posed_age(rig);
    assert!(close(age, 2.5), "{age}");
    assert_eq!(h.rig(rig).ended, None, "held");
    let flame = |h: &Harness| {
        h.parts(rig)
            .into_iter()
            .find(|p| p.piece == P::Flame(3))
            .unwrap()
    };
    let f = flame(&h);
    let (_, _, want) = bench_flame(3, age * 1000.0, 1.0);
    assert!(close(f.material.base_color.alpha(), want), "full");
    assert!(f.transform.scale.x > 0.0);

    // The buff runs out at 3.0 s (the pose at 2.95 s still holds it).
    h.tick(9);
    h.set_auras(rogue, vec![]);
    h.tick(1);
    let ended = h.rig(rig).ended.expect("the buff ended");
    assert!(close(ended, 3.0), "{ended}");
    h.tick(6);
    let age = h.posed_age(rig);
    assert!(close(age, 3.3));
    let (_, _, want) = bench_flame(3, age * 1000.0, 0.5);
    assert!(
        close(flame(&h).material.base_color.alpha(), want),
        "half out"
    );
    h.tick(2);
    assert!(close(h.posed_age(rig), 3.4));
    assert!(
        h.particles(rig).iter().any(|p| p.0.life == 2.5),
        "the fire is out, the smoke still sinking"
    );
    h.tick(3);
    assert!(close(h.posed_age(rig), 3.55));
    assert_eq!(
        h.rigs_of(rogue).len(),
        1,
        "up until the last smoke is spent"
    );
    h.tick(2);
    assert!(h.rigs_of(rogue).is_empty(), "played out by 3.6 s");
    assert_eq!(h.count::<CheatDeathPart>(), 0);
    assert_eq!(h.count::<ClientParticle>(), 0);
}

/// A save whose buff is never seen ends its fire at the end of the onset
/// (1.9 s), so it is out by 2.5 s.
#[test]
fn a_buff_never_seen_ends_with_the_onset() {
    let mut h = Harness::new();
    let rogue = h.spawn_rogue(Vec3::new(0.0, 1.0, 0.0));
    h.save(rogue, CHEAT_DEATH_NAME);
    h.tick(39);
    let rig = h.rig_of(rogue);
    assert!(close(h.posed_age(rig), 1.85));
    assert_eq!(h.rig(rig).ended, None);
    h.tick(2);
    let ended = h.rig(rig).ended.expect("ended with the onset");
    assert!((1.9 - 1e-4..1.95 + 1e-4).contains(&ended), "{ended}");
    while h.posed_age(rig) < ended + 0.6 {
        h.tick(1);
    }
    assert!(
        h.parts(rig)
            .iter()
            .filter(|p| matches!(p.piece, P::Flame(_)))
            .all(|p| p.transform.scale == Vec3::ZERO && p.material.base_color.alpha() == 0.0),
        "the fire is out"
    );
}

/// Nothing is drawn on a corpse: a save whose Rogue another blow of the same
/// frame killed raises no rig (a living Rogue saved in the same frame still
/// gets its own); a Rogue that dies with the skull up takes its rig, every
/// piece and every particle with it that frame; and a Rogue that is gone
/// takes its rig too.
#[test]
fn nothing_draws_on_a_corpse() {
    let mut h = Harness::new();
    let corpse = h.spawn_rogue(Vec3::new(0.0, 1.0, 0.0));
    let living = h.spawn_rogue(Vec3::new(5.0, 1.0, 0.0));
    h.cheat_death(corpse);
    h.kill(corpse);
    h.cheat_death(living);
    h.tick(1);
    assert!(
        h.rigs_of(corpse).is_empty(),
        "no rig on a same-frame corpse"
    );
    assert_eq!(h.rigs_of(living).len(), 1);

    // 0.7 s in: the skull, rings, streaks and smoke all out.
    h.tick(14);
    let rig = h.rig_of(living);
    assert!(!h.particles(rig).is_empty());
    assert!(h
        .parts(rig)
        .iter()
        .any(|p| p.piece == P::SkullBody && p.material.base_color.alpha() > 0.5));
    h.kill(living);
    h.tick(1);
    assert!(h.rigs_of(living).is_empty(), "down with its Rogue");
    assert_eq!(h.count::<CheatDeathPart>(), 0, "every piece with it");
    assert_eq!(h.count::<ClientParticle>(), 0, "every particle with it");

    let gone = h.spawn_rogue(Vec3::new(-5.0, 1.0, 0.0));
    h.cheat_death(gone);
    h.tick(3);
    assert_eq!(h.rigs_of(gone).len(), 1);
    h.app.world_mut().entity_mut(gone).despawn();
    h.tick(1);
    assert_eq!(
        h.count::<CheatDeathRig>(),
        0,
        "a rig without its Rogue goes"
    );
    assert_eq!(h.count::<CheatDeathPart>(), 0);
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

/// Where `p` lands on a 1280 × 720 viewport through `camera` and Bevy's
/// default perspective (45° field of view), y down.
fn project(camera: &Transform, p: Vec3) -> Vec2 {
    let size = Vec2::new(1280.0, 720.0);
    let projection = PerspectiveProjection {
        aspect_ratio: size.x / size.y,
        ..default()
    };
    let clip = projection.get_clip_from_view() * camera.compute_matrix().inverse() * p.extend(1.0);
    assert!(clip.w > 0.0, "in front of the camera");
    let ndc = clip.truncate() / clip.w;
    Vec2::new((ndc.x + 1.0) / 2.0 * size.x, (1.0 - ndc.y) / 2.0 * size.y)
}

/// At the match camera, at every yaw from the default zoom in to the
/// closest, the skull's chin stands above the top of the Rogue's head on
/// screen (it floats over the head, never over the face), and the skull
/// (crown to chin, 1.19 R) is at least 10 px tall at the default zoom on a
/// 720 px viewport.
#[test]
fn the_skull_floats_clear_of_the_head_at_the_match_camera() {
    let feet = Vec3::new(3.0, 0.0, 2.0);
    let default_zoom = CameraController::default().zoom_distance;
    let mut checked = 0;
    for zoom in [20.0, default_zoom] {
        for step in 0..8 {
            let yaw = step as f32 * std::f32::consts::FRAC_PI_4;
            let cam = match_camera(feet + Vec3::Y, zoom, yaw);
            let mut h = Harness::new();
            h.app.world_mut().spawn((Camera3d::default(), cam));
            let rogue = h.spawn_rogue(feet + Vec3::Y);
            h.cheat_death(rogue);
            // 0.3 s: the skull at full.
            h.tick(7);
            let rig = h.rig_of(rogue);
            let rig_at = h.app.world().get::<Transform>(rig).unwrap().translation;
            let skull = h
                .parts(rig)
                .into_iter()
                .find(|p| p.piece == P::SkullBody)
                .unwrap()
                .transform;
            let centre = rig_at + skull.translation;
            let r = skull.scale.y / 2.0;
            let up = skull.rotation * Vec3::Y;
            // The chin, 0.42 R below the skull's centre in its own plane; the
            // crown 0.77 R above. The head: the capsule's top, 2.25 yd up.
            let chin = project(&cam, centre - up * 0.42 * r);
            let crown = project(&cam, centre + up * 0.77 * r);
            let head = project(&cam, feet + Vec3::Y * 2.25);
            assert!(
                chin.y < head.y,
                "zoom {zoom}, yaw {yaw}: chin {chin} not above the head {head}"
            );
            if zoom == default_zoom {
                assert!(
                    chin.y - crown.y >= 10.0,
                    "yaw {yaw}: the skull is {} px tall",
                    chin.y - crown.y
                );
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 16);
}
