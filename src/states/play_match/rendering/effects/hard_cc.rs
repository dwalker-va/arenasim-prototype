use bevy::color::LinearRgba;
use bevy::prelude::*;
use bevy::render::mesh::ConeAnchor;
use std::f32::consts::{FRAC_PI_2, TAU};

use crate::states::play_match::abilities::SpellSchool;
use crate::states::play_match::components::*;

// ==============================================================================
// Hard CC Receiver Treatment — Root and Stun
// ==============================================================================
//
// Root and Stun were the two `AuraType`s with no visual on EITHER side, which
// left six abilities rendering nothing at all: Frost Nova and Spider Web (Root),
// and Cheap Shot, Kidney Shot, Hammer of Justice and Boar Charge (Stun). All six
// are instant AND aura-only, so they enter neither generic caster-side hook
// (`CastingState` -> casting orb, `QueuedInstantAttack` -> `InstantAttackLanded`)
// — the receiver side is the only side available for four of them, and this
// module is entirely receiver-side.
//
// The spatial grammar carries the distinction, because Root and Stun differ
// mechanically and must be told apart at a glance:
//
//   Root  -> AT THE FEET. Ice crystals (Frost school) or a webbed sheet (Nature)
//            stabbing up around the victim, then completely STILL. A rooted unit
//            may still cast and swing; only its feet are held.
//   Stun  -> OVER THE HEAD. A hueless whirl of beads turning once per second.
//            A stunned unit is provably inert (`is_incapacitated` blocks
//            auto-attacks and strips `CastingState`), so the whirl converts an
//            ABSENCE of motion into a presence.
//
// Two things this treatment deliberately does NOT do:
//
//   * It never encloses the body. Enclosure is this game's Incapacitate
//     language — Freezing Trap spawns a `Cuboid::new(1.5, 2.3, 1.5)` ice block
//     around its victim. Root leaves the torso free to act, so the web stops at
//     the shins.
//   * It never touches the victim's body at all: no tint, no mesh swap, no pose,
//     no gait suppression. That sidesteps the whole `OriginalBodyMaterial`
//     contention family (see `shared-restore-slot-mutual-exclusion.md`), and it
//     means a Cheap Shot on a stealthed Rogue stays a non-event. Gait
//     suppression would also be actively wrong: `advance_gait` is
//     distance-driven, so a unit the sim already holds still self-idles and
//     eases to `rest_y` on its own. The job here is to EXPLAIN the stillness,
//     not manufacture it.
//
// Every visual is a WORLD-SPACE rig that follows its owner — never a child of
// the sim entity (whose yaw is sim-written, which would make the whirl's spin
// fight the unit's facing snaps) and never a child of the `VisualBody` (whose
// local y belongs to the gaits, which would lift a ground piece off the floor).
//
// Graphical-only: registered in `states/mod.rs` alone, never in
// `add_core_combat_systems`. No `game_rng` draw, no sim `Transform` write, no
// combat-code change — headless stays byte-identical by construction.

// ------------------------------------------------------------------------------
// Shared constants
// ------------------------------------------------------------------------------

/// Fixed WORLD height for every ground piece. NOT derived from `rest_y`: the
/// arena floor spawns with an identity transform at y=0, while combatants sim at
/// y=1.0 and pets at y=0.75. One fixed height is correct for both; a
/// `rest_y`-derived offset is correct for neither.
const CC_GROUND_Y: f32 = 0.06;
/// Uniform scale applied to a pet's whole rig, so one set of local coordinates
/// serves both unit sizes.
const CC_PET_STATURE: f32 = 0.55;

/// Apply flare: a thin expanding annulus marking the instant the CC lands.
/// Per-VICTIM, so a Frost Nova catching three enemies pops three rings at once
/// and the AoE reads as an AoE with no caster-side hook at all.
const CC_FLARE_SECS: f32 = 0.40;
const CC_FLARE_START: f32 = 0.25;
const CC_FLARE_END_GROUND: f32 = 3.2;
const CC_FLARE_END_HEAD: f32 = 2.4;
/// The flare's ring is drawn as an annulus whose inner edge is this fraction of
/// its outer, so it stays a thin band as it expands rather than a filling disc.
const CC_FLARE_INNER_RATIO: f32 = 0.80;
const CC_FLARE_COLOR: Color = Color::srgba(0.75, 0.86, 1.00, 0.80);
const CC_FLARE_EMISSIVE: LinearRgba = LinearRgba::new(1.2, 1.6, 2.4, 1.0);

// ------------------------------------------------------------------------------
// Root constants
// ------------------------------------------------------------------------------

const ROOT_GROW_SECS: f32 = 0.18;
const ROOT_RETRACT_SECS: f32 = 0.22;

