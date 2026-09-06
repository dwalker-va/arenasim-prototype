use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use std::f32::consts::TAU;

use super::heal_impact::{emissive_of, heal_jitter, holy_gold_pale, nature_green};
use super::spell_bolts::{soft_dot_texture, star_flash_texture};
use crate::states::play_match::components::*;

// ==============================================================================
// Cast-side heal visuals — the caster's glowing spell hands (graphical-only)
// ==============================================================================
//
// The caster-side counterpart of `heal_impact.rs`. Until this module every
// hard cast — heals included — telegraphed as the same generic gathering orb.
// The Classic client shows something entirely different for heals (measured
// verdict: `docs/design/2026-09-06-cast-side-heal-client-data.md`, wago.tools
// DB2 + M2 parsing, build 1.15.9.69547): the whole cast-side show lives on the
// caster's TWO SPELL HANDS (attach 21/22, always both, pure symmetric
// attachment), in two family dressings over one shared skeleton:
//
// - **Holy** (Priest + Paladin: Flash Heal, Holy Light, Flash of Light) —
//   `holy_precast_low_hand.m2` per hand: a camera-facing two-layer gold glow
//   ball (0.88 + 0.54 u quads) with three wide (0.33 u) gold ribbon wisps
//   swirling around it on a 1 s cycle. The launch (kit 270) is literally the
//   SAME model replayed as a one-shot — a brief re-flare of the identical
//   glow-and-wisps. Holy has no dedicated launch vocabulary.
// - **Nature** (Shaman: Lesser Healing Wave) — `nature_precast_low_hand.m2`:
//   the same swirl skeleton re-dressed green and smaller (0.76 + 0.36 u
//   glows, 0.10 u star-thread wisps) plus a lazy shed of leaves (11/s/hand,
//   0.086 u/s omni drift, 0.8 s life). Nature spends its novelty budget on a
//   dedicated launch model instead: `nature_cast_hand.m2`, a ~0.3 s tight
//   (~2°) jet of water-ring shockwaves (21.4/s) and gold glow sparks.
//
// Timing is event-edge-driven in the source: the precast loop lives from the
// cast-start edge to the cast-end edge — its duration IS the actual cast
// time, decided by gameplay, never a visual constant — and an interrupted
// cast fires the end edge without the launch edge, so the hand glow and body
// loop simply STOP DEAD. No interrupt flourish exists, and none is added.
//
// The body language is the omni cast pair (52 ReadySpellOmni looping, then
// 54 SpellCastOmni at launch), approximated on the limbless capsule by
// `update_heal_cast_posture`: a slight hands-raised back-lean through the
// loop and a forward release surge at launch.
//
// Everything here is additive (every material and emitter in the source set
// is M2 blend 4), rides the `VisualBody` rigidly (rig = child at the spell-
// hand socket, so walk bob / posture / facing carry it with zero per-frame
// sim reads — the fixed-timestep strobe lesson), and draws no `game_rng`
// (scatter is position-hashed off a monotonic counter).

// --- The spell-hand sockets ---------------------------------------------------

/// Where a spell hand sits, local to the [`VisualBody`] (capsule radius 0.5,
/// height 2.5). X matches the weapon-mount hand line (`weapon_mount` in
/// `play_match/mod.rs` puts every grip at 0.7–0.8 |x|; the Paladin's mace —
/// the one weapon a heal caster in this set actually holds — grips at 0.72),
/// so the glow lights up where the hands already demonstrably are. Z presents
/// the hands slightly forward of the grips — raised toward the target, the
/// ReadySpellOmni read. Clears the 0.5 capsule radius with margin, like every
/// weapon mount.
pub const SPELL_HAND_X: f32 = 0.72;
pub const SPELL_HAND_Y: f32 = 0.05;
pub const SPELL_HAND_Z: f32 = 0.30;

/// The socket for one hand; `side` is +1 (main-hand side) or -1 (off).
pub fn spell_hand_local(side: f32) -> Vec3 {
    Vec3::new(SPELL_HAND_X * side, SPELL_HAND_Y, SPELL_HAND_Z)
}

// --- Blessed defaults (workshop) ---------------------------------------------

