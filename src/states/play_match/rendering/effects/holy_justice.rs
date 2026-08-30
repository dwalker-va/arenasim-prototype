use bevy::color::LinearRgba;
use bevy::prelude::*;
use std::f32::consts::{FRAC_PI_2, TAU};

use crate::states::play_match::components::*;

// ==============================================================================
// Hammer of Justice — the streak and the rune
// ==============================================================================
//
// There is no hammer. The Classic client data is unambiguous: `HasMissile = 0`
// and `SpellVisualMissileSetID = 0`, so nothing is thrown, nothing flies, and no
// hammer geometry exists anywhere in the spell. Internally it is not even called
// Hammer — the assets are `fistofjustice_cast_base.m2` and
// `fistofjustice_impact_chest.m2`.
//
// What it actually is:
//
//   Caster side  A FLAT GROUND DECAL, 667ms. Every one of its 28 vertices sits
//                at z = 0.00-0.03. It is asymmetric, reaching ~4 units FORWARD
//                from the Paladin toward the target, and its elements translate
//                outward 2.84 units while scaling 7.2x between 100 and 600ms —
//                a gold streak racing along the ground. That travel is how a
//                10yd ability covers its range without a projectile.
//                Colour: #FF7E0C orange -> #FFFF0C yellow flash at 67ms ->
//                back to orange.
//   Target side  A golden rune (#F1F950) and a yellow starburst at the chest.
//
// So the Paladin's mace deliberately does NOT swing — `swing_style_for_ability`
// returns `None` for this ability on purpose. Swinging it would invent a weapon
// attack the spell does not have, and the source's own body animation is
// `SpecialUnarmed`, which our rig cannot express and which is not a weapon
// stroke either.
//
// ONE DELIBERATE DIVERGENCE. The source's streak is a fixed ~4 units regardless
// of where the victim stands; ours is scaled to the real caster-target distance
// so it arrives at the unit being stunned. At a 10yd range and this camera, a
// streak that stops a third of the way there reads as a misfire rather than a
// reach, and the streak is the only thing connecting the two units.
//
// Graphical-only, keyed on the `InstantAbilityFired` marker. No `game_rng` draw,
// no sim write — headless stays byte-identical.

/// Total life of the ground streak. Source: 667ms.
const HOJ_STREAK_SECS: f32 = 0.667;
/// Fraction of that life over which the streak extends. Source: it travels
/// between 100ms and 600ms of 667, i.e. the middle three-quarters — it does not
/// start moving instantly and it settles before it fades.
const HOJ_STREAK_GROW_FROM: f32 = 0.15;
const HOJ_STREAK_GROW_TO: f32 = 0.90;
/// Radius the caster's ground ring expands to, in yards.
///
/// The reference shows a flat golden ring sweeping out around the PALADIN'S OWN
/// FEET — not a streak travelling toward the victim. A directional streak
/// implies a projectile, and this spell has none (`HasMissile = 0`); the ring is
/// what the source's "elements translate outward while scaling 7.2x" actually
/// draws.
const HOJ_RING_RADIUS: f32 = 2.6;
/// Band width in yards. Kept SLIM — at 0.55 it was a fifth of its own radius
/// and read as a heavy static disc rather than a wave passing outward, which
/// also put it uncomfortably close to the selection ring's vocabulary (a
/// translucent torus at a unit's feet, `selection.rs`). The two are told apart
/// by this one being a brief bright sweep that expands and goes.
const HOJ_RING_THICKNESS: f32 = 0.26;
/// Rays blasting out of the rune at the victim's chest. The reference shows
/// long, thin, straight tapering spokes reaching well past the ring — they are
/// most of what the effect reads as, and the first version drew none.
const HOJ_RAY_COUNT: u32 = 7;
/// How far a ray reaches, as a multiple of the rune's own radius.
const HOJ_RAY_REACH: f32 = 3.1;
const HOJ_RAY_WIDTH: f32 = 0.030;
/// Height above the floor. Same reasoning as the hard-CC ground pieces: the
/// arena floor is at y=0 with an identity transform, so this is a fixed world
/// height rather than a `rest_y`-derived offset.
const HOJ_GROUND_Y: f32 = 0.055;

/// The source's measured keyframes: orange, flashing to near-white yellow very
/// early (67ms of 667 = 0.10 of its life), then settling back to orange.
const HOJ_ORANGE: Color = Color::srgba(1.00, 0.49, 0.05, 0.85);
const HOJ_FLASH: Color = Color::srgba(1.00, 1.00, 0.32, 0.95);
const HOJ_FLASH_AT: f32 = 0.10;
const HOJ_STREAK_EMISSIVE: LinearRgba = LinearRgba::new(3.2, 1.9, 0.4, 1.0);