/// Eight faceted crystals on a ring just outside the body's 0.5yd radius,
/// shin-to-knee tall on a 2.5yd capsule.
const ROOT_SPIKE_COUNT: u32 = 8;
const ROOT_SPIKE_RING_R: f32 = 0.62;
const ROOT_SPIKE_BASE_R: f32 = 0.16;
const ROOT_SPIKE_HEIGHT: f32 = 0.90;
/// Radians the tip leans outward from vertical.
const ROOT_SPIKE_SPLAY: f32 = 0.22;
/// Radians of phase, so the ring never lines up with the camera axis.
const ROOT_SPIKE_PHASE: f32 = 0.19;
/// Fraction of height the hash may remove, so the crown is ragged rather than a
/// picket fence.
const ROOT_SPIKE_JITTER: f32 = 0.34;
/// Five sides reads faceted rather than smooth, matching the chunky low-poly
/// crystals of the source material.
const ROOT_SPIKE_SIDES: u32 = 5;

/// Lit, not additive: solid shaded objects standing next to this game's
/// additive-glow vocabulary is itself a legibility axis. The barely-blue tint
/// never competes with a class body colour.
const ROOT_ICE_COLOR: Color = Color::srgb(0.80, 0.86, 0.95);
const ROOT_ICE_EMISSIVE: LinearRgba = LinearRgba::new(0.10, 0.16, 0.30, 1.0);
const ROOT_ICE_ROUGHNESS: f32 = 0.25;

// ------------------------------------------------------------------------------
// Root: web variant (Nature school)
// ------------------------------------------------------------------------------

/// A webbed sheet over the shins, built with real orb-web topology: radial
/// spokes from the leg out to a hem pinned on the floor, crossed by concentric
/// rings. A web only reads as a web if you can see THROUGH it — a solid surface
/// reads as an object and tapered cones read as thorns.
const WEB_ATTACH_R: f32 = 0.54;
const WEB_ATTACH_H: f32 = 0.82;
const WEB_HEM_R: f32 = 1.15;
const WEB_SPOKES: u32 = 11;
const WEB_RINGS: u32 = 4;
/// Fabric sag exponent: y falls away fast near the leg, then flattens toward the
/// hem, the way cloth hangs.
const WEB_SAG: f32 = 1.7;
/// Segments per spoke, so the sag curve is visible rather than a straight ramp.
const WEB_SPOKE_SEGS: u32 = 6;
const WEB_THREAD_R: f32 = 0.014;
/// Fraction of the hem radius the hash may remove per spoke, so the outline is
/// irregular as a real web is.
const WEB_HEM_JITTER: f32 = 0.22;
const WEB_SILK_COLOR: Color = Color::srgb(0.93, 0.92, 0.87);
const WEB_SILK_EMISSIVE: LinearRgba = LinearRgba::new(0.16, 0.16, 0.13, 1.0);

// ------------------------------------------------------------------------------
// Stun constants
// ------------------------------------------------------------------------------

const STUN_GROW_SECS: f32 = 0.14;
const STUN_RETRACT_SECS: f32 = 0.18;

/// Height of the whirl above the victim's BODY CENTRE — not above its sim y.
///
/// The two are not the same for a pet and the difference is large: a pet's sim
/// entity sits at `owner_position + 0.75`, so world y ~1.75, while its
/// `VisualBody` child carries `rest_y = 0.3 - 1.75 = -1.45` and the capsule
/// actually renders at world 0.3 (`play_match/mod.rs:1310-1338`). Anchoring off
/// the sim y would hang a pet's whirl ~1.9yd above its head. `rest_y` IS the
/// sim-to-render correction, so `translation.y + rest_y + lift` is right for
/// both unit kinds — the same derivation `update_fear_visuals` uses to place its
/// flash at the torso.
///
/// Combatant: body centre 1.0, `Capsule3d::new(0.5, 1.5)` crown at 2.25, whirl
/// at 2.55 — clear of the crown and of the casting orb at 2.50.
/// Pet: body centre 0.3, `Capsule3d::new(0.35, 0.6)` crown at 0.95, whirl at 1.20.
const STUN_LIFT_ABOVE_BODY: f32 = 1.55;
const STUN_LIFT_ABOVE_BODY_PET: f32 = 0.90;