/// Global multiplier on the glow quad sizes.
pub const GLOW_SCALE: f32 = 1.0;
/// Holy glow quad sizes per hand (outer soft, brighter core), yards.
pub const HOLY_CAST_GLOW_SIZES: [f32; 2] = [0.88, 0.54];
/// Nature glow quad sizes per hand, yards.
pub const NATURE_CAST_GLOW_SIZES: [f32; 2] = [0.76, 0.36];
/// Width of a Holy ribbon wisp (source half-widths 0.167 -> 0.33 u full).
pub const HOLY_CAST_WISP_WIDTH: f32 = 0.33;
/// Width of a Nature star-thread wisp (source 0.08–0.11 u; blessed 0.10).
pub const NATURE_CAST_WISP_WIDTH: f32 = 0.10;
/// The three wisps' trail lifetimes, seconds (source edge lifetimes).
pub const HEAL_CAST_WISP_LIFETIMES: [f32; 3] = [0.40, 0.60, 0.50];
/// Orbit rate of the wisp swirl, cycles per second (the model's 1000 ms
/// loop). Shared by both families — same rig template in the source.
pub const HOLY_CAST_ORBIT_HZ: f32 = 1.0;
/// Orbit radius of the wisp heads around the hand (source pivots ~0.2 u out).
pub const HEAL_CAST_WISP_ORBIT_RADIUS: f32 = 0.20;

/// Holy launch: the one-shot re-flare of the SAME glow, and how hard it hits.
pub const HOLY_LAUNCH_FLARE_INTENSITY: f32 = 1.0;
pub const HOLY_LAUNCH_FLARE_SECS: f32 = 0.45;
/// Flash of Light carries no cast kit in the source (its cast, uniquely,
/// just stops). The blessed call overrides that: FoL launches with the
/// standard Holy re-flare. Flip to `false` to restore the source's silence.
pub const FLASH_OF_LIGHT_HAS_LAUNCH_FLASH: bool = true;

/// Nature loop: the leaf shed (rate per second PER HAND, drift speed u/s,
/// life seconds — transcribed from the parsed leaf emitter).
pub const NATURE_CAST_LEAF_RATE: f32 = 11.0;
pub const NATURE_CAST_LEAF_SPEED: f32 = 0.086;
pub const NATURE_CAST_LEAF_LIFE: f32 = 0.8;
const NATURE_LEAF_SIZE: f32 = 0.055;

/// Nature launch: the water-ring + gold-spark jet (`nature_cast_hand.m2`).
/// Rates per second per hand, mote life and speeds transcribed; the ~2°
/// spread is the source's 0.035 rad directional collimation.
pub const NATURE_LAUNCH_BURST_SECS: f32 = 0.3;
pub const NATURE_LAUNCH_RING_RATE: f32 = 21.4;
pub const NATURE_LAUNCH_SPARK_RATE: f32 = 30.0;
pub const NATURE_LAUNCH_SPREAD: f32 = 0.035;
pub const NATURE_LAUNCH_BURST_SCALE: f32 = 1.0;
const NATURE_LAUNCH_RING_SPEED: f32 = 0.261;
const NATURE_LAUNCH_SPARK_SPEED: f32 = 0.278;
const NATURE_LAUNCH_RING_SIZE: f32 = 0.16;
const NATURE_LAUNCH_SPARK_SIZE: f32 = 0.07;

/// Seconds the hand glow blooms in from nothing at cast start. The source
/// snaps (InitialAnimID -1, no intro); a couple of frames of growth just
/// keeps the pop from strobing at render rates above the tick rate.
pub const HEAL_CAST_BLOOM_SECS: f32 = 0.15;

// --- Body posture (ReadySpellOmni -> SpellCastOmni, capsule dialect) ----------

/// Torso pitch of the hands-raised precast loop, radians. NEGATIVE leans the
/// torso BACK (chest open, hands presented) — the ReadySpellOmni read.
pub const HEAL_CAST_READY_PITCH: f32 = -0.10;
/// Peak forward pitch of the release surge — SpellCastOmni's both-hands
/// upward/outward throw, read as the torso committing toward the target.
pub const HEAL_CAST_RELEASE_PITCH: f32 = 0.16;
/// Seconds the loop pitch eases in over at cast start.
pub const HEAL_CAST_POSTURE_EASE_SECS: f32 = 0.25;

// --- Pure geometry (probed directly) ------------------------------------------

/// How long a family's launch flare runs.
pub fn heal_cast_flare_secs(kind: HealCastKind) -> f32 {
    match kind {
        HealCastKind::Holy => HOLY_LAUNCH_FLARE_SECS,
        HealCastKind::Nature => NATURE_LAUNCH_BURST_SECS,
    }
}

/// The glow quad sizes (outer, core) for a family.
pub fn heal_cast_glow_sizes(kind: HealCastKind) -> [f32; 2] {
    let sizes = match kind {
        HealCastKind::Holy => HOLY_CAST_GLOW_SIZES,
        HealCastKind::Nature => NATURE_CAST_GLOW_SIZES,
    };
    [sizes[0] * GLOW_SCALE, sizes[1] * GLOW_SCALE]
}

