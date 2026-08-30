use bevy::color::LinearRgba;
use bevy::prelude::*;

use std::f32::consts::TAU;

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
/// Half-thickness of the streak at its middle, as a fraction of the sprite
/// half-width. The gaussian falloff around the spine is what softens the edge.
/// At 0.075 the stroke is about 6.7:1 — a slash. See `crescent_aspect`.
///
/// The streak runs the FULL width of the sprite (2.0 units), so this also sets
/// the aspect ratio: at 0.09 the stroke is roughly 11:1, which is a slash. The
/// earlier circular-arc parametrisation could not get past about 2:1 no matter
/// how it was tuned — see the module note above — and rendered as a pill.
const CRESCENT_THICKNESS: f32 = 0.075;
/// Fraction of that thickness still present at the very tips. A slash is fat in
/// the middle and fine at the points; a constant-width band is a bar.
const CRESCENT_TIP: f32 = 0.10;
/// How far the streak bows off straight, as a fraction of the sprite half-width.
/// The WoW reference is very nearly a straight cut with just enough curve to
/// read as a sweep rather than a laser.
const CRESCENT_BOW: f32 = 0.20;
/// How sharply brightness falls off toward the two ends.
///
/// Applied to the same elliptical profile that shapes the width, so the stroke
/// holds its weight along most of its length and gives way near the tips.
const CRESCENT_TAPER: f32 = 1.35;

/// The streak's length-to-thickness ratio, exposed so a probe can assert the
/// SHAPE. A slash is long and fine; anything approaching square is a pill.
pub fn crescent_aspect() -> f32 {
    // The stroke spans the sprite's full 2.0 width; the visible band is roughly
    // two sigma either side of the spine.
    2.0 / (CRESCENT_THICKNESS * 4.0)
}

/// How far the streak bows off straight, for the "a cut, not a laser" check.
pub fn crescent_bow() -> f32 {
    CRESCENT_BOW
}

/// Where an ability's crescents are arranged.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrescentLayout {
    /// Swept across the front of the CASTER, along its line of aim — Kidney
    /// Shot's spray of slashes.
    CasterSweep,
    /// A flat ring hovering above the VICTIM's head, tilted off horizontal, its
    /// crescents overlapping around the ellipse.
    ///
    /// Cheap Shot shares SpellVisualKit 411 with Sap, and the reference for that
    /// kit is unmistakable: a magenta-and-white ring floating over the target's
    /// skull, not anything at the caster. Gouge — the other rogue incapacitate —
    /// shows the same ring, so this is the incapacitate family's own mark.
    VictimHalo,
}

/// Per-ability deployment of the shared crescent.
#[derive(Clone, Copy)]
pub struct CrescentSpec {
    /// Where the fan is arranged, and around whom.
    pub layout: CrescentLayout,
    /// How many crescents the flare spawns.
    pub count: u32,
    /// Height above the ANCHOR unit's SIM origin — the caster for a
    /// `CasterSweep`, the victim for a `VictimHalo`. A combatant's body centre
    /// is its sim y, its crown +1.25 and its feet -1.25, so head height is
    /// ~+0.9 and torso ~+0.15.
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
    /// Total lateral width the fan is swept across, in yards, centred on the
    /// caster's line of aim.
    ///
    /// A combatant capsule is 1.0yd across, so anything under that reads as a
    /// clump beside the body rather than a slash through it — which is what the
    /// first version did by spawning every crescent at one point.
    pub spread: f32,
    /// Tint at birth.
    pub color: Color,
    /// Tint at [`CRESCENT_FLASH_AT`] through the crescent's life. The source's
    /// Kidney Shot spikes to a white-pink at 200ms of a 1233ms model before
    /// settling to crimson, so a two-stop lerp cannot describe it.
    pub color_mid: Color,
    /// Tint at death. Cheap Shot passes the same value for all three (the
    /// source has ZERO colour tracks on its model — it is untinted white);
    /// Kidney Shot travels magenta -> flash -> deep crimson.
    pub color_end: Color,
    pub emissive: LinearRgba,
}

