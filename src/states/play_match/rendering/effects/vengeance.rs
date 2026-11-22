//! Vengeance — both of the Paladin's hands glow pale blue for the buff's life
//! (AS-377).
//!
//! From the client data joined in
//! `docs/design/2026-10-18-paladin-talent-client-data.md` §11 (the buff
//! 20050–20055, SpellVisual 6597, the same at Era, TBC and Wrath): the `(7,8)`
//! state kit 5898 holds `vengeance_state_hand.m2` on BOTH hands (attachments
//! 21 and 22) for the buff's whole life, a 1.0 s loop: a small blue quad, soft
//! white-to-pale-gold glows and sky-blue sparks on each hand. A melee crit
//! refreshes the buff, so a Retribution Paladin in melee wears it nearly all
//! match.
//!
//! Played as the signed-off AS-337 Paladin Talent Visuals Bench draws it
//! (https://claude.ai/artifact/31rnhhKpACF51iUsqK5g7k, version
//! 1792379243-e3d6, "Vengeance" tab, committed as
//! `docs/design/benches/2026-10-18-paladin-talent-bench.html`): every layer
//! on, at the client's values (`VENGEANCE_SIZE_MUL` and
//! `VENGEANCE_OPACITY_MUL` 1.0), shown for the whole buff (the ruling; the
//! bench's other mode, 1.5 s on each proc, was not built). The pieces below
//! are the bench's, one for one: its `customDraw` (a camera-facing blue glow
//! and a small level ring on each hand) and its two emitters (the glows and
//! the sparks).
//!
//! **The particles are a fixed pool.** Each emitter's rate times its life is
//! a whole number (15/s × 0.8 s = 12 glows, 50/s × 0.4 s = 20 sparks), so a
//! hand emitting at its exact rate holds exactly that many particles once it
//! has warmed up, and particle `j` (born `j / rate` seconds after the onset)
//! can take slot `j mod N`, just vacated by particle `j − N`. Every piece is
//! therefore a pure function of the rig's age ([`hand_piece_pose`]): no
//! entity churn, nothing remembered between frames, and the probes read the
//! same definition the placement draws. The scatter is the bench's own hash
//! ([`bench_hash`]); no `game_rng` is drawn.
//!
//! **Lifecycle.** Detected renderer-side off `ActiveAuras`, like Divine
//! Protection's globe and the shields: a living unit holding Vengeance wears
//! one rig, raised on the first frame it holds the buff and taken down on the
//! frame it no longer does — run out, purged, its bearer dead (auras stay on a
//! corpse) or gone. A crit refreshes the aura in place (`apply_pending_auras`,
//! refresh-only stacking), so the rig keeps its age: the glow just continues,
//! with no new onset. The bench draws no envelope, so neither is there one
//! here: the quad is on from the onset and gone with the buff. A polymorphed
//! bearer's hands are hidden with its weapons (the sheep has no hands).
//!
//! **Anchor.** Each hand rides the bearer's RENDERED body (its `VisualBody`:
//! walk bob, the swing's lean and step, the Cyclone lift) at the spell-hand
//! socket every hand effect uses ([`spell_hand_local`], the weapon grip
//! line), placed in `PostUpdate` after every writer of the body, as the
//! shield cards are. The rig is never rotated, so a piece's local pose is its
//! world one, and the particles ride the hand they were born on, as the card
//! asks ("riding the hands through the swing").
//!
//! Graphical-only: nothing here writes sim state, and every system is
//! registered in `states/mod.rs` only. Headless never runs any of it.

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::druid_heals::keyed;
use super::heal_cast::spell_hand_local;
use super::paladin_oneshots::{
    bench_hash, oneshot_glow_texture, oneshot_ring_texture, piece_rotation, PieceFacing, PiecePose,
    RING_STROKE,
};
use super::shield::disc_texture;
use crate::states::play_match::components::*;

// ── The bench's constants block (signed off at version 1792379243-e3d6) ─────