pub fn heal_cast_wisp_width(kind: HealCastKind) -> f32 {
    match kind {
        HealCastKind::Holy => HOLY_CAST_WISP_WIDTH,
        HealCastKind::Nature => NATURE_CAST_WISP_WIDTH,
    }
}

/// Rendered trail length of the `index`th wisp: the arc its head sweeps in
/// one edge lifetime at the orbit's linear speed.
pub fn heal_cast_wisp_length(index: u32) -> f32 {
    TAU * HOLY_CAST_ORBIT_HZ * HEAL_CAST_WISP_ORBIT_RADIUS
        * HEAL_CAST_WISP_LIFETIMES[index as usize % 3]
}

/// The `index`th wisp's orbit plane. The source hangs the three ribbon heads
/// off three independent bone chains pivoting in three different directions;
/// three fixed, visibly distinct plane tilts reproduce that read.
pub fn heal_cast_wisp_plane(index: u32) -> Quat {
    match index % 3 {
        0 => Quat::from_rotation_x(0.45),
        1 => Quat::from_rotation_z(0.55) * Quat::from_rotation_x(-0.30),
        _ => Quat::from_rotation_x(-0.50) * Quat::from_rotation_z(-0.35),
    }
}

/// Rig-local position of the `index`th wisp head at `age` seconds into the
/// cast: an orbit around the hand in the wisp's own tilted plane, all three
/// on the shared 1 s cycle, phase-staggered a third of a turn apart.
pub fn heal_cast_wisp_head(index: u32, age: f32) -> Vec3 {
    let angle = index as f32 * TAU / 3.0 + TAU * HOLY_CAST_ORBIT_HZ * age;
    heal_cast_wisp_plane(index)
        * Vec3::new(angle.cos(), 0.0, angle.sin())
        * HEAL_CAST_WISP_ORBIT_RADIUS
}

/// Rig-local direction the `index`th wisp head is moving at `age` — the
/// orbit tangent its trail streams back along.
pub fn heal_cast_wisp_tangent(index: u32, age: f32) -> Vec3 {
    let angle = index as f32 * TAU / 3.0 + TAU * HOLY_CAST_ORBIT_HZ * age;
    heal_cast_wisp_plane(index) * Vec3::new(-angle.sin(), 0.0, angle.cos())
}

/// The launch jet's rig-local direction: forward along the facing, tilted up
/// — hands throwing the heal out and up toward the target, the SpellCastOmni
/// release line.
pub fn nature_launch_jet_dir() -> Vec3 {
    Vec3::new(0.0, 0.35, 1.0).normalize()
}

/// Torso pitch of the release surge at normalized flare progress `t` in
/// 0..1: a fast commit forward, decaying back to neutral by the flare's end.
pub fn heal_cast_release_pitch(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    HEAL_CAST_RELEASE_PITCH * (t / 0.25).clamp(0.0, 1.0) * (1.0 - t)
}

// --- Palette ------------------------------------------------------------------

/// The three Holy ribbon tints (transcribed ribbon color tracks).
fn holy_wisp_color(index: u32) -> Color {
    match index % 3 {
        0 => Color::srgb(0.988, 0.863, 0.165),
        1 => Color::srgb(0.992, 0.804, 0.290),
        _ => Color::srgb(0.984, 0.855, 0.145),
    }
}

/// The three Nature star-thread tints (transcribed).
fn nature_wisp_color(index: u32) -> Color {
    match index % 3 {
        0 => Color::srgb(0.235, 1.0, 0.0),
        1 => Color::srgb(0.318, 1.0, 0.106),
        _ => Color::srgb(0.235, 1.0, 0.0),
    }
}

/// `leafbrown` rendered additively — a warm amber.
fn leaf_brown() -> Color {
    Color::srgb(0.62, 0.44, 0.20)
}

/// The launch rings' pale water-blue (`shockwavewater1`).
fn water_ring_blue() -> Color {
    Color::srgb(0.55, 0.75, 0.95)
}

fn glow_color(kind: HealCastKind) -> Color {
    match kind {
        HealCastKind::Holy => holy_gold_pale(),
        HealCastKind::Nature => nature_green(),
    }
}

/// Source alphas: Holy ribbons 0.80, Nature threads 0.70.
fn wisp_alpha(kind: HealCastKind) -> f32 {
    match kind {
        HealCastKind::Holy => 0.80,
        HealCastKind::Nature => 0.70,
    }
}

// --- Shared assets ------------------------------------------------------------

/// Meshes and sprites every hand rig shares. Built once, lazily.
pub struct HealCastAssets {
    quad: Handle<Mesh>,
    ring: Handle<Mesh>,
    star: Handle<Image>,
    dot: Handle<Image>,
}