/// How far the victim halo tilts off horizontal, as a fraction of its radius.
/// The reference ring is tipped ~20-30 degrees, which is what makes it read as
/// a ring rather than a flat line when seen from the game camera.
const HALO_TILT: f32 = 0.42;

/// Where in a crescent's life the mid tint peaks. The source flashes early —
/// 200ms into a 1233ms model — so the spike belongs near the front of the
/// stroke, not the middle.
const CRESCENT_FLASH_AT: f32 = 0.22;

/// Cheap Shot: four untinted white crescents at head height in two quick
/// strokes. Fast and colourless, which is exactly what the source is.
pub const CHEAP_SHOT_CRESCENTS: CrescentSpec = CrescentSpec {
    layout: CrescentLayout::VictimHalo,
    count: 4,
    // Above the victim's crown (2.25) with a little clear air, matching the
    // half-to-one head-height gap in the reference.
    height: 1.55,
    // Radius of the halo ring, not a forward reach. The reference ring is
    // 2.5-2.8x head width; this sits above that on purpose. At the reference
    // proportion the halo was geometrically faithful and still easy to miss
    // under the stun whirl that follows it — and the whirl runs 4-6s against
    // this flash's fraction of a second, so the flash has to win its moment or
    // it may as well not fire.
    reach: 1.05,
    // Source: bone 0 pops at 0-100ms, bone 1 follows at 100-200ms. Two pairs,
    // 100ms apart — the stagger is per crescent, so pairs fall out of 0.05.
    stagger: 0.05,
    // Long enough to register before the whirl takes over. The source's own
    // 634ms is the whole cast; this is one crescent's dwell within it.
    lifetime: 0.42,
    // Each crescent's world length. Must keep pace with `reach` — see
    // `the_halo_crescents_close_the_ring`: four bands too short for the
    // circumference read as disconnected dashes rather than a ring.
    size: 1.75,
    roll_step: 0.40,
    // Slightly wider than the 1.0yd body, so the pair-and-pair strokes cross it
    // rather than sitting on one shoulder.
    spread: 1.45,
    // NOT untinted white. The DB2 read said "zero colour tracks", which means
    // no ANIMATED tint — the base texture itself is white-and-magenta, and the
    // sampled reference is #F8C8F8 fringing to #F0A0E8 against three different
    // ambients, so the magenta is intrinsic rather than borrowed from the scene.
    color: Color::srgba(0.97, 0.80, 0.97, 0.88),
    color_mid: Color::srgba(1.00, 0.95, 1.00, 0.92),
    color_end: Color::srgba(0.94, 0.63, 0.91, 0.85),
    emissive: LinearRgba::new(3.0, 1.9, 2.9, 1.0),
};