/// The rune at the victim's chest. Source is 1734ms, shortened here: the stun
/// it announces lasts 6s, and a rune hanging for most of two seconds competes
/// with the whirl that is doing the actual work of saying "stunned".
const HOJ_RUNE_SECS: f32 = 1.05;
/// Height above the victim's SIM origin. A combatant's body centre is its sim
/// y, so the chest sits a little above it.
const HOJ_RUNE_HEIGHT: f32 = 0.35;
/// World size of the whole rune SPRITE, most of which is starburst. The ring
/// itself is `OUTER_R` of the half-width, so at 4.6 the ring lands ~1.4yd across
/// — about 1.5x the target's body, matching the reference — while the rays reach
/// well past it.
const HOJ_RUNE_SIZE: f32 = 4.6;
/// Measured `#F1F950`.
const HOJ_RUNE_COLOR: Color = Color::srgba(0.945, 0.976, 0.314, 0.90);
const HOJ_RUNE_EMISSIVE: LinearRgba = LinearRgba::new(3.4, 3.6, 0.9, 1.0);
/// Turns the rune makes over its life. Slow — it is a seal settling onto the
/// victim, not a spinning prop.
const HOJ_RUNE_SPIN: f32 = 0.35;

const HOJ_TEX_PX: u32 = 128;

/// The rune ring's outer radius as a fraction of its sprite's half-width.
/// Named at module scope so the starburst-clipping invariant can assert on it.
const OUTER_R_FOR_TEST: f32 = 0.30;

/// The rune's texture: a circular seal — two concentric rings with radial ticks
/// between them. Reads as a judgement mark rather than a generic glow, which is
/// what separates it from every other gold burst in the game.
fn rune_texture() -> Image {
    use bevy::image::Image;
    use bevy::render::render_asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    // The ring occupies only the middle THIRD of the sprite. That is not an
    // aesthetic choice: the rays reach `HOJ_RAY_REACH` times the ring's radius,
    // and a sprite only extends to 1.0 along its axes, so a ring at 0.86 left
    // the rays clipped to 48% of their length and entirely inside the ring's own
    // footprint — which is why the first version rendered as a small orange gear
    // with no starburst at all. The world size grows to compensate, so the RING
    // stays the same size on screen while the rays gain room.
    const OUTER_R: f32 = OUTER_R_FOR_TEST;
    const INNER_R: f32 = 0.18;
    const RING_W: f32 = 0.022;
    const TICKS: f32 = 8.0;
    const TICK_W: f32 = 0.20;

    let size = HOJ_TEX_PX;
    let mut data = vec![0u8; (size * size * 4) as usize];
    let centre = (size as f32 - 1.0) / 2.0;

    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 - centre) / centre;
            let dy = (y as f32 - centre) / centre;
            let r = (dx * dx + dy * dy).sqrt();
            let theta = dx.atan2(-dy);

            let ring = |target: f32| (-(((r - target) / RING_W).powi(2))).exp();
            let rings = ring(OUTER_R).max(ring(INNER_R));

            // Radial ticks living only in the gap between the two rings.
            let between = r > INNER_R && r < OUTER_R;
            let ticks = if between {
                // `.abs()` doubles a sine's frequency, so the half-angle is what
                // makes TICKS mean TICKS rather than twice as many. Without it
                // the rune drew 16 ticks while its constant said 8.
                let phase = (theta * TICKS * 0.5).sin().abs();
                if phase > 1.0 - TICK_W {
                    (phase - (1.0 - TICK_W)) / TICK_W
                } else {
                    0.0
                }
            } else {
                0.0
            };

            // Starburst: long straight tapering spokes reaching well past the
            // ring. In the reference these are most of what the effect reads
            // as, and the first version drew none — the rune alone is a quiet
            // disc where the real thing detonates.
            let mut rays: f32 = 0.0;
            if r > INNER_R * 0.35 {
                for k in 0..HOJ_RAY_COUNT {
                    let spoke = k as f32 / HOJ_RAY_COUNT as f32 * TAU + 0.21;
                    let mut off = (theta - spoke).abs();
                    if off > std::f32::consts::PI {
                        off = TAU - off;
                    }
                    // Narrow in angle, and narrowing further as it reaches out,
                    // so a spoke is a needle rather than a wedge.
                    let width = HOJ_RAY_WIDTH * (1.0 + 0.5 / (r + 0.35));
                    let across = (-((off / width).powi(2))).exp();
                    // Fade along the spoke, out to HOJ_RAY_REACH ring-radii.
                    let reach = (1.0 - r / (OUTER_R * HOJ_RAY_REACH)).clamp(0.0, 1.0);
                    rays = rays.max(across * reach.powf(0.7));
                }
            }
            // A white-hot core where the rays converge.
            let core = (-((r / 0.07).powi(2))).exp();

            let a = (rings + ticks * 0.8 + rays * 0.95 + core).clamp(0.0, 1.0);
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

