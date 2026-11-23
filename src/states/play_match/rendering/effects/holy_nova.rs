//! Holy Nova — the gold dome bursting out from the Priest's feet to the
//! spell's true reach, trailed by gold stars, and the TBC star fall on each
//! ally it heals (AS-355). Its hit on each enemy is the holy starburst
//! (`holy_starburst.rs`, the High scale).
//!
//! From the client data joined in
//! `docs/design/2026-10-17-priest-talent-client-data.md` §4 (AS-335; Holy
//! Nova's damage visual 3643, its heal visual 6882):
//!
//! - **Cast** (kit 3154): `holy_precast_high_hand.m2` on both hands, the
//!   shipped high Holy hands (`HealCastKind::HolyFlash`, flashed off the
//!   caster-centred `InstantAbilityFired` the Priest AI spawns), and
//!   `holynova_impact_base.m2` at the Priest's feet: a gold hemisphere shell
//!   growing to 6.22 yd over 767 ms, brightest in its first 0.13 s, trailed
//!   by gold stars.
//! - **Heal** (TBC kit 431, the ruling): `holylight_impact_head.m2`, Prayer of
//!   Healing's — gold stars falling from the head over about 2.5 s, over a
//!   gold glow at the feet.
//!
//! Played as the signed-off AS-335 Priest Talent Visuals Bench draws it
//! (https://claude.ai/artifact/PaMJeCzqUTVThWXZcNsyoc, version
//! 1792207490-53c9, the "Holy Nova" tab; its source as committed with the
//! sign-off is `docs/design/benches/2026-10-17-priest-talent-bench.html`).
//! Rulings (2026-10-17): the dome shows the true reach (the tab's spread knob
//! at 1.6), and the heal lands as the TBC star fall. Every layer on. The
//! pieces are the tab's `customDraw` and its `stars` emitter, one for one:
//!
//!   dome   -> nine level rings (`ring3`), ring `k` at `h = k / 9` of the
//!             dome's radius `r` up and `r √(1 − h²)` across, at
//!             `a (1 − h / 2)`, over a soft ground glow (`groundDisc`) of
//!             radius `r` at `a / 4`, all (255,216,0); `r` grows linearly to
//!             the reach over 767 ms and `a` keys 0 -> 0.7 at 134 ms -> 0 at
//!             767 ms;
//!   stars  -> four-point stars, the rate 0 -> 500/s at 300 ms -> 0 at
//!             767 ms (191 of them), born in a square 6.5 x spread across
//!             and 0.4 x spread deep, 0.4 yd above the floor, launched within
//!             1.4 rad of straight up at 1.67 x spread yd/s, living 1 s,
//!             spinning 0.6 turns a second, 0.56 -> 0.28 -> 0.03 yd radius,
//!             pale -> yellow -> gold;
//!   heals  -> per healed ally, from 150 ms after the cast for 2.5 s: twelve
//!             gold stars (`sprite('star')`, 0.14 yd, (253,255,170)) each
//!             falling from 2.3 to 0.2 yd above the feet 0.9 times a second,
//!             0.5 yd off the axis, at `a (1 − ph / 2)` where `a` keys 0 -> 1
//!             at 300 ms -> 0 at 1500 ms; and a ground glow of radius 0.7 yd
//!             at 0.6 of 0 -> 1 at 300 ms, held to 1234 ms -> 0 at 1667 ms,
//!             (255,210,60).
//!
//! **Blend, as the bench composites each call.** The rings and ground glows
//! (`ring3`, `groundDisc`) and the stars emitter are `lighter`:
//! `AlphaMode::Add`. The heal's falling stars are `customDraw`'s own
//! `sprite()` calls, which the canvas composites `source-over`:
//! `AlphaMode::Blend`.
//!
//! **The dome's radius is the reach.** It is read from the Priest's own book
//! (`holy_nova_reach`), the value its AI decided the nova's members with,
//! never restated; a book whose Holy Nova reaches farther draws a wider dome.
//! The stars' spread scales with it, so at the shipped 10 yd it is the
//! ruling's 1.6.
//!
//! **Lifecycle.** The dome rides the `InstantAbilityFired` the Priest AI
//! spawns as it commits to the nova (`instant_ability.rs`'s router builds
//! it); the star fall rides the `StarFallLanding` each landed heal spawns. A
//! second nova, or a second heal on one ally while its first fall plays,
//! plays its own rig. Both play out on a unit that dies or is polymorphed
//! under them (a landing, the Conflagrate precedent) and go at once with one
//! that is gone; stealth owns the body's material, which nothing here
//! touches. Everything is a `PlayMatchEntity`.
//!
//! **Departures from the bench**, each deliberate:
//!
//! 1. **Held in front of the body.** Every piece is drawn on its own view ray
//!    in front of its unit's RENDERED body — the dome and its stars in front
//!    of the Priest's, the fall in front of its ally's — scaled by the same
//!    ratio (the shared body hold, `body_hold.rs`): the same screen footprint,
//!    sorted in front, as the bench paints them over the bodies.
//! 2. **The dome reaches exactly the reach** (10.0 yd), where the bench's
//!    knob draws 6.22 x 1.6 = 9.95.
//! 3. **Strokes have a world width.** The bench strokes each ring 2 px; here
//!    each is 48 flat chords [`DOME_LINE_WIDTH`] wide, 2 px at the match
//!    camera (Conflagrate's rune ring).
//! 4. **The rings stand on the ground glow's lift**, [`DOME_FLOOR_LIFT`]
//!    (the bench's 0.03 yd), so the lowest is not drawn into the floor.
//! 5. **The stars' white core is tinted**, as every shared star is.
//! 6. **The fall rides the drawn body.** The bench draws it at the ally's
//!    floor; here it stands on the ally's rendered feet, so a bobbing or
//!    lifted ally carries it. The dome stands on the Priest's own floor, as
//!    a ground effect does.
//!
//! **At the match camera** (60 yd, pitch 38.7°, FOV 45°, about 22 px a yard at
//! 1080p): the dome ends about 440 px across; a trailing star is up to 25 px;
//! a falling heal star 3 px, its glow 31 px.
//!
//! Graphical-only: nothing here writes sim state, no `game_rng` is drawn (the
//! scatter is the benches' shared hash), and every system is registered in
//! `states/mod.rs` only. Headless never runs any of it.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::body_hold::{held_in, BodyHold, HoldDisjoint};
use super::conflagrate::{conflagrate_material, ConflagrateSprite};
use super::druid_heals::{body_point, keyed};
use super::holy_starburst::{set_additive_look, HOLY_NOVA_BENCH_SPREAD};
use super::inner_focus::{focus_track_rgb, track_births};
use super::paladin_oneshots::{bench_hash, oneshot_ground_texture};
use super::shield::disc_texture;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::components::*;