const STUN_ARMS: u32 = 2;
const STUN_BEADS_PER_ARM: u32 = 5;
/// Turns per arm. Deliberately flattened from the source's ~1.75: a full spiral
/// reads as mush at range, a lazy double-hook stays legible.
const STUN_SPIRAL_TURNS: f32 = 0.75;
const STUN_R_INNER: f32 = 0.22;
const STUN_R_OUTER: f32 = 0.92;
/// Vertical rise from inner to outer bead — a shallow ~20 degree saucer flare.
const STUN_RISE: f32 = 0.34;
const STUN_BEAD_MIN: f32 = 0.085;
const STUN_BEAD_MAX: f32 = 0.155;
/// Exactly one revolution per second, matching the source's 9 keyframes of 45
/// degrees over 1000ms. Continuous Y rotation is otherwise unclaimed here, so it
/// is a genuinely unique motion signature.
const STUN_SPIN_HZ: f32 = 1.0;
const STUN_BOB_AMP: f32 = 0.04;
const STUN_BOB_PERIOD: f32 = 1.6;

/// Hueless on purpose: one look for Cheap Shot, Kidney Shot, Hammer of Justice
/// and Boar Charge alike. Spends nothing from the hue budget and matches the
/// HUD's already-white stun label.
///
/// NOT `unlit`. The unlit branch of `pbr.wgsl` is
/// `out.color = material.base_color` — it discards emissive outright, since
/// emissive is only added inside `apply_pbr_lighting`. An unlit bead is
/// therefore LDR white with nothing for `Bloom::NATURAL` to bloom, which is
/// exactly the flat, un-shining look this treatment must not have. Every
/// glowing effect in this codebase (trap discs, the fear shroud) uses emissive
/// + `AlphaMode::Add` with no `unlit`; `unlit: true` is for things wanting a
/// FLAT colour regardless of arena lighting, like the berserk mask.
const STUN_BEAD_COLOR: Color = Color::srgba(0.95, 0.97, 1.00, 0.90);
const STUN_BEAD_EMISSIVE: LinearRgba = LinearRgba::new(3.0, 3.3, 4.2, 1.0);

/// Each bead carries a larger, dimmer shell so the glow has an explicit
/// falloff instead of a hard-edged additive disc. This is what the design bench
/// drew as a radial gradient out to 2.4x the bead radius, and it is what makes
/// the whirl read as SHINING rather than as a ring of dots. Bloom widens it
/// further; the shell means the look does not depend on bloom being on.
/// Each bead is a camera-facing QUAD carrying a procedural sparkle texture, not
/// a sphere.
///
/// Geometry cannot produce a glow. Any solid mesh — sphere, or a bigger sphere
/// behind it — has a hard silhouette edge, so an additive sphere renders as a
/// flat disc and a stack of them reads as soap bubbles rather than light. The
/// falloff has to live in the texture's alpha, which is what the design bench
/// drew as a radial gradient. The rays are what make it a STAR rather than a
/// dot: a plain round glow still reads as a bubble at this size.
///
/// Billboarded via `rotation = camera.rotation`, the idiom the Berserker Rage
/// glyph already uses. Because the beads are children of a hub that SPINS, the
/// billboard has to counter-rotate by the hub's own rotation — see
/// [`billboard_cc_beads`].
const STUN_SPARKLE_PX: u32 = 128;
/// Half-width of a ray as a fraction of the sprite radius. Narrow: a fat ray
/// reads as a cross, not a glint.
const STUN_RAY_WIDTH: f32 = 0.13;
/// How far the ray reaches relative to the core.
const STUN_RAY_REACH: f32 = 0.55;
/// The quad spans this multiple of the bead's core size, because most of the
/// sprite is the transparent falloff around the core.
const STUN_SPARKLE_SPAN: f32 = 5.0;

// ==============================================================================
// Pure seams
// ==============================================================================

/// Deterministic per-piece variation. The `fear_mote_jitter` hash, NOT
/// `game_rng` — visual-only, so headless byte-identity holds by construction.
pub fn cc_jitter(seed: u32) -> f32 {
    let s = seed
        .wrapping_mul(747_796_405)
        .wrapping_add(2_891_336_453);
    ((s >> 9) & 0xFFFF) as f32 / 65536.0
}

/// Which restraint object a rooted unit wears, chosen from the aura's school so
/// a future root inherits a treatment with no code change. Frost Nova is
/// `spell_school: Frost`, Spider Web is `Nature`; anything else — including an
/// aura that lost its school on the way through `AuraPending::from_ability`,
/// which maps Physical/None to `None` — gets ice.
pub fn root_style(aura: &Aura) -> RootStyle {
    match aura.spell_school {
        Some(SpellSchool::Nature) => RootStyle::Web,
        _ => RootStyle::Ice,
    }
}