/// Spawns both halves of a Hammer of Justice: the ground streak from the
/// Paladin toward its victim, and the rune that blooms on the victim's chest.
#[allow(clippy::too_many_arguments)]
pub fn spawn_holy_justice(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    rune_tex: &mut Option<Handle<Image>>,
    caster_pos: Vec3,
    target_pos: Vec3,
) {
    // Kept only so the ring's component can record which way the strike went;
    // the ring itself is radially symmetric.
    let Some(direction) = (target_pos - caster_pos).with_y(0.0).try_normalize() else {
        return;
    };

    // ---- The caster's ground ring ----
    //
    // A flat annulus sweeping out around the paladin's own feet. The first
    // version was a streak aimed at the victim, which implies a projectile this
    // spell does not have (`HasMissile = 0`) — the reference shows a ring.
    let material = materials.add(StandardMaterial {
        base_color: HOJ_ORANGE,
        emissive: HOJ_STREAK_EMISSIVE,
        alpha_mode: AlphaMode::Add,
        unlit: false,
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(
            Annulus::new(1.0 - HOJ_RING_THICKNESS / HOJ_RING_RADIUS, 1.0)
                .mesh()
                .resolution(64),
        )),
        MeshMaterial3d(material),
        Transform::from_translation(caster_pos.with_y(HOJ_GROUND_Y))
            .with_rotation(Quat::from_rotation_x(-FRAC_PI_2))
            .with_scale(Vec3::ZERO),
        HolyStreak {
            age: 0.0,
            length: HOJ_RING_RADIUS,
            direction,
            origin: caster_pos.with_y(HOJ_GROUND_Y),
        },
        PlayMatchEntity,
    ));

    // ---- The rune at the victim's chest ----
    let rtex = rune_tex
        .get_or_insert_with(|| images.add(rune_texture()))
        .clone();
    let rune_material = materials.add(StandardMaterial {
        base_color: HOJ_RUNE_COLOR,
        base_color_texture: Some(rtex.clone()),
        emissive: HOJ_RUNE_EMISSIVE,
        emissive_texture: Some(rtex),
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial3d(rune_material),
        Transform::from_translation(target_pos + Vec3::Y * HOJ_RUNE_HEIGHT)
            .with_scale(Vec3::ZERO),
        JusticeRune { age: 0.0 },
        PlayMatchEntity,
    ));
}