// ── The bench's constants block (signed off at version 1792207490-53c9) ─────

// Holy Nova: holynova_impact_base.m2 (kit 3154) at the Priest's feet; holy_impactdd_high_chest.m2 (kit 3153) on each enemy hit. AS-335 bench.
pub const HOLY_NOVA_DOME: bool = true;
// HOLY_NOVA_DOME_RADIUS: the caster's book's Holy Nova range (the ruling), not the knob's 9.95.
pub const HOLY_NOVA_STARS: bool = true;
// HOLY_NOVA_ENEMY_STARBURST, HOLY_NOVA_SIZE_MUL: `holy_starburst.rs`.
/// `HealLanding::PrayerOfHealingStarFall`, the TBC landing (the ruling).
pub const HOLY_NOVA_STAR_FALL: bool = true;

/// The reach the ruling's spread knob (1.6) was set for: the shipped range.
pub const HOLY_NOVA_BENCH_REACH: f32 = 10.0;

// ── The dome (holynova_impact_base.m2) ───────────────────────────────────────

/// The shell grows to the reach over this, and is drawn only inside it.
pub const DOME_SECS: f32 = 0.767;
/// Its weight, keyed in milliseconds (`white8x8` 0.2 + `armorreflect3` 0.5).
pub const DOME_WEIGHT: [(f32, f32); 3] = [(0.0, 0.0), (134.0, 0.7), (767.0, 0.0)];
/// Nine level rings, each a 48-sided circle (the bench's `ring3`).
pub const DOME_RINGS: u8 = 9;
pub const DOME_SEGMENTS: u8 = 48;
pub const DOME_RGB: [u8; 3] = [255, 216, 0];
/// The ground glow is drawn at this share of the dome's weight.
pub const DOME_GROUND_GAIN: f32 = 0.25;
/// The bench draws its ground glows 0.03 yd above the floor.
pub const DOME_FLOOR_LIFT: f32 = 0.03;
/// A ring's line, yards: the bench's 2 px stroke at the match camera (60 yd,
/// FOV 45°, 1080p, about 21.7 px a yard), as Conflagrate's rune ring.
pub const DOME_LINE_WIDTH: f32 = 0.092;
/// Nothing is drawn at or below this alpha or radius (`ring3`'s early out).
pub const NOVA_ALPHA_FLOOR: f32 = 0.003;
const RADIUS_FLOOR: f32 = 0.001;

