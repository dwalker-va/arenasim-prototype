use bevy::color::LinearRgba;
use bevy::prelude::*;

use crate::states::play_match::components::*;

// ==============================================================================
// Rogue Crescent Flares — Cheap Shot and Kidney Shot
// ==============================================================================
//
// The two rogue stuns are byte-identical on the receiver side (both apply a
// plain 4s/6s `Stun`, both get the whirl shipped in section A) and differ
// ENTIRELY on the caster side. From the Classic client data:
//
//   Cheap Shot   `Attack1H` swing, 634ms.  FOUR crescents at HEAD height,
//                UNTINTED WHITE, two-stroke: two pop immediately, two ~100ms
//                later. No particles. It shares its visual with Sap.
//   Kidney Shot  `Attack1HPierce` lunge, 1233ms.  THREE crescents at TORSO
//                height, staggered much wider, tinted magenta -> white flash ->
//                deep crimson. Its own cast model, used by no other ability.
//
// So the crescent is the shared vocabulary and everything about how it is
// deployed is per-ability: count, height, stagger, colour, lifetime. This
// module owns the geometry and the animation; each ability supplies a
// [`CrescentSpec`].
//
// The crescent is a camera-facing QUAD carrying a procedural arc texture, not a
// mesh. The shape needs soft edges along both its length and its thickness, and
// geometry cannot produce those — the same lesson the stun whirl's sparkle
// taught (`hard_cc.rs`). The alpha lives in the texture.
//
// Graphical-only: registered in `states/mod.rs` alone, keyed on the
// `InstantAbilityFired` marker the class AI spawns. No `game_rng` draw, no sim
// write — headless stays byte-identical.

/// Resolution of the procedural crescent texture.
const CRESCENT_PX: u32 = 128;
/// Radius of the arc's spine, as a fraction of the sprite half-width.
const CRESCENT_ARC_R: f32 = 0.66;
/// Thickness of the stroke, as a fraction of the sprite half-width. The
/// gaussian falloff around the spine is what makes the edge soft.
const CRESCENT_THICKNESS: f32 = 0.15;
/// Half-angle the arc spans, in radians. Beyond ~1.4 it closes toward a full
/// ring and stops reading as a slash.
const CRESCENT_SPAN: f32 = 1.15;
/// How sharply the stroke tapers to nothing at the two ends of the arc.
const CRESCENT_TAPER: f32 = 2.2;

/// Per-ability deployment of the shared crescent.
#[derive(Clone, Copy)]
pub struct CrescentSpec {
    /// How many crescents the flare spawns.
    pub count: u32,
    /// Height above the caster's SIM origin. A combatant's body centre is its
    /// sim y, its crown +1.25 and its feet -1.25, so head height is ~+0.9 and
    /// torso ~+0.15.
    pub height: f32,
    /// Distance in front of the caster, along the line to the target.
    pub reach: f32,
    /// Seconds between successive crescents appearing.
    pub stagger: f32,
    /// How long each crescent lives.
    pub lifetime: f32,
    /// World size of one crescent.
    pub size: f32,
    /// Roll applied to crescent `i`, so the fan is not a stack of parallel
    /// copies. Radians, multiplied by the index.
    pub roll_step: f32,
    /// Tint at birth.
    pub color: Color,
    /// Tint at death. Cheap Shot passes the same value twice (the source has
    /// ZERO colour tracks — it is untinted white); Kidney Shot travels from
    /// magenta to crimson.
    pub color_end: Color,
    pub emissive: LinearRgba,
}

/// Cheap Shot: four untinted white crescents at head height in two quick
/// strokes. Fast and colourless, which is exactly what the source is.
pub const CHEAP_SHOT_CRESCENTS: CrescentSpec = CrescentSpec {
    count: 4,
    height: 0.90,
    reach: 0.85,
    // Source: bone 0 pops at 0-100ms, bone 1 follows at 100-200ms. Two pairs,
    // 100ms apart — the stagger is per crescent, so pairs fall out of 0.05.
    stagger: 0.05,
    lifetime: 0.30,
    size: 1.10,
    roll_step: 0.55,
    color: Color::srgba(0.97, 0.98, 1.00, 0.85),
    color_end: Color::srgba(0.97, 0.98, 1.00, 0.85),
    emissive: LinearRgba::new(2.6, 2.7, 3.0, 1.0),
};