// Vengeance (visual 6597): kit 5898 vengeance_state_hand.m2 @Hands (21, 22)
// for the buff, 1.0 s loop. AS-337 bench. Every layer on, the whole buff.
pub const VENGEANCE_BLUE_QUAD: bool = true;
pub const VENGEANCE_GLOWS: bool = true;
pub const VENGEANCE_SPARKS: bool = true;
pub const VENGEANCE_SIZE_MUL: f32 = 1.0;
pub const VENGEANCE_OPACITY_MUL: f32 = 1.0;
// AuraApplyRoute: owned by the hands (by name), not the band.

// ── The blue quad (`customDraw`) ─────────────────────────────────────────────

/// The client's `whiteringfat128` quad, blue (0.37, 0.74, 1.0) at weight 0.8,
/// as the bench draws it: a camera-facing glow 2 × 0.37 yd across...
pub const QUAD_SIZE: f32 = 2.0 * 0.37;
pub const QUAD_RGB: [u8; 3] = [94, 189, 255];
/// ...at the weight times 0.8 (`bbGlow(..., a * 0.8)` with `a = 0.8`)...
pub const QUAD_WEIGHT: f32 = 0.8;
pub const QUAD_ALPHA: f32 = QUAD_WEIGHT * 0.8;
/// ...and a level ring of radius 0.12 yd round the hand at the weight.
pub const RING_RADIUS: f32 = 0.12;
pub const RING_RGB: [u8; 3] = [200, 230, 255];
pub const RING_ALPHA: f32 = QUAD_WEIGHT;

// ── The emitters ─────────────────────────────────────────────────────────────

/// `shockwave10b` glows: 15/s on each hand, each living 0.8 s, born on the
/// hand and still.
pub const GLOW_RATE: f32 = 15.0;
pub const GLOW_LIFE: f32 = 0.8;
/// The pool a hand's glows fill: `GLOW_RATE × GLOW_LIFE`.
pub const GLOW_SLOTS: u8 = 12;
/// Diameter over the particle's life, yards (the bench's scale 0.2 → 0.3,
/// drawn at twice its value).
pub const GLOW_DIAMETER: [(f32, f32); 2] = [(0.0, 0.4), (1.0, 0.6)];
/// White → pale gold → white, over the particle's life.
pub const GLOW_RGB: [(f32, [u8; 3]); 3] = [
    (0.0, [255, 255, 255]),
    (0.65, [255, 244, 180]),
    (1.0, [255, 255, 255]),
];

/// `flare` sparks: 50/s on each hand, each living 0.4 s, born in a box 0.2 yd
/// across and 0.28 yd tall round the hand and flying out at 0.44 yd/s.
pub const SPARK_RATE: f32 = 50.0;
pub const SPARK_LIFE: f32 = 0.4;
/// The pool a hand's sparks fill: `SPARK_RATE × SPARK_LIFE`.
pub const SPARK_SLOTS: u8 = 20;
pub const SPARK_SPEED: f32 = 0.44;
/// The birth box: `(across, tall)`, yards (the bench's `area`, `areaY`).
pub const SPARK_BOX: (f32, f32) = (0.2, 0.28);
/// Diameter over the particle's life, yards (scale 0.08 → 0.05).
pub const SPARK_DIAMETER: [(f32, f32); 2] = [(0.0, 0.16), (1.0, 0.10)];
/// White → sky blue → white.
pub const SPARK_RGB: [(f32, [u8; 3]); 3] = [
    (0.0, [255, 255, 255]),
    (0.65, [75, 177, 255]),
    (1.0, [255, 255, 255]),
];

/// Both emitters' alpha over the particle's life: in to full at 0.65, out by
/// the end.
pub const PARTICLE_ALPHA: [(f32, f32); 3] = [(0.0, 0.0), (0.65, 1.0), (1.0, 0.0)];