// ── The trailing stars (the tab's `stars` emitter) ───────────────────────────

/// The rate, stars a second, keyed in seconds: 191 stars.
pub const NOVA_STAR_RATE: [(f32, f32); 3] = [(0.0, 0.0), (0.3, 500.0), (0.767, 0.0)];
/// The square they are born in (`area`) and its depth (`areaY`), per unit of
/// spread; how far above the floor its centre is (`pos`, not spread).
pub const NOVA_STAR_AREA: f32 = 6.5;
pub const NOVA_STAR_AREA_DEPTH: f32 = 0.4;
pub const NOVA_STAR_LIFT: f32 = 0.4;
/// Launched within this of straight up (`vRange`), at this per unit of
/// spread, living this long, spinning this many turns a second.
pub const NOVA_STAR_CONE: f32 = 1.4;
pub const NOVA_STAR_SPEED: f32 = 1.67;
pub const NOVA_STAR_LIFE: f32 = 1.0;
pub const NOVA_STAR_SPIN: f32 = 0.6;
pub const NOVA_STAR_RGB: [(f32, [u8; 3]); 3] = [
    (0.0, [255, 252, 222]),
    (0.5, [255, 249, 98]),
    (1.0, [255, 222, 39]),
];
pub const NOVA_STAR_ALPHA: [(f32, f32); 3] = [(0.0, 0.39), (0.5, 1.0), (1.0, 0.0)];
/// The sprite's RADIUS, yards (the bench's `scale`).
pub const NOVA_STAR_RADIUS: [(f32, f32); 3] = [(0.0, 0.56), (0.5, 0.28), (1.0, 0.03)];

// ── The heal's star fall (holylight_impact_head.m2, TBC) ─────────────────────

/// The fall starts this long after the cast (the bench's `t > 150`) and plays
/// for [`FALL_SECS`] (`u < 2500`).
pub const FALL_DELAY_SECS: f32 = 0.15;
pub const FALL_SECS: f32 = 2.5;
pub const FALL_STARS: u8 = 12;
/// Cycles a second, the top and the drop of each star's fall, yards above
/// the feet, and its offset from the axis.
pub const FALL_RATE: f32 = 0.9;
pub const FALL_TOP: f32 = 2.3;
pub const FALL_DROP: f32 = 2.1;
pub const FALL_OFFSET: f32 = 0.5;
/// A star's full width, yards, and colour.
pub const FALL_STAR_SIZE: f32 = 0.14;
pub const FALL_STAR_RGB: [u8; 3] = [253, 255, 170];
/// The stars' weight, keyed in milliseconds after the fall starts.
pub const FALL_STAR_WEIGHT: [(f32, f32); 3] = [(0.0, 0.0), (300.0, 1.0), (1500.0, 0.0)];
/// The glow at the feet: its radius, colour, gain and weight.
pub const FALL_GLOW_RADIUS: f32 = 0.7;
pub const FALL_GLOW_RGB: [u8; 3] = [255, 210, 60];
pub const FALL_GLOW_GAIN: f32 = 0.6;
pub const FALL_GLOW_WEIGHT: [(f32, f32); 4] =
    [(0.0, 0.0), (300.0, 1.0), (1234.0, 1.0), (1667.0, 0.0)];

/// Generated sprite textures, pixels on a side.
const SPRITE_PX: u32 = 64;

// ── The reach ────────────────────────────────────────────────────────────────

/// Holy Nova's reach for `priest`: its own book's range for the spell, the
/// value its AI decided the nova's members with. `None` for a unit whose book
/// lacks it.
pub fn holy_nova_reach(priest: &Combatant) -> Option<f32> {
    priest.book().get(&AbilityType::HolyNova).map(|c| c.range)
}