/// Grow/retract envelope, shared by both kinds. `age` counts up from spawn;
/// `retract` counts up from the moment the exit was armed and is `None` while
/// the CC is held.
///
/// The grow term is FROZEN at the height the rig had reached when the exit was
/// armed — `age - retract` is exactly the age at that moment. Letting `age` go
/// on feeding the grow term meant a root broken mid-grow briefly kept RISING,
/// because the grow curve outran the retract ramp for the first few frames.
/// Freezing it makes the envelope monotone non-increasing once armed, so a CC
/// broken 50ms after it landed sinks from its partial height instead of
/// bulging first. `age` itself keeps advancing, so the whirl goes on spinning
/// as it fades.
pub fn cc_envelope(
    age: f32,
    retract: Option<f32>,
    grow_secs: f32,
    retract_secs: f32,
) -> f32 {
    let grow_age = match retract {
        None => age,
        Some(r) => (age - r).max(0.0),
    };
    let grown = (grow_age / grow_secs).clamp(0.0, 1.0).sqrt();
    match retract {
        None => grown,
        Some(r) => grown * (1.0 - (r / retract_secs).clamp(0.0, 1.0)),
    }
}

/// How long a rig of this kind takes to retract once its exit is armed.
pub fn retract_secs(kind: CcKind) -> f32 {
    match kind {
        CcKind::Root => ROOT_RETRACT_SECS,
        CcKind::Stun => STUN_RETRACT_SECS,
    }
}

// ==============================================================================
// Rig construction
// ==============================================================================

/// Spawns the hub for one rig with its children already in place, positioned
/// correctly at spawn so nothing is ever seen at the origin for a frame.
fn spawn_rig<B: Bundle>(
    commands: &mut Commands,
    children: Vec<B>,
    owner: Entity,
    kind: CcKind,
    origin: Vec3,
    lift: f32,
) {
    let hub = commands
        .spawn((
            Transform::from_translation(origin).with_scale(Vec3::ZERO),
            Visibility::default(),
            CcRig {
                owner,
                kind,
                age: 0.0,
                retract: None,
                lift,
            },
            PlayMatchEntity,
        ))
        .id();

    // Children are UNMARKED — `despawn()` is recursive, the same reason
    // `SheepPart`'s siblings are untagged. Only the hub carries the marker and
    // the scene tag.
    for child in children {
        let e = commands.spawn(child).id();
        commands.entity(hub).add_child(e);
    }
}

fn build_ice_crystals(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    owner_seed: u32,
) -> Vec<(Mesh3d, MeshMaterial3d<StandardMaterial>, Transform)> {
    let material = materials.add(StandardMaterial {
        base_color: ROOT_ICE_COLOR,
        emissive: ROOT_ICE_EMISSIVE,
        perceptual_roughness: ROOT_ICE_ROUGHNESS,
        ..default()
    });
    // Anchored at the BASE so the cone grows up out of y=0 and the rig's scale
    // envelope reads as the crystal rising from the floor.
    let mesh = meshes.add(
        Cone::new(ROOT_SPIKE_BASE_R, ROOT_SPIKE_HEIGHT)
            .mesh()
            .resolution(ROOT_SPIKE_SIDES)
            .anchor(ConeAnchor::Base),
    );

    (0..ROOT_SPIKE_COUNT)
        .map(|i| {
            let j = cc_jitter(owner_seed.wrapping_add(i.wrapping_mul(31)));
            let theta = (i as f32 / ROOT_SPIKE_COUNT as f32) * TAU + ROOT_SPIKE_PHASE;
            let height_scale = 1.0 - ROOT_SPIKE_JITTER * j;

            let radial = Vec3::new(theta.cos(), 0.0, theta.sin());
            let tip = (Vec3::Y * ROOT_SPIKE_SPLAY.cos() + radial * ROOT_SPIKE_SPLAY.sin())
                .normalize();

            (
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform {
                    translation: radial * ROOT_SPIKE_RING_R,
                    rotation: Quat::from_rotation_arc(Vec3::Y, tip),
                    scale: Vec3::new(1.0, height_scale, 1.0),
                },
            )
        })
        .collect()
}

/// One thread of the web, as a thin cylinder spanning `a` to `b`.
fn thread(
    mesh: &Handle<Mesh>,
    material: &Handle<StandardMaterial>,
    a: Vec3,
    b: Vec3,
) -> (Mesh3d, MeshMaterial3d<StandardMaterial>, Transform) {
    let delta = b - a;
    let len = delta.length().max(1.0e-4);
    (
        Mesh3d(mesh.clone()),
        MeshMaterial3d(material.clone()),
        Transform {
            translation: a + delta * 0.5,
            rotation: Quat::from_rotation_arc(Vec3::Y, delta / len),
            // The unit cylinder is 1.0 tall, so y-scale IS the span length.
            scale: Vec3::new(1.0, len, 1.0),
        },
    )
}