/// Kidney Shot: three crescents at TORSO height, spread much wider than Cheap
/// Shot's pairs, travelling magenta -> white-pink flash -> deep crimson.
///
/// The colours are the source's measured keyframes: `#F53BB1` -> `#FFD1FF` at
/// 200ms -> `#F00764` at 566ms. They were the one surprise in the research and
/// are uncorroborated by any second source, so they are worth a spot-check in a
/// real client — but hot pink sits well clear of Mortal Strike's dark crimson
/// trail, and the silhouettes differ anyway (torso crescents against a weapon
/// ribbon).
pub const KIDNEY_SHOT_CRESCENTS: CrescentSpec = CrescentSpec {
    layout: CrescentLayout::CasterSweep,
    count: 3,
    height: 0.10,
    reach: 0.95,
    // Source alpha peaks at 466 / 633 / 800ms — ~167ms apart.
    stagger: 0.167,
    lifetime: 0.45,
    size: 1.35,
    roll_step: 0.55,
    // Wider than Cheap Shot's: the finisher's slashes carry further across.
    spread: 1.80,
    color: Color::srgba(0.96, 0.23, 0.69, 0.90),
    color_mid: Color::srgba(1.00, 0.82, 1.00, 0.95),
    color_end: Color::srgba(0.94, 0.03, 0.39, 0.90),
    emissive: LinearRgba::new(3.0, 0.8, 2.0, 1.0),
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
            // Sprite space: both axes -1..1. The streak runs along X, which is
            // also the axis `update_crescent_flares` turns to follow the aim.
            let dx = (x as f32 - centre) / centre;
            let dy = (y as f32 - centre) / centre;

            // Distance from the middle toward either tip.
            let t = dx.abs().min(1.0);
            // Elliptical profile: full at the belly, easing to nothing at the
            // tips. Shapes BOTH the width and the brightness, so the stroke
            // narrows and fades together the way a real slash does.
            let profile = (1.0 - t * t).max(0.0).sqrt();

            // The spine bows off the straight line, deepest at the middle.
            let spine = CRESCENT_BOW * profile;
            let half_width =
                CRESCENT_THICKNESS * (CRESCENT_TIP + (1.0 - CRESCENT_TIP) * profile);

            let d = (dy - spine) / half_width.max(1.0e-4);
            let band = (-(d * d)).exp();
            let along = profile.powf(CRESCENT_TAPER);

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

    // A halo belongs to the VICTIM; a sweep belongs to the caster. Without a
    // target the halo has nobody to sit above, so it falls back to the caster
    // rather than being dropped — a stun with no visual at all is worse than one
    // in the wrong place.
    let anchor = match spec.layout {
        CrescentLayout::VictimHalo => aim.unwrap_or(caster_pos),
        CrescentLayout::CasterSweep => caster_pos,
    };

    // Lateral axis across the caster's front. The crescents are SWEPT across
    // this, which is what the source does ("crescent quads swept across the
    // front of the caster") and what makes the flare read as a body-wide arc.
    // Spawning them all at one point and fanning only their ROLL — the first
    // version — produced a rosette clumped to one side of the body instead.
    let across = Vec3::Y.cross(forward).normalize_or_zero();

    for i in 0..spec.count {
        // -0.5 .. +0.5 over the fan, or 0 for a single crescent.
        let t = if spec.count > 1 {
            i as f32 / (spec.count - 1) as f32 - 0.5
        } else {
            0.0
        };
        let origin = match spec.layout {
            // Around the ring, at `reach` radius, tilted off horizontal so it
            // reads as an ellipse from the game camera rather than a line.
            CrescentLayout::VictimHalo => {
                let a = i as f32 / spec.count as f32 * TAU;
                anchor
                    + Vec3::Y * spec.height
                    + Vec3::new(a.cos(), 0.0, a.sin()) * spec.reach
                    + Vec3::Y * (a.sin() * HALO_TILT * spec.reach)
            }
            CrescentLayout::CasterSweep => {
                anchor
                    + Vec3::Y * spec.height
                    + forward * spec.reach
                    + across * (t * spec.spread)
            }
        };
        // Each crescent is also rolled, so successive slashes are angled rather
        // than a picket fence of parallel copies. The spread does the work of
        // covering the body; the roll only keeps them from looking stamped.
        let roll = match spec.layout {
            // Each crescent lies tangent to the ring, so together they trace the
            // ellipse instead of pointing every which way.
            CrescentLayout::VictimHalo => {
                i as f32 / spec.count as f32 * TAU + spec.roll_step
            }
            CrescentLayout::CasterSweep => {
                (i as f32 - (spec.count as f32 - 1.0) * 0.5) * spec.roll_step
            }
        };
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
                // The slash travels ACROSS the body, not along the aim.
                sweep: across,
                delay: spec.stagger * i as f32,
                age: 0.0,
                lifetime: spec.lifetime,
                roll,
                size: spec.size,
                color: spec.color,
                color_mid: spec.color_mid,
                color_end: spec.color_end,
                emissive: spec.emissive,
            },
            PlayMatchEntity,
        ));
    }
}