/// The stars' spread for a nova reaching `reach`: the ruling's 1.6 at the
/// shipped reach, in proportion to it.
pub fn nova_star_spread(reach: f32) -> f32 {
    HOLY_NOVA_BENCH_SPREAD * reach / HOLY_NOVA_BENCH_REACH
}

// ── Poses ────────────────────────────────────────────────────────────────────

/// The dome `age` seconds in, for a nova reaching `reach`: its radius and
/// weight, or `None` once it is over.
pub fn dome_state(age: f32, reach: f32) -> Option<(f32, f32)> {
    if !HOLY_NOVA_DOME || !(0.0..DOME_SECS).contains(&age) {
        return None;
    }
    Some((reach * age / DOME_SECS, keyed(&DOME_WEIGHT, age * 1000.0)))
}

/// Ring `k` of the dome: its height above the floor, its radius and its
/// alpha, or `None` where the bench draws nothing.
pub fn dome_ring(k: u8, age: f32, reach: f32) -> Option<(f32, f32, f32)> {
    let (r, a) = dome_state(age, reach)?;
    let h = k as f32 / DOME_RINGS as f32;
    let radius = r * (1.0 - h * h).sqrt();
    let alpha = a * (1.0 - h * 0.5);
    (alpha > NOVA_ALPHA_FLOOR && radius > RADIUS_FLOOR).then_some((
        r * h + DOME_FLOOR_LIFT,
        radius,
        alpha,
    ))
}

/// The dome's ground glow: its radius and alpha, or `None`.
pub fn dome_ground(age: f32, reach: f32) -> Option<(f32, f32)> {
    let (r, a) = dome_state(age, reach)?;
    let alpha = a * DOME_GROUND_GAIN;
    (alpha > NOVA_ALPHA_FLOOR && r > RADIUS_FLOOR).then_some((r, alpha))
}

/// Chord `i` of a ring of `radius`: its two ends, from the ring's centre.
pub fn dome_chord(i: u8, radius: f32) -> (Vec3, Vec3) {
    let at = |n: u8| {
        let a = n as f32 / DOME_SEGMENTS as f32 * TAU;
        Vec3::new(a.cos(), 0.0, a.sin()) * radius
    };
    (at(i), at(i + 1))
}

/// Every birth of the trailing stars, seconds after the cast.
pub fn nova_star_births() -> Vec<f32> {
    track_births(&NOVA_STAR_RATE)
}

fn scatter(seed: u32, j: u32, k: u32) -> f32 {
    bench_hash(
        seed.wrapping_mul(613)
            .wrapping_add(j)
            .wrapping_mul(8)
            .wrapping_add(k)
            .wrapping_add(3550),
    )
}

/// One sprite's look at a moment.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct NovaSprite {
    /// Where it truly is, from its anchor (the floor where a star was born,
    /// or the ally's feet).
    pub at: Vec3,
    /// The sprite's full width, yards.
    pub size: f32,
    pub rgb: [u8; 3],
    pub alpha: f32,
    /// Its roll in the camera's plane.
    pub roll: f32,
}

/// Trailing star `j`, born `born` seconds in, `age` seconds after the cast,
/// in a nova of `spread` seeded `seed`: where it is from the floor point it
/// was born over, its look; `None` before its birth and after its life.
pub fn nova_star(j: u32, born: f32, age: f32, spread: f32, seed: u32) -> Option<NovaSprite> {
    let page = age - born;
    if !HOLY_NOVA_STARS || !(0.0..NOVA_STAR_LIFE).contains(&page) {
        return None;
    }
    let t = page / NOVA_STAR_LIFE;
    let polar = (scatter(seed, j, 1) * 2.0 - 1.0) * NOVA_STAR_CONE;
    let azimuth = scatter(seed, j, 2) * TAU;
    let dir = Vec3::new(
        polar.sin() * azimuth.cos(),
        polar.cos(),
        polar.sin() * azimuth.sin(),
    );
    let area = NOVA_STAR_AREA * spread;
    let depth = NOVA_STAR_AREA_DEPTH * spread;
    let start = Vec3::new(
        (scatter(seed, j, 3) - 0.5) * area,
        NOVA_STAR_LIFT + (scatter(seed, j, 4) - 0.5) * depth,
        (scatter(seed, j, 5) - 0.5) * area,
    );
    let spin = scatter(seed, j, 6) * TAU + NOVA_STAR_SPIN * TAU * page;
    Some(NovaSprite {
        at: start + dir * NOVA_STAR_SPEED * spread * page,
        size: 2.0 * keyed(&NOVA_STAR_RADIUS, t),
        rgb: focus_track_rgb(&NOVA_STAR_RGB, t),
        alpha: keyed(&NOVA_STAR_ALPHA, t),
        // The canvas turns a sprite clockwise on screen.
        roll: -spin,
    })
}