fn build_web_sheet(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    owner_seed: u32,
) -> Vec<(Mesh3d, MeshMaterial3d<StandardMaterial>, Transform)> {
    let material = materials.add(StandardMaterial {
        base_color: WEB_SILK_COLOR,
        emissive: WEB_SILK_EMISSIVE,
        perceptual_roughness: 0.9,
        ..default()
    });
    let mesh = meshes.add(Cylinder::new(WEB_THREAD_R, 1.0));

    // Hem radius varies per spoke so the outline is irregular.
    let hem_at = |i: u32| {
        WEB_HEM_R * (1.0 - WEB_HEM_JITTER * cc_jitter(owner_seed.wrapping_add(i.wrapping_mul(53))))
    };
    // `t` runs 0 at the leg to 1 at the hem.
    let point = |i: u32, t: f32| {
        let theta = (i as f32 / WEB_SPOKES as f32) * TAU + ROOT_SPIKE_PHASE;
        let r = WEB_ATTACH_R + (hem_at(i) - WEB_ATTACH_R) * t;
        Vec3::new(
            theta.cos() * r,
            WEB_ATTACH_H * (1.0 - t).powf(WEB_SAG),
            theta.sin() * r,
        )
    };

    let mut parts = Vec::new();
    // Radial spokes, segmented so the sag curve is a curve.
    for i in 0..WEB_SPOKES {
        for k in 0..WEB_SPOKE_SEGS {
            let t0 = k as f32 / WEB_SPOKE_SEGS as f32;
            let t1 = (k + 1) as f32 / WEB_SPOKE_SEGS as f32;
            parts.push(thread(&mesh, &material, point(i, t0), point(i, t1)));
        }
    }
    // Concentric rings crossing them — the half that makes it read as a web
    // rather than a tent.
    for k in 1..=WEB_RINGS {
        let t = k as f32 / (WEB_RINGS + 1) as f32;
        for i in 0..WEB_SPOKES {
            parts.push(thread(
                &mesh,
                &material,
                point(i, t),
                point((i + 1) % WEB_SPOKES, t),
            ));
        }
    }
    parts
}