/// The bench's `sprite('glow')`: a soft radial glow, 1 at its centre, 0.45 at
/// 0.45 of its radius, 0 at its edge.
pub const SPRITE_GLOW_STOPS: [(f32, f32); 3] = [(0.0, 1.0), (0.45, 0.45), (1.0, 0.0)];

// ── Routing ──────────────────────────────────────────────────────────────────

/// Whether a unit holds Vengeance.
pub fn holds_vengeance(auras: &ActiveAuras) -> bool {
    auras
        .auras
        .iter()
        .any(|a| is_vengeance(a.effect_type, &a.ability_name))
}

// ── The pieces ───────────────────────────────────────────────────────────────

/// Which hand: the main hand (`+1`, the weapon's side) or the off hand (`-1`).
pub const HAND_SIDES: [f32; 2] = [1.0, -1.0];

/// One drawn piece of a hand.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Hash)]
pub enum HandPiece {
    /// The blue quad's camera-facing glow.
    Quad,
    /// Its level ring.
    Ring,
    /// One slot of the glow emitter's pool.
    Glow { slot: u8 },
    /// One slot of the spark emitter's pool.
    Spark { slot: u8 },
}

/// The sprite a piece is drawn with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandSprite {
    /// The bench's `bbGlow` (the blue quad).
    Quad,
    /// A thin ring (`ring3`).
    Ring,
    /// The bench's `sprite('glow')` (both emitters).
    Particle,
}

impl HandPiece {
    pub fn sprite(self) -> HandSprite {
        match self {
            HandPiece::Quad => HandSprite::Quad,
            HandPiece::Ring => HandSprite::Ring,
            HandPiece::Glow { .. } | HandPiece::Spark { .. } => HandSprite::Particle,
        }
    }
}

/// The pieces one hand draws, by its layer toggles.
pub fn hand_pieces() -> Vec<HandPiece> {
    let mut pieces = Vec::new();
    if VENGEANCE_BLUE_QUAD {
        pieces.extend([HandPiece::Quad, HandPiece::Ring]);
    }
    if VENGEANCE_GLOWS {
        pieces.extend((0..GLOW_SLOTS).map(|slot| HandPiece::Glow { slot }));
    }
    if VENGEANCE_SPARKS {
        pieces.extend((0..SPARK_SLOTS).map(|slot| HandPiece::Spark { slot }));
    }
    pieces
}

/// The particle in `slot` of a pool of `slots` filled at `rate`, `age`
/// seconds after the onset: its birth index `j` (born at `j / rate`) and its
/// own age, or `None` while the slot has had no birth yet. Particle `j` takes
/// slot `j mod slots`, and its successor is born `slots / rate` (its life)
/// after it, so the slot always holds the latest birth.
pub fn slot_particle(slot: u8, slots: u8, rate: f32, age: f32) -> Option<(u32, f32)> {
    let (slot, slots) = (slot as u32, slots as u32);
    let born = (age.max(0.0) * rate).floor() as u32;
    if born < slot {
        return None;
    }
    let j = born - (born - slot) % slots;
    Some((j, (age - j as f32 / rate).max(0.0)))
}

/// A colour track at `t`, interpolated in sRGB bytes and truncated, as the
/// bench's canvas does (`rgb()`'s `|0`).
fn track_rgb(track: &[(f32, [u8; 3])], t: f32) -> [u8; 3] {
    let channel = |c: usize| {
        let keys: Vec<(f32, f32)> = track.iter().map(|(t, v)| (*t, v[c] as f32)).collect();
        keyed(&keys, t).floor().clamp(0.0, 255.0) as u8
    };
    [channel(0), channel(1), channel(2)]
}

/// The bench's scatter for particle `j` of `layer` (0 glows, 1 sparks) on
/// hand `hand` (0 main, 1 off), draw `k`: 0..1.
fn scatter(hand: u32, layer: u32, j: u32, k: u32) -> f32 {
    bench_hash(
        j.wrapping_mul(4)
            .wrapping_add(hand * 2 + layer)
            .wrapping_mul(8)
            .wrapping_add(k),
    )
}