/// Milliseconds into the fall `age` seconds after the heal, or `None` outside
/// it.
fn fall_ms(age: f32) -> Option<f32> {
    let u = (age - FALL_DELAY_SECS) * 1000.0;
    (u > 0.0 && u < FALL_SECS * 1000.0).then_some(u)
}

/// Falling star `k`, `age` seconds after the heal: where it is from the
/// ally's feet, its look; `None` where the bench draws nothing.
pub fn fall_star(k: u8, age: f32) -> Option<NovaSprite> {
    if !HOLY_NOVA_STAR_FALL {
        return None;
    }
    let u = fall_ms(age)?;
    let kf = k as f32;
    let ph = (u / 1000.0 * FALL_RATE + kf / FALL_STARS as f32) % 1.0;
    let alpha = keyed(&FALL_STAR_WEIGHT, u) * (1.0 - ph * 0.5);
    (alpha > NOVA_ALPHA_FLOOR).then_some(NovaSprite {
        at: Vec3::new(
            FALL_OFFSET * (kf * 2.1).sin(),
            FALL_TOP - ph * FALL_DROP,
            FALL_OFFSET * (kf * 1.3).cos(),
        ),
        size: FALL_STAR_SIZE,
        rgb: FALL_STAR_RGB,
        alpha,
        roll: 0.0,
    })
}

/// The fall's glow at the feet, `age` seconds after the heal: its radius and
/// alpha, or `None`.
pub fn fall_glow(age: f32) -> Option<(f32, f32)> {
    if !HOLY_NOVA_STAR_FALL {
        return None;
    }
    let u = fall_ms(age)?;
    let alpha = keyed(&FALL_GLOW_WEIGHT, u) * FALL_GLOW_GAIN;
    (alpha > NOVA_ALPHA_FLOOR).then_some((FALL_GLOW_RADIUS, alpha))
}

/// The dome's whole life: until its shell has gone and its last star has
/// lived out its life.
pub fn holy_nova_secs() -> f32 {
    nova_star_births()
        .last()
        .map_or(0.0, |b| b + NOVA_STAR_LIFE)
        .max(DOME_SECS)
}

/// The fall's whole life.
pub fn star_fall_secs() -> f32 {
    FALL_DELAY_SECS + FALL_SECS
}

/// Lay a unit quad flat, facing up, its local X along `along` (in the ground
/// plane).
pub fn flat_along(along: Vec3) -> Quat {
    Quat::from_rotation_y((-along.z).atan2(along.x)) * Quat::from_rotation_x(-FRAC_PI_2)
}

// ── Components ───────────────────────────────────────────────────────────────

/// One nova at its Priest's feet: parent of its rings, its ground glow and
/// its trailing stars.
#[derive(Component, Debug)]
pub struct HolyNovaDome {
    pub caster: Entity,
    /// The reach it grows to, yards: the caster's book's (`holy_nova_reach`).
    pub reach: f32,
    pub age: f32,
    /// The stars' scatter seed.
    pub seed: u32,
}

/// One drawn piece of a [`HolyNovaDome`].
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub enum DomePiece {
    /// Chord `segment` of ring `ring`.
    Ring { ring: u8, segment: u8 },
    /// The ground glow.
    Ground,
    /// Trailing star `j`, born `born` seconds in, and the floor point under
    /// the Priest on its birth frame (`None` until then).
    Star {
        j: u32,
        born: f32,
        born_at: Option<Vec3>,
    },
}

/// One heal's star fall on its ally: parent of its stars and its glow.
#[derive(Component, Debug)]
pub struct HolyStarFall {
    pub target: Entity,
    pub age: f32,
}

/// One drawn piece of a [`HolyStarFall`].
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum FallPiece {
    /// The glow at the feet.
    Glow,
    /// Falling star `k`.
    Star(u8),
}