/// A four-point sparkle: a soft radial core with narrow rays along both axes,
/// white with the shape carried entirely in the alpha channel.
///
/// Generated rather than shipped as an asset for the same reason
/// `create_surface_texture` is (`play_match/mod.rs:151`) — it is a handful of
/// arithmetic and stays tunable by named constants instead of by reopening an
/// image editor.
fn sparkle_texture() -> Image {
    use bevy::image::Image;
    use bevy::render::render_asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let size = STUN_SPARKLE_PX;
    let mut data = vec![0u8; (size * size * 4) as usize];
    let centre = (size as f32 - 1.0) / 2.0;

    for y in 0..size {
        for x in 0..size {
            // -1..1 across the sprite.
            let dx = (x as f32 - centre) / centre;
            let dy = (y as f32 - centre) / centre;
            let r = (dx * dx + dy * dy).sqrt();

            // Core: smooth falloff to nothing at the rim. The exponent is what
            // keeps it a tight point of light rather than a fog ball.
            let core = (1.0 - r).clamp(0.0, 1.0).powf(2.8);

            // Rays: bright along one axis, pinched hard on the other, fading
            // out along their length.
            let ray = |across: f32, along: f32| {
                let width = (1.0 - across.abs() / STUN_RAY_WIDTH).clamp(0.0, 1.0);
                let reach = (1.0 - along.abs()).clamp(0.0, 1.0).powf(1.8);
                width * width * reach * STUN_RAY_REACH
            };

            let a = (core + ray(dy, dx) + ray(dx, dy)).clamp(0.0, 1.0);
            let i = ((y * size + x) * 4) as usize;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            data[i + 3] = (a * 255.0) as u8;
        }
    }

    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn build_stun_whirl(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    sparkle: Handle<Image>,
) -> Vec<(Mesh3d, MeshMaterial3d<StandardMaterial>, Transform, CcBead)> {
    // The sparkle drives BOTH channels: base_color_texture supplies the alpha
    // that shapes the sprite, and emissive_texture keeps the rays from emitting
    // where the sprite is transparent. `unlit` stays false or the emissive is
    // discarded entirely (see `STUN_BEAD_COLOR`).
    let material = materials.add(StandardMaterial {
        base_color: STUN_BEAD_COLOR,
        base_color_texture: Some(sparkle.clone()),
        emissive: STUN_BEAD_EMISSIVE,
        emissive_texture: Some(sparkle),
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    let mesh = meshes.add(Rectangle::new(1.0, 1.0));

    let mut parts = Vec::new();
    for arm in 0..STUN_ARMS {
        for k in 0..STUN_BEADS_PER_ARM {
            // 0.2 .. 1.0 — no bead sits at the exact centre.
            let t = (k + 1) as f32 / STUN_BEADS_PER_ARM as f32;
            let theta =
                (arm as f32 / STUN_ARMS as f32) * TAU + t * STUN_SPIRAL_TURNS * TAU;
            let r = STUN_R_INNER + (STUN_R_OUTER - STUN_R_INNER) * t;
            let y = STUN_RISE * t;
            // Outer beads are fatter, so an arm overlaps into a continuous
            // glowing streak instead of reading as five separate dots.
            let s = STUN_BEAD_MIN + (STUN_BEAD_MAX - STUN_BEAD_MIN) * t;

            let at = Vec3::new(theta.cos() * r, y, theta.sin() * r);
            parts.push((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(at)
                    .with_scale(Vec3::splat(s * STUN_SPARKLE_SPAN)),
                CcBead,
            ));
        }
    }
    parts
}

fn spawn_cc_flare(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    origin: Vec3,
    end_scale: f32,
) {
    let mesh = meshes.add(Annulus::new(CC_FLARE_INNER_RATIO, 1.0).mesh().resolution(56));
    // Emissive, NOT unlit — see `STUN_BEAD_COLOR` for why the two are
    // mutually exclusive in Bevy's PBR shader.
    let material = materials.add(StandardMaterial {
        base_color: CC_FLARE_COLOR,
        emissive: CC_FLARE_EMISSIVE,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        ..default()
    });
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        // 2D primitives mesh into XY facing +Z; -90 degrees about X lays the
        // ring flat with its normal up.
        Transform::from_translation(origin)
            .with_rotation(Quat::from_rotation_x(-FRAC_PI_2))
            .with_scale(Vec3::splat(CC_FLARE_START)),
        CcFlare {
            lifetime: CC_FLARE_SECS,
            end_scale,
        },
        PlayMatchEntity,
    ));
}

// ==============================================================================
// Systems
// ==============================================================================

/// The single marker owner for BOTH `RootedVisual` and `StunnedVisual`.
///
/// One system evaluating both predicates in its body is a deliberate improvement
/// on the Fear/Polymorph pair, which needs a `.chain()` in `states/mod.rs` to
/// avoid a deadlock: each inserts a marker the other's `Without` excludes, and
/// marker inserts are deferred `Command`s, so a same-frame double-hit marks both
/// permanently and neither can restore. Root and Stun genuinely CO-HOLD (separate
/// DR categories, disjoint space, both must show at once), so that case would
/// otherwise need the same fix — evaluating both in-body has no deferred-Command
/// race at all.
///
/// `ActiveAuras` is OPTIONAL because `update_auras` REMOVES the component once
/// the last aura expires; a query that required it would never observe natural
/// expiry. `is_alive()` is folded into both predicates because
/// `process_aura_breaks` skips the dead, so a killing blow leaves the aura
/// ticking out on the corpse. There is deliberately no `Without<DeathAnimation>`
/// filter, so the death sink and this restore compose in the same frame.
pub fn update_hard_cc_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    // The sparkle is identical for every bead of every stun, so it is built
    // once and the handle reused rather than regenerating 64KB per application.
    mut sparkle: Local<Option<Handle<Image>>>,
    combatants: Query<(
        Entity,
        &Combatant,
        &Transform,
        Option<&ActiveAuras>,
        Option<&RootedVisual>,
        Option<&StunnedVisual>,
        Option<&Pet>,
        Option<&Children>,
    )>,
    bodies: Query<&VisualBody>,
    mut rigs: Query<(&mut CcRig,)>,
) {
    for (entity, combatant, transform, auras, rooted, stunned_marker, pet, children) in
        combatants.iter()
    {
        let alive = combatant.is_alive();
        let root_aura = if alive {
            auras.and_then(|a| {
                a.auras
                    .iter()
                    .find(|au| au.effect_type == AuraType::Root)
            })
        } else {
            None
        };
        let is_stunned = alive
            && auras.is_some_and(|a| a.auras.iter().any(|au| au.effect_type == AuraType::Stun));

        let is_pet = pet.is_some();
        let stature = if is_pet { CC_PET_STATURE } else { 1.0 };
        let seed = entity.index();

        // The whirl anchors off the RENDERED body, not the sim entity — see
        // `STUN_LIFT_ABOVE_BODY`. Absent a body child, `rest_y` of 0 degrades to
        // the sim y, which is correct for a combatant.
        let rest_y = children
            .and_then(|cs| cs.iter().find_map(|c| bodies.get(c).ok()))
            .map_or(0.0, |b| b.rest_y);
        let stun_lift = rest_y
            + if is_pet {
                STUN_LIFT_ABOVE_BODY_PET
            } else {
                STUN_LIFT_ABOVE_BODY
            };

        // Whether a rig of this kind is currently HELD (spawned and not
        // retracting). Spawning is gated on this rather than on the marker's
        // absence, so the treatment RECONCILES: if a rig is despawned out from
        // under us while the aura still holds, the next frame rebuilds it. The
        // reachable case is the animation sandbox's `clear_body_state`, whose
        // leftover sweep despawns every `PlayMatchEntity` that is not a
        // `SandboxEntity` — which matches the rig hub — while leaving the marker
        // on the unit. Gating on the marker alone made that desync permanent for
        // the rest of the CC.
        let held = |rigs: &Query<(&mut CcRig,)>, kind: CcKind| {
            rigs.iter()
                .any(|(r,)| r.owner == entity && r.kind == kind && r.retract.is_none())
        };
        let root_held = held(&rigs, CcKind::Root);
        let stun_held = held(&rigs, CcKind::Stun);

        // Arms the retract on this unit's rig of the given kind. Filters on BOTH
        // owner and kind, so two rooted units never strip each other's rig and a
        // root and a stun on one unit never strip each other's.
        let arm_retract = |rigs: &mut Query<(&mut CcRig,)>, kind: CcKind| {
            for (mut rig,) in rigs.iter_mut() {
                if rig.owner == entity && rig.kind == kind && rig.retract.is_none() {
                    rig.retract = Some(0.0);
                }
            }
        };

        // ---- Root ----
        if let Some(aura) = root_aura {
            let style = root_style(aura);
            // A CC replacement can swap the aura within one tick, so the marker
            // disagreeing with the aura's school is a rebuild, not drift.
            let style_changed = rooted.is_some_and(|m| m.style != style);
            if style_changed || !root_held {
                if style_changed {
                    arm_retract(&mut rigs, CcKind::Root);
                }
                let parts = match style {
                    RootStyle::Ice => build_ice_crystals(&mut meshes, &mut materials, seed),
                    RootStyle::Web => build_web_sheet(&mut meshes, &mut materials, seed),
                };
                let origin =
                    Vec3::new(transform.translation.x, CC_GROUND_Y, transform.translation.z);
                spawn_rig(&mut commands, parts, entity, CcKind::Root, origin, 0.0);
                commands.entity(entity).try_insert(RootedVisual { style });
                // Only on a genuine appearance or a style swap. A silent
                // reconcile must not pop a second flare.
                if rooted.is_none() || style_changed {
                    spawn_cc_flare(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        origin + Vec3::Y * 0.01,
                        CC_FLARE_END_GROUND * stature,
                    );
                }
            }
        } else if rooted.is_some() {
            commands.entity(entity).remove::<RootedVisual>();
            arm_retract(&mut rigs, CcKind::Root);
        }

        // ---- Stun ----
        if is_stunned {
            if !stun_held {
                let tex = sparkle
                    .get_or_insert_with(|| images.add(sparkle_texture()))
                    .clone();
                let parts = build_stun_whirl(&mut meshes, &mut materials, tex);
                let origin = transform.translation + Vec3::Y * stun_lift;
                spawn_rig(&mut commands, parts, entity, CcKind::Stun, origin, stun_lift);
                commands.entity(entity).try_insert(StunnedVisual);
                if stunned_marker.is_none() {
                    spawn_cc_flare(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        origin,
                        CC_FLARE_END_HEAD * stature,
                    );
                }
            }
        } else if stunned_marker.is_some() {
            commands.entity(entity).remove::<StunnedVisual>();
            arm_retract(&mut rigs, CcKind::Stun);
        }
    }
}