/// Spark `j` of `hand`'s birth point, relative to the hand: uniform in the
/// [`SPARK_BOX`] (the bench's `(rnd − 0.5) × area`).
pub fn spark_birth(hand: u32, j: u32) -> Vec3 {
    let (across, tall) = SPARK_BOX;
    Vec3::new(
        (scatter(hand, 1, j, 1) - 0.5) * across,
        (scatter(hand, 1, j, 2) - 0.5) * tall,
        (scatter(hand, 1, j, 3) - 0.5) * across,
    )
}

/// Spark `j` of `hand`'s direction: the bench's `vRange` 3.14, a random point
/// of the cube `[-1, 1]³` normalised.
pub fn spark_direction(hand: u32, j: u32) -> Vec3 {
    let d = Vec3::new(
        scatter(hand, 1, j, 4) * 2.0 - 1.0,
        scatter(hand, 1, j, 5) * 2.0 - 1.0,
        scatter(hand, 1, j, 6) * 2.0 - 1.0,
    );
    d.try_normalize().unwrap_or(Vec3::Y)
}

/// A piece's pose on `hand` (0 main, 1 off) `age` seconds after the onset:
/// its position relative to the hand, its size (a quad's full width and
/// height), facing, colour and alpha — the size and opacity knobs applied.
/// Pure: the probes and the placement read one definition.
pub fn hand_piece_pose(piece: HandPiece, age: f32, hand: u32) -> PiecePose {
    let camera = PieceFacing::Camera;
    let particle = |rgb: [u8; 3], alpha: f32, at: Vec3, diameter: f32| PiecePose {
        at,
        size: Vec2::splat(diameter),
        facing: camera,
        rgb,
        alpha,
    };
    let pose = match piece {
        HandPiece::Quad => particle(QUAD_RGB, QUAD_ALPHA, Vec3::ZERO, QUAD_SIZE),
        HandPiece::Ring => PiecePose {
            at: Vec3::ZERO,
            // The ring sprite's stroke runs at RING_STROKE.0 of the quad's
            // half-width, so the quad is sized to put it at the radius.
            size: Vec2::splat(2.0 * RING_RADIUS / RING_STROKE.0),
            facing: PieceFacing::Flat { turn: 0.0 },
            rgb: RING_RGB,
            alpha: RING_ALPHA,
        },
        HandPiece::Glow { slot } => match slot_particle(slot, GLOW_SLOTS, GLOW_RATE, age) {
            Some((_, life)) => {
                let t = life / GLOW_LIFE;
                particle(
                    track_rgb(&GLOW_RGB, t),
                    keyed(&PARTICLE_ALPHA, t),
                    Vec3::ZERO,
                    keyed(&GLOW_DIAMETER, t),
                )
            }
            None => particle(GLOW_RGB[0].1, 0.0, Vec3::ZERO, GLOW_DIAMETER[0].1),
        },
        HandPiece::Spark { slot } => match slot_particle(slot, SPARK_SLOTS, SPARK_RATE, age) {
            Some((j, life)) => {
                let t = life / SPARK_LIFE;
                particle(
                    track_rgb(&SPARK_RGB, t),
                    keyed(&PARTICLE_ALPHA, t),
                    spark_birth(hand, j) + spark_direction(hand, j) * SPARK_SPEED * life,
                    keyed(&SPARK_DIAMETER, t),
                )
            }
            None => particle(SPARK_RGB[0].1, 0.0, Vec3::ZERO, SPARK_DIAMETER[0].1),
        },
    };
    PiecePose {
        at: pose.at * VENGEANCE_SIZE_MUL,
        size: pose.size * VENGEANCE_SIZE_MUL,
        alpha: pose.alpha * VENGEANCE_OPACITY_MUL,
        ..pose
    }
}

// ── Assets ───────────────────────────────────────────────────────────────────

