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
/// Width of the streak on the ground, in yards.
const HOJ_STREAK_WIDTH: f32 = 1.5;
/// Height above the floor. Same reasoning as the hard-CC ground pieces: the
/// arena floor is at y=0 with an identity transform, so this is a fixed world
/// height rather than a `rest_y`-derived offset.
const HOJ_GROUND_Y: f32 = 0.055;
/// Longest streak we will draw, in yards. Hammer of Justice's range is 10, and
/// a target beyond that cannot be hit — this only guards a stale position.
const HOJ_STREAK_MAX: f32 = 12.0;

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
const HOJ_RUNE_SIZE: f32 = 1.25;
/// Measured `#F1F950`.
const HOJ_RUNE_COLOR: Color = Color::srgba(0.945, 0.976, 0.314, 0.90);
const HOJ_RUNE_EMISSIVE: LinearRgba = LinearRgba::new(3.4, 3.6, 0.9, 1.0);
/// Turns the rune makes over its life. Slow — it is a seal settling onto the
/// victim, not a spinning prop.
const HOJ_RUNE_SPIN: f32 = 0.35;

const HOJ_TEX_PX: u32 = 128;

/// The streak's texture: bright at the LEADING edge, trailing off to nothing
/// behind, and soft across its width.
///
/// `u` runs 0 at the trailing end to 1 at the leading end. The asymmetry is the
/// whole point — a symmetric strip reads as a static bar, whereas a bright head
/// dragging a dimming tail reads as something travelling.
fn streak_texture() -> Image {
    use bevy::image::Image;
    use bevy::render::render_asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let size = HOJ_TEX_PX;
    let mut data = vec![0u8; (size * size * 4) as usize];

    for y in 0..size {
        for x in 0..size {
            let u = x as f32 / (size - 1) as f32;
            let v = y as f32 / (size - 1) as f32;

            // Along: ramps toward the head, with a hard spike in the last fifth.
            let ramp = u.powf(1.6);
            let head = (-(((u - 1.0) / 0.12).powi(2))).exp();
            let along = (ramp * 0.75 + head).clamp(0.0, 1.0);

            // Across: gaussian, narrowing toward the tail so the streak comes to
            // a point behind rather than ending in a blunt edge.
            let half_width = 0.16 + 0.20 * u;
            let d = (v - 0.5) / half_width;
            let across = (-(d * d)).exp();

            let a = (along * across).clamp(0.0, 1.0);
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

/// The rune's texture: a circular seal — two concentric rings with radial ticks
/// between them. Reads as a judgement mark rather than a generic glow, which is
/// what separates it from every other gold burst in the game.
fn rune_texture() -> Image {
    use bevy::image::Image;
    use bevy::render::render_asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    const OUTER_R: f32 = 0.86;
    const INNER_R: f32 = 0.52;
    const RING_W: f32 = 0.045;
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

            let a = (rings + ticks * 0.8).clamp(0.0, 1.0);
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
    streak_tex: &mut Option<Handle<Image>>,
    rune_tex: &mut Option<Handle<Image>>,
    caster_pos: Vec3,
    target_pos: Vec3,
) {
    let to_target = (target_pos - caster_pos).with_y(0.0);
    let distance = to_target.length().clamp(0.5, HOJ_STREAK_MAX);
    let Some(direction) = to_target.try_normalize() else {
        return;
    };

    // ---- The ground streak ----
    let tex = streak_tex
        .get_or_insert_with(|| images.add(streak_texture()))
        .clone();
    let material = materials.add(StandardMaterial {
        base_color: HOJ_ORANGE,
        base_color_texture: Some(tex.clone()),
        emissive: HOJ_STREAK_EMISSIVE,
        emissive_texture: Some(tex),
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    // The quad's +X runs along its length (matching the texture's `u`, which
    // ramps 0 at the tail to 1 at the head), so the yaw must align local +X with
    // the aim — `atan2(-dz, dx)`, the same convention `spawn_arena_walls`
    // documents for its length-along-+X cuboids (`play_match/mod.rs:459`).
    //
    // NOT the `atan2(dx, dz)` used for facing a UNIT toward something: that
    // aligns local +Z, which here is the quad's normal after the flat rotation,
    // and would lay the streak exactly ACROSS the line of fire.
    let yaw = (-direction.z).atan2(direction.x);
    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial3d(material),
        Transform::from_translation(caster_pos.with_y(HOJ_GROUND_Y))
            .with_rotation(Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-FRAC_PI_2)),
        HolyStreak {
            age: 0.0,
            length: distance,
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

        // Extend over the middle of the life, easing out so it decelerates into
        // the victim rather than snapping to full length.
        let travel = ((k - HOJ_STREAK_GROW_FROM) / (HOJ_STREAK_GROW_TO - HOJ_STREAK_GROW_FROM))
            .clamp(0.0, 1.0);
        let reach = (streak.length * travel.sqrt()).max(0.01);
        transform.scale = Vec3::new(reach, HOJ_STREAK_WIDTH, 1.0);
        // A Bevy `Rectangle` is CENTRED on its origin, so scaling local X alone
        // would span -reach/2..+reach/2 about the caster: the head would stop
        // halfway to the victim and the tail would run back through the
        // Paladin's own body. Walking the centre out by half the current reach
        // anchors the quad at its tail, so it grows forward only.
        transform.translation = streak.origin + streak.direction * (reach * 0.5);

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
    fn the_streak_is_brightest_at_its_leading_edge() {
        // A symmetric strip reads as a static bar. The whole reason this decal
        // says "travelling" is that its head is bright and its tail is not.
        let img = streak_texture();
        let size = HOJ_TEX_PX as usize;
        let mid = size / 2;
        let head = alpha_of(&img, size - 3, mid, size);
        let tail = alpha_of(&img, 2, mid, size);
        assert!(
            head > tail + 100,
            "head {head} should dominate tail {tail}"
        );
    }

    #[test]
    fn the_streak_tapers_across_its_width() {
        let img = streak_texture();
        let size = HOJ_TEX_PX as usize;
        let spine = alpha_of(&img, size - 3, size / 2, size);
        let edge = alpha_of(&img, size - 3, 2, size);
        assert!(spine > edge + 100, "the streak must have soft sides");
    }

    #[test]
    fn the_rune_is_a_ring_not_a_disc() {
        // A filled blob is every other gold burst in the game. The rune has to
        // read as a mark.
        let img = rune_texture();
        let size = HOJ_TEX_PX as usize;
        let c = size / 2;
        assert!(
            alpha_of(&img, c, c, size) < 40,
            "the rune's centre must be open"
        );
        // The outer ring, straight up from centre.
        let ring_y = c - (0.86 * c as f32) as usize + 1;
        assert!(
            alpha_of(&img, c, ring_y, size) > 150,
            "the outer ring should be solid"
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