/// Follows the owner, ages the rig, and applies the envelope, spin and bob.
///
/// `Without<Combatant>` on the rig query is load-bearing: two `Transform`
/// queries conflict at Bevy's STATIC access check even though the archetypes are
/// disjoint at runtime, and the result is a B0001 panic at schedule init. This
/// is the idiom `update_ice_blocks` already uses.
pub fn update_cc_rigs(
    time: Res<Time>,
    mut rigs: Query<(&mut CcRig, &mut Transform), Without<Combatant>>,
    owners: Query<(&Transform, Option<&Pet>), With<Combatant>>,
) {
    // `rig.lift` already folds in the owner's `rest_y`, resolved at spawn.
    let dt = time.delta_secs();

    for (mut rig, mut transform) in rigs.iter_mut() {
        rig.age += dt;
        if let Some(r) = rig.retract.as_mut() {
            *r += dt;
        }

        let Ok((owner_transform, pet)) = owners.get(rig.owner) else {
            // Owner despawned — `cleanup_cc_rigs` drains it; nothing to follow.
            continue;
        };
        let is_pet = pet.is_some();
        let stature = if is_pet { CC_PET_STATURE } else { 1.0 };

        match rig.kind {
            CcKind::Root => {
                let e = cc_envelope(rig.age, rig.retract, ROOT_GROW_SECS, ROOT_RETRACT_SECS);
                transform.translation = Vec3::new(
                    owner_transform.translation.x,
                    CC_GROUND_Y,
                    owner_transform.translation.z,
                );
                transform.scale = Vec3::splat(stature * e);
            }
            CcKind::Stun => {
                let e = cc_envelope(rig.age, rig.retract, STUN_GROW_SECS, STUN_RETRACT_SECS);
                let bob = STUN_BOB_AMP * (rig.age * TAU / STUN_BOB_PERIOD).sin();
                transform.translation = Vec3::new(
                    owner_transform.translation.x,
                    owner_transform.translation.y + rig.lift + bob,
                    owner_transform.translation.z,
                );
                // Wall-clock driven, NEVER sim displacement: the whirl must turn
                // smoothly over a perfectly stationary victim. See
                // `fixed-timestep-visual-strobe.md`.
                transform.rotation = Quat::from_rotation_y(-rig.age * TAU * STUN_SPIN_HZ);
                transform.scale = Vec3::splat(stature * e);
            }
        }
    }
}