/// The quad and sprites every piece is drawn with, built once.
#[derive(Clone)]
pub struct HolyNovaAssets {
    pub quad: Handle<Mesh>,
    pub star: Handle<Image>,
    pub ground: Handle<Image>,
}

impl HolyNovaAssets {
    pub fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            star: images.add(disc_texture(SPRITE_PX, |r, a| {
                ConflagrateSprite::Star.alpha_at(r, a)
            })),
            ground: images.add(oneshot_ground_texture()),
        }
    }
}

fn additive(
    materials: &mut Assets<StandardMaterial>,
    texture: Option<Handle<Image>>,
) -> Handle<StandardMaterial> {
    materials.add(conflagrate_material(true, Color::WHITE, 0.0, texture))
}

/// Build a nova's dome at `caster`, growing to `reach`: the whole rig, its
/// rings, ground glow and stars, at once. Called by the instant-ability
/// router (`instant_ability.rs`) off the Priest's caster-centred marker.
pub fn spawn_holy_nova_dome(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    assets: &mut Option<HolyNovaAssets>,
    caster: Entity,
    reach: f32,
) -> Entity {
    let assets = assets
        .get_or_insert_with(|| HolyNovaAssets::build(meshes, images))
        .clone();
    let piece = |commands: &mut Commands, piece: DomePiece, material: Handle<StandardMaterial>| {
        commands
            .spawn((
                piece,
                Mesh3d(assets.quad.clone()),
                MeshMaterial3d(material),
                Transform::from_scale(Vec3::splat(1e-4)),
                Visibility::Hidden,
                NotShadowCaster,
            ))
            .id()
    };
    let mut children = Vec::new();
    // One material a ring: its chords share its look.
    for ring in 0..DOME_RINGS {
        let material = additive(materials, None);
        for segment in 0..DOME_SEGMENTS {
            children.push(piece(
                commands,
                DomePiece::Ring { ring, segment },
                material.clone(),
            ));
        }
    }
    let ground = additive(materials, Some(assets.ground.clone()));
    children.push(piece(commands, DomePiece::Ground, ground));
    for (j, born) in nova_star_births().into_iter().enumerate() {
        let material = additive(materials, Some(assets.star.clone()));
        children.push(piece(
            commands,
            DomePiece::Star {
                j: j as u32,
                born,
                born_at: None,
            },
            material,
        ));
    }
    let rig = commands
        .spawn((Transform::default(), Visibility::default(), PlayMatchEntity))
        .id();
    commands
        .entity(rig)
        .insert(HolyNovaDome {
            caster,
            reach,
            age: 0.0,
            seed: rig.index(),
        })
        .add_children(&children);
    rig
}

/// Update (graphical-only): age each dome, and retire it after its life or at
/// once when its Priest is gone. The one owner of a dome's end.
pub fn animate_holy_nova_domes(
    mut commands: Commands,
    time: Res<Time>,
    mut rigs: Query<(Entity, &mut HolyNovaDome)>,
    units: Query<(), With<Combatant>>,
) {
    let dt = time.delta_secs();
    for (entity, mut rig) in rigs.iter_mut() {
        if units.get(rig.caster).is_err() {
            commands.entity(entity).despawn();
            continue;
        }
        rig.age += dt;
        if rig.age >= holy_nova_secs() {
            commands.entity(entity).despawn();
        }
    }
}