/// Billboards, pops and fades every live crescent.
///
/// The quad faces the camera, is turned so its LONG AXIS runs along the slash's
/// SWEEP as seen on screen, and is then rolled about the view axis by its own
/// `roll` to fan the strokes apart.
///
/// That middle step is what makes it read as a cut, and which axis it uses
/// matters more than it looks. A blade sweeps ACROSS a body, right to left —
/// not along the line to the target. Aligning to the aim instead put the
/// streaks vertical: for the usual camera, looking down the line of attack from
/// behind and above, the aim projects to almost exactly screen-UP, so the
/// slashes ran head to toe down the victim.
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
            // SCREEN-HORIZONTAL, plus the fan's own roll. The quad's local +X is
            // screen-right once billboarded, so this needs no extra term at all.
            //
            // Two world axes were tried first and both fail from some bearings,
            // because projecting a horizontal world vector onto the screen goes
            // degenerate whenever that vector points near the view axis. Aiming
            // along the AIM put the slashes head-to-toe for the usual
            // over-the-shoulder camera (the aim projects to almost exactly
            // screen-up). Aiming along the across-body SWEEP fixed that case and
            // broke the side-on one, where the sweep points into the screen
            // instead. There is no world axis that reads from every angle.
            //
            // The sprite is already billboarded — it faces the camera whatever
            // the world is doing — so orienting it in screen space is consistent
            // with what it fundamentally is, and it reads as a slash from every
            // bearing. `sweep` is kept on the component because it still records
            // which way the blade travelled, and a future non-billboarded
            // treatment would want it.
            transform.rotation = rot * Quat::from_rotation_z(flare.roll);
        }

        if let Some(material) = materials.get_mut(&handle.0) {
            // Squared fade so the stroke thins early rather than lingering.
            let fade = (1.0 - k) * (1.0 - k);
            // Three-stop travel: birth -> an early flash -> death. A two-stop
            // lerp cannot describe the source's spike to white-pink at 200ms of
            // a 1233ms model, and dropping the flash loses the one thing that
            // makes Kidney Shot's colour read as a strike rather than a tint.
            let (from, to, t) = if k < CRESCENT_FLASH_AT {
                (flare.color, flare.color_mid, k / CRESCENT_FLASH_AT)
            } else {
                (
                    flare.color_mid,
                    flare.color_end,
                    (k - CRESCENT_FLASH_AT) / (1.0 - CRESCENT_FLASH_AT),
                )
            };
            let from = from.to_srgba();
            let to = to.to_srgba();
            material.base_color = Color::srgba(
                from.red + (to.red - from.red) * t,
                from.green + (to.green - from.green) * t,
                from.blue + (to.blue - from.blue) * t,
                (from.alpha + (to.alpha - from.alpha) * t) * fade,
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

    /// Alpha at a point in sprite space, both axes -1..1.
    fn alpha_at(img: &Image, sx: f32, sy: f32) -> u8 {
        let size = CRESCENT_PX as usize;
        let c = (size as f32 - 1.0) / 2.0;
        let x = (c + sx * c).round().clamp(0.0, (size - 1) as f32) as usize;
        let y = (c + sy * c).round().clamp(0.0, (size - 1) as f32) as usize;
        img.data.as_ref().unwrap()[(y * size + x) * 4 + 3]
    }

    #[test]
    fn the_streak_is_long_and_fine_not_a_pill() {
        // The defect this replaced: a circular arc cannot be both shallow and
        // long inside a square sprite, so flattening it to stop it reading as a
        // crescent moon also shortened it, and it rendered as a fat pink pill.
        // Length against thickness is the property that actually distinguishes a
        // slash, so assert it directly.
        let aspect = crescent_aspect();
        assert!(
            aspect > 6.0,
            "the stroke is {aspect}:1 — anything under about 6 reads as a blob"
        );
        assert!(aspect < 30.0, "{aspect}:1 is a hairline, not a slash");
    }

    #[test]
    fn the_streak_runs_the_length_of_the_sprite() {
        // It must actually reach toward both tips. The pill version occupied
        // barely half the sprite.
        let img = crescent_texture();
        // On the spine, which bows away from y = 0 at the middle.
        assert!(alpha_at(&img, 0.0, CRESCENT_BOW) > 200, "the belly is missing");
        assert!(
            alpha_at(&img, 0.7, CRESCENT_BOW * 0.51) > 60,
            "the stroke does not carry out toward its tips"
        );
        // And it comes to a point rather than ending square.
        assert!(alpha_at(&img, 0.99, 0.0) < 60, "the tip should fade out");
    }

    #[test]
    fn the_streak_is_fattest_at_its_belly() {
        // A constant-width band is a bar. Count the lit pixels down a column at
        // the middle and near a tip.
        let img = crescent_texture();
        let size = CRESCENT_PX as usize;
        let column = |sx: f32| -> usize {
            (0..size)
                .filter(|&i| {
                    let sy = i as f32 / (size - 1) as f32 * 2.0 - 1.0;
                    alpha_at(&img, sx, sy) > 40
                })
                .count()
        };
        let belly = column(0.0);
        let tip = column(0.85);
        assert!(belly > 0, "nothing lit at the belly at all");
        assert!(belly > tip, "belly {belly}px is not fatter than tip {tip}px");
    }

    #[test]
    fn the_streak_bows_but_stays_a_cut() {
        // Enough curve to read as a sweep, not so much that it becomes a moon.
        let bow = crescent_bow();
        assert!(bow > 0.05, "{bow} is a straight laser beam");
        assert!(bow < 0.45, "{bow} bows so far it reads as a crescent again");
    }

    #[test]
    fn the_halo_crescents_close_the_ring() {
        // `reach` and `size` have to move together. The crescents are placed
        // around a ring of radius `reach`, and each is `size` long, so if their
        // combined length falls short of the circumference the halo breaks into
        // disconnected dashes — which is not a ring, and nothing else in the
        // suite would notice. The reference shows them overlapping, with
        // hot-white blowouts where they cross.
        let spec = CHEAP_SHOT_CRESCENTS;
        let circumference = TAU * spec.reach;
        let covered = spec.count as f32 * spec.size;
        assert!(
            covered >= circumference,
            "{} crescents of {} only cover {covered:.2} of a {circumference:.2} \
             ring — the halo will read as dashes",
            spec.count,
            spec.size
        );
        // But not so long they wrap over each other into a solid disc.
        assert!(
            covered < circumference * 1.6,
            "the crescents overlap so far the ring fills in"
        );
    }

    #[test]
    fn cheap_shots_halo_is_magenta_not_white() {
        // The DB2 read said Cheap Shot's model has ZERO COLOUR TRACKS, and this
        // test originally asserted the crescents were untinted white as a
        // result. That was a misreading: no colour track means no ANIMATED
        // tint. The base texture is white-and-magenta, and the reference
        // screenshots sample #F8C8F8 fringing to #F0A0E8 against a purple, a
        // green and an orange ambient — so the magenta is intrinsic, not
        // borrowed from the scene.
        let c = CHEAP_SHOT_CRESCENTS.color.to_srgba();
        assert!(
            c.red > c.green + 0.1 && c.blue > c.green + 0.1,
            "Cheap Shot's halo should be magenta, got {c:?}"
        );
        // And it still passes through a white-hot flash, which is what the
        // overlapping crescents blow out to where they cross.
        let mid = CHEAP_SHOT_CRESCENTS.color_mid.to_srgba();
        assert!(
            mid.green > c.green,
            "the flash should be whiter than the body of the stroke"
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

}