/// Turns every sparkle to face the camera.
///
/// A quad is only a glowing point of light while it is face-on; edge-on it
/// vanishes. The beads are children of a hub that spins about Y, so copying the
/// camera's rotation directly is not enough — the parent's rotation composes on
/// top of it and the sprites would counter-spin visibly. Pre-multiplying by the
/// hub's inverse cancels it, leaving each bead world-aligned to the camera while
/// the hub goes on carrying them around the orbit.
///
/// The hub is a root entity, so its local rotation IS its world rotation.
pub fn billboard_cc_beads(
    camera: Query<&Transform, (With<Camera3d>, Without<CcRig>, Without<CcBead>)>,
    rigs: Query<(&CcRig, &Transform, &Children), Without<CcBead>>,
    mut beads: Query<&mut Transform, With<CcBead>>,
) {
    let Some(cam) = camera.iter().next() else {
        return;
    };
    for (rig, hub, children) in rigs.iter() {
        if rig.kind != CcKind::Stun {
            continue;
        }
        let facing = hub.rotation.inverse() * cam.rotation;
        for child in children.iter() {
            if let Ok(mut bead) = beads.get_mut(child) {
                bead.rotation = facing;
            }
        }
    }
}

/// Grows and fades the apply flare.
pub fn update_cc_flares(
    time: Res<Time>,
    mut flares: Query<(
        &mut CcFlare,
        &mut Transform,
        &MeshMaterial3d<StandardMaterial>,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();

    for (mut flare, mut transform, material_handle) in flares.iter_mut() {
        flare.lifetime -= dt;
        let progress = (flare.lifetime / CC_FLARE_SECS).clamp(0.0, 1.0);
        let elapsed = 1.0 - progress;

        let scale = CC_FLARE_START + (flare.end_scale - CC_FLARE_START) * elapsed.sqrt();
        transform.scale = Vec3::splat(scale);

        if let Some(material) = materials.get_mut(&material_handle.0) {
            // Squared, so the ring thins out early and the expansion reads as
            // dissipation rather than a growing solid.
            let fade = progress * progress;
            material.base_color = CC_FLARE_COLOR.with_alpha(CC_FLARE_COLOR.alpha() * fade);
            material.emissive = LinearRgba::new(
                CC_FLARE_EMISSIVE.red * fade,
                CC_FLARE_EMISSIVE.green * fade,
                CC_FLARE_EMISSIVE.blue * fade,
                1.0,
            );
        }
    }
}

/// Despawns rigs that have finished retracting, and any rig whose owner is gone.
pub fn cleanup_cc_rigs(
    mut commands: Commands,
    rigs: Query<(Entity, &CcRig)>,
    owners: Query<(), With<Combatant>>,
) {
    for (entity, rig) in rigs.iter() {
        let finished = rig
            .retract
            .is_some_and(|r| r >= retract_secs(rig.kind));
        // Recursive despawn takes the unmarked children with it.
        if finished || owners.get(rig.owner).is_err() {
            commands.entity(entity).despawn();
        }
    }
}

/// Despawns expired flares.
pub fn cleanup_cc_flares(mut commands: Commands, flares: Query<(Entity, &CcFlare)>) {
    for (entity, flare) in flares.iter() {
        if flare.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}