impl HealCastAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            // The water-ring sprite: a thin annulus, billboarded and expanded
            // over its life (the `hard_cc.rs` CC-flare idiom).
            ring: meshes.add(Annulus::new(0.72, 1.0).mesh().resolution(40)),
            star: images.add(star_flash_texture()),
            dot: images.add(soft_dot_texture()),
        }
    }
}

fn additive_material(
    materials: &mut Assets<StandardMaterial>,
    color: Color,
    strength: f32,
    texture: Option<Handle<Image>>,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color,
        base_color_texture: texture.clone(),
        emissive: emissive_of(color, strength),
        emissive_texture: texture,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    })
}

// --- Systems ------------------------------------------------------------------

/// Spawn the two hand rigs (and the cast posture) when a combatant starts a
/// hard-cast heal. Non-heal casts are untouched — they keep the generic
/// casting orb (`spawn_casting_orbs` skips heals; this is the replacement).
pub fn spawn_heal_cast_glows(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<HealCastAssets>>,
    new_casts: Query<(Entity, &CastingState, &Children), Added<CastingState>>,
    bodies: Query<(), With<VisualBody>>,
    existing: Query<&HealCastHand>,
) {
    for (caster, casting, children) in new_casts.iter() {
        let Some(kind) = HealCastKind::for_ability(casting.ability) else {
            continue;
        };
        // A cast already flagged interrupted never lights up (the casting-orb
        // guard: its ending marker was already consumed).
        if casting.interrupted {
            continue;
        }
        // One live LOOP per caster. Scoped to Loop — a back-to-back heal can
        // begin while the previous launch's flare is still playing out, and
        // must not be swallowed by it.
        if existing
            .iter()
            .any(|rig| rig.caster == caster && matches!(rig.phase, HealCastPhase::Loop))
        {
            continue;
        }
        let Some(body) = children.iter().find(|&child| bodies.get(child).is_ok()) else {
            continue; // headless-shaped caster with no rendered body
        };

        let assets = assets.get_or_insert_with(|| HealCastAssets::build(&mut meshes, &mut images));
        let has_launch_flash = match casting.ability {
            crate::states::play_match::abilities::AbilityType::FlashOfLight => {
                FLASH_OF_LIGHT_HAS_LAUNCH_FLASH
            }
            _ => true,
        };
        let wisp_width = heal_cast_wisp_width(kind);

        for side in [1.0_f32, -1.0] {
            // Per-rig materials: alphas are animated absolutely each frame,
            // so nothing outside this rig may share the handles.
            let outer_material =
                additive_material(&mut materials, glow_color(kind), 2.0, Some(assets.dot.clone()));
            let core_material =
                additive_material(&mut materials, glow_color(kind), 2.8, Some(assets.dot.clone()));

            let rig = commands
                .spawn((
                    Transform::from_translation(spell_hand_local(side)),
                    Visibility::default(),
                    HealCastHand {
                        caster,
                        body,
                        kind,
                        side,
                        age: 0.0,
                        phase: HealCastPhase::Loop,
                        has_launch_flash,
                        leaf_carry: 0.0,
                        burst_carry: [0.0; 2],
                        emitted: 0,
                        quad: assets.quad.clone(),
                        ring_mesh: assets.ring.clone(),
                        leaf_material: additive_material(
                            &mut materials,
                            leaf_brown(),
                            1.2,
                            Some(assets.dot.clone()),
                        ),
                        spark_material: additive_material(
                            &mut materials,
                            super::heal_impact::holy_gold(),
                            2.4,
                            Some(assets.dot.clone()),
                        ),
                        ring_material: additive_material(
                            &mut materials,
                            water_ring_blue(),
                            1.8,
                            None,
                        ),
                    },
                ))
                .id();
            commands.entity(body).add_child(rig);

            let mut parts: Vec<Entity> = Vec::new();
            for (role, material, alpha) in [
                (HealCastPieceRole::GlowOuter, outer_material, 0.55),
                (HealCastPieceRole::GlowCore, core_material, 0.85),
            ] {
                parts.push(
                    commands
                        .spawn((
                            HealCastPiece { role, base_alpha: alpha },
                            Mesh3d(assets.quad.clone()),
                            MeshMaterial3d(material),
                            Transform::from_scale(Vec3::splat(1e-3)),
                            NotShadowCaster,
                        ))
                        .id(),
                );
            }
            for index in 0..3_u32 {
                let (color, texture) = match kind {
                    // Ribbon-blur streamers for Holy, star threads for Nature
                    // — the source's genericglow2b vs star11b split.
                    HealCastKind::Holy => (holy_wisp_color(index), assets.dot.clone()),
                    HealCastKind::Nature => (nature_wisp_color(index), assets.star.clone()),
                };
                let material = additive_material(&mut materials, color, 2.2, Some(texture));
                parts.push(
                    commands
                        .spawn((
                            HealCastPiece {
                                role: HealCastPieceRole::Wisp { index },
                                base_alpha: wisp_alpha(kind),
                            },
                            Mesh3d(assets.quad.clone()),
                            MeshMaterial3d(material),
                            Transform::from_translation(heal_cast_wisp_head(index, 0.0))
                                .with_scale(Vec3::new(wisp_width, 1e-3, 1.0)),
                            NotShadowCaster,
                        ))
                        .id(),
                );
            }
            commands.entity(rig).add_children(&parts);
        }

        commands
            .entity(caster)
            .try_insert(HealCastPosture { pitch: 0.0 });
    }
}