/// Generated sprite textures, pixels on a side.
const SPRITE_PX: u32 = 128;

/// The bench's `sprite('glow')`: [`SPRITE_GLOW_STOPS`] over the sprite's
/// radius, nothing beyond it.
pub fn vengeance_particle_texture() -> Image {
    disc_texture(SPRITE_PX, |r, _| keyed(&SPRITE_GLOW_STOPS, r))
}

/// The mesh and the textures every hand draws with, built once.
pub struct VengeanceAssets {
    quad: Handle<Mesh>,
    glow: Handle<Image>,
    ring: Handle<Image>,
    particle: Handle<Image>,
}

impl VengeanceAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            glow: images.add(oneshot_glow_texture()),
            ring: images.add(oneshot_ring_texture()),
            particle: images.add(vengeance_particle_texture()),
        }
    }

    fn texture(&self, sprite: HandSprite) -> Handle<Image> {
        match sprite {
            HandSprite::Quad => self.glow.clone(),
            HandSprite::Ring => self.ring.clone(),
            HandSprite::Particle => self.particle.clone(),
        }
    }
}

// ── Components ───────────────────────────────────────────────────────────────

/// One bearer's Vengeance: a rig at its feet, parent of its two hands. Lives
/// exactly as long as Vengeance is on a living bearer.
#[derive(Component, Debug)]
pub struct VengeanceHands {
    pub bearer: Entity,
    /// Seconds since the buff's onset (a refresh does not reset it).
    pub age: f32,
}

/// One hand of a [`VengeanceHands`] rig, parent of its pieces.
#[derive(Component, Debug)]
pub struct VengeanceHand {
    /// `+1` the main hand, `-1` the off hand ([`HAND_SIDES`]).
    pub side: f32,
}

impl VengeanceHand {
    /// The hand's index for the scatter: 0 main, 1 off.
    pub fn index(&self) -> u32 {
        u32::from(self.side < 0.0)
    }
}

/// One piece of a [`VengeanceHand`].
#[derive(Component, Debug)]
pub struct VengeanceHandPiece {
    pub piece: HandPiece,
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// Update (graphical-only): raise a rig on every living unit holding
/// Vengeance, age each rig, hide it while its bearer is a sheep, and take it
/// down when the buff ends (run out or purged) or its bearer dies or is gone.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_vengeance_hands(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<VengeanceAssets>>,
    bearers: Query<(
        Entity,
        &Combatant,
        &Transform,
        &ActiveAuras,
        Has<PolymorphedVisual>,
    )>,
    mut rigs: Query<(Entity, &mut VengeanceHands, &mut Visibility)>,
) {
    let dt = time.delta_secs();
    let held = |entity: Entity| {
        bearers
            .get(entity)
            .ok()
            .filter(|(_, combatant, _, auras, _)| combatant.is_alive() && holds_vengeance(auras))
            .map(|(_, _, _, _, sheep)| sheep)
    };
    let hidden = |sheep: bool| {
        if sheep {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        }
    };
    let mut worn = Vec::new();
    for (rig, mut hands, mut visibility) in rigs.iter_mut() {
        match held(hands.bearer) {
            Some(sheep) => {
                hands.age += dt;
                visibility.set_if_neq(hidden(sheep));
                worn.push(hands.bearer);
            }
            None => commands.entity(rig).despawn(),
        }
    }
    for (entity, _, transform, _, _) in bearers.iter() {
        let Some(sheep) = held(entity) else {
            continue;
        };
        if worn.contains(&entity) {
            continue;
        }
        let assets = assets.get_or_insert_with(|| VengeanceAssets::build(&mut meshes, &mut images));
        let rig = commands
            .spawn((
                VengeanceHands {
                    bearer: entity,
                    age: 0.0,
                },
                Transform::from_translation(transform.translation),
                hidden(sheep),
                PlayMatchEntity,
            ))
            .id();
        for side in HAND_SIDES {
            let hand = commands
                .spawn((
                    VengeanceHand { side },
                    Transform::from_translation(spell_hand_local(side)),
                    Visibility::Inherited,
                ))
                .id();
            commands.entity(rig).add_child(hand);
            for piece in hand_pieces() {
                let pose = hand_piece_pose(piece, 0.0, VengeanceHand { side }.index());
                let material = materials.add(StandardMaterial {
                    base_color: Color::srgb_u8(pose.rgb[0], pose.rgb[1], pose.rgb[2])
                        .with_alpha(0.0),
                    base_color_texture: Some(assets.texture(piece.sprite())),
                    // The bench adds colour × sprite × alpha and nothing else.
                    unlit: true,
                    alpha_mode: AlphaMode::Add,
                    cull_mode: None,
                    double_sided: true,
                    ..default()
                });
                let child = commands
                    .spawn((
                        VengeanceHandPiece { piece },
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(material),
                        Transform::from_scale(Vec3::splat(1e-3)),
                        NotShadowCaster,
                    ))
                    .id();
                commands.entity(hand).add_child(child);
            }
        }
    }
}