/// The procedural arc: a gaussian band along a circular spine, tapering to
/// nothing at both ends. White, with the whole shape carried in the alpha.
///
/// Generated rather than shipped as an asset for the same reason
/// `create_surface_texture` and the stun sparkle are — a handful of arithmetic
/// that stays tunable by named constants instead of by reopening an editor.
fn crescent_texture() -> Image {
    use bevy::image::Image;
    use bevy::render::render_asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let size = CRESCENT_PX;
    let mut data = vec![0u8; (size * size * 4) as usize];
    let centre = (size as f32 - 1.0) / 2.0;

    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 - centre) / centre;
            let dy = (y as f32 - centre) / centre;
            let r = (dx * dx + dy * dy).sqrt();
            // Angle from straight UP, signed. Image y grows downward, so the
            // negation is what puts the arc's crown at the top of the sprite
            // rather than the bottom.
            let theta = dx.atan2(-dy);

            // Distance from the arc's spine, as a fraction of its thickness.
            let d = (r - CRESCENT_ARC_R) / CRESCENT_THICKNESS;
            let band = (-(d * d)).exp();
            // Taper to nothing at both ends of the span.
            let t = (theta.abs() / CRESCENT_SPAN).min(1.0);
            let along = (1.0 - t).powf(CRESCENT_TAPER).clamp(0.0, 1.0);

            let a = (band * along).clamp(0.0, 1.0);
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

/// Spawns one ability's crescent fan.
///
/// `caster_pos` and `aim` are world positions; the fan is planted between them,
/// which is where the source sweeps its crescents ("across the front of the
/// caster"). With no target — not a case either rogue stun has — the fan falls
/// back to the caster's own facing-agnostic +Z.
#[allow(clippy::too_many_arguments)]
pub fn spawn_crescent_fan(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    texture: &mut Option<Handle<Image>>,
    spec: CrescentSpec,
    caster_pos: Vec3,
    aim: Option<Vec3>,
) {
    let tex = texture
        .get_or_insert_with(|| images.add(crescent_texture()))
        .clone();
    let mesh = meshes.add(Rectangle::new(1.0, 1.0));

    let forward = aim
        .map(|a| (a - caster_pos).with_y(0.0))
        .filter(|v| v.length_squared() > 1.0e-4)
        .map(|v| v.normalize())
        .unwrap_or(Vec3::Z);

    for i in 0..spec.count {
        let origin = caster_pos + Vec3::Y * spec.height + forward * spec.reach;
        // Fan the crescents apart so they read as successive slashes rather
        // than one thick smear.
        let roll = (i as f32 - (spec.count as f32 - 1.0) * 0.5) * spec.roll_step;
        let material = materials.add(StandardMaterial {
            base_color: spec.color,
            base_color_texture: Some(tex.clone()),
            emissive: spec.emissive,
            emissive_texture: Some(tex.clone()),
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            double_sided: true,
            ..default()
        });
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(origin).with_scale(Vec3::splat(spec.size)),
            CrescentFlare {
                delay: spec.stagger * i as f32,
                age: 0.0,
                lifetime: spec.lifetime,
                roll,
                size: spec.size,
                color: spec.color,
                color_end: spec.color_end,
                emissive: spec.emissive,
            },
            PlayMatchEntity,
        ));
    }
}