/// Per-frame animation: the glow pulse, the wisp swirl, the Nature leaf
/// shed, the launch flare/burst, and mote aging. Time comes from `Res<Time>`
/// accumulation — never gated on per-frame sim movement.
pub fn update_heal_cast_glows(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rigs: Query<(Entity, &mut HealCastHand, Option<&Children>)>,
    mut pieces: Query<
        (
            &HealCastPiece,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (Without<HealCastLeaf>, Without<HealCastBurstMote>),
    >,
    mut leaves: Query<
        (Entity, &mut HealCastLeaf, &mut Transform),
        (Without<HealCastPiece>, Without<HealCastBurstMote>),
    >,
    mut bursts: Query<
        (Entity, &mut HealCastBurstMote, &mut Transform),
        (Without<HealCastPiece>, Without<HealCastLeaf>),
    >,
) {
    let dt = time.delta_secs();

    for (rig_entity, mut rig, children) in rigs.iter_mut() {
        rig.age += dt;
        let age = rig.age;

        // The flare clock, and the alpha/scale factors each phase applies.
        // `bloom` covers cast start; `flare_alpha`/`flare_scale` cover launch.
        let bloom = (age / HEAL_CAST_BLOOM_SECS).clamp(0.0, 1.0);
        let (flare_alpha, flare_scale) = match rig.phase {
            HealCastPhase::Loop => (1.0, 1.0),
            HealCastPhase::Flare { remaining } => {
                let remaining = remaining - dt;
                if remaining <= 0.0 {
                    commands.entity(rig_entity).despawn();
                    continue;
                }
                rig.phase = HealCastPhase::Flare { remaining };
                let total = heal_cast_flare_secs(rig.kind);
                let t = 1.0 - remaining / total;
                // The re-flare: brighter than the loop ever was, swelling as
                // it dies (Holy). Nature's loop pieces are already gone.
                (
                    (1.0 + HOLY_LAUNCH_FLARE_INTENSITY) * (1.0 - t),
                    1.0 + 0.5 * HOLY_LAUNCH_FLARE_INTENSITY * t,
                )
            }
        };

        // Nature emission: leaves through the loop, the jet through the flare.
        if rig.kind == HealCastKind::Nature {
            match rig.phase {
                HealCastPhase::Loop => {
                    rig.leaf_carry += NATURE_CAST_LEAF_RATE * dt;
                    while rig.leaf_carry >= 1.0 {
                        rig.leaf_carry -= 1.0;
                        let i = rig.emitted;
                        rig.emitted = rig.emitted.wrapping_add(1);
                        let seed = rig_entity.index().wrapping_add(i.wrapping_mul(0x9E37_79B9));
                        // Omni drift: a full-sphere direction, position-hashed.
                        let a = heal_jitter(seed) * TAU;
                        let z = heal_jitter(seed ^ 0x1EAF) * 2.0 - 1.0;
                        let r = (1.0 - z * z).max(0.0).sqrt();
                        let dir = Vec3::new(a.cos() * r, z, a.sin() * r);
                        let jitter = Vec3::new(
                            (heal_jitter(seed ^ 0x51ED) - 0.5) * 0.12,
                            (heal_jitter(seed ^ 0x2DDF) - 0.5) * 0.12,
                            0.0,
                        );
                        let leaf = commands
                            .spawn((
                                HealCastLeaf {
                                    velocity: dir * NATURE_CAST_LEAF_SPEED,
                                    age: 0.0,
                                    life: NATURE_CAST_LEAF_LIFE,
                                },
                                Mesh3d(rig.quad.clone()),
                                MeshMaterial3d(rig.leaf_material.clone()),
                                Transform::from_translation(jitter)
                                    .with_scale(Vec3::splat(NATURE_LEAF_SIZE)),
                                NotShadowCaster,
                            ))
                            .id();
                        commands.entity(rig_entity).add_child(leaf);
                    }
                }
                HealCastPhase::Flare { .. } => {
                    let jet = nature_launch_jet_dir();
                    for (ei, (rate, speed, kind)) in [
                        (
                            NATURE_LAUNCH_RING_RATE,
                            NATURE_LAUNCH_RING_SPEED,
                            HealCastBurstKind::WaterRing,
                        ),
                        (
                            NATURE_LAUNCH_SPARK_RATE,
                            NATURE_LAUNCH_SPARK_SPEED,
                            HealCastBurstKind::GoldSpark,
                        ),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        rig.burst_carry[ei] += rate * dt;
                        while rig.burst_carry[ei] >= 1.0 {
                            rig.burst_carry[ei] -= 1.0;
                            let i = rig.emitted;
                            rig.emitted = rig.emitted.wrapping_add(1);
                            let seed =
                                rig_entity.index().wrapping_add(i.wrapping_mul(0x85EB_CA6B));
                            // A tight directional jet: tilt the axis by a
                            // hashed angle within the ~2° spread cone.
                            let tilt = heal_jitter(seed) * NATURE_LAUNCH_SPREAD;
                            let roll = heal_jitter(seed ^ 0x27D4) * TAU;
                            let dir = Quat::from_axis_angle(jet, roll)
                                * Quat::from_axis_angle(jet.cross(Vec3::Y).normalize(), tilt)
                                * jet;
                            let (mesh, material, size) = match kind {
                                HealCastBurstKind::WaterRing => (
                                    rig.ring_mesh.clone(),
                                    rig.ring_material.clone(),
                                    NATURE_LAUNCH_RING_SIZE,
                                ),
                                HealCastBurstKind::GoldSpark => (
                                    rig.quad.clone(),
                                    rig.spark_material.clone(),
                                    NATURE_LAUNCH_SPARK_SIZE,
                                ),
                            };
                            let mote = commands
                                .spawn((
                                    HealCastBurstMote {
                                        kind,
                                        velocity: dir * speed * NATURE_LAUNCH_BURST_SCALE,
                                        age: 0.0,
                                        life: NATURE_LAUNCH_BURST_SECS,
                                    },
                                    Mesh3d(mesh),
                                    MeshMaterial3d(material),
                                    Transform::from_scale(Vec3::splat(
                                        size * NATURE_LAUNCH_BURST_SCALE,
                                    )),
                                    NotShadowCaster,
                                ))
                                .id();
                            commands.entity(rig_entity).add_child(mote);
                        }
                    }
                }
            }
        }

        let Some(children) = children else {
            continue;
        };
        let glow_sizes = heal_cast_glow_sizes(rig.kind);
        let wisp_width = heal_cast_wisp_width(rig.kind);
        // The model's own 1 s loop, as a gentle breathing pulse.
        let pulse = 1.0 + 0.06 * (TAU * HOLY_CAST_ORBIT_HZ * age).sin();

        for child in children.iter() {
            if let Ok((piece, mut part, material)) = pieces.get_mut(child) {
                let alpha = piece.base_alpha * bloom * flare_alpha;
                match piece.role {
                    HealCastPieceRole::GlowOuter => {
                        part.scale =
                            Vec3::splat((glow_sizes[0] * pulse * flare_scale).max(1e-3));
                    }
                    HealCastPieceRole::GlowCore => {
                        part.scale =
                            Vec3::splat((glow_sizes[1] * pulse * flare_scale).max(1e-3));
                    }
                    HealCastPieceRole::Wisp { index } => {
                        // The head orbits; the trail streams back along the
                        // tangent, one edge-lifetime long, centred behind it.
                        let head = heal_cast_wisp_head(index, age);
                        let tangent = heal_cast_wisp_tangent(index, age);
                        let length = heal_cast_wisp_length(index) * bloom;
                        part.translation = head - tangent * (length * 0.5);
                        part.scale = Vec3::new(
                            wisp_width * flare_scale,
                            length.max(1e-3),
                            1.0,
                        );
                    }
                }
                if let Some(material) = materials.get_mut(&material.0) {
                    material.base_color.set_alpha(alpha);
                }
            }

            if let Ok((leaf_entity, mut leaf, mut part)) = leaves.get_mut(child) {
                leaf.age += dt;
                if leaf.age >= leaf.life {
                    commands.entity(leaf_entity).despawn();
                    continue;
                }
                part.translation += leaf.velocity * dt;
                let k = 1.0 - leaf.age / leaf.life;
                part.scale = Vec3::splat((NATURE_LEAF_SIZE * k.powf(0.5)).max(1e-4));
            }

            if let Ok((mote_entity, mut mote, mut part)) = bursts.get_mut(child) {
                mote.age += dt;
                if mote.age >= mote.life {
                    commands.entity(mote_entity).despawn();
                    continue;
                }
                part.translation += mote.velocity * dt;
                let k = 1.0 - mote.age / mote.life;
                match mote.kind {
                    HealCastBurstKind::WaterRing => {
                        // The shockwave read: the ring EXPANDS as it fades.
                        let swell = 1.0 + 2.5 * (1.0 - k);
                        part.scale = Vec3::splat(
                            (NATURE_LAUNCH_RING_SIZE * NATURE_LAUNCH_BURST_SCALE * swell
                                * k.powf(0.35))
                            .max(1e-4),
                        );
                    }
                    HealCastBurstKind::GoldSpark => {
                        part.scale = Vec3::splat(
                            (NATURE_LAUNCH_SPARK_SIZE * NATURE_LAUNCH_BURST_SCALE
                                * k.powf(0.6))
                            .max(1e-4),
                        );
                    }
                }
            }
        }
    }
}

/// Consume `CastEnding` markers for heal casts, transitioning the hand rigs.
/// Landed -> the launch (Holy re-flares its own glow; Nature swaps the loop
/// pieces for the burst jet). Fizzled or Interrupted -> everything stops
/// DEAD, immediately — the source has no interrupt visual and fires none.
///
/// Runs in `FixedUpdate` after `CombatSystemPhase::CombatResolution` and
/// BEFORE `consume_cast_ending_signals`: that consumer owns despawning the
/// marker (this one only reads it), and FixedUpdate can tick multiple times
/// per rendered frame, so an Update-schedule consumer could miss a marker.
pub fn consume_heal_cast_endings(
    mut commands: Commands,
    signals: Query<&CastEnding>,
    mut rigs: Query<(Entity, &mut HealCastHand, Option<&Children>)>,
    loop_pieces: Query<(), Or<(With<HealCastPiece>, With<HealCastLeaf>)>>,
) {
    for ending in signals.iter() {
        for (rig_entity, mut rig, children) in rigs.iter_mut() {
            if rig.caster != ending.caster || !matches!(rig.phase, HealCastPhase::Loop) {
                continue;
            }
            match ending.kind {
                CastEndingKind::Landed if rig.has_launch_flash => {
                    rig.phase = HealCastPhase::Flare {
                        remaining: heal_cast_flare_secs(rig.kind),
                    };
                    if rig.kind == HealCastKind::Nature {
                        // The loop kit dies; the dedicated launch model takes
                        // over. Glows, wisps and airborne leaves all go.
                        if let Some(children) = children {
                            for child in children.iter() {
                                if loop_pieces.get(child).is_ok() {
                                    commands.entity(child).despawn();
                                }
                            }
                        }
                    }
                }
                _ => {
                    // Fizzle, interrupt, or a flash-less Flash of Light
                    // landing: the hand glow just vanishes, mid-swirl.
                    commands.entity(rig_entity).despawn();
                }
            }
        }
    }
}

/// The capsule dialect of ReadySpellOmni -> SpellCastOmni. Derives the torso
/// pitch from the caster's live hand rigs (main side owns it, the body-lean
/// rule) and writes the `VisualBody` rotation ABSOLUTELY — no compounding.
/// Runs after `animate_body_lean`, so while a heal cast is live the cast
/// posture owns the torso; the frame no rig remains it snaps the body back
/// to identity and removes itself (an interrupted heal's body loop stops
/// dead — faithful). Cedes entirely to the death fall.
pub fn update_heal_cast_posture(
    mut commands: Commands,
    time: Res<Time>,
    rigs: Query<&HealCastHand>,
    mut casters: Query<
        (Entity, &mut HealCastPosture, &Children, Option<&DeathAnimation>),
        With<Combatant>,
    >,
    mut bodies: Query<&mut Transform, With<VisualBody>>,
) {
    let dt = time.delta_secs();

    for (caster, mut posture, children, dying) in casters.iter_mut() {
        // Main side owns the posture (the body-lean rule). Prefer a LOOP rig
        // over a still-flaring one — a back-to-back heal's new loop starts
        // while the previous launch's flare plays out, and the loop's ready
        // lean is the pose the new cast wants.
        let rig = rigs
            .iter()
            .filter(|rig| rig.caster == caster && rig.side > 0.0)
            .max_by_key(|rig| matches!(rig.phase, HealCastPhase::Loop) as u8);

        if dying.is_some() {
            // The death fall owns rotation from here; never write it again.
            commands.entity(caster).remove::<HealCastPosture>();
            continue;
        }

        let Some(rig) = rig else {
            // Cast over (landed flare spent, interrupt, fizzle): stop dead.
            for child in children.iter() {
                if let Ok(mut body) = bodies.get_mut(child) {
                    body.rotation = Quat::IDENTITY;
                }
            }
            commands.entity(caster).remove::<HealCastPosture>();
            continue;
        };

        match rig.phase {
            HealCastPhase::Loop => {
                let rate = HEAL_CAST_READY_PITCH.abs() / HEAL_CAST_POSTURE_EASE_SECS;
                posture.pitch = (posture.pitch - rate * dt).max(HEAL_CAST_READY_PITCH);
            }
            HealCastPhase::Flare { remaining } => {
                let total = heal_cast_flare_secs(rig.kind);
                posture.pitch = heal_cast_release_pitch(1.0 - remaining / total);
            }
        }

        for child in children.iter() {
            if let Ok(mut body) = bodies.get_mut(child) {
                body.rotation = Quat::from_rotation_x(posture.pitch);
            }
        }
    }
}

/// Turn the flat pieces of every hand rig to face the camera.
///
/// The rigs are children of the `VisualBody`, so a piece's rendered rotation
/// composes through the caster's facing and the body's posture pitch; the
/// billboard therefore counter-rotates by that full parent world rotation
/// (`caster * body` — the rig's own local rotation is always identity).
/// Wisps keep a roll about the view axis aligning their long axis with the
/// orbit tangent's projection, so the trail visibly streams behind its head.
pub fn billboard_heal_cast_glows(
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<HealCastPiece>,
            Without<HealCastLeaf>,
            Without<HealCastBurstMote>,
        ),
    >,
    rigs: Query<(&HealCastHand, &Children)>,
    casters: Query<
        &Transform,
        (
            With<Combatant>,
            Without<Camera3d>,
            Without<HealCastPiece>,
            Without<HealCastLeaf>,
            Without<HealCastBurstMote>,
        ),
    >,
    bodies: Query<
        &Transform,
        (
            With<VisualBody>,
            Without<Camera3d>,
            Without<HealCastPiece>,
            Without<HealCastLeaf>,
            Without<HealCastBurstMote>,
        ),
    >,
    mut pieces: Query<
        (&HealCastPiece, &mut Transform),
        (Without<HealCastLeaf>, Without<HealCastBurstMote>),
    >,
    mut leaves: Query<
        &mut Transform,
        (With<HealCastLeaf>, Without<HealCastPiece>, Without<HealCastBurstMote>),
    >,
    mut bursts: Query<
        &mut Transform,
        (With<HealCastBurstMote>, Without<HealCastPiece>, Without<HealCastLeaf>),
    >,
) {
    let Some(cam) = camera.iter().next() else {
        return;
    };
    for (rig, children) in rigs.iter() {
        let caster_rot = casters
            .get(rig.caster)
            .map(|t| t.rotation)
            .unwrap_or(Quat::IDENTITY);
        let body_rot = bodies
            .get(rig.body)
            .map(|t| t.rotation)
            .unwrap_or(Quat::IDENTITY);
        let parent_world = caster_rot * body_rot;
        let facing = parent_world.inverse() * cam.rotation;

        for child in children.iter() {
            if let Ok((piece, mut part)) = pieces.get_mut(child) {
                match piece.role {
                    HealCastPieceRole::Wisp { index } => {
                        let tangent_world =
                            parent_world * heal_cast_wisp_tangent(index, rig.age);
                        let t_cam = cam.rotation.inverse() * tangent_world;
                        // Roll the quad's long (+Y) axis onto the tangent's
                        // projection in the billboard plane.
                        let roll = (-t_cam.x).atan2(t_cam.y);
                        part.rotation = facing * Quat::from_rotation_z(roll);
                    }
                    _ => {
                        part.rotation = facing;
                    }
                }
            }
            if let Ok(mut part) = leaves.get_mut(child) {
                part.rotation = facing;
            }
            if let Ok(mut part) = bursts.get_mut(child) {
                part.rotation = facing;
            }
        }
    }
}

/// Despawn hand rigs whose cast silently went away with no `CastEnding`
/// marker — caster death, match end, entity teardown — and rigs whose
/// `CastingState` sits in its interrupted-display window (the state lingers
/// for HUD feedback, but the hand glow stops DEAD at the interrupt, not half
/// a second later). Flare-phase rigs own their remaining lifetime; the
/// update system retires them.
pub fn cleanup_heal_cast_glows(
    mut commands: Commands,
    rigs: Query<(Entity, &HealCastHand)>,
    cast_states: Query<&CastingState>,
) {
    for (rig_entity, rig) in rigs.iter() {
        if !matches!(rig.phase, HealCastPhase::Loop) {
            continue;
        }
        match cast_states.get(rig.caster) {
            Ok(casting) if !casting.interrupted => {}
            _ => {
                commands.entity(rig_entity).despawn();
            }
        }
    }
}