/// Races the streak out along the ground and fades it.
pub fn update_holy_streaks(
    time: Res<Time>,
    mut streaks: Query<(
        &mut HolyStreak,
        &mut Transform,
        &MeshMaterial3d<StandardMaterial>,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();

    for (mut streak, mut transform, handle) in streaks.iter_mut() {
        streak.age += dt;
        let k = (streak.age / HOJ_STREAK_SECS).clamp(0.0, 1.0);

        // Sweep outward over the middle of the life, easing out so the ring
        // decelerates rather than snapping to full size.
        let travel = ((k - HOJ_STREAK_GROW_FROM) / (HOJ_STREAK_GROW_TO - HOJ_STREAK_GROW_FROM))
            .clamp(0.0, 1.0);
        let radius = (streak.length * travel.sqrt()).max(0.01);
        // Uniform: the ring is a unit annulus, so one scale grows it in place.
        // It stays centred on the caster, which is why nothing has to walk its
        // origin the way the old aimed quad did.
        transform.scale = Vec3::splat(radius);
        transform.translation = streak.origin;

        if let Some(material) = materials.get_mut(&handle.0) {
            // Orange, flashing near-white very early, then back to orange.
            let (from, to, t) = if k < HOJ_FLASH_AT {
                (HOJ_ORANGE, HOJ_FLASH, k / HOJ_FLASH_AT)
            } else {
                (HOJ_FLASH, HOJ_ORANGE, (k - HOJ_FLASH_AT) / (1.0 - HOJ_FLASH_AT))
            };
            let a = from.to_srgba();
            let b = to.to_srgba();
            // Hold full brightness while it travels, then fade once it lands.
            let fade = if k < HOJ_STREAK_GROW_TO {
                1.0
            } else {
                let out = (k - HOJ_STREAK_GROW_TO) / (1.0 - HOJ_STREAK_GROW_TO);
                (1.0 - out) * (1.0 - out)
            };
            material.base_color = Color::srgba(
                a.red + (b.red - a.red) * t,
                a.green + (b.green - a.green) * t,
                a.blue + (b.blue - a.blue) * t,
                (a.alpha + (b.alpha - a.alpha) * t) * fade,
            );
            material.emissive = LinearRgba::new(
                HOJ_STREAK_EMISSIVE.red * fade,
                HOJ_STREAK_EMISSIVE.green * fade,
                HOJ_STREAK_EMISSIVE.blue * fade,
                1.0,
            );
        }
    }
}

/// Blooms, turns and fades the rune, billboarded at the victim's chest.
pub fn update_justice_runes(
    time: Res<Time>,
    camera: Query<&Transform, (With<Camera3d>, Without<JusticeRune>)>,
    mut runes: Query<(
        &mut JusticeRune,
        &mut Transform,
        &MeshMaterial3d<StandardMaterial>,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();
    let cam_rot = camera.iter().next().map(|t| t.rotation);

    for (mut rune, mut transform, handle) in runes.iter_mut() {
        rune.age += dt;
        let k = (rune.age / HOJ_RUNE_SECS).clamp(0.0, 1.0);

        // Snap open, hold, then widen slightly as it dissolves — a seal
        // releasing rather than shrinking away.
        let scale = if k < 0.18 {
            (k / 0.18).sqrt()
        } else {
            1.0 + 0.25 * ((k - 0.18) / 0.82)
        };
        transform.scale = Vec3::splat(HOJ_RUNE_SIZE * scale);

        if let Some(rot) = cam_rot {
            transform.rotation = rot * Quat::from_rotation_z(k * HOJ_RUNE_SPIN * TAU);
        }

        if let Some(material) = materials.get_mut(&handle.0) {
            let fade = (1.0 - k) * (1.0 - k);
            let base = HOJ_RUNE_COLOR.to_srgba();
            material.base_color = Color::srgba(base.red, base.green, base.blue, base.alpha * fade);
            material.emissive = LinearRgba::new(
                HOJ_RUNE_EMISSIVE.red * fade,
                HOJ_RUNE_EMISSIVE.green * fade,
                HOJ_RUNE_EMISSIVE.blue * fade,
                1.0,
            );
        }
    }
}

/// Despawns both halves once they have played out.
pub fn cleanup_holy_justice(
    mut commands: Commands,
    streaks: Query<(Entity, &HolyStreak)>,
    runes: Query<(Entity, &JusticeRune)>,
) {
    for (entity, streak) in streaks.iter() {
        if streak.age >= HOJ_STREAK_SECS {
            commands.entity(entity).despawn();
        }
    }
    for (entity, rune) in runes.iter() {
        if rune.age >= HOJ_RUNE_SECS {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha_of(img: &Image, x: usize, y: usize, size: usize) -> u8 {
        img.data.as_ref().unwrap()[(y * size + x) * 4 + 3]
    }

    #[test]
    fn the_starburst_reaches_past_the_ring() {
        // The bug this pins: the rays reach HOJ_RAY_REACH times the ring's
        // radius, but a sprite only extends to 1.0 along its axes. With the ring
        // at 0.86 the rays were clipped to under half their length and fell
        // entirely INSIDE the ring's own footprint — the rune rendered as a
        // small gear with no starburst whatsoever, and every existing test
        // passed because none of them looked outside the ring.
        assert!(
            OUTER_R_FOR_TEST * HOJ_RAY_REACH > 0.9,
            "the rays cannot even reach the sprite edge"
        );
        // And the ring must leave most of the sprite to the rays.
        assert!(
            OUTER_R_FOR_TEST < 0.45,
            "the ring fills the sprite, leaving the starburst nowhere to go"
        );
    }

    #[test]
    fn the_rune_throws_a_starburst() {
        // The reference detonates: long thin rays reaching well past the rune's
        // own ring. The first version drew only the ring and read as a quiet
        // disc. Sample straight out along a spoke, past the ring, and require
        // something to be there.
        let img = rune_texture();
        let size = HOJ_TEX_PX as usize;
        let c = (size as f32 - 1.0) / 2.0;
        let alpha_at = |sx: f32, sy: f32| -> u8 {
            let x = (c + sx * c).round().clamp(0.0, (size - 1) as f32) as usize;
            let y = (c + sy * c).round().clamp(0.0, (size - 1) as f32) as usize;
            img.data.as_ref().unwrap()[(y * size + x) * 4 + 3]
        };

        // Walk each spoke's own angle out past the ring and take the best.
        let mut best_far: u8 = 0;
        for k in 0..HOJ_RAY_COUNT {
            let a = k as f32 / HOJ_RAY_COUNT as f32 * TAU + 0.21;
            // Well beyond the ring, where only a ray can reach.
            let out = OUTER_R_FOR_TEST * 2.0;
            let (sx, sy) = (a.sin() * out, -a.cos() * out);
            best_far = best_far.max(alpha_at(sx, sy));
        }
        assert!(
            best_far > 50,
            "nothing reaches past the ring — the burst has no rays"
        );

        // And a hot core where they converge.
        assert!(alpha_at(0.0, 0.0) > 180, "the core should be near-opaque");
    }

    #[test]
    fn the_rays_are_spokes_not_a_disc() {
        // If the rays were wide enough to merge they would fill the sprite and
        // the shape would be a blob. Between two spokes, out past the ring,
        // there must be a genuine gap.
        let img = rune_texture();
        let size = HOJ_TEX_PX as usize;
        let c = (size as f32 - 1.0) / 2.0;
        let alpha_at = |sx: f32, sy: f32| -> u8 {
            let x = (c + sx * c).round().clamp(0.0, (size - 1) as f32) as usize;
            let y = (c + sy * c).round().clamp(0.0, (size - 1) as f32) as usize;
            img.data.as_ref().unwrap()[(y * size + x) * 4 + 3]
        };
        let step = TAU / HOJ_RAY_COUNT as f32;
        // Halfway between spoke 0 and spoke 1, out past the ring.
        let a = 0.21 + step * 0.5;
        let out = OUTER_R_FOR_TEST * 2.0;
        let (sx, sy) = (a.sin() * out, -a.cos() * out);
        assert!(
            alpha_at(sx, sy) < 40,
            "the gap between spokes is lit — the rays have merged into a disc"
        );
    }

    #[test]
    fn the_rune_is_a_seal_not_a_filled_disc() {
        // The reference has a white-hot core AND a ring of glyphs, with dark
        // between them — that gap is what makes it read as a seal rather than a
        // blob of light. (An earlier version of this test demanded an EMPTY
        // centre, which the reference contradicts: the core is one of the
        // brightest things in the frame.)
        let img = rune_texture();
        let size = HOJ_TEX_PX as usize;
        let c = (size as f32 - 1.0) / 2.0;
        let alpha_at = |sx: f32, sy: f32| -> u8 {
            let x = (c + sx * c).round().clamp(0.0, (size - 1) as f32) as usize;
            let y = (c + sy * c).round().clamp(0.0, (size - 1) as f32) as usize;
            img.data.as_ref().unwrap()[(y * size + x) * 4 + 3]
        };

        // The outer ring, straight up from centre, is solid. Sampled at the
        // ring's ACTUAL radius rather than a baked-in number — this assertion
        // silently moved off the ring when the proportions changed.
        assert!(
            alpha_at(0.0, -OUTER_R_FOR_TEST) > 150,
            "the outer ring should be solid"
        );

        // Between the core and the inner ring, off any spoke, it must go dark.
        let step = TAU / HOJ_RAY_COUNT as f32;
        let a = 0.21 + step * 0.5;
        // Between the core and the inner ring, off any spoke.
        let mid = OUTER_R_FOR_TEST * 0.45;
        let (sx, sy) = (a.sin() * mid, -a.cos() * mid);
        assert!(
            alpha_at(sx, sy) < 60,
            "no gap between the core and the ring — the rune is a filled disc"
        );
    }

    #[test]
    fn the_streak_settles_before_it_fades() {
        // It must reach full length while still bright, or the connection to
        // the victim is never actually drawn.
        assert!(
            HOJ_STREAK_GROW_TO < 1.0,
            "the streak has no time to land before it dies"
        );
        assert!(HOJ_STREAK_GROW_FROM < HOJ_STREAK_GROW_TO);
    }

    #[test]
    fn the_rune_outlasts_the_streak() {
        // Cause then consequence: the streak arrives, the rune holds after it.
        assert!(
            HOJ_RUNE_SECS > HOJ_STREAK_SECS,
            "the rune should still be there once the streak has gone"
        );
    }
}