/// Billboards, pops and fades every live crescent.
///
/// The quad faces the camera and is then rolled about the view axis by its own
/// `roll`, so the fan spreads across the screen rather than around the world —
/// a slash reads by its screen-space direction, and a world-space fan would
/// collapse to a line from the wrong angle.
pub fn update_crescent_flares(
    time: Res<Time>,
    camera: Query<&Transform, (With<Camera3d>, Without<CrescentFlare>)>,
    mut flares: Query<(
        &mut CrescentFlare,
        &mut Transform,
        &MeshMaterial3d<StandardMaterial>,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();
    let cam_rot = camera.iter().next().map(|t| t.rotation);

    for (mut flare, mut transform, handle) in flares.iter_mut() {
        flare.age += dt;
        // Held back until its turn in the stagger — invisible, not absent, so
        // the spawn site stays one loop.
        if flare.age < flare.delay {
            transform.scale = Vec3::ZERO;
            continue;
        }
        let k = ((flare.age - flare.delay) / flare.lifetime).clamp(0.0, 1.0);

        // Snap open, then thin out. The source scales 1.3 -> 1.0 -> 1.3, so the
        // crescent arrives already large, tightens, and blows out as it dies.
        let scale = if k < 0.25 {
            1.30 - 0.30 * (k / 0.25)
        } else {
            1.00 + 0.30 * ((k - 0.25) / 0.75)
        };
        transform.scale = Vec3::splat(flare.size * scale);

        if let Some(rot) = cam_rot {
            transform.rotation = rot * Quat::from_rotation_z(flare.roll);
        }

        if let Some(material) = materials.get_mut(&handle.0) {
            // Squared fade so the stroke thins early rather than lingering.
            let fade = (1.0 - k) * (1.0 - k);
            let from = flare.color.to_srgba();
            let to = flare.color_end.to_srgba();
            material.base_color = Color::srgba(
                from.red + (to.red - from.red) * k,
                from.green + (to.green - from.green) * k,
                from.blue + (to.blue - from.blue) * k,
                from.alpha * fade,
            );
            material.emissive = LinearRgba::new(
                flare.emissive.red * fade,
                flare.emissive.green * fade,
                flare.emissive.blue * fade,
                1.0,
            );
        }
    }
}

/// Despawns crescents whose stroke has played out.
pub fn cleanup_crescent_flares(mut commands: Commands, flares: Query<(Entity, &CrescentFlare)>) {
    for (entity, flare) in flares.iter() {
        if flare.age >= flare.delay + flare.lifetime {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crescent_texture_is_a_hollow_arc() {
        let img = crescent_texture();
        let size = CRESCENT_PX as usize;
        let alpha_at = |x: usize, y: usize| img.data.as_ref().unwrap()[(y * size + x) * 4 + 3];
        let c = size / 2;

        // Hollow: the centre of the sprite is empty, because the stroke lives
        // out on a circular spine.
        assert_eq!(alpha_at(c, c), 0, "a crescent must not be a filled blob");
        // The spine is bright directly above centre (theta = 0).
        let spine_y = c - (CRESCENT_ARC_R * (c as f32)) as usize;
        assert!(
            alpha_at(c, spine_y) > 200,
            "the arc's spine should be near-opaque"
        );
        // And nothing survives at the opposite side, past the span.
        let opposite_y = c + (CRESCENT_ARC_R * (c as f32)) as usize;
        assert_eq!(
            alpha_at(c, opposite_y),
            0,
            "the arc must taper out, not close into a ring"
        );
    }

    #[test]
    fn cheap_shots_crescents_are_untinted() {
        // The source has ZERO colour tracks on Cheap Shot's model — it is white
        // throughout, which is the sharpest contrast with Kidney Shot's magenta.
        assert_eq!(
            CHEAP_SHOT_CRESCENTS.color, CHEAP_SHOT_CRESCENTS.color_end,
            "Cheap Shot's crescents must not travel through a tint"
        );
    }

    #[test]
    fn the_fan_fits_inside_the_stroke() {
        // Every crescent must be born and dead within the swing that spawned
        // it, or the flare outlives the animation it belongs to.
        let spec = CHEAP_SHOT_CRESCENTS;
        let last = spec.stagger * (spec.count - 1) as f32 + spec.lifetime;
        let stroke = SwingStyle::CheapShot.stroke_secs();
        assert!(
            last <= stroke,
            "fan runs {last}s but the stroke is only {stroke}s"
        );
    }

    #[test]
    fn the_arc_span_stays_a_slash() {
        // Past ~1.4 rad the taper windows overlap and the arc closes toward a
        // full ring, which reads as a halo rather than a stroke.
        assert!(CRESCENT_SPAN < 1.4, "span has closed into a ring");
        assert!(CRESCENT_SPAN > 0.5, "span has collapsed to a dot");
    }

    #[test]
    fn a_crescent_is_wider_than_it_is_thick() {
        // The arc length at the spine must exceed the stroke thickness, or the
        // "crescent" is really just a smudge.
        let arc_len = 2.0 * CRESCENT_SPAN * CRESCENT_ARC_R;
        assert!(
            arc_len > CRESCENT_THICKNESS * 3.0,
            "arc {arc_len} is not meaningfully longer than its {CRESCENT_THICKNESS} thickness"
        );
    }
}