/// `BodyPlacement` (PostUpdate): stand each dome on its Priest's floor, and
/// place each piece where it is drawn — held in front of the Priest's
/// RENDERED body for the camera — sized and coloured for its age. The rig is
/// unrotated, so a piece's local pose is its world offset.
pub fn place_holy_nova_domes(
    hold: BodyHold,
    mut rigs: Query<(&HolyNovaDome, &mut Transform, &Children), (Without<DomePiece>, HoldDisjoint)>,
    mut pieces: Query<
        (
            &mut DomePiece,
            &mut Transform,
            &mut Visibility,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (Without<HolyNovaDome>, HoldDisjoint),
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let view = hold.view();
    let cam = view.map(|v| v.rotation).unwrap_or_default();
    for (rig, mut rig_transform, children) in rigs.iter_mut() {
        let Some(body) = hold.body(rig.caster) else {
            continue;
        };
        // A ground effect stands on the Priest's own floor, not on its bob.
        let floor = body_point(0.0, body.unit.translation, false);
        rig_transform.translation = floor;
        let capsule = body.capsule();
        let spread = nova_star_spread(rig.reach);
        for child in children.iter() {
            let Ok((mut piece, mut transform, mut visibility, material)) = pieces.get_mut(child)
            else {
                continue;
            };
            let drawn = match *piece {
                DomePiece::Ring { ring, segment } => {
                    dome_ring(ring, rig.age, rig.reach).map(|(height, radius, alpha)| {
                        let (from, to) = dome_chord(segment, radius);
                        let span = to - from;
                        let mid = floor + Vec3::Y * height + (from + to) * 0.5;
                        let (at, k) = held_in(view, mid, capsule);
                        (
                            at,
                            flat_along(span.try_normalize().unwrap_or(Vec3::X)),
                            Vec3::new(span.length(), DOME_LINE_WIDTH, 1.0) * k,
                            DOME_RGB,
                            alpha,
                        )
                    })
                }
                DomePiece::Ground => dome_ground(rig.age, rig.reach).map(|(radius, alpha)| {
                    let (at, k) = held_in(view, floor + Vec3::Y * DOME_FLOOR_LIFT, capsule);
                    (
                        at,
                        flat_along(Vec3::X),
                        Vec3::new(2.0 * radius, 2.0 * radius, 1.0) * k,
                        DOME_RGB,
                        alpha,
                    )
                }),
                DomePiece::Star {
                    j,
                    born,
                    ref mut born_at,
                } => match nova_star(j, born, rig.age, spread, rig.seed) {
                    Some(s) => {
                        let origin = *born_at.get_or_insert(floor);
                        let (at, k) = held_in(view, origin + s.at, capsule);
                        Some((
                            at,
                            cam * Quat::from_rotation_z(s.roll),
                            Vec3::new(s.size * k, s.size * k, 1.0),
                            s.rgb,
                            s.alpha,
                        ))
                    }
                    None => {
                        *born_at = None;
                        None
                    }
                },
            };
            match drawn {
                Some((at, rotation, scale, rgb, alpha)) => {
                    transform.translation = at - floor;
                    transform.rotation = rotation;
                    transform.scale = scale.max(Vec3::splat(1e-4));
                    visibility.set_if_neq(Visibility::Inherited);
                    set_additive_look(&mut materials, &material.0, rgb, alpha.clamp(0.0, 1.0));
                }
                None => {
                    transform.scale = Vec3::splat(1e-4);
                    visibility.set_if_neq(Visibility::Hidden);
                }
            }
        }
    }
}

/// Update (graphical-only): a star fall for every heal the sim landed, its
/// whole rig built at once. An ally that is gone gets nothing.
pub fn spawn_holy_star_falls(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<HolyNovaAssets>>,
    landings: Query<(Entity, &StarFallLanding)>,
    units: Query<(), With<Combatant>>,
) {
    for (marker, landing) in landings.iter() {
        commands.entity(marker).despawn();
        if units.get(landing.target).is_err() || !HOLY_NOVA_STAR_FALL {
            continue;
        }
        let assets = assets
            .get_or_insert_with(|| HolyNovaAssets::build(&mut meshes, &mut images))
            .clone();
        let mut children = Vec::new();
        let glow = additive(&mut materials, Some(assets.ground.clone()));
        children.push((FallPiece::Glow, glow));
        for k in 0..FALL_STARS {
            // `customDraw`'s own `sprite()`: composited source-over.
            let material = materials.add(conflagrate_material(
                false,
                Color::WHITE,
                0.0,
                Some(assets.star.clone()),
            ));
            children.push((FallPiece::Star(k), material));
        }
        let children: Vec<Entity> = children
            .into_iter()
            .map(|(piece, material)| {
                commands
                    .spawn((
                        piece,
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(material),
                        Transform::from_scale(Vec3::splat(1e-4)),
                        Visibility::Hidden,
                        NotShadowCaster,
                    ))
                    .id()
            })
            .collect();
        commands
            .spawn((
                HolyStarFall {
                    target: landing.target,
                    age: 0.0,
                },
                Transform::default(),
                Visibility::default(),
                PlayMatchEntity,
            ))
            .add_children(&children);
    }
}

/// Update (graphical-only): age each fall, and retire it after its life or at
/// once when its ally is gone. The one owner of a fall's end.
pub fn animate_holy_star_falls(
    mut commands: Commands,
    time: Res<Time>,
    mut rigs: Query<(Entity, &mut HolyStarFall)>,
    units: Query<(), With<Combatant>>,
) {
    let dt = time.delta_secs();
    for (entity, mut rig) in rigs.iter_mut() {
        if units.get(rig.target).is_err() {
            commands.entity(entity).despawn();
            continue;
        }
        rig.age += dt;
        if rig.age >= star_fall_secs() {
            commands.entity(entity).despawn();
        }
    }
}

/// `BodyPlacement` (PostUpdate): stand each fall on its ally's RENDERED feet,
/// and place each piece where it is drawn — held in front of that body for
/// the camera — sized and coloured for its age.
pub fn place_holy_star_falls(
    hold: BodyHold,
    mut rigs: Query<(&HolyStarFall, &mut Transform, &Children), (Without<FallPiece>, HoldDisjoint)>,
    mut pieces: Query<
        (
            &FallPiece,
            &mut Transform,
            &mut Visibility,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (Without<HolyStarFall>, HoldDisjoint),
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let view = hold.view();
    let cam = view.map(|v| v.rotation).unwrap_or_default();
    for (rig, mut rig_transform, children) in rigs.iter_mut() {
        let Some(body) = hold.body(rig.target) else {
            continue;
        };
        let feet = body.point(0.0);
        rig_transform.translation = feet;
        let capsule = body.capsule();
        for child in children.iter() {
            let Ok((piece, mut transform, mut visibility, material)) = pieces.get_mut(child) else {
                continue;
            };
            let drawn = match *piece {
                FallPiece::Glow => fall_glow(rig.age).map(|(radius, alpha)| {
                    let (at, k) = held_in(view, feet + Vec3::Y * DOME_FLOOR_LIFT, capsule);
                    (
                        at,
                        flat_along(Vec3::X),
                        Vec3::new(2.0 * radius, 2.0 * radius, 1.0) * k,
                        FALL_GLOW_RGB,
                        alpha,
                        true,
                    )
                }),
                FallPiece::Star(k) => fall_star(k, rig.age).map(|s| {
                    let (at, k) = held_in(view, feet + s.at, capsule);
                    (
                        at,
                        cam * Quat::from_rotation_z(s.roll),
                        Vec3::new(s.size * k, s.size * k, 1.0),
                        s.rgb,
                        s.alpha,
                        false,
                    )
                }),
            };
            match drawn {
                Some((at, rotation, scale, rgb, alpha, add)) => {
                    transform.translation = at - feet;
                    transform.rotation = rotation;
                    transform.scale = scale.max(Vec3::splat(1e-4));
                    visibility.set_if_neq(Visibility::Inherited);
                    let alpha = alpha.clamp(0.0, 1.0);
                    if add {
                        set_additive_look(&mut materials, &material.0, rgb, alpha);
                    } else {
                        let color = Color::srgb_u8(rgb[0], rgb[1], rgb[2]).with_alpha(alpha);
                        if materials
                            .get(&material.0)
                            .is_some_and(|m| m.base_color != color)
                        {
                            if let Some(m) = materials.get_mut(&material.0) {
                                m.base_color = color;
                            }
                        }
                    }
                }
                None => {
                    transform.scale = Vec3::splat(1e-4);
                    visibility.set_if_neq(Visibility::Hidden);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 191 trailing stars; the dome is gone at 767 ms and its last star
    /// about a second after.
    #[test]
    fn the_nova_bears_the_benchs_stars() {
        let births = nova_star_births();
        assert_eq!(births.len(), 191);
        assert!(births.iter().all(|b| (0.0..DOME_SECS).contains(b)));
        assert!((holy_nova_secs() - (births[190] + 1.0)).abs() < 1e-6);
        assert!((star_fall_secs() - 2.65).abs() < 1e-6);
    }

    #[test]
    fn a_flat_quad_faces_up_and_runs_along_its_direction() {
        for along in [
            Vec3::X,
            Vec3::Z,
            -Vec3::X,
            Vec3::new(1.0, 0.0, -1.0).normalize(),
        ] {
            let q = flat_along(along);
            assert!((q * Vec3::Z - Vec3::Y).length() < 1e-5, "{along}");
            assert!((q * Vec3::X - along).length() < 1e-5, "{along}");
        }
    }
}