/// Where a bearer's `side` hand stands in the world: the spell-hand socket
/// on its rendered body (`body`, its `VisualBody`'s local transform).
pub fn vengeance_hand_point(bearer: &Transform, body: &Transform, side: f32) -> Vec3 {
    bearer.transform_point(body.transform_point(spell_hand_local(side)))
}

/// PostUpdate (graphical-only), after every writer of a body's transform and
/// before propagation: stand each rig at its bearer's feet and each hand on
/// its rendered spell hand, and pose, turn and colour the pieces. Nothing in
/// a rig is rotated, so a piece's local pose is its world one.
#[allow(clippy::type_complexity)]
pub fn place_vengeance_hands(
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<VengeanceHands>,
            Without<VengeanceHand>,
            Without<VengeanceHandPiece>,
        ),
    >,
    mut rigs: Query<
        (&VengeanceHands, &mut Transform, &Children),
        (
            Without<Combatant>,
            Without<VisualBody>,
            Without<VengeanceHand>,
            Without<VengeanceHandPiece>,
        ),
    >,
    bearers: Query<
        (&Transform, Option<&Children>),
        (
            With<Combatant>,
            Without<VengeanceHands>,
            Without<VengeanceHand>,
            Without<VengeanceHandPiece>,
        ),
    >,
    bodies: Query<
        &Transform,
        (
            With<VisualBody>,
            Without<Combatant>,
            Without<VengeanceHands>,
            Without<VengeanceHand>,
            Without<VengeanceHandPiece>,
        ),
    >,
    mut hands: Query<
        (&VengeanceHand, &mut Transform, &Children),
        (
            Without<Combatant>,
            Without<VisualBody>,
            Without<VengeanceHands>,
            Without<VengeanceHandPiece>,
        ),
    >,
    mut pieces: Query<
        (
            &VengeanceHandPiece,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (
            Without<Combatant>,
            Without<VisualBody>,
            Without<VengeanceHands>,
            Without<VengeanceHand>,
        ),
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let cam = camera.iter().next().map(|c| c.rotation);
    for (rig, mut rig_transform, rig_children) in rigs.iter_mut() {
        let Ok((bearer, bearer_children)) = bearers.get(rig.bearer) else {
            continue;
        };
        let body = bearer_children
            .and_then(|kids| kids.iter().find_map(|k| bodies.get(k).ok()))
            .copied()
            .unwrap_or_default();
        rig_transform.translation = bearer.translation;
        for hand_entity in rig_children.iter() {
            let Ok((hand, mut hand_transform, hand_children)) = hands.get_mut(hand_entity) else {
                continue;
            };
            hand_transform.translation =
                vengeance_hand_point(bearer, &body, hand.side) - bearer.translation;
            for child in hand_children.iter() {
                let Ok((piece, mut part, material)) = pieces.get_mut(child) else {
                    continue;
                };
                let pose = hand_piece_pose(piece.piece, rig.age, hand.index());
                part.translation = pose.at;
                part.scale = pose.size.extend(1.0).max(Vec3::splat(1e-3));
                part.rotation = piece_rotation(pose.facing, Quat::IDENTITY, cam);
                let color = Color::srgb_u8(pose.rgb[0], pose.rgb[1], pose.rgb[2])
                    .with_alpha(pose.alpha.clamp(0.0, 1.0));
                // Only touch the asset when the look moved: `get_mut` marks it
                // changed and re-uploads it.
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constants block, as the signed-off bench exports it, and the
    /// layers' numbers as its `customDraw` and emitters carry them.
    #[test]
    fn the_constants_are_the_benchs() {
        assert_eq!(VENGEANCE_SIZE_MUL, 1.0);
        assert_eq!(VENGEANCE_OPACITY_MUL, 1.0);
        assert!((QUAD_SIZE - 0.74).abs() < 1e-6);
        assert!((QUAD_ALPHA - 0.64).abs() < 1e-6);
        assert_eq!(RING_RADIUS, 0.12);
        assert_eq!(RING_ALPHA, 0.8);
        assert_eq!((GLOW_RATE, GLOW_LIFE), (15.0, 0.8));
        assert_eq!((SPARK_RATE, SPARK_LIFE, SPARK_SPEED), (50.0, 0.4, 0.44));
    }

    /// Each pool holds exactly rate × life, so a slot is free the moment its
    /// next particle is born.
    #[test]
    fn each_pool_is_its_emitters_rate_times_its_life() {
        assert_eq!((GLOW_RATE * GLOW_LIFE).round(), GLOW_SLOTS as f32);
        assert!((GLOW_RATE * GLOW_LIFE - GLOW_SLOTS as f32).abs() < 1e-5);
        assert!((SPARK_RATE * SPARK_LIFE - SPARK_SLOTS as f32).abs() < 1e-5);
    }

    /// A slot holds the latest particle born into it, whose age is always
    /// short of the life; a slot not yet reached holds none.
    #[test]
    fn a_slot_holds_its_latest_birth() {
        assert_eq!(slot_particle(0, 12, 15.0, 0.0), Some((0, 0.0)));
        assert_eq!(slot_particle(3, 12, 15.0, 0.1), None, "born at 0.2 s");
        let (j, life) = slot_particle(3, 12, 15.0, 0.21).unwrap();
        assert_eq!(j, 3);
        assert!((life - 0.01).abs() < 1e-5);
        // 2.0 s in: births 0..=30, slot 3 holds 27 (born 1.8 s).
        let (j, life) = slot_particle(3, 12, 15.0, 2.0).unwrap();
        assert_eq!(j, 27);
        assert!((life - 0.2).abs() < 1e-5);
        for k in 0..2000 {
            let age = k as f32 * 0.0137;
            for slot in 0..12 {
                if let Some((j, life)) = slot_particle(slot, 12, 15.0, age) {
                    assert_eq!(j % 12, slot as u32);
                    assert!((0.0..0.8 + 1e-4).contains(&life), "{age} {slot} {life}");
                }
            }
        }
    }

    /// The particle sprite: 1 at the centre, 0.45 at 0.45 of the radius,
    /// nothing at the edge or beyond it.
    #[test]
    fn the_particle_sprite_is_the_benchs_glow() {
        let image = vengeance_particle_texture();
        let px = SPRITE_PX as usize;
        let alpha_at = |x: f32| {
            let col = (((x + 1.0) / 2.0) * px as f32) as usize;
            image.data.as_ref().unwrap()[((px / 2) * px + col.min(px - 1)) * 4 + 3]
        };
        assert!(alpha_at(0.0) > 250);
        assert!((110..=120).contains(&alpha_at(0.45)), "{}", alpha_at(0.45));
        assert!(alpha_at(0.999) <= 3, "{}", alpha_at(0.999));
    }
}
